//! Street corners cost speed, not reflex: be under the turn speed or loop back
//! (port of `freight_fate/states/driving_turns.py`, the `TurnCommitmentMixin`).
//!
//! The sim carries no heading or yaw -- `LaneKeeping` has a lateral offset and
//! a discrete lane index and nothing else -- so a "manual turn" could only ever
//! have been a reaction-window quick-time event. That version was built and
//! withdrawn (2026-07-15) because a timed snap excludes players with slow
//! reaction or motor impairments. The owner's replacement is commitment, not
//! reflex: a corner is a speed you have to be under by the time you reach it,
//! announced far enough out to brake by ear, exactly the way the exit ramp
//! already demands `RAMP_MAX_MPH` and the facility gate demands
//! `FACILITY_GATE_LIMIT_MPH`. Braking to a spoken number is plannable; snapping
//! an arrow inside a second and a half is not.
//!
//! The turn speed is anchored to the road, never invented: the street the truck
//! is turning ONTO carries a baked `local_speed_mph` (25 named, 15 unnamed
//! service ways), and the CORNER's own speed comes from its measured turn angle
//! through `ff_core::data::corners` -- TxDOT's design radius for that angle,
//! priced at the lateral a loaded combination actually holds. A square city
//! corner lands a little over 9 mph, a sweeping 60-degree one near 12, a
//! switchback under 8, and the advisory is the lower of that and the street.
//!
//! Until 2026-09-18 both ends were assumed constants instead -- the street
//! limit clamped between 15 and 20 -- so every corner in the game got the same
//! answer, and a truck already held at 14-15 by the speed keeper was under all
//! of them and never heard an advisory at all. `docs/turn-geometry-brief.md`
//! is the standing record of that directive and the sources behind the model.
//!
//! Every corner is called once its window opens, with its advise speed,
//! whether or not the truck is already under it, so the number a driver has
//! to make is never a guess.
//!
//! A miss costs time and nothing else: the truck goes around the block and
//! comes out on the street it missed. Until 2026-10-14 it was the fourth
//! instance of the shipped loop-back pattern (blown ramp stop, missed
//! destination exit, missed facility gate) and dropped the truck back onto
//! the approach to try again, which a player heard as a teleport.
//!
//! Highway junctions are deliberately not judged here. Their cues carry no
//! direction, radius, or lane ordinal, and `docs/nav-phrasing-brief.md` forbids
//! speaking a lane ordinal that was never harvested.

use crate::speech::EventPriority;
use ff_core::data::corners::{corner_radius_ft, ASSUMED_TURN_DEG};
use ff_core::data::curves::RouteCurve;
use ff_core::sim::trip_models::{NavigationCue, FACILITY_ACCESS_LIMIT_MPH};
use ff_core::sim::turn_guide::{TurnShape, TurnSide};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_speed_control::{KEEPER_EASE_REAL_S, KEEPER_SETTLE_REAL_S};

/// The approach call is sized in REAL seconds of hearing-and-braking time, the
/// same budget the exit callout gets, then converted to game miles at the
/// current pace. `LOCAL_TURN_LOOKAHEAD_MI` alone is about 1.4 real seconds at
/// street speed -- a reflex window, which is the thing this design refuses.
pub const TURN_WARNING_REAL_S: f64 = 25.0;
/// Bounded by the road it lives on: a city block is short, and no corner call
/// should arrive two miles of arterial before the corner exists. Never later
/// than the route's own street lead, so the call that carries the advise
/// speed is the first word about a corner, not a repeat after it.
pub const TURN_WINDOW_MIN_MI: f64 = ff_core::sim::trip::LOCAL_TURN_LOOKAHEAD_MI;
pub const TURN_WINDOW_MAX_MI: f64 = 2.0;
/// The nominal corner speed the APPROACH WINDOW is sized against -- not a
/// ceiling on the corner itself, which `data::corners` derives per angle.
/// A crawling truck still gets a window sized as if it were doing twenty, so
/// the call arrives with road left to brake in rather than on top of the turn.
pub const TURN_CORNER_MAX_MPH: f64 = 20.0;
/// Real seconds the clock takes to slide from the trip's pacing down to real
/// time ahead of a corner's brake point: the exit release, run the other way.
pub const TURN_CLOCK_EASE_REAL_S: f64 = ff_core::sim::trip::EXIT_APPROACH_RELEASE_S;
/// Brake deadband: the truck may be a few mph over without failing, the same
/// forgiveness the curve assist's hysteresis grants.
pub const TURN_SPEED_MARGIN_MPH: f64 = 3.0;
/// A judged corner this close to where the yard or lot begins is the turn
/// off the street into the driveway, not a street corner.
pub const DRIVEWAY_TURN_WITHIN_MI: f64 = 0.01;
/// Game minutes a missed corner costs: three turns around one block at 20,
/// with signals on two of them. The gate and the destination exit charge 20
/// for a highway-scale turnaround, miles of frontage road at 45 and back.
pub const TURN_MISS_LOOP_MIN: f64 = 8.0;
/// How far past the corner it stays the corner in play, and where the truck
/// comes out on the new street after going around the block.
pub const TURN_COMMIT_TAIL_MI: f64 = 0.15;
/// Inside this the approach call drops the distance and says "now" -- and is
/// an act-now instruction rather than a lead, which changes how it is paced.
pub const TURN_NOW_MI: f64 = 0.05;
/// The pursuit guide starts leaning into the corner this far out, reaching its
/// full lean at the corner itself.
///
/// The turn guide's own lead, by name and not by a second number. This was
/// 0.2 beside the turn guide's 0.12, so the lane guide opened a corner's lean
/// first and the turn guide then took the engine over and started the same
/// lean again from nothing (review I3, 2026-09-19). One constant, so the
/// engine's lean and the tone's open at the same place.
pub const TURN_GUIDE_LEAD_MI: f64 = ff_core::sim::turn_guide::LEAD_MI;
pub const TURN_GUIDE_DEMAND: f64 = 0.9;
/// An exit ramp peels right; the lane model already pushes the truck that way,
/// so the engine leans with it instead of sitting dead centre.
pub const RAMP_GUIDE_DEMAND: f64 = 0.45;

