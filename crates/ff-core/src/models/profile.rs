//! Player profile with atomic packed-container save/load (port of
//! `freight_fate/models/profile.py`).
//!
//! Saves are `.ffsave` files: a magic header plus zlib-compressed JSON, signed
//! inside with this install's HMAC key. The container keeps casual
//! hand-editing out of career state; the signature is the actual tamper
//! check, and a failed check marks the profile as modified rather than
//! refusing to load it. Plain `.json` saves from older versions still load
//! and are converted in place.
//!
//! On Windows and Linux, Freight Fate is portable: profiles and settings live
//! in a `saves` directory inside the game's own main directory -- next to the
//! executable in packaged builds, the project root when running from source.
//! macOS uses the per-user `~/Library/Application Support/FreightFate`
//! folder, and so do Windows and Linux when the game's own folder cannot be
//! written (see [`paths`]). Override the location with `FREIGHT_FATE_DATA_DIR`
//! (which the tests use).
//!
//! When running from source, `FREIGHT_FATE_SKIP_SAVE_SIGNING=1` skips the
//! save-signature check (the file is re-signed locally on load) so arbitrary
//! save files can be loaded for testing. Packaged builds ignore the flag.
//!
//! Saves are atomic: written to a temp file, then renamed over the old save,
//! so a crash mid-write can never corrupt an existing profile.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use indexmap::IndexMap;
use serde_json::{Map, Value};

use crate::models::business_constants::{is_owner_operator, COMPANY_DRIVER, INDEPENDENT_AUTHORITY};
use crate::models::career::{Career, JobView};
use crate::models::career_ladder::STARTER_CARRIER_NAME;
use crate::models::carrier_fleet::{assigned_truck_key, slip_seat_pool, slip_seats};
use crate::models::enforcement::DrivingRecord;
use crate::models::jobs::{py_str, py_truthy, Job};
use crate::models::loyalty::LoyaltyAccount;
use crate::models::market::Market;
use crate::models::money_guard::MoneyGuard;
use crate::models::safety_record::SAFETY_RECORD_BASELINE;
use crate::models::save_migration::json_f64;
use crate::models::start_options::{DEFAULT_START_KEY, START_MODE_COMPANY};
use crate::models::trailers::{normalized_trailer_programs, DEFAULT_TRAILER_PROGRAMS};
use crate::models::trucks::{build_truck_specs, NO_UPGRADES};
use crate::sim::hos::{DutyLog, HosClock};
use crate::sim::vehicle::{TruckSpecs, TruckState};

pub mod condition;
pub mod origin;
pub mod paths;
pub mod plausibility;
pub mod serialize;
pub mod signing;
mod traits;

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tests_compat;
#[cfg(test)]
mod tests_gate;
#[cfg(test)]
mod tests_origin;
#[cfg(test)]
mod tests_portable;
#[cfg(test)]
mod tests_python_fixture;

pub use condition::{fresh_condition, truck_tank_gal, CONDITION_FIELDS};
pub use paths::{
    data_dir, data_dir_if_allowed, game_root, profiles_dir, save_root, SaveRoots, DATA_DIR_ENV,
};
pub use signing::{
    decode_save_bytes, encode_save_bytes, is_signature_valid, signature_for,
    signature_for_with_secret, ProfileIntegrityError,
};

/// A per-truck condition record: the plain dict the 1.9 line keeps.
pub type ConditionRecord = Map<String, Value>;

// The 1.9 line's per-truck condition records are plain dicts, so the
// TruckCondition dataclass the mainline introduced is not used here.
/// Repair links that come with a chain set, and the garage restocks to.
pub const CHAIN_REPAIR_LINKS_PER_SET: i64 = 2;

pub const SAVE_VERSION: i64 = 11;

// The release line careers are created on, stamped into every save (the
// `created_line` field below). 1.9 rebalanced the whole career arc, so
// careers from earlier lines do not carry over (owner ruling, 2026-08-08):
// the load gate turns them away without touching the file, which stays
// playable by the 1.8 builds that wrote it.
pub const CREATED_LINE: &str = "1.9";

// The first save version only 1.9 builds ever wrote. The 1.8 line (dev and
// every stable release) stops at SAVE_VERSION 5; versions 6 and up were
// introduced by the 1.9 career arc (grounded start choices, 2026-06-27) and
// exist nowhere else. Saves from 1.9 dev builds that predate the
// `created_line` marker are recognized by this threshold instead, so
// testers' existing 1.9 careers are not locked out.
pub const FIRST_1_9_SAVE_VERSION: i64 = 6;
pub const STARTING_MONEY: f64 = 5_000.0;
pub const DEFAULT_CITY: &str = "chicago_il_us";
pub const DEFAULT_FUEL_GAL: f64 = 150.0;
pub const SIGNATURE_FIELD: &str = "_signature";
pub const SIGNATURE_VERSION_FIELD: &str = "_signature_version";
pub const SIGNATURE_VERSION: i64 = 3;
pub const SECRET_FILE: &str = "profile.key";

// Condition fields that were stored flat on the profile before per-truck
// conditions (SAVE_VERSION 11 / SIGNATURE_VERSION 2). Kept for two reasons:
// validating v1 signatures against the field set they were signed over, and
// migrating legacy saves into per-truck records.
pub const LEGACY_CONDITION_FIELDS: &[&str] = &[
    "truck_damage_pct",
    "tire_wear_pct",
    "brake_wear_pct",
    "engine_wear_pct",
    "truck_fuel_gal",
];

