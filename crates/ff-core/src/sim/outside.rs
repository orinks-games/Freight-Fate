//! What is alive outside the truck, and how much of it reaches the cab.
//!
//! A player asked for crickets at night, birds by day and water under the
//! bridges, muffled with the driver window up and clear on the driver's side
//! with it down (2026-11-19). Nothing here is spoken and nothing touches the
//! drive: it decides which bed plays and at what level, so the game's audio
//! loop is plumbing and a test can assert the choice without a game.
//!
//! Three state machines, each moved only by its `next`:
//! - [`OutsideScene`]: quiet, birds or insects, from the clock, the
//!   temperature and the weather.
//! - [`Water`]: whether the truck is over a river, held a few real seconds.
//!   The trip moves it, with the river's name, through
//!   `sim::river_crossing::RiverCrossing`.
//! - [`BedFader`]: the bed actually playing and its level, which fades out
//!   before a new bed fades in.
//!
//! The window up or down is not a fourth one here: each bed is two aligned
//! copies, one rendered muffled, and [`bed_mix`] splits a level between them
//! by where the glass is (`sim::cab::Window`).

use crate::sim::hos;
use crate::sim::weather::WeatherKind;

/// The part of the day, as `hos::time_of_day` names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DayPhase {
    Dawn,
    Day,
    Dusk,
    Night,
}

impl DayPhase {
    /// The phase at a local wall-clock hour.
    pub fn at_hour(local_hour: f64) -> DayPhase {
        match hos::time_of_day(local_hour) {
            "dawn" => DayPhase::Dawn,
            "day" => DayPhase::Day,
            "dusk" => DayPhase::Dusk,
            _ => DayPhase::Night,
        }
    }
}

/// What the scene is judged on, read fresh each time.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OutsideConditions {
    pub phase: DayPhase,
    /// Outside temperature; `None` with seasons off, which counts as warm.
    pub temperature_c: Option<f64>,
    pub weather: WeatherKind,
}

/// Birds sing at dawn and by day from this temperature up.
///
/// Assumed for the ear, not sourced: a line that keeps the birds out of a
/// deep freeze without silencing an ordinary cold morning.
pub const BIRDS_MIN_C: f64 = 0.0;
/// Insects call at dusk and at night from this temperature up.
///
/// Assumed for the ear, not sourced: crickets go quiet as nights get cold,
/// and 10 °C is a round line for "a cool night with nothing calling".
pub const INSECTS_MIN_C: f64 = 10.0;
/// A scene already playing holds on until it is this much colder than its
/// line, so a reading wobbling around the line does not flap the bed.
pub const SCENE_HOLD_C: f64 = 2.0;

/// What is alive outside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OutsideScene {
    #[default]
    Quiet,
    Birds,
    Insects,
}

impl OutsideScene {
    /// The scene for `now`. The only place the scene moves.
    ///
    /// In order: heavy rain, a thunderstorm, snow or ice is quiet; dawn and
    /// day are birds from [`BIRDS_MIN_C`]; dusk and night are insects from
    /// [`INSECTS_MIN_C`]; anything else is quiet. The scene playing now holds
    /// [`SCENE_HOLD_C`] under its line.
    pub fn next(self, now: OutsideConditions) -> OutsideScene {
        if matches!(
            now.weather,
            WeatherKind::HeavyRain
                | WeatherKind::Thunderstorm
                | WeatherKind::Snow
                | WeatherKind::Ice
        ) {
            return OutsideScene::Quiet;
        }
        let (candidate, line) = match now.phase {
            DayPhase::Dawn | DayPhase::Day => (OutsideScene::Birds, BIRDS_MIN_C),
            DayPhase::Dusk | DayPhase::Night => (OutsideScene::Insects, INSECTS_MIN_C),
        };
        let line = if self == candidate {
            line - SCENE_HOLD_C
        } else {
            line
        };
        match now.temperature_c {
            Some(t) if t < line => OutsideScene::Quiet,
            _ => candidate,
        }
    }

    /// The state's name, as the agent server reports it.
    pub fn name(self) -> &'static str {
        match self {
            OutsideScene::Quiet => "quiet",
            OutsideScene::Birds => "birds",
            OutsideScene::Insects => "insects",
        }
    }
}

/// The bed a scene plays. One insect chorus every night (Orinks,
/// 2026-11-20).
pub fn scene_bed(scene: OutsideScene) -> Option<Bed> {
    match scene {
        OutsideScene::Quiet => None,
        OutsideScene::Birds => Some(BIRDS),
        OutsideScene::Insects => Some(INSECTS),
    }
}

/// One outside bed: the clear recording and its muffled (window up) twin,
/// rendered from it so the two play in step.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bed {
    /// The bed's name, as the agent server reports it.
    pub name: &'static str,
    pub open: &'static str,
    pub sealed: &'static str,
    /// How far the road may cover the muffled copy at speed (see
    /// [`road_mask_to`]). The insects keep more: through a closed window at
    /// highway speed they are how the driver knows it is night (Orinks,
    /// 2026-11-19).
    pub sealed_road_floor: f64,
}

/// How far the road covers a bed at speed, unless the bed says otherwise.
pub const ROAD_FLOOR: f64 = 0.5;
/// The insects' muffled copy at speed. Kept under the cab road bed through
/// closed glass at highway speed while still audible when stopped or with the
/// window cracked.
pub const INSECTS_SEALED_ROAD_FLOOR: f64 = 0.35;

