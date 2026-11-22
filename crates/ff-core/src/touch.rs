//! Touch gestures, and the recogniser that reads them from fingers.
//!
//! [`Gesture`] is everything a touch screen can ask of the game: the codes
//! the iPhone's native surface (`ios/ff_touch.m`) queues, and what
//! [`recognizer::Recognizer`] makes of raw finger events on Android, and on
//! iPhone and iPad with Touch gestures, New gesture reading on. Neither
//! knows about keys or screens: the game crate's `touch` module says what a
//! gesture does where it lands. Nothing here touches a platform, so every
//! rule is tested on the desktop.

pub mod recognizer;

/// A gesture a touch screen recognized: the native surface on iPhone and
/// iPad, or [`recognizer::Recognizer`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Gesture {
    Tap,
    DoubleTap,
    SwipeUp,
    SwipeDown,
    SwipeLeft,
    SwipeRight,
    TwoFingerTap,
    TwoFingerSwipeUp,
    TwoFingerSwipeDown,
    TwoFingerSwipeLeft,
    TwoFingerSwipeRight,
    ThreeFingerSwipeUp,
    ThreeFingerSwipeDown,
    ThreeFingerSwipeLeft,
    ThreeFingerSwipeRight,
    /// Opens the driving command list, and reads a name field back.
    ThreeFingerTap,
    /// A swipe up and hold anywhere on the screen (gas).
    HoldUpperBegan,
    /// A swipe down and hold anywhere on the screen (brake).
    HoldLowerBegan,
    HoldEnded,
    /// The two-finger scrub: the touch surface's own recogniser, or
    /// VoiceOver's escape action with direct touch off.
    Escape,
    /// VoiceOver's two-finger double tap.
    MagicTap,
    /// VoiceOver's double tap, when direct touch is not active.
    Activate,
    /// VoiceOver's swipe up on the adjustable element.
    Increment,
    /// VoiceOver's swipe down on the adjustable element.
    Decrement,
    /// A second finger's tap while gas is held.
    UpperHoldTap,
    UpperHoldDoubleTap,
    UpperHoldSwipeUp,
    UpperHoldSwipeDown,
    UpperHoldSwipeLeft,
    UpperHoldSwipeRight,
    /// A second finger's tap while brake is held.
    LowerHoldTap,
    LowerHoldDoubleTap,
    LowerHoldSwipeUp,
    LowerHoldSwipeDown,
    LowerHoldSwipeLeft,
    LowerHoldSwipeRight,
    /// The old two-finger emergency brake hold; the native side no longer
    /// sends it, and the deep swipe took its place.
    EmergencyBrakeHoldBegan,
    /// Two fingers held anywhere (horn).
    HornHoldBegan,
    /// A swipe left and hold (steer left).
    HoldLeftBegan,
    /// A swipe right and hold (steer right).
    HoldRightBegan,
    /// A swipe down about three flicks deep: the emergency brake while
    /// moving, the parking brake when stopped.
    DeepSwipeDownBegan,
    /// One finger held still (straighten the wheel).
    StillHoldBegan,
    /// A second finger's swipe left and hold while a pedal is held.
    SecondSteerLeftBegan,
    SecondSteerRightBegan,
    /// The second finger's steering lifted, the pedal still held.
    SecondSteerEnded,
    /// Two fingers turned right like a key (start the engine).
    RotateRight,
    /// Two fingers turned left (stop the engine).
    RotateLeft,
    /// A hardware keyboard connected or went away.
    KeyboardConnected,
    KeyboardDisconnected,
    /// How far a one-finger hold travelled along its direction, in whole
    /// points, sent as it lifts while Practice gestures is open.
    Travel(u16),
}

/// The first code of the swipe lengths: [`TRAVEL_CODE_BASE`] plus the points.
pub const TRAVEL_CODE_BASE: i32 = 1000;
/// The longest swipe length a code carries; a longer one reads as this.
pub const MAX_TRAVEL_POINTS: u16 = 999;

