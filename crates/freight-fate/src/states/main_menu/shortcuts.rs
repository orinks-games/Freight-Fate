//! Settings, Gameplay, Controls, then Keyboard shortcuts or Controller
//! buttons: one row per driving control, each naming the key or button it is
//! on today. Enter on a row, then press the key or button you want for it.
//!
//! The capture is one press. A key that is fixed, or already someone else's,
//! is refused with the reason and the screen keeps listening; Escape keeps
//! what the row had. On the pad the right bumper held during the press picks
//! the second layer, and pressing the button the row already has keeps it,
//! since every other button is a candidate and Escape is a keyboard key.
//! Each change is saved as it lands, so a crash a minute later loses nothing.

use crate::app::GameContext;
use crate::bindings::{Action, Chord, PadChord, Rebind};
use crate::controller::{ControllerAction, ControllerButton};
use crate::impl_state_for_menu;
use crate::states::base::default_handle_event;
use crate::states::base::{InputEvent, Key, Label, Menu, MenuCore, MenuItem, Mods};

use super::settings::save_settings;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShortcutDevice {
    Keyboard,
    Controller,
}

pub struct ShortcutsState {
    menu: MenuCore<Self>,
    device: ShortcutDevice,
    /// The control waiting for its new key or button.
    capturing: Option<Action>,
}

impl ShortcutsState {
    pub fn new(device: ShortcutDevice) -> Self {
        let (title, help) = match device {
            ShortcutDevice::Keyboard => (
                "Keyboard shortcuts",
                "Up and Down pick a control, Enter then a key press moves it, \
                 Escape goes back.",
            ),
            ShortcutDevice::Controller => (
                "Controller buttons",
                "Up and Down pick a control, Enter then a button press moves it, \
                 Escape goes back.",
            ),
        };
        Self {
            menu: MenuCore::new(title).with_intro_help(help),
            device,
            capturing: None,
        }
    }

    pub fn device(&self) -> ShortcutDevice {
        self.device
    }

    /// The control being captured for, if any (the tests read it).
    pub fn capturing(&self) -> Option<Action> {
        self.capturing
    }

    fn current_name(&self, ctx: &GameContext, action: Action) -> String {
        match self.device {
            ShortcutDevice::Keyboard => ctx.bindings.spoken(action),
            ShortcutDevice::Controller => ctx.bindings.pad_spoken(action),
        }
    }

    fn begin_capture(&mut self, ctx: &mut GameContext, action: Action) {
        self.capturing = Some(action);
        let current = self.current_name(ctx, action);
        let prompt = match self.device {
            ShortcutDevice::Keyboard => format!(
                "Press the new key for {}. Escape keeps {current}.",
                action.label()
            ),
            ShortcutDevice::Controller => format!(
                "Press the new button for {}, with the right bumper held for the \
                 second layer. Press {current} again to keep it.",
                action.label()
            ),
        };
        ctx.say(&prompt);
    }

    fn finish(&mut self, ctx: &mut GameContext, action: Action, result: Rebind, chosen: &str) {
        let current = self.current_name(ctx, action);
        match result {
            Rebind::Done => {
                self.capturing = None;
                ctx.bindings.store(&mut ctx.settings);
                save_settings(&ctx.settings);
                ctx.audio.play("ui/menu_select");
                let now = self.current_name(ctx, action);
                self.refresh(ctx, true);
                ctx.say(&format!("{} is now {now}.", action.label()));
            }
            Rebind::Unchanged => {
                self.capturing = None;
                ctx.say(&format!("Kept {current}."));
            }
            Rebind::Taken(other) => {
                ctx.audio.play("ui/error");
                ctx.say(&format!(
                    "{} is already {}. Press another, or keep {current} with Escape.",
                    sentence_start(chosen),
                    other.label()
                ));
            }
            Rebind::Reserved(reason) => {
                ctx.audio.play("ui/error");
                ctx.say(&format!(
                    "{}. Press another, or keep {current} with Escape.",
                    sentence_start(reason)
                ));
            }
        }
    }

