//! Where the game keeps its saves and settings.
//!
//! Port of `data_dir`, `_save_root`, `game_root` and their helpers from
//! `freight_fate/models/profile.py`. Settings needs them before the profile
//! model lands, and the profile port is expected to build on these (adding
//! the one-time legacy-save migration `data_dir` ran there, which is not
//! reproduced here).
//!
//! On Windows and Linux, Freight Fate is portable: profiles and settings
//! live in a `saves` directory inside the game's own main directory -- next
//! to the executable in a packaged build, the project root when running from
//! source.
//!
//! macOS apps live in `/Applications` and must not write beside themselves
//! (that folder is admin-owned and often read-only), so on macOS saves go in
//! the standard per-user `~/Library/Application Support/FreightFate` folder.
//! iOS is the same, with the app's sandbox as home: its bundle is signed and
//! read-only, and `Library/Application Support` is backed up with the app.
//!
//! The same reasoning covers Windows and Linux when the game itself sits in
//! a read-only location (for example Windows `Program Files`): if the
//! `saves` folder beside the game cannot be written, saves fall back to that
//! per-user data directory instead of failing on the first write and
//! crashing mid-session.
//!
//! Override the location with the `FREIGHT_FATE_DATA_DIR` environment
//! variable (which the tests use).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

/// Environment variable that pins the save directory for a run.
pub const DATA_DIR_ENV: &str = "FREIGHT_FATE_DATA_DIR";

thread_local! {
    /// This thread's save directory, when something has pinned one.
    ///
    /// The environment variable above is what a PLAYER's run uses, and a
    /// process has exactly one environment. That was fine while the game was
    /// the only caller and fatal for the test suite: every test that wanted
    /// its own saves had to set the same process-global variable, so every
    /// test that wanted its own saves had to take a lock and wait its turn.
    /// The whole freight-fate suite ran at one-core speed on a 28-core
    /// machine because of it -- 123.6 seconds on one thread against 117.4 on
    /// eight, which is no parallelism at all.
    ///
    /// A thread-local override is the injectable per-test root that removes
    /// the reason for the lock. Tests pin their own directory here and never
    /// touch the environment, so hundreds of them can run at once without
    /// being able to see each other's saves. The game sets it never and is
    /// bit-for-bit unaffected: with nothing pinned, [`data_dir`] falls
    /// through to exactly the environment variable and [`save_root`] it
    /// always used.
    ///
    /// Threading a root through every call site instead would be the purer
    /// fix, but `data_dir()` is read from the profile model, the settings
    /// model, the cloud service and the online store, none of which have a
    /// test-supplied value to thread. This puts the injection at the one
    /// place that reads the process's answer.
    static THREAD_DATA_DIR: std::cell::RefCell<Option<PathBuf>> =
        const { std::cell::RefCell::new(None) };
}

/// Pin this thread's save directory (`None` clears it), returning whatever
/// was pinned before so a scope guard can put it back.
pub fn set_thread_data_dir(dir: Option<PathBuf>) -> Option<PathBuf> {
    THREAD_DATA_DIR.with(|slot| std::mem::replace(&mut *slot.borrow_mut(), dir))
}

/// This thread's pinned save directory, if any.
pub fn thread_data_dir() -> Option<PathBuf> {
    THREAD_DATA_DIR.with(|slot| slot.borrow().clone())
}

