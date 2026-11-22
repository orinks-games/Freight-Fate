//! `nbi-decks`: the deck of every bridge the pavement layer has, from the
//! National Bridge Inventory, as `deck=`, `nbi=` and `grate=` tags on the
//! structure's line in `corridor.pavement`.
//!
//! Run it after `hpms-pavement`, which rewrites the layer and drops these
//! tags. Every record carried on a structure (record type 1) in 20-mile
//! boxes along the archived road is fetched from the NTAD feature service
//! and cached under `.route-cache/nbi/`; the match is `decks`. A dry run
//! until `--write`.

pub mod decks;

use rayon::prelude::*;
use std::collections::{BTreeMap, HashSet};

use self::decks::{match_direction, place, rewrite, Line, Matched, Record, Reject};
use super::hpms::pavement_match::RoadIndex;
use super::hpms::traffic::envelopes_with;
use super::hpms::Client;
use crate::cli::{Args, Spec};
use crate::pyjson::Value;
use crate::util::Result;
use crate::world::layers::{corridor_mut, leg_id, leg_miles, round, Only, Route, Run};

const USAGE: &str = "usage: nbi-decks [--only from:to;from:to] [--verbose] [--refresh] [--write]";

pub const QUERY_URL: &str = "https://services.arcgis.com/xOi1kZaI0eWDREZv/arcgis/rest/services/\
                             NTAD_National_Bridge_Inventory/FeatureServer/0/query";
const BOX_SPAN_MI: f64 = 20.0;
const BOX_PAD_M: f64 = 400.0;
const PAGE_SIZE: usize = 2000;
const FIELDS: &str = "OBJECTID,STATE_CODE_001,STRUCTURE_NUMBER_008,FACILITY_CARRIED_007,\
                      FEATURES_DESC_006A,STRUCTURE_TYPE_043B,MAIN_UNIT_SPANS_045,\
                      MAX_SPAN_LEN_MT_048,STRUCTURE_LEN_MT_049,DECK_STRUCTURE_TYPE_107,\
                      SURFACE_TYPE_108A,LATDD,LONGDD";
const WHERE: &str = "RECORD_TYPE_005A='1'";
/// A structure this long or more counts toward the match rate: the bar the
/// bridge joints and the deck whine use.
const COUNTED_MI: f64 = 0.05;

pub fn source(accessed: &str) -> String {
    format!(
        "National Bridge Inventory, NTAD feature service (Bureau of Transportation Statistics, \
         https://services.arcgis.com/xOi1kZaI0eWDREZv/arcgis/rest/services/\
         NTAD_National_Bridge_Inventory/FeatureServer/0), accessed {accessed}; codes per the \
         FHWA Recording and Coding Guide for the Structure Inventory and Appraisal of the \
         Nation's Bridges (1995, https://www.fhwa.dot.gov/bridge/mtguide.pdf): Item 5A record \
         type (page 2), Item 43B structure type (page 24), Items 45, 48 and 49 spans and \
         lengths (pages 26 to 28), Item 107 deck structure type and Item 108A wearing surface \
         (page 70). A structure's deck= is READ: open_grating is Item 107 code 3; concrete is \
         107 code 1 or 2 under 108A 0 to 4; asphalt is 108A code 6; other is anything else. \
         Which record belongs to which structure is DERIVED: a record carried on the structure \
         whose point projects within 60 m of the archived road, inside the structure or within \
         0.02 mi of its ends, with an Item 49 length within 0.05 mi of the structure's measured \
         span; the best length fit wins, then the nearer point. nbi= is the record's state FIPS \
         and structure number. grate=from:to on a movable open grating (43B 15, 16 or 17) is \
         DERIVED: Item 45 spans times Item 48, centred on the structure (NBI's point may be the \
         bridge's beginning, Item 16, page 8), clipped to it. A structure with no tag matched \
         no record."
    )
}

