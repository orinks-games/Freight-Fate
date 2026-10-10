//! The baked binary data container (`world.ffdata`).
//!
//! # Why
//!
//! The shipped data tree is ~136 MB of JSON, 94 MB of it the fifty state leg
//! shards, and `World::load()` reads and parses every byte of those shards at
//! startup just to learn each leg's endpoints and mileage. The heavy per-mile
//! corridor is already deferred (see `world_models::Leg`), but the *parse* is
//! not: serde_json still walks all 94 MB to hand the lazy leg its raw
//! `Value`. That is the half-second before the menu, and the several hundred
//! MB of retained `Value` trees behind it.
//!
//! A baked container fixes both. The file is memory-mapped, so opening it
//! touches nothing; the eager half (the city table and the per-leg endpoint
//! fields) is one small compressed section; every heavy blob -- each leg's
//! corridor, the five nationwide side maps, the curve table -- is its own
//! zstd frame reached through an offset, decoded on first touch exactly where
//! the JSON path decoded it on first touch.
//!
//! # Format
//!
//! ```text
//! offset  size  meaning
//! 0       8     magic b"FFDATA\0\0"
//! 8       4     format version, u32 LE (refused, never half-read, on mismatch)
//! 12      4     flags, u32 LE (reserved, 0)
//! 16      8     directory offset, u64 LE
//! 24      8     directory length, u64 LE
//! 32      ..    section payloads, in directory order
//! ..      ..    directory: bincode Vec<Section>{name, offset, stored, raw, codec}
//! ```
//!
//! Sections are named ([`SECTION_CITIES`] and friends). A section is either
//! stored raw (`codec` 0) or as one zstd frame (`codec` 1) whose decompressed
//! length is `raw`. The corridor section is the exception: it is a *region*
//! of many independent zstd frames, one per leg, and each `BakedLeg` carries
//! the offset and length of its own frame within it, so driving one leg
//! decompresses one leg.
//!
//! Everything else the game reads out of `data/` -- `street_limits.json`,
//! `buffs.json`, the two radio catalogs -- is stored as its compressed JSON
//! text under a `text:<relative path>` section and parsed by the same parser
//! the JSON tree uses. Those files are small, they hold free-form JSON the
//! model types keep as `Value`, and reusing the parser is the strongest
//! guarantee that the two paths cannot disagree.
//!
//! # Determinism
//!
//! Same input tree, byte-identical output: sections are written in a fixed
//! order, maps are stored as ordered key/value lists (the loaders' own file
//! order, or sorted where the source is a hash map), and nothing records a
//! time or a path. `ff-bake --check` re-bakes to a temp file and compares.
//!
//! # Re-baking
//!
//! ```text
//! cargo run -p ff-core --bin ff-bake -- \
//!     --data-dir data --out <dir>/world.ffdata
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexMap;
use memmap2::Mmap;
use serde::{Deserialize, Serialize};

use super::curves::CurveRecord;
use super::world_local_data::{CityServiceData, CityServiceEntry};
use super::world_models::{
    City, CorridorDetail, DataError, FacilityApproach, FacilityEndpoint, LocalApproach,
    LocalGeometry,
};

mod bake;
pub mod records;

pub use bake::{bake, bake_bytes, bake_is_deterministic, BakeReport, SectionSize};
use records::{
    BakedCity, BakedCityServiceEntry, BakedCorridor, BakedCurveRecord, BakedFacilityApproach,
    BakedFacilityEndpoint, BakedLeg, BakedLocalApproach, BakedLocalGeometry,
};

/// The file name the runtime looks for beside the other data files.
pub const BAKED_FILE_NAME: &str = "world.ffdata";

/// First eight bytes of the container.
pub const MAGIC: &[u8; 8] = b"FFDATA\0\0";

/// Bumped whenever a section's encoding changes. A file written by another
/// version is refused with a message naming both, never half-read.
///
/// 2: a stop carries the interchange that serves it (`exit_ref`,
/// `interchange_mi`).
///
/// 3: a local segment carries the measured turn angle at the junction onto it
/// (`local_turn_deg`), which `data::corners` prices the corner from.
///
/// 4: an interchange carries its OSM-derived ramp length per direction
/// (`ramp_length_ft_forward/backward`, `ramp_length_source`).
///
/// 5: an interchange carries the OSM node its ramp ends at per direction
/// (`ramp_terminal_node_*`); a facility street carries its posted limit and
/// kind and its controls, and a facility approach its driveway and one
/// street chain per ramp terminal (`exit_chains`).
///
/// 6: a road stop carries the street chains from its exit's ramp terminals
/// (`approach_chains`).
///
/// 7: a street limit carries the statute it follows (`basis`: town or rural).
///
/// 8: a landmark carries the directions it is heard in (`directions`).
pub const FORMAT_VERSION: u32 = 8;

