//! The one-time Driving assistance picker: the first screen of a launch
//! until the player has answered it (owner, 2026-09-30).
//!
//! Too many first drives went wrong at the wheel, street corners above all,
//! and a player who hit that on Balanced was never told the choice existed.
//! So everyone answers it once, a fresh install and an existing player
//! alike, before the main menu. The cursor starts on the preset they already
//! have and Escape keeps it; either way `assist_preset_chosen` is spent and
//! the picker never comes back. Changing it later is the Driving assistance
//! preset row, which is the same choice.

use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::states::base::{Label, Menu, MenuCore, MenuItem};
use crate::states::main_menu::MainMenuState;
use crate::states::main_menu_help::render_help_line;

/// `(preset, row, help)`, the recommended one first.
const PRESETS: [(&str, &str, &str); 3] = [
    (
        "all",
        "All assists: the truck steers. Recommended for your first drives.",
        "It holds the lane, steers the bends and street corners, moves out \
         of closed lanes, and takes your exits and open weigh stations. A tap \
         of {{steer_left}} or {{steer_right}} changes lanes. Speed, rest and \
         fuel stops, and the docks are yours.",
    ),
    (
        "balanced",
        "Balanced: you steer, with help.",
        "It eases you through bends and back from a drift. Street corners, \
         lane changes, exit signals and speed are yours. It stops for you at \
         your destination.",
    ),
    (
        "realistic",
        "Realistic: you drive.",
        "A modern truck's safety systems and nothing more. Every bend, corner \
         and exit is yours.",
    ),
];

pub struct AssistPickerState {
    pub menu: MenuCore<Self>,
}

impl Default for AssistPickerState {
    fn default() -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE)
                .with_intro_help("Enter chooses. Escape keeps what you have."),
        }
    }
}

impl AssistPickerState {
    pub const TITLE: &'static str = "Driving assistance";
    pub const QUESTION: &'static str = "How much should the truck do for you? You can \
         change this later in Settings, Gameplay, Driving assistance.";
    pub const KEEP_CUSTOM: &'static str = "Keep my custom assists";

    pub fn new() -> Self {
        Self::default()
    }

    /// Whether this launch opens on the picker.
    pub fn is_owed(ctx: &GameContext) -> bool {
        !ctx.settings.assist_preset_chosen
    }

    /// Apply `preset` (or keep what is set), spend the picker, and go on to
    /// the main menu. No confirmation line: the player just chose it, and
    /// the main menu announces itself next.
    fn finish(&mut self, ctx: &mut GameContext, preset: Option<&str>) {
        if let Some(preset) = preset {
            ctx.settings.apply_driving_assistance_preset(preset);
        }
        ctx.settings.assist_preset_chosen = true;
        if let Err(e) = ctx.settings.save() {
            // The worst case is answering it again next launch.
            log::warn!("Could not save settings: {e}");
        }
        ctx.reset_to(MainMenuState::new());
    }
}

impl Menu for AssistPickerState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        ctx.say(&format!("{}. {}", Self::TITLE, Self::QUESTION));
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let current = ctx.settings.driving_assistance_preset.clone();
        let mut items = Vec::new();
        // A player who set assists one by one keeps them unless they choose
        // otherwise, so that answer is first and the cursor starts there.
        if current == "custom" {
            items.push(
                MenuItem::new(Self::KEEP_CUSTOM, |s: &mut Self, ctx| s.finish(ctx, None))
                    .help("Keeps the assists you set one by one."),
            );
        }
        for (preset, row, help) in PRESETS {
            items.push(
                MenuItem::new(row, move |s: &mut Self, ctx| s.finish(ctx, Some(preset)))
                    .help(Label::dynamic(move |_s, ctx| render_help_line(ctx, help))),
            );
        }
        self.menu.index = PRESETS
            .iter()
            .position(|(preset, ..)| *preset == current)
            .unwrap_or(0);
        items
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        // Escape keeps what is set. Never stuck here, and never asked twice.
        self.finish(ctx, None);
    }
}

impl_state_for_menu!(AssistPickerState);
