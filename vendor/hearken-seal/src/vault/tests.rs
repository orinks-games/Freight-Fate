use super::*;

const NAME: &str = "records.json";

fn sealing() -> (tempfile::TempDir, Vault, Key) {
    let dir = tempfile::tempdir().unwrap();
    let key = Key::generate().unwrap();
    let vault = Vault::with_key(dir.path(), Some(key.clone()));
    (dir, vault, key)
}

/// A new vault on the same folder, as the next run of the game sees it.
fn reopen(dir: &tempfile::TempDir, key: &Key) -> Vault {
    Vault::with_key(dir.path(), Some(key.clone()))
}

fn tampered(opened: Opened) -> Tamper {
    match opened {
        Opened::Tampered(how) => how,
        other => panic!("expected Tampered, got {other:?}"),
    }
}

fn sealed(opened: Opened) -> Vec<u8> {
    match opened {
        Opened::Sealed(bytes) => bytes,
        other => panic!("expected Sealed, got {other:?}"),
    }
}

#[test]
fn a_sealed_file_round_trips_and_cannot_be_read() {
    let (dir, mut vault, key) = sealing();
    assert!(vault.is_sealing());
    assert!(matches!(vault.read(NAME), Opened::Missing));
    vault.write(NAME, b"{\"best\":12}").unwrap();
    assert_eq!(sealed(vault.read(NAME)), b"{\"best\":12}");
    assert_eq!(sealed(reopen(&dir, &key).read(NAME)), b"{\"best\":12}");
    let raw = std::fs::read(dir.path().join(NAME)).unwrap();
    assert!(raw.starts_with(b"HKSL"));
    assert!(!raw.windows(4).any(|w| w == b"best"));
    let names: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(names, [NAME], "the folder holds only the game's file");
    assert!(!dir.path().join(name::temp(NAME)).exists());
}

#[test]
fn a_build_without_a_key_keeps_plain_files() {
    let dir = tempfile::tempdir().unwrap();
    let mut vault = Vault::with_key(dir.path(), None);
    assert!(!vault.is_sealing());
    assert!(matches!(vault.read(NAME), Opened::Missing));
    vault.write(NAME, b"{\"best\":3}").unwrap();
    assert_eq!(
        std::fs::read(dir.path().join(NAME)).unwrap(),
        b"{\"best\":3}"
    );
    assert_eq!(vault.read(NAME).trusted().unwrap(), b"{\"best\":3}");
    vault.remove(NAME).unwrap();
    assert!(matches!(vault.read(NAME), Opened::Missing));
}

#[test]
fn every_changed_byte_is_forged() {
    let key = Key::generate().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let (dir, mut vault) = witnessed(&key, &elsewhere);
    let reopen = |dir: &tempfile::TempDir, key: &Key| {
        Vault::with_key(dir.path(), Some(key.clone())).with_witnesses(vault_witness(&elsewhere))
    };
    vault.write(NAME, b"{\"best\":12}").unwrap();
    let path = dir.path().join(NAME);
    let good = std::fs::read(&path).unwrap();
    for at in 0..good.len() {
        let mut bad = good.clone();
        bad[at] ^= 0x20;
        std::fs::write(&path, &bad).unwrap();
        let how = tampered(reopen(&dir, &key).read(NAME));
        // A changed magic makes it a plain file over a sealed one.
        let expected = if at < 4 {
            Tamper::Unsealed
        } else {
            Tamper::Forged
        };
        assert_eq!(how, expected, "byte {at}");
    }
    std::fs::write(&path, &good[..good.len() - 3]).unwrap();
    assert_eq!(tampered(reopen(&dir, &key).read(NAME)), Tamper::Forged);
    let mut longer = good.clone();
    longer.extend_from_slice(b"99");
    std::fs::write(&path, &longer).unwrap();
    assert_eq!(tampered(reopen(&dir, &key).read(NAME)), Tamper::Forged);
    std::fs::write(&path, &good).unwrap();
    assert!(reopen(&dir, &key).read(NAME).trusted().is_some());
}

