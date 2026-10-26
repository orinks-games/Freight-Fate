//! The playtest harness's own coverage (port of
//! `tests/test_playtest_harness.py`).
//!
//! These are the gate for the 119 transcript test files that code against
//! [`PlaytestHarness`]: if the harness cannot start a route, drive it, and
//! report what was heard, nothing downstream means anything.
//!
//! # What is ignored and why
//!
//! The Python file drove most of its cases through `start_delivery`, which
//! walks the real menus from the main menu to the wheel. That walk works:
//! `states::city::launch_driving` builds the real `DrivingState`, and the
//! pull-in beat at the destination gate opens the dock menu over a drive
//! that is still live, so those cases run as written.
//!
//! One delivery case is still parked, and its `blocked:` reason names what
//! on: the event pacer measures "would this line start after its moment"
//! on the wall clock, and a harness delivery covers hundreds of miles in
//! seconds of it, so the road's ambient lines -- state lines, city
//! passings -- are dropped as stale. The Python harness recorded above the
//! pacer and never saw it.
//!
//! A handful of Python cases patched a method on the live drive
//! (`_upcoming_exit_stop`, `trip.speed_limit_at`, `trip.grade_at`) with
//! `monkeypatch.setattr`. Rust has no equivalent for an inherent method, so
//! those are ported as far as they go and marked with what they would need.

use ff_core::models::business::LEASED_OWNER_OPERATOR;
use ff_core::models::career::LEVEL_XP;
use ff_core::sim::enforcement_observe::OBSERVE_HOLD_MI;
use ff_core::sim::season::{date_text, real_clock_game_hours};
use ff_core::sim::transmission::REVERSE;
use ff_core::sim::trip_models::{TripEventData, TripEventKind};
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup, StartDelivery};
use freight_fate::speech::SpeechChannel;
use freight_fate::states::base::Key;
use freight_fate::states::driving_core::{HAZARD_SAFE_MPH, RAMP_MAX_MPH};

const MPH_PER_MPS: f64 = 2.23694;

// -- the result's own assertions ------------------------------------------------------
//
// The three `assert_*` helpers are what every transcript test calls, so they
// are pinned here rather than only exercised through a drive.

#[test]
fn assert_ordered_allows_unrelated_speech_between_phrases() {
    let result = freight_fate::playtest::PlaytestResult {
        transcript: vec![
            "Freight Fate.".to_string(),
            "Weather is clear.".to_string(),
            "Dispatch routed you to Rochester.".to_string(),
        ],
        ..Default::default()
    };
    result.assert_ordered(&["Freight Fate", "Dispatch routed"]);
}

#[test]
#[should_panic(expected = "Missing or out-of-order phrase")]
fn assert_ordered_rejects_a_phrase_that_came_first() {
    let result = freight_fate::playtest::PlaytestResult {
        transcript: vec![
            "Dispatch routed you to Rochester.".to_string(),
            "Freight Fate.".to_string(),
        ],
        ..Default::default()
    };
    result.assert_ordered(&["Freight Fate", "Dispatch routed"]);
}

#[test]
#[should_panic(expected = "raw map data spoken")]
fn assert_screen_reader_friendly_rejects_raw_map_data() {
    let result = freight_fate::playtest::PlaytestResult {
        transcript: vec!["Passing node/12345 on the left.".to_string()],
        spoken: vec![freight_fate::speech::SpokenEntry {
            sequence: 0,
            channel: SpeechChannel::Main,
            text: "Passing node/12345 on the left.".to_string(),
            interrupt: true,
        }],
        ..Default::default()
    };
    result.assert_screen_reader_friendly();
}

#[test]
fn assert_no_known_destination_exit_regressions_reads_whole_words() {
    // "121 miles remaining" must not trip the `\b21 miles remaining\b` guard.
    let result = freight_fate::playtest::PlaytestResult {
        transcript: vec!["121 miles remaining.".to_string()],
        remaining_miles: 0.0,
        ..Default::default()
    };
    result.assert_no_known_destination_exit_regressions();
}

// -- the environment ------------------------------------------------------------------

/// `test_playtest_harness_forces_headless_environment_before_pygame`.
///
/// The Python version proved it in a subprocess, because importing the
/// harness module set the variables. Here the rig sets them when it builds
/// the app, so building one and reading them back is the same proof.
#[test]
fn test_playtest_harness_forces_headless_environment() {
    let harness = PlaytestHarness::new();
    assert_eq!(std::env::var("SDL_VIDEODRIVER").as_deref(), Ok("dummy"));
    assert_eq!(std::env::var("SDL_AUDIODRIVER").as_deref(), Ok("dummy"));
    assert_eq!(std::env::var("FREIGHT_FATE_NO_SPEECH").as_deref(), Ok("1"));
    assert!(harness.app.is_headless());
}

/// Simulated frames buy the event pacer simulated seconds.
///
/// The pacer decides whether a queued ambient line would start speaking too
/// late to still be true, and it decides it in REAL seconds, because that is
/// the only kind a player's ear has. A harness that drives 500 miles in
/// fourteen seconds of wall clock and lets the pacer read that wall clock is
/// telling it every line on the road is minutes late: 75 ambient lines were
/// dropped on one Indianapolis-Nashville-Atlanta delivery against 32 spoken,
/// state crossings and city passings among them. So the harness owns the
/// pacer's clock and pays it one frame of real time per frame it steps. This
/// test is here so the next person to "simplify" that back to the wall clock
/// finds out here rather than in a transcript that quietly went silent.
#[test]
fn test_the_harness_pays_the_pacer_a_frame_of_real_time_per_frame() {
    let mut harness = PlaytestHarness::new();
    harness.start_route("Newark", "New York", RouteSetup::default());
    harness.prepare_for_driving(45.0);
    let before = harness.clock_now();
    harness.drive_frames(600);
    let elapsed = harness.clock_now() - before;
    // Ten seconds of road at sixty frames a second, which is what those
    // frames would have cost a player.
    assert!(
        (elapsed - 10.0).abs() < 0.01,
        "600 frames advanced the pacer clock by {elapsed} seconds"
    );
}

