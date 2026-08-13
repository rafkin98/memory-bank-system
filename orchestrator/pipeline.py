#!/usr/bin/env python3
"""Memory Bank pipeline harness (Cursor SDK).

Runs the full Memory Bank development pipeline as a deterministic, verdict-routed
state machine. Each stage runs as its own Cursor agent with its own model, pinned
in the corresponding `.cursor/agents/mb-<stage>.md` file. Those files are the single
source of truth for both this harness and the in-IDE `/orchestrate` command, so the
two paths never drift.

Usage:
    export CURSOR_API_KEY="cursor_..."
    python orchestrator/pipeline.py --task "Add rate limiting to the public API"
    python orchestrator/pipeline.py --task-file spec.md      # read task from a file
    python orchestrator/pipeline.py < spec.md                # read task from stdin
    python orchestrator/pipeline.py                          # type it interactively

Options:
    --task TEXT        The task description. If omitted, the harness reads --task-file,
                       then stdin (piped or typed interactively).
    --task-file PATH   Read the task/brief from a file (e.g. a Markdown spec).
    --repo PATH        Repo root to run against (default: current directory).
    --max-loops N      Max remediation loops per failing edge before escalating (default: 3).
    --security         Enable both optional security stages (SCAN + PENTEST). Off by default.
    --scan             Enable only the optional SCAN stage.
    --pentest          Enable only the optional PENTEST stage (Level 3-4 only).
    --print-config     Print the stage -> model plan and exit (no agent calls).
    --dry-run          Resolve the route and print stages without calling the SDK.

By default SCAN and PENTEST do NOT run. Opt in with --security (both), or --scan /
--pentest individually.
"""
from __future__ import annotations

import argparse
import os
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

# ---------------------------------------------------------------------------
# Pipeline definition
# ---------------------------------------------------------------------------

# Base forward route per complexity level, WITHOUT the optional security stages.
# SCAN and PENTEST are opt-in and injected by build_route() when enabled.
# Verdict-based back-edges are applied on top of these at runtime (see route_on_fail).
BASE_ROUTES: dict[int, list[str]] = {
    1: ["van", "build", "reflect"],
    2: ["van", "plan", "build", "judge", "reflect"],
    3: ["van", "plan", "creative", "build", "judge",
        "integrate", "validate", "reflect"],
    4: ["van", "plan", "creative", "build", "judge",
        "integrate", "validate", "reflect", "archive"],
}


def build_route(level: int, scan: bool, pentest: bool) -> list[str]:
    """Resolve the stage route for a level, injecting optional security stages.

    SCAN runs immediately before JUDGE (right after BUILD). PENTEST runs right
    after VALIDATE (Level 3-4 only, since lower levels have no VALIDATE stage).
    """
    route = list(BASE_ROUTES[level])
    if scan and "judge" in route:
        route.insert(route.index("judge"), "scan")
    if pentest and "validate" in route:
        route.insert(route.index("validate") + 1, "pentest")
    return route

# Output file each verdict-bearing stage writes, relative to memory-bank/.
STAGE_OUTPUT: dict[str, str] = {
    "van": "projectbrief.md",
    "scan": "security/scan-latest.md",
    "judge": "review/review-latest.md",
    "integrate": "integration/integration-latest.md",
    "validate": "validation/validation-latest.md",
    "pentest": "security/pentest-latest.md",
}

# Stages whose verdict can send us backward.
VERDICT_STAGES = {"scan", "judge", "integrate", "validate", "pentest"}

MAX_LOOPS_DEFAULT = 3

# --- Cost estimation -------------------------------------------------------
# Approximate USD prices per 1,000,000 tokens, keyed by base model slug.
#
# ⚠ PLACEHOLDER VALUES — EDIT THESE. As of 2026-07-31 Cursor reports usage in
# tokens only and no longer returns per-request dollar costs, so any dollar
# figure here is a LOCAL ESTIMATE, not something Cursor bills you. Replace with
# the real prices for your plan (and note launch promos change them). Set a
# model to None to mark its price as unknown (its tokens are excluded from the
# estimate and flagged).
PRICE_TABLE: dict[str, Optional[dict[str, float]]] = {
    # model_base: {input, output, cache_read, cache_write}  ($ per 1M tokens)
    "claude-opus-5": {"input": 15.0, "output": 75.0, "cache_read": 1.50, "cache_write": 18.75},
    "gpt-5.6-sol":   {"input": 1.25, "output": 10.0, "cache_read": 0.13, "cache_write": 1.25},
    "composer-2.5":  {"input": 1.25, "output": 10.0, "cache_read": 0.13, "cache_write": 1.25},
}

