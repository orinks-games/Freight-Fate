//! Key and button practice: the keyboard and controller counterpart of
//! gesture practice. Every key or button names what it does at the wheel,
//! and nothing reaches the game. Shift F1 opens it from any screen.

use std::time::{Duration, Instant};

use crate::app::GameContext;
use crate::bindings::{pad_button_short_name, Action, Chord, PadChord};
use crate::controller::ControllerButton;
use crate::states::base::{InputEvent, Key, Mods, State};

/// How far a trigger or the left stick moves before practice names it, and
/// how near rest it must come back before it is named again.
const ANALOG_PRESS: f64 = 0.5;
const ANALOG_REST: f64 = 0.2;
/// A held right stick steps the radio repeatedly; it is named once per push.
const STICK_REPEAT: Duration = Duration::from_millis(1500);
/// The second Escape, or Start, must come this soon after the first.
const LEAVE_WINDOW: Duration = Duration::from_secs(2);
/// A pad can report the Start that leaves practice twice, 16 ms apart; the
/// echo must not pause the drive underneath.
const START_BOUNCE: Duration = Duration::from_millis(150);

/// The global key that opens practice.
pub fn opens_input_practice(key: Key, mods: Mods) -> bool {
    key == Key::F1 && mods == Mods::SHIFT
}

/// The global pad chord that opens practice: Start while Back is held. Both
/// are fixed buttons, so the chord means the same on every screen.
pub fn opens_input_practice_pad(ctx: &GameContext, button: ControllerButton) -> bool {
    button == ControllerButton::Start && ctx.controller.is_down(ControllerButton::Back)
}

#[derive(Default)]
pub struct InputPracticeState {
    last_leave: Option<Instant>,
    last_stick: Option<(ControllerButton, Instant)>,
    throttle_named: bool,
    brake_named: bool,
    steer_named: bool,
}

impl InputPracticeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Escape or Start: the first names it, a second within two seconds
    /// leaves.
    fn leave_press(&mut self, ctx: &mut GameContext, name: &str) {
        let now = Instant::now();
        if self
            .last_leave
            .is_some_and(|then| now.duration_since(then) <= LEAVE_WINDOW)
        {
            ctx.say("Leaving key and button practice.");
            if name == "Start" {
                ctx.controller
                    .swallow_press(ControllerButton::Start, START_BOUNCE);
            }
            ctx.pop_state();
        } else {
            self.last_leave = Some(now);
            ctx.say(&format!("{name}: pause menu. Again to leave practice."));
        }
    }

    fn key_down(&mut self, ctx: &mut GameContext, key: Key, mods: Mods) {
        if key == Key::Escape {
            self.leave_press(ctx, "Escape");
            return;
        }
        self.last_leave = None;
        let text = describe_key(ctx, key, mods);
        ctx.say(&text);
    }

    fn button_down(&mut self, ctx: &mut GameContext, button: ControllerButton) {
        let modified = ctx.controller.modifier && button != ControllerButton::RightShoulder;
        if opens_input_practice_pad(ctx, button) {
            self.last_leave = None;
            ctx.say("Back plus Start: key and button practice.");
            return;
        }
        if button == ControllerButton::Start && !modified {
            self.leave_press(ctx, "Start");
            return;
        }
        self.last_leave = None;
        if is_stick_direction(button) {
            let now = Instant::now();
            if self
                .last_stick
                .is_some_and(|(b, then)| b == button && now.duration_since(then) < STICK_REPEAT)
            {
                self.last_stick = Some((button, now));
                return;
            }
            self.last_stick = Some((button, now));
        }
        if let Some(text) = describe_button(ctx, button, modified) {
            ctx.say(&text);
        }
    }

    /// The triggers and the left stick have no button events; name each
    /// once per push.
    fn poll_analog(&mut self, ctx: &mut GameContext) {
        let throttle = ctx.controller.throttle_target();
        let brake = ctx.controller.brake_target();
        let steer = ctx.controller.steering().abs();
        if edge(&mut self.throttle_named, throttle) {
            self.last_leave = None;
            ctx.say("Right trigger: Accelerate, as far as you press it.");
        }
        if edge(&mut self.brake_named, brake) {
            self.last_leave = None;
            ctx.say(
                "Left trigger: Brake, as far as you press it. All the way down: Emergency brake.",
            );
        }
        if edge(&mut self.steer_named, steer) {
            self.last_leave = None;
            let text = if ctx.settings.lane_is_automated() {
                "Left stick: push left or right to change one lane."
            } else {
                "Left stick: steer."
            };
            ctx.say(text);
        }
    }
}

/// True once when `value` passes the press point; rearms near rest.
fn edge(named: &mut bool, value: f64) -> bool {
    if value < ANALOG_REST {
        *named = false;
    } else if value >= ANALOG_PRESS && !*named {
        *named = true;
        return true;
    }
    false
}

fn is_stick_direction(button: ControllerButton) -> bool {
    matches!(
        button,
        ControllerButton::RightStickUp
            | ControllerButton::RightStickDown
            | ControllerButton::RightStickLeft
            | ControllerButton::RightStickRight
    )
}

