//! The mirror check a blind driver cannot make: when the lane you left is
//! open again (port of `freight_fate/states/driving_lane_gap.py`, the
//! `LaneGapMixin`).
//!
//! Passing is a two-part manoeuvre and the game only ever spoke the first
//! part. Moving over says "Changing to the left lane" and arriving says "In
//! the left lane", and from there the driver is on their own: the vehicle they
//! are passing makes no sound once it is beside the cab, and nothing says when
//! it is behind them. A sighted player glances at the mirror. A blind player
//! was left guessing, and guessing wrong is a sideswipe -- the collision
//! message even tells them to check mirrors they have no way to check (tester
//! Darren Duff, 1.9.0.dev0: "Not sure how you are supposed to know once you
//! are past vehicles and it's safe to switch lanes again").
//!
//! So the pass gets its closing half: one line, when the lane behind the
//! manoeuvre is genuinely open again.
//!
//! * **The same authority that judges the collision.** `_finish_lane_change`
//!   asks the traffic manager whether the lane it arrived in is occupied. This
//!   asks the same question of the lane the driver came from, through the same
//!   `vehicle_in_lane` call, over a window a margin WIDER than the collision
//!   test uses -- and swept forward over the real seconds a driver spends
//!   acting on the answer. A static margin was not enough: traffic moves on
//!   compressed game time, so at highway compression a cruise closing on
//!   slowed traffic ate half a mile between "Right lane open" and the drift
//!   landing, and the lane the cue had honestly called open sideswiped anyway
//!   (tester Jerry Jicha, 1.9.0.dev0: "if it says it is open, then it is
//!   open"). Open now means open until the driver is across.
//! * **Once per vehicle passed.** Latched on the vehicle's key, like the
//!   pass-by whoosh. A car riding the edge of the window must not chant.
//! * **Only while it is still true.** Back in that lane, out of it by a fresh
//!   change, stopped, or the road took the lane away, and the watch is dropped
//!   without a word. A closed lane is never called open.
//!
//! The same clearance reading answers the L key on demand, so a driver who
//! missed the line -- or who wants to know before committing -- can ask.

use ff_core::sim::traffic_manager::TrafficVehicle;
use ff_core::sim::trip_models::{OpenSide, RoadStop, TripEvent};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_updates::lane_count_words;

/// How far under a vehicle pacing it in the right lane lane keeping eases
/// the truck, so the vehicle pulls ahead and the truck drops in behind it.
const PASS_RETURN_EASE_MPH: f64 = 10.0;

/// How long the left lane stays excused after lane keeping's move back was
/// called off or held by traffic.
pub const KEEP_RIGHT_EXCUSE_S: f64 = 30.0;

/// The time gap the move back keeps to a vehicle in the lane it moves into:
/// to the one ahead at the truck's speed, and from the one behind at its own.
const DROP_IN_HEADWAY_S: f64 = 3.0;

/// The rig, cab to trailer tail. The truck's position is its front bumper.
const RIG_LENGTH_MI: f64 = 72.0 / 5280.0;
/// Room past a thing gone around before the move back starts: the length of
/// the move itself is already in the speed, so this is the thing's own
/// length and a little air (a car on the shoulder, a closure's taper).
const PASS_CLEAR_MARGIN_MI: f64 = 200.0 / 5280.0;
/// A sudden lane closure runs on past where it was seen: its cones, then a
/// work area. A short closure's length, kept clear of before moving back.
const CLOSURE_CLEAR_MI: f64 = 0.5;

/// A traffic vehicle's real length, by kind. `TrafficVehicle::length_mi` is
/// a quarter mile for every vehicle, a deliberately wide footprint the
/// collision and clearance checks share; a move back that keeps a real
/// headway reads the vehicle at its own size instead.
fn real_length_mi(vehicle_class: &str) -> f64 {
    let feet = match vehicle_class {
        "car" => 15.0,
        "state trooper" => 17.0,
        "box truck" => 26.0,
        "semi" => 72.0,
        _ => 20.0,
    };
    feet / 5280.0
}

