// -- the driver window and the climate keys: Z, Alt Z, Shift Z ------------------------
//
// A player's ask (2026-10-17): pure ambience, so what is asserted is
// the confirmation and the sound, never an effect on the drive.

fn shift(k: Key) -> InputEvent {
    InputEvent::key_mods(k, Mods::SHIFT)
}

#[test]
fn test_z_lowers_the_driver_window_and_the_road_comes_in() {
    use freight_fate::audio::CH_CAB_WIND;
    let mut app = TestApp::new();
    let audio = app.record_audio();
    let mut d = a_drive(&mut app);
    d.trip.truck.engine_on = true;
    d.trip.truck.velocity_mps = mph_to_mps(65.0);
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &key(Key::Z));
    assert_eq!(last(&app), "Driver window down.");
    assert!(audio
        .borrow()
        .played
        .iter()
        .any(|(k, ..)| k == "cab/window_motor"));

    // The glass takes its two seconds; the outside follows it down.
    d.update_cab_audio(&mut app.ctx, 1.0);
    let half = audio.borrow().outside_lift.unwrap();
    d.update_cab_audio(&mut app.ctx, 1.0);
    let full = audio.borrow().outside_lift.unwrap();
    assert!(1.0 < half && half < full, "{half} then {full}");
    let rush = audio.borrow().loops.get(&CH_CAB_WIND).cloned().unwrap();
    assert_eq!(rush.key, "cab/wind");
    assert!(rush.pan < 0.0, "the driver's window is on the left");

    d.handle_key_event(&mut app.ctx, &key(Key::Z));
    assert_eq!(last(&app), "Driver window up.");
    d.update_cab_audio(&mut app.ctx, 3.0);
    assert_eq!(audio.borrow().outside_lift, Some(1.0));
    assert!(!audio.borrow().loops.contains_key(&CH_CAB_WIND));
}

#[test]
fn test_the_fan_steps_round_and_blows_only_with_the_engine_on() {
    use freight_fate::audio::CH_CAB_FAN;
    let mut app = TestApp::new();
    let audio = app.record_audio();
    let mut d = a_drive(&mut app);
    d.trip.truck.engine_on = false;
    app.clear_speech();

    d.handle_key_event(&mut app.ctx, &alt(Key::Z));
    assert_eq!(last(&app), "Fan low.");
    d.update_cab_audio(&mut app.ctx, 0.1);
    assert!(
        !audio.borrow().loops.contains_key(&CH_CAB_FAN),
        "no blower with the engine off"
    );

    d.trip.truck.engine_on = true;
    d.update_cab_audio(&mut app.ctx, 0.1);
    let low = audio.borrow().loops.get(&CH_CAB_FAN).cloned().unwrap();
    d.handle_key_event(&mut app.ctx, &alt(Key::Z));
    assert_eq!(last(&app), "Fan medium.");
    d.update_cab_audio(&mut app.ctx, 0.1);
    let medium = audio.borrow().loops.get(&CH_CAB_FAN).cloned().unwrap();
    assert!(medium.volume > low.volume && medium.rate > low.rate);

    d.handle_key_event(&mut app.ctx, &alt(Key::Z));
    d.handle_key_event(&mut app.ctx, &alt(Key::Z));
    assert_eq!(last(&app), "Fan off.");
    d.update_cab_audio(&mut app.ctx, 0.1);
    assert!(!audio.borrow().loops.contains_key(&CH_CAB_FAN));
}

#[test]
fn test_shift_z_flips_heat_and_air_conditioning_and_the_clutch_clicks_in() {
    let mut app = TestApp::new();
    let audio = app.record_audio();
    let mut d = a_drive(&mut app);
    d.trip.truck.engine_on = true;
    d.cab = ff_core::sim::cab::CabState::for_outside(Some(0.0));
    d.handle_key_event(&mut app.ctx, &alt(Key::Z));
    app.clear_speech();
    audio.borrow_mut().played.clear();

    d.handle_key_event(&mut app.ctx, &shift(Key::Z));
    assert_eq!(last(&app), "Air conditioning.");
    assert!(audio
        .borrow()
        .played
        .iter()
        .any(|(k, ..)| k == "cab/ac_clutch"));

    d.handle_key_event(&mut app.ctx, &shift(Key::Z));
    assert_eq!(last(&app), "Heat.");
}
