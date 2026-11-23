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
    /// What the help says on a touch screen, when [`Self::help`] names keys.
    pub touch_help: Option<Label<S>>,
    /// The click played when this item is activated. `None` for items whose
    /// action plays its own confirmation sound, so the two do not stack.
    pub select_sound: Option<String>,
    /// The row leaves the menu the way Escape does ("Back", "Back to
    /// terminal", "Cancel"). A touch screen adds a Back row only to a menu
    /// with none; see [`goes_back_label`] and [`MenuItem::goes_back`].
    pub leaves: bool,
}

impl<S> Clone for MenuItem<S> {
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            action: Rc::clone(&self.action),
            help: self.help.clone(),
            touch_help: self.touch_help.clone(),
            select_sound: self.select_sound.clone(),
            leaves: self.leaves,
        }
    }
}

impl<S: 'static> MenuItem<S> {
    /// `MenuItem(label, action)` with the default help and select sound.
    pub fn new(
        label: impl Into<Label<S>>,
        action: impl Fn(&mut S, &mut GameContext) + 'static,
    ) -> Self {
        let label = label.into();
        let leaves = matches!(&label, Label::Text(text) if goes_back_label(text));
        Self {
            label,
            action: Rc::new(action),
            help: Label::Text(String::new()),
            touch_help: None,
            select_sound: Some("ui/menu_select".to_string()),
            leaves,
        }
    }

    /// Mark a row that leaves the menu but whose label does not say so
    /// ("Skip and start driving"), so a touch screen adds no Back row.
    pub fn goes_back(mut self) -> Self {
        self.leaves = true;
        self
    }

    /// A row that does nothing when activated (an information line).
    pub fn inert(label: impl Into<Label<S>>) -> Self {
        Self::new(label, |_, _| {})
    }

    pub fn help(mut self, help: impl Into<Label<S>>) -> Self {
        self.help = help.into();
        self
    }

    /// The help on a touch screen, for a row whose [`Self::help`] names keys.
    pub fn touch_help(mut self, help: impl Into<Label<S>>) -> Self {
        self.touch_help = Some(help.into());
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

    /// The row's help for the device in use, picked as it is spoken: an
    /// iPad player can connect a keyboard mid-session.
    pub fn help_text(&self, state: &S, ctx: &GameContext) -> String {
        match &self.touch_help {
            Some(touch) if ctx.touch_hints() => touch.text(state, ctx),
            _ => self.help.text(state, ctx),
        }
    }
}

/// A row label that leaves the menu by its wording: "Back", "Cancel",
/// "Close", or one that starts "Back to", "Return to", "Resume" or "No,".
/// A row this misses is marked with [`MenuItem::goes_back`].
pub fn goes_back_label(text: &str) -> bool {
    matches!(text, "Back" | "Cancel" | "Close")
        || ["Back to ", "Return to ", "Resume ", "No, "]
            .iter()
            .any(|start| text.starts_with(start))
}

/// The touch screen's Back row: last, on a menu Escape leaves that has no
/// row of its own going back (no keyboard means no Escape, and a scrub or a
/// two-finger flick down is not for every hand). An empty menu keeps its
/// intro help, which says how to go back.
fn add_touch_back_row<S: Menu>(menu: &S, ctx: &GameContext, items: &mut Vec<MenuItem<S>>) {
    if ctx.controller.device() != ff_core::input_hints::TOUCH
        || !menu.touch_back_row()
        || items.is_empty()
        || items.iter().any(|item| item.leaves)
    {
        return;
    }
    items.push(MenuItem::new("Back", |s: &mut S, ctx| s.go_back(ctx)));
}

/// The reusable state of a `MenuState`: the Python class attributes
/// (`title`, `intro_help`, `open_sound_key`) and instance fields (`items`,
/// `index`). Embed one in a screen struct and implement [`Menu`].
pub struct MenuCore<S> {
    pub title: String,
    pub intro_help: String,
    /// What the intro help says on a touch screen. `None` uses
    /// [`TOUCH_INTRO_HELP`], which serves every menu whose help only says how
    /// to move, choose and go back.
    pub touch_help: Option<String>,
    /// `None` plays nothing on entry.
    pub open_sound_key: Option<String>,
    pub items: Vec<MenuItem<S>>,
    pub index: usize,
}