/// A route started off the board still speaks city names, not map keys.
///
/// `start_route` skips the dispatch board, and the board is what fills in a
/// job's spoken endpoints. Left empty, every line that names the delivery
/// city fell back to the slug -- "in hattiesburg_ms_us" on the missed-exit
/// call -- which a transcript test cannot tell from a real defect.
#[test]
fn test_start_route_speaks_the_city_not_its_map_key() {
    let mut harness = PlaytestHarness::new();
    harness.start_route("jackson_ms_us", "hattiesburg_ms_us", RouteSetup::default());
    let (origin, destination) = harness.with_drive(|drive, _| {
        (
            drive.job.origin_spoken.clone(),
            drive.job.destination_spoken.clone(),
        )
    });
    // Disambiguated the way dispatch says them -- there is more than one
    // Jackson on the map.
    assert_eq!(origin, "Jackson, Mississippi");
    assert_eq!(destination, "Hattiesburg");
    assert!(!harness.transcript_text().contains("hattiesburg_ms_us"));
}

/// `test_app_forces_dummy_video_when_speech_is_disabled`: the Python test
/// launched a subprocess that built a real windowed `App` with a sentinel
/// video driver and checked pygame had replaced it. Porting it means
/// spawning the built `freightfate` binary, which a unit test cannot rely on
/// having been linked yet.
#[test]
#[ignore = "needs the built freightfate binary to spawn a windowed subprocess"]
fn test_app_forces_dummy_video_when_speech_is_disabled() {}

// -- starting a route -----------------------------------------------------------------

#[test]
fn test_playtest_harness_neutralizes_random_traffic_by_default() {
    let mut harness = PlaytestHarness::new();
    harness.start_route("Buffalo", "Rochester", RouteSetup::default());

    harness.read_drive(|drive| {
        assert!(drive.trip.npc_vehicles().is_empty());
        assert!(drive.trip.traffic_pressures.is_empty());
        assert!(!drive.trip.traffic_manager.rolling_bubble);
        assert_eq!(drive.weather().current, WeatherKind::Clear);
    });
}

#[test]
fn test_playtest_harness_can_exercise_npc_traffic() {
    let mut harness = PlaytestHarness::new();
    harness.start_route("Buffalo", "Rochester", RouteSetup::default());
    harness.prepare_for_driving(55.0);
    harness.add_npc_traffic_ahead("merging_vehicle", 0.8, 42.0, 1);

    harness.drive_frames(8);

    let text = harness.transcript_text();
    assert!(text.contains("[event] Merging vehicle"), "{text}");
    assert!(text.contains("ahead."), "{text}");
}

#[test]
fn test_upcoming_key_leaves_traffic_pressure_to_its_own_advisory() {
    // U stopped reciting traffic pressure (owner report, 2026-08-15).
    //
    // Two of its three sources restated the clause printed right beside them
    // in the same readout -- the construction taper's own squeeze and the
    // exit traffic for the stop just named. The advisory itself is unchanged
    // and is covered where it is emitted; what this pins is that pressing U
    // does not repeat it.
    let mut harness = PlaytestHarness::new();
    harness.start_route("Buffalo", "Rochester", RouteSetup::default());
    // Park ten miles short of a real stop so the readout has something of its
    // own to say; the Python case got that for free from the dispatch board's
    // job, and without it U answers "Nothing notable" and proves nothing.
    let stop_mi = harness.read_drive(|drive| {
        drive
            .trip
            .stops
            .first()
            .map(|stop| stop.at_mi)
            .expect("this corridor carries a road stop")
    });
    harness.with_drive(|drive, _| drive.trip.position_mi = (stop_mi - 10.0).max(0.0));
    harness.add_traffic_pressure_ahead(2.0, "exit", "right", "exit traffic for harness ramp");

    harness.press_key(Key::U, None);

    let text = harness.transcript_text();
    assert!(text.contains("Coming up:"), "{text}");
    assert!(!text.contains("exit traffic for harness ramp"), "{text}");
    assert!(!text.contains("move right and target"), "{text}");
}

#[test]
fn test_playtest_transcript_preserves_hazard_warning_and_outcome() {
    let mut harness = PlaytestHarness::new();
    harness.start_route("Buffalo", "Rochester", RouteSetup::default());
    let warning = "Brake now! A slow vehicle ahead.";
    harness.emit_trip_event(
        TripEventKind::Hazard,
        warning,
        TripEventData {
            deadline_s: Some(3.0),
            ..Default::default()
        },
    );
    harness.with_drive(|drive, ctx| {
        drive.truck_mut().velocity_mps = (HAZARD_SAFE_MPH - 1.0) / 2.2369362920544;
        drive.update_hazard(ctx, 1.0 / 60.0);
    });

    let transcript = harness.transcript();
    let warning_index = transcript
        .iter()
        .position(|line| line == &format!("[event] {warning}"))
        .expect("the hazard warning was spoken");
    let outcome_index = transcript
        .iter()
        .position(|line| line == "[event] Clear.")
        .expect("the hazard resolution was spoken");
    assert!(warning_index < outcome_index);
}

