//! How often traffic forces the truck to slow, measured rather than felt.
//!
//! Owner, 2026-08-24: "I don't like the idea that I have to brake when I (the
//! player) am in the only lane available and it forces me to brake. In those
//! situations you'd brake hard, ie the emergency brake. But it happens far too
//! often in my experience."
//!
//! "Far too often" is a rate, so this file produces one. The sweep drives
//! seeded deliveries over a spread of real corridors, records every occasion
//! the trip raised a lead-vehicle hazard -- the "Change lanes or brake!" /
//! "Brake!" call that the automatic brake then acts on -- and reports events
//! per hundred miles split by how many lanes the road has in the direction of
//! travel.
//!
//! Every run pins its trip seed and its weather. An unseeded delivery draws
//! its own road and its own sky, and rain alone moves every NPC's speed by up
//! to fourteen miles an hour, which is most of the effect being measured.
//!
//! The sweep itself is `#[ignore]`d: it is a measurement, and it drives
//! hundreds of route miles. The cases below it are the assertions the
//! measurement earned.

use std::collections::BTreeMap;

use ff_core::data::world::{get_world, World};
use ff_core::sim::trip_models::{highway_class, TRAFFIC_WARNING_GAP_S};
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};
use freight_fate::states::base::Key;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::HAZARD_SAFE_MPH;

use crate::transcript_cruise_support::{frame, hold, release_keys, DT};

// -- picking the road ------------------------------------------------------------------

/// One corridor the sweep drives, and what the lane bake says it is.
#[derive(Debug, Clone)]
pub struct Corridor {
    pub origin: String,
    pub destination: String,
    pub highway: String,
    pub miles: f64,
    /// Share of the leg's miles with exactly one lane in the direction of
    /// travel, from the baked lane segments.
    pub single_share: f64,
}

impl Corridor {
    pub fn class(&self) -> &'static str {
        highway_class(&self.highway)
    }
}

/// Lanes our side at a leg-relative offset, straight off the lane bake, with
/// the same fallbacks `Trip::lane_count_at` applies.
fn lanes_your_side(leg: &ff_core::data::world_models::Leg, offset_mi: f64) -> i64 {
    for seg in leg.lane_segments() {
        if seg.start_mi <= offset_mi && offset_mi <= seg.end_mi {
            return seg.your_side().max(1);
        }
    }
    if leg.divided == Some(false) {
        return 1;
    }
    if leg.lanes != 0 {
        leg.lanes.max(1)
    } else {
        2
    }
}

/// Share of a leg's miles that has one lane our side.
fn single_lane_share(leg: &ff_core::data::world_models::Leg) -> f64 {
    let mut single = 0.0;
    let mut total = 0.0;
    let mut mile = 0.0;
    while mile < leg.miles {
        total += 1.0;
        if lanes_your_side(leg, mile) < 2 {
            single += 1.0;
        }
        mile += 0.5;
    }
    if total == 0.0 {
        0.0
    } else {
        single / total
    }
}

/// A fixed spread of single-lane-heavy and multi-lane corridors, walked in a
/// deterministic order so the sweep drives the same roads every run.
///
/// One corridor per (state, bucket) so the sweep does not spend itself on one
/// region's road-building habits.
pub fn corridors(world: &'static World, want: usize) -> (Vec<Corridor>, Vec<Corridor>) {
    let mut keys: Vec<&String> = world.cities.keys().collect();
    keys.sort();
    let mut single: Vec<Corridor> = Vec::new();
    let mut multi: Vec<Corridor> = Vec::new();
    let mut single_states: Vec<String> = Vec::new();
    let mut multi_states: Vec<String> = Vec::new();
    for city in keys {
        let Some(entry) = world.cities.get(city) else {
            continue;
        };
        let state = entry.state.clone();
        for leg in world.neighbors(city) {
            // Long enough to be a drive rather than an on-ramp, short enough
            // that one corridor does not eat the whole sweep.
            if !(45.0..=190.0).contains(&leg.miles) {
                continue;
            }
            if leg.a != *city {
                continue; // drive each leg once, in its baked direction
            }
            let share = single_lane_share(leg);
            let corridor = Corridor {
                origin: leg.a.clone(),
                destination: leg.b.clone(),
                highway: leg.highway.clone(),
                miles: leg.miles,
                single_share: share,
            };
            let (bucket, seen) = if share >= 0.6 {
                (&mut single, &mut single_states)
            } else if share <= 0.05 {
                (&mut multi, &mut multi_states)
            } else {
                continue; // mixed: it would muddy both halves of the split
            };
            if bucket.len() >= want || seen.contains(&state) {
                continue;
            }
            seen.push(state.clone());
            bucket.push(corridor);
        }
        if single.len() >= want && multi.len() >= want {
            break;
        }
    }
    (single, multi)
}

// -- what one forced slow-down was -------------------------------------------------------

/// One occasion the road made the truck give up speed for a vehicle ahead.
#[derive(Debug, Clone)]
pub struct Forced {
    pub corridor: String,
    pub seed: i64,
    pub mile: f64,
    /// Lanes in the direction of travel where it happened.
    pub lanes: i64,
    pub class: &'static str,
    pub posted_mph: f64,
    pub truck_mph: f64,
    pub lead_mph: f64,
    pub lead_class: String,
    pub lead_intent: String,
    /// What the bubble was steering the lead toward, and what the hill was
    /// allowing it -- the two halves of the speed it actually had.
    pub lead_target_mph: f64,
    pub lead_climb_mph: f64,
    pub lead_grade_pct: f64,
    /// Whether the road gave traffic here a reason to be stopping.
    pub braking_zone: bool,
    /// Posted limit where the lead is, and where the bubble drew it.
    pub lead_here_mph: f64,
    pub lead_spawn_mph: Option<f64>,
    pub gap_mi: f64,
    pub closing_mph: f64,
    /// Speed the truck was down to when the hazard cleared.
    pub bottom_mph: f64,
    /// The moment the ASSIST first put the pedal down, if it ever did: the
    /// gap to the lead then, the time that gap was worth at the rate the
    /// truck was closing, and the speed it acted at. NaN where the driver's
    /// own window was never used up.
    pub assist_gap_mi: f64,
    pub assist_ttc_s: f64,
    pub assist_mph: f64,
    /// Time still left on the hazard's own deadline when the assist acted.
    /// THIS is the game's time-to-collision: the traffic warning is a timer,
    /// and the modelled collision happens when it reaches zero.
    pub assist_deadline_s: f64,
    /// Closest the truck ever got to the lead over the whole episode.
    pub nearest_gap_mi: f64,
    /// Whether the assist spent the emergency application on it.
    pub emergency: bool,
    /// The key of the vehicle the warning was ABOUT. Everything measured
    /// during the episode is measured against this one: `traffic_context`
    /// re-picks whatever is nearest each frame, so reading it after the
    /// hazard cleared reports the gap to a different car entirely.
    pub lead_key: String,
    /// The whole window the call bought before the assist must act: the
    /// hazard deadline the instant the warning landed. This is what the
    /// lane-change allowance was being added to.
    pub granted_s: f64,
    /// Gap to the lead when the hazard cleared, miles.
    pub cleared_gap_mi: f64,
    /// Game seconds from the call to the truck being back within 3 mph of
    /// what the road allows.
    pub recover_s: f64,
    /// The words the driver heard.
    pub call: String,
    /// What was said when it resolved.
    pub resolution: String,
}

