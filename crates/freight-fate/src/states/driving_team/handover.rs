//! Handing the wheel over and taking it back, from the pause menu.

use ff_core::models::business_constants::is_owner_operator;
use ff_core::models::co_driver::{self, Pairing};
use ff_core::models::jobs::Job;
use ff_core::sim::hos::{self, HosClock};

use crate::app::{GameContext, Say};
use crate::states::base::MenuItem;
use crate::states::driving::DrivingState;
use crate::states::driving_core::profile_of;
use crate::states::driving_pause_states::PauseMenuState;

use super::pilot::StopPurpose;
use super::TeamMode;

impl DrivingState {
    /// Who would take the wheel: this load's co-driver, or the one dispatch
    /// would pair.
    pub fn team_pairing(&self, ctx: &GameContext) -> Pairing {
        if let Some(team) = &self.team {
            return Pairing {
                co_driver: team.co_driver,
                reason: team.reason,
            };
        }
        job_pairing(ctx, &self.job)
    }

    /// Whether the pause menu offers the handover now.
    pub fn handover_offered(&self, ctx: &GameContext) -> bool {
        // A co-driver back in the bunk before their own rest is done has no
        // hours to give.
        let rested = self
            .team
            .as_ref()
            .is_none_or(|team| !co_driver::shift_over(&team.off_wheel_hos, &ctx.settings.hos_mode));
        rested
            && !self.co_driver_driving()
            && self.pull_over.is_none()
            && self.hazard_deadline.is_none()
            && self.team_available()
    }
}

/// Who dispatch pairs with the player for `job`: the same answer at the
/// shipper and on the road.
pub fn job_pairing(ctx: &GameContext, job: &Job) -> Pairing {
    let profile = profile_of(ctx);
    let seed = co_driver::load_seed(
        &job.origin,
        &job.destination,
        job.cargo.key,
        job.weight_tons,
    );
    co_driver::pair(
        &profile.achievement_stats,
        &profile.business_status,
        profile.career.level(),
        &seed,
    )
}

/// The terms, as the handover rows' help reads them.
pub fn terms_text(ctx: &GameContext, name: &str) -> String {
    format!("{} {}", back_text(ctx, name), pay_text(ctx, name))
}

/// The pay and the logbook while the co-driver drives.
fn pay_text(ctx: &GameContext, name: &str) -> String {
    let owner = is_owner_operator(&profile_of(ctx).business_status);
    let pay = if owner {
        format!(
            "You pay {name} {} percent of the settlement for the miles they drive.",
            (co_driver::OWNER_CO_DRIVER_PAY_SHARE * 100.0).round()
        )
    } else {
        format!("Miles {name} drives pay you half.")
    };
    format!(
        "{pay} Those miles earn you half the experience. While {name} drives, you are logged in \
         the sleeper berth."
    )
}

/// The minutes the co-driver can drive for the player when their own hours
/// would end the hand-over: they run out before the receiver, and, with the
/// player's rest owed, before that rest is in. None otherwise, and with
/// hours off.
fn hours_end_handover(ctx: &GameContext, d: &DrivingState) -> Option<f64> {
    let mode = &ctx.settings.hos_mode;
    let fresh = HosClock::new();
    let co = d.team.as_ref().map_or(&fresh, |team| &team.off_wheel_hos);
    // A co-driver short of their own rest gets it in the sleeper while the
    // player drives; handed the wheel now, what counts is what they have.
    let usable = co_driver::usable_min(co, mode)?;
    if usable >= drive_min_left(ctx, d) {
        return None;
    }
    let player = &profile_of(ctx).hos;
    if !co_driver::fully_rested(player) && co_driver::rest_left_min(player) <= usable {
        return None;
    }
    Some(usable)
}

/// The planner's driving minutes from the truck to the receiver.
fn drive_min_left(ctx: &GameContext, d: &DrivingState) -> f64 {
    ff_core::models::jobs::route_drive_hours(
        Some(&d.trip.route),
        d.trip.position_mi,
        Some(ctx.world),
    ) * 60.0
}

