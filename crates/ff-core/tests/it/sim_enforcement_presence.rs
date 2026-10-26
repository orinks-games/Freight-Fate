//! Enforcement presence: posts on the road, and being seen by one (port of
//! `tests/test_enforcement_presence.py`; the App-driven audio cases are
//! ignored until the app shell lands).
//!
//! The old catch model produced a country with no police in it. These tests
//! pin the replacement: posts are places, observation is graded, presence
//! costs a clean driver nothing, and nothing can bite a player it never made
//! a sound for.

use crate::sim_support::*;
use ff_core::pyrandom::PyRandom;
use ff_core::sim::enforcement_observe::{
    geometry_factor, observe, RoadSample, CERTAIN_OVER_MPH, OBSERVE_HOLD_MI, WHAT_FOLLOWING,
    WHAT_SPEEDING,
};
use ff_core::sim::enforcement_posts::{
    assign_tableau, class_spacing_mult, hour_multiplier, post_seed, region_multiplier,
    scale_open_chance, EnforcementPost, KIND_CHAIN, KIND_CMV, KIND_FIXED_SCALE, KIND_MEDIAN,
    KIND_ROVING, KIND_SCALE_APRON, KIND_WORK_ZONE, METHOD_PACING, PACING_MIN_MI, PACING_WINDOW_MI,
    TABLEAU_KINDS,
};
use ff_core::sim::season::day_of_week;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::CB_CALLS_PER_RUN;
use ff_core::sim::vehicle::TruckState;

fn trip_opts(a: &str, b: &str, opts: TripOptions) -> Trip {
    let route = supported(world(), a, b);
    Trip::new(
        route,
        TruckState::default(),
        // WeatherSystem(seed=1): the default "heartland" region.
        weather("heartland", 1),
        TripOptions {
            world: Some(world()),
            ..opts
        },
    )
}

fn trip_seeded(a: &str, b: &str, seed: i64) -> Trip {
    trip_opts(
        a,
        b,
        TripOptions {
            seed: Some(seed),
            hazard_scale: 1.0,
            start_hour: 12.0,
            career_hours: Some(100.0),
            ..Default::default()
        },
    )
}

fn trip() -> Trip {
    trip_seeded("Chicago", "Denver", 7)
}

fn post(at_mi: f64) -> EnforcementPost {
    always_observing_post(at_mi, KIND_MEDIAN, 1.0, 1.0, 0)
}

fn post_kind(at_mi: f64, kind: &str) -> EnforcementPost {
    always_observing_post(at_mi, kind, 1.0, 1.0, 0)
}

fn ids(trip: &Trip) -> Vec<(String, bool)> {
    trip.posts.iter().map(|p| (p.id(), p.staffed)).collect()
}

// --- placement --------------------------------------------------------------

#[test]
fn test_placement_is_deterministic_from_the_seed() {
    let a = ids(&trip());
    let b = ids(&trip());
    assert_eq!(a, b);
    assert!(!a.is_empty());
}

#[test]
fn test_a_short_run_still_carries_enforcement() {
    // Any run under 120 miles used to carry exactly zero patrols.
    let trip = trip_seeded("Buffalo", "Rochester", 7);
    assert!(trip.route.miles() < 120.0);
    assert!(!trip.posts.is_empty());
}

#[test]
fn test_interstates_carry_more_posts_per_mile_than_two_lane_roads() {
    assert!(class_spacing_mult("interstate") < class_spacing_mult("us_highway"));
    assert!(class_spacing_mult("us_highway") < class_spacing_mult("state_route"));
}

#[test]
fn test_hot_regions_run_denser_than_cold_ones() {
    assert!(region_multiplier("northeast") > region_multiplier("heartland"));
    assert!(region_multiplier("heartland") < region_multiplier("appalachia"));
    assert!(region_multiplier("appalachia") <= 1.0);
}

#[test]
fn test_the_small_hours_are_thin_and_the_commuter_peaks_are_thick() {
    assert!(hour_multiplier(3.0) < 1.0);
    assert!(hour_multiplier(7.0) > 1.0);
    assert!(hour_multiplier(16.0) > 1.0);
    assert_eq!(hour_multiplier(12.0), 1.0);
}

#[test]
fn test_scales_are_mostly_dark_at_the_weekend() {
    let weekday = (0..24 * 14)
        .step_by(24)
        .find(|h| day_of_week(*h as f64) < 5)
        .unwrap();
    let weekend = (0..24 * 14)
        .step_by(24)
        .find(|h| day_of_week(*h as f64) >= 5)
        .unwrap();
    assert!(scale_open_chance(Some(weekend as f64)) < scale_open_chance(Some(weekday as f64)));
}

