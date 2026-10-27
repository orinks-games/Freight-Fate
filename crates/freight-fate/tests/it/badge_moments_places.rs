//! Place and corridor badges land on the settlement of the run that earns
//! them: the city, state or region it ends in, the lane it drove, and the
//! ground it covered. Each is checked against the near miss, usually the
//! same corridor driven the other way.

use ff_core::data::regions::REGIONS;
use ff_core::data::world::get_world;
use ff_core::data::world_models::Route;
use serde_json::json;

use crate::badge_moments_support::*;

fn deliver_on(app: &mut freight_fate::app::testing::TestApp, route: Route) {
    let mut drive = run_on(app, route);
    settle(app, &mut drive);
}

fn state_of(city: &str) -> String {
    get_world().city(city).expect("on the map").state.clone()
}

fn region_of(city: &str) -> String {
    get_world().city(city).expect("on the map").region.clone()
}

/// Arriving into `city` earns `id`; leaving it for the neighbour does not.
fn arriving_earns(id: &str, city: &str, into: Route) {
    let origin = into.cities[0].clone();
    let mut app = career_in(city);
    deliver_on(&mut app, reversed(&into));
    assert!(!earned(&app, id), "{id} came leaving {city}");
    profile(&mut app).current_city = origin;
    deliver_on(&mut app, into);
    assert!(earned(&app, id), "{id} missed arriving in {city}");
}

#[test]
fn every_plain_city_badge_lands_on_arriving_there_not_leaving() {
    for (city, id) in [
        ("phoenix_az_us", "phoenix_arrival"),
        ("wichita_ks_us", "wichita_arrival"),
        ("bakersfield_ca_us", "bakersfield_arrival"),
        ("las_vegas_nv_us", "vegas_arrival"),
        ("nashville_tn_us", "nashville_delivery"),
        ("el_paso_tx_us", "el_paso_arrival"),
        ("laredo_tx_us", "laredo_arrival"),
        ("baton_rouge_la_us", "baton_rouge_arrival"),
        ("sacramento_ca_us", "sacramento_arrival"),
        ("muskogee_ok_us", "muskogee_arrival"),
        ("kansas_city_mo_us", "kansas_city_arrival"),
        ("memphis_tn_us", "memphis_arrival"),
        ("saginaw_mi_us", "saginaw_arrival"),
        ("fort_worth_tx_us", "fort_worth_arrival"),
        ("san_antonio_tx_us", "san_antonio_arrival"),
        ("new_orleans_la_us", "new_orleans_arrival"),
        ("houston_tx_us", "houston_arrival"),
        ("winslow_az_us", "winslow_arrival"),
        ("chattanooga_tn_us", "chattanooga_arrival"),
        ("jackson_tn_us", "jackson_arrival"),
        ("jackson_ms_us", "jackson_arrival"),
        ("abilene_tx_us", "abilene_arrival"),
    ] {
        arriving_earns(id, city, route_into(city));
    }
}

/// A city in `state` with a neighbour outside every state in `group` (the
/// states that share the badge), and the run in from there.
fn a_run_into_state(state: &str, group: &[&str]) -> (String, Route) {
    let world = get_world();
    world
        .cities
        .values()
        .filter(|city| city.state == state)
        .find_map(|city| {
            let into = world.neighbors(&city.key).iter().find_map(|leg| {
                let other = if leg.a == city.key { &leg.b } else { &leg.a };
                if group.contains(&state_of(other).as_str()) {
                    return None;
                }
                world.supported_route(other, &city.key, None).ok().flatten()
            })?;
            Some((city.key.clone(), into))
        })
        .unwrap_or_else(|| panic!("no run crosses into {state}"))
}

#[test]
fn every_state_badge_needs_the_run_to_end_in_that_state() {
    const DAKOTAS: &[&str] = &["North Dakota", "South Dakota"];
    const NORTHERN_NEW_ENGLAND: &[&str] = &["Maine", "Vermont", "New Hampshire"];
    for (state, id, group) in [
        ("Virginia", "virginia_line", &["Virginia"][..]),
        ("Kentucky", "kentucky_delivery", &["Kentucky"][..]),
        ("New Jersey", "jersey_delivery", &["New Jersey"][..]),
        ("Wyoming", "wyoming_delivery", &["Wyoming"][..]),
        ("North Dakota", "dakota_delivery", DAKOTAS),
        ("South Dakota", "dakota_delivery", DAKOTAS),
        ("Montana", "montana_delivery", &["Montana"][..]),
        ("Maine", "new_england_delivery", NORTHERN_NEW_ENGLAND),
        ("Vermont", "new_england_delivery", NORTHERN_NEW_ENGLAND),
        (
            "New Hampshire",
            "new_england_delivery",
            NORTHERN_NEW_ENGLAND,
        ),
    ] {
        let (city, into) = a_run_into_state(state, group);
        arriving_earns(id, &city, into);
    }
}

