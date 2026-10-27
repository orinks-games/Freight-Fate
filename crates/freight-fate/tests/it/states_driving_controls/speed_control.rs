// -- the manual gearbox (test_driving_manual_controls.py) ----------------------------

#[test]
fn test_shift_modified_manual_downshift_uses_clutch_before_next_update() {
    // The frame-loop half of the Python test (five seconds of `update`, then
    // "no damage and the revs came back") waits on `states::driving_updates`;
    // what this pins is the part the control surface owns -- held Shift
    // engages the clutch on the SAME event that selects the gear, so the
    // downshift never grinds.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.automatic_transmission = false;
    let truck = &mut d.trip.truck;
    truck.start_engine();
    truck.set_air_ready(false);
    truck.transmission.automatic = false;
    truck.transmission.gear = 2;
    truck.transmission.clutch = 0.0; // stale until the update loop samples held keys
    truck.velocity_mps = 60.0 / 2.23694;
    truck.rpm = truck.specs.idle_rpm;

    d.handle_key_event(
        &mut app.ctx,
        &InputEvent::KeyDown {
            key: Key::Q,
            mods: Mods::SHIFT,
            text: Some('q'),
            repeat: false,
        },
    );

    assert_eq!(d.trip.truck.transmission.gear, 1);
    assert_eq!(d.trip.truck.transmission.clutch, 1.0);
    assert_eq!(d.trip.truck.damage_pct, 0.0);
}

// -- the cruise dial (test_cruise_steps.py) ------------------------------------------

#[test]
fn test_plus_key_snaps_an_off_grid_cruise_target() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    cruise_at(&mut d, &mut app, 32.0);

    d.handle_key_event(&mut app.ctx, &InputEvent::key_text(Key::Equals, '='));
    assert_eq!(d.cruise_mph, Some(35.0));
    d.handle_key_event(&mut app.ctx, &InputEvent::key_text(Key::Equals, '='));
    assert_eq!(d.cruise_mph, Some(40.0));
}

#[test]
fn test_ctrl_plus_and_minus_step_by_one() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    cruise_at(&mut d, &mut app, 35.0);

    d.handle_key_event(&mut app.ctx, &InputEvent::key_mods(Key::Equals, Mods::CTRL));
    assert_eq!(d.cruise_mph, Some(36.0));
    d.handle_key_event(&mut app.ctx, &InputEvent::key_mods(Key::Minus, Mods::CTRL));
    assert_eq!(d.cruise_mph, Some(35.0));
}

#[test]
fn test_the_dial_also_answers_the_typed_plus_and_minus() {
    // The `+`/`-` fallback: a keyboard whose plus lives on a shifted key sends
    // a different keycode, and `event.unicode` is what makes the tap land.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    cruise_at(&mut d, &mut app, 35.0);

    d.handle_key_event(&mut app.ctx, &InputEvent::key_text(Key::Other(0x2b), '+'));
    assert_eq!(d.cruise_mph, Some(40.0));
    d.handle_key_event(&mut app.ctx, &InputEvent::key_text(Key::Other(0x2d), '-'));
    assert_eq!(d.cruise_mph, Some(35.0));
}

#[test]
fn test_os_key_repeat_does_not_race_the_cruise_target_up() {
    // Holding Plus used to fire OS auto-repeat KeyDowns every ~30 ms and walk
    // the live adaptive-cruise set speed to the ceiling in about a second.
    // Repeats are ignored; only a fresh press steps.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    cruise_at(&mut d, &mut app, 25.0);

    d.handle_key_event(&mut app.ctx, &InputEvent::key_text(Key::Equals, '='));
    assert_eq!(d.cruise_mph, Some(30.0));
    for _ in 0..20 {
        d.handle_key_event(
            &mut app.ctx,
            &InputEvent::KeyDown {
                key: Key::Equals,
                mods: Mods::NONE,
                text: Some('='),
                repeat: true,
            },
        );
    }
    assert_eq!(
        d.cruise_mph,
        Some(30.0),
        "OS key-repeat must not advance the cruise target"
    );
    assert_eq!(d.speed_control_target_mph, Some(30.0));
}

