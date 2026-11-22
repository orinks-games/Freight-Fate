//! The Canadian legs as built by the `cargo tools` pipeline (the BC and
//! Yukon pilot, then each corridor): each carries its own archived road, a grade profile, read
//! speed postings, read province lines and curves from the Canadian
//! gameplay folder.

use crate::data_support::{data_dir, world};
use ff_core::data::curves::leg_curves;
use ff_core::data::world_models::Leg;

/// Every leg that starts or ends in Canada: the BC and Yukon legs rebuilt
/// from scaffolds and every corridor built since.
fn canadian_legs() -> Vec<String> {
    world()
        .legs
        .iter()
        .filter(|l| l.a.ends_with("_ca") || l.b.ends_with("_ca"))
        .map(|l| format!("{}:{}", l.a, l.b))
        .collect()
}

fn geometry_files(country: &str) -> Vec<String> {
    let dir = data_dir().join(format!("world_data/{country}/geometry"));
    let mut files: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".jsonl"))
        .map(|n| format!("world_data/{country}/geometry/{n}"))
        .collect();
    files.sort();
    files
}

fn leg(id: &str) -> &'static Leg {
    let (a, b) = id.split_once(':').unwrap();
    world()
        .legs
        .iter()
        .find(|l| l.a == a && l.b == b)
        .unwrap_or_else(|| panic!("{id} is not in the world"))
}

fn archived_ids(path: &str) -> Vec<String> {
    let text = std::fs::read_to_string(data_dir().join(path)).unwrap();
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|r| r.get("leg").and_then(|v| v.as_str()).map(String::from))
        .collect()
}

#[test]
fn every_canadian_leg_has_an_archived_road_in_its_countrys_folder() {
    let ca = geometry_files("ca");
    let mut ids: Vec<String> = Vec::new();
    for path in ca.iter().chain(geometry_files("us").iter()) {
        ids.extend(archived_ids(path));
    }
    let legs = canadian_legs();
    assert!(legs.len() > 100, "only {} Canadian legs", legs.len());
    for id in &legs {
        assert!(ids.iter().any(|i| i == id), "{id} has no archived road");
    }
    for path in &ca {
        for id in archived_ids(path) {
            let from = id.split(':').next().unwrap_or_default();
            assert!(
                from.ends_with("_ca"),
                "{id} starts in the US but is filed in {path}"
            );
        }
    }
}

#[test]
fn every_canadian_leg_has_a_grade_profile_that_covers_it() {
    for id in canadian_legs() {
        let leg = leg(&id);
        let segments = leg.grade_segments();
        assert!(!segments.is_empty(), "{id} has no grade profile");
        let end = segments.iter().map(|s| s.end_mi).fold(0.0, f64::max);
        assert!(
            end >= leg.miles * 0.9,
            "{id}: grades end at mile {end:.1} of {:.1}",
            leg.miles
        );
    }
}

#[test]
fn curves_load_from_the_canadian_gameplay_folder() {
    assert!(!leg_curves("dawson_creek_bc_ca:fort_st_john_bc_ca", true).is_empty());
    assert!(!leg_curves("fort_nelson_bc_ca:watson_lake_yt_ca", true).is_empty());
}

#[test]
fn province_and_border_lines_are_read_with_their_names() {
    let yukon = leg("fort_nelson_bc_ca:watson_lake_yt_ca");
    assert!(
        yukon.state_crossings().iter().any(|c| c.state == "Yukon"),
        "{:?}",
        yukon.state_crossings()
    );
    let border = leg("haines_junction_yt_ca:tok_ak_us");
    assert!(
        border
            .state_crossings()
            .iter()
            .any(|c| c.from_state == "Yukon" && c.state == "Alaska"),
        "{:?}",
        border.state_crossings()
    );
    let miles: f64 = border.state_miles().iter().map(|s| s.miles).sum();
    assert!(
        (miles - border.miles).abs() <= 1.0,
        "state miles {miles} of {}",
        border.miles
    );
}

