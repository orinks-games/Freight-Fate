//! YouTube links in a personal playlist, played through a yt-dlp the player
//! installed themselves.
//!
//! Freight Fate does not ship yt-dlp (owner's call, 2026-10-30): it is
//! large, it needs updating whenever YouTube changes, and YouTube's terms do
//! not allow it, so a player who wants YouTube stations opts in by
//! installing it. The game looks for it in the Playlists folder first, then
//! on the PATH.
//!
//! A YouTube link is not a stream. yt-dlp turns it into one: for a
//! livestream, the HLS playlist of its smallest video variant, whose audio
//! Hearken's HLS reader demuxes and plays (the video packets are dropped).
//! That address belongs to this listener and expires after a few hours, so
//! it is asked for on every fresh tune and kept only until shortly before
//! the expiry it carries.
//!
//! yt-dlp runs on its own thread with a hard time limit, so a slow or stuck
//! run never holds the game loop; the answer comes back over a channel the
//! backend polls each frame. A run still in flight at shutdown is killed
//! when its time limit passes, never waited on.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;
use regex::Regex;
use url::Url;

/// The longest one yt-dlp run may take before it is killed and the tune
/// counts as failed.
pub(crate) const RESOLVE_TIMEOUT: Duration = Duration::from_secs(30);

/// How long before its stated expiry a resolved address is given up, so a
/// tune never connects to one that dies a minute later.
const EXPIRY_MARGIN_S: u64 = 10 * 60;

/// How long an address with no expiry of its own is reused.
const UNDATED_KEEP: Duration = Duration::from_secs(30 * 60);

/// The format yt-dlp is asked for: audio alone where YouTube offers it (an
/// ordinary video), else the smallest format with both (a livestream, whose
/// HLS variants all carry video).
const FORMAT: &str = "bestaudio[ext=m4a]/bestaudio/worst";

#[cfg(windows)]
const PROGRAM: &str = "yt-dlp.exe";
#[cfg(not(windows))]
const PROGRAM: &str = "yt-dlp";

/// Whether `url` is a YouTube page that yt-dlp has to turn into a stream.
pub fn is_youtube_url(url: &str) -> bool {
    let Ok(parsed) = Url::parse(url.trim()) else {
        return false;
    };
    if !matches!(parsed.scheme(), "http" | "https") {
        return false;
    }
    let host = parsed.host_str().unwrap_or_default().to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    matches!(
        host,
        "youtube.com" | "m.youtube.com" | "music.youtube.com" | "youtu.be"
    )
}

/// The yt-dlp to run: one in `dirs` (searched in order), else one on the
/// PATH.
pub fn find_yt_dlp(dirs: &[PathBuf]) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    dirs.iter()
        .cloned()
        .chain(std::env::split_paths(&path))
        .map(|dir| dir.join(PROGRAM))
        .find(|candidate| candidate.is_file())
}

/// Where the game looks for yt-dlp before the PATH: the Playlists folder,
/// beside the playlists that name YouTube links. On a Mac, Homebrew's
/// folders too, which an app opened from the Finder does not have on its
/// PATH; on Linux, where `pip install --user` puts it.
fn search_dirs() -> Vec<PathBuf> {
    let mut dirs =
        vec![ff_core::models::profile::data_dir().join(ff_core::radio::PLAYLISTS_DIR_NAME)];
    if cfg!(target_os = "macos") {
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
    }
    if cfg!(unix) {
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".local/bin"));
        }
    }
    dirs
}

/// The stream address in yt-dlp's `--get-url` output: its first line that
/// is a web address.
fn address_in(output: &str) -> Option<String> {
    output
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("https://") || line.starts_with("http://"))
        .map(str::to_string)
}

/// The Unix time a googlevideo address stops working, from the `expire`
/// it carries as a path segment (HLS manifests) or a query value (direct
/// media).
fn expires_at(address: &str) -> Option<u64> {
    static PATH: Lazy<Regex> = Lazy::new(|| Regex::new(r"/expire/(\d+)").expect("valid regex"));
    static QUERY: Lazy<Regex> = Lazy::new(|| Regex::new(r"[?&]expire=(\d+)").expect("valid regex"));
    PATH.captures(address)
        .or_else(|| QUERY.captures(address))
        .and_then(|caps| caps[1].parse().ok())
}

/// How long from `now_unix` an address may still be tuned to.
fn usable_for(address: &str, now_unix: u64) -> Duration {
    match expires_at(address) {
        Some(expiry) => Duration::from_secs(expiry.saturating_sub(now_unix + EXPIRY_MARGIN_S)),
        None => UNDATED_KEEP,
    }
}

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// One yt-dlp run in flight.
pub(crate) struct Resolve {
    answer: Receiver<Result<String, String>>,
}

