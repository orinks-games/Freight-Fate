//! Climate rows for Alaska and Canada, apart from the freight regions.
//!
//! The simulated temperature (`sim::season::temperature_c`) reads a climate
//! key. In the lower 48 the key is the freight region from
//! [`classify_region`], so nothing there moves. Alaska, the Yukon and
//! British Columbia share the `pacific_northwest` freight region and the
//! prairies borrow `heartland` or `rockies`, which left Fairbanks at -3 °C on
//! the coldest night of the year. [`climate_region`] gives those places a row
//! of their own while the freight region (hazards, fuel prices, market tags,
//! weather-kind weights) stays as it was.
//!
//! The rows are for simulated weather only. A live reading, where there is
//! one, decides the temperature (`WeatherSystem::temperature_c`).
//!
//! Every row is **derived** from one station's published 1991 to 2020
//! normals: NOAA NCEI monthly normals for Alaska (degrees Fahrenheit,
//! converted here), Environment and Climate Change Canada climate normals
//! for Canada (degrees Celsius). The station's January and July values are
//! stored as published, and [`StationNormals::climate`] applies the formula:
//!
//! - mean = (January mean + July mean) / 2
//! - seasonal half-swing = (July mean - January mean) / 2
//! - daily half-swing = half the mean daily range (max - min), averaged over
//!   January and July
//!
//! A province or state that spans two climates is split by latitude and
//! longitude in [`climate_region`]; each split has a test line with the
//! station that justifies it.

use super::regions::classify_region;
use super::world_models::DataError;

/// The unit a station's normals are published in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NormalsUnit {
    Fahrenheit,
    Celsius,
}

/// One month's published normals: daily mean, mean daily maximum, mean
/// daily minimum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MonthNormals {
    pub mean: f64,
    pub max: f64,
    pub min: f64,
}

/// One station's January and July normals and the climate key they derive.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StationNormals {
    pub key: &'static str,
    pub station: &'static str,
    /// Where the numbers were read.
    pub source: &'static str,
    pub unit: NormalsUnit,
    pub january: MonthNormals,
    pub july: MonthNormals,
}

impl StationNormals {
    fn celsius(&self, value: f64) -> f64 {
        match self.unit {
            NormalsUnit::Celsius => value,
            NormalsUnit::Fahrenheit => (value - 32.0) * 5.0 / 9.0,
        }
    }

    /// The station's January mean in Celsius.
    pub fn january_mean_c(&self) -> f64 {
        self.celsius(self.january.mean)
    }

    /// The station's July mean in Celsius.
    pub fn july_mean_c(&self) -> f64 {
        self.celsius(self.july.mean)
    }

    /// (annual mean, seasonal half-swing, daily half-swing) in Celsius, the
    /// shape of `sim::season::REGION_CLIMATE`.
    pub fn climate(&self) -> (f64, f64, f64) {
        let jan = self.january_mean_c();
        let jul = self.july_mean_c();
        // A range is a difference, so a Fahrenheit range converts by 5/9
        // alone.
        let range = |m: &MonthNormals| match self.unit {
            NormalsUnit::Celsius => m.max - m.min,
            NormalsUnit::Fahrenheit => (m.max - m.min) * 5.0 / 9.0,
        };
        let daily = (range(&self.january) + range(&self.july)) / 4.0;
        ((jan + jul) / 2.0, (jul - jan) / 2.0, daily)
    }
}

const NOAA: &str = "NOAA NCEI U.S. Climate Normals 1991-2020, monthly \
                    (ncei.noaa.gov/data/normals-monthly/1991-2020)";
const ECCC: &str = "Environment and Climate Change Canada, Canadian Climate \
                    Normals 1991-2020 (climate.weather.gc.ca)";

/// Thompson's normals: the Meteorological Service's API serves the
/// 1981-2010 period for this station, not 1991-2020.
const ECCC_1981: &str = "Environment and Climate Change Canada, Canadian Climate \
                         Normals 1981-2010 (api.weather.gc.ca, collection \
                         climate-normals, read 2026-11-22)";

