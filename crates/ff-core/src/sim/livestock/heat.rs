//! Heat on a standing trailer.
//!
//! Read: the BQA Transportation manual's danger line is a Temperature
//! Humidity Index of 84, its example is 98 degrees Fahrenheit at 30 percent
//! humidity scoring 83, and each mile an hour of wind lowers the index by
//! about 1. So the air moving through the trailer at road speed keeps cattle
//! under the line, and the risk is the truck standing still.
//!
//! The game has temperature and no humidity. Assumed: 50 percent relative
//! humidity. Derived: with the standard index
//! `THI = 0.8 T + RH (T - 14.4) + 46.4` (T in Celsius, RH as a fraction),
//! which reproduces the manual's own example, the line of 84 falls at
//! 34.5 C, 94 F.

use serde::{Deserialize, Serialize};

/// Read: the manual's danger line.
pub const THI_DANGER: f64 = 84.0;
/// Assumed: the game carries no humidity.
pub const ASSUMED_RELATIVE_HUMIDITY: f64 = 0.5;
/// Assumed: below this speed the air through the trailer no longer keeps
/// the cattle cool. The manual's figure is about one index point per mile an
/// hour of wind; a crawl gives back almost all of that.
pub const AIRFLOW_MPH: f64 = 15.0;

/// The Temperature Humidity Index for a Celsius temperature and a relative
/// humidity between 0 and 1.
pub fn thi(temp_c: f64, relative_humidity: f64) -> f64 {
    0.8 * temp_c + relative_humidity * (temp_c - 14.4) + 46.4
}

pub(crate) fn thi_at_assumed_humidity(temp_f: f64) -> f64 {
    thi((temp_f - 32.0) * 5.0 / 9.0, ASSUMED_RELATIVE_HUMIDITY)
}

/// The Fahrenheit temperature where the index reaches the danger line at the
/// assumed humidity.
pub fn danger_temp_f() -> f64 {
    let rh = ASSUMED_RELATIVE_HUMIDITY;
    let temp_c = (THI_DANGER - 46.4 + rh * 14.4) / (0.8 + rh);
    temp_c * 9.0 / 5.0 + 32.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Heat {
    Cool,
    HotMoving,
    HotStanding,
}

/// The two inputs heat reads. It holds no timer of its own.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeatReading {
    pub temp_f: f64,
    pub speed_mph: f64,
}

impl Heat {
    pub fn next(self, reading: HeatReading) -> Heat {
        if reading.temp_f < danger_temp_f() {
            Heat::Cool
        } else if reading.speed_mph < AIRFLOW_MPH {
            Heat::HotStanding
        } else {
            Heat::HotMoving
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Heat::Cool => "cool",
            Heat::HotMoving => "hot_moving",
            Heat::HotStanding => "hot_standing",
        }
    }

    pub fn from_key(key: &str) -> Option<Heat> {
        [Heat::Cool, Heat::HotMoving, Heat::HotStanding]
            .into_iter()
            .find(|state| state.key() == key)
    }
}

#[cfg(test)]
mod tests {
    use super::Heat::*;
    use super::*;

    #[test]
    fn the_index_reproduces_the_manuals_example() {
        // 98 F at 30 percent scores 83 in the manual.
        let c = (98.0 - 32.0) * 5.0 / 9.0;
        assert!((thi(c, 0.30) - 83.0).abs() < 1.0, "{}", thi(c, 0.30));
    }

    #[test]
    fn the_danger_line_falls_at_94_degrees_at_half_humidity() {
        assert!((danger_temp_f() - 94.0).abs() < 0.2, "{}", danger_temp_f());
        assert!((thi_at_assumed_humidity(danger_temp_f()) - THI_DANGER).abs() < 1e-9);
    }

    #[test]
    fn heat_follows_its_two_inputs_only() {
        let hot = danger_temp_f() + 1.0;
        let cool = danger_temp_f() - 1.0;
        let table = [
            (Cool, cool, 0.0, Cool),
            (Cool, hot, 0.0, HotStanding),
            (Cool, hot, 60.0, HotMoving),
            (HotMoving, hot, 5.0, HotStanding),
            (HotStanding, hot, 40.0, HotMoving),
            (HotStanding, cool, 0.0, Cool),
            (HotMoving, cool, 60.0, Cool),
        ];
        for (from, temp_f, speed_mph, to) in table {
            assert_eq!(
                from.next(HeatReading { temp_f, speed_mph }),
                to,
                "{from:?} at {temp_f} F, {speed_mph} mph"
            );
        }
    }
}
