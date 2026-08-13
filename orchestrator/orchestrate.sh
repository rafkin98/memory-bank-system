#!/usr/bin/env bash
#
# Memory Bank pipeline harness (Bash / cursor-agent CLI).
#
# A deterministic, verdict-routed orchestrator that drives each Memory Bank stage
# as its own headless `cursor-agent` run, each on the model pinned in the matching
# .cursor/agents/mb-<stage>.md file. Those files are the single source of truth
# shared with the /orchestrate command and the Python harness.
#
# Requires: cursor-agent (logged in, or CURSOR_API_KEY set), jq.
#
# Usage:
#   orchestrator/orchestrate.sh --task "Add rate limiting to the public API"
#   orchestrator/orchestrate.sh --task-file spec.md      # read task from a file
#   orchestrator/orchestrate.sh < spec.md                # read task from stdin
#   orchestrator/orchestrate.sh                          # type it interactively
#   orchestrator/orchestrate.sh --task "..." --security
#   orchestrator/orchestrate.sh --print-config
#   orchestrator/orchestrate.sh --task "..." --dry-run
#
set -euo pipefail

# ---------------------------------------------------------------------------
# Config
# ---------------------------------------------------------------------------
REPO="."
TASK=""
TASK_FILE=""
MAX_LOOPS=3
SCAN=0
PENTEST=0
DRY_RUN=0
PRINT_CONFIG=0

# Base routes per level (security stages injected separately).
route_base() {
  case "$1" in
    1) echo "van build reflect" ;;
    2) echo "van plan build judge reflect" ;;
    3) echo "van plan creative build judge integrate validate reflect" ;;
    4) echo "van plan creative build judge integrate validate reflect archive" ;;
  esac
}

# Output file (under memory-bank/) for each verdict-bearing stage.
stage_output() {
  case "$1" in
    van)       echo "projectbrief.md" ;;
    scan)      echo "security/scan-latest.md" ;;
    judge)     echo "review/review-latest.md" ;;
    integrate) echo "integration/integration-latest.md" ;;
    validate)  echo "validation/validation-latest.md" ;;
    pentest)   echo "security/pentest-latest.md" ;;
    *)         echo "" ;;
  esac
}

is_verdict_stage() {
  case "$1" in scan|judge|integrate|validate|pentest) return 0 ;; *) return 1 ;; esac
}

# Approximate USD prices per 1M tokens. PLACEHOLDERS — edit these. Cursor reports
# tokens only (no per-request dollars), so any $ figure here is a local estimate.
# Format: "input output cache_read cache_write"
declare -A PRICE
PRICE[claude-opus-5]="15.0 75.0 1.50 18.75"
PRICE[gpt-5.6-sol]="1.25 10.0 0.13 1.25"
PRICE[composer-2.5]="1.25 10.0 0.13 1.25"

# Token accumulators (associative, keyed by model slug).
declare -A TOK_IN TOK_OUT TOK_CR TOK_CW STAGE_COUNT

# ---------------------------------------------------------------------------
# Arg parsing
# ---------------------------------------------------------------------------
while [ $# -gt 0 ]; do
  case "$1" in
    --task)         TASK="$2"; shift 2 ;;
    --task-file)    TASK_FILE="$2"; shift 2 ;;
    --repo)         REPO="$2"; shift 2 ;;
    --max-loops)    MAX_LOOPS="$2"; shift 2 ;;
    --security)     SCAN=1; PENTEST=1; shift ;;
    --scan)         SCAN=1; shift ;;
    --pentest)      PENTEST=1; shift ;;
    --print-config) PRINT_CONFIG=1; shift ;;
    --dry-run)      DRY_RUN=1; shift ;;
    -h|--help)
      grep '^#' "$0" | sed 's/^# \{0,1\}//; 1d'; exit 0 ;;
    *) echo "Unknown option: $1" >&2; exit 1 ;;
  esac
done

REPO="$(cd "$REPO" && pwd)"
AGENTS_DIR="$REPO/.cursor/agents"
[ -d "$AGENTS_DIR" ] || { echo "ERROR: $AGENTS_DIR not found (run from repo root or pass --repo)" >&2; exit 1; }

# ---------------------------------------------------------------------------
# Stage file helpers (single source of truth: .cursor/agents/mb-*.md)
# ---------------------------------------------------------------------------
stage_file() { echo "$AGENTS_DIR/mb-$1.md"; }

# Raw model string from frontmatter (may contain [params]).
stage_model_raw() {
  awk -F':[[:space:]]*' '/^model:/{print $2; exit}' "$(stage_file "$1")"
}
# Model slug with [params] stripped (CLI --model needs a plain slug).
stage_model() { stage_model_raw "$1" | sed 's/\[.*\]//'; }

