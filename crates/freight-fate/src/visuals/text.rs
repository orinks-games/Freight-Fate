//! Text for the visual display: Overpass (an open-licence face drawn after
//! the US highway sign alphabet, so the HUD's signs read like real ones),
//! rasterized glyph by glyph into one atlas texture.
//!
//! Overpass covers Latin and Cyrillic. Chinese and Japanese -- which the
//! game speaks, and so captions show -- come from a font the operating
//! system already has, memory-mapped the first time such a character is
//! drawn, so a player who never needs them never pays for them.

use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hash, Hasher};
use std::sync::Arc;

use ab_glyph::{Font, FontArc, FontRef, GlyphId, PxScale};
use ff_core::l10n::Translator;

use super::draw::{Painter, Rgba};

const REGULAR: &[u8] = include_bytes!("../../../../assets/fonts/Overpass-Regular.ttf");
const BOLD: &[u8] = include_bytes!("../../../../assets/fonts/Overpass-Bold.ttf");

/// Atlas edge in pixels. 1024 square holds every glyph a frame of this UI
/// draws at Retina sizes; when it fills, the cache starts over.
pub const ATLAS_SIZE: u32 = 1024;
/// The solid white block in the atlas's corner that plain shapes sample.
const WHITE_BLOCK: u32 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Bold,
}

/// System fonts tried, in order, for a character Overpass lacks.
const FALLBACK_FONTS: &[(&str, u32)] = &[
    // macOS
    ("/System/Library/Fonts/Hiragino Sans GB.ttc", 0),
    ("/System/Library/Fonts/STHeiti Medium.ttc", 0),
    ("/System/Library/Fonts/Supplemental/Arial Unicode.ttf", 0),
    ("/Library/Fonts/Arial Unicode.ttf", 0),
    // Windows
    ("C:\\Windows\\Fonts\\msyh.ttc", 0),
    ("C:\\Windows\\Fonts\\YuGothM.ttc", 0),
    ("C:\\Windows\\Fonts\\meiryo.ttc", 0),
    ("C:\\Windows\\Fonts\\simsun.ttc", 0),
    ("C:\\Windows\\Fonts\\seguisym.ttf", 0),
    // Linux
    ("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc", 0),
    ("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc", 0),
    (
        "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
        0,
    ),
    ("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc", 0),
    (
        "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
        0,
    ),
    (
        "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        0,
    ),
    ("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf", 0),
];

/// Where one rasterized glyph lives in the atlas and how to place it.
#[derive(Clone, Copy, Debug)]
struct Cached {
    /// Atlas rectangle, pixels: x, y, w, h. Zero size for blank glyphs.
    rect: [u32; 4],
    /// Offset from the pen position (baseline) to the bitmap's top-left.
    offset: (f32, f32),
}

/// Shelf packing: rows of glyphs, each row as tall as its tallest glyph.
struct Shelves {
    x: u32,
    y: u32,
    row_h: u32,
}

impl Shelves {
    fn new() -> Shelves {
        // The first row starts beside the white block.
        Shelves {
            x: WHITE_BLOCK + 2,
            y: 0,
            row_h: WHITE_BLOCK + 2,
        }
    }

    fn place(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (w, h) = (w + 1, h + 1); // one pixel of air against bleeding
        if w > ATLAS_SIZE || h > ATLAS_SIZE {
            return None;
        }
        if self.x + w > ATLAS_SIZE {
            self.y += self.row_h;
            self.x = 0;
            self.row_h = 0;
        }
        if self.y + h > ATLAS_SIZE {
            return None;
        }
        let at = (self.x, self.y);
        self.x += w;
        self.row_h = self.row_h.max(h);
        Some(at)
    }
}

/// The CPU copy of the atlas, plus the region changed since the last upload.
pub struct Atlas {
    pub pixels: Vec<u8>,
    dirty: Option<[u32; 4]>,
    shelves: Shelves,
}

impl Atlas {
    fn new() -> Atlas {
        let mut atlas = Atlas {
            pixels: vec![0; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize],
            dirty: None,
            shelves: Shelves::new(),
        };
        atlas.reset();
        atlas
    }

