//! From an accelerometer reading to the angle tilt steering reads.
//!
//! The phone's motion sensor is read through SDL2's sensor API
//! (`SDL_SENSOR_ACCEL`), so iPhone and Android share one path. What SDL
//! hands over, from the source the `sdl2-sys` 0.38 crate bundles (SDL
//! 2.26.4):
//!
//! - **Units and sign.** Metres per second squared, and at rest the vector
//!   points away from the ground (`include/SDL_sensor.h`: "a device at rest
//!   will have an value of SDL_STANDARD_GRAVITY away from the center of the
//!   earth"). On iPhone, `src/sensor/coremotion/SDL_coremotionsensor.m`
//!   negates CoreMotion's `accelerometerData` (in g, where the axis pointing
//!   down reads +1) and multiplies by 9.80665, so a phone lying face up
//!   reads about (0, 0, +9.8) and one held upright in portrait (0, +9.8, 0).
//!   On Android, `src/sensor/android/SDL_androidsensor.c` passes the
//!   `ASensorEvent` values through untouched, and Android's accelerometer
//!   already reads +9.8 on the axis pointing up. Gravity is minus the
//!   reading on both.
//! - **Frame.** The device's own portrait frame, +x to the right edge, +y to
//!   the top, +z out of the screen, whatever the interface does ("The axis
//!   data is not changed when the phone is rotated", `SDL_sensor.h`).
//!   [`to_screen`] turns it into the screen's frame with the display
//!   orientation from `SDL_GetDisplayOrientation`, which on iPhone follows
//!   the interface (`src/video/uikit/SDL_uikitmodes.m`, from the status bar
//!   orientation).
//! - **Raw or gravity.** Raw: CoreMotion's `accelerometerData` and Android's
//!   `TYPE_ACCELEROMETER` include the hand's own movement. [`GravityFilter`]
//!   low-passes it with a 0.1 second time constant to leave gravity.
//!
//! The tilt is then the angle of the screen's left to right axis from level,
//! `asin(gx / |g|)` with `gx` gravity along the screen's rightward axis:
//! right edge down is positive. It reads the same with the phone held
//! upright like a wheel or lying flatter in the hands, because only the
//! sideways share of gravity moves it.

/// The low-pass time constant that leaves gravity, in seconds.
pub const GRAVITY_TIME_CONSTANT_S: f64 = 0.1;

/// A reading this much weaker than gravity has no direction worth reading
/// (free fall, or a sensor reporting zeros), in metres per second squared.
const MIN_GRAVITY: f64 = 2.0;

/// Which way the interface is turned relative to the device's portrait
/// frame, as `SDL_GetDisplayOrientation` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenOrientation {
    /// Upright, home edge at the bottom.
    Portrait,
    /// Upside down.
    PortraitFlipped,
    /// `SDL_ORIENTATION_LANDSCAPE`: the device's right edge is at the top
    /// (turned a quarter anticlockwise).
    Landscape,
    /// `SDL_ORIENTATION_LANDSCAPE_FLIPPED`: the device's left edge is at the
    /// top (turned a quarter clockwise).
    LandscapeFlipped,
}

/// A vector in the device's portrait frame, as the screen sees it: the
/// screen's rightward and upward components.
pub fn to_screen(device: [f64; 3], orientation: ScreenOrientation) -> [f64; 2] {
    let [x, y, _] = device;
    match orientation {
        ScreenOrientation::Portrait => [x, y],
        ScreenOrientation::PortraitFlipped => [-x, -y],
        // Right edge up: the screen's right is the device's bottom, and the
        // screen's top is the device's right edge.
        ScreenOrientation::Landscape => [-y, x],
        // Left edge up: the screen's right is the device's top.
        ScreenOrientation::LandscapeFlipped => [y, -x],
    }
}

/// Gravity out of raw accelerometer readings, by a first-order low-pass.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct GravityFilter {
    gravity: Option<[f64; 3]>,
}

impl GravityFilter {
    /// Fold in one SDL reading taken `dt` seconds after the last; returns
    /// gravity in the device frame (minus the low-passed reading).
    pub fn feed(&mut self, reading: [f64; 3], dt: f64) -> [f64; 3] {
        let down = reading.map(|v| -v);
        let next = match self.gravity {
            None => down,
            Some(g) => {
                let alpha = (dt.max(0.0) / (GRAVITY_TIME_CONSTANT_S + dt.max(0.0))).clamp(0.0, 1.0);
                [
                    g[0] + alpha * (down[0] - g[0]),
                    g[1] + alpha * (down[1] - g[1]),
                    g[2] + alpha * (down[2] - g[2]),
                ]
            }
        };
        self.gravity = Some(next);
        next
    }

    /// Forget the history, for when the sensor stops and starts again.
    pub fn reset(&mut self) {
        self.gravity = None;
    }
}