/// The `announced_navigation` half that says a corner's advise speed went
/// out inside the route's own act-now call for it.
pub const ADVICE_IN_CALL: &str = "advice";

/// `_is_judged_turn(cue)`: a baked street maneuver with a real side to it.
///
/// `local:start` is where the chain begins, not a corner, and an "ahead"
/// or "straight" maneuver has nothing to steer through.
pub fn is_judged_turn(cue: &NavigationCue) -> bool {
    cue.kind == "local_turn"
        && cue.key.starts_with("local:turn:")
        && matches!(
            cue.direction.trim().to_lowercase().as_str(),
            "left" | "right"
        )
}

impl DrivingState {
    // -- the corner in play ---------------------------------------------------

    /// `_reset_turn_state_for_trip()`: cue keys belong to one trip's route; a
    /// surface-chain swap builds a new one, so the latches start clean with it.
    pub fn reset_turn_state_for_trip(&mut self) {
        self.turn_trip_id = self.trip_generation;
        self.turn_advised.clear();
        self.turn_resolved.clear();
        self.turn_announced.clear();
        self.turn_called_now.clear();
        self.turn_grace_s = 0.0;
        self.trip.controlled_turn = false;
    }

    /// `_turn_cues_in_play()`: every unsettled corner from here on, nearest
    /// first.
    ///
    /// The commitment loop only ever judges one corner at a time, so it asks
    /// for the first of these. A planner that has to BRAKE for corners needs
    /// the whole run: city blocks are shorter than one corner's own tail, so
    /// the next corner is regularly already in front of the truck while this
    /// one is still being taken.
    pub fn turn_cues_in_play(&mut self) -> Vec<NavigationCue> {
        if self.turn_trip_id != self.trip_generation {
            self.reset_turn_state_for_trip();
        }
        let position = self.trip.position_mi;
        let mut cues: Vec<NavigationCue> = self
            .trip
            .navigation_cues
            .iter()
            .filter(|cue| {
                is_judged_turn(cue)
                    && !self.turn_resolved.contains(&cue.key)
                    && cue.at_mi - position >= -TURN_COMMIT_TAIL_MI
            })
            .cloned()
            .collect();
        cues.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
        cues
    }

    /// `_turn_cue_in_play()`: the corner currently being approached or judged,
    /// or None.
    pub fn turn_cue_in_play(&mut self) -> Option<NavigationCue> {
        self.turn_cues_in_play().into_iter().next()
    }

    /// The same read without the lazy latch reset, for the one caller that
    /// only has `&self`.
    ///
    /// Python's `_turn_cues_in_play` re-seeds the per-corner latches when the
    /// trip has been swapped; the pursuit guide below reads the corner from
    /// the audio pass, which `_update_turn_commitment` has already run ahead
    /// of in the same frame (`driving_updates.py`: 577 before 595), so the
    /// re-seed has always happened by then.
    /// Whether `cue` is the turn off the public street into the facility's
    /// driveway (where the yard or lot zone begins, inbound).
    pub fn is_driveway_turn(&self, cue: &NavigationCue) -> bool {
        self.trip
            .driveway_mi()
            .is_some_and(|at| (cue.at_mi - at).abs() <= DRIVEWAY_TURN_WITHIN_MI)
    }