#[test]
fn test_deterministic_hook_restores_inspection_event() {
    let mut harness = PlaytestHarness::new();
    harness.start_route("Buffalo", "Rochester", RouteSetup::default());
    harness.emit_trip_event(
        TripEventKind::Inspection,
        "Inspection station ahead.",
        TripEventData {
            name: Some("Harness safety scale".to_string()),
            ..Default::default()
        },
    );

    let result = harness.result();
    assert!(
        result
            .transcript_text()
            .contains("Inspection station ahead"),
        "{}",
        result.transcript_text()
    );
    assert!(result
        .spoken
        .iter()
        .any(|entry| entry.channel == SpeechChannel::Event));
    result.assert_screen_reader_friendly();
}

#[test]
fn test_playtest_route_report_includes_current_location_on_real_keyboard_path() {
    let mut harness = PlaytestHarness::new();
    harness.start_route("Buffalo", "Rochester", RouteSetup::default());
    harness.with_drive(|drive, _| drive.trip.position_mi = 40.0);
    harness.press_key(Key::R, None);

    let transcript = harness.transcript();
    let last = transcript.last().expect("R said something");
    assert!(
        last.ends_with("On I-90 East in New York, toward Rochester, New York."),
        "{last}"
    );
    assert!(last.contains("percent there"), "{last}");
}

/// Both styles share the one safe gesture now: a fresh press at a standstill,
/// held through the engage beat. A hold that predates the stop never engages,
/// in either style.
#[test]
fn test_playtest_transcript_covers_both_automatic_direction_styles() {
    let mut harness = PlaytestHarness::new();
    harness.start_route("Newark", "New York", RouteSetup::default());
    harness.with_drive(|drive, _| drive.truck_mut().velocity_mps = 0.0);

    harness.app.ctx.settings.automatic_direction_changes = "simple".to_string();
    harness.with_drive(|drive, ctx| {
        drive.reverse_brake_held = true;
        assert!(!drive.update_reverse_controls(ctx, false, true, false, true, 1.0 / 60.0));
        assert_ne!(drive.truck().transmission.gear, REVERSE);
        drive.update_reverse_controls(ctx, false, false, false, false, 1.0 / 60.0);
    });
    assert!(hold_direction(&mut harness, false, true, 0.75));
    // The gear is the outcome; reverse itself says nothing now, because the
    // beep runs for as long as the truck is in reverse and a one-shot
    // sentence cannot (owner, 2026-08-21).
    harness.read_drive(|drive| assert_eq!(drive.truck().transmission.gear, REVERSE));
    assert!(!harness
        .transcript()
        .iter()
        .any(|line| line == "[event] Reverse selected. Backing slowly."));

    harness.with_drive(|drive, _| drive.truck_mut().transmission.gear = 1);
    harness.clear_speech();
    harness.app.ctx.settings.automatic_direction_changes = "deliberate".to_string();
    harness.with_drive(|drive, ctx| {
        drive.reverse_brake_held = true;
        assert!(!drive.update_reverse_controls(ctx, false, true, false, true, 1.0 / 60.0));
        assert_ne!(drive.truck().transmission.gear, REVERSE);
    });
    assert!(harness.transcript().is_empty());

    harness.with_drive(|drive, ctx| {
        drive.update_reverse_controls(ctx, false, false, false, false, 1.0 / 60.0);
    });
    assert!(hold_direction(&mut harness, false, true, 0.75));
    harness.read_drive(|drive| assert_eq!(drive.truck().transmission.gear, REVERSE));
    assert!(!harness
        .transcript()
        .iter()
        .any(|line| line == "[event] Reverse selected. Backing slowly."));
}

/// The Python `hold(...)` helper: run the direction gesture for `seconds`.
fn hold_direction(
    harness: &mut PlaytestHarness,
    accelerating: bool,
    braking_key: bool,
    seconds: f64,
) -> bool {
    let frames = (seconds * 60.0) as usize + 2;
    let mut result = false;
    for _ in 0..frames {
        result = harness.with_drive(|drive, ctx| {
            drive.update_reverse_controls(
                ctx,
                accelerating,
                braking_key,
                accelerating,
                braking_key,
                1.0 / 60.0,
            )
        });
    }
    result
}

