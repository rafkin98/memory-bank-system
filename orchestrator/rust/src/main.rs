//! Memory Bank pipeline harness (Rust / cursor-agent CLI).
//!
//! A deterministic, verdict-routed orchestrator that drives each Memory Bank
//! stage as its own headless `cursor-agent` run, each on the model pinned in the
//! matching `.cursor/agents/mb-<stage>.md` file. Those files are the single
//! source of truth shared with the `/orchestrate` command, the Python harness,
//! and the Bash harness — so none of the four paths drift.
//!
//! This is a thin, dependency-light CLI wrapper (there is no first-party Rust
//! SDK): it shells out to `cursor-agent -p --output-format json` per stage.
//!
//! Prereqs: `cursor-agent` installed and logged in (or `CURSOR_API_KEY` set).
//!
//! Usage:
//!   mb-orchestrator --task "Add rate limiting to the public API"
//!   mb-orchestrator --task-file spec.md      # read task from a file
//!   mb-orchestrator < spec.md                # read task from stdin
//!   mb-orchestrator                          # type it interactively
//!   mb-orchestrator --task "..." --security
//!   mb-orchestrator --print-config
//!   mb-orchestrator --task "..." --dry-run

use std::collections::HashMap;
use std::fs;
use std::io::{IsTerminal, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// ---------------------------------------------------------------------------
// Static config
// ---------------------------------------------------------------------------

/// Base forward route per complexity level, WITHOUT the optional security
/// stages. SCAN/PENTEST are injected by `build_route` when enabled.
fn base_route(level: u8) -> Vec<&'static str> {
    match level {
        1 => vec!["van", "build", "reflect"],
        2 => vec!["van", "plan", "build", "judge", "reflect"],
        3 => vec![
            "van", "plan", "creative", "build", "judge", "integrate", "validate", "reflect",
        ],
        _ => vec![
            "van", "plan", "creative", "build", "judge", "integrate", "validate", "reflect",
            "archive",
        ],
    }
}

/// Output file (under memory-bank/) that a verdict-bearing stage writes.
fn stage_output(name: &str) -> Option<&'static str> {
    match name {
        "van" => Some("projectbrief.md"),
        "scan" => Some("security/scan-latest.md"),
        "judge" => Some("review/review-latest.md"),
        "integrate" => Some("integration/integration-latest.md"),
        "validate" => Some("validation/validation-latest.md"),
        "pentest" => Some("security/pentest-latest.md"),
        _ => None,
    }
}

fn is_verdict_stage(name: &str) -> bool {
    matches!(name, "scan" | "judge" | "integrate" | "validate" | "pentest")
}

const ALL_STAGES: [&str; 11] = [
    "van", "plan", "creative", "build", "scan", "judge", "integrate", "validate", "pentest",
    "reflect", "archive",
];

/// Approximate USD price per 1M tokens: [input, output, cache_read, cache_write].
///
/// ⚠ PLACEHOLDERS — edit these. Cursor reports tokens only (no per-request
/// dollars), so any figure here is a LOCAL ESTIMATE. `None` => price unknown.
fn price(model_base: &str) -> Option<[f64; 4]> {
    match model_base {
        "claude-opus-5" => Some([15.0, 75.0, 1.50, 18.75]),
        "gpt-5.6-sol" => Some([1.25, 10.0, 0.13, 1.25]),
        "composer-2.5" => Some([1.25, 10.0, 0.13, 1.25]),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// CLI config
// ---------------------------------------------------------------------------

struct Config {
    repo: PathBuf,
    task: String,
    task_file: Option<String>,
    max_loops: u32,
    scan: bool,
    pentest: bool,
    dry_run: bool,
    print_config: bool,
}

fn parse_args() -> Result<Config, String> {
    let mut cfg = Config {
        repo: PathBuf::from("."),
        task: String::new(),
        task_file: None,
        max_loops: 3,
        scan: false,
        pentest: false,
        dry_run: false,
        print_config: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--task" => cfg.task = args.next().ok_or("--task needs a value")?,
            "--task-file" => cfg.task_file = Some(args.next().ok_or("--task-file needs a value")?),
            "--repo" => cfg.repo = PathBuf::from(args.next().ok_or("--repo needs a value")?),
            "--max-loops" => {
                cfg.max_loops = args
                    .next()
                    .ok_or("--max-loops needs a value")?
                    .parse()
                    .map_err(|_| "--max-loops must be an integer".to_string())?
            }
            "--security" => {
                cfg.scan = true;
                cfg.pentest = true;
            }
            "--scan" => cfg.scan = true,
            "--pentest" => cfg.pentest = true,
            "--dry-run" => cfg.dry_run = true,
            "--print-config" => cfg.print_config = true,
            "-h" | "--help" => {
                print_help();
                std::process::exit(0);
            }
            other => return Err(format!("Unknown option: {other}")),
        }
    }
    Ok(cfg)
}

/// Resolve the task from --task, then --task-file, then stdin (piped or typed).
fn resolve_task(cfg: &Config) -> Result<String, String> {
    if !cfg.task.trim().is_empty() {
        return Ok(cfg.task.trim().to_string());
    }
    if let Some(path) = &cfg.task_file {
        let text = fs::read_to_string(path)
            .map_err(|e| format!("--task-file '{path}': {e}"))?;
        let text = text.trim().to_string();
        if text.is_empty() {
            return Err(format!("--task-file '{path}' is empty"));
        }
        return Ok(text);
    }
    if std::io::stdin().is_terminal() {
        eprintln!("Enter the task (finish with Ctrl-D on a blank line):");
    }
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| format!("reading task from stdin: {e}"))?;
    Ok(buf.trim().to_string())
}

