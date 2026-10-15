//! The sky and the weather in it: the day's colours, the sun and moon
//! where they really are relative to the road's heading, stars, cloud,
//! the distant land on the horizon, and rain, snow and lightning.

use ff_core::sim::weather::WeatherKind;

use super::draw::{Layer, Painter, Rgba};
use super::palette::{Land, Light};
use super::scene_drive::{Biome, Terrain, WeatherLook};
use super::scenery::{hash, unit};

pub struct SkyFrame<'a> {
    pub w: f32,
    pub h: f32,
    pub horizon: f32,
    pub focal: f32,
    /// Screen height down to which the land must reach (the far road).
    pub land_top: f32,
    /// Compass bearing the camera looks along, radians.
    pub heading: f32,
    pub weather: &'a WeatherLook,
    pub light: &'a Light,
    pub land: &'a Land,
    pub t: f32,
    /// How fast the truck is moving, for rain and snow streaming past.
    pub speed_mph: f32,
}

fn wrap(a: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    (a + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI
}

impl SkyFrame<'_> {
    /// Screen x of a compass bearing, or None outside the view.
    fn x_of(&self, bearing: f32) -> Option<f32> {
        let rel = wrap(bearing - self.heading);
        (rel.abs() < 1.2).then(|| self.w * 0.5 + rel.tan() * self.focal)
    }

    fn y_of(&self, elevation: f32) -> f32 {
        self.horizon - elevation.tan() * self.focal
    }
}

pub fn draw_sky(p: &mut Painter, f: &SkyFrame) {
    let light = f.light;
    let mid = light.zenith.mix(light.horizon, 0.55);
    p.rect_vgrad(0.0, 0.0, f.w, f.horizon * 0.55, light.zenith, mid);
    p.rect_vgrad(
        0.0,
        f.horizon * 0.55,
        f.w,
        f.horizon * 0.45 + 1.0,
        mid,
        light.horizon,
    );
    p.rect(0.0, f.horizon, f.w, f.h - f.horizon, light.horizon);
    stars(p, f);
    sun_and_moon(p, f);
    clouds(p, f);
    ridges(p, f);
}

fn stars(p: &mut Painter, f: &SkyFrame) {
    let k = f.light.night * (1.0 - f.light.overcast);
    if k < 0.15 {
        return;
    }
    for i in 0..160u32 {
        let h = hash(i.wrapping_mul(0x9e37_79b9) ^ 0x51ed);
        let bearing = unit(h) * std::f32::consts::TAU;
        let elev = 0.06 + unit(hash(h)).powf(0.7) * 1.2;
        let rel = wrap(bearing - f.heading);
        if rel.abs() > 1.0 {
            continue;
        }
        let x = f.w * 0.5 + rel * f.focal;
        let y = f.y_of(elev.min(1.3));
        if y < 0.0 || y > f.horizon - 4.0 {
            continue;
        }
        let twinkle = 0.65 + 0.35 * (f.t * (1.0 + unit(h >> 7) * 2.0) + unit(h >> 3) * 6.0).sin();
        let size = 0.6 + unit(h >> 11) * 1.1;
        p.circle(
            x,
            y,
            size,
            Rgba::rgb(235, 238, 255).alpha(k * twinkle * 0.9),
        );
    }
}

fn sun_and_moon(p: &mut Painter, f: &SkyFrame) {
    let light = f.light;
    let clear = 1.0 - light.overcast * 0.9;
    if light.sun_elev > -0.12 {
        if let Some(x) = f.x_of(light.sun_bearing) {
            let y = f.y_of(light.sun_elev.max(-0.05));
            let low = (1.0 - light.sun_elev / 0.6).clamp(0.0, 1.0);
            let core = Rgba::rgb(255, 250, 230).mix(Rgba::rgb(255, 170, 90), low);
            let r = f.h * 0.028;
            p.glow(x, y, r * 9.0, core.alpha(0.28 * clear));
            p.glow(x, y, r * 3.5, core.alpha(0.45 * clear));
            p.circle(x, y, r, core.alpha(clear.max(0.15)));
        }
    }
    if light.night > 0.3 && light.overcast < 0.75 {
        let bearing = light.sun_bearing + std::f32::consts::PI * 0.92;
        let elev = (-light.sun_elev * 0.7).clamp(0.12, 0.9);
        if let Some(x) = f.x_of(bearing) {
            let y = f.y_of(elev);
            let r = f.h * 0.022;
            let a = light.night * (1.0 - light.overcast);
            p.glow(x, y, r * 5.0, Rgba::rgb(200, 210, 240).alpha(0.18 * a));
            p.circle(x, y, r, Rgba::rgb(236, 236, 226).alpha(a));
            p.circle(
                x - r * 0.3,
                y - r * 0.2,
                r * 0.22,
                Rgba::rgb(200, 200, 192).alpha(a * 0.6),
            );
            p.circle(
                x + r * 0.35,
                y + r * 0.25,
                r * 0.16,
                Rgba::rgb(204, 204, 196).alpha(a * 0.5),
            );
        }
    }
}

