//! No work zone closes the right lane where an exit is taken.
//!
//! Every exit is taken from the right lane. Player problem report,
//! 2026-11-16: signaled for a truck stop inside a work zone with the right
//! lane coned off, the truck could not get back into the right lane and
//! drove past the only stop before the break was due. A closure over an
//! exit goes on the left side instead (`keep_exits_open`).

use ff_core::sim::trip::{keep_exits_open, Trip, TripOptions};
use ff_core::sim::trip_models::{Zone, EXIT_COMMIT_WINDOW_MI, EXIT_KEEP_RIGHT_MI};
use ff_core::sim::vehicle::TruckState;

use crate::sim_support::{weather, world};

/// A work zone as `place_zones` draws it: the merge taper, then the zone.
fn work_zone(taper_start: f64, at: f64, end: f64, side: Option<&str>) -> Vec<Zone> {
    vec![
        Zone::new(taper_start, at, 55.0, "construction merge").with_closed_side(side),
        Zone::new(at, end, 45.0, "construction").with_closed_side(side),
    ]
}

/// The exit window of a stop at `at`.
fn window(at: f64) -> (f64, f64) {
    (at - EXIT_KEEP_RIGHT_MI, at + EXIT_COMMIT_WINDOW_MI)
}

fn sides(zones: &[Zone]) -> Vec<Option<String>> {
    zones.iter().map(|z| z.closed_side.clone()).collect()
}

fn left() -> Option<String> {
    Some("left".to_string())
}

fn right() -> Option<String> {
    Some("right".to_string())
}

