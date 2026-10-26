use super::*;

const PO: &str = r#"
msgid ""
msgstr ""
"Language: es\n"

msgid "Settings"
msgstr "Ajustes"

msgid "off"
msgstr "desactivado"

msgid "heavy traffic"
msgstr "tráfico denso"

msgid "It is {tod}."
msgstr "Son las {tod}."

msgid "Truck damage is {damage_pct} percent."
msgstr "El camión tiene un {damage_pct} por ciento de daños."

msgid "{distance}: {condition} ahead."
msgstr "{distance}: {condition} más adelante."

msgid "Back in {where_}."
msgstr "De vuelta en {where_}."

msgid "{count} miles to go."
msgstr "Faltan {count} {count:milla|millas}."

#, fuzzy
msgid "Fuzzy line."
msgstr "No confirmada."

msgid "Untranslated line."
msgstr ""

msgid "Mobile"
msgstr "Móvil"

msgid "Bad hole {a}."
msgstr "Mal {b}."

msgid "Dispatch board"
msgstr "Tablón de cargas"

msgid "{label} {index} of {count}."
msgstr "{label} {index} de {count}."

msgid "{position} of {count}."
msgstr "{position} de {count}."

msgid "{prefix}{tons} tons of {cargo}"
msgstr "{prefix}{tons} toneladas de {cargo}"

msgid "Nuts. We sell them. Next exit."
msgstr "Nueces. Las vendemos. Próxima salida."

msgid "Billboard"
msgstr "Valla"

msgid "Multi"
"line."
msgstr "Varias "
"líneas."
"#;

fn spanish() -> Translator {
    Translator::new(Catalog::from_po("es", PO).expect("test catalog parses"))
}

#[test]
fn english_is_the_identity() {
    let tr = Translator::english();
    assert!(tr.is_english());
    assert!(matches!(tr.translate("It is 3:15 PM."), Cow::Borrowed(_)));
}

#[test]
fn exact_lines_translate() {
    let tr = spanish();
    assert_eq!(tr.translate("Settings"), "Ajustes");
    assert_eq!(tr.translate("Multiline."), "Varias líneas.");
}

#[test]
fn templates_carry_their_captures() {
    let tr = spanish();
    assert_eq!(tr.translate("It is 3:15 PM."), "Son las 3:15 PM.");
    assert_eq!(
        tr.translate("Truck damage is 12 percent."),
        "El camión tiene un 12 por ciento de daños."
    );
}

#[test]
fn captures_are_translated_when_the_catalog_knows_them() {
    let tr = spanish();
    assert_eq!(
        tr.translate("2 miles: heavy traffic ahead."),
        "2 miles: tráfico denso más adelante."
    );
}

#[test]
fn place_names_are_never_translated() {
    let tr = spanish();
    assert_eq!(tr.translate("Back in Mobile."), "De vuelta en Mobile.");
}

#[test]
fn plural_choices_follow_the_number() {
    let tr = spanish();
    assert_eq!(tr.translate("1 miles to go."), "Faltan 1 milla.");
    assert_eq!(tr.translate("3 miles to go."), "Faltan 3 millas.");
}

#[test]
fn sentences_translate_one_at_a_time() {
    let tr = spanish();
    assert_eq!(
        tr.translate("It is 9:00 AM. Truck damage is 3 percent."),
        "Son las 9:00 AM. El camión tiene un 3 por ciento de daños."
    );
    // An unknown sentence stays English beside a known one.
    assert_eq!(
        tr.translate("It is 9:00 AM. Nobody knows this one."),
        "Son las 9:00 AM. Nobody knows this one."
    );
}

#[test]
fn clauses_translate_one_at_a_time() {
    let tr = spanish();
    assert_eq!(tr.translate("Settings: off"), "Ajustes: desactivado");
}

#[test]
fn untranslated_fuzzy_and_broken_entries_are_skipped() {
    let catalog = Catalog::from_po("es", PO).expect("parses");
    assert_eq!(catalog.problems.len(), 1, "{:?}", catalog.problems);
    let tr = Translator::new(catalog);
    assert_eq!(tr.translate("Fuzzy line."), "Fuzzy line.");
    assert_eq!(tr.translate("Untranslated line."), "Untranslated line.");
    assert_eq!(tr.translate("Bad hole x."), "Bad hole x.");
}

#[test]
fn unknown_lines_pass_through_and_are_logged() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("misses.txt");
    let tr = spanish().log_misses_to(path.clone());
    assert_eq!(tr.translate("Nothing here."), "Nothing here.");
    assert_eq!(tr.translate("Nothing here."), "Nothing here.");
    assert_eq!(tr.translate("62"), "62");
    let logged = std::fs::read_to_string(path).expect("miss log written");
    assert_eq!(logged, "Nothing here.\n");
}

#[test]
fn non_ascii_text_survives_splitting() {
    let tr = Translator::new(
        Catalog::from_po(
            "zh_CN",
            "msgid \"It is {tod}.\"\nmsgstr \"现在是{tod}。\"\n\nmsgid \"Settings\"\nmsgstr \"设置\"\n",
        )
        .expect("parses"),
    );
    assert_eq!(
        tr.translate("It is 9:00 AM. Settings"),
        "现在是9:00 AM。设置"
    );
    // Already-translated text passes through without panicking on
    // multi-byte boundaries.
    assert_eq!(tr.translate("现在是。 设置: 你好"), "现在是。 设置: 你好");
}