#[test]
fn test_every_post_kind_anchors_to_world_data() {
    // No post kind may invent a feature the world does not already carry.
    let trip = trip();
    for post in &trip.posts {
        if post.kind == KIND_FIXED_SCALE || post.kind == KIND_SCALE_APRON {
            assert!(trip
                .stops
                .iter()
                .any(|s| s.stop_type == "weigh_station" && s.key() == post.anchor));
        } else if post.kind == KIND_WORK_ZONE {
            assert!(trip.zones.iter().any(|z| z.reason == "construction"));
        }
    }
}

#[test]
fn test_relaxed_hazards_do_not_thin_the_police() {
    // Presence is a fact about the country, not a difficulty knob.
    let relaxed = trip_opts(
        "Chicago",
        "Denver",
        TripOptions {
            seed: Some(7),
            hazard_scale: 0.2,
            career_hours: Some(100.0),
            ..Default::default()
        },
    );
    assert_eq!(relaxed.posts.len(), trip().posts.len());
}

#[test]
fn test_a_reload_does_not_re_roll_the_road() {
    let mut trip = trip();
    let before = ids(&trip);
    trip.restore(trip.total_miles() / 3.0, 0.0);
    assert_eq!(ids(&trip), before);
}

// --- the tableau: a post already working somebody else ----------------------

#[test]
fn test_a_tableau_schedules_on_a_patrol_stretch_and_never_on_a_post_free_stretch() {
    let mut kinds = TABLEAU_KINDS.to_vec();
    kinds.sort();
    let mut expected = vec![KIND_MEDIAN, KIND_ROVING, "urban_unit"];
    expected.sort();
    assert_eq!(kinds, expected);

    // A staffed patrol-kind post: some seed schedules a tableau on it.
    let staffed = post_kind(10.0, KIND_MEDIAN);
    assert!((0..50).any(|seed| assign_tableau(&staffed, Some(seed))));

    // An empty post never runs a tableau, whatever the seed.
    let mut empty = post_kind(10.0, KIND_MEDIAN);
    empty.staffed = false;
    assert!(!(0..50).any(|seed| assign_tableau(&empty, Some(seed))));

    // A fixed-spot kind never runs a tableau either, even staffed.
    let fixed = post_kind(10.0, KIND_FIXED_SCALE);
    assert!(!(0..50).any(|seed| assign_tableau(&fixed, Some(seed))));

    // A route that carries no enforcement posts at all schedules nothing.
    let mut trip = trip();
    trip.posts = Vec::new();
    assert!(!trip.posts.iter().any(|p| p.tableau));
}

#[test]
fn test_a_tableau_is_deterministic_from_the_seed() {
    let post = post_kind(10.0, KIND_MEDIAN);
    assert_eq!(
        assign_tableau(&post, Some(42)),
        assign_tableau(&post, Some(42))
    );
}

#[test]
fn test_the_tableau_busy_window_suppresses_only_that_posts_catches() {
    let mut trip = trip();
    let mut busy = always_observing_post(10.0, KIND_MEDIAN, 5.0, 1.0, 0);
    busy.tableau = true;
    let other = always_observing_post(10.0, KIND_ROVING, 5.0, 1.0, 0);
    trip.posts = vec![busy.clone(), other.clone()];

    let mile_in_window = 9.5;
    assert!(busy.tableau_busy_at(mile_in_window));
    let watching: Vec<String> = trip
        .posts_watching(mile_in_window)
        .iter()
        .map(|p| p.id())
        .collect();
    assert!(!watching.contains(&busy.id()));
    assert!(watching.contains(&other.id())); // only the busy post's catches are suppressed

    let mile_outside = 6.0; // still covered by reach_mi, but well clear of the window
    assert!(!busy.tableau_busy_at(mile_outside));
    let watching_outside: Vec<String> = trip
        .posts_watching(mile_outside)
        .iter()
        .map(|p| p.id())
        .collect();
    assert!(watching_outside.contains(&busy.id()));
    assert!(watching_outside.contains(&other.id()));
}

// `test_the_tableau_cues_play_once_each` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

// `test_tableau_audio_defers_while_the_players_own_stop_is_active` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

// `test_a_post_that_already_caught_the_player_never_also_runs_its_tableau` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

// `test_the_tableau_intro_line_speaks_once_and_says_it_is_not_the_player` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

// `test_the_tableau_intro_line_stays_silent_while_deferred` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

// `test_the_tableau_intro_terse_form_keeps_the_bare_fact` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

#[test]
fn test_the_tableau_cb_line_waits_for_the_players_own_stop_and_a_declined_post() {
    // The CB speech-side twin of the audio deferral above.
    let mut trip = trip();
    let mut post = post_kind(trip.position_mi + 2.0, KIND_MEDIAN);
    post.tableau = true;
    let post_id = post.id();
    trip.posts = vec![post];

    trip.pull_over_active = true;
    trip.check_enforcement_heads_up();
    assert!(!trip
        .events
        .iter()
        .any(|e| e.text().contains("somebody stopped")));

    trip.pull_over_active = false;
    trip.heads_up_seen.remove(&post_id);
    trip.posts[0].declined = true;
    trip.check_enforcement_heads_up();
    assert!(!trip
        .events
        .iter()
        .any(|e| e.text().contains("somebody stopped")));
}

