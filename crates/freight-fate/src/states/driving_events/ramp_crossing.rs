//! Working the pedals for a ramp terminal, and crossing it: honour the light
//! or the sign, or pay for it.

use crate::speech::EventPriority;
use ff_core::models::enforcement::{
    career_citations, citation_fine, construction_zone_fine_clause, RED_LIGHT_FINE, STOP_SIGN_FINE,
};
use ff_core::pyfmt::fmt_grouped;
use ff_core::pyrandom::PyRandom;
use ff_core::sim::cross_traffic::{
    yield_crossing_times_s, CrossVehicle, COMBINATION_LENGTH_FT, TRACTOR_LENGTH_FT,
    YIELD_LINE_TO_CROSSROAD_FT,
};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use super::ramp_terminal::CrossMeeting;
use crate::states::driving_stops::assist_servo_brake;

impl DrivingState {
    /// Route-transition assistance works the pedals for the terminal.
    ///
    /// Stopping a rig blind inside the bar's grace window while the light
    /// cycles in real time is a positioning task whose failure mode is
    /// trailer damage -- the 2026-07-22 playtest ended a clean run with cross
    /// traffic in the trailer. With route-transition assistance on, the assist
    /// brakes for a red (or a yellow it cannot legally beat), holds the stop
    /// at the bar, and keeps a green crossing under the clean-roll speed. The
    /// phases still speak. Pulling ahead when the light releases is the
    /// driver's move unless facility stopping assistance is on, in which case
    /// that assist takes it (`terminal_release_text`).
    pub fn update_ramp_terminal_assist(&mut self, ctx: &mut GameContext) {
        self.update_ramp_terminal_assist_with_input(ctx, false);
    }

    /// Whether route-transition assistance has the pedals for the light,
    /// sign or yield ahead: announced, and not yet crossed.
    pub(crate) fn terminal_assist_owns(&self, ctx: &GameContext) -> bool {
        ctx.settings.route_transition_assist
            && self.terminal_live()
            && !self.ramp_terminal_done
            && matches!(
                self.ramp_control.as_str(),
                "signal" | "stop" | "yield" | "roundabout"
            )
            && self.ramp_light_announced
    }

    /// The speed to drive a truck short of its ramp's light or sign up at,
    /// when nothing is braking it for the line: the green's roll speed, or
    /// on a red or a sign what the assist can still stop from in the road
    /// left. `None` when the line is the assist's to hold or already close.
    pub(crate) fn terminal_close_up_mph(&self, ctx: &GameContext) -> Option<f64> {
        if !self.terminal_assist_owns(ctx)
            || self.ramp_waiting_at_light
            || self.ramp_waiting_at_sign
            || self.ramp_assist_brake > 0.0
        {
            return None;
        }
        let gap_mi = self.terminal_gap_mi()?;
        if gap_mi <= RAMP_ASSIST_HOLD_MI {
            return None;
        }
        let green = self.ramp_control == "signal" && self.ramp_light_phase() == "green";
        Some(if green {
            GREEN_ROLL_MPH - 5.0
        } else {
            let stoppable_mps = (2.0 * LINE_APPROACH_DECEL_MPS2 * gap_mi * 1609.344).sqrt();
            (stoppable_mps * MPH_PER_MPS)
                .min(self.ramp_cap_here_mph())
                .max(LINE_CREEP_MPH)
        })
    }

    /// Whether this assist drives the truck up to the line itself: the
    /// driver left speed control on, which the terminal only paused, so the
    /// pedals are the assists' from the bend to the bar. The co-driver does
    /// their own driving up.
    ///
    /// The assist used to brake for the light and then let go, and a truck
    /// on the All assists preset coasted to a stand 400 feet short of a red
    /// at Cleveland's exit 170A and sat there through every green after it
    /// (agent drive, 2026-10-25).
    pub(crate) fn terminal_assist_closes_up(&self, ctx: &GameContext) -> bool {
        self.speed_control_armed
            && !self.team.as_ref().is_some_and(|team| team.wheel.is_some())
            && self.terminal_close_up_mph(ctx).is_some()
    }

    /// Whether the light or sign at this ramp's end, not yet crossed, takes
    /// the pedals on from the ramp cap for a driver who left speed control
    /// on. The cap letting go there is a handover, not a release: the
    /// terminal is named a few hundred feet later and drives the truck up to
    /// its line, so "released" was untrue (owner, Cleveland, 2026-10-25).
    pub(crate) fn terminal_takes_over_from_the_cap(&self) -> bool {
        self.speed_control_armed
            && self.ramp_mi.is_some()
            && !self.ramp_terminal_done
            && matches!(
                self.ramp_control.as_str(),
                "signal" | "stop" | "yield" | "roundabout"
            )
    }

