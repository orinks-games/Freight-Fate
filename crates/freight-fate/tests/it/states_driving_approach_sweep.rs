//! The destination approach assist, driven to real facilities all over the map.
//!
//! Owner, 2026-08-24: "destination approach assistance works on some legs and
//! not others" -- on the legs where it fails the truck is never brought to a
//! stop ready to pull in.
//!
//! The four `states_driving_facility.rs` approach-assist cases were deferred
//! on "a hands-off end-to-end drive over baked chain data". This file is that
//! drive; those four now use its rigging for their own narrower bars, and it
//! runs the same drive as a SWEEP, because the defect is precisely that one
//! destination arrives and the next one does not. Every run here pins its
//! trip seed and its weather: an unseeded delivery draws its own road and its
//! own sky, and letting dispatch's random draw decide which shape got
//! measured is what hid this.
//!
//! The driver in these runs is a player, not a ghost. They roll the ramp and
//! the city streets at the posted number, and they lift the moment the assist
//! announces it has the pedals -- from there nothing but the game touches the
//! truck, which is what "it stops me at the destination" has to mean.

use ff_core::data::world::{get_world, World};
use ff_core::data::world_models::{CorridorDetail, GradeSegment, Leg, Route};
use ff_core::models::career::{Career, LEVEL_XP};
use ff_core::models::carrier_fleet::{assigned_truck_key, FLEET_TIERS};
use ff_core::models::jobs::{cargo_type, Job};
use ff_core::models::profile::Profile;
use ff_core::models::trailers::trailer_keys_for_cargo;
use ff_core::models::trucks::truck_model_or_panic;
use ff_core::sim::trip_models::FACILITY_GATE_LIMIT_MPH;
use ff_core::sim::vehicle::{G, KG_PER_TON, REFERENCE_CARGO_KG};
use ff_core::sim::weather::WeatherKind;

use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};
use freight_fate::states::base::Key;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{
    DOCKING_MAX_MPH, FACILITY_LANE_ROLL_MPH, RAMP_ACCESS_MI, RED_STOP_MPH,
};
use freight_fate::states::driving_menu_states::FacilityArrivalState;

use crate::transcript_cruise_support::{frame, hold, quiet, release_keys, DT, MPS_PER_MPH};

// -- picking the destinations ----------------------------------------------------------

/// A destination the sweep drives to: which city, which facility, and whether
/// the facility's approach is a real turn-level street chain.
#[derive(Debug, Clone)]
pub struct Destination {
    pub city: String,
    pub location: String,
    /// The state whose vehicle code governs its streets. One per state is how
    /// the sweep spreads itself; it also reads back in a failure.
    pub state: String,
    pub chain: bool,
}

impl Destination {
    pub fn kind(&self) -> &'static str {
        if self.chain {
            "street chain"
        } else {
            "plain"
        }
    }
}

/// One chain-capable facility and one plain one per state, walked in a fixed
/// order so the sweep drives the same places every run.
///
/// Both kinds have to be covered, and covered separately, because the two take
/// different roads to the same gate: a plain facility's ramp ends AT the gate,
/// while a chain facility's ramp hands off to up to a mile of city streets
/// that are a trip of their own.
pub fn destinations(world: &'static World, want: usize) -> (Vec<Destination>, Vec<Destination>) {
    let mut keys: Vec<&String> = world.cities.keys().collect();
    keys.sort();
    let mut chain: Vec<Destination> = Vec::new();
    let mut plain: Vec<Destination> = Vec::new();
    let mut chain_states: Vec<String> = Vec::new();
    let mut plain_states: Vec<String> = Vec::new();
    for city in keys {
        let Some(entry) = world.cities.get(city) else {
            continue;
        };
        // A destination is only drivable here if the world has somewhere to
        // start from that connects straight to it.
        if world.neighbors(city).is_empty() {
            continue;
        }
        let state = entry.state.clone();
        for location in &entry.locations {
            let Ok(route) = world.facility_approach_route(city, &location.name) else {
                continue;
            };
            // The same bar `surface_chain_route` applies: a genuine
            // multi-segment turn-level route, not a single synthetic leg.
            let is_chain =
                route.legs.len() >= 2 && route.legs.iter().any(|leg| leg.local_speed_mph > 0.0);
            let (bucket, seen) = if is_chain {
                (&mut chain, &mut chain_states)
            } else {
                (&mut plain, &mut plain_states)
            };
            if bucket.len() >= want || seen.contains(&state) {
                continue;
            }
            seen.push(state.clone());
            bucket.push(Destination {
                city: city.clone(),
                location: location.name.clone(),
                state: state.clone(),
                chain: is_chain,
            });
        }
        if chain.len() >= want && plain.len() >= want {
            break;
        }
    }
    (chain, plain)
}

// -- driving one of them ---------------------------------------------------------------

/// What one arrival did.
#[derive(Debug)]
pub struct Arrival {
    /// The dock menu opened, or the assist stopped and held at the gate with
    /// its prompt spoken. Either is "ready to pull in".
    pub ready: bool,
    /// The dock menu opened on its own.
    pub docked: bool,
    /// The assist announced that it had taken the pedals.
    pub assist_spoke: bool,
    /// Road speed when the run ended, mph.
    pub speed_mph: f64,
    /// Road still to the gate when the run ended, feet.
    pub short_by_ft: f64,
    /// The road grade under the gate, as a fraction: positive climbing to it.
    pub gate_grade_pct: f64,
    /// Whether the truck ever reached the facility's street chain.
    pub on_chain: bool,
    /// Road speed the moment the arrival point went under the wheels, mph.
    pub speed_at_point_mph: Option<f64>,
    /// How far the truck ran on past that point before it stopped, feet.
    pub past_the_point_ft: f64,
    /// Carrier-catalog tractor used by this run.
    pub truck_key: String,
    /// Rated pull, so a failed profile names the driveline it used.
    pub max_torque_nm: f64,
    /// Gross combination mass after the trailer and payload are aboard.
    pub gross_mass_kg: f64,
    /// Strongest cold full-service deceleration the truck reported.
    pub full_service_decel_mps2: f64,
    /// Highest feed-forward pedal needed to hold the two-mph gate crawl.
    pub max_creep_hold_throttle: f64,
    /// Slowest physical sample while still short of the gate.
    pub min_creep_speed_mph: Option<f64>,
    /// Lowest gear the automatic selected during the gate crawl.
    pub min_creep_gear: Option<i32>,
    /// A dead engine at the gate would make a successful position misleading.
    pub stalled: bool,
    /// Lowest service-brake pressure seen anywhere on the approach, psi.
    pub min_air_psi: f64,
    /// This truck's own low-air warning pressure, so a case can ask whether
    /// the approach ever got near the cliff without naming a number of its
    /// own.
    pub air_low_warning_psi: f64,
    /// How many times the pedal ROSE on the way in -- the air system charges
    /// a whole application for each one, so this is what the approach spent.
    /// Diagnostic, not a bar: what a chain costs depends on how many corners
    /// it turns.
    pub brake_applications: usize,
    /// Every line the driver heard.
    /// Feet driven at three miles an hour or less after the assist took
    /// the pedals, with more than 250 feet still to the gate.
    pub crawl_short_ft: f64,
    /// [`arrive_hands_off`] set its street speed.
    /// Hands-off runs: each street corner's own number and the speed the
    /// truck took it at, in order.
    pub corner_passes: Vec<(f64, f64)>,
    /// (target, slowest mph in the last 0.1 mi before the corner, mph as
    /// that 0.1 mi began).
    pub corner_mins: Vec<(f64, f64, f64)>,
    pub street_kicked: bool,
    /// Facility stopping assistance had the pedals with a street corner
    /// still ahead of the truck.
    pub latched_with_corner_ahead: bool,
    pub heard: Vec<String>,
}

impl Arrival {
    pub fn said(&self, needle: &str) -> bool {
        self.heard.iter().any(|line| line.contains(needle))
    }

