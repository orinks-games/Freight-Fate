//! The origin facility's street chain, driven OUT of the gate.
//!
//! The arrival side was measured and pinned in `states_driving_approach_sweep`:
//! a delivery's last mile of city streets had no real-time pin, so the ground
//! closed four to seven times faster than the brakes could answer for and the
//! truck crossed its own gate at up to twenty-four miles an hour. The fix was
//! one flag on the chain trip.
//!
//! `begin_departure_chain` sets no such flag, so the streets a truck drives
//! LEAVING a facility still run compressed. Whether that costs the driver
//! anything is a different question with a different answer -- there is no
//! assist braking to a gate out here -- but a corner that arrives faster than
//! a driver can take it is a defect wherever it happens, so it had to be
//! measured rather than assumed either way. This file drives the departure the
//! way its twin drives the arrival: one chain-capable facility per state, in a
//! fixed order, every run seeded and its weather pinned, and the driver holding
//! the game's own advised number for whatever corner is in play.
//!
//! THE ANSWER, and what each test here is for.
//!
//! The STREETS are clean and want no pin. `controlled_turn` already holds the
//! clock at real time from the moment a corner enters its own window until the
//! corner resolves, and that window is sized in real seconds, so a corner's
//! run-up is never compressed: 0 to 13 feet of compressed ground per departure
//! in the last tenth of a mile before a corner, against a truck seventy feet
//! long, all of it the frame or two between one corner resolving and the next
//! latching. Pinning the whole clock to real time changes the outcome of two
//! corners in a hundred and twenty-five, and the arrival chain -- which IS
//! pinned -- scores the same on every corner measure. Copying the arrival's
//! flag here would have been copying the flag that exists rather than the
//! fault that exists.
//!
//! What the sweep DID find is two faults of its own, both fixed with it: the
//! acceleration lane that ends the chain is a real length of road spent by
//! compressed ground, and the destination approach assist could not tell a
//! departure from an arrival and braked for the on-ramp as though it were the
//! dock.
//!
//! What it measures, per corner: how much road was left when the corner was
//! first spoken about, how many REAL seconds of hearing-and-acting time that
//! road was worth at the pace the ground was actually moving, and how the
//! corner ended -- taken, missed, or never judged at all. The arrival chain is
//! measured with the same ruler, as the control, behind `arrival_control_probe`.

use ff_core::sim::trip::NAV_LEAD_MIN_MI;
use ff_core::sim::weather::WeatherKind;

use ff_core::sim::trip_models::merge_traffic_target_mph;
use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};
use freight_fate::states::base::Key;
use freight_fate::states::driving_turns::{
    is_judged_turn, TURN_COMMIT_TAIL_MI, TURN_WARNING_REAL_S,
};

use crate::states_driving_approach_sweep::{
    destinations, driver_target_mph, Destination, PER_KIND,
};
use crate::transcript_cruise_support::{frame, hold, quiet, release_keys, DT, MPS_PER_MPH};

// -- what one corner did ---------------------------------------------------------------

/// How a judged street corner ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Taken: the truck was under the advised number and the corner settled.
    Taken,
    /// Failed: a loop-back, or the corner made for the driver.
    Missed,
    /// Neither. The truck ran more than a commit tail past the corner while
    /// its own cue was still speaking, so the corner dropped out of play
    /// unjudged -- no turn, no miss, and nothing said about either.
    Dropped,
    /// Still in front of the truck when the run ended.
    Unreached,
}

/// One judged street corner on a departure chain, and the driver's chance at it.
#[derive(Debug, Clone)]
pub struct Corner {
    pub street: String,
    /// Where the corner is, in chain miles.
    pub at_mi: f64,
    /// Road left when the first word about this corner was spoken, miles.
    /// Negative when the truck was already past it; `None` when nothing was
    /// ever said about it at all.
    pub cue_ahead_mi: Option<f64>,
    /// Real seconds between that first word and the corner going under the
    /// wheels -- negative when the corner came first. This is the whole
    /// number: the window is SIZED in [`TURN_WARNING_REAL_S`] of them, and
    /// compression does not shorten a sentence.
    pub cue_lead_s: Option<f64>,
    /// Clock compression in force at the moment the cue was spoken.
    pub cue_scale: Option<f64>,
    /// Road speed at the corner, mph, and the number it was advised at.
    pub speed_mph: Option<f64>,
    pub advised_mph: f64,
    pub outcome: Outcome,
}

impl Corner {
    /// A corner the driver was given a fair chance at: told about it far
    /// enough ahead to act, and it settled as a turn.
    pub fn fair(&self) -> bool {
        self.outcome != Outcome::Missed
            && self.outcome != Outcome::Dropped
            && self.cue_lead_s.is_some_and(|s| s >= CUE_LEAD_FLOOR_S)
    }
}