fn print_help() {
    println!(
        "mb-orchestrator — Memory Bank pipeline harness (Rust / cursor-agent CLI)\n\n\
         Usage:\n  \
           mb-orchestrator --task \"<description>\" [--repo PATH] [--max-loops N]\n  \
           mb-orchestrator --task-file spec.md   # or:  mb-orchestrator < spec.md\n  \
           mb-orchestrator                       # type the task interactively\n  \
           mb-orchestrator --task \"...\" --security | --scan | --pentest\n  \
           mb-orchestrator --print-config\n  \
           mb-orchestrator --task \"...\" --dry-run\n"
    );
}

// ---------------------------------------------------------------------------
// Stage definitions (single source of truth: .cursor/agents/mb-*.md)
// ---------------------------------------------------------------------------

struct StageDef {
    name: String,
    model: String,      // verbatim from frontmatter (may contain [params])
    model_base: String, // slug with [params] stripped (CLI --model needs plain)
    prompt: String,     // subagent body
}

fn stage_path(repo: &Path, name: &str) -> PathBuf {
    repo.join(".cursor").join("agents").join(format!("mb-{name}.md"))
}

/// Remove a trailing/inline `[...]` params block from a model slug.
fn strip_params(model: &str) -> String {
    match model.find('[') {
        Some(i) => model[..i].trim().to_string(),
        None => model.trim().to_string(),
    }
}

fn load_stage(repo: &Path, name: &str) -> Result<StageDef, String> {
    let path = stage_path(repo, name);
    let text = fs::read_to_string(&path)
        .map_err(|e| format!("Missing/unreadable subagent file {}: {e}", path.display()))?;

    // Split YAML frontmatter delimited by lines that are exactly "---".
    let mut lines = text.lines();
    if lines.next().map(str::trim) != Some("---") {
        return Err(format!("{} has no YAML frontmatter", path.display()));
    }
    let mut model = "inherit".to_string();
    let mut body_lines: Vec<&str> = Vec::new();
    let mut in_frontmatter = true;
    for line in lines {
        if in_frontmatter {
            if line.trim() == "---" {
                in_frontmatter = false;
                continue;
            }
            if let Some(rest) = line.trim_start().strip_prefix("model:") {
                model = rest.trim().to_string();
            }
        } else {
            body_lines.push(line);
        }
    }
    if in_frontmatter {
        return Err(format!("{} frontmatter is not closed", path.display()));
    }
    let model_base = strip_params(&model);
    Ok(StageDef {
        name: name.to_string(),
        model,
        model_base,
        prompt: body_lines.join("\n").trim().to_string(),
    })
}

// ---------------------------------------------------------------------------
// Routing
// ---------------------------------------------------------------------------

fn build_route(level: u8, scan: bool, pentest: bool) -> Vec<&'static str> {
    let mut route = base_route(level);
    if scan {
        if let Some(idx) = route.iter().position(|s| *s == "judge") {
            route.insert(idx, "scan");
        }
    }
    if pentest {
        if let Some(idx) = route.iter().position(|s| *s == "validate") {
            route.insert(idx + 1, "pentest");
        }
    }
    route
}

// ---------------------------------------------------------------------------
// Verdict + level parsing
// ---------------------------------------------------------------------------

