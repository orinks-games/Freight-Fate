//! Badges for what a single run was: the freight in the box, the clock and
//! calendar it arrived on, the gauges and tickets it came in with, the
//! tolls it paid. Each lands on the settlement (or, for a toll, the plaza)
//! of the run that meets it, and not on the near miss before it.

use chrono::{DateTime, TimeZone, Utc};
use ff_core::data::world_models::Route;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::sim::trip_models::TripEventKind;
use ff_core::sim::vehicle::{combination_tare_kg, max_legal_cargo_tons, TruckSpecs};
use ff_core::sim::weather::WeatherKind;
use freight_fate::app::testing::TestApp;
use freight_fate::net::testing::set_server_time;
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::DRIVE_PHASE_DELIVERY;
use serde_json::json;

use crate::badge_moments_support::*;

const CHICAGO: &str = "chicago_il_us";
const CLEVELAND: &str = "cleveland_oh_us";

fn chicago_run() -> Route {
    route(CHICAGO, CLEVELAND)
}

/// `near` changes an ordinary Chicago run so it misses `id`, and `far` so it
/// earns it; they settle in that order on one career.
fn run_earns(
    id: &str,
    near: impl FnOnce(&mut TestApp, &mut DrivingState),
    far: impl FnOnce(&mut TestApp, &mut DrivingState),
) {
    let mut app = career_in(CHICAGO);
    let mut drive = run_on(&mut app, chicago_run());
    near(&mut app, &mut drive);
    settle(&mut app, &mut drive);
    assert!(!earned(&app, id), "{id} came on the near miss");
    let mut drive = run_on(&mut app, chicago_run());
    far(&mut app, &mut drive);
    settle(&mut app, &mut drive);
    assert!(earned(&app, id), "{id} missed");
}

// -- the freight -------------------------------------------------------------------

#[test]
fn every_credential_load_earns_its_own_badge() {
    for (credential, id) in [
        ("refrigerated", "reefer_load"),
        ("flatbed_securement", "securement_load"),
        ("heavy_haul", "heavy_haul_load"),
        ("high_value", "high_value_load"),
        ("doubles_triples", "doubles_load"),
        ("hazmat", "hazmat_load"),
        ("tank", "tank_load"),
        ("twic", "port_load"),
        ("lcv", "lcv_load"),
    ] {
        let mut app = career_in(CHICAGO);
        deliver(&mut app, CHICAGO, CLEVELAND);
        assert!(!earned(&app, id), "{id} came on general freight");
        let mut drive = run_job(
            &mut app,
            chicago_run(),
            cargo_needing(credential),
            12.0,
            1000.0,
        );
        settle(&mut app, &mut drive);
        assert!(earned(&app, id), "{id} missed a {credential} load");
    }
}

#[test]
fn farm_load_is_grain_or_farm_inputs() {
    for cargo in ["grain", "farm_inputs"] {
        let mut app = career_in(CHICAGO);
        deliver(&mut app, CHICAGO, CLEVELAND);
        assert!(!earned(&app, "farm_load"));
        let mut drive = run_job(&mut app, chicago_run(), &CARGO_CATALOG[cargo], 20.0, 1000.0);
        settle(&mut app, &mut drive);
        assert!(earned(&app, "farm_load"), "{cargo} missed it");
    }
}

#[test]
fn max_gross_load_needs_a_load_at_the_legal_ceiling() {
    // Dispatch never offers more than a stock rig can carry under 80,000 lb,
    // so the badge has to be reachable below that ceiling.
    let ceiling = max_legal_cargo_tons(combination_tare_kg(&TruckSpecs::default()));
    let mut app = career_in(CHICAGO);
    let general = &CARGO_CATALOG["general"];
    let mut drive = run_job(&mut app, chicago_run(), general, ceiling - 1.5, 1000.0);
    settle(&mut app, &mut drive);
    assert!(!earned(&app, "max_gross_load"));
    let mut drive = run_job(&mut app, chicago_run(), general, ceiling - 0.5, 1000.0);
    settle(&mut app, &mut drive);
    assert!(earned(&app, "max_gross_load"));
}

#[test]
fn sixteen_tons_needs_sixteen_tons_on_the_trailer() {
    let mut app = career_in(CHICAGO);
    let general = &CARGO_CATALOG["general"];
    let mut drive = run_job(&mut app, chicago_run(), general, 15.0, 1000.0);
    settle(&mut app, &mut drive);
    assert!(!earned(&app, "sixteen_tons"));
    let mut drive = run_job(&mut app, chicago_run(), general, 16.0, 1000.0);
    settle(&mut app, &mut drive);
    assert!(earned(&app, "sixteen_tons"));
}

#[test]
fn big_payday_is_four_thousand_gross_on_one_load() {
    let mut app = career_in(CHICAGO);
    let general = &CARGO_CATALOG["general"];
    let mut drive = run_job(&mut app, chicago_run(), general, 12.0, 1000.0);
    settle(&mut app, &mut drive);
    assert!(!earned(&app, "big_payday"));
    let mut drive = run_job(&mut app, chicago_run(), general, 12.0, 10_000.0);
    settle(&mut app, &mut drive);
    assert!(earned(&app, "big_payday"));
}

