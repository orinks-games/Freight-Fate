//! Main menu, profile selection, name entry, settings, and help screens
//! (port of `freight_fate/states/main_menu.py`).
//!
//! The Python module was one file; here the settings screens, the career
//! management screens and the achievements browser live in submodules and
//! are re-exported from this one, so `states::main_menu::X` still names
//! every screen the Python `main_menu` did (including the ones it imported
//! from `main_menu_help`, `main_menu_career` and `update`).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Datelike, Local, Timelike};
use ff_core::models::business::status_label;
use ff_core::models::jobs::facility_text;
use ff_core::models::profile::{data_dir, LegacyCareerError, LoadError, Profile};
use ff_core::models::start_options::option_for_profile;
use ff_core::music::{select_menu_music_sequence, MenuMusicProfile};
use ff_core::playtest_levers::apply_continue_levers;
use ff_core::pyfmt::{fmt_f, fmt_grouped};

use crate::app::{share, version, GameContext, Say, SharedState};
use crate::browser::open_url;
use crate::discord_presence::PresenceState;
use crate::online_presence::IdentityStore;
use crate::states::base::{Menu, MenuCore, MenuItem, SimpleMenuState};
use crate::states::city::CityMenuState;
use crate::states::city_pickup::PickupFacilityState;
use crate::states::driving::{DrivingState, ACTIVE_TRIP_DEADLINE_MODEL};
use crate::states::learn_sounds::LearnSoundsState;
use crate::states::online_hub::OnlineHubState;
use crate::states::online_offer::OnlineOfferState;
use crate::states::save_notice::{
    DrivingRecordNoticeState, SaveMigrationNoticeState, SaveModifiedNoticeState,
};
use crate::states::text_entry::{TextEntry, TextEntryCore};
use crate::updater;
use crate::{impl_state_for_menu, impl_state_for_text_entry};

mod achievements;
mod careers;
mod settings;
mod settings_actions;
mod settings_items;
mod shortcuts;
mod touch_gestures;
mod touch_practice;

pub use achievements::{AchievementCareerState, AchievementCategoryState, AchievementsState};
pub use careers::{
    CareerAction, CareerActionsState, ConfirmCareerActionState, LoadDriverState, ManageCareersState,
};
pub use settings::{
    GameplaySettingsState, SettingsCategoryState, SettingsState, SETTINGS_LAYOUT_NOTICES,
};
pub use shortcuts::{ShortcutDevice, ShortcutsState};
pub use touch_gestures::{TouchCommandPickerState, TouchGesturesState};
pub use touch_practice::{TouchPracticeOfferState, TouchPracticeState};

pub use crate::states::main_menu_career::{
    region_menu_name, CareerStartState, HomeCityState, HomeTerminalState,
};
pub use crate::states::main_menu_help::{
    controls_help_page, help_page, help_pages, render_help_line, HelpState, HELP_PAGES,
};
pub use crate::states::update::{UpdateCheckState, UpdateChecker, UpdatePromptState};

/// A clearly-named stand-in for a screen another port task still owns.
///
/// Nothing pushes one any more: the city hub, the online offer, the online
/// hub, the pickup facility and the driving state are all real, so every
/// flow through this menu lands on the screen it names. Kept for the next
/// screen that needs a stand-in, so the flow around it stays testable
/// before its port lands.
pub fn todo_state(name: &str) -> SimpleMenuState {
    SimpleMenuState::new(
        &format!("{name} (not ported yet)"),
        vec![
            MenuItem::new("Back", |s: &mut SimpleMenuState, ctx| s.go_back(ctx))
                .help("This screen is not ported yet; Escape goes back."),
        ],
    )
}

// -- the save scan -----------------------------------------------------------------

thread_local! {
    static LAST_INVALID_SAVES: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
    // Careers the load gate refused because they were created before the 1.9
    // line. They must stay visible in the career list with a spoken label -- a
    // missing career reads as data loss to a blind player -- so loadable_saves
    // collects them here for the menus to show alongside the loadable ones.
    static LEGACY_SAVES: RefCell<Vec<LegacyCareerError>> = const { RefCell::new(Vec::new()) };
    // Reused within a single reuse_loadable_saves_scan() block (see below) so a
    // state that asks several times in one pass -- MainMenuState.enter() calls
    // loadable_saves() three times: build_items, announce_entry's legacy-save
    // check, and its own profile lookup -- pays for one save-directory scan
    // instead of three. Outside such a block every call still rescans, so a
    // state that lists saves after creating or deleting one never sees stale
    // results.
    static LOADABLE_SAVES_CACHE: RefCell<Option<Vec<(PathBuf, Profile)>>> =
        const { RefCell::new(None) };
    static LOADABLE_SAVES_CACHE_ACTIVE: Cell<bool> = const { Cell::new(false) };
    // How many real save-directory scans have run on this thread (the seam
    // the Python tests reached by counting `Profile.list_saves` calls).
    static SCAN_COUNT: Cell<usize> = const { Cell::new(0) };
}

