//! The `driving_core.py` helpers that take the driving state itself
//! (`_advance_rest_clock`, `_secure_truck_for_stopped_menu`,
//! `_shut_down_engine`, `_wake_air_instruction`, `_deadline_*`,
//! `_perform_shoulder_sleep`). Free functions over `&mut DrivingState` and
//! the context, as they were module functions in Python; the rest-stop and
//! pause menus call them.

use ff_core::models::enforcement::{
    career_citations, citation_fine, construction_zone_fine_clause,
};
use ff_core::pyfmt::fmt_grouped;
use ff_core::sim::driving_modes::tuning_for_time_scale;
use ff_core::sim::hos::{self, clock_text};
use ff_core::sim::timezones::city_zone;

use crate::app::GameContext;
use crate::states::driving::DrivingState;

use super::{hos_mut_of, profile_mut_of, profile_of, set_engine_running, DOCKING_MAX_MPH};

/// Resting advances game time, so deadlines keep counting.
///
/// A running engine idles through the wait and burns fuel for it, as it
/// does at the pickup and the dock; every sleep shuts it down first.
/// `duty_status` (a `hos::DUTY_STATUSES` name) records the rest on the
/// logbook; `note` defaults to "".
pub fn advance_rest_clock(
    driving: &mut DrivingState,
    ctx: &mut GameContext,
    minutes: f64,
    duty_status: Option<&str>,
    note: &str,
) {
    let start_hour = driving.absolute_game_hour(ctx, None);
    driving
        .trip
        .truck
        .burn_idle_fuel_over_game_time(minutes * 60.0);
    driving.trip.truck.advance_parked_time(minutes);
    // Reefer/APU keep burning (and cargo temp keeps drifting) through a rest.
    let rest_s = minutes * 60.0;
    driving.trip.truck.advance_hotel_power(rest_s, rest_s);
    driving.trip.game_minutes += minutes;
    driving.trip.weather.update(minutes);
    // The cattle's limit, the heat and an animal down keep running.
    driving.advance_livestock_parked(ctx, minutes);
    // Whoever is off the wheel rests through the stop too.
    driving.team_menu_minutes(ctx, minutes);
    // While the co-driver works the stop, the player's own log reads the
    // sleeper berth.
    let duty_status = duty_status.map(|status| {
        if driving.co_driver_driving() {
            "sleeper_berth"
        } else {
            status
        }
    });
    let end_hour = driving.absolute_game_hour(ctx, None);
    if let Some(status) = duty_status {
        let location = driving.logbook_location(ctx);
        profile_mut_of(ctx)
            .duty_log
            .record(status, start_hour, end_hour, &location, note);
    }
}

/// A coffee or energy drink's lift that wore off during a sleep is paid
/// back before the sleep's relief is worked out, and silently: the rest
/// preview already read the figure. Call it after the sleep's
/// [`advance_rest_clock`]. Every other block leaves the loan for the road,
/// where its one line is spoken once the driver is back at the wheel.
pub fn settle_lift_before_sleep(driving: &DrivingState, ctx: &mut GameContext) {
    let now_h = driving.absolute_game_hour(ctx, None);
    profile_mut_of(ctx).repay_stimulant_loan(now_h);
}

/// Time on duty not driving at a stop: fueling, the walk-around, a scale,
/// an inspection, dock work, chain work, a repair waited out at the truck.
/// The same as [`advance_rest_clock`], plus the duty clock and the fatigue
/// the work builds ([`accrue_on_duty_fatigue`]).
pub fn advance_on_duty_clock(
    driving: &mut DrivingState,
    ctx: &mut GameContext,
    minutes: f64,
    duty_status: Option<&str>,
    note: &str,
) {
    accrue_on_duty_fatigue(driving, ctx, minutes);
    advance_rest_clock(driving, ctx, minutes, duty_status, note);
    hos_mut_of(ctx).on_duty(minutes);
}