#[test]
fn test_os_key_repeat_does_not_race_the_cruise_target_down() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    cruise_at(&mut d, &mut app, 55.0);

    d.handle_key_event(&mut app.ctx, &InputEvent::key_text(Key::Minus, '-'));
    assert_eq!(d.cruise_mph, Some(50.0));
    for _ in 0..20 {
        d.handle_key_event(
            &mut app.ctx,
            &InputEvent::KeyDown {
                key: Key::Minus,
                mods: Mods::NONE,
                text: Some('-'),
                repeat: true,
            },
        );
    }
    assert_eq!(d.cruise_mph, Some(50.0));
}

#[test]
fn test_discrete_plus_presses_still_step_five_each() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    cruise_at(&mut d, &mut app, 25.0);

    for expected in [30.0, 35.0, 40.0] {
        d.handle_key_event(&mut app.ctx, &InputEvent::key_text(Key::Equals, '='));
        assert_eq!(d.cruise_mph, Some(expected));
    }
}

#[test]
fn test_keeper_zone_adjust_snaps_the_resume_target() {
    // The speed keeper owns a restricted zone, but +/- still steps the
    // remembered open-road target that adaptive cruise resumes to -- and must
    // not disturb the keeper's own held speed while it does.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    d.trip.truck.engine_on = true;
    let start = d.trip.position_mi;
    d.trip
        .zones
        .push(Zone::new(start - 0.1, start + 3.0, 25.0, "school"));
    d.trip.truck.velocity_mps = mph_to_mps(25.0);
    d.engage_keeper(&mut app.ctx, 25.0, "school", Some(25.0), false);
    let keeper_before = d.keeper_mph;
    d.speed_control_target_mph = Some(62.0);

    d.handle_key_event(&mut app.ctx, &InputEvent::key_text(Key::Equals, '='));

    assert_eq!(d.speed_control_target_mph, Some(65.0));
    assert_eq!(d.keeper_mph, keeper_before);
}

#[test]
fn test_keeper_raw_capture_rounds_to_the_whole_mph() {
    // `_engage_keeper`'s plain K-set branch (no explicit target_mph) rounds the
    // captured speed to the whole mph the player hears, mirroring
    // `_engage_cruise`'s rounding -- otherwise an unrounded 24.95 would spend
    // the first snap tap healing an invisible fraction instead of making an
    // audible step.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    d.trip.truck.engine_on = true;
    let start = d.trip.position_mi;
    d.trip
        .zones
        .push(Zone::new(start - 0.1, start + 3.0, 30.0, "school"));
    d.trip.truck.velocity_mps = mph_to_mps(24.95); // off the whole mph

    d.engage_keeper(&mut app.ctx, 30.0, "school", None, false);

    assert_eq!(d.keeper_mph, Some(25.0));
}

#[test]
fn test_high_idle_still_owns_the_keys_when_parked() {
    // Parked with a latched high idle, +/- steps the idle RPM, not any cruise
    // or keeper target -- the branch `_adjust_cruise` checks first.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(true);
    d.trip.truck.start_engine();
    d.trip.truck.velocity_mps = 0.0;
    d.trip.truck.high_idle_rpm = Some(HIGH_IDLE_DEFAULT_RPM);

    d.handle_key_event(&mut app.ctx, &InputEvent::key_text(Key::Equals, '='));

    assert_eq!(
        d.trip.truck.high_idle_rpm,
        Some(HIGH_IDLE_DEFAULT_RPM + HIGH_IDLE_STEP_RPM)
    );
    assert_eq!(d.cruise_mph, None);
    assert_eq!(d.speed_control_target_mph, None);
}

// -- the speed-control session (driving_speed_control.rs) ----------------------------

#[test]
fn test_speed_authority_predicate_reads_all_three() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    assert!(!d.speed_authority_engaged());
    d.cruise_mph = Some(55.0);
    assert!(d.speed_authority_engaged());
    d.cruise_mph = None;
    d.keeper_mph = Some(25.0);
    assert!(d.speed_authority_engaged());
    d.keeper_mph = None;
    d.curve_assist_active = true;
    assert!(d.speed_authority_engaged());
}

