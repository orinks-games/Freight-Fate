//! Port of the `GameContext` tests of `tests/test_driving_speech_ladder.py`:
//! the S4 driving speech ladder as `say` / `say_event` apply it. (The table
//! tests live in `ff_core::speech_pacing`; the whole-drive proofs wait for
//! `states::driving`.)
//!
//! Where the Python tests built a real `DrivingState` only to have a
//! profile with `tutorial_done=True` on the context (`_real_driving`), these
//! set that profile directly: the tests are about the rung, not the drive.

use ff_core::models::profile::Profile;
use ff_core::speech_pacing::{Disposition, SpeechCategory, DRIVING_SPEECH_DISPOSITIONS};
use ff_core::speech_text::SpokenMessage;
use freight_fate::app::testing::{stepping_clock, TestApp};
use freight_fate::app::{Say, SayEvent};
use hearken::speech::EventSpeechPacer;

fn app() -> TestApp {
    let mut app = TestApp::new();
    app.ctx.settings.sapi_events = true;
    app
}

fn set_rung(app: &mut TestApp, rung: &str) {
    app.ctx.settings.driving_speech = rung.to_string();
}

/// `_real_driving`'s one effect on the context: a profile past the
/// walkthrough.
fn past_the_walkthrough(app: &mut TestApp) {
    let mut profile = Profile::new();
    profile.name = "Ladder Fix Round".to_string();
    profile.current_city = "Denver".to_string();
    profile.tutorial_done = true;
    app.ctx.profile = Some(profile);
}

fn logged_last(app: &TestApp) -> String {
    app.ctx.message_log.messages.last().unwrap().text.clone()
}

#[test]
fn test_a_silenced_category_never_reaches_the_voice() {
    let mut app = app();
    set_rung(&mut app, "urgent_only");

    app.ctx.say_event_with(
        "Load damage 43 percent.",
        SayEvent::queued().category(SpeechCategory::Status),
    );

    assert!(app.event_lines().is_empty());
    app.shutdown();
}

#[test]
fn test_a_silenced_category_stays_out_of_the_message_log() {
    // The buffer follows speech; status-query keys still answer on demand.
    let mut app = app();
    set_rung(&mut app, "urgent_only");

    app.ctx.say_event_with(
        "Load damage 43 percent.",
        SayEvent::queued().category(SpeechCategory::Status),
    );

    assert!(app.ctx.message_log.messages.is_empty());
    app.shutdown();
}

#[test]
fn test_a_silenced_category_never_reaches_the_voice_via_say() {
    let mut app = app();
    set_rung(&mut app, "urgent_only");

    app.ctx.say_with(
        "Load damage 43 percent.",
        Say::new().category(SpeechCategory::Status),
    );

    assert!(app.main_lines().is_empty());
    app.shutdown();
}

#[test]
fn test_a_silenced_category_stays_out_of_the_log_through_say() {
    let mut app = app();
    set_rung(&mut app, "urgent_only");

    app.ctx.say_with(
        "Load damage 43 percent.",
        Say::new().category(SpeechCategory::Status),
    );

    assert!(app.ctx.message_log.messages.is_empty());
    app.shutdown();
}

#[test]
fn test_safety_speaks_at_the_quietest_rung() {
    let mut app = app();
    set_rung(&mut app, "urgent_only");

    app.ctx.say_event_with(
        "Change lanes or brake! Slow car ahead.",
        SayEvent::new().category(SpeechCategory::Safety),
    );

    assert_eq!(
        app.event_lines(),
        vec!["Change lanes or brake! Slow car ahead.".to_string()]
    );
    app.shutdown();
}

#[test]
fn test_the_rung_picks_the_rendering() {
    let mut app = app();
    let pair = SpokenMessage::with_terse(
        "Watch your speed. The limit is 65 miles per hour.",
        "Limit 65.",
    );

    set_rung(&mut app, "quiet");
    app.ctx
        .say_event_with(pair, SayEvent::new().category(SpeechCategory::Navigation));

    assert_eq!(app.event_lines(), vec!["Limit 65.".to_string()]);
    app.shutdown();
}

