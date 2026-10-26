//! The road view: a chase camera riding behind and above the trailer, and
//! the road, its markings and everything beside it seen through it.
//!
//! The road's centreline is integrated from the drive's curvature and grade
//! samples, so a bend on the map is a bend on screen and a climb is a climb.
//! It is drawn as bands from the farthest to the nearest, each band painting
//! over what lies behind it, which is all a crest needs to hide the road
//! beyond it. Things beside the road are drawn with the band they stand in.

use super::draw::{Painter, Rgba};
use super::palette::{self, Land, Light};
use super::scene_drive::{
    DriveView, LaneLook, RoadAhead, SignView, TrafficView, ROAD_SAMPLES, ROAD_START_FT,
    ROAD_STEP_FT,
};
use super::scenery::{self, Prop};
use super::signs;
use super::text::Text;
use super::vehicles;

/// Cab front to trailer doors, feet (a 53-foot trailer behind a day cab).
pub const RIG_LENGTH_FT: f32 = 72.0;
const CAM_BACK_FT: f32 = 85.0;
const CAM_HEIGHT_FT: f32 = 40.0;
const NEAR_FT: f32 = 6.0;
const MEDIAN_FT: f32 = 46.0;

/// Carried from frame to frame: the camera eases after the truck instead of
/// being bolted to it, so a lane change reads as the truck moving.
#[derive(Default)]
pub struct RoadState {
    cam_u: Option<f32>,
    cam_yaw: f32,
}

/// Where things are across the road, feet; the right edge line is 0 and
/// the lanes run leftward (negative).
#[derive(Clone, Copy, Debug)]
pub struct Geom {
    pub lane_w: f32,
    pub lanes: i32,
    pub divided: bool,
    pub shoulder_r: f32,
    pub shoulder_l: f32,
    pub median: f32,
    pub street: bool,
    pub ramp: bool,
}

impl Geom {
    pub fn new(l: &LaneLook) -> Geom {
        let median = if l.street || l.ramp {
            0.0
        } else if l.urban {
            6.0
        } else {
            MEDIAN_FT
        };
        Geom {
            lane_w: l.lane_width_ft,
            lanes: l.count.max(1),
            divided: l.divided && !l.street,
            shoulder_r: l.shoulder_right_ft,
            shoulder_l: l.shoulder_left_ft,
            median,
            street: l.street,
            ramp: l.ramp,
        }
    }

    pub fn lane_center(&self, lane: i32) -> f32 {
        -(lane as f32 + 0.5) * self.lane_w
    }

    /// Left edge of the travel lanes.
    pub fn left_edge(&self) -> f32 {
        -(self.lanes as f32) * self.lane_w
    }

    /// The outer edge of the pavement on the right.
    pub fn right_outer(&self) -> f32 {
        self.shoulder_r
    }

    /// The far side's outermost pavement (oncoming lanes and shoulders).
    pub fn left_outer(&self) -> f32 {
        let ours = self.left_edge() - self.shoulder_l;
        if self.ramp {
            ours
        } else if self.divided {
            ours - self.median - self.shoulder_l - self.lanes as f32 * self.lane_w - self.shoulder_r
        } else {
            self.left_edge() * 2.0 - self.shoulder_r
        }
    }
}

/// The projection: for each road sample, screen x is `a + u * b`, screen y
/// of the road surface is `sy`, and something `h` feet up is `b * h`
/// higher. `b` is pixels per foot at that distance.
pub struct Camera {
    pub w: f32,
    pub horizon: f32,
    pub focal: f32,
    proj: Vec<(f32, f32, f32, f32)>,
    first: usize,
}

