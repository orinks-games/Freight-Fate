//! Trip hazard, GPS cue, toll, and city-event tests (port of
//! `tests/test_trip_cues.py`).

use std::collections::HashMap;

use crate::sim_support::*;
use chrono::NaiveDate;
use ff_core::data::state_welcome::welcome_sign;
use ff_core::data::world_models::{CrossingLoad, Route};
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
fn whittier_tunnel_gate_runs_in_the_trip_and_toll_is_one_way() {
    let world = world();
    let leg = world
        .legs
        .iter()
        .find(|leg| leg.a == "bear_valley_ak_us" && leg.b == "whittier_ak_us")
        .expect("Whittier tunnel leg")
        .as_ref()
        .clone();
    let route = Route::from_legs(
        vec!["bear_valley_ak_us".into(), "whittier_ak_us".into()],
        vec![leg.clone()],
    );
    let mut truck = TruckState::default();
    truck.start_engine();
    truck.velocity_mps = 25.0 * 1609.344 / 3600.0;
    let may = ff_core::sim::season::real_clock_game_hours(Some(
        NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(5, 35, 0)
            .unwrap(),
    ));
    let mut trip = Trip::new(
        route,
        truck,
        default_weather(),
        TripOptions {
            world: Some(world),
            career_hours: Some(may),
            start_hour: 5.0 + 35.0 / 60.0,
            ..TripOptions::seeded(91)
        },
    );
    let events = trip.update(1.0);
    assert!(events.iter().any(|event| event.text().starts_with("Anton Anderson Memorial Tunnel. One lane shared with the railroad. Gates run on a published schedule.")));
    assert!(
        trip.game_minutes > 0.0,
        "the physical tunnel drive advances game time"
    );
    trip.position_mi = trip.total_miles();
    trip.check_tolls();
    assert_eq!(trip.toll_charges.len(), 1);
    let return_route = Route::from_legs(
        vec!["whittier_ak_us".into(), "bear_valley_ak_us".into()],
        vec![leg],
    );
    let mut return_trip = Trip::new(
        return_route,
        TruckState::default(),
        default_weather(),
        TripOptions {
            world: Some(world),
            ..TripOptions::seeded(92)
        },
    );
    return_trip.position_mi = return_trip.total_miles();
    return_trip.check_tolls();
    assert!(return_trip.toll_charges.is_empty(), "return is toll-free");
    let spoken_tolls = |trip: &Trip| {
        trip.build_navigation_cues()
            .iter()
            .filter(|cue| cue.kind == "toll")
            .count()
    };
    assert_eq!(
        spoken_tolls(&return_trip),
        0,
        "no toll cue where none is charged"
    );
    assert_eq!(
        spoken_tolls(&trip),
        1,
        "the toll cue is spoken toward Whittier"
    );
}

fn hazmat_load() -> CrossingLoad {
    CrossingLoad::standard_rig(true).with_cargo("hazardous", false, 30_000)
}

fn refused_by_none(route: &Route, load: &CrossingLoad) -> bool {
    route
        .legs
        .iter()
        .all(|leg| leg.crossings.iter().all(|c| c.refusal(load).is_none()))
}

#[test]
fn crossings_wall_off_refused_legs_and_leave_plain_freight_alone() {
    let world = world();
    let plain = CrossingLoad::standard_rig(true).with_cargo("general", false, 30_000);
    assert!(world.crossing_blocks(&plain).is_empty());
    let fuel = CrossingLoad::standard_rig(true).with_cargo("fuel_bulk", true, 40_000);
    let route = world
        .legal_route("hartford_ct_us", "boston_ma_us", &fuel)
        .unwrap()
        .expect("bulk fuel has a legal road to Boston");
    assert!(refused_by_none(&route, &fuel));
    assert!(world
        .legal_route_options("hartford_ct_us", "boston_ma_us", 3, &fuel)
        .unwrap()
        .iter()
        .all(|r| refused_by_none(r, &fuel)));
    assert_eq!(
        world
            .legal_route("hartford_ct_us", "boston_ma_us", &plain)
            .unwrap()
            .map(|r| r.cities),
        world
            .supported_route("hartford_ct_us", "boston_ma_us", None)
            .unwrap()
            .map(|r| r.cities),
    );
    assert!(
        world
            .legal_route("bear_valley_ak_us", "whittier_ak_us", &hazmat_load())
            .unwrap()
            .is_none(),
        "the tunnel is the only road to Whittier, so no placarded load is planned there"
    );
}

/// Each tunnel with a published legal detour is planned onto that detour at
/// close to normal miles, so the freight stays on the board.
#[test]
fn restricted_tunnels_detour_by_their_real_legal_roads() {
    let world = world();
    let fuel = CrossingLoad::standard_rig(true).with_cargo("fuel_bulk", true, 40_000);
    let packaged = hazmat_load();
    let cases = [
        ("worcester_ma_us", "boston_ma_us", 8.0),
        ("providence_ri_us", "boston_ma_us", 8.0),
        ("portsmouth_nh_us", "boston_ma_us", 8.0),
        ("boston_ma_us", "portsmouth_nh_us", 8.0),
        ("manchester_nh_us", "boston_ma_us", 8.0),
        ("boston_ma_us", "manchester_nh_us", 8.0),
        ("boston_ma_us", "worcester_ma_us", 8.0),
        ("wilmington_de_us", "baltimore_md_us", 15.0),
        ("philadelphia_pa_us", "baltimore_md_us", 15.0),
        ("denver_co_us", "silverthorne_co_us", 15.0),
    ];
    for (from, to, extra) in cases {
        let open = world
            .supported_route(from, to, None)
            .unwrap()
            .expect("open road");
        for load in [&fuel, &packaged] {
            let legal = world
                .legal_route(from, to, load)
                .unwrap()
                .unwrap_or_else(|| panic!("{from} to {to} has a legal road"));
            assert!(refused_by_none(&legal, load), "{from} to {to}");
            assert!(
                legal.miles() <= open.miles() + extra,
                "{from} to {to}: {} legal miles against {} open",
                legal.miles(),
                open.miles()
            );
            assert!(
                legal.miles() <= ff_core::data::world::max_alternate_miles(open.miles()),
                "{from} to {to} stays on the job board"
            );
            assert!(
                legal
                    .legs
                    .iter()
                    .map(|leg| leg.id)
                    .ne(open.legs.iter().map(|leg| leg.id)),
                "{from} to {to} leaves the tunnel leg"
            );
            if from == "boston_ma_us" || to == "boston_ma_us" {
                assert!(
                    legal
                        .legs
                        .iter()
                        .any(|leg| leg.detour_for == "Boston tunnels"),
                    "{from} to {to} drives the Boston tunnels detour"
                );
            }
        }
    }
}

