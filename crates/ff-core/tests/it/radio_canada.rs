//! Canada's cities hear Canadian stations: the map's Canadian cities with a
//! station of the Canadian imported tier (`radio_imported_canada.json`) in
//! range, held to a floor so a rebuild cannot quietly empty the country.

use ff_core::radio::default_radio_catalog;
use ff_core::sim::trip_route_helpers::haversine_distance_mi;

use crate::data_support::world;

/// The November 27 build reached 108 of the map's 150 Canadian cities.
const CITIES_WITH_A_STATION: usize = 105;

#[test]
fn most_canadian_cities_have_a_canadian_station_in_range() {
    let stations: Vec<_> = default_radio_catalog()
        .iter()
        .filter(|s| s.id.starts_with("rb-ised-"))
        .collect();
    let mut without: Vec<String> = Vec::new();
    let mut with = 0;
    for city in world().cities.values().filter(|c| c.country == "CA") {
        let heard = stations.iter().any(|s| match (s.lat, s.lon) {
            (Some(lat), Some(lon)) => {
                haversine_distance_mi(city.lat, city.lon, lat, lon) <= s.range_miles
            }
            _ => false,
        });
        if heard {
            with += 1;
        } else {
            without.push(city.spoken_qualified());
        }
    }
    without.sort();
    assert!(
        with >= CITIES_WITH_A_STATION,
        "{with} Canadian cities have a station; none at {without:?}"
    );
}
