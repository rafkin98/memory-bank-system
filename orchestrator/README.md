# Memory Bank Pipeline Harness

A deterministic, verdict-routed orchestrator for the Memory Bank pipeline. It runs each
stage (VAN → PLAN → CREATIVE → BUILD → SCAN → JUDGE → INTEGRATE → VALIDATE → PENTEST →
REFLECT → ARCHIVE) as its own Cursor agent, **each on its own model**, and routes
between stages based on the `## Verdict:` lines the stages write into `memory-bank/`.

This is the "harness" counterpart to the in-IDE `/orchestrate` command. All of them read
the **same** `.cursor/agents/mb-*.md` files, so stage prompts and per-stage models stay in
sync. Use a harness when you want guaranteed per-stage models, deterministic loop/retry
control, or CI-runnable automation; use `/orchestrate` for an interactive run inside Cursor.

## Three interchangeable implementations

All three implement the **identical** state machine (routes, verdict parsing, remediation
loops, token/cost reporting) over the same source of truth. Pick by ecosystem:

| Impl | Entry point | Drives agents via | Extra prereqs |
| :--- | :--- | :--- | :--- |
| **Python** | `pipeline.py` | [Cursor Python SDK](https://cursor.com/docs/sdk/python) (`Agent.prompt`) | `pip install -r requirements.txt` |
| **Bash** | `orchestrate.sh` | `cursor-agent` CLI + `jq` | `cursor-agent`, `jq` |
| **Rust** | `rust/` (`cargo run`) | `cursor-agent` CLI (`std::process`) | `cargo`, `cursor-agent` |

The Bash and Rust harnesses are thin wrappers around the headless
[`cursor-agent` CLI](https://cursor.com/docs/cli) (`cursor-agent -p --output-format json`),
since there is no first-party Rust SDK. They behave the same as the Python harness; the
one difference is token reporting (see [Token usage](#token-usage--cost-estimate)).

## Single source of truth

| File | Provides |
| :--- | :--- |
| `.cursor/agents/mb-<stage>.md` (frontmatter `model:`) | The model each stage runs on |
| `.cursor/agents/mb-<stage>.md` (body) | The stage's instructions / prompt |
| `pipeline.py` / `orchestrate.sh` / `rust/src/main.rs` | Routing by complexity level + verdict-based remediation loops |

To change a stage's model or prompt, edit its `.cursor/agents/mb-<stage>.md` file — all
three harnesses and the in-IDE command pick it up.

## Install (Python)

```bash
python -m venv .venv && source .venv/bin/activate
pip install -r orchestrator/requirements.txt
export CURSOR_API_KEY="cursor_..."   # from cursor.com/dashboard/integrations
```

## Run (Python)

```bash
# Run the pipeline against the current repo (security stages OFF by default)
python orchestrator/pipeline.py --task "Add rate limiting to the public API"

# Three ways to supply the task (precedence: --task > --task-file > stdin)
python orchestrator/pipeline.py --task-file spec.md    # read from a Markdown file
python orchestrator/pipeline.py < spec.md              # pipe from stdin
python orchestrator/pipeline.py                        # type it, then Ctrl-D

# Opt into the security stages: SCAN + PENTEST
python orchestrator/pipeline.py --task "..." --security
# ...or just one of them
python orchestrator/pipeline.py --task "..." --scan
python orchestrator/pipeline.py --task "..." --pentest

# Inspect the stage -> model plan and both route variants (no agent calls)
python orchestrator/pipeline.py --print-config

# Resolve the route and print stages without spending tokens
python orchestrator/pipeline.py --task "..." --dry-run          # default (no security)
python orchestrator/pipeline.py --task "..." --security --dry-run

# Run against a different repo, allow more remediation loops
python orchestrator/pipeline.py --repo /path/to/project --task "..." --max-loops 5
```

The harness runs agents **locally** (`local.cwd = repo root`), so they read and edit the
real files in your working tree, exactly like the in-IDE agents.

## Run (Bash)

Requires the `cursor-agent` CLI (logged in via `cursor-agent login`, or `CURSOR_API_KEY`
set) and `jq`. Same flags as the Python harness.

```bash
# Same interface, driven by the cursor-agent CLI
orchestrator/orchestrate.sh --task "Add rate limiting to the public API"
orchestrator/orchestrate.sh --task-file spec.md              # or:  ... < spec.md
orchestrator/orchestrate.sh --task "..." --security          # SCAN + PENTEST
orchestrator/orchestrate.sh --print-config                   # stage -> model plan
orchestrator/orchestrate.sh --task "..." --dry-run --security # resolve route, no calls
orchestrator/orchestrate.sh --repo /path/to/project --task "..." --max-loops 5
```

Each stage becomes one headless run: `cursor-agent -p --model <slug> --output-format json
--force --trust --workspace <repo> "<prompt>"`. Edit the `PRICE` associative array near the
top of the script to set your prices.

## Run (Rust)

Requires `cargo` and the `cursor-agent` CLI. The crate is dependency-light (only
`serde_json`) and wraps the same CLI.

```bash
cd orchestrator/rust
cargo build --release                       # binary at target/release/mb-orchestrator

# Same interface (run against the repo root two levels up, or pass --repo)
cargo run -- --print-config --repo ../..
cargo run -- --task "Add rate limiting" --repo ../..
cargo run -- --task-file spec.md --repo ../..     # or:  ... --repo ../.. < spec.md
cargo run -- --task "..." --security --repo ../..
cargo run -- --task "..." --dry-run --repo ../..
```

Edit the `price()` function in `rust/src/main.rs` to set your prices.

## How routing works

1. **VAN** runs first and writes `memory-bank/projectbrief.md`. The harness parses the
   `Level: [N]` line to choose the route (Level 1–4).
2. **SCAN and PENTEST are optional and off by default.** Enable them with `--security`
   (both), or `--scan` / `--pentest` individually. When enabled, SCAN is injected right
   before JUDGE and PENTEST right after VALIDATE (Level 3-4 only).
3. Each stage runs in order. After a verdict stage (SCAN, JUDGE, INTEGRATE, VALIDATE,
   PENTEST) the harness reads its report and parses `## Verdict:`.
   - `PASS` / `CONDITIONAL` → advance.
   - `FAIL` → jump back to the remediation stage (BUILD by default, or the stage named in
     `Route to:` / inferred from `Failure type:`), carrying the failing report forward as
     remediation context.
4. Each failing edge (e.g. `judge->build`) is capped at `--max-loops` (default 3). Exceeding
   the cap stops the run with a non-zero exit code so a human can step in.

## Token usage & cost estimate

After each stage the harness prints that run's token usage, and at the end it prints a
per-model summary with a total:

```
[JUDGE] running on claude-opus-5[effort=high] ...
  [JUDGE] tokens: in 12.4k, out 3.1k, cache 8.0k/500, total 24.0k

── Usage summary ──────────────────────────────
  claude-opus-5    2 run(s)   157.5k tok   ~$2.89
  composer-2.5     1 run(s)    83.0k tok   ~$0.27
  gpt-5.6-sol      1 run(s)    35.0k tok   ~$0.09
  TOTAL            4 run(s)   275.5k tok   ~$3.25
```

- **Tokens** come from the runtime's usage report (`input`/`output`/`cache_read`/
  `cache_write`). The **Python** harness reads the SDK's `result.usage`. The **Bash/Rust**
  harnesses parse the `usage` object from the CLI's `--output-format json`, which is
  **best-effort** — the CLI does not always include a `usage` block, in which case the stage
  prints "tokens: not reported by runtime" and contributes 0 to the summary.
- **Dollar cost is a LOCAL ESTIMATE.** As of 2026-07-31 Cursor reports usage in **tokens only**
  and no longer returns per-request dollar costs, so the `$` figures come from the price table
  in each harness (`PRICE_TABLE` in `pipeline.py`, the `PRICE` array in `orchestrate.sh`, the
  `price()` fn in `main.rs`) — **placeholder prices you must edit** for your plan. Models with
  no price set are counted in tokens but excluded from the `$` total. For authoritative billed
  cost, Team/Enterprise admins can reconcile via the Cursor Admin API
  (`POST /teams/filtered-usage-events`, which flags `isHeadless` calls). Run `--print-config`
  to see the current table.

## Exit codes

| Code | Meaning |
| :--- | :--- |
| `0` | Pipeline completed |
| `1` | Startup / config error, or an unparseable verdict / missing `Level:` line |
| `2` | A stage agent failed to run, or a remediation edge exceeded `--max-loops` |

## Per-stage model defaults

Defaults are chosen so that **review stages run on a different model family than BUILD**,
which reduces correlated blind spots between the code author and its reviewers.

| Stage | Default model | Rationale |
| :--- | :--- | :--- |
| VAN | `gpt-5.6-sol` | Fast reasoning for triage + complexity routing |
| PLAN | `claude-opus-5[effort=high]` | Strongest architectural planning |
| CREATIVE | `claude-opus-5[effort=high]` | Deep design trade-off exploration |
| BUILD | `composer-2.5` | Fast, strong code generation / edits |
| SCAN | `gpt-5.6-sol` | Independent static security review |
| JUDGE | `claude-opus-5[effort=high]` | Rigorous, adversarial code review |
| INTEGRATE | `composer-2.5` | Mechanical build/test execution |
| VALIDATE | `gpt-5.6-sol` | Acceptance-criteria verification |
| PENTEST | `claude-opus-5[effort=high]` | Attacker-mindset dynamic testing |
| REFLECT | `gpt-5.6-sol` | Synthesis, mid cost |
| ARCHIVE | `composer-2.5` | Mechanical compilation |

These are starting points — tweak the `model:` field in any `.cursor/agents/mb-*.md`.

## Caveats

- **Model slugs evolve and are account-specific.** Verify availability with the Cursor
  model picker, `cursor-agent models`, or `Cursor.models.list()`. If a slug errors, swap it
  in the agent file.
- **Bracket params** (e.g. `[effort=high]`) are honored in-IDE only. The SDK and the
  `cursor-agent` CLI's `--model` take a plain slug, so **all three harnesses strip the
  brackets** and pass the base slug (`--print-config` shows both). Set effort via the agent
  frontmatter for the in-IDE path.
- **CLI auth (Bash/Rust):** the `cursor-agent` CLI must be authenticated (`cursor-agent
  login` or `CURSOR_API_KEY`). Headless runs use `--force --trust` so stages can edit files
  without interactive approval — run them on a branch / in CI, not on unsaved work you can't
  discard.
- **Plan restrictions:** on legacy request-based plans without Max Mode, Cursor may fall
  back to Composer regardless of the configured model. Usage-based plans / Max Mode honor
  per-stage models.
