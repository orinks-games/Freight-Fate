//! Every assist, every kind of exit: the realistic exit rules hold whoever
//! is driving (owner requirement, 2026-09-24, PR #217).
//!
//! Each run starts about two miles before the gore at road speed with the
//! exit signalled, and drives to the stop at the end of the ramp, or through
//! the street chain to the facility gate. What every run owes:
//!
//! 1. Never more than about ten under road speed on the mainline.
//! 2. At or under the exit speed where the ramp curve begins.
//! 3. A clean terminal: nothing run, nothing crossed "far too fast".
//! 4. An arrival: no missed exit, no strand, no crawl far from the stop.
//!    On the streets, the light and the stop sign are stops too: nothing
//!    run there either, and the assists drive on from them.
//! 5. No line naming an assist that is off, or a key this lane keeping
//!    does not give the driver.
//!
//! The driver in these runs is a competent human who brakes LATE: only when
//! the road left asks for more than a firm stop, so an assist that is on
//! gets the whole of its job and the driver is the backstop. With every
//! assist on the driver must never have to brake at all.
//!
//! Lane keeping off is not in the matrix: nothing here steers, and a truck
//! nobody steers leaves the road, which measures the ghost and not the
//! assist. Full and partial are; the partial driver follows the engine's
//! lean through the ramp curve, which is the cue partial lane keeping owes.
//!
//! Diagnosis: `MATRIX_DUMP=1` prints every run's transcript, and
//! `MATRIX_TRACE=Yield/All` (a `Kind/Preset` pair) prints the pedals every
//! third of a second down that run's ramp. Both need `--nocapture`.

use ff_core::data::world::get_world;
use ff_core::data::world_models::{
    CorridorDetail, Driveway, ExitChain, GradeSegment, Interchange, Leg, LocalGeometrySegment,
    Route, SpeedLimitSample, Stop, StreetControl, StreetLimit,
};
use ff_core::settings::Settings;
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::{WeatherKind, WeatherSystem};
use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup, StartDelivery};
use freight_fate::states::base::{Key, Mods};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::{
    DOCKING_MAX_MPH, EXIT_MAINLINE_EASE_MPH, GREEN_ROLL_MPH, RAMP_ACCESS_MI, RED_STOP_MPH,
    YIELD_ROLL_MPH,
};
use freight_fate::states::driving_menu_states::FacilityArrivalState;
use freight_fate::states::driving_rest_states::RestStopState;

use crate::states_driving_approach_sweep::{destinations, driver_target_mph};
use crate::transcript_cruise_support::{quiet, MPS_PER_MPH};

pub(crate) const DT: f64 = 1.0 / 30.0;
const MPH_PER_MPS: f64 = 2.23694;
pub(crate) const STOP_MI: f64 = 40.0;
const STOP_TERMINAL_NODE: i64 = 4242;
const ROAD_MPH: f64 = 70.0;
/// The driver brakes only past this: a firm, late stop.
const DRIVER_LATE_MPS2: f64 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Preset {
    None,
    Cruise,
    ExitSpeed,
    Transition,
    Curve,
    Facility,
    Keeper,
    LanePartial,
    All,
}

const PRESETS: [Preset; 9] = [
    Preset::None,
    Preset::Cruise,
    Preset::ExitSpeed,
    Preset::Transition,
    Preset::Curve,
    Preset::Facility,
    Preset::Keeper,
    Preset::LanePartial,
    Preset::All,
];

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Kind {
    SignalRed,
    SignalGreen,
    StopSign,
    Yield,
    FreeFlow5x,
    ShortMeasured,
    DestinationChain,
    /// The destination's streets with a traffic light (red until the truck
    /// is stopped at it) and a stop sign on them, played through the ramp
    /// terminal's own rules.
    StreetChain,
}

const KINDS: [Kind; 8] = [
    Kind::SignalRed,
    Kind::SignalGreen,
    Kind::StopSign,
    Kind::Yield,
    Kind::FreeFlow5x,
    Kind::ShortMeasured,
    Kind::DestinationChain,
    Kind::StreetChain,
];

impl Preset {
    fn cruise(self) -> bool {
        matches!(self, Preset::Cruise | Preset::Keeper | Preset::All)
    }

    /// Whether an assist brakes the deceleration lane to the exit speed, so
    /// the driver never hears the ramp curve called too fast.
    fn owns_ramp_speed(self) -> bool {
        matches!(
            self,
            Preset::ExitSpeed | Preset::Transition | Preset::Curve | Preset::Facility | Preset::All
        )
    }

    fn transition(self) -> bool {
        matches!(self, Preset::Transition | Preset::All)
    }

