//! Trip hazard, GPS cue, toll, and city-event tests (port of
//! `tests/test_trip_cues.py`).

use std::collections::HashMap;

use crate::sim_support::*;
use ff_core::data::state_welcome::welcome_sign;
use ff_core::data::world_models::Route;
use ff_core::pyrandom::PyRandom;
use ff_core::sim::traffic_manager::{BrakingZone, TrafficManager, TrafficVehicle};
use ff_core::sim::trip::{Trip, TripOptions, ZONE_WARNING_MIN_MI};
use ff_core::sim::trip_models::{
    eligible_hazards, NPCVehicle, NavigationCue, TrafficPressure, TripEventKind, Zone,
    FACILITY_GATE_LIMIT_MPH, HAZARDS,
};
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::{WeatherKind, REGION_WEIGHTS};
use ff_core::speech_text::brake_lights_cue;

fn pool(
    region: &str,
    weather: WeatherKind,
    terrain: &str,
    hour: f64,
) -> HashMap<&'static str, f64> {
    eligible_hazards(region, weather, terrain, hour)
        .into_iter()
        .collect()
}

#[test]
fn test_every_region_has_clear_day_hazards() {
    // Every region always has plausible clear, calm, daytime hazards: the
    // nationwide staples are never filtered out.
    let noon = 12.0;
    let mut regions: Vec<&str> = REGION_WEIGHTS.iter().map(|(r, _)| *r).collect();
    regions.push("atlantis");
    for region in regions {
        let pool = pool(region, WeatherKind::Clear, "flat", noon);
        assert!(pool.contains_key("debris on the road"));
        // No weather- or terrain-specific hazard leaks into a clear flat day.
        let text = pool.keys().cloned().collect::<Vec<_>>().join(" ");
        for word in [
            "snow",
            "ice",
            "fog",
            "crosswind",
            "dust",
            "water",
            "hail",
            "rockfall",
            "tumbleweed",
        ] {
            assert!(
                !text.contains(word),
                "{word:?} should not occur on a clear day"
            );
        }
    }
}

#[test]
fn test_weather_and_terrain_gate_hazards() {
    // Snow hazards only appear when it is snowing.
    let clear = pool("great_lakes", WeatherKind::Clear, "flat", 12.0);
    let snowy = pool("great_lakes", WeatherKind::Snow, "flat", 12.0);
    assert!(!clear
        .keys()
        .any(|t| t.contains("snow") || t.contains("ice")));
    assert!(snowy.keys().any(|t| t.contains("snow")));

    // Rockfall is a mountain-terrain hazard, not a flatland one.
    let flat = pool("rockies", WeatherKind::Clear, "flat", 12.0);
    let mountain = pool("rockies", WeatherKind::Clear, "mountain", 12.0);
    assert!(!flat.contains_key("rockfall debris on the road"));
    assert!(mountain.contains_key("rockfall debris on the road"));

    // The dropped, implausible hazards are gone for good.
    let mut everything: Vec<&str> = Vec::new();
    for (region, _) in REGION_WEIGHTS.iter() {
        for weather in WeatherKind::ALL {
            for terrain in ["flat", "hills", "mountain"] {
                for (t, _) in eligible_hazards(region, weather, terrain, 3.0) {
                    everything.push(t);
                }
            }
        }
    }
    assert!(!everything.iter().any(|t| t.contains("farm equipment")));
    assert!(!everything.iter().any(|t| t.contains("dust devil")));
}

#[test]
fn test_wildlife_is_biased_to_dawn_dusk_and_night() {
    // Deer and elk are far likelier at night than at midday, and the same
    // catalog drives both -- only the time of day changes the weight.
    let day = pool("great_lakes", WeatherKind::Clear, "flat", 12.0);
    let night = pool("great_lakes", WeatherKind::Clear, "flat", 23.0);
    let deer = "a deer crossing the road";
    assert!(night[deer] > day[deer]);
    // Non-animal staples keep the same weight regardless of the hour.
    assert_eq!(night["debris on the road"], day["debris on the road"]);
}

