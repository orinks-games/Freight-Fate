//! The Speech voice rows on Android: engine, then language, then voice.
//!
//! Android's voice list holds every installed engine's voices, each named
//! "engine: language: voice" by Prism's Android backend, and one engine
//! alone can offer hundreds. Three rows browse that list as a tree instead
//! of one long cycle. All three set the one `speech_voice` setting, the
//! full name, so the voice is saved and applied exactly as on any other
//! platform: the engine and language rows only choose which voices the
//! voice row offers, and moving either one picks a voice in the new group
//! at once, so the change is heard.
//!
//! A list whose names do not all have three parts (a Prism without the
//! grouped names) keeps the single Speech voice row.

use super::{adjust, SpeechSpec};
use crate::app::GameContext;
use crate::states::base::Label;
use crate::states::main_menu::settings::SettingsCategoryState;

/// One voice name split into its three parts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Parts<'a> {
    engine: &'a str,
    language: &'a str,
    voice: &'a str,
}

fn parts(name: &str) -> Option<Parts<'_>> {
    let mut pieces = name.splitn(3, ": ");
    Some(Parts {
        engine: pieces.next()?,
        language: pieces.next()?,
        voice: pieces.next()?,
    })
}

/// Whether the voices can be browsed in groups: every name has all three
/// parts.
pub(super) fn grouped(voices: &[String]) -> bool {
    !voices.is_empty() && voices.iter().all(|voice| parts(voice).is_some())
}

/// The distinct values of one part, in list order (Prism lists the engine
/// speaking now first, and each engine's voices by language).
fn distinct<'a>(names: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    let mut out: Vec<&str> = Vec::new();
    for name in names {
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

fn engines(voices: &[String]) -> Vec<&str> {
    distinct(voices.iter().filter_map(|v| parts(v)).map(|p| p.engine))
}

fn languages<'a>(voices: &'a [String], engine: &str) -> Vec<&'a str> {
    distinct(
        voices
            .iter()
            .filter_map(|v| parts(v))
            .filter(|p| p.engine == engine)
            .map(|p| p.language),
    )
}

fn group<'a>(voices: &'a [String], engine: &str, language: &str) -> Vec<&'a String> {
    voices
        .iter()
        .filter(|v| parts(v).is_some_and(|p| p.engine == engine && p.language == language))
        .collect()
}

/// The saved voice's parts, when it is on the list.
fn current<'a>(saved: &'a str, voices: &[String]) -> Option<Parts<'a>> {
    voices
        .iter()
        .any(|voice| voice == saved)
        .then(|| parts(saved))
        .flatten()
}

/// The language an engine starts on: `wanted` when the engine has it, else
/// the same language for another country ("English (United Kingdom)" for
/// "English (United States)"), else its first.
fn home_language<'a>(voices: &'a [String], engine: &str, wanted: Option<&str>) -> Option<&'a str> {
    let offered = languages(voices, engine);
    let base = |name: &str| name.split(" (").next().unwrap_or(name).to_string();
    wanted
        .and_then(|wanted| {
            offered
                .iter()
                .find(|l| **l == wanted)
                .or_else(|| offered.iter().find(|l| base(l) == base(wanted)))
        })
        .or(offered.first())
        .copied()
}

/// The first voice of `engine` in its home language for `wanted`.
fn first_voice(voices: &[String], engine: &str, wanted: Option<&str>) -> Option<String> {
    let language = home_language(voices, engine, wanted)?;
    group(voices, engine, language)
        .first()
        .map(|v| (*v).clone())
}

fn step(len: usize, i: usize, d: i64) -> usize {
    (i as i64 + d).rem_euclid(len as i64) as usize
}

/// The Voice engine row's value: the saved voice's engine, or the engine
/// speaking now (listed first) while no listed voice is saved.
pub(super) fn engine_label(saved: &str, voices: &[String]) -> String {
    current(saved, voices)
        .map(|p| p.engine)
        .or_else(|| engines(voices).first().copied())
        .unwrap_or("default")
        .to_string()
}

/// The Voice language row's value.
pub(super) fn language_label(saved: &str, voices: &[String]) -> String {
    current(saved, voices)
        .map_or("default", |p| p.language)
        .to_string()
}

/// The Speech voice row's value: the voice's own name, without its engine
/// and language.
pub(super) fn voice_label(saved: &str, voices: &[String]) -> String {
    current(saved, voices)
        .map_or("default", |p| p.voice)
        .to_string()
}

