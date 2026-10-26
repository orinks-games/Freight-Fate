// -- the R key: route status (test_info_keys.py) --------------------------------------
//
// These eleven were stubbed in `app_info_keys.rs` and are written out here,
// where the drive helper already empties the road and pins the sky.

#[test]
fn test_route_key_reports_progress_then_road_state_and_destination() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.position_mi = 40.0;
    d.trip.zones = vec![Zone::new(35.0, 45.0, 45.0, "construction")];
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &key(Key::R));

    // Two short sentences and nothing else: the grade, the zone, the nearest
    // named place, and the next maneuver all have their own key.
    let pct = d.trip.progress_percent();
    assert_eq!(
        last(&app),
        format!(
            "{pct} percent there, 34 miles left. On I-90 East in New York, toward Rochester, \
             New York."
        )
    );
}

#[test]
fn test_route_key_counts_down_to_a_planned_stop_instead_of_the_destination() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.position_mi = 20.0;
    let (at_mi, stop_key, spoken_name) = {
        let stop = d
            .trip
            .stops
            .iter()
            .find(|s| s.at_mi > d.trip.position_mi)
            .expect("the corridor has a stop ahead");
        (stop.at_mi, stop.key(), stop.spoken_name())
    };
    d.trip.planned_stop_key = Some(stop_key);
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &key(Key::R));

    let report = last(&app);
    let ahead = spoken_closing_distance(at_mi - d.trip.position_mi, d.trip.imperial());
    assert!(
        report.contains(&format!("{ahead} to {spoken_name}.")),
        "{report}"
    );
    assert!(!report.contains("left."), "{report}");
    assert!(
        report.contains("On I-90 East in New York, toward Rochester, New York."),
        "{report}"
    );
}

#[test]
fn test_route_key_falls_back_to_the_destination_once_the_plan_is_behind() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    let (at_mi, stop_key, spoken_name) = {
        let stop = d.trip.stops.first().expect("the corridor has a stop");
        (stop.at_mi, stop.key(), stop.spoken_name())
    };
    d.trip.planned_stop_key = Some(stop_key);
    d.trip.position_mi = at_mi + 1.0;
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &key(Key::R));

    let report = last(&app);
    let remaining = d.trip.distance_text(d.trip.remaining_miles());
    assert!(report.contains(&format!("{remaining} left.")), "{report}");
    assert!(!report.contains(&spoken_name), "{report}");
}

#[test]
fn test_route_key_reports_reverse_route_direction() {
    let mut app = TestApp::new();
    let mut d = a_drive_between(&mut app, "Rochester", "Buffalo", "company yard");
    d.trip.position_mi = 34.8;
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &key(Key::R));

    assert!(
        last(&app).contains("On I-90 West in New York, toward Buffalo, New York"),
        "{}",
        last(&app)
    );
}

#[test]
fn test_route_key_uses_metric_distances() {
    let mut app = TestApp::new();
    app.ctx.settings.imperial_units = false;
    let mut d = a_drive(&mut app);
    d.trip.set_imperial(false);
    d.trip.position_mi = 20.0;
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &key(Key::R));

    let report = last(&app);
    assert!(report.contains("87 kilometers left."), "{report}");
    assert!(!report.contains(" miles"), "{report}");
}

#[test]
fn test_route_key_answers_with_the_gate_on_the_facility_approach() {
    // After the destination exit, R describes the approach, not the dead
    // highway (playtest 2026-07-22: "on I-90 West, 3 miles remaining" with a
    // frozen countdown while rolling city streets toward the gate).
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.destination_exit_taken = true;
    d.trip.position_mi = d.trip.total_miles() - 2.0;
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &key(Key::R));

    let report = last(&app);
    assert!(
        report.starts_with("Off the highway, on the facility approach"),
        "{report}"
    );
    assert!(!report.contains("I-90"), "{report}");
    assert!(!report.contains("into the trip"), "{report}");
}

