//! Runtime data files (port of `freight_fate/data/data_resources.py`).
//!
//! Every runtime loader of a loose data file under the data root (`data/`
//! in a checkout, `freight_fate/data` in a release) must go through
//! [`read_data_text`]; reading siblings of the source tree directly works in
//! a source checkout and silently (or loudly) breaks in a packaged build.
//! The release ships a baked binary container, so there are two
//! questions here: WHERE the tree is, answered once by [`data_root`], and
//! whether a `world.ffdata` sits in it, answered once by [`baked`].
//!
//! The JSON tree always wins where it exists. A source checkout has no
//! container and behaves exactly as it always did; a release ships the
//! container and no loose JSON, and [`read_text_at`] falls through to it.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use once_cell::sync::OnceCell;

use super::baked::{BakedData, BAKED_FILE_NAME};
use super::world_models::DataError;

/// Environment variable that pins the data tree for a run (tests, tooling,
/// a portable install that keeps its data elsewhere).
pub const DATA_ROOT_ENV: &str = "FREIGHT_FATE_DATA_ROOT";

static DATA_ROOT: OnceCell<PathBuf> = OnceCell::new();

/// Folder holding packaged data and sound packs for one executable.
///
/// macOS keeps immutable assets in the app bundle's Resources directory;
/// portable Windows and Linux builds keep them beside the executable.
pub fn resource_dir_for_executable(executable: &Path, macos: bool) -> PathBuf {
    let executable_dir = executable.parent().unwrap_or_else(|| Path::new("."));
    if macos
        && executable_dir
            .file_name()
            .is_some_and(|name| name == "MacOS")
        && executable_dir
            .parent()
            .is_some_and(|contents| contents.file_name().is_some_and(|name| name == "Contents"))
    {
        return executable_dir
            .parent()
            .expect("checked Contents parent")
            .join("Resources");
    }
    executable_dir.to_path_buf()
}

/// Walk up from `start` looking for the checkout's `data/` tree. A bare
/// `data/` directory is not enough; it must hold the indexed world.
fn find_source_tree(start: &Path) -> Option<PathBuf> {
    let mut cursor = Some(start);
    while let Some(dir) = cursor {
        let candidate = dir.join("data");
        if candidate.join("world_data").join("index.json").is_file() {
            return Some(candidate);
        }
        cursor = dir.parent();
    }
    None
}

fn resolve_data_root() -> PathBuf {
    if let Some(root) = std::env::var_os(DATA_ROOT_ENV) {
        let root = PathBuf::from(root);
        if !root.as_os_str().is_empty() {
            return root;
        }
    }
    let executable = crate::settings::paths::game_executable().ok();
    let exe_dir = executable
        .as_ref()
        .and_then(|exe| exe.parent().map(Path::to_path_buf));
    if let Some(executable) = &executable {
        let resources = resource_dir_for_executable(executable, cfg!(target_os = "macos"));
        let packaged = resources.join("freight_fate").join("data");
        if packaged.is_dir() {
            return packaged;
        }
    }
    if let Some(dir) = &exe_dir {
        // A packaged build: `freight_fate/data` beside the executable.
        let packaged = dir.join("freight_fate").join("data");
        if packaged.is_dir() {
            return packaged;
        }
        // A source checkout run from `target/...`.
        if let Some(found) = find_source_tree(dir) {
            return found;
        }
    }
    // The crate's own manifest directory: `crates/ff-core` -> repo root.
    if let Some(found) = find_source_tree(Path::new(env!("CARGO_MANIFEST_DIR"))) {
        return found;
    }
    // Nothing found; callers see `None` from `read_data_text` and degrade.
    exe_dir
        .map(|dir| dir.join("freight_fate").join("data"))
        .unwrap_or_else(|| PathBuf::from("freight_fate/data"))
}

/// The directory that stands in for the Python package's `data/` folder.
///
/// Resolved once per process: `FREIGHT_FATE_DATA_ROOT` if set, else
/// `<exe dir>/freight_fate/data` if it exists (a packaged build), else the
/// checkout's `data/` tree found by walking up from the
/// executable or from this crate's manifest directory.
pub fn data_root() -> &'static Path {
    DATA_ROOT.get_or_init(resolve_data_root)
}

