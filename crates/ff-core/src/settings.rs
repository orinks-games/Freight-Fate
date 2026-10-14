//! Persistent game settings (units, volumes, transmission mode, pacing).
//!
//! Port of `freight_fate/settings.py`. The struct, its defaults, the preset
//! tables and the behaviour helpers live here; the file format, the loader
//! and its nineteen migrations are in [`migrate`]; where the file lives is
//! [`paths`].
//!
//! The Python dataclass copied whatever a settings file said straight onto
//! the attribute and sorted it out in `from_dict`. The fields here are typed,
//! so that first copy already coerces (see `coerce` in `migrate`): a value of
//! the wrong JSON shape lands on the same fallback the Python migration
//! would have taken it to, and the value checks then run exactly as written.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::pyfmt::{fmt_f, fmt_grouped, round_py_int};
use crate::speech_pacing::{disposition_for, Disposition, SpeechCategory, DEFAULT_DRIVING_SPEECH};
use crate::units::{
    distance_unit, hud_speed, spoken_distance, spoken_gap, to_distance, MILES_TO_KM,
};

mod migrate;
pub mod paths;
#[cfg(test)]
mod tests;

pub use migrate::{parse_settings_text, py_json_dump_flat, SETTINGS_FILE_NAME};
pub use paths::{
    data_dir, game_root, save_root, set_thread_data_dir, thread_data_dir, DATA_DIR_ENV,
};

// Driving-mode pacing, as a game-clock multiplier: at 20x, one real minute
// is twenty game minutes. "Realistic" (40x) was retired on 2026-08-19 by
// owner ruling, and the name is why: it was the MOST compressed setting on
// the row, so the one word on the dial that promises real driving delivered
// the furthest thing from it -- real driving is 1x, and 40x made the driving
// day flash past. A save that still carries it lands on standard and is told
// once (RETIRED_TIME_SCALE below).
// Real time (1x) joined the row on 2026-08-22: the driving clock runs at the
// speed of the wall clock, which with live weather is as true to life as the
// game gets. It is last in the cycle so the two compressed pacings keep their
// places, and like the others it can be changed mid-drive from the pause menu,
// taking effect when the truck next stops (PACE_CHANGE_MAX_MPH).
pub const TIME_SCALES: [f64; 3] = [10.0, 20.0, 1.0];
pub const RETIRED_TIME_SCALE: f64 = 40.0;
pub const TIME_SCALE_FALLBACK: f64 = 20.0;
/// Budget for the "your pacing row lost a setting" line, spent one visit at
/// a time on the real control. Same shape and reasoning as
/// LANE_KEEPING_RENAME_NOTICES: it queues behind the row announcement, so a
/// player who keeps arrowing loses it and the next visit says it again.
pub const PACE_RETIRED_NOTICES: i64 = 3;
pub const PROFILE_SHARING_CONSENT_VERSION: i64 = 3;

/// Bumped when the settings *menu* is reorganized enough that a returning
/// player needs telling where things moved -- it tracks the shape of the
/// menus, not any one field. A load that finds an older value on disk (or
/// none) records which version it came from, and every notice newer than
/// that is spoken once; a fresh install writes the current version and hears
/// nothing.
///
/// 1: the Gameplay category (Driving assistance, Difficulty and hours of
///    service, World and traffic, Controls), and the world-data rows leaving
///    Speech and weather.
/// 2: the speed keeper moving to Driving assistance, and the lane and edge
///    cue volume moving to Audio.
/// 3: speech_verbosity (0 terse / 1 normal) became the driving_speech ladder.
pub const SETTINGS_VERSION: i64 = 3;

/// Which chatter switch governs each roadside-callout category. Zone entries
/// (parks, forests, wilderness) share one switch; the lone highway heritage
/// marker rides with the scenic passes. "village" is deliberately absent:
/// town and village names are not chatter. They are governed by the
/// place_callouts ladder instead, because the name that explains a speed
/// limit drop must survive a player turning the ambient colour off.
pub const CHATTER_CATEGORY_FIELDS: [(&str, &str); 10] = [
    ("national_park", "chatter_parks"),
    ("national_forest", "chatter_parks"),
    ("wilderness", "chatter_parks"),
    ("protected_area", "chatter_parks"),
    ("river", "chatter_rivers"),
    ("mountain_pass", "chatter_passes"),
    ("highway_marker", "chatter_passes"),
    ("museum", "chatter_museums"),
    ("billboard", "chatter_billboards"),
    // Placed roadside billboards baked as leg landmarks (billboard spider);
    // ride the same switch as the random-pool billboards so one toggle
    // governs both.
    ("billboard_sign", "chatter_billboards"),
];

/// The player-facing chatter switches, in menu order.
pub const CHATTER_FIELDS: [&str; 5] = [
    "chatter_parks",
    "chatter_rivers",
    "chatter_passes",
    "chatter_museums",
    "chatter_billboards",
];

/// How much the ride-along says about the places along the road, whatever
/// place data the world carries (curated route towns on one line, the baked
/// village layer on another -- the player never needs to know which).
pub const PLACE_CALLOUT_MODES: [&str; 3] = ["off", "sparse", "all"];

/// How often a background cloud backup's all-clear ("<career> is backed
/// up.") is spoken: at every accepted upload, once per career per session,
/// or never. Refusals and the "backed up again" recovery line speak at
/// every tier -- a career silently stopping backing up is the failure the
/// all-clear exists to rule out.
pub const BACKUP_ANNOUNCEMENT_MODES: [&str; 3] = ["every", "once", "off"];

/// How much of the lane-holding work the truck does for the driver. The
/// setting used to be called ``steering_assist`` and its values read exactly
/// backwards: "off" meant the truck held the lane FOR you and took your
/// exits, while "realistic" was the manual task -- and "off" was the
/// default. A player who believed they had turned assistance off was in the
/// most assisted mode there is, and their exits took themselves. The values
/// now name what the truck does.
pub const LANE_KEEPING_MODES: [&str; 3] = ["full", "partial", "off"];

