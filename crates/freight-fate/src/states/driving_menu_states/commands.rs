//! Every driving command by name: F2 at the wheel, and three-finger tap on
//! iPhone and iPad.
//!
//! A keyboard driver has a key for each control; a touch or switch driver
//! does not. This list names each one-shot control, and choosing a row closes
//! the list and runs the control exactly as its key would, so the answer is
//! spoken over the road the driver returns to. The held controls -- the
//! pedals, steering, the emergency brake, the horn -- stay off the list:
//! they last as long as a key or finger is held, which a menu row cannot do.
//!
//! The commands sit in a short list of groups, in a fixed order, so a row is
//! found by count: the Status menu first, then the groups, then the copy
//! row, help and the way back. A group opens in place; Back from it returns
//! to its own row in the top list.

use crate::app::{GameContext, Say};
use crate::bindings::{Action, TouchCommand};
use crate::impl_state_for_menu;
use crate::states::base::{
    default_announce_entry, default_handle_event, InputEvent, Key, Label, Menu, MenuCore, MenuItem,
};
use crate::states::driving::DrivingState;
use crate::states::driving_menu_states::DriveRef;

const COMMANDS_INTRO_HELP: &str =
    "Up and down pick a command, Enter runs it and returns to driving, Escape returns without \
     one.";

/// A row that is not one of the player's bindable actions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Fixed {
    CruiseUp,
    CruiseDown,
    NextStation,
    PreviousStation,
    Help,
    /// Tilt steering takes level again. Listed only while it is on.
    SetTiltLevel,
}

impl Fixed {
    fn label(self) -> &'static str {
        match self {
            Fixed::CruiseUp => "Raise the cruise target",
            Fixed::CruiseDown => "Lower the cruise target",
            Fixed::NextStation => "Next radio station",
            Fixed::PreviousStation => "Previous radio station",
            Fixed::Help => "Driving help",
            Fixed::SetTiltLevel => "Set tilt level",
        }
    }

    /// Its key; `None` for a phone's command, which has no key.
    fn key_name(self) -> Option<&'static str> {
        match self {
            Fixed::CruiseUp => Some("Plus"),
            Fixed::CruiseDown => Some("Minus"),
            Fixed::NextStation => Some("Page Down"),
            Fixed::PreviousStation => Some("Page Up"),
            Fixed::Help => Some("F1"),
            Fixed::SetTiltLevel => None,
        }
    }

    /// The gesture at the wheel, as a noun.
    fn touch_noun(self, ctx: &GameContext) -> Option<String> {
        match self {
            Fixed::CruiseUp => ctx.bindings.touch_gesture_for(Action::CruiseUp),
            Fixed::CruiseDown => ctx.bindings.touch_gesture_for(Action::CruiseDown),
            Fixed::NextStation => Some("a three-finger flick right".to_string()),
            Fixed::PreviousStation => Some("a three-finger flick left".to_string()),
            Fixed::Help => Some("a two-finger flick up".to_string()),
            Fixed::SetTiltLevel => crate::bindings::touch_slots()
                .find(|gesture| {
                    ctx.bindings.touch_command(*gesture)
                        == Some(crate::bindings::TouchCommand::SetTiltLevel)
                })
                .and_then(crate::bindings::touch_gesture_noun),
        }
    }

    fn run(self, drive: &mut DrivingState, ctx: &mut GameContext) {
        match self {
            Fixed::CruiseUp => drive.adjust_cruise(ctx, 1, false),
            Fixed::CruiseDown => drive.adjust_cruise(ctx, -1, false),
            Fixed::NextStation => drive.tune_radio(ctx, 1),
            Fixed::PreviousStation => drive.tune_radio(ctx, -1),
            Fixed::Help => drive.speak_driving_help(ctx),
            Fixed::SetTiltLevel => drive.set_tilt_level(ctx),
        }
    }
}