#[test]
fn test_radio_controls_are_keyboard_reachable() {
    let mut harness = PlaytestHarness::new();
    harness.start_route("Buffalo", "Rochester", RouteSetup::default());
    // At the top of the load the engine is off, and a dead cab has no radio:
    // the key answers with the no-power line instead of toggling.
    let before_enabled = harness.read_drive(|drive| drive.radio.enabled);
    harness.press_key(Key::M, None);
    assert_eq!(
        harness.read_drive(|drive| drive.radio.enabled),
        before_enabled
    );
    let last = harness.transcript().last().cloned().unwrap_or_default();
    assert!(last.to_lowercase().contains("no power"), "{last}");

    harness.prepare_for_driving(30.0);
    harness.press_key(Key::M, None);
    assert_ne!(
        harness.read_drive(|drive| drive.radio.enabled),
        before_enabled
    );
    let last = harness.transcript().last().cloned().unwrap_or_default();
    assert!(last.to_lowercase().starts_with("radio "), "{last}");

    // The dial is inert with the radio switched off (Darren, 2026-08-16), so
    // reaching the tuning keys means having the radio on first.
    if !harness.read_drive(|drive| drive.radio.enabled) {
        let parked = harness.read_drive(|drive| drive.radio.station_id.clone());
        harness.press_key(Key::PageDown, None);
        assert_eq!(harness.read_drive(|d| d.radio.station_id.clone()), parked);
        assert_eq!(
            harness.transcript().last().cloned().unwrap_or_default(),
            "Radio off."
        );
        harness.press_key(Key::M, None);
    }
    assert!(harness.read_drive(|drive| drive.radio.enabled));
    let before_station = harness.read_drive(|d| d.radio.station_id.clone());
    harness.press_key(Key::PageDown, None);
    assert_ne!(
        harness.read_drive(|d| d.radio.station_id.clone()),
        before_station
    );
    let last = harness.transcript().last().cloned().unwrap_or_default();
    assert!(
        last.to_lowercase().contains("selected") || last.to_lowercase().contains("tuned"),
        "{last}"
    );
    harness.press_key(Key::PageUp, None);
    assert_eq!(
        harness.read_drive(|d| d.radio.station_id.clone()),
        before_station
    );
    // Semicolon and apostrophe stay as secondary dial keys: Page Up and Page
    // Down are Fn chords on many laptops and missing on 60 percent keyboards,
    // and there is no user-facing key remapping.
    harness.press_key(Key::Quote, Some('\''));
    assert_ne!(
        harness.read_drive(|d| d.radio.station_id.clone()),
        before_station
    );
    harness.press_key(Key::Semicolon, Some(';'));
    assert_eq!(
        harness.read_drive(|d| d.radio.station_id.clone()),
        before_station
    );
    harness.press_key(Key::Y, Some('y'));
    assert!(harness
        .transcript()
        .last()
        .is_some_and(|line| line.starts_with("Radio ")));

    let result = harness.result();
    assert!(result.transcript_text().to_lowercase().contains("radio"));
    result.assert_screen_reader_friendly();
}

#[test]
fn test_lane_setup_keys_change_lane_and_speak_result() {
    let mut harness = PlaytestHarness::new();
    harness.start_route("Buffalo", "Rochester", RouteSetup::default());
    harness.prepare_for_driving(45.0);
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    harness.with_drive(|drive, _| drive.lane.lane = 0);
    harness.press_key(Key::Left, None);
    harness.read_drive(|drive| assert_eq!(drive.lane_change_target, Some(1)));
    harness.with_drive(|drive, ctx| drive.update_tap_lane_change(ctx, 3.0));
    harness.read_drive(|drive| assert_eq!(drive.lane.lane, 1));
    let last = harness.transcript().last().cloned().unwrap_or_default();
    assert!(last.to_lowercase().contains("left lane"), "{last}");
}

#[test]
fn test_deterministic_landmark_and_billboard_hooks_honor_granular_toggles() {
    let mut harness = PlaytestHarness::new();
    harness.start_route("Buffalo", "Rochester", RouteSetup::default());
    harness.emit_trip_event(
        TripEventKind::Landmark,
        "Crossing the Harness River.",
        TripEventData {
            category: Some("river".to_string()),
            ..Default::default()
        },
    );
    harness.emit_trip_event(
        TripEventKind::Billboard,
        "Billboard: Harness coffee ahead.",
        TripEventData {
            category: Some("billboard".to_string()),
            ..Default::default()
        },
    );
    // Realistic frames, not one synthetic 999-second leap: ambient lines now
    // expire if they wait too long, so a single enormous dt ages the whole
    // queue out instead of draining it. A tenth of a second at a time is what
    // the road actually does, and one line leaves the queue per frame by
    // design -- the spacing is the point of the channel.
    for _ in 0..200 {
        if harness.read_drive(|drive| drive.pending_ambient_events.is_empty()) {
            break;
        }
        harness.with_drive(|drive, ctx| {
            drive.ambient_event_cooldown_s = 0.0;
            drive.update_ambient_events(ctx, 0.1);
        });
    }
    let before = harness.spoken().len();
    harness.app.ctx.settings.chatter_rivers = false;
    harness.app.ctx.settings.chatter_billboards = false;
    harness.emit_trip_event(
        TripEventKind::Landmark,
        "Crossing the Muted River.",
        TripEventData {
            category: Some("river".to_string()),
            ..Default::default()
        },
    );
    harness.emit_trip_event(
        TripEventKind::Billboard,
        "Billboard: Muted roadside joke.",
        TripEventData {
            category: Some("billboard".to_string()),
            ..Default::default()
        },
    );

    let text = harness.transcript_text();
    assert!(text.contains("Harness River"), "{text}");
    assert!(text.contains("Harness coffee"), "{text}");
    assert_eq!(harness.spoken().len(), before);
    assert!(!text.contains("Muted"), "{text}");
}

#[test]
fn test_keyboard_navigation_failure_is_bounded_and_descriptive() {
    let mut harness = PlaytestHarness::new();
    harness
        .app
        .push_state(freight_fate::states::main_menu::MainMenuState::new());
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        harness.select_current_menu_text("Missing harness action");
    }))
    .expect_err("an unreachable menu item panics");
    let message = panic.downcast_ref::<String>().cloned().unwrap_or_else(|| {
        panic
            .downcast_ref::<&str>()
            .copied()
            .unwrap_or("")
            .to_string()
    });
    assert!(message.contains("not reachable with Down"), "{message}");
}