// -- who may reach the player's real save folder ---------------------------------
//
// [`THREAD_DATA_DIR`] above is thread-local, and it has to be: it is what
// lets hundreds of tests each have their own saves at once. But a
// thread-local is invisible to a thread the test did not pin, and the value
// a thread with nothing pinned falls through to is [`save_root`] -- the
// owner's real save folder, with their careers in it.
//
// Nothing spawns such a thread today (every worker captures the directory
// before spawning), and "nothing does today" is not a guarantee. It is the
// one seam here whose failure mode is writing over somebody's career, so the
// fallback is a capability rather than a default:
//
// * [`allow_real_save_dir`] is called once, by the game's `main()`, and only
//   by it. A test binary has no such `main()`, so no test process can hold
//   it -- nothing to remember and nothing to forget.
// * Process-wide (`AtomicBool`) on purpose, so a spawned worker refuses
//   exactly as the game loop would allow. That is the case a per-thread flag
//   could not reach and the reason this seam exists at all.
// * Without it, [`data_dir`] records the path in [`refused_save_dirs`] and
//   panics naming it. A quiet fallback to a temporary folder would read as a
//   fresh career -- a state the game handles perfectly gracefully and nobody
//   would ever look at.
//
// A pinned thread directory and `FREIGHT_FATE_DATA_DIR` both still answer
// first and are untouched: they are somebody saying, explicitly, which
// directory to use.

/// Set once by the game's `main()`. Process-wide, so a spawned worker sees
/// it exactly as the game loop does.
static REAL_SAVE_DIR_ALLOWED: AtomicBool = AtomicBool::new(false);

/// Every real save directory that was asked for and refused, in order.
static REFUSED_SAVE_DIRS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// "This process is the real game": from here on [`data_dir`] may fall
/// through to the player's own save folder.
///
/// Called from `main()` and nowhere else. Nothing undoes it.
pub fn allow_real_save_dir() {
    REAL_SAVE_DIR_ALLOWED.store(true, Ordering::SeqCst);
}

/// Whether the player's real save folder may be reached in this process.
pub fn real_save_dir_allowed() -> bool {
    REAL_SAVE_DIR_ALLOWED.load(Ordering::SeqCst)
}

/// Every real save directory that was refused, oldest first.
pub fn refused_save_dirs() -> Vec<String> {
    REFUSED_SAVE_DIRS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .clone()
}

/// Record `path` and panic: something that is not the game asked for the
/// player's own save folder.
///
/// Shared with `models::profile::paths`, which has its own `data_dir` over
/// the same roots, so both doors carry the same lock.
///
/// # Panics
///
/// Always.
#[cold]
pub fn refuse_real_save_dir(path: &Path) -> ! {
    REFUSED_SAVE_DIRS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(path.display().to_string());
    panic!(
        "refusing to use the real save directory {}: this process never \
         called settings::paths::allow_real_save_dir(), so it is not the \
         game. If this is a test, pin a directory for the thread that asked \
         -- set_thread_data_dir(Some(dir)) -- and remember a thread you \
         spawn does NOT inherit the pin.",
        path.display()
    );
}

static UNWRITABLE_WARNED: AtomicBool = AtomicBool::new(false);
static WRITABLE_CACHE: Lazy<Mutex<HashMap<PathBuf, bool>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// Guards the process environment for tests that genuinely write one.
///
/// A read guard means "I am relying on the environment holding still"; a
/// write guard means "I am about to change it". Readers run together, so the
/// common case -- a test that just wants its own save directory, which now
/// pins [`THREAD_DATA_DIR`] instead -- costs nothing, while the handful that
/// really do set `FREIGHT_FATE_SKIP_SAVE_SIGNING` or `FREIGHT_FATE_DATA_DIR`
/// still get the whole process to themselves.
///
/// This was a plain `Mutex` held by every test that wanted an isolated data
/// directory, which is most of them: `ff-core`'s own suite ran 33.1 seconds
/// on one thread and still 18.8 on sixteen. `sim::weather` already used the
/// read/write shape for the same reason.
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::RwLock<()> = std::sync::RwLock::new(());

fn home_dir() -> PathBuf {
    #[allow(deprecated)]
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// The standard per-user save location on macOS (and in the iOS sandbox).
fn macos_data_dir() -> PathBuf {
    home_dir()
        .join("Library")
        .join("Application Support")
        .join("FreightFate")
}

/// Where saves lived before the portable layout (per-user folders).
///
/// On macOS this is also the *current* save location, since app bundles
/// cannot store saves beside themselves.
pub fn legacy_data_dir() -> PathBuf {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        return macos_data_dir();
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(home_dir)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".local").join("share"))
    };
    base.join("FreightFate")
}

