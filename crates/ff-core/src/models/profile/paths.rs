//! Where saves live, and the one-time migration of older layouts into the
//! active location (the `data_dir` / `_migrate_legacy` half of `profile.py`).
//!
//! `settings::paths` ports `game_root`, `save_root` and the override; this
//! module builds on it and adds the legacy-save migration `data_dir` ran in
//! Python. Every root is injectable through [`SaveRoots`] so the portable-save
//! tests can pin temp locations the way the Python tests monkeypatched the
//! module globals.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use once_cell::sync::Lazy;
use parking_lot::Mutex;

use crate::settings::paths::legacy_data_dir;
pub use crate::settings::{game_root, DATA_DIR_ENV};

static LEGACY_CHECKED: AtomicBool = AtomicBool::new(false);
static UNWRITABLE_WARNED: AtomicBool = AtomicBool::new(false);
static WRITABLE_CACHE: Lazy<Mutex<HashMap<PathBuf, bool>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

fn home_dir() -> PathBuf {
    #[allow(deprecated)]
    std::env::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// The standard per-user save location on macOS.
pub fn macos_data_dir() -> PathBuf {
    home_dir()
        .join("Library")
        .join("Application Support")
        .join("FreightFate")
}

/// The roots `data_dir` derives the save location from. [`SaveRoots::current`]
/// reads the real process; tests build one by hand.
#[derive(Debug, Clone)]
pub struct SaveRoots {
    /// `FREIGHT_FATE_DATA_DIR`, when set and non-empty.
    pub override_dir: Option<PathBuf>,
    /// `game_root()`.
    pub game_root: PathBuf,
    /// `_legacy_data_dir()`: the per-user folder of the pre-portable layout.
    pub legacy_data_dir: PathBuf,
    /// `_macos_data_dir()`.
    pub macos_data_dir: PathBuf,
    /// `sys.platform == "darwin"`.
    pub macos: bool,
    /// The executable's directory in a packaged build (`is_frozen()`).
    pub frozen_exe_dir: Option<PathBuf>,
    /// Force the writable-probe answer (tests); `None` probes the disk.
    pub writable: Option<bool>,
}

impl SaveRoots {
    pub fn current() -> Self {
        // The thread's pinned root first, then the process environment: the
        // test suite injects a per-test root rather than sharing one
        // process-global variable behind a lock (see `THREAD_DATA_DIR`).
        let override_dir = crate::settings::paths::thread_data_dir().or_else(|| {
            std::env::var_os(DATA_DIR_ENV)
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        });
        SaveRoots {
            override_dir,
            game_root: game_root(),
            legacy_data_dir: legacy_data_dir(),
            macos_data_dir: macos_data_dir(),
            macos: cfg!(any(target_os = "macos", target_os = "ios")),
            frozen_exe_dir: if super::is_frozen() {
                std::env::current_exe()
                    .ok()
                    .and_then(|exe| exe.parent().map(Path::to_path_buf))
            } else {
                None
            },
            writable: None,
        }
    }

    /// A portable (Windows/Linux) layout rooted at `game_root`, with the
    /// per-user legacy folder at `legacy`; the shape the portable-save tests
    /// pin.
    pub fn portable(game_root: &Path, legacy: &Path) -> Self {
        SaveRoots {
            override_dir: None,
            game_root: game_root.to_path_buf(),
            legacy_data_dir: legacy.to_path_buf(),
            macos_data_dir: legacy.to_path_buf(),
            macos: false,
            frozen_exe_dir: None,
            writable: None,
        }
    }
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

/// Whether `path` exists (or can be created) and accepts a write.
///
/// Detects installs in protected locations, such as Windows `Program Files`,
/// where the portable `saves` folder beside the game would raise on the first
/// save and crash the game mid-session. Cached per path: the answer cannot
/// change within one run, and without the cache this was a real
/// mkdir+write+unlink against disk every single call.
pub fn is_writable_dir(path: &Path) -> bool {
    let mut cache = WRITABLE_CACHE.lock();
    if let Some(known) = cache.get(path) {
        return *known;
    }
    let writable = probe_writable(path);
    cache.insert(path.to_path_buf(), writable);
    writable
}

/// How many times the disk probe has run for `path` (tests).
#[cfg(test)]
pub(crate) fn writable_probe_cached(path: &Path) -> bool {
    WRITABLE_CACHE.lock().contains_key(path)
}

/// The active save directory for this platform (`_save_root`).
///
/// Windows and Linux keep the portable `saves` folder next to the game.
/// macOS uses the per-user Application Support folder so the app never has
/// to write into `/Applications`. When the game sits in a read-only location
/// such as `Program Files`, Windows and Linux fall back to that same per-user
/// folder rather than crashing on the first save.
pub fn save_root_in(roots: &SaveRoots) -> PathBuf {
    if roots.macos {
        return roots.macos_data_dir.clone();
    }
    let writable = roots
        .writable
        .unwrap_or_else(|| is_writable_dir(&roots.game_root));
    if writable {
        return roots.game_root.join("saves");
    }
    let fallback = roots.legacy_data_dir.clone();
    if !UNWRITABLE_WARNED.swap(true, Ordering::SeqCst) {
        log::warn!(
            "Game directory {} is not writable; saving to the per-user folder {} instead. \
             Move Freight Fate out of a protected location such as Program Files to keep \
             saves beside the game.",
            roots.game_root.display(),
            fallback.display()
        );
    }
    fallback
}

pub fn save_root() -> PathBuf {
    save_root_in(&SaveRoots::current())
}

/// One-time migration of old save folders into the portable one.
fn migrate_legacy(roots: &SaveRoots, target: &Path) {
    if migrate_nearby_portable_saves(roots, target) {
        return;
    }
    if target.exists() {
        return;
    }
    let legacy = &roots.legacy_data_dir;
    if legacy != target && legacy.is_dir() {
        // A first run silently inheriting an old career looks like a haunted
        // save; the log line makes "where did this come from" answerable.
        log::info!(
            "Save migration: copying legacy saves from {} into {}",
            legacy.display(),
            target.display()
        );
        copy_save_tree(legacy, target);
    }
}

/// Move earlier portable layouts into the current save root.
///
/// These folders are user-owned portable save folders inside the game's own
/// directory, so leaving them behind creates two plausible save locations.
/// Per-user legacy folders are still copied, not moved.
fn migrate_nearby_portable_saves(roots: &SaveRoots, target: &Path) -> bool {
    for source in portable_migration_candidates(roots, target) {
        if !source.is_dir() {
            continue;
        }
        if !target.exists() {
            log::info!(
                "Save migration: moving portable saves from {} to {}",
                source.display(),
                target.display()
            );
            move_save_tree(&source, target);
            return target.exists();
        }
        log::info!(
            "Save migration: merging portable saves from {} into {}",
            source.display(),
            target.display()
        );
        merge_save_tree(&source, target);
        return true;
    }
    false
}

/// A breadcrumb where a moved save tree used to be.
///
/// A plain text file (never a directory, so it can't re-trigger candidate
/// scans) telling a player -- or a debugging session -- where the saves went
/// instead of leaving them to vanish without a trace.
fn leave_migration_marker(source: &Path, target: &Path) {
    let name = source
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let marker = source.with_file_name(format!("{name}-moved.txt"));
    let _ = std::fs::write(
        marker,
        format!(
            "Freight Fate moved the saves that were in this folder to:\n{}\nThis breadcrumb is safe to delete.\n",
            target.display()
        ),
    );
}

fn copy_dir_recursive(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let dest = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir_recursive(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

/// Copy a save tree without blocking startup if the filesystem objects.
fn copy_save_tree(source: &Path, target: &Path) {
    // never block startup on a migration; old saves stay where they are
    let _ = copy_dir_recursive(source, target);
}

/// Move a nearby portable save folder into the active location.
fn move_save_tree(source: &Path, target: &Path) {
    let moved = (|| -> std::io::Result<()> {
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        if std::fs::rename(source, target).is_err() {
            copy_dir_recursive(source, target)?;
            std::fs::remove_dir_all(source)?;
        }
        Ok(())
    })();
    if moved.is_ok() {
        leave_migration_marker(source, target);
    }
}

fn walk_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            out.push(path.clone());
            walk_files(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Merge a duplicate nearby save folder without overwriting current saves.
fn merge_save_tree(source: &Path, target: &Path) {
    let mut paths = Vec::new();
    walk_files(source, &mut paths);
    for path in &paths {
        let Ok(rel) = path.strip_prefix(source) else {
            continue;
        };
        let dest = target.join(rel);
        if path.is_dir() {
            let _ = std::fs::create_dir_all(&dest);
        } else if !dest.exists() {
            if let Some(parent) = dest.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::rename(path, &dest).is_err() && std::fs::copy(path, &dest).is_ok() {
                let _ = std::fs::remove_file(path);
            }
        }
    }
    // Remove the duplicate tree only when every file was moved or already
    // existed in the active tree, and leave a breadcrumb in its place.
    let mut remaining = Vec::new();
    walk_files(source, &mut remaining);
    if !remaining.iter().any(|p| p.is_file()) {
        let _ = std::fs::remove_dir_all(source);
        leave_migration_marker(source, target);
    }
}

/// Nearby save roots to fold into the active location.
///
/// Covers a previous archive layout nested inside the game's own folder and,
/// on macOS, the misplaced `saves` folder beside the app bundle (or inside it)
/// that earlier builds created in `/Applications`.
///
/// Candidates never leave the game's own main directory: a player may keep
/// several extracted copies of the game side by side (say, a 1.8 install next
/// to a 1.9 test build), and one copy scanning its parent folder would steal
/// -- and then share -- the saves of another. Each copy owns exactly the
/// saves inside its own folder.
fn portable_migration_candidates(roots: &SaveRoots, target: &Path) -> Vec<PathBuf> {
    let root = &roots.game_root;
    let mut candidates = vec![root.join("saves"), root.join("FreightFate").join("saves")];
    if let Some(exe_dir) = &roots.frozen_exe_dir {
        candidates.push(exe_dir.join("saves"));
    }
    let mut result: Vec<PathBuf> = Vec::new();
    for path in candidates {
        if path == target || result.contains(&path) {
            continue;
        }
        result.push(path);
    }
    result
}

/// `data_dir()` against explicit roots and an explicit "legacy layouts
/// already checked" flag.
pub fn data_dir_in(roots: &SaveRoots, legacy_checked: &AtomicBool) -> PathBuf {
    if let Some(override_dir) = &roots.override_dir {
        return override_dir.clone();
    }
    let target = save_root_in(roots);
    if !legacy_checked.swap(true, Ordering::SeqCst) {
        migrate_legacy(roots, &target);
    }
    target
}

/// Where settings and profiles live: this thread's pinned directory or
/// `FREIGHT_FATE_DATA_DIR` when either says so, else the portable save root
/// -- migrating older layouts into it on the first call of the process.
///
/// The second door to the player's own save folder, so it carries the same
/// lock as `settings::paths::data_dir`; see the capability note there for
/// why the fallback is refused rather than defaulted. The check comes before
/// [`data_dir_in`] because that call is what migrates legacy layouts, and a
/// migration is a write.
///
/// [`data_dir_in`] itself is deliberately NOT guarded: its roots are handed
/// in, so the portable-save tests that build a synthetic layout under a
/// temporary directory are asking about roots of their own, not the
/// player's.
///
/// # Panics
///
/// When nothing is pinned, the environment says nothing, and
/// `settings::paths::allow_real_save_dir` has not been called. The path is
/// recorded in `settings::paths::refused_save_dirs` first.
pub fn data_dir() -> PathBuf {
    let roots = SaveRoots::current();
    if roots.override_dir.is_none() && !crate::settings::paths::real_save_dir_allowed() {
        crate::settings::paths::refuse_real_save_dir(&save_root_in(&roots));
    }
    data_dir_in(&roots, &LEGACY_CHECKED)
}

/// [`data_dir`] for best-effort bookkeeping: `None` where `data_dir` would
/// refuse, instead of panicking.
pub fn data_dir_if_allowed() -> Option<PathBuf> {
    let roots = SaveRoots::current();
    if roots.override_dir.is_none() && !crate::settings::paths::real_save_dir_allowed() {
        return None;
    }
    Some(data_dir_in(&roots, &LEGACY_CHECKED))
}

/// Bookkeeping shared by this release line's careers, under the shared root.
pub fn career_data_dir() -> PathBuf {
    let dir = data_dir().join(super::CREATED_LINE);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// The running release line's career folder, created.
pub fn profiles_dir() -> PathBuf {
    let dir = career_data_dir().join("profiles");
    let _ = std::fs::create_dir_all(&dir);
    dir
}
