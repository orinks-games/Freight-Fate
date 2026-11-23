//! The iPhone and iPad deep swipe, read and set in points of finger travel.
//!
//! The settings file keeps `touch_deep_swipe_factor` (how many flicks deep
//! the swipe goes), so a file someone edited by hand still loads to the same
//! threshold. The Touch gestures screen shows and steps the product, the
//! distance the native recogniser compares against.

use super::Settings;

/// The shortest deep swipe the Touch gestures row offers, in points.
pub const DEEP_SWIPE_MIN_POINTS: i64 = 60;
/// The longest deep swipe the row offers, in points.
pub const DEEP_SWIPE_MAX_POINTS: i64 = 180;
/// One step of the row, in points.
pub const DEEP_SWIPE_STEP_POINTS: i64 = 10;

impl Settings {
    /// The flick distance the native recogniser uses: `ff_touch_set_tuning`
    /// holds it to 10 to 120 points.
    fn touch_flick_points_applied(&self) -> f64 {
        self.touch_flick_points.clamp(10.0, 120.0)
    }

    /// Where the deep swipe starts, in whole points of travel, as the native
    /// recogniser applies it (it holds the factor to 1.5 to 6).
    pub fn touch_deep_swipe_points(&self) -> i64 {
        let points =
            self.touch_flick_points_applied() * self.touch_deep_swipe_factor.clamp(1.5, 6.0);
        points.round() as i64
    }

    /// Move the deep swipe `steps` row steps further (or nearer, when
    /// negative), within the row's range, and return where it now starts.
    /// The stored setting stays the factor.
    pub fn step_touch_deep_swipe(&mut self, steps: i64) -> i64 {
        let step = DEEP_SWIPE_STEP_POINTS;
        // An edited file can sit between steps; the first step lands on one.
        let current = (self.touch_deep_swipe_points() + step / 2) / step * step;
        let points = (current + steps * step).clamp(DEEP_SWIPE_MIN_POINTS, DEEP_SWIPE_MAX_POINTS);
        self.touch_deep_swipe_factor = points as f64 / self.touch_flick_points_applied();
        self.touch_deep_swipe_points()
    }

    /// How many degrees from level steer fully, as tilt steering applies
    /// it: the row's range, on a whole step.
    pub fn touch_tilt_full(&self) -> f64 {
        use crate::sim::tilt_steering::{FULL_MAX_DEGREES, FULL_MIN_DEGREES, FULL_STEP_DEGREES};
        let degrees = if self.touch_tilt_full_degrees.is_finite() {
            self.touch_tilt_full_degrees
        } else {
            crate::sim::tilt_steering::FULL_DEFAULT_DEGREES
        };
        ((degrees / FULL_STEP_DEGREES).round() * FULL_STEP_DEGREES)
            .clamp(FULL_MIN_DEGREES, FULL_MAX_DEGREES)
    }

    /// Move "Tilt for full steering" `steps` steps, within its range, and
    /// return where it now is.
    pub fn step_touch_tilt_full(&mut self, steps: i64) -> f64 {
        use crate::sim::tilt_steering::{FULL_MAX_DEGREES, FULL_MIN_DEGREES, FULL_STEP_DEGREES};
        self.touch_tilt_full_degrees = (self.touch_tilt_full() + steps as f64 * FULL_STEP_DEGREES)
            .clamp(FULL_MIN_DEGREES, FULL_MAX_DEGREES);
        self.touch_tilt_full()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tilt_for_full_steering_steps_in_fives_between_fifteen_and_forty_five() {
        let mut s = Settings::default();
        assert_eq!(s.touch_tilt_full(), 30.0);
        assert_eq!(s.step_touch_tilt_full(1), 35.0);
        assert_eq!(s.step_touch_tilt_full(5), 45.0);
        assert_eq!(s.step_touch_tilt_full(-10), 15.0);
        s.touch_tilt_full_degrees = 27.0;
        assert_eq!(s.touch_tilt_full(), 25.0);
        s.touch_tilt_full_degrees = f64::NAN;
        assert_eq!(s.touch_tilt_full(), 30.0);
        s.touch_tilt_full_degrees = 90.0;
        assert_eq!(s.touch_tilt_full(), 45.0);
    }

    #[test]
    fn the_default_deep_swipe_starts_at_ninety_points() {
        assert_eq!(Settings::default().touch_deep_swipe_points(), 90);
    }

    #[test]
    fn a_file_with_the_old_factor_loads_to_the_same_threshold() {
        let map = serde_json::json!({"touch_flick_points": 30.0, "touch_deep_swipe_factor": 4.0});
        let serde_json::Value::Object(map) = map else {
            unreachable!()
        };
        let s = Settings::from_dict(Some(&map));
        assert_eq!(s.touch_deep_swipe_factor, 4.0);
        assert_eq!(s.touch_deep_swipe_points(), 120);
    }

    #[test]
    fn the_row_steps_by_ten_and_holds_at_both_ends() {
        let mut s = Settings::default();
        assert_eq!(s.step_touch_deep_swipe(1), 100);
        assert_eq!(s.step_touch_deep_swipe(-2), 80);
        for _ in 0..10 {
            s.step_touch_deep_swipe(-1);
        }
        assert_eq!(s.touch_deep_swipe_points(), DEEP_SWIPE_MIN_POINTS);
        assert_eq!(s.touch_deep_swipe_factor, 2.0);
        for _ in 0..20 {
            s.step_touch_deep_swipe(1);
        }
        assert_eq!(s.touch_deep_swipe_points(), DEEP_SWIPE_MAX_POINTS);
        assert_eq!(s.touch_deep_swipe_factor, 6.0);
    }

    #[test]
    fn an_edited_value_between_steps_lands_on_one() {
        let mut s = Settings {
            touch_deep_swipe_factor: 2.9,
            ..Settings::default()
        };
        assert_eq!(s.touch_deep_swipe_points(), 87);
        assert_eq!(s.step_touch_deep_swipe(1), 100);
    }
}
