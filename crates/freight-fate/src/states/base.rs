//! State machine foundation and the accessible menu base (port of
//! `freight_fate/states/base.py`).
//!
//! Every screen is a [`State`] on the app's state stack. [`Menu`] provides
//! fully speech-driven list navigation: arrow keys with wrap-around,
//! Home/End, first-letter jumping, Enter to activate, Escape to go back, and
//! F1 to repeat contextual help. Each state also exposes `lines()` -- visible
//! text mirroring the speech output for low-vision players and sighted
//! helpers.
//!
//! # Shape of the port
//!
//! Python states held `self.ctx`; here every hook takes `ctx: &mut
//! GameContext`. `MenuState` subclasses become a struct embedding a
//! [`MenuCore`] and implementing [`Menu`] (the `build_items` hook plus any
//! overrides); `impl_state_for_menu!` writes the forwarding [`State`] impl.
//! Duck-typed optional members (`captures_text_input`,
//! `wants_controller_repeat`, `tick_covered_music`, `apply_radio_settings_now`,
//! `radio`) are trait methods with defaults.
//!
//! Input arrives as [`InputEvent`]: the pygame event fused with its
//! `unicode` (`KeyDown { text }`), the controller events as the manager
//! sees them, and the two window events the loop cares about. The SDL
//! keycode <-> [`Key`] conversion lives in `app::sdl_shell`.

use std::any::Any;

use ff_core::message_log::{Message, MessageLog};
use ff_core::radio::RadioState;

use crate::app::GameContext;
pub use crate::app::Say;
use crate::controller::{ControllerAxis, ControllerButton};
use crate::discord_presence::PresenceState;
use crate::touch::Gesture;

mod menu;

pub use menu::{
    DynamicLabel, Label, Menu, MenuAction, MenuCore, MenuItem, SimpleMenuState, DEFAULT_INTRO_HELP,
};

// -- spoken helpers ---------------------------------------------------------------

/// A spoken fragment ending in exactly one sentence mark, never two.
///
/// Menu/list labels are sometimes plain ("Sleep 10 hours") and sometimes
/// whole sentences that already end in a period (settlement summary lines).
/// Appending "." unconditionally produces ".." which a screen reader voices as
/// "dot dot", so add the period only when one is not already there.
pub fn end_sentence(text: &str) -> String {
    let text = text.trim_end();
    if text.ends_with(['.', '!', '?', ':']) {
        text.to_string()
    } else {
        format!("{text}.")
    }
}

/// A single typed character the way an accessible text field announces it.
///
/// Shared by every text-entry field in the game (currently just driver name
/// entry) so a character reviewed one at a time always sounds the same way.
/// A space reads as "space" rather than silence. A capital letter gets a
/// leading "cap" -- speech synthesis pronounces "J" and "j" identically, so
/// without the marker a player arrowing through a typed name has no way to
/// hear that the first letter capitalized the way they meant it to.
/// Punctuation is named: most voices say nothing for a bare colon or slash,
/// so typing a stream address character by character was a run of silent
/// clicks (owner, 2026-10-09: "I can't type a colon ... I hear a click").
pub fn spoken_char(ch: char) -> String {
    let name = match ch {
        ' ' => "space",
        ':' => "colon",
        ';' => "semicolon",
        '/' => "slash",
        '\\' => "backslash",
        '.' => "dot",
        ',' => "comma",
        '-' => "dash",
        '_' => "underscore",
        '?' => "question mark",
        '!' => "exclamation mark",
        '=' => "equals",
        '&' => "and",
        '%' => "percent",
        '#' => "number sign",
        '@' => "at",
        '~' => "tilde",
        '+' => "plus",
        '*' => "star",
        '\'' => "apostrophe",
        '"' => "quote",
        '(' => "left paren",
        ')' => "right paren",
        _ => "",
    };
    if !name.is_empty() {
        return name.to_string();
    }
    if ch.is_alphabetic() && ch.is_uppercase() {
        return format!("cap {}", ch.to_lowercase());
    }
    ch.to_string()
}