const HEADER_LEN: usize = 32;

/// Codec 0: the bytes are the payload.
const CODEC_RAW: u8 = 0;
/// Codec 1: one zstd frame, `raw` bytes when decompressed.
const CODEC_ZSTD: u8 = 1;
/// Codec 2: a region of independently compressed frames addressed by their
/// own offsets (the per-leg corridors); the section is never decoded whole.
const CODEC_REGION: u8 = 2;

pub const SECTION_CITIES: &str = "cities";
pub const SECTION_LEGS: &str = "legs";
pub const SECTION_CORRIDORS: &str = "corridors";
pub const SECTION_CURVES: &str = "curves";
pub const SECTION_CITY_SERVICES: &str = "city_services";
pub const SECTION_FACILITY_APPROACHES: &str = "facility_approaches";
pub const SECTION_FACILITY_ENDPOINTS: &str = "facility_endpoints";
pub const SECTION_LOCAL_APPROACHES: &str = "local_approaches";
pub const SECTION_LOCAL_GEOMETRY: &str = "local_geometry";

/// Loose data files stored as their compressed JSON text, read back through
/// [`BakedData::text`] by the same loaders that read them off disk.
pub const TEXT_FILES: &[&str] = &[
    "street_limits.json",
    "buffs.json",
    "channel3000.json",
    "radio_catalog.json",
    "radio_imported.json",
    "radio_community.json",
];

/// The `text:` section name for a relative data path.
pub fn text_section_name(relative: &str) -> String {
    format!("text:{}", relative.replace('\\', "/"))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Section {
    name: String,
    offset: u64,
    stored: u64,
    raw: u64,
    codec: u8,
}

fn bincode_config() -> bincode::config::Configuration {
    bincode::config::standard()
}

fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, DataError> {
    bincode::serde::encode_to_vec(value, bincode_config())
        .map_err(|e| DataError::io(format!("baked encode: {e}")))
}

fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8], what: &str) -> Result<T, DataError> {
    bincode::serde::decode_from_slice(bytes, bincode_config())
        .map(|(value, _)| value)
        .map_err(|e| DataError::io(format!("{BAKED_FILE_NAME} section {what}: {e}")))
}

/// Ordered key/value pairs; an `IndexMap` on the wire would cost a map
/// framing per entry and lose the file order the loaders depend on.
type Pairs<T> = Vec<(String, T)>;

fn to_index_map<M, T: From<M>>(pairs: Pairs<M>) -> IndexMap<String, T> {
    pairs
        .into_iter()
        .map(|(key, value)| (key, T::from(value)))
        .collect()
}

/// One opened container: the mapping and its directory. Nothing is decoded
/// until asked for, and the eager sections are decoded straight into the
/// world that consumes them -- caching them here would only buy a second
/// copy of every string.
pub struct BakedData {
    path: PathBuf,
    mmap: Mmap,
    sections: HashMap<String, Section>,
}

/// Where one leg's corridor frame sits. A lazy leg's builder captures this
/// by value, so nothing has to keep the eager leg table alive.
#[derive(Debug, Clone, Copy, Default)]
pub struct CorridorRef {
    pub offset: u64,
    pub stored_len: u32,
    pub raw_len: u32,
}

impl From<&BakedLeg> for CorridorRef {
    fn from(leg: &BakedLeg) -> Self {
        CorridorRef {
            offset: leg.corridor_offset,
            stored_len: leg.corridor_stored_len,
            raw_len: leg.corridor_raw_len,
        }
    }
}

impl std::fmt::Debug for BakedData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BakedData({}, {} sections)",
            self.path.display(),
            self.sections.len()
        )
    }
}

