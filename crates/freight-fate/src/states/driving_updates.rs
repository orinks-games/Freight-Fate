//! The per-frame heart of the drive (port of
//! `freight_fate/states/driving_updates.py`, the `DrivingUpdateMixin`).
//!
//! One `impl DrivingState` block per section of the Python file:
//!
//! * [`frame`] -- `update()` itself, the frame loop every other block hangs
//!   off, plus the safety-call re-speak and the retarder transcript trace.
//! * [`air`] -- held inputs turned into pedals: the air-brake lockout, the
//!   air/spring-brake announcements, the reverse gesture, the over-rev
//!   warning, the horn's protection valve, and the idle settle a menu needs.
//! * [`fatigue`] -- the HOS shift clock, fatigue accrual, and microsleeps.
//! * [`lanes`] -- discrete lanes: the drift model's frame, tap changes,
//!   crossings, a road that narrows under the truck, coned-off lanes and
//!   keep-right pressure.
//! * [`cues`] -- what the lane sounds like: the edge ladder, off-pavement
//!   transitions, the curve run's verdict, the transverse strips, the
//!   locator and steering tocks, and the guidance director.
//! * [`engine_audio`] -- the engine bed, the shift duck, road joints, the
//!   jake growl, the air-fill loop, and automatic retarder management.
//! * [`radio`] -- reception, the FM fringe, station rotation and personal
//!   playlists, the dial keys, and the live-data source syncs.
//! * [`radio_synth`] -- the Roadhouse in Synthesized mode.
//! * [`hazards`] -- the braking budget, automatic emergency braking, hazard
//!   resolution, and the grade advisory.
//! * [`enforcement`] -- the dash overspeed alert, the pull-over and its
//!   compliance tracker, weigh stations, and the run-from-the-stop opt-in.
//! * [`pacing`] -- a mid-drive pace change, the merge handoff's bound, and
//!   the session-log trace of the clock's real-time pins.
//! * [`conditions`] -- hot brakes, the destination approach assist, traction
//!   states and the chain law.
//!
//! [`pending`] is this task's TEMPORARY stub surface: every method
//! `driving_updates.py` called on a mixin that has not been ported yet. Each
//! block names the module that owns it; that module's task deletes its block
//! when it lands.

pub mod air;
pub mod cab;
pub mod conditions;
pub mod cues;
pub mod curve_servo;
pub mod enforcement;
pub mod engine_audio;
pub mod fatigue;
pub mod frame;
pub mod hazards;
pub mod lanes;
pub mod live_sources;
pub mod pacing;
pub mod pending;
pub mod radio;
pub mod radio_channel3000;
pub mod radio_synth;
pub mod stops;

use std::cell::Cell;

/// `LIMIT_DROP_SPEECH_LATENCY_S`.
///
/// The zone-entry line rides ROUTE now (queued, willing to wait this long
/// behind other speech before flushing). Until the line has had that long to
/// actually be spoken, holding the accelerator is not yet disregard -- nobody
/// has told the driver anything, and speech latency must never masquerade as
/// defiance (the research doc's coupled invariant on the R1 demotion).
pub fn limit_drop_speech_latency_s() -> f64 {
    crate::app::FF_PACER.important_wait_s
}

/// Spoken word for a lane count, used by the road-narrows call
/// (`leave_a_lane_the_road_closed`): "one lane" reads better than "1 lane(s)".
pub fn lane_count_words(count: i64) -> String {
    match count {
        1 => "one lane".to_string(),
        2 => "two lanes".to_string(),
        3 => "three lanes".to_string(),
        4 => "four lanes".to_string(),
        other => format!("{other} lanes"),
    }
}

// Re-crossings inside this window are pinballing, not lane changes.
pub const LANE_CROSS_REPEAT_S: f64 = 4.0;
// One brush against a vehicle alongside is one contact, however many times
// the tires cross the line while it is happening.
pub const SIDESWIPE_REPEAT_S: f64 = 3.0;