/// Coalesce every [`loadable_saves`] call made inside `f`.
///
/// Reentrant: a nested call is a no-op, so this can wrap a method that also
/// wraps itself (or calls another wrapped method) without the inner scope
/// prematurely dropping the cache the outer scope is still relying on.
pub fn reuse_loadable_saves_scan<R>(f: impl FnOnce() -> R) -> R {
    let already_active = LOADABLE_SAVES_CACHE_ACTIVE.with(|a| a.replace(true));
    if !already_active {
        LOADABLE_SAVES_CACHE.with(|c| *c.borrow_mut() = None);
    }
    let result = f();
    if !already_active {
        LOADABLE_SAVES_CACHE_ACTIVE.with(|a| a.set(false));
        LOADABLE_SAVES_CACHE.with(|c| *c.borrow_mut() = None);
    }
    result
}

/// Return readable saves in newest-first order.
///
/// Inside a [`reuse_loadable_saves_scan`] block, the first real scan's
/// result (and the [`last_invalid_saves`] / [`legacy_saves`] it populates)
/// is reused for the rest of the block instead of rescanning the save
/// directory again.
pub fn loadable_saves() -> Vec<(PathBuf, Profile)> {
    let active = LOADABLE_SAVES_CACHE_ACTIVE.with(|a| a.get());
    if active {
        if let Some(cached) = LOADABLE_SAVES_CACHE.with(|c| c.borrow().clone()) {
            return cached;
        }
    }
    SCAN_COUNT.with(|n| n.set(n.get() + 1));
    let mut invalid = Vec::new();
    let mut legacy = Vec::new();
    let mut saves = Vec::new();
    for path in Profile::list_saves() {
        match Profile::load(&path) {
            Ok(profile) => {
                // Loading may have converted a legacy file in place; report
                // the path the career actually lives at now. That is the file
                // just read unless the conversion moved it: a save whose file
                // name differs from the career name (copied or renamed) lives
                // where it was found, not where its name points.
                let path = if path.exists() { path } else { profile.path() };
                saves.push((path, profile));
            }
            Err(LoadError::LegacyCareer(err)) => legacy.push(err),
            Err(LoadError::Integrity(_)) => invalid.push(path),
            Err(LoadError::Io(_)) => continue,
        }
    }
    LAST_INVALID_SAVES.with(|v| *v.borrow_mut() = invalid);
    LEGACY_SAVES.with(|v| *v.borrow_mut() = legacy);
    if active {
        LOADABLE_SAVES_CACHE.with(|c| *c.borrow_mut() = Some(saves.clone()));
    }
    saves
}

/// Saves the last scan could not read (moved aside).
pub fn last_invalid_saves() -> Vec<PathBuf> {
    LAST_INVALID_SAVES.with(|v| v.borrow().clone())
}

/// Careers the last scan refused as pre-1.9.
pub fn legacy_saves() -> Vec<LegacyCareerError> {
    LEGACY_SAVES.with(|v| v.borrow().clone())
}

/// How many real save-directory scans have run on this thread.
pub fn loadable_saves_scan_count() -> usize {
    SCAN_COUNT.with(|n| n.get())
}

// -- entering the world ---------------------------------------------------------------

/// The next one-time save notice the player has not heard yet, if any.
pub fn pending_notice_state(ctx: &GameContext) -> Option<SharedState> {
    let profile = ctx.profile.as_ref()?;
    if profile.migration_notice_pending {
        return Some(share(SaveMigrationNoticeState::new()));
    }
    if profile.integrity_notice_pending {
        return Some(share(SaveModifiedNoticeState::new()));
    }
    if profile.driving_record.notice_pending {
        return Some(share(DrivingRecordNoticeState::new()));
    }
    None
}

/// Whether a first-run player should hear the orinks.net offer at all.
// TODO(lead): belongs in `states::online_offer` (`should_offer_online`);
// reproduced here until that module lands.
pub fn should_offer_online(ctx: &GameContext) -> bool {
    if ctx.settings.online_offer_seen {
        return false;
    }
    IdentityStore::platform(&data_dir()).load().is_none()
}