const fn f(mean: f64, max: f64, min: f64) -> MonthNormals {
    MonthNormals { mean, max, min }
}

/// The northern climate rows, read 2026-11-22.
pub const NORTHERN_STATIONS: &[StationNormals] = &[
    // Fairbanks, Delta Junction, Tok, Glennallen, the Parks Highway north of
    // the Alaska Range. Northway (Tok) reads January -13.3 F, Big Delta -1.0 F.
    StationNormals {
        key: "alaska_interior",
        station: "Fairbanks Intl AP (USW00026411)",
        source: NOAA,
        unit: NormalsUnit::Fahrenheit,
        january: f(-8.3, 0.6, -17.2),
        july: f(62.9, 72.7, 53.1),
    },
    // Anchorage, the Mat-Su valleys, the Kenai, Valdez.
    StationNormals {
        key: "alaska_south_coast",
        station: "Anchorage Intl AP (USW00026451)",
        source: NOAA,
        unit: NormalsUnit::Fahrenheit,
        january: f(16.9, 22.7, 11.0),
        july: f(59.6, 66.2, 52.9),
    },
    // Southeast Alaska east of 141 W: Skagway, Haines, Juneau, Ketchikan.
    StationNormals {
        key: "alaska_panhandle",
        station: "Juneau Intl AP (USW00025309)",
        source: NOAA,
        unit: NormalsUnit::Fahrenheit,
        january: f(28.5, 33.1, 23.8),
        july: f(57.0, 64.0, 50.1),
    },
    // North of the Brooks Range: the Dalton Highway to Deadhorse. Deadhorse
    // has 21 years in the period (NOAA flag R). Nunavut uses it too.
    StationNormals {
        key: "arctic_coast",
        station: "Deadhorse AP (USW00027406)",
        source: NOAA,
        unit: NormalsUnit::Fahrenheit,
        january: f(-14.9, -8.0, -21.7),
        july: f(47.3, 55.0, 39.7),
    },
    // The Whitehorse basin and the southern lakes: Whitehorse, Teslin
    // (January -16.8). The rest of the Yukon is colder.
    StationNormals {
        key: "yukon_south",
        station: "Whitehorse A (2101300)",
        source: ECCC,
        unit: NormalsUnit::Celsius,
        january: f(-15.0, -11.1, -18.9),
        july: f(14.5, 20.5, 8.5),
    },
    // Yellowknife, Inuvik (January -24.9), Dawson (-25.7) and Mayo (-23.2).
    StationNormals {
        key: "subarctic",
        station: "Yellowknife A (2204100)",
        source: ECCC,
        unit: NormalsUnit::Celsius,
        january: f(-25.5, -21.6, -29.4),
        july: f(17.1, 21.5, 12.6),
    },
    // The boreal belt west of Hudson Bay: northern Alberta, northeast BC,
    // the Yukon outside the Whitehorse basin, the south of the Northwest
    // Territories, northern Saskatchewan and Manitoba. Fort Nelson reads
    // January -20.3, Watson Lake -21.7, Burwash -20.6, Hay River -21.4,
    // Fort McMurray -17.2, La Ronge -18.3, The Pas -18.8. Thompson has a row of its own.
    StationNormals {
        key: "northern_forest_west",
        station: "High Level (3073140)",
        source: ECCC,
        unit: NormalsUnit::Celsius,
        january: f(-19.8, -14.6, -25.0),
        july: f(16.8, 23.3, 10.3),
    },
    // Manitoba north of 55: Thompson, at the end of Highway 6, reads 4 C
    // colder in January than the western belt's High Level row.
    StationNormals {
        key: "manitoba_north",
        station: "Thompson A (5062922)",
        source: ECCC_1981,
        unit: NormalsUnit::Celsius,
        january: f(-23.9, -18.3, -29.3),
        july: f(16.2, 23.1, 9.1),
    },
    // The boreal belt east of Lake Superior: northern Ontario and Quebec,
    // Labrador. Kapuskasing reads January -17.4, Val-d'Or -16.3, Rouyn
    // -15.1, Bagotville -15.1, Goose Bay -17.0.
    StationNormals {
        key: "northern_forest_east",
        station: "Timmins Victor Power (6078282)",
        source: ECCC,
        unit: NormalsUnit::Celsius,
        january: f(-16.4, -10.4, -22.3),
        july: f(17.7, 24.4, 10.9),
    },
    // The Thompson, Okanagan and West Kootenay valleys: Kamloops, Kelowna
    // (January -2.6), Creston (-1.4).
    StationNormals {
        key: "bc_south_interior",
        station: "Kamloops (1163780)",
        source: ECCC,
        unit: NormalsUnit::Celsius,
        january: f(-2.7, 0.4, -5.7),
        july: f(21.9, 29.3, 14.5),
    },
    // The Cariboo and the north-central interior, plus the Kootenays:
    // Prince George, Williams Lake (January -6.9), Smithers (-7.2),
    // Cranbrook (-6.1), Golden (-7.5), and Jasper over the Alberta line
    // (-8.8). Terrace (-3.3) is the warm outlier.
    StationNormals {
        key: "bc_north_interior",
        station: "Prince George (1096439)",
        source: ECCC,
        unit: NormalsUnit::Celsius,
        january: f(-7.8, -3.9, -11.7),
        july: f(15.9, 22.6, 9.1),
    },
    // Central Alberta and the Peace country: Edmonton, Red Deer (January
    // -11.6), Lloydminster (-14.3), Grande Prairie
    // (January -13.9), Peace River (-15.0), Dawson Creek (-13.5), Fort St.
    // John (-12.7).
    StationNormals {
        key: "aspen_parkland",
        station: "Edmonton Intl A (3012205)",
        source: ECCC,
        unit: NormalsUnit::Celsius,
        january: f(-12.3, -6.5, -18.1),
        july: f(16.2, 23.0, 9.4),
    },
    // Southern Alberta, where chinooks keep winter milder: Calgary,
    // Lethbridge (January -6.0), Medicine Hat (-8.5).
    StationNormals {
        key: "southern_alberta",
        station: "Calgary Intl A (3031092)",
        source: ECCC,
        unit: NormalsUnit::Celsius,
        january: f(-7.6, -1.5, -13.5),
        july: f(16.9, 23.5, 10.2),
    },
    // Southern Saskatchewan and Manitoba, and Kenora: Regina (January
    // -14.7), Winnipeg (-16.3), Brandon (-16.7), Kenora (-15.8).
    StationNormals {
        key: "prairies",
        station: "Saskatoon Diefenbaker Intl A (4057120)",
        source: ECCC,
        unit: NormalsUnit::Celsius,
        january: f(-15.4, -10.0, -20.7),
        july: f(18.3, 25.0, 11.5),
    },
    // Ottawa (January -10.0), Montreal, Sherbrooke (-10.8), Quebec City
    // (-11.9) and the lower St. Lawrence to Riviere-du-Loup (-11.4). The borrowed `northeast` row bottoms at -4, five
    // degrees too warm.
    StationNormals {
        key: "st_lawrence",
        station: "Montreal Trudeau Intl A (7025250)",
        source: ECCC,
        unit: NormalsUnit::Celsius,
        january: f(-9.2, -5.0, -13.5),
        july: f(21.7, 26.7, 16.7),
    },
    // New Brunswick, Nova Scotia, Prince Edward Island and the island of
    // Newfoundland: Moncton (January -8.4), Fredericton (-9.2), Halifax
    // (-5.7), Gander (-6.6), St. John's (-4.2).
    StationNormals {
        key: "atlantic_canada",
        station: "Charlottetown A (8300300)",
        source: ECCC,
        unit: NormalsUnit::Celsius,
        january: f(-7.3, -3.0, -11.5),
        july: f(19.0, 23.6, 14.4),
    },
];

