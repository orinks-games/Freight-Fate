//! The driving prelude (port of `freight_fate/states/driving_core.py`).
//!
//! Continuous controls (throttle, brake, clutch) are sampled from held keys
//! each frame. Everything the player needs to know is available on demand
//! from information keys, and important changes announce themselves.
//!
//! In Python every `driving_*.py` sibling did `from .driving_core import *`,
//! so this module is the one namespace the whole driving subsystem shares:
//! its constants ([`constants`]), its helper functions, the first-run
//! [`Tutorial`], the radio playback adapter ([`DrivingRadioBackend`]), the
//! facility engine row ([`FacilityEngine`]), and the handful of types the
//! [`DrivingState`] struct needs at construction ([`PendingAmbient`],
//! [`CurveRun`], [`RigBuff`], the [`siren::SirenLoop`]). A mixin module
//! starts with `use crate::states::driving_core::*;` and gets the same names
//! the Python file had, plus the `ff_core` re-exports listed below.
//!
//! Adapters: `ff_core` traits the driving layer has to hand the data types it
//! already holds (`Settings` as a `RadioSettingsAccess`, a `Route` as a
//! `DriveMusicRoute` / `RadioRoute`) are newtypes here, because the orphan
//! rule forbids implementing a foreign trait for a foreign type.

pub mod constants;
pub mod siren;
mod state_helpers;

use std::collections::BTreeMap;
use std::rc::Rc;

use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};

use ff_core::achievements::increment_stat;
use ff_core::data::amenities::spoken_amenities;
use ff_core::data::curves::RouteCurve;
use ff_core::data::world_models::Route;
use ff_core::models::economy::damage_severity_mult;
use ff_core::models::profile::Profile;
use ff_core::music::DriveMusicRoute;
use ff_core::radio::{
    estimate_signal, signal_volume_factor, RadioPlaybackBackend, RadioPlaybackError, RadioRoute,
    RadioSettingsAccess, RadioState, RadioStation, PERSONAL_PLAYLIST_SOURCE_TYPE,
};
use ff_core::settings::Settings;
use ff_core::sim::enforcement_posts::EnforcementPost;
use ff_core::sim::hos::HosClock;
use ff_core::sim::trip_models::{RoadStop, TripEvent, TripEventKind};
use ff_core::sim::vehicle::TruckState;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, Say};
use crate::audio::RADIO_TUNE_FADE_MS;
use crate::states::base::{Menu, MenuItem};
use crate::states::driving::DrivingState;

pub use constants::*;
pub use siren::{
    enforcement_signature_wav, register_enforcement_sounds, SirenLoop, PASS_MARKER_LEAD_S,
    SIGNATURE_HIGH_HZ, SIGNATURE_KEY, SIGNATURE_LOW_HZ, SIGNATURE_RATE, SIREN_FULL_LEVEL,
    SIREN_HOLD_TIMEOUT_S, SIREN_RISE_S, SIREN_START_LEVEL,
};
pub use state_helpers::*;

// -- the names `from .driving_core import *` carried ---------------------------------
//
// Everything driving_core.py imported at module level and the siblings then
// used without importing it themselves. Kept explicit so a mixin port reads
// the same way as its Python source; anything else comes from `ff_core`
// directly.
pub use crate::speech::EventPriority;
pub use ff_core::achievements::{add_unique_stat, reset_stat};
pub use ff_core::data::amenities::classify_brand;
pub use ff_core::data::buffs::buffs_for_stop;
pub use ff_core::models::business::{
    build_business_settlement, has_weigh_station_transponder, pay_label,
    player_pays_operating_costs, reputation_pay_bonus,
};
pub use ff_core::models::business_constants::is_owner_operator;
pub use ff_core::models::career::{xp_class_multiplier, xp_streak_bonus};
pub use ff_core::models::economy::{pay_advance_grant, pay_advance_unavailable_reason, MOTEL_COST};
pub use ff_core::models::enforcement::{
    career_citations, citation_fine, construction_zone_fine_clause,
};
pub use ff_core::models::jobs::{
    fair_active_deadline, job_from_payload, job_payload, normalize_job_cities, Job,
};
pub use ff_core::models::profile;
pub use ff_core::models::settlement::{
    carrier_accessorial_charges, charge_summary, charge_total, wallet_delta,
};
pub use ff_core::music::{
    select_drive_music_sequence, select_menu_music_sequence, select_station_playlist,
};
pub use ff_core::radio::{
    truck_position, RadioReception, SAFE_ROUTE_PLAYLIST, SIGNAL_FULL_VOLUME,
    STATIC_SIGNAL_THRESHOLD,
};
pub use ff_core::radio_content::{content_duration_s, plan_break};
pub use ff_core::settings::{acc_gap_seconds, ACC_GAP_CHOICES, ACC_GAP_DEFAULT};
pub use ff_core::sim::driving_modes::tuning_for_time_scale;
pub use ff_core::sim::enforcement_observe::OBSERVE_LEEWAY_MPH;
pub use ff_core::sim::hos::{clock_text, is_night, time_of_day};
pub use ff_core::sim::lane::{lane_label, lane_phrase, LaneKeeping};
pub use ff_core::sim::lane_guidance::LaneGuidance;
pub use ff_core::sim::pedal_latch::PedalLatch;
pub use ff_core::sim::timezones::city_zone;
pub use ff_core::sim::transmission::REVERSE;
pub use ff_core::sim::trip::{Trip, TripOptions};
pub use ff_core::sim::trip_models::{
    acceleration_lane_capability_mph, acceleration_lane_mi, merge_traffic_target_mph,
    APPROACH_DECEL_MPS2, APPROACH_REACTION_S, DESTINATION_LOCAL_APPROACH_MI, METERS_PER_MILE,
    RAMP_MAX_MPH as TRIP_RAMP_MAX_MPH, RAMP_MIN_DESIGN_MPH,
};
pub use ff_core::sim::vehicle::{
    CHAIN_SAFE_MPH, DAMAGE_BAND_LAST_CALL, DAMAGE_BAND_LIMP, DAMAGE_BAND_NONE,
    DAMAGE_BAND_OUT_OF_SERVICE, DAMAGE_BAND_REDUCED, DAMAGE_CREEP_CAP_MPH, DAMAGE_DERATE_PCT,
    DAMAGE_LAST_CALL_PCT, DAMAGE_LIMP_CAP_MPH, DAMAGE_LIMP_PCT, DAMAGE_MAX_PCT,
    DAMAGE_OUT_OF_SERVICE_PCT, G, HIGH_IDLE_DEFAULT_RPM, HIGH_IDLE_MAX_RPM, HIGH_IDLE_MIN_RPM,
    HIGH_IDLE_STEP_RPM, JAKE_STAGES, KG_PER_TON, REFERENCE_CARGO_KG, REVERSE_ENGAGE_MAX_MPH,
    TIRE_WINTER,
};
pub use ff_core::sim::weather::{WeatherKind, WeatherSystem};
pub use ff_core::sim::{hos, trip_models};
pub use ff_core::sound_catalog::BAR_SOLID_VOLUME;

