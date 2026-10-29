//! The adversarial battery's scenarios (port of
//! `tools/playtest_break_scenarios/`).
//!
//! Python split these by system family across nine files, each registering
//! itself with a decorator. Here the registry is one explicit table,
//! [`SCENARIOS`], and the scenario bodies live in submodules of the same
//! names. A new scenario is a function plus a row.
//!
//! # What replaced the monkeypatches
//!
//! Python reached the road a scenario wanted by replacing methods on the live
//! trip. Rust has no seam for an inherent method, so each of those becomes
//! the ROAD that answers the way the patch did:
//!
//! | Python | here |
//! |---|---|
//! | `trip.grade_at = lambda mile: g` | [`force_grade`][super::breaker::force_grade] bakes one `GradeSegment` over every leg |
//! | `weather.update = lambda *a: None` | `weather.forced`, the lock the game already has |
//! | `ctx._event_pacer = EventSpeechPacer(clock=...)` | the rig's own [`FakeClock`][crate::app::testing::FakeClock], advanced per frame |
//! | `pygame.key.get_pressed` | the app's real `HeldKeys` |
//! | `ctx.push_state(rig.d)` | [`Rig::with_drive_on_stack`][super::breaker::Rig::with_drive_on_stack] |
//! | a private `_method()` call | the menu row that calls it, chosen by name |
//!
//! Where a Python check reached a private helper that has no public Rust
//! surface, the port says so at the check rather than dropping it silently.

pub mod assists;
pub mod career_economy;
pub mod dispatch_saveload;
pub mod driving_physics;
pub mod enforcement;
pub mod radio_weather;
pub mod resources;
pub mod settings_misc;
pub mod text;
pub mod today_1_9;
pub mod unlawful;

use super::breaker::Scenario;