#[test]
fn spoken_tunnel_rules_match_the_planned_detour() {
    let world = world();
    let eisenhower = world
        .legs
        .iter()
        .flat_map(|leg| leg.crossings.iter())
        .find(|c| c.name == "Eisenhower-Johnson Memorial Tunnel")
        .expect("EJMT is on the map");
    assert!(eisenhower.hazmat_text.contains("US 6 over Loveland Pass"));
    let over_the_pass = world
        .legal_route("denver_co_us", "silverthorne_co_us", &hazmat_load())
        .unwrap()
        .expect("Loveland Pass is open to placarded loads");
    assert_eq!(over_the_pass.legs.len(), 1);
    assert_eq!(over_the_pass.legs[0].highway, "US-6");
    assert!(over_the_pass.legs[0].crossings.is_empty());
    // No standing WSDOT rule bans flammable cargo from the I-90 tunnels, and
    // no leg reaches the Holland or Lincoln tunnels: New York legs cross the
    // George Washington Bridge, whose upper level takes placarded loads.
    for leg in &world.legs {
        for crossing in &leg.crossings {
            assert!(!crossing.name.contains("Lake Washington"));
            assert!(!crossing.name.contains("Holland"));
            assert!(!crossing.name.contains("Lincoln"));
        }
    }
    let fuel = CrossingLoad::standard_rig(true).with_cargo("fuel_bulk", true, 40_000);
    for from in ["allentown_pa_us", "newark_nj_us", "spokane_wa_us"] {
        let to = if from == "spokane_wa_us" {
            "seattle_wa_us"
        } else {
            "new_york_ny_us"
        };
        assert_eq!(
            world
                .legal_route(from, to, &fuel)
                .unwrap()
                .map(|r| r.cities),
            world
                .supported_route(from, to, None)
                .unwrap()
                .map(|r| r.cities),
            "{from} to {to} keeps its normal road for bulk fuel"
        );
    }
}

#[test]
fn bay_bridge_turns_bulk_fuel_south_and_bridge_tolls_are_read() {
    let world = world();
    let fuel = CrossingLoad::standard_rig(true).with_cargo("fuel_bulk", true, 40_000);
    let open = world
        .supported_route("sacramento_ca_us", "san_francisco_ca_us", None)
        .unwrap()
        .expect("open road");
    assert!(open.legs.iter().any(|leg| leg
        .crossings
        .iter()
        .any(|c| c.kind == "bridge" && c.refusal(&fuel).is_some())));
    let legal = world
        .legal_route("sacramento_ca_us", "san_francisco_ca_us", &fuel)
        .unwrap()
        .expect("bulk fuel reaches San Francisco without the Bay Bridge");
    assert!(legal
        .legs
        .iter()
        .all(|leg| leg.crossings.iter().all(|c| c.refusal(&fuel).is_none())));
    let milk = CrossingLoad::standard_rig(true).with_cargo("milk", true, 40_000);
    assert!(open
        .legs
        .iter()
        .all(|leg| leg.crossings.iter().all(|c| c.refusal(&milk).is_none())));
    let toll = |a: &str, b: &str| {
        world
            .legs
            .iter()
            .find(|leg| leg.a == a && leg.b == b)
            .expect("leg")
            .toll_events()
            .first()
            .map(|toll| (toll.amount, toll.estimated))
    };
    assert_eq!(
        toll("sault_ste_marie_mi_us", "traverse_city_mi_us"),
        Some((25.0, false))
    );
    assert_eq!(
        toll("norfolk_va_us", "cape_charles_va_us"),
        Some((48.0, false))
    );
}

fn wind_trip(from: &str, to: &str, cargo_kg: f64) -> Trip {
    let world = world();
    let leg = world
        .legs
        .iter()
        .find(|leg| leg.a == from && leg.b == to)
        .expect("wind leg")
        .as_ref()
        .clone();
    assert!(leg.crossings.iter().any(|c| c.wind.is_some()));
    let route = Route::from_legs(vec![from.into(), to.into()], vec![leg]);
    let mut truck = TruckState::default();
    truck.start_engine();
    truck.trailer_attached = true;
    truck.cargo_key = "general".into();
    truck.cargo_kg = cargo_kg;
    truck.velocity_mps = 10.0;
    Trip::new(
        route,
        truck,
        default_weather(),
        TripOptions {
            world: Some(world),
            ..TripOptions::seeded(93)
        },
    )
}

#[derive(Clone)]
struct StationWind(std::rc::Rc<std::cell::Cell<f64>>);

impl ff_core::sim::weather::WeatherProvider for StationWind {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}
    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        Some(WeatherKind::Wind)
    }
    fn get_wind_kmh(&mut self, _city: &str) -> Option<f64> {
        Some(self.0.get())
    }
}