fn capitalized(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// What the action does, with the lane-keeping case the arrows change.
fn action_text(ctx: &GameContext, action: Action) -> String {
    match action {
        Action::SteerLeft if ctx.settings.lane_is_automated() => "Change one lane left".to_string(),
        Action::SteerRight if ctx.settings.lane_is_automated() => {
            "Change one lane right".to_string()
        }
        Action::ShiftUp | Action::ShiftDown | Action::Neutral | Action::Reverse
            if ctx.settings.automatic_transmission =>
        {
            format!("{}, in a manual truck", action.label())
        }
        other => other.label().to_string(),
    }
}

/// The answer for one key press, in the order the drive reads keys: message
/// review first, then the fixed keys, then the player's own table.
pub fn describe_key(ctx: &GameContext, key: Key, mods: Mods) -> String {
    let name = capitalized(&Chord { key, mods }.spoken());
    let fixed = match key {
        Key::LCtrl | Key::RCtrl => return "Control: stop the driving voice.".to_string(),
        Key::LShift | Key::RShift => {
            return "Shift: hold as the clutch in a manual truck.".to_string()
        }
        Key::LAlt | Key::RAlt => return "Alt: hold with another key.".to_string(),
        Key::F1 if opens_input_practice(key, mods) => Some("key and button practice"),
        Key::F1 => Some("help"),
        Key::F2 => Some("driving commands"),
        Key::Comma if mods.ctrl => Some("oldest message"),
        Key::Period if mods.ctrl => Some("newest message"),
        Key::Comma => Some("previous message"),
        Key::Period => Some("next message"),
        Key::LeftBracket => Some("previous message category"),
        Key::RightBracket => Some("next message category"),
        Key::C if mods == Mods::CTRL => Some("copy the message in review"),
        _ => None,
    };
    if let Some(text) = fixed {
        return format!("{name}: {text}.");
    }
    if let Some(action) = ctx.bindings.action_for(key, mods) {
        return format!("{name}: {}.", action_text(ctx, action));
    }
    let plus = matches!(key, Key::Equals | Key::Plus | Key::KpPlus);
    let minus = matches!(key, Key::Minus | Key::KpMinus);
    let text = match key {
        _ if plus && mods.ctrl => "cruise target up by 1",
        _ if plus => "cruise target up to the next 5",
        _ if minus && mods.ctrl => "cruise target down by 1",
        _ if minus => "cruise target down to the next 5",
        Key::Return | Key::KpEnter => "choose, or confirm an arrival",
        Key::PageUp | Key::Semicolon if mods.ctrl => "previous radio category",
        Key::PageUp | Key::Semicolon if mods.shift => "raise radio volume",
        Key::PageUp | Key::Semicolon => "tune to the previous radio station",
        Key::PageDown | Key::Quote if mods.ctrl => "next radio category",
        Key::PageDown | Key::Quote if mods.shift => "lower radio volume",
        Key::PageDown | Key::Quote => "tune to the next radio station",
        _ => "no command",
    };
    format!("{name}: {text}.")
}

/// The answer for one button press; `None` for the guide button, which the
/// system keeps.
pub fn describe_button(
    ctx: &GameContext,
    button: ControllerButton,
    modified: bool,
) -> Option<String> {
    let chord = PadChord { button, modified };
    let name = if modified {
        capitalized(&chord.spoken())
    } else {
        capitalized(pad_button_short_name(button))
    };
    let fixed = match button {
        ControllerButton::Guide => return None,
        ControllerButton::RightShoulder => Some("hold for the second layer of buttons"),
        ControllerButton::LeftShoulder => Some("hold as the clutch in a manual truck"),
        ControllerButton::Back if !modified => {
            Some("help, or stop the driving voice while it speaks")
        }
        ControllerButton::Start if modified => Some(Action::Status.label()),
        _ => None,
    };
    if let Some(text) = fixed {
        return Some(format!("{name}: {text}."));
    }
    let mut text = match ctx.bindings.pad_action_for(button, modified) {
        Some(action) => action_text(ctx, action),
        None => "no command".to_string(),
    };
    // The drive gives A to a ready arrival before its own command.
    if button == ControllerButton::A && !modified {
        text = if text == "no command" {
            "confirm an arrival when one is ready".to_string()
        } else {
            format!("{text}, or confirm an arrival when one is ready")
        };
    }
    Some(format!("{name}: {text}."))
}

impl State for InputPracticeState {
    fn enter(&mut self, ctx: &mut GameContext) {
        ctx.audio.horn_stop();
        ctx.say(
            "Key and button practice. Press a key or button to hear what it does while \
             driving. Escape twice, or Start twice, to leave.",
        );
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        if let InputEvent::KeyDown {
            key,
            mods,
            repeat: false,
            ..
        } = event
        {
            self.key_down(ctx, *key, *mods);
        }
    }

    fn handle_controller(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        if let InputEvent::ControllerButtonDown { button, .. } = event {
            self.button_down(ctx, *button);
        }
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        self.poll_analog(ctx);
    }

    /// Every key is practiced here, the review keys included.
    fn handle_message_review(&mut self, _ctx: &mut GameContext, _event: &InputEvent) -> bool {
        false
    }
}
