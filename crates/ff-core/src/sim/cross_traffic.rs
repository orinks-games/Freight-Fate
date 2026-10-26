//! The cross bubble: real NPC traffic on the road a ramp terminal meets.
//!
//! The mainline traffic bubble rotated ninety degrees (owner design,
//! 2026-08-20 -- "make passing traffic actually pass; this is why they're
//! NPCs"). When a terminal announces, a short simulated stretch of crossroad
//! comes to life around the conflict point: vehicles spawn at the edges with
//! seeded Poisson arrivals, drive through, and despawn on the far side. They
//! are entities, not audio sweeps -- they platoon behind slow leaders (the
//! real burst-then-gap rhythm no random stream produces), they queue for the
//! cross street's own signal phase, and their pan and loudness fall out of
//! simulated position, so finding a gap is a listening skill: exactly how a
//! sighted driver reads an intersection by looking left.
//!
//! Axis convention: position is MILES along the crossroad, negative on the
//! side the vehicle entered from, zero at the conflict point in front of the
//! player's stop bar. Vehicles all drive toward positive (each carries its
//! `from_side` so audio knows which ear it started in); left-entering and
//! right-entering streams are two independent lanes that do not interact,
//! which matches a two-way crossroad seen from the stop bar.
//!
//! Arrival rates are DESIGN CONSTANTS keyed to what the terminal already
//! knows -- its control kind and whether it is near a route city -- declared
//! as such below. The honest number would be the crossroad's own AADT, which
//! the bake does not carry yet; when it does (ROADMAP), the rates read from
//! it and these constants become the fallback.
//!
//! Port of `freight_fate/sim/cross_traffic.py`.

use crate::pyrandom::PyRandom;
use crate::sim::trip_models::{TRUCK_ACCEL_ALPHA_FPS2, TRUCK_ACCEL_BETA};

// -- Where the truck crosses, measured from the yield line --------------------
// A yield is judged where the truck actually meets cross traffic: the
// crossroad, a few feet past the line, for as long as the truck takes to get
// its whole length across it. It used to be judged at an arbitrary point about
// a hundred feet past the line, so a gap that was clear at the line and held
// through the crossing could still read as forced (fix/yield-at-the-line,
// 2026-09-24).

/// READ: MUTCD 11th ed. (2023) Section 3B.19 paragraph 13 -- "the stop line or
/// yield line should be placed at the desired stopping or yielding point, but
/// should not be placed more than 30 feet or less than 4 feet from the nearest
/// edge of the intersecting traveled way".
pub const YIELD_LINE_MIN_TO_CROSSROAD_FT: f64 = 4.0;
pub const YIELD_LINE_MAX_TO_CROSSROAD_FT: f64 = 30.0;
/// ASSUMED: where this line sits in that band, the middle of it. No ramp
/// terminal's markings are baked.
pub const YIELD_LINE_TO_CROSSROAD_FT: f64 =
    (YIELD_LINE_MIN_TO_CROSSROAD_FT + YIELD_LINE_MAX_TO_CROSSROAD_FT) / 2.0;
/// ASSUMED: the crossroad the truck crosses, two 12-foot lanes. Both of this
/// model's streams pass the one conflict point, so the truck clears both.
pub const CROSSROAD_WIDTH_FT: f64 = 24.0;
/// READ: the WB-67 design vehicle's overall length, 73.5 ft (AASHTO Green
/// Book 2018 Table 2-1a), the tractor-semitrailer `data::corners` already
/// turns.
pub const COMBINATION_LENGTH_FT: f64 = 73.5;
/// ASSUMED: a tractor running bobtail, cab to rear bumper.
pub const TRACTOR_LENGTH_FT: f64 = 25.0;

/// Seconds for a truck doing `speed_mph` to cover `feet`, pulling through on
/// a loaded truck's own acceleration (Long, TRR 1737, the model the
/// acceleration lane uses): `a = ALPHA - BETA * v` in feet and seconds.
pub fn truck_time_to_cover_s(speed_mph: f64, feet: f64) -> f64 {
    const DT: f64 = 0.05;
    let mut v = speed_mph.max(0.0) * 5280.0 / 3600.0;
    let mut covered = 0.0;
    let mut t = 0.0;
    while covered < feet && t < 120.0 {
        let a = (TRUCK_ACCEL_ALPHA_FPS2 - TRUCK_ACCEL_BETA * v).max(0.0);
        covered += v * DT + 0.5 * a * DT * DT;
        v += a * DT;
        t += DT;
    }
    t
}

