//! The driving state (port of `freight_fate/states/driving.py`, the
//! "compatibility facade" that composed sixteen mixins onto `State`).
//!
//! # Shape of the port
//!
//! Python's `DrivingState` was one fat object partitioned only by file: every
//! mixin read and wrote `self.<attr>` on state owned by `__init__` or by other
//! mixins. Here it is one struct -- [`DrivingState`], every field listed and
//! grouped by the Python module that initialises or owns it -- and one
//! `impl DrivingState` block per former mixin file (`driving_controls.rs`,
//! `driving_updates.rs`, ...). There is no dynamic dispatch to preserve; the
//! mixins were namespaces.
//!
//! Three things are not fields:
//!
//! * `self.truck` and `self.weather` -- the Rust `Trip` owns its
//!   `TruckState` and `WeatherSystem`, so they are `self.trip.truck` and
//!   `self.trip.weather` ([`DrivingState::truck`] / [`DrivingState::weather`]
//!   are the short forms). Python aliased one object from two places.
//! * `self.hos` -- an alias of `profile.hos`; use
//!   [`crate::states::driving_core::hos_of`] / `hos_mut_of`.
//! * `self._radio_backend` -- built per call by
//!   [`DrivingState::with_radio_backend`].
//!
//! The `State` impl forwards to methods the mixin modules provide; until a
//! mixin lands, [`pending`] carries a temporary stub for each such method so
//! the crate builds. A mixin task deletes its stubs from `pending` as it
//! implements them and touches nothing else here.
//!
//! Submodules: `driving/init.rs` (`new`), `driving/snapshot.rs` (save and
//! resume), `driving/lifecycle.rs` (enter/exit and the facade's own
//! helpers), `driving/facade.rs` (the trip accessors, the trip swap and the
//! `State` impl).

mod facade;
mod init;
mod lifecycle;
mod snapshot;

use std::collections::{HashMap, HashSet, VecDeque};

use ff_core::data::curves::RouteCurve;
use ff_core::data::world_models::Route;
use ff_core::models::jobs::Job;
use ff_core::pyrandom::PyRandom;
use ff_core::radio::RadioState;
use ff_core::sim::cross_traffic::CrossTraffic;
use ff_core::sim::enforcement_observe::Observation;
use ff_core::sim::lane::LaneKeeping;
use ff_core::sim::lane_guidance::LaneGuidance;
use ff_core::sim::pedal_latch::PedalLatch;
use ff_core::sim::trip::Trip;
use ff_core::sim::trip_models::{ExitRampLayout, RoadStop};
use ff_core::sim::turn_guide::TurnGuide;

use crate::states::driving_core::{
    CbChatterRecall, CurveRun, Instructor, PendingAmbient, PendingSound, RigBuffs, SirenLoop,
};
use crate::states::driving_updates::curve_servo::CurveServo;

pub use snapshot::ACTIVE_TRIP_DEADLINE_MODEL;

/// `_destination_exit_cache`: the position the scan was computed at, and
/// what it found -- `(miles ahead, exit label, spoken phrase)`, or `None`
/// where there is no destination exit to take.
pub type DestinationExitScan = (f64, Option<(f64, String, String)>);

/// Live truck control with a fully audio HUD.
///
/// Field groups are headed by the Python module that initialises them; a
/// group's fields are listed in the order `__init__` (or the module's own
/// `_reset_*`/`_*_init` helper) assigned them. Leading underscores are
/// dropped (`self._cruise_mph` -> `cruise_mph`).
/// One lap of a shuffled personal playlist: every entry index once, in the
/// order this lap plays them, and the cursor into that order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaylistShuffleLap {
    pub order: Vec<usize>,
    pub cursor: usize,
    /// Which lap this is, so each one seeds a different order.
    pub lap: u64,
}

/// What the driver has done at the stop the truck is parked at. It lives on
/// the drive, not the stop's menu, because leaving the menu and opening it
/// again with T is the same visit: the fuel still pays for the shower, the
/// scale check-in is still done, the CAT ticket still prices a reweigh.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct StopVisit {
    /// `RoadStop::key` of the stop; any other stop starts a fresh visit.
    pub key: String,
    pub fueled: bool,
    /// A loyalty shower credit or points redeemed here: the shower is free.
    pub free_shower: bool,
    pub inspected: bool,
    /// Game hour of this visit's full-price CAT Scale ticket.
    pub full_weigh_h: Option<f64>,
}

pub struct DrivingState {
    // ---- driving.py: identity -----------------------------------------------------------
    pub job: Job,
    /// The route the drive was dispatched on. `trip.route` is the active
    /// one and differs on a surface or departure street chain.
    pub route: Route,
    /// `DRIVE_PHASE_PICKUP` / `DRIVE_PHASE_DELIVERY` / `DRIVE_PHASE_SCHOOL`.
    pub phase: &'static str,
    pub trip_seed: i64,
    pub resumed: bool,
    /// The trip, which owns the truck and the weather (`self.trip.truck`,
    /// `self.trip.weather`).
    pub trip: Trip,
    /// Bumped every time `trip` is replaced (surface/departure chain swaps);
    /// the Python `id(self.trip)` the turn latches compared against.
    pub trip_generation: u64,
    pub start_damage: f64,
    // Trip-start wear, for "this run added..." deltas at settlement
    // (mid-trip saves re-sync the profile, so the profile can't provide
    // these once the trip is underway).
    pub start_tire_wear: f64,
    pub start_brake_wear: f64,
    pub start_engine_wear: f64,
    // Rig-care buffs (quick lube, tire rotation) hold for the rest of
    // the trip and die with it -- keyed by buff group, see data/buffs.py.
    pub rig_buffs: RigBuffs,
    /// See [`StopVisit`]; read and written through `stop_visit`.
    pub stop_visit: StopVisit,
    /// The truck moved in automatic at some point on this run, so the gate
    /// pays no manual-spec differential however the run ends.
    pub drove_automatic: bool,
    pub weather_source_real: bool,
    /// The route mile the cab next asks the Weather Service for warnings at.
    pub alerts_next_poll_mi: f64,
    /// The point asked about and not yet answered: latitude, longitude, and
    /// the route mile it was asked at.
    pub alerts_pending: Option<(f64, f64, f64)>,
    /// Warning ids already read out this drive, so each is said once.
    pub alerts_said: std::collections::HashSet<String>,
    pub live_weather_controls_calendar: bool,
    pub traffic_source_real: bool,
    pub parking_source_real: bool,
    pub lane: LaneKeeping,