    /// What went wrong, for a failure message that names the place and the
    /// numbers rather than just "assertion failed".
    pub fn report(&self, destination: &Destination) -> String {
        let tail: Vec<&String> = self.heard.iter().rev().take(60).rev().collect();
        format!(
            "{} ({}, {}, {}, truck={} torque={:.0} Nm gross={:.0} kg brake={:.2} m/s2): ready={} \
             assist_spoke={} on_chain={} speed={:.2} mph, {:.0} ft short of the gate, creep hold \
             {:.3}, creep speed {:?}, creep gear {:?}, stalled={}, air floor {:.1} psi over {} \
             brake applications\nlast heard: {:#?}",
            destination.location,
            destination.city,
            destination.state,
            destination.kind(),
            self.truck_key,
            self.max_torque_nm,
            self.gross_mass_kg,
            self.full_service_decel_mps2,
            self.ready,
            self.assist_spoke,
            self.on_chain,
            self.speed_mph,
            self.short_by_ft,
            self.max_creep_hold_throttle,
            self.min_creep_speed_mph,
            self.min_creep_gear,
            self.stalled,
            self.min_air_psi,
            self.brake_applications,
            tail,
        )
    }
}

/// What a competent player would be doing here, in mph.
///
/// The number the game itself holds a ramp at rather than the ramp ceiling: a
/// driver who pushes past the route-transition assist's cap spends the run
/// fighting its brake, and a truck that pumps its air down to the spring
/// brakes is a test of the air system, not of the arrival. On the streets it
/// is the posted number, eased to the advised speed for a corner in play --
/// a driver who hears "turn right, ten miles an hour" and holds twenty-nine
/// through it is testing the missed-turn loop-back, not the assist.
///
/// "In play" is the game's own word for it, and `turn_cue_in_play` is the
/// game's own answer: an unresolved corner within its commit tail, which is
/// the corner the truck is still taking. This used to narrow that to corners
/// whose milepost was still ahead, so the moment the front wheels crossed the
/// corner -- with the "turn right now, advise nine" still being spoken, and
/// the judgement of the corner still a grace period away -- the driver here
/// went back to the posted number and floored it. Measured on the Abilene
/// chain: 8.6 mph to 19.8 in the last seven hundred feet, the corner then
/// failed on the speed the driver had just put on, and the loop-back set the
/// truck down on the gate still doing 19.8. That is the loop-back being
/// tested, which this driver exists not to do.
pub fn driver_target_mph(d: &mut DrivingState) -> f64 {
    if d.ramp_mi.is_some() {
        return d.armed_ramp_mph(None);
    }
    let posted = d.trip.speed_limit_at(d.trip.position_mi).0;
    // The slowest corner in play, not just the nearest: a driver who has
    // heard the next two calls does not build up for a 14 mph left with an
    // 8 mph right a block after it, and doing so here fought the keeper's
    // snub for the slower one with the accelerator, a fresh application
    // every time (Abingdon Company Yard, on the realistic corner speeds).
    let cues = d.turn_cues_in_play();
    cues.iter()
        .map(|cue| d.turn_speed_mph(cue))
        .fold(posted, f64::min)
}

/// Drive the last mile to `destination`: down the destination ramp, through
/// any street chain, to the gate.
pub fn arrive(destination: &Destination) -> Arrival {
    arrive_with(destination, None, None, 18.0)
}

/// [`arrive`], but nobody drives the streets: the driver takes the ramp and
/// lets go at the first street, and from there the speed keeper and facility
/// stopping assistance have every pedal, the way a player on All assists
/// drives into a yard.
///
/// `street_speed_mph`, when given, puts the truck at that speed once, the
/// first frame the gate is within `STREET_KICK_MI` with a corner still
/// ahead: the keeper holding a street limit into the last corner, which is
/// where the arrival used to take the pedals.
pub fn arrive_hands_off(destination: &Destination, street_speed_mph: Option<f64>) -> Arrival {
    COAST_PAST.with(|c| c.set(false));
    arrive_driven(destination, None, None, 18.0, Some(street_speed_mph))
}

/// [`arrive_hands_off`] where, at the street speed's moment, every pedal
/// assist is switched off and the truck coasts on past the last corner: a
/// driver who overran the driveway.
pub fn arrive_coasting_past(destination: &Destination, street_speed_mph: f64) -> Arrival {
    COAST_PAST.with(|c| c.set(true));
    let arrival = arrive_driven(destination, None, None, 18.0, Some(Some(street_speed_mph)));
    COAST_PAST.with(|c| c.set(false));
    arrival
}

