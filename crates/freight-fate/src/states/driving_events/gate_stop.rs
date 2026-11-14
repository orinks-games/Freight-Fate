//! The stop at a facility gate, as the driving state drives it: the one way
//! the gate's state machine (`ff_core::sim::arrival_gate`) moves, the
//! confirm key, and the line that names the parking brake.

use ff_core::sim::arrival_gate::{GateCue, GateEvent};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::speech::EventPriority;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_updates::live;

impl DrivingState {
    /// The only way `self.gate` moves. Stamps the live fact the "ahead"
    /// line's rescue gate reads, since the frame that would carry it across
    /// may never come (the dock menu takes over).
    pub(crate) fn step_gate(&mut self, event: GateEvent) -> Option<GateCue> {
        let (next, cue) = self.gate.next(event);
        self.gate = next;
        live::set_gate_stop_prompted(next.stop_prompted());
        cue
    }

    /// One frame's reading at the gate with the driver stopping the truck.
    pub(crate) fn read_gate(&mut self, dt: f64) -> Option<GateCue> {
        let truck = &self.trip.truck;
        let event = GateEvent::Reading {
            speed_mph: truck.speed_mph(),
            parking_brake: truck.parking_brake,
            dt_s: dt,
        };
        self.step_gate(event)
    }

    /// At rest with the parking brake off and no assist to set it: the one
    /// thing the gate is still waiting for is the brake (or the confirm key).
    pub fn at_rest_without_parking_brake(&self, ctx: &GameContext) -> bool {
        !ctx.settings.destination_approach_assist
            && self.trip.truck.speed_mph() <= DOCKING_MAX_MPH
            && !self.trip.truck.parking_brake
    }

    /// Whether Enter or controller A opens the facility now: at rest at the
    /// gate without the assist, or at the assist's hold with it.
    pub fn facility_confirmation_ready(&self, ctx: &GameContext) -> bool {
        if ctx.settings.destination_approach_assist {
            return self.assisted_facility_confirmation_ready(ctx);
        }
        self.trip.truck.speed_mph() <= DOCKING_MAX_MPH
            && self.arrival_gate_query_text(ctx).is_some()
    }

    /// Enter or controller A while the trip has ended at a facility gate.
    ///
    /// Without the assist, a truck at rest opens the dock or check-in brake
    /// or no brake, as the manual says; the opening sets the brake. With the
    /// assist, only its hold opens. A key that does nothing says nothing.
    pub fn confirm_facility_arrival(&mut self, ctx: &mut GameContext) {
        if self.arrival_gate_query_text(ctx).is_none() {
            return;
        }
        let truck = &self.trip.truck;
        let event = GateEvent::Confirm {
            speed_mph: truck.speed_mph(),
            parking_brake: truck.parking_brake,
            assist: ctx.settings.destination_approach_assist,
        };
        if self.step_gate(event) == Some(GateCue::Open) {
            self.open_ready_facility_arrival(ctx);
        }
    }

    /// Stopped at the gate with the brake off for the rest delay: say once
    /// what is missing, naming the control while it is still being taught.
    pub(crate) fn say_gate_brake_line(&mut self, ctx: &mut GameContext, pickup: bool) {
        let message = if self.instruction_retired(ctx, "parking_brake") {
            if pickup {
                "Set the parking brake to check in.".to_string()
            } else {
                "Set the parking brake to dock.".to_string()
            }
        } else {
            let key = ctx.control_hint("parking_brake");
            if pickup {
                format!("Set the parking brake to check in. Press {key}.")
            } else {
                format!("Set the parking brake to dock. Press {key}.")
            }
        };
        ctx.audio.play_with("ui/notify", 0.7, 0.0);
        // Same contract as the creep line, and only while the gate is still
        // waiting: handed back after Enter opened the dock, it would ask for
        // a brake the opening has already set.
        self.refresh_live_facts();
        ctx.say_event_with(
            message,
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Navigation)
                .valid(|| !live::arrival_menu_open()),
        );
    }
}
