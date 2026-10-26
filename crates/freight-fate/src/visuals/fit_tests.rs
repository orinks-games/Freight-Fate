//! Every catalog's drawn labels fit the space the display gives them: no
//! gauge label runs into its bar, and no sign word is cut short. Widths are
//! measured as wide as any platform's fallback font draws them, so a test
//! that passes here passes on Linux, Windows and macOS alike.

use std::sync::Arc;

use ff_core::l10n::{Catalog, Translator};

use super::hud::{
    lane_line, GAUGE_LABEL_GAP, GAUGE_LABEL_MAX, GAUGE_TEXT, GAUGE_VALUE_COLUMN, GAUGE_VALUE_GAP,
    LANE_LINE_TEXT, LANE_LINE_WIDTH,
};
use super::signs::{service_rows, SERVICE_LINE_FT, SERVICE_TEXT_FT};
use super::text::{Text, Weight};

/// English, then every catalog in `lang/`.
fn every_language() -> Vec<(&'static str, Text)> {
    let catalogs = [
        ("de", include_str!("../../../../lang/de.po")),
        ("es", include_str!("../../../../lang/es.po")),
        ("fr", include_str!("../../../../lang/fr.po")),
        ("it", include_str!("../../../../lang/it.po")),
        ("ja", include_str!("../../../../lang/ja.po")),
        ("pl", include_str!("../../../../lang/pl.po")),
        ("pt_BR", include_str!("../../../../lang/pt_BR.po")),
        ("ru", include_str!("../../../../lang/ru.po")),
        ("tr", include_str!("../../../../lang/tr.po")),
        ("zh_CN", include_str!("../../../../lang/zh_CN.po")),
    ];
    let mut out = vec![("en", Text::new())];
    for (code, po) in catalogs {
        let translator = Arc::new(Translator::new(
            Catalog::from_po(code, po).expect("the catalog parses"),
        ));
        let mut text = Text::new();
        text.set_translator(&translator);
        out.push((code, text));
    }
    out
}

/// The labels in `english`, translated, that measure wider than `max`.
fn too_wide(text: &mut Text, english: &[&str], size: f32, weight: Weight, max: f32) -> Vec<String> {
    english
        .iter()
        .map(|label| text.tr_drawn(label))
        .collect::<Vec<_>>()
        .into_iter()
        .filter_map(|drawn| {
            let w = text.measure_on_any_platform(&drawn, size, weight);
            (w > max).then(|| format!("{drawn} ({w:.2} > {max:.2})"))
        })
        .collect()
}

#[test]
fn no_gauge_label_or_value_runs_into_its_bar_in_any_language() {
    let mut problems = Vec::new();
    for (code, mut text) in every_language() {
        let labels: Vec<String> = ["FUEL", "AIR", "TRUCK", "CARGO"]
            .iter()
            .map(|label| text.tr_drawn(label))
            .collect();
        // The column grows to the widest label, up to its cap.
        for label in &labels {
            let w = text.measure_on_any_platform(label, GAUGE_TEXT, Weight::Bold);
            if w + GAUGE_LABEL_GAP > GAUGE_LABEL_MAX {
                problems.push(format!(
                    "{code}: {label} ({w:.1}) past the {GAUGE_LABEL_MAX} column"
                ));
            }
        }
        for bad in too_wide(
            &mut text,
            &["100% damage", "150 psi", "100%"],
            GAUGE_TEXT,
            Weight::Regular,
            GAUGE_VALUE_COLUMN - GAUGE_VALUE_GAP,
        ) {
            problems.push(format!("{code}: value {bad}"));
        }
        // The speed box beside the gauges is 54 units wide.
        for bad in too_wide(
            &mut text,
            &["SPEED", "LIMIT"],
            GAUGE_TEXT,
            Weight::Bold,
            48.0,
        ) {
            problems.push(format!("{code}: speed box {bad}"));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn no_service_sign_word_is_cut_short_in_any_language() {
    let mut problems = Vec::new();
    for (code, mut text) in every_language() {
        for english in [
            "WEIGH STATION",
            "ALL TRUCKS",
            "REST AREA",
            "TOLL",
            "PLAZA AHEAD",
            "1 MILE",
            "1/2 MILE",
        ] {
            let drawn = text.tr_drawn(english);
            let rows = service_rows(&mut text, std::slice::from_ref(&drawn));
            if rows.join(" ") != drawn {
                problems.push(format!("{code}: {drawn} breaks inside a word: {rows:?}"));
            }
            if rows.len() > 2 {
                problems.push(format!("{code}: {drawn} takes {} lines", rows.len()));
            }
            for row in &rows {
                let w = text.measure_on_any_platform(row, SERVICE_TEXT_FT, Weight::Bold);
                if w > SERVICE_LINE_FT {
                    problems.push(format!("{code}: {row} ({w:.1} ft)"));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn the_speed_limit_and_road_work_words_fit_their_signs_in_any_language() {
    let mut problems = Vec::new();
    for (code, mut text) in every_language() {
        // A 4 by 5 ft speed limit sign, its words 0.85 ft tall.
        for bad in too_wide(&mut text, &["SPEED", "LIMIT"], 0.85, Weight::Bold, 3.5) {
            problems.push(format!("{code}: speed limit {bad}"));
        }
        // A 5.6 ft road work diamond narrows toward its bottom row, which
        // holds the English AHEAD with a little to spare.
        let s = 5.6 * 0.13;
        for (english, room) in [("ROAD", 3.4), ("WORK", 3.8), ("AHEAD", 2.1)] {
            for bad in too_wide(&mut text, &[english], s, Weight::Bold, room) {
                problems.push(format!("{code}: road work {bad}"));
            }
        }
        // A 7 by 3.6 ft end road work sign.
        for bad in too_wide(
            &mut text,
            &["END", "ROAD WORK"],
            3.6 * 0.26,
            Weight::Bold,
            6.5,
        ) {
            problems.push(format!("{code}: end road work {bad}"));
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn the_lane_line_fits_its_column_for_every_lane_and_mode_in_any_language() {
    use ff_core::settings::LANE_KEEPING_MODES;
    use ff_core::sim::lane::lane_label;
    let lanes = [(1, 3), (1, 2), (0, 2), (0, 1)];
    let mut problems = Vec::new();
    for (code, mut text) in every_language() {
        for (index, count) in lanes {
            // As the drive captures it: the lane's name, capitalized.
            let name = lane_label(index, count);
            let name = name[..1].to_uppercase() + &name[1..];
            for mode in LANE_KEEPING_MODES {
                let line = lane_line(&text, &name, mode);
                let w = text.measure_on_any_platform(&line, LANE_LINE_TEXT, Weight::Regular);
                if w > LANE_LINE_WIDTH {
                    problems.push(format!("{code}: {line} ({w:.1})"));
                }
                if code != "en" && line.contains(" lane  ") {
                    problems.push(format!("{code}: untranslated {line}"));
                }
            }
        }
    }
    assert!(problems.is_empty(), "{problems:#?}");
}

#[test]
fn a_character_overpass_lacks_counts_as_a_full_em_on_any_platform() {
    let mut text = Text::new();
    let latin = text.measure("ROAD", 10.0, Weight::Bold);
    assert_eq!(
        text.measure_on_any_platform("ROAD", 10.0, Weight::Bold),
        latin
    );
    let cjk = text.measure_on_any_platform("この先", 10.0, Weight::Bold);
    assert!(cjk >= 29.99, "three full-width characters: {cjk}");
}