/// Absolute path of a runtime data file under [`data_root`].
pub fn data_path(relative: &str) -> PathBuf {
    data_root().join(relative)
}

/// The text of a runtime data file, or `None` when it does not exist.
/// Callers that cannot degrade gracefully raise on `None` themselves.
pub fn read_data_text(relative: &str) -> Option<String> {
    read_text_at(&data_path(relative))
}

/// Read a UTF-8 text file, `None` when it does not exist (or cannot be read).
///
/// A path under a data root that has a baked container falls through to the
/// container's copy of the same file when the loose JSON is not on disk,
/// which is how a release with no JSON tree still answers `buffs.json`.
pub fn read_text_at(path: &Path) -> Option<String> {
    if path.exists() {
        return std::fs::read_to_string(path).ok();
    }
    baked_text_for(path)
}

#[cfg(test)]
mod packaged_resource_tests {
    use super::*;

    #[test]
    fn macos_app_resources_are_resolved_above_the_executable() {
        let executable = Path::new("/Applications/FreightFate.app/Contents/MacOS/FreightFate");
        assert_eq!(
            resource_dir_for_executable(executable, true),
            PathBuf::from("/Applications/FreightFate.app/Contents/Resources")
        );
    }

    #[test]
    fn non_macos_resources_stay_beside_the_executable() {
        let executable = Path::new("/games/FreightFate/FreightFate");
        assert_eq!(
            resource_dir_for_executable(executable, false),
            PathBuf::from("/games/FreightFate")
        );
    }
}

static BAKED: OnceCell<Option<Arc<BakedData>>> = OnceCell::new();

/// The baked container beside the shipped data, opened once, or `None` when
/// this build ships the JSON tree instead.
///
/// A container that is there but unreadable -- truncated, or written by
/// another format version -- is fatal on purpose. A release ships no JSON
/// tree to fall back to, so degrading here would turn one clear message
/// naming the file and the re-bake command into a hundred missing-data
/// symptoms. Callers that can report an error properly use [`baked_at`].
pub fn baked() -> Option<&'static Arc<BakedData>> {
    BAKED
        .get_or_init(
            || match open_container(&data_root().join(BAKED_FILE_NAME)) {
                Ok(found) => found,
                Err(err) => panic!("{err}"),
            },
        )
        .as_ref()
}

/// The container under `data_dir`, sharing the process-wide mapping when
/// `data_dir` is the default data root. `Ok(None)` means there is no
/// container there; an unreadable one is an error, never a silent absence.
pub fn baked_at(data_dir: &Path) -> Result<Option<Arc<BakedData>>, DataError> {
    if data_dir == data_root() {
        return Ok(baked().cloned());
    }
    open_container(&data_dir.join(BAKED_FILE_NAME))
}

fn open_container(path: &Path) -> Result<Option<Arc<BakedData>>, DataError> {
    if !path.is_file() {
        return Ok(None);
    }
    BakedData::open(path).map(Some)
}

fn baked_text_for(path: &Path) -> Option<String> {
    let baked = baked()?;
    let relative = path.strip_prefix(data_root()).ok()?;
    let relative = relative.to_str()?;
    baked.text(relative)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_root_resolves_to_the_source_tree_in_a_checkout() {
        let root = data_root();
        assert!(
            root.join("world_data").join("index.json").is_file(),
            "{root:?}"
        );
    }

    #[test]
    fn a_bare_data_folder_is_not_the_source_tree() {
        let tmp = tempfile::tempdir().unwrap();
        let start = tmp.path().join("crates").join("ff-core");
        std::fs::create_dir_all(&start).unwrap();
        std::fs::create_dir_all(tmp.path().join("data")).unwrap();
        assert_eq!(find_source_tree(&start), None);
        let index = tmp.path().join("data").join("world_data");
        std::fs::create_dir_all(&index).unwrap();
        std::fs::write(index.join("index.json"), "{}").unwrap();
        assert_eq!(find_source_tree(&start), Some(tmp.path().join("data")));
    }

    #[test]
    fn read_data_text_returns_none_for_a_missing_file() {
        assert!(read_data_text("definitely-not-a-real-file.json").is_none());
        assert!(read_data_text("buffs.json").is_some());
    }
}
