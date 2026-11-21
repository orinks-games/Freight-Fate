//! The beds outside the truck each frame: birds or insects, and a river
//! under the bridge, each heard through the glass or clear through the open
//! driver window.
//!
//! Pure ambience (see `ff_core::sim::outside`): nothing here is spoken. Each
//! bed is two copies started together and kept running for as long as the
//! bed lasts, one of them at zero if need be, so moving the glass is a
//! crossfade of two aligned recordings.

use ff_core::sim::outside::{
    bed_mix, near_river, road_mask, road_mask_to, scene_bed, Bed, BedFader, DayPhase,
    OutsideConditions, OutsideScene, Water, WaterEvent, NATURE_LEVEL, RIVER, SCENE_FADE_S,
    WATER_FADE_IN_S, WATER_FADE_OUT_S, WATER_LEVEL,
};

use crate::app::GameContext;
use crate::audio::{CH_NATURE_OPEN, CH_NATURE_SEALED, CH_WATER_OPEN, CH_WATER_SEALED};
use crate::states::driving::DrivingState;

/// The open window is on the driver's side, the same lean as its wind rush.
const OPEN_PAN: f64 = -0.5;
/// How quickly a bed's loops go when there is nothing left to play.
const STOP_FADE_MS: u32 = 100;

/// What the outside beds hold between frames.
#[derive(Clone, Debug, Default)]
pub struct OutsideAudio {
    pub scene: OutsideScene,
    pub water: Water,
    pub nature: BedFader,
    pub river: BedFader,
    /// The trip's river miles, and the trip they were read from.
    rivers: Vec<f64>,
    rivers_trip: Option<u64>,
    temperature_c: Option<f64>,
}

impl OutsideAudio {
    /// The river miles in use, for a test or the agent server.
    pub fn river_miles(&self) -> &[f64] {
        &self.rivers
    }

    /// The temperature the scene was last judged on.
    pub fn temperature_c(&self) -> Option<f64> {
        self.temperature_c
    }
}

impl DrivingState {
    /// Called in `update_audio` after `update_cab_audio`, which moves the
    /// glass this frame's mix follows.
    pub fn update_outside_audio(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.outside.rivers_trip != Some(self.trip_generation) {
            self.outside.rivers = self.trip.river_crossing_miles();
            self.outside.rivers_trip = Some(self.trip_generation);
        }
        // Read fresh each frame: a staged or resumed drive moves its clock
        // after the first frame, and a reading held from before that judged
        // the first seconds on the wrong hour. The scene's hysteresis keeps
        // a wobbling reading from flapping the bed.
        self.outside.temperature_c = self.trip.weather.temperature_c();
        let now = OutsideConditions {
            phase: DayPhase::at_hour(self.trip.local_hour()),
            temperature_c: self.outside.temperature_c,
            weather: self.trip.weather.current,
        };
        let o = &mut self.outside;
        o.scene = o.scene.next(now);
        let want = scene_bed(o.scene);
        o.nature = o.nature.next(want, dt, SCENE_FADE_S, SCENE_FADE_S);
        o.water = o.water.next(WaterEvent::Tick {
            dt_real_s: dt,
            near: near_river(&o.rivers, self.trip.position_mi),
        });
        let river = o.water.is_over().then_some(RIVER);
        o.river = o.river.next(river, dt, WATER_FADE_IN_S, WATER_FADE_OUT_S);

        let glass = self.cab.glass();
        let speed = self.trip.truck.velocity_mps;
        let (nature, river) = (o.nature, o.river);
        let masks = nature.bed.map_or((1.0, 1.0), |bed| {
            (road_mask(speed), road_mask_to(speed, bed.sealed_road_floor))
        });
        play_bed(
            ctx,
            (CH_NATURE_SEALED, CH_NATURE_OPEN),
            nature,
            NATURE_LEVEL,
            glass,
            masks,
        );
        // The tires do not cover the river: it is short, and it is the point.
        play_bed(
            ctx,
            (CH_WATER_SEALED, CH_WATER_OPEN),
            river,
            WATER_LEVEL,
            glass,
            (1.0, 1.0),
        );
    }
}

/// Both copies of a fader's bed at the frame's mix, or both stopped.
fn play_bed(
    ctx: &mut GameContext,
    (sealed_ch, open_ch): (u32, u32),
    fader: BedFader,
    level: f64,
    glass: f64,
    (open_mask, sealed_mask): (f64, f64),
) {
    let Some(Bed { open, sealed, .. }) = fader.bed else {
        ctx.audio.stop_loop_with(sealed_ch, STOP_FADE_MS);
        ctx.audio.stop_loop_with(open_ch, STOP_FADE_MS);
        return;
    };
    let mix = bed_mix(level * fader.level, glass, open_mask, sealed_mask);
    // Same key as last frame: the backend only moves the level.
    ctx.audio.start_loop_with(sealed_ch, sealed, mix.sealed, 0);
    ctx.audio.start_loop_with(open_ch, open, mix.open, 0);
    ctx.audio.set_loop_pan(open_ch, OPEN_PAN);
}