/// The first-day welcome spoken at career creation.
// TODO(lead): belongs in `states::city` (`first_day_orientation_message`);
// reproduced here, word for word, until that module lands.
pub fn first_day_orientation_message(ctx: &GameContext, prefix: &str) -> String {
    let Some(p) = ctx.profile.as_ref() else {
        return String::new();
    };
    let terminal = ctx
        .world
        .home_terminal(&p.current_city)
        .map(|t| t.spoken_name())
        .unwrap_or_default();
    let option = option_for_profile(p);
    // Spoken city, never the map key (same fix as the states::city copy).
    let location = format!(
        "{terminal} in {} service area",
        ff_core::speech_text::the_city(&ctx.world.spoken_city(&p.current_city, None))
    );
    if option.is_owner_operator() {
        return format!(
            "{prefix}First-day briefing: leased to {}, parked at {location}. You own a new \
             truck with a full tank and {} dollars of working capital. Fuel, repairs, truck \
             wear, trailer programs, and business reserves come out of your cash. First \
             objective: open the dispatch board and choose an unlocked load with a deadline \
             you can protect.",
            option.carrier_name,
            fmt_grouped(p.money(), 0)
        );
    }
    format!(
        "{prefix}First-day briefing: welcome aboard {}. Your assigned truck is parked at \
         {location}. The carrier covers fuel, repairs, insurance, and trailer support. \
         Dispatch style: {}. As a new hire, dispatch assigns your load and route, and \
         refusing an assignment goes on your service record. First objective: open the \
         dispatch board, accept the assigned load, and deliver it cleanly.",
        option.carrier_name,
        option.dispatch.summary()
    )
}

/// The city menu, or the one-time orinks.net offer ahead of it.
pub fn first_state_after_career_creation(ctx: &GameContext) -> SharedState {
    if should_offer_online(ctx) {
        return share(OnlineOfferState::default());
    }
    // The welcome is spoken just before this state is pushed, so its entry
    // announcement queues behind it rather than cutting it off.
    share(CityMenuState::new(ctx, true))
}

/// Resume a saved mid-trip delivery if there is one, else the terminal hub.
///
/// `queue_entry_announcement` reaches the city menu only -- see
/// [`world_entry_state`]. A resumed drive already queues its own entry lines
/// (`DrivingState.enter` always speaks with `interrupt=False`), and a
/// pending save notice is a separate, unrelated screen, so neither needs the
/// flag threaded through.
pub fn enter_world(ctx: &mut GameContext, queue_entry_announcement: bool) {
    let state = pending_notice_state(ctx)
        .unwrap_or_else(|| world_entry_state(ctx, queue_entry_announcement));
    ctx.push_shared(state);
}

/// Build the first playable state for the current profile.
pub fn world_entry_state(ctx: &mut GameContext, queue_entry_announcement: bool) -> SharedState {
    let trip_kind = ctx
        .profile
        .as_ref()
        .and_then(|p| p.active_trip.as_ref())
        .map(|trip| {
            trip.get("kind")
                .and_then(|k| k.as_str())
                .unwrap_or("")
                .to_string()
        });
    if let Some(kind) = trip_kind {
        // A save from before local city-service drives were retired can still
        // carry one mid-trip. There is no route or phase left to resume it
        // with, so it drops the driver at the terminal instead of failing to
        // load -- this branch reads only that old snapshot shape and stays
        // even after every other city-service-drive code path is gone.
        if kind == "city_service_drive" {
            if let Some(p) = ctx.profile.as_mut() {
                p.active_trip = None;
            }
            ctx.save_profile();
            ctx.say(
                "Local service drives were retired in this update; you are parked at the terminal.",
            );
            return share(CityMenuState::new(ctx, true));
        }
        // A trip saved on a CDL that has since been pulled -- quit on the
        // roadside screen that pulled it, or a save from before a mid-drive
        // suspension ended the run -- does not resume: it closes out the
        // way the roadside does, and the terminal says the CDL status.
        if close_out_pulled_licence_trip(ctx) {
            ctx.say(
                "Your saved run cannot go on: dispatch cancels it, and a relief driver brings \
                 the truck back.",
            );
            return share(CityMenuState::new(ctx, true));
        }
        let snapshot = ctx
            .profile
            .as_ref()
            .and_then(|p| p.active_trip.clone())
            .unwrap_or(serde_json::Value::Null);
        let resumed: Option<SharedState> = if kind == "pickup" {
            PickupFacilityState::from_snapshot(ctx, &snapshot).map(share)
        } else {
            DrivingState::from_snapshot(ctx, &snapshot)
                .map(share)
                .inspect(|state| {
                    let deadline = state
                        .borrow()
                        .as_any()
                        .downcast_ref::<DrivingState>()
                        .map(|drive| (drive.job.deadline_game_h, drive.job.deadline_covers_rest));
                    if let Some((deadline, covers_rest)) = deadline {
                        let before = snapshot
                            .get("job")
                            .and_then(|job| job.get("deadline_game_h"))
                            .and_then(serde_json::Value::as_f64);
                        let prior_model = snapshot
                            .get("deadline_model")
                            .and_then(serde_json::Value::as_i64)
                            .unwrap_or(0);
                        if persist_deadline_migration(ctx, deadline, covers_rest)
                            && before.is_some_and(|old| deadline > old)
                        {
                            let reason = if prior_model == 1 && covers_rest {
                                " The adjusted time covers the 10-hour sleep your current legal hours require."
                            } else {
                                " The adjusted time covers the route's legal driving plan."
                            };
                            ctx.say_with(
                                format!(
                                    "Dispatch adjusted this saved delivery deadline to {} hours.{reason}",
                                    fmt_f(deadline, 1)
                                ),
                                Say::queued(),
                            );
                        }
                    }
                })
        };
        if let Some(state) = resumed {
            return state;
        }
        // An unreadable snapshot: clear it rather than retry it on every load.
        if let Some(p) = ctx.profile.as_mut() {
            p.active_trip = None;
        }
    }
    // The welcome that names this driver (from Continue or Choose career) is
    // spoken just before this state is chosen, so its entry announcement
    // queues behind that line instead of cutting it off.
    share(CityMenuState::new(ctx, queue_entry_announcement))
}