/// The margin and the forward sweep live with the trip now
/// (`Trip::lane_blocker_at`), because the hazard call that names the open
/// side reads the same clearance this cue and the L key do.
pub use ff_core::sim::traffic_manager::{LANE_GAP_ACT_REAL_S, LANE_GAP_MARGIN_MI};
/// Real seconds between two of these lines. A queue of vehicles clearing one
/// after another is a real sequence of facts, but read out back to back it is
/// a chant over the top of whatever else the road is saying.
pub const LANE_GAP_CUE_MIN_GAP_S: f64 = 4.0;

impl DrivingState {
    // -- clearance, read through the collision logic's own authority ----------

    /// The vehicle holding `lane_index` beside the truck, or None.
    ///
    /// The window is the sideswipe test's, widened by `LANE_GAP_MARGIN_MI` at
    /// both ends and swept `LANE_GAP_ACT_REAL_S` real seconds forward through
    /// the traffic's relative motion: whatever this call misses, the collision
    /// check misses too, so "open" can never be the more optimistic of the two
    /// answers -- not now, and not by the time the driver acting on it arrives
    /// in the lane. The reading is the trip's, shared with the hazard call
    /// that names the open side, so the two can never disagree.
    pub fn lane_gap_blocker(&self, lane_index: i64) -> Option<TrafficVehicle> {
        self.trip.lane_blocker_at(None, lane_index).cloned()
    }

    /// Whether roadwork has this lane coned off where the truck is.
    pub fn lane_gap_closed_by_zone(&mut self, lane_index: i64) -> bool {
        let zone = self.trip.active_zone();
        zone.is_some_and(|zone| {
            zone.reason == "construction" && zone.closed_lane == Some(lane_index)
        })
    }

    /// Whether moving into `lane_index` right now is clear of traffic.
    pub fn lane_gap_open(&self, lane_index: i64) -> bool {
        self.lane_gap_blocker(lane_index).is_none()
    }

    // -- the spoken cue -------------------------------------------------------

    pub fn arm_lane_gap_watch(&mut self, lane_index: i64) {
        self.lane_gap_watch = Some(lane_index);
        self.lane_gap_blocker_key = None;
        self.lane_gap_blocker_class = String::new();
        // A fresh manoeuvre is owed a fresh answer, even about a vehicle an
        // earlier one already called: the driver is passing it again.
        self.lane_gap_said_keys.clear();
    }

    pub fn drop_lane_gap_watch(&mut self) {
        self.lane_gap_watch = None;
        self.lane_gap_blocker_key = None;
        self.lane_gap_blocker_class = String::new();
        self.lane_gap_said_keys.clear();
    }

