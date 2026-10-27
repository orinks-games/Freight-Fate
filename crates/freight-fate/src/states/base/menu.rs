//! The accessible menu base: [`MenuCore`] + the [`Menu`] trait and its
//! rows ([`MenuItem`], [`Label`]), plus `impl_state_for_menu!` and the
//! ready-made [`SimpleMenuState`]. Split out of `states::base` for length;
//! everything here is re-exported from there.

use std::rc::Rc;

use ff_core::radio::RadioState;

use crate::app::{GameContext, Say};
use crate::controller::ControllerAction;
use crate::discord_presence::PresenceState;
use crate::states::base::{end_sentence, CopyFocus, InputEvent, Key};

// -- menus -------------------------------------------------------------------------------

/// A menu row's label or help: a fixed string, or computed while the menu is
/// open. Callable for the same reason the Python field was: a row whose help
/// depends on state that changes while the menu is open (a waiting cloud
/// conflict, say) must not read out an answer that was true when the screen
/// was built. Read it through [`Label::text`], never off the field.
pub enum Label<S> {
    Text(String),
    Dynamic(DynamicLabel<S>),
}

/// A label computed while the menu is open.
pub type DynamicLabel<S> = Rc<dyn Fn(&S, &GameContext) -> String>;

impl<S> Label<S> {
    pub fn dynamic(f: impl Fn(&S, &GameContext) -> String + 'static) -> Self {
        Label::Dynamic(Rc::new(f))
    }

    pub fn text(&self, state: &S, ctx: &GameContext) -> String {
        match self {
            Label::Text(text) => text.clone(),
            Label::Dynamic(f) => f(state, ctx),
        }
    }
}

impl<S> Clone for Label<S> {
    fn clone(&self) -> Self {
        match self {
            Label::Text(text) => Label::Text(text.clone()),
            Label::Dynamic(f) => Label::Dynamic(Rc::clone(f)),
        }
    }
}

impl<S> From<&str> for Label<S> {
    fn from(text: &str) -> Self {
        Label::Text(text.to_string())
    }
}

impl<S> From<String> for Label<S> {
    fn from(text: String) -> Self {
        Label::Text(text)
    }
}

/// What activating a row does: the Python `action` callable, given the
/// screen and the context.
pub type MenuAction<S> = Rc<dyn Fn(&mut S, &mut GameContext)>;

/// One menu row (`MenuItem` dataclass).
pub struct MenuItem<S> {
    pub label: Label<S>,
    pub action: MenuAction<S>,
    pub help: Label<S>,
    /// The click played when this item is activated. `None` for items whose
    /// action plays its own confirmation sound, so the two do not stack.
    pub select_sound: Option<String>,
}

impl<S> Clone for MenuItem<S> {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            action: Rc::clone(&self.action),
            help: self.help.clone(),
            select_sound: self.select_sound.clone(),
        }
    }
}

impl<S: 'static> MenuItem<S> {
    /// `MenuItem(label, action)` with the default help and select sound.
    pub fn new(
        label: impl Into<Label<S>>,
        action: impl Fn(&mut S, &mut GameContext) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            action: Rc::new(action),
            help: Label::Text(String::new()),
            select_sound: Some("ui/menu_select".to_string()),
        }
    }

    /// A row that does nothing when activated (an information line).
    pub fn inert(label: impl Into<Label<S>>) -> Self {
        Self::new(label, |_, _| {})
    }

    pub fn help(mut self, help: impl Into<Label<S>>) -> Self {
        self.help = help.into();
        self
    }

    /// `select_sound=None` / another key.
    pub fn select_sound(mut self, key: Option<&str>) -> Self {
        self.select_sound = key.map(str::to_string);
        self
    }

    pub fn text(&self, state: &S, ctx: &GameContext) -> String {
        self.label.text(state, ctx)
    }

    pub fn help_text(&self, state: &S, ctx: &GameContext) -> String {
        self.help.text(state, ctx)
    }
}