impl Gesture {
    /// The code `ios/ff_touch.m` queues for this gesture.
    pub fn from_code(code: i32) -> Option<Gesture> {
        Some(match code {
            0 => Gesture::Tap,
            1 => Gesture::DoubleTap,
            2 => Gesture::SwipeUp,
            3 => Gesture::SwipeDown,
            4 => Gesture::SwipeLeft,
            5 => Gesture::SwipeRight,
            6 => Gesture::TwoFingerTap,
            7 => Gesture::TwoFingerSwipeUp,
            8 => Gesture::TwoFingerSwipeDown,
            9 => Gesture::TwoFingerSwipeLeft,
            10 => Gesture::TwoFingerSwipeRight,
            11 => Gesture::ThreeFingerSwipeUp,
            12 => Gesture::ThreeFingerSwipeDown,
            14 => Gesture::HoldUpperBegan,
            15 => Gesture::HoldLowerBegan,
            16 => Gesture::HoldEnded,
            17 => Gesture::Escape,
            18 => Gesture::MagicTap,
            19 => Gesture::Activate,
            20 => Gesture::Increment,
            21 => Gesture::Decrement,
            22 => Gesture::ThreeFingerSwipeLeft,
            23 => Gesture::ThreeFingerSwipeRight,
            24 => Gesture::ThreeFingerTap,
            25 => Gesture::UpperHoldTap,
            26 => Gesture::UpperHoldDoubleTap,
            27 => Gesture::UpperHoldSwipeUp,
            28 => Gesture::UpperHoldSwipeDown,
            29 => Gesture::UpperHoldSwipeLeft,
            30 => Gesture::UpperHoldSwipeRight,
            31 => Gesture::LowerHoldTap,
            32 => Gesture::LowerHoldDoubleTap,
            33 => Gesture::LowerHoldSwipeUp,
            34 => Gesture::LowerHoldSwipeDown,
            35 => Gesture::LowerHoldSwipeLeft,
            36 => Gesture::LowerHoldSwipeRight,
            37 => Gesture::EmergencyBrakeHoldBegan,
            38 => Gesture::HornHoldBegan,
            39 => Gesture::HoldLeftBegan,
            40 => Gesture::HoldRightBegan,
            41 => Gesture::DeepSwipeDownBegan,
            42 => Gesture::StillHoldBegan,
            43 => Gesture::SecondSteerLeftBegan,
            44 => Gesture::SecondSteerRightBegan,
            45 => Gesture::SecondSteerEnded,
            46 => Gesture::RotateRight,
            47 => Gesture::RotateLeft,
            48 => Gesture::KeyboardConnected,
            49 => Gesture::KeyboardDisconnected,
            travel
                if (TRAVEL_CODE_BASE..=TRAVEL_CODE_BASE + i32::from(MAX_TRAVEL_POINTS))
                    .contains(&travel) =>
            {
                Gesture::Travel(u16::try_from(travel - TRAVEL_CODE_BASE).ok()?)
            }
            _ => return None,
        })
    }

    /// A hold that steers, which full lane keeping leaves to the flicks.
    pub fn steers(self) -> bool {
        matches!(
            self,
            Gesture::HoldLeftBegan
                | Gesture::HoldRightBegan
                | Gesture::SecondSteerLeftBegan
                | Gesture::SecondSteerRightBegan
        )
    }

    /// The haptic this gesture gets, as `ios/ff_touch.m` assigns them: a
    /// bump for a hold starting or ending, a tick for one step, a warning
    /// for the deep swipe, a success for the engine key. The keyboard
    /// notices and a swipe's length get none.
    pub fn haptic(self) -> Option<Haptic> {
        use Gesture as G;
        Some(match self {
            G::KeyboardConnected | G::KeyboardDisconnected | G::Travel(_) => return None,
            G::HoldUpperBegan
            | G::HoldLowerBegan
            | G::HoldLeftBegan
            | G::HoldRightBegan
            | G::StillHoldBegan
            | G::HornHoldBegan
            | G::SecondSteerLeftBegan
            | G::SecondSteerRightBegan
            | G::SecondSteerEnded
            | G::HoldEnded => Haptic::Bump,
            G::DeepSwipeDownBegan | G::EmergencyBrakeHoldBegan => Haptic::Warning,
            G::RotateRight | G::RotateLeft => Haptic::Success,
            _ => Haptic::Tick,
        })
    }

    /// The name the agent server's `gesture` tool takes.
    pub fn name(self) -> &'static str {
        NAMES
            .iter()
            .find(|(gesture, _)| *gesture == self)
            .map_or("", |(_, name)| name)
    }

    /// The gesture an agent names.
    pub fn from_name(name: &str) -> Option<Gesture> {
        NAMES
            .iter()
            .find(|(_, n)| *n == name)
            .map(|(gesture, _)| *gesture)
    }

    /// Every gesture the agent server can send, by name.
    pub fn names() -> impl Iterator<Item = &'static str> {
        NAMES.iter().map(|(_, name)| *name)
    }
}

