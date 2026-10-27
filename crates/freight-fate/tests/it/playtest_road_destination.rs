//! `--playtest-road --find destination`: does the finder land the truck at
//! the delivery off-ramp, with the approach still ahead of it?
//!
//! A finder that compiles but drops you in the wrong place is worse than no
//! finder: the owner drives, hears nothing, and reads that as the fix not
//! working. So placement is proved on the transcript -- from the mile the
//! tool chooses, driving forward produces the destination-exit call, and
//! driving past it produces the loop-back.

use ff_core::data::world::get_world;

use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::playtest::road::destination::destination_lead_mi;
use freight_fate::playtest::road::{
    build_trip, find_feature, first_world_pair, sampled_world_pairs, Hit, RoadOptions, FEATURES,
};
use freight_fate::states::driving_core::DESTINATION_EXIT_SCAN_WINDOW_MI;

const MPH_PER_MPS: f64 = 2.23694;
const DT: f64 = 1.0 / 60.0;
/// The seed both the search and the drive run on, so a failure here is the
/// same road every time.
const TRIP_SEED: i64 = 20260826;

fn destination_options() -> RoadOptions {
    RoadOptions {
        feature: "destination".to_string(),
        trip_seed: Some(TRIP_SEED),
        // The driver must be able to blow the exit. With lane keeping on
        // full the truck takes it for them and there is no miss to test.
        lane_keeping: Some("off".to_string()),
        ..Default::default()
    }
}

fn find_one(origin: &str, destination: &str) -> Option<Hit> {
    let opts = destination_options();
    let pairs = vec![(origin.to_string(), destination.to_string())];
    find_feature(get_world(), &pairs, "destination", &opts, Some(TRIP_SEED))
        .into_iter()
        .next()
}

/// How many corridors the map-wide sweeps below sample.
///
/// The sweeps used to resolve every ordered city pair. That was affordable on
/// a small map and is not on this one, so they take a spread sample instead
/// -- wide enough that a placement rule broken in one region still shows up,
/// bounded so the cost does not grow with the next batch of cities.
const SWEEP_PAIRS: usize = 24;

fn world_pair() -> (String, String) {
    first_world_pair(get_world()).expect("current world has a supported route")
}

// -- the finder ------------------------------------------------------------------------

#[test]
fn destination_is_one_of_the_named_features() {
    assert!(FEATURES.contains(&"destination"));
    // The count is the list's own guard: a feature added to `find_feature`
    // and forgotten here is a feature no operator can discover.
    assert_eq!(FEATURES.len(), 12);
}

#[test]
fn every_supported_route_offers_its_delivery_exit() {
    let world = get_world();
    let opts = destination_options();
    let pairs = sampled_world_pairs(world, SWEEP_PAIRS);
    let hits = find_feature(world, &pairs, "destination", &opts, Some(TRIP_SEED));
    assert_eq!(
        hits.len(),
        pairs.len(),
        "every supported route has a delivery exit; got {hits:#?}"
    );
    for hit in &hits {
        assert!(
            hit.at_mi > 0.0 && hit.at_mi < hit.total_mi,
            "{} -> {} put the exit at {:.1} of {:.0}",
            hit.origin,
            hit.destination,
            hit.at_mi,
            hit.total_mi
        );
        // The drive only ever crowns an interchange inside its final
        // approach window; a hit outside it would be pointing somewhere the
        // game does not call the destination exit.
        assert!(
            hit.total_mi - hit.at_mi <= DESTINATION_EXIT_SCAN_WINDOW_MI,
            "{} -> {} put the exit {:.1} mi from the end",
            hit.origin,
            hit.destination,
            hit.total_mi - hit.at_mi
        );
        assert!(hit.label.starts_with("destination exit"), "{}", hit.label);
    }
    // Signed exits sort ahead of the synthetic end-of-route one, the way an
    // open weigh station sorts ahead of a dark one.
    let signed: Vec<bool> = hits.iter().map(|hit| hit.magnitude > 0.0).collect();
    let first_unsigned = signed.iter().position(|s| !s).unwrap_or(signed.len());
    assert!(
        signed[first_unsigned..].iter().all(|s| !s),
        "signed and unsigned exits are interleaved: {signed:?}"
    );
}