/// The reusable state of a `MenuState`: the Python class attributes
/// (`title`, `intro_help`, `open_sound_key`) and instance fields (`items`,
/// `index`). Embed one in a screen struct and implement [`Menu`].
pub struct MenuCore<S> {
    pub title: String,
    pub intro_help: String,
    /// `None` plays nothing on entry.
    pub open_sound_key: Option<String>,
    pub items: Vec<MenuItem<S>>,
    pub index: usize,
}

pub const DEFAULT_INTRO_HELP: &str =
    "Enter selects, Escape goes back. Control stops the current speech.";

impl<S> MenuCore<S> {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            intro_help: DEFAULT_INTRO_HELP.to_string(),
            open_sound_key: Some("ui/menu_open".to_string()),
            items: Vec::new(),
            index: 0,
        }
    }

    pub fn with_intro_help(mut self, help: &str) -> Self {
        self.intro_help = help.to_string();
        self
    }

    pub fn with_open_sound(mut self, key: Option<&str>) -> Self {
        self.open_sound_key = key.map(str::to_string);
        self
    }

    fn clamp_index(&mut self) {
        self.index = self.index.min(self.items.len().saturating_sub(1));
    }
}

impl<S> Default for MenuCore<S> {
    fn default() -> Self {
        Self::new("Menu")
    }
}