// FM fringe rendering. The bed creeps in below full quieting
// (radio.SIGNAL_FULL_VOLUME) and deepens quadratically; pickets begin below
// the static threshold (radio.STATIC_SIGNAL_THRESHOLD). Both are references
// to the radio module's own constants, not copies -- a hardcoded copy of
// each drifted silently out of sync with radio.py's smear ruling once
// already (2026-08-13), so this system reads the numbers it must agree
// with instead of remembering them. PICKET_DUCK is the program level while
// a splash owns the channel -- capture lost, near-silent, restored sharply.
pub const FRINGE_BED_SIGNAL: f64 = ff_core::radio::SIGNAL_FULL_VOLUME;
// Peak bed level ~= where the program used to sit, never a wall of noise on
// top of it: the owner's smear ruling -- static takes the program's place.
pub const FRINGE_BED_MAX_VOLUME: f64 = 0.35;
pub const PICKET_SIGNAL: f64 = ff_core::radio::STATIC_SIGNAL_THRESHOLD;
pub const PICKET_DUCK: f64 = 0.12;

// Shift plus the dial keys steps radio_volume by this much -- the same
// 10-percent grid the Settings > Audio "In-cab radio volume" row uses
// (main_menu.py's _volume helper), so the wheel and the menu can never
// disagree about a reachable value.
pub const RADIO_VOLUME_STEP: f64 = 0.1;

// How far down the left trigger has to be before it counts as the emergency
// application rather than a hard service stop. The controller help, the input
// hints and the manual have all promised "press the left trigger fully for
// the hardest stop" since the pad shipped, and nothing implemented it: the
// emergency flag was read from the B key alone, so a pad driver got a full
// service application and none of what the emergency one carries (owner,
// 2026-08-16). Set high on purpose -- this is the pedal you stand on when
// something is about to happen, and it must not fire on a firm normal stop.
pub const PAD_EMERGENCY_BRAKE: f64 = 0.97;
// Flutter rate bounds: parked multipath barely moves (slow wander floor);
// the ceiling is perceptual -- past ~9 events a second it just reads as
// noise, and the one-shot mixer would thrash.
pub const PICKET_MIN_RATE_HZ: f64 = 0.4;
pub const PICKET_MAX_RATE_HZ: f64 = 9.0;
pub const FM_DEFAULT_MHZ: f64 = 98.0; // mid-band; wavelength varies ~10 percent over 88-108

// Personal playlist pacing. A file starts playing the moment play_music_file
// returns, so it only needs long enough for the fade-in not to read as a
// finished track. A stream entry connects on a worker thread and is silent
// until it lands, so it gets the same order of grace the curated real streams
// get before a re-tune (radio_reconnect_timer), and two attempts before the
// entry is written off and the playlist moves on.
pub const PLAYLIST_FADE_HOLD_S: f64 = 1.5;
pub const PLAYLIST_CONNECT_HOLD_S: f64 = 9.0;
pub const PLAYLIST_CONNECT_TRIES: u32 = 2;
pub const PLAYLIST_RETRY_S: f64 = 30.0; // how often a playlist with nothing playable looks again

// Sustained redline quietly grinds the engine down (Truck._update_temps), so
// the player must hear about it while it is happening, not at the end screen.
// The grace period lets a shift's momentary flare pass unremarked.
// (OVERREV_GRACE_S is needed at construction time, so it lives in the
// driving prelude's constants and is re-exported here.)
pub use crate::states::driving_core::OVERREV_GRACE_S;
pub const OVERREV_REPEAT_S: f64 = 10.0;

// An automatic shift caps audible engine load so the bed doesn't duck out.
pub const SHIFT_LOAD_CAP: f64 = 0.45;
// ...but the load floor (0.68) keeps a capped engine at 82 percent of full
// level -- the "undertone at the last rpm" the owner heard through every
// shift. The disengage duck drops the whole bed below that floor through
// the torque interrupt, then rides the same recovery curve back up: the
// engine genuinely falls away and returns, like a clutch actually opening.
// How far it falls is bounded by the game's own model: a torque interrupt
// is an UNLOADED engine, so the gap sits a little under the off-throttle
// bed (cap x duck = 0.54 of full, 2 dB under coasting), never a cliff
// below it. The 0.35 it shipped with put the gap 7.5 dB under coasting and
// 11 dB under full load -- more than the whole full-to-idle span -- and a
// one-second downshift at that level was "the engine cuts out when it
// shifts" (testers, 2026-09-03).
pub const SHIFT_DISENGAGE_DUCK: f64 = 0.65;
// The gear taking at the end of an auto shift: a soft pick from the shift
// bank, quieter than the interrupt clunk (0.65) that opened the shift.
pub const SHIFT_END_CLUNK_VOLUME: f64 = 0.4;
// When the shift completes the cap eases from SHIFT_LOAD_CAP back to full over
// this window. The curve (a key into audio_fades.CURVES) shapes the return: an
// ease-out leaves the shift level quickly -- so the engine doesn't sit soft --
// while still arriving at full load gently instead of snapping. A plain "linear"
// ramp had to be stretched long to hide the snap, which sounded too soft.
pub const SHIFT_LOAD_RECOVERY_S: f64 = 0.032;
pub const SHIFT_LOAD_RECOVERY_CURVE: &str = "ease_out";

