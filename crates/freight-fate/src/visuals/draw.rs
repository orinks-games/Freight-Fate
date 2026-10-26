//! The painter: every shape on screen becomes coloured triangles in one of
//! three batches, and each batch is a single `SDL_RenderGeometry` call. A
//! frame is therefore three draw calls whatever it shows, which is what
//! keeps the visual display cheap on a laptop GPU.
//!
//! Shapes are anti-aliased the way Dear ImGui does it: a filled polygon is
//! drawn slightly inset, then ringed with a one-pixel fringe that fades to
//! transparent, so edges come out smooth without multisampling.
//!
//! Everything samples one texture, the glyph atlas: text samples glyph
//! coverage, and every solid shape samples the atlas's white block, so
//! vertex colour alone decides what a fill looks like.

use sdl2::pixels::Color;
use sdl2::rect::FPoint;
use sdl2::render::Vertex;

/// A colour with straight (not premultiplied) alpha, components 0..=1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const fn rgb(r: u8, g: u8, b: u8) -> Rgba {
        Rgba {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: 1.0,
        }
    }

    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Rgba {
        Rgba {
            r: r as f32 / 255.0,
            g: g as f32 / 255.0,
            b: b as f32 / 255.0,
            a: a as f32 / 255.0,
        }
    }

    pub const WHITE: Rgba = Rgba::rgb(255, 255, 255);
    pub const BLACK: Rgba = Rgba::rgb(0, 0, 0);

    pub fn alpha(self, a: f32) -> Rgba {
        Rgba {
            a: a.clamp(0.0, 1.0),
            ..self
        }
    }

    pub fn fade(self, k: f32) -> Rgba {
        Rgba {
            a: (self.a * k).clamp(0.0, 1.0),
            ..self
        }
    }

    /// Linear mix toward `other`; `t` 0 is self, 1 is other.
    pub fn mix(self, other: Rgba, t: f32) -> Rgba {
        let t = t.clamp(0.0, 1.0);
        Rgba {
            r: self.r + (other.r - self.r) * t,
            g: self.g + (other.g - self.g) * t,
            b: self.b + (other.b - self.b) * t,
            a: self.a + (other.a - self.a) * t,
        }
    }

    /// Every channel scaled by `k` (lighting); alpha kept.
    pub fn shade(self, k: f32) -> Rgba {
        Rgba {
            r: (self.r * k).clamp(0.0, 1.0),
            g: (self.g * k).clamp(0.0, 1.0),
            b: (self.b * k).clamp(0.0, 1.0),
            a: self.a,
        }
    }

    /// Channel-wise product (a coloured light on a coloured surface).
    pub fn tint(self, light: Rgba) -> Rgba {
        Rgba {
            r: (self.r * light.r).clamp(0.0, 1.0),
            g: (self.g * light.g).clamp(0.0, 1.0),
            b: (self.b * light.b).clamp(0.0, 1.0),
            a: self.a,
        }
    }

    pub fn to_color(self) -> Color {
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        Color::RGBA(byte(self.r), byte(self.g), byte(self.b), byte(self.a))
    }
}

/// Which batch a shape lands in. Drawn in this order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    /// The road view: sky, terrain, road, vehicles, weather.
    World = 0,
    /// Lights in the world (headlights, tail lights, beacons), added
    /// to what is under them rather than painted over it.
    Glow = 1,
    /// Dashboard, signs on the HUD, captions, menus.
    Hud = 2,
}

pub const LAYERS: usize = 3;

#[derive(Default)]
pub struct Batch {
    pub vertices: Vec<Vertex>,
    pub indices: Vec<i32>,
}

impl Batch {
    fn clear(&mut self) {
        self.vertices.clear();
        self.indices.clear();
    }
}

pub type Pt = (f32, f32);

pub struct Painter {
    pub batches: [Batch; LAYERS],
    layer: usize,
    /// Atlas coordinates of the middle of the white block.
    white: FPoint,
    /// Fringe width in pixels.
    aa: f32,
    /// Scratch for polygon outlines, reused between shapes.
    scratch: Vec<Pt>,
    normals: Vec<Pt>,
}