    /// The driveway turn is on this route and not yet settled -- not taken
    /// cleanly, not gone around the block for.
    pub fn driveway_turn_pending(&self) -> bool {
        self.trip.navigation_cues.iter().any(|cue| {
            is_judged_turn(cue)
                && self.is_driveway_turn(cue)
                && !self.turn_resolved.contains(&cue.key)
        })
    }

    pub(crate) fn turn_cue_in_play_read(&self) -> Option<NavigationCue> {
        let position = self.trip.position_mi;
        self.trip
            .navigation_cues
            .iter()
            .filter(|cue| {
                is_judged_turn(cue)
                    && !self.turn_resolved.contains(&cue.key)
                    && cue.at_mi - position >= -TURN_COMMIT_TAIL_MI
            })
            .min_by(|a, b| a.at_mi.total_cmp(&b.at_mi))
            .cloned()
    }

    /// A street corner's shape: the angle the map measured at the junction
    /// (a square corner when it could not) and the WB-67 design radius for
    /// it from `data::corners`, the same geometry that sets its advise speed.
    pub fn corner_shape(&self, cue: &NavigationCue, side: TurnSide) -> TurnShape {
        let degrees = self
            .trip
            .route
            .legs
            .get(self.turn_leg_index(cue))
            .map(|leg| leg.local_turn_deg)
            .filter(|deg| *deg > 0.0)
            .unwrap_or(ASSUMED_TURN_DEG);
        TurnShape {
            side,
            deflection_deg: degrees,
            radius_ft: corner_radius_ft(degrees),
        }
    }

    /// The road's own bend at a street corner the truck is inside, as the
    /// lane model takes it: one over the radius, positive turning right, over
    /// the arc the corner sweeps. Zero anywhere else.
    ///
    /// The lane had no corners in it. The engine leaned into one and asked
    /// for a turn's worth of wheel, but the road under the truck ran straight
    /// on, so a driver who steered as asked only drove across their own lane
    /// and then heard the lean point back the other way (forum report 448,
    /// 2026-09-30). A corner bends the lane's road like a mapped bend now:
    /// nothing steering it runs wide, and curve assistance takes it.
    ///
    /// Read from the cues directly rather than the corner judging, whose
    /// latches can close a corner before the truck is through its arc.
    pub fn street_corner_curvature(&self) -> f64 {
        let into = |cue: &NavigationCue| self.trip.position_mi - cue.at_mi;
        self.trip
            .navigation_cues
            .iter()
            .filter(|cue| is_judged_turn(cue) && into(cue) >= 0.0)
            .find_map(|cue| {
                let side = TurnSide::parse(&cue.direction)?;
                let shape = self.corner_shape(cue, side);
                let arc_mi = shape.radius_ft * shape.deflection_deg.to_radians() / 5280.0;
                (into(cue) < arc_mi).then(|| side.sign() / shape.radius_ft)
            })
            .unwrap_or(0.0)
    }

    /// The side a hold toward follows the road for (`LaneKeeping::
    /// turn_in_play`): the turn the engine leans for, or failing that the
    /// nearest turn ahead the driver has already been told about.
    ///
    /// From the call, not from the lean's lead. The instinct is to hold the
    /// moment the game says turn, and "Right turn onto South Dakota Street,
    /// one mile" held right steered a truck a mile short of the corner off the
    /// road; "Sharp left, half a mile" held left changed lanes into the
    /// median (owner's drives, 2026-09-30). Called, the hold waits straight
    /// on in its lane until the turn arrives.
    pub fn hold_turn_side(&mut self) -> f64 {
        if let Some(shape) = self.turn_guide_input(false).shape {
            return shape.side.sign();
        }
        let position = self.trip.position_mi;
        let (limit, _) = self.trip.speed_limit_at(position);
        let mut nearest: Option<(f64, f64)> = None;
        let mut consider = |ahead: f64, side: Option<TurnSide>| {
            if let Some(side) = side.filter(|_| ahead > 0.0) {
                if nearest.is_none_or(|(held, _)| ahead < held) {
                    nearest = Some((ahead, side.sign()));
                }
            }
        };
        for cue in self
            .trip
            .navigation_cues
            .iter()
            .filter(|cue| is_judged_turn(cue))
        {
            let called = self.turn_advised.contains(&cue.key)
                || ["advance", "near"].iter().any(|half| {
                    self.trip
                        .announced_navigation
                        .contains(&format!("{}:{half}", cue.key))
                });
            if called {
                consider(cue.at_mi - position, TurnSide::parse(&cue.direction));
            }
        }
        for bend in self.trip.curves.iter().filter(|bend| !bend.connector) {
            // A bend the road warns about, whose call has gone out.
            if (bend.advisory_mph as f64) < limit && self.trip.curve_called(bend) {
                consider(
                    bend.start_mi - position,
                    TurnSide::parse(&bend.direction.to_string()),
                );
            }
        }
        nearest.map_or(0.0, |(_, sign)| sign)
    }