#[test]
fn locales_map_to_catalogs() {
    assert_eq!(match_locale("es-MX"), Some("es"));
    assert_eq!(match_locale("pt_PT.UTF-8"), Some("pt_BR"));
    assert_eq!(match_locale("zh-Hans-CN"), Some("zh_CN"));
    assert_eq!(match_locale("en-US"), Some("en"));
    assert_eq!(match_locale("ko-KR"), None);
    assert_eq!(resolve(AUTO, Some("de-DE")), "de");
    assert_eq!(resolve(AUTO, None), "en");
    assert_eq!(resolve("fr", Some("de-DE")), "fr");
    assert_eq!(resolve("xx", None), "en");
}

#[test]
fn plural_rules() {
    use plural::plural_index;
    assert_eq!(plural_index("es", 1.0), 0);
    assert_eq!(plural_index("es", 2.0), 1);
    assert_eq!(plural_index("fr", 0.0), 0);
    assert_eq!(plural_index("ru", 21.0), 0);
    assert_eq!(plural_index("ru", 23.0), 1);
    assert_eq!(plural_index("ru", 25.0), 2);
    assert_eq!(plural_index("ru", 11.0), 2);
    assert_eq!(plural_index("pl", 1.0), 0);
    assert_eq!(plural_index("pl", 22.0), 1);
    assert_eq!(plural_index("pl", 21.0), 2);
    assert_eq!(plural_index("ja", 5.0), 0);
    assert_eq!(plural::number_in("1,250 dollars"), Some(1250.0));
    assert_eq!(plural::number_in("2.5 miles"), Some(2.5));
    assert_eq!(plural::number_in("none"), None);
}

#[test]
fn po_round_trips_escapes() {
    let text = "msgid \"a \\\"b\\\"\\n\"\nmsgstr \"c\\\\d\"\n";
    let entries = po::parse(text).expect("parses");
    assert_eq!(entries[0].msgid, "a \"b\"\n");
    assert_eq!(entries[0].msgstr, "c\\d");
    assert_eq!(po::quote("a \"b\"\n"), "\"a \\\"b\\\"\\n\"");
}

#[test]
fn menu_items_with_their_position_translate() {
    let tr = spanish();
    assert_eq!(
        tr.translate("Dispatch board. 3 of 16."),
        "Tablón de cargas. 3 de 16."
    );
    // A numbers-only template never takes words into its holes.
    assert_eq!(tr.translate("Out of fuel."), "Out of fuel.");
}

#[test]
fn an_empty_hole_and_side_by_side_holes_match() {
    let tr = spanish();
    assert_eq!(
        tr.translate("12 tons of heavy traffic"),
        "12 toneladas de tráfico denso"
    );
}

#[test]
fn a_known_passage_inside_a_longer_line_translates_whole() {
    let tr = spanish();
    assert_eq!(
        tr.translate("Billboard: Nuts. We sell them. Next exit. It is 3:15 PM."),
        "Valla: Nueces. Las vendemos. Próxima salida. Son las 3:15 PM."
    );
}

#[test]
fn a_captured_number_takes_the_languages_separators() {
    use super::plural::localize_number;
    assert_eq!(localize_number("de", "25,000").as_deref(), Some("25.000"));
    assert_eq!(localize_number("de", "4.89").as_deref(), Some("4,89"));
    assert_eq!(
        localize_number("fr", "1,250.5").as_deref(),
        Some("1\u{a0}250,5")
    );
    assert_eq!(localize_number("pt_BR", "-2.5").as_deref(), Some("-2,5"));
    // Plain integers, clock times, malformed groups and English-style
    // languages are left alone.
    assert_eq!(localize_number("de", "55"), None);
    assert_eq!(localize_number("de", "3:15"), None);
    assert_eq!(localize_number("de", "12,34"), None);
    assert_eq!(localize_number("ja", "25,000"), None);
    assert_eq!(localize_number("tr", "1,250.5").as_deref(), Some("1.250,5"));
    let catalog = Catalog::from_po(
        "de",
        "msgid \"{count} dollars\"\nmsgstr \"{count} Dollar\"\n",
    )
    .expect("parses");
    let tr = Translator::new(catalog);
    assert_eq!(tr.translate("25,000 dollars"), "25.000 Dollar");
}

#[test]
fn a_the_hole_drops_the_english_article() {
    let catalog = Catalog::from_po(
        "de",
        "msgid \"{the_city} metro freight market\"\nmsgstr \"Frachtmarkt Großraum {the_city}\"\n",
    )
    .expect("parses");
    let tr = Translator::new(catalog);
    assert_eq!(
        tr.translate("the Chicago metro freight market"),
        "Frachtmarkt Großraum Chicago"
    );
}

#[test]
fn a_fragment_opening_a_sentence_or_trimmed_still_matches() {
    let catalog = Catalog::from_po(
        "es",
        "msgid \"confirmed truck parking\"\nmsgstr \"aparcamiento confirmado\"\n\n\
         msgid \"Billboard: \"\nmsgstr \"Valla: \"\n",
    )
    .expect("parses");
    let tr = Translator::new(catalog);
    assert_eq!(
        tr.translate("Confirmed truck parking."),
        "Aparcamiento confirmado."
    );
    assert_eq!(tr.translate("Billboard:"), "Valla:");
}

