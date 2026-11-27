use super::*;

fn inputs() -> SkyInputs {
    SkyInputs {
        temperature_c: Some(4.0),
        wind_kmh: 10.0,
        ..SkyInputs::default()
    }
}

fn no_freezing() -> impl FnMut() -> bool {
    || false
}

fn call(inputs: &SkyInputs) -> Option<SkyCall> {
    decide(inputs, &mut no_freezing())
}

fn said(phrase: &str, basis: SkyBasis) -> Option<SkyCall> {
    Some(SkyCall {
        phrase: phrase.to_string(),
        basis,
    })
}

#[test]
fn test_a_code_wins_over_the_city_page_text() {
    let sky = SkyInputs {
        codes: vec![65],
        page_text: Some("Mostly Cloudy".to_string()),
        ..inputs()
    };
    assert_eq!(call(&sky), said("Light Rain", SkyBasis::Code));
}

#[test]
fn test_the_most_telling_of_two_codes_decides() {
    let sky = SkyInputs {
        codes: vec![10, 65],
        ..inputs()
    };
    assert_eq!(call(&sky), said("Light Rain", SkyBasis::Code));
    let sky = SkyInputs {
        codes: vec![65, 17],
        ..inputs()
    };
    assert_eq!(call(&sky), said("Thunderstorm", SkyBasis::Code));
}

#[test]
fn test_a_past_hour_code_falls_to_the_page_text() {
    let sky = SkyInputs {
        codes: vec![25],
        page_text: Some("Cloudy".to_string()),
        ..inputs()
    };
    assert_eq!(call(&sky), said("Cloudy", SkyBasis::Text));
}

#[test]
fn test_the_gauge_adds_rain_to_a_dry_sky() {
    let sky = SkyInputs {
        page_text: Some("Cloudy".to_string()),
        precipitating: true,
        rate_mm_h: Some(1.2),
        ..inputs()
    };
    assert_eq!(call(&sky), said("Rain", SkyBasis::Gauge));
}

#[test]
fn test_a_dry_gauge_never_removes_an_observers_snow() {
    let sky = SkyInputs {
        page_text: Some("Light Snow".to_string()),
        precipitating: false,
        rate_mm_h: Some(0.0),
        temperature_c: Some(-3.0),
        ..inputs()
    };
    assert_eq!(call(&sky), said("Light Snow", SkyBasis::Text));
}

#[test]
fn test_a_wet_observation_keeps_its_own_words_over_the_gauge() {
    let sky = SkyInputs {
        codes: vec![80],
        precipitating: true,
        temperature_c: Some(-3.0),
        ..inputs()
    };
    let mut asked = false;
    let decided = decide(&sky, &mut || {
        asked = true;
        true
    });
    assert_eq!(decided, said("Snow", SkyBasis::Code));
    assert!(!asked, "nothing to confirm when an observer said snow");
}

#[test]
fn test_below_freezing_the_gauge_is_snow_unless_environment_canada_says_freezing() {
    let sky = SkyInputs {
        precipitating: true,
        temperature_c: Some(-2.0),
        cloud: Some(CloudCover::Cloudy),
        ..inputs()
    };
    assert_eq!(call(&sky), said("Snow", SkyBasis::Gauge));
    assert_eq!(
        decide(&sky, &mut || true),
        said("Freezing Rain", SkyBasis::Gauge)
    );
    let at_zero = SkyInputs {
        temperature_c: Some(0.0),
        ..sky
    };
    assert_eq!(call(&at_zero), said("Snow", SkyBasis::Gauge));
}

#[test]
fn test_freezing_is_asked_only_when_the_gauge_measures_at_or_below_zero() {
    let mut asked = 0;
    let warm = SkyInputs {
        precipitating: true,
        ..inputs()
    };
    let dry_cold = SkyInputs {
        temperature_c: Some(-5.0),
        cloud: Some(CloudCover::Clear),
        ..inputs()
    };
    for sky in [&warm, &dry_cold] {
        decide(sky, &mut || {
            asked += 1;
            true
        });
    }
    assert_eq!(asked, 0);
}

#[test]
fn test_heavy_rain_from_the_measured_rate() {
    let sky = |rate| SkyInputs {
        precipitating: true,
        rate_mm_h: Some(rate),
        ..inputs()
    };
    assert_eq!(
        call(&sky(HEAVY_RAIN_MM_H)),
        said("Heavy Rain", SkyBasis::Gauge)
    );
    assert_eq!(call(&sky(7.5)), said("Rain", SkyBasis::Gauge));
}

#[test]
fn test_cloud_cover_then_the_forecast_when_nothing_else_speaks() {
    let sky = SkyInputs {
        cloud: Some(CloudCover::Clear),
        forecast_text: Some("Rain".to_string()),
        ..inputs()
    };
    assert_eq!(call(&sky), said("Clear", SkyBasis::CloudAmount));
    let sky = SkyInputs {
        cloud: None,
        forecast_text: Some("  Rain ".to_string()),
        ..inputs()
    };
    assert_eq!(call(&sky), said("Rain", SkyBasis::Forecast));
    let sky = SkyInputs {
        page_text: Some(" ".to_string()),
        forecast_text: Some(String::new()),
        ..inputs()
    };
    assert_eq!(call(&sky), None);
}

#[test]
fn test_says_freezing_reads_rain_and_drizzle() {
    assert!(says_freezing("Light Freezing Drizzle"));
    assert!(says_freezing("Freezing rain warning"));
    assert!(!says_freezing("Freezing fog"));
    assert!(!says_freezing("Snow"));
}