/// A row that changes a value in place: flick up or Right raises it, flick
/// down or Left lowers it, and the list stays open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Value {
    RadioVolume,
    RadioCategory,
}

impl Value {
    /// The actions it runs, raising then lowering.
    fn actions(self) -> (Action, Action) {
        match self {
            Value::RadioVolume => (Action::RadioVolumeUp, Action::RadioVolumeDown),
            Value::RadioCategory => (Action::RadioCategoryNext, Action::RadioCategoryPrev),
        }
    }

    fn keyboard_help(self) -> &'static str {
        match self {
            Value::RadioVolume => {
                "Left and Right change it. On the keyboard: Shift Page Down and Shift Page Up."
            }
            Value::RadioCategory => {
                "Left and Right change it. On the keyboard: Control Page Up and Control Page \
                 Down."
            }
        }
    }

    fn run(self, drive: &mut DrivingState, ctx: &mut GameContext, direction: i64) {
        match self {
            Value::RadioVolume => drive.adjust_radio_volume(ctx, direction),
            Value::RadioCategory => drive.jump_radio_category(ctx, direction),
        }
    }
}

/// The groups of the top list, in their fixed order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Group {
    Speed,
    Road,
    Place,
    Truck,
    Gears,
    Radio,
    Hours,
}

/// One row of a list.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Entry {
    Run(Action),
    Fixed(Fixed),
    Value(Value),
    Open(Group),
    Back,
}

use Entry::{Fixed as F, Open, Run};

/// The top list. The Status menu leads, so a three-finger tap then a
/// double tap opens it.
const TOP: &[Entry] = &[
    Run(Action::Status),
    Open(Group::Speed),
    Open(Group::Road),
    Open(Group::Place),
    Open(Group::Truck),
    Open(Group::Gears),
    Open(Group::Radio),
    Open(Group::Hours),
    Run(Action::CopyLastSpoken),
    F(Fixed::Help),
    Entry::Back,
];

/// The group of the driving command list that holds `action`, by its
/// spoken name; `None` for a top-level row or a command the list lacks.
pub fn command_group_label(action: Action) -> Option<&'static str> {
    TOP.iter().find_map(|entry| match entry {
        Open(group) => group
            .entries()
            .contains(&Run(action))
            .then(|| group.label()),
        _ => None,
    })
}

/// One row of the driving command list's top level, for a screen that
/// lists commands the same way (the command picker of Touch gestures).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CommandListRow {
    Command(TouchCommand),
    /// A group by its spoken name, with the commands its rows run.
    Group(&'static str, Vec<TouchCommand>),
}

/// The driving command list's top level in its order: its commands and its
/// groups. Rows that run no command a gesture can hold (Driving help, Back)
/// are left out, and a row that changes a value gives both of its commands.
pub fn command_list_rows() -> Vec<CommandListRow> {
    TOP.iter()
        .filter_map(|entry| match entry {
            Run(action) => Some(CommandListRow::Command(TouchCommand::Action(*action))),
            Open(group) => Some(CommandListRow::Group(
                group.label(),
                group.entries().iter().flat_map(entry_commands).collect(),
            )),
            _ => None,
        })
        .collect()
}

fn entry_commands(entry: &Entry) -> Vec<TouchCommand> {
    let actions = match entry {
        Run(action) => vec![*action],
        F(Fixed::CruiseUp) => vec![Action::CruiseUp],
        F(Fixed::CruiseDown) => vec![Action::CruiseDown],
        F(Fixed::NextStation) => vec![Action::RadioTuneNext],
        F(Fixed::PreviousStation) => vec![Action::RadioTunePrev],
        F(Fixed::SetTiltLevel) => return vec![TouchCommand::SetTiltLevel],
        Entry::Value(value) => {
            let (raise, lower) = value.actions();
            vec![raise, lower]
        }
        _ => Vec::new(),
    };
    actions.into_iter().map(TouchCommand::Action).collect()
}

