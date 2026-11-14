//! The county each stretch of a leg runs through.
//!
//! `corridor.counties` is a list of runs in the leg's native frame: each one
//! names the county from its `at_mi` to the next run's. `cargo tools
//! census-counties` writes them from the Census Bureau's county boundaries
//! and the archived route geometry; a leg without the field has no county
//! to speak, and the location key says so.

use serde_json::Value;

use crate::data::world_models::DataError;
use crate::data::world_parsing::{get_str, req_float};
use crate::pyfmt::py_str_float;

/// One county along a leg, from `at_mi` (native frame) to the next run.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CountyRun {
    pub at_mi: f64,
    /// The spoken name, with its kind: "Lake County", "Orleans Parish",
    /// "Richmond City".
    pub name: String,
    pub source: String,
}

/// Parse `corridor.counties`, sorted by `at_mi`.
pub fn parse_county_runs(
    raw: &[Value],
    leg_miles: f64,
    from_city: &str,
    to_city: &str,
) -> Result<Vec<CountyRun>, DataError> {
    let mut runs = raw
        .iter()
        .map(|raw| {
            let raw = raw.as_object().ok_or_else(|| {
                DataError::value(format!(
                    "{from_city} to {to_city} county run must be an object"
                ))
            })?;
            let at_mi = req_float(raw, "at_mi")?;
            if !(0.0..=leg_miles).contains(&at_mi) {
                return Err(DataError::value(format!(
                    "{from_city} to {to_city} county run has at_mi {}, outside leg mileage 0-{}",
                    py_str_float(at_mi),
                    py_str_float(leg_miles)
                )));
            }
            let name = get_str(raw, "name");
            if name.trim().is_empty() {
                return Err(DataError::value(format!(
                    "{from_city} to {to_city} county run at {} has no name",
                    py_str_float(at_mi)
                )));
            }
            Ok(CountyRun {
                at_mi,
                name,
                source: get_str(raw, "source"),
            })
        })
        .collect::<Result<Vec<_>, _>>()?;
    runs.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
    Ok(runs)
}

/// The county at `offset_mi` (native frame): the last run that starts at or
/// before it, or the first run ahead of a leg whose first run starts late.
/// Empty when the leg has no county runs.
pub fn county_at(runs: &[CountyRun], offset_mi: f64) -> &str {
    let Some(first) = runs.first() else {
        return "";
    };
    runs.iter()
        .take_while(|run| run.at_mi <= offset_mi)
        .last()
        .unwrap_or(first)
        .name
        .as_str()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn runs() -> Vec<CountyRun> {
        parse_county_runs(
            &[
                json!({"at_mi": 12.4, "name": "Lake County", "source": "s"}),
                json!({"at_mi": 0.0, "name": "Cook County", "source": "s"}),
            ],
            40.0,
            "a",
            "b",
        )
        .unwrap()
    }

    #[test]
    fn the_run_under_the_truck_names_the_county() {
        let runs = runs();
        assert_eq!(county_at(&runs, 0.0), "Cook County");
        assert_eq!(county_at(&runs, 12.3), "Cook County");
        assert_eq!(county_at(&runs, 12.4), "Lake County");
        assert_eq!(county_at(&runs, 40.0), "Lake County");
        assert_eq!(county_at(&[], 5.0), "");
    }

    #[test]
    fn a_run_off_the_leg_or_without_a_name_is_refused() {
        assert!(parse_county_runs(&[json!({"at_mi": 41.0, "name": "X"})], 40.0, "a", "b").is_err());
        assert!(parse_county_runs(&[json!({"at_mi": 1.0, "name": " "})], 40.0, "a", "b").is_err());
    }
}
