//! Missing the facility gate: too fast past the entrance means a loop-back
//! (port of `freight_fate/states/driving_facility_gate.py`, the
//! `FacilityGateMixin`).
//!
//! The destination facility's gate ends the drive, and the trip model clamps the
//! odometer there -- which used to be an invisible treadmill: cross the gate at
//! highway speed and the miles simply stopped while the truck barreled on, no
//! overshoot, no consequence. A truck that crosses the gate above the gate
//! zone's own posted limit (`FACILITY_GATE_LIMIT_MPH`, the 15 the last half
//! mile is signed at) has missed the entrance.
//!
//! The miss is the third instance of an existing pattern -- the blown ramp stop
//! and the missed destination exit both loop back -- and reuses its idioms: a
//! scripted reposition through the next safe turnaround, game time charged, and
//! the gate ahead again. The spoken cue is the only signage, so a pre-gate speed
//! warning with an explicit target speed always precedes the first possible
//! miss, and a real-time reaction window (`ramp_arrival_grace_seconds`, the
//! same rule the ramp uses) must expire before one latches -- under time
//! compression a distance alone is unbeatable. HOS, the clock, and fuel keep
//! running through every loop; the lost time is the consequence, never a fine.

use crate::speech::EventPriority;
use ff_core::sim::trip_models::{
    APPROACH_DECEL_MPS2, APPROACH_REACTION_S, FACILITY_ACCESS_TAIL_MI, FACILITY_GATE_LIMIT_MPH,
    FACILITY_GATE_ZONE_MI, METERS_PER_MILE, MPH_PER_MPS,
};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_updates::live;

/// Game minutes one loop through the safe turnaround costs -- the same charge
/// as the missed-destination-exit loop, which is the same maneuver a road up.
pub const GATE_MISS_LOOP_MIN: f64 = 20.0;
/// The pre-gate warning window can never reach past the road it lives on:
/// the facility access-road tail.
pub const GATE_WARNING_MAX_MI: f64 = FACILITY_ACCESS_TAIL_MI;

impl DrivingState {
    /// `_post_gate_zone()`: put the gate's 15 back on the map, now that the
    /// exit is behind.
    ///
    /// The arrival zones are stripped at trip start so nothing posts a silent
    /// low limit under a spoken 65 on the final freeway miles. That left the
    /// pre-gate warning naming a speed that was posted nowhere: it said "slow
    /// to 15" while the last half mile still read the corridor's 55, so every
    /// assist held 55 straight through the entrance and into the loop-back
    /// (owner playtest, 2026-08-21). Once the destination exit is taken the
    /// road under the truck IS the facility's own, so the 15 the warning names
    /// is the number in force -- and a facility with a street chain never
    /// reaches here, because its chain builds a gate zone of its own.
    pub fn post_gate_zone(&mut self, ctx: &GameContext) {
        if self.trip.gate_zone().is_some() {
            return;
        }
        if self.surface_chain_route(ctx).is_some() {
            // This facility's own streets are about to become the trip, and
            // that chain builds its own gate zone at the end of itself. Posting
            // one here as well would announce the same gate twice: once off the
            // ramp on the highway trip, once again on the streets.
            return;
        }
        let zone = self.trip.facility_gate_zone();
        self.trip.zones.push(zone);
        self.trip
            .zones
            .sort_by(|a, b| a.start_mi.total_cmp(&b.start_mi));
    }