impl Group {
    fn label(self) -> &'static str {
        match self {
            Group::Speed => "Speed and cruise",
            Group::Road => "Road and stops",
            Group::Place => "Where am I",
            Group::Truck => "Truck",
            Group::Gears => "Gears and engine brake",
            Group::Radio => "Radio and CB",
            Group::Hours => "Hours and clock",
        }
    }

    fn entries(self) -> &'static [Entry] {
        match self {
            Group::Speed => &[
                Run(Action::Speed),
                Run(Action::SpeedLimit),
                Run(Action::SafeSpeed),
                Run(Action::Cruise),
                Run(Action::CruiseResume),
                F(Fixed::CruiseUp),
                F(Fixed::CruiseDown),
            ],
            Group::Road => &[
                Run(Action::Upcoming),
                Run(Action::Route),
                Run(Action::Rest),
                Run(Action::TakeExit),
                Run(Action::Weather),
                Run(Action::Grade),
                Run(Action::Lane),
                Run(Action::LaneLocator),
                Run(Action::MirrorLeft),
                Run(Action::MirrorRight),
                Run(Action::LastAnnouncement),
            ],
            Group::Place => &[
                Run(Action::PlaceState),
                Run(Action::PlaceRoad),
                Run(Action::PlaceTown),
                Run(Action::PlaceDirection),
                Run(Action::PlaceCounty),
            ],
            Group::Truck => &[
                Run(Action::Engine),
                Run(Action::ParkingBrake),
                Run(Action::Fuel),
                Run(Action::DriverWindow),
                Run(Action::Fan),
                Run(Action::Climate),
                Run(Action::Reefer),
                Run(Action::Apu),
                // Last, so leaving it out with tilt steering off moves no
                // other row.
                F(Fixed::SetTiltLevel),
            ],
            Group::Gears => &[
                Run(Action::ShiftUp),
                Run(Action::ShiftDown),
                Run(Action::Neutral),
                Run(Action::Reverse),
                Run(Action::TransmissionMode),
                Run(Action::EngineBrake),
                Run(Action::AutoJake),
                Run(Action::JakeStage1),
                Run(Action::JakeStage2),
                Run(Action::JakeStage3),
            ],
            Group::Radio => &[
                Run(Action::Radio),
                F(Fixed::NextStation),
                F(Fixed::PreviousStation),
                Entry::Value(Value::RadioVolume),
                Entry::Value(Value::RadioCategory),
                Run(Action::RadioFavorite),
                Run(Action::RadioStatus),
                Run(Action::RadioNowPlaying),
                Run(Action::Cb),
            ],
            Group::Hours => &[
                Run(Action::Clock),
                Run(Action::HosWheel),
                Run(Action::HosBreak),
                Run(Action::HosDrive),
            ],
        }
    }
}

