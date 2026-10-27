//! `audit-radio-streams`: listen to every stream on the dial. Is it there,
//! is it the station, is it silent?
//!
//! `check-radio-streams` asks whether a host answers with audio. That misses
//! the failures a driver actually hears: a mount that answers and plays dead
//! air, a stream that connects and drops after two seconds, and an address
//! that plays perfectly well but is a different station from the one the
//! dial names. This records what each stream says about itself and decodes
//! a sample of it, so those can be judged.
//!
//! Per station it records the HTTP answer from curl (status, final address
//! after redirects, content type), the ICY headers the server names itself
//! with, the first `StreamTitle` in the in-band metadata when the server
//! sends one, and an ffmpeg decode of `--seconds` of audio: the codec, how
//! many seconds decoded, peak and mean level, and how much was silence.
//!
//! Verdicts: `ok`; `dead` (nothing decodable came back); `dropped` (it
//! connected but stopped well short of the sample); `silent` (it decoded,
//! but at a level or with a share of silence that is dead air). Whether a
//! stream is the station it claims to be is a judgement, not a verdict: the
//! report carries the evidence for a person to read.
//!
//! It needs `curl` and `ffmpeg` on PATH and an open network; the
//! radio-sweep workflow runs it on a GitHub runner. It only reports:
//!
//! ```text
//! cargo tools audit-radio-streams --only curated --output audit.json
//! cargo tools audit-radio-streams --shard 2/4 --output part2.json
//! ```

use std::collections::HashSet;
use std::io::Read;
use std::process::{Command, Stdio};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex::Regex;

use super::{catalog_rows, header, parallel, parse_headers, text, web_stream};
use crate::cli::{Args, Spec};
use crate::pyjson::{self, Object, Options, Value};
use crate::util::{self, Result};

/// What the game sends (`audio::hearken_radio::radio_user_agent`, with the
/// version left at the series). Shoutcast hosts answer a browser agent with
/// their HTML status page and a player agent with audio, so probing as a
/// browser calls live stations dead. A browser agent is tried second, and
/// only reported.
const GAME_AGENT: &str = "FreightFate/1.9 (+https://orinks.net/freight-fate)";
const BROWSER_AGENT: &str =
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) FreightFate/1.9 stream check";
const LONG_SAMPLE_S: f64 = 60.0;
const HEADER_SECONDS: u64 = 8;
/// Dead air: a peak this low is a carrier with nothing on it, or a sign-off
/// loop of room tone. Real programme peaks within a few dB of full scale.
const SILENT_PEAK_DB: f64 = -45.0;
const SILENCE_FLOOR_DB: i32 = -50;
const SILENCE_SHARE: f64 = 0.9;
/// A stream that connects and hands over less than this much of the sample
/// has dropped, which on the dial is a tune followed by a fallback hand-off.
const DROPPED_SHARE: f64 = 0.4;
const HLS_MARKERS: &[&str] = &["#EXT-X-", "#EXTINF"];

fn re(pattern: &str) -> Regex {
    Regex::new(pattern).expect("a fixed pattern compiles")
}

fn round1(x: f64) -> f64 {
    (x * 10.0).round_ties_even() / 10.0
}

fn clip(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn rows_to_audit(only: &str, recheck_dropped: bool) -> Result<Vec<Object>> {
    let mut rows = Vec::new();
    if only == "all" || only == "curated" {
        rows.extend(catalog_rows("radio_catalog.json", "curated")?);
    }
    if only == "all" || only == "imported" {
        rows.extend(catalog_rows("radio_imported.json", "imported")?);
        let health = super::data_path("radio_stream_health.json");
        if recheck_dropped && health.exists() {
            let present: HashSet<String> = rows.iter().map(|r| text(r, "id").to_string()).collect();
            let dead = pyjson::load_path(&health)?;
            for row in dead
                .get("dead")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let Some(row) = row.as_object() else { continue };
                if text(row, "tier") == "imported" && !present.contains(text(row, "id")) {
                    let mut row = row.clone();
                    row.insert("_tier", Value::from("imported_dropped"));
                    rows.push(row);
                }
            }
        }
    }
    Ok(rows
        .into_iter()
        .filter(|r| web_stream(r).is_some())
        .collect())
}