#[test]
fn test_upcoming_stop_only_looks_ahead() {
    let mut trip = make_trip(world(), "Chicago", "Indianapolis", 2);
    let stop = trip.stops[0].clone();
    trip.position_mi = stop.at_mi - 3.0;
    assert_eq!(trip.upcoming_stop(5.0).map(|s| s.key()), Some(stop.key()));
    trip.position_mi = stop.at_mi - 10.0;
    assert!(trip.upcoming_stop(5.0).is_none());
    trip.position_mi = stop.at_mi + 0.1; // just past: the exit is gone
    let next_stop = trip.upcoming_stop(5.0).map(|s| s.key());
    assert_ne!(next_stop, Some(stop.key()));
}

#[test]
fn test_eta_tracks_current_speed() {
    // Regression: the C key's ETA was a constant 55 mph guess that never
    // responded to how fast you were actually going.
    let mut trip = make_trip(world(), "Chicago", "Indianapolis", 2);
    let parked = trip.eta_game_hours(55.0);
    assert!(parked > 0.0);
    trip.truck.velocity_mps = 31.3; // ~70 mph
    let fast = trip.eta_game_hours(55.0);
    trip.truck.velocity_mps = 13.4; // ~30 mph
    let slow = trip.eta_game_hours(55.0);
    assert!(fast < parked && parked < slow); // parked assumes 55 mph, between the two
                                             // parked or crawling falls back to highway pace, never infinity
    trip.truck.velocity_mps = 0.5;
    assert_eq!(trip.eta_game_hours(55.0), parked);
}

#[test]
fn test_progress_summary_mentions_highway() {
    let mut trip = make_trip(world(), "Chicago", "Indianapolis", 2);
    let text = trip.progress_summary(true);
    assert!(text.contains("I-65"), "{text}");
    assert!(text.contains("Indianapolis, Indiana"), "{text}");
    assert!(text.contains("Current grade 0.0 percent, level"), "{text}");
    // The summary reports the nearest upcoming cue; an early stop leads here.
    assert!(text.contains("Next stop"), "{text}");
    let metric = trip.progress_summary(false);
    assert!(metric.contains("kilometers"), "{metric}");

    // Once past that stop, the summary surfaces the upcoming state-line crossing.
    trip.position_mi = 25.0;
    let state_text = trip.progress_summary(true);
    assert!(state_text.contains("Next state line"), "{state_text}");
    assert!(state_text.contains("Illinois into Indiana"), "{state_text}");
}

#[test]
fn test_gps_state_crossing_and_rest_stop_cues_deduplicate() {
    let mut trip = make_trip(world(), "Chicago", "Indianapolis", 2);
    trip.traffic_manager.rolling_bubble = false;
    trip.traffic_manager.vehicles = Vec::new();

    // Read the line's position rather than pinning a number to it. Correcting
    // Chicago-Indianapolis from 183 to the 185 miles its baked route runs moved
    // every along-route position, and a hardcoded probe then lands somewhere
    // else entirely -- next to an interchange, in one case, whose exit cue
    // joined the assertion.
    // State crossings speak once, at the line -- the old 10-mile advance
    // warning was cut in the reduce-repeated-alerts player-feedback round.
    let line_mi = trip.route.legs[0].state_crossings()[0].at_mi;
    trip.position_mi = (line_mi - 10.0).max(0.0);
    let advance = trip.update(0.0);
    let repeat = trip.update(0.0);
    assert!(gps_events(&advance).is_empty());
    assert!(gps_events(&repeat).is_empty());

    trip.position_mi = (line_mi - 1.0).max(0.0);
    let near = trip.update(0.0);
    assert!(gps_events(&near).is_empty());

    trip.position_mi = line_mi;
    let crossing = trip.update(0.0);
    assert_eq!(
        messages_of(&crossing, TripEventKind::StateCrossing),
        vec!["Crossing into Indiana near the I-65 state line south of Hammond."]
    );
    let again = trip.update(0.0);
    assert!(messages_of(&again, TripEventKind::StateCrossing).is_empty());

    // Road stops keep their single actionable announcement from check_stops
    // at five miles; the extra one-mile reminder is gone for the same reason.
    let rest_stop_mi = trip
        .stops
        .iter()
        .find(|s| s.name == "Loves Travel Stop Lafayette")
        .expect("the curated Lafayette stop")
        .at_mi;
    trip.position_mi = rest_stop_mi - 1.0;
    let rest = trip.update(0.0);
    // The dense maxspeed sweep gives this I-65 leg a real 65 mph zone at
    // mile 120; arriving from the 55 zone at the crossing announces that
    // raise. The rest-stop cue still does not re-fire.
    assert_eq!(gps_messages(&rest), vec!["Speed limit raised to 65."]);
}

