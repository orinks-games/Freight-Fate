use super::*;

#[test]
fn snapshot_labels() {
    assert!(is_snapshot_label("1.9-tester-20261017"));
    assert!(is_snapshot_label("nightly-20260611"));
    assert!(!is_snapshot_label("1.9-tester-2026101"));
    assert!(!is_snapshot_label("v1.9.4"));
    assert_eq!(label_package_version("v1.9.4").unwrap(), "1.9.4");
}

#[test]
fn project_version_is_the_dev_version() {
    let version = project_version().unwrap();
    assert!(version.starts_with(env!("CARGO_PKG_VERSION")), "{version}");
    assert!(version.contains(".dev"), "{version}");
}

#[test]
fn secret_scan_flags_tokens_and_names() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("notes.md"),
        "ghp_abcdefghijklmnopqrstuvwxyz",
    )
    .unwrap();
    std::fs::write(dir.path().join(".env.local"), "x").unwrap();
    std::fs::write(
        dir.path().join("game.bin"),
        "ghp_abcdefghijklmnopqrstuvwxyz",
    )
    .unwrap();
    let err = verify_no_shipped_secrets(dir.path()).unwrap_err();
    assert!(err.contains(".env.local: secret-shaped file name"));
    assert!(err.contains("notes.md: looks like a GitHub token"));
    assert!(!err.contains("game.bin"));
}

#[test]
fn native_libraries_skip_build_leftovers() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["SDL2.dll", "freightfate.pdb", "x.lib", "deps"] {
        if name == "deps" {
            std::fs::create_dir(dir.path().join(name)).unwrap();
        } else {
            std::fs::write(dir.path().join(name), "").unwrap();
        }
    }
    let libs = rust_native_libraries(dir.path(), &["dll"]);
    assert_eq!(libs, vec![dir.path().join("SDL2.dll")]);
}

#[test]
fn appimage_launcher_runs_the_staged_executable() {
    // The AppImage copies the staged folder to opt/freightfate, and AppRun
    // execs the game executable inside it. Copying build/ instead put the
    // staged folder itself at opt/freightfate/FreightFate.
    let apprun = std::fs::read_to_string(util::repo_root().join("tools/appimage/AppRun"))
        .expect("AppRun is committed");
    assert!(apprun.contains(&format!("\"$HERE/opt/freightfate/{APP_NAME}\"")));
    assert_eq!(staged_dir(), build_dir().join(APP_NAME));
}