impl Forced {
    pub fn given_up_mph(&self) -> f64 {
        (self.truck_mph - self.bottom_mph).max(0.0)
    }

    /// The slowest a speed drawn for the road the LEAD IS ON could be.
    pub fn lead_floor_mph(&self) -> f64 {
        15.0_f64.max(self.lead_here_mph * 0.45)
    }

    /// Why this one happened, in one word.
    ///
    /// `carried`  -- the model is STEERING the lead slower than any speed this
    ///               road could have drawn for it. Before the re-base that was
    ///               a town's number carried out onto the highway.
    /// `phantom`  -- the lead is on the brakes where the road gives no reason.
    /// `climbing` -- the lead is well under its own target: a vehicle
    ///               genuinely getting back up to speed out of a slower
    ///               stretch, which is a real thing to come up behind.
    /// `road`     -- a speed this road could have produced: a real encounter.
    pub fn cause(&self) -> &'static str {
        if self.lead_target_mph < self.lead_floor_mph() - 0.5 {
            return "carried";
        }
        if self.intent_is_braking() && !self.braking_zone {
            return "phantom";
        }
        if self.lead_mph < self.lead_target_mph - 8.0 {
            return "climbing";
        }
        "road"
    }

    pub fn intent_is_braking(&self) -> bool {
        self.lead_intent == "braking"
    }
}

/// One bubble vehicle, seen once, against the road it is on.
///
/// The screening question the sweep exists to answer: is a vehicle's speed a
/// speed for the road it is ON, or one it brought from somewhere else?
#[derive(Debug, Clone)]
pub struct Census {
    pub class: String,
    pub intent: String,
    pub speed_mph: f64,
    /// What the model says this vehicle is heading for. The speed lags it by
    /// the acceleration integrator, so the TARGET is where an invariant about
    /// what the model asserts can be checked exactly.
    pub target_mph: f64,
    /// On the brakes where the road gives no reason to be.
    pub braking_unplaced: bool,
    /// The lane it is in, and how many the road has our side there.
    pub lane: i64,
    pub lanes_here: i64,
    /// Posted limit where the vehicle is now.
    pub here_mph: f64,
    /// Posted limit in the cell the rolling bubble drew it in, or `None` for
    /// a vehicle that did not come from the rolling bubble.
    pub spawn_mph: Option<f64>,
}

impl Census {
    /// Speed as a share of the number posted where it is.
    pub fn share(&self) -> f64 {
        if self.here_mph <= 0.0 {
            1.0
        } else {
            self.speed_mph / self.here_mph
        }
    }

    /// The slowest a speed DRAWN for this road could have come out.
    pub fn floor_here_mph(&self) -> f64 {
        15.0_f64.max(self.here_mph * 0.45)
    }

    /// Slower than anything the speed draw for THIS road can produce.
    pub fn below_the_floor(&self) -> bool {
        self.speed_mph < self.floor_here_mph() - 0.5
    }

    /// The model is STEERING this vehicle below the floor of the road it is
    /// on -- not lagging toward it, aiming at it. Always false on a road
    /// whose posting the vehicle's speed is re-read from.
    pub fn aimed_below_the_floor(&self) -> bool {
        self.target_mph < self.floor_here_mph() - 0.5
    }
}

/// What one seeded drive over one corridor met.
#[derive(Debug, Default)]
pub struct Run {
    pub single_mi: f64,
    pub multi_mi: f64,
    pub forced: Vec<Forced>,
    pub census: Vec<Census>,
    /// How many times the bubble was counted, so the census can report a
    /// POPULATION rather than a pile of sightings. Traffic where traffic
    /// really is is a shipped feature: any change here has to leave the
    /// population alone and move only the speeds.
    pub census_samples: usize,
    /// Vehicles per mile of road in the direction of travel that the volume
    /// model itself asks for, summed over the counts. The AADT bake, this
    /// hour's share of the day and the peak direction's share, over the speed
    /// traffic is moving -- the same arithmetic `leg_density` does, read back
    /// as a density rather than as a per-cell chance.
    pub implied_per_mile: f64,
    /// Every vehicle the bubble put on this road, by key. The population a
    /// sample sees is dwell time times this; only THIS number says whether a
    /// change moved how much traffic is placed.
    pub keys: std::collections::HashSet<String>,
}

impl Run {
    pub fn miles(&self) -> f64 {
        self.single_mi + self.multi_mi
    }
}

/// The route mile a rolling-bubble vehicle was drawn at, from its key.
///
/// `replenish` names each one `bubble:{cell}` and the cell is a fixed
/// `SPAWN_CELL_MI` slice of road, so the key says where the vehicle came
/// into being -- which is where its speed was drawn from the posted number.
pub fn spawn_cell_mi(key: &str) -> Option<f64> {
    let cell: i64 = key
        .strip_prefix("bubble:")?
        .split(':')
        .next()?
        .parse()
        .ok()?;
    Some(cell as f64 * ff_core::sim::traffic_manager::SPAWN_CELL_MI)
}

/// What a driver holding the legal number would be doing here.
fn driver_target_mph(d: &mut DrivingState) -> f64 {
    d.trip.speed_limit_at(d.trip.position_mi).0
}