#[test]
fn test_gps_traffic_cue_deduplicates() {
    let mut trip = make_trip(world(), "Chicago", "Indianapolis", 2);
    trip.navigation_cues.push(NavigationCue::new(
        "traffic:test",
        "traffic",
        10.0,
        "traffic queue ahead at 45 miles per hour",
        "Traffic slowing ahead; target speed 45.",
    ));

    trip.position_mi = 8.5;
    let first = trip.update(0.0);
    let second = trip.update(0.0);

    assert_eq!(
        gps_messages(&first),
        vec!["Traffic slowing ahead in 2 miles; traffic queue ahead at 45 miles per hour."]
    );
    assert!(gps_events(&second).is_empty());
}

#[test]
fn test_toll_cues_and_charges_deduplicate() {
    let mut trip = make_trip(world(), "New York", "Philadelphia", 2);

    // No advance state-crossing chatter -- the line itself will speak when
    // the truck reaches it.
    trip.position_mi = 6.1;
    let crossing = trip.update(0.0);
    assert!(gps_events(&crossing).is_empty());

    trip.position_mi = 7.2;
    let advance = trip.update(0.0);
    let repeat = trip.update(0.0);

    assert_eq!(
        gps_messages(&advance),
        vec![
            "ticket system toll point ahead, New Jersey Turnpike ticket entry. \
             Toll about 18 dollars."
        ]
    );
    assert!(gps_events(&repeat).is_empty());

    trip.position_mi = 9.0;
    let charged = trip.update(0.0);
    let charged_again = trip.update(0.0);

    assert_eq!(
        messages_of(&charged, TripEventKind::TollCharged),
        vec![
            "ticket system toll at New Jersey Turnpike ticket entry, \
             estimated 18 dollars, billed to carrier settlement."
        ]
    );
    assert_eq!(trip.toll_expense(), 18.0);
    assert!(messages_of(&charged_again, TripEventKind::TollCharged).is_empty());
}

#[test]
fn test_non_toll_route_does_not_charge_tolls() {
    let mut trip = make_trip(world(), "Chicago", "Indianapolis", 2);

    trip.position_mi = trip.total_miles();
    let events = trip.update(0.0);

    assert_eq!(trip.toll_expense(), 0.0);
    assert!(messages_of(&events, TripEventKind::TollCharged).is_empty());
}

#[test]
fn test_zero_amount_toll_entry_marker_does_not_record_expense() {
    let mut trip = make_trip(world(), "Philadelphia", "Pittsburgh", 2);

    trip.position_mi = 16.1;
    let advance = trip.update(0.0);
    assert_eq!(
        gps_messages(&advance),
        vec!["ticket system toll point ahead, Pennsylvania Turnpike eastern ticket entry."]
    );

    trip.position_mi = 18.0;
    let entry = trip.update(0.0);
    // Nothing is charged at a ticket entry, and the cue ahead named it.
    assert!(
        gps_messages(&entry).is_empty(),
        "{:?}",
        gps_messages(&entry)
    );
    assert_eq!(trip.toll_expense(), 0.0);
    assert!(messages_of(&entry, TripEventKind::TollCharged).is_empty());
}

