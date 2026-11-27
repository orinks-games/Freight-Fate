//! Environment Canada weather alerts, read keyless from the
//! `weather-alerts` collection of api.weather.gc.ca, into the same
//! [`WeatherAlert`] the National Weather Service alerts become.
//!
//! The feed is a set of polygons, one per forecast area an alert covers,
//! asked for by a small box around the point. One alert covers many areas,
//! so its features share the alert id (the part of the feature id before
//! `_fea`) and are kept once; an alert that has ended stays in the feed
//! with its status set, and is dropped here.

use std::collections::HashSet;

use serde_json::Value;

use crate::sim::real_weather::iso_timestamp;
use crate::sim::real_weather_alerts::{alert_effect, WeatherAlert};
use crate::sim::real_weather_canada::CANADA_API_ROOT;

/// Half the side of the box asked about around a point, in degrees.
pub const ALERT_BOX_DEG: f64 = 0.02;

/// The alerts feed URL for a point.
pub fn canada_alerts_url(lat: f64, lon: f64) -> String {
    format!(
        "{CANADA_API_ROOT}/collections/weather-alerts/items?f=json&bbox={:.4},{:.4},{:.4},{:.4}&limit=50",
        lon - ALERT_BOX_DEG,
        lat - ALERT_BOX_DEG,
        lon + ALERT_BOX_DEG,
        lat + ALERT_BOX_DEG
    )
}

/// "freezing rain warning" as the product name the cab says: "Freezing
/// Rain Warning".
pub fn title_case(name: &str) -> String {
    name.split_whitespace()
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().chain(chars).collect(),
                None => String::new(),
            }
        })
        .collect::<Vec<String>>()
        .join(" ")
}

/// The Weather Service severity word for an Environment Canada risk colour.
pub fn severity_for_colour(colour: Option<&str>) -> &'static str {
    match colour.map(str::to_ascii_lowercase).as_deref() {
        Some("red") => "Extreme",
        Some("orange") => "Severe",
        Some("yellow") => "Moderate",
        _ => "Minor",
    }
}

