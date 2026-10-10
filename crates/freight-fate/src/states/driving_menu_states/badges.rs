//! Every achievement a completed delivery can earn
//! (`ArrivalState._award_arrival_achievements`), kept apart from the money
//! so the settlement arithmetic stays readable.

use chrono::{Datelike, Local};
use ff_core::achievements::{add_unique_stat, increment_stat, reset_stat};
use ff_core::data::regions::REGIONS;
use ff_core::models::carrier_fleet::{fleet_tier_for_level, FLEET_TIERS};
use ff_core::sim::season::{
    date_text, is_friday_the_thirteenth, is_truck_driver_appreciation_week, player_calendar_hours,
    season,
};
use ff_core::sim::vehicle::{combination_tare_kg, max_legal_cargo_tons, TruckSpecs};
use serde_json::Value;

use crate::app::GameContext;
use crate::net::server_now;
use crate::states::driving::DrivingState;
use crate::states::driving_core::{is_night, profile_mut_of, profile_of};
use crate::states::driving_menu_states::{simple_arrival_badge, ArrivalState};

/// Whether live weather is driving the calendar right now.
///
/// The same two-part answer the terminal's Time and weather readout uses: a
/// provider has to be attached AND the player has to have left the calendar
/// following it. Read defensively -- a trip built without a weather system
/// must not crash a settlement.
fn live_calendar(ctx: &GameContext, d: &DrivingState) -> bool {
    d.trip.weather.provider.is_some() && ctx.settings.live_weather_controls_calendar
}

/// How close to the legal ceiling a load has to come to count as "pushing
/// the legal weight limit". Dispatch clamps every load to what a stock
/// tractor and trailer can haul under 80,000 lb, so the badge is measured
/// from that ceiling rather than a fixed tonnage the board can never offer.
pub const MAX_GROSS_MARGIN_TONS: f64 = 1.0;

/// The cargo weight a load needs for "Grossed Out at the Scale House".
pub fn max_gross_badge_tons() -> f64 {
    max_legal_cargo_tons(combination_tare_kg(&TruckSpecs::default())) - MAX_GROSS_MARGIN_TONS
}

/// Towns strung along the old Mother Road that the map carries.
pub const ROUTE66: [&str; 20] = [
    "chicago_il_us",
    "bloomington_il_us",
    "springfield_il_us",
    "st_louis_mo_us",
    "rolla_mo_us",
    "springfield_mo_us",
    "joplin_mo_us",
    "tulsa_ok_us",
    "oklahoma_city_ok_us",
    "amarillo_tx_us",
    "tucumcari_nm_us",
    "albuquerque_nm_us",
    "gallup_nm_us",
    "holbrook_az_us",
    "winslow_az_us",
    "flagstaff_az_us",
    "kingman_az_us",
    "barstow_ca_us",
    "victorville_ca_us",
    "los_angeles_ca_us",
];

/// Redwood country: the far-north California towns in the big trees' shade.
pub const NORCAL_GIANTS: [&str; 7] = [
    "santa_rosa_ca_us",
    "chico_ca_us",
    "ukiah_ca_us",
    "willits_ca_us",
    "fortuna_ca_us",
    "eureka_ca_us",
    "crescent_city_ca_us",
];

/// How many of the map's regions the career has hauled through. Only the
/// current region names count, so a key a save kept from an older map can
/// neither stand in for one nor push the count past the real total.
fn canonical_regions_visited(ctx: &GameContext) -> usize {
    let visited = profile_of(ctx).achievement_stats.get("regions_visited");
    let visited = visited.and_then(Value::as_array);
    REGIONS
        .iter()
        .filter(|region| {
            visited.is_some_and(|values| values.iter().any(|v| v.as_str() == Some(**region)))
        })
        .count()
}