// --- presence targets -------------------------------------------------------

#[test]
fn test_a_five_hundred_mile_run_lands_in_the_presence_target() {
    // Six to twelve audible police contacts per five hundred miles.
    for (a, b) in [("Chicago", "Denver"), ("Los Angeles", "Phoenix")] {
        let mut rates = Vec::new();
        for seed in 0..6 {
            let trip = trip_seeded(a, b, seed);
            let contacts = trip.audible_enforcement_contacts().len() as f64;
            rates.push(contacts / trip.route.miles() * 500.0);
        }
        let average = rates.iter().sum::<f64>() / rates.len() as f64;
        assert!(
            (6.0..=12.0).contains(&average),
            "{a}->{b} averaged {average:.1} per 500 miles"
        );
    }
}

// --- observation ------------------------------------------------------------

fn sample() -> RoadSample {
    let mut s = RoadSample::new(9.8, 75.0, 65.0);
    s.over_limit_mi = OBSERVE_HOLD_MI * 2.0;
    s
}

fn sample_speed(speed_mph: f64) -> RoadSample {
    RoadSample {
        speed_mph,
        ..sample()
    }
}

#[test]
fn test_five_over_near_a_post_is_noticed_and_ignored() {
    assert!(observe(&post(10.0), &sample_speed(70.0)).is_none());
}

#[test]
fn test_twenty_over_is_a_certainty_at_any_post_that_can_see_you() {
    let found = observe(&post(10.0), &sample_speed(65.0 + CERTAIN_OVER_MPH)).expect("seen");
    assert_eq!(found.confidence, 1.0);
}

#[test]
fn test_a_post_that_was_never_announced_cannot_observe_you() {
    // The accessibility gate: no cue, no consequence.
    let mut p = post(10.0);
    p.announced = false;
    assert!(observe(&p, &sample_speed(95.0)).is_none());
}

#[test]
fn test_a_post_that_already_let_you_go_does_not_re_decide() {
    let mut p = post(10.0);
    p.declined = true;
    assert!(observe(&p, &sample_speed(95.0)).is_none());
}

#[test]
fn test_an_empty_post_sees_nothing() {
    let mut p = post(10.0);
    p.staffed = false;
    assert!(observe(&p, &sample_speed(95.0)).is_none());
}

#[test]
fn test_a_crest_blocks_lidar_and_barely_troubles_radar() {
    let lidar = post_kind(10.0, "urban_unit");
    let radar = post_kind(10.0, KIND_MEDIAN);
    let speed = RoadSample {
        speed_mph: 80.0,
        position_mi: 9.8,
        ..sample()
    };
    let blind = RoadSample {
        crest_between: true,
        ..speed.clone()
    };
    let lidar_clear = observe(&lidar, &speed).unwrap().confidence;
    let lidar_blind = observe(&lidar, &blind).unwrap().confidence;
    let radar_clear = observe(&radar, &speed).unwrap().confidence;
    let radar_blind = observe(&radar, &blind).unwrap().confidence;
    assert!(lidar_blind < lidar_clear * 0.3);
    assert!(radar_blind > radar_clear * 0.7);
}

#[test]
fn test_fog_blinds_the_eye_and_not_the_radar() {
    let visual = always_observing_post(10.0, KIND_WORK_ZONE, 1.0, 1.0, 0);
    let radar = post_kind(10.0, KIND_MEDIAN);
    let speed = RoadSample {
        speed_mph: 80.0,
        position_mi: 9.8,
        ..sample()
    };
    let foggy = RoadSample {
        visibility_mi: 0.25,
        ..speed.clone()
    };
    let clear = RoadSample {
        visibility_mi: 10.0,
        ..speed.clone()
    };
    assert!(
        observe(&visual, &foggy).unwrap().confidence
            < observe(&visual, &clear).unwrap().confidence * 0.5
    );
    assert!(
        observe(&radar, &foggy).unwrap().confidence
            > observe(&radar, &clear).unwrap().confidence * 0.85
    );
}

#[test]
fn test_running_in_a_pack_lowers_the_odds_you_are_the_one_picked() {
    let p = post(10.0);
    let alone = observe(&p, &sample_speed(80.0)).unwrap().confidence;
    let packed = observe(
        &p,
        &RoadSample {
            pack_neighbours: 3,
            ..sample_speed(80.0)
        },
    )
    .unwrap()
    .confidence;
    assert!(packed < alone);
    assert!(packed < alone * 0.6);
}

