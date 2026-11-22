//! Tilt steering's place in the loop: what the sensor (or the agent server
//! standing in for it) read this frame, fed to the one machine
//! ([`ff_core::sim::tilt_steering`]) with the setting and whether the player
//! is at the wheel, and the lines it says.
//!
//! The drive reads [`TiltInput::steering`] where it reads the controller's
//! left stick (`driving_updates/lanes.rs`). The shell locks the screen's
//! orientation from [`TiltSteering::locks_screen`].

use ff_core::sim::tilt_steering::{TiltEvent, TiltLine, TiltSteering};

use super::App;

/// Whether tilt steering is wanted: the switch is on and this is a phone
/// (`crate::device::phone`). A desktop with the switch set in its file, or
/// a laptop with an accelerometer of its own, never opens the sensor.
pub fn wanted(settings: &ff_core::settings::Settings) -> bool {
    settings.touch_tilt_steering && crate::device::phone()
}

/// What the motion sensor gave this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TiltRead {
    /// Not running, or nothing new since the last frame.
    Idle,
    /// The device has no motion sensor, or it would not start.
    Missing,
    /// A fresh angle from flat, in degrees, right edge down positive.
    Sample(f64),
}

/// Tilt steering as the game holds it between frames.
#[derive(Debug)]
pub struct TiltInput {
    machine: TiltSteering,
    /// The machine's clock: the frame time that has passed, so a held
    /// world (the agent server's lockstep) holds it too.
    clock_s: f64,
    at_wheel: bool,
    /// The last angle read, from flat.
    degrees: Option<f64>,
    /// Whether the device has a motion sensor, once known.
    pub sensor_present: Option<bool>,
    /// The agent server stands in for the sensor: the pose it holds the
    /// phone in, `None` while it sends no samples.
    agent_pose: Option<Option<f64>>,
    /// The agent server stands in for a phone with no motion sensor.
    agent_no_sensor: bool,
    lines: Vec<TiltLine>,
    /// Counts the machine's moves, so a line cut off mid-sentence is not
    /// replayed once it is no longer true ("No tilt reading" after the
    /// readings came back).
    moves: std::rc::Rc<std::cell::Cell<u64>>,
}

impl Default for TiltInput {
    fn default() -> Self {
        Self {
            machine: TiltSteering::Off,
            clock_s: 0.0,
            at_wheel: false,
            degrees: None,
            sensor_present: None,
            agent_pose: None,
            agent_no_sensor: false,
            lines: Vec::new(),
            moves: std::rc::Rc::default(),
        }
    }
}

impl TiltInput {
    pub fn state(&self) -> TiltSteering {
        self.machine
    }

    /// The last angle read from flat.
    pub fn degrees(&self) -> Option<f64> {
        self.degrees
    }

    /// The steering this frame, from -1 to 1: nothing unless live.
    pub fn steering(&self, full_degrees: f64) -> f64 {
        self.degrees
            .map_or(0.0, |degrees| self.machine.steering(degrees, full_degrees))
    }

    /// "Set tilt level": take level again from the pose the phone is in.
    pub fn request_level(&mut self) {
        let at_s = self.clock_s;
        self.feed(TiltEvent::SetLevel { at_s });
    }

    /// The agent server holds the phone at `degrees`, or sends nothing for
    /// `None`. The first call makes it the sensor for the rest of the run.
    pub fn set_agent_pose(&mut self, degrees: Option<f64>) {
        self.agent_pose = Some(degrees);
        self.agent_no_sensor = false;
        self.sensor_present = Some(true);
    }

    /// The agent server stands in for a phone with no motion sensor: the
    /// next time tilt steering runs, opening the sensor finds none.
    pub fn set_agent_no_sensor(&mut self) {
        self.agent_pose = Some(None);
        self.agent_no_sensor = true;
        self.sensor_present = None;
    }

    /// The agent server's pose, while it stands in for the sensor.
    pub fn agent_pose(&self) -> Option<Option<f64>> {
        self.agent_pose
    }

    fn feed(&mut self, event: TiltEvent) {
        let (next, line) = self.machine.next(event);
        if next.key() != self.machine.key() {
            log::debug!("tilt steering: {} -> {}", self.machine.key(), next.key());
            self.moves.set(self.moves.get() + 1);
        }
        self.machine = next;
        self.lines.extend(line);
    }

    /// One frame: the setting, the sensor's read, and whether the player
    /// is at the wheel, then the clock. Returns the lines to say.
    pub fn step(
        &mut self,
        setting: bool,
        at_wheel: bool,
        read: TiltRead,
        dt: f64,
    ) -> Vec<TiltLine> {
        self.clock_s += dt.max(0.0);
        let at_s = self.clock_s;
        if setting != self.machine.is_on() {
            self.feed(TiltEvent::Setting { on: setting });
        }
        match read {
            TiltRead::Missing => {
                self.sensor_present = Some(false);
                self.feed(TiltEvent::Sensor { present: false });
            }
            TiltRead::Sample(_) => {
                self.sensor_present = Some(true);
                self.feed(TiltEvent::Sensor { present: true });
            }
            TiltRead::Idle => {}
        }
        if at_wheel != self.at_wheel {
            self.at_wheel = at_wheel;
            self.feed(TiltEvent::Wheel { at_wheel, at_s });
        }
        if let TiltRead::Sample(degrees) = read {
            self.degrees = Some(degrees);
            self.feed(TiltEvent::Sample { degrees, at_s });
        }
        self.feed(TiltEvent::Tick { at_s });
        std::mem::take(&mut self.lines)
    }

