# Memory Bank System Release Notes

> **Personal Note**: Memory Bank is my personal hobby project that I develop for my own use in coding projects. As this is a personal project, I don't maintain an issues tracker or actively collect feedback. However, if you're using these rules and encounter issues, one of the great advantages is that you can ask the Cursor AI directly to modify or update the rules to better suit your specific workflow. The system is designed to be adaptable by the AI, allowing you to customize it for your own needs without requiring external support.

## Version 1.1 - Cursor Orchestration, Multi-Language Harness & Optional Security

> Building on v1.0, this release brings **automated orchestration to Cursor** (not just Claude Code), adds a standalone **Python / Bash / Rust** harness for terminal & CI runs, makes the **security stages opt-in**, and raises the engineering bar of the agents.

### 🌟 Major Features

#### Cursor orchestration with per-stage models
- **`/orchestrate` command** for Cursor (`.cursor/commands/orchestrate.md`) drives the full pipeline via subagents.
- **11 stage subagents** (`.cursor/agents/mb-*.md`) — each pins its **own model** in YAML frontmatter (`model:`), so review stages run on a different model family than BUILD to reduce correlated blind spots.

#### Standalone orchestrator harness (`orchestrator/`)
- Three interchangeable implementations of the same verdict-routed state machine, all reading the same `mb-*.md` stage files:
  - **Python** (`pipeline.py`) — Cursor Python SDK
  - **Bash** (`orchestrate.sh`) — `cursor-agent` CLI + `jq`
  - **Rust** (`rust/`) — `cursor-agent` CLI wrapper (only `serde_json` dep)
- **Task input three ways:** `--task "..."`, `--task-file spec.md` (e.g. a Markdown brief), or piped/typed via **stdin**.
- Per-stage + summary **token reporting** with a local (editable) **cost estimate**; `--print-config`, `--dry-run`, and `--max-loops` controls.

#### Security stages are now optional (off by default)
- SCAN and PENTEST no longer run automatically. Opt in with `--security` / `--scan` / `--pentest` (harness) or by asking for security in the task text (`/orchestrate`).
- **New default routes (security off):** L1 = 3, L2 = 5, L3 = 8, L4 = 9 stages. Enabling security injects SCAN before JUDGE and PENTEST after VALIDATE.

#### Raised engineering bar (agent standards)
- **BUILD** now runs as a **Principal Engineer** — simplest clean solution, no bloat, a self-review loop, and a hard verification gate (tests/lint/types) before "done".
- **JUDGE** as **Principal Engineer / Code Reviewer**, **VALIDATE** as **Principal QA Engineer**, **REFLECT** as **Senior Analyst** — each probing until genuinely satisfied.
- **VAN** complexity assessment aligned to the full `complexity-decision-tree.mdc` criteria.

### 📚 Files Added
- `.cursor/commands/orchestrate.md` and `.cursor/agents/mb-*.md` (11 stage subagents)
- `orchestrator/` — `pipeline.py`, `orchestrate.sh`, `rust/` (`Cargo.toml`, `src/main.rs`), `requirements.txt`, `README.md`

### 📝 Files Updated
- `.claude/skills/orchestrate/SKILL.md` and `.claude/CLAUDE.md` — optional security + principal-level agent standards
- `README.md` and `USER_GUIDE.md` — Cursor orchestration, the harness, task input modes, optional-security routing

### 🔧 Requirements
- **Python harness:** `pip install -r orchestrator/requirements.txt` (`cursor-sdk`) + `CURSOR_API_KEY`
- **Bash harness:** `cursor-agent` CLI (logged in) + `jq`
- **Rust harness:** `cargo` + `cursor-agent` CLI

---

## Version 1.0 - Security Pipeline & Claude Code Orchestrator

> Building upon Memory Bank v0.8, this release adds dedicated security stages (SCAN and PENTEST) to the pipeline, expands from 9 to 11 stages, and introduces Claude Code support via the `/orchestrate` skill.

### 🌟 Major Features

#### 11-Stage Security Pipeline
- **SCAN stage** — Static security analysis between BUILD and JUDGE (L2/L3/L4)
  - 25-point security rubric (SAST, Dependencies, Secrets, OWASP, Architecture)
  - 10-point abbreviated checklist for Level 2
  - Verdict routing: PASS/CONDITIONAL → JUDGE, FAIL → BUILD
- **PENTEST stage** — Dynamic penetration testing between VALIDATE and REFLECT (L3/L4)
  - Attack surface mapping, auth/injection/API/input testing
  - Severity classification (Critical/High/Medium/Low)
  - Failure routing: code bugs → BUILD, config issues → INTEGRATE

