//! Player-chosen controls: which key, and which pad button, does what at the
//! wheel.
//!
//! Every discrete driving control is an [`Action`]. Each action has a
//! default keyboard chord (a key plus the modifiers held with it) and, for
//! the ones the pad reaches, a default button on the plain or the
//! right-bumper layer. A player can move any of them from Settings, Gameplay,
//! Controls; the choice is saved in `Settings` as text and rebuilt into a
//! [`KeyBindings`] table on the context, which is what the driving state and
//! the spoken hints read.
//!
//! What stays fixed, on purpose: Escape for the pause menu, Enter to confirm,
//! F1 for help, F2 for the command list, the Control keys that stop the event
//! voice, the clutch on Shift and the left bumper, plus and minus for the
//! cruise target, the radio dial keys, the message-review keys, and every menu key. Those are either
//! the screen reader's own vocabulary or a control with several physical
//! keys already, and moving them would cost more than it gives. On the pad,
//! Start (pause), Back (stop the voice, then help), the two bumpers and the
//! analog triggers and sticks stay where they are for the same reasons.
//!
//! A chord matches exactly: Shift K is its own binding, separate from K. A
//! key pressed with modifiers that no binding claims falls back to the bare
//! key's action, which is how the table always behaved (Shift held for the
//! clutch never stopped W from shifting up, and Alt E still starts the
//! engine).

use std::collections::HashMap;

use ff_core::settings::Settings;

use crate::app::held_keys::HeldKeys;
use crate::controller::ControllerButton;
use crate::states::base::{Key, Mods};
use crate::touch::Gesture;

mod touch;

/// Chords, pad chords and the key and button names are Hearken's; the
/// table of driving actions, the reserved keys and the saved strings are
/// Freight Fate's.
pub use hearken::bindings::{
    key_saved_name, key_spoken_name, pad_button_short_name, parse_key_name, Chord, PadChord,
};
pub use touch::{touch_gesture_name, touch_gesture_noun, touch_slots, TouchCommand};

/// One discrete driving control a player can move to another key or button.
///
/// The order here is the order of the rows on the shortcuts screens.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Accelerate,
    Brake,
    EmergencyBrake,
    SteerLeft,
    SteerRight,
    Straighten,
    Engine,
    ParkingBrake,
    Horn,
    /// Keyboard only by default, like the lane locator: the cab controls
    /// are pure ambience and the pad has no free buttons worth spending.
    DriverWindow,
    Fan,
    Climate,
    Cruise,
    CruiseResume,
    /// Pad only: the keyboard's plus and minus keys stay fixed.
    CruiseUp,
    CruiseDown,
    TakeExit,
    Rest,
    Status,
    Speed,
    SpeedLimit,
    SafeSpeed,
    Fuel,
    Clock,
    Route,
    Weather,
    Lane,
    LaneLocator,
    Grade,
    Upcoming,
    LastAnnouncement,
    /// Keyboard only by default: copies the last spoken line to the
    /// clipboard. Handled on every screen, not just at the wheel.
    CopyLastSpoken,
    Cb,
    HosWheel,
    HosBreak,
    HosDrive,
    PlaceState,
    PlaceRoad,
    PlaceTown,
    PlaceDirection,
    EngineBrake,
    AutoJake,
    JakeStage1,
    JakeStage2,
    JakeStage3,
    /// Pad only: one button walks the stages the keyboard has three keys for.
    CycleJake,
    ShiftUp,
    ShiftDown,
    Neutral,
    Reverse,
    TransmissionMode,
    Radio,
    RadioFavorite,
    /// Pad only: the keyboard radio dial keys stay fixed.
    RadioTuneNext,
    RadioTunePrev,
    RadioVolumeUp,
    RadioVolumeDown,
    RadioCategoryNext,
    RadioCategoryPrev,
    RadioStatus,
    RadioNowPlaying,
}

/// `(action, saved id, spoken label, default keyboard chords, default pad
/// chords)`. A control with more than one default chord keeps them all
/// until the player moves it, and then has only the chord they chose.
type Row = (
    Action,
    &'static str,
    &'static str,
    &'static [Chord],
    &'static [PadChord],
);

use ControllerButton as Pad;

