//! Route events, exits and ramps, the destination approach, cruise and the
//! speed keeper's spoken half (port of
//! `freight_fate/states/driving_events.py`, the `DrivingEventMixin`).
//!
//! The Python mixin was one 5,500-line class body; here it is one
//! `impl DrivingState` block per section, split by what each section is
//! about:
//!
//! * [`ambient`] -- the ambient line queue (`_speak_ambient_event` and the
//!   drain that ages lines out rather than performing them late).
//! * [`trip_events`] -- `_handle_trip_event` and everything that decides
//!   whether a trip event speaks, in what category, at what priority.
//! * [`stops`] -- the rest key (`T`), the selected sleep stop, and opening a
//!   route point's menu.
//! * [`exits`] -- the exit signal (`X`), the exit lane, the countdown, and
//!   the exit speed assist; [`destination_exit`] is the delivery's own exit:
//!   the interchange scan, its name, and its announcement.
//! * [`chain_stop`] -- roadside snow-chain work, shared by the pause menu
//!   and facility stopping assistance.
//! * [`chains`] -- the facility street chains: surface (arrival) and
//!   departure, plus the acceleration lane that ends the departure one.
//! * [`ramp_terminal`] -- what meets you at the end of a ramp: the light,
//!   the sign, the cross-traffic bubble, the stop bar and its tone;
//!   [`ramp_crossing`] works the pedals for it and judges the crossing.
//!   [`street_controls`] plays a facility chain's lights and signs through
//!   the same two.
//! * [`update_exit`] -- the per-frame advance of an armed exit or an active
//!   ramp, and the destination terminal loop-back.
//! * [`cruise`] -- arming and adjusting cruise and the speed keeper;
//!   [`cruise_loop`] is the per-frame loop, its caps and its grade preview.
//! * [`arrival`] -- the gate, the missed destination exit, the dock menu,
//!   and the state's `lines()` / presence.
//!
//! [`pending`] is this task's TEMPORARY stub surface: every method and free
//! function `driving_events.py` called on a mixin that has not been ported
//! yet. Each block names the module that owns it; that module's task deletes
//! its block here when it lands its real implementation.

pub mod ambient;
pub mod arrival;
pub mod billboard_moment;
pub mod chain_stop;
pub mod chains;
pub mod cruise;
pub mod cruise_loop;
pub mod decel_lane;
pub mod descent_control;
pub mod destination_exit;
pub mod exits;
pub mod pending;
pub mod ramp_crossing;
pub mod ramp_terminal;
pub mod stop_plan;
pub mod stops;
pub mod street_controls;
pub mod trip_events;
pub mod update_exit;

use ff_core::sim::trip_models::TripEventKind;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::GameContext;
use crate::states::driving::DrivingState;

/// Reach the drive again from a callback that only gets the context, when
/// the drive is still ON the stack under the wait screen.
///
/// Python's `TimedMessageState(on_complete=...)` closures captured `self`;
/// a Rust `'static` callback cannot, so this finds the drive on the state
/// stack instead. The drive is never the state being dispatched when one of
/// these runs (the wait screen is), so the borrow always succeeds.
///
/// **Only for a wait screen that was PUSHED over the drive.** A wait screen
/// that `replace_state`d the drive has taken it off the stack, and this
/// search then finds nothing -- which is how the dock and check-in menus
/// came to never open. Those sites capture
/// [`crate::states::driving_menu_states::DriveRef::active`] before the
/// replace and use the handle, exactly as Python's closure kept `self`.
pub fn with_drive<R>(
    ctx: &mut GameContext,
    f: impl FnOnce(&mut DrivingState, &mut GameContext) -> R,
) -> Option<R> {
    for shared in ctx.states().into_iter().rev() {
        let Ok(mut state) = shared.try_borrow_mut() else {
            continue;
        };
        let Some(drive) = state.as_any_mut().downcast_mut::<DrivingState>() else {
            continue;
        };
        return Some(f(drive, ctx));
    }
    log::error!("driving_events: the drive was not on the stack for a deferred callback");
    None
}

/// Flavor kinds the driving speech ladder deliberately does not govern. They
/// answer to the chatter switches and the place-callouts ladder instead. Kept
/// as an explicit set rather than an absence, so the "is every kind
/// classified" test can tell "decided to leave alone" from "forgot".
pub const FLAVOR_EVENT_KINDS: [TripEventKind; 5] = [
    TripEventKind::Landmark,
    TripEventKind::Billboard,
    TripEventKind::CityReached,
    TripEventKind::StateCrossing,
    TripEventKind::TimezoneCrossing,
];

/// `_EVENT_CATEGORIES`: what each trip event kind is ABOUT, for the ladder.
pub fn event_category_for_kind(kind: TripEventKind) -> Option<SpeechCategory> {
    match kind {
        TripEventKind::Hazard => Some(SpeechCategory::Safety),
        TripEventKind::Inspection => Some(SpeechCategory::Safety),
        // Entering and leaving a zone is the road's state, and its content is
        // a limit the driver can ask S for at any moment -- the resolver's own
        // rule puts that in STATUS (owner, 2026-08-17: these have to go at
        // quiet). The act-now half of a work zone is its lane closure, which
        // is a HAZARD and stays SAFETY.
        TripEventKind::ZoneEnter => Some(SpeechCategory::Status),
        TripEventKind::ZoneExit => Some(SpeechCategory::Status),
        // A stop you have not reached yet: "Road Ranger, exit 292, one mile."
        // Worth a tone at urgent only rather than words -- a player who has
        // turned the road down that far knows how to pull in, and the arrival
        // itself (STOP_REACHED) still speaks.
        TripEventKind::StopAhead => Some(SpeechCategory::NavigationAdvisory),
        TripEventKind::StopReached => Some(SpeechCategory::Navigation),
        TripEventKind::Checkpoint => Some(SpeechCategory::Navigation),
        TripEventKind::GpsCue => Some(SpeechCategory::Navigation),
        TripEventKind::Arrived => Some(SpeechCategory::Navigation),
        TripEventKind::Curve => Some(SpeechCategory::NavigationAdvisory),
        TripEventKind::TollCharged => Some(SpeechCategory::Money),
        TripEventKind::WeatherChange => Some(SpeechCategory::Status),
        TripEventKind::Lane => Some(SpeechCategory::Status),
        _ => None,
    }
}

// How many ambient lines may wait for the road to go quiet, and how long a
// waiting one stays worth saying. Both exist because the queue below replaced
// a single slot: without a bound a long hazard would bank a recital and
// perform it once the road cleared, which is the failure the one slot was
// crudely preventing. Sized in REAL seconds, not game miles -- a line waits
// in the player's ear, not on the road -- and generously, because the lines
// that were being lost (a state line, a lane count) describe where the truck
// IS rather than something coming up, and stay true while they wait.
pub const AMBIENT_QUEUE_MAX: usize = 4;
pub const AMBIENT_QUEUE_MAX_AGE_S: f64 = 12.0;
