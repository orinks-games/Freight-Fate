use super::*;
use crate::sim::season::{region_climate, temperature_c, CAREER_START_DAY_OF_YEAR, DAYS_PER_YEAR};

/// Career hours at midnight on a day of the year.
fn hours_for_day(doy: f64) -> f64 {
    (doy - CAREER_START_DAY_OF_YEAR).rem_euclid(DAYS_PER_YEAR) * 24.0
}

/// The model's mean over a month of days, every hour of each.
fn month_mean(key: &str, first_day: f64, days: usize) -> f64 {
    let mut total = 0.0;
    for d in 0..days {
        let start = hours_for_day(first_day + d as f64);
        for h in 0..24 {
            total += temperature_c(key, start + f64::from(h));
        }
    }
    total / (days * 24) as f64
}

#[test]
fn each_row_matches_its_station_in_january_and_july() {
    for s in NORTHERN_STATIONS {
        let jan = month_mean(s.key, 0.0, 31);
        let jul = month_mean(s.key, 181.0, 31);
        assert!(
            (jan - s.january_mean_c()).abs() <= 1.0,
            "{}: January {jan:.1} against {} at {:.1}",
            s.key,
            s.station,
            s.january_mean_c()
        );
        assert!(
            (jul - s.july_mean_c()).abs() <= 1.0,
            "{}: July {jul:.1} against {} at {:.1}",
            s.key,
            s.station,
            s.july_mean_c()
        );
        assert_eq!(region_climate(s.key), s.climate(), "{} is not read", s.key);
    }
}

#[test]
fn fahrenheit_normals_convert() {
    let fairbanks = NORTHERN_STATIONS
        .iter()
        .find(|s| s.key == "alaska_interior")
        .unwrap();
    // -8.3 F and 62.9 F, as published.
    assert!((fairbanks.january_mean_c() - -22.39).abs() < 0.01);
    assert!((fairbanks.july_mean_c() - 17.17).abs() < 0.01);
    let (_, _, daily) = fairbanks.climate();
    // Daily ranges of 17.8 F and 19.6 F: half their mean, in Celsius.
    assert!((daily - (17.8 + 19.6) / 4.0 * 5.0 / 9.0).abs() < 1e-9);
}

#[test]
fn keys_are_unique_and_never_a_lower_48_region() {
    for (i, s) in NORTHERN_STATIONS.iter().enumerate() {
        assert!(
            crate::sim::season::REGION_CLIMATE
                .iter()
                .all(|(k, _)| *k != s.key),
            "{} shadows a lower-48 row",
            s.key
        );
        assert!(
            NORTHERN_STATIONS[i + 1..].iter().all(|o| o.key != s.key),
            "{} twice",
            s.key
        );
    }
}