/// Clear and save a saved trip the CDL no longer allows. False (nothing
/// touched) while the CDL is clear.
fn close_out_pulled_licence_trip(ctx: &mut GameContext) -> bool {
    let Some(p) = ctx.profile.as_mut() else {
        return false;
    };
    if !p.driving_record.suspended(p.game_hours) {
        return false;
    }
    p.active_trip = None;
    p.pay_advance_used_for_load = false;
    ctx.save_profile();
    true
}

/// Write a one-time fair-deadline repair back into the saved active trip.
///
/// `DrivingState::from_snapshot` repairs snapshots written under an earlier
/// deadline model, but it reads a `&Value` and so cannot
/// record that it did. That signature is deliberate: the pause menu and the
/// snapshot round-trip tests hand it detached snapshots which must not reach
/// into the career at all. Python got the write-back for free because its
/// `from_snapshot` mutated the very dict `profile.active_trip` held, which is
/// the same coupling in a form nobody could opt out of.
///
/// So the persistence belongs to the one caller that owns the profile: this
/// one. Without it the floor is not one-time at all -- quit before the next
/// stop save, continue, and it is recomputed from the hours used by then,
/// handing out fresh deadline on every resume. That is the exploit
/// `test_current_active_trip_keeps_its_deadline_across_resumes` exists to
/// prevent.
///
/// The profile is saved here rather than left to the next save point, which
/// is where Python left it: a session that ends without one would re-apply
/// the floor on the next launch, which is the same exploit through a slower
/// door.
fn persist_deadline_migration(
    ctx: &mut GameContext,
    deadline_game_h: f64,
    deadline_covers_rest: bool,
) -> bool {
    let migrated = ctx
        .profile
        .as_mut()
        .and_then(|p| p.active_trip.as_mut())
        .and_then(serde_json::Value::as_object_mut)
        .is_some_and(|trip| {
            let model = trip
                .get("deadline_model")
                .and_then(serde_json::Value::as_i64)
                .unwrap_or(0);
            if model >= ACTIVE_TRIP_DEADLINE_MODEL {
                return false;
            }
            if let Some(job) = trip
                .get_mut("job")
                .and_then(serde_json::Value::as_object_mut)
            {
                job.insert(
                    "deadline_game_h".to_string(),
                    serde_json::json!(deadline_game_h),
                );
                job.insert(
                    "deadline_covers_rest".to_string(),
                    serde_json::json!(deadline_covers_rest),
                );
            }
            trip.insert(
                "deadline_model".to_string(),
                serde_json::json!(ACTIVE_TRIP_DEADLINE_MODEL),
            );
            true
        });
    if migrated {
        ctx.save_profile();
    }
    migrated
}

// -- career summaries ---------------------------------------------------------------

