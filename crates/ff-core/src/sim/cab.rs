//! The driver window and the cab climate: pure ambience.
//!
//! A player asked to open the window and run the heater or the air
//! conditioning (2026-10-17). Nothing here touches the drive: no fuel cost,
//! no fatigue effect, no warning about rain coming in. A comfort mechanic
//! would add a new way to be wrong and new spoken warnings, and neither the
//! fuel cost of a class 8 air conditioner nor the alertness benefit of cold
//! air has a source this game could cite. What changes is the sound.
//!
//! Every drive starts with the window up and the fan off; none of it is
//! saved. The state lives here, headless, so a test can assert what a key
//! press did without a game.

/// Seconds the glass takes to travel all the way, which is also how long
/// the window motor runs. The outside sounds follow the glass, so the cab
/// opens up over the same two seconds the motor is heard.
pub const GLASS_TRAVEL_S: f64 = 2.0;

/// How much louder the outside sounds (the tires, rain, the weather wind)
/// get with the window all the way down: 0.7 over sealed, about 4.6 dB.
pub const OUTSIDE_LIFT_OPEN: f64 = 0.7;
/// The wind rush through the open window at highway speed and above.
pub const WIND_RUSH_FULL: f64 = 0.7;
/// The road speed (m/s, about 67 mph) where the wind rush is at full level.
/// It rises with the square of speed below that, the way air pressure does.
pub const WIND_RUSH_FULL_MPS: f64 = 30.0;

/// The blower's four positions, in the order one key steps through them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum FanSpeed {
    #[default]
    Off,
    Low,
    Medium,
    High,
}

impl FanSpeed {
    /// The next position, wrapping from high back to off.
    pub fn next(self) -> Self {
        match self {
            FanSpeed::Off => FanSpeed::Low,
            FanSpeed::Low => FanSpeed::Medium,
            FanSpeed::Medium => FanSpeed::High,
            FanSpeed::High => FanSpeed::Off,
        }
    }

    /// Loop gain and playback rate of the blower at this position.
    pub fn mix(self) -> Option<(f64, f64)> {
        match self {
            FanSpeed::Off => None,
            FanSpeed::Low => Some((0.25, 0.85)),
            FanSpeed::Medium => Some((0.4, 1.0)),
            FanSpeed::High => Some((0.6, 1.15)),
        }
    }
}

/// Whether the fan blows warm or cold air.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClimateMode {
    Heat,
    AirConditioning,
}

impl ClimateMode {
    pub fn other(self) -> Self {
        match self {
            ClimateMode::Heat => ClimateMode::AirConditioning,
            ClimateMode::AirConditioning => ClimateMode::Heat,
        }
    }
}

/// Below this outside temperature a drive starts on heat, at or above it on
/// air conditioning, so the first fan press blows the air a driver would
/// reach for. 18 °C is the usual indoor comfort floor.
pub const HEAT_BELOW_C: f64 = 18.0;

#[derive(Clone, Debug, PartialEq)]
pub struct CabState {
    window_open: bool,
    /// Where the glass is: 0 all the way up, 1 all the way down.
    glass: f64,
    pub fan: FanSpeed,
    pub mode: ClimateMode,
}

impl Default for CabState {
    fn default() -> Self {
        Self::for_outside(None)
    }
}

impl CabState {
    /// A fresh cab: window up, fan off, the mode picked by the weather.
    pub fn for_outside(temperature_c: Option<f64>) -> Self {
        let mode = match temperature_c {
            Some(t) if t >= HEAT_BELOW_C => ClimateMode::AirConditioning,
            _ => ClimateMode::Heat,
        };
        Self {
            window_open: false,
            glass: 0.0,
            fan: FanSpeed::Off,
            mode,
        }
    }

    pub fn window_open(&self) -> bool {
        self.window_open
    }

    /// Where the glass is, 0 up to 1 down.
    pub fn glass(&self) -> f64 {
        self.glass
    }

