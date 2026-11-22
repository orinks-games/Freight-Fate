//! Settings, Gameplay, Controls, Touch gestures (iPhone and iPad): one row
//! per driving gesture, each naming the command it runs today. Enter on a
//! row opens the list of commands; choosing one puts it on the gesture and
//! saves at once. The fixed gestures (pedal holds, steering, Enter, pause,
//! help, message review, the radio dial, the command list, the keyboard) are
//! not listed, the same way the fixed keys are not on the keyboard screen.

use crate::app::GameContext;
use crate::bindings::{touch_gesture_name, touch_slots, TouchCommand};
use crate::impl_state_for_menu;
use crate::states::base::{Key, Label, Menu, MenuCore, MenuItem};
use crate::touch::Gesture;

use super::settings::save_settings;
use super::InputPracticeState;

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

/// The deep swipe row's help, in each input's own words.
fn deep_swipe_help(ctx: &GameContext) -> &'static str {
    match ctx.controller.device() {
        ff_core::input_hints::TOUCH => {
            "How far a swipe down travels before it becomes the emergency brake, or the \
             parking brake when stopped. Practice gestures reads the length of each swipe \
             down. Flick up or down to change it."
        }
        ff_core::input_hints::CONTROLLER => {
            "How far a swipe down travels before it becomes the emergency \
             brake, or the parking brake when stopped. Practice gestures reads the length \
             of each swipe down. D-pad Right or Left changes it."
        }
        _ => {
            "How far a swipe down travels before it becomes the emergency \
             brake, or the parking brake when stopped. Practice gestures reads the length \
             of each swipe down. Right or Left arrow changes it."
        }
    }
}

/// The tilt steering row's help, in each input's own words.
fn tilt_help(ctx: &GameContext) -> &'static str {
    match ctx.controller.device() {
        ff_core::input_hints::TOUCH => {
            "On, tilting the phone left or right steers, and the screen stops turning with \
             the phone while you drive. With lane keeping on full, a tilt changes one lane. \
             Swipes and flicks still steer. Flick up or down to change it."
        }
        ff_core::input_hints::CONTROLLER => {
            "On, tilting the phone left or right steers, and the screen stops turning with \
             the phone while you drive. With lane keeping on full, a tilt changes one lane. \
             Swipes and flicks still steer. D-pad Right or Left changes it."
        }
        _ => {
            "On, tilting the phone left or right steers, and the screen stops turning with \
             the phone while you drive. With lane keeping on full, a tilt changes one lane. \
             Swipes and flicks still steer. Right or Left arrow changes it."
        }
    }
}

/// The tilt for full steering row's help, in each input's own words.
fn tilt_full_help(ctx: &GameContext) -> &'static str {
    match ctx.controller.device() {
        ff_core::input_hints::TOUCH => {
            "How far the phone tilts for full steering. A smaller number steers harder. \
             Flick up or down to change it."
        }
        ff_core::input_hints::CONTROLLER => {
            "How far the phone tilts for full steering. A smaller number steers harder. \
             D-pad Right or Left changes it."
        }
        _ => {
            "How far the phone tilts for full steering. A smaller number steers harder. \
             Right or Left arrow changes it."
        }
    }
}

/// Whether tilt steering is steering: on, on a device with a motion sensor.
fn tilt_on(ctx: &GameContext) -> bool {
    crate::app::tilt::wanted(&ctx.settings)
        && ctx.tilt.state() != ff_core::sim::tilt_steering::TiltSteering::NoSensor
}

pub struct TouchGesturesState {
    menu: MenuCore<Self>,
    /// Where the deep swipe row sits, one of the rows Left and Right change.
    deep_swipe_row: usize,
    /// Where the two tilt steering rows sit; the second only while it is on.
    tilt_row: usize,
    tilt_full_row: usize,
}

