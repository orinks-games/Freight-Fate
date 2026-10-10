//! Weather system tests.
//!
//! Ported from `tests/test_weather_trip.py` (every test; the ones that need
//! the trip simulation, the world data, the truck or the app shell are
//! `#[ignore]`d with the dependency named), the pure `WeatherSystem` + fake
//! provider tests of `tests/test_real_weather.py` (the rest need the NWS
//! provider in `sim::real_weather`), and a pin on `fmt_g`, the private
//! Python `%g` formatter `describe` uses for visibility.

use super::test_support::{env_write, new_system};
use super::*;
use crate::sim::season::CAREER_START_DAY_OF_YEAR;
use std::collections::HashSet;

fn system(region: &str, seed: i64) -> WeatherSystem {
    new_system(region, Some(seed), None, None, true)
}

fn system_at(region: &str, seed: i64, game_hours: f64) -> WeatherSystem {
    new_system(region, Some(seed), None, Some(game_hours), true)
}

fn with_provider(region: &str, seed: i64, provider: Box<dyn WeatherProvider>) -> WeatherSystem {
    new_system(region, Some(seed), Some(provider), None, true)
}

// -- tests/test_weather_trip.py ---------------------------------------------

#[test]
fn test_all_conditions_have_effects() {
    for kind in WeatherKind::ALL {
        assert!(EFFECTS.iter().any(|(k, _)| *k == kind));
        let _ = effects(kind);
    }
}

/// The surface field keys the traction-equipment ladder, so every row must
/// use one of the four words the physics understands.
#[test]
fn test_every_condition_names_a_real_surface() {
    for (kind, eff) in EFFECTS {
        assert!(
            ["dry", "wet", "snow", "ice"].contains(&eff.surface),
            "{kind:?}"
        );
    }
    assert_eq!(effects(WeatherKind::Snow).surface, "snow");
    assert_eq!(effects(WeatherKind::Ice).surface, "ice");
    assert_eq!(effects(WeatherKind::Rain).surface, "wet");
    assert_eq!(effects(WeatherKind::Clear).surface, "dry");
}

/// Conditions seen across a run with the career clock set to a season.
fn season_conditions(region: &str, game_hours: f64) -> HashSet<WeatherKind> {
    let mut ws = system_at(region, 5, game_hours);
    let mut seen = HashSet::from([ws.current]);
    for _ in 0..400 {
        ws.update(60.0); // advance about an hour per step
        seen.insert(ws.current);
    }
    seen
}

#[test]
fn test_summer_runs_have_no_snow() {
    let summer_hours = (200.0 - CAREER_START_DAY_OF_YEAR) * 24.0; // mid July
    let seen = season_conditions("great_lakes", summer_hours);
    assert!(!seen.contains(&WeatherKind::Snow));
}

#[test]
fn test_winter_runs_have_snow_but_no_thunderstorms() {
    let winter_hours = (15.0 - CAREER_START_DAY_OF_YEAR).rem_euclid(365.0) * 24.0; // mid January
    let seen = season_conditions("great_lakes", winter_hours);
    assert!(seen.contains(&WeatherKind::Snow));
    assert!(!seen.contains(&WeatherKind::Thunderstorm));
}

#[test]
fn test_seasonal_weather_is_deterministic_with_seed() {
    let winter_hours = (15.0_f64 - 80.0).rem_euclid(365.0) * 24.0;
    let mut a = system_at("rockies", 11, winter_hours);
    let mut b = system_at("rockies", 11, winter_hours);
    for _ in 0..80 {
        assert_eq!(a.update(45.0), b.update(45.0));
    }
    assert_eq!(a.current, b.current);
    assert_eq!(a.season(), b.season());
    assert_eq!(a.season(), Some("winter"));
}

#[test]
fn test_seasons_off_by_default_leaves_temperature_unknown() {
    let mut ws = system("heartland", 1);
    assert_eq!(ws.game_hours, None);
    assert_eq!(ws.temperature_c(), None);
    assert_eq!(ws.season(), None);
}

#[test]
fn test_weather_is_deterministic_with_seed() {
    let mut a = system("great_lakes", 7);
    let mut b = system("great_lakes", 7);
    for _ in 0..50 {
        assert_eq!(a.update(13.0), b.update(13.0));
    }
    assert_eq!(a.current, b.current);
}

#[test]
fn test_weather_eventually_changes() {
    let mut ws = system("pacific_northwest", 3);
    let changes: Vec<Option<WeatherKind>> = (0..200).map(|_| ws.update(15.0)).collect();
    assert!(changes.iter().any(|c| c.is_some()));
}

/// Spoken weather color sits on the wheel-time budget; the career clock
/// still burns drive time. 20x must not drain the color timer twenty times
/// as fast.
#[test]
fn test_weather_color_ticks_on_sitting_not_compressed_game_time() {
    let mut ws = system_at("heartland", 1, 100.0);
    ws.minutes_until_change = 10.0;
    ws.update_paced(30.0, 1.0);
    assert!(
        (ws.game_hours.unwrap() - 100.5).abs() < 1e-9,
        "career clock must still take the game minutes"
    );
    assert!(
        (ws.minutes_until_change - 9.0).abs() < 1e-9,
        "color timer must tick sitting minutes, not the compressed 30"
    );
}

#[test]
fn test_update_without_a_split_still_uses_one_clock() {
    // Rest skips and the older call keep one interval for both clocks.
    let mut ws = system_at("heartland", 1, 50.0);
    ws.minutes_until_change = 12.0;
    ws.update(6.0);
    assert!((ws.game_hours.unwrap() - 50.1).abs() < 1e-9);
    assert!((ws.minutes_until_change - 6.0).abs() < 1e-9);
}

#[test]
fn test_bad_weather_reduces_grip() {
    assert!(effects(WeatherKind::Snow).grip < effects(WeatherKind::Clear).grip);
    assert!(effects(WeatherKind::HeavyRain).grip < effects(WeatherKind::Rain).grip);
}

#[test]
fn test_forecast_returns_requested_segments() {
    let mut ws = system("atlantic_southeast", 1);
    assert_eq!(ws.forecast(3).len(), 3);
}

/// Pressing V speaks a forecast; it must not change future weather.
#[test]
fn test_forecast_does_not_regenerate_weather_timeline() {
    let mut with_forecast = system("great_lakes", 9);
    let mut untouched = system("great_lakes", 9);
    for _ in 0..5 {
        assert_eq!(with_forecast.forecast(2).len(), 2);
    }
    for _ in 0..80 {
        assert_eq!(with_forecast.update(10.0), untouched.update(10.0));
    }
    assert_eq!(with_forecast.current, untouched.current);
}

#[test]
fn test_force_weather_override_locks_condition() {
    const VAR: &str = "FREIGHT_FATE_FORCE_WEATHER";
    let _guard = env_write();
    std::env::set_var(VAR, "snow");
    let mut ws = WeatherSystem::new("heartland", Some(1), None, None, true);
    assert_eq!(ws.current, WeatherKind::Snow);
    for _ in 0..200 {
        // never drifts off the forced condition
        ws.update(30.0);
        assert_eq!(ws.current, WeatherKind::Snow);
    }

    std::env::set_var(VAR, "heavy_rain");
    assert_eq!(
        WeatherSystem::new("heartland", Some(1), None, None, true).current,
        WeatherKind::HeavyRain
    );
    std::env::remove_var(VAR);
    std::env::set_var(VAR, "bogus");
    assert_eq!(
        WeatherSystem::new("heartland", Some(1), None, None, true).forced,
        None
    );
    std::env::remove_var(VAR);
}

#[test]
fn test_force_weather_accepts_every_python_spelling() {
    const VAR: &str = "FREIGHT_FATE_FORCE_WEATHER";
    let _guard = env_write();
    for (spelling, kind) in [
        ("ICE", WeatherKind::Ice),
        ("freezing rain", WeatherKind::Ice),
        ("freezing_rain", WeatherKind::Ice),
        (" Heavy Rain ", WeatherKind::HeavyRain),
        ("high_winds", WeatherKind::Wind),
        ("wind", WeatherKind::Wind),
    ] {
        std::env::set_var(VAR, spelling);
        assert_eq!(forced_weather(), Some(kind), "{spelling:?}");
    }
    std::env::set_var(VAR, "   ");
    assert_eq!(forced_weather(), None);
    std::env::remove_var(VAR);
    assert_eq!(forced_weather(), None);
}

// -- tests/test_real_weather.py: the pure WeatherSystem + fake-provider tests --

struct ConditionsOnlyProvider;

impl WeatherProvider for ConditionsOnlyProvider {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}

    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        Some(WeatherKind::HeavyRain)
    }

    fn stale(&mut self, _city: &str) -> bool {
        false
    }

    fn unavailable(&mut self, _city: &str) -> bool {
        false
    }
}

#[test]
fn test_live_report_omits_modeled_temperature_when_observation_has_none() {
    let mut ws = with_provider("desert_southwest", 1, Box::new(ConditionsOnlyProvider));
    ws.set_city("route-cell", 33.45, -112.07);
    ws.update(1.0);

    assert_eq!(ws.source_status(), "live");
    assert!(ws.temperature_c().is_some()); // The seasonal model remains available to mechanics.
    assert!(!ws.report_lead(true).contains("degrees"));
    assert!(!ws.source_conditions(true).contains("degrees"));
    assert!(!ws.source_conditions(true).contains("visibility"));
    assert!(!ws.source_conditions(true).contains("slick roads"));
}

/// Per-cell readings the test rewrites between ticks; the `Rc<RefCell>` is
/// the Python test reaching into the provider object it still holds.
type SharedReadings =
    std::rc::Rc<std::cell::RefCell<std::collections::HashMap<String, Option<WeatherKind>>>>;

struct LocationProvider {
    data: SharedReadings,
}

impl WeatherProvider for LocationProvider {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}

    fn get(&mut self, city: &str) -> Option<WeatherKind> {
        self.data.borrow().get(city).copied().flatten()
    }

    fn stale(&mut self, _city: &str) -> bool {
        false
    }

    fn unavailable(&mut self, _city: &str) -> bool {
        false
    }
}

#[test]
fn test_late_observation_for_previous_route_cell_cannot_replace_current_cell() {
    let data: SharedReadings = Default::default();
    data.borrow_mut().insert("cell-a".to_string(), None);
    data.borrow_mut()
        .insert("cell-b".to_string(), Some(WeatherKind::Rain));
    let provider = LocationProvider { data: data.clone() };
    let mut ws = with_provider("great_lakes", 1, Box::new(provider));
    ws.set_city("cell-a", 41.0, -87.0);
    ws.update(0.0);
    ws.set_city("cell-b", 40.0, -86.0);
    ws.update(0.0);
    assert_eq!(ws.current, WeatherKind::Rain);

    data.borrow_mut()
        .insert("cell-a".to_string(), Some(WeatherKind::HeavyRain));
    ws.update(0.0);
    assert_eq!(ws.city.as_deref(), Some("cell-b"));
    assert_eq!(ws.current, WeatherKind::Rain);
}

struct Pending;

impl WeatherProvider for Pending {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}

    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        None
    }

    fn unavailable(&mut self, _city: &str) -> bool {
        false // still fetching, not offline
    }
}

/// With a provider attached, weather starts clear and holds -- no simulated
/// warm-up -- until live data (or a confirmed offline state) arrives.
#[test]
fn test_weather_system_holds_clear_while_live_data_pending() {
    let mut ws = with_provider("pacific_northwest", 1, Box::new(Pending));
    ws.set_city("Seattle", 47.61, -122.33);
    assert_eq!(ws.current, WeatherKind::Clear);
    for _ in 0..200 {
        assert_eq!(ws.update(30.0), None); // no simulated transitions while pending
    }
    assert_eq!(ws.current, WeatherKind::Clear);
    assert!(!ws.live);
}

#[test]
fn test_weather_system_without_provider_unchanged() {
    let mut ws = system("great_lakes", 3);
    ws.update(1.0);
    assert!(!ws.live);
}

/// A provider that is offline from the first tick (the shape of
/// `SyncProvider(fetch=raise OSError)` in the Python tests, minus the NWS
/// machinery): the system must fall back to simulated weather and say so.
struct Offline;

impl WeatherProvider for Offline {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}

    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        None
    }

    fn unavailable(&mut self, _city: &str) -> bool {
        true
    }
}

#[test]
fn test_weather_system_falls_back_when_offline() {
    let mut ws = with_provider("great_lakes", 2, Box::new(Offline));
    ws.set_city("Chicago", 41.88, -87.63);
    let changes: Vec<Option<WeatherKind>> = (0..200).map(|_| ws.update(15.0)).collect();
    assert!(!ws.live);
    assert!(changes.iter().any(|c| c.is_some())); // simulated weather still evolves
    assert_eq!(ws.source_status(), "fallback");
    assert!(ws
        .report_lead(true)
        .starts_with("Simulated fallback weather: "));
    assert_eq!(ws.observation_age_value(), "not applicable");
}

/// A stable live feed with a temperature and an observation age: the spoken
/// report lines, end to end, without the NWS provider.
struct SteadyRain {
    age_s: f64,
    refreshing: bool,
}

impl WeatherProvider for SteadyRain {
    fn request(&mut self, _city: &str, _lat: f64, _lon: f64) {}

    fn get(&mut self, _city: &str) -> Option<WeatherKind> {
        Some(WeatherKind::Rain)
    }

    fn get_temperature(&mut self, _city: &str) -> Option<f64> {
        Some(18.0)
    }

    fn observation_age_s(&mut self, _city: &str) -> Option<f64> {
        Some(self.age_s)
    }

    fn refreshing(&mut self, _city: &str) -> bool {
        self.refreshing
    }
}

#[test]
fn test_live_report_lines_read_the_station_back() {
    let provider = SteadyRain {
        age_s: 12.0 * 60.0,
        refreshing: false,
    };
    let mut ws = with_provider("great_lakes", 1, Box::new(provider));
    ws.set_city("Chicago", 41.88, -87.63);
    assert_eq!(ws.update(1.0), Some(WeatherKind::Rain));
    assert!(ws.live);
    assert_eq!(ws.source_status(), "live");
    assert_eq!(ws.temperature_c(), Some(18.0));
    // 18 C -> 64.4 F -> "64".
    assert_eq!(ws.source_conditions(true), "rain, 64 degrees");
    assert_eq!(ws.source_conditions(false), "rain, 18 degrees Celsius");
    assert_eq!(
        ws.report_lead(true),
        "Live weather: rain, 64 degrees. The observation is 12 minutes old"
    );
    assert_eq!(ws.observation_age_value(), "12 minutes old");
    assert_eq!(ws.event_source_label(), "Live weather");
    assert_eq!(ws.conditions_label(), "Live conditions");
    assert!(!ws.has_simulated_forecast());
    assert_eq!(ws.describe(true, false), "rain, 64 degrees");
    // Stable live data: no further changes, simulation stays paused.
    for _ in 0..100 {
        assert_eq!(ws.update(30.0), None);
    }
}