#[test]
fn test_route_key_counts_the_ramp_down_after_the_destination_exit() {
    // The mainline odometer freezes once the exit is taken (the ramp
    // consumes the movement), and R used to keep speaking that frozen
    // remainder: Tim heard "4 miles to go" four identical times over a
    // minute, the last one three seconds after he was already at the gate,
    // and braked hard for it (tester report, 2026-08-30). R must speak the
    // ramp's own countdown, and the countdown must move.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.destination_exit_taken = true;
    // The frozen mainline remainder Tim kept hearing.
    d.trip.position_mi = d.trip.total_miles() - 4.0;
    d.ramp_mi = Some(0.5);
    app.clear_speech();
    d.handle_key_event(&mut app.ctx, &key(Key::R));
    let far = last(&app);
    assert!(far.to_lowercase().contains("half a mile to"), "{far}");
    assert!(!far.contains("4 miles"), "{far}");

    d.ramp_mi = Some(0.1);
    app.clear_speech();
    d.handle_key_event(&mut app.ctx, &key(Key::R));
    let near = last(&app);
    assert!(near.contains("feet to"), "{near}");
    assert_ne!(far, near, "the countdown must move as the truck does");
}

#[test]
fn test_route_key_answers_with_the_gate_when_the_route_has_ended() {
    // Rolled past the gate: R agrees with the S key's gate override.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.destination_exit_taken = true;
    d.trip.position_mi = d.trip.total_miles();
    d.trip.finished = true;
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &key(Key::R));

    let report = last(&app);
    assert!(
        report.starts_with("You have arrived"),
        "{report}"
    );
    assert!(report.contains("Stop to dock"), "{report}");
}

/// `_on_the_surface_chain(app)`: a drive handed over to the destination
/// facility's street chain.
fn on_the_surface_chain(app: &mut TestApp) -> DrivingState {
    let mut d = a_drive(app);
    d.destination_exit_taken = true;
    d.ramp_mi = None;
    assert!(
        d.begin_surface_chain(&mut app.ctx, false),
        "no street chain for this facility"
    );
    d
}

#[test]
fn test_route_key_never_says_zero_miles_closing_on_the_gate() {
    // Named regression for the owner report of 2026-08-15. `Trip::distance_text`
    // rounds to whole miles, so every answer inside the last half mile was
    // "0 miles to the gate" -- and at 25 mph on city streets that half mile
    // takes over a minute. Walk the chain down to a couple of hundred feet and
    // the countdown has to keep meaning something.
    let mut app = TestApp::new();
    let mut d = on_the_surface_chain(&mut app);
    assert!(
        d.trip.total_miles() >= 0.5,
        "chain too short to walk the whole ladder"
    );
    let mut heard: Vec<String> = Vec::new();
    for remaining in [0.5, 0.4, 0.3, 0.2, 0.1, 0.05, 200.0 / 5280.0, 60.0 / 5280.0] {
        if remaining > d.trip.total_miles() {
            continue;
        }
        d.trip.position_mi = d.trip.total_miles() - remaining;
        app.clear_speech();
        d.handle_key_event(&mut app.ctx, &key(Key::R));
        heard.push(last(&app));
    }

    assert!(!heard.is_empty(), "the chain was too short to walk down");
    for report in &heard {
        assert!(!report.contains("0 miles"), "{report}");
        assert!(!report.contains("0 kilometers"), "{report}");
    }
    let second_last = &heard[heard.len() - 2];
    let final_line = &heard[heard.len() - 1];
    assert!(
        second_last.contains("200 feet to the gate"),
        "{second_last}"
    );
    assert!(final_line.contains("50 feet to the gate"), "{final_line}");
    assert!(heard[0].contains("Half a mile to the gate"), "{}", heard[0]);
}