/// One line per split: (state, lat, lon, place, expected key).
const SPLITS: &[(&str, f64, f64, &str, &str)] = &[
    ("AK", 64.84, -147.72, "Fairbanks", "alaska_interior"),
    ("AK", 63.34, -142.99, "Tok", "alaska_interior"),
    ("AK", 62.11, -145.48, "Glennallen", "alaska_interior"),
    ("AK", 63.88, -149.02, "Healy", "alaska_interior"),
    ("AK", 67.25, -150.18, "Coldfoot", "alaska_interior"),
    ("AK", 70.20, -148.46, "Deadhorse", "arctic_coast"),
    ("AK", 61.22, -149.90, "Anchorage", "alaska_south_coast"),
    ("AK", 61.58, -149.44, "Wasilla", "alaska_south_coast"),
    ("AK", 61.13, -146.35, "Valdez", "alaska_south_coast"),
    ("AK", 59.65, -151.54, "Homer", "alaska_south_coast"),
    ("AK", 59.46, -135.31, "Skagway", "alaska_panhandle"),
    ("AK", 58.30, -134.42, "Juneau", "alaska_panhandle"),
    ("YT", 60.72, -135.05, "Whitehorse", "yukon_south"),
    ("YT", 60.17, -132.73, "Teslin", "yukon_south"),
    ("YT", 60.06, -128.71, "Watson Lake", "northern_forest_west"),
    (
        "YT",
        61.36,
        -139.00,
        "Burwash Landing",
        "northern_forest_west",
    ),
    ("YT", 62.10, -136.29, "Carmacks", "northern_forest_west"),
    ("YT", 64.06, -139.43, "Dawson", "subarctic"),
    ("NT", 62.45, -114.37, "Yellowknife", "subarctic"),
    ("NT", 60.82, -115.79, "Hay River", "northern_forest_west"),
    ("BC", 49.18, -122.73, "Surrey", "pacific_northwest"),
    ("BC", 49.38, -121.44, "Hope", "pacific_northwest"),
    ("BC", 54.31, -130.32, "Prince Rupert", "pacific_northwest"),
    ("BC", 50.67, -120.33, "Kamloops", "bc_south_interior"),
    ("BC", 49.89, -119.50, "Kelowna", "bc_south_interior"),
    ("BC", 53.87, -122.74, "Prince George", "bc_north_interior"),
    ("BC", 52.13, -122.14, "Williams Lake", "bc_north_interior"),
    ("BC", 49.51, -115.77, "Cranbrook", "bc_north_interior"),
    ("BC", 55.76, -120.23, "Dawson Creek", "aspen_parkland"),
    ("BC", 56.24, -120.81, "Fort St. John", "aspen_parkland"),
    ("BC", 58.80, -122.70, "Fort Nelson", "northern_forest_west"),
    ("AB", 51.05, -114.07, "Calgary", "southern_alberta"),
    ("AB", 52.21, -113.82, "Red Deer", "aspen_parkland"),
    ("AB", 52.88, -118.08, "Jasper", "bc_north_interior"),
    ("BC", 49.11, -116.52, "Creston", "bc_south_interior"),
    ("BC", 51.31, -116.98, "Golden", "bc_north_interior"),
    ("ON", 44.23, -76.49, "Kingston", "great_lakes"),
    ("ON", 44.59, -75.68, "Brockville", "st_lawrence"),
    ("QC", 47.84, -69.54, "Riviere-du-Loup", "st_lawrence"),
    ("QC", 45.40, -71.89, "Sherbrooke", "st_lawrence"),
    ("QC", 48.24, -79.02, "Rouyn-Noranda", "northern_forest_east"),
    ("AB", 49.69, -112.84, "Lethbridge", "southern_alberta"),
    ("AB", 53.55, -113.49, "Edmonton", "aspen_parkland"),
    ("AB", 55.17, -118.80, "Grande Prairie", "aspen_parkland"),
    (
        "AB",
        56.73,
        -111.38,
        "Fort McMurray",
        "northern_forest_west",
    ),
    ("AB", 58.52, -117.14, "High Level", "northern_forest_west"),
    ("SK", 50.45, -104.61, "Regina", "prairies"),
    ("SK", 53.20, -105.75, "Prince Albert", "prairies"),
    ("SK", 55.10, -105.28, "La Ronge", "northern_forest_west"),
    ("MB", 49.90, -97.14, "Winnipeg", "prairies"),
    ("MB", 53.82, -101.25, "The Pas", "northern_forest_west"),
    ("MB", 55.74, -97.86, "Thompson", "northern_forest_west"),
    ("ON", 49.77, -94.49, "Kenora", "prairies"),
    ("ON", 48.38, -89.25, "Thunder Bay", "upper_midwest"),
    ("ON", 46.49, -80.99, "Sudbury", "upper_midwest"),
    ("ON", 46.31, -79.46, "North Bay", "upper_midwest"),
    ("ON", 48.48, -81.33, "Timmins", "northern_forest_east"),
    ("ON", 49.41, -82.43, "Kapuskasing", "northern_forest_east"),
    ("ON", 43.65, -79.38, "Toronto", "great_lakes"),
    ("ON", 42.31, -83.04, "Windsor", "great_lakes"),
    ("ON", 45.42, -75.70, "Ottawa", "st_lawrence"),
    ("QC", 45.50, -73.57, "Montreal", "st_lawrence"),
    ("QC", 46.81, -71.21, "Quebec City", "st_lawrence"),
    ("QC", 48.45, -68.52, "Rimouski", "st_lawrence"),
    ("QC", 48.10, -77.78, "Val-d'Or", "northern_forest_east"),
    ("QC", 48.43, -71.07, "Saguenay", "northern_forest_east"),
    ("QC", 50.21, -66.38, "Sept-Iles", "northern_forest_east"),
    ("NL", 53.30, -60.42, "Goose Bay", "northern_forest_east"),
    ("NL", 47.56, -52.71, "St. John's", "atlantic_canada"),
    ("NS", 44.65, -63.58, "Halifax", "atlantic_canada"),
    ("NB", 46.09, -64.78, "Moncton", "atlantic_canada"),
    ("PE", 46.24, -63.13, "Charlottetown", "atlantic_canada"),
];