#[test]
fn test_traffic_context_and_warning_are_grounded_in_lead_vehicle() {
    let mut trip = make_trip(world(), "Chicago", "Indianapolis", 2);
    trip.truck.velocity_mps = 29.0;
    trip.position_mi = 9.98;
    trip.traffic_manager.rolling_bubble = false;
    // The ROAD carries the reason now, not the vehicle's own label: a jam it
    // is slowing for, covering the mile the lead sits on. Without one, the
    // braking label ends the moment the update runs and the lead is just
    // steady traffic.
    //
    // Put on the TRIP, not straight onto the traffic manager: the manager's
    // braking zones are derived from the trip's, handed over on every update,
    // so a list poked in from the side is gone by the time the vehicles run.
    // The jam starts ahead of the truck so entering it is not part of what
    // this test is about; the approach mile the handover adds is what reaches
    // back over the lead.
    trip.zones
        .push(Zone::new(10.5, 12.0, 45.0, "heavy traffic"));
    // Held DOWN to 45 from a 65 cruise, which is what "braking_traffic"
    // means. This fixture used to give the same number twice -- a vehicle
    // sitting at its own target, so not braking by any reading, while still
    // carrying the braking label. It passed only because the label was taken
    // at its word for the life of the vehicle; now that the label ends when
    // its reason does, the fixture has to depict the thing it is named for.
    trip.traffic_manager.vehicles = vec![TrafficVehicle::from(NPCVehicle::new(
        "npc:queue",
        10.0,
        45.0,
        65.0,
        0,
        "braking_traffic",
    ))];

    let context = trip.traffic_context().expect("a lead in the lane");
    assert_eq!(context.lead.speed_mph, 45.0);
    assert!(context.closing_mph > 15.0);
    assert_eq!(trip.traffic_target_speed(), Some(45.0));

    let events = trip.update(1.0);

    let hazards: Vec<_> = events
        .iter()
        .filter(|e| e.kind == TripEventKind::Hazard)
        .collect();
    assert!(!hazards.is_empty());
    // Pinned whole, call included: this stretch of I-65 out of Chicago has
    // three lanes and no closure, so the lane change is a move the driver can
    // actually make and the dodge call is the right one. A bare "Brake!" here
    // would mean the open-lane test had stopped seeing the road. From the
    // right lane of three, the lane a tap lands in is the middle lane, and
    // the call names it the way the L key does.
    assert_eq!(
        hazards[0].text(),
        "Change lanes or brake! Brake lights right ahead. Middle lane open."
    );
    assert_eq!(hazards[0].data.dodgeable, Some(true));
    assert!(hazards[0].data.traffic.is_some());
}

#[test]
fn test_city_events_announce_a_state_line_the_map_does_not_carry() {
    // The fallback that keeps a state line from passing in silence: where
    // the route has no surveyed boundary, this prefix is the only thing that
    // says the state changed.
    let w = world();
    let route = route_from_cities(w, &["Chicago", "Cleveland", "Pittsburgh"]);
    assert!(!route.legs[0].state_crossings().is_empty());
    // The same leg with its surveyed boundary taken away.
    let stripped = with_corridor(&route.legs[0], |d| d.state_crossings.clear());
    let route = replace_leg(&route, 0, stripped);

    let truck = TruckState::default();
    let weather = weather("great_lakes", 1);
    let mut trip = Trip::new(route.clone(), truck, weather, TripOptions::seeded(2));
    trip.position_mi = route.legs[0].miles;

    let events = trip.update(0.0);

    assert_eq!(
        messages_of(&events, TripEventKind::CityReached),
        vec!["Crossing into Ohio. Passing Cleveland, Ohio."]
    );
}

#[test]
fn test_city_events_include_state_without_repeating_crossing() {
    let w = world();
    let route = route_from_cities(w, &["New York", "Buffalo", "Cleveland"]);
    let truck = TruckState::default();
    let weather = weather("northeast", 1);
    let mut trip = Trip::new(route.clone(), truck, weather, TripOptions::seeded(2));
    trip.position_mi = route.legs[0].miles;

    let events = trip.update(0.0);

    assert_eq!(
        messages_of(&events, TripEventKind::CityReached),
        vec!["Passing Buffalo, New York."]
    );
}