/// When the co-driver's hours run out before the player's 10-hour rest is
/// in: how much driving they have, and where the shift ends. Before it, a
/// player learned that only after handing over (player report,
/// 2026-10-20), and a tester handed over to a co-driver with 85 minutes
/// left that only this help mentioned (tester report, 2026-11-19).
///
/// The flag is true when the text says the hand-back itself, awake with the
/// player's own hours reaching the receiver, so the help leaves out the
/// usual "once you have had your 10-hour rest", which would be untrue.
fn short_shift_text(ctx: &GameContext, d: &DrivingState, name: &str) -> Option<(String, bool)> {
    let mode = &ctx.settings.hos_mode;
    let player = &profile_of(ctx).hos;
    let usable = hours_end_handover(ctx, d)?;
    if co_driver::fully_rested(player) {
        return None;
    }
    let left = hos::duration_text(usable / 60.0);
    // Where their hours reach a lot, they park there; where they do not,
    // they stop on the shoulder (see `shoulder`).
    let here = d.trip.position_mi;
    let reach = usable / 60.0 * d.co_driver_pace_mph();
    let lot_in_reach = d
        .co_driver_stops(StopPurpose::ShiftEnd, here)
        .iter()
        .any(|stop| stop.at_mi <= here + reach);
    // The player's own logbook as it will stand when the co-driver's shift
    // ends: that much more time in the sleeper, that much less trip.
    let mut at_end = player.clone();
    at_end.sleeper(usable);
    let trip_after = (drive_min_left(ctx, d) - usable).max(0.0);
    if co_driver::player_finishes(&at_end, mode, trip_after) {
        let text = if lot_in_reach {
            format!(
                "{name} has {left} of driving left, then parks and hands you the wheel. Your own \
                 hours reach the receiver from there."
            )
        } else {
            format!(
                "{name} has {left} of driving left and no truck lot in reach, so they stop on the \
                 shoulder and hand you the wheel. Your own hours reach the receiver from there."
            )
        };
        return Some((text, true));
    }
    let text = if lot_in_reach {
        format!(
            "{name} has {left} of driving left, short of your 10-hour rest, so they park and you \
             finish it there."
        )
    } else {
        format!(
            "{name} has {left} of driving left, short of your 10-hour rest, and no truck lot in \
             reach, so they stop on the shoulder and you finish it there. Shoulder rest is poor \
             and risks a ticket or minor damage."
        )
    };
    Some((text, false))
}

/// The introduction to team driving, said once per career the first time a
/// co-driver is offered: at the shipper's dock (`at_dock`) or in the pause
/// menu. Before it, the rows turned up with nothing to say why (owner
/// report, 2026-10-19).
pub fn intro_text(name: &str, at_dock: bool) -> String {
    let take = if at_dock {
        format!(
            "{name} can drive from here, or take over later from the pause menu, while you rest \
             in the sleeper."
        )
    } else {
        format!("{name} can take the wheel from this menu while you rest in the sleeper.")
    };
    // When they hand back is on the rows' help with the pay terms, so the
    // load line stays short (owner, 2026-11-04).
    format!("This load is long enough to share with a co-driver. {take}")
}

/// When the co-driver hands the wheel back.
fn back_text(ctx: &GameContext, name: &str) -> String {
    let mode = &ctx.settings.hos_mode;
    if !co_driver::enforced(mode) {
        format!("{name} drives to the receiver's gate unless something needs you.")
    } else if co_driver::fully_rested(&profile_of(ctx).hos) {
        format!("{name} drives until their own hours run out.")
    } else {
        format!("{name} hands the wheel back once you have had your 10-hour rest.")
    }
}

/// The introduction, if this career has not heard it yet. The caller
/// marks it heard (`mark_intro_heard`) once it is actually spoken.
pub fn new_intro(ctx: &GameContext, name: &str, at_dock: bool) -> Option<String> {
    let profile = ctx.profile.as_ref()?;
    if co_driver::introduced(&profile.achievement_stats) {
        return None;
    }
    Some(intro_text(name, at_dock))
}