fn clouds(p: &mut Painter, f: &SkyFrame) {
    let light = f.light;
    let w = f.weather;
    let count = match w.kind {
        WeatherKind::Clear => 5,
        WeatherKind::Wind => 8,
        WeatherKind::Fog => 0,
        _ => 16,
    };
    let lit_top = Rgba::rgb(250, 250, 252)
        .tint(light.ambient)
        .mix(light.horizon, 0.25);
    let storm = matches!(w.kind, WeatherKind::Thunderstorm | WeatherKind::HeavyRain);
    let base_tone = if storm {
        Rgba::rgb(70, 74, 84)
    } else if light.overcast > 0.5 {
        Rgba::rgb(150, 154, 162)
    } else {
        Rgba::rgb(214, 216, 222)
    };
    let under = base_tone.tint(light.ambient).mix(light.horizon, 0.2);
    let drift = f.t * 0.004;
    for i in 0..count {
        let h = hash(i as u32 * 7919 + 13);
        let bearing = unit(h) * std::f32::consts::TAU + drift;
        let elev = 0.04 + unit(hash(h)) * 0.32;
        let rel = wrap(bearing - f.heading);
        if rel.abs() > 1.15 {
            continue;
        }
        let x = f.w * 0.5 + rel.tan() * f.focal;
        let y = f.y_of(elev);
        let size = (0.05 + unit(h >> 9) * 0.07) * f.w * (1.0 - elev);
        let puffs = 5 + (h % 3) as i32;
        for k in 0..puffs {
            let hk = hash(h ^ (k as u32 * 31 + 1));
            let ox = (k as f32 / (puffs - 1) as f32 - 0.5) * size * 2.0;
            let oy =
                -unit(hk) * size * 0.35 * (1.0 - (2.0 * k as f32 / (puffs - 1) as f32 - 1.0).abs());
            let r = size * (0.32 + unit(hk >> 5) * 0.25);
            p.ellipse(x + ox, y + oy, r * 1.25, r, under.alpha(0.9));
            p.ellipse(
                x + ox,
                y + oy - r * 0.25,
                r * 1.1,
                r * 0.75,
                lit_top.mix(under, light.overcast * 0.8).alpha(0.85),
            );
        }
    }
    if light.overcast > 0.55 {
        // Low cloud deck in long bands.
        let deck = under.shade(0.92);
        for i in 0..6 {
            let y = f.horizon * (0.12 + i as f32 * 0.14);
            let shift = ((f.heading * 0.7 + drift) * f.w * 0.3 + i as f32 * 170.0) % (f.w * 0.6);
            for k in -1..3 {
                let x = k as f32 * f.w * 0.6 + shift;
                p.ellipse(
                    x,
                    y,
                    f.w * 0.42,
                    f.horizon * 0.07,
                    deck.alpha(0.28 * light.overcast),
                );
            }
        }
    }
}

/// Height of the skyline at a bearing, in radians of elevation.
fn skyline(bearing: f32, seed: u32, rough: f32) -> f32 {
    let mut v = 0.0;
    let mut amp = 1.0;
    let mut total = 0.0;
    for (i, freq) in [3.0f32, 7.0, 13.0, 29.0, 61.0].into_iter().enumerate() {
        let phase = unit(hash(seed.wrapping_add(i as u32 * 101))) * std::f32::consts::TAU;
        let s = (bearing * freq + phase).sin();
        // Ridged for the rough octaves: sharp peaks instead of humps.
        v += amp
            * if rough > 0.5 && i > 0 {
                1.0 - s.abs() * 2.0
            } else {
                s
            };
        total += amp;
        amp *= 0.5;
    }
    (v / total * 0.5 + 0.5).clamp(0.0, 1.0)
}