// -- input events ------------------------------------------------------------------

/// The pygame `K_*` constants the game reads, as one enum. `Other` carries
/// any SDL keycode outside that set so text entry still sees its `text`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Key {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Num0,
    Num1,
    Num2,
    Num3,
    Num4,
    Num5,
    Num6,
    Num7,
    Num8,
    Num9,
    Kp0,
    Kp1,
    Kp2,
    Kp3,
    Kp4,
    Kp5,
    Kp6,
    Kp7,
    Kp8,
    Kp9,
    KpEnter,
    KpPlus,
    KpMinus,
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    Return,
    Escape,
    Space,
    Tab,
    Backspace,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    Insert,
    Delete,
    Comma,
    Period,
    Slash,
    Backslash,
    Backquote,
    Equals,
    Plus,
    Minus,
    Semicolon,
    Quote,
    LeftBracket,
    RightBracket,
    LCtrl,
    RCtrl,
    LShift,
    RShift,
    LAlt,
    RAlt,
    /// An SDL keycode the game has no binding for.
    Other(i32),
}

impl Key {
    /// The key a typed character arrives on (`ord(ch.lower())` in the Python
    /// tests): letters and digits map to their keys, anything else to the
    /// few punctuation keys the game binds, else `Other(codepoint)`.
    pub fn from_char(ch: char) -> Key {
        match ch.to_ascii_lowercase() {
            'a' => Key::A,
            'b' => Key::B,
            'c' => Key::C,
            'd' => Key::D,
            'e' => Key::E,
            'f' => Key::F,
            'g' => Key::G,
            'h' => Key::H,
            'i' => Key::I,
            'j' => Key::J,
            'k' => Key::K,
            'l' => Key::L,
            'm' => Key::M,
            'n' => Key::N,
            'o' => Key::O,
            'p' => Key::P,
            'q' => Key::Q,
            'r' => Key::R,
            's' => Key::S,
            't' => Key::T,
            'u' => Key::U,
            'v' => Key::V,
            'w' => Key::W,
            'x' => Key::X,
            'y' => Key::Y,
            'z' => Key::Z,
            '0' => Key::Num0,
            '1' => Key::Num1,
            '2' => Key::Num2,
            '3' => Key::Num3,
            '4' => Key::Num4,
            '5' => Key::Num5,
            '6' => Key::Num6,
            '7' => Key::Num7,
            '8' => Key::Num8,
            '9' => Key::Num9,
            ' ' => Key::Space,
            ',' => Key::Comma,
            '.' => Key::Period,
            '=' => Key::Equals,
            '+' => Key::Plus,
            '-' => Key::Minus,
            ';' => Key::Semicolon,
            '\'' => Key::Quote,
            '[' => Key::LeftBracket,
            ']' => Key::RightBracket,
            other => Key::Other(other as i32),
        }
    }
}

/// Modifier state on a key event (`event.mod` masked with `KMOD_*`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Mods {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

impl Mods {
    pub const NONE: Mods = Mods {
        shift: false,
        ctrl: false,
        alt: false,
    };
    pub const SHIFT: Mods = Mods {
        shift: true,
        ctrl: false,
        alt: false,
    };
    pub const CTRL: Mods = Mods {
        shift: false,
        ctrl: true,
        alt: false,
    };
    pub const ALT: Mods = Mods {
        shift: false,
        ctrl: false,
        alt: true,
    };
}

/// One event the app loop hands to a state or the controller manager.
#[derive(Clone, Debug, PartialEq)]
pub enum InputEvent {
    /// `KEYDOWN` with pygame's fused `unicode` (`None` when it was empty).
    KeyDown {
        key: Key,
        mods: Mods,
        text: Option<char>,
        /// True when the operating system auto-repeated a held key.
        repeat: bool,
    },
    KeyUp {
        key: Key,
        mods: Mods,
    },
    ControllerButtonDown {
        button: ControllerButton,
        instance_id: u32,
    },
    ControllerButtonUp {
        button: ControllerButton,
        instance_id: u32,
    },
    ControllerAxis {
        axis: ControllerAxis,
        value: i16,
        instance_id: u32,
    },
    ControllerAdded {
        device_index: u32,
    },
    ControllerRemoved {
        instance_id: u32,
    },
    WindowFocusGained,
    /// SDL releases every key when the window loses focus; the app drops
    /// what it thinks is held with them, so alt-tabbing away mid-drive can
    /// never leave a pedal down.
    WindowFocusLost,
    Quit,
    /// A touch gesture on iPhone or iPad. The state takes it with
    /// [`State::handle_gesture`], or the app presses the key it stands for.
    Gesture(Gesture),
}

