//! Every screen that is not the road: menus, readers and prompts on a
//! panel, over a quiet highway at dusk (or over the paused drive), plus the
//! captions and the achievement banner that can sit over anything.

use std::time::Instant;

use ff_core::sim::weather::WeatherKind;

use super::captions::Captions;
use super::draw::{Layer, Painter, Rgba};
use super::hud::{cluster_top, unit_of, DIM, INK, PANEL, PANEL_EDGE};
use super::palette::ACCENT;
use super::scene::Panel;
use super::scene_drive::{
    Biome, DriveView, Hud, LaneLook, RoadAhead, Season, SignKind, SignView, Signal, Terrain,
    TrafficView, VehicleKind, WeatherLook, ROAD_SAMPLES,
};
use super::text::{Text, Weight};

/// Where the panel goes: on its own over the backdrop, or beside a drive.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    Left,
    Right,
}

/// The title backdrop: an evening interstate across the plains, a rig
/// ahead, nothing moving. Drawn by the same road renderer as a real drive.
pub fn showcase() -> DriveView {
    let curvature = (0..ROAD_SAMPLES)
        .map(|i| {
            let s = RoadAhead::sample_s(i);
            if (700.0..2600.0).contains(&s) {
                1.0 / 2600.0
            } else {
                0.0
            }
        })
        .collect();
    let grade = (0..ROAD_SAMPLES)
        .map(|i| {
            let s = RoadAhead::sample_s(i);
            if s < 1400.0 {
                0.02
            } else {
                -0.015
            }
        })
        .collect();
    DriveView {
        position_ft: 52_800.0,
        road: RoadAhead { curvature, grade },
        lanes: LaneLook {
            count: 2,
            truck_lane: 0,
            offset_ft: 0.0,
            yaw: 0.0,
            divided: true,
            shoulder_right_ft: 10.0,
            shoulder_left_ft: 4.0,
            lane_width_ft: 12.0,
            ramp: false,
            street: false,
            urban: false,
            rumble: 0.0,
            blocked_left: false,
            blocked_right: false,
        },
        traffic: vec![
            TrafficView {
                ahead_ft: 900.0,
                lane: 1,
                kind: VehicleKind::Semi,
                braking: false,
                signal: Signal::None,
                beacons: false,
                seed: 4,
            },
            TrafficView {
                ahead_ft: 420.0,
                lane: 0,
                kind: VehicleKind::Car,
                braking: false,
                signal: Signal::None,
                beacons: false,
                seed: 5,
            },
        ],
        signs: vec![
            SignView {
                ahead_ft: 1500.0,
                side: 1,
                kind: SignKind::Billboard {
                    text: "Freight Fate".to_string(),
                    seed: 0,
                },
            },
            SignView {
                ahead_ft: 260.0,
                side: 1,
                kind: SignKind::SpeedLimit(70),
            },
        ],
        zones: Vec::new(),
        weather: WeatherLook {
            kind: WeatherKind::Clear,
            visibility_ft: 52_800.0,
            wind: 0.1,
            wet: false,
            snow_cover: false,
            hour: 17.7,
            season: Season::Summer,
            biome: Biome::Plains,
            terrain: Terrain::Hills,
            heading: Some(4.6),
        },
        hud: Hud {
            title: String::new(),
            speed_mph: 0.0,
            limit_mph: None,
            limit_reason: None,
            rpm: 0.0,
            gear: String::new(),
            automatic: true,
            throttle: 0.0,
            brake: 0.0,
            cruise_mph: None,
            keeper_mph: None,
            jake_stage: 0,
            fuel_fraction: 1.0,
            air_psi: 120.0,
            air_low: false,
            parking_brake: false,
            spring_brakes: false,
            engine_on: true,
            truck_damage: 0.0,
            cargo_damage: 0.0,
            signal: Signal::None,
            reverse: false,
            progress: 0.0,
            remaining_text: String::new(),
            destination: String::new(),
            next_cue: None,
            curve: None,
            clock: String::new(),
            weather_name: String::new(),
            lane_name: String::new(),
            lane_assist: String::new(),
            imperial: true,
            siren: false,
            pulled_over: false,
            deadhead: false,
        },
    }
}