#[test]
fn test_zone_warnings_come_one_at_a_time_and_never_for_one_underfoot() {
    // Owner playtest, 2026-08-17: five contradictory lines in sixty
    // milliseconds on a facility approach.
    let mut trip = make_trip(world(), "Chicago", "Indianapolis", 2);
    trip.zones = vec![
        Zone::new(0.05, 0.30, 15.0, "facility access road"), // underfoot: no warning
        Zone::new(0.60, 1.00, 25.0, "facility access road"),
        Zone::new(1.00, 1.40, 15.0, "facility access road"),
        Zone::new(1.40, 1.60, 15.0, "facility gate"),
    ];
    trip.announced_zone_warnings.clear();
    trip.pending_zone_warning = None;
    trip.position_mi = 0.0;

    // Several ticks at a standstill: the loop runs every frame and must not
    // spend the whole approach's worth of warnings on the first one.
    for _ in 0..5 {
        trip.events.clear();
        trip.check_zones();
    }
    let warned: Vec<_> = trip
        .events
        .iter()
        .filter(|e| e.kind == TripEventKind::GpsCue)
        .collect();
    assert!(
        warned.len() <= 1,
        "one outstanding warning, not one per frame"
    );

    // And nothing was said about the zone already under the wheels.
    let said = warned
        .iter()
        .map(|e| e.text())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(!said.contains("15") || ZONE_WARNING_MIN_MI < 0.05);
}

#[test]
fn test_distances_to_things_ahead_never_round_down_to_zero() {
    // Owner playtest, 2026-08-17: "What is this in 0 miles BS?"
    let mut trip = make_trip(world(), "Chicago", "Indianapolis", 2);

    for miles in [0.4, 0.3, 0.25, 0.1, 0.05, 0.0] {
        let spoken = trip.ahead_text(miles);
        assert!(!spoken.contains("0 mile"), "{miles} mi spoke as {spoken:?}");
    }

    assert_eq!(trip.ahead_text(0.25), "a quarter mile");
    assert_eq!(trip.ahead_text(1.0), "one mile");
    // Far enough out that whole miles are the natural wording again.
    assert_eq!(trip.ahead_text(5.0), "5 miles");

    // The lines the owner actually heard it in, end to end.
    trip.zones = vec![Zone::new(0.20, 0.60, 15.0, "facility access road")];
    trip.announced_zone_warnings.clear();
    trip.pending_zone_warning = None;
    trip.position_mi = 0.0;
    trip.events.clear();
    trip.check_zones();
    let zone_said = trip
        .events
        .iter()
        .map(|e| e.text())
        .collect::<Vec<_>>()
        .join(" ");
    assert!(!zone_said.contains("0 mile"), "{zone_said}");

    let pressure = TrafficPressure {
        start_mi: 0.2,
        end_mi: 0.6,
        kind: "route_merge".into(),
        direction: "right".into(),
        intensity: 0.5,
        target_speed_mph: 45.0,
        reason: "on-ramp".into(),
    };
    assert!(!trip
        .traffic_pressure_message(&pressure, 0.2)
        .normal
        .contains("0 mile"));
}