impl TouchGesturesState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Touch gestures").with_intro_help(GESTURES_HELP),
            deep_swipe_row: usize::MAX,
            tilt_row: usize::MAX,
            tilt_full_row: usize::MAX,
        }
    }

    /// Turn tilt steering on or off, save, and read the row again. A
    /// device with no motion sensor keeps it off and says so.
    fn toggle_tilt(&mut self, ctx: &mut GameContext) {
        let turning_on = !tilt_on(ctx);
        if turning_on && ctx.tilt.sensor_present == Some(false) {
            ctx.settings.touch_tilt_steering = false;
            save_settings(&ctx.settings);
            ctx.say(ff_core::sim::tilt_steering::TiltLine::NoSensor.text());
            return;
        }
        ctx.settings.touch_tilt_steering = turning_on;
        save_settings(&ctx.settings);
        ctx.audio.play("ui/menu_select");
        self.refresh(ctx, true);
        self.speak_current(ctx);
    }

    /// Move the tilt for full steering one step, save, and read the row.
    fn step_tilt_full(&mut self, ctx: &mut GameContext, direction: i64) {
        ctx.settings.step_touch_tilt_full(direction.signum());
        save_settings(&ctx.settings);
        ctx.audio.play("ui/menu_select");
        self.speak_current(ctx);
    }

    /// Left and Right, or a flick up and down, on a row that changes.
    fn step_row(&mut self, ctx: &mut GameContext, direction: i64) -> bool {
        let index = self.menu.index;
        if index == self.deep_swipe_row {
            self.step_deep_swipe(ctx, direction);
        } else if index == self.tilt_row {
            self.toggle_tilt(ctx);
        } else if index == self.tilt_full_row {
            self.step_tilt_full(ctx, direction);
        } else {
            return false;
        }
        true
    }

    /// Move where the deep swipe starts one step, hand it to the touch
    /// surface, save, and read the row again.
    fn step_deep_swipe(&mut self, ctx: &mut GameContext, direction: i64) {
        ctx.settings.step_touch_deep_swipe(direction.signum());
        crate::app::sdl_shell::set_touch_tuning(&ctx.settings);
        save_settings(&ctx.settings);
        ctx.audio.play("ui/menu_select");
        self.speak_current(ctx);
    }

    fn reset_all(&mut self, ctx: &mut GameContext) {
        ctx.bindings.reset_touch();
        ctx.bindings.store(&mut ctx.settings);
        save_settings(&ctx.settings);
        ctx.say("Every touch gesture is back to its default.");
    }

    /// Switch between the native and the shared gesture recognizer, save,
    /// and read the row again. The touch surface lets go of any held
    /// pedal as it switches.
    fn toggle_shared_recogniser(&mut self, ctx: &mut GameContext) {
        ctx.settings.touch_shared_recogniser = !ctx.settings.touch_shared_recogniser;
        save_settings(&ctx.settings);
        ctx.audio.play("ui/menu_select");
        self.speak_current(ctx);
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
    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        // The switch can go off without a press: a phone with no motion
        // sensor, found once it was on. The angle row goes with it.
        if (self.tilt_full_row != usize::MAX) != tilt_on(ctx) {
            if self.menu.index == self.tilt_full_row {
                self.menu.index = self.tilt_row;
            }
            self.refresh(ctx, true);
        }
    }

    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        self.tilt_row = items.len();
        items.push(
            MenuItem::new(
                Label::dynamic(|_s: &Self, ctx| {
                    if tilt_on(ctx) {
                        "Tilt steering: On".to_string()
                    } else {
                        "Tilt steering: Off".to_string()
                    }
                }),
                |s: &mut Self, ctx| s.toggle_tilt(ctx),
            )
            .help(Label::dynamic(|_s: &Self, ctx| tilt_help(ctx).to_string())),
        );
        self.tilt_full_row = usize::MAX;
        if tilt_on(ctx) {
            self.tilt_full_row = items.len();
            items.push(
                MenuItem::new(
                    Label::dynamic(|_s: &Self, ctx| {
                        format!(
                            "Tilt for full steering: {} degrees",
                            ctx.settings.touch_tilt_full()
                        )
                    }),
                    |s: &mut Self, ctx| s.step_tilt_full(ctx, 1),
                )
                .help(Label::dynamic(|_s: &Self, ctx| {
                    tilt_full_help(ctx).to_string()
                })),
            );
        }
        items.extend(touch_slots().map(|gesture| {
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
        }));
        self.deep_swipe_row = items.len();
        items.push(
            MenuItem::new(
                Label::dynamic(|_s: &Self, ctx| {
                    format!(
                        "Deep swipe starts at: {}",
                        ctx.settings.touch_deep_swipe_points()
                    )
                }),
                |s: &mut Self, ctx| s.step_deep_swipe(ctx, 1),
            )
            .help(Label::dynamic(|_s: &Self, ctx| {
                deep_swipe_help(ctx).to_string()
            })),
        );
        items.push(
            MenuItem::new("Practice gestures", |_s: &mut Self, ctx| {
                ctx.push_state(InputPracticeState::gestures());
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
        // iPhone and iPad only: Android has only the shared recognizer.
        if crate::device::iphone() {
            items.push(
                MenuItem::new(
                    Label::dynamic(|_s: &Self, ctx| {
                        if ctx.settings.touch_shared_recogniser {
                            "New gesture reading: On".to_string()
                        } else {
                            "New gesture reading: Off".to_string()
                        }
                    }),
                    |s: &mut Self, ctx| s.toggle_shared_recogniser(ctx),
                )
                .help(
                    "Reads your gestures the way the Android version does. Turn it off if a \
                     gesture stops working.",
                ),
            );
        }
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }

    /// Right and Left change the deep swipe and tilt rows; a flick up or
    /// down on touch arrives as the same keys.
    fn handle_extra_key(&mut self, ctx: &mut GameContext, key: Key) -> bool {
        match key {
            Key::Right => self.step_row(ctx, 1),
            Key::Left => self.step_row(ctx, -1),
            _ => false,
        }
    }

    fn adjust(&mut self, ctx: &mut GameContext, direction: i64) {
        self.step_row(ctx, direction);
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
