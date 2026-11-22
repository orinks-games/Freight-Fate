//! Settings, Gameplay, Controls: the units, the keyboard, a controller and,
//! on a phone, the touch screen.
//!
//! A phone lists the keyboard rows only while a hardware keyboard is
//! connected and the controller rows only while a controller is, and puts
//! Touch gestures first. The desktop list never changes. The rows and their
//! Left and Right actions are built together here, so hiding a row can never
//! leave the arrow keys answering for its neighbour.

use super::*;
use crate::bindings::Action;
use crate::states::main_menu::shortcuts::{ShortcutDevice, ShortcutsState};
use crate::states::main_menu::touch_gestures::TouchGesturesState;

/// One Controls row and what Left and Right do on it; `None` for a row that
/// opens a screen and has no value to step.
type ControlsRow = (Row, Option<Adjust>);

/// What decides which Controls rows a phone lists. A change while the
/// screen is open rebuilds it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::states::main_menu) struct ControlsHardware {
    /// The keyboard rows: always on a desktop, on a phone only while a
    /// hardware keyboard is connected.
    pub keyboard: bool,
    /// The Controller row itself. A phone with controller support turned
    /// off keeps it, because a pad is never opened while support is off and
    /// this row is the only way to turn it back on.
    pub controller_row: bool,
    /// Haptics, the second layer bumper and the button screen.
    pub controller: bool,
    pub phone: bool,
}

impl ControlsHardware {
    pub fn now(ctx: &GameContext) -> Self {
        let phone = crate::device::phone();
        let pad = ctx.controller.connected();
        Self {
            keyboard: !phone || ctx.controller.hardware_keyboard(),
            controller_row: !phone || pad || !ctx.settings.controller_enabled,
            controller: !phone || pad,
            phone,
        }
    }
}

/// The Controller row: the setting, and while it is on, the pad the game
/// is listening to, so a player can tell whether their controller was found.
fn controller_label(ctx: &GameContext) -> String {
    if !ctx.settings.controller_enabled {
        return "Controller: disabled".to_string();
    }
    if ctx.controller.connected() {
        format!("Controller: enabled, {} connected", ctx.controller.name())
    } else {
        "Controller: enabled, none connected".to_string()
    }
}

/// Only the iPhone and iPad game has a touch surface to bind.
fn touch_gestures_row() -> ControlsRow {
    let row = MenuItem::new(
        "Touch gestures",
        |_s: &mut SettingsCategoryState, ctx: &mut GameContext| {
            ctx.push_state(TouchGesturesState::new())
        },
    )
    .help(
        "Choose which driving command each touch gesture runs, including a second \
         finger while holding gas or brake.",
    );
    (row, None)
}

fn stepped(label: Label<SettingsCategoryState>, action: Adjust, help: &str) -> ControlsRow {
    (row(label, Rc::clone(&action), help), Some(action))
}

/// The transmission row's help on a touch screen, naming the gestures the
/// shift commands are on now.
fn transmission_touch_help(ctx: &GameContext) -> String {
    let up = ctx.touch_instruction(Action::ShiftUp);
    let down = ctx.touch_instruction(Action::ShiftDown);
    format!(
        "Automatic shifts for you. In manual, the clutch works itself. \
         {up} to shift up. {down} to shift down."
    )
}

/// Map the label to the row it names, for keeping focus across a rebuild:
/// the part before the colon, which no value changes.
pub(in crate::states::main_menu) fn row_key(label: &str) -> &str {
    label.split(':').next().unwrap_or(label)
}