impl BakedData {
    /// Memory-map a container and read its directory. Nothing else is
    /// touched: a container opened only to answer a `text:` lookup never
    /// decodes a leg.
    pub fn open(path: &Path) -> Result<Arc<BakedData>, DataError> {
        let file = std::fs::File::open(path)
            .map_err(|e| DataError::io(format!("{}: {e}", path.display())))?;
        // SAFETY: the shipped data file is read-only for the life of the
        // process. A player editing it under a running game gets the same
        // answer any mmap'd asset gives, and the header check below runs
        // before a single byte is trusted.
        let mmap = unsafe { Mmap::map(&file) }
            .map_err(|e| DataError::io(format!("{}: {e}", path.display())))?;
        if mmap.len() < HEADER_LEN {
            return Err(DataError::io(format!(
                "{} is truncated: {} bytes, header needs {HEADER_LEN}",
                path.display(),
                mmap.len()
            )));
        }
        if &mmap[..8] != MAGIC {
            return Err(DataError::io(format!(
                "{} is not a Freight Fate baked data file",
                path.display()
            )));
        }
        let version = u32::from_le_bytes(mmap[8..12].try_into().expect("4 bytes"));
        if version != FORMAT_VERSION {
            return Err(DataError::io(format!(
                "{} is baked data format {version}, this build reads format \
                 {FORMAT_VERSION}. Re-bake it: cargo run -p ff-core --bin \
                 ff-bake -- --data-dir data --out {}",
                path.display(),
                path.display()
            )));
        }
        let dir_offset = u64::from_le_bytes(mmap[16..24].try_into().expect("8 bytes")) as usize;
        let dir_len = u64::from_le_bytes(mmap[24..32].try_into().expect("8 bytes")) as usize;
        let end = dir_offset.checked_add(dir_len).ok_or_else(|| {
            DataError::io(format!("{}: directory offset overflows", path.display()))
        })?;
        if end > mmap.len() {
            return Err(DataError::io(format!(
                "{} is truncated: directory ends at {end}, file is {} bytes",
                path.display(),
                mmap.len()
            )));
        }
        let entries: Vec<Section> = decode(&mmap[dir_offset..end], "directory")?;
        let mut sections = HashMap::with_capacity(entries.len());
        for entry in entries {
            let stop = (entry.offset as usize).saturating_add(entry.stored as usize);
            if stop > mmap.len() {
                return Err(DataError::io(format!(
                    "{}: section {:?} runs past the end of the file",
                    path.display(),
                    entry.name
                )));
            }
            sections.insert(entry.name.clone(), entry);
        }
        Ok(Arc::new(BakedData {
            path: path.to_path_buf(),
            mmap,
            sections,
        }))
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Section names, sorted -- for `--check` output and tests.
    pub fn section_names(&self) -> Vec<&str> {
        let mut names: Vec<&str> = self.sections.keys().map(String::as_str).collect();
        names.sort_unstable();
        names
    }

    /// The stored (on-disk) byte length of a section.
    pub fn section_stored_len(&self, name: &str) -> Option<u64> {
        self.sections.get(name).map(|s| s.stored)
    }

    fn raw_slice(&self, offset: u64, len: u64) -> &[u8] {
        let start = offset as usize;
        &self.mmap[start..start + len as usize]
    }

    /// The decompressed bytes of a section, or `None` when it is absent.
    fn section_bytes(&self, name: &str) -> Result<Option<Vec<u8>>, DataError> {
        let Some(section) = self.sections.get(name) else {
            return Ok(None);
        };
        let stored = self.raw_slice(section.offset, section.stored);
        match section.codec {
            CODEC_RAW => Ok(Some(stored.to_vec())),
            CODEC_ZSTD => Ok(Some(inflate(stored, section.raw as usize, name)?)),
            CODEC_REGION => Err(DataError::io(format!(
                "{BAKED_FILE_NAME} section {name} is a frame region, not one blob"
            ))),
            other => Err(DataError::io(format!(
                "{BAKED_FILE_NAME} section {name} uses unknown codec {other}"
            ))),
        }
    }

    fn section_value<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
    ) -> Result<Option<T>, DataError> {
        match self.section_bytes(name)? {
            None => Ok(None),
            Some(bytes) => Ok(Some(decode(&bytes, name)?)),
        }
    }

    /// The JSON text of a loose data file stored in the container.
    pub fn text(&self, relative: &str) -> Option<String> {
        let name = text_section_name(relative);
        let bytes = self.section_bytes(&name).ok()??;
        String::from_utf8(bytes).ok()
    }

    /// The city table, decoded and handed over. Called once, by the world
    /// being built.
    pub fn take_cities(&self) -> Result<Vec<BakedCity>, DataError> {
        let cities: Pairs<BakedCity> = self
            .section_value(SECTION_CITIES)?
            .ok_or_else(|| DataError::io("baked data has no city table"))?;
        Ok(cities.into_iter().map(|(_, city)| city).collect())
    }

    /// Every leg's eager fields and corridor offset, decoded and handed over.
    pub fn take_legs(&self) -> Result<Vec<BakedLeg>, DataError> {
        self.section_value(SECTION_LEGS)?
            .ok_or_else(|| DataError::io("baked data has no leg table"))
    }

    /// One leg's corridor detail, decompressed and decoded on its own.
    pub fn leg_corridor(&self, leg: &BakedLeg) -> Result<CorridorDetail, DataError> {
        self.corridor_at(CorridorRef::from(leg))
    }

