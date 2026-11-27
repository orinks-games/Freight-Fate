//! `import-canada-stations`: build `data/radio_imported_canada.json`, the
//! Canadian half of the imported dial tier.
//!
//! Radio Browser is the finding aid for stream addresses (read at build
//! time only, never by the game), and ISED's Broadcasting Database is the
//! licence record that says where each station's transmitter stands. A
//! Radio Browser row becomes a station on the dial when it names an
//! operating ISED station:
//!
//! 1. by call sign, written in capitals in its name ("CHFI-FM 98.1") or in a
//!    CBC stream mount (`/CBLAFM_CBC`); or
//! 2. failing that, by the one dial position in its name plus the province
//!    in its `state` field, when exactly one operating ISED transmitter in
//!    that province is on that frequency. Never a guess between two.
//!
//! A row that contradicts itself is left out and counted: a province that
//! none of the station's transmitters is in, or a dial position none of
//! them broadcasts on. Each station is placed once, at its main
//! transmitter (rebroadcasters are a follow-up). Position, frequency and
//! community are read from ISED; `range_miles` is derived from the largest
//! licensed ERP through the US tier's table, and AM is a flat 25 miles.
//!
//! ```text
//! cargo tools import-canada-stations --fetch
//! cargo tools import-canada-stations
//! cargo tools import-canada-stations --check
//! cargo tools import-canada-stations --transmitters FILE
//! cargo tools import-canada-stations --names FILE
//! ```
//!
//! Two cached inputs, both under the gitignored `data/radio-cache/` and
//! both rebuilt by `--fetch`: `rb_ca.json` (a Radio Browser
//! `countrycode=CA` snapshot ordered by clicks) and `ised/` (`fmstatio.dbf`,
//! `amstatio.dbf` and the export log from ISED's `baserad.zip`). Checked-in
//! inputs: the curated catalog, the US file and the community stations (a
//! call sign or stream already on the dial wins), and
//! `radio_stream_health.json` (a stream the sweep found dead stays off;
//! run `cargo tools check-radio-streams --only canada` and rebuild).
//!
//! `--transmitters FILE` also writes every operating Canadian station's
//! main transmitter, with or without a stream, for the site's placement
//! table. `--names FILE` writes a review table of every built station's
//! id, call sign, directory name and spoken name.
//!
//! A station's name is its brand, derived from the directory name by
//! [`spoken_name`]. What a rule cannot derive goes in
//! `radio_imported_canada_overrides.json`, keyed by station id the way
//! `radio_imported_overrides.json` is for the US file: `overrides` sets
//! fields on a built row, `dropped` leaves a row out, and an entry whose
//! station is no longer built fails the run.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::Duration;

use ff_core::radio::{call_sign_base, canonical_stream_url, normalize_stream_url};
use regex::Regex;

use super::canada_join::{
    band_word, call_signs_in, cbc_mount, clean_terrestrial_name, dial_text, frequency_in,
    load_ised, main_transmitter, province_named, range_for, spoken_name, Band, Stations,
    Transmitter,
};
use super::{catalog_rows, data_path, text};
use crate::cli::{Args, Spec};
use crate::pyjson::{self, Object, Options, Value};
use crate::util::{self, Result};

pub const OUTPUT: &str = "radio_imported_canada.json";
pub const OVERRIDES: &str = "radio_imported_canada_overrides.json";
pub const SOURCE_TEXT: &str = "community radio directory, Canadian transmitter site";
const RB_URL: &str = "https://de1.api.radio-browser.info/json/stations/search?countrycode=CA&hidebroken=true&limit=10000&order=clickcount&reverse=true";
const ISED_URL: &str = "https://www.ic.gc.ca/engineering/BC_DBF_FILES/baserad.zip";
const ISED_FILES: &[&str] = &["fmstatio.dbf", "amstatio.dbf"];

fn cache_dir() -> PathBuf {
    data_path("radio-cache")
}

/// Why a Radio Browser row did not become a station.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
enum LeftOut {
    NoMatch,
    AmbiguousFrequency,
    AmbiguousBand,
    ProvinceDisagrees,
    FrequencyDisagrees,
    NoMainTransmitter,
}