/// Drive one corridor at one seed and one departure hour, recording every
/// lead-vehicle hazard.
///
/// The driver is a player who holds the posted truck number and otherwise
/// keeps their hands off: the automatic brake is on (its shipped default), so
/// what the run measures is the game deciding the truck has to slow, which is
/// what the report is about.
pub fn drive_corridor(corridor: &Corridor, seed: i64, hour: f64, max_miles: f64) -> Run {
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.speed_keeper = false;
    harness.app.ctx.settings.automatic_emergency_braking = true;
    harness.start_route(
        &corridor.origin,
        &corridor.destination,
        RouteSetup::seeded(seed).named("Traffic Rate"),
    );
    harness.with_drive(|d, ctx| {
        // `start_route` empties the road for the measurements that want it
        // empty. This one wants the road the player actually gets.
        d.trip.traffic_manager.rolling_bubble = true;
        d.trip.traffic_manager.spawn_initial_traffic();
        // Random hazards stay off: a deer arms the same machinery and would
        // be counted as traffic. The traffic branch of `check_hazards` runs
        // before this counter is ever read, so it is untouched.
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.posts.clear();
        // Pinned, not drawn: rain moves every NPC speed by up to fourteen
        // miles an hour, which is most of what is being measured.
        d.weather_mut().current = WeatherKind::Clear;
        d.trip.start_hour = hour;
        d.trip.game_minutes = 0.0;
        d.trip.traffic_manager.start_hour = hour;
        d.trip.traffic_manager.hour = hour;
        d.departure_checked = true;
        d.destination_exit_taken = true;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().set_air_ready(false);
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
    });
    harness.clear_speech();

    let mut run = Run::default();
    let mut pending: Option<Forced> = None;
    let mut bottom_mph = f64::INFINITY;
    let mut recovering: Option<usize> = None;
    // Long enough for `max_miles` of road at highway speed and then some; the
    // loop breaks on distance or on the drive ending.
    for (census_tick, _) in (0..(60 * 60 * 40)).enumerate() {
        if !harness.has_drive() {
            break;
        }
        let before = harness.read_drive(|d| d.trip.position_mi);
        let target = harness.with_drive(|d, _| driver_target_mph(d));
        let rolling = harness.read_drive(|d| d.truck().speed_mph() < target - 1.0);
        if rolling {
            hold(&mut harness, &[Key::Up]);
        } else {
            release_keys(&mut harness);
        }
        frame(&mut harness, DT);
        if !harness.has_drive() {
            break;
        }
        let (after, speed_mph, lanes, hazard_live, fuel, finished) = harness.read_drive(|d| {
            (
                d.trip.position_mi,
                d.truck().speed_mph(),
                d.trip.lane_count_at(None),
                d.hazard_deadline.is_some(),
                d.truck().fuel_gal,
                d.trip.finished,
            )
        });
        if fuel < 40.0 {
            // Refuelling is a stop of its own; the sweep is about the road.
            harness.with_drive(|d, _| {
                let tank = d.truck().specs.fuel_tank_gal;
                d.truck_mut().fuel_gal = tank;
            });
        }
        let moved = (after - before).max(0.0);
        if lanes < 2 {
            run.single_mi += moved;
        } else {
            run.multi_mi += moved;
        }

        if census_tick.is_multiple_of(30) {
            harness.read_drive(|d| {
                for v in &d.trip.traffic_manager.vehicles {
                    run.keys.insert(v.key.clone());
                }
            });
        }
        if census_tick.is_multiple_of(600) {
            run.census_samples += 1;
            run.implied_per_mile += harness.read_drive(|d| {
                let manager = &d.trip.traffic_manager;
                let Some(leg) = manager.leg_at(d.trip.position_mi) else {
                    return 0.0;
                };
                let night = ff_core::sim::hos::is_night(manager.hour);
                let occupied = manager.leg_density(leg, night, Some(d.trip.position_mi));
                // `leg_density` is P(this cell of road is carrying somebody)
                // for a Poisson arrival, so the density it came from is
                // -ln(1 - p) per cell width.
                -(1.0 - occupied.min(0.999)).ln() / ff_core::sim::traffic_manager::SPAWN_CELL_MI
            });
            run.census.extend(harness.read_drive(|d| {
                let manager = &d.trip.traffic_manager;
                manager
                    .vehicles
                    .iter()
                    .map(|v| Census {
                        class: v.vehicle_class.clone(),
                        intent: v.intent.clone(),
                        speed_mph: v.speed_mph,
                        target_mph: v.target_speed_mph,
                        braking_unplaced: v.intent == "braking"
                            && !manager.braking_plausible_at(v.position_mi),
                        lane: v.lane,
                        lanes_here: manager.lane_count_at(v.position_mi),
                        here_mph: manager.posted_limit_at(v.position_mi),
                        spawn_mph: spawn_cell_mi(&v.key).map(|mile| manager.posted_limit_at(mile)),
                    })
                    .collect::<Vec<_>>()
            }));
        }

        // A lead-vehicle hazard is the trip event that carries a traffic
        // context; every other hazard carries none.
        let fired = harness.with_drive(|d, _| {
            let posted = d.trip.speed_limit_at(d.trip.position_mi).0;
            d.trip
                .events
                .iter()
                .find(|e| e.data.traffic.is_some())
                .map(|e| {
                    let context = e.data.traffic.clone().expect("filtered on it");
                    let manager = &d.trip.traffic_manager;
                    let grade_pct = manager.grade_pct_at(context.lead.position_mi);
                    let climb = ff_core::sim::traffic_manager::climb_speed_mph(
                        &context.lead.vehicle_class,
                        grade_pct,
                    );
                    Forced {
                        corridor: format!("{} - {}", corridor.origin, corridor.destination),
                        seed,
                        mile: d.trip.position_mi,
                        lanes: d.trip.lane_count_at(None),
                        class: corridor.class(),
                        posted_mph: posted,
                        truck_mph: d.truck().speed_mph(),
                        lead_mph: context.lead.speed_mph,
                        lead_class: context.lead.vehicle_class.clone(),
                        lead_intent: context.lead.intent.clone(),
                        lead_target_mph: context.lead.target_speed_mph,
                        lead_climb_mph: climb,
                        lead_grade_pct: grade_pct,
                        braking_zone: manager.braking_plausible_at(context.lead.position_mi),
                        lead_here_mph: manager.posted_limit_at(context.lead.position_mi),
                        lead_spawn_mph: spawn_cell_mi(&context.lead.key)
                            .map(|mile| manager.posted_limit_at(mile)),
                        gap_mi: context.gap_mi,
                        closing_mph: context.closing_mph,
                        bottom_mph: d.truck().speed_mph(),
                        assist_gap_mi: f64::NAN,
                        assist_ttc_s: f64::NAN,
                        assist_mph: f64::NAN,
                        assist_deadline_s: f64::NAN,
                        nearest_gap_mi: context.gap_mi,
                        emergency: false,
                        lead_key: context.lead.key.clone(),
                        granted_s: d.hazard_deadline.unwrap_or(f64::NAN),
                        cleared_gap_mi: context.gap_mi,
                        recover_s: 0.0,
                        call: e.message.normal.clone(),
                        resolution: String::new(),
                    }
                })
        });
        if let Some(mut forced) = fired {
            if let Some(previous) = pending.take() {
                run.forced.push(previous);
            }
            if !forced.granted_s.is_finite() {
                forced.granted_s = harness
                    .read_drive(|d| d.hazard_deadline)
                    .unwrap_or(f64::NAN);
            }
            forced.bottom_mph = speed_mph;
            bottom_mph = speed_mph;
            pending = Some(forced);
            recovering = None;
        }
        if let Some(forced) = pending.as_mut() {
            bottom_mph = bottom_mph.min(speed_mph);
            forced.bottom_mph = bottom_mph;
            let key = forced.lead_key.clone();
            let (aeb_on, emergency, gap, closing, left_s) = harness.read_drive(move |d| {
                let lead = d
                    .trip
                    .traffic_manager
                    .vehicles
                    .iter()
                    .find(|v| v.key == key);
                (
                    d.aeb_brake > 0.0,
                    d.aeb_emergency,
                    lead.map(|v| v.position_mi - d.trip.position_mi),
                    lead.map(|v| d.truck().speed_mph() - v.speed_mph),
                    d.hazard_deadline.unwrap_or(f64::NAN),
                )
            });
            forced.emergency |= emergency;
            // Only while the hazard is LIVE: once it has resolved the truck
            // simply drives past, and the gap then says nothing about how
            // close the assist let it get. The gap is SIGNED -- negative
            // means the truck was already level with the vehicle it was
            // still being warned about, which is a reading in its own right.
            if let Some(gap) = gap.filter(|_| hazard_live) {
                forced.nearest_gap_mi = forced.nearest_gap_mi.min(gap);
                // The FIRST frame the assist takes the pedal is the moment
                // the whole report is about: how near the vehicle in front
                // the truck already is before anything acts.
                if aeb_on && forced.assist_gap_mi.is_nan() {
                    forced.assist_gap_mi = gap;
                    forced.assist_mph = speed_mph;
                    forced.assist_deadline_s = left_s;
                    let closing = closing.unwrap_or(0.0);
                    forced.assist_ttc_s = if closing > 0.1 {
                        gap / closing * 3600.0
                    } else {
                        f64::INFINITY
                    };
                }
            }
            forced.recover_s += DT * harness.read_drive(|d| d.trip.effective_time_scale());
            if !hazard_live {
                if recovering.is_none() {
                    recovering = Some(0);
                    forced.cleared_gap_mi = harness
                        .read_drive(|d| d.trip.traffic_context().map(|c| c.gap_mi))
                        .unwrap_or(f64::NAN);
                    forced.resolution = harness
                        .read_drive(|d| d.last_event_message.clone())
                        .to_string();
                }
                if speed_mph >= target - 3.0 {
                    run.forced.push(pending.take().expect("checked"));
                    bottom_mph = f64::INFINITY;
                    recovering = None;
                }
            }
        }
        if finished || run.miles() >= max_miles {
            break;
        }
    }
    if let Some(forced) = pending.take() {
        run.forced.push(forced);
    }
    release_keys(&mut harness);
    run
}