impl Camera {
    pub fn new(
        road: &RoadAhead,
        truck_u: f32,
        truck_yaw: f32,
        state: &mut RoadState,
        size: (f32, f32),
        dt: f32,
    ) -> Camera {
        let (w, h) = size;
        let n = ROAD_SAMPLES;
        let step = ROAD_STEP_FT;
        // Integrate heading and elevation from the samples.
        let mut xs = vec![0.0f32; n];
        let mut ys = vec![0.0f32; n];
        let mut heading = 0.0f32;
        for i in 1..n {
            let k = road.curvature.get(i - 1).copied().unwrap_or(0.0);
            let g = road.grade.get(i - 1).copied().unwrap_or(0.0);
            xs[i] = xs[i - 1] + heading * step + 0.5 * k * step * step;
            heading += k * step;
            ys[i] = ys[i - 1] + g * step;
        }
        // Re-reference to the truck: the road there points straight ahead
        // and is level, so its own grade does not tilt the whole view.
        let at = |s: f32| ((s - ROAD_START_FT) / step).clamp(0.0, (n - 1) as f32);
        let lerp = |v: &[f32], s: f32| {
            let f = at(s);
            let i = (f.floor() as usize).min(n - 2);
            let t = f - i as f32;
            v[i] + (v[i + 1] - v[i]) * t
        };
        let x0 = lerp(&xs, 0.0);
        let h0 = (lerp(&xs, step) - lerp(&xs, -step)) / (2.0 * step);
        let y0 = lerp(&ys, 0.0);
        let g0 = (lerp(&ys, 0.0) - lerp(&ys, -RIG_LENGTH_FT)) / RIG_LENGTH_FT;
        for i in 0..n {
            let s = RoadAhead::sample_s(i);
            xs[i] -= x0 + h0 * s;
            ys[i] -= y0 + g0 * s;
        }
        let cam_s = -RIG_LENGTH_FT - CAM_BACK_FT;
        // The camera trails the truck's lateral position by a fraction of a
        // second, and aims a little ahead of the cab.
        let follow = 1.0 - (-dt.max(0.0) * 4.0).exp();
        let cam_u = match state.cam_u {
            Some(u) if (u - truck_u).abs() < 40.0 => u + (truck_u - u) * follow,
            _ => truck_u,
        };
        state.cam_u = Some(cam_u);
        let look_s = 160.0;
        let cam_x = lerp(&xs, cam_s) + cam_u;
        let look_x = lerp(&xs, look_s) + truck_u;
        let aim = (look_x - cam_x) / (look_s - cam_s) + truck_yaw * 0.25;
        state.cam_yaw += (aim - state.cam_yaw) * (1.0 - (-dt.max(0.0) * 3.0).exp());
        if !state.cam_yaw.is_finite() || dt <= 0.0 {
            state.cam_yaw = aim;
        }
        let yaw = state.cam_yaw;
        let cam_y = lerp(&ys, cam_s) + CAM_HEIGHT_FT;
        // Looking a little down on the rig from high behind it, with a
        // longer lens: the road ahead gets the screen, not the sky.
        let horizon = h * 0.2;
        let focal = h * 1.17;
        let cx = w * 0.5;
        let mut proj = Vec::with_capacity(n);
        let mut first = n;
        for i in 0..n {
            let s = RoadAhead::sample_s(i);
            let z = s - cam_s;
            if z < NEAR_FT {
                proj.push((cx, 0.0, h * 4.0, z));
                continue;
            }
            first = first.min(i);
            let b = focal / z;
            let a = cx + (xs[i] - cam_x - yaw * z) * b;
            let sy = horizon - (ys[i] - cam_y) * b;
            proj.push((a, b, sy, z));
        }
        Camera {
            w,
            horizon,
            focal,
            proj,
            first: first.min(n - 1),
        }
    }

    fn at(&self, s: f32) -> Option<(f32, f32, f32, f32)> {
        let f = (s - ROAD_START_FT) / ROAD_STEP_FT;
        if f < self.first as f32 || f > (self.proj.len() - 1) as f32 {
            return None;
        }
        let i = (f.floor() as usize).min(self.proj.len() - 2);
        let t = f - i as f32;
        let (a0, b0, y0, z0) = self.proj[i];
        let (a1, b1, y1, z1) = self.proj[i + 1];
        Some((
            a0 + (a1 - a0) * t,
            b0 + (b1 - b0) * t,
            y0 + (y1 - y0) * t,
            z0 + (z1 - z0) * t,
        ))
    }

    /// Screen point of road position `s` ahead, `u` across, `h` up; with the
    /// pixels-per-foot scale and the distance from the camera.
    pub fn project(&self, s: f32, u: f32, h: f32) -> Option<(f32, f32, f32, f32)> {
        let (a, b, sy, z) = self.at(s)?;
        Some((a + u * b, sy - h * b, b, z))
    }