#[test]
fn long_haul_needs_nine_hundred_miles() {
    let near = route("chicago_il_us", "new_york_ny_us");
    let far = route("chicago_il_us", "denver_co_us");
    assert!(near.miles() < 900.0 && far.miles() >= 900.0);
    corridor_earns("long_haul", near, far);
}

#[test]
fn appalachia_delivery_needs_the_run_to_end_in_appalachia() {
    let city = "pittsburgh_pa_us";
    let region = region_of(city);
    assert_eq!(region, "appalachia");
    arriving_earns(
        "appalachia_delivery",
        city,
        route_into_from(city, |c| c.region != region),
    );
}

#[test]
fn pnw_delivery_needs_the_run_to_end_in_the_pacific_northwest() {
    let city = "spokane_wa_us";
    let region = region_of(city);
    assert_eq!(region, "pacific_northwest");
    arriving_earns(
        "pnw_delivery",
        city,
        route_into_from(city, |c| c.region != region),
    );
}

#[test]
fn norcal_giants_lands_anywhere_in_redwood_country() {
    // Willits and Fortuna only border other redwood towns, so every run
    // comes up from the Bay, outside the big trees.
    for city in [
        "santa_rosa_ca_us",
        "chico_ca_us",
        "ukiah_ca_us",
        "willits_ca_us",
        "fortuna_ca_us",
        "eureka_ca_us",
        "crescent_city_ca_us",
    ] {
        arriving_earns("norcal_giants", city, route("san_francisco_ca_us", city));
    }
}

#[test]
fn detroit_run_is_the_load_out_of_detroit_not_into_it() {
    arriving_earns(
        "detroit_run",
        "chicago_il_us",
        route("detroit_mi_us", "chicago_il_us"),
    );
}

#[test]
fn lubbock_arrival_is_lubbock_in_the_rearview() {
    // The title puts the city behind the truck, so it is the load OUT.
    arriving_earns(
        "lubbock_arrival",
        "amarillo_tx_us",
        route("lubbock_tx_us", "amarillo_tx_us"),
    );
}

/// Settle `route` arriving at `hour` local time.
fn deliver_at(app: &mut freight_fate::app::testing::TestApp, route: Route, hour: f64) {
    let mut drive = run_on(app, route);
    arrive_at_local_hour(&mut drive, hour);
    settle(app, &mut drive);
}

/// Arriving by `route` at `late` does not earn `id`; at `on` it does.
fn timed_arrival_earns(id: &str, route: Route, late: f64, on: f64) {
    let mut app = career_in(&route.cities[0]);
    deliver_at(&mut app, route.clone(), late);
    assert!(!earned(&app, id), "{id} came at {late}:00");
    deliver_at(&mut app, route, on);
    assert!(earned(&app, id), "{id} missed at {on}:00");
}

#[test]
fn amarillo_arrival_is_by_daybreak() {
    timed_arrival_earns(
        "amarillo_arrival",
        route("lubbock_tx_us", "amarillo_tx_us"),
        13.0,
        8.0,
    );
}

#[test]
fn birmingham_morning_is_a_morning_arrival() {
    timed_arrival_earns(
        "birmingham_morning",
        route("atlanta_ga_us", "birmingham_al_us"),
        14.0,
        8.0,
    );
}

#[test]
fn gulf_coast_by_two_means_before_two_in_the_afternoon() {
    timed_arrival_earns(
        "gulf_coast_by_two",
        route("jackson_ms_us", "gulfport_ms_us"),
        15.0,
        13.0,
    );
}

#[test]
fn georgia_arrival_is_midnight_freight() {
    let into = route("chattanooga_tn_us", "atlanta_ga_us");
    assert_eq!(state_of("atlanta_ga_us"), "Georgia");
    timed_arrival_earns("georgia_arrival", into, 12.0, 23.0);
}