    /// `_gate_warning_window_mi()`: how far out the pre-gate speed warning
    /// fires, and how far back a miss repositions -- a full spoken window, so
    /// the retry is winnable.
    ///
    /// The signed gate zone, plus the road a loaded truck braking at its
    /// normal rate needs to be down to the gate speed by the time it enters
    /// it. The zone is the road that carries the gate's 15: the last street
    /// of a facility chain, the signed half mile of a synthetic approach.
    /// This used to be twenty-five real seconds of travel at the current
    /// pace, never less than a fixed half mile back from the end -- the
    /// ramp callout's lead, which on city streets at compressed time fired
    /// "gate in half a mile" four corners before the yard while the zone
    /// itself did not begin until the last street (agent playtest, Tyler,
    /// 2026-09-03). Hearing time is the reaction window's job, not the
    /// distance's: the window runs on the real clock from the line, and the
    /// gate holds the truck while it runs.
    ///
    /// Behind a driveway the window is the yard's alone. Up to the driveway
    /// the street keeps its own limit and the driveway is a turn, judged at
    /// its corner speed by its own call; "slow to 15" on the street before it
    /// was the old gate zone's number posted on a public road.
    pub fn gate_warning_window_mi(&self) -> f64 {
        let zone_mi = self.gate_zone_length_mi().min(GATE_WARNING_MAX_MI);
        let speed = self.trip.truck.speed_mph().max(FACILITY_GATE_LIMIT_MPH) / MPH_PER_MPS;
        let gate = FACILITY_GATE_LIMIT_MPH / MPH_PER_MPS;
        let braking_mi = ((speed * speed - gate * gate) / (2.0 * APPROACH_DECEL_MPS2)).max(0.0)
            / METERS_PER_MILE;
        // Never less than the seconds it takes to hear the line and act on
        // it at this speed: with no signed stretch in front of a street gate,
        // a truck barely over its number was told "Gate in 0.0 miles".
        let braking_mi = if zone_mi > 0.0 {
            braking_mi
        } else {
            braking_mi.max(APPROACH_REACTION_S * speed * MPH_PER_MPS / 3600.0)
        };
        let window = (zone_mi + braking_mi).min(GATE_WARNING_MAX_MI);
        match self.trip.driveway_mi() {
            Some(driveway) => window.min((self.trip.total_miles() - driveway).max(0.0)),
            None => window,
        }
    }

    /// The length of the road signed at the gate speed: the trip's own yard
    /// or gate zone once posted, else the one the run would post. None at all
    /// on a street chain whose gate stands on the street itself.
    fn gate_zone_length_mi(&self) -> f64 {
        let zone = match self.trip.gate_zone() {
            Some(zone) => zone.clone(),
            None if self.trip.has_street_detail() => return 0.0,
            None => self.trip.facility_gate_zone(),
        };
        (zone.end_mi - zone.start_mi).max(0.0)
    }

    /// `_gate_miss_grace_seconds(message)`: real reaction seconds after
    /// `message`, the ramp-arrival rule.
    pub fn gate_miss_grace_seconds(&self, ctx: &GameContext, message: &str) -> f64 {
        let speech_rate = if ctx.settings.sapi_events && ctx.speech.event_supports_rate() {
            ctx.settings.speech_rate
        } else {
            0.0
        };
        ramp_arrival_grace_seconds(message, speech_rate)
    }