    // ---- driving.py: music and radio (read by driving_updates) ------------------------
    pub day_music_sequence: Vec<String>,
    pub night_music_sequence: Vec<String>,
    pub music_night: bool,
    pub radio: RadioState,
    // Station rotation: per-station shuffled song order, with break slots
    // (host/id/ad) every few songs on the stations that have a live host.
    pub radio_station_id: String,
    pub radio_playlist: Vec<String>,
    /// The playlist entry now playing, as resolved (a synthesized piece's
    /// classic stand-in while it renders); empty when nothing was resolved.
    pub radio_playing_key: String,
    pub radio_track_index: usize,
    pub radio_elapsed_s: f64,
    pub radio_break_queue: Vec<String>,
    pub radio_break_pos: usize,
    pub radio_break_count: usize,
    pub radio_tracks_since_break: usize,
    // Synthesized Roadhouse: the (synthesized, seed) last applied, and the playing track's length.
    pub synth_music_applied: Option<(bool, i64)>,
    pub radio_track_len: Option<(String, f64)>,
    // How long the stations have been on the air this drive, in real seconds.
    // A station keeps broadcasting while the driver is listening to another
    // one (or to nothing at all), so tuning back in has to land where it got
    // to rather than restarting its running order.
    pub radio_airtime_s: f64,
    // Channel 3000's running order (None when the build has no schedule for
    // it), and the clip of it the cab last started.
    pub channel3000: Option<ff_core::channel3000::Schedule>,
    pub channel3000_serial: u64,
    // Personal playlist stations: where each playlist left off this drive,
    // and a hold between entries so neither a fade-in nor a stream still
    // connecting ever reads as a finished track.
    pub playlist_positions: HashMap<String, usize>,
    // With shuffle on, the order this lap plays each playlist's entries in
    // and how far along it is; rebuilt for the next lap when it runs out.
    pub playlist_shuffle: HashMap<String, PlaylistShuffleLap>,
    pub playlist_wait_s: f64,
    pub playlist_stream_tries: u32,
    pub playlist_stream_skips: u32,
    // Which playlists have already said they have nothing that will play,
    // so a broken folder explains itself once instead of every 30 seconds.
    pub playlist_silence_spoken: HashSet<String>,
    // Reception: signal re-checked on a slow cadence while driving so
    // ranged stations fade with distance and drop past their contour.
    pub radio_signal_timer: f64,
    // Re-tune cadence for a real stream that went silent (a dock bed
    // borrowed the music channel, or the connection stalled mid-drive).
    pub radio_reconnect_timer: f64,
    // The song the tuned stream last reported (ICY StreamTitle), polled on
    // the reception tick; None when nothing is streaming or the station
    // sends no song information. Shift+Y speaks it, the Tab radio screen
    // and the drivers board line carry it.
    pub radio_now_playing: Option<String>,
    // The radio draws power from the engine: last power state, so the
    // per-frame sync starts or stops playback only on the transition.
    pub radio_powered: bool,
    pub radio_signal_factor: f64,
    // FM fringe renderer state: cached signal/dial from the reception
    // tick, the hiss-bed flag, and the picket scheduler (its own seeded
    // rng -- flutter is cosmetic but must replay deterministically).
    pub radio_fringe_signal: Option<f64>,
    pub radio_fringe_freq: f64,
    pub fringe_bed_active: bool,
    pub radio_picket_duck: f64,
    pub picket_duck_s: f64,
    pub picket_wait_s: f64,
    pub fringe_rng: PyRandom,
    /// The first-run walkthrough, or a driving-school lesson; `None` once
    /// `tutorial_done`.
    pub tutorial: Option<Box<dyn Instructor>>,

    // ---- driving.py: hours, fatigue, hazards (driving_updates / driving_events) -------
    pub hos_fine_count: i64, // escalates with each failed inspection
    /// Most recent quarter-game-minute HOS-stop planning check. This cache is
    /// intentionally session-only; only a warning actually spoken is saved.
    pub hos_stop_check_key: Option<String>,
    pub hos_stop_warning_pending: Option<String>,
    pub hos_plan_hint_check_key: Option<String>,
    pub hos_plan_hint_pending: Option<String>,
    pub enforcement_events: HashSet<String>,
    pub out_of_service_count: i64,
    pub drowsy_said: bool,
    pub severe_said: bool,
    pub fatigue_cue_gm: f64, // game minutes since the last drowsy cue
    pub microsleep_deadline: Option<f64>, // reaction window, real seconds
    pub microsleep_gm: f64,  // game minutes since the last nod
    pub microsleep_cooldown_gm: f64,
    pub microsleep_misses: i64, // consecutive nods drifted off the road
    pub hazard_deadline: Option<f64>,
    // What is actually pending, in the order it armed. A second hazard
    // arming while one is still live folds its name in here instead of
    // clobbering the first (see _handle_trip_event's HAZARD branch), so
    // the resolution line can name everything that just cleared.
    pub hazard_names: Vec<String>,
    pub horn_scare_tried: bool,
    pub hazard_slow_hint_said: bool,
    pub automatic_braking_announced: bool,
    pub automatic_braking_escalated: bool,
    // The service application the hazard assist is holding (0.0 = off the
    // pedal), whether it has earned the emergency application on top, and
    // the measurements that decide it. Held rather than re-decided every
    // frame: the air system charges a whole brake application every time
    // the pedal rises, and the assist's own braking retreats the very
    // threshold that engaged it.
    pub aeb_brake: f64,
    pub aeb_emergency: bool,
    pub aeb_hold_s: f64,
    pub aeb_losing_s: f64,
    pub aeb_decel_mps2: f64,
    pub aeb_last_speed_mps: Option<f64>,
    pub last_event_message: String, // last spoken route announcement, for replay
    /// The newest CB call, for the Alt C repeat. Its own slot because the
    /// one above is overwritten by every landmark, lane count and weather
    /// change that follows, which is exactly the case a driver who missed
    /// the CB is in. See [`CbChatterRecall`].
    pub last_cb_chatter: Option<CbChatterRecall>,
    pub speed_announce_timer: f64,
    pub last_announced_mph: f64,
    // Compliance grace after a posted-limit drop: braking time before
    // strikes accrue, earned only while actually slowing.
    pub enforced_limit_prev: Option<f64>,
    pub limit_drop_grace_s: f64,
    // The demoted (ROUTE-queued) zone-entry line may lag its boundary;
    // the throttle-held grace collapse must not arm until it has had
    // time to actually be spoken (research doc, R1's coupled invariant).
    pub limit_drop_throttle_exempt_s: f64,
    // Dash overspeed alert: armed while over the limit, chiming on an
    // interval until the truck settles back under.
    pub overspeed_active: bool,
    pub overspeed_chime_timer: f64,
    // Congestion badges: both kinds of slow inside one trip earns a nod.
    pub construction_seen: bool,
    pub traffic_seen: bool,
    // Billboards that notice the drive: the record at the last pool sign,
    // and how many pool signs this trip has read.
    pub billboard_watch: crate::states::driving_events::billboard_moment::BillboardWatch,
    pub pool_billboards_heard: usize,
    pub brake_squeal_cooldown_s: f64, // hot-brake squeal cue spacing
    pub hydro_active: bool,           // spoken hydroplane warning edge tracking
    pub jake_slip_active: bool,       // spoken jake-slip warning edge tracking
    // The cylinder selector's position, like the real dash switch: J
    // engages at whatever stage was last chosen. Full retard by default
    // -- the setting every driver leaves it on until ice says otherwise.
    pub jake_selected_stage: i32,
    pub chains_fast_active: bool, // spoken chains-over-speed warning edge tracking
    /// The cruise and keeper set speeds held down to chain speed while the
    /// chains are on, given back when they come off (`apply_chain_speed_cap`).
    pub chain_cap_cruise_mph: Option<f64>,
    pub chain_cap_keeper_mph: Option<f64>,
    /// "Cruise held to 30 miles per hour for chains." is said once per set.
    pub chain_cap_said: bool,
    pub chain_law_warned: HashSet<(i64, i64)>, // (area, level) spoken warnings
    pub chain_law_cited: HashSet<(i64, i64)>,  // (area, level) already fined
    /// (area, level) -> whether its checkpoint is staffed, rolled once.
    pub chain_checkpoint_staffed: HashMap<(i64, i64), bool>,
    /// Cited with chains aboard: held on the apron until they are on.
    pub chain_checkpoint_hold: bool,
    /// Colorado chain-carry rule: warned once, and rolled once, per drive.
    pub chain_carry_warned: bool,
    pub chain_carry_rolled: bool,
    /// "A chain is slapping the fender" is said once per install.
    pub chain_slap_said: bool,
    /// The chains were tightened once since they went on.
    pub chain_tightened: bool,
    /// Stop planning's no-chains reminder, said once per corridor approach
    /// (`ChainCorridorAhead::key`). Not saved: a resume may repeat it once.
    pub chain_reminder_said: HashSet<String>,
    /// Facility stopping assistance's pending roadside chain stop.
    pub chain_stop: Option<crate::states::driving_events::chain_stop::ChainStop>,
    /// Its held brake application, one of the assists' pedal floors.
    pub chain_stop_brake: f64,
    /// Chain-law areas the chain-up stop has handled, declined or had
    /// cancelled: never retried.
    pub chain_stop_areas_done: HashSet<usize>,
    /// The last chain-law area each chain-off decision was made after.
    pub chain_off_done: HashSet<usize>,
    /// Route mile the bare road began, past the last chain-law area.
    pub chain_off_bare_from: Option<f64>,
    /// The slap tighten was offered once since the chains went on.
    pub chain_assist_tighten_tried: bool,
    /// A chain broke this drive and the assist hasn't answered it yet.
    pub chain_assist_snapped: bool,
    // Curve management: the curve the too-fast warning has been spoken for
    // (its start milepost; the ramp curve has its own id), `driving_rollover`.
    pub curve_warned_mi: Option<f64>,