# Prompt body: everything after the closing frontmatter '---'.
stage_prompt() {
  awk 'BEGIN{fm=0} /^---[[:space:]]*$/{fm++; next} fm>=2{print}' "$(stage_file "$1")"
}

# ---------------------------------------------------------------------------
# Route building
# ---------------------------------------------------------------------------
build_route() {  # $1=level ; SCAN before JUDGE, PENTEST after VALIDATE
  local out=() s
  for s in $(route_base "$1"); do
    [ "$SCAN" -eq 1 ] && [ "$s" = "judge" ] && out+=("scan")
    out+=("$s")
    [ "$PENTEST" -eq 1 ] && [ "$s" = "validate" ] && out+=("pentest")
  done
  echo "${out[*]}"
}

# ---------------------------------------------------------------------------
# Verdict + routing
# ---------------------------------------------------------------------------
read_verdict() {  # $1=stage -> prints PASS/CONDITIONAL/FAIL/UNKNOWN
  local f; f="$REPO/memory-bank/$(stage_output "$1")"
  [ -f "$f" ] || { echo "UNKNOWN"; return; }
  local v; v="$(grep -m1 '^## Verdict:' "$f" | sed 's/.*Verdict:[[:space:]]*//' | awk '{print $1}')"
  echo "${v:-UNKNOWN}" | tr '[:lower:]' '[:upper:]'
}

route_on_fail() {  # $1=stage -> prints target stage
  local f; f="$REPO/memory-bank/$(stage_output "$1")"
  local route_to="" fail_type=""
  if [ -f "$f" ]; then
    route_to="$(grep -m1 -iE 'Route to:' "$f" | sed 's/.*[Rr]oute to:[[:space:]]*//' | awk '{print $1}' | tr '[:upper:]' '[:lower:]')"
    fail_type="$(grep -m1 -iE '(Failure type|Type):' "$f" | sed -E 's/.*[Tt]ype:[[:space:]]*//' | awk '{print $1}' | tr '[:upper:]' '[:lower:]')"
  fi
  case "$route_to" in build|judge|integrate) echo "$route_to"; return ;; esac
  case "$fail_type" in
    code_bug|build_errors)               echo "build"; return ;;
    config_issue|integration_issue)      echo "integrate"; return ;;
    quality_issue|quality_issues)        echo "judge"; return ;;
  esac
  echo "build"  # sensible default for every verdict stage
}

# ---------------------------------------------------------------------------
# Token helpers
# ---------------------------------------------------------------------------
fmt_tokens() {  # $1=int
  awk -v n="$1" 'BEGIN{
    if (n>=1000000) printf "%.1fM", n/1000000;
    else if (n>=1000) printf "%.1fk", n/1000;
    else printf "%d", n;
  }'
}

# ---------------------------------------------------------------------------
# Run one stage via cursor-agent
# ---------------------------------------------------------------------------
run_stage() {  # $1=stage  $2=extra_context ; echoes nothing, updates accumulators
  local name="$1" extra="${2:-}"
  local model prompt
  model="$(stage_model "$name")"
  prompt="$(stage_prompt "$name")

---
TASK: $TASK
CURRENT STAGE: $(echo "$name" | tr '[:lower:]' '[:upper:]')"
  if [ -n "$extra" ]; then
    prompt="$prompt

REMEDIATION CONTEXT (fix these findings this pass):
$extra"
  fi

  local -a model_flag=()
  case "$model" in ""|inherit) ;; *) model_flag=(--model "$model") ;; esac

  local json
  if ! json="$(cursor-agent -p "${model_flag[@]}" \
                  --output-format json --force --trust \
                  --workspace "$REPO" "$prompt")"; then
    echo "ERROR: [$name] cursor-agent run failed" >&2
    return 1
  fi

  # Token usage is best-effort: the CLI may or may not include a `usage` object.
  local i o cr cw
  i="$(echo "$json"  | jq -r '.usage.inputTokens      // 0' 2>/dev/null || echo 0)"
  o="$(echo "$json"  | jq -r '.usage.outputTokens     // 0' 2>/dev/null || echo 0)"
  cr="$(echo "$json" | jq -r '.usage.cacheReadTokens  // 0' 2>/dev/null || echo 0)"
  cw="$(echo "$json" | jq -r '.usage.cacheWriteTokens // 0' 2>/dev/null || echo 0)"

  TOK_IN[$model]=$(( ${TOK_IN[$model]:-0} + i ))
  TOK_OUT[$model]=$(( ${TOK_OUT[$model]:-0} + o ))
  TOK_CR[$model]=$(( ${TOK_CR[$model]:-0} + cr ))
  TOK_CW[$model]=$(( ${TOK_CW[$model]:-0} + cw ))
  STAGE_COUNT[$model]=$(( ${STAGE_COUNT[$model]:-0} + 1 ))

  local total=$(( i + o + cr + cw ))
  if [ "$total" -gt 0 ]; then
    echo "  [$(echo "$name" | tr '[:lower:]' '[:upper:]')] tokens: in $(fmt_tokens "$i"), out $(fmt_tokens "$o"), cache $(fmt_tokens "$cr")/$(fmt_tokens "$cw"), total $(fmt_tokens "$total")"
  else
    echo "  [$(echo "$name" | tr '[:lower:]' '[:upper:]')] tokens: not reported by runtime"
  fi
}