/// Darken the backdrop toward the panel's side so its text stands clear.
pub fn scrim(p: &mut Painter, w: f32, h: f32, placement: Placement, strength: f32) {
    p.set_layer(Layer::Hud);
    let dark = Rgba::new(6, 8, 12, 255);
    let (l, r) = match placement {
        Placement::Left => (dark.alpha(0.55 * strength), dark.alpha(0.05 * strength)),
        Placement::Right => (dark.alpha(0.2 * strength), dark.alpha(0.72 * strength)),
    };
    p.rect_hgrad(0.0, 0.0, w, h, l, r);
}

/// The game's name, top right, on screens that are not the road.
pub fn brand(p: &mut Painter, text: &mut Text, w: f32, h: f32) {
    p.set_layer(Layer::Hud);
    let u = unit_of(w, h);
    let size = 20.0 * u;
    // The title, as the main menu names it; not a label to translate.
    let word = "Freight Fate".to_uppercase();
    let spacing = 3.0 * u;
    let total: f32 = word
        .chars()
        .map(|c| text.measure(&c.to_string(), size, Weight::Bold) + spacing)
        .sum::<f32>()
        - spacing;
    let mut x = w - 28.0 * u - total;
    let y = 40.0 * u;
    p.rect(x, y + 8.0 * u, total, 3.0 * u, ACCENT);
    for c in word.chars() {
        let s = c.to_string();
        x += text.draw(p, x, y, &s, size, Weight::Bold, INK) + spacing;
    }
}

