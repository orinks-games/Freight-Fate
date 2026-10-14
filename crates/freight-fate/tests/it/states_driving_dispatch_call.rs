//! Driver-initiated offline dispatch calls from the pause menu.

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Menu;
use freight_fate::states::driving_core::{FIELD_REPAIR_DAMAGE_PCT, MECHANIC_WAIT_MIN};
use freight_fate::states::driving_dispatch_call::{
    dispatch_request, local_dispatch_response, DispatchCallState, CALL_DELAY, CALL_LOAD, CALL_ROAD,
    CALL_TRUCK,
};
use freight_fate::states::driving_menu_states::DriveRef;
use freight_fate::states::driving_pause_states::PauseMenuState;

use crate::states_driving_menus_support::*;

#[test]
fn test_call_dispatch_is_available_only_while_stopped() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let mut pause = PauseMenuState::with_drive(DriveRef::of(&drive));
    let stopped = build_labels(&mut pause, &mut app.ctx);
    assert!(
        stopped.iter().any(|row| row == "Call dispatch"),
        "{stopped:?}"
    );

    with_drive(&drive, |driving| {
        driving.trip.truck.velocity_mps = 20.0;
    });
    let moving = build_labels(&mut pause, &mut app.ctx);
    assert!(
        !moving.iter().any(|row| row == "Call dispatch"),
        "{moving:?}"
    );
}

#[test]
fn test_dispatch_call_menu_covers_the_supported_request_categories() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let mut call = DispatchCallState::new(DriveRef::of(&drive));
    let rows = build_labels(&mut call, &mut app.ctx);
    assert_eq!(
        rows,
        vec![
            "Report a delay",
            "Ask about hours",
            "Ask about the road ahead",
            "Report truck trouble",
            "Report load trouble",
        ]
    );
}

#[test]
fn test_local_dispatch_uses_a_valid_shared_request_and_repeatable_decision() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |driving, ctx| {
        driving.trip.game_minutes = 60.0;
        let request = dispatch_request(driving, ctx, CALL_DELAY);
        assert!(request.validate().is_empty(), "{:?}", request.validate());
        assert_eq!(request.kind, CALL_DELAY);
        assert_eq!(request.source_game, "freight_fate");
        assert!(request.context.contains_key("remaining_miles"));

        let first = local_dispatch_response(&request, driving, ctx);
        let second = local_dispatch_response(&request, driving, ctx);
        assert!(first.validate().is_empty(), "{:?}", first.validate());
        assert_eq!(first.decision, second.decision);
        assert_eq!(first.message, second.message);
        assert_eq!(first.effects, second.effects);
    });
}

#[test]
fn test_remote_dispatch_off_preserves_the_local_answer() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let expected = drive_and_ctx(&drive, &mut app, |driving, ctx| {
        let request = dispatch_request(driving, ctx, CALL_DELAY);
        local_dispatch_response(&request, driving, ctx).message
    });
    app.ctx.settings.remote_dispatch_calls = false;
    let mut call = DispatchCallState::new(DriveRef::of(&drive));
    app.clear_speech();
    activate(&mut call, &mut app.ctx, "Report a delay");
    assert_eq!(last(&app), format!("Dispatch: {expected}"));
}

#[test]
fn test_hours_call_answers_on_the_menu_speech_channel() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let mut call = DispatchCallState::new(DriveRef::of(&drive));
    Menu::enter(&mut call, &mut app.ctx);
    app.clear_speech();
    activate(&mut call, &mut app.ctx, "Ask about hours");
    assert!(last(&app).starts_with("Dispatch:"), "{}", last(&app));
    assert!(last(&app).contains("next limit"), "{}", last(&app));
}

#[test]
fn test_dispatch_authorizes_and_applies_a_needed_roadside_repair() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    let before = with_drive(&drive, |driving| {
        driving.trip.truck.damage_pct = 60.0;
        driving.trip.game_minutes
    });
    let mut call = DispatchCallState::new(DriveRef::of(&drive));
    app.clear_speech();
    activate(&mut call, &mut app.ctx, "Report truck trouble");

    with_drive(&drive, |driving| {
        assert_eq!(driving.trip.truck.damage_pct, FIELD_REPAIR_DAMAGE_PCT);
        assert_eq!(driving.trip.game_minutes, before + MECHANIC_WAIT_MIN);
    });
    assert!(
        last(&app).contains("Roadside repair is authorized"),
        "{}",
        last(&app)
    );
    assert!(
        last(&app).contains("mobile mechanic patched the truck"),
        "{}",
        last(&app)
    );
}

