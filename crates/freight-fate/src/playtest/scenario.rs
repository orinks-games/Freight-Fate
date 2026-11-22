//! One request that puts the sandbox career in any situation an agent wants
//! to drive: any city, any level, any business status, cash, reputation,
//! credentials, local clock, fuel, damage, rest, market and board seeds,
//! any setting, with or without a load in progress. The agent server's
//! `scenario` tool; nothing here is reachable from the shipped menus.
//!
//! Every field is optional and applied in place on the career the process
//! runs -- for the agent server that is always the audited sandbox copy, so
//! a scenario can be as unreasonable as the test needs. The terminal screen
//! is rebuilt afterwards so the dispatch board, the garage and the clock
//! all read the new situation.

use serde_json::{Map, Value};

use ff_core::data::world::World;
use ff_core::models::business_constants::{
    COMPANY_DRIVER, INDEPENDENT_AUTHORITY, LEASED_OWNER_OPERATOR,
};
use ff_core::models::career::LEVEL_XP;
use ff_core::models::career_ladder::MAX_CAREER_LEVEL;
use ff_core::models::credentials::credential_keys;
use ff_core::models::profile::Profile;
use ff_core::playtest_levers::{apply_city, apply_clock, resolve_city_forgiving};
use ff_core::pyfmt::fmt_grouped;
use ff_core::sim::alertness::Alertness;
use ff_core::sim::hos::HosClock;
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::WeatherKind;

use crate::app::GameContext;

/// The co-driver's logbook counters, in game minutes. `None` leaves that
/// counter as it is.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CoDriverHos {
    pub driving_min: Option<f64>,
    pub duty_min: Option<f64>,
    pub since_break_min: Option<f64>,
    /// Unbroken rest so far, in the sleeper, toward their 10 hours.
    pub rest_min: Option<f64>,
}

impl CoDriverHos {
    /// True when no counter was asked for.
    pub fn is_empty(&self) -> bool {
        *self == CoDriverHos::default()
    }

    /// Put the counters on the co-driver's logbook.
    pub fn apply(&self, hos: &mut HosClock) {
        if let Some(minutes) = self.driving_min {
            hos.driving_min = minutes;
        }
        if let Some(minutes) = self.duty_min {
            hos.duty_min = minutes;
        }
        if let Some(minutes) = self.since_break_min {
            hos.since_break_min = minutes;
        }
        if let Some(minutes) = self.rest_min {
            hos.off_duty_min = minutes;
            hos.non_driving_min = minutes;
            hos.status = "sleeper_berth".to_string();
        }
    }

    /// What the counters are now, for the agent to hear back.
    fn note(&self, hos: &HosClock) -> String {
        let left = ff_core::models::co_driver::shift_left_min(hos, "realistic").unwrap_or(0.0);
        format!(
            "Co-driver's logbook: {:.0} minutes driven, {:.0} on duty, {:.0} since a break, \
             {:.0} of rest; {left:.0} minutes of driving left on Realistic hours.",
            hos.driving_min, hos.duty_min, hos.since_break_min, hos.off_duty_min
        )
    }
}

