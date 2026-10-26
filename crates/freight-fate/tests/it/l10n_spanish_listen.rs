//! Real spoken lines run through the shipped Spanish catalog: lines a
//! Spanish listen heard half in English must come out whole.

use crate::states_city_support::*;
use ff_core::l10n::{Catalog, Translator};
use freight_fate::app::testing::TestApp;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::states::city::CityMenuState;

fn spanish() -> Translator {
    let po = include_str!("../../../../lang/es.po");
    Translator::new(Catalog::from_po("es", po).expect("es.po parses"))
}

/// Every English sentence of `line` that is still in its Spanish; short
/// ones (a key name, "Tab status menu") aside.
fn english_left(es: &Translator, line: &str) -> Vec<String> {
    let spanish = es.translate(line);
    line.split(". ")
        .map(|s| s.trim_end_matches('.'))
        .filter(|s| s.split_whitespace().count() >= 5 && spanish.contains(*s))
        .map(str::to_string)
        .collect()
}

fn arrival(city: &str, first_day_done: bool) -> Vec<String> {
    let mut app = TestApp::new();
    career(&mut app, "Playtest", city);
    if first_day_done {
        profile_mut(&mut app)
            .achievements
            .push("first_day".to_string());
    }
    let city = CityMenuState::new(&app.ctx, true);
    app.push_state(city);
    app.main_lines()
}

#[test]
fn every_yard_arrival_is_whole_spanish() {
    // Chicago's yard name is also a catalog entry, which once turned
    // "company yard: Chicago Company Yard" into the label said twice.
    let es = spanish();
    for city in [
        "Aberdeen", "Chicago", "Denver", "Atlanta", "Dallas", "Seattle",
    ] {
        for first_day_done in [false, true] {
            let lines = arrival(city, first_day_done);
            let said = lines.join(" ");
            assert!(said.contains(" dollars."), "{said}");
            for line in &lines {
                assert_eq!(english_left(&es, line), Vec::<String>::new(), "{line}");
                let spanish = es.translate(line);
                assert!(
                    !spanish.contains("patio de la empresa patio de la empresa")
                        && !spanish.contains("Patio de la empresa patio"),
                    "{spanish}"
                );
            }
        }
    }
}

#[test]
fn the_chicago_start_and_kenosha_drive_are_whole_spanish() {
    let es = spanish();
    for first_day_done in [false, true] {
        let mut harness = PlaytestHarness::new();
        let setup = StartDelivery::named("Chicago Listen");
        let setup = if first_day_done {
            setup.configure(|p| p.achievements.push("first_day".to_string()))
        } else {
            setup
        };
        harness.start_delivery(setup);
        let lines = harness.app.main_lines();
        let said = lines.join(" ");
        assert!(said.contains("Chicago Company Yard"), "{said}");
        assert!(said.contains("toward Kenosha"), "{said}");
        assert!(said.contains("to pickup at"), "{said}");
        let mut leaks = Vec::new();
        // The board's market and offer rows and the facility phrases
        // ("retail distribution hub ... in Kenosha") included.
        for line in &lines {
            leaks.extend(english_left(&es, line));
            let spanish = es.translate(line);
            for fragment in [
                "tons of",
                "Market watch",
                "Carrier gross",
                "Deadline",
                "distribution hub",
                " in Kenosha",
                " in Chicago",
                " in ",
                "trainer-recommended",
                "de recomendado",
                " tight",
                " loose",
            ] {
                if spanish.contains(fragment) {
                    leaks.push(format!("{fragment}: {spanish}"));
                }
            }
        }
        assert_eq!(leaks, Vec::<String>::new());
    }
}

#[test]
fn the_keyboard_help_is_whole_spanish() {
    let es = spanish();
    let mut harness = PlaytestHarness::new();
    harness.start_delivery(StartDelivery::named("Help Listen"));
    harness.clear_speech();
    harness.with_drive(|drive, ctx| drive.speak_keyboard_help(ctx));
    let lines = harness.app.main_lines();
    assert!(lines.join(" ").contains("tuning keys"), "{lines:?}");
    for line in &lines {
        assert_eq!(english_left(&es, line), Vec::<String>::new());
    }
}

#[test]
fn a_called_off_lane_keeping_move_is_whole_spanish() {
    // The line lane keeping says when it calls a move off was left
    // untranslated, so a Spanish listen heard it in English.
    let es = spanish();
    for (line, want) in [
        (
            "Staying in the left lane.",
            "Sigues en el carril izquierdo.",
        ),
        ("Staying in the right lane.", "Sigues en el carril derecho."),
        (
            "Staying in the middle lane.",
            "Sigues en el carril central.",
        ),
        (
            "The road narrows to two lanes.",
            "La carretera se estrecha a dos carriles.",
        ),
    ] {
        assert_eq!(es.translate(line), want);
    }
}

#[test]
fn the_radio_sound_row_is_whole_spanish_both_ways() {
    let es = spanish();
    assert_eq!(
        es.translate("Radio sound: Cab speakers"),
        "Sonido de la radio: Altavoces de la cabina"
    );
    assert_eq!(
        es.translate("Radio sound: Clean"),
        "Sonido de la radio: Limpio"
    );
}

#[test]
fn the_visual_display_row_is_whole_spanish_both_ways() {
    let es = spanish();
    assert_eq!(
        es.translate("Visual display: on"),
        "Dibujo en pantalla: activado"
    );
    assert_eq!(
        es.translate("Visual display: off"),
        "Dibujo en pantalla: desactivado"
    );
}