/// The one value every fallback lands on: the loader's, the menu label's,
/// and the spoken notice's. They agreed only by coincidence before, which is
/// how a label and a behaviour can quietly come apart. "full" is right
/// because landing on "off" instead would start drift, rumble strips, and
/// off-road damage AND stop granting the destination exit -- a difficulty
/// spike with no audible cause. It is safe as a default and unsafe as a
/// *silent* one, so an unreadable value is spoken about once (see
/// `Settings::lane_keeping_unreadable`).
pub const LANE_KEEPING_FALLBACK: &str = "full";

/// Spoken value labels. The bare value word must never appear alone: "off"
/// here means the hardest mode, while "off" in overspeed warning, the speed
/// keeper, and descent control all mean less help. The clause is what keeps
/// a listener from carrying the wrong sense across rows, and "full" has to
/// disambiguate itself from "full manual" in the same breath.
pub const LANE_KEEPING_LABELS: [(&str, &str); 3] = [
    (
        "full",
        "full, the truck holds the lane and takes your exits",
    ),
    ("partial", "partial, gentle drift and you steer with help"),
    ("off", "off, you hold the lane and take your own exits"),
];

/// Every legacy value maps to the mode that behaves identically, so the
/// rename moves nobody's difficulty. Anything else -- a corrupt file, a
/// value from a build that is not this one -- lands on the fallback above:
/// silently handing a blind player a manual steering task they never opted
/// into is by far the worse failure.
pub const LANE_KEEPING_FROM_LEGACY: [(&str, &str); 3] =
    [("off", "full"), ("light", "partial"), ("realistic", "off")];
pub const LANE_KEEPING_TO_LEGACY: [(&str, &str); 3] =
    [("full", "off"), ("partial", "light"), ("off", "realistic")];

/// How many times the row explains its own rename before it stops. A note
/// queued behind a row announcement is cut off when the player keeps
/// arrowing, so one shot is not enough for something this consequential;
/// three makes a lost announcement self-correcting.
pub const LANE_KEEPING_RENAME_NOTICES: i64 = 3;

/// The adaptive-cruise cushion choices. The Python settings module imported
/// these from `states.driving_core` (`ACC_GAP_CHOICES`, seconds per
/// setting, and `ACC_GAP_DEFAULT`); the driving state owns the seconds, so
/// this copy exists only for the value check on load and must stay in step
/// with the driving port's table. Every one of them sits far clear of the
/// tailgating threshold (1.2 s) -- the closest setting the game offers must
/// never be a setting that gets the driver ticketed for choosing it.
pub const ACC_GAP_CHOICES: [(&str, f64); 3] = [("close", 2.5), ("normal", 3.0), ("far", 3.5)];
pub const ACC_GAP_DEFAULT: &str = "normal";

pub const LANE_CUE_LOUDNESS_MODES: [&str; 3] = ["subtle", "standard", "prominent"];
pub const DESCENT_SPEED_CONTROL_MODES: [&str; 4] = ["off", "realistic", "balanced", "interactive"];
pub const PEDAL_LATCH_MODES: [&str; 2] = ["on", "off"];
pub const UPDATE_CHANNELS: [&str; 3] = ["", "stable", "dev"];

/// One preset field's value: the assist switches are bools, descent speed
/// control and lane keeping are modes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AssistValue {
    Flag(bool),
    Mode(&'static str),
}

pub const DRIVING_ASSIST_FIELDS: [&str; 9] = [
    "automatic_emergency_braking",
    "lane_departure_warning",
    "stop_and_go_assist",
    "descent_speed_control",
    "exit_speed_assist",
    // Facility stopping assistance is a preset field again (owner, 2026-09-11).
    // The 2026-08-31 merge with the rest-stop assist left it outside the
    // presets, so a fresh install on Balanced coasted past its own pickup
    // while the manual promised Balanced "stops for you at your destination".
    // Realistic leaves it off, the way a real truck's assists would.
    "destination_approach_assist",
    "curve_speed_assist",
    "route_transition_assist",
    // Lane keeping is a preset field like the rest. It used to sit outside
    // them, which is how the preset row came to read "Realistic" over fully
    // automated lane keeping -- the one row a player checks to learn how much
    // the truck is doing could not see the biggest thing it was doing.
    "lane_keeping",
];

use AssistValue::{Flag, Mode};

pub const DRIVING_ASSIST_PRESETS: [(&str, [AssistValue; 9]); 3] = [
    (
        "realistic",
        [
            Flag(true),
            Flag(true),
            Flag(true),
            Mode("realistic"),
            Flag(true),
            Flag(false),
            Flag(true),
            Flag(true),
            Mode("off"),
        ],
    ),
    (
        "balanced",
        [
            Flag(true),
            Flag(true),
            Flag(true),
            Mode("balanced"),
            Flag(true),
            Flag(true),
            Flag(true),
            Flag(true),
            Mode("partial"),
        ],
    ),
    (
        "all",
        [
            Flag(true),
            Flag(true),
            Flag(true),
            Mode("interactive"),
            Flag(true),
            Flag(true),
            Flag(true),
            Flag(true),
            Mode("full"),
        ],
    ),
];

/// `DRIVING_ASSIST_PRESETS[name]`.
pub fn driving_assist_preset(name: &str) -> Option<&'static [AssistValue; 9]> {
    DRIVING_ASSIST_PRESETS
        .iter()
        .find(|(preset, _)| *preset == name)
        .map(|(_, values)| values)
}