const TABLE: &[Row] = &[
    (
        Action::Accelerate,
        "accelerate",
        "Accelerate",
        &[Chord::plain(Key::Up)],
        &[],
    ),
    (
        Action::Brake,
        "brake",
        "Brake",
        &[Chord::plain(Key::Down)],
        &[],
    ),
    (
        Action::EmergencyBrake,
        "emergency_brake",
        "Emergency brake",
        &[Chord::plain(Key::B)],
        &[],
    ),
    (
        Action::SteerLeft,
        "steer_left",
        "Steer left",
        &[Chord::plain(Key::Left)],
        &[],
    ),
    (
        Action::SteerRight,
        "steer_right",
        "Steer right",
        &[Chord::plain(Key::Right)],
        &[],
    ),
    (
        Action::Straighten,
        "straighten",
        "Straighten up",
        &[Chord::plain(Key::Slash)],
        &[],
    ),
    (
        Action::Engine,
        "engine",
        "Engine on or off",
        &[Chord::plain(Key::E)],
        &[PadChord::modified(Pad::A)],
    ),
    (
        Action::ParkingBrake,
        "parking_brake",
        "Parking brake",
        &[Chord::plain(Key::P)],
        &[PadChord::modified(Pad::Y)],
    ),
    (
        Action::Horn,
        "horn",
        "Horn",
        &[Chord::plain(Key::H)],
        &[PadChord::plain(Pad::LeftStick)],
    ),
    (
        Action::DriverWindow,
        "driver_window",
        "Driver window down or up",
        &[Chord::plain(Key::Z)],
        &[],
    ),
    (Action::Fan, "fan", "Fan speed", &[Chord::alt(Key::Z)], &[]),
    (
        Action::Climate,
        "climate",
        "Heat or air conditioning",
        &[Chord::shift(Key::Z)],
        &[],
    ),
    (
        Action::Cruise,
        "cruise",
        "Automatic speed control",
        &[Chord::plain(Key::K)],
        &[PadChord::plain(Pad::Y)],
    ),
    (
        Action::CruiseResume,
        "cruise_resume",
        "Resume the last cruise speed",
        &[Chord::shift(Key::K)],
        &[],
    ),
    (
        Action::CruiseUp,
        "cruise_up",
        "Cruise target up",
        &[],
        &[PadChord::modified(Pad::DPadRight)],
    ),
    (
        Action::CruiseDown,
        "cruise_down",
        "Cruise target down",
        &[],
        &[PadChord::modified(Pad::DPadLeft)],
    ),
    (
        Action::TakeExit,
        "take_exit",
        "Signal for the exit or a pull-over",
        &[Chord::plain(Key::X)],
        &[PadChord::plain(Pad::DPadDown)],
    ),
    (
        Action::Rest,
        "rest",
        "Rest stop",
        &[Chord::plain(Key::T)],
        &[PadChord::modified(Pad::DPadDown)],
    ),
    (
        Action::Status,
        "status",
        "Status menu",
        &[Chord::plain(Key::Tab)],
        &[PadChord::modified(Pad::Start)],
    ),
    (
        Action::Speed,
        "speed",
        "Speed",
        &[Chord::plain(Key::Space)],
        &[PadChord::plain(Pad::B)],
    ),
    (
        Action::SpeedLimit,
        "speed_limit",
        "Posted speed limit",
        &[Chord::plain(Key::S)],
        &[PadChord::modified(Pad::X)],
    ),
    (
        Action::SafeSpeed,
        "safe_speed",
        "Safe speed",
        &[Chord::plain(Key::D)],
        &[],
    ),
    (
        Action::Fuel,
        "fuel",
        "Fuel",
        &[Chord::plain(Key::F)],
        &[PadChord::modified(Pad::B)],
    ),
    (
        Action::Clock,
        "clock",
        "Clock",
        &[Chord::plain(Key::C)],
        &[PadChord::plain(Pad::DPadRight)],
    ),
    (
        Action::Route,
        "route",
        "Route and location",
        &[Chord::plain(Key::R)],
        &[
            PadChord::plain(Pad::DPadUp),
            PadChord::modified(Pad::DPadUp),
        ],
    ),
    (
        Action::Weather,
        "weather",
        "Weather",
        &[Chord::plain(Key::V)],
        &[PadChord::plain(Pad::DPadLeft)],
    ),
    (
        Action::Lane,
        "lane",
        "Lane position",
        &[Chord::plain(Key::L)],
        &[],
    ),
    (
        Action::LaneLocator,
        "lane_locator",
        "Lane locator",
        &[Chord::plain(Key::I)],
        &[],
    ),
    (
        Action::Grade,
        "grade",
        "Grade",
        &[Chord::plain(Key::G)],
        &[],
    ),
    (
        Action::Upcoming,
        "upcoming",
        "Road ahead",
        &[Chord::plain(Key::U)],
        &[],
    ),
    (
        Action::LastAnnouncement,
        "last_announcement",
        "Repeat the last announcement",
        &[Chord::plain(Key::A)],
        &[],
    ),
    (
        Action::CopyLastSpoken,
        "copy_last_spoken",
        "Copy the last spoken text",
        &[Chord {
            key: Key::C,
            mods: Mods {
                shift: true,
                ctrl: true,
                alt: false,
            },
        }],
        &[],
    ),
    (
        Action::Cb,
        "cb",
        "Repeat the last CB chatter",
        &[Chord::alt(Key::C)],
        &[],
    ),
    (
        Action::HosWheel,
        "hos_wheel",
        "Time at the wheel",
        &[Chord::alt(Key::A)],
        &[],
    ),
    (
        Action::HosBreak,
        "hos_break",
        "When the break is due",
        &[Chord::alt(Key::S)],
        &[],
    ),
    (
        Action::HosDrive,
        "hos_drive",
        "What ends this shift",
        &[Chord::alt(Key::D)],
        &[],
    ),
    (
        Action::PlaceState,
        "place_state",
        "The state",
        &[Chord::alt(Key::Num1), Chord::alt(Key::Kp1)],
        &[],
    ),
    (
        Action::PlaceRoad,
        "place_road",
        "The road",
        &[Chord::alt(Key::Num2), Chord::alt(Key::Kp2)],
        &[],
    ),
    (
        Action::PlaceTown,
        "place_town",
        "The town",
        &[Chord::alt(Key::Num3), Chord::alt(Key::Kp3)],
        &[],
    ),
    (
        Action::PlaceDirection,
        "place_direction",
        "The direction",
        &[Chord::alt(Key::Num4), Chord::alt(Key::Kp4)],
        &[],
    ),
    (
        Action::EngineBrake,
        "engine_brake",
        "Engine brake",
        &[Chord::plain(Key::J)],
        &[PadChord::plain(Pad::RightStick)],
    ),
    (
        Action::AutoJake,
        "auto_jake",
        "Automatic engine brake on or off",
        &[Chord::alt(Key::J)],
        &[],
    ),
    (
        Action::JakeStage1,
        "jake_stage_1",
        "Engine brake stage 1",
        &[Chord::plain(Key::Num1)],
        &[],
    ),
    (
        Action::JakeStage2,
        "jake_stage_2",
        "Engine brake stage 2",
        &[Chord::plain(Key::Num2)],
        &[],
    ),
    (
        Action::JakeStage3,
        "jake_stage_3",
        "Engine brake stage 3",
        &[Chord::plain(Key::Num3)],
        &[],
    ),
    (
        Action::CycleJake,
        "cycle_jake",
        "Next engine brake stage",
        &[],
        &[PadChord::modified(Pad::RightStick)],
    ),
    (
        Action::ShiftUp,
        "shift_up",
        "Shift up",
        &[Chord::plain(Key::W)],
        &[PadChord::plain(Pad::A)],
    ),
    (
        Action::ShiftDown,
        "shift_down",
        "Shift down",
        &[Chord::plain(Key::Q)],
        &[PadChord::plain(Pad::X)],
    ),
    (
        Action::Neutral,
        "neutral",
        "Neutral",
        &[Chord::plain(Key::N)],
        &[],
    ),
    (
        Action::Reverse,
        "reverse",
        "Reverse",
        &[Chord::plain(Key::Backspace)],
        &[],
    ),
    (
        Action::TransmissionMode,
        "transmission_mode",
        "Automatic or manual shifting",
        &[Chord::alt(Key::T)],
        &[],
    ),
    (
        Action::Radio,
        "radio",
        "Radio on or off",
        &[Chord::plain(Key::M)],
        &[PadChord::modified(Pad::RightStickUp)],
    ),
    (
        Action::RadioFavorite,
        "radio_favorite",
        "Save the station as a favorite",
        &[Chord::plain(Key::O)],
        &[PadChord::modified(Pad::RightStickDown)],
    ),
    (
        Action::RadioTuneNext,
        "radio_tune_next",
        "Tune to the next radio station",
        &[],
        &[PadChord::plain(Pad::RightStickRight)],
    ),
    (
        Action::RadioTunePrev,
        "radio_tune_prev",
        "Tune to the previous radio station",
        &[],
        &[PadChord::plain(Pad::RightStickLeft)],
    ),
    (
        Action::RadioVolumeUp,
        "radio_volume_up",
        "Raise radio volume",
        &[],
        &[PadChord::plain(Pad::RightStickUp)],
    ),
    (
        Action::RadioVolumeDown,
        "radio_volume_down",
        "Lower radio volume",
        &[],
        &[PadChord::plain(Pad::RightStickDown)],
    ),
    (
        Action::RadioCategoryNext,
        "radio_category_next",
        "Next radio category",
        &[],
        &[PadChord::modified(Pad::RightStickRight)],
    ),
    (
        Action::RadioCategoryPrev,
        "radio_category_prev",
        "Previous radio category",
        &[],
        &[PadChord::modified(Pad::RightStickLeft)],
    ),
    (
        Action::RadioStatus,
        "radio_status",
        "Radio status",
        &[Chord::plain(Key::Y)],
        &[PadChord::modified(Pad::LeftStick)],
    ),
    (
        Action::RadioNowPlaying,
        "radio_now_playing",
        "What the radio is playing",
        &[Chord::shift(Key::Y)],
        &[PadChord::modified(Pad::Back)],
    ),
];

