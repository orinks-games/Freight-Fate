//! Everywhere a drive stops and a menu takes over: route stops and their
//! fuel island, the loyalty desk, a full lot, the emergency shoulder, and
//! the three roadside enforcement outcomes (port of
//! `freight_fate/states/driving_rest_states.py`).
//!
//! # Python's two mixins
//!
//! * `_FuelPumpMixin` -- shared by `RestStopState` and `ParkingFullState`,
//!   because a lot filling up does not lock the pumps -- is the
//!   [`fuel_pump::FuelPump`] trait: four small accessors from the screen, the
//!   label, the purchase and the save as provided methods.
//! * `_RoadsideExitMixin` -- shared by the traffic stop and the enforcement
//!   stop -- is the [`roadside::RoadsideExit`] trait, same shape: the exit
//!   row, the suspended-exit wording, and closing the run out from the
//!   shoulder.
//!
//! Neither is a "mixin" in any Rust sense: a trait with provided methods and
//! no state, which is exactly what those two classes were.

mod fuel_pump;
mod loyalty;
mod parking_full;
mod rest_preview;
mod rest_stop;
mod roadside;
mod shoulder;

pub use fuel_pump::FuelPump;
pub use loyalty::LoyaltyRewardsState;
pub use parking_full::ParkingFullState;
pub use rest_stop::{RestFocus, RestStopState};
pub use roadside::{
    EnforcementStopState, FelonyStopState, LicencePulledState, RoadsideExit, TrafficStopState,
};
pub use shoulder::ShoulderSleepConfirmationState;

use ff_core::models::enforcement;
use ff_core::pyfmt::fmt_f;
use ff_core::sim::trip_models::RoadStop;

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::{profile_mut_of, profile_of};
use crate::states::driving_menu_states::{push_over_drive, DriveRef};
use crate::states::driving_team::notes::NoteWeight;
use crate::states::driving_updates::pending::EnforcementStopParams;

/// Leaving a stop menu. The engine is whatever it already was: "starts the
/// engine" over a running one sent the driver's press to shut it down.
pub fn back_on_the_road_line(engine_on: bool) -> String {
    if engine_on {
        "Parking brake set.".to_string()
    } else {
        "Engine off, parking brake set.".to_string()
    }
}

/// Where the career clock stands right now, mid-trip included.
pub fn record_hours(ctx: &GameContext, driving: &DrivingState) -> f64 {
    profile_of(ctx).game_hours + driving.trip.game_minutes / 60.0
}

/// What a fresh timed suspension or disqualification means, and what still
/// works while it runs.
pub fn suspension_text(ctx: &GameContext, hours: f64, verb: &str) -> String {
    let profile = profile_of(ctx);
    let left = enforcement::days_text(profile.driving_record.days_left(hours));
    format!(
        "Your CDL is {verb} for {left}. Driving jobs are off the dispatch board until it clears, \
         {}. Your money and your truck are safe; rest, repairs, and the garage are still open.",
        enforcement::clears_text(profile)
    )
}

/// Spoken movement on the serious-violation ladder, consequence attached.
pub fn serious_violation_text(ctx: &GameContext, count: i64, hours: f64) -> String {
    if count <= 1 {
        return "That is a serious violation on your record. One more inside three years and your \
                CDL is suspended for 60 days, and driving jobs stop until it clears."
            .to_string();
    }
    let which = enforcement::ordinal_word(count);
    format!(
        "That is your {which} serious violation in three years. {}",
        suspension_text(ctx, hours, "suspended")
    )
}

/// The major-offense outcome, said as fact with the way forward.
pub fn major_offense_text(ctx: &GameContext, kind: &str, hours: f64) -> String {
    let raw_name = profile_of(ctx).name.clone();
    let name = if raw_name.is_empty() {
        "This driver".to_string()
    } else {
        raw_name
    };
    if kind == enforcement::SUSPENSION_LIFETIME {
        return format!(
            "That is your second major offense. Under federal rules a second major offense \
             disqualifies a commercial license for life, so this driver will not drive \
             commercially again. Nothing is taken away: {name} keeps every dollar, the truck, \
             and the whole record, and you can open this career any time to look back over it. \
             Rest, repairs, and the garage still work here, and the dispatch \
             board can still be read, but there is no driving work and no date this clears. When \
             you want the road again, start a new career from the title menu. Everything you \
             learned still applies."
        );
    }
    format!(
        "Running from a police stop in a commercial vehicle is a felony, and a major offense on \
         your CDL, which is a one-year disqualification. {} One more major offense is a lifetime \
         disqualification.",
        suspension_text(ctx, hours, "disqualified")
    )
}

/// Speak a repeat count the way a person would: 'twice now', 'three times
/// now' -- never frozen at the second occurrence's wording.
pub fn times_now_text(count: i64) -> String {
    if count == 2 {
        return "twice now".to_string();
    }
    format!("{} times now", enforcement::count_word(count))
}