impl Painter {
    pub fn new(white_uv: (f32, f32)) -> Painter {
        Painter {
            batches: Default::default(),
            layer: Layer::World as usize,
            white: FPoint::new(white_uv.0, white_uv.1),
            aa: 1.0,
            scratch: Vec::new(),
            normals: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        for batch in &mut self.batches {
            batch.clear();
        }
        self.layer = Layer::World as usize;
    }

    pub fn set_layer(&mut self, layer: Layer) {
        self.layer = layer as usize;
    }

    pub fn layer(&self) -> Layer {
        match self.layer {
            0 => Layer::World,
            1 => Layer::Glow,
            _ => Layer::Hud,
        }
    }

    pub fn vertex_count(&self) -> usize {
        self.batches.iter().map(|b| b.vertices.len()).sum()
    }

    fn batch(&mut self) -> &mut Batch {
        &mut self.batches[self.layer]
    }

    /// One vertex. Colours arrive converted: a shape converts each colour
    /// once, not once per vertex (the conversion was the painter's hottest
    /// line).
    fn push(&mut self, p: Pt, c: Color) -> i32 {
        let white = self.white;
        let batch = self.batch();
        batch.vertices.push(Vertex {
            position: FPoint::new(p.0, p.1),
            color: c,
            tex_coord: white,
        });
        (batch.vertices.len() - 1) as i32
    }

    fn push_uv(&mut self, p: Pt, c: Color, uv: Pt) -> i32 {
        let batch = self.batch();
        batch.vertices.push(Vertex {
            position: FPoint::new(p.0, p.1),
            color: c,
            tex_coord: FPoint::new(uv.0, uv.1),
        });
        (batch.vertices.len() - 1) as i32
    }

    fn index3(&mut self, a: i32, b: i32, c: i32) {
        self.batch().indices.extend_from_slice(&[a, b, c]);
    }

    // -- plain (unsmoothed) fills ------------------------------------------------

    /// A convex quad in drawing order, one colour per corner.
    pub fn quad(&mut self, p: [Pt; 4], c: [Rgba; 4]) {
        self.quad_colors(p, c.map(Rgba::to_color));
    }

    fn quad_colors(&mut self, p: [Pt; 4], c: [Color; 4]) {
        let i0 = self.push(p[0], c[0]);
        let i1 = self.push(p[1], c[1]);
        let i2 = self.push(p[2], c[2]);
        let i3 = self.push(p[3], c[3]);
        self.index3(i0, i1, i2);
        self.index3(i0, i2, i3);
    }

    pub fn quad_flat(&mut self, p: [Pt; 4], c: Rgba) {
        self.quad_colors(p, [c.to_color(); 4]);
    }

    pub fn rect(&mut self, x: f32, y: f32, w: f32, h: f32, c: Rgba) {
        self.quad_flat([(x, y), (x + w, y), (x + w, y + h), (x, y + h)], c);
    }

    pub fn rect_vgrad(&mut self, x: f32, y: f32, w: f32, h: f32, top: Rgba, bottom: Rgba) {
        self.quad(
            [(x, y), (x + w, y), (x + w, y + h), (x, y + h)],
            [top, top, bottom, bottom],
        );
    }

    pub fn rect_hgrad(&mut self, x: f32, y: f32, w: f32, h: f32, left: Rgba, right: Rgba) {
        self.quad(
            [(x, y), (x + w, y), (x + w, y + h), (x, y + h)],
            [left, right, right, left],
        );
    }

    /// A textured quad (a glyph): screen rectangle and atlas rectangle.
    pub fn image(&mut self, dst: [f32; 4], uv: [f32; 4], c: Rgba) {
        let [x0, y0, x1, y1] = dst;
        let [u0, v0, u1, v1] = uv;
        let c = c.to_color();
        let a = self.push_uv((x0, y0), c, (u0, v0));
        let b = self.push_uv((x1, y0), c, (u1, v0));
        let d = self.push_uv((x1, y1), c, (u1, v1));
        let e = self.push_uv((x0, y1), c, (u0, v1));
        self.index3(a, b, d);
        self.index3(a, d, e);
    }

    // -- anti-aliased shapes -----------------------------------------------------

    /// Fill a convex polygon with smooth edges. Points may wind either way.
    pub fn convex(&mut self, points: &[Pt], c: Rgba) {
        self.convex_colors(points, None, c);
    }

    /// [`Painter::convex`] with one colour per point (`colors` same length).
    pub fn convex_shaded(&mut self, points: &[Pt], colors: &[Rgba]) {
        let fallback = colors.first().copied().unwrap_or(Rgba::WHITE);
        self.convex_colors(points, Some(colors), fallback);
    }

    fn convex_colors(&mut self, points: &[Pt], colors: Option<&[Rgba]>, c: Rgba) {
        let n = points.len();
        if n < 3 {
            return;
        }
        let flat = c.to_color();
        let color_at = |i: usize| {
            colors
                .and_then(|cs| cs.get(i).copied())
                .map_or(flat, Rgba::to_color)
        };
        // Winding: shoelace sign decides which side "outward" is.
        let mut area = 0.0;
        let (mut lo, mut hi) = (points[0], points[0]);
        for (&(x0, y0), &(x1, y1)) in points.iter().zip(points.iter().cycle().skip(1)) {
            area += x0 * y1 - x1 * y0;
            lo = (lo.0.min(x0), lo.1.min(y0));
            hi = (hi.0.max(x0), hi.1.max(y0));
        }
        if area.abs() < 0.01 {
            return;
        }
        if (hi.0 - lo.0).max(hi.1 - lo.1) < 3.0 {
            // A speck (a far tree, a distant sign): a fringe would cost more
            // vertices than the shape and change nothing anyone can see.
            let base = self.batch().vertices.len() as i32;
            for (i, &pt) in points.iter().enumerate() {
                self.push(pt, color_at(i));
            }
            for i in 1..(n as i32 - 1) {
                self.index3(base, base + i, base + i + 1);
            }
            return;
        }
        // A positive shoelace sum is clockwise on a y-down screen, and the
        // outward side of a clockwise edge (dx, dy) is (dy, -dx).
        let sign = if area > 0.0 { 1.0 } else { -1.0 };
        self.normals.clear();
        for i in 0..n {
            let (x0, y0) = points[i];
            let (x1, y1) = points[(i + 1) % n];
            let (dx, dy) = (x1 - x0, y1 - y0);
            let len = (dx * dx + dy * dy).sqrt().max(1e-6);
            self.normals.push((sign * dy / len, -sign * dx / len));
        }
        let half = self.aa * 0.5;
        let base = self.batch().vertices.len() as i32;
        for (i, &(x, y)) in points.iter().enumerate() {
            let prev = self.normals[(i + n - 1) % n];
            let next = self.normals[i];
            let (mut nx, mut ny) = ((prev.0 + next.0) * 0.5, (prev.1 + next.1) * 0.5);
            let d2 = (nx * nx + ny * ny).max(0.2);
            nx /= d2;
            ny /= d2;
            let col = color_at(i);
            let clear = Color { a: 0, ..col };
            self.push((x - nx * half, y - ny * half), col);
            self.push((x + nx * half, y + ny * half), clear);
        }
        for i in 1..(n as i32 - 1) {
            self.index3(base, base + 2 * i, base + 2 * (i + 1));
        }
        for i in 0..n as i32 {
            let j = (i + 1) % n as i32;
            let (ii, io) = (base + 2 * i, base + 2 * i + 1);
            let (ji, jo) = (base + 2 * j, base + 2 * j + 1);
            self.index3(ii, ji, jo);
            self.index3(ii, jo, io);
        }
    }

    /// A rounded rectangle, smooth-edged.
    pub fn round_rect(&mut self, x: f32, y: f32, w: f32, h: f32, r: f32, c: Rgba) {
        let pts = round_rect_points(&mut std::mem::take(&mut self.scratch), x, y, w, h, r);
        self.convex(&pts, c);
        self.scratch = pts;
    }

    /// A rounded rectangle with a vertical gradient.
    #[allow(clippy::too_many_arguments)]
    pub fn round_rect_vgrad(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        top: Rgba,
        bottom: Rgba,
    ) {
        let pts = round_rect_points(&mut std::mem::take(&mut self.scratch), x, y, w, h, r);
        let colors: Vec<Rgba> = pts
            .iter()
            .map(|p| top.mix(bottom, (p.1 - y) / h.max(1.0)))
            .collect();
        self.convex_shaded(&pts, &colors);
        self.scratch = pts;
    }

    /// The outline of a rounded rectangle, `width` pixels thick.
    #[allow(clippy::too_many_arguments)]
    pub fn round_rect_outline(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r: f32,
        width: f32,
        c: Rgba,
    ) {
        let pts = round_rect_points(&mut std::mem::take(&mut self.scratch), x, y, w, h, r);
        self.polyline(&pts, width, c, true);
        self.scratch = pts;
    }

    pub fn circle(&mut self, cx: f32, cy: f32, r: f32, c: Rgba) {
        if r <= 0.0 {
            return;
        }
        let mut pts = std::mem::take(&mut self.scratch);
        pts.clear();
        for &(cos, sin) in unit_circle(circle_segments(r)) {
            pts.push((cx + r * cos, cy + r * sin));
        }
        self.convex(&pts, c);
        self.scratch = pts;
    }

    pub fn ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, c: Rgba) {
        if rx <= 0.0 || ry <= 0.0 {
            return;
        }
        let mut pts = std::mem::take(&mut self.scratch);
        pts.clear();
        for &(cos, sin) in unit_circle(circle_segments(rx.max(ry))) {
            pts.push((cx + rx * cos, cy + ry * sin));
        }
        self.convex(&pts, c);
        self.scratch = pts;
    }

