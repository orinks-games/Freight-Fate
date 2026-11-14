//! A game's data files, sealed against editing in a release.
//!
//! A released game keeps its players' records and progress in files on
//! their computer, where anyone with a text editor could otherwise change
//! them. A [`Vault`] reads and writes those files through a seal: in a
//! release built with the game's secret, every file is encrypted and
//! authenticated (XChaCha20-Poly1305) with its own name bound in. A file
//! that was edited, swapped for another of the game's files, or forged
//! reads as [`Opened::Tampered`], never as good data. The folder holds the
//! game's own files and nothing else.
//!
//! The engine only reports what it found. Each game decides what to do
//! with a tampered file, and with a plain file from an older version
//! ([`Opened::Legacy`]).
//!
//! In every other build (a developer build, `cargo test`, `hearken play`)
//! there is no key, and the vault reads and writes plain files, so a
//! developer can look at and change them. Such a build reports a sealed
//! file as [`Opened::Locked`] and will not overwrite it.
//!
//! A vault given witnesses ([`Vault::with_witnesses`]) leaves a marker
//! outside the folder once it seals, listing the old plain files still to
//! import, so a hand-written plain file under any other name, or over one
//! already sealed, is no longer accepted as a legacy one.
//!
//! What this cannot do: the key is inside the program the player runs.
//! Someone willing to take that program apart can recover it and forge any
//! file. Each file stands alone, so putting back an older copy of a file
//! (or of the whole folder), deleting one, or copying one from another
//! player's install cannot be told apart from real play, since every copy
//! of a game shares its key. While a game imports legacy files, a player
//! who also clears the witnesses (or runs the game as a new user, on a new
//! computer, or with the platform folders moved) can get a plain file
//! imported. This raises cheating from "open a text editor" to "reverse
//! engineer the program"; no offline scheme can do more. Results that must
//! be trusted belong on a server that checks them.
//!
//! The file format:
//!
//! ```text
//! offset size
//! 0      4    "HKSL"
//! 4      1    version = 2
//! 5      3    reserved, zero
//! 8      24   nonce (random)
//! 32     ..   ciphertext of the file's bytes, then the 16-byte Poly1305 tag
//! ```
//!
//! The key is `HKDF-SHA256(salt = "hearken-seal", ikm = secret, info =
//! "files v1")`, and the associated data is the 32-byte header, then
//! `"hearken-seal/v2\0"`, then the file's name. Version 1 files (with a
//! folder id and a generation, kept by a separate ledger file) never shipped
//! in a release and read as forged.

mod format;
mod key;
mod name;
mod vault;

pub use key::Key;
pub use vault::{Opened, Tamper, Vault};

/// The build variable `hearken release` passes the game's secret in.
pub const KEY_VAR: &str = "HEARKEN_SEAL_KEY";

/// The key this build seals with: set by `hearken release` through
/// `HEARKEN_SEAL_KEY`, and `None` in every other build. Setting the
/// variable when the game runs does nothing: the key exists only at build
/// time.
pub fn build_key() -> Option<Key> {
    let bytes: Option<[u8; 32]> = include!(concat!(env!("OUT_DIR"), "/seal_key.rs"));
    bytes.map(Key::from_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_key_is_the_one_it_was_built_with() {
        // Meaningful when the tests are built with HEARKEN_SEAL_KEY set.
        match std::env::var(KEY_VAR).ok().filter(|k| !k.trim().is_empty()) {
            Some(hex) => assert_eq!(
                build_key().map(|k| k.to_hex()),
                Some(hex.trim().to_ascii_lowercase())
            ),
            None => assert_eq!(build_key(), None),
        }
    }
}
