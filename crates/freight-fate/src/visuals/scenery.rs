//! What stands beside the road: trees, brush, rock, power lines, reflector
//! posts, and in town buildings and street lights.
//!
//! None of it is map data -- the world does not record where each tree
//! is -- so it is decoration, but deterministic decoration: each forty-foot
//! stretch of the route hashes to the same props every time it is driven,
//! and the mix follows the landscape the route really runs through.

use super::draw::{Layer, Painter, Rgba};
use super::road::{Geom, RoadFrame};
use super::scene_drive::{Biome, DriveView, Season, Terrain};
use super::vehicles::{draw_box, BoxPaint};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PropKind {
    Broadleaf,
    Conifer,
    Shrub,
    Cactus,
    Rock,
    Pole,
    Delineator,
    Building,
    StreetLight,
}

#[derive(Clone, Copy, Debug)]
pub struct Prop {
    pub s: f32,
    pub u: f32,
    pub kind: PropKind,
    /// Height in feet (width for rock and shrub).
    pub size: f32,
    pub seed: u32,
}

/// A small, fast, well-mixed hash (from the "lowbias32" family).
pub fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^= x >> 16;
    x
}

pub fn unit(x: u32) -> f32 {
    (x & 0xffff) as f32 / 65535.0
}

const CELL_FT: f32 = 40.0;
const PROPS_TO_FT: f32 = 3400.0;

pub fn props(view: &DriveView, g: &Geom) -> Vec<Prop> {
    let w = &view.weather;
    let lanes = &view.lanes;
    let base = view.position_ft;
    let first = ((base + 8.0) / CELL_FT as f64).floor() as i64;
    let last = ((base + PROPS_TO_FT as f64) / CELL_FT as f64).floor() as i64;
    let mut out = Vec::new();
    let right = g.right_outer() + 3.0;
    let left = g.left_outer() - 3.0;
    for cell in first..=last {
        let cell_s = (cell as f64 * CELL_FT as f64 - base) as f32;
        let id = cell as u32;
        // Reflector posts every tenth of a mile, on the shoulder's edge.
        if !lanes.street && cell.rem_euclid(13) == 0 && cell_s < 1600.0 {
            out.push(Prop {
                s: cell_s,
                u: g.right_outer() + 1.0,
                kind: PropKind::Delineator,
                size: 4.0,
                seed: id,
            });
        }
        if lanes.urban {
            urban_cell(&mut out, cell, cell_s, g, right, left);
            continue;
        }
        // Power line on the right, poles every five cells.
        let poles =
            matches!(w.biome, Biome::Plains | Biome::Woodland | Biome::Steppe) && !lanes.ramp;
        if poles && cell.rem_euclid(5) == 0 {
            out.push(Prop {
                s: cell_s,
                u: right + 46.0,
                kind: PropKind::Pole,
                size: 32.0,
                seed: id,
            });
        }
        // Woods come in stands with clearings between: a slow hash over
        // blocks of cells sets how thick each stretch is.
        let grove = unit(hash((cell.div_euclid(9) as u32).wrapping_mul(0x9e37)));
        for side in [1i32, -1] {
            let h = hash(
                id.wrapping_mul(2654435761)
                    .wrapping_add((side as u32).wrapping_mul(0x5bd1)),
            );
            let roll = unit(h);
            let back = 10.0 + unit(hash(h)) * 150.0;
            let u = if side > 0 { right + back } else { left - back };
            if side > 0 && poles && (u - (right + 46.0)).abs() < 8.0 {
                continue;
            }
            let jitter = unit(hash(h ^ 0xabcd)) * CELL_FT;
            let s = cell_s + jitter;
            if !(4.0..PROPS_TO_FT).contains(&s) {
                continue;
            }
            let near = s < 1800.0;
            let tree_size = 22.0 + unit(hash(h ^ 0x77)) * 30.0;
            let kind = match w.biome {
                Biome::Woodland => {
                    if roll < 0.15 + grove * 0.6 {
                        Some((PropKind::Broadleaf, tree_size))
                    } else if roll < 0.82 && near {
                        Some((PropKind::Shrub, 4.0 + unit(h >> 3) * 5.0))
                    } else {
                        None
                    }
                }
                Biome::Conifer => {
                    if roll < 0.2 + grove * 0.65 {
                        Some((PropKind::Conifer, tree_size * 1.25))
                    } else if roll < 0.9 && near && w.terrain == Terrain::Mountains {
                        Some((PropKind::Rock, 4.0 + unit(h >> 5) * 10.0))
                    } else {
                        None
                    }
                }
                Biome::Plains => {
                    if roll < 0.04 + grove * 0.1 {
                        Some((PropKind::Broadleaf, tree_size * 0.8))
                    } else if roll < 0.3 && near {
                        Some((PropKind::Shrub, 2.5 + unit(h >> 3) * 3.0))
                    } else {
                        None
                    }
                }
                Biome::Steppe => {
                    if roll < 0.02 + grove * 0.05 {
                        // A juniper now and then.
                        Some((PropKind::Conifer, 9.0 + unit(h >> 2) * 9.0))
                    } else if roll < 0.08 && near {
                        Some((PropKind::Rock, 3.0 + unit(h >> 5) * 8.0))
                    } else if roll < 0.62 && near {
                        Some((PropKind::Shrub, 2.0 + unit(h >> 3) * 2.5))
                    } else {
                        None
                    }
                }
                Biome::Desert => {
                    if roll < 0.035 {
                        Some((PropKind::Cactus, 14.0 + unit(h >> 2) * 18.0))
                    } else if roll < 0.12 && near {
                        Some((PropKind::Rock, 3.0 + unit(h >> 5) * 9.0))
                    } else if roll < 0.55 && near {
                        Some((PropKind::Shrub, 2.0 + unit(h >> 3) * 2.5))
                    } else {
                        None
                    }
                }
            };
            if let Some((kind, size)) = kind {
                out.push(Prop {
                    s,
                    u,
                    kind,
                    size,
                    seed: h,
                });
            }
        }
    }
    out
}

