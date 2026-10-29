//! Lane model for the 1-D driving view: a discrete lane index plus a
//! continuous position within the current lane.
//!
//! `lane` counts from the rightmost driving lane (0) leftward; a rural
//! two-lane interstate is lanes 0 (right) and 1 (left). `offset` is centered
//! at 0.0 within the current lane. Absolute 1.0 means the tires are touching
//! the lane line, and larger values mean the truck is leaving the lane: across
//! a line with a neighboring lane that becomes a lane change, across the
//! outside edge it becomes the shoulder or the median.
//!
//! Port of `freight_fate/sim/lane.py`.

use crate::pyrandom::PyRandom;

pub const MPH_PER_MPS: f64 = 2.23694;

pub const LANE_EDGE: f64 = 1.0;
pub const LANE_WIDTH: f64 = 2.0; // offset units from one lane center to the next
pub const CROSS_AT: f64 = 1.12; // straddling the line this far commits the lane change
pub const OFF_ROAD: f64 = 1.3;
pub const MAX_OFFSET: f64 = 1.5;
pub const RUMBLE_START: f64 = 0.85;
pub const RUMBLE_FULL: f64 = 1.15;
/// Inside this offset the truck is truthfully centered, matching
/// [`LaneKeeping::describe`].
pub const CENTERED_MAX: f64 = 0.25;
pub const OFF_ROAD_GRACE_S: f64 = 2.0;
pub const OFF_ROAD_REPEAT_S: f64 = 3.0;
pub const WANDER_RATE: f64 = 0.05;
pub const WIND_RATE: f64 = 0.10;
/// How long a held steer key takes to carry the truck a whole lane over,
/// seconds: the full-lane-keeping tap change's own 2.5 (`LANE_TAP_CHANGE_S`
/// in the game reads this).
///
/// A held key asks for a HEADING, never a turning rate (owner, 2026-09-30).
/// As a rate, a key that stayed down kept turning the truck -- two seconds at
/// forty built eleven degrees, and letting go at the capped 0.2 g took two
/// more seconds and another lane to straighten out, into the median. Asked
/// for as a heading, a hold angles the truck over at most enough to cross a
/// lane in this long, however long it is held, and letting go takes out a
/// few degrees in well under a second. A tap never gets near the angle, so it
/// is a nudge by itself.
pub const LANE_CHANGE_S: f64 = 2.5;

// -- heading ------------------------------------------------------------------
//
// The truck carries a HEADING relative to the road, and steering turns the
// truck rather than sliding it sideways. Until 2026-09-18 it had none: a bend
// pushed the offset straight over and the driver held the opposite key against
// it, which meant the wheel that tracks a left-hand bend was the RIGHT one.
// Nobody noticed while the guide only reported drift, and it became untenable
// the moment the engine started leaning the way the road turns -- following
// that cue steered the truck off the road, which is how it was found (owner's
// drive on AZ-260, 100 percent cargo damage in one bend).
//
// With a heading it comes out by itself. Leave the wheel alone in a bend and
// the road turns away beneath a truck still pointing straight, so it runs
// WIDE, to the outside, the way inertia really takes it. Hold the wheel into
// the bend and the truck tracks it. "Steer toward the lean" is then simply
// true.
//
// The model is the standard bicycle (Ackermann) one every vehicle-dynamics
// text starts from: a steer angle at the front axle turns the vehicle at
// `yaw_rate = v * tan(delta) / wheelbase`.

/// Tractor wheelbase, feet. A bicycle model uses the steering unit's own
/// wheelbase; the trailer follows it (low-speed off-tracking is modelled
/// separately by `data::corners`, not here).
pub const WHEELBASE_FT: f64 = 20.0;
/// Full lock at the front axle, radians -- about 30 degrees.
pub const MAX_STEER_RAD: f64 = 0.52;
/// The most lateral acceleration a steering input may ask for, in g.
///
/// A keyboard has no proportional control: a held arrow is full lock, and at
/// highway speed full lock is not a steering input, it is a rollover. Capping
/// the yaw rate so the resulting `v * yaw_rate` stays here gives the driver
/// the same authority a real one uses -- lots of it at yard speed, very little
/// at seventy -- without pretending the keyboard is a wheel. Set well under
/// the 0.35 g static rollover threshold a loaded combination is built to
/// (NHTSA DOT HS 811 734), because this is the routine limit, not the edge.
pub const MAX_STEER_LATERAL_G: f64 = 0.2;
/// The ceiling on the truck's TOTAL cornering, in g.
///
/// The cap above is about the keyboard, and it belongs on what the DRIVER
/// asks for. It must not bind the wheel the road itself is asking for: the
/// shipped advisories are priced at 0.30 g plus bank, so a 0.2 g ceiling on
/// everything meant that at the very number curve assistance brakes to, no
/// input -- the driver's, the assist's, nothing -- could hold the bend, and
/// the truck ran wide at its own advisory with partial lane keeping on
/// (review finding, 2026-09-19). Following the road is not a rollover risk;
/// taking a bend far above its advisory is, so the total still stops at the
/// static rollover threshold a loaded combination is built to, and past that
/// the truck understeers wide exactly as it should.
///
/// This is what the TIRES must supply, so a banked bend is credited its bank
/// (`RoadConditions::bank`) on top of it. The governing equation the whole
/// curve bake is built on is `e + f = V^2 / 15R`: the bank carries part of the
/// lateral load and only the rest reaches the tires. Comparing the total
/// against the tire limit priced the bank as if it did nothing, and the
/// shipped advisories are `0.30 g PLUS bank` -- so at the very number the cab
/// calls out, the tightest bends sat at or over a ceiling meant for the
/// rollover edge, and the truck ran wide with every assist steering (agent
/// drive, AZ-260 into Payson, 2026-09-19). Credited, the advisory leaves
/// 0.05 g of tire between the road's demand and the ceiling, which is the
/// authority lane keeping corrects a drift with. See
/// `tests/it/sim_bends_hold_their_advisory.rs`, which sweeps every signed bend
/// in the bake at its own advisory.
pub const MAX_ROAD_LATERAL_G: f64 = 0.35;
/// The most bank that may be credited, as a slope. `SUPERELEVATION_BUILT` in
/// `data::curves` -- the 6 percent both cited DOT manuals build to -- and the
/// screen that keeps a malformed row from buying cornering it never earned.
pub const MAX_CREDITED_BANK: f64 = 0.06;
/// Half a twelve-foot lane, feet: what `offset` 1.0 is worth on the ground.
pub const HALF_LANE_FT: f64 = 6.0;
/// How hard PARTIAL lane keeping steers for the driver.
///
/// Nothing recentres the heading on its own: a truck pointing five degrees
/// off the road does not rotate itself back, it keeps going off, and making
/// it self-correct was the difference between a bend you must drive and one
/// that drives itself (caught the first time this model ran -- the recentring
/// cancelled the road's own rotation and a 600-foot bend read as straight).
/// Assistance is a thing that STEERS, so it is modelled as steering: a lane
/// error and a heading error, each worth this much wheel.
pub const ASSIST_OFFSET_GAIN: f64 = 0.7;
pub const ASSIST_YAW_GAIN: f64 = 3.0;
pub const FPS_PER_MPH: f64 = 1.466_667;
pub const G_FPS2: f64 = 32.174;