#[test]
fn a_capitalised_fragment_still_fills_its_template() {
    let catalog = Catalog::from_po(
        "es",
        "msgid \"divided, {lane_word} lanes your side\"\n\
         msgstr \"calzadas separadas, {lane_word} vías en tu sentido\"\n\n\
         msgid \"{lane_word} lanes your side\"\nmsgstr \"{lane_word} vías en tu sentido\"\n\n\
         msgid \"two\"\nmsgstr \"dos\"\n",
    )
    .expect("parses");
    let tr = Translator::new(catalog);
    assert_eq!(
        tr.translate("Divided, two lanes your side."),
        "Calzadas separadas, dos vías en tu sentido."
    );
}

#[test]
fn an_exact_phrase_with_a_full_stop_beats_a_template() {
    let catalog = Catalog::from_po(
        "es",
        "msgid \"{at} the {noun}.\"\nmsgstr \"{at}: {noun}.\"\n\n\
         msgid \"In the right lane\"\nmsgstr \"En el carril derecho\"\n",
    )
    .expect("parses");
    let tr = Translator::new(catalog);
    assert_eq!(tr.translate("In the right lane."), "En el carril derecho.");
}

#[test]
fn side_by_side_holes_split_before_a_comma_tail() {
    let catalog = Catalog::from_po(
        "de",
        "msgid \"Merge onto {shield} toward {toward}{on_it_tail}.\"\n\
         msgstr \"Auf {shield} Richtung {toward} auffahren{on_it_tail}.\"\n\n\
         msgid \", then {distance} on it\"\nmsgstr \", dann {distance} darauf\"\n",
    )
    .expect("parses");
    let tr = Translator::new(catalog);
    assert_eq!(
        tr.translate("Merge onto I-90 East toward Rochester, New York, then 74 miles on it."),
        "Auf I-90 East Richtung Rochester, New York auffahren, dann 74 miles darauf."
    );
}

#[test]
fn the_settings_name_gives_the_english_name_then_the_language_own() {
    let label = |code| super::language(code).map(Language::label);
    assert_eq!(label("es").as_deref(), Some("Spanish (Español)"));
    assert_eq!(label("fr").as_deref(), Some("French (Français)"));
    assert_eq!(
        label("zh_CN").as_deref(),
        Some("Simplified Chinese (简体中文)")
    );
    assert_eq!(label("en").as_deref(), Some("English"));
}

#[test]
fn a_language_added_from_a_file_is_known_and_matches_its_locale() {
    // The only test that adds a language: the list is set once a process.
    super::add_languages([
        (
            "tlh".to_string(),
            "Klingon".to_string(),
            "tlhIngan Hol".to_string(),
        ),
        (
            "es".to_string(),
            "not added".to_string(),
            "not added again".to_string(),
        ),
    ]);
    assert_eq!(
        super::language("tlh").map(|lang| lang.autonym),
        Some("tlhIngan Hol")
    );
    assert_eq!(
        super::language("tlh").map(Language::label).as_deref(),
        Some("Klingon (tlhIngan Hol)")
    );
    assert_eq!(match_locale("tlh-US"), Some("tlh"));
    assert_eq!(
        super::language("es").map(|lang| lang.autonym),
        Some("Español")
    );
}

#[test]
fn a_po_header_field_is_read_by_name() {
    let text = "msgid \"\"\nmsgstr \"Language: nl\\nX-Language-Autonym: Nederlands\\n\"\n";
    assert_eq!(
        super::po::header_field(text, "x-language-autonym").as_deref(),
        Some("Nederlands")
    );
    assert_eq!(super::po::header_field(text, "Plural-Forms"), None);
}

const MIXED_PO: &str = r#"
msgid ""
msgstr ""
"Language: es\n"

msgid "Language: {language_label}"
msgstr "Idioma: {language_label}"

msgid "automatic, {name}"
msgstr "automático, {name}"

msgid "Back"
msgstr "Volver"

msgctxt "controller button"
msgid "Back"
msgstr "Back"

msgid "How to play"
msgstr "Cómo jugar"

msgid "Left"
msgstr "Izquierda"

msgid "Up"
msgstr "Arriba"

msgid "and"
msgstr "y"

msgid "Route"
msgstr "Ruta"

msgid "{count} left"
msgstr "quedan {count}"

msgid "Lifetime {what}"
msgstr "{what} en total"

msgid "Escape goes back"
msgstr "Escape vuelve atrás"

msgid "{position} of {count}."
msgstr "{position} de {count}."
"#;

fn mixed() -> Translator {
    Translator::new(Catalog::from_po("es", MIXED_PO).expect("test catalog parses"))
}

#[test]
fn the_automatic_language_row_translates_whole() {
    assert_eq!(
        mixed().translate("Language: automatic, Español."),
        "Idioma: automático, Español."
    );
}

#[test]
fn the_back_row_is_the_plain_entry_and_the_button_name_is_left_out() {
    let catalog = Catalog::from_po("es", MIXED_PO).expect("parses");
    let entries = po::parse(MIXED_PO).expect("parses");
    let button = entries
        .iter()
        .find(|entry| entry.msgctxt.as_deref() == Some("controller button"))
        .expect("the button entry keeps its context");
    assert_eq!(button.msgid, "Back");
    // Whichever order the file lists them, the row's translation is spoken.
    assert_eq!(
        Translator::new(catalog).translate("Back. 7 of 7."),
        "Volver. 7 de 7."
    );
    let reversed = MIXED_PO.replace(
        "msgid \"Back\"\nmsgstr \"Volver\"\n\nmsgctxt \"controller button\"\nmsgid \"Back\"\nmsgstr \"Back\"",
        "msgctxt \"controller button\"\nmsgid \"Back\"\nmsgstr \"Back\"\n\nmsgid \"Back\"\nmsgstr \"Volver\"",
    );
    assert_ne!(reversed, MIXED_PO);
    let tr = Translator::new(Catalog::from_po("es", &reversed).expect("parses"));
    assert_eq!(tr.translate("Back."), "Volver.");
}

