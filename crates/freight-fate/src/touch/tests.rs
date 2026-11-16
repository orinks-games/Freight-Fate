//! The gesture translator, the route table and the agent names.
use super::*;

impl Gesture {
    fn hold_events_output(self) -> TouchOutput {
        TouchOutput {
            events: self.hold_events(),
        }
    }
}

fn pressed(out: &TouchOutput) -> Vec<Key> {
    out.events
        .iter()
        .filter_map(|event| match event {
            InputEvent::KeyDown { key, .. } => Some(*key),
            _ => None,
        })
        .collect()
}

/// Every gesture on every kind of screen, written out: a new gesture or
/// a new mode fails here until its row says what it does.
#[test]
fn every_gesture_has_a_route_on_every_screen() {
    use Route as R;
    use TouchMode as M;
    let e = Action::EmergencyBrake;
    // (gesture, menu, field, wheel). Practice names every gesture.
    let table: &[(Gesture, Route, Route, Route)] = &[
        (
            Gesture::Tap,
            R::Screen(Some(Key::Comma)),
            R::Screen(Some(Key::Comma)),
            R::Screen(Some(Key::Comma)),
        ),
        (
            Gesture::DoubleTap,
            R::Screen(Some(Key::Return)),
            R::Screen(Some(Key::Return)),
            R::Screen(Some(Key::Return)),
        ),
        (
            Gesture::SwipeUp,
            R::Screen(Some(Key::Up)),
            R::Screen(Some(Key::Up)),
            R::Screen(Some(Key::Up)),
        ),
        (
            Gesture::SwipeDown,
            R::Screen(Some(Key::Down)),
            R::Screen(Some(Key::Down)),
            R::Screen(Some(Key::Down)),
        ),
        (
            Gesture::SwipeLeft,
            R::Screen(Some(Key::Left)),
            R::Screen(Some(Key::Left)),
            R::Screen(Some(Key::Left)),
        ),
        (
            Gesture::SwipeRight,
            R::Screen(Some(Key::Right)),
            R::Screen(Some(Key::Right)),
            R::Screen(Some(Key::Right)),
        ),
        (
            Gesture::TwoFingerTap,
            R::StopSpeech,
            R::StopSpeech,
            R::Key(Key::LCtrl),
        ),
        (
            Gesture::TwoFingerSwipeUp,
            R::Screen(Some(Key::F1)),
            R::Screen(Some(Key::F1)),
            R::Screen(Some(Key::F1)),
        ),
        (
            Gesture::TwoFingerSwipeDown,
            R::Screen(Some(Key::Escape)),
            R::Screen(Some(Key::Escape)),
            R::Screen(Some(Key::Escape)),
        ),
        (
            Gesture::TwoFingerSwipeLeft,
            R::Screen(Some(Key::Comma)),
            R::Screen(Some(Key::Comma)),
            R::Screen(Some(Key::Comma)),
        ),
        (
            Gesture::TwoFingerSwipeRight,
            R::Screen(Some(Key::Period)),
            R::Screen(Some(Key::Period)),
            R::Screen(Some(Key::Period)),
        ),
        (
            Gesture::ThreeFingerSwipeUp,
            R::Screen(Some(Key::Home)),
            R::Screen(Some(Key::Home)),
            R::Screen(Some(Key::Home)),
        ),
        (
            Gesture::ThreeFingerSwipeDown,
            R::Screen(Some(Key::End)),
            R::Screen(Some(Key::End)),
            R::Screen(Some(Key::End)),
        ),
        (
            Gesture::ThreeFingerSwipeLeft,
            R::Screen(Some(Key::PageUp)),
            R::Screen(Some(Key::PageUp)),
            R::Screen(Some(Key::PageUp)),
        ),
        (
            Gesture::ThreeFingerSwipeRight,
            R::Screen(Some(Key::PageDown)),
            R::Screen(Some(Key::PageDown)),
            R::Screen(Some(Key::PageDown)),
        ),
        (
            Gesture::ThreeFingerTap,
            R::Screen(Some(Key::Tab)),
            R::Screen(Some(Key::F2)),
            R::Screen(Some(Key::F2)),
        ),
        (
            Gesture::HoldUpperBegan,
            R::HoldKey(Key::Up),
            R::HoldKey(Key::Up),
            R::DriveThenHoldKey(Key::Up),
        ),
        (
            Gesture::HoldLowerBegan,
            R::HoldKey(Key::Down),
            R::HoldKey(Key::Down),
            R::HoldKey(Key::Down),
        ),
        (Gesture::HoldEnded, R::Lift, R::Lift, R::Lift),
        (
            Gesture::Escape,
            R::Screen(Some(Key::Escape)),
            R::Screen(Some(Key::Escape)),
            R::Screen(Some(Key::Escape)),
        ),
        (
            Gesture::MagicTap,
            R::Screen(Some(Key::Space)),
            R::Screen(Some(Key::Space)),
            R::Screen(Some(Key::Space)),
        ),
        (
            Gesture::Activate,
            R::Screen(Some(Key::Return)),
            R::Screen(Some(Key::Return)),
            R::Screen(Some(Key::Return)),
        ),
        (
            Gesture::Increment,
            R::Screen(Some(Key::Up)),
            R::Screen(Some(Key::Up)),
            R::Screen(Some(Key::Up)),
        ),
        (
            Gesture::Decrement,
            R::Screen(Some(Key::Down)),
            R::Screen(Some(Key::Down)),
            R::Screen(Some(Key::Down)),
        ),
        (
            Gesture::UpperHoldTap,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::UpperHoldDoubleTap,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::UpperHoldSwipeUp,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::UpperHoldSwipeDown,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::UpperHoldSwipeLeft,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::UpperHoldSwipeRight,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::LowerHoldTap,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::LowerHoldDoubleTap,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::LowerHoldSwipeUp,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::LowerHoldSwipeDown,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::LowerHoldSwipeLeft,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::LowerHoldSwipeRight,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::EmergencyBrakeHoldBegan,
            R::Nothing,
            R::Nothing,
            R::BoundHold(e),
        ),
        (
            Gesture::HornHoldBegan,
            R::Nothing,
            R::Nothing,
            R::BoundHold(Action::Horn),
        ),
        (
            Gesture::HoldLeftBegan,
            R::HoldKey(Key::Left),
            R::HoldKey(Key::Left),
            R::Steer(Key::Left),
        ),
        (
            Gesture::HoldRightBegan,
            R::HoldKey(Key::Right),
            R::HoldKey(Key::Right),
            R::Steer(Key::Right),
        ),
        (
            Gesture::DeepSwipeDownBegan,
            R::Screen(Some(Key::Down)),
            R::Screen(Some(Key::Down)),
            R::DriveThenBoundHold(e),
        ),
        (
            Gesture::StillHoldBegan,
            R::Nothing,
            R::Nothing,
            R::BoundHold(Action::Straighten),
        ),
        (
            Gesture::SecondSteerLeftBegan,
            R::HoldKey(Key::Left),
            R::HoldKey(Key::Left),
            R::Steer(Key::Left),
        ),
        (
            Gesture::SecondSteerRightBegan,
            R::HoldKey(Key::Right),
            R::HoldKey(Key::Right),
            R::Steer(Key::Right),
        ),
        (
            Gesture::SecondSteerEnded,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::RotateRight,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::RotateLeft,
            R::Screen(None),
            R::Screen(None),
            R::Screen(None),
        ),
        (
            Gesture::KeyboardConnected,
            R::Keyboard,
            R::Keyboard,
            R::Keyboard,
        ),
        (
            Gesture::KeyboardDisconnected,
            R::Keyboard,
            R::Keyboard,
            R::Keyboard,
        ),
    ];
    assert_eq!(TouchMode::ALL.len(), 4, "a new mode needs its column");
    for code in 0..64 {
        if let Some(gesture) = Gesture::from_code(code) {
            assert!(
                table.iter().any(|(g, ..)| *g == gesture),
                "{gesture:?} has no row"
            );
        }
    }
    for (gesture, menu, field, wheel) in table {
        for (mode, expected) in [(M::Menu, menu), (M::Field, field), (M::Wheel, wheel)] {
            assert_eq!(route(mode, *gesture), *expected, "{gesture:?} in {mode:?}");
        }
        let practice = route(M::Practice, *gesture);
        let keyboard = matches!(
            gesture,
            Gesture::KeyboardConnected | Gesture::KeyboardDisconnected
        );
        assert_eq!(
            practice,
            if keyboard { R::Keyboard } else { R::Practice },
            "{gesture:?} in practice"
        );
    }
}

