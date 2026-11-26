//! Run a batched JSON script against `freightfate --agent-server` in one
//! process and one command invocation.
//!
//! Driving `--agent-server` interactively one MCP tool call per turn sends the
//! full conversation context back to the model on every step. Piping a JSON
//! array of steps into `cargo tools agent-script` executes the entire drive in
//! a single turn, prints only the relevant transcript lines, and checks inline
//! `expect` / `forbid` assertions.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::cli::{Args, Spec};
use crate::util::{repo_root, Result};

const USAGE: &str = "usage: agent-script [--bin PATH] [--filter PATTERNS] [--verbose] [--reset] [--json] <script.json | ->";

#[derive(Debug, Clone, Deserialize)]
struct ScriptStep {
    tool: String,
    #[serde(default)]
    args: Value,
    #[serde(default)]
    label: Option<String>,
    #[serde(default)]
    expect: Vec<String>,
    #[serde(default)]
    forbid: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
struct StepReport {
    index: usize,
    label: String,
    tool: String,
    lines: Vec<String>,
    failures: Vec<String>,
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["bin", "filter"],
            flags: &["verbose", "reset", "json", "help"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;

    if args.flag("help") {
        println!("{USAGE}");
        return Ok(0);
    }

    let source = args
        .positionals
        .first()
        .ok_or_else(|| format!("missing script path (or `-` for stdin)\n{USAGE}"))?;

    let raw = if source == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("reading stdin: {e}"))?;
        buf
    } else {
        std::fs::read_to_string(source).map_err(|e| format!("reading {source}: {e}"))?
    };

    let steps = parse_script(&raw)?;
    let filters = parse_filters(args.value("filter"));
    let verbose = args.flag("verbose");
    let json_out = args.flag("json");
    let reset = args.flag("reset");

    let root = repo_root();
    let bin = resolve_binary(&root, args.path("bin").as_deref())?;

    let mut cmd = Command::new(&bin);
    cmd.arg("--agent-server");
    if reset {
        cmd.arg("--reset");
    }
    cmd.current_dir(&root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    #[cfg(target_os = "linux")]
    if std::env::var_os("SDL_VIDEODRIVER").is_none() {
        cmd.env("SDL_VIDEODRIVER", "dummy");
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("starting {}: {e}", bin.display()))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| "failed to open agent-server stdin".to_string())?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| "failed to open agent-server stdout".to_string())?;
    let mut reader = BufReader::new(stdout);

    let mut next_id: u64 = 1;
    let init_params = json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": { "name": "ff-tools-agent-script", "version": "1" }
    });
    let _ = rpc(
        &mut stdin,
        &mut reader,
        &mut next_id,
        "initialize",
        init_params,
    )?;

    let mut reports = Vec::with_capacity(steps.len());
    let mut any_failed = false;
    let mut called_quit = false;

    for (idx, step) in steps.iter().enumerate() {
        if step.tool == "quit_game" {
            called_quit = true;
        }
        let tool_args = if step.args.is_null() {
            json!({})
        } else {
            step.args.clone()
        };
        let resp = rpc(
            &mut stdin,
            &mut reader,
            &mut next_id,
            "tools/call",
            json!({ "name": step.tool, "arguments": tool_args }),
        )?;
        let text = extract_tool_text(&resp);
        let mut failures = check_assertions(&text, &step.expect, &step.forbid);
        if is_tool_error(&resp) {
            failures.insert(0, format!("tool returned isError: {text}"));
        }
        if !failures.is_empty() {
            any_failed = true;
        }
        let lines = filter_lines(&text, &filters, verbose);
        let label = step
            .label
            .clone()
            .unwrap_or_else(|| format!("step {} ({})", idx + 1, step.tool));
        reports.push(StepReport {
            index: idx + 1,
            label,
            tool: step.tool.clone(),
            lines,
            failures,
        });
    }

    if !called_quit {
        let _ = rpc(
            &mut stdin,
            &mut reader,
            &mut next_id,
            "tools/call",
            json!({ "name": "quit_game", "arguments": {} }),
        );
    }

    drop(stdin);
    let _ = child.wait();

    if json_out {
        let rendered = serde_json::to_string_pretty(&reports)
            .map_err(|e| format!("serializing report JSON: {e}"))?;
        println!("{rendered}");
    } else {
        for r in &reports {
            if r.lines.is_empty() && r.failures.is_empty() && !verbose {
                continue;
            }
            println!("=== {} ===", r.label);
            for line in &r.lines {
                println!("{line}");
            }
            for fail in &r.failures {
                println!("FAIL: {fail}");
            }
        }
    }

    Ok(if any_failed { 1 } else { 0 })
}