#### Claude Code Orchestrator
- `/orchestrate` skill runs the full pipeline as multi-agent orchestration
- SCAN and PENTEST subagent prompts integrated into orchestrator
- Automatic verdict parsing and failure routing across all security stages

#### Updated Complexity Routing
- Level 1: VAN → BUILD → REFLECT (3 stages, unchanged)
- Level 2: VAN → PLAN → BUILD → **SCAN** → JUDGE → REFLECT (6 stages)
- Level 3: VAN → PLAN → CREATIVE → BUILD → **SCAN** → JUDGE → INTEGRATE → VALIDATE → **PENTEST** → REFLECT (10 stages)
- Level 4: Full pipeline + ARCHIVE (11 stages)

### 🔄 New Agent Roles
- **Security Analyst** (SCAN) — SAST, dependency audit, secrets scanning, OWASP assessment
- **Penetration Tester** (PENTEST) — Attack simulation, auth testing, injection probing, API security

### 📚 Files Added
- `.cursor/commands/scan.md` and `.cursor/commands/pentest.md`
- `.cursor/rules/isolation_rules/visual-maps/scan-mode-map.mdc` and `pentest-mode-map.mdc`
- `.claude/skills/orchestrate/SKILL.md` (with SCAN/PENTEST agent prompts)
- `memory-bank/security/` output directory

### 📝 Files Updated
- `main.mdc` — Mode architecture, complexity routing, rule loading, reference maps
- `agent-roles.mdc` — Security Analyst and Penetration Tester role definitions
- `memory-bank-paths.mdc` — Security scan and pentest document paths
- `workflow-level2.mdc` — SCAN phase added (8 phases total)
- `workflow-level3.mdc` — SCAN and PENTEST phases added (12 phases total)
- `workflow-level4.mdc` — SCAN and PENTEST phases added (12 phases total)
- `README.md` and `USER_GUIDE.md` — Full documentation updates

### 🔧 Requirements
- Cursor version 2.0+ for Cursor commands
- Claude Code for `/orchestrate` skill (optional)
- No external security tools required — Claude reasoning handles analysis

---

## Version 0.8 - Enhanced Commands and Workflow

> Building upon the token-optimized workflows established in v0.7-beta, this release focuses on improved command integration and workflow enhancements.

### 🌟 Major Features

#### Cursor 2.0 Commands Integration _(Enhanced)_
- Native integration with Cursor 2.0 commands feature
- Improved command discovery and usage
- Streamlined workflow transitions
- Better integration with Cursor's native features

#### Workflow Refinements _(Enhanced)_
- Enhanced command-to-command transitions
- Improved context preservation between commands
- Better error handling and recovery
- More intuitive workflow guidance

### 🔄 Process Improvements

#### Command System Enhancements
- Improved command documentation
- Better command discovery mechanisms
- Enhanced workflow routing logic
- Streamlined command execution

### 📚 Documentation Enhancements
- Updated README with command-focused workflow
- Enhanced command documentation
- Improved migration guides
- Better examples and usage patterns

### 🛠 Technical Improvements
- Optimized command execution
- Improved rule loading for commands
- Enhanced context management
- Better integration with Cursor 2.0 features

### 📋 Known Issues
- None reported in current release

### 🔜 Upcoming Features
- Further workflow optimizations
- Enhanced command capabilities
- Improved context management
- Additional integration features

### 📝 Notes
- This release builds upon v0.7-beta's token optimization foundation
- Enhanced command integration and workflow improvements
- No manual migration required
- Backward compatible with v0.7-beta workflows

### 🔧 Requirements
- Requires Cursor version 2.0 or higher (commands feature)
- Compatible with Claude 4 Sonnet (recommended) and newer models
- Compatible with all existing Memory Bank v0.7-beta installations

---

## Version 0.7-beta - Token-Optimized Workflows

> Building upon the architectural foundations established in v0.6-beta.1, this release introduces significant token efficiency optimizations and enhanced workflow capabilities with substantial improvements in context management.

### 🌟 Major Features

#### Hierarchical Rule Loading System _(New)_
- Just-In-Time (JIT) loading of specialized rules
- Core rule caching across mode transitions
- Complexity-based rule selection
- Significant reduction in rule-related token usage

#### Progressive Documentation Framework _(New)_
- Concise templates that scale with task complexity
- Tabular formats for efficient option comparison
- "Detail-on-demand" approach for creative phases
- Streamlined documentation without sacrificing quality