/// Only the modes where the driver does the lane work have a drift model.
/// "full" is absent on purpose: it pins the offset to lane centre. The
/// multiplier scales the wander.
pub fn assist_tuning(assist: &str) -> Option<f64> {
    match assist {
        "partial" => Some(0.45),
        "off" => Some(1.0),
        _ => None,
    }
}

/// Whether this mode steers for the driver as well as damping the wander.
pub fn assist_steers(assist: &str) -> bool {
    assist == "partial"
}

/// The steer angle that exactly tracks a road of this curvature.
///
/// The bicycle model solved the other way round: `yaw_rate = v tan(d) / L`
/// has to equal the rate the road itself turns, `v * curvature`, and the
/// speed cancels -- so the angle a bend wants is a property of the BEND, not
/// of how fast you take it. That is what makes turn assistance separable from
/// lane keeping: this is feed-forward off the road's shape, where lane keeping
/// is feedback off the driver's error. Owner's ruling, 2026-09-18: they are
/// not the same assist and should not be one setting.
/// What the road under the truck is doing this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadConditions {
    /// One over the bend's radius in feet, signed: positive turns right.
    pub curvature: f64,
    /// Crosswind strength, 0 to 1.
    pub wind: f64,
    /// Tire grip, 0 to 1. Below one the truck understeers.
    pub grip: f64,
    /// The bend's superelevation, as a slope. Zero on unbanked road. Credited
    /// to the cornering ceiling, because bank the road carries is load the
    /// tires do not -- see [`MAX_ROAD_LATERAL_G`].
    pub bank: f64,
}

impl Default for RoadConditions {
    fn default() -> Self {
        RoadConditions {
            curvature: 0.0,
            wind: 0.0,
            grip: 1.0,
            bank: 0.0,
        }
    }
}

pub fn tracking_steer_rad(road_curvature: f64) -> f64 {
    (road_curvature * WHEELBASE_FT).atan()
}

pub const DEFAULT_LANE_COUNT: i64 = 2;

/// Spoken name for a lane index: right, left, or middle.
///
/// A single-lane road has no sides to name. Calling it "the right lane"
/// invites the driver to wonder what is in the left one when there is no
/// left one (Cary, 2026-08-15), so it answers to the road itself. Callers
/// that build "the {label} lane" get "the single lane"; the readouts that
/// want a bare noun use [`lane_phrase`] below.
pub fn lane_label(index: i64, count: i64) -> &'static str {
    if count <= 1 {
        return "single";
    }
    if index <= 0 {
        return "right";
    }
    if index >= count - 1 {
        return "left";
    }
    "middle"
}

/// How a readout names the lane the truck is in, article included.
///
/// "In the right lane" reads naturally; "In the single lane" does not, so
/// a one-lane road simply says "In the lane".
pub fn lane_phrase(index: i64, count: i64) -> String {
    if count <= 1 {
        return "the lane".to_string();
    }
    format!("the {} lane", lane_label(index, count))
}

/// "In the right lane", written out whole for each lane so a translation
/// matches the phrase itself rather than a short "In {...}" pattern that
/// other lines share.
pub fn in_lane_phrase(index: i64, count: i64) -> &'static str {
    if count <= 1 {
        return "In the lane";
    }
    match lane_label(index, count) {
        "right" => "In the right lane",
        "left" => "In the left lane",
        _ => "In the middle lane",
    }
}

