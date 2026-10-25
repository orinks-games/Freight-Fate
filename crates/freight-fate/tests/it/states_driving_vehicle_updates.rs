use super::*;

// -- the frame loop's own machinery ---------------------------------------------------

#[test]
fn test_the_frame_loop_runs_a_whole_second_without_touching_the_truck() {
    // The smoke case the harness suite would otherwise be the first to find:
    // `update_frame` wires two dozen mixins together, and a parked truck with
    // the engine off must simply sit there through it.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let position = d.trip.position_mi;
    for _ in 0..60 {
        d.update_frame(&mut app.ctx, 1.0 / 60.0);
    }
    assert_eq!(d.trip.position_mi, position);
    assert_eq!(d.trip.truck.damage_pct, 0.0);
    assert!(!d.trip.truck.engine_on);
}

#[test]
fn test_the_retarder_trace_writes_one_line_per_change() {
    // The trace is a transcript line, not a per-frame log: the stage it
    // last wrote is what stops it repeating.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.engine_brake_stage = 2;
    d.trace_engine_brake();
    assert_eq!(d.traced_jake_stage, 2);
    d.trace_engine_brake();
    assert_eq!(d.traced_jake_stage, 2);
    d.trip.truck.engine_brake_stage = 0;
    d.trace_engine_brake();
    assert_eq!(d.traced_jake_stage, 0);
}

#[test]
fn test_air_ready_announces_once_while_the_parking_brake_is_set() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.start_engine();
    d.trip.truck.set_parking_brake();
    d.trip.truck.set_air_ready(true);
    app.clear_speech();
    d.update_air_brake_announcements(&mut app.ctx, true, false, false, false);
    let said = app.event_lines();
    assert_eq!(said.len(), 1);
    assert_eq!(said[0], "Air ready.");
    // A second pass with the flag already set says nothing more.
    d.update_air_brake_announcements(&mut app.ctx, true, false, false, false);
    assert_eq!(app.event_lines().len(), 1);
}

#[test]
fn test_the_air_brake_lockout_says_why_the_truck_will_not_roll() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    d.maybe_say_air_brake_lockout(&mut app.ctx);
    let said = app.event_lines();
    assert_eq!(said.len(), 1);
    assert_eq!(said[0], "Engine off.");
    // The cue timer holds it off for four seconds, however hard the driver
    // leans on the accelerator.
    d.maybe_say_air_brake_lockout(&mut app.ctx);
    assert_eq!(app.event_lines().len(), 1);
}

#[test]
fn test_the_direction_change_needs_a_fresh_press_held_at_a_standstill() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.start_engine();
    d.trip.truck.transmission.automatic = true;
    d.trip.truck.velocity_mps = 0.0;
    // A press that predates the stop never arms: the edge is what counts.
    d.reverse_brake_held = true;
    let backing = d.update_reverse_controls(&mut app.ctx, false, true, true, true, 0.1);
    assert!(!backing);
    assert_eq!(d.direction_armed, "");
    // A fresh press arms it, and the hold engages reverse.
    d.reverse_brake_held = false;
    d.update_reverse_controls(&mut app.ctx, false, true, false, true, 0.0);
    assert_eq!(d.direction_armed, "reverse");
    let mut engaged = false;
    for _ in 0..20 {
        if d.update_reverse_controls(&mut app.ctx, false, true, false, true, 0.1) {
            engaged = true;
            break;
        }
    }
    assert!(engaged);
    assert!(d.trip.truck.transmission.in_reverse());
}

#[test]
fn test_a_confirm_tap_at_the_yard_just_brakes() {
    // Owner-hit on 2026-07-14: a screen-reader driver checking the truck is
    // holding must never find themselves in reverse for it.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.start_engine();
    d.trip.truck.transmission.automatic = true;
    d.trip.truck.velocity_mps = 0.0;
    d.update_reverse_controls(&mut app.ctx, false, true, false, true, 0.0);
    assert_eq!(d.direction_armed, "reverse");
    // Let go well inside the hold: the arm dies with the press.
    d.update_reverse_controls(&mut app.ctx, false, false, false, false, 0.2);
    assert_eq!(d.direction_armed, "");
    assert!(!d.trip.truck.transmission.in_reverse());
}