impl Resolve {
    /// Ask `program` for the stream behind `page`, giving up after
    /// `timeout`. Returns at once; [`Resolve::poll`] collects the answer.
    pub(crate) fn start(program: &Path, page: &str, timeout: Duration) -> Self {
        let (tell, answer) = mpsc::sync_channel(1);
        let mut command = Command::new(program);
        command
            .args([
                "--no-warnings",
                "--no-playlist",
                "-f",
                FORMAT,
                "--get-url",
                "--",
            ])
            .arg(page)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        hide_console(&mut command);
        let spawned = command.spawn();
        let page = page.to_string();
        let worker = thread::Builder::new().name("yt-dlp".into()).spawn(move || {
            let result = spawned
                .map_err(|err| format!("could not start yt-dlp: {err}"))
                .and_then(|child| wait_for(child, timeout));
            if let Err(reason) = &result {
                log::warn!("YouTube link not resolved: {page} ({reason})");
            }
            // The receiver is gone when the tune was cancelled; nothing
            // is waiting for this answer.
            let _ = tell.send(result);
        });
        if let Err(err) = worker {
            log::warn!("Could not start the yt-dlp thread: {err}");
        }
        Self { answer }
    }

    /// The stream address once yt-dlp has answered: `None` while it runs.
    pub(crate) fn poll(&self) -> Option<Result<String, String>> {
        match self.answer.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err("yt-dlp thread ended".into())),
        }
    }
}

/// Wait for `child` within `timeout`, then read the address it printed.
fn wait_for(mut child: Child, timeout: Duration) -> Result<String, String> {
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(format!("yt-dlp took longer than {} s", timeout.as_secs()));
            }
            Ok(None) => thread::sleep(Duration::from_millis(50)),
            Err(err) => return Err(format!("could not wait for yt-dlp: {err}")),
        }
    };
    // The address is one short line, so the pipe never fills before exit.
    let mut out = String::new();
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    if !status.success() {
        let mut err = String::new();
        if let Some(mut stderr) = child.stderr.take() {
            let _ = stderr.read_to_string(&mut err);
        }
        let last = err.lines().rev().find(|line| !line.trim().is_empty());
        return Err(format!(
            "yt-dlp failed ({status}): {}",
            last.unwrap_or("no message")
        ));
    }
    address_in(&out).ok_or_else(|| "yt-dlp printed no address".to_string())
}

