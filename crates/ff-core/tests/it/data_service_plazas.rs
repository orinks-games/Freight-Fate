//! New Jersey Turnpike and I-95 Connecticut service plazas sit on the road
//! and the side of it they really serve: a southbound-only plaza is never
//! offered to a northbound driver, and a Merritt Parkway plaza (Route 15
//! bans trucks) is never on an I-95 run.

use ff_core::data::world::get_world;
use ff_core::data::world_models::Leg;

fn leg(a: &str, b: &str) -> &'static Leg {
    get_world()
        .legs
        .iter()
        .find(|leg| leg.a == a && leg.b == b)
        .unwrap_or_else(|| panic!("no {a} -> {b} leg"))
}

/// Stop names a driver is offered running the leg forward or in reverse.
fn offered(leg: &Leg, forward: bool) -> Vec<&str> {
    leg.stops
        .iter()
        .filter(|stop| stop.applies_to_direction(forward))
        .map(|stop| stop.name.as_str())
        .collect()
}

#[test]
fn test_southbound_turnpike_plazas_serve_only_the_southbound_run() {
    let to_philadelphia = leg("newark_nj_us", "philadelphia_pa_us");
    let to_trenton = leg("newark_nj_us", "trenton_nj_us");
    for leg in [to_philadelphia, to_trenton] {
        assert!(offered(leg, true).contains(&"Thomas Edison Service Area"));
        assert!(!offered(leg, false).contains(&"Thomas Edison Service Area"));
        let edison = leg
            .stops
            .iter()
            .find(|stop| stop.name == "Thomas Edison Service Area")
            .expect("on the leg");
        assert_eq!(edison.stop_type, "service_plaza");
        assert!(edison.actions.iter().any(|a| a == "fuel"));
    }
    assert!(offered(to_philadelphia, true).contains(&"Richard Stockton Service Area"));
    assert!(!offered(to_philadelphia, false).contains(&"Richard Stockton Service Area"));
    assert!(offered(to_trenton, true).contains(&"Molly Pitcher Service Area"));
    assert!(!offered(to_trenton, false).contains(&"Molly Pitcher Service Area"));
}

#[test]
fn test_northbound_only_joyce_kilmer_is_off_the_southbound_turnpike_leg() {
    // Joyce Kilmer sits on the northbound roadway only, and no leg runs the
    // Turnpike northbound past it, so the Newark -> Trenton leg drops it.
    let to_trenton = leg("newark_nj_us", "trenton_nj_us");
    assert!(!to_trenton
        .stops
        .iter()
        .any(|stop| stop.name.contains("Joyce Kilmer")));
}

#[test]
fn test_thomas_edison_is_off_the_garden_state_parkway() {
    let parkway = leg("toms_river_nj_us", "newark_nj_us");
    assert!(!parkway
        .stops
        .iter()
        .any(|stop| stop.name.contains("Thomas Edison")));
}

#[test]
fn test_darien_serves_only_southbound_i95() {
    // Legs whose forward run is southbound I-95 through Darien.
    let southbound = [
        leg("hartford_ct_us", "new_york_ny_us"),
        leg("new_haven_ct_us", "new_york_ny_us"),
        leg("providence_ri_us", "new_york_ny_us"),
    ];
    for leg in southbound {
        let darien = leg
            .stops
            .iter()
            .find(|stop| stop.name == "Darien Service Plaza")
            .unwrap_or_else(|| panic!("Darien on {} -> {}", leg.a, leg.b));
        assert_eq!(darien.stop_type, "service_plaza");
        assert_eq!(darien.directions, ["forward"]);
        assert!(darien.actions.iter().any(|a| a == "fuel"));
    }
    // Northbound legs out of New York pass the separate northbound plaza,
    // never the southbound one.
    for leg in [
        leg("new_york_ny_us", "boston_ma_us"),
        leg("new_york_ny_us", "bridgeport_ct_us"),
    ] {
        assert!(
            !leg.stops.iter().any(|stop| stop.name.contains("Darien")),
            "{} -> {}",
            leg.a,
            leg.b
        );
    }
}

#[test]
fn test_no_merritt_parkway_plaza_on_any_leg() {
    for leg in &get_world().legs {
        for stop in &leg.stops {
            assert!(
                !stop.name.contains("New Canaan"),
                "{} on {} -> {}",
                stop.name,
                leg.a,
                leg.b
            );
        }
    }
}
