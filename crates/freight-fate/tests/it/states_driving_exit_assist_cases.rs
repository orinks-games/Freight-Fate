//! Single cases on the exit assist matrix's bench
//! (`states_driving_exit_assist_matrix.rs`): the yield judged where the truck
//! meets the crossroad, and a truck stop's streets from the ramp to its lot.

use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::states::driving_core::RAMP_ACCESS_MI;
use freight_fate::states::driving_rest_states::RestStopState;

use crate::states_driving_exit_assist_matrix::{start, Kind, Preset, DT, STOP_MI};
use crate::transcript_cruise_support::MPS_PER_MPH;

// -- a yield, judged where the truck meets the crossroad ------------------------------

/// Drive the yield bench to its ramp, then put the truck's front at the
/// crossroad -- the yield line plus the MUTCD 3B.19 distance to the near edge
/// -- rolling at 12 mph, with one car on an otherwise empty crossroad whose
/// front reaches the conflict window `car_after_clear_s` seconds after the
/// truck's rear will have cleared it (negative: before). Returns what was said
/// once the crossing was judged.
fn roll_the_yield_with_a_car(car_after_clear_s: f64) -> String {
    use ff_core::sim::cross_traffic::{
        yield_crossing_times_s, CrossTraffic, CrossVehicle, COMBINATION_LENGTH_FT,
        CONFLICT_WINDOW_FT, YIELD_LINE_TO_CROSSROAD_FT,
    };
    let mut harness = start(Preset::None, Kind::Yield);
    for _ in 0..(30 * 60 * 3) {
        if harness.read_drive(|d| d.ramp_mi.is_some()) {
            break;
        }
        frame_plain(&mut harness);
    }
    let past_line_ft = YIELD_LINE_TO_CROSSROAD_FT + 0.5;
    let (_, clear_s) = yield_crossing_times_s(12.0, -past_line_ft, COMBINATION_LENGTH_FT);
    let car_mph = 45.0;
    let car_fps = car_mph * 5280.0 / 3600.0;
    let front_ft = CONFLICT_WINDOW_FT + (clear_s + car_after_clear_s) * car_fps;
    harness.clear_speech();
    harness.with_drive(move |d, ctx| {
        assert!(
            d.truck().trailer_attached,
            "the combination length is the one timed"
        );
        d.ramp_control = "yield".to_string();
        d.ramp_terminal_done = false;
        d.ramp_light_announced = true;
        let mut bubble = CrossTraffic::new(1, "yield", false);
        bubble.vehicles = vec![CrossVehicle {
            position_mi: -(front_ft + 15.0) / 5280.0,
            speed_mph: car_mph,
            target_mph: car_mph,
            vehicle_class: "car",
            length_mi: 15.0 / 5280.0,
            from_side: "left",
            crossed: false,
            committed: false,
            sound_started: false,
            id: 1,
        }];
        d.cross_bubble = Some(bubble);
        d.ramp_mi = Some(RAMP_ACCESS_MI - past_line_ft / 5280.0);
        d.truck_mut().velocity_mps = 12.0 * MPS_PER_MPH;
        d.update_ramp_terminal(ctx);
        assert!(
            d.ramp_terminal_done,
            "the crossing is judged at the crossroad"
        );
    });
    harness.transcript_text()
}

fn frame_plain(harness: &mut PlaytestHarness) {
    harness.advance_clock(DT);
    harness.with_drive(|d, ctx| d.update_frame(ctx, DT));
}

#[test]
fn test_a_gap_clear_at_the_line_and_through_the_crossing_is_clean() {
    // The reported case: the car arrives a second after the truck is across.
    // Judged a hundred feet past the line it read as forced; judged at the
    // crossroad over the truck's own crossing it is a clean gap.
    let heard = roll_the_yield_with_a_car(1.0);
    assert!(heard.contains("Through the yield in a gap"), "{heard}");
    assert!(!heard.contains("forced the gap"), "{heard}");
}

#[test]
fn test_a_gap_that_closes_during_the_crossing_is_forced() {
    let heard = roll_the_yield_with_a_car(-1.5);
    assert!(heard.contains("You forced the gap at the yield"), "{heard}");
}