#[test]
fn tulsa_arrival_is_right_on_schedule() {
    let into = route("oklahoma_city_ok_us", "tulsa_ok_us");
    let mut app = career_in("oklahoma_city_ok_us");
    let mut late = run_on(&mut app, into.clone());
    late.trip.game_minutes = (late.job.deadline_game_h + 1.0) * 60.0;
    settle(&mut app, &mut late);
    assert!(!earned(&app, "tulsa_arrival"), "a late Tulsa run earned it");
    deliver_on(&mut app, into);
    assert!(earned(&app, "tulsa_arrival"));
}

/// Settling `route` with `damage_pct` of fresh damage does not earn `id`;
/// the same run clean does.
fn clean_run_earns(id: &str, route: Route) {
    let mut app = career_in(&route.cities[0]);
    let mut dented = run_on(&mut app, route.clone());
    dented.trip.truck.damage_pct = dented.start_damage + 5.0;
    settle(&mut app, &mut dented);
    assert!(!earned(&app, id), "{id} came on a damaged run");
    deliver_on(&mut app, route);
    assert!(earned(&app, id), "{id} missed a clean run");
}

#[test]
fn waco_survivor_is_a_clean_run_into_waco() {
    clean_run_earns("waco_survivor", route("dallas_tx_us", "waco_tx_us"));
}

#[test]
fn mountain_clean_is_a_clean_run_over_mountain_road() {
    let over = route("denver_co_us", "salt_lake_city_ut_us");
    assert!(over.legs.iter().any(|leg| leg.terrain == "mountain"));
    clean_run_earns("mountain_clean", over);
}

/// `near` does not earn `id`; `far` does.
fn corridor_earns(id: &str, near: Route, far: Route) {
    let mut app = career_in(&near.cities[0]);
    deliver_on(&mut app, near);
    assert!(!earned(&app, id), "{id} came on the near miss");
    deliver_on(&mut app, far);
    assert!(earned(&app, id), "{id} missed");
}

#[test]
fn multi_state_needs_three_states_on_one_run() {
    let near = route("chicago_il_us", "cleveland_oh_us");
    let far = route("chicago_il_us", "pittsburgh_pa_us");
    let states = |r: &Route| {
        let mut s: Vec<String> = r.cities.iter().map(|c| state_of(c)).collect();
        s.dedup();
        s.len()
    };
    assert_eq!((states(&near), states(&far)), (2, 3));
    corridor_earns("multi_state", near, far);
}

#[test]
fn three_regions_counts_regions_across_the_career() {
    let mut app = career_in("chicago_il_us");
    deliver(&mut app, "chicago_il_us", "cleveland_oh_us");
    deliver(&mut app, "cleveland_oh_us", "pittsburgh_pa_us");
    assert_eq!(
        profile(&mut app).achievement_stats["regions_visited"],
        json!(["great_lakes", "appalachia"])
    );
    assert!(!earned(&app, "three_regions"));
    deliver(&mut app, "pittsburgh_pa_us", "new_york_ny_us");
    assert!(earned(&app, "three_regions"));
}

#[test]
fn all_regions_needs_every_region_on_the_map() {
    let mut app = career_in("chicago_il_us");
    // A save from an older map can carry region names that no longer
    // exist; they must not stand in for a real one.
    let mut regions: Vec<String> = vec!["midwest".into(), "southeast".into()];
    regions.extend(
        REGIONS
            .iter()
            .filter(|r| !["great_lakes", "appalachia"].contains(r))
            .map(|r| r.to_string()),
    );
    assert_eq!(regions.len(), REGIONS.len());
    profile(&mut app)
        .achievement_stats
        .insert("regions_visited".to_string(), json!(regions));
    deliver(&mut app, "chicago_il_us", "cleveland_oh_us");
    assert!(
        !earned(&app, "all_regions"),
        "one region short of the map is not all of it"
    );
    deliver(&mut app, "cleveland_oh_us", "pittsburgh_pa_us");
    assert!(earned(&app, "all_regions"));
}

#[test]
fn every_region_on_the_map_is_a_canonical_region() {
    let mut on_map: Vec<String> = get_world()
        .cities
        .values()
        .map(|c| c.region.clone())
        .collect();
    on_map.sort();
    on_map.dedup();
    let mut canonical: Vec<String> = REGIONS.iter().map(|r| r.to_string()).collect();
    canonical.sort();
    assert_eq!(
        on_map, canonical,
        "all_regions counts REGIONS, so the map must match it"
    );
}

