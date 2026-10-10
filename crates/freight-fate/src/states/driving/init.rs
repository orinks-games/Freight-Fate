//! `DrivingState.__init__` (port of `freight_fate/states/driving.py`), plus
//! the `__init__`-time work the mixins delegated: `_enforcement_init`,
//! `_reset_lane_gap`, `_reset_turn_state_for_trip`.
//!
//! Field order follows the Python constructor, and every field the struct
//! declares is assigned here -- no `..Default::default()`, so a field added
//! to the struct without a starting value is a compile error rather than a
//! silent zero.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use ff_core::data::world_models::Route;
use ff_core::models::cargo_condition::cargo_fragility;
use ff_core::models::jobs::Job;
use ff_core::pyrandom::PyRandom;
use ff_core::radio::community::{load_community_stations, new_community_stations};
use ff_core::radio::{
    default_radio_catalog, load_personal_playlists, RadioState, RadioStation, PLAYLISTS_DIR_NAME,
};
use ff_core::radio_rotation::initial_airtime_s;
use ff_core::sim::lane_guidance::{HAIRPIN_ADVISORY_MPH, STRIP_LEAD_MI};
use ff_core::sim::real_traffic::RealTrafficProvider;
use ff_core::sim::real_weather::RealWeatherProvider;
use ff_core::sim::season::real_clock_game_hours;
use ff_core::sim::surge::{liquid_load_for, LiquidCargo};
use ff_core::sim::trip_traffic::TrafficProvider;
use ff_core::sim::truck_parking::TruckParkingProvider;
use ff_core::sim::turn_guide::TurnGuide;
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::WeatherProvider;

use crate::app::GameContext;
use crate::net::UreqTransport;
use crate::states::driving_core::*;
use crate::states::driving_events::billboard_moment::BillboardWatch;
use crate::states::driving_updates::radio_channel3000::{
    channel3000_for_drive, channel3000_playable,
};

use super::DrivingState;

/// `random.Random(None if trip_seed is None else trip_seed ^ xor)`: the
/// per-purpose streams stay unseeded when the drive itself is.
fn seeded_rng(trip_seed: Option<i64>, xor: i64) -> PyRandom {
    match trip_seed {
        None => PyRandom::new_unseeded(),
        Some(seed) => PyRandom::new_from_i64(seed ^ xor),
    }
}

/// `data_dir() / "Playlists"` (`radio.personal_playlists_dir`).
fn personal_playlists_dir() -> std::path::PathBuf {
    profile::data_dir().join(PLAYLISTS_DIR_NAME)
}