    // ---- driving.py: enforcement counters and the live stop (driving_updates) ----------
    // Trooper pull-overs: a strike inside a patrol window may get you stopped
    // for an immediate ticket, separate from the silent at-delivery strikes.
    pub speeding_tickets: i64,
    pub ticket_fines_paid: f64,
    // No-engine-brake zones: town noise ordinances around route cities
    // (driving_engine_brake).
    pub jake_zone_fines: i64, // citations this trip; escalates the fine
    pub jake_fines_paid: f64,
    pub jake_violation_deadline_s: Option<f64>, // grace after the warning
    pub jake_citation_latched: bool,            // one citation per continuous engagement
    // Zones whose one warning has already been spent this trip; a second
    // engagement in the same town is cited without a fresh grace window.
    pub jake_zone_grace_used: HashSet<String>,
    pub jake_zone_warned_key: Option<String>, // approach callout latch
    pub assist_zone_cue_key: Option<String>,  // once-per-zone assist-release cue
    /// `None` | `PULL_OVER_LIGHTS` | `PULL_OVER_STOPPING`.
    pub pull_over: Option<String>,
    pub pull_over_start_mi: f64,
    pub pull_over_signaled: bool,
    pub pull_over_over: f64,  // mph over the limit when clocked
    pub pull_over_limit: f64, // posted limit when clocked
    pub pull_over_kind: String,
    pub pull_over_title: String,
    pub pull_over_summary: String,
    pub pull_over_fine: f64,
    pub pull_over_reputation_hit: f64,
    pub pull_over_return: String,
    // Whether the violation happened inside roadwork, which doubles the fine.
    pub pull_over_construction_zone: bool,
    pub pull_over_warning_level: i64,
    pub failure_to_stop_count: i64,
    // Real seconds owed to the player before the stop is judged at all:
    // the spoken instruction plus reaction time. Nothing drains until it
    // has run out.
    pub pull_over_grace_s: f64,
    // How long the stop has stayed unresolved past the final warning.
    pub pull_over_forced_s: f64,
    // Real seconds the truck has held speed, unbraked, past the final
    // warning: the road to a pursuit. Any brake zeroes it.
    pub pull_over_run_s: f64,
    // Ladder movement spoken during this trip, restated once at the
    // delivery summary so nothing about your standing is heard only
    // on a road the player has already left.
    pub record_events: Vec<String>,
    pub fatigue_events: i64, // run-off-road microsleeps this trip
    // Every open scale whose notice has been spoken this trip, by key.
    pub weigh_station_noticed: HashSet<String>,
    // The half-mile "slow for the scale" nudge, latched separately so it
    // speaks once per announced scale and never re-fires on a re-approach.
    pub weigh_station_reminder_key: String,
    // Real driving seconds since that reminder was spoken, counting only
    // frames the cab was free (no hazard, pull-over, ramp or menu). A
    // crossing before SCALE_REMINDER_REAL_LEAD_S of them is not judged a
    // bypass, but only when the game itself held that reminder back.
    pub weigh_station_reminder_age_s: f64,
    // Open scales whose last reminder the game held back: the notice only
    // latched inside the reminder window, or the cab was taken (a stop, a
    // ramp, a hazard, a departure lane) while the truck was inside it,
    // before or after the reminder spoke. A reminder made late by the
    // driver's own crawl or signal is not here.
    pub scale_reminder_held_by_game: HashSet<String>,
    // Open scales whose reminder the driver held quiet inside its window,
    // under the bypass speed or signalled for the ramp. A cab taken after
    // such a reminder finally speaks does not excuse the crossing.
    pub scale_reminder_late_by_driver: HashSet<String>,
    // A stop or a pause just ended: re-announce the open scale still ahead
    // once the cab is free. Holds the name of a scale just checked in at,
    // or an empty string when there is none.
    pub scale_reannounce: Option<String>,
    // A scale crossed with its own exit armed: judged after the exit watch
    // runs, later in the same frame, never on ramp speed alone.
    pub weigh_station_pending: Option<RoadStop>,
    // Weigh-in-motion verdicts for a transponder-equipped truck, keyed by
    // _weigh_station_key(stop): "green" or "red", rolled once at the
    // notice point in EnforcementWatchMixin._resolve_transponder_verdict
    // and read both by the bypass judgment and by the half-mile reminder.
    pub weigh_station_transponder_verdict: HashMap<String, String>,
    // Last retarder stage written to the transcript, so _trace_engine_brake
    // logs a CHANGE rather than a frame. -1 rather than 0: the first
    // observation is worth a line even if the jake starts down.
    pub traced_jake_stage: i32,
    pub unsafe_damage_stop_key: String,
    // Compliance tracker for the active stop: 0..1, judged from behavior
    // (signaling and slowing), not distance. Reset on every stop-ending path.
    pub pull_over_compliance: f64,    // seeded on begin
    pub pull_over_elapsed: f64,       // s since the lights came on (signal grace)
    pub pull_over_prev_mph: f64,      // last-tick speed, to classify accel vs decel
    pub pull_over_coast_s: f64,       // consecutive s with no braking and no accel
    pub pull_over_signal_boost: bool, // the one-time signal bump has fired
    pub pull_over_nosignal_hit: bool, // the one-time no-signal 1/4 hit has fired
    // The road-joint spacer used to draw from the shared patrol stream
    // about a hundred times a mile, so whether a trooper caught you came
    // down to how many joints had played -- frame timing, effectively, and
    // a reload re-rolled it. Ambience keeps its own stream, and there is
    // no longer a shared enforcement stream at all: every police decision
    // is a named, position-quantised seed (see sim/enforcement_posts.
    // post_seed), so nothing the audio layer does can consume it.
    pub road_texture_rng: PyRandom,

