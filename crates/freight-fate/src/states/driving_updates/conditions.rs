//! Standing physical conditions: hot brakes, the destination approach
//! assist, the traction states, and the chain law.

use crate::speech::EventPriority;
use ff_core::models::enforcement::{chain_law_fine, CHAIN_CARRY_FINE_CO};
use ff_core::pyfmt::{fmt_f, fmt_grouped};
use ff_core::pyrandom::PyRandom;
use ff_core::sim::vehicle::MIN_STOPPING_DECEL_MPS2;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_stops::arrival_servo_brake;

/// One step of waiting out a chain law at a checkpoint.
pub const CHAIN_WAIT_STEP_MIN: f64 = 30.0;
/// The longest single wait at a checkpoint: 8 hours.
pub const CHAIN_WAIT_MAX_MIN: f64 = 480.0;
/// The pace of the scripted drive back from a chain checkpoint.
pub const CHAIN_TURN_BACK_MPH: f64 = 45.0;
/// The shortest turn back: finding a place to turn a rig around.
pub const CHAIN_TURN_BACK_MIN_MIN: f64 = 5.0;

/// Which number a facility-lane hold is keeping: see
/// `DrivingState::facility_lane_hold_mph`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LaneHold {
    /// The lane's own posted number, with the stop still far enough off.
    Lane,
    /// The comfort-rate shed to the gate creep, now under the lane's number.
    Shed,
}

impl DrivingState {
    /// Squeal when hot brakes are worked past their fade temperature.
    pub fn update_brake_heat_cue(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.brake_squeal_cooldown_s > 0.0 {
            self.brake_squeal_cooldown_s = (self.brake_squeal_cooldown_s - dt).max(0.0);
            return;
        }
        let t = &self.trip.truck;
        if t.brake >= 0.4 && t.speed_mph() > 10.0 && t.brake_temp_c >= t.specs.brake_fade_temp_c {
            ctx.audio.play_with("vehicle/brake_squeal", 0.8, 0.0);
            self.brake_squeal_cooldown_s = 4.0;
        }
    }