impl LeftOut {
    fn key(self) -> &'static str {
        match self {
            LeftOut::NoMatch => "left_out_no_call_sign_or_frequency",
            LeftOut::AmbiguousFrequency => "left_out_frequency_matches_several",
            LeftOut::AmbiguousBand => "left_out_band_unclear",
            LeftOut::ProvinceDisagrees => "left_out_province_disagrees",
            LeftOut::FrequencyDisagrees => "left_out_frequency_disagrees",
            LeftOut::NoMainTransmitter => "left_out_no_main_transmitter",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MatchedBy {
    CallSign,
    Frequency,
}

/// One Radio Browser row resolved to an ISED station.
struct Candidate<'a> {
    row: &'a Object,
    site: &'a Transmitter,
    by: MatchedBy,
    clicks: i64,
}

fn sites_in<'a>(stations: &'a Stations, base: &str) -> Vec<(Band, &'a [Transmitter])> {
    [Band::Am, Band::Fm]
        .into_iter()
        .filter_map(|band| {
            stations
                .get(&(base.to_string(), band))
                .map(|s| (band, s.as_slice()))
        })
        .collect()
}

/// The station a row names by call sign, with the band settled.
fn by_call_sign<'a>(
    stations: &'a Stations,
    name: &str,
    urls: &[&str],
) -> Option<std::result::Result<(Band, &'a [Transmitter]), LeftOut>> {
    let mut hits: Vec<(String, Option<Band>)> = call_signs_in(name);
    for url in urls {
        if let Some((base, band)) = cbc_mount(url) {
            hits.push((base, Some(band)));
        }
    }
    let (base, written) = hits
        .into_iter()
        .find(|(base, _)| !sites_in(stations, base).is_empty())?;
    let bands = sites_in(stations, &base);
    if let [only] = bands.as_slice() {
        return Some(Ok(*only));
    }
    let band = written
        .or_else(|| frequency_in(name).map(|(b, _)| b))
        .or_else(|| band_word(name));
    Some(
        band.and_then(|b| bands.into_iter().find(|(x, _)| *x == b))
            .ok_or(LeftOut::AmbiguousBand),
    )
}

/// The one operating transmitter in `province` on the dial position the
/// name gives, as its station's sites.
fn by_frequency<'a>(
    stations: &'a Stations,
    name: &str,
    province: Option<&str>,
) -> std::result::Result<(Band, &'a [Transmitter]), LeftOut> {
    let (Some((band, freq)), Some(province)) = (frequency_in(name), province) else {
        return Err(LeftOut::NoMatch);
    };
    let matches: Vec<&(String, Band)> = stations
        .iter()
        .filter(|((_, b), sites)| {
            *b == band
                && sites
                    .iter()
                    .any(|t| t.province == province && (t.frequency - freq).abs() < 0.05)
        })
        .map(|(key, _)| key)
        .collect();
    match matches.as_slice() {
        [] => Err(LeftOut::NoMatch),
        [key] => Ok((band, stations[key].as_slice())),
        _ => Err(LeftOut::AmbiguousFrequency),
    }
}

fn resolve<'a>(
    stations: &'a Stations,
    row: &'a Object,
) -> std::result::Result<Candidate<'a>, LeftOut> {
    let name = clean_terrestrial_name(text(row, "name"));
    let province = province_named(text(row, "state"));
    let urls = [text(row, "url_resolved"), text(row, "url")];
    let (by, (band, sites)) = match by_call_sign(stations, &name, &urls) {
        Some(found) => (MatchedBy::CallSign, found?),
        None => (
            MatchedBy::Frequency,
            by_frequency(stations, &name, province)?,
        ),
    };
    if province.is_some_and(|p| !sites.iter().any(|t| t.province == p)) {
        return Err(LeftOut::ProvinceDisagrees);
    }
    if let Some((written_band, freq)) = frequency_in(&name) {
        if written_band == band && !sites.iter().any(|t| (t.frequency - freq).abs() < 0.05) {
            return Err(LeftOut::FrequencyDisagrees);
        }
    }
    let site = main_transmitter(sites).ok_or(LeftOut::NoMainTransmitter)?;
    Ok(Candidate {
        row,
        site,
        by,
        clicks: row.get("clickcount").and_then(Value::as_i64).unwrap_or(0),
    })
}

