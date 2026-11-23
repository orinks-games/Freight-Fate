//! What a drive met on the road, kept for the delivery the road journal
//! posts to orinks.net: the kinds of weather, whether a chain law was in
//! force, and the work zones, scales and inspections the truck went
//! through. Bookkeeping only: nothing here is spoken, and nothing here
//! changes the drive.
//!
//! The drive notes each fact where it already knows it (the sky, the posted
//! chain law, the zone and scale mileposts it crosses, the career's
//! inspection count), and the tally rides the active-trip snapshot, so a
//! drive saved at a stop and resumed still reports the whole run.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// The most weather keys a delivery sends (the site's limit).
pub const MAX_WEATHER_KEYS: usize = 8;
/// The most of any one count a delivery sends (the site's limit).
pub const MAX_COUNT: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RoadTally {
    /// `WeatherKind::key`s the truck drove through, first met first.
    pub weather: Vec<String>,
    /// A chain law was posted where the truck was.
    pub chain_law: bool,
    /// One key per work zone the truck entered.
    pub work_zones: Vec<String>,
    /// One key per open scale the truck passed or checked in at.
    pub scales: Vec<String>,
    /// The career's clean-inspection count when the drive began; `None` on
    /// a snapshot from before this tally, which starts counting at resume.
    pub inspections_before: Option<i64>,
}

/// The truck went from `before_mi` to `now_mi` this frame and passed `at_mi`.
pub fn crossed(before_mi: f64, now_mi: f64, at_mi: f64) -> bool {
    before_mi < at_mi && at_mi <= now_mi
}

fn note(list: &mut Vec<String>, key: String, cap: usize) {
    if list.len() < cap && !list.contains(&key) {
        list.push(key);
    }
}

impl RoadTally {
    /// A fresh tally for a drive beginning with `inspections` clean
    /// inspections on the career.
    pub fn starting_at(inspections: i64) -> Self {
        RoadTally {
            inspections_before: Some(inspections),
            ..RoadTally::default()
        }
    }

    pub fn note_weather(&mut self, key: &str) {
        note(&mut self.weather, key.to_string(), MAX_WEATHER_KEYS);
    }

    pub fn note_chain_law(&mut self) {
        self.chain_law = true;
    }

    pub fn note_work_zone(&mut self, key: String) {
        note(&mut self.work_zones, key, MAX_COUNT);
    }

    pub fn note_scale(&mut self, key: String) {
        note(&mut self.scales, key, MAX_COUNT);
    }

    /// The inspections since the drive began, given the career's count now.
    pub fn inspections(&self, inspections_now: i64) -> usize {
        let before = self.inspections_before.unwrap_or(inspections_now);
        (inspections_now - before).clamp(0, MAX_COUNT as i64) as usize
    }

    /// The delivery's `conditions`: only the parts that are true for this
    /// trip, or `None` when none are.
    pub fn conditions(&self, inspections_now: i64) -> Option<Value> {
        let mut out = Map::new();
        if !self.weather.is_empty() {
            out.insert("weather".into(), Value::from(self.weather.clone()));
        }
        if self.chain_law {
            out.insert("chainLaw".into(), Value::Bool(true));
        }
        let counts = [
            ("workZones", self.work_zones.len()),
            ("scales", self.scales.len()),
            ("inspections", self.inspections(inspections_now)),
        ];
        for (name, count) in counts {
            if count > 0 {
                out.insert(name.into(), Value::from(count.min(MAX_COUNT)));
            }
        }
        (!out.is_empty()).then_some(Value::Object(out))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_uneventful_drive_sends_no_conditions() {
        assert_eq!(RoadTally::starting_at(3).conditions(3), None);
    }

    #[test]
    fn only_what_happened_is_sent() {
        let mut tally = RoadTally::starting_at(2);
        tally.note_weather("rain");
        tally.note_weather("rain");
        tally.note_weather("fog");
        tally.note_chain_law();
        tally.note_work_zone("a".into());
        tally.note_work_zone("a".into());
        tally.note_work_zone("b".into());
        assert_eq!(
            tally.conditions(3),
            Some(
                json!({"weather": ["rain", "fog"], "chainLaw": true, "workZones": 2, "inspections": 1})
            )
        );
    }

    #[test]
    fn the_site_limits_hold() {
        let mut tally = RoadTally::starting_at(0);
        for i in 0..20 {
            tally.note_weather(&format!("kind_{i}"));
        }
        for i in 0..600 {
            tally.note_scale(format!("scale {i}"));
        }
        let sent = tally.conditions(10_000).unwrap();
        assert_eq!(sent["weather"].as_array().unwrap().len(), MAX_WEATHER_KEYS);
        assert_eq!(sent["scales"], MAX_COUNT);
        assert_eq!(sent["inspections"], MAX_COUNT);
    }

    #[test]
    fn an_old_snapshot_counts_from_the_resume_and_never_below_zero() {
        let old: RoadTally = serde_json::from_value(json!({})).unwrap();
        assert_eq!(old.inspections(40), 0);
        assert_eq!(RoadTally::starting_at(5).inspections(4), 0);
    }

    #[test]
    fn a_tally_survives_the_snapshot() {
        let mut tally = RoadTally::starting_at(7);
        tally.note_weather("snow");
        tally.note_chain_law();
        tally.note_scale("weigh".into());
        let back: RoadTally =
            serde_json::from_value(serde_json::to_value(&tally).unwrap()).unwrap();
        assert_eq!(back, tally);
    }

    #[test]
    fn crossing_counts_the_frame_that_reaches_the_milepost() {
        assert!(crossed(9.9, 10.0, 10.0));
        assert!(!crossed(10.0, 10.1, 10.0));
        assert!(!crossed(9.0, 9.5, 10.0));
    }
}