fn parse_script(raw: &str) -> Result<Vec<ScriptStep>> {
    let steps: Vec<ScriptStep> =
        serde_json::from_str(raw).map_err(|e| format!("invalid script JSON: {e}"))?;
    if steps.is_empty() {
        return Err("script JSON array is empty".to_string());
    }
    Ok(steps)
}

fn parse_filters(raw: Option<&str>) -> Vec<String> {
    match raw {
        Some(s) => s
            .split(',')
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect(),
        None => Vec::new(),
    }
}

fn resolve_binary(root: &Path, explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        return Err(format!("binary not found at {}", path.display()));
    }
    let exe = if cfg!(windows) {
        "freightfate.exe"
    } else {
        "freightfate"
    };
    let candidates = [
        root.join("target").join("release").join(exe),
        root.join("target").join("debug").join(exe),
    ];
    let newest = candidates
        .into_iter()
        .filter(|c| c.is_file())
        .max_by_key(|c| {
            std::fs::metadata(c)
                .and_then(|m| m.modified())
                .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
        });
    if let Some(bin) = newest {
        return Ok(bin);
    }
    Err("no freightfate binary found in target/release or target/debug; build it first (`cargo build -p freight-fate --bin freightfate`) or pass --bin PATH".to_string())
}

fn rpc(
    stdin: &mut impl Write,
    reader: &mut impl BufRead,
    next_id: &mut u64,
    method: &str,
    params: Value,
) -> Result<Value> {
    let id = *next_id;
    *next_id += 1;
    let req = json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    });
    writeln!(stdin, "{req}").map_err(|e| format!("writing {method} request: {e}"))?;
    stdin
        .flush()
        .map_err(|e| format!("flushing {method} request: {e}"))?;

    let mut line = String::new();
    loop {
        line.clear();
        let n = reader
            .read_line(&mut line)
            .map_err(|e| format!("reading {method} response: {e}"))?;
        if n == 0 {
            return Err(format!(
                "agent-server closed stdout while waiting for response to {method} (id={id})"
            ));
        }
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let msg: Value =
            serde_json::from_str(trimmed).map_err(|e| format!("invalid JSON-RPC response: {e}"))?;
        if msg.get("id").and_then(Value::as_u64) == Some(id) {
            if let Some(err) = msg.get("error") {
                return Err(format!("{method} RPC error: {err}"));
            }
            return Ok(msg);
        }
    }
}