/// The persisted fields, in the Python dataclass's declaration order
/// (which is the order `save` writes them in). Each row is
/// `name: type = default => coercion`, the coercion naming how a raw JSON
/// value lands on the typed field (see `migrate::coerce`).
macro_rules! settings_fields {
    ($( $(#[$attr:meta])* $name:ident : $ty:ty = $default:expr => $coerce:ident ),* $(,)?) => {
        #[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
        #[serde(default)]
        pub struct Settings {
            $( $(#[$attr])* pub $name: $ty, )*
            /// Set by `load` when the lane-keeping value on disk could not be
            /// read at all and the fallback was taken blind. Deliberately not a
            /// saved field: it describes one load, and the truck must say so
            /// once rather than leave a player wondering why their exits are
            /// suddenly being taken. (A `ClassVar` in Python, shadowed per
            /// instance by the loader.)
            #[serde(skip)]
            pub lane_keeping_unreadable: bool,
        }

        impl Default for Settings {
            fn default() -> Self {
                Self {
                    $( $name: ($default).into(), )*
                    lane_keeping_unreadable: false,
                }
            }
        }

        impl Settings {
            /// Every persisted field name, in declaration order
            /// (`dataclasses.fields(Settings)`).
            pub const FIELD_NAMES: &'static [&'static str] = &[ $( stringify!($name) ),* ];

            /// `asdict(self)`: every persisted field as JSON, in declaration
            /// order (the legacy `steering_assist` key is `save`'s business).
            pub fn ordered_values(&self) -> Vec<(&'static str, Value)> {
                vec![ $( (stringify!($name), Value::from(self.$name.clone())) ),* ]
            }

            /// The first step of `from_dict`: copy a raw file value onto the
            /// field it names, coercing to the field's type. `false` for a
            /// key that is not a field (ignored, as `hasattr` ignored it).
            pub(crate) fn assign_raw(&mut self, key: &str, value: &Value) -> bool {
                match key {
                    $( stringify!($name) => {
                        migrate::coerce::$coerce(&mut self.$name, value, key);
                        true
                    } )*
                    _ => false,
                }
            }
        }
    };
}

settings_fields! {
    /// Master switch for the orinks.net and sharing services: the drivers
    /// board, `online_presence`, `cloud_saves`, Mastodon sharing, and
    /// Discord presence behave as disabled while it is off, without losing
    /// their individual settings. Live-data simulation sources
    /// (`real_weather`, `real_traffic`, `real_parking`) are deliberately NOT
    /// gated here -- they follow their own Settings toggles (owner ruling,
    /// 2026-08-08: two testers lost real weather to this switch with no
    /// explanation at the weather toggle).
    online_services: bool = true => bool_truthy,
    imperial_units: bool = true => bool_truthy,
    /// The engine voice: "real" plays the multisample recorded-cab ring
    /// (release builds carry the licensed cuts); "classic" keeps the
    /// original single pitched loop for players who prefer the familiar
    /// sound.
    engine_voice: String = "real" => str_plain,
    /// The jake brake voice: "real" plays the recorded 1600 jake (the engine
    /// brake growl players hear today); "classic" swaps in the synthesized
    /// growl the game shipped before it, kept as the future jake A/B.
    jake_voice: String = "real" => str_checked,
    /// How much room adaptive cruise leaves to the vehicle ahead: close,
    /// normal, or far (2.5 / 3.0 / 3.5 seconds -- ACC_GAP_CHOICES). A
    /// preference rather than a difficulty, so it is deliberately NOT a
    /// driving-assistance preset field: choosing a longer cushion must not
    /// flip the preset row to Custom.
    ///
    /// It exists because the truck could put the driver inside a citation
    /// and they had no say in it (tester Darren, I-75 2026-08-18: fined
    /// 1,200 dollars for a gap adaptive cruise was managing). Enforcement no
    /// longer reads a momentary dip as tailgating, and now the cushion is
    /// the driver's call as well.
    acc_following_gap: String = "normal" => str_checked,
    /// friendlier default for new players
    automatic_transmission: bool = true => bool_truthy,
    /// Simple keeps the familiar hold-through-stop behavior. Deliberate
    /// requires a release and second press before an automatic changes
    /// direction.
    automatic_direction_changes: String = "simple" => str_checked,
    /// Distance compression while driving. Relaxed (10x) by default: new
    /// players get the most real time to hear and react to spoken events;
    /// veterans can step up to standard in Settings, Gameplay, or all the
    /// way to real time (1x), where the driving clock keeps pace with the
    /// wall clock. See TIME_SCALES for the three offered and why the old
    /// Realistic went.
    time_scale: f64 = 10.0 => float_plain,
    /// Spoken once (up to PACE_RETIRED_NOTICES times) to a player whose
    /// saved pacing was the retired Realistic, because their truck now bills
    /// the clock at half the rate it did and nothing else would tell them.
    pace_retired_notice_left: i64 = 0 => int_strict,
    /// live conditions from the NWS API, and with them the Service's active
    /// warnings (blizzard, high wind, flash flood, dense fog): dispatch plans
    /// around those and the cab reads one out on the way in
    real_weather: bool = false => bool_truthy,
    /// live traffic incidents from state 511 APIs
    real_traffic: bool = false => bool_truthy,
    /// live truck parking availability from TPIMS APIs
    real_parking: bool = false => bool_truthy,
    /// This week's US diesel price from the federal weekly survey at every
    /// pump, with each region's usual difference on top, in place of the
    /// price drawn for the session. On by default (owner ruling,
    /// 2026-09-12): the feed is keyless and the pumps should charge what
    /// the country is paying this week.
    real_fuel_prices: bool = true => bool_truthy,
    /// Preserve the historical behavior by default: live weather also
    /// follows the wall-clock date. Turn this off to let the career calendar
    /// advance while live conditions continue to come from the NWS.
    live_weather_controls_calendar: bool = true => bool_strict,
    /// hours of service: realistic/relaxed (debug_off is an internal dev
    /// bypass)
    hos_mode: String = "realistic" => str_checked,
    /// Optional early advice from the legal-reach planner. The mandatory
    /// hours warnings and requested readouts do not depend on this switch.
    hos_planning_hints: bool = false => bool_strict,
    /// How much of the lane-holding work the truck does. "full" keeps the
    /// truck centred, takes your exits for you, and turns Left and Right
    /// into tap lane changes. "partial" drifts gently and gives you generous
    /// steering authority, but the lane work is yours. "off" is the whole
    /// manual task: you hold the lane, and every exit needs its signal and
    /// its exit lane. It is one of the preset fields, so the preset row can
    /// never again read "Realistic" over fully automated lane keeping.
    ///
    /// The default is the realistic preset's value, so a fresh install
    /// really is the ruleset the preset row has been claiming all along:
    /// for months the row read "Realistic" while lane keeping was fully
    /// automated, because the preset could not see this field. Owner ruling
    /// 2026-08-09 -- make the truck match the label players have been
    /// reading rather than renaming the label to match a setting nobody
    /// chose. Existing players are untouched: their saved value migrates to
    /// whatever they already had.
    ///
    /// Ships on PARTIAL (owner, 2026-09-18), which makes a fresh install the
    /// Balanced preset rather than Realistic. The lane model grew a heading
    /// that day, so steering turns the truck instead of sliding it, and a
    /// truck nobody steers genuinely leaves the road -- "off" stopped being
    /// the mild drift it used to be and became a continuous driving task.
    /// Partial steers for the error while leaving the driver something to
    /// feel and to fight, where full holds the lane outright and hands over a
    /// job a new driver never gets to learn.
    ///
    /// Ships on FULL (owner, 2026-09-30), making a fresh install the All
    /// assists preset: too many first drives went wrong at the wheel, street
    /// corners above all. The truck drives while the engine lean still plays,
    /// so a new driver hears the cue before being asked to steer by it, and
    /// steps down to Balanced when ready. Saved settings keep their value.
    lane_keeping: String = "full" => str_checked,
    /// How many more times the Lane keeping row explains that it used to be
    /// called Lane drift. Zero by default: a fresh install has nothing to
    /// explain, and only a load that actually found the old key on disk
    /// raises it. A setting rather than a profile field on purpose -- the
    /// rename is global, so a per-career counter would re-fire on every
    /// career and fire for careers created after the update, who never saw
    /// the old name.
    lane_keeping_rename_notice_left: i64 = 0 => int_strict,
    /// How loud the lane and edge cues speak: the edge-boundary textures,
    /// the lane locator, and the dead-man's-curve strips all scale by it.
    /// subtle/standard/prominent
    lane_cue_loudness: String = "standard" => str_checked,
    /// What the lane guide leans when you drift: the road bed you are
    /// already hearing, or a tone of its own.
    ///
    /// OFF by default, and that is the whole point. The community ruled
    /// against steering tones on the audiogames.net thread (JaceK,
    /// 2026-07-17: a continuous tone overwhelms the soundscape and hurts
    /// players with sensory or hearing conditions), Forza's blind driving
    /// assists reached the same answer, and the guide has panned the
    /// existing bed ever since. What the ruling objects to is a tone nobody
    /// asked for, so this is a choice and never a default.
    ///
    /// It exists because the bed genuinely fails some drivers: vehicle/road
    /// is -33.3 dBFS RMS against the engine's -18.7 and already runs at full
    /// gain by highway speed, and a bed 15 dB under the engine carries no
    /// pan at all (Darren, 2026-08-17). Regenerating the bed louder is still
    /// the fix that helps everyone and is still on the roadmap.
    lane_guide_tone: bool = false => bool_strict,
    /// The shipped defaults now match the realistic preset field for field
    /// -- lane keeping was the only one that did not, and it is the default
    /// the row has been claiming since before it could see that field.
    /// Moved from "realistic" to "balanced" on 2026-09-18 with `lane_keeping`,
    /// and to "all" on 2026-09-30, so the row a fresh install shows is still
    /// the truth about the ruleset it is running -- the whole point of the
    /// 2026-08-09 ruling.
    driving_assistance_preset: String = "all" => str_checked,
    automatic_emergency_braking: bool = true => bool_strict,
    lane_departure_warning: bool = true => bool_strict,
    stop_and_go_assist: bool = true => bool_strict,
    /// All assists' value: a fresh install is the All assists preset from
    /// 2026-09-30, and every field it names has to agree or the row reads
    /// "custom".
    descent_speed_control: String = "interactive" => str_checked,
    exit_speed_assist: bool = true => bool_strict,
    /// On in Balanced and All assists; see `descent_speed_control` above.
    destination_approach_assist: bool = true => bool_strict,
    /// An explicit-plan accessibility aid, separate from the realism
    /// presets: T plans a sleep stop, X signals for it, and only then may
    /// this bring the truck to a complete stop at the entrance. Presets
    /// never turn it on.
    selected_stop_assist: bool = false => bool_strict,
    curve_speed_assist: bool = true => bool_strict,
    route_transition_assist: bool = true => bool_strict,
    /// Lets an armed speed-control session cover low-speed zones without a
    /// held accelerator, then hand back to adaptive cruise on open roads.
    /// This input accessibility aid stays independent of the assistance
    /// preset above: presets never touch it.
    speed_keeper: bool = true => bool_truthy,
    /// Cruise reads the baked grade profile a mile and a half ahead and
    /// plans against it: banks a little momentum before a climb, gives up
    /// the last few mph at a crest instead of fighting for them, and stops
    /// adding speed it is about to brake away before a descent. Every modern
    /// truck ships this as part of its cruise, not as a driver-assistance
    /// level, so it sits outside the assistance presets the way the speed
    /// keeper does.
    predictive_cruise: bool = true => bool_truthy,
    /// Double-tap-and-hold latches the brake key so a steady snub needs
    /// no sustained hold; a fresh press of the same key or the opposite
    /// pedal releases it. The throttle key never latches: at a standstill
    /// it is only for moving and for the direction-change hold. The same
    /// input-accessibility layer as the keeper: presets never touch it.
    /// "on" / "off"; older three-way values ("assists first", "latch first")
    /// migrate to on.
    pedal_latch: String = "on" => pedal_latch,
    /// The co-driver reads the road: spoken curve calls from the baked
    /// geometry ("Sharp left, quarter mile, advise 35"), only for bends that
    /// actually demand slowing at your current speed. The first audible
    /// slice of the steering-by-ear work.
    curve_callouts: bool = true => bool_truthy,
    master_volume: f64 = 1.0 => level,
    sfx_volume: f64 = 0.8 => level,
    music_volume: f64 = 0.5 => level,
    /// Music source: false plays the full soundtrack; true swaps menu music
    /// and the Roadhouse for synthesized music -- pieces the game composes
    /// from `music_seed`, the restored 1.5 tracks and hand-made modules --
    /// for players who want no AI-made music (owner, 2026-09-21).
    synth_music: bool = false => bool_truthy,
    /// Seeds every synthesized piece. Global, not per career, so a seed a
    /// player likes sounds the same on any career and can be shared.
    music_seed: i64 = 48213 => int_lenient,
    radio_volume: f64 = 0.25 => level,
    radio_enabled: bool = true => bool_truthy,
    radio_station_id: String = "route_playlist" => str_checked,
    /// The one radio licensing gate: on hides real public streams and
    /// personal playlists so nothing licensed reaches a broadcast. Off by
    /// default -- the full dial is the out-of-the-box experience, and safe
    /// mode is the explicit choice a streamer makes. (The former separate
    /// real-streams opt-in folded into this switch, 2026-08-12.)
    radio_streamer_safe: bool = false => bool_truthy,
    /// Personal playlists play in a random order: every track once before
    /// any repeats, a fresh order each lap, and a lap never opens on the
    /// track that just ended. Off plays the file top to bottom and resumes
    /// where it left off. The Playlists folder was built for M3U stream
    /// lists and is being used for MP3 collections (Hailey, drivers board,
    /// 2026-09-18).
    radio_shuffle_playlists: bool = false => bool_truthy,
    weather_volume: f64 = 0.65 => level,
    engine_volume: f64 = 0.55 => level,
    ui_volume: f64 = 0.9 => level,
    /// Step the game sounds back while the road voice speaks: engine,
    /// weather, and the radio drop to half volume for the length of the
    /// line, then come back (XAG 105; speech priority research, R13). Off by
    /// default: in an audio-first sim the engine is the instrument panel --
    /// a blind driver reads speed off it -- so ducking is opt-in for players
    /// who need it, not a default that changes what everyone hears (owner,
    /// 2026-08-12).
    duck_audio_for_speech: bool = false => bool_strict,
    /// How much of the road's INFORMATION speaks: a ladder of named rungs
    /// that cut whole categories, not one global compression. Flavor is not
    /// governed here -- billboards, places and landmarks answer to the
    /// chatter switches and the place-callouts ladder (owner, 2026-08-15).
    driving_speech: String = DEFAULT_DRIVING_SPEECH => str_checked,
    /// Roadside chatter: the ambient color spoken between navigation cues.
    /// Each category has its own switch so a player can keep the geography
    /// (rivers, passes) while silencing the jokes (billboards), or vice
    /// versa. Safety and navigation speech is never affected by these.
    /// entering parks, forests, and wild lands
    chatter_parks: bool = true => bool_strict,
    /// named river crossings
    chatter_rivers: bool = true => bool_strict,
    /// mountain passes and scenic highway markers
    chatter_passes: bool = true => bool_strict,
    /// museums and roadside attractions
    chatter_museums: bool = true => bool_strict,
    /// parody billboards
    chatter_billboards: bool = true => bool_strict,
    /// Place names along the road. "sparse" speaks only the names that
    /// explain a speed limit change ("Entering Strawberry" right before the
    /// 35); "all" adds the towns the route passes; "off" silences place
    /// names entirely. The full baked place layer is never read aloud at any
    /// tier -- it exists to answer on-demand orientation questions.
    place_callouts: String = "sparse" => str_checked,
    /// speak "N of M" position in menus
    announce_menu_position: bool = true => bool_truthy,
    /// how often "<career> is backed up." is spoken after a background
    /// cloud backup: "every" upload, "once" per career per session, "off"
    backup_announcements: String = "every" => str_checked,
    /// driving events on a separate voice
    sapi_events: bool = true => bool_truthy,
    /// which voice that is (e.g. SAPI/OneCore)
    event_backend: String = "SAPI" => str_checked,
    /// Send every line to the screen reader's braille display and speak
    /// nothing: menus, readouts, and the driving events that would otherwise
    /// go to the separate event voice. Asked for on AppleVis (2026-09-02) by
    /// a player who plays from the display. Only NVDA and JAWS can braille
    /// through Prism; with any other voice bound the game keeps speaking, and
    /// the Settings row says so, because a silent game is the one outcome
    /// this must never produce.
    braille_only: bool = false => bool_strict,
    /// voice speed, 0..1 (backend default ~0.5)
    speech_rate: f64 = 0.5 => level,
    /// voice pitch, 0..1 (backend default ~0.5)
    speech_pitch: f64 = 0.5 => level,
    /// voice loudness, 0..1
    speech_volume: f64 = 1.0 => level,
    /// installed voice name; "" = backend default
    speech_voice: String = "" => str_plain,
    /// "stable"/"dev"; "" follows this build's channel
    update_channel: String = "" => str_checked,
    /// release tag the player chose to skip
    skipped_update: String = "" => str_plain,
    /// show broad activity in Discord (privacy-safe)
    discord_presence: bool = true => bool_truthy,
    /// Share the public driver profile and on-duty board status on
    /// orinks.net. Off here because a player with no account has nothing to
    /// share: without a confirmed driver identity nothing is ever sent (see
    /// online_presence). Connecting an account turns this on, since a
    /// connected account that publishes nothing leaves a profile reading "no
    /// career statistics yet". orinks.net stays the authority: this only
    /// flips true once the server confirms, and board listing further
    /// requires choosing the public visibility on the site.
    online_presence: bool = false => bool_truthy,
    /// Let a signed-in player send Call Dispatch requests to an opted-in
    /// remote dispatcher, with the local office as fallback.
    remote_dispatch_calls: bool = false => bool_truthy,
    /// speak when another driver goes on or off duty
    duty_notifications: bool = false => bool_truthy,
    profile_sharing_consent_version: i64 = 0 => int_lenient,
    /// A failed server revocation keeps public state uncertain, but stops
    /// all local publication immediately and retries when the player
    /// activates the stable Profile sharing item again.
    profile_sharing_pending_off: bool = false => bool_truthy,
    /// Back up saves to the player's own Orinks account after each local
    /// save. Off here for the same reason as `online_presence`: no account,
    /// nothing to upload. Connecting an account turns this on -- the public
    /// career statistics are derived from the latest accepted backup, so the
    /// two only make sense together -- and the Online menu turns it off
    /// again on its own.
    cloud_saves: bool = false => bool_strict,
    /// Post short public summaries of notable deliveries (new badges, level
    /// ups, perfect streaks) to the player's own Mastodon account through
    /// orinks.net. Off by default, separate from Profile sharing, and inert
    /// until a Mastodon account is linked on the site.
    mastodon_sharing: bool = false => bool_strict,
    /// Last-known link state and handle, refreshed on every status check.
    /// Two fields because a link can exist without a handle (the server
    /// could not read the account name): linked gates the toggle, the handle
    /// is only spoken. The server stays the authority; this cache only keeps
    /// the settings menu from needing the network to read a label.
    mastodon_linked: bool = false => bool_strict,
    mastodon_linked_handle: String = "" => str_checked,
    /// accept game-controller input alongside the keyboard
    controller_enabled: bool = true => bool_strict,
    /// rumble/vibration feedback on the controller
    haptics_enabled: bool = true => bool_strict,
    /// Whether the one-time first-run offer to connect this computer to
    /// orinks.net has been made. Per install, not per career: the
    /// connection belongs to the computer, so a second career must not ask
    /// again. Set on either answer, so declining is respected and the
    /// prompt cannot reappear after a mid-prompt quit.
    online_offer_seen: bool = false => bool_truthy,
    /// The settings-menu layout version this file was last written by. See
    /// SETTINGS_VERSION, which lists what each one changed: an older value
    /// on load means every layout above it is new to this player, so the
    /// Gameplay submenu explains once where their settings moved.
    settings_version: i64 = SETTINGS_VERSION => int_lenient,
    /// Which layout version this player was last told about. Set on load
    /// when an older settings_version was found on disk, and cleared back
    /// to -1 (nothing owed) the first time the Gameplay submenu speaks the
    /// "where things moved" notices for every version above it. Persisted
    /// so a player who quits before opening Gameplay still hears it next
    /// time; a fresh install never sets it. An int rather than the single
    /// bool it replaced, so the next reorganization does not need a field
    /// of its own -- and so a player two layouts behind hears both moves
    /// instead of only the newest.
    settings_layout_notice_from: i64 = -1 => int_strict,
    /// Driving controls the player moved to other keys, as `action=chord`
    /// pairs joined with semicolons (`engine=z;cruise_resume=shift+o`);
    /// empty means every key is at its default. The game crate's
    /// `bindings` module owns the vocabulary.
    key_bindings: String = "" => str_plain,
    /// The same for pad buttons (`engine=mod+a;horn=paddle_1`).
    pad_bindings: String = "" => str_plain,
    /// The same for the iPhone and iPad driving gestures
    /// (`tap=fuel;magic_tap=none`).
    touch_bindings: String = "" => str_plain,
    /// Steer AWAY from the engine's lean instead of toward it, for
    /// drivers who learned that habit in audio racing games.
    steering_guide_inverted: bool = false => bool_strict,
    /// Whether the player has answered the one-time Driving assistance
    /// picker that opens a launch. False on a fresh install and on every
    /// file written before it existed, so everyone answers it once
    /// (owner, 2026-09-30).
    assist_preset_chosen: bool = false => bool_strict,
}

impl Settings {
    /// One persisted field by name, as JSON (`getattr(settings, name)`).
    pub fn field_value(&self, name: &str) -> Option<Value> {
        self.ordered_values()
            .into_iter()
            .find(|(field, _)| *field == name)
            .map(|(_, value)| value)
    }

    /// Set one persisted field by name from a JSON value, with the same
    /// coercion the loader applies (`setattr(settings, name, value)`).
    /// `false` when `name` is not a field.
    pub fn set_field(&mut self, name: &str, value: &Value) -> bool {
        self.assign_raw(name, value)
    }

    /// Whether the truck holds the lane -- and takes the exits -- itself.
    ///
    /// Every spoken instruction about steering hangs off this one answer:
    /// automated means a tap changes lanes and the destination exit is
    /// granted, manual means holding a direction steers and every exit needs
    /// its signal and its lane. Nine call sites used to compare the raw
    /// string, which is exactly how spoken advice comes to name a key the
    /// driver's settings do not give them.
    pub fn lane_is_automated(&self) -> bool {
        self.lane_keeping == "full"
    }

    /// Whether the lane work -- and the exit -- belongs to the driver.
    pub fn lane_is_manual(&self) -> bool {
        !self.lane_is_automated()
    }

    /// Whether something supplies the wheel a bend asks for, so the truck
    /// follows the road's curve without the driver steering it: curve
    /// assistance, or partial lane keeping, which steers through the road's
    /// curve the same way while lane changes and speed stay the driver's
    /// (owner ruling, 2026-09-24). Lane keeping off adds nothing, so with
    /// curve assistance off too the bend is the driver's own wheel; full
    /// holds the lane outright.
    pub fn road_steers_the_bend(&self) -> bool {
        self.curve_speed_assist || self.lane_keeping == "partial"
    }

    /// The spoken value with the clause that says what it costs you.
    pub fn lane_keeping_label(&self) -> &'static str {
        lane_keeping_label_for(&self.lane_keeping).unwrap_or_else(|| {
            lane_keeping_label_for(LANE_KEEPING_FALLBACK).expect("fallback label")
        })
    }

    /// The preset fields' current values, in DRIVING_ASSIST_FIELDS order.
    pub fn assist_values(&self) -> [AssistValue; 9] {
        [
            Flag(self.automatic_emergency_braking),
            Flag(self.lane_departure_warning),
            Flag(self.stop_and_go_assist),
            Mode(static_mode(&self.descent_speed_control)),
            Flag(self.exit_speed_assist),
            Flag(self.destination_approach_assist),
            Flag(self.curve_speed_assist),
            Flag(self.route_transition_assist),
            Mode(static_mode(&self.lane_keeping)),
        ]
    }

    /// `getattr(settings, field)` for one preset field.
    pub fn assist_value(&self, field: &str) -> Option<AssistValue> {
        DRIVING_ASSIST_FIELDS
            .iter()
            .position(|name| *name == field)
            .map(|index| self.assist_values()[index])
    }

    /// `setattr(settings, field, value)` for one preset field; `false` when
    /// `field` is not a preset field or `value` is the wrong shape for it.
    pub fn set_assist_value(&mut self, field: &str, value: AssistValue) -> bool {
        match (field, value) {
            ("automatic_emergency_braking", Flag(v)) => self.automatic_emergency_braking = v,
            ("lane_departure_warning", Flag(v)) => self.lane_departure_warning = v,
            ("stop_and_go_assist", Flag(v)) => self.stop_and_go_assist = v,
            ("descent_speed_control", Mode(v)) => self.descent_speed_control = v.to_string(),
            ("exit_speed_assist", Flag(v)) => self.exit_speed_assist = v,
            ("destination_approach_assist", Flag(v)) => self.destination_approach_assist = v,
            ("curve_speed_assist", Flag(v)) => self.curve_speed_assist = v,
            ("route_transition_assist", Flag(v)) => self.route_transition_assist = v,
            ("lane_keeping", Mode(v)) => self.lane_keeping = v.to_string(),
            _ => return false,
        }
        true
    }

    /// Set every preset field to the named preset's values. `false` (and no
    /// change) for a name that is not a preset -- Python raised `KeyError`.
    pub fn apply_driving_assistance_preset(&mut self, preset: &str) -> bool {
        let Some(values) = driving_assist_preset(preset) else {
            return false;
        };
        for (field, value) in DRIVING_ASSIST_FIELDS.iter().zip(values.iter()) {
            self.set_assist_value(field, *value);
        }
        self.driving_assistance_preset = preset.to_string();
        true
    }

    /// Re-derive the preset row from the real fields: the one preset whose
    /// mapping matches exactly, or "custom".
    pub fn refresh_driving_assistance_preset(&mut self) -> &'static str {
        let values = self.assist_values();
        let mut matches = DRIVING_ASSIST_PRESETS
            .iter()
            .filter(|(_, mapping)| *mapping == values)
            .map(|(name, _)| *name);
        let name = match (matches.next(), matches.next()) {
            (Some(only), None) => only,
            _ => "custom",
        };
        self.driving_assistance_preset = name.to_string();
        name
    }

    /// How the player's rung delivers this category of information.
    pub fn speech_disposition(&self, category: Option<SpeechCategory>) -> Disposition {
        disposition_for(&self.driving_speech, category)
    }

    /// Whether this category reaches the voice at all on this rung.
    pub fn speaks(&self, category: Option<SpeechCategory>) -> bool {
        self.speech_disposition(category) != Disposition::Silent
    }

    /// Whether spoken lines take their terse rendering on this rung.
    ///
    /// The rung picks the rendering, so `SpokenMessage` keeps the
    /// single-boolean `render` signature S2 gave it.
    pub fn renders_terse(&self) -> bool {
        self.driving_speech == "quiet" || self.driving_speech == "urgent_only"
    }

    /// One chatter switch by field name (`getattr(settings, field)`).
    pub fn chatter_field(&self, field: &str) -> Option<bool> {
        match field {
            "chatter_parks" => Some(self.chatter_parks),
            "chatter_rivers" => Some(self.chatter_rivers),
            "chatter_passes" => Some(self.chatter_passes),
            "chatter_museums" => Some(self.chatter_museums),
            "chatter_billboards" => Some(self.chatter_billboards),
            _ => None,
        }
    }

    /// Set one chatter switch by field name; `false` when it is not one.
    pub fn set_chatter_field(&mut self, field: &str, enabled: bool) -> bool {
        match field {
            "chatter_parks" => self.chatter_parks = enabled,
            "chatter_rivers" => self.chatter_rivers = enabled,
            "chatter_passes" => self.chatter_passes = enabled,
            "chatter_museums" => self.chatter_museums = enabled,
            "chatter_billboards" => self.chatter_billboards = enabled,
            _ => return false,
        }
        true
    }

    /// Whether a roadside-callout category is currently spoken.
    ///
    /// Unknown categories default to on so a future bake category speaks
    /// rather than silently vanishing.
    pub fn chatter_enabled(&self, category: &str) -> bool {
        CHATTER_CATEGORY_FIELDS
            .iter()
            .find(|(name, _)| *name == category)
            .and_then(|(_, field)| self.chatter_field(field))
            .unwrap_or(true)
    }

    /// The master menu label state: everything, off, or custom.
    pub fn chatter_summary(&self) -> &'static str {
        let states: Vec<bool> = CHATTER_FIELDS
            .iter()
            .map(|field| self.chatter_field(field).unwrap_or(true))
            .collect();
        if states.iter().all(|on| *on) {
            "everything"
        } else if !states.iter().any(|on| *on) {
            "off"
        } else {
            "custom"
        }
    }

    pub fn set_all_chatter(&mut self, enabled: bool) {
        for field in CHATTER_FIELDS {
            self.set_chatter_field(field, enabled);
        }
    }

    // -- unit formatting ---------------------------------------------------------

    pub fn speed_text(&self, mph: f64) -> String {
        if self.imperial_units {
            format!("{} per hour", spoken_distance(mph, "mile"))
        } else {
            format!(
                "{} per hour",
                spoken_distance(mph * MILES_TO_KM, "kilometer")
            )
        }
    }

    /// `speed_text`'s bare number, for the terse slot grammar where the
    /// frame carries the unit ("Limit 65.").
    pub fn speed_value(&self, mph: f64) -> String {
        let value = if self.imperial_units {
            mph
        } else {
            mph * MILES_TO_KM
        };
        round_py_int(value).to_string()
    }

    /// Spoken distance in the player's unit. `precise` keeps one decimal for
    /// short spans ("1.2 miles ahead") where whole numbers would read as
    /// zero or lie by half a mile.
    pub fn distance_text(&self, miles: f64, precise: bool) -> String {
        distance_text_for(miles, self.imperial_units, precise)
    }

    /// Colloquial short range for pacenote-style calls: quarter-mile steps
    /// under a mile ("half a mile"), 100-meter steps under a kilometer ("400
    /// meters"), the normal precise form beyond.
    pub fn short_distance_text(&self, miles: f64) -> String {
        short_distance_text_for(miles, self.imperial_units)
    }

    /// A spoken distance kept to one decimal, for close-range cues.
    pub fn gap_text(&self, miles: f64) -> String {
        spoken_gap(miles, self.imperial_units)
    }

    /// Speed for the visual HUD, in the short written form.
    pub fn hud_speed_text(&self, mph: f64) -> String {
        hud_speed(mph, self.imperial_units)
    }

    /// A bare converted distance, for readouts that name the unit once after
    /// two numbers ("12 of 400 miles").
    pub fn distance_value(&self, miles: f64, decimals: usize, grouped: bool) -> String {
        let value = to_distance(miles, self.imperial_units);
        if grouped {
            fmt_grouped(value, decimals)
        } else {
            fmt_f(value, decimals)
        }
    }

    /// The player's distance unit, to pair with `distance_value`.
    pub fn distance_unit_text(&self, plural: bool) -> &'static str {
        distance_unit(self.imperial_units, plural)
    }

    /// A per-mile rate as a rate in the player's own distance unit.
    pub fn per_distance(&self, per_mile: f64) -> f64 {
        if self.imperial_units {
            per_mile
        } else {
            per_mile / MILES_TO_KM
        }
    }
}