thread_local! {
    static COAST_PAST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// How far short of the gate [`arrive_hands_off`] sets its street speed.
pub const STREET_KICK_MI: f64 = 0.38;

/// Re-lay a facility's street chain on a constant grade.
///
/// The shipped chains are built from local street geometry, which carries no
/// grade segments at all, so the road under every one of them reads dead
/// level. A gate at the top of a climb is a real shape and the one a
/// brake-only stop profile gets wrong in the other direction, so the case
/// that wants one has to build it: the same streets, the same cues, the same
/// speeds, on a hill.
fn regrade_chain(d: &mut DrivingState, grade_pct: f64) {
    let city = d.trip.route.cities[0].clone();
    let legs: Vec<Leg> = d
        .trip
        .route
        .legs
        .iter()
        .map(|leg| {
            let detail = CorridorDetail {
                grade_segments: vec![GradeSegment::new(
                    0.0,
                    leg.miles,
                    grade_pct * 100.0,
                    "rolling",
                    "test bench",
                )],
                ..Default::default()
            };
            Leg::local(
                &city,
                leg.miles,
                &leg.highway,
                &leg.local_cue,
                leg.local_speed_mph,
            )
            // Same streets, same cues, same corners -- only the hill is new.
            .with_turn_deg(leg.local_turn_deg)
            .with_detail(detail)
        })
        .collect();
    let cities = vec![city; legs.len() + 1];
    d.trip.route = Route::from_legs(cities, legs);
}

/// Take the traffic controls off the chain under the truck.
pub fn clear_street_controls(d: &mut DrivingState) {
    for leg in d.trip.route.legs.iter_mut() {
        std::sync::Arc::make_mut(leg).local_controls.clear();
    }
}

/// [`arrive`], with the facility's street chain re-laid on a constant grade.
pub fn arrive_over(destination: &Destination, chain_grade_pct: Option<f64>) -> Arrival {
    arrive_with(destination, chain_grade_pct, None, 18.0)
}

/// Drive one tractor and payload through the same physical approach.
fn arrive_with(
    destination: &Destination,
    chain_grade_pct: Option<f64>,
    company_profile: Option<Profile>,
    tons: f64,
) -> Arrival {
    arrive_driven(destination, chain_grade_pct, company_profile, tons, None)
}

fn arrive_driven(
    destination: &Destination,
    chain_grade_pct: Option<f64>,
    company_profile: Option<Profile>,
    tons: f64,
    hands_off_on_streets: Option<Option<f64>>,
) -> Arrival {
    let street_kick = hands_off_on_streets.flatten();
    let hands_off_on_streets = hands_off_on_streets.is_some();
    let mut kicked = false;
    let mut latched_with_corner_ahead = false;
    let mut corner_passes: Vec<(f64, f64)> = Vec::new();
    let mut corner_mins: Vec<(f64, f64, f64)> = Vec::new();
    let mut corner_min = f64::INFINITY;
    let mut corner_entry = f64::NAN;
    let mut corner_in_play: Option<(f64, f64)> = None;
    let world = get_world();
    let origin = if destination.city == "shelby_mt_us" && company_profile.is_some() {
        "helena_mt_us".to_string()
    } else {
        world.neighbors(&destination.city)[0]
            .other(&destination.city)
            .to_string()
    };
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.destination_approach_assist = true;
    harness.app.ctx.settings.speed_keeper = true;
    harness.app.ctx.settings.automatic_transmission = true;
    // Lane keeping holds the WHEEL; this sweep is about the assist that holds
    // the PEDALS. They were the same question while steering was a nudge on
    // the truck's position, and stopped being one when the lane model grew a
    // heading (2026-09-18): a truck nobody steers now genuinely leaves the
    // road, which is the point of that change and not this file's subject.
    // Every run here would otherwise measure whether a ghost can steer.
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    let mut route_setup = RouteSetup::seeded(4242)
        .named("Approach Sweep")
        .destination_location(&destination.location);
    route_setup.tons = tons;
    if origin == "helena_mt_us" {
        route_setup.route_cities = Some(
            world
                .supported_route(&origin, &destination.city, None)
                .expect("Helena to Shelby route lookup must succeed")
                .expect("Helena to Shelby must remain a supported run")
                .cities,
        );
    }
    harness.start_route(&origin, &destination.city, route_setup);
    let configured_truck_key = company_profile
        .as_ref()
        .map(Profile::active_truck_key)
        .unwrap_or_else(|| "rig".to_string());
    harness.with_drive(|d, ctx| {
        if let Some(profile) = company_profile {
            d.trip.truck.specs = profile.truck_specs();
            ctx.profile = Some(profile);
        }
        if destination.city == "shelby_mt_us" {
            // Dispatch-board jobs carry the speakable city. start_route is a
            // lower-level seam, so hydrate the same field and prove the dock
            // never exposes an internal map key to the player.
            d.job.destination_spoken = "Shelby, Montana".to_string();
        }
        quiet(&mut d.trip);
        // Pinned, not drawn: rain changes what the road can shed, and a sweep
        // whose sky differs per destination is measuring the sky.
        d.weather_mut().current = WeatherKind::Clear;
        d.departure_checked = true;
        // A first-run career would talk over the arrival with lesson prompts.
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        // IN GEAR, like a truck already doing highway speed. This used to be
        // left in neutral and got away with it: speed control put throttle
        // down and the automatic engaged from there. Route-transition
        // assistance now pauses speed control when it takes the pedals, so
        // nothing asked for a gear and the truck coasted the whole approach --
        // and the whole gate crawl -- out of gear.
        d.truck_mut().transmission.gear = 9;
        d.truck_mut().rpm = 1500.0;
        d.truck_mut().set_air_ready(false);
        d.speed_control_armed = true;
    });
    let exit = harness.with_drive(|d, ctx| {
        d.destination_exit_stop(ctx)
            .expect("a delivery always has a destination exit")
    });
    let at = exit.at_mi;
    harness.with_drive(move |d, ctx| {
        d.exit_stop = Some(exit);
        d.exit_lane_entered = true;
        d.exit_signal_on = true; // signalled for it, like a driver
        d.trip.position_mi = at;
        d.truck_mut().velocity_mps = 40.0 * MPS_PER_MPH;
        d.update_exit(ctx, 0.0, 0.0);
    });
    assert!(
        harness.read_drive(|d| d.ramp_mi.is_some()),
        "{}: never got onto the destination ramp",
        destination.city
    );
    harness.with_drive(|d, _| {
        // The light or stop sign at the end of a ramp has an assist of its
        // own, and its own suite. Clear it, so the only automation that can
        // bring this truck up at the facility is the one under test.
        d.ramp_control = String::new();
        d.ramp_terminal_done = true;
    });
    harness.clear_speech();

    let mut ready = false;
    let mut docked = false;
    let mut on_chain = false;
    let mut gate_grade_pct = 0.0;
    let mut handed_off = false;
    let mut speed_at_point_mph: Option<f64> = None;
    let mut past_the_point_ft = 0.0;
    let mut max_creep_hold_throttle = 0.0f64;
    let mut min_creep_speed_mph: Option<f64> = None;
    let mut min_creep_gear: Option<i32> = None;
    let mut min_air_psi = f64::INFINITY;
    let mut crawl_short_ft = 0.0f64;
    let mut air_low_warning_psi = 0.0f64;
    let mut brake_applications = 0usize;
    let mut last_brake = 0.0f64;
    // Ten minutes for each mile of city streets at a crawl, and no more: a
    // truck that has not arrived by then is not going to. Per mile, because
    // the streets are as long as the facility is far: this was a flat ten
    // minutes while no sampled chain ran much past a mile, and the 2026-09-17
    // sweep gave Aurora Company Yard its real 3.9 miles of them.
    let chain_mi = world
        .facility_source_approach(&destination.city, &destination.location)
        .ok()
        .flatten()
        .map_or(1.0, |approach| approach.total_miles.max(1.0));
    let budget_frames = (60.0 * 600.0 * chain_mi).ceil() as usize;
    for _ in 0..budget_frames {
        if !harness.has_drive() {
            // The automatic pull-in first replaces the drive with a timed
            // spoken transition. Finish it and require the real dock menu;
            // merely losing the drive is not proof that delivery can continue.
            harness.finish_timed_state();
            ready = harness.state_is::<FacilityArrivalState>();
            docked = ready;
            break;
        }
        if harness.read_drive(|d| d.arrival_menu_open) {
            harness.finish_timed_state();
            ready = harness.state_is::<FacilityArrivalState>();
            docked = ready;
            break;
        }
        let now_on_chain = harness.read_drive(|d| d.surface_chain);
        if now_on_chain && !on_chain {
            // The streets' own lights and signs have an assist of their own
            // and their own suite (`states_driving_street_controls.rs`), the
            // way the ramp's terminal does: cleared for the same reason.
            harness.with_drive(|d, _| clear_street_controls(d));
            if let Some(grade_pct) = chain_grade_pct {
                harness.with_drive(move |d, _| regrade_chain(d, grade_pct));
            }
        }
        on_chain |= now_on_chain;
        // What the approach cost the tanks, every frame of it. An arrival that
        // is measured only on where the truck ENDED cannot tell "stopped at
        // the gate" from "stopped wherever the spring brakes caught it", and
        // that is exactly the shape the 2026-09-18 defect had.
        let (air_psi, pedal, warning_psi) = harness.read_drive(|d| {
            (
                d.truck().air_pressure_psi(),
                d.truck().brake.clamp(0.0, 1.0),
                d.truck().specs.air_low_warning_psi,
            )
        });
        min_air_psi = min_air_psi.min(air_psi);
        air_low_warning_psi = warning_psi;
        if pedal > last_brake + 1e-9 {
            brake_applications += 1;
        }
        last_brake = pedal;
        let (remaining, speed, hold_throttle, gear) = harness.read_drive(|d| {
            (
                d.ramp_mi.unwrap_or_else(|| d.trip.remaining_miles()),
                d.truck().speed_mph(),
                d.truck().hold_throttle(),
                d.truck().transmission.gear,
            )
        });
        // Feet covered at a walk while the gate is still more than the final
        // creep stretch off: road the truck should have driven, not crawled.
        if handed_off && remaining * 5280.0 > 250.0 && speed <= 3.0 {
            crawl_short_ft += speed * MPS_PER_MPH * DT * 3.28084;
        }
        if handed_off && remaining > 0.0 && speed <= 2.1 {
            max_creep_hold_throttle = max_creep_hold_throttle.max(hold_throttle);
            min_creep_speed_mph = Some(min_creep_speed_mph.map_or(speed, |seen| seen.min(speed)));
            min_creep_gear = Some(min_creep_gear.map_or(gear, |seen| seen.min(gear)));
        }
        // The arrival point, and everything after it. Integrated from the
        // truck's own speed rather than read off the trip: `position_mi`
        // jumps when the chain trip is swapped in, so it cannot measure how
        // far the truck ran past a gate it has already passed.
        if speed_at_point_mph.is_none() {
            let at_point = harness.read_drive(|d| {
                (d.trip.remaining_miles() <= 0.0 || d.trip.finished)
                    && d.ramp_mi.is_none()
                    && d.destination_exit_taken
            });
            if at_point {
                speed_at_point_mph = Some(harness.read_drive(|d| d.truck().speed_mph()));
            }
        } else {
            past_the_point_ft += harness.read_drive(|d| d.truck().velocity_mps) * DT * 3.28084;
        }
        // Stopped at the gate with the hold prompt spoken IS the arrival: the
        // assist holds there and waits for the driver to pull in.
        if harness
            .read_drive(|d| d.arrival_full_stop_said && d.truck().speed_mph() <= DOCKING_MAX_MPH)
        {
            ready = true;
            break;
        }
        if !handed_off {
            // The moment the assist claims the pedals the driver lifts, and
            // never touches anything again.
            handed_off = harness.read_drive(|d| d.destination_arrival_active);
            if handed_off {
                release_keys(&mut harness);
                // What the road under the gate is doing, read where the shed
                // begins: an upgrade sheds speed for free and is where a
                // brake-only profile stops SHORT of the gate.
                gate_grade_pct = harness.read_drive(|d| {
                    let end = d.trip.total_miles();
                    (d.trip.grade_at(end) + d.trip.grade_at((end - 0.1).max(0.0))) / 2.0
                });
            }
        }
        if hands_off_on_streets && on_chain {
            // Off the ramp: the keeper has the streets, nobody else.
            release_keys(&mut harness);
            let (corner_ahead, next_corner) = harness.with_drive(|d, _| {
                let position = d.trip.position_mi;
                let cue = d.turn_cue_in_play().filter(|cue| cue.at_mi > position);
                let next = cue.as_ref().map(|cue| (cue.at_mi, d.turn_speed_mph(cue)));
                (cue.is_some(), next)
            });
            // The speed each corner was actually taken at, against its own
            // number: read on the first frame past it.
            if let Some((at_mi, target)) = corner_in_play {
                let (position, speed) =
                    harness.read_drive(|d| (d.trip.position_mi, d.truck().speed_mph()));
                if position >= at_mi || next_corner.is_none_or(|(next, _)| next != at_mi) {
                    corner_passes.push((target, speed));
                    corner_mins.push((target, corner_min.min(speed), corner_entry));
                    corner_min = f64::INFINITY;
                    corner_entry = f64::NAN;
                    corner_in_play = None;
                } else if position >= at_mi - 0.1 {
                    if corner_entry.is_nan() {
                        corner_entry = speed;
                    }
                    corner_min = corner_min.min(speed);
                }
            }
            if corner_in_play.is_none() {
                corner_in_play = next_corner;
            }
            latched_with_corner_ahead |=
                corner_ahead && harness.read_drive(|d| d.destination_arrival_active);
            if let Some(mph) = street_kick.filter(|_| !kicked && corner_ahead) {
                let near = harness.read_drive(|d| d.trip.remaining_miles() <= STREET_KICK_MI);
                if near {
                    kicked = true;
                    let coast = COAST_PAST.with(|c| c.get());
                    harness.with_drive(move |d, ctx| {
                        d.truck_mut().velocity_mps = mph * MPS_PER_MPH;
                        if coast {
                            d.keeper_mph = None;
                            d.speed_control_armed = false;
                            d.keeper_snub = 0.0;
                            d.truck_mut().brake = 0.0;
                            ctx.settings.destination_approach_assist = false;
                            ctx.settings.route_transition_assist = false;
                            ctx.settings.curve_speed_assist = false;
                            ctx.settings.speed_keeper = false;
                        } else {
                            d.keeper_mph = Some(mph);
                        }
                    });
                }
            }
        } else if !handed_off {
            let rolling =
                harness.with_drive(|d, _| driver_target_mph(d) > d.truck().speed_mph() + 2.0);
            if rolling {
                hold(&mut harness, &[Key::Up]);
            } else {
                release_keys(&mut harness);
            }
        }
        frame(&mut harness, DT);
    }
    release_keys(&mut harness);
    let (speed_mph, short_by_ft, max_torque_nm, gross_mass_kg, full_service_decel_mps2, stalled) =
        if harness.has_drive() {
            harness.read_drive(|d| {
                (
                    d.truck().speed_mph(),
                    d.ramp_mi.unwrap_or_else(|| d.trip.remaining_miles()) * 5280.0,
                    d.truck().specs.max_torque_nm,
                    d.truck().gross_mass_kg(),
                    d.truck().full_service_decel_mps2(),
                    d.truck().stalled,
                )
            })
        } else {
            let model = truck_model_or_panic(&configured_truck_key);
            (
                0.0,
                0.0,
                model.specs.max_torque_nm,
                model.specs.mass_kg - REFERENCE_CARGO_KG + tons * KG_PER_TON,
                model.specs.max_brake_decel_g * G,
                false,
            )
        };
    let heard = harness.transcript();
    Arrival {
        ready,
        docked,
        gate_grade_pct,
        // On a ramp that ends at the gate, the deceleration lane's "Slowing
        // for the ramp." is the one line for the slowdown to the gate.
        assist_spoke: heard.iter().any(|line| {
            line.contains("Slowing for the entrance.") || line.contains("Slowing for the ramp.")
        }),
        speed_mph,
        short_by_ft,
        on_chain,
        speed_at_point_mph,
        past_the_point_ft,
        truck_key: configured_truck_key,
        max_torque_nm,
        gross_mass_kg,
        full_service_decel_mps2,
        max_creep_hold_throttle,
        min_creep_speed_mph,
        min_creep_gear,
        stalled,
        min_air_psi,
        air_low_warning_psi,
        brake_applications,
        crawl_short_ft,
        street_kicked: kicked,
        corner_passes,
        corner_mins,
        latched_with_corner_ahead,
        heard,
    }
}

// -- what every arrival owes the driver --------------------------------------------------

/// How many destinations of each kind the sweep drives.
pub const PER_KIND: usize = 25;

/// A tractor-trailer's own length. Stopping AT the gate means stopping within
/// it, not a city block later.
pub const TRUCK_LENGTH_FT: f64 = 70.0;

/// The whole promise, checked on one arrival: it stopped, it stopped at the
/// gate, and it said so. `None` when nothing is wrong.
pub fn what_went_wrong(destination: &Destination, arrival: &Arrival) -> Option<String> {
    let fault = |why: &str| Some(format!("{why}\n{}", arrival.report(destination)));
    // 1. It stopped, and the dock is reachable from where it stopped.
    if !arrival.ready {
        return fault("never reached a stop ready to pull in");
    }
    if arrival.speed_mph > DOCKING_MAX_MPH {
        return fault("still rolling at the end of the run");
    }
    // 2. It stopped AT the gate. Crossing the arrival point over the gate's
    //    own posted number is the Spokane report -- "it did not automatically
    //    stop at the destination; I had to stop" -- and running a city block
    //    past it is the same complaint from the other side.
    if let Some(speed) = arrival.speed_at_point_mph {
        if speed > FACILITY_GATE_LIMIT_MPH {
            return fault("crossed its own gate over the gate's posted number");
        }
    }
    if arrival.past_the_point_ft > TRUCK_LENGTH_FT {
        return fault("stopped more than its own length past the gate");
    }
    // 3. Nothing untrue was said about where the truck was.
    for lie in [
        "Drove past",
        "You never stopped",
        "Missed the destination exit",
    ] {
        if arrival.said(lie) {
            return fault("was told it had blown the arrival it made");
        }
    }
    // 4. The assist named itself when it took the pedals. A truck that slows
    //    and halts in silence is, to a blind driver, indistinguishable from an
    //    assist that is not working -- which is how this was reported three
    //    times before anyone measured it.
    if !arrival.assist_spoke {
        return fault("took the pedals without saying so");
    }
    // 5. And it ended on an instruction the driver can act on: either the dock
    //    menu opened by itself, or the assist is holding and said which key
    //    opens it.
    if !arrival.docked && !arrival.said("Holding at the entrance. Press Enter to continue.") {
        return fault("stopped without telling the driver how to pull in");
    }
    if destination.chain {
        // A chain facility's streets are the way in. Stopping at the bottom of
        // the ramp is a stop up to a mile short of the gate, and being told
        // "you are at" the facility there is untrue by that same mile.
        if !arrival.on_chain {
            return fault("never drove the facility's own streets");
        }
        if !arrival.said("to the facility gate.") {
            return fault("was handed city streets without being told");
        }
        if arrival.said("Come to a stop.") && !arrival.docked {
            return fault("was told it had arrived while the gate was still a mile of streets on");
        }
    }
    None
}

#[test]
#[cfg_attr(
    ci_quick,
    ignore = "sweep: hands-off arrivals at every kind of destination on the map -- left to the nightly by --cfg ci_quick"
)]
fn test_the_approach_assist_stops_the_truck_at_every_kind_of_destination() {
    let world = get_world();
    let (chain, plain) = destinations(world, PER_KIND);
    assert_eq!(
        (chain.len(), plain.len()),
        (PER_KIND, PER_KIND),
        "the shipped world no longer offers {PER_KIND} of each kind of destination"
    );
    let mut failures: Vec<String> = Vec::new();
    let mut climbed = 0;
    for destination in chain.iter().chain(plain.iter()) {
        let arrival = arrive(destination);
        if arrival.gate_grade_pct > 0.01 {
            climbed += 1;
        }
        if let Some(fault) = what_went_wrong(destination, &arrival) {
            failures.push(fault);
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} destinations did not stop the truck ready to pull in:\n\n{}",
        failures.len(),
        chain.len() + plain.len(),
        failures.join("\n\n")
    );
    // The gate at the top of a climb is the case a brake-only stop profile
    // gets wrong in the other direction -- the road sheds the speed for free
    // and the truck halts short, with the dock never opening. The shipped
    // world supplies real ones; if it ever stops doing so this sweep has
    // quietly lost that half of the coverage.
    assert!(
        climbed > 0,
        "no destination in the sweep climbs to its gate any more"
    );
}

