//! Where a phone app is (active, inactive, in the background), and what the
//! game does as it moves.
//!
//! iOS and Android tell an app about leaving in several notifications, in
//! orders that depend on how the player left: a call, Siri or the lock
//! button make the app inactive and then put it in the background; Control
//! Center and the notification shade make it inactive and active again
//! without ever reaching the background; an app ended from the switcher
//! gets nothing more after the background. Each arrives here as one
//! [`AppEvent`], and [`AppLifecycle::next`] answers with the new state and the
//! [`AppAction`]s the game takes, in order. Nothing else moves the place.
//!
//! The rules the actions keep:
//!
//! - Leaving the foreground at the wheel pauses the drive, so the truck never
//!   rolls unheard, and lets go of every control held.
//! - Reaching the background saves the career and the drive in progress, so
//!   the system ending the app loses nothing. Coming back puts the save back
//!   at the last stop, because the game's rule stays that a drive is saved
//!   only at a stop; the background save is there for the system, not the
//!   player.
//! - Coming back to the foreground reads the screen again, since whatever was
//!   being said when the player left was cut off.
//! - Sound that the system took (a call, an alarm, Siri) or the output that
//!   went away (headphones out) pauses the drive as leaving does.
//! - Once the system says the app is ending, nothing after it is a return.
//!   Android ends an app swiped out of its recent apps screen by waking it
//!   to quit: SDL queues the quit and "terminating", then the four
//!   foreground events of the wake. Taken as a return, they read the screen
//!   to a player who just closed the game, and track the run again after
//!   the quit ended it, so the next start offers a problem report for a
//!   crash that never happened.
//!
//! A desktop window losing focus is not one of these events: the game keeps
//! running there.

/// Where the app is, as the platform reports it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AppPlace {
    /// In the foreground and taking touches.
    #[default]
    Active,
    /// Still on screen, but something is over it (a call, Control Center,
    /// the app switcher) or it is on its way to or from the background.
    Inactive,
    /// Off screen. The system may end the app here without a word.
    Background,
    /// The system said the app is ending. Nothing moves it again.
    Ending,
}

/// A platform notification about the app or its sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppEvent {
    /// iOS `willResignActive`, SDL `APP_WILLENTERBACKGROUND`.
    WillResignActive,
    /// iOS `didEnterBackground`, SDL `APP_DIDENTERBACKGROUND`.
    DidEnterBackground,
    /// iOS `willEnterForeground`, SDL `APP_WILLENTERFOREGROUND`.
    WillEnterForeground,
    /// iOS `didBecomeActive`, SDL `APP_DIDENTERFOREGROUND`.
    DidBecomeActive,
    /// The system took the sound: a call, an alarm, Siri.
    AudioInterruptionBegan,
    /// The system gave the sound back.
    AudioInterruptionEnded,
    /// The output the game was playing through went away: headphones out, a
    /// Bluetooth device off or out of range.
    OutputDeviceLost,
    /// SDL `APP_TERMINATING`: the system is ending the app (Android's
    /// activity destroyed, iOS `willTerminate`).
    WillTerminate,
}

/// What the game does on a transition, in the order given.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppAction {
    /// Let go of every key, button and touch held.
    ReleaseControls,
    /// At the wheel, open the pause menu (as Escape does). Anywhere else,
    /// nothing: a menu is already still.
    PauseDrive,
    /// Write the career, with the drive in progress where it can be saved.
    SaveForBackground,
    /// Put the save back at the drive's last stop once the app is back.
    RestoreStopSave,
    /// The run counts as ended cleanly while it is away (problem reports).
    EndReportSession,
    /// The run is tracked again (problem reports).
    ResumeReportSession,
    /// Ask the system for the sound again.
    WakeAudio,
    /// Say the screen again: the pause menu from its title, a menu its
    /// focused row.
    ReadScreenAgain,
}

use AppAction::*;