/// What one departure did.
#[derive(Debug)]
pub struct Departure {
    /// The truck actually pulled out onto the facility's own streets.
    pub on_chain: bool,
    /// Length of the street chain, miles.
    pub chain_mi: f64,
    /// Stop signs, lights and yields the chain placed on itself.
    pub chain_stops: Vec<String>,
    pub corners: Vec<Corner>,
    /// `turn_miss_count` at the end of the run.
    pub misses: i64,
    /// The acceleration lane the chain hands off to, in feet; the real
    /// seconds the truck actually spent on it; and the road speed reached at
    /// its taper against the number the highway is running.
    pub lane_ft: Option<f64>,
    pub lane_real_s: f64,
    pub merge_mph: Option<f64>,
    pub highway_mph: Option<f64>,
    /// Highest and lowest clock compression seen while on the streets.
    pub scale_lo: f64,
    pub scale_hi: f64,
    /// Highest clock compression seen within [`CORNER_BAND_MI`] of a judged
    /// corner -- the number that says whether corners run on the real clock.
    pub scale_near_corner: f64,
    /// Feet of road covered at compressed pace on the approach to a corner
    /// the truck has still to take.
    pub compressed_band_ft: f64,
    /// Highest clock compression seen anywhere on the acceleration lane.
    pub scale_on_lane: f64,
    /// The destination approach assist claimed the pedals during the run.
    pub assist_fired: bool,
    pub heard: Vec<String>,
}

impl Departure {
    pub fn said(&self, needle: &str) -> bool {
        self.heard.iter().any(|line| line.contains(needle))
    }

    pub fn bad_corners(&self) -> Vec<&Corner> {
        self.corners.iter().filter(|c| !c.fair()).collect()
    }
}

/// Real seconds of hearing-and-acting time a driver is owed for a corner.
///
/// Not a number invented here: [`TURN_WARNING_REAL_S`] is what the turn
/// window is SIZED in, and the whole point of sizing a warning in real
/// seconds is that the driver gets those seconds. A third of it leaves room
/// for a corner the truck was already crawling toward without excusing one
/// that arrives in a couple of seconds.
///
/// Deliberately NOT asserted anywhere. Measured, 74 corners of 125 come in
/// under it on a departure -- and 74 of 125 come in under it on the ARRIVAL
/// chain too, which runs on the real clock. What that number is reading is
/// the shipped street geometry, where a downtown chain puts corners 260 feet
/// apart, and not the clock; asserting it here would pin the map rather than
/// the behaviour. The probes print it because the comparison is the evidence.
pub const CUE_LEAD_FLOOR_S: f64 = TURN_WARNING_REAL_S / 3.0;

/// How close to a corner counts as being at it, for the clock.
///
/// Not a number invented here either: [`NAV_LEAD_MIN_MI`] is the game's own
/// near band, the distance inside which the route stops previewing a maneuver
/// and starts calling it. If the clock is compressed anywhere in there, the
/// corner is arriving faster than it is being spoken about.
pub const CORNER_BAND_MI: f64 = NAV_LEAD_MIN_MI;

/// How the bench driver answers the advised number for a corner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Driver {
    /// Throttle only, the way the arrival sweep drives: lift and let the
    /// game's own automation do the shedding.
    Lift,
    /// Throttle and brake: a driver who actually holds the advised number.
    Hold,
}

/// One departure's setup: who is driving, and at what pacing.
#[derive(Debug, Clone, Copy)]
pub struct Bench {
    pub driver: Driver,
    /// `None` leaves the shipped default pacing; `Some(1.0)` is the real
    /// clock, which is the A side of the compression A/B.
    pub time_scale: Option<f64>,
}

impl Default for Bench {
    fn default() -> Self {
        Bench {
            driver: Driver::Hold,
            time_scale: None,
        }
    }
}

// -- watching a street chain's corners -----------------------------------------------------

/// The corner instrument, armed on whatever street chain the drive is
/// currently running and ticked once per frame.
///
/// Shared by both directions on purpose: the arrival chain is the CONTROL for
/// everything measured on the departure, and a control measured with a
/// different ruler proves nothing.
pub struct CornerWatch {
    corners: Vec<Corner>,
    keys: Vec<String>,
    cue_at_s: Vec<Option<f64>>,
    reach_at_s: Vec<Option<f64>>,
    said_lines: usize,
    /// Worst compression seen approaching an unjudged corner inside
    /// [`CORNER_BAND_MI`], and how many real seconds were spent compressed
    /// there. The worst reading alone is twitchy -- one frame between one
    /// corner resolving and the next latching reads as full compression --
    /// so the seconds are the number that means anything.
    pub scale_near_corner: f64,
    pub compressed_band_ft: f64,
}