#[test]
fn every_discovered_delivery_has_a_destination_hit() {
    let (origin, destination) = world_pair();
    let hit = find_one(&origin, &destination).expect("a supported delivery has a destination exit");
    assert!(hit.at_mi > 0.0 && hit.at_mi < hit.total_mi);
    assert!(
        hit.total_mi - hit.at_mi <= DESTINATION_EXIT_SCAN_WINDOW_MI,
        "{:.1} of {:.0}",
        hit.at_mi,
        hit.total_mi
    );
}

#[test]
fn the_lead_clears_the_callout_window() {
    let world = get_world();
    let opts = destination_options();
    for (origin, destination) in sampled_world_pairs(world, SWEEP_PAIRS) {
        let trip = build_trip(world, &origin, &destination, Some(TRIP_SEED))
            .expect("the discovered route routes");
        let lead = destination_lead_mi(&trip, opts.speed);
        // The drive announces one exit window out; the lead has to be
        // strictly longer or the call fires before the wheel is handed over.
        // Five miles is the window's own floor, so nothing can be shorter.
        assert!(
            lead > 5.0,
            "{origin} -> {destination} lead is only {lead:.1} mi"
        );
    }
}

// -- the drive -------------------------------------------------------------------------

/// Roll forward at a pinned speed, running the REAL frame, until `stop` says
/// so. Returns whether it stopped for that reason rather than running out.
fn roll(
    harness: &mut PlaytestHarness,
    speed_mph: f64,
    frames: usize,
    mut stop: impl FnMut(&mut PlaytestHarness) -> bool,
) -> bool {
    for _ in 0..frames {
        harness.advance_frame_clock();
        harness.with_drive(|drive, ctx| {
            let (tank, cut_out) = {
                let specs = &drive.truck().specs;
                (specs.fuel_tank_gal, specs.air_governor_cut_out_psi)
            };
            drive.truck_mut().fuel_gal = tank;
            drive.truck_mut().set_air_pressure_psi(cut_out);
            drive.truck_mut().parking_brake = false;
            drive.truck_mut().grade = 0.0;
            // Pinned rather than driven: this test is about WHERE the lines
            // land, and a truck that slows for a hill covers a different
            // stretch of road every time the map changes.
            drive.truck_mut().velocity_mps = speed_mph / MPH_PER_MPS;
            drive.update_frame(ctx, DT);
        });
        if stop(harness) {
            return true;
        }
    }
    false
}

fn said(harness: &PlaytestHarness, needle: &str) -> bool {
    harness
        .transcript()
        .iter()
        .any(|line| line.to_lowercase().contains(needle))
}

/// Start where the finder says, drive forward, and read what was heard.
fn drive_the_finder(origin: &str, destination: &str) -> (PlaytestHarness, Hit, f64) {
    let opts = destination_options();
    let hit = find_one(origin, destination)
        .unwrap_or_else(|| panic!("no delivery exit found for {origin} -> {destination}"));
    let mut harness = PlaytestHarness::new();
    let start_mi = harness.start_road_feature(&hit, &opts);
    harness.with_drive(|drive, _| {
        drive.tutorial = None;
        drive.trip.set_patrols(Vec::new());
        drive.truck_mut().transmission.automatic = true;
    });
    (harness, hit, start_mi)
}

