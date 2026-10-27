//! Light and colour: the sky through the day, how weather greys it, how
//! distance fades things into the haze, and what the land looks like where
//! the truck is.

use ff_core::sim::weather::WeatherKind;

use super::draw::Rgba;
use super::scene_drive::{Biome, Season, WeatherLook};

/// Lighting for one frame.
#[derive(Clone, Copy, Debug)]
pub struct Light {
    pub zenith: Rgba,
    pub horizon: Rgba,
    /// Multiplies every surface colour (dim and blue at night, warm at dusk).
    pub ambient: Rgba,
    /// What distance fades toward.
    pub fog: Rgba,
    /// Distance at which fog has taken about two thirds of a colour.
    pub fog_ft: f32,
    /// 0 full day .. 1 full night.
    pub night: f32,
    /// 0 clear .. 1 solid overcast.
    pub overcast: f32,
    /// The sun's height, radians; below zero it has set.
    pub sun_elev: f32,
    /// The sun's compass bearing, radians (0 north, clockwise).
    pub sun_bearing: f32,
}

/// (hour, zenith, horizon, ambient, night)
type Key = (f32, (u8, u8, u8), (u8, u8, u8), (f32, f32, f32), f32);

const KEYS: [Key; 12] = [
    (0.0, (6, 9, 22), (18, 24, 44), (0.20, 0.23, 0.34), 1.0),
    (4.8, (8, 12, 28), (26, 30, 52), (0.22, 0.25, 0.36), 1.0),
    (5.8, (36, 52, 98), (226, 138, 92), (0.55, 0.48, 0.50), 0.55),
    (
        6.8,
        (58, 104, 178),
        (236, 186, 140),
        (0.86, 0.80, 0.74),
        0.15,
    ),
    (
        8.5,
        (56, 118, 204),
        (170, 204, 234),
        (1.00, 0.98, 0.95),
        0.0,
    ),
    (
        12.5,
        (44, 108, 206),
        (166, 204, 238),
        (1.04, 1.03, 1.00),
        0.0,
    ),
    (
        16.5,
        (58, 114, 196),
        (188, 208, 228),
        (1.00, 0.97, 0.92),
        0.0,
    ),
    (
        18.4,
        (64, 92, 160),
        (240, 170, 110),
        (0.90, 0.76, 0.62),
        0.1,
    ),
    (
        19.4,
        (40, 48, 104),
        (238, 120, 72),
        (0.62, 0.50, 0.50),
        0.45,
    ),
    (20.4, (16, 20, 52), (78, 54, 84), (0.34, 0.33, 0.44), 0.85),
    (21.5, (6, 9, 22), (18, 24, 44), (0.20, 0.23, 0.34), 1.0),
    (24.0, (6, 9, 22), (18, 24, 44), (0.20, 0.23, 0.34), 1.0),
];

fn rgb(c: (u8, u8, u8)) -> Rgba {
    Rgba::rgb(c.0, c.1, c.2)
}

fn overcast_of(kind: WeatherKind) -> f32 {
    match kind {
        WeatherKind::Clear => 0.0,
        WeatherKind::Wind => 0.15,
        WeatherKind::Cloudy => 0.6,
        WeatherKind::Ice => 0.7,
        WeatherKind::Fog | WeatherKind::Rain => 0.82,
        WeatherKind::Snow => 0.88,
        WeatherKind::HeavyRain => 0.95,
        WeatherKind::Thunderstorm => 1.0,
    }
}

/// How far the air itself lets you see before weather is counted.
fn haze_ft(kind: WeatherKind) -> f32 {
    match kind {
        WeatherKind::Clear => 14000.0,
        WeatherKind::Wind => 9000.0,
        WeatherKind::Cloudy => 10000.0,
        WeatherKind::Ice => 6000.0,
        WeatherKind::Rain => 4200.0,
        WeatherKind::Snow => 2200.0,
        WeatherKind::HeavyRain => 2400.0,
        WeatherKind::Thunderstorm => 2200.0,
        WeatherKind::Fog => 900.0,
    }
}