/// `_shift_recovery_curve`: resolved once, as the Python module resolved it
/// at import time.
pub fn shift_recovery_curve(t: f64) -> f64 {
    static CURVE: std::sync::OnceLock<ff_core::audio_fades::CurveFn> = std::sync::OnceLock::new();
    (CURVE.get_or_init(|| ff_core::audio_fades::curve(SHIFT_LOAD_RECOVERY_CURVE)))(t)
}

// Low-pass raw throttle before it reaches the audible engine-load envelope.
pub const ENGINE_LOAD_SMOOTH_S: f64 = 0.45;

// The jake's voice: synthesized growl loops at fixed rpm points, picked by
// nearest engine speed. Retarding power goes as cylinders x rpm, so the
// level grows with both the selected stage and the revs; the loop cuts out
// through shifts and clutch (the stair-stepping signature: buzz, gap,
// resume higher -- jake_v3.py's design notes, owner-approved 2026-07-18).
pub const JAKE_LOOP_RPMS: [i64; 6] = [1200, 1400, 1600, 1800, 2000, 2200];
// Two, four, six cylinders. Stage one stays modest twice over: the owner
// heard 0.45 as still too loud (2026-07-22), and no one has a verified
// recording of a real low stage -- do not dramatize what we cannot confirm.
pub const JAKE_STAGE_GAIN: [f64; 3] = [0.25, 0.65, 1.0];
pub const JAKE_MIN_RPM: f64 = 950.0;
// How long the growl's loop is kept, silent, through a gap before it is
// stopped. The gap is still heard (shift, clutch, a throttle blip), but the
// return is the same loop coming back up rather than the loop restarted from
// its top: a controller trading throttle and jake near a no-engine-brake zone
// restarted it dozens of times in a row (agent drive, 2026-09-24). Assumed:
// longer than a shift or a cruise throttle pulse, short enough that a jake
// switched off is simply off.
pub const JAKE_CUE_HOLD_S: f64 = 2.0;
// Both jake voices are 1600 rpm cuts (the one real recording and the one
// synth kept beside it), and one voice stands for every band, so the growl
// used to sit on one note however the revs moved. It is re-pitched from
// this native speed every frame instead: a downshift on a grade now climbs
// with the revs and a gear run down to the shift point falls with them,
// which is what a manual driver listens for. Clamped so 950 rpm and redline
// stay inside what a re-pitched recording can carry without sounding like
// tape.
pub const JAKE_VOICE_NATIVE_RPM: f64 = 1600.0;
pub const JAKE_RATE_MIN: f64 = 0.55;
pub const JAKE_RATE_MAX: f64 = 1.4;

// How far over a curve's advisory speed the truck has to be before the curve
// assist reaches for the retarder. The engine brake is for shedding real
// speed; a bend the truck is a few mph over is a lift and a touch of the
// drums, which do it quietly and are legal in every town.
//
// This threshold no longer gates the curve assist at all: a corner never
// raises the retarder, whatever the overspeed (owner ruling 2026-08-11). It
// survives as the line the service trim below draws for "well over the
// advisory", which is what it always measured.
// How hard the retarder works once a GRADE has called for it: past this much
// over the advisory it gets everything, otherwise the working setting, stage
// two. Reached only on a downgrade -- see `update_lane`.
pub const CURVE_ASSIST_JAKE_FULL_MPH: f64 = 15.0;

// Auto jake (automatic box, owner design 2026-07-22): J arms retarder
// management the way a real AMT integrates it. The controller holds the
// engagement speed (or the descent-control target) by stepping the stage,
// rate-limited so the growl steps audibly like an ECU thinking, and never
// selects a stage whose retard the drive axle cannot hold.
pub const AUTO_JAKE_STEP_S: f64 = 1.5; // seconds between stage steps
pub const AUTO_JAKE_OVER_MPH: f64 = 1.0; // this far above target: step up
pub const AUTO_JAKE_UNDER_MPH: f64 = 3.0; // this far below target: step down
                                          // Still this far over the number and the stage stands, so the release does
                                          // not chase the raise threshold a quarter of a mile per hour away. The same
                                          // hysteresis the raise line needs, sized to this controller's own raise
                                          // line.