# Token accumulator field order.
_TOK_FIELDS = ("input_tokens", "output_tokens", "cache_read_tokens",
               "cache_write_tokens", "total_tokens")


def _empty_acc() -> dict[str, int]:
    return {f: 0 for f in _TOK_FIELDS}


def add_usage(acc: dict[str, int], usage) -> None:
    """Add an SDK TokenUsage (or None) into an accumulator dict."""
    if usage is None:
        return
    for f in _TOK_FIELDS:
        acc[f] += int(getattr(usage, f, 0) or 0)


def fmt_tokens(n: int) -> str:
    if n >= 1_000_000:
        return f"{n / 1_000_000:.1f}M"
    if n >= 1_000:
        return f"{n / 1_000:.1f}k"
    return str(n)


def estimate_cost(model_base: str, acc: dict[str, int]) -> Optional[float]:
    """Estimate USD cost for one model's token totals. None if price unknown."""
    price = PRICE_TABLE.get(model_base)
    if not price:
        return None
    return (
        acc["input_tokens"] / 1e6 * price["input"]
        + acc["output_tokens"] / 1e6 * price["output"]
        + acc["cache_read_tokens"] / 1e6 * price["cache_read"]
        + acc["cache_write_tokens"] / 1e6 * price["cache_write"]
    )


@dataclass
class StageDef:
    name: str
    model: str          # verbatim from frontmatter (may include [params])
    model_base: str     # slug with [params] stripped, passed to the SDK
    prompt: str         # subagent body (instructions)


@dataclass
class RunState:
    task: str
    level: int
    loop_counts: dict[str, int] = field(default_factory=dict)
    pending_context: dict[str, str] = field(default_factory=dict)
    build_judge_iterations: int = 0


# ---------------------------------------------------------------------------
# Loading stage definitions from .cursor/agents/mb-*.md
# ---------------------------------------------------------------------------

_FRONTMATTER_RE = re.compile(r"^---\s*\n(.*?)\n---\s*\n(.*)$", re.DOTALL)
_MODEL_RE = re.compile(r"^\s*model:\s*(.+?)\s*$", re.MULTILINE)


def load_stage(repo: Path, name: str) -> StageDef:
    path = repo / ".cursor" / "agents" / f"mb-{name}.md"
    if not path.exists():
        raise FileNotFoundError(f"Missing subagent file: {path}")
    text = path.read_text(encoding="utf-8")
    m = _FRONTMATTER_RE.match(text)
    if not m:
        raise ValueError(f"{path} has no YAML frontmatter")
    frontmatter, body = m.group(1), m.group(2).strip()
    model_match = _MODEL_RE.search(frontmatter)
    model = model_match.group(1).strip() if model_match else "inherit"
    model_base = re.sub(r"\[.*?\]", "", model).strip()
    return StageDef(name=name, model=model, model_base=model_base, prompt=body)


def load_all_stages(repo: Path) -> dict[str, StageDef]:
    names = ["van", "plan", "creative", "build", "scan", "judge",
             "integrate", "validate", "pentest", "reflect", "archive"]
    return {n: load_stage(repo, n) for n in names}


# ---------------------------------------------------------------------------
# Verdict parsing + routing
# ---------------------------------------------------------------------------

_VERDICT_RE = re.compile(r"^##\s*Verdict:\s*([A-Za-z_]+)", re.MULTILINE)
_ROUTE_TO_RE = re.compile(r"Route to:\s*([A-Za-z]+)", re.IGNORECASE)
_FAIL_TYPE_RE = re.compile(r"(?:Failure type|Type):\s*([A-Za-z_]+)", re.IGNORECASE)
_LEVEL_RE = re.compile(r"Level:\s*([1-4])")


def parse_verdict(text: str) -> str:
    m = _VERDICT_RE.search(text)
    return m.group(1).strip().upper() if m else "UNKNOWN"


def parse_level(text: str) -> Optional[int]:
    m = _LEVEL_RE.search(text)
    return int(m.group(1)) if m else None


