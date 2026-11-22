//! The values of the Speech rows that name a voice: Main voice, Driving
//! event voice and Speech voice.
//!
//! Pure functions over the settings and the lists the speech sink reports,
//! so the menu code only calls them and the table tests below pin every
//! spoken value and every step.

use ff_core::settings::Settings;

use crate::speech::{SpeechSink, BUILTIN_BACKEND};

/// A voice's spoken name from its registry name.
pub(super) fn spoken_voice_name(registry: &str) -> String {
    match registry {
        "OneCore" => "Windows OneCore".to_string(),
        BUILTIN_BACKEND => "built-in voice".to_string(),
        "UIA" => "Narrator".to_string(),
        other => other.to_string(),
    }
}

/// The value of the Main voice row. `options` is what can speak right now;
/// a saved choice missing from it is named and marked unavailable.
pub(super) fn main_voice_label(saved: &str, options: &[String]) -> String {
    if saved.is_empty() {
        return "automatic".to_string();
    }
    let name = spoken_voice_name(saved);
    if options.iter().any(|option| option == saved) {
        name
    } else {
        format!("{name}, unavailable")
    }
}

/// The Main voice row stepped by `d` from `saved`: `None` for automatic.
///
/// The stops are automatic and then `options`, wrapping. A saved choice that
/// is not a stop moves to the nearest end: Right to the first stop, Left to
/// the last, as the Speech voice row does for an unknown voice.
pub(super) fn main_voice_step(saved: &str, options: &[String], d: i64) -> Option<String> {
    let mut stops: Vec<Option<&str>> = vec![None];
    stops.extend(options.iter().map(|option| Some(option.as_str())));
    let current = (!saved.is_empty()).then_some(saved);
    let i = match stops.iter().position(|stop| *stop == current) {
        Some(i) => (i as i64 + d).rem_euclid(stops.len() as i64) as usize,
        None if d >= 0 => 0,
        None => stops.len() - 1,
    };
    stops[i].map(str::to_string)
}

/// Whether road events go through the main voice: the separate voice is off,
/// it names the voice that is the main voice right now, or it is not among
/// the separate voices this machine offers (nothing but the main voice can
/// speak).
pub(super) fn events_share_main(s: &Settings, main_name: &str, options: &[String]) -> bool {
    !s.sapi_events || s.event_backend == main_name || !options.contains(&s.event_backend)
}

/// The value of the Driving event voice row. `options` is the separate
/// event voices on offer.
pub(super) fn event_voice_label(s: &Settings, main_name: &str, options: &[String]) -> String {
    if events_share_main(s, main_name, options) {
        "main voice".to_string()
    } else {
        spoken_voice_name(&s.event_backend)
    }
}

/// The Driving event voice row stepped by `d`: `None` for the main voice.
/// The stops are the main voice and then `options`, wrapping.
pub(super) fn event_voice_step(
    s: &Settings,
    main_name: &str,
    options: &[String],
    d: i64,
) -> Option<String> {
    let mut stops: Vec<Option<&str>> = vec![None];
    stops.extend(options.iter().map(|option| Some(option.as_str())));
    let current = (!events_share_main(s, main_name, options)).then_some(s.event_backend.as_str());
    let i = stops.iter().position(|stop| *stop == current).unwrap_or(0) as i64;
    stops[(i + d).rem_euclid(stops.len() as i64) as usize].map(str::to_string)
}

/// The Driving event voice row's help, which names each platform's voices.
pub(super) fn event_voice_help() -> &'static str {
    if crate::device::iphone() {
        "Road events through the main voice or a separate system \
     voice that VoiceOver cannot cut off. The rate, pitch, \
     volume, and voice rows below appear only when the voice \
     supports them. With VoiceOver running, set those in \
     VoiceOver."
    } else if cfg!(target_os = "android") {
        "Road events through the main voice or a second text to \
     speech voice, so a menu line never cuts off a road event. \
     The rate, pitch, volume, and voice rows below appear only \
     when the voice supports them."
    } else {
        "Road events through the main voice or a separate SAPI or \
     OneCore voice a screen reader cannot cut off. The rate, \
     pitch, volume, and voice rows below appear only when the \
     voice supports them. With a screen reader running, set \
     those in the screen reader."
    }
}

/// The value of the Speech voice row: the saved installed voice when the
/// bound voice lists it, otherwise "default".
pub(super) fn speech_voice_label(saved: &str, voices: &[String]) -> String {
    if !saved.is_empty() && voices.iter().any(|voice| voice == saved) {
        saved.to_string()
    } else {
        "default".to_string()
    }
}

