//! The drive's short accessors onto its trip, the trip swap, and the `State`
//! facade that forwards to the mixin modules.

use ff_core::radio::RadioState;
use ff_core::sim::trip::Trip;
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::WeatherSystem;

use crate::app::GameContext;
use crate::discord_presence::PresenceState;
use crate::states::base::{InputEvent, State};
use crate::touch::Gesture;

use super::DrivingState;

impl DrivingState {
    /// `self.truck`: the truck rides on the trip.
    #[inline]
    pub fn truck(&self) -> &TruckState {
        &self.trip.truck
    }

    #[inline]
    pub fn truck_mut(&mut self) -> &mut TruckState {
        &mut self.trip.truck
    }

    /// `self.weather`: the weather system rides on the trip.
    #[inline]
    pub fn weather(&self) -> &WeatherSystem {
        &self.trip.weather
    }

    #[inline]
    pub fn weather_mut(&mut self) -> &mut WeatherSystem {
        &mut self.trip.weather
    }

    /// Replace the active trip (surface or departure chain) and bump the
    /// generation the turn latches compare against (`id(self.trip)`).
    pub fn replace_trip(&mut self, trip: Trip) -> Trip {
        self.trip_generation += 1;
        // The keeper's "keep aiming at the corner already being slowed for"
        // memory is a milepost on the OLD trip. Carried across the swap it
        // held the street chain's last corner -- 20 mph, to a mile the new
        // road reaches much later -- through the whole acceleration lane, so
        // the truck merged at 19 into 40 mph traffic (Brandon, Waco onto
        // TX-31, 2026-09-01: "cruise didn't build up to traffic
        // speed"). The corner latches reset on the same generation bump.
        self.keeper_ease_target = None;
        // The curve servo is the same kind of memory and needs the same
        // treatment: its start and hold mileposts belong to the road just
        // swapped out. A servo armed for a bend near a destination exit is
        // never past its hold point on a short street chain, so it stayed
        // armed for the whole approach -- braking the surface streets down to
        // a highway bend's advisory, and, since it now pins the clock and
        // holds a downgrade, doing both of those on a road it never saw
        // (review finding, 2026-09-19).
        self.curve_servo = None;
        self.trip.curve_shed_active = false;
        std::mem::replace(&mut self.trip, trip)
    }
}

// -- the State facade ---------------------------------------------------------------------
//
// Forwards to the mixin modules' methods:
//   driving_controls.rs : handle_key_event, handle_controller_event,
//                         handle_controller_disconnect
//   driving_updates.rs  : update_frame, tick_drive_music, apply_radio_settings_to_drive
//   driving_events.rs   : visible_lines, presence_state, online_presence_state

impl State for DrivingState {
    // At the wheel, main-channel lines (achievements, assist notices, info
    // replies) queue instead of cutting whatever is mid-air -- the event
    // channel's discipline, extended to the other voice (research doc, R2).
    fn paces_main_speech(&self) -> bool {
        true
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        self.enter_drive(ctx);
    }

    fn exit(&mut self, ctx: &mut GameContext) {
        self.exit_drive(ctx);
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        self.handle_key_event(ctx, event);
    }

    fn handle_gesture(&mut self, ctx: &mut GameContext, gesture: Gesture) -> bool {
        self.handle_touch_gesture(ctx, gesture)
    }

    fn handle_controller(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        self.handle_controller_event(ctx, event);
    }

    fn on_controller_disconnect(&mut self, ctx: &mut GameContext) {
        self.handle_controller_disconnect(ctx);
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        self.update_frame(ctx, dt);
    }

    fn lines(&self, ctx: &GameContext) -> Vec<String> {
        self.visible_lines(ctx)
    }

    fn presence(&self, ctx: &GameContext) -> Option<PresenceState> {
        self.presence_state(ctx)
    }

    fn online_presence(&self, ctx: &GameContext) -> Option<PresenceState> {
        self.online_presence_state(ctx)
    }

    fn ticks_covered_music(&self) -> bool {
        true
    }

    fn tick_covered_music(&mut self, ctx: &mut GameContext, dt: f64) {
        self.tick_drive_music(ctx, dt);
    }

    fn applies_radio_settings(&self) -> bool {
        true
    }

    fn apply_radio_settings_now(&mut self, ctx: &mut GameContext) {
        self.apply_radio_settings_to_drive(ctx);
    }

    fn radio(&self) -> Option<&RadioState> {
        Some(&self.radio)
    }
}
