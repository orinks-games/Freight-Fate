//! The co-driver judges a stop's reach at the pace the weather lets the
//! truck make. Ice holds the truck at 20, and a co-driver who planned a lot
//! 67 miles off at highway pace crawled toward it until their hours ran
//! out short of it (issue #518).

use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};
use freight_fate::states::driving_team::TeamMode;

const DT: f64 = 1.0 / 60.0;

/// The player has driven 10 hours and hands over in ride-along, still owed
/// their rest; Dan has 1.75 hours of driving before he hands back. The only
/// lots ahead are the ones at `lots` miles.
fn short_shift(name: &str, weather: WeatherKind, lots: &[f64]) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    harness.start_route(
        "Chicago",
        "Des Moines",
        RouteSetup::seeded(4242).named(name),
    );
    harness.app.ctx.settings.hos_mode = "realistic".to_string();
    harness
        .app
        .ctx
        .profile
        .as_mut()
        .expect("a career")
        .hos
        .drive(600.0);
    let lots = lots.to_vec();
    harness.with_drive(move |d, ctx| {
        d.tutorial = None;
        d.departure_checked = true;
        d.destination_exit_taken = true;
        d.trip.hazard_check_mi = 1e9;
        d.trip.inspection_check_mi = 1e9;
        d.trip.traffic_manager.rolling_bubble = false;
        d.trip.set_npc_vehicles(Vec::new());
        d.trip.traffic_pressures.clear();
        d.trip.zones.retain(|z| z.aadt.is_none());
        d.weather_mut().forced = Some(weather);
        d.weather_mut().current = weather;
        d.trip.set_patrols(Vec::new());
        d.trip.posts.clear();
        let lot = d
            .trip
            .stops
            .iter()
            .find(|stop| {
                stop.parking != "none"
                    && stop.stop_type != "weigh_station"
                    && stop.stop_type != "delivery_destination"
            })
            .cloned()
            .expect("a lot");
        let here = d.trip.position_mi;
        d.trip
            .stops
            .retain(|stop| stop.stop_type == "delivery_destination");
        for ahead in lots {
            let mut stop = lot.clone();
            stop.at_mi = here + ahead;
            stop.name = format!("{} at {ahead}", stop.name);
            d.trip.stops.push(stop);
        }
        d.trip.stops.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
        d.hand_wheel_to_co_driver(ctx, TeamMode::RideAlong);
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().parking_brake = false;
        let mph = if weather == WeatherKind::Ice {
            20.0
        } else {
            60.0
        };
        d.truck_mut().velocity_mps = mph / 2.236_936;
        d.cruise_mph = Some(60.0);
    });
    let hos = &mut harness.app.ctx.profile.as_mut().expect("a career").hos;
    hos.drive(7.0 * 60.0);
    hos.take_break(30.0);
    hos.drive(2.0 * 60.0);
    harness
}

fn frames(harness: &mut PlaytestHarness, n: usize) {
    for _ in 0..n {
        harness.advance_frame_clock();
        harness.app.frame(DT);
    }
}

fn making_for(harness: &PlaytestHarness) -> Option<String> {
    harness.read_drive(|d| {
        d.team
            .as_ref()
            .and_then(|team| team.wheel.as_ref())
            .and_then(|wheel| wheel.pilot.making_for.as_ref())
            .and_then(|(key, _)| d.trip.stops.iter().find(|stop| &stop.key() == key))
            .map(|stop| stop.name.clone())
    })
}

fn tail(harness: &PlaytestHarness) -> String {
    let lines = harness.transcript();
    let from = lines.len().saturating_sub(20);
    lines[from..].join("\n")
}

#[test]
fn on_ice_the_co_driver_plans_the_lot_their_hours_reach_at_20() {
    // At 50, 1.75 hours reach both lots; at 20 only the one 30 miles on.
    let mut harness = short_shift("Team Pace Ice", WeatherKind::Ice, &[30.0, 67.0]);
    frames(&mut harness, 600);
    let lot = making_for(&harness);
    assert!(
        lot.as_deref().is_some_and(|name| name.ends_with(" at 30")),
        "{lot:?}\n{}",
        tail(&harness)
    );
    let said = harness.transcript().join("\n");
    assert!(said.contains("Dan plans to stop at "), "{said}");
    assert!(!said.contains(" in 67.0 miles"), "{said}");
}

#[test]
fn a_lot_the_ice_puts_out_of_reach_is_dropped_from_the_plan() {
    let mut harness = short_shift("Team Pace Turns", WeatherKind::Clear, &[67.0, 150.0]);
    frames(&mut harness, 600);
    let lot = making_for(&harness);
    assert!(
        lot.as_deref().is_some_and(|name| name.ends_with(" at 67")),
        "planned in clear weather: {lot:?}\n{}",
        tail(&harness)
    );
    harness.with_drive(|d, _| {
        d.weather_mut().forced = Some(WeatherKind::Ice);
        d.weather_mut().current = WeatherKind::Ice;
    });
    frames(&mut harness, 60);
    let (lot, planned) = harness.read_drive(|d| {
        (
            d.team
                .as_ref()
                .and_then(|team| team.wheel.as_ref())
                .and_then(|wheel| wheel.pilot.making_for.clone()),
            d.trip.planned_stop_key.clone(),
        )
    });
    assert_eq!(lot, None, "{}", tail(&harness));
    assert_eq!(planned, None);
    assert!(
        harness.read_drive(|d| d.co_driver_driving()),
        "Dan drives on while his hours last: {}",
        tail(&harness)
    );
}

#[test]
fn a_set_speed_over_the_limit_is_brought_down_to_it() {
    // Crossing into Indiana's 65 for trucks, Dan held the 70 he had set in
    // Illinois (agent drive, 2026-10-26).
    let mut harness = short_shift("Team Pace Limit", WeatherKind::Clear, &[150.0]);
    harness.with_drive(|d, _| {
        d.speed_control_armed = true;
        d.cruise_mph = Some(80.0);
    });
    frames(&mut harness, 600);
    let (set, limit) = harness.with_drive(|d, _| {
        let (limit, _) = d.trip.speed_limit_at(d.trip.position_mi);
        (d.cruise_mph, limit)
    });
    assert!(
        set.is_some_and(|set| set <= limit),
        "set {set:?} over the limit {limit}: {}",
        tail(&harness)
    );
}