fn is_tool_error(resp: &Value) -> bool {
    resp.get("result")
        .and_then(|r| r.get("isError"))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

fn extract_tool_text(resp: &Value) -> String {
    let Some(items) = resp
        .get("result")
        .and_then(|r| r.get("content"))
        .and_then(Value::as_array)
    else {
        return String::new();
    };
    let mut out = String::new();
    for item in items {
        if let Some(text) = item.get("text").and_then(Value::as_str) {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(text);
        }
    }
    out
}

fn filter_lines(text: &str, filters: &[String], verbose: bool) -> Vec<String> {
    text.lines()
        .map(str::trim_end)
        .filter(|line| {
            if line.is_empty() {
                return false;
            }
            if !verbose && *line == "(quiet -- nothing new to hear)" {
                return false;
            }
            if filters.is_empty() {
                return true;
            }
            filters.iter().any(|pat| line.contains(pat.as_str()))
        })
        .map(str::to_string)
        .collect()
}

fn check_assertions(text: &str, expect: &[String], forbid: &[String]) -> Vec<String> {
    let mut failures = Vec::new();
    for needle in expect {
        if !text.contains(needle.as_str()) {
            failures.push(format!("expected output to contain {needle:?}"));
        }
    }
    for needle in forbid {
        if text.contains(needle.as_str()) {
            failures.push(format!("expected output NOT to contain {needle:?}"));
        }
    }
    failures
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_script_reads_steps_with_defaults() {
        let raw = r#"[
            {"tool": "scenario", "args": {"truck": "heavy"}},
            {"tool": "pedal", "args": {"key": "up", "seconds": 5.0}, "label": "accel", "expect": ["turbo_spool"], "forbid": ["error"]}
        ]"#;
        let steps = parse_script(raw).unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].tool, "scenario");
        assert!(steps[0].label.is_none());
        assert_eq!(steps[1].label.as_deref(), Some("accel"));
        assert_eq!(steps[1].expect, vec!["turbo_spool"]);
        assert_eq!(steps[1].forbid, vec!["error"]);
    }

    #[test]
    fn parse_script_rejects_empty_array() {
        assert!(parse_script("[]").is_err());
    }

    #[test]
    fn filter_lines_drops_quiet_and_applies_patterns() {
        let text = "(quiet -- nothing new to hear)\n[sound] vehicle/turbo_blowoff\n[bed] ambient/birds\n[engine] 600 rpm -> 1500 rpm\n";
        let filters = parse_filters(Some("[sound], [engine]"));
        let kept = filter_lines(text, &filters, false);
        assert_eq!(
            kept,
            vec![
                "[sound] vehicle/turbo_blowoff",
                "[engine] 600 rpm -> 1500 rpm"
            ]
        );
    }

    #[test]
    fn check_assertions_reports_missing_and_forbidden() {
        let text = "[sound] vehicle/turbo_spool\n[spoken] Error";
        let fails = check_assertions(
            text,
            &["turbo_spool".into(), "brake_set".into()],
            &["Error".into()],
        );
        assert_eq!(fails.len(), 2);
        assert!(fails[0].contains("brake_set"));
        assert!(fails[1].contains("Error"));
    }

    #[test]
    fn extract_tool_text_joins_content_blocks() {
        let resp = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "result": {
                "content": [
                    {"type": "text", "text": "line 1"},
                    {"type": "text", "text": "line 2"}
                ]
            }
        });
        assert_eq!(extract_tool_text(&resp), "line 1\nline 2");
    }

    #[test]
    fn is_tool_error_detects_mcp_error_flag() {
        let ok = json!({"jsonrpc": "2.0", "id": 1, "result": {"content": []}});
        let err = json!({"jsonrpc": "2.0", "id": 2, "result": {"isError": true, "content": [{"type": "text", "text": "unknown setting"}]}});
        assert!(!is_tool_error(&ok));
        assert!(is_tool_error(&err));
    }

    #[test]
    fn resolve_binary_prefers_newest_build() {
        let dir = tempfile::tempdir().unwrap();
        let exe = if cfg!(windows) {
            "freightfate.exe"
        } else {
            "freightfate"
        };
        let rel = dir.path().join("target").join("release").join(exe);
        let dbg = dir.path().join("target").join("debug").join(exe);
        std::fs::create_dir_all(rel.parent().unwrap()).unwrap();
        std::fs::create_dir_all(dbg.parent().unwrap()).unwrap();
        std::fs::write(&rel, b"old-release").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(25));
        std::fs::write(&dbg, b"fresh-debug").unwrap();
        assert_eq!(resolve_binary(dir.path(), None).unwrap(), dbg);
    }
}