pub const AUTO_JAKE_RELEASE_MPH: f64 = 0.25;
// A step back the other way waits this long -- adaptive cruise's own
// reversal time (`CRUISE_JAKE_REVERSE_S`), for the same reason.
pub const AUTO_JAKE_REVERSE_S: f64 = crate::states::driving_core::CRUISE_JAKE_REVERSE_S;

// The air-fill loop re-arms only this far below governor release. air_ready
// flips at exactly 100 psi and normal service braking dips the reservoirs a
// few psi, so without hysteresis the fill hiss would flutter on and off every
// few seconds all drive long. A cold start (55) or a real low-air situation
// still brings it in; once playing it runs until the air is ready again.
pub const AIR_FILL_REARM_PSI: f64 = 8.0;
// A bed under the idle, not a foreground event (owner's ear, 2026-07-22).
pub const AIR_FILL_VOLUME: f64 = 0.6;

// The asset is baked at Darren's -16 dBFS RMS, which is 2.6 dB over the
// engine. That is the level that fixed being inaudible, and it is louder
// than a cue needs to be once it is the only thing carrying the pan, so
// the channel takes it back down and the loudness setting scales from
// there like every other lane cue.
pub const LANE_GUIDE_TONE_VOLUME: f64 = 0.35;

/// The live half of a `say_event(valid=...)` gate.
///
/// Python's `valid` lambdas closed over `self` and read the drive at the
/// moment the pacer offered a cut line its rescue. A Rust `Valid` is
/// `Box<dyn Fn() -> bool>` with no arguments and a `'static` bound, so it
/// cannot borrow the drive -- and there is no way to reach the state stack
/// from inside one. This is that reading, mirrored into thread-local cells
/// the frame loop refreshes every tick (and every emit site refreshes on the
/// spot), so the gate answers about the truck as it is rather than as it was
/// when the line was written. One drive is live per thread: the game runs
/// one, and a test builds its own on its own thread.
pub mod live {
    use super::Cell;

    thread_local! {
        static OVERSPEED_ACTIVE: Cell<bool> = const { Cell::new(false) };
        static PULL_OVER_ACTIVE: Cell<bool> = const { Cell::new(false) };
        static DAMAGE_PCT: Cell<f64> = const { Cell::new(0.0) };
        static POSITION_MI: Cell<f64> = const { Cell::new(0.0) };
        static SPEED_MPH: Cell<f64> = const { Cell::new(0.0) };
        static HAZARD_ACTIVE: Cell<bool> = const { Cell::new(false) };
        static ARRIVAL_MENU_OPEN: Cell<bool> = const { Cell::new(false) };
        static GATE_STOP_PROMPTED: Cell<bool> = const { Cell::new(false) };
        static ON_RAMP: Cell<bool> = const { Cell::new(false) };
        static POSTED_LIMIT_MPH: Cell<f64> = const { Cell::new(0.0) };
        static RAMP_HOLDING: Cell<bool> = const { Cell::new(false) };
        static MOVE_LOCKOUT: Cell<u8> = const { Cell::new(0) };
    }

    /// What is stopping the truck moving under its own throttle, if
    /// anything: 0 nothing, then [`LOCKOUT_ENGINE_OFF`], [`LOCKOUT_AIR`],
    /// [`LOCKOUT_PARKING_BRAKE`]. The lockout's queued line speaks only
    /// while its own reason still holds: queued behind other speech, "Parking
    /// brake set. Press P to release it." spoke after the brake was off and
    /// the truck rolling, and pressing P there slams the spring brakes on
    /// (agent drive out of Aberdeen Company Yard, 2026-09-30).
    pub fn set_move_lockout(value: u8) {
        MOVE_LOCKOUT.with(|cell| cell.set(value));
    }

    pub fn move_lockout() -> u8 {
        MOVE_LOCKOUT.with(|cell| cell.get())
    }

    pub const LOCKOUT_ENGINE_OFF: u8 = 1;
    pub const LOCKOUT_AIR: u8 = 2;
    pub const LOCKOUT_PARKING_BRAKE: u8 = 3;

    /// Whether the truck has left the mainline for a ramp. A mainline line
    /// ("Speed limit raised to 75") cut by the take line is about a road
    /// the truck is no longer on, so it is not worth handing back.
    pub fn set_on_ramp(value: bool) {
        ON_RAMP.with(|cell| cell.set(value));
    }