/// Whether `path` exists (or can be created) and accepts a write.
///
/// Detects installs in protected locations, such as Windows `Program
/// Files`, where the portable `saves` folder beside the game would raise on
/// the first save and crash the game mid-session.
///
/// Cached per path: `save_root()` re-derives this on every save-directory
/// lookup (several times per menu enter), and the answer cannot change
/// within one run -- `game_root()` is fixed once the process starts, and
/// nothing else in the portable-save code path relocates it mid-session.
/// Without the cache this was a real mkdir+write+unlink against disk every
/// single call.
fn is_writable_dir(path: &Path) -> bool {
    let mut cache = WRITABLE_CACHE.lock();
    if let Some(known) = cache.get(path) {
        return *known;
    }
    let writable = probe_writable(path);
    cache.insert(path.to_path_buf(), writable);
    writable
}

fn probe_writable(path: &Path) -> bool {
    if std::fs::create_dir_all(path).is_err() {
        return false;
    }
    let probe = path.join(".freightfate-write-test");
    if std::fs::write(&probe, b"").is_err() {
        return false;
    }
    std::fs::remove_file(&probe).is_ok()
}

/// The active save directory for this platform.
///
/// Windows and Linux keep the portable `saves` folder next to the game.
/// macOS uses the per-user Application Support folder so the app never has
/// to write into `/Applications`. When the game sits in a read-only location
/// such as `Program Files`, Windows and Linux fall back to that same
/// per-user folder rather than crashing on the first save.
pub fn save_root() -> PathBuf {
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        return macos_data_dir();
    }
    let root = game_root();
    if is_writable_dir(&root) {
        return root.join("saves");
    }
    let fallback = legacy_data_dir();
    if !UNWRITABLE_WARNED.swap(true, Ordering::SeqCst) {
        log::warn!(
            "Game directory {} is not writable; saving to the per-user folder {} instead. \
             Move Freight Fate out of a protected location such as Program Files to keep \
             saves beside the game.",
            root.display(),
            fallback.display()
        );
    }
    fallback
}

/// The directory of the running executable, resolved.
fn executable_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // Absolute, but not canonical: `canonicalize` on Windows yields a
    // verbatim `\?\` path, which then leaks into every save path and log
    // line built on it.
    let exe = std::path::absolute(&exe).unwrap_or(exe);
    exe.parent().map(Path::to_path_buf)
}

/// A packaged build ships `freight_fate/data` beside the executable (the
/// release layout); that is what "frozen" meant for the Python build.
fn packaged_exe_dir() -> Option<PathBuf> {
    let dir = executable_dir()?;
    let executable = dir.join(if cfg!(windows) {
        "FreightFate.exe"
    } else {
        "FreightFate"
    });
    let resources = crate::data::data_resources::resource_dir_for_executable(
        &executable,
        cfg!(target_os = "macos"),
    );
    if resources.join("freight_fate").join("data").is_dir() {
        Some(dir)
    } else {
        None
    }
}

fn macos_app_bundle(exe_dir: &Path) -> Option<PathBuf> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    if exe_dir.file_name()? != "MacOS" || exe_dir.parent()?.file_name()? != "Contents" {
        return None;
    }
    let bundle = exe_dir.parent()?.parent()?;
    if bundle.extension()? == "app" {
        Some(bundle.to_path_buf())
    } else {
        None
    }
}

/// Walk up from `start` looking for the project root: the workspace
/// `Cargo.toml` with `crates/ff-core` beside it. A crate's own `Cargo.toml`
/// does not qualify, so the walk from `crates/ff-core` goes past it.
fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut cursor = Some(start);
    while let Some(dir) = cursor {
        if dir.join("Cargo.toml").is_file() && dir.join("crates").join("ff-core").is_dir() {
            return Some(dir.to_path_buf());
        }
        cursor = dir.parent();
    }
    None
}

