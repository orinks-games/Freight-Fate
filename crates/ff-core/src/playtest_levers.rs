//! Environment-variable playtest levers, never active in normal play (port of
//! `freight_fate/playtest_levers.py`).
//!
//! These exist for the alpha test book: a tester relocates a parked career or
//! moves the clock forward so a scenario (a night snow run over the Rockies,
//! say) can be reached without hours of setup driving. Each lever speaks what
//! it did in plain language, moves no miles and no money, and refuses to touch
//! a career that has a load in progress. A future shared-profile event ledger
//! must record forced relocations and clock moves so shared saves stay honest
//! (docs/profile-invariants.md).
//!
//! `FREIGHT_FATE_FORCE_CITY`
//!     Relocate a parked career to a city (slug or display name) when it is
//!     loaded from the main menu.
//! `FREIGHT_FATE_FORCE_CLOCK`
//!     Advance the career clock forward to the next occurrence of a local
//!     wall-clock hour (0-23) when the career is loaded. Ten or more hours
//!     of waiting counts as a full break, like sleeping at the terminal.
//! `FREIGHT_FATE_FORCE_DEST`
//!     Guarantee the dispatch board offers a load to a destination, and put
//!     that load first in line when dispatch assigns loads.
//! `FREIGHT_FATE_FORCE_PERSIST`
//!     Set to 1 to make a lever session permanent. WITHOUT it, any lever
//!     session is a SANDBOX (owner design 2026-07-15): the whole run plays on
//!     the loaded career in memory and nothing is ever saved -- quit, launch
//!     normally, and the career is exactly where it was, same city, same
//!     date, same money. A tester teleports somewhere, breaks whatever the
//!     scenario needs, and their real save never knows.

use regex::Regex;

use crate::data::world::World;
use crate::models::profile::Profile;
use crate::sim::hos::{clock_text, time_of_day};
use crate::sim::timezones::{city_zone, to_local};

pub const CITY_ENV: &str = "FREIGHT_FATE_FORCE_CITY";
pub const CLOCK_ENV: &str = "FREIGHT_FATE_FORCE_CLOCK";
pub const DEST_ENV: &str = "FREIGHT_FATE_FORCE_DEST";
pub const PERSIST_ENV: &str = "FREIGHT_FATE_FORCE_PERSIST";

fn env_trimmed(name: &str) -> String {
    std::env::var(name)
        .map(|v| v.trim().to_string())
        .unwrap_or_default()
}

pub fn forced_city() -> String {
    env_trimmed(CITY_ENV)
}

/// `forced_clock_hour()`: parses the raw value the way `float()` does.
pub fn forced_clock_hour() -> Option<f64> {
    parse_clock_hour(&env_trimmed(CLOCK_ENV))
}

/// The clock lever's parsing rule on an already-trimmed value.
pub fn parse_clock_hour(raw: &str) -> Option<f64> {
    if raw.is_empty() {
        return None;
    }
    let hour: f64 = raw.parse().ok()?;
    if !(0.0..24.0).contains(&hour) {
        return None;
    }
    Some(hour)
}

pub fn forced_dispatch_destination() -> String {
    env_trimmed(DEST_ENV)
}

pub fn persist_requested() -> bool {
    let raw = env_trimmed(PERSIST_ENV);
    !(raw.is_empty() || raw == "0")
}

/// The slice of `GameContext` the levers touch.
pub trait LeverContext {
    fn world(&self) -> &World;
    fn profile_mut(&mut self) -> &mut Profile;
    /// `ctx.playtest_sandbox = True`.
    fn set_playtest_sandbox(&mut self, sandbox: bool);
    /// `ctx.save_profile()`.
    fn save_profile(&mut self);
}