/// Where the app is, and whether the system holds its sound.
///
/// `sound_taken` is set by an interruption beginning, in any place, and
/// cleared when the app becomes active again or the interruption ends. It
/// decides who reads the screen after a call: iOS sends the interruption's
/// end before or after the app becomes active, or not at all (Apple does not
/// promise an end for every begin), so becoming active always reads, and an
/// end reads only when the app is active and the sound was still taken (an
/// alarm, where the app never left). Each return is read exactly once.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AppLifecycle {
    pub place: AppPlace,
    pub sound_taken: bool,
}

impl AppLifecycle {
    /// The state after `event`, and what to do about it.
    pub fn next(self, event: AppEvent) -> (Self, &'static [AppAction]) {
        use AppEvent::*;
        if self.place == AppPlace::Ending {
            return (self, &[]);
        }
        let active = self.place == AppPlace::Active;
        match event {
            AudioInterruptionBegan => {
                let next = Self {
                    sound_taken: true,
                    ..self
                };
                let actions: &'static [AppAction] = if active {
                    &[ReleaseControls, PauseDrive]
                } else {
                    &[]
                };
                (next, actions)
            }
            AudioInterruptionEnded => {
                let next = Self {
                    sound_taken: false,
                    ..self
                };
                let actions: &'static [AppAction] = if active && self.sound_taken {
                    &[WakeAudio, ReadScreenAgain]
                } else {
                    &[WakeAudio]
                };
                (next, actions)
            }
            _ => {
                let (place, actions) = self.place.next(event);
                let sound_taken = if event == DidBecomeActive && !active {
                    false
                } else {
                    self.sound_taken
                };
                (Self { place, sound_taken }, actions)
            }
        }
    }

    /// `status`: the place, and `sound_taken` while the system holds it.
    pub fn key(self) -> String {
        if self.sound_taken {
            format!("{}, sound_taken", self.place.key())
        } else {
            self.place.key().to_string()
        }
    }
}

impl AppPlace {
    /// The place after one of the four app events, and what to do about
    /// it. The sound's events belong to [`AppLifecycle::next`]; here they
    /// change nothing.
    pub fn next(self, event: AppEvent) -> (Self, &'static [AppAction]) {
        use AppEvent::*;
        use AppPlace::*;
        match (self, event) {
            // Leaving: a call, Siri, Control Center, the switcher, the lock.
            (Active, WillResignActive) => (Inactive, &[ReleaseControls, PauseDrive]),
            // ... and on into the background.
            (Inactive, DidEnterBackground) => (Background, &[SaveForBackground, EndReportSession]),
            // A background with no resign before it (a platform that skips
            // one): everything leaving does, at once.
            (Active, DidEnterBackground) => (
                Background,
                &[
                    ReleaseControls,
                    PauseDrive,
                    SaveForBackground,
                    EndReportSession,
                ],
            ),
            // Coming back from the background, which may still stop at
            // inactive (the switcher held open) before it is active.
            (Background, WillEnterForeground) => {
                (Inactive, &[ResumeReportSession, RestoreStopSave])
            }
            (Background, DidBecomeActive) => (
                Active,
                &[
                    ResumeReportSession,
                    RestoreStopSave,
                    WakeAudio,
                    ReadScreenAgain,
                ],
            ),
            // Back from inactive, whether or not it reached the background.
            (Inactive, DidBecomeActive) => (Active, &[WakeAudio, ReadScreenAgain]),
            // The output gone while playing: the drive pauses as it does
            // when the player leaves.
            (Active, OutputDeviceLost) => (Active, &[ReleaseControls, PauseDrive]),
            // Ending, from anywhere: the run ended cleanly, and stays ended.
            (_, WillTerminate) => (Ending, &[EndReportSession]),
            // Everything else repeats what the place already says, or is
            // news away from the foreground, where leaving already paused
            // the drive.
            _ => (self, &[]),
        }
    }

    /// The name `status` and `scenario` use.
    pub fn key(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Background => "background",
            Self::Ending => "ending",
        }
    }
}

impl AppEvent {
    /// Every event, in the order [`AppEvent::key`] lists them.
    pub const ALL: [AppEvent; 8] = [
        AppEvent::WillResignActive,
        AppEvent::DidEnterBackground,
        AppEvent::WillEnterForeground,
        AppEvent::DidBecomeActive,
        AppEvent::AudioInterruptionBegan,
        AppEvent::AudioInterruptionEnded,
        AppEvent::OutputDeviceLost,
        AppEvent::WillTerminate,
    ];