fn int_stat(ctx: &GameContext, key: &str) -> i64 {
    profile_of(ctx)
        .achievement_stats
        .get(key)
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn award_arrival_achievements(
    state: &mut ArrivalState,
    ctx: &mut GameContext,
    d: &DrivingState,
    on_time: bool,
    trip_damage: f64,
    toll_expense: f64,
    route_miles: f64,
    speeding_tickets: i64,
    gross_pay: f64,
) {
    let route = &d.route;
    let world = ctx.world;
    let mut states: Vec<String> = Vec::new();
    let mut regions: Vec<String> = Vec::new();
    for city in &route.cities {
        if let Ok(city) = world.city(city) {
            if !states.contains(&city.state) {
                states.push(city.state.clone());
            }
            if !regions.contains(&city.region) {
                regions.push(city.region.clone());
            }
        }
    }
    let mut region_count = 0usize;
    for region in &regions {
        region_count = add_unique_stat(profile_mut_of(ctx), "regions_visited", region);
    }

    let mut ids: Vec<String> = vec!["first_delivery".to_string()];
    let push = |ids: &mut Vec<String>, id: &str| ids.push(id.to_string());
    // Rookie chain: spread across the first few runs instead of all landing
    // on delivery one, alongside first_delivery.
    let deliveries = profile_of(ctx).career.deliveries;
    if on_time && deliveries >= 2 {
        push(&mut ids, "first_on_time");
    }
    if trip_damage <= 1.0 && deliveries >= 3 {
        push(&mut ids, "clean_delivery");
    }
    if speeding_tickets == 0 && deliveries >= 4 {
        push(&mut ids, "speed_limit_saint");
    }
    if toll_expense > 0.0 {
        push(&mut ids, "toll_paid");
    } else if route_miles >= 300.0 {
        push(&mut ids, "no_toll_long");
    }
    if states.len() >= 2 {
        push(&mut ids, "state_crossing");
    }
    if states.len() >= 3 {
        push(&mut ids, "multi_state");
    }
    if regions.len() >= 3 || region_count >= 3 {
        push(&mut ids, "three_regions");
    }
    if route_miles >= 900.0 {
        push(&mut ids, "long_haul");
    }
    if deliveries >= 5 {
        push(&mut ids, "five_deliveries");
    }
    if deliveries >= 10 {
        push(&mut ids, "ten_deliveries");
    }
    let level = profile_of(ctx).career.level();
    if level >= 3 {
        push(&mut ids, "level_three");
    }
    let money = profile_of(ctx).money();
    if money >= 25_000.0 {
        push(&mut ids, "twenty_five_grand");
    }
    let total_miles = profile_of(ctx).career.total_miles;
    if total_miles >= 1_000.0 {
        push(&mut ids, "thousand_miles");
    }

    // -- Landmarks: direction, famous corridors, and city-arrival badges --
    let origin = route.cities.first().cloned().unwrap_or_default();
    let dest = route.cities.last().cloned().unwrap_or_default();
    let origin_city = world.city(&origin).ok();
    let dest_city = world.city(&dest).ok();
    let origin_lon = origin_city.map(|c| c.lon).unwrap_or(0.0);
    let dest_lon = dest_city.map(|c| c.lon).unwrap_or(0.0);
    if dest_lon - origin_lon > 1.0 {
        push(&mut ids, "eastbound_delivery");
    }
    if origin_lon - dest_lon > 1.0 {
        push(&mut ids, "westbound_delivery");
    }
    if (dest_lon - origin_lon).abs() >= 35.0 {
        push(&mut ids, "coast_to_coast");
    }
    if ROUTE66.contains(&origin.as_str()) && ROUTE66.contains(&dest.as_str()) {
        push(&mut ids, "route66_run");
    }
    // Wall-clock badge conditions ("by Daybreak", "Midnight Freight") read
    // the destination's local clock, matching what the player just heard.
    let arrival_hour = d.trip.local_hour();
    if let Some(badge) = simple_arrival_badge(&dest) {
        push(&mut ids, badge);
    }
    // Badges whose title names a condition, so the condition is enforced:
    if dest == "amarillo_tx_us" && (5.0..12.0).contains(&arrival_hour) {
        // "by Daybreak"
        push(&mut ids, "amarillo_arrival");
    }
    if dest == "tulsa_ok_us" && on_time {
        // "Right on Schedule"
        push(&mut ids, "tulsa_arrival");
    }
    let dest_state = dest_city.map(|c| c.state.clone()).unwrap_or_default();
    let dest_region = dest_city.map(|c| c.region.clone()).unwrap_or_default();
    if dest_state == "Georgia" && is_night(arrival_hour) {
        push(&mut ids, "georgia_arrival"); // "Midnight Freight"
    }
    // Departures: the title puts the city in the rearview / "out of" it.
    if origin == "lubbock_tx_us" {
        // "in the Rearview"
        push(&mut ids, "lubbock_arrival");
    }
    if origin == "detroit_mi_us" {
        // "Last Load Out of"
        push(&mut ids, "detroit_run");
    }

    // -- Challenges: grind milestones, long hauls, spotless runs ----------
    if canonical_regions_visited(ctx) >= REGIONS.len() {
        push(&mut ids, "all_regions");
    }
    if deliveries >= 50 {
        push(&mut ids, "fifty_deliveries");
    }
    if deliveries >= 100 {
        push(&mut ids, "hundred_deliveries");
    }
    if total_miles >= 10_000.0 {
        push(&mut ids, "ten_thousand_miles");
    }
    if total_miles >= 50_000.0 {
        push(&mut ids, "fifty_thousand_miles");
    }
    if money >= 100_000.0 {
        push(&mut ids, "hundred_grand");
    }
    // Ladder milestones for the 30-level arc. "max_level" is the level-20
    // veteran badge (its copy has said "level twenty" since the ladder grew
    // past the old cap).
    for (milestone, badge) in [
        (5, "level_five"),
        (10, "level_ten"),
        (15, "level_fifteen"),
        (20, "max_level"),
        (25, "level_twenty_five"),
        (30, "level_thirty"),
    ] {
        if level >= milestone {
            push(&mut ids, badge);
        }
    }
    let reputation = profile_of(ctx).standing();
    if reputation >= 100.0 {
        push(&mut ids, "top_reputation");
    }
    if gross_pay >= 4_000.0 {
        push(&mut ids, "big_payday");
    }
    if route_miles >= 1_200.0 && on_time && trip_damage <= 1.0 {
        push(&mut ids, "grueling_clean");
    }
    if route.legs.iter().any(|leg| leg.terrain == "mountain") && trip_damage <= 1.0 {
        push(&mut ids, "mountain_clean");
    }
    if route.legs.len() >= 4 {
        push(&mut ids, "multi_leg_haul");
    }
    // Five consecutive on-time, undamaged, ticket-free deliveries.
    let perfect = on_time && trip_damage <= 1.0 && speeding_tickets == 0;
    let streak = if perfect {
        int_stat(ctx, "perfect_streak") + 1
    } else {
        0
    };
    profile_mut_of(ctx)
        .achievement_stats
        .insert("perfect_streak".to_string(), Value::from(streak));
    if streak >= 5 {
        push(&mut ids, "perfect_streak");
    }

    // -- Landmarks, second verse: state, region, and timed city badges ----
    let job = &d.job;
    let hours = d.trip.game_minutes / 60.0;
    const STATE_BADGES: [(&str, &str); 10] = [
        ("Virginia", "virginia_line"),
        ("Kentucky", "kentucky_delivery"),
        ("New Jersey", "jersey_delivery"),
        ("Wyoming", "wyoming_delivery"),
        ("North Dakota", "dakota_delivery"),
        ("South Dakota", "dakota_delivery"),
        ("Montana", "montana_delivery"),
        ("Maine", "new_england_delivery"),
        ("Vermont", "new_england_delivery"),
        ("New Hampshire", "new_england_delivery"),
    ];
    if let Some((_, badge)) = STATE_BADGES.iter().find(|(name, _)| *name == dest_state) {
        push(&mut ids, badge);
    }
    // Map coverage milestones across the whole city network.
    let city_count = add_unique_stat(profile_mut_of(ctx), "cities_delivered", &dest);
    if city_count >= 25 {
        push(&mut ids, "twenty_five_cities");
    }
    if city_count >= 75 {
        push(&mut ids, "seventy_five_cities");
    }
    if city_count >= 150 {
        push(&mut ids, "hundred_fifty_cities");
    }
    let state_count = add_unique_stat(profile_mut_of(ctx), "states_delivered", &dest_state);
    if state_count >= 15 {
        push(&mut ids, "fifteen_states");
    }
    if state_count >= 30 {
        push(&mut ids, "thirty_states");
    }
    if dest_region == "appalachia" {
        push(&mut ids, "appalachia_delivery");
    }
    if dest_region == "pacific_northwest" {
        push(&mut ids, "pnw_delivery");
    }
    if dest == "birmingham_al_us" && (6.0..11.0).contains(&arrival_hour) {
        // morning run
        push(&mut ids, "birmingham_morning");
    }
    if dest == "waco_tx_us" && trip_damage <= 1.0 {
        // "Just Fine"
        push(&mut ids, "waco_survivor");
    }
    if dest == "gulfport_ms_us" && arrival_hour < 14.0 {
        // "by Two"
        push(&mut ids, "gulf_coast_by_two");
    }
    if NORCAL_GIANTS.contains(&dest.as_str()) {
        push(&mut ids, "norcal_giants");
    }
    const TRIANGLE: [&str; 5] = [
        "dallas_tx_us",
        "fort_worth_tx_us",
        "houston_tx_us",
        "san_antonio_tx_us",
        "austin_tx_us",
    ];
    if TRIANGLE.contains(&origin.as_str()) && TRIANGLE.contains(&dest.as_str()) {
        push(&mut ids, "texas_triangle");
    }

    // -- Routes, second verse: compass runs and marathon dispatches -------
    let origin_lat = origin_city.map(|c| c.lat).unwrap_or(0.0);
    let dest_lat = dest_city.map(|c| c.lat).unwrap_or(0.0);
    if dest_lat - origin_lat >= 4.0 {
        push(&mut ids, "true_north_run");
    }
    if origin_lat - dest_lat >= 4.0 {
        push(&mut ids, "southbound_run");
    }
    let terrains: Vec<&str> = route.legs.iter().map(|leg| leg.terrain.as_str()).collect();
    if ["flat", "hills", "mountain"]
        .iter()
        .all(|t| terrains.contains(t))
    {
        push(&mut ids, "all_terrain_route");
    }
    if hours >= 24.0 {
        push(&mut ids, "long_day_run");
    }
    let last_route = profile_of(ctx)
        .achievement_stats
        .get("last_route")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect::<Vec<String>>()
        })
        .unwrap_or_default();
    if last_route == vec![dest.clone(), origin.clone()] {
        push(&mut ids, "return_trip");
    }
    profile_mut_of(ctx).achievement_stats.insert(
        "last_route".to_string(),
        Value::from(vec![Value::from(origin.clone()), Value::from(dest.clone())]),
    );

    // -- Cargo: what's in the box matters ---------------------------------
    // Every credential the load needed gets its badge, so a fuel tanker
    // (tank AND hazmat) counts for both.
    for credential in job.cargo.credentials {
        match *credential {
            "refrigerated" => push(&mut ids, "reefer_load"),
            "flatbed_securement" => push(&mut ids, "securement_load"),
            "heavy_haul" => push(&mut ids, "heavy_haul_load"),
            "high_value" => push(&mut ids, "high_value_load"),
            "doubles_triples" => push(&mut ids, "doubles_load"),
            "hazmat" => push(&mut ids, "hazmat_load"),
            "tank" => push(&mut ids, "tank_load"),
            "twic" => push(&mut ids, "port_load"),
            "lcv" => push(&mut ids, "lcv_load"),
            _ => {}
        }
    }
    if job.cargo.key == "grain" || job.cargo.key == "farm_inputs" {
        push(&mut ids, "farm_load");
    }
    if job.weight_tons >= max_gross_badge_tons() {
        push(&mut ids, "max_gross_load");
    }

    // -- Career, second verse: the numbers keep climbing ------------------
    if deliveries >= 25 {
        push(&mut ids, "twenty_five_deliveries");
    }
    if deliveries >= 200 {
        push(&mut ids, "two_hundred_deliveries");
    }
    if money >= 250_000.0 {
        push(&mut ids, "quarter_million_bank");
    }
    if profile_of(ctx).career.total_earnings >= 500_000.0 {
        push(&mut ids, "half_million_earned");
    }
    if total_miles >= 100_000.0 {
        push(&mut ids, "hundred_k_miles");
    }
    if reputation >= 90.0 {
        push(&mut ids, "rep_ninety");
    }
    if profile_of(ctx).game_hours >= 30.0 * 24.0 {
        push(&mut ids, "month_on_road");
    }
    // A career's home city is where its very first delivery loaded up.
    if deliveries == 1 && !profile_of(ctx).achievement_stats.contains_key("home_city") {
        profile_mut_of(ctx)
            .achievement_stats
            .insert("home_city".to_string(), Value::from(origin.clone()));
    }
    let home_city = profile_of(ctx)
        .achievement_stats
        .get("home_city")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    if deliveries >= 10 && dest == home_city {
        push(&mut ids, "home_return");
    }

    // -- Seasons: the calendar rides shotgun ------------------------------
    //
    // The calendar the player HEARS, not raw career time. These badges answer
    // to a date and a season the player was told, so reading the un-offset
    // career clock fired April's Fool in August with the real-time calendar
    // on (reported 2026-08-11), and called a delivery a winter one while the
    // live weather said otherwise.
    let live = live_calendar(ctx, d);
    let calendar_hours = {
        let p = profile_of(ctx);
        player_calendar_hours(p.game_hours, Some(p.calendar_game_hours()), live)
    };
    let career_season = season(calendar_hours);
    if career_season == "winter" {
        push(&mut ids, "winter_delivery");
    }
    if add_unique_stat(profile_mut_of(ctx), "seasons_delivered", career_season) >= 4 {
        push(&mut ids, "four_seasons");
    }
    if dest_region == "desert_southwest" && career_season == "summer" {
        push(&mut ids, "desert_summer");
    }
    if date_text(calendar_hours) == "April 1" {
        push(&mut ids, "april_first");
    }

    // -- Deliveries, second verse: clocks, gauges, and close calls --------
    if on_time && hours >= 0.9 * job.deadline_game_h {
        push(&mut ids, "deadline_squeaker");
    }
    if !on_time {
        push(&mut ids, "first_late");
    }
    if arrival_hour < 4.0 {
        push(&mut ids, "midnight_delivery");
    }
    // Careers start at 6:00, so "before the roosters" means before that.
    if (3.0..6.0).contains(&d.trip.start_hour) {
        push(&mut ids, "dawn_run");
    }
    // Better than eight miles to the gallon over the whole run. The model is
    // calibrated for 6.5-7 at cruise, so this is a light foot, not a freebie;
    // a run with no fuel burned at all (a resumed trip) cannot earn it.
    if d.trip.fuel_used_gal > 0.0 && d.trip.position_mi / d.trip.fuel_used_gal > 8.0 {
        push(&mut ids, "thrifty_run");
    }
    if d.trip.truck.fuel_fraction() < 0.08 {
        push(&mut ids, "fuel_fumes");
    }
    if route_miles >= 300.0 && on_time && trip_damage <= 1.0 && speeding_tickets == 0 {
        push(&mut ids, "spotless_long");
    }
    if d.speeding_tickets >= 1 {
        push(&mut ids, "first_ticket");
    }
    if d.speeding_tickets >= 2 {
        push(&mut ids, "second_ticket");
    }

    // -- Dates worth noticing, and records worth keeping ------------------
    let arrival_date = date_text(calendar_hours);
    if arrival_date == "December 25" {
        push(&mut ids, "christmas_delivery");
    }
    if arrival_date == "January 1" && arrival_hour < 3.0 {
        push(&mut ids, "new_year_run");
    }
    // Days the player is living rather than the career's: read off
    // orinks.net's clock, because the computer's can be set to any date. No
    // word from the server this session, no badge.
    if let Some(today) = server_now().map(|now| now.with_timezone(&Local).date_naive()) {
        if is_friday_the_thirteenth(today) && trip_damage <= 1.0 {
            push(&mut ids, "friday_thirteenth");
        }
        // Ten-four day: the date reads as the acknowledgment every driver on
        // the channel has been saying all year.
        if (today.month(), today.day()) == (10, 4) {
            push(&mut ids, "ten_four_day");
        }
        if is_truck_driver_appreciation_week(today) {
            push(&mut ids, "appreciation_week");
        }
    }
    if job.distance_mi >= 1_000.0 {
        push(&mut ids, "five_hundred_mile_run");
    }
    if job.weight_tons >= 16.0 {
        push(&mut ids, "sixteen_tons");
    }
    if arrival_hour < 4.0 && increment_stat(profile_mut_of(ctx), "night_deliveries") >= 10 {
        push(&mut ids, "night_shift_regular");
    }
    // A clean licence is a running record, so a single ticket ends it: the
    // counter only advances on deliveries that came with none.
    if d.speeding_tickets == 0 {
        if increment_stat(profile_mut_of(ctx), "ticket_free_deliveries") >= 50 {
            push(&mut ids, "never_fought_the_law");
        }
    } else {
        reset_stat(profile_mut_of(ctx), "ticket_free_deliveries");
    }
    // Everything that could have gone wrong, going wrong slowly.
    if on_time
        && arrival_hour < 4.0
        && d.trip.truck.fuel_fraction() < 0.15
        && d.trip.weather.effects().grip < 0.9
    {
        push(&mut ids, "one_for_the_road");
    }
    // The tractor and the band it came out of: slip-seating means a junior
    // driver meets a lot of iron.
    let truck_key = profile_of(ctx).active_truck_key().to_string();
    if add_unique_stat(profile_mut_of(ctx), "tractors_driven", &truck_key) >= 5 {
        push(&mut ids, "five_tractors");
    }
    if !profile_of(ctx).owns_equipment() {
        let tier = fleet_tier_for_level(profile_of(ctx).career.level()).key;
        if add_unique_stat(profile_mut_of(ctx), "fleet_tiers_driven", tier) >= FLEET_TIERS.len() {
            push(&mut ids, "every_fleet_tier");
        }
    }

    for achievement_id in ids {
        if let Some(result) = ctx.award_achievement_with(&achievement_id, false, false) {
            state.record_badge(
                result.award.message.normal.clone(),
                result.award.achievement.name.to_string(),
                result.publicly_eligible,
            );
        }
    }
    ctx.save_profile();
}