#[test]
fn test_an_untagged_line_still_speaks_at_the_quietest_rung() {
    let mut app = app();
    set_rung(&mut app, "urgent_only");

    app.ctx
        .say_event_with("Something nobody classified.", SayEvent::queued());

    assert_eq!(
        app.event_lines(),
        vec!["Something nobody classified.".to_string()]
    );
    app.shutdown();
}

#[test]
fn test_the_ladder_does_not_apply_before_the_walkthrough_is_done() {
    // R15, defended against a new mechanism. Terse used to silence the
    // tutorial outright, which orphaned exactly the new player most likely
    // to pick the quietest setting on day one. A rung must not do it either.
    let mut app = app();
    set_rung(&mut app, "urgent_only");
    let mut profile = Profile::new();
    profile.name = "New Driver".to_string();
    profile.current_city = "Denver".to_string();
    profile.tutorial_done = false;
    app.ctx.profile = Some(profile);

    app.ctx.say_event_with(
        "Press E to start the engine.",
        SayEvent::queued().category(SpeechCategory::Coaching),
    );

    assert_eq!(
        app.event_lines(),
        vec!["Press E to start the engine.".to_string()]
    );
    app.shutdown();
}

#[test]
fn test_the_ladder_applies_once_the_walkthrough_is_done() {
    let mut app = app();
    set_rung(&mut app, "urgent_only");
    past_the_walkthrough(&mut app);

    app.ctx.say_event_with(
        "Press E to start the engine.",
        SayEvent::queued().category(SpeechCategory::Coaching),
    );

    assert!(app.event_lines().is_empty());
    app.shutdown();
}

#[test]
fn test_the_ladder_applies_with_no_profile_at_all() {
    // ctx.profile is Option; the exemption must default to "the rung
    // applies" when there is no profile (a menu, a screen with no career
    // loaded), never to "nobody can ever be silenced".
    let mut app = app();
    set_rung(&mut app, "urgent_only");
    assert!(app.ctx.profile.is_none());

    app.ctx.say_event_with(
        "Press E to start the engine.",
        SayEvent::queued().category(SpeechCategory::Coaching),
    );

    assert!(app.event_lines().is_empty());
    app.shutdown();
}

#[test]
fn test_nothing_a_rung_silences_makes_a_sound() {
    // The owner retired the stand-in notes (2026-10-03): a line quiet or
    // urgent only leaves out is silent, through either channel, and a
    // standing condition re-firing stays silent too.
    for (rung, row) in DRIVING_SPEECH_DISPOSITIONS {
        for (category, disposition) in row {
            if disposition != Disposition::Silent {
                continue;
            }
            let mut app = app();
            let audio = app.record_audio();
            set_rung(&mut app, rung);
            for _ in 0..3 {
                app.ctx.say_event_with(
                    "Parking brake set. Press P to release it.",
                    SayEvent::queued()
                        .key("air_brake_lockout")
                        .category(category),
                );
                app.ctx
                    .say_with("Nice smooth shift.", Say::new().category(category));
            }
            assert!(
                audio.borrow().played.is_empty(),
                "{rung} {category:?}: {:?}",
                audio.borrow().played
            );
            assert!(app.event_lines().is_empty(), "{rung} {category:?}");
            app.shutdown();
        }
    }
}

#[test]
fn test_a_silent_category_asks_the_audio_layer_for_nothing() {
    // SILENT and EARCON both cut the words, and only EARCON is supposed to
    // sound anything -- the entire remaining difference at the voice
    // between "quiet" and "urgent_only".
    let mut app = app();
    let audio = app.record_audio();
    set_rung(&mut app, "urgent_only");

    app.ctx.say_event_with(
        "Load damage 43 percent.",
        SayEvent::queued().category(SpeechCategory::Status),
    );

    assert!(audio.borrow().played.is_empty());
    app.shutdown();
}