    /// `_turn_leg_index(cue)`.
    pub fn turn_leg_index(&self, cue: &NavigationCue) -> usize {
        cue.key
            .rsplit_once(':')
            .and_then(|(_, tail)| tail.parse::<usize>().ok())
            .unwrap_or(0)
    }

    /// `_turn_street_text(cue)`: the street being turned onto, in the world's
    /// own spoken name.
    pub fn turn_street_text(&self, cue: &NavigationCue) -> String {
        let index = self.turn_leg_index(cue);
        let legs = &self.trip.route.legs;
        if let Some(leg) = legs.get(index) {
            if !leg.highway.is_empty() {
                return leg.highway.clone();
            }
        }
        "the next street".to_string()
    }

    /// `_turn_speed_mph(cue)`: the speed the corner has to be taken under --
    /// the lower of the street's own posted limit and what the corner's own
    /// geometry allows THIS combination, loaded as it is right now. An empty
    /// trailer's weight sits far lower than a full one's, so it takes the same
    /// corner faster; `corners` owns that span.
    ///
    /// There is deliberately NO floor. The old one clamped every corner to at
    /// least the 15 mph gate crawl, which made a truck already held at 14-15 by
    /// the speed keeper faster than every corner on the route, so the advisory
    /// never spoke and the owner drove past a turn in Spokane (2026-08-21).
    /// A corner is now as slow as its own angle says it is, which for a square
    /// city corner is a little over 9.
    pub fn turn_speed_mph(&self, cue: &NavigationCue) -> f64 {
        let index = self.turn_leg_index(cue);
        let leg = self.trip.route.legs.get(index);
        // The street's own posted limit where the chain carries it -- the
        // same number its zone posts -- else the bake's street speed.
        let posted = leg
            .map(|leg| leg.street_limit_mph().unwrap_or(leg.local_speed_mph))
            .unwrap_or(0.0);
        let street = if posted != 0.0 {
            posted
        } else {
            FACILITY_ACCESS_LIMIT_MPH
        };
        // 0.0 is a corner nobody measured -- a legacy or estimated route --
        // and `corners` prices that as a square one rather than inventing a
        // shape for it.
        let measured = leg.map(|leg| leg.local_turn_deg).filter(|deg| *deg > 0.0);
        // The turn off the street into the gate or driveway itself: the
        // facility's own entry speed, then the yard's limit inside.
        if self.is_driveway_turn(cue) {
            return street.min(ff_core::data::corners::DRIVEWAY_TURN_MPH);
        }
        // A gentle bend that stays on the same road is no turn at all: it
        // keeps the posted limit, and the curve model prices a real curve.
        // The bend cap is for a routed turn onto another road (a fork or a
        // slight turn at a junction).
        if self.is_same_road_bend(cue) {
            return street;
        }
        street.min(ff_core::data::corners::corner_speed_side_mph(
            measured,
            ff_core::data::corners::CornerSide::from_direction(&cue.direction),
            self.trip.truck.roll_load_fraction(),
        ))
    }

    /// Whether `cue` is a bend of [`BEND_MAX_DEG`] or less that carries on
    /// along the same named road, not a turn onto another one.
    ///
    /// [`BEND_MAX_DEG`]: ff_core::data::corners::BEND_MAX_DEG
    pub fn is_same_road_bend(&self, cue: &NavigationCue) -> bool {
        let index = self.turn_leg_index(cue);
        let legs = &self.trip.route.legs;
        let (Some(leg), Some(prev)) = (
            legs.get(index),
            index.checked_sub(1).and_then(|i| legs.get(i)),
        ) else {
            return false;
        };
        let deg = leg.local_turn_deg;
        deg > 0.0
            && deg <= ff_core::data::corners::BEND_MAX_DEG
            && !leg.highway.trim().is_empty()
            && leg.highway.trim().eq_ignore_ascii_case(prev.highway.trim())
    }