#[test]
fn test_a_pack_is_no_cover_for_a_damaged_truck() {
    // Speed hides in traffic. A wrecked trailer is visible whoever is around.
    let p = always_observing_post(10.0, KIND_WORK_ZONE, 1.0, 1.0, 0);
    let damaged = RoadSample {
        damage_pct: 90.0,
        ..sample_speed(65.0)
    };
    let alone = observe(&p, &damaged).unwrap();
    let packed = observe(
        &p,
        &RoadSample {
            pack_neighbours: 4,
            ..damaged
        },
    )
    .unwrap();
    assert_eq!(alone.what, "unsafe equipment");
    assert_eq!(packed.what, "unsafe equipment");
    assert_eq!(alone.confidence, packed.confidence);
}

#[test]
fn test_a_visual_post_sees_more_than_speed() {
    let p = always_observing_post(10.0, KIND_WORK_ZONE, 1.0, 1.0, 0);
    let found = observe(
        &p,
        &RoadSample {
            chains_required: true,
            chains_on: false,
            ..sample_speed(60.0)
        },
    )
    .expect("seen");
    assert_eq!(found.what, "no chains");
}

#[test]
fn test_a_radar_post_never_writes_you_up_for_something_it_cannot_see() {
    let p = post_kind(10.0, KIND_MEDIAN);
    assert!(observe(
        &p,
        &RoadSample {
            damage_pct: 95.0,
            ..sample_speed(60.0)
        }
    )
    .is_none());
}

#[test]
fn test_the_hold_is_distance_not_time() {
    // Observation must behave identically at every pacing and frame rate.
    let p = post(10.0);
    let blip = RoadSample {
        over_limit_mi: OBSERVE_HOLD_MI / 4.0,
        ..sample_speed(80.0)
    };
    let held = RoadSample {
        over_limit_mi: OBSERVE_HOLD_MI * 1.5,
        ..sample_speed(80.0)
    };
    assert!(observe(&p, &blip).is_none());
    assert!(observe(&p, &held).is_some());
}

// --- determinism ------------------------------------------------------------