#[test]
fn test_shift_k_resumes_the_remembered_speed() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    cruise_at(&mut d, &mut app, 60.0);
    // Braking cancels the session but remembers the target, like a car's
    // RESUME button.
    d.cancel_cruise(&mut app.ctx, false);
    assert_eq!(d.cruise_mph, None);
    assert!(!d.speed_control_armed);
    assert_eq!(d.resume_target_mph, Some(60.0));

    app.clear_speech();
    d.handle_key_event(&mut app.ctx, &InputEvent::key_mods(Key::K, Mods::SHIFT));
    assert!(d.speed_control_armed);
    assert_eq!(d.speed_control_target_mph, Some(60.0));
    assert_eq!(
        last(&app),
        "Resuming automatic speed control at 60 miles per hour."
    );
}

#[test]
fn test_resume_refuses_without_a_remembered_speed_or_an_engine() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    d.handle_key_event(&mut app.ctx, &InputEvent::key_mods(Key::K, Mods::SHIFT));
    assert_eq!(last(&app), "No remembered cruise speed yet. K sets one.");

    d.resume_target_mph = Some(55.0);
    app.clear_speech();
    d.handle_key_event(&mut app.ctx, &InputEvent::key_mods(Key::K, Mods::SHIFT));
    assert_eq!(last(&app), "Resume needs the engine running.");

    d.speed_control_armed = true;
    app.clear_speech();
    d.handle_key_event(&mut app.ctx, &InputEvent::key_mods(Key::K, Mods::SHIFT));
    assert_eq!(last(&app), "Automatic speed control is already on.");
}

#[test]
fn resume_refusal_names_a_moved_cruise_key() {
    let mut app = TestApp::new();
    app.ctx.settings.key_bindings = "cruise=f7".into();
    app.ctx.apply_bindings();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    d.handle_key_event(&mut app.ctx, &InputEvent::key_mods(Key::K, Mods::SHIFT));
    assert_eq!(last(&app), "No remembered cruise speed yet. F7 sets one.");
}

#[test]
fn test_a_transit_pause_lifts_itself_once_the_bar_is_honored() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    cruise_at(&mut d, &mut app, 60.0);
    assert!(d.pause_speed_control(&mut app.ctx, true));
    assert!(d.speed_control_paused_at_stop);
    assert!(d.speed_control_transit_pause);
    assert!(d.speed_control_armed); // the session is remembered, not dropped

    // Still rolling toward the bar with the ramp ahead: nothing lifts.
    d.ramp_mi = Some(0.4);
    assert!(!d.lift_transit_pause(false));

    // Stopped at the bar, then rolling again off the brake.
    d.trip.truck.velocity_mps = 0.0;
    assert!(!d.lift_transit_pause(false));
    assert!(d.speed_control_stop_honored);
    d.trip.truck.velocity_mps = mph_to_mps(20.0);
    assert!(d.lift_transit_pause(false));
    assert!(!d.speed_control_paused_at_stop);
}

#[test]
fn test_an_arrival_pause_is_never_lifted_by_rolling_again() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    cruise_at(&mut d, &mut app, 60.0);
    d.pause_speed_control(&mut app.ctx, false);
    d.trip.truck.velocity_mps = mph_to_mps(30.0);
    assert!(!d.lift_transit_pause(false));
    assert!(d.speed_control_paused_at_stop);
}

#[test]
fn test_cancelling_the_keeper_alone_keeps_a_remembered_cruise_target() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    cruise_at(&mut d, &mut app, 62.0);
    d.cancel_cruise(&mut app.ctx, false);
    assert_eq!(d.resume_target_mph, Some(62.0));
    // A keeper-only cancel carries no target and must not clobber it.
    d.cancel_keeper(&mut app.ctx, false);
    assert_eq!(d.resume_target_mph, Some(62.0));
}

