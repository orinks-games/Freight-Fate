//! Problem reports: the game's logs sent ahead to orinks.net, and the report
//! form opened in the player's web browser with them already attached.
//!
//! The report type, its limits and the crash marker are Hearken's
//! (`hearken-report`); this module is what the game adds around them.
//!
//! * [`draft`] builds the report the game sends ahead: name, version,
//!   whether the last run crashed, and the session logs (`game.log`, and
//!   `game.prev.log` from the run before). The player writes the rest on the
//!   web page, so the description is left empty.
//! * [`RelaySender`] posts it through [`crate::net::request`], the one door
//!   to the network, gzipped, on its own tier with a minute to get through.
//!   The relay's address follows [`crate::online_presence::base_url`], so a
//!   sandbox that points Orinks calls nowhere sends its reports nowhere too.
//! * [`begin_session`] and [`end_session`] keep the crash marker. A marker
//!   still set when the game starts means the last run never exited
//!   normally, and the main menu offers to report it once.
//!
//! The screens are in `states::problem_report`.

use std::cell::RefCell;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use flate2::write::GzEncoder;
use flate2::Compression;
use hearken_report::Session;
pub use hearken_report::{Answer, Opened, Report, SendError, Sender};

use crate::net::{self, NetError, Tier};

/// The name the relay knows the game by.
pub const APP_NAME: &str = "Freight Fate";

/// The crash marker, in the data folder beside the settings.
pub const SESSION_MARKER: &str = "session.running";

/// The report relay: `/open` takes the logs ahead and answers with the page.
pub fn endpoint() -> String {
    format!("{}/api/problem-reports", crate::online_presence::base_url())
}

/// The bug-report form on GitHub, the fallback when the relay or the
/// browser cannot be reached.
pub fn github_issue_url() -> String {
    format!(
        "https://github.com/{}/issues/new?template=bug_report.yml",
        crate::updater::REPO
    )
}

/// The report the game sends ahead of the web form: no description yet,
/// the version, whether the last run crashed, and the session logs.
///
/// `game.log` is this run; `game.prev.log` is the run before, which after
/// a crash is the one that matters. Each keeps its last megabyte.
pub fn draft(crashed: bool) -> Report {
    let mut report = Report::new(APP_NAME, crate::app::version(), "").crashed(crashed);
    let Some(path) = crate::app::active_log_path() else {
        return report;
    };
    for log in [path.clone(), previous_log(&path)] {
        let Some(name) = log.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Ok(bytes) = fs::read(&log) {
            report = report.with_log(name, &String::from_utf8_lossy(&bytes));
        }
    }
    report
}

/// `game.prev.log` beside `game.log`, named the way `logging` rotates it.
fn previous_log(path: &Path) -> std::path::PathBuf {
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("game");
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    path.with_file_name(format!("{stem}.prev{ext}"))
}

// -- the sender --------------------------------------------------------------------

/// Posts a report's JSON to the relay through the game's own client.
///
/// Gzipped, since the body is mostly log text, and marked so with
/// `Content-Encoding`; the relay unpacks it. A 4xx comes back as an
/// answer for `hearken-report` to read (422 is a bad reply address, 429 a
/// limit), and only a failure to get any answer is an error.
#[derive(Debug, Default, Clone, Copy)]
pub struct RelaySender;

impl Sender for RelaySender {
    fn post(&self, endpoint: &str, json: &[u8]) -> Result<Answer, String> {
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        let body = encoder
            .write_all(json)
            .and_then(|()| encoder.finish())
            .map_err(|e| format!("The report could not be packed: {e}."))?;
        let extra = [("Content-Encoding".to_string(), "gzip".to_string())];
        let headers = crate::online_presence::request_headers(&extra, None);
        match net::request(
            Tier::ProblemReport,
            Some("POST"),
            endpoint,
            Some(&body),
            &headers,
        ) {
            Ok(reply) => Ok(Answer {
                status: reply.status,
                body: String::from_utf8_lossy(&reply.body).into_owned(),
            }),
            Err(NetError::Http { code, body }) => Ok(Answer {
                status: code,
                body: String::from_utf8_lossy(&body).into_owned(),
            }),
            Err(e) => Err(net::describe_error(&e)),
        }
    }
}

