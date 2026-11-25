use super::{Catalog, Translator};

#[test]
fn a_newer_career_notice_keeps_the_version_separator() {
    let catalog = Catalog::from_po(
        "es",
        r#"
msgid "{name} was made in Freight Fate {line}. This version cannot continue it. Nothing was lost: the save is untouched and still works in Freight Fate {line}."
msgstr "La carrera de {name} se creó en Freight Fate {line}. Esta versión no puede continuarla. No se perdió nada: la partida no se modificó y sigue funcionando en Freight Fate {line}."
"#,
    )
    .expect("parses");
    let tr = Translator::new(catalog);
    assert_eq!(
        tr.translate("Future Driver was made in Freight Fate 2.10. This version cannot continue it. Nothing was lost: the save is untouched and still works in Freight Fate 2.10."),
        "La carrera de Future Driver se creó en Freight Fate 2.10. Esta versión no puede continuarla. No se perdió nada: la partida no se modificó y sigue funcionando en Freight Fate 2.10."
    );
}

#[test]
fn a_version_capture_is_verbatim_but_a_measure_still_localizes() {
    let catalog = Catalog::from_po(
        "de",
        r#"
msgid "Version {version}. Distance: {line} miles."
msgstr "Version {version}. Entfernung: {line} Meilen."
"#,
    )
    .expect("parses");
    let tr = Translator::new(catalog);
    assert_eq!(
        tr.translate("Version 2.10. Distance: 4.89 miles."),
        "Version 2.10. Entfernung: 4,89 Meilen."
    );
}

#[test]
fn a_career_name_is_not_translated_but_the_default_driver_is() {
    let catalog = Catalog::from_po(
        "ru",
        r#"
msgid "May"
msgstr "май"

msgid "Driver"
msgstr "Водитель"

msgid "Welcome back, {name}."
msgstr "С возвращением, {name}."

msgid "Driver: {driver_name}"
msgstr "Водитель: {driver_name}"

msgid "Yes, reset {name}"
msgstr "Да, сбросить {name}"

msgid "Locked: {name}"
msgstr "Закрыто: {name}"
"#,
    )
    .expect("parses");
    let tr = Translator::new(catalog);
    assert_eq!(tr.translate("Welcome back, May."), "С возвращением, May.");
    assert_eq!(tr.translate("Welcome back, 2.10."), "С возвращением, 2.10.");
    assert_eq!(
        tr.translate("Welcome back, Driver."),
        "С возвращением, Водитель."
    );
    assert_eq!(tr.translate("Driver: May"), "Водитель: May");
    assert_eq!(tr.translate("Driver: Driver"), "Водитель: Водитель");
    assert_eq!(tr.translate("Yes, reset May."), "Да, сбросить May.");
    assert_eq!(tr.translate("Yes, reset Driver."), "Да, сбросить Водитель.");
    assert_eq!(tr.translate("Locked: May"), "Закрыто: май");
}

#[test]
fn a_custom_career_name_skips_capitalization_and_article_contraction() {
    let catalog = Catalog::from_po(
        "es",
        "msgid \"Welcome back, {name}.\"\nmsgstr \"{name} regresó.\"\n",
    )
    .expect("parses");
    let tr = Translator::new(catalog);
    for name in [
        "casa de el mar",
        "a el río",
        "mi nombre \u{f0000}0\u{f0000}",
    ] {
        assert_eq!(
            tr.translate(&format!("Welcome back, {name}.")),
            format!("{name} regresó.")
        );
    }
}

#[test]
fn a_co_driver_named_june_stays_verbatim_while_calendar_months_translate() {
    let cases = [
        ("vi", include_str!("../../../../../lang/vi.po"), "Tháng Sáu"),
        ("tr", include_str!("../../../../../lang/tr.po"), "Haziran"),
    ];

    for (language, po, month) in cases {
        let translator = Translator::new(Catalog::from_po(language, po).expect("parses"));
        for line in [
            "Depart with June driving, and ride along",
            "Depart with June driving, and sleep",
        ] {
            let departure = translator.translate(line);
            assert_ne!(
                departure, line,
                "{language}: departure line is untranslated"
            );
            assert!(departure.contains("June"), "{language}: {departure}");
            assert!(!departure.contains(month), "{language}: {departure}");
        }
        let pairing = translator.translate("June, your co-driver.");
        assert_ne!(pairing, "June, your co-driver.", "{language}: {pairing}");
        assert!(pairing.starts_with("June"), "{language}: {pairing}");

        let date = crate::sim::season::date_text(24.0 * 100.0);
        let (date_month, day) = date
            .split_once(' ')
            .expect("calendar date has a month and day");
        assert_eq!((date_month, day), ("June", "29"));
        assert_eq!(translator.translate(date_month), month);
        assert!(
            translator.translate("Locked: June").contains(month),
            "{language} generic name placeholder should remain translatable"
        );
    }
}
