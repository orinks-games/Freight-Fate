use super::*;
use crate::sim::real_weather::iso_timestamp;
use crate::sim::real_weather_canada::map_condition_canada;
use crate::sim::weather::WeatherKind;

fn fixture(name: &str) -> Value {
    let text = match name {
        "stations" => include_str!("fixtures/stations-all.json"),
        "pearson" => include_str!("fixtures/obs-toronto-pearson-manned.json"),
        "oro-minutely" => include_str!("fixtures/obs-barrie-oro-minutely.json"),
        "oro-hourly" => include_str!("fixtures/obs-barrie-oro-hourly.json"),
        other => panic!("no fixture {other}"),
    };
    serde_json::from_str(text).expect("fixture parses")
}

fn station(id: &str, lat: f64, lon: f64) -> Station {
    Station {
        id: id.to_string(),
        name: id.to_string(),
        lat,
        lon,
    }
}

#[test]
fn test_the_station_list_reads_every_station_with_a_position() {
    let list = parse_station_list(&fixture("stations"));
    assert_eq!(list.len(), 938);
    let oro = list
        .iter()
        .find(|s| s.id == "6117700")
        .expect("Barrie-Oro is listed");
    assert_eq!(oro.name, "BARRIE-ORO");
    assert!((oro.lat - 44.4844).abs() < 1e-6 && (oro.lon + 79.5464).abs() < 1e-6);
}

#[test]
fn test_a_station_with_no_id_or_position_is_left_out() {
    let doc = serde_json::json!({"features": [
        {"geometry": {"coordinates": [-79.0, 44.0]}, "properties": {"name": "NO ID"}},
        {"geometry": null, "properties": {"msc_id": "1", "name": "NO PLACE"}},
        {"geometry": {"coordinates": [-79.0, 44.0]}, "properties": {"msc_id": " 2 "}},
    ]});
    assert_eq!(
        parse_station_list(&doc),
        vec![station("2", 44.0, -79.0)]
            .into_iter()
            .map(|s| Station {
                name: String::new(),
                ..s
            })
            .collect::<Vec<_>>()
    );
    assert!(parse_station_list(&serde_json::json!({})).is_empty());
}

#[test]
fn test_nearest_stations_walk_at_most_the_limit_within_reach() {
    let list = parse_station_list(&fixture("stations"));
    // Downtown Barrie.
    let near = nearest_stations(&list, 44.3894, -79.6903);
    assert_eq!(near.len(), STATION_WALK_LIMIT);
    assert!(near.windows(2).all(|w| w[0].1 <= w[1].1), "nearest first");
    assert!(near.iter().all(|(_, mi)| *mi <= SITE_REACH_MI));
}

#[test]
fn test_beyond_reach_the_single_nearest_answers() {
    let list = vec![
        station("far", 50.0, -120.0),
        station("farther", 49.0, -118.0),
    ];
    let near = nearest_stations(&list, 53.30, -120.17);
    assert_eq!(near.len(), 1);
    assert_eq!(near[0].0.id, "far");
    assert!(near[0].1 > SITE_REACH_MI);
    assert!(nearest_stations(&[], 53.3, -120.17).is_empty());
}

#[test]
fn test_mcbride_reads_a_station_far_away() {
    let list = parse_station_list(&fixture("stations"));
    let near = nearest_stations(&list, 53.3015, -120.1686);
    assert_eq!(near.len(), 1, "{near:?}");
    assert!(near[0].1 > SITE_REACH_MI);
}

#[test]
fn test_a_manned_airport_report_reads_codes_and_visibility() {
    let reading = parse_swob(&fixture("pearson")).expect("parses");
    assert_eq!(
        reading.station_name.as_deref(),
        Some("Toronto/Pearson International")
    );
    assert_eq!(
        reading.observed_at,
        iso_timestamp("2026-11-27T17:00:00.000Z")
    );
    assert_eq!(reading.temperature_c, Some(3.7));
    // The 10-minute wind is null; the 2-minute one answers.
    assert_eq!(reading.wind_kmh, Some(15.5));
    assert_eq!(reading.wind_direction_deg, Some(15.0));
    // A gust marked -1 is not used.
    assert_eq!(reading.gust_kmh, None);
    assert_eq!(reading.visibility_km, Some(2.01));
    assert_eq!(reading.present_weather, vec![65, 10]);
    // Layer code 35 is not a cloud amount.
    assert_eq!(reading.cloud, None);
    assert!(!reading.precipitating());
}