#[test]
fn test_the_grade_advisory_stays_quiet_on_terse_speech() {
    // The G key answers on demand, so an advisory nobody asked for is
    // exactly what terse exists to remove.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.driving_speech = "terse".to_string();
    d.trip.truck.velocity_mps = mph_to_mps(60.0);
    app.clear_speech();
    for _ in 0..40 {
        d.trip.position_mi += 0.2;
        d.update_grade_advisory(&mut app.ctx);
    }
    assert!(app.event_lines().is_empty());
    assert_eq!(d.grade_warned_sign, 0);
}

#[test]
fn test_the_hazard_budget_leaves_the_driver_their_own_window() {
    // Built forward from the moment the assist must act, so speed, grade and
    // brake heat come out of the truck's time rather than the driver's.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.velocity_mps = mph_to_mps(65.0);
    d.hazard_dodgeable = false;
    d.hazard_in_lane = false;
    let window = 4.0;
    let deadline = d.hazard_deadline_for(window, None);
    assert!(deadline >= window);
    assert!((deadline - (d.aeb_engage_s(d.hazard_target_mph(None)) + window)).abs() < 1e-9);
    // An object in the lane asks for nearly a stop; a lane to take instead
    // buys the driver the time that move costs. They are separate additions
    // now, so the test asks for them separately.
    let in_lane_only = d.hazard_deadline_for(
        window,
        Some(HazardShape {
            dodgeable: false,
            in_lane: true,
            lead_mph: None,
            brake_to_mph: None,
        }),
    );
    assert!(
        in_lane_only > deadline,
        "the near stop takes longer than 25"
    );
    let dodgeable = d.hazard_deadline_for(
        window,
        Some(HazardShape {
            dodgeable: true,
            in_lane: true,
            lead_mph: None,
            brake_to_mph: None,
        }),
    );
    assert!((dodgeable - (in_lane_only + LANE_TAP_CHANGE_S)).abs() < 1e-9);
}

#[test]
fn test_braking_below_the_hazard_speed_clears_it_and_says_so() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.hazard_deadline = Some(5.0);
    d.hazard_names = vec!["the deer".to_string()];
    d.hazard_dodgeable = false;
    d.trip.truck.velocity_mps = mph_to_mps(10.0);
    app.clear_speech();
    d.update_hazard(&mut app.ctx, 1.0 / 60.0);
    assert!(d.hazard_deadline.is_none());
    assert!(d.hazard_names.is_empty());
    assert!(app
        .event_lines()
        .iter()
        .any(|line| line == "Clear of the deer."));
}

#[test]
fn test_the_hazard_assist_holds_one_application_rather_than_fanning_it() {
    // Deciding the pedal afresh every frame is what emptied the tanks: the
    // assist's own braking retreats the threshold that engaged it.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.automatic_emergency_braking = true;
    d.trip.truck.start_engine();
    d.trip.truck.velocity_mps = mph_to_mps(65.0);
    d.hazard_dodgeable = false;
    d.hazard_deadline = Some(0.5); // inside the engage budget already
    d.update_hazard(&mut app.ctx, 1.0 / 60.0);
    assert_eq!(d.aeb_brake, 1.0);
    assert!(d.automatic_braking_announced);
    // The application stays on while the hazard is live.
    d.trip.truck.velocity_mps = mph_to_mps(40.0);
    d.update_hazard(&mut app.ctx, 1.0 / 60.0);
    assert_eq!(d.aeb_brake, 1.0);
    // Releasing hands the pedal back and forgets what the stop measured.
    d.release_hazard_brake();
    assert_eq!(d.aeb_brake, 0.0);
    assert!(!d.automatic_braking_announced);
}

