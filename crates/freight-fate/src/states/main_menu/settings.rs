//! The settings screens: the category picker, the Gameplay parent, and the
//! per-category lists (the `SettingsState`, `GameplaySettingsState` and
//! `SettingsCategoryState` classes of `main_menu.py`). The rows themselves
//! are built in `settings_items`; the toggles they drive are in
//! `settings_actions`.

use std::rc::Rc;

use ff_core::settings::{lane_keeping_to_legacy, Settings};

use crate::app::{active_log_path, GameContext, Say};
use crate::impl_state_for_menu;
use crate::states::base::{InputEvent, Key, Menu, MenuCore, MenuItem};
use crate::states::online_hub::OnlineHubState;

/// Top-level settings: a category picker that opens per-category submenus.
///
/// A tabbed multi-page layout reads poorly without a screen to show the
/// tabs, so each category is its own spoken submenu instead, matching every
/// other menu's navigation model.
pub struct SettingsState {
    menu: MenuCore<Self>,
}

impl SettingsState {
    // Gameplay is now a category with its own submenu (Driving assistance,
    // Difficulty and hours of service, World and traffic, and Controls) rather
    // than a flat list -- it opens GameplaySettingsState. The rest are plain
    // category lists.
    pub const CATEGORIES: [(&'static str, &'static str); 5] = [
        ("Gameplay", "gameplay"),
        ("Audio", "audio"),
        ("Speech", "speech"),
        ("Updates", "updates"),
        ("Problem reports", "reports"),
    ];

    /// Categories with nothing to do on iPhone and iPad: TestFlight and the
    /// App Store do the updating, and the log sits where the player cannot
    /// open it.
    const HIDDEN_ON_IOS: [&'static str; 2] = ["updates", "reports"];

    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Settings")
                .with_intro_help("Up and Down pick a category, Enter opens it, Escape goes back."),
        }
    }

    fn open(&mut self, ctx: &mut GameContext, category: &str) {
        if category == "gameplay" {
            ctx.push_state(GameplaySettingsState::new());
            return;
        }
        ctx.push_state(SettingsCategoryState::new(category));
    }

    fn open_online_hub(&mut self, ctx: &mut GameContext) {
        let hub = OnlineHubState::new(ctx);
        ctx.push_state(hub);
    }
}

impl Default for SettingsState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for SettingsState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Self::CATEGORIES
            .iter()
            .filter(|(_, key)| !Self::HIDDEN_ON_IOS.contains(key) || !cfg!(target_os = "ios"))
            // AG Now updates the game itself, so the update channel and the
            // manual check have nothing to do there either.
            .filter(|(_, key)| *key != "updates" || crate::updater::self_updates())
            .map(|(label, key)| {
                MenuItem::new(*label, move |s: &mut Self, ctx| s.open(ctx, key))
                    .help(format!("Open {} settings.", label.to_lowercase()))
            })
            .collect();
        // The Online items moved to the main menu; this stays in the old spot
        // (after Speech) for a release or two so muscle memory still lands
        // somewhere useful.
        items.insert(
            3,
            MenuItem::new("Online", |s: &mut Self, ctx| s.open_online_hub(ctx)).help(
                "Online options have moved to the Online menu on the \
                 main menu; this opens it.",
            ),
        );
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        save_settings(&ctx.settings);
        ctx.audio.play("ui/menu_back");
        // Popping first lets the revealed menu's own entry announcement play,
        // then "Settings saved." interrupts and wins -- said after the pop,
        // not before it, so it is not the one left cancelled by the other.
        ctx.pop_state();
        ctx.say("Settings saved.");
    }
}

impl_state_for_menu!(SettingsState);

pub(super) fn save_settings(settings: &Settings) {
    if let Err(e) = settings.save() {
        log::warn!("Could not save settings: {e}");
    }
}