static STW_EDGE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"^https?://\d+\.live\.streamtheworld\.com(?::\d+)?/([A-Za-z0-9_]+?)(?:_SC)?(?:\?.*)?$",
    )
    .expect("regex")
});

/// The stream address to store. A numbered StreamTheWorld edge
/// (`26183.live.streamtheworld.com`) is whichever server answered the day
/// Radio Browser checked; the broadcaster's own redirect address is the
/// stable one, the form the US tier already carries.
fn stable_stream_url(row: &Object) -> String {
    let resolved = text(row, "url_resolved");
    let url = if resolved.is_empty() {
        text(row, "url")
    } else {
        resolved
    };
    if let Some(c) = STW_EDGE.captures(url) {
        let ext = if text(row, "codec").to_uppercase().contains("AAC") {
            "aac"
        } else {
            "mp3"
        };
        return format!(
            "https://playerservices.streamtheworld.com/api/livestream-redirect/{}.{ext}",
            &c[1]
        );
    }
    canonical_stream_url(url)
}

fn station_row(c: &Candidate, stream_url: String) -> Object {
    let site = c.site;
    let call_sign = site.call_sign.replace('-', " ");
    let name = spoken_name(
        &clean_terrestrial_name(text(c.row, "name")),
        &dial_text(site.band, site.frequency),
        &site.community,
    );
    let tags = text(c.row, "tags").trim();
    let mut row = crate::pyobj! {
        "id" => format!("rb-ised-{}", text(c.row, "stationuuid")),
        "call_sign" => call_sign.clone(),
        "name" => if name.is_empty() { call_sign } else { name },
        "format" => if tags.is_empty() { format!("{} radio", site.band.label()) } else { tags.to_string() },
        "source" => SOURCE_TEXT,
        "source_type" => "imported",
        "station_type" => "imported",
        "stream_url" => stream_url,
        "stream_format" => text(c.row, "codec").to_lowercase(),
        "lat" => site.lat,
        "lon" => site.lon,
        "range_miles" => range_for(site.band, site.erp_kw),
        "market" => site.community.clone(),
        "state" => site.province,
        "country" => "CA",
        "safe_for_streaming" => false,
        "real_stream" => true,
        "supported" => true,
    };
    if site.band == Band::Fm {
        row.insert("frequency_mhz", Value::from(site.frequency));
    }
    row
}

/// What the dial already holds: call sign bases and stream identities.
struct Dial {
    call_signs: HashSet<String>,
    urls: HashSet<String>,
}

fn dial() -> Result<Dial> {
    let mut rows = catalog_rows("radio_catalog.json", "curated")?;
    for name in ["radio_imported.json", "radio_community.json"] {
        if data_path(name).exists() {
            rows.extend(catalog_rows(name, "imported")?);
        }
    }
    Ok(Dial {
        call_signs: rows
            .iter()
            .map(|r| call_sign_base(text(r, "call_sign")))
            .filter(|b| !b.is_empty())
            .collect(),
        urls: rows
            .iter()
            .map(|r| text(r, "stream_url"))
            .filter(|u| !u.is_empty())
            .map(normalize_stream_url)
            .collect(),
    })
}

/// Dead imported ids and repaired addresses from the last stream sweep.
fn health() -> (HashSet<String>, HashMap<String, String>) {
    let Ok(doc) = pyjson::load_path(&data_path("radio_stream_health.json")) else {
        return (HashSet::new(), HashMap::new());
    };
    let list = |key: &str| -> Vec<Object> {
        doc.get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_object)
            .filter(|r| text(r, "tier") == "imported")
            .cloned()
            .collect()
    };
    let dead = list("dead")
        .iter()
        .map(|r| text(r, "id").to_string())
        .collect();
    let repaired = list("repaired")
        .iter()
        .map(|r| {
            (
                text(r, "id").to_string(),
                text(r, "repaired_url").to_string(),
            )
        })
        .collect();
    (dead, repaired)
}

/// Hand corrections to the built rows: fields to set by station id, and
/// ids to leave out.
struct Overrides {
    set: Vec<(String, Object)>,
    dropped: HashSet<String>,
}

