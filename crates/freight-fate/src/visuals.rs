//! The visual display: what the game window shows, so a sighted player can
//! drive by eye as well as by ear, and a low-vision player has large,
//! high-contrast text to back up the speech.
//!
//! Until this existed the window was a blank fill and the game lived
//! entirely in speech and sound; all of that is unchanged. The display only
//! reads the game -- [`scene::Scene::capture`] copies what to show out of
//! the screen stack each frame -- and never feeds anything back into it.
//!
//! * `scene`, `scene_drive` -- the copy: menus and readers as panels, the
//!   drive as road shape, traffic, signs, weather and dashboard readings.
//! * `road`, `scenery`, `signs`, `vehicles`, `sky` -- the road view through
//!   a chase camera behind the truck.
//! * `hud`, `screens`, `captions` -- the dashboard, menus, and what the
//!   game just said.
//! * `draw`, `text`, `palette` -- triangles, glyphs, and light.
//!
//! Cost is kept down on purpose. A frame is three draw calls whatever it
//! shows. A screen that would look exactly like the last frame is not drawn
//! or presented at all, so a menu costs nothing between key presses, and
//! the shell skips drawing entirely while the window is minimized or, on a
//! Mac, completely covered.

mod captions;
pub mod draw;
#[cfg(test)]
mod fit_tests;
mod hud;
mod palette;
mod road;
pub mod scene;
pub mod scene_drive;
mod scenery;
mod screens;
mod signs;
mod sky;
pub mod text;
mod vehicles;

use std::sync::Arc;
use std::time::Instant;

use ff_core::l10n::Translator;
use ff_core::message_log::MessageLog;

use captions::Captions;
use draw::{Batch, Painter, Rgba, LAYERS};
use road::{Camera, Geom, RoadFrame, RoadState};
pub use scene::{Panel, Scene, Screen};
pub use scene_drive::DriveView;
use screens::Placement;
use sky::SkyFrame;
use text::{Atlas, Text};

/// Background before anything is drawn (the game's old window colour).
const CLEAR: Rgba = Rgba::rgb(12, 12, 16);

/// A label read off the game while capturing a scene, in the player's
/// language. Like [`text::Text::tr_drawn`], every literal handed to a
/// `tr_drawn` call is extracted for translation.
pub fn tr_drawn(translator: &Translator, english: &str) -> String {
    translator.translate(english).into_owned()
}

pub struct Visuals {
    text: Text,
    painter: Painter,
    captions: Captions,
    road: RoadState,
    started: Instant,
    last_frame: Option<Instant>,
    last_scene: Option<Scene>,
    last_size: (u32, u32),
    /// Heading for the sky when the route has no coordinates: turned by
    /// the curves the truck drives through.
    drift_heading: f32,
    last_position_ft: Option<f64>,
    /// The language changed since the last frame: draw it again.
    relabel: bool,
}

impl Default for Visuals {
    fn default() -> Self {
        Self::new()
    }
}

impl Visuals {
    pub fn new() -> Visuals {
        Visuals {
            text: Text::new(),
            painter: Painter::new(Atlas::white_uv()),
            captions: Captions::default(),
            road: RoadState::default(),
            started: Instant::now(),
            last_frame: None,
            last_scene: None,
            last_size: (0, 0),
            drift_heading: 4.7,
            last_position_ft: None,
            relabel: false,
        }
    }

    /// Draw the display's own labels in `translator`'s language.
    pub fn set_translator(&mut self, translator: &Arc<Translator>) {
        self.relabel |= self.text.set_translator(translator);
    }

