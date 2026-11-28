//! An accessible text field: characters echo as you type, and a review
//! cursor lets you check what you typed without retyping it (port of
//! `freight_fate/states/text_entry.py`).
//!
//! Extracted from the new-career name entry so the Radio app's station
//! search could share it; anything that asks a blind player to type a short
//! string should ride this rather than grow its own key handling.
//!
//! Shape: [`TextEntryCore`] holds the field (`heading`, `field_label`,
//! `name`, `cursor`); the [`TextEntry`] trait carries the behaviour as
//! provided methods over an embedded core, with `confirm` as the one abstract
//! hook (`_confirm` in Python); `impl_state_for_text_entry!` writes the
//! forwarding [`State`] impl. [`TextEntryState`] is the ready-made concrete
//! field whose confirm is a closure.

use crate::app::{GameContext, Say};
use crate::states::base::{spoken_char, InputEvent, Key, State};
#[cfg(any(target_os = "ios", test))]
use std::time::{Duration, Instant};

/// The longest string a field takes.
pub const MAX_LEN: usize = 24;

/// A field's controls in gestures, for a touch player with no hardware
/// keyboard to name keys on. The on-screen keyboard rises with the field.
pub const TOUCH_FIELD_HELP: &str = "Type with the on-screen keyboard, then double tap. \
    Flick left or right to review the letters, flick up or down with three fingers to jump \
    to the start or end, and flick down with two fingers to cancel.";

/// Whether a field prompt should name gestures rather than keys.
pub fn touch_prompt(ctx: &GameContext) -> bool {
    ctx.controller.device() == ff_core::input_hints::TOUCH
}

pub(crate) fn touch_field_help(ctx: &GameContext) -> Option<&'static str> {
    if !touch_prompt(ctx) {
        return None;
    }
    #[cfg(target_os = "ios")]
    if crate::app::sdl_shell::voice_over_running() {
        return Some(voice_over_field_help(
            crate::app::sdl_shell::software_keyboard_shown(),
            ctx.controller.hardware_keyboard(),
        ));
    }
    Some(TOUCH_FIELD_HELP)
}

#[cfg(any(target_os = "ios", test))]
fn voice_over_field_help(software_keyboard: bool, hardware_keyboard: bool) -> &'static str {
    if software_keyboard {
        return "Type with the on-screen keyboard. In the text field, choose Characters in the \
                VoiceOver rotor, then flick up or down to move the cursor. Use Delete to remove \
                the character before the cursor, Return to confirm, or a two-finger scrub to cancel.";
    }
    if hardware_keyboard {
        return "Type with your keyboard. Use the left and right arrow keys to review letters, \
                Return to confirm, or Escape to cancel.";
    }
    "In the text field, choose Characters in the VoiceOver rotor, then flick up or down to move the \
     cursor. Use Return to confirm, or a two-finger scrub to cancel."
}

#[cfg(any(target_os = "ios", test))]
#[derive(Default)]
struct VoiceOverFieldPrompt {
    deadline: Option<Instant>,
}

#[cfg(any(target_os = "ios", test))]
impl VoiceOverFieldPrompt {
    const WAIT: Duration = Duration::from_secs(1);

    fn defer(&mut self, voice_over: bool, software_keyboard_shown: bool, now: Instant) -> bool {
        self.deadline = (voice_over && !software_keyboard_shown).then_some(now + Self::WAIT);
        self.deadline.is_some()
    }