/// The situation asked for. `None` leaves that part of the career alone.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scenario {
    /// The career's name when one has to be created (default "Playtest").
    pub name: Option<String>,
    pub city: Option<String>,
    pub level: Option<i64>,
    pub deliveries: Option<i64>,
    pub money: Option<f64>,
    pub reputation: Option<f64>,
    /// "company", "leased" or "independent" (or the exact status key).
    pub business: Option<String>,
    /// Credential keys bought outright, replacing what the career held.
    pub endorsements: Option<Vec<String>>,
    /// Local clock, 0 to 24, moved forward to.
    pub hour: Option<f64>,
    pub fuel_pct: Option<f64>,
    pub damage_pct: Option<f64>,
    /// Component wear on the active truck, 0 to 100: what a roadside
    /// inspection reads.
    pub tire_wear_pct: Option<f64>,
    pub brake_wear_pct: Option<f64>,
    /// The driving record's lifetime citation count and the career's
    /// out-of-service events: together with damage they set the safety
    /// record band that decides who gets pulled into the inspection lane.
    pub citations: Option<i64>,
    pub out_of_service_events: Option<i64>,
    /// A full sleep taken: hours of service and fatigue reset.
    pub rested: bool,
    /// The first-run walkthrough taught again on the next drive.
    pub first_run: bool,
    /// Open Touch gestures over the terminal: the iPhone and iPad settings
    /// screen, which a desktop build's Controls list leaves out.
    pub touch_gestures: bool,
    /// Tilt steering on or off (Touch gestures, Tilt steering). The agent
    /// server's `tilt` tool is the phone's motion sensor.
    pub tilt_steering: Option<bool>,
    /// Play as an iPhone (true) or as this desktop again (false): the phone's
    /// menus, rows and help, starting with no hardware keyboard, so prompts
    /// name gestures. Applied before anything else in the scenario.
    pub phone: Option<bool>,
    /// The fatigue number, 0 to 100, set after `rested`.
    pub fatigue: Option<f64>,
    /// Sandbox HOS counters for a near-limit road start. These are staged
    /// before `start_at`, which carries the clock into the new drive.
    pub hos_driving_min: Option<f64>,
    pub hos_duty_min: Option<f64>,
    pub hos_since_break_min: Option<f64>,
    /// The co-driver's own logbook counters, the way the `hos_` ones stage
    /// the player's: put on the load in progress if it has a co-driver, and
    /// on the next co-driver paired otherwise.
    pub co_driver_hos: CoDriverHos,
    /// Drop any load in progress so the career is parked at the terminal.
    pub clear_load: bool,
    pub market_seed: Option<i64>,
    pub board_seed: Option<i64>,
    /// The city the truck delivered from to get here: the lane memory reads
    /// as one delivery from there to the parked city, which is what shapes
    /// the next board (the straight return damped, lanes recent both ways).
    pub came_from: Option<String>,
    /// Settings by field name, applied to this session.
    pub settings: Map<String, Value>,
    /// The driver window every drive from here on starts with: `Some(true)`
    /// all the way down, `Some(false)` up.
    pub window_down: Option<bool>,
    /// The road surface every drive from here on keeps: `Some(true)` jointed
    /// the whole way, `Some(false)` smooth.
    pub road_jointed: Option<bool>,
    /// The simulated sky every drive from here on starts under:
    /// `Some(Some(kind))` forced, `Some(None)` free (`parse_weather`).
    pub weather: Option<Option<WeatherKind>>,
}

/// A `weather` field: a condition by name ("snow", "ice", "cloudy",
/// "heavy_rain", or its spoken value such as "freezing rain") to force, or
/// "free" to unlock the sky.
pub fn parse_weather(value: &str) -> Result<Option<WeatherKind>, String> {
    let value = value.trim().to_lowercase();
    if value == "free" {
        return Ok(None);
    }
    WeatherKind::ALL
        .into_iter()
        .find(|kind| kind.name().to_lowercase() == value || kind.value() == value)
        .map(Some)
        .ok_or_else(|| {
            let names: Vec<String> = WeatherKind::ALL
                .iter()
                .map(|kind| kind.name().to_lowercase())
                .collect();
            format!("weather must be free or one of {}", names.join(", "))
        })
}

fn number(args: &Map<String, Value>, key: &str) -> Result<Option<f64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_f64()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a number")),
    }
}

fn integer(args: &Map<String, Value>, key: &str) -> Result<Option<i64>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_i64()
            .map(Some)
            .ok_or_else(|| format!("{key} must be a whole number")),
    }
}

fn text(args: &Map<String, Value>, key: &str) -> Result<Option<String>, String> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(value) => value
            .as_str()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .map(Some)
            .ok_or_else(|| format!("{key} must be text")),
    }
}