#[test]
fn test_the_gate_zone_never_swallows_the_streets_before_it() {
    // Owner report, 2026-08-17: "it says it's holding 25 when it's really
    // doing more like 14." Root cause found 2026-08-18, and it is arithmetic.
    let mut trip = make_trip(world(), "Chicago", "Indianapolis", 2);
    for leg_lengths in [vec![0.05, 0.21, 0.05, 0.09, 0.14], vec![0.2, 0.2]] {
        let mut starts = Vec::new();
        let mut acc = 0.0;
        for length in &leg_lengths {
            starts.push(acc);
            acc += length;
        }
        // A same-city street chain at 25 mph is a facility approach route.
        let city = "chicago_il_us";
        let legs: Vec<_> = leg_lengths
            .iter()
            .map(|m| ff_core::data::world_models::Leg::local(city, *m, "Street", "", 25.0))
            .collect();
        trip.route = Route::from_legs(vec![city.to_string(); legs.len() + 1], legs);
        trip.leg_starts = starts.clone();
        assert!(trip.is_facility_approach_route());
        let zones = trip.facility_speed_zones();

        let gate: Vec<_> = zones
            .iter()
            .filter(|z| z.limit_mph == FACILITY_GATE_LIMIT_MPH)
            .collect();
        assert!(!gate.is_empty(), "the gate zone vanished");
        // It starts no earlier than the final leg, so it can never reach back
        // over a street the driver is still meant to be doing 25 on.
        let last = *starts.last().unwrap();
        assert!(
            gate[0].start_mi >= last - 1e-9,
            "gate zone starts at {} but the last leg starts at {last}",
            gate[0].start_mi
        );
        assert!(
            gate[0].start_mi > 0.0,
            "the gate zone covered the whole approach"
        );

        // And nothing 25 is left fully shadowed by it.
        for zone in &zones {
            if zone.limit_mph == 25.0 {
                assert!(
                    zone.start_mi < gate[0].start_mi,
                    "a 25 street sits wholly inside the gate"
                );
            }
        }
    }
}

#[test]
fn test_debris_speaks_its_kind_and_the_split_keeps_the_old_rate() {
    // "Debris in the road" told a blind driver nothing about the dodge
    // (Brandon, 2026-08-20). The named split must sum to the 1.2 weight the
    // one generic entry carried.
    let names = [
        "the ladder",
        "the lumber",
        "the mattress",
        "the boxes",
        "the tarp",
        "the debris",
    ];
    let debris: Vec<_> = HAZARDS.iter().filter(|h| names.contains(&h.name)).collect();
    assert_eq!(debris.len(), 6);
    assert!(debris.iter().all(|h| h.in_lane));
    assert!((debris.iter().map(|h| h.weight).sum::<f64>() - 1.2).abs() < 1e-9);
    let found: Vec<&str> = debris.iter().map(|h| h.name).collect();
    assert!(found.contains(&"the ladder") && found.contains(&"the mattress"));
}