#[test]
fn test_route_key_names_the_street_under_the_wheels() {
    // The chain's report follows the truck, not the street it started on.
    let mut app = TestApp::new();
    let mut d = on_the_surface_chain(&mut app);
    let legs: Vec<(f64, String)> = d
        .trip
        .route
        .legs
        .iter()
        .map(|leg| (leg.miles, leg.highway.clone()))
        .collect();
    assert!(legs.len() >= 2);
    // Street and city read as written: capitalising the line's first word
    // once lowercased the rest, and "US 83" was read as the word "us".
    let city = app
        .ctx
        .world
        .spoken_city(&d.trip.route.cities[0], Some(true));
    app.clear_speech();

    d.trip.position_mi = legs[0].0 * 0.5;
    d.handle_key_event(&mut app.ctx, &key(Key::R));
    assert!(
        last(&app).contains(&format!("On city streets, {}, in {city}.", legs[0].1)),
        "{}",
        last(&app)
    );

    d.trip.position_mi = legs[0].0 + legs[1].0 * 0.5;
    d.handle_key_event(&mut app.ctx, &key(Key::R));
    assert!(
        last(&app).contains(&format!("On city streets, {}, in {city}.", legs[1].1)),
        "{}",
        last(&app)
    );
}

#[test]
fn test_route_key_counts_down_to_the_on_ramp_leaving_the_origin_gate() {
    // The departure chain is city streets, and the highway readout was wrong
    // on it twice over: it called a two-mile street chain's percent the run's
    // progress, and it pointed the driver "toward" the city they were standing
    // in (owner report, 2026-08-15).
    let mut app = TestApp::new();
    let mut d = a_drive_between(&mut app, "Rochester", "Buffalo", "Rochester freight market");
    assert!(
        d.begin_departure_chain(&mut app.ctx, false),
        "no departure chain for this facility"
    );
    let highway = d
        .highway_trip
        .as_ref()
        .expect("the departure chain keeps the highway trip")
        .route
        .legs[0]
        .highway
        .clone();
    d.trip.position_mi = d.trip.total_miles() * 0.5;
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &key(Key::R));

    let report = last(&app);
    assert!(
        report.starts_with("On city streets,"),
        "{report}"
    );
    assert!(
        report.contains(&format!("to the {highway} on-ramp.")),
        "{report}"
    );
    assert!(!report.contains("percent there"), "{report}");
    assert!(!report.contains("toward"), "{report}");
    assert!(!report.contains("0 miles"), "{report}");
}

#[test]
fn test_route_key_answers_the_pickup_drive_as_city_streets() {
    // The pickup drive is streets from end to end: no highway leg to frame.
    let mut app = TestApp::new();
    let world = get_world();
    let (origin, location) = ("Rochester", "Rochester freight market");
    app.ctx.profile = Some(Profile::named_in("Info Keys", origin));
    let highway = world
        .supported_route(origin, "Buffalo", None)
        .expect("the world routes")
        .expect("the corridor is supported");
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        12.0,
        origin,
        location,
        "Buffalo",
        highway.miles(),
        1000.0,
        12.0,
    );
    job.destination_location = "Buffalo freight market".to_string();
    let route = world
        .facility_approach_route(origin, location)
        .expect("the facility has an approach route");
    let mut d = DrivingState::new(&mut app.ctx, job, route, None, DRIVE_PHASE_PICKUP, None);
    d.trip.set_npc_vehicles(Vec::new());
    d.trip.weather.current = WeatherKind::Clear;
    d.trip.position_mi = 0.0f64.max(d.trip.total_miles() - 200.0 / 5280.0);
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &key(Key::R));

    let report = last(&app);
    assert!(
        report.starts_with("On city streets,"),
        "{report}"
    );
    assert!(report.contains("200 feet to the gate at"), "{report}");
    assert!(!report.contains("percent there"), "{report}");
}

#[test]
#[ignore = "needs a busy event voice (Python patched ctx.event_voice_busy)"]
fn test_controller_back_button_stops_the_driving_voice() {
    // Back silences the road while it is talking, and reads help when it is
    // not. The "not" half is covered live above.
}