/// No console window flashes up over the game on Windows.
#[cfg(windows)]
fn hide_console(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_console(_command: &mut Command) {}

/// Resolved addresses by YouTube link, each kept until shortly before it
/// expires, so tuning back and forth along the dial does not run yt-dlp
/// every time.
#[derive(Default)]
pub(crate) struct Resolved {
    addresses: HashMap<String, (String, Instant)>,
}

impl Resolved {
    /// The address still usable for `page`, if one is known.
    pub(crate) fn get(&mut self, page: &str) -> Option<String> {
        let (address, until) = self.addresses.get(page)?;
        if Instant::now() < *until {
            return Some(address.clone());
        }
        self.addresses.remove(page);
        None
    }

    pub(crate) fn insert(&mut self, page: &str, address: String) {
        let until = Instant::now() + usable_for(&address, now_unix());
        self.addresses.insert(page.to_string(), (address, until));
    }

    /// Forget `page`'s address: it was tuned and did not play.
    pub(crate) fn forget(&mut self, page: &str) {
        self.addresses.remove(page);
    }
}

/// Whether the player has a yt-dlp the game can find.
pub fn yt_dlp_installed() -> bool {
    find_yt_dlp(&search_dirs()).is_some()
}

/// Whether `entries` hold a YouTube link that cannot play because the
/// player has no yt-dlp the game can find.
pub fn needs_yt_dlp(entries: &[String]) -> bool {
    entries.iter().any(|entry| is_youtube_url(entry)) && !yt_dlp_installed()
}

/// Start resolving `page` with the player's yt-dlp, or say why it cannot.
pub(crate) fn resolve(page: &str) -> Result<Resolve, String> {
    let program = find_yt_dlp(&search_dirs()).ok_or_else(|| {
        "yt-dlp is not installed (looked in the Playlists folder and on the PATH)".to_string()
    })?;
    log::info!("Resolving YouTube link with {}: {page}", program.display());
    Ok(Resolve::start(&program, page, RESOLVE_TIMEOUT))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn youtube_pages_are_recognised() {
        for page in [
            "https://www.youtube.com/watch?v=abc",
            "https://youtube.com/@SomeRadio/live",
            "https://m.youtube.com/live/abc",
            "https://youtu.be/abc",
            "http://music.youtube.com/watch?v=abc",
            "  https://WWW.YouTube.com/watch?v=abc  ",
        ] {
            assert!(is_youtube_url(page), "{page}");
        }
        for other in [
            "https://radio.example/stream.mp3",
            "https://notyoutube.com/watch?v=abc",
            "https://youtube.com.example/x",
            "C:\\music\\youtube.com.mp3",
            "ftp://youtube.com/x",
        ] {
            assert!(!is_youtube_url(other), "{other}");
        }
    }

    #[test]
    fn the_address_is_the_first_web_line() {
        let out = "\nhttps://manifest.googlevideo.com/api/manifest/hls_playlist/a/index.m3u8\nhttps://second\n";
        assert_eq!(
            address_in(out).as_deref(),
            Some("https://manifest.googlevideo.com/api/manifest/hls_playlist/a/index.m3u8")
        );
        assert_eq!(address_in("ERROR: nothing\n"), None);
    }

    #[test]
    fn an_address_is_kept_until_shortly_before_it_expires() {
        let hls = "https://manifest.googlevideo.com/api/manifest/hls_playlist/expire/1000000/ei/x/itag/91/index.m3u8";
        let direct = "https://rr1.googlevideo.com/videoplayback?expire=1000000&itag=140";
        assert_eq!(expires_at(hls), Some(1_000_000));
        assert_eq!(expires_at(direct), Some(1_000_000));
        let now = 1_000_000 - 3600;
        assert_eq!(
            usable_for(hls, now),
            Duration::from_secs(3600 - EXPIRY_MARGIN_S)
        );
        // Expiring inside the margin: not reused at all.
        assert_eq!(usable_for(direct, 1_000_000 - 60), Duration::ZERO);
        assert_eq!(usable_for("https://other/stream", now), UNDATED_KEEP);
    }

    #[test]
    fn a_resolved_address_can_be_forgotten() {
        let mut resolved = Resolved::default();
        resolved.insert("https://youtu.be/a", "https://stream/a".into());
        assert_eq!(
            resolved.get("https://youtu.be/a").as_deref(),
            Some("https://stream/a")
        );
        resolved.forget("https://youtu.be/a");
        assert_eq!(resolved.get("https://youtu.be/a"), None);
    }

    #[test]
    fn yt_dlp_is_found_in_a_listed_folder_first() {
        let dir = std::env::temp_dir().join(format!("ff-ytdlp-find-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_ne!(
            find_yt_dlp(std::slice::from_ref(&dir)).as_deref(),
            Some(dir.join(PROGRAM).as_path())
        );
        std::fs::write(dir.join(PROGRAM), b"").unwrap();
        assert_eq!(
            find_yt_dlp(std::slice::from_ref(&dir)),
            Some(dir.join(PROGRAM))
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[cfg(unix)]
    fn fake_yt_dlp(name: &str, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("ff-ytdlp-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("yt-dlp");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[cfg(unix)]
    fn answer(resolve: &Resolve) -> Result<String, String> {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(result) = resolve.poll() {
                return result;
            }
            assert!(Instant::now() < deadline, "no answer from the fake yt-dlp");
            thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_page_goes_to_yt_dlp_and_its_address_comes_back() {
        // Echoes its last argument, so the test sees the page arrived intact.
        let program = fake_yt_dlp(
            "ok",
            r#"for a; do last="$a"; done; echo "https://stream.example/live.m3u8?for=$last""#,
        );
        let resolve = Resolve::start(&program, "https://youtu.be/abc", Duration::from_secs(10));
        assert_eq!(
            answer(&resolve).as_deref(),
            Ok("https://stream.example/live.m3u8?for=https://youtu.be/abc")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_failing_yt_dlp_reports_its_last_error_line() {
        let program = fake_yt_dlp(
            "fail",
            "echo 'ERROR: This live event has ended.' >&2; exit 1",
        );
        let resolve = Resolve::start(&program, "https://youtu.be/abc", Duration::from_secs(10));
        let err = answer(&resolve).unwrap_err();
        assert!(err.contains("This live event has ended."), "{err}");
    }

    #[cfg(unix)]
    #[test]
    fn a_stuck_yt_dlp_is_killed_at_its_time_limit() {
        let program = fake_yt_dlp("stuck", "sleep 30");
        let started = Instant::now();
        let resolve = Resolve::start(&program, "https://youtu.be/abc", Duration::from_millis(300));
        let err = answer(&resolve).unwrap_err();
        assert!(err.contains("longer than"), "{err}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }
}
