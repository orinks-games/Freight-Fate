//! Which live weather service answers for a place.
//!
//! The game's own data picks the service, never the coordinates: a road
//! mile or a city in a US state (Alaska included) asks the National Weather
//! Service (`api.weather.gov`), one in a Canadian province or territory asks
//! Environment and Climate Change Canada (`api.weather.gc.ca`). Neither is
//! ever sent a point in the other's country to see whether it answers.
//! A place in no country either serves is `None`: simulated weather, and
//! nothing is asked.

/// The live weather and alert service for a place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WeatherService {
    /// The US National Weather Service, for the United States and Alaska.
    UnitedStates,
    /// Environment and Climate Change Canada's MSC GeoMet service.
    Canada,
}

impl WeatherService {
    /// The service for a two-letter country code. An empty code (a place
    /// the data does not name) stays on the Weather Service, as it always
    /// has; a country neither service covers has none.
    pub fn for_country(country: &str) -> Option<Self> {
        let country = country.trim();
        if country.is_empty() || country.eq_ignore_ascii_case("US") {
            Some(Self::UnitedStates)
        } else if country.eq_ignore_ascii_case("CA") {
            Some(Self::Canada)
        } else {
            None
        }
    }

    /// The short key the agent server's `status` reports.
    pub fn key(self) -> &'static str {
        match self {
            Self::UnitedStates => "nws",
            Self::Canada => "eccc",
        }
    }
}

/// [`WeatherService::key`] for an optional service: "none" when no
/// service answers there.
pub fn service_key(service: Option<WeatherService>) -> &'static str {
    service.map_or("none", WeatherService::key)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_the_country_picks_the_service() {
        let table = [
            ("US", Some(WeatherService::UnitedStates)),
            ("us", Some(WeatherService::UnitedStates)),
            ("", Some(WeatherService::UnitedStates)),
            ("CA", Some(WeatherService::Canada)),
            ("ca", Some(WeatherService::Canada)),
            ("MX", None),
        ];
        for (country, expected) in table {
            assert_eq!(
                WeatherService::for_country(country),
                expected,
                "{country:?}"
            );
        }
        assert_eq!(service_key(Some(WeatherService::Canada)), "eccc");
        assert_eq!(service_key(Some(WeatherService::UnitedStates)), "nws");
        assert_eq!(service_key(None), "none");
    }
}