/// Small deterministic lane simulation for audio-only steering cues.
#[derive(Debug, Clone)]
pub struct LaneKeeping {
    rng: PyRandom,
    pub offset: f64,
    /// Truck heading relative to the road's own direction, radians. Positive
    /// points right of the road. Zero means tracking it.
    pub yaw_rad: f64,
    pub steering: f64,
    /// The straighten-up key is held: steer out the heading error only,
    /// the heading half of partial lane keeping's law. It points the truck
    /// down the road and leaves where it sits in the lane to the driver.
    pub straighten: bool,
    /// The side of the signed turn in play -- a bend the road warns about,
    /// a ramp curve, a street corner, from its lead to its end: -1 left,
    /// 1 right, 0 none. The driving state sets it every frame.
    ///
    /// A hold toward it follows the road (owner, 2026-09-30). Holding into
    /// a turn is every driver's instinct, and a held key is far more wheel
    /// than a corner wants -- twice it at city speed, on top of curve
    /// assistance's own when that is on -- so the owner held right into a
    /// right turn and left the road. A hold that starts toward the turn asks
    /// for the road's own wheel instead: straight on until the turn begins,
    /// round with it, straight again after, until the key is let go.
    pub turn_in_play: f64,
    /// The side a hold is following the road toward, 0 when it is not.
    following: f64,
    pub lane: i64, // everyone starts in the right lane
    pub lane_count: i64,
    pub crossed: i64, // last update's lane change: +1 left, -1 right
    /// An exit lane is open to the right of the right lane: from the start of
    /// its taper to the gore, the right side of lane 0 is a lane line, not the
    /// road edge. The driving state opens it for an exit the truck is set to
    /// take; nothing else ever does.
    pub exit_lane_open: bool,
    /// Set for the one update in which the truck crossed into that exit lane.
    /// Not a `crossed` change: the exit lane is not a traffic lane, so nothing
    /// that answers a lane change (the mirror check above all) applies to it.
    pub entered_exit_lane: bool,
    wander: f64,
    wander_target: f64,
    wander_timer: f64,
    gust: f64,
    gust_target: f64,
    gust_timer: f64,
    off_road_timer: f64,
    event_cooldown: f64,
}

impl Default for LaneKeeping {
    fn default() -> Self {
        Self::new(None)
    }
}

impl LaneKeeping {
    /// `LaneKeeping(seed)`: `None` draws from entropy, as `random.Random()` did.
    pub fn new(seed: Option<i64>) -> Self {
        let rng = match seed {
            Some(seed) => PyRandom::new_from_i64(seed),
            None => PyRandom::new_unseeded(),
        };
        Self {
            rng,
            offset: 0.0,
            yaw_rad: 0.0,
            steering: 0.0,
            straighten: false,
            turn_in_play: 0.0,
            following: 0.0,
            lane: 0,
            lane_count: DEFAULT_LANE_COUNT,
            crossed: 0,
            exit_lane_open: false,
            entered_exit_lane: false,
            wander: 0.0,
            wander_target: 0.0,
            wander_timer: 0.0,
            gust: 0.0,
            gust_target: 0.0,
            gust_timer: 0.0,
            off_road_timer: 0.0,
            event_cooldown: 0.0,
        }
    }

    /// Whether a hold is following the road through a turn (see
    /// [`Self::turn_in_play`]).
    pub fn is_following(&self) -> bool {
        self.following != 0.0
    }

    /// The driver's wheel this frame: a steer key is the whole of it, a
    /// stick as far as it is pushed. Keys win over the stick, and both keys
    /// at once are no key.
    pub fn steer_input(&mut self, key_dir: i8, stick: f64) {
        self.steering = if key_dir != 0 {
            f64::from(key_dir.signum())
        } else {
            stick.clamp(-1.0, 1.0)
        };
    }