/// When a truck `to_line_ft` short of the yield line (negative once past it)
/// enters the crossroad and when its rear clears it, in seconds from now.
pub fn yield_crossing_times_s(speed_mph: f64, to_line_ft: f64, length_ft: f64) -> (f64, f64) {
    let to_edge = (to_line_ft + YIELD_LINE_TO_CROSSROAD_FT).max(0.0);
    let enter = truck_time_to_cover_s(speed_mph, to_edge);
    let exit = truck_time_to_cover_s(speed_mph, to_edge + CROSSROAD_WIDTH_FT + length_ft);
    (enter, exit)
}

/// The simulated stretch: a third of a mile each side of the conflict point.
pub const CROSS_EXTENT_MI: f64 = 0.35;
/// The conflict window: a vehicle inside this many feet of the crossing line
/// is a collision if the truck noses through. Sized as a generous lane-and-a
/// -half so the window opens only when the crossing is genuinely clear.
pub const CONFLICT_WINDOW_FT: f64 = 55.0;
const CONFLICT_WINDOW_MI: f64 = CONFLICT_WINDOW_FT / 5280.0;

/// Mean seconds between arrivals PER SIDE, by (near_city, control kind).
/// Design constants, stated as such (see module docs): a signalized
/// urban crossroad is a busy arterial, a rural stop-sign road is nearly
/// empty, and free-flow terminals have no cross street at all (the caller
/// never builds a bubble for them). Poisson arrivals; platooning below is
/// what turns these into the bursts and long gaps real junctions have.
pub const ARRIVAL_MEAN_S: [((bool, &str), f64); 6] = [
    ((true, "signal"), 7.0),
    ((true, "stop"), 11.0),
    ((true, "yield"), 9.0),
    ((false, "signal"), 14.0),
    ((false, "stop"), 30.0),
    ((false, "yield"), 22.0),
];
pub const DEFAULT_ARRIVAL_MEAN_S: f64 = 18.0;

/// `ARRIVAL_MEAN_S[(near_city, control)]`, if the table has the pair.
pub fn arrival_mean_s(near_city: bool, control: &str) -> Option<f64> {
    ARRIVAL_MEAN_S
        .iter()
        .find(|((city, kind), _)| *city == near_city && *kind == control)
        .map(|(_, mean)| *mean)
}

/// Crossroad speed by context: urban arterial pace vs a rural two-lane.
pub fn cross_speed_mph(near_city: bool) -> f64 {
    if near_city {
        32.0
    } else {
        48.0
    }
}
pub const CROSS_SPEED_JITTER_MPH: f64 = 6.0;

/// Following model on the cross axis: the speed allowed into a gap is what a
/// comfortable brake can shed inside it, v = SAFE_SPEED_K * sqrt(gap_mi).
/// K = sqrt(2 * a * 3600) with a as mph per second: 200 is a ~5.6 mph/s
/// brake, inside the 8 mph/s the chase below can actually deliver, so the
/// envelope is followable with margin instead of a promise the dynamics miss.
pub const SAFE_SPEED_K: f64 = 200.0;
/// The minimum standing gap is a car length and a half of daylight.
pub const MIN_GAP_MI: f64 = 30.0 / 5280.0;

/// Where cross traffic stops for ITS red: just short of the conflict point,
/// the crossroad's own stop bar.
pub const CROSS_BAR_MI: f64 = -45.0 / 5280.0;

/// Vehicle classes on a crossroad are not an interstate's mix: cars and
/// pickups dominate, semis are rare visitors. Weights are design constants;
/// every class named here has both a pass and a crossing sound shipped
/// (traffic/<class>_cross), so the ear can tell a semi-sized gap problem
/// from a motorcycle-sized one. `(class, weight, length_ft)`.
pub const CROSS_CLASSES: [(&str, f64, f64); 7] = [
    ("car", 5.0, 15.0),
    ("pickup", 2.5, 20.0),
    ("box truck", 0.8, 26.0),
    ("semi", 0.5, 70.0),
    ("motorcycle", 0.5, 8.0),
    ("bus", 0.3, 40.0),
    ("tractor", 0.15, 18.0),
];