// -- the sweep -------------------------------------------------------------------------

/// Corridors of each kind, and the seeds each is driven at.
pub const PER_KIND: usize = 6;
pub const SEEDS: [i64; 3] = [11, 4242, 90210];
/// Departure hours: a midday shoulder, an evening rush, and a night run.
pub const HOURS: [f64; 3] = [11.0, 17.0, 2.0];
/// Road driven per (corridor, seed) pair.
pub const MILES_PER_RUN: f64 = 60.0;

/// Events per hundred miles.
pub fn per_hundred(events: usize, miles: f64) -> f64 {
    if miles <= 0.0 {
        0.0
    } else {
        events as f64 * 100.0 / miles
    }
}

/// `FF_TRAFFIC_<NAME>`, for shrinking the sweep while iterating on it.
fn env_f64(name: &str, fallback: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(fallback)
}

#[test]
#[ignore = "measurement sweep: run with --release --ignored --nocapture"]
fn traffic_forced_slow_rate_sweep() {
    let world = get_world();
    let per_kind = env_f64("FF_TRAFFIC_PER_KIND", PER_KIND as f64) as usize;
    let miles_per_run = env_f64("FF_TRAFFIC_MILES", MILES_PER_RUN);
    let seed_count = env_f64("FF_TRAFFIC_SEEDS", SEEDS.len() as f64) as usize;
    let (single, multi) = corridors(world, per_kind);
    println!("single-lane corridors ({}):", single.len());
    for c in &single {
        println!(
            "  {} -> {} on {} ({:.0} mi, {:.0}% single lane)",
            c.origin,
            c.destination,
            c.highway,
            c.miles,
            c.single_share * 100.0
        );
    }
    println!("multi-lane corridors ({}):", multi.len());
    for c in &multi {
        println!(
            "  {} -> {} on {} ({:.0} mi)",
            c.origin, c.destination, c.highway, c.miles
        );
    }

    let mut single_mi = 0.0;
    let mut multi_mi = 0.0;
    let mut events: Vec<Forced> = Vec::new();
    let mut census: Vec<Census> = Vec::new();
    let mut census_samples = 0usize;
    let mut implied_per_mile = 0.0;
    let mut placed = 0usize;
    for corridor in single.iter().chain(multi.iter()) {
        for (i, seed) in SEEDS.iter().take(seed_count).enumerate() {
            let run = drive_corridor(corridor, *seed, HOURS[i % HOURS.len()], miles_per_run);
            single_mi += run.single_mi;
            multi_mi += run.multi_mi;
            events.extend(run.forced);
            census.extend(run.census);
            census_samples += run.census_samples;
            implied_per_mile += run.implied_per_mile;
            placed += run.keys.len();
        }
    }

    let single_events = events.iter().filter(|e| e.lanes < 2).count();
    let multi_events = events.iter().filter(|e| e.lanes >= 2).count();
    println!();
    println!("FORCED SLOW-DOWNS PER HUNDRED MILES");
    println!(
        "  one lane your side : {:>6.2} per 100 mi ({} events over {:.1} mi)",
        per_hundred(single_events, single_mi),
        single_events,
        single_mi
    );
    println!(
        "  two or more        : {:>6.2} per 100 mi ({} events over {:.1} mi)",
        per_hundred(multi_events, multi_mi),
        multi_events,
        multi_mi
    );

    let mut by_class: BTreeMap<&'static str, (usize, f64)> = BTreeMap::new();
    for e in &events {
        by_class.entry(e.class).or_insert((0, 0.0)).0 += 1;
    }
    let below = census.iter().filter(|c| c.below_the_floor()).count();
    let carried = census
        .iter()
        .filter(|c| {
            c.below_the_floor() && c.spawn_mph.is_some_and(|spawn| spawn < c.here_mph - 1.0)
        })
        .count();
    println!();
    println!(
        "BUBBLE CENSUS ({} sightings over {census_samples} counts)",
        census.len()
    );
    let bubble_mi = ff_core::sim::traffic_manager::BUBBLE_BEHIND_MI
        + ff_core::sim::traffic_manager::BUBBLE_AHEAD_MI;
    println!(
        "  vehicles in the bubble, mean               : {:.2} ({:.3} per mile of road)",
        census.len() as f64 / census_samples.max(1) as f64,
        census.len() as f64 / census_samples.max(1) as f64 / bubble_mi
    );
    println!(
        "  per mile the VOLUME MODEL asks for         : {:.3}",
        implied_per_mile / census_samples.max(1) as f64
    );
    println!(
        "  vehicles PLACED per hundred miles          : {:.1} ({placed} in all)",
        per_hundred(placed, single_mi + multi_mi)
    );
    println!(
        "  slower than any speed this road could DRAW : {below} ({:.1}%)",
        per_hundred(below, census.len() as f64)
    );
    println!(
        "  ... of those, drawn where the road was slower: {carried} ({:.1}% of all)",
        per_hundred(carried, census.len() as f64)
    );
    let mut share_bands: BTreeMap<i64, usize> = BTreeMap::new();
    for c in &census {
        *share_bands.entry((c.share() * 10.0) as i64).or_insert(0) += 1;
    }
    println!("  speed as a share of the posted number, by tenth:");
    for (band, count) in &share_bands {
        println!(
            "    {:.1}-{:.1}  {count}",
            *band as f64 / 10.0,
            (*band as f64 + 1.0) / 10.0
        );
    }

    println!();
    println!("BY CAUSE");
    let mut by_cause: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();
    for e in &events {
        let entry = by_cause.entry(e.cause()).or_insert((0, 0));
        entry.0 += 1;
        if e.lanes < 2 {
            entry.1 += 1;
        }
    }
    for (cause, (count, single)) in &by_cause {
        println!("  {cause:<10} {count:>3}  ({single} of them on a one-lane stretch)");
    }

    println!();
    println!("BY LEAD CLASS");
    let mut by_lead: BTreeMap<String, usize> = BTreeMap::new();
    for e in &events {
        *by_lead.entry(e.lead_class.clone()).or_insert(0) += 1;
    }
    for (class, count) in &by_lead {
        println!("  {class:<16} {count}");
    }

    println!();
    println!("WHAT THE BUBBLE PUT OUT THERE");
    let mut mix: BTreeMap<(String, String), usize> = BTreeMap::new();
    for c in &census {
        *mix.entry((c.class.clone(), c.intent.clone())).or_insert(0) += 1;
    }
    for ((class, intent), count) in &mix {
        println!("  {class:<16} {intent:<12} {count}");
    }

    println!();
    println!("EVERY EVENT");
    println!(
        "{:<34} {:>5} {:>4} {:>6} {:>7} {:>7} {:>7} {:>7} {:>7} {:>7}",
        "corridor",
        "mile",
        "ln",
        "posted",
        "truck",
        "lead",
        "gap mi",
        "closing",
        "gave up",
        "recov s"
    );
    for e in &events {
        let diagnosis = format!(
            "seed {} {:<8} {} {} tgt {:.1} climb {:.1} grade {:.2}% here {:.0} drawn-at {:?}              zone {}",
            e.seed,
            e.cause(),
            e.lead_class,
            e.lead_intent,
            e.lead_target_mph,
            e.lead_climb_mph,
            e.lead_grade_pct,
            e.lead_here_mph,
            e.lead_spawn_mph,
            e.braking_zone
        );
        println!(
            "{:<34} {:>5.1} {:>4} {:>6.0} {:>7.1} {:>7.1} {:>7.3} {:>7.1} {:>7.1} {:>7.0}  {} | {} | {}",
            e.corridor,
            e.mile,
            e.lanes,
            e.posted_mph,
            e.truck_mph,
            e.lead_mph,
            e.gap_mi,
            e.closing_mph,
            e.given_up_mph(),
            e.recover_s,
            diagnosis,
            e.call,
            e.resolution,
        );
    }
}