fn trip_str(job: &serde_json::Value, key: &str) -> String {
    job.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Where the career stands, for the career list (`_career_location`).
pub fn career_location(ctx: &GameContext, profile: &Profile) -> String {
    let empty = serde_json::Value::Object(Default::default());
    let trip = profile.active_trip.as_ref().unwrap_or(&empty);
    let job = trip.get("job").cloned().unwrap_or(empty.clone());
    let kind = trip.get("kind").and_then(|k| k.as_str()).unwrap_or("");
    let destination = job
        .get("destination")
        .and_then(|v| v.as_str())
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let or_default = |key: &str, default: &str| {
        let value = trip_str(&job, key);
        if value.is_empty() {
            default.to_string()
        } else {
            value
        }
    };
    // Spoken fields exist in post-slug payloads; legacy payloads fall back to
    // origin/destination, which there hold the old speakable display name.
    if kind == "pickup_drive" {
        let mut origin = trip_str(&job, "origin_spoken");
        if origin.is_empty() {
            origin = trip_str(&job, "origin");
        }
        if origin.is_empty() {
            origin = profile.current_city.clone();
        }
        let facility = facility_text(
            &or_default("origin_type", "metro_market"),
            &trip_str(&job, "origin_location"),
            &origin,
            &trip_str(&job, "origin_locality"),
        );
        return format!("driving to pickup at {facility}");
    }
    if let Some(destination) = destination {
        let mut spoken = trip_str(&job, "destination_spoken");
        if spoken.is_empty() {
            spoken = destination;
        }
        let facility = facility_text(
            &or_default("destination_type", "metro_market"),
            &trip_str(&job, "destination_location"),
            &spoken,
            &trip_str(&job, "destination_locality"),
        );
        if kind == "pickup" {
            let loaded = if trip
                .get("loaded")
                .is_some_and(|v| v.as_bool().unwrap_or(false))
            {
                "loaded for"
            } else {
                "picking up for"
            };
            return format!("{loaded} {facility}");
        }
        return format!("on the road to {facility}");
    }
    match ctx.world.home_terminal(&profile.current_city) {
        Ok(terminal) => format!(
            "at {} in {}",
            terminal.name,
            ctx.world.spoken_city(&profile.current_city, None)
        ),
        Err(_) => format!("in {}", profile.current_city),
    }
}

/// `Mon D, YYYY at H:MM AM` for a save file's modification time.
pub fn saved_label(path: &Path) -> String {
    let modified = std::fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(std::time::SystemTime::UNIX_EPOCH);
    let stamp: DateTime<Local> = modified.into();
    let hour = match stamp.hour() % 12 {
        0 => 12,
        h => h,
    };
    let am_pm = if stamp.hour() < 12 { "AM" } else { "PM" };
    format!(
        "{} {}, {} at {hour}:{:02} {am_pm}",
        stamp.format("%b"),
        stamp.day(),
        stamp.year(),
        stamp.minute()
    )
}

/// One career's spoken summary line (`_career_summary`).
pub fn career_summary(
    ctx: &GameContext,
    path: &Path,
    profile: &Profile,
    include_saved: bool,
) -> String {
    let mut parts = vec![
        format!("{}: level {}", profile.name, profile.career.level()),
        format!(
            "{} {}",
            profile.carrier_name,
            status_label(&profile.business_status)
        ),
        format!("{} dollars", fmt_grouped(profile.money(), 0)),
        career_location(ctx, profile),
        format!("{} deliveries", profile.career.deliveries),
    ];
    if include_saved {
        parts.push(format!("last saved {}", saved_label(path)));
    }
    parts.join(", ")
}

// -- the main menu ----------------------------------------------------------------------

// one startup update check per game session, shared across instances
static UPDATE_CHECK: Mutex<(Option<UpdateChecker>, bool)> = Mutex::new((None, false));

pub struct MainMenuState {
    menu: MenuCore<Self>,
}

impl MainMenuState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Freight Fate"),
        }
    }

    /// Start a fresh silent check for the next main-menu update cycle.
    pub fn arm_update_check(settings: &ff_core::settings::Settings) {
        if !updater::is_frozen() || !updater::SELF_UPDATES {
            return;
        }
        let mut guard = UPDATE_CHECK.lock().unwrap_or_else(|e| e.into_inner());
        *guard = (Some(UpdateChecker::new(settings)), false);
    }

    /// Test seam for the class-level checker: install (or clear) the
    /// session's checker and the prompted flag.
    pub fn install_update_check(checker: Option<UpdateChecker>, prompted: bool) {
        let mut guard = UPDATE_CHECK.lock().unwrap_or_else(|e| e.into_inner());
        *guard = (checker, prompted);
    }

    /// Whether a session checker is armed, and whether it has prompted.
    pub fn update_check_status() -> (bool, bool) {
        let guard = UPDATE_CHECK.lock().unwrap_or_else(|e| e.into_inner());
        (guard.0.is_some(), guard.1)
    }

    fn continue_latest(&mut self, ctx: &mut GameContext) {
        let saves = loadable_saves();
        let Some((_, latest)) = saves.into_iter().next() else {
            ctx.say("No saved careers found.");
            self.refresh(ctx, true);
            return;
        };
        ctx.profile = Some(latest);
        let lever_notes = apply_continue_levers(ctx);
        let p = ctx.profile.as_ref().expect("just loaded");
        let welcome = if p.active_trip.is_some() {
            format!("Welcome back, {}.", p.name)
        } else {
            let terminal_name = ctx
                .world
                .home_terminal(&p.current_city)
                .map(|t| t.name)
                .unwrap_or_default();
            format!(
                "Welcome back, {}. You are parked at {terminal_name} in {} with {} dollars.",
                p.name,
                ctx.world.spoken_city(&p.current_city, None),
                fmt_grouped(p.money(), 0)
            )
        };
        ctx.say(&welcome);
        // This welcome is spoken first; a resumed drive already queues its own
        // lines, and only the plain city-menu hand-off needs telling not to
        // cut it off (see world_entry_state).
        enter_world(ctx, true);
        for note in lever_notes {
            ctx.say_with(note, Say::queued());
        }
    }

    fn report_issue(&mut self, ctx: &mut GameContext) {
        let url = format!(
            "https://github.com/{}/issues/new?template=bug_report.yml",
            updater::REPO
        );
        // Through `browser::open_url`, not `webbrowser::open` directly: this
        // row used to be the one browser door with no seam on it at all, so
        // nothing could stand between a test that pressed it and a real page
        // in a real browser.
        let opened = open_url(&url);
        if !opened {
            ctx.say(&format!(
                "Could not open a web browser. Report problems at github.com/{}/issues.",
                updater::REPO
            ));
            return;
        }
        ctx.say(&format!(
            "Opening the bug report page in your web browser. Attach your game \
             log: game.log in {}. If you restarted the game after the problem, \
             attach game.prev.log, the previous run's log.",
            crate::app::logging::log_folder_words()
        ));
    }
}