fn read_output(repo: &Path, stage: &str) -> String {
    match stage_output(stage) {
        Some(rel) => fs::read_to_string(repo.join("memory-bank").join(rel)).unwrap_or_default(),
        None => String::new(),
    }
}

/// First whitespace/`:`-separated token after a case-insensitive marker on the
/// first line that contains it. Used for "## Verdict:", "Route to:", "Type:".
fn field_after(text: &str, marker: &str) -> Option<String> {
    let low = text.to_lowercase();
    let m = marker.to_lowercase();
    let pos = low.find(&m)?;
    let rest = &text[pos + marker.len()..];
    rest.split(|c: char| c.is_whitespace())
        .find(|t| !t.is_empty())
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric() && c != '_').to_string())
        .filter(|s| !s.is_empty())
}

fn parse_verdict(text: &str) -> String {
    field_after(text, "## Verdict:")
        .map(|s| s.to_uppercase())
        .unwrap_or_else(|| "UNKNOWN".to_string())
}

fn parse_level(text: &str) -> Option<u8> {
    let low = text.to_lowercase();
    let pos = low.find("level:")?;
    text[pos + "level:".len()..]
        .chars()
        .find(|c| c.is_ascii_digit())
        .and_then(|c| c.to_digit(10))
        .map(|d| d as u8)
        .filter(|&d| (1..=4).contains(&d))
}

fn route_on_fail(report: &str) -> &'static str {
    if let Some(rt) = field_after(report, "Route to:") {
        match rt.to_lowercase().as_str() {
            "build" => return "build",
            "judge" => return "judge",
            "integrate" => return "integrate",
            _ => {}
        }
    }
    if let Some(ft) = field_after(report, "Type:") {
        match ft.to_lowercase().as_str() {
            "code_bug" | "build_errors" => return "build",
            "config_issue" | "integration_issue" => return "integrate",
            "quality_issue" | "quality_issues" => return "judge",
            _ => {}
        }
    }
    "build"
}

// ---------------------------------------------------------------------------
// Token accounting
// ---------------------------------------------------------------------------

/// [input, output, cache_read, cache_write]
type Tokens = [u64; 4];

fn fmt_tokens(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}k", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn estimate_cost(model_base: &str, t: &Tokens) -> Option<f64> {
    let p = price(model_base)?;
    Some(
        t[0] as f64 / 1e6 * p[0]
            + t[1] as f64 / 1e6 * p[1]
            + t[2] as f64 / 1e6 * p[2]
            + t[3] as f64 / 1e6 * p[3],
    )
}

// ---------------------------------------------------------------------------
// Stage execution via cursor-agent
// ---------------------------------------------------------------------------

/// Runs one stage. Returns best-effort token usage (all-zero if unreported).
fn run_stage(cfg: &Config, stage: &StageDef, extra: &str) -> Result<Tokens, String> {
    let mut prompt = format!(
        "{}\n\n---\nTASK: {}\nCURRENT STAGE: {}",
        stage.prompt,
        cfg.task,
        stage.name.to_uppercase()
    );
    if !extra.is_empty() {
        prompt.push_str(&format!(
            "\n\nREMEDIATION CONTEXT (fix these findings this pass):\n{extra}"
        ));
    }

    let mut cmd = Command::new("cursor-agent");
    cmd.arg("-p")
        .args(["--output-format", "json"])
        .arg("--force")
        .arg("--trust")
        .arg("--workspace")
        .arg(&cfg.repo)
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit());
    if !matches!(stage.model_base.as_str(), "" | "inherit") {
        cmd.args(["--model", &stage.model_base]);
    }
    cmd.arg(&prompt);

    let out = cmd
        .output()
        .map_err(|e| format!("[{}] failed to launch cursor-agent: {e}", stage.name))?;
    if !out.status.success() {
        return Err(format!(
            "[{}] cursor-agent exited with status {}",
            stage.name, out.status
        ));
    }

    // Best-effort usage parse: the CLI JSON may or may not carry a `usage` block.
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut tokens: Tokens = [0; 4];
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        let u = &v["usage"];
        let get = |k: &str| u[k].as_u64().unwrap_or(0);
        tokens = [
            get("inputTokens"),
            get("outputTokens"),
            get("cacheReadTokens"),
            get("cacheWriteTokens"),
        ];
    }
    Ok(tokens)
}