#[test]
fn bobtail_done_lands_on_the_empty_run_not_a_loaded_one() {
    let mut app = career_in(CHICAGO);
    deliver(&mut app, CHICAGO, CLEVELAND);
    assert!(!earned(&app, "bobtail_done"));
    let empty_route = route(CLEVELAND, CHICAGO);
    let miles = empty_route.miles();
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        0.0,
        CLEVELAND,
        "company yard",
        CHICAGO,
        miles,
        0.0,
        miles / 55.0 + 12.0,
    );
    job.bobtail = true;
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        empty_route,
        Some(4),
        DRIVE_PHASE_DELIVERY,
        Some(12.0),
    );
    drive.trip.position_mi = drive.trip.total_miles();
    settle(&mut app, &mut drive);
    assert!(earned(&app, "bobtail_done"));
}

// -- the clock ---------------------------------------------------------------------

#[test]
fn deadline_squeaker_is_on_time_with_under_a_tenth_to_spare() {
    run_earns(
        "deadline_squeaker",
        |_, d| d.trip.game_minutes = d.job.deadline_game_h * 0.5 * 60.0,
        |_, d| d.trip.game_minutes = d.job.deadline_game_h * 0.95 * 60.0,
    );
}

#[test]
fn first_late_lands_on_the_first_late_delivery() {
    run_earns(
        "first_late",
        |_, _| {},
        |_, d| d.trip.game_minutes = (d.job.deadline_game_h + 1.0) * 60.0,
    );
}

#[test]
fn long_day_run_needs_twenty_four_hours_on_the_clock() {
    run_earns(
        "long_day_run",
        |_, d| d.trip.game_minutes = 23.0 * 60.0,
        |_, d| d.trip.game_minutes = 25.0 * 60.0,
    );
}

#[test]
fn midnight_delivery_is_an_arrival_before_four() {
    run_earns(
        "midnight_delivery",
        |_, d| arrive_at_local_hour(d, 5.0),
        |_, d| arrive_at_local_hour(d, 2.0),
    );
}

#[test]
fn one_for_the_road_is_on_time_before_four_low_on_fuel_on_a_slick_road() {
    fn small_hours_on_fumes(d: &mut DrivingState) {
        arrive_at_local_hour(d, 2.0);
        d.trip.truck.fuel_gal = d.trip.truck.specs.fuel_tank_gal * 0.10;
    }
    run_earns(
        "one_for_the_road",
        |_, d| {
            small_hours_on_fumes(d);
            d.trip.weather.current = WeatherKind::Clear;
            assert!(d.trip.weather.effects().grip >= 0.9);
        },
        |_, d| {
            small_hours_on_fumes(d);
            d.trip.weather.current = WeatherKind::Snow;
            assert!(d.trip.weather.effects().grip < 0.9);
        },
    );
}

#[test]
fn dawn_run_is_a_departure_before_six() {
    run_earns(
        "dawn_run",
        |_, d| d.trip.start_hour = 6.5,
        |_, d| d.trip.start_hour = 4.0,
    );
}

// -- the gauges and the tickets ----------------------------------------------------

#[test]
fn fuel_fumes_is_arriving_under_eight_percent() {
    run_earns(
        "fuel_fumes",
        |_, d| d.trip.truck.fuel_gal = d.trip.truck.specs.fuel_tank_gal * 0.10,
        |_, d| d.trip.truck.fuel_gal = d.trip.truck.specs.fuel_tank_gal * 0.05,
    );
}

#[test]
fn first_ticket_lands_with_the_first_ticketed_run() {
    run_earns("first_ticket", |_, _| {}, |_, d| d.speeding_tickets = 1);
}

#[test]
fn second_ticket_needs_two_tickets_on_one_run() {
    run_earns(
        "second_ticket",
        |_, d| d.speeding_tickets = 1,
        |_, d| d.speeding_tickets = 2,
    );
}

// -- the calendar ------------------------------------------------------------------

/// Arriving on `near` does not earn `id`; arriving on `far` does.
fn date_earns(id: &str, near: &str, far: &str) {
    run_earns(
        id,
        |app, d| arrive_on(app, d, near, 12.0),
        |app, d| arrive_on(app, d, far, 12.0),
    );
}

#[test]
fn winter_delivery_is_a_delivery_in_winter() {
    date_earns("winter_delivery", "March 25", "January 15");
}

#[test]
fn april_first_is_april_first_on_the_career_calendar() {
    date_earns("april_first", "March 31", "April 1");
}

/// orinks.net's clock at noon Eastern on a real date.
fn server_on(y: i32, m: u32, d: u32) -> Option<DateTime<Utc>> {
    Some(Utc.with_ymd_and_hms(y, m, d, 16, 0, 0).unwrap())
}

