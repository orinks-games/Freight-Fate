//! The co-driver's hands on the app (see `states::driving_team`).
//!
//! Each tick, before the top screen updates, the co-driver acts: at the
//! wheel through the drive's pilot, on a screen the road opened by walking
//! its cursor to a row and pressing Enter, the way the player would. A
//! screen the player opened is left to them. While a shift is slept
//! through, it passes as time (`states::driving_team::sleep_ahead`), and
//! what is still driven runs several ticks a frame with the sound down.

use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::playtest::menu::menu_rows;
use crate::states::base::{InputEvent, Key, Mods};
use crate::states::driving::DrivingState;
use crate::states::driving_team::overlay::{overlay_move, OverlayMove};

use super::{App, SharedState, FPS};

/// Real time a frame may spend running the driven parts of a slept-through
/// shift ahead (the merge, the receiver's exit and gate). The player is
/// asleep with the sound down, so the window may run at a few frames a
/// second.
const FAST_FORWARD_BUDGET: Duration = Duration::from_millis(100);
/// Ticks run ahead in one frame, at most.
const FAST_FORWARD_TICKS: usize = 1000;
/// A beat between the co-driver's keys on a screen.
const MENU_BEAT_S: f64 = 0.4;
/// Cursor moves without reaching the row: something is wrong with the
/// screen, and the co-driver hands back.
const MENU_MOVES_MAX: u32 = 40;

/// The co-driver's place on the screens over the drive.
#[derive(Debug, Default)]
pub struct CoDriverHands {
    /// The stack depth under the first screen the player opened during the
    /// shift. Screens above it are the player's.
    pub player_depth: Option<usize>,
    beat_s: f64,
    moves: u32,
}

fn find_drive(states: &[SharedState]) -> Option<SharedState> {
    states
        .iter()
        .rev()
        .find(|state| {
            state
                .try_borrow()
                .is_ok_and(|state| state.as_any().is::<DrivingState>())
        })
        .cloned()
}

fn with_drive<R>(handle: &SharedState, f: impl FnOnce(&mut DrivingState) -> R) -> Option<R> {
    let mut state = handle.try_borrow_mut().ok()?;
    let drive = state.as_any_mut().downcast_mut::<DrivingState>()?;
    Some(f(drive))
}

impl App {
    /// A player's event while the co-driver drives: everything it says is
    /// an answer the player asked for, and a screen it opens is theirs.
    pub(crate) fn handle_player_event_while_away(&mut self, event: &InputEvent) {
        let depth = self.ctx.stack_len();
        let previous = self.ctx.player_asked_begin();
        self.route_event(event);
        self.ctx.player_asked_end(previous);
        if self.ctx.stack_len() > depth && self.co_driver.player_depth.is_none() {
            self.co_driver.player_depth = Some(depth);
        }
    }

    /// The co-driver's move this tick, if they hold the wheel.
    pub(crate) fn co_driver_step(&mut self, dt: f64) {
        if !self.ctx.co_driver_away {
            self.co_driver = CoDriverHands::default();
            return;
        }
        let states = self.ctx.states();
        if let Some(depth) = self.co_driver.player_depth {
            if states.len() > depth {
                return; // the player's own screen is up
            }
            self.co_driver.player_depth = None;
        }
        let (Some(top), Some(drive)) = (states.last().cloned(), find_drive(&states)) else {
            // No drive under the screens: nothing for a co-driver to do.
            // The drive's exit hands everything back; reaching here means
            // it was dropped without one, so at least the player's own
            // assists come back before the next settings save.
            log::error!("the co-driver's drive left the screens without its exit");
            self.ctx.settings.take_assists_back();
            self.ctx.co_driver_away = false;
            self.ctx.sync_cue_mute();
            self.ctx.co_driver_fast_forward = false;
            self.ctx.apply_volumes();
            return;
        };
        if Rc::ptr_eq(&top, &drive) {
            self.co_driver.moves = 0;
            let ctx = &mut self.ctx;
            with_drive(&drive, |d| d.pilot_frame(ctx, dt));
            self.ctx.run_deferred();
            return;
        }
        self.co_driver.beat_s = (self.co_driver.beat_s - dt).max(0.0);
        if self.co_driver.beat_s > 0.0 {
            return;
        }
        let choice = {
            let (Ok(top_ref), Ok(drive_ref)) = (top.try_borrow(), drive.try_borrow()) else {
                return;
            };
            let Some(d) = drive_ref.as_any().downcast_ref::<DrivingState>() else {
                return;
            };
            overlay_move(&*top_ref, d, &self.ctx)
        };
        match choice {
            OverlayMove::Wait => {}
            OverlayMove::Summon(reason) => {
                let ctx = &mut self.ctx;
                with_drive(&drive, |d| d.co_driver_summon(ctx, &reason));
            }
            OverlayMove::Row(label) => self.co_driver_choose_row(&top, &drive, &label),
        }
    }