#[test]
fn test_hands_off_street_approach_takes_every_corner_and_reaches_the_gate() {
    // A tester on All assists, Elkhart Manufacturing Plant, 2026-10-14:
    // "the truck refused, point blank, to finish the route into the yard".
    // Facility stopping assistance took the pedals at 48 mph with an 11 mph
    // left turn still ahead, so the truck sailed past the corner; once it was
    // back on the approach the latched arrival held a two-mile-an-hour walk
    // for the last quarter mile until the assist was switched off. Nobody
    // touches a pedal on the streets here: the keeper takes the corners, the
    // assist takes the final shed, and the truck has to get to the gate.
    let destination = Destination {
        city: "elkhart_in_us".to_string(),
        location: "Elkhart Manufacturing Plant".to_string(),
        state: "IN".to_string(),
        chain: true,
    };
    let arrival = arrive_hands_off(&destination, None);
    if std::env::var("FF_PROBE").is_ok() {
        for line in &arrival.heard {
            eprintln!("{line}");
        }
        eprintln!("{arrival:?}");
    }
    assert!(
        arrival.on_chain,
        "Elkhart's plant is reached through its streets"
    );
    let missed: Vec<&String> = arrival
        .heard
        .iter()
        .filter(|line| line.contains("Missed the turn"))
        .collect();
    assert!(
        missed.is_empty(),
        "the assists missed a corner on the way in: {missed:?}"
    );
    assert!(
        arrival.ready,
        "never reached the gate hands off; stopped {:.0} ft short at {:.1} mph",
        arrival.short_by_ft, arrival.speed_mph
    );
    assert!(
        arrival.crawl_short_ft < 50.0,
        "crawled {:.0} ft at a walk with the gate still more than 250 ft off",
        arrival.crawl_short_ft
    );
    // Braked onto each corner's number, not under it and back up again.
    // A corner the truck was already under as its last 0.1 mi began (built
    // back up from the corner before) is measured from where it was.
    for (target, slowest, entry) in &arrival.corner_mins {
        let floor = if entry.is_nan() {
            *target
        } else {
            target.min(*entry)
        };
        assert!(
            *slowest >= floor - 2.0,
            "dipped to {slowest:.1} before a {target:.1} corner: {:?}",
            arrival.corner_mins
        );
    }
    // And each corner at its own number, not a held one from the last.
    for (target, taken) in &arrival.corner_passes {
        assert!(
            (taken - target).abs() <= 1.0 || *taken <= *target && *target <= 6.5,
            "took a {target:.1} corner at {taken:.1}: {:?}",
            arrival.corner_passes
        );
    }
}

