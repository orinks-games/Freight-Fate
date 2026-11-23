//! Seams and bridge joints under the wheels (see `ff_core::sim::road_texture`):
//! on jointed road each seam is two thumps, the steer axle and then the
//! drives a wheelbase later, as hard as the surface and its faulting say,
//! and each end of a bridge the truck hears is a heavier joint under all
//! three axles. On concrete grooved across the lane the tires whine, and on
//! an open steel grating they hum, at a pitch that follows the speed. Pure
//! ambience: nothing here is spoken.

use ff_core::sim::road_texture::joints::{joints_heard, JointSpan};
use ff_core::sim::road_texture::song::{SongEvent, TireSong, SONG_FADE_MS};
use ff_core::sim::road_texture::{
    Axle, AxleHits, Crossing, Hit, RoadSurface, RoadSurfaceEvent, SEAM_MIN_SPEED_MPS,
    SEAM_SPACING_M,
};

use crate::app::GameContext;
use crate::audio::{BANK_JITTER, CH_BRIDGE_DECK, CH_ROAD_SONG};
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
/// Real seconds between two seam rumbles. A seam every 4.6 m at highway
/// speed is twelve hits a second, and a pulse per hit would run together
/// into one steady buzz; the controller pulses at most this often, on the
/// steer axle, so each pulse is felt on its own. Assumed for feel. Bridge
/// joints pulse on every axle, as before.
pub const SEAM_RUMBLE_GAP_S: f64 = 0.25;
/// A frame that moves the truck farther than this is a staged jump, not
/// driving, and crosses no joint.
const JUMP_MI: f64 = 1.0;

const SEAM_STEER: &str = "vehicle/seam_steer";
const SEAM_DRIVE: &str = "vehicle/seam_drive";
const BRIDGE_JOINT: &str = "vehicle/bridge_joint";
const BRIDGE_DECK: &str = "vehicle/bridge_deck";
/// Reference speed (60 mph) for the concrete bridge deck resonance bed.
const BRIDGE_DECK_REF_MPS: f64 = 26.82;
/// Level of the concrete bridge deck bed at and above reference speed.
const BRIDGE_DECK_LEVEL: f64 = 0.42;
/// Fade time in milliseconds when entering or leaving a concrete bridge deck.
const BRIDGE_DECK_FADE_MS: u32 = 220;
/// A pack without the banks still has the old seam.
const FALLBACK: &str = "vehicle/road_joint";

/// What the road texture holds between frames.
#[derive(Clone, Debug)]
pub struct RoadTexture {
    pub surface: RoadSurface,
    /// What the tires are singing.
    pub song: TireSong,
    pub hits: AxleHits,
    /// The route's jointed spans (`Trip::bridge_joint_spans`).
    pub spans: Vec<JointSpan>,
    /// Real seconds since the last bridge joint the driver heard.
    pub since_joint_s: f64,
    /// The route mile the truck was at last frame.
    pub last_mi: f64,
    /// Real seconds since the controller last pulsed for a seam.
    pub since_seam_rumble_s: f64,
}

impl RoadTexture {
    pub fn new(
        surface: RoadSurface,
        song: TireSong,
        spans: Vec<JointSpan>,
        position_mi: f64,
    ) -> RoadTexture {
        RoadTexture {
            surface,
            song,
            hits: AxleHits::default(),
            spans,
            since_joint_s: f64::INFINITY,
            last_mi: position_mi,
            since_seam_rumble_s: f64::INFINITY,
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

        self.update_tire_song(ctx, speed, dt);
        self.update_bridge_deck(ctx, speed, position);

        self.road_texture.since_seam_rumble_s += dt;
        for hit in landed {
            play_hit(ctx, hit, severity);
            if seam_rumbles(hit, self.road_texture.since_seam_rumble_s) {
                ctx.controller.rumble.joint(severity);
                if hit.crossing == Crossing::Seam {
                    self.road_texture.since_seam_rumble_s = 0.0;
                }
            }
        }
    }
}

impl DrivingState {
    /// The whine or the hum, one loop on the Road surface volume. Ramps are
    /// outside the highway soundscape, as for the seams. `dt` is real
    /// seconds, which the song's holds count.
    fn update_tire_song(&mut self, ctx: &mut GameContext, speed: f64, dt: f64) {
        let under = if self.trip.on_ramp {
            ff_core::sim::road_texture::song::Underfoot::Plain
        } else {
            self.trip.underfoot_at(self.trip.position_mi)
        };
        let song = self.road_texture.song.next(SongEvent::Under { under, dt });
        self.road_texture.song = song;
        match song.mix(speed) {
            Some((sound, rate, level)) => {
                ctx.audio
                    .start_loop_with(CH_ROAD_SONG, sound.key, level, SONG_FADE_MS);
                ctx.audio.set_loop_rate(CH_ROAD_SONG, rate);
            }
            None => ctx.audio.stop_loop_with(CH_ROAD_SONG, SONG_FADE_MS),
        }
    }

    /// Hollow concrete resonance bed while rolling across a bridge span.
    /// Open steel grates keep their own grate hum instead.
    fn update_bridge_deck(&mut self, ctx: &mut GameContext, speed: f64, position: f64) {
        let on_grate = matches!(
            self.road_texture.song,
            TireSong::Hum(h) if !h.past
        ) || matches!(
            self.road_texture.song,
            TireSong::Held(ff_core::sim::road_texture::song::HeldSong::Hum)
        );
        let on_span = matches!(
            self.trip.bridge,
            ff_core::sim::bridges::BridgeCrossing::On { .. }
        ) || self
            .road_texture
            .spans
            .iter()
            .any(|s| position >= s.from_mi && position <= s.to_mi);
        if !self.trip.on_ramp && !on_grate && on_span && speed > SEAM_MIN_SPEED_MPS {
            let ratio = speed / BRIDGE_DECK_REF_MPS;
            let rate = ratio.clamp(0.55, 1.35);
            let level = BRIDGE_DECK_LEVEL * ratio.powf(1.2).min(1.0);
            ctx.audio
                .start_loop_with(CH_BRIDGE_DECK, BRIDGE_DECK, level, BRIDGE_DECK_FADE_MS);
            ctx.audio.set_loop_rate(CH_BRIDGE_DECK, rate);
        } else {
            ctx.audio
                .stop_loop_with(CH_BRIDGE_DECK, BRIDGE_DECK_FADE_MS);
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

/// Whether a hit pulses the controller: every bridge joint hit, and a
/// seam's steer hit once [`SEAM_RUMBLE_GAP_S`] has passed since the last.
pub fn seam_rumbles(hit: Hit, since_seam_rumble_s: f64) -> bool {
    match hit.crossing {
        Crossing::BridgeJoint => true,
        Crossing::Seam => hit.axle == Axle::Steer && since_seam_rumble_s >= SEAM_RUMBLE_GAP_S,
    }
}

fn play_hit(ctx: &mut GameContext, hit: Hit, severity: f64) {
    let (bank, level) = hit_sound(hit, severity);
    ctx.audio.play_bank_with(bank, FALLBACK, level, 0.0);
}
