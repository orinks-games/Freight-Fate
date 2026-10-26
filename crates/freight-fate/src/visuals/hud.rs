//! The dashboard over the road view: the instruments a driver glances at,
//! the posted limit, the next bend and the next turn, the traffic around
//! the rig, and how far there is to go.

use ff_core::pyfmt::round_py_int;
use ff_core::units::MILES_TO_KM;

use super::draw::{Layer, Painter, Rgba};
use super::palette::ACCENT;
use super::scene_drive::{DriveView, Signal, VehicleKind};
use super::text::{Text, Weight};

pub const PANEL: Rgba = Rgba::new(14, 16, 22, 222);
pub const PANEL_EDGE: Rgba = Rgba::new(255, 255, 255, 26);
pub const INK: Rgba = Rgba::rgb(236, 238, 242);
pub const DIM: Rgba = Rgba::rgb(150, 156, 168);
const RED: Rgba = Rgba::rgb(240, 64, 52);
const GREEN: Rgba = Rgba::rgb(86, 214, 120);
const CYAN: Rgba = Rgba::rgb(90, 200, 240);
const AMBER: Rgba = Rgba::rgb(255, 176, 40);

/// Layout units: 1.0 at the 900 x 640 window the game opens in.
pub fn unit_of(w: f32, h: f32) -> f32 {
    (h / 640.0).min(w / 900.0).max(0.5)
}

/// Where the cluster sits, so captions can stack above it.
pub fn cluster_top(w: f32, h: f32) -> f32 {
    let u = unit_of(w, h);
    h - 10.0 * u - 136.0 * u
}

/// The lane line's text size and the column it has, in layout units.
pub(super) const LANE_LINE_TEXT: f32 = 10.0;
pub(super) const LANE_LINE_WIDTH: f32 = 190.0;

/// The lane line under the telltales, a whole line for each lane. The
/// spoken lane names are shaped for sentences ("carril de la mitad"), so
/// the drawn line never fills one in; anything but the middle, left or
/// right lane of a multi-lane road reads as the single lane.
pub(super) fn lane_line(text: &Text, lane_name: &str, lane_assist: &str) -> String {
    use ff_core::sim::lane::lane_label;
    let is = |index, count| lane_name.eq_ignore_ascii_case(lane_label(index, count));
    if is(1, 3) {
        text.tr_drawn(&format!("Middle lane  ·  lane keeping {lane_assist}"))
    } else if is(1, 2) {
        text.tr_drawn(&format!("Left lane  ·  lane keeping {lane_assist}"))
    } else if is(0, 2) {
        text.tr_drawn(&format!("Right lane  ·  lane keeping {lane_assist}"))
    } else {
        text.tr_drawn(&format!("Single lane  ·  lane keeping {lane_assist}"))
    }
}

/// The next bend's advisory and severity, in the player's units: the number
/// is the one the spoken warning gives, rounded the same way. Each band is
/// its own label, so a catalog translates the severity word along with the
/// rest instead of filling in the English key.
pub(super) fn curve_label(
    text: &Text,
    advisory_mph: i64,
    severity: &str,
    imperial: bool,
) -> String {
    if imperial {
        let mph = advisory_mph;
        match severity {
            "hairpin" => text.tr_drawn(&format!("{mph} mph hairpin")),
            "sharp" => text.tr_drawn(&format!("{mph} mph sharp")),
            "moderate" => text.tr_drawn(&format!("{mph} mph moderate")),
            _ => text.tr_drawn(&format!("{mph} mph gentle")),
        }
    } else {
        let kmh = round_py_int(advisory_mph as f64 * MILES_TO_KM);
        match severity {
            "hairpin" => text.tr_drawn(&format!("{kmh} km/h hairpin")),
            "sharp" => text.tr_drawn(&format!("{kmh} km/h sharp")),
            "moderate" => text.tr_drawn(&format!("{kmh} km/h moderate")),
            _ => text.tr_drawn(&format!("{kmh} km/h gentle")),
        }
    }
}

/// The gauge rows' text size, in layout units.
pub(super) const GAUGE_TEXT: f32 = 9.5;
/// Room for a gauge's value ("40% damage") after its bar, gap included.
pub(super) const GAUGE_VALUE_COLUMN: f32 = 74.0;
/// The gap between a bar and its value.
pub(super) const GAUGE_VALUE_GAP: f32 = 8.0;
/// The gap between a gauge's label and its bar.
pub(super) const GAUGE_LABEL_GAP: f32 = 6.0;
/// The label column: the English layout's width, and the most a translation
/// may take from the bars.
pub(super) const GAUGE_LABEL_MIN: f32 = 46.0;
pub(super) const GAUGE_LABEL_MAX: f32 = 58.0;