fn urban_cell(out: &mut Vec<Prop>, cell: i64, cell_s: f32, g: &Geom, right: f32, left: f32) {
    let id = cell as u32;
    if cell.rem_euclid(4) == 0 {
        out.push(Prop {
            s: cell_s,
            u: g.right_outer() + 2.0,
            kind: PropKind::StreetLight,
            size: 30.0,
            seed: id,
        });
    }
    if cell.rem_euclid(5) == 0 {
        for side in [1i32, -1] {
            let h = hash(id.wrapping_mul(0x2545_f491).wrapping_add(side as u32));
            if unit(h) < 0.25 {
                continue; // a gap: a lot, a cross street
            }
            let setback = if g.street { 12.0 } else { 40.0 } + unit(hash(h)) * 50.0;
            let u = if side > 0 {
                right + setback
            } else {
                left - setback
            };
            out.push(Prop {
                s: cell_s + 6.0,
                u,
                kind: PropKind::Building,
                size: 22.0 + unit(hash(h ^ 0x1f)).powi(2) * 110.0,
                seed: h,
            });
        }
    }
}

pub fn draw(p: &mut Painter, f: &RoadFrame, prop: &Prop) {
    let Some((x, y, b, z)) = f.cam.project(prop.s, prop.u, 0.0) else {
        return;
    };
    let px = prop.size * b;
    if px < 0.8 || x < -px * 3.0 - 200.0 || x > f.cam.w + px * 3.0 + 200.0 {
        return;
    }
    let light = f.light;
    let land = f.land;
    let lit = |c: Rgba| light.lit(c, z);
    let w = &f.view.weather;
    let snow = w.snow_cover;
    let sun_side = if light.sun_elev > 0.0 { 1.0 } else { 0.0 };
    match prop.kind {
        PropKind::Broadleaf => {
            let hgt = px;
            let trunk_w = (hgt * 0.06).max(1.0);
            p.rect(
                x - trunk_w * 0.5,
                y - hgt * 0.45,
                trunk_w,
                hgt * 0.45,
                lit(land.trunk),
            );
            if light.night < 0.6 && px > 6.0 {
                p.ellipse(x, y, hgt * 0.32, hgt * 0.05, Rgba::new(0, 0, 0, 40));
            }
            let bare = w.season == Season::Winter && w.biome != Biome::Conifer;
            let (dark, base) = if bare {
                (Rgba::rgb(92, 80, 70), Rgba::rgb(118, 104, 92))
            } else {
                (land.tree_dark, land.tree)
            };
            let alpha = if bare { 0.72 } else { 1.0 };
            let r = hgt * 0.3;
            let wobble = (unit(prop.seed) - 0.5) * r * 0.4;
            p.ellipse(
                x - r * 0.45,
                y - hgt * 0.55,
                r * 0.8,
                r * 0.75,
                lit(dark).fade(alpha),
            );
            p.ellipse(
                x + r * 0.42 + wobble,
                y - hgt * 0.6,
                r * 0.78,
                r * 0.72,
                lit(dark).fade(alpha),
            );
            p.ellipse(
                x + wobble * 0.5,
                y - hgt * 0.76,
                r * 0.85,
                r * 0.8,
                lit(base).fade(alpha),
            );
            if sun_side > 0.0 && !bare {
                p.ellipse(
                    x + r * 0.25 + wobble * 0.5,
                    y - hgt * 0.82,
                    r * 0.45,
                    r * 0.38,
                    lit(base.shade(1.18)).fade(0.8),
                );
            }
            if snow {
                p.ellipse(
                    x,
                    y - hgt * 0.9,
                    r * 0.6,
                    r * 0.25,
                    lit(Rgba::rgb(236, 240, 244)),
                );
            }
        }
        PropKind::Conifer => {
            let hgt = px;
            let trunk_w = (hgt * 0.05).max(1.0);
            p.rect(
                x - trunk_w * 0.5,
                y - hgt * 0.18,
                trunk_w,
                hgt * 0.18,
                lit(land.trunk),
            );
            let tiers = [(0.08, 0.30, 0.55), (0.32, 0.24, 0.80), (0.55, 0.17, 1.0)];
            for (k, (bottom, half_w, top)) in tiers.into_iter().enumerate() {
                let c = if k == 2 { land.tree } else { land.tree_dark };
                let pts = [
                    (x - hgt * half_w, y - hgt * bottom),
                    (x + hgt * half_w, y - hgt * bottom),
                    (x, y - hgt * top),
                ];
                p.convex(&pts, lit(c));
                if snow {
                    let cap = [
                        (
                            x - hgt * half_w * 0.35,
                            y - hgt * (top - (top - bottom) * 0.35),
                        ),
                        (
                            x + hgt * half_w * 0.35,
                            y - hgt * (top - (top - bottom) * 0.35),
                        ),
                        (x, y - hgt * top),
                    ];
                    p.convex(&cap, lit(Rgba::rgb(236, 240, 244)));
                }
            }
        }
        PropKind::Shrub => {
            let r = px * 0.5;
            let c = if snow {
                Rgba::rgb(200, 206, 210)
            } else if w.biome == Biome::Desert {
                Rgba::rgb(138, 142, 104)
            } else if w.biome == Biome::Steppe {
                Rgba::rgb(146, 152, 124) // sage
            } else {
                land.tree_dark
            };
            p.ellipse(x, y - r * 0.55, r * 1.2, r * 0.7, lit(c));
            p.ellipse(
                x + r * 0.4,
                y - r * 0.75,
                r * 0.7,
                r * 0.5,
                lit(c.shade(1.1)),
            );
        }
        PropKind::Cactus => {
            let hgt = px;
            let wd = (hgt * 0.11).max(1.0);
            let green = lit(Rgba::rgb(78, 110, 62));
            p.round_rect(x - wd * 0.5, y - hgt, wd, hgt, wd * 0.5, green);
            if hgt > 10.0 {
                let arm = |p: &mut Painter, dir: f32, at: f32, len: f32| {
                    let ax = x + dir * wd * 0.5;
                    p.round_rect(
                        ax.min(ax + dir * wd * 1.4),
                        y - hgt * at - wd * 0.5,
                        wd * 1.4,
                        wd,
                        wd * 0.4,
                        green,
                    );
                    let up_x = ax + dir * wd * 1.4 - if dir > 0.0 { wd } else { 0.0 };
                    p.round_rect(up_x, y - hgt * at - len, wd, len, wd * 0.5, green);
                };
                arm(p, -1.0, 0.45, hgt * 0.28);
                if prop.seed.is_multiple_of(2) {
                    arm(p, 1.0, 0.58, hgt * 0.22);
                }
            }
        }
        PropKind::Rock => {
            let r = px * 0.5;
            let c = if w.biome == Biome::Desert {
                Rgba::rgb(150, 108, 82)
            } else {
                Rgba::rgb(118, 116, 112)
            };
            let k = unit(prop.seed);
            let pts = [
                (x - r, y),
                (x - r * 0.8, y - r * (0.5 + k * 0.3)),
                (x - r * 0.1, y - r * 0.85),
                (x + r * 0.7, y - r * (0.55 + k * 0.2)),
                (x + r, y),
            ];
            p.convex(&pts, lit(c));
            if snow {
                p.ellipse(
                    x - r * 0.1,
                    y - r * 0.75,
                    r * 0.5,
                    r * 0.15,
                    lit(Rgba::rgb(236, 240, 244)),
                );
            }
        }
        PropKind::Pole => {
            let hgt = px;
            let wd = (0.9 * b).max(1.0);
            let wood = lit(Rgba::rgb(96, 74, 52));
            p.rect(x - wd * 0.5, y - hgt, wd, hgt, wood);
            p.rect(
                x - hgt * 0.12,
                y - hgt * 0.92,
                hgt * 0.24,
                (0.6 * b).max(1.0),
                wood,
            );
            // Wires back to the pole before this one.
            if let Some((x2, y2, b2, _)) = f.cam.project(prop.s + CELL_FT * 5.0, prop.u, 0.0) {
                let top2 = y2 - prop.size * b2 * 0.92;
                let wire = lit(Rgba::rgb(40, 40, 44)).fade(0.8);
                for k in [-0.1f32, 0.1] {
                    let (ax, ay) = (x + hgt * k, y - hgt * 0.92);
                    let (bx, by) = (x2 + prop.size * b2 * k, top2);
                    let sag = (ay - by).abs() * 0.08 + 2.0;
                    let mid = ((ax + bx) * 0.5, (ay + by) * 0.5 + sag);
                    p.polyline(&[(ax, ay), mid, (bx, by)], 1.0, wire, false);
                }
            }
        }
        PropKind::Delineator => {
            let hgt = px;
            let wd = (0.35 * b).max(1.0);
            p.rect(
                x - wd * 0.5,
                y - hgt,
                wd,
                hgt,
                lit(Rgba::rgb(230, 230, 226)),
            );
            let refl = Rgba::rgb(255, 200, 80);
            p.rect(
                x - wd * 0.5,
                y - hgt * 0.95,
                wd,
                hgt * 0.15,
                light.lamp(refl, z).fade(0.9),
            );
            if light.night > 0.5 && z < 600.0 {
                p.set_layer(Layer::Glow);
                p.glow(
                    x,
                    y - hgt * 0.88,
                    (hgt * 0.4).max(2.0),
                    refl.alpha(0.5 * light.night),
                );
                p.set_layer(Layer::World);
            }
        }
        PropKind::Building => building(p, f, prop),
        PropKind::StreetLight => {
            let hgt = px;
            let wd = (0.6 * b).max(1.0);
            let steel = lit(Rgba::rgb(120, 124, 128));
            p.rect(x - wd * 0.5, y - hgt, wd, hgt, steel);
            let arm_to = f.cam.project(prop.s, prop.u - 8.0, prop.size);
            if let Some((ax, ay, _, _)) = arm_to {
                p.line((x, y - hgt), (ax, ay), wd, steel);
                p.round_rect(ax - 1.2 * b, ay - 0.2 * b, 2.4 * b, 0.8 * b, 0.3 * b, steel);
                if light.night > 0.3 {
                    let lamp = Rgba::rgb(255, 214, 150);
                    p.rect(
                        ax - 1.0 * b,
                        ay + 0.4 * b,
                        2.0 * b,
                        (0.3 * b).max(1.0),
                        light.lamp(lamp, z),
                    );
                    p.set_layer(Layer::Glow);
                    p.glow(
                        ax,
                        ay + 0.6 * b,
                        (6.0 * b).max(3.0),
                        lamp.alpha(0.55 * light.night),
                    );
                    if let Some((gx, gy, gb, _)) = f.cam.project(prop.s, prop.u - 8.0, 0.0) {
                        p.glow_ellipse(gx, gy, 16.0 * gb, 5.0 * gb, lamp.alpha(0.18 * light.night));
                    }
                    p.set_layer(Layer::World);
                }
            }
        }
    }
}