    /// A soft round light: `inner` at the centre fading to nothing at `r`.
    /// No fringe needed -- the edge is already transparent.
    pub fn glow(&mut self, cx: f32, cy: f32, r: f32, inner: Rgba) {
        if r <= 0.5 {
            return;
        }
        let segments = circle_segments(r).min(28);
        let (core, edge) = (inner.to_color(), inner.alpha(0.0).to_color());
        let centre = self.push((cx, cy), core);
        let first = self.batch().vertices.len() as i32;
        for &(cos, sin) in unit_circle(segments) {
            self.push((cx + r * cos, cy + r * sin), edge);
        }
        for i in 0..segments as i32 {
            let j = (i + 1) % segments as i32;
            self.index3(centre, first + i, first + j);
        }
    }

    /// An elliptical soft light.
    pub fn glow_ellipse(&mut self, cx: f32, cy: f32, rx: f32, ry: f32, inner: Rgba) {
        if rx <= 0.5 || ry <= 0.5 {
            return;
        }
        let segments = circle_segments(rx.max(ry)).min(28);
        let (core, edge) = (inner.to_color(), inner.alpha(0.0).to_color());
        let centre = self.push((cx, cy), core);
        let first = self.batch().vertices.len() as i32;
        for &(cos, sin) in unit_circle(segments) {
            self.push((cx + rx * cos, cy + ry * sin), edge);
        }
        for i in 0..segments as i32 {
            let j = (i + 1) % segments as i32;
            self.index3(centre, first + i, first + j);
        }
    }