#[test]
fn test_speed_keeper_ease_window_follows_the_driving_mode() {
    // The keeper's ease is budgeted in real seconds, so a compressed clock has
    // to buy more road for the same warning. A corner is the exception: it
    // decompresses the trip to real time, and the ease is sized on that clock
    // rather than on the pacing the player picked.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.velocity_mps = 25.0 / 2.23694;

    assert!(d.keeper_ease_mi(20.0, 10.0) > d.keeper_ease_mi(20.0, 4.0));
    assert!(d.keeper_ease_mi(20.0, 4.0) > d.keeper_ease_mi(20.0, 1.0));
    // The ceiling trims the discretionary reaction budget so a long access
    // road is not crawled -- but never the PHYSICAL shed, which the window's
    // docstring promises is a floor. At 40x the 25-to-20 shed alone outruns
    // the cap, so the window follows the physics (clamping it was how the
    // keeper arrived at 15.47 over a 15 sign on long-route draws -- the
    // one-in-four flake, fixed 2026-08-20).
    assert!(d.keeper_ease_mi(20.0, 40.0) > KEEPER_EASE_MAX_MI);
    // The cap still binds where reaction, not physics, is the bigger ask: a
    // one-mph trim at 30x wants little shed road, and the six-plus seconds of
    // hearing-and-deciding it would otherwise buy are what the ceiling exists
    // to trim.
    assert!((d.keeper_ease_mi(24.0, 30.0) - KEEPER_EASE_MAX_MI).abs() < 1e-9);

    // A bigger drop buys more road than the base window at the same pacing.
    assert!(d.keeper_ease_mi(5.0, 1.0) > d.keeper_ease_mi(24.0, 1.0));

    // A corner runs on the real clock whichever pacing the player chose, so
    // its ease is sized there and never on the compressed road. Sizing it on
    // the pacing read the corner as close from half a mile back and held the
    // whole block at the corner speed.
    d.trip.time_scale = 40.0;
    d.trip.controlled_turn = false;
    assert!(d.trip.effective_time_scale() > 1.0);
    assert!((d.keeper_turn_ease_scale() - 1.0).abs() < 1e-9);
    d.trip.controlled_turn = true;
    assert!((d.keeper_turn_ease_scale() - 1.0).abs() < 1e-9);
}

#[test]
fn test_the_keeper_sheds_for_the_barrels_from_the_start_of_a_merge_taper() {
    // A merge taper is a mile of road that exists for one shed. The keeper
    // used to take it over from cruise and hold the taper's number until its
    // ordinary ease window opened, then meet the barrels two over (owner,
    // 2026-09-14). On a taper the work zone's number is in demand at once.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.ctx.settings.speed_keeper = true;
    d.speed_control_armed = true;
    d.keeper_mph = Some(55.0);
    d.trip.zones = vec![
        Zone::new(5.0, 6.0, 55.0, "construction merge"),
        Zone::new(6.0, 8.0, 45.0, "construction"),
    ];
    d.trip.truck.velocity_mps = 52.0 / freight_fate::states::driving_core::MPH_PER_MPS;
    // Just inside the taper, a whole mile from the barrels: further than the
    // keeper's own ease window reaches, and still the answer.
    d.trip.position_mi = 5.02;
    assert_eq!(
        d.keeper_speed_ahead(&mut app.ctx),
        Some((45.0, "construction".to_string()))
    );
    // Ahead of the taper the ordinary window still governs: a mile and a
    // half from the barrels on the open corridor is not yet the keeper's.
    d.keeper_ease_target = None;
    d.trip.position_mi = 4.5;
    assert_eq!(d.keeper_speed_ahead(&mut app.ctx), None);
}

#[test]
fn test_the_restricted_zone_look_ahead_waits_for_the_spoken_warning() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.speed_control_armed = true;
    app.ctx.settings.speed_keeper = true;
    d.trip.position_mi = 0.0;
    d.trip.zones = vec![Zone::new(0.2, 3.0, 25.0, "construction")];
    // Cruise and the warning share a window; which one lands first must not
    // come down to frame order.
    assert_eq!(d.restricted_zone_limit_ahead(&mut app.ctx), None);
}