impl Default for MainMenuState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for MainMenuState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        let sequence = reuse_loadable_saves_scan(|| {
            // The base `Menu::enter`: rebuild the rows, play the open sound,
            // announce.
            self.refresh(ctx, true);
            if let Some(key) = self.menu.open_sound_key.clone() {
                ctx.audio.play(&key);
            }
            self.announce_entry(ctx);
            let newest;
            let profile: Option<&Profile> = match ctx.profile.as_ref() {
                Some(p) => Some(p),
                None => {
                    newest = loadable_saves().into_iter().next().map(|(_, p)| p);
                    newest.as_ref()
                }
            };
            select_menu_music_sequence(profile.map(|p| p as &dyn MenuMusicProfile))
        });
        let refs: Vec<&str> = sequence.iter().map(String::as_str).collect();
        ctx.play_music_sequence("menu", &refs);
        let armed = UPDATE_CHECK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .0
            .is_some();
        if !armed {
            Self::arm_update_check(&ctx.settings);
        }
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        // The sound device now opens on a worker thread, so its verdict can
        // land a beat AFTER this menu's greeting already asked. Same rule
        // as at entry: silence with no word is never allowed.
        if ctx.audio.take_silence_notice() {
            ctx.say(
                "Game sounds could not start on this computer: the voice, but \
                 no engine, traffic, or alert sounds. Check that sound works \
                 elsewhere, then start Freight Fate again.",
            );
        }
        if crate::online_presence::take_secret_store_timeout_notice() {
            ctx.say(
                "The system password store did not answer in time, so online \
                 features may need you to link your account again under Online. \
                 If a permission dialog is still open, allow Freight Fate and \
                 restart.",
            );
        }
        let info = {
            let mut guard = UPDATE_CHECK.lock().unwrap_or_else(|e| e.into_inner());
            let (checker, prompted) = &mut *guard;
            let Some(checker) = checker else {
                return;
            };
            if *prompted || !checker.is_done() {
                return;
            }
            *prompted = true;
            checker.result()
        };
        if let Some(info) = info {
            if info.tag != ctx.settings.skipped_update {
                ctx.push_state(UpdatePromptState::new(info));
            }
        }
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let mut warning = String::new();
        let invalid = last_invalid_saves();
        if !invalid.is_empty() {
            let count = invalid.len();
            warning = if count == 1 {
                format!("{count} saved career could not be read and was moved aside. ")
            } else {
                format!("{count} saved careers could not be read and were moved aside. ")
            };
        }
        if ctx.settings.lane_keeping_unreadable {
            // Falling back to full lane keeping is the right answer and a
            // silent one is not: it deletes the destination-exit decision
            // outright, and nothing later in the drive would explain why.
            ctx.settings.lane_keeping_unreadable = false;
            warning.push_str(
                "Your lane keeping setting could not be read, so it is set to \
                 full: the truck holds the lane and takes your exits. Change it \
                 in Settings, Gameplay, Driving assistance. ",
            );
        }
        if ctx.audio.take_silence_notice() {
            // The game starts anyway when the sound device will not open --
            // that is the design, and a driver who can still hear the voice
            // can still work. What must not happen is the player being left
            // to guess: with no engine, no traffic and no alerts, and nothing
            // said, a broken sound setup is indistinguishable from a broken
            // game. Reported on Linux, where the device open failed and the
            // whole drive ran silent without a word.
            warning.push_str(
                "Game sounds could not start on this computer: the voice, but \
                 no engine, traffic, or alert sounds. Check that sound works \
                 elsewhere, then start Freight Fate again. ",
            );
        }
        if crate::online_presence::take_secret_store_timeout_notice() {
            // A Keychain ACL prompt after a new Mac signature can block the
            // driver-token read at launch. We stop waiting after a few
            // seconds so the menu opens; say why online may need a re-link.
            warning.push_str(
                "The system password store did not answer in time, so online \
                 features may need you to link your account again under Online. \
                 If a permission dialog is still open, allow Freight Fate and \
                 restart. ",
            );
        }
        if loadable_saves().is_empty() && !legacy_saves().is_empty() {
            // Every saved career predates 1.9, so there is no Continue item
            // where the player expects one. Say where the careers went before
            // that silence reads as data loss; once a 1.9 career exists, the
            // labels in Choose career carry the explanation instead.
            warning.push_str(
                "Your saved careers are from an earlier version of Freight \
                 Fate; they are listed under Choose career. ",
            );
        }
        let text = format!(
            "Welcome to Freight Fate, version {}. An audio trucking adventure across America. {warning}",
            updater::spoken_version(version())
        );
        ctx.say(text.trim_end());
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        let saves = loadable_saves();
        if let Some((latest_path, latest_profile)) = saves.first() {
            items.push(
                MenuItem::new(
                    format!(
                        "Continue latest career: {}",
                        career_summary(ctx, latest_path, latest_profile, false)
                    ),
                    |s: &mut Self, ctx| s.continue_latest(ctx),
                )
                .help(format!("Load the newest save for {}.", latest_profile.name)),
            );
        }
        if !saves.is_empty() || !legacy_saves().is_empty() {
            // Careers from earlier versions cannot continue, but they still
            // belong on the list: even with nothing loadable, Choose career is
            // where a player finds them and hears what happened.
            items.push(
                MenuItem::new("Choose career", |_s: &mut Self, ctx| {
                    ctx.push_state(LoadDriverState::new())
                })
                .help("Any saved career, not only the newest."),
            );
        }
        if !saves.is_empty() || !legacy_saves().is_empty() {
            items.push(
                MenuItem::new("Manage careers", |_s: &mut Self, ctx| {
                    ctx.push_state(ManageCareersState::new())
                })
                .help("Reset or delete saved careers."),
            );
        }
        items.push(
            MenuItem::new("New career", |_s: &mut Self, ctx| {
                ctx.push_state(NameEntryState::new())
            })
            .help("Start a fresh career."),
        );
        items.push(
            MenuItem::new("Achievements", |_s: &mut Self, ctx| {
                ctx.push_state(AchievementCareerState::new())
            })
            .help("Earned and locked achievements for a saved career."),
        );
        items.push(
            MenuItem::new("Online", |_s: &mut Self, ctx| {
                let hub = OnlineHubState::new(ctx);
                ctx.push_state(hub)
            })
            .help(
                "Drivers on duty, your orinks.net account, cloud backup \
                 and restore, and sharing choices like Mastodon and Discord.",
            ),
        );
        items.push(
            MenuItem::new("How to play", |_s: &mut Self, ctx| {
                ctx.push_state(HelpState::new())
            })
            .help("The controls and the goal of the game."),
        );
        items.push(
            MenuItem::new("Learn game sounds", |_s: &mut Self, ctx| {
                ctx.push_state(LearnSoundsState::new())
            })
            .help("Any sound the road uses, and what it means."),
        );
        items.push(
            MenuItem::new("Settings", |_s: &mut Self, ctx| {
                ctx.push_state(SettingsState::new())
            })
            .help(if cfg!(target_os = "ios") {
                "Units, transmission mode, volumes, weather, voices, and trip pacing."
            } else {
                "Units, transmission mode, volumes, weather, voices, \
                 update channel, and trip pacing."
            }),
        );
        items.push(
            MenuItem::new("Report a problem", |s: &mut Self, ctx| s.report_issue(ctx))
                .help("The bug report page on GitHub, in your web browser."),
        );
        // An iPhone app is never quit from inside; the system closes it.
        if !cfg!(target_os = "ios") {
            items.push(
                MenuItem::new("Quit", |_s: &mut Self, ctx| ctx.quit()).help("Exit the game."),
            );
        }
        items
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        if cfg!(target_os = "ios") {
            return;
        }
        ctx.audio.play("ui/menu_back");
        ctx.push_state(ConfirmQuitState::new());
    }

    fn presence(&self, _ctx: &GameContext) -> Option<PresenceState> {
        Some(PresenceState::activity("In the main menu"))
    }
}