    /// A ring segment (gauge arcs): angles in radians, 0 pointing right,
    /// growing clockwise on screen.
    #[allow(clippy::too_many_arguments)]
    pub fn arc(&mut self, cx: f32, cy: f32, r_in: f32, r_out: f32, a0: f32, a1: f32, c: Rgba) {
        if r_out <= r_in || (a1 - a0).abs() < 1e-4 {
            return;
        }
        let span = (a1 - a0).abs();
        let segments = ((span * r_out / 4.0).ceil() as usize).clamp(4, 96);
        let half = self.aa * 0.5;
        let base = self.batch().vertices.len() as i32;
        let (solid, clear) = (c.to_color(), c.alpha(0.0).to_color());
        for i in 0..=segments {
            let a = a0 + (a1 - a0) * i as f32 / segments as f32;
            let (sin, cos) = a.sin_cos();
            let at = |r: f32| (cx + r * cos, cy + r * sin);
            self.push(at(r_in - half), clear);
            self.push(at(r_in + half), solid);
            self.push(at(r_out - half), solid);
            self.push(at(r_out + half), clear);
        }
        for i in 0..segments as i32 {
            let a = base + i * 4;
            let b = a + 4;
            for k in 0..3 {
                self.index3(a + k, b + k, b + k + 1);
                self.index3(a + k, b + k + 1, a + k + 1);
            }
        }
    }