// Packed save container: this magic header, then zlib-deflated profile JSON.
// The container stops accidental and casual hand-editing; the HMAC signature
// inside the JSON remains the actual tamper check. Signed plain-JSON saves
// still load and are converted on their next save (the old file is kept as
// `.json.bak` so an older game version can still be rolled back to); an
// unsigned one loads marked as modified.
pub const SAVE_MAGIC: &[u8] = b"FFSAVE1\x00";
pub const SAVE_SUFFIX: &str = ".ffsave";
pub const LEGACY_SAVE_SUFFIX: &str = ".json";

/// `Profile.__dataclass_fields__`, in declaration order: the JSON keys of a
/// save and the field set the pre-v3 signatures were computed over.
pub const PROFILE_FIELDS: &[&str] = &[
    "name",
    "money",
    "current_city",
    "created_line",
    "migration_notice_pending",
    "integrity_modified",
    "integrity_notice_pending",
    "hos_key_notice_left",
    "game_hours",
    "calendar_offset_days",
    "tutorial_done",
    "truck",
    "owned_trucks",
    "truck_conditions",
    "upgrades",
    "active_trip",
    "dispatch_board_cache",
    "fatigue",
    "active_buffs",
    "pay_advance",
    "fines_owed",
    "pay_advance_used_for_load",
    "business_status",
    "carrier_name",
    "carrier_key",
    "start_mode",
    "authority_readiness",
    "weigh_station_transponder",
    "trailer_programs",
    "owned_trailers",
    "owner_operator_declined",
    "career",
    "driving_record",
    "selection_score",
    "out_of_service_events",
    "market",
    "hos",
    "duty_log",
    "loyalty",
    "achievements",
    "achievement_stats",
    "radio_favorites",
    "recent_lanes",
];

/// Called with the profile after every successful save. The app points this
/// at the cloud backup service so every save site -- deliveries,
/// achievements, menu exits, shutdown -- queues a backup without knowing the
/// service exists. Best-effort only: a listener failure must never break the
/// local save.
pub type SaveListener = Arc<dyn Fn(&Profile) + Send + Sync>;

thread_local! {
    /// The hook belongs to whichever thread installed it.
    ///
    /// One `App` owns one `GameContext`, the context is the only thing that
    /// saves a profile, and it holds `Rc`s and boxed sinks -- so it is
    /// `!Send` and can never leave the thread that built it. Installer and
    /// caller are therefore always the same thread, and a thread-local is
    /// exactly as correct for the game as the process-global `Mutex` it
    /// replaces.
    ///
    /// What it is not is exactly as correct for the TESTS. A global made
    /// every `App` in the suite share one hook, so a second app's shutdown
    /// tore out the first app's listener; the fix at the time was to let
    /// only one app exist at a time, behind the process-wide environment
    /// lock. Per thread, apps stop being able to see each other and the
    /// reason for that lock goes away.
    static SAVE_LISTENER: std::cell::RefCell<Option<SaveListener>> =
        const { std::cell::RefCell::new(None) };
}

/// Install (or clear) the `save_listener` hook for the current thread.
pub fn set_save_listener(listener: Option<SaveListener>) {
    SAVE_LISTENER.with(|slot| *slot.borrow_mut() = listener);
}

fn notify_save_listener(profile: &Profile) {
    // Cloned out of the slot before the call: a listener that saves a
    // profile itself would otherwise re-enter this borrow and panic.
    let listener = SAVE_LISTENER.with(|slot| slot.borrow().clone());
    if let Some(listener) = listener {
        // A panicking listener must not take the save with it.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| listener(profile)));
        if result.is_err() {
            log::debug!("Profile save listener failed");
        }
    }
}

/// True when running as a packaged build rather than a source checkout: the
/// executable has a `build_info.json` beside it (what `cargo tools build-release`
/// stamps). A `cargo run` binary in `target/` has none and stays a source
/// checkout.
pub fn is_frozen() -> bool {
    std::env::current_exe()
        .ok()
        .map(|exe| {
            crate::data::data_resources::resource_dir_for_executable(
                &exe,
                cfg!(target_os = "macos"),
            )
            .join("build_info.json")
            .is_file()
        })
        .unwrap_or(false)
}

/// The version `cargo tools build-release` stamped into the build beside
/// `executable`, if any (`freight_fate._baked_version`).
pub fn baked_version_beside(executable: &Path) -> Option<String> {
    let info_path = crate::data::data_resources::resource_dir_for_executable(
        executable,
        cfg!(target_os = "macos"),
    )
    .join("build_info.json");
    let text = std::fs::read_to_string(info_path).ok()?;
    let data: Value = serde_json::from_str(&text).ok()?;
    let version = data.as_object()?.get("package_version")?;
    if !py_truthy(version) {
        return None;
    }
    Some(py_str(version))
}

/// `freight_fate._baked_version()` for the running executable.
pub fn baked_version() -> Option<String> {
    baked_version_beside(&std::env::current_exe().ok()?)
}

/// `freight_fate.__version__`: the baked version in a packaged build, else the
/// crate version (the workspace's `package-version` carries the same number).
pub fn game_version() -> String {
    baked_version().unwrap_or_else(|| env!("CARGO_PKG_VERSION").to_string())
}

/// Dev escape hatch: `FREIGHT_FATE_SKIP_SAVE_SIGNING=1` loads any save
/// regardless of its signature, so arbitrary files can be tested. Honored
/// only when running from source; packaged player builds always enforce
/// signing so the flag can never become a tampering vector.
pub fn signing_checks_disabled_when(frozen: bool) -> bool {
    !frozen && std::env::var("FREIGHT_FATE_SKIP_SAVE_SIGNING").as_deref() == Ok("1")
}