impl InputEvent {
    /// `pygame.event.Event(KEYDOWN, key=key, unicode="")`.
    pub fn key(key: Key) -> InputEvent {
        InputEvent::KeyDown {
            key,
            mods: Mods::NONE,
            text: None,
            repeat: false,
        }
    }

    /// A key press carrying its typed character.
    pub fn key_text(key: Key, text: char) -> InputEvent {
        InputEvent::KeyDown {
            key,
            mods: Mods::NONE,
            text: Some(text),
            repeat: false,
        }
    }

    /// A key press with modifiers held.
    pub fn key_mods(key: Key, mods: Mods) -> InputEvent {
        InputEvent::KeyDown {
            key,
            mods,
            text: None,
            repeat: false,
        }
    }

    pub fn key_up(key: Key) -> InputEvent {
        InputEvent::KeyUp {
            key,
            mods: Mods::NONE,
        }
    }

    /// A typed character on the key it lives on.
    pub fn typed(ch: char) -> InputEvent {
        InputEvent::key_text(Key::from_char(ch), ch)
    }

    /// `CONTROLLERBUTTONDOWN` from instance 0.
    pub fn button(button: ControllerButton) -> InputEvent {
        InputEvent::ControllerButtonDown {
            button,
            instance_id: 0,
        }
    }

    pub fn button_up(button: ControllerButton) -> InputEvent {
        InputEvent::ControllerButtonUp {
            button,
            instance_id: 0,
        }
    }

    pub fn axis(axis: ControllerAxis, value: i16) -> InputEvent {
        InputEvent::ControllerAxis {
            axis,
            value,
            instance_id: 0,
        }
    }

    /// Whether this is one of the `CONTROLLER*` events the manager owns.
    pub fn is_controller_event(&self) -> bool {
        matches!(
            self,
            InputEvent::ControllerButtonDown { .. }
                | InputEvent::ControllerButtonUp { .. }
                | InputEvent::ControllerAxis { .. }
                | InputEvent::ControllerAdded { .. }
                | InputEvent::ControllerRemoved { .. }
        )
    }

    /// The key of a `KeyDown`, else `None`.
    pub fn key_down(&self) -> Option<(Key, Mods, Option<char>)> {
        match self {
            InputEvent::KeyDown {
                key, mods, text, ..
            } => Some((*key, *mods, *text)),
            _ => None,
        }
    }

    /// Whether this is an OS auto-repeat `KeyDown` (false for everything else).
    pub fn key_repeat(&self) -> bool {
        matches!(self, InputEvent::KeyDown { repeat: true, .. })
    }
}

// -- the State contract -------------------------------------------------------------

/// Downcasting support every state gets for free (`app.state()` as a
/// concrete screen in tests and the harness).
pub trait AsAny {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<T: 'static> AsAny for T {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// Base contract for all game screens.
pub trait State: AsAny {
    /// While this state is on top of the stack, main-channel speech
    /// (`ctx.say`) queues instead of interrupting. False for menus and
    /// readers -- cancelling on navigation is how screen readers behave --
    /// and True for the driving state, where an achievement or assist
    /// notice landing with interrupt=True would stamp on whatever the
    /// player was being told (speech priority research, R2). A menu pushed
    /// over a drive is the top state, so it keeps immediate speech.
    fn paces_main_speech(&self) -> bool {
        false
    }

    /// Called when the state becomes active (pushed or revealed).
    fn enter(&mut self, _ctx: &mut GameContext) {}