    fn take_ready(
        &mut self,
        voice_over: bool,
        software_keyboard_shown: bool,
        now: Instant,
    ) -> bool {
        if self
            .deadline
            .is_some_and(|deadline| !voice_over || software_keyboard_shown || now >= deadline)
        {
            self.deadline = None;
            return true;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::{
        voice_over_field_help, Instant, State, TextEntryCore, TextEntryState, VoiceOverFieldPrompt,
    };

    #[test]
    fn voice_over_waits_for_the_keyboard_then_announces_once() {
        let now = Instant::now();
        let mut prompt = VoiceOverFieldPrompt::default();
        assert!(prompt.defer(true, false, now));
        assert!(!prompt.take_ready(true, false, now));
        assert!(prompt.take_ready(true, true, now));
        assert!(!prompt.take_ready(true, true, now));
    }

    #[test]
    fn voice_over_does_not_wait_indefinitely_if_the_keyboard_stays_hidden() {
        let now = Instant::now();
        let mut prompt = VoiceOverFieldPrompt::default();
        assert!(prompt.defer(true, false, now));
        assert!(prompt.take_ready(true, false, now + VoiceOverFieldPrompt::WAIT));
        assert!(!prompt.take_ready(true, true, now + VoiceOverFieldPrompt::WAIT));
    }

    #[test]
    fn voice_over_off_or_keyboard_already_shown_does_not_defer() {
        let now = Instant::now();
        let mut prompt = VoiceOverFieldPrompt::default();
        assert!(!prompt.defer(false, false, now));
        assert!(!prompt.defer(true, true, now));
        assert!(!prompt.take_ready(true, true, now));
    }

    #[test]
    fn voice_over_help_follows_the_keyboard_in_use() {
        assert!(voice_over_field_help(false, true).contains("left and right arrow keys"));
        assert!(voice_over_field_help(true, true).contains("Characters in the VoiceOver rotor"));
        assert!(voice_over_field_help(true, true).contains("Delete"));
        assert_eq!(
            voice_over_field_help(true, false),
            voice_over_field_help(true, true)
        );
        assert!(voice_over_field_help(false, false).contains("In the text field"));
        assert!(voice_over_field_help(false, false).contains("two-finger scrub to cancel"));
    }

    #[test]
    fn native_field_names_follow_the_current_field_label() {
        let mut field = TextEntryState::new("Heading", "", |_, _| {});
        assert_eq!(field.ios_field().map(|(label, _, _)| label), Some("Text"));
        field.entry.field_label = "Station name".to_string();
        assert_eq!(
            field.ios_field().map(|(label, _, _)| label),
            Some("Station name")
        );
        field.entry.field_label = "Stream address".to_string();
        assert_eq!(
            field.ios_field().map(|(label, _, _)| label),
            Some("Stream address")
        );
    }

    #[test]
    fn native_field_syncs_utf16_cursor_and_mid_text_edits() {
        let mut field = TextEntryState::new("Heading", "Field", |_, _| {});
        let field: &mut dyn State = &mut field;
        fn text_and_cursor(field: &dyn State) -> Option<(&[char], usize)> {
            field.ios_field().map(|(_, text, cursor)| (text, cursor))
        }
        assert_eq!(text_and_cursor(field), Some(([].as_slice(), 0)));
        field.apply_ios_field_edit("A🚚bc", 4);
        assert_eq!(
            text_and_cursor(field),
            Some((['A', '🚚', 'b', 'c'].as_slice(), 3))
        );
        field.apply_ios_field_edit("A🚚bc", 3);
        assert_eq!(
            text_and_cursor(field),
            Some((['A', '🚚', 'b', 'c'].as_slice(), 2))
        );
        field.apply_ios_field_edit("Abc", 1);
        assert_eq!(
            text_and_cursor(field),
            Some((['A', 'b', 'c'].as_slice(), 1))
        );
        field.apply_ios_field_edit("Aëbc", 2);
        assert_eq!(
            text_and_cursor(field),
            Some((['A', 'ë', 'b', 'c'].as_slice(), 2))
        );
    }

    #[test]
    fn native_field_rejects_invalid_text_without_losing_the_suffix() {
        let mut entry = TextEntryCore::new("Heading", "Field");
        entry.max_len = 3;
        entry.apply_ios_edit("abc", 1);
        entry.apply_ios_edit("axbc", 2);
        entry.apply_ios_edit("a\nb", 2);
        assert_eq!((entry.text().as_str(), entry.cursor), ("abc", 1));
    }

    #[test]
    fn native_field_truncates_paste_at_the_cursor_and_keeps_the_suffix() {
        let mut entry = TextEntryCore::new("Heading", "Field");
        entry.max_len = 5;
        entry.set_text("Abc");
        entry.apply_ios_edit("A🚚xybc", 5);
        assert_eq!((entry.text().as_str(), entry.cursor), ("A🚚xbc", 3));
        entry.apply_ios_edit("A🚚xxyzbc", 7);
        assert_eq!((entry.text().as_str(), entry.cursor), ("A🚚xbc", 3));

        entry.set_text("Abccd");
        entry.apply_ios_edit("AbZZZZcd", 6);
        assert_eq!((entry.text().as_str(), entry.cursor), ("AbZcd", 3));
    }
}

/// The field itself: what the Python class kept on `self`.
pub struct TextEntryCore {
    pub heading: String,
    pub field_label: String,
    /// The typed text (kept as chars so the cursor indexes code points the
    /// way a Python `str` does).
    pub name: Vec<char>,
    /// A gap position from 0 to `name.len()`, the way an OS text field's
    /// caret works.
    pub cursor: usize,
    pub max_len: usize,
    #[cfg(target_os = "ios")]
    voice_over_prompt: VoiceOverFieldPrompt,
}

impl TextEntryCore {
    pub fn new(heading: &str, field_label: &str) -> Self {
        Self {
            heading: heading.to_string(),
            field_label: field_label.to_string(),
            name: Vec::new(),
            cursor: 0,
            max_len: MAX_LEN,
            #[cfg(target_os = "ios")]
            voice_over_prompt: VoiceOverFieldPrompt::default(),
        }
    }

    /// The typed text as a string.
    pub fn text(&self) -> String {
        self.name.iter().collect()
    }

    /// Replace the typed text (cursor to the end).
    pub fn set_text(&mut self, text: &str) {
        self.name = text.chars().collect();
        self.cursor = self.name.len();
    }

    #[cfg(any(target_os = "ios", test))]
    pub(crate) fn apply_ios_edit(&mut self, text: &str, cursor_utf16: usize) {
        let mut name = Vec::with_capacity(self.max_len);
        let mut offset = 0;
        let mut cursor = 0;
        for ch in text.chars() {
            if !is_printable(ch) {
                return;
            }
            offset += ch.len_utf16();
            name.push(ch);
            if offset <= cursor_utf16 {
                cursor += 1;
            }
        }
        if name.len() > self.max_len {
            let suffix_len = name[cursor..]
                .iter()
                .rev()
                .zip(self.name.iter().rev())
                .take_while(|(new, old)| new == old)
                .count();
            let Some(keep_before_suffix) = self.max_len.checked_sub(suffix_len) else {
                return;
            };
            let suffix_start = name.len() - suffix_len;
            name.drain(keep_before_suffix..suffix_start);
            cursor = cursor.min(keep_before_suffix);
        }
        self.name = name;
        self.cursor = cursor;
    }
}

impl Default for TextEntryCore {
    fn default() -> Self {
        Self::new("Text entry", "Text")
    }
}

/// Accessible text entry: characters are echoed as you type.
///
/// A review cursor tracks where in the typed text the player currently is.
/// Left and Right step the cursor one character and speak whatever character
/// sits between the old and new position -- the standard way a screen reader
/// lets a player check what they typed without retyping it. Home and End
/// jump to either end and speak the character now there. Typing and
/// Backspace act at the cursor, not just at the end, so a player who arrows
/// back to fix a letter edits in place instead of being surprised that
/// Backspace deletes from the end regardless of where they were reviewing.
///
/// Implementors set `heading` and `field_label` on the core for the visual
/// lines, speak their own prompt in `enter` (the default speaks the shared
/// one), and act on Enter in `confirm`.
pub trait TextEntry: Sized + 'static {
    fn entry(&self) -> &TextEntryCore;
    fn entry_mut(&mut self) -> &mut TextEntryCore;
    /// Enter was pressed.
    fn confirm(&mut self, ctx: &mut GameContext);

    // -- State hooks, forwarded by impl_state_for_text_entry! ----------------------

    /// Keep typed commas; the global repeat key yields.
    fn captures_text_input(&self) -> bool {
        true
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        let heading = self.entry().heading.clone();
        if let Some(help) = touch_field_help(ctx) {
            ctx.say(&format!("{heading}. {help}"));
            return;
        }
        ctx.say(&format!(
            "{heading}. Type, then press Enter. \
             Left and right arrows review the letters you have typed, \
             Home and End jump to the start or end. Press Escape to cancel."
        ));
    }

    fn exit(&mut self, _ctx: &mut GameContext) {}

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
    }

