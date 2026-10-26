//! `playtest-watch`: watch a live manual playtest and report what is worth
//! interrupting for.
//!
//! A playtest is somebody driving a truck by ear for half an hour. Whoever
//! is running the session cannot read a log at the same time, and the
//! interesting lines (a crash, a cloud request from a session that was
//! supposed to be sandboxed, a warning nobody heard over the engine) are a
//! few among tens of thousands. This reads the session log as it is written
//! and prints one line per thing worth knowing, so a shell watching its
//! stdout gets a notification instead of a wall of text.
//!
//! Three duties, deliberately different in cadence:
//!
//! * **Now.** Errors, crashes, and any request that reached the site are
//!   printed the moment they land.
//! * **Occasionally.** Every few minutes, one check-in line: where the drive
//!   has got to and what was last spoken. It goes quiet on its own when
//!   nothing is happening and speaks up again when the road starts talking.
//! * **After.** When the game exits, one closing summary: how long, how
//!   much was spoken, what went wrong, and whether the sandbox stayed off
//!   the network.
//!
//! ```text
//! cargo tools playtest-watch
//! cargo tools playtest-watch --log logs/playtest.log --every 60
//! ```
//!
//! It exits when the session does, so a shell can wait on it.

use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use regex::Regex;
use serde_json::Value;

use crate::cli::{Args, Spec};
use crate::util::{self, Result};

/// Driving speech is near-continuous, so a gap this long means the session
/// is parked, in a menu, or over.
const DEFAULT_EVERY_S: f64 = 180.0;
const QUIET_REPORTS_BEFORE_HUSH: u32 = 1;
/// How long to keep reading after the game is gone: the last speech and the
/// shutdown lines are still being flushed while the process tears down.
const DRAIN: Duration = Duration::from_secs(3);
const POLL: Duration = Duration::from_secs(1);

/// "2026-08-20 19:44:01,123 LEVEL logger: message". The game logs under
/// module paths (`freight_fate::cloud_saves`).
static LINE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^(?P<time>\d{4}-\d\d-\d\d \d\d:\d\d:\d\d),\d+ (?P<level>[A-Z]+) (?P<logger>[\w.:]+): (?P<message>.*)$",
    )
    .expect("fixed pattern")
});

const TRANSCRIPT_LOGGER: &str = "freight_fate.transcript";

/// Loggers that have no business speaking during a sandboxed playtest. They
/// speak on failure (a refused upload, an unreachable site); a request that
/// succeeds writes nothing, so silence here is not proof that nothing was
/// sent. The sandbox's guarantee is that nothing can be published, and the
/// identity audit below is what proves that.
const NETWORK_LOGGERS: &[&str] = &[
    "freight_fate.cloud_saves",
    "freight_fate.online_presence",
    "freight_fate.online_activation",
];

/// Raw text that means a crash even outside the log format: a panic, or a
/// native crash report written straight into the file.
const CRASH_MARKERS: &[&str] = &[
    "panicked at",
    "stack backtrace:",
    "Windows fatal exception",
    "Traceback (most recent call last)",
];

/// The transcript's own bookkeeping: useful to count, never useful to quote
/// back as "the last thing the driver heard".
const TRANSCRIPT_NOISE: &[&str] = &["[ladder]", "[pacer]"];
/// Instrumentation: a line the truck wrote about what it did, with no
/// spoken line behind it.
const TRANSCRIPT_INSTRUMENTS: &[&str] = &["[jake]"];
/// Lines the game logs on every start no matter what.
const BENIGN_WARNINGS: &[&str] = &["Could not set default device"];

/// What `freight_fate::playtest::sandbox` counts as the driver identity and
/// as the settings that publish. Change one, change both, except
/// meaningful_play.json: the game rewrites it during any drive (nothing
/// uploadable with cloud saves off), so naming it would be a false alarm.
const IDENTITY_NAMES: &[&str] = &[
    "online.json",
    "online.token",
    "cloud_saves.json",
    "online-outbox.json",
];
const OFFLINE_SETTINGS: &[&str] = &["cloud_saves", "online_presence", "online_services"];

/// What the watcher believes about the live game process.
struct Session {
    path: PathBuf,
    pid: Option<u64>,
    log: Option<PathBuf>,
    sandbox: Option<PathBuf>,
    running: bool,
}

impl Session {
    fn new(path: PathBuf) -> Self {
        let mut session = Session {
            path,
            pid: None,
            log: None,
            sandbox: None,
            running: false,
        };
        session.refresh();
        session
    }