// -- how hard the response is, split by whether there was anywhere to go -------------------

/// Mean of a sample, or NaN for an empty one.
fn mean(values: &[f64]) -> f64 {
    let live: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if live.is_empty() {
        f64::NAN
    } else {
        live.iter().sum::<f64>() / live.len() as f64
    }
}

/// Median of a sample, or NaN for an empty one.
fn median(values: &[f64]) -> f64 {
    let mut live: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    if live.is_empty() {
        return f64::NAN;
    }
    live.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
    live[live.len() / 2]
}

/// What the assist did on one bucket of roads, printed as one block.
fn report_bucket(label: &str, events: &[&Forced], miles: f64) {
    let acted: Vec<&&Forced> = events
        .iter()
        .filter(|e| e.assist_gap_mi.is_finite())
        .collect();
    let gaps_ft: Vec<f64> = acted.iter().map(|e| e.assist_gap_mi * 5280.0).collect();
    let ttcs: Vec<f64> = acted.iter().map(|e| e.assist_ttc_s).collect();
    let at_mph: Vec<f64> = acted.iter().map(|e| e.assist_mph).collect();
    let left: Vec<f64> = acted.iter().map(|e| e.assist_deadline_s).collect();
    let given_up: Vec<f64> = events.iter().map(|e| e.given_up_mph()).collect();
    let nearest_ft: Vec<f64> = events.iter().map(|e| e.nearest_gap_mi * 5280.0).collect();
    let emergencies = events.iter().filter(|e| e.emergency).count();
    println!("  {label}");
    println!(
        "    forced slow-downs          : {} ({:.2} per 100 mi over {:.0} mi)",
        events.len(),
        per_hundred(events.len(), miles),
        miles
    );
    let granted: Vec<f64> = events.iter().map(|e| e.granted_s).collect();
    println!(
        "    window granted at the call : mean {:.2} s, median {:.2} s",
        mean(&granted),
        median(&granted)
    );
    println!("    the assist had to act      : {} of them", acted.len());
    println!(
        "    closing distance when it did: mean {:.0} ft, median {:.0} ft",
        mean(&gaps_ft),
        median(&gaps_ft)
    );
    println!(
        "    deadline left when it did   : mean {:.2} s, median {:.2} s",
        mean(&left),
        median(&left)
    );
    println!(
        "    (geometric time to contact) : mean {:.2} s, median {:.2} s",
        mean(&ttcs),
        median(&ttcs)
    );
    println!(
        "    speed when it acted         : mean {:.1} mph",
        mean(&at_mph)
    );
    println!(
        "    speed given up              : mean {:.1} mph, median {:.1} mph",
        mean(&given_up),
        median(&given_up)
    );
    println!(
        "    nearest the lead while live : worst {:.0} ft, mean {:.0} ft, median {:.0} ft",
        nearest_ft
            .iter()
            .copied()
            .filter(|v| v.is_finite())
            .fold(f64::INFINITY, f64::min),
        mean(&nearest_ft),
        median(&nearest_ft)
    );
    println!("    emergency application spent : {emergencies}");
    // The emergency application is reserved for a stop MEASURED to be losing
    // ground, so any one of them has to be shown rather than counted.
    for e in events.iter().filter(|e| e.emergency) {
        println!(
            "      -> {} mile {:.1}, {} lanes, truck {:.1} on a {:.0} limit, lead {:.1} ({} {}),              grade {:.2}%, window {:.2} s",
            e.corridor,
            e.mile,
            e.lanes,
            e.truck_mph,
            e.posted_mph,
            e.lead_mph,
            e.lead_class,
            e.lead_intent,
            e.lead_grade_pct,
            e.granted_s
        );
    }
}

/// One controlled meeting with a slower vehicle: same lead, same speeds, same
/// closing rate, and nothing different but how many lanes the road has.
///
/// The sweep answers what the field does; this answers the question the field
/// cannot, because no two real encounters are alike. Everything is measured
/// against the lead the warning was about.
#[derive(Debug, Clone)]
pub struct BenchRun {
    pub lanes: i64,
    pub call: String,
    pub resolution: String,
    /// Gap when the warning landed, and when the assist first took the pedal.
    pub warn_gap_ft: f64,
    pub assist_gap_ft: f64,
    pub assist_ttc_s: f64,
    pub assist_mph: f64,
    /// Slowest the truck got, and closest it ever came to the lead.
    pub bottom_mph: f64,
    pub nearest_gap_ft: f64,
    pub emergency: bool,
    /// Game seconds from the warning to the assist acting.
    pub assist_after_s: f64,
    /// The whole window the call bought before the assist had to act.
    pub granted_s: f64,
    /// Time left on the hazard's deadline when the assist acted.
    pub assist_deadline_s: f64,
}

