//! The two lines at the edges of a runaway (issue 533).
//!
//! The damage itself is the truck's (`TruckState::update_wear`); this block
//! only speaks the state `ff_core::sim::runaway::Runaway` moves through.

use ff_core::sim::runaway::{RunawayEvent, RunawayTransition};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;

impl DrivingState {
    /// Feed the runaway machine this frame's speed and speak its edges.
    ///
    /// A wreck or the co-driver's shift ends it without a word: the wreck has
    /// its own line, and the co-driver's driving speaks only through what
    /// their other damage says.
    pub fn update_runaway(&mut self, ctx: &mut GameContext) {
        let event = if self.co_driver_driving() || self.trip.truck.out_of_service() {
            RunawayEvent::Abandoned
        } else {
            RunawayEvent::Speed {
                mph: self.trip.truck.speed_mph(),
            }
        };
        match self.runaway.next(event) {
            Some(RunawayTransition::Began) => {
                ctx.audio.play("ui/warning");
                ctx.say_event_with(
                    "Runaway! The truck is taking damage.",
                    SayEvent::new().category(SpeechCategory::Safety),
                );
            }
            Some(RunawayTransition::Ended) => {
                // Queued, not cutting in: a cut "Runaway!" would be offered
                // a rescue and come back after this line, untrue.
                let damage = self.trip.truck.damage_pct;
                ctx.say_event_with(
                    format!("Back under control. Damage {damage:.0} percent."),
                    SayEvent::queued().category(SpeechCategory::Safety),
                );
            }
            None => {}
        }
    }
}