def route_on_fail(stage: str, report: str) -> str:
    """Return the stage name to jump back to for a FAIL verdict."""
    route_to = _ROUTE_TO_RE.search(report)
    fail_type = _FAIL_TYPE_RE.search(report)
    if route_to:
        target = route_to.group(1).lower()
        if target in ("build", "judge", "integrate"):
            return target
    if fail_type:
        t = fail_type.group(1).lower()
        if t in ("code_bug", "build_errors"):
            return "build"
        if t == "config_issue":
            return "integrate"
        if t in ("quality_issue", "quality_issues"):
            return "judge"
        if t == "integration_issue":
            return "integrate"
    # Sensible defaults per stage.
    return {"scan": "build", "judge": "build", "integrate": "build",
            "validate": "build", "pentest": "build"}[stage]


# ---------------------------------------------------------------------------
# Agent execution (Cursor SDK)
# ---------------------------------------------------------------------------

def run_stage_agent(stage: StageDef, repo: Path, task: str,
                    extra_context: str, api_key: Optional[str]):
    """Run one stage as a one-shot Cursor agent. Raises on hard failure.

    Returns the run's TokenUsage (or None if the runtime didn't report it).
    """
    from cursor_sdk import Agent, AgentOptions, LocalAgentOptions, CursorAgentError

    prompt = (
        f"{stage.prompt}\n\n"
        f"---\n"
        f"TASK: {task}\n"
        f"CURRENT STAGE: {stage.name.upper()}\n"
    )
    if extra_context:
        prompt += (
            f"\nREMEDIATION CONTEXT (fix these findings this pass):\n{extra_context}\n"
        )

    opts = AgentOptions(
        # model string is passed with [params] stripped for SDK compatibility.
        model=stage.model_base,
        local=LocalAgentOptions(cwd=str(repo)),
    )
    if api_key:
        opts.api_key = api_key

    try:
        result = Agent.prompt(prompt, opts)
    except CursorAgentError as err:  # never started: auth/config/network
        raise RuntimeError(
            f"[{stage.name}] agent failed to start: {err} "
            f"(retryable={getattr(err, 'is_retryable', '?')})"
        ) from err

    status = getattr(result, "status", "unknown")
    if status == "error":  # started but failed mid-run
        raise RuntimeError(
            f"[{stage.name}] agent run failed (id={getattr(result, 'id', '?')})"
        )

    return getattr(result, "usage", None)


# ---------------------------------------------------------------------------
# Orchestration loop
# ---------------------------------------------------------------------------

def read_output(repo: Path, stage: str) -> str:
    rel = STAGE_OUTPUT.get(stage)
    if not rel:
        return ""
    path = repo / "memory-bank" / rel
    return path.read_text(encoding="utf-8") if path.exists() else ""


def progress(msg: str) -> None:
    print(f"  {msg}", flush=True)


def print_stage_tokens(name: str, usage) -> None:
    if usage is None:
        progress(f"[{name.upper()}] tokens: not reported by runtime")
        return
    progress(
        f"[{name.upper()}] tokens: in {fmt_tokens(getattr(usage, 'input_tokens', 0))}, "
        f"out {fmt_tokens(getattr(usage, 'output_tokens', 0))}, "
        f"cache {fmt_tokens(getattr(usage, 'cache_read_tokens', 0))}"
        f"/{fmt_tokens(getattr(usage, 'cache_write_tokens', 0))}, "
        f"total {fmt_tokens(getattr(usage, 'total_tokens', 0))}"
    )


def print_usage_summary(usage_by_model: dict[str, dict[str, int]],
                        stages_by_model: dict[str, int]) -> None:
    if not usage_by_model:
        return
    print("\n── Usage summary ──────────────────────────────")
    grand = _empty_acc()
    total_cost = 0.0
    any_unknown = False
    for model in sorted(usage_by_model):
        acc = usage_by_model[model]
        for f in _TOK_FIELDS:
            grand[f] += acc[f]
        cost = estimate_cost(model, acc)
        if cost is None:
            any_unknown = True
            cost_str = "no price set"
        else:
            total_cost += cost
            cost_str = f"~${cost:.2f}"
        print(f"  {model:<26} {stages_by_model[model]:>2} run(s)  "
              f"{fmt_tokens(acc['total_tokens']):>7} tok   {cost_str}")
    print(f"  {'TOTAL':<26} {sum(stages_by_model.values()):>2} run(s)  "
          f"{fmt_tokens(grand['total_tokens']):>7} tok   ~${total_cost:.2f}")
    print("  (in {i}, out {o}, cache {cr}/{cw})".format(
        i=fmt_tokens(grand["input_tokens"]), o=fmt_tokens(grand["output_tokens"]),
        cr=fmt_tokens(grand["cache_read_tokens"]), cw=fmt_tokens(grand["cache_write_tokens"])))
    print("  ⚠ Cost is a LOCAL ESTIMATE from PRICE_TABLE (placeholder prices — "
          "edit them). Cursor reports tokens only, not dollars.")
    if any_unknown:
        print("  ⚠ Some models had no price set; their tokens are excluded from the $ estimate.")