    /// Build this frame's geometry for a `size` pixel canvas. Returns false
    /// when the picture would be identical to the one already on screen,
    /// in which case nothing was built and nothing needs presenting.
    pub fn build(&mut self, scene: Scene, log: &MessageLog, size: (u32, u32), force: bool) -> bool {
        let now = Instant::now();
        let added = self.captions.sync(log, now);
        let fading = self.captions.tick(now);
        let force = force || std::mem::take(&mut self.relabel);
        let unchanged = !force
            && !added
            && !fading
            && size == self.last_size
            && !scene.animates()
            && self.last_scene.as_ref() == Some(&scene);
        if unchanged {
            return false;
        }
        let dt = self
            .last_frame
            .map_or(0.0, |t| now.duration_since(t).as_secs_f32().min(0.25));
        self.last_frame = Some(now);
        self.last_size = size;
        let (w, h) = (size.0 as f32, size.1 as f32);
        let t = now.duration_since(self.started).as_secs_f32();
        if let Screen::Drive { view, .. } = &scene.screen {
            self.drift(view);
        }
        self.painter.clear();
        self.text.begin_frame();
        let p = &mut self.painter;
        p.rect(0.0, 0.0, w, h, CLEAR);
        match &scene.screen {
            Screen::Blank => {}
            Screen::Drive { view, overlay } => {
                draw_drive(
                    p,
                    &mut self.text,
                    &mut self.road,
                    view,
                    self.drift_heading,
                    (w, h),
                    dt,
                    t,
                    true,
                );
                if let Some(panel) = overlay {
                    screens::scrim(p, w, h, Placement::Right, 1.0);
                    screens::panel(p, &mut self.text, panel, w, h, Placement::Right);
                }
            }
            Screen::Panel(panel) => {
                let backdrop = screens::showcase();
                let mut still = RoadState::default();
                draw_drive(
                    p,
                    &mut self.text,
                    &mut still,
                    &backdrop,
                    0.0,
                    (w, h),
                    0.0,
                    0.0,
                    false,
                );
                screens::scrim(p, w, h, Placement::Left, 1.0);
                screens::brand(p, &mut self.text, w, h);
                screens::panel(p, &mut self.text, panel, w, h, Placement::Left);
            }
        }
        // Captions clear the dashboard whenever the road is on screen.
        let on_road = matches!(&scene.screen, Screen::Drive { .. });
        screens::captions(p, &mut self.text, &self.captions, w, h, on_road, now);
        if let Some(notice) = &scene.achievement {
            let at_wheel = matches!(&scene.screen, Screen::Drive { overlay: None, .. });
            screens::achievement(p, &mut self.text, notice, w, h, at_wheel);
        }
        self.last_scene = Some(scene);
        true
    }

    /// Turn the fallback sky heading with the road.
    fn drift(&mut self, view: &DriveView) {
        let pos = view.position_ft;
        if let Some(last) = self.last_position_ft {
            let moved = (pos - last) as f32;
            if (0.0..600.0).contains(&moved) {
                let at_truck =
                    ((0.0 - scene_drive::ROAD_START_FT) / scene_drive::ROAD_STEP_FT) as usize;
                let k = view.road.curvature.get(at_truck).copied().unwrap_or(0.0);
                self.drift_heading =
                    (self.drift_heading + k * moved).rem_euclid(std::f32::consts::TAU);
            }
        }
        self.last_position_ft = Some(pos);
    }

    /// The triangles of the frame just built, one batch per layer.
    pub fn batches(&self) -> &[Batch; LAYERS] {
        &self.painter.batches
    }

    /// The glyph atlas and the part of it changed since the last upload.
    pub fn atlas(&mut self) -> (&[u8], Option<[u32; 4]>) {
        let dirty = self.text.atlas.take_dirty();
        (&self.text.atlas.pixels, dirty)
    }

    /// The next upload sends the whole atlas (it is going to a new texture).
    pub fn invalidate_atlas(&mut self) {
        self.text.atlas.mark_all();
    }
}

/// Upload whatever the glyph atlas gained, then draw the frame just built
/// onto `canvas` (the window, or an offscreen surface). The caller presents.
pub fn draw_frame<T: sdl2::render::RenderTarget>(
    canvas: &mut sdl2::render::Canvas<T>,
    atlas: &mut sdl2::render::Texture,
    visuals: &mut Visuals,
) -> Result<(), String> {
    use sdl2::render::BlendMode;
    let (pixels, dirty) = visuals.atlas();
    if let Some([x, y, w, h]) = dirty {
        let pitch = (text::ATLAS_SIZE * 4) as usize;
        let start = y as usize * pitch + x as usize * 4;
        atlas
            .update(
                sdl2::rect::Rect::new(x as i32, y as i32, w, h),
                &pixels[start..],
                pitch,
            )
            .map_err(|e| e.to_string())?;
    }
    canvas.set_draw_color(CLEAR.to_color());
    canvas.clear();
    for (layer, batch) in visuals.batches().iter().enumerate() {
        if batch.indices.is_empty() {
            continue;
        }
        // The light layer adds to what is under it.
        atlas.set_blend_mode(if layer == draw::Layer::Glow as usize {
            BlendMode::Add
        } else {
            BlendMode::Blend
        });
        canvas.render_geometry(&batch.vertices, Some(atlas), batch.indices.as_slice())?;
    }
    Ok(())
}

/// The atlas texture for a canvas, filtered for smooth scaling.
pub fn create_atlas<T>(
    creator: &sdl2::render::TextureCreator<T>,
) -> Result<sdl2::render::Texture, String> {
    sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "1");
    let mut texture = creator
        .create_texture_streaming(
            sdl2::pixels::PixelFormatEnum::RGBA32,
            text::ATLAS_SIZE,
            text::ATLAS_SIZE,
        )
        .map_err(|e| e.to_string())?;
    texture.set_blend_mode(sdl2::render::BlendMode::Blend);
    Ok(texture)
}