#[test]
fn test_the_animal_brake_call_names_the_animal() {
    let names = [
        "the dog",
        "the coyote",
        "the livestock",
        "the raccoon",
        "the animal",
    ];
    let animals: Vec<_> = HAZARDS.iter().filter(|h| names.contains(&h.name)).collect();
    assert_eq!(animals.len(), 5);
    assert!(animals.iter().all(|h| h.animal));
    assert!((animals.iter().map(|h| h.weight).sum::<f64>() - 0.7).abs() < 1e-9);
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- DrivingUpdateMixin._horn_scare_animals"]
fn test_the_horn_moves_a_movable_animal() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- DrivingUpdateMixin._horn_scare_animals"]
fn test_the_horn_gets_one_attempt_per_hazard() {}

/// `zlib.crc32(data)`: the IEEE CRC-32 Python's zlib module computes.
fn crc32(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

#[test]
fn test_the_welcome_sign_is_deterministic_and_authored() {
    // The pick is crc32-seeded so the same trip reads the same sign every
    // run -- str hash() is process-randomized and must never seed it.
    let seed = 7 ^ crc32(b"Texas") as i64;
    let sign = welcome_sign("Texas", &mut PyRandom::new_from_i64(seed));
    assert!(sign.starts_with("Welcome to Texas"), "{sign}");
    assert_eq!(
        sign,
        welcome_sign("Texas", &mut PyRandom::new_from_i64(seed))
    );
    assert_eq!(welcome_sign("Atlantis", &mut PyRandom::new_from_i64(1)), "");
}

// `test_the_horn_drains_the_air_tanks_to_the_protection_valve` lives in
// `sim::vehicle`'s own tests: the air-drain step is crate-private, which an
// integration test cannot reach.

#[test]
fn test_brake_lights_name_the_cause_when_the_road_knows_it() {
    // Brandon asked WHY the brake lights (2026-08-20). A braking cue inside
    // a construction or congestion zone names the cause; outside any
    // mile-mapped zone it says nothing about cause.
    let caused = brake_lights_cue(
        "half a mile",
        "30 miles per hour",
        "30",
        "Road work is the cause.",
    );
    assert!(caused.normal.contains("Road work is the cause."));
    assert!(
        !caused.terse.as_deref().unwrap_or("").contains("Road work"),
        "the cause must not bloat terse mode"
    );
    let plain = brake_lights_cue("half a mile", "30 miles per hour", "30", "");
    assert!(!plain.normal.to_lowercase().contains("cause"));

    let mut mgr = TrafficManager::bare(&Route::default(), &[]);
    mgr.braking_zones = vec![
        BrakingZone::new(10.0, 14.0, "construction", None),
        BrakingZone::new(20.0, 25.0, "heavy traffic", None),
    ];
    assert_eq!(mgr.braking_reason_at(12.0), "construction");
    assert_eq!(mgr.braking_reason_at(22.0), "heavy traffic");
    assert_eq!(mgr.braking_reason_at(50.0), "");
}

/// Rush hour is a local thing. Denver is two hours behind the trip's Eastern
/// clock, so 10 AM Eastern is 8 AM there, the middle of the morning rush. The
/// traffic manager already reads the local hour; the trip's own rush-hour
/// judgements read the Eastern one, and so put Denver's rush at 4:30 AM.
#[test]
fn test_rush_hour_follows_the_local_clock() {
    let opts = TripOptions {
        start_hour: 10.0,
        ..TripOptions::seeded(7)
    };
    let trip = make_trip_with(world(), "Denver", "Cheyenne", opts);
    assert!((trip.local_hour() - 8.0).abs() < 1e-9);
    assert_eq!(trip.congestion_phrase(), "rush hour congestion");
    let leg = trip.route.legs[0].clone();
    assert!(trip.rush_hour_traffic_bias(&leg) > 0.0);
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- driving status browse"]
fn test_the_status_browse_says_how_much_to_the_next_level() {}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- AbandonJobConfirmationState"]
fn test_abandoning_a_bobtail_costs_nothing() {}

#[test]
fn test_a_zone_exit_rise_waits_for_the_limit_window() {
    // A short slow zone ended right behind the arrival line, and two limit
    // numbers in one breath outran the voice (player report, 2026-10-17).
    // The exit's rise is held, then named once the window opens.
    use std::cell::Cell;
    use std::rc::Rc;
    let now = Rc::new(Cell::new(0.0));
    let mut trip = make_trip(world(), "Chicago", "Indianapolis", 2);
    let clock_now = Rc::clone(&now);
    trip.event_breather
        .set_clock(Box::new(move || clock_now.get()));
    trip.position_mi = 30.0;
    let corridor = trip.corridor_limit_at(31.0);
    assert!(corridor > 25.0, "the test needs a rise out of the zone");
    trip.zones = vec![Zone::new(29.9, 30.5, 25.0, "test")];
    trip.announced_zone_warnings.clear();
    trip.pending_zone_warning = None;
    trip.announced_speed_limit = Some(corridor);
    trip.check_zones();
    assert!(trip.entered_zone.is_some());
    trip.event_breather.spoke("limit");

    trip.events.clear();
    trip.position_mi = 31.0;
    trip.check_zones();
    trip.check_speed_limit();
    assert!(
        !trip.events.iter().any(|e| e.text().contains("Speed limit")),
        "{:?}",
        trip.events.iter().map(|e| e.text()).collect::<Vec<_>>()
    );

    now.set(60.0);
    trip.check_speed_limit();
    assert!(
        trip.events
            .iter()
            .any(|e| e.text().starts_with("Speed limit raised to")),
        "{:?}",
        trip.events.iter().map(|e| e.text()).collect::<Vec<_>>()
    );
}