// -- the profile the drive runs on -------------------------------------------------------

/// `self.ctx.profile`: a drive only exists with a career loaded, so reading
/// it is a programming error otherwise (Python: `AttributeError` on `None`).
pub fn profile_of(ctx: &GameContext) -> &Profile {
    ctx.profile
        .as_ref()
        .expect("a drive runs on a loaded profile")
}

pub fn profile_mut_of(ctx: &mut GameContext) -> &mut Profile {
    ctx.profile
        .as_mut()
        .expect("a drive runs on a loaded profile")
}

/// `self.hos`: the shift clock lives on the profile; the Python field was an
/// alias of `profile.hos`, so there is no copy to get out of step.
pub fn hos_of(ctx: &GameContext) -> &HosClock {
    &profile_of(ctx).hos
}

pub fn hos_mut_of(ctx: &mut GameContext) -> &mut HosClock {
    &mut profile_mut_of(ctx).hos
}

// -- pure helpers ---------------------------------------------------------------------

/// The next cruise set point from a +/- tap.
///
/// Plain taps walk the fives grid the way a real cruise stalk does: an
/// off-grid target (K captures the exact road speed, so 32 happens all
/// the time) snaps outward to the next multiple, healing itself in one
/// press instead of stepping 37, 42 forever (testers Jerry and Sarah,
/// 2026-08-13). Ctrl taps move by exactly one for the players who need
/// a precise number and cannot feather K onto it. The epsilon keeps a
/// float a hair off the grid from turning a tap into a no-op snap.
pub fn cruise_step_target(target_mph: f64, direction: i32, fine: bool) -> f64 {
    let stepped = if fine {
        target_mph + direction as f64
    } else if direction > 0 {
        let notches = (target_mph / CRUISE_STEP_MPH + 1e-9).floor();
        (notches + 1.0) * CRUISE_STEP_MPH
    } else {
        let notches = (target_mph / CRUISE_STEP_MPH - 1e-9).ceil();
        (notches - 1.0) * CRUISE_STEP_MPH
    };
    stepped.clamp(CRUISE_MIN_MPH, CRUISE_MAX_MPH)
}

/// Conservatively cover the spoken cue plus a real response window.
///
/// Screen-reader speech completion is not observable through every Prism
/// backend, so model a deliberately slow 30-to-60 WPM voice, scaled by the
/// player's event-voice rate, instead of starting a fixed timer when the
/// utterance is merely queued. Once the player sets the parking brake, the stop
/// remains accepted while the truck finishes decelerating.
/// (`speech_rate` defaulted to 0.5 in Python.)
pub fn ramp_arrival_grace_seconds(message: &str, speech_rate: f64) -> f64 {
    let rate = speech_rate.clamp(0.0, 1.0);
    let modeled_wpm = RAMP_SPEECH_WPM_MIN + (RAMP_SPEECH_WPM_MAX - RAMP_SPEECH_WPM_MIN) * rate;
    let spoken_seconds = message.split_whitespace().count() as f64 * 60.0 / modeled_wpm;
    RAMP_ARRIVAL_GRACE_MIN_S.max(spoken_seconds + RAMP_ARRIVAL_REACTION_S)
}