#[test]
fn test_observation_age_text_rounds_down_to_whole_minutes() {
    let mut ws = with_provider(
        "great_lakes",
        1,
        Box::new(SteadyRain {
            age_s: 59.0,
            refreshing: false,
        }),
    );
    ws.set_city("Chicago", 41.88, -87.63);
    assert_eq!(
        ws.observation_age_text().as_deref(),
        Some("The observation is less than a minute old")
    );
    assert_eq!(ws.observation_age_value(), "less than a minute old");

    let mut ws = with_provider(
        "great_lakes",
        1,
        Box::new(SteadyRain {
            age_s: 119.0,
            refreshing: true,
        }),
    );
    ws.set_city("Chicago", 41.88, -87.63);
    assert_eq!(
        ws.observation_age_text().as_deref(),
        Some("The observation is 1 minute old")
    );
    assert_eq!(
        ws.last_known_notice(),
        "The observation is 1 minute old. Live weather is updating for your current location"
    );
    // No city tracked: nothing to age.
    let mut ws = with_provider(
        "great_lakes",
        1,
        Box::new(SteadyRain {
            age_s: 0.0,
            refreshing: false,
        }),
    );
    assert_eq!(ws.observation_age_text(), None);
    assert_eq!(ws.last_known_notice(), "Observation age is unavailable");
    assert_eq!(
        ws.live_observation_notice(),
        "Observation age is unavailable"
    );
    assert_eq!(ws.observation_age_value(), "unavailable");
    assert!(ws.live_weather_loading());
    assert_eq!(ws.source_status(), "loading");
    assert_eq!(ws.source_conditions(true), "neutral conditions");
    assert_eq!(
        ws.report_lead(true),
        "Live weather is loading for your current route position. \
         Temporary neutral driving conditions in use"
    );
}

// -- describe(): the spoken condition line ----------------------------------

#[test]
fn test_describe_reads_visibility_with_python_g_formatting() {
    let _guard = env_write();
    std::env::set_var("FREIGHT_FATE_FORCE_WEATHER", "fog");
    let mut fog = WeatherSystem::new("heartland", Some(1), None, None, true);
    std::env::set_var("FREIGHT_FATE_FORCE_WEATHER", "thunderstorm");
    let mut storm = WeatherSystem::new("heartland", Some(1), None, None, true);
    std::env::set_var("FREIGHT_FATE_FORCE_WEATHER", "heavy rain");
    let mut heavy = WeatherSystem::new("heartland", Some(1), None, None, true);
    std::env::set_var("FREIGHT_FATE_FORCE_WEATHER", "ice");
    let mut ice = WeatherSystem::new("heartland", Some(1), None, None, true);
    std::env::remove_var("FREIGHT_FATE_FORCE_WEATHER");
    drop(_guard);

    assert_eq!(fog.describe(true, false), "fog, visibility 0.3 miles");
    assert_eq!(
        fog.describe(false, false),
        "fog, visibility 0.482803 kilometers"
    );
    assert_eq!(
        storm.describe(true, false),
        "thunderstorm, visibility 1 miles, slick roads"
    );
    assert_eq!(
        storm.describe(false, false),
        "thunderstorm, visibility 1.60934 kilometers, slick roads"
    );
    assert_eq!(
        heavy.describe(true, false),
        "heavy rain, visibility 1.5 miles, slick roads"
    );
    assert_eq!(
        heavy.describe(false, false),
        "heavy rain, visibility 2.41402 kilometers, slick roads"
    );
    assert_eq!(ice.describe(true, false), "freezing rain, ice on the road");
}

#[test]
fn test_fmt_g_matches_python_format_g() {
    assert_eq!(fmt_g(0.3), "0.3");
    assert_eq!(fmt_g(1.0), "1");
    assert_eq!(fmt_g(1.5), "1.5");
    assert_eq!(fmt_g(0.3 * 1.609344), "0.482803");
    assert_eq!(fmt_g(1.5 * 1.609344), "2.41402");
    assert_eq!(fmt_g(1.0 * 1.609344), "1.60934");
    assert_eq!(fmt_g(0.0), "0");
    assert_eq!(fmt_g(100000.0), "100000");
    assert_eq!(fmt_g(1000000.0), "1e+06");
    assert_eq!(fmt_g(0.0001), "0.0001");
    assert_eq!(fmt_g(0.00001), "1e-05");
    assert_eq!(fmt_g(123456789.0), "1.23457e+08");
    assert_eq!(fmt_g(-2.5), "-2.5");
}

#[test]
fn test_weather_kind_round_trips_its_spoken_value() {
    for kind in WeatherKind::ALL {
        assert_eq!(WeatherKind::from_value(kind.value()), Some(kind));
    }
    assert_eq!(WeatherKind::from_value("drizzle"), None);
    assert_eq!(WeatherKind::Ice.value(), "freezing rain");
    assert_eq!(WeatherKind::HeavyRain.name(), "HEAVY_RAIN");
    assert_eq!(region_weights("atlantis"), &DEFAULT_WEIGHTS[..]);
    assert_eq!(region_weights("heartland"), &DEFAULT_WEIGHTS[..]);
    assert_eq!(REGION_WEIGHTS.len(), 16);
}

#[test]
fn test_forecast_weights_append_an_unlisted_current_kind_last() {
    // Freezing rain is not in any region table; `weights[cur] = ... * 2.5`
    // appends it at the end of the dict, which is where random.choices saw
    // it. The draw order is what makes a seeded forecast reproducible.
    let mut weights: Vec<(WeatherKind, f64)> = region_weights("heartland").to_vec();
    set_weight(&mut weights, WeatherKind::Ice, |w| w.unwrap_or(1.0) * 2.5);
    assert_eq!(weights.len(), 9);
    assert_eq!(weights[8], (WeatherKind::Ice, 2.5));
    set_weight(&mut weights, WeatherKind::Clear, |w| w.unwrap_or(1.0) * 2.5);
    assert_eq!(weights[0], (WeatherKind::Clear, 10.0));
    assert_eq!(weights.len(), 9);
}

// -- the rest of tests/test_weather_trip.py, waiting on their dependencies --

#[test]
fn test_all_regions_in_world_have_weights() {
    let regions: HashSet<&str> = crate::data::world::get_world()
        .cities
        .values()
        .map(|c| c.region.as_str())
        .collect();
    for region in regions {
        assert!(
            REGION_WEIGHTS.iter().any(|(r, _)| *r == region),
            "no weather weights for {region}"
        );
    }
}

#[test]
fn test_route_weather_coordinates_follow_multiple_points_on_long_leg() {
    let world = crate::data::world::get_world();
    let route = first_route("chicago_il_us", "indianapolis_in_us");
    let mut trip = trip_on(route.clone(), system("great_lakes", 1), seeded(2));

    let start = trip.latlon_at(Some(0.0));
    let middle = trip.latlon_at(Some(route.miles() / 2.0));
    let end = trip.latlon_at(Some(route.miles()));

    let chicago = world.city("chicago_il_us").unwrap();
    let indy = world.city("indianapolis_in_us").unwrap();
    assert!(approx_abs(start.0, chicago.lat, 0.001) && approx_abs(start.1, chicago.lon, 0.001));
    assert!(approx_abs(end.0, indy.lat, 0.001) && approx_abs(end.1, indy.lon, 0.001));
    assert!(start.0 > middle.0 && middle.0 > end.0);
    assert!(start.1 < middle.1 && middle.1 < end.1);

    let mut keys = Vec::new();
    for position in [0.0, 19.9, 20.0, 59.9, 60.0, route.miles() - 1.0] {
        trip.position_mi = position;
        keys.push(trip.weather_location().unwrap().0);
    }
    assert_eq!(keys[0], keys[1]);
    assert_ne!(keys[1], keys[2]);
    assert_ne!(keys[3], keys[4]);
    let distinct: HashSet<&String> = keys.iter().collect();
    assert_eq!(distinct.len(), 5);
}

#[test]
fn test_route_weather_coordinates_reverse_with_travel_direction() {
    use crate::data::world_models::Route;

    let world = crate::data::world::get_world();
    let route = first_route("chicago_il_us", "indianapolis_in_us");
    let mut cities = route.cities.clone();
    cities.reverse();
    let mut legs = route.legs.clone();
    legs.reverse();
    let reverse = Route::new(cities, legs);
    let trip = trip_on(reverse.clone(), system("corn_belt", 1), seeded(2));
    let indy = world.city("indianapolis_in_us").unwrap();
    let chicago = world.city("chicago_il_us").unwrap();
    let start = trip.latlon_at(Some(0.0));
    assert!(approx_abs(start.0, indy.lat, 0.001) && approx_abs(start.1, indy.lon, 0.001));
    let end = trip.latlon_at(Some(reverse.miles()));
    assert!(approx_abs(end.0, chicago.lat, 0.001) && approx_abs(end.1, chicago.lon, 0.001));
}

#[test]
fn test_route_weather_location_switches_at_multi_leg_boundary() {
    use crate::data::world_models::{CorridorDetail, Leg, Route, RoutePoint};

    let rp = |at_mi: f64, lat: f64, lon: f64| RoutePoint { at_mi, lat, lon };
    let leg_one = Leg::new(
        "chicago_il_us",
        "indianapolis_in_us",
        40.0,
        "I-65",
        "flat",
        Vec::new(),
    )
    .with_detail(CorridorDetail {
        route_points: vec![rp(0.0, 41.8781, -87.6298), rp(40.0, 39.7684, -86.1581)],
        ..Default::default()
    });
    let leg_two = Leg::new(
        "indianapolis_in_us",
        "columbus_oh_us",
        40.0,
        "I-70",
        "flat",
        Vec::new(),
    )
    .with_detail(CorridorDetail {
        route_points: vec![rp(0.0, 39.7684, -86.1581), rp(40.0, 39.9612, -82.9988)],
        ..Default::default()
    });
    let route = Route::from_legs(
        vec![
            "chicago_il_us".to_string(),
            "indianapolis_in_us".to_string(),
            "columbus_oh_us".to_string(),
        ],
        vec![leg_one, leg_two],
    );
    let mut trip = trip_on(route, system("great_lakes", 1), seeded(2));

    trip.position_mi = 39.9;
    let before = trip.weather_location().unwrap().0;
    trip.position_mi = 40.0;
    let boundary = trip.weather_location().unwrap().0;
    trip.position_mi = 40.1;
    let after = trip.weather_location().unwrap().0;

    assert_ne!(before, boundary);
    assert_eq!(boundary, after);
    assert!(boundary.contains("indianapolis_in_us:columbus_oh_us"));
}

#[test]
fn test_normal_route_cell_refresh_is_silent_and_failures_hold_last_known() {
    use crate::sim::trip_models::TripEventKind;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Shared {
        data: HashMap<String, WeatherKind>,
        failed: HashSet<String>,
    }
    struct MovingProvider(Arc<Mutex<Shared>>);
    impl WeatherProvider for MovingProvider {
        fn request(&mut self, _: &str, _: f64, _: f64) {}
        fn get(&mut self, key: &str) -> Option<WeatherKind> {
            self.0.lock().unwrap().data.get(key).copied()
        }
        fn unavailable(&mut self, key: &str) -> bool {
            self.0.lock().unwrap().failed.contains(key)
        }
    }

    let shared = Arc::new(Mutex::new(Shared::default()));
    let route = first_route("chicago_il_us", "indianapolis_in_us");
    let weather = with_provider(
        "great_lakes",
        1,
        Box::new(MovingProvider(Arc::clone(&shared))),
    );
    let mut trip = trip_on(route, weather, seeded(2));

    trip.position_mi = 0.0;
    let first_key = trip.weather_location().unwrap().0;
    shared
        .lock()
        .unwrap()
        .data
        .insert(first_key, WeatherKind::Clear);
    trip.update(0.0);

    trip.position_mi = 20.0;
    let second_key = trip.weather_location().unwrap().0;
    assert!(kind_messages(&trip.update(0.0), TripEventKind::WeatherChange).is_empty());
    shared
        .lock()
        .unwrap()
        .data
        .insert(second_key, WeatherKind::Clear);
    assert!(kind_messages(&trip.update(0.0), TripEventKind::WeatherChange).is_empty());

    trip.position_mi = 40.0;
    let third_key = trip.weather_location().unwrap().0;
    shared.lock().unwrap().failed.insert(third_key);
    let events = kind_messages(&trip.update(0.0), TripEventKind::WeatherChange);
    // One dropped request at a cell boundary never simulates: the sky holds
    // the last live conditions silently (owner ruling, 2026-08-08).
    assert!(events.is_empty());
    assert_eq!(trip.weather.source_status(), "last_known");
    assert_eq!(trip.weather.current, WeatherKind::Clear);
}

#[test]
fn test_live_change_omits_modeled_temperature_and_does_not_hide_later_stale_status() {
    use crate::sim::trip_models::TripEventKind;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct Shared {
        data: HashMap<String, WeatherKind>,
        stale_keys: HashSet<String>,
        failed_keys: HashSet<String>,
    }
    struct MovingProvider(Arc<Mutex<Shared>>);
    impl WeatherProvider for MovingProvider {
        fn request(&mut self, _: &str, _: f64, _: f64) {}
        fn get(&mut self, key: &str) -> Option<WeatherKind> {
            self.0.lock().unwrap().data.get(key).copied()
        }
        fn stale(&mut self, key: &str) -> bool {
            self.0.lock().unwrap().stale_keys.contains(key)
        }
        fn observation_age_s(&mut self, key: &str) -> Option<f64> {
            Some(if self.0.lock().unwrap().stale_keys.contains(key) {
                12.0 * 60.0
            } else {
                0.0
            })
        }
        fn refresh_failed(&mut self, key: &str) -> bool {
            self.0.lock().unwrap().failed_keys.contains(key)
        }
    }

    let shared = Arc::new(Mutex::new(Shared::default()));
    let route = first_route("chicago_il_us", "indianapolis_in_us");
    let weather = with_provider(
        "great_lakes",
        1,
        Box::new(MovingProvider(Arc::clone(&shared))),
    );
    let mut trip = trip_on(route, weather, seeded(2));

    trip.position_mi = 0.0;
    let first_key = trip.weather_location().unwrap().0;
    shared
        .lock()
        .unwrap()
        .data
        .insert(first_key, WeatherKind::Clear);
    trip.update(0.0);

    trip.position_mi = 20.0;
    let second_key = trip.weather_location().unwrap().0;
    shared
        .lock()
        .unwrap()
        .data
        .insert(second_key.clone(), WeatherKind::HeavyRain);
    let changed = kind_messages(&trip.update(0.0), TripEventKind::WeatherChange);
    assert_eq!(changed.len(), 1, "{changed:?}");
    assert!(
        changed[0].starts_with("Live weather changing: heavy rain"),
        "{}",
        changed[0]
    );
    assert!(!changed[0].contains("degrees"));

    shared.lock().unwrap().stale_keys.insert(second_key.clone());
    shared.lock().unwrap().failed_keys.insert(second_key);
    let delayed = kind_messages(&trip.update(0.0), TripEventKind::WeatherChange);
    assert_eq!(delayed.len(), 1, "{delayed:?}");
    assert!(
        delayed[0].starts_with("The observation is 12 minutes old"),
        "{}",
        delayed[0]
    );
    assert!(delayed[0].contains("Last-known conditions in use"));
    assert!(!delayed[0].to_lowercase().contains("updat"));
}