// -- the brake latch, and the throttle key that never latches ----------------------

const DT: f64 = 1.0 / 60.0;

/// Tap, release, press and hold through the catch window.
fn catch_gesture(d: &mut DrivingState, app: &mut TestApp, throttle: bool, seconds_held: f64) {
    let run = |held: bool, seconds: f64, d: &mut DrivingState, app: &mut TestApp| {
        let mut t = 0.0;
        while t < seconds {
            let (up, down) = if throttle {
                (held, false)
            } else {
                (false, held)
            };
            d.update_pedal_latches(&mut app.ctx, up, down, 0.0, DT);
            t += DT;
        }
    };
    run(true, 0.2, d, app);
    run(false, 0.2, d, app);
    run(true, seconds_held, d, app);
}

fn throttle_latch_speech(app: &TestApp) -> Vec<String> {
    app.event_lines()
        .into_iter()
        .chain(app.main_lines())
        .filter(|l| {
            let lower = l.to_lowercase();
            lower.contains("throttle latched")
                || l == "Throttle released."
                || lower.contains("adaptive cruise holds the speed")
                || lower.contains("speed keeper holds the speed")
        })
        .collect()
}

#[test]
fn test_holding_the_throttle_never_latches() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    catch_gesture(&mut d, &mut app, true, 0.8);
    assert!(
        throttle_latch_speech(&app).is_empty(),
        "{:?}",
        app.event_lines()
    );
    assert!(!d.brake_latch.latched);
    // Releasing the key must not leave a hidden throttle catch behind:
    // the function returns the brake, and the throttle side is gone.
    let down = d.update_pedal_latches(&mut app.ctx, false, false, 0.0, DT);
    assert!(!down);
}

#[test]
fn test_a_plain_brake_catch_keeps_its_plain_line() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    app.clear_speech();
    catch_gesture(&mut d, &mut app, false, 0.8);
    assert!(
        app.event_lines().iter().any(|l| l == "Brake latched."),
        "{:?}",
        app.event_lines()
    );
    assert!(d.brake_latch.latched);
}

#[test]
fn test_the_latch_setting_off_drops_a_held_brake_and_says_so() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    catch_gesture(&mut d, &mut app, false, 0.8);
    assert!(d.brake_latch.latched);
    app.ctx.settings.pedal_latch = "off".to_string();
    app.clear_speech();
    let down = d.update_pedal_latches(&mut app.ctx, false, false, 0.0, DT);
    assert!(!down);
    assert!(!d.brake_latch.latched);
    assert!(
        app.event_lines().iter().any(|l| l == "Brake released."),
        "{:?}",
        app.event_lines()
    );
}

#[test]
fn test_the_accelerator_releases_a_latched_brake() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    catch_gesture(&mut d, &mut app, false, 0.8);
    assert!(d.brake_latch.latched);
    app.clear_speech();
    d.update_pedal_latches(&mut app.ctx, true, false, 0.0, DT);
    assert!(!d.brake_latch.latched);
    assert!(
        app.event_lines().iter().any(|l| l == "Brake released."),
        "{:?}",
        app.event_lines()
    );
}

#[test]
fn test_the_throttle_catch_gesture_never_grabs_the_shift_back_to_forward() {
    // The catch used to land first (half a second against six tenths) and
    // wipe the pending shift, so pumping the throttle in reverse re-armed
    // and lost it every time (owner, at the scale, 2026-08-21: "I can't
    // get out of reverse?"). The throttle side is gone, so the armed
    // shift is still there after the same gesture.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.direction_armed = "forward".to_string();
    catch_gesture(&mut d, &mut app, true, 0.8);
    assert_eq!(d.direction_armed, "forward");
    assert!(
        throttle_latch_speech(&app).is_empty(),
        "{:?}",
        app.event_lines()
    );
}

// -- cases whose mixin has not landed ------------------------------------------------

