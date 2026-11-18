//! Whether a native library loads on a phone with 16 KB memory pages.
//!
//! `zipalign -P 16` places each library on a 16 KB boundary inside the APK,
//! but the library's own loadable segments must be aligned to 16 KB as well,
//! which only the linker decides. NDK r28 and newer link that way by
//! default; this reads the result rather than trusting the NDK.

use crate::util::Result;

/// The page size of the phones that need it, Android 15 and newer.
pub(super) const PAGE: u64 = 16 * 1024;

/// `PT_LOAD`: a segment the loader maps.
const PT_LOAD: u32 = 1;

/// The smallest alignment among a 64-bit little-endian ELF file's loadable
/// segments, which is the page size it can load under.
pub(super) fn load_alignment(bytes: &[u8]) -> Result<u64> {
    if bytes.get(..4) != Some(b"\x7fELF".as_slice()) {
        return Err("not an ELF file".into());
    }
    // EI_CLASS 2 is 64-bit and EI_DATA 1 little-endian: every ABI built.
    if bytes.get(4) != Some(&2) || bytes.get(5) != Some(&1) {
        return Err("not a 64-bit little-endian ELF file".into());
    }
    let phoff = read_u64(bytes, 0x20)?;
    let entsize = u64::from(read_u16(bytes, 0x36)?);
    let count = read_u16(bytes, 0x38)?;
    let mut smallest = None;
    for index in 0..u64::from(count) {
        let header = phoff
            .checked_add(index * entsize)
            .and_then(|at| usize::try_from(at).ok())
            .ok_or("program header out of range")?;
        if read_u32(bytes, header)? != PT_LOAD {
            continue;
        }
        let align = read_u64(bytes, header + 0x30)?;
        smallest = Some(smallest.map_or(align, |s: u64| s.min(align)));
    }
    smallest.ok_or_else(|| "no loadable segments".into())
}

/// Refuse a library that would not load under 16 KB pages.
pub(super) fn check_page_alignment(name: &str, bytes: &[u8]) -> Result<()> {
    let align = load_alignment(bytes).map_err(|e| format!("{name}: {e}"))?;
    if align < PAGE {
        return Err(format!(
            "{name}'s segments are aligned to {align} bytes, not 16 KB, so it would \
             not load on a phone with 16 KB pages; build with NDK r28 or newer"
        ));
    }
    Ok(())
}

fn field<const N: usize>(bytes: &[u8], at: usize) -> Result<[u8; N]> {
    bytes
        .get(at..at + N)
        .and_then(|slice| slice.try_into().ok())
        .ok_or_else(|| "the ELF file is cut short".into())
}

fn read_u16(bytes: &[u8], at: usize) -> Result<u16> {
    field(bytes, at).map(u16::from_le_bytes)
}

fn read_u32(bytes: &[u8], at: usize) -> Result<u32> {
    field(bytes, at).map(u32::from_le_bytes)
}

fn read_u64(bytes: &[u8], at: usize) -> Result<u64> {
    field(bytes, at).map(u64::from_le_bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An ELF64 header and one program header per `(type, align)`.
    fn elf(segments: &[(u32, u64)]) -> Vec<u8> {
        let mut bytes = vec![0u8; 64];
        bytes[..4].copy_from_slice(b"\x7fELF");
        bytes[4] = 2;
        bytes[5] = 1;
        bytes[0x20..0x28].copy_from_slice(&64u64.to_le_bytes());
        bytes[0x36..0x38].copy_from_slice(&56u16.to_le_bytes());
        bytes[0x38..0x3a].copy_from_slice(&(segments.len() as u16).to_le_bytes());
        for &(kind, align) in segments {
            let mut header = vec![0u8; 56];
            header[..4].copy_from_slice(&kind.to_le_bytes());
            header[0x30..0x38].copy_from_slice(&align.to_le_bytes());
            bytes.extend(header);
        }
        bytes
    }

    #[test]
    fn the_smallest_loadable_segment_decides() {
        let lib = elf(&[(PT_LOAD, PAGE), (6, 8), (PT_LOAD, 4096)]);
        assert_eq!(load_alignment(&lib).unwrap(), 4096);
        assert!(check_page_alignment("libold.so", &lib)
            .unwrap_err()
            .contains("libold.so's segments are aligned to 4096 bytes"));
    }

    #[test]
    fn a_library_aligned_to_16_kb_passes() {
        let lib = elf(&[(PT_LOAD, PAGE), (2, 8), (PT_LOAD, 64 * 1024)]);
        assert_eq!(load_alignment(&lib).unwrap(), PAGE);
        assert!(check_page_alignment("libmain.so", &lib).is_ok());
    }

    #[test]
    fn a_file_that_is_not_a_library_is_refused() {
        assert!(load_alignment(b"PK\x03\x04").is_err());
        assert!(load_alignment(&elf(&[])).is_err());
        let mut cut = elf(&[(PT_LOAD, PAGE)]);
        cut.truncate(80);
        assert!(load_alignment(&cut).is_err());
    }
}