#[test]
fn test_freshly_fetched_old_observation_change_stays_live_and_announces_age() {
    use crate::sim::trip_models::TripEventKind;

    struct OldObservationProvider;
    impl WeatherProvider for OldObservationProvider {
        fn request(&mut self, _: &str, _: f64, _: f64) {}
        fn get(&mut self, _: &str) -> Option<WeatherKind> {
            Some(WeatherKind::HeavyRain)
        }
        fn observation_age_s(&mut self, _: &str) -> Option<f64> {
            Some(12.0 * 60.0)
        }
    }

    let route = first_route("chicago_il_us", "indianapolis_in_us");
    let weather = with_provider("great_lakes", 1, Box::new(OldObservationProvider));
    let mut trip = trip_on(route, weather, seeded(2));

    let changes = kind_messages(&trip.update(0.0), TripEventKind::WeatherChange);
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert!(
        changes[0].starts_with("Live weather changing: heavy rain"),
        "{}",
        changes[0]
    );
    assert!(changes[0].contains("The observation is 12 minutes old"));
    assert!(!changes[0].to_lowercase().contains("updat"));
}

#[test]
#[ignore = "Python monkeypatched WeatherSystem._sample to force the fallback condition"]
fn test_offline_live_weather_change_is_identified_as_simulated_fallback() {}

#[test]
fn test_relaxed_hazard_scale_lowers_hazard_risk() {
    // Relaxed mode keeps random road hazards rare via the hazard scale.
    use crate::sim::hos::RELAXED_HAZARD_SCALE;

    let normal = make_trip(4, 1.0);
    let relaxed = make_trip(4, RELAXED_HAZARD_SCALE);
    // Same route, weather, and clock: the only difference is the scale.
    assert!((relaxed.hazard_risk() - normal.hazard_risk() * RELAXED_HAZARD_SCALE).abs() < 1e-9);
    assert!(relaxed.hazard_risk() < normal.hazard_risk());
}

#[test]
fn test_corridor_busyness_scales_hazard_check_frequency() {
    let mut dense = trip_on(
        route_of(&["New York", "Boston"]),
        system("northeast", 1),
        seeded(1),
    );
    let mut sparse = trip_on(
        route_of(&["Las Vegas", "Reno"]),
        system("great_basin", 1),
        seeded(1),
    );
    dense.position_mi = 25.0;
    sparse.position_mi = sparse.total_miles() / 2.0;
    assert!(
        dense.corridor_hazard_factor_at(dense.position_mi)
            > sparse.corridor_hazard_factor_at(sparse.position_mi)
    );
}

#[test]
fn test_hazard_check_interval_shortens_on_busy_corridors() {
    // Python pinned the uniform(20, 60) draw to 40 with a fixed RNG; here
    // both trips re-seed their stream identically so the one draw matches.
    use crate::pyrandom::PyRandom;

    let mut dense = trip_on(
        route_of(&["New York", "Boston"]),
        system("northeast", 1),
        seeded(1),
    );
    let mut sparse = trip_on(
        route_of(&["Las Vegas", "Reno"]),
        system("great_basin", 1),
        seeded(1),
    );
    dense.position_mi = 25.0;
    sparse.position_mi = sparse.total_miles() / 2.0;
    dense.rng = PyRandom::new_from_i64(40);
    sparse.rng = PyRandom::new_from_i64(40);
    assert!(dense.next_hazard_check_interval_mi() < sparse.next_hazard_check_interval_mi());
}

#[test]
fn test_relaxed_mode_thins_traffic_density() {
    // Relaxed mode also makes ambient traffic rarer, not just hazards.
    use crate::sim::hos::RELAXED_HAZARD_SCALE;

    let normal = make_trip(4, 1.0);
    let relaxed = make_trip(4, RELAXED_HAZARD_SCALE);
    let leg = normal.route.legs[0].clone();
    let expected = normal.leg_traffic_density(&leg, 0.0, false) * RELAXED_HAZARD_SCALE;
    assert!((relaxed.leg_traffic_density(&leg, 0.0, false) - expected).abs() < 1e-9);
    assert!(
        relaxed.leg_traffic_density(&leg, 0.0, false)
            < normal.leg_traffic_density(&leg, 0.0, false)
    );
}

#[test]
fn test_relaxed_mode_reduces_merge_exit_pressure() {
    use crate::sim::hos::RELAXED_HAZARD_SCALE;

    let normal = make_trip(4, 1.0);
    let relaxed = make_trip(4, RELAXED_HAZARD_SCALE);

    let normal_exit = normal
        .traffic_pressures
        .iter()
        .find(|p| p.kind == "exit")
        .expect("an exit pressure")
        .clone();
    let stop_mile = normal.stops[0].at_mi - 2.0;

    let expected = normal.traffic_pressure_intensity(stop_mile, "exit") * RELAXED_HAZARD_SCALE;
    assert!((relaxed.traffic_pressure_intensity(stop_mile, "exit") - expected).abs() < 1e-9);
    if let Some(relaxed_exit) = relaxed.traffic_pressures.iter().find(|p| p.kind == "exit") {
        assert!(relaxed_exit.intensity < normal_exit.intensity);
        assert!(relaxed_exit.target_speed_mph > normal_exit.target_speed_mph);
    }
}

// -- Trip fixtures shared by the trip half of test_weather_trip.py -------------

/// `make_trip(world, start, end, seed=2, **kwargs)`: a quiet run with an
/// automatic, running truck and the rolling traffic bubble off. The
/// truck lives on the trip (`trip.truck`).
fn make_trip_on(
    start: &str,
    end: &str,
    opts: crate::sim::trip::TripOptions,
) -> crate::sim::trip::Trip {
    use crate::data::world::get_world;
    use crate::sim::trip::{Trip, TripOptions};
    use crate::sim::vehicle::TruckState;

    let route = get_world().route_options(start, end, 3, false).unwrap()[0].clone();
    let mut truck = TruckState::default();
    truck.transmission.automatic = true;
    truck.start_engine();
    let mut trip = Trip::new(
        route,
        truck,
        system("great_lakes", 1),
        TripOptions {
            seed: Some(opts.seed.unwrap_or(2)),
            ..opts
        },
    );
    trip.traffic_manager.rolling_bubble = false;
    trip
}

/// `Trip(route, TruckState(), weather, seed=..., **kwargs)`.
fn trip_on(
    route: crate::data::world_models::Route,
    weather: WeatherSystem,
    opts: crate::sim::trip::TripOptions,
) -> crate::sim::trip::Trip {
    use crate::sim::trip::Trip;
    use crate::sim::vehicle::TruckState;

    Trip::new(route, TruckState::default(), weather, opts)
}

fn seeded(seed: i64) -> crate::sim::trip::TripOptions {
    crate::sim::trip::TripOptions::seeded(seed)
}

/// `world.route_from_cities([...])`.
fn route_of(cities: &[&str]) -> crate::data::world_models::Route {
    crate::data::world::get_world()
        .route_from_cities(cities)
        .unwrap_or_else(|| panic!("no route through {cities:?}"))
}

/// `world.route_options(start, end)[0]`.
fn first_route(start: &str, end: &str) -> crate::data::world_models::Route {
    crate::data::world::get_world()
        .route_options(start, end, 3, false)
        .unwrap()[0]
        .clone()
}

/// GPS-cue events, excluding additive interchange/exit cues.
fn gps_events(
    events: &[crate::sim::trip_models::TripEvent],
) -> Vec<crate::sim::trip_models::TripEvent> {
    use crate::sim::trip_models::TripEventKind;
    events
        .iter()
        .filter(|e| {
            e.kind == TripEventKind::GpsCue
                && e.data
                    .cue
                    .as_ref()
                    .is_none_or(|cue| cue.kind != "interchange")
        })
        .cloned()
        .collect()
}

fn gps_messages(events: &[crate::sim::trip_models::TripEvent]) -> Vec<String> {
    gps_events(events)
        .iter()
        .map(|e| e.text().to_string())
        .collect()
}

fn kind_messages(
    events: &[crate::sim::trip_models::TripEvent],
    kind: crate::sim::trip_models::TripEventKind,
) -> Vec<String> {
    events
        .iter()
        .filter(|e| e.kind == kind)
        .map(|e| e.text().to_string())
        .collect()
}

/// A controllable clock for `trip.event_breather` in tests that teleport
/// across several announceable events with no real time between them.
struct FakeClock {
    now: std::rc::Rc<std::cell::Cell<f64>>,
}

impl FakeClock {
    fn new() -> Self {
        Self {
            now: std::rc::Rc::new(std::cell::Cell::new(0.0)),
        }
    }

    fn clock(&self) -> crate::speech_pacing::Clock {
        let now = std::rc::Rc::clone(&self.now);
        Box::new(move || now.get())
    }

    fn advance(&self, seconds: f64) {
        self.now.set(self.now.get() + seconds);
    }
}

/// `_brake_until_speed`: the INSPECTION events raised while braking down.
fn brake_until_speed(
    trip: &mut crate::sim::trip::Trip,
    target_mph: f64,
    emergency: bool,
    limit_s: f64,
) -> Vec<crate::sim::trip_models::TripEvent> {
    use crate::sim::trip_models::TripEventKind;

    let dt = 1.0 / 60.0;
    trip.truck.throttle = 0.0;
    trip.truck.brake = 1.0;
    trip.truck.emergency_brake = emergency;
    let mut inspections = Vec::new();
    for _ in 0..((limit_s / dt) as usize) {
        trip.truck.auto_shift();
        trip.truck.update(dt);
        inspections.extend(
            trip.update(dt)
                .into_iter()
                .filter(|e| e.kind == TripEventKind::Inspection),
        );
        if trip.truck.speed_mph() <= target_mph {
            break;
        }
    }
    inspections
}

/// The lane-closure sentence the construction warning carries, if any.
fn closure_part(zone: &crate::sim::trip_models::Zone) -> String {
    match zone.closed_lane {
        None => "All lanes open through the work. ".to_string(),
        Some(lane) => {
            let (shut, keep) = if lane == 0 {
                ("right", "left")
            } else {
                ("left", "right")
            };
            format!("The {shut} lane is closed, merge {keep} before the work zone. ")
        }
    }
}

fn approx_abs(a: f64, b: f64, tol: f64) -> bool {
    (a - b).abs() <= tol
}

// -- Trip-backed cases (tests/test_weather_trip.py::make_trip) -------------------

/// `make_trip(world, seed=..., hazard_scale=...)`: a quiet Chicago-Indianapolis
/// run with an automatic, running truck and the rolling bubble off.
fn make_trip(seed: i64, hazard_scale: f64) -> crate::sim::trip::Trip {
    use crate::data::world::get_world;
    use crate::sim::trip::{Trip, TripOptions};
    use crate::sim::vehicle::TruckState;

    let route = get_world()
        .route_options("Chicago", "Indianapolis", 3, false)
        .unwrap()[0]
        .clone();
    let mut truck = TruckState::default();
    truck.transmission.automatic = true;
    truck.start_engine();
    let mut trip = Trip::new(
        route,
        truck,
        system("great_lakes", 1),
        TripOptions {
            seed: Some(seed),
            hazard_scale,
            ..Default::default()
        },
    );
    trip.traffic_manager.rolling_bubble = false;
    trip
}

#[test]
fn real_time_empty_bubble_arrival_reaches_the_player_traffic_status() {
    use crate::sim::traffic_manager::TrafficVehicle;
    use crate::sim::traffic_manager::{BUBBLE_AHEAD_MI, BUBBLE_BEHIND_MI, SPAWN_CELL_MI};

    let mut trip = make_trip(7, 1.0);
    trip.time_scale = 1.0;
    trip.position_mi = 20.0;
    trip.traffic_manager.rolling_bubble = true;
    trip.traffic_manager.vehicles.clear();
    trip.traffic_manager.vehicles.push(TrafficVehicle::new(
        "future:traffic",
        trip.position_mi + 50.0,
        65.0,
        65.0,
        0,
        "cruising",
        "car",
    ));
    let first = ((trip.position_mi - BUBBLE_BEHIND_MI) / SPAWN_CELL_MI) as i64;
    let last = ((trip.position_mi + BUBBLE_AHEAD_MI) / SPAWN_CELL_MI) as i64;
    trip.traffic_manager.spawned_cells.extend(first..=last);

    for _ in 0..90 {
        trip.traffic_manager
            .update(1.0, trip.position_mi, 1.0, Some(12.0), Some(false));
        if trip
            .traffic_manager
            .vehicles
            .iter()
            .any(|vehicle| vehicle.key.starts_with("real-time:"))
        {
            break;
        }
    }

    let status = trip.npc_traffic_status();
    assert!(!status.contains("no close traffic"), "{status}");
    // A slow lead is called once it is within earshot, so close on it.
    if let Some(arrival) = trip
        .traffic_manager
        .vehicles
        .iter()
        .find(|vehicle| vehicle.key.starts_with("real-time:"))
    {
        trip.position_mi = arrival.position_mi - 0.3;
    }
    trip.check_npc_traffic_cues();
    assert!(
        trip.events
            .iter()
            .any(|event| event.data.npc_vehicle.is_some()),
        "the Real time arrival never reached the automatic traffic cue"
    );
}

#[test]
fn test_relaxed_mode_thins_random_inspection_odds() {
    // Relaxed mode pulls a violating driver over less often; the random log
    // check is thinned by the hazard scale (weigh stations are not).
    use crate::sim::hos::RELAXED_HAZARD_SCALE;

    let normal = make_trip(4, 1.0);
    let relaxed = make_trip(4, RELAXED_HAZARD_SCALE);
    let leg = normal.route.legs[0].clone();
    let expected = normal.random_inspection_odds(&leg) * RELAXED_HAZARD_SCALE;
    assert!((relaxed.random_inspection_odds(&leg) - expected).abs() < 1e-9);
    assert!(relaxed.random_inspection_odds(&leg) < normal.random_inspection_odds(&leg));
}

#[test]
fn test_corridor_speed_limit_by_highway_and_region() {
    use crate::sim::trip_models::{corridor_speed_limit, BASE_SPEED_LIMIT_MPH};

    // Rural Interstates run faster out West, slower in the Northeast.
    assert_eq!(corridor_speed_limit("I-80", "great_basin"), 80.0);
    assert_eq!(corridor_speed_limit("I-90", "northeast"), 65.0);
    assert_eq!(corridor_speed_limit("I-70", "heartland"), 70.0);
    // US highways and state routes are slower than Interstates.
    assert_eq!(corridor_speed_limit("US-30", "heartland"), 65.0);
    assert_eq!(corridor_speed_limit("SR-99", "california"), 60.0);
    // An unknown region on an Interstate falls back to the base limit.
    assert_eq!(
        corridor_speed_limit("I-5", "atlantis"),
        BASE_SPEED_LIMIT_MPH
    );
}

