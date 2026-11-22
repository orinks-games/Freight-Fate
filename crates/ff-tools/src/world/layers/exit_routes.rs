//! `osm-exit-routes`: the spoken name of a numbered Canadian road an exit
//! signs (`corridor.interchanges[].via`).
//!
//! Ontario and Quebec tag a road's `ref` and an exit's `destination:ref` with
//! the bare number ("255", "8"), so "exit 185 for 255" told the driver a
//! number and not which road it was. The sign in Quebec says Route 255, and
//! in Ontario Highway 8 or County Road 8, which share their numbers. So each
//! bare number in a Canadian exit's `via` is matched to the OpenStreetMap
//! road route relation of that number nearest the exit (within
//! [`MATCH_M`]), and the relation names the kind of road:
//!
//! - its `network`: `CA:QC:A` is an autoroute ("A-20"), `CA:QC:R` a Quebec
//!   route ("Route 255"), `CA:ON:primary`, `secondary` and `tertiary` a
//!   King's or secondary highway ("Highway 8"), and Manitoba's `PTH` and
//!   `PR` their trunk highways and provincial roads;
//! - otherwise its own `name` ("Lambton County Road 31", "Durham Regional
//!   Road 60") gives the kind ("County Road 31", "Regional Road 60"), and
//!   an unnamed one its Ontario network (a county, a region, Ottawa).
//!
//! A provincial road beats a county road of the same number within reach.
//! A number with no relation in reach, or only a township line or sideroad,
//! stays as it was. French cardinals on the sign ("20 Est") are spoken in
//! English, as the rest of the guidance is, and a bilingual sign's second
//! copy of a road ("15 Sud;15 South") is dropped. Each rewritten exit's `source`
//! says the road kind was derived and from what.

use super::features::{self, tag, Select};
use super::{corridor_mut, leg_id, leg_miles, pbf_paths, Only, Run};
use crate::cli::{Args, Spec};
use crate::pyjson::Value;
use crate::util::Result;
use crate::world::osm::geo::seg_dist_m;

const USAGE: &str = "usage: osm-exit-routes --pbf FILE [--pbf FILE ...] [--only from:to;from:to] \
                     [--extract-date YYYY-MM-DD] [--snapshot NAME] [--write]";

/// How far from the exit the signed road may run: a destination sign points
/// at a road the ramp reaches within a few kilometres.
pub const MATCH_M: f64 = 6000.0;

const NOTE_MARK: &str = "Road kind in via derived";

/// One numbered road route, with the lines of its member ways.
pub struct RouteRel {
    pub network: String,
    pub route_ref: String,
    pub name: String,
    pub lines: Vec<Vec<(f64, f64)>>,
}

/// A spoken prefix for the road, and its rank (0 provincial, 1 county or
/// regional). `None` when the relation is a kind no exit sign names.
pub fn road_kind(network: &str, name: &str) -> Option<(&'static str, u8)> {
    match network {
        "CA:QC:A" => return Some(("A-", 0)),
        "CA:QC:R" => return Some(("Route ", 0)),
        "CA:ON:primary" | "CA:ON:secondary" | "CA:ON:tertiary" | "CA:ON:private_toll" => {
            return Some(("Highway ", 0))
        }
        "CA:MB:PTH" => return Some(("PTH ", 0)),
        "CA:MB:PR" => return Some(("PR ", 0)),
        _ => {}
    }
    if !network.starts_with("CA:") {
        return None;
    }
    let name = name.to_lowercase();
    if name.contains("regional road") || name.contains("region road") {
        Some(("Regional Road ", 1))
    } else if name.contains("county road") {
        Some(("County Road ", 1))
    } else if name.contains("district road") {
        Some(("District Road ", 1))
    } else if !name.is_empty() {
        None
    } else {
        unnamed_ontario_kind(network)
    }
}