/// Drive into a lead doing `lead_mph` on a road of `lanes_your_side` lanes,
/// hands off from the moment the warning lands, and measure the response.
/// Lane keeping on partial: on full the truck passes instead of braking
/// (`states_driving_passing`), and hands off is the driver's case.
pub fn bench_lead(lanes_your_side: i64, lead_mph: f64) -> BenchRun {
    use ff_core::sim::traffic_manager::TrafficVehicle;

    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.lane_keeping = "partial".to_string();
    harness.app.ctx.settings.speed_keeper = false;
    harness.app.ctx.settings.automatic_emergency_braking = true;
    harness.app.ctx.settings.time_scale = 1.0;
    harness.start_route(
        "aberdeen_sd_us",
        "pierre_sd_us",
        RouteSetup::seeded(7).named("Lead Response"),
    );
    harness.with_drive(|d, ctx| {
        lane_bench(d, 65.0, lanes_your_side);
        d.trip.time_scale = 1.0;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.posts.clear();
        d.weather_mut().current = WeatherKind::Clear;
        d.departure_checked = true;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().set_air_ready(false);
        d.trip.position_mi = 20.0;
        d.truck_mut().velocity_mps = 65.0 / 2.23694;
        d.trip.traffic_manager.rolling_bubble = false;
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
    });
    assert_eq!(
        harness.read_drive(|d| d.trip.lane_count_at(None)),
        lanes_your_side,
        "the bench did not build the road the case asked for"
    );
    harness.clear_speech();
    harness.with_drive(move |d, _| {
        let gap_mi = lead_mph * (TRAFFIC_WARNING_GAP_S - 0.4) / 3600.0;
        let mut lead = TrafficVehicle::new(
            "bench:lead",
            d.trip.position_mi + gap_mi,
            lead_mph,
            lead_mph,
            0,
            "following",
            "car",
        );
        lead.lane = 0;
        d.trip.set_npc_vehicles(vec![lead]);
    });

    let mut run = BenchRun {
        lanes: lanes_your_side,
        call: String::new(),
        resolution: String::new(),
        warn_gap_ft: f64::NAN,
        assist_gap_ft: f64::NAN,
        assist_ttc_s: f64::NAN,
        assist_mph: f64::NAN,
        bottom_mph: f64::INFINITY,
        nearest_gap_ft: f64::INFINITY,
        emergency: false,
        assist_after_s: f64::NAN,
        granted_s: f64::NAN,
        assist_deadline_s: f64::NAN,
    };
    let mut warned = false;
    let mut since_warn = 0.0;
    for _ in 0..(60 * 180) {
        if !harness.has_drive() {
            break;
        }
        let rolling = !warned && harness.read_drive(|d| d.truck().speed_mph() < 64.0);
        if rolling {
            hold(&mut harness, &[Key::Up]);
        } else {
            release_keys(&mut harness);
        }
        frame(&mut harness, DT);
        let (speed, live, aeb_on, emergency, gap_mi, left_s) = harness.read_drive(|d| {
            let lead = d
                .trip
                .traffic_manager
                .vehicles
                .iter()
                .find(|v| v.key == "bench:lead");
            (
                d.truck().speed_mph(),
                d.hazard_deadline.is_some(),
                d.aeb_brake > 0.0,
                d.aeb_emergency,
                lead.map(|v| v.position_mi - d.trip.position_mi),
                d.hazard_deadline.unwrap_or(f64::NAN),
            )
        });
        if live && !warned {
            warned = true;
            run.warn_gap_ft = gap_mi.unwrap_or(f64::NAN) * 5280.0;
            run.granted_s = harness
                .read_drive(|d| d.hazard_deadline)
                .unwrap_or(f64::NAN);
            run.call = harness
                .transcript()
                .iter()
                .find(|line| line.contains("Brake!") || line.contains("brake!"))
                .cloned()
                .unwrap_or_default();
        }
        if warned {
            since_warn += DT;
        }
        run.emergency |= emergency;
        run.bottom_mph = run.bottom_mph.min(speed);
        if let Some(gap) = gap_mi.filter(|_| live) {
            run.nearest_gap_ft = run.nearest_gap_ft.min(gap * 5280.0);
            if aeb_on && run.assist_gap_ft.is_nan() {
                run.assist_gap_ft = gap * 5280.0;
                run.assist_mph = speed;
                run.assist_deadline_s = left_s;
                run.assist_after_s = since_warn;
                let closing = speed - lead_mph;
                run.assist_ttc_s = if closing > 0.1 {
                    gap / closing * 3600.0
                } else {
                    f64::INFINITY
                };
            }
        }
        if warned && !live {
            run.resolution = harness
                .transcript()
                .iter()
                .find(|line| line.contains("Clear of"))
                .cloned()
                .unwrap_or_default();
            break;
        }
        harness.with_drive(move |d, _| {
            d.lane.offset = 0.0;
            d.lane.steering = 0.0;
            d.lane.lane = 0;
            if let Some(lead) = d.trip.traffic_manager.vehicles.first_mut() {
                lead.speed_mph = lead_mph;
                lead.target_speed_mph = lead_mph;
                lead.limit_offset_mph = None;
            }
        });
    }
    run
}

fn report_bench(run: &BenchRun) {
    println!("  {} lane(s) your side, truck at 65", run.lanes);
    println!("    call                        : {}", run.call);
    println!(
        "    gap when the warning landed : {:.0} ft, window granted {:.2} s",
        run.warn_gap_ft, run.granted_s
    );
    println!(
        "    the assist first acted      : {:.2} s after the call, {:.2} s left on the deadline, at {:.1} mph",
        run.assist_after_s, run.assist_deadline_s, run.assist_mph
    );
    println!(
        "    ... the lead was then        : {:.0} ft away (geometric time to contact {:.2} s)",
        run.assist_gap_ft, run.assist_ttc_s
    );
    println!(
        "    slowest, and nearest the lead: {:.1} mph, {:.0} ft",
        run.bottom_mph, run.nearest_gap_ft
    );
    println!("    emergency application spent : {}", run.emergency);
    println!("    resolution                  : {}", run.resolution);
}

#[test]
#[ignore = "measurement bench: run with --ignored --nocapture"]
fn assist_response_paired_bench() {
    // Two leads: one the truck can coast down to, and one it cannot. The
    // second is the case the assist exists for.
    for lead_mph in [45.0, 10.0] {
        println!("THE SAME MEETING ON TWO ROADS, LEAD DOING {lead_mph:.0}");
        for lanes in [1, 2] {
            report_bench(&bench_lead(lanes, lead_mph));
        }
    }
}

/// Seeds for the response sweep. Its own, not the rate sweep's: the question
/// is what the assist DOES rather than how often it has to, and a rate of well
/// under one an a hundred miles needs more deliveries before a difference of
/// one event stops being noise.
pub const RESPONSE_SEEDS: [i64; 6] = [11, 4242, 90210, 31337, 5150, 777];

/// The measurement the response fix is judged on: a hundred and twenty seeded
/// deliveries over twenty corridors, half single-lane-heavy, weather pinned.
///
/// The question is not how OFTEN traffic holds the truck up -- that is the
/// sweep above, and it must not move -- but how the game RESPONDS when it
/// does, split by whether the road offered anywhere to go.
#[test]
#[ignore = "measurement sweep: run with --release --ignored --nocapture"]
fn assist_response_by_lane_count_sweep() {
    let world = get_world();
    let per_kind = env_f64("FF_TRAFFIC_PER_KIND", 10.0) as usize;
    let miles_per_run = env_f64("FF_TRAFFIC_MILES", MILES_PER_RUN);
    let (single, multi) = corridors(world, per_kind);
    println!(
        "{} single-lane-heavy corridors, {} multi-lane, {} seeds each",
        single.len(),
        multi.len(),
        RESPONSE_SEEDS.len()
    );
    let mut events: Vec<Forced> = Vec::new();
    let mut single_mi = 0.0;
    let mut multi_mi = 0.0;
    for corridor in single.iter().chain(multi.iter()) {
        for (i, seed) in RESPONSE_SEEDS.iter().enumerate() {
            let run = drive_corridor(corridor, *seed, HOURS[i % HOURS.len()], miles_per_run);
            single_mi += run.single_mi;
            multi_mi += run.multi_mi;
            events.extend(run.forced);
        }
    }
    let one: Vec<&Forced> = events.iter().filter(|e| e.lanes < 2).collect();
    let more: Vec<&Forced> = events.iter().filter(|e| e.lanes >= 2).collect();
    println!();
    println!("ASSIST RESPONSE TO A LEAD VEHICLE");
    report_bucket("one lane your side (nowhere to go)", &one, single_mi);
    report_bucket("two or more (a lane to take)", &more, multi_mi);
}

// -- what the sweep earned ---------------------------------------------------------------