#[test]
fn test_speed_limit_varies_by_corridor_and_drops_in_cities() {
    use crate::sim::trip_models::URBAN_LIMIT_MPH;

    let mut trip = make_trip(2, 1.0); // Chicago -> Indianapolis, an Interstate corridor
                                      // Near the origin city the limit drops to the urban value.
    let (near_city, reason) = trip.speed_limit_at(1.0);
    assert!(reason.is_none());
    assert_eq!(near_city, URBAN_LIMIT_MPH);
    // Out on the open road it is the faster corridor limit.
    let half = trip.total_miles() / 2.0;
    let (open_road, reason) = trip.speed_limit_at(half);
    assert!(reason.is_none());
    assert!(open_road >= 65.0);
    assert!(open_road > near_city);
}

#[test]
fn test_speed_limit_change_is_announced_crossing_out_of_a_city() {
    use crate::sim::trip_models::{TripEventKind, URBAN_RADIUS_MI};

    let mut trip = make_trip(2, 1.0);
    trip.truck.throttle = 0.95;
    let mut messages: Vec<String> = Vec::new();
    for _ in 0..8000 {
        trip.truck.auto_shift();
        trip.truck.update(1.0 / 60.0);
        for event in trip.update(1.0 / 60.0) {
            if event.kind == TripEventKind::GpsCue {
                messages.push(event.text().to_string());
            }
        }
        if trip.position_mi > URBAN_RADIUS_MI + 4.0 {
            break;
        }
    }
    // Leaving the urban stretch raises the posted limit, and that is spoken.
    assert!(messages.iter().any(|m| m.contains("Speed limit")));
}

#[test]
fn test_speed_limit_cue_names_direction_and_city() {
    // Python forced the corridor limit to 45 then 65 by monkeypatching; here
    // the announced limit is set on the other side of the real posting so
    // the same drop and rise happen against the road as baked.
    use crate::sim::road_event_pacing::LIMIT_GAP_REAL_S;

    let mut trip = make_trip(2, 1.0); // Chicago -> Indianapolis
    trip.entered_zone = None;
    let clock = FakeClock::new();
    trip.event_breather.set_clock(clock.clock());

    // A drop near the origin city names the direction and the city.
    trip.position_mi = 0.0;
    let posted = trip.corridor_limit_at(0.0);
    trip.announced_speed_limit = Some(posted + 20.0);
    trip.events.clear();
    trip.check_speed_limit();
    let lowered: Vec<String> = trip.events.iter().map(|e| e.text().to_string()).collect();
    assert!(
        lowered
            .iter()
            .any(|m| m.contains("reduced to") && m.contains("approaching")),
        "{lowered:?}"
    );

    clock.advance(LIMIT_GAP_REAL_S);

    // A rise just states the higher value -- no "approaching" on the way up.
    trip.announced_speed_limit = Some(posted - 20.0);
    trip.events.clear();
    trip.check_speed_limit();
    let raised: Vec<String> = trip.events.iter().map(|e| e.text().to_string()).collect();
    assert!(raised.iter().any(|m| m.contains("raised to")), "{raised:?}");
    assert!(raised.iter().all(|m| !m.contains("approaching")));
}

#[test]
fn test_speed_limit_drop_behind_a_city_says_leaving() {
    // Owner-found live leaving Sedona (2026-07-20): a drop with the town in
    // the mirror must not claim you are approaching it.
    let mut trip = make_trip(2, 1.0); // Chicago -> Indianapolis
    trip.entered_zone = None;
    trip.position_mi = 2.0; // past Chicago's milepost, still inside its radius
    let posted = trip.corridor_limit_at(2.0);
    trip.announced_speed_limit = Some(posted + 10.0);
    trip.events.clear();
    trip.check_speed_limit();
    let lowered: Vec<String> = trip.events.iter().map(|e| e.text().to_string()).collect();
    assert!(
        lowered
            .iter()
            .any(|m| m.contains("reduced to") && m.contains("leaving")),
        "{lowered:?}"
    );
    assert!(lowered.iter().all(|m| !m.contains("approaching")));
}

#[test]
fn test_weather_drag_multiplier_increases_resistance() {
    use crate::sim::vehicle::TruckState;

    let mut truck = TruckState {
        velocity_mps: 25.0,
        ..TruckState::default()
    };
    let base = truck.resistance_force();
    truck.drag_mult = 1.25; // a strong headwind / storm
    assert!(truck.resistance_force() > base);
}

#[test]
fn test_visibility_shortens_hazard_reaction() {
    let mut trip = make_trip(2, 1.0);
    trip.weather.current = WeatherKind::Clear;
    assert_eq!(trip.visibility_reaction_factor(), 1.0);
    trip.weather.current = WeatherKind::HeavyRain; // 1.5 mi visibility
    assert!((trip.visibility_reaction_factor() - 0.5).abs() < 1e-9);
    trip.weather.current = WeatherKind::Fog; // 0.3 mi -> floored
    assert!((trip.visibility_reaction_factor() - 0.4).abs() < 1e-9);
}

#[test]
fn test_too_fast_for_conditions_risks_traction_loss() {
    use crate::sim::trip_models::TripEventKind;

    let mut trip = make_trip(2, 1.0);
    trip.hazard_check_mi = 1e9; // silence the random environmental hazards
    trip.inspection_check_mi = 1e9;
    trip.traffic_manager.rolling_bubble = false;
    trip.traffic_manager.vehicles = Vec::new();

    let mut hits: Vec<String> = Vec::new();
    for _ in 0..12000 {
        trip.weather.current = WeatherKind::Snow; // grip 0.45, safe speed 35
        trip.truck.velocity_mps = 27.0; // ~60 mph, well over safe
        for e in trip.update(1.0 / 60.0) {
            if e.kind == TripEventKind::Hazard {
                hits.push(e.text().to_string());
            }
        }
        if !hits.is_empty() {
            break;
        }
    }
    assert!(
        hits.iter().any(|m| m.contains("sliding on the snow")),
        "{hits:?}"
    );

    // At a safe speed for the snow, no traction-loss incident fires.
    let mut trip2 = make_trip(7, 1.0);
    trip2.hazard_check_mi = 1e9;
    trip2.inspection_check_mi = 1e9;
    trip2.traffic_manager.rolling_bubble = false;
    trip2.traffic_manager.vehicles = Vec::new();
    let mut safe_hits: Vec<String> = Vec::new();
    for _ in 0..6000 {
        trip2.weather.current = WeatherKind::Snow;
        trip2.truck.velocity_mps = 14.0; // ~31 mph, under safe 35
        for e in trip2.update(1.0 / 60.0) {
            if e.kind == TripEventKind::Hazard {
                safe_hits.push(e.text().to_string());
            }
        }
    }
    assert!(!safe_hits.iter().any(|m| m.contains("sliding on the snow")));
}

#[test]
fn test_trip_completes_and_emits_arrival() {
    use crate::sim::trip_models::TripEventKind;

    let mut trip = make_trip(2, 1.0);
    trip.truck.throttle = 0.85;
    let mut arrived = false;
    let mut frames = 0;
    loop {
        trip.truck.auto_shift();
        trip.truck.update(1.0 / 60.0);
        arrived |= trip
            .update(1.0 / 60.0)
            .iter()
            .any(|e| e.kind == TripEventKind::Arrived);
        assert!(frames < 60 * 60 * 30, "trip never finished");
        frames += 1;
        if trip.finished {
            break;
        }
    }
    assert!(arrived);
    assert_eq!(trip.remaining_miles(), 0.0);
}

#[test]
fn test_trip_announces_stops_ahead() {
    use crate::sim::trip_models::TripEventKind;

    let mut trip = make_trip(2, 1.0);
    trip.truck.throttle = 0.85;
    let mut announced = false;
    for _ in 0..(60 * 60 * 10) {
        trip.truck.auto_shift();
        trip.truck.update(1.0 / 60.0);
        announced |= trip
            .update(1.0 / 60.0)
            .iter()
            .any(|e| e.kind == TripEventKind::StopAhead);
        if trip.finished || announced {
            break;
        }
    }
    assert!(announced);
}

#[test]
fn test_trip_uses_explicit_stop_positions() {
    let trip = make_trip(2, 1.0);
    let by_name = |name: &str| {
        trip.stops
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("{name} on the route"))
            .clone()
    };
    // Hand-curated stops keep their explicit checked-in positions and parking,
    // even with additive OpenStreetMap stops now interleaved on the leg.
    // Rescaled with the leg: Chicago-Indianapolis was 183 miles and is 185,
    // so a curated stop keeps its PLACE on the road rather than its old number.
    assert_eq!(by_name("Pilot Travel Center Remington").at_mi, 94.52);
    assert_eq!(by_name("Loves Travel Stop Lafayette").at_mi, 122.63);
    assert_eq!(
        by_name("Pilot Travel Center Remington").parking,
        "confirmed"
    );
    assert_eq!(by_name("Loves Travel Stop Lafayette").parking, "confirmed");
    // No stop sits at the naive route midpoint, and every stop declares a
    // concrete, non-unknown parking value.
    assert!(trip
        .stops
        .iter()
        .all(|s| s.at_mi != trip.route.miles() / 2.0));
    assert!(trip.stops.iter().all(|s| s.parking != "unknown"));
}

#[test]
fn test_trip_uses_only_curated_pois_at_runtime() {
    let route = route_of(&["Memphis", "Nashville"]);
    let trip = trip_on(route.clone(), system("great_lakes", 1), seeded(2));

    assert!(!route.raw_stop_details().is_empty());
    assert!(route.raw_stop_details().iter().all(|s| s.curated()));
    assert!(!route.stop_details().is_empty());
    assert!(!trip.stops.is_empty());
    let route_stops = route.stop_details();
    let curated: HashSet<&str> = route_stops.iter().map(|s| s.name.as_str()).collect();
    assert!(trip.stops.iter().all(|s| curated.contains(s.name.as_str())));
}

#[test]
fn test_trip_places_reverse_route_stops_from_travel_direction() {
    let route = route_of(&["Dallas", "San Antonio"]);
    let trip = trip_on(route, system("southern_plains", 1), seeded(2));

    let position = |name: &str| {
        let stop = trip
            .stops
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("{name} on the route"));
        (stop.at_mi * 10.0).round() / 10.0
    };
    // Curated stops are positioned from the direction of travel (not the raw
    // stored order); additive OSM stops do not displace them.
    assert_eq!(position("Hill County Safety Rest Area"), 56.8);
    assert_eq!(position("Road Ranger Waco"), 89.7);
    assert_eq!(position("Bell County Safety Rest Area"), 136.5);
    // Every stop stays ordered along the direction of travel.
    let ats: Vec<f64> = trip.stops.iter().map(|s| s.at_mi).collect();
    let mut sorted = ats.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(ats, sorted);
}

#[test]
fn test_zone_speed_limits_apply() {
    let mut trip = make_trip_on("Atlanta", "Dallas", seeded(2));
    assert!(
        !trip.zones.is_empty(),
        "long route should have at least one zone"
    );
    // Congestion zones follow the clock; test against an always-on zone.
    let zone = trip
        .zones
        .iter()
        .find(|z| z.aadt.is_none())
        .cloned()
        .expect("an always-on zone");
    let inside = (zone.start_mi + zone.end_mi) / 2.0;
    let (limit, reason) = trip.speed_limit_at(inside);
    assert_eq!(limit, zone.limit_mph);
    assert_eq!(reason.as_deref(), Some(zone.reason.as_str()));
    let (limit, reason) = trip.speed_limit_at(zone.end_mi + 50.0);
    assert!(reason.is_none() || limit != zone.limit_mph);
}

#[test]
fn test_delivery_final_miles_use_facility_approach_limits() {
    use crate::sim::trip_models::RAMP_MAX_MPH;

    let mut trip = make_trip(2, 1.0);
    let total = trip.total_miles();
    let (limit, reason) = trip.speed_limit_at(total - 1.0);
    assert_eq!(limit, RAMP_MAX_MPH);
    assert_eq!(reason.as_deref(), Some("destination approach"));
    let (limit, reason) = trip.speed_limit_at(total - 0.2);
    assert_eq!(limit, 15.0);
    assert_eq!(reason.as_deref(), Some("facility gate"));
}

#[test]
fn test_the_last_three_miles_are_not_a_thirty_five_wall() {
    // Shane, 2026-08-15: the truck slowed to a crawl miles from the exit.
    use crate::sim::trip_models::RAMP_MAX_MPH;

    let mut trip = make_trip(2, 1.0);
    let total = trip.total_miles();
    let (limit, reason) = trip.speed_limit_at(total - 3.0);
    assert!(reason.is_none());
    assert!(limit > RAMP_MAX_MPH);
    let approach = trip
        .zones
        .iter()
        .find(|z| z.reason == "destination approach")
        .expect("the approach zone");
    assert_eq!(approach.limit_mph, RAMP_MAX_MPH);
    assert!(
        approach.limit_mph >= RAMP_MAX_MPH,
        "the exit must stay enterable"
    );
}

#[test]
fn test_the_destination_approach_starts_where_the_shed_needs_it() {
    use crate::sim::trip_models::{approach_shed_mi, DESTINATION_LOCAL_APPROACH_MI, RAMP_MAX_MPH};

    let trip = make_trip(2, 1.0);
    let approach = trip
        .zones
        .iter()
        .find(|z| z.reason == "destination approach")
        .expect("the approach zone")
        .clone();
    let entry_mph = trip.corridor_limit_at(approach.start_mi);
    let expected = trip.total_miles()
        - (DESTINATION_LOCAL_APPROACH_MI + approach_shed_mi(entry_mph, RAMP_MAX_MPH));
    assert!(approx_abs(approach.start_mi, expected, 0.05));
    assert!(approx_abs(approach.end_mi, trip.total_miles(), 1e-6));
    // Whatever the corridor runs at, the shed is a fraction of a mile.
    assert!(approach_shed_mi(entry_mph, RAMP_MAX_MPH) < 1.0);
}

#[test]
fn test_a_facility_with_a_longer_approach_road_gets_a_longer_zone() {
    use crate::data::world_constants::FACILITY_APPROACH_TRUSTED_MAX_MI;
    use crate::sim::trip::TripOptions;
    use crate::sim::trip_models::RAMP_MAX_MPH;

    let with = |mi: f64| {
        make_trip_on(
            "Chicago",
            "Indianapolis",
            TripOptions {
                destination_approach_mi: Some(mi),
                ..Default::default()
            },
        )
    };
    let approach = |trip: &crate::sim::trip::Trip| {
        trip.zones
            .iter()
            .find(|z| z.reason == "destination approach")
            .expect("the approach zone")
            .clone()
    };
    let near = with(0.6);
    let far = with(2.5);
    let near_zone = approach(&near);
    let far_zone = approach(&far);
    assert!(far.total_miles() - far_zone.start_mi > near.total_miles() - near_zone.start_mi);
    assert_eq!(far_zone.limit_mph, RAMP_MAX_MPH);
    assert_eq!(near_zone.limit_mph, RAMP_MAX_MPH);

    // A record longer than any real approach road is geocoding noise.
    let wild = with(35.0);
    let wild_zone = approach(&wild);
    assert!(wild.total_miles() - wild_zone.start_mi <= FACILITY_APPROACH_TRUSTED_MAX_MI + 1.0);
}

