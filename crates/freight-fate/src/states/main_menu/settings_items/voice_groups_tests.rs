//! The grouped voice rows against a small list shaped like Prism's.

use super::*;

const US: &str = "English (United States)";

fn list() -> Vec<String> {
    [
        "Google: Deutsch (Deutschland): de-de-x-deb",
        "Google: Deutsch (Deutschland): de-de-x-nfh",
        "Google: English (United Kingdom): en-gb-x-gba",
        "Google: English (United States): en-us-x-iob",
        "Google: English (United States): en-us-x-iog",
        "Eloquence: English (United Kingdom): Reed",
        "Eloquence: English (United States): Reed",
        "Eloquence: English (United States): Shelley",
    ]
    .map(String::from)
    .to_vec()
}

#[test]
fn only_three_part_names_are_grouped() {
    assert!(grouped(&list()));
    assert!(!grouped(&[]));
    let mut mixed = list();
    mixed.push("Zira".to_string());
    assert!(!grouped(&mixed), "a plain name keeps the single row");
}

#[test]
fn the_rows_read_the_saved_voices_parts() {
    let voices = list();
    let saved = "Eloquence: English (United States): Shelley";
    assert_eq!(engine_label(saved, &voices), "Eloquence");
    assert_eq!(language_label(saved, &voices), US);
    assert_eq!(voice_label(saved, &voices), "Shelley");
    // Nothing saved: the engine speaking now is listed first.
    assert_eq!(engine_label("", &voices), "Google");
    assert_eq!(language_label("", &voices), "default");
    assert_eq!(voice_label("gone", &voices), "default");
}

#[test]
fn a_new_engine_keeps_the_language_heard_now() {
    let voices = list();
    let next = step_engine(
        "Google: English (United States): en-us-x-iog",
        &voices,
        None,
        1,
    );
    assert_eq!(
        next.as_deref(),
        Some("Eloquence: English (United States): Reed")
    );
    // Back round to Google, which has German first: the language holds.
    let back = step_engine(
        "Eloquence: English (United Kingdom): Reed",
        &voices,
        None,
        1,
    );
    assert_eq!(
        back.as_deref(),
        Some("Google: English (United Kingdom): en-gb-x-gba")
    );
}

#[test]
fn from_no_saved_voice_an_engine_starts_on_the_phones_language() {
    let voices = list();
    assert_eq!(
        step_engine("", &voices, Some(US), 0).as_deref(),
        Some("Google: English (United States): en-us-x-iob")
    );
    // Another country's English when the engine lacks the phone's own.
    let voices: Vec<String> = list()
        .into_iter()
        .filter(|v| !v.starts_with("Google: English (United States)"))
        .collect();
    assert_eq!(
        step_engine("", &voices, Some(US), 0).as_deref(),
        Some("Google: English (United Kingdom): en-gb-x-gba")
    );
    // No language known: the engine's first.
    assert_eq!(
        step_engine("", &list(), None, 0).as_deref(),
        Some("Google: Deutsch (Deutschland): de-de-x-deb")
    );
}

#[test]
fn the_language_row_steps_through_one_engines_languages() {
    let voices = list();
    let saved = "Google: English (United Kingdom): en-gb-x-gba";
    assert_eq!(
        step_language(saved, &voices, None, 1).as_deref(),
        Some("Google: English (United States): en-us-x-iob")
    );
    assert_eq!(
        step_language(saved, &voices, None, -1).as_deref(),
        Some("Google: Deutsch (Deutschland): de-de-x-deb")
    );
    // Wraps, and never leaves the engine.
    assert_eq!(
        step_language(
            "Google: English (United States): en-us-x-iog",
            &voices,
            None,
            1
        )
        .as_deref(),
        Some("Google: Deutsch (Deutschland): de-de-x-deb")
    );
    assert_eq!(
        step_language("", &voices, Some(US), 1).as_deref(),
        Some("Google: English (United States): en-us-x-iob")
    );
}

#[test]
fn the_voice_row_stays_in_its_engine_and_language() {
    let voices = list();
    let saved = "Eloquence: English (United States): Reed";
    assert_eq!(
        step_voice(saved, &voices, None, 1).as_deref(),
        Some("Eloquence: English (United States): Shelley")
    );
    assert_eq!(
        step_voice(
            "Eloquence: English (United States): Shelley",
            &voices,
            None,
            1
        )
        .as_deref(),
        Some(saved),
        "wraps within the group"
    );
    assert_eq!(
        step_voice("", &voices, Some(US), -1).as_deref(),
        Some("Google: English (United States): en-us-x-iog")
    );
    assert_eq!(step_voice("", &[], Some(US), 1), None);
}

#[test]
fn a_voice_name_may_itself_hold_a_colon() {
    let voices = vec!["Acme: English (United States): Voice: Two".to_string()];
    let saved = voices[0].as_str();
    assert!(grouped(&voices));
    assert_eq!(voice_label(saved, &voices), "Voice: Two");
}