impl_state_for_menu!(MainMenuState);

/// One spoken yes/no gate in front of quitting the game.
///
/// Escape at the main menu raises it, and so does the window's close button
/// or Alt+F4 from anywhere -- that close used to end the process on the spot,
/// which cost Darren two routes to a mis-hit key (2026-08-22).
///
/// `unsaved_drive` is what makes the gate worth reading rather than a
/// keystroke to swat away: quitting from the title loses nothing, quitting
/// mid-leg loses the leg, and only the second one needs saying.
pub struct ConfirmQuitState {
    menu: MenuCore<Self>,
    unsaved_drive: bool,
}

impl ConfirmQuitState {
    pub fn new() -> Self {
        Self::with_unsaved_drive(false)
    }

    /// `ConfirmQuitState(ctx, unsaved_drive=...)`.
    pub fn with_unsaved_drive(unsaved_drive: bool) -> Self {
        Self {
            menu: MenuCore::new("Quit Freight Fate?").with_open_sound(Some("ui/error")),
            unsaved_drive,
        }
    }

    fn question(&self) -> &'static str {
        if !self.unsaved_drive {
            return "Quit Freight Fate?";
        }
        // The same bargain the pause menu's quit already explains, in the
        // same words: you can only save at a stop.
        "Quit Freight Fate? You are part way through a drive. Saves happen \
         only at a stop, so this drive resumes from your last stop, not \
         from here."
    }
}