/// A vertically navigated, fully spoken menu.
///
/// Implementors provide the two accessors and `build_items`; everything
/// else is the Python `MenuState` behaviour as provided methods, each
/// overridable. The [`State`] hooks (`enter`, `update`, `lines`, ...) are
/// here too, under the same names, so `impl_state_for_menu!` can forward a
/// screen's whole `State` impl to them -- a screen that needs a custom
/// `update` overrides `Menu::update` and the forwarding picks it up.
///
/// ```ignore
/// pub struct ConfirmQuitState { menu: MenuCore<Self> }
///
/// impl ConfirmQuitState {
///     pub fn new() -> Self { Self { menu: MenuCore::new("Quit Freight Fate?") } }
/// }
///
/// impl Menu for ConfirmQuitState {
///     fn menu(&self) -> &MenuCore<Self> { &self.menu }
///     fn menu_mut(&mut self) -> &mut MenuCore<Self> { &mut self.menu }
///     fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
///         vec![
///             MenuItem::new("Yes, quit", |_s, ctx| ctx.quit()).help("Closes the game."),
///             MenuItem::new("No, go back", |s, ctx| s.go_back(ctx)),
///         ]
///     }
/// }
///
/// impl_state_for_menu!(ConfirmQuitState);
/// ```
pub trait Menu: Sized + 'static {
    fn menu(&self) -> &MenuCore<Self>;
    fn menu_mut(&mut self) -> &mut MenuCore<Self>;
    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>>;

    // -- State hooks, forwarded by impl_state_for_menu! ----------------------------

    fn paces_main_speech(&self) -> bool {
        false
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        let items = self.build_items(ctx);
        let core = self.menu_mut();
        core.items = items;
        core.clamp_index();
        if let Some(key) = self.menu().open_sound_key.clone() {
            ctx.audio.play(&key);
        }
        self.announce_entry(ctx);
    }

    fn exit(&mut self, _ctx: &mut GameContext) {}

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
    }

    fn on_controller_disconnect(&mut self, _ctx: &mut GameContext) {}

    fn presence(&self, _ctx: &GameContext) -> Option<PresenceState> {
        None
    }

    fn online_presence(&self, _ctx: &GameContext) -> Option<PresenceState> {
        None
    }

    fn captures_text_input(&self) -> bool {
        false
    }

    /// Menus opt into held D-pad left/right auto-repeat for adjusting options.
    fn wants_controller_repeat(&self) -> bool {
        true
    }

    fn ticks_covered_music(&self) -> bool {
        false
    }

    fn tick_covered_music(&mut self, _ctx: &mut GameContext, _dt: f64) {}

    fn applies_radio_settings(&self) -> bool {
        false
    }

    fn apply_radio_settings_now(&mut self, _ctx: &mut GameContext) {}

    fn radio(&self) -> Option<&RadioState> {
        None
    }

    /// A hook for keys a menu handles itself (PageUp and PageDown on a paged
    /// list). Return true when the key was used.
    fn handle_extra_key(&mut self, _ctx: &mut GameContext, _key: Key) -> bool {
        false
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        let Some((key, _mods, text)) = event.key_down() else {
            return;
        };
        if event.key_repeat() && self.held_key_moves_nothing(key) {
            return;
        }
        if self.handle_extra_key(ctx, key) {
            return;
        }
        match key {
            Key::Down => self.move_by(ctx, 1),
            Key::Up => self.move_by(ctx, -1),
            Key::Home => self.jump(ctx, 0),
            Key::End => {
                let last = self.menu().items.len().saturating_sub(1);
                self.jump(ctx, last);
            }
            Key::Return | Key::Space | Key::KpEnter => self.activate(ctx),
            Key::Escape => self.go_back(ctx),
            Key::F1 => {
                let help = self.current_help(ctx);
                ctx.say(&help);
            }
            Key::LCtrl | Key::RCtrl => ctx.stop_speech(),
            _ => {
                if let Some(ch) = text.filter(|ch| ch.is_alphanumeric()) {
                    let lower: String = ch.to_lowercase().collect();
                    self.first_letter_input(ctx, &lower, event.key_repeat());
                }
            }
        }
    }

    fn handle_controller(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        match ctx.controller.menu_action(event) {
            Some(ControllerAction::MenuDown) => self.move_by(ctx, 1),
            Some(ControllerAction::MenuUp) => self.move_by(ctx, -1),
            Some(ControllerAction::AdjustRight) => self.adjust(ctx, 1),
            Some(ControllerAction::AdjustLeft) => self.adjust(ctx, -1),
            Some(ControllerAction::Confirm) => self.activate(ctx),
            Some(ControllerAction::Back) => self.go_back(ctx),
            Some(ControllerAction::Help) => {
                let help = self.current_help(ctx);
                ctx.say(&help);
            }
            None => {}
        }
    }

    fn lines(&self, ctx: &GameContext) -> Vec<String> {
        let core = self.menu();
        let mut out = vec![core.title.clone(), String::new()];
        for (i, item) in core.items.iter().enumerate() {
            let marker = if i == core.index { "> " } else { "  " };
            out.push(format!("{marker}{}", item.text(self, ctx)));
        }
        out
    }

    // -- menu behaviour -------------------------------------------------------------

    /// The focused row's label and index.
    fn copy_focus(&self, ctx: &GameContext) -> Option<CopyFocus> {
        let core = self.menu();
        let item = core.items.get(core.index)?;
        Some(CopyFocus {
            index: core.index,
            row: end_sentence(&item.text(self, ctx)),
        })
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let title = end_sentence(&self.menu().title);
        ctx.say(&title);
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }

    fn refresh(&mut self, ctx: &mut GameContext, keep_index: bool) {
        let old = self.menu().index;
        let items = self.build_items(ctx);
        let core = self.menu_mut();
        core.items = items;
        core.index = if keep_index { old } else { 0 };
        core.clamp_index();
    }

    fn current_text(&self, ctx: &GameContext) -> String {
        let core = self.menu();
        if core.items.is_empty() {
            return "No options available.".to_string();
        }
        let label = end_sentence(&core.items[core.index].text(self, ctx));
        if !ctx.settings.announce_menu_position {
            return label;
        }
        // The cue is its own catalog line (`ff_core::l10n::POSITION_CUE`),
        // translated apart from the row so it never depends on the row.
        let cue = format!(
            "{position} of {count}.",
            position = core.index + 1,
            count = core.items.len()
        );
        format!("{label} {cue}")
    }

    fn speak_current(&mut self, ctx: &mut GameContext) {
        let text = self.current_text(ctx);
        ctx.say_with(text, Say::new().review(false));
    }

    fn current_help(&self, ctx: &GameContext) -> String {
        let core = self.menu();
        if core.items.is_empty() {
            return core.intro_help.clone();
        }
        let item = &core.items[core.index];
        let help = item.help_text(self, ctx);
        if help.is_empty() {
            format!("{}.", item.text(self, ctx))
        } else {
            help
        }
    }

    /// Whether an auto-repeat of this navigation key would leave the cursor
    /// where it is.
    ///
    /// A key held down when a menu opens keeps auto-repeating into it. A
    /// tester braking onto the shoulder for a scale-bypass stop was still
    /// holding the brake when the one-row stop screen opened, and the
    /// keyboard's repeat read "Pull back onto the highway." twenty times in
    /// six tenths of a second, each one cutting off the last (log,
    /// 2026-10-07). A repeat that cannot move the cursor says nothing; a
    /// fresh press still reads the row.
    fn held_key_moves_nothing(&self, key: Key) -> bool {
        let core = self.menu();
        let count = core.items.len();
        match key {
            Key::Up | Key::Down => count <= 1,
            Key::Home => core.index == 0,
            Key::End => core.index + 1 >= count,
            _ => false,
        }
    }

    fn move_by(&mut self, ctx: &mut GameContext, delta: i64) {
        let core = self.menu_mut();
        if core.items.is_empty() {
            return;
        }
        let n = core.items.len() as i64;
        core.index = ((core.index as i64 + delta).rem_euclid(n)) as usize;
        ctx.audio.play("ui/menu_move");
        self.speak_current(ctx);
    }

    fn jump(&mut self, ctx: &mut GameContext, index: usize) {
        let core = self.menu_mut();
        if core.items.is_empty() {
            return;
        }
        core.index = index.min(core.items.len() - 1);
        ctx.audio.play("ui/menu_move");
        self.speak_current(ctx);
    }

    fn activate(&mut self, ctx: &mut GameContext) {
        let core = self.menu();
        if core.items.is_empty() {
            return;
        }
        let item = core.items[core.index].clone();
        if let Some(sound) = &item.select_sound {
            ctx.audio.play(sound);
        }
        (item.action)(self, ctx);
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
    }

    /// Change the current option (D-pad left/right). No-op unless the menu
    /// has adjustable options; the settings screens override this.
    fn adjust(&mut self, _ctx: &mut GameContext, _direction: i64) {}

    /// A unique initial may activate; ambiguous initials retain cyclic navigation.
    fn first_letter_input(&mut self, ctx: &mut GameContext, ch: &str, repeat: bool) {
        if repeat && ctx.settings.menu_accelerator_keys {
            return;
        }
        let unique = ctx.settings.menu_accelerator_keys
            && self
                .menu()
                .items
                .iter()
                .filter(|item| item.text(self, ctx).to_lowercase().starts_with(ch))
                .count()
                == 1;
        self.first_letter_jump(ctx, ch);
        if unique {
            self.activate(ctx);
        }
    }

    fn first_letter_jump(&mut self, ctx: &mut GameContext, ch: &str) {
        let n = self.menu().items.len();
        if n == 0 {
            return;
        }
        let start = self.menu().index;
        for offset in 1..=n {
            let i = (start + offset) % n;
            let text = self.menu().items[i].text(self, ctx).to_lowercase();
            if text.starts_with(ch) {
                self.menu_mut().index = i;
                ctx.audio.play("ui/menu_move");
                self.speak_current(ctx);
                return;
            }
        }
    }
}