#[test]
fn test_the_facility_gate_zone_is_unchanged() {
    use crate::sim::trip_models::{FACILITY_GATE_LIMIT_MPH, FACILITY_GATE_ZONE_MI};

    let trip = make_trip(2, 1.0);
    let gate = trip
        .zones
        .iter()
        .find(|z| z.reason == "facility gate")
        .expect("the gate zone");
    assert_eq!(gate.limit_mph, FACILITY_GATE_LIMIT_MPH);
    assert!(approx_abs(
        gate.start_mi,
        trip.total_miles() - FACILITY_GATE_ZONE_MI,
        1e-6
    ));
    assert!(approx_abs(gate.end_mi, trip.total_miles(), 1e-6));
}

#[test]
fn test_the_approach_speaks_no_more_often_than_it_used_to() {
    let trip = make_trip(2, 1.0);
    let arrival: Vec<&str> = trip
        .zones
        .iter()
        .filter(|z| z.reason == "destination approach" || z.reason == "facility gate")
        .map(|z| z.reason.as_str())
        .collect();
    assert_eq!(arrival, vec!["destination approach", "facility gate"]);
}

/// A Chicago facility whose approach is the short synthetic single leg: no
/// street chain, and short enough that the whole of it is access road. It
/// used to be written as "Chicago's first facility", which held only while
/// that facility's endpoint sat on the 2.1-mile floor; the 2026-09-17
/// endpoint re-sweep moved it from a commuter line to a real intermodal
/// yard 3.6 miles out, and a long approach starts as an arterial.
fn short_synthetic_approach(world: &crate::data::world::World) -> crate::data::world_models::Route {
    world
        .city("Chicago")
        .unwrap()
        .locations
        .iter()
        .filter_map(|location| {
            world
                .facility_approach_route("Chicago", &location.name)
                .ok()
        })
        .find(|route| route.legs.len() == 1 && route.miles() <= 2.5)
        .expect("Chicago has a facility with a short synthetic approach")
}

#[test]
fn test_pickup_deadhead_route_uses_local_facility_limits() {
    let world = crate::data::world::get_world();
    let route = short_synthetic_approach(world);
    let mut trip = trip_on(route, system("great_lakes", 1), seeded(2));

    let (limit, reason) = trip.speed_limit_at(0.1);
    assert_eq!(limit, 25.0);
    assert_eq!(reason.as_deref(), Some("facility access road"));

    let total = trip.total_miles();
    let (limit, reason) = trip.speed_limit_at(total - 0.2);
    assert_eq!(limit, 15.0);
    assert_eq!(reason.as_deref(), Some("facility gate"));
}

#[test]
fn test_driving_through_a_city_lists_its_stops_once() {
    // A city's stops hang off every leg that meets it, a mile out from the
    // endpoint, so passing through collected the same facility twice.
    use crate::sim::trip_models::SHARED_CITY_STOP_MERGE_MI;

    let world = crate::data::world::get_world();
    let route = world
        .shortest_route(
            &world.resolve_city_key("Chicago"),
            &world.resolve_city_key("Los Angeles"),
            None,
            false,
        )
        .unwrap()
        .expect("a route");
    let trip = trip_on(route.clone(), system("midwest", 1), seeded(7));
    let mut stops = trip.stops.clone();
    stops.sort_by(|a, b| a.at_mi.partial_cmp(&b.at_mi).unwrap());
    let per_leg: usize = route
        .legs
        .iter()
        .enumerate()
        .map(|(i, leg)| {
            leg.stops
                .iter()
                .filter(|s| s.curated() && s.applies_to_direction(route.cities[i] == leg.a))
                .count()
        })
        .sum();
    assert!(
        stops.len() < per_leg,
        "route no longer exercises shared-city stops"
    );
    for pair in stops.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if b.at_mi - a.at_mi <= SHARED_CITY_STOP_MERGE_MI {
            assert_ne!(
                a.name,
                b.name,
                "{} listed twice {:.2} mi apart",
                a.name,
                b.at_mi - a.at_mi
            );
        }
    }
}

#[test]
fn test_a_merged_city_stop_keeps_an_exit_label() {
    use crate::sim::trip::Trip;
    use crate::sim::trip_models::RoadStop;

    let stop = |at_mi: f64, exit_label: &str| {
        let mut s = RoadStop::new("Pilot", at_mi, "travel_center");
        s.actions = vec!["fuel".to_string()];
        s.exit_label = exit_label.to_string();
        s
    };
    let merged = Trip::merge_shared_city_stops(vec![
        stop(100.0, ""),
        stop(102.0, "exit 2A"),
        stop(140.0, "exit 60"),
    ]);
    let ats: Vec<f64> = merged.iter().map(|s| s.at_mi).collect();
    assert_eq!(ats, vec![100.0, 140.0]); // twin folded in, far kept
    assert_eq!(merged[0].exit_label, "exit 2A");
}

/// Two stops on the trip that share a name, nearest first.
///
/// New York to Miami used to pass a run of stops all called "Love's Travel
/// Stop", and these cases leaned on that. The 2026-09-17 store import gave
/// each its town, so the map no longer promises a namesake anywhere. One is
/// made when the route has none: the rule under test keys a plan by name AND
/// mile, and needs only two stops that differ in the mile alone.
fn namesakes_on(trip: &mut crate::sim::trip::Trip) -> Vec<crate::sim::trip_models::RoadStop> {
    use crate::sim::trip_models::RoadStop;
    let mut by_name: std::collections::BTreeMap<String, Vec<RoadStop>> = Default::default();
    for stop in &trip.stops {
        by_name
            .entry(stop.name.clone())
            .or_default()
            .push(stop.clone());
    }
    let mut namesakes = by_name
        .into_values()
        .find(|group| group.len() >= 2)
        .unwrap_or_else(|| {
            let first = trip
                .stops
                .iter()
                .find(|s| s.stop_type == "travel_center")
                .cloned()
                .expect("a travel center on the route");
            let mut twin = first.clone();
            twin.at_mi = first.at_mi + 60.0;
            trip.stops.push(twin.clone());
            trip.stops.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
            vec![first, twin]
        });
    namesakes.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
    namesakes
}

#[test]
fn test_signaling_for_a_namesake_does_not_pass_as_taking_the_planned_exit() {
    let world = crate::data::world::get_world();
    let route = world
        .shortest_route(
            &world.resolve_city_key("New York"),
            &world.resolve_city_key("Miami"),
            None,
            false,
        )
        .unwrap()
        .expect("a route");
    let mut trip = trip_on(route, system("southeast", 1), seeded(7));
    let namesakes = namesakes_on(&mut trip);
    let (planned, other) = (namesakes[0].clone(), namesakes[1].clone());
    trip.planned_stop_key = Some(planned.key());

    // Signaling for a *different* Love's must not cover a blown planned stop.
    trip.exit_in_progress = Some(other.key());
    trip.position_mi = planned.at_mi + 1.0;
    trip.events = Vec::new();
    trip.check_stops();
    assert!(trip
        .events
        .iter()
        .any(|e| e.text().contains("your planned stop")));

    // Signaling for the planned one itself still stays quiet.
    trip.planned_stop_key = Some(planned.key());
    trip.exit_in_progress = Some(planned.key());
    trip.events = Vec::new();
    trip.check_stops();
    assert!(!trip
        .events
        .iter()
        .any(|e| e.text().contains("your planned stop")));
    assert_eq!(trip.planned_stop_key, Some(planned.key()));
}

#[test]
fn test_a_plan_survives_passing_a_stop_that_shares_its_name() {
    let world = crate::data::world::get_world();
    let route = world
        .shortest_route(
            &world.resolve_city_key("New York"),
            &world.resolve_city_key("Miami"),
            None,
            false,
        )
        .unwrap()
        .expect("a route");
    let mut trip = trip_on(route, system("southeast", 1), seeded(7));
    let namesakes = namesakes_on(&mut trip);
    assert!(namesakes.len() >= 2);
    let (target, earlier) = (namesakes[namesakes.len() - 1].clone(), namesakes[0].clone());
    trip.planned_stop_key = Some(target.key());

    // Only the stop actually planned counts as planned.
    let planned: Vec<String> = trip
        .stops
        .iter()
        .filter(|s| trip.is_planned(s))
        .map(|s| s.key())
        .collect();
    assert_eq!(planned, vec![target.key()]);

    // Roll past an earlier namesake: the plan is untouched and stays quiet.
    trip.position_mi = earlier.at_mi + 1.0;
    trip.events = Vec::new();
    trip.check_stops();
    assert!(!trip
        .events
        .iter()
        .any(|e| e.text().contains("planned stop")));
    assert_eq!(trip.planned_stop_key, Some(target.key()));

    // Past the planned stop itself, it cancels as before.
    trip.position_mi = target.at_mi + 1.0;
    trip.events = Vec::new();
    trip.check_stops();
    assert!(trip
        .events
        .iter()
        .any(|e| e.text().contains("your planned stop")));
    assert!(trip.planned_stop_key.is_none());
}

#[test]
fn test_every_stop_announces_even_when_names_repeat() {
    use crate::sim::trip_models::TripEventKind;

    let world = crate::data::world::get_world();
    let route = world
        .shortest_route(
            &world.resolve_city_key("New York"),
            &world.resolve_city_key("Miami"),
            None,
            false,
        )
        .unwrap()
        .expect("a route");
    let mut trip = trip_on(route, system("southeast", 1), seeded(7));
    let repeated = trip
        .stops
        .iter()
        .any(|s| trip.stops.iter().filter(|o| o.name == s.name).count() > 1);
    assert!(
        repeated,
        "route no longer exercises repeated stop names; pick another"
    );

    let mut announced: HashSet<String> = HashSet::new();
    let mut stops = trip.stops.clone();
    stops.sort_by(|a, b| a.at_mi.partial_cmp(&b.at_mi).unwrap());
    for stop in &stops {
        trip.position_mi = stop.at_mi - 1.0;
        trip.events = Vec::new();
        trip.check_stops();
        for event in &trip.events {
            if event.kind == TripEventKind::StopAhead {
                announced.insert(event.data.stop.as_ref().unwrap().key());
            }
        }
    }
    assert_eq!(announced.len(), trip.stops.len());
}

#[test]
fn test_facility_gate_warns_before_final_low_speed_zone() {
    use crate::sim::trip_models::TripEventKind;

    let world = crate::data::world::get_world();
    let route = short_synthetic_approach(world);
    let mut trip = trip_on(route, system("great_lakes", 1), seeded(2));

    trip.position_mi = trip.total_miles() - 2.0;
    // Two ticks, because the teleport lands ON a zone change: an advance
    // warning never shares a breath with the arrival line for the zone the
    // truck is entering.
    let mut events = trip.update(0.0);
    events.extend(trip.update(0.0));
    let warnings = kind_messages(&events, TripEventKind::GpsCue);
    assert!(
        warnings
            .iter()
            .any(|w| w == "In 2 miles, facility gate ahead. Speed limit 15."),
        "{warnings:?}"
    );
}

#[test]
fn test_zone_entry_is_worded_apart_from_its_advance_warning() {
    // The heads-up and the change itself must not sound alike.
    use crate::sim::road_event_pacing::ZONE_GAP_REAL_S;
    use crate::sim::trip_models::TripEventKind;

    let world = crate::data::world::get_world();
    let route = short_synthetic_approach(world);
    let mut trip = trip_on(route, system("great_lakes", 1), seeded(2));
    let gate = trip
        .zones
        .iter()
        .find(|z| z.reason == "facility gate")
        .cloned()
        .expect("the gate zone");

    let clock = FakeClock::new();
    trip.event_breather.set_clock(clock.clock());

    trip.position_mi = gate.start_mi - 2.0;
    let mut events = trip.update(0.0);
    events.extend(trip.update(0.0));
    let warning = events
        .iter()
        .filter(|e| e.kind == TripEventKind::GpsCue && e.text().contains("facility gate"))
        .map(|e| e.text().to_string())
        .next()
        .expect("the gate warning");
    // Two miles out: a heads-up, and the gate limit is not in force yet.
    assert_eq!(warning, "In 2 miles, facility gate ahead. Speed limit 15.");
    let pos = trip.position_mi;
    assert_ne!(trip.speed_limit_at(pos).0, 15.0);

    clock.advance(ZONE_GAP_REAL_S);
    trip.position_mi = gate.start_mi + 0.05;
    let entries = kind_messages(&trip.update(0.0), TripEventKind::ZoneEnter);
    let entry = entries.last().expect("a zone entry").clone();
    assert_eq!(entry, "Entering facility gate zone. Speed limit 15.");
    assert_ne!(entry, warning.split_once(", ").unwrap().1);
    let pos = trip.position_mi;
    assert_eq!(
        trip.speed_limit_at(pos),
        (15.0, Some("facility gate".to_string()))
    );
}

#[test]
fn test_construction_zone_warns_before_entry() {
    use crate::sim::trip_models::CONSTRUCTION_TAPER_LIMIT_MPH;

    let mut trip = make_trip(2, 1.0);
    let zone = trip
        .zones
        .iter()
        .find(|z| z.reason == "construction")
        .cloned()
        .expect("a construction zone");
    // A warned curve pins the clock (its own feature, its own test).
    trip.curves = Vec::new();
    trip.truck.velocity_mps = 70.0 / 2.23694;
    trip.time_scale = 20.0;

    let lookahead = trip.zone_warning_lookahead_mi();
    assert!(lookahead >= 6.0);

    trip.position_mi = zone.start_mi - lookahead;
    let events = trip.update(0.0);
    let warnings = gps_messages(&events);
    assert_eq!(
        warnings,
        vec![format!(
            "In {}, construction ahead. {}Speed limit {:.0} from one mile out, then {:.0} through the work zone.",
            trip.ahead_text(lookahead),
            closure_part(&zone),
            CONSTRUCTION_TAPER_LIMIT_MPH,
            zone.limit_mph
        )]
    );
}

#[test]
fn test_construction_zone_has_staged_merge_taper() {
    use crate::sim::trip_models::{CONSTRUCTION_TAPER_LIMIT_MPH, CONSTRUCTION_TAPER_MI};

    let mut trip = make_trip(2, 1.0);
    let zone = trip
        .zones
        .iter()
        .find(|z| z.reason == "construction")
        .cloned()
        .expect("a construction zone");
    let taper = trip
        .zones
        .iter()
        .find(|z| z.reason == "construction merge" && z.end_mi == zone.start_mi)
        .cloned()
        .expect("the taper");
    assert!(approx_abs(
        taper.start_mi,
        zone.start_mi - CONSTRUCTION_TAPER_MI,
        1e-6
    ));
    assert_eq!(taper.limit_mph, CONSTRUCTION_TAPER_LIMIT_MPH);
    assert_eq!(
        trip.speed_limit_at((taper.start_mi + taper.end_mi) / 2.0),
        (
            CONSTRUCTION_TAPER_LIMIT_MPH,
            Some("construction merge".to_string())
        )
    );
    assert_eq!(
        trip.speed_limit_at((zone.start_mi + zone.end_mi) / 2.0),
        (zone.limit_mph, Some("construction".to_string()))
    );
}

