//! The outside beds' levels in an agent's ears. Each bed is two copies whose
//! levels move with the driver's glass, so whether a copy is playing says
//! little: what it carries is the fact under test.

use std::collections::HashMap;

use crate::audio::CH_OUTSIDE;

/// One report per tenth of a level a copy moves.
const STEP: f64 = 0.1;

#[derive(Default)]
pub(super) struct BedLevels {
    steps: HashMap<u32, i64>,
}

impl BedLevels {
    /// The line to hear when `channel`, playing `key`, is now at `volume`;
    /// `None` off the outside channels or within the step already reported.
    pub(super) fn heard(&mut self, channel: u32, key: &str, volume: f64) -> Option<String> {
        if !CH_OUTSIDE.contains(&channel) {
            return None;
        }
        let step = (volume.max(0.0) / STEP).round() as i64;
        if self.steps.insert(channel, step) == Some(step) {
            return None;
        }
        Some(format!("[bed] {key} level {:.1}", step as f64 * STEP))
    }

    pub(super) fn stopped(&mut self, channel: u32) {
        self.steps.remove(&channel);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audio::{CH_NATURE_OPEN, CH_ROAD};

    #[test]
    fn a_level_is_heard_once_per_step_and_only_on_the_outside_beds() {
        let mut levels = BedLevels::default();
        let key = "ambient/insects_1";
        assert_eq!(
            levels.heard(CH_NATURE_OPEN, key, 0.0).as_deref(),
            Some("[bed] ambient/insects_1 level 0.0")
        );
        assert_eq!(levels.heard(CH_NATURE_OPEN, key, 0.02), None);
        assert_eq!(
            levels.heard(CH_NATURE_OPEN, key, 0.81).as_deref(),
            Some("[bed] ambient/insects_1 level 0.8")
        );
        assert_eq!(levels.heard(CH_ROAD, "vehicle/road", 0.5), None);
        levels.stopped(CH_NATURE_OPEN);
        assert!(levels.heard(CH_NATURE_OPEN, key, 0.8).is_some());
    }
}