    /// Give the truck the accelerator up to the line when the assist closes
    /// up: only with nothing on the brake, so a driver's foot, or another
    /// assist braking for a vehicle ahead, wins.
    fn close_up_to_the_line(&mut self, ctx: &GameContext) {
        if !self.terminal_assist_closes_up(ctx) || self.trip.truck.brake > 0.05 {
            return;
        }
        let Some(target) = self.terminal_close_up_mph(ctx) else {
            return;
        };
        let short_by = target - self.trip.truck.speed_mph();
        if short_by > 0.0 {
            let truck = &mut self.trip.truck;
            truck.throttle = truck
                .throttle
                .max(LINE_CLOSE_UP_THROTTLE.min(short_by / 3.0).max(0.1));
        }
    }

    /// A live accelerator press overrides this assist, including its red-light hold.
    pub fn update_ramp_terminal_assist_with_input(
        &mut self,
        ctx: &mut GameContext,
        accelerating: bool,
    ) {
        // Standing down lets go of its own application. The frame holds the
        // pedal at this servo's last press (`assist_floor`), so a press left
        // behind when the terminal ended -- crossed, run, or the ramp left --
        // held the truck on its brakes for good (merge bench, 2026-09-23).
        if !self.terminal_assist_owns(ctx) {
            self.ramp_assist_brake = 0.0;
            return;
        }
        let Some(gap_mi) = self.terminal_gap_mi() else {
            return;
        };
        if accelerating {
            // Clear only this assist's held application. The input layer owns
            // the pedals; never erase a driver's brake or another assist's.
            self.ramp_assist_brake = 0.0;
            return;
        }
        if self.ramp_waiting_at_light {
            // Holding for green: the assist keeps the brakes on.
            self.trip.truck.throttle = 0.0;
            self.trip.truck.brake = 1.0;
            return;
        }
        let speed = self.trip.truck.speed_mph();
        if self.ramp_control == "signal" {
            let phase = self.ramp_light_phase();
            let must_stop = phase == "red" || (phase == "yellow" && gap_mi > 0.0);
            if !must_stop && self.on_street_control() {
                // A green on the streets is driven at the street's own
                // speed: the ramp's roll target is for the turn a ramp
                // terminal always is, and a corner here has its own call and
                // its own speed.
                self.ramp_assist_said = false;
                self.ramp_assist_brake = 0.0;
                return;
            }
            if !must_stop {
                // The green stands the servo down, and the NEXT red is a new
                // take: it has to announce itself again. Left latched, a
                // light that went red, green, then red again on the approach
                // braked the truck hard from ramp speed without a word --
                // "braking for the light" had been spent on the first red
                // (Tyler Cross-Dock replay, 2026-09-03).
                self.ramp_assist_said = false;
                // A green (or a yellow already at the bar) is legal to roll,
                // but not at speed. The lift alone left a truck arriving on
                // green at the ramp's own advisory, and it crossed "far too
                // fast" with the assist on (agent drive, Eagles Landing,
                // 2026-09-22; longer greens make that arrival the common
                // one). The assist's job is to take the truck THROUGH the
                // light, then facility assistance takes over (owner,
                // 2026-09-22). So the servo meets a roll target at the bar
                // instead of a stop, and lets go once the truck is under it -- a
                // measured application, never the held service floor that
                // spent reservoir air on the way down (Joshua, 2026-08-28).
                self.roll_the_terminal(ctx, GREEN_ROLL_MPH - 5.0, gap_mi, "the green light");
                self.close_up_to_the_line(ctx);
                return;
            }
        }
        if matches!(self.ramp_control.as_str(), "yield" | "roundabout") {
            let clear = self.yield_gap_clear();
            // Already stopped at the line waiting for a gap, the gap is the
            // hold's to announce and release ("Gap in traffic."), below. The
            // roll here used to catch it first: the gap came, nothing was
            // said, the terminal never counted as honored, and the truck sat
            // at the line under a promise that "assistance is holding for
            // your gap" (every-assist audit, 2026-09-24).
            if clear && !self.ramp_waiting_at_sign {
                // A clear yield is rolled, not stopped: the gap verdict lands
                // at the line. Braking to a dead stop on a clear yield is the
                // rear-end setup the roadmap warns the LEAD car will pull.
                //
                // Rolled at the yield's own speed, the way a green is: the
                // lift alone let a truck that came off the ramp curve at 19
                // reach the line over it, and it left the stop profile's last
                // press held by the frame's pedal floor, so a gap opening on
                // the approach braked the truck to a stand 270 feet short of
                // an empty line (every-assist audit, 2026-09-24).
                self.roll_the_terminal(ctx, YIELD_ROLL_MPH - 3.0, gap_mi, "the yield");
                return;
            }
            // Not clear: fall through and brake for the line like a stop.
        }
        if speed <= RED_STOP_MPH && gap_mi <= RAMP_ASSIST_HOLD_MI {
            // At the bar with the truck stopped: the assist owns the hold.
            self.trip.truck.throttle = 0.0;
            self.trip.truck.brake = 1.0;
            self.ramp_assist_brake = 0.0;
            if matches!(self.ramp_control.as_str(), "stop" | "yield" | "roundabout") {
                // The assist holds the stop; the release now waits for the
                // bubble's gap, same as an unassisted stop. The hold above
                // keeps the brakes on through the wait.
                let noun = match self.ramp_control.as_str() {
                    "stop" => "sign",
                    "yield" => "yield",
                    _ => "roundabout entry",
                };
                let blocked = if self.ramp_control == "stop" {
                    self.cross_bubble
                        .as_ref()
                        .is_some_and(|bubble| !bubble.clear_to_cross())
                } else {
                    !self.yield_gap_clear()
                };
                // "Stopped" is the sign's word: at the sign the hold is the
                // stop. A yield is held from the moment the truck is under
                // the stop speed, still creeping, and "Stopped at the yield"
                // was heard at 4 miles per hour (agent drive, exit 255,
                // 2026-09-24). "At the yield" is true at any creep.
                let at = if self.ramp_control == "stop" {
                    "Stopped at"
                } else {
                    "At"
                };
                if blocked {
                    if !self.ramp_waiting_at_sign {
                        self.ramp_waiting_at_sign = true;
                        let what = self.crossing_description();
                        self.say_terminal_hold(
                            ctx,
                            &format!("{at} the {noun}. {what}."),
                            SpeechCategory::Navigation,
                        );
                    }
                    return;
                }
                self.ramp_terminal_done = true;
                let lead = if self.ramp_waiting_at_sign {
                    "Gap in traffic.".to_string()
                } else {
                    format!("{at} the {noun}.")
                };
                self.ramp_waiting_at_sign = false;
                let message = self.terminal_release_text(ctx, &lead, true);
                self.say_route_navigation(ctx, &message);
            } else if !self.ramp_waiting_at_light {
                self.ramp_waiting_at_light = true;
                // ROUTE, not the ambient default: names an automation (the ramp
                // assist) that just took the brakes, same as the stop-sign
                // sibling above (automation-handoff sweep, 2026-08-20, the
                // deferred 2026-08-15 audit).
                self.say_terminal_hold(ctx, "Red light.", SpeechCategory::Confirmation);
            }
            return;
        }
        if speed <= RED_STOP_MPH {
            // Already stopped, but short of the hold window: a driver braking
            // on their own on top of the assist lands here, and a standing
            // truck has nothing left to brake for. The assist must hand the
            // pedals back -- pinning throttle at zero and the brake at its
            // floor against a truck that is already stopped is a hold with no
            // release, and the driver cannot move again (playtest softlock,
            // 2026-07-24). The queue guidance is what tells them to close the
            // gap to the bar from here. Dropping the held application matters
            // for the same reason: a creep to the bar must start from an open
            // pedal, not from whatever the approach was holding.
            self.ramp_assist_brake = 0.0;
            self.close_up_to_the_line(ctx);
            return;
        }
        // Brake down the approach: needed deceleration to stop at the bar,
        // recomputed each tick, mapped onto brake application. As the gap
        // closes the demand rises and the pedal follows.
        let gap_m = 0.5f64.max(gap_mi * 1609.344);
        let v_mps = 0.0f64.max(self.trip.truck.velocity_mps);
        let needed = (v_mps * v_mps) / (2.0 * gap_m);
        if needed < RAMP_ASSIST_DECEL_RELEASE_MPS2 && gap_m > 30.0 {
            self.ramp_assist_brake = 0.0;
            self.close_up_to_the_line(ctx);
            return;
        }
        let idle = self.ramp_assist_brake <= 0.0;
        if idle && needed < RAMP_ASSIST_DECEL_START_MPS2 && gap_m > 30.0 {
            self.close_up_to_the_line(ctx);
            return;
        }
        self.ramp_assist_brake =
            assist_servo_brake(self.ramp_assist_brake, needed, &self.trip.truck);
        self.trip.truck.throttle = 0.0;
        self.trip.truck.brake = self.trip.truck.brake.max(self.ramp_assist_brake);
        if !self.ramp_assist_said {
            self.ramp_assist_said = true;
            // A transit stop: the bar is honored and then driven away from, so
            // the session comes back on its own past it rather than waiting
            // for a departure that never happens on a ramp.
            self.pause_speed_control(ctx, true);
            let what = self.terminal_noun();
            // A yield whose gap closed on the roll is the same approach the
            // roll line already named: "slowing for the yield" then "braking
            // for the yield" back to back said one thing twice. A light is
            // different -- red after green is a new take and says so.
            let rolled_here = self.ramp_green_roll_said
                && matches!(self.ramp_control.as_str(), "yield" | "roundabout");
            if !rolled_here {
                self.say_route_confirmation(ctx, format!("Braking for the {what}."));
            }
        }
    }