    /// True while the screen is busy with what Enter started (a request on
    /// its way): keys do nothing until it answers.
    fn input_paused(&self) -> bool {
        false
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        if self.input_paused() {
            return;
        }
        let Some((key, mods, text)) = event.key_down() else {
            return;
        };
        match key {
            Key::V if mods.ctrl => self.paste(ctx),
            Key::Escape => {
                ctx.audio.play("ui/menu_back");
                ctx.pop_state();
            }
            Key::Return | Key::KpEnter => self.confirm(ctx),
            // Help, a two-finger flick up on a phone: the field's opening
            // line again, which says how it works.
            Key::F1 => {
                #[cfg(target_os = "ios")]
                {
                    self.entry_mut().voice_over_prompt = VoiceOverFieldPrompt::default();
                }
                self.enter(ctx);
            }
            Key::Backspace => self.backspace(ctx),
            Key::Left => self.move_cursor(ctx, -1),
            Key::Right => self.move_cursor(ctx, 1),
            Key::Home => self.jump_cursor(ctx, 0, "Start"),
            Key::End => {
                let end = self.entry().name.len();
                self.jump_cursor(ctx, end, "End");
            }
            Key::F2 => {
                let field = self.entry();
                let text = if field.name.is_empty() {
                    "Empty.".to_string()
                } else {
                    field.text()
                };
                ctx.say_with(text, Say::new().review(false));
            }
            _ => {
                if let Some(ch) = text {
                    let field = self.entry();
                    if is_printable(ch) && field.name.len() < field.max_len {
                        self.insert(ctx, ch);
                    }
                }
            }
        }
    }