/// Tractors belong to farm country.
fn rural_only_bonus(name: &str) -> f64 {
    if name == "tractor" {
        5.0
    } else {
        1.0
    }
}

/// One NPC on the crossroad, driving toward positive positions.
#[derive(Clone, Debug, PartialEq)]
pub struct CrossVehicle {
    pub position_mi: f64,
    pub speed_mph: f64,
    pub target_mph: f64,
    pub vehicle_class: &'static str,
    pub length_mi: f64,
    /// `"left"` | `"right"` -- the ear it entered in.
    pub from_side: &'static str,
    /// Has passed the conflict point (for the sweep cue).
    pub crossed: bool,
    /// Was already entering the intersection when the cross street was held.
    pub committed: bool,
    /// Its crossing cue has been triggered.
    pub sound_started: bool,
    /// Stable for the vehicle's whole crossing, so the game can keep one
    /// sound on one vehicle while the list around it grows and shrinks.
    pub id: u64,
}

impl CrossVehicle {
    pub fn front_mi(&self) -> f64 {
        self.position_mi + self.length_mi
    }
}

const SIDES: [&str; 2] = ["left", "right"];

/// The living crossroad at one terminal.
///
/// `player_has_green` means the cross street is being held. At a signal that
/// includes the player's green and yellow plus any shared red clearance, so
/// cross traffic queues at its own bar before the player's green begins. Stop
/// and yield terminals leave it false (cross traffic has the right of way and
/// never stops).
#[derive(Clone, Debug)]
pub struct CrossTraffic {
    pub seed: i64,
    /// `"signal"` | `"stop"` | `"yield"`
    pub control: String,
    pub near_city: bool,
    pub vehicles: Vec<CrossVehicle>,
    pub player_has_green: bool,
    cross_street_was_stopped: bool,
    rng: PyRandom,
    /// Seconds to the next arrival, per side (left, right).
    next_spawn_s: [f64; 2],
    next_id: u64,
}

impl CrossTraffic {
    /// `CrossTraffic(seed, control, near_city)`, pre-rolled.
    pub fn new(seed: i64, control: &str, near_city: bool) -> Self {
        let mut bubble = Self {
            seed,
            control: control.to_string(),
            near_city,
            vehicles: Vec::new(),
            player_has_green: false,
            cross_street_was_stopped: false,
            rng: PyRandom::new_from_i64(seed),
            next_spawn_s: [0.0, 0.0],
            next_id: 1,
        };
        // Pre-roll the road so the bubble is mid-life when the player first
        // hears it: an intersection does not begin existing when you arrive.
        for _ in 0..120 {
            bubble.update(0.5);
        }
        bubble
    }

    // -- spawning ---------------------------------------------------------

    fn arrival_mean_s(&self) -> f64 {
        arrival_mean_s(self.near_city, &self.control).unwrap_or(DEFAULT_ARRIVAL_MEAN_S)
    }

    fn draw_class(&mut self) -> (&'static str, f64) {
        let classes: Vec<(&'static str, f64, f64)> = if self.near_city {
            CROSS_CLASSES.to_vec()
        } else {
            CROSS_CLASSES
                .iter()
                .map(|&(name, weight, length)| (name, weight * rural_only_bonus(name), length))
                .collect()
        };
        let total: f64 = classes.iter().map(|(_, w, _)| *w).sum();
        let mut roll = self.rng.random() * total;
        for &(name, weight, length) in &classes {
            roll -= weight;
            if roll <= 0.0 {
                return (name, length / 5280.0);
            }
        }
        (classes[0].0, classes[0].2 / 5280.0)
    }

