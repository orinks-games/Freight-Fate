//! Local encryption surrounds the existing signed save container. Cloud payloads
//! stay portable JSON and retain their independent server verification.
use hearken_seal::{Opened, Vault};
use std::io;
use std::path::Path;

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