fn building(p: &mut Painter, f: &RoadFrame, prop: &Prop) {
    let h = prop.seed;
    let width = 40.0 + unit(hash(h ^ 3)) * 80.0;
    let depth = 40.0 + unit(hash(h ^ 5)) * 60.0;
    let (u0, u1) = if prop.u > 0.0 {
        (prop.u, prop.u + width)
    } else {
        (prop.u - width, prop.u)
    };
    let palette = [
        Rgba::rgb(168, 150, 128),
        Rgba::rgb(140, 92, 72),
        Rgba::rgb(120, 130, 140),
        Rgba::rgb(190, 186, 176),
        Rgba::rgb(96, 104, 118),
        Rgba::rgb(160, 120, 92),
    ];
    let wall = palette[(h % palette.len() as u32) as usize];
    let paint = BoxPaint {
        back: wall,
        side: wall.shade(0.82),
        top: wall.shade(0.7),
    };
    let faces = draw_box(p, f, prop.s, prop.s + depth, u0, u1, 0.0, prop.size, paint);
    // Windows on the face toward the camera: rows of glass, lit at night.
    let Some(face) = faces else {
        return;
    };
    let night = f.light.night;
    let floors = (prop.size / 11.0).floor().max(1.0) as i32;
    let cols = (width / 12.0).floor().max(1.0) as i32;
    let px_floor = face.height_px() / floors as f32;
    if px_floor < 3.0 {
        return;
    }
    for fl in 0..floors {
        for col in 0..cols {
            let hh = hash(h ^ (fl as u32 * 131 + col as u32 * 7919));
            let lit_window = night > 0.4 && unit(hh) < 0.45;
            let c = if lit_window {
                Rgba::rgb(255, 210, 140)
            } else {
                f.light.lit(Rgba::rgb(60, 72, 88), face.dist)
            };
            let fx0 = (col as f32 + 0.25) / cols as f32;
            let fx1 = (col as f32 + 0.75) / cols as f32;
            let fy0 = (fl as f32 + 0.3) / floors as f32;
            let fy1 = (fl as f32 + 0.75) / floors as f32;
            face.quad(p, fx0, fy0, fx1, fy1, c);
        }
    }
}