impl CornerWatch {
    /// Read the judged corners off the chain the drive is on right now.
    pub fn arm(harness: &mut PlaytestHarness) -> Self {
        let (corners, keys): (Vec<Corner>, Vec<String>) = harness.with_drive(|d, _| {
            let cues: Vec<_> = d
                .trip
                .navigation_cues
                .iter()
                .filter(|cue| is_judged_turn(cue))
                .cloned()
                .collect();
            let corners = cues
                .iter()
                .map(|cue| Corner {
                    street: d.turn_street_text(cue),
                    at_mi: cue.at_mi,
                    cue_ahead_mi: None,
                    cue_lead_s: None,
                    cue_scale: None,
                    speed_mph: None,
                    advised_mph: d.turn_speed_mph(cue),
                    outcome: Outcome::Unreached,
                })
                .collect();
            let keys = cues.iter().map(|cue| cue.key.clone()).collect();
            (corners, keys)
        });
        let said_lines = harness.transcript().len();
        let n = corners.len();
        CornerWatch {
            corners,
            keys,
            cue_at_s: vec![None; n],
            reach_at_s: vec![None; n],
            said_lines,
            scale_near_corner: 0.0,
            compressed_band_ft: 0.0,
        }
    }

    /// One frame, read before the frame is stepped.
    pub fn tick(&mut self, harness: &mut PlaytestHarness, real_s: f64, position: f64, speed: f64) {
        let scale = harness.read_drive(|d| d.trip.effective_time_scale());
        // Only the APPROACH counts, and only while the corner is still to be
        // judged: the clock is free to compress again the moment a corner is
        // behind the truck, and a tenth of a mile of tail is exactly that.
        // Inside the corner's brake point, that is: reaction seconds plus the
        // shed to its advise speed, where the owner's design puts real time
        // (ROADMAP, flight's notes). Before it the clock may still be paced.
        let approaching = (0..self.corners.len()).any(|i| {
            let ahead = self.corners[i].at_mi - position;
            if !(0.0 < ahead && ahead <= CORNER_BAND_MI) {
                return false;
            }
            let key = self.keys[i].clone();
            harness.read_drive(|d| {
                let brake_mi = d
                    .trip
                    .navigation_cues
                    .iter()
                    .find(|cue| cue.key == key)
                    .map_or(0.0, |cue| d.turn_brake_point_mi(cue));
                !d.turn_resolved.contains(&key) && ahead <= brake_mi
            })
        });
        if approaching {
            self.scale_near_corner = self.scale_near_corner.max(scale);
            if scale > 1.5 {
                // Ground, not seconds: what a compressed tick costs is the
                // road it eats, and that is what a driver has to answer for.
                let mps = harness.read_drive(|d| d.truck().velocity_mps);
                self.compressed_band_ft += mps * scale * DT * 3.28084;
            }
        }
        // Has anything been SAID about a corner yet? Read it off the
        // transcript rather than off a latch: what the driver got is the
        // sentence the pacer actually delivered, and three different latches
        // can carry the first one. The first pending corner whose street is
        // named wins the line, because a chain can carry the same name twice.
        let lines = harness.transcript();
        for line in lines.iter().skip(self.said_lines) {
            if !line.to_lowercase().contains("turn") {
                continue;
            }
            for i in 0..self.corners.len() {
                if self.cue_at_s[i].is_none() && line.contains(&self.corners[i].street) {
                    self.cue_at_s[i] = Some(real_s);
                    self.corners[i].cue_ahead_mi = Some(self.corners[i].at_mi - position);
                    self.corners[i].cue_scale = Some(scale);
                    break;
                }
            }
        }
        self.said_lines = lines.len();
        for i in 0..self.corners.len() {
            if self.reach_at_s[i].is_none() && position >= self.corners[i].at_mi {
                self.reach_at_s[i] = Some(real_s);
                self.corners[i].speed_mph = Some(speed);
            }
            // A corner more than a commit tail behind the truck is out of
            // play for good: settled, failed, or quietly abandoned.
            if self.corners[i].outcome == Outcome::Unreached
                && position > self.corners[i].at_mi + TURN_COMMIT_TAIL_MI
            {
                let key = self.keys[i].clone();
                let judged = harness.read_drive(|d| d.turn_resolved.contains(&key));
                self.corners[i].outcome = if judged {
                    Outcome::Taken
                } else {
                    Outcome::Dropped
                };
            }
        }
    }

    /// Close the books against everything the driver heard.
    pub fn finish(mut self, heard: &[String]) -> Vec<Corner> {
        for (i, corner) in self.corners.iter_mut().enumerate() {
            corner.cue_lead_s = match (self.cue_at_s[i], self.reach_at_s[i]) {
                (Some(cue), Some(reach)) => Some(reach - cue),
                _ => None,
            };
            if heard
                .iter()
                .any(|line| line.contains(&format!("Missed the turn onto {}", corner.street)))
            {
                corner.outcome = Outcome::Missed;
            }
        }
        self.corners
    }
}