#[test]
fn canadian_postings_are_read_or_marked_as_gaps() {
    let mut read = 0;
    for id in canadian_legs() {
        for s in leg(&id).speed_limits() {
            assert!(
                !s.source.starts_with("Coarse "),
                "{id} still carries a coarse posting at mile {}",
                s.at_mi
            );
            if s.mph.is_some() && !s.source.to_ascii_lowercase().contains("assumed") {
                read += 1;
            }
        }
    }
    assert!(read > 100, "only {read} read postings on the Canadian legs");
}

#[test]
fn prairie_province_lines_are_read_with_their_names() {
    let line = leg("lloydminster_ab_ca:north_battleford_sk_ca");
    assert!(
        line.state_crossings()
            .iter()
            .any(|c| c.from_state == "Alberta" && c.state == "Saskatchewan"),
        "{:?}",
        line.state_crossings()
    );
    let border = leg("winnipeg_mb_ca:grand_forks_nd_us");
    assert!(
        border
            .state_crossings()
            .iter()
            .any(|c| c.from_state == "Manitoba" && c.state == "North Dakota"),
        "{:?}",
        border.state_crossings()
    );
}

#[test]
fn the_lower_post_weave_reads_as_one_entry_into_yukon() {
    let weave = leg("fort_nelson_bc_ca:watson_lake_yt_ca");
    let lines: Vec<(f64, &str, &str)> = weave
        .state_crossings()
        .iter()
        .map(|c| (c.at_mi, c.state.as_str(), c.place.as_str()))
        .collect();
    assert_eq!(lines, vec![(311.7, "Yukon", "Lower Post")]);
    let miles: Vec<(&str, f64)> = weave
        .state_miles()
        .iter()
        .map(|m| (m.state.as_str(), m.miles))
        .collect();
    assert_eq!(miles[0], ("British Columbia", 311.7));
    assert_eq!(miles.len(), 2);
    // A true in-and-out of British Columbia is left alone.
    assert_eq!(
        leg("watson_lake_yt_ca:whitehorse_yt_ca")
            .state_crossings()
            .len(),
        2
    );
}

#[test]
fn the_weave_screen_changes_no_us_leg() {
    let world = world();
    let mut checked = 0;
    for path in std::fs::read_dir(data_dir().join("world_data/us/legs")).unwrap() {
        let text = std::fs::read_to_string(path.unwrap().path()).unwrap();
        let shard: serde_json::Value =
            serde_json::from_str(text.trim_start_matches('\u{feff}')).unwrap();
        for raw in shard["legs"].as_array().unwrap() {
            let (a, b) = (raw["from"].as_str().unwrap(), raw["to"].as_str().unwrap());
            let us = [a, b]
                .iter()
                .all(|s| world.cities.get(*s).is_some_and(|c| c.country == "US"));
            if !us {
                continue;
            }
            let stored = raw["corridor"]["state_crossings"]
                .as_array()
                .map_or(0, Vec::len);
            let leg = world.legs.iter().find(|l| l.a == a && l.b == b).unwrap();
            assert_eq!(leg.state_crossings().len(), stored, "{a} to {b}");
            checked += 1;
        }
    }
    assert!(checked > 1000, "only {checked} US legs checked");
}

#[test]
fn canadian_postings_are_driven_at_the_number_on_the_sign() {
    // Fort Nelson to Watson Lake is Canadian end to end: every read posting
    // is a whole km/h sign.
    let mut signs = 0;
    for s in leg("fort_nelson_bc_ca:watson_lake_yt_ca").speed_limits() {
        let Some(mph) = s.mph else { continue };
        let kmh = mph * ff_core::units::MILES_TO_KM;
        assert!(
            (kmh - kmh.round()).abs() < 1e-6,
            "mile {}: {kmh} km/h",
            s.at_mi
        );
        assert_eq!(kmh.round() % 10.0, 0.0, "mile {}: {kmh} km/h", s.at_mi);
        signs += 1;
    }
    assert!(signs > 5);
    // Alaska's side of the border stays in mph.
    let tok = leg("haines_junction_yt_ca:tok_ak_us");
    let alaska: Vec<f64> = tok
        .speed_limits()
        .iter()
        .filter(|s| s.at_mi > 202.0)
        .filter_map(|s| s.mph)
        .collect();
    assert!(!alaska.is_empty());
    assert!(alaska.iter().all(|mph| mph % 5.0 == 0.0), "{alaska:?}");
}

