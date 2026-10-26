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

/// The longest string a field takes.
pub const MAX_LEN: usize = 24;

/// A field's controls in gestures, for a touch player with no hardware
/// keyboard to name keys on. The on-screen keyboard rises with the field.
pub const TOUCH_FIELD_HELP: &str = "Type with the on-screen keyboard, then double tap. \
    Flick left or right to review the letters, swipe up or down with three fingers to jump \
    to the start or end, and swipe down with two fingers to cancel.";

/// Whether a field prompt should name gestures rather than keys.
pub fn touch_prompt(ctx: &GameContext) -> bool {
    ctx.controller.device() == ff_core::input_hints::TOUCH
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
}

impl TextEntryCore {
    pub fn new(heading: &str, field_label: &str) -> Self {
        Self {
            heading: heading.to_string(),
            field_label: field_label.to_string(),
            name: Vec::new(),
            cursor: 0,
            max_len: MAX_LEN,
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
        if touch_prompt(ctx) {
            ctx.say(&format!("{heading}. {TOUCH_FIELD_HELP}"));
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

    fn lines(&self, _ctx: &GameContext) -> Vec<String> {
        let field = self.entry();
        let before: String = field.name[..field.cursor].iter().collect();
        let after: String = field.name[field.cursor..].iter().collect();
        vec![
            field.heading.clone(),
            String::new(),
            format!("{}: {before}|{after}", field.field_label),
            "Left and right arrows review letters, Home and End jump to the \
             ends. Enter to confirm, Escape to cancel, F2 to hear the whole text, \
             Control V to paste."
                .to_string(),
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

/// Write the [`State`] impl for a [`TextEntry`] screen, forwarding the
/// hooks to the `TextEntry` methods of the same name.
#[macro_export]
macro_rules! impl_state_for_text_entry {
    ($ty:ty) => {
        impl $crate::states::base::State for $ty {
            fn enter(&mut self, ctx: &mut $crate::app::GameContext) {
                <Self as $crate::states::text_entry::TextEntry>::enter(self, ctx)
            }
            fn exit(&mut self, ctx: &mut $crate::app::GameContext) {
                <Self as $crate::states::text_entry::TextEntry>::exit(self, ctx)
            }
            fn update(&mut self, ctx: &mut $crate::app::GameContext, dt: f64) {
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
