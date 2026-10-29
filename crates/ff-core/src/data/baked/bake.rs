//! Building `world.ffdata` from the JSON tree.
//!
//! The baker runs the *shipped loaders* -- `World::load_from_json`,
//! `curves::build_from_sources`, `world_local_data::load_*` -- and writes
//! what they produced. Nothing here reads a JSON file with its own parser, so
//! the binary and the JSON tree cannot describe different worlds: they are
//! the same parse, stored twice.

use std::path::Path;

use serde::Serialize;

use super::records::{
    BakedCity, BakedCityServiceEntry, BakedCorridor, BakedCurveRecord, BakedFacilityApproach,
    BakedFacilityEndpoint, BakedLeg, BakedLocalApproach, BakedLocalGeometry, BakedStop,
};
use super::{
    encode, text_section_name, Section, CODEC_REGION, CODEC_ZSTD, FORMAT_VERSION, HEADER_LEN,
    MAGIC, SECTION_CITIES, SECTION_CITY_SERVICES, SECTION_CORRIDORS, SECTION_CURVES,
    SECTION_FACILITY_APPROACHES, SECTION_FACILITY_ENDPOINTS, SECTION_LEGS,
    SECTION_LOCAL_APPROACHES, SECTION_LOCAL_GEOMETRY, TEXT_FILES,
};
use crate::data::curves;
use crate::data::world::World;
use crate::data::world_local_data::{
    load_city_service_data, load_facility_approaches, load_facility_endpoints,
    load_local_approaches, load_local_geometries,
};
use crate::data::world_models::DataError;

/// Compression level for the sections decoded whole. High enough to matter
/// on 30 MB of side maps, low enough that a re-bake is seconds rather than
/// minutes -- zstd decompression speed barely moves with the level, so this
/// trades bake time against size and nothing else.
const BLOB_LEVEL: i32 = 12;
/// Per-leg corridor frames. Each is small (a few KB to a few hundred KB) and
/// is decompressed on the frame a player enters the leg, so this one is
/// chosen for decode latency headroom.
const CORRIDOR_LEVEL: i32 = 12;
/// The loose JSON texts are tiny and never on a latency path.
const TEXT_LEVEL: i32 = 19;

/// What one section cost, JSON against baked.
#[derive(Debug, Clone)]
pub struct SectionSize {
    pub name: String,
    /// Bytes of JSON this section replaces (0 where another row counts them).
    pub json_bytes: u64,
    /// Bytes the section occupies in the container.
    pub baked_bytes: u64,
}

/// The size table `ff-bake` prints.
#[derive(Debug, Clone, Default)]
pub struct BakeReport {
    pub sections: Vec<SectionSize>,
    pub total_json: u64,
    pub total_baked: u64,
    /// Legs written, and how many carried a corridor blob.
    pub legs: usize,
    pub legs_with_corridor: usize,
    /// Short speed-limit postings the load-time artifact fold removed.
    pub speed_limits_folded: usize,
}

impl BakeReport {
    /// The size table, one line per section plus a total.
    pub fn table(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!(
            "{:<28} {:>14} {:>14} {:>8}\n",
            "section", "json bytes", "baked bytes", "ratio"
        ));
        out.push_str(&format!("{}\n", "-".repeat(68)));
        for section in &self.sections {
            let ratio = if section.json_bytes > 0 {
                format!(
                    "{:.1}x",
                    section.json_bytes as f64 / section.baked_bytes.max(1) as f64
                )
            } else {
                "-".to_string()
            };
            out.push_str(&format!(
                "{:<28} {:>14} {:>14} {:>8}\n",
                section.name, section.json_bytes, section.baked_bytes, ratio
            ));
        }
        out.push_str(&format!("{}\n", "-".repeat(68)));
        out.push_str(&format!(
            "{:<28} {:>14} {:>14} {:>8}\n",
            "total",
            self.total_json,
            self.total_baked,
            format!(
                "{:.1}x",
                self.total_json as f64 / self.total_baked.max(1) as f64
            )
        ));
        out
    }
}

/// A container under construction: payload bytes plus the directory.
struct Writer {
    body: Vec<u8>,
    directory: Vec<Section>,
    report: BakeReport,
}

impl Writer {
    fn new() -> Self {
        Writer {
            body: Vec::with_capacity(32 << 20),
            directory: Vec::new(),
            report: BakeReport::default(),
        }
    }

    fn offset(&self) -> u64 {
        (HEADER_LEN + self.body.len()) as u64
    }

    /// Append a section already reduced to bytes.
    fn push(&mut self, name: &str, payload: &[u8], raw_len: u64, codec: u8, json_bytes: u64) {
        let offset = self.offset();
        self.body.extend_from_slice(payload);
        self.directory.push(Section {
            name: name.to_string(),
            offset,
            stored: payload.len() as u64,
            raw: raw_len,
            codec,
        });
        self.report.sections.push(SectionSize {
            name: name.to_string(),
            json_bytes,
            baked_bytes: payload.len() as u64,
        });
        self.report.total_json += json_bytes;
        self.report.total_baked += payload.len() as u64;
    }

