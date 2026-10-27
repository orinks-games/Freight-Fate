//! `check-radio-streams`: probe every real stream on the dial and report
//! which ones still play.
//!
//! The curated catalog and the imported directory tier both carry stream
//! URLs that were reachable when they were written down and may not be now:
//! a station folds, an account lapses, a host moves. A dead stream on the
//! dial costs a screen reader user a tune, a wait, and a fallback hand-off,
//! so the data should know which ones are gone.
//!
//! This only reports. It writes a health file that the imported catalog's
//! build reads to drop dead imported rows, and prints the curated
//! casualties for a person to mark `"supported": false` with a note.
//!
//! ```text
//! cargo tools check-radio-streams
//! cargo tools check-radio-streams --only curated --workers 8
//! cargo tools check-radio-streams --recheck-dead
//! ```
//!
//! A probe asks curl for the first couple of kilobytes and hangs up.
//! Verdicts: `ok` (audio came back, or a Shoutcast v1 `ICY 200 OK`
//! greeting); `playlist` (a .pls/.m3u, which the game resolves at play
//! time); `dead` (4xx/5xx, refused, unknown name, or silence until the
//! timeout; retried once, and re-asked at the Shoutcast `/;` mount, before
//! it counts).
//!
//! Certificates are not verified and old cipher suites are allowed: a long
//! tail of directory hosts stream fine behind self-signed or expired
//! certificates and older suites, and a strict client measures its own TLS
//! policy instead of the dial (the first sweep called KBOO, WBAI, KPFK and
//! two dozen more dead on that alone).
//!
//! The report accumulates. A row dropped from the built catalog is no
//! longer there to probe, so casualties are carried forward until
//! `--recheck-dead` sees one answer again.

use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};

use super::{catalog_rows, header, parallel, parse_headers, text, web_stream};
use crate::cli::{Args, Spec};
use crate::pyjson::{self, Object, Options, Value};
use crate::util::{self, Result};

/// A player's user agent, not a crawler's: several Icecast hosts answer a
/// bare client with 403 and a browser agent with audio.
const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) FreightFate/1.9 stream check";
const PROBE_BYTES: usize = 2048;

const AUDIO_TYPES: &[&str] = &[
    "audio/",
    "application/ogg",
    "application/octet-stream",
    "video/", // some Icecast hosts mislabel AAC as video/mp4
];
const PLAYLIST_TYPES: &[&str] = &[
    "audio/x-mpegurl",
    "audio/mpegurl",
    "audio/x-scpls",
    "application/pls+xml",
    "application/vnd.apple.mpegurl",
    "application/x-mpegurl",
];

struct Verdict {
    verdict: &'static str,
    detail: String,
    repaired_url: Option<String>,
}

fn verdict(verdict: &'static str, detail: impl Into<String>) -> Verdict {
    Verdict {
        verdict,
        detail: detail.into(),
        repaired_url: None,
    }
}

/// Split curl's `-D -` output into the header blocks (every response,
/// redirects included) and the body that follows the last one.
fn split_dump(out: &[u8]) -> (String, &[u8]) {
    let mut pos = 0;
    loop {
        let rest = &out[pos..];
        if !(rest.starts_with(b"HTTP/") || rest.starts_with(b"ICY")) {
            break;
        }
        let end = rest
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .map(|p| p + 4)
            .or_else(|| rest.windows(2).position(|w| w == b"\n\n").map(|p| p + 2));
        match end {
            Some(end) => pos += end,
            None => {
                pos = out.len();
                break;
            }
        }
    }
    (
        String::from_utf8_lossy(&out[..pos]).into_owned(),
        &out[pos..],
    )
}