pub const INSECTS: Bed = Bed {
    name: "insects",
    open: "ambient/insects",
    sealed: "ambient/insects_sealed",
    sealed_road_floor: INSECTS_SEALED_ROAD_FLOOR,
};
pub const BIRDS: Bed = Bed {
    name: "birds_day",
    open: "ambient/birds_day",
    sealed: "ambient/birds_day_sealed",
    sealed_road_floor: ROAD_FLOOR,
};
pub const RIVER: Bed = Bed {
    name: "river",
    open: "ambient/river",
    sealed: "ambient/river_sealed",
    sealed_road_floor: ROAD_FLOOR,
};

/// Within this many route miles of a river point the truck is over it.
pub const RIVER_SPAN_MI: f64 = 0.15;
/// Real seconds the river keeps playing after the truck leaves it. At the
/// default pacing a crossing passes in about a second of wall time, too short
/// to hear.
pub const RIVER_HOLD_S: f64 = 6.0;

/// Whether the truck is over water.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub enum Water {
    #[default]
    Away,
    /// Over a river, or just past one: `hold_s` real seconds are left.
    Over { hold_s: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum WaterEvent {
    /// Real time passed; `near` is whether a river point is within
    /// [`RIVER_SPAN_MI`] of the truck.
    Tick { dt_real_s: f64, near: bool },
}

impl Water {
    /// The water after `event`. The only place it moves.
    pub fn next(self, event: WaterEvent) -> Water {
        let WaterEvent::Tick { dt_real_s, near } = event;
        match self {
            _ if near => Water::Over {
                hold_s: RIVER_HOLD_S,
            },
            Water::Away => Water::Away,
            Water::Over { hold_s } => {
                let hold_s = hold_s - dt_real_s.max(0.0);
                if hold_s <= 0.0 {
                    Water::Away
                } else {
                    Water::Over { hold_s }
                }
            }
        }
    }

    pub fn is_over(self) -> bool {
        matches!(self, Water::Over { .. })
    }

    /// The state's name, as the agent server reports it.
    pub fn name(self) -> &'static str {
        match self {
            Water::Away => "away",
            Water::Over { .. } => "over",
        }
    }
}

/// Whether a river point is within [`RIVER_SPAN_MI`] of `position_mi`.
/// `rivers` is sorted, as `Trip::river_crossing_miles` returns it.
pub fn near_river(rivers: &[f64], position_mi: f64) -> bool {
    let i = rivers.partition_point(|&at| at < position_mi - RIVER_SPAN_MI);
    rivers
        .get(i)
        .is_some_and(|&at| at <= position_mi + RIVER_SPAN_MI)
}

/// The level of a nature bed (birds, insects) with the window down and the
/// truck parked. Orinks's ear sets it. Its loops ride the Weather volume,
/// 0.65 by default, where the old night bed rode Effects at 0.8; the 1.8 dB
/// between them is in the recordings, so a warm night is as loud as it was.
/// Never above 1: the engine clamps a sound there, and a bed above it would
/// flatten at a high Weather volume.
pub const NATURE_LEVEL: f64 = 1.0;
/// The river's level, window down. Orinks's ear sets it. The 6 dB the river
/// sits above the nature beds is in its recording, so the two share a level.
pub const WATER_LEVEL: f64 = 1.0;
/// Real seconds a nature bed takes to fade in, and again to fade out.
pub const SCENE_FADE_S: f64 = 3.0;
pub const WATER_FADE_IN_S: f64 = 0.8;
pub const WATER_FADE_OUT_S: f64 = 2.5;

/// How much of a bed's level each copy carries.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BedMix {
    /// The muffled copy, centred.
    pub sealed: f64,
    /// The clear copy, on the driver's side.
    pub open: f64,
}

/// Split `level` between the two copies by the glass (0 up, 1 down), each
/// after its own road mask.
pub fn bed_mix(level: f64, glass: f64, open_mask: f64, sealed_mask: f64) -> BedMix {
    let glass = glass.clamp(0.0, 1.0);
    let level = level.max(0.0);
    BedMix {
        sealed: level * sealed_mask * (1.0 - glass),
        open: level * open_mask * glass,
    }
}

/// The tires cover a nature bed at speed: full parked, half at 30 m/s
/// (about 67 mph) and above.
pub fn road_mask(speed_mps: f64) -> f64 {
    road_mask_to(speed_mps, ROAD_FLOOR)
}

/// The road's mask falling from 1 parked to `floor` at 30 m/s and above.
pub fn road_mask_to(speed_mps: f64, floor: f64) -> f64 {
    1.0 - (1.0 - floor) * (speed_mps.max(0.0) / 30.0).min(1.0)
}

/// The bed playing now and its level. A new bed waits for the old one to
/// fade all the way out, so two recordings never overlap.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct BedFader {
    pub bed: Option<Bed>,
    /// 0 to 1 of the bed's full level.
    pub level: f64,
}

impl BedFader {
    /// Move toward `want` over `dt_real_s`: fade in over `fade_in_s`, out
    /// over `fade_out_s`. A different bed fades the current one out first.
    pub fn next(self, want: Option<Bed>, dt_real_s: f64, fade_in_s: f64, fade_out_s: f64) -> Self {
        let dt = dt_real_s.max(0.0);
        let down = |level: f64| (level - dt / fade_out_s.max(1e-6)).max(0.0);
        match (self.bed, want) {
            (None, None) => BedFader::default(),
            (None, Some(bed)) => BedFader {
                bed: Some(bed),
                level: (dt / fade_in_s.max(1e-6)).min(1.0),
            },
            (Some(bed), Some(wanted)) if bed == wanted => BedFader {
                bed: Some(bed),
                level: (self.level + dt / fade_in_s.max(1e-6)).min(1.0),
            },
            (Some(bed), _) => {
                let level = down(self.level);
                if level > 0.0 {
                    BedFader {
                        bed: Some(bed),
                        level,
                    }
                } else {
                    BedFader::default()
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
