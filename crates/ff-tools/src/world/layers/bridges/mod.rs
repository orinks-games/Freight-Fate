//! `osm-bridges`: the named bridges each leg drives over
//! (`corridor.bridges`), with their real span and the water under them.
//!
//! What it reads from the extract: bridge-tagged road ways (`bridge=*` with
//! a road `highway=*`), `man_made=bridge` outlines, `type=bridge`
//! relations, and rivers, canals, tidal channels and coastline for the
//! water. A bridge is named by its own `bridge:name`, a bridge relation,
//! an outline, or a way name that reads as the bridge's. Spoken names go through
//! `world::spoken_name` (`name:en` only for script a voice cannot read).
//!
//! A bridge is written when it is on the curated sheet (`docs/bridges/`,
//! see `sheet.rs`), or when its real length reaches the bar
//! ([`BAR_FT`]). Its span is derived by projecting its ways onto the leg's
//! archived road (`place.rs`).
//!
//! ```text
//! cargo tools osm-bridges --pbf FILE [--only a:b;c:d] [--scope north|all]
//!                         [--bar-ft N] [--extract-date YYYY-MM-DD] [--write]
//! ```
//!
//! `--survey` also lists unnamed bridge runs by the way's own name and id
//! (to find a curated bridge's ways) and writes nothing.
//!
//! Writes require `--only` to list complete routes covered by the extract.
//! `--scope` intersects the list; it does not assert extract coverage.
//! `--scope north` (the default) selects legs touching Canada or Alaska;
//! `all` selects every leg. The dry run prints every placed bridge
//! for `--only`, and for every run the counts by length band and country,
//! how often the five-mile spacing guard would fire, and the sheet's
//! entries no leg crosses.

mod place;
mod sheet;

use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use super::features::{self, Select};
use super::{corridor_mut, leg_id, leg_miles, pbf_paths, Route, Run};
use crate::cli::{Args, Spec};
use crate::pyjson::Value;
use crate::util::Result;
use place::Catalog;
use sheet::Sheet;

const USAGE: &str = "usage: osm-bridges --pbf FILE [--pbf FILE ...] [--only from:to;from:to] \
                     [--scope north|all] [--bar-ft N] [--sheet DIR] [--extract-date YYYY-MM-DD] \
                     [--snapshot NAME] [--survey] [--write (requires --only)]";

/// The real length an uncurated bridge needs to be written, in feet.
///
/// No published standard defines a "major bridge" by total length in the
/// US or Canada: FHWA's only length class is "major or unusual structures"
/// (stayed girder, segmental, or a clear unsupported span over 500 feet,
/// Federal-aid Policy Guide NS 23 CFR 650 C, item 9.2), which is a main
/// span, not a total length, and OpenStreetMap does not record spans.
/// So the bar is calibrated against the curated sheet; see the report
/// (`/mnt/project-files/bridges/brief-2-report.md`) for how well it
/// separates.
pub const BAR_FT: f64 = 3000.0;

/// The spacing guard the game applies (`ff_core::sim::bridges`): one
/// spoken bridge name in any this many route miles, the longer winning.
const SPACING_MI: f64 = 5.0;

const BANDS_FT: &[(f64, &str)] = &[
    (500.0, "under 500 ft"),
    (1000.0, "500 to 1,000 ft"),
    (2000.0, "1,000 to 2,000 ft"),
    (3000.0, "2,000 to 3,000 ft"),
    (5000.0, "3,000 to 5,000 ft"),
    (10000.0, "5,000 to 10,000 ft"),
    (f64::INFINITY, "10,000 ft and over"),
];

fn band(ft: f64) -> &'static str {
    BANDS_FT
        .iter()
        .find(|(top, _)| ft < *top)
        .map_or("10,000 ft and over", |b| b.1)
}

/// A leg the north scope writes: one end in Canada or Alaska.
fn is_north(id: &str) -> bool {
    id.split(':')
        .any(|slug| slug.ends_with("_ca") || slug.ends_with("_ak_us"))
}

fn parse_covered_legs(value: Option<&str>, require: bool) -> Result<Option<Vec<String>>> {
    let Some(value) = value else {
        return if require {
            Err("--write requires --only with complete from:to routes".to_string())
        } else {
            Ok(None)
        };
    };
    let mut legs = Vec::new();
    for leg in value.split(';') {
        let leg = leg.trim();
        let Some((from, to)) = leg.split_once(':') else {
            return Err(format!("invalid --only leg {leg:?}; expected from:to"));
        };
        let (from, to) = (from.trim(), to.trim());
        if from.is_empty() || to.is_empty() || to.contains(':') {
            return Err(format!("invalid --only leg {leg:?}; expected from:to"));
        }
        legs.push(format!("{from}:{to}"));
    }
    if legs.is_empty() {
        return Err("--only must select at least one complete from:to route".to_string());
    }
    Ok(Some(legs))
}