    // ---- driving_enforcement.py: `_enforcement_init` ---------------------------------
    pub siren: SirenLoop,
    // Continuous distance over the limit. This -- not a real-time hold --
    // is what a post reads as a speed. See _update_enforcement_watch.
    pub over_limit_mi: f64,
    // Continuous distance closed up on the vehicle ahead; same idea, and
    // read by the same kind of hold. See _accrue_following_gap.
    pub closed_up_mi: f64,
    pub enforcement_prev_mi: f64,
    pub passed_post_ids: HashSet<String>,
    pub marked_post_ids: HashSet<String>,
    // A tableau's two cues, each fired once: the siren some seconds
    // before the spot, and the stopped pair as you go by it.
    pub tableau_siren_ids: HashSet<String>,
    pub tableau_pass_ids: HashSet<String>,
    pub scale_bed_key: String,
    pub scale_bed_volume: f64,
    pub radio_cue_duck: f64,
    pub radio_cue_duck_s: f64,
    pub radio_cut_for_stop: bool,
    pub pending_sounds: Vec<PendingSound>,
    // Posts whose look was deferred because the cab was busy. They keep
    // their turn; nothing is silently dropped.
    pub deferred_post_ids: HashSet<String>,
    // The look itself, and the mile it was taken at: what makes the
    // deferral a postponement rather than a discard.
    pub held_observation: Option<(Observation, f64)>,
    // Miles each pacing unit has held station behind the truck.
    pub pacing_mi: HashMap<String, f64>,
    // Reused buffer for per-frame post-id lookups into the sets above.
    pub post_id_scratch: String,
    pub rescue_offered: bool,

    // ---- driving_damage.py --------------------------------------------------------------
    // Damage bands. The band itself is derived from damage_pct every
    // frame; what is trip-scoped is the last band ANNOUNCED (so an edge
    // speaks once) and the limp cap mid-wind-down. Both round-trip through
    // the snapshot -- a resume that re-announced limp mode, or snapped the
    // cap back to full speed, would be lying about the truck.
    pub damage_band: i32,
    // The deepest band this run reached, for settlement to grade against.
    pub worst_damage_band: i32,
    // This run already put one preventable-equipment event on the record.
    pub equipment_event_logged: bool,
    pub limp_cap_mph: Option<f64>,
    pub limp_cruise_said: bool,
    pub out_of_service_creep_s: f64,
    pub recovering: bool,
    // Warning state for tires, brakes, and engine. New drives start clear so
    // existing wear is announced once; resumed drives derive these levels
    // from the saved wear and do not repeat an acknowledged warning.
    pub maintenance_levels: [u8; 3],
    pub maintenance_pending_levels: [u8; 3],
    // The highest cargo-condition rung already spoken, so each one warns
    // once. Trip-scoped and snapshotted like the damage band.
    pub cargo_cue_at: f64,
    // The coaching tail ("brake and corner gently from here") is teaching,
    // not news: it belongs on the first load-damage report of the episode,
    // and every escalation after it speaks only the new number and the
    // consequence (research doc R11). Snapshotted so a resume mid-episode
    // does not re-teach it.
    pub cargo_coaching_said: bool,

