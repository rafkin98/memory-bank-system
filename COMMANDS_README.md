# Memory Bank Commands

This directory contains Cursor 2.0 commands and Claude Code skills that implement the Memory Bank pipeline. The system supports these interfaces:

- **Cursor IDE (manual)** — 11 individual `/commands` you run stage by stage
- **Cursor IDE (automated)** — a `/orchestrate` command, or a standalone Python/Bash/Rust harness in [`orchestrator/`](orchestrator/README.md), each running stages on their own pinned model
- **Claude Code** — a single `/orchestrate` skill that runs the full pipeline automatically

> **SCAN and PENTEST are optional and off by default** — opt in with `--security` / `--scan` / `--pentest` (harness) or by asking for security in the task text (`/orchestrate`).

## Available Commands (Cursor IDE)

### `/van` - Initialization & Entry Point
**Purpose:** Initialize Memory Bank, detect platform, determine task complexity, and route to appropriate workflows.

**When to use:**
- Starting a new task or project
- Initializing Memory Bank structure
- Determining task complexity

**Next steps:**
- Level 1 tasks → `/build`
- Level 2-4 tasks → `/plan`

### `/plan` - Task Planning
**Purpose:** Create detailed implementation plans based on complexity level.

**When to use:**
- After `/van` determines Level 2-4 complexity
- Need to create structured implementation plan

**Next steps:**
- Creative phases identified → `/creative`
- No creative phases → `/build`

### `/creative` - Design Decisions
**Purpose:** Perform structured design exploration for components requiring creative phases.

**When to use:**
- After `/plan` identifies components needing design decisions
- Need to explore architecture, UI/UX, or algorithm options

**Next steps:**
- After all creative phases complete → `/build`

### `/build` - Code Implementation
**Purpose:** Implement planned changes following the implementation plan and creative decisions.

**When to use:**
- After planning is complete (and creative phases if needed)
- Ready to start coding

**Next steps:**
- After implementation complete → `/scan`

### `/scan` - Security Analysis
**Purpose:** Static security analysis to catch vulnerabilities before code review. Uses SAST, dependency auditing, secrets scanning, and OWASP compliance assessment.

**When to use:**
- After `/build` completes implementation
- Level 2: 10-point security checklist
- Level 3-4: Full 25-point security rubric

**Verdicts:**
- **PASS** (no high/critical findings) → `/judge`
- **CONDITIONAL** (medium findings only) → `/judge` with security notes
- **FAIL** (high/critical findings) → `/build` for remediation

### `/judge` - Code Review
**Purpose:** Rubric-based code quality assessment with scoring across 5 categories.

**When to use:**
- After `/scan` passes or conditionally passes
- Level 2: 10-point checklist
- Level 3-4: Full 25-point rubric

**Verdicts:**
- **PASS** (>= 80%) → `/integrate` (L3-4) or `/reflect` (L2)
- **CONDITIONAL** (60-79%) → proceed with notes
- **FAIL** (< 60%) → `/build` to fix critical issues

### `/integrate` - Integration & Release Prep
**Purpose:** Merge components, resolve dependencies, verify the build, run integration tests.

**When to use:**
- After `/judge` passes (Level 3-4 only)

**Verdicts:**
- **PASS** → `/validate`
- **FAIL** (build errors) → `/build`
- **FAIL** (quality issues) → `/judge`

### `/validate` - End-to-End Testing
**Purpose:** Behavioral testing, acceptance criteria verification, regression testing, traceability matrix.

**When to use:**
- After `/integrate` passes (Level 3-4 only)

**Verdicts:**
- **PASS** → `/pentest` (L3-4) or `/reflect` (L2)
- **FAIL** (code bug) → `/build`
- **FAIL** (integration issue) → `/integrate`

### `/pentest` - Penetration Testing
**Purpose:** Dynamic security testing against the integrated system. Tests auth, injection vectors, API security, and input validation.

**When to use:**
- After `/validate` passes (Level 3-4 only)
- Requires a testable integrated system

**Verdicts:**
- **PASS** (no critical/high findings) → `/reflect`
- **FAIL** (code bug) → `/build`
- **FAIL** (config issue) → `/integrate`