/// A work-zone drum: orange with white reflective bands.
pub fn draw_drum(p: &mut Painter, f: &RoadFrame, s: f32, u: f32) {
    let Some((x, y, b, z)) = f.cam.project(s, u, 0.0) else {
        return;
    };
    let (hgt, wd) = (3.4 * b, 2.0 * b);
    if hgt < 1.0 {
        return;
    }
    let light = f.light;
    p.ellipse(
        x,
        y,
        wd * 0.75,
        wd * 0.2,
        light.lit(Rgba::rgb(28, 28, 30), z),
    );
    p.round_rect(
        x - wd * 0.5,
        y - hgt,
        wd,
        hgt * 0.94,
        wd * 0.25,
        light.lit(Rgba::rgb(238, 104, 26), z),
    );
    let band = Rgba::rgb(242, 242, 238);
    for k in [0.2f32, 0.5] {
        p.rect(
            x - wd * 0.5,
            y - hgt * (1.0 - k),
            wd,
            hgt * 0.12,
            light.lit(band, z),
        );
    }
    if light.night > 0.4 && z < 700.0 {
        p.set_layer(Layer::Glow);
        p.glow(x, y - hgt * 0.65, wd, band.alpha(0.25 * light.night));
        p.set_layer(Layer::World);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_hash_spreads_neighbouring_cells() {
        let a = unit(hash(1000));
        let b = unit(hash(1001));
        assert!((a - b).abs() > 0.01);
        assert_eq!(hash(42), hash(42));
    }
}
