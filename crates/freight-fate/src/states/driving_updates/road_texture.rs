//! Seams and bridge joints under the wheels (see `ff_core::sim::road_texture`):
//! on a jointed stretch each seam is two thumps, the steer axle and then the
//! drives a wheelbase later, and each end of a river bridge is a heavier
//! joint under all three axles. Pure ambience: nothing here is spoken.

use ff_core::sim::outside::near_river;
use ff_core::sim::road_texture::Axle;
use ff_core::sim::road_texture::{
    AxleHits, Crossing, Hit, RoadSurface, RoadSurfaceEvent, SEAM_MIN_SPEED_MPS, SEAM_SPACING_M,
};

use crate::app::GameContext;
use crate::states::driving::DrivingState;

/// A seam under the steer axle at 30 m/s and above. Orinks's ear sets it;
/// it starts where the demo he heard sat, about 10 dB over the road loop's
/// peaks at 65 mph.
pub const SEAM_LEVEL: f64 = 0.5;
/// The drive tandem lands a little harder than the steer axle.
pub const DRIVE_AXLE_GAIN: f64 = 1.25;
/// A bridge end joint, every axle. Orinks's ear sets it.
pub const BRIDGE_JOINT_LEVEL: f64 = 0.75;
/// The small jolt a bridge joint gives the wheel, under the steer axle.
const BRIDGE_JOINT_IMPACT: f64 = 0.2;

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
    /// Whether the truck was over a river last frame, so each bridge end is
    /// crossed once.
    pub on_bridge: bool,
}

impl RoadTexture {
    pub fn new(surface: RoadSurface) -> RoadTexture {
        RoadTexture {
            surface,
            hits: AxleHits::default(),
            on_bridge: false,
        }
    }
}

impl DrivingState {
    /// Called from `update_audio` each frame. `dt` is real seconds, and the
    /// distances are real wheel travel, not the trip model's compressed
    /// route distance, so a stretch sounds the same at every pace.
    pub fn update_road_texture(&mut self, ctx: &mut GameContext, dt: f64) {
        if dt <= 0.0 {
            return;
        }
        let speed = self.trip.truck.velocity_mps.max(0.0);
        let metres = speed * dt;
        let severity = (speed / 30.0).min(1.0);
        let rolling = speed > SEAM_MIN_SPEED_MPS;

        let mut landed = self.road_texture.hits.travel(metres);
        self.road_texture.surface = self.road_texture.surface.next(
            RoadSurfaceEvent::Travel { metres },
            &mut self.road_texture_rng,
        );

        // Ramps are outside the highway soundscape.
        if rolling && !self.trip.on_ramp && self.road_texture.surface.is_jointed() {
            self.road_joint_accumulator_m += metres;
            if self.road_joint_accumulator_m >= self.next_joint_distance_m {
                self.road_joint_accumulator_m %= self.next_joint_distance_m;
                self.next_joint_distance_m = self
                    .road_texture_rng
                    .uniform(SEAM_SPACING_M.0, SEAM_SPACING_M.1);
                landed.push(self.road_texture.hits.cross(Crossing::Seam));
            }
        }

        let over = near_river(self.outside.river_miles(), self.trip.position_mi);
        if over != self.road_texture.on_bridge {
            self.road_texture.on_bridge = over;
            if rolling {
                landed.push(self.road_texture.hits.cross(Crossing::BridgeJoint));
                ctx.controller.rumble.impact(BRIDGE_JOINT_IMPACT);
            }
        }

        for hit in landed {
            play_hit(ctx, hit, severity);
        }
    }
}

fn play_hit(ctx: &mut GameContext, hit: Hit, severity: f64) {
    let (bank, level) = match (hit.crossing, hit.axle) {
        (Crossing::Seam, Axle::Steer) => (SEAM_STEER, SEAM_LEVEL),
        (Crossing::Seam, _) => (SEAM_DRIVE, SEAM_LEVEL * DRIVE_AXLE_GAIN),
        (Crossing::BridgeJoint, _) => (BRIDGE_JOINT, BRIDGE_JOINT_LEVEL),
    };
    ctx.audio
        .play_bank_with(bank, FALLBACK, level * severity, 0.0);
    ctx.controller.rumble.joint(severity);
}
