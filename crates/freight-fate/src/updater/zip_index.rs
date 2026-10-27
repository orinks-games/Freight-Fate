//! Just enough of the zip format to update without the sound packs: read a
//! release archive's index (its central directory) from the end of the
//! file, work out which byte ranges hold everything but the skipped
//! entries, and write a new index for the archive those ranges make.
//!
//! Each kept entry is copied byte for byte (local header, data and any data
//! descriptor), so its CRC-32 still checks when the archive is unpacked.
//! Zip64 archives are refused: a release is far below 4 GB, and the caller
//! falls back to the full download.

const EOCD_SIG: u32 = 0x0605_4b50;
const ZIP64_LOCATOR_SIG: u32 = 0x0706_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
pub const LOCAL_SIG: [u8; 4] = [0x50, 0x4b, 0x03, 0x04];
const EOCD_LEN: usize = 22;
const CENTRAL_LEN: usize = 46;
/// The end record, plus the longest comment it may carry.
pub const TAIL_LEN: u64 = (EOCD_LEN + u16::MAX as usize) as u64;

/// One file in the archive's index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub crc32: u32,
    pub size: u64,
    /// Where the entry's local header starts.
    pub offset: u64,
    /// The whole central directory record, rewritten with a new offset.
    record: Vec<u8>,
}

/// Where the index sits: `(offset, length, entry count)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct End {
    pub directory_offset: u64,
    pub directory_len: u64,
    pub entries: u16,
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

/// Find the end record in the last bytes of the archive.
pub fn find_end(tail: &[u8]) -> Option<End> {
    let last = tail.len().checked_sub(EOCD_LEN)?;
    let at = (0..=last).rev().find(|&i| {
        u32_at(tail, i) == Some(EOCD_SIG)
            && u16_at(tail, i + 20)
                .is_some_and(|comment| i + EOCD_LEN + comment as usize == tail.len())
    })?;
    // A zip64 locator sits just before a zip64 end record's end record.
    if at >= 20 && u32_at(tail, at - 20) == Some(ZIP64_LOCATOR_SIG) {
        return None;
    }
    let entries = u16_at(tail, at + 10)?;
    let directory_len = u32_at(tail, at + 12)?;
    let directory_offset = u32_at(tail, at + 16)?;
    if entries == u16::MAX || directory_len == u32::MAX || directory_offset == u32::MAX {
        return None;
    }
    Some(End {
        directory_offset: u64::from(directory_offset),
        directory_len: u64::from(directory_len),
        entries,
    })
}

/// Read every record of the central directory.
pub fn parse_directory(directory: &[u8], end: End) -> Option<Vec<Entry>> {
    let mut entries = Vec::with_capacity(usize::from(end.entries));
    let mut at = 0;
    while at < directory.len() {
        if u32_at(directory, at)? != CENTRAL_SIG {
            return None;
        }
        let crc32 = u32_at(directory, at + 16)?;
        let compressed = u32_at(directory, at + 20)?;
        let size = u32_at(directory, at + 24)?;
        let name_len = usize::from(u16_at(directory, at + 28)?);
        let extra_len = usize::from(u16_at(directory, at + 30)?);
        let comment_len = usize::from(u16_at(directory, at + 32)?);
        let offset = u32_at(directory, at + 42)?;
        if compressed == u32::MAX || size == u32::MAX || offset == u32::MAX {
            return None;
        }
        let len = CENTRAL_LEN + name_len + extra_len + comment_len;
        let record = directory.get(at..at + len)?.to_vec();
        let name =
            String::from_utf8_lossy(&record[CENTRAL_LEN..CENTRAL_LEN + name_len]).into_owned();
        entries.push(Entry {
            name,
            crc32,
            size: u64::from(size),
            offset: u64::from(offset),
            record,
        });
        at += len;
    }
    (entries.len() == usize::from(end.entries)).then_some(entries)
}

/// What to fetch for an archive without the entries at `skip`, and the
/// index to append once those bytes are on disk.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    /// Half-open byte ranges of the original archive, in order.
    pub ranges: Vec<(u64, u64)>,
    /// The new central directory and end record.
    pub directory: Vec<u8>,
    /// Where each kept entry's local header lands in the new archive.
    pub local_headers: Vec<u64>,
}

/// Plan the archive that leaves out `entries[i]` for each `i` in `skip`.
/// An entry's bytes run from its local header to the next entry's (or to
/// the index), which also covers any data descriptor after its data.
pub fn plan(entries: &[Entry], end: End, skip: &[usize]) -> Option<Plan> {
    let mut order: Vec<usize> = (0..entries.len()).collect();
    order.sort_by_key(|&i| entries[i].offset);
    let mut span = vec![(0u64, 0u64); entries.len()];
    for (pos, &i) in order.iter().enumerate() {
        let next = order
            .get(pos + 1)
            .map_or(end.directory_offset, |&j| entries[j].offset);
        if next < entries[i].offset {
            return None;
        }
        span[i] = (entries[i].offset, next);
    }
    // Everything before the index, minus the skipped spans.
    let mut cuts: Vec<(u64, u64)> = skip.iter().map(|&i| span[i]).collect();
    cuts.sort_unstable();
    let mut ranges = Vec::new();
    let mut cursor = 0;
    for &(start, stop) in &cuts {
        if start > cursor {
            ranges.push((cursor, start));
        }
        cursor = cursor.max(stop);
    }
    if end.directory_offset > cursor {
        ranges.push((cursor, end.directory_offset));
    }
    let removed_before = |offset: u64| -> u64 {
        cuts.iter()
            .filter(|(start, _)| *start < offset)
            .map(|(start, stop)| stop - start)
            .sum()
    };
    let mut directory = Vec::new();
    let mut local_headers = Vec::new();
    let mut kept: u16 = 0;
    for (i, entry) in entries.iter().enumerate() {
        if skip.contains(&i) {
            continue;
        }
        let moved = entry.offset - removed_before(entry.offset);
        let mut record = entry.record.clone();
        record[42..46].copy_from_slice(&u32::try_from(moved).ok()?.to_le_bytes());
        directory.extend_from_slice(&record);
        local_headers.push(moved);
        kept += 1;
    }
    let directory_offset: u64 = ranges.iter().map(|(start, stop)| stop - start).sum();
    let directory_len = directory.len();
    directory.extend_from_slice(&EOCD_SIG.to_le_bytes());
    directory.extend_from_slice(&[0, 0, 0, 0]); // disk numbers
    directory.extend_from_slice(&kept.to_le_bytes());
    directory.extend_from_slice(&kept.to_le_bytes());
    directory.extend_from_slice(&u32::try_from(directory_len).ok()?.to_le_bytes());
    directory.extend_from_slice(&u32::try_from(directory_offset).ok()?.to_le_bytes());
    directory.extend_from_slice(&[0, 0]); // no comment
    Some(Plan {
        ranges,
        directory,
        local_headers,
    })
}