    /// No room at the edge: the last arrival is still on top of it.
    ///
    /// Poisson interarrivals can be near zero; without this gate two
    /// vehicles spawn overlapped and the following model can never
    /// separate them. The waiting arrival is simply held outside the
    /// bubble -- which is where a real platoon forms anyway.
    fn entry_blocked(&self, side: &str) -> bool {
        let edge = -CROSS_EXTENT_MI;
        let longest = CROSS_CLASSES
            .iter()
            .map(|(_, _, length)| *length)
            .fold(f64::MIN, f64::max);
        let room = MIN_GAP_MI + longest / 5280.0;
        self.vehicles
            .iter()
            .any(|v| v.from_side == side && v.position_mi < edge + room)
    }

    fn spawn(&mut self, side: &'static str) {
        let (name, length_mi) = self.draw_class();
        let base = cross_speed_mph(self.near_city);
        let mut speed = base
            + self
                .rng
                .uniform(-CROSS_SPEED_JITTER_MPH, CROSS_SPEED_JITTER_MPH);
        if name == "tractor" {
            speed = speed.min(20.0); // a tractor is the platoon-maker
        }
        // Never enter faster than the gap ahead allows: a fast car arriving
        // on a slow tractor's tail joins the platoon, it does not ram it.
        // (Python `min(..., key=position_mi)`: the FIRST of the rearmost.)
        let mut rear: Option<&CrossVehicle> = None;
        for v in self.vehicles.iter().filter(|v| v.from_side == side) {
            if rear.is_none_or(|r| v.position_mi < r.position_mi) {
                rear = Some(v);
            }
        }
        if let Some(rear) = rear {
            let gap = rear.position_mi - (-CROSS_EXTENT_MI + length_mi);
            let surplus = (gap - MIN_GAP_MI).max(0.0);
            speed = speed.min((rear.speed_mph.powi(2) + SAFE_SPEED_K.powi(2) * surplus).sqrt());
        }
        self.vehicles.push(CrossVehicle {
            position_mi: -CROSS_EXTENT_MI,
            speed_mph: speed,
            target_mph: speed,
            vehicle_class: name,
            length_mi,
            from_side: side,
            crossed: false,
            committed: false,
            sound_started: false,
            id: self.next_id,
        });
        self.next_id += 1;
    }

    // -- the frame --------------------------------------------------------