    fn lines(&self, ctx: &GameContext) -> Vec<String> {
        let field = self.entry();
        let before: String = field.name[..field.cursor].iter().collect();
        let after: String = field.name[field.cursor..].iter().collect();
        let help = touch_field_help(ctx).unwrap_or(
            "Left and right arrows review letters, Home and End jump to the \
             ends. Enter to confirm, Escape to cancel, F2 to hear the whole text, \
             Control V to paste.",
        );
        vec![
            field.heading.clone(),
            String::new(),
            format!("{}: {before}|{after}", field.field_label),
            help.to_string(),
        ]
    }

    // -- field editing --------------------------------------------------------------

    fn insert(&mut self, ctx: &mut GameContext, ch: char) {
        let field = self.entry_mut();
        field.name.insert(field.cursor, ch);
        field.cursor += 1;
        ctx.audio.play("ui/tick");
        ctx.say_with(spoken_char(ch), Say::new().review(false));
    }

    /// Control V: the clipboard's first non-empty line, at the cursor, cut
    /// to what the field has room for. The pasted text is read back, since
    /// nobody can check a long address they did not type any other way.
    fn paste(&mut self, ctx: &mut GameContext) {
        let text = ctx.clipboard.get_text().unwrap_or_default();
        let line: Vec<char> = text
            .lines()
            .map(str::trim)
            .find(|l| !l.is_empty())
            .unwrap_or("")
            .chars()
            .filter(|&c| is_printable(c))
            .collect();
        let field = self.entry_mut();
        let room = field.max_len.saturating_sub(field.name.len());
        if line.is_empty() || room == 0 {
            ctx.audio.play("ui/error");
            return;
        }
        let pasted: Vec<char> = line.into_iter().take(room).collect();
        let at = field.cursor;
        field.name.splice(at..at, pasted.iter().copied());
        field.cursor += pasted.len();
        ctx.audio.play("ui/tick");
        let pasted: String = pasted.into_iter().collect();
        ctx.say_with(format!("Pasted {pasted}"), Say::new().review(false));
    }