/// The game's main directory: the executable's directory in a packaged
/// build (the enclosing folder of the app bundle on macOS), the project root
/// when running from source.
pub fn game_root() -> PathBuf {
    if let Some(exe_dir) = packaged_exe_dir() {
        if let Some(bundle) = macos_app_bundle(&exe_dir) {
            return bundle.parent().map(Path::to_path_buf).unwrap_or(exe_dir);
        }
        return exe_dir;
    }
    // Running from source: this crate sits at `crates/ff-core` in the repo.
    if let Some(root) = find_project_root(Path::new(env!("CARGO_MANIFEST_DIR"))) {
        return root;
    }
    if let Some(root) = executable_dir().and_then(|dir| find_project_root(&dir)) {
        return root;
    }
    executable_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Where settings and profiles live: this thread's pinned directory,
/// `FREIGHT_FATE_DATA_DIR` when set (and not empty), else [`save_root`].
///
/// # Panics
///
/// When nothing is pinned, the environment says nothing, and
/// [`allow_real_save_dir`] has not been called -- which outside the real
/// game means something reached for the player's own careers. The path is
/// recorded in [`refused_save_dirs`] first.
pub fn data_dir() -> PathBuf {
    if let Some(pinned) = thread_data_dir() {
        return pinned;
    }
    if let Some(override_dir) = std::env::var_os(DATA_DIR_ENV) {
        if !override_dir.is_empty() {
            return PathBuf::from(override_dir);
        }
    }
    let root = save_root();
    if !real_save_dir_allowed() {
        refuse_real_save_dir(&root);
    }
    root
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These cases are ABOUT the environment variable, so they really do
    /// write it and take the exclusive guard.
    fn with_env<T>(value: Option<&Path>, body: impl FnOnce() -> T) -> T {
        let _guard = ENV_LOCK.write().unwrap_or_else(|e| e.into_inner());
        let previous = std::env::var_os(DATA_DIR_ENV);
        match value {
            Some(path) => std::env::set_var(DATA_DIR_ENV, path),
            None => std::env::remove_var(DATA_DIR_ENV),
        }
        let result = body();
        match previous {
            Some(old) => std::env::set_var(DATA_DIR_ENV, old),
            None => std::env::remove_var(DATA_DIR_ENV),
        }
        result
    }

    #[test]
    fn the_override_wins_and_an_empty_override_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        with_env(Some(tmp.path()), || {
            assert_eq!(data_dir(), tmp.path());
        });
        with_env(Some(Path::new("")), || {
            // An empty override is not an override, so `data_dir` falls
            // through to `save_root` -- which in a process that is not the
            // game is refused BY NAME rather than handed over. The refusal
            // naming exactly `save_root()` is the same claim the old
            // `assert_eq!(data_dir(), save_root())` made, and it also proves
            // the fallthrough never reached anybody's careers.
            let root = save_root().display().to_string();
            let outcome = std::panic::catch_unwind(data_dir);
            assert!(outcome.is_err(), "the real save folder was handed over");
            assert!(refused_save_dirs().contains(&root), "{root}");
        });
    }

    #[test]
    fn the_source_checkout_keeps_saves_beside_the_project() {
        let root = game_root();
        let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("ff-core sits two levels under the repo root");
        assert_eq!(root, repo);
        assert!(root.join("Cargo.toml").is_file(), "{}", root.display());
        if !cfg!(target_os = "macos") {
            assert_eq!(save_root(), root.join("saves"));
        }
    }

    #[test]
    fn the_writable_probe_is_cached_and_honest() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("fresh");
        assert!(is_writable_dir(&dir));
        assert!(is_writable_dir(&dir));
        assert!(!dir.join(".freightfate-write-test").exists());
        // A file where a directory should be cannot be written into.
        let blocked = tmp.path().join("blocked");
        std::fs::write(&blocked, b"x").unwrap();
        assert!(!is_writable_dir(&blocked));
    }
}