/// A bounded version of the sweep for the normal suite: fewer corridors,
/// fewer seeds, shorter runs, same roads.
fn bounded_sweep() -> (Vec<Forced>, Vec<Census>, f64, usize) {
    let world = get_world();
    let (single, multi) = corridors(world, 3);
    let mut events: Vec<Forced> = Vec::new();
    let mut census: Vec<Census> = Vec::new();
    let mut miles = 0.0;
    let mut placed = 0usize;
    for corridor in single.iter().chain(multi.iter()) {
        for (i, seed) in SEEDS.iter().take(2).enumerate() {
            let run = drive_corridor(corridor, *seed, HOURS[i % HOURS.len()], 60.0);
            miles += run.miles();
            placed += run.keys.len();
            events.extend(run.forced);
            census.extend(run.census);
        }
    }
    (events, census, miles, placed)
}

#[test]
#[cfg_attr(
    ci_quick,
    ignore = "sweep: seeded deliveries over a spread of real corridors -- left to the nightly by --cfg ci_quick"
)]
fn traffic_drives_the_road_it_is_on_not_the_road_it_was_drawn_on() {
    // The defect this file was written for. A bubble vehicle's speed used to
    // be drawn once, from the number posted in the cell it appeared in, and
    // kept for the rest of its life. A US route drops to thirty through every
    // town it passes, so a car drawn in one carried thirty out onto the
    // sixty-five on the far side, where the truck met it as a wall nothing on
    // the road explained -- and on a one-lane stretch there was no way past
    // it. Over 5 200 seeded route miles that was most of the traffic the game
    // made the truck brake for.
    //
    // The invariant is exact, and needs no threshold: whatever a vehicle is
    // DOING, what the model is steering it toward can never be slower than
    // the slowest speed a draw for the road under it could have produced.
    // (Its actual speed may still be under that while it accelerates out of a
    // town, which is why this reads the target.)
    let (_, census, _, _) = bounded_sweep();
    assert!(!census.is_empty(), "the sweep put nothing on the road");
    let aimed_low: Vec<&Census> = census
        .iter()
        .filter(|c| c.aimed_below_the_floor())
        .collect();
    assert!(
        aimed_low.is_empty(),
        "{} of {} bubble vehicles are being steered slower than their own road's floor, \
         e.g. {:?}",
        aimed_low.len(),
        census.len(),
        aimed_low.iter().take(3).collect::<Vec<_>>()
    );
}

#[test]
#[cfg_attr(
    ci_quick,
    ignore = "sweep: seeded deliveries over a spread of real corridors -- left to the nightly by --cfg ci_quick"
)]
fn nobody_is_on_the_brakes_where_the_road_gives_no_reason() {
    // "Merging is POSITIONAL: it happens at interchanges, and hard braking
    // happens in congestion placed from real volumes" -- and both spawners
    // used to hand the braking intent out anywhere. A vehicle on the brakes
    // with nothing behind it has no cause line to offer either, so the
    // warning came out as a bare "Brake lights right ahead." and stopped
    // there, which is exactly the invented phantom wave the bubble refuses
    // to place.
    let (events, census, _, _) = bounded_sweep();
    let phantoms: Vec<&Census> = census.iter().filter(|c| c.braking_unplaced).collect();
    assert!(
        phantoms.is_empty(),
        "{} bubble vehicles are braking where the road gives no reason, e.g. {:?}",
        phantoms.len(),
        phantoms.iter().take(3).collect::<Vec<_>>()
    );
    for event in &events {
        assert!(
            !event.intent_is_braking() || event.braking_zone,
            "the truck was made to brake for phantom brake lights: {event:?}"
        );
    }
}

#[test]
#[cfg_attr(
    ci_quick,
    ignore = "sweep: seeded deliveries over a spread of real corridors -- left to the nightly by --cfg ci_quick"
)]
fn the_road_still_has_traffic_on_it() {
    // The guard on every change in this file. The complaint that started it
    // was that traffic holds the truck up too often, and the cheapest way to
    // make that complaint go away is to put less traffic out there -- which
    // would be throwing away the feature rather than fixing it. Traffic where
    // traffic really is is what the AADT bake exists for.
    //
    // The floor is set far under what the volume model itself asks for -- the
    // bubble places about 42 vehicles per hundred miles and its own model
    // would carry several times that at once -- so this catches a road being
    // emptied without pretending to know the right number.
    let (events, _, miles, placed) = bounded_sweep();
    assert!(miles > 500.0, "the sweep only drove {miles:.0} miles");
    assert!(
        per_hundred(placed, miles) >= 20.0,
        "only {:.1} vehicles placed per hundred miles: the road has been emptied",
        per_hundred(placed, miles)
    );
    assert!(
        !events.is_empty(),
        "not one vehicle held the truck up in {miles:.0} miles: traffic that never \
         costs anything is not traffic"
    );
}

#[test]
#[cfg_attr(
    ci_quick,
    ignore = "sweep: seeded deliveries over a spread of real corridors -- left to the nightly by --cfg ci_quick"
)]
fn every_forced_slow_down_is_one_the_road_explains() {
    // What the whole sweep is for, as an assertion: the truck may only be
    // made to brake for a vehicle whose speed the road it is on could have
    // produced. Before this file, seven of every ten were for a vehicle
    // carrying a number in from somewhere slower.
    let (events, _, _, _) = bounded_sweep();
    for event in &events {
        assert!(
            matches!(event.cause(), "road" | "climbing"),
            "the truck braked for traffic the road does not explain: {event:?}"
        );
        // Whatever it is doing at this instant, what the model is STEERING it
        // toward has to be a speed this road could have drawn.
        assert!(
            event.lead_target_mph >= event.lead_floor_mph() - 0.5,
            "the lead is being steered at {:.1} on a road posted {:.0}, whose slowest draw              is {:.1}: {event:?}",
            event.lead_target_mph,
            event.lead_here_mph,
            event.lead_floor_mph()
        );
    }
}

// -- what the driver is told when there is no way past ------------------------------------

/// A long straight leg posted at `limit_mph`, with the lane count the caller
/// wants and nothing else on it.
///
/// `divided` is what `Trip::lanes_at` falls back to when a leg carries no
/// baked lane segments, and it is the same answer the driving state steers
/// by, so a leg marked undivided is a road with one lane your side.
pub fn lane_bench(drive: &mut DrivingState, limit_mph: f64, lanes_your_side: i64) {
    use ff_core::data::world_models::{CorridorDetail, GradeSegment, Leg, Route, SpeedLimitSample};

    let city = drive.trip.route.cities[0].clone();
    let far = drive
        .trip
        .route
        .cities
        .last()
        .cloned()
        .unwrap_or_else(|| city.clone());
    let detail = CorridorDetail {
        speed_limits: vec![SpeedLimitSample {
            at_mi: 0.0,
            mph: Some(limit_mph),
            source: "test bench".to_string(),
            hgv: false,
        }],
        grade_segments: vec![GradeSegment::new(0.0, 200.0, 0.0, "flat", "test bench")],
        ..Default::default()
    };
    // Two DIFFERENT cities: a route that begins and ends in the same place is
    // a facility approach, and `check_hazards` declines to ambush a yard
    // crawl -- which would quietly make this bench measure nothing.
    let mut leg = Leg::new(&city, &far, 200.0, "US-83", "flat", Vec::new()).with_detail(detail);
    leg.divided = Some(lanes_your_side >= 2);
    leg.lanes = lanes_your_side;
    let route = Route::from_legs(vec![city, far], vec![leg]);
    drive.trip.route = route;
    // The traffic manager clamps every vehicle to the lanes ITS route has
    // here; pointed at the old road it folded a left-lane vehicle into the
    // right lane on the first frame.
    drive
        .trip
        .traffic_manager
        .set_route(&drive.trip.route, &drive.trip.leg_starts);
    drive.reset_turn_state_for_trip();
    drive.trip.zones.clear();
    drive.trip.curves.clear();
    drive.destination_exit_taken = true;
}

