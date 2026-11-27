use super::*;

fn details() -> ObservationDetails {
    ObservationDetails {
        station: Some("Barrie-Oro".to_string()),
        station_distance_mi: Some(6.2),
        wind_direction_deg: Some(45.0),
        gust_kmh: Some(34.0),
        visibility_mi: Some(1.24),
        snow_depth_cm: Some(14.0),
        forecast: Some(vec![WeatherKind::Rain, WeatherKind::Snow]),
        basis: Some("code, station 6151064".to_string()),
    }
}

#[test]
fn test_the_station_line_gives_name_and_distance() {
    let d = details();
    assert_eq!(
        station_line(&d, true).as_deref(),
        Some("Station: Barrie-Oro, 6 miles away.")
    );
    assert_eq!(
        station_line(&d, false).as_deref(),
        Some("Station: Barrie-Oro, 10 kilometers away.")
    );
}

#[test]
fn test_the_station_line_is_singular_nearby_or_left_out() {
    let mut d = details();
    d.station_distance_mi = Some(1.2);
    assert_eq!(
        station_line(&d, true).as_deref(),
        Some("Station: Barrie-Oro, 1 mile away.")
    );
    d.station_distance_mi = Some(0.4);
    assert_eq!(
        station_line(&d, true).as_deref(),
        Some("Station: Barrie-Oro, nearby.")
    );
    d.station_distance_mi = None;
    assert_eq!(
        station_line(&d, true).as_deref(),
        Some("Station: Barrie-Oro.")
    );
    d.station = None;
    assert_eq!(station_line(&d, true), None);
    d.station = Some("  ".to_string());
    assert_eq!(station_line(&d, true), None);
}

#[test]
fn test_the_wind_line_gives_direction_speed_and_gust() {
    let d = details();
    assert_eq!(
        wind_line(&d, 20.0, true),
        "Wind: northeast at 12 miles per hour, gusting to 21."
    );
    assert_eq!(
        wind_line(&d, 20.0, false),
        "Wind: northeast at 20 kilometers per hour, gusting to 34."
    );
}

#[test]
fn test_the_wind_line_is_calm_and_drops_what_is_missing() {
    let mut d = details();
    assert_eq!(wind_line(&d, 1.0, true), "Wind: calm.");
    // A gust no stronger than the sustained wind is no gust.
    assert_eq!(
        wind_line(&d, 34.0, false),
        "Wind: northeast at 34 kilometers per hour."
    );
    d.wind_direction_deg = None;
    assert_eq!(
        wind_line(&d, 20.0, false),
        "Wind: 20 kilometers per hour, gusting to 34."
    );
    d.gust_kmh = None;
    assert_eq!(wind_line(&d, 20.0, false), "Wind: 20 kilometers per hour.");
    assert_eq!(wind_line(&d, 1.7, true), "Wind: calm.");
    assert_eq!(wind_line(&d, 2.0, true), "Wind: 1 mile per hour.");
}

#[test]
fn test_compass_points_round_to_the_nearest_of_eight() {
    assert_eq!(compass_point(0.0), "north");
    assert_eq!(compass_point(22.4), "north");
    assert_eq!(compass_point(22.5), "northeast");
    assert_eq!(compass_point(180.0), "south");
    assert_eq!(compass_point(270.0), "west");
    assert_eq!(compass_point(337.5), "north");
    assert_eq!(compass_point(359.0), "north");
    assert_eq!(compass_point(-90.0), "west");
}

#[test]
fn test_the_visibility_line_uses_short_numbers() {
    let mut d = details();
    assert_eq!(
        visibility_line(&d, true).as_deref(),
        Some("Visibility: 1.2 miles.")
    );
    assert_eq!(
        visibility_line(&d, false).as_deref(),
        Some("Visibility: 2 kilometers.")
    );
    d.visibility_mi = Some(1.0);
    assert_eq!(
        visibility_line(&d, true).as_deref(),
        Some("Visibility: 1 mile.")
    );
    d.visibility_mi = Some(15.0);
    assert_eq!(
        visibility_line(&d, true).as_deref(),
        Some("Visibility: 15 miles.")
    );
    d.visibility_mi = None;
    assert_eq!(visibility_line(&d, true), None);
}