/// How long a voice row change waits for the speech worker before the rows
/// are rebuilt and the changed row is spoken anyway.
pub(super) const ROWS_WAIT_S: f64 = 0.3;

/// The Speech rows after a Main voice or Driving event voice change.
///
/// The threaded sink answers from what it published before the switch ran,
/// so rebuilding at once would show the old voice's rows. The changed row
/// is held until the sink has caught up, or [`ROWS_WAIT_S`] has passed.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum RowsWait {
    Idle,
    Waiting { left_s: f64 },
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum RowsEvent {
    /// A voice row changed; `settled` is the sink's answer right after.
    VoiceChanged { settled: bool },
    /// A frame passed.
    Frame { dt: f64, settled: bool },
    /// Any other key, button or gesture: the player has moved on, so the
    /// changed row is no longer theirs to hear.
    OtherInput,
}

/// What a [`RowsWait`] transition asks of the menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RowsAction {
    Nothing,
    /// Rebuild the rows and speak the changed row.
    Speak,
    /// Rebuild the rows silently.
    Rebuild,
}

impl RowsWait {
    /// The next state, and what the menu does now.
    pub(super) fn next(self, event: RowsEvent) -> (Self, RowsAction) {
        match (self, event) {
            (_, RowsEvent::VoiceChanged { settled: true }) => (Self::Idle, RowsAction::Speak),
            (_, RowsEvent::VoiceChanged { settled: false }) => (
                Self::Waiting {
                    left_s: ROWS_WAIT_S,
                },
                RowsAction::Nothing,
            ),
            (Self::Idle, RowsEvent::Frame { .. } | RowsEvent::OtherInput) => {
                (Self::Idle, RowsAction::Nothing)
            }
            (Self::Waiting { .. }, RowsEvent::OtherInput) => (Self::Idle, RowsAction::Rebuild),
            (Self::Waiting { left_s }, RowsEvent::Frame { dt, settled }) => {
                let left_s = left_s - dt;
                if settled || left_s <= 0.0 {
                    (Self::Idle, RowsAction::Speak)
                } else {
                    (Self::Waiting { left_s }, RowsAction::Nothing)
                }
            }
        }
    }
}

/// What decides which Speech rows exist: the main voice's three controls,
/// whether a bound voice lists installed voices, and whether the main voice
/// is JAWS (the arrow keys row).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct RowsSignature {
    rate: bool,
    pitch: bool,
    volume: bool,
    voices: bool,
    jaws: bool,
}