#[test]
fn test_raising_the_rung_still_speaks_an_active_silenced_condition() {
    // The silenced branches' earcon dedup must not write into the state the
    // SPEAKING path's `is_repeat` reads: silence a keyed STATUS condition at
    // quiet, raise the rung with the condition still active and its text
    // unchanged -- STATUS speaks at the raised rung (the Python test names
    // "coaching", an unknown rung that falls back to standard, where STATUS
    // is TRANSITIONS: a first occurrence under the key speaks).
    let mut app = app();
    set_rung(&mut app, "urgent_only");

    app.ctx.say_event_with(
        "Parking brake set. Press P to release it.",
        SayEvent::queued()
            .key("air_brake_lockout")
            .category(SpeechCategory::Status),
    );
    assert!(app.event_lines().is_empty()); // silenced at urgent_only

    set_rung(&mut app, "coaching");
    app.ctx.say_event_with(
        "Parking brake set. Press P to release it.",
        SayEvent::queued()
            .key("air_brake_lockout")
            .category(SpeechCategory::Status),
    );

    assert_eq!(
        app.event_lines(),
        vec!["Parking brake set. Press P to release it.".to_string()]
    );
    app.shutdown();
}

#[test]
fn test_an_unchanged_status_line_speaks_once() {
    let mut app = app();
    set_rung(&mut app, "coaching");

    for _ in 0..4 {
        app.ctx.say_event_with(
            "Gap to the truck ahead: 3 seconds.",
            SayEvent::queued()
                .key("lead_gap")
                .category(SpeechCategory::Status),
        );
    }

    assert_eq!(
        app.event_lines(),
        vec!["Gap to the truck ahead: 3 seconds.".to_string()]
    );
    app.shutdown();
}

#[test]
fn test_a_changed_status_line_speaks_again() {
    let mut app = app();
    set_rung(&mut app, "coaching");
    // A real gap-closing update arrives seconds apart, not in the same
    // instant -- advance the pacer's clock between the two calls so the
    // anti-backlog projection doesn't read the second line as starting
    // stale behind the first one's still-projected utterance.
    let clock = app.fake_pacer_clock();
    clock.set(1000.0);

    app.ctx.say_event_with(
        "Gap to the truck ahead: 3 seconds.",
        SayEvent::queued()
            .key("lead_gap")
            .category(SpeechCategory::Status),
    );
    clock.advance(5.0);
    app.ctx.say_event_with(
        "Gap to the truck ahead: 1 second.",
        SayEvent::queued()
            .key("lead_gap")
            .category(SpeechCategory::Status),
    );

    assert_eq!(
        app.event_lines(),
        vec![
            "Gap to the truck ahead: 3 seconds.".to_string(),
            "Gap to the truck ahead: 1 second.".to_string(),
        ]
    );
    app.shutdown();
}

#[test]
fn test_a_key_the_player_pressed_is_never_silenced_by_the_rung() {
    // The rung governs what the road volunteers, not what you asked for.
    // At quiet the cruise dial answered a press with a chime and no number
    // (owner, 2026-08-17). The RENDERING still follows the rung, so the
    // answer gets shorter, never absent.
    let mut app = app();
    set_rung(&mut app, "urgent_only");
    // The first-run gate outranks the rung; this is not a first drive.
    past_the_walkthrough(&mut app);
    app.clear_speech();

    // Volunteered: Urgent only turns a confirmation into a sound.
    app.ctx.say_with(
        "Transmission changed to manual.",
        Say::new().category(SpeechCategory::Confirmation),
    );
    assert!(app.main_lines().is_empty());

    app.ctx.player_asked(|ctx| {
        ctx.say_with(
            "Transmission changed to manual.",
            Say::new().category(SpeechCategory::Confirmation),
        )
    });
    assert!(
        !app.main_lines().is_empty(),
        "a line the player asked for was swallowed by the rung"
    );
    app.shutdown();
}

// `test_the_cruise_dial_answers_with_the_number_alone_at_quiet` is live in `crates/ff-core/src/settings/tests.rs`.