/// A work zone as (taper start, zone start, zone end, closed side).
type Drawn = (f64, f64, f64, Option<&'static str>);

#[test]
fn keep_exits_open_transition_table() {
    // (case, zone (taper start, start, end, side), stop mile, sides after)
    let cases: [(&str, Drawn, f64, Option<String>); 8] = [
        (
            "right closure over a stop",
            (19.0, 20.0, 26.0, Some("right")),
            23.0,
            left(),
        ),
        (
            "right closure clear of every stop",
            (19.0, 20.0, 26.0, Some("right")),
            40.0,
            right(),
        ),
        (
            "left closure untouched",
            (19.0, 20.0, 26.0, Some("left")),
            23.0,
            left(),
        ),
        ("no closure untouched", (19.0, 20.0, 26.0, None), 23.0, None),
        // The window runs 0.4 past the stop, into the taper but not the zone.
        (
            "taper alone in the window",
            (19.0, 20.0, 26.0, Some("right")),
            18.8,
            left(),
        ),
        // The window ends at the stop plus 0.4; the taper starts right there.
        (
            "stop just before the zone",
            (19.0, 20.0, 26.0, Some("right")),
            19.0 - EXIT_COMMIT_WINDOW_MI,
            right(),
        ),
        // The window starts 2 miles before the stop; the zone ends there.
        (
            "stop just past the zone",
            (19.0, 20.0, 26.0, Some("right")),
            26.0 + EXIT_KEEP_RIGHT_MI,
            right(),
        ),
        (
            "stop at the zone's end",
            (19.0, 20.0, 26.0, Some("right")),
            26.0,
            left(),
        ),
    ];
    for (case, (taper, at, end, side), stop, want) in cases {
        let mut zones = work_zone(taper, at, end, side);
        let moved = keep_exits_open(&mut zones, &[window(stop)]);
        assert_eq!(sides(&zones), vec![want.clone(), want.clone()], "{case}");
        let flipped = side == Some("right") && want == left();
        assert_eq!(moved, usize::from(flipped), "{case}");
        // The closed lane follows the side: right is lane 0, left the far
        // lane (lane 1 on a two-lane road before the lane count is known).
        let lane = want.as_deref().map(|side| i64::from(side == "left"));
        for zone in &zones {
            assert_eq!(zone.closed_lane, lane, "{case}");
        }
    }
}

#[test]
fn keep_exits_open_moves_each_work_zone_on_its_own() {
    let mut zones = work_zone(9.0, 10.0, 14.0, Some("right"));
    zones.extend(work_zone(49.0, 50.0, 55.0, Some("right")));
    zones.push(Zone::new(30.0, 34.0, 50.0, "heavy traffic"));
    let moved = keep_exits_open(&mut zones, &[window(12.0)]);
    assert_eq!(moved, 1);
    assert_eq!(
        sides(&zones),
        vec![left(), left(), right(), right(), None],
        "only the zone over the stop moves, taper and zone together"
    );
}

/// Every right closure on `trip` whose footprint covers an exit window.
fn right_closures_over_exits(trip: &Trip) -> Vec<String> {
    let windows = trip.exit_windows();
    let mut found = Vec::new();
    for zone in &trip.zones {
        if !zone.reason.starts_with("construction") || zone.closed_side.as_deref() != Some("right")
        {
            continue;
        }
        if let Some((from, to)) = windows
            .iter()
            .find(|(from, to)| zone.start_mi < *to && zone.end_mi > *from)
        {
            let exit = from + EXIT_KEEP_RIGHT_MI;
            let stop = trip
                .stops
                .iter()
                .find(|s| (s.at_mi - exit).abs() < 1e-9)
                .map_or("the destination exit".to_string(), |s| {
                    format!("{} ({})", s.name, s.stop_type)
                });
            found.push(format!(
                "{} {:.2}-{:.2} over {stop} at {exit:.2} (window to {to:.2})",
                zone.reason, zone.start_mi, zone.end_mi
            ));
        }
    }
    found
}

fn trip_on(start: &str, end: &str, seed: i64) -> Option<Trip> {
    let route = world()
        .route_options(start, end, 1, false)
        .ok()?
        .into_iter()
        .next()?;
    Some(Trip::new(
        route,
        TruckState::default(),
        weather("heartland", seed),
        TripOptions {
            seed: Some(seed),
            world: Some(world()),
            ..Default::default()
        },
    ))
}

#[test]
fn a_known_right_closure_over_a_stop_closes_the_left_lane() {
    // Found by the sweep below on dev's data before the fix: a right closure
    // from mile 428.4 to 436.8 over Love's Travel Stop Sparta (Tennessee) at
    // mile 430.8.
    let trip = trip_on(KNOWN.0, KNOWN.1, KNOWN.2).expect("route");
    assert!(right_closures_over_exits(&trip).is_empty());
    let windows = trip.exit_windows();
    assert!(
        trip.zones.iter().any(|z| z.reason == "construction"
            && z.closed_side.as_deref() == Some("left")
            && windows
                .iter()
                .any(|(from, to)| z.start_mi < *to && z.end_mi > *from)),
        "the work zone over the stop still closes a lane, on the left"
    );
}

const KNOWN: (&str, &str, i64) = ("altoona_pa_us", "troy_al_us", 2);

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "sweep: work zones over a spread of routes and seeds"
)]
fn no_right_closure_covers_an_exit_on_any_route() {
    let names = world().city_names();
    let n = names.len();
    let mut trips = 0usize;
    let mut closures = 0usize;
    let mut failures: Vec<String> = Vec::new();
    for i in (0..n).step_by((n / 40).max(1)) {
        let (start, end) = (&names[i], &names[(i * 37 + 11) % n]);
        if start == end {
            continue;
        }
        for seed in 1..=5 {
            let Some(trip) = trip_on(start, end, seed) else {
                break;
            };
            trips += 1;
            closures += trip
                .zones
                .iter()
                .filter(|z| z.reason == "construction" && z.closed_side.is_some())
                .count();
            for found in right_closures_over_exits(&trip) {
                failures.push(format!("{start} -> {end}, seed {seed}: {found}"));
            }
        }
    }
    println!(
        "{trips} trips, {closures} closures, {} over an exit",
        failures.len()
    );
    assert!(trips > 100, "the sweep built only {trips} trips");
    assert!(
        failures.is_empty(),
        "{} right closures cover an exit.\n{}",
        failures.len(),
        failures.join("\n")
    );
}