    /// Encode, compress, append.
    fn push_value<T: Serialize>(
        &mut self,
        name: &str,
        value: &T,
        level: i32,
        json_bytes: u64,
    ) -> Result<(), DataError> {
        let plain = encode(value)?;
        let packed = deflate(&plain, level)?;
        self.push(name, &packed, plain.len() as u64, CODEC_ZSTD, json_bytes);
        Ok(())
    }

    fn finish(mut self) -> Result<(Vec<u8>, BakeReport), DataError> {
        let dir_offset = self.offset();
        let dir_bytes = encode(&self.directory)?;
        self.report.sections.push(SectionSize {
            name: "directory".to_string(),
            json_bytes: 0,
            baked_bytes: dir_bytes.len() as u64,
        });
        self.report.total_baked += dir_bytes.len() as u64;
        let mut out = Vec::with_capacity(HEADER_LEN + self.body.len() + dir_bytes.len());
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&dir_offset.to_le_bytes());
        out.extend_from_slice(&(dir_bytes.len() as u64).to_le_bytes());
        debug_assert_eq!(out.len(), HEADER_LEN);
        out.extend_from_slice(&self.body);
        out.extend_from_slice(&dir_bytes);
        Ok((out, self.report))
    }
}

fn deflate(bytes: &[u8], level: i32) -> Result<Vec<u8>, DataError> {
    zstd::bulk::compress(bytes, level).map_err(|e| DataError::io(format!("zstd: {e}")))
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn dir_len(dir: &Path, extension: &str) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == extension))
        .map(|p| file_len(&p))
        .sum()
}