#[test]
fn test_construction_warning_lead_allows_normal_braking() {
    let mut trip = make_trip(2, 1.0);
    let zone = trip
        .zones
        .iter()
        .find(|z| z.reason == "construction")
        .cloned()
        .expect("a construction zone");
    trip.truck.velocity_mps = 70.0 / 2.23694;
    trip.time_scale = 20.0;
    trip.position_mi = zone.start_mi - trip.zone_warning_lookahead_mi();

    let events = trip.update(0.0);
    // NOT "Brake now!" -- that is the emergency hazard opening, and this
    // warning fires miles out. It leads with the distance like every other
    // zone warning (Shane, 2026-08-24).
    let first = &gps_messages(&events)[0];
    assert!(!first.starts_with("Brake now"));
    assert!(first.starts_with("In "));

    let inspections = brake_until_speed(&mut trip, zone.limit_mph, false, 20.0);
    assert!(trip.position_mi < zone.start_mi);
    assert!(inspections.is_empty());
}

#[test]
fn test_construction_zone_does_not_fine_on_entry_tick() {
    use crate::sim::trip_models::TripEventKind;

    let mut trip = make_trip(2, 1.0);
    let zone = trip
        .zones
        .iter()
        .find(|z| z.reason == "construction")
        .cloned()
        .expect("a construction zone");
    trip.truck.velocity_mps = 31.3; // about 70 mph

    trip.position_mi = zone.start_mi - 0.2;
    let moved_mi = 0.35;
    trip.position_mi += moved_mi;
    trip.check_zones();
    trip.check_inspections(moved_mi);

    let kinds: Vec<_> = trip.events.iter().map(|e| e.kind).collect();
    assert!(kinds.contains(&TripEventKind::ZoneEnter));
    assert!(!kinds.contains(&TripEventKind::Inspection));
}

#[test]
fn test_construction_zone_speeding_fine_waits_for_grace_distance() {
    use crate::sim::trip_models::{
        TripEventKind, CONSTRUCTION_ENFORCEMENT_GRACE_MI, CONSTRUCTION_TAPER_LIMIT_MPH,
    };

    let mut trip = make_trip(2, 1.0);
    let zone = trip
        .zones
        .iter()
        .find(|z| z.reason == "construction")
        .cloned()
        .expect("a construction zone");
    trip.truck.velocity_mps = 31.3; // about 70 mph

    trip.position_mi = zone.start_mi - 2.0;
    let advance = trip.update(0.0);
    // A denser map can land an exit-pressure cue in the same window; the
    // construction warning itself is what this test pins down.
    let construction_cues: Vec<String> = gps_messages(&advance)
        .into_iter()
        .filter(|m| m.contains("construction ahead"))
        .collect();
    assert_eq!(
        construction_cues,
        vec![format!(
            "In 2 miles, construction ahead. {}Speed limit {:.0} from one mile out, then {:.0} through the work zone.",
            closure_part(&zone),
            CONSTRUCTION_TAPER_LIMIT_MPH,
            zone.limit_mph
        )]
    );

    trip.position_mi = zone.start_mi + CONSTRUCTION_ENFORCEMENT_GRACE_MI - 0.1;
    trip.events = Vec::new();
    trip.check_zones();
    trip.check_inspections(0.4);
    assert!(kind_messages(&trip.events, TripEventKind::Inspection).is_empty());

    trip.position_mi = zone.start_mi + CONSTRUCTION_ENFORCEMENT_GRACE_MI + 0.1;
    trip.events = Vec::new();
    trip.check_inspections(0.8);
    assert_eq!(
        kind_messages(&trip.events, TripEventKind::Inspection),
        vec!["Trooper in the construction zone clocks your speed."]
    );
}

#[test]
fn test_late_emergency_brake_can_save_construction_speeding() {
    use crate::sim::trip_models::CONSTRUCTION_ENFORCEMENT_GRACE_MI;

    let mut trip = make_trip(2, 1.0);
    let zone = trip
        .zones
        .iter()
        .find(|z| z.reason == "construction")
        .cloned()
        .expect("a construction zone");
    trip.truck.velocity_mps = 70.0 / 2.23694;
    trip.time_scale = 20.0;
    trip.position_mi = zone.start_mi + CONSTRUCTION_ENFORCEMENT_GRACE_MI - 0.7;
    trip.check_zones();
    trip.events = Vec::new();

    let inspections = brake_until_speed(&mut trip, zone.limit_mph + 9.0, true, 5.0);
    assert!(trip.position_mi < zone.start_mi + CONSTRUCTION_ENFORCEMENT_GRACE_MI);
    assert!(inspections.is_empty());
}

#[test]
fn test_grades_are_bounded() {
    let trip = make_trip_on("Denver", "Salt Lake City", seeded(2));
    let mut mile = 0;
    while (mile as f64) < trip.total_miles() {
        assert!(trip.grade_at(mile as f64).abs() <= 0.08);
        mile += 3;
    }
}

#[test]
fn test_route_derived_flat_grade_is_stable_across_trip_seeds() {
    let trip_a = make_trip(1, 1.0);
    let trip_b = make_trip(99, 1.0);
    let miles = [0.0, 20.0, 33.0, 72.0, 122.0, 183.0];
    let grades_a: Vec<f64> = miles.iter().map(|m| trip_a.grade_at(*m)).collect();
    let grades_b: Vec<f64> = miles.iter().map(|m| trip_b.grade_at(*m)).collect();
    assert_eq!(grades_a, grades_b);
    assert!(grades_a.iter().map(|g| g.abs()).fold(f64::MIN, f64::max) < 0.002);
    let terrains: HashSet<String> = miles.iter().map(|m| trip_a.terrain_at(Some(*m))).collect();
    assert_eq!(terrains, HashSet::from(["flat".to_string()]));
}

#[test]
fn test_traffic_varies_by_seed_but_route_grade_does_not() {
    let trip_a = make_trip(1, 1.0);
    let trip_b = make_trip(8, 1.0);
    let miles = [10.0, 80.0, 150.0];
    let grades =
        |t: &crate::sim::trip::Trip| miles.iter().map(|m| t.grade_at(*m)).collect::<Vec<_>>();
    assert_eq!(grades(&trip_a), grades(&trip_b));
    let traffic = |t: &crate::sim::trip::Trip| {
        t.npc_vehicles()
            .iter()
            .map(|v| (v.at_mi(), v.speed_mph, v.reason()))
            .collect::<Vec<_>>()
    };
    assert_ne!(traffic(&trip_a), traffic(&trip_b));
}

#[test]
fn test_npc_traffic_model_applies_to_enriched_and_legacy_routes() {
    for cities in [["Chicago", "Indianapolis"], ["Chicago", "St. Louis"]] {
        let route = route_of(&cities);
        let mut weather = system("great_lakes", 1);
        weather.current = WeatherKind::Clear;
        let trip = trip_on(route, weather, seeded(1));
        assert!(!trip.npc_vehicles().is_empty(), "{cities:?}");
    }
}

#[test]
fn test_npc_traffic_seeding_is_deterministic() {
    use crate::sim::trip::TripOptions;

    let route = route_of(&["Chicago", "Indianapolis"]);
    let clear = || {
        let mut w = system("great_lakes", 1);
        w.current = WeatherKind::Clear;
        w
    };
    let opts = || TripOptions {
        seed: Some(1),
        start_hour: 8.0,
        ..Default::default()
    };
    let trip_a = trip_on(route.clone(), clear(), opts());
    let trip_b = trip_on(route, clear(), opts());

    let signature = |trip: &crate::sim::trip::Trip| {
        trip.npc_vehicles()
            .iter()
            .map(|v| {
                (
                    (v.position_mi * 100.0).round() / 100.0,
                    (v.speed_mph * 10.0).round() / 10.0,
                    v.relative_lane,
                    v.behavior(),
                )
            })
            .collect::<Vec<_>>()
    };
    assert!(!signature(&trip_a).is_empty());
    assert_eq!(signature(&trip_a), signature(&trip_b));
}

#[test]
fn test_npc_traffic_moves_each_trip_tick() {
    use crate::sim::traffic_manager::TrafficVehicle;
    use crate::sim::trip_models::NPCVehicle;

    let mut trip = make_trip(2, 1.0);
    trip.traffic_manager.rolling_bubble = false;
    trip.traffic_manager.vehicles = vec![TrafficVehicle::from(NPCVehicle::new(
        "npc:test",
        5.0,
        60.0,
        60.0,
        0,
        "steady_truck",
    ))];
    trip.hazard_check_mi = 1e9;
    trip.inspection_check_mi = 1e9;

    // Against the clock the sim actually converts with.
    let expected = 5.0 + 60.0 * 1.0 * trip.effective_time_scale() / 3600.0;
    trip.update(1.0);

    let moved = trip
        .npc_vehicles()
        .iter()
        .find(|v| v.key == "npc:test")
        .expect("the vehicle stays in the bubble");
    assert!(approx_abs(moved.position_mi, expected, 1e-6));
    assert!(moved.position_mi > 5.0);
}

#[test]
fn test_npc_vehicles_property_tracks_traffic_manager() {
    use crate::sim::traffic_manager::TrafficVehicle;
    use crate::sim::trip_models::NPCVehicle;

    let mut trip = make_trip(2, 1.0);
    let vehicle = TrafficVehicle::from(NPCVehicle::new(
        "npc:compat",
        5.0,
        55.0,
        55.0,
        0,
        "steady_truck",
    ));
    trip.set_npc_vehicles(vec![vehicle.clone()]);
    assert_eq!(trip.traffic_manager.vehicles, vec![vehicle.clone()]);
    assert_eq!(trip.npc_vehicles(), &[vehicle]);
}

#[test]
fn test_bad_weather_slows_modeled_traffic() {
    let route = route_of(&["Chicago", "Indianapolis"]);
    let mut clear_weather = system("great_lakes", 1);
    clear_weather.current = WeatherKind::Clear;
    let mut rain_weather = system("great_lakes", 1);
    rain_weather.current = WeatherKind::HeavyRain;

    let clear = trip_on(route.clone(), clear_weather, seeded(1));
    let rain = trip_on(route, rain_weather, seeded(1));

    assert!(!clear.npc_vehicles().is_empty());
    assert!(!rain.npc_vehicles().is_empty());
    // Troopers cruise at a fixed patrol speed regardless of weather; the
    // weather comparison belongs to the first civilian vehicle.
    let clear_civilian = clear
        .npc_vehicles()
        .iter()
        .find(|v| v.vehicle_class != "state trooper")
        .unwrap();
    let rain_civilian = rain
        .npc_vehicles()
        .iter()
        .find(|v| v.vehicle_class != "state trooper")
        .unwrap();
    assert_eq!(rain_civilian.at_mi(), clear_civilian.at_mi());
    assert!(rain_civilian.speed_mph < clear_civilian.speed_mph);
}

#[test]
fn test_rush_hour_can_slow_modeled_traffic() {
    use crate::sim::trip::TripOptions;

    let route = route_of(&["Chicago", "Indianapolis"]);
    let at = |hour: f64| {
        trip_on(
            route.clone(),
            system("great_lakes", 1),
            TripOptions {
                seed: Some(1),
                start_hour: hour,
                ..Default::default()
            },
        )
    };
    let midday = at(12.0);
    let rush = at(8.0);
    assert!(rush.rush_hour_traffic_bias(&route.legs[0]) > 0.0);
    if !rush.npc_vehicles().is_empty() && !midday.npc_vehicles().is_empty() {
        let min = |t: &crate::sim::trip::Trip| {
            t.npc_vehicles()
                .iter()
                .map(|v| v.speed_mph)
                .fold(f64::MAX, f64::min)
        };
        assert!(min(&rush) <= min(&midday));
    }
}

#[test]
fn test_traffic_pressure_marks_exit_and_construction_context() {
    let trip = make_trip(2, 1.0);
    assert!(trip
        .traffic_pressures
        .iter()
        .any(|p| p.kind == "exit" && p.direction == "right"));
    if trip.zones.iter().any(|z| z.reason == "construction merge") {
        assert!(trip
            .traffic_pressures
            .iter()
            .any(|p| p.kind == "construction_merge" && p.direction == "left"));
    }
}

#[test]
fn test_merge_traffic_pressures_drop_the_speed_advisory() {
    // route_merge and construction_merge are MERGE situations: the truck
    // holds its lane and leaves a gap, it never has a target speed to be
    // ready for. Exit traffic keeps its speed.
    use crate::sim::trip_models::TrafficPressure;

    let trip = make_trip(2, 1.0);
    let pressure = |kind: &str, direction: &str| TrafficPressure {
        start_mi: 0.0,
        end_mi: 1.0,
        kind: kind.to_string(),
        direction: direction.to_string(),
        intensity: 0.8,
        target_speed_mph: 35.0,
        reason: "probe".to_string(),
    };
    let route_merge_msg = trip.traffic_pressure_message(&pressure("route_merge", "right"), 1.0);
    let construction_merge_msg =
        trip.traffic_pressure_message(&pressure("construction_merge", "left"), 1.0);
    let exit_msg = trip.traffic_pressure_message(&pressure("exit", "right"), 1.0);
    let distance = trip.ahead_text(1.0);

    assert_eq!(
        route_merge_msg.normal,
        format!("Merging traffic in {distance}. Keep right.")
    );
    assert_eq!(
        construction_merge_msg.normal,
        format!("Traffic squeezing at the construction taper in {distance}. Merge left early.")
    );
    assert!(!route_merge_msg.normal.contains("35"));
    assert!(!construction_merge_msg.normal.contains("35"));
    assert!(!route_merge_msg.normal.contains("be ready"));
    assert!(!construction_merge_msg.normal.contains("be ready"));
    // Exit traffic is not a merge situation.
    assert!(exit_msg.normal.contains("35"));
}

#[test]
fn test_traffic_pressure_gps_cue_deduplicates() {
    use crate::sim::trip_models::{
        traffic_pressure_key, TripEventKind, TRAFFIC_PRESSURE_LOOKAHEAD_MI,
    };

    let mut trip = make_trip(2, 1.0);
    // Only one pressure cue fires per update, so pick an exit pressure with
    // no neighbor inside the lookahead window.
    let pressures = trip.traffic_pressures.clone();
    let isolated = |p: &crate::sim::trip_models::TrafficPressure| {
        pressures.iter().all(|q| {
            traffic_pressure_key(q) == traffic_pressure_key(p)
                || (q.start_mi - p.start_mi).abs() > TRAFFIC_PRESSURE_LOOKAHEAD_MI + 1.0
        })
    };
    let pressure = pressures
        .iter()
        .find(|p| p.kind == "exit" && p.start_mi > 1.0 && isolated(p))
        .cloned()
        .expect("an isolated exit pressure");
    let key = traffic_pressure_key(&pressure);
    trip.position_mi = pressure.start_mi - 1.0;

    let first = trip.update(0.0);
    let second = trip.update(0.0);

    let is_ours = |e: &crate::sim::trip_models::TripEvent| {
        e.kind == TripEventKind::GpsCue
            && e.data
                .traffic_pressure
                .as_ref()
                .is_some_and(|p| traffic_pressure_key(p) == key)
    };
    let cues: Vec<_> = first.iter().filter(|e| is_ours(e)).collect();
    assert_eq!(cues.len(), 1);
    assert!(cues[0].text().contains("Exit traffic building"));
    assert!(cues[0].text().contains("Hold the right lane"));
    assert!(!second.iter().any(is_ours));
}

