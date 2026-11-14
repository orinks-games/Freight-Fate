//! Local encryption surrounds the existing signed save container. Cloud payloads
//! stay portable JSON and retain their independent server verification.
use hearken_seal::{Opened, Vault};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

fn name(path: &Path) -> io::Result<&str> {
    path.file_name().and_then(|n| n.to_str()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "The career has no valid file name.",
        )
    })
}

fn vault(path: &Path) -> Vault {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    #[cfg(test)]
    let key = TEST_KEY
        .with(|k| k.borrow().clone())
        .or_else(hearken_seal::build_key);
    #[cfg(not(test))]
    let key = hearken_seal::build_key();
    Vault::with_key(parent, key).with_witnesses(vec![parent.join(".freight-fate-sealed")])
}

/// Read without moving or rewriting a locked, forged, or inaccessible career.
/// A genuine legacy container still passes the existing HMAC checks before import.
pub fn read(path: &Path) -> io::Result<(Vec<u8>, bool)> {
    match vault(path).read(name(path)?) {
        Opened::Sealed(bytes) | Opened::Plain(bytes) => Ok((bytes, false)),
        Opened::Legacy(bytes) => Ok((bytes, true)),
        Opened::Missing => Err(io::Error::new(io::ErrorKind::NotFound, "Career not found.")),
        Opened::Locked => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "This copy of the game cannot open this career. Use a released version of Freight Fate.",
        )),
        Opened::Tampered(reason) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("The career file could not be verified: {reason}. Under Online, restore a cloud backup."),
        )),
        Opened::Failed(error) => {
            log::warn!("Could not read career {}: {error}", path.display());
            Err(io::Error::new(error.kind(), "The career file was left unchanged. Try again; if it still fails, use Report a problem."))
        },
    }
}

/// Check before a cloud restore moves an existing file out of the way.
pub fn ensure_writable(path: &Path) -> io::Result<()> {
    if matches!(vault(path).read(name(path)?), Opened::Locked) {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "This build cannot replace a sealed career. Use the released game.",
        ));
    }
    Ok(())
}

/// Delete only through the same key-aware storage boundary as writing.
pub fn remove(path: &Path) -> io::Result<()> {
    vault(path).remove(name(path)?)
}

/// Vault writes atomically, keeps a legacy backup, and refuses unsealed developer
/// builds permission to overwrite a release's sealed career.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    vault(path).write(name(path)?, bytes)
}

/// What a save scan renames a career it cannot decode to: `X.ffsave.invalid`,
/// then `.invalid1`, `.invalid2` ([`super::signing::quarantine`]).
const SET_ASIDE: &str = ".invalid";
/// The first bytes of every sealed file.
const SEALED_MAGIC: &[u8; 4] = b"HKSL";

fn starts_sealed(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 4];
    std::fs::File::open(path)
        .and_then(|mut file| file.read_exact(&mut head))
        .is_ok()
        && &head == SEALED_MAGIC
}