    /// A thick smooth line.
    pub fn line(&mut self, a: Pt, b: Pt, width: f32, c: Rgba) {
        self.polyline(&[a, b], width, c, false);
    }

    /// A thick smooth polyline (`closed` joins the last point to the first).
    pub fn polyline(&mut self, points: &[Pt], width: f32, c: Rgba, closed: bool) {
        let n = points.len();
        if n < 2 {
            return;
        }
        let segs = if closed { n } else { n - 1 };
        let core = (width * 0.5 - self.aa * 0.5).max(0.0);
        let fringe = core + self.aa;
        // Thin lines fade instead of thinning below a pixel.
        let c = if width < 1.0 { c.fade(width) } else { c };
        let (solid, clear) = (c.to_color(), c.alpha(0.0).to_color());
        for s in 0..segs {
            let p0 = points[s];
            let p1 = points[(s + 1) % n];
            let (dx, dy) = (p1.0 - p0.0, p1.1 - p0.1);
            let len = (dx * dx + dy * dy).sqrt();
            if len < 1e-4 {
                continue;
            }
            let (nx, ny) = (-dy / len, dx / len);
            // Extend each segment by the core half-width so joints overlap
            // rather than notch.
            let (ex, ey) = (dx / len * core * 0.5, dy / len * core * 0.5);
            let a = (p0.0 - ex, p0.1 - ey);
            let b = (p1.0 + ex, p1.1 + ey);
            let off = |p: Pt, d: f32| (p.0 + nx * d, p.1 + ny * d);
            self.quad_colors(
                [off(a, fringe), off(b, fringe), off(b, core), off(a, core)],
                [clear, clear, solid, solid],
            );
            self.quad_colors(
                [off(a, core), off(b, core), off(b, -core), off(a, -core)],
                [solid; 4],
            );
            self.quad_colors(
                [
                    off(a, -core),
                    off(b, -core),
                    off(b, -fringe),
                    off(a, -fringe),
                ],
                [solid, solid, clear, clear],
            );
        }
    }
}

/// Enough segments that the anti-aliased edge reads as round.
fn circle_segments(r: f32) -> usize {
    ((r * 0.6).ceil() as usize).clamp(8, 48)
}

/// Points around a unit circle for each segment count up to 64, computed
/// once instead of a sine and cosine per point per frame.
fn unit_circle(segments: usize) -> &'static [(f32, f32)] {
    static TABLES: std::sync::OnceLock<Vec<Vec<(f32, f32)>>> = std::sync::OnceLock::new();
    let tables = TABLES.get_or_init(|| {
        (0..=64usize)
            .map(|n| {
                (0..n)
                    .map(|i| {
                        let (sin, cos) = (i as f32 / n as f32 * std::f32::consts::TAU).sin_cos();
                        (cos, sin)
                    })
                    .collect()
            })
            .collect()
    });
    &tables[segments.min(64)]
}

