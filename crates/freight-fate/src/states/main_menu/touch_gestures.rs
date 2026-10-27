//! Settings, Gameplay, Controls, Touch gestures (iPhone and iPad): one row
//! per driving gesture, each naming the command it runs today. Enter on a
//! row opens the list of commands; choosing one puts it on the gesture and
//! saves at once. The fixed gestures (pedal holds, steering, Enter, pause,
//! help, message review, the radio dial, the command list, the keyboard) are
//! not listed, the same way the fixed keys are not on the keyboard screen.

use crate::app::GameContext;
use crate::bindings::{touch_gesture_name, touch_slots, TouchCommand};
use crate::impl_state_for_menu;
use crate::states::base::{Label, Menu, MenuCore, MenuItem};
use crate::touch::Gesture;

use super::settings::save_settings;
use super::TouchPracticeState;

const GESTURES_HELP: &str = "Move up and down to pick a gesture, then confirm it to choose the \
                             command it runs while driving. Go back to leave.";

fn command_name(ctx: &GameContext, gesture: Gesture) -> &'static str {
    ctx.bindings
        .touch_command(gesture)
        .unwrap_or(TouchCommand::Nothing)
        .label()
}

fn gesture_name(gesture: Gesture) -> &'static str {
    touch_gesture_name(gesture).unwrap_or("Gesture")
}

pub struct TouchGesturesState {
    menu: MenuCore<Self>,
}

impl TouchGesturesState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Touch gestures").with_intro_help(GESTURES_HELP),
        }
    }

    fn reset_all(&mut self, ctx: &mut GameContext) {
        ctx.bindings.reset_touch();
        ctx.bindings.store(&mut ctx.settings);
        save_settings(&ctx.settings);
        ctx.say("Every touch gesture is back to its default.");
    }

    fn toggle_haptics(&mut self, ctx: &mut GameContext) {
        ctx.settings.touch_haptics = !ctx.settings.touch_haptics;
        crate::app::sdl_shell::set_touch_haptics(ctx.settings.touch_haptics);
        save_settings(&ctx.settings);
        ctx.say(if ctx.settings.touch_haptics {
            "Touch haptics enabled."
        } else {
            "Touch haptics disabled."
        });
    }
}

impl Default for TouchGesturesState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for TouchGesturesState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = touch_slots()
            .map(|gesture| {
                MenuItem::new(
                    Label::dynamic(move |_s: &Self, ctx| {
                        format!("{}: {}", gesture_name(gesture), command_name(ctx, gesture))
                    }),
                    move |_s: &mut Self, ctx: &mut GameContext| {
                        let picker = TouchCommandPickerState::new(ctx, gesture);
                        ctx.push_state(picker);
                    },
                )
                .help("Confirm, then choose the command this gesture runs while driving.")
            })
            .collect();
        items.push(
            MenuItem::new("Practice gestures", |_s: &mut Self, ctx| {
                ctx.push_state(TouchPracticeState::new());
            })
            .help("Names each gesture and its current command without controlling the truck."),
        );
        items.push(
            MenuItem::new(
                Label::dynamic(|_s: &Self, ctx| {
                    format!(
                        "Touch haptics: {}",
                        if ctx.settings.touch_haptics {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    )
                }),
                |s: &mut Self, ctx| s.toggle_haptics(ctx),
            )
            .help("Light feedback confirms pedals and commands. Emergency braking warns."),
        );
        items.push(
            MenuItem::new(
                "Reset every touch gesture to its default",
                |s: &mut Self, ctx| s.reset_all(ctx),
            )
            .help("Puts every gesture on this screen back where it started."),
        );
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        save_settings(&ctx.settings);
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
    }
}

impl_state_for_menu!(TouchGesturesState);

/// The commands one gesture can run, opened on the one it runs now.
pub struct TouchCommandPickerState {
    menu: MenuCore<Self>,
    gesture: Gesture,
}

impl TouchCommandPickerState {
    pub fn new(ctx: &GameContext, gesture: Gesture) -> Self {
        let mut menu = MenuCore::new(gesture_name(gesture)).with_intro_help(
            "Move up and down to pick a command and confirm to put it on the gesture. Going back keeps the one it has.",
        );
        let current = ctx.bindings.touch_command(gesture);
        menu.index = TouchCommand::all()
            .position(|command| Some(command) == current)
            .unwrap_or(0);
        Self { menu, gesture }
    }

    fn choose(&mut self, ctx: &mut GameContext, command: TouchCommand) {
        ctx.bindings.set_touch_command(self.gesture, command);
        ctx.bindings.store(&mut ctx.settings);
        save_settings(&ctx.settings);
        ctx.audio.play("ui/menu_select");
        ctx.pop_state();
        ctx.say(&format!(
            "{} now runs {}.",
            gesture_name(self.gesture),
            command.label()
        ));
    }
}

impl Menu for TouchCommandPickerState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        TouchCommand::all()
            .map(|command| {
                MenuItem::new(command.label(), move |s: &mut Self, ctx| {
                    s.choose(ctx, command)
                })
            })
            .collect()
    }
}

impl_state_for_menu!(TouchCommandPickerState);