    pub fn lane_name(&self) -> &'static str {
        lane_label(self.lane, self.lane_count)
    }

    pub fn set_lane_count(&mut self, count: i64) {
        self.lane_count = count.max(1);
        self.lane = self.lane.min(self.lane_count - 1);
    }

    /// Put the truck squarely in the middle of a lane, pointing along it.
    ///
    /// For the places the GAME moves the truck rather than the driver: onto a
    /// ramp at the gore, out of a lane the road just closed, across to the
    /// open lane after the barrels, onto the highway off a departure chain.
    /// Every one of those used to write `offset = 0.0` and nothing else,
    /// which was a complete reset while the model was a position. It is a
    /// heading now, and the heading is what carries the truck across its
    /// lane -- so a truck "put in the middle" of a single-lane ramp still
    /// pointing off the mainline was over the edge within a second, from a
    /// move it did not make itself (review finding, 2026-09-19).
    pub fn recentre(&mut self, lane: i64) {
        self.lane = lane.clamp(0, (self.lane_count - 1).max(0));
        self.offset = 0.0;
        self.yaw_rad = 0.0;
        self.off_road_timer = 0.0;
    }

    /// How far past center toward a road *edge* -- a side with no
    /// neighboring lane. Drifting toward another lane never rumbles; the
    /// rumble strip lives on the shoulder and the median.
    fn edge_excursion_inner(&self) -> f64 {
        if self.offset > 0.0 && self.lane == 0 && !self.exit_lane_open {
            return self.offset;
        }
        if self.offset < 0.0 && self.lane >= self.lane_count - 1 {
            return -self.offset;
        }
        0.0
    }

    /// Advance the lane model.
    ///
    /// `road_curvature` is the road's own bend at the truck, as 1 over its
    /// radius in feet, signed: positive turns right. Zero is straight.
    ///
    /// Returns true when the truck has been off the road edge long enough to
    /// fire a warning/damage event. A completed drift across an interior lane
    /// line is reported through `crossed` (+1 moved left, -1 moved right)
    /// for the frame it happens. `assist == "full"` is lane keeping doing the
    /// whole job: the truck stays centered and no drift accrues. The discrete
    /// `lane` is still honored there, driven by tap-to-change controls.
    pub fn update(
        &mut self,
        dt: f64,
        speed_mps: f64,
        road: RoadConditions,
        assist: &str,
        turn_assist: bool,
    ) -> bool {
        let RoadConditions {
            curvature: road_curvature,
            wind,
            grip,
            bank,
        } = road;
        self.crossed = 0;
        self.entered_exit_lane = false;
        let Some(drift_mult) = assist_tuning(assist) else {
            self.offset = 0.0;
            self.yaw_rad = 0.0;
            self.off_road_timer = 0.0;
            return false;
        };

        let mph = speed_mps * MPH_PER_MPS;
        if mph < 2.0 {
            self.off_road_timer = 0.0;
            return false;
        }
        let fps = mph * FPS_PER_MPH;

        self.wander_timer -= dt;
        if self.wander_timer <= 0.0 {
            self.wander_timer = self.rng.uniform(10.0, 25.0);
            self.wander_target = self.rng.uniform(-1.0, 1.0) * WANDER_RATE;
        }
        self.wander += (self.wander_target - self.wander) * (dt / 3.0).min(1.0);

        self.gust_timer -= dt;
        if self.gust_timer <= 0.0 {
            self.gust_timer = self.rng.uniform(3.0, 8.0);
            self.gust_target = self.rng.uniform(-1.0, 1.0);
        }
        self.gust += (self.gust_target - self.gust) * (dt / 1.5).min(1.0);

        // What the driver asked the front axle for, plus whatever partial
        // lane keeping is contributing, capped so a held key is a steering
        // input rather than a rollover (see MAX_STEER_LATERAL_G).
        // Straighten first: held, it squares the truck with the road and
        // leaves the lane position alone, on partial as the help promises.
        // Checked after partial's own help, it added nothing there and the
        // truck was still pulled to the centre (2026-09-28).
        //
        // And letting go straightens (owner, 2026-09-30). A heading the
        // driver cannot see outlived every key: a tap left a drift, a hold
        // into a turn kept turning, and unwinding after a corner crossed into
        // the next lane -- three drives, three trips off the road. With no
        // key held the truck squares itself with the road, as the
        // straighten-up key does; where it sits in the lane stays the
        // driver's, and on partial the lane keeping's own help does the same
        // and more.
        //
        // A hold toward the turn in play follows the road until it is let go
        // (see `turn_in_play`); turning the other way is a new input. Following
        // the road keeps the lane as well, with partial lane keeping's law:
        // the hold owns the wheel, and at street speed a turn's lead is the
        // better part of a minute, long enough for the wander to walk an
        // unanswerable truck off its lane.
        if self.steering == 0.0 || self.steering * self.following < 0.0 {
            self.following = 0.0;
        }
        if self.following == 0.0 && self.steering * self.turn_in_play > 0.0 {
            self.following = self.turn_in_play.signum();
        }
        // The driver's own steer asks for a heading (see LANE_CHANGE_S): as
        // far as a stick is pushed, the whole of it for a key, the heading
        // that crosses a lane in that long at this speed.
        let lane_change_heading = (LANE_WIDTH * HALF_LANE_FT / LANE_CHANGE_S / fps)
            .min(1.0)
            .asin();
        let helper = if self.straighten {
            -self.yaw_rad * ASSIST_YAW_GAIN
        } else if self.following != 0.0 {
            -(self.offset * ASSIST_OFFSET_GAIN + self.yaw_rad * ASSIST_YAW_GAIN)
        } else if self.steering != 0.0 {
            (self.steering * lane_change_heading - self.yaw_rad) * ASSIST_YAW_GAIN
        } else if assist_steers(assist) {
            -(self.offset * ASSIST_OFFSET_GAIN + self.yaw_rad * ASSIST_YAW_GAIN)
        } else {
            // Let go: a heading of nothing, squared up.
            -self.yaw_rad * ASSIST_YAW_GAIN
        };
        // Turn assistance: the wheel the BEND wants, handed over whether or
        // not anything is helping with the driver's own error. A following
        // hold, key or stick, hands it over too; with assistance on it was
        // already there, so the hold adds nothing on top.
        let tracking_yaw = if turn_assist || self.following != 0.0 {
            fps * tracking_steer_rad(road_curvature).tan() / WHEELBASE_FT
        } else {
            0.0
        };
        let commanded = helper.clamp(-1.0, 1.0) * MAX_STEER_RAD;
        let rate_at = |g: f64| {
            if fps > 1.0 {
                g * G_FPS2 / fps
            } else {
                f64::MAX
            }
        };
        // The driver's own input is capped; the wheel the ROAD is asking for
        // is not, or a bend could not be held at the speed its own advisory
        // names. Both together still stop at the rollover ceiling.
        let driver_cap = rate_at(MAX_STEER_LATERAL_G);
        let mut yaw_rate =
            (fps * commanded.tan() / WHEELBASE_FT).clamp(-driver_cap, driver_cap) + tracking_yaw;
        // The tire limit plus whatever the road's bank carries for the truck.
        let road_cap = rate_at(MAX_ROAD_LATERAL_G + bank.clamp(0.0, MAX_CREDITED_BANK));
        yaw_rate = yaw_rate.clamp(-road_cap, road_cap);
        // Understeer: tires that cannot hold the road do not turn the truck as
        // far as the wheel asked, so a bend taken on ice runs wide even with
        // the wheel into it. This is where load and grip live now.
        yaw_rate *= grip.clamp(0.0, 1.0);

        // The road turns underneath. Holding the wheel still in a bend leaves
        // the truck pointing where it was, so the RELATIVE heading opens up
        // and it runs wide -- which is the whole point of carrying a heading.
        let road_yaw_rate = fps * road_curvature;
        self.yaw_rad += (yaw_rate - road_yaw_rate) * dt;
        self.yaw_rad = self.yaw_rad.clamp(-0.6, 0.6);

        // Wander and crosswind are a small standing HEADING error, not a
        // shove on the position and not a rate: integrating them as a yaw
        // rate turned WANDER_RATE's 0.05 into 0.05 RADIANS PER SECOND, which
        // is half a radian of heading in ten seconds and put the truck in the
        // median on every approach the first time this ran. Sized so the
        // lateral drift they produce is the one the old model produced --
        // `offset` moves at `sin(yaw) * fps / HALF_LANE_FT`, so the yaw worth
        // a given drift rate is that rate times HALF_LANE_FT over speed.
        let drift_rate = (self.wander + wind * self.gust * WIND_RATE) * drift_mult;
        let disturbance_yaw = drift_rate * HALF_LANE_FT / fps.max(1.0);

        // Heading is what moves the truck across its lane.
        self.offset += (self.yaw_rad + disturbance_yaw).sin() * fps * dt / HALF_LANE_FT;

        // Straddle an interior line far enough and the truck is in the next
        // lane over: re-center the offset relative to the new lane so the
        // player finishes the change by straightening out.
        if self.offset <= -CROSS_AT && self.lane < self.lane_count - 1 {
            self.lane += 1;
            self.offset += LANE_WIDTH;
            self.crossed = 1;
        } else if self.offset >= CROSS_AT && self.lane > 0 {
            self.lane -= 1;
            self.offset -= LANE_WIDTH;
            self.crossed = -1;
        } else if self.offset >= CROSS_AT && self.exit_lane_open {
            // Across the line into the exit lane. Its own right side is the
            // ramp's edge, so the lane it opened is the truck's lane now.
            self.offset -= LANE_WIDTH;
            self.exit_lane_open = false;
            self.entered_exit_lane = true;
        }
        self.offset = self.offset.clamp(-MAX_OFFSET, MAX_OFFSET);

        self.event_cooldown = (self.event_cooldown - dt).max(0.0);
        if self.edge_excursion_inner() >= OFF_ROAD {
            self.off_road_timer += dt;
            if self.off_road_timer >= OFF_ROAD_GRACE_S && self.event_cooldown <= 0.0 {
                self.event_cooldown = OFF_ROAD_REPEAT_S;
                return true;
            }
        } else {
            self.off_road_timer = 0.0;
        }
        false
    }

    /// 0..1 rumble-strip cue level at the road edge (shoulder or median).
    pub fn rumble_level(&self) -> f64 {
        ((self.edge_excursion_inner() - RUMBLE_START) / (RUMBLE_FULL - RUMBLE_START))
            .clamp(0.0, 1.0)
    }

    /// Public read of how far past center toward a true road edge.
    pub fn edge_excursion(&self) -> f64 {
        self.edge_excursion_inner()
    }

    pub fn describe(&self) -> String {
        let lane_part = in_lane_phrase(self.lane, self.lane_count);
        let side = if self.offset < 0.0 { "left" } else { "right" };
        let away = self.offset.abs();
        if away < CENTERED_MAX {
            return format!("{lane_part}, centered.");
        }
        if away < 0.7 {
            return format!("{lane_part}, drifting {side}.");
        }
        if self.edge_excursion_inner() >= OFF_ROAD {
            return format!("Off the road on the {side}!");
        }
        if away < CROSS_AT {
            return format!("{lane_part}, at the {side} edge of the lane.");
        }
        format!("{lane_part}, crossing the {side} lane line.")
    }
}

