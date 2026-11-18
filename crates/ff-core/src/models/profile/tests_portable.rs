//! Ported from `tests/test_portable_saves.py`: game-directory storage and
//! legacy migration. The Python tests monkeypatched the module's roots; here
//! the same roots ride in a [`SaveRoots`] and the "legacy layouts already
//! checked" flag is a fresh atomic per test.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use super::paths::{data_dir_in, game_root, is_writable_dir, save_root_in, SaveRoots};
use super::tests::with_data_dir;
use super::*;

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

/// `_reset`: point both roots at controlled temp locations (the portable
/// Windows/Linux layout, saves beside the game).
fn reset(
    tmp: &Path,
    game_dir: Option<&Path>,
    legacy_dir: Option<&Path>,
) -> (SaveRoots, PathBuf, PathBuf) {
    let game = game_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| tmp.join("game"));
    std::fs::create_dir_all(&game).unwrap();
    let legacy = legacy_dir
        .map(Path::to_path_buf)
        .unwrap_or_else(|| tmp.join("appdata"))
        .join("FreightFate");
    (SaveRoots::portable(&game, &legacy), game, legacy)
}

fn data_dir_of(roots: &SaveRoots) -> PathBuf {
    data_dir_in(roots, &AtomicBool::new(false))
}

#[test]
fn test_env_override_wins() {
    with_data_dir(|dir| {
        assert_eq!(data_dir(), dir);
    });
    let tmp = tempfile::tempdir().unwrap();
    let mut roots = SaveRoots::portable(tmp.path(), tmp.path());
    roots.override_dir = Some(tmp.path().join("custom"));
    assert_eq!(data_dir_of(&roots), tmp.path().join("custom"));
}

#[test]
fn test_data_dir_is_saves_inside_game_root() {
    let tmp = tempfile::tempdir().unwrap();
    let (roots, game, _) = reset(tmp.path(), None, None);
    assert_eq!(data_dir_of(&roots), game.join("saves"));
}

#[test]
fn test_game_root_from_source_is_project_root() {
    let root = game_root();
    assert!(root.join("crates").join("ff-core").is_dir());
    assert!(root.join("data").join("world_data").is_dir());
}

/// The mkdir+write+unlink probe must run once per path per process, not
/// once per caller: `save_root()` and `data_dir()` re-derive this on every
/// save lookup, several times per menu enter.
#[test]
fn test_is_writable_dir_probes_disk_once_per_path() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("writable");
    assert!(is_writable_dir(&target));
    assert!(super::paths::writable_probe_cached(&target));
    // Removing the probe's side effects does not trigger a re-probe.
    std::fs::remove_dir_all(&target).unwrap();
    assert!(is_writable_dir(&target));
    assert!(!target.exists());
}

#[test]
fn test_legacy_saves_migrate_once() {
    let tmp = tempfile::tempdir().unwrap();
    let (roots, _game, legacy) = reset(tmp.path(), None, None);
    let old_profile = legacy.join("profiles").join("Driver.json");
    write(&old_profile, "{}");
    write(&legacy.join("settings.json"), "{}");

    let target = data_dir_of(&roots);
    assert!(target.join("profiles").join("Driver.json").is_file());
    assert!(target.join("settings.json").is_file());
    assert!(old_profile.is_file()); // originals are left in place
}

#[test]
fn test_migration_never_overwrites_portable_saves() {
    let tmp = tempfile::tempdir().unwrap();
    let (roots, game, legacy) = reset(tmp.path(), None, None);
    write(&legacy.join("profiles").join("Old.json"), "{}");
    let existing = game.join("saves").join("profiles");
    write(&existing.join("Current.json"), "{}");

    let target = data_dir_of(&roots);
    assert!(target.join("profiles").join("Current.json").is_file());
    assert!(!target.join("profiles").join("Old.json").exists());
}

/// A saves folder in the parent directory belongs to someone else -- another
/// install, another version -- and must never be pulled in.
#[test]
fn test_parent_folder_saves_are_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let game = tmp.path().join("freightfate").join("FreightFate");
    std::fs::create_dir_all(&game).unwrap();
    let other_profile = tmp
        .path()
        .join("freightfate")
        .join("saves")
        .join("profiles")
        .join("Driver.json");
    write(&other_profile, "{}");
    let (roots, _, _) = reset(tmp.path(), Some(&game), None);

    let target = data_dir_of(&roots);
    assert_eq!(target, game.join("saves"));
    assert!(!target.join("profiles").join("Driver.json").exists());
    assert!(other_profile.is_file()); // the other install keeps its saves
}