pub fn light(w: &WeatherLook) -> Light {
    let hour = w.hour.rem_euclid(24.0);
    let i = KEYS
        .iter()
        .rposition(|k| k.0 <= hour)
        .unwrap_or(0)
        .min(KEYS.len() - 2);
    let (a, b) = (KEYS[i], KEYS[i + 1]);
    let t = ((hour - a.0) / (b.0 - a.0).max(1e-3)).clamp(0.0, 1.0);
    let t = t * t * (3.0 - 2.0 * t);
    let mut zenith = rgb(a.1).mix(rgb(b.1), t);
    let mut horizon = rgb(a.2).mix(rgb(b.2), t);
    let amb = (
        a.3 .0 + (b.3 .0 - a.3 .0) * t,
        a.3 .1 + (b.3 .1 - a.3 .1) * t,
        a.3 .2 + (b.3 .2 - a.3 .2) * t,
    );
    let night = a.4 + (b.4 - a.4) * t;
    let mut ambient = Rgba {
        r: amb.0,
        g: amb.1,
        b: amb.2,
        a: 1.0,
    };
    let overcast = overcast_of(w.kind);
    if overcast > 0.0 {
        // Cloud cover greys the sky and flattens the light; a storm darkens it.
        let lum = 0.30 * ambient.r + 0.55 * ambient.g + 0.15 * ambient.b;
        let storm = if w.kind == WeatherKind::Thunderstorm {
            0.62
        } else {
            1.0
        };
        let grey_top = Rgba::rgb(120, 128, 140).shade(lum * storm);
        let grey_low = Rgba::rgb(170, 176, 184).shade(lum * storm);
        zenith = zenith.mix(grey_top, overcast);
        horizon = horizon.mix(grey_low, overcast);
        let flat = Rgba {
            r: lum,
            g: lum,
            b: lum * 1.04,
            a: 1.0,
        };
        ambient = ambient
            .mix(flat, overcast * 0.7)
            .shade(1.0 - overcast * 0.25 * (2.0 - storm));
    }
    if w.kind == WeatherKind::Fog {
        let lum = 0.30 * ambient.r + 0.55 * ambient.g + 0.15 * ambient.b;
        let fog = Rgba::rgb(196, 198, 202).shade(lum.min(1.0));
        zenith = zenith.mix(fog, 0.75);
        horizon = horizon.mix(fog, 0.92);
    }
    if w.snow_cover && night < 0.5 {
        // Snow on the ground throws light back up.
        ambient = ambient.shade(1.06);
    }
    let fog_ft = haze_ft(w.kind)
        .min(w.visibility_ft.max(200.0) * 0.45)
        .max(200.0);
    // The sun: up at six, down at six, south at noon; its bearing on the
    // screen depends on which way the road runs.
    let day = ((hour - 6.0) / 12.0).clamp(-0.5, 1.5);
    let sun_elev = (day * std::f32::consts::PI).sin() * 1.05;
    let sun_bearing = (90.0 + (hour - 6.0) * 15.0).to_radians();
    Light {
        zenith,
        horizon,
        ambient,
        fog: horizon,
        fog_ft,
        night,
        overcast,
        sun_elev,
        sun_bearing,
    }
}

impl Light {
    pub fn fog_amount(&self, dist_ft: f32) -> f32 {
        1.0 - (-dist_ft.max(0.0) / self.fog_ft).exp()
    }

    /// A surface colour as seen at `dist_ft`: lit, then faded into the haze.
    pub fn lit(&self, base: Rgba, dist_ft: f32) -> Rgba {
        base.tint(self.ambient)
            .mix(self.fog.alpha(base.a), self.fog_amount(dist_ft))
    }

    /// A light source (lamps, tail lights): not darkened by night, only
    /// swallowed by fog.
    pub fn lamp(&self, base: Rgba, dist_ft: f32) -> Rgba {
        base.fade(1.0 - self.fog_amount(dist_ft) * 0.85)
    }
}

pub struct Land {
    pub grass: Rgba,
    pub grass_alt: Rgba,
    pub tree: Rgba,
    pub tree_dark: Rgba,
    pub trunk: Rgba,
    pub far_ridge: Rgba,
    pub near_ridge: Rgba,
}