#[cfg(test)]
mod tests {
    //! Ported from `tests/test_lane_keeping.py` (the Settings-backed cases
    //! belong to the settings port) and the `LaneKeeping` section of
    //! `tests/test_lane_discrete.py` (the DrivingState cases belong to the
    //! app-shell bucket).
    use super::*;

    /// A left-hand bend of `radius_ft`, as the model now takes it.
    fn left_bend(radius_ft: f64) -> f64 {
        -1.0 / radius_ft
    }

    fn run_lane(lane: &mut LaneKeeping, seconds: f64, curve: f64, wind: f64, assist: &str) -> i64 {
        let dt = 0.1;
        let mut events = 0;
        for _ in 0..((seconds / dt) as i64) {
            if lane.update(
                dt,
                29.0,
                RoadConditions {
                    curvature: curve,
                    wind,
                    grip: 1.0,
                    bank: 0.0,
                },
                assist,
                false,
            ) {
                events += 1;
            }
        }
        events
    }

    #[test]
    fn an_open_exit_lane_is_a_lane_to_the_right_not_the_shoulder() {
        // Held right from the right lane: with the exit lane open the truck
        // crosses into it and never touches a rumble strip; with it closed the
        // same steer is the shoulder.
        for open in [true, false] {
            let mut lane = LaneKeeping::new(Some(11));
            lane.exit_lane_open = open;
            lane.steering = 1.0;
            let mut entered = false;
            let mut rumble: f64 = 0.0;
            for _ in 0..80 {
                lane.update(0.05, 29.0, RoadConditions::default(), "partial", false);
                entered |= lane.entered_exit_lane;
                if !entered {
                    rumble = rumble.max(lane.rumble_level());
                }
            }
            assert_eq!(entered, open);
            assert_eq!(lane.lane, 0);
            assert_eq!(lane.crossed, 0);
            if open {
                assert_eq!(rumble, 0.0);
                assert!(!lane.exit_lane_open, "entering the lane closes it");
            } else {
                assert!(rumble > 0.0);
            }
        }
    }

    // -- heading -------------------------------------------------------------

    #[test]
    fn a_bend_taken_without_steering_runs_the_truck_wide() {
        // The whole reason heading exists. Hands off in a LEFT-hander, the
        // road turns away from a truck still pointing straight, so it ends up
        // to the RIGHT -- the outside. Inertia, not a shove.
        let mut lane = LaneKeeping::new(Some(11));
        run_lane(&mut lane, 6.0, left_bend(600.0), 0.0, "off");
        assert!(
            lane.offset > 0.2,
            "hands off in a left bend must run wide right; got {}",
            lane.offset
        );
        assert!(lane.yaw_rad > 0.0, "the truck should be pointing wide");
    }