/// The spoken zone crossing: terse mode says only the zone itself.
pub fn timezone_crossing_message(event: &TripEvent, terse: bool) -> String {
    if let (true, Some(zone)) = (terse, event.data.to_zone) {
        return format!("{}.", zone.name);
    }
    event.text().to_string()
}

/// `_route_event_sound`: the earcon a route event plays, if any.
pub fn route_event_sound(event: &TripEvent) -> Option<&'static str> {
    match event.kind {
        TripEventKind::Hazard => Some("events/hazard_warning"),
        TripEventKind::Inspection => Some("events/inspection_warning"),
        TripEventKind::TollCharged => Some("events/toll_charged"),
        TripEventKind::StateCrossing | TripEventKind::Checkpoint => Some("events/state_crossing"),
        // A boundary marker like a state line; reuse its earcon until the
        // sound pack gains a dedicated one.
        TripEventKind::TimezoneCrossing => Some("events/state_crossing"),
        TripEventKind::ZoneEnter => {
            match event.data.zone.as_ref().map(|zone| zone.reason.as_str()) {
                Some("construction") => Some("events/construction_zone"),
                // The catalog says this earcon means the traffic in front of
                // you is coming down in speed, so only a jam earns it. A
                // yard access road, a gate or an approach is a limit, not
                // traffic; the handler's plain notice covers those. Every
                // drive used to open on the traffic cue with the engine off
                // (agent drive, 2026-09-01).
                Some("heavy traffic") => Some("events/traffic_slowing"),
                _ => None,
            }
        }
        TripEventKind::GpsCue => {
            if event.data.cb_patrol.is_some() {
                return Some("events/cb_radio_chatter");
            }
            if event.data.traffic_pressure.is_some() {
                return Some("events/traffic_slowing");
            }
            let cue_kind = event.data.cue.as_ref().map(|cue| cue.kind.as_str());
            match cue_kind {
                Some("local_turn") => {
                    local_turn_sound(event.data.cue.as_ref().map(|c| c.direction.as_str()))
                }
                Some("traffic") => Some(traffic_vehicle_sound(event)),
                // The toll earcon means the truck was billed, so it plays
                // once, at the plaza. On the "toll point ahead" warning too,
                // it sounded twice per plaza (agent drive, 2026-10-17).
                _ => None,
            }
        }
        _ => None,
    }
}

/// The earcon for a slower vehicle ahead: the slowing-traffic cue, or the
/// trooper's own pass when it is a trooper. The vehicle's own sound is its
/// traffic loop once it is near; a pass whoosh here, with the vehicle still a
/// quarter mile up the road, sounded like something going by that never did
/// (owner drive, 2026-10-09).
pub fn traffic_vehicle_sound(event: &TripEvent) -> &'static str {
    let trooper = event
        .data
        .npc_vehicle
        .as_ref()
        .is_some_and(|v| v.vehicle_class.trim().eq_ignore_ascii_case("state trooper"));
    if trooper {
        "traffic/trooper_pass"
    } else {
        "events/traffic_slowing"
    }
}

/// `_local_turn_sound`: the turn earcon for a cue direction.
pub fn local_turn_sound(direction: Option<&str>) -> Option<&'static str> {
    match direction.unwrap_or("").trim().to_lowercase().as_str() {
        "left" => Some("events/turn_left"),
        "right" => Some("events/turn_right"),
        "ahead" | "straight" => Some("events/turn_ahead"),
        _ => None,
    }
}

/// Stereo pan for a route event's sound cue; only local turns pan.
pub fn route_event_sound_pan(event: &TripEvent) -> f64 {
    if event.kind != TripEventKind::GpsCue {
        return 0.0;
    }
    let Some(cue) = event.data.cue.as_ref() else {
        return 0.0;
    };
    if cue.kind != "local_turn" {
        return 0.0;
    }
    match cue.direction.trim().to_lowercase().as_str() {
        "left" => -TURN_CUE_PAN,
        "right" => TURN_CUE_PAN,
        _ => 0.0,
    }
}

/// `_poi_ambient_key`: the ambience bed for a stop at this hour.
pub fn poi_ambient_key(stop: &RoadStop, hour: f64) -> &'static str {
    if stop.stop_type == "weigh_station" {
        return "poi/weigh_station_lane";
    }
    if is_night(hour) {
        return "poi/rest_stop_night";
    }
    "ambient/truck_stop"
}

/// What a road shop charges to bring `damage_pct` down to `down_to`.
///
/// Same severity curve as the terminal garage (`damage_severity_mult`) so
/// the two never disagree about what deep damage is worth, plus whatever
/// call-out fee getting a mechanic to the truck carries.
pub fn road_repair_cost(damage_pct: f64, down_to: f64, callout_fee: f64) -> f64 {
    let repaired = (damage_pct - down_to).max(0.0);
    callout_fee + repaired * MECHANIC_RATE_PER_PCT * damage_severity_mult(damage_pct)
}