// Spoken once to a player whose settings predate a layout, the first time they
// open the Gameplay submenu, one entry per settings layout version they missed.
// A blind player cannot see a menu change shape, so the words carry the whole
// change -- and the reassurance that nothing about their actual settings moved,
// which is true by construction: every move keeps the saved key it always had.
pub const SETTINGS_LAYOUT_NOTICES: [(i64, &str); 3] = [
    (
        1,
        "Gameplay is now a category with its own submenu: Driving assistance, \
         Difficulty and hours of service, World and traffic, and Controls. \
         Weather, traffic, and parking sources moved into World and traffic \
         from Speech and weather. Nothing about your settings changed.",
    ),
    (
        2,
        "Two rows moved. Speed keeper is now in Driving assistance instead of \
         Controls. Lane and edge cue prominence is now Lane and edge cue \
         volume, in Audio under Gameplay cues volume. Your choices came with \
         them. Overspeed warning no longer has a row: it stays quiet until you \
         are heading for a ticket.",
    ),
    (
        3,
        "Speech verbosity is now Driving speech, in the Speech category, with \
         two more steps. Normal is now standard and terse is now quiet; your \
         choice came with you. Both still speak every safety call, route \
         instruction, and money consequence. Quiet keeps short confirmations \
         and status updates, including lane openings. Urgent only speaks \
         safety warnings and directions requiring action; road heads-ups and \
         confirmations become sounds, while routine costs and status are silent. \
         Events silenced by your setting stay out of message review.",
    ),
];

/// The Gameplay parent: a submenu of submenus.
///
/// Gameplay used to be one flat list long enough to lose things in. It is now
/// a category that opens four smaller lists, each its own spoken screen, using
/// the same per-category machinery as every other settings screen.
pub struct GameplaySettingsState {
    menu: MenuCore<Self>,
}

impl GameplaySettingsState {
    pub const SUBCATEGORIES: [(&'static str, &'static str); 4] = [
        ("Driving assistance", "assistance"),
        ("Difficulty and hours of service", "difficulty"),
        ("World and traffic", "world"),
        ("Controls", "controls"),
    ];

    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Gameplay").with_intro_help(
                "Four screens. Up and Down pick one, Enter opens it, Escape goes back.",
            ),
        }
    }
}

impl Default for GameplaySettingsState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for GameplaySettingsState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Self::SUBCATEGORIES
            .iter()
            .map(|(label, key)| {
                MenuItem::new(*label, move |_s: &mut Self, ctx| {
                    ctx.push_state(SettingsCategoryState::new(key))
                })
                .help(format!("Open {} settings.", label.to_lowercase()))
            })
            .collect();
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        base_announce_entry(self, ctx);
        let s = &mut ctx.settings;
        if s.settings_layout_notice_from >= 0 {
            // Said once and only to a player whose settings moved under them,
            // oldest layout first so two moves arrive in the order they
            // happened. Queued behind the menu's own entry line rather than
            // interrupting it, and cleared to disk immediately so a mid-notice
            // quit does not replay it forever -- but a player who never reaches
            // this screen keeps the flag, and hears it whenever they arrive.
            let from = s.settings_layout_notice_from;
            let owed: Vec<&str> = SETTINGS_LAYOUT_NOTICES
                .iter()
                .filter(|(version, _)| *version > from)
                .map(|(_, text)| *text)
                .collect();
            s.settings_layout_notice_from = -1;
            save_settings(s);
            for text in owed {
                ctx.say_with(text, Say::queued().review(false));
            }
        }
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        save_settings(&ctx.settings);
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
    }
}

impl_state_for_menu!(GameplaySettingsState);

/// The base `Menu::announce_entry`, for overrides that add to it.
// TODO(lead): the provided `Menu` methods cannot be called once overridden;
// a callable default (free fn) on the base would retire these copies.
pub(super) fn base_announce_entry<S: Menu>(s: &mut S, ctx: &mut GameContext) {
    let title = crate::states::base::end_sentence(&s.menu().title);
    ctx.say(&title);
    let current = s.current_text(ctx);
    ctx.say_with(current, Say::queued().review(false));
}

/// The base `Menu::handle_event`, for an override that handles a key or two
/// and falls through for the rest.
pub(super) fn base_handle_event<S: Menu>(s: &mut S, ctx: &mut GameContext, event: &InputEvent) {
    let Some((key, _mods, text)) = event.key_down() else {
        return;
    };
    match key {
        Key::Down => s.move_by(ctx, 1),
        Key::Up => s.move_by(ctx, -1),
        Key::Home => s.jump(ctx, 0),
        Key::End => {
            let last = s.menu().items.len().saturating_sub(1);
            s.jump(ctx, last);
        }
        Key::Return | Key::Space | Key::KpEnter => s.activate(ctx),
        Key::Escape => s.go_back(ctx),
        Key::F1 => {
            let help = s.current_help(ctx);
            ctx.say(&help);
        }
        Key::LCtrl | Key::RCtrl => ctx.stop_speech(),
        _ => {
            if let Some(ch) = text.filter(|ch| ch.is_alphanumeric()) {
                let lower: String = ch.to_lowercase().collect();
                s.first_letter_jump(ctx, &lower);
            }
        }
    }
}