/// How many rows Page Up and Page Down move in a menu, every menu alike.
pub const PAGE_ROWS: usize = 10;

pub const DEFAULT_INTRO_HELP: &str =
    "Enter selects, Escape goes back. Control stops the current speech.";

/// The intro help on a touch screen, for a menu with no touch help of its
/// own.
pub const TOUCH_INTRO_HELP: &str =
    "Flick right or left to move and double tap to choose. Flick down with two fingers to go back.";

impl<S> MenuCore<S> {
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_string(),
            intro_help: DEFAULT_INTRO_HELP.to_string(),
            touch_help: None,
            open_sound_key: Some("ui/menu_open".to_string()),
            items: Vec::new(),
            index: 0,
        }
    }

    pub fn with_intro_help(mut self, help: &str) -> Self {
        self.intro_help = help.to_string();
        self
    }

    /// The intro help on a touch screen, where the keyboard help says
    /// something particular to this screen.
    pub fn with_touch_help(mut self, help: &str) -> Self {
        self.touch_help = Some(help.to_string());
        self
    }

    /// The intro help for the device in use, picked as it is spoken.
    pub fn intro(&self, ctx: &GameContext) -> String {
        if ctx.touch_hints() {
            return self
                .touch_help
                .clone()
                .unwrap_or_else(|| TOUCH_INTRO_HELP.to_string());
        }
        self.intro_help.clone()
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

    /// Whether a touch screen may add a Back row. False for the main menu
    /// and for prompts that must be answered, where Escape answers rather
    /// than leaves.
    fn touch_back_row(&self) -> bool {
        true
    }

    // -- State hooks, forwarded by impl_state_for_menu! ----------------------------

    fn paces_main_speech(&self) -> bool {
        false
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        default_enter(self, ctx)
    }

    fn exit(&mut self, _ctx: &mut GameContext) {}

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
    }

    fn on_controller_disconnect(&mut self, _ctx: &mut GameContext) {}

    /// [`State::on_app_return`]: the focused row again.
    fn on_app_return(&mut self, ctx: &mut GameContext) {
        self.speak_current(ctx);
    }

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
        default_handle_event(self, ctx, event)
    }

    /// A touch gesture the menu answers itself: a tap reads the focused row
    /// again, the way VoiceOver's does. The pause menu also takes the
    /// two-finger double tap, which no other menu answers.
    fn handle_gesture(&mut self, ctx: &mut GameContext, gesture: crate::touch::Gesture) -> bool {
        default_handle_gesture(self, ctx, gesture)
    }

    fn handle_controller(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        default_handle_controller(self, ctx, event)
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
        default_announce_entry(self, ctx)
    }

    fn refresh(&mut self, ctx: &mut GameContext, keep_index: bool) {
        let old = self.menu().index;
        let mut items = self.build_items(ctx);
        add_touch_back_row(self, ctx, &mut items);
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
            return core.intro(ctx);
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
            Key::Home | Key::PageUp => core.index == 0,
            Key::End | Key::PageDown => core.index + 1 >= count,
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

    /// Page Up and Page Down (a three-finger flick left or right on touch):
    /// [`PAGE_ROWS`] rows back or forward,
    /// stopping at the first and last row, where the row is read again.
    /// Nothing wraps, so a jump never lands somewhere the player cannot
    /// place. A menu whose pages are its own takes the keys first
    /// ([`Self::handle_extra_key`]).
    fn page_by(&mut self, ctx: &mut GameContext, forward: bool) {
        let core = self.menu();
        if core.items.is_empty() {
            return;
        }
        let target = if forward {
            (core.index + PAGE_ROWS).min(core.items.len() - 1)
        } else {
            core.index.saturating_sub(PAGE_ROWS)
        };
        self.jump(ctx, target);
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
        default_go_back(self, ctx)
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
            fn handle_gesture(
                &mut self,
                ctx: &mut $crate::app::GameContext,
                gesture: $crate::touch::Gesture,
            ) -> bool {
                <Self as $crate::states::base::Menu>::handle_gesture(self, ctx, gesture)
            }
            fn on_controller_disconnect(&mut self, ctx: &mut $crate::app::GameContext) {
                <Self as $crate::states::base::Menu>::on_controller_disconnect(self, ctx)
            }
            fn on_app_return(&mut self, ctx: &mut $crate::app::GameContext) {
                <Self as $crate::states::base::Menu>::on_app_return(self, ctx)
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
        screen.menu = screen
            .menu
            .with_intro_help(
                "Up and Down read a line at a time, Enter repeats it, Escape goes back.",
            )
            .with_touch_help(
                "Flick right or left to read a line at a time, and double tap to repeat it. \
                 Flick down with two fingers to go back.",
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

/// The provided [`Menu::enter`]: build the rows, keep the cursor in range,
/// play the open sound and announce. An overriding screen calls this to
/// fall through.
pub fn default_enter<S: Menu>(menu: &mut S, ctx: &mut GameContext) {
    let mut items = menu.build_items(ctx);
    add_touch_back_row(menu, ctx, &mut items);
    let core = menu.menu_mut();
    core.items = items;
    core.clamp_index();
    if let Some(key) = menu.menu().open_sound_key.clone() {
        ctx.audio.play(&key);
    }
    menu.announce_entry(ctx);
}

/// The provided [`Menu::handle_event`], for an override that handles a key
/// or two and falls through for the rest. Screens used to keep their own
/// copies, which missed the held-key guard and [`Menu::handle_extra_key`].
pub fn default_handle_event<S: Menu>(menu: &mut S, ctx: &mut GameContext, event: &InputEvent) {
    let Some((key, _mods, text)) = event.key_down() else {
        return;
    };
    if event.key_repeat() && menu.held_key_moves_nothing(key) {
        return;
    }
    if menu.handle_extra_key(ctx, key) {
        return;
    }
    match key {
        Key::Down => menu.move_by(ctx, 1),
        Key::Up => menu.move_by(ctx, -1),
        Key::Home => menu.jump(ctx, 0),
        Key::End => {
            let last = menu.menu().items.len().saturating_sub(1);
            menu.jump(ctx, last);
        }
        Key::PageUp => menu.page_by(ctx, false),
        Key::PageDown => menu.page_by(ctx, true),
        Key::Return | Key::Space | Key::KpEnter => menu.activate(ctx),
        Key::Escape => menu.go_back(ctx),
        Key::F1 => {
            let help = menu.current_help(ctx);
            ctx.say(&help);
        }
        Key::LCtrl | Key::RCtrl => ctx.stop_speech(),
        _ => {
            if let Some(ch) = text.filter(|ch| ch.is_alphanumeric()) {
                let lower: String = ch.to_lowercase().collect();
                menu.first_letter_input(ctx, &lower, event.key_repeat());
            }
        }
    }
}

/// The controller half of a menu's input: the D-pad moves and adjusts, A
/// confirms, B goes back, and the help button reads the row's help.
pub fn default_handle_controller<S: Menu>(menu: &mut S, ctx: &mut GameContext, event: &InputEvent) {
    match ctx.controller.menu_action(event) {
        Some(ControllerAction::MenuDown) => menu.move_by(ctx, 1),
        Some(ControllerAction::MenuUp) => menu.move_by(ctx, -1),
        Some(ControllerAction::AdjustRight) => menu.adjust(ctx, 1),
        Some(ControllerAction::AdjustLeft) => menu.adjust(ctx, -1),
        Some(ControllerAction::Confirm) => menu.activate(ctx),
        Some(ControllerAction::Back) => menu.go_back(ctx),
        Some(ControllerAction::Help) => {
            let help = menu.current_help(ctx);
            ctx.say(&help);
        }
        None => {}
    }
}

/// The provided [`Menu::handle_gesture`]: a tap reads the focused row.
pub fn default_handle_gesture<S: Menu>(
    menu: &mut S,
    ctx: &mut GameContext,
    gesture: crate::touch::Gesture,
) -> bool {
    if gesture != crate::touch::Gesture::Tap {
        return false;
    }
    menu.speak_current(ctx);
    true
}

/// The provided [`Menu::announce_entry`]: the title, then the current row.
pub fn default_announce_entry<S: Menu>(menu: &mut S, ctx: &mut GameContext) {
    let title = end_sentence(&menu.menu().title);
    ctx.say(&title);
    let current = menu.current_text(ctx);
    ctx.say_with(current, Say::queued().review(false));
}

/// The provided [`Menu::go_back`]: the back sound, and pop the screen.
pub fn default_go_back<S: Menu>(_menu: &mut S, ctx: &mut GameContext) {
    ctx.audio.play("ui/menu_back");
    ctx.pop_state();
}