/// [`Action::all`] as a slice, for Hearken's [`hearken::bindings::Action`].
static ALL: [Action; TABLE.len()] = {
    let mut out = [Action::Accelerate; TABLE.len()];
    let mut i = 0;
    while i < TABLE.len() {
        out[i] = TABLE[i].0;
        i += 1;
    }
    out
};

fn row(action: Action) -> &'static Row {
    TABLE
        .iter()
        .find(|(a, ..)| *a == action)
        .expect("every Action has a row in TABLE")
}

impl Action {
    /// Every action, in screen order.
    pub fn all() -> impl Iterator<Item = Action> {
        ALL.iter().copied()
    }

    /// The id the settings file stores.
    pub fn id(self) -> &'static str {
        row(self).1
    }

    pub fn from_id(id: &str) -> Option<Action> {
        TABLE.iter().find(|(_, i, ..)| *i == id).map(|(a, ..)| *a)
    }

    /// The row label: what the control does.
    pub fn label(self) -> &'static str {
        row(self).2
    }

    pub fn default_chords(self) -> &'static [Chord] {
        row(self).3
    }

    pub fn default_pad_chords(self) -> &'static [PadChord] {
        row(self).4
    }

    /// Whether the keyboard has this control at all.
    pub fn on_keyboard(self) -> bool {
        !self.default_chords().is_empty()
    }

    /// Whether the pad has this control at all.
    pub fn on_pad(self) -> bool {
        !self.default_pad_chords().is_empty()
    }

    /// The controls that act only while held: the pedals, steering, the
    /// emergency brake and the horn. A menu row or a tap cannot run them.
    pub fn held(self) -> bool {
        matches!(
            self,
            Action::Accelerate
                | Action::Brake
                | Action::EmergencyBrake
                | Action::SteerLeft
                | Action::SteerRight
                | Action::Straighten
                | Action::Horn
        )
    }
}