# ---------------------------------------------------------------------------
# Usage summary
# ---------------------------------------------------------------------------
print_usage_summary() {
  [ -n "${STAGE_COUNT[*]+x}" ] || return 0  # no-op if no stages ran (set -u safe)
  echo ""
  echo "── Usage summary ──────────────────────────────"
  local total_cost=0 any_unknown=0 gi=0 go=0 gcr=0 gcw=0 gruns=0
  local model
  for model in $(printf '%s\n' "${!STAGE_COUNT[@]}" | sort); do
    local i=${TOK_IN[$model]:-0} o=${TOK_OUT[$model]:-0} cr=${TOK_CR[$model]:-0} cw=${TOK_CW[$model]:-0}
    local runs=${STAGE_COUNT[$model]:-0} tot=$(( i + o + cr + cw ))
    gi=$((gi+i)); go=$((go+o)); gcr=$((gcr+cr)); gcw=$((gcw+cw)); gruns=$((gruns+runs))
    local cost_str
    if [ -n "${PRICE[$model]:-}" ]; then
      read -r pi po pcr pcw <<<"${PRICE[$model]}"
      local cost
      cost="$(awk -v i="$i" -v o="$o" -v cr="$cr" -v cw="$cw" -v pi="$pi" -v po="$po" -v pcr="$pcr" -v pcw="$pcw" \
        'BEGIN{printf "%.4f", i/1e6*pi + o/1e6*po + cr/1e6*pcr + cw/1e6*pcw}')"
      total_cost="$(awk -v a="$total_cost" -v b="$cost" 'BEGIN{printf "%.4f", a+b}')"
      cost_str="$(awk -v c="$cost" 'BEGIN{printf "~$%.2f", c}')"
    else
      any_unknown=1; cost_str="no price set"
    fi
    printf "  %-26s %2d run(s)  %7s tok   %s\n" "$model" "$runs" "$(fmt_tokens "$tot")" "$cost_str"
  done
  local gtot=$(( gi + go + gcr + gcw ))
  printf "  %-26s %2d run(s)  %7s tok   %s\n" "TOTAL" "$gruns" "$(fmt_tokens "$gtot")" \
    "$(awk -v c="$total_cost" 'BEGIN{printf "~$%.2f", c}')"
  echo "  (in $(fmt_tokens "$gi"), out $(fmt_tokens "$go"), cache $(fmt_tokens "$gcr")/$(fmt_tokens "$gcw"))"
  echo "  ⚠ Cost is a LOCAL ESTIMATE from the PRICE table (placeholder prices — edit them). Cursor reports tokens only."
  [ "$any_unknown" -eq 1 ] && echo "  ⚠ Some models had no price set; their tokens are excluded from the \$ estimate."
  return 0
}

# ---------------------------------------------------------------------------
# --print-config
# ---------------------------------------------------------------------------
if [ "$PRINT_CONFIG" -eq 1 ]; then
  echo "Stage -> model (from .cursor/agents/mb-*.md):"
  echo ""
  for s in van plan creative build scan judge integrate validate pentest reflect archive; do
    raw="$(stage_model_raw "$s")"; base="$(stage_model "$s")"
    if [ "$raw" != "$base" ]; then
      printf "  %-10s %s  (CLI uses: %s)\n" "$s" "$raw" "$base"
    else
      printf "  %-10s %s\n" "$s" "$raw"
    fi
  done
  echo ""
  echo "Level routes (default — security OFF):"
  for l in 1 2 3 4; do echo "  L$l: $(route_base "$l" | tr 'a-z ' 'A-Z ' )"; done
  echo ""
  echo "Price table (\$/1M tokens — PLACEHOLDERS): in out cache_read cache_write"
  for m in $(printf '%s\n' "${!PRICE[@]}" | sort); do printf "  %-26s %s\n" "$m" "${PRICE[$m]}"; done
  exit 0