// -- the record-keeping the roadside screens share ---------------------------------------
//
// The two blocks `driving_updates/pending.rs` held for this module, plus the
// three screens it pushes and the two `driving_events/pending.rs` held.

impl DrivingState {
    /// A minor violation by a new driver gets a warning: no fine, no
    /// reputation, nothing on the record. Serious violations and
    /// out-of-service orders are decided by the caller and still count.
    pub fn new_driver_warning(&self, ctx: &GameContext) -> bool {
        ctx.profile
            .as_ref()
            .is_some_and(|profile| enforcement::new_driver_warned(profile.career.level()))
    }

    /// Book a spoken roadside enforcement event onto the career record.
    ///
    /// Only enforcement the player actually heard reaches the record. The
    /// silent at-delivery settlement strike stays money-only, so a suspension
    /// can never materialise at a delivery summary with no warning behind it.
    pub fn log_enforcement(
        &mut self,
        ctx: &mut GameContext,
        fine: f64,
        serious: bool,
        major: bool,
        reason: &str,
    ) -> String {
        if ctx.profile.is_none() || self.enforcement_bypassed(ctx) {
            // the debug hours modes freeze the ladder as well as the stop
            return String::new();
        }
        let hours = record_hours(ctx, self);
        let place = self.record_place(ctx);
        let kind = if major {
            enforcement::RECORD_MAJOR
        } else if serious {
            enforcement::RECORD_SERIOUS
        } else {
            enforcement::RECORD_CITATION
        };
        {
            let record = &mut profile_mut_of(ctx).driving_record;
            record.record_citation_at(fine, hours);
            record.note(kind, reason, fine, hours, &place);
        }
        let text = if major {
            let kind = profile_mut_of(ctx)
                .driving_record
                .record_major_offense(hours);
            if kind == enforcement::SUSPENSION_LIFETIME {
                // The career is over. The roadside line says so now; the
                // terminal reads the full notice once, re-readable, the way
                // it reads a termination or a repossession.
                let record = &mut profile_mut_of(ctx).driving_record;
                record.setback_notice_kind = enforcement::SETBACK_DISQUALIFICATION.to_string();
                record.setback_notice_lines = enforcement::disqualification_notice_lines();
            }
            major_offense_text(ctx, kind, hours)
        } else if serious {
            let count = profile_mut_of(ctx)
                .driving_record
                .record_serious_violation(hours);
            serious_violation_text(ctx, count, hours)
        } else {
            return String::new();
        };
        self.record_events.push(text.clone());
        text
    }

    /// Book running off the road asleep, and say what it just cost.
    pub fn log_fatigue_event(&mut self, ctx: &mut GameContext) -> String {
        let hours = record_hours(ctx, self);
        let hit = enforcement::FATIGUE_EVENT_REPUTATION_HIT;
        let place = self.record_place(ctx);
        let (count, serious) = {
            let record = &mut profile_mut_of(ctx).driving_record;
            let booked = record.record_fatigue_event(hours);
            let kind = if booked.1 > 0 {
                enforcement::RECORD_SERIOUS
            } else {
                enforcement::RECORD_FATIGUE
            };
            record.note(kind, "Ran off the road asleep", 0.0, hours, &place);
            booked
        };
        let text = if count < enforcement::FATIGUE_EVENTS_BEFORE_SERIOUS {
            format!(
                "Running off the road asleep is a preventable safety incident and it goes on \
                 your record: {} points off your reputation. Do it again and it becomes a \
                 fatigued-driving violation on your CDL.",
                fmt_f(hit, 0)
            )
        } else {
            format!(
                "That is {} that you have run off the road asleep. Driving impaired by fatigue \
                 is a federal violation, so this one counts against your license as well as {} \
                 points off your reputation. {}",
                times_now_text(count),
                fmt_f(hit, 0),
                serious_violation_text(ctx, serious, hours)
            )
        };
        self.record_events.push(text.clone());
        text
    }

    /// `ctx.push_state(RestStopState(ctx, self, stop, preferred_rest=...))`.
    pub fn push_rest_stop_state(
        &mut self,
        ctx: &mut GameContext,
        stop: &RoadStop,
        preferred_rest: RestFocus,
    ) {
        let mut state = RestStopState::new(ctx, stop.clone(), preferred_rest);
        state.enter_over_drive(ctx, self);
        push_over_drive(ctx, state);
    }

    /// `ctx.push_state(ParkingFullState(ctx, self, stop))`.
    pub fn push_parking_full_state(&mut self, ctx: &mut GameContext, stop: &RoadStop) {
        let mut state = ParkingFullState::new(ctx, stop.clone());
        state.enter_over_drive(ctx, self);
        push_over_drive(ctx, state);
    }