    /// Advance the crossroad by `dt` REAL seconds (terminals run on
    /// the real clock). Returns vehicles that crossed the conflict point
    /// this frame, for the crossing-sweep cue.
    pub fn update(&mut self, dt: f64) -> Vec<CrossVehicle> {
        if self.player_has_green && !self.cross_street_was_stopped {
            for vehicle in &mut self.vehicles {
                vehicle.committed = vehicle.front_mi() >= CROSS_BAR_MI;
            }
        } else if !self.player_has_green {
            for vehicle in &mut self.vehicles {
                vehicle.committed = false;
            }
        }
        self.cross_street_was_stopped = self.player_has_green;

        for (i, side) in SIDES.iter().enumerate() {
            self.next_spawn_s[i] -= dt;
            if self.next_spawn_s[i] <= 0.0 {
                if self.entry_blocked(side) {
                    self.next_spawn_s[i] = 0.5; // hold at the edge for room
                } else {
                    self.spawn(side);
                    let mean = self.arrival_mean_s();
                    self.next_spawn_s[i] = self.rng.expovariate(1.0 / mean);
                }
            }
        }
        let mut crossed_now: Vec<CrossVehicle> = Vec::new();
        for side in SIDES {
            // Leader first (a stable sort, like Python's).
            let mut lane: Vec<usize> = (0..self.vehicles.len())
                .filter(|&i| self.vehicles[i].from_side == side)
                .collect();
            lane.sort_by(|&a, &b| {
                let ka = -self.vehicles[a].position_mi;
                let kb = -self.vehicles[b].position_mi;
                ka.partial_cmp(&kb).expect("positions are finite")
            });
            // `(position_mi, speed_mph)` of the leader as it stands after its
            // own move this frame.
            let mut leader: Option<(f64, f64)> = None;
            for i in lane {
                let player_has_green = self.player_has_green;
                let v = &mut self.vehicles[i];
                let mut target = v.target_mph;
                // The cross street's own red: queue at its bar while the
                // player holds green. Vehicles already past the bar clear
                // the intersection rather than trapping themselves in it.
                if player_has_green && !v.committed {
                    let bar_gap = (CROSS_BAR_MI - v.front_mi()).max(0.0);
                    target = target.min(SAFE_SPEED_K * bar_gap.sqrt());
                }
                if let Some((leader_pos, leader_speed)) = leader {
                    let gap = leader_pos - v.front_mi();
                    if gap <= MIN_GAP_MI {
                        // Inside the standing gap: fall behind the leader
                        // until daylight reopens.
                        target = target.min((leader_speed - 5.0).max(0.0));
                    } else {
                        // The braking invariant: with both able to brake at
                        // the envelope rate, the gap never closes below the
                        // minimum while v^2 <= leader^2 + K^2 * surplus. The
                        // additive form (leader + K*sqrt(surplus)) permits
                        // more than that and overlapped when a leader was
                        // itself braking for ITS leader.
                        target = target.min(
                            (leader_speed.powi(2) + SAFE_SPEED_K.powi(2) * (gap - MIN_GAP_MI))
                                .sqrt(),
                        );
                    }
                }
                // Constant-rate brake and throttle. Not a proportional chase:
                // error-proportional decay never quite reaches the target, and
                // a follower riding just above its allowed speed compounds the
                // shortfall into the gap until it overlaps its leader. The 8
                // here is the brake the SAFE_SPEED_K envelope assumes margin
                // against.
                if v.speed_mph > target {
                    v.speed_mph = (v.speed_mph - 8.0 * dt).max(target);
                } else {
                    v.speed_mph = (v.speed_mph + 5.0 * dt).min(target);
                }
                v.speed_mph = v.speed_mph.max(0.0);
                let before = v.position_mi;
                v.position_mi += v.speed_mph * dt / 3600.0;
                if player_has_green && !v.committed && v.front_mi() > CROSS_BAR_MI {
                    v.position_mi = CROSS_BAR_MI - v.length_mi;
                    v.speed_mph = 0.0;
                }
                if !v.crossed && before < 0.0 && 0.0 <= v.position_mi {
                    v.crossed = true;
                    crossed_now.push(v.clone());
                }
                leader = Some((v.position_mi, v.speed_mph));
            }
        }
        self.vehicles.retain(|v| v.position_mi <= CROSS_EXTENT_MI);
        crossed_now
    }

    // -- questions the terminal asks --------------------------------------

    /// The vehicle inside the conflict window right now, if any.
    pub fn occupant(&self) -> Option<&CrossVehicle> {
        self.vehicles.iter().find(|v| {
            v.position_mi + v.length_mi > -CONFLICT_WINDOW_MI && v.position_mi < CONFLICT_WINDOW_MI
        })
    }

    /// A vehicle is inside the conflict window right now.
    pub fn occupied(&self) -> bool {
        self.occupant().is_some()
    }

    /// The nearest vehicle that would reach the conflict point within
    /// `within_s` seconds at its current speed -- the one a driver about
    /// to pull out actually has to answer for.
    pub fn approaching(&self, within_s: f64) -> Option<&CrossVehicle> {
        let mut best: Option<&CrossVehicle> = None;
        let mut best_eta = within_s;
        for v in &self.vehicles {
            if v.position_mi >= 0.0 || v.speed_mph <= 1.0 {
                continue;
            }
            let eta = -v.position_mi * 3600.0 / v.speed_mph;
            if eta <= best_eta {
                best_eta = eta;
                best = Some(v);
            }
        }
        best
    }

    /// The gap-acceptance answer: nothing in the window, nothing about
    /// to arrive in it.
    pub fn clear_to_cross(&self) -> bool {
        self.blocker().is_none()
    }

    /// The vehicle that makes `clear_to_cross` false: the one in the window,
    /// else the one about to arrive. The wait names this one, so the words
    /// and the crossing sound are about the same car.
    pub fn blocker(&self) -> Option<&CrossVehicle> {
        self.occupant().or_else(|| self.approaching(4.0))
    }