// -- driving one --------------------------------------------------------------------------

/// Pull out of `origin`'s gate and drive its streets to the on-ramp.
pub fn depart(origin: &Destination) -> Departure {
    depart_on(origin, Bench::default())
}

pub fn depart_on(origin: &Destination, bench: Bench) -> Departure {
    let world = ff_core::data::world::get_world();
    let destination = world.neighbors(&origin.city)[0]
        .other(&origin.city)
        .to_string();
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.destination_approach_assist = true;
    harness.app.ctx.settings.speed_keeper = true;
    if let Some(scale) = bench.time_scale {
        harness.app.ctx.settings.time_scale = scale;
    }
    harness.start_route(
        &origin.city,
        &destination,
        RouteSetup::seeded(4242)
            .named("Departure Sweep")
            .origin_location(&origin.location),
    );
    harness.with_drive(|d, ctx| {
        quiet(&mut d.trip);
        // Pinned, not drawn: a sweep whose sky differs per facility is
        // measuring the sky.
        d.weather_mut().current = WeatherKind::Clear;
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
        d.truck_mut().start_engine();
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().set_air_ready(false);
        d.speed_control_armed = true;
    });
    harness.clear_speech();

    // The first frame is the one that pulls out of the gate.
    frame(&mut harness, DT);
    if !harness.read_drive(|d| d.departure_chain) {
        return Departure {
            on_chain: false,
            chain_mi: 0.0,
            chain_stops: Vec::new(),
            corners: Vec::new(),
            misses: 0,
            lane_ft: None,
            lane_real_s: 0.0,
            merge_mph: None,
            highway_mph: None,
            scale_lo: 0.0,
            scale_hi: 0.0,
            scale_near_corner: 0.0,
            compressed_band_ft: 0.0,
            scale_on_lane: 0.0,
            assist_fired: false,
            heard: harness.transcript(),
        };
    }
    // The chain is a trip of its own, built inside the drive; quiet it the
    // same way the highway trip was, so the only thing under measurement is
    // the road.
    harness.with_drive(|d, _| quiet(&mut d.trip));

    let chain_mi = harness.read_drive(|d| d.trip.total_miles());
    let chain_stops = harness.read_drive(|d| {
        d.trip
            .stops
            .iter()
            .map(|stop| format!("{} ({})", stop.name, stop.stop_type))
            .collect::<Vec<String>>()
    });
    let mut watch = CornerWatch::arm(&mut harness);

    let mut scale_lo = f64::MAX;
    let mut scale_hi = 0.0f64;
    let mut lane_ft: Option<f64> = None;
    let mut lane_real_s = 0.0f64;
    let mut merge_mph: Option<f64> = None;
    let mut highway_mph: Option<f64> = None;
    let mut scale_on_lane = 0.0f64;
    let mut assist_fired = false;
    let mut real_s = 0.0f64;
    let mut left_chain = false;
    // Three miles of city streets, loop-backs included, plus an acceleration
    // lane -- all of it on the real clock, so this is minutes of them.
    for _ in 0..(60 * 1800) {
        if !harness.has_drive() {
            break;
        }
        let still_on_chain = harness.read_drive(|d| d.departure_chain);
        if still_on_chain {
            let (position, speed, scale) = harness.read_drive(|d| {
                (
                    d.trip.position_mi,
                    d.truck().speed_mph(),
                    d.trip.effective_time_scale(),
                )
            });
            scale_lo = scale_lo.min(scale);
            scale_hi = scale_hi.max(scale);
            assist_fired |= harness.read_drive(|d| d.destination_arrival_active);
            watch.tick(&mut harness, real_s, position, speed);
            let target = harness.with_drive(|d, _| driver_target_mph(d));
            if target > speed + 2.0 {
                hold(&mut harness, &[Key::Up]);
            } else if bench.driver == Driver::Hold && speed > target + 1.0 {
                hold(&mut harness, &[Key::Down]);
            } else {
                release_keys(&mut harness);
            }
        } else {
            if !left_chain {
                left_chain = true;
                lane_ft = harness.read_drive(|d| d.departure_ramp_mi.map(|mi| mi * 5280.0));
                highway_mph =
                    Some(harness.with_drive(|d, _| d.trip.speed_limit_at(d.trip.position_mi).0));
            }
            // On the acceleration lane the driver is flat out, which is what
            // "build your speed and look for a gap" asks for.
            hold(&mut harness, &[Key::Up]);
            scale_on_lane =
                scale_on_lane.max(harness.read_drive(|d| d.trip.effective_time_scale()));
            lane_real_s += DT;
            assist_fired |= harness.read_drive(|d| d.destination_arrival_active);
            if merge_mph.is_none() && harness.read_drive(|d| d.departure_ramp_mi.is_none()) {
                merge_mph = Some(harness.read_drive(|d| d.truck().speed_mph()));
                break;
            }
        }
        frame(&mut harness, DT);
        real_s += DT;
    }
    release_keys(&mut harness);
    let misses = harness.read_drive(|d| d.turn_miss_count);
    let heard = harness.transcript();
    let scale_near_corner = watch.scale_near_corner;
    let compressed_band_ft = watch.compressed_band_ft;
    let corners = watch.finish(&heard);
    Departure {
        on_chain: true,
        chain_mi,
        chain_stops,
        corners,
        misses,
        lane_ft,
        lane_real_s,
        merge_mph,
        highway_mph,
        scale_lo: if scale_lo == f64::MAX { 0.0 } else { scale_lo },
        scale_hi,
        scale_near_corner,
        compressed_band_ft,
        scale_on_lane,
        assist_fired,
        heard,
    }
}

