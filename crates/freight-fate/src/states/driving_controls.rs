//! The discrete control surface at the wheel (port of
//! `freight_fate/states/driving_controls.py`, the `DrivingControlsMixin`).
//!
//! Continuous controls -- throttle, brake, clutch, steering -- are polled from
//! held keys and the pad's axes by `driving_updates`. Everything here is a
//! DISCRETE press: a key or a pad button the player deliberately hit, answered
//! immediately. Every answer is spoken inside `ctx.player_asked(...)`, so a
//! readout somebody asked for may cut the line in progress even though
//! unasked-for lines queue at the wheel (see `GameContext::player_asked`).
//!
//! Submodules follow the Python file's own sections:
//!
//! * [`keys`] -- `handle_event` / `_handle_key`, the whole keyboard table,
//!   and the assist-off tap lane change.
//! * [`pad`] -- `handle_controller`, the modified (right-bumper) layer, and
//!   the disconnect that pauses the drive.
//! * [`vehicle`] -- the controls that move metal: the engine, the parking
//!   brake, the jake stalk and its stages, and the manual gearbox.
//! * [`info`] -- the road and speed keys (Space, S, D, A, G, U, F, V, I).
//! * [`cab`] -- the driver window, the fan and heat or air conditioning.
//! * [`clock`] -- C and the three Alt hours keys, plus the shoulder-sleep
//!   question the rest key and the pause menu both ask.
//! * [`status`] -- the Tab status screen's lines, the gear name and the
//!   air-brake sentence.
//! * [`touch`] -- the iPhone and iPad driving gestures, each running the
//!   command its touch binding names.
//! * [`help`] -- F1: the keyboard layout, or the pad's, following the device.
//! * [`latches`] -- `_update_pedal_latches`, the brake latch only, called
//!   once per frame by `driving_updates` with the raw pedal inputs.
//! * [`pending`] -- TEMPORARY stubs for everything this file calls on a mixin
//!   that has not been ported yet, exactly as `driving_events::pending` works.
//!   `driving_speed_control` shares them.

pub mod cab;
pub mod clock;
pub mod help;
pub mod hos_stops;
pub mod info;
pub mod keys;
pub mod latches;
pub mod pad;
pub mod pending;
pub mod status;
pub mod touch;
pub mod vehicle;

/// Wear meters join the status readout once they're worth planning around.
pub const WEAR_STATUS_PCT: f64 = 50.0;

/// D looks this far ahead for a bend: about the pacenote call distance, so
/// the one number never contradicts the call you just heard.
pub const SAFE_SPEED_CURVE_MI: f64 = 0.5;

/// The most clauses the U readout may ever speak: the ramp control ahead,
/// the next imposed limit, the next stop, and the next demanding bend.
pub const UPCOMING_MAX_CLAUSES: usize = 4;