    fn refresh(&mut self) {
        let Some(data) = std::fs::read_to_string(&self.path)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            return;
        };
        self.pid = data.get("pid").and_then(Value::as_u64);
        if let Some(log) = data.get("log").and_then(Value::as_str) {
            self.log = Some(PathBuf::from(log));
        }
        if let Some(sandbox) = data.get("sandbox").and_then(Value::as_str) {
            self.sandbox = Some(PathBuf::from(sandbox));
        }
        self.running = data.get("running").is_some_and(|r| match r {
            Value::Bool(b) => *b,
            Value::Null => false,
            Value::Number(n) => n.as_f64() != Some(0.0),
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Object(o) => !o.is_empty(),
        });
    }

    /// True while the game is still running. Two signals, because neither
    /// alone is trustworthy: a crashed process does not write the session
    /// file's flag, and a pid can be missing on a session started some other
    /// way.
    fn alive(&mut self) -> bool {
        self.refresh();
        if !self.running {
            return false;
        }
        self.pid.is_none_or(pid_alive)
    }
}

fn pid_alive(pid: u64) -> bool {
    if cfg!(windows) {
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {pid}"), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(true)
    } else {
        std::process::Command::new("kill")
            .args(["-0", &pid.to_string()])
            .stderr(std::process::Stdio::piped())
            .output()
            // A refusal to signal (another user's process) still means alive.
            .map(|o| o.status.success() || String::from_utf8_lossy(&o.stderr).contains("ermi"))
            .unwrap_or(true)
    }
}

/// A log file read as it grows, surviving the game rotating it: the game
/// renames an existing log to `*.prev.log` at startup, and a file that
/// shrank is that rotation, so it is reopened.
struct Tail {
    path: PathBuf,
    reader: Option<BufReader<std::fs::File>>,
    pos: u64,
}

impl Tail {
    fn new(path: PathBuf) -> Self {
        Tail {
            path,
            reader: None,
            pos: 0,
        }
    }

    fn open(&mut self, at_end: bool) {
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return;
        };
        self.pos = if at_end {
            file.seek(SeekFrom::End(0)).unwrap_or(0)
        } else {
            0
        };
        self.reader = Some(BufReader::new(file));
    }

    fn lines(&mut self) -> Vec<String> {
        if self.reader.is_none() {
            self.open(true);
            if self.reader.is_none() {
                return Vec::new();
            }
        }
        let Ok(size) = self.path.metadata().map(|m| m.len()) else {
            return Vec::new();
        };
        if size < self.pos {
            self.reader = None;
            self.open(false);
        }
        let Some(reader) = self.reader.as_mut() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    self.pos += n as u64;
                    let line = String::from_utf8_lossy(&buf);
                    out.push(line.strip_suffix('\n').unwrap_or(&line).to_string());
                }
            }
        }
        out
    }
}

#[derive(Default)]
struct Watcher {
    every: f64,
    started: Option<Instant>,
    spoken: usize,
    silenced: usize,
    instrumented: usize,
    last_spoken: Option<String>,
    errors: Vec<String>,
    warnings: Vec<String>,
    network: Vec<String>,
    crashes: Vec<String>,
    crash_context: Vec<String>,
    since_checkin: usize,
    quiet_reports: u32,
    in_crash: bool,
}

impl Watcher {
    fn new(every: f64) -> Self {
        Watcher {
            every,
            started: Some(Instant::now()),
            ..Watcher::default()
        }
    }

    fn elapsed(&self) -> f64 {
        self.started.map_or(0.0, |s| s.elapsed().as_secs_f64())
    }

    /// Fold one log line in; return whatever must be said right now.
    fn consume(&mut self, raw: &str) -> Vec<String> {
        let trimmed = util::py_strip(raw);
        if CRASH_MARKERS.iter().any(|m| raw.contains(m)) {
            self.in_crash = true;
            self.crashes.push(trimmed.to_string());
            return vec![format!("CRASH: {trimmed}")];
        }
        let Some(caps) = LINE_RE.captures(raw) else {
            // A crash report's body belongs with the crash it explains;
            // counting each frame as its own error would report one failure
            // as a dozen.
            if self.in_crash && !trimmed.is_empty() {
                self.crash_context.push(trimmed.to_string());
            }
            return Vec::new();
        };
        self.in_crash = false;
        let level = &caps["level"];
        let logger = caps["logger"].replace("::", ".");
        let message = &caps["message"];

        if logger == TRANSCRIPT_LOGGER {
            if TRANSCRIPT_INSTRUMENTS
                .iter()
                .any(|p| message.starts_with(p))
            {
                self.instrumented += 1;
            } else if TRANSCRIPT_NOISE.iter().any(|p| message.starts_with(p)) {
                self.silenced += 1;
            } else {
                self.spoken += 1;
                self.since_checkin += 1;
                self.last_spoken = Some(
                    message
                        .strip_prefix("[event] ")
                        .unwrap_or(message)
                        .to_string(),
                );
            }
            return Vec::new();
        }
        if NETWORK_LOGGERS.iter().any(|n| logger.starts_with(n)) {
            self.network.push(format!("{logger}: {message}"));
            return vec![format!(
                "ONLINE trouble in a sandboxed session -- {logger}: {message}"
            )];
        }
        if level == "ERROR" || level == "CRITICAL" {
            self.errors.push(format!("{logger}: {message}"));
            return vec![format!("{level}: {logger}: {message}")];
        }
        if level == "WARNING" || level == "WARN" {
            if BENIGN_WARNINGS.iter().any(|b| message.contains(b)) {
                return Vec::new();
            }
            self.warnings.push(format!("{logger}: {message}"));
            return vec![format!("WARNING: {logger}: {message}")];
        }
        Vec::new()
    }

