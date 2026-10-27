//! Session logging (`_configure_logging` / `active_log_path` in
//! `freight_fate/app.py`): console logging from source, a fresh log file in
//! the packaged game, the previous run kept as `game.prev.log`, and the
//! `freight_fate.transcript` lines interleaved into the same file.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use log::LevelFilter;

/// Where this session's log actually ended up, or None when nothing is
/// being written to disk (a source checkout with no explicit log file, or a
/// folder the game could not write to). Recorded by `configure_logging`
/// rather than derived again later, so the settings screen reports the real
/// file instead of the one the game meant to open.
static LOG_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
static CONFIGURED: OnceLock<()> = OnceLock::new();

/// The log file this session is writing, or None when there is none.
pub fn active_log_path() -> Option<PathBuf> {
    LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The folder holding `game.log`, in words for the bug-report prompt.
///
/// The folder this session actually writes, when there is one. Otherwise the
/// packaged layout: beside the game, except on macOS, where the log lives
/// with the player's data. Saying "next to the game" on a Mac sent testers to
/// the folder the 1.8 build wrote, so they attached weeks-old logs from a
/// different version (issue 266).
pub fn log_folder_words() -> String {
    if let Some(dir) = active_log_path().as_deref().and_then(Path::parent) {
        return dir.display().to_string();
    }
    if cfg!(target_os = "macos") {
        "the logs folder inside Library, Application Support, FreightFate, in your home folder"
            .to_string()
    } else {
        "the logs folder next to the game".to_string()
    }
}

/// `FREIGHT_FATE_LOG` level names (Python logging names) to a filter.
pub fn parse_level(name: &str) -> LevelFilter {
    match name.trim().to_ascii_uppercase().as_str() {
        "DEBUG" | "TRACE" => LevelFilter::Debug,
        "INFO" => LevelFilter::Info,
        "WARNING" | "WARN" => LevelFilter::Warn,
        "ERROR" | "CRITICAL" | "FATAL" => LevelFilter::Error,
        "NOTSET" => LevelFilter::Trace,
        _ => LevelFilter::Info,
    }
}

/// Console logging from source; a fresh log file in the packaged game.
///
/// The windowed build has no console, so without a file every warning --
/// update failures especially -- vanishes. Portable builds keep the log in
/// the game folder; macOS keeps it with the player's data under Application
/// Support because an installed app bundle is read-only. An explicit
/// `FREIGHT_FATE_LOG_FILE` (set for
/// playtests/observation) forces file output and an INFO default even from a
/// source checkout, so a session can be reviewed after the fact without
/// streaming to a console. Safe to call more than once; only the first call
/// configures anything.
pub fn configure_logging() {
    if CONFIGURED.set(()).is_err() {
        return;
    }
    let packaged = crate::updater::is_frozen();
    let explicit = std::env::var("FREIGHT_FATE_LOG_FILE")
        .ok()
        .filter(|s| !s.is_empty());
    let default_level = if packaged || explicit.is_some() {
        "INFO"
    } else {
        "WARNING"
    };
    let level = std::env::var("FREIGHT_FATE_LOG")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_level.to_string());
    let level = parse_level(&level);

    let log_path: Option<PathBuf> = match explicit {
        Some(path) => Some(PathBuf::from(path)),
        None if packaged => Some(packaged_log_path(
            cfg!(any(target_os = "macos", target_os = "ios")),
            &ff_core::settings::game_root(),
            &ff_core::models::profile::data_dir(),
        )),
        None => None,
    };
    let file = log_path.as_deref().and_then(open_log_file);
    if file.is_some() {
        *LOG_FILE.lock().unwrap_or_else(|e| e.into_inner()) = log_path;
    }

    let mut builder = env_logger::Builder::new();
    builder.filter_level(level);
    // "%(asctime)s %(levelname)s %(name)s: %(message)s"
    builder.format(|buf, record| {
        writeln!(
            buf,
            "{} {} {}: {}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S,%3f"),
            python_level_name(record.level()),
            record.target(),
            record.args()
        )
    });
    // A native crash or an abort never reaches the panic hook below; this
    // gets it into the same file.
    #[cfg(windows)]
    if let Some(file) = &file {
        super::crash_log::install(file);
    }
    match file {
        Some(file) => {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
        }
        None => {
            builder.target(env_logger::Target::Stderr);
        }
    }
    let _ = builder.try_init();

    // Python's faulthandler wrote native crash tracebacks into the log as
    // the process died; the nearest Rust equivalent is the panic hook, so a
    // panic's message and location reach the file before the default hook
    // prints it.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("Fatal error: {info}");
        default_hook(info);
    }));
}

fn python_level_name(level: log::Level) -> &'static str {
    match level {
        log::Level::Error => "ERROR",
        log::Level::Warn => "WARNING",
        log::Level::Info => "INFO",
        log::Level::Debug => "DEBUG",
        log::Level::Trace => "DEBUG",
    }
}

/// Create the log file, keeping the previous run's as `<stem>.prev<ext>`:
/// after a crash the player relaunches the game to report it, and that
/// relaunch must not wipe the evidence. `None` on an unwritable disk:
/// console-only is the best we can do.
fn open_log_file(path: &Path) -> Option<File> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok()?;
    }
    if path.exists() {
        // Rotation is best-effort; a locked file still gets a fresh log.
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("game");
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        let prev = path.with_file_name(format!("{stem}.prev{ext}"));
        let _ = fs::rename(path, prev);
    }
    File::create(path).ok()
}

fn packaged_log_path(macos: bool, game_root: &Path, data_dir: &Path) -> PathBuf {
    let root = if macos { data_dir } else { game_root };
    root.join("logs").join("game.log")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_level_names_parse() {
        assert_eq!(parse_level("WARNING"), LevelFilter::Warn);
        assert_eq!(parse_level("info"), LevelFilter::Info);
        assert_eq!(parse_level("DEBUG"), LevelFilter::Debug);
        assert_eq!(parse_level("nonsense"), LevelFilter::Info);
    }

    #[test]
    fn rotation_keeps_the_previous_log() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("game.log");
        fs::write(&path, "old run").unwrap();
        let mut file = open_log_file(&path).unwrap();
        writeln!(file, "new run").unwrap();
        drop(file);
        assert_eq!(
            fs::read_to_string(dir.path().join("game.prev.log")).unwrap(),
            "old run"
        );
        assert!(fs::read_to_string(&path).unwrap().starts_with("new run"));
    }

    #[test]
    fn packaged_macos_log_uses_application_support_not_the_app_bundle() {
        let app = Path::new("/Applications/FreightFate.app");
        let data = Path::new("/Users/driver/Library/Application Support/FreightFate");
        assert_eq!(
            packaged_log_path(true, app, data),
            data.join("logs").join("game.log")
        );
    }

    #[test]
    fn packaged_portable_log_stays_beside_the_game() {
        let game = Path::new("/games/FreightFate");
        let data = game.join("saves");
        assert_eq!(
            packaged_log_path(false, game, &data),
            game.join("logs").join("game.log")
        );
    }
}