#### Optimized Mode Transitions _(Enhanced)_
- Unified context transfer protocol
- Standardized transition documents
- Selective context preservation
- Improved context retention between modes

#### Enhanced Multi-Level Workflow System _(Enhanced)_
- **Level 1: Quick Bug Fix Pipeline**
  - Ultra-compact documentation templates
  - Consolidated memory bank updates
  - Streamlined 3-phase workflow

- **Level 2: Enhancement Pipeline**
  - Balanced 4-phase workflow
  - Simplified planning templates
  - Faster documentation process

- **Level 3: Feature Development Pipeline**
  - Comprehensive planning system
  - Optimized creative phase exploration
  - Improved context efficiency

- **Level 4: Enterprise Pipeline**
  - Advanced 6-phase workflow
  - Tiered documentation templates
  - Enhanced governance controls

### 🔄 Process Improvements

#### Token-Optimized Architecture
- Reduced context usage for system rules
- More context available for actual development tasks
- Adaptive complexity scaling based on task requirements
- Differential memory bank updates to minimize token waste

#### Mode-Based Optimization
- **VAN Mode**: Efficient complexity determination with minimal overhead
- **PLAN Mode**: Complexity-appropriate planning templates
- **CREATIVE Mode**: Progressive documentation with tabular comparisons
- **BUILD Mode**: Streamlined implementation guidance
- **REFLECT Mode**: Context-aware review mechanisms
- **ARCHIVE Mode**: Efficient knowledge preservation

#### Advanced Workflow Optimization
- Intelligent level transition system
- Clear complexity assessment criteria
- Streamlined mode switching
- Enhanced task tracking capabilities

### 📚 Documentation Enhancements
- Level-specific documentation templates
- Progressive disclosure model for complex documentation
- Standardized comparison formats for design decisions
- Enhanced context preservation between documentation phases

### 🛠 Technical Improvements
- Graph-based rule architecture for efficient navigation
- Rule dependency tracking for optimal loading
- Context compression techniques for memory bank files
- Adaptive rule partitioning for targeted loading

### 📋 Known Issues
- None reported in current release

### 🧠 The Determinism Challenge in AI Workflows

While Memory Bank provides robust structure through visual maps and process flows, it's important to acknowledge an inherent limitation: the non-deterministic nature of AI agents. Despite our best efforts to create well-defined pathways and structured processes, language models fundamentally operate on probability distributions rather than fixed rules.

This creates what I call the "determinism paradox" – we need structure for reliability, but rigidity undermines the adaptability that makes AI valuable. Memory Bank addresses this through:

- **Guiding rather than forcing**: Using visual maps that shape behavior without rigid constraints
- **Bounded flexibility**: Creating structured frameworks within which creative problem-solving can occur
- **Adaptive complexity**: Adjusting guidance based on task requirements rather than enforcing one-size-fits-all processes

As a companion to Memory Bank, I'm developing an MCP Server (Model-Context-Protocol) project that aims to further address this challenge by integrating deterministic code checkpoints with probabilistic language model capabilities. This hybrid approach creates a system where AI can operate flexibly while still following predictable workflows – maintaining the balance between structure and adaptability that makes Memory Bank effective.

When using Memory Bank, you may occasionally need to guide the agent back to the intended workflow. This isn't a failure of the system but rather a reflection of the fundamental tension between structure and flexibility in AI systems.

### 🔜 Upcoming Features
- Dynamic template generation based on task characteristics
- Automatic context summarization for long-running tasks
- Cross-task knowledge preservation
- Partial rule loading within specialized rule files
- MCP integration for improved workflow adherence

### 📝 Notes
- This release builds upon v0.6-beta.1's architectural foundation
- Significantly enhances JIT Rule Loading efficiency 
- No manual migration required
- New files added to `.cursor/rules/isolation_rules/` directory

### 🔧 Requirements
- Requires Cursor version 0.48 or higher
- Compatible with Claude 3.7 Sonnet (recommended) and newer models
- Compatible with all existing Memory Bank v0.6-beta.1 installations

### 📈 Optimization Approaches
- **Rule Loading**: Hierarchical loading with core caching and specialized lazy-loading
- **Creative Phase**: Progressive documentation with tabular comparisons
- **Mode Transitions**: Unified context transfer with selective preservation
- **Level 1 Workflow**: Ultra-compact templates with consolidated updates
- **Memory Bank**: Differential updates and context compression

---
Released on: May 7, 2025