/// Hearken's binding table runs on these. Every reserved key is Freight
/// Fate's own, worded its own way, so the engine's list is off: modified
/// Start and Back stay free for the second layer, as they always were.
impl hearken::bindings::Action for Action {
    fn all() -> &'static [Self] {
        &ALL
    }
    fn id(self) -> &'static str {
        row(self).1
    }
    fn label(self) -> &'static str {
        row(self).2
    }
    fn default_keys(self) -> &'static [Chord] {
        row(self).3
    }
    fn default_pad(self) -> &'static [PadChord] {
        row(self).4
    }
    fn reserved_key(chord: &Chord) -> Option<&'static str> {
        reserved_key_reason(chord)
    }
    fn reserved_pad(chord: &PadChord) -> Option<&'static str> {
        reserved_pad_reason(chord)
    }
    fn keeps_engine_reserved() -> bool {
        false
    }
}

/// Why a key cannot be chosen, as the screen says it.
pub fn reserved_key_reason(chord: &Chord) -> Option<&'static str> {
    let bare = chord.mods == Mods::NONE;
    let fixed = match chord.key {
        Key::Escape => Some("Escape is the pause menu"),
        Key::Return | Key::KpEnter => Some("Enter confirms"),
        Key::F1 => Some("F1 is help"),
        Key::F2 => Some("F2 lists the driving commands"),
        Key::LCtrl | Key::RCtrl | Key::LShift | Key::RShift | Key::LAlt | Key::RAlt => {
            Some("a modifier key on its own cannot be a shortcut")
        }
        Key::Equals | Key::Plus | Key::KpPlus | Key::Minus | Key::KpMinus => {
            Some("plus and minus set the cruise target")
        }
        Key::PageUp | Key::PageDown | Key::Semicolon | Key::Quote => {
            Some("the radio dial keys are fixed")
        }
        Key::Comma | Key::Period | Key::LeftBracket | Key::RightBracket => {
            Some("the message review keys are fixed")
        }
        Key::Other(_) => Some("that key cannot be used"),
        _ => None,
    };
    if let Some(reason) = fixed {
        // Escape, F1, F2 and the modifier keys are fixed however they are
        // pressed (the first three are answered before the table is asked);
        // the rest are only claimed bare, so Alt with a review key is free.
        let always = matches!(
            chord.key,
            Key::Escape
                | Key::F1
                | Key::F2
                | Key::LCtrl
                | Key::RCtrl
                | Key::LShift
                | Key::RShift
                | Key::LAlt
                | Key::RAlt
                | Key::Other(_)
        );
        if bare || always {
            return Some(reason);
        }
    }
    if chord.key == Key::C && chord.mods == Mods::CTRL {
        return Some("Control C copies the message in review");
    }
    None
}