#[test]
fn test_npc_traffic_cue_and_status_are_reviewable() {
    use crate::sim::traffic_manager::TrafficVehicle;
    use crate::sim::trip_models::{NPCVehicle, TripEventKind};

    let mut trip = make_trip(2, 1.0);
    trip.truck.velocity_mps = 29.0;
    trip.position_mi = 10.0;
    trip.traffic_manager.rolling_bubble = false;
    trip.traffic_manager.vehicles = vec![TrafficVehicle::from(NPCVehicle::new(
        "npc:merge",
        10.8,
        42.0,
        42.0,
        0,
        "merging_vehicle",
    ))];

    let events = trip.update(0.0);
    let npc_cues: Vec<_> = events
        .iter()
        .filter(|e| {
            e.kind == TripEventKind::GpsCue
                && e.data
                    .npc_vehicle
                    .as_ref()
                    .is_some_and(|v| v.key == "npc:merge")
        })
        .collect();
    assert_eq!(npc_cues.len(), 1);
    assert!(npc_cues[0].text().contains("Merging vehicle"));
    assert!(npc_cues[0].text().contains("ahead."));
    let status = trip.npc_traffic_status();
    assert_eq!(
        status,
        "Traffic: Merging vehicle, 0.8 miles ahead, 42 miles per hour."
    );
}

#[test]
fn test_metric_toggle_updates_npc_traffic_cue_units() {
    use crate::sim::traffic_manager::TrafficVehicle;
    use crate::sim::trip_models::{NPCVehicle, TripEventKind};

    let mut trip = make_trip(2, 1.0);
    trip.truck.velocity_mps = 29.0;
    trip.position_mi = 10.0;
    trip.set_imperial(false);
    trip.traffic_manager.rolling_bubble = false;
    trip.traffic_manager.vehicles = vec![TrafficVehicle::from(NPCVehicle::new(
        "npc:metric-merge",
        10.8,
        42.0,
        42.0,
        0,
        "merging_vehicle",
    ))];

    let events = trip.update(0.0);
    let npc_cue = events
        .iter()
        .find(|e| {
            e.kind == TripEventKind::GpsCue
                && e.data
                    .npc_vehicle
                    .as_ref()
                    .is_some_and(|v| v.key == "npc:metric-merge")
        })
        .expect("the merge cue");
    assert!(
        npc_cue.text().contains("1.3 kilometers ahead"),
        "{}",
        npc_cue.text()
    );
    assert!(!npc_cue.text().contains("miles"));
}

#[test]
fn test_metric_toggle_updates_npc_traffic_cue_speed_units() {
    use crate::sim::traffic_manager::TrafficVehicle;
    use crate::sim::trip_models::{NPCVehicle, TripEventKind};

    let mut trip = make_trip(2, 1.0);
    trip.truck.velocity_mps = 29.0;
    trip.position_mi = 10.0;
    trip.set_imperial(false);
    trip.traffic_manager.rolling_bubble = false;
    // The jam the lead is on the brakes for. Without one on the road there is
    // no brake-lights cue to check the units of: the braking label ends when
    // its reason does, and a lead with no reason under it is steady traffic,
    // which is not announced at all. It goes on the TRIP, which is what hands
    // the manager its braking zones on every update; one poked straight onto
    // the manager is overwritten before the vehicles run.
    trip.zones.push(crate::sim::trip_models::Zone::new(
        11.0,
        12.5,
        42.0,
        "heavy traffic",
    ));
    // Held down to 42 from a 65 cruise: the same fixture fault as the one in
    // test_traffic_context_and_warning_are_grounded_in_lead_vehicle -- the
    // number twice over is a vehicle at its own target, which is not braking.
    trip.traffic_manager.vehicles = vec![TrafficVehicle::from(NPCVehicle::new(
        "npc:metric-brake",
        10.8,
        42.0,
        65.0,
        0,
        "braking_traffic",
    ))];

    let events = trip.update(0.0);
    let npc_cue = events
        .iter()
        .find(|e| {
            e.kind == TripEventKind::GpsCue
                && e.data
                    .npc_vehicle
                    .as_ref()
                    .is_some_and(|v| v.key == "npc:metric-brake")
        })
        .expect("the brake-lights cue");
    assert!(
        npc_cue.text().contains("68 kilometers per hour"),
        "{}",
        npc_cue.text()
    );
    assert!(!npc_cue.text().contains("miles"));
}

#[test]
fn test_npc_traffic_status_includes_speed_units() {
    use crate::sim::traffic_manager::TrafficVehicle;
    use crate::sim::trip_models::NPCVehicle;

    let mut trip = make_trip(2, 1.0);
    trip.position_mi = 10.0;
    trip.traffic_manager.rolling_bubble = false;
    trip.traffic_manager.vehicles = vec![TrafficVehicle::from(NPCVehicle::new(
        "npc:status",
        10.8,
        68.0,
        68.0,
        0,
        "steady_truck",
    ))];
    assert_eq!(
        trip.npc_traffic_status(),
        "Traffic: vehicle, 0.8 miles ahead, 68 miles per hour."
    );
}

#[test]
fn test_npc_traffic_status_names_a_slow_box_truck_with_comma_shape() {
    use crate::sim::traffic_manager::TrafficVehicle;

    let mut trip = make_trip(2, 1.0);
    trip.position_mi = 10.0;
    trip.traffic_manager.rolling_bubble = false;
    trip.traffic_manager.vehicles = vec![TrafficVehicle::new(
        "lead-box",
        12.2,
        60.0,
        60.0,
        0,
        "following",
        "box truck",
    )];
    assert_eq!(
        trip.npc_traffic_status(),
        "Traffic: Slow box truck, 2.2 miles ahead, 60 miles per hour."
    );
}

#[test]
fn test_time_scale_compresses_fuel_burn() {
    use crate::sim::trip::TripOptions;

    let mut trip = make_trip_on(
        "Chicago",
        "Indianapolis",
        TripOptions {
            time_scale: 40.0,
            ..Default::default()
        },
    );
    // A sharp bend pins the clock to real time (its own feature, its own
    // test); the subject here is fuel compression on an open road.
    trip.curves = Vec::new();
    trip.truck.velocity_mps = 26.0; // already at cruise: full pacing applies
    trip.truck.throttle = 0.9;
    for _ in 0..(60 * 30) {
        trip.truck.auto_shift();
        trip.truck.update(1.0 / 60.0);
        trip.update(1.0 / 60.0);
    }
    assert_eq!(trip.truck.fuel_burn_mult, 40.0);
    assert!(trip.truck.fuel_gal < trip.truck.specs.fuel_tank_gal - 0.5);
}

#[test]
fn test_clock_compression_ramps_with_road_speed() {
    // Physics runs in real time, so the clock eases off while maneuvering.
    use crate::sim::trip::TripOptions;
    use crate::sim::trip_models::{FULL_COMPRESSION_MPH, LOW_SPEED_TIME_SCALE};

    let mut trip = make_trip_on(
        "Chicago",
        "Indianapolis",
        TripOptions {
            time_scale: 20.0,
            ..Default::default()
        },
    );

    trip.truck.velocity_mps = 0.0; // parked: near real-time pacing
    assert!(approx_abs(
        trip.effective_time_scale(),
        LOW_SPEED_TIME_SCALE,
        1e-9
    ));
    let before = trip.game_minutes;
    trip.update(1.0);
    assert!(approx_abs(
        trip.game_minutes - before,
        LOW_SPEED_TIME_SCALE / 60.0,
        1e-9
    ));

    trip.truck.velocity_mps = 25.0 / 2.23694; // 25 mph: mid-ramp
    let mid = trip.effective_time_scale();
    assert!(LOW_SPEED_TIME_SCALE < mid && mid < 20.0);

    trip.truck.velocity_mps = (FULL_COMPRESSION_MPH + 10.0) / 2.23694; // cruise
    assert!(approx_abs(trip.effective_time_scale(), 20.0, 1e-9));
    let before = trip.game_minutes;
    trip.update(1.0);
    assert!(approx_abs(trip.game_minutes - before, 20.0 / 60.0, 1e-9));
}

#[test]
fn test_parking_brake_waiting_runs_at_double_pacing() {
    // Player-armed waiting runs the clock at double the configured pacing;
    // the auto-set brake at trip start does not.
    use crate::sim::trip::TripOptions;
    use crate::sim::trip_models::{LOW_SPEED_TIME_SCALE, PARKED_TIME_SCALE_MULT};

    let mut trip = make_trip_on(
        "Chicago",
        "Indianapolis",
        TripOptions {
            time_scale: 20.0,
            ..Default::default()
        },
    );

    trip.truck.velocity_mps = 0.0;
    trip.truck.parking_brake = true; // auto-set (trip start): not waiting
    assert!(approx_abs(
        trip.effective_time_scale(),
        LOW_SPEED_TIME_SCALE,
        1e-9
    ));

    trip.waiting = true; // the player's own brake press arms it
    assert!(approx_abs(
        trip.effective_time_scale(),
        20.0 * PARKED_TIME_SCALE_MULT,
        1e-9
    ));
    let before = trip.game_minutes;
    trip.update(1.0);
    assert!(approx_abs(
        trip.game_minutes - before,
        20.0 * PARKED_TIME_SCALE_MULT / 60.0,
        1e-9
    ));
    assert!(trip.waiting); // still parked: stays armed

    trip.truck.velocity_mps = 5.0 / 2.23694; // rolling with the brake dragging
    assert!(trip.effective_time_scale() < 20.0 * PARKED_TIME_SCALE_MULT / 2.0);

    trip.truck.velocity_mps = 0.0;
    trip.truck.parking_brake = false; // any release path disarms on the next frame
    trip.update(1.0);
    assert!(!trip.waiting);
    assert!(approx_abs(
        trip.effective_time_scale(),
        LOW_SPEED_TIME_SCALE,
        1e-9
    ));
}