fn str_field(f: &serde_json::Value, k: &str) -> String {
    f.pointer(&format!("/attributes/{k}"))
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn num_field(f: &serde_json::Value, k: &str) -> Option<f64> {
    let v = f.pointer(&format!("/attributes/{k}"))?;
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.trim().parse().ok()))
}

fn query_box(client: &Client, b: (f64, f64, f64, f64)) -> Result<Vec<(i64, Record)>> {
    let mut out = Vec::new();
    let mut offset = 0;
    loop {
        let v = client.query(&[
            ("where", WHERE.into()),
            ("geometry", super::hpms::envelope(b)),
            ("geometryType", "esriGeometryEnvelope".into()),
            ("inSR", "4326".into()),
            ("spatialRel", "esriSpatialRelIntersects".into()),
            ("outFields", FIELDS.into()),
            ("returnGeometry", "false".into()),
            ("orderByFields", "OBJECTID".into()),
            ("resultOffset", offset.to_string()),
            ("resultRecordCount", PAGE_SIZE.to_string()),
            ("f", "json".into()),
        ])?;
        let Some(features) = v.get("features").and_then(|f| f.as_array()) else {
            break;
        };
        for f in features {
            let (Some(lat), Some(lon)) = (num_field(f, "LATDD"), num_field(f, "LONGDD")) else {
                continue;
            };
            out.push((
                num_field(f, "OBJECTID").map_or(-1, |x| x as i64),
                Record {
                    id: format!(
                        "{}-{}",
                        str_field(f, "STATE_CODE_001"),
                        str_field(f, "STRUCTURE_NUMBER_008").replace(' ', "_")
                    ),
                    lat,
                    lon,
                    type_43b: num_field(f, "STRUCTURE_TYPE_043B").map_or(0, |x| x as i64),
                    main_spans: num_field(f, "MAIN_UNIT_SPANS_045").map(|x| x as i64),
                    max_span_m: num_field(f, "MAX_SPAN_LEN_MT_048"),
                    length_m: num_field(f, "STRUCTURE_LEN_MT_049"),
                    deck_107: str_field(f, "DECK_STRUCTURE_TYPE_107"),
                    surface_108a: str_field(f, "SURFACE_TYPE_108A"),
                    carried: str_field(f, "FACILITY_CARRIED_007"),
                    over: str_field(f, "FEATURES_DESC_006A"),
                },
            ));
        }
        if features.is_empty()
            || v.get("exceededTransferLimit").and_then(|x| x.as_bool()) != Some(true)
        {
            break;
        }
        offset += PAGE_SIZE;
    }
    Ok(out)
}

fn fetch(client: &Client, route: &Route) -> Result<Vec<Record>> {
    let boxes = envelopes_with(route, BOX_SPAN_MI, BOX_PAD_M);
    let answers: Result<Vec<Vec<(i64, Record)>>> =
        boxes.par_iter().map(|b| query_box(client, *b)).collect();
    let mut seen = HashSet::new();
    Ok(answers?
        .into_iter()
        .flatten()
        .filter(|(id, _)| *id < 0 || seen.insert(*id))
        .map(|(_, r)| r)
        .collect())
}

/// One leg's tallies, for the report and the totals.
#[derive(Debug, Default, Clone)]
pub struct Tally {
    /// Bridges and causeways of [`COUNTED_MI`] or more, per direction
    /// summed, and how many matched a record.
    pub counted: usize,
    pub matched: usize,
    pub decks: BTreeMap<&'static str, usize>,
    pub ties: usize,
    pub tie_disagree: usize,
    pub split: usize,
    pub rejects: BTreeMap<Reject, usize>,
}

impl Tally {
    fn add(&mut self, o: &Tally) {
        self.counted += o.counted;
        self.matched += o.matched;
        for (k, v) in &o.decks {
            *self.decks.entry(k).or_default() += v;
        }
        self.ties += o.ties;
        self.tie_disagree += o.tie_disagree;
        self.split += o.split;
        for (k, v) in &o.rejects {
            *self.rejects.entry(*k).or_default() += v;
        }
    }
}