/// The derived (mean, seasonal, daily) row for a northern climate key.
pub fn northern_climate(key: &str) -> Option<(f64, f64, f64)> {
    NORTHERN_STATIONS
        .iter()
        .find(|s| s.key == key)
        .map(StationNormals::climate)
}

/// The climate key for a place: its own row in Alaska and Canada, the
/// freight region everywhere else.
///
/// Three Canadian areas keep a lower-48 row because the normals match its
/// coldest point within 2 °C: the BC coast (`pacific_northwest`, Vancouver
/// 4.1 against 3.0), southern Ontario (`great_lakes`, Toronto Pearson -5.0
/// against -6.0) and the Thunder Bay to North Bay belt (`upper_midwest`,
/// Sudbury -12.7 against -11.5). The tests hold each of them to it.
pub fn climate_region(state: &str, lat: f64, lon: f64) -> Result<&'static str, DataError> {
    Ok(match state {
        "AK" => {
            if lat >= 68.0 {
                "arctic_coast"
            } else if lon >= -141.0 {
                "alaska_panhandle"
            } else if lat >= 63.0 || (lat >= 61.8 && lon > -148.5) {
                // North of the Alaska Range, and the Copper River basin.
                "alaska_interior"
            } else {
                "alaska_south_coast"
            }
        }
        "YT" => {
            if lat >= 63.0 {
                "subarctic"
            } else if lat < 61.5 && lon > -137.0 && lon < -131.0 {
                "yukon_south"
            } else {
                "northern_forest_west"
            }
        }
        "NT" => {
            if lat < 61.5 {
                "northern_forest_west"
            } else {
                "subarctic"
            }
        }
        "NU" => "arctic_coast",
        "BC" => {
            if lat >= 57.5 {
                "northern_forest_west"
            } else if lat >= 55.5 && lon >= -122.5 {
                "aspen_parkland" // the Peace country
            } else if (lat < 50.5 && lon <= -121.3) || lon <= -129.0 {
                "pacific_northwest" // the coast
            } else if lat < 51.2 && lon < -116.0 {
                "bc_south_interior"
            } else {
                "bc_north_interior"
            }
        }
        "AB" => {
            if lat >= 56.0 {
                "northern_forest_west"
            } else if (52.0..54.0).contains(&lat) && lon <= -117.0 {
                "bc_north_interior" // Jasper and Hinton, in the mountains
            } else if lat >= 52.0 {
                "aspen_parkland"
            } else {
                "southern_alberta"
            }
        }
        "SK" => {
            if lat >= 54.5 {
                "northern_forest_west"
            } else {
                "prairies"
            }
        }
        "MB" => {
            if lat >= 55.0 {
                "manitoba_north"
            } else if lat >= 53.5 {
                "northern_forest_west"
            } else {
                "prairies"
            }
        }
        "ON" => {
            if lon <= -91.0 {
                "prairies" // Kenora and Dryden
            } else if lat >= 50.0 || (lat >= 48.0 && lon > -88.5) {
                "northern_forest_east"
            } else if lon >= -76.0 {
                "st_lawrence" // Ottawa, Brockville and Cornwall
            } else if lat >= 45.5 {
                "upper_midwest" // Thunder Bay, the Soo, Sudbury, North Bay
            } else {
                "great_lakes"
            }
        }
        "QC" => {
            if lat >= 49.5 || (lat >= 47.5 && lon < -70.0) {
                "northern_forest_east"
            } else {
                "st_lawrence"
            }
        }
        "NL" => {
            if lat >= 51.7 {
                "northern_forest_east" // Labrador
            } else {
                "atlantic_canada"
            }
        }
        "NB" | "NS" | "PE" => "atlantic_canada",
        _ => return classify_region(state, lat, lon),
    })
}

#[cfg(test)]
mod tests;