pub(super) fn rows_signature(speech: &dyn SpeechSink) -> RowsSignature {
    RowsSignature {
        rate: speech.supports_rate(),
        pitch: speech.supports_pitch(),
        volume: speech.supports_volume(),
        voices: !speech.voice_names().is_empty(),
        jaws: speech.backend_name().eq_ignore_ascii_case("jaws"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn spoken_names() {
        for (registry, spoken) in [
            ("OneCore", "Windows OneCore"),
            ("Genny", "built-in voice"),
            ("UIA", "Narrator"),
            ("SAPI", "SAPI"),
            ("NVDA", "NVDA"),
            ("Speech Dispatcher", "Speech Dispatcher"),
        ] {
            assert_eq!(spoken_voice_name(registry), spoken);
        }
    }

    #[test]
    fn main_voice_labels() {
        let options = names(&["NVDA", "OneCore", "SAPI", "Genny"]);
        for (saved, label) in [
            ("", "automatic"),
            ("SAPI", "SAPI"),
            ("OneCore", "Windows OneCore"),
            ("Genny", "built-in voice"),
            ("JAWS", "JAWS, unavailable"),
            ("UIA", "Narrator, unavailable"),
        ] {
            assert_eq!(main_voice_label(saved, &options), label, "{saved}");
        }
        assert_eq!(
            main_voice_label("UIA", &names(&["SAPI", "UIA"])),
            "Narrator"
        );
    }

    #[test]
    fn main_voice_steps() {
        let options = names(&["NVDA", "SAPI", "Genny"]);
        let some = |name: &str| Some(name.to_string());
        for (saved, d, next) in [
            ("", 1, some("NVDA")),
            ("NVDA", 1, some("SAPI")),
            ("Genny", 1, None),
            ("", -1, some("Genny")),
            ("NVDA", -1, None),
            // Not a stop: Right to the first, Left to the last.
            ("JAWS", 1, None),
            ("JAWS", -1, some("Genny")),
        ] {
            assert_eq!(main_voice_step(saved, &options, d), next, "{saved} {d}");
        }
        // Nothing can speak: automatic is the only stop.
        assert_eq!(main_voice_step("", &[], 1), None);
        assert_eq!(main_voice_step("JAWS", &[], -1), None);
    }

    fn events(sapi_events: bool, event_backend: &str) -> Settings {
        Settings {
            sapi_events,
            event_backend: event_backend.to_string(),
            ..Settings::default()
        }
    }

    #[test]
    fn event_voice_labels() {
        let options = names(&["OneCore", "SAPI", "Genny"]);
        for (s, main, label) in [
            (events(false, "SAPI"), "NVDA", "main voice"),
            (events(true, "SAPI"), "NVDA", "SAPI"),
            (events(true, "OneCore"), "NVDA", "Windows OneCore"),
            // The event voice names the main voice: they are one voice.
            (events(true, "SAPI"), "SAPI", "main voice"),
            (events(true, "Genny"), "SAPI", "built-in voice"),
        ] {
            assert_eq!(event_voice_label(&s, main, &options), label);
        }
        // Nothing separate on offer: events go through the main voice.
        assert_eq!(
            event_voice_label(&events(true, "SAPI"), "Genny", &[]),
            "main voice"
        );
    }

    #[test]
    fn event_voice_steps() {
        let options = names(&["OneCore", "SAPI"]);
        let some = |name: &str| Some(name.to_string());
        for (s, main, d, next) in [
            (events(false, "SAPI"), "NVDA", 1, some("OneCore")),
            (events(true, "OneCore"), "NVDA", 1, some("SAPI")),
            (events(true, "SAPI"), "NVDA", 1, None),
            (events(false, "SAPI"), "NVDA", -1, some("SAPI")),
            // Sharing the main voice counts as the main voice stop.
            (events(true, "SAPI"), "SAPI", 1, some("OneCore")),
            (events(true, "SAPI"), "SAPI", -1, some("SAPI")),
        ] {
            assert_eq!(event_voice_step(&s, main, &options, d), next);
        }
    }

    #[test]
    fn rows_wait_transitions() {
        use RowsAction::{Nothing, Rebuild, Speak};
        use RowsEvent::{Frame, OtherInput, VoiceChanged};
        let idle = RowsWait::Idle;
        let waiting = |left_s: f64| RowsWait::Waiting { left_s };
        for (state, event, next) in [
            // A sink that is caught up at once: speak now.
            (idle, VoiceChanged { settled: true }, (idle, Speak)),
            (
                idle,
                VoiceChanged { settled: false },
                (waiting(0.3), Nothing),
            ),
            // Another press while waiting starts the wait over.
            (
                waiting(0.1),
                VoiceChanged { settled: false },
                (waiting(0.3), Nothing),
            ),
            (waiting(0.1), VoiceChanged { settled: true }, (idle, Speak)),
            (
                waiting(0.3),
                Frame {
                    dt: 0.1,
                    settled: false,
                },
                (waiting(0.2), Nothing),
            ),
            (
                waiting(0.3),
                Frame {
                    dt: 0.1,
                    settled: true,
                },
                (idle, Speak),
            ),
            // The deadline passes with the sink still behind: speak anyway.
            (
                waiting(0.05),
                Frame {
                    dt: 0.1,
                    settled: false,
                },
                (idle, Speak),
            ),
            // The player moved on: rebuild, and leave the speaking to them.
            (waiting(0.2), OtherInput, (idle, Rebuild)),
            (idle, OtherInput, (idle, Nothing)),
            (
                idle,
                Frame {
                    dt: 0.1,
                    settled: false,
                },
                (idle, Nothing),
            ),
            (
                idle,
                Frame {
                    dt: 0.1,
                    settled: true,
                },
                (idle, Nothing),
            ),
        ] {
            let (got, action) = state.next(event);
            let got = match got {
                RowsWait::Waiting { left_s } => waiting((left_s * 1000.0).round() / 1000.0),
                idle => idle,
            };
            assert_eq!((got, action), next, "{state:?} {event:?}");
        }
    }

    #[test]
    fn speech_voice_labels() {
        let voices = names(&["David", "Zira"]);
        assert_eq!(speech_voice_label("Zira", &voices), "Zira");
        assert_eq!(speech_voice_label("", &voices), "default");
        assert_eq!(speech_voice_label("Kathy", &voices), "default");
        assert_eq!(speech_voice_label("Zira", &[]), "default");
    }
}