#[test]
fn a_canadian_ramp_advisory_keeps_its_km_h() {
    // A 50 km/h ramp sign is 31.07 mph, not the 50 mph the US bare-number
    // screen would make of it.
    let leg = leg("vancouver_bc_ca:abbotsford_bc_ca");
    let advisories: Vec<f64> = leg
        .interchanges()
        .iter()
        .flat_map(|x| [x.ramp_advisory_mph_forward, x.ramp_advisory_mph_backward])
        .flatten()
        .collect();
    assert!(!advisories.is_empty());
    assert!(advisories.iter().all(|mph| *mph < 40.0), "{advisories:?}");
}

#[test]
fn a_loaded_truck_is_offered_fuel_between_fort_nelson_and_watson_lake() {
    for id in [
        "fort_nelson_bc_ca:watson_lake_yt_ca",
        "watson_lake_yt_ca:fort_nelson_bc_ca",
    ] {
        let fuel: Vec<&str> = leg(id)
            .stops
            .iter()
            .filter(|s| s.actions.iter().any(|a| a == "fuel") && s.accessible_to(false))
            .map(|s| s.name.as_str())
            .collect();
        assert!(
            fuel.contains(&"Toad River Lodge") && fuel.contains(&"Liard Hot Springs Lodge"),
            "{id}: {fuel:?}"
        );
        // Each says how its truck access was decided.
        for s in leg(id)
            .stops
            .iter()
            .filter(|s| fuel.contains(&s.name.as_str()))
        {
            assert!(
                s.vehicle_access_basis == "read" || s.vehicle_access_basis == "assumed",
                "{id}: {} has no access basis",
                s.name
            );
        }
    }
}

fn exit_phrase(id: &str, exit_ref: &str) -> String {
    leg(id)
        .interchanges()
        .iter()
        .find(|x| x.exit_ref == exit_ref)
        .unwrap_or_else(|| panic!("{id} has no exit {exit_ref}"))
        .spoken_phrase()
}

#[test]
fn ontario_and_quebec_exits_name_the_road_they_sign() {
    // OpenStreetMap signs these by bare number; the sign says the road.
    let quebec = exit_phrase("trois_rivieres_qc_ca:drummondville_qc_ca", "185");
    assert!(
        quebec.starts_with("exit 185 for Route 255 toward"),
        "{quebec}"
    );
    let ontario = exit_phrase("kitchener_on_ca:mississauga_on_ca", "278");
    assert!(
        ontario.starts_with("exit 278 for Highway 8 toward"),
        "{ontario}"
    );
}

#[test]
fn quebec_exits_give_their_directions_in_english() {
    for l in world()
        .legs
        .iter()
        .filter(|l| l.a.ends_with("_qc_ca") || l.b.ends_with("_qc_ca"))
    {
        for x in l.interchanges() {
            let french = x
                .via
                .split(|c: char| c == ';' || c.is_whitespace())
                .any(|w| matches!(w.to_lowercase().as_str(), "est" | "ouest" | "nord" | "sud"));
            assert!(!french, "{} to {} exit {}: {}", l.a, l.b, x.exit_ref, x.via);
        }
    }
}

#[test]
fn ontario_exits_do_not_name_an_emergency_detour_as_their_road() {
    // "Highway 401 EDR" marks the detour to take when the 401 closes; the
    // exit leads to Highway 3.
    for l in world()
        .legs
        .iter()
        .filter(|l| l.a.ends_with("_on_ca") || l.b.ends_with("_on_ca"))
    {
        for x in l.interchanges() {
            let detour = x
                .via
                .split_whitespace()
                .any(|w| w.eq_ignore_ascii_case("EDR"));
            assert!(!detour, "{} to {} exit {}: {}", l.a, l.b, x.exit_ref, x.via);
        }
    }
}

