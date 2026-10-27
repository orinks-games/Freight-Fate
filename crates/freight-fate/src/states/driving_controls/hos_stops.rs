//! Route advice and advance warnings for the next required rest.

use crate::app::{GameContext, SayEvent};
use crate::speech::{DeliveryStatus, EventPriority};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{hos_mut_of, hos_of};
use ff_core::models::jobs::hos_stops::{plan_hos_stop, HosStopAdvice, StopPlanningRoute};
use ff_core::models::jobs::route_drive_hours;
use ff_core::speech_pacing::SpeechCategory;

impl DrivingState {
    /// One opt-in suggestion before the normal 60-minute HOS warning. The
    /// marker lives in the ELD's saved warned set and clears with the relevant
    /// break, credited split, or full reset. Quiet rungs receive no tip or tone.
    pub fn maybe_hos_planning_hint(&mut self, ctx: &mut GameContext) {
        self.settle_hos_plan_hint(ctx, false);
        if self.hos_plan_hint_pending.is_some() {
            return;
        }
        if !ctx.settings.hos_planning_hints
            || !ctx.settings.speaks(Some(SpeechCategory::Coaching))
            || self.departure_chain
            || self.surface_chain
            || self.selected_stop_key.is_some()
            || self.ramp_stop.is_some()
            || self.exit_signal_on
            || self.hos_stop_warning_pending.is_some()
            || ctx.event_delivery_pending()
        {
            return;
        }
        let Some(limit) = hos_of(ctx).next_limit(&ctx.settings.hos_mode) else {
            return;
        };
        if limit.remaining_min <= 60.0 || limit.remaining_min > 180.0 {
            return;
        }
        let prefix = format!("{}:plan-hint:", limit.kind);
        let stop_warning_prefix = format!("{}:hos-stop:", limit.kind);
        if hos_of(ctx)
            .warned
            .iter()
            .any(|key| key.starts_with(&prefix) || key.starts_with(&stop_warning_prefix))
        {
            return;
        }
        let check_key = format!(
            "{}:{}",
            limit.kind,
            (limit.remaining_min * 4.0).floor() as i64
        );
        if self.hos_plan_hint_check_key.as_deref() == Some(&check_key) {
            return;
        }
        self.hos_plan_hint_check_key = Some(check_key);
        let Some(advice) = self.hos_stop_advice(ctx) else {
            return;
        };
        // A destination within legal reach needs no intermediate rest.
        // A new key after a rest lets the speech ladder speak this advice
        // again, while the saved prefix prevents duplicates in this window.
        let fallback_distance = ctx.settings.distance_text(advice.ahead_mi, false);
        let suggested_distance = advice
            .suggested
            .as_ref()
            .map(|option| ctx.settings.distance_text(option.ahead_mi, false))
            .unwrap_or_else(|| fallback_distance.clone());
        let hint = match self.co_driver_takes_the_sleep(ctx, &advice) {
            Some(name) if !advice.destination_reachable => {
                Some(format!("{name} can take the wheel from the pause menu."))
            }
            Some(_) => None,
            None => advice.planning_hint(&suggested_distance, &fallback_distance),
        };
        if let Some(message) = hint {
            let key = format!("{prefix}{:.0}", self.absolute_game_hour(ctx, None) * 60.0);
            ctx.reset_event_condition(&key);
            self.hos_plan_hint_pending = Some(key.clone());
            ctx.say_event_with(
                message,
                SayEvent::queued()
                    .key(&key)
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Coaching)
                    .receipt(),
            );
        }
    }

    fn settle_hos_plan_hint(&mut self, ctx: &mut GameContext, interrupt_pending: bool) {
        let Some(key) = self.hos_plan_hint_pending.clone() else {
            return;
        };
        match ctx.event_delivery_status(&key) {
            Some(DeliveryStatus::Pending) if !interrupt_pending => return,
            Some(DeliveryStatus::Completed) => {
                if !hos_of(ctx).warned.contains(&key) {
                    hos_mut_of(ctx).warned.push(key);
                }
            }
            Some(DeliveryStatus::Pending | DeliveryStatus::Interrupted) | None => {
                ctx.reset_event_condition(&key);
                self.hos_plan_hint_check_key = None;
            }
        }
        self.hos_plan_hint_pending = None;
    }

    pub(crate) fn settle_last_hos_stop_warning(
        &mut self,
        ctx: &mut GameContext,
        interrupt_pending: bool,
    ) {
        let Some(key) = self.hos_stop_warning_pending.clone() else {
            return;
        };
        match ctx.event_delivery_status(&key) {
            Some(DeliveryStatus::Pending) if !interrupt_pending => return,
            Some(DeliveryStatus::Completed) => {
                if !hos_of(ctx).warned.contains(&key) {
                    hos_mut_of(ctx).warned.push(key);
                }
            }
            Some(DeliveryStatus::Pending | DeliveryStatus::Interrupted) | None => {
                ctx.reset_event_condition(&key);
                self.hos_stop_check_key = None;
            }
        }
        self.hos_stop_warning_pending = None;
    }

    /// The player cut the warning off: that counts as having heard it.
    ///
    /// An interrupted warning is otherwise said again in full, which is
    /// right when another line cut it and wrong when the player did --
    /// every press of Control brought the whole line back, as many times
    /// as it was pressed (Shane's log, 12 September).
    pub(crate) fn acknowledge_last_hos_stop_warning(&mut self, ctx: &mut GameContext) {
        let Some(key) = self.hos_stop_warning_pending.take() else {
            return;
        };
        if !hos_of(ctx).warned.contains(&key) {
            hos_mut_of(ctx).warned.push(key);
        }
    }

    /// Everything a player's stop-speech key settles at once.
    pub(crate) fn warnings_stopped_by_player(&mut self, ctx: &mut GameContext) {
        self.note_critical_speech_stopped();
        self.acknowledge_last_hos_stop_warning(ctx);
        if let Some(key) = self.hos_plan_hint_pending.take() {
            if !hos_of(ctx).warned.contains(&key) {
                hos_mut_of(ctx).warned.push(key);
            }
        }
        self.acknowledge_maintenance_warnings();
    }

    /// On a team load, the rested co-driver's name when the rest ahead is
    /// a 10-hour sleep: the deadline has them take the wheel there, so a
    /// stop to sleep at is the wrong advice (player report, 2026-10-20).
    fn co_driver_takes_the_sleep(
        &self,
        ctx: &GameContext,
        advice: &HosStopAdvice,
    ) -> Option<&'static str> {
        (self.job.team && advice.action == "sleep" && self.handover_offered(ctx))
            .then(|| self.team_pairing(ctx).co_driver.name)
    }

    pub fn hos_stop_advice(&self, ctx: &GameContext) -> Option<HosStopAdvice> {
        let (trip, local_drive_min) = if self.departure_chain {
            let highway = self.highway_trip.as_ref()?;
            (
                highway,
                route_drive_hours(
                    Some(&self.trip.route),
                    self.trip.position_mi,
                    Some(ctx.world),
                ) * 60.0,
            )
        } else {
            (&self.trip, 0.0)
        };
        plan_hos_stop(
            &StopPlanningRoute {
                route: &trip.route,
                stops: &trip.stops,
                position_mi: trip.position_mi,
                bobtail: trip.bobtail,
                local_drive_min,
                world: Some(ctx.world),
            },
            hos_of(ctx),
            &ctx.settings.hos_mode,
        )
    }

    pub fn warn_last_hos_stop(&mut self, ctx: &mut GameContext) {
        self.settle_last_hos_stop_warning(ctx, false);
        if self.hos_stop_warning_pending.is_some() {
            return;
        }
        if ctx.event_delivery_pending() {
            return;
        }
        if self.departure_chain || self.surface_chain {
            return;
        }
        let Some(limit) = hos_of(ctx).next_limit(&ctx.settings.hos_mode) else {
            return;
        };
        // Route timing walks curves and speed zones. Only do it when an
        // HOS-capable stop enters the same real-time warning window used for
        // exits, then at most once per quarter game minute. A delay can still
        // change the answer while the stop remains ahead.
        let window_mi = self.exit_window_mi();
        let Some(candidate_key) = self.trip.stops.iter().find_map(|stop| {
            let ahead = stop.at_mi - self.trip.position_mi;
            let can_rest = stop
                .actions
                .iter()
                .any(|action| action == "break" || action == "sleep");
            ((0.0..=window_mi).contains(&ahead)
                && stop.parking != "none"
                && stop.accessible_to(self.trip.bobtail)
                && can_rest)
                .then(|| stop.key())
        }) else {
            return;
        };
        let candidate_warning_suffix = format!(":hos-stop:{candidate_key}");
        let candidate_warned = hos_of(ctx)
            .warned
            .iter()
            .any(|key| key.ends_with(&candidate_warning_suffix));
        let check_key = format!(
            "{}:{candidate_key}:{}:{candidate_warned}",
            limit.kind,
            (limit.remaining_min * 4.0).floor() as i64
        );
        if self.hos_stop_check_key.as_deref() == Some(&check_key) {
            return;
        }
        self.hos_stop_check_key = Some(check_key);
        let Some(advice) = self.hos_stop_advice(ctx) else {
            return;
        };
        if advice.destination_reachable || self.co_driver_takes_the_sleep(ctx, &advice).is_some() {
            return;
        }
        let Some(stop) = &advice.stop else {
            return;
        };
        if advice.ahead_mi > window_mi {
            return;
        }
        let key = format!("{}:hos-stop:{}", advice.limit_kind, stop.key());
        if hos_of(ctx).warned.contains(&key) {
            return;
        }
        let distance = ctx.settings.distance_text(advice.ahead_mi, false);
        let Some(message) = advice.road_warning(&distance) else {
            return;
        };
        ctx.reset_event_condition(&key);
        self.hos_stop_warning_pending = Some(key.clone());
        ctx.say_event_with(
            message,
            SayEvent::queued()
                .priority(EventPriority::Important)
                .key(&key)
                .category(SpeechCategory::Safety)
                .receipt(),
        );
    }
}