/// Record that this career has heard the introduction.
pub fn mark_intro_heard(ctx: &mut GameContext) {
    if let Some(profile) = ctx.profile.as_mut() {
        co_driver::mark_introduced(&mut profile.achievement_stats);
    }
}

/// The introduction, if this career has not heard it yet, marked heard:
/// for a caller that speaks it straight away.
pub fn take_intro(ctx: &mut GameContext, name: &str, at_dock: bool) -> Option<String> {
    let intro = new_intro(ctx, name, at_dock)?;
    mark_intro_heard(ctx);
    Some(intro)
}

impl PauseMenuState {
    /// The co-driver rows, placed after Resume driving.
    pub(crate) fn team_rows(
        &mut self,
        ctx: &mut GameContext,
        d: &DrivingState,
    ) -> Vec<MenuItem<PauseMenuState>> {
        if d.co_driver_driving() {
            let mut help = match d.co_driver_clock_clause(ctx) {
                Some(status) => format!("{status} Wakes you and gives you the wheel."),
                None => "Wakes you and gives you the wheel.".to_string(),
            };
            // Driving zeroes the unbroken rest, so a rest cut short is lost
            // (issue 520).
            if d.player_rest_left_min(ctx)
                .is_some_and(|left| left > 0.0 && left < ff_core::sim::hos::SLEEP_MIN)
            {
                help.push_str(" Your 10-hour rest starts over.");
            }
            return vec![MenuItem::new("Take the wheel back", |s: &mut Self, ctx| {
                s.take_the_wheel_back(ctx)
            })
            .help(help)];
        }
        if !d.handover_offered(ctx) {
            return Vec::new();
        }
        let pairing = d.team_pairing(ctx);
        let name = pairing.co_driver.name;
        // Kept until the menu's entry speaks it; a refresh that builds
        // these rows does not use it up.
        self.team_intro = new_intro(ctx, name, false);
        let carrier = ff_core::models::career::carrier_name_of(profile_of(ctx));
        let who = co_driver::pairing_line(&pairing, &carrier);
        let terms = match short_shift_text(ctx, d, name) {
            Some((short, true)) => format!("{short} {}", pay_text(ctx, name)),
            Some((short, false)) => format!("{short} {}", terms_text(ctx, name)),
            None => terms_text(ctx, name),
        };
        // When their hours end the hand-over, the rows say for how long.
        let (ride, sleep) = match hours_end_handover(ctx, d) {
            Some(usable) => {
                let time = hos::duration_text(usable / 60.0);
                (
                    format!("Hand the wheel to {name} for {time} and ride along"),
                    format!("Hand the wheel to {name} for {time} and sleep"),
                )
            }
            None => (
                format!("Hand the wheel to {name} and ride along"),
                format!("Hand the wheel to {name} and sleep"),
            ),
        };
        vec![
            MenuItem::new(ride, |s: &mut Self, ctx| {
                s.hand_over(ctx, TeamMode::RideAlong)
            })
            .help(format!(
                "{who} You rest in the sleeper and hear the road. {terms}"
            )),
            MenuItem::new(sleep, |s: &mut Self, ctx| {
                s.hand_over(ctx, TeamMode::SleepThrough)
            })
            .help(format!(
                "{who} You wake when {name} needs you, with a summary of the drive. {terms}"
            )),
        ]
    }

    fn hand_over(&mut self, ctx: &mut GameContext, mode: TeamMode) {
        let line = self
            .drive()
            .with(ctx, |d, ctx| {
                d.hand_wheel_to_co_driver(ctx, mode);
                d.team_pairing_line(ctx)
            })
            .unwrap_or_default();
        ctx.audio.play("ui/unpause");
        ctx.resume_event_speech();
        ctx.pop_state();
        ctx.player_asked(|ctx| ctx.say_with(line, Say::queued().review(false)));
    }

    fn take_the_wheel_back(&mut self, ctx: &mut GameContext) {
        self.drive().with(ctx, |d, ctx| d.take_wheel_back(ctx));
        ctx.audio.play("ui/unpause");
        ctx.resume_event_speech();
        ctx.pop_state();
        ctx.say_with("You have the wheel.", Say::queued().review(false));
    }
}