#[test]
fn test_traction_states_speak_once_on_the_edge_they_begin() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.chains_on = true;
    d.trip.truck.velocity_mps = mph_to_mps(45.0);
    app.clear_speech();
    d.update_traction_cues(&mut app.ctx);
    assert!(d.chains_fast_active);
    let first = app.event_lines().len();
    assert!(first >= 1);
    d.update_traction_cues(&mut app.ctx);
    assert_eq!(app.event_lines().len(), first);
    // Slowing back under the chain speed re-arms it for the next excursion.
    d.trip.truck.velocity_mps = mph_to_mps(10.0);
    d.update_traction_cues(&mut app.ctx);
    assert!(!d.chains_fast_active);
}

#[test]
fn test_the_live_weather_switch_only_moves_when_the_setting_does() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    assert!(!d.weather_source_real);
    app.ctx.settings.real_weather = true;
    d.sync_weather_source(&mut app.ctx);
    assert!(d.weather_source_real);
    assert!(d.trip.weather.provider.is_some());
    app.ctx.settings.real_weather = false;
    d.sync_weather_source(&mut app.ctx);
    assert!(!d.weather_source_real);
    assert!(d.trip.weather.provider.is_none());
    assert!(!d.trip.weather.live);
}

#[test]
fn test_the_shift_clock_runs_on_game_time_while_the_truck_rolls() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    profile_mut_of(&mut app.ctx).fatigue = 0.0;
    d.trip.truck.start_engine();
    d.trip.truck.velocity_mps = mph_to_mps(60.0);
    let before = hos_mut_of(&mut app.ctx).driving_min;
    for _ in 0..60 {
        d.update_hours_and_fatigue(&mut app.ctx, 1.0 / 60.0);
    }
    assert!(hos_mut_of(&mut app.ctx).driving_min > before);
    assert!(profile_mut_of(&mut app.ctx).fatigue > 0.0);
}

// -- the air system's voice (test_driving_features.py) --------------------------------

/// `step(psi)` from the low-air regressions: the reading the frame loop
/// would have taken before the pressure moved.
fn air_step(d: &mut DrivingState, app: &mut TestApp, psi: f64) {
    let was_low = d.trip.truck.air_low_warning();
    let was_spring = d.trip.truck.spring_brakes_active();
    let engine_on = d.trip.truck.engine_on;
    d.trip.truck.set_air_pressure_psi(psi);
    d.update_air_brake_announcements(&mut app.ctx, engine_on, false, was_low, was_spring);
}

#[test]
fn test_low_air_warning_does_not_repeat_while_bouncing_near_threshold() {
    // Regression for the tester report: heavy/repeated service braking makes
    // pressure hover right around the 60 psi threshold while the compressor
    // catches up. Each dip below 60 must not re-fire the full warning line as
    // long as pressure never climbs back out to the hysteresis clear point.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.engine_on = true;
    d.trip.truck.set_air_pressure_psi(125.0);
    d.low_air_said = false;
    app.clear_speech();

    // First dip below the warning threshold: exactly one warning.
    air_step(&mut d, &mut app, 55.0);
    assert_eq!(app.event_lines().len(), 1);
    assert!(app.event_lines()[0].starts_with("Low air warning"));

    // Repeated braking bounces pressure just below and just above 60 psi,
    // but never clears the 68 psi hysteresis band. None of this may re-fire
    // the warning.
    for psi in [58.0, 61.0, 59.0, 62.0, 57.0, 63.0, 60.0, 56.0] {
        air_step(&mut d, &mut app, psi);
    }
    assert_eq!(app.event_lines().len(), 1);
}

#[test]
fn test_low_air_warning_rearms_after_recovering_above_clear_threshold() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // Python stubbed `say_event`; the real pacer would treat an identical
    // repeat inside its own window as a repeat. Step the clock between lines
    // so what is asserted is this module's latch, not the pacer's.
    app.ctx.event_pacer = hearken::speech::EventSpeechPacer::with_clock(
        freight_fate::app::FF_PACER,
        stepping_clock(30.0),
    );
    d.trip.truck.engine_on = true;
    d.trip.truck.set_air_pressure_psi(125.0);
    d.low_air_said = false;
    app.clear_speech();

    air_step(&mut d, &mut app, 55.0); // first dip: warns once
    assert_eq!(app.event_lines().len(), 1);

    air_step(&mut d, &mut app, 63.0); // ticks back up but stays inside the band
    air_step(&mut d, &mut app, 55.0); // dips again: still must not re-warn
    assert_eq!(app.event_lines().len(), 1);

    air_step(&mut d, &mut app, 70.0); // genuinely recovers clear of 68 psi
    air_step(&mut d, &mut app, 55.0); // dips again: a fresh low-air event
    assert_eq!(app.event_lines().len(), 2);
    assert!(app.event_lines()[1].starts_with("Low air warning"));
}