/// The first non-empty `StreamTitle` in the in-band ICY metadata.
fn stream_title(body: &[u8], metaint: usize) -> String {
    static TITLE: LazyLock<Regex> = LazyLock::new(|| re(r"(?s)StreamTitle='(.*?)';"));
    let mut offset = metaint;
    while offset < body.len() {
        let length = usize::from(body[offset]) * 16;
        let block = &body[offset + 1..(offset + 1 + length).min(body.len())];
        if length > 0 && block.len() == length {
            let mut end = block.len();
            while end > 0 && block[end - 1] == 0 {
                end -= 1;
            }
            let text = String::from_utf8_lossy(&block[..end]);
            if let Some(caps) = TITLE.captures(&text) {
                let title = util::py_strip(&caps[1]);
                if !title.is_empty() {
                    return title.to_string();
                }
            }
        }
        offset += 1 + length + metaint;
    }
    String::new()
}

fn playlist_target(body: &[u8]) -> String {
    let text = String::from_utf8_lossy(&body[..body.len().min(65536)]);
    for line in util::py_splitlines(&text) {
        let mut line = util::py_strip(line);
        if line.to_lowercase().starts_with("file") {
            if let Some((_, target)) = line.split_once('=') {
                line = util::py_strip(target);
            }
        }
        if line.starts_with("http://") || line.starts_with("https://") {
            return line.to_string();
        }
    }
    String::new()
}

