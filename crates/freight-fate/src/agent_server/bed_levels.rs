//! The outside beds' levels in an agent's ears. Each bed is two copies whose
//! levels move with the driver's glass, so whether a copy is playing says
//! little: what it carries is the fact under test. The tires' song is heard
//! the same way, and its playback rate too, since its pitch follows speed.

use std::collections::HashMap;

use crate::audio::{CH_OUTSIDE, CH_ROAD_SONG};

/// One report per tenth of a level a copy moves.
const STEP: f64 = 0.1;

#[derive(Default)]
pub(super) struct BedLevels {
    steps: HashMap<u32, i64>,
    rate_steps: HashMap<u32, i64>,
}

/// The channels whose level is a fact under test.
pub(super) fn reports_level(channel: u32) -> bool {
    CH_OUTSIDE.contains(&channel) || channel == CH_ROAD_SONG
}

impl BedLevels {
    /// The line to hear when `channel`, playing `key`, is now at `volume`;
    /// `None` off the outside channels or within the step already reported.
    pub(super) fn heard(&mut self, channel: u32, key: &str, volume: f64) -> Option<String> {
        if !reports_level(channel) {
            return None;
        }
        let step = (volume.max(0.0) / STEP).round() as i64;
        if self.steps.insert(channel, step) == Some(step) {
            return None;
        }
        Some(format!("[bed] {key} level {:.1}", step as f64 * STEP))
    }

    /// The line to hear when the song on `channel` now plays at `rate`;
    /// `None` off the song's channel or within the step already reported.
    pub(super) fn rate_heard(&mut self, channel: u32, key: &str, rate: f64) -> Option<String> {
        if channel != CH_ROAD_SONG {
            return None;
        }
        let step = (rate.max(0.0) / STEP).round() as i64;
        if self.rate_steps.insert(channel, step) == Some(step) {
            return None;
        }
        Some(format!("[bed] {key} rate {:.1}", step as f64 * STEP))
    }

    pub(super) fn stopped(&mut self, channel: u32) {
        self.steps.remove(&channel);
        self.rate_steps.remove(&channel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{CH_NATURE_OPEN, CH_ROAD};

    #[test]
    fn a_level_is_heard_once_per_step_and_only_on_the_outside_beds() {
        let mut levels = BedLevels::default();
        let key = "ambient/insects";
        assert_eq!(
            levels.heard(CH_NATURE_OPEN, key, 0.0).as_deref(),
            Some("[bed] ambient/insects level 0.0")
        );
        assert_eq!(levels.heard(CH_NATURE_OPEN, key, 0.02), None);
        assert_eq!(
            levels.heard(CH_NATURE_OPEN, key, 0.81).as_deref(),
            Some("[bed] ambient/insects level 0.8")
        );
        assert_eq!(levels.heard(CH_ROAD, "vehicle/road", 0.5), None);
        levels.stopped(CH_NATURE_OPEN);
        assert!(levels.heard(CH_NATURE_OPEN, key, 0.8).is_some());
    }

    #[test]
    fn the_tire_song_reports_its_level_and_rate() {
        let mut levels = BedLevels::default();
        let key = "vehicle/tined_whine";
        assert_eq!(
            levels.heard(CH_ROAD_SONG, key, 0.36).as_deref(),
            Some("[bed] vehicle/tined_whine level 0.4")
        );
        assert_eq!(
            levels.rate_heard(CH_ROAD_SONG, key, 1.0).as_deref(),
            Some("[bed] vehicle/tined_whine rate 1.0")
        );
        assert_eq!(levels.rate_heard(CH_ROAD_SONG, key, 1.02), None);
        assert_eq!(
            levels.rate_heard(CH_ROAD_SONG, key, 0.54).as_deref(),
            Some("[bed] vehicle/tined_whine rate 0.5")
        );
        assert_eq!(levels.rate_heard(CH_ROAD, "vehicle/road", 0.5), None);
    }
}