    /// Ease the truck down so it ARRIVES stopped, not so it stops on arrival.
    ///
    /// The setting promises "slows and stops at the selected facility
    /// arrival point". Only the stopping half existed: the arrival gate
    /// applies full brake, and it runs inside `if self.trip.finished` --
    /// true only once the truck is AT the point. So the assist could hold a
    /// truck that had already stopped, and nothing more. The owner drove a
    /// delivery to Odessa, braked himself, and the assist announced "stopped
    /// and holding" as though it had done it (2026-08-19: "it did not stop
    /// me. I stopped").
    ///
    /// Priced like the exit assist's ramp glide rather than as a fixed
    /// trigger distance: road speed stands until the truck is inside the
    /// distance it needs to shed, then the cap follows the deceleration
    /// down. A driver already slower than the cap is left alone; this never
    /// steers, and the only speed it ever adds is a walk over the last
    /// lengths to the point, because the dock opens at the point and not a
    /// truck-length short of it.
    pub fn update_destination_approach_assist(&mut self, ctx: &mut GameContext) {
        if !ctx.settings.destination_approach_assist {
            self.destination_arrival_active = false;
            self.destination_assist_brake = 0.0;
            return;
        }
        // A controlled ramp terminal is part of the route, not the facility
        // gate. Let route-transition assistance own its mandatory stop first;
        // otherwise the full stop at a red light satisfies the arrival's creep
        // branch and latches the truck at two miles per hour after green. Once
        // the terminal releases the driver, the arrival is evaluated afresh
        // against the actual distance left to the destination.
        if self.ramp_mi.is_some()
            && !self.ramp_terminal_done
            && matches!(
                self.ramp_control.as_str(),
                "signal" | "stop" | "yield" | "roundabout"
            )
        {
            self.destination_arrival_active = false;
            self.destination_assist_brake = 0.0;
            return;
        }
        // HOW FAR TO THE GATE -- which is not the same as how far to the end
        // of the route. trip.remaining_miles measures the route, and it stays
        // parked while the truck is on the ramp: the harness showed it reading
        // 3.200 mi with the truck crawling yards from the market, so the cap
        // came out at 215 mph and the assist waved the truck through. The
        // arrival lives on the ramp instead -- ramp_mi counts down from the
        // ramp's own length to the stop, and the dock opens when it reaches zero
        // at docking speed; anything faster is a blown stop and the driver is
        // told they drove past (owner, three runs, 2026-08-19/20).
        //
        // A same-city street chain to a gate has no ramp, so that route shape
        // still measures off the route, which for it is the same thing.
        //
        // And a ramp that hands off to a street chain is NEITHER: its end is a
        // driving continuation, not the gate, and the chain's own trip carries
        // the arrival a mile further on. Treating it as the gate stopped the
        // truck dead at the bottom of the ramp with the city still to drive
        // (owner, Spokane, 2026-08-22).
        //
        // `outbound` is the trip's own word for which END of a facility's
        // streets the gate is at, and it is why a street chain alone is not
        // enough to arrive on. The origin's chain is the SAME streets driven
        // the other way -- same shape, same route test -- so a departure read
        // as an arrival, latched on the on-ramp as though it were the dock,
        // and braked the truck to a walk on the way OUT of the yard. It did
        // this on all twenty-five chain departures swept: every one of them
        // heard "Destination approach assistance slowing" with the delivery
        // still a whole run away, and lost the pedals for the merge -- three
        // to four miles an hour off the taper even once the lane itself ran
        // on the real clock, and the spoken line untrue at every one.
        // Every ramp stop is a facility entrance once its route control has
        // cleared: pickups/deliveries, a planned rest stop, and a required
        // scale all deserve the same progressive stop profile.
        let ramp_is_facility = self.ramp_stop.is_some();
        let ramp_continues_to_destination_streets = self.ramp_continues_to_destination_streets(ctx);
        if ramp_continues_to_destination_streets && self.approach_pull_ahead {
            // The bar to the streets, hands off. The terminal released the
            // truck to this assist (`terminal_release_text`), and on a chain
            // facility the streets are still ahead: the arrival is theirs, not
            // this ramp's, so nothing here latches or aims at a point. The
            // assist runs the truck up to the ramp's posted limit -- road,
            // not a gate (owner, 2026-09-01: "speed up to the limit to get to
            // that point as efficiently as it can") -- to the ramp's end,
            // where `begin_surface_chain` takes the truck at that speed and
            // hands the streets to the speed keeper. Throttle only ever
            // raised, never zeroed: a driver already on the pedal is not
            // fought, and a foot on the brake wins outright.
            self.destination_arrival_active = false;
            if let Some(ramp_mi) = self.ramp_mi.filter(|mi| *mi > 0.0) {
                // The street's limit past the bar, never the mainline's: the
                // milepost holds at the gore while the truck is on the ramp,
                // so the limit "here" was the interstate's, and the assist
                // wound a truck leaving a stop bar toward 70 on the way into
                // town (every-assist audit, 2026-09-24). With no street zone
                // to read, the facility access limit stands in.
                let limit_mph = self
                    .street_limit_past_bar_mph()
                    .unwrap_or(ff_core::sim::trip_models::FACILITY_ACCESS_LIMIT_MPH);
                let target_mph = limit_mph.max(FACILITY_LANE_ROLL_MPH);
                self.hold_approach_speed_with(
                    ramp_mi * 1609.344,
                    target_mph,
                    APPROACH_ROLL_THROTTLE_MAX,
                );
            }
            return;
        }
        let on_facility_ramp =
            self.ramp_mi.is_some() && ramp_is_facility && !ramp_continues_to_destination_streets;
        let remaining_mi = if on_facility_ramp {
            self.ramp_mi
        } else if !self.trip.finished
            && self.trip.is_facility_approach_route()
            && !self.trip.outbound
        {
            Some(self.trip.remaining_miles())
        } else {
            None
        };
        let Some(remaining_mi) = remaining_mi else {
            self.destination_arrival_active = false;
            return;
        };
        // THE STREETS' CORNERS ARE THE KEEPER'S. On a facility's own street
        // route the arrival is the final shed to the gate and nothing else;
        // a judged corner still ahead belongs to the speed keeper, which
        // slows for it. Latching with one ahead priced the stop at the gate
        // and ignored the corner: the assist took the pedals at 48 mph with
        // an 11 mph left turn a sixth of a mile on, paused the keeper that
        // would have slowed for it, and sailed past the turn; once the truck
        // was back on the approach the latch held a walk for the last
        // quarter mile and the truck never reached the yard until the
        // driver switched the assist off (tester, Elkhart Manufacturing
        // Plant, 2026-10-14). So no latch while a corner is ahead, and a
        // latch already made -- a truck put back before a corner -- lets go
        // and hands the keeper back.
        let on_streets = !on_facility_ramp && self.ramp_mi.is_none();
        if on_streets {
            let position = self.trip.position_mi;
            let corner_ahead = self
                .turn_cue_in_play_read()
                .is_some_and(|cue| cue.at_mi > position);
            if corner_ahead {
                if self.destination_arrival_active {
                    self.destination_arrival_active = false;
                    self.destination_assist_brake = 0.0;
                    if self.speed_control_paused_at_stop {
                        // The arrival's pause is held until departure; this
                        // one ends as soon as the truck is rolling.
                        self.speed_control_transit_pause = true;
                    }
                }
                return;
            }
        }
        // No margin held back, deliberately. Stopping short is not a safer
        // version of stopping: the dock opens at the END of the ramp, so a
        // truck halted two hundred feet shy of it with the brake held is a
        // truck that never arrives -- which is what a reserve of exactly that
        // size did on the first run after the clock fix. The gentle rate below
        // is the margin.
        let remaining_m = remaining_mi.max(0.0) * 1609.344;
        if remaining_m <= 0.0 {
            // At the point or past it: whatever is still on has to come off.
            if self.trip.truck.speed_mph() > DOCKING_MAX_MPH {
                self.trip.truck.throttle = 0.0;
                self.trip.truck.brake = 1.0;
                if self.cruise_mph.is_some() || self.keeper_mph.is_some() {
                    self.pause_speed_control(ctx, false);
                }
            }
            return;
        }
        // WHEN THE ARRIVAL TAKES THE PEDALS. The trigger is the game's ordinary
        // approach comfort curve with a pedal-build allowance in front of it --
        // the truck spends `lag` seconds at its current speed while the brake
        // comes up, THEN sheds at `a`:
        //
        //     v * lag  +  v^2 / (2a)  =  remaining
        //
        // whose positive root is the fastest it may still be doing and stop in
        // the road it has left. Solved, not approximated: taking the lag off
        // the distance first and then applying sqrt(2 a d) under-triggers,
        // because it prices the lag at the speed AFTER the shed. And priced at
        // the comfort rate, not the firmer rate the servo below will brake at:
        // triggering on the firm rate meant starting at the last metre that
        // could possibly work, and a throttle still decaying ate the margin.
        // NET OF THE SURGE. A tank the liquid can move in gives some of the
        // rate back at the worst moment, so a profile priced at the dry-van
        // comfort rate starts too late and arrives over the number. The
        // penalty is the truck's own -- a property of the tank and how full
        // it is, which is why a stopping distance built on it is a number the
        // driver can learn -- and the ramp bar has used it since it was
        // written. It reaches the facility arrival here for the same reason
        // every CDL manual gives: with a liquid load you brake earlier
        // (owner, 2026-09-20; FMCSA Cargo Tank Incidents Study on partial
        // loads and surge).
        let a = (APPROACH_DECEL_MPS2 - self.trip.truck.surge_decel_penalty_mps2())
            .max(MIN_STOPPING_DECEL_MPS2);
        let lag = APPROACH_ASSIST_REACTION_S;
        let cap_mps = -a * lag + ((a * lag).powi(2) + 2.0 * a * remaining_m).sqrt();
        if !self.destination_arrival_active {
            // A driver who braked the pull-ahead off has the last stretch back
            // for this ramp: the latch from standstill stays off, and only a
            // truck over the curve is taken (the setting's core promise).
            let facility_final_approach = self.ramp_mi.is_some()
                && ramp_is_facility
                && self.ramp_terminal_done
                && !self.approach_pull_ahead_canceled;
            if !facility_final_approach && self.trip.truck.velocity_mps <= cap_mps {
                return;
            }
            // LATCHED from here to the gate. This used to re-decide every
            // frame against the curve: over it, brake; under it, stand down
            // and hand the pedals back. Standing down is what zeroed the servo
            // and -- until the zone keeper learned to stay paused -- let the
            // keeper wind the truck back up toward the street limit, so the
            // assist chased a hair over the curve the whole way down and the
            // truck crossed the arrival point at 13.8 mph with the driver's
            // foot the only thing that stopped it (owner, Spokane,
            // 2026-08-21; Odessa, 2026-08-19, whose fix covered the ramp).
            // An arrival that has begun does not un-begin.
            self.destination_arrival_active = true;
            self.destination_assist_brake = 0.0;
            // ROUTE, not the ambient default: an automation naming that it
            // has the pedals, the same class as "Route-transition assistance
            // slowing". This assist used to take the brakes WITHOUT A WORD --
            // the only line it owned fired after the truck was already
            // stopped -- which for a blind driver is indistinguishable from
            // an assist that is not working, and is the likeliest reason it
            // was reported three times as "it did not stop me".
            //
            // Unless the terminal release already armed the pull-ahead: the
            // truck moving off the bar is that handoff, and saying it here
            // too announced it twice.
            if !self.approach_pull_ahead {
                ctx.say_event_with(
                    "Slowing for the entrance.",
                    SayEvent::queued()
                        .priority(EventPriority::Important)
                        .category(SpeechCategory::Confirmation),
                );
            }
        }
        // The arrival owns the pedals: throttle off, and automatic speed
        // control paused the ARRIVAL way -- held until departure, never lifted
        // on its own -- so nothing holds a street limit against the shed.
        self.trip.truck.throttle = 0.0;
        if self.cruise_mph.is_some() || self.keeper_mph.is_some() {
            self.pause_speed_control(ctx, false);
        }
        // HOW HARD. The stop profile itself, from the moment the arrival
        // begins, recomputed each tick and mapped onto the pedal by
        // `arrival_servo_brake` -- which, unlike the ramp assist's servo,
        // has no floor, because a gate is not a bar. Floored at the ramp's
        // start rate the arrival waited until the road needed 0.6 m/s2, about
        // 35 metres out at street speed, then chased a demand that climbs
        // faster than the pedal follows and crossed the gate at 12 mph with
        // the brake at full.
        //
        // The profile aims at a WALK at the point, not at rest, and it asks
        // the brake only for the share of that the road is not already
        // taking. A profile to zero speed is exact for a truck whose only
        // retarder is the brake; the real one has rolling resistance, drag
        // and grade shedding speed underneath it, and the servo holds the
        // pedal a band above the demand, so the truck always came down
        // harder than the profile -- and an arrival that undershoots does
        // not self-correct: the demand falls as the square of the speed while
        // the road left falls linearly, so it converges on a stop SHORT of
        // the point, with the brake held and the throttle forced to zero.
        // Nine metres short at two hundredths of a mile an hour, the dock
        // never opening, is what Jerry's Hobbs arrival looked like
        // (2026-08-22); the bench found the street chain does the same thing
        // on an upgrade. The dock opens only AT the point, so the last
        // lengths are a creep the assist holds, throttle against the road if
        // it has to, until the point's own full-brake branch above stops it.
        //
        // A facility lane is road until the final truck-lengths, so on one the
        // pace is the road's own number brought down on the comfort profile
        // (`facility_lane_hold_mph`), not a flat walk. The flat 12 mph walk
        // held from wherever the latch fired: on a ramp with no terminal
        // control -- a scale, a freeway-to-freeway ramp, or the dice -- that
        // is the top of the ramp, and the truck crawled the whole half mile
        // at 12 with the lane posted at 15 or more (owner, 2026-09-03).
        // And the facility's own streets the same way: the stretch from the
        // last corner to the gate is road too. Without this a street arrival
        // fell through to the bare gate creep below, and any truck under the
        // stop profile when the latch fired -- slowed for the last corner,
        // or put back on the approach -- coasted down to two miles an hour
        // and walked the rest of the way in (the same crawl the ramp lost
        // on 2026-09-03, still alive on the streets).
        if (on_facility_ramp || on_streets) && remaining_mi > ARRIVAL_FINAL_CREEP_MI {
            let (hold_mph, lane) = self.facility_lane_hold_mph(remaining_m);
            // Road, not a gate: the pedal that runs a chain ramp up to its
            // limit, unless the hold is down at the walk anyway.
            let throttle_max = if hold_mph > FACILITY_LANE_ROLL_MPH {
                APPROACH_ROLL_THROTTLE_MAX
            } else {
                ARRIVAL_CREEP_THROTTLE_MAX
            };
            self.hold_approach_speed_profile(
                remaining_m,
                hold_mph,
                ARRIVAL_CREEP_MPH,
                Some(lane),
                throttle_max,
            );
            return;
        }
        self.hold_approach_speed(remaining_m, ARRIVAL_CREEP_MPH);
    }