    // ---- driving_events.py: exits, ramps, the destination ---------------------------------
    pub signal_timer: f64,
    pub exit_stop: Option<RoadStop>, // active route exit
    // Stable proof that the player explicitly selected an optional rest
    // stop with T. _exit_stop is not enough: destination approaches infer
    // it automatically, and a canceled signal can leave it populated.
    pub selected_stop_key: Option<String>,
    /// Whether T selected this stop for the next 30-minute HOS break.
    pub selected_stop_break: bool,
    /// What the driver told T this stop is for; `None` for a stop the
    /// co-driver chose.
    pub selected_stop_purpose: Option<crate::states::driving_events::stop_plan::StopPlanPurpose>,
    pub selected_stop_assist_armed: bool,
    pub selected_stop_assist_said: bool,
    pub selected_stop_assist_brake: f64,
    pub exit_signal_on: bool,
    pub exit_signal_canceled: bool,
    pub canceled_exit_key: Option<String>, // do not rediscover a deliberately canceled exit
    /// The truck is in the exit lane: steered into it where it opened at the
    /// taper, or -- with lane keeping on full -- handed to lane keeping, which
    /// takes it at the gore. See `update_exit_preparation`.
    pub exit_lane_entered: bool,
    /// "Exit lane opening." has been said for this approach: the lane is open
    /// from here to the end of the gore window.
    pub exit_taper_said: bool,
    pub exit_cancel_armed: bool,
    pub exit_right_hold_s: f64,
    pub exit_right_taps: i64,
    pub exit_tap_hint_said: bool,
    /// `set[float]` of the `EXIT_COUNTDOWN_MILESTONES_MI` already spoken.
    pub exit_countdown_said: Vec<f64>,
    pub ramp_mi: Option<f64>, // ramp distance left, once taken
    /// The ramp taken at the gore, piece by piece: the deceleration lane, the
    /// curve, the run to the stop bar. See `driving_events/decel_lane.rs`.
    pub ramp_layout: Option<ExitRampLayout>,
    /// The exit assists' held application in the deceleration lane, a pedal
    /// floor like the terminal servo's (`ramp_assist_brake`).
    pub decel_lane_brake: f64,
    pub decel_lane_assist_said: bool,
    pub ramp_stop: Option<RoadStop>,
    pub ramp_end_said: bool,
    pub ramp_arrival_grace_s: f64,
    // How many times this drive has been carried past the destination
    // terminal at speed; a repeat miss earns the braking hint.
    pub ramp_terminal_miss_count: i64,
    // Ramp terminal control for the active ramp: what meets you where the
    // ramp joins the surface road, and the light's cycle state if a signal.
    // "signal" | "stop" | "yield" | "roundabout" | "none" | "" (no ramp)
    pub ramp_control: String,
    pub ramp_light_profile: u8, // seeded fixed timing plan for this terminal
    pub ramp_light_offset_s: f64, // seeded phase into the light cycle
    pub ramp_light_timer: f64,  // real seconds since the ramp was taken
    pub ramp_light_announced: bool,
    pub ramp_light_last_phase: String, // "red" | "yellow" | "green", once announced
    pub ramp_terminal_done: bool,
    pub ramp_waiting_at_light: bool,
    // Stopped at the sign with cross traffic rolling: the clear call is
    // waiting on the bubble's gap.
    pub ramp_waiting_at_sign: bool,
    // The living crossroad at a controlled terminal (sim.cross_traffic);
    // built per ramp by _begin_ramp_terminal, None between ramps.
    pub cross_bubble: Option<CrossTraffic>,
    pub ramp_creep_prompt_said: bool,
    // The gap to the bar the last "Stopped N short" line named.
    pub ramp_creep_prompt_gap_mi: f64,
    pub ramp_gap_milestones_said: HashSet<i64>,
    pub ramp_bar_tick_timer: f64,
    pub bar_solid_on: bool, // the bar's continuous final-zone tone
    pub ramp_assist_said: bool,
    pub ramp_green_roll_said: bool, // "slowing for the green" spoken this terminal
    // The pedal route-transition assistance is currently holding for the
    // terminal, so it can follow the demand up without letting go and
    // re-making the application every few frames.
    pub ramp_assist_brake: f64,
    // Facility stopping assistance is driving the truck on from the honored
    // terminal -- to the entrance on a plain ramp, to the streets on a chain
    // -- so nobody is asked to pull ahead (owner ruling, 2026-09-01). Set
    // where the terminal releases; the driver's own brake cancels it, and a
    // cancel holds for the rest of this ramp.
    pub approach_pull_ahead: bool,
    pub approach_pull_ahead_canceled: bool,
    // A street control on a facility chain, played through the ramp
    // terminal's own state above (`driving_events/street_controls.rs`): the
    // route mile of its stop bar while it is live, its baked kind, and the
    // bars this trip has already played, by trip generation.
    pub street_bar_mi: Option<f64>,
    pub street_control_kind: String,
    pub street_controls_played: HashSet<i64>,
    pub street_controls_trip: u64,
    // The live street signal's (red, green) seconds; None at a ramp end,
    // which keeps its own timing profile. It cycles on the trip's clock.
    pub street_light_split: Option<(f64, f64)>,
    // Whether the streets play their lights and signs at all
    // (`street_controls::STREET_CONTROLS_IN_PLAY`, off for 1.9).
    pub street_controls_on: bool,
    // Safety-call re-arm window (curve calls vs the Ctrl reflex).
    pub critical_curve: Option<RouteCurve>,
    pub critical_call_age_s: f64,
    pub critical_respeak_at: Option<f64>,
    pub destination_exit_taken: bool,
    // True only while the destination approach assist is actually shedding
    // for the arrival point. The zone keeper refuses to re-engage into it,
    // the way it already refuses on a ramp -- without that the keeper held
    // a street limit straight through the market gate.
    pub destination_arrival_active: bool,
    pub destination_assist_brake: f64,
    pub destination_chain_ahead: Option<bool>, // memo, see _destination_street_chain_ahead
    pub missed_destination_exit_said: bool,
    pub destination_exit_announced_key: String,
    pub destination_exit_response_s: f64,
    // Surface chain: after the destination ramp, the drive continues on
    // the facility's real street chain to the gate instead of a scripted
    // arrival. The highway trip is kept for records; the active trip
    // becomes the surface route.
    pub surface_chain: bool,
    // The road stop whose streets from its exit ramp to its lot are the trip
    // right now (`begin_stop_chain`); the highway waits in `highway_trip`.
    pub stop_chain: Option<RoadStop>,
    pub stop_chain_end_said: bool,
    // Whether a road stop's streets are driven at all
    // (`chains::STOP_STREETS_IN_PLAY`, off for 1.9).
    pub stop_streets_on: bool,
    // Whether this drive takes the facility streets at all: the Facility
    // streets setting as the drive started (a save carries its own), so a
    // change in Settings applies from the next drive and a resumed chain
    // rebuilds as it was driven.
    pub facility_streets_on: bool,
    pub highway_trip: Option<Trip>,
    // Departure chain: the mirror. A loaded run out of a chain-capable
    // origin facility starts on its streets and merges onto the highway.
    // Checked lazily on the first drive tick so restored mid-highway
    // saves are never pulled back onto the streets.
    pub departure_chain: bool,
    pub departure_checked: bool,
    // -1 so the first tick always resets, whatever leg a resumed
    // run starts on.
    pub ladder_leg_index: i64,
    // (position when computed, scan result) -- see _destination_exit_details
    pub destination_exit_cache: Option<DestinationExitScan>,
    /// Whether this trip's route has a labeled destination exit at all, keyed
    /// by `trip_generation`: only a route without one may fall back to the
    /// estimated exit before the end.
    pub destination_exit_labeled: Option<(u64, bool)>,