/// Outline points of a rounded rectangle, clockwise from the top-left
/// corner, written into `out` (reused allocation).
fn round_rect_points(out: &mut Vec<Pt>, x: f32, y: f32, w: f32, h: f32, r: f32) -> Vec<Pt> {
    out.clear();
    let r = r.min(w * 0.5).min(h * 0.5).max(0.0);
    let steps = ((r * 0.5).ceil() as usize).clamp(1, 10);
    let corners = [
        (x + w - r, y + r, -std::f32::consts::FRAC_PI_2),
        (x + w - r, y + h - r, 0.0),
        (x + r, y + h - r, std::f32::consts::FRAC_PI_2),
        (x + r, y + r, std::f32::consts::PI),
    ];
    for (cx, cy, start) in corners {
        for i in 0..=steps {
            let a = start + std::f32::consts::FRAC_PI_2 * i as f32 / steps as f32;
            out.push((cx + r * a.cos(), cy + r * a.sin()));
        }
    }
    std::mem::take(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn indices_are_in_range(p: &Painter) -> bool {
        p.batches
            .iter()
            .all(|b| b.indices.iter().all(|&i| (i as usize) < b.vertices.len()))
    }

    #[test]
    fn every_shape_emits_whole_triangles_with_valid_indices() {
        let mut p = Painter::new((1.0, 1.0));
        p.rect(0.0, 0.0, 10.0, 10.0, Rgba::WHITE);
        p.round_rect(0.0, 0.0, 40.0, 20.0, 6.0, Rgba::BLACK);
        p.circle(50.0, 50.0, 12.0, Rgba::WHITE);
        p.arc(50.0, 50.0, 30.0, 34.0, 0.0, 3.0, Rgba::WHITE);
        p.set_layer(Layer::Glow);
        p.glow(10.0, 10.0, 20.0, Rgba::WHITE);
        p.set_layer(Layer::Hud);
        p.polyline(
            &[(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)],
            3.0,
            Rgba::WHITE,
            true,
        );
        p.convex(&[(0.0, 0.0), (0.0, 10.0), (10.0, 10.0)], Rgba::WHITE);
        for batch in &p.batches {
            assert_eq!(batch.indices.len() % 3, 0);
            assert!(!batch.indices.is_empty());
        }
        assert!(indices_are_in_range(&p));
    }

    #[test]
    fn a_degenerate_polygon_draws_nothing() {
        let mut p = Painter::new((1.0, 1.0));
        p.convex(&[(0.0, 0.0), (5.0, 0.0), (10.0, 0.0)], Rgba::WHITE);
        p.convex(&[(0.0, 0.0), (5.0, 5.0)], Rgba::WHITE);
        assert_eq!(p.vertex_count(), 0);
    }

    #[test]
    fn the_fringe_fades_out_and_the_core_keeps_its_colour() {
        let mut p = Painter::new((1.0, 1.0));
        p.convex(
            &[(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)],
            Rgba::WHITE,
        );
        let alphas: Vec<u8> = p.batches[0].vertices.iter().map(|v| v.color.a).collect();
        assert_eq!(alphas.iter().filter(|&&a| a == 255).count(), 4);
        assert_eq!(alphas.iter().filter(|&&a| a == 0).count(), 4);
        // Either winding: the transparent ring sits outside the square and
        // the solid core inside it.
        for square in [
            [(0.0, 0.0), (20.0, 0.0), (20.0, 20.0), (0.0, 20.0)],
            [(0.0, 0.0), (0.0, 20.0), (20.0, 20.0), (20.0, 0.0)],
        ] {
            let mut p = Painter::new((1.0, 1.0));
            p.convex(&square, Rgba::WHITE);
            for v in &p.batches[0].vertices {
                let (x, y) = (v.position.x, v.position.y);
                let inside = (0.0..=20.0).contains(&x) && (0.0..=20.0).contains(&y);
                assert_eq!(inside, v.color.a == 255, "{x},{y} alpha {}", v.color.a);
            }
        }
    }

    #[test]
    fn colour_mixing_stays_in_range() {
        let a = Rgba::rgb(10, 20, 30);
        let b = Rgba::rgb(250, 240, 230);
        assert_eq!(a.mix(b, 0.0), a);
        assert_eq!(a.mix(b, 1.0), b);
        assert_eq!(a.mix(b, 7.0), b);
        assert_eq!(
            Rgba::WHITE.shade(3.0).to_color(),
            Color::RGBA(255, 255, 255, 255)
        );
    }
}
