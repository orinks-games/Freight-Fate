//! The enforcement watch: hearing the police, and being seen by them (port of
//! `freight_fate/states/driving_enforcement.py`, the `EnforcementWatchMixin`).
//!
//! This is the game-layer half of the presence model. `sim/enforcement_posts`
//! decides where the posts are and who is sitting in them;
//! `sim/enforcement_observe` decides what one of them notices and how sure it
//! is. This module assembles what the truck is doing into a sample, plays the
//! cues, makes the named seeded draw, and hands a confirmed observation to the
//! pull-over machinery that already exists.
//!
//! Three rules shape everything here.
//!
//! **Audible before it can bite.** A staffed post cannot observe a driver who
//! was never told it was there. That is not balance, it is the whole
//! accessibility contract: a blind player cannot see a cruiser on a crossover,
//! so if the game never made a sound about it, the ticket it writes is
//! arbitrary. Every staffed post emits its marker earcon before it enters
//! observing range, and `observe` refuses to look at a driver who has not
//! heard it. An UNSTAFFED post makes no sound at all: a cue you cannot tell
//! from a staffed one, for a car nobody is sitting in, is what taught players
//! the police do not enforce.
//!
//! **Speech is rationed; earcons are not.** Two spoken enforcement lines for a
//! whole run, spent on the things that cost money -- an open scale, and
//! anything that has already taken something from you. The marked-unit pass is
//! never spoken: it is a fact about the world with no action attached, and
//! narrating it twelve times a run is pure noise. A closed scale is never
//! spoken either; the ambience swells, nothing is said, and the absence of
//! speech is what says "closed".
//!
//! **One demand at a time.** Nothing here fires while a hazard deadline is
//! running, during a microsleep, on a ramp, inside an arrival gate sequence, or
//! during a stop already in progress. Deferred, never dropped: the post keeps
//! watching, and the moment the cab is quiet again it gets its look.
//!
//! Submodules: [`cues`] (the earcons, the tableau, the scale bed), [`scales`]
//! (weigh stations, their guidance and their screening), [`watch`] (the siren
//! hold, the road sample, and the per-frame draw).

mod cues;
mod inspections;
mod scales;
mod watch;

pub use inspections::roadside_inspection_scale_for;

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

// -- cue tuning --------------------------------------------------------------

/// The marker earcon fires this far before a post starts watching, so the cue
/// and the observation can never land in the same instant.
pub const POST_MARKER_LEAD_MI: f64 = 0.25;

/// How far a held look can travel before the officer has simply lost you. A
/// trooper who clocks a truck pulls out and catches it; one who never did is
/// not entitled to the stop half a state later. Generous on purpose: the thing
/// doing the deferring is usually a hazard, and a hazard window now runs long
/// enough to be answerable, which at full compression is several miles of road.
/// Five was not enough -- it stranded two looks out of three in a bench drive.
pub const DEFERRED_STOP_MAX_MI: f64 = 10.0;

// A marked unit going the other way is the most common police sound on a real
// road, and it now is here too. Pan is confirmation only -- the gesture in the
// asset is what says "oncoming pass" -- and the level falls off with distance
// from the post so a distant unit reads as distant.
pub const PASS_PAN: f64 = 0.55;
pub const PASS_BASE_VOLUME: f64 = 0.7;

// The weigh-station bed. It starts early and quiet and swells as the scale
// closes, which is a distance cue that works at any pacing: a loop rising over
// real seconds conveys closing speed correctly however compressed the clock is.
pub const SCALE_BED_START_MI: f64 = 2.2;
pub const SCALE_BED_MIN_VOLUME: f64 = 0.10;
pub const SCALE_BED_OPEN_MAX_VOLUME: f64 = 0.62;
pub const SCALE_BED_CLOSED_MAX_VOLUME: f64 = 0.26;
pub const SCALE_BED_FADE_MS: u32 = 700;

// The radio duck for a cue. A sibling of the picket duck, never the picket
// duck itself -- that one self-heals on _stop_radio_fringe and would drag the
// enforcement duck away with it.
pub const RADIO_CUE_DUCK: f64 = 0.22;
pub const RADIO_CUE_DUCK_S: f64 = 1.1;

/// How far past a post the truck has to be before its pass earcon has fired.
pub const PASS_TRIGGER_MI: f64 = 0.05;