impl DrivingState {
    /// `DrivingState(ctx, job, route, trip_seed=None, phase=DRIVE_PHASE_DELIVERY,
    /// start_hour=None)`.
    pub fn new(
        ctx: &mut GameContext,
        job: Job,
        route: Route,
        trip_seed: Option<i64>,
        phase: &'static str,
        start_hour: Option<f64>,
    ) -> DrivingState {
        // The seed as it was PASSED: the cosmetic streams below stay
        // unseeded when the drive is, which a derived trip seed would hide.
        let seed_arg = trip_seed;
        let trip_seed = trip_seed.unwrap_or_else(|| PyRandom::new_unseeded().randrange(1 << 31));
        if ctx.settings.time_scale == 1.0 && start_hour.is_none() {
            profile_mut_of(ctx).sync_calendar_to(real_clock_game_hours(None));
            ctx.save_profile();
        }
        // `_enforcement_init` opened with this; the synthesized signature has
        // to exist before the first enforcement cue can play.
        register_enforcement_sounds();

        let mut truck = TruckState::new(profile_of(ctx).truck_specs());
        // Loaded delivery runs carry the job's payload; pickup deadheads and
        // empty bobtail repositions run light. Gross weight drives the physics,
        // so a heavy load pulls away gently and lugs on grades.
        truck.cargo_kg = if phase == DRIVE_PHASE_DELIVERY {
            job.weight_tons * KG_PER_TON
        } else {
            0.0
        };
        // A reposition run is the tractor alone -- nothing on the fifth
        // wheel. Pickup deadheads haul their empty box.
        truck.trailer_attached = !job.bobtail;
        truck.transmission.automatic = ctx.settings.automatic_transmission;
        // How well this freight survives being thrown about. Fed to the truck
        // because the forces that move a load are the truck's, but what the
        // receiver does about the result belongs to models/cargo_condition.
        let delivery_cargo = if phase == DRIVE_PHASE_DELIVERY {
            Some(job.cargo)
        } else {
            None
        };
        truck.cargo_fragility = cargo_fragility(delivery_cargo);
        // A tank load is the only freight that keeps moving after the truck
        // stops. How full the shell is comes straight from the load's weight,
        // so the wave is a deterministic property of the job -- the same run
        // always drives the same way. Every other kind of freight leaves this
        // None and never touches a line of the surge model.
        truck.liquid = liquid_load_for(
            delivery_cargo.map(|cargo| cargo as &dyn LiquidCargo),
            job.weight_tons,
        );
        profile_of(ctx).load_truck_condition(&mut truck);
        truck.set_cold_air_start();

        let start_damage = profile_of(ctx).truck_damage_pct();
        // Trip-start wear, for "this run added..." deltas at settlement
        // (mid-trip saves re-sync the profile, so the profile can't provide
        // these once the trip is underway).
        let start_tire_wear = profile_of(ctx).tire_wear_pct();
        let start_brake_wear = profile_of(ctx).brake_wear_pct();
        let start_engine_wear = profile_of(ctx).engine_wear_pct();

        let region = ctx
            .world
            .city(&job.origin)
            .map(|city| city.region.clone())
            .unwrap_or_default();
        let weather = WeatherSystem::new(
            &region,
            Some(trip_seed),
            live_weather_provider(ctx),
            Some(profile_of(ctx).calendar_game_hours()),
            ctx.settings.live_weather_controls_calendar,
        );

        let trip_start_hour = start_hour.unwrap_or_else(|| {
            if ctx.settings.time_scale == 1.0 {
                profile_of(ctx).calendar_game_hours().rem_euclid(24.0)
            } else {
                profile_of(ctx).game_hours.rem_euclid(24.0)
            }
        });
        let hazard_scale = hos::hazard_scale(&ctx.settings.hos_mode)
            * tuning_for_time_scale(ctx.settings.time_scale).hazard_frequency;
        // The arrival zones size the approach from the facility's own record
        // rather than a flat last-three-miles, so the run holds corridor speed
        // until the local road really begins.
        let (approach_city, approach_location) = if phase == DRIVE_PHASE_PICKUP {
            (&job.origin, &job.origin_location)
        } else {
            (&job.destination, &job.destination_location)
        };
        let destination_approach_mi = ctx
            .world
            .facility_approach_miles(approach_city, approach_location)
            .ok()
            .flatten();
        let destination_label = if phase == DRIVE_PHASE_PICKUP {
            job.origin_facility_text()
        } else {
            job.destination_facility_text()
        };
        // Whose vehicle code governs a same-city facility approach: the
        // route's own legs carry no state on a synthetic approach, so the
        // destination city answers for it. A real corridor run ignores this
        // and reads the state under the wheels per leg.
        let local_city = if phase == DRIVE_PHASE_PICKUP {
            &job.origin
        } else {
            &job.destination
        };
        let local_state = ctx
            .world
            .city(local_city)
            .map(|city| city.state.clone())
            .unwrap_or_default();
        let mut trip = Trip::new(
            route.clone(),
            truck,
            weather,
            TripOptions {
                time_scale: ctx.settings.time_scale,
                seed: Some(trip_seed),
                start_hour: trip_start_hour,
                imperial: ctx.settings.imperial_units,
                hazard_scale,
                career_hours: Some(profile_of(ctx).game_hours),
                traffic_provider: live_traffic_provider(ctx),
                parking_provider: live_parking_provider(ctx),
                bobtail: job.bobtail,
                destination_label,
                destination_approach_mi,
                local_state,
                ..Default::default()
            },
        );
        // How often a trooper pulls this driver in for a routine inspection
        // rides the safety record from the first mile.
        {
            let (scale, blitz) = crate::states::driving_enforcement::roadside_inspection_scale_for(
                ctx,
                trip.truck.damage_pct,
            );
            trip.roadside_inspection_scale = scale;
            trip.roadcheck_blitz = blitz;
        }
        if ctx.settings.time_scale == 1.0 && start_hour.is_none() {
            let local_hour = profile_of(ctx).calendar_game_hours().rem_euclid(24.0);
            let reference_hour = (local_hour - trip.start_timezone.offset_h).rem_euclid(24.0);
            trip.start_hour = reference_hour;
            trip.traffic_manager.start_hour = reference_hour;
        }
        if phase == DRIVE_PHASE_DELIVERY {
            // The destination exit, ramp terminal, and street chain own the
            // arrival speeds now. The trip's legacy last-miles arrival zones
            // were already silenced as freeway chatter, but they stayed
            // enforceable -- a silent 35 under a spoken 65, writing real
            // speeding fines on the final highway miles (owner-hit on I-10).
            // The gate's own 15 comes back the moment the destination exit is
            // taken (_post_gate_zone): by then the last half mile is the
            // facility's driveway rather than the interstate, and the pre-gate
            // warning has to be naming a limit that is really in force.
            trip.zones.retain(|zone| {
                zone.reason != "destination approach" && zone.reason != "facility gate"
            });
        }
        // `_reset_turn_state_for_trip`: the cue latches belong to one route.
        trip.controlled_turn = false;

        let weather_name = trip.weather.current.name();
        let day_music_sequence =
            select_drive_music_sequence(&MusicRoute(&route), trip_seed, 12.0, weather_name);
        let night_music_sequence =
            select_drive_music_sequence(&MusicRoute(&route), trip_seed, 0.0, weather_name);
        let music_night = is_night(trip.local_start_hour());

        let mut catalog: Vec<RadioStation> = default_radio_catalog().to_vec();
        let channel3000 = channel3000_for_drive(
            trip_seed,
            trip.local_start_hour(),
            initial_airtime_s(trip_seed),
        );
        if !channel3000_playable(channel3000.as_ref()) {
            catalog.retain(|station| !DrivingState::is_channel3000(station));
        }
        let community = load_community_stations(&crate::community_stations::cache_path());
        catalog.extend(new_community_stations(&catalog, community));
        catalog.extend(load_personal_playlists(&personal_playlists_dir()));
        let radio = RadioState::from_settings(
            catalog,
            &RadioSettingsView(&ctx.settings),
            &profile_of(ctx).radio_favorites,
        );

        let tutorial: Option<Box<dyn Instructor>> = if profile_of(ctx).tutorial_done {
            None
        } else {
            Some(Box::new(Tutorial::new()))
        };

        // Dead-man's-curve strips: fixed road furniture ahead of each hairpin.
        let transverse_strip_miles: Vec<f64> = trip
            .curves
            .iter()
            .filter(|curve| !curve.connector && (curve.advisory_mph as f64) <= HAIRPIN_ADVISORY_MPH)
            .map(|curve| (curve.start_mi.min(curve.end_mi) - STRIP_LEAD_MI).max(0.05))
            .collect();

        let mut road_texture_rng = seeded_rng(seed_arg, 0x5EA7);
        let next_joint_distance_m = road_texture_rng.uniform(14.0, 18.0);

        let damage_band = trip.truck.damage_band();
        let engine_on = trip.truck.engine_on;
        let air_ready = trip.truck.air_ready();
        let air_low_warning = trip.truck.air_low_warning();
        let spring_brakes_active = trip.truck.spring_brakes_active();
        let status_text = format!("Press {} to start the engine.", ctx.control_hint("engine"));

        DrivingState {
            job,
            route: route.clone(),
            phase,
            trip_seed,
            resumed: false,
            trip,
            trip_generation: 0,
            start_damage,
            start_tire_wear,
            start_brake_wear,
            start_engine_wear,
            rig_buffs: RigBuffs::new(),
            stop_visit: Default::default(),
            drove_automatic: false,
            weather_source_real: ctx.settings.real_weather,
            alerts_next_poll_mi: 0.0,
            alerts_pending: None,
            alerts_said: std::collections::HashSet::new(),
            live_weather_controls_calendar: ctx.settings.live_weather_controls_calendar,
            traffic_source_real: ctx.settings.real_traffic,
            parking_source_real: ctx.settings.real_parking,
            lane: LaneKeeping::new(Some(trip_seed)),
            day_music_sequence,
            night_music_sequence,
            music_night,
            radio,
            radio_station_id: String::new(),
            radio_playlist: Vec::new(),
            radio_playing_key: String::new(),
            radio_track_index: 0,
            radio_elapsed_s: 0.0,
            radio_break_queue: Vec::new(),
            radio_break_pos: 0,
            radio_break_count: 0,
            radio_tracks_since_break: 0,
            synth_music_applied: None,
            radio_track_len: None,
            // The stations were already on the air before this drive began.
            radio_airtime_s: initial_airtime_s(trip_seed),
            channel3000,
            channel3000_serial: 0,
            playlist_positions: HashMap::new(),
            playlist_shuffle: HashMap::new(),
            playlist_wait_s: 0.0,
            playlist_stream_tries: 0,
            playlist_stream_skips: 0,
            playlist_silence_spoken: HashSet::new(),
            radio_signal_timer: 0.0,
            radio_reconnect_timer: 0.0,
            radio_now_playing: None,
            radio_powered: engine_on,
            radio_signal_factor: 1.0,
            radio_fringe_signal: None,
            radio_fringe_freq: 0.0,
            fringe_bed_active: false,
            radio_picket_duck: 1.0,
            picket_duck_s: 0.0,
            picket_wait_s: 0.0,
            fringe_rng: seeded_rng(seed_arg, 0x0046_524D),
            tutorial,
            hos_fine_count: 0,
            hos_stop_check_key: None,
            hos_stop_warning_pending: None,
            hos_plan_hint_check_key: None,
            hos_plan_hint_pending: None,
            enforcement_events: HashSet::new(),
            out_of_service_count: 0,
            drowsy_said: false,
            severe_said: false,
            fatigue_cue_gm: 0.0,
            microsleep_deadline: None,
            microsleep_gm: 0.0,
            microsleep_cooldown_gm: 0.0,
            microsleep_misses: 0,
            hazard_deadline: None,
            hazard_names: Vec::new(),
            horn_scare_tried: false,
            hazard_slow_hint_said: false,
            automatic_braking_announced: false,
            automatic_braking_escalated: false,
            aeb_brake: 0.0,
            aeb_emergency: false,
            aeb_hold_s: 0.0,
            aeb_losing_s: 0.0,
            aeb_decel_mps2: 0.0,
            aeb_last_speed_mps: None,
            last_event_message: String::new(),
            last_cb_chatter: None,
            speed_announce_timer: 0.0,
            last_announced_mph: 0.0,
            enforced_limit_prev: None,
            limit_drop_grace_s: 0.0,
            limit_drop_throttle_exempt_s: 0.0,
            overspeed_active: false,
            overspeed_chime_timer: 0.0,
            construction_seen: false,
            traffic_seen: false,
            billboard_watch: BillboardWatch::now(ctx, start_damage),
            pool_billboards_heard: 0,
            brake_squeal_cooldown_s: 0.0,
            hydro_active: false,
            jake_slip_active: false,
            jake_selected_stage: JAKE_STAGES,
            chains_fast_active: false,
            chain_law_warned: HashSet::new(),
            chain_law_cited: HashSet::new(),
            curve_warned_mi: None,
            speeding_tickets: 0,
            ticket_fines_paid: 0.0,
            jake_zone_fines: 0,
            jake_fines_paid: 0.0,
            jake_violation_deadline_s: None,
            jake_citation_latched: false,
            jake_zone_grace_used: HashSet::new(),
            jake_zone_warned_key: None,
            assist_zone_cue_key: None,
            pull_over: None,
            pull_over_start_mi: 0.0,
            pull_over_signaled: false,
            pull_over_over: 0.0,
            pull_over_limit: 0.0,
            pull_over_kind: "speeding".to_string(),
            pull_over_title: "Traffic stop".to_string(),
            pull_over_summary: String::new(),
            pull_over_fine: 0.0,
            pull_over_reputation_hit: 0.0,
            pull_over_return: "Back on the highway. Watch your speed.".to_string(),
            pull_over_construction_zone: false,
            pull_over_warning_level: 0,
            failure_to_stop_count: 0,
            pull_over_grace_s: 0.0,
            pull_over_forced_s: 0.0,
            pull_over_run_s: 0.0,
            record_events: Vec::new(),
            fatigue_events: 0,
            weigh_station_noticed: HashSet::new(),
            weigh_station_reminder_key: String::new(),
            weigh_station_reminder_age_s: 0.0,
            scale_reminder_held_by_game: HashSet::new(),
            scale_reminder_late_by_driver: HashSet::new(),
            scale_reannounce: None,
            weigh_station_pending: None,
            weigh_station_transponder_verdict: HashMap::new(),
            traced_jake_stage: -1,
            unsafe_damage_stop_key: String::new(),
            pull_over_compliance: 0.0,
            pull_over_elapsed: 0.0,
            pull_over_prev_mph: 0.0,
            pull_over_coast_s: 0.0,
            pull_over_signal_boost: false,
            pull_over_nosignal_hit: false,
            road_texture_rng,
            siren: SirenLoop::new(),
            over_limit_mi: 0.0,
            closed_up_mi: 0.0,
            enforcement_prev_mi: 0.0,
            passed_post_ids: HashSet::new(),
            marked_post_ids: HashSet::new(),
            tableau_siren_ids: HashSet::new(),
            tableau_pass_ids: HashSet::new(),
            scale_bed_key: String::new(),
            scale_bed_volume: 0.0,
            radio_cue_duck: 1.0,
            radio_cue_duck_s: 0.0,
            radio_cut_for_stop: false,
            pending_sounds: Vec::new(),
            deferred_post_ids: HashSet::new(),
            held_observation: None,
            pacing_mi: HashMap::new(),
            post_id_scratch: String::new(),
            rescue_offered: false,
            damage_band,
            worst_damage_band: damage_band,
            limp_cap_mph: None,
            limp_cruise_said: false,
            out_of_service_creep_s: 0.0,
            recovering: false,
            maintenance_levels: [0; 3],
            maintenance_pending_levels: [0; 3],
            cargo_cue_at: 0.0,
            cargo_coaching_said: false,
            signal_timer: 0.0,
            exit_stop: None,
            selected_stop_key: None,
            selected_stop_break: false,
            selected_stop_assist_armed: false,
            selected_stop_assist_said: false,
            selected_stop_assist_brake: 0.0,
            exit_signal_on: false,
            exit_signal_canceled: false,
            canceled_exit_key: None,
            exit_lane_entered: false,
            exit_taper_said: false,
            exit_cancel_armed: false,
            exit_right_hold_s: 0.0,
            exit_right_taps: 0,
            exit_tap_hint_said: false,
            exit_countdown_said: Vec::new(),
            ramp_mi: None,
            ramp_layout: None,
            decel_lane_brake: 0.0,
            decel_lane_assist_said: false,
            ramp_stop: None,
            ramp_end_said: false,
            ramp_arrival_grace_s: 0.0,
            ramp_terminal_miss_count: 0,
            ramp_control: String::new(),
            ramp_light_profile: 0,
            ramp_light_offset_s: 0.0,
            ramp_light_timer: 0.0,
            ramp_light_announced: false,
            ramp_light_last_phase: String::new(),
            ramp_terminal_done: false,
            ramp_waiting_at_light: false,
            ramp_waiting_at_sign: false,
            cross_bubble: None,
            ramp_creep_prompt_said: false,
            ramp_creep_prompt_gap_mi: 0.0,
            ramp_gap_milestones_said: HashSet::new(),
            ramp_bar_tick_timer: 0.0,
            bar_solid_on: false,
            ramp_assist_said: false,
            ramp_green_roll_said: false,
            ramp_assist_brake: 0.0,
            approach_pull_ahead: false,
            approach_pull_ahead_canceled: false,
            street_bar_mi: None,
            street_control_kind: String::new(),
            street_controls_played: HashSet::new(),
            street_controls_trip: 0,
            street_light_split: None,
            street_controls_on:
                crate::states::driving_events::street_controls::STREET_CONTROLS_IN_PLAY,
            critical_curve: None,
            critical_call_age_s: 0.0,
            critical_respeak_at: None,
            destination_exit_taken: false,
            destination_arrival_active: false,
            destination_assist_brake: 0.0,
            destination_chain_ahead: None,
            missed_destination_exit_said: false,
            destination_exit_announced_key: String::new(),
            destination_exit_response_s: 0.0,
            surface_chain: false,
            stop_chain: None,
            stop_chain_end_said: false,
            stop_streets_on: crate::states::driving_events::chains::STOP_STREETS_IN_PLAY,
            highway_trip: None,
            departure_chain: false,
            departure_checked: false,
            ladder_leg_index: -1,
            destination_exit_cache: None,
            destination_exit_labeled: None,
            cruise_mph: None,
            cruise_working_mph: None,
            cruise_held_mph: None,
            cruise_held_reason: String::new(),
            cruise_throttle: 0.0,
            cruise_applied: 0.0,
            cruise_trim: 0.0,
            cruise_jake_stage: 0,
            cruise_jake_cooldown_s: 0.0,
            cruise_jake_reverse_s: 0.0,
            cruise_jake_last_step: 0,
            cruise_snubbing: false,
            pcc_phase: String::new(),
            pcc_cue_s: 0.0,
            climb_cue_said: false,
            climb_cue_s: 0.0,
            climb_beaten_s: 0.0,
            descent_said_mph: None,
            descent_safe_mph: None,
            descent_safe_key: None,
            descent_posted_cap_mph: None,
            trailer_refused: false,
            trailer_repaired: false,
            visible_trailer_defect: (String::new(), f64::NEG_INFINITY),
            nice_speed_mi: 0.0,
            double_nickel_mi: 0.0,
            jake_descent_mi: 0.0,
            radio_states_station: String::new(),
            radio_states_held: HashSet::new(),
            cruise_descent_mph: None,
            cruise_exit_mph: None,
            cruise_curve_mph: None,
            cruise_curve_end_mi: None,
            cruise_resume_after_mi: None,
            grade_warned_sign: 0,
            grade_scan_mi: -1e9,
            speed_control_armed: false,
            speed_control_paused_at_stop: false,
            speed_control_transit_pause: false,
            speed_control_stop_honored: false,
            speed_control_target_mph: None,
            acc_following: false,
            acc_weather_gap_said: false,
            acc_limit_capped: false,
            acc_limit_cap_said: None,
            acc_weather_cap_said: None,
            construction_slowdown: None,
            construction_taper_stage: None,
            acc_limit_hold: None,
            acc_follow_cue_s: 0.0,
            descent_control_active: false,
            descent_limit_state: String::new(),
            descent_beaten_s: 0.0,
            descent_capture_active: false,
            assist_exit_slowing_said: false,
            lane_keeping_grant_said: false,
            lane_keeping_takes_exit_said: false,
            curve_assist_active: false,
            curve_assist_cue_s: 0.0,
            curve_assist_spoke: false,
            curve_servo: None,
            curve_servo_declined_to_mi: None,
            transition_assist_active: false,
            keeper_mph: None,
            keeper_throttle: 0.0,
            keeper_zone: String::new(),
            speed_control_floor_said: false,
            keeper_zone_limit: None,
            keeper_ease_said: None,
            keeper_ease_target: None,
            keeper_held_mph: None,
            keeper_held_reason: String::new(),
            keeper_snub: 0.0,
            keeper_snub_target_mph: 0.0,
            keeper_droop_s: 0.0,
            keeper_droop_said: false,
            keeper_droop_cue_s: 0.0,
            keeper_overrun_s: 0.0,
            keeper_overrun_said: false,
            arrival_stop_said: false,
            arrival_full_stop_said: false,
            arrival_menu_open: false,
            gate_reminder_s: 0.0,
            gate_speed_warned: false,
            gate_warning_spoken: false,
            gate_grace_s: 0.0,
            gate_miss_count: 0,
            wrong_way_mi: 0.0,
            wrong_way_said_at: 0.0,
            traffic_sounds: [None, None, None],
            traffic_bed_volume: 0.0,
            traffic_ramp_rolled_ft: 0.0,
            lane_gap_watch: None,
            lane_gap_prev_lane: Some(0),
            lane_gap_blocker_key: None,
            lane_gap_blocker_class: String::new(),
            lane_gap_said_keys: HashSet::new(),
            lane_gap_cue_s: 0.0,
            turn_miss_count: 0,
            turn_trip_id: 0,
            turn_advised: HashSet::new(),
            turn_missed: HashSet::new(),
            turn_resolved: HashSet::new(),
            turn_announced: HashSet::new(),
            turn_called_now: HashSet::new(),
            turn_grace_s: 0.0,
            air_ready_said: air_ready,
            low_air_said: air_low_warning,
            // New drives start clear so a tank already under the line still
            // earns one cue; resumed drives re-derive the latch below.
            low_fuel_said: false,
            fuel_range_short_said: false,
            spring_brake_said: spring_brakes_active,
            brake_lockout_cue_timer: 0.0,
            brake_air_hissed: false,
            pending_low_air_buzzer: false,
            brake_peak_application: 0.0,
            overrev_s: 0.0,
            overrev_warn_due: OVERREV_GRACE_S,
            lane_change_target: None,
            lane_change_timer: 0.0,
            lane_signal_timer: 0.0,
            passing: None,
            pass_returning: false,
            merge_deadline: None,
            departure_ramp_mi: None,
            departure_merge_road_mph: 0.0,
            departure_cruise_handoff_mph: None,
            departure_merge_recovery: false,
            clock_pacing: Default::default(),
            lane_count_seen: None,
            lane_before_narrow: None,
            merge_taper_warned: None,
            hazard_dodgeable: false,
            hazard_in_lane: false,
            hazard_lead_mph: None,
            hazard_lane: 0,
            left_lane_s: 0.0,
            keep_right_nags: 0,
            ambient_event_cooldown_s: 0.0,
            pending_ambient_events: VecDeque::new(),
            road_joint_accumulator_m: 0.0,
            next_joint_distance_m,
            lane_guidance: LaneGuidance::new(),
            edge_loop_key: None,
            road_pan_applied: None,
            lane_guide_tone_on: false,
            lane_guide_pan_applied: 0.0,
            turn_guide: TurnGuide::new(),
            engine_guide_pan_applied: None,
            transverse_strip_miles,
            transverse_fired: Vec::new(),
            lane_locator_on: false,
            lane_locator_timer: 0.0,
            steer_cue_active: false,
            exit_blinker_active: false,
            steer_cue_timer: 0.0,
            steer_cue_hold_s: 0.0,
            curve_run: None,
            cross_repeat_s: 0.0,
            sideswipe_cooldown_s: 0.0,
            road_position_band: None,
            reverse_cue_active: false,
            air_cue_active: false,
            jake_cue_key: None,
            jake_cue_idle_s: 0.0,
            curve_assist_jake: false,
            auto_jake: false,
            auto_jake_enabled: true,
            resume_target_mph: None,
            auto_jake_hold_mph: None,
            auto_jake_cooldown_s: 0.0,
            auto_jake_reverse_s: 0.0,
            auto_jake_last_step: 0,
            shift_recover_t: 1.0,
            shift_hold_rpm: None,
            manual_engage_clunk_pending: false,
            engine_audio_throttle: 0.0,
            reverse_brake_held: false,
            reverse_accel_held: false,
            direction_armed: String::new(),
            direction_hold_s: 0.0,
            brake_latch: PedalLatch::new(),
            pacenote_spoken: HashSet::new(),
            status_text,
            liquid_audio_ok: None,
            liquid_wash_on: false,
            liquid_lateral_cooldown_s: 0.0,
            liquid_surge_said: false,
            liquid_settled_said: false,
            liquid_settle_timer_s: 0.0,
            entered_once: false,
        }
    }
}