/// Write the [`State`] impl for a [`Menu`] screen, forwarding every hook to
/// the `Menu` method of the same name (see the example on [`Menu`]).
#[macro_export]
macro_rules! impl_state_for_menu {
    ($ty:ty) => {
        impl $crate::states::base::State for $ty {
            fn paces_main_speech(&self) -> bool {
                <Self as $crate::states::base::Menu>::paces_main_speech(self)
            }
            fn enter(&mut self, ctx: &mut $crate::app::GameContext) {
                <Self as $crate::states::base::Menu>::enter(self, ctx)
            }
            fn exit(&mut self, ctx: &mut $crate::app::GameContext) {
                <Self as $crate::states::base::Menu>::exit(self, ctx)
            }
            fn handle_event(
                &mut self,
                ctx: &mut $crate::app::GameContext,
                event: &$crate::states::base::InputEvent,
            ) {
                <Self as $crate::states::base::Menu>::handle_event(self, ctx, event)
            }
            fn handle_controller(
                &mut self,
                ctx: &mut $crate::app::GameContext,
                event: &$crate::states::base::InputEvent,
            ) {
                <Self as $crate::states::base::Menu>::handle_controller(self, ctx, event)
            }
            fn on_controller_disconnect(&mut self, ctx: &mut $crate::app::GameContext) {
                <Self as $crate::states::base::Menu>::on_controller_disconnect(self, ctx)
            }
            fn update(&mut self, ctx: &mut $crate::app::GameContext, dt: f64) {
                <Self as $crate::states::base::Menu>::update(self, ctx, dt)
            }
            fn copy_focus(
                &self,
                ctx: &$crate::app::GameContext,
            ) -> Option<$crate::states::base::CopyFocus> {
                <Self as $crate::states::base::Menu>::copy_focus(self, ctx)
            }
            fn lines(&self, ctx: &$crate::app::GameContext) -> Vec<String> {
                <Self as $crate::states::base::Menu>::lines(self, ctx)
            }
            fn presence(
                &self,
                ctx: &$crate::app::GameContext,
            ) -> Option<$crate::discord_presence::PresenceState> {
                <Self as $crate::states::base::Menu>::presence(self, ctx)
            }
            fn online_presence(
                &self,
                ctx: &$crate::app::GameContext,
            ) -> Option<$crate::discord_presence::PresenceState> {
                <Self as $crate::states::base::Menu>::online_presence(self, ctx)
            }
            fn captures_text_input(&self) -> bool {
                <Self as $crate::states::base::Menu>::captures_text_input(self)
            }
            fn wants_controller_repeat(&self) -> bool {
                <Self as $crate::states::base::Menu>::wants_controller_repeat(self)
            }
            fn ticks_covered_music(&self) -> bool {
                <Self as $crate::states::base::Menu>::ticks_covered_music(self)
            }
            fn tick_covered_music(&mut self, ctx: &mut $crate::app::GameContext, dt: f64) {
                <Self as $crate::states::base::Menu>::tick_covered_music(self, ctx, dt)
            }
            fn applies_radio_settings(&self) -> bool {
                <Self as $crate::states::base::Menu>::applies_radio_settings(self)
            }
            fn apply_radio_settings_now(&mut self, ctx: &mut $crate::app::GameContext) {
                <Self as $crate::states::base::Menu>::apply_radio_settings_now(self, ctx)
            }
            fn radio(&self) -> Option<&ff_core::radio::RadioState> {
                <Self as $crate::states::base::Menu>::radio(self)
            }
        }
    };
}

