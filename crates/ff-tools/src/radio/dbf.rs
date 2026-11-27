//! Just enough of dBase III to read ISED's broadcasting tables.
//!
//! ISED publishes the Canadian licence record as `.dbf` files: a 32-byte
//! header, one 32-byte descriptor per field up to a `0x0D` terminator, then
//! fixed-width records whose first byte is `*` when the record is deleted.
//! Every field is text on disk, numbers included, so a row comes back as
//! field name to trimmed string. The language byte in these exports is 0 and
//! the accented city names ("Québec", "Trois-Rivières") are Latin-1.

use std::collections::HashMap;
use std::path::Path;

use crate::util::Result;

/// One record: field name to its trimmed text.
pub type Row = HashMap<String, String>;

struct Field {
    name: String,
    width: usize,
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

fn u16_at(data: &[u8], at: usize) -> usize {
    data.get(at..at + 2)
        .map_or(0, |b| usize::from(u16::from_le_bytes([b[0], b[1]])))
}

fn u32_at(data: &[u8], at: usize) -> usize {
    data.get(at..at + 4)
        .map_or(0, |b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
}

/// Every live record in a dBase III table held in memory.
pub fn parse(data: &[u8]) -> Result<Vec<Row>> {
    if data.len() < 32 {
        return Err("not a dBase table: shorter than its header".into());
    }
    let records = u32_at(data, 4);
    let header_len = u16_at(data, 8);
    let record_len = u16_at(data, 10);
    let mut fields = Vec::new();
    let mut at = 32;
    while data.get(at).is_some_and(|&b| b != 0x0D) {
        let Some(descriptor) = data.get(at..at + 32) else {
            return Err("not a dBase table: field list runs past the end".into());
        };
        let name_end = descriptor[..11].iter().position(|&b| b == 0).unwrap_or(11);
        fields.push(Field {
            name: latin1(&descriptor[..name_end]),
            width: usize::from(descriptor[16]),
        });
        at += 32;
    }
    if fields.is_empty() || record_len == 0 {
        return Err("not a dBase table: no fields".into());
    }
    let mut rows = Vec::with_capacity(records);
    for k in 0..records {
        let start = header_len + k * record_len;
        let Some(record) = data.get(start..start + record_len) else {
            break;
        };
        if record[0] == b'*' {
            continue;
        }
        let mut row = Row::with_capacity(fields.len());
        let mut pos = 1;
        for field in &fields {
            let end = (pos + field.width).min(record.len());
            row.insert(
                field.name.clone(),
                latin1(&record[pos..end]).trim().to_string(),
            );
            pos = end;
        }
        rows.push(row);
    }
    Ok(rows)
}

/// [`parse`] a table on disk, naming the file in any error.
pub fn read(path: &Path) -> Result<Vec<Row>> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse(&data).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A two-field table with one live record and one deleted one.
    fn table() -> Vec<u8> {
        let mut data = vec![0u8; 32];
        data[0] = 3;
        data[4..8].copy_from_slice(&2u32.to_le_bytes());
        let header_len: u16 = 32 + 2 * 32 + 1;
        data[8..10].copy_from_slice(&header_len.to_le_bytes());
        data[10..12].copy_from_slice(&(1u16 + 10 + 4).to_le_bytes());
        for (name, kind, width) in [("CITY", b'C', 10u8), ("FREQ", b'N', 4)] {
            let mut d = [0u8; 32];
            d[..name.len()].copy_from_slice(name.as_bytes());
            d[11] = kind;
            d[16] = width;
            data.extend_from_slice(&d);
        }
        data.push(0x0D);
        data.extend_from_slice(b" Qu\xe9bec    91.9");
        data.extend_from_slice(b"*Gone      88.1");
        data
    }

    #[test]
    fn a_table_reads_its_live_records_in_latin_1() {
        let rows = parse(&table()).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["CITY"], "Québec");
        assert_eq!(rows[0]["FREQ"], "91.9");
        assert!(parse(b"short").is_err());
    }
}
