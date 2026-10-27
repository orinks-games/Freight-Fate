//! Road signs in the world, drawn to the MUTCD patterns a US driver knows:
//! the white speed limit rectangle, yellow curve diamonds and chevrons,
//! orange work-zone signs, green guide signs, blue service signs. They are
//! drawn about a third larger than life so they can be read on a laptop
//! screen at the distance a driver would read them.

use super::draw::{Layer, Painter, Pt, Rgba};
use super::road::{Geom, RoadFrame};
use super::scene_drive::{SignKind, SignView};
use super::text::{Text, Weight};

const WHITE: Rgba = Rgba::rgb(244, 244, 240);
const BLACK: Rgba = Rgba::rgb(20, 20, 22);
const YELLOW: Rgba = Rgba::rgb(250, 200, 30);
const ORANGE: Rgba = Rgba::rgb(244, 120, 30);
const GREEN: Rgba = Rgba::rgb(0, 106, 62);
const BLUE: Rgba = Rgba::rgb(20, 68, 150);
const POST: Rgba = Rgba::rgb(130, 132, 134);

/// Width, height, bottom edge above the ground, and set-back from the
/// pavement, feet.
fn dims(kind: &SignKind, service_rows: usize) -> (f32, f32, f32, f32) {
    match kind {
        SignKind::SpeedLimit(_) => (4.0, 5.0, 6.5, 4.0),
        SignKind::Curve { .. } => (5.0, 5.0, 7.5, 4.0),
        SignKind::Chevron { .. } => (2.6, 3.4, 4.5, 2.0),
        SignKind::RoadWork => (5.6, 5.6, 7.0, 4.0),
        SignKind::EndRoadWork => (7.0, 3.6, 6.5, 4.0),
        SignKind::Guide { lines, distance } => {
            let rows = lines.len() as f32 + if distance.is_some() { 1.0 } else { 0.0 };
            (20.0, 4.0 + rows * 3.2, 9.0, 8.0)
        }
        SignKind::Service { .. } => (13.0, 3.0 + service_rows as f32 * 2.8, 7.5, 6.0),
        SignKind::Welcome(_) => (16.0, 8.0, 5.0, 10.0),
        SignKind::Billboard { .. } => (48.0, 14.0, 20.0, 40.0),
    }
}

/// A service sign's text height and line width, feet (13 ft wide, a foot
/// of border each side).
pub(super) const SERVICE_TEXT_FT: f32 = 1.8;
pub(super) const SERVICE_LINE_FT: f32 = 11.0;

/// A blue service sign's lines, each wrapped to the sign's width the way a
/// guide sign's are, so a long translation takes two lines instead of being
/// cut short. Measured at ten times the size, where the wrap is the same.
pub(super) fn service_rows(text: &mut Text, lines: &[String]) -> Vec<String> {
    let mut rows = Vec::new();
    for line in lines {
        rows.extend(text.wrap(
            line,
            SERVICE_TEXT_FT * 10.0,
            Weight::Bold,
            SERVICE_LINE_FT * 10.0,
        ));
    }
    rows
}

fn left_side(g: &Geom) -> f32 {
    if g.divided {
        g.left_edge() - g.shoulder_l - 3.0
    } else {
        g.left_outer() - 3.0
    }
}

