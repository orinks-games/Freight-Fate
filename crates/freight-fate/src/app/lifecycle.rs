//! A phone app leaving and coming back: the actions of
//! [`ff_core::app_lifecycle`], applied to the game.
//!
//! The place lives on [`App`], moved only by [`App::handle_app_event`]. The
//! platform's notifications reach it as [`InputEvent::App`]: SDL's four app
//! events on both phones (translated in `sdl_shell`), and on iOS the sound
//! session's interruptions and lost outputs from `ios/ff_lifecycle.m`. The
//! agent server sends the same events by name, so a drive can leave and come
//! back headless.

use std::time::Instant;

use ff_core::app_lifecycle::{AppAction, AppEvent, AppLifecycle, AppPlace};
use serde_json::Value;

use super::App;
use crate::states::driving::DrivingState;

/// The save put down as the app reached the background, and the one it
/// stood in for, to be put back when the app returns.
#[derive(Debug, Default)]
pub(crate) struct BackgroundSave {
    /// The drive's place as it went to the background.
    written: Option<Value>,
    /// The drive's last stop, which the career saves at.
    stop: Option<Value>,
}

impl App {
    /// Where the app is, for the agent server's `status`.
    pub fn app_place(&self) -> AppPlace {
        self.app_lifecycle.place
    }

    /// The place and whether the system holds the sound, for `status`.
    pub fn app_lifecycle(&self) -> AppLifecycle {
        self.app_lifecycle
    }

    /// One platform notification, through the place's transition.
    pub(crate) fn handle_app_event(&mut self, event: AppEvent) {
        let (next, actions) = self.app_lifecycle.next(event);
        log::info!(
            "app lifecycle: {} while {} -> {}",
            event.key(),
            self.app_lifecycle.key(),
            next.key()
        );
        self.app_lifecycle = next;
        for &action in actions {
            self.apply_app_action(action);
        }
    }

    fn apply_app_action(&mut self, action: AppAction) {
        match action {
            AppAction::ReleaseControls => self.ctx.input.clear(),
            AppAction::PauseDrive => {
                if let Some(state) = self.ctx.state() {
                    state.borrow_mut().on_app_leaving(&mut self.ctx);
                    self.ctx.run_deferred();
                }
            }
            AppAction::SaveForBackground => self.save_for_background(),
            AppAction::RestoreStopSave => self.restore_stop_save(),
            AppAction::EndReportSession => crate::problem_reports::end_session(),
            AppAction::ResumeReportSession => {
                crate::problem_reports::resume_session(&ff_core::settings::data_dir());
            }
            AppAction::WakeAudio => native::wake_audio(),
            AppAction::ReadScreenAgain => {
                if let Some(state) = self.ctx.state() {
                    state.borrow_mut().on_app_return(&mut self.ctx);
                    self.ctx.run_deferred();
                }
            }
        }
    }

    /// Write the career, and the drive in progress as it stands, so the
    /// system ending the app in the background loses nothing.
    ///
    /// The drive is saved in the shape a stop saves it in (the same
    /// snapshot an out-of-service order writes on the shoulder), so Continue
    /// after the app was ended resumes it here, parked. iOS gives an app
    /// about five seconds in the background before it is suspended; the
    /// write is one profile file, logged on both sides with its time.
    fn save_for_background(&mut self) {
        let started = Instant::now();
        log::info!("app lifecycle: background save started");
        if let Some(profile) = self.ctx.profile.as_ref() {
            // Only a drive the save already carries (from its dispatch or a
            // stop), the rule a settled roadside stop follows too.
            let carried = profile.active_trip.is_some();
            let drive = self
                .ctx
                .states()
                .into_iter()
                .rev()
                .filter(|_| carried)
                .find_map(|state| {
                    let state = state.try_borrow().ok()?;
                    let drive = state.as_any().downcast_ref::<DrivingState>()?;
                    Some(parked(drive.snapshot(&self.ctx)))
                });
            if let (Some(snapshot), Some(profile)) = (drive, self.ctx.profile.as_mut()) {
                let stop = profile.active_trip.replace(snapshot.clone());
                // A second background before the return keeps the first
                // stop: the snapshot just written is not a stop.
                let stop = match self.background_save.take() {
                    Some(earlier) => earlier.stop,
                    None => stop,
                };
                self.background_save = Some(BackgroundSave {
                    written: Some(snapshot),
                    stop,
                });
            }
            self.ctx.save_profile();
        }
        log::info!(
            "app lifecycle: background save finished in {} ms",
            started.elapsed().as_millis()
        );
        native::background_saved();
    }

    /// Back from the background: the save points at the drive's last stop
    /// again, as a save does everywhere else in the game, unless the drive
    /// has written a newer stop since.
    fn restore_stop_save(&mut self) {
        let Some(saved) = self.background_save.take() else {
            return;
        };
        let Some(profile) = self.ctx.profile.as_mut() else {
            return;
        };
        if profile.active_trip != saved.written {
            return;
        }
        profile.active_trip = saved.stop;
        self.ctx.save_profile();
        log::info!("app lifecycle: the save is back at the last stop");
    }
}

/// A drive saved mid-road resumes stopped, so it is saved with the parking
/// brake set: the resume line then tells the truth about the brake, and the
/// truck holds where it was until the player drives off.
fn parked(mut snapshot: Value) -> Value {
    if let Some(air) = snapshot.get_mut("air_brake").and_then(Value::as_object_mut) {
        air.insert("parking_brake".to_string(), Value::Bool(true));
    }
    snapshot
}

/// The iOS sound session and background time (`ios/ff_lifecycle.m`).
#[cfg(target_os = "ios")]
pub(crate) mod native {
    extern "C" {
        fn ff_lifecycle_install(terminating: extern "C" fn());
        fn ff_lifecycle_next() -> i32;
        fn ff_lifecycle_background_saved();
        fn ff_lifecycle_wake_audio() -> i32;
    }

    extern "C" fn terminating() {
        // The app is ended from the foreground (rare on iOS); nothing runs
        // after this returns, so the run is marked as ended here.
        crate::problem_reports::end_session();
    }

    /// Set the sound session to play through the silent switch, and watch
    /// for interruptions and lost outputs. Before the audio engine opens.
    pub(crate) fn install() {
        // SAFETY: called once on the main thread before the game loop; UIKit
        // keeps the function pointer for the life of the app.
        unsafe { ff_lifecycle_install(terminating) };
    }

    /// The next queued sound-session event.
    pub(crate) fn next_event() -> Option<ff_core::app_lifecycle::AppEvent> {
        loop {
            // SAFETY: a plain queue pop, no arguments.
            let code = unsafe { ff_lifecycle_next() };
            if code < 0 {
                return None;
            }
            if let Some(event) = ff_core::app_lifecycle::AppEvent::from_code(code) {
                return Some(event);
            }
        }
    }

    /// The background save is written; the extra time asked for it can end.
    pub(super) fn background_saved() {
        // SAFETY: ends the background task the bridge began, if any.
        unsafe { ff_lifecycle_background_saved() };
    }

    /// Ask for the sound session again after it was taken.
    pub(super) fn wake_audio() {
        // SAFETY: no arguments; the bridge logs its own failure.
        if unsafe { ff_lifecycle_wake_audio() } == 0 {
            log::warn!("app lifecycle: the sound session would not come back yet");
        }
    }
}

/// Everywhere else the system has no sound session to ask for, and no
/// background time to end.
#[cfg(not(target_os = "ios"))]
mod native {
    pub(super) fn background_saved() {}
    pub(super) fn wake_audio() {}
}