    /// Each corner ahead, nearest first, with the speed the truck must be at
    /// when it reaches that corner: its own turn speed, lowered only when a
    /// later, slower corner comes too soon after it to brake for once this
    /// one is cleared (the keeper's ordinary shed, from this corner's number
    /// to the later one's, needs more road than the gap between them).
    pub fn corner_targets_ahead(&mut self) -> Vec<(NavigationCue, f64)> {
        let cues = self.turn_cues_in_play();
        let mut out: Vec<(NavigationCue, f64)> = cues
            .into_iter()
            .map(|cue| {
                let own = self.turn_speed_mph(&cue);
                (cue, own)
            })
            .collect();
        let decel = crate::states::driving_events::pending::KEEPER_SNUB_DECEL_MPS2;
        for i in (0..out.len().saturating_sub(1)).rev() {
            let (later_at, later_mps) = (out[i + 1].0.at_mi, out[i + 1].1 / 2.23694);
            let gap_m = ((later_at - out[i].0.at_mi) * 1609.344).max(0.0);
            let reachable = (later_mps * later_mps + 2.0 * decel * gap_m).sqrt() * 2.23694;
            if reachable < out[i].1 {
                out[i].1 = reachable;
            }
        }
        out
    }

    /// `_turn_window_mi()`: how far out the corner is called -- a full spoken
    /// window, so there is road left to brake in once it has been heard.
    pub fn turn_window_mi(&self) -> f64 {
        let speed = self.trip.truck.speed_mph().max(TURN_CORNER_MAX_MPH);
        let miles = TURN_WARNING_REAL_S * speed * self.trip.effective_time_scale() / 3600.0;
        TURN_WINDOW_MIN_MI.max(miles.min(TURN_WINDOW_MAX_MI))
    }

    /// `_turn_grace_seconds(message)`: real reaction seconds after `message`,
    /// the ramp-arrival rule -- a corner is never failed while its own cue is
    /// still being spoken.
    pub fn turn_grace_seconds(&self, ctx: &GameContext, message: &str) -> f64 {
        let speech_rate = if ctx.settings.sapi_events && ctx.speech.event_supports_rate() {
            ctx.settings.speech_rate
        } else {
            0.0
        };
        ramp_arrival_grace_seconds(message, speech_rate)
    }

    /// `_turn_miss_suspended()`: something else owns the wheel -- a hazard
    /// swerve, a microsleep, a traffic stop, or an open arrival menu must
    /// never read as a miss.
    pub fn turn_miss_suspended(&self) -> bool {
        self.hazard_deadline.is_some()
            || self.microsleep_deadline.is_some()
            || self.pull_over.is_some()
            || self.arrival_menu_open
    }

    /// `ahead` at which the corner's clock is real time: reaction seconds
    /// plus the shed down to the advise speed, both on the real clock, so
    /// everything a driver does about the corner is plannable by ear.
    ///
    /// It used to go real at the approach CALL, which is sized in real
    /// seconds on the compressed clock and so opened up to two miles out: a
    /// 30 mph approach then crawled for four real minutes (flight, and the
    /// agent drive into Abilene, 2026-09-23). The call stays time-based; only
    /// the clock waits for the brake point.
    pub fn turn_brake_point_mi(&self, cue: &NavigationCue) -> f64 {
        let speed = self.trip.truck.speed_mph().max(1.0);
        let reaction_mi = (KEEPER_EASE_REAL_S + KEEPER_SETTLE_REAL_S) * speed / 3600.0;
        reaction_mi + self.keeper_shed_mi(self.turn_speed_mph(cue), 1.0)
    }

    /// Whether a facility street chain has reached the brake point for the
    /// stop at its end: reaction seconds plus the shed to a standstill, on
    /// the real clock, the same rule a turn keeps.
    pub fn at_the_gates_brake_point(&self) -> bool {
        let speed = self.trip.truck.speed_mph().max(1.0);
        let reaction_mi = (KEEPER_EASE_REAL_S + KEEPER_SETTLE_REAL_S) * speed / 3600.0;
        self.trip.remaining_miles() <= reaction_mi + self.keeper_shed_mi(0.0, 1.0)
    }

    /// Pace the clock for a corner `ahead` miles off: real time inside the
    /// brake point, sliding down to it over `TURN_CLOCK_EASE_REAL_S` before.
    pub(crate) fn pace_clock_for_turn(&mut self, cue: &NavigationCue, ahead: f64) {
        let brake_mi = self.turn_brake_point_mi(cue);
        self.trip.controlled_turn = ahead <= brake_mi;
        if self.trip.controlled_turn {
            return;
        }
        // The trip's own pacing here, since the turn clock was cleared at the
        // top of the frame: the ease is sized on the mean of the two ends.
        let paced = self.trip.effective_time_scale();
        let speed = self.trip.truck.speed_mph().max(1.0);
        let ease_mi = TURN_CLOCK_EASE_REAL_S * speed * (paced + 1.0) / 2.0 / 3600.0;
        self.trip.turn_clock = ((brake_mi + ease_mi - ahead) / ease_mi).clamp(0.0, 1.0);
    }