/// What Left/Right (or D-pad left/right) does on one settings row.
pub(super) type Adjust = Rc<dyn Fn(&mut SettingsCategoryState, &mut GameContext, i64)>;

/// One category of settings as a spoken list.
///
/// Up and down pick a setting; Right arrow or Enter changes it forward and
/// Left changes it backward (the same per-item adjust model as the old tabbed
/// screen, minus the tab switching). Escape returns to the settings
/// categories, and each change is saved as it is made.
pub struct SettingsCategoryState {
    menu: MenuCore<Self>,
    pub category: String,
}

impl SettingsCategoryState {
    pub const TITLES: [(&'static str, &'static str); 8] = [
        ("assistance", "Driving assistance"),
        ("difficulty", "Difficulty and hours of service"),
        ("world", "World and traffic"),
        ("controls", "Controls"),
        ("audio", "Audio"),
        ("speech", "Speech"),
        ("updates", "Updates"),
        ("reports", "Problem reports"),
    ];

    pub fn new(category: &str) -> Self {
        let title = Self::TITLES
            .iter()
            .find(|(key, _)| *key == category)
            .map(|(_, title)| *title)
            .unwrap_or("Settings");
        Self {
            menu: MenuCore::new(title).with_intro_help(
                "Up and Down pick a setting. Right arrow or Enter changes it forward, \
                 Left arrow backward. Escape goes back.",
            ),
            category: category.to_string(),
        }
    }

    pub fn title(&self) -> &str {
        &self.menu.title
    }

    /// `_adjust`: step the current row by `direction`.
    pub fn adjust_by(&mut self, ctx: &mut GameContext, direction: i64) {
        let actions = self.adjust_actions(ctx);
        let index = self.menu.index;
        if let Some(action) = actions.get(index) {
            let action = Rc::clone(action);
            action(self, ctx, direction);
        }
    }

    /// Tell a player who was on Realistic why their row now says Standard.
    ///
    /// Their game clock runs at half the rate they set it to, so a driving
    /// day now takes twice the real time it used to. Nothing else on the
    /// row would say so: the label reads Standard as though they had
    /// chosen it. Same budget and same queueing as the lane-keeping rename
    /// -- it follows the row announcement rather than interrupting it, so a
    /// player arrowing straight past loses it and hears it next visit.
    fn maybe_say_pace_retired(&mut self, ctx: &mut GameContext) {
        if self.category != "difficulty" || ctx.settings.pace_retired_notice_left <= 0 {
            return;
        }
        let on_row = self
            .menu
            .items
            .get(self.menu.index)
            .is_some_and(|item| item.text(self, ctx).starts_with("Driving mode"));
        if !on_row {
            return;
        }
        ctx.settings.pace_retired_notice_left -= 1;
        save_settings(&ctx.settings);
        ctx.say_with(
            "This row used to offer Realistic, and yours was set to it. \
             That setting is retired: it was the fastest here, not the most \
             true to life. You are on Standard now, so the game clock runs \
             at half the speed it did.",
            Say::queued(),
        );
    }

    /// Tell a returning player their Lane drift row is now Lane keeping.
    ///
    /// Only the real control says it, and only for a player whose settings
    /// file actually carried the old name. It queues behind the row
    /// announcement rather than interrupting it, which means a player who
    /// keeps arrowing loses it -- so the budget is three, and a lost
    /// announcement corrects itself on the next visit.
    fn maybe_say_lane_keeping_rename(&mut self, ctx: &mut GameContext) {
        if self.category != "assistance" || ctx.settings.lane_keeping_rename_notice_left <= 0 {
            return;
        }
        let on_row = self
            .menu
            .items
            .get(self.menu.index)
            .is_some_and(|item| item.text(self, ctx).starts_with("Lane keeping"));
        if !on_row {
            return;
        }
        ctx.settings.lane_keeping_rename_notice_left -= 1;
        save_settings(&ctx.settings);
        let was = lane_keeping_to_legacy(&ctx.settings.lane_keeping);
        let unchanged = match ctx.settings.lane_keeping.as_str() {
            "full" => "the truck still holds the lane and takes your exits",
            "partial" => "the drift and the steering help are just as they were",
            "off" => "you still hold the lane and take your own exits",
            _ => "the truck still holds the lane and takes your exits",
        };
        ctx.say_with(
            format!(
                "This row used to be Lane drift, and yours read {was}. \
                 Nothing about your driving changed: {unchanged}."
            ),
            Say::queued(),
        );
    }