/// [`Settings::distance_text`] with the unit setting passed rather than
/// borrowed.
///
/// The wording lives here, and the method is the one-line delegate, so a
/// caller that cannot hold a `&Settings` still speaks the same sentence. A
/// `say_event(valid=...)` gate is exactly that caller: it is `'static`, so it
/// captures the player's unit as a `bool` and asks this.
pub fn distance_text_for(miles: f64, imperial: bool, precise: bool) -> String {
    let value = to_distance(miles, imperial);
    let unit = if imperial { "mile" } else { "kilometer" };
    let text = if precise {
        fmt_f(value, 1)
    } else {
        fmt_f(value, 0)
    };
    let plural = if text.parse::<f64>().ok() == Some(1.0) {
        ""
    } else {
        "s"
    };
    format!("{text} {unit}{plural}")
}

/// [`Settings::short_distance_text`] with the unit setting passed rather than
/// borrowed. See [`distance_text_for`].
pub fn short_distance_text_for(miles: f64, imperial: bool) -> String {
    if imperial {
        if miles > 1.125 {
            return distance_text_for(miles, imperial, true);
        }
        let quarters = round_py_int(miles * 4.0).max(1);
        return match quarters {
            1 => "a quarter mile".to_string(),
            2 => "half a mile".to_string(),
            3 => "three quarters of a mile".to_string(),
            4 => "one mile".to_string(),
            _ => distance_text_for(miles, imperial, true),
        };
    }
    let km = miles * MILES_TO_KM;
    if km >= 0.95 {
        return distance_text_for(miles, imperial, true);
    }
    let meters = round_py_int(km * 10.0).max(1) * 100;
    format!("{meters} meters")
}