    /// The corridor at one offset in the frame region.
    pub fn corridor_at(&self, at: CorridorRef) -> Result<CorridorDetail, DataError> {
        if at.stored_len == 0 {
            return Ok(CorridorDetail::default());
        }
        let Some(section) = self.sections.get(SECTION_CORRIDORS) else {
            return Err(DataError::io("baked data has no corridor section"));
        };
        let start = section.offset + at.offset;
        let stored = self.raw_slice(start, at.stored_len as u64);
        let bytes = inflate(stored, at.raw_len as usize, "corridor")?;
        let baked: BakedCorridor = decode(&bytes, "corridor")?;
        Ok(baked.into())
    }

    /// The screened per-leg curve table, exactly what `curves::load` builds
    /// from `curves.jsonl` and the artifact list.
    pub fn curves(&self) -> Result<HashMap<String, Vec<CurveRecord>>, DataError> {
        let pairs: Option<Pairs<Vec<BakedCurveRecord>>> = self.section_value(SECTION_CURVES)?;
        Ok(pairs
            .unwrap_or_default()
            .into_iter()
            .map(|(key, rows)| (key, rows.into_iter().map(CurveRecord::from).collect()))
            .collect())
    }

    pub fn city_service_data(&self) -> Result<CityServiceData, DataError> {
        let pairs: Option<Pairs<Pairs<BakedCityServiceEntry>>> =
            self.section_value(SECTION_CITY_SERVICES)?;
        Ok(pairs
            .unwrap_or_default()
            .into_iter()
            .map(|(city, services)| {
                (
                    city,
                    services
                        .into_iter()
                        .map(|(key, entry)| (key, CityServiceEntry::from(entry)))
                        .collect::<IndexMap<String, CityServiceEntry>>(),
                )
            })
            .collect())
    }

    pub fn facility_approaches(&self) -> Result<IndexMap<String, FacilityApproach>, DataError> {
        let pairs: Option<Pairs<BakedFacilityApproach>> =
            self.section_value(SECTION_FACILITY_APPROACHES)?;
        Ok(to_index_map(pairs.unwrap_or_default()))
    }

    pub fn facility_endpoints(&self) -> Result<IndexMap<String, FacilityEndpoint>, DataError> {
        let pairs: Option<Pairs<BakedFacilityEndpoint>> =
            self.section_value(SECTION_FACILITY_ENDPOINTS)?;
        Ok(to_index_map(pairs.unwrap_or_default()))
    }

    pub fn local_approaches(&self) -> Result<IndexMap<String, LocalApproach>, DataError> {
        let pairs: Option<Pairs<BakedLocalApproach>> =
            self.section_value(SECTION_LOCAL_APPROACHES)?;
        Ok(to_index_map(pairs.unwrap_or_default()))
    }

    pub fn local_geometries(&self) -> Result<IndexMap<String, LocalGeometry>, DataError> {
        let pairs: Option<Pairs<BakedLocalGeometry>> =
            self.section_value(SECTION_LOCAL_GEOMETRY)?;
        Ok(to_index_map(pairs.unwrap_or_default()))
    }

    /// The city table as the world model wants it, in file order.
    pub fn cities(&self) -> Result<IndexMap<String, City>, DataError> {
        Ok(self
            .take_cities()?
            .into_iter()
            .map(|city| {
                let key = city.key.clone();
                (key, City::from(city))
            })
            .collect())
    }
}

fn inflate(stored: &[u8], raw_len: usize, what: &str) -> Result<Vec<u8>, DataError> {
    zstd::bulk::decompress(stored, raw_len)
        .map_err(|e| DataError::io(format!("{BAKED_FILE_NAME} section {what}: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_that_is_not_a_container_is_refused_by_name() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(BAKED_FILE_NAME);
        std::fs::write(&path, vec![b'x'; 4096]).expect("write");
        let err = BakedData::open(&path).expect_err("refused");
        assert!(
            err.to_string()
                .contains("not a Freight Fate baked data file"),
            "{err}"
        );
    }

    #[test]
    fn a_wrong_format_version_names_both_versions_and_the_rebake() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(BAKED_FILE_NAME);
        let mut bytes = vec![0u8; HEADER_LEN];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..12].copy_from_slice(&(FORMAT_VERSION + 41).to_le_bytes());
        bytes[16..24].copy_from_slice(&(HEADER_LEN as u64).to_le_bytes());
        std::fs::write(&path, &bytes).expect("write");
        let err = BakedData::open(&path).expect_err("refused");
        let text = err.to_string();
        // Whatever the build reads today, plus the 41 written above: this
        // said "format 42" while the format was 1, and broke when it moved.
        let written = format!("format {}", FORMAT_VERSION + 41);
        assert!(text.contains(&written), "{text}");
        assert!(
            text.contains(&format!("reads format {FORMAT_VERSION}")),
            "{text}"
        );
        assert!(text.contains("ff-bake"), "{text}");
    }
}
