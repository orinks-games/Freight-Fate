use super::*;

/// Every code the iPhone's surface queues reads back as one gesture, and
/// every gesture the agent server names has a name that reads back.
#[test]
fn codes_and_names_read_back() {
    let gestures: Vec<Gesture> = (0..=49).filter_map(Gesture::from_code).collect();
    // 13 was retired.
    assert_eq!(gestures.len(), 49);
    for name in Gesture::names() {
        let gesture = Gesture::from_name(name).expect("a named gesture");
        assert_eq!(gesture.name(), name);
    }
}

/// The haptic table is `ff_haptic_kind` in `ios/ff_touch.m`, kind for kind,
/// which Android's table was ported from.
#[test]
fn the_haptics_follow_the_iphone_table() {
    use Gesture as G;
    let cases = [
        (G::Tap, Some(Haptic::Tick)),
        (G::SwipeUp, Some(Haptic::Tick)),
        (G::UpperHoldTap, Some(Haptic::Tick)),
        (G::Escape, Some(Haptic::Tick)),
        (G::HoldUpperBegan, Some(Haptic::Bump)),
        (G::HoldLeftBegan, Some(Haptic::Bump)),
        (G::StillHoldBegan, Some(Haptic::Bump)),
        (G::HornHoldBegan, Some(Haptic::Bump)),
        (G::SecondSteerEnded, Some(Haptic::Bump)),
        (G::HoldEnded, Some(Haptic::Bump)),
        (G::DeepSwipeDownBegan, Some(Haptic::Warning)),
        (G::EmergencyBrakeHoldBegan, Some(Haptic::Warning)),
        (G::RotateRight, Some(Haptic::Success)),
        (G::RotateLeft, Some(Haptic::Success)),
        (G::KeyboardConnected, None),
        (G::KeyboardDisconnected, None),
        (G::Travel(120), None),
    ];
    for (gesture, haptic) in cases {
        assert_eq!(gesture.haptic(), haptic, "{gesture:?}");
    }
    assert_eq!(
        [Haptic::Bump, Haptic::Tick, Haptic::Warning, Haptic::Success].map(Haptic::code),
        [0, 1, 2, 3]
    );
}

#[test]
fn finger_kinds_read_from_codes_and_names() {
    let kinds = [
        FingerKind::Down,
        FingerKind::Move,
        FingerKind::Up,
        FingerKind::Cancel,
    ];
    for (code, kind) in kinds.into_iter().enumerate() {
        assert_eq!(FingerKind::from_code(code as i32), Some(kind));
    }
    assert_eq!(FingerKind::from_code(4), None);
    assert_eq!(FingerKind::from_name("move"), Some(FingerKind::Move));
    assert_eq!(FingerKind::from_name("lift"), None);
}