/// The gauge labels' column, in layout units: wide enough for the widest
/// label in the player's language and a gap, so no label runs into its bar.
pub(super) fn gauge_label_column(text: &mut Text, labels: &[&str]) -> f32 {
    let widest = labels
        .iter()
        .map(|label| text.measure(label, GAUGE_TEXT, Weight::Bold))
        .fold(0.0, f32::max);
    (widest + GAUGE_LABEL_GAP).clamp(GAUGE_LABEL_MIN, GAUGE_LABEL_MAX)
}

fn speed_units(text: &Text, view: &DriveView) -> (f64, String, f64) {
    if view.hud.imperial {
        (1.0, text.tr_drawn("mph"), 80.0)
    } else {
        (1.609_344, text.tr_drawn("km/h"), 130.0)
    }
}

pub fn draw(p: &mut Painter, text: &mut Text, view: &DriveView, w: f32, h: f32, t: f32) {
    p.set_layer(Layer::Hud);
    let u = unit_of(w, h);
    top_left(p, text, view, u, t);
    next_cue(p, text, view, w, u);
    top_right(p, text, view, w, u);
    cluster(p, text, view, w, h, u, t);
    if view.hud.pulled_over || view.hud.siren {
        let flash = (t * 2.5).fract() < 0.5;
        let c = if flash { RED } else { Rgba::rgb(60, 110, 255) };
        let label = if view.hud.pulled_over {
            text.tr_drawn("PULL OVER")
        } else {
            text.tr_drawn("SIREN BEHIND")
        };
        let label = label.as_str();
        let size = 18.0 * u;
        let tw = text.measure(label, size, Weight::Bold);
        let (bw, bh) = (tw + 30.0 * u, 34.0 * u);
        let y = 92.0 * u;
        p.round_rect(w * 0.5 - bw * 0.5, y, bw, bh, 8.0 * u, c.alpha(0.88));
        text.draw(
            p,
            w * 0.5 - tw * 0.5,
            y + bh * 0.68,
            label,
            size,
            Weight::Bold,
            Rgba::WHITE,
        );
    }
}

