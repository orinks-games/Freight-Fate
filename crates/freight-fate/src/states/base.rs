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
use crate::bindings::Action;
use crate::controller::{ControllerAxis, ControllerButton};
use crate::discord_presence::PresenceState;
use crate::touch::Gesture;

mod menu;

pub use menu::{
    DynamicLabel, Label, Menu, MenuAction, MenuCore, MenuItem, SimpleMenuState, DEFAULT_INTRO_HELP,
};

// -- spoken helpers ---------------------------------------------------------------

/// `end_sentence` keeps a spoken fragment to one sentence mark; `spoken_char`
/// names a typed character the way an accessible text field does (space,
/// capitals and punctuation). Both are Hearken's.
pub use hearken::text::{end_sentence, spoken_char};

// -- input events ------------------------------------------------------------------

/// The keys the game reads and the modifier state on a key event, from
/// Hearken. `Key::from_char` maps a typed character to its key.
pub use hearken::input::{Key, Mods};

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
        let focus = self.copy_focus(ctx);
        handle_message_review_with_focus(ctx, event, focus)
    }

    /// The focused row (no position cue) and its index, for the copy key:
    /// until something new is logged after the screen opened or focus last
    /// moved, the row is copied. None on screens without rows.
    fn copy_focus(&self, _ctx: &GameContext) -> Option<CopyFocus> {
        None
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
    handle_message_review_with_focus(ctx, event, None)
}

/// The row that has focus, and where it sits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyFocus {
    pub index: usize,
    pub row: String,
}

/// `handle_message_review`, knowing the active screen's focused row.
pub fn handle_message_review_with_focus(
    ctx: &mut GameContext,
    event: &InputEvent,
    focus: Option<CopyFocus>,
) -> bool {
    let Some((key, mods, _)) = event.key_down() else {
        return false;
    };
    let ctrl = mods.ctrl;
    if ctx.bindings.action_for(key, mods) == Some(Action::CopyLastSpoken) {
        copy_last_spoken_with_focus(ctx, focus);
        return true;
    }
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

/// The text the copy key puts on the clipboard: the newest line in the
/// message log, whatever the review filter shows. The log leaves out routine
/// menu movement and the review and clipboard confirmations, so after a
/// search this is the result line, and on a new screen it is the heading.
pub fn newest_copyable_line(ctx: &GameContext) -> Option<String> {
    ctx.message_log
        .messages
        .iter()
        .rev()
        .map(|message| strip_position_cue(&message.text))
        .find(|text| !text.is_empty())
}

/// Drop a trailing menu position cue ("Achievements. 2 of 8." becomes
/// "Achievements."). Only a cue standing as its own sentence is removed, so
/// "Results 1 to 25 of 52." is kept whole.
pub fn strip_position_cue(text: &str) -> String {
    let trimmed = text.trim_end();
    let body = trimmed.strip_suffix('.').unwrap_or(trimmed);
    if let Some((head, cue)) = body.rsplit_once(". ") {
        let mut parts = cue.split(" of ");
        if let (Some(n), Some(m), None) = (parts.next(), parts.next(), parts.next()) {
            let digits = |s: &str| !s.is_empty() && s.chars().all(|c| c.is_ascii_digit());
            if digits(n) && digits(m) {
                return format!("{head}.");
            }
        }
    }
    trimmed.to_string()
}

/// Put the newest message-log line on the clipboard and say how it went.
pub fn copy_last_spoken(ctx: &mut GameContext) {
    copy_last_spoken_with_focus(ctx, None);
}

/// As `copy_last_spoken`, but when nothing has been logged since the
/// screen opened or focus last moved (`GameContext::note_copy_focus`), the
/// focused row is copied instead, without its position cue.
pub fn copy_last_spoken_with_focus(ctx: &mut GameContext, focus: Option<CopyFocus>) {
    let newest = newest_copyable_line(ctx);
    let quiet = ctx.message_log.total_added() == ctx.copy_mark;
    let text = match focus {
        Some(focus) if quiet || newest.is_none() => Some(focus.row),
        _ => newest,
    };
    copy_text(ctx, text);
}

/// Put `text` on the clipboard and say how it went. The replies are not
/// logged, so a second press copies the same line again rather than
/// "Copied.".
pub fn copy_text(ctx: &mut GameContext, text: Option<String>) {
    let Some(text) = text else {
        ctx.say_with("Nothing to copy yet.", Say::new().review(false));
        return;
    };
    let reply = if ctx.write_clipboard_text(&text) {
        "Copied."
    } else {
        "Could not copy to the clipboard."
    };
    ctx.say_with(reply, Say::new().review(false));
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