#[test]
fn a_file_copied_over_another_is_forged() {
    let (dir, mut vault, key) = sealing();
    vault.write("a.json", b"a's").unwrap();
    vault.write("b.json", b"b's").unwrap();
    std::fs::copy(dir.path().join("a.json"), dir.path().join("b.json")).unwrap();
    assert_eq!(tampered(reopen(&dir, &key).read("b.json")), Tamper::Forged);
    assert_eq!(sealed(reopen(&dir, &key).read("a.json")), b"a's");
}

#[test]
fn another_keys_file_is_forged() {
    let (dir, mut vault, key) = sealing();
    vault.write(NAME, b"mine").unwrap();
    let other = tempfile::tempdir().unwrap();
    let mut theirs = Vault::with_key(other.path(), Some(Key::generate().unwrap()));
    theirs.write(NAME, b"theirs").unwrap();
    std::fs::copy(other.path().join(NAME), dir.path().join(NAME)).unwrap();
    assert_eq!(tampered(reopen(&dir, &key).read(NAME)), Tamper::Forged);
}

#[test]
fn a_leftover_temporary_file_changes_nothing() {
    let (dir, mut vault, key) = sealing();
    vault.write(NAME, b"good").unwrap();
    std::fs::write(dir.path().join(name::temp(NAME)), b"half a file").unwrap();
    assert_eq!(sealed(reopen(&dir, &key).read(NAME)), b"good");
    vault.write(NAME, b"better").unwrap();
    assert_eq!(sealed(reopen(&dir, &key).read(NAME)), b"better");
}

#[test]
fn a_legacy_plain_file_can_be_imported_once() {
    let key = Key::generate().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let (dir, mut vault) = witnessed(&key, &elsewhere);
    std::fs::write(dir.path().join(NAME), b"{\"best\":7}").unwrap();
    match vault.read(NAME) {
        Opened::Legacy(bytes) => assert_eq!(bytes, b"{\"best\":7}"),
        other => panic!("expected Legacy, got {other:?}"),
    }
    vault.write(NAME, b"{\"best\":7}").unwrap();
    assert_eq!(
        std::fs::read(dir.path().join("records.json.plain-backup")).unwrap(),
        b"{\"best\":7}"
    );
    assert_eq!(sealed(vault.read(NAME)), b"{\"best\":7}");
    std::fs::write(dir.path().join(NAME), b"{\"best\":9999}").unwrap();
    assert_eq!(tampered(vault.read(NAME)), Tamper::Unsealed);
}

