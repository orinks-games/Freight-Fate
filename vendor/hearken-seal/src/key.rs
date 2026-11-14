//! A game's sealing secret.

use sha2::{Digest, Sha256};

/// A game's sealing secret: 32 random bytes the developer keeps, the same
/// for every release of the game. `Debug` prints only its [`Key::id`].
#[derive(Clone, PartialEq, Eq)]
pub struct Key([u8; 32]);

impl Key {
    pub fn from_bytes(bytes: [u8; 32]) -> Key {
        Key(bytes)
    }

    /// A new random key, from the operating system.
    pub fn generate() -> std::io::Result<Key> {
        let mut bytes = [0u8; 32];
        getrandom::fill(&mut bytes).map_err(|e| std::io::Error::other(e.to_string()))?;
        Ok(Key(bytes))
    }

    /// A key from 64 hex digits, surrounding whitespace allowed.
    pub fn from_hex(hex: &str) -> Option<Key> {
        let hex = hex.trim();
        if hex.len() != 64 || !hex.is_ascii() {
            return None;
        }
        let mut bytes = [0u8; 32];
        for (i, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(Key(bytes))
    }

    /// The key as 64 lowercase hex digits: what `HEARKEN_SEAL_KEY` takes.
    pub fn to_hex(&self) -> String {
        self.0.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// `k1:` and 16 hex digits, the first 8 bytes of
    /// `SHA-256("hearken-seal key id\0" || secret)`. It names the key
    /// without revealing it, so it is safe to commit as a game's `seal-id`.
    pub fn id(&self) -> String {
        let mut hash = Sha256::new();
        hash.update(b"hearken-seal key id\0");
        hash.update(self.0);
        let digest = hash.finalize();
        let hex: String = digest[..8].iter().map(|b| format!("{b:02x}")).collect();
        format!("k1:{hex}")
    }

    /// The key that seals files, derived from the secret.
    pub(crate) fn file_key(&self) -> [u8; 32] {
        let hk = hkdf::Hkdf::<Sha256>::new(Some(b"hearken-seal"), &self.0);
        let mut okm = [0u8; 32];
        hk.expand(b"files v1", &mut okm)
            .expect("32 bytes is a valid HKDF-SHA256 length");
        okm
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Key({})", self.id())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_id_names_the_key_without_showing_it() {
        let key = Key::from_bytes([7; 32]);
        let id = key.id();
        assert_eq!(id, Key::from_bytes([7; 32]).id());
        assert!(id.starts_with("k1:") && id.len() == 19);
        assert_ne!(id, Key::from_bytes([8; 32]).id());
        let hex = key.to_hex();
        assert!(!hex.contains(&id[3..]));
        let debug = format!("{key:?}");
        assert!(debug.contains(&id) && !debug.contains(&hex) && !debug.contains("07, 07"));
    }

    #[test]
    fn hex_round_trips() {
        let key = Key::generate().unwrap();
        assert_eq!(
            Key::from_hex(&format!(" {}\n", key.to_hex())),
            Some(key.clone())
        );
        assert_eq!(Key::from_hex(&key.to_hex().to_uppercase()), Some(key));
        assert_eq!(Key::from_hex("abc"), None);
        assert_eq!(Key::from_hex(&"zz".repeat(32)), None);
        assert_ne!(Key::generate().unwrap(), Key::generate().unwrap());
    }

    #[test]
    fn the_file_key_is_not_the_secret() {
        let key = Key::from_bytes([1; 32]);
        assert_ne!(key.file_key(), [1; 32]);
        assert_eq!(key.file_key(), Key::from_bytes([1; 32]).file_key());
    }
}
