//! Career stats screen: the terminal's driver record as a reviewable menu
//! (port of `freight_fate/states/career_stats.py`).

use ff_core::models::business_constants::is_owner_operator;
use ff_core::models::career::xp_to_next_level;
use ff_core::models::carrier_fleet::equipment_status_lines;
use ff_core::models::co_driver;
use ff_core::models::credentials::{credential, CredentialTier, CREDENTIALS};
use ff_core::models::enforcement;
use ff_core::models::profile::Profile;
use ff_core::models::solvency::debt_line;
use ff_core::pyfmt::{fmt_f, fmt_grouped};

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem, SimpleMenuState};
use ff_core::sim::hos::clock_text;
use serde_json::Value;
use std::collections::BTreeMap;

/// Fresh hours of service and zero fatigue: sleeping gains nothing but time.
pub fn fully_rested(profile: &Profile) -> bool {
    profile.hos.driving_min <= 0.0 && profile.hos.duty_min <= 0.0 && profile.fatigue <= 0.0
}

/// Career stats as a list of lines, matching the driving status screens.
pub struct CareerStatsState {
    menu: MenuCore<Self>,
}

impl CareerStatsState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Career stats").with_intro_help(
                "Up and down review the lines. Enter repeats a line. Escape returns to the \
                 terminal.",
            ),
        }
    }

    /// The spoken lines, in order (`_lines`).
    pub fn stat_lines(ctx: &GameContext) -> Vec<String> {
        let Some(p) = ctx.profile.as_ref() else {
            return Vec::new();
        };
        let s = &ctx.settings;
        let career = &p.career;
        let pct = if career.deliveries != 0 {
            100.0 * career.on_time_deliveries as f64 / career.deliveries as f64
        } else {
            100.0
        };
        let rest = if fully_rested(p) {
            "fully rested".to_string()
        } else {
            format!("fatigue {} percent", fmt_f(p.fatigue, 0))
        };
        let hours_now = p.game_hours;
        // Earned credentials were only ever spoken once, at the level-up
        // that granted them; these lines are the reviewable record (owner
        // got stuck declining a reefer load he was already cleared to
        // haul). One line per ladder tier, in tier order, plus what is
        // still waiting on a background check.
        let held = career.endorsements();
        let tier_line = |tier: CredentialTier, noun: &str, empty: &str| {
            let mut labels: Vec<&str> = CREDENTIALS
                .iter()
                .filter(|c| c.tier == tier && held.contains(c.key))
                .map(|c| c.label)
                .collect();
            labels.sort_unstable();
            if labels.is_empty() {
                format!("{noun}: {empty}")
            } else {
                format!("{noun}: {}", labels.join(", "))
            }
        };
        let certificates = tier_line(CredentialTier::Certificate, "Certificates", "none yet");
        let endorsements = if held.contains("tank") && held.contains("hazmat") {
            let line = tier_line(CredentialTier::Endorsement, "Endorsements", "none yet");
            format!("{line}, the X combination")
        } else {
            tier_line(CredentialTier::Endorsement, "Endorsements", "none yet")
        };
        let mut specialist: Vec<&str> = CREDENTIALS
            .iter()
            .filter(|c| {
                held.contains(c.key)
                    && matches!(
                        c.tier,
                        CredentialTier::Specialist | CredentialTier::Training
                    )
            })
            .map(|c| c.label)
            .collect();
        specialist.sort_unstable();
        let mut pending: Vec<String> = career
            .pending_credentials
            .iter()
            .filter_map(|p| {
                let cred = credential(&p.key)?;
                let days = ((p.ready_at_h - hours_now) / 24.0).ceil().max(0.0);
                Some(format!(
                    "{} background check in progress, about {} days left",
                    cred.gate_label,
                    fmt_f(days, 0)
                ))
            })
            .collect();
        pending.sort();
        // Money was reviewable nowhere on this screen, which left a player
        // asking "how much do I owe" with no way to find out short of opening
        // a fuel menu. Balance is always here now; what is owed joins it only
        // when it is real. The slower career rate rides the trust line, which
        // is on this screen already.
        let owed = debt_line(p);
        let level = career.level();
        let next = match xp_to_next_level(career.xp) {
            Some(xp_owed) => format!(", {} to level {}", fmt_grouped(xp_owed, 0), level + 1),
            None => ", top level".to_string(),
        };
        let mut lines = vec![
            format!(
                "Level {level} driver, {} experience{next}",
                fmt_f(career.xp, 0)
            ),
            format!(
                "Reputation: {}",
                enforcement::reputation_text(p.standing(), ctx.settings.score_display)
            ),
            enforcement::dispatch_trust_line_with_display(p, ctx.settings.score_display),
            enforcement::career_menu_status(p),
            enforcement::standing_text(p),
        ];
        // What the driver is actually in, and what stands between them and
        // the next tier. The hold was spoken at the dispatch hand-over and at
        // the level-up that brought no truck, and nowhere a player could go
        // and ASK -- which is what this screen is for (Brandon, 2026-08-22).
        lines.extend(equipment_status_lines(p));
        lines.push(format!("Balance: {} dollars", fmt_grouped(p.money(), 0)));
        if !owed.is_empty() {
            lines.push(owed);
        }
        lines.push(certificates);
        lines.push(endorsements);
        if !specialist.is_empty() {
            lines.push(format!("Training and cards: {}", specialist.join(", ")));
        }
        lines.extend(pending);
        lines.push(format!(
            "Deliveries: {}, {} percent on time",
            career.deliveries,
            fmt_f(pct, 0)
        ));
        lines.push(format!(
            "Lifetime {}: {}",
            s.distance_unit_text(true),
            s.distance_value(career.total_miles, 0, true)
        ));
        let places = visited_places(ctx);
        let states = places.len();
        let cities: usize = places.iter().map(|(_, cities)| cities.len()).sum();
        lines.push(format!("States visited: {states}"));
        lines.push(format!("Cities visited: {cities}"));
        lines.push(format!(
            "Lifetime earnings: {} dollars",
            fmt_grouped(career.total_earnings, 0)
        ));
        lines.push(format!("Rest: {rest}"));
        let hos = p.hos.summary(&ctx.settings.hos_mode);
        let summary = hos.trim_end_matches('.');
        lines.push(format!("Hours: {summary}"));
        lines
    }
}