    // -- spoken text ----------------------------------------------------------

    /// `_turn_approach_text(cue, ahead_mi)`: the approach call, in the pacenote
    /// grammar -- side, street, distance, advisory.
    ///
    /// Always says the verb, because a bare "Right now" reads as a direction
    /// or as timing, never reliably one.
    pub fn turn_approach_text(
        &self,
        ctx: &GameContext,
        cue: &NavigationCue,
        ahead_mi: f64,
    ) -> String {
        let settings = &ctx.settings;
        let direction = cue.direction.trim().to_lowercase();
        let street = self.turn_street_text(cue);
        let call = if ahead_mi <= TURN_NOW_MI {
            format!("Turn {direction} now onto {street}.")
        } else {
            let distance = settings.short_distance_text(ahead_mi);
            format!("{} turn onto {street}, {distance}.", capitalize(&direction))
        };
        format!("{call} {}", self.turn_advice_text(ctx, cue))
    }

    /// The approach call's advisory half alone: "Advise 11 miles per hour.",
    /// with the keeper's own line when it is taking the corner.
    ///
    /// Terse keeps the advise speed. It used to drop it with the rest of the
    /// tail, which left a terse driver no number to brake to at all.
    pub fn turn_advice_text(&self, ctx: &GameContext, cue: &NavigationCue) -> String {
        let target = ctx.settings.speed_text(self.turn_speed_mph(cue));
        let advice = format!("Advise {target}.");
        // "Speed keeper easing." used to follow on every corner the keeper
        // slowed for. Owner, 2026-10-25: the move onto city streets said too
        // much; what the keeper does is in the settings help.
        advice
    }

    // -- the frame ------------------------------------------------------------