#[test]
fn route66_run_needs_both_ends_on_the_mother_road() {
    corridor_earns(
        "route66_run",
        route("chicago_il_us", "cleveland_oh_us"),
        route("chicago_il_us", "st_louis_mo_us"),
    );
}

#[test]
fn route66_run_counts_the_small_towns_too() {
    corridor_earns(
        "route66_run",
        route("joplin_mo_us", "dallas_tx_us"),
        route("joplin_mo_us", "tulsa_ok_us"),
    );
    corridor_earns(
        "route66_run",
        route("gallup_nm_us", "phoenix_az_us"),
        route("gallup_nm_us", "kingman_az_us"),
    );
}

#[test]
fn texas_triangle_needs_both_ends_in_the_triangle() {
    corridor_earns(
        "texas_triangle",
        route("dallas_tx_us", "waco_tx_us"),
        route("dallas_tx_us", "fort_worth_tx_us"),
    );
}

#[test]
fn coast_to_coast_needs_thirty_five_degrees_of_longitude() {
    let lon = |c: &str| get_world().city(c).expect("on the map").lon;
    assert!(lon("new_york_ny_us") - lon("chicago_il_us") < 35.0);
    assert!(lon("new_york_ny_us") - lon("los_angeles_ca_us") >= 35.0);
    corridor_earns(
        "coast_to_coast",
        route("chicago_il_us", "new_york_ny_us"),
        route("los_angeles_ca_us", "new_york_ny_us"),
    );
}

#[test]
fn true_north_run_needs_four_degrees_north() {
    corridor_earns(
        "true_north_run",
        route("houston_tx_us", "dallas_tx_us"),
        route("houston_tx_us", "oklahoma_city_ok_us"),
    );
}

#[test]
fn southbound_run_needs_four_degrees_south() {
    corridor_earns(
        "southbound_run",
        route("dallas_tx_us", "houston_tx_us"),
        route("oklahoma_city_ok_us", "houston_tx_us"),
    );
}

#[test]
fn all_terrain_route_needs_flat_hills_and_mountain_on_one_run() {
    let near = route("chicago_il_us", "new_york_ny_us");
    let far = route("kingman_az_us", "holbrook_az_us");
    let has = |r: &Route, t: &str| r.legs.iter().any(|leg| leg.terrain == t);
    assert!(!has(&near, "mountain"));
    assert!(has(&far, "flat") && has(&far, "hills") && has(&far, "mountain"));
    corridor_earns("all_terrain_route", near, far);
}

#[test]
fn multi_leg_haul_needs_four_legs() {
    let near = route("houston_tx_us", "oklahoma_city_ok_us");
    let far = route("chicago_il_us", "denver_co_us");
    assert_eq!((near.legs.len(), far.legs.len()), (3, 4));
    corridor_earns("multi_leg_haul", near, far);
}

#[test]
fn five_hundred_mile_run_is_a_thousand_mile_dispatch() {
    // The id is historical; the copy and the check are a thousand miles.
    let near = route("chicago_il_us", "new_york_ny_us");
    let far = route("chicago_il_us", "denver_co_us");
    assert!(near.miles() < 1_000.0 && far.miles() >= 1_000.0);
    corridor_earns("five_hundred_mile_run", near, far);
}

#[test]
fn spotless_long_needs_three_hundred_clean_on_time_miles() {
    let near = route("chicago_il_us", "detroit_mi_us");
    let far = route("chicago_il_us", "cleveland_oh_us");
    assert!(near.miles() < 300.0 && far.miles() >= 300.0);
    corridor_earns("spotless_long", near, far);
}

#[test]
fn no_toll_long_is_three_hundred_miles_with_no_toll_bill() {
    let near = route("chicago_il_us", "detroit_mi_us");
    let far = route("chicago_il_us", "cleveland_oh_us");
    corridor_earns("no_toll_long", near, far);
}

#[test]
fn grueling_clean_is_twelve_hundred_miles_on_time_and_clean() {
    let far = route("chicago_il_us", "salt_lake_city_ut_us");
    assert!(far.miles() >= 1_200.0, "{}", far.miles());
    let mut app = career_in("chicago_il_us");
    let mut late = run_on(&mut app, far.clone());
    late.trip.game_minutes = (late.job.deadline_game_h + 1.0) * 60.0;
    settle(&mut app, &mut late);
    assert!(!earned(&app, "grueling_clean"), "a late run earned it");
    deliver_on(&mut app, far);
    assert!(earned(&app, "grueling_clean"));
}
