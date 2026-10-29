//! Mexico freight v1: the Laredo border transfer yards (`border_yards.json`).
//!
//! The yards are real US-side facilities with a published address and a
//! source; the Mexican consignee cities are text on a load and never a city,
//! route or destination on the map; no border toll is ever charged.

use crate::data_support::{data_dir, supported, world};
use ff_core::data::border_yards::{parse_border_yards, BorderYards};
use ff_core::models::border_freight::{border_yard, paperwork_minutes, southbound_consignee};
use ff_core::models::jobs::{JobBoard, OfferOptions};
use ff_core::models::profile::Profile;
use ff_core::models::trailer_yard::pickup_plan;

const LAREDO: &str = "laredo_tx_us";

fn yards() -> BorderYards {
    let text = std::fs::read_to_string(data_dir().join("border_yards.json")).expect("data file");
    parse_border_yards(&text).expect("border_yards.json validates")
}

fn miles_between(a: (f64, f64), b: (f64, f64)) -> f64 {
    let (lat1, lon1) = (a.0.to_radians(), a.1.to_radians());
    let (lat2, lon2) = (b.0.to_radians(), b.1.to_radians());
    let h = ((lat2 - lat1) / 2.0).sin().powi(2)
        + lat1.cos() * lat2.cos() * ((lon2 - lon1) / 2.0).sin().powi(2);
    2.0 * 3958.8 * h.sqrt().asin()
}

#[test]
fn every_yard_and_consignee_carries_a_source_and_the_yards_sit_in_laredo() {
    let data = yards();
    assert_eq!(data.city, LAREDO);
    assert_eq!(data.yards.len(), 2);
    let laredo = world().city(LAREDO).expect("Laredo is a map city");
    for yard in &data.yards {
        assert!(yard.source.starts_with("https://"), "{}", yard.id);
        assert!(
            !yard.address.is_empty() && yard.address.contains("Laredo, TX"),
            "{}",
            yard.id
        );
        assert!(data.area.contains(yard.lat, yard.lon), "{}", yard.id);
        let miles = miles_between((laredo.lat, laredo.lon), (yard.lat, yard.lon));
        assert!(miles < 15.0, "{} is {miles:.1} miles from Laredo", yard.id);
    }
    assert_eq!(data.consignees.len(), 2);
    for consignee in &data.consignees {
        assert!(!consignee.source.trim().is_empty(), "{}", consignee.spoken);
    }
    assert!(data.bridge.source.starts_with("https://www.cbp.gov/"));
}

#[test]
fn spoken_border_text_is_plain_ascii_and_never_claims_a_crossing_or_a_toll() {
    let data = yards();
    let lines = [
        &data.text.load_card,
        &data.text.yard_drop,
        &data.text.northbound_wait,
        &data.text.northbound_wait_after_hours,
        &data.text.no_load_ready,
    ];
    for line in lines {
        assert!(line.is_ascii(), "{line}");
        let lower = line.to_lowercase();
        for banned in [
            "peso",
            "toll",
            "carta porte",
            "c-tpat",
            "in-bond",
            "in bond",
            "dps",
        ] {
            assert!(!lower.contains(banned), "{banned} in {line}");
        }
    }
    let spoken: Vec<&str> = data.consignees.iter().map(|c| c.spoken.as_str()).collect();
    assert_eq!(spoken, ["Monterrey, Nuevo Leon", "Saltillo, Coahuila"]);
    // The accented form is kept as data for a later switch, never spoken.
    assert_eq!(data.consignees[0].name, "Monterrey, Nuevo Le\u{f3}n");
}