// -- a latched throttle is gone; the brake latch and a live key remain --------------
//
// The rest of the old `test_pedal_latch_assists.py` cases that needed the
// real per-frame loop. Cruise/keeper/curve no longer fight a latched
// throttle because that latch does not exist. The two hand-held-key cases
// stay: a physical hold is still live manual override.

/// Out on the corridor, where the posted limit is the interstate's own and 60
/// miles an hour is not an overspeed. `start_drive` left the truck near the
/// origin, where the limit is well under 60 and the dash alarm would arm.
fn on_the_open_road(d: &mut DrivingState) {
    d.trip.position_mi = d.trip.total_miles() / 2.0;
    let at = d.trip.position_mi;
    let (limit, _) = d.trip.speed_limit_at(at);
    assert!(
        limit >= 60.0,
        "the open-road limit here is {limit}, so 60 would arm the dash alarm"
    );
}

/// `_drive_frames(driving, seconds)`: the whole per-frame loop, not one
/// mixin's slice of it.
///
/// The pacer's clock moves with the frames. Python captured at `ctx.say_event`
/// and never reached the pacer at all; here the capture sits under it, and a
/// frame loop that costs no wall time leaves the pacer believing the voice is
/// still working through a backlog -- which drops the ambient confirmation
/// lines two of these cases are about. Advancing the clock by the same dt the
/// truck gets is what a real second of driving does.
fn drive_frames(d: &mut DrivingState, app: &mut TestApp, clock: &FakeClock, seconds: f64) {
    let mut t = 0.0;
    while t < seconds {
        d.update_frame(&mut app.ctx, DT);
        clock.advance(DT);
        t += DT;
    }
}

/// `release_air_brakes(driving)` plus the engine: `_update_cruise` cancels the
/// session without a running engine.
fn ready_to_roll(d: &mut DrivingState) {
    d.trip.truck.set_air_ready(false);
    d.trip.truck.engine_on = true;
}

fn press_for(d: &mut DrivingState, app: &mut TestApp, clock: &FakeClock, key: Key, seconds: f64) {
    app.ctx.input.press(key, Mods::NONE);
    drive_frames(d, app, clock, seconds);
}

fn release_for(d: &mut DrivingState, app: &mut TestApp, clock: &FakeClock, key: Key, seconds: f64) {
    app.ctx.input.release(key, Mods::NONE);
    drive_frames(d, app, clock, seconds);
}

fn in_reverse_at_rest(d: &mut DrivingState) {
    ready_to_roll(d);
    d.trip.truck.transmission.automatic = true;
    d.trip.truck.transmission.gear = REVERSE;
    d.trip.truck.velocity_mps = 0.0;
    assert!(d.trip.truck.transmission.in_reverse());
}

#[test]
fn test_a_hand_held_key_still_stands_the_assists_down() {
    // Physical hold keeps today's manual-override meaning.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    ready_to_roll(&mut d);
    on_the_open_road(&mut d);
    app.ctx.input.press(Key::Up, Mods::NONE);
    d.trip.truck.velocity_mps = mph_to_mps(60.0);
    d.engage_cruise(&mut app.ctx, 55.0, false);

    drive_frames(&mut d, &mut app, &clock, 2.0);

    assert!(d.cruise_mph.is_some()); // engaged, waiting for the key to lift
    assert!(d.trip.truck.throttle > 0.9, "{}", d.trip.truck.throttle); // the hand owns the pedal
}

#[test]
fn test_the_throttle_catch_gesture_never_holds_the_pedal() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    ready_to_roll(&mut d);
    on_the_open_road(&mut d);
    d.trip.truck.velocity_mps = mph_to_mps(60.0);
    let audio = app.record_audio();
    app.clear_speech();

    press_for(&mut d, &mut app, &clock, Key::Up, 0.2);
    release_for(&mut d, &mut app, &clock, Key::Up, 0.2);
    press_for(&mut d, &mut app, &clock, Key::Up, 0.8);
    release_for(&mut d, &mut app, &clock, Key::Up, 1.0);

    assert!(
        throttle_latch_speech(&app).is_empty(),
        "{:?}",
        app.event_lines()
    );
    assert!(
        !audio.borrow().played.iter().any(|(k, _, _)| k == "ui/tick"),
        "{:?}",
        audio.borrow().played
    );
    assert!(
        d.trip.truck.throttle < 0.05,
        "throttle stayed applied after release: {}",
        d.trip.truck.throttle
    );
}