#[test]
fn test_spring_brake_warning_bypasses_low_air_cooldown() {
    // A genuinely worsening situation must still warn immediately even while
    // the low-air warning's latch is active from an earlier, milder dip --
    // escalation to spring brakes is its own event.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    // Python stubbed `say_event`; the real pacer would treat an identical
    // repeat inside its own window as a repeat. Step the clock between lines
    // so what is asserted is this module's latch, not the pacer's.
    app.ctx.event_pacer = hearken::speech::EventSpeechPacer::with_clock(
        freight_fate::app::FF_PACER,
        stepping_clock(30.0),
    );
    d.trip.truck.engine_on = true;
    d.trip.truck.set_air_pressure_psi(125.0);
    d.low_air_said = false;
    d.spring_brake_said = false;
    app.clear_speech();

    air_step(&mut d, &mut app, 55.0); // low-air warning fires and latches
    assert_eq!(app.event_lines().len(), 1);
    assert!(app.event_lines()[0].starts_with("Low air warning"));

    // Pressure keeps falling, straight through into spring-brake range,
    // without ever recovering above the low-air clear threshold.
    air_step(&mut d, &mut app, 35.0);
    assert_eq!(app.event_lines().len(), 2);
    assert!(app.event_lines()[1].starts_with("Spring brakes applied"));
}

#[test]
fn test_sustained_redline_speaks_the_wear_it_is_actually_causing() {
    // Over-revving charges ENGINE WEAR, not incident damage. This warning
    // used to read damage_pct, which for most drivers sits at zero, so it
    // announced "taking damage, now 0 percent" while real harm piled up on a
    // meter it never mentioned.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let tape = CueAudio::install(&mut app);
    let redline_mps;
    {
        let t = &mut d.trip.truck;
        t.engine_on = true;
        t.transmission.gear = 1;
        let ratio = t.transmission.ratio_for(1).abs();
        let wheel_rps = (t.specs.max_rpm * 1.1) / (60.0 * ratio);
        redline_mps = wheel_rps * 2.0 * std::f64::consts::PI * t.specs.wheel_radius_m;
        t.velocity_mps = redline_mps;
        t.rpm = t.specs.max_rpm;
        t.engine_wear_pct = 12.0;
    }
    // Python stubbed `say_event`; the real pacer would treat an identical
    // repeat inside its own window as a repeat. Step the clock between lines
    // so what is asserted is this module's latch, not the pacer's.
    app.ctx.event_pacer = hearken::speech::EventSpeechPacer::with_clock(
        freight_fate::app::FF_PACER,
        stepping_clock(30.0),
    );
    app.clear_speech();

    d.update_overrev(&mut app.ctx, 1.0); // inside the grace period: a shift flare
    assert!(app.event_lines().is_empty());

    d.update_overrev(&mut app.ctx, 1.0); // sustained past the grace: warn
    let said = app.event_lines().last().cloned().unwrap_or_default();
    assert!(said.to_lowercase().contains("redline"));
    assert!(said.contains("12 percent"));
    assert!(said.to_lowercase().contains("engine wear"));
    assert!(tape.keys().iter().any(|key| key == "ui/warning"));

    app.clear_speech();
    d.update_overrev(&mut app.ctx, 5.0); // repeat interval not reached yet
    assert!(app.event_lines().is_empty());
    // Python stubbed `say_event`, so its repeat spoke an identical line. The
    // real keyed condition earns the voice only when the number it carries
    // has moved -- which is exactly what redline does to engine wear.
    d.trip.truck.engine_wear_pct = 13.0;
    d.update_overrev(&mut app.ctx, 6.0); // past it: nag again while wear accrues
    assert_eq!(app.event_lines().len(), 1);

    app.clear_speech();
    // easing off resets the whole cycle
    d.trip.truck.velocity_mps = 0.0;
    d.trip.truck.rpm = d.trip.truck.specs.idle_rpm;
    d.update_overrev(&mut app.ctx, 1.0);
    assert_eq!(d.overrev_s, 0.0);
    d.trip.truck.velocity_mps = redline_mps;
    d.trip.truck.rpm = d.trip.truck.specs.max_rpm;
    d.update_overrev(&mut app.ctx, 1.0); // back at redline, but within fresh grace
    assert!(app.event_lines().is_empty());
}