    /// Watch the lane the truck moved out of, and say when it is open.
    pub fn update_lane_gap(&mut self, ctx: &mut GameContext, dt: f64) {
        let current = self.lane.lane;
        let previous = self.lane_gap_prev_lane;
        self.lane_gap_prev_lane = Some(current);
        self.lane_gap_cue_s = (self.lane_gap_cue_s - dt).max(0.0);
        if let Some(previous) = previous {
            if previous != current {
                // The truck just changed lanes: the lane it left is the way back.
                self.arm_lane_gap_watch(previous);
            }
        }
        let Some(watch) = self.lane_gap_watch else {
            return;
        };
        if watch == current || watch >= self.lane.lane_count || watch < 0 {
            // Already back in it, or the road no longer has it.
            self.drop_lane_gap_watch();
            return;
        }
        if self.lane_gap_closed_by_zone(watch) {
            // Never call a coned-off lane open. The merge warning owns this
            // stretch of road and it is telling the driver the opposite.
            self.drop_lane_gap_watch();
            return;
        }
        if self.trip.truck.speed_mph() <= LANE_MIN_MPH {
            return; // nothing is being passed at a crawl; keep watching
        }
        if self.lane_change_target.is_some() {
            return; // a change is already underway, in some direction
        }
        if let Some(blocker) = self.lane_gap_blocker(watch) {
            self.lane_gap_blocker_key = Some(blocker.key.clone());
            self.lane_gap_blocker_class = if blocker.vehicle_class.is_empty() {
                "vehicle".to_string()
            } else {
                blocker.vehicle_class.clone()
            };
            return;
        }
        let Some(key) = self.lane_gap_blocker_key.clone() else {
            return; // nobody was ever alongside
        };
        if self.lane_gap_said_keys.contains(&key) {
            return; // this one has been called
        }
        // Marked spoken either way. A cue held back by the spacing and let out
        // later would be describing a gap that has moved on since.
        self.lane_gap_said_keys.insert(key.clone());
        let went_ahead = self
            .trip
            .traffic_manager
            .vehicles
            .iter()
            .any(|v| v.key == key && v.position_mi > self.trip.position_mi);
        if went_ahead {
            // It passed the truck, not the other way round: nobody was
            // overtaken, so there is nothing to be clear of. With passers
            // coming up the lanes beside, this said "Left lane open" for
            // every car that went by (owner drive, 2026-10-09).
            return;
        }
        if self.lane_gap_cue_s > 0.0 || self.passing.is_some() {
            // Spaced out, or lane keeping is passing and takes the lane back
            // itself: "In the right lane." is the line for that.
            return;
        }
        self.lane_gap_cue_s = LANE_GAP_CUE_MIN_GAP_S;
        let name = lane_label(watch, self.lane.lane_count);
        let capitalized = capitalize(name);
        let message = format!("{capitalized} lane open.");
        ctx.play_event_cue(Some(SpeechCategory::Status), "ui/notify", 0.45, 0.0);
        // ROUTE priority: "the lane you were boxed out of is open" is the
        // transition a driver is actively waiting on to merge back, and at
        // the ambient default it was dropped as stale behind the very
        // traffic flurry that boxed them out (Shane, 2026-08-20). Category
        // stays STATUS so the verbosity ladder still governs HOW it speaks.
        ctx.say_event_with(
            message,
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Status),
        );
    }

    // -- full lane keeping lines up for its exit -----------------------------

    /// Lane keeping on full takes the exit, and the exit lane opens only
    /// beside the right lane, so it moves there itself: one lane at a time
    /// from the two-mile anchor in, and only into a lane this same clearance
    /// reading calls open. It used to hold whatever lane the truck was in, so
    /// a truck that pulled out around traffic three miles out rode the middle
    /// lane through the gore and missed the destination exit twice, each
    /// loop-back promising "lane keeping will take it" (owner's drive, I-94
    /// into Chicago, 2026-10-01).
    pub fn keep_right_for_exit(&mut self, ctx: &mut GameContext, stop: &RoadStop) {
        let ahead = stop.at_mi - self.trip.position_mi;
        if self.in_right_lane_for_exit()
            // A dodge in progress is the driver's; it is not undone for an
            // exit, and the move right waits until the hazard is answered.
            || self.hazard_deadline.is_some()
            || !(ahead > 0.0 && ahead <= EXIT_KEEP_RIGHT_MI)
            || !self.lane_keeping_may_change_lanes()
        {
            return;
        }
        let target = self.lane.lane - 1;
        if Some(target) == self.closed_lane_here() || !self.lane_gap_open(target) {
            return;
        }
        self.begin_lane_change(target);
        let name = lane_label(target, self.lane.lane_count);
        ctx.say_event_with(
            format!("Changing to the {name} lane for the exit."),
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Confirmation),
        );
    }

    // -- full lane keeping goes around what is in the lane --------------------

    /// Lane keeping on full answers a hazard confined to the truck's lane by
    /// changing into the lane the call found open, instead of braking for
    /// it: the truck is the one steering, so it takes the gap a driver would
    /// (owner ruling, 2026-10-01, for a slow vehicle). A ladder, a tire
    /// carcass or a car on the shoulder is gone around the same way (owner,
    /// 2026-10-19: "go around things by lane changing as much as
    /// possible"); braking is what is left when no lane is open.
    ///
    /// A slow vehicle is still followed in the two miles where the truck is
    /// lining up for its own exit, since matching its speed costs little. A
    /// thing sitting in the lane is not: braking for it there is a near stop,
    /// so it is gone around unless the exit is too close to get back for.
    /// Returns whether the change began, so the call can say so.
    pub fn pass_for_hazard(&mut self, ctx: &GameContext, event: &TripEvent) -> bool {
        let vehicle = event.data.traffic.is_some();
        if !ctx.settings.lane_is_automated()
            || (vehicle && self.lining_up_for_exit())
            || (!vehicle && self.too_close_to_exit_to_pass())
            || !self.lane_keeping_may_change_lanes()
        {
            return false;
        }
        let Some(step) = event.data.open_side.and_then(OpenSide::pass_step) else {
            return false;
        };
        let home = self.lane.lane;
        self.begin_lane_change(home + step);
        self.passing = Some((home, home + step));
        // A thing in the lane stays where it is: the truck holds the passing
        // lane until its whole length is by it.
        self.pass_clear_mi = (!vehicle).then(|| {
            let ahead_s = self.hazard_deadline.unwrap_or(0.0).max(LANE_TAP_CHANGE_S);
            self.trip.position_mi
                + self.trip.truck.speed_mph() * ahead_s / 3600.0
                + RIG_LENGTH_MI
                + if event.data.name.as_deref() == Some("the lane closure") {
                    CLOSURE_CLEAR_MI
                } else {
                    PASS_CLEAR_MARGIN_MI
                }
        });
        true
    }

    /// Whether an armed exit is so close that a lane change out and back,
    /// and the taper, no longer fit before it.
    fn too_close_to_exit_to_pass(&self) -> bool {
        if !self.lining_up_for_exit() {
            return false;
        }
        let Some(stop) = self.exit_stop.as_ref() else {
            return false;
        };
        let ahead = stop.at_mi - self.trip.position_mi;
        let out_and_back_mi = 2.0 * LANE_TAP_CHANGE_S * self.trip.truck.speed_mph() / 3600.0;
        ahead < out_and_back_mi + EXIT_TAPER_MI + PASS_CLEAR_MARGIN_MI
    }

    /// Back into the lane a pass left once the vehicle passed is behind and
    /// the lane is clear: keep right except to pass. The same clearance
    /// reading as every other lane change, so a vehicle still alongside or
    /// ahead in that lane holds the truck where it is. Dropped without a word
    /// if the truck is no longer in the lane it passed in (the driver moved
    /// it, or the road took the lane away) or leaves the highway.
    pub fn update_pass_return(&mut self, ctx: &mut GameContext) {
        self.pass_return_ease_mph = None;
        let Some((home, passed_in)) = self.passing else {
            return;
        };
        if self.lane_change_target.is_some() {
            return; // still moving over to pass
        }
        if home > passed_in {
            // Passed on the right: already where keeping right puts it.
            self.passing = None;
            return;
        }
        if !ctx.settings.lane_is_automated()
            || self.ramp_mi.is_some()
            || self.lane.lane != passed_in
            || home >= self.lane.lane_count
        {
            self.passing = None;
            self.pass_clear_mi = None;
            return;
        }
        let beside_it = self
            .pass_clear_mi
            .is_some_and(|clear| self.trip.position_mi < clear);
        if beside_it {
            // Still beside what it went around: the left lane is excused.
            self.keep_right_excused_s = KEEP_RIGHT_EXCUSE_S;
        }
        if self.hazard_deadline.is_some()
            || beside_it
            || !self.lane_keeping_may_change_lanes()
            || Some(home) == self.closed_lane_here()
        {
            return;
        }
        self.pass_clear_mi = None;
        if !self.pass_return_open(home) {
            // Held by traffic, which is lane keeping's call and not the
            // driver's: the left lane is excused while it lasts.
            self.keep_right_excused_s = KEEP_RIGHT_EXCUSE_S;
            self.ease_for_pacer(home);
            return;
        }
        self.passing = None;
        // "In the right lane." on arrival is the whole report; the blinker
        // says it is coming.
        self.begin_lane_change(home);
        self.pass_returning = true;
    }

    /// A change lane keeping is making checks the lane it is moving into on
    /// every frame of the move, not only when it began, and calls the move
    /// off if a vehicle takes that lane or the road ends it first. The move
    /// takes seconds, and a vehicle off a ramp can join the lane in them:
    /// lane keeping finished the move back from a pass anyway, into the side
    /// of a box truck that had just merged (player report, 2026-10-13).
    pub fn hold_keeping_lane_change(&mut self, ctx: &mut GameContext) {
        let Some(target) = self.lane_change_target else {
            self.keeping_lane_change = false;
            return;
        };
        if !self.keeping_lane_change {
            return; // the driver's own change is theirs to answer for
        }
        let count = self.lane.lane_count;
        let ended = target >= count;
        let open = if self.pass_returning {
            self.pass_return_open(target)
        } else {
            self.lane_gap_open(target)
        };
        if !ended && Some(target) != self.closed_lane_here() && open {
            return;
        }
        self.lane_change_target = None;
        self.keeping_lane_change = false;
        // The truck stays where it is because lane keeping called the move
        // off, so the CB has nothing to say about it being there.
        self.keep_right_excused_s = KEEP_RIGHT_EXCUSE_S;
        let home = self.lane.lane;
        if std::mem::take(&mut self.pass_returning) {
            // Still beside the vehicle it passed: try the move back again
            // once the lane is clear.
            self.passing = self.passing.or(Some((target, home)));
        } else if self
            .passing
            .is_some_and(|(_, passed_in)| passed_in == target)
        {
            // The pass never happened, so the hazard call is the driver's
            // again and the automatic brake answers it.
            self.passing = None;
        }
        // Each lane's line whole, so a translation can say the lane the way
        // its own language does rather than slot a bare "left" into a frame.
        let text = if ended {
            format!("The road narrows to {}.", lane_count_words(count))
        } else {
            match lane_label(home, count) {
                "left" => "Staying in the left lane.",
                "right" => "Staying in the right lane.",
                _ => "Staying in the middle lane.",
            }
            .to_string()
        };
        ctx.say_event_with(
            text,
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Confirmation),
        );
    }

    /// The vehicle keeping the move back from a pass out of `lane`, or None.
    ///
    /// A real headway, not the wide footprint a lane change otherwise reads:
    /// a vehicle ahead must stay `DROP_IN_HEADWAY_S` ahead of the cab at the
    /// truck's speed, and one behind that far behind the trailer at its own,
    /// through the seconds the move takes. The truck drops in behind a
    /// vehicle that has just gone by, the way a driver would, instead of
    /// waiting for it to be a third of a mile up the road. A vehicle on the
    /// ramp beside the right lane counts as in it.
    pub fn pass_return_blocker(&self, lane: i64) -> Option<&TrafficVehicle> {
        let speed = self.trip.truck.speed_mph();
        let horizon_hr = LANE_GAP_ACT_REAL_S * self.trip.effective_time_scale() / 3600.0;
        let ahead_need = speed * DROP_IN_HEADWAY_S / 3600.0;
        self.trip
            .traffic_manager
            .vehicles
            .iter()
            .filter(|v| v.lane == lane || (lane == 0 && v.lane == -1))
            .find(|v| {
                let rear = v.position_mi - self.trip.position_mi;
                let front = rear + real_length_mi(&v.vehicle_class);
                let drift = (v.speed_mph - speed) * horizon_hr;
                let behind_need = RIG_LENGTH_MI + v.speed_mph * DROP_IN_HEADWAY_S / 3600.0;
                rear.min(rear + drift) < ahead_need && front.max(front + drift) > -behind_need
            })
    }

    /// Whether the move back from a pass can go into `lane` now.
    pub fn pass_return_open(&self, lane: i64) -> bool {
        self.pass_return_blocker(lane).is_none()
    }

    /// A vehicle holding the right lane at the truck's speed or faster
    /// paces it, and lane keeping would wait in the left lane for as long as
    /// that lasts. Ease off under it, so it pulls ahead and the truck drops
    /// in behind, without a word: the driver hears the engine ease. A slower
    /// vehicle there is the one being passed, and the pass carries on at
    /// speed.
    fn ease_for_pacer(&mut self, lane: i64) {
        let Some(pacer) = self.pass_return_blocker(lane) else {
            return;
        };
        if pacer.speed_mph + 2.0 < self.trip.truck.speed_mph() {
            return;
        }
        self.pass_return_ease_mph =
            Some((pacer.speed_mph - PASS_RETURN_EASE_MPH).max(LANE_MIN_MPH));
    }

    /// Whether an armed exit is close enough that the truck should be
    /// heading for the right lane, not out of it.
    pub fn lining_up_for_exit(&self) -> bool {
        self.exit_stop.as_ref().is_some_and(|stop| {
            let ahead = stop.at_mi - self.trip.position_mi;
            !self.exit_signal_canceled && ahead > 0.0 && ahead <= EXIT_KEEP_RIGHT_MI
        })
    }

    /// What every change lane keeping makes on its own needs: no change
    /// already underway, on the highway, awake, and rolling with the engine
    /// on -- the same conditions a tap is refused for.
    fn lane_keeping_may_change_lanes(&self) -> bool {
        self.lane_change_target.is_none()
            && self.ramp_mi.is_none()
            && self.microsleep_deadline.is_none()
            && self.trip.truck.engine_on
            && self.trip.truck.speed_mph() >= LANE_MIN_MPH
    }

    /// Start a lane change lane keeping chose: the tap change's own drift,
    /// signal and arrival, which runs the same sideswipe check the
    /// clearance reading has already passed.
    fn begin_lane_change(&mut self, target: i64) {
        self.lane_change_target = Some(target);
        self.keeping_lane_change = true;
        self.pass_returning = false;
        self.lane_change_timer = LANE_TAP_CHANGE_S;
        self.lane_signal_timer = 0.0;
    }

    // -- the on-demand readout ------------------------------------------------

    /// The L key: where the truck sits, and whether the next lane over is
    /// open. The driver who missed the spoken line, or who wants to know
    /// before they commit, asks for the same reading it speaks from.
    pub fn lane_status_text(&mut self) -> String {
        let lane_count = self.lane.lane_count;
        let mut parts = vec![self.lane.describe()];
        for neighbour in [self.lane.lane - 1, self.lane.lane + 1] {
            if !(0..lane_count).contains(&neighbour) {
                continue;
            }
            let name = capitalize(lane_label(neighbour, lane_count));
            if self.lane_gap_closed_by_zone(neighbour) {
                parts.push(format!("{name} lane closed for construction."));
                continue;
            }
            match self.lane_blocked_text(neighbour) {
                None => parts.push(format!("{name} lane open.")),
                Some(blocked) => parts.push(blocked),
            }
        }
        parts.join(" ")
    }

    /// "Right lane blocked by a state trooper.", or None while that lane
    /// is open: the L key's reading, and the answer to a lane change on
    /// full lane keeping that has to wait.
    pub fn lane_blocked_text(&self, lane_index: i64) -> Option<String> {
        let blocker = self.lane_gap_blocker(lane_index)?;
        let name = capitalize(lane_label(lane_index, self.lane.lane_count));
        let vehicle = if blocker.vehicle_class.is_empty() {
            "vehicle".to_string()
        } else {
            blocker.vehicle_class
        };
        Some(format!("{name} lane blocked by a {vehicle}."))
    }
}

/// Python's `str.capitalize()`: upper-case the first character, lower-case the
/// rest. The lane labels are already lower-case words, so this only lifts the
/// first letter.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
    }
}