/// Ontario's upper-tier regions number Regional Roads, Ottawa its Ottawa
/// Roads, and its counties County Roads. A relation with no name says only
/// its network; a single-tier city other than Ottawa (Hamilton, Kingston,
/// Greater Sudbury, Chatham-Kent) and anything below a county (a township
/// line or sideroad) is left to its bare number.
fn unnamed_ontario_kind(network: &str) -> Option<(&'static str, u8)> {
    const REGIONS: &[&str] = &["Durham", "Halton", "Niagara", "Peel", "Waterloo", "York"];
    const CITIES: &[&str] = &[
        "Hamilton",
        "Kingston",
        "Greater Sudbury",
        "Chatham-Kent",
        "Quinte West",
        "Cornwall",
        "Brant",
        "Kawartha Lakes",
        "Haldimand",
        "Norfolk",
        "Prince Edward",
    ];
    let area = network.strip_prefix("CA:ON:")?;
    if area.contains(':') || CITIES.contains(&area) {
        return None;
    }
    Some(if REGIONS.contains(&area) {
        ("Regional Road ", 1)
    } else if area == "Ottawa" {
        ("Ottawa Road ", 1)
    } else {
        ("County Road ", 1)
    })
}

/// "20 Est" to ("20", "East"); `None` when the chunk does not lead with a
/// bare route number.
pub fn bare_chunk(chunk: &str) -> Option<(String, String)> {
    let mut words = chunk.split_whitespace();
    let number = words.next()?;
    let bare = number.chars().next().is_some_and(|c| c.is_ascii_digit())
        && number
            .chars()
            .all(|c| c.is_ascii_digit() || c.is_ascii_uppercase());
    if !bare {
        return None;
    }
    let rest: Vec<String> = words.map(english_cardinal).collect();
    Some((number.to_string(), rest.join(" ")))
}

pub fn english_cardinal(word: &str) -> String {
    // Signs and OSM write them either way: "20 Est", "A-55 nord".
    match word.to_lowercase().as_str() {
        "est" => "East",
        "ouest" => "West",
        "nord" => "North",
        "sud" => "South",
        _ => word,
    }
    .to_string()
}

/// An Ontario emergency detour route ("401 EDR"): a marker for the road to
/// follow when a 400-series highway closes, not a road the exit leads to.
const DETOUR_NOTE: &str = "Emergency detour route refs (EDR) dropped: they mark a \
     detour, not the road the exit leads to";

fn is_detour_ref(chunk: &str) -> bool {
    chunk
        .split_whitespace()
        .any(|w| w.eq_ignore_ascii_case("EDR"))
}

fn distance_m(lat: f64, lon: f64, lines: &[Vec<(f64, f64)>]) -> f64 {
    let coslat = lat.to_radians().cos();
    lines
        .iter()
        .flat_map(|line| line.windows(2))
        .map(|pair| seg_dist_m(lat, lon, pair[0], pair[1], coslat))
        .fold(f64::INFINITY, f64::min)
}