/// The active alerts in a `weather-alerts` document that concern a truck,
/// one per alert, in the feed's order. `now` is Unix seconds: an alert
/// whose event has ended by then is dropped, as is one marked ended.
pub fn parse_canada_alerts(doc: &Value, now: f64) -> Vec<WeatherAlert> {
    let Some(features) = doc.get("features").and_then(Value::as_array) else {
        return Vec::new();
    };
    let mut seen: HashSet<String> = HashSet::new();
    let mut alerts = Vec::new();
    for feature in features {
        let Some(props) = feature.get("properties") else {
            continue;
        };
        let text = |key: &str| {
            props
                .get(key)
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string()
        };
        if text("status_en").eq_ignore_ascii_case("ended") {
            continue;
        }
        if iso_timestamp(&text("event_end_datetime")).is_some_and(|end| end <= now) {
            continue;
        }
        let event = title_case(&text("alert_name_en"));
        if event.is_empty() || alert_effect(&event).is_none() {
            continue;
        }
        let feature_id = feature.get("id").and_then(Value::as_str).unwrap_or("");
        let id = feature_id
            .split_once("_fea")
            .map_or(feature_id, |(alert, _)| alert)
            .to_string();
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        let area = text("feature_name_en");
        alerts.push(WeatherAlert {
            id,
            severity: severity_for_colour(props.get("risk_colour_en").and_then(Value::as_str))
                .to_string(),
            headline: if area.is_empty() {
                event.clone()
            } else {
                format!("{event} for {area}")
            },
            event,
            area,
            description: text("alert_text_en"),
        });
    }
    alerts
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sim::real_weather_alerts::AlertEffect;
    use crate::sim::weather::WeatherKind;
    use serde_json::json;

    /// 2026-11-27 16:00 UTC, when the samples were read.
    const NOW: f64 = 1_795_795_200.0;

    fn feature(id: &str, name: &str, status: &str, end: &str, colour: Value) -> Value {
        json!({
            "id": id,
            "type": "Feature",
            "properties": {
                "alert_name_en": name,
                "alert_type": "warning",
                "status_en": status,
                "risk_colour_en": colour,
                "feature_name_en": "Le Bic - Rimouski area",
                "alert_text_en": "What: Freezing rain, gusts up to 90 km/h.",
                "event_end_datetime": end,
                "province": "QC"
            }
        })
    }

    #[test]
    fn test_the_saved_rimouski_answer_parses() {
        let doc: Value = serde_json::from_str(include_str!(
            "real_weather_canada/fixtures/alerts-rimouski.json"
        ))
        .expect("fixture");
        // An ended special weather statement and a freezing rain warning
        // still in force over Rimouski on 2026-11-27.
        let alerts = parse_canada_alerts(&doc, NOW);
        assert_eq!(alerts.len(), 1, "{alerts:?}");
        assert_eq!(alerts[0].id, "45617795514295485202611260504");
        assert_eq!(alerts[0].event, "Freezing Rain Warning");
        assert_eq!(alerts[0].severity, "Moderate");
        assert_eq!(alerts[0].area, "Le Bic - Rimouski area");
        assert!(alerts[0]
            .description
            .starts_with("What: A few hours of freezing rain"));
        assert_eq!(alerts[0].spoken(), "Freezing Rain Warning");
        // Past the warning's end it is gone too.
        let after = 1_795_852_800.0; // 2026-11-28 08:00 UTC
        assert!(parse_canada_alerts(&doc, after).is_empty());
    }

    #[test]
    fn test_ended_and_past_alerts_are_dropped_and_areas_kept_once() {
        let doc = json!({"features": [
            feature("a1_fea1-1", "freezing rain warning", "issued", "2026-11-28T09:00:00Z", json!("orange")),
            feature("a1_fea1-2", "freezing rain warning", "issued", "2026-11-28T09:00:00Z", json!("orange")),
            feature("a2_fea1-1", "wind warning", "ended", "2026-11-28T09:00:00Z", json!("yellow")),
            feature("a3_fea1-1", "fog advisory", "continued", "2026-11-27T12:00:00Z", Value::Null),
            feature("a4_fea1-1", "frost advisory", "issued", "2026-11-28T09:00:00Z", Value::Null),
            feature("a5_fea1-1", "wind warning", "continued", "", json!("red")),
        ]});
        let alerts = parse_canada_alerts(&doc, NOW);
        let ids: Vec<&str> = alerts.iter().map(|a| a.id.as_str()).collect();
        assert_eq!(ids, ["a1", "a5"]);
        assert_eq!(alerts[0].event, "Freezing Rain Warning");
        assert_eq!(alerts[0].severity, "Severe");
        assert_eq!(alerts[1].severity, "Extreme");
        assert_eq!(
            alerts[1].spoken(),
            "Wind Warning, gusts to 90 kilometers per hour"
        );
        assert_eq!(alerts[0].spoken(), "Freezing Rain Warning");
    }

    #[test]
    fn test_title_case_and_severity() {
        assert_eq!(
            title_case("les suetes wind warning"),
            "Les Suetes Wind Warning"
        );
        assert_eq!(title_case("  snow squall  watch "), "Snow Squall Watch");
        assert_eq!(severity_for_colour(Some("yellow")), "Moderate");
        assert_eq!(severity_for_colour(None), "Minor");
        assert!(canada_alerts_url(48.45, -68.52).starts_with(
            "https://api.weather.gc.ca/collections/weather-alerts/items?f=json&bbox=-68.5400,48.4300,-68.5000,48.4700"
        ));
    }

    #[test]
    fn test_every_environment_canada_alert_name_has_its_planning_meaning() {
        use AlertEffect::{Avoid, Brief, Slow};
        let table: &[(&str, Option<AlertEffect>)] = &[
            ("fog advisory", Some(Slow(WeatherKind::Fog))),
            ("rainfall warning", Some(Slow(WeatherKind::HeavyRain))),
            ("flash freeze warning", Some(Slow(WeatherKind::Ice))),
            ("freezing drizzle advisory", Some(Slow(WeatherKind::Ice))),
            ("arctic outflow warning", Some(Brief)),
            ("weather advisory", Some(Brief)),
            ("blizzard warning", Some(Avoid)),
            ("hurricane warning", Some(Avoid)),
            ("freezing rain warning", Some(Slow(WeatherKind::Ice))),
            ("snowfall warning", Some(Slow(WeatherKind::Snow))),
            ("snow squall warning", Some(Slow(WeatherKind::Snow))),
            ("snow squall watch", Some(Slow(WeatherKind::Snow))),
            ("blowing snow advisory", Some(Slow(WeatherKind::Snow))),
            ("winter storm warning", Some(Slow(WeatherKind::Snow))),
            ("winter storm watch", Some(Slow(WeatherKind::Snow))),
            ("wind warning", Some(Slow(WeatherKind::Wind))),
            ("les suetes wind warning", Some(Slow(WeatherKind::Wind))),
            ("wreckhouse wind warning", Some(Slow(WeatherKind::Wind))),
            ("severe thunderstorm warning", Some(Brief)),
            ("severe thunderstorm watch", Some(Brief)),
            ("tornado watch", Some(Brief)),
            ("tropical storm warning", Some(Brief)),
            ("extreme cold warning", Some(Brief)),
            ("special weather statement", Some(Brief)),
            ("tornado warning", Some(Avoid)),
            ("frost advisory", None),
            ("heat warning", None),
            ("storm surge warning", None),
            ("coastal flooding statement", None),
            ("air quality", None),
            ("tsunami warning", None),
            ("tsunami advisory", None),
        ];
        for (name, expected) in table {
            assert_eq!(alert_effect(&title_case(name)), *expected, "{name}");
        }
        // The Weather Service names keep their meaning.
        assert_eq!(
            alert_effect("Winter Weather Advisory"),
            Some(Slow(WeatherKind::Snow))
        );
        assert_eq!(
            alert_effect("Dense Fog Advisory"),
            Some(Slow(WeatherKind::Fog))
        );
    }
}