    /// The first vehicle that will be in the conflict window at any moment
    /// between `enter_s` and `exit_s` from now, at its current speed: the one
    /// a truck crossing over that interval would meet.
    ///
    /// A vehicle occupies the window from the moment its front reaches the
    /// window's near edge until its rear passes the far edge. One already
    /// inside counts from now; one already through never does.
    pub fn conflict_between(&self, enter_s: f64, exit_s: f64) -> Option<&CrossVehicle> {
        let mut best: Option<(&CrossVehicle, f64)> = None;
        for v in &self.vehicles {
            let rear_clear_mi = CONFLICT_WINDOW_MI - v.position_mi;
            if rear_clear_mi <= 0.0 {
                continue; // through already
            }
            let front_to_window_mi = (-CONFLICT_WINDOW_MI - v.front_mi()).max(0.0);
            let (arrive_s, leave_s) = if v.speed_mph <= 0.1 {
                if front_to_window_mi > 0.0 {
                    continue; // standing clear of the window
                }
                (0.0, f64::INFINITY)
            } else {
                let fps = v.speed_mph / 3600.0;
                (front_to_window_mi / fps, rear_clear_mi / fps)
            };
            if arrive_s < exit_s && leave_s > enter_s && best.is_none_or(|(_, t)| arrive_s < t) {
                best = Some((v, arrive_s));
            }
        }
        best.map(|(v, _)| v)
    }

