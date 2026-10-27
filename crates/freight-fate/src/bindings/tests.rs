//! The binding table's tests, kept apart so the table stays readable.

use super::*;

#[test]
fn every_default_chord_is_saveable_and_unreserved() {
    for action in Action::all() {
        for chord in action.default_chords() {
            assert!(chord.saved().is_some(), "{action:?}");
            assert_eq!(reserved_key_reason(chord), None, "{action:?}");
            assert_eq!(Chord::parse(&chord.saved().unwrap()), Some(*chord));
        }
        for chord in action.default_pad_chords() {
            assert_eq!(reserved_pad_reason(chord), None, "{action:?}");
            assert_eq!(PadChord::parse(&chord.saved()), Some(*chord));
        }
        assert_eq!(Action::from_id(action.id()), Some(action));
    }
}

#[test]
fn no_two_actions_share_a_default_chord() {
    let mut seen: HashMap<Chord, Action> = HashMap::new();
    for action in Action::all() {
        for chord in action.default_chords() {
            assert!(
                seen.insert(*chord, action).is_none(),
                "{chord:?} is both {action:?} and {:?}",
                seen[chord]
            );
        }
    }
    let mut seen: HashMap<PadChord, Action> = HashMap::new();
    for action in Action::all() {
        for chord in action.default_pad_chords() {
            assert!(
                seen.insert(*chord, action).is_none(),
                "{chord:?} is both {action:?} and {:?}",
                seen[chord]
            );
        }
    }
}

#[test]
fn a_saved_binding_keeps_its_key_when_an_update_adds_that_default() {
    // Horn moved to slash on a build before Straighten took slash by
    // default: the horn keeps it, and straighten waits for a key.
    let settings = Settings {
        key_bindings: "horn=slash".into(),
        ..Settings::default()
    };
    let b = KeyBindings::from_settings(&settings);
    assert_eq!(b.action_for(Key::Slash, Mods::NONE), Some(Action::Horn));
    assert!(b.chords(Action::Straighten).is_empty());
}

#[test]
fn a_saved_pad_binding_keeps_a_new_radio_default() {
    let settings = Settings {
        pad_bindings: "horn=rstick_right".into(),
        ..Settings::default()
    };
    let bindings = KeyBindings::from_settings(&settings);
    assert_eq!(
        bindings.pad_action_for(ControllerButton::RightStickRight, false),
        Some(Action::Horn)
    );
    assert!(bindings.pad_chords(Action::RadioTuneNext).is_empty());
}

#[test]
fn a_chord_with_unbound_modifiers_falls_back_to_the_bare_key() {
    let b = KeyBindings::default();
    assert_eq!(b.action_for(Key::E, Mods::ALT), Some(Action::Engine));
    assert_eq!(b.action_for(Key::W, Mods::SHIFT), Some(Action::ShiftUp));
    assert_eq!(
        b.action_for(Key::K, Mods::SHIFT),
        Some(Action::CruiseResume)
    );
    assert_eq!(b.action_for(Key::J, Mods::ALT), Some(Action::AutoJake));
    assert_eq!(b.action_for(Key::Num1, Mods::ALT), Some(Action::PlaceState));
    assert_eq!(
        b.action_for(Key::Num1, Mods::NONE),
        Some(Action::JakeStage1)
    );
    assert_eq!(b.action_for(Key::Escape, Mods::NONE), None);
}

#[test]
fn moving_a_key_frees_the_old_one_and_refuses_a_taken_one() {
    let mut b = KeyBindings::default();
    assert_eq!(
        b.set_chord(Action::Engine, Chord::plain(Key::T)),
        Rebind::Taken(Action::Rest)
    );
    assert_eq!(
        b.set_chord(Action::Engine, Chord::plain(Key::Backslash)),
        Rebind::Done
    );
    assert_eq!(
        b.action_for(Key::Backslash, Mods::NONE),
        Some(Action::Engine)
    );
    assert_eq!(b.action_for(Key::E, Mods::NONE), None);
    assert_eq!(
        b.set_chord(Action::Engine, Chord::plain(Key::Backslash)),
        Rebind::Unchanged
    );
    assert_eq!(
        b.set_chord(Action::Engine, Chord::plain(Key::Escape)),
        Rebind::Reserved("Escape is the pause menu")
    );
    // Back to the default clears the override rather than recording it.
    assert_eq!(
        b.set_chord(Action::Engine, Chord::plain(Key::E)),
        Rebind::Done
    );
    assert!(b.is_default());
}

