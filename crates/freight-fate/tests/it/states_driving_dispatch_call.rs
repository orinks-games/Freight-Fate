//! Driver-initiated offline dispatch calls from the pause menu.

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Menu;
use freight_fate::states::driving_core::{FIELD_REPAIR_DAMAGE_PCT, MECHANIC_WAIT_MIN};
use freight_fate::states::driving_dispatch_call::{
    dispatch_request, local_dispatch_response, DispatchCallState, CALL_DELAY,
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