/// Every inspection feeds both the one-off badge and the career tally.
/// (Python's `event=` keyword was accepted and unused by `award_achievement`.)
pub fn record_inspection(ctx: &mut GameContext) {
    ctx.award_achievement("inspection");
    let regular = ctx
        .profile
        .as_mut()
        .is_some_and(|profile| increment_stat(profile, "inspections_passed") >= 5);
    if regular {
        ctx.award_achievement("scale_regular");
    }
}

/// Collapse runs of the same spoken name so a facility-approach route
/// (which repeats one city key once per local leg) does not read as
/// "Portland to Portland to Portland" on the Map and settlement screens.
pub fn unique_consecutive(parts: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(parts.len());
    for part in parts {
        if out.last().map(String::as_str) != Some(part.as_str()) {
            out.push(part.clone());
        }
    }
    out
}

/// `_join_phrase`: "a", "a, and b", "a, b, and c".
pub fn join_phrase(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        _ => format!(
            "{}, and {}",
            parts[..parts.len() - 1].join(", "),
            parts[parts.len() - 1]
        ),
    }
}

/// `_poi_offers_text`: what a stop offers, for the route-info menu.
pub fn poi_offers_text(stop: &RoadStop) -> String {
    let offers: Vec<String> = stop
        .actions
        .iter()
        .filter_map(|action| {
            POI_ACTION_LABELS
                .iter()
                .find(|(key, _)| key == action)
                .map(|(_, label)| label.to_string())
        })
        .collect();
    let services: Vec<String> = stop
        .services
        .iter()
        .map(|service| {
            POI_SERVICE_LABELS
                .iter()
                .find(|(key, _)| key == service)
                .map(|(_, label)| label.to_string())
                .unwrap_or_else(|| service.replace('_', " "))
        })
        .collect();
    let mut parts: Vec<String> = Vec::new();
    if !offers.is_empty() {
        parts.push(format!("offers {}", join_phrase(&offers)));
    }
    if !services.is_empty() {
        parts.push(format!("listed services: {}", join_phrase(&services)));
    }
    let brand_text = spoken_amenities(&stop.name, &stop.stop_type);
    if !brand_text.is_empty() {
        parts.push(brand_text);
    }
    let parking_text = stop.parking_text();
    if !parking_text.is_empty() {
        parts.push(parking_text);
    }
    if parts.is_empty() {
        "services not listed".to_string()
    } else {
        parts.join("; ")
    }
}

/// `FREEWAY_VIA_RE`: whether an interchange's `via` names an interstate.
pub static FREEWAY_VIA_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(FREEWAY_VIA_PATTERN).expect("a valid freeway pattern"));

pub fn freeway_via_matches(via: &str) -> bool {
    FREEWAY_VIA_RE.is_match(via)
}

/// `DrivingControlsMixin._JAKE_STAGE_WORD`: the spoken retarder stage.
pub fn jake_stage_word(stage: i32) -> &'static str {
    match stage {
        1 => "one",
        2 => "two",
        3 => "three",
        _ => "",
    }
}

/// Start or stop the engine from a menu, keeping the audio loop in step.
///
/// The driving frame loop only notices engine transitions that happen inside
/// `truck.update()`, so a change made while a menu holds the screen has to
/// move the audio itself; otherwise the loop idles on forever with the engine
/// off, or the truck runs in silence. Returns false only when the starter
/// refuses (no fuel), which leaves both the truck and the audio untouched.
pub fn set_engine_running(ctx: &mut GameContext, truck: &mut TruckState, running: bool) -> bool {
    if running {
        if !truck.start_engine() {
            return false;
        }
        ctx.audio.engine_start();
        return true;
    }
    if truck.engine_on {
        truck.stop_engine();
        ctx.audio.engine_stop();
    }
    true
}

// -- small types the DrivingState struct is built from ----------------------------------

/// The three facts about a hazard the assist has to budget against.
///
/// They were one flag once, and that is the defect this type exists to make
/// impossible: `dodgeable` decided BOTH whether the driver got a lane-change
/// allowance AND what braking alone had to reach, so the two could never be
/// told apart. They answer different questions.
///
/// * `dodgeable` -- is there somewhere to go: the hazard sits in one lane
///   AND the road has an open lane on this side. It buys the driver
///   `LANE_TAP_CHANGE_S` of extra window, and nothing else.
/// * `in_lane` -- is the thing sitting in our lane (an object, a stopped
///   car, the vehicle ahead) rather than spanning the road (fog, ice, a
///   crosswind). It decides the near stop.
/// * `lead_mph` -- the hazard IS a moving vehicle, and its speed is what
///   clears it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HazardShape {
    pub dodgeable: bool,
    pub in_lane: bool,
    pub lead_mph: Option<f64>,
    /// What braking alone has to reach, for a thing in the lane a truck
    /// passes slowly rather than stops for. See `HazardDef::brake_to_mph`.
    pub brake_to_mph: Option<f64>,
}