    /// The name the agent server's `app_event` tool takes.
    pub fn key(self) -> &'static str {
        match self {
            Self::WillResignActive => "will_resign_active",
            Self::DidEnterBackground => "did_enter_background",
            Self::WillEnterForeground => "will_enter_foreground",
            Self::DidBecomeActive => "did_become_active",
            Self::AudioInterruptionBegan => "audio_interruption_began",
            Self::AudioInterruptionEnded => "audio_interruption_ended",
            Self::OutputDeviceLost => "output_device_lost",
            Self::WillTerminate => "will_terminate",
        }
    }

    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|event| event.key() == key)
    }

    /// The code the iOS bridge queues (`ios/ff_lifecycle.m`).
    pub fn from_code(code: i32) -> Option<Self> {
        usize::try_from(code)
            .ok()
            .and_then(|index| Self::ALL.get(index).copied())
    }
}

#[cfg(test)]
mod tests {
    use super::AppEvent::*;
    use super::AppPlace::*;
    use super::*;

    const NONE: &[AppAction] = &[];
    const LEAVE: &[AppAction] = &[ReleaseControls, PauseDrive];
    const BACK: &[AppAction] = &[WakeAudio, ReadScreenAgain];
    const WAKE: &[AppAction] = &[WakeAudio];
    const SAVE: &[AppAction] = &[SaveForBackground, EndReportSession];
    const LEAVE_AND_SAVE: &[AppAction] = &[
        ReleaseControls,
        PauseDrive,
        SaveForBackground,
        EndReportSession,
    ];
    const FOREGROUND: &[AppAction] = &[ResumeReportSession, RestoreStopSave];
    const END: &[AppAction] = &[EndReportSession];
    const BACK_FROM_BACKGROUND: &[AppAction] = &[
        ResumeReportSession,
        RestoreStopSave,
        WakeAudio,
        ReadScreenAgain,
    ];

    fn at(place: AppPlace, sound_taken: bool) -> AppLifecycle {
        AppLifecycle { place, sound_taken }
    }

    /// One row: where it was, the sound held, the event, where it goes,
    /// the sound held after, and the actions.
    type Row = (
        AppPlace,
        bool,
        AppEvent,
        AppPlace,
        bool,
        &'static [AppAction],
    );