#[test]
fn every_gesture_has_an_agent_name() {
    for code in 0..64 {
        let Some(gesture) = Gesture::from_code(code) else {
            continue;
        };
        assert!(!gesture.name().is_empty(), "{gesture:?}");
        assert_eq!(Gesture::from_name(gesture.name()), Some(gesture));
    }
    let mut names: Vec<&str> = Gesture::names().collect();
    let count = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), count, "a name is used twice");
}

#[test]
fn every_native_code_round_trips_and_unknown_codes_are_ignored() {
    for code in (0..50).filter(|code| *code != 13) {
        assert!(Gesture::from_code(code).is_some(), "code {code}");
    }
    // 13 was the three-finger double tap, retired with the keyboard toggle.
    assert_eq!(Gesture::from_code(13), None);
    assert_eq!(Gesture::from_code(50), None);
    assert_eq!(Gesture::from_code(-1), None);
}

#[test]
fn discrete_gestures_reach_the_state_as_gestures() {
    let mut touch = TouchInput::new();
    for gesture in [
        Gesture::Tap,
        Gesture::MagicTap,
        Gesture::ThreeFingerTap,
        Gesture::UpperHoldTap,
    ] {
        assert_eq!(
            touch.handle(gesture).events,
            vec![InputEvent::Gesture(gesture)]
        );
    }
    assert_eq!(touch.held(), None);
}