    /// Walk the cursor to `label` one row a beat, then press Enter on it.
    fn co_driver_choose_row(&mut self, top: &SharedState, drive: &SharedState, label: &str) {
        let rows = top
            .try_borrow()
            .ok()
            .and_then(|state| menu_rows(&*state, &self.ctx));
        let Some((rows, focus)) = rows else {
            return;
        };
        let Some(target) = rows.iter().position(|row| row == label) else {
            return;
        };
        self.co_driver.beat_s = MENU_BEAT_S;
        if self.co_driver.moves >= MENU_MOVES_MAX {
            let ctx = &mut self.ctx;
            with_drive(drive, |d| d.co_driver_summon(ctx, "screen"));
            return;
        }
        self.co_driver.moves += 1;
        let key = if target == focus {
            log::info!(
                target: super::TRANSCRIPT_TARGET,
                "[co-driver] chooses {label}"
            );
            let ctx = &mut self.ctx;
            with_drive(drive, |d| d.note_co_driver_choice(ctx, label));
            self.co_driver.moves = 0;
            Key::Return
        } else if target > focus {
            Key::Down
        } else {
            Key::Up
        };
        self.dispatch_to_state(&InputEvent::key(key));
        self.dispatch_to_state(&InputEvent::KeyUp {
            key,
            mods: Mods::NONE,
        });
    }

    /// Run a slept-through shift ahead: more ticks this frame, inside a
    /// real-time budget, until it ends or the player opens a screen.
    pub(crate) fn co_driver_fast_forward(&mut self) {
        if !self.ctx.co_driver_fast_forward {
            return;
        }
        let dt = 1.0 / FPS as f64;
        self.co_driver_sleep_ahead();
        let started = Instant::now();
        for _ in 0..FAST_FORWARD_TICKS {
            if !self.ctx.co_driver_fast_forward
                || self.co_driver.player_depth.is_some()
                || started.elapsed() >= FAST_FORWARD_BUDGET
            {
                break;
            }
            self.fast_forward_tick(dt);
        }
    }

    /// The open road ahead of a sleeping player, simulated at once (see
    /// `states::driving_team::sleep_ahead`): only when the drive is the top
    /// screen and the player has none open.
    fn co_driver_sleep_ahead(&mut self) {
        if self.co_driver.player_depth.is_some() {
            return;
        }
        let states = self.ctx.states();
        let (Some(top), Some(drive)) = (states.last().cloned(), find_drive(&states)) else {
            return;
        };
        if !Rc::ptr_eq(&top, &drive) {
            return;
        }
        let ctx = &mut self.ctx;
        with_drive(&drive, |d| d.sleep_ahead(ctx));
        self.ctx.run_deferred();
    }

    /// One tick run ahead: the co-driver's move and the screen it is on,
    /// and none of the frame's other work (controller repeat, speech,
    /// audio fades, presence), which keeps to the real frame.
    fn fast_forward_tick(&mut self, dt: f64) {
        self.co_driver_step(dt);
        if let Some(state) = self.ctx.state() {
            state.borrow_mut().update(&mut self.ctx, dt);
            self.ctx.run_deferred();
        }
    }
}