#[test]
fn ten_four_day_is_the_real_fourth_of_october() {
    run_earns(
        "ten_four_day",
        |app, d| {
            // The career calendar alone never earns it, with or without
            // word from the server.
            arrive_on(app, d, "October 4", 12.0);
            set_server_time(None);
        },
        |_, _| set_server_time(server_on(2026, 10, 4)),
    );
    let mut app = career_in(CHICAGO);
    set_server_time(server_on(2026, 10, 3));
    let mut drive = run_on(&mut app, chicago_run());
    arrive_on(&mut app, &drive, "October 4", 12.0);
    settle(&mut app, &mut drive);
    assert!(
        !earned(&app, "ten_four_day"),
        "earned off the career calendar"
    );
}

#[test]
fn appreciation_week_is_the_real_week_in_september() {
    run_earns(
        "appreciation_week",
        |_, _| set_server_time(server_on(2026, 9, 12)),
        |_, _| set_server_time(server_on(2026, 9, 13)),
    );
}

#[test]
fn christmas_delivery_is_christmas_day() {
    date_earns("christmas_delivery", "December 24", "December 25");
}

#[test]
fn new_year_run_is_the_small_hours_of_january_first() {
    // The date alone is not enough: the badge is the run that sees the year in.
    run_earns(
        "new_year_run",
        |app, d| arrive_on(app, d, "January 1", 12.0),
        |app, d| {
            arrive_at_local_hour(d, 1.0);
            arrive_on(app, d, "January 1", 1.0);
        },
    );
}

#[test]
fn friday_thirteenth_is_a_clean_run_on_the_real_day() {
    run_earns(
        "friday_thirteenth",
        |_, d| {
            // The right day, but the truck came in dented.
            set_server_time(server_on(2026, 11, 13));
            d.trip.truck.damage_pct = d.start_damage + 5.0;
        },
        |_, _| set_server_time(server_on(2026, 11, 13)),
    );
    // April 13 is a Friday on the career calendar, and that no longer counts.
    let mut app = career_in(CHICAGO);
    set_server_time(server_on(2026, 11, 12));
    let mut drive = run_on(&mut app, chicago_run());
    arrive_on(&mut app, &drive, "April 13", 12.0);
    settle(&mut app, &mut drive);
    assert!(!earned(&app, "friday_thirteenth"));
}

#[test]
fn four_seasons_lands_on_the_fourth_season_delivered() {
    let mut app = career_in(CHICAGO);
    profile(&mut app)
        .achievement_stats
        .insert("seasons_delivered".to_string(), json!(["summer", "autumn"]));
    let mut spring = run_on(&mut app, chicago_run());
    arrive_on(&mut app, &spring, "April 20", 12.0);
    settle(&mut app, &mut spring);
    assert!(!earned(&app, "four_seasons"), "three seasons is not four");
    let mut winter = run_on(&mut app, chicago_run());
    arrive_on(&mut app, &winter, "January 15", 12.0);
    settle(&mut app, &mut winter);
    assert!(earned(&app, "four_seasons"));
}

#[test]
fn desert_summer_is_a_summer_delivery_into_the_desert() {
    let into = route_into("albuquerque_nm_us");
    let mut app = career_in(&into.cities[0]);
    let mut spring = run_on(&mut app, into.clone());
    arrive_on(&mut app, &spring, "April 20", 12.0);
    settle(&mut app, &mut spring);
    assert!(!earned(&app, "desert_summer"), "spring in the desert");
    let mut summer = run_on(&mut app, chicago_run());
    arrive_on(&mut app, &summer, "July 15", 12.0);
    settle(&mut app, &mut summer);
    assert!(!earned(&app, "desert_summer"), "summer in Cleveland");
    let mut summer = run_on(&mut app, into);
    arrive_on(&mut app, &summer, "July 15", 12.0);
    settle(&mut app, &mut summer);
    assert!(earned(&app, "desert_summer"));
}

// -- tolls -------------------------------------------------------------------------

#[test]
fn toll_paid_lands_at_the_plaza() {
    let mut app = career_in(CHICAGO);
    let mut drive = run_on(&mut app, chicago_run());
    drive.trip.check_tolls();
    let toll = drive
        .trip
        .events
        .iter()
        .find(|event| event.kind == TripEventKind::TollCharged)
        .cloned()
        .expect("the Chicago to Cleveland run crosses a charged toll");
    assert!(!earned(&app, "toll_paid"));
    drive.handle_trip_event(&mut app.ctx, &toll);
    assert!(earned(&app, "toll_paid"));
}

#[test]
fn toll_paid_also_lands_at_settlement_and_rules_out_the_no_toll_badge() {
    // A run resumed past its plazas still has the bill on its settlement.
    let mut app = career_in(CHICAGO);
    let mut drive = run_on(&mut app, chicago_run());
    drive.trip.check_tolls();
    assert!(drive.trip.toll_expense() > 0.0);
    assert!(!earned(&app, "toll_paid"));
    settle(&mut app, &mut drive);
    assert!(earned(&app, "toll_paid"));
    assert!(!earned(&app, "no_toll_long"));
}