    /// The pace the arrival keeps on a facility lane `remaining_m` short of
    /// the gate: the lane's own number -- the posted limit here, and never
    /// more than the ramp's advisory speed -- or the comfort-rate shed that
    /// reaches the gate creep exactly at the entrance, whichever is lower.
    /// Never below the facility-lane roll, so a lane with no posted number
    /// still moves. The final creep stretch is the caller's. The second
    /// half says which of the two the hold is.
    fn facility_lane_hold_mph(&mut self, remaining_m: f64) -> (f64, LaneHold) {
        let (posted_mph, _) = self.trip.speed_limit_at(self.trip.position_mi);
        // On the facility's streets there is no ramp advisory to cap it;
        // the street's own posted number is the lane's.
        let lane_mph = if self.ramp_mi.is_some() {
            posted_mph.min(self.ramp_cap_here_mph())
        } else {
            posted_mph
        };
        let creep = ARRIVAL_CREEP_MPH / 2.23694;
        let profile_mph =
            (creep * creep + 2.0 * APPROACH_DECEL_MPS2 * remaining_m.max(0.0)).sqrt() * 2.23694;
        let kind = if profile_mph < lane_mph {
            LaneHold::Shed
        } else {
            LaneHold::Lane
        };
        (profile_mph.min(lane_mph).max(FACILITY_LANE_ROLL_MPH), kind)
    }