    fn reset(&mut self) {
        self.pixels.fill(0);
        for y in 0..WHITE_BLOCK {
            for x in 0..WHITE_BLOCK {
                self.put(x, y, 255);
            }
        }
        self.shelves = Shelves::new();
        self.dirty = Some([0, 0, ATLAS_SIZE, ATLAS_SIZE]);
    }

    fn put(&mut self, x: u32, y: u32, coverage: u8) {
        let i = ((y * ATLAS_SIZE + x) * 4) as usize;
        // White everywhere; coverage is the alpha. Byte order is moot
        // because the colour bytes are all the same.
        self.pixels[i..i + 4].copy_from_slice(&[255, 255, 255, coverage]);
    }

    fn mark(&mut self, x: u32, y: u32, w: u32, h: u32) {
        self.dirty = Some(match self.dirty {
            None => [x, y, x + w, y + h],
            Some([x0, y0, x1, y1]) => [x0.min(x), y0.min(y), x1.max(x + w), y1.max(y + h)],
        });
    }

    /// Everything needs uploading again (a new texture).
    pub fn mark_all(&mut self) {
        self.dirty = Some([0, 0, ATLAS_SIZE, ATLAS_SIZE]);
    }

    /// The changed region since the last call, as x, y, w, h.
    pub fn take_dirty(&mut self) -> Option<[u32; 4]> {
        self.dirty
            .take()
            .map(|[x0, y0, x1, y1]| [x0, y0, x1 - x0, y1 - y0])
    }

    pub fn white_uv() -> (f32, f32) {
        let centre = WHITE_BLOCK as f32 * 0.5 / ATLAS_SIZE as f32;
        (centre, centre)
    }
}

/// A face that can draw a character: the bundled pair, or a system font.
///
/// The ASCII glyphs and the size scale are looked up once: every frame
/// measures and draws hundreds of characters, and ab_glyph's scaled-font
/// helpers re-read the font's tables on every advance.
struct Face {
    font: FontArc,
    ascii: [GlyphId; 128],
    /// Pixels per font unit, per pixel of size (a `PxScale` is the
    /// ascent-to-descent height).
    per_px: f32,
    ascent: f32,
    descent: f32,
}

impl Face {
    fn new(font: FontArc) -> Face {
        let mut ascii = [GlyphId(0); 128];
        for (code, slot) in ascii.iter_mut().enumerate() {
            *slot = font.glyph_id(char::from(code as u8));
        }
        let (ascent, descent) = (font.ascent_unscaled(), font.descent_unscaled());
        Face {
            ascii,
            per_px: 1.0 / (ascent - descent).max(1.0),
            ascent,
            descent,
            font,
        }
    }

    fn glyph_id(&self, ch: char) -> GlyphId {
        match self.ascii.get(ch as usize) {
            Some(&glyph) => glyph,
            None => self.font.glyph_id(ch),
        }
    }

    fn advance(&self, glyph: GlyphId, size: f32) -> f32 {
        self.font.h_advance_unscaled(glyph) * size * self.per_px
    }

    fn kern(&self, a: GlyphId, b: GlyphId, size: f32) -> f32 {
        self.font.kern_unscaled(a, b) * size * self.per_px
    }
}

/// FxHash: the glyph and wrap caches are hit thousands of times a frame
/// with small keys, where the default SipHash costs more than the lookup.
#[derive(Default, Clone, Copy)]
struct Fx(u64);

impl Fx {
    fn add(&mut self, v: u64) {
        self.0 = (self.0.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }
}

impl Hasher for Fx {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for chunk in bytes.chunks(8) {
            let mut word = [0u8; 8];
            word[..chunk.len()].copy_from_slice(chunk);
            self.add(u64::from_le_bytes(word));
        }
    }
    fn write_u8(&mut self, n: u8) {
        self.add(n as u64);
    }
    fn write_u16(&mut self, n: u16) {
        self.add(n as u64);
    }
    fn write_u32(&mut self, n: u32) {
        self.add(n as u64);
    }
    fn write_u64(&mut self, n: u64) {
        self.add(n);
    }
    fn write_usize(&mut self, n: usize) {
        self.add(n as u64);
    }
}

type FxMap<K, V> = HashMap<K, V, BuildHasherDefault<Fx>>;

/// A wrap remembered: the text's hash, size, weight and width, in tenths.
type WrapKey = (u64, u32, Weight, u32);