/// One ambient line waiting its turn, and how long it has waited
/// (`driving_events.PendingAmbient`, defined here because the struct holds
/// the queue from construction).
///
/// `key` names a standing thing the line is ABOUT -- a patrol post the
/// truck is closing on, a traffic pressure, a toll -- rather than a moment
/// that happened. A standing thing restates itself as the distance falls,
/// and the queue supersedes rather than stacks for those. A moment (a state
/// line, a lane count, a billboard) has no key and keeps its place.
pub struct PendingAmbient {
    pub message: String,
    pub sound: Option<String>,
    pub category: Option<SpeechCategory>,
    pub waited_s: f64,
    pub key: Option<String>,
    pub priority: Option<EventPriority>,
    /// A line that counts down toward something can re-render at delivery, so
    /// a wait never makes it lie: the 12-second age cap is REAL seconds, and
    /// under time compression that is miles -- a travel plaza queued at "in 5
    /// miles" was performed with two left and the building in sight (Brandon,
    /// 2026-08-20). Returning None means the moment passed; drop, not speak.
    pub render: Option<AmbientRender>,
}

/// The re-render hook of a [`PendingAmbient`]: given the drive as it is at
/// delivery, the line to speak now, or `None` when its moment has passed.
pub type AmbientRender = Rc<dyn Fn(&DrivingState, &GameContext) -> Option<String>>;

/// The last thing the CB said, kept so a driver who missed it can hear it
/// again (Sarah A., issue 156).
///
/// CB chatter names something on the road ahead and goes past exactly once,
/// so missing it used to cost the chance to act on it. Neither existing
/// replay answers for it: `last_event_message` behind the A key is one slot
/// that every other route announcement overwrites within seconds, and the
/// message log walks the history by recency, never by topic.
///
/// `post` rides along whenever the call was about an enforcement post, so
/// the repeat can say how far off it is NOW instead of parroting the
/// distance it was then. A rescued line has to still be true -- the same
/// rule the hazard, curve and stop callouts already carry.
#[derive(Clone, Debug)]
pub struct CbChatterRecall {
    /// The line as the CB said it, for chatter with no distance in it.
    pub text: String,
    /// The post the call was about, when it was about one.
    pub post: Option<EnforcementPost>,
}

impl PendingAmbient {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            sound: None,
            category: None,
            waited_s: 0.0,
            key: None,
            priority: None,
            render: None,
        }
    }
}

/// `_curve_run`: the bend underway, and how it is going
/// (`driving_updates._update_curve_run`).
#[derive(Debug, Clone, PartialEq)]
pub struct CurveRun {
    pub curve: RouteCurve,
    pub demanding: bool,
    pub touched: bool,
    pub hot: bool,
}

/// One rig-care buff held for the trip (`rig_buffs[group]`), as the rest
/// stop writes it: `{"id", "label", "rate"}`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RigBuff {
    pub id: String,
    pub label: String,
    pub rate: f64,
}

/// `rig_buffs`: buff group -> buff. Ordered so a snapshot is stable.
pub type RigBuffs = BTreeMap<String, RigBuff>;

/// `_pending_sounds` entry: `(delay_s, key, volume, pan)`.
pub type PendingSound = (f64, String, f64, f64);

// -- the first-run tutorial and the instructor hook -----------------------------------------

/// What a drive calls on its instructor, if it has one: the first-run
/// [`Tutorial`], or a driving-school lesson that duck-typed it in Python.
pub trait Instructor {
    fn begin(&mut self, ctx: &mut GameContext);
    fn on_engine_started(&mut self, ctx: &mut GameContext);
    fn on_parking_brake_released(&mut self, ctx: &mut GameContext);
    fn on_gear_engaged(&mut self, ctx: &mut GameContext);
    fn update(&mut self, ctx: &mut GameContext, dt: f64, truck: &TruckState);
}

/// First-drive guidance, spoken step by step as the player succeeds.
///
/// Deliberately verbosity-blind (research doc, R15): terse speech is a
/// filter on running commentary, and first-run teaching is not commentary.
/// A brand-new player who picks terse before their first drive -- exactly
/// the player who hates chatty games -- must still be told the status,
/// help, and hazard keys exist, or they can never pull information they
/// were never told about. The gate is `tutorial_done` itself: this type
/// is only constructed while the walkthrough is unfinished, and finishing
/// it then flipping terse on resurrects nothing.
#[derive(Debug, Default)]
pub struct Tutorial {
    pub stage: i32,
    timer: f64,
    hinted: bool,
}

impl Tutorial {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Instructor for Tutorial {
    fn begin(&mut self, ctx: &mut GameContext) {
        let text = format!(
            "Your first run. Press {} to start the engine.",
            ctx.control_hint("engine")
        );
        ctx.say_with(text, Say::queued());
    }

    fn on_engine_started(&mut self, ctx: &mut GameContext) {
        if self.stage == 0 {
            self.stage = 1;
            self.timer = 0.0;
            self.hinted = false;
            let text = if ctx.settings.automatic_transmission {
                format!(
                    "At air ready, press {} to release the parking brake, then hold {} to \
                     accelerate.",
                    ctx.control_hint("parking_brake"),
                    ctx.control_hint("accelerate")
                )
            } else {
                format!(
                    "At air ready, press {} to release the parking brake, then hold {}, select \
                     {} for first gear, and release the clutch.",
                    ctx.control_hint("parking_brake"),
                    ctx.control_hint("clutch"),
                    ctx.control_hint("gear_first")
                )
            };
            ctx.say_with(text, Say::queued());
        }
    }