#[test]
fn menu_gestures_press_and_release_the_menu_keys() {
    for (gesture, key) in [
        (Gesture::SwipeUp, Key::Up),
        (Gesture::SwipeDown, Key::Down),
        (Gesture::DeepSwipeDownBegan, Key::Down),
        (Gesture::DoubleTap, Key::Return),
        (Gesture::Activate, Key::Return),
        (Gesture::TwoFingerSwipeDown, Key::Escape),
        (Gesture::Escape, Key::Escape),
        (Gesture::Increment, Key::Up),
        (Gesture::Decrement, Key::Down),
        (Gesture::TwoFingerSwipeUp, Key::F1),
        (Gesture::ThreeFingerTap, Key::Tab),
        (Gesture::TwoFingerTap, Key::LCtrl),
    ] {
        assert_eq!(
            gesture.key_events(),
            vec![
                InputEvent::KeyDown {
                    key,
                    mods: Mods::NONE,
                    text: None,
                    repeat: false,
                },
                InputEvent::KeyUp {
                    key,
                    mods: Mods::NONE,
                },
            ],
            "{gesture:?}"
        );
    }
}

#[test]
fn a_second_finger_during_a_hold_presses_no_key() {
    assert!(Gesture::UpperHoldTap.key_events().is_empty());
    assert!(Gesture::LowerHoldSwipeRight.key_events().is_empty());
}

#[test]
fn review_gestures_carry_the_typed_character() {
    assert!(matches!(
        Gesture::TwoFingerSwipeLeft.key_events().first(),
        Some(InputEvent::KeyDown {
            key: Key::Comma,
            text: Some(','),
            ..
        })
    ));
    assert!(matches!(
        Gesture::MagicTap.key_events().first(),
        Some(InputEvent::KeyDown {
            key: Key::Space,
            text: Some(' '),
            ..
        })
    ));
}

#[test]
fn holds_keep_the_pedal_down_until_the_finger_lifts() {
    let mut touch = TouchInput::new();
    let out = touch.handle(Gesture::HoldUpperBegan);
    assert_eq!(
        out.events,
        vec![InputEvent::Gesture(Gesture::HoldUpperBegan)]
    );
    assert_eq!(
        pressed(&Gesture::HoldUpperBegan.hold_events_output()),
        vec![Key::Up]
    );
    assert_eq!(touch.held(), Some(Key::Up));

    let out = touch.handle(Gesture::HoldEnded);
    assert_eq!(
        out.events,
        vec![InputEvent::KeyUp {
            key: Key::Up,
            mods: Mods::NONE,
        }]
    );
    assert_eq!(touch.held(), None);
}

