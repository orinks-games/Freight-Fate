//! Vehicles: boxes projected through the road camera, so they sit in their
//! lanes, follow the road round bends, and show their sides as they pass.
//! Each kind gets the details a driver reads from behind -- tail and brake
//! lights, signals, a trailer's doors and reflective tape, a trooper's
//! light bar.

use super::draw::{Layer, Painter, Rgba};
use super::road::{RoadFrame, RIG_LENGTH_FT};
use super::scene_drive::{Signal, TrafficView, VehicleKind};
use super::scenery::{hash, unit};

#[derive(Clone, Copy)]
pub struct BoxPaint {
    pub back: Rgba,
    pub side: Rgba,
    pub top: Rgba,
}

impl BoxPaint {
    pub fn of(c: Rgba) -> BoxPaint {
        BoxPaint {
            back: c,
            side: c.shade(0.8),
            top: c.shade(1.1),
        }
    }
}

/// The face of a box toward the camera, for placing details on it.
pub struct Face {
    /// Top-left, top-right, bottom-right, bottom-left on screen.
    pts: [(f32, f32); 4],
    pub dist: f32,
}

impl Face {
    /// A point on the face: `fx` 0 left .. 1 right, `fy` 0 top .. 1 bottom.
    pub fn at(&self, fx: f32, fy: f32) -> (f32, f32) {
        let [tl, tr, br, bl] = self.pts;
        let top = (tl.0 + (tr.0 - tl.0) * fx, tl.1 + (tr.1 - tl.1) * fx);
        let bot = (bl.0 + (br.0 - bl.0) * fx, bl.1 + (br.1 - bl.1) * fx);
        (top.0 + (bot.0 - top.0) * fy, top.1 + (bot.1 - top.1) * fy)
    }

    pub fn quad(&self, p: &mut Painter, fx0: f32, fy0: f32, fx1: f32, fy1: f32, c: Rgba) {
        p.quad_flat(
            [
                self.at(fx0, fy0),
                self.at(fx1, fy0),
                self.at(fx1, fy1),
                self.at(fx0, fy1),
            ],
            c,
        );
    }

    pub fn width_px(&self) -> f32 {
        (self.pts[1].0 - self.pts[0].0).abs()
    }

    pub fn height_px(&self) -> f32 {
        (self.pts[3].1 - self.pts[0].1).abs()
    }
}

/// Draw a box from `s0` (its back, toward the camera) to `s1`, across
/// `u0..u1`, `h0..h1` feet up; returns its back face when that is on screen.
#[allow(clippy::too_many_arguments)]
pub fn draw_box(
    p: &mut Painter,
    f: &RoadFrame,
    s0: f32,
    s1: f32,
    u0: f32,
    u1: f32,
    h0: f32,
    h1: f32,
    paint: BoxPaint,
) -> Option<Face> {
    draw_box_skewed(p, f, s0, s1, (u0, u1), (u0, u1), h0, h1, paint)
}

/// [`draw_box`] whose far end sits at a different lateral place (a rig
/// angled across its lane).
#[allow(clippy::too_many_arguments)]
pub fn draw_box_skewed(
    p: &mut Painter,
    f: &RoadFrame,
    s0: f32,
    s1: f32,
    back_u: (f32, f32),
    front_u: (f32, f32),
    h0: f32,
    h1: f32,
    paint: BoxPaint,
) -> Option<Face> {
    let cam = f.cam;
    let pr = |s: f32, u: f32, h: f32| cam.project(s, u, h).map(|(x, y, _, z)| (x, y, z));
    let (btl, btr, bbr, bbl) = (
        pr(s0, back_u.0, h1)?,
        pr(s0, back_u.1, h1)?,
        pr(s0, back_u.1, h0)?,
        pr(s0, back_u.0, h0)?,
    );
    let dist = btl.2;
    let lit = |c: Rgba| f.light.lit(c, dist);
    let xy = |v: (f32, f32, f32)| (v.0, v.1);
    let front = (
        pr(s1, front_u.0, h1),
        pr(s1, front_u.1, h1),
        pr(s1, front_u.1, h0),
        pr(s1, front_u.0, h0),
    );
    if let (Some(ftl), Some(ftr), Some(fbr), Some(fbl)) = front {
        // A side shows when the far end has moved past the near end
        // toward the middle of the view; the top when it is below the eye.
        if ftl.0 < btl.0 - 0.3 {
            p.convex(&[xy(btl), xy(ftl), xy(fbl), xy(bbl)], lit(paint.side));
        }
        if ftr.0 > btr.0 + 0.3 {
            p.convex(&[xy(btr), xy(ftr), xy(fbr), xy(bbr)], lit(paint.side));
        }
        if ftl.1 < btl.1 - 0.3 {
            p.convex(&[xy(btl), xy(btr), xy(ftr), xy(ftl)], lit(paint.top));
        }
    }
    let face = Face {
        pts: [xy(btl), xy(btr), xy(bbr), xy(bbl)],
        dist,
    };
    if face.width_px() < 0.5 && face.height_px() < 0.5 {
        return None;
    }
    p.convex(&face.pts, lit(paint.back));
    Some(face)
}