    /// The farthest road sample's screen height: the top of the land.
    pub fn far_y(&self) -> f32 {
        self.proj.last().map_or(self.horizon, |p| p.2)
    }
}

/// Things drawn in depth order with the road.
enum Obj<'a> {
    Prop(Prop),
    Sign(&'a SignView),
    Vehicle(&'a TrafficView),
    Truck,
    Drum { s: f32, u: f32 },
}

pub struct RoadFrame<'a> {
    pub view: &'a DriveView,
    pub light: &'a Light,
    pub land: &'a Land,
    pub cam: &'a Camera,
    pub geom: Geom,
    pub truck_u: f32,
    /// Seconds since the display started, for anything that blinks.
    pub t: f32,
}

pub fn truck_u(view: &DriveView, geom: &Geom) -> f32 {
    geom.lane_center(view.lanes.truck_lane) + view.lanes.offset_ft
}

pub fn draw(p: &mut Painter, text: &mut Text, f: &RoadFrame) {
    let view = f.view;
    let cam = f.cam;
    let g = &f.geom;
    // Everything beside and on the road, far to near.
    let mut objs: Vec<(f32, Obj)> = Vec::new();
    for prop in scenery::props(view, g) {
        objs.push((prop.s, Obj::Prop(prop)));
    }
    for sign in &view.signs {
        objs.push((sign.ahead_ft, Obj::Sign(sign)));
    }
    for vehicle in &view.traffic {
        objs.push((vehicle.ahead_ft, Obj::Vehicle(vehicle)));
    }
    for zone in &view.zones {
        let Some(lane) = zone.closed_lane else {
            continue;
        };
        if lane < 0 || lane >= g.lanes {
            continue;
        }
        // Drums down the closed lane's inside line, every 50 feet.
        let edge = if lane == 0 {
            g.lane_center(0) - g.lane_w * 0.45
        } else {
            g.lane_center(lane) + g.lane_w * 0.45
        };
        let anchor = view.position_ft as f32 % 50.0;
        let mut s = (zone.start_ft.max(0.0) / 50.0).ceil() * 50.0 - anchor;
        while s < zone.end_ft.min(3600.0) {
            objs.push((s, Obj::Drum { s, u: edge }));
            s += 50.0;
        }
    }
    objs.push((-RIG_LENGTH_FT, Obj::Truck));
    objs.sort_by(|a, b| b.0.total_cmp(&a.0));

    let mut next_obj = 0;
    let n = cam.proj.len();
    // Skip whatever is hidden in fog entirely.
    let mut last = n - 1;
    while last > cam.first + 1 && f.light.fog_amount(cam.proj[last].3) > 0.995 {
        last -= 1;
    }
    // Objects past the last band are lost in the haze.
    let last_s = RoadAhead::sample_s(last);
    while next_obj < objs.len() && objs[next_obj].0 > last_s {
        next_obj += 1;
    }
    for i in (cam.first..last).rev() {
        let s_near = RoadAhead::sample_s(i);
        band(p, f, i);
        while next_obj < objs.len() && objs[next_obj].0 >= s_near {
            draw_obj(p, text, f, &objs[next_obj].1);
            next_obj += 1;
        }
    }
    while next_obj < objs.len() {
        draw_obj(p, text, f, &objs[next_obj].1);
        next_obj += 1;
    }
}

/// How strongly the truck's headlights show, 0 in daylight.
fn headlight_strength(light: &Light) -> f32 {
    let night = light
        .night
        .max(if light.fog_ft < 1500.0 { 0.4 } else { 0.0 });
    if night < 0.2 {
        0.0
    } else {
        night
    }
}

fn draw_obj(p: &mut Painter, text: &mut Text, f: &RoadFrame, obj: &Obj) {
    match obj {
        Obj::Prop(prop) => scenery::draw(p, f, prop),
        Obj::Sign(sign) => signs::draw(p, text, f, sign),
        Obj::Vehicle(v) => vehicles::draw_traffic(p, f, v),
        Obj::Truck => vehicles::draw_truck(p, f),
        Obj::Drum { s, u } => scenery::draw_drum(p, f, *s, *u),
    }
}