    /// `_update_turn_commitment(dt)`: call the corner, hold the clock through
    /// it, judge it once.
    pub fn update_turn_commitment(&mut self, ctx: &mut GameContext, dt: f64) {
        let cue = self.turn_cue_in_play();
        if self.turn_grace_s > 0.0 {
            self.turn_grace_s = 0.0f64.max(self.turn_grace_s - dt);
        }
        // Re-decided every frame from where the corner is now.
        self.trip.turn_clock = 0.0;
        let Some(cue) = cue else {
            self.trip.controlled_turn = false;
            return;
        };
        let ahead = cue.at_mi - self.trip.position_mi;
        if !self.turn_advised.contains(&cue.key) {
            if ahead <= 0.0 && self.trip.truck.speed_mph() <= self.turn_speed_mph(&cue) {
                // A cold arrival already under the corner's speed: there is
                // nothing left to call and nothing to fail.
                self.resolve_turn(&cue);
                return;
            }
            // Every corner is called, at its speed, whether or not the truck
            // is already under it. Calling only the corners a truck was too
            // fast for left a driver who had slowed down hearing an advise
            // speed on some turns and not others, with no way to know how
            // fast the next one could be taken (player report, 2026-10-13:
            // "I'm not a mind reader"). Knowing the number is what lets a
            // driver run the block between corners at the street's speed.
            if ahead > self.turn_window_mi() {
                return;
            }
            if ahead > 0.0 && self.trip.turning_through_corner() {
                // Still round the last corner, whose chime has just sounded:
                // this call waits for the rear to clear it, or the words name
                // one side over the other side's chime.
                self.pace_clock_for_turn(&cue, ahead);
                return;
            }
            // Either the window opened, or a resumed save arrived at the
            // corner cold. Both start the clock with the corner's own advice;
            // neither may latch a miss on first contact.
            self.turn_advised.insert(cue.key.clone());
            self.pace_clock_for_turn(&cue, ahead);
            let full = self.turn_approach_text(ctx, &cue, 0.0f64.max(ahead));
            self.turn_grace_s = self.turn_grace_seconds(ctx, &full);
            // The route's own lead ("In a quarter mile, turn left onto a side
            // street") may have just named this corner. Then the call keeps
            // only what the lead did not say, the advise speed, instead of the
            // whole corner again seconds later (owner, Abilene streets,
            // 2026-09-23). A "now" call is the instruction itself and stays
            // whole -- unless the route's own act-now call ("Turn right onto
            // a service road.") already gave it, which it does for a turn
            // right at the gate: then "Turn right now onto a service road"
            // was the same turn said twice (agent drive, Aberdeen yard).
            let heard = |drive: &Self, half: &str, category: SpeechCategory| {
                drive
                    .trip
                    .announced_navigation
                    .contains(&format!("{}:{half}", cue.key))
                    && (ctx.settings.speaks(Some(category)) || !ctx.ladder_applies())
            };
            let near_heard = heard(self, "near", SpeechCategory::Navigation);
            let lead_heard = (ahead > TURN_NOW_MI
                && heard(self, "advance", SpeechCategory::NavigationAdvisory))
                || near_heard;
            let advice_in_call = self
                .trip
                .announced_navigation
                .contains(&format!("{}:{ADVICE_IN_CALL}", cue.key));
            let message = if advice_in_call {
                String::new()
            } else if lead_heard {
                self.turn_advice_text(ctx, &cue)
            } else {
                full
            };
            // The side's chime goes out with the words that name the turn,
            // and only when those words are spoken: a rung that silences the
            // call silences its chime too (owner, 2026-09-20). It used to
            // wait for the corner itself, so the chime for the turn just
            // taken landed next to the call for the next one, and a driver
            // heard a right chime beside "Left turn" (player report,
            // 2026-10-13).
            if ctx.settings.speaks(Some(SpeechCategory::Navigation)) || !ctx.ladder_applies() {
                self.chime_corner(ctx, &cue);
            }
            // A LEAD is deliberately left on the droppable ambient default,
            // unlike the act-now navigation calls raised to ROUTE alongside
            // it: a quarter-mile warning that survives to be spoken AFTER the
            // turn has been missed is worse than one that never arrives
            // (test_missed_turn_speaks_at_urgent_only caught exactly that, the
            // stale cue landing on top of the miss announcement). Going stale
            // is the correct end for a lead.
            //
            // A call that already says "now" is not a lead. It is the only
            // instruction the driver gets for a corner they are already at --
            // a cold arrival, or a loop-back onto a corner closer to the start
            // of the street chain than one spoken window -- and dropped as
            // stale it leaves the driver with nothing: "Turn right now onto
            // West Main Avenue" was dropped twice on one arrival and never
            // once heard (owner, Spokane, 2026-08-22). ROUTE, like the
            // trip's own near call.
            if !lead_heard && 0.0f64.max(ahead) <= TURN_NOW_MI {
                self.turn_called_now.insert(cue.key.clone());
            }
            // The advise speed for a corner whose act-now call was just said
            // belongs to that call, not to the chatter: as chatter it waited
            // past its budget behind the call and flushed the voice, which
            // handed the call back to be said again ("Turn right onto Port
            // of Cleveland" twice, agent drive into Cleveland, 2026-10-25).
            if message.is_empty() {
                return;
            }
            if 0.0f64.max(ahead) <= TURN_NOW_MI || near_heard {
                ctx.say_event_with(
                    message,
                    SayEvent::queued()
                        .priority(EventPriority::Important)
                        .category(SpeechCategory::Navigation),
                );
            } else {
                ctx.say_event_with(
                    message,
                    SayEvent::queued().category(SpeechCategory::Navigation),
                );
            }
            return;
        }
        if ahead > 0.0 {
            self.pace_clock_for_turn(&cue, ahead);
            return;
        }
        let too_fast =
            self.trip.truck.speed_mph() > self.turn_speed_mph(&cue) + TURN_SPEED_MARGIN_MPH;
        if !too_fast {
            // Taken under its speed: settled here, its tone now. The grace
            // below only keeps a corner from being FAILED while its own cue
            // is still speaking. It used to hold a corner taken cleanly too,
            // for as long as the off-the-ramp line took at the slowest
            // modelled voice -- 71 seconds for the Ardmore yard's -- while
            // the truck drove through the next two corners, whose calls came
            // late and whose tones all sounded together at the end of it
            // (live drive into Ardmore, 2026-09-24).
            self.resolve_turn(&cue);
            return;
        }
        if self.turn_grace_s > 0.0 {
            return; // the corner's own cue is still speaking
        }
        if self.turn_miss_suspended() {
            self.resolve_turn(&cue);
            return;
        }
        self.handle_missed_turn(ctx, &cue);
    }

    /// `_resolve_turn(cue)`: this corner is settled; the clock goes back to
    /// trip pacing.
    pub fn resolve_turn(&mut self, cue: &NavigationCue) {
        self.turn_resolved.insert(cue.key.clone());
        self.turn_grace_s = 0.0;
        self.trip.controlled_turn = false;
    }

    /// The turn's chime, panned to its side: once per corner, with the
    /// first words that name it, whichever line that is (this call, the
    /// route's own lead, or the street chain's briefing).
    pub fn chime_corner(&mut self, ctx: &mut GameContext, cue: &NavigationCue) {
        if !self.turn_announced.insert(cue.key.clone()) {
            return;
        }
        if let Some(sound) = local_turn_sound(Some(&cue.direction)) {
            let pan = if cue.direction.trim().eq_ignore_ascii_case("left") {
                -TURN_CUE_PAN
            } else {
                TURN_CUE_PAN
            };
            ctx.audio.play_with(sound, 1.0, pan);
        }
    }

