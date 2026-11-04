//! Cruise coming back after a hazard stop in a work-zone approach.

use ff_core::sim::trip_models::{TripEvent, TripEventData, TripEventKind, Zone};
use ff_core::sim::trip_route_helpers::zone_key;
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::base::Key;
use freight_fate::states::driving_core::MPH_PER_MPS;

use crate::transcript_cruise_support::*;

fn count(harness: &PlaytestHarness, needle: &str) -> usize {
    spoken(harness)
        .iter()
        .filter(|line| line.to_lowercase().contains(needle))
        .count()
}

/// The zone ahead is a fact about the road, not about one cruise session.
/// A hazard stop in the approach paused cruise and wiped what it had already
/// said; resuming at a crawl put the barrels outside the braking window, and
/// as cruise's target climbed back past the zone's numbers it said "Construction
/// zone ahead, cruise easing to 55" and "to 45" a second time (owner's NVDA
/// drives, I-84 and WI-29, 2026-11-03).
#[test]
fn test_cruise_does_not_repeat_the_zone_easing_after_a_hazard_stop() {
    let mut harness = bench_drive("Zone Resume", 70.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 31.3; // ~70 mph
    });
    press(&mut harness, Key::K, None);
    let barrels = harness.read_drive(|d| d.trip.position_mi) + 1.5;
    let taper = Zone::new(barrels - 1.0, barrels, 55.0, "construction merge");
    let work = Zone::new(barrels, barrels + 3.0, 45.0, "construction");
    let announced = work.clone();
    harness.with_drive(move |d, _| {
        d.trip.zones.push(taper.clone());
        d.trip.zones.push(work.clone());
        d.trip.announced_zone_warnings.insert(zone_key(&announced));
    });
    harness.with_drive(|d, ctx| d.update_cruise(ctx, 0.1, false, false, false));
    assert_eq!(count(&harness, "easing to 55"), 1, "{:?}", spoken(&harness));

    // A hazard brakes the truck to a crawl a mile out.
    harness.with_drive(|d, ctx| {
        d.handle_trip_event(
            ctx,
            &TripEvent {
                kind: TripEventKind::Hazard,
                message: "Brake! Slow car right ahead.".into(),
                data: TripEventData {
                    deadline_s: Some(4.0),
                    ..Default::default()
                },
            },
        )
    });
    assert!(harness.read_drive(|d| d.cruise_mph).is_none());
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 25.0 / MPH_PER_MPS);
    harness.with_drive(|d, ctx| d.clear_hazard(ctx));
    harness.with_drive(|d, ctx| d.resume_speed_control_if_ready(ctx, false));
    // The keeper builds speed and hands back to cruise.
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 30.0 / MPH_PER_MPS);
    harness.with_drive(|d, ctx| d.update_keeper(ctx, 0.1, false, false, false));
    assert!(
        harness.read_drive(|d| d.cruise_mph).is_some(),
        "{:?}",
        spoken(&harness)
    );

    // Back up to speed through the taper and on to the barrels.
    for (speed_mph, to_barrels) in [(40.0, 0.9), (55.0, 0.6), (55.0, 0.2), (46.0, 0.05)] {
        harness.with_drive(move |d, _| {
            d.truck_mut().velocity_mps = speed_mph / MPH_PER_MPS;
            d.trip.position_mi = barrels - to_barrels;
        });
        // Cruise's working target climbs back from the crawl a little each
        // frame; the repeat came once it passed the zone's numbers, about
        // ten seconds after "Cruise resuming".
        for _ in 0..60 {
            harness.with_drive(|d, ctx| d.update_cruise(ctx, 0.25, false, false, false));
            harness.advance_clock(0.25);
        }
    }
    assert_eq!(count(&harness, "easing to 55"), 1, "{:?}", spoken(&harness));
    assert!(
        count(&harness, "easing to 45") <= 1,
        "{:?}",
        spoken(&harness)
    );
}

/// Braking cruise off in the approach and setting it again at a crawl,
/// then dialling it back up, said the zone's easing line a second time
/// (agent drive, WI-29, 2026-11-04). Past the zone, the next one is news.
#[test]
fn test_cruise_set_again_in_a_zone_approach_stays_quiet_and_the_next_zone_speaks() {
    let mut harness = bench_drive("Zone Reset", 70.0, 0.0);
    press(&mut harness, Key::E, None);
    harness.with_drive(|d, _| {
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().velocity_mps = 31.3; // ~70 mph
    });
    press(&mut harness, Key::K, None);
    let barrels = harness.read_drive(|d| d.trip.position_mi) + 0.5;
    let work = Zone::new(barrels, barrels + 1.0, 45.0, "construction");
    let announced = work.clone();
    harness.with_drive(move |d, _| {
        d.trip.zones.push(work.clone());
        d.trip.announced_zone_warnings.insert(zone_key(&announced));
    });
    harness.with_drive(|d, ctx| d.update_cruise(ctx, 0.1, false, false, false));
    assert_eq!(count(&harness, "easing to 45"), 1, "{:?}", spoken(&harness));

    // Brake cruise off, set it again at 26, and dial it back to 70.
    harness.with_drive(|d, ctx| d.cancel_cruise(ctx, false));
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 26.0 / MPH_PER_MPS);
    press(&mut harness, Key::K, None);
    assert!(harness.read_drive(|d| d.cruise_mph).is_some());
    for _ in 0..9 {
        press(&mut harness, Key::Plus, Some('+'));
    }
    for _ in 0..60 {
        harness.with_drive(|d, ctx| d.update_cruise(ctx, 0.25, false, false, false));
        harness.advance_clock(0.25);
    }
    assert_eq!(
        harness.read_drive(|d| d.cruise_mph),
        Some(70.0),
        "{:?}",
        spoken(&harness)
    );
    assert_eq!(count(&harness, "easing to 45"), 1, "{:?}", spoken(&harness));

    // Out the far end and up to speed; a second zone further on is news.
    harness.with_drive(move |d, _| {
        d.trip.position_mi = barrels + 1.2;
        d.truck_mut().velocity_mps = 31.3;
    });
    for _ in 0..60 {
        harness.with_drive(|d, ctx| d.update_cruise(ctx, 0.25, false, false, false));
    }
    let next = Zone::new(barrels + 1.5, barrels + 2.5, 40.0, "construction");
    let announced = next.clone();
    harness.with_drive(move |d, _| {
        d.trip.zones.push(next.clone());
        d.trip.announced_zone_warnings.insert(zone_key(&announced));
    });
    harness.with_drive(|d, ctx| d.update_cruise(ctx, 0.1, false, false, false));
    assert_eq!(count(&harness, "easing to 40"), 1, "{:?}", spoken(&harness));
}