fn blow(peak_mph: f64) -> Option<ff_core::sim::weather::WindBlow> {
    Some(ff_core::sim::weather::WindBlow {
        kind: WeatherKind::Wind,
        peak_mph,
        minutes: 0.0,
    })
}

/// Live wind of `bridge` km/h at a crossing's own station and `around` km/h
/// everywhere else; records where each station was asked for.
#[derive(Clone)]
struct BridgeStation {
    bridge: f64,
    around: f64,
    asked: std::rc::Rc<std::cell::RefCell<Vec<(String, f64, f64)>>>,
}

impl ff_core::sim::weather::WeatherProvider for BridgeStation {
    fn request(&mut self, city: &str, lat: f64, lon: f64) {
        self.asked.borrow_mut().push((city.to_string(), lat, lon));
    }
    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        Some(WeatherKind::Wind)
    }
    fn get_wind_kmh(&mut self, city: &str) -> Option<f64> {
        Some(if city.starts_with("crossing:") {
            self.bridge
        } else {
            self.around
        })
    }
}

#[test]
fn mackinac_high_wind_is_an_escort_wait_on_duty() {
    let mut trip = wind_trip("sault_ste_marie_mi_us", "traverse_city_mi_us", 13_600.0);
    trip.weather.forced = Some(WeatherKind::Wind);
    trip.weather.current = WeatherKind::Wind;
    trip.weather.blow = blow(45.0);
    let start_minutes = trip.game_minutes;
    let events = trip.update(1.0);
    let wait = events
        .iter()
        .find(|e| e.data.context.as_deref() == Some("tunnel_wait"))
        .expect("escort wait");
    assert_eq!(wait.data.amount, Some(20.0));
    assert!(
        trip.game_minutes - start_minutes >= 20.0,
        "the escort wait moves the trip clock"
    );
    assert!(wait.text().contains("wait for an escort"));
    assert!(wait.text().contains("20 minutes on duty"));
    for _ in 0..30 {
        trip.update(1.0);
    }
    assert!(trip.position_mi > 0.0);
}

#[test]
fn class_rules_pass_allowed_classes_and_route_banned_ones_around() {
    use ff_core::data::world_models::HazardClass;
    let world = world();
    let packaged = |class| {
        CrossingLoad::standard_rig(true)
            .with_cargo("hazardous", false, 30_000)
            .with_hazard_class(Some(class))
    };
    let open = world
        .supported_route("norfolk_va_us", "richmond_va_us", None)
        .unwrap()
        .expect("open road");
    let refused = |load: &CrossingLoad| {
        open.legs
            .iter()
            .flat_map(|leg| leg.crossings.iter())
            .find_map(|c| c.refusal_spoken(load))
    };
    assert!(open.legs.iter().any(|leg| leg
        .crossings
        .iter()
        .any(|c| c.name == "Hampton Roads Bridge-Tunnel")));
    assert_eq!(refused(&packaged(HazardClass::NonflammableGas)), None);
    assert_eq!(
        refused(&packaged(HazardClass::FlammableLiquid)),
        None,
        "non-bulk paint"
    );
    let gas = refused(&packaged(HazardClass::PoisonGas)).expect("poison gas refused");
    assert!(gas.starts_with("This load is poison gas. "), "{gas}");
    let legal = world
        .legal_route(
            "norfolk_va_us",
            "richmond_va_us",
            &packaged(HazardClass::PoisonGas),
        )
        .unwrap()
        .expect("poison gas reaches Richmond another way");
    assert!(legal.legs.iter().all(|leg| leg
        .crossings
        .iter()
        .all(|c| c.refusal(&packaged(HazardClass::PoisonGas)).is_none())));
    let cbbt = world
        .legs
        .iter()
        .flat_map(|leg| leg.crossings.iter())
        .find(|c| c.name == "Chesapeake Bay Bridge-Tunnel")
        .expect("CBBT");
    assert_eq!(
        cbbt.refusal_spoken(&packaged(HazardClass::FlammableLiquid))
            .as_deref(),
        Some(
            "This load is flammable liquid. A placarded truckload of it is over the \
             Chesapeake Bay Bridge-Tunnel's quantity limits."
        ),
        "quantity-limited classes are refused as over the limits, not as banned"
    );
    let banned = cbbt
        .refusal_spoken(&packaged(HazardClass::DangerousWhenWet))
        .expect("4.3 is banned outright");
    assert!(
        banned.starts_with("This load is dangerous when wet. Explosives, poison gas"),
        "{banned}"
    );
    for class in [
        HazardClass::Explosive14,
        HazardClass::Explosive15,
        HazardClass::Explosive16,
        HazardClass::NonflammableGas,
    ] {
        assert!(cbbt.refusal(&packaged(class)).is_none(), "{class:?}");
    }
    // The Golden Gate never refuses outright: explosives wait for the hour.
    let blast = packaged(HazardClass::Explosive11);
    let gg = world
        .supported_route("san_francisco_ca_us", "santa_rosa_ca_us", None)
        .unwrap()
        .expect("open road");
    assert!(gg.legs.iter().any(|leg| leg
        .crossings
        .iter()
        .any(|c| c.name == "Golden Gate Bridge" && c.refusal(&blast).is_none())));
}

fn golden_gate_trip(
    class: Option<ff_core::data::world_models::HazardClass>,
    career_hours: Option<f64>,
    start_hour: f64,
) -> Trip {
    let world = world();
    let leg = world
        .legs
        .iter()
        .find(|leg| leg.a == "san_francisco_ca_us" && leg.b == "santa_rosa_ca_us")
        .expect("Golden Gate leg")
        .as_ref()
        .clone();
    let route = Route::from_legs(
        vec!["san_francisco_ca_us".into(), "santa_rosa_ca_us".into()],
        vec![leg],
    );
    let mut truck = TruckState::default();
    truck.start_engine();
    truck.trailer_attached = true;
    truck.cargo_key = "hazardous".into();
    truck.cargo_kg = 9_000.0;
    truck.placarded_hazmat = true;
    truck.hazard_class = class;
    truck.velocity_mps = 10.0;
    Trip::new(
        route,
        truck,
        default_weather(),
        TripOptions {
            world: Some(world),
            start_hour,
            career_hours,
            ..TripOptions::seeded(93)
        },
    )
}