    /// `_handle_missed_turn(cue)`: the truck goes around the block.
    ///
    /// A truck too fast for a corner drives on past it, makes three turns
    /// round the block, and comes out on the street it missed: time lost,
    /// nothing else. The truck carries on forward and never rewinds. This
    /// used to drop it back a full spoken window before the corner to try
    /// again, with a brake lesson from the second miss, and a player heard
    /// it as a teleport (player report, 2026-10-13).
    pub fn handle_missed_turn(&mut self, ctx: &mut GameContext, cue: &NavigationCue) {
        self.turn_miss_count += 1;
        self.trip.game_minutes += TURN_MISS_LOOP_MIN;
        // The controllers drop but the session stays armed: on a facility
        // approach the keeper manages the corners, and it resumes on its own
        // on the new street instead of leaving the truck idling (owner
        // ruling, 2026-09-01).
        self.cancel_cruise(ctx, true);
        self.cancel_keeper(ctx, true);
        self.resolve_turn(cue);
        self.trip.position_mi = self.trip.position_mi.max(cue.at_mi + TURN_COMMIT_TAIL_MI);
        let message = if self.terse_speech(ctx) {
            "Missed the turn. Going around the block.".to_string()
        } else {
            let street = self.turn_street_text(cue);
            format!("Missed the turn onto {street}. Going around the block.")
        };
        ctx.audio.play("ui/warning");
        self.set_status("Missed the turn. Went around the block.");
        // A turn on the route, not an optional stop: it must survive
        // quiet and urgent-only as words, not an earcon blip.
        ctx.say_event_with(
            message,
            SayEvent::new().category(SpeechCategory::Navigation),
        );
    }

    // -- the pursuit guide ----------------------------------------------------

    /// `_maneuver_steer_demand(connector=None)`: signed steer for the maneuvers
    /// that carry no mainline curve record.
    ///
    /// Connector arcs are excluded from the curve PUSH and from the pacenotes
    /// on purpose (ramps carry their own speech), and street legs carry baked
    /// maneuvers instead of curve geometry. Excluding them from the GUIDE as
    /// well silenced the panned road bed -- the only continuous directional
    /// audio the game has -- through every exit ramp and every turn, which is
    /// exactly where a blind driver needs it. This feeds the existing bed a
    /// synthetic demand; it never adds a tone (`sim::lane_guidance`).
    pub fn maneuver_steer_demand(&self, connector: Option<&RouteCurve>) -> f64 {
        if let Some(connector) = connector {
            let tightness = 0.2f64.max(1.0 - connector.min_radius_ft as f64 / 5000.0);
            let excess = 0.0f64.max(self.trip.truck.speed_mph() - connector.advisory_mph as f64);
            let magnitude = 1.0f64.min(tightness * (1.0 + excess * 0.04));
            return if connector.direction == 'L' {
                -magnitude
            } else {
                magnitude
            };
        }
        if self.ramp_mi.is_some() {
            // The exit ramp bends only in its curve now (realistic exit,
            // 2026-09-24), so the engine leans for the curve, leading into it
            // the way it leads into a street turn, and rides centred down the
            // deceleration lane and the run to the bar. A lean for the whole
            // ramp asked a driver steering for themselves to turn on straight
            // road. A ramp the game put the truck on with no layout behind it
            // (the loop-back to a missed terminal) keeps the old whole-ramp
            // lean.
            if self.ramp_layout.is_none() {
                return RAMP_GUIDE_DEMAND;
            }
            if self.ramp_curve_radius_ft().is_some() {
                return RAMP_GUIDE_DEMAND;
            }
            return match self.deceleration_lane_left_mi() {
                Some(left) if left < TURN_GUIDE_LEAD_MI => {
                    RAMP_GUIDE_DEMAND * (1.0 - left / TURN_GUIDE_LEAD_MI)
                }
                _ => 0.0,
            };
        }
        let Some(cue) = self.turn_cue_in_play_read() else {
            return 0.0;
        };
        let ahead = cue.at_mi - self.trip.position_mi;
        if ahead > TURN_GUIDE_LEAD_MI {
            return 0.0;
        }
        let magnitude = if ahead <= 0.0 {
            TURN_GUIDE_DEMAND
        } else {
            TURN_GUIDE_DEMAND * (1.0 - ahead / TURN_GUIDE_LEAD_MI)
        };
        if cue.direction.to_lowercase() == "left" {
            -magnitude
        } else {
            magnitude
        }
    }
}

/// Python's `str.capitalize()`: first character upper, the rest lower.
fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
    }
}