pub struct DrivingCommandsState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    /// The newest log line from before the list opened, so the copy row
    /// copies what the player heard at the wheel and not the list heading.
    copy_text: Option<String>,
    /// The group open in place of the top list.
    group: Option<Group>,
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
            group: None,
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

    /// Show `group`, or the top list for `None`, on the row `index`.
    fn show(&mut self, ctx: &mut GameContext, group: Option<Group>, index: usize) {
        self.group = group;
        self.menu.title = group.map_or("Driving commands", Group::label).to_string();
        self.refresh(ctx, false);
        self.menu.index = index.min(self.menu.items.len().saturating_sub(1));
        default_announce_entry(self, ctx);
    }

    fn entries(&self) -> &'static [Entry] {
        self.group.map_or(TOP, Group::entries)
    }

    fn focused_value(&self) -> Option<Value> {
        match self.entries().get(self.menu.index) {
            Some(Entry::Value(value)) => Some(*value),
            _ => None,
        }
    }

    /// Raise or lower the focused value row in place.
    fn adjust_value(&mut self, ctx: &mut GameContext, direction: i64) -> bool {
        let Some(value) = self.focused_value() else {
            return false;
        };
        self.driving.with(ctx, |drive, ctx| {
            let previous = ctx.player_asked_begin();
            value.run(drive, ctx, direction.signum());
            ctx.player_asked_end(previous);
        });
        true
    }

    fn item(&self, ctx: &GameContext, entry: Entry) -> MenuItem<Self> {
        let mut item = self.entry_item(ctx, entry);
        // A command row closes the list only to run, so a label like
        // "Resume the last cruise speed" must not count as the group's way
        // back: the touch Back row belongs on every group.
        if !matches!(entry, Entry::Back) {
            item.leaves = false;
        }
        item
    }

    fn entry_item(&self, ctx: &GameContext, entry: Entry) -> MenuItem<Self> {
        match entry {
            Run(action) => action_item(ctx, action),
            F(fixed) => {
                // Set tilt level has no key and no gesture until the player
                // gives it one, so its help says what it does instead.
                const TILT_LEVEL_HELP: &str =
                    "Tilt steering treats the way you hold the phone now as level.";
                let touch = fixed.touch_noun(ctx).map_or_else(
                    || TILT_LEVEL_HELP.to_string(),
                    |noun| format!("At the wheel: {noun}."),
                );
                MenuItem::new(fixed.label(), move |s: &mut Self, ctx: &mut GameContext| {
                    s.run_on_drive(ctx, |drive, ctx| fixed.run(drive, ctx));
                })
                .help(fixed.key_name().map_or_else(
                    || TILT_LEVEL_HELP.to_string(),
                    |key| format!("On the keyboard: {key}."),
                ))
                .touch_help(touch)
            }
            Entry::Value(value) => value_item(ctx, value),
            Open(group) => MenuItem::new(group.label(), move |s: &mut Self, ctx| {
                s.show(ctx, Some(group), 0);
            }),
            Entry::Back => MenuItem::new("Back to driving", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Close the list without running a command."),
        }
    }
}

fn action_item(ctx: &GameContext, action: Action) -> MenuItem<DrivingCommandsState> {
    let mut help = format!("On the keyboard: {}.", ctx.bindings.spoken(action));
    if action.on_pad() {
        let pad_spoken = ctx.pad_name(action);
        help.push_str(&format!(" On the controller: {pad_spoken}."));
    }
    // The Status menu's gesture is this list, so its row has no help here.
    let touch = ctx
        .bindings
        .touch_gesture_for(action)
        .filter(|_| action != Action::Status)
        .map_or(String::new(), |noun| format!("At the wheel: {noun}."));
    MenuItem::new(
        action.label(),
        move |s: &mut DrivingCommandsState, ctx: &mut GameContext| {
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
    .touch_help(touch)
}

fn value_item(ctx: &GameContext, value: Value) -> MenuItem<DrivingCommandsState> {
    let mut help = value.keyboard_help().to_string();
    let (up, down) = value.actions();
    let (pad_spoken, pad_spoken2) = (ctx.pad_name(up), ctx.pad_name(down));
    help.push_str(&format!(
        " On the controller: {pad_spoken} and {pad_spoken2}."
    ));
    let label = Label::dynamic(
        move |s: &DrivingCommandsState, ctx: &GameContext| match value {
            Value::RadioVolume => {
                let pct = ff_core::pyfmt::round_py(ctx.settings.radio_volume * 100.0) as i64;
                format!("Radio volume: {pct} percent")
            }
            Value::RadioCategory => s
                .driving
                .read(|drive| {
                    if !drive.radio.enabled || !drive.trip.truck.engine_on {
                        return None;
                    }
                    let station = drive.radio.tuned_station();
                    let group = drive.radio.group(&station);
                    Some(ff_core::radio::dial_category_name(group))
                })
                .flatten()
                .map_or("Radio category".to_string(), |name| {
                    format!("Radio category: {name}")
                }),
        },
    );
    // Enter reads the row again; the list stays open.
    MenuItem::new(label, |s: &mut DrivingCommandsState, ctx| {
        s.speak_current(ctx)
    })
    .help(help)
    .touch_help("Flick up or down to change it.")
    .select_sound(None)
}

impl Menu for DrivingCommandsState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let tilt = crate::app::tilt::wanted(&ctx.settings);
        self.entries()
            .iter()
            .filter(|entry| tilt || **entry != F(Fixed::SetTiltLevel))
            .map(|entry| self.item(ctx, *entry))
            .collect()
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let direction = match event.key_down() {
            Some((Key::Right, _, _)) => 1,
            Some((Key::Left, _, _)) => -1,
            _ => 0,
        };
        if direction != 0 && self.adjust_value(ctx, direction) {
            return;
        }
        default_handle_event(self, ctx, event);
    }

    /// The controller's left and right on a value row.
    fn adjust(&mut self, ctx: &mut GameContext, direction: i64) {
        self.adjust_value(ctx, direction);
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/menu_back");
        if let Some(group) = self.group {
            let row = TOP
                .iter()
                .position(|entry| *entry == Open(group))
                .unwrap_or(0);
            self.show(ctx, None, row);
            return;
        }
        ctx.pop_state();
        ctx.say_with("Back to driving.", Say::queued());
    }
}