### `/reflect` - Task Reflection
**Purpose:** Facilitate structured reflection on completed implementation.

**When to use:**
- After security and quality gates pass
- Need to document lessons learned and process improvements

**Next steps:**
- Level 1-3 → task complete (or `/archive` for L3 optionally)
- Level 4 → `/archive`

### `/archive` - Task Archiving
**Purpose:** Create comprehensive archive documentation and update Memory Bank.

**When to use:**
- After `/reflect` completes (Level 4 mandatory, Level 3 recommended)
- Ready to finalize task documentation

**Next steps:**
- After archiving complete → `/van` (for next task)

## Automated orchestration (`/orchestrate` & harness)

You can run the entire pipeline automatically — no individual commands needed — three ways, all reading the same `.cursor/agents/mb-*.md` stage files and routing on the same verdicts:

- **Claude Code** — `/orchestrate Add user authentication with OAuth2 support`. See `.claude/skills/orchestrate/SKILL.md`.
- **Cursor `/orchestrate` command** — same in Cursor chat; drives the `mb-*` subagents, each on its own pinned model. See `.cursor/commands/orchestrate.md`.
- **Terminal / CI harness** — Python, Bash, or Rust:

```bash
python3 orchestrator/pipeline.py --task "..."      # or --task-file spec.md, or < spec.md
orchestrator/orchestrate.sh --task "..."
cd orchestrator/rust && cargo run -- --task "..." --repo ../..
```

All harnesses accept the task via `--task`, `--task-file`, or stdin. See [`orchestrator/README.md`](orchestrator/README.md).

## Command Workflows by Complexity

Default routes (**security stages off**):

```
Level 1 (Bug Fix):
  /van → /build → /reflect

Level 2 (Enhancement):
  /van → /plan → /build → /judge → /reflect

Level 3 (Feature):
  /van → /plan → /creative → /build → /judge → /integrate → /validate → /reflect

Level 4 (System):
  /van → /plan → /creative → /build → /judge → /integrate → /validate → /reflect → /archive
```

With security enabled (`--security`), SCAN is injected before JUDGE and PENTEST after VALIDATE:

```
Level 3 (Feature, +security):
  /van → /plan → /creative → /build → /scan → /judge → /integrate → /validate → /pentest → /reflect
```

## Progressive Rule Loading

Each command implements progressive rule loading to optimize context usage:

1. **Core Rules** - Always loaded (main.mdc, memory-bank-paths.mdc, agent-roles.mdc)
2. **Mode-Specific Rules** - Loaded based on command (visual maps)
3. **Complexity-Specific Rules** - Loaded based on task complexity level
4. **Specialized Rules** - Lazy loaded only when needed (e.g., creative phase types)

This approach reduces initial token usage by ~70% compared to loading all rules at once.

## Memory Bank Integration

All commands read from and update files in the `memory-bank/` directory:

- **tasks.md** - Source of truth for task tracking
- **activeContext.md** - Current project focus
- **progress.md** - Implementation status
- **projectbrief.md** - Project foundation
- **creative/** - Creative phase documents
- **security/** - Security scan and pentest reports
- **review/** - Code review reports
- **integration/** - Integration reports
- **validation/** - Validation reports
- **reflection/** - Reflection documents
- **archive/** - Archive documents

## Migration from Custom Modes

These commands replace the previous custom modes:
- **VAN Mode** → `/van` command
- **PLAN Mode** → `/plan` command
- **CREATIVE Mode** → `/creative` command
- **BUILD Mode** → `/build` command
- **SCAN Mode** → `/scan` command (new in v1.0)
- **JUDGE Mode** → `/judge` command
- **INTEGRATE Mode** → `/integrate` command
- **VALIDATE Mode** → `/validate` command
- **PENTEST Mode** → `/pentest` command (new in v1.0)
- **REFLECT Mode** → `/reflect` command
- **ARCHIVE Mode** → `/archive` command

The functionality remains the same, but now uses Cursor 2.0's commands feature instead of custom modes. For automated runs, use `/orchestrate` (Cursor or Claude Code) or the [`orchestrator/`](orchestrator/README.md) harness instead of individual commands.
