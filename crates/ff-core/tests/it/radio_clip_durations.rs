//! Every host break, station ID and ad in the shipped content tables runs as
//! long as its clip in `music.pak`.
//!
//! The playback loop times each break item from the table
//! (`content_duration_s`), not from the file, so a table number longer than
//! the clip is dead air before the next song and a shorter one cuts the clip
//! off. The pack is builder-local (`cargo tools build-release` downloads it),
//! so the test skips, saying so, when it is absent.

use std::path::PathBuf;

use ff_core::assets_pack::{pack_available, StreamedPack, DEFAULT_MUSIC_PACK_NAME};
use ff_core::music::STATION_HOST_SEGMENTS;
use ff_core::radio_content::ContentTables;

/// How far a table number may sit from the clip, in seconds.
const TOLERANCE_S: f64 = 0.25;

fn music_pack() -> Option<StreamedPack> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets")
        .join(DEFAULT_MUSIC_PACK_NAME);
    if !pack_available(&path) {
        eprintln!(
            "SKIPPING {}: not present (music.pak is builder-local and cargo tools \
             build-release downloads it)",
            path.display()
        );
        return None;
    }
    Some(StreamedPack::open(&path).expect("music.pak opens"))
}

fn read_u16(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}

fn read_u32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}

fn read_i64(b: &[u8], at: usize) -> Option<i64> {
    Some(i64::from_le_bytes(b.get(at..at + 8)?.try_into().ok()?))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Length of an Ogg Opus or Ogg Vorbis file in seconds, without decoding:
/// the last page's granule position, less Opus's pre-skip, over the sample
/// rate (always 48 kHz for Opus; Vorbis states its own).
fn ogg_length_s(bytes: &[u8]) -> Option<f64> {
    let last_page = bytes.windows(4).rposition(|w| w == b"OggS")?;
    let granule = read_i64(bytes, last_page + 6)?;
    if let Some(head) = find(bytes, b"OpusHead") {
        let pre_skip = read_u16(bytes, head + 10)?;
        return Some((granule - i64::from(pre_skip)) as f64 / 48_000.0);
    }
    let vorbis = find(bytes, b"\x01vorbis")?;
    let rate = read_u32(bytes, vorbis + 12)?;
    (rate > 0).then(|| granule as f64 / f64::from(rate))
}

#[test]
fn test_every_break_item_lasts_as_long_as_its_clip() {
    let Some(pack) = music_pack() else {
        return;
    };
    let names = pack.names();
    let tables = ContentTables::shipped();
    let mut rows: Vec<(&str, f64)> = Vec::new();
    for (_, pool) in STATION_HOST_SEGMENTS.iter() {
        rows.extend(pool.iter().map(|t| (t.key.as_str(), t.duration_s)));
    }
    for (_, pool) in &tables.station_ids {
        rows.extend(pool.iter().map(|t| (t.key.as_str(), t.duration_s)));
    }
    rows.extend(
        tables
            .ad_spots
            .iter()
            .map(|t| (t.key.as_str(), t.duration_s)),
    );
    assert!(rows.len() > 100, "the tables are not empty: {}", rows.len());

    let mut wrong = Vec::new();
    for (key, table_s) in rows {
        let prefix = format!("music/{key}.");
        let Some(name) = names.iter().find(|n| n.starts_with(&prefix)) else {
            wrong.push(format!("{key}: no clip in music.pak"));
            continue;
        };
        let bytes = pack.read(name).expect("a listed entry reads");
        let Some(file_s) = ogg_length_s(&bytes) else {
            wrong.push(format!("{name}: not an Ogg Opus or Vorbis file"));
            continue;
        };
        if (file_s - table_s).abs() > TOLERANCE_S {
            wrong.push(format!(
                "{key}: the table says {table_s:.1} s, the clip is {file_s:.2} s"
            ));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}