// -- the continuous soundscape (test_driving_features.py) -----------------------------

#[test]
fn test_reverse_audio_cue_loops_while_reverse_is_engaged() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let tape = CueAudio::install(&mut app);
    d.trip.truck.start_engine();
    d.trip.truck.transmission.gear = ff_core::sim::transmission::REVERSE;
    tape.clear_reverse();

    d.update_audio(&mut app.ctx, 0.0);
    d.update_audio(&mut app.ctx, 0.0);
    assert_eq!(tape.reverse(), vec!["start"]);

    d.trip.truck.transmission.gear = 1;
    d.update_audio(&mut app.ctx, 0.0);
    assert_eq!(tape.reverse(), vec!["start", "stop"]);

    d.trip.truck.transmission.gear = ff_core::sim::transmission::REVERSE;
    d.update_audio(&mut app.ctx, 0.0);
    assert_eq!(tape.reverse(), vec!["start", "stop", "start"]);
}

#[test]
fn test_air_fill_loop_plays_until_governor_release() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let tape = CueAudio::install(&mut app);
    d.trip.truck.set_cold_air_start();
    d.trip.truck.start_engine();
    d.trip.truck.velocity_mps = 0.0;
    tape.clear_loops();

    d.update_audio(&mut app.ctx, 0.0);
    d.update_audio(&mut app.ctx, 0.0); // still building: the loop must not restack
    assert_eq!(
        air_loops(&tape),
        vec![("start", CH_AIR, "vehicle/air_pressurize".to_string())]
    );

    d.trip.truck.set_air_ready(true); // governor release
    d.update_audio(&mut app.ctx, 0.0);
    assert_eq!(air_loops(&tape).last().expect("a loop call").0, "stop");

    // Routine braking dips just under the 100 psi line constantly; the fill
    // hiss must NOT flutter back in for those (hysteresis).
    tape.clear_loops();
    d.trip.truck.set_air_pressure_psi(97.0);
    d.update_audio(&mut app.ctx, 0.0);
    assert!(air_loops(&tape).is_empty());

    // A genuinely low air system still brings the fill loop back.
    d.trip.truck.set_air_pressure_psi(88.0);
    d.update_audio(&mut app.ctx, 0.0);
    assert_eq!(
        air_loops(&tape),
        vec![("start", CH_AIR, "vehicle/air_pressurize".to_string())]
    );
}

fn air_loops(tape: &AudioTape) -> Vec<LoopCall> {
    tape.loops()
        .into_iter()
        .filter(|(_, channel, _)| *channel == CH_AIR)
        .collect()
}

#[test]
fn test_road_joint_thumps_pause_off_highway() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let tape = CueAudio::install(&mut app);
    d.road_joint_accumulator_m = 0.0;
    d.next_joint_distance_m = 15.0;
    d.trip.on_ramp = true;
    d.trip.truck.velocity_mps = 20.0;

    d.update_audio(&mut app.ctx, 1.0);

    assert_eq!(d.road_joint_accumulator_m, 0.0);
    assert!(!tape.keys().iter().any(|key| key == "vehicle/road_joint"));
}