    /// What the assist's lines call the control at the end of this ramp.
    fn terminal_noun(&self) -> &'static str {
        match self.ramp_control.as_str() {
            "signal" => "light",
            "yield" => "yield",
            "roundabout" => "roundabout",
            _ => "stop sign",
        }
    }

    /// The ramp cap's lift taking the pedals, said once.
    ///
    /// On a ramp that ends at a light or a sign the lift and the terminal's
    /// braking are one act by one assist -- slowing for what is at the end --
    /// and each used to announce itself: "Route-transition assistance
    /// slowing." then "Route-transition assistance braking for the yield." a
    /// second later (agent drive, yield at exit 255, 2026-09-24). So the lift
    /// names the terminal, and the terminal's own line for this approach is
    /// spent.
    pub(crate) fn say_ramp_lift(&mut self, ctx: &mut GameContext) {
        let controlled = self.ramp_mi.is_some()
            && !self.ramp_terminal_done
            && matches!(
                self.ramp_control.as_str(),
                "signal" | "stop" | "yield" | "roundabout"
            );
        if !controlled {
            // Once per ramp: the deceleration lane's assist or the arrival
            // has already said it is slowing the truck for this ramp, and a
            // second "Slowing for the ramp." came 11 seconds after the first
            // (owner, 2026-10-25).
            if !self.decel_lane_assist_said && !self.destination_arrival_active {
                self.say_route_confirmation(ctx, "Slowing for the ramp.");
            }
            return;
        }
        // Spend the line the terminal would have said for this same act: the
        // roll's on a green, the stop's on a red or a sign, either at a yield
        // (whose roll and stop are one approach).
        let what = match self.ramp_control.as_str() {
            "signal" if self.ramp_light_phase() == "green" => {
                self.ramp_green_roll_said = true;
                "green light"
            }
            "yield" | "roundabout" => {
                self.ramp_green_roll_said = true;
                self.ramp_assist_said = true;
                self.terminal_noun()
            }
            _ => {
                self.ramp_assist_said = true;
                self.terminal_noun()
            }
        };
        self.say_route_confirmation(ctx, format!("Slowing for the {what}."));
    }

    /// Take the truck through a terminal it may roll -- a green, or a yield
    /// with its gap -- at `roll_mph` or under by the bar.
    ///
    /// The servo meets a roll target at the bar instead of a stop, and lets
    /// go once the truck is under it: a measured application, never the held
    /// service floor that spent reservoir air on the way down (Joshua,
    /// 2026-08-28). Said once per roll (`ramp_green_roll_said`, which names
    /// the green it was first written for).
    fn roll_the_terminal(&mut self, ctx: &mut GameContext, roll_mph: f64, gap_mi: f64, what: &str) {
        if self.trip.truck.speed_mph() <= roll_mph {
            self.ramp_assist_brake = 0.0;
            return;
        }
        if gap_mi <= crate::states::driving_stops::bar_tick_range_mi(&self.trip.truck) {
            self.trip.truck.throttle = 0.0;
        }
        let gap_m = 0.5f64.max(gap_mi * 1609.344);
        let v_mps = 0.0f64.max(self.trip.truck.velocity_mps);
        let roll_mps = roll_mph / MPH_PER_MPS;
        let needed = (v_mps * v_mps - roll_mps * roll_mps).max(0.0) / (2.0 * gap_m);
        let idle = self.ramp_assist_brake <= 0.0;
        if needed < RAMP_ASSIST_DECEL_RELEASE_MPS2
            || (idle && needed < RAMP_ASSIST_DECEL_START_MPS2)
        {
            self.ramp_assist_brake = 0.0;
            return;
        }
        self.ramp_assist_brake =
            assist_servo_brake(self.ramp_assist_brake, needed, &self.trip.truck);
        self.trip.truck.throttle = 0.0;
        self.trip.truck.brake = self.trip.truck.brake.max(self.ramp_assist_brake);
        if !self.ramp_green_roll_said {
            self.ramp_green_roll_said = true;
            self.pause_speed_control(ctx, true);
            self.say_route_confirmation(ctx, format!("Slowing for {what}."));
        }
    }

    /// The terminal's servo has a stop still to make at the bar: a sign, or
    /// a light it is braking for or holding at.
    /// When this truck enters the crossroad past a yield line and when its
    /// rear clears it, in seconds from now, were it doing `speed_mph` here.
    fn yield_crossing_s(&self, speed_mph: f64) -> (f64, f64) {
        let to_line_ft = self.terminal_gap_mi().map_or(0.0, |gap_mi| gap_mi * 5280.0);
        let length_ft = if self.trip.truck.trailer_attached {
            COMBINATION_LENGTH_FT
        } else {
            TRACTOR_LENGTH_FT
        };
        yield_crossing_times_s(speed_mph, to_line_ft, length_ft)
    }

    /// Whether a yield's gap is there for THIS truck: nothing in the
    /// crossroad from when it enters to when its rear clears, timed at the
    /// roll speed it will cross at (or from a stand, if it is stopped).
    ///
    /// The cross bubble's own `clear_to_cross` looks four seconds ahead,
    /// which a car clears and a loaded tractor-semitrailer pulling away from
    /// the line does not: it needs about ten.
    pub fn yield_gap_clear(&self) -> bool {
        self.yield_blocker().is_none()
    }

    /// The vehicle that closes a yield's gap for this truck, if any.
    fn yield_blocker(&self) -> Option<&CrossVehicle> {
        let bubble = self.cross_bubble.as_ref()?;
        let roll_mph = self.trip.truck.speed_mph().min(YIELD_ROLL_MPH - 3.0);
        let (enter, exit) = self.yield_crossing_s(roll_mph);
        bubble.conflict_between(enter, exit)
    }

    /// The vehicle a stop at this terminal is waiting on: the one that shut
    /// the gap the release is waiting for, by the same test.
    fn terminal_blocker(&self) -> Option<&CrossVehicle> {
        if self.ramp_control == "stop" {
            self.cross_bubble.as_ref()?.blocker()
        } else {
            self.yield_blocker()
        }
    }

    /// What a truck rolling a yield meets as its front reaches the crossroad:
    /// a vehicle in the conflict window now is a hit; one that reaches it
    /// before the truck's rear has cleared the crossroad is a forced gap; a
    /// gap that holds all the way across is clean.
    fn yield_meeting(&mut self, speed_mph: f64) -> (CrossMeeting, Option<CrossVehicle>) {
        if self.cross_bubble.is_none() {
            // No bubble to consult (an older save mid-ramp): roll the same
            // seeded crossroad the terminal would have built.
            let _ = self.cross_violation_meets();
        }
        let (_, exit) = self.yield_crossing_s(speed_mph);
        let Some(bubble) = self.cross_bubble.as_ref() else {
            return (CrossMeeting::Empty, None);
        };
        if let Some(vehicle) = bubble.occupant() {
            return (CrossMeeting::Hit, Some(vehicle.clone()));
        }
        if let Some(vehicle) = bubble.conflict_between(0.0, exit) {
            return (CrossMeeting::Near, Some(vehicle.clone()));
        }
        (CrossMeeting::Empty, None)
    }

    pub(crate) fn ramp_terminal_owns_the_stop(&self) -> bool {
        self.terminal_live()
            && !self.ramp_terminal_done
            && self.ramp_light_announced
            && (self.ramp_assist_brake > 0.0
                || self.ramp_waiting_at_light
                || self.ramp_control == "stop")
    }

    /// "A semi crossing from the left", or "Cross traffic" with nothing near.
    ///
    /// Names the vehicle the wait is FOR. It used to name whichever vehicle
    /// was next to reach the crossing inside eight seconds, which skips one
    /// already in the crossroad: "A car crossing from the left" was said while
    /// the car and pickup actually holding the truck crossed in the right ear
    /// (agent drive, yield at exit 255, 2026-09-24).
    fn crossing_description(&self) -> String {
        match self.terminal_blocker() {
            Some(nearest) => format!(
                "A {} crossing from the {}",
                nearest.vehicle_class, nearest.from_side
            ),
            None => "Cross traffic".to_string(),
        }
    }

    /// Crossing the terminal: honor the light or the sign, or pay for it.
    ///
    /// A driver still braking gets the length of the grace distance past the
    /// bar to finish the stop; carrying speed beyond it commits the run.
    pub fn update_ramp_terminal(&mut self, ctx: &mut GameContext) {
        let speed = self.trip.truck.speed_mph();
        let past_bar = self
            .terminal_gap_mi()
            .is_some_and(|gap_mi| gap_mi <= -RAMP_TERMINAL_GRACE_MI);
        if self.ramp_control == "signal" {
            self.cross_traffic_light(ctx, speed, past_bar);
            return;
        }
        if self.ramp_control == "stop" {
            self.cross_stop_sign(ctx, speed, past_bar);
            return;
        }
        if matches!(self.ramp_control.as_str(), "yield" | "roundabout") {
            self.cross_yield(ctx, speed);
            return;
        }
        self.ramp_terminal_done = true;
    }

    fn cross_traffic_light(&mut self, ctx: &mut GameContext, speed: f64, past_bar: bool) {
        if self.ramp_light_is_red() {
            if speed <= RED_STOP_MPH {
                if !self.ramp_waiting_at_light {
                    self.ramp_waiting_at_light = true;
                    self.say_terminal_hold(
                        ctx,
                        "Stopped at the red light.",
                        SpeechCategory::Navigation,
                    );
                }
                return;
            }
            if !past_bar {
                return; // still braking down to the stop bar
            }
            self.ramp_terminal_done = true;
            self.ramp_waiting_at_light = false;
            // What the run actually meets is the bubble's answer now,
            // not the old certainty: cross traffic flows on the player's
            // red, so this usually finds a vehicle -- but a gambler who
            // threads a real gap gets away with it, exactly like the road.
            let (met, vehicle) = self.cross_violation_meets();
            let pan = if vehicle
                .as_ref()
                .is_none_or(|vehicle| vehicle.from_side == "left")
            {
                -0.4
            } else {
                0.4
            };
            let cue = Self::cross_vehicle_sound(vehicle.as_ref());
            let place = self.terminal_where();
            if speed > STOP_ROLL_CLIP_MPH {
                match met {
                    CrossMeeting::Hit => {
                        let severity = Self::cross_hit_severity(RED_RUN_DAMAGE, vehicle.as_ref());
                        let hit = Self::cross_hit_clause(vehicle.as_ref());
                        ctx.audio.play_with(&cue, 1.0, pan);
                        ctx.audio.play("vehicle/collision");
                        ctx.controller.rumble.impact(severity);
                        // A driver already hard on the brakes, carried through
                        // by the load, did not make a preventable mistake. The
                        // violation still stands; the discipline does not.
                        let preventable = !self.trip.truck.pushed_through_by_surge();
                        self.trip.truck.apply_collision(severity, preventable);
                        let damage = self.trip.truck.damage_pct;
                        self.say_safety_interrupt(
                            ctx,
                            &format!(
                                "You ran the red light{place} and {hit}! Total damage \
                                 {damage:.0} percent."
                            ),
                        );
                    }
                    CrossMeeting::Near => {
                        ctx.audio.play_with(&cue, 1.0, pan);
                        self.say_confirmation_interrupt(
                            ctx,
                            &format!(
                                "You ran the red light{place}. Cross traffic brakes hard and \
                                 leans on the horn."
                            ),
                        );
                    }
                    CrossMeeting::Empty => {
                        self.say_confirmation_interrupt(
                            ctx,
                            &format!("You ran the red light{place}. Nothing was crossing."),
                        );
                    }
                }
            } else if met == CrossMeeting::Empty {
                self.say_confirmation_interrupt(ctx, "Rolled the red light.");
            } else {
                ctx.audio.play_with(&cue, 1.0, pan);
                self.say_confirmation_interrupt(
                    ctx,
                    "Rolled the red light. Cross traffic leans on the horn.",
                );
            }
            self.cite_signal_run(ctx, "the red light", RED_LIGHT_FINE);
            return;
        }
        self.ramp_terminal_done = true;
        self.ramp_waiting_at_light = false;
        if self.on_street_control() {
            // Through a street's green at the street's own speed: the light
            // was named on the approach, and nothing about it is news.
            return;
        }
        let on_yellow = self.ramp_light_phase() == "yellow";
        // A green the driver already heard -- named on the approach, or
        // called as the light changed while the truck rolled toward it -- is
        // not news at the bar. Chiming and naming it again a few seconds later
        // was the light announcing green twice (owner, agent drive into
        // Milwaukee, 2026-10-04).
        let green_heard = self.ramp_light_announced && self.ramp_light_last_phase == "green";
        if !(green_heard && !on_yellow && speed <= GREEN_ROLL_MPH) {
            ctx.audio.play_with("events/ramp_light_green", 0.7, 0.0);
        }
        let message = if speed > GREEN_ROLL_MPH {
            "Through the light, far too fast. Stop at the entrance."
        } else if on_yellow {
            "Through on the yellow. Stop at the entrance."
        } else if green_heard {
            "Through the intersection. Stop at the entrance."
        } else {
            "Green light. Through the intersection. Stop at the entrance."
        };
        self.say_route_confirmation(ctx, message);
    }

    fn cross_stop_sign(&mut self, ctx: &mut GameContext, speed: f64, past_bar: bool) {
        if speed > RED_STOP_MPH && !past_bar {
            return; // still braking down to the stop bar
        }
        if speed <= RED_STOP_MPH {
            // Stopped at the sign: the clear call now waits for a real
            // gap in the cross bubble instead of arriving with the stop.
            // The crossing cues are the information -- each one is a
            // vehicle in the ear it comes from -- and "clear" is spoken
            // only when the window is genuinely open.
            let blocked = self
                .cross_bubble
                .as_ref()
                .is_some_and(|bubble| !bubble.clear_to_cross());
            if blocked {
                if !self.ramp_waiting_at_sign {
                    self.ramp_waiting_at_sign = true;
                    let what = self.crossing_description();
                    self.say_terminal_hold(
                        ctx,
                        &format!("Stopped at the sign. {what}; wait for your gap."),
                        SpeechCategory::Navigation,
                    );
                }
                return;
            }
            self.ramp_terminal_done = true;
            let lead = if self.ramp_waiting_at_sign {
                "Gap in traffic."
            } else {
                "Stopped at the sign."
            };
            self.ramp_waiting_at_sign = false;
            let message = self.terminal_release_text(ctx, lead, true);
            self.say_route_navigation(ctx, &message);
            return;
        }
        self.ramp_terminal_done = true;
        // Same honesty as the light: the bubble says what the blown sign
        // actually met. A stop-sign crossroad is often empty -- that is
        // what makes rolling one tempting, and what makes the day a
        // semi IS crossing the lesson it should be.
        let (met, vehicle) = self.cross_violation_meets();
        let pan = if vehicle
            .as_ref()
            .is_none_or(|vehicle| vehicle.from_side == "right")
        {
            0.4
        } else {
            -0.4
        };
        let cue = Self::cross_vehicle_sound(vehicle.as_ref());
        let place = self.terminal_where();
        if speed > STOP_ROLL_CLIP_MPH {
            match met {
                CrossMeeting::Hit => {
                    let severity = Self::cross_hit_severity(STOP_ROLL_DAMAGE, vehicle.as_ref());
                    let hit = Self::cross_hit_clause(vehicle.as_ref());
                    ctx.audio.play_with(&cue, 1.0, pan);
                    ctx.audio.play("vehicle/collision");
                    ctx.controller.rumble.impact(severity);
                    let preventable = !self.trip.truck.pushed_through_by_surge();
                    self.trip.truck.apply_collision(severity, preventable);
                    let damage = self.trip.truck.damage_pct;
                    self.say_safety_interrupt(
                        ctx,
                        &format!(
                            "You blew the stop sign{place} and {hit}! Total damage \
                             {damage:.0} percent."
                        ),
                    );
                }
                CrossMeeting::Near => {
                    ctx.audio.play_with(&cue, 1.0, pan);
                    self.say_confirmation_interrupt(
                        ctx,
                        &format!(
                            "You blew the stop sign{place}. Cross traffic brakes hard and leans \
                             on the horn."
                        ),
                    );
                }
                CrossMeeting::Empty => {
                    self.say_confirmation_interrupt(
                        ctx,
                        &format!("You blew the stop sign{place}. The crossroad was empty."),
                    );
                }
            }
        } else if met == CrossMeeting::Empty {
            self.say_confirmation_interrupt(ctx, &format!("Rolled the stop sign{place}."));
        } else {
            ctx.audio.play_with(&cue, 1.0, pan);
            self.say_confirmation_interrupt(
                ctx,
                &format!("Rolled the stop sign{place}. Cross traffic leans on the horn."),
            );
        }
        self.cite_signal_run(ctx, "the stop sign", STOP_SIGN_FINE);
    }

    /// Running the ramp-end light or sign risks a citation on the same rails
    /// as the chain-law checkpoint: one flat, seeded roll for whether anyone
    /// was watching the crossroad, then the career's own repeat scaling and
    /// the work-zone doubling. What the crossing met is decided separately
    /// by the cross bubble, so a blown light can cost the trailer, the fine,
    /// both, or nothing -- dice, not a fixed price (owner playtest
    /// 2026-07-15: "ALWAYS clips cross traffic and never draws a citation").
    ///
    /// Money rides ROUTE's never-dropped queue, like every other citation:
    /// the collision line that may precede it is an interrupt, and a busy
    /// stretch must not age the figure out.
    fn cite_signal_run(&mut self, ctx: &mut GameContext, offense: &str, base_fine: f64) {
        if ctx.profile.is_none() || self.enforcement_bypassed(ctx) {
            return;
        }
        let at_mi = self.terminal_seed_mi();
        let mut rng = PyRandom::new_from_str(&format!("{}:signal-run:{at_mi:.1}", self.trip_seed));
        if rng.random() >= SIGNAL_RUN_CATCH_CHANCE {
            return;
        }
        if self.new_driver_warning(ctx) {
            ctx.audio.play("ui/warning");
            ctx.say_event_with(
                format!(
                    "A patrol car at the crossroad saw you run {offense} and gives you a warning."
                ),
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Navigation),
            );
            return;
        }
        let zone = self.trip.in_construction_zone();
        let fine = citation_fine(base_fine, career_citations(profile_of(ctx)), zone, None);
        let money = {
            let p = profile_mut_of(ctx);
            p.spend(fine);
            p.money()
        };
        self.ticket_fines_paid += fine;
        let saw_it = match self.trip.active_post_at(self.trip.position_mi) {
            Some(post) => format!("A trooper working this {} saw it", post.reason()),
            None => "A patrol car sitting at the crossroad saw it".to_string(),
        };
        // Not a serious violation under 49 CFR 383.51 Table 2: the citation
        // goes on the record and scales the next fine, nothing more.
        self.log_enforcement(ctx, fine, false, false, &format!("Ran {offense}"));
        ctx.audio.play("ui/error");
        ctx.say_event_with(
            format!(
                "{saw_it}. Running {offense} is a citation, {} dollars.{} You have {} dollars.",
                fmt_grouped(fine, 0),
                construction_zone_fine_clause(zone),
                fmt_grouped(money, 0)
            ),
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Money),
        );
    }

    /// The yield rule, straight from the sign: a gap taken at roll speed is
    /// the clean crossing, stopping is always legal, and an occupied window is
    /// the clip machinery -- at THEIR closing speed, because you rolled under
    /// their bumper.
    ///
    /// Judged where the truck meets cross traffic, not at an arbitrary point:
    /// the crossroad begins a few feet past the yield line, and the gap has to
    /// hold for as long as the truck takes to get its whole length across
    /// (`yield_meeting`).
    fn cross_yield(&mut self, ctx: &mut GameContext, speed: f64) {
        let noun = if self.ramp_control == "roundabout" {
            "roundabout"
        } else {
            "yield"
        };
        if speed <= RED_STOP_MPH {
            // Stopped: exactly the stop sign's wait, spoken for a yield --
            // except the gap it waits for is one this truck can pull across
            // from a stand.
            let blocked = !self.yield_gap_clear();
            if blocked {
                if !self.ramp_waiting_at_sign {
                    self.ramp_waiting_at_sign = true;
                    let what = self.crossing_description();
                    self.say_terminal_hold(
                        ctx,
                        &format!("At the {noun}. {what}; wait for your gap."),
                        SpeechCategory::Navigation,
                    );
                }
                return;
            }
            self.ramp_terminal_done = true;
            self.ramp_waiting_at_sign = false;
            let message = self.terminal_release_text(ctx, "Gap in traffic.", true);
            self.say_route_navigation(ctx, &message);
            return;
        }
        // Rolling: the verdict lands where the truck's front reaches the
        // crossroad. It used to wait for the stop bar's grace distance, about
        // a hundred feet past the line, so a gap that was clear at the line
        // and held through the crossing could read as forced by a car that
        // arrived after the truck had gone (fix/yield-at-the-line,
        // 2026-09-24).
        let at_the_crossroad = self
            .terminal_gap_mi()
            .is_some_and(|gap_mi| gap_mi <= -YIELD_LINE_TO_CROSSROAD_FT / 5280.0);
        if !at_the_crossroad {
            return; // still rolling down to the crossroad; the gap decides there
        }
        self.ramp_terminal_done = true;
        let (met, vehicle) = self.yield_meeting(speed);
        let pan = if vehicle
            .as_ref()
            .is_none_or(|vehicle| vehicle.from_side == "right")
        {
            0.4
        } else {
            -0.4
        };
        let cue = Self::cross_vehicle_sound(vehicle.as_ref());
        if met == CrossMeeting::Hit {
            let severity = Self::cross_hit_severity(STOP_ROLL_DAMAGE, vehicle.as_ref());
            // "into cross traffic and cross traffic clipped" reads twice; the
            // yield line already names what was rolled into.
            let hit = Self::cross_hit_clause(vehicle.as_ref())
                .replace("cross traffic clipped", "it clipped");
            ctx.audio.play_with(&cue, 1.0, pan);
            ctx.audio.play("vehicle/collision");
            ctx.controller.rumble.impact(severity);
            let preventable = !self.trip.truck.pushed_through_by_surge();
            self.trip.truck.apply_collision(severity, preventable);
            let damage = self.trip.truck.damage_pct;
            self.say_safety_interrupt(
                ctx,
                &format!(
                    "You rolled the {noun} into cross traffic and {hit}! Total damage \
                     {damage:.0} percent."
                ),
            );
        } else if met == CrossMeeting::Near {
            ctx.audio.play_with(&cue, 1.0, pan);
            self.say_confirmation_interrupt(
                ctx,
                &format!(
                    "You forced the gap at the {noun}. Cross traffic brakes hard and leans on the \
                     horn."
                ),
            );
        } else if speed > YIELD_ROLL_MPH {
            let tail = if self.on_street_control() {
                ""
            } else {
                " Stop at the entrance."
            };
            self.say_route_confirmation(ctx, format!("Through the {noun}, far too fast.{tail}"));
        } else {
            let message =
                self.terminal_release_text(ctx, &format!("Through the {noun} in a gap."), false);
            self.say_route_confirmation(ctx, &message);
        }
    }

    /// The terminal is honored and the way is clear: who pulls ahead.
    ///
    /// The old release handed the last stretch back to the driver even with
    /// facility stopping assistance on, so the truck sat at idle at a clear
    /// sign until the driver drove it up to the entrance, where the assist
    /// took the pedals again (agent drive, Chicago to Gary, 2026-09-01). The
    /// owner's ruling: the assist goes from the signal to the entrance, hands
    /// off. With it on, the release arms the pull-ahead and says no more;
    /// with it off, the release is the driver's, exactly as before.
    ///
    /// `clear` is the stopped release ("Clear; pull ahead"); a green or a gap
    /// rolled through says only what happened.
    pub(crate) fn terminal_release_text(
        &mut self,
        ctx: &GameContext,
        lead: &str,
        clear: bool,
    ) -> String {
        if self.on_street_control() {
            return self.street_release_text(ctx, lead, clear);
        }
        // A facility with a street chain is miles past this terminal: "the
        // entrance" there was followed by "5 miles to the facility gate"
        // (agent drive into Abilene, 2026-09-22).
        let whither = if self.ramp_continues_to_destination_streets(ctx) {
            "onto the streets"
        } else {
            "to the entrance"
        };
        if self.approach_pull_ahead_available(ctx) {
            self.approach_pull_ahead = true;
            let clear = if clear { " Clear." } else { "" };
            return format!("{lead}{clear}");
        }
        if clear {
            format!("{lead} Clear; pull ahead {whither}.")
        } else {
            format!("{lead} Pull ahead {whither}.")
        }
    }

    /// One interrupting SAFETY line.
    pub(crate) fn say_safety_interrupt(&self, ctx: &mut GameContext, message: &str) {
        let mut opts = SayEvent::new();
        opts.category = Some(SpeechCategory::Safety);
        ctx.say_event_with(message.to_string(), opts);
    }

    /// One interrupting CONFIRMATION line.
    pub(crate) fn say_confirmation_interrupt(&self, ctx: &mut GameContext, message: &str) {
        let mut opts = SayEvent::new();
        opts.category = Some(SpeechCategory::Confirmation);
        ctx.say_event_with(message.to_string(), opts);
    }
}
