//! Every driving command by name: F2 at the wheel, and three-finger tap on
//! iPhone and iPad.
//!
//! A keyboard driver has a key for each control; a touch or switch driver
//! does not. This list names each one-shot control, and choosing a row closes
//! the list and runs the control exactly as its key would, so the answer is
//! spoken over the road the driver returns to. The held controls -- the
//! pedals, steering, the emergency brake, the horn -- stay off the list:
//! they last as long as a key or finger is held, which a menu row cannot do.

use crate::app::{GameContext, Say};
use crate::bindings::Action;
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving::DrivingState;
use crate::states::driving_menu_states::DriveRef;

const COMMANDS_INTRO_HELP: &str =
    "Up and down pick a command, Enter runs it and returns to driving, Escape returns without \
     one.";

/// A row that is not one of the player's bindable actions.
#[derive(Clone, Copy)]
enum Fixed {
    CruiseUp,
    CruiseDown,
    NextStation,
    PreviousStation,
    Help,
}

impl Fixed {
    const ALL: [Fixed; 5] = [
        Fixed::CruiseUp,
        Fixed::CruiseDown,
        Fixed::NextStation,
        Fixed::PreviousStation,
        Fixed::Help,
    ];

    fn label(self) -> &'static str {
        match self {
            Fixed::CruiseUp => "Raise the cruise target",
            Fixed::CruiseDown => "Lower the cruise target",
            Fixed::NextStation => "Next radio station",
            Fixed::PreviousStation => "Previous radio station",
            Fixed::Help => "Driving help",
        }
    }

    fn key_name(self) -> &'static str {
        match self {
            Fixed::CruiseUp => "Plus",
            Fixed::CruiseDown => "Minus",
            Fixed::NextStation => "Page Down",
            Fixed::PreviousStation => "Page Up",
            Fixed::Help => "F1",
        }
    }

    fn run(self, drive: &mut DrivingState, ctx: &mut GameContext) {
        match self {
            Fixed::CruiseUp => drive.adjust_cruise(ctx, 1, false),
            Fixed::CruiseDown => drive.adjust_cruise(ctx, -1, false),
            Fixed::NextStation => drive.tune_radio(ctx, 1),
            Fixed::PreviousStation => drive.tune_radio(ctx, -1),
            Fixed::Help => drive.speak_driving_help(ctx),
        }
    }
}

/// Whether the list offers this action.
pub fn offers(action: Action) -> bool {
    action.on_keyboard() && !action.held()
}

pub struct DrivingCommandsState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    /// The newest log line from before the list opened, so the copy row
    /// copies what the player heard at the wheel and not the list heading.
    copy_text: Option<String>,
}

impl DrivingCommandsState {
    pub fn new(ctx: &GameContext) -> Self {
        let mut state = Self::with_drive(DriveRef::active(ctx));
        state.copy_text = crate::states::base::newest_copyable_line(ctx);
        state
    }

    /// The same list built over a drive the caller already shares.
    pub fn with_drive(driving: DriveRef) -> Self {
        DrivingCommandsState {
            menu: MenuCore::new("Driving commands").with_intro_help(COMMANDS_INTRO_HELP),
            driving,
            copy_text: None,
        }
    }

    /// Close the list, then run `f` on the drive as a key press would.
    fn run_on_drive(
        &mut self,
        ctx: &mut GameContext,
        f: impl FnOnce(&mut DrivingState, &mut GameContext),
    ) {
        ctx.pop_state();
        self.driving.with(ctx, |drive, ctx| {
            let previous = ctx.player_asked_begin();
            f(drive, ctx);
            ctx.player_asked_end(previous);
        });
    }
}

impl Menu for DrivingCommandsState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Action::all()
            .filter(|action| offers(*action))
            .map(|action| {
                let mut help = format!("On the keyboard: {}.", ctx.bindings.spoken(action));
                if action.on_pad() {
                    help.push_str(&format!(
                        " On the controller: {}.",
                        ctx.bindings.pad_spoken(action)
                    ));
                }
                MenuItem::new(
                    action.label(),
                    move |s: &mut Self, ctx: &mut GameContext| {
                        if action == Action::CopyLastSpoken {
                            if let Some(text) = s.copy_text.clone() {
                                ctx.pop_state();
                                crate::states::base::copy_text(ctx, Some(text));
                                return;
                            }
                        }
                        s.run_on_drive(ctx, |drive, ctx| drive.run_key_action(ctx, action));
                    },
                )
                .help(help)
            })
            .collect();
        items.extend(Fixed::ALL.into_iter().map(|fixed| {
            MenuItem::new(fixed.label(), move |s: &mut Self, ctx: &mut GameContext| {
                s.run_on_drive(ctx, |drive, ctx| fixed.run(drive, ctx));
            })
            .help(format!("On the keyboard: {}.", fixed.key_name()))
        }));
        items.push(
            MenuItem::new("Back to driving", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Close the list without running a command."),
        );
        items
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
        ctx.say_with("Back to driving.", Say::queued());
    }
}

impl_state_for_menu!(DrivingCommandsState);