#[test]
fn test_the_arrival_leaves_the_last_corner_to_the_keeper() {
    // The other half of the Elkhart report: the keeper holding the street's
    // 45 into the last corner. The arrival's curve to the gate was already
    // under that speed, so it latched there -- with the corner still ahead --
    // and its gentle shed to the gate could never make the corner's speed.
    let destination = Destination {
        city: "elkhart_in_us".to_string(),
        location: "Elkhart Manufacturing Plant".to_string(),
        state: "IN".to_string(),
        chain: true,
    };
    let arrival = arrive_hands_off(&destination, Some(48.0));
    if std::env::var("FF_PROBE").is_ok() {
        for line in &arrival.heard {
            eprintln!("{line}");
        }
        eprintln!(
            "corners (target, taken) mph: {:?}\nslowest in the last 0.1 mi: {:?}\nready={} latched_with_corner_ahead={} crawl_short_ft={:.0} speed_at_point={:?}",
            arrival.corner_passes,
            arrival.corner_mins,
            arrival.ready,
            arrival.latched_with_corner_ahead,
            arrival.crawl_short_ft,
            arrival.speed_at_point_mph
        );
    }
    assert!(
        arrival.street_kicked,
        "never reached the last corner with the gate {STREET_KICK_MI} mi off"
    );
    assert!(
        !arrival.latched_with_corner_ahead,
        "facility stopping assistance took the pedals with a corner still ahead"
    );
    // And the turn is made as routed, the first time: no miss, no loop
    // around the block (owner, 2026-10-14).
    let detour: Vec<&String> = arrival
        .heard
        .iter()
        .filter(|line| line.contains("Missed the turn") || line.contains("around the block"))
        .collect();
    assert!(
        detour.is_empty(),
        "the assists did not make the last turn: {detour:?}"
    );
    let yard_lines = arrival
        .heard
        .iter()
        .filter(|line| line.contains("Into the yard"))
        .count();
    assert_eq!(
        yard_lines, 1,
        "the yard is announced once, at the real entry"
    );
    let alarms: Vec<&String> = arrival
        .heard
        .iter()
        .filter(|line| line.contains("can't hold"))
        .collect();
    assert!(
        alarms.is_empty(),
        "a corner the keeper made raised an alarm: {alarms:?}"
    );
    let (target, taken) = *arrival.corner_passes.last().expect("a corner was taken");
    assert!(
        taken <= target + 1.0,
        "took the last corner at {taken:.1} mph against its {target:.1}"
    );
    assert!(
        arrival.ready,
        "never reached the gate hands off; stopped {:.0} ft short at {:.1} mph",
        arrival.short_by_ft, arrival.speed_mph
    );
    assert!(
        arrival.crawl_short_ft < 50.0,
        "crawled {:.0} ft at a walk with the gate still more than 250 ft off",
        arrival.crawl_short_ft
    );
}

#[test]
fn test_an_overrun_driveway_announces_the_yard_once() {
    // QA on the Elkhart fix: overrunning the driveway turn said "Into the
    // yard. Yard limit 15." as the truck rolled over the driveway's milepost,
    // then "Missed the turn", then the yard line again after the loop. The
    // yard is announced once, when the truck has really turned in.
    let destination = Destination {
        city: "elkhart_in_us".to_string(),
        location: "Elkhart Manufacturing Plant".to_string(),
        state: "IN".to_string(),
        chain: true,
    };
    let arrival = arrive_coasting_past(&destination, 30.0);
    if std::env::var("FF_PROBE").is_ok() {
        eprintln!("corners {:?}", arrival.corner_passes);
        for line in &arrival.heard {
            eprintln!("{line}");
        }
    }
    assert!(arrival.street_kicked);
    let missed = arrival
        .heard
        .iter()
        .position(|l| l.contains("Missed the turn"));
    assert!(
        missed.is_some(),
        "coasting at 30 past a 6 mph driveway must miss it"
    );
    let yard: Vec<usize> = arrival
        .heard
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains("Into the yard"))
        .map(|(i, _)| i)
        .collect();
    assert!(
        yard.len() <= 1,
        "the yard was announced {} times",
        yard.len()
    );
    assert!(
        yard.iter().all(|i| Some(*i) > missed),
        "the yard was announced before the missed turn"
    );
}

