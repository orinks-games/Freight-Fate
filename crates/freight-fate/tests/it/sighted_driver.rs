//! The sighted driver: a player who reads the visual display and nothing
//! else. Shared by the in-process drives (`visuals_play`) and the drive in
//! a real window (`tests/real_window.rs`), so both play by the same rules.
//!
//! Its only input is the display's capture of the drive -- what the window
//! draws -- and its only output is key presses and releases, the same ones
//! a player makes.

use freight_fate::states::base::{InputEvent, Key};
use freight_fate::visuals::scene_drive::{DriveView, RoadAhead};

pub const DT: f64 = 1.0 / 60.0;
const FT_PER_MI: f32 = 5280.0;

/// Which pedal is down, which way the wheel is held.
#[derive(Default)]
struct Controls {
    pedal: Option<Key>,
    wheel: Option<Key>,
}

#[derive(Default)]
pub struct SightedDriver {
    controls: Controls,
    /// The lane it means to be in.
    pub target_lane: Option<i32>,
    /// The advisory of the bend it is slowing for, once it has read the sign.
    bend_mph: Option<f64>,
    in_bend: bool,
    /// Gap to the vehicle ahead in its lane last frame, feet.
    last_gap: Option<f32>,
    closing_fps: f32,
    /// Passes it has started.
    pub passes: u32,
}

/// What the dashboard's curve panel says, as a distance in feet.
fn shown_distance_ft(text: &str) -> Option<f32> {
    let number: String = text
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == ',')
        .filter(|c| *c != ',')
        .collect();
    let value: f32 = number.parse().ok()?;
    Some(if text.contains("mile") {
        value * FT_PER_MI
    } else {
        value
    })
}

/// How hard the road on screen bends right around the truck, 1/ft.
pub fn bend_under_truck(view: &DriveView) -> f32 {
    view.road
        .curvature
        .iter()
        .enumerate()
        .filter(|(i, _)| (-80.0..250.0).contains(&RoadAhead::sample_s(*i)))
        .map(|(_, k)| k.abs())
        .fold(0.0, f32::max)
}

impl SightedDriver {
    fn lane_clear(view: &DriveView, lane: i32, behind_ft: f32, ahead_ft: f32) -> bool {
        !view
            .traffic
            .iter()
            .any(|v| v.lane == lane && v.ahead_ft > -behind_ft && v.ahead_ft < ahead_ft)
    }

    /// One frame's look at the screen; returns the keys to press and release.
    pub fn decide(&mut self, view: &DriveView) -> Vec<InputEvent> {
        let hud = &view.hud;
        let lanes = &view.lanes;
        let speed = hud.speed_mph;
        let speed_fps = (speed * 1.4667) as f32;
        let lane = lanes.truck_lane;
        let target_lane = *self.target_lane.get_or_insert(lane);

        // Speed: the posted limit on the sign, a little under.
        let limit = hud.limit_mph.unwrap_or(55.0);
        let mut target = limit - 2.0;

        // Bends: read the warning sign's advisory once braking for it from
        // here would start to take most of the distance shown.
        if let Some((_, advisory, distance, _)) = &hud.curve {
            if let Some(ft) = shown_distance_ft(distance) {
                let (v, a) = (speed_fps, *advisory as f32 * 1.4667);
                let needed = ((v * v - a * a).max(0.0) / (2.0 * 4.5)) + 700.0;
                if ft < needed {
                    self.bend_mph = Some(*advisory as f64);
                }
            }
        }
        let bending = bend_under_truck(view) > 1.0 / 5000.0;
        if bending && self.bend_mph.is_some() {
            self.in_bend = true;
        }
        if self.in_bend && !bending {
            // Through it.
            self.in_bend = false;
            self.bend_mph = None;
        }
        if let Some(mph) = self.bend_mph {
            target = target.min(mph - 3.0);
        }

        // Traffic: the nearest vehicle drawn ahead in the truck's own lane.
        let gap = view
            .traffic
            .iter()
            .filter(|v| v.lane == lane && v.ahead_ft > 0.0)
            .map(|v| v.ahead_ft)
            .fold(f32::INFINITY, f32::min);
        if gap.is_finite() {
            if let Some(last) = self.last_gap {
                let now = (last - gap) / DT as f32;
                self.closing_fps = self.closing_fps * 0.9 + now * 0.1;
            }
            self.last_gap = Some(gap);
        } else {
            self.last_gap = None;
            self.closing_fps = 0.0;
        }
        let closing = gap.is_finite() && self.closing_fps > 3.0 && gap < 900.0;
        if closing && target_lane == lane && !self.in_bend {
            // Pass on the left when the radar shows the left lane open.
            if lane + 1 < lanes.count
                && !lanes.blocked_left
                && Self::lane_clear(view, lane + 1, 200.0, 500.0)
            {
                self.target_lane = Some(lane + 1);
                self.passes += 1;
            }
        }
        if target_lane > 0 && target_lane == lane && !closing {
            // Keep right once the right lane is clear well past the car passed.
            if !lanes.blocked_right && Self::lane_clear(view, target_lane - 1, 250.0, 700.0) {
                self.target_lane = Some(target_lane - 1);
            }
        }
        if target_lane == lane && gap.is_finite() {
            // Keep a few seconds back: ease off while the gap shrinks on
            // screen, and drop below the car's pace inside the gap.
            let want_gap = (speed_fps * 3.5).max(180.0);
            if gap < want_gap * 1.8 && self.closing_fps > 0.0 {
                target = target.min(speed - (self.closing_fps / 1.4667) as f64 - 2.0);
            }
            if gap < want_gap {
                target = target.min(speed - 4.0);
            }
        }

        let mut events = Vec::new();
        let pedal = if speed < target - 1.0 {
            Some(Key::Up)
        } else if speed > target + 1.5 {
            Some(Key::Down)
        } else {
            None
        };
        Self::hold(&mut self.controls.pedal, pedal, &mut events);

        // Steering: keep the truck, as drawn between the lane lines, on the
        // centre of the lane it wants, looking ahead along its heading.
        let target_lane = self.target_lane.unwrap_or(lane);
        let w = lanes.lane_width_ft;
        let u = -(lane as f32 + 0.5) * w + lanes.offset_ft;
        let want = -(target_lane as f32 + 0.5) * w;
        // Where the truck will be in a second and a half on its present
        // heading: steering to that, not to where it is, keeps it from
        // swinging past the lane it is moving into.
        let error = u - want + lanes.yaw.sin() * speed_fps * 1.5;
        let wheel = if error > 0.6 {
            Some(Key::Left)
        } else if error < -0.6 {
            Some(Key::Right)
        } else {
            None
        };
        Self::hold(&mut self.controls.wheel, wheel, &mut events);
        events
    }

    fn hold(slot: &mut Option<Key>, want: Option<Key>, events: &mut Vec<InputEvent>) {
        if *slot == want {
            return;
        }
        if let Some(key) = slot.take() {
            events.push(InputEvent::key_up(key));
        }
        if let Some(key) = want {
            events.push(InputEvent::key(key));
            *slot = Some(key);
        }
    }
}