// -- speed control, the transcript suite's core --------------------------------------

/// `test_realistic_speed_control_transitions_do_not_issue_speeding_fines`,
/// the construction case at standard pacing.
///
/// A full delivery under an isolated data directory; the sibling parameter
/// cases in the Python file (40x pacing, the Delaware bend, the strict-xfail
/// heavy-traffic case) are the transcript suite's, not the harness's.
#[test]
fn test_realistic_speed_control_transitions_do_not_issue_speeding_fines() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
    harness.app.ctx.settings.speed_keeper = true;
    harness.app.ctx.settings.automatic_transmission = true;
    harness.app.ctx.settings.time_scale = 20.0;
    harness.start_route("Chicago", "Indianapolis", RouteSetup::seeded(0));
    let (start_mi, end_mi) = harness.read_drive(|drive| {
        let zone = drive
            .trip
            .zones
            .iter()
            .find(|zone| zone.reason == "construction")
            .expect("this seed places a work zone");
        (
            (zone.start_mi - 9.0).max(0.0),
            (zone.end_mi + 3.0).min(drive.trip.total_miles()),
        )
    });
    let result = harness.drive_speed_control_segment(start_mi, end_mi, 70.0);
    let result = {
        harness.settle_delivery_after_segment();
        let _ = result;
        harness.result()
    };

    assert_eq!(
        result.speed_control_transitions,
        vec![
            "cruise".to_string(),
            "keeper".to_string(),
            "cruise".to_string()
        ]
    );
    assert_eq!(result.speeding_tickets, 0, "{}", result.transcript_text());
    assert_eq!(result.inspection_fines, 0, "{}", result.transcript_text());
    // Cruise never held the truck over the limit far enough for a post to
    // read a speed out of it, and nothing was written.
    assert!(result.max_over_limit_mi < OBSERVE_HOLD_MI);
    let text = result.transcript_text();
    assert!(!text.contains("Lights and siren"), "{text}");
    assert!(!text.contains("Speeding strike"), "{text}");
    assert!(!text.to_lowercase().contains("speeding fines"), "{text}");
    assert_eq!(result.deliveries, 1);
    assert!(text.contains("Speed keeper holding"), "{text}");
    assert!(text.contains("Cruise resuming"), "{text}");
    // At the posted limit, not highway speed. The tolerance covers the cruise
    // loop's two-mph brake deadband, which lands the handover a fraction over
    // 45 -- far inside the speeding leeway, and the no-fine assertions above
    // are what actually pin the behaviour.
    let entry = result
        .construction_entry_speed_mph
        .expect("the work zone was entered");
    assert!(entry <= 46.0, "{entry}");
    assert_eq!(
        text.matches("Construction zone ahead, cruise easing to 45.")
            .count(),
        1
    );
    // Two stages, in the order the warning promised them: the taper's 55
    // first, the work zone's 45 at the barrels.
    let to_55 = text
        .find("Construction zone ahead, cruise easing to 55.")
        .unwrap_or_else(|| panic!("no easing to 55: {text}"));
    let to_45 = text
        .find("Construction zone ahead, cruise easing to 45.")
        .expect("checked above");
    assert!(to_55 < to_45, "{text}");
}

#[test]
fn test_realistic_cruise_eases_for_destination_exit_without_speeding_fine() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
    // The case is about the REALISTIC ruleset, so it names it rather than
    // inheriting whatever a fresh install ships -- which moved to Balanced on
    // 2026-09-18 and took descent control's brake capture with it.
    harness
        .app
        .ctx
        .settings
        .apply_driving_assistance_preset("realistic");
    harness.app.ctx.settings.speed_keeper = true;
    harness.app.ctx.settings.automatic_transmission = true;
    harness.app.ctx.settings.time_scale = 10.0;
    harness.start_route("Chicago", "Indianapolis", RouteSetup::seeded(0));
    let gore_mph = harness.with_drive(|drive, ctx| {
        let destination = drive
            .destination_exit_stop(ctx)
            .expect("this route has a destination exit");
        drive.gore_acceptance_mph(Some(&destination))
    });
    let result = harness.drive_destination_exit_with_speed_control(70.0, None);

    let exit_speed = result
        .destination_exit_speed_mph
        .expect("the ramp was entered");
    // The gore accepts road speed -- the deceleration lane exists so a driver
    // leaves at it and sheds inside it -- and the ramp's own number governs
    // from there (owner, 2026-08-21). The flat 45 was never the gate's job.
    assert!(gore_mph > RAMP_MAX_MPH);
    assert!(exit_speed <= gore_mph, "{exit_speed} > {gore_mph}");
    assert_eq!(result.speeding_tickets, 0, "{}", result.transcript_text());
    assert!(result.max_over_limit_mi < OBSERVE_HOLD_MI);
    let text = result.transcript_text();
    assert!(!text.contains("Lights and siren"), "{text}");
    assert!(text.contains("destination exit"), "{text}");
    // How cruise takes the ramp is not narrated (owner, 2026-10-14).
    assert!(!text.contains("pauses on the ramp"), "{text}");
    // The exit speed is named once the truck is in the deceleration lane.
    assert!(text.contains("Exit speed "), "{text}");
    // The exit key is a turn signal now: "Signal set for ..." (the blinker
    // waits for half a mile) replaced the older "Signaling for ..." callout
    // when the cancel/confirm model landed.
    assert!(text.contains("Signal set for"), "{text}");
    assert!(text.contains(" of ramp"), "{text}");
    assert!(
        !text.to_lowercase().contains("missed the destination exit"),
        "{text}"
    );
    assert_eq!(result.deliveries, 1);
}