fn ridges(p: &mut Painter, f: &SkyFrame) {
    let w = f.weather;
    let light = f.light;
    let (far_deg, near_deg, rough) = match (w.terrain, w.biome) {
        (Terrain::Mountains, _) => (7.5f32, 3.6f32, 1.0f32),
        (Terrain::Hills, _) => (2.6, 1.6, 0.0),
        (Terrain::Flat, Biome::Desert) => (2.2, 0.8, 1.0),
        // Basin and range: mountains always somewhere on the horizon.
        (Terrain::Flat, Biome::Steppe) => (3.2, 1.0, 1.0),
        (Terrain::Flat, Biome::Plains) => (0.5, 0.35, 0.0),
        (Terrain::Flat, _) => (0.9, 0.7, 0.0),
    };
    let base = f.land_top.max(f.horizon) + 2.0;
    let step = (f.w / 120.0).max(4.0);
    let layers = [
        (far_deg, f.land.far_ridge, 0.62f32, 17u32),
        (near_deg, f.land.near_ridge, 0.38f32, 29u32),
    ];
    let fog_far = light.fog_amount(30000.0);
    for (deg, color, haze, seed) in layers {
        let c = color
            .tint(light.ambient)
            .mix(light.horizon, haze.max(fog_far));
        let snow_cap = w.terrain == Terrain::Mountains && deg > 5.0;
        let mut x = -step;
        let mut prev: Option<(f32, f32)> = None;
        while x <= f.w + step {
            let bearing = f.heading + ((x - f.w * 0.5) / f.focal).atan();
            let mut e = skyline(bearing, seed, rough);
            if w.biome == Biome::Desert && w.terrain == Terrain::Flat {
                // Mesas: flat tops and steep sides.
                e = if e > 0.55 { 0.9 } else { e * 0.4 };
            }
            let y = f.horizon - (deg.to_radians() * e).tan() * f.focal;
            if let Some((px, py)) = prev {
                p.quad_flat([(px, py), (x, y), (x, base), (px, base)], c);
                if snow_cap {
                    let line = f.horizon - (deg.to_radians() * 0.72).tan() * f.focal;
                    if py < line || y < line {
                        let snow = Rgba::rgb(240, 242, 248)
                            .tint(light.ambient)
                            .mix(light.horizon, haze);
                        p.quad_flat(
                            [
                                (px, py),
                                (x, y),
                                (x, y.max(line).min(base)),
                                (px, py.max(line).min(base)),
                            ],
                            snow,
                        );
                    }
                }
            }
            prev = Some((x, y));
            x += step;
        }
    }
}

/// Rain, snow and fog over the finished scene, and the lightning flash.
pub fn draw_weather(p: &mut Painter, f: &SkyFrame) {
    let w = f.weather;
    let light = f.light;
    match w.kind {
        WeatherKind::Rain | WeatherKind::HeavyRain | WeatherKind::Thunderstorm => rain(p, f),
        WeatherKind::Snow => snow(p, f),
        _ => {}
    }
    if w.kind == WeatherKind::Fog || light.fog_ft < 1500.0 {
        // Haze hangs thickest at eye level.
        let k = (1.0 - light.fog_ft / 1500.0).clamp(0.2, 0.7);
        p.rect_vgrad(
            0.0,
            0.0,
            f.w,
            f.horizon,
            light.fog.alpha(0.0),
            light.fog.alpha(k * 0.6),
        );
        p.rect_vgrad(
            0.0,
            f.horizon,
            f.w,
            f.h * 0.35,
            light.fog.alpha(k * 0.6),
            light.fog.alpha(0.0),
        );
    }
    if w.kind == WeatherKind::Thunderstorm {
        let window = (f.t / 7.0).floor();
        let h = hash(window as u32 ^ 0xbeef);
        let at = 1.0 + unit(h) * 4.5;
        let since = f.t - window * 7.0 - at;
        if (0.0..0.45).contains(&since) {
            let flicker = if since < 0.08 || (0.16..0.24).contains(&since) {
                1.0
            } else {
                0.35
            };
            let k = (1.0 - since / 0.45) * flicker;
            p.set_layer(Layer::Glow);
            p.rect(0.0, 0.0, f.w, f.h, Rgba::rgb(200, 210, 255).alpha(0.32 * k));
            p.set_layer(Layer::World);
            let mut x = f.w * (0.2 + unit(h >> 8) * 0.6);
            let mut y = 0.0;
            let mut pts = vec![(x, y)];
            let mut seed = h;
            while y < f.horizon * 0.95 {
                seed = hash(seed);
                x += (unit(seed) - 0.5) * f.w * 0.05;
                y += f.horizon * (0.06 + unit(seed >> 5) * 0.08);
                pts.push((x, y));
            }
            p.polyline(&pts, 2.0, Rgba::rgb(240, 244, 255).alpha(k), false);
        }
    }
}