/// Relocation and clock levers, applied as a saved career resumes.
///
/// Only a parked career with no accepted load moves; anything mid-trip keeps
/// its state. Returns the spoken notes for the caller to queue after its own
/// entry announcement (entry speech interrupts, and a lever note must never
/// be lost to it).
///
/// Any lever session is a sandbox unless `FREIGHT_FATE_FORCE_PERSIST` says
/// otherwise: `ctx.playtest_sandbox` goes True and `save_profile` becomes a
/// no-op for the whole run, so the career file on disk never learns the
/// scenario happened.
pub fn apply_continue_levers<C: LeverContext + ?Sized>(ctx: &mut C) -> Vec<String> {
    let city = forced_city();
    let clock = forced_clock_hour();
    if city.is_empty() && clock.is_none() && forced_dispatch_destination().is_empty() {
        return Vec::new();
    }
    if ctx.profile_mut().active_trip.is_some() {
        if city.is_empty() && clock.is_none() {
            // A forced dispatch destination alone has nothing to do until
            // the board opens; it neither moves nor sandboxes a live load.
            return Vec::new();
        }
        return vec![
            "Playtest lever ignored: this career has a load in progress. \
             Deliver or abandon it first."
                .to_string(),
        ];
    }
    let mut notes: Vec<String> = Vec::new();
    if !city.is_empty() {
        notes.extend(apply_city(ctx, &city));
    }
    if let Some(hour) = clock {
        notes.extend(apply_clock(ctx, hour));
    }
    if !persist_requested() {
        ctx.set_playtest_sandbox(true);
        notes.push(
            "Playtest sandbox: nothing this session is saved. Your career \
             resumes untouched next time you play normally."
                .to_string(),
        );
    } else if !notes.is_empty() {
        ctx.save_profile();
    }
    notes
}

/// Resolve a tester-typed city: exact slug or display name first, then a
/// slugified retry so "holbrook,az,us", "Holbrook, AZ, US", and the
/// PowerShell-array casualty "holbrook az us" all land on holbrook_az_us.
pub fn resolve_city_forgiving(world: &World, city: &str) -> String {
    let key = world.resolve_city_key(city);
    if world.cities.contains_key(&key) {
        return key;
    }
    let slug_re = Regex::new(r"[^a-z0-9]+").expect("a valid regex");
    let slugged = slug_re
        .replace_all(&city.to_lowercase(), "_")
        .trim_matches('_')
        .to_string();
    let key = world.resolve_city_key(&slugged);
    if world.cities.contains_key(&key) {
        key
    } else {
        city.to_string()
    }
}

/// Move a parked career to `city` with no miles driven (the city lever; the
/// agent server's scenario tool reuses it).
pub fn apply_city<C: LeverContext + ?Sized>(ctx: &mut C, city: &str) -> Vec<String> {
    let key = resolve_city_forgiving(ctx.world(), city);
    if !ctx.world().cities.contains_key(&key) {
        return vec![format!(
            "Playtest lever: no city called {city}. Staying put."
        )];
    }
    let current_city = ctx.profile_mut().current_city.clone();
    if key == ctx.world().resolve_city_key(&current_city) {
        return Vec::new();
    }
    let spoken = ctx.world().spoken_city(&key, Some(true));
    let p = ctx.profile_mut();
    p.current_city = key;
    p.dispatch_board_cache = None;
    vec![format!(
        "Playtest lever: relocated to {spoken}. No miles driven, no money changed."
    )]
}

/// Move the local clock forward to `hour`, logging the wait off duty and
/// counting a ten-hour wait as a full break (the clock lever; the agent
/// server's scenario tool reuses it).
pub fn apply_clock<C: LeverContext + ?Sized>(ctx: &mut C, hour: f64) -> Vec<String> {
    let current_city = ctx.profile_mut().current_city.clone();
    let Ok(city_obj) = ctx.world().city(&current_city) else {
        return vec![
            "Playtest lever: cannot read the local clock here. Clock unchanged.".to_string(),
        ];
    };
    let zone = city_zone(city_obj);
    let game_hours = ctx.profile_mut().game_hours;
    let local = to_local(game_hours, zone).rem_euclid(24.0);
    let delta = (hour - local).rem_euclid(24.0);
    if delta < 0.05 {
        return Vec::new();
    }
    let place = match ctx.world().home_terminal(&current_city) {
        Ok(terminal) => terminal.name,
        Err(_) => ctx.world().spoken_city(&current_city, None),
    };
    let p = ctx.profile_mut();
    let start = p.game_hours;
    p.game_hours += delta;
    let end = p.game_hours;
    p.duty_log
        .record("off_duty", start, end, &place, "playtest clock lever");
    let day = p.market_day();
    p.market.advance_to(day);
    let mut rested = "";
    if delta >= 10.0 {
        // Ten-plus hours parked is a full break in any honest logbook.
        p.hos.sleep();
        p.fatigue = 0.0;
        rested = " You waited out a full break; hours of service reset.";
    }
    vec![format!(
        "Playtest lever: clock moved forward to {}, {} local.{rested}",
        clock_text(hour),
        time_of_day(hour)
    )]
}

#[cfg(test)]
mod tests;