    /// One line on how the drive is going, or None to stay quiet.
    fn checkin(&mut self) -> Option<String> {
        let elapsed = duration(self.elapsed());
        if self.since_checkin == 0 {
            self.quiet_reports += 1;
            if self.quiet_reports > QUIET_REPORTS_BEFORE_HUSH {
                return None;
            }
            return Some(format!(
                "quiet -- nothing spoken in the last {} ({elapsed} in)",
                duration(self.every)
            ));
        }
        let said = std::mem::take(&mut self.since_checkin);
        self.quiet_reports = 0;
        let tail = self
            .last_spoken
            .as_ref()
            .map(|l| format!(" Last: \"{l}\""))
            .unwrap_or_default();
        Some(format!(
            "{elapsed} in, {} spoken since last check-in.{tail}",
            count(said, "line")
        ))
    }

    fn summary(&self, sandbox: Option<&Path>) -> Vec<String> {
        let mut out = vec![format!(
            "SESSION ENDED after {}: {} spoken, {} silenced by the rung or the pacer, {} \
             truck-state notes, {}, {}, {}.",
            duration(self.elapsed()),
            count(self.spoken, "line"),
            self.silenced,
            self.instrumented,
            count(self.crashes.len(), "crash"),
            count(self.errors.len(), "error"),
            count(self.warnings.len(), "warning"),
        )];
        out.extend(self.crashes.iter().take(3).map(|c| format!("  crash: {c}")));
        out.extend(
            self.crash_context
                .iter()
                .take(8)
                .map(|f| format!("    {f}")),
        );
        out.extend(self.errors.iter().take(5).map(|e| format!("  error: {e}")));
        if self.errors.len() > 5 {
            out.push(format!(
                "  ...and {} more errors in the log",
                self.errors.len() - 5
            ));
        }
        out.extend(
            self.warnings
                .iter()
                .take(5)
                .map(|w| format!("  warning: {w}")),
        );
        if self.warnings.len() > 5 {
            out.push(format!(
                "  ...and {} more warnings in the log",
                self.warnings.len() - 5
            ));
        }
        if self.network.is_empty() {
            // Not "nothing reached the site": these loggers speak on failure.
            out.push("  network: no online call reported a problem".into());
        } else {
            out.push(format!(
                "  NETWORK: {} logged a problem from a sandboxed session",
                count(self.network.len(), "online call")
            ));
        }
        if let Some(sandbox) = sandbox {
            let problems = audit(sandbox);
            if problems.is_empty() {
                out.push("  sandbox: still has no driver identity".into());
            } else {
                out.push("  SANDBOX BREACHED:".into());
                out.extend(problems.iter().map(|p| format!("    - {p}")));
            }
        }
        out
    }
}

/// True for a file that would carry the real account into a sandbox,
/// backup spellings included (`online.json.pre-clerk.bak`).
fn is_identity(name: &str) -> bool {
    IDENTITY_NAMES.contains(&name)
        || name.starts_with("online.json")
        || name.starts_with("online.token")
}

/// Every reason this sandbox could still reach the real account. Run after
/// the session: a drive can create files, so this checks that it did not
/// write an identity into a folder that started clean.
fn audit(sandbox: &Path) -> Vec<String> {
    let mut problems: Vec<String> = util::walk_files(sandbox)
        .iter()
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| is_identity(&n.to_string_lossy()))
        })
        .map(|p| format!("identity file in the sandbox: {}", util::rel(p, sandbox)))
        .collect();
    let settings = sandbox.join("settings.json");
    if settings.is_file() {
        let Some(data) = std::fs::read_to_string(&settings)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        else {
            problems.push("settings.json is unreadable; cannot confirm publishing is off".into());
            return problems;
        };
        for key in OFFLINE_SETTINGS {
            let on = data.get(key).is_some_and(|v| match v {
                Value::Bool(b) => *b,
                Value::Null => false,
                Value::Number(n) => n.as_f64() != Some(0.0),
                Value::String(s) => !s.is_empty(),
                _ => true,
            });
            if on {
                problems.push(format!("settings.json still has {key} on"));
            }
        }
    }
    problems
}