#[test]
fn test_the_assists_still_roll_a_clear_yield() {
    // Route-transition assistance, alone and with facility stopping
    // assistance, rolls an empty crossroad's yield rather than stopping at it.
    for preset in [Preset::Transition, Preset::All] {
        let mut harness = start(preset, Kind::Yield);
        let mut done = false;
        for _ in 0..(30 * 60 * 6) {
            // Nothing on the crossroad, for the whole run.
            harness.with_drive(|d, _| {
                if let Some(bubble) = d.cross_bubble.as_mut() {
                    bubble.vehicles.clear();
                }
            });
            frame_plain(&mut harness);
            if harness.read_drive(|d| d.ramp_mi.is_some() && d.ramp_terminal_done) {
                done = true;
                break;
            }
        }
        let heard = harness.transcript_text();
        assert!(done, "{preset:?}: never crossed\n{heard}");
        assert!(
            heard.contains("Through the yield in a gap"),
            "{preset:?}\n{heard}"
        );
        assert!(
            !heard.contains("Stopped at the yield"),
            "{preset:?}\n{heard}"
        );
        assert!(!heard.contains("far too fast"), "{preset:?}\n{heard}");
    }
}

// -- a truck stop's streets ------------------------------------------------------------

#[test]
fn test_a_truck_stops_streets_give_the_highway_back_at_the_lot() {
    // The free-flow exit's truck stop has streets from the ramp's end to its
    // lot. They are driven as a trip of their own and, at the lot, the
    // highway comes back where the ramp left it, for the stop to open on.
    let mut harness = start(Preset::None, Kind::FreeFlow5x);
    let mut on_streets = false;
    for _ in 0..(30 * 60 * 6) {
        frame_plain(&mut harness);
        if harness.read_drive(|d| d.stop_chain.is_some()) {
            on_streets = true;
            break;
        }
    }
    assert!(on_streets, "{}", harness.transcript_text());
    let (reasons, road_stop, saved_mi, highway_mi) = harness.with_drive(|d, ctx| {
        let reasons: Vec<String> = d.trip.zones.iter().map(|z| z.reason.clone()).collect();
        let saved = d.snapshot(ctx)["position_mi"].as_f64().unwrap_or(-1.0);
        let highway = d.highway_trip.as_ref().map_or(-1.0, |t| t.position_mi);
        (reasons, d.trip.road_stop, saved, highway)
    });
    assert_eq!(reasons, vec!["access road".to_string(), "lot".to_string()]);
    assert!(road_stop);
    // A save made on these streets is a save at the stop's exit.
    assert_eq!(saved_mi, highway_mi);
    assert!((highway_mi - STOP_MI).abs() < 0.5, "{highway_mi}");
    harness.with_drive(|d, _| {
        let end = d.trip.total_miles();
        d.trip.position_mi = end;
        d.truck_mut().velocity_mps = 0.0;
    });
    frame_plain(&mut harness);
    harness.finish_timed_state();
    assert!(
        harness.state_is::<RestStopState>(),
        "{}",
        harness.transcript_text()
    );
}

#[test]
fn test_a_truck_stops_streets_are_off_in_1_9() {
    // Owner decision, 2026-09-24: a stop's streets are off for 1.9, because
    // most exits have a ramp end baked one way only (the Love's at Baird had
    // streets eastbound and none westbound). The bench stop has streets; a
    // drive with the gate as shipped keeps its entrance at the ramp's end.
    use freight_fate::states::driving_events::chains::STOP_STREETS_IN_PLAY;
    let mut harness = start(Preset::None, Kind::FreeFlow5x);
    assert!(harness.read_drive(|d| d.stop_chain_route(&d.trip.stops[0]).is_some()));
    harness.with_drive(|d, _| d.stop_streets_on = STOP_STREETS_IN_PLAY);
    assert!(harness.read_drive(|d| d.stop_chain_route(&d.trip.stops[0]).is_none()));
    let mut at_the_end = false;
    for _ in 0..(30 * 60 * 6) {
        frame_plain(&mut harness);
        if harness.read_drive(|d| d.stop_chain.is_some()) {
            break;
        }
        if harness.read_drive(|d| d.ramp_mi.is_some() && d.ramp_terminal_done) {
            at_the_end = true;
            break;
        }
    }
    for _ in 0..30 {
        frame_plain(&mut harness);
    }
    let heard = harness.transcript_text();
    assert!(at_the_end, "{heard}");
    assert!(harness.read_drive(|d| d.stop_chain.is_none()), "{heard}");
    assert!(!heard.contains("Off the ramp"), "{heard}");
}