    fn on_parking_brake_released(&mut self, ctx: &mut GameContext) {
        if self.stage == 1 && ctx.settings.automatic_transmission {
            self.stage = 2;
            self.timer = 0.0;
            self.hinted = false;
            let text = format!(
                "Parking brake released. Hold {} to accelerate.",
                ctx.control_hint("accelerate")
            );
            ctx.say_with(text, Say::queued());
        } else if self.stage == 1 {
            self.timer = 0.0;
            self.hinted = false;
            ctx.say_with(
                "Parking brake released. Shift into first gear.",
                Say::queued(),
            );
        }
    }

    fn on_gear_engaged(&mut self, ctx: &mut GameContext) {
        if self.stage == 1 {
            self.stage = 2;
            self.timer = 0.0;
            self.hinted = false;
            let text = format!(
                "In gear. Hold {} to accelerate.",
                ctx.control_hint("accelerate")
            );
            ctx.say_with(text, Say::queued());
        }
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64, truck: &TruckState) {
        self.timer += dt;
        if self.stage == 2 && truck.speed_mph() > 20.0 {
            self.stage = 3;
            let text = format!(
                "Rolling. {} for your speed, {} for a full report, {} for all the controls. Brake \
                 hard at a hazard warning; {} stops fast.",
                ctx.control_hint("speed"),
                ctx.control_hint("status_menu"),
                ctx.control_hint("help"),
                ctx.control_hint("emergency_brake")
            );
            ctx.say_with(text, Say::queued());
            profile_mut_of(ctx).tutorial_done = true;
            ctx.save_profile();
        } else if (self.stage == 0 || self.stage == 1) && self.timer > 25.0 && !self.hinted {
            self.hinted = true;
            let text = if self.stage == 0 {
                format!(
                    "Reminder: press {} to start the engine.",
                    ctx.control_hint("engine")
                )
            } else if truck.parking_brake && truck.air_ready() {
                // The timer does not know the compressor finished: "wait for
                // air" right after "Air ready" contradicted the truck (agent
                // drive, 2026-09-01). Air is up, so the only step left is P.
                format!(
                    "Reminder: air is ready. Press {} to release the parking brake.",
                    ctx.control_hint("parking_brake")
                )
            } else if truck.parking_brake {
                format!(
                    "Reminder: wait for air pressure to reach 100 psi, then press {} to release \
                     the parking brake.",
                    ctx.control_hint("parking_brake")
                )
            } else {
                format!(
                    "Reminder: hold {}, select {}, then release the clutch.",
                    ctx.control_hint("clutch"),
                    ctx.control_hint("gear_first")
                )
            };
            ctx.say_with(text, Say::queued());
        }
    }
}

// -- the radio playback adapter ---------------------------------------------------------

/// Adapts radio station choices onto the existing safe music backend
/// (`_DrivingRadioBackend`).
///
/// Python's backend held a reference to the driving state; here it borrows
/// the drive and the context for one radio call, which is why it is built by
/// [`DrivingState::with_radio_backend`] rather than stored. The reception it
/// needs (`radio.current_reception()` in Python) is recomputed from the
/// truck position the radio carried when the call began, because the radio
/// itself is on loan to the call.
pub struct DrivingRadioBackend<'a> {
    pub driving: &'a mut DrivingState,
    pub ctx: &'a mut GameContext,
    position: Option<(f64, f64)>,
    elevation_ft: Option<f64>,
}

impl<'a> DrivingRadioBackend<'a> {
    pub fn new(
        driving: &'a mut DrivingState,
        ctx: &'a mut GameContext,
        position: Option<(f64, f64)>,
        elevation_ft: Option<f64>,
    ) -> Self {
        Self {
            driving,
            ctx,
            position,
            elevation_ft,
        }
    }

    /// `radio.current_reception()` for the station being started.
    fn reception_for(&self, station: &RadioStation) -> RadioReception {
        if station.fallback {
            let mut reception = RadioReception::new(station.clone(), None, 1.0, "fallback");
            reception.fallback = true;
            return reception;
        }
        estimate_signal(station, self.position, self.elevation_ft)
    }
}

impl RadioPlaybackBackend for DrivingRadioBackend<'_> {
    fn play_station(
        &mut self,
        station: &RadioStation,
        _volume: f64,
    ) -> Result<(), RadioPlaybackError> {
        let reception = self.reception_for(station);
        self.start(&reception)
    }

    fn play_reception(
        &mut self,
        reception: &RadioReception,
        _volume: f64,
    ) -> Result<(), RadioPlaybackError> {
        self.start(reception)
    }

    fn stop_radio(&mut self) {
        self.ctx.audio.stop_music_with(600);
    }
}