/// A span said the way a person watching a playtest would say it.
fn duration(seconds: f64) -> String {
    if seconds < 90.0 {
        format!("{} sec", util::py_round0(seconds))
    } else {
        format!("{} min", util::py_round0(seconds / 60.0))
    }
}

fn count(n: usize, noun: &str) -> String {
    if n == 1 {
        format!("{n} {noun}")
    } else if noun == "crash" {
        format!("{n} crashes")
    } else {
        format!("{n} {noun}s")
    }
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["log", "session", "every", "wait"],
            flags: &[],
        },
    )
    .map_err(|e| {
        format!("{e}\nusage: playtest-watch [--log FILE] [--session FILE] [--every S] [--wait S]")
    })?;
    let logs = util::repo_root().join("logs");
    let session_path = args
        .path("session")
        .unwrap_or_else(|| logs.join("playtest-session.json"));
    let every: f64 = args.number("every", DEFAULT_EVERY_S).map_err(|e| e.0)?;
    let wait: f64 = args.number("wait", 120.0).map_err(|e| e.0)?;
    let mut session = Session::new(session_path);

    let deadline = Instant::now() + Duration::from_secs_f64(wait);
    while !session.alive() && Instant::now() < deadline {
        std::thread::sleep(POLL);
    }
    if !session.alive() && args.path("log").is_none() {
        println!(
            "No playtest session is running (start one with: cargo run --release -p \
             freight-fate --bin freightfate -- --playtest-sandbox --launch)."
        );
        return Ok(1);
    }
    let log_path = args
        .path("log")
        .or_else(|| session.log.clone())
        .unwrap_or_else(|| logs.join("playtest-manual.log"));
    println!(
        "Watching {}; check-in every {}.",
        log_path.file_name().unwrap_or_default().to_string_lossy(),
        duration(every)
    );
    let mut tail = Tail::new(log_path);
    let mut watcher = Watcher::new(every);
    let every = Duration::from_secs_f64(every);
    let mut next_checkin = Instant::now() + every;
    let mut ended_at: Option<Instant> = None;
    loop {
        for raw in tail.lines() {
            for note in watcher.consume(&raw) {
                println!("{note}");
            }
        }
        let now = Instant::now();
        if now >= next_checkin {
            if let Some(note) = watcher.checkin() {
                println!("{note}");
            }
            next_checkin = now + every;
        }
        match ended_at {
            None if !session.alive() => ended_at = Some(now),
            Some(at) if now.duration_since(at) >= DRAIN => break,
            _ => {}
        }
        std::thread::sleep(POLL);
    }
    for raw in tail.lines() {
        for note in watcher.consume(&raw) {
            println!("{note}");
        }
    }
    for line in watcher.summary(session.sandbox.as_deref()) {
        println!("{line}");
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_sorted_into_the_right_channel() {
        let mut w = Watcher::new(60.0);
        assert!(w
            .consume("2026-08-20 19:44:01,123 INFO freight_fate::transcript: [event] Fuel low.")
            .is_empty());
        assert_eq!(w.last_spoken.as_deref(), Some("Fuel low."));
        assert!(w
            .consume("2026-08-20 19:44:01,123 INFO freight_fate::transcript: [pacer] x")
            .is_empty());
        let net = w.consume("2026-08-20 19:44:02,000 WARN freight_fate::cloud_saves: refused");
        assert_eq!(
            net,
            ["ONLINE trouble in a sandboxed session -- freight_fate.cloud_saves: refused"]
        );
        let err = w.consume("2026-08-20 19:44:03,000 ERROR freight_fate::audio: no device");
        assert_eq!(err, ["ERROR: freight_fate.audio: no device"]);
        assert_eq!(
            w.consume("thread 'main' panicked at src/x.rs:1:1:").len(),
            1
        );
        assert!(w.consume("   0: frame").is_empty());
        assert_eq!(w.crash_context, ["0: frame"]);
        assert_eq!((w.spoken, w.silenced, w.errors.len()), (1, 1, 1));
        assert_eq!(count(2, "crash"), "2 crashes");
        assert_eq!(duration(200.0), "3 min");
    }

    #[test]
    fn checkins_hush_after_one_quiet_report() {
        let mut w = Watcher::new(60.0);
        assert!(w.checkin().unwrap().starts_with("quiet"));
        assert!(w.checkin().is_none());
    }

    #[test]
    fn the_audit_finds_identities_and_publishing_settings() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("online.json.bak"), "{}").unwrap();
        std::fs::write(dir.path().join("settings.json"), r#"{"cloud_saves": true}"#).unwrap();
        assert_eq!(
            audit(dir.path()),
            [
                "identity file in the sandbox: online.json.bak",
                "settings.json still has cloud_saves on",
            ]
        );
    }
}
