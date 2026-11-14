//! A rest area signed for one side of a divided highway is offered only to a
//! driver on that side (`stop_bounds`).

use ff_core::data::stop_bounds::signed_bound;
use ff_core::data::world::get_world;
use ff_core::data::world_models::Leg;

fn leg(a: &str, b: &str) -> &'static Leg {
    get_world()
        .legs
        .iter()
        .find(|leg| leg.a == a && leg.b == b)
        .unwrap_or_else(|| panic!("no {a} -> {b} leg"))
}

fn offered(leg: &Leg, forward: bool) -> Vec<&str> {
    leg.stops
        .iter()
        .filter(|stop| stop.applies_to_direction(forward))
        .map(|stop| stop.name.as_str())
        .collect()
}

#[test]
fn test_northbound_i43_offers_only_the_northbound_rest_area() {
    // Orinks's drive, 2026-11-04: both were announced three seconds apart.
    let to_green_bay = leg("sheboygan_wi_us", "green_bay_wi_us");
    let north = offered(to_green_bay, true);
    assert!(north.contains(&"Rest Area #52"), "{north:?}");
    assert!(!north.contains(&"Rest Area #51"), "{north:?}");
    let south = offered(to_green_bay, false);
    assert!(south.contains(&"Rest Area #51"), "{south:?}");
    assert!(!south.contains(&"Rest Area #52"), "{south:?}");
}

#[test]
fn test_facing_pairs_split_one_to_each_side() {
    for (a, b, east_or_north, west_or_south) in [
        (
            "madison_wi_us",
            "milwaukee_wi_us",
            "Rest Area #13",
            "Rest Area #14",
        ),
        (
            "peoria_il_us",
            "galesburg_il_us",
            "Spoon River EB",
            "Spoon River WB",
        ),
        (
            "little_rock_ar_us",
            "conway_ar_us",
            "Conway East",
            "Conway West",
        ),
    ] {
        let leg = leg(a, b);
        for forward in [true, false] {
            let names = offered(leg, forward);
            let both = names.contains(&east_or_north) && names.contains(&west_or_south);
            assert!(!both, "{a} -> {b} forward={forward} offers both: {names:?}");
        }
    }
}

#[test]
fn test_almost_every_signed_bound_stop_serves_one_direction() {
    // Calibrated 2026-11-04: 88 of 92 resolve; the four left are named in
    // `stop_bounds`. A rise here means a new import or a changed rule.
    let mut signed = 0;
    let mut unresolved = Vec::new();
    for leg in &get_world().legs {
        for stop in &leg.stops {
            if signed_bound(&stop.source).is_none() {
                continue;
            }
            signed += 1;
            if stop.directions == ["both"] {
                unresolved.push(format!("{} -> {}: {}", leg.a, leg.b, stop.name));
            }
        }
    }
    assert!(signed >= 92, "only {signed} signed-bound stops");
    assert!(unresolved.len() <= 4, "{unresolved:#?}");
}
