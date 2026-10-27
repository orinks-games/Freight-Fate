//! One driver's public profile as a spoken list: the screen behind Enter on
//! the drivers list, and behind Your profile on the Online menu.
//!
//! The rows are the profile page on orinks.net read aloud in the order the
//! online-profile design fixed: who they are, then how their career is
//! going, then what they have earned, one fact per row so a screen reader
//! user can re-read the one they wanted. Every value is the site's word; the
//! game computes nothing and shows nothing the page does not.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use ff_core::models::enforcement;
use ff_core::models::solvency::money_text;
use ff_core::pyfmt::{fmt_grouped, py_int};
use ff_core::settings::ScoreDisplay;
use serde_json::Value;

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::online_presence::{self, ProfileFetch};
use crate::states::base::{Menu, MenuCore, MenuItem, SimpleMenuState};

use super::board::updated_text;
use super::support::{menu_default_enter, online_transport, run_worker, Mailbox};

// -- reading the site's JSON --------------------------------------------------------------

/// A non-empty string field, or nothing.
fn text(entry: &Value, key: &str) -> Option<String> {
    match entry.get(key) {
        Some(Value::String(s)) if !s.trim().is_empty() => Some(s.trim().to_string()),
        Some(Value::Number(n)) => Some(online_presence::py_str(&Value::Number(n.clone()))),
        _ => None,
    }
}