    /// Called when the state is removed from the stack.
    fn exit(&mut self, _ctx: &mut GameContext) {}

    fn handle_event(&mut self, _ctx: &mut GameContext, _event: &InputEvent) {}

    /// A touch gesture this screen answers itself. `false` (every screen but
    /// the drive) has the app press the gesture's key instead.
    fn handle_gesture(&mut self, _ctx: &mut GameContext, _gesture: Gesture) -> bool {
        false
    }

    /// Controller buttons a plain keyboard-driven state understands,
    /// translated into the key events it already handles. This keeps simple
    /// screens (update prompts, name entry, the help reader) usable from a
    /// controller without bespoke code. Menus and the driving state
    /// override this entirely.
    fn handle_controller(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let InputEvent::ControllerButtonDown { button, .. } = event else {
            return;
        };
        if let Some(key) = controller_key_map(*button) {
            self.handle_event(ctx, &InputEvent::key(key));
        }
    }

    /// Called when the active controller is unplugged. The driving state
    /// pauses; menus keep going on the keyboard.
    fn on_controller_disconnect(&mut self, _ctx: &mut GameContext) {}

    /// Per-frame update. Overrides MUST call `ctx.update_music_rotation(dt)`
    /// (the Python `super().update(dt)`).
    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
    }

    /// Visible text for the window, mirroring what speech says.
    fn lines(&self, _ctx: &GameContext) -> Vec<String> {
        Vec::new()
    }

    /// Broad, privacy-safe activity for Discord Rich Presence, or None to
    /// leave presence unchanged. The app polls the active state each frame
    /// and hands the result to the presence service; states only report
    /// status here and never touch Discord itself.
    fn presence(&self, _ctx: &GameContext) -> Option<PresenceState> {
        None
    }

    /// The on-duty snapshot for the live drivers board, or None.
    ///
    /// Only the active hauling states (driving, pulled over, resting,
    /// delivering, and the pause menu over any of them, which reports
    /// `PAUSED_ACTIVITY`) report themselves; everything else returns None so
    /// the player drops off the public board when they are not on a job.
    fn online_presence(&self, _ctx: &GameContext) -> Option<PresenceState> {
        None
    }

    /// `captures_text_input`: a screen that takes typed text declines the
    /// global message-review keys, so a comma reaches the field.
    fn captures_text_input(&self) -> bool {
        false
    }

    /// `wants_controller_repeat`: whether held D-pad left/right auto-repeat
    /// events are forwarded to `handle_controller` (menus say yes).
    fn wants_controller_repeat(&self) -> bool {
        false
    }

    /// Whether this state keeps music turning over while covered by a menu
    /// (the Python `hasattr(state, "tick_covered_music")`).
    fn ticks_covered_music(&self) -> bool {
        false
    }

    /// Advance a covered drive's own playlist (see `ticks_covered_music`).
    fn tick_covered_music(&mut self, _ctx: &mut GameContext, _dt: f64) {}

    /// Whether this state can apply a radio settings change right now (the
    /// Python `hasattr(state, "apply_radio_settings_now")`).
    fn applies_radio_settings(&self) -> bool {
        false
    }

    /// Apply a radio settings change to a covered drive, right now.
    fn apply_radio_settings_now(&mut self, _ctx: &mut GameContext) {}

    /// The state's radio, when it carries one (`getattr(state, "radio")`).
    fn radio(&self) -> Option<&RadioState> {
        None
    }

    /// Walk the message log. True when the key belonged to review.
    ///
    /// The app offers every key event here before the active state sees it,
    /// so the review controls work on every screen without a state opting
    /// in. Screens that take typed text are the exception: a driver name may
    /// well contain a comma, and punctuation has to reach the field.
    fn handle_message_review(&mut self, ctx: &mut GameContext, event: &InputEvent) -> bool {
        if self.captures_text_input() {
            return false;
        }
        handle_message_review(ctx, event)
    }
}