/// A soft dark patch under a vehicle: it sits on the road, not over it.
fn shadow(p: &mut Painter, f: &RoadFrame, s: f32, u: f32, len: f32, width: f32) {
    let a = f.cam.project(s - 1.0, u - width * 0.6, 0.0);
    let b = f.cam.project(s - 1.0, u + width * 0.6, 0.0);
    let c = f.cam.project(s + len, u + width * 0.6, 0.0);
    let d = f.cam.project(s + len, u - width * 0.6, 0.0);
    if let (Some(a), Some(b), Some(c), Some(d)) = (a, b, c, d) {
        let k = 0.35 * (1.0 - f.light.fog_amount(a.3));
        p.convex(
            &[(a.0, a.1), (b.0, b.1), (c.0, c.1), (d.0, d.1)],
            Rgba::new(0, 0, 0, 255).fade(k),
        );
    }
}

const CAR_PAINT: [Rgba; 10] = [
    Rgba::rgb(226, 226, 222),
    Rgba::rgb(172, 176, 180),
    Rgba::rgb(30, 32, 36),
    Rgba::rgb(98, 102, 108),
    Rgba::rgb(152, 30, 32),
    Rgba::rgb(36, 66, 124),
    Rgba::rgb(42, 72, 54),
    Rgba::rgb(192, 178, 152),
    Rgba::rgb(30, 40, 72),
    Rgba::rgb(220, 220, 214),
];

const CAB_PAINT: [Rgba; 7] = [
    Rgba::rgb(168, 26, 30),
    Rgba::rgb(28, 58, 120),
    Rgba::rgb(236, 236, 232),
    Rgba::rgb(26, 28, 32),
    Rgba::rgb(40, 96, 58),
    Rgba::rgb(222, 160, 30),
    Rgba::rgb(110, 116, 124),
];

/// Whether a blinker is lit right now (about 1.5 flashes a second).
fn blink(t: f32) -> bool {
    (t * 1.5).fract() < 0.55
}

struct Lamps {
    brake: bool,
    signal: Signal,
    reverse: bool,
    t: f32,
}

/// Tail lights on a face: two red lamps at `fy`, `size` as a fraction of
/// the face width; brake lights brighten them and throw a glow.
fn tail_lights(p: &mut Painter, f: &RoadFrame, face: &Face, fy: f32, size: f32, lamps: &Lamps) {
    let night = f.light.night;
    let red_dim = Rgba::rgb(120, 16, 16);
    let red_on = Rgba::rgb(255, 40, 30);
    let amber = Rgba::rgb(255, 160, 30);
    let lit_tail = night > 0.35 || f.light.fog_ft < 1500.0;
    for (side, fx) in [(Signal::Left, 0.04f32), (Signal::Right, 0.96 - size)] {
        let signalling = lamps.signal == side && blink(lamps.t);
        let base = if lamps.brake {
            red_on
        } else if lit_tail {
            red_on.shade(0.7)
        } else {
            red_dim
        };
        let c = if signalling { amber } else { base };
        face.quad(
            p,
            fx,
            fy,
            fx + size,
            fy + size * 0.6,
            f.light.lamp(c, face.dist),
        );
        let bright = lamps.brake || signalling || lit_tail;
        if bright {
            let (cx, cy) = face.at(fx + size * 0.5, fy + size * 0.3);
            let r = (face.width_px() * size * 1.4).max(2.0);
            let strength = if lamps.brake || signalling {
                0.75
            } else {
                0.4 * night
            };
            p.set_layer(Layer::Glow);
            p.glow(
                cx,
                cy,
                r,
                c.alpha(strength * (1.0 - f.light.fog_amount(face.dist) * 0.6)),
            );
            if f.view.weather.wet && night > 0.3 {
                // The wet road doubles the lights as streaks under them.
                p.glow_ellipse(cx, cy + r * 2.2, r * 0.5, r * 1.8, c.alpha(strength * 0.35));
            }
            p.set_layer(Layer::World);
        }
    }
    if lamps.reverse {
        face.quad(
            p,
            0.4,
            fy,
            0.6,
            fy + size * 0.5,
            f.light.lamp(Rgba::rgb(250, 250, 240), face.dist),
        );
    }
}