#[test]
fn a_sentence_with_only_single_words_matching_stays_english() {
    let tr = mixed();
    let intro = "Left and Right arrows or Page Up and Page Down change pages. \
                 Up and Down let you read a little at a time.";
    assert_eq!(tr.translate(intro), intro);
    // Clauses go all together or not at all.
    assert_eq!(tr.translate("Route: 2 miles of 2"), "Route: 2 miles of 2");
    assert_eq!(
        tr.translate("Lifetime earnings: 0 dollars."),
        "Lifetime earnings: 0 dollars."
    );
    // A whole sentence the catalog has still translates beside English ones.
    assert_eq!(
        tr.translate("Up and Down read it. Escape goes back."),
        "Up and Down read it. Escape vuelve atrás."
    );
    // A name-like clause does not hold its sentence back.
    assert_eq!(
        tr.translate("Route: Bemidji Company Yard"),
        "Ruta: Bemidji Company Yard"
    );
}

#[test]
fn the_position_cue_translates_apart_from_its_row() {
    let tr = mixed();
    // The row is in the catalog.
    assert_eq!(tr.translate("How to play. 4 of 8."), "Cómo jugar. 4 de 8.");
    // The row is not; the cue is still the player's language.
    assert_eq!(
        tr.translate("Business status. 4 of 14."),
        "Business status. 4 de 14."
    );
    // A range is not a position cue.
    assert_eq!(
        tr.translate("Results 26 to 50 of 52."),
        "Results 26 to 50 of 52."
    );
    // Without a cue translation the cue stays English.
    let tr = Translator::new(
        Catalog::from_po("es", "msgid \"How to play\"\nmsgstr \"Cómo jugar\"\n").expect("parses"),
    );
    assert_eq!(tr.translate("How to play. 4 of 8."), "Cómo jugar. 4 of 8.");
}

#[test]
fn a_template_without_a_stop_beats_a_looser_one_with_it() {
    let tr = Translator::new(
        Catalog::from_po(
            "es",
            r#"
msgid "Deliveries: {deliveries}, {f} percent on time"
msgstr "Entregas: {deliveries}, {f} por ciento a tiempo"

msgid "{f} percent {what}."
msgstr "{f} por ciento de {what}."
"#,
        )
        .expect("parses"),
    );
    assert_eq!(
        tr.translate("Deliveries: 3, 50 percent on time."),
        "Entregas: 3, 50 por ciento a tiempo."
    );
}

#[test]
fn eld_status_translates_as_whole_sentences() {
    let po = r#"
msgid ""
msgstr ""
"Language: es\n"

msgid "ELD status on duty not driving."
msgstr "Estado del ELD: en servicio sin conducir."

msgid "Hours of service: {duration_text} of driving left, {duration_text2} of duty window left."
msgstr "Horas de servicio: quedan {duration_text} de conducción y {duration_text2} de ventana de servicio."

msgid "Hold {up} to accelerate, {down} to brake."
msgstr "Mantén {up} para acelerar y {down} para frenar."
"#;
    let t = Translator::new(Catalog::from_po("es", po).expect("parses"));
    assert_eq!(
        t.translate(
            "ELD status on duty not driving. Hours of service: 9 hours of driving left, 3 hours of duty window left."
        ),
        "Estado del ELD: en servicio sin conducir. Horas de servicio: quedan 9 hours de conducción y 3 hours de ventana de servicio."
    );
    // A sentence the catalog lacks stays whole English beside one it has.
    assert_eq!(
        t.translate(
            "Hold Up arrow to accelerate, Down arrow to brake. The throttle key never latches."
        ),
        "Mantén Up arrow para acelerar y Down arrow para frenar. The throttle key never latches."
    );
}

/// Lines a Spanish listen heard mixed although the catalog had every
/// sentence: the matcher, not the catalog, left them English.
const LISTEN_PO: &str = r#"
msgid ""
msgstr ""
"Language: es\n"

msgid "Place callouts: {place_callouts}"
msgstr "Avisos de lugares: {place_callouts}"

msgid "sparse"
msgstr "pocos"

msgid "Driving speech: {replace}"
msgstr "Avisos de conducción: {replace}"

msgid "standard"
msgstr "estándar"

msgid "Open-road target: {open_road_target}"
msgstr "Objetivo en carretera abierta: {open_road_target}"

msgid "posted limit when the open road begins"
msgstr "límite señalizado cuando empiece la carretera abierta"

msgid "How to play. {controls} {page_title}"
msgstr "Cómo jugar. {controls} {page_title}"

msgid "Left and Right arrows change pages. Escape goes back."
msgstr "Las flechas izquierda y derecha cambian de página. Escape vuelve atrás."

msgid "Left"
msgstr "Izquierda"

msgid "Page {page} of {total}: {title}."
msgstr "Página {page} de {total}: {title}."