    // ---- driving_events.py / driving_speed_control.py: cruise and the keeper -----------
    pub cruise_mph: Option<f64>,
    // The rate-limited speed cruise is actually chasing this frame; it eases
    // from the engage speed up to _cruise_mph so a big resume error never
    // lands on the pedal at once. None whenever cruise is off.
    pub cruise_working_mph: Option<f64>,
    // What cruise ACTUALLY held this frame, after every cap, and the phrase
    // for why -- published by the loop so the status keys can answer with the
    // number the truck is on rather than the number the dial is set to. The
    // reason is empty when nothing named it (the weather cap, or nothing
    // capping at all). None whenever cruise is off, and until the loop has
    // run once in a fresh session.
    pub cruise_held_mph: Option<f64>,
    pub cruise_held_reason: String,
    pub cruise_throttle: f64,
    pub cruise_applied: f64,
    pub cruise_trim: f64,       // integral trim on top of the grade feed-forward
    pub cruise_jake_stage: i32, // retarder stages cruise itself commanded
    pub cruise_jake_cooldown_s: f64, // quiet time between those stage steps
    pub cruise_jake_reverse_s: f64, // ...and the longer one before a step back
    pub cruise_jake_last_step: i32, // +1 raised, -1 lowered, 0 released
    pub cruise_snubbing: bool,  // a service-brake snub is in progress
    pub pcc_phase: String,      // what the grade preview is doing, for the cue
    pub climb_cue_said: bool,   // cruise has already owned up to this pull
    pub climb_cue_s: f64,       // quiet time between hand-back cues
    pub climb_beaten_s: f64,    // how long the grade has genuinely been winning
    pub descent_said_mph: Option<f64>, // the descent-control number last spoken
    // The safe descent speed of the hill under and just ahead of the truck
    // (`refresh_descent_safe`), and the grade it was worked out for.
    pub descent_safe_mph: Option<f64>,
    pub descent_safe_key: Option<i64>,
    pub descent_posted_cap_mph: Option<f64>, // the posted cap cruise keeps here
    // A trailer refused at the shipper: the yard swapped it, so the box
    // under the truck is sound and no scale house should say otherwise.
    pub trailer_refused: bool,
    // The hooked trailer's defect was fixed on the shoulder after an
    // inspector parked the truck for it: no later inspection finds it again.
    pub trailer_repaired: bool,
    // The hooked trailer's visible defect for the enforcement watch, read
    // from the pickup plan once per half mile rather than every frame:
    // (defect or empty, mile it was read at).
    pub visible_trailer_defect: (String, f64),
    // Equipment faults already on an inspection report or citation this
    // drive (the finding as the officer read it). The driver carries that
    // paper to the next shop, so no later post writes the same fault again.
    pub written_up: HashSet<String>,
    pub nice_speed_mi: f64,    // distance held at a very particular speed
    pub double_nickel_mi: f64, // and at the limit Congress retired in 1995
    pub jake_descent_mi: f64,  // downgrade held on the engine alone
    pub radio_states_station: String, // station the state tally belongs to
    pub radio_states_held: HashSet<String>,
    pub cruise_descent_mph: Option<f64>, // interactive descent ceiling, while it lasts
    pub cruise_exit_mph: Option<f64>,
    pub cruise_curve_mph: Option<f64>,
    pub cruise_curve_end_mi: Option<f64>,
    // A bend too tight for cruise to ease into PAUSES the session rather
    // than disarming it (owner ruling, 2026-09-01: on the highway a hazard
    // or a curve pauses adaptive cruise and it comes back). This is the mile
    // the pause lifts at -- the bend's end (the chain's, for a linked pair)
    // plus the commit tail -- and the resume path holds off until the truck
    // is past it. Cleared by any disarm.
    pub cruise_resume_after_mi: Option<f64>,
    // Sign of the steep grade already announced, so each hill is called out
    // once rather than every frame, and where the road profile was last read.
    pub grade_warned_sign: i32,
    pub grade_scan_mi: f64,
    // K arms one continuous speed-control session. The active controller
    // changes between adaptive cruise on open roads and the speed keeper in
    // restricted zones, while this target remembers what cruise should
    // resume at after the zone ends.
    pub speed_control_armed: bool,
    // Set while an armed session is held at a planned stop. An ARRIVAL --
    // a pickup or delivery gate, a planned rest stop -- is remembered but
    // must not re-engage on its own: it resumes when the player departs,
    // or when they arm it again by hand. A TRANSIT stop -- the light or
    // sign at the end of a ramp -- has no departure to wait for, so its
    // pause lifts itself once the stop has been honored and the truck is
    // rolling again.
    pub speed_control_paused_at_stop: bool,
    pub speed_control_transit_pause: bool,
    pub speed_control_stop_honored: bool,
    pub speed_control_target_mph: Option<f64>,
    pub acc_following: bool,
    pub acc_weather_gap_said: bool,
    pub acc_limit_capped: bool,
    pub acc_limit_cap_said: Option<f64>,
    pub acc_weather_cap_said: Option<f64>,
    // (end mile, limit, reason) of a restricted zone cruise has begun
    // slowing for -- a work zone or heavy traffic.
    pub construction_slowdown: Option<(f64, f64, String)>,
    /// `(barrels mile, taper limit)` while cruise is still on the merge
    /// taper's number ahead of that zone; cleared the moment the zone's own
    /// number takes over, so the target never climbs back as the braking
    /// window shrinks.
    pub construction_taper_stage: Option<(f64, f64)>,
    /// A lower posted limit adaptive cruise is already easing for:
    /// `(start_mi, limit_mph, reason)`, held until the truck reaches it.
    /// The lookahead is a braking distance that shrinks as cruise slows,
    /// so without this the drop slipped out of the window, the cap lifted,
    /// the truck wound back up, and the drop came back -- four brake-and-
    /// surge cycles and four "Posted limit lower" lines on one approach
    /// (agent drive, RI-146, 2026-09-02). The construction hold above is
    /// the same shape for the same reason.
    pub acc_limit_hold: Option<(f64, f64, Option<String>)>,
    pub acc_follow_cue_s: f64, // quiet window between "Traffic ahead" cues
    pub descent_control_active: bool,
    pub descent_limit_state: String,
    // How long the truck has been over what descent control works to AND
    // still gaining speed with everything applied. The descent twin of
    // climb_beaten_s: one frame of either is a grade boundary, not a runaway.
    pub descent_beaten_s: f64,
    pub descent_capture_active: bool,
    pub assist_exit_slowing_said: bool,
    pub curve_assist_active: bool,
    pub curve_assist_cue_s: f64,
    // Whether the SLOWING cue was actually spoken for the run in progress.
    // The release line is paired to it, so an engagement that stayed quiet
    // (see the ramp case in driving_updates) never leaves a lone
    // "released" hanging with nothing before it.
    pub curve_assist_spoke: bool,
    // The proactive half of curve speed assistance: armed by the curve call,
    // it brakes the truck to the chain's tightest advisory on the approach
    // and holds it through the bend (see driving_updates::curve_servo). The
    // fields above are the reactive half, inside the bend.
    pub curve_servo: Option<CurveServo>,
    // Where the servo the driver's own brake cancelled would have let go:
    // the bends up to here are theirs, so its lookahead leaves them alone.
    pub curve_servo_declined_to_mi: Option<f64>,
    pub transition_assist_active: bool,
    pub keeper_mph: Option<f64>,
    pub keeper_throttle: f64,
    pub keeper_zone: String,
    // With the speed keeper off, an open-road resume below cruise's holding
    // speed has nothing to bridge it, so the cab says once what the session
    // is waiting for. Reset when cruise engages or the session disarms.
    pub speed_control_floor_said: bool,
    // The posted number the keeper last took its target from, so a street
    // that posts a HIGHER one can hand the keeper back up to street speed.
    pub keeper_zone_limit: Option<f64>,
    // The lower number ahead the keeper is easing for, and the say-once
    // latch for the posted-limit version of that cue.
    pub keeper_ease_said: Option<f64>,
    pub keeper_ease_target: Option<(f64, f64, String)>,
    // What the keeper is holding RIGHT NOW and why, published by the loop
    // for the status keys (the cruise loop's `cruise_held_*` pair, for the
    // other controller). The reason is empty unless a lead vehicle set the
    // number; None until the loop has run.
    pub keeper_held_mph: Option<f64>,
    pub keeper_held_reason: String,
    // The service-brake snub the keeper is holding (0.0 = off the pedal),
    // and how long it has been out of authority with the truck still over
    // the number -- past which it owns up rather than riding it out.
    pub keeper_snub: f64,
    /// The keeper is braking a late corner down to its number
    /// (`keeper_corner_brakes`); the held snub stays on until it is there.
    pub keeper_corner_braking: bool,
    /// The number the held snub is bringing the truck down to -- the
    /// keeper's WORKING target, which on an approach is the corner or zone
    /// ahead and not the set speed. The release rule reads this.
    pub keeper_snub_target_mph: f64,
    // And the mirror of that on the other side of the number: how long the
    // keeper has been flat out and still losing the grade, plus the
    // say-once latch and the per-hill cooldown for owning up to it.
    pub keeper_droop_s: f64,
    pub keeper_droop_said: bool,
    pub keeper_droop_cue_s: f64,
    pub keeper_overrun_s: f64,
    pub keeper_overrun_said: bool,

