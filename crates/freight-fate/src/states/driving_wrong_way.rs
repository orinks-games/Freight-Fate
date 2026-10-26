//! Backing down a live road: the one manoeuvre nothing used to object to
//! (port of `freight_fate/states/driving_wrong_way.py`, the `WrongWayMixin`).
//!
//! Reverse exists for the yard and the dock, and the sim already prices its
//! abuse -- the box refuses reverse above a walking pace, and holding a loaded
//! rig against its governor in reverse wears the engine out. What none of that
//! covers is *where*. The adversarial harness backed a tractor-trailer a full
//! mile down an interstate to route mile zero, and the only thing the game
//! said in all that time was a merge instruction for the exit it was reversing
//! away from. A sighted player would at least see the world sliding the wrong
//! way; a blind player had nothing at all, which makes this an accessibility
//! gap before it is a realism one.
//!
//! Backing on a traveled lane is prohibited outright on a controlled-access
//! highway and is how real drivers get hit, so the ladder here is short and it
//! escalates on distance rather than time:
//!
//! * **the reminder** -- past a truck length or two, say which way the truck
//!   is going and what it is undoing. This is the line the harness was looking
//!   for.
//! * **the warning** -- past a tenth of a mile, name it as illegal, because by
//!   then it is not a misjudged dock approach, and give the distance back so
//!   the player knows how far they have to make up.
//! * **traffic** -- past a quarter mile, the road stops being empty. Reversing
//!   into a live lane is a collision waiting to happen, and the collision is
//!   what a real driver would meet.
//!
//! Legitimate backing is exempt: the yard, the dock approach, a facility gate
//! zone, and anywhere within reach of a stop the player pulled into. A driver
//! lining up on a dock must never be scolded for doing their job.

use ff_core::sim::trip_models::FACILITY_GATE_ZONE_MI;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

// The rungs, in miles backed along the route since reverse was engaged.
pub const WRONG_WAY_REMIND_MI: f64 = 0.02; // about a hundred feet: past any dock manoeuvre
pub const WRONG_WAY_WARN_MI: f64 = 0.10;
pub const WRONG_WAY_TRAFFIC_MI: f64 = 0.25;
/// How far the warning repeats after that, so a player who keeps going keeps
/// hearing it with a fresh distance rather than once and then silence.
pub const WRONG_WAY_REPEAT_MI: f64 = 0.25;
/// Backing into a live lane. Modest per hit -- the point is the event and the
/// noise it makes, not a one-shot kill -- but it repeats for as long as the
/// truck keeps going the wrong way.
pub const WRONG_WAY_COLLISION_SEVERITY: f64 = 0.3;
/// Anywhere a driver has real business in reverse.
pub const WRONG_WAY_STOP_RADIUS_MI: f64 = 0.3;

impl DrivingState {
    /// True where backing is the job: the yard, a dock, a stop's lot.
    pub fn reverse_is_legitimate(&self) -> bool {
        let trip = &self.trip;
        if trip.position_mi <= WRONG_WAY_STOP_RADIUS_MI {
            return true; // still in the origin yard
        }
        if trip.position_mi >= trip.total_miles() - FACILITY_GATE_ZONE_MI {
            return true; // lining up on the receiver
        }
        trip.nearest_stop_within(WRONG_WAY_STOP_RADIUS_MI).is_some()
    }

    pub fn reset_wrong_way(&mut self) {
        self.wrong_way_mi = 0.0;
        self.wrong_way_said_at = 0.0;
    }

    /// Watch how far the truck has travelled backwards along the route.
    pub fn update_wrong_way(&mut self, ctx: &mut GameContext, _dt: f64) {
        let backing =
            self.trip.truck.transmission.in_reverse() && self.trip.truck.speed_mph() > 0.5;
        if !backing || self.reverse_is_legitimate() {
            self.reset_wrong_way();
            return;
        }
        // Distance along the route, not wheel rotations: what matters is how
        // much of the trip is being undone.
        self.wrong_way_mi += self.trip.last_moved_mi.abs();
        let backed = self.wrong_way_mi;
        let said_at = self.wrong_way_said_at;

        // Fires AT the rung, then repeats on the interval. Gating the first
        // one behind the repeat distance too would have let the truck reach a
        // third of a mile back down the road before traffic noticed it.
        let reached_traffic =
            said_at < WRONG_WAY_TRAFFIC_MI || backed - said_at >= WRONG_WAY_REPEAT_MI;
        if backed >= WRONG_WAY_TRAFFIC_MI && reached_traffic {
            self.wrong_way_said_at = backed;
            ctx.audio.play("ui/warning");
            ctx.say_event_with(
                "You are backing into traffic. Stop, select a forward gear, and get the truck \
                 pointed the right way.",
                SayEvent::new().category(SpeechCategory::Safety),
            );
            let severity = WRONG_WAY_COLLISION_SEVERITY;
            ctx.audio.play("vehicle/collision");
            ctx.controller.rumble.impact(severity);
            self.trip.truck.apply_collision(severity, true);
            let damage_pct = self.trip.truck.damage_pct;
            ctx.say_event_with(
                format!("Something hit the trailer. Total damage {damage_pct:.0} percent."),
                SayEvent::new().category(SpeechCategory::Safety),
            );
            return;
        }
        if backed >= WRONG_WAY_WARN_MI && said_at < WRONG_WAY_WARN_MI {
            self.wrong_way_said_at = backed;
            ctx.audio.play("ui/warning");
            // Precise: at this rung the distance is a tenth of a mile, and the
            // whole-number form would say "0 miles" to a driver who has just
            // been told they are giving the route back.
            let distance = ctx.settings.distance_text(backed, true);
            ctx.say_event_with(
                format!(
                    "You are driving the wrong way. Backing on a traveled lane is illegal, and \
                     you have given up {distance} of the route. Stop and select a forward gear."
                ),
                SayEvent::new().category(SpeechCategory::Safety),
            );
            return;
        }
        if backed >= WRONG_WAY_REMIND_MI && said_at < WRONG_WAY_REMIND_MI {
            self.wrong_way_said_at = backed;
            ctx.say_event_with(
                "You are still in reverse, backing away from your destination.",
                // Never dropped, for the same reason as the hazard follow-up:
                // a reminder that you are still going backwards down a live
                // lane is not chatter, however busy the road has got.
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Safety),
            );
        }
    }
}