msgid "Main menu"
msgstr "Menú principal"

msgid "It is {tod}."
msgstr "Son las {tod}."

msgid "{remaining} remaining of {f}"
msgstr "quedan {remaining} de {f}"

msgid "{place}{state} on {highway}"
msgstr "{place}{state} en {highway}"

msgid "Next place in {ahead_text}: {text}."
msgstr "Próximo lugar en {ahead_text}: {text}."

msgid "{carrier} is looking for steady service."
msgstr "{carrier} busca un servicio constante."

msgid " Career objective: {terminal_text}"
msgstr " Objetivo de carrera: {terminal_text}"

msgid "You have {grouped} dollars."
msgstr "Tienes {grouped} dólares."

msgid "ELD: {summary}"
msgstr "{summary}"

msgid "ELD status on duty not driving."
msgstr "Estado del ELD: en servicio sin conducir."

msgid "Driver: {driver_name}"
msgstr "Conductor: {driver_name}"

msgid "Tap the brake to latch it."
msgstr "Toca el freno para retenerlo."

msgid "the Down arrow"
msgstr "la flecha abajo"

msgid "{down} once releases it."
msgstr "{down} una vez lo suelta."
"#;

fn listen() -> Translator {
    Translator::new(Catalog::from_po("es", LISTEN_PO).expect("listen catalog parses"))
}

#[test]
fn a_value_hole_named_like_a_place_is_still_translated() {
    let tr = listen();
    // "place_callouts", "replace" and "open_road_target" hold values, not
    // names: they were matched as names because "place" and "road" sit
    // inside them.
    assert_eq!(
        tr.translate("Place callouts: sparse."),
        "Avisos de lugares: pocos."
    );
    assert_eq!(
        tr.translate("Driving speech: standard."),
        "Avisos de conducción: estándar."
    );
    assert_eq!(
        tr.translate("Open-road target: posted limit when the open road begins."),
        "Objetivo en carretera abierta: límite señalizado cuando empiece la carretera abierta."
    );
}

#[test]
fn two_holes_split_by_a_space_split_where_a_sentence_ends() {
    assert_eq!(
        listen().translate(
            "How to play. Left and Right arrows change pages. Escape goes back. Page 1 of 19: Main menu."
        ),
        "Cómo jugar. Las flechas izquierda y derecha cambian de página. Escape vuelve atrás. \
         Página 1 de 19: Menú principal."
    );
}

#[test]
fn a_sentence_opens_with_a_capital_and_names_need_few_letters_of_frame() {
    let tr = listen();
    assert_eq!(
        tr.translate("It is 8 AM. 451 miles remaining of 451."),
        "Son las 8 AM. Quedan 451 miles de 451."
    );
    assert_eq!(
        tr.translate("Next place in 20 miles: Groton, South Dakota on US-12."),
        "Próximo lugar en 20 miles: Groton, South Dakota en US-12."
    );
}

#[test]
fn a_joined_piece_and_a_name_with_a_label_still_translate() {
    assert_eq!(
        listen().translate(
            "You have 5,000 dollars.  Career objective: Northstar Freight Lines is looking for steady service."
        ),
        "Tienes 5.000 dólares.  Objetivo de carrera: Northstar Freight Lines busca un servicio constante."
    );
}

#[test]
fn a_summary_without_its_full_stop_finds_its_sentence() {
    let tr = listen();
    assert_eq!(
        tr.translate("ELD: ELD status on duty not driving."),
        "Estado del ELD: en servicio sin conducir."
    );
    assert_eq!(tr.translate("Driver: Playtest."), "Conductor: Playtest.");
}

#[test]
fn help_sentences_after_a_key_name_translate_whole() {
    assert_eq!(
        listen().translate("Tap the brake to latch it. The Down arrow once releases it."),
        "Toca el freno para retenerlo. La flecha abajo una vez lo suelta."
    );
}

#[test]
fn a_clock_and_its_zone_translate_as_one_reading() {
    let po = r#"
msgid ""
msgstr ""
"Language: es\n"

msgid "Time: {clock}, {deadline_text}"
msgstr "Hora: {clock}, {deadline_text}"

msgid "{clock} Central Time"
msgstr "{clock} hora del Centro"

msgid "due tomorrow"
msgstr "vence mañana"
"#;
    let tr = Translator::new(Catalog::from_po("es", po).expect("parses"));
    assert_eq!(
        tr.translate("Time: 8:02 AM Central Time, due tomorrow."),
        "Hora: 8:02 AM hora del Centro, vence mañana."
    );
}

/// The shipped Spanish catalog, as a player gets it.
fn shipped_spanish() -> Translator {
    let po = include_str!("../../../../lang/es.po");
    Translator::new(Catalog::from_po("es", po).expect("lang/es.po parses"))
}

/// The shipped Simplified Chinese catalog, as a player gets it.
fn shipped_chinese() -> Translator {
    let po = include_str!("../../../../lang/zh_CN.po");
    Translator::new(Catalog::from_po("zh_CN", po).expect("lang/zh_CN.po parses"))
}