pub fn draw(p: &mut Painter, text: &mut Text, f: &RoadFrame, sign: &SignView) {
    let g = &f.geom;
    let rows = match &sign.kind {
        SignKind::Service { lines } => service_rows(text, lines),
        _ => Vec::new(),
    };
    let (w_ft, h_ft, bottom, back) = dims(&sign.kind, rows.len());
    let u = if sign.side > 0 {
        g.right_outer() + back + w_ft * 0.5
    } else {
        left_side(g) - back - w_ft * 0.5
    };
    let Some((x, y, b, z)) = f.cam.project(sign.ahead_ft, u, 0.0) else {
        return;
    };
    let (pw, ph) = (w_ft * b, h_ft * b);
    if pw < 1.2 || x + pw < -50.0 || x - pw > f.cam.w + 50.0 {
        return;
    }
    let light = f.light;
    // Sign faces are retroreflective: in the headlights at night they
    // shine back, so they are lit by the lamps and not the dark.
    let retro = if sign.side > 0 && !matches!(sign.kind, SignKind::Billboard { .. }) {
        light.night * (1.0 - z / 900.0).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let paint = |c: Rgba| light.lit(c, z).mix(light.lamp(c.shade(0.92), z), retro);
    let top = y - (bottom + h_ft) * b;
    let left = x - pw * 0.5;
    // Posts.
    let post_w = (0.4 * b).max(1.0);
    let post = paint(POST);
    let posts: &[f32] = match sign.kind {
        SignKind::Guide { .. } | SignKind::Service { .. } | SignKind::Welcome(_) => &[0.2, 0.8],
        SignKind::Billboard { .. } => &[0.3, 0.7],
        _ => &[0.5],
    };
    for &k in posts {
        let px = left + pw * k;
        let w = if matches!(sign.kind, SignKind::Billboard { .. }) {
            post_w * 3.0
        } else {
            post_w
        };
        p.rect(px - w * 0.5, top + ph, w, y - top - ph, post);
    }
    let legible = ph >= 9.0;
    let ink = paint(BLACK);
    match &sign.kind {
        SignKind::SpeedLimit(mph) => {
            p.rect(left, top, pw, ph, paint(WHITE));
            frame(p, left, top, pw, ph, ink, b);
            if legible {
                let small = ph * 0.17;
                let label = text.tr_drawn("SPEED");
                centered(
                    p,
                    text,
                    x,
                    top + ph * 0.24,
                    &label,
                    small,
                    Weight::Bold,
                    ink,
                );
                let label = text.tr_drawn("LIMIT");
                centered(
                    p,
                    text,
                    x,
                    top + ph * 0.43,
                    &label,
                    small,
                    Weight::Bold,
                    ink,
                );
                centered(
                    p,
                    text,
                    x,
                    top + ph * 0.88,
                    &mph.to_string(),
                    ph * 0.42,
                    Weight::Bold,
                    ink,
                );
            }
        }
        SignKind::Curve { left: to_left, mph } => {
            let plaque_h = ph * 0.42;
            let d = ph - plaque_h * 0.2;
            diamond(p, x, top + d * 0.5, d * 0.5, paint(YELLOW), ink, b);
            if legible {
                curve_arrow(p, x, top + d * 0.5, d * 0.25, *to_left, ink);
            }
            let py = top + ph + 0.5 * b;
            p.rect(x - pw * 0.36, py, pw * 0.72, plaque_h, paint(YELLOW));
            frame(p, x - pw * 0.36, py, pw * 0.72, plaque_h, ink, b);
            if legible {
                centered(
                    p,
                    text,
                    x,
                    py + plaque_h * 0.5,
                    &mph.to_string(),
                    plaque_h * 0.42,
                    Weight::Bold,
                    ink,
                );
                let label = text.tr_drawn("MPH");
                centered(
                    p,
                    text,
                    x,
                    py + plaque_h * 0.88,
                    &label,
                    plaque_h * 0.3,
                    Weight::Bold,
                    ink,
                );
            }
        }
        SignKind::Chevron { left: to_left } => {
            p.rect(left, top, pw, ph, paint(YELLOW));
            let dir = if *to_left { -1.0 } else { 1.0 };
            let (cx, cy) = (x, top + ph * 0.5);
            let (aw, ah) = (pw * 0.32, ph * 0.36);
            let thick = pw * 0.22;
            let pts: [Pt; 6] = [
                (cx - dir * aw, cy - ah),
                (cx - dir * aw + dir * thick, cy - ah),
                (cx + dir * aw, cy),
                (cx - dir * aw + dir * thick, cy + ah),
                (cx - dir * aw, cy + ah),
                (cx + dir * (aw - thick), cy),
            ];
            p.convex(&[pts[0], pts[1], pts[2], pts[5]], ink);
            p.convex(&[pts[5], pts[2], pts[3], pts[4]], ink);
        }
        SignKind::RoadWork => {
            diamond(p, x, top + ph * 0.5, ph * 0.5, paint(ORANGE), ink, b);
            if legible {
                let s = ph * 0.13;
                let label = text.tr_drawn("ROAD");
                centered(p, text, x, top + ph * 0.4, &label, s, Weight::Bold, ink);
                let label = text.tr_drawn("WORK");
                centered(p, text, x, top + ph * 0.56, &label, s, Weight::Bold, ink);
                let label = text.tr_drawn("AHEAD");
                centered(p, text, x, top + ph * 0.72, &label, s, Weight::Bold, ink);
            }
        }
        SignKind::EndRoadWork => {
            p.rect(left, top, pw, ph, paint(ORANGE));
            frame(p, left, top, pw, ph, ink, b);
            if legible {
                let label = text.tr_drawn("END");
                centered(
                    p,
                    text,
                    x,
                    top + ph * 0.42,
                    &label,
                    ph * 0.26,
                    Weight::Bold,
                    ink,
                );
                let label = text.tr_drawn("ROAD WORK");
                centered(
                    p,
                    text,
                    x,
                    top + ph * 0.8,
                    &label,
                    ph * 0.26,
                    Weight::Bold,
                    ink,
                );
            }
        }
        SignKind::Guide { lines, distance } => {
            let bg = paint(GREEN);
            let fg = paint(WHITE);
            p.round_rect(left, top, pw, ph, 0.8 * b, bg);
            p.round_rect_outline(
                left + 0.5 * b,
                top + 0.5 * b,
                pw - b,
                ph - b,
                0.6 * b,
                (0.25 * b).max(1.0),
                fg,
            );
            if legible {
                let size = (2.0 * b).min(ph * 0.3);
                let mut row = top + 2.0 * b + size;
                for line in lines {
                    for part in text
                        .wrap(line, size, Weight::Bold, pw - 3.0 * b)
                        .into_iter()
                        .take(2)
                    {
                        centered_baseline(p, text, x, row, &part, size, Weight::Bold, fg);
                        row += size * 1.2;
                    }
                }
                if let Some(d) = distance {
                    centered_baseline(
                        p,
                        text,
                        x,
                        top + ph - 1.4 * b,
                        d,
                        size * 0.85,
                        Weight::Bold,
                        fg,
                    );
                }
            }
        }
        SignKind::Service { .. } => {
            let fg = paint(WHITE);
            p.round_rect(left, top, pw, ph, 0.6 * b, paint(BLUE));
            p.round_rect_outline(
                left + 0.4 * b,
                top + 0.4 * b,
                pw - 0.8 * b,
                ph - 0.8 * b,
                0.5 * b,
                (0.2 * b).max(1.0),
                fg,
            );
            if legible {
                let size = (1.8 * b).min(ph * 0.28);
                let mut row = top + 1.6 * b + size;
                for line in &rows {
                    let fitted = text.ellipsize(line, size, Weight::Bold, pw - 2.0 * b);
                    centered_baseline(p, text, x, row, &fitted, size, Weight::Bold, fg);
                    row += 2.8 * b;
                }
            }
        }
        SignKind::Welcome(state) => {
            p.round_rect(left, top, pw, ph, 0.6 * b, paint(Rgba::rgb(26, 52, 98)));
            p.rect(left, top + ph * 0.72, pw, ph * 0.06, paint(YELLOW));
            if legible {
                let fg = paint(WHITE);
                let label = text.tr_drawn("WELCOME TO");
                centered(
                    p,
                    text,
                    x,
                    top + ph * 0.28,
                    &label,
                    ph * 0.14,
                    Weight::Bold,
                    fg,
                );
                let size = ph * 0.26;
                let fitted = text.ellipsize(&state.to_uppercase(), size, Weight::Bold, pw * 0.92);
                centered(p, text, x, top + ph * 0.54, &fitted, size, Weight::Bold, fg);
            }
        }
        SignKind::Billboard { text: words, seed } => {
            billboard(p, text, f, left, top, pw, ph, b, z, words, *seed);
        }
    }
}

fn frame(p: &mut Painter, x: f32, y: f32, w: f32, h: f32, ink: Rgba, b: f32) {
    let t = (0.18 * b).max(0.8);
    let inset = 0.25 * b;
    p.round_rect_outline(
        x + inset,
        y + inset,
        w - 2.0 * inset,
        h - 2.0 * inset,
        0.3 * b,
        t,
        ink,
    );
}

fn diamond(p: &mut Painter, cx: f32, cy: f32, r: f32, fill: Rgba, ink: Rgba, b: f32) {
    let pts = [(cx, cy - r), (cx + r, cy), (cx, cy + r), (cx - r, cy)];
    p.convex(&pts, fill);
    let inner = r * 0.9;
    let border = [
        (cx, cy - inner),
        (cx + inner, cy),
        (cx, cy + inner),
        (cx - inner, cy),
    ];
    p.polyline(&border, (0.2 * b).max(0.8), ink, true);
}

/// The W1-2 arrow: straight up, then bending the way the road goes.
fn curve_arrow(p: &mut Painter, cx: f32, cy: f32, r: f32, to_left: bool, ink: Rgba) {
    let dir = if to_left { -1.0 } else { 1.0 };
    let mut pts = Vec::new();
    pts.push((cx - dir * r * 0.15, cy + r * 1.2));
    pts.push((cx - dir * r * 0.15, cy + r * 0.2));
    for k in 0..=8 {
        let a = k as f32 / 8.0 * std::f32::consts::FRAC_PI_2;
        pts.push((
            cx - dir * r * 0.15 + dir * r * 0.75 * (1.0 - a.cos()),
            cy + r * 0.2 - r * 0.75 * a.sin(),
        ));
    }
    let tip = *pts.last().unwrap_or(&(cx, cy));
    p.polyline(&pts, r * 0.32, ink, false);
    let head = [
        (tip.0 + dir * r * 0.55, tip.1),
        (tip.0 - dir * r * 0.05, tip.1 - r * 0.45),
        (tip.0 - dir * r * 0.05, tip.1 + r * 0.45),
    ];
    p.convex(&head, ink);
}

#[allow(clippy::too_many_arguments)]
fn centered(
    p: &mut Painter,
    text: &mut Text,
    cx: f32,
    cy: f32,
    s: &str,
    size: f32,
    weight: Weight,
    c: Rgba,
) {
    let (ascent, _) = text.metrics(size);
    centered_baseline(p, text, cx, cy + ascent * 0.36, s, size, weight, c);
}

#[allow(clippy::too_many_arguments)]
fn centered_baseline(
    p: &mut Painter,
    text: &mut Text,
    cx: f32,
    baseline: f32,
    s: &str,
    size: f32,
    weight: Weight,
    c: Rgba,
) {
    let w = text.measure(s, size, weight);
    text.draw_scaled(p, cx - w * 0.5, baseline, s, size, weight, c);
}

const BOARD_PAINT: [(Rgba, Rgba); 6] = [
    (Rgba::rgb(28, 40, 76), Rgba::rgb(255, 214, 90)),
    (Rgba::rgb(180, 30, 38), Rgba::rgb(255, 250, 240)),
    (Rgba::rgb(244, 240, 228), Rgba::rgb(30, 34, 40)),
    (Rgba::rgb(20, 110, 96), Rgba::rgb(250, 248, 236)),
    (Rgba::rgb(250, 196, 40), Rgba::rgb(24, 24, 28)),
    (Rgba::rgb(40, 44, 52), Rgba::rgb(130, 220, 255)),
];

#[allow(clippy::too_many_arguments)]
fn billboard(
    p: &mut Painter,
    text: &mut Text,
    f: &RoadFrame,
    left: f32,
    top: f32,
    pw: f32,
    ph: f32,
    b: f32,
    z: f32,
    words: &str,
    seed: u32,
) {
    let light = f.light;
    let (bg, fg) = BOARD_PAINT[(seed % BOARD_PAINT.len() as u32) as usize];
    // Lit from below at night, so the face keeps most of its colour.
    let night_lit = light.night > 0.4;
    let paint = |c: Rgba| {
        let lit = light.lit(c, z);
        if night_lit {
            lit.mix(light.lamp(c.shade(0.8), z), 0.75)
        } else {
            lit
        }
    };
    p.rect(
        left - 0.6 * b,
        top - 0.6 * b,
        pw + 1.2 * b,
        ph + 1.2 * b,
        paint(Rgba::rgb(70, 70, 74)),
    );
    p.rect(left, top, pw, ph, paint(bg));
    p.rect(left, top + ph * 0.86, pw, ph * 0.14, paint(fg).fade(0.85));
    if ph >= 8.0 {
        let size = (ph * 0.2).min(3.6 * b);
        let lines = text.wrap(words, size, Weight::Bold, pw * 0.9);
        let shown: Vec<String> = lines.into_iter().take(3).collect();
        let total = shown.len() as f32 * size * 1.15;
        let mut row = top + (ph * 0.86 - total) * 0.5 + size;
        for line in &shown {
            let w = text.measure(line, size, Weight::Bold);
            text.draw_scaled(
                p,
                left + (pw - w) * 0.5,
                row,
                line,
                size,
                Weight::Bold,
                paint(fg),
            );
            row += size * 1.15;
        }
    }
    if night_lit {
        p.set_layer(Layer::Glow);
        for k in [0.25f32, 0.75] {
            p.glow_ellipse(
                left + pw * k,
                top + ph * 0.3,
                pw * 0.3,
                ph * 0.6,
                Rgba::rgb(255, 230, 180).alpha(0.12 * light.night),
            );
        }
        p.set_layer(Layer::World);
    }
}