/// The screen's sideways tilt in degrees from gravity in the device frame:
/// right edge down is positive. `None` when the reading is too weak to have
/// a direction.
pub fn tilt_degrees(gravity: [f64; 3], orientation: ScreenOrientation) -> Option<f64> {
    let magnitude =
        (gravity[0] * gravity[0] + gravity[1] * gravity[1] + gravity[2] * gravity[2]).sqrt();
    if !magnitude.is_finite() || magnitude < MIN_GRAVITY {
        return None;
    }
    let [right, _] = to_screen(gravity, orientation);
    Some((right / magnitude).clamp(-1.0, 1.0).asin().to_degrees())
}

#[cfg(test)]
mod tests {
    use super::*;

    const G: f64 = 9.80665;

    /// SDL's reading for a phone whose right edge is down by `degrees`,
    /// held upright in portrait: gravity has a share along +x, the rest
    /// down the screen (-y), and SDL reports minus that.
    fn portrait_reading(degrees: f64) -> [f64; 3] {
        let r = degrees.to_radians();
        [-(G * r.sin()), G * r.cos(), 0.0]
    }

    #[test]
    fn each_orientation_maps_the_device_axes_onto_the_screen() {
        let x = [1.0, 0.0, 0.0];
        let y = [0.0, 1.0, 0.0];
        use ScreenOrientation::*;
        let table: &[(ScreenOrientation, [f64; 2], [f64; 2])] = &[
            (Portrait, [1.0, 0.0], [0.0, 1.0]),
            (PortraitFlipped, [-1.0, 0.0], [0.0, -1.0]),
            (Landscape, [0.0, 1.0], [-1.0, 0.0]),
            (LandscapeFlipped, [0.0, -1.0], [1.0, 0.0]),
        ];
        for &(orientation, device_x, device_y) in table {
            assert_eq!(to_screen(x, orientation), device_x, "{orientation:?} x");
            assert_eq!(to_screen(y, orientation), device_y, "{orientation:?} y");
        }
    }

    #[test]
    fn right_edge_down_reads_positive_in_every_orientation() {
        use ScreenOrientation::*;
        // The screen's right edge down by 20 degrees, for each way the
        // interface can face: gravity's share along the screen's right.
        let s = 20f64.to_radians().sin();
        let c = 20f64.to_radians().cos();
        let table: &[(ScreenOrientation, [f64; 3])] = &[
            // Screen right is device +x.
            (Portrait, [s, -c, 0.0]),
            // Screen right is device -x.
            (PortraitFlipped, [-s, c, 0.0]),
            // Screen right is device -y, screen down is device -x.
            (Landscape, [-c, -s, 0.0]),
            // Screen right is device +y, screen down is device +x.
            (LandscapeFlipped, [c, s, 0.0]),
        ];
        for &(orientation, gravity) in table {
            let g = gravity.map(|v| v * G);
            let degrees = tilt_degrees(g, orientation).unwrap();
            assert!((degrees - 20.0).abs() < 1e-9, "{orientation:?}: {degrees}");
        }
    }

    #[test]
    fn upright_and_flat_read_the_same_tilt() {
        // Twenty degrees of roll about the screen's normal (upright) and
        // about the long axis lying flatter both read twenty.
        let r = 20f64.to_radians();
        let upright = [r.sin() * G, -r.cos() * G, 0.0];
        let flat = [r.sin() * G, 0.0, -r.cos() * G];
        let pitched = [r.sin() * G, -r.cos() * G * 0.6, -r.cos() * G * 0.8];
        for g in [upright, flat, pitched] {
            let degrees = tilt_degrees(g, ScreenOrientation::Portrait).unwrap();
            assert!((degrees - 20.0).abs() < 1e-9, "{g:?}: {degrees}");
        }
    }

    #[test]
    fn the_filter_turns_an_sdl_reading_into_gravity_and_smooths_a_jolt() {
        let mut filter = GravityFilter::default();
        let g = filter.feed(portrait_reading(10.0), 1.0 / 60.0);
        let degrees = tilt_degrees(g, ScreenOrientation::Portrait).unwrap();
        assert!((degrees - 10.0).abs() < 1e-9, "{degrees}");
        // One frame's jolt of a whole g sideways, which raw would read as
        // 45 degrees, moves it a few.
        let jolt = [-G, G, 0.0];
        let g = filter.feed(jolt, 1.0 / 60.0);
        let degrees = tilt_degrees(g, ScreenOrientation::Portrait).unwrap();
        assert!(degrees < 18.0, "{degrees}");
        // A held pose wins within a few time constants.
        for _ in 0..60 {
            filter.feed(portrait_reading(-15.0), 1.0 / 60.0);
        }
        let g = filter.feed(portrait_reading(-15.0), 1.0 / 60.0);
        let degrees = tilt_degrees(g, ScreenOrientation::Portrait).unwrap();
        assert!((degrees + 15.0).abs() < 0.1, "{degrees}");
    }

    #[test]
    fn a_reading_with_no_weight_has_no_tilt() {
        assert_eq!(
            tilt_degrees([0.0, 0.0, 0.0], ScreenOrientation::Portrait),
            None
        );
        assert_eq!(
            tilt_degrees([f64::NAN, 0.0, 9.8], ScreenOrientation::Portrait),
            None
        );
    }
}