/// Fatigue for `minutes` on duty not driving, starting now: a third of the
/// driving rate for the hour of day (`hos::on_duty_fatigue_gain`), scaled by
/// the driving mode and any food or drink buff the way driving is. Call it
/// before the clock moves. Nothing while the co-driver has the wheel: the
/// player is in the bunk then.
pub fn accrue_on_duty_fatigue(driving: &DrivingState, ctx: &mut GameContext, minutes: f64) {
    if driving.co_driver_driving() {
        return;
    }
    let start_local = driving.trip.local_hour();
    let now_h = driving.absolute_game_hour(ctx, None);
    let mode_rate = tuning_for_time_scale(driving.trip.time_scale).fatigue_rate;
    let p = profile_mut_of(ctx);
    let gain = hos::on_duty_fatigue_gain(start_local, minutes, |h| {
        mode_rate * p.fatigue_buff_rate(now_h + h)
    });
    p.fatigue = (p.fatigue + gain).min(100.0);
}

/// The minutes hand work of `base_min` takes in town, where no drive
/// tracks the band: the band the fatigue number falls in, read the way
/// [`DrivingState::hand_work_min`] reads it on the road.
pub fn town_hand_work_min(ctx: &GameContext, base_min: f64) -> f64 {
    let fatigue = ctx.profile.as_ref().map_or(0.0, |p| p.fatigue);
    let relaxed = tuning_for_time_scale(ctx.settings.time_scale).name == "relaxed";
    ff_core::sim::alertness::Alertness::from_fatigue(fatigue).hand_work_min(base_min, relaxed)
}

/// Atomically secure a slow truck before a menu freezes driving physics.
/// (`max_mph` defaults to `DOCKING_MAX_MPH`; see [`secure_truck_for_stopped_menu`].)
pub fn secure_truck_for_stopped_menu_at(
    driving: &mut DrivingState,
    ctx: &mut GameContext,
    max_mph: f64,
) -> bool {
    let truck = &mut driving.trip.truck;
    if truck.speed_mph() > max_mph {
        return false;
    }
    truck.velocity_mps = 0.0;
    truck.throttle = 0.0;
    truck.brake = 1.0;
    truck.set_parking_brake();
    driving.cancel_cruise_for_parking_brake(ctx);
    // The parking brake holds the truck now, so every assist lets go of the
    // service brake. The brake ramp keeps whatever pedal an assist is
    // holding, so a hold left latched under the menu kept the brakes on
    // after the driver drove off: 0 mph at full revs with the parking brake
    // released (Shane, Jerry and Jessie, 2026-09-21).
    driving.destination_arrival_active = false;
    driving.destination_assist_brake = 0.0;
    driving.keeper_snub = 0.0;
    driving.keeper_corner_braking = false;
    driving.aeb_brake = 0.0;
    driving.curve_servo = None;
    true
}

/// `_secure_truck_for_stopped_menu(driving)` with the default speed ceiling.
pub fn secure_truck_for_stopped_menu(driving: &mut DrivingState, ctx: &mut GameContext) -> bool {
    secure_truck_for_stopped_menu_at(driving, ctx, DOCKING_MAX_MPH)
}

/// Stop the engine before a night's sleep; no truck idles through ten
/// hours. Returns the spoken prefix, empty when it was already off.
pub fn shut_down_engine(driving: &mut DrivingState, ctx: &mut GameContext) -> String {
    if !driving.trip.truck.engine_on {
        return String::new();
    }
    set_engine_running(ctx, &mut driving.trip.truck, false);
    "You shut down the engine. ".to_string()
}

/// The truck's state after a rest that left the air bled down: the engine
/// (when it is off) and the pressure the driver is waiting on.
pub fn wake_air_instruction(driving: &DrivingState) -> String {
    let truck = &driving.trip.truck;
    if truck.air_ready() {
        return String::new();
    }
    let psi = truck.air_pressure_psi();
    if truck.engine_on {
        format!(" Air {psi:.0} psi.")
    } else {
        format!(" Engine off, air {psi:.0} psi.")
    }
}

/// The delivery appointment in the receiving city's local time.
///
/// Anchored on the job's destination, not the current trip's endpoint: a
/// pickup drive ends at the origin facility, possibly in another zone.
pub fn deadline_appointment(driving: &DrivingState, ctx: &GameContext) -> String {
    let zone = ctx
        .world
        .city(&driving.job.destination)
        .ok()
        .map(|city| city_zone(city));
    driving
        .trip
        .deadline_clock_text(driving.job.deadline_game_h, zone)
}