/// The spoken road for a bare `number` signed at (`lat`, `lon`), and the
/// relation's network, or `None` when nothing in reach names it.
pub fn match_road<'a>(
    rels: &'a [RouteRel],
    number: &str,
    lat: f64,
    lon: f64,
) -> Option<(String, &'a RouteRel)> {
    let mut best: Option<(u8, f64, &RouteRel, &'static str)> = None;
    for rel in rels.iter().filter(|r| r.route_ref == number) {
        let Some((prefix, rank)) = road_kind(&rel.network, &rel.name) else {
            continue;
        };
        let d = distance_m(lat, lon, &rel.lines);
        if d > MATCH_M {
            continue;
        }
        if best.is_none_or(|(r, bd, _, _)| (rank, d) < (r, bd)) {
            best = Some((rank, d, rel, prefix));
        }
    }
    best.map(|(_, _, rel, prefix)| (format!("{prefix}{number}"), rel))
}

/// The rewritten `via` and the networks it read, or `None` when nothing in
/// it changes.
pub fn rewrite_via(
    via: &str,
    rels: &[RouteRel],
    lat: f64,
    lon: f64,
) -> Option<(String, Vec<String>)> {
    let mut networks = Vec::new();
    let mut out = Vec::new();
    for chunk in via.split(';') {
        let chunk = chunk.trim();
        if is_detour_ref(chunk) {
            continue;
        }
        let rewritten = match bare_chunk(chunk) {
            Some((number, rest)) => match match_road(rels, &number, lat, lon) {
                Some((road, rel)) => {
                    networks.push(rel.network.clone());
                    [road, rest]
                        .into_iter()
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                }
                None => [number, rest]
                    .into_iter()
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
            },
            None => chunk
                .split_whitespace()
                .map(english_cardinal)
                .collect::<Vec<_>>()
                .join(" "),
        };
        // A bilingual sign lists one road twice ("15 Sud;15 South"); once
        // in English it is the same road, said once.
        if !out.contains(&rewritten) {
            out.push(rewritten);
        }
    }
    let joined = out.join(";");
    (joined != via).then_some((joined, networks))
}

fn read_routes(pbfs: &[std::path::PathBuf]) -> Result<Vec<RouteRel>> {
    let keep = |t: &[(&str, &str)]| {
        let has = |k: &str| t.iter().find(|(tk, _)| *tk == k).map(|(_, v)| *v);
        has("type") == Some("route")
            && has("route") == Some("road")
            && has("network").is_some_and(|n| n.starts_with("CA:"))
            && has("ref").is_some()
    };
    let found = features::read(
        pbfs,
        &Select {
            node: None,
            way: None,
            relation: Some(&keep),
        },
    )?;
    Ok(found
        .relations
        .into_iter()
        .map(|r| RouteRel {
            network: tag(&r.tags, "network").unwrap_or("").to_string(),
            route_ref: tag(&r.tags, "ref").unwrap_or("").trim().to_uppercase(),
            name: tag(&r.tags, "name").unwrap_or("").to_string(),
            lines: r.members.into_iter().map(|(_, coords)| coords).collect(),
        })
        .collect())
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["pbf", "only", "extract-date", "snapshot"],
            flags: &["write"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let pbfs = pbf_paths(&args, USAGE)?;
    let date = super::extract_date(&args, &pbfs)?;
    let snapshot = args.string("snapshot", "OpenStreetMap planet file");
    let only = Only::parse(&args);
    let rels = read_routes(&pbfs)?;
    println!("{} Canadian road route relations", rels.len());
    let mut run = Run::load()?;
    let archive = std::mem::take(&mut run.archive);
    let (mut rewritten, mut left_bare, mut no_archive) = (0, 0, 0);
    let mut by_network: std::collections::BTreeMap<String, usize> = Default::default();
    for leg in run.legs_mut()?.iter_mut() {
        let id = leg_id(leg);
        if !only.wants(&id) {
            continue;
        }
        let Some(line) = archive.get(&id) else {
            no_archive += 1;
            continue;
        };
        let miles = leg_miles(leg);
        let corridor = corridor_mut(leg)?;
        let Some(exits) = corridor
            .get_mut("interchanges")
            .and_then(Value::as_array_mut)
        else {
            continue;
        };
        for exit in exits.iter_mut() {
            let Some(obj) = exit.as_object_mut() else {
                continue;
            };
            let via = obj.get("via").and_then(Value::as_str).unwrap_or("");
            if via.is_empty() {
                continue;
            }
            let at_mi = obj.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
            let (lat, lon) = line.point_at(at_mi, miles);
            match rewrite_via(via, &rels, lat, lon) {
                Some((new_via, networks)) => {
                    rewritten += 1;
                    for n in &networks {
                        *by_network.entry(n.clone()).or_default() += 1;
                    }
                    let detour = via.split(';').any(is_detour_ref);
                    obj.insert("via", Value::from(new_via.clone()));
                    let mut source = obj
                        .get("source")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    if !source.contains(NOTE_MARK) {
                        source = format!(
                            "{source} {NOTE_MARK}: each bare route number matched to the \
                             OpenStreetMap road route relation of that number nearest the exit \
                             (within 6 km), its network or name giving the kind of road, from \
                             the {snapshot} of {date}; French cardinals in English."
                        );
                    }
                    if detour && !source.contains(DETOUR_NOTE) {
                        source = format!("{source} {DETOUR_NOTE}.");
                    }
                    obj.insert("source", Value::from(source.trim().to_string()));
                }
                None => {
                    if via.split(';').any(|c| bare_chunk(c.trim()).is_some()) {
                        left_bare += 1;
                    }
                }
            }
        }
    }
    run.archive = archive;
    println!(
        "{rewritten} exits name their road now, {left_bare} still sign a bare number with no \
         road of it in reach, {no_archive} legs with no archived road kept."
    );
    for (network, n) in &by_network {
        println!("  {network}: {n}");
    }
    run.finish(args.flag("write"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rel(network: &str, number: &str, name: &str, at: (f64, f64)) -> RouteRel {
        RouteRel {
            network: network.into(),
            route_ref: number.into(),
            name: name.into(),
            lines: vec![vec![at, (at.0 + 0.01, at.1)]],
        }
    }

    #[test]
    fn a_quebec_route_and_an_autoroute_get_their_names() {
        let rels = [
            rel("CA:QC:R", "255", "R 255", (45.85, -72.48)),
            rel("CA:QC:A", "20", "Autoroute 20", (45.85, -72.48)),
        ];
        assert_eq!(
            rewrite_via("255", &rels, 45.85, -72.48).unwrap().0,
            "Route 255"
        );
        assert_eq!(
            rewrite_via("20 Est;255", &rels, 45.85, -72.48).unwrap().0,
            "A-20 East;Route 255"
        );
        assert_eq!(
            rewrite_via("20 Est;20 East;255", &rels, 45.85, -72.48)
                .unwrap()
                .0,
            "A-20 East;Route 255"
        );
    }

    #[test]
    fn detour_refs_are_dropped_and_lower_case_directions_read_in_english() {
        let rels = [rel("CA:QC:A", "55", "Autoroute 55", (45.9, -72.5))];
        assert_eq!(
            rewrite_via("Route 161;A-55 nord;A-955", &rels, 45.9, -72.5)
                .unwrap()
                .0,
            "Route 161;A-55 North;A-955"
        );
        assert_eq!(
            rewrite_via("Highway 3;Highway 401 EDR", &rels, 45.9, -72.5)
                .unwrap()
                .0,
            "Highway 3"
        );
        assert_eq!(
            rewrite_via("County Road 11 EDR", &rels, 45.9, -72.5)
                .unwrap()
                .0,
            ""
        );
    }

    #[test]
    fn ontario_prefers_the_highway_and_reads_a_county_road_from_its_name() {
        let rels = [
            rel(
                "CA:ON:Waterloo",
                "8",
                "Waterloo Regional Road 8",
                (43.4, -80.4),
            ),
            rel("CA:ON:primary", "8", "Highway 8", (43.41, -80.4)),
            rel(
                "CA:ON:Lambton",
                "31",
                "Lambton County Road 31",
                (42.9, -82.2),
            ),
            rel("CA:ON:Simcoe:Innisfil:Line", "10", "", (44.3, -79.6)),
            rel("CA:ON:Essex", "50", "", (42.1, -83.0)),
            rel("CA:ON:Hamilton", "20", "", (43.2, -79.9)),
        ];
        assert_eq!(rewrite_via("8", &rels, 43.4, -80.4).unwrap().0, "Highway 8");
        assert_eq!(
            rewrite_via("31 North", &rels, 42.9, -82.2).unwrap().0,
            "County Road 31 North"
        );
        assert_eq!(rewrite_via("10", &rels, 44.3, -79.6), None);
        assert_eq!(
            rewrite_via("50", &rels, 42.1, -83.0).unwrap().0,
            "County Road 50"
        );
        assert_eq!(rewrite_via("20", &rels, 43.2, -79.9), None);
    }

    #[test]
    fn a_road_out_of_reach_or_already_named_is_left_alone() {
        let rels = [rel("CA:QC:R", "117", "R 117", (46.0, -74.0))];
        assert_eq!(rewrite_via("117", &rels, 47.0, -74.0), None);
        assert_eq!(
            rewrite_via("155 Nord", &rels, 47.0, -74.0).unwrap().0,
            "155 North"
        );
        assert_eq!(rewrite_via("US 2;VT 78", &rels, 46.0, -74.0), None);
        assert_eq!(rewrite_via("Route 117", &rels, 46.0, -74.0), None);
    }
}