#[test]
fn test_standard_says_a_coaching_tip_once_per_leg() {
    // FIRST_OCCURRENCE, which the table promised and nothing implemented.
    let mut app = app();
    past_the_walkthrough(&mut app);
    set_rung(&mut app, "standard");
    app.clear_speech();

    for _ in 0..3 {
        app.ctx.say_event_with(
            "Keep it under 30 or the chains will not last.",
            SayEvent::queued()
                .key("chains_fast")
                .category(SpeechCategory::Coaching),
        );
    }
    assert_eq!(app.event_lines().len(), 1, "{:?}", app.event_lines());

    // A new leg is a fresh road: the tip is worth one more telling.
    // Asserted at the seam -- a same-text call here would be decided by the
    // pacer's own repeat window rather than by the rung.
    assert!(!app.ctx.ladder_said().is_empty());
    app.ctx.reset_ladder_leg_memory();
    assert!(app.ctx.ladder_said().is_empty());
    app.shutdown();
}

#[test]
fn test_standard_speaks_a_status_change_but_not_its_re_assertion() {
    // TRANSITIONS: enter, worsen, and clear -- not every re-fire.
    let mut app = app();
    past_the_walkthrough(&mut app);
    set_rung(&mut app, "standard");
    app.clear_speech();

    for _ in 0..3 {
        app.ctx.event_pacer.forget_condition("load_damage");
        app.ctx.say_event_with(
            "Load damage 43 percent.",
            SayEvent::queued()
                .key("load_damage")
                .category(SpeechCategory::Status),
        );
    }
    assert_eq!(app.event_lines().len(), 1, "{:?}", app.event_lines());

    // Worsened: a different number is a transition and speaks.
    app.ctx.event_pacer.forget_condition("load_damage");
    app.ctx.say_event_with(
        "Load damage 61 percent.",
        SayEvent::queued()
            .key("load_damage")
            .category(SpeechCategory::Status),
    );
    assert_eq!(app.event_lines().len(), 2, "{:?}", app.event_lines());

    // Cleared: the memory of what was last said must go with it, or a
    // condition that returns word-for-word never speaks again.
    assert_eq!(
        app.ctx.ladder_last().get("load_damage").map(String::as_str),
        Some("Load damage 61 percent.")
    );
    app.ctx.reset_event_condition("load_damage");
    assert!(!app.ctx.ladder_last().contains_key("load_damage"));
    app.shutdown();
}

#[test]
fn test_standard_speaks_a_keyless_status_line_again_when_it_says_something_new() {
    // Darren, 2026-08-19: 310 dropped lines in one leg on standard, the
    // default rung. A keyless line is a discrete moment, already
    // edge-gated at its own call site, so the rung must not suppress it.
    let mut app = app();
    past_the_walkthrough(&mut app);
    set_rung(&mut app, "standard");
    app.clear_speech();
    app.ctx.event_pacer =
        EventSpeechPacer::with_clock(freight_fate::app::FF_PACER, stepping_clock(60.0));

    // The truck slows and speeds up again over a long leg, so it reads 62
    // more than once -- minutes apart, a different moment each time.
    for mph in [62, 51, 62, 71, 62] {
        app.ctx.say_event_with(
            format!("{mph} miles per hour"),
            SayEvent::queued().category(SpeechCategory::Status),
        );
    }
    assert_eq!(app.event_lines().len(), 5, "{:?}", app.event_lines());
    app.shutdown();
}

#[test]
fn test_standard_says_the_same_lane_gap_line_about_a_later_car() {
    // The most-dropped line in Darren's log, seventeen times in one leg:
    // word-for-word identical and news every time, because it is a
    // different car each time.
    let mut app = app();
    past_the_walkthrough(&mut app);
    set_rung(&mut app, "standard");
    app.clear_speech();
    app.ctx.event_pacer =
        EventSpeechPacer::with_clock(freight_fate::app::FF_PACER, stepping_clock(60.0));

    let line = "Clear of the car. Right lane open.";
    for _ in 0..3 {
        app.ctx
            .say_event_with(line, SayEvent::queued().category(SpeechCategory::Status));
    }
    assert_eq!(app.event_lines().len(), 3, "{:?}", app.event_lines());
    app.shutdown();
}