/// A screen's panel: title, any text above the rows, the rows with the
/// focused one lit, and a line saying how to move.
pub fn panel(p: &mut Painter, text: &mut Text, panel: &Panel, w: f32, h: f32, place: Placement) {
    p.set_layer(Layer::Hud);
    let u = unit_of(w, h);
    let pw = (w * 0.48).clamp(320.0 * u, 560.0 * u).min(w - 32.0 * u);
    let margin = 28.0 * u;
    let px = match place {
        Placement::Left => margin,
        Placement::Right => w - pw - margin,
    };
    let top = margin;
    let bottom_room = 88.0 * u; // captions sit below the panel
    let max_h = h - top - bottom_room;
    let pad = 20.0 * u;
    let inner = pw - pad * 2.0;
    let title_size = 22.0 * u;
    let body_size = 14.5 * u;
    let row_size = 16.5 * u;
    let row_h = 32.0 * u;

    let title_lines: Vec<String> = text
        .wrap(&panel.title, title_size, Weight::Bold, inner - 70.0 * u)
        .into_iter()
        .take(2)
        .collect();
    let mut body: Vec<(String, bool)> = Vec::new();
    for (i, line) in panel.body.iter().enumerate() {
        let focused = panel.body_focus == Some(i);
        if line.trim().is_empty() {
            body.push((String::new(), false));
            continue;
        }
        for part in text.wrap(line, body_size, Weight::Regular, inner - 10.0 * u) {
            body.push((part, focused));
        }
    }
    let rows: &[String] = panel.rows.as_ref().map(|r| r.0.as_slice()).unwrap_or(&[]);
    let focus = panel.rows.as_ref().map(|r| r.1);
    // A long row (a job on the board, a help line) wraps rather than
    // losing its end; three lines at most.
    let row_line = row_size * 1.3;
    let wrapped: Vec<Vec<String>> = rows
        .iter()
        .enumerate()
        .map(|(i, row)| {
            let weight = if Some(i) == focus {
                Weight::Bold
            } else {
                Weight::Regular
            };
            let mut lines = text.wrap(row, row_size, weight, inner - 16.0 * u);
            if lines.len() > 3 {
                let rest = lines[2..].join(" ");
                lines.truncate(2);
                lines.push(text.ellipsize(&rest, row_size, weight, inner - 16.0 * u));
            }
            lines
        })
        .collect();
    let row_height = |i: usize| row_h + (wrapped[i].len().max(1) - 1) as f32 * row_line;
    // Heights, each exactly what is drawn below.
    let title_step = title_size * 1.2;
    let header_h = pad + title_lines.len() as f32 * title_step + 18.0 * u;
    let footer_h = 34.0 * u;
    let body_line = body_size * 1.45;
    let gap = 10.0 * u;
    // Fit what fits: rows keep priority over a long summary.
    let room = (max_h - header_h - footer_h).max(row_h);
    let rows_wanted: f32 = (0..rows.len()).map(row_height).sum();
    let body_room = if rows.is_empty() {
        room
    } else {
        (room - rows_wanted.min(room * 0.6)).max(body_line * 2.0)
    };
    let body_fit = ((body_room / body_line).floor().max(0.0) as usize).min(body.len());
    let body_h = if body_fit > 0 {
        body_fit as f32 * body_line + gap
    } else {
        0.0
    };
    // The rows shown: a window around the focus that fits the room left.
    let rows_room = room - body_h;
    let (first_row, last_row) = row_window(rows.len(), focus.unwrap_or(0), rows_room, &row_height);
    let rows_h: f32 = (first_row..last_row).map(row_height).sum();
    let ph = (header_h + body_h + rows_h + footer_h)
        .min(max_h)
        .max(120.0 * u);
    p.round_rect(px, top, pw, ph, 16.0 * u, PANEL);
    p.round_rect_outline(px, top, pw, ph, 16.0 * u, 1.0, PANEL_EDGE);

    for (i, line) in title_lines.iter().enumerate() {
        let baseline = top + pad + title_size * 0.9 + i as f32 * title_step;
        text.draw(p, px + pad, baseline, line, title_size, Weight::Bold, INK);
    }
    if let Some(f) = focus {
        let counter = text.tr_drawn(&format!("{} of {}", f + 1, rows.len()));
        let cw = text.measure(&counter, 12.0 * u, Weight::Regular);
        text.draw(
            p,
            px + pw - pad - cw,
            top + pad + title_size * 0.8,
            &counter,
            12.0 * u,
            Weight::Regular,
            DIM,
        );
    }
    p.rect(
        px + pad,
        top + header_h - 12.0 * u,
        40.0 * u,
        3.0 * u,
        ACCENT,
    );
    let mut y = top + header_h;

    // The body: scrolled to keep its marked line in view.
    let focus_line = body.iter().position(|(_, f)| *f).unwrap_or(0);
    let body_start = if body_fit == 0 || focus_line < body_fit {
        0
    } else {
        (focus_line + 1 - body_fit).min(body.len() - body_fit)
    };
    for (line, focused) in body.iter().skip(body_start).take(body_fit) {
        if *focused {
            p.round_rect(
                px + pad - 8.0 * u,
                y,
                inner + 16.0 * u,
                body_line,
                5.0 * u,
                ACCENT.alpha(0.16),
            );
            p.rect(px + pad - 8.0 * u, y, 3.0 * u, body_line, ACCENT);
        }
        let c = if *focused { INK } else { INK.alpha(0.86) };
        text.draw(
            p,
            px + pad,
            y + body_line * 0.5 + body_size * 0.36,
            line,
            body_size,
            Weight::Regular,
            c,
        );
        y += body_line;
    }
    if body_fit > 0 {
        y += gap;
    }

    // The rows, scrolled to keep focus in view.
    if !rows.is_empty() {
        if first_row > 0 {
            more_marker(p, px + pw - pad - 4.0 * u, y + 4.0 * u, -1.0, u);
        }
        for (i, lines) in wrapped.iter().enumerate().take(last_row).skip(first_row) {
            let focused = Some(i) == focus;
            let rh = row_height(i);
            if focused {
                p.round_rect(
                    px + 10.0 * u,
                    y + 2.0 * u,
                    pw - 20.0 * u,
                    rh - 4.0 * u,
                    8.0 * u,
                    ACCENT.alpha(0.2),
                );
                p.round_rect(
                    px + 10.0 * u,
                    y + 7.0 * u,
                    4.0 * u,
                    rh - 14.0 * u,
                    2.0 * u,
                    ACCENT,
                );
            }
            let weight = if focused {
                Weight::Bold
            } else {
                Weight::Regular
            };
            let c = if focused {
                Rgba::WHITE
            } else {
                INK.alpha(0.82)
            };
            let mut baseline = y + row_h * 0.5 + row_size * 0.36;
            for line in lines {
                text.draw(p, px + pad + 6.0 * u, baseline, line, row_size, weight, c);
                baseline += row_line;
            }
            y += rh;
        }
        if last_row < rows.len() {
            more_marker(p, px + pw - pad - 4.0 * u, y - 4.0 * u, 1.0, u);
        }
    }

    let hint = if rows.is_empty() {
        text.tr_drawn("Escape to go back")
    } else {
        text.tr_drawn("Arrows to move  ·  Enter to choose  ·  Escape to go back")
    };
    let hint = text.ellipsize(&hint, 11.0 * u, Weight::Regular, inner);
    p.rect(
        px + pad,
        top + ph - footer_h + 4.0 * u,
        inner,
        1.0,
        PANEL_EDGE,
    );
    text.draw(
        p,
        px + pad,
        top + ph - 13.0 * u,
        &hint,
        11.0 * u,
        Weight::Regular,
        DIM,
    );
}