/// One band of road between sample `i` and `i + 1`.
fn band(p: &mut Painter, f: &RoadFrame, i: usize) {
    let cam = f.cam;
    let (a0, b0, y0, z0) = cam.proj[i];
    let (a1, b1, y1, z1) = cam.proj[i + 1];
    if y1 >= y0 - 0.01 {
        return; // seen edge-on or from behind a crest
    }
    let view = f.view;
    let g = &f.geom;
    let light = f.light;
    let land = f.land;
    let s0 = RoadAhead::sample_s(i);
    let world = view.position_ft as f32 + s0;
    let dist = (z0 + z1) * 0.5;
    let lit = |c: Rgba| light.lit(c, dist);
    // Ground across the whole screen, in long faint stripes that slide by.
    let stripe = ((world / 72.0).floor() as i64).rem_euclid(2) == 0;
    let ground = if stripe { land.grass } else { land.grass_alt };
    // Overlapping its neighbours by a pixel: the bands are often thinner
    // than a pixel, and a renderer that rounds them can leave a hairline.
    p.rect(0.0, y1, cam.w, y0 - y1 + 1.5, lit(ground));
    let strip = |p: &mut Painter, u0: f32, u1: f32, c: Rgba| {
        p.quad_flat(
            [
                (a1 + u0 * b1, y1),
                (a1 + u1 * b1, y1),
                (a0 + u1 * b0, y0 + 1.0),
                (a0 + u0 * b0, y0 + 1.0),
            ],
            c,
        );
    };
    let w = &view.weather;
    let mut asphalt = if w.wet {
        palette::ASPHALT_WET
    } else {
        palette::ASPHALT
    };
    if w.wet {
        // A wet road mirrors the sky toward the distance.
        asphalt = asphalt.mix(light.horizon, (dist / 2600.0).clamp(0.0, 0.45));
    }
    if w.snow_cover {
        asphalt = asphalt.mix(Rgba::rgb(214, 218, 224), 0.72);
    }
    let shoulder = if w.snow_cover {
        Rgba::rgb(226, 230, 236)
    } else if g.street {
        palette::CONCRETE
    } else {
        palette::SHOULDER
    };
    let left = g.left_edge();
    // Gravel verge, then shoulders and lanes on our side.
    let verge = lit(Rgba::rgb(120, 112, 98).mix(land.grass, 0.4));
    strip(p, g.right_outer(), g.right_outer() + 3.0, verge);
    strip(p, 0.0, g.shoulder_r, lit(shoulder));
    strip(p, left - g.shoulder_l, left, lit(shoulder));
    strip(p, left, 0.0, lit(asphalt));
    // Wheel paths: worn lighter, or dark tracks through snow.
    let track = if w.snow_cover {
        lit(palette::ASPHALT_WET).fade(0.55)
    } else {
        lit(Rgba::rgb(90, 90, 92)).fade(0.25)
    };
    for lane in 0..g.lanes {
        let c = g.lane_center(lane);
        for side in [-3.3f32, 3.3] {
            strip(p, c + side - 1.0, c + side + 1.0, track);
        }
    }
    if g.ramp {
        // Nothing beside a ramp but grass.
    } else if g.divided {
        let far_right = left - g.shoulder_l - g.median;
        let far_left = far_right - g.shoulder_r - g.lanes as f32 * g.lane_w;
        if g.median < 10.0 {
            // A concrete barrier down the middle of a city freeway.
            strip(p, far_right, left - g.shoulder_l, lit(shoulder));
        } else {
            strip(p, left - g.shoulder_l - 2.5, left - g.shoulder_l, verge);
            strip(p, far_right, far_right + 2.5, verge);
        }
        strip(p, far_left - g.shoulder_l, far_right, lit(shoulder));
        strip(p, far_left, far_right - g.shoulder_r, lit(asphalt));
    } else {
        let far = left * 2.0;
        strip(p, far - g.shoulder_r, far, lit(shoulder));
        strip(p, far, left, lit(asphalt));
    }
    if g.street {
        // Curbs and sidewalks.
        let walk = lit(Rgba::rgb(172, 170, 164));
        strip(p, g.shoulder_r, g.shoulder_r + 8.0, walk);
        let far = g.left_outer();
        strip(p, far - 8.0, far, walk);
    }
    // Markings. Lines thinner than a pixel fade instead of shimmering.
    let line = |p: &mut Painter, u: f32, width: f32, c: Rgba| {
        let px = width * b0;
        let (half, c) = if px < 1.2 {
            (0.6 / b0, c.fade(px / 1.2))
        } else {
            (width * 0.5, c)
        };
        strip(p, u - half, u + half, c);
    };
    let white = light.lit(palette::LINE_WHITE, dist);
    let yellow = light.lit(palette::LINE_YELLOW, dist);
    let marking_fade = if w.snow_cover { 0.25 } else { 1.0 };
    let white = white.fade(marking_fade);
    let yellow = yellow.fade(marking_fade);
    if !g.street {
        line(p, 0.0, 0.5, white);
    }
    if g.divided || g.ramp {
        line(p, left, 0.5, yellow);
    } else {
        line(p, left - 0.45, 0.4, yellow);
        line(p, left + 0.45, 0.4, yellow);
    }
    // The headlight pool on this stretch of road ahead of the cab, painted
    // with the band so a car or a crest in front of it hides it.
    let beams = headlight_strength(light);
    if beams > 0.0 && s0 > 0.0 && s0 < 420.0 {
        let fade = |s: f32| (1.0 - s / 420.0).clamp(0.0, 1.0).powf(1.4) * 0.34 * beams;
        let spread = |s: f32| 5.0 + s * 0.04;
        let u = f.truck_u;
        let s1 = s0 + ROAD_STEP_FT;
        let warm = Rgba::rgb(255, 238, 205);
        let (c0, c1) = (warm.alpha(fade(s0)), warm.alpha(fade(s1)));
        let (h0, h1) = (spread(s0), spread(s1));
        p.quad(
            [
                (a1 + (u - h1) * b1, y1),
                (a1 + (u + h1) * b1, y1),
                (a0 + (u + h0) * b0, y0 + 0.5),
                (a0 + (u - h0) * b0, y0 + 0.5),
            ],
            [c1, c1, c0, c0],
        );
    }
    // Dashes: ten feet painted in every forty, fixed to the pavement.
    let s1 = s0 + ROAD_STEP_FT;
    let phase = (view.position_ft as f32).rem_euclid(40.0);
    let mut d = ((s0 + phase) / 40.0).floor() * 40.0 - phase;
    while d < s1 {
        let (lo, hi) = (d.max(s0), (d + 10.0).min(s1));
        if hi > lo {
            let t0 = (lo - s0) / ROAD_STEP_FT;
            let t1 = (hi - s0) / ROAD_STEP_FT;
            let ya = y0 + (y1 - y0) * t0;
            let yb = y0 + (y1 - y0) * t1;
            let (aa, ab) = (a0 + (a1 - a0) * t0, a0 + (a1 - a0) * t1);
            let (ba, bb) = (b0 + (b1 - b0) * t0, b0 + (b1 - b0) * t1);
            let px = 0.5 * ba;
            let c = if px < 1.0 { white.fade(px) } else { white };
            let half = 0.25f32.max(0.5 / ba);
            for lane in 1..g.lanes {
                let u = -(lane as f32) * g.lane_w;
                p.quad_flat(
                    [
                        (ab + (u - half) * bb, yb),
                        (ab + (u + half) * bb, yb),
                        (aa + (u + half) * ba, ya),
                        (aa + (u - half) * ba, ya),
                    ],
                    c,
                );
            }
            if !g.divided && !g.ramp {
                // The oncoming side's lane lines.
                for lane in 1..g.lanes {
                    let u = left - (lane as f32) * g.lane_w;
                    p.quad_flat(
                        [
                            (ab + (u - half) * bb, yb),
                            (ab + (u + half) * bb, yb),
                            (aa + (u + half) * ba, ya),
                            (aa + (u - half) * ba, ya),
                        ],
                        c,
                    );
                }
            }
        }
        d += 40.0;
    }
}
