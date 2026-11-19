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