/// A career from the 1.8 line (or earlier) that 1.9 does not continue.
///
/// Raised by the load gate *before* any migration or resave machinery runs:
/// the file on disk stays byte-for-byte intact, still loadable by the build
/// that wrote it. Carries the driver name so menus can label the career
/// instead of letting it vanish from the list.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{name}: career created before the 1.9 line")]
pub struct LegacyCareerError {
    pub name: String,
    pub path: Option<PathBuf>,
}

/// Why `Profile::load` refused a file.
#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error(transparent)]
    Integrity(#[from] ProfileIntegrityError),
    #[error(transparent)]
    LegacyCareer(#[from] LegacyCareerError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Whether a raw save dict was created on the 1.8 line or earlier.
///
/// The explicit `created_line` marker decides when present. Saves written
/// before the marker existed are judged by their save version: the 1.8 line
/// never wrote a version past 5, while every 1.9 build has written 6 or
/// higher since the career arc landed -- so existing 1.9 tester careers pass
/// and are stamped with the marker on their next save.
pub fn is_pre_1_9_save(data: &Map<String, Value>) -> bool {
    if data.get("created_line").is_some_and(py_truthy) {
        return false;
    }
    match data.get("version") {
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => {
            n.as_i64().unwrap_or(0) < FIRST_1_9_SAVE_VERSION
        }
        _ => true,
    }
}

/// Whether an on-disk save was created before the 1.9 line.
///
/// Unreadable files are not legacy saves -- they fail the load gate on their
/// own terms. Used by the new-career flow so starting over with the same
/// driver name can never overwrite a career an earlier build still owns.
pub fn is_pre_1_9_save_file(path: &Path) -> bool {
    let Ok(raw) = std::fs::read(path) else {
        return false;
    };
    match decode_save_bytes(&raw) {
        Ok((data, _)) => is_pre_1_9_save(&data),
        Err(_) => false,
    }
}

/// The canonical packed save path for a profile or cloud slot name.
pub fn save_path_for(name: &str) -> PathBuf {
    profiles_dir().join(format!("{}{SAVE_SUFFIX}", signing::sanitized_stem(name)))
}

/// The existing save file for a slot name: packed preferred, legacy accepted.
pub fn find_save_path(name: &str) -> Option<PathBuf> {
    let packed = save_path_for(name);
    if packed.exists() {
        return Some(packed);
    }
    let legacy = packed.with_extension("json");
    legacy.exists().then_some(legacy)
}

/// The player's career: everything a save carries.
#[derive(Debug, Clone, PartialEq)]
pub struct Profile {
    pub name: String,
    /// Private so every change goes through `earn`, `spend` or `set_money`
    /// and the money guard sees it; a direct write would wrongly mark an
    /// honest career as modified. Read it with [`Profile::money`].
    money: f64,
    pub current_city: String,
    // The release line this career was created on. New careers stamp the
    // current line; a save without the field is judged by its save version
    // instead (see is_pre_1_9_save), and pre-1.9 saves never get this far --
    // the load gate turns them away before from_dict, so the default here can
    // only ever backfill a 1.9 career from before the marker existed.
    pub created_line: String,
    // An old save was converted to the per-truck format; the player has not yet
    // heard the one-time notice. Cleared when they dismiss it.
    pub migration_notice_pending: bool,
    // The save failed its local signature check (edited outside the game, or
    // copied from another machine, whose signing key differs). Sticky: it is
    // signed into every later save, so clearing it by hand just trips the
    // signature again. Local play continues; shared features read this flag.
    pub integrity_modified: bool,
    // The player has not yet heard the one-time spoken notice about the flag.
    pub integrity_notice_pending: bool,
    // Clock presses left that still append "hours of service moved to Alt A,
    // Alt S, and Alt D". The detail moved off C onto its own keys, and muscle
    // memory says C, so the pointer rides C -- three times, then never again.
    pub hos_key_notice_left: i64,
    /// in-game clock, hours since career start
    pub game_hours: f64,
    // Whole-day offset used only by the spoken calendar and seasonal weather.
    // Existing careers can anchor their independent calendar to today's date
    // without changing deadlines, HOS, markets, or elapsed career time.
    pub calendar_offset_days: i64,
    // Session-only sub-day offset used by the spoken calendar and driving
    // clock. Real-time driving re-anchors it on selection or drive start; it
    // is intentionally not saved or sent to the cloud validator.
    pub calendar_offset_hours: f64,
    // Session-only: live weather is driving the spoken calendar, so the date
    // the player hears is today's real one. The app mirrors the settings into
    // it every frame; never saved.
    pub live_calendar: bool,
    pub tutorial_done: bool,
    /// owner-operator active tractor, or assignment key
    pub truck: String,
    /// owned tractors after buy-in
    pub owned_trucks: Vec<String>,
    // Condition follows the truck, not the profile: wear, damage, and fuel per
    // owned truck key. The flat `tire_wear_pct`/`truck_fuel_gal`/... names
    // remain as accessors (below) proxying to the active truck's record.
    pub truck_conditions: IndexMap<String, ConditionRecord>,
    /// owned-tractor upgrade key -> tier
    pub upgrades: IndexMap<String, i64>,
    /// mid-delivery snapshot, see DrivingState
    pub active_trip: Option<Value>,
    pub dispatch_board_cache: Option<Value>,
    /// 0 fresh .. 100 exhausted
    pub fatigue: f64,
    /// timed consumables, see data/buffs
    pub active_buffs: Vec<Value>,
    /// outstanding dispatcher advance owed, repaid at delivery
    pub pay_advance: f64,
    // Fines a settlement could not fully collect. Carried forward and taken
    // out of the next one, because the alternative -- saying a fine was paid
    // and then writing it off -- tells the player something untrue.
    pub fines_owed: f64,
    pub pay_advance_used_for_load: bool,
    /// company driver, then leased-on owner-operator
    pub business_status: String,
    pub carrier_name: String,
    pub carrier_key: String,
    pub start_mode: String,
    pub authority_readiness: bool,
    // Owner-operator-purchased weigh-in-motion bypass subscription (see
    // models/business.has_weigh_station_transponder). Company drivers never
    // set this -- their fleet issues one free at
    // business.WEIGH_STATION_TRANSPONDER_LEVEL instead.
    pub weigh_station_transponder: bool,
    pub trailer_programs: Vec<String>,
    pub owned_trailers: Vec<String>,
    /// The driver chose to stay a company driver with the buy-in open
    /// (Business status, "Stay a company driver"): the buy-in row stays,
    /// the nudges toward it stop. Cleared by buying in or by reopening the
    /// plan from the same screen.
    pub owner_operator_declined: bool,
    pub career: Career,
    // Citations, serious violations, and CDL standing. Enforcement outlives a
    // trip: the old build kept the felony count on the trip snapshot and then
    // threw the snapshot away, so nothing a driver did downstream ever
    // remembered it.
    pub driving_record: DrivingRecord,
    // How interesting this driver looks to a screening lane, 0 to 100, higher
    // being worse. Derived from reputation, citations, out-of-service history,
    // damage carried and clean inspections (see models/safety_record) and
    // refreshed whenever any of those move; stored so the scale can read it
    // without rebuilding the whole history mid-approach. Spoken as "safety
    // record", never as a number and never as a trade acronym.
    pub selection_score: f64,
    // Times this driver has been placed out of service, roadside or at a
    // scale. Feeds the safety record; kept on the profile rather than the
    // license file because it is a carrier fact, not a licensing one.
    pub out_of_service_events: i64,
    pub market: Market,
    /// hours-of-service shift clock
    pub hos: HosClock,
    /// rolling Record of Duty Status
    pub duty_log: DutyLog,
    /// truck stop loyalty program
    pub loyalty: LoyaltyAccount,
    pub achievements: Vec<String>,
    pub achievement_stats: Map<String, Value>,
    // Station ids the driver saved with the favorite key; they surface as the
    // radio dial's early Favorites category. Additive with a default, so older
    // saves load unchanged and from_dict simply fills it in.
    pub radio_favorites: Vec<String>,
    // Last few delivered from:to lanes, newest first -- assigned dispatch
    // prefers a lane not in this list so short-haul careers stop bouncing
    // between the same two cities forever.
    pub recent_lanes: Vec<String>,

    // Set by from_dict when the raw dict needed a format migration, so load()
    // can rewrite the converted save to disk. Never serialized.
    pub needs_migration_resave: bool,

    // Runtime shadow of `money`, held obfuscated so an editor writing the
    // field directly leaves the two disagreeing. Never serialized; see
    // models/money_guard.rs. Seeded at creation and at load, moved by
    // earn/spend, resynced by set_money.
    pub money_guard: MoneyGuard,
}

impl Default for Profile {
    fn default() -> Self {
        Profile {
            name: "Driver".to_string(),
            money: STARTING_MONEY,
            current_city: DEFAULT_CITY.to_string(),
            created_line: CREATED_LINE.to_string(),
            migration_notice_pending: false,
            integrity_modified: false,
            integrity_notice_pending: false,
            hos_key_notice_left: 3,
            game_hours: 6.0,
            calendar_offset_days: 0,
            calendar_offset_hours: 0.0,
            live_calendar: false,
            tutorial_done: false,
            truck: "rig".to_string(),
            owned_trucks: Vec::new(),
            truck_conditions: IndexMap::new(),
            upgrades: IndexMap::new(),
            active_trip: None,
            dispatch_board_cache: None,
            fatigue: 0.0,
            active_buffs: Vec::new(),
            pay_advance: 0.0,
            fines_owed: 0.0,
            pay_advance_used_for_load: false,
            business_status: COMPANY_DRIVER.to_string(),
            carrier_name: STARTER_CARRIER_NAME.to_string(),
            carrier_key: DEFAULT_START_KEY.to_string(),
            start_mode: START_MODE_COMPANY.to_string(),
            authority_readiness: false,
            weigh_station_transponder: false,
            trailer_programs: Vec::new(),
            owned_trailers: Vec::new(),
            owner_operator_declined: false,
            career: Career::new(),
            driving_record: DrivingRecord::new(),
            selection_score: SAFETY_RECORD_BASELINE,
            out_of_service_events: 0,
            market: Market::new(),
            hos: HosClock::new(),
            duty_log: DutyLog::new(),
            loyalty: LoyaltyAccount::new(),
            achievements: Vec::new(),
            achievement_stats: Map::new(),
            radio_favorites: Vec::new(),
            recent_lanes: Vec::new(),
            needs_migration_resave: false,
            money_guard: MoneyGuard::seeded(STARTING_MONEY),
        }
    }
}

/// `RECENT_LANES_KEPT`.
pub const RECENT_LANES_KEPT: usize = 6;

impl Profile {
    /// The reputation everyone reads and every gate checks: the delivery
    /// ledger (`career.reputation`) less what the driving record still inside
    /// the review window costs, see `enforcement::standing_reputation`. The
    /// ledger keeps earning underneath, so an aged-out record gives the
    /// points back.
    pub fn standing(&self) -> f64 {
        crate::models::enforcement::standing_reputation(
            self.career.reputation,
            &self.driving_record,
            self.game_hours,
        )
    }

    /// `Profile()`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `Profile(name=name)`.
    pub fn named(name: &str) -> Self {
        Profile {
            name: name.to_string(),
            ..Self::default()
        }
    }

    /// `Profile(name=name, current_city=city)`.
    pub fn named_in(name: &str, current_city: &str) -> Self {
        Profile {
            name: name.to_string(),
            current_city: current_city.to_string(),
            ..Self::default()
        }
    }

    // -- money -----------------------------------------------------------------

    /// Verify the live `money` field against the shadow the money guard
    /// keeps. A balance rewritten outside the game's own paths (a memory
    /// editor mid-session) leaves the two disagreeing; the first legitimate
    /// transaction or save then marks the career the same way an edited save
    /// file is marked. Legitimate bulk writes go through `set_money`, which
    /// resyncs the shadow instead. Returns false when a divergence was seen.
    pub fn audit_money(&mut self) -> bool {
        if self.money_guard.check(self.money) {
            return true;
        }
        log::warn!(
            "{}: balance changed outside a game transaction; marking career modified",
            self.name
        );
        self.integrity_modified = true;
        self.integrity_notice_pending = true;
        origin::forget_origin(&self.name);
        false
    }

    /// Clear the modified mark after the server accepted the career on
    /// review; where it arrived from no longer matters.
    pub fn absolve(&mut self) {
        self.integrity_modified = false;
        self.integrity_notice_pending = false;
        origin::forget_origin(&self.name);
    }

    /// The career balance in dollars.
    pub fn money(&self) -> f64 {
        self.money
    }

    /// Credit `amount` dollars through the money guard.
    pub fn earn(&mut self, amount: f64) {
        self.audit_money();
        self.money += amount;
        self.money_guard.apply(amount);
    }

    /// Debit `amount` dollars through the money guard.
    pub fn spend(&mut self, amount: f64) {
        self.audit_money();
        self.money -= amount;
        self.money_guard.apply(-amount);
    }

    /// Set the balance outright (start options, scenario levers, settlement
    /// corrections): the guard adopts the new number rather than flagging
    /// it. Shadows the `set_money` trait methods, which resync the same way.
    pub fn set_money(&mut self, money: f64) {
        self.money = money;
        self.money_guard.resync(money);
    }

    // -- truck -----------------------------------------------------------------

    /// True when the profile is responsible for owned tractor equipment.
    pub fn owns_equipment(&self) -> bool {
        is_owner_operator(&self.business_status)
    }

    /// Truck model currently used for simulation.
    ///
    /// Company drivers operate whatever tractor the carrier fleet has
    /// assigned for their level band. The profile still carries `truck` for
    /// save compatibility and for the owner-operator path, but company-driver
    /// play should not treat it as player-owned equipment.
    pub fn active_truck_key(&self) -> String {
        if self.owns_equipment() {
            return self.truck.clone();
        }
        // A slip-seating driver keeps the tractor dispatch handed them for this
        // run (`take_slip_seat` wrote it into `truck`, which has always doubled
        // as the assignment key). It has to still be one of this driver's
        // spares to count: a promotion moves the pool on, and a save written
        // before slip-seating carries a value from the old scheme.
        if slip_seats(self) && slip_seat_pool(self).contains(&self.truck.as_str()) {
            return self.truck.clone();
        }
        assigned_truck_key::<_, Job>(self, None).to_string()
    }

    /// Draw the tractor dispatch has picked for this load; returns its key.
    ///
    /// Company drivers only, and only while they are still slip-seating -- an
    /// owner-operator's truck is their own, and a senior company driver has a
    /// seat of their own to come back to.
    pub fn take_slip_seat<J: JobView + ?Sized>(&mut self, job: &J) -> String {
        if self.owns_equipment() || !slip_seats(self) {
            return self.active_truck_key();
        }
        let key = assigned_truck_key(self, Some(job));
        self.truck = key.to_string();
        key.to_string()
    }

    /// Player-owned tractors to show in menus.
    pub fn visible_owned_trucks(&self) -> Vec<String> {
        if self.owns_equipment() {
            self.owned_trucks.clone()
        } else {
            Vec::new()
        }
    }

    /// Trailer programs the player controls for owner-operator dispatch.
    pub fn active_trailer_programs(&self) -> Vec<String> {
        if !self.owns_equipment() {
            return Vec::new();
        }
        let programs: Vec<String> = normalized_trailer_programs(&self.trailer_programs)
            .into_iter()
            .map(str::to_string)
            .collect();
        if self.business_status == INDEPENDENT_AUTHORITY {
            let mut combined = programs.clone();
            for key in normalized_trailer_programs(&self.owned_trailers) {
                if !combined.iter().any(|k| k == key) {
                    combined.push(key.to_string());
                }
            }
            if !combined.is_empty() {
                return combined;
            }
        }
        if !programs.is_empty() {
            return programs;
        }
        DEFAULT_TRAILER_PROGRAMS
            .iter()
            .map(|k| k.to_string())
            .collect()
    }

    /// Player-owned trailers to show in menus.
    pub fn visible_owned_trailers(&self) -> Vec<String> {
        if self.business_status != INDEPENDENT_AUTHORITY {
            return Vec::new();
        }
        normalized_trailer_programs(&self.owned_trailers)
            .into_iter()
            .map(str::to_string)
            .collect()
    }

    /// The active truck's specs with this profile's upgrades applied.
    pub fn truck_specs(&self) -> TruckSpecs {
        if self.owns_equipment() {
            build_truck_specs(&self.active_truck_key(), &self.upgrades)
        } else {
            build_truck_specs(&self.active_truck_key(), &NO_UPGRADES)
        }
    }

    // -- per-truck condition ---------------------------------------------------
    //
    // Condition lives in `truck_conditions` keyed by truck. The flat names
    // below stay as accessors routed through the *active* truck's record, so
    // the garage, the rig readout, and the save layer keep using
    // `p.tire_wear_pct()` unchanged while each truck carries its own wear,
    // damage, and fuel.

    /// The active truck's condition record, created on first touch.
    fn condition_mut(&mut self) -> &mut ConditionRecord {
        let key = self.active_truck_key();
        self.truck_conditions
            .entry(key)
            .or_insert_with(|| fresh_condition(DEFAULT_FUEL_GAL))
    }

    /// The active truck's record, if it has been touched yet.
    fn condition(&self) -> Option<&ConditionRecord> {
        self.truck_conditions.get(&self.active_truck_key())
    }

    fn condition_f64(&self, key: &str, default: f64) -> f64 {
        self.condition()
            .map(|rec| json_f64(rec.get(key), default))
            .unwrap_or(default)
    }

    fn set_condition(&mut self, key: &str, value: Value) {
        self.condition_mut().insert(key.to_string(), value);
    }

    /// Give a newly acquired truck its own fresh, full-tank record.
    pub fn provision_truck_condition(&mut self, key: &str, fuel_gal: Option<f64>) {
        let tank = fuel_gal.unwrap_or_else(|| truck_tank_gal(key, &NO_UPGRADES));
        self.truck_conditions
            .insert(key.to_string(), fresh_condition(tank));
    }

    pub fn tire_wear_pct(&self) -> f64 {
        self.condition_f64("tire_wear_pct", 0.0)
    }
    pub fn set_tire_wear_pct(&mut self, value: f64) {
        self.set_condition("tire_wear_pct", Value::from(value));
    }

    pub fn brake_wear_pct(&self) -> f64 {
        self.condition_f64("brake_wear_pct", 0.0)
    }
    pub fn set_brake_wear_pct(&mut self, value: f64) {
        self.set_condition("brake_wear_pct", Value::from(value));
    }

    pub fn engine_wear_pct(&self) -> f64 {
        self.condition_f64("engine_wear_pct", 0.0)
    }
    pub fn set_engine_wear_pct(&mut self, value: f64) {
        self.set_condition("engine_wear_pct", Value::from(value));
    }

    pub fn truck_damage_pct(&self) -> f64 {
        self.condition_f64("damage_pct", 0.0)
    }
    pub fn set_truck_damage_pct(&mut self, value: f64) {
        self.set_condition("damage_pct", Value::from(value));
    }

    pub fn truck_fuel_gal(&self) -> f64 {
        self.condition_f64("fuel_gal", DEFAULT_FUEL_GAL)
    }
    pub fn set_truck_fuel_gal(&mut self, value: f64) {
        self.set_condition("fuel_gal", Value::from(value));
    }

    pub fn road_grime_pct(&self) -> f64 {
        self.condition_f64("grime_pct", 0.0)
    }
    pub fn set_road_grime_pct(&mut self, value: f64) {
        self.set_condition("grime_pct", Value::from(value));
    }

    pub fn tire_type(&self) -> String {
        self.condition()
            .and_then(|rec| rec.get("tire_type"))
            .map(py_str)
            .unwrap_or_else(|| "all_season".to_string())
    }
    pub fn set_tire_type(&mut self, value: &str) {
        self.set_condition("tire_type", Value::from(value));
    }

    pub fn chains_owned(&self) -> bool {
        self.condition()
            .and_then(|rec| rec.get("chains_owned"))
            .is_some_and(py_truthy)
    }
    pub fn set_chains_owned(&mut self, value: bool) {
        self.set_condition("chains_owned", Value::from(value));
    }

    pub fn chain_wear_pct(&self) -> f64 {
        self.condition_f64("chain_wear_pct", 0.0)
    }
    pub fn set_chain_wear_pct(&mut self, value: f64) {
        self.set_condition("chain_wear_pct", Value::from(value));
    }

    /// The Montana chain season (`mt_chain_season_id`) whose carry warning
    /// was last spoken, so it is said once a season, not once a drive.
    pub fn mt_chain_warned_season(&self) -> Option<i64> {
        self.condition()
            .and_then(|rec| rec.get("mt_chain_warned_season"))
            .and_then(Value::as_i64)
    }
    pub fn set_mt_chain_warned_season(&mut self, season: i64) {
        self.set_condition("mt_chain_warned_season", Value::from(season));
    }

    /// Spare repair links in the side box; a set comes with two
    /// (`CHAIN_REPAIR_LINKS_PER_SET`). Older saves read as a full kit.
    pub fn chain_repair_links(&self) -> i64 {
        self.condition_f64("chain_repair_links", CHAIN_REPAIR_LINKS_PER_SET as f64) as i64
    }
    pub fn set_chain_repair_links(&mut self, value: i64) {
        self.set_condition("chain_repair_links", Value::from(value.max(0)));
    }

    /// Put the saved rig condition onto a fresh `TruckState` at trip start.
    ///
    /// Fuel, incident damage, and the wear meters travel together so no sync
    /// site can pick up one and drop another.
    pub fn load_truck_condition(&self, truck: &mut TruckState) {
        truck.fuel_gal = self.truck_fuel_gal().min(truck.specs.fuel_tank_gal);
        truck.damage_pct = self.truck_damage_pct();
        truck.tire_wear_pct = self.tire_wear_pct();
        truck.brake_wear_pct = self.brake_wear_pct();
        truck.engine_wear_pct = self.engine_wear_pct();
        truck.tire_type = self.tire_type();
        truck.chain_wear_pct = self.chain_wear_pct();
    }

    /// Record a delivered from:to lane for dispatch-variety preference.
    pub fn remember_lane(&mut self, lane: &str) {
        if lane.is_empty() {
            return;
        }
        let mut lanes = vec![lane.to_string()];
        lanes.extend(self.recent_lanes.iter().filter(|e| *e != lane).cloned());
        lanes.truncate(RECENT_LANES_KEPT);
        self.recent_lanes = lanes;
    }

    /// Write the rig's current condition back to the profile for saving.
    pub fn store_truck_condition(&mut self, truck: &TruckState) {
        self.set_truck_fuel_gal(truck.fuel_gal);
        self.set_truck_damage_pct(truck.damage_pct);
        self.set_tire_wear_pct(truck.tire_wear_pct);
        self.set_brake_wear_pct(truck.brake_wear_pct);
        self.set_engine_wear_pct(truck.engine_wear_pct);
        // Tire type is chosen at the garage, never behind the wheel, so it only
        // flows profile-to-truck. Chain wear accrues while driving chained.
        self.set_chain_wear_pct(truck.chain_wear_pct);
    }

    /// Fatigue accrual multiplier from the active food or drink buff.
    ///
    /// 1.0 when nothing is active. `now_h` is the absolute game hour
    /// (game_hours plus trip minutes), the same clock the entries store.
    pub fn fatigue_buff_rate(&self, now_h: f64) -> f64 {
        for entry in &self.active_buffs {
            let Some(entry) = entry.as_object() else {
                continue;
            };
            if entry.get("group").and_then(Value::as_str) == Some("fatigue")
                && now_h < json_f64(entry.get("expires_h"), 0.0)
            {
                return json_f64(entry.get("rate"), 1.0);
            }
        }
        1.0
    }

    /// One active buff per group: the newest replaces its predecessor.
    pub fn add_timed_buff(&mut self, entry: Value) {
        let group = entry.get("group").cloned();
        self.active_buffs
            .retain(|b| b.get("group").cloned() != group);
        self.active_buffs.push(entry);
    }

    /// Drop timed buffs past their hour; returns them for announcing.
    pub fn expire_buffs(&mut self, now_h: f64) -> Vec<Value> {
        let expired: Vec<Value> = self
            .active_buffs
            .iter()
            .filter(|b| now_h >= json_f64(b.get("expires_h"), 0.0))
            .cloned()
            .collect();
        if !expired.is_empty() {
            self.active_buffs.retain(|b| !expired.contains(b));
        }
        expired
    }

    pub fn market_day(&self) -> i64 {
        (self.game_hours / 24.0).floor() as i64
    }

    pub fn calendar_game_hours(&self) -> f64 {
        self.game_hours + self.calendar_offset_days as f64 * 24.0 + self.calendar_offset_hours
    }

    /// The date and hour the player hears right now: the real clock while
    /// live weather drives the calendar, otherwise the career's own calendar.
    pub fn player_calendar_hours(&self) -> f64 {
        crate::sim::season::player_calendar_hours(
            self.game_hours,
            Some(self.calendar_game_hours()),
            self.live_calendar,
        )
    }

    /// Whether this profile has progressed beyond a just-created career.
    pub fn has_started_career(&self) -> bool {
        self.game_hours > 6.0 + 1e-6
            || self.calendar_offset_days != 0
            || self.tutorial_done
            || self.active_trip.is_some()
            || self.dispatch_board_cache.is_some()
            || self.money != STARTING_MONEY
            || !self.upgrades.is_empty()
            || self.pay_advance > 0.0
            || self.career.xp > 0.0
            || self.career.deliveries > 0
            || self.career.total_miles > 0.0
            || !self.achievements.is_empty()
    }

    /// Make the independent calendar show target's date without moving time.
    pub fn anchor_calendar_to(&mut self, target_game_hours: f64) {
        let target_day = ((target_game_hours / 24.0).floor() as i64).rem_euclid(365);
        let career_day = ((self.game_hours / 24.0).floor() as i64).rem_euclid(365);
        self.calendar_offset_days = (target_day - career_day).rem_euclid(365);
    }

    /// Make the independent calendar show target's date and time without
    /// moving career time, deadlines, or hours of service.
    pub fn sync_calendar_to(&mut self, target_game_hours: f64) {
        self.calendar_offset_hours = 0.0;
        self.anchor_calendar_to(target_game_hours);
        self.sync_calendar_hour_to(target_game_hours.rem_euclid(24.0));
    }

    /// Restore only the independent calendar's time of day. Resumed real-time
    /// trips carry their departure clock in the trip snapshot while the date
    /// remains in the profile's saved whole-day offset.
    pub fn sync_calendar_hour_to(&mut self, target_hour: f64) {
        self.calendar_offset_hours = 0.0;
        self.calendar_offset_hours =
            target_hour.rem_euclid(24.0) - self.calendar_game_hours().rem_euclid(24.0);
    }

    // -- persistence -----------------------------------------------------------

    /// `profile.path`: where this profile saves.
    pub fn path(&self) -> PathBuf {
        save_path_for(&self.name)
    }

    /// Write the signed, packed save atomically; returns its path.
    pub fn save(&self) -> std::io::Result<PathBuf> {
        let path = self.path();
        if !self.money_guard.matches(self.money) {
            origin::forget_origin(&self.name);
        }
        let tmp = path.with_extension("ffsave.tmp");
        std::fs::write(&tmp, encode_save_bytes(&self.to_dict()))?;
        std::fs::rename(&tmp, &path)?;
        // A converted legacy save keeps one plain-JSON copy as .json.bak so an
        // older game version can be rolled back to; the live file is packed.
        let legacy = path.with_extension("json");
        if legacy.exists() {
            let _ = std::fs::rename(&legacy, legacy.with_extension("json.bak"));
        }
        notify_save_listener(self);
        Ok(path)
    }

    /// `Profile.load(path)` for the running build.
    pub fn load(path: &Path) -> Result<Profile, LoadError> {
        Self::load_with(path, is_frozen())
    }

    /// `Profile.load(path)` with `is_frozen()` pinned (the skip-signing flag
    /// is honored only from source).
    pub fn load_with(path: &Path, frozen: bool) -> Result<Profile, LoadError> {
        let raw = std::fs::read(path)?;
        let (data, packed) = match decode_save_bytes(&raw) {
            Ok(decoded) => decoded,
            Err(err) => {
                // Unreadable beyond repair: move it aside so the picker's spoken
                // warning stays truthful and the file is not re-tried every visit.
                let _ = signing::quarantine(path);
                return Err(err.into());
            }
        };
        if is_pre_1_9_save(&data) {
            // A career from the 1.8 line or earlier. 1.9 starts everyone fresh
            // (the rebalanced arc cannot absorb old-scale careers), so refuse
            // before any signature check, migration, or resave can touch the
            // file: it stays intact on disk, still playable by the build that
            // wrote it.
            let name = match data.get("name") {
                Some(v) if py_truthy(v) => py_str(v),
                _ => "Driver".to_string(),
            };
            return Err(LegacyCareerError {
                name,
                path: Some(path.to_path_buf()),
            }
            .into());
        }
        let signed = data.contains_key(SIGNATURE_FIELD);
        let skip = signing_checks_disabled_when(frozen);
        let mut resign = false;
        let mut tampered = false;
        if signed && !is_signature_valid(&data) {
            if skip {
                // Re-save below so the file gets a valid local signature and
                // keeps loading once the dev flag is off again.
                log::warn!(
                    "Signature check skipped for {} (FREIGHT_FATE_SKIP_SAVE_SIGNING)",
                    path.display()
                );
                resign = true;
            } else {
                tampered = true;
            }
        } else if !signed && !skip {
            // Every build of the 1.9 line signs what it writes, and a save
            // from before the line was refused above, so a save that reaches
            // here unsigned had its signature taken off by hand -- packed or
            // plain. Plain unsigned JSON used to keep an amnesty as the shape
            // of saves from before signing; none of those load any more, and
            // the amnesty had become the easy way to edit a career: write the
            // numbers as JSON and the game signed them for you.
            tampered = true;
        }
        let mut profile = Profile::from_dict(&data);
        let money_impossible =
            plausibility::money_is_impossible(profile.money, profile.career.total_earnings);
        if !skip && money_impossible {
            // Signed or not: a balance rewritten while the game ran is signed
            // by the game itself, so the file looks honest and the number
            // does not.
            log::warn!(
                "{}: balance {} is more than {} of lifetime earnings could hold",
                path.display(),
                profile.money,
                profile.career.total_earnings
            );
            tampered = true;
        }
        if tampered || resign {
            // Only a copy from another computer (a good file whose signature
            // this data directory cannot check) records where it came from:
            // the raw dict, before migration or marking, is what the other
            // computer backed up. Any other reason forgets an earlier record.
            let copied = signed
                && !skip
                && !money_impossible
                && !data.get("integrity_modified").is_some_and(py_truthy);
            if copied {
                let (_, hash) = origin::cloud_content(&Value::Object(data.clone()));
                origin::record_origin(&profile.name, &hash);
            } else {
                origin::forget_origin(&profile.name);
            }
        }
        if tampered && !profile.integrity_modified {
            profile.integrity_modified = true;
            profile.integrity_notice_pending = true;
        }
        if profile.needs_migration_resave || resign || !signed || tampered || !packed {
            profile.save()?;
        }
        Ok(profile)
    }

    /// Every save on disk, newest first; a packed save shadows a leftover
    /// legacy twin of the same career.
    pub fn list_saves() -> Vec<PathBuf> {
        let dir = profiles_dir();
        let mut found: IndexMap<String, PathBuf> = IndexMap::new();
        let entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        for suffix in [LEGACY_SAVE_SUFFIX, SAVE_SUFFIX] {
            for path in &entries {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string());
                let Some(name) = name else { continue };
                if let Some(stem) = name.strip_suffix(suffix) {
                    found.insert(stem.to_string(), path.clone());
                }
            }
        }
        let mut saves: Vec<PathBuf> = found.into_values().collect();
        saves.sort_by_key(|p| {
            std::cmp::Reverse(
                p.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            )
        });
        saves
    }

    /// Remove this profile's save files (packed and legacy).
    pub fn delete(&self) {
        let path = self.path();
        origin::forget_origin(&self.name);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("json"));
    }
}