#[test]
fn test_a_normal_throttle_hold_leaves_reverse() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    in_reverse_at_rest(&mut d);
    let audio = app.record_audio();
    app.clear_speech();

    press_for(&mut d, &mut app, &clock, Key::Up, 0.8);

    assert!(
        !d.trip.truck.transmission.in_reverse(),
        "gear {}",
        d.trip.truck.transmission.gear
    );
    assert_eq!(d.trip.truck.transmission.gear, 1);
    assert!(
        throttle_latch_speech(&app).is_empty(),
        "{:?}",
        app.event_lines()
    );
    assert!(
        !audio.borrow().played.iter().any(|(k, _, _)| k == "ui/tick"),
        "{:?}",
        audio.borrow().played
    );
}

#[test]
fn test_pumping_the_throttle_still_leaves_reverse() {
    // The remaining reverse fight after the 2026-08-21 trap patch: a driver
    // who taps then holds -- pumping to get moving -- used to catch the
    // throttle latch at half a second and lose the six-tenth shift.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    in_reverse_at_rest(&mut d);
    let audio = app.record_audio();
    app.clear_speech();

    press_for(&mut d, &mut app, &clock, Key::Up, 0.2);
    release_for(&mut d, &mut app, &clock, Key::Up, 0.2);
    press_for(&mut d, &mut app, &clock, Key::Up, 0.8);

    assert!(
        !d.trip.truck.transmission.in_reverse(),
        "gear {}",
        d.trip.truck.transmission.gear
    );
    assert_eq!(d.trip.truck.transmission.gear, 1);
    assert!(
        throttle_latch_speech(&app).is_empty(),
        "{:?}",
        app.event_lines()
    );
    assert!(
        !audio.borrow().played.iter().any(|(k, _, _)| k == "ui/tick"),
        "{:?}",
        audio.borrow().played
    );
}

#[test]
fn test_the_brake_latch_still_holds_hands_free() {
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    ready_to_roll(&mut d);
    on_the_open_road(&mut d);
    d.trip.truck.velocity_mps = mph_to_mps(30.0);
    let audio = app.record_audio();
    app.clear_speech();
    catch_gesture(&mut d, &mut app, false, 0.8);
    assert!(d.brake_latch.latched);
    assert!(
        app.event_lines().iter().any(|l| l == "Brake latched."),
        "{:?}",
        app.event_lines()
    );
    assert!(
        audio.borrow().played.iter().any(|(k, _, _)| k == "ui/tick"),
        "{:?}",
        audio.borrow().played
    );

    // Hands off: the blended brake latch must keep the pedal down.
    drive_frames(&mut d, &mut app, &clock, 1.0);
    assert!(d.brake_latch.latched);
    assert!(
        d.trip.truck.brake > 0.5,
        "latched brake did not stay applied: {}",
        d.trip.truck.brake
    );
}

#[test]
fn test_a_hand_held_key_stands_the_keeper_down() {
    // The spec bullet names the keeper; the existing coverage only engages
    // cruise. Both read the same hand_accelerating argument, but pin it.
    let mut app = TestApp::new();
    let clock = app.fake_pacer_clock();
    let mut d = a_drive(&mut app);
    ready_to_roll(&mut d);
    let start = d.trip.position_mi;
    d.trip
        .zones
        .push(Zone::new(start - 0.1, start + 3.0, 25.0, "school"));
    app.ctx.input.press(Key::Up, Mods::NONE);
    d.trip.truck.velocity_mps = mph_to_mps(30.0);
    d.engage_keeper(&mut app.ctx, 25.0, "school", Some(25.0), false);

    drive_frames(&mut d, &mut app, &clock, 2.0);

    assert!(d.keeper_mph.is_some()); // engaged, waiting for the key to lift
    assert!(d.trip.truck.throttle > 0.9, "{}", d.trip.truck.throttle); // the hand owns the pedal
}