struct LegResult {
    lines: Vec<String>,
    tally: Tally,
    /// Every open grating within 150 m of the road: (record, at, off, kept).
    gratings: Vec<(Record, f64, f64, Option<Reject>)>,
}

fn read_leg(client: &Client, route: &Route, layer: &Value) -> Result<Option<LegResult>> {
    let Some(texts) = layer.get("structures").and_then(Value::as_array) else {
        return Ok(None);
    };
    let lines: Vec<Line> = texts
        .iter()
        .filter_map(|t| t.as_str().and_then(Line::parse))
        .collect();
    if lines.len() != texts.len() {
        return Err("a structure line does not parse".into());
    }
    let scale = layer
        .get("meta")
        .and_then(|m| m.get("mile_scale"))
        .and_then(Value::as_f64)
        .unwrap_or(1.0);
    let records = fetch(client, route)?;
    let index = RoadIndex::new(route);
    let on_road = place(&index, &records);
    let fwd = match_direction(&lines, true, &on_road, &records, scale);
    let rev = match_direction(&lines, false, &on_road, &records, scale);
    let (out, split) = rewrite(&lines, &fwd, &rev);
    let mut tally = Tally {
        split,
        ties: fwd.ties + rev.ties,
        tie_disagree: fwd.tie_disagree + rev.tie_disagree,
        ..Default::default()
    };
    let tally_dir = |m: &Matched, t: &mut Tally| {
        for (g, k) in &m.kept {
            if g.to_mi - g.from_mi < COUNTED_MI {
                continue;
            }
            t.counted += 1;
            if let Some(k) = k {
                t.matched += 1;
                *t.decks.entry(k.deck).or_default() += 1;
            }
        }
    };
    tally_dir(&fwd, &mut tally);
    tally_dir(&rev, &mut tally);
    let fate = |i: usize| -> Option<Reject> {
        match (fwd.fate[i], rev.fate[i]) {
            (None, _) | (_, None) => None,
            (Some(a), Some(b)) => Some(a.max(b)),
        }
    };
    let mut gratings = Vec::new();
    for (i, p) in on_road.iter().enumerate() {
        if let Some(r) = fate(i) {
            if p.off_m <= 150.0 {
                *tally.rejects.entry(r).or_default() += 1;
            }
        }
        let rec = &records[p.record];
        if rec.deck() == "open_grating" && p.off_m <= 150.0 {
            gratings.push((rec.clone(), p.at_mi, p.off_m, fate(i)));
        }
    }
    Ok(Some(LegResult {
        lines: out,
        tally,
        gratings,
    }))
}