/// Bring back sealed careers that an older release set aside.
///
/// A release from before sealing (1.9.5 and earlier) cannot decode a sealed
/// career, so its save scan renames the file to `X.ffsave.invalid`. One start
/// of such a release on this saves folder would leave this build with no
/// careers to list. A set-aside sealed file goes back under its own name when
/// no career holds that name and the file opens there; anything else stays
/// exactly where it is. Returns how many came back.
pub fn restore_set_aside(dir: &Path) -> usize {
    if !vault(&dir.join("career")).is_sealing() {
        // A build without the key could not open one, so it moves nothing.
        return 0;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    // The newest set-aside copy of each career file name.
    let mut newest: HashMap<String, (SystemTime, PathBuf)> = HashMap::new();
    for aside in entries.flatten().map(|entry| entry.path()) {
        let Some(file_name) = aside.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some((career, counter)) = file_name.rsplit_once(SET_ASIDE) else {
            continue;
        };
        if !career.ends_with(super::SAVE_SUFFIX)
            || !counter.bytes().all(|b| b.is_ascii_digit())
            || dir.join(career).exists()
            || !starts_sealed(&aside)
        {
            continue;
        }
        let modified = aside
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(SystemTime::UNIX_EPOCH);
        if newest.get(career).is_none_or(|(seen, _)| modified > *seen) {
            newest.insert(career.to_string(), (modified, aside));
        }
    }
    let mut restored = 0;
    for (career, (_, aside)) in newest {
        let home = dir.join(&career);
        if let Err(e) = std::fs::rename(&aside, &home) {
            log::warn!("Could not bring back {}: {e}", aside.display());
            continue;
        }
        // The seal covers the file's name, so it can only be judged in place.
        let opens = read(&home).is_ok_and(|(bytes, _)| super::decode_save_bytes(&bytes).is_ok());
        if opens {
            log::info!(
                "Brought back {career}, which an older version had set aside as {}",
                aside.display()
            );
            restored += 1;
        } else if let Err(e) = std::fs::rename(&home, &aside) {
            log::warn!("Could not put {} back aside: {e}", home.display());
        }
    }
    restored
}

#[cfg(test)]
thread_local! {
    static TEST_KEY: std::cell::RefCell<Option<hearken_seal::Key>> = const { std::cell::RefCell::new(None) };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::profile::{decode_save_bytes, encode_save_bytes, origin, Profile};
    struct KeyGuard;
    impl Drop for KeyGuard {
        fn drop(&mut self) {
            TEST_KEY.with(|k| *k.borrow_mut() = None);
        }
    }
    fn sealing() -> KeyGuard {
        TEST_KEY.with(|k| *k.borrow_mut() = Some(hearken_seal::Key::from_bytes([37; 32])));
        KeyGuard
    }
    #[test]
    fn sealed_profile_round_trip_preserves_board_payload_and_integrity() {
        crate::models::profile::tests::with_data_dir(|_| {
            let _key = sealing();
            let mut p = Profile::new();
            p.name = "José Driver 東京".into();
            let before = origin::cloud_content(&serde_json::Value::Object(p.to_dict()));
            let path = p.save().unwrap();
            assert!(std::fs::read(&path).unwrap().starts_with(b"HKSL"));
            let back = Profile::load_with(&path, true).unwrap();
            assert!(!back.integrity_modified);
            assert_eq!(
                origin::cloud_content(&serde_json::Value::Object(back.to_dict())),
                before
            );
        });
    }
    #[test]
    fn legacy_import_keeps_backup_and_rejects_a_plain_replacement() {
        crate::models::profile::tests::with_data_dir(|_| {
            let p = Profile::new();
            let path = p.save().unwrap();
            let original = std::fs::read(&path).unwrap();
            let _key = sealing();
            assert!(!Profile::load_with(&path, true).unwrap().integrity_modified);
            assert!(std::fs::read(&path).unwrap().starts_with(b"HKSL"));
            assert_eq!(
                std::fs::read(path.with_file_name(format!(
                    "{}.plain-backup",
                    path.file_name().unwrap().to_str().unwrap()
                )))
                .unwrap(),
                original
            );
            std::fs::write(&path, original).unwrap();
            assert!(read(&path).is_err());
        });
    }
    #[test]
    fn forged_and_locked_saves_are_preserved() {
        crate::models::profile::tests::with_data_dir(|_| {
            let key = sealing();
            let p = Profile::new();
            let path = p.save().unwrap();
            let original = std::fs::read(&path).unwrap();
            drop(key);
            assert!(Profile::load_with(&path, false).is_err());
            assert!(p.save().is_err());
            assert!(ensure_writable(&path).is_err());
            origin::record_origin(&p.name, "verified-cloud-origin");
            p.delete();
            assert_eq!(
                origin::origin_for(&p.name).as_deref(),
                Some("verified-cloud-origin")
            );
            assert_eq!(std::fs::read(&path).unwrap(), original);
            let _key = sealing();
            let mut bad = original;
            let last = bad.len() - 1;
            bad[last] ^= 1;
            std::fs::write(&path, &bad).unwrap();
            assert!(Profile::load_with(&path, true).is_err());
            assert_eq!(std::fs::read(&path).unwrap(), bad);
        });
    }
    /// What 1.9.5's save scan does to a career it cannot decode.
    fn set_aside_like_an_older_release(path: &Path) -> PathBuf {
        crate::models::profile::signing::quarantine(path).unwrap()
    }
    #[test]
    fn a_sealed_career_an_older_release_set_aside_comes_back() {
        crate::models::profile::tests::with_data_dir(|_| {
            let _key = sealing();
            let path = Profile::new().save().unwrap();
            let sealed = std::fs::read(&path).unwrap();
            let aside = set_aside_like_an_older_release(&path);
            assert!(!path.exists());
            assert_eq!(Profile::list_saves(), vec![path.clone()]);
            assert!(!aside.exists());
            assert_eq!(std::fs::read(&path).unwrap(), sealed);
            assert!(!Profile::load_with(&path, true).unwrap().integrity_modified);
        });
    }
    #[test]
    fn the_newest_set_aside_copy_is_the_one_that_comes_back() {
        crate::models::profile::tests::with_data_dir(|_| {
            let _key = sealing();
            let mut p = Profile::new();
            let path = p.save().unwrap();
            let older = set_aside_like_an_older_release(&path);
            p.spend(250.0);
            p.save().unwrap();
            let newer = set_aside_like_an_older_release(&path);
            let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
            std::fs::File::options()
                .write(true)
                .open(&newer)
                .unwrap()
                .set_modified(later)
                .unwrap();
            assert_eq!(restore_set_aside(path.parent().unwrap()), 1);
            assert!(older.exists());
            assert!(!newer.exists());
            assert_eq!(Profile::load_with(&path, true).unwrap().money(), p.money());
        });
    }
    #[test]
    fn a_set_aside_file_that_would_not_open_stays_aside() {
        crate::models::profile::tests::with_data_dir(|_| {
            let _key = sealing();
            let path = Profile::new().save().unwrap();
            let dir = path.parent().unwrap().to_path_buf();
            // A sealed file damaged on disk.
            let mut bad = std::fs::read(&path).unwrap();
            let last = bad.len() - 1;
            bad[last] ^= 1;
            std::fs::write(&path, &bad).unwrap();
            let aside = set_aside_like_an_older_release(&path);
            // A plain file this build set aside itself.
            let plain = dir.join("Other.ffsave.invalid");
            std::fs::write(&plain, b"FFSAVE1\0not a career").unwrap();
            assert_eq!(restore_set_aside(&dir), 0);
            assert!(Profile::list_saves().is_empty());
            assert_eq!(std::fs::read(&aside).unwrap(), bad);
            assert!(plain.exists());
            assert!(!path.exists());
        });
    }
    #[test]
    fn a_set_aside_copy_never_replaces_a_career_that_is_there() {
        crate::models::profile::tests::with_data_dir(|_| {
            let _key = sealing();
            let mut p = Profile::new();
            let path = p.save().unwrap();
            let aside = set_aside_like_an_older_release(&path);
            p.spend(250.0);
            p.save().unwrap();
            let current = std::fs::read(&path).unwrap();
            assert_eq!(restore_set_aside(path.parent().unwrap()), 0);
            assert!(aside.exists());
            assert_eq!(std::fs::read(&path).unwrap(), current);
        });
    }
    #[test]
    fn a_build_without_the_key_leaves_set_aside_careers_alone() {
        crate::models::profile::tests::with_data_dir(|_| {
            let key = sealing();
            let path = Profile::new().save().unwrap();
            let aside = set_aside_like_an_older_release(&path);
            drop(key);
            assert!(Profile::list_saves().is_empty());
            assert!(aside.exists());
        });
    }
    #[test]
    fn restored_portable_payload_is_signed_locally_then_sealed() {
        crate::models::profile::tests::with_data_dir(|_| {
            let _key = sealing();
            let p = Profile::new();
            let payload = p.to_dict();
            let path = p.path();
            write(&path, &encode_save_bytes(&payload)).unwrap();
            let (bytes, legacy) = read(&path).unwrap();
            assert!(!legacy);
            assert_eq!(decode_save_bytes(&bytes).unwrap().0, payload);
            assert!(!Profile::load_with(&path, true).unwrap().integrity_modified);
        });
    }
}