/// Which rows to show, `first..last`: as many as fit in `room`, with the
/// focused row in view and, where possible, rows on both sides of it.
fn row_window(n: usize, focus: usize, room: f32, height: &dyn Fn(usize) -> f32) -> (usize, usize) {
    if n == 0 {
        return (0, 0);
    }
    let focus = focus.min(n - 1);
    let (mut first, mut last) = (focus, focus + 1);
    let mut used = height(focus);
    // Grow toward whichever side has had less, until nothing more fits.
    loop {
        let before = focus - first;
        let after = last - focus - 1;
        let grow_down = last < n && (after <= before || first == 0);
        if grow_down {
            if used + height(last) > room {
                break;
            }
            used += height(last);
            last += 1;
        } else if first > 0 {
            if used + height(first - 1) > room {
                break;
            }
            used += height(first - 1);
            first -= 1;
        } else {
            break;
        }
    }
    (first, last)
}

fn more_marker(p: &mut Painter, x: f32, y: f32, dir: f32, u: f32) {
    let r = 5.0 * u;
    p.convex(
        &[
            (x - r, y - dir * r * 0.5),
            (x + r, y - dir * r * 0.5),
            (x, y + dir * r * 0.7),
        ],
        DIM,
    );
}

/// Spoken lines as captions, newest at the bottom, just above whatever
/// the bottom of the screen holds.
pub fn captions(
    p: &mut Painter,
    text: &mut Text,
    captions: &Captions,
    w: f32,
    h: f32,
    driving: bool,
    now: Instant,
) {
    if captions.is_empty() {
        return;
    }
    p.set_layer(Layer::Hud);
    let u = unit_of(w, h);
    let size = 16.0 * u;
    let max_w = (w * 0.72).min(720.0 * u);
    let mut blocks: Vec<(Vec<String>, f32, bool)> = Vec::new();
    for c in captions.visible() {
        let lines: Vec<String> = text
            .wrap(&c.text, size, Weight::Regular, max_w - 28.0 * u)
            .into_iter()
            .take(3)
            .collect();
        blocks.push((lines, c.alpha(now), c.event));
    }
    let line_h = size * 1.3;
    let mut bottom = if driving {
        cluster_top(w, h) - 10.0 * u
    } else {
        h - 18.0 * u
    };
    for (lines, alpha, event) in blocks.iter().rev() {
        let widest = lines
            .iter()
            .map(|l| text.measure(l, size, Weight::Regular))
            .fold(0.0, f32::max);
        let bw = widest + 28.0 * u;
        let bh = lines.len() as f32 * line_h + 14.0 * u;
        let x = w * 0.5 - bw * 0.5;
        let y = bottom - bh;
        p.round_rect(x, y, bw, bh, 10.0 * u, PANEL.fade(*alpha));
        if *event {
            p.round_rect(
                x,
                y + 6.0 * u,
                3.0 * u,
                bh - 12.0 * u,
                1.5 * u,
                ACCENT.fade(*alpha),
            );
        }
        let mut row = y + 7.0 * u + size;
        for line in lines {
            let lw = text.measure(line, size, Weight::Regular);
            text.draw(
                p,
                w * 0.5 - lw * 0.5,
                row,
                line,
                size,
                Weight::Regular,
                INK.fade(*alpha),
            );
            row += line_h;
        }
        bottom = y - 6.0 * u;
    }
}