    fn apply(self, s: &mut Settings) {
        s.automatic_emergency_braking = false;
        s.stop_and_go_assist = false;
        s.descent_speed_control = "off".to_string();
        s.exit_speed_assist = false;
        s.destination_approach_assist = false;
        s.curve_speed_assist = false;
        s.route_transition_assist = false;
        s.speed_keeper = false;
        s.lane_keeping = "full".to_string();
        match self {
            Preset::None | Preset::Cruise => {}
            Preset::ExitSpeed => s.exit_speed_assist = true,
            Preset::Transition => s.route_transition_assist = true,
            Preset::Curve => s.curve_speed_assist = true,
            Preset::Facility => s.destination_approach_assist = true,
            Preset::Keeper => s.speed_keeper = true,
            Preset::LanePartial => s.lane_keeping = "partial".to_string(),
            Preset::All => {
                s.apply_driving_assistance_preset("all");
                s.speed_keeper = true;
            }
        }
        s.automatic_transmission = true;
    }
}

impl Kind {
    fn control(self) -> &'static str {
        match self {
            Kind::SignalRed | Kind::SignalGreen => "signal",
            Kind::StopSign | Kind::ShortMeasured => "stop",
            Kind::Yield => "yield",
            Kind::FreeFlow5x | Kind::DestinationChain | Kind::StreetChain => "none",
        }
    }

    fn chain(self) -> bool {
        matches!(self, Kind::DestinationChain | Kind::StreetChain)
    }

    fn time_scale(self) -> f64 {
        if self == Kind::FreeFlow5x {
            5.0
        } else {
            1.0
        }
    }
}

// -- the roads ---------------------------------------------------------------------------