    /// What the agent server's stand-in reads this frame.
    fn agent_read(&self, running: bool) -> Option<TiltRead> {
        let pose = self.agent_pose?;
        Some(match pose {
            _ if !running => TiltRead::Idle,
            _ if self.agent_no_sensor => TiltRead::Missing,
            Some(degrees) => TiltRead::Sample(degrees),
            None => TiltRead::Idle,
        })
    }

    /// The line `status` carries: the state, the level and the last angle.
    pub fn status_line(&self) -> String {
        let angle = |value: Option<f64>| value.map_or("none".to_string(), |d| format!("{d:.1}"));
        format!(
            "tilt {}; tilt_level {}; tilt_angle {}",
            self.machine.key(),
            angle(self.machine.level()),
            angle(self.degrees)
        )
    }
}

impl App {
    /// Read the sensor, move tilt steering, say what it says, and lock or
    /// free the screen's orientation. Runs after the frame's events, so a
    /// drive that just resumed is at the wheel this frame.
    pub(super) fn update_tilt(&mut self, dt: f64) {
        let setting = wanted(&self.ctx.settings);
        // Only in the foreground. Leaving the wheel for the app switcher, a
        // call or the home screen is leaving the wheel, so the return takes
        // level again.
        let active = self.app_place() == ff_core::app_lifecycle::AppPlace::Active;
        let running = setting && active;
        let at_wheel = active && self.touch_mode() == crate::touch::TouchMode::Wheel;
        let read = match self.ctx.tilt.agent_read(running) {
            Some(read) => read,
            None => match self.shell.as_mut() {
                Some(shell) => shell.read_tilt(running),
                None => TiltRead::Idle,
            },
        };
        for line in self.ctx.tilt.step(setting, at_wheel, read, dt) {
            match line {
                TiltLine::NoSensor => self.ctx.say(line.text()),
                _ => {
                    let moves = std::rc::Rc::clone(&self.ctx.tilt.moves);
                    let now = moves.get();
                    self.ctx.say_event_with(
                        line.text(),
                        super::SayEvent::new().valid(move || moves.get() == now),
                    );
                }
            }
        }
        // A phone with no motion sensor found once the switch was on: the
        // switch goes off in the file too, so the Touch gestures rows and
        // the command list agree with what steers.
        if setting && self.ctx.tilt.state() == TiltSteering::NoSensor {
            self.ctx.settings.touch_tilt_steering = false;
            if let Err(error) = self.ctx.settings.save() {
                log::warn!("Could not save settings: {error}");
            }
        }
        let lock = self.ctx.tilt.state().locks_screen();
        if let Some(shell) = self.shell.as_mut() {
            shell.lock_orientation(lock);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FRAME: f64 = 1.0 / 60.0;

    fn frames(tilt: &mut TiltInput, n: usize, at_wheel: bool, read: TiltRead) -> Vec<TiltLine> {
        (0..n)
            .flat_map(|_| tilt.step(true, at_wheel, read, FRAME))
            .collect()
    }

    #[test]
    fn the_drive_takes_level_then_steers_from_it() {
        let mut tilt = TiltInput::default();
        frames(&mut tilt, 10, false, TiltRead::Sample(10.0));
        assert_eq!(tilt.state().key(), "waiting");
        assert_eq!(tilt.steering(30.0), 0.0);
        let lines = frames(&mut tilt, 20, true, TiltRead::Sample(10.0));
        assert!(lines.is_empty());
        assert_eq!(tilt.state().level(), Some(10.0));
        frames(&mut tilt, 1, true, TiltRead::Sample(27.0));
        assert!((tilt.steering(30.0) - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_missing_sensor_is_said_once() {
        let mut tilt = TiltInput::default();
        let lines = frames(&mut tilt, 5, true, TiltRead::Missing);
        assert_eq!(lines, vec![TiltLine::NoSensor]);
        assert_eq!(tilt.state().key(), "no_sensor");
        assert_eq!(tilt.sensor_present, Some(false));
    }

    #[test]
    fn a_gap_in_the_samples_is_lost_and_back() {
        let mut tilt = TiltInput::default();
        frames(&mut tilt, 30, true, TiltRead::Sample(0.0));
        let lost = frames(&mut tilt, 70, true, TiltRead::Idle);
        assert_eq!(lost, vec![TiltLine::Lost]);
        let back = frames(&mut tilt, 30, true, TiltRead::Sample(6.0));
        assert_eq!(back, vec![TiltLine::Back]);
        assert_eq!(tilt.state().level(), Some(6.0));
    }

    #[test]
    fn a_held_world_holds_the_clock() {
        let mut tilt = TiltInput::default();
        frames(&mut tilt, 30, true, TiltRead::Sample(0.0));
        for _ in 0..500 {
            assert!(tilt.step(true, true, TiltRead::Idle, 0.0).is_empty());
        }
        assert_eq!(tilt.state().key(), "live");
    }
}