/// What one probe of `url` heard.
fn probe(url: &str, timeout: f64) -> Verdict {
    let child = Command::new("curl")
        .args(["-s", "-S", "-k", "-L", "--max-redirs", "8"])
        .args(["--ciphers", "DEFAULT@SECLEVEL=1"])
        .args(["-m", &format!("{timeout}")])
        .args(["-A", USER_AGENT, "-H", "Icy-MetaData: 1"])
        .args(["-H", &format!("Range: bytes=0-{}", PROBE_BYTES - 1)])
        .args(["-H", "Accept: */*", "-D", "-", "-o", "-", url])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return verdict("dead", format!("could not run curl: {e}")),
    };
    let mut out = Vec::new();
    if let Some(mut stdout) = child.stdout.take() {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = stdout.read(&mut chunk) {
            if n == 0 {
                break;
            }
            out.extend_from_slice(&chunk[..n]);
            if split_dump(&out).1.len() >= PROBE_BYTES || out.len() > 65536 {
                break;
            }
        }
    }
    // Enough heard; an endless stream is hung up on here.
    let _ = child.kill();
    let stderr = child
        .wait_with_output()
        .map(|o| String::from_utf8_lossy(&o.stderr).trim().to_string())
        .unwrap_or_default();
    let (raw, body) = split_dump(&out);
    classify(&raw, &body[..body.len().min(PROBE_BYTES)], &stderr)
}

fn classify(raw: &str, body: &[u8], stderr: &str) -> Verdict {
    let (status, headers) = parse_headers(raw);
    if status == 0 {
        let reason = if stderr.is_empty() {
            "no answer"
        } else {
            stderr
        };
        return verdict("dead", reason.chars().take(120).collect::<String>());
    }
    if status >= 400 {
        return verdict("dead", format!("HTTP {status}"));
    }
    let shoutcast = raw
        .replace("\r\n", "\n")
        .split("\n\n")
        .filter(|b| !b.trim().is_empty())
        .last()
        .is_some_and(|b| b.trim_start().starts_with("ICY 200"));
    let content_type = header(&headers, "content-type")
        .unwrap_or("")
        .to_lowercase();
    if PLAYLIST_TYPES.iter().any(|k| content_type.starts_with(k)) {
        return verdict("playlist", content_type);
    }
    if AUDIO_TYPES.iter().any(|k| content_type.starts_with(k)) {
        return verdict("ok", content_type);
    }
    if shoutcast && content_type.is_empty() {
        return verdict("ok", "shoutcast ICY 200");
    }
    // A mount that names no type but hands over bytes is still playing; an
    // HTML error page dressed as 200 is not.
    if !body.is_empty()
        && !content_type.starts_with("text/")
        && !content_type.starts_with("application/json")
    {
        let shown = if content_type.is_empty() {
            "untyped bytes".to_string()
        } else {
            content_type
        };
        return verdict("ok", shown);
    }
    let shown = if content_type.is_empty() {
        "empty"
    } else {
        &content_type
    };
    verdict("dead", format!("not audio: {shown}"))
}

/// The `/;` form of a URL, which is where Shoutcast keeps the audio: its
/// bare root answers with an HTML status page.
fn shoutcast_mount(url: &str) -> String {
    format!("{}/;", url.trim_end_matches('/'))
}

/// A verdict, with a retry and a Shoutcast fallback.
fn probe_twice(url: &str, timeout: f64) -> Verdict {
    let result = probe(url, timeout);
    if result.verdict != "dead" {
        return result;
    }
    if result.detail.contains("not audio: text/html") {
        let mount = shoutcast_mount(url);
        let mut mounted = probe(&mount, timeout);
        if mounted.verdict != "dead" {
            mounted.detail = format!("{} (at {mount})", mounted.detail);
            mounted.repaired_url = Some(mount);
            return mounted;
        }
    }
    probe(url, timeout)
}

fn previous(path: &Path, key: &str) -> Vec<Object> {
    pyjson::load_path(path)
        .ok()
        .and_then(|v| v.get(key).and_then(Value::as_array).cloned())
        .unwrap_or_default()
        .into_iter()
        .filter_map(|v| v.as_object().cloned())
        .collect()
}