    fn capture_key(&mut self, ctx: &mut GameContext, action: Action, key: Key, mods: Mods) {
        if matches!(
            key,
            Key::LCtrl | Key::RCtrl | Key::LShift | Key::RShift | Key::LAlt | Key::RAlt
        ) {
            return; // half a chord: wait for the key that goes with it
        }
        if key == Key::Escape {
            self.capturing = None;
            let current = self.current_name(ctx, action);
            ctx.say(&format!("Kept {current}."));
            return;
        }
        let chord = Chord { key, mods };
        let result = ctx.bindings.set_chord(action, chord);
        self.finish(ctx, action, result, &chord.spoken());
    }

    fn capture_button(&mut self, ctx: &mut GameContext, action: Action, button: ControllerButton) {
        if matches!(
            button,
            ControllerButton::LeftShoulder | ControllerButton::RightShoulder
        ) {
            return; // the layer keys, not a choice
        }
        let chord = PadChord {
            button,
            modified: ctx.controller.modifier,
        };
        let result = ctx.bindings.set_pad_chord(action, chord);
        self.finish(ctx, action, result, &chord.spoken());
    }

    fn reset_all(&mut self, ctx: &mut GameContext) {
        match self.device {
            ShortcutDevice::Keyboard => ctx.bindings.reset_keys(),
            ShortcutDevice::Controller => ctx.bindings.reset_pad(),
        }
        ctx.bindings.store(&mut ctx.settings);
        save_settings(&ctx.settings);
        self.refresh(ctx, true);
        ctx.say(match self.device {
            ShortcutDevice::Keyboard => "Every keyboard shortcut is back to its default.",
            ShortcutDevice::Controller => "Every controller button is back to its default.",
        });
    }
}

impl Menu for ShortcutsState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let device = self.device;
        let mut items: Vec<MenuItem<Self>> = Action::all()
            .filter(|action| match device {
                ShortcutDevice::Keyboard => action.on_keyboard(),
                ShortcutDevice::Controller => action.on_pad(),
            })
            .map(|action| {
                MenuItem::new(
                    Label::dynamic(move |s: &Self, ctx| {
                        format!("{}: {}", action.label(), s.current_name(ctx, action))
                    }),
                    move |s: &mut Self, ctx| s.begin_capture(ctx, action),
                )
                .help(match device {
                    ShortcutDevice::Keyboard => {
                        "Enter, then press the key you want for this control. \
                         Escape keeps the one it has."
                    }
                    ShortcutDevice::Controller => {
                        "Enter, then press the button you want for this control, \
                         with the right bumper held for the second layer."
                    }
                })
            })
            .collect();
        items.push(
            MenuItem::new(
                match device {
                    ShortcutDevice::Keyboard => "Reset every keyboard shortcut to its default",
                    ShortcutDevice::Controller => "Reset every controller button to its default",
                },
                |s: &mut Self, ctx| s.reset_all(ctx),
            )
            .help("Puts every control on this screen back where it started."),
        );
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let Some(action) = self.capturing else {
            return default_handle_event(self, ctx, event);
        };
        if let Some((key, mods, _)) = event.key_down() {
            self.capture_key(ctx, action, key, mods);
        }
    }

    fn handle_controller(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        if let Some(action) = self.capturing {
            if let InputEvent::ControllerButtonDown { button, .. } = event {
                self.capture_button(ctx, action, *button);
            }
            return;
        }
        match ctx.controller.menu_action(event) {
            Some(ControllerAction::MenuDown) => self.move_by(ctx, 1),
            Some(ControllerAction::MenuUp) => self.move_by(ctx, -1),
            Some(ControllerAction::Confirm) => self.activate(ctx),
            Some(ControllerAction::Back) => self.go_back(ctx),
            Some(ControllerAction::Help) => {
                let help = self.current_help(ctx);
                ctx.say(&help);
            }
            _ => {}
        }
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        save_settings(&ctx.settings);
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
    }
}

impl_state_for_menu!(ShortcutsState);

/// "the A button" or "start is pause" with its first letter up, to open a
/// sentence.
fn sentence_start(phrase: &str) -> String {
    let mut chars = phrase.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