impl Default for CareerStatsState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for CareerStatsState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        for line in Self::stat_lines(ctx) {
            // The list opens from the row after the count it backs, where
            // the driver profile has it too.
            let after = line.starts_with("Cities visited:");
            let spoken = line.clone();
            items.push(
                MenuItem::new(line, move |_s: &mut Self, ctx| ctx.say(&spoken))
                    .help("Repeat this status line."),
            );
            if after {
                items.push(
                    MenuItem::new("Places visited", |_s: &mut Self, ctx| {
                        let lines = places_lines(ctx);
                        ctx.push_state(SimpleMenuState::readout("Places visited", lines));
                    })
                    .help("Open the list of states you have delivered to, each with its cities."),
                );
            }
        }
        // Who you have teamed with, for a player who wants the story. Only
        // there once a team load is done, and never read unless opened.
        if ctx
            .profile
            .as_ref()
            .is_some_and(|p| co_driver::team_loads(&p.achievement_stats) > 0)
        {
            items.push(
                MenuItem::new("Co-drivers", |_s: &mut Self, ctx| {
                    let lines = co_driver_lines(ctx);
                    ctx.push_state(SimpleMenuState::readout("Co-drivers", lines));
                })
                .help("Open the list of co-drivers you have teamed with, each with their story."),
            );
        }
        items.push(
            MenuItem::new("Citations and violations", |_s: &mut Self, ctx| {
                let lines = record_lines(ctx);
                ctx.push_state(SimpleMenuState::readout("Citations and violations", lines));
            })
            .help(
                "Open the list, newest first: what it was, why, what it cost, when, \
                 and where.",
            ),
        );
        items.push(
            MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Back to the terminal menu."),
        );
        items
    }
}

impl_state_for_menu!(CareerStatsState);