#[test]
fn a_staged_drive_does_not_replay_the_road_behind_it() {
    // Staged at mile 1175 of Aberdeen to Abilene, the first frame spoke every
    // state line from South Dakota on, charged a turnpike toll and earned two
    // achievements for road nobody drove (agent drives, 2026-09-23).
    let opts = destination_options();
    let hit = find_one("Aberdeen", "Abilene").expect("the Abilene delivery exit");
    let mut harness = PlaytestHarness::new();
    let start_mi = harness.start_road_feature(&hit, &opts);
    assert!(start_mi > 1000.0, "staged at {start_mi:.0}");
    harness.clear_speech();
    harness.drive_frames(30);
    let text = harness.transcript_text();
    assert!(!text.contains("Crossing into"), "{text}");
    assert!(!text.contains("toll"), "{text}");
    assert!(!text.contains("trooper has somebody"), "{text}");
    harness.read_drive(|d| {
        assert!(d.trip.toll_charges.is_empty());
        // Posts wholly behind the start are heard-and-passed without a sound.
        for post in &d.trip.posts {
            if post.at_mi + post.reach_mi < start_mi {
                assert!(d.marked_post_ids.contains(&post.id()));
                assert!(!post.announced, "a post behind the start made a noise");
            }
        }
    });
}

#[test]
fn the_finder_starts_before_the_destination_exit_call_and_reaches_it() {
    let (origin, destination) = world_pair();
    let (mut harness, hit, start_mi) = drive_the_finder(&origin, &destination);
    assert!(
        start_mi < hit.at_mi,
        "started at {start_mi:.1}, exit at {:.1}",
        hit.at_mi
    );

    // Nothing has been said about the exit yet: the whole point of the lead
    // is that the driver hears the approach OPEN rather than arriving in the
    // middle of it.
    let announced_at_start = harness.read_drive(|d| !d.destination_exit_announced_key.is_empty());
    assert!(!announced_at_start, "{}", harness.transcript_text());
    assert!(!said(&harness, "destination exit"));

    let reached = roll(&mut harness, 62.0, 200_000, |h| {
        h.read_drive(|d| !d.destination_exit_announced_key.is_empty())
    });
    assert!(
        reached,
        "the drive never reached the destination-exit call\n{}",
        harness.transcript_text()
    );
    assert!(
        said(&harness, "destination exit"),
        "{}",
        harness.transcript_text()
    );
    // The line names the exit and how far off it is, not a bare heading.
    assert!(
        said(&harness, "in ") && said(&harness, "destination exit"),
        "{}",
        harness.transcript_text()
    );
}

#[test]
fn blowing_past_the_exit_loops_back() {
    let (origin, destination) = world_pair();
    let (mut harness, _hit, _start_mi) = drive_the_finder(&origin, &destination);
    // Never press X: the exit goes by unanswered, which is the case both
    // reported defects were driven from.
    let looped = roll(&mut harness, 62.0, 400_000, |h| {
        h.read_drive(|d| d.missed_destination_exit_said)
    });
    assert!(
        looped,
        "driving past the exit never produced the loop-back\n{}",
        harness.transcript_text()
    );
    assert!(
        said(&harness, "destination exit"),
        "the call was skipped entirely\n{}",
        harness.transcript_text()
    );
    assert!(
        said(&harness, "turned around"),
        "the miss did not name the loop-back\n{}",
        harness.transcript_text()
    );
    // The loop-back puts the exit ahead again rather than stranding the trip
    // at the end of the route.
    let (position, total) = harness.read_drive(|d| (d.trip.position_mi, d.trip.total_miles()));
    assert!(
        position < total - 1.0,
        "the loop-back left the truck at {position:.1} of {total:.0}"
    );
}

#[test]
fn the_miss_names_the_destination_not_its_map_key() {
    let (origin, destination) = world_pair();
    let (mut harness, hit, _start_mi) = drive_the_finder(&origin, &destination);
    let looped = roll(&mut harness, 55.0, 400_000, |h| {
        h.read_drive(|d| d.missed_destination_exit_said)
    });
    assert!(
        looped,
        "the discovered approach never missed its exit\n{}",
        harness.transcript_text()
    );
    let text = harness.transcript_text();
    assert!(!text.contains("_us"), "the map key was spoken:\n{text}");
    assert!(
        text.to_lowercase()
            .contains(&hit.destination.to_lowercase()),
        "the destination was never named:\n{text}"
    );
}
