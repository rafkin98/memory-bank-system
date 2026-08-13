# Memory Bank System v1.0

This project uses the Memory Bank development pipeline — a structured multi-stage workflow with optional security gates.

## Usage

Run `/orchestrate <task description>` to execute the full pipeline automatically.

The orchestrator spawns each stage as a subagent, parses verdicts, and routes failures back automatically. **SCAN and PENTEST are optional and OFF by default** — opt in by adding `--security` (both), `--scan`, or `--pentest` to the task, or by asking for a security scan / pentest:

```
VAN → PLAN → CREATIVE → BUILD → JUDGE → INTEGRATE → VALIDATE → REFLECT → ARCHIVE
              (with --security: SCAN before JUDGE, PENTEST after VALIDATE)
```

## Complexity Routing (default — security off)

- **Level 1** (bug fix): VAN → BUILD → REFLECT
- **Level 2** (enhancement): VAN → PLAN → BUILD → JUDGE → REFLECT
- **Level 3** (feature): VAN → PLAN → CREATIVE → BUILD → JUDGE → INTEGRATE → VALIDATE → REFLECT
- **Level 4** (system): Full L3 pipeline + ARCHIVE

## Memory Bank

All pipeline outputs are stored in `memory-bank/`:

- `tasks.md` — source of truth for task tracking
- `activeContext.md` — current stage and focus
- `progress.md` — implementation status
- `projectbrief.md` — project context and complexity level
- `creative/` — design decision documents
- `security/` — scan and pentest reports
- `review/` — code review reports
- `integration/` — integration reports
- `validation/` — validation reports
- `reflection/` — retrospective documents
- `archive/` — completed task archives

## Failure Routing

- **JUDGE FAIL** → back to BUILD (fix code quality)
- **INTEGRATE FAIL** → back to BUILD or JUDGE
- **VALIDATE FAIL** → back to BUILD, JUDGE, or INTEGRATE
- **SCAN FAIL** (when enabled) → back to BUILD (remediate vulnerabilities)
- **PENTEST FAIL** (when enabled) → back to BUILD (code bug) or INTEGRATE (config issue)

After 3 failed loops on any stage, the orchestrator asks for guidance.

## Engineering bar

Each stage runs at a senior/principal bar: BUILD (Principal Engineer) ships the simplest clean, optimized solution that fully works and self-verifies via a tests/lint/types gate; JUDGE (Principal Engineer reviewer) independently probes for bloat/over-engineering and fails egregious complexity back to BUILD; VALIDATE (Principal QA) verifies every acceptance criterion with evidence; REFLECT (Senior Analyst) assesses whether the result is clean, simple, and robust.