fn top_left(p: &mut Painter, text: &mut Text, view: &DriveView, u: f32, t: f32) {
    let hud = &view.hud;
    let (x, y) = (14.0 * u, 14.0 * u);
    let mut right = x;
    if let Some(limit) = hud.limit_mph {
        let (sw, sh) = (54.0 * u, 68.0 * u);
        p.round_rect(x, y, sw, sh, 6.0 * u, Rgba::rgb(244, 244, 240));
        p.round_rect_outline(
            x + 3.0 * u,
            y + 3.0 * u,
            sw - 6.0 * u,
            sh - 6.0 * u,
            4.0 * u,
            2.0 * u,
            Rgba::rgb(20, 20, 22),
        );
        let ink = Rgba::rgb(20, 20, 22);
        let words = [
            (0.29, text.tr_drawn("SPEED")),
            (0.47, text.tr_drawn("LIMIT")),
        ];
        for (row, word) in &words {
            let (row, word) = (*row, word.as_str());
            let s = 9.5 * u;
            let tw = text.measure(word, s, Weight::Bold);
            text.draw(
                p,
                x + (sw - tw) * 0.5,
                y + sh * row,
                word,
                s,
                Weight::Bold,
                ink,
            );
        }
        let shown = if hud.imperial {
            limit
        } else {
            limit * 1.609_344
        };
        let number = format!("{:.0}", shown);
        let s = 27.0 * u;
        let tw = text.measure(&number, s, Weight::Bold);
        text.draw(
            p,
            x + (sw - tw) * 0.5,
            y + sh * 0.9,
            &number,
            s,
            Weight::Bold,
            ink,
        );
        right = x + sw + 8.0 * u;
        // Over the limit: the speed under the sign turns red.
        let over = hud.speed_mph > limit + 1.0;
        if let Some(reason) = &hud.limit_reason {
            let s = 11.0 * u;
            let label = text.ellipsize(reason, s, Weight::Bold, 120.0 * u);
            let tw = text.measure(&label, s, Weight::Bold);
            p.round_rect(x, y + sh + 6.0 * u, tw + 12.0 * u, 18.0 * u, 5.0 * u, PANEL);
            text.draw(
                p,
                x + 6.0 * u,
                y + sh + 19.0 * u,
                &label,
                s,
                Weight::Bold,
                AMBER,
            );
        }
        if over && (t * 3.0).fract() < 0.6 {
            p.round_rect_outline(
                x - 3.0 * u,
                y - 3.0 * u,
                sw + 6.0 * u,
                sh + 6.0 * u,
                8.0 * u,
                3.0 * u,
                RED,
            );
        }
    }
    if let Some((to_left, mph, distance, severity)) = &hud.curve {
        // A small curve-warning diamond with the advisory and how far.
        let r = 25.0 * u;
        let (cx, cy) = (right + r, y + r + 2.0 * u);
        let pts = [(cx, cy - r), (cx + r, cy), (cx, cy + r), (cx - r, cy)];
        p.convex(&pts, Rgba::rgb(250, 200, 30));
        let k = 0.88;
        let inner = [
            (cx, cy - r * k),
            (cx + r * k, cy),
            (cx, cy + r * k),
            (cx - r * k, cy),
        ];
        let ink = Rgba::rgb(20, 20, 22);
        p.polyline(&inner, 1.6 * u, ink, true);
        let dir = if *to_left { -1.0 } else { 1.0 };
        let arrow = [
            (cx - dir * 3.0 * u, cy + 10.0 * u),
            (cx - dir * 3.0 * u, cy + 1.0 * u),
            (cx + dir * 1.0 * u, cy - 5.0 * u),
            (cx + dir * 6.0 * u, cy - 8.0 * u),
        ];
        p.polyline(&arrow, 3.0 * u, ink, false);
        p.convex(
            &[
                (cx + dir * 11.0 * u, cy - 10.0 * u),
                (cx + dir * 4.0 * u, cy - 13.0 * u),
                (cx + dir * 6.0 * u, cy - 4.0 * u),
            ],
            ink,
        );
        let label = curve_label(text, *mph, severity, hud.imperial);
        let s = 12.0 * u;
        let tw =
            text.measure(&label, s, Weight::Bold)
                .max(text.measure(distance, s, Weight::Regular));
        let bx = cx - r * 0.2;
        let by = cy + r + 6.0 * u;
        p.round_rect(bx - 6.0 * u, by, tw + 12.0 * u, 34.0 * u, 5.0 * u, PANEL);
        text.draw(
            p,
            bx,
            by + 14.0 * u,
            &label,
            s,
            Weight::Bold,
            Rgba::rgb(250, 200, 30),
        );
        text.draw(p, bx, by + 28.0 * u, distance, s, Weight::Regular, INK);
    }
}

fn next_cue(p: &mut Painter, text: &mut Text, view: &DriveView, w: f32, u: f32) {
    let Some((what, distance)) = &view.hud.next_cue else {
        return;
    };
    let size = 15.0 * u;
    let max_w = (w * 0.42).min(420.0 * u);
    let lines: Vec<String> = text
        .wrap(what, size, Weight::Bold, max_w - 24.0 * u)
        .into_iter()
        .take(2)
        .collect();
    let distance_w = text.measure(distance, size * 0.9, Weight::Bold);
    let widest = lines
        .iter()
        .map(|l| text.measure(l, size, Weight::Bold))
        .fold(distance_w, f32::max);
    let bw = widest + 28.0 * u;
    let bh = 18.0 * u + lines.len() as f32 * size * 1.25 + size;
    let x = w * 0.5 - bw * 0.5;
    let y = 12.0 * u;
    let green = Rgba::rgb(0, 106, 62);
    p.round_rect(x, y, bw, bh, 8.0 * u, green.alpha(0.95));
    p.round_rect_outline(
        x + 3.0 * u,
        y + 3.0 * u,
        bw - 6.0 * u,
        bh - 6.0 * u,
        6.0 * u,
        1.5 * u,
        Rgba::WHITE.alpha(0.9),
    );
    let mut row = y + 10.0 * u + size;
    for line in &lines {
        let tw = text.measure(line, size, Weight::Bold);
        text.draw(
            p,
            w * 0.5 - tw * 0.5,
            row,
            line,
            size,
            Weight::Bold,
            Rgba::WHITE,
        );
        row += size * 1.25;
    }
    let s = size * 0.9;
    let tw = text.measure(distance, s, Weight::Bold);
    text.draw(
        p,
        w * 0.5 - tw * 0.5,
        row + 2.0 * u,
        distance,
        s,
        Weight::Bold,
        Rgba::rgb(255, 222, 120),
    );
}