#[test]
fn test_every_region_has_clear_day_hazards() {
    // Every region always has plausible clear, calm, daytime hazards: the
    // nationwide staples are never filtered out.
    use crate::sim::trip_models::eligible_hazards;

    let noon = 12.0;
    let mut regions: Vec<&str> = REGION_WEIGHTS.iter().map(|(r, _)| *r).collect();
    regions.push("atlantis");
    for region in regions {
        let pool: Vec<&str> = eligible_hazards(region, WeatherKind::Clear, "flat", noon)
            .into_iter()
            .map(|(text, _)| text)
            .collect();
        assert!(pool.contains(&"debris on the road"));
        // No weather- or terrain-specific hazard leaks into a clear flat day.
        let text = pool.join(" ");
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
    use crate::sim::trip_models::eligible_hazards;

    let texts = |region: &str, weather: WeatherKind, terrain: &str| -> Vec<&'static str> {
        eligible_hazards(region, weather, terrain, 12.0)
            .into_iter()
            .map(|(text, _)| text)
            .collect()
    };
    // Snow hazards only appear when it is snowing.
    let clear = texts("great_lakes", WeatherKind::Clear, "flat");
    let snowy = texts("great_lakes", WeatherKind::Snow, "flat");
    assert!(!clear
        .iter()
        .any(|t| t.contains("snow") || t.contains("ice")));
    assert!(snowy.iter().any(|t| t.contains("snow")));

    // Rockfall is a mountain-terrain hazard, not a flatland one.
    let flat = texts("rockies", WeatherKind::Clear, "flat");
    let mountain = texts("rockies", WeatherKind::Clear, "mountain");
    assert!(!flat.contains(&"rockfall debris on the road"));
    assert!(mountain.contains(&"rockfall debris on the road"));

    // The dropped, implausible hazards are gone for good.
    let mut everything: Vec<&str> = Vec::new();
    for (region, _) in REGION_WEIGHTS.iter() {
        for weather in WeatherKind::ALL {
            for terrain in ["flat", "hills", "mountain"] {
                everything.extend(
                    eligible_hazards(region, weather, terrain, 3.0)
                        .into_iter()
                        .map(|(t, _)| t),
                );
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
    use crate::sim::trip_models::eligible_hazards;
    use std::collections::HashMap;

    let pool = |hour: f64| -> HashMap<&'static str, f64> {
        eligible_hazards("great_lakes", WeatherKind::Clear, "flat", hour)
            .into_iter()
            .collect()
    };
    let day = pool(12.0);
    let night = pool(23.0);
    let deer = "a deer crossing the road";
    assert!(night[deer] > day[deer]);
    // Non-animal staples keep the same weight regardless of the hour.
    assert_eq!(night["debris on the road"], day["debris on the road"]);
}

#[test]
fn test_upcoming_stop_only_looks_ahead() {
    let mut trip = make_trip(2, 1.0);
    let stop = trip.stops[0].clone();
    trip.position_mi = stop.at_mi - 3.0;
    assert_eq!(trip.upcoming_stop(5.0).map(|s| s.key()), Some(stop.key()));
    trip.position_mi = stop.at_mi - 10.0;
    assert!(trip.upcoming_stop(5.0).is_none());
    trip.position_mi = stop.at_mi + 0.1; // just past: the exit is gone
    assert_ne!(trip.upcoming_stop(5.0).map(|s| s.key()), Some(stop.key()));
}

#[test]
fn test_eta_tracks_current_speed() {
    // Regression: the C key's ETA was a constant 55 mph guess.
    let mut trip = make_trip(2, 1.0);
    let parked = trip.eta_game_hours(55.0);
    assert!(parked > 0.0);
    trip.truck.velocity_mps = 31.3; // ~70 mph
    let fast = trip.eta_game_hours(55.0);
    trip.truck.velocity_mps = 13.4; // ~30 mph
    let slow = trip.eta_game_hours(55.0);
    assert!(fast < parked && parked < slow);
    trip.truck.velocity_mps = 0.5;
    assert_eq!(trip.eta_game_hours(55.0), parked);
}

#[test]
fn test_progress_summary_mentions_highway() {
    let mut trip = make_trip(2, 1.0);
    let text = trip.progress_summary(true);
    assert!(text.contains("I-65"), "{text}");
    assert!(text.contains("Indianapolis, Indiana"), "{text}");
    assert!(text.contains("Current grade 0.0 percent, level"), "{text}");
    assert!(text.contains("Next stop"), "{text}");
    let metric = trip.progress_summary(false);
    assert!(metric.contains("kilometers"), "{metric}");
    trip.position_mi = 25.0;
    let state_text = trip.progress_summary(true);
    assert!(state_text.contains("Next state line"), "{state_text}");
    assert!(state_text.contains("Illinois into Indiana"), "{state_text}");
}

#[test]
fn test_gps_state_crossing_and_rest_stop_cues_deduplicate() {
    use crate::sim::trip_models::TripEventKind;

    let mut trip = make_trip(2, 1.0);
    trip.traffic_manager.rolling_bubble = false;
    trip.traffic_manager.vehicles = Vec::new();

    let line_mi = trip.route.legs[0].state_crossings()[0].at_mi;
    trip.position_mi = (line_mi - 10.0).max(0.0);
    let advance = trip.update(0.0);
    let repeat = trip.update(0.0);
    assert!(gps_events(&advance).is_empty());
    assert!(gps_events(&repeat).is_empty());

    trip.position_mi = (line_mi - 1.0).max(0.0);
    assert!(gps_events(&trip.update(0.0)).is_empty());

    // Keep the probe tied to the archived route's state-boundary crossing.
    trip.position_mi = line_mi;
    let crossing = trip.update(0.0);
    assert_eq!(
        kind_messages(&crossing, TripEventKind::StateCrossing),
        vec!["Crossing into Indiana near the I-65 state line south of Hammond."]
    );
    assert!(kind_messages(&trip.update(0.0), TripEventKind::StateCrossing).is_empty());

    let rest_stop_mi = trip
        .stops
        .iter()
        .find(|s| s.name == "Loves Travel Stop Lafayette")
        .expect("the curated Lafayette stop")
        .at_mi;
    trip.position_mi = rest_stop_mi - 1.0;
    let rest = trip.update(0.0);
    assert_eq!(gps_messages(&rest), vec!["Speed limit raised to 65."]);
}

#[test]
fn test_likely_parking_is_not_announced_as_truck_parking() {
    use crate::data::world_models::Stop;
    use crate::sim::trip_models::RoadStop;

    let stop = Stop {
        name: "Fuel".to_string(),
        at_mi: 1.0,
        parking: "likely".to_string(),
        ..Stop::default()
    };
    assert_eq!(stop.parking_label(), "");
    let mut road_stop = RoadStop::new("Fuel", 1.0, "travel_center");
    road_stop.parking = "likely".to_string();
    assert_eq!(road_stop.parking_text(), "");
}

#[test]
fn test_likely_parking_route_cue_just_announces_stop() {
    let trip = make_trip(2, 1.0);
    let leg = trip.route.legs[0].clone();
    let likely_stop = leg
        .stops
        .iter()
        .find(|stop| stop.parking == "likely")
        .expect("a likely-parking stop on the first leg");
    let cue = trip
        .navigation_cues
        .iter()
        .find(|cue| cue.key.ends_with(&format!(":{}", likely_stop.name)))
        .expect("the stop's cue");
    assert_eq!(cue.near_text, "");
}

#[test]
fn test_gps_traffic_cue_deduplicates() {
    use crate::sim::trip_models::NavigationCue;

    let mut trip = make_trip(2, 1.0);
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
fn test_route_context_describes_near_traffic_without_zero_distance() {
    use crate::sim::trip_models::NavigationCue;

    let mut trip = make_trip(2, 1.0);
    trip.navigation_cues =
        vec![
            NavigationCue::new("traffic:test", "traffic", 10.1, "traffic queue ahead", "")
                .with_speed(Some(45.0)),
        ];
    trip.position_mi = 10.0;
    let context = trip.next_navigation_context(true);
    assert_eq!(
        context,
        "Traffic just ahead: traffic queue ahead at 45 miles per hour."
    );
    assert!(!context.contains('0'));
}

#[test]
fn test_toll_cues_and_charges_deduplicate() {
    use crate::sim::trip_models::TripEventKind;

    let mut trip = make_trip_on("New York", "Philadelphia", seeded(2));
    trip.position_mi = 6.1;
    assert!(gps_events(&trip.update(0.0)).is_empty());

    trip.position_mi = 7.2;
    let advance = trip.update(0.0);
    let repeat = trip.update(0.0);
    assert_eq!(
        gps_messages(&advance),
        vec!["ticket system toll point ahead, New Jersey Turnpike ticket entry. Estimated toll 18 dollars, billed to carrier settlement."]
    );
    assert!(gps_events(&repeat).is_empty());

    trip.position_mi = 9.0;
    let charged = trip.update(0.0);
    let charged_again = trip.update(0.0);
    assert_eq!(
        kind_messages(&charged, TripEventKind::TollCharged),
        vec!["ticket system toll at New Jersey Turnpike ticket entry, estimated 18 dollars, billed to carrier settlement."]
    );
    assert_eq!(trip.toll_expense(), 18.0);
    assert!(kind_messages(&charged_again, TripEventKind::TollCharged).is_empty());
}

#[test]
fn test_non_toll_route_does_not_charge_tolls() {
    use crate::sim::trip_models::TripEventKind;

    let mut trip = make_trip(2, 1.0);
    trip.position_mi = trip.total_miles();
    let events = trip.update(0.0);
    assert_eq!(trip.toll_expense(), 0.0);
    assert!(kind_messages(&events, TripEventKind::TollCharged).is_empty());
}

#[test]
fn test_zero_amount_toll_entry_marker_does_not_record_expense() {
    use crate::sim::trip_models::TripEventKind;

    let mut trip = make_trip_on("Philadelphia", "Pittsburgh", seeded(2));
    trip.position_mi = 16.1;
    assert_eq!(
        gps_messages(&trip.update(0.0)),
        vec!["ticket system toll point ahead, Pennsylvania Turnpike eastern ticket entry. Entry recorded for carrier settlement."]
    );
    trip.position_mi = 18.0;
    let entry = trip.update(0.0);
    assert_eq!(
        gps_messages(&entry),
        vec!["ticket system entry recorded at Pennsylvania Turnpike eastern ticket entry. Toll billed at carrier settlement."]
    );
    assert_eq!(trip.toll_expense(), 0.0);
    assert!(kind_messages(&entry, TripEventKind::TollCharged).is_empty());
}

#[test]
fn test_traffic_context_and_warning_are_grounded_in_lead_vehicle() {
    use crate::sim::traffic_manager::TrafficVehicle;
    use crate::sim::trip_models::{NPCVehicle, TripEventKind};

    let mut trip = make_trip(2, 1.0);
    trip.truck.velocity_mps = 29.0;
    trip.position_mi = 9.98;
    trip.traffic_manager.rolling_bubble = false;
    // The ROAD carries the reason now, not the vehicle's own label: a jam it
    // is slowing for, covering the mile the lead sits on. It goes on the
    // TRIP, which is what hands the manager its braking zones on every
    // update; one poked straight onto the manager is overwritten before the
    // vehicles run.
    trip.zones.push(crate::sim::trip_models::Zone::new(
        10.5,
        12.0,
        45.0,
        "heavy traffic",
    ));
    // Held DOWN to 45 from a 65 cruise, which is what "braking_traffic" means.
    // This fixture used to give the same number twice -- a vehicle sitting at
    // its own target, so not braking by any reading, while still carrying the
    // braking label. It passed only because the label was taken at its word
    // for the life of the vehicle; now that the label ends when its reason
    // does, the fixture has to actually depict the thing it is named for.
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
    assert!(hazards[0].text().contains("Brake lights"));
    assert!(hazards[0].data.traffic.is_some());
}

#[test]
fn test_city_events_do_not_repeat_mapped_state_crossings() {
    // The mapped boundary owns the state line; the city line does not
    // repeat it.
    use crate::sim::trip_models::TripEventKind;

    let route = route_of(&["Chicago", "Cleveland", "Pittsburgh"]);
    let mut trip = trip_on(route.clone(), system("great_lakes", 1), seeded(2));
    trip.position_mi = route.legs[0].miles;

    let events = trip.update(0.0);
    assert_eq!(
        kind_messages(&events, TripEventKind::CityReached),
        vec!["Passing Cleveland, Ohio. Continuing on I-76 toward Pittsburgh."]
    );
    let state_events = kind_messages(&events, TripEventKind::StateCrossing);
    assert!(
        state_events
            .iter()
            .any(|m| m.starts_with("Crossing into Ohio near ")),
        "{state_events:?}"
    );
}

#[test]
fn test_city_events_keep_crossing_fallback_without_mapped_state_line() {
    use crate::data::world_models::Route;
    use crate::sim::trip_models::TripEventKind;
    use std::sync::Arc;

    let route = route_of(&["Chicago", "Cleveland", "Pittsburgh"]);
    let mut detail = route.legs[0].corridor().clone();
    detail.state_crossings.clear();
    let mut legs = route.legs.clone();
    legs[0] = Arc::new((*route.legs[0]).clone().with_detail(detail));
    let route = Route::new(route.cities.clone(), legs);
    let mut trip = trip_on(route.clone(), system("great_lakes", 1), seeded(2));
    trip.position_mi = route.legs[0].miles;

    let events = trip.update(0.0);
    assert_eq!(
        kind_messages(&events, TripEventKind::CityReached),
        vec!["Crossing into Ohio. Passing Cleveland, Ohio. Continuing on I-76 toward Pittsburgh."]
    );
}

#[test]
fn test_city_events_include_state_without_repeating_crossing() {
    use crate::sim::trip_models::TripEventKind;

    let route = route_of(&["New York", "Buffalo", "Cleveland"]);
    let mut trip = trip_on(route.clone(), system("northeast", 1), seeded(2));
    trip.position_mi = route.legs[0].miles;
    let events = trip.update(0.0);
    assert_eq!(
        kind_messages(&events, TripEventKind::CityReached),
        vec!["Passing Buffalo, New York. Continuing on I-90 toward Cleveland."]
    );
}

#[test]
fn test_same_city_highway_dispatch_is_not_a_facility_approach() {
    // Endpoints alone lied: a yard-to-cross-dock job inside one city rides
    // the interstate and still starts and ends at the same city key (owner,
    // 2026-07-24, Fernley).
    use crate::data::world_models::{CorridorDetail, Leg, Route, RoutePoint};
    use crate::sim::trip::Trip;
    use crate::sim::vehicle::TruckState;

    let trip_for = |route: Route| {
        let mut truck = TruckState::default();
        truck.transmission.automatic = true;
        truck.start_engine();
        Trip::new(route, truck, system("great_lakes", 1), seeded(2))
    };
    // A real dispatch leg always carries corridor geometry; the synthetic
    // facility approach never does -- that geometry is the discriminator.
    let highway_loop = Route::from_legs(
        vec!["fernley_nv_us".to_string(), "fernley_nv_us".to_string()],
        vec![Leg::new(
            "fernley_nv_us",
            "fernley_nv_us",
            17.0,
            "I-80",
            "flat",
            Vec::new(),
        )
        .with_detail(CorridorDetail {
            route_points: vec![
                RoutePoint {
                    at_mi: 0.0,
                    lat: 39.6,
                    lon: -119.3,
                },
                RoutePoint {
                    at_mi: 17.0,
                    lat: 39.5,
                    lon: -119.1,
                },
            ],
            ..Default::default()
        })],
    );
    let trip = trip_for(highway_loop);
    assert!(!trip.is_facility_approach_route());
    assert!(!trip
        .zones
        .iter()
        .any(|z| z.reason == "facility access road"));

    let street_chain = Route::from_legs(
        vec!["fernley_nv_us".to_string(), "fernley_nv_us".to_string()],
        vec![Leg::local("fernley_nv_us", 1.2, "Main Street", "", 25.0)],
    );
    assert!(trip_for(street_chain).is_facility_approach_route());
}

#[test]
fn test_trip_requests_first_cell_weather_at_construction() {
    // The first fetch must not wait for the first driving tick.
    use std::sync::{Arc, Mutex};

    struct RecordingProvider(Arc<Mutex<Vec<(String, f64, f64)>>>);
    impl WeatherProvider for RecordingProvider {
        fn request(&mut self, key: &str, lat: f64, lon: f64) {
            self.0.lock().unwrap().push((key.to_string(), lat, lon));
        }
        fn get(&mut self, _: &str) -> Option<WeatherKind> {
            None
        }
    }

    let requests = Arc::new(Mutex::new(Vec::new()));
    let route = first_route("chicago_il_us", "indianapolis_in_us");
    let weather = with_provider(
        "great_lakes",
        1,
        Box::new(RecordingProvider(Arc::clone(&requests))),
    );
    let _trip = trip_on(route, weather, seeded(2));

    let requests = requests.lock().unwrap();
    assert!(
        !requests.is_empty(),
        "trip construction should start the first weather fetch"
    );
    let (key, lat, lon) = &requests[0];
    assert!(key.starts_with("route:chicago_il_us:"), "{key}");
    assert!(*lat != 0.0 || *lon != 0.0);
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs App and states::city"]
fn test_city_menu_warms_the_weather_provider() {
    // TODO(port): port once the Rust App and states::city is available.
}

#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- needs App"]
fn test_live_data_providers_ignore_the_online_services_master_switch() {
    // TODO(port): port once the Rust App is available.
}

#[test]
fn test_weather_is_asked_again_at_a_state_line() {
    // Brandon, tester report 2026-08-18: rain carried from Florida into
    // Alabama with live weather on. The state is now part of the cell key.
    let mut trip = make_trip(2, 1.0);

    let mut crossing: Option<(f64, String, String)> = None;
    let mut previous = String::new();
    let mut mile = 0.0;
    while mile < trip.total_miles() {
        let state = trip.state_at(Some(mile));
        if !previous.is_empty() && !state.is_empty() && state != previous {
            crossing = Some((mile, previous.clone(), state.clone()));
            break;
        }
        if !state.is_empty() {
            previous = state;
        }
        mile += 0.25;
    }
    let (at_mi, before, after) = crossing.expect("this route has a baked state crossing");

    trip.position_mi = at_mi - 0.5;
    let (key_before, lat_before, _) = trip.weather_location().unwrap();
    trip.position_mi = at_mi + 0.25;
    let (key_after, lat_after, _) = trip.weather_location().unwrap();

    assert_ne!(
        key_before, key_after,
        "the weather key survived a state crossing"
    );
    assert!(key_before.contains(&before) && key_after.contains(&after));
    // And the coordinate moved with us rather than staying at the cell start.
    assert_ne!(lat_before, lat_after);

    // Inside one state the key must still be STABLE -- and so must the
    // point it is looked up at. The coordinate used to follow the truck for
    // the rest of a straddling cell, and every few hundred yards of it was
    // a new station key and a fresh fetch (29 in a minute, Brandon's
    // Louisiana line, 2026-09-01).
    trip.position_mi = at_mi + 1.0;
    let (steady_a, lat_a, lon_a) = trip.weather_location().unwrap();
    trip.position_mi = at_mi + 1.5;
    let (steady_b, lat_b, lon_b) = trip.weather_location().unwrap();
    assert_eq!(
        steady_a, steady_b,
        "the key now churns within a single state"
    );
    assert_eq!(
        (lat_a, lon_a),
        (lat_b, lon_b),
        "the lookup point now follows the truck within a single state"
    );
    assert_eq!(
        (lat_a, lon_a),
        (lat_after, {
            trip.position_mi = at_mi + 0.25;
            trip.weather_location().unwrap().2
        }),
        "the lookup point must be the same from the crossing onward"
    );
}