/// Every scenario, in run order.
pub static SCENARIOS: &[Scenario] = &[
    Scenario {
        name: "floor_it_through_town",
        description: "Hold the floor through urban speed zones; every dollar it costs must \
                      come from an officer who was audibly there.",
        run: driving_physics::floor_it_through_town,
    },
    Scenario {
        name: "hairpin_at_70_no_assists",
        description: "Take a 25-mph hairpin at 70 with every assist off; does anything \
                      push back?",
        run: driving_physics::hairpin_at_70_no_assists,
    },
    Scenario {
        name: "reverse_down_the_route",
        description: "Engage reverse and back down the interstate; position must clamp, \
                      someone should object.",
        run: driving_physics::reverse_down_the_route,
    },
    Scenario {
        name: "slam_reverse_at_speed",
        description: "Manual box: grab reverse at 60 mph; a real driveline would grenade.",
        run: driving_physics::slam_reverse_at_speed,
    },
    Scenario {
        name: "neutral_coast_mountain",
        description: "Slam neutral on a 6% descent and ride it; how fast does it get, and \
                      does anything object?",
        run: driving_physics::neutral_coast_mountain,
    },
    Scenario {
        name: "dynamite_parking_brake_at_60",
        description: "Pull the parking brake valve at 60: flat-spots, no waiting \
                      fast-forward, honest speech.",
        run: driving_physics::dynamite_parking_brake_at_60,
    },
    Scenario {
        name: "redline_damage_readout",
        description: "Force a road-driven over-rev; the warning quotes a damage number that \
                      must be honest.",
        run: driving_physics::redline_damage_readout,
    },
    Scenario {
        name: "assists_fight_descent",
        description: "Cruise + curve assist + descent control on a 6% grade with bends; \
                      count cue spam.",
        run: assists::assists_fight_descent,
    },
    Scenario {
        name: "jake_toggle_fine_dodge",
        description: "Toggle the jake off at each warning in a ban zone: fines dodged \
                      forever, warnings spam.",
        run: assists::jake_toggle_fine_dodge,
    },
    Scenario {
        name: "gate_overshoot_with_assists",
        description: "Carry past the facility gate at 70 with the approach assist off, then \
                      on; verify both.",
        run: assists::gate_overshoot_with_assists,
    },
    Scenario {
        name: "ramp_speed_control_handback",
        description: "Signal early at 20x, honor the ramp's stop bar, drive on: no early \
                      shed, and speed control returns unaided.",
        run: assists::ramp_speed_control_handback,
    },
    Scenario {
        name: "scale_check_in_guidance",
        description: "Follow the scale announcement literally -- T mid-warning, then X: both \
                      must serve the scale.",
        run: enforcement::scale_check_in_guidance,
    },
    Scenario {
        name: "bald_tires_get_a_walk_around",
        description: "Roll past staffed commercial-vehicle units on bald tires; one must see the \
                      tread, pull the truck in for a Level 2, write the tire up and replace it.",
        run: enforcement::bald_tires_get_a_walk_around,
    },
    Scenario {
        name: "scale_pull_over_stands_down_exit",
        description: "Blow past the scale with a truck-stop exit armed: the pull-over must \
                      own the road.",
        run: enforcement::scale_pull_over_stands_down_exit,
    },
    Scenario {
        name: "fuel_rescue_farming",
        description: "Run dry three times as a company driver, once as an owner-op; is the \
                      rescue farmable?",
        run: resources::fuel_rescue_farming,
    },
    Scenario {
        name: "hos_marathon_and_rest_cheese",
        description: "22-hour drive, micro-rest cheese, and the in-cab waiting clock vs the \
                      HOS ledger.",
        run: resources::hos_marathon_and_rest_cheese,
    },
    Scenario {
        name: "microsleep_throttle_through",
        description: "Fatigue 100, never react, keep the floor down; does the forced stop \
                      actually stop you?",
        run: resources::microsleep_throttle_through,
    },
    Scenario {
        name: "hos_rest_minute_cheese",
        description: "In violation, rest one minute thirty times; split-sleeper credit must \
                      not double-dip.",
        run: resources::hos_rest_minute_cheese,
    },
    Scenario {
        name: "motel_rest_deadline_crunch",
        description: "Take a 10-hour motel room with 1 minute left to deliver; the game must \
                      not paper over it.",
        run: resources::motel_rest_deadline_crunch,
    },
    Scenario {
        name: "settlement_spoken_balance",
        description: "Delivery carrying a balance owed larger than the load pays; the spoken \
                      ledger must match the money, and the run must still pay the driver.",
        run: career_economy::settlement_spoken_balance,
    },
    Scenario {
        name: "abandon_and_advance_cycle",
        description: "Take a pay advance, abandon at 99% complete; the cycle must never mint \
                      money.",
        run: career_economy::abandon_and_advance_cycle,
    },
    Scenario {
        name: "endorsement_wallet_edges",
        description: "Buy a course broke, then with the exact dollar; refusals and balances \
                      must be honest.",
        run: career_economy::endorsement_wallet_edges,
    },
    Scenario {
        name: "credential_ladder_gates",
        description: "Book courses past their level, prerequisite, and background-check \
                      gates; every refusal must be named and the TSA wait must be real.",
        run: career_economy::credential_ladder_gates,
    },
    Scenario {
        name: "money_exact_zero_and_below",
        description: "Fines that land the balance on exactly $0, then below; escalation and \
                      ledger must agree.",
        run: career_economy::money_exact_zero_and_below,
    },
    Scenario {
        name: "level_up_at_settlement_boundary",
        description: "XP one point under a level threshold, settle a delivery; the level-up \
                      must land exactly.",
        run: career_economy::level_up_at_settlement_boundary,
    },
    Scenario {
        name: "owner_op_buyin_at_level_18_boundary",
        description: "Buy into owner-operator at exactly level 18 with exact capital; refuse \
                      it at level 17.",
        run: career_economy::owner_op_buyin_at_level_18_boundary,
    },
    Scenario {
        name: "short_hop_streak_xp_farming",
        description: "Chain board-minimum on-time deliveries; the streak may double the \
                      road lesson and never mint XP off the flat completion award.",
        run: career_economy::short_hop_streak_xp_farming,
    },
    Scenario {
        name: "dispatch_decline_budget",
        description: "Decline assigned loads until dispatch runs dry; re-enter the board \
                      hunting a reroll.",
        run: dispatch_saveload::dispatch_decline_budget,
    },
    Scenario {
        name: "save_scum_enforcement",
        description: "Save and reload during a traffic stop and a live hazard; do the \
                      consequences survive?",
        run: dispatch_saveload::save_scum_enforcement,
    },
    Scenario {
        name: "chains_on_dry_interstate_at_70",
        description: "Leave the chains on and run 70 on bare pavement: wear math, snap \
                      event, spoken truth.",
        run: radio_weather::chains_on_dry_interstate_at_70,
    },
    Scenario {
        name: "glare_ice_full_jake",
        description: "Stage-3 jake in a low gear on glare ice: the drive axle must slide, \
                      and say so.",
        run: radio_weather::glare_ice_full_jake,
    },
    Scenario {
        name: "chain_law_citation_balance",
        description: "Roll through a Level 2 chain law bare; citation, spoken balance, and \
                      tire claims checked.",
        run: radio_weather::chain_law_citation_balance,
    },
    Scenario {
        name: "radio_dial_abuse_offline",
        description: "Spin the dial, favorite it, tune through a dead handover, then \
                      teleport 1,200 miles.",
        run: radio_weather::radio_dial_abuse_offline,
    },
    Scenario {
        name: "settings_flips_mid_drive",
        description: "Flip transmission and units at 55 mph; the cab must announce and \
                      switch everywhere.",
        run: settings_misc::settings_flips_mid_drive,
    },
    Scenario {
        name: "waiting_time_warp",
        description: "Deliberate parked waiting: the double-speed clock must bill time \
                      honestly.",
        run: settings_misc::waiting_time_warp,
    },
    Scenario {
        name: "hazard_ignored_to_100_damage",
        description: "No AEB, never brake for hazards; collision math, spoken damage, and \
                      what 100% allows.",
        run: settings_misc::hazard_ignored_to_100_damage,
    },
    Scenario {
        name: "abandon_an_empty_run_costs_nothing",
        description: "Shane's bobtail turnaround: no load means no contract, so calling it \
                      off can cost no money.",
        run: today_1_9::abandon_an_empty_run_costs_nothing,
    },
    Scenario {
        name: "jake_stays_off_where_it_does_not_belong",
        description: "Brandon's climb report: the retarder holds a load back downhill and \
                      does nothing else.",
        run: today_1_9::jake_stays_off_where_it_does_not_belong,
    },
    Scenario {
        name: "named_hazards_keep_their_frequency",
        description: "Brandon's naming asks: debris and animals say what they are, and are \
                      no more common than before.",
        run: today_1_9::named_hazards_keep_their_frequency,
    },
    Scenario {
        name: "weather_cap_releases_when_the_sky_does",
        description: "Fight the storm cap with the cruise key, then clear the sky: the cap \
                      must hold, then let go.",
        run: today_1_9::weather_cap_releases_when_the_sky_does,
    },
    Scenario {
        name: "brake_lights_never_invent_a_cause",
        description: "Brandon's why: a slowdown names its cause only where the road really \
                      has one, and never guesses.",
        run: today_1_9::brake_lights_never_invent_a_cause,
    },
    Scenario {
        name: "speeding_past_staffed_posts",
        description: "Hold twenty over past a row of staffed crossovers; the citation must \
                      price itself the same way the model does.",
        run: unlawful::speeding_past_staffed_posts,
    },
    Scenario {
        name: "scale_bypass_to_the_end",
        description: "Blow an open scale and ride the failure-to-stop ladder out; every line \
                      must still be true when it is spoken.",
        run: unlawful::scale_bypass_to_the_end,
    },
    Scenario {
        name: "work_zone_speeding_doubles_the_fine",
        description: "The same overspeed inside roadwork must cost exactly twice what it \
                      costs anywhere else.",
        run: unlawful::work_zone_speeding_doubles_the_fine,
    },
    Scenario {
        name: "repeat_citations_stop_compounding",
        description: "A career offender's fines must climb to the cap and then stop, not \
                      compound forever.",
        run: unlawful::repeat_citations_stop_compounding,
    },
    Scenario {
        name: "honk_the_air_down_to_the_valve",
        description: "Lean on the horn with the compressor gone: the protection valve must \
                      save the brakes, not the horn.",
        run: unlawful::honk_the_air_down_to_the_valve,
    },
    Scenario {
        name: "prepass_green_is_not_a_bypass_charge",
        description: "A weigh-in-motion green light must clear the scale; driving on cannot \
                      then be charged as a bypass.",
        run: unlawful::prepass_green_is_not_a_bypass_charge,
    },
];