pub fn draw_traffic(p: &mut Painter, f: &RoadFrame, v: &TrafficView) {
    let g = &f.geom;
    let lane = v.lane.clamp(0, (g.lanes - 1).max(0));
    let u = g.lane_center(lane) + (unit(hash(v.seed)) - 0.5) * 1.2;
    let s = v.ahead_ft;
    if s < -RIG_LENGTH_FT - 45.0 {
        return; // behind the camera
    }
    let lamps = Lamps {
        brake: v.braking,
        signal: v.signal,
        reverse: false,
        t: f.t + unit(v.seed) * 0.6,
    };
    match v.kind {
        VehicleKind::Car => car(p, f, s, u, CAR_PAINT[(v.seed % 10) as usize], &lamps, None),
        VehicleKind::Trooper => {
            let paint = if v.seed.is_multiple_of(2) {
                Rgba::rgb(24, 26, 30)
            } else {
                Rgba::rgb(196, 182, 150)
            };
            car(p, f, s, u, paint, &lamps, Some(v.beacons));
        }
        VehicleKind::Service => pickup(p, f, s, u, v.seed, &lamps),
        VehicleKind::BoxTruck => box_truck(p, f, s, u, v.seed, &lamps),
        VehicleKind::Semi => {
            let cab = CAB_PAINT[(v.seed % 7) as usize];
            let trailer = match v.seed % 5 {
                0 => Rgba::rgb(150, 156, 162),
                1 => Rgba::rgb(60, 92, 150),
                _ => Rgba::rgb(232, 232, 228),
            };
            semi(p, f, s, (u, u), cab, trailer, &lamps, false);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn car(
    p: &mut Painter,
    f: &RoadFrame,
    s: f32,
    u: f32,
    paint: Rgba,
    lamps: &Lamps,
    light_bar: Option<bool>,
) {
    let half = 3.0;
    shadow(p, f, s, u, 15.0, 6.2);
    let glass = Rgba::rgb(30, 38, 48);
    // Cabin first (it is farther forward), then the body over its base.
    let cabin = draw_box(
        p,
        f,
        s + 3.6,
        s + 10.5,
        u - half + 0.5,
        u + half - 0.5,
        3.2,
        4.8,
        BoxPaint {
            back: glass,
            side: glass.mix(paint, 0.35),
            top: paint.shade(1.05),
        },
    );
    if let Some(window) = &cabin {
        // A sky reflection across the rear glass.
        window.quad(
            p,
            0.1,
            0.1,
            0.9,
            0.35,
            f.light
                .lit(Rgba::rgb(110, 124, 140), window.dist)
                .fade(0.35),
        );
    }
    if let Some(on) = light_bar {
        if let Some(bar) = draw_box(
            p,
            f,
            s + 6.0,
            s + 7.0,
            u - 2.2,
            u + 2.2,
            4.8,
            5.2,
            BoxPaint::of(Rgba::rgb(40, 40, 46)),
        ) {
            let flash = (f.t * 3.0).fract() < 0.5;
            let (red, blue) = (Rgba::rgb(255, 30, 40), Rgba::rgb(40, 90, 255));
            let (l, r) = if on {
                if flash {
                    (red, blue.shade(0.3))
                } else {
                    (red.shade(0.3), blue)
                }
            } else {
                (red.shade(0.45), blue.shade(0.45))
            };
            bar.quad(p, 0.05, 0.1, 0.48, 0.9, l);
            bar.quad(p, 0.52, 0.1, 0.95, 0.9, r);
            if on {
                p.set_layer(Layer::Glow);
                let lit_side = if flash { (0.27, red) } else { (0.73, blue) };
                let (cx, cy) = bar.at(lit_side.0, 0.5);
                p.glow(
                    cx,
                    cy,
                    (bar.width_px() * 1.6).max(6.0),
                    lit_side.1.alpha(0.8),
                );
                p.set_layer(Layer::World);
            }
        }
    }
    let Some(body) = draw_box(
        p,
        f,
        s,
        s + 15.0,
        u - half,
        u + half,
        0.9,
        3.3,
        BoxPaint::of(paint),
    ) else {
        return;
    };
    // Bumper, plate, wheels peeking out below.
    body.quad(
        p,
        0.02,
        0.78,
        0.98,
        1.0,
        f.light.lit(paint.shade(0.55), body.dist),
    );
    body.quad(
        p,
        0.4,
        0.58,
        0.6,
        0.75,
        f.light.lit(Rgba::rgb(220, 220, 210), body.dist),
    );
    let tyre = f.light.lit(Rgba::rgb(18, 18, 20), body.dist);
    let (bl, br) = (body.at(0.0, 1.0), body.at(1.0, 1.0));
    let wheel_h = body.height_px() * 0.38;
    let wheel_w = body.width_px() * 0.13;
    p.rect(bl.0 + wheel_w * 0.3, bl.1, wheel_w, wheel_h * 0.9, tyre);
    p.rect(br.0 - wheel_w * 1.3, br.1, wheel_w, wheel_h * 0.9, tyre);
    tail_lights(p, f, &body, 0.18, 0.18, lamps);
}

fn pickup(p: &mut Painter, f: &RoadFrame, s: f32, u: f32, seed: u32, lamps: &Lamps) {
    let paint = if seed.is_multiple_of(2) {
        Rgba::rgb(236, 236, 230)
    } else {
        Rgba::rgb(228, 190, 40)
    };
    shadow(p, f, s, u, 18.0, 6.6);
    draw_box(
        p,
        f,
        s + 7.0,
        s + 13.0,
        u - 3.2,
        u + 3.2,
        2.4,
        6.4,
        BoxPaint {
            back: Rgba::rgb(36, 42, 52),
            side: paint.shade(0.85),
            top: paint,
        },
    );
    if let Some(bar) = draw_box(
        p,
        f,
        s + 9.0,
        s + 10.0,
        u - 2.4,
        u + 2.4,
        6.4,
        6.9,
        BoxPaint::of(Rgba::rgb(60, 52, 30)),
    ) {
        let on = blink(f.t * 1.3);
        let amber = Rgba::rgb(255, 170, 30);
        bar.quad(
            p,
            0.05,
            0.1,
            0.95,
            0.9,
            if on { amber } else { amber.shade(0.4) },
        );
        if on {
            let (cx, cy) = bar.at(0.5, 0.5);
            p.set_layer(Layer::Glow);
            p.glow(cx, cy, (bar.width_px() * 1.2).max(5.0), amber.alpha(0.6));
            p.set_layer(Layer::World);
        }
    }
    let Some(bed) = draw_box(
        p,
        f,
        s,
        s + 7.0,
        u - 3.2,
        u + 3.2,
        1.4,
        4.2,
        BoxPaint::of(paint),
    ) else {
        return;
    };
    bed.quad(
        p,
        0.02,
        0.8,
        0.98,
        1.0,
        f.light.lit(Rgba::rgb(60, 60, 62), bed.dist),
    );
    tail_lights(p, f, &bed, 0.12, 0.14, lamps);
}

fn box_truck(p: &mut Painter, f: &RoadFrame, s: f32, u: f32, seed: u32, lamps: &Lamps) {
    let cab = CAB_PAINT[(seed % 7) as usize];
    shadow(p, f, s, u, 28.0, 8.0);
    draw_box(
        p,
        f,
        s + 20.5,
        s + 28.0,
        u - 3.8,
        u + 3.8,
        1.5,
        8.6,
        BoxPaint::of(cab),
    );
    let Some(back) = draw_box(
        p,
        f,
        s,
        s + 20.0,
        u - 4.0,
        u + 4.0,
        2.8,
        12.0,
        BoxPaint::of(Rgba::rgb(234, 234, 230)),
    ) else {
        return;
    };
    // Roll-up door slats.
    if back.height_px() > 12.0 {
        let slat = f.light.lit(Rgba::rgb(190, 190, 186), back.dist);
        for k in 1..8 {
            let y = k as f32 / 8.0 * 0.86;
            back.quad(p, 0.05, y, 0.95, y + 0.012, slat);
        }
    }
    back.quad(
        p,
        0.0,
        0.96,
        1.0,
        1.0,
        f.light.lit(Rgba::rgb(200, 40, 40), back.dist),
    );
    tail_lights(p, f, &back, 0.86, 0.09, lamps);
    wheels_under(p, f, s + 3.0, u, 4.0, 2.8);
}

/// Dual wheels under a trailer or box, seen from behind.
fn wheels_under(p: &mut Painter, f: &RoadFrame, s: f32, u: f32, half: f32, top: f32) {
    let tyre = Rgba::rgb(16, 16, 18);
    for (a, b) in [
        (u - half + 0.2, u - half + 2.4),
        (u + half - 2.4, u + half - 0.2),
    ] {
        let tl = f.cam.project(s, a, top.min(3.4));
        let br = f.cam.project(s, b, 0.0);
        if let (Some(tl), Some(br)) = (tl, br) {
            p.rect(
                tl.0,
                tl.1,
                br.0 - tl.0,
                br.1 - tl.1,
                f.light.lit(tyre, tl.3),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn semi(
    p: &mut Painter,
    f: &RoadFrame,
    s: f32,
    u: (f32, f32),
    cab: Rgba,
    trailer: Rgba,
    lamps: &Lamps,
    player: bool,
) {
    // `u` is the lateral place at the trailer's doors and at the cab front.
    let at = |k: f32| u.0 + (u.1 - u.0) * k;
    let half = 4.25;
    shadow(p, f, s, at(0.5), 72.0, 8.6);
    // Tractor ahead of the trailer: only its sides and roof fairing show.
    let cab_back = s + 54.0;
    draw_box_skewed(
        p,
        f,
        cab_back,
        s + 72.0,
        (at(0.75) - 4.0, at(0.75) + 4.0),
        (at(1.0) - 4.0, at(1.0) + 4.0),
        1.5,
        11.0,
        BoxPaint::of(cab),
    );
    let Some(doors) = draw_box_skewed(
        p,
        f,
        s,
        s + 53.0,
        (at(0.0) - half, at(0.0) + half),
        (at(0.74) - half, at(0.74) + half),
        4.0,
        13.5,
        BoxPaint {
            back: trailer,
            side: trailer.shade(0.86),
            top: trailer.shade(1.04),
        },
    ) else {
        return;
    };
    let dist = doors.dist;
    let lit = |c: Rgba| f.light.lit(c, dist);
    if doors.height_px() > 10.0 {
        // Two doors: the seam, lock rods, hinges.
        doors.quad(p, 0.497, 0.02, 0.503, 0.94, lit(trailer.shade(0.6)));
        let rod = lit(Rgba::rgb(120, 122, 126));
        for fx in [0.2f32, 0.32, 0.68, 0.8] {
            doors.quad(p, fx, 0.04, fx + 0.012, 0.93, rod);
        }
        for fy in [0.12f32, 0.48, 0.84] {
            doors.quad(p, 0.0, fy, 0.05, fy + 0.04, rod);
            doors.quad(p, 0.95, fy, 1.0, fy + 0.04, rod);
        }
        if player {
            // The game's own amber stripe across the doors.
            doors.quad(p, 0.0, 0.62, 1.0, 0.67, lit(Rgba::rgb(242, 168, 40)));
        }
    }
    // Red-and-white reflective tape along the bottom edge.
    let segments = 10;
    for k in 0..segments {
        let c = if k % 2 == 0 {
            Rgba::rgb(210, 30, 36)
        } else {
            Rgba::rgb(236, 236, 230)
        };
        let x0 = k as f32 / segments as f32;
        doors.quad(
            p,
            x0,
            0.94,
            x0 + 1.0 / segments as f32,
            1.0,
            f.light.lamp(lit(c), 0.0),
        );
    }
    // Underride bar and wheels below the deck.
    let bar0 = f.cam.project(s - 0.5, at(0.0) - half + 0.6, 2.4);
    let bar1 = f.cam.project(s - 0.5, at(0.0) + half - 0.6, 1.6);
    if let (Some(a), Some(b)) = (bar0, bar1) {
        p.rect(a.0, a.1, b.0 - a.0, b.1 - a.1, lit(Rgba::rgb(40, 40, 44)));
    }
    wheels_under(p, f, s + 4.0, at(0.06), half, 4.0);
    tail_lights(p, f, &doors, 0.8, 0.07, lamps);
    // Clearance lights along the top, lit at night.
    if f.light.night > 0.3 {
        let amber = Rgba::rgb(255, 150, 40);
        let red = Rgba::rgb(255, 40, 30);
        for fx in [0.02f32, 0.45, 0.53, 0.94] {
            let c = if player || fx < 0.1 || fx > 0.9 {
                red
            } else {
                amber
            };
            doors.quad(p, fx, 0.0, fx + 0.04, 0.02, c);
            let (cx, cy) = doors.at(fx + 0.02, 0.01);
            p.set_layer(Layer::Glow);
            p.glow(
                cx,
                cy,
                (doors.width_px() * 0.05).max(2.0),
                c.alpha(0.5 * f.light.night),
            );
            p.set_layer(Layer::World);
        }
    }
}

/// The player's rig, with its own paint, lights, and road spray.
pub fn draw_truck(p: &mut Painter, f: &RoadFrame) {
    let hud = &f.view.hud;
    let yaw = f.view.lanes.yaw.clamp(-0.35, 0.35);
    // The cab leads; the doors trail behind on the yaw.
    let front = f.truck_u;
    let back = front - yaw * RIG_LENGTH_FT;
    let lamps = Lamps {
        brake: hud.brake > 0.03,
        signal: hud.signal,
        reverse: hud.reverse,
        t: f.t,
    };
    spray(p, f, back);
    semi(
        p,
        f,
        -RIG_LENGTH_FT,
        (back, front),
        Rgba::rgb(150, 26, 32),
        Rgba::rgb(238, 238, 234),
        &lamps,
        true,
    );
    blind_spot_lights(p, f, front);
}

/// Amber warning lights at the cab's mirrors when traffic holds the lane on
/// that side, the way a truck's blind-spot monitor lights.
fn blind_spot_lights(p: &mut Painter, f: &RoadFrame, u: f32) {
    let lanes = &f.view.lanes;
    let amber = Rgba::rgb(255, 170, 30);
    for (blocked, side) in [(lanes.blocked_left, -1.0f32), (lanes.blocked_right, 1.0)] {
        if !blocked {
            continue;
        }
        let Some((x, y, b, _)) = f.cam.project(-14.0, u + side * 6.2, 9.5) else {
            continue;
        };
        let r = (1.8 * b).max(6.0);
        p.set_layer(Layer::Glow);
        p.glow(x, y, r * 2.6, amber.alpha(0.7));
        p.set_layer(Layer::World);
        let tri = [(x - r, y + r * 0.8), (x + r, y + r * 0.8), (x, y - r)];
        p.convex(&tri, amber);
        p.rect(
            x - r * 0.12,
            y - r * 0.35,
            r * 0.24,
            r * 0.65,
            Rgba::rgb(40, 24, 0),
        );
        p.circle(x, y + r * 0.52, r * 0.13, Rgba::rgb(40, 24, 0));
    }
}

/// Mist thrown up off a wet or snowy road behind the wheels at speed.
fn spray(p: &mut Painter, f: &RoadFrame, u: f32) {
    let w = &f.view.weather;
    let speed = f.view.hud.speed_mph as f32;
    if !(w.wet || w.snow_cover) || speed < 20.0 {
        return;
    }
    let strength = ((speed - 20.0) / 40.0).clamp(0.0, 1.0);
    let c = if w.snow_cover {
        Rgba::rgb(240, 242, 246)
    } else {
        Rgba::rgb(210, 214, 220)
    };
    for k in 0..14u32 {
        let seed = hash(k.wrapping_mul(977));
        let phase = (f.t * (1.6 + unit(seed))).fract();
        let s = -RIG_LENGTH_FT - 2.0 - phase * 22.0;
        let side = if k % 2 == 0 { -3.4 } else { 3.4 };
        let lift = 1.0 + phase * 6.0;
        if let Some((x, y, b, _)) = f
            .cam
            .project(s, u + side + (unit(seed >> 4) - 0.5) * 3.0, lift)
        {
            let r = (2.5 + phase * 6.0) * b;
            p.glow(x, y, r, c.alpha(0.22 * strength * (1.0 - phase)));
        }
    }
}
