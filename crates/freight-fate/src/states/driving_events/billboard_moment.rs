//! Everyday pool billboards that notice the drive, swapped at speak time
//! (`ff_core::data::billboards_dynamic` has the lines and the rules).
//!
//! The pool is drawn when the trip starts; what Big Jim and the small-hours
//! signs answer to only exists once the truck is moving, so the swap happens
//! as a pool sign is read. Placed attraction signs (`billboard_sign`) pass
//! through untouched.

use ff_core::data::billboards_dynamic::{
    dynamic_billboard, holiday_on, BillboardMoment, Incident, SMALL_HOURS,
};
use ff_core::sim::hos::FATIGUE_DROWSY;
use ff_core::sim::season::calendar_date;
use ff_core::speech_text::SpokenMessage;

use chrono::Datelike;

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::profile_of;

/// Incident damage that counts as hitting something: shoulder and lane
/// strikes add a percent or more, so half a percent is never wear noise.
const COLLISION_DAMAGE_PCT: f64 = 0.5;

/// What the record held when the last pool sign was read.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BillboardWatch {
    pub damage_pct: f64,
    pub citations: i64,
    pub out_of_service: usize,
}

impl BillboardWatch {
    pub fn now(ctx: &GameContext, damage_pct: f64) -> Self {
        let record = &profile_of(ctx).driving_record;
        BillboardWatch {
            damage_pct,
            citations: record.citations,
            out_of_service: record.out_of_service_times.len(),
        }
    }

    /// The incident Big Jim answers, the most serious first.
    fn incident_since(self, was: BillboardWatch) -> Option<Incident> {
        if self.out_of_service > was.out_of_service {
            Some(Incident::OutOfService)
        } else if self.citations > was.citations {
            Some(Incident::Citation)
        } else if self.damage_pct - was.damage_pct >= COLLISION_DAMAGE_PCT {
            Some(Incident::Collision)
        } else {
            None
        }
    }
}

impl DrivingState {
    /// The text a pool billboard is read with: its own, or a line about the
    /// drive. Every pool sign heard moves the watch on, so an incident is
    /// answered once.
    pub(super) fn billboard_message(
        &mut self,
        ctx: &GameContext,
        category: &str,
        message: SpokenMessage,
    ) -> SpokenMessage {
        if category != "billboard" {
            return message;
        }
        let now = BillboardWatch::now(ctx, self.trip.truck.damage_pct);
        let profile = profile_of(ctx);
        let date = calendar_date(profile.player_calendar_hours());
        let local_hour = self.trip.local_hour().rem_euclid(24.0);
        let state = self.trip.state_code_at(self.trip.position_mi);
        let moment = BillboardMoment {
            incident: now.incident_since(self.billboard_watch),
            tired: profile.fatigue >= FATIGUE_DROWSY || SMALL_HOURS.contains(&local_hour),
            holiday: holiday_on(date.month(), date.day()),
            nth: self.pool_billboards_heard,
            offset: self.billboard_offset,
            state: state.as_deref(),
        };
        self.billboard_watch = now;
        self.pool_billboards_heard += 1;
        match dynamic_billboard(moment) {
            Some(line) => SpokenMessage::new(format!("Billboard: {line}")),
            None => message,
        }
    }
}