#[test]
fn bindings_round_trip_through_settings() {
    let mut b = KeyBindings::default();
    b.set_chord(Action::Engine, Chord::alt(Key::Backslash));
    b.set_chord(Action::Accelerate, Chord::plain(Key::F5));
    b.set_pad_chord(Action::Horn, PadChord::modified(ControllerButton::Paddle1));
    b.set_pad_chord(
        Action::RadioTuneNext,
        PadChord::plain(ControllerButton::Paddle2),
    );
    let mut settings = Settings::default();
    b.store(&mut settings);
    assert_eq!(settings.key_bindings, "accelerate=f5;engine=alt+backslash");
    assert_eq!(
        settings.pad_bindings,
        "horn=mod+paddle_1;radio_tune_next=paddle_2"
    );
    assert_eq!(KeyBindings::from_settings(&settings), b);
    // A default table writes nothing.
    KeyBindings::default().store(&mut settings);
    assert_eq!(settings.key_bindings, "");
    assert_eq!(settings.pad_bindings, "");
}

#[test]
fn garbage_in_the_settings_is_ignored() {
    let settings = Settings {
        key_bindings: "engine=nope;teleport=t;horn=alt+;=;;accelerate=w".to_string(),
        ..Settings::default()
    };
    let b = KeyBindings::from_settings(&settings);
    assert_eq!(b.chords(Action::Engine), vec![Chord::plain(Key::E)]);
    assert_eq!(b.chords(Action::Accelerate), vec![Chord::plain(Key::W)]);
}

#[test]
fn default_hint_phrases_match_the_fixed_table() {
    let b = KeyBindings::default();
    for (hint, _) in ff_core::input_hints::HINTS {
        let expected_kb = ff_core::input_hints::control_hint(hint, ff_core::input_hints::KEYBOARD);
        if let Some(phrase) = b.hint_key_phrase(hint) {
            assert_eq!(phrase, expected_kb, "{hint}");
        }
        let expected_pad =
            ff_core::input_hints::control_hint(hint, ff_core::input_hints::CONTROLLER);
        if let Some(phrase) = b.hint_pad_phrase(hint) {
            assert_eq!(phrase, expected_pad, "{hint}");
        }
    }
}

#[test]
fn a_moved_key_changes_the_hint() {
    let mut b = KeyBindings::default();
    b.set_chord(Action::Engine, Chord::plain(Key::Backslash));
    assert_eq!(b.hint_key_phrase("engine").as_deref(), Some("backslash"));
    b.set_pad_chord(Action::Engine, PadChord::plain(ControllerButton::Paddle2));
    assert_eq!(b.hint_pad_phrase("engine").as_deref(), Some("paddle 2"));
}

/// Saved strings as today's builds write them (every field in
/// `Action::all` order, `;` between entries), read back and written out
/// byte for byte.
#[test]
fn todays_saved_strings_round_trip_byte_for_byte() {
    let key_bindings = "accelerate=f5;straighten=ctrl+alt+s;engine=alt+z;\
                        horn=slash;cruise_resume=shift+backspace;\
                        copy_last_spoken=ctrl+shift+v;place_state=alt+kp_9;\
                        radio_now_playing=f12";
    let pad_bindings = "engine=paddle_3;horn=mod+paddle_1;status=mod+back;\
                        route=mod+touchpad;radio_tune_next=paddle_2;\
                        radio_volume_up=mod+rstick_up;radio_category_prev=rstick_down";
    let settings = Settings {
        key_bindings: key_bindings.to_string(),
        pad_bindings: pad_bindings.to_string(),
        ..Settings::default()
    };
    let b = KeyBindings::from_settings(&settings);
    let mut out = Settings::default();
    b.store(&mut out);
    assert_eq!(out.key_bindings, key_bindings);
    assert_eq!(out.pad_bindings, pad_bindings);
    assert_eq!(out.touch_bindings, settings.touch_bindings);
    assert_eq!(
        b.pad_action_for(ControllerButton::RightStickDown, false),
        Some(Action::RadioCategoryPrev)
    );
    assert_eq!(b.action_for(Key::Slash, Mods::NONE), Some(Action::Horn));
    assert!(b
        .chords(Action::Straighten)
        .iter()
        .all(|c| c.key != Key::Slash));
}