impl SettingsCategoryState {
    /// Every Controls row this run lists, with its Left and Right action.
    pub(super) fn controls_rows(&self, ctx: &GameContext) -> Vec<ControlsRow> {
        let has = ControlsHardware::now(ctx);
        let mut rows: Vec<ControlsRow> = Vec::new();
        if has.phone {
            rows.push(touch_gestures_row());
        }
        rows.push(stepped(
            dyn_label(|s| {
                format!(
                    "Units: {}",
                    if s.imperial_units {
                        "imperial, miles"
                    } else {
                        "metric, kilometers"
                    }
                )
            }),
            adjust(|s, ctx, d| s.toggle_units(ctx, d)),
            "Distance and speed in miles or kilometers.",
        ));
        let (transmission, action) = stepped(
            dyn_label(|s| {
                format!(
                    "Transmission: {}",
                    if s.automatic_transmission {
                        "automatic"
                    } else {
                        "manual"
                    }
                )
            }),
            adjust(|s, ctx, d| s.toggle_transmission(ctx, d)),
            "Automatic shifts for you. Manual: hold the clutch, \
             then {{shift_up}} shifts up and {{shift_down}} shifts down.",
        );
        rows.push((
            transmission.touch_help(Label::dynamic(|_s, ctx| transmission_touch_help(ctx))),
            action,
        ));
        if has.controller_row {
            rows.push(stepped(
                Label::dynamic(|_s, ctx| controller_label(ctx)),
                adjust(|s, ctx, d| s.toggle_controller(ctx, d)),
                if has.phone {
                    "Game-controller input alongside the touch screen, which \
                     always stays active. The first connected controller is \
                     used."
                } else {
                    "Game-controller input alongside the keyboard, which \
                     always stays active. The first connected controller is \
                     used."
                },
            ));
        }
        if has.controller {
            rows.push(stepped(
                dyn_label(|s| {
                    format!(
                        "Haptics: {}",
                        if s.haptics_enabled {
                            "enabled"
                        } else {
                            "disabled"
                        }
                    )
                }),
                adjust(|s, ctx, d| s.toggle_haptics(ctx, d)),
                "Controller rumble for hazards, hard braking, the rumble \
                 strip, and road seams. Needs a controller connected.",
            ));
            rows.push(stepped(
                dyn_label(|s| {
                    format!(
                        "Second layer bumper: {}",
                        if s.left_bumper_layer { "left" } else { "right" }
                    )
                }),
                adjust(|s, ctx, d| s.toggle_left_bumper_layer(ctx, d)),
                "Which bumper you hold for the second layer of controller \
                 buttons. The other bumper is the clutch.",
            ));
        }
        // A build without the visual display has nothing to turn off.
        #[cfg(feature = "visuals")]
        rows.push(stepped(
            dyn_label(|s| format!("Visual display: {}", on_off(s.visual_display))),
            adjust(|s, ctx, d| s.toggle_visual_display(ctx, d)),
            if has.phone {
                "Draws the road, the dashboard, menus and captions of what \
                 is spoken on the screen, for sighted and low-vision \
                 players. Off keeps the screen dark and saves battery. \
                 Speech and sound are the same either way."
            } else {
                "Draws the road, the dashboard, menus and captions of what \
                 is spoken in the game window, for sighted and low-vision \
                 players. Off keeps the window dark and saves battery. \
                 Speech and sound are the same either way."
            },
        ));
        if has.keyboard {
            rows.push(stepped(
                dyn_label(|s| format!("Letters run menu items: {}", on_off(s.menu_accelerator_keys))),
                adjust(|s, ctx, d| s.toggle_menu_accelerator_keys(ctx, d)),
                "On, typing the first letter of an item runs it when no other item starts with that letter. When several share it, the letter moves between them and Enter runs the one you are on. Off, letters only move to the item.",
            ));
            rows.push((
                MenuItem::new(
                    "Keyboard shortcuts",
                    |_s: &mut SettingsCategoryState, ctx| {
                        ctx.push_state(ShortcutsState::new(ShortcutDevice::Keyboard))
                    },
                )
                .help(
                    "Move any driving key to another key. Enter on a control, then \
                     press the key you want for it.",
                ),
                None,
            ));
        }
        if has.controller {
            rows.push((
                MenuItem::new(
                    "Controller buttons",
                    |_s: &mut SettingsCategoryState, ctx| {
                        ctx.push_state(ShortcutsState::new(ShortcutDevice::Controller))
                    },
                )
                .help(Label::dynamic(|_s, ctx| {
                    if ctx.settings.left_bumper_layer {
                        "Move any driving control to another pad button, plain or with \
                         the left bumper held."
                    } else {
                        "Move any driving control to another pad button, plain or with \
                         the right bumper held."
                    }
                    .to_string()
                })),
                None,
            ));
        }
        // The speed keeper moved to Driving assistance: it holds a speed
        // for you, which is what every other row on that screen does.
        // Controls is the keyboard, the controller, and the units the
        // numbers arrive in.
        rows.push((back_row(), None));
        rows
    }
}