impl Scenario {
    /// Read the tool's arguments, refusing anything malformed with a
    /// sentence the agent can act on.
    pub fn from_json(args: &Map<String, Value>) -> Result<Scenario, String> {
        let endorsements = match args.get("endorsements") {
            None | Some(Value::Null) => None,
            Some(Value::Array(items)) => Some(
                items
                    .iter()
                    .map(|item| {
                        item.as_str()
                            .map(|s| s.trim().to_lowercase())
                            .ok_or_else(|| "endorsements must be a list of names".to_string())
                    })
                    .collect::<Result<Vec<String>, String>>()?,
            ),
            Some(_) => return Err("endorsements must be a list of names".to_string()),
        };
        if let Some(list) = &endorsements {
            let known: Vec<&str> = credential_keys().collect();
            for key in list {
                if !known.contains(&key.as_str()) {
                    return Err(format!(
                        "unknown endorsement {key}; the catalog has {}",
                        known.join(", ")
                    ));
                }
            }
        }
        let window_down = match text(args, "window")?.as_deref() {
            None => None,
            Some("down") => Some(true),
            Some("up") => Some(false),
            Some(other) => return Err(format!("window must be up or down, not {other}")),
        };
        let road_jointed = match text(args, "road_surface")?.as_deref() {
            None => None,
            Some("jointed") => Some(true),
            Some("smooth") => Some(false),
            Some(other) => {
                return Err(format!(
                    "road_surface must be jointed or smooth, not {other}"
                ))
            }
        };
        let weather = text(args, "weather")?
            .map(|value| parse_weather(&value))
            .transpose()?;
        let settings = match args.get("settings") {
            None | Some(Value::Null) => Map::new(),
            Some(Value::Object(map)) => map.clone(),
            Some(_) => return Err("settings must be an object of name to value".to_string()),
        };
        let hour = number(args, "hour")?;
        if let Some(hour) = hour {
            if !(0.0..24.0).contains(&hour) {
                return Err("hour must be from 0 up to 24".to_string());
            }
        }
        let level = integer(args, "level")?;
        if let Some(level) = level {
            if !(1..=MAX_CAREER_LEVEL).contains(&level) {
                return Err(format!("level must be from 1 to {MAX_CAREER_LEVEL}"));
            }
        }
        for key in [
            "fuel_pct",
            "damage_pct",
            "tire_wear_pct",
            "brake_wear_pct",
            "fatigue",
        ] {
            if let Some(pct) = number(args, key)? {
                if !(0.0..=100.0).contains(&pct) {
                    return Err(format!("{key} must be from 0 to 100"));
                }
            }
        }
        for key in ["citations", "out_of_service_events"] {
            if let Some(count) = integer(args, key)? {
                if count < 0 {
                    return Err(format!("{key} must be zero or more"));
                }
            }
        }
        for key in [
            "hos_driving_min",
            "hos_duty_min",
            "hos_since_break_min",
            "co_driver_driving_min",
            "co_driver_duty_min",
            "co_driver_since_break_min",
            "co_driver_rest_min",
        ] {
            if let Some(minutes) = number(args, key)? {
                if minutes < 0.0 {
                    return Err(format!("{key} must be zero or more"));
                }
            }
        }
        let business = match text(args, "business")? {
            None => None,
            Some(word) => Some(match word.to_lowercase().as_str() {
                "company" | "company_driver" => COMPANY_DRIVER.to_string(),
                "leased" | "leased_owner_operator" => LEASED_OWNER_OPERATOR.to_string(),
                "independent" | "independent_authority" | "authority" => {
                    INDEPENDENT_AUTHORITY.to_string()
                }
                other => {
                    return Err(format!(
                        "business must be company, leased or independent, not {other}"
                    ))
                }
            }),
        };
        Ok(Scenario {
            name: text(args, "name")?,
            city: text(args, "city")?,
            level,
            deliveries: integer(args, "deliveries")?,
            money: number(args, "money")?,
            reputation: number(args, "reputation")?,
            business,
            endorsements,
            hour,
            fuel_pct: number(args, "fuel_pct")?,
            damage_pct: number(args, "damage_pct")?,
            tire_wear_pct: number(args, "tire_wear_pct")?,
            brake_wear_pct: number(args, "brake_wear_pct")?,
            citations: integer(args, "citations")?,
            out_of_service_events: integer(args, "out_of_service_events")?,
            rested: args.get("rested").and_then(Value::as_bool).unwrap_or(false),
            first_run: args
                .get("first_run")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            touch_gestures: args
                .get("touch_gestures")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            tilt_steering: args.get("tilt_steering").and_then(Value::as_bool),
            phone: args.get("phone").and_then(Value::as_bool),
            fatigue: number(args, "fatigue")?,
            hos_driving_min: number(args, "hos_driving_min")?,
            hos_duty_min: number(args, "hos_duty_min")?,
            hos_since_break_min: number(args, "hos_since_break_min")?,
            co_driver_hos: CoDriverHos {
                driving_min: number(args, "co_driver_driving_min")?,
                duty_min: number(args, "co_driver_duty_min")?,
                since_break_min: number(args, "co_driver_since_break_min")?,
                rest_min: number(args, "co_driver_rest_min")?,
            },
            clear_load: args
                .get("clear_load")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            market_seed: integer(args, "market_seed")?,
            board_seed: integer(args, "board_seed")?,
            came_from: text(args, "came_from")?,
            settings,
            window_down,
            road_jointed,
            weather,
        })
    }