#[test]
fn emergency_brake_and_horn_holds_are_left_for_live_bindings() {
    for gesture in [
        Gesture::EmergencyBrakeHoldBegan,
        Gesture::HornHoldBegan,
        Gesture::StillHoldBegan,
    ] {
        let mut touch = TouchInput::new();
        assert_eq!(gesture.held_key(), None);
        assert_eq!(
            touch.handle(gesture).events,
            vec![InputEvent::Gesture(gesture)]
        );
        assert_eq!(touch.held(), None);
        assert_eq!(
            touch.handle(Gesture::HoldEnded).events,
            vec![InputEvent::Gesture(Gesture::HoldEnded)]
        );
    }
}

#[test]
fn a_new_hold_releases_the_old_one_first() {
    let mut touch = TouchInput::new();
    touch.handle(Gesture::HoldUpperBegan);
    let out = touch.handle(Gesture::HoldLowerBegan);
    assert_eq!(
        out.events,
        vec![
            InputEvent::KeyUp {
                key: Key::Up,
                mods: Mods::NONE,
            },
            InputEvent::Gesture(Gesture::HoldLowerBegan),
        ]
    );
    assert_eq!(touch.held(), Some(Key::Down));
}

#[test]
fn a_lift_without_a_pedal_reaches_the_live_binding_handler() {
    let mut touch = TouchInput::new();
    assert_eq!(
        touch.handle(Gesture::HoldEnded).events,
        vec![InputEvent::Gesture(Gesture::HoldEnded)]
    );
}

#[test]
fn steering_holds_keep_the_wheel_key_down_until_the_finger_lifts() {
    let mut touch = TouchInput::new();
    touch.handle(Gesture::HoldLeftBegan);
    assert_eq!(touch.held(), Some(Key::Left));
    assert!(Gesture::HoldLeftBegan.steers());
    assert_eq!(
        touch.handle(Gesture::HoldEnded).events,
        vec![InputEvent::KeyUp {
            key: Key::Left,
            mods: Mods::NONE,
        }]
    );
}

#[test]
fn a_second_finger_steers_under_the_held_pedal() {
    let mut touch = TouchInput::new();
    touch.handle(Gesture::HoldUpperBegan);
    assert_eq!(
        touch.handle(Gesture::SecondSteerRightBegan).events,
        vec![InputEvent::Gesture(Gesture::SecondSteerRightBegan)]
    );
    assert_eq!(
        touch.handle(Gesture::SecondSteerEnded).events,
        vec![InputEvent::KeyUp {
            key: Key::Right,
            mods: Mods::NONE,
        }]
    );
    assert_eq!(touch.held(), Some(Key::Up));
    touch.handle(Gesture::SecondSteerLeftBegan);
    assert_eq!(
        touch.handle(Gesture::HoldEnded).events,
        vec![
            InputEvent::KeyUp {
                key: Key::Left,
                mods: Mods::NONE,
            },
            InputEvent::KeyUp {
                key: Key::Up,
                mods: Mods::NONE,
            },
        ]
    );
}

#[test]
fn a_deep_swipe_lets_the_brake_key_go_first() {
    let mut touch = TouchInput::new();
    touch.handle(Gesture::HoldLowerBegan);
    assert_eq!(
        touch.handle(Gesture::DeepSwipeDownBegan).events,
        vec![
            InputEvent::KeyUp {
                key: Key::Down,
                mods: Mods::NONE,
            },
            InputEvent::Gesture(Gesture::DeepSwipeDownBegan),
        ]
    );
    assert_eq!(touch.held(), None);
    assert_eq!(
        touch.handle(Gesture::HoldEnded).events,
        vec![InputEvent::Gesture(Gesture::HoldEnded)]
    );
}