/// The reported bug: a 1.9 test build extracted next to a 1.8 install must
/// not steal the 1.8 saves through their shared parent folder.
#[test]
fn test_sibling_install_saves_are_left_alone() {
    let tmp = tempfile::tempdir().unwrap();
    let old_install_profile = tmp
        .path()
        .join("Games")
        .join("FreightFate")
        .join("saves")
        .join("profiles")
        .join("Driver.json");
    write(&old_install_profile, "{}");
    let new_install = tmp.path().join("Games").join("FreightFate-1.9");
    std::fs::create_dir_all(&new_install).unwrap();
    let (roots, _, _) = reset(tmp.path(), Some(&new_install), None);

    let target = data_dir_of(&roots);
    assert_eq!(target, new_install.join("saves"));
    assert!(!target.join("profiles").join("Driver.json").exists());
    assert!(old_install_profile.is_file()); // 1.8 keeps its careers
}

#[test]
fn test_parent_install_moves_nested_portable_saves() {
    let tmp = tempfile::tempdir().unwrap();
    let game = tmp.path().join("freightfate");
    std::fs::create_dir_all(&game).unwrap();
    let old_profile = game
        .join("FreightFate")
        .join("saves")
        .join("profiles")
        .join("Driver.json");
    write(&old_profile, "{}");
    let (roots, _, _) = reset(tmp.path(), Some(&game), None);

    let target = data_dir_of(&roots);
    assert_eq!(target, game.join("saves"));
    assert!(target.join("profiles").join("Driver.json").is_file());
    assert!(!game.join("FreightFate").join("saves").exists());
}

#[test]
fn test_existing_active_saves_merge_nested_duplicate() {
    let tmp = tempfile::tempdir().unwrap();
    let game = tmp.path().join("freightfate");
    std::fs::create_dir_all(&game).unwrap();
    write(
        &game.join("saves").join("profiles").join("Current.json"),
        "{}",
    );
    write(
        &game
            .join("FreightFate")
            .join("saves")
            .join("profiles")
            .join("Old.json"),
        "{}",
    );
    let (roots, _, _) = reset(tmp.path(), Some(&game), None);

    let target = data_dir_of(&roots);
    assert!(target.join("profiles").join("Current.json").is_file());
    assert!(target.join("profiles").join("Old.json").is_file());
    assert!(!game.join("FreightFate").join("saves").exists());
}

#[test]
fn test_moved_save_tree_leaves_breadcrumb_and_log() {
    let tmp = tempfile::tempdir().unwrap();
    let game = tmp.path().join("freightfate");
    std::fs::create_dir_all(&game).unwrap();
    write(
        &game
            .join("FreightFate")
            .join("saves")
            .join("profiles")
            .join("Driver.json"),
        "{}",
    );
    let (roots, _, _) = reset(tmp.path(), Some(&game), None);

    let target = data_dir_of(&roots);

    // Where saves vanish from, a breadcrumb file says where they went. (The
    // "Save migration" log line is not captured here: no log sink in tests.)
    let marker = game.join("FreightFate").join("saves-moved.txt");
    assert!(marker.is_file());
    assert!(std::fs::read_to_string(&marker)
        .unwrap()
        .contains(&target.display().to_string()));
}

#[test]
fn test_merged_save_tree_leaves_breadcrumb() {
    let tmp = tempfile::tempdir().unwrap();
    let game = tmp.path().join("freightfate");
    std::fs::create_dir_all(&game).unwrap();
    write(
        &game.join("saves").join("profiles").join("Current.json"),
        "{}",
    );
    write(
        &game
            .join("FreightFate")
            .join("saves")
            .join("profiles")
            .join("Old.json"),
        "{}",
    );
    let (roots, _, _) = reset(tmp.path(), Some(&game), None);

    data_dir_of(&roots);

    let marker = game.join("FreightFate").join("saves-moved.txt");
    assert!(marker.is_file());
}

#[test]
fn test_legacy_copy_migration_is_logged() {
    // The copy itself is the observable here (no log capture in the Rust
    // tests); the log line rides along with it.
    let tmp = tempfile::tempdir().unwrap();
    let (roots, _, legacy) = reset(tmp.path(), None, None);
    write(&legacy.join("profiles").join("Driver.json"), "{}");
    let target = data_dir_of(&roots);
    assert!(target.join("profiles").join("Driver.json").is_file());
}