// -- live-data providers ------------------------------------------------------------
//
// The Python constructor took these straight off the context
// (`ctx.real_weather_provider()`, `ctx.real_traffic_provider()`,
// `ctx.truck_parking_provider()`), which caches one of each for the whole
// session. `GameContext` only hands out borrows, and a `Trip`/`WeatherSystem`
// has to OWN its provider, so a drive builds its own here when the setting is
// on. Same class, same transport, same behaviour on the road; what is lost is
// the cross-trip cache, which the shell can hand back by exposing `Arc` clones
// (see the task report).

fn live_weather_provider(ctx: &GameContext) -> Option<Box<dyn WeatherProvider>> {
    if !ctx.settings.real_weather {
        return None;
    }
    Some(Box::new(RealWeatherProvider::with_nws(Arc::new(
        UreqTransport,
    ))))
}

fn live_traffic_provider(ctx: &GameContext) -> Option<Arc<dyn TrafficProvider>> {
    if !ctx.settings.real_traffic {
        return None;
    }
    Some(Arc::new(RealTrafficProvider::new(Arc::new(UreqTransport))))
}

fn live_parking_provider(ctx: &GameContext) -> Option<Arc<TruckParkingProvider>> {
    if !ctx.settings.real_parking {
        return None;
    }
    Some(Arc::new(TruckParkingProvider::new(Arc::new(UreqTransport))))
}
