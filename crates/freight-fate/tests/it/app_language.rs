//! The language setting: every built-in catalog loads, the chosen language
//! reaches both speech channels and message review, and a translator's own
//! file in the data folder replaces the built-in one.

use ff_core::l10n::{self, Catalog};
use ff_core::models::profile::data_dir;
use freight_fate::app::language::{added_languages, built_in_catalog, offered_languages};
use freight_fate::app::testing::TestApp;

const TEST_CATALOG: &str = r#"
msgid "New screen."
msgstr "Pantalla nueva."

msgid "Truck damage is {damage_pct} percent."
msgstr "El camión tiene un {damage_pct} por ciento de daños."
"#;

fn with_override(app: &TestApp) {
    let dir = data_dir().join("lang");
    std::fs::create_dir_all(&dir).expect("lang folder");
    std::fs::write(dir.join("es.po"), TEST_CATALOG).expect("override written");
    let _ = app;
}

/// Only a catalog that translates something is compiled in (`build.rs`):
/// an empty one would never be offered and only weighs down the download.
#[test]
fn every_built_in_catalog_loads_cleanly_and_translates_something() {
    assert!(built_in_catalog("es").is_some(), "Spanish is built in");
    for lang in l10n::LANGUAGES
        .iter()
        .filter(|lang| lang.code != l10n::ENGLISH)
    {
        let Some(text) = built_in_catalog(lang.code) else {
            continue;
        };
        let catalog = Catalog::from_po(lang.code, text)
            .unwrap_or_else(|err| panic!("{} does not parse: {err}", lang.code));
        assert!(
            catalog.problems.is_empty(),
            "{}: {:?}",
            lang.code,
            catalog.problems
        );
        assert!(
            !catalog.is_empty(),
            "{} is built in with no translations",
            lang.code
        );
    }
    assert_eq!(offered_languages()[0].code, l10n::ENGLISH);
}

#[test]
fn the_chosen_language_reaches_speech_and_message_review() {
    let mut app = TestApp::new();
    with_override(&app);
    app.ctx.set_language("es");
    app.ctx.say("New screen.");
    app.ctx.say_event("Truck damage is 12 percent.");
    assert_eq!(app.main_lines(), vec!["Pantalla nueva."]);
    assert_eq!(
        app.event_lines(),
        vec!["El camión tiene un 12 por ciento de daños."]
    );
    let reviewed: Vec<String> = app
        .ctx
        .message_log
        .filtered_messages()
        .into_iter()
        .map(|m| m.text.clone())
        .collect();
    assert!(
        reviewed.contains(&"El camión tiene un 12 por ciento de daños.".to_string()),
        "{reviewed:?}"
    );
}

#[test]
fn english_and_unknown_lines_pass_through() {
    let mut app = TestApp::new();
    with_override(&app);
    app.ctx.set_language("es");
    app.ctx.say("Nothing in the catalog says this.");
    app.ctx.set_language("en");
    app.ctx.say("New screen.");
    assert_eq!(
        app.main_lines(),
        vec!["Nothing in the catalog says this.", "New screen."]
    );
}

#[test]
fn test_apps_speak_english_whatever_the_setting() {
    // A headless app never loads a catalog on its own, so a developer's
    // system language cannot change what a test hears.
    let mut app = TestApp::new();
    assert!(app.ctx.translator.is_english());
    app.ctx.say("New screen.");
    assert_eq!(app.main_lines(), vec!["New screen."]);
}

#[test]
fn a_language_added_as_a_file_alone_is_found_with_its_name() {
    let dir = tempfile::tempdir().expect("temp dir");
    std::fs::create_dir_all(dir.path().join("lang")).expect("lang folder");
    std::fs::write(
        dir.path().join("lang").join("nl.po"),
        "msgid \"\"\nmsgstr \"Language: nl\\nX-Language-Autonym: Nederlands\\n\"\n\n\
         msgid \"New screen.\"\nmsgstr \"Nieuw scherm.\"\n",
    )
    .expect("file written");
    // A built-in language dropped in the same folder is an override, not
    // a new language.
    std::fs::write(dir.path().join("lang").join("es.po"), TEST_CATALOG).expect("file written");
    let added = added_languages(dir.path());
    assert!(added.contains(&(
        "nl".to_string(),
        "Nederlands".to_string(),
        "Nederlands".to_string()
    )));
    assert!(!added.iter().any(|(code, _, _)| code == "es"));
}