fn rain(p: &mut Painter, f: &SkyFrame) {
    let w = f.weather;
    let count = match w.kind {
        WeatherKind::Rain => 130u32,
        WeatherKind::HeavyRain => 260,
        _ => 300,
    };
    let scale = f.h / 640.0;
    let speed = f.speed_mph.max(0.0);
    let color = Rgba::rgb(196, 206, 222)
        .tint(f.light.ambient)
        .mix(Rgba::rgb(170, 180, 196), 0.4);
    let span = f.h * 1.2;
    for i in 0..count {
        let h = hash(i.wrapping_mul(2654435761));
        let x0 = unit(h) * f.w;
        let fall = 900.0 * scale * (0.8 + unit(h >> 4) * 0.4);
        let y = (unit(hash(h)) * span + f.t * fall).rem_euclid(span) - f.h * 0.1;
        // Driving into rain: drops stream outward from the road ahead.
        let out = (x0 - f.w * 0.5) / f.w * speed * 0.012;
        let slant = 0.12 + w.wind * 0.45 + out;
        let len = (16.0 + unit(h >> 9) * 16.0) * scale * (1.0 + speed / 80.0);
        let x = x0 + y * out * 0.4;
        p.line(
            (x, y),
            (x + slant * len, y + len),
            1.1 * scale.max(1.0),
            color.alpha(0.18 + unit(h >> 13) * 0.2),
        );
    }
}

fn snow(p: &mut Painter, f: &SkyFrame) {
    let scale = f.h / 640.0;
    let speed = f.speed_mph.max(0.0);
    let streaming = (speed / 45.0).clamp(0.0, 1.0);
    let vx = f.w * 0.5;
    let vy = f.horizon;
    let white = Rgba::rgb(246, 248, 252);
    for i in 0..240u32 {
        let h = hash(i.wrapping_mul(0x85eb_ca6b) ^ 0x1234);
        // Falling flakes, drifting with the wind...
        let fall = 60.0 * scale * (0.6 + unit(h >> 4) * 0.8);
        let fy = (unit(hash(h)) * f.h * 1.1 + f.t * fall).rem_euclid(f.h * 1.1) - f.h * 0.05;
        let sway = (f.t * (0.8 + unit(h >> 6)) + unit(h) * 6.0).sin() * 8.0 * scale;
        let fx = (unit(h) * f.w + sway + f.t * f.weather.wind * 40.0 * scale).rem_euclid(f.w);
        // ...and, at speed, rushing out of the road ahead toward the glass.
        let angle = unit(h >> 2) * std::f32::consts::TAU;
        let r = (unit(hash(h ^ 77)) + f.t * (0.12 + speed * 0.004)).fract();
        let reach = r * r * f.w * 0.75;
        let sx = vx + angle.cos() * reach;
        let sy = vy + angle.sin() * reach * 0.75;
        let x = fx + (sx - fx) * streaming;
        let y = fy + (sy - fy) * streaming;
        // Far flakes (near the vanishing point) are small and faint.
        let near = 1.0 - streaming * (1.0 - (r / 0.35).clamp(0.0, 1.0));
        let size = (0.8 + unit(h >> 10) * 1.4) * scale * (1.0 + streaming * r * 1.6);
        p.circle(
            x,
            y,
            size * (0.4 + 0.6 * near),
            white.alpha((0.5 + unit(h >> 12) * 0.35) * near),
        );
    }
}