    /// Rebuild, save, click, speak the row (`_announce`).
    pub(super) fn announce(&mut self, ctx: &mut GameContext) {
        self.refresh(ctx, true);
        save_settings(&ctx.settings);
        ctx.audio.play("ui/menu_select");
        self.speak_current(ctx);
    }

    pub(super) fn announce_speech_preview(&mut self, ctx: &mut GameContext, setting: &str) {
        self.refresh(ctx, true);
        save_settings(&ctx.settings);
        ctx.audio.play("ui/menu_select");
        let text = self.current_text(ctx);
        let heard = ctx.tr(&text);
        if !ctx.speech.say_adjustment_preview(setting, &heard, true) {
            ctx.say(&text);
        }
    }

    /// Where this session's log is, in words a player can act on.
    ///
    /// The log already records every spoken line, so it is the most useful
    /// thing a player can attach to a bug report -- but nothing in the game
    /// ever mentioned it, so nobody sent one. Read from the path logging
    /// actually opened, so a folder the game could not write to reports
    /// honestly instead of naming a file that is not there.
    pub fn log_location_lines(&self) -> Vec<String> {
        let Some(path) = active_log_path() else {
            return vec!["This copy is not writing a log file. Packaged downloads \
                 always write one; a copy run from source prints to its console \
                 instead."
                .to_string()];
        };
        let mut out = vec![
            format!("The game log is saved as {}.", path.display()),
            "It records this session, including everything the game said out loud. \
             Attach it to a bug report."
                .to_string(),
        ];
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let suffix = path
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let previous_name = format!("{stem}.prev{suffix}");
        let previous = path.with_file_name(&previous_name);
        if previous.exists() {
            out.push(format!(
                "The session before this one is kept beside it as {previous_name}."
            ));
        }
        out.push(
            "Both files stay on this computer; the game never sends them anywhere.".to_string(),
        );
        out
    }

    pub(super) fn say_log_location(&mut self, ctx: &mut GameContext) {
        ctx.say(&self.log_location_lines().join(" "));
    }
}

impl Menu for SettingsCategoryState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        self.category_items(ctx)
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        match event.key_down() {
            Some((Key::Right, _, _)) => self.adjust_by(ctx, 1),
            Some((Key::Left, _, _)) => self.adjust_by(ctx, -1),
            _ => base_handle_event(self, ctx, event),
        }
    }

    /// D-pad left/right on a controller maps to the same per-item adjust.
    fn adjust(&mut self, ctx: &mut GameContext, direction: i64) {
        self.adjust_by(ctx, direction);
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        // Entry speaks the landing row through ``ctx.say`` rather than
        // ``speak_current``, so a row-specific notice attached only to the
        // latter is never heard by a player whose row is the one they land
        // on -- which is every player for Driving mode, the first row of
        // Difficulty and hours of service. Both notices are guarded by their
        // own counter, so running them here costs nothing on a visit that
        // arrows in from elsewhere.
        base_announce_entry(self, ctx);
        self.maybe_say_lane_keeping_rename(ctx);
        self.maybe_say_pace_retired(ctx);
    }

    fn speak_current(&mut self, ctx: &mut GameContext) {
        let text = self.current_text(ctx);
        ctx.say_with(text, Say::new().review(false));
        self.maybe_say_lane_keeping_rename(ctx);
        self.maybe_say_pace_retired(ctx);
    }

    fn lines(&self, ctx: &GameContext) -> Vec<String> {
        // Spoken-only information would be invisible to a low-vision player
        // reading the window, so the log's location is mirrored on screen.
        let core = self.menu();
        let mut out = vec![core.title.clone(), String::new()];
        for (i, item) in core.items.iter().enumerate() {
            let marker = if i == core.index { "> " } else { "  " };
            out.push(format!("{marker}{}", item.text(self, ctx)));
        }
        if self.category == "reports" {
            out.push(String::new());
            out.extend(self.log_location_lines());
        }
        out
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        // Settings are saved as each change is made; just return to the
        // category list (the top-level picker says "Settings saved" on exit).
        save_settings(&ctx.settings);
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
    }
}

impl_state_for_menu!(SettingsCategoryState);