/// The voice after the Voice engine row moves by `d`: the next engine's
/// first voice in the language heard now (or the phone's language).
pub(super) fn step_engine(
    saved: &str,
    voices: &[String],
    phone_language: Option<&str>,
    d: i64,
) -> Option<String> {
    let engines = engines(voices);
    let now = current(saved, voices);
    let engine = now.map_or(engines.first().copied(), |p| Some(p.engine))?;
    let i = engines.iter().position(|e| *e == engine).unwrap_or(0);
    let next = engines[step(engines.len(), i, d)];
    let wanted = now.map(|p| p.language).or(phone_language);
    first_voice(voices, next, wanted)
}

/// The voice after the Voice language row moves by `d`: the first voice of
/// the next language in the same engine. From no saved voice the row starts
/// on the phone's language.
pub(super) fn step_language(
    saved: &str,
    voices: &[String],
    phone_language: Option<&str>,
    d: i64,
) -> Option<String> {
    let now = current(saved, voices);
    let engine = now.map_or(engines(voices).first().copied(), |p| Some(p.engine))?;
    let offered = languages(voices, engine);
    let language = match now {
        Some(p) => {
            let i = offered.iter().position(|l| *l == p.language).unwrap_or(0);
            offered[step(offered.len(), i, d)]
        }
        None => home_language(voices, engine, phone_language)?,
    };
    group(voices, engine, language)
        .first()
        .map(|v| (*v).clone())
}

/// The voice after the Speech voice row moves by `d`, within the engine and
/// language. From no saved voice Right takes the first voice of the phone's
/// language and Left the last.
pub(super) fn step_voice(
    saved: &str,
    voices: &[String],
    phone_language: Option<&str>,
    d: i64,
) -> Option<String> {
    match current(saved, voices) {
        Some(p) => {
            let voices = group(voices, p.engine, p.language);
            let i = voices.iter().position(|v| *v == saved).unwrap_or(0);
            Some(voices[step(voices.len(), i, d)].clone())
        }
        None => {
            let engine = *engines(voices).first()?;
            let language = home_language(voices, engine, phone_language)?;
            let voices = group(voices, engine, language);
            let pick = if d >= 0 {
                voices.first()
            } else {
                voices.last()
            };
            pick.map(|v| (*v).clone())
        }
    }
}

/// The phone's language, as Prism names voice languages.
fn phone_language() -> Option<String> {
    #[cfg(target_os = "android")]
    {
        crate::android::default_language_name()
    }
    #[cfg(not(target_os = "android"))]
    {
        None
    }
}

type Stepper = fn(&str, &[String], Option<&str>, i64) -> Option<String>;

impl SettingsCategoryState {
    /// Move one of the grouped voice rows and speak the change in the new
    /// voice.
    fn step_grouped_voice(&mut self, ctx: &mut GameContext, d: i64, stepper: Stepper) {
        let voices = ctx.speech.voice_names();
        let language = phone_language();
        let Some(voice) = stepper(&ctx.settings.speech_voice, &voices, language.as_deref(), d)
        else {
            return;
        };
        ctx.settings.speech_voice = voice;
        ctx.apply_speech();
        self.announce_speech_preview(ctx, "speech_voice");
    }
}

/// The three rows that replace the single Speech voice row.
pub(super) fn specs() -> Vec<SpeechSpec> {
    vec![
        SpeechSpec {
            label: Label::dynamic(|_s, ctx| {
                let voices = ctx.speech.voice_names();
                format!(
                    "Voice engine: {}",
                    engine_label(&ctx.settings.speech_voice, &voices)
                )
            }),
            action: adjust(|s, ctx, d| s.step_grouped_voice(ctx, d, step_engine)),
            help: "Which text to speech engine on your phone the game speaks \
                   with. Changing it picks one of its voices, in the same \
                   language when it has one.",
        },
        SpeechSpec {
            label: Label::dynamic(|_s, ctx| {
                let voices = ctx.speech.voice_names();
                format!(
                    "Voice language: {}",
                    language_label(&ctx.settings.speech_voice, &voices)
                )
            }),
            action: adjust(|s, ctx, d| s.step_grouped_voice(ctx, d, step_language)),
            help: "Which of the engine's languages the Speech voice row offers \
                   voices in. Changing it picks the first voice in that language.",
        },
        SpeechSpec {
            label: Label::dynamic(|_s, ctx| {
                let voices = ctx.speech.voice_names();
                format!(
                    "Speech voice: {}",
                    voice_label(&ctx.settings.speech_voice, &voices)
                )
            }),
            action: adjust(|s, ctx, d| s.step_grouped_voice(ctx, d, step_voice)),
            help: "Which voice of that engine and language the game speaks with.",
        },
    ]
}

#[cfg(test)]
#[path = "voice_groups_tests.rs"]
mod tests;