pub struct Text {
    regular: Face,
    bold: Face,
    /// System fonts opened so far, in [`FALLBACK_FONTS`] order; `None`
    /// until the first character Overpass cannot draw.
    fallbacks: Option<Vec<Face>>,
    /// Which face draws a character Overpass lacks (index into
    /// `fallbacks`), remembered per character.
    fallback_for: HashMap<char, Option<usize>>,
    cache: FxMap<(u8, GlyphId, u16), Cached>,
    /// Lines already wrapped: captions and menu rows wrap the same text
    /// frame after frame.
    wraps: FxMap<WrapKey, Vec<String>>,
    pub atlas: Atlas,
    /// Set when the atlas filled up mid-frame: the next frame starts it over.
    overflowed: bool,
    /// The player's language, for the labels the display draws itself.
    translator: Arc<Translator>,
}

/// Which face drew a glyph: 0 regular, 1 bold, 2.. fallback index + 2.
type FaceId = u8;

impl Default for Text {
    fn default() -> Self {
        Self::new()
    }
}

impl Text {
    pub fn new() -> Text {
        let face = |bytes: &'static [u8]| {
            Face::new(FontArc::new(
                FontRef::try_from_slice(bytes).expect("bundled Overpass parses"),
            ))
        };
        Text {
            regular: face(REGULAR),
            bold: face(BOLD),
            fallbacks: None,
            fallback_for: HashMap::new(),
            cache: FxMap::default(),
            wraps: FxMap::default(),
            atlas: Atlas::new(),
            overflowed: false,
            translator: Arc::new(Translator::english()),
        }
    }

    /// A label the display draws itself, in the player's language. Every
    /// literal handed to a `tr_drawn` call is extracted for translation,
    /// whatever its shape ("SPEED", "CRUISE {value}").
    pub fn tr_drawn(&self, english: &str) -> String {
        self.translator.translate(english).into_owned()
    }

    /// Draw labels in `translator`'s language from now on. True when the
    /// language changed, so the frame on screen is stale.
    pub fn set_translator(&mut self, translator: &Arc<Translator>) -> bool {
        if Arc::ptr_eq(&self.translator, translator) {
            return false;
        }
        self.translator = Arc::clone(translator);
        true
    }

    /// Start a frame: an atlas that filled up last frame is cleared now,
    /// before anything of this frame references it.
    pub fn begin_frame(&mut self) {
        if self.overflowed {
            self.overflowed = false;
            self.cache.clear();
            self.atlas.reset();
        }
        if self.wraps.len() > 512 {
            self.wraps.clear();
        }
    }

    fn face(&self, id: FaceId) -> &Face {
        match id {
            0 => &self.regular,
            1 => &self.bold,
            n => &self
                .fallbacks
                .as_ref()
                .expect("a fallback id is only handed out once they are open")[(n - 2) as usize],
        }
    }

    /// The face that draws `ch` in `weight`, and its glyph.
    fn resolve(&mut self, ch: char, weight: Weight) -> (FaceId, GlyphId) {
        let primary: FaceId = if weight == Weight::Bold { 1 } else { 0 };
        let glyph = self.face(primary).glyph_id(ch);
        if glyph.0 != 0 || ch.is_whitespace() || ch.is_control() {
            return (primary, glyph);
        }
        let index = match self.fallback_for.get(&ch) {
            Some(found) => *found,
            None => {
                let fallbacks = self.fallbacks.get_or_insert_with(open_fallbacks);
                let found = fallbacks.iter().position(|f| f.glyph_id(ch).0 != 0);
                self.fallback_for.insert(ch, found);
                found
            }
        };
        match index {
            Some(i) => {
                let id = (i + 2) as FaceId;
                (id, self.face(id).glyph_id(ch))
            }
            None => (primary, glyph), // nobody has it: Overpass's empty box
        }
    }

    /// Height of a line of text at `size` pixels: ascent and descent.
    pub fn metrics(&self, size: f32) -> (f32, f32) {
        let face = &self.regular;
        let k = size * face.per_px;
        (face.ascent * k, -face.descent * k)
    }

    /// Width in pixels of `text` at `size` in `weight`.
    /// The most `text` can measure on any platform: a character Overpass
    /// lacks is drawn by whatever system font is installed (Hiragino on a
    /// Mac, Yu Gothic or Microsoft YaHei on Windows, Noto CJK on Linux), so
    /// it counts as at least a full em, the width of a CJK ideograph or
    /// kana in every one of them. The fit tests use this so a label that
    /// fits on one platform's fallback font fits on the others too.
    #[cfg(test)]
    pub(super) fn measure_on_any_platform(&mut self, text: &str, size: f32, weight: Weight) -> f32 {
        let primary: FaceId = if weight == Weight::Bold { 1 } else { 0 };
        let mut width = self.measure(text, size, weight);
        for ch in text.chars() {
            let covered = self.face(primary).glyph_id(ch).0 != 0;
            if !covered && !ch.is_whitespace() && !ch.is_control() {
                let mut buf = [0u8; 4];
                let here = self.measure(ch.encode_utf8(&mut buf), size, weight);
                width += (size - here).max(0.0);
            }
        }
        width
    }

    pub fn measure(&mut self, text: &str, size: f32, weight: Weight) -> f32 {
        let mut width = 0.0;
        let mut prev: Option<(FaceId, GlyphId)> = None;
        for ch in text.chars() {
            let (face_id, glyph) = self.resolve(ch, weight);
            let face = self.face(face_id);
            if let Some((pf, pg)) = prev {
                if pf == face_id {
                    width += face.kern(pg, glyph, size);
                }
            }
            width += face.advance(glyph, size);
            prev = Some((face_id, glyph));
        }
        width
    }

    /// Draw `text` with its baseline at `y`, starting at `x`; returns the
    /// width drawn. Positions snap to whole pixels so glyphs stay crisp.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        painter: &mut Painter,
        x: f32,
        y: f32,
        text: &str,
        size: f32,
        weight: Weight,
        color: Rgba,
    ) -> f32 {
        let size_q = size.round().clamp(4.0, 400.0) as u16;
        self.draw_at(painter, x, y, text, size_q, 1.0, weight, color)
    }

    /// Text on something moving in the world (a sign growing as the truck
    /// nears it): rasterized at the next size up a fixed ladder and scaled
    /// down to fit, so an approaching sign reuses a handful of glyph sizes
    /// instead of rasterizing a new one every frame.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_scaled(
        &mut self,
        painter: &mut Painter,
        x: f32,
        y: f32,
        text: &str,
        size: f32,
        weight: Weight,
        color: Rgba,
    ) -> f32 {
        if size < 3.0 {
            return 0.0;
        }
        const LADDER: [u16; 14] = [6, 8, 10, 12, 14, 17, 20, 24, 29, 35, 42, 50, 60, 72];
        let raster = LADDER
            .iter()
            .copied()
            .find(|&s| s as f32 >= size)
            .unwrap_or(LADDER[LADDER.len() - 1]);
        let k = size / raster as f32;
        self.draw_at(painter, x, y, text, raster, k, weight, color)
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_at(
        &mut self,
        painter: &mut Painter,
        x: f32,
        y: f32,
        text: &str,
        size_q: u16,
        k: f32,
        weight: Weight,
        color: Rgba,
    ) -> f32 {
        let size = size_q as f32;
        let snap = |v: f32| if k == 1.0 { v.round() } else { v };
        let mut pen = snap(x);
        let baseline = snap(y);
        let mut prev: Option<(FaceId, GlyphId)> = None;
        let inv = 1.0 / ATLAS_SIZE as f32;
        for ch in text.chars() {
            let (face, glyph) = self.resolve(ch, weight);
            let (advance, kern) = {
                let f = self.face(face);
                let kern = match prev {
                    Some((pf, pg)) if pf == face => f.kern(pg, glyph, size),
                    _ => 0.0,
                };
                (f.advance(glyph, size), kern)
            };
            pen += kern * k;
            if let Some(cached) = self.glyph(face, glyph, size_q) {
                let [ax, ay, w, h] = cached.rect;
                if w > 0 && h > 0 {
                    let gx = snap(pen + cached.offset.0 * k);
                    let gy = baseline + cached.offset.1 * k;
                    painter.image(
                        [gx, gy, gx + w as f32 * k, gy + h as f32 * k],
                        [
                            ax as f32 * inv,
                            ay as f32 * inv,
                            (ax + w) as f32 * inv,
                            (ay + h) as f32 * inv,
                        ],
                        color,
                    );
                }
            }
            pen += advance * k;
            prev = Some((face, glyph));
        }
        pen - snap(x)
    }

    /// The cached glyph, rasterizing it into the atlas on first use.
    fn glyph(&mut self, face: FaceId, glyph: GlyphId, size: u16) -> Option<Cached> {
        if let Some(found) = self.cache.get(&(face, glyph, size)) {
            return Some(*found);
        }
        if self.overflowed {
            return None;
        }
        let font = self.face(face).font.clone();
        let positioned =
            glyph.with_scale_and_position(PxScale::from(size as f32), (0.0f32, 0.0f32));
        let Some(outline) = font.outline_glyph(positioned) else {
            let blank = Cached {
                rect: [0, 0, 0, 0],
                offset: (0.0, 0.0),
            };
            self.cache.insert((face, glyph, size), blank);
            return Some(blank);
        };
        let bounds = outline.px_bounds();
        let (w, h) = (bounds.width() as u32, bounds.height() as u32);
        let Some((ax, ay)) = self.atlas.shelves.place(w, h) else {
            self.overflowed = true;
            return None;
        };
        outline.draw(|x, y, coverage| {
            let value = (coverage.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            if x < w && y < h {
                self.atlas.put(ax + x, ay + y, value);
            }
        });
        self.atlas.mark(ax, ay, w, h);
        let cached = Cached {
            rect: [ax, ay, w, h],
            offset: (bounds.min.x, bounds.min.y),
        };
        self.cache.insert((face, glyph, size), cached);
        Some(cached)
    }

    /// Break `text` into lines no wider than `max_w`, at spaces where it
    /// can, mid-word where a word alone is too wide. Ideographic text
    /// (no spaces) breaks between characters.
    pub fn wrap(&mut self, text: &str, size: f32, weight: Weight, max_w: f32) -> Vec<String> {
        let mut hasher = Fx::default();
        text.hash(&mut hasher);
        let key = (
            hasher.finish(),
            (size * 10.0) as u32,
            weight,
            (max_w * 10.0).max(0.0) as u32,
        );
        if let Some(lines) = self.wraps.get(&key) {
            return lines.clone();
        }
        let lines = self.wrap_uncached(text, size, weight, max_w);
        self.wraps.insert(key, lines.clone());
        lines
    }

    fn wrap_uncached(&mut self, text: &str, size: f32, weight: Weight, max_w: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            // The line's width so far, each word measured once.
            let mut line_w = 0.0;
            for word in split_keep_spaces(paragraph) {
                let inked = self.measure(word.trim_end(), size, weight);
                if line.is_empty() {
                    if inked > max_w {
                        // A single word wider than the line: break it.
                        for ch in word.chars() {
                            let ch_w = self.measure(&ch.to_string(), size, weight);
                            if !line.is_empty() && line_w + ch_w > max_w {
                                lines.push(std::mem::take(&mut line));
                                line_w = 0.0;
                            }
                            line.push(ch);
                            line_w += ch_w;
                        }
                    } else {
                        line_w = self.measure(&word, size, weight);
                        line.push_str(&word);
                    }
                } else if line_w + inked <= max_w {
                    line_w += self.measure(&word, size, weight);
                    line.push_str(&word);
                } else {
                    lines.push(line.trim_end().to_string());
                    line = word.trim_start().to_string();
                    line_w = self.measure(&line, size, weight);
                }
            }
            lines.push(line.trim_end().to_string());
        }
        lines
    }

    /// `text`, cut short with an ellipsis if it is wider than `max_w`.
    pub fn ellipsize(&mut self, text: &str, size: f32, weight: Weight, max_w: f32) -> String {
        if self.measure(text, size, weight) <= max_w {
            return text.to_string();
        }
        let mut out = String::new();
        for ch in text.chars() {
            let next = format!("{out}{ch}…");
            if self.measure(&next, size, weight) > max_w {
                break;
            }
            out.push(ch);
        }
        format!("{}…", out.trim_end())
    }
}