    fn backspace(&mut self, ctx: &mut GameContext) {
        let field = self.entry_mut();
        if field.cursor == 0 {
            ctx.audio.play("ui/error");
            return;
        }
        let removed = field.name.remove(field.cursor - 1);
        field.cursor -= 1;
        let rest = if field.name.is_empty() {
            "Empty.".to_string()
        } else {
            field.text()
        };
        ctx.say_with(
            format!("Deleted {}. {rest}", spoken_char(removed)),
            Say::new().review(false),
        );
    }

    /// Step the review cursor one character left or right.
    ///
    /// Both directions speak the character that sat between the old and new
    /// cursor position -- the character just moved over -- matching how a
    /// screen reader announces caret movement in a plain text field.
    fn move_cursor(&mut self, ctx: &mut GameContext, delta: i64) {
        let field = self.entry_mut();
        let new_cursor = field.cursor as i64 + delta;
        if new_cursor < 0 || new_cursor > field.name.len() as i64 {
            ctx.audio.play("ui/error");
            return;
        }
        let new_cursor = new_cursor as usize;
        let spoken_index = field.cursor.min(new_cursor);
        field.cursor = new_cursor;
        let ch = field.name[spoken_index];
        ctx.say_with(spoken_char(ch), Say::new().review(false));
    }

    fn jump_cursor(&mut self, ctx: &mut GameContext, target: usize, label: &str) {
        let field = self.entry_mut();
        if field.cursor == target {
            ctx.audio.play("ui/error");
            return;
        }
        field.cursor = target;
        if field.name.is_empty() {
            ctx.say_with(format!("{label}. Empty."), Say::new().review(false));
            return;
        }
        let edge = if target == 0 {
            field.name[0]
        } else {
            field.name[field.name.len() - 1]
        };
        ctx.say_with(
            format!("{label}. {}", spoken_char(edge)),
            Say::new().review(false),
        );
    }
}

/// `str.isprintable()` for one character, near enough: control characters
/// and the non-space separators are not typed into a name.
pub fn is_printable(ch: char) -> bool {
    if ch == ' ' {
        return true;
    }
    !(ch.is_control() || ch.is_whitespace())
}

#[cfg(target_os = "ios")]
pub(crate) fn defer_voice_over_prompt(field: &mut impl TextEntry) -> bool {
    use crate::app::sdl_shell::{software_keyboard_shown, voice_over_running};
    field.entry_mut().voice_over_prompt.defer(
        voice_over_running(),
        software_keyboard_shown(),
        Instant::now(),
    )
}

#[cfg(target_os = "ios")]
pub(crate) fn announce_voice_over_prompt(field: &mut impl TextEntry, ctx: &mut GameContext) {
    use crate::app::sdl_shell::{software_keyboard_shown, voice_over_running};
    if field.entry().voice_over_prompt.deadline.is_none() {
        return;
    }
    let keyboard_shown = software_keyboard_shown();
    if field.entry_mut().voice_over_prompt.take_ready(
        voice_over_running(),
        keyboard_shown,
        Instant::now(),
    ) {
        log::debug!("VoiceOver field prompt ready (software keyboard shown: {keyboard_shown})");
        field.enter(ctx);
    }
}