// `test_rolling_t_plans_exact_sleep_stop_without_silently_selecting_exit`,
// `test_x_cancel_clears_explicit_assist_but_keeps_route_plan`,
// `test_rolling_t_without_sleep_stop_gives_recovery_guidance` and
// `test_t_during_police_stop_names_the_trooper_action` are live in
// `crates/freight-fate/tests/transcript_rest_stop_assist.rs`.

// `test_the_planner_sees_past_the_corner_it_is_already_easing_for` is live in `crates/freight-fate/tests/states_driving_turns.rs`.

/// Whether the state on top of the stack is a `T`.
fn top_is<T: 'static>(app: &TestApp) -> bool {
    app.ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<T>())
}

#[test]
fn test_the_tab_key_opens_the_status_screen() {
    // Tab is the one key that leaves the wheel for the reference screens, so
    // everything that lives there (the route, the driver, the map, the radio,
    // the tablet) is one press away from driving.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    assert!(!top_is::<DrivingStatusState>(&app));

    d.handle_key_event(&mut app.ctx, &key(Key::Tab));

    assert!(top_is::<DrivingStatusState>(&app));
}

#[test]
fn test_escape_and_start_open_the_pause_menu() {
    // Both devices reach the pause menu, and the horn never sticks on behind
    // it: Escape while leaning on the horn opened the menu with the note still
    // sounding.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.horn_on = true;

    d.handle_key_event(&mut app.ctx, &key(Key::Escape));

    assert!(top_is::<PauseMenuState>(&app));
    assert!(!d.trip.truck.horn_on);

    // The pad's Start button is the same door.
    drop(d);
    drop(app);
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.handle_controller_event(&mut app.ctx, &pad(ControllerButton::Start));
    assert!(top_is::<PauseMenuState>(&app));
}

#[test]
fn test_the_radio_dial_keys_tune_jump_and_change_volume() {
    // Page Up / `;` tunes down, Page Down / `'` tunes up, Ctrl jumps a whole
    // category, Shift moves the radio volume in ten percent steps.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.radio_volume = 0.5;

    // Shift is the volume, on both spellings of the dial.
    d.handle_key_event(
        &mut app.ctx,
        &InputEvent::key_mods(Key::PageUp, Mods::SHIFT),
    );
    assert!(
        (app.ctx.settings.radio_volume - 0.6).abs() < 1e-6,
        "{}",
        app.ctx.settings.radio_volume
    );
    d.handle_key_event(&mut app.ctx, &InputEvent::key_mods(Key::Quote, Mods::SHIFT));
    assert!(
        (app.ctx.settings.radio_volume - 0.5).abs() < 1e-6,
        "{}",
        app.ctx.settings.radio_volume
    );

    // Plain and Ctrl move the dial, not the volume: whichever station they
    // land on, the volume the driver set is untouched.
    for event in [
        key(Key::PageDown),
        key(Key::Semicolon),
        InputEvent::key_mods(Key::PageDown, Mods::CTRL),
        InputEvent::key_mods(Key::PageUp, Mods::CTRL),
    ] {
        d.handle_key_event(&mut app.ctx, &event);
        assert!(
            (app.ctx.settings.radio_volume - 0.5).abs() < 1e-6,
            "{}",
            app.ctx.settings.radio_volume
        );
    }
}

// -- tests/test_driving_speech_ladder.py (the cab lines) -------------------------------

