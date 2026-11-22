//! Where a phone app is (active, inactive, in the background), and what the
//! game does as it moves.
//!
//! iOS and Android tell an app about leaving in several notifications, in
//! orders that depend on how the player left: a call, Siri or the lock
//! button make the app inactive and then put it in the background; Control
//! Center and the notification shade make it inactive and active again
//! without ever reaching the background; an app ended from the switcher
//! gets nothing more after the background. Each arrives here as one
//! [`AppEvent`], and [`AppPlace::next`] answers with the new place and the
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

impl AppPlace {
    /// The place after `event`, and what to do about it.
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
            // The sound taken or its output gone while playing: the drive
            // pauses as it does when the player leaves.
            (Active, AudioInterruptionBegan | OutputDeviceLost) => {
                (Active, &[ReleaseControls, PauseDrive])
            }
            (Active, AudioInterruptionEnded) => (Active, &[WakeAudio, ReadScreenAgain]),
            // Not in the foreground, the sound coming back is woken; the
            // screen is read when the app is active again.
            (Inactive | Background, AudioInterruptionEnded) => (self, &[WakeAudio]),
            // Everything else repeats what the place already says, or is
            // sound news away from the foreground, where leaving already
            // paused the drive.
            _ => (self, &[]),
        }
    }

    /// The name `status` and `scenario` use.
    pub fn key(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Inactive => "inactive",
            Self::Background => "background",
        }
    }
}

impl AppEvent {
    /// Every event, in the order [`AppEvent::key`] lists them.
    pub const ALL: [AppEvent; 7] = [
        AppEvent::WillResignActive,
        AppEvent::DidEnterBackground,
        AppEvent::WillEnterForeground,
        AppEvent::DidBecomeActive,
        AppEvent::AudioInterruptionBegan,
        AppEvent::AudioInterruptionEnded,
        AppEvent::OutputDeviceLost,
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

    /// Every place against every event.
    #[test]
    fn transition_table() {
        let leave: &[AppAction] = &[ReleaseControls, PauseDrive];
        let none: &[AppAction] = &[];
        let table: [(AppPlace, AppEvent, AppPlace, &[AppAction]); 21] = [
            (Active, WillResignActive, Inactive, leave),
            (
                Active,
                DidEnterBackground,
                Background,
                &[
                    ReleaseControls,
                    PauseDrive,
                    SaveForBackground,
                    EndReportSession,
                ],
            ),
            (Active, WillEnterForeground, Active, none),
            (Active, DidBecomeActive, Active, none),
            (Active, AudioInterruptionBegan, Active, leave),
            (
                Active,
                AudioInterruptionEnded,
                Active,
                &[WakeAudio, ReadScreenAgain],
            ),
            (Active, OutputDeviceLost, Active, leave),
            (Inactive, WillResignActive, Inactive, none),
            (
                Inactive,
                DidEnterBackground,
                Background,
                &[SaveForBackground, EndReportSession],
            ),
            (Inactive, WillEnterForeground, Inactive, none),
            (
                Inactive,
                DidBecomeActive,
                Active,
                &[WakeAudio, ReadScreenAgain],
            ),
            (Inactive, AudioInterruptionBegan, Inactive, none),
            (Inactive, AudioInterruptionEnded, Inactive, &[WakeAudio]),
            (Inactive, OutputDeviceLost, Inactive, none),
            (Background, WillResignActive, Background, none),
            (Background, DidEnterBackground, Background, none),
            (
                Background,
                WillEnterForeground,
                Inactive,
                &[ResumeReportSession, RestoreStopSave],
            ),
            (
                Background,
                DidBecomeActive,
                Active,
                &[
                    ResumeReportSession,
                    RestoreStopSave,
                    WakeAudio,
                    ReadScreenAgain,
                ],
            ),
            (Background, AudioInterruptionBegan, Background, none),
            (Background, AudioInterruptionEnded, Background, &[WakeAudio]),
            (Background, OutputDeviceLost, Background, none),
        ];
        for (from, event, to, actions) in table {
            assert_eq!(from.next(event), (to, actions), "{from:?} + {event:?}");
        }
        // The table covers every pair.
        for from in [Active, Inactive, Background] {
            for event in AppEvent::ALL {
                assert!(
                    table.iter().any(|(f, e, _, _)| *f == from && *e == event),
                    "{from:?} + {event:?} is missing"
                );
            }
        }
    }

    /// Runs a sequence of events from active, gathering every action.
    fn run(events: &[AppEvent]) -> (AppPlace, Vec<AppAction>) {
        let mut place = Active;
        let mut done = Vec::new();
        for &event in events {
            let (next, actions) = place.next(event);
            place = next;
            done.extend_from_slice(actions);
        }
        (place, done)
    }

    #[test]
    fn a_call_or_the_lock_button_pauses_saves_and_comes_back_read() {
        let (place, actions) = run(&[
            WillResignActive,
            DidEnterBackground,
            WillEnterForeground,
            DidBecomeActive,
        ]);
        assert_eq!(place, Active);
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
        let (place, actions) = run(&[WillResignActive, DidBecomeActive]);
        assert_eq!(place, Active);
        assert_eq!(
            actions,
            [ReleaseControls, PauseDrive, WakeAudio, ReadScreenAgain]
        );
    }

    #[test]
    fn a_call_taking_the_sound_pauses_once() {
        // iOS sends the interruption and the resign in either order.
        for events in [
            [AudioInterruptionBegan, WillResignActive],
            [WillResignActive, AudioInterruptionBegan],
        ] {
            let (_, actions) = run(&events);
            let pauses = actions.iter().filter(|a| **a == PauseDrive).count();
            // The second pause lands on the pause menu, which ignores it;
            // the table only promises that the drive is paused.
            assert!(pauses >= 1, "{events:?}");
        }
        let (place, actions) = run(&[
            WillResignActive,
            AudioInterruptionBegan,
            AudioInterruptionEnded,
            DidBecomeActive,
        ]);
        assert_eq!(place, Active);
        assert_eq!(
            actions,
            [
                ReleaseControls,
                PauseDrive,
                WakeAudio,
                WakeAudio,
                ReadScreenAgain
            ]
        );
    }

    #[test]
    fn a_background_with_no_resign_still_pauses_first() {
        let (place, actions) = run(&[DidEnterBackground]);
        assert_eq!(place, Background);
        assert_eq!(actions[..2], [ReleaseControls, PauseDrive]);
    }

    #[test]
    fn event_names_and_codes_round_trip() {
        for (code, event) in AppEvent::ALL.into_iter().enumerate() {
            assert_eq!(AppEvent::from_key(event.key()), Some(event));
            assert_eq!(AppEvent::from_code(code as i32), Some(event));
        }
        assert_eq!(AppEvent::from_code(-1), None);
        assert_eq!(AppEvent::from_code(7), None);
        assert_eq!(AppEvent::from_key("resume"), None);
    }
}