    /// The stop profile's pedal for this frame: brake down to `target_mph`
    /// over `remaining_m`, or hold it with a capped throttle nudge.
    ///
    /// Shared by the arrival latch above and the chain ramp's roll to the
    /// streets. Nothing is added while anyone has a foot on the brake, and the
    /// throttle is only ever raised, so a driver's own pedal always wins.
    fn hold_approach_speed(&mut self, remaining_m: f64, target_mph: f64) {
        self.hold_approach_speed_with(remaining_m, target_mph, ARRIVAL_CREEP_THROTTLE_MAX);
    }

    /// `hold_approach_speed` with the pedal ceiling chosen by the caller: a
    /// creep to a point, or the chain ramp's run up to the posted limit.
    fn hold_approach_speed_with(&mut self, remaining_m: f64, target_mph: f64, throttle_max: f64) {
        self.hold_approach_speed_profile(remaining_m, target_mph, target_mph, None, throttle_max);
    }

    /// The general form: keep `hold_mph`, and once over it brake only as
    /// hard as the profile that reaches `end_mph` at the point asks -- which
    /// a long lane out is next to nothing. A truck over the lane's number
    /// far from the gate is lifted and left to drag, the rule the ramp cap
    /// already follows: a service floor held down a whole ramp spent the air
    /// the gate needed (Shelby, on the bench, with the tanks still building).
    ///
    /// `lane` is what a facility-lane hold is: the lane's number, kept on a
    /// downgrade by the brake that balances the grade; or the shed itself,
    /// under which the brake eases off with the shortfall rather than letting
    /// go outright, so the pedal is continuous across the hold. Treating the
    /// two sides of a shed differently -- the profile's rate a hair over,
    /// nothing a hair under -- put a step in the pedal at the hold and the
    /// servo climbed that step every few frames down the shed. `None` is the
    /// gate creep and the chain ramp's roll, whose hold is the throttle's
    /// alone.
    fn hold_approach_speed_profile(
        &mut self,
        remaining_m: f64,
        hold_mph: f64,
        end_mph: f64,
        lane: Option<LaneHold>,
        throttle_max: f64,
    ) {
        let remaining_m = remaining_m.max(0.5);
        let v = self.trip.truck.velocity_mps;
        let creep = hold_mph / 2.23694;
        let end = end_mph / 2.23694;
        // What the road takes off on its own, m/s2: positive when it slows
        // the truck, negative when gravity is pushing it down to the gate.
        let road = self.trip.truck.resistance_force() / self.trip.truck.gross_mass_kg();
        let needed = 0.0f64.max(v * v - end * end) / (2.0 * remaining_m);
        if v > creep {
            self.destination_assist_brake = arrival_servo_brake(
                self.destination_assist_brake,
                needed - road,
                &self.trip.truck,
            );
            self.trip.truck.brake = self.trip.truck.brake.max(self.destination_assist_brake);
            return;
        }
        // At the hold, short of the point. On a downgrade that is pushing
        // the truck, keeping the pace is the BRAKE's job: the pedal that
        // balances the grade, eased off as the truck falls under the hold
        // and never dropped outright. Dropping it re-applied the pedal every
        // other frame -- gravity took the truck back over the hold, the
        // brake came on, the truck fell under, the brake let go -- 7 psi a
        // half-second down Shelby's ramp until the spring brakes set a third
        // of a mile short (bench, 2026-09-03).
        let balance = match lane {
            // Eases in proportion to the shortfall and reaches zero a little
            // under the profile, so a truck well under it is driven up to it
            // on the throttle. Keeping the rate that still reached the gate
            // from the current speed instead held a hair of brake the moment
            // the coast alone could not make the gate -- at 12 mph, from the
            // bar, which is the crawl this change is for.
            Some(LaneHold::Shed) => {
                Some(APPROACH_DECEL_MPS2 - ARRIVAL_CREEP_THROTTLE_GAIN * (creep - v) - road)
            }
            Some(LaneHold::Lane) if road < 0.0 => {
                Some(-road - ARRIVAL_CREEP_THROTTLE_GAIN * (creep - v))
            }
            _ => None,
        };
        if let Some(balance) = balance {
            self.destination_assist_brake = if balance > 0.0 {
                arrival_servo_brake(self.destination_assist_brake, balance, &self.trip.truck)
            } else {
                0.0
            };
            self.trip.truck.brake = self.trip.truck.brake.max(self.destination_assist_brake);
            if self.destination_assist_brake > 0.0 {
                return;
            }
        } else {
            self.destination_assist_brake = 0.0;
        }
        // Otherwise the brake is off and the throttle holds the pace -- the
        // road's balancing pedal plus a nudge for the shortfall, capped so it
        // is a creep and never a launch. Nothing is added while anyone else
        // has a foot on the brake: a driver braking at the gate, the hazard
        // assist, a stop ahead all win.
        if self.trip.truck.brake > 0.0 || self.trip.truck.parking_brake {
            return;
        }
        let tr = &self.trip.truck.transmission;
        if !tr.automatic && (tr.clutch > 0.5 || tr.shifting() || tr.in_neutral()) {
            return; // the driver is shifting; throttle would only rev the engine
        }
        let pedal = self.trip.truck.hold_throttle() + ARRIVAL_CREEP_THROTTLE_GAIN * (creep - v);
        self.trip.truck.throttle = self
            .trip
            .truck
            .throttle
            .max(throttle_max.min(pedal.max(0.0)));
    }

