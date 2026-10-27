//! Port of `tests/test_main_channel_pacing.py`: the main channel comes
//! under discipline while the player is driving.
//!
//! The event voice has a pacer; the main channel had 535 call sites
//! defaulting to interrupt=True and none (research doc, part 1.1). R2's
//! central fix: while the top state is the driving state, `ctx.say` queues
//! instead of cutting, so an achievement or assist notice cannot stamp on
//! the line mid-air. Menus -- including menus pushed OVER a drive -- keep
//! today's immediate behavior, mirroring how screen readers cancel speech
//! on navigation.

use crate::states_driving_menus_support as drive_support;
use freight_fate::app::testing::TestApp;
use freight_fate::app::{Say, SayEvent};
use freight_fate::controller::ControllerButton;
use freight_fate::states::base::{InputEvent, Key, State};
use hearken::speech::EventPriority;

struct Wheel;
impl State for Wheel {
    fn paces_main_speech(&self) -> bool {
        true
    }
}

struct Menu;
impl State for Menu {}

fn calls(app: &TestApp) -> Vec<(String, bool)> {
    app.main_calls()
}

#[test]
fn test_the_driving_state_declares_main_channel_pacing() {
    let mut app = TestApp::new();
    let drive = drive_support::a_drive(&mut app);
    assert!(drive.borrow().paces_main_speech());
    assert!(!Menu.paces_main_speech());
}

#[test]
fn a_plain_state_does_not_pace_main_speech() {
    assert!(!Menu.paces_main_speech());
}

#[test]
fn test_main_speech_queues_while_at_the_wheel() {
    let mut app = TestApp::new();
    app.push_state(Wheel);

    app.ctx.say("New achievement! Bumper-to-Bumper Blues.");

    assert_eq!(
        calls(&app),
        vec![(
            "New achievement! Bumper-to-Bumper Blues.".to_string(),
            false
        )]
    );
    app.shutdown();
}

#[test]
fn test_a_menu_over_the_drive_keeps_immediate_speech() {
    let mut app = TestApp::new();
    app.push_state(Wheel);
    app.push_state(Menu);

    app.ctx.say("Settings. Audio. 1 of 9.");

    assert_eq!(
        calls(&app),
        vec![("Settings. Audio. 1 of 9.".to_string(), true)]
    );
    app.shutdown();
}

#[test]
fn test_menu_speech_with_no_drive_anywhere_still_interrupts() {
    let mut app = TestApp::new();
    app.push_state(Menu);

    app.ctx.say("Main menu. New career. 1 of 6.");

    assert_eq!(
        calls(&app),
        vec![("Main menu. New career. 1 of 6.".to_string(), true)]
    );
    app.shutdown();
}

#[test]
fn test_queued_reply_no_longer_purges_the_shared_event_channel() {
    // The point of the demotion on a shared voice: a main-channel line
    // during the drive cannot cut a pending ROUTE line any more, so nothing
    // needs rescuing -- the road line simply keeps its place in the queue.
    let mut app = TestApp::new();
    app.ctx.settings.sapi_events = false; // events ride the main voice
    app.push_state(Wheel);

    let scale = "Open weigh station ahead in two miles. All trucks must pull in.";
    app.ctx
        .say_event_with(scale, SayEvent::queued().priority(EventPriority::Important));
    app.ctx.say("Fifty five miles per hour."); // info reply, default interrupt

    // Reply queued behind the scale line; the scale line was never cut, so
    // it appears exactly once -- no rescue, no repetition.
    assert_eq!(
        calls(&app),
        vec![
            (scale.to_string(), false),
            ("Fifty five miles per hour.".to_string(), false)
        ]
    );
    app.shutdown();
}

#[test]
fn test_a_readout_the_player_asked_for_still_cuts() {
    // R2 was aimed at lines nobody asked for, and caught info keys too.
    //
    // On the 1.8 release every readout cut the line in progress, so pressing
    // a key was how you got out from under an announcement. R2's central
    // demotion removed that on 1.9 for the keys as well as for the notices,
    // which is what a tester reported as the controller "not interrupting"
    // (Sarah R. via the owner, 2026-08-16) -- it was never
    // controller-specific.
    let mut app = TestApp::new();
    app.push_state(Wheel);

    app.ctx
        .player_asked(|ctx| ctx.say("Speed limit 65 miles per hour."));

    assert_eq!(
        calls(&app),
        vec![("Speed limit 65 miles per hour.".to_string(), true)]
    );
    app.shutdown();
}

#[test]
fn test_the_asked_for_exemption_does_not_leak_past_the_press() {
    // An assist notice arriving after the key must still queue.
    let mut app = TestApp::new();
    app.push_state(Wheel);

    app.ctx
        .player_asked(|ctx| ctx.say("Speed limit 65 miles per hour."));
    app.ctx.say("New achievement! Bumper-to-Bumper Blues.");

    assert_eq!(
        calls(&app).last().unwrap(),
        &(
            "New achievement! Bumper-to-Bumper Blues.".to_string(),
            false
        )
    );
    app.shutdown();
}

#[test]
fn test_nested_presses_restore_rather_than_latch() {
    // A handler that opens a screen which speaks must not stick the flag on.
    let mut app = TestApp::new();
    app.push_state(Wheel);
    assert!(!app.ctx.speech_requested());
    app.ctx.player_asked(|ctx| {
        ctx.player_asked(|ctx| {
            assert!(ctx.speech_requested());
        });
        assert!(ctx.speech_requested());
    });
    assert!(!app.ctx.speech_requested());
    // The token form restores too.
    let token = app.ctx.player_asked_begin();
    assert!(app.ctx.speech_requested());
    app.ctx.player_asked_end(token);
    assert!(!app.ctx.speech_requested());
    app.shutdown();
}

#[test]
fn test_pressing_an_info_key_at_the_wheel_cuts_the_line_in_progress() {
    // End to end through the real driving state, keyboard and pad alike.
    let mut app = TestApp::new();
    let drive = drive_support::a_drive(&mut app);
    app.clear_speech();

    drive_support::with_drive(&drive, |d| {
        d.handle_key_event(&mut app.ctx, &InputEvent::key(Key::Space))
    });
    let said = calls(&app);
    assert!(
        said.last().is_some_and(|(_, interrupt)| *interrupt),
        "a pressed key must cut: {said:?}"
    );

    drive_support::with_drive(&drive, |d| {
        d.handle_controller_event(&mut app.ctx, &InputEvent::button(ControllerButton::B))
    });
    let said = calls(&app);
    assert!(
        said.last().is_some_and(|(_, interrupt)| *interrupt),
        "a pad button is a request too: {said:?}"
    );

    // And the drive's own chatter still queues behind whatever is playing.
    app.ctx.say("New achievement! Bumper-to-Bumper Blues.");
    assert_eq!(
        calls(&app).last().cloned(),
        Some((
            "New achievement! Bumper-to-Bumper Blues.".to_string(),
            false
        ))
    );
}

/// `say` with an explicit `interrupt=False` queues everywhere.
#[test]
fn queued_say_stays_queued_in_a_menu() {
    let mut app = TestApp::new();
    app.push_state(Menu);
    app.ctx.say_with("Loading.", Say::queued());
    assert_eq!(calls(&app), vec![("Loading.".to_string(), false)]);
    app.shutdown();
}