    /// `ctx.push_state(ShoulderSleepConfirmationState(ctx, self, reason, mi))`.
    pub fn push_shoulder_sleep_confirmation(
        &mut self,
        ctx: &mut GameContext,
        reason: &str,
        anchor_mi: f64,
    ) {
        // Pushed from the drive's own handler, so it IS the direct-from-
        // driving case; nothing in this screen's entry reads the drive.
        let state = ShoulderSleepConfirmationState::new(
            DriveRef::active(ctx),
            reason,
            Some(anchor_mi),
            true,
        );
        ctx.push_state(state);
    }

    /// `ctx.push_state(TrafficStopState(...))`.
    #[allow(clippy::too_many_arguments)]
    pub fn push_traffic_stop_state(
        &mut self,
        ctx: &mut GameContext,
        signaled: bool,
        over: f64,
        limit: f64,
        clean_stop: bool,
        warned: bool,
        construction_zone: bool,
    ) {
        let state = TrafficStopState::new(
            ctx,
            self,
            signaled,
            over,
            limit,
            clean_stop,
            warned,
            construction_zone,
        );
        if self.co_driver_settles_roadside(ctx, state.outcome_text(), state.licence_pulled(ctx)) {
            return;
        }
        ctx.push_state(state);
    }

    /// A roadside stop with the co-driver at the wheel: it is settled when
    /// the screen is built, so they take the outcome, note it and drive on,
    /// with no screen at all. True when that is done. A stop that pulls the
    /// license ends the run, so they hand back and the screen is the
    /// player's.
    fn co_driver_settles_roadside(
        &mut self,
        ctx: &mut GameContext,
        outcome: &str,
        licence_pulled: bool,
    ) -> bool {
        let Some(name) = self.co_driver_at_wheel_name() else {
            return false;
        };
        if licence_pulled {
            self.co_driver_summon(ctx, "roadside");
            return false;
        }
        let outcome = co_driver_outcome(outcome, name);
        let line = format!("A trooper pulled {name} over. {outcome}");
        self.co_driver_note(ctx, line.trim_end().to_string(), NoteWeight::Wake);
        true
    }

    /// `ctx.push_state(EnforcementStopState(...))`.
    pub fn push_enforcement_stop_state(
        &mut self,
        ctx: &mut GameContext,
        stop: EnforcementStopParams,
    ) {
        let state = EnforcementStopState::new(ctx, self, stop);
        let outcome = format!("{} {}", state.summary, state.outcome_text());
        if self.co_driver_settles_roadside(ctx, outcome.trim(), state.licence_pulled(ctx)) {
            return;
        }
        ctx.push_state(state);
    }

    /// End the drive when the CDL was just pulled with no officer present.
    ///
    /// For the record events that happen at speed (a run off the road
    /// asleep, the barrels): driving on with a suspended or disqualified CDL
    /// is exactly what the suspension forbids, so the truck stops on the
    /// shoulder and the roadside exit closes out the run. The debug hours
    /// modes, which freeze the ladder, never end a run.
    pub fn end_drive_if_licence_pulled(&mut self, ctx: &mut GameContext) {
        let pulled = ctx
            .profile
            .as_ref()
            .is_some_and(|p| p.driving_record.suspended(record_hours(ctx, self)));
        if !pulled || self.enforcement_bypassed(ctx) {
            return;
        }
        self.trip.truck.velocity_mps = 0.0;
        self.trip.truck.throttle = 0.0;
        self.trip.truck.brake = 1.0;
        self.trip.truck.set_parking_brake();
        let state = LicencePulledState::new(ctx, self);
        ctx.push_state(state);
    }

    /// `ctx.push_state(FelonyStopState(ctx, self))`.
    pub fn push_felony_stop_state(&mut self, ctx: &mut GameContext) {
        let state = FelonyStopState::new(ctx, self);
        ctx.push_state(state);
    }
}

/// A stop screen's outcome, told of the co-driver who was at the wheel.
/// The screen speaks to the player ("lets you off"); the co-driver's note
/// must not say the trooper stopped them. "Your record" stays: the record
/// is the player's career.
fn co_driver_outcome(outcome: &str, name: &str) -> String {
    outcome
        .replace("lets you off", &format!("lets {name} off"))
        .replace("lets you go", &format!("lets {name} go"))
        .replace(
            "You pulled over promptly",
            &format!("{name} pulled over promptly"),
        )
        .replace("waves you through", "waves the truck through")
}

#[cfg(test)]
mod tests {
    use super::co_driver_outcome;

    #[test]
    fn the_co_drivers_stop_is_told_of_them_not_the_player() {
        let warning = co_driver_outcome(
            "12 miles per hour over the 65 limit. You pulled over promptly, so the trooper lets it go with a warning.",
            "Dan",
        );
        assert!(warning.contains("Dan pulled over promptly"), "{warning}");
        let off = co_driver_outcome("The trooper lets you off with a warning.", "Dan");
        assert_eq!(off, "The trooper lets Dan off with a warning.");
        assert!(
            !co_driver_outcome("The officer lets you go with a warning.", "Dan").contains("you")
        );
    }
}