/// `State._controller_key_map`: A->Return, B->Escape, Back->F1, D-pad->arrows.
pub fn controller_key_map(button: ControllerButton) -> Option<Key> {
    match button {
        ControllerButton::A => Some(Key::Return),
        ControllerButton::B => Some(Key::Escape),
        ControllerButton::Back => Some(Key::F1),
        ControllerButton::DPadUp => Some(Key::Up),
        ControllerButton::DPadDown => Some(Key::Down),
        ControllerButton::DPadLeft => Some(Key::Left),
        ControllerButton::DPadRight => Some(Key::Right),
        _ => None,
    }
}

/// The global review keys: `[`/`]` cycle the category filter, `,`/`.` step
/// through the log, `Ctrl+,`/`Ctrl+.` jump to the ends, `Ctrl+C` copies the
/// message in review. True when the key was one of them.
pub fn handle_message_review(ctx: &mut GameContext, event: &InputEvent) -> bool {
    let Some((key, mods, _)) = event.key_down() else {
        return false;
    };
    let ctrl = mods.ctrl;
    match key {
        Key::LeftBracket => {
            if let Some(category) = ctx.message_log.previous_category() {
                let notice = hidden_notice(&mut ctx.message_log);
                ctx.say_with(
                    format!("{category} messages.{notice}"),
                    Say::new().review(false),
                );
            }
            true
        }
        Key::RightBracket => {
            if let Some(category) = ctx.message_log.next_category() {
                let notice = hidden_notice(&mut ctx.message_log);
                ctx.say_with(
                    format!("{category} messages.{notice}"),
                    Say::new().review(false),
                );
            }
            true
        }
        Key::Comma if ctrl => {
            let message = ctx.message_log.first_message().cloned();
            speak_review_message(ctx, message.as_ref());
            true
        }
        Key::Period if ctrl => {
            let message = ctx.message_log.last_message().cloned();
            speak_review_message(ctx, message.as_ref());
            true
        }
        Key::Comma => {
            let message = ctx.message_log.previous_message().cloned();
            speak_review_message(ctx, message.as_ref());
            true
        }
        Key::Period => {
            let message = ctx.message_log.next_message().cloned();
            match message {
                None => {
                    // Already on the newest one this filter shows. Saying
                    // nothing is what let a whole delivery settlement sit
                    // invisible behind a filter left on Event, so the end of
                    // the list is exactly where the count has to be spoken.
                    let notice = hidden_notice(&mut ctx.message_log);
                    if !notice.is_empty() {
                        ctx.say_with(notice.trim().to_string(), Say::new().review(false));
                    }
                }
                Some(message) => speak_review_message(ctx, Some(&message)),
            }
            true
        }
        Key::C if ctrl => {
            let Some(message) = ctx.message_log.message_in_review().cloned() else {
                return true;
            };
            if ctx.write_clipboard_text(&message.text) {
                ctx.say_with("Message copied to clipboard.", Say::new().review(false));
            }
            true
        }
        _ => false,
    }
}

/// How many newer messages the chosen category is holding back.
///
/// Empty when nothing is filtered out, so the common case stays silent. The
/// filter is the driver's stated preference and now survives a lapse in
/// review (Tim S, 2026-08-21) -- which is safe only while the review keeps
/// saying what the preference costs.
pub fn hidden_notice(log: &mut MessageLog) -> String {
    let hidden = log.hidden_newer_count();
    if hidden == 0 {
        return String::new();
    }
    let plural = if hidden == 1 { "message" } else { "messages" };
    format!(" {hidden} newer {plural} outside this filter.")
}

/// Speak a reviewed message, with the hidden-count notice on the newest one.
pub fn speak_review_message(ctx: &mut GameContext, message: Option<&Message>) {
    let Some(message) = message else {
        return;
    };
    // Reviewing is a deliberate act: silence the event voice first, or a
    // hazard call still playing talks over the line being reviewed.
    ctx.stop_event_speech();
    let visible_len = ctx.message_log.filtered_messages().len();
    // Only on the newest one the filter shows: appending the count to every
    // step back through the history would read it a dozen times.
    let at_newest = visible_len > 0 && ctx.message_log.index >= visible_len as isize - 1;
    let notice = if at_newest {
        hidden_notice(&mut ctx.message_log)
    } else {
        String::new()
    };
    ctx.say_with(
        format!("{}{notice}", message.text),
        Say::new().review(false),
    );
}