#[test]
fn golden_gate_holds_explosives_through_weekday_rush_hour_then_escorts_them() {
    use ff_core::data::world_models::HazardClass;
    let waits = |class, days: Option<f64>, hour| {
        let mut trip = golden_gate_trip(class, days, hour);
        let start = trip.game_minutes;
        let waits = trip
            .update(1.0)
            .into_iter()
            .filter(|e| e.data.context.as_deref() == Some("tunnel_wait"))
            .map(|e| (e.data.amount, e.text().to_string()))
            .collect::<Vec<_>>();
        (waits, trip.game_minutes - start)
    };
    // The career calendar starts on a Wednesday, so five days on is Monday
    // and four is Sunday. The trip clock is Eastern; San Francisco is three
    // hours behind it, so Eastern 10 AM is 7 AM at the bridge.
    let monday = Some(24.0 * 5.0);
    let sunday = Some(24.0 * 4.0);
    let (rush, minutes) = waits(Some(HazardClass::Explosive11), monday, 10.0);
    assert_eq!(
        rush.len(),
        1,
        "one line for the ban and the escort: {rush:?}"
    );
    assert_eq!(rush[0].0, Some(170.0));
    assert!(rush[0].1.contains("weekday rush hours"));
    assert!(rush[0].1.contains("inspection and a bridge escort"));
    assert!(
        rush[0].1.contains(
            "You wait on duty until 9:30 AM, then 20 minutes for the escort, and may cross at 9:50 AM."
        ),
        "{}",
        rush[0].1
    );
    assert!(
        minutes >= 170.0,
        "both waits move the trip clock: {minutes}"
    );
    let (before_dawn, minutes) = waits(Some(HazardClass::Explosive11), monday, 7.0);
    assert_eq!(before_dawn.len(), 1, "4 AM at the bridge is before the ban");
    assert_eq!(before_dawn[0].0, Some(20.0));
    assert!(minutes >= 20.0, "the escort moves the trip clock");
    let (midday, _) = waits(Some(HazardClass::Explosive13), monday, 15.0);
    assert_eq!(midday.len(), 1, "escort only");
    assert_eq!(midday[0].0, Some(20.0));
    let (fireworks, minutes) = waits(Some(HazardClass::Explosive14), monday, 10.0);
    assert!(
        fireworks.is_empty(),
        "1.4 is not an explosive under Vehicle Code 31600(c): {fireworks:?}"
    );
    assert!(minutes < 20.0, "fireworks are not held: {minutes}");
    let (weekend, _) = waits(Some(HazardClass::Explosive11), sunday, 10.0);
    assert_eq!(weekend.len(), 1, "no rush-hour ban at the weekend");
    assert_eq!(weekend[0].0, Some(20.0));
    assert!(waits(Some(HazardClass::FlammableLiquid), monday, 10.0)
        .0
        .is_empty());
}

#[test]
fn mackinac_escorts_placarded_and_heavy_loads_once_in_any_weather() {
    let waits = |key: &str, cargo_kg: f64, sky: WeatherKind| {
        let mut trip = wind_trip("sault_ste_marie_mi_us", "traverse_city_mi_us", cargo_kg);
        trip.truck.cargo_key = key.into();
        if key == "hazardous" {
            trip.truck.hazard_class =
                Some(ff_core::data::world_models::HazardClass::FlammableLiquid);
        }
        trip.weather.forced = Some(sky);
        trip.weather.current = sky;
        if sky == WeatherKind::Wind {
            trip.weather.blow = blow(45.0);
        }
        trip.update(1.0)
            .into_iter()
            .filter(|e| e.data.context.as_deref() == Some("tunnel_wait"))
            .map(|e| (e.data.amount, e.text().to_string()))
            .collect::<Vec<_>>()
    };
    let placarded = waits("hazardous", 9_000.0, WeatherKind::Clear);
    assert_eq!(placarded.len(), 1);
    assert_eq!(placarded[0].0, Some(20.0));
    assert!(placarded[0].1.contains("escorts placarded and heavy loads"));
    assert!(placarded[0].1.contains("20 minutes on duty"));
    let mut clocked = wind_trip("sault_ste_marie_mi_us", "traverse_city_mi_us", 9_000.0);
    clocked.truck.cargo_key = "hazardous".into();
    clocked.truck.hazard_class = Some(ff_core::data::world_models::HazardClass::FlammableLiquid);
    clocked.weather.forced = Some(WeatherKind::Clear);
    clocked.weather.current = WeatherKind::Clear;
    let start_minutes = clocked.game_minutes;
    clocked.update(1.0);
    assert!(
        clocked.game_minutes - start_minutes >= 20.0,
        "the escort wait moves the trip clock"
    );
    assert!(waits("general", 13_600.0, WeatherKind::Clear).is_empty());
    assert_eq!(
        waits("general", 25_000.0, WeatherKind::Clear).len(),
        1,
        "over 40 tons gross"
    );
    let windy = waits("hazardous", 9_000.0, WeatherKind::Wind);
    assert_eq!(windy.len(), 1, "one escort covers wind and placards");
    assert!(windy[0].1.contains("wait for an escort"));
}