#[test]
fn test_the_snow_line_starts_at_one_centimeter() {
    let mut d = details();
    assert_eq!(
        snow_line(&d, false).as_deref(),
        Some("Snow on the ground: 14 centimeters.")
    );
    assert_eq!(
        snow_line(&d, true).as_deref(),
        Some("Snow on the ground: 6 inches.")
    );
    d.snow_depth_cm = Some(1.0);
    assert_eq!(
        snow_line(&d, true).as_deref(),
        Some("Snow on the ground: 1 inch.")
    );
    assert_eq!(
        snow_line(&d, false).as_deref(),
        Some("Snow on the ground: 1 centimeter.")
    );
    d.snow_depth_cm = Some(0.5);
    assert_eq!(snow_line(&d, false), None);
    d.snow_depth_cm = None;
    assert_eq!(snow_line(&d, false), None);
}

#[test]
fn test_the_forecast_line_names_the_changes() {
    let mut d = details();
    assert_eq!(
        forecast_line(&d, WeatherKind::Cloudy).as_deref(),
        Some("Forecast ahead: rain, then snow.")
    );
    d.forecast = Some(Vec::new());
    assert_eq!(
        forecast_line(&d, WeatherKind::Cloudy).as_deref(),
        Some("Forecast ahead: cloudy.")
    );
    d.forecast = None;
    assert_eq!(forecast_line(&d, WeatherKind::Cloudy), None);
}

#[test]
fn test_forecast_changes_keep_the_first_two_changes() {
    use WeatherKind::*;
    assert_eq!(
        forecast_changes(Cloudy, &[Cloudy, Rain, Rain, Snow, Clear]),
        vec![Rain, Snow]
    );
    assert_eq!(
        forecast_changes(Rain, &[Rain, Rain]),
        Vec::<WeatherKind>::new()
    );
    assert_eq!(forecast_changes(Rain, &[Snow, Rain]), vec![Snow, Rain]);
}

#[test]
fn test_station_lines_come_in_order_and_skip_the_unmeasured() {
    let d = details();
    assert_eq!(
        station_lines(&d, 20.0, true),
        vec![
            "Station: Barrie-Oro, 6 miles away.",
            "Wind: northeast at 12 miles per hour, gusting to 21.",
            "Visibility: 1.2 miles.",
            "Snow on the ground: 6 inches.",
        ]
    );
    assert_eq!(
        station_lines(&ObservationDetails::default(), 0.0, true),
        vec!["Wind: calm."]
    );
}

#[test]
fn test_describe_station_for_the_agent_server() {
    assert_eq!(
        describe_station(&details()),
        "Barrie-Oro, 6.2 mi, code, station 6151064"
    );
    assert_eq!(describe_station(&ObservationDetails::default()), "none");
}

#[test]
fn test_station_names_are_spoken_as_places() {
    for (raw, spoken) in [
        ("BARRIE-ORO", "Barrie-Oro"),
        ("TORONTO INTL A", "Toronto International Airport"),
        ("WASAGA PROVINCIAL PARK", "Wasaga Provincial Park"),
        ("OTTAWA CDA RCS", "Ottawa"),
        ("EGBERT CS", "Egbert"),
        ("L'ACADIE", "L'Acadie"),
        ("VAL-D'OR A", "Val-D'Or Airport"),
        ("ST ALBAN'S", "St Alban's"),
        ("TRENTON A", "Trenton Airport"),
        ("COLLINGWOOD (AUT)", "Collingwood"),
        ("KINGSTON CLIMATE", "Kingston"),
        (
            "MONTREAL/PIERRE ELLIOTT TRUDEAU INTL",
            "Montreal Pierre Elliott Trudeau International",
        ),
        ("MCBRIDE UPPER FRASER (", "Mcbride Upper Fraser"),
        ("Barrie-Oro", "Barrie-Oro"),
        ("SARNIA 'A'", "Sarnia Airport"),
        (
            "Billy Bishop Toronto City Airpor",
            "Billy Bishop Toronto City Airport",
        ),
        ("Short Airpor", "Short Airpor"),
    ] {
        assert_eq!(clean_station_name(raw), spoken, "{raw}");
    }
}