/// Transcript proof for issue #113 using the real X key path.
#[test]
fn test_delayed_x_takes_announced_destination_exit_after_window_shrinks() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.time_scale = 20.0;
    harness.start_route("Chicago", "Indianapolis", RouteSetup::seeded(0));
    let response_before = harness.with_drive(|drive, ctx| {
        drive.tutorial = None;
        drive.trip.time_scale = 20.0;
        drive.trip.set_patrols(Vec::new());
        drive.truck_mut().velocity_mps = 54.0 / MPH_PER_MPS;
        let destination = drive
            .destination_exit_stop(ctx)
            .expect("this route has a destination exit");
        drive.trip.position_mi = destination.at_mi - drive.exit_window_mi() + 0.01;
        drive.check_destination_exit(ctx);
        drive.truck_mut().velocity_mps = 30.0 / MPH_PER_MPS;
        drive.destination_exit_response_s
    });
    // Give the player five real seconds to hear and react instead of the
    // former same-frame coverage that masked the bug.
    for _ in 0..5 * 60 {
        harness.with_drive(|drive, ctx| drive.update_frame(ctx, 1.0 / 60.0));
    }
    harness.read_drive(|drive| {
        assert!(
            (drive.destination_exit_response_s - (response_before - 5.0)).abs() < 0.05,
            "{} vs {}",
            drive.destination_exit_response_s,
            response_before - 5.0
        );
    });

    harness.press_key(Key::X, None);

    harness.read_drive(|drive| {
        let stop = drive.exit_stop.as_ref().expect("X armed the exit");
        assert_eq!(stop.stop_type, "delivery_destination");
        assert!(drive.cruise_mph.is_none());
        assert!(drive.cruise_exit_mph.is_none());
    });
    let text = harness.transcript_text();
    assert!(text.contains("destination exit"), "{text}");
    // "Signal set for ..." is the 1.9 wording of the old "Signaling for ...".
    assert!(text.contains("Signal set for"), "{text}");
    assert!(!text.contains("No exit coming up"), "{text}");
    assert!(
        !text.to_lowercase().contains("missed the destination exit"),
        "{text}"
    );
}

#[test]
#[ignore = "needs a way to override _upcoming_exit_stop / grade_at on a live drive"]
fn test_signaled_downhill_exit_keeps_cruise_below_ramp_limit() {}

#[test]
#[ignore = "needs a way to override _upcoming_exit_stop on a live drive"]
fn test_rest_stop_arrival_cue_allows_immediate_parking_brake_stop() {}

// -- whole deliveries -------------------------------------------------------------------

#[test]
fn test_playtest_harness_drives_a_specific_route() {
    // The Newark -> New York corridor crosses to NY at the GWB on I-95 (the
    // Holland Tunnel fix); driving it directly should complete and never
    // mention the tunnel.
    let mut harness = PlaytestHarness::new();
    harness.start_route("Newark", "New York", RouteSetup::default());
    harness.read_drive(|drive| assert_eq!(drive.trip.route.highways(), vec!["I-95".to_string()]));
    let result = harness.drive_delivery_to_completion();

    assert_eq!(result.deliveries, 1);
    assert_eq!(result.destination, "New York");
    assert_eq!(result.remaining_miles, 0.0);
    let text = result.transcript_text();
    assert!(!text.contains("Holland Tunnel"), "{text}");
    // State lines announce only when crossed; this short delivery finishes at
    // the terminal before its mapped crossing cue.
    assert!(!text.contains("New Jersey into New York"), "{text}");
}

// -- the menu walk to the wheel ----------------------------------------------------------
//
// Every case below drives `start_delivery`, which walks the real menus from
// the main menu to the wheel and needs the city screens to hand over the
// real drive.

#[test]
fn test_each_driving_mode_completes_a_full_spoken_delivery() {
    for (mode, time_scale) in [("relaxed", 10.0), ("standard", 20.0)] {
        let mut harness = PlaytestHarness::new();
        harness.app.ctx.settings.time_scale = time_scale;
        harness.start_delivery(StartDelivery::named(&format!("Harness {mode} Mode")));
        let result = harness.drive_delivery_to_completion();

        assert_eq!(result.deliveries, 1);
        assert_eq!(result.destination, result.current_city);
        assert!(result.transcript_text().contains("Dispatch routed you to"));
        assert!(result.transcript_text().to_lowercase().contains("arrived"));
        result.assert_no_known_destination_exit_regressions();
    }
}

#[test]
fn test_real_time_mode_starts_the_spoken_trip_clock_at_real_time() {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.time_scale = 1.0;
    harness.start_delivery(StartDelivery::named("Harness Real Time Clock"));

    let target = real_clock_game_hours(None);
    let calendar = harness
        .app
        .ctx
        .profile
        .as_ref()
        .unwrap()
        .calendar_game_hours();
    let local_start_hour = harness.read_drive(|d| d.trip.local_start_hour());
    assert_eq!(date_text(calendar), date_text(target));
    assert!((calendar - target).abs() < 1.0 / 60.0);
    assert!((local_start_hour - calendar.rem_euclid(24.0)).abs() < 1e-9);
}