#[test]
fn test_a_minutely_report_reads_its_gauge_and_drops_a_negative_depth() {
    let reading = parse_swob(&fixture("oro-minutely")).expect("parses");
    assert_eq!(reading.wind_kmh, Some(11.5));
    assert_eq!(reading.wind_direction_deg, Some(47.0));
    assert_eq!(reading.precipitation_10min_mm, Some(0.0));
    assert_eq!(reading.precipitation_1hr_mm, Some(0.3));
    assert_eq!(reading.snow_depth_cm, None);
    // The last ten minutes measured nothing, whatever the hour did.
    assert!(!reading.precipitating());
    assert_eq!(reading.precipitation_rate_mm_h(), Some(0.0));
    assert!(reading.present_weather.is_empty());
}

#[test]
fn test_an_hourly_report_reads_the_last_hour() {
    let reading = parse_swob(&fixture("oro-hourly")).expect("parses");
    assert_eq!(reading.precipitation_10min_mm, None);
    assert_eq!(reading.precipitation_1hr_mm, Some(0.4));
    assert!(reading.precipitating());
    assert_eq!(reading.precipitation_rate_mm_h(), Some(0.4));
}

#[test]
fn test_missing_values_are_absent_null_msng_or_marked() {
    let doc = serde_json::json!({"features": [{"properties": {
        "stn_nam-value": "MSNG",
        "date_tm-value": "2026-11-27T17:00:00.000Z",
        "air_temp": "MSNG",
        "avg_wnd_spd_10m_pst10mts": null,
        "avg_wnd_spd_10m_pst1mt": "7.2",
        "vis": 4.0, "vis-qa": -1,
        "snw_dpth": 12.0, "snw_dpth-qa": "100",
        "prsnt_wx_1": 80, "prsnt_wx_2": "MSNG",
        "tot_cld_amt": 75,
    }}]});
    let reading = parse_swob(&doc).expect("parses");
    assert_eq!(reading.station_name, None);
    assert_eq!(reading.temperature_c, None);
    assert_eq!(reading.wind_kmh, Some(7.2));
    assert_eq!(reading.visibility_km, None);
    assert_eq!(reading.snow_depth_cm, Some(12.0));
    assert_eq!(reading.present_weather, vec![80]);
    assert_eq!(reading.cloud, Some(CloudCover::Cloudy));
    assert!(parse_swob(&serde_json::json!({"features": []})).is_err());
}

#[test]
fn test_cloud_cover_from_percent_else_the_most_covered_layer() {
    let cover = |props: Value| {
        parse_swob(&serde_json::json!({"features": [{"properties": props}]}))
            .expect("parses")
            .cloud
    };
    assert_eq!(
        cover(serde_json::json!({"tot_cld_amt": 49})),
        Some(CloudCover::Clear)
    );
    assert_eq!(
        cover(serde_json::json!({"tot_cld_amt": 50})),
        Some(CloudCover::Cloudy)
    );
    assert_eq!(
        cover(serde_json::json!({"cld_amt_code_1": 1, "cld_amt_code_2": 3})),
        Some(CloudCover::Cloudy)
    );
    assert_eq!(
        cover(serde_json::json!({"cld_amt_code_1": 2, "cld_amt_code_2": 36})),
        Some(CloudCover::Clear)
    );
    assert_eq!(cover(serde_json::json!({"cld_amt_code_1": 39})), None);
}

#[test]
fn test_the_code_table_is_the_guides() {
    let rows = present_weather::all();
    assert_eq!(rows.len(), 705);
    assert!(rows.iter().all(|row| !row.description.is_empty()));
    assert!(present_weather::lookup(844).is_none());
    assert!(present_weather::lookup(250).is_none());
    assert!(present_weather::phrase(60_000).is_none());
}

#[test]
fn test_codes_land_on_the_game_conditions() {
    let kind = |code: u16| {
        present_weather::phrase(code)
            .map(|phrase| map_condition_canada(phrase, 10.0, None, Some(2.0)))
    };
    // Environment Canada's table, not WMO's: 65 is light rain.
    assert_eq!(kind(65), Some(WeatherKind::Rain));
    assert_eq!(kind(364), Some(WeatherKind::Rain));
    assert_eq!(kind(67), Some(WeatherKind::HeavyRain));
    assert_eq!(kind(61), Some(WeatherKind::Ice));
    assert_eq!(kind(71), Some(WeatherKind::Ice));
    assert_eq!(kind(80), Some(WeatherKind::Snow));
    assert_eq!(kind(17), Some(WeatherKind::Thunderstorm));
}

#[test]
fn test_codes_about_the_past_hour_are_not_weather_now() {
    for code in (20..=29).chain(321..=326).chain(171..=184).chain(873..=875) {
        assert_eq!(present_weather::phrase(code), None, "code {code}");
    }
}