#[test]
fn test_great_falls_signal_stop_does_not_become_a_two_mph_destination_crawl() {
    // Shane P, 2026-08-26: on the logged Eugene-to-Great-Falls bulk run, the
    // destination assist announced before the ramp-end signal, the
    // route-transition assist stopped the truck at red, and the destination
    // assist then held two mph after green until he disabled it. Recreate the
    // same route, facility, load, carrier tractor, automatic transmission and
    // clear Montana weather. The player lifts for each assist and never
    // touches a pedal again: since the owner's 2026-09-01 ruling the green
    // releases the truck to facility stopping assistance, which drives it
    // from the bar to the entrance itself.
    let route_cities = [
        "eugene_or_us",
        "tri_cities_wa_us",
        "spokane_wa_us",
        "coeur_d_alene_id_us",
        "kellogg_id_us",
        "superior_mt_us",
        "missoula_mt_us",
        "helena_mt_us",
        "great_falls_mt_us",
    ];
    let mut setup = RouteSetup::seeded(4242)
        .named("munchkinbear")
        .cities(&route_cities)
        .origin_location("Eugene Grain Elevator")
        .destination_location("Great Falls Materials Yard");
    setup.cargo = "bulk".to_string();
    setup.tons = 15.0;

    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.destination_approach_assist = true;
    harness.app.ctx.settings.route_transition_assist = true;
    harness.app.ctx.settings.speed_keeper = true;
    harness.app.ctx.settings.automatic_transmission = true;
    harness.start_route("eugene_or_us", "great_falls_mt_us", setup);

    let mut truck_key = String::new();
    harness.with_drive(|d, ctx| {
        let mut profile = Profile::named_in("munchkinbear", "eugene_or_us");
        profile.career = Career::with_xp(LEVEL_XP[10]);
        truck_key = profile.take_slip_seat(&d.job);
        d.trip.truck.specs = profile.truck_specs();
        ctx.profile = Some(profile);
        d.job.origin_type = "farm_elevator".to_string();
        d.job.destination_type = "construction_materials_yard".to_string();
        quiet(&mut d.trip);
        d.weather_mut().current = WeatherKind::Clear;
        d.departure_checked = true;
        d.tutorial = None;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        // IN GEAR, like a truck already doing highway speed. This used to be
        // left in neutral and got away with it: speed control put throttle
        // down and the automatic engaged from there. Route-transition
        // assistance now pauses speed control when it takes the pedals, so
        // nothing asked for a gear and the truck coasted the whole approach --
        // and the whole gate crawl -- out of gear.
        d.truck_mut().transmission.gear = 9;
        d.truck_mut().rpm = 1500.0;
        d.truck_mut().set_air_ready(false);
        d.speed_control_armed = true;
    });
    assert_eq!(trailer_keys_for_cargo("bulk"), ["bulk"]);

    let exit = harness.with_drive(|d, ctx| {
        d.destination_exit_stop(ctx)
            .expect("the Great Falls delivery has a destination exit")
    });
    let exit_at = exit.at_mi;
    harness.with_drive(move |d, ctx| {
        d.exit_stop = Some(exit);
        d.exit_lane_entered = true;
        d.exit_signal_on = true;
        d.trip.position_mi = exit_at;
        d.truck_mut().velocity_mps = 40.0 * MPS_PER_MPH;
        d.update_exit(ctx, 0.0, 0.0);

        // Shane met a green about 1000 feet from the bar, then yellow and red.
        // Pin that same sequence and the same 39 mph approach from his log.
        d.ramp_mi = Some(RAMP_ACCESS_MI + 850.0 / 5280.0);
        d.ramp_control = "signal".to_string();
        d.ramp_terminal_done = false;
        d.ramp_light_announced = true;
        d.ramp_light_last_phase = "green".to_string();
        d.ramp_light_offset_s = d.ramp_light_red_s() + d.ramp_light_green_s() - 2.0;
        d.ramp_light_timer = 0.0;
        d.ramp_waiting_at_light = false;
        d.ramp_assist_said = false;
        d.ramp_assist_brake = 0.0;
        d.truck_mut().velocity_mps = 39.0 * MPS_PER_MPH;
    });
    harness.clear_speech();

    let mut terminal_done = false;
    let mut docked = false;
    let mut max_speed_after_green = 0.0f64;
    let mut two_mph_crawl_ft = 0.0f64;
    let mut previous_ramp_mi = harness.read_drive(|d| d.ramp_mi.unwrap_or(0.0));
    for _ in 0..(60 * 300) {
        if !harness.has_drive() {
            harness.finish_timed_state();
            docked = harness.state_is::<FacilityArrivalState>();
            break;
        }
        if harness.read_drive(|d| d.arrival_menu_open) {
            harness.finish_timed_state();
            docked = harness.state_is::<FacilityArrivalState>();
            break;
        }
        let (done, waiting, speed) = harness.read_drive(|d| {
            (
                d.ramp_terminal_done,
                d.ramp_waiting_at_light,
                d.truck().speed_mph(),
            )
        });
        // Hold the logged red until the truck has made its complete stop,
        // then let the next frame deliver green. The report preserves the
        // phases and the stop, not the light's random offset.
        harness.with_drive(|d, _| {
            if !done && !waiting && d.ramp_light_phase() == "red" {
                d.ramp_light_offset_s = 1.0;
                d.ramp_light_timer = 0.0;
            } else if waiting {
                d.ramp_light_offset_s = d.ramp_light_red_s();
                d.ramp_light_timer = 0.0;
                d.ramp_light_last_phase = "red".to_string();
            }
        });
        terminal_done |= done;
        if terminal_done {
            max_speed_after_green = max_speed_after_green.max(speed);
        }
        release_keys(&mut harness); // hands off: the assist pulls ahead
        frame(&mut harness, DT);
        if !harness.has_drive() {
            continue;
        }
        let (now_ramp_mi, now_speed) =
            harness.read_drive(|d| (d.ramp_mi.unwrap_or(0.0), d.truck().speed_mph()));
        if terminal_done && (1.5..=2.5).contains(&now_speed) {
            two_mph_crawl_ft += (previous_ramp_mi - now_ramp_mi).max(0.0) * 5280.0;
        }
        previous_ramp_mi = now_ramp_mi;
        if waiting {
            release_keys(&mut harness);
        }
    }
    release_keys(&mut harness);

    println!(
        "truck={truck_key} max_after_green={max_speed_after_green:.1} mph two_mph_crawl={two_mph_crawl_ft:.0} ft\n{}",
        harness.transcript_text()
    );
    assert_eq!(truck_key, "long_run_midroof");
    assert!(docked, "{}", harness.transcript_text());
    assert!(
        max_speed_after_green >= 10.0,
        "the signal stop still held the truck at a crawl: {}",
        harness.transcript_text()
    );
    assert!(
        two_mph_crawl_ft < 250.0,
        "the final two-mph creep lasted {two_mph_crawl_ft:.0} feet"
    );
    harness.result().assert_ordered(&[
        "Braking for the light.",
        "Red light.",
        "Light green.",
        "Pulling into construction materials yard Great Falls Materials Yard",
        "At construction materials yard Great Falls Materials Yard in Great Falls.",
    ]);
    // The green release activates the handoff with only the light color;
    // the latch a frame later must not add another announcement.
    let heard = harness.transcript();
    assert!(
        !heard
            .iter()
            .any(|line| line.contains("Pull ahead ") || line.contains("taking the pedals")),
        "{}",
        harness.transcript_text()
    );
}

// -- from the ramp-end sign to the entrance, hands off ----------------------------------

/// What a drive from a ramp-end stop sign did once the sign was clear.
struct SignRelease {
    heard: Vec<String>,
    /// The truck reached the sign and stopped there with the terminal done.
    stopped_at_sign: bool,
    /// After the stop it rolled off past walking pace with no throttle held.
    moved_off_alone: bool,
    /// It reached the facility's own streets.
    on_chain: bool,
    /// The assist stopped it at the gate with the hold prompt spoken.
    held_at_entrance: bool,
    /// Stood still for ten seconds after the release, or after the brake.
    stood_still: bool,
    speed_mph: f64,
    /// The fastest the truck went between moving off from the sign and
    /// reaching the streets: the ramp roll runs up to the posted limit, not
    /// a walk (owner, 2026-09-01).
    ramp_top_mph: f64,
    /// The lane's own number from the sign to the entrance: the posted
    /// limit, never above the ramp's advisory speed. What a plain ramp's
    /// hands-off drive runs up to, floored at the facility-lane roll.
    lane_mph: f64,
}

impl SignRelease {
    fn said(&self, needle: &str) -> bool {
        self.heard.iter().any(|line| line.contains(needle))
    }

    fn report(&self, destination: &Destination) -> String {
        format!(
            "{} ({}, {}): stopped_at_sign={} moved_off_alone={} on_chain={} held_at_entrance={} \
             stood_still={} speed={:.2}\nheard: {:#?}",
            destination.location,
            destination.city,
            destination.kind(),
            self.stopped_at_sign,
            self.moved_off_alone,
            self.on_chain,
            self.held_at_entrance,
            self.stood_still,
            self.speed_mph,
            self.heard
        )
    }
}