/// Run a command to completion or `limit`, returning its exit code (None on
/// a timeout or failed start), standard output and standard error.
fn run_capture(cmd: &mut Command, limit: Duration) -> (Option<i32>, String, String) {
    let Ok(mut child) = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return (None, String::new(), String::new());
    };
    let reader = |pipe: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            if let Some(mut pipe) = pipe {
                let _ = pipe.read_to_end(&mut bytes);
            }
            String::from_utf8_lossy(&bytes).into_owned()
        })
    };
    let out = reader(
        child
            .stdout
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let err = reader(
        child
            .stderr
            .take()
            .map(|p| Box::new(p) as Box<dyn Read + Send>),
    );
    let deadline = Instant::now() + limit;
    let code = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.code(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    (
        code,
        out.join().unwrap_or_default(),
        err.join().unwrap_or_default(),
    )
}

fn curl_probe(url: &str, agent: &str, out: &mut Object) -> Result<()> {
    let dir = tempfile::tempdir().map_err(|e| e.to_string())?;
    let head_path = dir.path().join("head");
    let body_path = dir.path().join("body");
    let (code, stdout, stderr) = run_capture(
        Command::new("curl")
            .args(["-sS", "-L", "-k", "--max-redirs", "8"])
            .args(["-m", &HEADER_SECONDS.to_string(), "-A", agent])
            .args(["-H", "Icy-MetaData: 1", "-D"])
            .arg(&head_path)
            .arg("-o")
            .arg(&body_path)
            .args(["-w", "%{http_code}\t%{content_type}\t%{url_effective}", url]),
        Duration::from_secs(HEADER_SECONDS + 10),
    );
    if code.is_none() && stdout.is_empty() && stderr.is_empty() {
        return Err("curl did not run or did not finish".into());
    }
    let raw = std::fs::read(&head_path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let body = std::fs::read(&body_path).unwrap_or_default();
    let (status, headers) = parse_headers(&raw);
    let written: Vec<&str> = stdout.split('\t').collect();
    let final_url = written.get(2).copied().unwrap_or(url);
    let content_type = header(&headers, "content-type")
        .filter(|t| !t.is_empty())
        .or_else(|| written.get(1).copied())
        .unwrap_or("")
        .to_lowercase();
    out.insert("http", Value::from(u64::from(status)));
    out.insert("final_url", Value::from(final_url));
    out.insert("content_type", Value::from(content_type.as_str()));
    out.insert("bytes", Value::from(body.len()));
    let curl_error = if matches!(code, Some(0) | Some(28)) {
        String::new()
    } else {
        clip(util::py_strip(&stderr), 160)
    };
    out.insert("curl_error", Value::from(curl_error));
    for key in [
        "icy-name",
        "icy-description",
        "icy-genre",
        "icy-url",
        "icy-br",
    ] {
        if let Some(value) = header(&headers, key).filter(|v| !v.is_empty()) {
            out.insert(key.replace('-', "_"), Value::from(value));
        }
    }
    if let Some(metaint) = header(&headers, "icy-metaint")
        .filter(|m| !m.is_empty() && m.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|m| m.parse::<usize>().ok())
        .filter(|&m| m > 0)
    {
        out.insert("stream_title", Value::from(stream_title(&body, metaint)));
    }
    let head = &body[..body.len().min(4096)];
    let head = &head[head
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(head.len())..];
    let lower: Vec<u8> = head.iter().take(10).map(u8::to_ascii_lowercase).collect();
    if head.starts_with(b"#EXTM3U")
        || lower.starts_with(b"[playlist]")
        || content_type.contains("mpegurl")
        || content_type.contains("scpls")
    {
        let text = String::from_utf8_lossy(&body[..body.len().min(65536)]);
        if HLS_MARKERS.iter().any(|m| text.contains(m)) {
            out.insert("hls", Value::Bool(true));
        } else {
            out.insert("playlist_target", Value::from(playlist_target(&body)));
        }
    }
    Ok(())
}

fn ffmpeg_sample(url: &str, seconds: f64, agent: &str, result: &mut Object) {
    static AUDIO: LazyLock<Regex> = LazyLock::new(|| re(r"Stream #\S+.*?Audio: (.*)"));
    static META: LazyLock<Regex> = LazyLock::new(|| re(r"^\s{4,}([A-Za-z_\-]+)\s*:\s(.*)$"));
    static TIME: LazyLock<Regex> = LazyLock::new(|| re(r"time=(\d+):(\d+):(\d+\.\d+)"));
    static DURATION: LazyLock<Regex> = LazyLock::new(|| re(r"silence_duration: ([\d.]+)"));
    static START: LazyLock<Regex> = LazyLock::new(|| re(r"silence_start: (-?[\d.]+)"));
    let (_, _, log) = run_capture(
        Command::new("ffmpeg")
            .args(["-nostdin", "-hide_banner", "-loglevel", "info"])
            .args(["-user_agent", agent, "-rw_timeout", "15000000"])
            .args([
                "-t",
                &crate::pyjson::float_repr(seconds),
                "-i",
                url,
                "-vn",
                "-af",
            ])
            .arg(format!(
                "silencedetect=noise={SILENCE_FLOOR_DB}dB:d=2,volumedetect"
            ))
            .args(["-f", "null", "-"]),
        Duration::from_secs_f64(seconds + 45.0),
    );
    if let Some(caps) = AUDIO.captures(&log) {
        result.insert("codec", Value::from(clip(util::py_strip(&caps[1]), 80)));
    }
    let wanted = [
        "icy-name",
        "icy-description",
        "icy-genre",
        "icy-url",
        "streamtitle",
        "title",
        "station",
    ];
    let mut meta = Object::new();
    for line in util::py_splitlines(&log) {
        if let Some(caps) = META.captures(line) {
            let key = caps[1].to_lowercase();
            if wanted.contains(&key.as_str()) && !meta.contains_key(&key) {
                meta.insert(key, Value::from(util::py_strip(&caps[2])));
            }
        }
    }
    if !meta.is_empty() {
        result.insert("ffmpeg_meta", Value::Object(meta));
    }
    let decoded = TIME.captures_iter(&log).last().map_or(0.0, |caps| {
        let n = |k: usize| caps[k].parse::<f64>().unwrap_or(0.0);
        n(1) * 3600.0 + n(2) * 60.0 + n(3)
    });
    result.insert("decoded_s", Value::Float(round1(decoded)));
    for key in ["mean_volume", "max_volume"] {
        let pattern = re(&format!(r"{key}: (-?[\d.]+|-inf) dB"));
        if let Some(caps) = pattern.captures(&log) {
            let level = if &caps[1] == "-inf" {
                -200.0
            } else {
                caps[1].parse().unwrap_or(-200.0)
            };
            result.insert(key, Value::Float(level));
        }
    }
    let mut silence: f64 = DURATION
        .captures_iter(&log)
        .filter_map(|c| c[1].parse::<f64>().ok())
        .sum();
    let starts: Vec<f64> = START
        .captures_iter(&log)
        .filter_map(|c| c[1].parse().ok())
        .collect();
    let ends = log.matches("silence_end:").count();
    if starts.len() > ends && decoded > 0.0 {
        if let Some(last) = starts.last() {
            silence += (decoded - last).max(0.0);
        }
    }
    result.insert("silence_s", Value::Float(round1(silence)));
    if decoded == 0.0 {
        let lines = util::py_splitlines(&log);
        let errors: Vec<&str> = lines
            .iter()
            .filter(|l| {
                l.contains("rror") || l.contains("HTTP") || l.to_lowercase().contains("failed")
            })
            .map(|l| util::py_strip(l))
            .collect();
        let all = util::py_splitlines(util::py_strip(&log));
        let tail = errors.last().or(all.last()).copied().unwrap_or("no output");
        result.insert("ffmpeg_error", Value::from(clip(tail, 160)));
    }
}

fn number(out: &Object, key: &str, default: f64) -> f64 {
    out.get(key).and_then(Value::as_f64).unwrap_or(default)
}

fn listen(url: &str, seconds: f64, agent: &str) -> Object {
    let mut out = Object::new();
    if let Err(error) = curl_probe(url, agent, &mut out) {
        out.insert("curl_error", Value::from(clip(&error, 160)));
    }
    let target = out
        .get("playlist_target")
        .and_then(Value::as_str)
        .filter(|t| !t.is_empty())
        .unwrap_or(url)
        .to_string();
    ffmpeg_sample(&target, seconds, agent, &mut out);
    let decoded = number(&out, "decoded_s", 0.0);
    let verdict = if decoded <= 0.5 {
        "dead"
    } else if decoded < seconds * DROPPED_SHARE {
        "dropped"
    } else if number(&out, "max_volume", 0.0) <= SILENT_PEAK_DB
        || number(&out, "silence_s", 0.0) >= decoded * SILENCE_SHARE
    {
        "silent"
    } else {
        "ok"
    };
    out.insert("verdict", Value::from(verdict));
    out
}

/// Addresses the same station is likely to answer at instead. A
/// StreamTheWorld numbered edge is one server of a pool that comes and
/// goes; the player-services redirect hands out a live one. A Shoutcast
/// root wants its `/;` mount. A host can drop one scheme and keep the
/// other.
fn repair_candidates(url: &str) -> Vec<String> {
    static EDGE: LazyLock<Regex> = LazyLock::new(|| {
        re(
            r"^https?://\d+\.live\.streamtheworld\.com(?::\d+)?/([A-Za-z0-9_]+?)(?:_SC)?(?:\.(mp3|aac))?/?$",
        )
    });
    static NUMBERED: LazyLock<Regex> = LazyLock::new(|| re(r"_\d+$"));
    let mut out = Vec::new();
    if let Some(caps) = EDGE.captures(url) {
        let mount = caps[1].to_string();
        let ext = caps.get(2).map_or("mp3", |m| m.as_str());
        let base = "https://playerservices.streamtheworld.com/api/livestream-redirect/";
        let bare = NUMBERED.replace(&mount, "").into_owned();
        let mut names = vec![mount];
        if !names.contains(&bare) {
            names.push(bare);
        }
        for name in names {
            out.push(format!("{base}{name}.{ext}"));
            out.push(format!("{base}{name}AAC.aac"));
        }
    }
    if !url.trim_end_matches('/').ends_with(';') {
        out.push(format!("{}/;", url.trim_end_matches('/')));
    }
    if let Some(rest) = url.strip_prefix("http://") {
        out.push(format!("https://{rest}"));
    } else if let Some(rest) = url.strip_prefix("https://") {
        out.push(format!("http://{rest}"));
    }
    out
}

fn audit(row: &Object, seconds: f64) -> Object {
    let url = text(row, "stream_url").to_string();
    let mut out = Object::new();
    out.insert("id", row.get("id").cloned().unwrap_or(Value::Null));
    out.insert("tier", Value::from(text(row, "_tier")));
    for key in ["name", "call_sign", "station_type", "source_type"] {
        out.insert(key, row.get(key).cloned().unwrap_or(Value::from("")));
    }
    out.insert(
        "supported",
        row.get("supported").cloned().unwrap_or(Value::Bool(true)),
    );
    out.insert("stream_url", Value::from(url.as_str()));
    for (k, v) in listen(&url, seconds, GAME_AGENT).0 {
        out.insert(k, v);
    }
    if text(&out, "verdict") == "silent" {
        // Twenty-five quiet seconds can be a pause between programmes; a
        // minute of it is dead air.
        let longer = listen(&url, LONG_SAMPLE_S, GAME_AGENT);
        let mut long = Object::new();
        for key in ["verdict", "decoded_s", "max_volume", "silence_s"] {
            long.insert(key, longer.get(key).cloned().unwrap_or(Value::Null));
        }
        out.insert("long_sample", Value::Object(long));
        out.insert(
            "verdict",
            longer.get("verdict").cloned().unwrap_or(Value::Null),
        );
    }
    if matches!(text(&out, "verdict"), "dead" | "dropped") {
        let mut fixed = false;
        for candidate in repair_candidates(&url) {
            let tried = listen(&candidate, seconds, GAME_AGENT);
            if text(&tried, "verdict") == "ok" {
                out.insert("repaired_url", Value::from(candidate));
                out.insert("repaired", Value::Object(tried));
                fixed = true;
                break;
            }
        }
        if !fixed {
            let browser = listen(&url, seconds, BROWSER_AGENT);
            out.insert(
                "browser_verdict",
                browser.get("verdict").cloned().unwrap_or(Value::Null),
            );
        }
    }
    out
}

const USAGE: &str = "usage: audit-radio-streams --output FILE [--only all|curated|imported] \
[--recheck-dropped] [--ids A,B] [--urls FILE] [--shard K/N] [--workers N] [--seconds S]";

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &[
                "only", "ids", "urls", "shard", "workers", "seconds", "output",
            ],
            flags: &["recheck-dropped"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let output = args
        .path("output")
        .ok_or_else(|| format!("--output is required\n{USAGE}"))?;
    let only = args
        .choice("only", &["all", "curated", "imported"], "all")
        .map_err(|e| e.0)?;
    let workers: usize = args.number("workers", 24).map_err(|e| e.0)?;
    let seconds: f64 = args.number("seconds", 25.0).map_err(|e| e.0)?;
    let ids = args.string("ids", "");
    let mut rows = if ids.is_empty() {
        rows_to_audit(&only, args.flag("recheck-dropped"))?
    } else {
        let wanted: HashSet<&str> = ids.split(',').collect();
        rows_to_audit("all", true)?
            .into_iter()
            .filter(|r| wanted.contains(text(r, "id")))
            .collect()
    };
    if let Some(urls) = args.path("urls") {
        rows = pyjson::load_path(&urls)?
            .as_array()
            .ok_or("--urls must name a JSON list")?
            .iter()
            .filter_map(Value::as_object)
            .map(|row| {
                let mut row = row.clone();
                row.insert("_tier", Value::from("candidate"));
                row
            })
            .collect();
    }
    let shard = args.string("shard", "1/1");
    let (part, total) = shard
        .split_once('/')
        .and_then(|(k, n)| Some((k.parse::<usize>().ok()?, n.parse::<usize>().ok()?)))
        .filter(|&(k, n)| k >= 1 && n >= 1)
        .ok_or_else(|| format!("--shard must be K/N, not {shard:?}"))?;
    let rows: Vec<Object> = rows.into_iter().skip(part - 1).step_by(total).collect();
    println!("Auditing {} streams with {workers} workers...", rows.len());
    let count = rows.len();
    let mut results = parallel(
        &rows,
        workers,
        |row| audit(row, seconds),
        |done| {
            if done % 100 == 0 {
                println!("  {done}/{count}");
            }
        },
    );
    results.sort_by(|a, b| {
        let key = |r: &Object| (text(r, "tier").to_string(), r.get("id").map(Value::py_str));
        key(a).cmp(&key(b))
    });
    let mut counts = Object::new();
    for r in &results {
        let verdict = text(r, "verdict").to_string();
        let n = counts.get(&verdict).and_then(Value::as_i64).unwrap_or(0);
        counts.insert(verdict, Value::from(n + 1));
    }
    println!(
        "{}",
        pyjson::dumps(&Value::Object(counts.clone()), &Options::compact())
    );
    let report = crate::pyobj! {
        "counts" => counts,
        "results" => results.into_iter().map(Value::Object).collect::<Vec<_>>(),
    };
    util::write_text(
        &output,
        &(pyjson::dumps(&Value::Object(report), &Options::indent(1).unicode()) + "\n"),
    )?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_are_tried_in_order() {
        assert_eq!(
            repair_candidates("https://14523.live.streamtheworld.com/KSTPFM_SC"),
            [
                "https://playerservices.streamtheworld.com/api/livestream-redirect/KSTPFM.mp3",
                "https://playerservices.streamtheworld.com/api/livestream-redirect/KSTPFMAAC.aac",
                "https://14523.live.streamtheworld.com/KSTPFM_SC/;",
                "http://14523.live.streamtheworld.com/KSTPFM_SC",
            ]
        );
        assert_eq!(repair_candidates("http://a/;"), ["https://a/;"]);
    }

    #[test]
    fn in_band_titles_and_playlist_targets_are_read() {
        let mut body = vec![b'x'; 4];
        let meta = b"StreamTitle='Song A';";
        body.push(2);
        body.extend_from_slice(meta);
        body.extend(std::iter::repeat_n(0u8, 32 - meta.len()));
        assert_eq!(stream_title(&body, 4), "Song A");
        assert_eq!(
            playlist_target(b"[playlist]\nFile1=http://s.example/live\n"),
            "http://s.example/live"
        );
    }
}