    pub fn on_ramp() -> bool {
        ON_RAMP.with(|cell| cell.get())
    }

    /// The posted limit governing the truck, in mph: the zone's while one is
    /// entered, else the corridor's. "Speed limit reduced to 45" cut by the
    /// next line and handed back after "raised to 55" names a limit the
    /// truck has already left.
    pub fn set_posted_limit_mph(value: f64) {
        POSTED_LIMIT_MPH.with(|cell| cell.set(value));
    }

    pub fn posted_limit_mph() -> f64 {
        POSTED_LIMIT_MPH.with(|cell| cell.get())
    }

    /// Whether a mainline limit line is still worth speaking: the truck is
    /// not on a ramp, and the limit it names (when it names one) still
    /// governs the truck.
    pub fn limit_line_holds(named_mph: Option<f64>) -> bool {
        !on_ramp() && named_mph.is_none_or(|mph| (posted_limit_mph() - mph).abs() < 0.5)
    }

    /// Whether the truck is still held at a ramp terminal's bar, waiting for
    /// its gap or its green. "Assistance is holding for your gap" is true
    /// only while this is; once the gap comes it is superseded.
    pub fn set_ramp_holding(value: bool) {
        RAMP_HOLDING.with(|cell| cell.set(value));
    }

    pub fn ramp_holding() -> bool {
        RAMP_HOLDING.with(|cell| cell.get())
    }

    pub fn set_overspeed_active(value: bool) {
        OVERSPEED_ACTIVE.with(|cell| cell.set(value));
    }

    pub fn overspeed_active() -> bool {
        OVERSPEED_ACTIVE.with(|cell| cell.get())
    }

    pub fn set_pull_over_active(value: bool) {
        PULL_OVER_ACTIVE.with(|cell| cell.set(value));
    }

    pub fn pull_over_active() -> bool {
        PULL_OVER_ACTIVE.with(|cell| cell.get())
    }

    pub fn set_damage_pct(value: f64) {
        DAMAGE_PCT.with(|cell| cell.set(value));
    }

    pub fn damage_pct() -> f64 {
        DAMAGE_PCT.with(|cell| cell.get())
    }

    pub fn set_position_mi(value: f64) {
        POSITION_MI.with(|cell| cell.set(value));
    }

    pub fn position_mi() -> f64 {
        POSITION_MI.with(|cell| cell.get())
    }

    pub fn set_speed_mph(value: f64) {
        SPEED_MPH.with(|cell| cell.set(value));
    }

    pub fn speed_mph() -> f64 {
        SPEED_MPH.with(|cell| cell.get())
    }

    /// Whether a hazard is still live -- the mirror of `_hazard_deadline is
    /// not None`, which is what Python's hazard rescue gate reads.
    pub fn set_hazard_active(value: bool) {
        HAZARD_ACTIVE.with(|cell| cell.set(value));
    }

    pub fn hazard_active() -> bool {
        HAZARD_ACTIVE.with(|cell| cell.get())
    }

    /// Whether the dock menu is up. The drive stops ticking while it is, so
    /// this one is stamped where the flag itself moves, not only per frame.
    pub fn set_arrival_menu_open(value: bool) {
        ARRIVAL_MENU_OPEN.with(|cell| cell.set(value));
    }

    pub fn arrival_menu_open() -> bool {
        ARRIVAL_MENU_OPEN.with(|cell| cell.get())
    }

    /// Whether the gate's own "At <facility>. Stop..." line has been
    /// spoken -- the mirror of `arrival_full_stop_said`. The "ahead" line
    /// that precedes it is only worth rescuing while this is still false:
    /// handed back after the stop line it told a truck already at the gate
    /// to slow down for it (agent playtest, 2026-09-02, both gates).
    /// Stamped where the flag moves as well as per frame, because the
    /// rescue is offered in the same frame the stop line lands.
    pub fn set_gate_stop_prompted(value: bool) {
        GATE_STOP_PROMPTED.with(|cell| cell.set(value));
    }

    pub fn gate_stop_prompted() -> bool {
        GATE_STOP_PROMPTED.with(|cell| cell.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_counts_read_as_words_up_to_four() {
        assert_eq!(lane_count_words(1), "one lane");
        assert_eq!(lane_count_words(4), "four lanes");
        assert_eq!(lane_count_words(5), "5 lanes");
    }
}