fn writes_placements(write: bool, survey: bool) -> bool {
    write && !survey
}

fn validate_covered_legs(
    selected: &[String],
    leg_ids: &HashSet<String>,
    archive_ids: &HashSet<String>,
    scope: &str,
    publishing: bool,
) -> Result<()> {
    for leg in selected {
        let reverse = leg
            .split_once(':')
            .map(|(from, to)| format!("{to}:{from}"))
            .unwrap_or_default();
        let known = if leg_ids.contains(leg) {
            leg
        } else if leg_ids.contains(&reverse) {
            &reverse
        } else {
            return Err(format!("--only references unknown leg {leg:?}"));
        };
        if publishing && (scope == "all" || is_north(known)) && !archive_ids.contains(known) {
            return Err(format!("--only leg {known:?} has no archived road"));
        }
    }
    Ok(())
}

fn selected_leg(id: &str, selected: Option<&[String]>) -> bool {
    let Some(selected) = selected else {
        return true;
    };
    let reverse = id
        .split_once(':')
        .map(|(from, to)| format!("{to}:{from}"))
        .unwrap_or_default();
    selected.iter().any(|leg| leg == id || leg == &reverse)
}

fn selected_for_write(id: &str, selected: Option<&[String]>, scope: &str) -> bool {
    selected_leg(id, selected) && (scope == "all" || is_north(id))
}