#[test]
fn every_long_western_canada_leg_has_fuel_a_loaded_truck_can_use() {
    // The West corridor: a leg over 200 miles that starts or ends in
    // British Columbia, Alberta, Saskatchewan or Manitoba.
    let west = |city: &str| {
        ["_bc_ca", "_ab_ca", "_sk_ca", "_mb_ca"]
            .iter()
            .any(|p| city.ends_with(p))
    };
    let dry: Vec<String> = world()
        .legs
        .iter()
        .filter(|l| l.miles > 200.0 && (west(&l.a) || west(&l.b)))
        .filter(|l| {
            !l.stops
                .iter()
                .any(|s| s.actions.iter().any(|a| a == "fuel") && s.accessible_to(false))
        })
        .map(|l| format!("{}:{} ({} mi)", l.a, l.b, l.miles))
        .collect();
    assert!(dry.is_empty(), "no loaded-truck fuel on {dry:#?}");
}

const PROVINCES: &[&str] = &[
    "Alberta",
    "British Columbia",
    "Manitoba",
    "New Brunswick",
    "Newfoundland and Labrador",
    "Northwest Territories",
    "Nova Scotia",
    "Nunavut",
    "Ontario",
    "Prince Edward Island",
    "Quebec",
    "Saskatchewan",
    "Yukon",
];

/// A km/h sign: the stored mph is the sign's own number over 1.609344.
fn is_kmh_sign(mph: f64) -> bool {
    let kmh = mph * 1.609_344;
    (kmh / 5.0 - (kmh / 5.0).round()).abs() * 5.0 < 0.05 && (mph - mph.round()).abs() > 0.01
}

/// Past a country line the limit in force is that country's sign
/// (2026-11-22): north of Coutts the port's short postings were dropped and
/// Montana's 70 mph held into Alberta, spoken as 113 km/h. Just past each
/// line, a mile and a half past it, and at every posting on the Canadian
/// side, the limit is a posted km/h number; on the United States side it is a
/// whole mph. An unposted stretch (a gap) is the region's default and passes.
#[test]
fn past_a_country_line_the_limit_is_that_countrys_sign() {
    let mut bad = Vec::new();
    for leg in &world().legs {
        let crossings = leg.state_crossings();
        let limits = leg.speed_limits();
        let in_force = |mi: f64| {
            limits
                .iter()
                .rev()
                .find(|s| s.at_mi <= mi)
                .and_then(|s| s.mph)
        };
        for (i, c) in crossings.iter().enumerate() {
            let into_canada = PROVINCES.contains(&c.state.as_str());
            let from_canada = PROVINCES.contains(&c.from_state.as_str());
            if into_canada == from_canada {
                continue;
            }
            let end = crossings.get(i + 1).map_or(leg.miles, |n| n.at_mi);
            let probe = (c.at_mi + 1.5).min(end);
            let mut points = vec![(c.at_mi + 0.05).min(end), probe];
            if into_canada {
                points.extend(
                    limits
                        .iter()
                        .filter(|s| s.at_mi > probe && s.at_mi < end)
                        .map(|s| s.at_mi),
                );
            }
            for mi in points {
                let Some(mph) = in_force(mi) else { continue };
                let ok = if into_canada {
                    is_kmh_sign(mph)
                } else {
                    (mph - mph.round()).abs() < 0.01
                };
                if !ok {
                    bad.push(format!(
                        "{}:{} at {mi:.1} ({} side): {mph:.2} mph",
                        leg.a, leg.b, c.state
                    ));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} places: {bad:#?}", bad.len());
}

#[test]
fn canadian_booths_say_the_canadian_price_and_bill_us_dollars() {
    // Orinks, 2026-11-22: a Canadian booth says the Canadian price. The
    // settlement keeps the one currency the career runs in.
    let toll = &leg("sarnia_on_ca:port_huron_mi_us").toll_events()[0];
    assert_eq!(toll.canadian_amount(), Some(35.0));
    assert!((toll.amount - 25.04).abs() < 0.005, "{}", toll.amount);
    let line = ff_core::speech_text::toll_charged(
        &toll.method_label(),
        &toll.name,
        &ff_core::speech_text::toll_amount_text(toll),
        toll.estimated,
    );
    assert_eq!(
        line.normal,
        "Cash or card toll at Blue Water Bridge, 35 Canadian dollars, billed to carrier settlement."
    );
    // The US-side booth into Canada still posts US dollars.
    let back = &leg("port_huron_mi_us:sarnia_on_ca").toll_events()[0];
    assert_eq!(back.canadian_amount(), None);
    assert_eq!(ff_core::speech_text::toll_amount_text(back), "26");
}
