//! What the player hears and can review of the co-driver's shift, and what
//! they never hear: the co-driver is an artificial driver, so no menu and
//! no menu sound of theirs reaches the player (owner, 2026-10-19).

use ff_core::message_log::MessageCategory;
use ff_core::sim::trip_models::{TripEvent, TripEventData, TripEventKind};
use ff_core::speech_text::SpokenMessage;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::base::Key;
use freight_fate::states::driving_team::notes::NoteWeight;
use freight_fate::states::driving_team::TeamMode;

const DT: f64 = 1.0 / 60.0;

fn team(mode: TeamMode) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Team Notes"));
    harness.with_drive(move |d, ctx| {
        d.tutorial = None;
        d.departure_checked = true;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.hand_wheel_to_co_driver(ctx, mode);
    });
    harness.clear_speech();
    harness
}

fn co_driver_review(harness: &PlaytestHarness) -> Vec<String> {
    harness
        .app
        .ctx
        .message_log
        .filtered_messages()
        .iter()
        .filter(|m| m.category == MessageCategory::CoDriver)
        .map(|m| m.text.clone())
        .collect()
}

#[test]
fn riding_along_a_note_is_heard_and_kept_under_the_co_drivers_review() {
    let mut harness = team(TeamMode::RideAlong);
    harness.with_drive(|d, ctx| {
        d.co_driver_note(
            ctx,
            "Dan plans to stop at the travel plaza.".to_string(),
            NoteWeight::Passing,
        );
    });
    assert!(
        harness
            .transcript()
            .iter()
            .any(|line| line.contains("Dan plans to stop at the travel plaza.")),
        "{:#?}",
        harness.transcript()
    );
    assert_eq!(
        co_driver_review(&harness),
        vec!["Dan plans to stop at the travel plaza.".to_string()]
    );
    assert!(harness.app.ctx.message_log.has(MessageCategory::CoDriver));
}

#[test]
fn asleep_the_review_keeps_only_what_the_wake_up_summary_does() {
    let mut harness = team(TeamMode::SleepThrough);
    harness.with_drive(|d, ctx| {
        d.co_driver_note(ctx, "A plan.".to_string(), NoteWeight::Passing);
        d.co_driver_note(ctx, "A fine.".to_string(), NoteWeight::Wake);
    });
    assert_eq!(co_driver_review(&harness), vec!["A fine.".to_string()]);
}

#[test]
fn the_co_drivers_menu_sounds_are_not_heard_and_the_players_are() {
    let mut harness = team(TeamMode::RideAlong);
    let log = harness.app.record_audio();
    harness.app.ctx.sync_cue_mute();
    harness.app.ctx.audio.play("ui/menu_move");
    harness.app.ctx.audio.play("vehicle/collision");
    harness
        .app
        .ctx
        .player_asked(|ctx| ctx.audio.play("ui/menu_select"));
    let played: Vec<String> = log
        .borrow()
        .played
        .iter()
        .map(|(key, _, _)| key.clone())
        .collect();
    assert_eq!(played, vec!["vehicle/collision", "ui/menu_select"]);
    harness.with_drive(|d, ctx| {
        d.take_wheel_back(ctx);
    });
    harness.app.ctx.audio.play("ui/menu_move");
    assert_eq!(log.borrow().played.len(), 3, "back at the wheel, heard");
}

#[test]
fn riding_along_the_player_hears_the_hazard_call() {
    let mut harness = team(TeamMode::RideAlong);
    let deer = TripEvent {
        kind: TripEventKind::Hazard,
        message: SpokenMessage::new("A deer crossing the road."),
        data: TripEventData {
            deadline_s: Some(3.5),
            dodgeable: Some(false),
            in_lane: Some(false),
            animal: Some(true),
            name: Some("the deer".to_string()),
            ..Default::default()
        },
    };
    harness.with_drive(move |d, ctx| {
        d.truck_mut().start_engine();
        d.truck_mut().velocity_mps = 25.0;
        d.handle_trip_event(ctx, &deer);
    });
    assert!(
        harness
            .transcript()
            .iter()
            .any(|line| line.contains("A deer crossing the road.")),
        "{:#?}",
        harness.transcript()
    );
}

#[test]
fn damage_from_before_the_shift_does_not_make_the_co_driver_hand_back() {
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Team Damage"));
    harness.with_drive(|d, ctx| {
        d.tutorial = None;
        d.departure_checked = true;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        // A scuff on the shoulder before the swap.
        d.trip.truck.add_damage(3.0, true);
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
    });
    for _ in 0..60 * 20 {
        harness.advance_frame_clock();
        harness.app.frame(DT);
    }
    assert!(
        harness.app.ctx.co_driver_away,
        "handed back for old damage:\n{}",
        harness.transcript().join("\n")
    );
}

#[test]
fn the_pause_menu_leaves_the_truck_work_to_the_co_driver() {
    let mut harness = team(TeamMode::RideAlong);
    harness.with_drive(|d, _| d.truck_mut().velocity_mps = 0.0);
    harness.press_key(Key::Escape, None);
    let rows = harness.menu_labels();
    assert!(!rows.iter().any(|row| row == "Call dispatch"), "{rows:?}");
    assert!(
        !rows.iter().any(|row| row == "Emergency shoulder sleep"),
        "{rows:?}"
    );
}