    /// Every place, with the sound held and not, against every event.
    #[test]
    fn transition_table() {
        #[rustfmt::skip]
        let table: [Row; 64] = [
            (Active, false, WillResignActive, Inactive, false, LEAVE),
            (Active, false, DidEnterBackground, Background, false, LEAVE_AND_SAVE),
            (Active, false, WillEnterForeground, Active, false, NONE),
            (Active, false, DidBecomeActive, Active, false, NONE),
            (Active, false, AudioInterruptionBegan, Active, true, LEAVE),
            (Active, false, AudioInterruptionEnded, Active, false, WAKE),
            (Active, false, OutputDeviceLost, Active, false, LEAVE),
            (Active, false, WillTerminate, Ending, false, END),
            (Active, true, WillResignActive, Inactive, true, LEAVE),
            (Active, true, DidEnterBackground, Background, true, LEAVE_AND_SAVE),
            (Active, true, WillEnterForeground, Active, true, NONE),
            (Active, true, DidBecomeActive, Active, true, NONE),
            (Active, true, AudioInterruptionBegan, Active, true, LEAVE),
            (Active, true, AudioInterruptionEnded, Active, false, BACK),
            (Active, true, OutputDeviceLost, Active, true, LEAVE),
            (Active, true, WillTerminate, Ending, true, END),
            (Inactive, false, WillResignActive, Inactive, false, NONE),
            (Inactive, false, DidEnterBackground, Background, false, SAVE),
            (Inactive, false, WillEnterForeground, Inactive, false, NONE),
            (Inactive, false, DidBecomeActive, Active, false, BACK),
            (Inactive, false, AudioInterruptionBegan, Inactive, true, NONE),
            (Inactive, false, AudioInterruptionEnded, Inactive, false, WAKE),
            (Inactive, false, OutputDeviceLost, Inactive, false, NONE),
            (Inactive, false, WillTerminate, Ending, false, END),
            (Inactive, true, WillResignActive, Inactive, true, NONE),
            (Inactive, true, DidEnterBackground, Background, true, SAVE),
            (Inactive, true, WillEnterForeground, Inactive, true, NONE),
            (Inactive, true, DidBecomeActive, Active, false, BACK),
            (Inactive, true, AudioInterruptionBegan, Inactive, true, NONE),
            (Inactive, true, AudioInterruptionEnded, Inactive, false, WAKE),
            (Inactive, true, OutputDeviceLost, Inactive, true, NONE),
            (Inactive, true, WillTerminate, Ending, true, END),
            (Background, false, WillResignActive, Background, false, NONE),
            (Background, false, DidEnterBackground, Background, false, NONE),
            (Background, false, WillEnterForeground, Inactive, false, FOREGROUND),
            (Background, false, DidBecomeActive, Active, false, BACK_FROM_BACKGROUND),
            (Background, false, AudioInterruptionBegan, Background, true, NONE),
            (Background, false, AudioInterruptionEnded, Background, false, WAKE),
            (Background, false, OutputDeviceLost, Background, false, NONE),
            (Background, false, WillTerminate, Ending, false, END),
            (Background, true, WillResignActive, Background, true, NONE),
            (Background, true, DidEnterBackground, Background, true, NONE),
            (Background, true, WillEnterForeground, Inactive, true, FOREGROUND),
            (Background, true, DidBecomeActive, Active, false, BACK_FROM_BACKGROUND),
            (Background, true, AudioInterruptionBegan, Background, true, NONE),
            (Background, true, AudioInterruptionEnded, Background, false, WAKE),
            (Background, true, OutputDeviceLost, Background, true, NONE),
            (Background, true, WillTerminate, Ending, true, END),
            (Ending, false, WillResignActive, Ending, false, NONE),
            (Ending, false, DidEnterBackground, Ending, false, NONE),
            (Ending, false, WillEnterForeground, Ending, false, NONE),
            (Ending, false, DidBecomeActive, Ending, false, NONE),
            (Ending, false, AudioInterruptionBegan, Ending, false, NONE),
            (Ending, false, AudioInterruptionEnded, Ending, false, NONE),
            (Ending, false, OutputDeviceLost, Ending, false, NONE),
            (Ending, false, WillTerminate, Ending, false, NONE),
            (Ending, true, WillResignActive, Ending, true, NONE),
            (Ending, true, DidEnterBackground, Ending, true, NONE),
            (Ending, true, WillEnterForeground, Ending, true, NONE),
            (Ending, true, DidBecomeActive, Ending, true, NONE),
            (Ending, true, AudioInterruptionBegan, Ending, true, NONE),
            (Ending, true, AudioInterruptionEnded, Ending, true, NONE),
            (Ending, true, OutputDeviceLost, Ending, true, NONE),
            (Ending, true, WillTerminate, Ending, true, NONE),
        ];
        for (place, taken, event, to, to_taken, actions) in table {
            assert_eq!(
                at(place, taken).next(event),
                (at(to, to_taken), actions),
                "{place:?} (sound taken {taken}) + {event:?}"
            );
        }
        for place in [Active, Inactive, Background, Ending] {
            for taken in [false, true] {
                for event in AppEvent::ALL {
                    assert!(
                        table
                            .iter()
                            .any(|row| row.0 == place && row.1 == taken && row.2 == event),
                        "{place:?} (sound taken {taken}) + {event:?} is missing"
                    );
                }
            }
        }
    }

    /// Runs a sequence of events from active, gathering every action.
    fn run(events: &[AppEvent]) -> (AppLifecycle, Vec<AppAction>) {
        let mut state = AppLifecycle::default();
        let mut done = Vec::new();
        for &event in events {
            let (next, actions) = state.next(event);
            state = next;
            done.extend_from_slice(actions);
        }
        (state, done)
    }

    fn count(actions: &[AppAction], action: AppAction) -> usize {
        actions.iter().filter(|a| **a == action).count()
    }