// -- the control: the same instrument on the ARRIVAL chain ---------------------------------

/// Drive `destination`'s arrival chain and watch its corners the same way.
///
/// This is the control for every corner number in this file. The arrival
/// chain is pinned to real time and was signed off two commits ago, so
/// whatever it scores is what a street chain scores when the clock is not the
/// problem -- and a departure number is only evidence of a departure defect
/// where it is WORSE than this.
pub fn arrive_corners(destination: &Destination, bench: Bench) -> (Vec<Corner>, Vec<String>) {
    let world = ff_core::data::world::get_world();
    let origin = world.neighbors(&destination.city)[0]
        .other(&destination.city)
        .to_string();
    let mut harness = PlaytestHarness::new();
    harness.app.ctx.settings.destination_approach_assist = true;
    harness.app.ctx.settings.speed_keeper = true;
    if let Some(scale) = bench.time_scale {
        harness.app.ctx.settings.time_scale = scale;
    }
    harness.start_route(
        &origin,
        &destination.city,
        RouteSetup::seeded(4242)
            .named("Departure Control")
            .destination_location(&destination.location),
    );
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
        d.exit_signal_on = true;
        d.trip.position_mi = at;
        d.truck_mut().velocity_mps = 40.0 * MPS_PER_MPH;
        d.update_exit(ctx, 0.0, 0.0);
    });
    harness.with_drive(|d, _| {
        d.ramp_control = String::new();
        d.ramp_terminal_done = true;
    });
    harness.clear_speech();

    // Roll the ramp until the chain takes over, then arm the instrument on it.
    let mut watch: Option<CornerWatch> = None;
    let mut real_s = 0.0f64;
    let mut handed_off = false;
    for _ in 0..(60 * 600) {
        if !harness.has_drive() || harness.read_drive(|d| d.arrival_menu_open) {
            break;
        }
        if watch.is_none() && harness.read_drive(|d| d.surface_chain) {
            harness.with_drive(|d, _| quiet(&mut d.trip));
            watch = Some(CornerWatch::arm(&mut harness));
            real_s = 0.0;
        }
        let (position, speed) = harness.read_drive(|d| (d.trip.position_mi, d.truck().speed_mph()));
        if let Some(watch) = watch.as_mut() {
            watch.tick(&mut harness, real_s, position, speed);
        }
        // The arrival assist owns the pedals once it claims them; before that
        // the driver holds the advised number exactly as on the departure.
        if !handed_off {
            handed_off = harness.read_drive(|d| d.destination_arrival_active);
        }
        if handed_off {
            release_keys(&mut harness);
        } else {
            let target = harness.with_drive(|d, _| driver_target_mph(d));
            if target > speed + 2.0 {
                hold(&mut harness, &[Key::Up]);
            } else if bench.driver == Driver::Hold && speed > target + 1.0 {
                hold(&mut harness, &[Key::Down]);
            } else {
                release_keys(&mut harness);
            }
        }
        if harness.read_drive(|d| d.arrival_full_stop_said && d.truck().speed_mph() <= 2.0) {
            break;
        }
        frame(&mut harness, DT);
        real_s += DT;
    }
    release_keys(&mut harness);
    let heard = harness.transcript();
    let corners = watch.map(|w| w.finish(&heard)).unwrap_or_default();
    (corners, heard)
}

// -- what every departure owes the driver --------------------------------------------------

/// A tractor-trailer's own length, the arrival sweep's yardstick for "at" a
/// point rather than a city block from it.
pub const TRUCK_LENGTH_FT: f64 = 70.0;

/// The 25 chain-capable facilities this file departs from, one per state.
fn origins() -> Vec<Destination> {
    let world = ff_core::data::world::get_world();
    let (chain, _plain) = destinations(world, PER_KIND);
    assert_eq!(
        chain.len(),
        PER_KIND,
        "the shipped world no longer offers {PER_KIND} chain-capable facilities"
    );
    chain
}