#[test]
fn a_build_without_the_key_cannot_read_or_overwrite_sealed_files() {
    let (dir, mut vault, _key) = sealing();
    vault.write(NAME, b"release save").unwrap();
    let before = std::fs::read(dir.path().join(NAME)).unwrap();
    let mut dev = Vault::with_key(dir.path(), None);
    assert!(matches!(dev.read(NAME), Opened::Locked));
    assert_eq!(
        dev.write(NAME, b"dev").unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        dev.remove(NAME).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(
        dev.set_aside(NAME).unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert_eq!(std::fs::read(dir.path().join(NAME)).unwrap(), before);
    // A build with another key cannot read them either.
    let mut other = Vault::with_key(dir.path(), Some(Key::generate().unwrap()));
    assert_eq!(tampered(other.read(NAME)), Tamper::Forged);
}

#[test]
fn remove_and_set_aside_leave_the_name_missing() {
    let (dir, mut vault, key) = sealing();
    vault.write(NAME, b"x").unwrap();
    vault.remove(NAME).unwrap();
    assert!(!dir.path().join(NAME).exists());
    assert!(matches!(reopen(&dir, &key).read(NAME), Opened::Missing));
    vault.remove(NAME).unwrap();

    vault.write(NAME, b"z").unwrap();
    std::fs::write(dir.path().join(NAME), b"HKSL edited").unwrap();
    let mut vault = reopen(&dir, &key);
    assert_eq!(tampered(vault.read(NAME)), Tamper::Forged);
    let first = vault.set_aside(NAME).unwrap().unwrap();
    assert_eq!(std::fs::read(&first).unwrap(), b"HKSL edited");
    assert!(matches!(vault.read(NAME), Opened::Missing));
    // A second one goes beside the first.
    std::fs::write(dir.path().join(NAME), b"again").unwrap();
    let second = vault.set_aside(NAME).unwrap().unwrap();
    assert!(second.ends_with("records.json.tampered-2"));
    assert_eq!(vault.set_aside(NAME).unwrap(), None);
    // A fresh write reads back.
    vault.write(NAME, b"w").unwrap();
    assert_eq!(sealed(reopen(&dir, &key).read(NAME)), b"w");
}

#[test]
fn an_older_copy_or_another_install_s_copy_is_accepted() {
    // The documented limit: each file stands alone.
    let (dir, mut vault, key) = sealing();
    vault.write(NAME, b"first").unwrap();
    let old = std::fs::read(dir.path().join(NAME)).unwrap();
    vault.write(NAME, b"second").unwrap();
    std::fs::write(dir.path().join(NAME), &old).unwrap();
    assert_eq!(sealed(reopen(&dir, &key).read(NAME)), b"first");
    let other = tempfile::tempdir().unwrap();
    std::fs::copy(dir.path().join(NAME), other.path().join(NAME)).unwrap();
    assert_eq!(sealed(reopen(&other, &key).read(NAME)), b"first");
}

#[test]
fn bad_names_are_refused() {
    let (_dir, mut vault, _key) = sealing();
    for bad in ["../x", "a/b", ".hidden", "x.plain-backup", &"x".repeat(241)] {
        assert!(matches!(
            vault.read(bad),
            Opened::Failed(e) if e.kind() == io::ErrorKind::InvalidInput
        ));
        assert_eq!(
            vault.write(bad, b"x").unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            vault.remove(bad).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            vault.set_aside(bad).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}

#[test]
fn a_folder_in_place_of_the_file_fails_and_is_never_trusted() {
    let (dir, mut vault, _key) = sealing();
    std::fs::create_dir(dir.path().join(NAME)).unwrap();
    assert!(matches!(vault.read(NAME), Opened::Failed(_)));
    assert!(vault.write(NAME, b"x").is_err());
}

#[cfg(unix)]
#[test]
fn a_link_to_another_file_is_judged_by_what_it_points_at() {
    let (dir, mut vault, key) = sealing();
    vault.write("a.json", b"a").unwrap();
    vault.write(NAME, b"mine").unwrap();
    std::fs::remove_file(dir.path().join(NAME)).unwrap();
    std::os::unix::fs::symlink(dir.path().join("a.json"), dir.path().join(NAME)).unwrap();
    assert_eq!(tampered(reopen(&dir, &key).read(NAME)), Tamper::Forged);
    // A dangling link is a missing file.
    std::fs::remove_file(dir.path().join(NAME)).unwrap();
    std::os::unix::fs::symlink(dir.path().join("nowhere"), dir.path().join(NAME)).unwrap();
    assert!(matches!(reopen(&dir, &key).read(NAME), Opened::Missing));
    // Writing replaces the link, not what it points at.
    let mut vault = reopen(&dir, &key);
    vault.set_aside(NAME).unwrap();
    vault.write(NAME, b"fresh").unwrap();
    assert!(!std::fs::symlink_metadata(dir.path().join(NAME))
        .unwrap()
        .file_type()
        .is_symlink());
}

#[test]
fn only_sealed_and_plain_are_trusted() {
    assert_eq!(Opened::Sealed(vec![1]).trusted(), Some(vec![1]));
    assert_eq!(Opened::Plain(vec![2]).trusted(), Some(vec![2]));
    assert_eq!(Opened::Legacy(vec![3]).trusted(), None);
    assert_eq!(Opened::Locked.trusted(), None);
    assert_eq!(Opened::Missing.trusted(), None);
    assert_eq!(Opened::Tampered(Tamper::Forged).trusted(), None);
}

fn vault_witness(elsewhere: &tempfile::TempDir) -> Vec<PathBuf> {
    vec![elsewhere.path().join("game").join("witness")]
}

/// A sealing vault on a folder of its own, with one witness in `elsewhere`.
fn witnessed(key: &Key, elsewhere: &tempfile::TempDir) -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    let vault =
        Vault::with_key(dir.path(), Some(key.clone())).with_witnesses(vault_witness(elsewhere));
    (dir, vault)
}

#[test]
fn a_plain_file_after_a_set_aside_or_remove_is_unsealed_once_witnessed() {
    // The attack: get the file set aside (or reset), then drop in a plain
    // one, hoping it imports as legacy.
    let key = Key::generate().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let (dir, mut vault) = witnessed(&key, &elsewhere);
    vault.write(NAME, b"{\"best\":20}").unwrap();
    std::fs::write(dir.path().join(NAME), b"garbled").unwrap();
    assert_eq!(tampered(vault.read(NAME)), Tamper::Unsealed);
    vault.set_aside(NAME).unwrap();
    std::fs::write(dir.path().join(NAME), b"{\"best\":999999}").unwrap();
    assert_eq!(tampered(vault.read(NAME)), Tamper::Unsealed);
    vault.remove(NAME).unwrap();
    std::fs::write(dir.path().join(NAME), b"{\"best\":999999}").unwrap();
    assert_eq!(tampered(vault.read(NAME)), Tamper::Unsealed);
}

#[test]
fn a_witness_stops_a_wiped_folder_from_importing_a_plain_file() {
    // The attack: delete the sealed file, then drop in a hand-written
    // plain file, as if from before sealing.
    let key = Key::generate().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let (dir, mut vault) = witnessed(&key, &elsewhere);
    std::fs::write(dir.path().join(NAME), b"{\"best\":20}").unwrap();
    // The genuine first import still works: nothing has sealed yet.
    assert!(matches!(vault.read(NAME), Opened::Legacy(_)));
    vault.write(NAME, b"{\"best\":20}").unwrap();
    assert!(elsewhere.path().join("game").join("witness").is_file());
    std::fs::remove_file(dir.path().join(NAME)).unwrap();
    std::fs::write(dir.path().join(NAME), b"{\"best\":999999}").unwrap();
    let mut again = Vault::with_key(dir.path(), Some(key.clone()))
        .with_witnesses(vec![elsewhere.path().join("game").join("witness")]);
    assert_eq!(tampered(again.read(NAME)), Tamper::Unsealed);
}

#[test]
fn a_witness_stops_a_fresh_folder_from_importing_a_plain_file() {
    // The attack: point the game at a new folder (portable.txt, XDG_DATA_HOME)
    // holding a hand-written plain file.
    let key = Key::generate().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let (_dir, mut vault) = witnessed(&key, &elsewhere);
    vault.write(NAME, b"{\"best\":20}").unwrap();
    let (fresh, mut other) = witnessed(&key, &elsewhere);
    std::fs::write(fresh.path().join(NAME), b"{\"best\":999999}").unwrap();
    assert_eq!(tampered(other.read(NAME)), Tamper::Unsealed);
    // An empty fresh folder is simply empty.
    let (_empty, mut empty) = witnessed(&key, &elsewhere);
    assert!(matches!(empty.read(NAME), Opened::Missing));
}

#[test]
fn a_witness_that_cannot_be_written_does_not_stop_saving() {
    let key = Key::generate().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::fs::write(elsewhere.path().join("game"), b"a file, not a folder").unwrap();
    let (dir, mut vault) = witnessed(&key, &elsewhere);
    vault.write(NAME, b"x").unwrap();
    assert_eq!(sealed(reopen(&dir, &key).read(NAME)), b"x");
}

#[test]
fn a_witness_never_reads_as_a_data_file() {
    // The attack: copy the witness into the data folder, under the name it
    // used to be sealed under or under a game's own.
    let key = Key::generate().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let (dir, mut vault) = witnessed(&key, &elsewhere);
    vault.write(NAME, b"x").unwrap();
    let witness = std::fs::read(&vault_witness(&elsewhere)[0]).unwrap();
    for name in ["hearken-seal.witness", "records.json", "witness"] {
        std::fs::write(dir.path().join(name), &witness).unwrap();
        assert_eq!(tampered(vault.read(name)), Tamper::Forged, "{name}");
    }
    assert!(name::check(WITNESS).is_err());
}

#[test]
fn every_old_plain_save_can_be_imported_after_the_first() {
    // An older, unsealed version left two genuine saves. Sealing the first
    // must not turn the second into tampering, in this run or the next.
    let key = Key::generate().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let (dir, mut vault) = witnessed(&key, &elsewhere);
    for slot in ["slot1.json", "slot2.json", "slot3.json"] {
        std::fs::write(dir.path().join(slot), format!("old {slot}")).unwrap();
    }
    assert!(matches!(vault.read("slot1.json"), Opened::Legacy(_)));
    vault.write("slot1.json", b"old slot1.json").unwrap();
    assert!(vault_witness(&elsewhere)[0].is_file());
    match vault.read("slot2.json") {
        Opened::Legacy(bytes) => assert_eq!(bytes, b"old slot2.json"),
        other => panic!("expected Legacy, got {other:?}"),
    }
    let mut next =
        Vault::with_key(dir.path(), Some(key.clone())).with_witnesses(vault_witness(&elsewhere));
    assert!(matches!(next.read("slot2.json"), Opened::Legacy(_)));
    next.write("slot2.json", b"old slot2.json").unwrap();
    assert!(matches!(next.read("slot3.json"), Opened::Legacy(_)));
    // A sealed name, or one that was never an old save, is not importable.
    std::fs::write(dir.path().join("slot1.json"), b"hand").unwrap();
    std::fs::write(dir.path().join("slot2.json"), b"hand").unwrap();
    std::fs::write(dir.path().join("slot4.json"), b"hand").unwrap();
    for slot in ["slot1.json", "slot2.json", "slot4.json"] {
        assert_eq!(tampered(next.read(slot)), Tamper::Unsealed, "{slot}");
    }
    // The folder holds only the game's files and their backups.
    let mut names: Vec<String> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "slot1.json",
            "slot1.json.plain-backup",
            "slot2.json",
            "slot2.json.plain-backup",
            "slot3.json",
            "slot4.json"
        ]
    );
}

#[test]
fn a_witness_that_does_not_authenticate_allows_no_import() {
    let key = Key::generate().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let (dir, mut vault) = witnessed(&key, &elsewhere);
    let witness = &vault_witness(&elsewhere)[0];
    std::fs::create_dir_all(witness.parent().unwrap()).unwrap();
    std::fs::write(witness, b"records.json").unwrap();
    std::fs::write(dir.path().join(NAME), b"{}").unwrap();
    assert_eq!(tampered(vault.read(NAME)), Tamper::Unsealed);
}

#[test]
fn a_checked_witness_follows_the_game_to_another_folder() {
    // The attack: the installed copy has sealed; add portable.txt (or
    // delete it), which moves both the data folder and the witness written,
    // and drop a hand-written plain file in the new folder.
    let key = Key::generate().unwrap();
    let config = tempfile::tempdir().unwrap();
    let exe = tempfile::tempdir().unwrap();
    let installed_witness = config.path().join("Game.hearken-seal");
    let portable_witness = exe.path().join("hearken-seal.witness");
    let installed = tempfile::tempdir().unwrap();
    std::fs::write(installed.path().join("old.json"), b"old").unwrap();
    Vault::with_key(installed.path(), Some(key.clone()))
        .with_witnesses(vec![installed_witness.clone()])
        .checking_witnesses(vec![portable_witness.clone()])
        .write(NAME, b"{\"best\":20}")
        .unwrap();
    assert!(!portable_witness.exists());
    let portable = tempfile::tempdir().unwrap();
    let mut moved = Vault::with_key(portable.path(), Some(key.clone()))
        .with_witnesses(vec![portable_witness.clone()])
        .checking_witnesses(vec![installed_witness.clone()]);
    std::fs::write(portable.path().join(NAME), b"{\"best\":999}").unwrap();
    assert_eq!(tampered(moved.read(NAME)), Tamper::Unsealed);
    // Its own first write carries the record on, never widening it.
    moved.write("other.json", b"x").unwrap();
    assert!(portable_witness.is_file());
    let mut only_portable = Vault::with_key(portable.path(), Some(key.clone()))
        .with_witnesses(vec![portable_witness.clone()]);
    assert_eq!(tampered(only_portable.read(NAME)), Tamper::Unsealed);
    std::fs::write(portable.path().join("old.json"), b"old").unwrap();
    assert!(matches!(only_portable.read("old.json"), Opened::Legacy(_)));
}

#[cfg(unix)]
#[test]
fn a_planted_temporary_link_is_never_written_through() {
    let (dir, mut vault, key) = sealing();
    let outside = tempfile::tempdir().unwrap();
    let precious = outside.path().join("precious");
    std::fs::write(&precious, b"precious outside file").unwrap();
    std::os::unix::fs::symlink(&precious, dir.path().join(name::temp(NAME))).unwrap();
    vault.write("progress.json", b"progress").unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("progress.json"),
        dir.path().join(name::temp("settings2.json")),
    )
    .unwrap();
    vault.write(NAME, b"good").unwrap();
    vault.write("settings2.json", b"settings").unwrap();
    assert_eq!(std::fs::read(&precious).unwrap(), b"precious outside file");
    let meta = std::fs::symlink_metadata(dir.path().join(NAME)).unwrap();
    assert!(meta.file_type().is_file());
    let mut again = reopen(&dir, &key);
    assert_eq!(sealed(again.read(NAME)), b"good");
    assert_eq!(sealed(again.read("progress.json")), b"progress");
    assert_eq!(sealed(again.read("settings2.json")), b"settings");
    // A build without a key does not write through one either.
    let plain = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(&precious, plain.path().join(name::temp(NAME))).unwrap();
    Vault::with_key(plain.path(), None)
        .write(NAME, b"dev")
        .unwrap();
    assert_eq!(std::fs::read(&precious).unwrap(), b"precious outside file");
}