fi

# Resolve task: --task wins, else --task-file, else stdin (piped or typed).
if [ -z "$TASK" ]; then
  if [ -n "$TASK_FILE" ]; then
    [ -f "$TASK_FILE" ] || { echo "ERROR: --task-file '$TASK_FILE' not found" >&2; exit 1; }
    TASK="$(cat "$TASK_FILE")"
  else
    [ -t 0 ] && echo "Enter the task (finish with Ctrl-D on a blank line):" >&2
    TASK="$(cat)"
  fi
fi
TASK="$(printf '%s' "$TASK" | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
[ -n "$TASK" ] || { echo "ERROR: no task provided (use --task, --task-file, or pipe/type via stdin)" >&2; exit 1; }

# ---------------------------------------------------------------------------
# Orchestration
# ---------------------------------------------------------------------------
echo "[VAN] running on $(stage_model_raw van) ..."
if [ "$DRY_RUN" -eq 0 ]; then
  run_stage van ""
  brief="$REPO/memory-bank/projectbrief.md"
  LEVEL="$(grep -m1 -oE 'Level:[[:space:]]*[1-4]' "$brief" 2>/dev/null | grep -oE '[1-4]' | head -n1 || true)"
  [ -n "${LEVEL:-}" ] || { echo "ERROR: VAN did not write a parseable 'Level: [N]' line in $brief" >&2; exit 1; }
else
  LEVEL=3
fi
echo "  [VAN] Complete — Level $LEVEL assessed"

ROUTE="$(build_route "$LEVEL")"
SEC=""
echo "$ROUTE" | grep -qw scan    && SEC="scan"
echo "$ROUTE" | grep -qw pentest && SEC="${SEC:+$SEC, }pentest"
echo ""
echo "Route (Level $LEVEL) (security: ${SEC:-off}): $(echo "$ROUTE" | tr 'a-z ' 'A-Z ')"
echo ""

# Loop counters keyed by "stage->target".
declare -A LOOPS
BUILD_ITERS=0
declare -A PENDING  # remediation context keyed by target stage

# Convert route to array; skip VAN (index 0, already ran).
read -r -a STAGES <<<"$ROUTE"
i=1
while [ "$i" -lt "${#STAGES[@]}" ]; do
  name="${STAGES[$i]}"
  echo "[$(echo "$name" | tr '[:lower:]' '[:upper:]')] running on $(stage_model_raw "$name") ..."
  if [ "$DRY_RUN" -eq 1 ]; then i=$((i+1)); continue; fi

  extra="${PENDING[$name]:-}"; unset "PENDING[$name]" 2>/dev/null || true
  run_stage "$name" "$extra"
  [ "$name" = "build" ] && BUILD_ITERS=$((BUILD_ITERS+1))

  if ! is_verdict_stage "$name"; then
    echo "  [$(echo "$name" | tr '[:lower:]' '[:upper:]')] Complete"
    i=$((i+1)); continue
  fi

  verdict="$(read_verdict "$name")"
  case "$verdict" in
    PASS|CONDITIONAL)
      echo "  [$(echo "$name" | tr '[:lower:]' '[:upper:]')] Complete — $verdict"
      i=$((i+1)) ;;
    FAIL)
      target="$(route_on_fail "$name")"
      edge="$name->$target"
      LOOPS[$edge]=$(( ${LOOPS[$edge]:-0} + 1 ))
      echo "  [$(echo "$name" | tr '[:lower:]' '[:upper:]')] FAIL — looping back to $(echo "$target" | tr '[:lower:]' '[:upper:]') (attempt ${LOOPS[$edge]}/$MAX_LOOPS)"
      if [ "${LOOPS[$edge]}" -gt "$MAX_LOOPS" ]; then
        echo "" >&2
        echo "ESCALATION: $edge exceeded $MAX_LOOPS remediation loops. Stopping for human review. See memory-bank/$(stage_output "$name")" >&2
        print_usage_summary
        exit 2
      fi
      PENDING[$target]="$(cat "$REPO/memory-bank/$(stage_output "$name")" 2>/dev/null || true)"
      # Jump index back to target stage.
      for j in "${!STAGES[@]}"; do [ "${STAGES[$j]}" = "$target" ] && { i="$j"; break; }; done ;;
    *)
      echo "" >&2
      echo "ERROR: could not parse a verdict from memory-bank/$(stage_output "$name") (got '$verdict')." >&2
      exit 1 ;;
  esac
done

echo ""
echo "Pipeline complete (Level $LEVEL). Build iterations: $BUILD_ITERS."
echo "Artifacts are in memory-bank/."
print_usage_summary