/// Down `destination`'s ramp to a stop sign at its end, with route-transition
/// assistance making the stop and the crossroad empty, so the clear comes
/// with the stop. Nobody ever holds the throttle; `brake_after_clear` holds
/// the brake once the truck has moved off, the way a driver cancels an
/// assist. Automatic speed control was never switched on by the driver.
fn arrive_from_the_sign(
    destination: &Destination,
    approach_assist: bool,
    brake_after_clear: bool,
) -> SignRelease {
    let world = get_world();
    let origin = world.neighbors(&destination.city)[0]
        .other(&destination.city)
        .to_string();
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.destination_approach_assist = approach_assist;
    harness.app.ctx.settings.route_transition_assist = true;
    harness.app.ctx.settings.speed_keeper = true;
    harness.app.ctx.settings.automatic_transmission = true;
    // "Hands off" here means no PEDAL and no wheel input from the test; lane
    // keeping is what holds the wheel, and since the lane model grew a heading
    // it has to be on for a truck nobody steers to stay on the road.
    harness.app.ctx.settings.lane_keeping = "full".to_string();
    let mut route_setup = RouteSetup::seeded(4242)
        .named("Sign Release")
        .destination_location(&destination.location);
    route_setup.tons = 18.0;
    harness.start_route(&origin, &destination.city, route_setup);
    harness.with_drive(|d, ctx| {
        quiet(&mut d.trip);
        d.weather_mut().current = WeatherKind::Clear;
        d.departure_checked = true;
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 9;
        d.truck_mut().rpm = 1500.0;
        d.truck_mut().set_air_ready(false);
    });
    let exit = harness.with_drive(|d, ctx| {
        d.destination_exit_stop(ctx)
            .expect("a delivery always has a destination exit")
    });
    let at = exit.at_mi;
    harness.with_drive(move |d, ctx| {
        d.exit_stop = Some(exit);
        d.exit_lane_entered = true;
        d.exit_signal_on = true;
        d.trip.position_mi = at;
        d.truck_mut().velocity_mps = 40.0 * MPS_PER_MPH;
        d.update_exit(ctx, 0.0, 0.0);
    });
    assert!(
        harness.read_drive(|d| d.ramp_mi.is_some()),
        "{}: never got onto the destination ramp",
        destination.city
    );
    harness.with_drive(|d, _| {
        // A stop sign, announced, with an empty crossroad: the stop is the
        // ramp assist's and the clear call lands with it.
        d.ramp_control = "stop".to_string();
        d.cross_bubble = None;
        d.ramp_light_announced = true;
        d.ramp_terminal_done = false;
        d.ramp_waiting_at_sign = false;
        d.ramp_assist_said = false;
        d.ramp_assist_brake = 0.0;
        d.ramp_mi = Some(RAMP_ACCESS_MI + 0.1);
        d.truck_mut().velocity_mps = 25.0 * MPS_PER_MPH;
    });
    let lane_mph = harness.with_drive(|d, _| {
        let (posted_mph, _) = d.trip.speed_limit_at(d.trip.position_mi);
        posted_mph.min(d.armed_ramp_mph(None))
    });
    harness.clear_speech();
    release_keys(&mut harness);

    let mut stopped_at_sign = false;
    let mut moved_off_alone = false;
    let mut on_chain = false;
    let mut held_at_entrance = false;
    let mut stood_still = false;
    let mut braked = false;
    let mut brake_frames = 0;
    let mut still_frames = 0;
    let mut ramp_top_mph: f64 = 0.0;
    for _ in 0..(60 * 600) {
        if !harness.has_drive() || harness.read_drive(|d| d.arrival_menu_open) {
            break;
        }
        let (done, speed, chain, full_stop_said) = harness.read_drive(|d| {
            (
                d.ramp_terminal_done,
                d.truck().speed_mph(),
                d.surface_chain,
                d.arrival_full_stop_said,
            )
        });
        on_chain |= chain;
        if done && speed <= RED_STOP_MPH && !stopped_at_sign {
            stopped_at_sign = true;
        }
        if stopped_at_sign && speed > 5.0 {
            moved_off_alone = true;
        }
        if stopped_at_sign && !chain {
            ramp_top_mph = ramp_top_mph.max(speed);
        }
        if full_stop_said && speed <= DOCKING_MAX_MPH {
            held_at_entrance = true;
            break;
        }
        if brake_after_clear && moved_off_alone && !braked {
            // The driver's foot on the brake, held until the truck is stopped
            // and a moment more, then lifted.
            hold(&mut harness, &[Key::Down]);
            brake_frames += 1;
            if speed <= DOCKING_MAX_MPH && brake_frames > 60 {
                braked = true;
                release_keys(&mut harness);
            }
        }
        // Standing after the release (assist off), or after the brake: ten
        // seconds with nobody touching anything.
        if stopped_at_sign && (!approach_assist || braked) && speed <= RED_STOP_MPH {
            still_frames += 1;
            if still_frames >= 60 * 10 {
                stood_still = true;
                break;
            }
        }
        frame(&mut harness, DT);
    }
    release_keys(&mut harness);
    let speed_mph = if harness.has_drive() {
        harness.read_drive(|d| d.truck().speed_mph())
    } else {
        0.0
    };
    SignRelease {
        heard: harness.transcript(),
        stopped_at_sign,
        moved_off_alone,
        on_chain,
        held_at_entrance,
        stood_still,
        speed_mph,
        ramp_top_mph,
        lane_mph,
    }
}

#[test]
fn test_the_assist_drives_from_the_clear_sign_to_the_entrance_hold_hands_off() {
    // Owner ruling, 2026-09-01, after an agent drive from Chicago to Gary:
    // "the approach assist should go from signal to approach entrance, hands
    // off." With the assist on, the clear at the ramp-end sign is the assist's
    // to act on: the truck moves off alone, takes the facility's streets on the
    // speed keeper, and ends held at the entrance waiting for Enter -- with no
    // key pressed after the ramp was taken.
    let (chain, _) = destinations(get_world(), 1);
    let destination = &chain[0];
    let release = arrive_from_the_sign(destination, true, false);
    println!("{}", release.report(destination));
    assert!(release.stopped_at_sign, "{}", release.report(destination));
    assert!(
        release.said("Stopped at the sign. Clear."),
        "{}",
        release.report(destination)
    );
    assert!(
        !release.said("pull ahead to the entrance"),
        "{}",
        release.report(destination)
    );
    assert!(release.moved_off_alone, "{}", release.report(destination));
    // Road, not a gate: the roll from the bar to the streets runs up past
    // the old 12 mph facility-lane walk toward the ramp's posted limit.
    assert!(
        release.ramp_top_mph > 17.0,
        "ramp top {:.1} mph: {}",
        release.ramp_top_mph,
        release.report(destination)
    );
    assert!(release.on_chain, "{}", release.report(destination));
    // The keeper takes the street limit in silence: "Speed limit 25" is
    // the line the driver needs (owner, 2026-10-25).
    assert!(
        !release.said("Speed keeper holding"),
        "{}",
        release.report(destination)
    );
    assert!(release.held_at_entrance, "{}", release.report(destination));
    assert!(
        release.said("Holding at the entrance. Press Enter to continue."),
        "{}",
        release.report(destination)
    );
    for lie in ["Drove past", "You never stopped", "missed"] {
        assert!(!release.said(lie), "{}", release.report(destination));
    }
}

#[test]
fn test_the_assist_drives_a_plain_ramp_from_the_sign_at_the_lane_speed() {
    // The same hands-off release on a facility whose ramp ends at the gate.
    // The stretch from the sign to the entrance is road until the final
    // lengths, so the truck runs up to the lane's own number -- or to the
    // facility-lane roll where the lane's number is under it -- rather than
    // walking whatever the lane allows at 12 (owner, 2026-09-03: "crawling
    // at 12"), then sheds to the gate creep and the dock opens.
    let (_, plain) = destinations(get_world(), 1);
    let destination = &plain[0];
    let release = arrive_from_the_sign(destination, true, false);
    println!("{}", release.report(destination));
    assert!(release.stopped_at_sign, "{}", release.report(destination));
    assert!(release.moved_off_alone, "{}", release.report(destination));
    // From the bar the stop profile is already under the lane's number and
    // falling, so the truck meets it on the way up rather than reaching the
    // number itself; what matters is that it is driven well past the walk.
    assert!(
        release.ramp_top_mph > FACILITY_LANE_ROLL_MPH + 5.0,
        "ramp top {:.1} mph against a lane number of {:.1}: {}",
        release.ramp_top_mph,
        release.lane_mph,
        release.report(destination)
    );
    // A plain ramp ends at the gate: the dock opens, there is no entrance
    // hold to press Enter at.
    assert!(
        release.said("Pulling into"),
        "{}",
        release.report(destination)
    );
    for lie in ["Drove past", "You never stopped", "missed"] {
        assert!(!release.said(lie), "{}", release.report(destination));
    }
}