#[test]
fn test_playtest_harness_records_headless_delivery_transcript() {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Harness Smoke"));
    let result = harness.drive_delivery_to_completion();

    let transcript = result.transcript_text();
    assert!(transcript.contains("Freight Fate"));
    assert!(transcript.contains("Dispatch routed you to"));
    assert!(transcript.to_lowercase().contains("arrived"));
    assert_eq!(result.deliveries, 1);
    result.assert_no_known_destination_exit_regressions();
}

#[test]
fn test_company_driver_first_delivery_transcript_builds_dispatch_trust() {
    let mut harness = PlaytestHarness::new();
    let result = harness.start_delivery(StartDelivery::named("Harness Training Arc"));

    // The training arc's wording is on the briefing and Career plan; the
    // drive itself must still never call a new hire a probationer.
    let text = result.transcript_text().to_lowercase();
    assert!(text.contains("dispatch"));
    assert!(!text.contains("probation"));
}

#[test]
fn test_new_hire_transcript_runs_assigned_load_and_route() {
    let mut harness = PlaytestHarness::new();
    let result = harness.start_delivery(StartDelivery::named("Harness New Hire"));

    let transcript = result.transcript_text();
    assert!(transcript.contains("Dispatch board. "));
    assert!(transcript.contains("Accept assigned dispatch:"));
    assert!(transcript.contains("Dispatch routed you to"));
    // The route menu never appears for a new company hire.
    assert!(!transcript.contains("Route planning to"));
    assert!(!transcript.contains("route option"));
}

#[test]
fn test_owner_operator_transcript_keeps_load_and_route_choice() {
    let mut harness = PlaytestHarness::new();
    let result = harness.start_delivery(StartDelivery::named("Harness Owner Choice").configure(
        |profile| {
            profile.business_status = LEASED_OWNER_OPERATOR.to_string();
            profile.achievements.push("first_day".to_string());
            // All trailer programs, so a specialty-heavy random board can
            // never leave the harness with zero unlocked jobs.
            profile.trailer_programs = ["dry_van", "reefer", "flatbed", "bulk"]
                .map(str::to_string)
                .to_vec();
        },
    ));

    let transcript = result.transcript_text();
    assert!(transcript.contains("dispatches available")); // browsable, not assigned
    assert!(!transcript.contains("Accept assigned dispatch:"));
    assert!(transcript.contains("Route planning to")); // route choice preserved
    assert!(transcript.contains("route option"));
    assert!(!transcript.contains("Dispatch routed you to"));
}

#[test]
fn test_mid_career_transcript_speaks_level_band_guidance() {
    let mut harness = PlaytestHarness::new();
    let result = harness.start_delivery(StartDelivery::named("Harness Senior Career").configure(
        |profile| {
            profile.achievements.push("first_day".to_string());
            profile.career.xp = LEVEL_XP[9];
            profile.career.deliveries = 20;
            profile.career.reputation = 86.0;
        },
    ));

    // The band's title and plan are on Career plan; the board marks the
    // load the band recommends.
    let text = result.transcript_text();
    assert!(text.contains("senior company lane"));
    assert!(!text.to_lowercase().contains("probation"));
}

/// How many times a line was ANNOUNCED, ignoring the pacer's rescue of it.
///
/// When driving events share the main voice, an interrupting main-channel
/// line cuts a ROUTE event line mid-sentence and the pacer hands the cut
/// line straight back to the voice to finish. That is one delivery
/// completing, not a second announcement -- but this harness records at the
/// voice, one rung below where the Python harness recorded (see the module
/// note), so it sees both halves. Collapse a verbatim re-delivery that
/// lands within a line or two of the original.
fn announcements_of(result: &freight_fate::playtest::PlaytestResult, needle: &str) -> usize {
    let mut count = 0;
    for (i, entry) in result.spoken.iter().enumerate() {
        if !entry.text.contains(needle) {
            continue;
        }
        let rescue = result.spoken[i.saturating_sub(3)..i]
            .iter()
            .any(|earlier| earlier.text == entry.text);
        if !rescue {
            count += 1;
        }
    }
    count
}

#[test]
fn test_speed_control_follows_job_from_deadhead_to_loaded_trip() {
    let mut harness = PlaytestHarness::new();
    let mut setup = StartDelivery::named("Speed Control Handoff");
    setup.arm_speed_control_on_deadhead = true;
    harness.start_delivery(setup);
    harness.read_drive(|drive| {
        assert!(drive.speed_control_armed);
        assert!(drive.keeper_mph.is_none());
        assert!(drive.cruise_mph.is_none());
    });
    harness.with_drive(|drive, ctx| {
        drive.truck_mut().set_air_ready(false);
        // Open-road resume now waits until the truck is at cruise's holding
        // speed before engaging, rather than snapping cruise on at a crawl and
        // flooring the throttle to chase the target (the resume-ramp fix).
        drive.truck_mut().velocity_mps = 25.0 / MPH_PER_MPS;
        drive.update_frame(ctx, 1.0 / 60.0);
    });
    harness.read_drive(|drive| {
        assert!(drive.keeper_mph.is_some() || drive.cruise_mph.is_some());
    });

    let transcript = harness.transcript_text();
    assert!(transcript.contains("Speed keeper holding"));
    assert_eq!(
        announcements_of(
            &harness.result(),
            "Automatic speed control paused for pickup"
        ),
        1
    );
    // And it comes back on its own after the pickup.
    let after = transcript
        .split("Automatic speed control paused for pickup")
        .last()
        .unwrap_or_default();
    assert!(
        [
            "Speed keeper holding",
            "Cruise resuming",
            "Speed control resuming"
        ]
        .iter()
        .any(|line| after.contains(line)),
        "{after}"
    );
}