impl DrivingRadioBackend<'_> {
    /// Start `reception.station` at the volume its signal supports.
    fn start(&mut self, reception: &RadioReception) -> Result<(), RadioPlaybackError> {
        let station = &reception.station;
        self.driving.radio_signal_factor = signal_volume_factor(reception);
        if station.source_type == PERSONAL_PLAYLIST_SOURCE_TYPE {
            self.driving.apply_radio_volume(self.ctx);
            // A playlist with nothing playable in it must reach the dial's
            // fallback, exactly as the Python raised through here: the
            // station comes off the dial after its second refusal and the
            // handover is spoken. Swallowing the error left the player on a
            // silent station with nothing said about it.
            return self
                .driving
                .start_playlist_station_checked(self.ctx, station, 900, false);
        }
        if station.real_stream {
            if station.stream_url.is_empty() {
                return Err(RadioPlaybackError("station has no stream URL".to_string()));
            }
            self.driving.apply_radio_volume(self.ctx);
            // The station on the air, for the reception tick and the drivers
            // board: the built-in and playlist branches record it as their
            // rotation starts, and a live stream has to record it here or
            // the cab has no answer for what it is playing.
            self.driving.radio_station_id = station.id.clone();
            return self
                .ctx
                .audio
                .play_radio_stream_with(&station.stream_url, RADIO_TUNE_FADE_MS)
                .map_err(|_| RadioPlaybackError("external stream playback failed".to_string()));
        }
        self.driving.apply_radio_volume(self.ctx);
        if station.fallback {
            self.driving.radio_station_id = station.id.clone();
            self.ctx.audio.stop_music_with(600);
        } else if DrivingState::is_channel3000(station) {
            self.driving.start_channel3000(self.ctx, 900);
        } else {
            self.driving.start_station_rotation(self.ctx, station, 900);
        }
        Ok(())
    }
}

impl DrivingState {
    /// Run one radio call with the playback backend attached: the Python
    /// `self.radio.<call>(self._radio_backend)`.
    ///
    /// The radio is lent to the closure while the backend borrows the drive,
    /// so a call that starts playback can reach the rotation and playlist
    /// machinery on the drive. Nothing on the drive reads `self.radio`
    /// during the call (`apply_radio_volume` reads the setting).
    pub fn with_radio_backend<R>(
        &mut self,
        ctx: &mut GameContext,
        call: impl FnOnce(&mut RadioState, &mut DrivingRadioBackend<'_>) -> R,
    ) -> R {
        let mut radio = std::mem::replace(&mut self.radio, RadioState::new(Vec::new()));
        let position = radio.position;
        let elevation_ft = radio.elevation_ft;
        let result = {
            let mut backend = DrivingRadioBackend::new(self, ctx, position, elevation_ft);
            call(&mut radio, &mut backend)
        };
        self.radio = radio;
        result
    }
}

// -- the facility engine row --------------------------------------------------------------

/// The engine kill switch, offered where a facility menu has taken over
/// (`FacilityEngineMixin`).
///
/// Arriving at a shipper or a receiver parks the truck under half a mile an
/// hour and hands straight to a menu, so the road's engine control is out of
/// reach at exactly the moment a driver reaches for it: sitting at the gate,
/// or waiting on a dock crew (new player feedback, 2026-08-17). A menu state
/// implements the two accessors and both facilities then offer the same one
/// row, worded the same way. The truck is reached through a closure because
/// the arrival menu's truck lives on the covered driving state, not on the
/// menu.
pub trait FacilityEngine: Menu {
    /// `facility_truck.engine_on`.
    fn facility_engine_on(&self, ctx: &GameContext) -> bool;

    /// Borrow `facility_truck` for one operation.
    fn with_facility_truck<R>(
        &mut self,
        ctx: &mut GameContext,
        f: impl FnOnce(&mut GameContext, &mut TruckState) -> R,
    ) -> R;

    /// Hook for a facility state that keeps a resume snapshot of its own.
    fn on_facility_engine_changed(&mut self, _ctx: &mut GameContext) {}

    /// One row that changes face, never two rows to arrow past.
    fn facility_engine_item(&self, ctx: &GameContext) -> MenuItem<Self> {
        if self.facility_engine_on(ctx) {
            MenuItem::new(FACILITY_ENGINE_SHUT_DOWN_ITEM, |s: &mut Self, ctx| {
                s.toggle_facility_engine(ctx)
            })
            .help("Engine off while parked, no fuel burned. Start it again before pulling out.")
        } else {
            MenuItem::new(FACILITY_ENGINE_START_ITEM, |s: &mut Self, ctx| {
                s.toggle_facility_engine(ctx)
            })
            .help("Starts the engine. The parking brake releases at 100 psi.")
        }
    }

    fn toggle_facility_engine(&mut self, ctx: &mut GameContext) {
        let (speed_mph, engine_on) =
            self.with_facility_truck(ctx, |_, truck| (truck.speed_mph(), truck.engine_on));
        if speed_mph > DOCKING_MAX_MPH {
            ctx.audio.play("ui/error");
            ctx.say("Stop before touching the engine.");
            return;
        }
        if engine_on {
            self.with_facility_truck(ctx, |ctx, truck| set_engine_running(ctx, truck, false));
            self.on_facility_engine_changed(ctx);
            self.refresh(ctx, true);
            ctx.say("Engine off.");
            return;
        }
        let started =
            self.with_facility_truck(ctx, |ctx, truck| set_engine_running(ctx, truck, true));
        if !started {
            ctx.audio.play("ui/error");
            ctx.say("The engine will not start.");
            return;
        }
        self.on_facility_engine_changed(ctx);
        self.refresh(ctx, true);
        let psi = self.with_facility_truck(ctx, |_, truck| truck.air_pressure_psi());
        ctx.say(&format!("Engine running. Air pressure {psi:.0} psi."));
    }
}

// -- adapters onto ff_core traits -------------------------------------------------------------

/// `Settings` read as the radio's settings (`RadioState.from_settings`).
pub struct RadioSettingsView<'a>(pub &'a Settings);