#[test]
fn test_with_the_assist_off_the_clear_sign_still_hands_the_last_stretch_to_the_driver() {
    let (chain, _) = destinations(get_world(), 1);
    let destination = &chain[0];
    let release = arrive_from_the_sign(destination, false, false);
    assert!(release.stopped_at_sign, "{}", release.report(destination));
    assert!(
        release.said("Stopped at the sign. Clear; pull ahead onto the streets."),
        "{}",
        release.report(destination)
    );
    assert!(
        !release.said("Facility stopping assistance"),
        "{}",
        release.report(destination)
    );
    // Nobody pulled ahead, so the truck is still at the bar.
    assert!(release.stood_still, "{}", release.report(destination));
    assert!(!release.moved_off_alone, "{}", release.report(destination));
    assert!(!release.on_chain, "{}", release.report(destination));
}

#[test]
fn test_the_drivers_brake_cancels_the_automatic_pull_ahead() {
    // The rule every assist follows: the driver's own brake hands the pedals
    // back. On both facility shapes -- the roll to the streets and the plain
    // ramp's drive to the gate -- a brake after the truck has moved off stops
    // it, says so, and leaves it stopped rather than creeping off again.
    let (chain, plain) = destinations(get_world(), 1);
    for destination in [&chain[0], &plain[0]] {
        let release = arrive_from_the_sign(destination, true, true);
        println!("{}", release.report(destination));
        assert!(release.moved_off_alone, "{}", release.report(destination));
        assert!(
            release.said(if destination.chain {
                "Pull ahead onto the streets."
            } else {
                "Pull ahead to the entrance."
            }),
            "{}",
            release.report(destination)
        );
        assert!(release.stood_still, "{}", release.report(destination));
        assert!(!release.on_chain, "{}", release.report(destination));
        assert!(!release.held_at_entrance, "{}", release.report(destination));
        assert!(
            release.speed_mph <= RED_STOP_MPH,
            "{}",
            release.report(destination)
        );
    }
}

#[test]
fn test_shelby_cross_dock_approach_assist_reaches_the_arrival_gate() {
    // Darren, 2026-08-25: approaching Shelby Cross-Dock, the assist took the
    // truck from 30 to 14 to 5 to 2 mph while route status still said the
    // facility was a mile away, then never opened the dock. Switching the
    // assist off let the arrival fire eight seconds later. Drive the shipped
    // facility and require the player-visible outcome, not merely a slow
    // truck: hands off once the assist speaks, the dock opens at a crawl.
    let destination = Destination {
        city: "shelby_mt_us".to_string(),
        location: "Shelby Cross-Dock".to_string(),
        state: "MT".to_string(),
        chain: false,
    };
    // The report did not preserve Darren's tractor model or career level. Use
    // the real company-fleet assignment function at every tier boundary under
    // his name, so this covers each class of iron he could have been assigned
    // rather than quietly substituting the harness's default truck. Level one
    // is the standard-rig control; the other four are the contrast cases.
    let levels = [1_i64, 4, 9, 13, 17];
    let mut arrivals = Vec::new();
    for level in levels {
        let profile = darren_company_profile(level);
        arrivals.push((level, arrive_with(&destination, None, Some(profile), 20.0)));
    }

    assert_eq!(
        trailer_keys_for_cargo("general"),
        ["dry_van"],
        "general freight must continue to exercise the dry-van mass model"
    );
    assert_eq!(arrivals[0].1.truck_key, "rig", "level-one control drifted");

    for (level, arrival) in &arrivals {
        println!("level {level}: {}", arrival.report(&destination));
        assert_eq!(
            what_went_wrong(&destination, arrival),
            None,
            "level {level}: {}",
            arrival.report(&destination)
        );
        assert!(
            arrival.docked && !arrival.stalled,
            "level {level}: {}",
            arrival.report(&destination)
        );
        assert!(
            arrival
                .min_creep_speed_mph
                .is_some_and(|speed| (1.9..=2.1).contains(&speed))
                && arrival.min_creep_gear.is_some()
                && arrival.max_creep_hold_throttle <= 0.35,
            "level {level}: gate crawl exceeded its throttle authority: {}",
            arrival.report(&destination)
        );
        assert!(
            arrival.said("Slowing for the entrance."),
            "level {level}: {}",
            arrival.report(&destination)
        );
        assert!(
            arrival.said("Pulling into freight terminal Shelby Cross-Dock"),
            "level {level}: {}",
            arrival.report(&destination)
        );
        assert!(
            !arrival.said("Holding at the entrance."),
            "level {level}: {}",
            arrival.report(&destination)
        );
        assert!(
            !arrival.said("shelby_mt_us")
                && arrival.said(
                    "At freight terminal Shelby Cross-Dock in Shelby, Montana. Drop the loaded \
                     trailer and hook an empty. 1 of 4."
                ),
            "level {level}: dock speech was not player-ready and actionable: {}",
            arrival.report(&destination)
        );
    }

    let min_torque = arrivals
        .iter()
        .map(|(_, arrival)| arrival.max_torque_nm)
        .fold(f64::INFINITY, f64::min);
    let max_torque = arrivals
        .iter()
        .map(|(_, arrival)| arrival.max_torque_nm)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        max_torque - min_torque >= 400.0,
        "fleet-tier sweep did not span meaningfully different power profiles: {arrivals:#?}"
    );
    let brake_span = arrivals
        .iter()
        .map(|(_, arrival)| arrival.full_service_decel_mps2)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(low, high), brake| {
            (low.min(brake), high.max(brake))
        });
    assert!(
        brake_span.1 - brake_span.0 < 0.001,
        "cold dry service-brake capacity unexpectedly varies by tractor: {arrivals:#?}"
    );
    assert!(
        arrivals
            .iter()
            .all(|(_, arrival)| arrival.min_creep_gear == Some(1)),
        "the automatic did not reach its launch gear before the crawl: {arrivals:#?}"
    );
}

/// Darren's exact level was not logged. Resolve his deterministic company
/// assignment at one level in each shipped fleet tier, including the real
/// heavy-load slip-seat choice for the regional tier.
fn darren_company_profile(level: i64) -> Profile {
    let world = get_world();
    let route = world
        .supported_route("helena_mt_us", "shelby_mt_us", None)
        .expect("Helena to Shelby route lookup must succeed")
        .expect("Helena to Shelby must remain a supported run");
    let mut job = Job::new(
        cargo_type("general").expect("general freight remains in the catalog"),
        20.0,
        "helena_mt_us",
        "Helena Terminal",
        "shelby_mt_us",
        route.miles().round(),
        500.0,
        2.0,
    );
    job.destination_location = "Shelby Cross-Dock".to_string();

    let mut profile = Profile::named_in("Darren", "helena_mt_us");
    profile.career = Career::with_xp(LEVEL_XP[(level - 1) as usize]);
    let expected = assigned_truck_key(&profile, Some(&job));
    profile.take_slip_seat(&job);
    assert_eq!(profile.active_truck_key(), expected);
    let tier = FLEET_TIERS
        .iter()
        .rev()
        .find(|tier| level >= tier.min_level)
        .expect("level one has a carrier tier");
    assert!(tier.pool.contains(&expected));
    profile
}

#[test]
#[ignore = "sweep probe: the same drives, printed rather than asserted"]
fn sweep_probe() {
    let world = get_world();
    let (chain, plain) = destinations(world, PER_KIND);
    let mut bad = 0;
    for d in chain.iter().chain(plain.iter()) {
        let arrival = arrive(d);
        let ok = arrival.ready && arrival.speed_mph <= DOCKING_MAX_MPH && arrival.assist_spoke;
        if !ok {
            bad += 1;
        }
        println!(
            "{:<6} {:<26} {:<44} chain={} onchain={} spoke={} v={:.2} short={:.0}ft              at_point={:?} past={:.0}ft",
            if ok { "ok" } else { "FAIL" },
            d.city,
            d.location,
            d.chain as u8,
            arrival.on_chain as u8,
            arrival.assist_spoke as u8,
            arrival.speed_mph,
            arrival.short_by_ft,
            arrival.speed_at_point_mph.map(|v| (v * 10.0).round() / 10.0),
            arrival.past_the_point_ft,
        );
        if !ok {
            for line in arrival.heard.iter().rev().take(10) {
                println!("      | {line}");
            }
        }
    }
    println!("failures: {bad}");
}