    /// `(vehicle_class, side_now, pan, closeness 0..1)` per vehicle worth
    /// hearing. Pan is where the vehicle IS (negative = left of the
    /// conflict point from the stop bar), closeness drives loudness.
    pub fn audible(&self) -> Vec<(&'static str, &'static str, f64, f64)> {
        let mut out = Vec::new();
        for v in &self.vehicles {
            let closeness = (1.0 - v.position_mi.abs() / CROSS_EXTENT_MI).max(0.0);
            if closeness <= 0.05 {
                continue;
            }
            let mut side = if v.position_mi < 0.0 { "left" } else { "right" };
            if v.from_side == "right" {
                side = if v.position_mi < 0.0 { "right" } else { "left" };
            }
            let pan = if side == "left" { -0.8 } else { 0.8 };
            out.push((v.vehicle_class, side, pan, closeness));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    //! Ported from `tests/test_cross_traffic.py`: the pure-simulation tests
    //! run; the terminal-integration tests (`DrivingEventMixin`) are
    //! ignored until the driving state is ported.
    use super::*;
    use crate::pyfmt::round_py_n;

    fn run(bubble: &mut CrossTraffic, seconds: f64) -> Vec<CrossVehicle> {
        let dt = 0.25;
        let mut crossed = Vec::new();
        for _ in 0..((seconds / dt) as i64) {
            crossed.extend(bubble.update(dt));
        }
        crossed
    }

    // -- the simulation itself -------------------------------------------------

    #[test]
    fn test_same_seed_same_road() {
        let mut a = CrossTraffic::new(42, "signal", true);
        let mut b = CrossTraffic::new(42, "signal", true);
        run(&mut a, 30.0);
        run(&mut b, 30.0);
        let key = |bubble: &CrossTraffic| {
            bubble
                .vehicles
                .iter()
                .map(|v| (v.vehicle_class, round_py_n(v.position_mi, 6), v.from_side))
                .collect::<Vec<_>>()
        };
        assert_eq!(key(&a), key(&b));
    }

    #[test]
    fn test_the_preroll_populates_the_road() {
        // An intersection does not begin existing when the player arrives.
        let bubble = CrossTraffic::new(7, "signal", true);
        assert!(
            !bubble.vehicles.is_empty(),
            "an urban signalized crossroad should be mid-life at first listen"
        );
    }

    #[test]
    fn test_an_urban_signal_is_busier_than_a_rural_stop() {
        let urban: usize = (0..5)
            .map(|s| run(&mut CrossTraffic::new(s, "signal", true), 120.0).len())
            .sum();
        let rural: usize = (0..5)
            .map(|s| run(&mut CrossTraffic::new(s, "stop", false), 120.0).len())
            .sum();
        assert!(urban > 2 * rural, "{urban} {rural}");
    }

    #[test]
    fn test_every_context_has_an_arrival_rate() {
        for near_city in [true, false] {
            for control in ["signal", "stop", "yield"] {
                assert!(arrival_mean_s(near_city, control).is_some());
            }
        }
    }

    #[test]
    fn test_vehicles_despawn_past_the_extent() {
        let mut bubble = CrossTraffic::new(3, "signal", true);
        run(&mut bubble, 300.0);
        assert!(bubble
            .vehicles
            .iter()
            .all(|v| v.position_mi <= CROSS_EXTENT_MI));
    }

    #[test]
    fn test_followers_do_not_drive_through_their_leader() {
        // Platooning is the point: a slow leader collects a queue, it does not
        // get overlapped. Rear bumper of the leader stays ahead of the follower's
        // front bumper (a small numeric tolerance for one integration step).
        let mut bubble = CrossTraffic::new(11, "signal", false);
        for _ in 0..1200 {
            bubble.update(0.25);
            for side in SIDES {
                let mut lane: Vec<&CrossVehicle> = bubble
                    .vehicles
                    .iter()
                    .filter(|v| v.from_side == side)
                    .collect();
                lane.sort_by(|a, b| (-a.position_mi).partial_cmp(&-b.position_mi).unwrap());
                for pair in lane.windows(2) {
                    let (leader, follower) = (pair[0], pair[1]);
                    assert!(follower.front_mi() <= leader.position_mi + 1e-4);
                }
            }
        }
    }

    #[test]
    fn test_cross_traffic_queues_on_the_players_green() {
        // The cross street runs the orthogonal phase: hold the player's green
        // long enough and the cross stream dies at its own bar.
        let mut bubble = CrossTraffic::new(5, "signal", true);
        bubble.player_has_green = true;
        run(&mut bubble, 20.0); // whatever was already inside the bar clears
        let late = run(&mut bubble, 60.0);
        assert!(
            late.is_empty(),
            "no vehicle should cross against its own red"
        );
        assert!(
            bubble.vehicles.iter().any(|v| v.speed_mph < 1.0),
            "a queue should form at the bar"
        );
    }

    #[test]
    fn test_the_queue_dissolves_when_the_light_flips() {
        let mut bubble = CrossTraffic::new(5, "signal", true);
        bubble.player_has_green = true;
        run(&mut bubble, 80.0);
        bubble.player_has_green = false;
        let released = run(&mut bubble, 45.0);
        assert!(
            !released.is_empty(),
            "the held queue should cross once the cross street gets its green"
        );
    }

    #[test]
    fn test_each_crossing_reports_exactly_once() {
        let mut bubble = CrossTraffic::new(9, "stop", true);
        let crossed = run(&mut bubble, 240.0);
        // Python checked `id()` uniqueness; the clones here carry no identity,
        // so the same fact is pinned another way: every reported vehicle is
        // flagged crossed and sits within one frame's travel past the
        // conflict point -- the crossing frame itself, which happens once.
        assert!(crossed.iter().all(|v| v.crossed));
        for v in &crossed {
            assert!(v.position_mi >= 0.0);
            assert!(v.position_mi <= v.speed_mph * 0.25 / 3600.0 + 1e-12);
        }
    }

    #[test]
    fn test_clear_to_cross_means_nothing_there_and_nothing_imminent() {
        let mut bubble = CrossTraffic::new(13, "stop", true);
        let (mut saw_clear, mut saw_blocked) = (false, false);
        for _ in 0..2400 {
            bubble.update(0.25);
            if bubble.clear_to_cross() {
                saw_clear = true;
                assert!(!bubble.occupied());
                assert!(bubble.approaching(4.0).is_none());
            } else if bubble.occupied() || bubble.approaching(4.0).is_some() {
                saw_blocked = true;
            }
        }
        assert!(saw_clear, "a stop-sign crossroad must offer real gaps");
        assert!(saw_blocked, "and real traffic to wait for");
    }

    #[test]
    fn test_a_rural_stop_offers_gaps_within_patience() {
        // The wait must end: at rural stop-sign arrival rates a clear window
        // has to open within a minute of watching, or the sign is a softlock.
        for seed in 0..8 {
            let mut bubble = CrossTraffic::new(seed, "stop", false);
            let mut found = false;
            for _ in 0..240 {
                bubble.update(0.25);
                if bubble.clear_to_cross() {
                    found = true;
                    break;
                }
            }
            assert!(found, "seed {seed}: no gap in 60 seconds at a rural stop");
        }
    }

    #[test]
    fn test_audible_pans_by_the_side_the_vehicle_is_on() {
        let mut bubble = CrossTraffic::new(2, "signal", true);
        run(&mut bubble, 30.0);
        for (class, side, pan, closeness) in bubble.audible() {
            assert!(!class.is_empty());
            assert!(side == "left" || side == "right");
            assert_eq!(pan, if side == "left" { -0.8 } else { 0.8 });
            assert!(closeness > 0.05 && closeness <= 1.0);
        }
    }

    // -- the terminal asks the bubble ------------------------------------------

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving_events (DrivingEventMixin._update_ramp_terminal)"]
    fn test_the_stop_sign_clear_waits_for_the_gap() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving_events (DrivingEventMixin._update_ramp_terminal)"]
    fn test_the_stop_sign_clear_is_immediate_on_an_empty_road() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving_events (DrivingEventMixin._update_ramp_terminal)"]
    fn test_blowing_an_empty_stop_sign_hits_nothing() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving_events (DrivingEventMixin._update_ramp_terminal)"]
    fn test_the_yield_waits_when_stopped_in_traffic() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving_events (DrivingEventMixin._update_ramp_terminal)"]
    fn test_a_clear_yield_is_rolled_not_stopped() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving_events (DrivingEventMixin._update_ramp_terminal)"]
    fn test_a_roundabout_speaks_as_a_roundabout() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving_events (DrivingEventMixin._update_ramp_terminal)"]
    fn test_rolling_a_yield_into_an_occupied_window_clips() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving_events (DrivingEventMixin._ramp_control_for)"]
    fn test_a_baked_yield_control_passes_through_the_chooser() {}

    #[test]
    #[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs states::driving_events (DrivingEventMixin._update_ramp_terminal)"]
    fn test_blowing_an_occupied_stop_sign_still_clips() {}

    /// One car on an empty crossroad, `ft` short of the conflict point.
    fn one_car(ft_short: f64, mph: f64) -> CrossTraffic {
        let mut bubble = CrossTraffic::new(1, "yield", false);
        bubble.vehicles = vec![CrossVehicle {
            position_mi: -(ft_short + 15.0) / 5280.0,
            speed_mph: mph,
            target_mph: mph,
            vehicle_class: "car",
            length_mi: 15.0 / 5280.0,
            from_side: "left",
            crossed: false,
            committed: false,
            sound_started: false,
            id: 1,
        }];
        bubble
    }

    #[test]
    fn test_the_crossing_is_timed_from_the_line_to_the_trucks_rear() {
        // 17 ft of the READ 4 to 30 to the crossroad, then 24 ft of it and
        // a 73.5 ft combination: at a steady-ish 12 mph that is a few seconds.
        let (enter, exit) = yield_crossing_times_s(12.0, 0.0, COMBINATION_LENGTH_FT);
        assert!(enter > 0.5 && enter < 1.5, "{enter}");
        assert!(exit > 4.0 && exit < 6.5, "{exit}");
        // From a stand it is much longer: a loaded truck pulls away slowly.
        let (_, from_stop) = yield_crossing_times_s(0.0, 0.0, COMBINATION_LENGTH_FT);
        assert!(from_stop > 9.0, "{from_stop}");
        // Past the line the entry is now.
        assert_eq!(
            yield_crossing_times_s(12.0, -30.0, COMBINATION_LENGTH_FT).0,
            0.0
        );
    }

    #[test]
    fn test_a_gap_is_judged_over_the_trucks_crossing_not_a_fixed_look() {
        // A car 400 ft out at 45 mph (66 ft/s) is at the window in about
        // five seconds: after a truck that clears in four, inside one that
        // clears in eight.
        let bubble = one_car(400.0, 45.0);
        assert!(bubble.conflict_between(0.0, 4.0).is_none());
        assert!(bubble.conflict_between(0.0, 8.0).is_some());
        // And already gone is never a conflict.
        let gone = one_car(-200.0, 45.0);
        assert!(gone.conflict_between(0.0, 60.0).is_none());
    }
}