fn rows_to_check(only: &str) -> Result<Vec<Object>> {
    let mut rows = Vec::new();
    if only == "all" || only == "curated" {
        rows.extend(catalog_rows("radio_catalog.json", "curated")?);
    }
    if (only == "all" || only == "imported") && super::data_path("radio_imported.json").exists() {
        rows.extend(catalog_rows("radio_imported.json", "imported")?);
    }
    Ok(rows
        .into_iter()
        .filter(|r| web_stream(r).is_some())
        .collect())
}

fn pick(row: &Object, keys: &[&str]) -> Value {
    let mut out = Object::new();
    for key in keys {
        out.insert(*key, row.get(key).cloned().unwrap_or(Value::from("")));
    }
    Value::Object(out)
}

const USAGE: &str = "usage: check-radio-streams [--only all|curated|imported] [--workers N] \
[--timeout S] [--limit N] [--output FILE] [--recheck-dead]";

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["only", "workers", "timeout", "limit", "output"],
            flags: &["recheck-dead"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let only = args
        .choice("only", &["all", "curated", "imported"], "all")
        .map_err(|e| e.0)?;
    let workers: usize = args.number("workers", 12).map_err(|e| e.0)?;
    let timeout: f64 = args.number("timeout", 12.0).map_err(|e| e.0)?;
    let limit: usize = args.number("limit", 0).map_err(|e| e.0)?;
    let output = args
        .path("output")
        .unwrap_or_else(|| super::data_path("radio_stream_health.json"));

    let mut rows = rows_to_check(&only)?;
    if args.flag("recheck-dead") {
        if !output.exists() {
            return Err(format!("no health file to recheck: {}", output.display()));
        }
        let dead: HashSet<String> = previous(&output, "dead")
            .iter()
            .map(|r| text(r, "id").to_string())
            .collect();
        rows.retain(|r| dead.contains(text(r, "id")));
    }
    if limit > 0 {
        rows.truncate(limit);
    }
    println!("Probing {} streams with {workers} workers...", rows.len());
    let total = rows.len();
    let outcomes = parallel(
        &rows,
        workers,
        |row| probe_twice(web_stream(row).unwrap_or(""), timeout),
        |done| {
            if done % 250 == 0 {
                println!("  {done}/{total}");
            }
        },
    );
    let mut results: Vec<Object> = Vec::new();
    for (row, outcome) in rows.iter().zip(outcomes) {
        let mut result = Object::new();
        result.insert("id", row.get("id").cloned().unwrap_or(Value::Null));
        for key in ["name", "call_sign"] {
            result.insert(key, row.get(key).cloned().unwrap_or(Value::from("")));
        }
        result.insert("tier", Value::from(text(row, "_tier")));
        result.insert(
            "source_type",
            row.get("source_type").cloned().unwrap_or(Value::from("")),
        );
        result.insert("stream_url", Value::from(text(row, "stream_url")));
        result.insert("verdict", Value::from(outcome.verdict));
        result.insert("detail", Value::from(outcome.detail));
        if let Some(url) = outcome.repaired_url {
            result.insert("repaired_url", Value::from(url));
        }
        results.push(result);
    }

    // Carry forward casualties from earlier sweeps; a station leaves this
    // list only by being probed again and answering.
    let probed: HashSet<&str> = results.iter().map(|r| text(r, "id")).collect();
    let by_tier_name = |a: &Object, b: &Object| {
        (text(a, "tier"), text(a, "name")).cmp(&(text(b, "tier"), text(b, "name")))
    };
    let mut dead: Vec<Object> = results
        .iter()
        .filter(|r| text(r, "verdict") == "dead")
        .cloned()
        .chain(
            previous(&output, "dead")
                .into_iter()
                .filter(|r| !probed.contains(text(r, "id"))),
        )
        .collect();
    dead.sort_by(by_tier_name);
    // Stations that only answered at the Shoutcast mount: alive, but the URL
    // in the data points at the status page. An earlier repair is carried
    // forward unless this sweep repaired the same station again: the built
    // catalog already carries the repaired URL, so a repaired station probes
    // clean, and forgetting the repair would send the next build back to
    // the broken URL.
    let repaired_now: HashSet<&str> = results
        .iter()
        .filter(|r| r.contains_key("repaired_url"))
        .map(|r| text(r, "id"))
        .collect();
    let mut repaired: Vec<Object> = results
        .iter()
        .filter(|r| r.contains_key("repaired_url"))
        .cloned()
        .chain(
            previous(&output, "repaired")
                .into_iter()
                .filter(|r| !repaired_now.contains(text(r, "id"))),
        )
        .collect();
    repaired.sort_by(by_tier_name);
    let mut counts = Object::new();
    for name in ["ok", "playlist", "dead"] {
        let n = results
            .iter()
            .filter(|r| text(r, "verdict") == name)
            .count();
        counts.insert(name, Value::from(n));
    }
    println!(
        "\n{{'ok': {}, 'playlist': {}, 'dead': {}}}",
        counts.get("ok").map_or(0, |v| v.as_i64().unwrap_or(0)),
        counts
            .get("playlist")
            .map_or(0, |v| v.as_i64().unwrap_or(0)),
        counts.get("dead").map_or(0, |v| v.as_i64().unwrap_or(0)),
    );
    let in_tier = |tier: &str| dead.iter().filter(|r| text(r, "tier") == tier).count();
    println!("  curated dead:  {}", in_tier("curated"));
    println!("  imported dead: {}", in_tier("imported"));
    println!(
        "  alive only at the Shoutcast mount (URL wants editing): {}",
        repaired.len()
    );
    let report = crate::pyobj! {
        "schema" => 1i64,
        "notes" => "Stream reachability, written by `cargo tools check-radio-streams`. \
            The imported catalog's build drops the imported ids listed here; curated \
            casualties are marked supported: false by hand with a note. Re-probe with \
            --recheck-dead before deleting anything: a station can come back.",
        "counts" => counts,
        "repaired" => repaired
            .iter()
            .map(|r| pick(r, &["id", "name", "tier", "stream_url", "repaired_url"]))
            .collect::<Vec<_>>(),
        "dead" => dead
            .iter()
            .map(|r| pick(r, &["id", "name", "tier", "stream_url", "detail"]))
            .collect::<Vec<_>>(),
    };
    util::write_text(
        &output,
        &(pyjson::dumps(
            &Value::Object(report),
            &Options::indent(1).sorted().unicode(),
        ) + "\n"),
    )?;
    println!("Wrote {}", output.display());
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn answers_are_classified_like_a_player_would_hear_them() {
        let ok = "HTTP/1.1 200 OK\r\nContent-Type: audio/mpeg\r\n\r\n";
        assert_eq!(classify(ok, b"x", "").verdict, "ok");
        let html = "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n";
        assert_eq!(classify(html, b"<html>", "").detail, "not audio: text/html");
        let icy = "ICY 200 OK\r\nicy-name: X\r\n\r\n";
        assert_eq!(classify(icy, b"\xff\xfb", "").detail, "shoutcast ICY 200");
        assert_eq!(
            classify("HTTP/1.1 404 Not Found\r\n\r\n", b"", "").detail,
            "HTTP 404"
        );
        let pls = "HTTP/1.1 200 OK\r\nContent-Type: audio/x-scpls\r\n\r\n";
        assert_eq!(classify(pls, b"[playlist]", "").verdict, "playlist");
        assert_eq!(
            classify("", b"", "curl: (6) Could not resolve host").detail,
            "curl: (6) Could not resolve host"
        );
        assert_eq!(shoutcast_mount("http://a:8000/"), "http://a:8000/;");
        let (head, body) =
            split_dump(b"HTTP/1.1 302 x\r\nLocation: y\r\n\r\nICY 200 OK\r\n\r\nAUDIO");
        assert_eq!(body, b"AUDIO");
        assert_eq!(parse_headers(&head).0, 200);
    }
}