/// Drive into a lead doing `lead_mph` on a road of `lanes_your_side` lanes,
/// hands off, and report every line heard and the slowest the truck got.
/// Lane keeping on partial, for the reason [`bench_lead`] gives.
fn met_a_slow_lead(name: &str, lanes_your_side: i64, lead_mph: f64) -> (Vec<String>, f64) {
    use ff_core::sim::traffic_manager::TrafficVehicle;

    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.lane_keeping = "partial".to_string();
    harness.app.ctx.settings.speed_keeper = false;
    harness.app.ctx.settings.automatic_emergency_braking = true;
    harness.app.ctx.settings.time_scale = 1.0;
    harness.start_route(
        "aberdeen_sd_us",
        "pierre_sd_us",
        RouteSetup::seeded(7).named(name),
    );
    harness.with_drive(|d, ctx| {
        lane_bench(d, 65.0, lanes_your_side);
        d.trip.time_scale = 1.0;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.posts.clear();
        d.weather_mut().current = WeatherKind::Clear;
        d.departure_checked = true;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().set_air_ready(false);
        d.trip.position_mi = 20.0;
        d.truck_mut().velocity_mps = 65.0 / 2.23694;
        d.trip.traffic_manager.rolling_bubble = false;
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
    });
    assert_eq!(
        harness.read_drive(|d| d.trip.lane_count_at(None)),
        lanes_your_side,
        "the bench did not build the road the case asked for"
    );
    harness.clear_speech();
    // A lead already inside the warning window: the case is about what is
    // said and done, not about how long the catching up takes.
    harness.with_drive(move |d, _| {
        let gap_mi = lead_mph * (TRAFFIC_WARNING_GAP_S - 0.4) / 3600.0;
        let mut lead = TrafficVehicle::new(
            "bench:lead",
            d.trip.position_mi + gap_mi,
            lead_mph,
            lead_mph,
            0,
            "following",
            "car",
        );
        lead.lane = 0;
        d.trip.set_npc_vehicles(vec![lead]);
    });
    let mut bottom_mph = f64::INFINITY;
    let mut warned = false;
    for _ in 0..(60 * 180) {
        if !harness.has_drive() {
            break;
        }
        // Up to the warning the driver is holding the road speed; from the
        // moment it lands they are hands off, because the assist is what the
        // owner's report is about.
        let rolling = !warned && harness.read_drive(|d| d.truck().speed_mph() < 64.0);
        if rolling {
            hold(&mut harness, &[Key::Up]);
        } else {
            release_keys(&mut harness);
        }
        frame(&mut harness, DT);
        let (speed, live) =
            harness.read_drive(|d| (d.truck().speed_mph(), d.hazard_deadline.is_some()));
        warned |= live;
        bottom_mph = bottom_mph.min(speed);
        if warned && !live {
            break;
        }
        // The lead holds its own steady speed; the manager is what moves it.
        // And the truck is held dead centre in its lane: the wander model is
        // its own suite's business, and letting the truck drift across the
        // line resolves the hazard by a route this case is not about.
        harness.with_drive(move |d, _| {
            d.lane.offset = 0.0;
            d.lane.steering = 0.0;
            d.lane.lane = 0;
            if let Some(lead) = d.trip.traffic_manager.vehicles.first_mut() {
                lead.speed_mph = lead_mph;
                lead.target_speed_mph = lead_mph;
                lead.limit_offset_mph = None;
            }
        });
    }
    (harness.transcript(), bottom_mph)
}

#[test]
fn with_no_lane_to_go_around_in_the_call_is_brake_and_nothing_else() {
    // The owner's case, 2026-08-24: one lane, a slower vehicle ahead, no way
    // past. What the game must never do here is offer a lane change, and what
    // it must never do afterwards is claim the truck took one.
    let (heard, bottom_mph) = met_a_slow_lead("One Lane Lead", 1, 45.0);
    let call = heard
        .iter()
        .find(|line| line.contains("brake!") || line.contains("Brake!"))
        .unwrap_or_else(|| panic!("no lead warning was spoken: {heard:#?}"));
    assert!(
        call.contains("Brake! Slow car"),
        "a one-lane road offered a lane change: {call:?}"
    );
    assert!(!call.contains("Change lanes"), "{call:?}");
    let resolution = heard
        .iter()
        .find(|line| line.contains("Clear of"))
        .unwrap_or_else(|| panic!("the hazard never resolved: {heard:#?}"));
    // What actually happened is that the truck came down to the vehicle's
    // speed. It did not nearly stop, and there was nowhere to ease around to.
    assert!(
        resolution.contains("Clear of the slow car."),
        "{resolution:?}"
    );
    assert!(!resolution.contains("ease around"), "{resolution:?}");
    // And the stop is to the LEAD's speed, not the near-stop a fixed object
    // in the lane demands (Brandon, 2026-08-23).
    assert!(
        bottom_mph > HAZARD_SAFE_MPH,
        "the truck was dragged to {bottom_mph:.1} for a vehicle doing 45"
    );
    assert!(
        bottom_mph < 50.0,
        "the truck never came down to the lead at all: {bottom_mph:.1}"
    );
}

#[test]
fn with_a_lane_open_the_call_offers_it() {
    // The other half of the same distinction, so a change to either wording
    // cannot quietly collapse the two cases into one.
    let (heard, _) = met_a_slow_lead("Two Lane Lead", 2, 45.0);
    let call = heard
        .iter()
        .find(|line| line.contains("brake!") || line.contains("Brake!"))
        .unwrap_or_else(|| panic!("no lead warning was spoken: {heard:#?}"));
    assert!(
        call.contains("Change lanes or brake! Slow car"),
        "a two-lane road did not offer the lane change: {call:?}"
    );
    let resolution = heard
        .iter()
        .find(|line| line.contains("Clear of"))
        .unwrap_or_else(|| panic!("the hazard never resolved: {heard:#?}"));
    // Braking down to the vehicle's speed is not going around it, whatever
    // the road offers: the swerve has its own line, spoken from the lane
    // change itself.
    assert!(
        resolution.contains("Clear of the slow car."),
        "{resolution:?}"
    );
    assert!(!resolution.contains("ease around"), "{resolution:?}");
}

#[test]
#[cfg_attr(
    ci_quick,
    ignore = "sweep: seeded deliveries over a spread of real corridors -- left to the nightly by --cfg ci_quick"
)]
fn nobody_is_placed_in_a_lane_the_road_does_not_have() {
    // "Passing traffic lives in the left lane" was applied to every road,
    // including the two-lane US routes that have no left lane. A vehicle put
    // there is in a lane that does not exist: it can never be the lead the
    // driver has to deal with, and its pass-by whoosh pans to the side of a
    // road that has no side. Nothing about the owner's report turns on it --
    // a phantom-lane vehicle makes the road EASIER -- but it is the road
    // being described to a blind driver as something it is not.
    let (_, census, _, _) = bounded_sweep();
    let elsewhere: Vec<&Census> = census.iter().filter(|c| c.lane >= c.lanes_here).collect();
    assert!(
        elsewhere.is_empty(),
        "{} of {} bubble vehicles are in a lane their road does not have, e.g. {:?}",
        elsewhere.len(),
        census.len(),
        elsewhere.iter().take(3).collect::<Vec<_>>()
    );
}