/// The agent server's names, one per gesture the screen can send, plus the
/// keyboard notices, so a session can put spoken hints on gestures.
const NAMES: &[(Gesture, &str)] = &[
    (Gesture::KeyboardDisconnected, "keyboard_disconnected"),
    (Gesture::KeyboardConnected, "keyboard_connected"),
    (Gesture::Tap, "tap"),
    (Gesture::DoubleTap, "double_tap"),
    (Gesture::SwipeUp, "flick_up"),
    (Gesture::SwipeDown, "flick_down"),
    (Gesture::SwipeLeft, "flick_left"),
    (Gesture::SwipeRight, "flick_right"),
    (Gesture::TwoFingerTap, "two_finger_tap"),
    (Gesture::MagicTap, "two_finger_double_tap"),
    (Gesture::Escape, "two_finger_scrub"),
    (Gesture::TwoFingerSwipeUp, "two_finger_flick_up"),
    (Gesture::TwoFingerSwipeDown, "two_finger_flick_down"),
    (Gesture::TwoFingerSwipeLeft, "two_finger_flick_left"),
    (Gesture::TwoFingerSwipeRight, "two_finger_flick_right"),
    (Gesture::HornHoldBegan, "two_finger_hold"),
    (Gesture::RotateRight, "two_finger_turn_right"),
    (Gesture::RotateLeft, "two_finger_turn_left"),
    (Gesture::ThreeFingerTap, "three_finger_tap"),
    (Gesture::ThreeFingerSwipeUp, "three_finger_flick_up"),
    (Gesture::ThreeFingerSwipeDown, "three_finger_flick_down"),
    (Gesture::ThreeFingerSwipeLeft, "three_finger_flick_left"),
    (Gesture::ThreeFingerSwipeRight, "three_finger_flick_right"),
    (Gesture::HoldUpperBegan, "gas_hold"),
    (Gesture::HoldLowerBegan, "brake_hold"),
    (Gesture::HoldLeftBegan, "steer_left_hold"),
    (Gesture::HoldRightBegan, "steer_right_hold"),
    (Gesture::DeepSwipeDownBegan, "deep_swipe"),
    (Gesture::StillHoldBegan, "still_hold"),
    (Gesture::HoldEnded, "lift"),
    (Gesture::UpperHoldTap, "gas_hold_tap"),
    (Gesture::UpperHoldDoubleTap, "gas_hold_double_tap"),
    (Gesture::UpperHoldSwipeUp, "gas_hold_flick_up"),
    (Gesture::UpperHoldSwipeDown, "gas_hold_flick_down"),
    (Gesture::UpperHoldSwipeLeft, "gas_hold_flick_left"),
    (Gesture::UpperHoldSwipeRight, "gas_hold_flick_right"),
    (Gesture::LowerHoldTap, "brake_hold_tap"),
    (Gesture::LowerHoldDoubleTap, "brake_hold_double_tap"),
    (Gesture::LowerHoldSwipeUp, "brake_hold_flick_up"),
    (Gesture::LowerHoldSwipeDown, "brake_hold_flick_down"),
    (Gesture::LowerHoldSwipeLeft, "brake_hold_flick_left"),
    (Gesture::LowerHoldSwipeRight, "brake_hold_flick_right"),
    (Gesture::SecondSteerLeftBegan, "second_finger_steer_left"),
    (Gesture::SecondSteerRightBegan, "second_finger_steer_right"),
    (Gesture::SecondSteerEnded, "second_finger_lift"),
    (Gesture::EmergencyBrakeHoldBegan, "emergency_brake_hold"),
    (Gesture::Activate, "voiceover_double_tap"),
    (Gesture::Increment, "voiceover_swipe_up"),
    (Gesture::Decrement, "voiceover_swipe_down"),
];

/// The four kinds of touch feedback, numbered as `ff_touch_haptic` and
/// Android's haptic call take them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Haptic {
    /// A light impact: a hold starting or ending.
    Bump = 0,
    /// A selection tick: one step.
    Tick = 1,
    /// A warning: the deep swipe.
    Warning = 2,
    /// A success: the engine key.
    Success = 3,
}

impl Haptic {
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// What one finger did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FingerKind {
    Down,
    Move,
    Up,
    /// The system took the touches away (a call, Control Center): every
    /// hold ends.
    Cancel,
}

impl FingerKind {
    /// The code `ios/ff_fingers.m` queues for this kind.
    pub fn from_code(code: i32) -> Option<FingerKind> {
        Some(match code {
            0 => FingerKind::Down,
            1 => FingerKind::Move,
            2 => FingerKind::Up,
            3 => FingerKind::Cancel,
            _ => return None,
        })
    }

    /// The name the agent server's `fingers` tool takes.
    pub fn from_name(name: &str) -> Option<FingerKind> {
        Some(match name {
            "down" => FingerKind::Down,
            "move" => FingerKind::Move,
            "up" => FingerKind::Up,
            "cancel" => FingerKind::Cancel,
            _ => return None,
        })
    }
}

/// One finger event: which finger (stable from down to up), where, in
/// points, and when, in seconds on the clock the recogniser ticks on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FingerEvent {
    pub finger: i64,
    pub kind: FingerKind,
    pub x: f32,
    pub y: f32,
    pub t: f64,
}

#[cfg(test)]
mod tests;