#[test]
fn test_auto_jake_manages_stages_on_an_automatic_box() {
    // The stage controller's half of the Python case; arming it (`J`) and
    // the manual stage pick belong to `driving_controls`.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    {
        let t = &mut d.trip.truck;
        t.set_air_ready(false);
        t.start_engine();
        t.transmission.automatic = true;
        t.transmission.gear = 8;
        t.velocity_mps = mph_to_mps(55.0);
        t.rpm = 1400.0;
        t.throttle = 0.0;
        t.grip = 1.0;
        t.engine_brake_stage = 1;
    }
    d.auto_jake = true;
    d.auto_jake_hold_mph = Some(55.0);

    // Gaining on the hold speed: the controller climbs the stages, one
    // rate-limited step at a time.
    d.trip.truck.velocity_mps = mph_to_mps(60.0);
    d.update_auto_jake(&mut app.ctx, 2.0);
    assert_eq!(d.trip.truck.engine_brake_stage, 2);
    d.update_auto_jake(&mut app.ctx, 2.0);
    assert_eq!(d.trip.truck.engine_brake_stage, 3);

    // Over-slowed on level road: the retarder comes all the way off, at once.
    // A retarder is for holding a truck BACK, and a truck seven under its own
    // number on flat ground needs no holding -- walking down a stage at a time
    // was what kept two cylinders cut for the rest of the drive.
    d.trip.truck.velocity_mps = mph_to_mps(48.0);
    d.update_auto_jake(&mut app.ctx, 2.0);
    assert_eq!(d.trip.truck.engine_brake_stage, 0);

    // On a real grade the ladder is still a ladder: there the retarder IS what
    // is keeping the number, so an over-slowed truck gives back one stage at a
    // time rather than dropping the whole hill onto the drums.
    force_grade(&mut d.trip, -0.06);
    d.trip.truck.engine_brake_stage = 3;
    d.auto_jake_cooldown_s = 0.0;
    d.trip.truck.velocity_mps = mph_to_mps(48.0);
    d.update_auto_jake(&mut app.ctx, 2.0);
    assert_eq!(d.trip.truck.engine_brake_stage, 2);
    d.update_auto_jake(&mut app.ctx, 2.0);
    assert_eq!(d.trip.truck.engine_brake_stage, 1);
    force_grade(&mut d.trip, 0.0);
    d.trip.truck.engine_brake_stage = 3;

    // Ice arrives: the stage collapses to what the drives can hold.
    d.trip.truck.velocity_mps = mph_to_mps(60.0);
    d.trip.truck.grip = 0.15;
    d.trip.truck.transmission.gear = 5;
    d.trip.truck.rpm = 1900.0;
    d.update_auto_jake(&mut app.ctx, 2.0);
    assert!(
        d.trip.truck.engine_brake_stage <= d.auto_jake_max_stage()
            || d.trip.truck.engine_brake_stage == 1
    );
}

#[test]
fn test_the_descent_advisory_names_controls_the_driver_actually_has() {
    // An automatic has no gear selection, so "pick your gear" names nothing.
    // W, Q, N and Backspace are all gated on a manual box. What an automatic
    // driver has is the brake, which is exactly what puts their transmission
    // in a lower gear -- so that is what the advisory tells them to use.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.transmission.automatic = true;
    let said = d.descend_advice(&mut app.ctx);
    assert!(!said.to_lowercase().contains("pick your gear"));
    assert!(said.starts_with("Set the jake with"));

    // A manual box hears the same: the jake is the action.
    d.trip.truck.transmission.automatic = false;
    assert!(d
        .descend_advice(&mut app.ctx)
        .starts_with("Set the jake with"));

    // With the jake already on, J turns it OFF: the agent drive into Edwards
    // (2026-09-24) heard "Set the engine brake with J", pressed it, and got
    // "Jake off." So the jake clause goes, and an automatic has nothing left
    // to be told.
    d.trip.truck.engine_brake_stage = 2;
    assert_eq!(
        d.descend_advice(&mut app.ctx),
        "Pick your gear before it starts."
    );
    d.trip.truck.transmission.automatic = true;
    assert_eq!(d.descend_advice(&mut app.ctx), "");
    d.trip.truck.engine_brake_stage = 0;
    d.auto_jake = true; // armed, sitting at stage zero on the flat
    assert_eq!(d.descend_advice(&mut app.ctx), "");
}