/// A ready-made menu for the simplest screens: a title and a fixed list of
/// rows, no state of its own (the Python pattern of a `MenuState` subclass
/// whose `build_items` returns literals). Also the example `Menu` impl.
pub struct SimpleMenuState {
    pub menu: MenuCore<SimpleMenuState>,
    rows: Vec<MenuItem<SimpleMenuState>>,
}

impl SimpleMenuState {
    pub fn new(title: &str, rows: Vec<MenuItem<SimpleMenuState>>) -> Self {
        Self {
            menu: MenuCore::new(title),
            rows,
        }
    }

    /// A screen of spoken lines: Up and Down read them one at a time, Enter
    /// repeats the line under focus, Back returns. The shape every readout
    /// takes (the driving status screens, the logbook), so a screen that
    /// used to be one long sentence can be re-read a line at a time.
    pub fn readout(title: &str, lines: Vec<String>) -> Self {
        let mut rows: Vec<MenuItem<SimpleMenuState>> = lines
            .into_iter()
            .map(|line| {
                let spoken = line.clone();
                MenuItem::new(line, move |_s: &mut SimpleMenuState, ctx| ctx.say(&spoken))
                    .help("Repeat this line.")
            })
            .collect();
        rows.push(
            MenuItem::new("Back", |s: &mut SimpleMenuState, ctx| s.go_back(ctx))
                .help("Return to the previous menu."),
        );
        let mut screen = Self::new(title, rows);
        screen.menu = screen.menu.with_intro_help(
            "Up and Down read a line at a time, Enter repeats it, Escape goes back.",
        );
        screen
    }
}

impl Menu for SimpleMenuState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        self.rows.clone()
    }
}

impl_state_for_menu!(SimpleMenuState);