#[test]
fn shipped_chinese_translates_dynamic_online_driver_rows() {
    let tr = shipped_chinese();
    assert_eq!(
        tr.translate(
            "Alayna Mayfield. Driving: Memphis, Tennessee to Rochester, Minnesota. grain, 5% there, short-haul stubnose. updated 2 minutes ago."
        ),
        "Alayna Mayfield. 正在驾驶：从Memphis, Tennessee前往Rochester, Minnesota。谷物，已行驶5%，短途平头车。2分钟前更新。"
    );
    assert_eq!(
        tr.translate(
            "Alex Bustos. Driving: Phoenix, Arizona to Buckeye, Arizona. general freight, 20% there, standard rig, listening to KGOL, Cruisin' Gold 105.9. updated just now."
        ),
        "Alex Bustos. 正在驾驶：从Phoenix, Arizona前往Buckeye, Arizona。普通货物，已行驶20%，标准卡车，正在收听KGOL, Cruisin' Gold 105.9。刚刚更新。"
    );
}

#[test]
fn shipped_chinese_translates_dynamic_region_and_training_rows() {
    let tr = shipped_chinese();
    assert_eq!(
        tr.translate("California (44 cities). 3 of 16."),
        "California（44个城市）。第 3 项，共 16 项。"
    );
    assert_eq!(
        tr.translate(
            "Refrigerated certificate course: 900 dollars, carrier-sponsored free at level 2. 2 of 11."
        ),
        "冷藏培训证书课程：900美元，2级时承运商免费资助。第 2 项，共 11 项。"
    );
    assert_eq!(
        tr.translate(
            "Hazmat endorsement course: 185 dollars, background check clears in about 30 days. 8 of 11."
        ),
        "危险品驾照签注课程：185美元，背景调查大约30天后通过。第 8 项，共 11 项。"
    );
}

#[test]
fn shipped_chinese_translates_dispatch_pickup_and_driving_lines() {
    let tr = shipped_chinese();
    assert_eq!(
        tr.translate(
            "Needs an extra rest on this shift's hours. ELD status off duty. Hours of service: 11 hours of driving left, 5.2 hours left in the 14-hour window. Sleeper split open: 7 more hours in the sleeper to pair it. Enter again to accept."
        ),
        "按本次轮班的工时计算，需要额外休息。ELD 状态：下班。驾驶工时：剩余驾驶时间11小时，14小时当班时段剩余5.2小时。卧铺分段休息尚未配对：还需在卧铺休息7小时。再次按Enter接受。"
    );
    assert_eq!(
        tr.translate("Loaded and sealed for Valencia, California."),
        "已装货并封好，运往Valencia, California。"
    );
    assert_eq!(
        tr.translate("Dry van 9195 checks out."),
        "厢式挂车9195检查无异常。"
    );
    assert_eq!(
        tr.translate(
            "On I-10 toward Indio, California. Next stop in 1 mile: travel center ahead at exit 239. Automatic transmission."
        ),
        "在 I-10 上朝向 Indio, California。前方1英里是下一个停靠点：前方239号出口处的卡车服务区。自动变速箱。"
    );
}

#[test]
fn shipped_spanish_says_every_eld_status_whole() {
    // The ELD app speaks the summary after "ELD: " and the Driver Hours
    // row after "Hours: "; both wrappers drop the summary's last stop. A
    // loose "{driven} driving" once took the stopped line and left
    // "ELD status en duty not de conducción".
    let tr = shipped_spanish();
    let hours = "Hours of service: 11 hours of driving left, 30-minute break due in 8 hours, \
                 14 hours left in the 14-hour window";
    let spanish_hours = "Horas de servicio: quedan 11 horas de conducción, la pausa de 30 minutos \
                         toca en 8 horas y quedan 14 horas de la ventana de 14 horas";
    for (status, spoken) in [
        ("driving", "conduciendo"),
        ("on duty not driving", "en servicio sin conducir"),
        ("off duty", "fuera de servicio"),
        ("sleeper berth", "litera"),
    ] {
        assert_eq!(
            tr.translate(&format!("ELD: ELD status {status}. {hours}. 1 of 4.")),
            format!("Estado del ELD: {spoken}. {spanish_hours}. 1 de 4."),
            "{status}"
        );
        assert_eq!(
            tr.translate(&format!("Hours: ELD status {status}. {hours}. 10 of 12.")),
            format!("Horas: Estado del ELD: {spoken}. {spanish_hours}. 10 de 12."),
            "{status}"
        );
    }
}

#[test]
fn shipped_spanish_keeps_a_template_hole_to_one_sentence() {
    // "{pct} percent {direction} ahead{length}. {advice}" once took most of
    // the F1 help as its percent and left two help sentences English.
    let tr = shipped_spanish();
    assert_eq!(
        tr.translate(
            "Page Down tunes to the next station, Page Up to the previous; semicolon and \
             apostrophe do the same. Control with the tuning keys jumps a category; Shift \
             changes the radio volume in 10 percent steps, on or off. S posted speed limit. \
             G the grade under the wheels, whether the truck is holding it, and the next \
             grade ahead. Tab status menu."
        ),
        "Page Down sintoniza la emisora siguiente y Page Up la anterior; punto y coma y \
         apóstrofo hacen lo mismo. Control con las teclas de sintonía salta de categoría; \
         Mayúsculas cambia el volumen de la radio de 10 en 10 por ciento, o la enciende o \
         apaga. S el límite de velocidad señalizado. G la pendiente bajo las ruedas, si el \
         camión la está aguantando, y la próxima pendiente. Tab el menú de estado."
    );
}