#[test]
fn a_simulated_high_wind_eases_so_an_empty_trailer_crosses_the_cbbt_within_the_cap() {
    let mut trip = wind_trip("norfolk_va_us", "cape_charles_va_us", 0.0);
    trip.weather.forced = Some(WeatherKind::Wind);
    trip.weather.current = WeatherKind::Wind;
    trip.weather.blow = blow(55.0);
    let start_minutes = trip.game_minutes;
    let events = trip.update(1.0);
    let wait = events
        .iter()
        .find(|e| e.data.context.as_deref() == Some("tunnel_wait"))
        .expect("wind wait");
    let waited = wait.data.amount.expect("minutes");
    assert!(
        (30.0..240.0).contains(&waited),
        "eased inside the cap: {waited}"
    );
    assert!(
        wait.text().contains("for the wind to drop"),
        "{}",
        wait.text()
    );
    assert!(trip.game_minutes - start_minutes >= waited);
    assert!(events
        .iter()
        .all(|e| e.data.context.as_deref() != Some("crossing_wind_hold")));
    assert!(trip.wind_held_gates.is_empty());
    // Whatever the peak, a simulated high wind never outlasts the cap.
    for _ in 0..12 {
        let mut trip = wind_trip("norfolk_va_us", "cape_charles_va_us", 0.0);
        trip.weather.forced = Some(WeatherKind::Wind);
        trip.weather.current = WeatherKind::Wind;
        trip.update(1.0);
        assert!(trip.wind_held_gates.is_empty(), "{:?}", trip.weather.blow);
    }
}

#[test]
fn an_empty_trailer_waits_out_cbbt_wind_on_duty_then_holds() {
    let mut trip = wind_trip("norfolk_va_us", "cape_charles_va_us", 0.0);
    let station = std::rc::Rc::new(std::cell::Cell::new(72.5));
    trip.weather.provider = Some(Box::new(StationWind(station)));
    trip.weather.city = Some("norfolk_va_us".into());
    let start_minutes = trip.game_minutes;
    let events = trip.update(1.0);
    let wait = events
        .iter()
        .find(|e| e.data.context.as_deref() == Some("tunnel_wait"))
        .expect("wind wait");
    assert_eq!(wait.data.amount, Some(240.0));
    assert!(wait.text().contains("Chesapeake Bay Bridge-Tunnel"));
    assert!(wait.text().contains("4 hours on duty"));
    assert!(trip.game_minutes - start_minutes >= 240.0);
    // The way around the bay is far beyond the alternate-route allowance.
    assert!(events
        .iter()
        .any(|e| e.text().contains("You hold here until the wind drops")));
    assert!(events
        .iter()
        .all(|e| e.data.context.as_deref() != Some("crossing_reroute")));
    let loaded = wind_trip("norfolk_va_us", "cape_charles_va_us", 9_000.0);
    let mut loaded = loaded;
    loaded.weather.forced = Some(WeatherKind::Wind);
    loaded.weather.current = WeatherKind::Wind;
    loaded.weather.blow = blow(45.0);
    let events = loaded.update(1.0);
    assert!(events
        .iter()
        .all(|e| e.data.context.as_deref() != Some("tunnel_wait")));
    for _ in 0..30 {
        loaded.update(1.0);
    }
    assert!(loaded.position_mi > 0.0, "a loaded rig crosses at 45 mph");
}

#[test]
fn mackinac_closed_to_trucks_holds_until_the_wind_drops() {
    let mut trip = wind_trip("sault_ste_marie_mi_us", "traverse_city_mi_us", 13_600.0);
    let station = std::rc::Rc::new(std::cell::Cell::new(100.0));
    trip.weather.provider = Some(Box::new(StationWind(station.clone())));
    trip.weather.city = Some("sault_ste_marie_mi_us".into());
    let events = trip.update(1.0);
    assert!(events
        .iter()
        .any(|e| e.text().contains("closed to trucks in high winds")));
    assert!(events
        .iter()
        .any(|e| e.text().contains("You hold here until the wind drops")));
    assert!(!trip.wind_held_gates.is_empty());
    for _ in 0..10 {
        let quiet = trip.update(1.0);
        assert!(quiet
            .iter()
            .all(|e| e.data.context.as_deref() != Some("tunnel_wait")));
    }
    assert!(trip.position_mi < 0.1, "held at the approach");
    station.set(20.0);
    let events = trip.update(1.0);
    assert!(events
        .iter()
        .any(|e| e.text().contains("The wind has dropped")));
    trip.truck.velocity_mps = 10.0;
    for _ in 0..30 {
        trip.update(1.0);
    }
    assert!(trip.position_mi > 0.0);
}

/// Escort-level wind on the first reading, calm after it.
struct DroppingWind(std::rc::Rc<std::cell::Cell<u32>>);

impl ff_core::sim::weather::WeatherProvider for DroppingWind {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}
    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        Some(WeatherKind::Wind)
    }
    fn get_wind_kmh(&mut self, _city: &str) -> Option<f64> {
        let reads = self.0.get();
        self.0.set(reads + 1);
        Some(if reads == 0 { 64.0 } else { 0.0 })
    }
}

#[test]
fn a_placarded_load_is_escorted_once_when_the_wind_drops_during_the_escort() {
    let mut trip = wind_trip("sault_ste_marie_mi_us", "traverse_city_mi_us", 9_000.0);
    trip.truck.cargo_key = "hazardous".into();
    trip.truck.hazard_class = Some(ff_core::data::world_models::HazardClass::FlammableLiquid);
    let reads = std::rc::Rc::new(std::cell::Cell::new(0));
    trip.weather.provider = Some(Box::new(DroppingWind(reads.clone())));
    trip.weather.city = Some("sault_ste_marie_mi_us".into());
    let start_minutes = trip.game_minutes;
    let waits = trip
        .update(1.0)
        .into_iter()
        .filter(|e| e.data.context.as_deref() == Some("tunnel_wait"))
        .map(|e| (e.data.amount, e.text().to_string()))
        .collect::<Vec<_>>();
    assert!(reads.get() >= 1, "the wind was read at the bridge");
    assert_eq!(waits.len(), 1, "one escort: {waits:?}");
    assert_eq!(waits[0].0, Some(20.0));
    assert!(waits[0].1.contains("wait for an escort"), "{}", waits[0].1);
    let minutes = trip.game_minutes - start_minutes;
    assert!(
        (20.0..40.0).contains(&minutes),
        "escorted once on the clock: {minutes}"
    );
}