#[test]
fn the_yards_are_laredo_freight_facilities() {
    let data = yards();
    let laredo = world().city(LAREDO).expect("Laredo");
    for yard in &data.yards {
        let location = laredo
            .locations
            .iter()
            .find(|l| l.id == yard.id)
            .unwrap_or_else(|| panic!("{} is not a Laredo facility", yard.id));
        assert_eq!(location.name, yard.name);
        assert_eq!(location.facility_type, "cross_dock");
        assert!(location.roles.iter().any(|r| r == "shipper"));
        assert!(location.roles.iter().any(|r| r == "receiver"));
        assert!(location.source_note.contains(&yard.source), "{}", yard.id);
        assert_eq!((location.lat, location.lon), (yard.lat, yard.lon));
        assert!(world().facility_by_id(&yard.id).is_ok());
    }
    // The yards add to Laredo's freight; the market's own cross-dock stays.
    assert!(laredo
        .locations
        .iter()
        .any(|l| l.id == "laredo-tx-us:cross_dock:laredo-cross-dock"));
}

#[test]
fn no_mexican_city_is_on_the_map_or_routable() {
    let w = world();
    assert!(w.cities.values().all(|c| c.country != "MX"));
    for name in [
        "Monterrey",
        "Saltillo",
        "Nuevo Laredo",
        "Monterrey, Nuevo Leon",
    ] {
        assert!(w.city(name).is_err(), "{name} resolved to a map city");
        let route = w.shortest_route(LAREDO, name, None, false);
        assert!(!matches!(route, Ok(Some(_))), "{name} is routable");
    }
    for consignee in &yards().consignees {
        let city = consignee.spoken.split(',').next().unwrap_or_default();
        assert!(
            w.cities.values().all(|c| c.name != city),
            "{city} became a map city"
        );
    }
}

#[test]
fn southbound_loads_are_delivered_to_a_yard_and_name_the_mexican_consignee() {
    let w = world();
    let data = yards();
    let mut found = None;
    for seed in 0..400 {
        let mut board = JobBoard::seeded(w, seed);
        let Some(job) = board.offer_to(
            "san_antonio_tx_us",
            LAREDO,
            &[] as &[&str],
            OfferOptions::level(3),
        ) else {
            continue;
        };
        if border_yard(&job.destination_facility_id).is_some() {
            found = Some(job);
            break;
        }
    }
    let job = found.expect("a San Antonio board sends a load to a Laredo yard");
    assert_eq!(job.destination, LAREDO);
    let consignee = southbound_consignee(&job).expect("a consignee");
    let yard = data.yard(&job.destination_facility_id).expect("a yard");
    let card = job.describe_plain();
    assert!(
        card.contains(&format!(
            "for consignee {}, delivered to {} in Laredo",
            consignee.spoken, yard.spoken
        )),
        "{card}"
    );
    assert!(card.is_ascii(), "{card}");
    // The consignee is keyed on the load, so the same load always names it.
    assert_eq!(southbound_consignee(&job.clone()), Some(consignee));
}

#[test]
fn northbound_loads_leave_a_yard_for_a_us_city_after_a_paperwork_wait() {
    let w = world();
    let mut found = None;
    'seeds: for seed in 0..200 {
        let mut board = JobBoard::seeded(w, seed);
        for job in board.offers(LAREDO, &[] as &[&str], OfferOptions::level(3)) {
            if border_yard(&job.origin_facility_id).is_some() {
                found = Some(job);
                break 'seeds;
            }
        }
    }
    let job = found.expect("a Laredo board offers a load out of a yard");
    let destination = w.city(&job.destination).expect("a map city");
    assert_eq!(destination.country, "US");
    let minutes = paperwork_minutes(&job);
    assert!((30.0..=90.0).contains(&minutes), "{minutes}");
    let plan = pickup_plan(&job, &Profile::named_in("Border Yard", "Laredo"));
    assert_eq!(plan.paperwork_minutes, minutes);
    // Southbound text never rides a northbound load.
    assert!(southbound_consignee(&job).is_none());
    assert!(!job.describe_plain().contains("consignee"));
}

#[test]
fn the_laredo_lanes_charge_no_toll() {
    // Nothing at the border is a toll the player pays: the transfer carrier
    // crosses the bridge, and the roads into Laredo carry no toll either.
    let w = world();
    for (a, b) in [("san_antonio_tx_us", LAREDO), (LAREDO, "san_antonio_tx_us")] {
        let route = supported(w, a, b);
        assert!(route.toll_events().is_empty(), "{a} -> {b} charges a toll");
    }
}