/// The cities this career has delivered to, grouped under their states,
/// states and cities each in alphabetical order. The save keeps city keys;
/// one the world no longer has is left out rather than read as a key. The
/// online profile groups the same list the same way.
pub fn visited_places(ctx: &GameContext) -> Vec<(String, Vec<String>)> {
    let Some(p) = ctx.profile.as_ref() else {
        return Vec::new();
    };
    let keys = p
        .achievement_stats
        .get("cities_delivered")
        .and_then(Value::as_array);
    let mut by_state: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for key in keys.into_iter().flatten().filter_map(Value::as_str) {
        if let Ok(city) = ctx.world.city(key) {
            let cities = by_state.entry(city.state.clone()).or_default();
            if !cities.contains(&city.name) {
                cities.push(city.name.clone());
            }
        }
    }
    by_state
        .into_iter()
        .map(|(state, mut cities)| {
            cities.sort_unstable();
            (state, cities)
        })
        .collect()
}

/// One line per state, its cities after it.
pub fn places_lines(ctx: &GameContext) -> Vec<String> {
    let lines: Vec<String> = visited_places(ctx)
        .into_iter()
        .map(|(state, cities)| format!("{state}: {}", cities.join(", ")))
        .collect();
    if lines.is_empty() {
        vec!["No deliveries yet.".to_string()]
    } else {
        lines
    }
}

/// One line per co-driver the career has teamed with, in cast order: the
/// loads run together, whether they are this owner-operator's hire, and
/// their story.
pub fn co_driver_lines(ctx: &GameContext) -> Vec<String> {
    let Some(p) = ctx.profile.as_ref() else {
        return Vec::new();
    };
    let stats = &p.achievement_stats;
    // The hire is only theirs while they run their own authority; back on
    // a carrier's payroll, the carrier pairs co-drivers again.
    let hire = is_owner_operator(&p.business_status)
        .then(|| co_driver::hired_co_driver(stats).map(|member| member.key))
        .flatten();
    co_driver::teamed_with(stats)
        .into_iter()
        .map(|(member, loads)| {
            let name = member.name;
            let bio = member.bio;
            // Whole lines, so a translation matches each as it is spoken.
            match (hire == Some(member.key), loads == 1) {
                (true, true) => format!("{name}, your hire, 1 team load. {bio}"),
                (true, false) => format!("{name}, your hire, {loads} team loads. {bio}"),
                (false, true) => format!("{name}, 1 team load. {bio}"),
                (false, false) => format!("{name}, {loads} team loads. {bio}"),
            }
        })
        .collect()
}

/// The record's entries as spoken lines, newest first, then the counts that
/// no entry explains.
pub fn record_lines(ctx: &GameContext) -> Vec<String> {
    let Some(p) = ctx.profile.as_ref() else {
        return Vec::new();
    };
    let record = &p.driving_record;
    let mut lines: Vec<String> = record
        .entries
        .iter()
        .rev()
        .map(|entry| {
            let what = match entry.kind.as_str() {
                enforcement::RECORD_SERIOUS => "Serious violation",
                enforcement::RECORD_MAJOR => "Major offense",
                enforcement::RECORD_FATIGUE => "Safety incident",
                enforcement::RECORD_CRASH => "Crash",
                _ => "Citation",
            };
            let day = (entry.game_hours / 24.0).floor() as i64 + 1;
            let mut line = format!(
                "{what}, day {day}, {}: {}.",
                clock_text(entry.game_hours),
                entry.reason
            );
            if entry.fine > 0.0 {
                line.push_str(&format!(" {} dollars.", fmt_grouped(entry.fine, 0)));
            }
            if !entry.place.is_empty() {
                line.push_str(&format!(" On {}.", entry.place));
            }
            line
        })
        .collect();
    let unexplained = record.unexplained_citations();
    if unexplained > 0 {
        lines.push(format!(
            "{} earlier {} recorded before reasons were kept.",
            unexplained,
            if unexplained == 1 {
                "citation"
            } else {
                "citations"
            }
        ));
    }
    if lines.is_empty() {
        lines.push("No citations or violations on your record.".to_string());
    }
    lines
}