impl Default for ConfirmQuitState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for ConfirmQuitState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let text = format!("{} {}", self.question(), self.current_text(ctx));
        ctx.say_with(text, Say::new().review(false));
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new("No, stay in Freight Fate", |s: &mut Self, ctx| {
                s.go_back(ctx)
            }),
            MenuItem::new("Yes, quit Freight Fate", |_s: &mut Self, ctx| {
                // The close button is the one exit that skips the terminal's
                // own save, so it saves the way Escape there does. Nowhere
                // else has anything to write: a drive saves only at a stop,
                // and the title is already saved.
                // `try_borrow`: this menu is the top state and is borrowed
                // for the duration of its own callback.
                let at_terminal = ctx.states().iter().any(|state| {
                    state
                        .try_borrow()
                        .is_ok_and(|state| state.as_any().is::<CityMenuState>())
                });
                if at_terminal {
                    ctx.save_profile();
                }
                ctx.quit()
            }),
        ]
    }
}

impl_state_for_menu!(ConfirmQuitState);

/// New-career driver name entry on the shared accessible text field.
pub struct NameEntryState {
    entry: TextEntryCore,
}

impl NameEntryState {
    pub fn new() -> Self {
        Self {
            entry: TextEntryCore::new("New career", "Driver name"),
        }
    }

    /// The typed name.
    pub fn name(&self) -> String {
        self.entry.text()
    }

    pub fn cursor(&self) -> usize {
        self.entry.cursor
    }
}

impl Default for NameEntryState {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEntry for NameEntryState {
    fn entry(&self) -> &TextEntryCore {
        &self.entry
    }

    fn entry_mut(&mut self) -> &mut TextEntryCore {
        &mut self.entry
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        if crate::states::text_entry::touch_prompt(ctx) {
            ctx.say(&format!(
                "New career. Type your driver name. {}",
                crate::states::text_entry::TOUCH_FIELD_HELP
            ));
            return;
        }
        ctx.say(
            "New career. Type your driver name, then Enter. Left and Right \
             arrows review the letters, Home and End jump to the start or \
             end. Escape cancels.",
        );
    }

    fn confirm(&mut self, ctx: &mut GameContext) {
        let typed = self.entry.text();
        let name = match typed.trim() {
            "" => "Driver".to_string(),
            trimmed => trimmed.to_string(),
        };
        ctx.audio.play("ui/menu_select");
        ctx.push_state(CareerStartState::new(&name));
    }
}

impl_state_for_text_entry!(NameEntryState);
