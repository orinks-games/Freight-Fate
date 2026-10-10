//! Shared lifecycle helpers for the driving state's speed controllers (port
//! of `freight_fate/states/driving_speed_control.py`, the
//! `SpeedControlStateMixin`).
//!
//! `driving_events::cruise` engages and works the two controllers; this is the
//! session around them -- arming, pausing at a stop, resuming, cancelling --
//! plus the look-ahead the speed keeper eases on.
//!
//! The eight `KEEPER_SNUB_*` / `KEEPER_OVERRUN_*` / `KEEPER_EASE_UNDERSHOOT`
//! constants still live in `driving_events::pending`, where they landed first
//! and where the cruise loop imports them; they are re-exported here so this
//! module's own surface reads like the Python file's.

use ff_core::sim::trip_route_helpers::zone_key;
use ff_core::sim::vehicle::MIN_STOPPING_DECEL_MPS2;
use ff_core::speech_pacing::SpeechCategory;
use ff_core::speech_text::SpokenMessage;

use crate::app::{GameContext, Say, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_turns::TURN_COMMIT_TAIL_MI;

pub use crate::states::driving_events::pending::{
    KEEPER_EASE_UNDERSHOOT_MPH, KEEPER_OVERRUN_MPH, KEEPER_OVERRUN_S, KEEPER_SNUB_DECEL_MPS2,
    KEEPER_SNUB_MAX_BRAKE, KEEPER_SNUB_MIN_BRAKE, KEEPER_SNUB_OVER_MPH, KEEPER_SNUB_UNDER_MPH,
};

// The keeper's ease and the arrival zones plan the same shed, so the three
// numbers under it have one definition, in the portable layer.
//
// How early the keeper starts shedding speed for something ahead, sized in
// REAL seconds rather than miles -- the same law the turn call and the zone
// warning already follow, because under time compression a fixed stretch of
// road passes before a truck can slow through it.
pub const KEEPER_EASE_REAL_S: f64 = APPROACH_REACTION_S;
// And down to the number this long BEFORE the point, never exactly on it: a
// corner reached at the advise speed with no margin is a corner taken at it,
// which is the tester report this exists to answer.
pub const KEEPER_SETTLE_REAL_S: f64 = ff_core::sim::trip_models::APPROACH_SETTLE_S;
// What the keeper actually delivers at street speed, measured on the bench
// rather than assumed: it is capped at light brake and comes off the pedal
// inside its own deadband, so it sheds about 0.4 m/s2 down there (0.9 at
// highway speed, where drag does most of the work). Sized on a comfortable
// 0.8 instead, the window was honest for a corner but half what a 25-to-15
// drop needs, and the truck reached the sign still doing 17.
//
// Kept at 0.4 on purpose, even though the snub added a few hours after that
// measurement (7f46880b) now guarantees KEEPER_SNUB_DECEL_MPS2 -- 0.6, net of
// the grade, held until the truck is a mile an hour UNDER the number, so the
// real figure on the flat is better than this. Planning against 0.4 keeps a
// third of the window in hand, and that is the margin that still makes the
// corner on a downgrade, on warm drums, or with the pedal saturated at
// KEEPER_SNUB_MAX_BRAKE. The two failures do not cost the same: arriving early
// costs a stretch at the lower number, arriving late costs the corner, the
// loop-back, and the session with it.
pub const KEEPER_EASE_DECEL_MPS2: f64 = APPROACH_DECEL_MPS2;
// How fast the keeper builds speed on a street, for the build-and-shed test
// between close corners (`keeper_build_and_shed_mi`). Measured on the bench,
// not assumed (`test_the_keepers_build_rate_is_the_one_its_planner_assumes`):
// 10 to 28 mph on the flat at 0.94 m/s2 empty and 0.49 with 18 t aboard.
// Planned under the loaded figure, for the reason the ease plans at 0.4: a
// build priced too fast is a corner the truck reaches over its number.
pub const KEEPER_BUILD_MPS2: f64 = 0.4;
// A ceiling, so a long access road is never crawled from one end.
pub const KEEPER_EASE_MAX_MI: f64 = 0.75;
// Scan step for the posted-limit look ahead. A city block is shorter than the
// tenth of a mile adaptive cruise steps by on the corridor.
pub const KEEPER_LIMIT_PROBE_MI: f64 = 0.05;

impl DrivingState {
    /// `_clear_cruise(*, preserve_exit_cap=False)`.
    pub fn clear_cruise(&mut self, preserve_exit_cap: bool) {
        self.cruise_mph = None;
        self.cruise_working_mph = None;
        self.cruise_held_mph = None;
        self.cruise_held_reason = String::new();
        self.cruise_throttle = 0.0;
        self.cruise_applied = 0.0;
        self.cruise_trim = 0.0;
        if self.cruise_jake_stage > 0 {
            // Hand the retarder back with the cruise session, but only the
            // stages cruise raised -- the driver's own jake switch stays put.
            self.cruise_jake_stage = 0;
            self.trip.truck.engine_brake_stage = 0;
        }
        if !preserve_exit_cap {
            self.cruise_exit_mph = None;
        }
        self.cruise_curve_mph = None;
        self.cruise_curve_end_mi = None;
        self.cruise_descent_mph = None;
        self.descent_said_mph = None;
        self.descent_safe_mph = None;
        self.descent_safe_key = None;
        self.descent_posted_cap_mph = None;
        self.trip.truck.descent_gear_hold = false;
        self.cruise_jake_last_step = 0;
        self.cruise_snubbing = false;
        self.pcc_phase = String::new();
        self.climb_cue_said = false;
        self.limp_cruise_said = false;
        self.acc_following = false;
        self.acc_weather_gap_said = false;
        self.acc_limit_capped = false;
        self.acc_limit_cap_said = None;
        self.acc_limit_hold = None;
        self.acc_weather_cap_said = None;
        self.construction_slowdown = None;
    }

    /// `_clear_keeper()`.
    pub fn clear_keeper(&mut self) {
        self.keeper_mph = None;
        self.keeper_throttle = 0.0;
        self.keeper_zone = String::new();
        self.keeper_zone_limit = None;
        self.keeper_ease_said = None;
        self.keeper_ease_target = None;
        self.keeper_held_mph = None;
        self.keeper_held_reason = String::new();
        self.keeper_snub = 0.0;
        self.keeper_droop_s = 0.0;
        self.keeper_droop_said = false;
        self.keeper_overrun_s = 0.0;
        self.keeper_overrun_said = false;
    }

    /// `_disarm_speed_control()`: the whole session off, both controllers.
    pub fn disarm_speed_control(&mut self, _ctx: &mut GameContext) {
        // Remember the open-road target across the cancel, like a car's
        // RESUME: braking drops the session, Shift+K brings the speed back.
        // A keeper-only cancel carries no target and must not clobber a
        // remembered one.
        // With nothing remembered yet, the keeper's speed is the last one
        // set, so resume brings that back.
        let remembered = self
            .speed_control_target_mph
            .or(self.cruise_mph)
            .or(self.keeper_mph.filter(|_| self.resume_target_mph.is_none()));
        if let Some(remembered) = remembered {
            if remembered != 0.0 {
                self.resume_target_mph = Some(remembered);
            }
        }
        self.clear_cruise(false);
        self.clear_keeper();
        self.speed_control_armed = false;
        self.clear_stop_pause();
        self.speed_control_target_mph = None;
        self.speed_control_floor_said = false;
        // A curve pause dies with the session it was holding open: a manual
        // K, the driver's own emergency brake, or any other disarm during the
        // bend must not leave a resume mile behind to re-engage a session
        // the driver just switched off.
        self.cruise_resume_after_mi = None;
    }

    /// `_speed_authority_engaged()`: whether an automatic speed system
    /// currently owns the pedal.
    ///
    /// Cruise, the speed keeper, or curve assist is holding or shedding
    /// speed. A hand-held throttle is live manual override and never
    /// consults this.
    pub fn speed_authority_engaged(&self) -> bool {
        self.cruise_mph.is_some() || self.keeper_mph.is_some() || self.curve_assist_active
    }

    /// `_resume_cruise()`: Shift+K -- re-arm the session at the last
    /// remembered set speed.
    pub fn resume_cruise(&mut self, ctx: &mut GameContext) {
        if self.speed_control_armed || self.cruise_mph.is_some() || self.keeper_mph.is_some() {
            ctx.say("Automatic speed control is already on.");
            return;
        }
        let Some(target) = self.resume_target_mph else {
            let key = ctx.control_name(crate::bindings::Action::Cruise);
            ctx.say(&format!("No remembered cruise speed yet. {key} sets one."));
            return;
        };
        if !self.trip.truck.engine_on {
            ctx.say("Resume needs the engine running.");
            return;
        }
        self.restore_speed_control_session(ctx, true, Some(target));
        let spoken = ctx.settings.speed_text(target);
        ctx.say_with(
            format!("Resuming automatic speed control at {spoken}."),
            Say::new(),
        );
        // The per-frame resume helper engages cruise or the keeper as soon as
        // the truck is rolling and off the brakes -- pressing resume while
        // still slowing arms it for the moment conditions clear.
    }

    /// `_clear_stop_pause()`: forget a stop pause, whichever kind of stop made
    /// it.
    pub fn clear_stop_pause(&mut self) {
        self.speed_control_paused_at_stop = false;
        self.speed_control_transit_pause = false;
        self.speed_control_stop_honored = false;
    }

    /// `_pause_speed_control(*, resume_when_rolling=False)`: pause an armed
    /// session at a planned stop without forgetting it.
    ///
    /// Two kinds of stop wear this pause. An ARRIVAL -- a pickup or delivery
    /// gate, a planned rest stop -- is held until the player departs, and
    /// departure is the only thing that clears it. A TRANSIT stop -- the light
    /// or sign at the end of a ramp -- has no departure at all: the driver
    /// stops at the bar and drives on. Held the arrival way, taking an exit
    /// left adaptive cruise and the speed keeper dead for the rest of the run,
    /// and only pressing resume brought them back (Shane, 2026-08-15).
    /// `resume_when_rolling` says this is a transit stop, and the pause lifts
    /// itself once the stop has been honored and the truck is rolling again.
    ///
    /// Clearing the controllers alone is never enough either way: the truck is
    /// still rolling toward the bar or the gate, so the resume check would
    /// re-engage the keeper on the very next frame and announce it -- right
    /// after telling the player it would wait.
    pub fn pause_speed_control(
        &mut self,
        _ctx: &mut GameContext,
        resume_when_rolling: bool,
    ) -> bool {
        if !self.speed_control_armed {
            return false;
        }
        let was_active = self.cruise_mph.is_some() || self.keeper_mph.is_some();
        self.clear_cruise(false);
        self.clear_keeper();
        self.speed_control_paused_at_stop = true;
        self.speed_control_transit_pause = resume_when_rolling;
        self.speed_control_stop_honored = false;
        was_active
    }

    /// `_lift_transit_pause(*, braking)`: whether a transit pause has run its
    /// course this frame.
    ///
    /// It ends where the stop it was made for ends: the truck honored the bar
    /// and is rolling again with the player off the brake. Running out of ramp
    /// AND out of armed exit counts as honoring it too -- a terminal with no
    /// control at all is honored by driving through it, a blown ramp hands the
    /// highway back at speed, and a cancelled exit leaves nothing to wait for.
    /// Both have to be clear: the exit assist pauses a mile and a half out,
    /// with the ramp still ahead and no bar taken yet. An arrival pause is
    /// never lifted here; only departure clears one.
    pub fn lift_transit_pause(&mut self, braking: bool) -> bool {
        if !self.speed_control_transit_pause {
            return false;
        }
        let speed = self.trip.truck.speed_mph();
        if speed <= RED_STOP_MPH || (self.ramp_mi.is_none() && self.exit_stop.is_none()) {
            self.speed_control_stop_honored = true;
        }
        if !self.speed_control_stop_honored || braking {
            return false;
        }
        // Rolling FORWARD: backing away from a bar is not driving on from it,
        // and speed_mph reads the same either way.
        if self.trip.truck.velocity_mps <= 0.0 || speed < KEEPER_MIN_MPH {
            return false;
        }
        self.clear_stop_pause();
        true
    }

    /// `_restore_speed_control_session(*, armed, target_mph)`.
    pub fn restore_speed_control_session(
        &mut self,
        _ctx: &mut GameContext,
        armed: bool,
        target_mph: Option<f64>,
    ) {
        self.clear_cruise(false);
        self.clear_keeper();
        self.speed_control_armed = armed;
        self.clear_stop_pause();
        self.speed_control_target_mph = if armed { target_mph } else { None };
        self.cruise_resume_after_mi = None;
    }

    /// `_resume_speed_control_if_ready(*, braking)`: resume a paused
    /// job-scoped session once the player is rolling again.
    pub fn resume_speed_control_if_ready(&mut self, ctx: &mut GameContext, braking: bool) {
        if !self.speed_control_armed || self.cruise_mph.is_some() || self.keeper_mph.is_some() {
            return;
        }
        if self.speed_control_paused_at_stop && !self.lift_transit_pause(braking) {
            return;
        }
        if self.trip.truck.emergency_brake {
            self.disarm_speed_control(ctx);
            // ROUTE, not the ambient default: the automation just released the
            // throttle (automation-handoff sweep, 2026-08-20, the deferred
            // 2026-08-15 audit).
            ctx.say_event_with(
                "Automatic speed control canceled.",
                SayEvent::queued()
                    .priority(EventPriority::Route)
                    .category(SpeechCategory::Confirmation),
            );
            return;
        }
        if self.hazard_deadline.is_some() {
            // A hazard is live: the announce-time pause above hands the
            // pedals back, and resuming here would wind the truck back up
            // into the very thing it is braking for. Cleared, the deadline
            // goes with it and the session comes back on its own.
            return;
        }
        if let Some(after_mi) = self.cruise_resume_after_mi {
            // A bend too tight for cruise paused the session (the curve
            // branch of `handle_curve_event`) and named the mile the pause
            // lifts at: the bend's end plus the commit tail. Resuming before
            // that would wind the truck back up to the remembered target in
            // the middle of the very corner it was paused for -- the reason
            // this used to be a disarm. Past it, the resume below re-engages
            // at the remembered target and says so, exactly as after a
            // hazard.
            if self.trip.position_mi <= after_mi {
                return;
            }
            self.cruise_resume_after_mi = None;
        }
        if self.ramp_mi.is_some() {
            // A ramp stop is in progress. Taking the exit hands the pedals
            // back, and the stop at the end of the ramp is the driver's to
            // make -- resuming here wound the truck back up and drove a player
            // straight past the destination terminal, silently, at 66 mph
            // (owner playtest, Buffalo to Albany, 2026-08-12). The whole ramp
            // counts, not just the stretch `trip.controlled_ramp` covers: that
            // flag drops the moment the light or sign is behind the truck,
            // which is exactly where the entrance still is. This, not the
            // pause above, is what keeps a transit pause from re-engaging on
            // the creep to the bar.
            return;
        }
        if self.street_bar_mi.is_some() && self.ramp_terminal_owns_the_stop() {
            // A light or sign on the streets the assist is stopping for or
            // holding at: the same guard as the ramp above, for the length of
            // that stop. Its release hands the streets back.
            return;
        }
        if self.destination_arrival_active {
            // The arrival owns the pedals, exactly as the ramp above does.
            // Without this the facility's own street chain had no guard at
            // all: the chain is built of zones, every zone re-engages the
            // keeper, and the keeper then holds the street limit straight
            // through the arrival point while the approach assist tries to
            // brake against it. Measured: the truck reached the market at
            // 13.8 mph (owner, Spokane, 2026-08-21: "it did not automatically
            // stop at the destination; I had to stop" -- the same sentence as
            // Odessa on 2026-08-19, whose fix only ever covered the ramp).
            //
            // Deliberately narrower than "the whole chain": the keeper holding
            // a street limit through the turns is useful and stays. Only the
            // final shed is the arrival's, and only while it lasts.
            return;
        }
        {
            let t = &self.trip.truck;
            if braking
                || t.air_brakes_holding()
                || !t.engine_on
                || t.stalled
                // Backing is the driver's own low-speed manoeuvre and never an
                // open road to hand back to. `speed_mph` is unsigned, so
                // without this a truck reversing at dock speed reads as
                // rolling.
                || t.transmission.in_reverse()
                || t.velocity_mps <= 0.0
                || t.speed_mph() < KEEPER_MIN_MPH
            {
                return;
            }
        }
        let position = self.trip.position_mi;
        let (limit, mut zone_reason) = self.trip.speed_limit_at(position);
        if zone_reason.is_none() && self.departure_ramp_mi.is_some() {
            // An acceleration lane is open road by the map and a low-speed
            // regime by the truck, and that gap left it with NO automation at
            // all: the keeper had already dropped on the yard's corners, and
            // cruise refuses below its own holding speed, so a driver pulling
            // out of a facility had to get the rig back up to road speed by
            // hand before anything would take over (Brandon, 2026-08-21). The
            // keeper is exactly the tool for the low-speed stretch -- that is
            // why it exists -- so it bridges this one too.
            zone_reason = Some("acceleration lane".to_string());
        }
        if let Some(zone_reason) = zone_reason {
            if !ctx.settings.speed_keeper {
                return;
            }
            self.engage_keeper(ctx, limit, &zone_reason, Some(limit), false);
            // ROUTE, not the ambient default: the automation is re-engaging on
            // its own (automation-handoff sweep, 2026-08-20, the deferred
            // 2026-08-15 audit).
            let held = ctx.settings.speed_text(self.keeper_mph.unwrap_or(0.0));
            let resuming = if zone_reason == "acceleration lane" {
                format!(
                    "Automatic speed control resuming. Speed keeper building to {held} for the \
                     merge."
                )
            } else {
                format!(
                    "Automatic speed control resuming. {}",
                    crate::states::driving_events::cruise::keeper_holding_line(&held, &zone_reason)
                )
            };
            ctx.say_event_with(
                resuming,
                SayEvent::queued()
                    .priority(EventPriority::Route)
                    .category(SpeechCategory::Confirmation),
            );
            return;
        }
        if self.trip.truck.speed_mph() < CRUISE_MIN_MPH {
            // Open road, but not yet at cruise's holding speed. Cruise must
            // not snap on here at the full remembered error and floor the
            // throttle to chase a high target from a crawl. It used to wait,
            // silently, for the driver to bring the rig back up by hand --
            // and after "Brake! Slow car right ahead" matched a car doing 17
            // the driver sat at 17 on a 55 road with nothing holding speed
            // and nothing said about why (owner, US-12 near Litchfield,
            // 2026-09-16). The keeper is the automation for the low-speed
            // stretch, exactly as it is for the acceleration lane: it builds
            // toward the posted limit behind whatever traffic held the truck
            // down, and `update_keeper` hands to cruise at road speed.
            if !ctx.settings.speed_keeper {
                if !self.speed_control_floor_said {
                    self.speed_control_floor_said = true;
                    let floor = ctx.settings.speed_text(CRUISE_MIN_MPH);
                    ctx.say_event_with(
                        format!(
                            "Automatic speed control resumes at {floor}. You have the pedals \
                             until then."
                        ),
                        SayEvent::queued()
                            .priority(EventPriority::Route)
                            .category(SpeechCategory::Confirmation),
                    );
                }
                return;
            }
            self.engage_keeper(ctx, limit, KEEPER_OPEN_ROAD_BRIDGE, Some(limit), false);
            let floor = ctx.settings.speed_text(CRUISE_MIN_MPH);
            ctx.say_event_with(
                SpokenMessage::with_terse(
                    format!(
                        "Automatic speed control resuming. Speed keeper building speed; \
                         adaptive cruise takes over at {floor}."
                    ),
                    "Speed control resuming.",
                ),
                SayEvent::queued()
                    .priority(EventPriority::Route)
                    .category(SpeechCategory::Confirmation),
            );
            return;
        }
        let target = self.speed_control_target_mph.unwrap_or(limit);
        self.engage_cruise(ctx, target, true);
    }

    /// `_cancel_cruise(*, preserve_session=False)`.
    pub fn cancel_cruise(&mut self, ctx: &mut GameContext, preserve_session: bool) {
        if preserve_session {
            self.clear_cruise(true);
        } else {
            self.disarm_speed_control(ctx);
        }
    }

    /// The parking brake ends speed control -- except under the pause a
    /// facility gate made, which belongs to the departure. The gate asks for
    /// the brake, and ending the session there lost the driver's set speed
    /// at every pickup (tester report, 2026-09-28).
    pub fn cancel_cruise_for_parking_brake(&mut self, ctx: &mut GameContext) {
        let gate_pause = self.trip.finished
            && self.speed_control_paused_at_stop
            && !self.speed_control_transit_pause;
        if gate_pause {
            self.clear_cruise(false);
            self.clear_keeper();
        } else {
            self.disarm_speed_control(ctx);
        }
    }

    /// `_cancel_keeper(*, preserve_session=False)`.
    pub fn cancel_keeper(&mut self, ctx: &mut GameContext, preserve_session: bool) {
        if preserve_session {
            self.clear_keeper();
        } else {
            self.disarm_speed_control(ctx);
        }
    }

    /// `_restricted_zone_limit_ahead()`: a lower restricted-zone limit inside
    /// the player's advance-warning window.
    ///
    /// Returns `(limit_mph, zone_reason)` for the nearest construction or
    /// heavy-traffic zone that is closer than the spoken advance-warning
    /// distance and whose limit is lower than the current corridor limit.
    /// `None` when there is nothing to pre-brake for.
    pub fn restricted_zone_limit_ahead(&mut self, ctx: &mut GameContext) -> Option<(f64, String)> {
        if !self.speed_control_armed || !ctx.settings.speed_keeper {
            self.construction_slowdown = None;
            self.construction_taper_stage = None;
            return None;
        }
        let position = self.trip.position_mi;
        let held = self.construction_slowdown.clone();
        let (limit_mph, reason) = match held {
            // Keep aiming at a zone already being slowed for. The warning
            // window is sized in real seconds, so it retracts as cruise slows:
            // without this the zone dropped back out of sight and cruise wound
            // the truck up again on the approach to the barrels.
            Some((end_mi, limit_mph, reason)) if position < end_mi => (limit_mph, reason),
            _ => {
                self.construction_slowdown = None;
                self.construction_taper_stage = None;
                let lookahead_mi = self.trip.zone_warning_lookahead_mi();
                // Aim at the zone itself, which is what the spoken warning
                // names and where the number has to be met; the merge taper
                // in front of it is the first stage of the same slowdown (see
                // below), never the whole of it -- slowing only to the
                // taper's number reached the barrels too fast. Heavy traffic
                // posts no taper, so it is simply the zone.
                let zone = self
                    .trip
                    .zones
                    .iter()
                    .filter(|z| {
                        RESTRICTED_ZONE_REASONS.contains(&z.reason.as_str())
                            && z.start_mi - position > 0.0
                            && z.start_mi - position <= lookahead_mi
                    })
                    .min_by(|a, b| a.start_mi.total_cmp(&b.start_mi))
                    .cloned()?;
                // Not before the player hears why: cruise and the warning share
                // a window, so which one landed first was down to frame order.
                if !self.trip.announced_zone_warnings.contains(&zone_key(&zone)) {
                    return None;
                }
                self.construction_slowdown =
                    Some((zone.end_mi, zone.limit_mph, zone.reason.clone()));
                // The warning promises two numbers for a work zone: the
                // taper's, then the zone's. Cruise used to take the zone's
                // from the moment the warning landed -- miles out under
                // compression -- so a driver told "55 at the taper, then 45"
                // was crawling at 45 long before the taper (owner,
                // 2026-09-14). Ease to the taper's number first, and to the
                // zone's once the barrels are inside braking distance.
                self.construction_taper_stage = self
                    .trip
                    .zones
                    .iter()
                    .find(|z| {
                        z.reason == "construction merge"
                            && (z.end_mi - zone.start_mi).abs() < 0.01
                            && z.limit_mph > zone.limit_mph
                    })
                    .map(|taper| (zone.start_mi, taper.limit_mph));
                (zone.limit_mph, zone.reason)
            }
        };
        let limit_mph = match self.construction_taper_stage {
            Some((barrels_mi, taper_mph)) if position < barrels_mi => {
                let speed = self.trip.truck.speed_mph();
                if barrels_mi - position > self.acc_limit_lookahead_mi(speed, limit_mph) {
                    taper_mph
                } else {
                    // The zone stage, latched: as cruise sheds toward the
                    // zone's number the braking window shrinks, and without
                    // the latch the target climbed back to the taper's over
                    // the last yards.
                    self.construction_taper_stage = None;
                    limit_mph
                }
            }
            Some(_) => {
                self.construction_taper_stage = None;
                limit_mph
            }
            None => limit_mph,
        };
        let (current_limit, _) = self.trip.speed_limit_at(position);
        if limit_mph < current_limit {
            Some((limit_mph, reason))
        } else {
            None
        }
    }

    /// `_keeper_ease_mi(target_mph, scale)`: how much road the keeper needs to
    /// be down to `target_mph` in time.
    ///
    /// Sized in real seconds and converted to miles at `scale`, so time
    /// compression cannot spend the window before the truck can use it. The
    /// physical shed time is the floor: a big drop buys more road however
    /// relaxed the clock is. A settling tail on top puts the truck at the
    /// number ahead of the point rather than exactly on it.
    ///
    /// Where the shed is what the window is for, its seconds are priced at the
    /// speed the truck is actually DOING through them -- the mean of the two
    /// ends -- and the settling tail down at the new number. Charging every one
    /// of them at the speed the truck came in at claimed 0.64 of a mile for a
    /// 25-to-15 drop that costs 0.45, and since the eased number became a held
    /// floor (7ff22b6e) each surplus yard is crawled at the low number instead
    /// of simply re-planned. The truck reaching a service way's 15 a third of a
    /// mile early and sitting there is the "does not hold speeds on access
    /// roads" report (tester, 2026-08).
    ///
    /// The reaction budget underneath is untouched, and stays priced at today's
    /// speed: those seconds are spent hearing and deciding, before any slowing
    /// starts, so that is the road they really cost.
    pub fn keeper_ease_mi(&self, target_mph: f64, scale: f64) -> f64 {
        let speed = self.trip.truck.speed_mph().max(1.0);
        let reaction_mi = (KEEPER_EASE_REAL_S + KEEPER_SETTLE_REAL_S) * speed * scale / 3600.0;
        let shed_mi = self.keeper_shed_mi(target_mph, scale);
        // The cap trims the discretionary reaction budget, never the physical
        // shed -- the docstring above promises the shed is a floor, and the old
        // min() clipped it anyway: on long-route draws the ramped time scale
        // pushes the shed past 0.75 mi, the window came back capped, and the
        // keeper started too late to make the number by the sign (the 1-in-4
        // "15.47 against 15.0" flake, ROADMAP 2026-08-19 -- which was the game
        // quietly overshooting posted drops on high-compression trips, not a
        // test artifact).
        shed_mi.max(KEEPER_EASE_MAX_MI.min(reaction_mi.max(shed_mi)))
    }

    /// The physical half of `keeper_ease_mi`: the road the keeper's shed
    /// down to `target_mph`, and its settling tail, cover at `scale`.
    pub fn keeper_shed_mi(&self, target_mph: f64, scale: f64) -> f64 {
        self.keeper_shed_from_mi(self.trip.truck.speed_mph(), target_mph, scale)
    }

    /// `keeper_shed_mi` from `from_mph` rather than the speed the truck is
    /// doing now.
    pub fn keeper_shed_from_mi(&self, from_mph: f64, target_mph: f64, scale: f64) -> f64 {
        let speed = from_mph.max(1.0);
        let target = 1.0f64.max(target_mph.min(speed));
        // Net of the surge, for the reason the facility arrival is: a tank
        // the liquid can move in gives some of the rate back at the worst
        // moment, so the ease has to start further out to make the number.
        let decel = (KEEPER_EASE_DECEL_MPS2 - self.trip.truck.surge_decel_penalty_mps2())
            .max(MIN_STOPPING_DECEL_MPS2);
        let shed_s = (speed - target) / MPH_PER_MPS / decel;
        // The mean of the two ends through the shed, then the settling tail
        // down at the new number, because that is where the truck spends it.
        let shed_mi = shed_s * (speed + target) / 2.0 * scale / 3600.0;
        shed_mi + KEEPER_SETTLE_REAL_S * target * scale / 3600.0
    }

    /// The road the keeper needs to build from the truck's speed up to the
    /// street's `limit_mph` and shed back down to a corner's `advise_mph`.
    /// Zero when there is nothing to build: at the limit, the ordinary ease
    /// window is the whole answer.
    pub fn keeper_build_and_shed_mi(&self, limit_mph: f64, advise_mph: f64, scale: f64) -> f64 {
        let speed = self.trip.truck.speed_mph();
        if limit_mph <= advise_mph || speed >= limit_mph - KEEPER_SNUB_UNDER_MPH {
            return 0.0;
        }
        let from = speed / MPH_PER_MPS;
        let to = limit_mph / MPH_PER_MPS;
        let build_mi = (to * to - from * from) / (2.0 * KEEPER_BUILD_MPS2) / METERS_PER_MILE;
        build_mi * scale + self.keeper_shed_from_mi(limit_mph, advise_mph, scale)
    }

    /// `_keeper_turn_ease_scale()`: the clock the keeper will actually ease a
    /// corner on.
    ///
    /// A corner in play decompresses the trip to real time so "Advise 20" is
    /// plannable, and that happens at its brake point -- reaction seconds plus
    /// this ease's own shed, so always at or before it (`turn_brake_point_mi`).
    /// Sizing the ease on the compressed clock instead read the
    /// corner as close from half a mile back and held the whole block at the
    /// corner speed, which is the sluggishness this fix must not trade the
    /// tester's problem for.
    pub fn keeper_turn_ease_scale(&self) -> f64 {
        self.trip.effective_time_scale().min(1.0)
    }

    /// `_keeper_speed_ahead()`: the lower number the keeper must already be
    /// shedding speed for.
    ///
    /// Returns `(mph, reason)` for the SLOWEST thing ahead the truck cannot
    /// arrive at over -- a judged street turn's advise speed, or a posted limit
    /// lower than the one under the wheels -- among those close enough that
    /// easing has to start. `None` when the road ahead asks for nothing the
    /// truck is not already doing.
    ///
    /// Adaptive cruise refuses to engage inside a zone, so on facility streets
    /// the keeper is the only automation there is, and it read only the limit
    /// under the wheels. It held the street's 25 into corners that advise 20
    /// and straight through the safe turnaround (tester, 2026-08).
    pub fn keeper_speed_ahead(&mut self, _ctx: &mut GameContext) -> Option<(f64, String)> {
        let position = self.trip.position_mi;
        // Keep aiming at the point already being slowed for. The window is
        // sized in real seconds, so it retracts as the truck slows: without
        // this the corner dropped back out of sight and the keeper wound the
        // truck up again on its approach -- the same trap the construction hold
        // above exists to close. It is a FLOOR on what to shed for, never the
        // whole answer: a held target that could also HIDE a lower one is how
        // the second corner of a short block went unbraked (below).
        let mut demand = match self.keeper_ease_target.clone() {
            Some(held) if position < held.0 => Some(held),
            _ => None,
        };
        let turn_scale = self.keeper_turn_ease_scale();
        let (limit, under_reason) = self.trip.speed_limit_at(position);
        for cue in self.turn_cues_in_play() {
            let ahead = cue.at_mi - position;
            if ahead <= 0.0 {
                continue;
            }
            let advise = self.turn_speed_mph(&cue);
            // A corner too close to build back up to the street's number and
            // shed it again is held at its own number from here: building to
            // the limit between two corners a quarter mile apart and braking
            // for the second at about 0.3 g in the last 0.07 mile is the
            // see-saw the September 23 drive into Abilene measured.
            let no_room = ahead <= self.keeper_build_and_shed_mi(limit, advise, turn_scale);
            if ahead > self.keeper_ease_mi(advise, turn_scale) && !no_room {
                continue;
            }
            // Every corner whose window is open, not just the nearest, and the
            // slowest of them wins. A corner holds its target through its own
            // tail, and a city block is shorter than that tail -- so aiming at
            // the nearest corner alone left the keeper still holding 20 for the
            // corner behind it while a 15 mph service way came up, and the
            // truck arrived over the number with the loop-back charged
            // (tester, turns "coming up really quickly").
            //
            // Held through each corner itself, not up to it: releasing on the
            // milepost puts the throttle back on mid-turn, so a run of corners
            // at one number holds to the far side of the last of them.
            let until = cue.at_mi + TURN_COMMIT_TAIL_MI;
            match demand.as_mut() {
                None => demand = Some((until, advise, "turn".to_string())),
                Some(current) => {
                    if advise < current.1 {
                        *current = (until, advise, "turn".to_string());
                    } else if advise == current.1 {
                        current.0 = current.0.max(until);
                    }
                }
            }
        }
        // A mapped bend curve speed assistance is braking for. The servo
        // owns the profile; folding its number in here is what stops the
        // keeper holding the street limit against the servo's brake and
        // winding the truck back up the moment the pedal eases. Held to the
        // servo's own end (the chain's, plus the commit tail), the same shape
        // as a corner's hold above, and the keeper's ease line stays quiet
        // for it because the curve call already named the number.
        if let Some(servo) = self.curve_servo.as_ref() {
            if position < servo.hold_to_mi {
                let lower = demand.as_ref().is_none_or(|d| servo.target_mph < d.1);
                if lower {
                    demand = Some((servo.hold_to_mi, servo.target_mph, "curve".to_string()));
                }
            }
        }
        // A merge taper exists for one thing: shedding to the work zone's
        // number by the barrels. Taking the taper over from cruise at its
        // start and then holding the taper's number until the ease window
        // opened lost a third of a mile of it, and the window itself was
        // sized a quarter short of what the truck sheds at standard pacing:
        // the barrels came at 47 against a posted 45 (owner, 2026-09-14).
        // On a taper the shed starts at once and reaches to its end.
        let taper_end = if under_reason.as_deref() == Some("construction merge") {
            self.trip
                .zones
                .iter()
                .find(|z| {
                    z.reason == "construction merge"
                        && z.start_mi <= position
                        && position < z.end_mi
                })
                .map(|z| z.end_mi)
        } else {
            None
        };
        let mut horizon = self.trip.total_miles().min(position + KEEPER_EASE_MAX_MI);
        if let Some(end) = taper_end {
            horizon = horizon.max(self.trip.total_miles().min(end + KEEPER_LIMIT_PROBE_MI));
        }
        let mut probe = position + KEEPER_LIMIT_PROBE_MI;
        let scale = self.trip.effective_time_scale();
        while probe <= horizon + 1e-6 {
            let (posted, reason) = self.trip.speed_limit_at(probe);
            let in_window = probe - position <= self.keeper_ease_mi(posted, scale);
            let taper_shed = taper_end.is_some_and(|end| probe <= end + KEEPER_LIMIT_PROBE_MI);
            if posted < limit && (in_window || taper_shed) {
                let lower = demand.as_ref().is_none_or(|d| posted < d.1);
                if lower {
                    demand = Some((
                        probe,
                        posted,
                        reason.unwrap_or_else(|| "posted limit".to_string()),
                    ));
                }
                break;
            }
            probe += KEEPER_LIMIT_PROBE_MI;
        }
        self.keeper_ease_target = demand.clone();
        demand.map(|(_, mph, reason)| (mph, reason))
    }
}