    /// `_check_gate_approach_warning(dt)`: warn about gate speed while braking
    /// can still make the entrance.
    ///
    /// Without this the first "slow down" fired AT the gate, so under the
    /// miss rule the first cue would have been the failure. Runs every
    /// drive tick; it also keeps the reaction window's clock, which must
    /// tick whether or not the trip has finished.
    pub fn check_gate_approach_warning(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.gate_grace_s > 0.0 {
            self.gate_grace_s = 0.0f64.max(self.gate_grace_s - dt);
        }
        if self.phase != DRIVE_PHASE_DELIVERY
            || !self.destination_exit_taken
            || self.ramp_mi.is_some()
            || self.trip.finished
        {
            return;
        }
        // The warning has been obeyed: the truck is at or under the gate
        // speed with the window spent. That warning is done; if the truck
        // speeds up again before the gate, a fresh one opens a fresh window.
        // Without this a driver who braked to a stop short of the gate (as
        // told) and then rolled the last fifty feet a few miles per hour
        // over was looped back on contact, with no window at all (agent
        // playtest, 2026-09-02). A truck that never slowed keeps its spent
        // window: blowing the gate at 70 is a miss on contact.
        if self.gate_speed_warned
            && self.gate_grace_s <= 0.0
            && self.trip.truck.speed_mph() <= FACILITY_GATE_LIMIT_MPH
        {
            self.gate_speed_warned = false;
        }
        // Once per approach. Retiring an obeyed warning re-opens the gate's
        // own window at contact (above); it does not earn a second line. On
        // a street chain every corner's slowdown obeyed it, and the first
        // straight at 30 heard "gate in 0.2 kilometers" again inside the
        // zone (agent playtest, Tyler, 2026-09-03).
        if self.gate_speed_warned
            || self.gate_warning_spoken
            || self.trip.truck.speed_mph() <= FACILITY_GATE_LIMIT_MPH
        {
            return;
        }
        let remaining = self.trip.total_miles() - self.trip.position_mi;
        if remaining > self.gate_warning_window_mi() {
            return;
        }
        let keeper_holds_the_gate = self.keeper_mph.is_some()
            && self
                .keeper_held_mph
                .is_some_and(|held| held <= FACILITY_GATE_LIMIT_MPH);
        if ctx.settings.destination_approach_assist || keeper_holds_the_gate {
            // Facility stopping assistance takes the truck to the gate, or
            // the keeper is already holding the gate's number: the warning
            // and its tone told a driver to do what an assist was doing
            // (agent drive A, 2026-09-24). No warning means no miss clock
            // either; a driver who takes over and runs the gate still gets
            // the gate's own window (`seed_gate_grace_at_gate`).
            return;
        }
        self.gate_speed_warned = true;
        self.gate_warning_spoken = true;
        let target = ctx.settings.speed_text(FACILITY_GATE_LIMIT_MPH);
        // In feet close in: a tenth of a mile and under read "0.0 miles".
        let distance = if remaining < 0.1 {
            self.short_distance_text(ctx, remaining)
        } else {
            ctx.settings.distance_text(remaining, true)
        };
        let message = if self.terse_speech(ctx) {
            format!("Gate in {distance}. Slow to {target}.")
        } else {
            format!("Facility gate in {distance}. Slow to {target}.")
        };
        self.gate_grace_s = self.gate_miss_grace_seconds(ctx, &message);
        ctx.audio.play("ui/warning");
        // Never dropped: this line starts the gate miss clock, so losing it
        // costs the driver the gate itself.
        ctx.say_event_with(
            message,
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Navigation),
        );
    }

    /// `_seed_gate_grace_at_gate(message)`: open the reaction window at the
    /// gate itself when no pre-gate warning was heard -- a resumed save can
    /// arrive here cold. The miss clock starts with the gate's own stop line,
    /// never on first contact.
    pub fn seed_gate_grace_at_gate(&mut self, ctx: &GameContext, message: &str) {
        if self.gate_speed_warned || self.trip.truck.speed_mph() <= FACILITY_GATE_LIMIT_MPH {
            return;
        }
        self.gate_speed_warned = true;
        self.gate_grace_s = self.gate_miss_grace_seconds(ctx, message);
    }

    /// `_gate_miss_pending()`: rolling past the gate has become a miss --
    /// warned, window expired, still above the gate zone's posted limit, and
    /// no assist or emergency owns the speed. The destination approach assist
    /// is checked by the caller (its branch brakes the truck itself and
    /// returns first), so the game's own braking curve can never trigger a
    /// miss. The speed keeper is an assist too: holding the gate zone's own
    /// limit is what it was asked to do, and the gate hands the pedals back
    /// from it with a fresh window (`handle_arrival_gate`) rather than
    /// calling its 15.4 a miss.
    pub fn gate_miss_pending(&self) -> bool {
        self.trip.truck.speed_mph() > FACILITY_GATE_LIMIT_MPH
            && self.gate_speed_warned
            && self.gate_grace_s <= 0.0
            && self.hazard_deadline.is_none()
            && self.pull_over.is_none()
            && self.keeper_mph.is_none()
            && !self.arrival_menu_open
    }

    /// The rest key pressed a truck length short of a facility gate. The
    /// driver has stopped where the pre-gate warning told them to brake, and
    /// the emergency shoulder-sleep dialog is the wrong answer there (agent
    /// playtest, 2026-09-02: fifty feet from the gate, twice). Names the
    /// gate and the distance instead; None anywhere else.
    pub fn gate_short_hint(&self, ctx: &GameContext) -> Option<String> {
        let approaching_gate = self.phase == DRIVE_PHASE_PICKUP
            || self.destination_exit_taken
            || self.surface_chain
            || self.trip.is_facility_approach_route();
        // A road stop's streets end at its lot, not at a gate.
        if !approaching_gate || self.trip.finished || self.stop_chain.is_some() {
            return None;
        }
        let remaining = self.trip.remaining_miles();
        if remaining > FACILITY_GATE_ZONE_MI {
            return None;
        }
        let facility = self.approach_facility_text(ctx);
        Some(format!(
            "Gate at {facility}, {} ahead.",
            self.closing_text(remaining)
        ))
    }

    /// `_charge_scripted_loop(minutes)`: HOS, fatigue, and idle fuel for a
    /// scripted loop-back -- the spoken "the clock is still running" line must
    /// be literally true.
    ///
    /// The scripted reposition never passes through the per-frame loop that
    /// would otherwise apply these (`_update_hours_and_fatigue`,
    /// `TruckState::update_fuel`), so it charges the same rate math directly
    /// against the loop's fixed minutes instead of duplicating the constants
    /// those apply. Shared by the missed facility gate and the missed
    /// destination exit, whose loops are the same maneuver.
    pub fn charge_scripted_loop(&mut self, ctx: &mut GameContext, minutes: f64) {
        self.charge_scripted_hours(ctx, minutes);
        self.trip
            .truck
            .burn_idle_fuel_over_game_time(minutes * 60.0);
    }

    /// The hours and fatigue half of `charge_scripted_loop`, for a scripted
    /// drive that burns its own fuel (the chain checkpoint turn back).
    pub fn charge_scripted_hours(&mut self, ctx: &mut GameContext, minutes: f64) {
        // A turnaround is driving time on every commercial reposition.
        hos_mut_of(ctx).drive(minutes);
        let fatigue_mult = tuning_for_time_scale(self.trip.time_scale).fatigue_rate;
        let night = is_night(self.trip.local_hour());
        let now_h = self.absolute_game_hour(ctx, None);
        {
            let p = profile_mut_of(ctx);
            p.fatigue = 100.0f64.min(
                p.fatigue
                    + hos::fatigue_rate_per_min(night)
                        * minutes
                        * fatigue_mult
                        * p.fatigue_buff_rate(now_h),
            );
        }
    }

    /// `_handle_missed_facility_gate()`: the scripted loop-back through the
    /// next safe turnaround.
    pub fn handle_missed_facility_gate(&mut self, ctx: &mut GameContext) {
        self.trip.finished = false;
        self.gate_miss_count += 1;
        self.trip.game_minutes += GATE_MISS_LOOP_MIN;
        self.charge_scripted_loop(ctx, GATE_MISS_LOOP_MIN);
        // Drop back a full warning window, not a fixed distance: under time
        // compression a fixed stretch passes before it can be heard, making
        // the re-approach unwinnable (the missed-exit loop's lesson).
        self.trip.position_mi = 0.0f64.max(self.trip.total_miles() - self.gate_warning_window_mi());
        // The say-once latches must never swallow the reposition -- when the
        // missed-exit loop let them, a second miss stranded the trip pinned
        // at route end. The next approach warns and speaks fresh, and the
        // "still at the gate" reminder is unreachable until the trip ends
        // at the gate again.
        self.arrival_stop_said = false;
        self.arrival_full_stop_said = false;
        live::set_gate_stop_prompted(false);
        self.gate_reminder_s = 0.0;
        self.gate_speed_warned = false;
        self.gate_warning_spoken = false;
        self.gate_grace_s = 0.0;
        self.cancel_cruise(ctx, false);
        let target = ctx.settings.speed_text(FACILITY_GATE_LIMIT_MPH);
        let mut message = if self.terse_speech(ctx) {
            format!("Missed the gate, too fast. Slow to {target}.")
        } else {
            format!(
                "Missed the gate at {}, too fast for the entrance. You turned around, and the \
                 gate is ahead again. Slow to {target}.",
                self.destination_facility_text(ctx)
            )
        };
        if self.gate_miss_count >= 2 {
            // The identical core line keeps the flow predictable by ear; a
            // repeat miss earns help, not scolding.
            message += &format!(
                " Brake with {} well before the gate.",
                ctx.control_hint("brake")
            );
            if !ctx.settings.destination_approach_assist {
                message +=
                    " Facility stopping assistance in Settings, Gameplay, Driving assistance, \
                     can stop the truck for you.";
            }
        }
        ctx.audio.play("ui/warning");
        self.set_status("Missed the facility gate. Use the next safe turnaround.");
        // The mandatory destination gate, not an optional stop: names the
        // loop-back maneuver that still delivers the load, so it must survive
        // quiet/urgent_only as words, not an earcon blip.
        ctx.say_event_with(
            message,
            SayEvent::new().category(SpeechCategory::Navigation),
        );
    }
}