/// Words with the spaces after them; CJK characters each count as a word.
fn split_keep_spaces(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut word = String::new();
    for ch in text.chars() {
        let wide = is_wide(ch);
        if wide && !word.is_empty() {
            out.push(std::mem::take(&mut word));
        }
        word.push(ch);
        if ch == ' ' || wide {
            out.push(std::mem::take(&mut word));
        }
    }
    if !word.is_empty() {
        out.push(word);
    }
    out
}

fn is_wide(ch: char) -> bool {
    matches!(ch as u32,
        0x3000..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xF900..=0xFAFF | 0xFF00..=0xFFEF)
}

/// Memory-map every system fallback font that is there. Mapped, not read:
/// these files run to tens of megabytes and only their glyph tables for the
/// characters actually drawn are ever touched.
fn open_fallbacks() -> Vec<Face> {
    let mut out = Vec::new();
    for (path, index) in FALLBACK_FONTS {
        let Ok(file) = std::fs::File::open(path) else {
            continue;
        };
        // SAFETY: the map is read-only and the file is a system font that
        // nothing in the game writes. A font replaced under a running game
        // would at worst draw a wrong glyph, the same as any reader of a
        // file that changes underneath it.
        let Ok(map) = (unsafe { memmap2::Mmap::map(&file) }) else {
            continue;
        };
        // Kept for the life of the process, which is how long the font is
        // used: there is one of these per installed fallback font at most.
        let bytes: &'static [u8] = Box::leak(Box::new(map));
        match FontRef::try_from_slice_and_index(bytes, *index) {
            Ok(font) => {
                log::info!("visuals: fallback font {path}");
                out.push(Face::new(FontArc::new(font)));
            }
            Err(e) => log::debug!("visuals: fallback font {path} unreadable: {e}"),
        }
    }
    if out.is_empty() {
        log::info!("visuals: no system fallback font found; some characters show as boxes");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latin_and_cyrillic_come_from_the_bundled_face() {
        let mut text = Text::new();
        assert_eq!(text.resolve('A', Weight::Regular).0, 0);
        assert_eq!(text.resolve('Ж', Weight::Bold).0, 1);
        assert!(text.fallbacks.is_none(), "no system font opened for these");
    }

    #[test]
    fn drawing_text_fills_the_atlas_and_emits_one_quad_per_inked_glyph() {
        let mut text = Text::new();
        let mut painter = Painter::new(Atlas::white_uv());
        text.atlas.take_dirty();
        painter.set_layer(super::super::draw::Layer::Hud);
        let width = text.draw(
            &mut painter,
            10.0,
            40.0,
            "Exit 42",
            20.0,
            Weight::Bold,
            Rgba::WHITE,
        );
        assert!(width > 40.0);
        // Six inked glyphs (the space has no bitmap), four vertices each.
        assert_eq!(painter.batches[2].vertices.len(), 6 * 4);
        assert!(text.atlas.take_dirty().is_some());
        // Drawn again: everything comes from the cache, nothing new to upload.
        text.draw(
            &mut painter,
            10.0,
            80.0,
            "Exit 42",
            20.0,
            Weight::Bold,
            Rgba::WHITE,
        );
        assert!(text.atlas.take_dirty().is_none());
    }

    #[test]
    fn wrapping_keeps_words_whole_and_lines_inside_the_width() {
        let mut text = Text::new();
        let lines = text.wrap(
            "Speed limit drops to 35 in 2 miles.",
            16.0,
            Weight::Regular,
            120.0,
        );
        assert!(lines.len() >= 2, "{lines:?}");
        for line in &lines {
            assert!(
                text.measure(line, 16.0, Weight::Regular) <= 120.0 + 0.5,
                "{line}"
            );
        }
        assert_eq!(lines.join(" "), "Speed limit drops to 35 in 2 miles.");
    }

    #[test]
    fn a_long_label_is_cut_with_an_ellipsis() {
        let mut text = Text::new();
        let cut = text.ellipsize(
            "Continue career: a very long driver name indeed",
            16.0,
            Weight::Regular,
            100.0,
        );
        assert!(cut.ends_with('…'));
        assert!(text.measure(&cut, 16.0, Weight::Regular) <= 100.0);
    }

    #[test]
    fn the_shelf_packer_refuses_once_the_atlas_is_full() {
        let mut shelves = Shelves::new();
        let mut placed = 0;
        while shelves.place(200, 200).is_some() {
            placed += 1;
        }
        assert!(placed >= 16, "a 1024 atlas holds at least 16 such glyphs");
        assert!(shelves.place(2000, 10).is_none());
    }
}