fn print_stage_tokens(name: &str, t: &Tokens) {
    let total: u64 = t.iter().sum();
    if total > 0 {
        println!(
            "  [{}] tokens: in {}, out {}, cache {}/{}, total {}",
            name.to_uppercase(),
            fmt_tokens(t[0]),
            fmt_tokens(t[1]),
            fmt_tokens(t[2]),
            fmt_tokens(t[3]),
            fmt_tokens(total)
        );
    } else {
        println!("  [{}] tokens: not reported by runtime", name.to_uppercase());
    }
}

fn print_usage_summary(by_model: &HashMap<String, Tokens>, runs: &HashMap<String, u32>) {
    if by_model.is_empty() {
        return;
    }
    println!("\n── Usage summary ──────────────────────────────");
    let mut grand: Tokens = [0; 4];
    let mut total_cost = 0.0;
    let mut any_unknown = false;
    let mut models: Vec<&String> = by_model.keys().collect();
    models.sort();
    for model in models {
        let t = &by_model[model];
        for i in 0..4 {
            grand[i] += t[i];
        }
        let tot: u64 = t.iter().sum();
        let cost_str = match estimate_cost(model, t) {
            Some(c) => {
                total_cost += c;
                format!("~${c:.2}")
            }
            None => {
                any_unknown = true;
                "no price set".to_string()
            }
        };
        println!(
            "  {:<26} {:>2} run(s)  {:>7} tok   {}",
            model, runs[model], fmt_tokens(tot), cost_str
        );
    }
    let grand_total: u64 = grand.iter().sum();
    let total_runs: u32 = runs.values().sum();
    println!(
        "  {:<26} {:>2} run(s)  {:>7} tok   ~${:.2}",
        "TOTAL", total_runs, fmt_tokens(grand_total), total_cost
    );
    println!(
        "  (in {}, out {}, cache {}/{})",
        fmt_tokens(grand[0]),
        fmt_tokens(grand[1]),
        fmt_tokens(grand[2]),
        fmt_tokens(grand[3])
    );
    println!(
        "  ⚠ Cost is a LOCAL ESTIMATE from price() (placeholder prices — edit them). \
         Cursor reports tokens only."
    );
    if any_unknown {
        println!("  ⚠ Some models had no price set; their tokens are excluded from the $ estimate.");
    }
}

// ---------------------------------------------------------------------------
// --print-config
// ---------------------------------------------------------------------------

fn print_config(cfg: &Config) -> Result<(), String> {
    println!("Stage -> model (from .cursor/agents/mb-*.md):\n");
    for name in ALL_STAGES {
        let s = load_stage(&cfg.repo, name)?;
        if s.model == s.model_base {
            println!("  {:<10} {}", name, s.model);
        } else {
            println!("  {:<10} {}  (CLI uses: {})", name, s.model, s.model_base);
        }
    }
    println!("\nLevel routes (default — security OFF):");
    for lvl in 1..=4u8 {
        let r: Vec<String> = base_route(lvl).iter().map(|s| s.to_uppercase()).collect();
        println!("  L{lvl}: {}", r.join(" -> "));
    }
    println!("\nWith --security (SCAN + PENTEST injected):");
    for lvl in 1..=4u8 {
        let r: Vec<String> = build_route(lvl, true, true)
            .iter()
            .map(|s| s.to_uppercase())
            .collect();
        println!("  L{lvl}: {}", r.join(" -> "));
    }
    println!("\nPrice table ($/1M tokens — PLACEHOLDERS): [input, output, cache_read, cache_write]");
    for m in ["claude-opus-5", "gpt-5.6-sol", "composer-2.5"] {
        if let Some(p) = price(m) {
            println!("  {:<26} {:?}", m, p);
        }
    }
    println!("  Note: Cursor reports tokens only; dollar figures are local estimates.");
    Ok(())
}

// ---------------------------------------------------------------------------
// Orchestration
// ---------------------------------------------------------------------------