pub fn deadline_text(driving: &DrivingState, ctx: &GameContext) -> String {
    let remaining = driving.job.deadline_game_h - driving.trip.game_minutes / 60.0;
    if remaining > 0.0 {
        // The appointment reads in the receiver's local time, the way a real
        // dispatcher quotes it -- the zone name keeps it unambiguous mid-route.
        return format!(
            "{remaining:.1} hours left to deliver, by {}.",
            deadline_appointment(driving, ctx)
        );
    }
    format!("You are now {:.1} hours past the deadline.", -remaining)
}

/// Apply the emergency shoulder-sleep outcome and return spoken text.
pub fn perform_shoulder_sleep(
    driving: &mut DrivingState,
    ctx: &mut GameContext,
    anchor_mi: f64,
    at_chain_checkpoint: bool,
) -> String {
    let engine_off = shut_down_engine(driving, ctx);
    advance_rest_clock(driving, ctx, hos::SLEEP_MIN, None, "");
    settle_lift_before_sleep(driving, ctx);
    hos_mut_of(ctx).sleep();
    {
        let p = profile_mut_of(ctx);
        p.fatigue = hos::rest_poor_sleep(p.fatigue, hos::SLEEP_MIN);
    }
    let mut parts = vec![if at_chain_checkpoint {
        format!(
            "{engine_off}You sleep poorly at the chain control, woken again and again by \
             passing trucks. It is {}. Driving and duty clocks reset, but you are still tired.{}",
            clock_text(driving.trip.local_hour()),
            wake_air_instruction(driving)
        )
    } else {
        format!(
            "{engine_off}You sleep poorly on the shoulder, woken again and again by passing \
             trucks. It is {}. Driving and duty clocks reset, but you are still tired.{}",
            clock_text(driving.trip.local_hour()),
            wake_air_instruction(driving)
        )
    }];
    // Held at a chain checkpoint, the officer keeps the truck there: no
    // parking ticket and no shoulder debris.
    if !at_chain_checkpoint {
        parts.extend(shoulder_risks(driving, ctx, anchor_mi));
    } else if let Some(line) = driving.recheck_chain_checkpoint_hold_after_rest(ctx) {
        parts.push(line.to_string());
    }
    let snapshot = driving.snapshot(ctx);
    {
        let p = profile_mut_of(ctx);
        p.store_truck_condition(&driving.trip.truck);
        p.active_trip = Some(snapshot);
    }
    ctx.save_profile();
    parts.push(deadline_text(driving, ctx));
    parts.join(" ")
}

/// The risks of a night parked on the shoulder at `anchor_mi`: a parking
/// ticket and roadside damage, each seeded by the place, applied here, and
/// the spoken line for each that came due.
pub fn shoulder_risks(
    driving: &mut DrivingState,
    ctx: &mut GameContext,
    anchor_mi: f64,
) -> Vec<String> {
    let mut parts = Vec::new();
    if hos::shoulder_fine_due(driving.trip_seed, anchor_mi) {
        let zone = driving.trip.in_construction_zone();
        let fine = citation_fine(
            hos::SHOULDER_FINE,
            career_citations(profile_of(ctx)),
            zone,
            None,
        );
        let money = {
            let p = profile_mut_of(ctx);
            p.spend(fine);
            p.money()
        };
        ctx.audio.play("ui/error");
        parts.push(format!(
            "A trooper ticketed you for illegal parking, {} dollars.{} You have {} dollars.",
            fmt_grouped(fine, 0),
            construction_zone_fine_clause(zone),
            fmt_grouped(money, 0)
        ));
    }
    if hos::shoulder_damage_due(driving.trip_seed, anchor_mi) {
        driving.trip.truck.add_damage_from(
            hos::SHOULDER_DAMAGE_PCT,
            true,
            "stopping on the shoulder",
        );
        parts.push(format!(
            "Roadside debris and wake turbulence added {:.0} percent truck damage.",
            hos::SHOULDER_DAMAGE_PCT
        ));
    }
    parts
}