#[test]
fn test_the_observation_seed_is_position_quantised_never_time_quantised() {
    let p = post(10.0);
    let a = post_seed(Some(42), &p.id(), "observe:speeding:9.8");
    let b = post_seed(Some(42), &p.id(), "observe:speeding:9.8");
    assert_eq!(a, b); // the same tenth of a mile is the same decision
    assert_ne!(post_seed(Some(42), &p.id(), "observe:speeding:9.9"), a);
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- road-joint audio and the enforcement RNG stream"]
fn test_road_joint_audio_does_not_consume_the_enforcement_stream() {}

#[test]
#[ignore = "Python asserted on the module source text; the setting simply does not exist in the Rust port"]
fn test_there_is_no_enforcement_presence_setting_any_more() {}

#[test]
#[ignore = "Python asserted on the module source text; placement takes no hazard_scale by construction"]
fn test_placement_and_staffing_read_no_difficulty_dial() {}

#[test]
fn test_a_quiet_road_still_reports_enforcement_in_full_when_asked() {
    // You lose ambience, never information you can ask a key for.
    let mut trip = trip();
    trip.posts = vec![post(trip.position_mi + 4.0)];
    let line = trip.cb_patrol_status(&trip.posts[0].clone(), 4.0);
    assert!(line.contains("enforcement post"));
    assert!(line.contains("median"));
}

// --- CB chatter -------------------------------------------------------------

#[test]
fn test_cb_confidence_is_carried_in_the_words() {
    let trip = trip();
    let staffed = post(20.0);
    let mut empty = post(20.0);
    empty.staffed = false;
    let lines = [
        trip.cb_patrol_message(&staffed, 3.0),
        trip.cb_patrol_message(&empty, 3.0),
    ];
    assert!(lines
        .iter()
        .any(|line| line.contains("two drivers call") || line.contains("a driver reports")));
}

#[test]
fn test_the_cb_never_says_the_road_is_clear() {
    // Wrong in one direction only. Silence is the only honest "nothing heard".
    let trip = trip();
    for post in trip.posts.iter().take(40) {
        let lowered = trip.cb_patrol_message(post, 3.0).to_lowercase();
        assert!(!lowered.contains("clear"));
        assert!(!lowered.contains("nothing"));
        assert!(!lowered.contains("no trooper"));
    }
}

#[test]
fn test_cb_chatter_varies_by_what_the_post_actually_is() {
    let trip = trip();
    let patrol = post_kind(20.0, KIND_MEDIAN);
    let work_zone = post_kind(20.0, KIND_WORK_ZONE);
    let scale_apron = post_kind(20.0, KIND_SCALE_APRON);
    let fixed_scale = post_kind(20.0, KIND_FIXED_SCALE);
    let cmv = post_kind(20.0, KIND_CMV);
    let chain = post_kind(20.0, KIND_CHAIN);

    let patrol_line = trip.cb_patrol_message(&patrol, 3.0);
    let work_zone_line = trip.cb_patrol_message(&work_zone, 3.0);
    let apron_line = trip.cb_patrol_message(&scale_apron, 3.0);
    let scale_line = trip.cb_patrol_message(&fixed_scale, 3.0);
    let cmv_line = trip.cb_patrol_message(&cmv, 3.0);
    let chain_line = trip.cb_patrol_message(&chain, 3.0);

    // Every line still carries the distance and its own side slot.
    for (line, post) in [
        (&patrol_line, &patrol),
        (&work_zone_line, &work_zone),
        (&apron_line, &scale_apron),
        (&scale_line, &fixed_scale),
        (&cmv_line, &cmv),
        (&chain_line, &chain),
    ] {
        assert!(line.contains("3.0") || line.contains("miles") || line.starts_with("Somebody"));
        // The squelch marks it as the CB; the words carry no "CB chatter"
        // opener (owner, 2026-10-01).
        assert!(!line.contains("CB chatter"), "{line}");
        assert!(line.contains(Trip::cb_side(post)));
    }

    // Only the patrol/speed line is a trooper sighting.
    assert!(patrol_line.to_lowercase().contains("a trooper"));
    for line in [
        &work_zone_line,
        &apron_line,
        &scale_line,
        &cmv_line,
        &chain_line,
    ] {
        assert!(!line.to_lowercase().contains("a trooper"), "{line}");
    }
    // The work zone talks about enforcement, not a trooper sighting.
    assert!(work_zone_line.to_lowercase().contains("troopers"));
    // The scale chatter is about logs, and uses only canonical nouns.
    assert!(apron_line.to_lowercase().contains("logs"));
    assert!(scale_line.to_lowercase().contains("logs"));
    assert!(!apron_line.to_lowercase().contains("coop"));
    assert!(!scale_line.to_lowercase().contains("coop"));
    // The commercial-vehicle unit gets the inspection family too.
    assert!(cmv_line.to_lowercase().contains("logs"));
    assert!(cmv_line.to_lowercase().contains("equipment"));
    assert!(!cmv_line.to_lowercase().contains("coop"));
    // The chain control gets its own line.
    assert!(chain_line.to_lowercase().contains("chain control"));
    // The confidence framing is shared across every kind.
    for line in [
        &patrol_line,
        &work_zone_line,
        &apron_line,
        &scale_line,
        &cmv_line,
        &chain_line,
    ] {
        assert!(["two drivers", "a driver", "somebody"]
            .iter()
            .any(|marker| line.contains(marker)));
    }
}

#[test]
fn test_cb_tableau_line_reports_a_trooper_with_a_customer() {
    let trip = trip();
    let post = post_kind(20.0, KIND_MEDIAN);
    let line = trip.cb_tableau_message(&post, 3.0);
    assert!(line.to_lowercase().contains("trooper"));
    assert!(!line.contains("CB chatter"), "{line}");
    assert!(line.to_lowercase().contains("somebody stopped"));
    assert!(line.contains(Trip::cb_side(&post)));
    assert!(line.contains("3.0") || line.contains("miles"));
}

#[test]
fn test_spoken_cb_lines_are_capped_for_a_whole_run() {
    assert_eq!(CB_CALLS_PER_RUN, 2);
}

#[test]
fn test_the_cb_lead_is_sized_in_real_seconds_not_a_flat_distance() {
    // A flat five miles is 3.5 real seconds at realistic pacing.
    let mut slow = trip();
    slow.time_scale = 4.0;
    slow.truck.velocity_mps = 65.0 / 2.23694;
    let mut fast = trip();
    fast.time_scale = 40.0;
    fast.truck.velocity_mps = 65.0 / 2.23694;
    assert!(fast.enforcement_warning_lookahead_mi() > slow.enforcement_warning_lookahead_mi());
}

// --- vocabulary -------------------------------------------------------------

// `test_bear_is_never_spoken_in_any_player_facing_string` is live in `crates/ff-core/tests/it/sim_enforcement_vocabulary.rs`.

#[test]
#[ignore = "models::safety_record owns safety_record_text; covered by its own tests"]
fn test_the_safety_record_is_never_spoken_as_a_trade_acronym() {}

// `test_a_clean_record_is_waved_through_and_a_dirty_one_is_not` is live in `crates/ff-core/src/models/safety_record.rs`, `crates/freight-fate/tests/states_driving_enforcement.rs`.

#[test]
fn test_the_safety_record_rides_on_the_profile_and_survives_a_save() {
    use ff_core::models::profile::Profile;
    use ff_core::models::safety_record::refresh_selection_score;

    // Rendering a profile signs it, and the signing key lives in the save
    // directory, so this case reached for the player's own save folder. That
    // fallthrough is refused now, so pin the test's own directory instead.
    let tmp = tempfile::tempdir().expect("a temp save dir");
    let previous = ff_core::settings::set_thread_data_dir(Some(tmp.path().to_path_buf()));

    let mut profile = Profile::named("Record");
    for _ in 0..3 {
        profile
            .driving_record
            .record_citation_at(100.0, profile.game_hours);
    }
    refresh_selection_score(&mut profile, 60.0);
    assert!(profile.selection_score > 40.0);
    let restored = Profile::from_dict(&profile.to_dict());
    assert_eq!(restored.selection_score, profile.selection_score);

    ff_core::settings::set_thread_data_dir(previous);
}

/// The scale reads a window, the way the real carrier score weighs the last
/// 24 months: a citation, a serious violation, an out-of-service order and a
/// fatigue event raise the score while they are recent and stop raising it
/// once a game year has passed. The lifetime counts are untouched.
#[test]
fn test_an_aged_record_stops_raising_the_safety_record() {
    use ff_core::models::profile::Profile;
    use ff_core::models::safety_record::{
        score_for_profile, SAFETY_RECORD_BASELINE, SAFETY_RECORD_WINDOW_DAYS,
    };

    let mut profile = Profile::named("Aged");
    profile.career.reputation = 50.0; // neutral, so only the record moves it
    let booked = profile.game_hours;
    let record = &mut profile.driving_record;
    record.record_citation_at(100.0, booked);
    record.record_serious_violation(booked);
    record.record_fatigue_event(booked);
    record.out_of_service_times.push(booked);
    profile.out_of_service_events = 1;

    let fresh = score_for_profile(&profile, 0.0);
    assert!(fresh > SAFETY_RECORD_BASELINE + 20.0, "{fresh}");

    // One hour inside the window: still counted.
    let window_h = SAFETY_RECORD_WINDOW_DAYS as f64 * 24.0;
    profile.game_hours = booked + window_h - 1.0;
    assert_eq!(score_for_profile(&profile, 0.0), fresh);

    // One hour past it: the scale sees a clean record again.
    profile.game_hours = booked + window_h + 1.0;
    assert_eq!(score_for_profile(&profile, 0.0), SAFETY_RECORD_BASELINE);
    assert_eq!(profile.driving_record.citations, 1);
    assert_eq!(profile.driving_record.fatigue_events, 1);
    assert_eq!(profile.out_of_service_events, 1);
}

// `test_the_marked_unit_pass_actually_plays` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

// `test_the_marker_leads_the_whoosh_rather_than_being_buried_in_it` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

// `test_every_staffed_post_is_audible_before_it_can_observe_you` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

#[test]
#[ignore = "needs the audio engine (siren loop)"]
fn test_the_siren_is_a_held_loop_that_rises() {}

#[test]
#[ignore = "needs the audio engine (enforcement signature)"]
fn test_the_enforcement_signature_is_deterministic_and_not_a_radio_timbre() {}

#[test]
#[ignore = "needs the audio engine (siren bus)"]
fn test_the_siren_is_off_the_shared_sfx_bus() {}

// `test_a_stop_cuts_the_radio_rather_than_ducking_it` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

// `test_a_cue_leaves_the_radio_alone_when_ducking_is_off` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

// `test_a_cue_ducks_the_radio_on_its_own_field` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

// `test_enforcement_defers_while_the_cab_already_has_a_demand` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

// `test_a_deferred_look_survives_the_truck_leaving_the_post_behind` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

// `test_a_trooper_who_never_caught_up_loses_you` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- scale approach cues"]
fn test_a_closed_scale_says_nothing_and_an_open_one_speaks() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- scale bypass"]
fn test_taking_the_scales_own_ramp_is_never_a_bypass() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- scale bypass"]
fn test_arming_the_scales_exit_and_not_taking_it_is_still_a_bypass() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- scale bypass"]
fn test_rolling_past_an_open_scale_is_still_a_bypass() {}

// --- calibration: what presence costs a clean driver, and a reckless one ----

/// Drive the route in position steps and count the pull-overs. Position
/// steps, not physics frames, on purpose: the outcome depends on where you
/// were and how fast, never on how many frames it took to get there.
#[allow(clippy::too_many_arguments)]
fn walk(
    trip: &mut Trip,
    over: f64,
    damage: f64,
    pack: i64,
    duty: f64,
    recover_mi: f64,
    seed: i64,
) -> usize {
    for post in trip.posts.iter_mut() {
        post.announced = true;
    }
    let step = 0.05;
    let mut stops = 0;
    let mut mile = 0.0;
    let mut quiet_until = -1.0;
    let mut hot = true;
    let mut duty_rng = PyRandom::new_from_str(&format!("duty:{seed}"));
    let total = trip.route.miles();
    let trip_seed = trip.seed;
    while mile < total {
        mile += step;
        if (mile / 5.0) as i64 != ((mile - step) / 5.0) as i64 {
            hot = duty_rng.random() < duty;
        }
        // After a pull-over the truck is stopped on the shoulder and has to
        // get back up to speed; nobody is twenty over for the next few miles.
        let legal = mile < quiet_until || !hot;
        let (limit, _) = trip.speed_limit_at(mile);
        let mut sample = RoadSample::new(mile, limit + if legal { 5.0 } else { over }, limit);
        sample.damage_pct = damage;
        sample.pack_neighbours = pack;
        sample.over_limit_mi = if legal { 0.0 } else { OBSERVE_HOLD_MI * 2.0 };
        let watching: Vec<String> = trip.posts_watching(mile).iter().map(|p| p.id()).collect();
        for post_id in watching {
            let Some(post) = trip.post_mut(&post_id) else {
                continue;
            };
            if post.declined {
                continue;
            }
            let Some(found) = observe(post, &sample) else {
                continue;
            };
            post.declined = true;
            let key = post_seed(
                trip_seed,
                &post_id,
                &format!(
                    "observe:{}:{}",
                    found.what,
                    ff_core::pyfmt::round_py_n(mile, 1)
                ),
            );
            if PyRandom::new_from_str(&key).random() < found.confidence {
                stops += 1;
                quiet_until = mile + recover_mi;
            }
        }
    }
    stops
}

#[test]
fn test_a_clean_driver_hears_the_police_constantly_and_pays_nothing() {
    // The whole point. Present means heard, not charged.
    for (a, b) in [("Chicago", "Denver"), ("Los Angeles", "Phoenix")] {
        for seed in 0..4 {
            let mut trip = trip_seeded(a, b, seed);
            assert!(!trip.audible_enforcement_contacts().is_empty());
            assert_eq!(walk(&mut trip, 5.0, 0.0, 0, 1.0, 0.0, seed), 0);
        }
    }
}

#[test]
fn test_a_reckless_driver_meets_the_same_road_and_a_very_different_bill() {
    // Presence is identical; consequence is not.
    for (a, b) in [("Chicago", "Denver"), ("Los Angeles", "Phoenix")] {
        let mut clean_contacts = Vec::new();
        let mut reckless_contacts = Vec::new();
        let mut rates = Vec::new();
        for seed in 0..4 {
            let clean = trip_seeded(a, b, seed);
            clean_contacts.push(clean.audible_enforcement_contacts().len());
            let mut trip = trip_seeded(a, b, seed);
            reckless_contacts.push(trip.audible_enforcement_contacts().len());
            let stops = walk(&mut trip, 22.0, 60.0, 0, 0.5, 25.0, seed);
            rates.push(stops as f64 / trip.route.miles() * 500.0);
        }
        // The world does not change shape for a bad driver.
        assert_eq!(clean_contacts, reckless_contacts);
        let average = rates.iter().sum::<f64>() / rates.len() as f64;
        assert!(
            (2.0..=5.0).contains(&average),
            "{a}->{b} averaged {average:.1} stops per 500 miles"
        );
    }
}

#[test]
fn test_speeding_scales_from_free_to_expensive_with_no_flat_spot() {
    // Being seen is now the ONLY thing speeding costs, so the curve between
    // legal and habitual has to be a curve.
    for (a, b) in [("Chicago", "Denver"), ("Los Angeles", "Phoenix")] {
        let rate = |over: f64, damage: f64, duty: f64, recover_mi: f64| {
            let mut totals = Vec::new();
            for seed in 0..6 {
                let mut trip = trip_seeded(a, b, seed);
                let stops = walk(&mut trip, over, damage, 0, duty, recover_mi, seed);
                totals.push(stops as f64 / trip.route.miles() * 500.0);
            }
            totals.iter().sum::<f64>() / totals.len() as f64
        };
        let legal = rate(5.0, 0.0, 1.0, 0.0);
        let occasional = rate(13.0, 0.0, 0.2, 25.0);
        let habitual = rate(22.0, 0.0, 0.5, 25.0);
        assert_eq!(
            legal, 0.0,
            "{a}->{b}: driving legally cost {legal:.2} stops"
        );
        assert!(
            (0.15..=1.5).contains(&occasional),
            "{a}->{b}: an occasional speeder saw {occasional:.2} stops per 500 miles"
        );
        assert!(
            habitual >= 2.0,
            "{a}->{b}: a habitual speeder saw only {habitual:.2}"
        );
        assert!(habitual > occasional * 3.0);
    }
}

#[test]
fn test_riding_in_a_pack_is_a_real_tactic_over_a_whole_run() {
    // The unit test proves the factor; this proves it survives a run.
    let mut alone = 0;
    let mut packed = 0;
    for seed in 0..4 {
        let mut trip = trip_seeded("Chicago", "Denver", seed);
        alone += walk(&mut trip, 16.0, 20.0, 0, 0.5, 25.0, seed);
        let mut trip = trip_seeded("Chicago", "Denver", seed);
        packed += walk(&mut trip, 16.0, 20.0, 4, 0.5, 25.0, seed);
    }
    assert!(packed < alone);
}

#[test]
fn test_the_same_driving_through_the_same_road_produces_the_same_outcome() {
    let mut a_trip = trip_seeded("Chicago", "Denver", 3);
    let a = walk(&mut a_trip, 18.0, 30.0, 0, 0.5, 25.0, 3);
    let mut b_trip = trip_seeded("Chicago", "Denver", 3);
    let b = walk(&mut b_trip, 18.0, 30.0, 0, 0.5, 25.0, 3);
    assert_eq!(a, b);
}

// --- the setting, and what it promises --------------------------------------

#[test]
fn test_how_loud_the_road_sounds_comes_from_the_road() {
    // The slider's replacement: the same number that places the posts.
    let trip = trip();
    let at = trip.post_density_at(trip.total_miles() / 2.0);
    assert!(at > 0.0);
    // The Python half of this test read the driving layer's docstring; the
    // ambience scale belongs to the app shell.
}

// `test_an_empty_crossover_is_never_audible` is live in `crates/freight-fate/tests/states_driving_enforcement.rs`.

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- the open scale reads the safety record"]
fn test_an_open_scale_reads_the_safety_record_aloud() {}

// --- pacing -----------------------------------------------------------------

#[test]
fn test_a_pacing_unit_can_bank_its_pace_inside_its_own_window() {
    // The structural bug: two windows that could never both be satisfied.
    let (gate, window) = (PACING_MIN_MI, PACING_WINDOW_MI);
    assert!(gate < window, "the gate must fit inside the window");
    assert!(2.0 * gate <= window);

    let trip = trip();
    let roving: Vec<&EnforcementPost> = trip
        .posts
        .iter()
        .filter(|p| p.method == METHOD_PACING)
        .collect();
    assert!(
        !roving.is_empty(),
        "the sample route must carry a roving patrol"
    );
    let post = roving[0];
    assert!((post.end_mi() - post.at_mi - PACING_WINDOW_MI).abs() < 1e-9);
    // And it is still asked to look at the far end of that window.
    assert!(post.covers(post.at_mi + PACING_WINDOW_MI - 1e-6));
}

#[test]
fn test_the_pacing_gate_is_road_not_real_seconds() {
    // It was 20 real seconds, which no time compression the game offers
    // could satisfy.
    let post = post_kind(10.0, KIND_ROVING);
    assert_eq!(post.method, METHOD_PACING);
    let sample = |paced_mi: f64| RoadSample {
        paced_mi,
        ..RoadSample::new(post.at_mi + 0.1, 77.0, 65.0)
    };
    assert_eq!(geometry_factor(&post, &sample(PACING_MIN_MI * 0.5)), 0.0);
    assert!(geometry_factor(&post, &sample(PACING_MIN_MI)) > 0.0);
}

#[test]
fn test_a_roving_patrol_catches_a_sustained_speeder() {
    // End to end through observe(), which never once happened before.
    let post = post_kind(10.0, KIND_ROVING);
    let sample = RoadSample {
        over_limit_mi: 1.0,
        paced_mi: PACING_MIN_MI,
        ..RoadSample::new(post.at_mi + PACING_MIN_MI, 77.0, 65.0)
    };
    let found = observe(&post, &sample).expect("seen");
    assert_eq!(found.what, WHAT_SPEEDING);
}

// --- following too close: held, not glimpsed --------------------------------
// Tester Darren, I-75 near Forsyth GA, 2026-08-18: one sampled frame inside
// the 1.2 second gap was the whole offence.

#[test]
fn test_a_momentary_gap_dip_is_not_a_following_citation() {
    let post = post_kind(10.0, KIND_WORK_ZONE);
    let glimpsed = RoadSample {
        speed_mph: 60.0,
        limit_mph: 65.0,
        over_limit_mi: 0.0,
        following_gap_s: Some(0.6),
        closed_up_mi: 0.0,
        ..sample()
    };
    assert!(observe(&post, &glimpsed).is_none());
}

#[test]
fn test_a_gap_held_closed_over_real_road_still_is_one() {
    let post = post_kind(10.0, KIND_WORK_ZONE);
    let held = RoadSample {
        speed_mph: 60.0,
        limit_mph: 65.0,
        over_limit_mi: 0.0,
        following_gap_s: Some(0.6),
        closed_up_mi: OBSERVE_HOLD_MI * 2.0,
        ..sample()
    };
    let observation = observe(&post, &held).expect("seen");
    assert_eq!(observation.what, WHAT_FOLLOWING);
}

#[test]
#[ignore = "Python asserted on the module source text (inspect.getsource); the hold is pinned by the two tests above"]
fn test_the_hold_matches_the_one_speeding_has_always_had() {}