    // ---- driving_events.py / driving_pickup.py / driving_facility_gate.py: arrival ----
    pub arrival_stop_said: bool,
    pub arrival_full_stop_said: bool,
    pub arrival_menu_open: bool,
    pub gate_reminder_s: f64,
    // Facility gate misses (driving_facility_gate.py): the pre-gate speed
    // warning latch, the real-time reaction window it opens, and how many
    // loop-backs this trip has cost -- the count escalates the spoken help.
    pub gate_speed_warned: bool,
    /// The pre-gate warning has spoken on this approach. Obeying it clears
    /// `gate_speed_warned` so the gate opens a fresh window at contact; this
    /// stays set so the line itself is never repeated. A miss resets both.
    pub gate_warning_spoken: bool,
    pub gate_grace_s: f64,
    pub gate_miss_count: i64,

    // ---- driving_wrong_way.py ----------------------------------------------------------
    // Backing down a live road: route miles given up since reverse was
    // engaged somewhere it has no business being, and the distance the
    // last warning was spoken at. Per-stint, not saved -- a reloaded trip
    // is not still reversing.
    pub wrong_way_mi: f64,
    pub wrong_way_said_at: f64,

    // ---- driving_traffic_sounds.rs ------------------------------------------------------
    // Which vehicle each traffic sound is following (`main:<key>` or
    // `cross:<id>`), the freeway bed's smoothed level, and how far the truck
    // has rolled down the current exit ramp. Per-stint, not saved.
    pub traffic_sounds: [Option<String>; 3],
    pub traffic_bed_volume: f64,
    pub traffic_ramp_rolled_ft: f64,

    // ---- driving_lane_gap.py: `_reset_lane_gap` ----------------------------------------
    // The lane a completed change moved out of, and the vehicle that was
    // holding it. Per-stint, not saved: a reloaded trip is not mid-pass.
    pub lane_gap_watch: Option<i64>,
    pub lane_gap_prev_lane: Option<i64>,
    pub lane_gap_blocker_key: Option<String>,
    pub lane_gap_blocker_class: String,
    pub lane_gap_said_keys: HashSet<String>,
    pub lane_gap_cue_s: f64,

    // ---- driving_turns.py: `_reset_turn_state_for_trip` -------------------------------
    // Street corners: how many corners this run has missed. The per-corner
    // latches are rebuilt whenever the
    // trip's route changes (`turn_trip_id` vs `trip_generation`).
    pub turn_miss_count: i64,
    pub turn_trip_id: u64,
    pub turn_advised: HashSet<String>,
    pub turn_resolved: HashSet<String>,
    /// Corners whose chime has sounded. It goes out once, with the first
    /// words that name the corner, so a driver who was told nothing about a
    /// corner is not chimed at for it either (owner, 2026-09-20).
    pub turn_announced: HashSet<String>,
    /// Corners whose own call already said "now": the route's call at the
    /// corner would only say the turn again.
    pub turn_called_now: HashSet<String>,
    pub turn_grace_s: f64,

    // ---- driving.py: air, brakes, engine (driving_updates / driving_controls) ----------
    pub air_ready_said: bool,
    pub low_air_said: bool,
    /// Once-per-threshold low-fuel cue, latched until a refill (`LOW_FUEL_WARN_FRACTION`).
    pub low_fuel_said: bool,
    /// The fuel range fell short of the next fuel stop; latched until it clears.
    pub fuel_range_short_said: bool,
    pub spring_brake_said: bool,
    pub brake_lockout_cue_timer: f64,
    pub brake_air_hissed: bool, // rising-edge guard for the brake-apply hiss
    pub pending_low_air_buzzer: bool, // cold-start buzzer, held past the crank
    pub brake_peak_application: f64, // hardest press this application, shapes the release
    pub overrev_s: f64,         // continuous seconds at damaging RPM
    pub overrev_warn_due: f64,  // repeats push it out further