    /// Whether this delivery's ramp hands off to a street chain rather than
    /// ending at the gate (see `destination_street_chain_ahead`).
    ///
    /// A road stop with its own streets from the exit to its lot continues
    /// the same way (`begin_stop_chain`).
    pub(crate) fn ramp_continues_to_destination_streets(&mut self, ctx: &GameContext) -> bool {
        match self.ramp_stop.clone() {
            Some(stop) if stop.stop_type == "delivery_destination" => {
                self.destination_street_chain_ahead(ctx)
            }
            Some(stop) => self.stop_chain_route(&stop).is_some(),
            None => false,
        }
    }

    /// Whether facility stopping assistance can take the truck on from the
    /// terminal it is at, hands off, all the way to the entrance hold.
    ///
    /// A plain ramp ends at the gate and this assist's own profile drives it.
    /// A chain facility's streets are the speed keeper's, so with the keeper
    /// off the promise "taking you to the entrance" could not be kept and the
    /// release stays the driver's. A driver who braked the pull-ahead off has
    /// said no for this ramp.
    pub(crate) fn approach_pull_ahead_available(&mut self, ctx: &GameContext) -> bool {
        if self.on_street_control() {
            // A light or sign on the streets: the speed keeper is what drives
            // on from it, so the same promise needs the keeper.
            return ctx.settings.destination_approach_assist
                && ctx.settings.speed_keeper
                && !self.approach_pull_ahead_canceled;
        }
        if !ctx.settings.destination_approach_assist
            || self.ramp_stop.is_none()
            || self.approach_pull_ahead_canceled
        {
            return false;
        }
        ctx.settings.speed_keeper || !self.ramp_continues_to_destination_streets(ctx)
    }