/// The number the merge line quotes, or None when it was never spoken.
fn lane_ending_mph(heard: &[String]) -> Option<f64> {
    let line = heard.iter().find(|line| line.contains("Lane ending at"))?;
    let tail = line.split("Lane ending at ").nth(1)?;
    tail.split_whitespace().next()?.parse::<f64>().ok()
}

#[test]
#[cfg_attr(
    ci_quick,
    ignore = "sweep: driven out of every facility exit on the map -- left to the nightly by --cfg ci_quick"
)]
fn test_the_acceleration_lane_out_of_a_yard_is_not_outrun_by_the_clock() {
    let mut failures: Vec<String> = Vec::new();
    for origin in origins() {
        let run = depart(&origin);
        assert!(
            run.on_chain,
            "{} ({}): never pulled out onto its own streets",
            origin.location, origin.city
        );
        let fault = |why: String| format!("{} ({}): {why}", origin.location, origin.city);
        let (Some(lane_ft), Some(merge_mph)) = (run.lane_ft, run.merge_mph) else {
            failures.push(fault(
                "the chain never handed off to an acceleration lane".to_string(),
            ));
            continue;
        };
        // 1. The lane is a length of REAL road, so it is driven on the real
        //    clock. Compression here spends the lane at the ground rate while
        //    the truck builds speed at the real one.
        if run.scale_on_lane > 1.0 {
            failures.push(fault(format!(
                "the acceleration lane ran at {:.1} times real time",
                run.scale_on_lane
            )));
        }
        // 2. And the self-contradiction that catches it however it is caused:
        //    a truck cannot cross a real length of road in less real time
        //    than its own HIGHEST speed over that road would take. It builds
        //    speed the whole way, so the honest figure is longer still.
        //    Measured before the pin: Abilene's 1790 feet went by in 12.8
        //    real seconds, where holding the 27 mph it reached needs 45.2.
        let at_top_speed_s = lane_ft / (merge_mph.max(1.0) * 5280.0 / 3600.0);
        if run.lane_real_s < at_top_speed_s {
            failures.push(fault(format!(
                "{lane_ft:.0} feet of acceleration lane went by in {:.1} real seconds, and a \
                 truck holding the {merge_mph:.0} mph it reached needs {at_top_speed_s:.1}",
                run.lane_real_s
            )));
        }
        // 3. The lane is announced, and the closing line tells the truth
        //    about the speed the driver actually has to merge with. A number
        //    that is not the truck's is worse than no number.
        if !run.said("of acceleration lane.") {
            failures.push(fault(
                "was handed an acceleration lane in silence".to_string(),
            ));
        }
        match lane_ending_mph(&run.heard) {
            None => {
                let road = run.highway_mph.unwrap_or(0.0);
                // The half mile an hour is the game's own grace, and this
                // rule has to share it or the two disagree over a sliver:
                // Ardmore Company Yard reaches its taper at 40.9 against a
                // 41.25 target -- 74.4 percent of the road instead of 75 --
                // and demanding "take a big gap" there is demanding a warning
                // about nothing (2026-09-20).
                if merge_mph + 0.5 < merge_traffic_target_mph(road) {
                    failures.push(fault(format!(
                        "reached the taper {:.0} under the road and was not told",
                        road - merge_mph
                    )));
                }
            }
            Some(said) if (said - merge_mph).abs() > 1.0 => {
                failures.push(fault(format!(
                    "was told the lane was ending at {said:.0} miles per hour while doing \
                     {merge_mph:.0}"
                )));
            }
            Some(_) => {}
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {PER_KIND} departures were run off their own acceleration lane:\n\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[cfg_attr(
    ci_quick,
    ignore = "sweep: driven out of every facility exit on the map -- left to the nightly by --cfg ci_quick"
)]
fn test_the_arrival_assist_never_takes_the_pedals_leaving_a_yard() {
    let mut failures: Vec<String> = Vec::new();
    for origin in origins() {
        let run = depart(&origin);
        // The origin's street chain is the same shape as the destination's --
        // one city, local legs, and the route test cannot tell them apart --
        // so the arrival assist read the on-ramp at the end of a departure as
        // the dock and braked for it. All twenty-five did this. A driver who
        // turns the assist on to be stopped at deliveries must not be stopped
        // on the way out of the yard they have just loaded at.
        if run.assist_fired {
            failures.push(format!(
                "{} ({}): the destination approach assist claimed the pedals leaving the yard",
                origin.location, origin.city
            ));
        }
        if run.said("Facility stopping assistance taking the pedals") {
            failures.push(format!(
                "{} ({}): was told the destination approach was slowing it with the delivery \
                 still a whole run away",
                origin.location, origin.city
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} faults across {PER_KIND} departures:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

#[test]
#[cfg_attr(
    ci_quick,
    ignore = "sweep: driven out of every facility exit on the map -- left to the nightly by --cfg ci_quick"
)]
fn test_a_departure_corner_is_never_approached_at_compressed_pace() {
    // THE VERDICT this file was written to reach, pinned so that it stays
    // true.
    //
    // The arrival chain's fault was the clock: it had no real-time pin, so
    // the ground closed seven times faster than the brakes could answer for.
    // The departure chain has no such pin either -- and measurably does not
    // need one, because the corner commitment loop already holds the clock at
    // real time from the moment a corner enters its own window until the
    // corner resolves, and that window is itself sized in real seconds.
    //
    // Measured over these twenty-five departures: the only compressed ground
    // on any corner approach is the frame or two between one corner resolving
    // and the next latching -- 0 to 13 feet per departure, against a truck 70
    // feet long. Driving the same twenty-five with the whole clock pinned to
    // real time changes the outcome of 2 corners in 125, and the arrival
    // chain, which IS pinned, scores the same on every corner measure.
    //
    // Since 2026-09-23 the pin starts at the corner's brake point rather than
    // at its call (a 30 mph approach crawled four real minutes from a
    // two-mile call), so the band this measures is the brake point, not a
    // flat tenth of a mile.
    //
    // So this asserts the mechanism rather than a flag: however the pinning
    // is done, the run-up inside a corner's brake point may not be covered
    // at compressed pace. Take the latch away, or
    // stop sizing the window in real seconds, and the compressed ground here
    // goes from feet to hundreds of feet and this fails.
    let mut failures: Vec<String> = Vec::new();
    let mut corners = 0;
    for origin in origins() {
        let run = depart(&origin);
        corners += run.corners.len();
        if run.compressed_band_ft > TRUCK_LENGTH_FT {
            failures.push(format!(
                "{} ({}): {:.0} feet of the run-up to a corner went by at up to {:.1} times real \
                 time",
                origin.location, origin.city, run.compressed_band_ft, run.scale_near_corner,
            ));
        }
    }
    assert!(
        corners >= 100,
        "only {corners} judged corners across {PER_KIND} chain departures: the sweep has lost \
         the coverage it was built for"
    );
    assert!(
        failures.is_empty(),
        "{} of {PER_KIND} departures drove up to a corner on the compressed clock:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

// -- the probe -----------------------------------------------------------------------------

fn print_run(origin: &Destination, run: &Departure) {
    println!(
        "{:<8} {:<24} {:<40} chain={:.2}mi scale={:.1}..{:.1} corners={} misses={} lane={:?}ft in {:.1}s merge={:?}/{:?} stops={:?}",
        if run.on_chain { "ok" } else { "NOCHAIN" },
        origin.city,
        origin.location,
        run.chain_mi,
        run.scale_lo,
        run.scale_hi,
        run.corners.len(),
        run.misses,
        run.lane_ft.map(|v| v.round()),
        run.lane_real_s,
        run.merge_mph.map(|v| v.round()),
        run.highway_mph.map(|v| v.round()),
        run.chain_stops,
    );
    for corner in &run.corners {
        println!(
            "    {:<6} {:<28} at={:.3} advised={:.0} ahead={:?} lead={:?} scale={:?} v={:?}",
            if corner.fair() { "" } else { "BAD" },
            corner.street,
            corner.at_mi,
            corner.advised_mph,
            corner.cue_ahead_mi.map(|v| (v * 1000.0).round() / 1000.0),
            corner.cue_lead_s.map(|v| (v * 10.0).round() / 10.0),
            corner.cue_scale.map(|v| (v * 10.0).round() / 10.0),
            corner.speed_mph.map(|v| (v * 10.0).round() / 10.0),
        );
        if corner.outcome != Outcome::Taken {
            println!("           outcome: {:?}", corner.outcome);
        }
    }
}

fn probe(bench: Bench, verbose: bool) {
    let world = ff_core::data::world::get_world();
    let (chain, _plain) = destinations(world, PER_KIND);
    let (mut corners, mut bad, mut missed, mut dropped, mut thin) = (0, 0, 0, 0, 0);
    let mut facilities_with_bad: Vec<String> = Vec::new();
    let mut short_lane = 0;
    for origin in chain.iter() {
        let run = depart_on(origin, bench);
        if verbose {
            print_run(origin, &run);
        }
        corners += run.corners.len();
        for corner in &run.corners {
            if !corner.fair() {
                bad += 1;
            }
            match corner.outcome {
                Outcome::Missed => missed += 1,
                Outcome::Dropped => dropped += 1,
                _ => {}
            }
            if !corner.cue_lead_s.is_some_and(|s| s >= CUE_LEAD_FLOOR_S) {
                thin += 1;
            }
        }
        if !run.bad_corners().is_empty() {
            facilities_with_bad.push(format!("{} ({})", origin.location, origin.city));
        }
        if let (Some(merge), Some(highway)) = (run.merge_mph, run.highway_mph) {
            if highway - merge >= 10.0 {
                short_lane += 1;
            }
        }
    }
    println!("--- bench {bench:?}");
    println!(
        "corners {corners}: bad {bad} (missed {missed}, dropped {dropped}, lead under \
         {CUE_LEAD_FLOOR_S:.1}s {thin}); facilities with a bad corner: {}",
        facilities_with_bad.len()
    );
    for name in &facilities_with_bad {
        println!("    {name}");
    }
    println!("merges more than 10 mph under the road: {short_lane}");
}

#[test]
#[ignore = "sweep probe: the same drives, printed rather than asserted"]
fn departure_probe() {
    probe(Bench::default(), true);
}

#[test]
#[ignore = "sweep probe: one facility, everything the driver heard"]
fn departure_probe_transcript() {
    let world = ff_core::data::world::get_world();
    let (chain, _plain) = destinations(world, PER_KIND);
    for origin in chain.iter().take(4) {
        let run = depart_on(origin, Bench::default());
        print_run(origin, &run);
        for line in &run.heard {
            println!("      | {line}");
        }
        println!();
    }
}

#[test]
#[ignore = "sweep probe: the A/B that isolates clock compression"]
fn departure_probe_ab() {
    for driver in [Driver::Lift, Driver::Hold] {
        for time_scale in [None, Some(1.0)] {
            probe(Bench { driver, time_scale }, false);
        }
    }
}

#[test]
#[ignore = "sweep probe: the arrival chain, measured with the departure's ruler"]
fn arrival_control_probe() {
    let world = ff_core::data::world::get_world();
    let (chain, _plain) = destinations(world, PER_KIND);
    for bench in [
        Bench {
            driver: Driver::Hold,
            time_scale: None,
        },
        Bench {
            driver: Driver::Lift,
            time_scale: None,
        },
    ] {
        let (mut total, mut bad, mut missed, mut dropped, mut thin) = (0, 0, 0, 0, 0);
        for destination in chain.iter() {
            let (corners, _heard) = arrive_corners(destination, bench);
            total += corners.len();
            for corner in &corners {
                if !corner.fair() {
                    bad += 1;
                }
                match corner.outcome {
                    Outcome::Missed => missed += 1,
                    Outcome::Dropped => dropped += 1,
                    _ => {}
                }
                if !corner.cue_lead_s.is_some_and(|s| s >= CUE_LEAD_FLOOR_S) {
                    thin += 1;
                }
            }
        }
        println!("--- ARRIVAL control, bench {bench:?}");
        println!(
            "corners {total}: bad {bad} (missed {missed}, dropped {dropped}, lead under \
             {CUE_LEAD_FLOOR_S:.1}s {thin})"
        );
    }
}

#[test]
#[ignore = "sweep probe: the acceleration lane the chain hands off to"]
fn departure_lane_probe() {
    let world = ff_core::data::world::get_world();
    let (chain, _plain) = destinations(world, PER_KIND);
    let mut short = Vec::new();
    for origin in chain.iter() {
        let run = depart_on(origin, Bench::default());
        let (Some(lane), Some(merge), Some(highway)) =
            (run.lane_ft, run.merge_mph, run.highway_mph)
        else {
            println!("{:<24} no acceleration lane", origin.city);
            continue;
        };
        let lane_at_speed_s = lane / (merge.max(1.0) * 1.46667);
        println!(
            "{:<24} lane {:>5.0} ft, driven in {:>5.1} real s (a truck holding {:.0} mph needs \
             {:>5.1} s), taper {:>4.0} mph vs road {:>3.0} mph, short by {:>4.0}",
            origin.city,
            lane,
            run.lane_real_s,
            merge,
            lane_at_speed_s,
            merge,
            highway,
            highway - merge,
        );
        println!(
            "    clock: lane worst {:.1}; approaching a corner inside {:.2} mi, worst {:.1},              {:.0} ft compressed; assist fired {}",
            run.scale_on_lane,
            CORNER_BAND_MI,
            run.scale_near_corner,
            run.compressed_band_ft,
            run.assist_fired,
        );
        short.push(highway - merge);
    }
    short.sort_by(f64::total_cmp);
    let n = short.len();
    println!(
        "merge shortfall over {n} departures: min {:.0}, median {:.0}, max {:.0}; more than 10 \
         under: {}",
        short.first().copied().unwrap_or(0.0),
        short[n / 2],
        short.last().copied().unwrap_or(0.0),
        short.iter().filter(|v| **v >= 10.0).count(),
    );
}