#[test]
fn live_wind_is_read_at_the_station_nearest_the_bridge() {
    let asked = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let station = |bridge: f64, around: f64| BridgeStation {
        bridge,
        around,
        asked: asked.clone(),
    };
    let mut trip = wind_trip("sault_ste_marie_mi_us", "traverse_city_mi_us", 13_600.0);
    trip.weather.provider = Some(Box::new(station(100.0, 10.0)));
    trip.weather.city = Some("sault_ste_marie_mi_us".into());
    let (key, lat, _) = trip.crossing_station(0).expect("the bridge has a station");
    assert_eq!(key, "crossing:Mackinac Bridge");
    // St. Ignace, at the north end of the bridge, not Sault Ste. Marie.
    assert!((45.7..46.0).contains(&lat), "{lat}");
    let events = trip.update(1.0);
    assert!(events
        .iter()
        .any(|e| e.text().contains("closed to trucks in high winds")));
    assert!(asked.borrow().iter().any(|(k, _, _)| *k == key));

    let mut calm = wind_trip("sault_ste_marie_mi_us", "traverse_city_mi_us", 13_600.0);
    calm.weather.provider = Some(Box::new(station(10.0, 100.0)));
    calm.weather.city = Some("sault_ste_marie_mi_us".into());
    let events = calm.update(1.0);
    assert!(
        events
            .iter()
            .all(|e| e.data.context.as_deref() != Some("tunnel_wait")),
        "a calm bridge is open whatever the wind where the truck starts"
    );
    assert!(calm.wind_held_gates.is_empty());
}

#[test]
fn a_refused_truck_is_rerouted_at_the_crossing_not_held() {
    let world = world();
    let leg = world
        .legs
        .iter()
        .find(|leg| leg.a == "denver_co_us" && leg.b == "salt_lake_city_ut_us")
        .expect("Denver to Salt Lake City leg")
        .as_ref()
        .clone();
    assert!(leg
        .crossings
        .iter()
        .any(|c| c.refusal(&hazmat_load()).is_some()));
    let route = Route::from_legs(
        vec!["denver_co_us".into(), "salt_lake_city_ut_us".into()],
        vec![leg],
    );
    let mut truck = TruckState::default();
    truck.start_engine();
    truck.cargo_key = "hazardous".into();
    truck.hazard_class = Some(ff_core::data::world_models::HazardClass::FlammableLiquid);
    truck.cargo_kg = 13_600.0;
    truck.placarded_hazmat = true;
    truck.velocity_mps = 10.0;
    let mut trip = Trip::new(
        route,
        truck,
        default_weather(),
        TripOptions {
            world: Some(world),
            ..TripOptions::seeded(93)
        },
    );
    let events = trip.update(1.0);
    assert!(events
        .iter()
        .any(|e| e.data.context.as_deref() == Some("crossing_reroute")
            && e.text().contains("Loveland Pass")
            && e.text().contains("Dispatch reroutes you from")));
    assert_eq!(
        trip.route.cities.first().map(String::as_str),
        Some("denver_co_us")
    );
    assert_eq!(
        trip.route.cities.last().map(String::as_str),
        Some("salt_lake_city_ut_us")
    );
    assert!(trip.route.legs.iter().all(|leg| leg
        .crossings
        .iter()
        .all(|c| c.refusal(&hazmat_load()).is_none())));
    assert_eq!(trip.leg_starts.len(), trip.route.legs.len());
    for _ in 0..30 {
        trip.update(1.0);
    }
    assert!(
        trip.position_mi > 0.0,
        "the truck drives on instead of holding"
    );
    assert!(trip.refused_tunnel_gates.is_empty());
}

#[test]
fn whittier_tunnel_schedule_keeps_alaska_time() {
    let world = world();
    let leg = world
        .legs
        .iter()
        .find(|leg| leg.a == "bear_valley_ak_us" && leg.b == "whittier_ak_us")
        .expect("Whittier tunnel leg")
        .as_ref()
        .clone();
    let route = Route::from_legs(
        vec!["bear_valley_ak_us".into(), "whittier_ak_us".into()],
        vec![leg],
    );
    let mut truck = TruckState::default();
    truck.start_engine();
    truck.velocity_mps = 25.0 * 1609.344 / 3600.0;
    // 8:00 AM Eastern on May 1 is 4:00 AM in Alaska: the tunnel is shut for
    // the night until the 5:30 AM opening.
    let may = ff_core::sim::season::real_clock_game_hours(Some(
        NaiveDate::from_ymd_opt(2026, 5, 1)
            .unwrap()
            .and_hms_opt(8, 0, 0)
            .unwrap(),
    ));
    let mut trip = Trip::new(
        route,
        truck,
        default_weather(),
        TripOptions {
            world: Some(world),
            career_hours: Some(may),
            start_hour: 8.0,
            ..TripOptions::seeded(93)
        },
    );
    trip.update(1.0);
    assert!(
        trip.game_minutes >= 90.0,
        "an Eastern 8 AM start waits out the Alaska night closure: {} minutes",
        trip.game_minutes
    );
}

