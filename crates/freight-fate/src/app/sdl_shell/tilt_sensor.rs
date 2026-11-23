//! The phone's motion sensor and the screen lock, for tilt steering.
//!
//! The accelerometer comes through SDL2's sensor API, so iPhone and Android
//! share this path and no native code reads it. What SDL hands over (units,
//! sign, frame, raw or gravity) is written down, from the SDL source, in
//! [`ff_core::sim::tilt_sensor`], which does the arithmetic. The sensor runs
//! only while tilt steering is on and the app is in the foreground; it stops
//! (the `Sensor` is dropped, which stops CoreMotion's updates) the frame
//! either ends.
//!
//! The screen lock: SDL's UIKit view controller answers
//! `supportedInterfaceOrientations` from the `SDL_IOS_ORIENTATIONS` hint each
//! time UIKit asks (`src/video/uikit/SDL_uikitwindow.m`,
//! `UIKit_GetSupportedOrientations`), so locking sets the hint to the
//! orientation the screen has and asks UIKit to ask again
//! (`ios/ff_orientation.m`); unlocking puts all four back. Android reads that
//! hint only when the window is made, so there the screen is not locked.

use std::time::Instant;

use ff_core::sim::tilt_sensor::{tilt_degrees, GravityFilter, ScreenOrientation};

use crate::app::tilt::TiltRead;

/// Every orientation the game turns to when nothing is locked.
pub(super) const ALL_ORIENTATIONS: &str =
    "Portrait PortraitUpsideDown LandscapeLeft LandscapeRight";

/// The accelerometer while tilt steering is running.
struct Running {
    /// Kept so the subsystem stays initialised while the sensor is open.
    _subsystem: sdl2::SensorSubsystem,
    sensor: sdl2::sensor::Sensor,
    filter: GravityFilter,
    last_raw: Option<[f32; 3]>,
    last_at: Option<Instant>,
}

#[derive(Default)]
pub(super) struct TiltSensor {
    running: Option<Running>,
    /// Opening found no accelerometer; not tried again until tilt steering
    /// stops and starts.
    missing: bool,
    /// Whether the screen's orientation is locked.
    locked: bool,
}

impl TiltSensor {
    /// This frame's reading. `want` is tilt steering on with the app in the
    /// foreground; without it the sensor is stopped.
    pub(super) fn read(&mut self, sdl: &sdl2::Sdl, want: bool) -> TiltRead {
        if !want {
            if self.running.take().is_some() {
                log::info!("tilt: motion sensor stopped");
            }
            self.missing = false;
            return TiltRead::Idle;
        }
        if self.missing {
            return TiltRead::Missing;
        }
        if self.running.is_none() {
            match open_accelerometer(sdl) {
                Ok(running) => {
                    log::info!("tilt: motion sensor started ({})", running.sensor.name());
                    self.running = Some(running);
                }
                Err(reason) => {
                    log::info!("tilt: no motion sensor ({reason})");
                    self.missing = true;
                    return TiltRead::Missing;
                }
            }
        }
        let Some(running) = self.running.as_mut() else {
            return TiltRead::Idle;
        };
        let raw = match running.sensor.get_data() {
            Ok(sdl2::sensor::SensorData::Accel(raw)) => raw,
            Ok(_) => return TiltRead::Idle,
            Err(error) => {
                log::warn!("tilt: reading the motion sensor failed: {error}");
                return TiltRead::Idle;
            }
        };
        // SDL passes a reading on only when it changed, and a live
        // accelerometer always jitters, so an unchanged one is no sample:
        // a second of them is a lost reading.
        if running.last_raw == Some(raw) {
            return TiltRead::Idle;
        }
        running.last_raw = Some(raw);
        let now = Instant::now();
        let dt = running
            .last_at
            .map_or(0.0, |at| now.duration_since(at).as_secs_f64());
        running.last_at = Some(now);
        let gravity = running.filter.feed(raw.map(f64::from), dt);
        match tilt_degrees(gravity, display_orientation()) {
            Some(degrees) => TiltRead::Sample(degrees),
            None => TiltRead::Idle,
        }
    }

    /// Hold the screen in the orientation it has, or let it turn again.
    /// Returns the hint the change set, `None` when nothing changed.
    pub(super) fn lock(&mut self, lock: bool) -> Option<&'static str> {
        if lock == self.locked {
            return None;
        }
        self.locked = lock;
        let hint = if lock {
            orientation_hint(display_orientation())
        } else {
            ALL_ORIENTATIONS
        };
        log::info!(
            "tilt: screen orientation {}",
            if lock { hint } else { "free" }
        );
        sdl2::hint::set("SDL_IOS_ORIENTATIONS", hint);
        Some(hint)
    }
}

fn open_accelerometer(sdl: &sdl2::Sdl) -> Result<Running, String> {
    let subsystem = sdl.sensor()?;
    let count = subsystem.num_sensors()?;
    for index in 0..count {
        let Ok(sensor) = subsystem.open(index) else {
            continue;
        };
        if sensor.sensor_type() == sdl2::sensor::SensorType::Accelerometer {
            return Ok(Running {
                _subsystem: subsystem,
                sensor,
                filter: GravityFilter::default(),
                last_raw: None,
                last_at: None,
            });
        }
    }
    Err(format!("{count} sensors, no accelerometer"))
}

/// The interface's orientation, as SDL reports it for the first display.
fn display_orientation() -> ScreenOrientation {
    use sdl2::sys::SDL_DisplayOrientation as O;
    // SAFETY: a plain query of SDL's display state; SDL answers UNKNOWN
    // rather than failing when the video subsystem has no such display.
    match unsafe { sdl2::sys::SDL_GetDisplayOrientation(0) } {
        O::SDL_ORIENTATION_PORTRAIT_FLIPPED => ScreenOrientation::PortraitFlipped,
        O::SDL_ORIENTATION_LANDSCAPE => ScreenOrientation::Landscape,
        O::SDL_ORIENTATION_LANDSCAPE_FLIPPED => ScreenOrientation::LandscapeFlipped,
        _ => ScreenOrientation::Portrait,
    }
}

/// The `SDL_IOS_ORIENTATIONS` name for one orientation. UIKit's landscape
/// names count the home edge, so SDL's landscape (right edge up) is UIKit's
/// LandscapeRight (`SDL_uikitmodes.m`).
fn orientation_hint(orientation: ScreenOrientation) -> &'static str {
    match orientation {
        ScreenOrientation::Portrait => "Portrait",
        ScreenOrientation::PortraitFlipped => "PortraitUpsideDown",
        ScreenOrientation::Landscape => "LandscapeRight",
        ScreenOrientation::LandscapeFlipped => "LandscapeLeft",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_orientation_locks_to_its_uikit_name() {
        for (orientation, name) in [
            (ScreenOrientation::Portrait, "Portrait"),
            (ScreenOrientation::PortraitFlipped, "PortraitUpsideDown"),
            (ScreenOrientation::Landscape, "LandscapeRight"),
            (ScreenOrientation::LandscapeFlipped, "LandscapeLeft"),
        ] {
            assert_eq!(orientation_hint(orientation), name);
            assert!(ALL_ORIENTATIONS.split(' ').any(|n| n == name));
        }
    }
}