    #[test]
    fn a_call_or_the_lock_button_pauses_saves_and_comes_back_read() {
        let (state, actions) = run(&[
            WillResignActive,
            DidEnterBackground,
            WillEnterForeground,
            DidBecomeActive,
        ]);
        assert_eq!(state, AppLifecycle::default());
        assert_eq!(
            actions,
            [
                ReleaseControls,
                PauseDrive,
                SaveForBackground,
                EndReportSession,
                ResumeReportSession,
                RestoreStopSave,
                WakeAudio,
                ReadScreenAgain,
            ]
        );
    }

    #[test]
    fn control_center_pauses_without_a_save() {
        let (state, actions) = run(&[WillResignActive, DidBecomeActive]);
        assert_eq!(state, AppLifecycle::default());
        assert_eq!(
            actions,
            [ReleaseControls, PauseDrive, WakeAudio, ReadScreenAgain]
        );
    }

    /// A call, in every order iOS may send its pieces, and with the end
    /// missing: one pause and exactly one reading on the way back.
    #[test]
    fn a_call_is_read_once_on_return_in_every_order() {
        let orders: [&[AppEvent]; 7] = [
            &[
                AudioInterruptionBegan,
                WillResignActive,
                DidBecomeActive,
                AudioInterruptionEnded,
            ],
            &[
                AudioInterruptionBegan,
                WillResignActive,
                AudioInterruptionEnded,
                DidBecomeActive,
            ],
            &[
                WillResignActive,
                AudioInterruptionBegan,
                DidBecomeActive,
                AudioInterruptionEnded,
            ],
            &[
                WillResignActive,
                AudioInterruptionBegan,
                AudioInterruptionEnded,
                DidBecomeActive,
            ],
            // No end at all.
            &[WillResignActive, AudioInterruptionBegan, DidBecomeActive],
            // Through the background.
            &[
                WillResignActive,
                AudioInterruptionBegan,
                DidEnterBackground,
                WillEnterForeground,
                DidBecomeActive,
                AudioInterruptionEnded,
            ],
            // An alarm: the app never leaves.
            &[AudioInterruptionBegan, AudioInterruptionEnded],
        ];
        for events in orders {
            let (state, actions) = run(events);
            assert_eq!(
                count(&actions, ReadScreenAgain),
                1,
                "{events:?}: {actions:?}"
            );
            assert!(count(&actions, PauseDrive) >= 1, "{events:?}");
            assert_eq!(state, AppLifecycle::default(), "{events:?}");
        }
    }

    /// Swiped out of Android's recent apps: SDL queues the quit's
    /// "terminating" ahead of the foreground events of the wake it ends
    /// the app with. The run stays ended and nothing is read.
    #[test]
    fn an_app_ended_from_the_recent_apps_screen_never_comes_back() {
        let (state, actions) = run(&[
            WillResignActive,
            DidEnterBackground,
            WillTerminate,
            WillEnterForeground,
            DidBecomeActive,
        ]);
        assert_eq!(state.place, Ending);
        assert_eq!(count(&actions, ResumeReportSession), 0);
        assert_eq!(count(&actions, ReadScreenAgain), 0);
        assert_eq!(actions.last(), Some(&EndReportSession));
    }

    #[test]
    fn a_background_with_no_resign_still_pauses_first() {
        let (state, actions) = run(&[DidEnterBackground]);
        assert_eq!(state.place, Background);
        assert_eq!(actions[..2], [ReleaseControls, PauseDrive]);
    }

    #[test]
    fn status_names_the_sound_taken() {
        assert_eq!(AppLifecycle::default().key(), "active");
        assert_eq!(at(Inactive, true).key(), "inactive, sound_taken");
    }

    #[test]
    fn event_names_and_codes_round_trip() {
        for (code, event) in AppEvent::ALL.into_iter().enumerate() {
            assert_eq!(AppEvent::from_key(event.key()), Some(event));
            assert_eq!(AppEvent::from_code(code as i32), Some(event));
        }
        assert_eq!(AppEvent::from_code(-1), None);
        assert_eq!(AppEvent::from_code(8), None);
        assert_eq!(AppEvent::from_key("resume"), None);
    }
}
