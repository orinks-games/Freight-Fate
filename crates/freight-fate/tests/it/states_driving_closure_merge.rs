//! Full lane keeping and the work zone that closes the truck's lane: on All
//! assists the truck leaves the closing lane itself, before the taper, and
//! drops back behind traffic that holds the gap shut. It used to ride the
//! closed lane into the barrels: damage, a fine and a serious violation.

use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip_models::Zone;
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::driving::DrivingState;

const DT: f64 = 1.0 / 60.0;

fn run_until(
    harness: &mut PlaytestHarness,
    max_frames: usize,
    mut done: impl FnMut(&PlaytestHarness) -> bool,
) -> bool {
    for _ in 0..max_frames {
        harness.advance_frame_clock();
        harness.app.frame(DT);
        if done(harness) {
            return true;
        }
    }
    false
}

/// The player at the wheel on `lane_keeping`, cruise set, rolling on a
/// quiet road with two lanes or more.
fn cruising(name: &str, lane_keeping: &str) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named(name));
    harness.with_drive(|d, ctx| {
        ctx.settings.lane_keeping = lane_keeping.to_string();
        d.tutorial = None;
        d.departure_checked = true;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.traffic_manager.rolling_bubble = false;
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.traffic_pressures.clear();
        d.trip.zones.clear();
        d.trip.weather.current = WeatherKind::Clear;
        d.trip.set_patrols(Vec::new());
        d.trip.posts.clear();
    });
    harness.prepare_for_driving(55.0);
    harness.with_drive(|d, ctx| d.engage_cruise(ctx, 60.0, false));
    let ok = run_until(&mut harness, 60 * 60 * 30, |h| {
        h.read_drive(|d| {
            d.truck().speed_mph() > 45.0
                && d.ramp_mi.is_none()
                && d.lane.lane_count >= 2
                && d.lane_change_target.is_none()
        })
    });
    assert!(ok, "never got cruising on a multi-lane road");
    harness.with_drive(|d, _| d.trip.zones.clear());
    harness
}

/// A work zone `ahead_mi` on, taper first, closing the truck's own lane.
/// Returns (taper start, zone end).
fn closure_on_the_trucks_lane(d: &mut DrivingState, ahead_mi: f64) -> (f64, f64) {
    let side = if d.lane.lane == 0 { "right" } else { "left" };
    let taper = d.trip.position_mi + ahead_mi;
    let start = taper + 1.0;
    let end = start + 2.0;
    d.trip
        .zones
        .push(Zone::new(taper, start, 55.0, "construction merge").with_closed_side(Some(side)));
    d.trip
        .zones
        .push(Zone::new(start, end, 45.0, "construction").with_closed_side(Some(side)));
    (taper, end)
}

fn assert_clean_through(harness: &PlaytestHarness) {
    let (damage, fines) = harness.read_drive(|d| (d.truck().damage_pct, d.ticket_fines_paid));
    let lines = harness.transcript();
    let tail = lines[lines.len().saturating_sub(20)..].join("\n");
    assert_eq!(damage, 0.0, "the truck took damage:\n{tail}");
    assert_eq!(fines, 0.0, "a ticket was written:\n{tail}");
    assert!(
        !lines.iter().any(|line| line.contains("barrels")),
        "the truck hit the barrels:\n{tail}"
    );
}

#[test]
fn full_lane_keeping_leaves_a_closing_lane_before_the_taper() {
    let mut harness = cruising("Keeping Early Merge", "full");
    let (taper, end) = harness.with_drive(|d, _| closure_on_the_trucks_lane(d, 1.5));
    let closing = harness.read_drive(|d| d.lane.lane);
    assert!(run_until(&mut harness, 60 * 60 * 5, |h| {
        h.read_drive(|d| d.trip.position_mi >= taper)
    }));
    assert_ne!(
        harness.read_drive(|d| d.lane.lane),
        closing,
        "still in the closing lane at the taper"
    );
    assert!(
        harness
            .transcript()
            .iter()
            .any(|line| line.contains("lane for the lane closure.")),
        "the move was never said:\n{}",
        harness.transcript_text()
    );
    assert!(run_until(&mut harness, 60 * 60 * 10, |h| {
        h.read_drive(|d| d.trip.position_mi > end + 0.2)
    }));
    assert_clean_through(&harness);
}

#[test]
fn full_lane_keeping_leaves_a_left_closure_from_the_left_lane() {
    let mut harness = cruising("Keeping Left Closure", "full");
    let (_, end) = harness.with_drive(|d, _| {
        d.lane.lane = d.lane.lane_count - 1;
        closure_on_the_trucks_lane(d, 1.0)
    });
    assert!(run_until(&mut harness, 60 * 60 * 10, |h| {
        h.read_drive(|d| d.trip.position_mi > end + 0.2)
    }));
    assert_clean_through(&harness);
}

#[test]
fn boxed_in_beside_traffic_full_lane_keeping_drops_back_and_merges() {
    let mut harness = cruising("Keeping Boxed Merge", "full");
    let (_, end) = harness.with_drive(|d, _| {
        // A truck keeping pace in the open lane, just behind the cab, holds
        // the gap shut all the way to the barrels unless lane keeping eases
        // off under it.
        let lane = d.lane.lane;
        let open = if lane == 0 { 1 } else { lane - 1 };
        let mph = d.truck().speed_mph();
        let beside = TrafficVehicle::new(
            "keeping:beside",
            d.trip.position_mi - 0.005,
            mph,
            mph,
            open - lane,
            "cruising",
            "truck",
        )
        .with_lane(open);
        d.trip.traffic_manager.vehicles = vec![beside];
        closure_on_the_trucks_lane(d, 1.0)
    });
    assert!(run_until(&mut harness, 60 * 60 * 10, |h| {
        h.read_drive(|d| d.trip.position_mi > end + 0.2)
    }));
    assert_clean_through(&harness);
}

#[test]
fn lane_keeping_never_passes_into_a_lane_the_work_zone_ahead_closes() {
    let mut harness = cruising("Keeping No Pass Into Closure", "full");
    harness.with_drive(|d, _| {
        d.lane.lane = 0;
        let side = "left";
        let taper = d.trip.position_mi + 0.5;
        d.trip.zones.push(
            Zone::new(taper, taper + 1.0, 55.0, "construction merge").with_closed_side(Some(side)),
        );
    });
    let closes = harness.read_drive(|d| d.lane_closes_ahead(d.lane.lane_count - 1));
    assert!(closes, "the left lane should read as closing ahead");
    let stays = harness.read_drive(|d| !d.lane_closes_ahead(0));
    assert!(stays, "the right lane is open through the zone");
}

#[test]
fn balanced_still_leaves_the_merge_to_the_driver() {
    let mut harness = cruising("Balanced Merge", "partial");
    let (taper, _) = harness.with_drive(|d, _| closure_on_the_trucks_lane(d, 1.5));
    let closing = harness.read_drive(|d| d.lane.lane);
    assert!(run_until(&mut harness, 60 * 60 * 5, |h| {
        h.read_drive(|d| d.trip.position_mi >= taper + 0.1)
    }));
    assert_eq!(harness.read_drive(|d| d.lane.lane), closing);
    assert!(
        harness
            .transcript()
            .iter()
            .any(|line| line.contains("closes at the work zone ahead. Move to the")),
        "the driver was never told to move:\n{}",
        harness.transcript_text()
    );
}