def run_pipeline(repo: Path, task: str, max_loops: int,
                 api_key: Optional[str], dry_run: bool,
                 scan: bool, pentest: bool) -> int:
    stages = load_all_stages(repo)

    # Token usage accumulated per model slug across the whole run.
    usage_by_model: dict[str, dict[str, int]] = {}
    stages_by_model: dict[str, int] = {}

    def track(stage: StageDef, usage) -> None:
        acc = usage_by_model.setdefault(stage.model_base, _empty_acc())
        add_usage(acc, usage)
        stages_by_model[stage.model_base] = stages_by_model.get(stage.model_base, 0) + 1

    # --- VAN first, to learn the complexity level ---
    print(f"[VAN] running on {stages['van'].model} ...", flush=True)
    if not dry_run:
        van_usage = run_stage_agent(stages["van"], repo, task, "", api_key)
        track(stages["van"], van_usage)
        print_stage_tokens("van", van_usage)
        brief = read_output(repo, "van")
        level = parse_level(brief)
        if level is None:
            print("ERROR: VAN did not write a parseable 'Level: [N]' line "
                  "in memory-bank/projectbrief.md", file=sys.stderr)
            return 1
    else:
        level = 3  # assume L3 for a dry run so the full route prints
    progress(f"[VAN] Complete — Level {level} assessed")

    route = build_route(level, scan, pentest)
    sec = [s for s in ("scan", "pentest") if s in route]
    sec_note = f" (security: {', '.join(sec)})" if sec else " (security: off)"
    print(f"\nRoute (Level {level}){sec_note}: "
          f"{' -> '.join(s.upper() for s in route)}\n")

    state = RunState(task=task, level=level)

    # VAN already ran; start after it.
    i = 1
    while i < len(route):
        name = route[i]
        stage = stages[name]
        extra = state.pending_context.pop(name, "")

        print(f"[{name.upper()}] running on {stage.model} ...", flush=True)
        if dry_run:
            i += 1
            continue

        usage = run_stage_agent(stage, repo, task, extra, api_key)
        track(stage, usage)
        print_stage_tokens(name, usage)
        if name == "build":
            state.build_judge_iterations += 1

        # Non-verdict stages just advance.
        if name not in VERDICT_STAGES:
            progress(f"[{name.upper()}] Complete")
            i += 1
            continue

        report = read_output(repo, name)
        verdict = parse_verdict(report)

        if verdict in ("PASS", "CONDITIONAL"):
            progress(f"[{name.upper()}] Complete — {verdict}")
            i += 1
            continue

        if verdict == "FAIL":
            target = route_on_fail(name, report)
            edge = f"{name}->{target}"
            state.loop_counts[edge] = state.loop_counts.get(edge, 0) + 1
            progress(f"[{name.upper()}] FAIL — looping back to {target.upper()} "
                     f"(attempt {state.loop_counts[edge]}/{max_loops})")
            if state.loop_counts[edge] > max_loops:
                print(f"\nESCALATION: {edge} exceeded {max_loops} remediation loops. "
                      f"Stopping so a human can intervene. "
                      f"See memory-bank/{STAGE_OUTPUT[name]}", file=sys.stderr)
                print_usage_summary(usage_by_model, stages_by_model)
                return 2
            # Carry the failing report into the remediation stage.
            state.pending_context[target] = report
            i = route.index(target)
            continue

        # Unknown / unparseable verdict — fail loudly rather than silently pass.
        print(f"\nERROR: could not parse a verdict from "
              f"memory-bank/{STAGE_OUTPUT[name]} (got '{verdict}').",
              file=sys.stderr)
        return 1

    print(f"\nPipeline complete (Level {level}). "
          f"Build iterations: {state.build_judge_iterations}. "
          f"Loop edges: {dict(state.loop_counts) or 'none'}.")
    print("Artifacts are in memory-bank/.")
    print_usage_summary(usage_by_model, stages_by_model)
    return 0


# ---------------------------------------------------------------------------
# CLI
# ---------------------------------------------------------------------------