// The tableau: a staffed patrol post that already has somebody stopped. Both
// cues scale with the road's own presence exactly the way the ordinary
// marked-unit pass does (`play_marked_unit_pass`) -- that scaling is loudness,
// never whether a cue plays at all. The mechanical truth is the catch
// suppression in `EnforcementPost::tableau_busy_at`, which neither cue's
// volume touches.
pub const TABLEAU_SHOULDER_PAN: f64 = 0.85; // hard right: US traffic keeps the shoulder there
pub const TABLEAU_SIREN_VOLUME: f64 = 0.7;
pub const TABLEAU_PASS_VOLUME: f64 = 0.7;

// The tableau's own introduction. Testers mistook the siren-and-pass cues
// for their own stop, so this reliably says whose stop it is -- every
// tableau, never a chance draw the way the CB flavor line is. Only the
// reason for the stop is a seeded pinch of colour, landing on some
// occurrences and not others; terse mode keeps the bare fact either way.
pub const TABLEAU_INTRO_LINE: &str = "A trooper has somebody stopped on the shoulder, not you.";
/// The line for a stopped pair within [`TABLEAU_REPEAT_WINDOW_MI`] of one
/// already passed.
pub const TABLEAU_ANOTHER_LINE: &str = "Another trooper has somebody stopped, not you.";
/// How far back a passed stopped pair makes the next one "another".
pub const TABLEAU_REPEAT_WINDOW_MI: f64 = 20.0;
pub const TABLEAU_INTRO_REASONS: [&str; 3] =
    ["for speeding", "for a log check", "over a light out"];

/// One short reminder this close to an announced open scale, if the truck is
/// still over the bypass speed with no scale exit armed. The full notice can
/// land miles out; nothing else spoke between it and the bypass point, and a
/// tester who mis-followed it heard silence all the way to the lights.
///
/// The floor of the window, not the whole of it: see
/// [`SCALE_REMINDER_REAL_LEAD_S`] and `scale_reminder_mi`.
pub const WEIGH_STATION_REMINDER_MI: f64 = 0.5;

/// The ceiling of the reminder window. The window only grows past half a
/// mile for a truck too fast for half a mile at the real clock to hold
/// [`SCALE_REMINDER_REAL_LEAD_S`]; this keeps that a short call, not a
/// second notice.
pub const WEIGH_STATION_REMINDER_MAX_MI: f64 = 1.0;

/// Real seconds the reminder must leave the driver before the scale can be
/// judged bypassed: hear "Signal for the scale exit", press the key, and let
/// the exit assist take the lane.
///
/// The window used to be a fixed half mile on the compressed clock, which at
/// ten times real speed and 61 mph is three real seconds -- a tester heard
/// the reminder and the bypass lights three seconds apart (log, 2026-10-07).
/// Now the reminder holds the clock to real time from where it speaks to the
/// gore, the way a signaled exit already does, and a crossing that comes
/// sooner than this after the reminder (a late notice, a busy cab) is not
/// charged at all.
pub const SCALE_REMINDER_REAL_LEAD_S: f64 = 15.0;

/// The sentence the open-scale lead distance is sized from. It must pace the
/// real announcement's longest realistic rendering -- a long stop name -- or
/// the spoken lead undershoots and the notice lands with no road left to act
/// on. The notice no longer names keys, but the sample keeps the lead it had
/// when it did: sized to the shorter line, the notice fell to the same two
/// miles as the stop's own distance call and lost the frame to it.
pub const SCALE_NOTICE_SAMPLE: &str = concat!(
    "Open weigh station ahead in two miles: Northbound Platte River Port ",
    "of Entry. All trucks must pull in. Signal for the scale exit with ",
    "right bumper plus D-pad down; the ramp brings you down to the scale. ",
    "Once you are stopped at the scale, press right bumper plus D-pad down ",
    "to check in."
);

// The fines for the things an officer sees rather than clocks -- chain law,
// following too close, lights, lane misuse -- are priced in
// models/enforcement, with every other citation amount and the multipliers
// that scale them, and reach this module through the driving_core prelude.
// They used to be declared here as well as there, two constants claiming to be
// the same Colorado citation with nothing keeping them equal.

impl DrivingState {
    // -- presence ------------------------------------------------------------

