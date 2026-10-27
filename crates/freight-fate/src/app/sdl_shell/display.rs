//! The window as a display: whether it can be seen, and drawing the
//! visual display (or the plain fill) into it.
//!
//! Drawing is paid for only when it shows: a frame identical to the last
//! one is not presented, and nothing is drawn while the window is minimized
//! or (on a Mac) entirely covered. The window used to be cleared and
//! presented sixty times a second whether anything changed or not.

#[cfg(all(target_os = "macos", feature = "visuals"))]
use std::time::Instant;

#[cfg(feature = "visuals")]
use std::sync::Arc;

#[cfg(feature = "visuals")]
use ff_core::l10n::Translator;
#[cfg(feature = "visuals")]
use ff_core::message_log::MessageLog;
use sdl2::event::WindowEvent;
use sdl2::pixels::Color;

#[cfg(feature = "visuals")]
use crate::visuals::{Scene, Visuals};

use super::super::BG_COLOR;
#[cfg(feature = "visuals")]
use super::super::WINDOW_SIZE;
use super::SdlShell;

/// How often a Mac asks AppKit whether the window is covered.
#[cfg(all(target_os = "macos", feature = "visuals"))]
const OCCLUSION_POLL: std::time::Duration = std::time::Duration::from_millis(500);

impl SdlShell {
    /// What the window's own events say about whether it can be seen.
    pub(super) fn note_window_event(&mut self, event: &WindowEvent) {
        match event {
            WindowEvent::Minimized | WindowEvent::Hidden => self.hidden = true,
            WindowEvent::Shown
            | WindowEvent::Restored
            | WindowEvent::Maximized
            | WindowEvent::Exposed
            | WindowEvent::SizeChanged(..)
            | WindowEvent::Resized(..)
            | WindowEvent::DisplayChanged(..) => {
                self.hidden = false;
                self.redraw = true;
            }
            _ => {}
        }
    }

    /// Whether drawing the visual display this frame could be seen at all.
    /// On a Mac this also asks AppKit, at most twice a second, whether the
    /// window is covered. Only the visual display asks: it is the one thing
    /// drawn frame after frame, and the plain fill is not.
    #[cfg(feature = "visuals")]
    pub fn can_be_seen(&mut self) -> bool {
        if self.hidden {
            return false;
        }
        #[cfg(target_os = "macos")]
        if self.cocoa {
            self.check_occlusion();
        }
        !self.covered()
    }

    #[cfg(all(target_os = "macos", feature = "visuals"))]
    fn covered(&self) -> bool {
        self.occluded
    }

    #[cfg(all(not(target_os = "macos"), feature = "visuals"))]
    fn covered(&self) -> bool {
        false
    }

    /// Ask AppKit, at most twice a second, whether the window is covered.
    #[cfg(all(target_os = "macos", feature = "visuals"))]
    fn check_occlusion(&mut self) {
        let now = Instant::now();
        if self
            .occlusion_checked
            .is_some_and(|at| now.duration_since(at) < OCCLUSION_POLL)
        {
            return;
        }
        self.occlusion_checked = Some(now);
        let covered = mac_window::covered(self.canvas.window());
        if covered != self.occluded {
            log::info!(
                "window {}",
                if covered {
                    "covered: drawing paused"
                } else {
                    "uncovered: drawing resumed"
                }
            );
            // Uncovered: whatever is on screen now is stale.
            self.redraw |= !covered;
        }
        self.occluded = covered;
    }

    /// The visual display off (or left out of the build): the plain dark
    /// fill, presented only when the window needs repainting rather than
    /// every frame.
    pub fn render_plain(&mut self) {
        #[cfg(feature = "visuals")]
        if self.showing_visuals {
            self.showing_visuals = false;
            self.redraw = true;
            // The atlas and frame memory are let go; turning the display
            // back on builds them again.
            self.visuals = None;
        }
        // Presented only when the window needs repainting, so whether it is
        // covered is not worth asking AppKit.
        if !self.redraw || self.hidden {
            return;
        }
        self.redraw = false;
        self.canvas
            .set_draw_color(Color::RGB(BG_COLOR.0, BG_COLOR.1, BG_COLOR.2));
        self.canvas.clear();
        self.canvas.present();
    }

    /// Draw the visual display for `scene`, if it changed or must be redrawn.
    #[cfg(feature = "visuals")]
    pub fn render_scene(&mut self, scene: Scene, log: &MessageLog, translator: &Arc<Translator>) {
        if !self.showing_visuals {
            self.showing_visuals = true;
            self.redraw = true;
        }
        let size = self.canvas.output_size().unwrap_or(WINDOW_SIZE);
        let force = std::mem::take(&mut self.redraw);
        let visuals = self.visuals.get_or_insert_with(Visuals::new);
        visuals.set_translator(translator);
        if !visuals.build(scene, log, size, force) {
            return;
        }
        if self.atlas.is_none() {
            match crate::visuals::create_atlas(&self.canvas.texture_creator()) {
                Ok(texture) => self.atlas = Some(texture),
                Err(e) => {
                    log::warn!("visuals: no atlas texture ({e}); the window stays plain");
                    self.visuals = None;
                    return;
                }
            }
        }
        let (Some(visuals), Some(atlas)) = (self.visuals.as_mut(), self.atlas.as_mut()) else {
            return;
        };
        if let Err(e) = crate::visuals::draw_frame(&mut self.canvas, atlas, visuals) {
            if !self.geometry_failed {
                self.geometry_failed = true;
                log::warn!("visuals: drawing failed: {e}");
            }
        }
        self.canvas.present();
    }
}

/// Asking AppKit whether the game window is covered. SDL 2.26 never
/// reports a window hidden behind others, and drawing one costs the same
/// as drawing a visible one.
#[cfg(all(target_os = "macos", feature = "visuals"))]
mod mac_window {
    use objc2_app_kit::{NSWindow, NSWindowOcclusionState};
    use sdl2::sys::{SDL_GetVersion, SDL_GetWindowWMInfo, SDL_SysWMinfo, SDL_bool, SDL_SYSWM_TYPE};

    /// The window's `NSWindow`, from SDL's own window-manager info. Not the
    /// sdl2 crate's raw window handle: on a Mac that panics unless the
    /// window was built with a Metal view.
    fn ns_window(window: &sdl2::video::Window) -> Option<&NSWindow> {
        // SAFETY: `SDL_SysWMinfo` is plain C data for which all-zero is the
        // empty value; SDL fills it for this live window, and the version
        // is the linked library's, as SDL requires. On Cocoa the info union
        // holds `struct { NSWindow *window; }`, so its first bytes are the
        // window pointer; the bindings only name the union's raw bytes. SDL
        // holds that NSWindow until the SDL window is destroyed, which
        // cannot happen while `window` is borrowed, and the game loop runs
        // on the main thread that created it, as AppKit requires.
        unsafe {
            let mut info: SDL_SysWMinfo = std::mem::zeroed();
            SDL_GetVersion(&mut info.version);
            if SDL_GetWindowWMInfo(window.raw(), &mut info) != SDL_bool::SDL_TRUE
                || info.subsystem != SDL_SYSWM_TYPE::SDL_SYSWM_COCOA
            {
                return None;
            }
            let ptr = std::ptr::read_unaligned(info.info.dummy.as_ptr().cast::<*const NSWindow>());
            ptr.as_ref()
        }
    }

    /// True when no part of the window can be seen.
    pub fn covered(window: &sdl2::video::Window) -> bool {
        ns_window(window)
            .is_some_and(|w| !w.occlusionState().contains(NSWindowOcclusionState::Visible))
    }
}