    // ---- driving.py: lanes, merges, hazards, ambient queue (driving_updates / events) ---
    // Discrete lanes: tap-change progress (assist off), closed-lane
    // policing, hazard-dodge context, and keep-right pressure.
    pub lane_change_target: Option<i64>,
    /// Full lane keeping: the side the left stick last asked to change
    /// lanes toward, until it comes back near the center.
    pub stick_lane_request: i8,
    pub lane_change_timer: f64,
    pub lane_signal_timer: f64,
    /// Lane keeping on full is passing a slow vehicle: (the lane to come
    /// back to, the lane it passed into). See `pass_for_hazard`.
    pub passing: Option<(i64, i64)>,
    // A pass around something sitting in the lane (debris, a car on the
    // shoulder, a closure) holds the move back until the truck is past this
    // point, so it does not drop straight back onto what it went around.
    pub pass_clear_mi: Option<f64>,
    /// Lane keeping is moving back after a pass: the landing line is the
    /// tail of going around traffic, not a lane change the driver made.
    pub pass_returning: bool,
    /// Lane keeping chose the change underway, so it keeps watching the lane
    /// it is moving into and calls the move off if that lane fills or ends.
    pub keeping_lane_change: bool,
    /// A cruise ceiling while the move back from a pass is held by a vehicle
    /// pacing the truck in the right lane: lane keeping eases off so it can
    /// drop in behind it. None whenever nothing is holding the move back.
    pub pass_return_ease_mph: Option<f64>,
    /// Seconds the left lane stays excused after lane keeping's move back
    /// was called off or held by traffic, so the CB does not blame the
    /// driver for a hold the assist made.
    pub keep_right_excused_s: f64,
    pub merge_deadline: Option<f64>,
    // Miles of acceleration lane still ahead after pulling out of a
    // facility. None once the lane is behind the truck (or when the run
    // never started at a facility at all).
    pub departure_ramp_mi: Option<f64>,
    /// The posted speed of the road the departure ramp feeds, read when the
    /// acceleration lane is armed. The closing line at the taper is about
    /// the traffic the truck has to merge WITH, which is this road -- not
    /// whatever zone happens to sit under the wheels at the taper.
    pub departure_merge_road_mph: f64,
    // Cruise takes the acceleration lane at this traffic-relative speed only
    // when this exact truck's predicted drivetrain/load/grade capability can
    // reach it. Otherwise the keeper stays flat out through the taper.
    pub departure_cruise_handoff_mph: Option<f64>,
    // The real pavement has ended but this truck is still materially slower
    // than the traffic it joined. Keep the low-speed merge handoff on the
    // real-time clock until it can safely become ordinary highway driving.
    pub departure_merge_recovery: bool,
    /// A waiting pace change, the merge handoff's bound and the clock
    /// trace (`driving_updates::pacing`).
    pub clock_pacing: crate::states::driving_updates::pacing::ClockPacing,
    // Lanes on our side last tick, so a road that narrows under the truck
    // can be told apart from a driver who steered into the cones.
    pub lane_count_seen: Option<i64>,
    // The truck's own lane index the instant before this frame's
    // set_lane_count clamp ran, so a narrowing road that actually moved
    // the truck can be told apart from one that merely renumbered a lane
    // the truck was never in.
    pub lane_before_narrow: Option<i64>,
    pub merge_taper_warned: Option<String>,
    // A lane change answers the live hazard: it sits in one lane AND this
    // road has an open lane on this side. Both halves -- see
    // TripEventData::dodgeable. NOT "what kind of thing is it".
    pub hazard_dodgeable: bool,
    // The live hazard occupies our lane and no other, whether or not there
    // is anywhere to go. What braking ALONE must reach turns on this, not on
    // hazard_dodgeable: see hazard_target_mph.
    pub hazard_in_lane: bool,
    // Every hazard in the live group is an animal on the road: the
    // automatic braking leaves it alone, and the deadline settles whether
    // the truck hits it. See `animal_outcome`.
    pub hazard_animal: bool,
    // Where the truck was when the live hazard group was first called: the
    // animal outcome is seeded on it, so a reload hears the same outcome
    // however the driver answered.
    pub hazard_arm_mi: f64,
    // See HazardShape::brake_to_mph.
    pub hazard_brake_to_mph: Option<f64>,
    // The speed of the VEHICLE this hazard is about, when it is a vehicle.
    // None for debris, animals and anything else that is not going
    // anywhere. See hazard_target_mph.
    pub hazard_lead_mph: Option<f64>,
    pub hazard_lane: i64,
    pub left_lane_s: f64,
    pub keep_right_nags: i64,
    pub ambient_event_cooldown_s: f64,
    // Ambient lines waiting for the road to go quiet. A FIFO, not the
    // one slot it used to be: see _speak_ambient_event.
    pub pending_ambient_events: VecDeque<PendingAmbient>,
    pub road_joint_accumulator_m: f64,
    pub next_joint_distance_m: f64,
    pub lane_guidance: LaneGuidance,
    pub edge_loop_key: Option<String>, // active edge-ladder rung loop
    /// Last lane-position pan set on the road bed; `None` until this drive
    /// has written one, so its first frame always writes.
    pub road_pan_applied: Option<f64>,
    // The opt-in guide tone: whether its loop is running, and where it
    // is panned. Separate from the bed's tracker so switching the
    // setting mid-drive cannot leave either one stuck off center.
    pub lane_guide_tone_on: bool,
    pub lane_guide_pan_applied: f64,
    /// The engine's lean through a turn: how much wheel the driver still owes.
    pub turn_guide: TurnGuide,
    /// Last pan written to the ENGINE for the steering guide.
    ///
    /// Its own field, not shared with `lane_guide_pan_applied`: that one
    /// belongs to the opt-in guide TONE, and while both wrote it each frame
    /// they clobbered the other's tracker and both channels re-panned forever.
    ///
    /// `None` until this drive has written one. The backend keeps the engine's
    /// pan across stops and across drives, so a tracker that started at 0.0
    /// claimed a centre nobody had set: a drive that ended leaning left the
    /// next one's engine panned down a straight road, on the channel that
    /// means "steer this way" (review I7, 2026-09-19).
    pub engine_guide_pan_applied: Option<f64>,
    // Dead-man's-curve strips: fixed road furniture ahead of each hairpin.
    pub transverse_strip_miles: Vec<f64>,
    /// `set[float]` of the strips already played.
    pub transverse_fired: Vec<f64>,
    pub lane_locator_on: bool, // I toggles the panned position tock
    pub lane_locator_timer: f64,
    /// The driver window and the climate controls: pure ambience, not saved.
    pub cab: ff_core::sim::cab::CabState,
    // The same tock, summoned by the steering itself rather than by I:
    // while a lane move is underway it runs on its own and ends with the
    // signal cancelling. See _update_steering_lane_cue.
    pub steer_cue_active: bool,
    pub exit_blinker_active: bool, // retain ownership until a held wheel is released
    pub steer_cue_timer: f64,
    pub steer_cue_hold_s: f64, // how long the wheel has been held this way
    pub curve_run: Option<CurveRun>, // the bend underway, and how it is going
    pub cross_repeat_s: f64,   // rapid re-crossings keep only the quiet thump
    pub sideswipe_cooldown_s: f64, // one contact, however many crossings
    // Off-pavement is a standing condition: the panned edge-rumble loop
    // carries "where am I right now", and speech fires only at transitions
    // -- left the pavement, worse, back on it (research doc R12). None means
    // on the pavement; otherwise the last-spoken severity band.
    pub road_position_band: Option<i32>,
    pub reverse_cue_active: bool,
    pub air_cue_active: bool, // compressor fill loop below governor release
    pub jake_cue_key: Option<String>, // jake growl loop currently playing
    pub jake_cue_idle_s: f64, // seconds that loop has been held silent
    pub curve_assist_jake: bool, // jake engaged BY the assist (not the player)
    pub auto_jake: bool,      // automatic-box retarder management (J on an AMT)
    pub auto_jake_enabled: bool, // Alt+J: whether J arms auto mode on an AMT
    pub resume_target_mph: Option<f64>, // Shift+K brings this speed back
    pub auto_jake_hold_mph: Option<f64>, // speed auto mode holds
    pub auto_jake_cooldown_s: f64, // rate limit between stage steps
    pub auto_jake_reverse_s: f64, // ...and the longer one before a step back
    pub auto_jake_last_step: i32, // +1 raised, -1 lowered, 0 released
    pub shift_recover_t: f64, // 0->1 recovery progress after an automatic shift ends
    pub shift_hold_rpm: Option<f64>, // engine voice held here through a shift
    pub manual_engage_clunk_pending: bool, // a manual shift's second clunk, owed at engagement
    // Smooth only the audible engine load. Physics keeps the raw throttle,
    // while small controller and cruise changes blend into the engine bed.
    pub engine_audio_throttle: f64,
    // Prev-frame accel/brake state, so a forward<->reverse shift needs a
    // fresh press (release then press) rather than a held control.
    pub reverse_brake_held: bool,
    pub reverse_accel_held: bool,
    // Direction-change gesture state: which change a fresh standstill
    // press has armed, and how long the control has been held since.
    // The gear engages only past DIRECTION_CHANGE_HOLD_S.
    pub direction_armed: String,
    pub direction_hold_s: f64,
    // Latching brake (double-tap-and-hold, see sim/pedal_latch.rs):
    // a latched brake reads as held everywhere downstream. The throttle
    // key never latches.
    pub brake_latch: PedalLatch,
    // Curve calls already made this trip (keys are curve entry miles).
    pub pacenote_spoken: HashSet<i64>,
    pub status_text: String,

    // ---- driving_liquid.py (lazily initialised in Python via getattr) -------------------
    pub liquid_audio_ok: Option<bool>,
    pub liquid_wash_on: bool,
    pub liquid_lateral_cooldown_s: f64,
    pub liquid_surge_said: bool,
    pub liquid_settled_said: bool,
    pub liquid_settle_timer_s: f64,

    // ---- driving.py: lifecycle -----------------------------------------------------------
    pub entered_once: bool,
    /// The co-driver for this load, once one has been paired.
    pub team: Option<crate::states::driving_team::TeamDrive>,
    /// A team departure chosen at the shipper: the co-driver takes the
    /// wheel on the drive's first frame.
    pub pending_team: Option<crate::states::driving_team::TeamMode>,
}

// The accessors onto the trip, the trip swap and the `State` facade live in
// `driving/facade.rs`.
//
// The TEMPORARY stub module that stood here while the mixins landed is gone:
// every block in it has been deleted by the module that took it over, this
// task's (`driving_updates.rs`) last of all.