// -- TimedMessageState ---------------------------------------------------------------

/// What a [`TimedMessageState`] does when its countdown ends.
pub type OnComplete = Box<dyn FnOnce(&mut GameContext)>;

/// A brief, spoken transition that ignores stray held navigation keys.
pub struct TimedMessageState {
    pub title: String,
    pub message: String,
    pub status: String,
    pub seconds: f64,
    pub remaining: f64,
    pub complete_text: String,
    pub sound_key: Option<String>,
    on_complete: Option<OnComplete>,
    complete: bool,
}

impl TimedMessageState {
    pub fn new(
        title: &str,
        message: &str,
        status: &str,
        seconds: f64,
        on_complete: impl FnOnce(&mut GameContext) + 'static,
    ) -> Self {
        let seconds = seconds.max(0.0);
        Self {
            title: title.to_string(),
            message: message.to_string(),
            status: status.to_string(),
            seconds,
            remaining: seconds,
            complete_text: String::new(),
            sound_key: Some("ui/notify".to_string()),
            on_complete: Some(Box::new(on_complete)),
            complete: false,
        }
    }

    pub fn complete_text(mut self, text: &str) -> Self {
        self.complete_text = text.to_string();
        self
    }

    /// `sound_key=...`; `None` plays nothing on entry.
    pub fn sound_key(mut self, key: Option<&str>) -> Self {
        self.sound_key = key.map(str::to_string);
        self
    }

    pub fn is_complete(&self) -> bool {
        self.complete
    }
}

impl State for TimedMessageState {
    fn enter(&mut self, ctx: &mut GameContext) {
        if let Some(key) = &self.sound_key {
            ctx.audio.play(key);
        }
        ctx.say(&self.message);
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let Some((key, _, _)) = event.key_down() else {
            return;
        };
        if matches!(
            key,
            Key::F1 | Key::Escape | Key::Return | Key::Space | Key::KpEnter
        ) {
            ctx.say(&self.status);
        }
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        if self.complete {
            return;
        }
        self.remaining = (self.remaining - dt).max(0.0);
        if self.remaining > 0.0 {
            return;
        }
        self.complete = true;
        if !self.complete_text.is_empty() {
            ctx.say_with(self.complete_text.clone(), Say::queued());
        }
        if let Some(on_complete) = self.on_complete.take() {
            on_complete(ctx);
        }
    }

    fn lines(&self, _ctx: &GameContext) -> Vec<String> {
        vec![
            self.title.clone(),
            String::new(),
            self.status.clone(),
            format!("Ready in {:.1} seconds.", self.remaining),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_sentence_adds_one_mark_only() {
        assert_eq!(end_sentence("Sleep 10 hours"), "Sleep 10 hours.");
        assert_eq!(end_sentence("Done. "), "Done.");
        assert_eq!(end_sentence("Ready?"), "Ready?");
        assert_eq!(end_sentence("Note:"), "Note:");
    }

    #[test]
    fn spoken_char_marks_space_and_capitals() {
        assert_eq!(spoken_char(' '), "space");
        assert_eq!(spoken_char('J'), "cap j");
        assert_eq!(spoken_char('j'), "j");
        assert_eq!(spoken_char('7'), "7");
        assert_eq!(spoken_char(','), "comma");
        assert_eq!(spoken_char(':'), "colon");
        assert_eq!(spoken_char('/'), "slash");
    }

    #[test]
    fn key_from_char_follows_ord_lower() {
        assert_eq!(Key::from_char('J'), Key::J);
        assert_eq!(Key::from_char(','), Key::Comma);
        assert_eq!(Key::from_char(' '), Key::Space);
        assert_eq!(Key::from_char('é'), Key::Other('é' as i32));
    }
}