/// Why a pad chord cannot be chosen, as the screen says it.
pub fn reserved_pad_reason(chord: &PadChord) -> Option<&'static str> {
    match chord.button {
        ControllerButton::LeftShoulder => Some("the left bumper is the clutch"),
        ControllerButton::RightShoulder => Some("the right bumper is the second layer"),
        ControllerButton::Guide => Some("the guide button belongs to the system"),
        ControllerButton::Start if !chord.modified => Some("Start is pause"),
        ControllerButton::Back if !chord.modified => {
            Some("Back stops the driving voice and reads help")
        }
        _ => None,
    }
}

/// The result of asking for a new binding: `Done`, `Unchanged` (the chord
/// already belongs to this action), `Reserved(reason)` or `Taken(other)`.
pub type Rebind = hearken::bindings::Rebind<Action>;

/// The live table: defaults plus what the player moved. The keyboard and
/// pad tables are Hearken's; the touch table and the `"id=chord;..."`
/// strings in `Settings` are Freight Fate's.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct KeyBindings {
    inner: hearken::bindings::KeyBindings<Action>,
    touch: HashMap<Gesture, TouchCommand>,
}

impl KeyBindings {
    /// The table `settings` describes; unreadable entries are ignored, so
    /// one bad line never costs the rest.
    pub fn from_settings(settings: &Settings) -> KeyBindings {
        let keys = entries(&settings.key_bindings)
            .filter_map(|(id, value)| Some((Action::from_id(id)?, Chord::parse(value)?)));
        let pad = entries(&settings.pad_bindings)
            .filter_map(|(id, value)| Some((Action::from_id(id)?, PadChord::parse(value)?)));
        KeyBindings {
            // Keeps a move only for a control the device has, as before.
            inner: hearken::bindings::KeyBindings::from_parts(keys, pad),
            touch: touch::parse_touch(&settings.touch_bindings),
        }
    }

    /// Write the moved controls back into `settings`.
    pub fn store(&self, settings: &mut Settings) {
        let keys: Vec<String> = self
            .inner
            .moved_keys()
            .filter_map(|(action, chord)| Some(format!("{}={}", action.id(), chord.saved()?)))
            .collect();
        let pad: Vec<String> = self
            .inner
            .moved_pad()
            .map(|(action, chord)| format!("{}={}", action.id(), chord.saved()))
            .collect();
        settings.key_bindings = keys.join(";");
        settings.pad_bindings = pad.join(";");
        settings.touch_bindings = touch::saved_touch(&self.touch);
    }

    /// True when nothing has been moved from its default.
    pub fn is_default(&self) -> bool {
        self.inner.is_default() && self.touch.is_empty()
    }

    pub fn reset(&mut self) {
        self.inner.reset();
        self.touch.clear();
    }

    pub fn reset_keys(&mut self) {
        self.inner.reset_keys();
    }

    pub fn reset_pad(&mut self) {
        self.inner.reset_pad();
    }

