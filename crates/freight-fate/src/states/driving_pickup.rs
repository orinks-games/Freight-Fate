//! Pickup-arrival flow for the deadhead driving phase (port of
//! `freight_fate/states/driving_pickup.py`, the `DrivingPickupMixin`).

use serde_json::Value;

use ff_core::speech_pacing::SpeechCategory;

use crate::speech::EventPriority;

use crate::app::{GameContext, SayEvent};
use crate::states::base::TimedMessageState;
use crate::states::city_pickup::{
    pickup_snapshot, PickupFacilityState, PickupOptions, PickupSnapshotOptions,
};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_menu_states::DriveRef;
use crate::states::driving_updates::live;

impl DrivingState {
    /// `_handle_pickup_gate()`.
    pub fn handle_pickup_gate(&mut self, ctx: &mut GameContext) {
        // The assist stops the truck here, exactly as it does at a dock, and
        // holds for the driver's key; see `hold_at_facility_entrance`.
        if self.hold_at_facility_entrance(ctx) {
            return;
        }
        if self.trip.truck.speed_mph() <= DOCKING_MAX_MPH && self.trip.truck.parking_brake {
            self.open_pickup_arrival(ctx);
            return;
        }
        if self.trip.truck.speed_mph() <= DELIVERY_PARK_MPH {
            self.handle_pickup_creep(ctx);
            return;
        }
        if self.arrival_stop_said {
            let facility = self.pickup_facility_text(ctx);
            self.remind_arrival_gate(
                ctx,
                "Pickup gate: stop to check in.",
                &format!("Still at {facility}. Stop to check in."),
                true,
            );
            return;
        }
        self.arrival_stop_said = true;
        self.gate_reminder_s = GATE_REMINDER_INTERVAL_S;
        let speed_control_paused = self.pause_speed_control(ctx, false);
        ctx.audio.play("ui/warning");
        self.set_status("Pickup ahead: stop at the gate.");
        let facility = self.pickup_facility_text(ctx);
        let message = if self.terse_speech(ctx) {
            format!("Pickup ahead: {facility}.")
        } else {
            format!("Pickup ahead: {facility}. Stop at the gate.")
        };
        // Rescued only until the gate's own stop line has landed: cut by
        // "At <facility>. Stop completely...", it came back behind it and
        // told a truck already at the gate to slow down for the gate
        // (agent playtest, 2026-09-02). Same family as the scale exit and
        // the dock hold prompt.
        ctx.say_event_with(
            message,
            SayEvent::new()
                .valid(|| !live::gate_stop_prompted())
                .category(SpeechCategory::Navigation),
        );
        if speed_control_paused {
            self.announce_pickup_speed_control_pause(ctx);
        }
    }