    /// The driver's own brake during the pull-ahead: the pedals are theirs
    /// again from here, said the way every other assist says it.
    pub fn cancel_approach_pull_ahead(&mut self, ctx: &mut GameContext) {
        self.approach_pull_ahead = false;
        self.approach_pull_ahead_canceled = true;
        self.destination_arrival_active = false;
        self.destination_assist_brake = 0.0;
        // ROUTE, not the ambient default: an automation just released the
        // pedals (the automation-handoff rule, 2026-08-20).
        let whither = if self.ramp_continues_to_destination_streets(ctx) {
            "onto the streets"
        } else {
            "to the entrance"
        };
        ctx.say_event_with(
            format!("Pull ahead {whither}."),
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Confirmation),
        );
    }

    /// Speak the physical traction states once, on the edge they begin.
    ///
    /// Each warning names the state and the action that clears it: ease off
    /// the speed when the tires float, ease off the jake when the drive
    /// wheels slide. The flag resets when the state clears, so a second
    /// excursion warns again.
    pub fn update_traction_cues(&mut self, ctx: &mut GameContext) {
        let planing = self.trip.truck.hydroplaning();
        if planing && !self.hydro_active {
            ctx.say_event_with(
                "Hydroplaning. The steering has gone light.",
                SayEvent::new().category(SpeechCategory::Safety),
            );
        }
        self.hydro_active = planing;
        let slipping = self.trip.truck.jake_slipping() && self.trip.truck.speed_mph() > 5.0;
        if slipping && !self.jake_slip_active {
            ctx.say_event_with(
                "The drive wheels are sliding under the engine brake.",
                SayEvent::new().category(SpeechCategory::Safety),
            );
        }
        self.jake_slip_active = slipping;
        if self.trip.truck.chains_just_snapped {
            self.trip.truck.chains_just_snapped = false;
            self.chain_assist_snapped = true;
            ctx.say_event_with(
                "A tire chain broke and is flailing. Slow down and pull over to check it.",
                SayEvent::new().category(SpeechCategory::Safety),
            );
        }
        if self.trip.truck.chains_on
            && self.trip.truck.chain_wear_pct >= CHAIN_SLAP_WARN_PCT
            && !self.chain_slap_said
        {
            self.chain_slap_said = true;
            ctx.say_event_with(
                "A chain is slapping the fender. Stop soon and tighten it.",
                SayEvent::new().category(SpeechCategory::Safety),
            );
        }
        let chains_fast =
            self.trip.truck.chains_on && self.trip.truck.speed_mph() > CHAIN_SAFE_MPH + 2.0;
        if chains_fast && !self.chains_fast_active {
            ctx.say_event_with(
                format!(
                    "Too fast for chains. Keep under {}.",
                    ctx.settings.speed_text(CHAIN_SAFE_MPH)
                ),
                SayEvent::new().category(SpeechCategory::Coaching),
            );
        }
        self.chains_fast_active = chains_fast;
    }

    /// Stop planning's no-chains reminder: before a chain-law area where the
    /// posted law chains this rig, with no usable chains aboard, say so once.
    /// No store is named: garages open only from a city terminal, so the
    /// place to buy chains is the board's pre-acceptance reminder.
    /// Speech only: it never stops the truck, changes the route or spends.
    pub fn update_chain_reminder(&mut self, ctx: &mut GameContext) {
        if !ctx.settings.hos_planning_hints
            || self.departure_chain
            || self.stop_chain.is_some()
            || self.chains_aboard(ctx)
        {
            return;
        }
        let Some(ahead) = self.trip.upcoming_chain_corridor(CHAIN_REMINDER_LEAD_MI) else {
            return;
        };
        if !self.chain_reminder_said.insert(ahead.key.clone()) {
            return;
        }
        let highway = ahead.highway;
        let line = if ahead.exit_label.is_empty() {
            format!("Chain law ahead on {highway}. You have no chains.")
        } else {
            let exit = ahead.exit_label;
            format!("Chain law ahead on {highway} at {exit}. You have no chains.")
        };
        // Advice, not a warning: queued behind whatever is speaking.
        ctx.say_event_with(
            line,
            SayEvent::queued().category(SpeechCategory::Navigation),
        );
    }

    /// Warn once per area, then run the deterministic checkpoint.
    ///
    /// The physics is the real enforcement -- glare ice at 0.15 grip does not
    /// negotiate -- but the law adds the honest paper consequence: roll past
    /// the midpoint of an active control out of compliance and the checkpoint
    /// at the bottom of the grade may have your number. One citation per area
    /// per level; the roll is seeded, so a reload does not re-roll the dice.
    pub fn update_chain_law(&mut self, ctx: &mut GameContext) {
        let level = self.trip.chain_law_level();
        if level == 0 || self.trip.truck.speed_mph() < 3.0 {
            return;
        }
        let Some(area) = self.trip.chain_law_area_at(self.trip.position_mi) else {
            return;
        };
        // Winter tires never excuse a semi, except at Wyoming Level 1;
        // Colorado's Level 1 leaves a tandem-drive truck unchained
        // (`rig_compliant_at`).
        if self.trip.rig_compliant_at(self.trip.position_mi) {
            return;
        }
        let key = (area as i64, level);
        if !self.chain_law_warned.contains(&key) {
            self.chain_law_warned.insert(key);
            let need = self.trip.chain_law_need_at(self.trip.position_mi);
            ctx.say_event_with(
                format!("Rolling into an active chain law without {need}."),
                SayEvent::new().category(SpeechCategory::Navigation),
            );
        }
        let (start, end) = self.trip.chain_law_areas[area];
        if self.trip.position_mi < (start + end) / 2.0 || self.chain_checkpoint_hold {
            return;
        }
        // Staffing is rolled once per area and level and kept, so a truck
        // that turns back and returns meets the same checkpoint; the roll is
        // seeded, so a reload never re-rolls it either.
        let state = self.trip.state_code_at(self.trip.position_mi);
        let trip_seed = self.trip_seed;
        let staffed = *self.chain_checkpoint_staffed.entry(key).or_insert_with(|| {
            let mut rng = PyRandom::new_from_str(&format!("{trip_seed}:chain-law:{area}:{level}"));
            rng.random() < chain_law_checkpoint_chance(state.as_deref())
        });
        if !staffed {
            return;
        }
        // A real checkpoint doesn't fine and wave you on: California
        // prohibits entering a control without devices (13 CCR 1072(h)) and
        // screening stops every truck. A noncompliant truck is held on the
        // apron every time it reaches a staffed checkpoint, until its chains
        // are on or the law lifts (`update_chain_checkpoint_hold`).
        self.hold_at_chain_checkpoint(ctx);
        let after = if self.usable_chains_aboard(ctx) {
            "You can't go on until your chains are on. Put them on from the pause menu."
        } else {
            "You can't go on without chains. Wait here until the chain law lifts, or turn back."
        };
        // Cited once per area and level: a second stop is a hold, not a
        // second fine.
        if !self.chain_law_cited.insert(key) {
            ctx.audio.play("ui/warning");
            ctx.say_event_with(
                format!("Chain checkpoint. The officer stops you again. {after}"),
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Navigation),
            );
            return;
        }
        if self.new_driver_warning(ctx) {
            ctx.audio.play("ui/warning");
            ctx.say_event_with(
                format!("Chain checkpoint. An officer stops you and gives you a warning. {after}"),
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Navigation),
            );
            return;
        }
        let zone = self.trip.in_construction_zone();
        let fine = citation_fine(
            chain_law_fine(state.as_deref()),
            career_citations(profile_of(ctx)),
            zone,
            None,
        );
        let money = {
            let p = profile_mut_of(ctx);
            p.spend(fine);
            p.money()
        };
        self.ticket_fines_paid += fine;
        // On the record like every other citation: this one was charged and
        // spoken but never booked, so a career of chain-law tickets still
        // read as a clean safety record (owner report, 2026-09-12). Not a
        // serious violation under 49 CFR 383.51 Table 2, so no ladder text.
        self.log_enforcement(
            ctx,
            fine,
            false,
            false,
            &format!("No chains under a level {level} chain law"),
        );
        ctx.audio.play("ui/error");
        // A citation is money, not an act-now warning: ROUTE's never-dropped
        // queue instead of an interrupt that could erase one.
        ctx.say_event_with(
            format!(
                "Chain checkpoint. An officer stops you and writes a chain-law citation, {} \
                 dollars.{} {after} You have {} dollars.",
                fmt_grouped(fine, 0),
                construction_zone_fine_clause(zone),
                fmt_grouped(money, 0)
            ),
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Money),
        );
    }

    /// Stop the truck on the checkpoint apron with the parking brake set.
    pub fn hold_at_chain_checkpoint(&mut self, ctx: &mut GameContext) {
        self.chain_checkpoint_hold = true;
        let truck = &mut self.trip.truck;
        truck.velocity_mps = 0.0;
        truck.throttle = 0.0;
        truck.brake = 1.0;
        truck.set_parking_brake();
        self.cancel_cruise_for_parking_brake(ctx);
    }

    /// Keep a held truck on the apron until its chains are on, or the law
    /// lifts. Releasing the brake to drive off sets it again and says why.
    /// With no usable set aboard (none, or one worn out on the apron) there
    /// is nothing to put on, so the line names the two ways out: wait for
    /// the law to lift, which releases the hold here, or turn back
    /// (`turn_back_from_chain_checkpoint`).
    pub fn update_chain_checkpoint_hold(&mut self, ctx: &mut GameContext) {
        if !self.chain_checkpoint_hold {
            return;
        }
        if self.trip.truck.chains_on || self.trip.chain_law_level() == 0 {
            self.chain_checkpoint_hold = false;
            if self.trip.truck.chains_on {
                ctx.say_event_with(
                    "Chains on. The officer waves you back onto the highway.",
                    SayEvent::new().category(SpeechCategory::Navigation),
                );
            } else {
                ctx.say_event_with(
                    "The chain law has lifted. The officer waves you back onto the highway.",
                    SayEvent::new().category(SpeechCategory::Navigation),
                );
            }
            return;
        }
        if !self.trip.truck.parking_brake || self.trip.truck.velocity_mps > 0.0 {
            self.hold_at_chain_checkpoint(ctx);
            let line = if self.usable_chains_aboard(ctx) {
                "Held at the chain checkpoint. Put your chains on from the pause menu."
            } else {
                "Held at the chain checkpoint. You have no chains. Call dispatch for a set, \
                 wait here until the chain law lifts, or turn back."
            };
            ctx.say_event_with(line, SayEvent::new().category(SpeechCategory::Navigation));
        }
    }

    /// Held at a checkpoint: wait for the law to lift on the apron, in
    /// `CHAIN_WAIT_STEP_MIN` steps logged on duty, not driving (the driver is
    /// with the truck at a checkpoint, not resting). The level is rechecked
    /// after each step; the wait stops when the law lifts, when the 14-hour
    /// duty window is used up, or after `CHAIN_WAIT_MAX_MIN`. Returns the
    /// outcome to speak, or None when the truck is not held.
    pub fn wait_for_chain_law_to_lift(&mut self, ctx: &mut GameContext) -> Option<String> {
        if !self.chain_checkpoint_hold {
            return None;
        }
        let duty_limit = ff_core::sim::hos::limits(&ctx.settings.hos_mode).map(|l| l.1);
        let mut waited = 0.0;
        loop {
            if self.trip.chain_law_level() == 0 {
                self.chain_checkpoint_hold = false;
                return Some(format!(
                    "The chain law lifted after {}. The officer waves you back onto the \
                     highway.",
                    ff_core::sim::hos::duration_text(waited / 60.0)
                ));
            }
            if duty_limit.is_some_and(|limit| hos_of(ctx).duty_min + CHAIN_WAIT_STEP_MIN > limit) {
                // Turning back is offered only when the drive back still
                // fits the hours; otherwise it would just be refused. With
                // nothing waited yet, no "You waited 0 minutes".
                let turn_back = self.chain_turn_back_refusal(ctx).is_none();
                let waited_text = ff_core::sim::hos::duration_text(waited / 60.0);
                return Some(match (waited > 0.0, turn_back) {
                    (true, true) => format!(
                        "You waited {waited_text}, and your 14-hour duty window is nearly used \
                         up. The chain law still stands. Sleep at the checkpoint from the pause \
                         menu, or turn back."
                    ),
                    (true, false) => format!(
                        "You waited {waited_text}, and your 14-hour duty window is nearly used \
                         up. The chain law still stands. Sleep at the checkpoint from the pause \
                         menu."
                    ),
                    (false, true) => "Your 14-hour duty window is nearly used up. The chain law \
                                      still stands. Sleep at the checkpoint from the pause menu, \
                                      or turn back."
                        .to_string(),
                    (false, false) => "Your 14-hour duty window is nearly used up. The chain \
                                       law still stands. Sleep at the checkpoint from the pause \
                                       menu."
                        .to_string(),
                });
            }
            if waited >= CHAIN_WAIT_MAX_MIN {
                return Some(format!(
                    "You waited {}. The chain law still stands. Wait again, or turn back.",
                    ff_core::sim::hos::duration_text(waited / 60.0)
                ));
            }
            advance_rest_clock(
                self,
                ctx,
                CHAIN_WAIT_STEP_MIN,
                Some("on_duty_not_driving"),
                "chain checkpoint",
            );
            hos_mut_of(ctx).on_duty(CHAIN_WAIT_STEP_MIN);
            waited += CHAIN_WAIT_STEP_MIN;
            // The sky is re-polled by `advance_rest_clock` (live weather
            // included); the warnings need asking again here, since they are
            // otherwise only re-read after the truck moves.
            self.refresh_weather_alerts_here(ctx);
        }
    }

    /// After a sleep at the checkpoint: ask again for the warnings and the
    /// live sky where the truck stands, and release the hold when the law
    /// lifted overnight. Returns the line to add: the lift, or that the law
    /// still stands. None when the truck was not held.
    pub fn recheck_chain_checkpoint_hold_after_rest(
        &mut self,
        ctx: &mut GameContext,
    ) -> Option<&'static str> {
        if !self.chain_checkpoint_hold {
            return None;
        }
        self.refresh_weather_alerts_here(ctx);
        self.trip.weather.update(0.0);
        if self.trip.chain_law_level() > 0 {
            return Some("The chain law still stands.");
        }
        self.chain_checkpoint_hold = false;
        Some("The chain law lifted overnight. The officer waves you back onto the highway.")
    }

    /// A set aboard that can still go on: owned and not worn out.
    pub fn usable_chains_aboard(&self, ctx: &GameContext) -> bool {
        profile_of(ctx).chains_owned() && self.trip.truck.chain_wear_pct < 100.0
    }

    /// The last route city at or behind the truck: its index in the route's
    /// cities and its route mile.
    pub fn last_route_city_behind(&self) -> (usize, f64) {
        let here = self.trip.position_mi;
        let count = self.trip.route.cities.len();
        (0..count.min(self.trip.leg_starts.len().max(1)))
            .rev()
            .map(|i| (i, self.trip.leg_starts.get(i).copied().unwrap_or(0.0)))
            .find(|&(_, mile)| mile <= here)
            .unwrap_or((0, 0.0))
    }

    /// The drive back to the last route city: its miles and driving minutes.
    pub fn chain_turn_back_drive(&self) -> (f64, f64) {
        let (_, city_mi) = self.last_route_city_behind();
        let miles = (self.trip.position_mi - city_mi).max(0.0);
        let minutes = (miles / CHAIN_TURN_BACK_MPH * 60.0).max(CHAIN_TURN_BACK_MIN_MIN);
        (miles, minutes)
    }

    /// Why the drive back from a checkpoint would break the hours, as the
    /// line to speak, or None when it fits. Gated on the nearest limit, so
    /// the 8-hour drive before the 30-minute break counts as well as the
    /// 11-hour driving limit and the 14-hour window.
    pub fn chain_turn_back_refusal(&self, ctx: &GameContext) -> Option<String> {
        let (_, minutes) = self.chain_turn_back_drive();
        let limit = hos_of(ctx).next_limit(&ctx.settings.hos_mode)?;
        if limit.remaining_min >= minutes {
            return None;
        }
        let drive = ff_core::sim::hos::duration_text(minutes / 60.0);
        Some(if limit.kind == "break" {
            format!(
                "Turning back takes {drive}, more driving than you have before your 30-minute \
                 break. Wait here, which counts as your break, or sleep at the checkpoint."
            )
        } else {
            format!(
                "Turning back takes {drive} of driving, more than your hours allow. Wait here \
                 for the chain law to lift, or sleep at the checkpoint from the pause menu."
            )
        })
    }

    /// Held at a checkpoint with no chains to put on: the scripted drive
    /// back to the last route city, charged like the facility-gate
    /// turnaround (hours, fatigue and fuel move with the clock). The hold
    /// ends, and the truck waits at that city with the parking brake set.
    /// Returns the line to speak, or None when the truck is not held.
    pub fn turn_back_from_chain_checkpoint(&mut self, ctx: &mut GameContext) -> Option<String> {
        if !self.chain_checkpoint_hold {
            return None;
        }
        if let Some(line) = self.chain_turn_back_refusal(ctx) {
            ctx.audio.play("ui/error");
            return Some(line);
        }
        let (index, city_mi) = self.last_route_city_behind();
        let (miles, minutes) = self.chain_turn_back_drive();
        self.trip.game_minutes += minutes;
        self.charge_scripted_hours(ctx, minutes);
        // A real drive back: on the odometer, at the highway burn.
        self.trip.truck.odometer_mi += miles;
        self.trip.truck.burn_cruise_fuel_over_miles(miles);
        self.trip.position_mi = city_mi;
        self.chain_checkpoint_hold = false;
        let truck = &mut self.trip.truck;
        truck.velocity_mps = 0.0;
        truck.throttle = 0.0;
        truck.set_parking_brake();
        self.cancel_cruise(ctx, false);
        let key = self
            .trip
            .route
            .cities
            .get(index)
            .cloned()
            .unwrap_or_default();
        let city = ctx.world.city(&key).map(|c| c.name.clone()).unwrap_or(key);
        ctx.audio.play("ui/warning");
        Some(format!(
            "You turned back to {city}, {} behind the checkpoint, in {} minutes. The chain law \
             ahead still stands.",
            ctx.settings.distance_text(miles, false),
            fmt_f(minutes, 0)
        ))
    }

    /// Colorado's carry rule: no chains aboard on a carry corridor's
    /// mountain run from September 1 to May 31 is its own citation, whatever
    /// the weather (CSP chain-law page; 100 plus 32, CDOT freight fact sheet,
    /// Aug 2026). Warned once on entry, rolled once past the run's midpoint
    /// at the same seeded checkpoint chance as the chain law.
    pub fn update_chain_carry(&mut self, ctx: &mut GameContext) {
        let at = self.trip.position_mi;
        if self.trip.truck.speed_mph() < 3.0 {
            return;
        }
        // Once a season, kept in the profile so a quit and resume or the
        // next drive doesn't say it again.
        let season = self
            .trip
            .current_career_hours()
            .map(ff_core::sim::trip_models::mt_chain_season_id);
        if season.is_some()
            && profile_of(ctx).mt_chain_warned_season() != season
            && self.trip.on_mt_chain_carry_run(at)
            && !(profile_of(ctx).chains_owned() && self.trip.truck.chain_wear_pct < 100.0)
        {
            profile_mut_of(ctx).set_mt_chain_warned_season(season.unwrap_or_default());
            ctx.say_event_with(
                "Montana requires trucks to carry chains from October 1 to April 30. You have \
                 none.",
                SayEvent::new().category(SpeechCategory::Navigation),
            );
        }
        if !self.trip.on_co_chain_carry_corridor(at) {
            return;
        }
        let aboard = profile_of(ctx).chains_owned() && self.trip.truck.chain_wear_pct < 100.0;
        if aboard {
            return;
        }
        if !self.chain_carry_warned {
            self.chain_carry_warned = true;
            ctx.say_event_with(
                "Colorado requires chains aboard on this road from September 1 to May 31. You \
                 have none.",
                SayEvent::new().category(SpeechCategory::Navigation),
            );
        }
        let Some(area) = self.trip.chain_law_area_at(at) else {
            return;
        };
        let (start, end) = self.trip.chain_law_areas[area];
        if self.chain_carry_rolled || at < (start + end) / 2.0 {
            return;
        }
        self.chain_carry_rolled = true;
        let mut rng = PyRandom::new_from_str(&format!("{}:chain-carry:{area}", self.trip_seed));
        if rng.random() >= CHAIN_LAW_CHECKPOINT_CHANCE {
            return;
        }
        if self.new_driver_warning(ctx) {
            ctx.audio.play("ui/warning");
            ctx.say_event_with(
                "Chain check. No chains aboard on a mountain corridor in chain season. The \
                 officer gives you a warning.",
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Navigation),
            );
            return;
        }
        let zone = self.trip.in_construction_zone();
        let fine = citation_fine(
            CHAIN_CARRY_FINE_CO,
            career_citations(profile_of(ctx)),
            zone,
            None,
        );
        let money = {
            let p = profile_mut_of(ctx);
            p.spend(fine);
            p.money()
        };
        self.ticket_fines_paid += fine;
        self.log_enforcement(
            ctx,
            fine,
            false,
            false,
            "No chains aboard on a Colorado chain-carry corridor",
        );
        ctx.audio.play("ui/error");
        ctx.say_event_with(
            format!(
                "Chain check. No chains aboard on a mountain corridor in chain season. Citation, \
                 {} dollars.{} You have {} dollars.",
                fmt_grouped(fine, 0),
                construction_zone_fine_clause(zone),
                fmt_grouped(money, 0)
            ),
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Money),
        );
    }
}