/// A read-only install (for example Program Files) must not crash on save:
/// the save root falls back to the per-user data directory instead.
#[test]
fn test_unwritable_game_dir_falls_back_to_user_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut roots, _, legacy) = reset(tmp.path(), None, None);
    roots.writable = Some(false);
    assert_eq!(data_dir_of(&roots), legacy);
}

/// Saving works end to end when the game's own folder cannot be written.
#[test]
fn test_save_survives_unwritable_game_dir() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut roots, _, legacy) = reset(tmp.path(), None, None);
    roots.writable = Some(false);
    // The real `save()` reads the process roots, so point the override at
    // the fallback this layout would pick and save through it.
    let fallback = data_dir_of(&roots);
    assert_eq!(fallback, legacy);
    with_data_dir(|_| {
        // Re-point THIS THREAD's save directory at the fallback. The pin is
        // what `data_dir()` reads, and `with_data_dir` has just set it to a
        // temp directory, so pinning again is how the override lands.
        let previous = crate::settings::paths::set_thread_data_dir(Some(fallback.clone()));
        let saved = Profile::named("Ryan").save().unwrap();
        crate::settings::paths::set_thread_data_dir(previous);
        assert_eq!(
            saved,
            legacy
                .join(CREATED_LINE)
                .join("profiles")
                .join("Ryan.ffsave")
        );
        assert!(saved.is_file());
    });
}

/// When the game folder is writable, the portable layout is unchanged.
#[test]
fn test_writable_game_dir_still_saves_beside_the_game() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut roots, game, _) = reset(tmp.path(), None, None);
    roots.writable = Some(true);
    assert_eq!(data_dir_of(&roots), game.join("saves"));
}

/// macOS saves land in Application Support, never beside the app bundle.
#[test]
fn test_macos_uses_application_support() {
    let tmp = tempfile::tempdir().unwrap();
    let app_support = tmp.path().join("Application Support").join("FreightFate");
    // The app bundle sits in an admin-owned /Applications-style folder.
    let applications = tmp.path().join("Applications");
    std::fs::create_dir_all(&applications).unwrap();
    let roots = SaveRoots {
        override_dir: None,
        game_root: applications,
        legacy_data_dir: app_support.clone(),
        macos_data_dir: app_support.clone(),
        macos: true,
        frozen_exe_dir: None,
        writable: None,
    };
    assert_eq!(save_root_in(&roots), app_support);
    assert_eq!(data_dir_of(&roots), app_support);
}

/// Saves an earlier build dropped beside/inside the bundle migrate into
/// Application Support rather than staying in /Applications.
#[test]
fn test_macos_app_moves_bundle_internal_saves() {
    let tmp = tempfile::tempdir().unwrap();
    let exe_dir = tmp
        .path()
        .join("Games")
        .join("FreightFate.app")
        .join("Contents")
        .join("MacOS");
    let old_profile = exe_dir.join("saves").join("profiles").join("Driver.json");
    write(&old_profile, "{}");
    let app_support = tmp.path().join("Application Support").join("FreightFate");
    let roots = SaveRoots {
        override_dir: None,
        game_root: tmp.path().join("Games"),
        legacy_data_dir: app_support.clone(),
        macos_data_dir: app_support.clone(),
        macos: true,
        frozen_exe_dir: Some(exe_dir.clone()),
        writable: None,
    };

    let target = data_dir_of(&roots);
    assert_eq!(target, app_support);
    assert!(target.join("profiles").join("Driver.json").is_file());
    assert!(!exe_dir.join("saves").exists());
}

/// The reported bug: saves dropped in /Applications next to the .app are
/// relocated into Application Support.
#[test]
fn test_macos_moves_saves_beside_app_bundle() {
    let tmp = tempfile::tempdir().unwrap();
    let applications = tmp.path().join("Applications");
    let beside = applications
        .join("saves")
        .join("profiles")
        .join("Driver.json");
    write(&beside, "{}");
    let app_support = tmp.path().join("Application Support").join("FreightFate");
    let roots = SaveRoots {
        override_dir: None,
        game_root: applications.clone(),
        legacy_data_dir: app_support.clone(),
        macos_data_dir: app_support.clone(),
        macos: true,
        frozen_exe_dir: None,
        writable: None,
    };

    let target = data_dir_of(&roots);
    assert_eq!(target, app_support);
    assert!(target.join("profiles").join("Driver.json").is_file());
    assert!(!applications.join("saves").exists());
}