/// Bake `data_dir` (the Python package's `data/` folder) into container bytes.
///
/// Deterministic: the same tree gives the same bytes, so `--check` is a plain
/// byte comparison and a re-bake diffs as "unchanged".
pub fn bake_bytes(data_dir: &Path) -> Result<(Vec<u8>, BakeReport), DataError> {
    let world_dir = data_dir.join("world_data");
    let us_dir = world_dir.join("us");
    let gameplay = us_dir.join("gameplay");

    // Everything below runs the shipped loaders, never a private parser.
    let world = World::load_from_json(data_dir)?;
    let mut writer = Writer::new();

    // --- cities (eager) -----------------------------------------------
    let cities_json = file_len(&world_dir.join("index.json"))
        + file_len(&world_dir.join("geo.json"))
        + file_len(&us_dir.join("cities.json"));
    let cities: Vec<(String, BakedCity)> = world
        .cities
        .iter()
        .map(|(key, city)| (key.clone(), BakedCity::from(city)))
        .collect();
    writer.push_value(SECTION_CITIES, &cities, BLOB_LEVEL, cities_json)?;

    // --- legs: eager fields, and one zstd frame per corridor ----------
    let legs_json = dir_len(&us_dir.join("legs"), "json");
    let mut corridor_region: Vec<u8> = Vec::with_capacity(24 << 20);
    let mut legs: Vec<BakedLeg> = Vec::with_capacity(world.legs.len());
    let mut with_corridor = 0usize;
    for leg in &world.legs {
        let detail = leg.try_corridor()?;
        writer.report.speed_limits_folded += detail.speed_limits_folded;
        let baked = BakedCorridor::from(detail);
        let plain = encode(&baked)?;
        // An empty corridor is the common case for a fixture-shaped leg and
        // costs nothing to store as "no blob at all".
        let empty = plain.len() <= 32 && detail == &Default::default();
        let (offset, stored, raw) = if empty {
            (0u64, 0u32, 0u32)
        } else {
            let packed = deflate(&plain, CORRIDOR_LEVEL)?;
            let offset = corridor_region.len() as u64;
            corridor_region.extend_from_slice(&packed);
            with_corridor += 1;
            (offset, packed.len() as u32, plain.len() as u32)
        };
        legs.push(BakedLeg {
            a: leg.a.clone(),
            b: leg.b.clone(),
            miles: leg.miles,
            highway: leg.highway.clone(),
            terrain: leg.terrain.clone(),
            stops: leg.stops.iter().map(BakedStop::from).collect(),
            truck_advisory: leg.truck_advisory.clone(),
            lanes: leg.lanes,
            local_cue: leg.local_cue.clone(),
            local_speed_mph: leg.local_speed_mph,
            local_turn_deg: leg.local_turn_deg,
            divided: leg.divided,
            meta_complete: leg.meta_complete,
            corridor_offset: offset,
            corridor_stored_len: stored,
            corridor_raw_len: raw,
        });
    }
    writer.report.legs = legs.len();
    writer.report.legs_with_corridor = with_corridor;
    writer.push_value(SECTION_LEGS, &legs, BLOB_LEVEL, legs_json)?;
    let region_len = corridor_region.len() as u64;
    writer.push(
        SECTION_CORRIDORS,
        &corridor_region,
        region_len,
        CODEC_REGION,
        0,
    );
    drop(corridor_region);

    // --- curves --------------------------------------------------------
    let curves_path = gameplay.join("curves.jsonl");
    let artifacts_path = gameplay.join("curve_artifacts.jsonl");
    let curves_json = file_len(&curves_path) + file_len(&artifacts_path);
    let curve_text = std::fs::read_to_string(&curves_path).unwrap_or_default();
    let artifact_text = std::fs::read_to_string(&artifacts_path).unwrap_or_default();
    let screened = curves::build_from_sources(&curve_text, &artifact_text, &world);
    // `build_from_sources` answers with a hash map; sort so the bytes are the
    // same every run.
    let mut curve_keys: Vec<&String> = screened.keys().collect();
    curve_keys.sort();
    let curve_pairs: Vec<(String, Vec<BakedCurveRecord>)> = curve_keys
        .into_iter()
        .map(|key| {
            (
                key.clone(),
                screened[key].iter().map(BakedCurveRecord::from).collect(),
            )
        })
        .collect();
    writer.push_value(SECTION_CURVES, &curve_pairs, BLOB_LEVEL, curves_json)?;

    // --- the five nationwide side maps ---------------------------------
    let services = load_city_service_data(&data_dir.join("city_services.json"))?;
    let service_pairs: Vec<(String, Vec<(String, BakedCityServiceEntry)>)> = services
        .iter()
        .map(|(city, entries)| {
            (
                city.clone(),
                entries
                    .iter()
                    .map(|(key, entry)| (key.clone(), BakedCityServiceEntry::from(entry)))
                    .collect(),
            )
        })
        .collect();
    writer.push_value(
        SECTION_CITY_SERVICES,
        &service_pairs,
        BLOB_LEVEL,
        file_len(&data_dir.join("city_services.json")),
    )?;

    let approaches = load_facility_approaches(&data_dir.join("facility_approaches.json"))?;
    let approach_pairs: Vec<(String, BakedFacilityApproach)> = approaches
        .iter()
        .map(|(key, value)| (key.clone(), BakedFacilityApproach::from(value)))
        .collect();
    writer.push_value(
        SECTION_FACILITY_APPROACHES,
        &approach_pairs,
        BLOB_LEVEL,
        file_len(&data_dir.join("facility_approaches.json")),
    )?;

    let endpoints = load_facility_endpoints(&data_dir.join("facility_endpoints.json"))?;
    let endpoint_pairs: Vec<(String, BakedFacilityEndpoint)> = endpoints
        .iter()
        .map(|(key, value)| (key.clone(), BakedFacilityEndpoint::from(value)))
        .collect();
    writer.push_value(
        SECTION_FACILITY_ENDPOINTS,
        &endpoint_pairs,
        BLOB_LEVEL,
        file_len(&data_dir.join("facility_endpoints.json")),
    )?;

    let local = load_local_approaches(&data_dir.join("local_approaches.json"))?;
    let local_pairs: Vec<(String, BakedLocalApproach)> = local
        .iter()
        .map(|(key, value)| (key.clone(), BakedLocalApproach::from(value)))
        .collect();
    writer.push_value(
        SECTION_LOCAL_APPROACHES,
        &local_pairs,
        BLOB_LEVEL,
        file_len(&data_dir.join("local_approaches.json")),
    )?;

    let geometry = load_local_geometries(&data_dir.join("local_geometry.json"))?;
    let geometry_pairs: Vec<(String, BakedLocalGeometry)> = geometry
        .iter()
        .map(|(key, value)| (key.clone(), BakedLocalGeometry::from(value)))
        .collect();
    writer.push_value(
        SECTION_LOCAL_GEOMETRY,
        &geometry_pairs,
        BLOB_LEVEL,
        file_len(&data_dir.join("local_geometry.json")),
    )?;

    // --- the loose JSON texts ------------------------------------------
    for relative in TEXT_FILES {
        let path = data_dir.join(relative);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let packed = deflate(text.as_bytes(), TEXT_LEVEL)?;
        writer.push(
            &text_section_name(relative),
            &packed,
            text.len() as u64,
            CODEC_ZSTD,
            file_len(&path),
        );
    }

    writer.finish()
}

/// Bake `data_dir` and write the container to `out`.
pub fn bake(data_dir: &Path, out: &Path) -> Result<BakeReport, DataError> {
    let (bytes, report) = bake_bytes(data_dir)?;
    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| DataError::io(format!("{}: {e}", parent.display())))?;
        }
    }
    std::fs::write(out, &bytes).map_err(|e| DataError::io(format!("{}: {e}", out.display())))?;
    Ok(report)
}

/// Sanity check used by the tests: nothing in the bake depends on a hash
/// iteration order, so two bakes of one tree agree byte for byte.
pub fn bake_is_deterministic(data_dir: &Path) -> Result<bool, DataError> {
    let (first, _) = bake_bytes(data_dir)?;
    let (second, _) = bake_bytes(data_dir)?;
    Ok(first == second)
}