#[test]
fn every_split_lands_where_its_normals_say() {
    for (state, lat, lon, place, want) in SPLITS {
        assert_eq!(
            climate_region(state, *lat, *lon).unwrap(),
            *want,
            "{place}, {state}"
        );
    }
}

#[test]
fn every_northern_key_has_a_row() {
    for (state, lat, lon, place, _) in SPLITS {
        let key = climate_region(state, *lat, *lon).unwrap();
        assert!(
            crate::sim::season::REGION_CLIMATE
                .iter()
                .any(|(k, _)| *k == key)
                || northern_climate(key).is_some(),
            "{place}: {key} has no climate row"
        );
    }
}

/// Canadian stations that keep a borrowed lower-48 row: (place, state,
/// lat, lon, published January mean C, ECCC climate id). Windsor (6139525,
/// -3.6) sits 2.4 above `great_lakes`, as Detroit across the river does on
/// the same row; it stays with its neighbour.
const BORROWED: &[(&str, &str, f64, f64, f64, &str)] = &[
    ("Vancouver", "BC", 49.19, -123.18, 4.1, "1108395"),
    ("Toronto", "ON", 43.68, -79.63, -5.0, "6158731"),
    ("Thunder Bay", "ON", 48.37, -89.33, -13.4, "6048260"),
    ("Sudbury", "ON", 46.63, -80.80, -12.7, "6068150"),
    ("North Bay", "ON", 46.36, -79.42, -12.3, "6085680"),
    ("Sault Ste. Marie", "ON", 46.48, -84.51, -9.8, "6057591"),
    ("Kingston", "ON", 44.23, -76.49, -7.0, "6104142"),
];

#[test]
fn borrowed_rows_stay_within_two_degrees_in_january() {
    for (place, state, lat, lon, jan, id) in BORROWED {
        let key = climate_region(state, *lat, *lon).unwrap();
        let (mean, seasonal, _) = region_climate(key);
        assert!(
            (mean - seasonal - jan).abs() <= 2.0,
            "{place} ({id}) reads {jan} in January; {key} bottoms at {}",
            mean - seasonal
        );
    }
}

#[test]
fn the_lower_48_keeps_its_freight_region() {
    for (state, lat, lon) in [
        ("WA", 47.61, -122.33),
        ("OR", 45.52, -122.68),
        ("TX", 29.76, -95.37),
        ("MI", 46.54, -87.40),
        ("NY", 42.89, -78.88),
        ("MT", 46.87, -113.99),
    ] {
        assert_eq!(
            climate_region(state, lat, lon).unwrap(),
            classify_region(state, lat, lon).unwrap()
        );
    }
    assert!(climate_region("Atlantis", 0.0, 0.0).is_err());
}

#[test]
fn a_fairbanks_january_night_is_a_fairbanks_night() {
    // 4 AM, January 15: the coldest point of the model's year.
    let night = hours_for_day(15.0) + 4.0;
    let fairbanks = temperature_c("alaska_interior", night);
    assert!(fairbanks < -25.0, "{fairbanks}");
    // Seattle reads the row it always read.
    assert_eq!(region_climate("pacific_northwest"), (11.0, 8.0, 6.0));
    // July still reads as summer up north.
    let afternoon = hours_for_day(196.0) + 15.0;
    assert!(temperature_c("alaska_interior", afternoon) > 20.0);
    assert!(temperature_c("arctic_coast", afternoon) > 10.0);
}