/// An achievement just earned: under the route sign while driving, above
/// the captions on other screens, never over the panel being read.
pub fn achievement(p: &mut Painter, text: &mut Text, notice: &str, w: f32, h: f32, driving: bool) {
    p.set_layer(Layer::Hud);
    let u = unit_of(w, h);
    let size = 15.0 * u;
    let line = text.ellipsize(notice, size, Weight::Bold, w * 0.6);
    let tw = text.measure(&line, size, Weight::Bold);
    let (bw, bh) = (tw + 56.0 * u, 40.0 * u);
    let x = w * 0.5 - bw * 0.5;
    let y = if driving { 96.0 * u } else { h - 200.0 * u };
    let gold = Rgba::rgb(232, 182, 60);
    p.round_rect_vgrad(x, y, bw, bh, 12.0 * u, gold.shade(1.1), gold.shade(0.8));
    // A star.
    let (cx, cy, r) = (x + 22.0 * u, y + bh * 0.5, 9.0 * u);
    let mut pts = Vec::new();
    for k in 0..10 {
        let a = -std::f32::consts::FRAC_PI_2 + k as f32 * std::f32::consts::PI / 5.0;
        let rr = if k % 2 == 0 { r } else { r * 0.45 };
        pts.push((cx + a.cos() * rr, cy + a.sin() * rr));
    }
    // A star is not convex: fan it from the centre.
    for k in 0..10 {
        p.convex(&[(cx, cy), pts[k], pts[(k + 1) % 10]], Rgba::rgb(60, 40, 6));
    }
    text.draw(
        p,
        x + 40.0 * u,
        y + bh * 0.66,
        &line,
        size,
        Weight::Bold,
        Rgba::rgb(40, 26, 4),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_row_window_keeps_the_focus_in_view_and_fills_the_room() {
        let one = |_: usize| 10.0;
        assert_eq!(row_window(5, 0, 100.0, &one), (0, 5));
        assert_eq!(row_window(20, 0, 50.0, &one), (0, 5));
        assert_eq!(row_window(20, 19, 50.0, &one), (15, 20));
        let (first, last) = row_window(20, 10, 50.0, &one);
        assert!(first <= 10 && 10 < last && last - first == 5);
        // A tall focused row still shows, even alone.
        let tall = |i: usize| if i == 3 { 90.0 } else { 10.0 };
        assert_eq!(row_window(6, 3, 60.0, &tall), (3, 4));
        assert_eq!(row_window(0, 0, 60.0, &one), (0, 0));
    }
}
