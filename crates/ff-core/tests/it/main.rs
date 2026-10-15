//! Every integration test in this crate, in ONE binary.
//!
//! Cargo links and runs a separate binary for each `tests/*.rs`, and runs
//! those binaries one after another. Collapsing them here means one link
//! and one process, with the tests running as parallel threads inside it.
//! Files live in `tests/it/`, which cargo does not auto-discover, so this
//! file is the only target and the `mod` lines below are what includes
//! them. A new test file needs a line here.
//!
//! TWO FILES ARE DELIBERATELY NOT HERE. data_baked and data_map_correction
//! each point the whole process at a different baked data root through
//! FREIGHT_FATE_DATA_ROOT, and the loader caches what it finds, so in one
//! process whichever ran first decided what the other one saw. They keep a
//! binary each, which is what process-global state costs.

mod data_support;
mod sim_support;

mod data_city_keys;
mod data_curve_management;
mod data_facility_approaches;
mod data_facility_endpoints;
mod data_interchanges;
mod data_lane_data;
mod data_leg_road_agreement;
mod data_local_approaches;
mod data_local_geometry;
mod data_regions;
mod data_service_plazas;
mod data_stop_access;
mod data_stop_exits;
mod data_street_chain;
mod data_street_turns;
mod data_surface_streets;
mod data_world;
mod data_world_overlay;
mod profile_integrity_export;
mod sim_bends_hold_their_advisory;
mod sim_billboard_facing;
mod sim_billboard_placement;
mod sim_chain_law;
mod sim_congestion;
mod sim_enforcement_presence;
mod sim_enforcement_vocabulary;
mod sim_facility_approaches;
mod sim_interchanges;
mod sim_lane_straighten;
mod sim_limit_lookahead;
mod sim_maxspeed;
mod sim_multilane_speech;
mod sim_real_construction_zones;
mod sim_scale_check_in_guidance;
mod sim_speech_clocks;
mod sim_steer_build;
mod sim_street_detail;
mod sim_traffic_bubble;
mod sim_traffic_manager;
mod sim_trip_cues;
mod sim_trip_properties;
mod sim_trip_resume;
mod sim_troopers;
mod sim_vehicle_access;
mod sim_weigh_station_transponder;