#[allow(clippy::too_many_arguments)]
fn draw_drive(
    p: &mut Painter,
    text: &mut Text,
    state: &mut RoadState,
    view: &DriveView,
    drift_heading: f32,
    size: (f32, f32),
    dt: f32,
    t: f32,
    with_hud: bool,
) {
    let (w, h) = size;
    let geom = Geom::new(&view.lanes);
    let truck_u = road::truck_u(view, &geom);
    let cam = Camera::new(&view.road, truck_u, view.lanes.yaw, state, size, dt);
    let light = palette::light(&view.weather);
    let land = palette::land(&view.weather);
    let sky = SkyFrame {
        w,
        h,
        horizon: cam.horizon,
        focal: cam.focal,
        land_top: cam.far_y(),
        heading: view.weather.heading.unwrap_or(drift_heading),
        weather: &view.weather,
        light: &light,
        land: &land,
        t,
        speed_mph: view.hud.speed_mph as f32,
    };
    p.set_layer(draw::Layer::World);
    sky::draw_sky(p, &sky);
    let frame = RoadFrame {
        view,
        light: &light,
        land: &land,
        cam: &cam,
        geom,
        truck_u,
        t,
    };
    road::draw(p, text, &frame);
    sky::draw_weather(p, &sky);
    if with_hud {
        hud::draw(p, text, view, w, h, t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_title_backdrop_and_a_menu_build_one_frame_then_nothing_until_a_change() {
        let mut visuals = Visuals::new();
        let log = MessageLog::new();
        let panel = Panel::from_lines(&[
            "Freight Fate".to_string(),
            String::new(),
            "> New career".to_string(),
            "  Quit".to_string(),
        ])
        .unwrap();
        let scene = Scene {
            screen: Screen::Panel(panel.clone()),
            achievement: None,
        };
        assert!(visuals.build(scene.clone(), &log, (900, 640), false));
        let drawn = visuals.painter.vertex_count();
        assert!(
            drawn > 1000,
            "a backdrop, a panel and its text: {drawn} vertices"
        );
        assert!(
            !visuals.build(scene.clone(), &log, (900, 640), false),
            "the same menu again is not redrawn"
        );
        assert!(
            visuals.build(scene, &log, (1280, 800), false),
            "a resize is"
        );
        let mut moved = panel;
        moved.rows = Some((vec!["New career".into(), "Quit".into()], 1));
        assert!(visuals.build(
            Scene {
                screen: Screen::Panel(moved),
                achievement: None
            },
            &log,
            (1280, 800),
            false
        ));
    }

    #[test]
    fn a_moving_drive_is_drawn_every_frame_with_valid_geometry() {
        let mut visuals = Visuals::new();
        let log = MessageLog::new();
        let mut view = screens::showcase();
        view.hud.speed_mph = 55.0;
        let scene = Scene {
            screen: Screen::Drive {
                view: Box::new(view),
                overlay: None,
            },
            achievement: None,
        };
        assert!(visuals.build(scene.clone(), &log, (900, 640), false));
        assert!(
            visuals.build(scene, &log, (900, 640), false),
            "the road is moving"
        );
        for batch in visuals.batches() {
            assert_eq!(batch.indices.len() % 3, 0);
            assert!(batch
                .indices
                .iter()
                .all(|&i| (i as usize) < batch.vertices.len()));
            for v in &batch.vertices {
                assert!(v.position.x.is_finite() && v.position.y.is_finite());
            }
        }
    }

    #[test]
    fn every_kind_of_weather_and_hour_draws() {
        use ff_core::sim::weather::WeatherKind;
        let mut visuals = Visuals::new();
        let log = MessageLog::new();
        for kind in WeatherKind::ALL {
            for hour in [2.0, 6.0, 12.0, 19.5] {
                let mut view = screens::showcase();
                view.weather.kind = kind;
                view.weather.hour = hour;
                view.weather.wet = matches!(kind, WeatherKind::Rain | WeatherKind::HeavyRain);
                view.weather.snow_cover = kind == WeatherKind::Snow;
                view.hud.speed_mph = 60.0;
                let scene = Scene {
                    screen: Screen::Drive {
                        view: Box::new(view),
                        overlay: None,
                    },
                    achievement: Some("Achievement unlocked: First Haul".into()),
                };
                assert!(visuals.build(scene, &log, (1800, 1280), true));
                for batch in visuals.batches() {
                    assert!(batch
                        .indices
                        .iter()
                        .all(|&i| (i as usize) < batch.vertices.len()));
                }
            }
        }
    }
}