thread_local! {
    static SENDER_OVERRIDE: RefCell<Option<Arc<dyn Sender>>> = const { RefCell::new(None) };
}

/// The sender the report screen posts through: the relay over the real
/// network, unless a test installed a fake on this thread. Read on the
/// game thread and handed to the worker, so the fake follows the report.
pub fn sender() -> Arc<dyn Sender> {
    SENDER_OVERRIDE
        .with(|slot| slot.borrow().clone())
        .unwrap_or_else(|| Arc::new(RelaySender))
}

/// Replace [`sender`] on this thread for a test; `None` restores the relay.
pub fn set_sender_override(sender: Option<Arc<dyn Sender>>) {
    SENDER_OVERRIDE.with(|slot| *slot.borrow_mut() = sender);
}

// -- the crash marker --------------------------------------------------------------

static SESSION: Mutex<Option<Session>> = Mutex::new(None);

/// Set by [`begin_session`] when the last run never ended; taken once by
/// the app, which carries it to the main menu.
static CRASH_OFFER: AtomicBool = AtomicBool::new(false);

/// Mark this run as running, and find out whether the last one ended
/// without quitting. Called once, by the game's entry, as soon as the data
/// folder is known; a second copy of the game finds the marker locked and
/// never reads the first copy as a crash.
pub fn begin_session(data_dir: &Path) {
    let session = Session::begin(&data_dir.join(SESSION_MARKER));
    if session.ended_unexpectedly() {
        log::info!("the last run ended without quitting; the main menu offers a report");
        CRASH_OFFER.store(true, Ordering::SeqCst);
    }
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(session);
}

/// Record that this run exited normally. Idempotent; nothing to do when no
/// session was begun.
pub fn end_session() {
    if let Some(session) = SESSION.lock().unwrap_or_else(|e| e.into_inner()).take() {
        session.end();
    }
}

/// Whether the last run crashed and nobody has been offered a report yet.
/// True at most once per process.
pub fn take_crash_offer() -> bool {
    CRASH_OFFER.swap(false, Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_previous_log_sits_beside_the_current_one() {
        let prev = previous_log(Path::new("/tmp/logs/game.log"));
        assert_eq!(prev, Path::new("/tmp/logs/game.prev.log"));
        let bare = previous_log(Path::new("/tmp/logs/session"));
        assert_eq!(bare, Path::new("/tmp/logs/session.prev"));
    }

    #[test]
    fn the_endpoint_follows_the_site_override() {
        // The override is process-wide, so this test owns it only for a
        // moment and puts it back.
        let before = std::env::var_os(crate::online_presence::ONLINE_URL_ENV);
        std::env::set_var(
            crate::online_presence::ONLINE_URL_ENV,
            "https://sandbox-offline.invalid/",
        );
        let endpoint = endpoint();
        match before {
            Some(value) => std::env::set_var(crate::online_presence::ONLINE_URL_ENV, value),
            None => std::env::remove_var(crate::online_presence::ONLINE_URL_ENV),
        }
        assert_eq!(
            endpoint,
            "https://sandbox-offline.invalid/api/problem-reports"
        );
    }

    #[test]
    fn a_draft_names_the_game_and_carries_no_description() {
        let report = draft(true);
        assert_eq!(report.app, APP_NAME);
        assert_eq!(report.version, crate::app::version());
        assert!(report.crashed);
        assert!(report.description.is_empty());
        assert!(report.validate_sent_ahead().is_ok(), "{report:?}");
    }

    #[test]
    fn the_crash_marker_is_read_once_and_cleared_by_a_clean_exit() {
        let dir = tempfile::tempdir().unwrap();
        // A first run that never ended.
        fs::write(dir.path().join(SESSION_MARKER), "running").unwrap();
        begin_session(dir.path());
        assert!(take_crash_offer());
        assert!(!take_crash_offer(), "offered once");
        end_session();
        end_session();
        assert_eq!(
            fs::read_to_string(dir.path().join(SESSION_MARKER)).unwrap(),
            ""
        );
        begin_session(dir.path());
        assert!(!take_crash_offer(), "a clean exit is not a crash");
        end_session();
    }
}