/// A straight 70 mph interstate with one exit at `STOP_MI`: its interchange
/// carries the terminal's control and, for the short kind, a measured ramp.
fn bench(d: &mut DrivingState, kind: Kind) {
    let city = d.trip.route.cities[0].clone();
    let miles = 400.0;
    let interchange = Interchange {
        at_mi: STOP_MI,
        exit_ref: "42".to_string(),
        destinations: vec!["Prairie".to_string()],
        highway: "I 90".to_string(),
        source: "test".to_string(),
        ramp_control: kind.control().to_string(),
        ramp_far_end: "surface".to_string(),
        // The shortest the bake reports in earnest: the run to the bar is
        // then only the Green Book stopping distance.
        ramp_length_ft_forward: (kind == Kind::ShortMeasured).then_some(300.0),
        // The free-flow exit ends on a crossroad the stop's own streets
        // leave from.
        ramp_terminal_node_forward: (kind == Kind::FreeFlow5x).then_some(STOP_TERMINAL_NODE),
        ramp_terminal_source: if kind == Kind::FreeFlow5x {
            "test bench".to_string()
        } else {
            String::new()
        },
        ..Default::default()
    };
    // Its truck stop lies down that crossroad: a 45 mph frontage road, then a
    // right onto the service road into the lot.
    let mut stop_record = Stop {
        name: "Prairie Travel Center".to_string(),
        at_mi: STOP_MI,
        exit_ref: "42".to_string(),
        interchange_mi: Some(STOP_MI),
        ..Default::default()
    };
    if kind == Kind::FreeFlow5x {
        stop_record.approach_chains = vec![ExitChain {
            terminal_node: STOP_TERMINAL_NODE,
            total_miles: 0.5,
            segments: vec![
                LocalGeometrySegment {
                    road: "Prairie Frontage Road".to_string(),
                    miles: 0.4,
                    cue: "Start on Prairie Frontage Road.".to_string(),
                    speed_mph: 45.0,
                    limit: Some(StreetLimit {
                        mph: 45.0,
                        source: "read".to_string(),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                LocalGeometrySegment {
                    road: "a service road".to_string(),
                    miles: 0.1,
                    cue: "Turn right onto a service road.".to_string(),
                    speed_mph: 15.0,
                    turn_deg: 90.0,
                    limit: Some(StreetLimit {
                        mph: 15.0,
                        source: "assumed".to_string(),
                        ..Default::default()
                    }),
                    ..Default::default()
                },
            ],
            driveway: Some(Driveway {
                at_mi: 0.4,
                kind: "service_road".to_string(),
                source: "test bench".to_string(),
                ..Default::default()
            }),
        }];
    }
    let detail = CorridorDetail {
        speed_limits: vec![SpeedLimitSample {
            at_mi: 0.0,
            mph: Some(ROAD_MPH),
            source: "test bench".to_string(),
            hgv: false,
        }],
        grade_segments: vec![GradeSegment::new(0.0, miles, 0.0, "flat", "test bench")],
        interchanges: vec![interchange],
        ..Default::default()
    };
    let leg = Leg::new(&city, &city, miles, "I 90", "flat", vec![stop_record]).with_detail(detail);
    let route = Route::from_legs(vec![city.clone(), city], vec![leg]);
    let truck = d.trip.truck.clone();
    let mut weather = WeatherSystem::new("heartland", Some(3), None, None, true);
    weather.current = WeatherKind::Clear;
    let mut trip = Trip::new(
        route,
        truck,
        weather,
        TripOptions {
            seed: Some(3),
            time_scale: kind.time_scale(),
            ..Default::default()
        },
    );
    quiet(&mut trip);
    trip.zones.clear();
    trip.curves.clear();
    let mut stop = RoadStop::new("Prairie Travel Center", STOP_MI, "truck_stop");
    stop.actions = ["park", "fuel", "food"]
        .iter()
        .map(|a| a.to_string())
        .collect();
    stop.parking = "confirmed".to_string();
    stop.exit_label = "exit 42".to_string();
    stop.interchange_mi = Some(STOP_MI);
    trip.stops = vec![stop];
    d.trip = trip;
    // The crossroad's traffic is seeded off the drive: pinned, so every run
    // meets the same cross traffic.
    d.trip_seed = 7;
    d.reset_turn_state_for_trip();
    d.destination_exit_taken = true;
    d.trip.position_mi = STOP_MI - 2.0;
    // A road stop's streets are off in 1.9; this case drives them as 2.0
    // will, and every other kind is a stop with none.
    d.stop_streets_on = kind == Kind::FreeFlow5x;
}

pub(crate) fn start(preset: Preset, kind: Kind) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    if kind.chain() {
        let world = get_world();
        let (chain, _) = destinations(world, 1);
        let destination = chain.first().expect("a chain facility").clone();
        let origin = world.neighbors(&destination.city)[0]
            .other(&destination.city)
            .to_string();
        preset.apply(&mut harness.app.ctx.settings);
        harness.start_route(
            &origin,
            &destination.city,
            RouteSetup::seeded(4242)
                .named("Exit Matrix")
                .destination_location(&destination.location),
        );
        harness.with_drive(|d, ctx| {
            quiet(&mut d.trip);
            if let Some(profile) = ctx.profile.as_mut() {
                profile.tutorial_done = true;
            }
            let exit = d
                .destination_exit_stop(ctx)
                .expect("a delivery has a destination exit");
            d.trip.position_mi = exit.at_mi - 2.0;
        });
    } else {
        harness.start_delivery(StartDelivery::named("Exit Matrix"));
        preset.apply(&mut harness.app.ctx.settings);
        harness.app.ctx.settings.time_scale = kind.time_scale();
        harness.with_drive(move |d, _| bench(d, kind));
    }
    harness.with_drive(|d, _| {
        d.tutorial = None;
        d.departure_checked = true;
        d.weather_mut().current = WeatherKind::Clear;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().rpm = 1500.0;
        let (limit, _) = d.trip.speed_limit_at(d.trip.position_mi);
        d.truck_mut().velocity_mps = limit * MPS_PER_MPH;
        d.truck_mut().throttle = 0.4;
    });
    if preset.cruise() {
        harness.press_key(Key::K, None);
    }
    harness.press_key(Key::X, None);
    harness
}

// -- the driver ----------------------------------------------------------------------------

/// Deceleration needed to be at `end_mph` in `miles`, m/s^2.
fn needed(speed_mph: f64, end_mph: f64, miles: f64) -> f64 {
    let v = speed_mph / MPH_PER_MPS;
    let e = end_mph / MPH_PER_MPS;
    ((v * v - e * e) / (2.0 * (miles * 1609.344).max(1.0))).max(0.0)
}

/// What the driver wants this frame: `(brake, accelerate)`.
fn driver_keys(d: &mut DrivingState, preset: Preset, kind: Kind, braking: bool) -> (bool, bool) {
    let speed = d.truck().speed_mph();
    let assist_braking = d.truck().brake > 0.01;
    let Some(ramp_mi) = d.ramp_mi else {
        if d.surface_chain || d.trip.is_facility_approach_route() {
            if let Some(keys) = street_bar_keys(d, preset, braking) {
                return keys;
            }
            // The streets: the posted number, the corner's advise speed, and
            // a stop at the gate, braked for late.
            if d.destination_arrival_active || d.keeper_mph.is_some() || d.cruise_mph.is_some() {
                // An assist has the pedals: the driver leaves them alone.
                return (false, false);
            }
            let target = driver_target_mph(d);
            let left = d.trip.remaining_miles();
            let demand = needed(speed, target, 0.02).max(needed(speed, 0.0, left));
            let brake = demand > DRIVER_LATE_MPS2 || (braking && demand > 0.5);
            let go = !brake && !assist_braking && speed < target - 2.0 && left > 0.01;
            return (brake, go);
        }
        // The mainline: the driver holds road speed unless cruise does.
        if preset.cruise() {
            return (false, false);
        }
        let (limit, _) = d.trip.speed_limit_at(d.trip.position_mi);
        return (false, !assist_braking && speed < limit - 1.0);
    };
    if d.destination_arrival_active {
        // Facility stopping assistance has announced it has the pedals.
        return (false, false);
    }
    let exit_mph = d.armed_ramp_mph(None);
    let bar_mi = ramp_mi - RAMP_ACCESS_MI;
    let mut demand = 0.0f64;
    let mut cruise_to = exit_mph;
    if let Some(lane_left) = d.deceleration_lane_left_mi() {
        demand = demand.max(needed(speed, exit_mph, lane_left));
    } else if d.ramp_curve_radius_ft().is_some() && speed > exit_mph + 1.0 {
        demand = demand.max(DRIVER_LATE_MPS2 + 1.0);
    }
    let mut creep = false;
    if !d.ramp_terminal_done && bar_mi > -0.01 {
        // A yield is rolled only into a gap the driver can hear is there,
        // judged close to the line; farther out it is approached as a stop.
        let control = d.ramp_control.clone();
        let clear = if matches!(control.as_str(), "yield" | "roundabout") {
            d.yield_gap_clear()
        } else {
            d.cross_bubble
                .as_ref()
                .is_none_or(|bubble| bubble.clear_to_cross())
        };
        let phase = d.ramp_light_phase();
        // Route-transition assistance rolls a clear yield itself; a driver on
        // their own stops at it and goes when the gap is there.
        let assisted_roll = preset.transition() && clear;
        let end = match control.as_str() {
            "signal" if phase == "green" => GREEN_ROLL_MPH - 5.0,
            "yield" | "roundabout" if assisted_roll => YIELD_ROLL_MPH - 3.0,
            _ => 0.0,
        };
        demand = demand.max(needed(speed, end, bar_mi.max(0.0)));
        // Never on the throttle toward a bar it will have to slow for: a
        // gentle stop profile caps what it drives at.
        let gentle = (end / MPH_PER_MPS).powi(2) + 2.0 * 0.8 * bar_mi.max(0.0) * 1609.344;
        cruise_to = cruise_to.min(gentle.sqrt() * MPH_PER_MPS);
        if end == 0.0 && speed <= RED_STOP_MPH && bar_mi < 0.03 {
            // Stopped at the bar, or just short of it. A red is waited out;
            // a sign or a yield with nothing coming is honoured by creeping
            // over the line at a walk, which counts as the stop.
            let waiting = match control.as_str() {
                "signal" => phase != "green",
                "yield" | "roundabout" => !clear,
                _ => d.ramp_waiting_at_sign && !clear,
            };
            if waiting {
                return (!assist_braking, false);
            }
            creep = true;
        }
    } else {
        // The ramp's end hands off to streets for a chain facility and for a
        // road stop with its own streets to its lot.
        let continues = kind.chain()
            || d.ramp_stop
                .clone()
                .is_some_and(|stop| d.stop_chain_route(&stop).is_some());
        if continues {
            // The ramp's end hands off to the streets: no stop there.
            cruise_to = 20.0;
        } else {
            // Past the terminal: on to the entrance at a walk, and stop there.
            let gentle = (2.0 * 0.8 * ramp_mi.max(0.0) * 1609.344).sqrt() * MPH_PER_MPS;
            cruise_to = 12.0f64.min(gentle.max(4.0));
            demand = demand.max(needed(speed, 0.0, ramp_mi.max(0.0)));
            if ramp_mi <= 0.0 {
                return (!assist_braking, false);
            }
        }
    }
    let brake = demand > DRIVER_LATE_MPS2 || (braking && demand > 0.5);
    if creep {
        return (false, speed < 2.0 && !assist_braking);
    }
    // A driver whose assist has announced it is braking for the terminal
    // leaves the throttle alone until the terminal is behind the truck.
    // Stopped short, it hears how far and closes the gap itself.
    let terminal_owned = d.ramp_assist_said && !d.ramp_terminal_done && speed > RED_STOP_MPH;
    let go = !brake
        && !assist_braking
        && !terminal_owned
        && !d.approach_pull_ahead
        && speed < cruise_to - 3.0
        && (d.ramp_terminal_done || bar_mi > 0.03);
    (brake, go)
}

/// The driver at a light or sign on the streets, or None when there is none
/// to stop for: a green is driven like the street. With route-transition
/// assistance on the assist makes the stop and the driver stays off the
/// pedals unless it is late; on their own they brake late for the bar, wait
/// out a red or a gap, and pull away.
fn street_bar_keys(d: &mut DrivingState, preset: Preset, braking: bool) -> Option<(bool, bool)> {
    if d.street_bar_mi.is_none() || d.ramp_terminal_done || !d.ramp_light_announced {
        return None;
    }
    let gap = d.terminal_gap_mi()?;
    let speed = d.truck().speed_mph();
    let assist_braking = d.truck().brake > 0.01;
    let control = d.ramp_control.clone();
    let phase = d.ramp_light_phase();
    let clear = if control == "yield" {
        d.yield_gap_clear()
    } else {
        d.cross_bubble
            .as_ref()
            .is_none_or(|bubble| bubble.clear_to_cross())
    };
    if control == "signal" && phase == "green" {
        return None;
    }
    let demand = needed(speed, 0.0, gap.max(0.0));
    if speed <= RED_STOP_MPH && gap < 0.03 {
        let waiting = match control.as_str() {
            "signal" => true,
            "yield" => !clear,
            _ => d.ramp_waiting_at_sign && !clear,
        };
        if waiting || (preset.transition() && d.ramp_assist_said) {
            return Some((!assist_braking && !preset.transition(), false));
        }
        return Some((
            false,
            speed < 2.0 && !assist_braking && !d.approach_pull_ahead,
        ));
    }
    let brake = demand > DRIVER_LATE_MPS2 || (braking && demand > 0.5);
    // Up to the bar on a gentle stop profile, and off the throttle once an
    // assist has said it is braking for it.
    let terminal_owned = d.ramp_assist_said && speed > RED_STOP_MPH;
    let gentle = (2.0 * 0.8 * gap.max(0.0) * 1609.344).sqrt() * MPH_PER_MPS;
    let go = !brake
        && !assist_braking
        && !terminal_owned
        && gap > 0.03
        && speed < gentle.min(driver_target_mph(d)) - 3.0;
    Some((brake, go))
}

/// Put a traffic light and a stop sign on the destination's streets, in
/// place of whatever the map has there, so every run meets the same two.
/// The street controls are off in 1.9; this case switches them on as 2.0
/// plays them.
fn lay_street_controls(d: &mut DrivingState) {
    d.street_controls_on = true;
    let streets: Vec<usize> = (1..d.trip.route.legs.len())
        .filter(|i| !d.trip.route.legs[*i].local_yard)
        .collect();
    for leg in d.trip.route.legs.iter_mut() {
        std::sync::Arc::make_mut(leg).local_controls.clear();
    }
    let (light, sign) = match streets.as_slice() {
        [] => return,
        [only] => ((*only, 0.0), (*only, d.trip.route.legs[*only].miles / 2.0)),
        [first, second, ..] => ((*first, 0.0), (*second, 0.0)),
    };
    for ((leg, at_mi), kind) in [(light, "signal"), (sign, "stop")] {
        std::sync::Arc::make_mut(&mut d.trip.route.legs[leg])
            .local_controls
            .push(StreetControl {
                at_mi,
                kind: kind.to_string(),
            });
    }
}

fn hold(harness: &mut PlaytestHarness, brake: bool, go: bool) {
    for (key, held) in [(Key::Down, brake), (Key::Up, go)] {
        if held {
            harness.app.ctx.input.press(key, Mods::NONE);
        } else {
            harness.app.ctx.input.release(key, Mods::NONE);
        }
    }
}

// -- one run ------------------------------------------------------------------------------

#[derive(Default, Debug)]
struct Run {
    worst_mainline_deficit: f64,
    curve_entry: Option<(f64, f64)>,
    arrived: bool,
    driver_brake_frames: usize,
    first_driver_brake: String,
    final_state: String,
    worst_where: String,
    stranded_s: f64,
    crawl_far_s: f64,
    heard: Vec<String>,
}

fn drive(preset: Preset, kind: Kind) -> Run {
    let mut harness = start(preset, kind);
    let mut run = Run::default();
    let mut braking = false;
    let mut on_ramp_seen = false;
    let mut stopped_s = 0.0;
    let mut laid = false;
    let budget = if kind.chain() {
        30 * 60 * 25
    } else {
        30 * 60 * 8
    };
    for frame in 0..budget {
        if !harness.has_drive() {
            harness.finish_timed_state();
            run.arrived = harness.state_is::<FacilityArrivalState>();
            break;
        }
        if harness.state_is::<RestStopState>() || harness.read_drive(|d| d.arrival_menu_open) {
            run.arrived = true;
            break;
        }
        if harness
            .read_drive(|d| d.arrival_full_stop_said && d.truck().speed_mph() <= DOCKING_MAX_MPH)
        {
            run.arrived = true;
            break;
        }
        let on_ramp = harness.read_drive(|d| d.ramp_mi.is_some());
        if on_ramp && !on_ramp_seen {
            on_ramp_seen = true;
        }
        // The light: red until the truck is stopped at it, or green until
        // the truck is through it.
        if on_ramp && kind == Kind::SignalRed {
            harness.with_drive(|d, _| {
                if !d.ramp_waiting_at_light && !d.ramp_terminal_done {
                    d.ramp_light_offset_s = 0.0;
                    d.ramp_light_timer = 0.0;
                }
            });
        }
        if on_ramp && kind == Kind::SignalGreen {
            harness.with_drive(|d, _| {
                if !d.ramp_terminal_done {
                    d.ramp_light_offset_s = d.ramp_light_red_s() + 1.0;
                    d.ramp_light_timer = 0.0;
                }
            });
        }
        if kind == Kind::StreetChain {
            if !laid && harness.read_drive(|d| d.surface_chain) {
                laid = true;
                harness.with_drive(|d, _| lay_street_controls(d));
            }
            harness.with_drive(|d, _| {
                // The street light: red until the truck is stopped at it.
                if d.street_bar_mi.is_some()
                    && d.ramp_control == "signal"
                    && !d.ramp_waiting_at_light
                    && !d.ramp_terminal_done
                {
                    d.ramp_light_offset_s = 0.0;
                    d.ramp_light_timer = 0.0;
                }
            });
        }
        let assist_braking = harness.read_drive(|d| d.truck().brake > 0.01);
        let (brake, go) = harness.with_drive(|d, _| driver_keys(d, preset, kind, braking));
        braking = brake;
        // Counted only where no assist was already on the brake: a foot
        // resting on a pedal the assist is holding has not done its job.
        if brake && !assist_braking {
            run.driver_brake_frames += 1;
            if run.first_driver_brake.is_empty() {
                run.first_driver_brake = harness.read_drive(|d| {
                    format!(
                        "ramp {:?} lane {:?} curve {:?} terminal_done {} control {} speed {:.1} brake {:.2} surface {} arrival {}",
                        d.ramp_mi,
                        d.deceleration_lane_left_mi(),
                        d.ramp_curve_radius_ft(),
                        d.ramp_terminal_done,
                        d.ramp_control,
                        d.truck().speed_mph(),
                        d.truck().brake,
                        d.surface_chain,
                        d.destination_arrival_active,
                    )
                });
            }
        }
        hold(&mut harness, brake, go);
        if preset == Preset::LanePartial {
            // Partial lane keeping holds the lane; the wheel a bend wants is
            // the driver's, and the cue for it is the engine's lean. This
            // driver follows the lean, which is what the cue is for.
            // Following it the way a driver does: into the bend while the
            // truck sits outside the lane's centre, and off again once it is
            // over. The streets are left to lane keeping. And on the approach,
            // Right into the exit lane where the cab says it opens -- the
            // truck is already in the right lane, so that is all it asks.
            let (lean, offset, ramp, exit_lane_open) = harness.read_drive(|d| {
                (
                    d.maneuver_steer_demand(None),
                    d.lane.offset,
                    d.on_laid_out_ramp(),
                    d.lane.exit_lane_open,
                )
            });
            let right = (ramp && lean > 0.1 && offset < 0.1) || exit_lane_open;
            let left = ramp && offset > 0.5;
            for (key, held) in [(Key::Right, right), (Key::Left, left)] {
                if held {
                    harness.app.ctx.input.press(key, Mods::NONE);
                } else {
                    harness.app.ctx.input.release(key, Mods::NONE);
                }
            }
        }
        if std::env::var("MATRIX_TRACE").is_ok_and(|v| v == format!("{kind:?}/{preset:?}"))
            && frame % 10 == 0
            && on_ramp
        {
            let line = harness.read_drive(|d| {
                format!(
                    "ramp {:.4} v {:.1} brake {:.2} thr {:.2} decel {:.2} term {:.2} dest {:.2} curve {:?} done {} keys {brake}/{go}",
                    d.ramp_mi.unwrap_or(0.0),
                    d.truck().speed_mph(),
                    d.truck().brake,
                    d.truck().throttle,
                    d.decel_lane_brake,
                    d.ramp_assist_brake,
                    d.destination_assist_brake,
                    d.curve_servo.as_ref().map(|s| s.brake),
                    d.ramp_terminal_done,
                )
            });
            eprintln!("{line}");
        }
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| {
            let cut_out = d.truck().specs.air_governor_cut_out_psi;
            d.truck_mut().set_air_pressure_psi(cut_out);
            d.update_frame(ctx, DT);
        });
        harness.finish_timed_state();
        if !harness.has_drive() {
            continue;
        }
        let (on_ramp, chain, speed, limit, radius, travelled, stop_left) =
            harness.with_drive(|d, _| {
                // Road speed is the posted number here, or a lower one the
                // road posts within the mile ahead: easing for a real posted
                // drop is not easing for the exit.
                let here = d.trip.position_mi;
                let (limit, _) = d.trip.speed_limit_at(here);
                let (ahead, _) = d.trip.speed_limit_at(here + 1.0);
                let limit = limit.min(ahead);
                let stop_left = match d.ramp_mi {
                    Some(mi) if !d.ramp_terminal_done => mi - RAMP_ACCESS_MI,
                    Some(mi) => mi,
                    // A light or sign on the streets is a stop too.
                    None if d.street_bar_mi.is_some() && !d.ramp_terminal_done => {
                        d.terminal_gap_mi().unwrap_or(0.0)
                    }
                    None => d.trip.remaining_miles(),
                };
                (
                    d.ramp_mi.is_some(),
                    d.surface_chain || d.trip.is_facility_approach_route(),
                    d.truck().speed_mph(),
                    limit,
                    d.ramp_curve_radius_ft(),
                    d.ramp_travelled_mi(),
                    stop_left,
                )
            });
        if !on_ramp
            && !on_ramp_seen
            && !chain
            && frame > 150
            && limit - speed > run.worst_mainline_deficit
        {
            run.worst_mainline_deficit = limit - speed;
            run.worst_where = harness.read_drive(|d| {
                format!(
                    "{:.1} short of the gore, limit {limit}, speed {speed:.1}, cruise {:?} keeper {:?} cap {:?}",
                    d.exit_stop.as_ref().map_or(f64::NAN, |s| s.at_mi - d.trip.position_mi),
                    d.cruise_mph,
                    d.keeper_mph,
                    d.ramp_approach_cap_mph(),
                )
            });
        }
        if radius.is_some() && run.curve_entry.is_none() {
            let exit_mph = harness.read_drive(|d| d.armed_ramp_mph(None));
            run.curve_entry = Some((speed, exit_mph));
        }
        let _ = travelled;
        // Stopped with nothing to stop for, or crawling far from it.
        let waiting = harness.read_drive(|d| {
            d.ramp_waiting_at_light || d.ramp_waiting_at_sign || d.destination_arrival_active
        });
        if speed < 0.5 && !waiting && stop_left > 0.05 {
            stopped_s += DT;
            run.stranded_s = run.stranded_s.max(stopped_s);
        } else {
            stopped_s = 0.0;
        }
        if speed < 5.0 && stop_left > 400.0 / 5280.0 && (on_ramp || chain) && !waiting {
            run.crawl_far_s += DT;
        }
    }
    hold(&mut harness, false, false);
    if !run.arrived && harness.has_drive() {
        run.final_state = harness.read_drive(|d| {
            format!(
                "ramp {:?} speed {:.1} brake {:.2} throttle {:.2} done {} sign_wait {} light_wait {} clear {:?} parking {} latched {:?} pull_ahead {} arrival {} chain {} left {:.2}",
                d.ramp_mi,
                d.truck().speed_mph(),
                d.truck().brake,
                d.truck().throttle,
                d.ramp_terminal_done,
                d.ramp_waiting_at_sign,
                d.ramp_waiting_at_light,
                d.cross_bubble.as_ref().map(|b| b.clear_to_cross()),
                d.truck().parking_brake,
                d.brake_latch,
                d.approach_pull_ahead,
                d.destination_arrival_active,
                d.surface_chain,
                d.trip.remaining_miles(),
            )
        });
    }
    run.heard = harness.transcript();
    run
}

// -- what every run owes -----------------------------------------------------------------

fn faults(preset: Preset, kind: Kind, run: &Run) -> Vec<String> {
    let mut faults = Vec::new();
    let heard = run.heard.join("\n");
    if run.worst_mainline_deficit > EXIT_MAINLINE_EASE_MPH + 3.0 {
        faults.push(format!(
            "{:.1} under road speed on the mainline ({})",
            run.worst_mainline_deficit, run.worst_where
        ));
    }
    match run.curve_entry {
        Some((speed, exit_mph)) if speed > exit_mph + 2.0 => faults.push(format!(
            "met the ramp curve at {speed:.1} against {exit_mph}"
        )),
        None => faults.push("never reached the ramp curve".to_string()),
        _ => {}
    }
    for bad in [
        "far too fast",
        "ran the red",
        "crept through the red",
        "blew the stop sign",
        "rolled the stop sign",
        "forced the gap",
        "into cross traffic",
        "Missed the destination exit",
        "Drove past",
        "missed the exit",
        // Lane keeping, full or partial, holds the ramp curve.
        "Off the pavement",
        "Across the centerline",
        // Nobody here takes the curve over what it costs this load.
        "rolled over",
    ] {
        if heard.contains(bad) {
            faults.push(format!("heard \"{bad}\""));
        }
    }
    // The late-braking driver can be told the curve is coming up too fast:
    // that is the warning doing its job, the reaction time a real driver
    // needs ahead of the braking this one leaves to the last. An assist that
    // owns the exit speed must never leave the truck needing it.
    if preset.owns_ramp_speed() && heard.contains(", too fast. Slow to") {
        faults.push("heard the curve called too fast with an assist on the pedals".to_string());
    }
    if !run.arrived {
        faults.push(format!("never arrived ({})", run.final_state));
    }
    if run.stranded_s > 15.0 {
        faults.push(format!(
            "stood {:.0} s with nothing to stop for",
            run.stranded_s
        ));
    }
    if run.crawl_far_s > 30.0 {
        faults.push(format!(
            "crawled {:.0} s far from the stop",
            run.crawl_far_s
        ));
    }
    let mut settings = Settings::default();
    preset.apply(&mut settings);
    let named = [
        ("Exit speed assistance", settings.exit_speed_assist),
        (
            "Route-transition assistance",
            settings.route_transition_assist,
        ),
        ("Curve assistance", settings.curve_speed_assist),
        (
            "Facility stopping assistance",
            settings.destination_approach_assist,
        ),
        ("Adaptive cruise", preset.cruise()),
        ("Automatic speed control", preset.cruise()),
    ];
    for (name, on) in named {
        if !on && heard.contains(name) {
            faults.push(format!("named {name}, which is off"));
        }
    }
    let automated = settings.lane_is_automated();
    for (phrase, wrong) in [
        ("Tap the Right arrow", !automated),
        ("Hold the Right arrow", automated),
        ("Steer right", automated),
    ] {
        if wrong && heard.contains(phrase) {
            faults.push(format!("said \"{phrase}\" to this lane keeping"));
        }
    }
    if preset == Preset::All && run.driver_brake_frames > 0 {
        faults.push(format!(
            "with every assist on the driver still had to brake ({} frames, first: {})",
            run.driver_brake_frames, run.first_driver_brake
        ));
    }
    if kind == Kind::FreeFlow5x {
        // The truck stop's own streets: off the ramp, down the frontage road,
        // the driveway turn, and the lot.
        for step in [
            "Off the ramp. Start on Prairie Frontage Road",
            "Into the lot. Lot limit 15.",
        ] {
            if !heard.contains(step) {
                faults.push(format!("never heard \"{step}\""));
            }
        }
    } else if !kind.chain() && heard.contains("Off the ramp") {
        // A stop with no streets baked keeps its entrance at the ramp's end.
        faults.push("left the ramp for streets the stop does not have".to_string());
    }
    faults
}

#[test]
#[cfg_attr(ci_quick, ignore = "sweep: every assist over every kind of exit")]
fn test_every_assist_follows_the_exit_rules_on_every_kind_of_exit() {
    let mut failures = Vec::new();
    let mut passed = 0;
    for kind in KINDS {
        for preset in PRESETS {
            let run = drive(preset, kind);
            let found = faults(preset, kind, &run);
            if std::env::var("MATRIX_DUMP").is_ok() {
                eprintln!(
                    "==== {kind:?} x {preset:?} {found:?}\n{}",
                    run.heard.join("\n")
                );
            }
            if found.is_empty() {
                passed += 1;
            } else {
                eprintln!("==== {kind:?} x {preset:?}\n{}", run.heard.join("\n"));
                let tail: Vec<&String> = run.heard.iter().rev().take(25).rev().collect();
                failures.push(format!(
                    "{kind:?} x {preset:?}: {found:?}\n  last heard: {tail:#?}"
                ));
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{passed} of {} passed\n{}",
        KINDS.len() * PRESETS.len(),
        failures.join("\n\n")
    );
}