#[test]
fn test_the_descent_advisory_stops_teaching_once_the_jake_is_known() {
    // Heard on every downgrade, "Set the engine brake with J" got old for a
    // driver who already uses it (player report, 2026-10). Switching the jake
    // on a few times retires it; the grade line itself stays.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.transmission.automatic = true;
    assert!(d.descend_advice(&mut app.ctx).starts_with("Set the jake"));
    for _ in 0..ff_core::spoken_advice::RETIRE_AFTER {
        d.toggle_engine_brake(&mut app.ctx); // on
        d.toggle_engine_brake(&mut app.ctx); // off
    }
    assert_eq!(d.descend_advice(&mut app.ctx), "");
    d.trip.truck.transmission.automatic = false;
    // The jake is the same key on a manual box, so it stays learned (owner,
    // 2026-10-25: a switch of transmission used to teach it three more times).
    assert_eq!(d.descend_advice(&mut app.ctx), "");
}

// -- ignored: the roadside screens a settled stop pushes ------------------------------
//
// `driving_rest_states` has not landed, so `TrafficStopState`,
// `EnforcementStopState` and `FelonyStopState` are still stubs in
// `driving_updates::pending`. The bodies below are the Python cases, ready to
// run the moment those screens exist.

#[test]
fn test_a_clean_stop_opens_the_traffic_stop_screen() {
    // `test_speeding_consequences.test_being_seen_is_the_only_thing_that_costs_money`:
    // once the truck is stopped the encounter hands off to the roadside
    // screen, which is what actually charges the ticket.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.velocity_mps = 0.0;
    d.begin_pull_over(&mut app.ctx, 55.0);
    d.update_pull_over(&mut app.ctx, 1.0 / 60.0, true);
    assert!(d.pull_over.is_none());
    assert!(!d.trip.pull_over_active);
    app.ctx.run_deferred();
    assert!(
        app.ctx
            .state()
            .is_some_and(|s| s.borrow().as_any().is::<TrafficStopState>()),
        "the roadside screen is what charges the ticket"
    );
}

#[test]
fn test_running_from_the_stop_is_conduct() {
    // `update_pursuit_by_conduct`: holding highway speed, unbraked, past the
    // final warning is the road to a felony, and it pushes `FelonyStopState`.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.begin_pull_over(&mut app.ctx, 55.0);
    d.pull_over_grace_s = 0.0;
    for _ in 0..(60.0 * 60.0) as i32 {
        d.trip.truck.velocity_mps = mph_to_mps(60.0);
        d.update_pull_over(&mut app.ctx, 1.0 / 60.0, false);
        if d.pull_over.is_none() {
            break;
        }
    }
    assert!(d.pull_over.is_none());
    app.ctx.run_deferred();
    assert!(
        app.ctx
            .state()
            .is_some_and(|s| s.borrow().as_any().is::<FelonyStopState>()),
        "holding speed through the final warning is the road to a felony"
    );
}

// -- the doubled assist line on a hot exit ramp --------------------------------------
//
// `tests/test_driving_features.py::test_a_hot_ramp_speaks_one_assist_line_not_two`
// and `::test_a_silent_ramp_engagement_never_leaves_a_lone_release`.

/// One mainline bend the truck is sitting in, for the case that must still
/// speak (`SimpleNamespace(...)` on the Python side).
fn a_bend_here(at_mi: f64) -> RouteCurve {
    RouteCurve {
        start_mi: at_mi,
        apex_mi: at_mi,
        end_mi: at_mi + 0.1,
        direction: 'L',
        advisory_mph: 35,
        min_radius_ft: 1000,
        deflection_deg: 40.0,
        connector: false,
    }
}