    /// `_handle_pickup_creep()`.
    pub fn handle_pickup_creep(&mut self, ctx: &mut GameContext) {
        if self.arrival_full_stop_said {
            return;
        }
        self.arrival_full_stop_said = true;
        live::set_gate_stop_prompted(true);
        let speed_control_paused = self.pause_speed_control(ctx, false);
        ctx.audio.play_with("ui/notify", 0.7, 0.0);
        self.set_status("Pickup gate: stop to check in.");
        let facility = self.pickup_facility_text(ctx);
        let message = if ctx.settings.destination_approach_assist {
            format!("At {facility}. Stop to check in.")
        } else {
            format!("At {facility}. Stop and set the parking brake.")
        };
        ctx.say_event_with(
            message,
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Navigation),
        );
        if speed_control_paused {
            self.announce_pickup_speed_control_pause(ctx);
        }
    }

    /// `_announce_pickup_speed_control_pause()`.
    pub fn announce_pickup_speed_control_pause(&mut self, ctx: &mut GameContext) {
        // ROUTE, not the ambient default: the automation just released the
        // throttle (automation-handoff sweep, 2026-08-20, the deferred
        // 2026-08-15 audit).
        ctx.say_event_with(
            "Automatic speed control paused for pickup.",
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Confirmation),
        );
    }

    /// `_open_pickup_arrival()`.
    pub fn open_pickup_arrival(&mut self, ctx: &mut GameContext) {
        if self.arrival_menu_open {
            return;
        }
        self.arrival_menu_open = true;
        // Same as the dock gate: the frame loop stops here, so the flag the
        // validity gates read has to be stamped where it moves.
        live::set_arrival_menu_open(true);
        let speed_control_paused = self.pause_speed_control(ctx, false);
        self.trip.truck.brake = 1.0;
        self.trip.truck.set_parking_brake();
        // A pickup gate is a menu-driven stop like a roadside inspection: the
        // frame loop that eases revs down between frames stops the instant
        // the check-in menu takes over, so without this the engine audio
        // froze at whatever rev the approach left it at, all the way through
        // the stop.
        self.settle_engine_to_idle(ctx);
        let air_brake = self.trip.truck.air_brake_snapshot();
        let engine_on = self.trip.truck.engine_on;
        let snapshot = pickup_snapshot(
            &self.job,
            &PickupSnapshotOptions {
                air_brake: Some(air_brake),
                engine_on,
                speed_control_armed: self.speed_control_armed,
                speed_control_target_mph: self.speed_control_target_mph,
                ..Default::default()
            },
        );
        let trip_minutes = self.trip.game_minutes;
        {
            let p = profile_mut_of(ctx);
            // A relayed load's deadhead ended in the shipper's city: the
            // truck is parked there now, and the board, the logbook and the
            // next dispatch all read the city off the profile.
            if p.current_city != self.job.origin {
                p.current_city = self.job.origin.clone();
            }
            // Store the whole record, not just fuel and damage: this line also
            // accrues brake and engine wear, which the flat names do not carry.
            p.store_truck_condition(&self.trip.truck);
            // Rolling to the check-in lane takes time and is on-duty time.
            p.game_hours += (trip_minutes + STOP_PULL_IN_MIN) / 60.0;
            p.hos.on_duty(STOP_PULL_IN_MIN);
            let day = p.market_day();
            p.market.advance_to(day);
            p.active_trip = Some(Value::Object(snapshot));
        }
        ctx.save_profile();
        if speed_control_paused {
            self.announce_pickup_speed_control_pause(ctx);
        }
        self.set_status("Pulling into pickup. Check-in menu opening.");

        // The pull-in is its own short spoken beat rather than a jump straight
        // to the check-in menu, so the arrival is something the driver hears
        // happen instead of a menu appearing under them.
        let facility = self.pickup_facility_text(ctx);
        // The drive's own handle, taken while it is still the active state:
        // the replace below takes it OFF the stack, so a callback that looks
        // it up there later finds nothing and the check-in menu never opens.
        // Python's closure kept `self` alive across the same replace.
        let drive = DriveRef::active(ctx);
        ctx.replace_state(
            TimedMessageState::new(
                "Pulling into pickup",
                &format!("Pulling into {facility}."),
                "Pulling into the pickup.",
                STOP_PULL_IN_WAIT_S,
                move |ctx: &mut GameContext| {
                    drive.with(ctx, |drive, ctx| {
                        drive.set_status("Parked at pickup. Check in and load.");
                        // `driving=self`: the arriving drive hands over the
                        // truck it was driving, plus its speed control session.
                        let job = drive.job.clone();
                        let opts = PickupOptions {
                            truck: Some(drive.trip.truck.clone()),
                            speed_control_armed: drive.speed_control_armed,
                            speed_control_target_mph: drive.speed_control_target_mph,
                            ..Default::default()
                        };
                        let state = PickupFacilityState::new(ctx, job, opts);
                        ctx.replace_state(state);
                    });
                },
            )
            .sound_key(Some("ui/notify")),
        );
    }

    /// `_pickup_facility_text()`.
    pub fn pickup_facility_text(&self, _ctx: &GameContext) -> String {
        self.job.origin_facility_text()
    }

    /// `_pickup_progress_summary()`.
    pub fn pickup_progress_summary(&self, ctx: &GameContext) -> String {
        // Spoken distances go through the unit setting; a player on metric must
        // not hear miles here just because this handler moved modules.
        let s = &ctx.settings;
        format!(
            "{} remaining of {} to pickup at {}.",
            s.gap_text(self.trip.remaining_miles()),
            s.distance_value(self.trip.total_miles(), 1, false),
            self.pickup_facility_text(ctx)
        )
    }
}