    /// How loud the policed country is right here, from the road itself.
    ///
    /// This was a player setting (full / standard / quiet) until 2026-08-16.
    /// It never touched placement, staffing or odds -- `announced` is set for
    /// every staffed post whatever the level -- but it did decide whether the
    /// marked-unit pass for an EMPTY crossover played at all, and that is the
    /// thing that made the road sound saturated with police who then ignored a
    /// speeder going by. By ear an empty car and a staffed one were the same
    /// cue, so the setting was buying atmosphere that read as broken
    /// enforcement (owner ruling, 2026-08-16: remove it, make it dynamic).
    ///
    /// What replaces it is the road: the same region, road class and clock
    /// that decide where posts go now decide how loudly the country polices.
    /// A hot region on an interstate at the afternoon peak sounds policed; a
    /// cold-region state route at four in the morning barely does -- and both
    /// are facts about where the truck is rather than a slider the player can
    /// set and then wonder why the game feels different.
    ///
    /// Deliberately the SAME number the placement walk uses to decide how far
    /// apart to put the posts, rather than a second formula that could drift
    /// from it: if the road is carrying posts close together, it should sound
    /// like it.
    pub fn ambience_scale(&self) -> f64 {
        self.trip.post_density_at(self.trip.position_mi)
    }

    /// Whether the cab already has a demand on the driver.
    ///
    /// The weigh-station and unsafe-damage checks used to guard on the stop
    /// and the ramp but not on the hazard deadline, so a trooper could light
    /// you up in the middle of a braking window you had two seconds to make.
    pub fn enforcement_busy(&self) -> bool {
        self.pull_over.is_some()
            || self.ramp_mi.is_some()
            || self.hazard_deadline.is_some()
            || self.microsleep_deadline.is_some()
            || self.arrival_menu_open
    }

    // -- scheduled audio -----------------------------------------------------

    pub fn schedule_sound(&mut self, delay_s: f64, key: &str, volume: f64, pan: f64) {
        self.pending_sounds
            .push((delay_s, key.to_string(), volume, pan));
    }

    pub fn service_pending_sounds(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.pending_sounds.is_empty() {
            return;
        }
        let mut still: Vec<PendingSound> = Vec::new();
        for (delay, key, volume, pan) in std::mem::take(&mut self.pending_sounds) {
            let remaining = delay - dt;
            if remaining <= 0.0 {
                ctx.audio.play_with(&key, volume, pan);
            } else {
                still.push((remaining, key, volume, pan));
            }
        }
        self.pending_sounds = still;
    }

    // -- radio ---------------------------------------------------------------

    /// Make a hole in the programme for an enforcement earcon.
    ///
    /// The catalog ships dozens of always-available police and fire scanner
    /// streams, so an enforcement earcon played on top of the radio is
    /// competing with material that sounds exactly like it. Ducking is what
    /// makes the synthesized signature legible.
    ///
    /// Honors the player's ducking setting (owner, 2026-08-17: the cop marker
    /// ducked the radio with auto-ducking off). The setting reads "game sounds
    /// step back for speech" and this is an earcon rather than speech, which is
    /// how it came to be exempt -- but from the seat there is one behavior with
    /// one name, and a player who has said do not step my audio back did not
    /// mean "except for this". The earcon still plays at full level; it just no
    /// longer digs itself a hole first.
    pub fn duck_radio_for_cue(&mut self, ctx: &mut GameContext) {
        if !ctx.settings.duck_audio_for_speech {
            return;
        }
        self.radio_cue_duck = RADIO_CUE_DUCK;
        self.radio_cue_duck_s = RADIO_CUE_DUCK_S;
        self.apply_radio_volume(ctx);
    }

    pub fn service_radio_cue_duck(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.radio_cue_duck_s <= 0.0 {
            return;
        }
        self.radio_cue_duck_s -= dt;
        if self.radio_cue_duck_s <= 0.0 && self.radio_cue_duck != 1.0 {
            self.radio_cue_duck = 1.0;
            self.apply_radio_volume(ctx);
        }
    }

    /// Kill the radio outright for the duration of a stop.
    ///
    /// Cut, not ducked. The sudden silence is itself an unambiguous cue that
    /// something has taken the cab over, and it removes any chance of a scanner
    /// stream being mistaken for the cruiser behind you.
    pub fn cut_radio_for_stop(&mut self, ctx: &mut GameContext) {
        if self.radio_cut_for_stop {
            return;
        }
        self.radio_cut_for_stop = true;
        ctx.audio.stop_music_with(200);
    }

    pub fn restore_radio_after_stop(&mut self, ctx: &mut GameContext) {
        if !self.radio_cut_for_stop {
            return;
        }
        self.radio_cut_for_stop = false;
        self.radio_cue_duck = 1.0;
        self.radio_cue_duck_s = 0.0;
        self.play_radio_current(ctx);
    }
}