#[test]
fn test_a_hot_ramp_speaks_one_assist_line_not_two() {
    // On a ramp, route-transition assistance owns the speech.
    //
    // A ramp adds 0.35 of curve weight, so any exit taken over about 43 mph
    // engages curve speed assistance too -- and with the realistic preset both
    // assists are on, so every hot ramp spoke twice back to back (logged
    // playtest of the four 1.9 assists, 2026-07-15). The braking is unchanged;
    // the line that survives is the one that names what it is braking for.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.curve_speed_assist = true;
    // `monkeypatch.setattr(driving.trip, "curve_at", lambda mile: None)`: no
    // baked bend under the truck, so the assist reaches its ramp heuristic.
    d.trip.curves = Vec::new();
    app.clear_speech();

    // On the ramp: fast enough that the ramp's own curve weight engages the
    // curve assist through its heuristic branch.
    d.ramp_mi = Some(d.trip.position_mi);
    for _ in 0..30 {
        d.trip.truck.velocity_mps = 55.0 * 0.44704;
        d.update_lane(&mut app.ctx, 1.0 / 60.0);
    }
    assert!(
        !app.event_lines()
            .iter()
            .any(|text| text.contains("Curve assistance")),
        "the ramp's own assist speaks for a ramp; the curve cue must not double it"
    );

    // Off the ramp, the same overspeed still announces itself normally.
    d.ramp_mi = None;
    d.curve_assist_cue_s = 0.0;
    d.curve_assist_active = false;
    d.trip.curves = vec![a_bend_here(d.trip.position_mi)];
    for _ in 0..30 {
        d.trip.truck.velocity_mps = 55.0 * 0.44704;
        d.update_lane(&mut app.ctx, 1.0 / 60.0);
    }
    assert!(
        app.event_lines()
            .iter()
            .any(|text| text.contains("Slowing for the bend.")),
        "silencing the ramp case must not silence a real mainline bend"
    );
}

#[test]
fn test_a_silent_ramp_engagement_never_leaves_a_lone_release() {
    // The release line is paired to the slowing line that opened it.
    //
    // Suppressing the ramp's engagement cue would otherwise leave "Curve
    // assistance released." hanging on its own with nothing before it, which
    // reads as a bug to anyone listening.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.curve_speed_assist = true;
    d.trip.curves = Vec::new();
    app.clear_speech();

    d.ramp_mi = Some(d.trip.position_mi);
    for _ in 0..30 {
        d.trip.truck.velocity_mps = 55.0 * 0.44704;
        d.update_lane(&mut app.ctx, 1.0 / 60.0);
    }
    // Slow down so the assist disengages while still on the ramp.
    d.curve_assist_cue_s = 0.0;
    for _ in 0..30 {
        d.trip.truck.velocity_mps = 20.0 * 0.44704;
        d.update_lane(&mut app.ctx, 1.0 / 60.0);
    }

    assert!(
        !app.event_lines()
            .iter()
            .any(|text| text.contains("Curve assistance")),
        "a run that never spoke must not announce its own release"
    );
}

#[test]
fn test_full_lane_changes_use_blinker_and_keep_crossing_confirmation() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let tape = CueAudio::install(&mut app);
    app.ctx.settings.lane_keeping = "full".into();
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = 25.0;
    d.trip.zones.clear();
    d.lane.lane_count = 2;
    d.lane.lane = 0;
    for (direction, target, pan) in [(1, 1, -0.6), (-1, 0, 0.6)] {
        tape.clear();
        app.clear_speech();
        d.tap_lane_change(&mut app.ctx, direction);
        assert_eq!(tape.last(), ("vehicle/turn_signal".into(), 0.8, pan));
        d.update_tap_lane_change(&mut app.ctx, 0.6);
        assert_eq!(tape.last(), ("vehicle/turn_signal".into(), 0.8, pan));
        assert!(app.ctx.audio.cue_held("vehicle/turn_signal"));
        for _ in 0..40 {
            d.update_tap_lane_change(&mut app.ctx, 0.1);
        }
        assert_eq!(d.lane.lane, target);
        assert_eq!(d.lane_change_target, None);
        assert!(!app.ctx.audio.cue_held("vehicle/turn_signal"));
        assert!(tape.keys().contains(&"vehicle/lane_line_cross".into()));
        assert!(!tape.keys().contains(&SIGNAL.into()));
        assert!(app.event_lines().iter().any(|line| line.contains("In the")));
        let count = tape.calls().len();
        d.update_tap_lane_change(&mut app.ctx, 2.0);
        assert_eq!(tape.calls().len(), count);
    }
}