#[test]
fn a_leftover_temporary_folder_does_not_stop_saving() {
    let (dir, mut vault, key) = sealing();
    vault.write(NAME, b"old").unwrap();
    let tmp = dir.path().join(name::temp(NAME));
    std::fs::create_dir_all(tmp.join("inside")).unwrap();
    vault.write(NAME, b"new").unwrap();
    assert_eq!(sealed(reopen(&dir, &key).read(NAME)), b"new");
    assert!(!tmp.exists());
}

#[test]
fn a_build_without_a_key_adds_nothing_to_a_release_s_folder() {
    let (dir, mut vault, key) = sealing();
    vault.write(NAME, b"release save").unwrap();
    let mut dev = Vault::with_key(dir.path(), None);
    assert_eq!(
        dev.write("progress.json", b"dev").unwrap_err().kind(),
        io::ErrorKind::PermissionDenied
    );
    assert!(matches!(
        reopen(&dir, &key).read("progress.json"),
        Opened::Missing
    ));
    // A folder of plain files is a developer's own.
    let plain = tempfile::tempdir().unwrap();
    let mut dev = Vault::with_key(plain.path(), None);
    dev.write(NAME, b"a").unwrap();
    dev.write("progress.json", b"b").unwrap();
    dev.write(NAME, b"c").unwrap();
}