/// Owner playtest, 2026-08-17: "quiet still feels busy".
///
/// The Python half of this is a source scan of `states/driving_*.py` for the
/// three transcript lines, checking each carries `SpeechCategory.CONFIRMATION`
/// -- and the scan had to learn to read each file under its own name, because
/// "Engine off." is spoken in three places with three meanings and an
/// unsorted glob graded whichever the filesystem handed over first (CI,
/// 2026-08-23). The port asserts the same thing where it is decidable: the
/// cab confirmations go to an earcon at quiet, and the air-brake lockout that
/// speaks the same words is a ROUTE event and keeps its voice.
#[test]
fn test_the_cab_confirmations_are_suppressed_at_urgent_only() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "urgent_only".to_string();
    let mut d = a_drive(&mut app);
    // The ladder only applies past the walkthrough.
    app.ctx.profile.as_mut().unwrap().tutorial_done = true;
    d.trip.truck.start_engine();
    d.trip.truck.set_air_ready(true);
    d.toggle_parking_brake(&mut app.ctx); // the drive starts with it set
    app.clear_speech();

    // "Parking brake set. Air pressure ... psi." -- a confirmation.
    d.toggle_parking_brake(&mut app.ctx);
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());

    // "Engine off." from the E key -- also a confirmation.
    d.toggle_engine(&mut app.ctx);
    assert!(!d.trip.truck.engine_on);
    assert!(app.main_lines().is_empty(), "{:?}", app.main_lines());

    // The other "Engine off." is not this one. The air-brake lockout speaks
    // the same two words at quiet as the terse form of "why the truck will
    // not roll", and it is a ROUTE event on the event channel rather than a
    // confirmation -- which is what the Python scan kept mis-grading. Pinned
    // where it is spoken, by
    // `states_driving_updates::test_the_air_brake_lockout_says_why_the_truck_will_not_roll`.
    assert!(app.event_lines().is_empty(), "{:?}", app.event_lines());
}

/// Standard hears the confirmations in full: Urgent only silences them, not
/// the category itself.
#[test]
fn test_the_cab_confirmations_still_speak_at_standard() {
    let mut app = TestApp::new();
    app.ctx.settings.driving_speech = "standard".to_string();
    let mut d = a_drive(&mut app);
    app.ctx.profile.as_mut().unwrap().tutorial_done = true;
    d.trip.truck.start_engine();
    d.trip.truck.set_air_ready(true);
    d.toggle_parking_brake(&mut app.ctx); // the drive starts with it set
    app.clear_speech();

    d.toggle_parking_brake(&mut app.ctx);
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.starts_with("Parking brake set. Air ")));

    app.clear_speech();
    d.toggle_engine(&mut app.ctx);
    assert_eq!(app.main_lines(), vec!["Engine off.".to_string()]);
}

#[test]
fn test_clock_key_counts_the_highway_run_while_still_on_the_departure_streets() {
    // Agent drive, Dallas to Sherman, 2026-09-11: pressed on the two miles of
    // streets out of the pickup, C said "arrival in 0.1 hours" with 123 miles
    // of interstate still parked in the highway trip. The streets alone are
    // not the run.
    let mut app = TestApp::new();
    let mut d = a_drive_between(&mut app, "Rochester", "Buffalo", "Rochester freight market");
    assert!(
        d.begin_departure_chain(&mut app.ctx, false),
        "no departure chain for this facility"
    );
    let highway_miles = d
        .highway_trip
        .as_ref()
        .expect("the departure chain keeps the highway trip")
        .total_miles();
    assert!(highway_miles > 30.0, "{highway_miles}");
    d.trip.position_mi = d.trip.total_miles() * 0.5;
    d.trip.truck.velocity_mps = mph_to_mps(25.0);
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &key(Key::C));

    let report = last(&app);
    let eta: f64 = report
        .split("Arrival ")
        .nth(1)
        .and_then(|rest| rest.split(' ').next())
        .and_then(|hours| hours.parse().ok())
        .unwrap_or_else(|| panic!("no arrival estimate in {report}"));
    // At least the highway at a fast pace, never the streets alone.
    assert!(eta >= highway_miles / 75.0, "{report}");
    assert!(report.contains("at a typical highway pace"), "{report}");
}