    /// Flip the window and return what to say.
    pub fn toggle_window(&mut self) -> &'static str {
        self.window_open = !self.window_open;
        if self.window_open {
            "Driver window down."
        } else {
            "Driver window up."
        }
    }

    /// Step the fan and return what to say. Each line is a whole literal
    /// so a catalog can translate it as a unit.
    pub fn step_fan(&mut self) -> &'static str {
        self.fan = self.fan.next();
        match self.fan {
            FanSpeed::Off => "Fan off.",
            FanSpeed::Low => "Fan low.",
            FanSpeed::Medium => "Fan medium.",
            FanSpeed::High => "Fan high.",
        }
    }

    /// Flip heat and air conditioning and return what to say.
    pub fn toggle_mode(&mut self) -> &'static str {
        self.mode = self.mode.other();
        match self.mode {
            ClimateMode::Heat => "Heat.",
            ClimateMode::AirConditioning => "Air conditioning.",
        }
    }

    /// Whether the compressor clutch should click in: air conditioning with
    /// the fan running.
    pub fn compressor_on(&self) -> bool {
        self.mode == ClimateMode::AirConditioning && self.fan != FanSpeed::Off
    }

    /// Move the glass toward where the switch sent it, over real seconds.
    pub fn step(&mut self, dt_real_s: f64) {
        if dt_real_s <= 0.0 {
            return;
        }
        let target = if self.window_open { 1.0 } else { 0.0 };
        let step = dt_real_s / GLASS_TRAVEL_S;
        self.glass = if self.glass < target {
            (self.glass + step).min(target)
        } else {
            (self.glass - step).max(target)
        };
    }

    /// Gain multiplier for the outside sounds.
    pub fn outside_lift(&self) -> f64 {
        1.0 + OUTSIDE_LIFT_OPEN * self.glass
    }

    /// Level of the wind rush through the window; 0 means stop the loop.
    pub fn wind_rush_gain(&self, speed_mps: f64) -> f64 {
        let s = (speed_mps.max(0.0) / WIND_RUSH_FULL_MPS).min(1.0);
        self.glass * s * s * WIND_RUSH_FULL
    }

    /// Playback rate of the wind rush: a little higher as the air speeds up.
    pub fn wind_rush_rate(speed_mps: f64) -> f64 {
        0.8 + 0.4 * (speed_mps.max(0.0) / WIND_RUSH_FULL_MPS).min(1.0)
    }

    /// The status screen's cab line, only when something differs from a
    /// fresh cab. The mode is named only while the fan blows it. Whole
    /// literals, for the same reason as the key lines.
    pub fn status_line(&self) -> Option<&'static str> {
        use ClimateMode::{AirConditioning as Ac, Heat};
        use FanSpeed::{High, Low, Medium, Off};
        Some(match (self.window_open, self.fan, self.mode) {
            (false, Off, _) => return None,
            (true, Off, _) => "Cab: driver window down",
            (false, Low, Heat) => "Cab: fan low, heat",
            (false, Medium, Heat) => "Cab: fan medium, heat",
            (false, High, Heat) => "Cab: fan high, heat",
            (false, Low, Ac) => "Cab: fan low, air conditioning",
            (false, Medium, Ac) => "Cab: fan medium, air conditioning",
            (false, High, Ac) => "Cab: fan high, air conditioning",
            (true, Low, Heat) => "Cab: driver window down, fan low, heat",
            (true, Medium, Heat) => "Cab: driver window down, fan medium, heat",
            (true, High, Heat) => "Cab: driver window down, fan high, heat",
            (true, Low, Ac) => "Cab: driver window down, fan low, air conditioning",
            (true, Medium, Ac) => "Cab: driver window down, fan medium, air conditioning",
            (true, High, Ac) => "Cab: driver window down, fan high, air conditioning",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fresh_cab_is_closed_and_quiet_and_says_nothing() {
        let cab = CabState::default();
        assert!(!cab.window_open());
        assert_eq!(cab.fan, FanSpeed::Off);
        assert_eq!(cab.status_line(), None);
        assert_eq!(cab.outside_lift(), 1.0);
        assert_eq!(cab.wind_rush_gain(30.0), 0.0);
    }

    #[test]
    fn the_mode_starts_on_what_the_weather_asks_for() {
        assert_eq!(CabState::for_outside(Some(-5.0)).mode, ClimateMode::Heat);
        assert_eq!(
            CabState::for_outside(Some(30.0)).mode,
            ClimateMode::AirConditioning
        );
        assert_eq!(CabState::for_outside(None).mode, ClimateMode::Heat);
    }

    #[test]
    fn the_glass_takes_its_travel_time_and_the_outside_follows_it() {
        let mut cab = CabState::default();
        assert_eq!(cab.toggle_window(), "Driver window down.");
        cab.step(GLASS_TRAVEL_S / 2.0);
        assert!((cab.glass() - 0.5).abs() < 1e-9);
        assert!((cab.outside_lift() - (1.0 + OUTSIDE_LIFT_OPEN / 2.0)).abs() < 1e-9);
        cab.step(GLASS_TRAVEL_S);
        assert_eq!(cab.glass(), 1.0);
        assert_eq!(cab.toggle_window(), "Driver window up.");
        cab.step(GLASS_TRAVEL_S * 3.0);
        assert_eq!(cab.glass(), 0.0);
    }

    #[test]
    fn the_wind_rush_grows_with_speed_and_tops_out() {
        let mut cab = CabState::default();
        cab.toggle_window();
        cab.step(GLASS_TRAVEL_S);
        assert_eq!(cab.wind_rush_gain(0.0), 0.0);
        let slow = cab.wind_rush_gain(10.0);
        let fast = cab.wind_rush_gain(25.0);
        assert!(slow > 0.0 && slow < fast);
        assert_eq!(cab.wind_rush_gain(40.0), WIND_RUSH_FULL);
    }

    #[test]
    fn the_fan_steps_round_and_the_mode_flips() {
        let mut cab = CabState::for_outside(Some(0.0));
        assert_eq!(cab.step_fan(), "Fan low.");
        assert_eq!(cab.step_fan(), "Fan medium.");
        assert_eq!(cab.step_fan(), "Fan high.");
        assert_eq!(cab.step_fan(), "Fan off.");
        assert!(!cab.compressor_on());
        assert_eq!(cab.toggle_mode(), "Air conditioning.");
        assert!(!cab.compressor_on(), "no compressor with the fan off");
        cab.step_fan();
        assert!(cab.compressor_on());
        assert_eq!(cab.toggle_mode(), "Heat.");
    }

    #[test]
    fn the_status_line_names_only_what_was_changed() {
        let mut cab = CabState::for_outside(Some(0.0));
        cab.toggle_mode();
        assert_eq!(cab.status_line(), None, "a mode with no air is not news");
        cab.step_fan();
        assert_eq!(cab.status_line(), Some("Cab: fan low, air conditioning"));
        cab.toggle_window();
        assert_eq!(
            cab.status_line(),
            Some("Cab: driver window down, fan low, air conditioning")
        );
    }
}