#[test]
fn shipped_spanish_says_a_yard_once_and_joins_de_el() {
    let tr = shipped_spanish();
    assert_eq!(
        tr.translate("company yard: Chicago Company Yard in Chicago. 5,000 dollars."),
        "patio de la empresa: Chicago Company Yard en Chicago. 5.000 dólares."
    );
    assert_eq!(
        tr.translate(
            "Dropping and hooking took 25 minutes. Hooked to dry van 3890, in good shape."
        ),
        "El intercambio de remolque llevó 25 minutos. Enganchado a furgón seco 3890, en buen \
         estado."
    );
    assert_eq!(
        tr.translate(
            "On I-94 toward Kenosha, Wisconsin. Next stop in 1 mile: travel center ahead at exit 50B."
        ),
        "En I-94 hacia Kenosha, Wisconsin. Próxima parada en 1 milla: área de servicio más \
         adelante en la salida 50B."
    );
}

#[test]
fn shipped_spanish_says_places_board_rows_and_regions_whole() {
    // Facility kinds and "in {town}" in Spanish with the names kept; the
    // market watch, the assigned job row, the region count and the terrain
    // as whole Spanish sentences.
    let tr = shipped_spanish();
    for (english, spanish) in [
        ("Pulling into freight terminal Bemidji Company Yard in Bemidji.", "Entrando en terminal de carga Bemidji Company Yard en Bemidji."),
        ("Loaded for freight terminal Macon Terminal in Macon.", "Cargado para terminal de carga Macon Terminal en Macon."),
        ("Dispatch routed you to retail distribution hub Amazon Fulfillment Center KRB1 in Kenosha, Wisconsin.", "Despacho te ha enviado a centro de distribución comercial Amazon Fulfillment Center KRB1 en Kenosha, Wisconsin."),
        ("Market watch: automotive tight, farm inputs tight, lumber paper loose, steel loose.", "Vistazo al mercado: automoción al alza, insumos agrícolas al alza, madera y papel a la baja, acero a la baja."),
        ("16 tons of general freight from Chicago Parcel Hub in Chicago, Illinois to the Fargo metro freight market. 62 miles. Carrier gross 332 dollars. Deadline 3 hours. Equipment: dry van trailer.", "16 toneladas de carga general desde Chicago Parcel Hub en Chicago, Illinois hasta el mercado de carga metropolitano de Fargo. 62 millas. Ingreso bruto de la empresa de transporte: 332 dólares. Plazo de entrega: 3 horas. Equipo: remolque de furgón seco."),
        ("16 tons of general freight from Chicago Parcel Hub in Chicago, Illinois to Amazon Fulfillment Center KRB1 in Kenosha, Wisconsin. 62 miles. Gross revenue 332 dollars. Deadline 12 hours, includes a 10-hour rest. Equipment: dry van trailer.", "16 toneladas de carga general desde Chicago Parcel Hub en Chicago, Illinois hasta Amazon Fulfillment Center KRB1 en Kenosha, Wisconsin. 62 millas. Ingresos brutos: 332 dólares. Plazo de entrega: 12 horas, incluye un descanso de 10 horas. Equipo: remolque de furgón seco."),
        (
            "Accepted. Deadhead 1.1 miles on South Canal Street to pickup at Chicago Parcel Hub in Chicago, Illinois.",
            "Aceptado. Trayecto en vacío de 1,1 millas por South Canal Street hasta la recogida en Chicago Parcel Hub en Chicago, Illinois.",
        ),
        (
            "Pulling into port Port of Tacoma in Tacoma, Washington.",
            "Entrando en puerto Port of Tacoma en Tacoma, Washington.",
        ),
        (
            "Loaded for warehouse Acme Storage in Reno, Nevada.",
            "Cargado para almacén Acme Storage en Reno, Nevada.",
        ),
        (
            "On I-5 toward Portland, Oregon. One lane your side. Current grade 0.1 percent downhill, terrain hills.",
            "En I-5 hacia Portland, Oregon. Un carril en tu sentido. Pendiente actual 0,1 por ciento de bajada, terreno de colinas.",
        ),
        (
            "On I-5 toward Portland, Oregon. Divided, one lane your side.",
            "En I-5 hacia Portland, Oregon. Con calzadas separadas, un carril en tu sentido.",
        ),
        (
            "Down to two lanes your side. Road widens to three lanes your side.",
            "Se reduce a dos carriles en tu sentido. La carretera se ensancha a tres carriles en tu sentido.",
        ),
        ("Great Lakes (29 cities). 8 of 16.", "Great Lakes (29 ciudades). 8 de 16."),
        ("Current grade 2.0 percent downhill, terrain hills.", "Pendiente actual 2,0 por ciento de bajada, terreno de colinas."),
        ("You have 5,000 dollars. First-day objective: open the dispatch board and accept your assigned trainer-recommended load.", "Tienes 5.000 dólares. Objetivo del primer día: abre el tablón de trabajos y acepta la carga asignada que recomienda el instructor."),
    ] {
        assert_eq!(tr.translate(english), spanish);
    }
}