/// Write the [`State`] impl for a [`TextEntry`] screen, forwarding the
/// hooks to the `TextEntry` methods of the same name.
#[macro_export]
macro_rules! impl_state_for_text_entry {
    ($ty:ty) => {
        impl $crate::states::base::State for $ty {
            fn enter(&mut self, ctx: &mut $crate::app::GameContext) {
                #[cfg(target_os = "ios")]
                if $crate::states::text_entry::defer_voice_over_prompt(self) {
                    return;
                }
                <Self as $crate::states::text_entry::TextEntry>::enter(self, ctx)
            }
            fn on_app_return(&mut self, ctx: &mut $crate::app::GameContext) {
                // The field's own prompt, as it says on opening.
                <Self as $crate::states::base::State>::enter(self, ctx)
            }
            fn exit(&mut self, ctx: &mut $crate::app::GameContext) {
                <Self as $crate::states::text_entry::TextEntry>::exit(self, ctx)
            }
            fn update(&mut self, ctx: &mut $crate::app::GameContext, dt: f64) {
                #[cfg(target_os = "ios")]
                $crate::states::text_entry::announce_voice_over_prompt(self, ctx);
                <Self as $crate::states::text_entry::TextEntry>::update(self, ctx, dt)
            }
            fn handle_event(
                &mut self,
                ctx: &mut $crate::app::GameContext,
                event: &$crate::states::base::InputEvent,
            ) {
                <Self as $crate::states::text_entry::TextEntry>::handle_event(self, ctx, event)
            }
            fn lines(&self, ctx: &$crate::app::GameContext) -> Vec<String> {
                <Self as $crate::states::text_entry::TextEntry>::lines(self, ctx)
            }
            fn captures_text_input(&self) -> bool {
                <Self as $crate::states::text_entry::TextEntry>::captures_text_input(self)
            }
            #[cfg(any(target_os = "ios", test))]
            fn ios_field(&self) -> Option<(&str, &[char], usize)> {
                let entry = <Self as $crate::states::text_entry::TextEntry>::entry(self);
                let label = if entry.field_label.is_empty() {
                    "Text"
                } else {
                    &entry.field_label
                };
                Some((label, &entry.name, entry.cursor))
            }
            #[cfg(any(target_os = "ios", test))]
            fn apply_ios_field_edit(&mut self, text: &str, cursor_utf16: usize) {
                use $crate::states::text_entry::TextEntry;
                if !TextEntry::input_paused(self) {
                    TextEntry::entry_mut(self).apply_ios_edit(text, cursor_utf16);
                }
            }
            fn touch_mode(&self) -> $crate::touch::TouchMode {
                $crate::touch::TouchMode::Field
            }
        }
    };
}

/// The ready-made field: a heading, a label, and a confirm closure that
/// receives the typed text.
pub struct TextEntryState {
    pub entry: TextEntryCore,
    on_confirm: OnConfirm,
}

/// What a [`TextEntryState`] does with the typed text on Enter.
pub type OnConfirm = Box<dyn FnMut(&mut GameContext, &str)>;

impl TextEntryState {
    pub fn new(
        heading: &str,
        field_label: &str,
        on_confirm: impl FnMut(&mut GameContext, &str) + 'static,
    ) -> Self {
        Self {
            entry: TextEntryCore::new(heading, field_label),
            on_confirm: Box::new(on_confirm),
        }
    }

    /// The typed text.
    pub fn name(&self) -> String {
        self.entry.text()
    }

    pub fn cursor(&self) -> usize {
        self.entry.cursor
    }
}

impl TextEntry for TextEntryState {
    fn entry(&self) -> &TextEntryCore {
        &self.entry
    }

    fn entry_mut(&mut self) -> &mut TextEntryCore {
        &mut self.entry
    }

    fn confirm(&mut self, ctx: &mut GameContext) {
        let text = self.entry.text();
        (self.on_confirm)(ctx, &text);
    }
}

impl_state_for_text_entry!(TextEntryState);

// Keep the trait object usable where a plain `State` is expected.
#[allow(dead_code)]
fn _assert_state(state: TextEntryState) -> Box<dyn State> {
    Box::new(state)
}