#[test]
fn test_load_call_reports_cargo_damage_not_condition() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |driving, ctx| {
        driving.trip.truck.cargo_damage_pct = 12.4;
        let request = dispatch_request(driving, ctx, CALL_LOAD);
        assert_eq!(request.summary, "Cargo damage is 12 percent.");
        let response = local_dispatch_response(&request, driving, ctx);
        assert!(
            response
                .message
                .starts_with("I have the freight report as damaged, with 12 percent cargo damage."),
            "{}",
            response.message
        );
        assert!(
            !response.message.contains("condition"),
            "{}",
            response.message
        );
    });
}

#[test]
fn test_request_summaries_round_hours_miles_and_damage() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |driving, ctx| {
        driving.trip.game_minutes = 100.0;
        driving.job.deadline_game_h = 7.0;
        driving.trip.truck.damage_pct = 33.333;
        let delay = dispatch_request(driving, ctx, CALL_DELAY).summary;
        assert_eq!(delay, "1.7 hours used of a 7 hour delivery window.");

        let road = dispatch_request(driving, ctx, CALL_ROAD).summary;
        let miles = road.strip_suffix(" miles remain.").expect(&road);
        assert!(!miles.contains('.'), "{road}");

        let truck = dispatch_request(driving, ctx, CALL_TRUCK).summary;
        assert_eq!(truck, "Truck damage is 33 percent.");
    });
}

#[test]
fn test_delay_summary_drops_a_trailing_zero_from_whole_hours() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |driving, ctx| {
        for (minutes, window, want) in [
            (60.0, 14.0, "1 hour used of a 14 hour delivery window."),
            (120.0, 14.5, "2 hours used of a 14.5 hour delivery window."),
            (0.0, 13.96, "0 hours used of a 14 hour delivery window."),
            (45.0, 9.25, "0.8 hours used of a 9.3 hour delivery window."),
        ] {
            driving.trip.game_minutes = minutes;
            driving.job.deadline_game_h = window;
            assert_eq!(dispatch_request(driving, ctx, CALL_DELAY).summary, want);
        }
    });
}

#[test]
fn test_road_summary_follows_the_distance_unit() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |driving, ctx| {
        ctx.settings.imperial_units = false;
        let road = dispatch_request(driving, ctx, CALL_ROAD).summary;
        assert!(road.ends_with(" kilometers remain."), "{road}");
        assert!(!road.contains("mile"), "{road}");
        let request = dispatch_request(driving, ctx, CALL_ROAD);
        let answer = local_dispatch_response(&request, driving, ctx).message;
        let distance = road.trim_end_matches('.');
        assert!(answer.contains(distance), "{answer} / {road}");
        assert!(!answer.contains("mile"), "{answer}");
        ctx.settings.imperial_units = true;
        let road = dispatch_request(driving, ctx, CALL_ROAD).summary;
        assert!(road.ends_with(" miles remain."), "{road}");
    });
}

#[test]
fn test_load_call_never_contradicts_its_condition_word() {
    // "as shifted but sound, 3 percent damaged" said the load was sound and
    // damaged in one breath, and "as damaged, 12 percent damaged" said it
    // twice.
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    drive_and_ctx(&drive, &mut app, |driving, ctx| {
        driving.trip.truck.cargo_damage_pct = 0.0;
        let request = dispatch_request(driving, ctx, CALL_LOAD);
        let secure = local_dispatch_response(&request, driving, ctx).message;
        assert!(secure.starts_with("The freight is secure."), "{secure}");
        assert!(!secure.contains("damage"), "{secure}");
        driving.trip.truck.cargo_damage_pct = 3.0;
        let request = dispatch_request(driving, ctx, CALL_LOAD);
        let low = local_dispatch_response(&request, driving, ctx).message;
        assert!(
            low.starts_with(
                "I have the freight report as shifted but sound, with only 3 percent cargo damage."
            ),
            "{low}"
        );
        for damage in [3.0, 12.0, 40.0, 80.0] {
            driving.trip.truck.cargo_damage_pct = damage;
            let request = dispatch_request(driving, ctx, CALL_LOAD);
            let message = local_dispatch_response(&request, driving, ctx).message;
            assert!(message.matches("damaged").count() <= 1, "{message}");
            assert!(!message.contains("percent damaged"), "{message}");
            assert!(
                message.contains(&format!("{damage:.0} percent cargo damage.")),
                "{message}"
            );
            if message.contains("sound") {
                assert!(message.contains("with only"), "{message}");
            }
        }
    });
}