fn overrides() -> Result<Overrides> {
    let path = data_path(OVERRIDES);
    if !path.exists() {
        return Ok(Overrides {
            set: Vec::new(),
            dropped: HashSet::new(),
        });
    }
    let doc = pyjson::load_path(&path)?;
    let set = doc
        .get("overrides")
        .and_then(Value::as_object)
        .map(|o| {
            o.iter()
                .filter_map(|(id, fields)| {
                    let mut fields = fields.as_object()?.clone();
                    fields.remove("why");
                    Some((id.clone(), fields))
                })
                .collect()
        })
        .unwrap_or_default();
    let dropped = doc
        .get("dropped")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .map(|r| text(r, "id").to_string())
        .collect();
    Ok(Overrides { set, dropped })
}

/// The ISED export date, from the log file name ISED ships in the zip
/// (`IC_CANADA_EXPORT_D20261124_...`), as `2026-11-24`.
fn export_date(ised: &Path) -> String {
    let names = std::fs::read_dir(ised)
        .map(|d| {
            d.filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    names
        .iter()
        .filter_map(|n| n.strip_prefix("IC_CANADA_EXPORT_D"))
        .filter_map(|rest| rest.get(..8))
        .filter(|d| d.bytes().all(|b| b.is_ascii_digit()))
        .map(|d| format!("{}-{}-{}", &d[..4], &d[4..6], &d[6..]))
        .max()
        .unwrap_or_else(|| "unknown".into())
}

fn build(directory: &[Object], stations: &Stations, ised_date: &str) -> Result<Value> {
    let dial = dial()?;
    let (dead, repaired) = health();
    let hand = overrides()?;
    let mut counts: BTreeMap<&'static str, usize> = BTreeMap::new();
    let mut candidates = Vec::new();
    for row in directory {
        match resolve(stations, row) {
            Ok(c) => {
                *counts
                    .entry(match c.by {
                        MatchedBy::CallSign => "matched_by_call_sign",
                        MatchedBy::Frequency => "matched_by_frequency",
                    })
                    .or_default() += 1;
                candidates.push(c);
            }
            Err(why) => *counts.entry(why.key()).or_default() += 1,
        }
    }
    // The most clicked row of a station whose stream plays is the one kept.
    candidates.sort_by(|a, b| {
        b.clicks
            .cmp(&a.clicks)
            .then_with(|| text(a.row, "stationuuid").cmp(text(b.row, "stationuuid")))
    });
    let mut placed: HashSet<(String, Band)> = HashSet::new();
    let mut seen_urls = dial.urls.clone();
    let mut rows: Vec<Object> = Vec::new();
    for c in &candidates {
        let id = format!("rb-ised-{}", text(c.row, "stationuuid"));
        let key = (c.site.base.clone(), c.site.band);
        let reason = if dial.call_signs.contains(&c.site.base) {
            "dropped_call_sign_already_on_dial"
        } else if hand.dropped.contains(&id) {
            "dropped_by_override"
        } else if dead.contains(&id) {
            "dropped_unreachable"
        } else if placed.contains(&key) {
            "dropped_duplicate_of_placed_station"
        } else {
            let url = repaired
                .get(&id)
                .cloned()
                .unwrap_or_else(|| stable_stream_url(c.row));
            if !(url.starts_with("http://") || url.starts_with("https://")) {
                "dropped_no_stream"
            } else if !seen_urls.insert(normalize_stream_url(&url)) {
                "dropped_stream_already_on_dial"
            } else {
                placed.insert(key);
                rows.push(station_row(c, url));
                continue;
            }
        };
        *counts.entry(reason).or_default() += 1;
    }
    let built: HashSet<String> = rows.iter().map(|r| text(r, "id").to_string()).collect();
    let candidate_ids: HashSet<String> = candidates
        .iter()
        .map(|c| format!("rb-ised-{}", text(c.row, "stationuuid")))
        .collect();
    let stale: Vec<&String> = hand
        .set
        .iter()
        .map(|(id, _)| id)
        .filter(|id| !built.contains(*id))
        .chain(
            hand.dropped
                .iter()
                .filter(|id| !candidate_ids.contains(*id)),
        )
        .collect();
    if !stale.is_empty() {
        return Err(format!(
            "{OVERRIDES} names stations this build does not place: {stale:?}"
        ));
    }
    for row in &mut rows {
        let id = text(row, "id").to_string();
        if let Some((_, fields)) = hand.set.iter().find(|(k, _)| *k == id) {
            for (key, value) in fields.iter() {
                row.insert(key.clone(), value.clone());
            }
        }
    }
    rows.sort_by(|a, b| {
        (text(a, "call_sign"), text(a, "id")).cmp(&(text(b, "call_sign"), text(b, "id")))
    });
    let mut counts_obj = Object::new();
    counts_obj.insert("radio_browser_rows", Value::from(directory.len()));
    counts_obj.insert("stations", Value::from(rows.len()));
    for (k, v) in counts {
        counts_obj.insert(k, Value::from(v));
    }
    let notes = format!(
        "Canadian stations in the imported dial tier, beside radio_imported.json. Built by \
         `cargo tools import-canada-stations` from a Radio Browser snapshot (stream addresses \
         only) joined to ISED's Broadcasting Database (export of {ised_date}): by the call sign \
         in the station's name or CBC stream mount, or else by the one dial position in its name \
         and the province in its directory entry when exactly one operating ISED transmitter \
         there matches. Read from ISED: lat, lon, frequency_mhz, market (the licensed \
         community) and state (the province). Derived: range_miles, from the largest licensed \
         FM ERP through the US tier's power table; AM is a flat 25 miles. One row per station, \
         at its main transmitter; rebroadcasters are not placed. A call sign or stream already \
         on the dial wins, and streams the sweep found dead are left out (see \
         radio_stream_health.json)."
    );
    Ok(Value::Object(crate::pyobj! {
        "schema" => 1i64,
        "notes" => notes,
        "counts" => counts_obj,
        "stations" => rows.into_iter().map(Value::Object).collect::<Vec<_>>(),
    }))
}

/// Every operating Canadian station's main transmitter, for the site.
fn transmitter_table(stations: &Stations, ised_date: &str) -> Value {
    let rows: Vec<Value> = stations
        .values()
        .filter_map(|sites| main_transmitter(sites))
        .map(|t| {
            Value::Object(crate::pyobj! {
                "call_sign" => t.base.clone(),
                "band" => t.band.label(),
                "frequency" => t.frequency,
                "province" => t.province,
                "community" => t.community.clone(),
                "lat" => t.lat,
                "lon" => t.lon,
                "erp_kw" => t.erp_kw.map_or(Value::Null, Value::from),
            })
        })
        .collect();
    Value::Object(crate::pyobj! {
        "schema" => 1i64,
        "source" => format!(
            "ISED Broadcasting Database (baserad.zip, export of {ised_date}): every operating \
             Canadian AM and FM station's main transmitter. frequency is MHz for FM and kHz for \
             AM; erp_kw is the largest licensed FM ERP, null for AM. Written by \
             `cargo tools import-canada-stations --transmitters`."
        ),
        "ised_export" => ised_date,
        "transmitters" => rows,
    })
}

/// Every built station's id, call sign, directory name and spoken name,
/// tab-separated and sorted by call sign, for a person to read through.
fn names_review(catalog: &Value, directory: &[Object]) -> String {
    let raw: HashMap<&str, &str> = directory
        .iter()
        .map(|r| (text(r, "stationuuid"), text(r, "name")))
        .collect();
    let mut out = String::from("id\tcall_sign\tdirectory_name\tname\n");
    let clean = |s: &str| s.replace(['\t', '\n'], " ");
    for row in catalog
        .get("stations")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
    {
        let id = text(row, "id");
        let uuid = id.trim_start_matches("rb-ised-");
        out.push_str(&format!(
            "{id}\t{}\t{}\t{}\n",
            text(row, "call_sign"),
            clean(raw.get(uuid).copied().unwrap_or("")),
            clean(text(row, "name")),
        ));
    }
    out
}

fn render(value: &Value) -> String {
    pyjson::dumps(value, &Options::indent(1).sorted().unicode()) + "\n"
}

fn with_retries<T>(what: &str, mut job: impl FnMut() -> Result<T>) -> Result<T> {
    let mut last = String::new();
    for attempt in 1..=6 {
        match job() {
            Ok(v) => return Ok(v),
            Err(e) => {
                println!("  {what}: attempt {attempt} failed: {e}");
                last = e;
                std::thread::sleep(Duration::from_secs(2 * attempt));
            }
        }
    }
    Err(format!("{what}: {last}"))
}

fn fetch(cache: &Path) -> Result<()> {
    std::fs::create_dir_all(cache.join("ised")).map_err(|e| e.to_string())?;
    let agent = crate::http::agent(Some(Duration::from_secs(300)));
    let body = with_retries("Radio Browser", || {
        let r = crate::http::request(
            &agent,
            "GET",
            RB_URL,
            &[("User-Agent", "FreightFate-tools")],
            None,
        )?;
        if r.status != 200 {
            return Err(format!("HTTP {}", r.status));
        }
        pyjson::loads(&String::from_utf8_lossy(&r.body)).map_err(|e| e.to_string())?;
        Ok(r.body)
    })?;
    std::fs::write(cache.join("rb_ca.json"), body).map_err(|e| e.to_string())?;
    let zip = with_retries("ISED", || {
        let r = crate::http::request(&agent, "GET", ISED_URL, &[], None)?;
        if r.status != 200 {
            return Err(format!("HTTP {}", r.status));
        }
        Ok(r.body)
    })?;
    let mut archive =
        zip::ZipArchive::new(std::io::Cursor::new(zip)).map_err(|e| format!("ISED zip: {e}"))?;
    for k in 0..archive.len() {
        let mut entry = archive.by_index(k).map_err(|e| e.to_string())?;
        let name = entry
            .name()
            .map_err(|e| e.to_string())?
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_string();
        let wanted = ISED_FILES.contains(&name.as_str())
            || (name.starts_with("IC_CANADA_EXPORT_") && name.ends_with(".log"));
        if wanted {
            let mut out =
                std::fs::File::create(cache.join("ised").join(&name)).map_err(|e| e.to_string())?;
            std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
        }
    }
    println!("Fetched Radio Browser and ISED into {}", cache.display());
    Ok(())
}

const USAGE: &str =
    "usage: import-canada-stations [--fetch] [--check] [--transmitters FILE] [--names FILE] [--output FILE]";

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["transmitters", "output", "names"],
            flags: &["fetch", "check"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let cache = cache_dir();
    let output = args.path("output").unwrap_or_else(|| data_path(OUTPUT));
    if args.flag("fetch") {
        fetch(&cache)?;
    }
    let rb_path = cache.join("rb_ca.json");
    let ised = cache.join("ised");
    let missing: Vec<PathBuf> = std::iter::once(rb_path.clone())
        .chain(ISED_FILES.iter().map(|f| ised.join(f)))
        .filter(|p| !p.exists())
        .collect();
    if let Some(path) = missing.first() {
        // The caches are gitignored, so a fresh clone has none; a check that
        // quietly passed here would read as one that proved something.
        if args.flag("check") {
            println!(
                "Skipped: {} is not here. This check runs where the caches are.",
                path.display()
            );
            return Ok(0);
        }
        return Err(format!(
            "missing input: {}\nFetch it with: cargo tools import-canada-stations --fetch",
            path.display()
        ));
    }
    let directory: Vec<Object> = pyjson::load_path(&rb_path)?
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_object)
        .cloned()
        .collect();
    let stations = load_ised(&ised)?;
    let date = export_date(&ised);
    println!(
        "{} Radio Browser rows, {} operating Canadian stations in ISED (export of {date})",
        directory.len(),
        stations.len()
    );
    if let Some(path) = args.path("transmitters") {
        util::write_text(&path, &render(&transmitter_table(&stations, &date)))?;
        println!("Wrote {}", path.display());
    }
    let catalog = build(&directory, &stations, &date)?;
    let text_out = render(&catalog);
    if let Some(path) = args.path("names") {
        util::write_text(&path, &names_review(&catalog, &directory))?;
        println!("Wrote {}", path.display());
    }
    if let Some(counts) = catalog.get("counts").and_then(Value::as_object) {
        for (k, v) in counts.iter() {
            println!("  {k}: {}", v.py_str());
        }
    }
    if args.flag("check") {
        if std::fs::read_to_string(&output).ok().as_deref() != Some(text_out.as_str()) {
            eprintln!(
                "Out of date: {}\nRe-run cargo tools import-canada-stations",
                output.display()
            );
            return Ok(1);
        }
        println!("{} is up to date", OUTPUT);
        return Ok(0);
    }
    util::write_text(&output, &text_out)?;
    println!("Wrote {}", output.display());
    Ok(0)
}