#[test]
fn test_border_booths_follow_inbound_country_crossings() {
    let world = world();
    let direct_trip = |from: &str, toward: &str| {
        let leg = world
            .legs
            .iter()
            .find(|leg| leg.a == from && leg.b == toward)
            .unwrap_or_else(|| panic!("directed leg {from} -> {toward} exists"))
            .as_ref()
            .clone();
        let route = Route::from_legs(vec![from.to_string(), toward.to_string()], vec![leg]);
        let mut truck = TruckState::default();
        truck.transmission.automatic = true;
        truck.start_engine();
        Trip::new(
            route,
            truck,
            default_weather(),
            TripOptions {
                world: Some(world),
                ..TripOptions::seeded(17)
            },
        )
    };
    let cases = [
        (
            "tok_ak_us",
            "whitehorse_yt_ca",
            Some((
                "Beaver Creek Port of Entry",
                107.8,
                "CA",
                "Canada Border Services Agency",
            )),
        ),
        (
            "whitehorse_yt_ca",
            "tok_ak_us",
            Some((
                "Alcan Port of Entry",
                297.5,
                "US",
                "U.S. Customs and Border Protection",
            )),
        ),
        ("blaine_wa_us", "surrey_bc_ca", None),
        (
            "surrey_bc_ca",
            "blaine_wa_us",
            Some((
                "Blaine Pacific Highway POE",
                12.8,
                "US",
                "U.S. Customs and Border Protection",
            )),
        ),
    ];
    let mut booth_count = 0;
    for (from, toward, expected) in cases {
        let trip = direct_trip(from, toward);
        match expected {
            Some((name, at_mi, country, agency)) => {
                assert_eq!(trip.border_booths.len(), 1, "{from} -> {toward}");
                let booth = &trip.border_booths[0];
                assert_eq!(booth.name, name);
                assert!((booth.at_mi - at_mi).abs() < 0.001);
                assert_eq!(booth.entering_country, country);
                assert_eq!(booth.agency, agency);
                assert!(!trip.navigation_cues.iter().any(|cue| {
                    cue.kind == "checkpoint"
                        && cue.near_text.starts_with("Passing ")
                        && cue.near_text.contains(name)
                }));
                booth_count += trip.border_booths.len();
            }
            None => {
                assert!(trip.border_booths.is_empty(), "{from} -> {toward}");
                assert!(trip.navigation_cues.iter().any(|cue| {
                    cue.kind == "checkpoint"
                        && cue.near_text.starts_with("Passing ")
                        && cue.near_text.contains("Blaine Pacific Highway POE")
                }));
            }
        }
    }
    assert_eq!(booth_count, 3);

    for route in [
        world
            .route_from_cities(&["whitehorse_yt_ca", "tok_ak_us"])
            .expect("legacy Whitehorse to Tok route"),
        world
            .supported_route("whitehorse_yt_ca", "tok_ak_us", None)
            .expect("supported Whitehorse to Tok route")
            .expect("supported route exists"),
    ] {
        assert_eq!(route.legs.len(), 1);
        assert_eq!(route.legs[0].a, "whitehorse_yt_ca");
        assert_eq!(route.legs[0].b, "tok_ak_us");
        assert_eq!(route.legs[0].checkpoints()[0].name, "Alcan Port of Entry");
    }

    let forward_leg = world
        .legs
        .iter()
        .find(|leg| leg.a == "tok_ak_us" && leg.b == "whitehorse_yt_ca")
        .expect("Tok to Whitehorse leg exists")
        .as_ref()
        .clone();
    let reversed_route = Route::from_legs(
        vec!["whitehorse_yt_ca".to_string(), "tok_ak_us".to_string()],
        vec![forward_leg],
    );
    let mut truck = TruckState::default();
    truck.start_engine();
    let reversed_trip = Trip::new(
        reversed_route,
        truck,
        default_weather(),
        TripOptions {
            world: Some(world),
            ..TripOptions::seeded(17)
        },
    );
    assert!(reversed_trip.border_booths.is_empty());
    assert!(reversed_trip.navigation_cues.iter().any(|cue| {
        cue.kind == "checkpoint"
            && cue.near_text.starts_with("Passing ")
            && cue.near_text.contains("Beaver Creek Port of Entry")
    }));
    assert_eq!(
        ff_core::data::world_models::RouteCheckpoint::new(
            "Port",
            1.0,
            "border",
            "Yukon",
            "Alaska Highway",
        )
        .label(),
        "port of entry"
    );
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

    // The Turnpike settles where the truck leaves it, at interchange 6.
    trip.position_mi = 73.6;
    let quiet = trip.update(0.0);
    assert!(gps_events(&quiet).is_empty());

    trip.position_mi = 74.7;
    let advance = trip.update(0.0);
    let repeat = trip.update(0.0);

    assert_eq!(
        gps_messages(&advance),
        vec![
            "E-ZPass toll point ahead, New Jersey Turnpike settlement. \
             Toll 63 dollars, billed to carrier settlement."
        ]
    );
    assert!(gps_events(&repeat).is_empty());

    trip.position_mi = 76.5;
    let charged = trip.update(0.0);
    let charged_again = trip.update(0.0);

    assert_eq!(
        messages_of(&charged, TripEventKind::TollCharged),
        vec!["E-ZPass toll at New Jersey Turnpike settlement, 63 dollars, billed to carrier settlement."]
    );
    assert_eq!(trip.toll_expense(), 62.51);
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
        vec![
            "ticket system toll point ahead, Pennsylvania Turnpike eastern ticket entry. \
             Entry recorded for carrier settlement."
        ]
    );

    trip.position_mi = 18.0;
    let entry = trip.update(0.0);
    assert_eq!(
        gps_messages(&entry),
        vec![
            "ticket system entry recorded at Pennsylvania Turnpike eastern ticket entry. \
             Toll billed at carrier settlement."
        ]
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
        vec!["Crossing into Ohio. Passing Cleveland, Ohio. Continuing on I-76 toward Pittsburgh."]
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
        vec!["Passing Buffalo, New York. Continuing on I-90 toward Cleveland."]
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
fn published_tolls_carry_plate_rates_and_one_way_collection() {
    let world = world();
    let tolls = |a: &str, b: &str| {
        world
            .legs
            .iter()
            .find(|leg| leg.a == a && leg.b == b)
            .expect("leg")
            .toll_events()
            .to_vec()
    };
    let bay = |a: &str, b: &str| {
        tolls(a, b)
            .into_iter()
            .find(|toll| toll.name == "Chesapeake Bay Bridge toll")
            .expect("Bay Bridge toll")
    };
    let east = bay("baltimore_md_us", "dover_de_us");
    assert_eq!(
        (east.amount, east.plate_amount(), east.estimated),
        (24.0, 36.0, false)
    );
    assert!(east.applies_to_direction(true) && !east.applies_to_direction(false));
    let west = bay("salisbury_md_us", "baltimore_md_us");
    assert!(!west.applies_to_direction(true) && west.applies_to_direction(false));
    let gateway = tolls("pittsburgh_pa_us", "cleveland_oh_us")
        .into_iter()
        .find(|toll| toll.name.contains("Gateway"))
        .expect("Gateway toll");
    assert!(
        !gateway.applies_to_direction(true),
        "no toll westbound into Ohio"
    );
    let thruway = tolls("springfield_ma_us", "albany_ny_us");
    assert_eq!(
        thruway.len(),
        2,
        "Mass Pike and Berkshire Connector both charge"
    );
    assert!(thruway
        .iter()
        .all(|toll| !toll.estimated && toll.plate_amount() >= toll.amount));
    let carquinez = tolls("sacramento_ca_us", "san_francisco_ca_us")
        .into_iter()
        .find(|toll| toll.name == "Carquinez Bridge toll")
        .expect("Carquinez toll");
    assert!(
        !carquinez.applies_to_direction(true) && carquinez.applies_to_direction(false),
        "collected eastbound only"
    );
    let named = |a: &str, b: &str, name: &str| {
        tolls(a, b)
            .into_iter()
            .filter(|toll| toll.name == name)
            .collect::<Vec<_>>()
    };
    for (from, forward) in [
        ("newark_nj_us", true),
        ("allentown_pa_us", true),
        ("philadelphia_pa_us", false),
    ] {
        let (a, b) = if forward {
            (from, "new_york_ny_us")
        } else {
            ("new_york_ny_us", from)
        };
        let gwb = named(a, b, "George Washington Bridge toll");
        assert_eq!(gwb.len(), 1, "{from} to New York pays the GWB");
        let gwb = &gwb[0];
        assert_eq!((gwb.amount, gwb.plate_amount()), (113.35, 128.35));
        assert!(
            gwb.applies_to_direction(forward) && !gwb.applies_to_direction(!forward),
            "{from}: collected into New York only"
        );
    }
    for from in ["los_angeles_ca_us", "sacramento_ca_us", "stockton_ca_us"] {
        let bay = named(
            from,
            "san_francisco_ca_us",
            "San Francisco-Oakland Bay Bridge toll",
        );
        assert_eq!(bay.len(), 1, "{from} to San Francisco pays the Bay Bridge");
        assert!(
            bay[0].applies_to_direction(true) && !bay[0].applies_to_direction(false),
            "{from}: collected westbound into San Francisco only"
        );
        assert_eq!(bay[0].amount, 30.5);
    }
    let golden = named(
        "san_francisco_ca_us",
        "santa_rosa_ca_us",
        "Golden Gate Bridge toll",
    );
    assert_eq!(golden.len(), 1);
    assert_eq!((golden[0].amount, golden[0].plate_amount()), (51.25, 52.5));
    assert!(
        !golden[0].applies_to_direction(true) && golden[0].applies_to_direction(false),
        "collected southbound into San Francisco only"
    );
    let turnpike = named(
        "new_york_ny_us",
        "philadelphia_pa_us",
        "New Jersey Turnpike settlement",
    );
    assert_eq!(turnpike.len(), 2, "one settlement each way");
    for forward in [true, false] {
        let due: Vec<_> = turnpike
            .iter()
            .filter(|toll| toll.applies_to_direction(forward))
            .collect();
        assert_eq!(due.len(), 1, "forward={forward}");
        assert_eq!(
            (due[0].amount, due[0].plate_amount(), due[0].estimated),
            (62.51, 68.6, false)
        );
    }
}

#[test]
fn a_saved_trip_resumes_on_the_detour_its_load_was_planned_on() {
    let world = world();
    let fuel = CrossingLoad::standard_rig(true).with_cargo("fuel_bulk", true, 40_000);
    let plain = CrossingLoad::standard_rig(true);
    for (from, to) in [
        ("worcester_ma_us", "boston_ma_us"),
        ("boston_ma_us", "worcester_ma_us"),
        ("baltimore_md_us", "wilmington_de_us"),
        ("wilmington_de_us", "baltimore_md_us"),
    ] {
        let planned = world
            .legal_route(from, to, &fuel)
            .unwrap()
            .expect("legal road");
        let resumed = world
            .route_from_cities_for(&planned.cities, &fuel)
            .expect("resumes");
        assert!(
            resumed
                .legs
                .iter()
                .map(|l| l.id)
                .eq(planned.legs.iter().map(|l| l.id)),
            "{from} to {to} resumes on the road it was planned on"
        );
        let open = world
            .supported_route(from, to, None)
            .unwrap()
            .expect("open road");
        assert!(
            open.legs.iter().all(|l| l.detour_for.is_empty()),
            "{from} to {to}"
        );
        let plain_resumed = world
            .route_from_cities_for(&open.cities, &plain)
            .expect("resumes");
        assert!(
            plain_resumed
                .legs
                .iter()
                .map(|l| l.id)
                .eq(open.legs.iter().map(|l| l.id)),
            "{from} to {to} plain freight stays on the main road"
        );
    }
}