/// A saved move onto a key that became reserved later (or a hand-edited
/// one) is kept, as it always was: only the rebinding screen refuses
/// reserved keys. Moves for a control the device lacks are dropped.
#[test]
fn a_saved_move_onto_a_reserved_key_is_kept_and_one_off_the_device_dropped() {
    let settings = Settings {
        key_bindings: "horn=comma;cruise_up=f6".to_string(),
        pad_bindings: "horn=start;lane=paddle_1".to_string(),
        ..Settings::default()
    };
    let b = KeyBindings::from_settings(&settings);
    assert_eq!(b.action_for(Key::Comma, Mods::NONE), Some(Action::Horn));
    assert_eq!(
        b.pad_action_for(ControllerButton::Start, false),
        Some(Action::Horn)
    );
    let mut out = Settings::default();
    b.store(&mut out);
    assert_eq!(out.key_bindings, "horn=comma");
    assert_eq!(out.pad_bindings, "horn=start");
}

#[test]
fn every_reserved_reason_reads_as_it_always_has() {
    let mut b = KeyBindings::default();
    for (chord, reason) in [
        (Chord::plain(Key::Escape), "Escape is the pause menu"),
        (Chord::shift(Key::Escape), "Escape is the pause menu"),
        (Chord::plain(Key::Return), "Enter confirms"),
        (Chord::plain(Key::KpEnter), "Enter confirms"),
        (Chord::plain(Key::F1), "F1 is help"),
        (Chord::plain(Key::F2), "F2 lists the driving commands"),
        (Chord::alt(Key::F2), "F2 lists the driving commands"),
        (
            Chord::plain(Key::LShift),
            "a modifier key on its own cannot be a shortcut",
        ),
        (
            Chord::plain(Key::Equals),
            "plus and minus set the cruise target",
        ),
        (Chord::plain(Key::PageUp), "the radio dial keys are fixed"),
        (
            Chord::plain(Key::Comma),
            "the message review keys are fixed",
        ),
        (Chord::plain(Key::Other(7)), "that key cannot be used"),
        (
            Chord {
                key: Key::C,
                mods: Mods::CTRL,
            },
            "Control C copies the message in review",
        ),
    ] {
        assert_eq!(
            b.set_chord(Action::Horn, chord),
            Rebind::Reserved(reason),
            "{chord:?}"
        );
    }
    // Enter and the review keys are only kept bare.
    assert_eq!(
        b.set_chord(Action::Horn, Chord::alt(Key::Return)),
        Rebind::Done
    );
    assert_eq!(
        b.set_chord(Action::Horn, Chord::alt(Key::Comma)),
        Rebind::Done
    );
    for (chord, reason) in [
        (
            PadChord::plain(ControllerButton::LeftShoulder),
            "the left bumper is the clutch",
        ),
        (
            PadChord::modified(ControllerButton::RightShoulder),
            "the right bumper is the second layer",
        ),
        (
            PadChord::plain(ControllerButton::Guide),
            "the guide button belongs to the system",
        ),
        (PadChord::plain(ControllerButton::Start), "Start is pause"),
        (
            PadChord::plain(ControllerButton::Back),
            "Back stops the driving voice and reads help",
        ),
    ] {
        assert_eq!(
            b.set_pad_chord(Action::Horn, chord),
            Rebind::Reserved(reason),
            "{chord:?}"
        );
    }
    // Start and Back are free on the second layer (they are Status's and
    // the radio's there by default).
    assert_eq!(
        b.set_pad_chord(Action::Horn, PadChord::modified(ControllerButton::Start)),
        Rebind::Taken(Action::Status)
    );
    assert_eq!(
        b.set_pad_chord(Action::Horn, PadChord::modified(ControllerButton::Back)),
        Rebind::Taken(Action::RadioNowPlaying)
    );
    b.reset();
    assert_eq!(
        b.set_pad_chord(
            Action::Status,
            PadChord::modified(ControllerButton::Paddle4)
        ),
        Rebind::Done
    );
    assert_eq!(
        b.set_pad_chord(Action::Horn, PadChord::modified(ControllerButton::Start)),
        Rebind::Done
    );
}