    /// True when nothing at all was asked for.
    pub fn is_empty(&self) -> bool {
        *self == Scenario::default()
    }
}

/// Put the co-driver's counters on the load in progress, when it has a
/// co-driver: the snapshot always holds their logbook as the one off the
/// wheel, so the resumed drive starts with it. None when the load in
/// progress has no co-driver, or there is none.
fn stage_co_driver_hos(p: &mut Profile, staged: &CoDriverHos) -> Option<String> {
    let team = p
        .active_trip
        .as_mut()
        .and_then(|trip| trip.get_mut("team"))
        .and_then(Value::as_object_mut);
    let team = team?;
    let mut hos = team
        .get("co_driver_hos")
        .map(HosClock::from_dict)
        .unwrap_or_default();
    staged.apply(&mut hos);
    let note = staged.note(&hos);
    team.insert("co_driver_hos".to_string(), hos.to_dict());
    Some(note)
}

/// A career to stage on when none is loaded: a bench career, one delivery
/// in so dispatch treats it as a working driver, tutorial teaching done so
/// the speech ladder runs as configured.
fn fresh_career(world: &World, scenario: &Scenario) -> Result<Profile, String> {
    let city = scenario.city.as_deref().unwrap_or("Topeka");
    let key = resolve_city_forgiving(world, city);
    if !world.cities.contains_key(&key) {
        return Err(format!("no city called {city}"));
    }
    let mut profile = Profile::named_in(scenario.name.as_deref().unwrap_or("Playtest"), &key);
    profile.tutorial_done = true;
    profile.career.deliveries = 1;
    Ok(profile)
}