fn run(cfg: &Config) -> Result<i32, String> {
    let mut by_model: HashMap<String, Tokens> = HashMap::new();
    let mut runs: HashMap<String, u32> = HashMap::new();
    let mut track = |stage: &StageDef, t: Tokens| {
        let acc = by_model.entry(stage.model_base.clone()).or_insert([0; 4]);
        for i in 0..4 {
            acc[i] += t[i];
        }
        *runs.entry(stage.model_base.clone()).or_insert(0) += 1;
    };

    // --- VAN first, to learn the complexity level ---
    let van = load_stage(&cfg.repo, "van")?;
    println!("[VAN] running on {} ...", van.model);
    let level: u8 = if cfg.dry_run {
        3
    } else {
        let t = run_stage(cfg, &van, "")?;
        track(&van, t);
        print_stage_tokens("van", &t);
        parse_level(&read_output(&cfg.repo, "van")).ok_or(
            "VAN did not write a parseable 'Level: [N]' line in memory-bank/projectbrief.md"
                .to_string(),
        )?
    };
    println!("  [VAN] Complete — Level {level} assessed");

    let route = build_route(level, cfg.scan, cfg.pentest);
    let sec: Vec<&str> = ["scan", "pentest"]
        .into_iter()
        .filter(|s| route.contains(s))
        .collect();
    let sec_note = if sec.is_empty() {
        "security: off".to_string()
    } else {
        format!("security: {}", sec.join(", "))
    };
    let printable: Vec<String> = route.iter().map(|s| s.to_uppercase()).collect();
    println!("\nRoute (Level {level}) ({sec_note}): {}\n", printable.join(" -> "));

    // Preload stage defs used by the route.
    let mut defs: HashMap<&str, StageDef> = HashMap::new();
    for &name in &route {
        if !defs.contains_key(name) {
            defs.insert(name, load_stage(&cfg.repo, name)?);
        }
    }

    let mut loop_counts: HashMap<String, u32> = HashMap::new();
    let mut pending: HashMap<String, String> = HashMap::new();
    let mut build_iters = 0u32;

    let mut i = 1usize; // VAN (index 0) already ran
    while i < route.len() {
        let name = route[i];
        let stage = &defs[name];
        println!("[{}] running on {} ...", name.to_uppercase(), stage.model);

        if cfg.dry_run {
            i += 1;
            continue;
        }

        let extra = pending.remove(name).unwrap_or_default();
        let t = run_stage(cfg, stage, &extra)?;
        track(stage, t);
        print_stage_tokens(name, &t);
        if name == "build" {
            build_iters += 1;
        }

        if !is_verdict_stage(name) {
            println!("  [{}] Complete", name.to_uppercase());
            i += 1;
            continue;
        }

        let report = read_output(&cfg.repo, name);
        match parse_verdict(&report).as_str() {
            "PASS" | "CONDITIONAL" => {
                println!("  [{}] Complete — {}", name.to_uppercase(), parse_verdict(&report));
                i += 1;
            }
            "FAIL" => {
                let target = route_on_fail(&report);
                let edge = format!("{name}->{target}");
                let c = loop_counts.entry(edge.clone()).or_insert(0);
                *c += 1;
                println!(
                    "  [{}] FAIL — looping back to {} (attempt {}/{})",
                    name.to_uppercase(),
                    target.to_uppercase(),
                    c,
                    cfg.max_loops
                );
                if *c > cfg.max_loops {
                    eprintln!(
                        "\nESCALATION: {edge} exceeded {} remediation loops. Stopping for human \
                         review. See memory-bank/{}",
                        cfg.max_loops,
                        stage_output(name).unwrap_or("")
                    );
                    print_usage_summary(&by_model, &runs);
                    return Ok(2);
                }
                pending.insert(target.to_string(), report);
                i = route.iter().position(|s| *s == target).unwrap_or(i);
            }
            other => {
                return Err(format!(
                    "could not parse a verdict from memory-bank/{} (got '{other}')",
                    stage_output(name).unwrap_or("")
                ));
            }
        }
    }

    println!("\nPipeline complete (Level {level}). Build iterations: {build_iters}.");
    println!("Artifacts are in memory-bank/.");
    print_usage_summary(&by_model, &runs);
    Ok(0)
}

fn main() {
    let mut cfg = match parse_args() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    };

    if !cfg.repo.join(".cursor").join("agents").is_dir() {
        eprintln!(
            "ERROR: {}/.cursor/agents not found. Run from the repo root or pass --repo.",
            cfg.repo.display()
        );
        std::process::exit(1);
    }

    if cfg.print_config {
        match print_config(&cfg) {
            Ok(()) => std::process::exit(0),
            Err(e) => {
                eprintln!("ERROR: {e}");
                std::process::exit(1);
            }
        }
    }

    cfg.task = match resolve_task(&cfg) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ERROR: {e}");
            std::process::exit(1);
        }
    };
    if cfg.task.is_empty() {
        eprintln!("ERROR: no task provided (use --task, --task-file, or pipe/type via stdin)");
        std::process::exit(1);
    }

    match run(&cfg) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("\nERROR: {e}");
            std::process::exit(2);
        }
    }
}