#[test]
fn shipped_spanish_dispatch_trust_bands_agree_in_gender() {
    // "full" alone is "completo" for lane assist; trust lines must not use it.
    // Whole per-band openings keep feminine agreement with "confianza".
    use crate::models::enforcement::{
        dispatch_trust_line_with_display, trust_text_with_display, TRUST_FULL, TRUST_GUARDED,
        TRUST_LAST_CHANCE, TRUST_POOR,
    };
    use crate::models::solvency::test_profile::FakeProfile;
    use crate::settings::ScoreDisplay;

    let tr = shipped_spanish();
    assert_eq!(tr.translate("full"), "completo");

    let bands = [
        (100.0, TRUST_FULL, "completa"),
        (35.0, TRUST_GUARDED, "con reservas"),
        (22.0, TRUST_POOR, "baja"),
        (10.0, TRUST_LAST_CHANCE, "última oportunidad"),
    ];
    for (rep, band, spanish_band) in bands {
        assert_eq!(crate::models::enforcement::trust_band(rep), band);
        for mode in [
            ScoreDisplay::Both,
            ScoreDisplay::Percent,
            ScoreDisplay::Description,
        ] {
            let mut profile = FakeProfile::new();
            profile.reputation = rep;
            let line = dispatch_trust_line_with_display(&profile, mode);
            let spoken = tr.translate(&line);
            assert!(
                spoken.contains(&format!("Confianza de despacho: {spanish_band}")),
                "mode={mode:?} band={band} english={line:?} spanish={spoken:?}"
            );
            assert!(
                !spoken.contains("Confianza de despacho: completo"),
                "mode={mode:?} band={band} kept masculine completo: {spoken:?}"
            );
            if mode != ScoreDisplay::Description {
                let head = trust_text_with_display(rep, mode);
                let head_es = tr.translate(&head);
                assert!(
                    head_es.contains(&format!("Confianza de despacho: {spanish_band}")),
                    "trust_text mode={mode:?} band={band} english={head:?} spanish={head_es:?}"
                );
            }
        }
    }

    // Debt hold: service is full, money sets the band. No reputation number.
    let mut debt = FakeProfile::new();
    debt.reputation = 85.0;
    debt.money = -5_000.0;
    for mode in [
        ScoreDisplay::Both,
        ScoreDisplay::Percent,
        ScoreDisplay::Description,
    ] {
        let line = dispatch_trust_line_with_display(&debt, mode);
        assert!(!line.contains("out of 100"), "{line}");
        let spoken = tr.translate(&line);
        let band = crate::models::enforcement::standing_band(&debt);
        let spanish_band = match band {
            TRUST_GUARDED => "con reservas",
            TRUST_POOR => "baja",
            TRUST_LAST_CHANCE => "última oportunidad",
            other => panic!("debt should hold the band below full, got {other}"),
        };
        assert!(
            spoken.contains(&format!("Confianza de despacho: {spanish_band}")),
            "debt hold mode={mode:?} band={band} english={line:?} spanish={spoken:?}"
        );
        assert!(
            !spoken.contains("Confianza de despacho: completo"),
            "debt hold kept masculino completo: {spoken:?}"
        );
        assert!(
            spoken.contains("debes") || spoken.contains("Debes") || spoken.contains("5.000"),
            "debt hold should name the debt: {spoken:?}"
        );
    }

    for (band, spanish_band) in [
        ("guarded", "con reservas"),
        ("poor", "baja"),
        ("last chance", "última oportunidad"),
    ] {
        let clause = crate::models::career::xp_rate_clause(band);
        let spoken = tr.translate(&clause);
        assert!(
            spoken.contains(spanish_band),
            "xp clause band={band} english={clause:?} spanish={spoken:?}"
        );
        assert!(!spoken.contains("completo"), "{spoken}");
    }
}

#[test]
fn every_facility_kind_and_market_class_has_its_phrases() {
    // lang/extra.pot carries a whole phrase per facility kind and per
    // market class; a new kind or class needs its own.
    let extra = include_str!("../../../../lang/extra.pot");
    for (_, label) in crate::data::world_constants::LOCATION_TYPE_LABELS {
        if *label == "metro freight market" {
            continue;
        }
        for phrase in [
            format!("{label} {{place_name}} in {{city}}"),
            format!("{label} {{place_name}} near {{town}} in {{city}}"),
        ] {
            assert!(extra.contains(&format!("msgid \"{phrase}\"")), "{phrase}");
        }
    }
    for key in crate::models::market::MARKET_CARGO_KEYS {
        let class = key.replace('_', " ");
        for condition in ["tight", "loose"] {
            let phrase = format!("{class} {condition}");
            assert!(extra.contains(&format!("msgid \"{phrase}\"")), "{phrase}");
        }
    }
}

#[test]
fn a_long_line_of_commas_is_matched_without_retrying_every_cut() {
    // The Spanish keyboard help is one 5,600-character line, and
    // "{a}, {b} to {c}, {d}, {e}{f}." tried every way of cutting it at its
    // commas: a minute on the game thread for one key press. A line like
    // it, which the template cannot fill, must be turned down at once; the
    // old search would not finish this before the heat death of the run.
    let template = Template::new(
        parse_source("{a}, {b} to {c}, {d}, {e}{f}."),
        parse_translation("{a}, {b} a {c}, {d}, {e}{f}."),
    )
    .expect("the template has literal letters");
    let line = "one, two to three, ".repeat(200) + "no full stop";
    assert!(template.captures(&line).is_none());
    // And one it can fill still is, the same way as before.
    let caps = template
        .captures("Up, Down to brake, Left, Right.")
        .expect("filled");
    assert_eq!(caps["a"], "Up");
    assert_eq!(caps["b"], "Down");
    assert_eq!(caps["c"], "brake");
    assert_eq!(caps["d"], "Left");
    assert_eq!(caps["e"], "");
    assert_eq!(caps["f"], "Right");
}