    #[test]
    fn steering_into_a_bend_tracks_it() {
        // Holding the wheel INTO the bend keeps the lane, which is what makes
        // "steer toward the lean" a true instruction. Before heading, holding
        // left here slid the truck off the left EDGE (owner's AZ-260 drive,
        // 2026-09-18, one destroyed load).
        //
        // Closed loop, because that is what driving is: the input answers the
        // error rather than being held open at some guessed angle. A bend of
        // this radius only wants a few percent of lock to track -- steering
        // the whole wheel into it would cut the corner just as surely as not
        // steering runs wide.
        // At a speed the bend can actually be taken at. A 600-foot radius
        // wants 0.47 g at 65 mph, past the rollover threshold, so
        // MAX_STEER_LATERAL_G correctly refuses to turn that hard and the
        // truck runs wide however the wheel is held -- the model saying, in
        // its own terms, that the advisory exists for a reason.
        //
        // The input is a share of the driver's 0.2 g (2026-09-30), and this
        // bend at forty wants 0.18 g of it, so the driver answering it holds
        // nearly the whole wheel: the gains are sized so that hold comes from
        // a lane error well inside the lines.
        let mut lane = LaneKeeping::new(Some(11));
        let dt = 0.05;
        let forty_mph = 40.0 / MPH_PER_MPS;
        for _ in 0..300 {
            lane.steering = (-(lane.offset * 2.0 + lane.yaw_rad * 48.0)).clamp(-1.0, 1.0);
            lane.update(
                dt,
                forty_mph,
                RoadConditions {
                    curvature: left_bend(600.0),
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "off",
                false,
            );
        }
        assert!(
            lane.offset.abs() < LANE_EDGE,
            "a driver answering the bend should stay in the lane; offset {}",
            lane.offset
        );
    }

    #[test]
    fn a_bend_can_be_held_at_its_own_advisory() {
        // The number curve assistance brakes to has to be a number the truck
        // can hold. Advisories are priced at 0.30 g plus bank; the keyboard
        // cap is 0.20 g, and while it bound the wheel the ROAD asks for as
        // well, a 600-foot bend at its own 50 mph advisory ran the truck out
        // of the lane whatever the driver or the assist did -- on partial
        // lane keeping, which is what a fresh install now ships (review
        // finding, 2026-09-19).
        //
        // Turn assistance on, hands off the wheel: the bend is the assist's
        // to hold, and holding it is the whole promise of the settings row.
        let mut lane = LaneKeeping::new(Some(11));
        let dt = 0.05;
        let fifty_mph = 50.0 / MPH_PER_MPS;
        for _ in 0..400 {
            lane.update(
                dt,
                fifty_mph,
                RoadConditions {
                    curvature: left_bend(600.0),
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "partial",
                true,
            );
        }

        assert!(
            lane.offset.abs() < LANE_EDGE,
            "the bend could not be held at its own advisory; offset {}",
            lane.offset
        );
    }

    #[test]
    fn a_banked_hairpin_can_be_held_at_its_own_advisory() {
        // AZ-260, mile 37.9: 146 feet of radius, advisory 30. Priced at
        // 0.30 g plus the road's 6 percent of bank, so at 30 the bend asks
        // 0.36 g of the road and 0.30 of the TIRES -- and comparing the whole
        // 0.36 against a tire limit of 0.35 ran the truck out of its lane at
        // the speed the cab had just called out, with every assist steering
        // (agent drive, 2026-09-19).
        let hairpin = |bank: f64| {
            let mut lane = LaneKeeping::new(Some(11));
            let dt = 0.05;
            let thirty_mph = 30.0 / MPH_PER_MPS;
            // 120 degrees of a 146-foot arc at 44 feet per second: 7 seconds.
            for _ in 0..140 {
                lane.update(
                    dt,
                    thirty_mph,
                    RoadConditions {
                        curvature: left_bend(146.0),
                        wind: 0.0,
                        grip: 1.0,
                        bank,
                    },
                    "partial",
                    true,
                );
            }
            lane.offset
        };

        let banked = hairpin(0.06);
        assert!(
            banked.abs() < LANE_EDGE,
            "the hairpin could not be held at its own advisory; offset {banked}"
        );
        // Lay the same bend flat and it is genuinely unholdable at 30, which
        // is the model saying the bank is doing the work rather than a fudge
        // factor: an unbanked 146-foot curve is not a 30 mph curve.
        let flat = hairpin(0.0);
        assert!(
            flat > LANE_EDGE,
            "expected an unbanked hairpin to run wide at 30; offset {flat}"
        );
    }

    #[test]
    fn far_above_the_advisory_the_truck_still_runs_wide() {
        // The other half of the same rule: following the road is exempt from
        // the driver's cap, not from physics. A 600-foot bend at 75 wants
        // 0.63 g, well past what a loaded combination will hold, so the truck
        // understeers wide however hard anything steers -- which is the model
        // saying in its own terms that the advisory exists for a reason.
        let mut lane = LaneKeeping::new(Some(11));
        let dt = 0.05;
        let seventy_five_mph = 75.0 / MPH_PER_MPS;
        for _ in 0..200 {
            lane.steering = -1.0; // full lock into the bend
            lane.update(
                dt,
                seventy_five_mph,
                RoadConditions {
                    curvature: left_bend(600.0),
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "partial",
                true,
            );
        }

        assert!(
            lane.offset > LANE_EDGE,
            "a bend taken far over its advisory must run wide; offset {}",
            lane.offset
        );
    }

    #[test]
    fn recentring_the_truck_points_it_along_the_lane() {
        // The game moves the truck itself at a gore, a lane closure and a
        // merge. Offset alone was a complete reset while the model was a
        // position; it is a heading now, and a truck "put in the middle"
        // still pointing off the road leaves the lane it was just placed in.
        let mut lane = LaneKeeping::new(Some(11));
        lane.set_lane_count(2);
        lane.steering = -1.0;
        for _ in 0..40 {
            lane.update(
                0.05,
                29.0,
                RoadConditions {
                    curvature: 0.0,
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "off",
                false,
            );
        }
        assert!(
            lane.yaw_rad.abs() > 0.05,
            "the truck should be pointing off"
        );

        lane.recentre(0);
        assert_eq!(lane.lane, 0);
        assert_eq!(lane.offset, 0.0);
        assert_eq!(lane.yaw_rad, 0.0);

        // And it stays put with nobody steering.
        lane.steering = 0.0;
        for _ in 0..40 {
            lane.update(
                0.05,
                29.0,
                RoadConditions {
                    curvature: 0.0,
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "off",
                false,
            );
        }
        assert!(
            lane.offset.abs() < 0.2,
            "a recentred truck drifted straight back out; offset {}",
            lane.offset
        );
    }

    #[test]
    fn steering_the_wrong_way_in_a_bend_leaves_the_road_faster() {
        let dt = 0.05;
        let mut hands_off = LaneKeeping::new(Some(11));
        for _ in 0..8 {
            hands_off.update(
                dt,
                29.0,
                RoadConditions {
                    curvature: left_bend(600.0),
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "off",
                false,
            );
        }
        let mut wrong = LaneKeeping::new(Some(11));
        for _ in 0..8 {
            wrong.steering = 0.2; // right, out of a left-hander
            wrong.update(
                dt,
                29.0,
                RoadConditions {
                    curvature: left_bend(600.0),
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "off",
                false,
            );
        }
        assert!(
            wrong.offset > hands_off.offset,
            "steering out of the bend must go wider than hands off: {} vs {}",
            wrong.offset,
            hands_off.offset
        );
    }

    #[test]
    fn a_tighter_bend_pulls_wide_faster_than_a_gentle_one() {
        // Curvature is the road's own geometry, so this falls out rather than
        // being a severity number somebody chose. Sampled early, because at
        // highway speed BOTH bends have the truck off the road in a couple of
        // seconds if nobody steers -- which is itself the point.
        let dt = 0.05;
        let mut gentle = LaneKeeping::new(Some(5));
        let mut tight = LaneKeeping::new(Some(5));
        for _ in 0..8 {
            gentle.update(
                dt,
                29.0,
                RoadConditions {
                    curvature: left_bend(2000.0),
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "off",
                false,
            );
            tight.update(
                dt,
                29.0,
                RoadConditions {
                    curvature: left_bend(400.0),
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "off",
                false,
            );
        }
        assert!(
            tight.offset > gentle.offset,
            "tight {} should be wider than gentle {}",
            tight.offset,
            gentle.offset
        );
    }

    #[test]
    fn ice_understeers_so_the_wheel_buys_less_turn() {
        let mut dry = LaneKeeping::new(Some(3));
        let mut icy = LaneKeeping::new(Some(3));
        let dt = 0.1;
        for _ in 0..40 {
            dry.steering = -0.5;
            icy.steering = -0.5;
            dry.update(
                dt,
                29.0,
                RoadConditions {
                    curvature: 0.0,
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "off",
                false,
            );
            icy.update(
                dt,
                29.0,
                RoadConditions {
                    curvature: 0.0,
                    wind: 0.0,
                    grip: 0.25,
                    bank: 0.0,
                },
                "off",
                false,
            );
        }
        assert!(
            dry.offset < icy.offset,
            "dry should have turned further left than ice: {} vs {}",
            dry.offset,
            icy.offset
        );
    }

    #[test]
    fn full_lane_keeping_still_pins_the_truck_and_its_heading() {
        let mut lane = LaneKeeping::new(Some(1));
        lane.offset = 0.9;
        lane.yaw_rad = 0.3;
        run_lane(&mut lane, 5.0, left_bend(400.0), 1.0, "full");
        assert_eq!(lane.offset, 0.0);
        assert_eq!(lane.yaw_rad, 0.0);
    }

    #[test]
    fn test_full_lane_keeping_preserves_centered_lane() {
        let mut lane = LaneKeeping::new(Some(1));
        lane.offset = 0.9;
        assert_eq!(run_lane(&mut lane, 30.0, 1.0, 1.0, "full"), 0);
        assert_eq!(lane.offset, 0.0);
    }

    #[test]
    fn test_drift_and_steering_correction() {
        // Ported from the old drift model, which shoved the POSITION. A
        // heading is what carries the truck off line now, and since letting go
        // straightens (2026-09-30) only a key still held keeps one: a light
        // steer held a moment too long.
        let dt = 0.05;
        let mut lane = LaneKeeping::new(Some(7));
        for _ in 0..40 {
            lane.steering = 0.3;
            lane.update(
                dt,
                29.0,
                RoadConditions {
                    curvature: 0.0,
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "off",
                false,
            );
        }
        let wandered = lane.offset.abs();
        assert!(
            wandered > 0.4,
            "the heading should have carried it off line"
        );

        // Let go and the truck squares with the road: the drift stops where
        // it is, and putting it back in the lane is still the driver's.
        lane.steering = 0.0;
        for _ in 0..40 {
            lane.update(dt, 29.0, RoadConditions::default(), "off", false);
        }
        assert!(lane.yaw_rad.abs() < 0.002, "still angled: {}", lane.yaw_rad);
        let settled = lane.offset.abs();
        for _ in 0..40 {
            lane.update(dt, 29.0, RoadConditions::default(), "off", false);
        }
        assert!(
            (lane.offset.abs() - settled).abs() < 0.15,
            "kept drifting after letting go: {settled} to {}",
            lane.offset.abs()
        );
        let wandered = lane.offset.abs();

        // Answering it with the wheel brings it back. The input asks for a
        // heading now (2026-09-30), so steering back by how far off centre
        // the truck is does it without chasing its own overshoot.
        for _ in 0..600 {
            lane.steering = (-lane.offset).clamp(-1.0, 1.0);
            lane.update(
                dt,
                29.0,
                RoadConditions {
                    curvature: 0.0,
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "off",
                false,
            );
        }
        assert!(
            lane.offset.abs() < wandered * 0.5,
            "steering back should recover most of it: {} from {wandered}",
            lane.offset
        );
    }

    #[test]
    fn test_off_road_event_repeats_after_grace() {
        let mut lane = LaneKeeping::new(Some(1));
        let fired = run_lane(&mut lane, 40.0, 1.0, 0.0, "off");
        assert!(lane.offset.abs() >= OFF_ROAD);
        assert!(lane.offset.abs() <= MAX_OFFSET);
        assert!(fired >= 2);
    }

    // -- LaneKeeping: the discrete layer under the drift model (test_lane_discrete.py)

    #[test]
    fn test_lane_labels() {
        assert_eq!(lane_label(0, 2), "right");
        assert_eq!(lane_label(1, 2), "left");
        assert_eq!(lane_label(1, 3), "middle");
        assert_eq!(lane_label(2, 3), "left");
    }

    #[test]
    fn test_steering_across_the_line_changes_lanes() {
        let mut lane = LaneKeeping::new(Some(3));
        lane.steering = -1.0; // hold left
        let mut crossed = 0;
        for _ in 0..200 {
            lane.update(
                0.1,
                29.0,
                RoadConditions {
                    curvature: 0.0,
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "off",
                false,
            );
            if lane.crossed != 0 {
                crossed = lane.crossed;
                break;
            }
        }
        assert_eq!(crossed, 1);
        assert_eq!(lane.lane, 1);
        // Entered the new lane from its right side, still drifting across it.
        assert!(lane.offset > 0.0);
    }

    #[test]
    fn test_no_lane_to_the_left_means_the_median() {
        let mut lane = LaneKeeping::new(Some(3));
        lane.lane = 1; // already in the left lane
        lane.steering = -1.0;
        let mut fired = false;
        for _ in 0..400 {
            if lane.update(
                0.1,
                29.0,
                RoadConditions {
                    curvature: 0.0,
                    wind: 0.0,
                    grip: 1.0,
                    bank: 0.0,
                },
                "off",
                false,
            ) {
                fired = true;
                break;
            }
        }
        assert!(fired); // off-road event, not a lane change
        assert_eq!(lane.lane, 1);
        assert_eq!(lane.crossed, 0);
    }

    #[test]
    fn test_interior_lane_line_does_not_rumble() {
        let mut lane = LaneKeeping::new(Some(1));
        lane.offset = -1.0; // straddling the line toward the left lane
        assert_eq!(lane.rumble_level(), 0.0);
        lane.offset = 1.0; // drifting onto the shoulder
        assert!(lane.rumble_level() > 0.0);
        assert!(lane_label(1, 2).contains("left"));
    }

    #[test]
    fn test_describe_names_the_lane() {
        let mut lane = LaneKeeping::new(Some(1));
        assert_eq!(lane.describe(), "In the right lane, centered.");
        lane.lane = 1;
        lane.offset = -0.5;
        assert_eq!(lane.describe(), "In the left lane, drifting left.");
    }

    #[test]
    fn test_a_single_lane_road_has_no_side_to_name() {
        // "The right lane" on a one-lane road invites the driver to wonder what
        // is in the left one, when there is no left one (Cary, 2026-08-15).
        let mut lane = LaneKeeping::new(Some(1));
        lane.set_lane_count(1);
        lane.offset = 0.0;
        assert_eq!(lane.describe(), "In the lane, centered.");
        lane.offset = -0.5;
        assert_eq!(lane.describe(), "In the lane, drifting left.");
        assert_eq!(lane_phrase(0, 1), "the lane");
        // Two lanes and up still name the side, which is the whole point there.
        assert_eq!(lane_phrase(0, 2), "the right lane");
        for (index, count) in [(0, 1), (0, 2), (1, 2), (1, 3)] {
            assert_eq!(
                in_lane_phrase(index, count),
                format!("In {}", lane_phrase(index, count))
            );
        }
    }

    #[test]
    fn test_set_lane_count_clamps_the_lane() {
        let mut lane = LaneKeeping::new(Some(1));
        lane.lane = 1;
        lane.set_lane_count(1);
        assert_eq!(lane.lane, 0);
        const { assert!(CROSS_AT > 1.0) }; // crossing requires actually straddling the line
    }

    #[test]
    fn test_describe_reads_edges_and_crossings() {
        let mut lane = LaneKeeping::new(Some(1));
        lane.offset = 0.9;
        assert_eq!(
            lane.describe(),
            "In the right lane, at the right edge of the lane."
        );
        lane.offset = 1.4;
        assert_eq!(lane.describe(), "Off the road on the right!");
        lane.offset = -1.2;
        assert_eq!(
            lane.describe(),
            "In the right lane, crossing the left lane line."
        );
    }
}