fn report(id: &str, r: &LegResult, verbose: bool) {
    let t = &r.tally;
    let decks: Vec<String> = t.decks.iter().map(|(k, v)| format!("{k} {v}")).collect();
    let rejects: Vec<String> = t
        .rejects
        .iter()
        .map(|(k, v)| format!("{v} {}", k.name()))
        .collect();
    println!(
        "  {id}: {} of {} structure crossings of {COUNTED_MI} mi or more matched ({:.0}%); \
         decks [{}]; {} ties ({} disagreeing on the deck), {} shared lines split; records \
         within 150 m not kept: {}",
        t.matched,
        t.counted,
        100.0 * t.matched as f64 / t.counted.max(1) as f64,
        decks.join(", "),
        t.ties,
        t.tie_disagree,
        t.split,
        rejects.join(", ")
    );
    for (rec, at, off, fate) in &r.gratings {
        println!(
            "    open grating {} ({} over {}, {:.0} m, type {}): mile {at:.2}, {off:.0} m off, {}",
            rec.id,
            rec.carried,
            rec.over,
            rec.length_m.unwrap_or(0.0),
            rec.type_43b,
            fate.map_or("KEPT", Reject::name)
        );
    }
    if verbose {
        for l in &r.lines {
            if l.contains("deck=") {
                println!("    {l}");
            }
        }
    }
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["only"],
            flags: &["write", "refresh", "verbose"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let only = Only::parse(&args);
    let client = Client::service("NBI", QUERY_URL, "nbi", args.flag("refresh"));
    let note = source(&super::hpms::today());
    let mut run = Run::load()?;
    let archive = std::mem::take(&mut run.archive);
    let legs = run.legs_mut()?;
    let wanted: Vec<usize> = (0..legs.len())
        .filter(|&i| only.wants(&leg_id(&legs[i])))
        .collect();
    let total = wanted.len();
    let results: Vec<(usize, Result<Option<LegResult>>)> = wanted
        .par_iter()
        .map(|&i| {
            let leg = &legs[i];
            let id = leg_id(leg);
            let Some(line) = archive.get(&id) else {
                return (i, Ok(None));
            };
            let Some(layer) = leg.get("corridor").and_then(|c| c.get("pavement")) else {
                return (i, Ok(None));
            };
            let route = Route::new(line, leg_miles(leg));
            (i, read_leg(&client, &route, layer))
        })
        .collect();
    let (mut changed, mut same, mut skipped, mut failed) = (0, 0, 0, 0);
    let mut all = Tally::default();
    let mut gratings_kept = 0;
    for (done, (i, result)) in results.into_iter().enumerate() {
        let id = leg_id(&legs[i]);
        let r = match result {
            Ok(Some(r)) => r,
            Ok(None) => {
                skipped += 1;
                continue;
            }
            Err(e) => {
                println!("  {id}: skipped, {e}");
                failed += 1;
                continue;
            }
        };
        all.add(&r.tally);
        gratings_kept += r.gratings.iter().filter(|g| g.3.is_none()).count();
        if only.is_some() {
            report(&id, &r, args.flag("verbose"));
        } else if (done + 1) % 100 == 0 {
            println!("  [{}/{total}] {id}", done + 1);
        }
        let corridor = corridor_mut(&mut legs[i])?;
        let Some(Value::Object(layer)) = corridor.get_mut("pavement") else {
            continue;
        };
        let new_lines: Vec<Value> = r.lines.into_iter().map(Value::Str).collect();
        let share = round(r.tally.matched as f64 / r.tally.counted.max(1) as f64, 3);
        let before = layer.clone();
        layer.insert("structures", Value::Array(new_lines));
        if let Some(Value::Object(meta)) = layer.get_mut("meta") {
            meta.insert("deck_read_share", Value::Float(share));
            meta.insert("deck_source", Value::Str(note.clone()));
        }
        let strip = |v: &crate::pyjson::Object| {
            let mut v = v.clone();
            if let Some(Value::Object(m)) = v.get_mut("meta") {
                m.remove("deck_source");
            }
            v
        };
        if strip(&before) == strip(layer) {
            same += 1;
        } else {
            changed += 1;
        }
    }
    run.archive = archive;
    let decks: Vec<String> = all.decks.iter().map(|(k, v)| format!("{k} {v}")).collect();
    let rejects: Vec<String> = all
        .rejects
        .iter()
        .map(|(k, v)| format!("{v} {}", k.name()))
        .collect();
    println!(
        "{changed} legs changed, {same} the same, {skipped} with no pavement layer or archived \
         road, {failed} failed and kept as they were. Structure crossings of {COUNTED_MI} mi or \
         more, each way: {} of {} matched a National Bridge Inventory record ({:.1}% READ, the \
         rest have no deck and fall back to the pavement run); decks [{}]; {} open gratings \
         kept. {} ties ({} disagreeing on the deck), {} shared lines split. Records within 150 \
         m not kept: {}. {} new answer(s) fetched.",
        all.matched,
        all.counted,
        100.0 * all.matched as f64 / all.counted.max(1) as f64,
        decks.join(", "),
        gratings_kept,
        all.ties,
        all.tie_disagree,
        all.split,
        rejects.join(", "),
        client.fetched.load(std::sync::atomic::Ordering::Relaxed)
    );
    run.finish(args.flag("write"))
}