impl RadioSettingsAccess for RadioSettingsView<'_> {
    fn radio_enabled(&self) -> bool {
        self.0.radio_enabled
    }
    fn radio_station_id(&self) -> String {
        self.0.radio_station_id.clone()
    }
    fn radio_volume(&self) -> f64 {
        self.0.radio_volume
    }
    fn radio_streamer_safe(&self) -> bool {
        self.0.radio_streamer_safe
    }
    fn synth_music(&self) -> bool {
        self.0.synth_music
    }
    fn set_radio_enabled(&mut self, _enabled: bool) {}
    fn set_radio_station_id(&mut self, _station_id: &str) {}
}

/// `Settings` written by the radio (`radio.write_settings(settings)`).
pub struct RadioSettingsMut<'a>(pub &'a mut Settings);

impl RadioSettingsAccess for RadioSettingsMut<'_> {
    fn radio_enabled(&self) -> bool {
        self.0.radio_enabled
    }
    fn radio_station_id(&self) -> String {
        self.0.radio_station_id.clone()
    }
    fn radio_volume(&self) -> f64 {
        self.0.radio_volume
    }
    fn radio_streamer_safe(&self) -> bool {
        self.0.radio_streamer_safe
    }
    fn synth_music(&self) -> bool {
        self.0.synth_music
    }
    fn set_radio_enabled(&mut self, enabled: bool) {
        self.0.radio_enabled = enabled;
    }
    fn set_radio_station_id(&mut self, station_id: &str) {
        self.0.radio_station_id = station_id.to_string();
    }
}

/// A `Route` as the music picker reads it (`music._route_key`).
pub struct MusicRoute<'a>(pub &'a Route);

impl DriveMusicRoute for MusicRoute<'_> {
    fn cities(&self) -> Vec<String> {
        self.0.cities.clone()
    }
    fn highways(&self) -> Vec<String> {
        self.0.highways()
    }
    fn terrain_summary(&self) -> String {
        self.0.terrain_summary().to_string()
    }
}

/// A `Route` as the radio reads it to place the truck (`radio.truck_position`).
pub struct RadioRouteView<'a>(pub &'a Route);

impl RadioRoute for RadioRouteView<'_> {
    fn leg_count(&self) -> usize {
        self.0.legs.len()
    }
    fn city(&self, index: usize) -> String {
        self.0.cities.get(index).cloned().unwrap_or_default()
    }
    fn leg_miles(&self, index: usize) -> f64 {
        self.0.legs[index].miles
    }
    fn leg_a(&self, index: usize) -> String {
        self.0.legs[index].a.clone()
    }
    fn leg_route_points(&self, index: usize) -> Vec<ff_core::radio::RoutePoint> {
        self.0.legs[index]
            .route_points()
            .iter()
            .map(|p| ff_core::radio::RoutePoint {
                lat: p.lat,
                lon: p.lon,
                at_mi: p.at_mi,
            })
            .collect()
    }
    fn leg_elevation_samples(&self, index: usize) -> Vec<ff_core::radio::ElevationSample> {
        self.0.legs[index]
            .elevation_samples()
            .iter()
            .map(|s| ff_core::radio::ElevationSample {
                at_mi: s.at_mi,
                elevation_ft: s.elevation_ft,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unique_consecutive_collapses_runs_only() {
        assert_eq!(unique_consecutive(&[]), Vec::<String>::new());
        assert_eq!(
            unique_consecutive(&["Portland".into(), "Portland".into(), "Astoria".into()]),
            vec!["Portland".to_string(), "Astoria".to_string()]
        );
        // A real corridor that revisits a city non-consecutively stays intact.
        assert_eq!(
            unique_consecutive(&["A".into(), "B".into(), "A".into(),]),
            vec!["A".to_string(), "B".to_string(), "A".to_string()]
        );
    }

    #[test]
    fn join_phrase_reads_like_a_list() {
        assert_eq!(join_phrase(&[]), "");
        assert_eq!(join_phrase(&["fuel".to_string()]), "fuel");
        assert_eq!(
            join_phrase(&["fuel".to_string(), "food".to_string()]),
            "fuel, and food"
        );
        assert_eq!(
            join_phrase(&["a".to_string(), "b".to_string(), "c".to_string()]),
            "a, b, and c"
        );
    }

    #[test]
    fn jake_stage_words_match_the_dash() {
        assert_eq!(jake_stage_word(1), "one");
        assert_eq!(jake_stage_word(3), "three");
    }
}
