//! Seams and bridge joints under the wheels (see `ff_core::sim::road_texture`):
//! on jointed road each seam is two thumps, the steer axle and then the
//! drives a wheelbase later, as hard as the surface and its faulting say,
//! and each end of a bridge the truck hears is a heavier joint under all
//! three axles. Pure ambience: nothing here is spoken.

use ff_core::sim::road_texture::joints::{joints_heard, JointSpan};
use ff_core::sim::road_texture::{
    Axle, AxleHits, Crossing, Hit, RoadSurface, RoadSurfaceEvent, SEAM_MIN_SPEED_MPS,
    SEAM_SPACING_M,
};

use crate::app::GameContext;
use crate::audio::BANK_JITTER;
use crate::states::driving::DrivingState;

/// A seam under the steer axle on good jointed concrete at 30 m/s and
/// above. Orinks's ear sets it; it starts where the demo he heard sat,
/// about 10 dB over the road loop's peaks at 65 mph.
pub const SEAM_LEVEL: f64 = 0.5;
/// The drive tandem lands a little harder than the steer axle.
pub const DRIVE_AXLE_GAIN: f64 = 1.25;
/// A bridge end joint, every axle. Orinks's ear sets it.
pub const BRIDGE_JOINT_LEVEL: f64 = 0.75;
/// The small jolt a bridge joint gives the wheel, under the steer axle.
const BRIDGE_JOINT_IMPACT: f64 = 0.2;
/// No hit asks for more than this, so the bank's own level variation never
/// takes it past 1.0 (the drives on poor concrete would reach 1.1).
pub const HIT_CEILING: f64 = 1.0 / BANK_JITTER.1;
/// A frame that moves the truck farther than this is a staged jump, not
/// driving, and crosses no joint.
const JUMP_MI: f64 = 1.0;

const SEAM_STEER: &str = "vehicle/seam_steer";
const SEAM_DRIVE: &str = "vehicle/seam_drive";
const BRIDGE_JOINT: &str = "vehicle/bridge_joint";
/// A pack without the banks still has the old seam.
const FALLBACK: &str = "vehicle/road_joint";

/// What the road texture holds between frames.
#[derive(Clone, Debug)]
pub struct RoadTexture {
    pub surface: RoadSurface,
    pub hits: AxleHits,
    /// The route's jointed spans (`Trip::bridge_joint_spans`).
    pub spans: Vec<JointSpan>,
    /// Real seconds since the last bridge joint the driver heard.
    pub since_joint_s: f64,
    /// The route mile the truck was at last frame.
    pub last_mi: f64,
}

impl RoadTexture {
    pub fn new(surface: RoadSurface, spans: Vec<JointSpan>, position_mi: f64) -> RoadTexture {
        RoadTexture {
            surface,
            hits: AxleHits::default(),
            spans,
            since_joint_s: f64::INFINITY,
            last_mi: position_mi,
        }
    }
}

impl DrivingState {
    /// Called from `update_audio` each frame. `dt` is real seconds, and the
    /// seam distances are real wheel travel, not the trip model's compressed
    /// route distance, so a stretch sounds the same at every pace.
    pub fn update_road_texture(&mut self, ctx: &mut GameContext, dt: f64) {
        if dt <= 0.0 {
            return;
        }
        let speed = self.trip.truck.velocity_mps.max(0.0);
        let metres = speed * dt;
        let severity = (speed / 30.0).min(1.0);
        let rolling = speed > SEAM_MIN_SPEED_MPS;
        let position = self.trip.position_mi;

        let mut landed = self.road_texture.hits.travel(metres);
        let reading = self.trip.pavement_reading_at(position);
        self.road_texture.surface = self.road_texture.surface.next(
            RoadSurfaceEvent::Travel { metres, reading },
            &mut self.road_texture_rng,
        );

        // Ramps are outside the highway soundscape.
        let seam = self.road_texture.surface.seam_factor();
        if let (Some(factor), true, false) = (seam, rolling, self.trip.on_ramp) {
            self.road_joint_accumulator_m += metres;
            if self.road_joint_accumulator_m >= self.next_joint_distance_m {
                self.road_joint_accumulator_m %= self.next_joint_distance_m;
                self.next_joint_distance_m = self
                    .road_texture_rng
                    .uniform(SEAM_SPACING_M.0, SEAM_SPACING_M.1);
                landed.push(self.road_texture.hits.cross(Crossing::Seam, factor));
            }
        }

        let from = self.road_texture.last_mi;
        self.road_texture.last_mi = position;
        let since = self.road_texture.since_joint_s + dt;
        let (joints, since) = if (position - from).abs() > JUMP_MI {
            (Vec::new(), since)
        } else {
            joints_heard(&self.road_texture.spans, from, position, since)
        };
        self.road_texture.since_joint_s = since;
        if rolling {
            for _ in joints {
                landed.push(self.road_texture.hits.cross(Crossing::BridgeJoint, 1.0));
                ctx.controller.rumble.impact(BRIDGE_JOINT_IMPACT);
            }
        }

        for hit in landed {
            play_hit(ctx, hit, severity);
        }
    }
}

/// The bank a hit plays from and the level it asks for: the axle, the
/// surface's factor and the speed, never above [`HIT_CEILING`].
pub fn hit_sound(hit: Hit, severity: f64) -> (&'static str, f64) {
    let (bank, level) = match (hit.crossing, hit.axle) {
        (Crossing::Seam, Axle::Steer) => (SEAM_STEER, SEAM_LEVEL * hit.factor),
        (Crossing::Seam, _) => (SEAM_DRIVE, SEAM_LEVEL * DRIVE_AXLE_GAIN * hit.factor),
        (Crossing::BridgeJoint, _) => (BRIDGE_JOINT, BRIDGE_JOINT_LEVEL * hit.factor),
    };
    (bank, (level * severity.clamp(0.0, 1.0)).min(HIT_CEILING))
}

fn play_hit(ctx: &mut GameContext, hit: Hit, severity: f64) {
    let (bank, level) = hit_sound(hit, severity);
    ctx.audio.play_bank_with(bank, FALLBACK, level, 0.0);
    ctx.controller.rumble.joint(severity);
}