/// A numeric field, or nothing.
fn num(entry: &Value, key: &str) -> Option<f64> {
    match entry.get(key) {
        Some(Value::Number(n)) => n.as_f64(),
        Some(Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

fn flag(entry: &Value, key: &str) -> Option<bool> {
    entry.get(key).and_then(Value::as_bool)
}

/// `1,234` for a count the site may have sent as a float.
fn whole(value: f64) -> String {
    fmt_grouped(value.round(), 0)
}

/// `1 citation` / `2 citations`.
fn counted(count: f64, singular: &str, plural: &str) -> String {
    let n = py_int(count);
    format!(
        "{} {}",
        whole(count),
        if n == 1 { singular } else { plural }
    )
}

/// The safety record as one spoken row, the way the page lists it.
fn safety_record_text(record: &Value) -> String {
    let mut parts = vec![
        counted(
            num(record, "citations").unwrap_or(0.0),
            "citation",
            "citations",
        ),
        counted(
            num(record, "seriousViolations").unwrap_or(0.0),
            "serious violation",
            "serious violations",
        ),
        counted(
            num(record, "majorOffenses").unwrap_or(0.0),
            "major offense",
            "major offenses",
        ),
    ];
    // Public by owner ruling 2026-09-24 (FMCSA publishes inspection
    // out-of-service results); the reason behind an order stays private.
    if let Some(orders) = num(record, "outOfServiceOrders") {
        parts.push(counted(
            orders,
            "out-of-service order",
            "out-of-service orders",
        ));
    }
    // A crash on the accident register (49 CFR 390.15), such as a rollover:
    // read when the site sends it, like the out-of-service count.
    if let Some(crashes) = num(record, "crashes") {
        parts.push(counted(crashes, "crash", "crashes"));
    }
    if let Some(claims) = num(record, "cargoClaims") {
        parts.push(counted(claims, "cargo claim", "cargo claims"));
    }
    if let Some(damage) = num(record, "preventableEquipmentDamage") {
        parts.push(counted(
            damage,
            "preventable equipment damage incident",
            "preventable equipment damage incidents",
        ));
    }
    parts.push(counted(
        num(record, "carrierTerminations").unwrap_or(0.0),
        "carrier termination",
        "carrier terminations",
    ));
    parts.push(counted(
        num(record, "repossessions").unwrap_or(0.0),
        "repossession",
        "repossessions",
    ));
    parts.join(", ")
}

/// The spoken rows for a profile the site sent, in the design's order. Back
/// is not among them; the screen adds it.
pub fn profile_rows(profile: &Value, score_display: ScoreDisplay) -> Vec<String> {
    let mut rows = Vec::new();
    let driver = profile.get("driver").cloned().unwrap_or(Value::Null);
    rows.push(text(&driver, "displayName").unwrap_or_else(|| "A driver".to_string()));

    match profile.get("presence").filter(|p| p.is_object()) {
        Some(presence) => {
            let mut bits = vec!["On duty".to_string()];
            bits.extend(text(presence, "activity"));
            bits.extend(text(presence, "detail"));
            if let Some(updated) = num(presence, "updatedAt") {
                bits.push(updated_text(updated));
            }
            rows.push(bits.join(". "));
        }
        None => rows.push("Off duty".to_string()),
    }

    let Some(snapshot) = profile.get("snapshot").filter(|s| s.is_object()) else {
        rows.push("No career shared yet".to_string());
        rows.extend(achievement_rows(profile));
        rows.extend(journal_rows(profile));
        return rows;
    };

    if let Some(name) = text(snapshot, "saveName") {
        rows.push(format!("Current career: {name}"));
    }
    // A career that is over says so before the employment it no longer has.
    if snapshot
        .get("careerEnded")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        rows.push("Career ended: CDL disqualified for life".to_string());
    }
    if let Some(employment) =
        text(snapshot, "businessIdentity").or_else(|| text(snapshot, "employmentStatus"))
    {
        rows.push(format!("Employment: {employment}"));
    }
    if let Some(carrier) = text(snapshot, "carrierName") {
        rows.push(format!("Carrier: {carrier}"));
    }
    match (num(snapshot, "level"), text(snapshot, "careerTitle")) {
        (Some(level), Some(title)) => rows.push(format!("Level {}, {title}", py_int(level))),
        (Some(level), None) => rows.push(format!("Level {}", py_int(level))),
        (None, Some(title)) => rows.push(title),
        (None, None) => {}
    }
    if let Some(truck) = text(snapshot, "truckName") {
        rows.push(match flag(snapshot, "truckIsCarrierAssigned") {
            Some(true) => format!("Assigned truck: {truck}"),
            Some(false) => format!("Owned truck: {truck}"),
            None => format!("Truck: {truck}"),
        });
    }
    if let Some(tier) = text(snapshot, "fleetTier") {
        rows.push(format!("Carrier fleet tier: {tier}"));
    }

    rows.push("Current career resume".to_string());
    if let Some(n) = num(snapshot, "deliveries") {
        rows.push(format!("Lifetime deliveries: {}", whole(n)));
    }
    if let Some(n) = num(snapshot, "milesDriven") {
        rows.push(format!("Lifetime miles: {}", whole(n)));
    }
    if let Some(p) = num(snapshot, "onTimeRate") {
        rows.push(format!("On time: {} percent", whole(p)));
    }
    if let Some(p) = num(snapshot, "damageFreeRate") {
        rows.push(format!("Damage free: {} percent", whole(p)));
    }
    if let Some(record) = snapshot.get("safetyRecord").filter(|r| r.is_object()) {
        rows.push(format!("Safety record: {}", safety_record_text(record)));
    }
    if let Some(n) = num(snapshot, "statesVisited") {
        rows.push(format!("States visited: {}", whole(n)));
    }
    if let Some(n) = num(snapshot, "citiesVisited") {
        rows.push(format!("Cities visited: {}", whole(n)));
    }
    if let Some(n) = num(snapshot, "longestHaulMiles") {
        rows.push(format!("Longest haul: {} miles", whole(n)));
    }
    if let Some(n) = num(snapshot, "lifetimeEarnings") {
        rows.push(format!("Lifetime career earnings: {}", money_text(n)));
    }
    // The page shows net worth only once every part of it is known; a
    // partial figure would read as a smaller fortune than the driver has.
    if let (Some(n), Some(true)) = (
        num(snapshot, "netWorth"),
        flag(snapshot, "netWorthComplete"),
    ) {
        rows.push(format!("Net worth: {}", money_text(n)));
    }
    if let Some(n) = num(snapshot, "reputation") {
        rows.push(format!(
            "Reputation: {}",
            enforcement::reputation_text(n, score_display)
        ));
    }
    if let Some(Value::Array(endorsements)) = snapshot.get("endorsements") {
        let names: Vec<String> = endorsements
            .iter()
            .filter_map(|e| e.as_str().map(str::to_string))
            .collect();
        rows.push(if names.is_empty() {
            "Endorsements: none yet".to_string()
        } else {
            format!("Endorsements: {}", names.join(", "))
        });
    }

    rows.extend(achievement_rows(profile));
    rows.extend(journal_rows(profile));
    rows
}

/// The account-wide total and the most recent few, labelled as such.
fn achievement_rows(profile: &Value) -> Vec<String> {
    let count = num(profile, "achievementCount").unwrap_or(0.0);
    if py_int(count) <= 0 {
        return vec!["No achievements yet".to_string()];
    }
    let mut rows = vec![format!(
        "Achievements across every career: {}",
        whole(count)
    )];
    if let Some(Value::Array(recent)) = profile.get("recentAchievements") {
        // The site sends what the badge was for beside its name; a row
        // without one (an older server) still reads as the name alone.
        rows.extend(recent.iter().filter_map(|item| {
            let label = text(item, "label")?;
            Some(match text(item, "description") {
                Some(description) if !description.trim().is_empty() => {
                    format!("Recent achievement: {label}. {}", description.trim())
                }
                _ => format!("Recent achievement: {label}"),
            })
        }));
    }
    rows
}

/// The places a driver has delivered to, one line per state with its cities,
/// in the site's order. `None` when the site sent no list (no shared career,
/// or a snapshot from before the list existed), so the screen offers no row.
pub fn places_rows(profile: &Value) -> Option<Vec<String>> {
    let places = profile.get("snapshot")?.get("visitedPlaces")?.as_array()?;
    let rows: Vec<String> = places
        .iter()
        .filter_map(|place| {
            let state = text(place, "state")?;
            let cities: Vec<String> = match place.get("cities") {
                Some(Value::Array(cities)) => cities
                    .iter()
                    .filter_map(|city| city.as_str().map(str::trim))
                    .filter(|city| !city.is_empty())
                    .map(str::to_string)
                    .collect(),
                _ => Vec::new(),
            };
            Some(if cities.is_empty() {
                state
            } else {
                format!("{state}: {}", cities.join(", "))
            })
        })
        .collect();
    Some(if rows.is_empty() {
        vec!["No deliveries yet.".to_string()]
    } else {
        rows
    })
}

/// The last few road-journal lines.
fn journal_rows(profile: &Value) -> Vec<String> {
    let lines: Vec<String> = match profile.get("events") {
        Some(Value::Array(events)) => events
            .iter()
            .filter_map(|event| text(event, "summary"))
            .map(|summary| format!("Road journal: {summary}"))
            .collect(),
        _ => Vec::new(),
    };
    if lines.is_empty() {
        vec!["No road journal entries yet".to_string()]
    } else {
        lines
    }
}

/// The one-line headline spoken when the profile lands: the name, then the
/// career in a breath. The rows carry the rest.
fn headline(profile: &Value) -> String {
    let driver = profile.get("driver").cloned().unwrap_or(Value::Null);
    let name = text(&driver, "displayName").unwrap_or_else(|| "A driver".to_string());
    let Some(snapshot) = profile.get("snapshot").filter(|s| s.is_object()) else {
        return format!("{name}. No career shared yet.");
    };
    let mut bits = vec![name];
    match (num(snapshot, "level"), text(snapshot, "careerTitle")) {
        (Some(level), Some(title)) => bits.push(format!("Level {}, {title}", py_int(level))),
        (Some(level), None) => bits.push(format!("Level {}", py_int(level))),
        (None, Some(title)) => bits.push(title),
        (None, None) => {}
    }
    if let Some(employment) =
        text(snapshot, "businessIdentity").or_else(|| text(snapshot, "employmentStatus"))
    {
        bits.push(employment);
    }
    format!("{}.", bits.join(". "))
}

// -- DriverProfileState -------------------------------------------------------------------

/// A driver's public profile, read as a menu.
pub struct DriverProfileState {
    pub menu: MenuCore<Self>,
    driver_id: String,
    /// The drivers-list row this was opened from, so the name is on screen
    /// before the site answers. Absent for Your profile.
    seed: Option<Value>,
    /// The player's own profile (Your profile on the Online menu). Same
    /// screen, but a hidden answer is worded as something they can change.
    own: bool,
    /// The site's answer once it lands; `None` until then.
    pub profile: Option<ProfileFetch>,
    result: Mailbox<ProfileFetch>,
    fetched: Arc<AtomicBool>,
    announced: bool,
    pub threaded: bool,
}

impl DriverProfileState {
    pub const TITLE: &'static str = "Driver profile";
    pub const OWN_TITLE: &'static str = "Your profile";

    pub fn new(_ctx: &mut GameContext, driver_id: &str, seed: Option<Value>, own: bool) -> Self {
        Self {
            menu: MenuCore::new(if own { Self::OWN_TITLE } else { Self::TITLE }),
            driver_id: driver_id.to_string(),
            seed,
            own,
            profile: None,
            result: Mailbox::new(),
            fetched: Arc::new(AtomicBool::new(false)),
            announced: false,
            threaded: true,
        }
    }

    /// Whose profile this is.
    pub fn driver_id(&self) -> &str {
        &self.driver_id
    }

    /// Whether the fetch has answered.
    pub fn fetched(&self) -> bool {
        self.fetched.load(Ordering::SeqCst)
    }

    fn start_fetch(&mut self) {
        self.profile = None;
        self.result = Mailbox::new();
        self.fetched = Arc::new(AtomicBool::new(false));
        self.announced = false;
        let result = self.result.clone();
        let fetched = Arc::clone(&self.fetched);
        let transport = online_transport();
        let driver_id = self.driver_id.clone();
        run_worker(self.threaded, "online-profile", move || {
            result.post(online_presence::fetch_driver_profile(
                &driver_id,
                transport.as_ref(),
            ));
            fetched.store(true, Ordering::SeqCst);
        });
    }

    /// Move a landed fetch out of the mailbox.
    fn absorb(&mut self) {
        if !self.fetched() {
            return;
        }
        if let Some(answer) = self.result.take() {
            self.profile = Some(answer);
        }
    }

    /// The name known before the site answers: the drivers-list row, or,
    /// for Your profile, nothing yet.
    fn seed_name(&self) -> Option<String> {
        self.seed.as_ref().and_then(|row| text(row, "displayName"))
    }

    /// What a hidden or unreachable answer says, in the player's terms.
    fn outcome_line(&self, fetch: &ProfileFetch) -> String {
        match (fetch, self.own) {
            (ProfileFetch::NotPublic, true) => {
                "Your profile is not public. Turn Profile sharing on, on the Online menu, \
                 to share it"
                    .to_string()
            }
            (ProfileFetch::NotPublic, false) => "This driver has no public profile".to_string(),
            (ProfileFetch::Unreachable, true) => "Your profile could not be reached".to_string(),
            (ProfileFetch::Unreachable, false) => "The profile could not be reached".to_string(),
            (ProfileFetch::Profile(_), _) => String::new(),
        }
    }
}

impl Menu for DriverProfileState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        self.start_fetch();
        menu_default_enter(self, ctx);
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        self.absorb();
        let speak = |s: &mut Self, ctx: &mut GameContext| s.speak_current(ctx);
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        match self.profile.as_ref() {
            None => {
                if let Some(name) = self.seed_name() {
                    items.push(MenuItem::new(name, speak));
                }
                items.push(MenuItem::new("Checking the profile", speak));
            }
            Some(ProfileFetch::Profile(profile)) => {
                let mut places = places_rows(profile);
                for row in profile_rows(profile, ctx.settings.score_display) {
                    // The list opens from the row after the count it backs.
                    let after = row.starts_with("Cities visited:");
                    items.push(MenuItem::new(row, speak));
                    if after {
                        if let Some(lines) = places.take() {
                            items.push(places_item(lines));
                        }
                    }
                }
                if let Some(lines) = places {
                    items.push(places_item(lines));
                }
            }
            Some(other) => {
                if let Some(name) = self.seed_name() {
                    items.push(MenuItem::new(name, speak));
                }
                items.push(MenuItem::new(self.outcome_line(other), speak));
            }
        }
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        if !self.fetched() || self.announced {
            return;
        }
        self.announced = true;
        self.refresh(ctx, false);
        let line = match self.profile.as_ref() {
            Some(ProfileFetch::Profile(profile)) => headline(profile),
            Some(other) => format!("{}.", self.outcome_line(other)),
            None => return,
        };
        ctx.say(&line);
    }
}

impl_state_for_menu!(DriverProfileState);

/// The row that opens a driver's places visited as a list to review.
fn places_item(lines: Vec<String>) -> MenuItem<DriverProfileState> {
    MenuItem::new("Places visited", move |_s: &mut DriverProfileState, ctx| {
        ctx.push_state(SimpleMenuState::readout("Places visited", lines.clone()));
    })
    .help("Open the list of states this driver has delivered to, each with its cities.")
}