fn top_right(p: &mut Painter, text: &mut Text, view: &DriveView, w: f32, u: f32) {
    let hud = &view.hud;
    let big = 20.0 * u;
    let small = 12.0 * u;
    let clock_w = text.measure(&hud.clock, big, Weight::Bold);
    let weather_w = text.measure(&hud.weather_name, small, Weight::Regular);
    let bw = clock_w.max(weather_w + 24.0 * u) + 24.0 * u;
    let (x, y) = (w - bw - 14.0 * u, 14.0 * u);
    p.round_rect(x, y, bw, 54.0 * u, 8.0 * u, PANEL);
    text.draw(
        p,
        x + bw - 12.0 * u - clock_w,
        y + 24.0 * u,
        &hud.clock,
        big,
        Weight::Bold,
        INK,
    );
    let wx = x + bw - 12.0 * u - weather_w;
    text.draw(
        p,
        wx,
        y + 44.0 * u,
        &hud.weather_name,
        small,
        Weight::Regular,
        DIM,
    );
    weather_icon(p, view, wx - 13.0 * u, y + 40.0 * u, 8.0 * u);
}

fn weather_icon(p: &mut Painter, view: &DriveView, cx: f32, cy: f32, r: f32) {
    use ff_core::sim::weather::WeatherKind as K;
    let night = !(6.0..19.5).contains(&view.weather.hour);
    let sun = Rgba::rgb(255, 204, 60);
    let cloud = Rgba::rgb(210, 216, 226);
    match view.weather.kind {
        K::Clear | K::Wind => {
            if night {
                p.circle(cx, cy, r * 0.8, Rgba::rgb(230, 230, 220));
                p.circle(cx + r * 0.4, cy - r * 0.3, r * 0.7, PANEL.alpha(1.0));
            } else {
                p.circle(cx, cy, r * 0.6, sun);
                for k in 0..8 {
                    let a = k as f32 / 8.0 * std::f32::consts::TAU;
                    p.line(
                        (cx + a.cos() * r * 0.8, cy + a.sin() * r * 0.8),
                        (cx + a.cos() * r * 1.1, cy + a.sin() * r * 1.1),
                        1.2,
                        sun,
                    );
                }
            }
        }
        kind => {
            p.ellipse(cx - r * 0.3, cy, r * 0.6, r * 0.45, cloud);
            p.ellipse(cx + r * 0.3, cy - r * 0.15, r * 0.65, r * 0.55, cloud);
            match kind {
                K::Rain | K::HeavyRain | K::Thunderstorm => {
                    for k in 0..3 {
                        let x = cx - r * 0.5 + k as f32 * r * 0.5;
                        p.line((x, cy + r * 0.55), (x - r * 0.2, cy + r * 1.0), 1.4, CYAN);
                    }
                    if kind == K::Thunderstorm {
                        p.convex(
                            &[
                                (cx, cy + r * 0.3),
                                (cx + r * 0.3, cy + r * 0.3),
                                (cx - r * 0.1, cy + r * 1.2),
                            ],
                            sun,
                        );
                    }
                }
                K::Snow | K::Ice => {
                    for k in 0..3 {
                        p.circle(
                            cx - r * 0.5 + k as f32 * r * 0.5,
                            cy + r * 0.85,
                            r * 0.13,
                            Rgba::WHITE,
                        );
                    }
                }
                K::Fog => {
                    for k in 0..2 {
                        let y = cy + r * (0.6 + k as f32 * 0.35);
                        p.line((cx - r * 0.8, y), (cx + r * 0.8, y), 1.4, cloud);
                    }
                }
                _ => {}
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn gauge_arc(
    p: &mut Painter,
    cx: f32,
    cy: f32,
    r: f32,
    frac0: f32,
    frac1: f32,
    width: f32,
    c: Rgba,
) {
    const START: f32 = 0.75 * std::f32::consts::PI;
    const SWEEP: f32 = 1.5 * std::f32::consts::PI;
    p.arc(
        cx,
        cy,
        r - width,
        r,
        START + SWEEP * frac0,
        START + SWEEP * frac1,
        c,
    );
}

fn needle_angle(frac: f32) -> f32 {
    0.75 * std::f32::consts::PI + 1.5 * std::f32::consts::PI * frac.clamp(0.0, 1.0)
}

fn cluster(p: &mut Painter, text: &mut Text, view: &DriveView, w: f32, h: f32, u: f32, t: f32) {
    let hud = &view.hud;
    let cw = (w - 28.0 * u).min(880.0 * u);
    let ch = 136.0 * u;
    let x = (w - cw) * 0.5;
    let y = h - ch - 10.0 * u;
    p.round_rect(x, y, cw, ch, 14.0 * u, PANEL);
    p.round_rect_outline(x, y, cw, ch, 14.0 * u, 1.0, PANEL_EDGE);
    // Route progress along the top edge.
    let px = x + 16.0 * u;
    let pw = cw - 32.0 * u;
    p.round_rect(
        px,
        y + 10.0 * u,
        pw,
        5.0 * u,
        2.5 * u,
        Rgba::new(255, 255, 255, 30),
    );
    p.round_rect(
        px,
        y + 10.0 * u,
        (pw * hud.progress as f32).max(5.0 * u),
        5.0 * u,
        2.5 * u,
        ACCENT,
    );
    let label_size = 11.0 * u;
    let title = text.ellipsize(&hud.title, label_size, Weight::Bold, pw * 0.65);
    text.draw(p, px, y + 30.0 * u, &title, label_size, Weight::Bold, DIM);
    let rw = text.measure(&hud.remaining_text, label_size, Weight::Bold);
    text.draw(
        p,
        px + pw - rw,
        y + 30.0 * u,
        &hud.remaining_text,
        label_size,
        Weight::Bold,
        INK,
    );

    let (factor, unit, max) = speed_units(text, view);
    let unit = unit.as_str();
    let row_y = y + 84.0 * u;
    // Speedometer.
    let (sx, sr) = (x + 70.0 * u, 48.0 * u);
    gauge_arc(
        p,
        sx,
        row_y,
        sr,
        0.0,
        1.0,
        4.0 * u,
        Rgba::new(255, 255, 255, 34),
    );
    let speed = hud.speed_mph.abs() * factor;
    let frac = (speed / max) as f32;
    gauge_arc(p, sx, row_y, sr, 0.0, frac.min(1.0), 4.0 * u, ACCENT);
    for k in 0..=8 {
        let a = needle_angle(k as f32 / 8.0);
        let (c, s) = (a.cos(), a.sin());
        p.line(
            (sx + c * (sr - 9.0 * u), row_y + s * (sr - 9.0 * u)),
            (sx + c * (sr - 5.0 * u), row_y + s * (sr - 5.0 * u)),
            1.4 * u,
            DIM,
        );
    }
    if let Some(limit) = hud.limit_mph {
        let a = needle_angle((limit * factor / max) as f32);
        p.line(
            (
                sx + a.cos() * (sr - 1.0 * u),
                row_y + a.sin() * (sr - 1.0 * u),
            ),
            (
                sx + a.cos() * (sr + 6.0 * u),
                row_y + a.sin() * (sr + 6.0 * u),
            ),
            3.0 * u,
            RED,
        );
    }
    if let Some(set) = hud.cruise_mph {
        let a = needle_angle((set * factor / max) as f32);
        p.circle(
            sx + a.cos() * (sr + 5.0 * u),
            row_y + a.sin() * (sr + 5.0 * u),
            3.0 * u,
            GREEN,
        );
    }
    let a = needle_angle(frac);
    p.line(
        (sx, row_y),
        (
            sx + a.cos() * (sr - 12.0 * u),
            row_y + a.sin() * (sr - 12.0 * u),
        ),
        2.2 * u,
        ACCENT,
    );
    p.circle(sx, row_y, 4.0 * u, ACCENT);
    let speed_text = format!("{:.0}", speed);
    let s = 24.0 * u;
    let over = hud.limit_mph.is_some_and(|l| hud.speed_mph > l + 1.0);
    let tw = text.measure(&speed_text, s, Weight::Bold);
    text.draw(
        p,
        sx - tw * 0.5,
        row_y + 30.0 * u,
        &speed_text,
        s,
        Weight::Bold,
        if over { RED } else { INK },
    );
    let tw = text.measure(unit, 10.0 * u, Weight::Regular);
    text.draw(
        p,
        sx - tw * 0.5,
        row_y + 43.0 * u,
        unit,
        10.0 * u,
        Weight::Regular,
        DIM,
    );

    // Tachometer and gear.
    let (tx, tr) = (x + 178.0 * u, 38.0 * u);
    gauge_arc(
        p,
        tx,
        row_y,
        tr,
        0.0,
        1.0,
        3.5 * u,
        Rgba::new(255, 255, 255, 34),
    );
    gauge_arc(p, tx, row_y, tr, 0.84, 1.0, 3.5 * u, RED.alpha(0.8));
    let rpm_frac = (hud.rpm / 2500.0) as f32;
    gauge_arc(
        p,
        tx,
        row_y,
        tr,
        0.0,
        rpm_frac.clamp(0.0, 1.0),
        3.5 * u,
        CYAN,
    );
    let gs = 24.0 * u;
    let gw = text.measure(&hud.gear, gs, Weight::Bold);
    text.draw(
        p,
        tx - gw * 0.5,
        row_y + 8.0 * u,
        &hud.gear,
        gs,
        Weight::Bold,
        INK,
    );
    let mode = if hud.automatic {
        text.tr_drawn("AUTO")
    } else {
        text.tr_drawn("MANUAL")
    };
    let mode = mode.as_str();
    let mw = text.measure(mode, 9.0 * u, Weight::Bold);
    text.draw(
        p,
        tx - mw * 0.5,
        row_y + 26.0 * u,
        mode,
        9.0 * u,
        Weight::Bold,
        DIM,
    );
    let rpm_text = text.tr_drawn(&format!("{:.0} rpm", hud.rpm));
    let rw = text.measure(&rpm_text, 9.5 * u, Weight::Regular);
    text.draw(
        p,
        tx - rw * 0.5,
        row_y + 43.0 * u,
        &rpm_text,
        9.5 * u,
        Weight::Regular,
        DIM,
    );

    // Telltales.
    let col = x + 236.0 * u;
    let mut lights: Vec<(String, Rgba)> = Vec::new();
    if hud.parking_brake || hud.spring_brakes {
        lights.push((text.tr_drawn("PARK BRAKE"), RED));
    }
    if hud.air_low {
        lights.push((text.tr_drawn("LOW AIR"), RED));
    }
    if !hud.engine_on {
        lights.push((text.tr_drawn("ENGINE OFF"), AMBER));
    }
    if let Some(set) = hud.cruise_mph {
        lights.push((text.tr_drawn(&format!("CRUISE {:.0}", set * factor)), GREEN));
    }
    if let Some(keep) = hud.keeper_mph {
        lights.push((text.tr_drawn(&format!("KEEPER {:.0}", keep * factor)), CYAN));
    }
    if hud.jake_stage > 0 {
        lights.push((text.tr_drawn(&format!("JAKE {}", hud.jake_stage)), GREEN));
    }
    if hud.truck_damage >= 60.0 {
        lights.push((text.tr_drawn("CHECK ENGINE"), AMBER));
    }
    match (view.lanes.blocked_left, view.lanes.blocked_right) {
        (true, true) => lights.push((text.tr_drawn("TRAFFIC BOTH SIDES"), AMBER)),
        (true, false) => lights.push((text.tr_drawn("TRAFFIC LEFT"), AMBER)),
        (false, true) => lights.push((text.tr_drawn("TRAFFIC RIGHT"), AMBER)),
        _ => {}
    }
    let lane_line = lane_line(text, &hud.lane_name, &hud.lane_assist);
    let tsize = LANE_LINE_TEXT * u;
    let mut ty = y + 46.0 * u;
    let mut tx2 = col;
    let col_w = LANE_LINE_WIDTH * u;
    for (label, c) in lights.iter().take(6) {
        let lw = text.measure(label, tsize, Weight::Bold) + 12.0 * u;
        if tx2 + lw > col + col_w {
            tx2 = col;
            ty += 22.0 * u;
        }
        p.round_rect(tx2, ty - 13.0 * u, lw, 18.0 * u, 4.0 * u, c.alpha(0.18));
        p.round_rect_outline(tx2, ty - 13.0 * u, lw, 18.0 * u, 4.0 * u, 1.0, c.alpha(0.7));
        text.draw(p, tx2 + 6.0 * u, ty, label, tsize, Weight::Bold, *c);
        tx2 += lw + 6.0 * u;
    }
    let lane_text = text.ellipsize(&lane_line, tsize, Weight::Regular, col_w);
    text.draw(
        p,
        col,
        y + 122.0 * u,
        &lane_text,
        tsize,
        Weight::Regular,
        DIM,
    );
    // Turn signal arrows.
    let on = (t * 1.5).fract() < 0.55;
    for (side, dir) in [(Signal::Left, -1.0f32), (Signal::Right, 1.0)] {
        let lit = hud.signal == side && on;
        let ax = col + col_w * 0.5 + dir * 30.0 * u;
        let ay = y + 98.0 * u;
        let c = if lit {
            GREEN
        } else {
            Rgba::new(255, 255, 255, 28)
        };
        p.convex(
            &[
                (ax + dir * 12.0 * u, ay),
                (ax, ay - 9.0 * u),
                (ax, ay - 4.0 * u),
                (ax - dir * 9.0 * u, ay - 4.0 * u),
            ],
            c,
        );
        p.convex(
            &[
                (ax + dir * 12.0 * u, ay),
                (ax - dir * 9.0 * u, ay + 4.0 * u),
                (ax, ay + 4.0 * u),
                (ax, ay + 9.0 * u),
            ],
            c,
        );
        p.rect(
            ax.min(ax - dir * 9.0 * u),
            ay - 4.0 * u,
            9.0 * u,
            8.0 * u,
            c,
        );
    }

    // Bars: fuel, air, truck, cargo.
    let bx = x + 440.0 * u;
    let bw = (cw - (bx - x) - 150.0 * u).max(80.0 * u);
    let fuel_c = if hud.fuel_fraction < 0.1 {
        RED
    } else if hud.fuel_fraction < 0.2 {
        AMBER
    } else {
        INK
    };
    let air_c = if hud.air_low {
        RED
    } else if hud.air_psi >= 100.0 {
        GREEN
    } else {
        AMBER
    };
    let damage_c = |d: f64| {
        if d >= 50.0 {
            RED
        } else if d >= 20.0 {
            AMBER
        } else {
            GREEN
        }
    };
    let bars = [
        (
            text.tr_drawn("FUEL"),
            hud.fuel_fraction,
            format!("{:.0}%", hud.fuel_fraction * 100.0),
            fuel_c,
        ),
        (
            text.tr_drawn("AIR"),
            hud.air_psi / 150.0,
            text.tr_drawn(&format!("{:.0} psi", hud.air_psi)),
            air_c,
        ),
        (
            text.tr_drawn("TRUCK"),
            hud.truck_damage / 100.0,
            text.tr_drawn(&format!("{:.0}% damage", hud.truck_damage)),
            damage_c(hud.truck_damage),
        ),
        (
            text.tr_drawn("CARGO"),
            hud.cargo_damage / 100.0,
            text.tr_drawn(&format!("{:.0}% damage", hud.cargo_damage)),
            damage_c(hud.cargo_damage),
        ),
    ];
    let labels: Vec<&str> = bars.iter().map(|bar| bar.0.as_str()).collect();
    let label_col = gauge_label_column(text, &labels) * u;
    for (k, (label, frac, value, c)) in bars.iter().enumerate() {
        let by = y + 46.0 * u + k as f32 * 21.0 * u;
        text.draw(p, bx, by, label, GAUGE_TEXT * u, Weight::Bold, DIM);
        let track_x = bx + label_col;
        let track_w = bw - label_col - GAUGE_VALUE_COLUMN * u;
        p.round_rect(
            track_x,
            by - 8.0 * u,
            track_w,
            7.0 * u,
            3.5 * u,
            Rgba::new(255, 255, 255, 30),
        );
        let fill =
            (track_w * (*frac as f32).clamp(0.0, 1.0)).max(if *frac > 0.0 { 7.0 * u } else { 0.0 });
        if fill > 0.0 {
            p.round_rect(track_x, by - 8.0 * u, fill, 7.0 * u, 3.5 * u, *c);
        }
        text.draw(
            p,
            track_x + track_w + GAUGE_VALUE_GAP * u,
            by,
            value,
            GAUGE_TEXT * u,
            Weight::Regular,
            INK,
        );
    }

    radar(
        p,
        view,
        x + cw - 132.0 * u,
        y + 40.0 * u,
        118.0 * u,
        ch - 50.0 * u,
        u,
        t,
    );
}

/// Every vehicle within a quarter mile behind and half a mile ahead, lane
/// by lane, with the rig in the middle.
#[allow(clippy::too_many_arguments)]
fn radar(p: &mut Painter, view: &DriveView, x: f32, y: f32, w: f32, h: f32, u: f32, t: f32) {
    p.round_rect(x, y, w, h, 8.0 * u, Rgba::new(0, 0, 0, 90));
    let lanes = view.lanes.count.max(1);
    let lane_w = (w - 16.0 * u) / lanes as f32;
    let left = x + 8.0 * u;
    let (ahead_ft, behind_ft) = (2640.0f32, 1320.0f32);
    let truck_y = y + h * (ahead_ft / (ahead_ft + behind_ft));
    let y_of = |ft: f32| truck_y - ft / (ahead_ft + behind_ft) * h;
    // Lane lines.
    for k in 0..=lanes {
        let lx = left + k as f32 * lane_w;
        let c = if k == 0 || k == lanes {
            Rgba::new(255, 255, 255, 70)
        } else {
            Rgba::new(255, 255, 255, 34)
        };
        p.rect(lx - 0.5, y + 4.0 * u, 1.0, h - 8.0 * u, c);
    }
    // Lanes count leftward from the right lane.
    let lane_x =
        |lane: i32| left + (lanes - 1 - lane.clamp(0, lanes - 1)) as f32 * lane_w + lane_w * 0.5;
    for v in &view.traffic {
        let vy = y_of(v.ahead_ft);
        if vy < y + 2.0 * u || vy > y + h - 2.0 * u {
            continue;
        }
        let (len, c) = match v.kind {
            VehicleKind::Semi => (16.0 * u, Rgba::rgb(200, 206, 214)),
            VehicleKind::BoxTruck => (10.0 * u, Rgba::rgb(180, 190, 200)),
            VehicleKind::Trooper => {
                let flash = v.beacons && (t * 3.0).fract() < 0.5;
                (6.0 * u, if flash { RED } else { Rgba::rgb(90, 120, 250) })
            }
            VehicleKind::Service => (6.0 * u, AMBER),
            VehicleKind::Car => (6.0 * u, Rgba::rgb(150, 190, 230)),
        };
        let vx = lane_x(v.lane);
        let vw = (lane_w * 0.42).min(8.0 * u);
        let c = if v.braking { RED } else { c };
        p.round_rect(vx - vw * 0.5, vy - len, vw, len, 2.0 * u, c);
    }
    // A lane beside the rig that traffic holds: shaded beside it,
    // matching the blind-spot lights on the mirrors.
    for (blocked, lane) in [
        (view.lanes.blocked_left, view.lanes.truck_lane + 1),
        (view.lanes.blocked_right, view.lanes.truck_lane - 1),
    ] {
        if blocked && (0..lanes).contains(&lane) {
            let cx = lane_x(lane);
            let top = truck_y - 14.0 * u;
            p.round_rect(
                cx - lane_w * 0.46,
                top,
                lane_w * 0.92,
                (y + h - 4.0 * u - top).min(40.0 * u),
                3.0 * u,
                AMBER.alpha(0.35),
            );
        }
    }
    // The rig: cab at the marker, trailer behind it.
    let tx = lane_x(view.lanes.truck_lane) + view.lanes.offset_ft / 12.0 * lane_w;
    let tw = (lane_w * 0.5).min(9.0 * u);
    p.round_rect(
        tx - tw * 0.5,
        truck_y,
        tw,
        72.0 / (ahead_ft + behind_ft) * h + 3.0 * u,
        2.0 * u,
        ACCENT,
    );
}

#[cfg(test)]
mod tests {
    use ff_core::settings::Settings;

    use super::curve_label;
    use crate::visuals::text::Text;

    #[test]
    fn the_curve_badge_gives_the_advisory_in_the_players_units() {
        let text = Text::new();
        assert_eq!(curve_label(&text, 35, "sharp", true), "35 mph sharp");
        assert_eq!(curve_label(&text, 35, "sharp", false), "56 km/h sharp");
        assert_eq!(curve_label(&text, 20, "hairpin", false), "32 km/h hairpin");
        // The number is the one the spoken warning gives for the same bend.
        let metric = Settings {
            imperial_units: false,
            ..Settings::default()
        };
        for mph in [15, 25, 35, 45, 55, 65] {
            assert_eq!(
                curve_label(&text, mph, "gentle", false),
                format!("{} km/h gentle", metric.speed_value(mph as f64))
            );
        }
    }
}