#[test]
fn test_structured_transcript_preserves_channel_interrupt_and_order() {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Harness Structured Speech"));
    harness.app.ctx.say_event_with(
        "Harness event channel check",
        freight_fate::app::SayEvent::queued(),
    );

    let result = harness.result();
    let channels: std::collections::HashSet<SpeechChannel> =
        result.spoken.iter().map(|entry| entry.channel).collect();
    assert_eq!(
        channels,
        [SpeechChannel::Main, SpeechChannel::Event]
            .into_iter()
            .collect()
    );
    assert!(result.spoken.iter().any(|entry| entry.interrupt));
    assert!(!result.spoken.last().expect("something was said").interrupt);
    result.assert_ordered(&[
        "Freight Fate",
        "New career",
        "Dispatch board.",
        "Dispatch routed",
    ]);
    result.assert_screen_reader_friendly();
}

#[test]
fn test_name_entry_uses_real_space_key_and_preserves_accessible_name() {
    let mut harness = PlaytestHarness::new();
    let result = harness.start_delivery(StartDelivery::named("Harness Driver"));
    assert_eq!(
        harness.app.ctx.profile.as_ref().map(|p| p.name.as_str()),
        Some("Harness Driver")
    );
    assert!(result.transcript.iter().any(|line| line == "space"));
}

/// Whichever job and whichever route the player picks, the delivery
/// completes at its destination with nothing left to drive.
///
/// One test per `(job_rank, route_rank)` pair rather than a nested loop in
/// one test: twelve complete deliveries in a single `#[test]` are twelve
/// deliveries on one thread, and a failure named none of them. The pairs are
/// independent, so this is the same twelve drives spread over twelve
/// threads.
fn assert_delivery_completes(job_rank: usize, route_rank: usize) {
    let mut harness = PlaytestHarness::new();
    let mut setup = StartDelivery::named(&format!("Property {job_rank}-{route_rank}"));
    setup.job_rank = job_rank;
    setup.route_rank = route_rank;
    harness.start_delivery(setup);
    let result = harness.drive_delivery_to_completion();

    assert_eq!(result.deliveries, 1);
    assert_eq!(result.destination, result.current_city);
    assert_eq!(result.remaining_miles, 0.0);
    result.assert_no_known_destination_exit_regressions();
}

macro_rules! delivery_property_case {
    ($name:ident, $job_rank:expr, $route_rank:expr) => {
        #[test]
        fn $name() {
            assert_delivery_completes($job_rank, $route_rank);
        }
    };
}

delivery_property_case!(test_playtest_harness_delivery_properties_job0_route0, 0, 0);
delivery_property_case!(test_playtest_harness_delivery_properties_job0_route1, 0, 1);
delivery_property_case!(test_playtest_harness_delivery_properties_job0_route2, 0, 2);
delivery_property_case!(test_playtest_harness_delivery_properties_job1_route0, 1, 0);
delivery_property_case!(test_playtest_harness_delivery_properties_job1_route1, 1, 1);
delivery_property_case!(test_playtest_harness_delivery_properties_job1_route2, 1, 2);
delivery_property_case!(test_playtest_harness_delivery_properties_job2_route0, 2, 0);
delivery_property_case!(test_playtest_harness_delivery_properties_job2_route1, 2, 1);
delivery_property_case!(test_playtest_harness_delivery_properties_job2_route2, 2, 2);
delivery_property_case!(test_playtest_harness_delivery_properties_job3_route0, 3, 0);
delivery_property_case!(test_playtest_harness_delivery_properties_job3_route1, 3, 1);
delivery_property_case!(test_playtest_harness_delivery_properties_job3_route2, 3, 2);

#[test]
#[ignore = "needs the driver-tablet weather screen"]
fn test_playtest_harness_weather_shortcut_and_tablet_share_live_source() {}

#[test]
#[ignore = "needs the online journal transport seam"]
fn test_delivery_publication_is_queued_without_spoken_interruption() {}

#[test]
#[ignore = "needs tests/career_1_9_scenarios (the CAREER_STAGES presets) to be ported"]
fn test_reusable_career_stage_presets_reach_real_dispatch() {}

#[test]
#[ignore = "needs the drive's hazard pressure budget, which this port does not expose yet"]
fn test_mode_transcripts_prove_hazard_warning_and_recovery_pressure() {}

/// The two `SpeechProbe` cases sat in this file because they needed a live
/// app; they are covered where the delivery layer is, in
/// `tests/app_driving_speech_ladder.rs` and
/// `tests/app_main_channel_pacing.rs`, against the same seam.
#[test]
#[ignore = "covered by tests/app_main_channel_pacing.rs against the same seam"]
fn test_app_speech_dispatch_flushes_stale_main_speech_for_urgent_events() {}

#[test]
#[ignore = "covered by tests/app_speech_ducking.rs against the same seam"]
fn test_app_dedicated_event_voice_does_not_interrupt_main_speech() {}

/// A guard on the note above: the menu walk really does reach the wheel, so
/// no case below it may go back to being parked on the city screens.
#[test]
fn the_menu_walk_reaches_the_real_drive() {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Menu Walk"));
    assert!(harness.has_drive());
    assert!(harness.state_is::<freight_fate::states::driving::DrivingState>());
}