pub fn land(w: &WeatherLook) -> Land {
    let (grass, tree): ((u8, u8, u8), (u8, u8, u8)) = match (w.biome, w.season) {
        (Biome::Desert, _) => ((184, 152, 110), (112, 122, 84)),
        (Biome::Steppe, Season::Winter) => ((168, 160, 134), (78, 92, 66)),
        (Biome::Steppe, _) => ((160, 152, 112), (82, 100, 68)),
        (Biome::Plains, Season::Winter) => ((150, 136, 98), (92, 84, 64)),
        (Biome::Plains, Season::Autumn) => ((160, 142, 88), (128, 104, 56)),
        (Biome::Plains, _) => ((128, 138, 72), (70, 98, 48)),
        (Biome::Conifer, Season::Winter) => ((108, 112, 92), (40, 66, 48)),
        (Biome::Conifer, _) => ((84, 110, 62), (36, 70, 46)),
        (Biome::Woodland, Season::Winter) => ((118, 110, 86), (88, 78, 64)),
        (Biome::Woodland, Season::Autumn) => ((112, 112, 60), (176, 104, 40)),
        (Biome::Woodland, Season::Spring) => ((92, 136, 60), (84, 132, 56)),
        (Biome::Woodland, Season::Summer) => ((76, 116, 52), (52, 96, 44)),
    };
    let mut grass = rgb(grass);
    let mut tree = rgb(tree);
    if w.snow_cover {
        grass = Rgba::rgb(228, 232, 238);
        tree = tree.mix(Rgba::rgb(220, 226, 232), 0.35);
    }
    let ridge = match w.biome {
        Biome::Desert => Rgba::rgb(170, 120, 96),
        Biome::Steppe => Rgba::rgb(138, 126, 120),
        Biome::Conifer => Rgba::rgb(78, 96, 110),
        _ => Rgba::rgb(96, 116, 110),
    };
    Land {
        grass,
        grass_alt: grass.shade(0.975),
        tree,
        tree_dark: tree.shade(0.72),
        trunk: Rgba::rgb(84, 64, 46),
        far_ridge: ridge.mix(Rgba::rgb(150, 170, 196), 0.45),
        near_ridge: ridge,
    }
}

pub const ASPHALT: Rgba = Rgba::rgb(66, 67, 70);
pub const ASPHALT_WET: Rgba = Rgba::rgb(42, 44, 48);
pub const SHOULDER: Rgba = Rgba::rgb(92, 91, 88);
pub const CONCRETE: Rgba = Rgba::rgb(150, 148, 142);
pub const LINE_WHITE: Rgba = Rgba::rgb(236, 236, 230);
pub const LINE_YELLOW: Rgba = Rgba::rgb(232, 178, 40);
/// The game's accent: the amber of a cab's marker lights.
pub const ACCENT: Rgba = Rgba::rgb(255, 190, 70);

#[cfg(test)]
mod tests {
    use super::super::scene_drive::Terrain;
    use super::*;

    fn look(hour: f32, kind: WeatherKind) -> WeatherLook {
        WeatherLook {
            kind,
            visibility_ft: 52800.0,
            wind: 0.0,
            wet: false,
            snow_cover: false,
            hour,
            season: Season::Summer,
            biome: Biome::Woodland,
            terrain: Terrain::Flat,
            heading: None,
        }
    }

    fn lum(c: Rgba) -> f32 {
        0.3 * c.r + 0.55 * c.g + 0.15 * c.b
    }

    #[test]
    fn noon_is_brighter_than_midnight_and_dusk_sits_between() {
        let noon = light(&look(12.0, WeatherKind::Clear));
        let dusk = light(&look(19.8, WeatherKind::Clear));
        let night = light(&look(0.5, WeatherKind::Clear));
        assert!(lum(noon.ambient) > lum(dusk.ambient));
        assert!(lum(dusk.ambient) > lum(night.ambient));
        assert!(night.night > 0.9 && noon.night < 0.05);
        assert!(noon.sun_elev > 0.9 && night.sun_elev < 0.0);
    }

    #[test]
    fn weather_closes_the_view_in() {
        let clear = light(&look(12.0, WeatherKind::Clear));
        let rain = light(&look(12.0, WeatherKind::HeavyRain));
        let mut foggy = look(12.0, WeatherKind::Fog);
        foggy.visibility_ft = 600.0;
        let fog = light(&foggy);
        assert!(rain.fog_ft < clear.fog_ft);
        assert!(fog.fog_ft < rain.fog_ft);
        assert!(
            fog.fog_amount(1000.0) > 0.9,
            "past the visibility, fog has it"
        );
        assert!(rain.overcast > clear.overcast);
    }
}