def print_config(repo: Path) -> int:
    stages = load_all_stages(repo)
    print("Stage -> model (from .cursor/agents/mb-*.md):\n")
    for name, s in stages.items():
        note = "" if s.model == s.model_base else f"  (SDK uses: {s.model_base})"
        print(f"  {name:<10} {s.model}{note}")
    print("\nLevel routes (default — security stages OFF):")
    for lvl, route in BASE_ROUTES.items():
        print(f"  L{lvl}: {' -> '.join(x.upper() for x in route)}")
    print("\nWith --security (SCAN + PENTEST injected):")
    for lvl in BASE_ROUTES:
        route = build_route(lvl, scan=True, pentest=True)
        print(f"  L{lvl}: {' -> '.join(x.upper() for x in route)}")

    print("\nPrice table ($/1M tokens — PLACEHOLDERS, edit PRICE_TABLE):")
    for model in sorted(PRICE_TABLE):
        pr = PRICE_TABLE[model]
        if pr:
            print(f"  {model:<26} in {pr['input']}, out {pr['output']}, "
                  f"cache_read {pr['cache_read']}, cache_write {pr['cache_write']}")
        else:
            print(f"  {model:<26} (no price set)")
    print("  Note: Cursor reports tokens only; dollar figures are local estimates.")
    return 0


def resolve_task(task: Optional[str], task_file: Optional[str]) -> str:
    """Resolve the task text from --task, then --task-file, then stdin.

    Precedence: explicit --task wins; else --task-file; else stdin (piped input,
    or typed interactively when attached to a TTY). Returns "" if none provided.
    """
    if task:
        return task.strip()
    if task_file:
        text = Path(task_file).read_text(encoding="utf-8").strip()
        if not text:
            raise ValueError(f"--task-file {task_file} is empty")
        return text
    if sys.stdin.isatty():
        print("Enter the task (finish with Ctrl-D on a blank line):", file=sys.stderr, flush=True)
    return sys.stdin.read().strip()


def main(argv: Optional[list[str]] = None) -> int:
    parser = argparse.ArgumentParser(description="Memory Bank pipeline harness (Cursor SDK)")
    parser.add_argument("--task", help="Task description (else --task-file, else stdin)")
    parser.add_argument("--task-file", help="Read the task/brief from a file (e.g. a Markdown spec)")
    parser.add_argument("--repo", default=".", help="Repo root (default: cwd)")
    parser.add_argument("--max-loops", type=int, default=MAX_LOOPS_DEFAULT,
                        help=f"Max remediation loops per edge (default: {MAX_LOOPS_DEFAULT})")
    parser.add_argument("--security", action="store_true",
                        help="Enable both optional security stages (SCAN + PENTEST)")
    parser.add_argument("--scan", action="store_true",
                        help="Enable the optional SCAN stage (static security analysis)")
    parser.add_argument("--pentest", action="store_true",
                        help="Enable the optional PENTEST stage (Level 3-4 only)")
    parser.add_argument("--print-config", action="store_true",
                        help="Print stage->model plan and exit")
    parser.add_argument("--dry-run", action="store_true",
                        help="Resolve the route and print stages without calling the SDK")
    args = parser.parse_args(argv)

    repo = Path(args.repo).resolve()
    if not (repo / ".cursor" / "agents").exists():
        print(f"ERROR: {repo}/.cursor/agents not found. Run from the repo root "
              f"or pass --repo.", file=sys.stderr)
        return 1

    if args.print_config:
        return print_config(repo)

    try:
        task = resolve_task(args.task, args.task_file)
    except (OSError, ValueError) as err:
        print(f"ERROR: {err}", file=sys.stderr)
        return 1
    if not task:
        parser.error("no task provided (use --task, --task-file, or pipe/type via stdin)")

    api_key = os.environ.get("CURSOR_API_KEY")
    if not api_key and not args.dry_run:
        print("WARNING: CURSOR_API_KEY is not set; the SDK will look for its own "
              "default credentials and may fail.", file=sys.stderr)

    scan_enabled = args.security or args.scan
    pentest_enabled = args.security or args.pentest

    try:
        return run_pipeline(repo, task, args.max_loops, api_key,
                            args.dry_run, scan_enabled, pentest_enabled)
    except FileNotFoundError as err:
        print(f"ERROR: {err}", file=sys.stderr)
        return 1
    except RuntimeError as err:
        print(f"ERROR: {err}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    raise SystemExit(main())