/// Replace a leg's bridges; drop the hand-written `highway_marker` lines
/// that name a bridge it now writes, so one bridge has one line.
fn merge(corridor: &mut crate::pyjson::Object, placed: &[place::Placed]) -> bool {
    if placed.iter().any(|p| p.survey_only) {
        return false;
    }
    let rows = Value::Array(placed.iter().map(place::Placed::row).collect());
    let mut changed = false;
    if placed.is_empty() {
        changed |= corridor.remove("bridges").is_some();
    } else if corridor.get("bridges") != Some(&rows) {
        corridor.insert("bridges", rows);
        changed = true;
    }
    if let Some(Value::Array(landmarks)) = corridor.get_mut("landmarks") {
        let before = landmarks.len();
        landmarks.retain(|lm| {
            let marker = lm.get("category").and_then(Value::as_str) == Some("highway_marker");
            let text = lm.get("spoken").and_then(Value::as_str).unwrap_or("");
            !(marker && placed.iter().any(|p| text.contains(&p.name)))
        });
        changed |= landmarks.len() != before;
    }
    changed
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &[
                "pbf",
                "only",
                "scope",
                "bar-ft",
                "sheet",
                "extract-date",
                "snapshot",
            ],
            flags: &["write", "survey"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let survey = args.flag("survey");
    let publication = writes_placements(args.flag("write"), survey);
    let selection =
        parse_covered_legs(args.value("only"), publication).map_err(|e| format!("{e}\n{USAGE}"))?;
    let pbfs = pbf_paths(&args, USAGE)?;
    let date = super::extract_date(&args, &pbfs)?;
    let snapshot = args.string("snapshot", "OpenStreetMap planet file");
    let bar_ft: f64 = args.number("bar-ft", BAR_FT).map_err(|e| e.to_string())?;
    let scope = args.string("scope", "north");
    if scope != "north" && scope != "all" {
        return Err(format!("--scope is north or all\n{USAGE}"));
    }
    let sheet_dir = PathBuf::from(args.string("sheet", "docs/bridges"));
    let mut sheet = Sheet::load(&sheet_dir)?;
    let mut run = Run::load()?;
    if let Some(selected) = &selection {
        let leg_ids: HashSet<String> = run.legs_mut()?.iter().map(leg_id).collect();
        let archive_ids: HashSet<String> = run.archive.keys().cloned().collect();
        validate_covered_legs(selected, &leg_ids, &archive_ids, &scope, publication)?;
    }

    let way =
        |t: &[(&str, &str)]| place::is_carrier(t) || place::is_outline(t) || place::is_water(t);
    let relation = |t: &[(&str, &str)]| t.iter().any(|(k, v)| *k == "type" && *v == "bridge");
    let found = features::read(
        &pbfs,
        &Select {
            node: None,
            way: Some(&way),
            relation: Some(&relation),
        },
    )?;
    let carriers: Vec<(i64, (f64, f64))> = found
        .ways
        .iter()
        .filter(|w| {
            let pairs: Vec<(&str, &str)> = w
                .tags
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            place::is_carrier(&pairs)
        })
        .map(|w| (w.id, w.coords[w.coords.len() / 2]))
        .collect();
    sheet.resolve(&found, &carriers);
    let catalog = Catalog::new(found);
    let named = catalog.carriers.iter().filter(|c| c.name.is_some()).count();
    println!(
        "{} bridge-tagged road ways in the extracts, {named} of them named; {} water lines; \
         {} curated bridges on the sheet",
        catalog.carriers.len(),
        catalog.water.len(),
        sheet.bridges.len()
    );
    let note = format!("OpenStreetMap, {snapshot} of {date}: https://www.openstreetmap.org/.");
    let archive = std::mem::take(&mut run.archive);
    let (mut changed, mut same, mut no_archive) = (0, 0, 0);
    let mut bands: BTreeMap<(String, &str), usize> = BTreeMap::new();
    let mut placed_names: Vec<String> = Vec::new();
    let mut placed_on: Vec<String> = Vec::new();
    let (mut total, mut legs_with, mut guarded, mut water_n) = (0usize, 0usize, 0usize, 0usize);
    for leg in run.legs_mut()?.iter_mut() {
        let id = leg_id(leg);
        if !selected_leg(&id, selection.as_deref()) {
            continue;
        }
        let Some(line) = archive.get(&id) else {
            no_archive += 1;
            continue;
        };
        let route = Route::new(line, leg_miles(leg));
        let placed = place::bake(&catalog, &sheet, &route, bar_ft, &note, survey);
        let country = if is_north(&id) { "north" } else { "us" };
        for p in &placed {
            *bands
                .entry((country.to_string(), band(p.length_ft)))
                .or_default() += 1;
            placed_names.push(p.name.clone());
            placed_on.push(format!("{id}|{}", p.name));
            water_n += usize::from(p.water);
        }
        // How often the game's spacing guard would silence one, this leg.
        for (i, p) in placed.iter().enumerate() {
            let beaten = placed.iter().enumerate().any(|(j, q)| {
                j != i
                    && q.name != p.name
                    && (q.from_mi - p.from_mi).abs() < SPACING_MI
                    && (q.length_ft > p.length_ft || (q.length_ft == p.length_ft && j < i))
            });
            guarded += usize::from(beaten);
        }
        total += placed.len();
        legs_with += usize::from(!placed.is_empty());
        if selection.is_some() {
            let list: Vec<String> = placed
                .iter()
                .map(|p| {
                    format!(
                        "{} {:.2}-{:.2} {:.0} ft {}{}{}",
                        p.name,
                        p.from_mi,
                        p.to_mi,
                        p.length_ft,
                        p.ways,
                        if p.water { " water" } else { "" },
                        if p.over.is_empty() {
                            String::new()
                        } else {
                            format!(" over {}", p.over)
                        },
                    ) + &if survey {
                        format!(" [{}]", p.osm)
                    } else {
                        String::new()
                    }
                })
                .collect();
            println!("  {id}: {}", list.join("; "));
        }
        if survey || !selected_for_write(&id, selection.as_deref(), &scope) {
            continue;
        }
        if merge(corridor_mut(leg)?, &placed) {
            changed += 1;
        } else {
            same += 1;
        }
    }
    run.archive = archive;
    println!(
        "{total} {} placed on {legs_with} legs ({water_n} over water).",
        if survey {
            "bridge candidates"
        } else {
            "named bridges"
        }
    );
    println!("By length band (north = legs touching Canada or Alaska):");
    for ((country, band), n) in &bands {
        println!("  {country:5} {band:20} {n}");
    }
    println!(
        "The five-mile spacing guard would silence {guarded} of them (a longer one within 5 \
         miles on the same leg)."
    );
    placed_names.sort();
    placed_names.dedup();
    let missing: Vec<String> = sheet
        .unplaced(&placed_names)
        .map(|b| b.name.clone())
        .collect();
    if !missing.is_empty() {
        println!("On the sheet but on no leg: {}", missing.join("; "));
    }
    for b in &sheet.bridges {
        for leg in &b.legs {
            let (a, z) = leg.split_once(':').unwrap_or((leg, ""));
            let here = |id: &str| placed_on.contains(&format!("{id}|{}", b.name));
            if selected_leg(leg, selection.as_deref()) && !here(leg) && !here(&format!("{z}:{a}")) {
                println!("  expected on {leg} but not placed: {}", b.name);
            }
        }
    }
    println!(
        "{changed} legs changed, {same} the same in scope {scope}, {no_archive} with no archived \
         road."
    );
    run.finish(publication)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_require_nonempty_valid_only_selection() {
        assert!(parse_covered_legs(None, true).is_err());
        assert!(parse_covered_legs(Some(""), true).is_err());
        assert!(parse_covered_legs(Some("a:b;not-a-route"), true).is_err());
        assert!(parse_covered_legs(Some("a:b;:d"), true).is_err());
        assert!(parse_covered_legs(Some("a:b;c:d"), true).is_ok());
        assert!(writes_placements(true, false));
        assert!(!writes_placements(false, false));
        assert!(!writes_placements(false, true));
        assert!(!writes_placements(true, true));
        assert_eq!(parse_covered_legs(None, false).unwrap(), None);
        assert_eq!(
            parse_covered_legs(None, writes_placements(true, true)).unwrap(),
            None
        );
    }

    #[test]
    fn covered_routes_validate_identity_in_all_modes_but_require_archives_for_writes() {
        let selected = vec!["a:b".to_string(), "c:d".to_string()];
        let leg_ids = ["a:b".to_string(), "c:d".to_string()].into_iter().collect();
        let archive_ids = ["a:b".to_string()].into_iter().collect();
        assert!(validate_covered_legs(
            &selected,
            &leg_ids,
            &archive_ids,
            "all",
            writes_placements(false, false),
        )
        .is_ok());
        assert!(validate_covered_legs(
            &selected,
            &leg_ids,
            &archive_ids,
            "all",
            writes_placements(false, true),
        )
        .is_ok());
        assert!(validate_covered_legs(
            &selected,
            &leg_ids,
            &archive_ids,
            "all",
            writes_placements(true, true),
        )
        .is_ok());
        assert!(validate_covered_legs(
            &selected,
            &leg_ids,
            &archive_ids,
            "all",
            writes_placements(true, false),
        )
        .is_err());
        assert!(
            validate_covered_legs(&["a:e".into()], &leg_ids, &archive_ids, "all", false).is_err()
        );

        let outside_scope = vec!["seattle_wa_us:portland_or_us".to_string()];
        let outside_leg_ids: HashSet<String> = outside_scope.iter().cloned().collect();
        let no_archive = HashSet::new();
        assert!(validate_covered_legs(
            &outside_scope,
            &outside_leg_ids,
            &no_archive,
            "north",
            true,
        )
        .is_ok());
    }

    #[test]
    fn writes_intersect_scope_and_selection_preserving_unselected_legs() {
        let selection = Some(vec!["seattle_wa_us:portland_or_us".to_string()]);
        let selected_id = "seattle_wa_us:portland_or_us";
        let unselected_id = "vancouver_bc_ca:seattle_wa_us";
        assert!(!selected_for_write(
            selected_id,
            selection.as_deref(),
            "north"
        ));
        assert!(selected_for_write(selected_id, selection.as_deref(), "all"));
        let north = Some(vec!["detroit_mi_us:windsor_on_ca".to_string()]);
        assert!(selected_for_write(
            "detroit_mi_us:windsor_on_ca",
            north.as_deref(),
            "north"
        ));

        let stale = Value::Array(Vec::new());
        let mut selected = crate::pyjson::Object::new();
        selected.insert("bridges", stale.clone());
        let mut unselected = crate::pyjson::Object::new();
        unselected.insert("bridges", stale.clone());
        if selected_for_write(selected_id, selection.as_deref(), "all") {
            merge(&mut selected, &[]);
        }
        if selected_for_write(unselected_id, selection.as_deref(), "all") {
            merge(&mut unselected, &[]);
        }
        assert!(selected.get("bridges").is_none());
        assert_eq!(unselected.get("bridges"), Some(&stale));

        let survey_only = place::Placed {
            from_mi: 1.0,
            to_mi: 1.1,
            name: "? US 1 (way 1)".to_string(),
            over: String::new(),
            water: false,
            ways: "both",
            curated: false,
            osm: "way/1".to_string(),
            length_ft: 528.0,
            source: "survey-only candidate".to_string(),
            survey_only: true,
            found_on: 0,
        };
        let mut no_survey_publish = crate::pyjson::Object::new();
        no_survey_publish.insert("bridges", stale.clone());
        assert!(!merge(&mut no_survey_publish, &[survey_only]));
        assert_eq!(no_survey_publish.get("bridges"), Some(&stale));
    }

    #[test]
    fn the_north_scope_is_canada_and_alaska() {
        assert!(is_north("detroit_mi_us:windsor_on_ca"));
        assert!(is_north("tok_ak_us:fairbanks_ak_us"));
        assert!(!is_north("seattle_wa_us:portland_or_us"));
    }

    #[test]
    fn bands_read_in_feet() {
        assert_eq!(band(400.0), "under 500 ft");
        assert_eq!(band(3000.0), "3,000 to 5,000 ft");
        assert_eq!(band(42_000.0), "10,000 ft and over");
    }
}