#[test]
#[ignore = "needs a Trip seam for the monkeypatched trip.grade_at"]
fn test_grade_key_reads_the_slope_and_whether_the_truck_holds_it() {
    // `_fixed_grade(d, -5.0, until_mi=9.0)`; G then says "Grade 5.0 percent
    // downhill", "for another ..." and either "Speed is building" or names the
    // jake. Python replaced `trip.grade_at`; Rust needs either a route built
    // with grade segments or a test seam on `Trip`.
}

#[test]
#[ignore = "needs a Trip seam for the monkeypatched trip.grade_at"]
fn test_grade_key_names_the_next_steep_grade_ahead() {}

#[test]
#[ignore = "needs a Trip seam for the monkeypatched trip.grade_at"]
fn test_grade_key_says_when_nothing_steep_is_coming() {}

#[test]
#[ignore = "needs a Trip seam for the monkeypatched trip.grade_at"]
fn test_grade_key_names_the_grade_the_preview_is_planning_for() {}

#[test]
#[ignore = "needs a Trip seam for the monkeypatched trip.grade_at"]
fn test_grade_key_does_not_call_a_punchy_pull_nothing_steep() {}

#[test]
#[ignore = "needs a Trip seam for the monkeypatched trip.grade_at"]
fn test_grade_key_names_the_same_hill_the_speed_control_cue_names() {}

#[test]
#[ignore = "needs a Trip seam for the monkeypatched trip.grade_at"]
fn test_grade_key_names_a_grade_that_steepens_without_letting_up() {}

#[test]
#[ignore = "needs a Trip seam for the monkeypatched trip.grade_at"]
fn test_grade_key_says_nothing_else_steep_while_on_a_steep_grade() {}

#[test]
fn test_upcoming_key_never_reports_enforcement() {
    // U is the road, not the police (owner ruling, 2026-08-15). Enforcement
    // heads-ups still reach the player on the CB; this key does not recite
    // them in any hours-of-service mode, enforced or not.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    for mode in ["realistic", "relaxed", "debug_off"] {
        app.ctx.settings.hos_mode = mode.to_string();
        d.trip.position_mi = 4.0;
        d.trip.posts = vec![observing_post(6.0, 4.0)];
        app.clear_speech();

        d.handle_key_event(&mut app.ctx, &key(Key::U));

        let report = last(&app).to_lowercase();
        assert!(d.trip.next_patrol_within(15.0).is_some(), "{mode}");
        for word in ["enforcement", "patrol", "trooper", "police", "bear"] {
            assert!(!report.contains(word), "{mode}: {word}: {report}");
        }
    }
    // The branch that used to gate this on the mode.
    assert!(!hos::HOS_NON_ENFORCED_MODES.is_empty());
}

#[test]
fn test_upcoming_key_does_not_repeat_the_next_exit_key() {
    // Shift+R is the listed-exit key, word for word; U stopped echoing it.
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.position_mi = 4.0;
    d.trip.zones = Vec::new();
    d.trip.stops = Vec::new();
    d.trip.curves = Vec::new();
    let (at_mi, text) = {
        let cue = d
            .trip
            .next_exit_cue()
            .expect("route has no listed exit to echo");
        (cue.at_mi, cue.text.clone())
    };
    d.trip.position_mi = 0.0f64.max(at_mi - 5.0);
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &key(Key::U));

    assert!(!last(&app).contains(&text), "{}", last(&app));
}

#[test]
fn test_cancelling_a_keeper_only_session_remembers_its_speed_for_resume() {
    let mut app = TestApp::new();
    let mut d = a_drive(&mut app);
    d.trip.truck.set_air_ready(false);
    d.trip.truck.start_engine();
    app.ctx.settings.speed_keeper = true;
    d.trip.truck.velocity_mps = mph_to_mps(27.0);
    d.engage_keeper(&mut app.ctx, 35.0, "work", None, false);
    let held = d
        .keeper_mph
        .unwrap_or_else(|| panic!("keeper off: {:?}", last(&app)));
    assert_eq!(d.resume_target_mph, None);
    d.cancel_keeper(&mut app.ctx, false);
    assert_eq!(d.resume_target_mph, Some(held));
}