/// Put the career in the situation asked for. Returns what changed, one
/// sentence per part, for the agent to hear back.
pub fn apply(ctx: &mut GameContext, scenario: &Scenario) -> Result<Vec<String>, String> {
    let mut notes: Vec<String> = Vec::new();
    let mut pending_co_driver = None;
    let world = ctx.world;
    if ctx.profile.is_none() {
        ctx.profile = Some(fresh_career(world, scenario)?);
        notes.push("A bench career was created for the scenario.".to_string());
    }
    if scenario.clear_load {
        let p = ctx.profile.as_mut().expect("just ensured");
        if p.active_trip.take().is_some() {
            notes.push("The load in progress was dropped; the career is parked.".to_string());
        }
    }
    if let Some(city) = &scenario.city {
        if !world
            .cities
            .contains_key(&resolve_city_forgiving(world, city))
        {
            return Err(format!("no city called {city}"));
        }
        if ctx
            .profile
            .as_ref()
            .is_some_and(|p| p.active_trip.is_some())
        {
            return Err(
                "a load is in progress; pass clear_load: true to drop it before moving the career"
                    .to_string(),
            );
        }
        notes.extend(apply_city(ctx, city));
    }
    {
        let p = ctx.profile.as_mut().expect("ensured above");
        if let Some(level) = scenario.level {
            let level = level.clamp(1, MAX_CAREER_LEVEL) as usize;
            p.career.xp = LEVEL_XP[level - 1];
            notes.push(format!("Level {level}."));
        }
        if let Some(deliveries) = scenario.deliveries {
            p.career.deliveries = deliveries.max(0);
            notes.push(format!("{} deliveries behind them.", deliveries.max(0)));
        }
        if let Some(money) = scenario.money {
            p.set_money(money);
            notes.push(format!("{} dollars in hand.", fmt_grouped(money, 0)));
        }
        if let Some(reputation) = scenario.reputation {
            p.career.reputation = reputation;
            notes.push(format!("Reputation {reputation}."));
        }
        if let Some(business) = &scenario.business {
            p.business_status = business.clone();
            notes.push(format!("Business status {business}."));
        }
        if let Some(endorsements) = &scenario.endorsements {
            p.career.purchased_endorsements = endorsements.clone();
            notes.push(if endorsements.is_empty() {
                "No bought endorsements.".to_string()
            } else {
                format!("Endorsements bought: {}.", endorsements.join(", "))
            });
        }
        if let Some(seed) = scenario.market_seed {
            p.market.seed = seed;
            notes.push(format!("Market seed {seed}."));
        }
        if scenario.fuel_pct.is_some()
            || scenario.damage_pct.is_some()
            || scenario.tire_wear_pct.is_some()
            || scenario.brake_wear_pct.is_some()
        {
            let mut truck = TruckState::new(p.truck_specs());
            p.load_truck_condition(&mut truck);
            if let Some(pct) = scenario.fuel_pct {
                truck.fuel_gal = truck.specs.fuel_tank_gal * pct / 100.0;
                notes.push(format!("Fuel {} percent.", pct.round()));
            }
            if let Some(pct) = scenario.damage_pct {
                truck.damage_pct = pct;
                notes.push(format!("Truck damage {} percent.", pct.round()));
            }
            if let Some(pct) = scenario.tire_wear_pct {
                truck.tire_wear_pct = pct;
                notes.push(format!("Tire wear {} percent.", pct.round()));
            }
            if let Some(pct) = scenario.brake_wear_pct {
                truck.brake_wear_pct = pct;
                notes.push(format!("Brake wear {} percent.", pct.round()));
            }
            p.store_truck_condition(&truck);
            // A slip-seating company driver is handed a yard spare per load,
            // each with its own record, so the next drive may read a different
            // key than the one just written. The sandbox asked for THIS truck
            // state: give every spare the same one.
            let fields: Vec<(&str, f64)> = [
                ("fuel_gal", Some(truck.fuel_gal)),
                ("damage_pct", scenario.damage_pct),
                ("tire_wear_pct", scenario.tire_wear_pct),
                ("brake_wear_pct", scenario.brake_wear_pct),
            ]
            .into_iter()
            .filter_map(|(key, value)| value.map(|v| (key, v)))
            .collect();
            for record in p.truck_conditions.values_mut() {
                for (key, value) in &fields {
                    record.insert((*key).to_string(), Value::from(*value));
                }
            }
        }
        // Dated now, so the safety record's window counts them.
        if let Some(count) = scenario.citations {
            p.driving_record.citations = count;
            p.driving_record.citation_times = vec![p.game_hours; count.max(0) as usize];
            notes.push(format!("Citations on the record: {count}."));
        }
        if let Some(count) = scenario.out_of_service_events {
            p.out_of_service_events = count;
            p.driving_record.out_of_service_times = vec![p.game_hours; count.max(0) as usize];
            notes.push(format!("Out-of-service events: {count}."));
        }
        if scenario.first_run {
            p.tutorial_done = false;
            notes.push("The next drive teaches the first run.".to_string());
        }
        if scenario.rested {
            p.hos.sleep();
            p.fatigue = 0.0;
            notes.push("Fully rested; hours of service reset.".to_string());
        }
        if let Some(minutes) = scenario.hos_driving_min {
            p.hos.driving_min = minutes;
            notes.push(format!("HOS driving counter: {minutes:.0} minutes."));
        }
        if let Some(minutes) = scenario.hos_duty_min {
            p.hos.duty_min = minutes;
            notes.push(format!("HOS duty counter: {minutes:.0} minutes."));
        }
        if let Some(minutes) = scenario.hos_since_break_min {
            p.hos.since_break_min = minutes;
            notes.push(format!("HOS since-break counter: {minutes:.0} minutes."));
        }
        if let Some(city) = &scenario.came_from {
            let from = resolve_city_forgiving(world, city);
            if !world.cities.contains_key(&from) {
                return Err(format!("no city called {city}"));
            }
            let here = world.resolve_city_key(&p.current_city);
            p.recent_lanes = vec![format!("{from}:{here}")];
            notes.push(format!(
                "Came in from {}.",
                world.spoken_city(&from, Some(true))
            ));
        }
        if !scenario.co_driver_hos.is_empty() {
            match stage_co_driver_hos(p, &scenario.co_driver_hos) {
                Some(note) => notes.push(note),
                None => {
                    pending_co_driver = Some(scenario.co_driver_hos);
                    notes.push(
                        "The co-driver's logbook counters go on the next co-driver paired for \
                         a load."
                            .to_string(),
                    );
                }
            }
        }
        // Whatever the board showed before, it shows the new situation now.
        p.dispatch_board_cache = None;
    }
    if pending_co_driver.is_some() {
        ctx.playtest_co_driver_hos = pending_co_driver;
    }
    if let Some(hour) = scenario.hour {
        notes.extend(apply_clock(ctx, hour));
    }
    // After the clock: a jump past ten hours rests the driver, and the
    // fatigue asked for must be the one the scenario ends with.
    if let Some(fatigue) = scenario.fatigue {
        if let Some(p) = ctx.profile.as_mut() {
            p.fatigue = fatigue;
            notes.push(format!(
                "Fatigue {fatigue:.0} percent, {}.",
                Alertness::from_fatigue(fatigue).key()
            ));
        }
    }
    if let Some(down) = scenario.window_down {
        ctx.playtest_window_down = Some(down);
        notes.push(if down {
            "Drives start with the driver window down.".to_string()
        } else {
            "Drives start with the driver window up.".to_string()
        });
    }
    if let Some(weather) = scenario.weather {
        ctx.playtest_weather = Some(weather);
        notes.push(match weather {
            Some(kind) => format!("Drives start under forced {}.", kind.value()),
            None => "Drives start under a free sky.".to_string(),
        });
    }
    if let Some(jointed) = scenario.road_jointed {
        ctx.playtest_road_jointed = Some(jointed);
        notes.push(if jointed {
            "Drives keep a jointed road surface.".to_string()
        } else {
            "Drives keep a smooth road surface.".to_string()
        });
    }
    if let Some(seed) = scenario.board_seed {
        ctx.dispatch_board_seed = Some(seed);
        notes.push(format!("Dispatch board seed {seed}."));
    }
    if let Some(on) = scenario.tilt_steering {
        ctx.settings.touch_tilt_steering = on;
        notes.push(format!("Tilt steering {}.", if on { "on" } else { "off" }));
    }
    for (name, value) in &scenario.settings {
        if !ctx.settings.set_field(name, value) {
            return Err(format!(
                "no setting called {name}, or {value} is not a value it takes"
            ));
        }
        notes.push(format!("Setting {name} is now {value}."));
    }
    ctx.save_profile();
    Ok(notes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_arguments_are_checked_before_anything_moves() {
        let bad = |json: &str| {
            let args: Map<String, Value> = serde_json::from_str(json).unwrap();
            Scenario::from_json(&args).expect_err("refused")
        };
        assert!(bad(r#"{"hour": 24}"#).contains("hour"));
        assert!(bad(r#"{"level": 0}"#).contains("level"));
        assert!(bad(r#"{"fuel_pct": 120}"#).contains("fuel_pct"));
        assert!(bad(r#"{"fatigue": -1}"#).contains("fatigue"));
        assert!(bad(r#"{"hos_since_break_min": -1}"#).contains("hos_since_break_min"));
        assert!(bad(r#"{"business": "owner"}"#).contains("business"));
        assert!(bad(r#"{"endorsements": ["rocket"]}"#).contains("unknown endorsement"));
        assert!(bad(r#"{"settings": 3}"#).contains("settings"));
        let args: Map<String, Value> = serde_json::from_str(
            r#"{"city": "Tonopah", "level": 5, "business": "leased", "endorsements": ["hazmat"],
                "hour": 6.5, "fuel_pct": 25, "rested": true, "fatigue": 64,
                "settings": {"real_traffic": true}}"#,
        )
        .unwrap();
        let scenario = Scenario::from_json(&args).unwrap();
        assert_eq!(scenario.city.as_deref(), Some("Tonopah"));
        assert_eq!(scenario.level, Some(5));
        assert_eq!(scenario.business.as_deref(), Some(LEASED_OWNER_OPERATOR));
        assert_eq!(scenario.endorsements, Some(vec!["hazmat".to_string()]));
        assert!(scenario.rested);
        assert_eq!(scenario.fatigue, Some(64.0));
        assert_eq!(
            scenario.settings.get("real_traffic"),
            Some(&Value::Bool(true))
        );
        assert!(Scenario::from_json(&Map::new()).unwrap().is_empty());
    }
}
