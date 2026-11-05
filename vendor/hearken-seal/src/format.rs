//! One sealed file's bytes: a header, then the ciphertext and its tag.

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};

/// The first four bytes of every sealed file. Anything else is plain.
pub const MAGIC: &[u8; 4] = b"HKSL";
const VERSION: u8 = 2;
pub const HEADER_LEN: usize = 32;
const TAG_LEN: usize = 16;
/// Between the header and the name in the associated data, so the format
/// can change without old files authenticating as new ones.
const DOMAIN: &[u8] = b"hearken-seal/v2\0";

/// Whether `bytes` claim to be sealed. Whether they really are is for
/// [`open`] to say.
pub fn is_sealed(bytes: &[u8]) -> bool {
    bytes.starts_with(MAGIC)
}

fn aad(header: &[u8], name: &str) -> Vec<u8> {
    let mut aad = Vec::with_capacity(header.len() + DOMAIN.len() + name.len());
    aad.extend_from_slice(header);
    aad.extend_from_slice(DOMAIN);
    aad.extend_from_slice(name.as_bytes());
    aad
}

/// `plain` sealed under `name` with `file_key` and a fresh random nonce.
pub fn seal(file_key: &[u8; 32], name: &str, plain: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut nonce = [0u8; 24];
    getrandom::fill(&mut nonce).map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut header = Vec::with_capacity(HEADER_LEN);
    header.extend_from_slice(MAGIC);
    header.extend_from_slice(&[VERSION, 0, 0, 0]);
    header.extend_from_slice(&nonce);
    let cipher = XChaCha20Poly1305::new(&(*file_key).into());
    let sealed = cipher
        .encrypt(
            &XNonce::from(nonce),
            Payload {
                msg: plain,
                aad: &aad(&header, name),
            },
        )
        .map_err(|_| std::io::Error::other("the file is too large to seal"))?;
    header.extend_from_slice(&sealed);
    Ok(header)
}

/// The bytes of a file sealed under `name` with `file_key`, or `None` when
/// it does not authenticate: changed, cut short, sealed under another name,
/// with another key, or in an older format.
pub fn open(file_key: &[u8; 32], name: &str, bytes: &[u8]) -> Option<Vec<u8>> {
    if bytes.len() < HEADER_LEN + TAG_LEN || !is_sealed(bytes) || bytes[4] != VERSION {
        return None;
    }
    let (header, body) = bytes.split_at(HEADER_LEN);
    let nonce: [u8; 24] = header[8..32].try_into().ok()?;
    let cipher = XChaCha20Poly1305::new(&(*file_key).into());
    cipher
        .decrypt(
            &XNonce::from(nonce),
            Payload {
                msg: body,
                aad: &aad(header, name),
            },
        )
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: [u8; 32] = [9; 32];

    #[test]
    fn round_trips_and_hides_the_bytes() {
        let sealed = seal(&KEY, "a.json", b"{\"best\":12}").unwrap();
        assert!(is_sealed(&sealed));
        assert!(!sealed.windows(4).any(|w| w == b"best"));
        assert_eq!(
            open(&KEY, "a.json", &sealed),
            Some(b"{\"best\":12}".to_vec())
        );
        assert_ne!(
            seal(&KEY, "a.json", b"x").unwrap(),
            seal(&KEY, "a.json", b"x").unwrap()
        );
    }

    #[test]
    fn any_changed_byte_fails() {
        let sealed = seal(&KEY, "a.json", b"hello there").unwrap();
        for at in 0..sealed.len() {
            for flip in [0x01, 0x80] {
                let mut bad = sealed.clone();
                bad[at] ^= flip;
                assert_eq!(open(&KEY, "a.json", &bad), None, "byte {at}");
            }
        }
        assert_eq!(open(&KEY, "a.json", &sealed[..sealed.len() - 1]), None);
        let mut longer = sealed.clone();
        longer.push(0);
        assert_eq!(open(&KEY, "a.json", &longer), None);
        assert_eq!(open(&KEY, "b.json", &sealed), None);
        assert_eq!(open(&[8; 32], "a.json", &sealed), None);
    }

    #[test]
    fn a_version_1_file_does_not_open() {
        let mut old = seal(&KEY, "a.json", b"x").unwrap();
        old[4] = 1;
        assert_eq!(open(&KEY, "a.json", &old), None);
    }
}
