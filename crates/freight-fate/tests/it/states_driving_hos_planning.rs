use ff_core::sim::trip_models::RoadStop;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::{InputEvent, Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::RAMP_TERMINAL_MISS_LOOP_MIN;

fn drive(scale: f64) -> PlaytestHarness {
    let mut h = PlaytestHarness::new();
    h.app.ctx.settings.time_scale = scale;
    h.app.ctx.settings.hos_mode = "realistic".into();
    h.start_delivery(StartDelivery::named("HOS Planning"));
    h.with_drive(|d, _| {
        d.departure_checked = true;
        d.trip.position_mi = 10.0;
        d.trip.truck.velocity_mps = 25.0;
    });
    h.clear_speech();
    h
}

fn stop(name: &str, mile: f64, action: &str) -> RoadStop {
    let mut stop = RoadStop::new(name, mile, "travel_center");
    stop.actions = vec![action.into()];
    stop.parking = "confirmed".into();
    stop
}

#[test]
fn self_serve_bobtail_records_work_in_each_pacing_mode() {
    for scale in [1.0, 10.0, 20.0] {
        let mut h = drive(scale);
        h.with_drive(|d, ctx| {
            d.job.bobtail = true;
            d.job.assigned = false;
            let before = ctx.profile.as_ref().unwrap().hos.driving_min;
            d.update_hours_and_fatigue(ctx, 1.0);
            let clock = &ctx.profile.as_ref().unwrap().hos;
            assert_eq!(clock.status, "driving");
            assert!(clock.driving_min > before);
            d.trip.truck.velocity_mps = 0.0;
            d.update_hours_and_fatigue(ctx, 1.0);
            assert_eq!(
                ctx.profile.as_ref().unwrap().hos.status,
                "on_duty_not_driving"
            );
            let before = ctx.profile.as_ref().unwrap().hos.driving_min;
            let receiver = RoadStop::new("Receiver", d.trip.total_miles(), "delivery_destination");
            d.loop_back_to_destination_terminal(ctx, &receiver);
            assert_eq!(
                ctx.profile.as_ref().unwrap().hos.driving_min,
                before + RAMP_TERMINAL_MISS_LOOP_MIN
            );
        });
    }
}

#[test]
fn hos_planning_names_last_compatible_reachable_stop() {
    let mut h = drive(10.0);
    h.with_drive(|d, ctx| {
        ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0 + 40.0;
        d.trip.stops = vec![
            stop("First", 12.0, "sleep"),
            stop("Last", 15.0, "sleep"),
            stop("Fuel only", 17.0, "fuel"),
        ];
        let advice = d.hos_route_context(ctx);
        assert!(advice.contains("Last"), "{advice}");
        assert!(!advice.contains("First"), "{advice}");
        assert!(advice.contains("minutes"), "{advice}");
    });
}

#[test]
fn hos_planning_rejects_passed_unusable_and_unreachable_stops() {
    let mut h = drive(20.0);
    h.with_drive(|d, ctx| {
        ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0 + 50.0;
        let mut unusable = stop("No parking", 11.0, "sleep");
        unusable.parking = "none".into();
        d.trip.stops = vec![
            stop("Passed", 9.0, "sleep"),
            unusable,
            stop("Too far", 150.0, "sleep"),
        ];
        let advice = d.hos_route_context(ctx);
        assert!(
            advice.contains("stop left before your hours limit"),
            "{advice}"
        );
    });
}

#[test]
fn requested_readouts_share_the_last_reachable_stop_advice() {
    let mut h = drive(10.0);
    h.with_drive(|d, ctx| {
        ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0 + 40.0;
        d.trip.stops = vec![stop("First", 12.0, "sleep"), stop("Last", 15.0, "sleep")];
    });

    for _ in 0..2 {
        h.clear_speech();
        h.key(InputEvent::key_mods(Key::D, Mods::ALT));
        let heard = h.transcript_text();
        assert!(heard.contains("Last"), "{heard}");
        assert!(heard.contains("Stop here."), "{heard}");
    }

    h.key(InputEvent::key(Key::Tab));
    h.select_menu_item("Route");
    let route_lines = h.menu_labels().join(" ");
    assert!(
        route_lines.contains("HOS route: Last legal sleep stop"),
        "{route_lines}"
    );
    assert!(
        !route_lines.contains("Next legal stop: Last"),
        "{route_lines}"
    );

    h.key(InputEvent::key(Key::Escape));
    h.select_menu_item("Driver apps");
    h.select_menu_item("ELD");
    let eld_lines = h.menu_labels().join(" ");
    assert!(
        eld_lines.contains("ELD route note: Last legal sleep stop"),
        "{eld_lines}"
    );
}

#[test]
fn last_stop_warning_keeps_audible_lead_at_each_pacing_mode() {
    for (scale, outside_mi, inside_mi) in [(1.0, 6.0, 4.0), (10.0, 6.0, 4.0), (20.0, 9.0, 7.0)] {
        let mut h = drive(scale);
        h.with_drive(|d, ctx| {
            ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0 + 40.0;
            d.trip.stops = vec![stop("Last chance", 20.0, "sleep")];
            d.trip.position_mi = 20.0 - outside_mi;
            d.update_hours_and_fatigue(ctx, 0.0);
        });
        assert!(!h.transcript_text().contains("Last reachable sleep stop"));

        h.clear_speech();
        h.with_drive(|d, ctx| {
            d.trip.position_mi = 20.0 - inside_mi;
            d.update_hours_and_fatigue(ctx, 0.0);
        });
        let heard = h.transcript_text();
        assert!(
            heard.contains("Last reachable sleep stop: travel center: Last chance"),
            "{heard}"
        );
        assert!(heard.contains("Plan to stop here"), "{heard}");

        h.clear_speech();
        h.with_drive(|d, ctx| d.update_hours_and_fatigue(ctx, 0.0));
        assert!(!h.transcript_text().contains("Last reachable sleep stop"));
    }
}

#[test]
fn automatic_warning_stays_quiet_when_the_destination_is_reachable() {
    let mut h = drive(20.0);
    h.with_drive(|d, ctx| {
        ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0 + 40.0;
        let destination = d.trip.total_miles();
        d.trip.position_mi = destination - 4.0;
        d.trip.stops = vec![stop("Unneeded stop", destination - 2.0, "sleep")];
        d.update_hours_and_fatigue(ctx, 0.0);
    });
    assert!(!h.transcript_text().contains("Last reachable sleep stop"));
}

#[test]
fn saved_last_stop_warning_does_not_repeat_after_resume() {
    let mut h = drive(20.0);
    h.with_drive(|d, ctx| {
        ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0 + 40.0;
        d.trip.position_mi = 16.0;
        d.trip.stops = vec![stop("Saved stop", 20.0, "sleep")];
        d.warn_last_hos_stop(ctx);
    });
    assert!(
        h.transcript_text().contains("Saved stop"),
        "{}",
        h.transcript_text()
    );

    h.advance_clock(30.0);
    let (snapshot, stops) = h.with_drive(|d, ctx| {
        d.warn_last_hos_stop(ctx);
        (d.snapshot(ctx), d.trip.stops.clone())
    });

    h.clear_speech();
    let mut resumed =
        DrivingState::from_snapshot(&mut h.app.ctx, &snapshot).expect("the drive resumes");
    resumed.trip.stops = stops;
    resumed.warn_last_hos_stop(&mut h.app.ctx);
    assert!(!h.transcript_text().contains("Saved stop"));

    h.app.ctx.profile.as_mut().unwrap().hos.sleep();
    h.app.ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0 + 40.0;
    h.advance_clock(3.0);
    resumed.warn_last_hos_stop(&mut h.app.ctx);
    assert!(
        h.transcript_text().contains("Saved stop"),
        "{}",
        h.transcript_text()
    );
}

#[test]
fn paused_unheard_last_stop_warning_retries_after_saved_resume() {
    let mut h = drive(20.0);
    h.with_drive(|d, ctx| {
        ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0 + 40.0;
        d.trip.position_mi = 16.0;
        d.trip.stops = vec![stop("Interrupted stop", 20.0, "sleep")];
        d.warn_last_hos_stop(ctx);
    });
    assert!(h.transcript_text().contains("Interrupted stop"));

    h.key(InputEvent::key(Key::Escape));
    let (snapshot, stops) = h.with_drive(|d, ctx| (d.snapshot(ctx), d.trip.stops.clone()));

    h.clear_speech();
    let mut resumed =
        DrivingState::from_snapshot(&mut h.app.ctx, &snapshot).expect("the drive resumes");
    resumed.trip.stops = stops;
    resumed.warn_last_hos_stop(&mut h.app.ctx);
    assert!(
        h.transcript_text().contains("Interrupted stop"),
        "{}",
        h.transcript_text()
    );
}

/// Control cutting the warning off counts as hearing it.
///
/// An interrupted warning is said again in full, which is right when
/// another line cut it and wrong when the player did: every press of
/// Control brought the whole line back (Shane, 12 September).
#[test]
fn control_silences_the_last_stop_warning_for_good() {
    let mut h = drive(20.0);
    h.with_drive(|d, ctx| {
        ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0 + 40.0;
        d.trip.position_mi = 16.0;
        d.trip.stops = vec![stop("Silenced stop", 20.0, "sleep")];
        d.warn_last_hos_stop(ctx);
    });
    assert!(h.transcript_text().contains("Silenced stop"));

    h.key(InputEvent::key(Key::LCtrl));
    h.clear_speech();
    for _ in 0..3 {
        h.with_drive(|d, ctx| d.warn_last_hos_stop(ctx));
    }
    assert!(
        !h.transcript_text().contains("Silenced stop"),
        "{}",
        h.transcript_text()
    );
}

#[test]
fn hos_and_all_maintenance_warnings_finish_without_competing() {
    let mut h = drive(20.0);
    h.with_drive(|d, ctx| {
        ctx.profile.as_mut().unwrap().hos.duty_min = 13.0 * 60.0 + 40.0;
        d.trip.position_mi = 16.0;
        d.trip.stops = vec![stop("Shared warning stop", 20.0, "sleep")];
        d.trip.truck.tire_wear_pct = 80.0;
        d.trip.truck.brake_wear_pct = 80.0;
        d.trip.truck.engine_wear_pct = 80.0;
    });

    for _ in 0..5 {
        h.with_drive(|d, ctx| {
            d.warn_last_hos_stop(ctx);
            d.update_damage_bands(ctx, 0.1);
        });
        h.advance_clock(30.0);
    }
    h.with_drive(|d, ctx| {
        d.warn_last_hos_stop(ctx);
        d.update_damage_bands(ctx, 0.1);
        assert_eq!(d.maintenance_levels, [1; 3]);
        assert_eq!(d.maintenance_pending_levels, [0; 3]);
        assert!(ctx
            .profile
            .as_ref()
            .unwrap()
            .hos
            .warned
            .iter()
            .any(|key| key.contains("Shared warning stop")));
    });

    let warnings: Vec<_> = h
        .app
        .event_lines()
        .into_iter()
        .filter(|line| line.contains("Service soon") || line.contains("Shared warning stop"))
        .collect();
    assert_eq!(warnings.len(), 4, "{warnings:?}");
}