    // -- keyboard ----------------------------------------------------------------

    /// The chords that trigger `action` today.
    ///
    /// A default the player already moved another control onto stays with
    /// that control: a default added in an update (Straighten on slash) must
    /// never take over a key the player bound before it existed.
    pub fn chords(&self, action: Action) -> Vec<Chord> {
        self.inner.chords(action)
    }

    /// The control's name for a spoken prompt, first chord only.
    pub fn spoken(&self, action: Action) -> String {
        self.inner.spoken(action)
    }

    /// The action a key press means: the exact chord if one is bound, else
    /// the bare key's action.
    pub fn action_for(&self, key: Key, mods: Mods) -> Option<Action> {
        self.inner.action_for(key, mods)
    }

    /// Whether `action`'s key is down right now, modifiers included.
    pub fn pressed(&self, input: &HeldKeys, action: Action) -> bool {
        self.inner.pressed(input, action)
    }

    /// Move `action` to `chord`.
    pub fn set_chord(&mut self, action: Action, chord: Chord) -> Rebind {
        self.inner.set_chord(action, chord)
    }

    // -- pad ---------------------------------------------------------------------

    /// Same rule as [`Self::chords`]: a player's binding outranks a default.
    pub fn pad_chords(&self, action: Action) -> Vec<PadChord> {
        self.inner.pad_chords(action)
    }

    pub fn pad_spoken(&self, action: Action) -> String {
        self.inner.pad_spoken(action)
    }

    pub fn pad_action_for(&self, button: ControllerButton, modified: bool) -> Option<Action> {
        self.inner.pad_action_for(button, modified)
    }

    pub fn set_pad_chord(&mut self, action: Action, chord: PadChord) -> Rebind {
        self.inner.set_pad_chord(action, chord)
    }

    // -- spoken hints -------------------------------------------------------------

    /// The keyboard phrase for a `control_hint` action id, when the hint
    /// names a control the player can move; `None` leaves the fixed table's
    /// wording alone.
    pub fn hint_key_phrase(&self, hint: &str) -> Option<String> {
        Some(match hint {
            "gears" => format!(
                "{} and {}",
                self.spoken(Action::ShiftUp),
                self.spoken(Action::ShiftDown)
            ),
            "gear_first" => self.spoken(Action::ShiftUp),
            other => self.spoken(hint_action(other)?),
        })
    }

    /// The pad phrase for a `control_hint` action id, same contract. The
    /// two-button phrases keep the table's tighter wording ("the A and X
    /// buttons") until one of the pair moves.
    pub fn hint_pad_phrase(&self, hint: &str) -> Option<String> {
        let pair = |a: Action, b: Action, joiner: &str| {
            if self.inner.pad_override(a).is_some() || self.inner.pad_override(b).is_some() {
                Some(format!(
                    "{} {joiner} {}",
                    self.pad_spoken(a),
                    self.pad_spoken(b)
                ))
            } else {
                None
            }
        };
        let action = match hint {
            "gear_first" => Action::ShiftUp,
            "cruise_adjust" => return pair(Action::CruiseDown, Action::CruiseUp, "or"),
            "gears" => return pair(Action::ShiftUp, Action::ShiftDown, "and"),
            other => hint_action(other)?,
        };
        if !action.on_pad() {
            return None;
        }
        Some(self.pad_spoken(action))
    }
}

/// The `input_hints` action ids that name a movable control.
fn hint_action(hint: &str) -> Option<Action> {
    Some(match hint {
        "accelerate" => Action::Accelerate,
        "brake" => Action::Brake,
        "emergency_brake" => Action::EmergencyBrake,
        "reverse" => Action::Reverse,
        "neutral" => Action::Neutral,
        "engine" => Action::Engine,
        "parking_brake" => Action::ParkingBrake,
        "take_exit" => Action::TakeExit,
        "rest" => Action::Rest,
        "cruise_set" => Action::Cruise,
        "speed" => Action::Speed,
        "status_menu" => Action::Status,
        "fuel" => Action::Fuel,
        "clock" => Action::Clock,
        "route" => Action::Route,
        "weather" => Action::Weather,
        "lane" => Action::Lane,
        "horn" => Action::Horn,
        "engine_brake" => Action::EngineBrake,
        _ => return None,
    })
}

/// `id=value` pairs from a saved bindings string.
fn entries(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.split(';')
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.split_once('='))
}

#[cfg(test)]
mod tests;