#[test]
fn test_first_occurrence_still_holds_a_verbatim_tip_but_not_a_changed_one() {
    // FIRST_OCCURRENCE is "once per leg" for the LINE, not for the
    // condition behind it.
    let mut app = app();
    past_the_walkthrough(&mut app);
    set_rung(&mut app, "standard");
    app.clear_speech();
    app.ctx.event_pacer =
        EventSpeechPacer::with_clock(freight_fate::app::FF_PACER, stepping_clock(60.0));

    let tip = "The chains are hammering the pavement. Keep it under 30.";
    for _ in 0..3 {
        app.ctx.say_event_with(
            tip,
            SayEvent::queued()
                .key("chains_fast")
                .category(SpeechCategory::Coaching),
        );
    }
    assert_eq!(app.event_lines().len(), 1, "{:?}", app.event_lines());

    app.ctx.say_event_with(
        "The chains are hammering the pavement. Keep it under 25.",
        SayEvent::queued()
            .key("chains_fast")
            .category(SpeechCategory::Coaching),
    );
    assert_eq!(app.event_lines().len(), 2, "{:?}", app.event_lines());
    app.shutdown();
}

#[test]
fn test_only_standard_can_reach_the_already_said_gate() {
    // Quiet and urgent_only must not be able to drop a line as
    // already-said: a property of the TABLE, pinned so a later edit has to
    // be deliberate.
    for (rung, row) in DRIVING_SPEECH_DISPOSITIONS.iter() {
        if *rung == "standard" {
            continue;
        }
        let offenders: Vec<_> = row
            .iter()
            .filter(|(_, d)| matches!(d, Disposition::FirstOccurrence | Disposition::Transitions))
            .collect();
        assert!(
            offenders.is_empty(),
            "{rung} now reaches the already-said gate for {offenders:?}; \
             that gate is standard's alone (Darren, 2026-08-19)"
        );
    }
}

/// The transcript lines the ladder writes, byte for byte.
#[test]
fn review_excludes_silenced_events_and_keeps_spoken_tips_once() {
    // Suppressed events stay out of review; repeated tips retain one entry.
    let mut app = app();
    past_the_walkthrough(&mut app);
    set_rung(&mut app, "urgent_only");
    app.ctx.say_event_with(
        "Load damage 43 percent.",
        SayEvent::queued().category(SpeechCategory::Status),
    );
    assert!(app.ctx.message_log.messages.is_empty());
    set_rung(&mut app, "standard");
    app.ctx.event_pacer =
        EventSpeechPacer::with_clock(freight_fate::app::FF_PACER, stepping_clock(60.0));
    for _ in 0..2 {
        app.ctx.say_event_with(
            "Keep it under 30.",
            SayEvent::queued()
                .key("chains_fast")
                .category(SpeechCategory::Coaching),
        );
    }
    assert_eq!(app.event_lines(), vec!["Keep it under 30.".to_string()]);
    assert_eq!(logged_last(&app), "Keep it under 30.");
    assert_eq!(app.ctx.message_log.messages.len(), 1);
    app.shutdown();
}

#[test]
fn quiet_keeps_short_status_and_confirmation_words_in_review() {
    for category in [SpeechCategory::Status, SpeechCategory::Confirmation] {
        let mut app = app();
        set_rung(&mut app, "quiet");
        let audio = app.record_audio();
        app.ctx.say_event_with(
            SpokenMessage::with_terse("Automatic braking is now released.", "Braking released."),
            SayEvent::new().category(category),
        );
        assert_eq!(app.event_lines(), vec!["Braking released."]);
        assert_eq!(logged_last(&app), "Braking released.");
        assert!(audio.borrow().played.is_empty());
        app.shutdown();
    }
}

#[test]
fn urgent_only_omits_routine_costs_but_answers_an_explicit_request() {
    let mut app = app();
    set_rung(&mut app, "urgent_only");
    let line = SpokenMessage::with_terse("Toll paid, twenty dollars.", "Toll 20 dollars.");
    app.ctx
        .say_event_with(&line, SayEvent::new().category(SpeechCategory::Money));
    assert!(app.event_lines().is_empty());
    assert!(app.ctx.message_log.messages.is_empty());
    app.ctx.say_event_with(
        &line,
        SayEvent::new().category(SpeechCategory::Money).force(true),
    );
    assert_eq!(app.event_lines(), vec!["Toll 20 dollars."]);
    assert_eq!(logged_last(&app), "Toll 20 dollars.");
    app.shutdown();
}
