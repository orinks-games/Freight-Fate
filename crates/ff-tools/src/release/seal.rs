//! Release-only save key provisioning. Never print or commit the secret.
use crate::util::Result;
use hearken_seal::Key;
use std::path::PathBuf;
use std::process::Command;

fn key_file() -> Result<PathBuf> {
    let update = super::updates::key_file()?;
    Ok(update
        .parent()
        .and_then(|p| p.parent())
        .ok_or("No key folder")?
        .join("seal-keys")
        .join("freight-fate.key"))
}
fn read_key() -> Result<Key> {
    let text = match std::env::var("HEARKEN_SEAL_KEY").ok().filter(|s| !s.trim().is_empty()) {
        Some(text) => text,
        None => std::fs::read_to_string(key_file()?).map_err(|_| "The career sealing key is missing. Set HEARKEN_SEAL_KEY or restore its local backup.".to_string())?,
    };
    Key::from_hex(text.trim()).ok_or("The career sealing key must contain 64 hex digits.".into())
}
fn validate_id(expected: &str, key: &Key) -> Result<()> {
    if key.id() != expected {
        return Err(
            "The career sealing key does not match seal-id. Refusing to build unreadable careers."
                .into(),
        );
    }
    Ok(())
}

pub fn configure(cmd: &mut Command) -> Result<()> {
    let expected = super::release_metadata("seal-id")?.filter(|s| !s.is_empty());
    match expected {
        Some(expected) => {
            let key = read_key()?;
            validate_id(&expected, &key)?;
            cmd.env("HEARKEN_SEAL_KEY", key.to_hex());
        }
        None => {
            cmd.env_remove("HEARKEN_SEAL_KEY");
        }
    }
    Ok(())
}
pub fn main(args: &[String]) -> Result<i32> {
    if args == ["--new"] {
        if super::release_metadata("seal-id")?.is_some() {
            return Err(
                "A seal-id is already configured; restore that key instead of replacing it.".into(),
            );
        }
        let key = Key::generate().map_err(|e| e.to_string())?;
        let path = key_file()?;
        super::updates::write_private(&path, &key.to_hex())?;
        println!("Saved the career sealing key at {}. Back it up securely. Add seal-id = \"{}\" to workspace.metadata.release and store this file as the repository HEARKEN_SEAL_KEY secret.", path.display(), key.id());
    } else if args.is_empty() {
        println!("Career sealing key ID: {}", read_key()?.id());
    } else {
        return Err("usage: seal-key [--new]".into());
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn release_key_must_match_the_persistent_identity() {
        let original = Key::from_bytes([1; 32]);
        let wrong = Key::from_bytes([2; 32]);
        assert!(validate_id(&original.id(), &original).is_ok());
        let error = validate_id(&original.id(), &wrong).unwrap_err();
        assert!(!error.contains(&wrong.to_hex()));
        assert!(!error.contains(&original.to_hex()));
        assert!(validate_id("invalid", &original).is_err());
    }
}