impl_state_for_menu!(DrivingCommandsState);

#[cfg(test)]
mod tests {
    use super::*;

    /// The actions the list must carry: every one-shot control the
    /// keyboard has.
    fn offers(action: Action) -> bool {
        action.on_keyboard() && !action.held()
    }

    /// Every row, top list and groups together.
    fn every_entry() -> Vec<Entry> {
        let groups = TOP.iter().filter_map(|entry| match entry {
            Open(group) => Some(group.entries().iter().copied()),
            _ => None,
        });
        TOP.iter().copied().chain(groups.flatten()).collect()
    }

    #[test]
    fn the_status_menu_is_the_first_row() {
        assert_eq!(TOP.first(), Some(&Run(Action::Status)));
        assert_eq!(TOP.last(), Some(&Entry::Back));
    }

    #[test]
    fn every_offered_action_sits_in_exactly_one_place() {
        let entries = every_entry();
        for action in Action::all().filter(|action| offers(*action)) {
            let places = entries
                .iter()
                .filter(|entry| **entry == Run(action))
                .count();
            assert_eq!(places, 1, "{action:?} is listed {places} times");
        }
        // And nothing listed is a control the list does not offer.
        for entry in &entries {
            if let Run(action) = entry {
                assert!(offers(*action), "{action:?} is listed but not offered");
            }
        }
    }

    #[test]
    fn every_group_is_on_the_top_list_once() {
        for group in [
            Group::Speed,
            Group::Road,
            Group::Place,
            Group::Truck,
            Group::Gears,
            Group::Radio,
            Group::Hours,
        ] {
            assert_eq!(
                TOP.iter().filter(|entry| **entry == Open(group)).count(),
                1,
                "{group:?}"
            );
            assert!(!group.entries().is_empty(), "{group:?}");
        }
    }

    #[test]
    fn the_fixed_rows_and_value_rows_are_listed_once() {
        let entries = every_entry();
        for fixed in [
            Fixed::CruiseUp,
            Fixed::CruiseDown,
            Fixed::NextStation,
            Fixed::PreviousStation,
            Fixed::Help,
            Fixed::SetTiltLevel,
        ] {
            assert_eq!(entries.iter().filter(|e| **e == F(fixed)).count(), 1);
        }
        for value in [Value::RadioVolume, Value::RadioCategory] {
            assert_eq!(
                entries
                    .iter()
                    .filter(|e| **e == Entry::Value(value))
                    .count(),
                1
            );
        }
    }
}