/// `LANE_KEEPING_LABELS.get(mode)`.
pub fn lane_keeping_label_for(mode: &str) -> Option<&'static str> {
    LANE_KEEPING_LABELS
        .iter()
        .find(|(name, _)| *name == mode)
        .map(|(_, label)| *label)
}

/// `LANE_KEEPING_FROM_LEGACY.get(value)`.
pub fn lane_keeping_from_legacy(value: &str) -> Option<&'static str> {
    LANE_KEEPING_FROM_LEGACY
        .iter()
        .find(|(legacy, _)| *legacy == value)
        .map(|(_, mode)| *mode)
}

/// `LANE_KEEPING_TO_LEGACY.get(mode, "off")`: the `steering_assist` value a
/// 1.8.x build reads for this mode.
pub fn lane_keeping_to_legacy(mode: &str) -> &'static str {
    LANE_KEEPING_TO_LEGACY
        .iter()
        .find(|(name, _)| *name == mode)
        .map(|(_, legacy)| *legacy)
        .unwrap_or("off")
}

/// `ACC_GAP_CHOICES[name]`, the cushion in seconds.
pub fn acc_gap_seconds(name: &str) -> Option<f64> {
    ACC_GAP_CHOICES
        .iter()
        .find(|(choice, _)| *choice == name)
        .map(|(_, seconds)| *seconds)
}

/// A mode field's value as the static str the preset tables use, or a
/// marker no preset matches when it holds something outside the tables
/// (only possible between the raw copy and the value checks in `from_dict`).
fn static_mode(value: &str) -> &'static str {
    for mode in DESCENT_SPEED_CONTROL_MODES {
        if mode == value {
            return mode;
        }
    }
    for mode in LANE_KEEPING_MODES {
        if mode == value {
            return mode;
        }
    }
    "\u{1}unknown"
}
