use super::{Catalog, Translator};

/// The shipped Spanish catalog, as a player gets it.
fn shipped_spanish() -> Translator {
    let po = include_str!("../../../../../lang/es.po");
    Translator::new(Catalog::from_po("es", po).expect("lang/es.po parses"))
}

/// The shipped Simplified Chinese catalog, as a player gets it.
fn shipped_chinese() -> Translator {
    let po = include_str!("../../../../../lang/zh_CN.po");
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
        tr.translate("California, 44 cities. 3 of 16."),
        "California，44个城市。第 3 项，共 16 项。"
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
            "Needs an extra rest on this shift's hours. Hours of service: 11 hours of driving left, 5.2 hours left in the duty window. Split rest started: 7 more hours in the sleeper finish it. Enter again to accept."
        ),
        "按本次轮班的工时计算，需要额外休息。驾驶工时：剩余驾驶时间11小时，14小时当班时段剩余5.2小时。卧铺分段休息尚未配对：还需在卧铺休息7小时。再次按Enter接受。"
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
            "On I-10 toward Indio, California. Next stop in 1 mile: travel center off exit 239. Automatic transmission."
        ),
        "在 I-10 上朝向 Indio, California。前方1英里是下一个停靠点：239号出口处的卡车服务区。自动变速箱。"
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
                 14 hours left in the duty window";
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
            "Page Down or apostrophe tunes to the next station, Page Up or semicolon to the \
             previous one. Control with a tuning key jumps a category. Shift with a tuning \
             key changes the radio volume in 10 percent steps, with the radio on or off. \
             S posted speed limit. \
             G the grade under the wheels, whether the truck is holding it, and the next \
             grade ahead. Tab status menu."
        ),
        "Page Down o el apóstrofo sintonizan la emisora siguiente, Page Up o el punto y \
         coma la anterior. Control con una tecla de sintonía salta de categoría. Mayúsculas \
         con una tecla de sintonía cambia el volumen de la radio de 10 en 10 por ciento, con \
         la radio encendida o apagada. S el límite de velocidad señalizado. G la pendiente bajo las ruedas, si el \
         camión la está aguantando, y la próxima pendiente. Tab el menú de estado."
    );
}

#[test]
fn shipped_spanish_translates_a_carrier_terminal_line() {
    // What kind of place it is belongs to the name and reads in lower case
    // ("... Chicago terminal"), which the name guards took for a sentence:
    // the whole line stayed English in every language.
    let tr = shipped_spanish();
    assert_eq!(
        tr.translate("Northstar Freight Lines Chicago terminal in Chicago. 5,000 dollars."),
        "Northstar Freight Lines Chicago terminal en Chicago. 5.000 dólares."
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
            "On I-94 toward Kenosha, Wisconsin. Next stop in 1 mile: travel center off exit 50B."
        ),
        "En I-94 hacia Kenosha, Wisconsin. Próxima parada en 1 milla: área de servicio en la \
         salida 50B."
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
        ("Dispatch routed you to Kenosha, Wisconsin.", "Despacho te envió a Kenosha, Wisconsin."),
        ("Market watch: automotive tight, farm inputs tight, lumber paper loose, steel loose.", "Vistazo al mercado: automoción al alza, insumos agrícolas al alza, madera y papel a la baja, acero a la baja."),
        ("16 tons of general freight from Chicago, Illinois to Fargo, North Dakota. 62 miles, deadline 3 hours. Pays 332 dollars.", "16 toneladas de carga general de Chicago, Illinois a Fargo, North Dakota. 62 millas, plazo 3 horas. Paga 332 dólares."),
        ("16 tons of general freight from Chicago, Illinois to Kenosha, Wisconsin. 62 miles, deadline 12 hours with a 10-hour rest. Pays 332 dollars.", "16 toneladas de carga general de Chicago, Illinois a Kenosha, Wisconsin. 62 millas, plazo 12 horas con un descanso de 10 horas. Paga 332 dólares."),
        (
            "Accepted. 1.1 miles empty to Chicago Parcel Hub.",
            "Aceptada. 1,1 millas en vacío hasta Chicago Parcel Hub.",
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
        ("Great Lakes, 29 cities. 8 of 16.", "Great Lakes, 29 ciudades. 8 de 16."),
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
fn shipped_spanish_says_a_whole_dispatch_board_row() {
    // The board's announcement is a frame of holes side by side, and a cut
    // at a comma inside "Kenosha, Wisconsin" left the rest of the row in
    // English: the deadline and the pay.
    let tr = shipped_spanish();
    let line = "Dispatch board. Assigned load: 16 tons of general freight from Chicago \
                Parcel Hub in Chicago, Illinois to Amazon Fulfillment Center KRB1 in Kenosha, \
                Wisconsin. 62 miles, deadline 2 hours. Pays 328 dollars, about 239 to you. \
                1 of 4.";
    let spanish = tr.translate(line);
    for english in ["deadline", "Pays", "to you", "miles", " in "] {
        assert!(!spanish.contains(english), "{english}: {spanish}");
    }
}

/// The shipped Polish catalog, as a player gets it.
fn shipped_polish() -> Translator {
    let po = include_str!("../../../../../lang/pl.po");
    Translator::new(Catalog::from_po("pl", po).expect("lang/pl.po parses"))
}

#[test]
fn shipped_polish_says_a_numbered_job_whole() {
    // "Job 2 of 7: " leads into the load sentence; the clause split broke
    // that sentence at the commas inside its place names, so the whole
    // sentence stayed English (issue 549).
    let tr = shipped_polish();
    let out = tr.translate(
        "Job 2 of 7: 19 tons of general freight from Pendleton, Oregon to Boise, Idaho. 364 miles, \
         deadline 6 hours. Pays 1,204 dollars.",
    );
    assert!(out.starts_with("Zlecenie 2 z 7: "), "{out}");
    assert!(!out.contains(" tons of "), "{out}");
    let out = tr.translate(
        "Recommended dispatch, senior company lane: Job 1 of 7: 13 tons of refrigerated goods from \
         Pendleton, Oregon to Spokane, Washington. 332 miles, deadline 6 hours. Pays 1,090 \
         dollars.",
    );
    assert!(out.contains("Zlecenie 1 z 7: "), "{out}");
    assert!(!out.contains(" tons of "), "{out}");
}

#[test]
fn shipped_polish_says_the_pay_after_costs_as_a_polish_number() {
    // "{take_home}" reads as a home city by its name, so "about 6,576 after
    // costs" kept the English comma: six point five, to a Polish or German
    // ear, beside a correctly written "7 323" for the gross.
    let tr = shipped_polish();
    let out = tr.translate("Pays 7,323 dollars, about 6,576 after costs.");
    assert!(out.contains("7\u{a0}323"), "{out}");
    assert!(out.contains("6\u{a0}576"), "{out}");
    assert!(!out.contains("6,576"), "{out}");
    let po = include_str!("../../../../../lang/de.po");
    let de = Translator::new(Catalog::from_po("de", po).expect("lang/de.po parses"));
    let out = de.translate("Pays 7,323 dollars, about 6,576 after costs.");
    assert!(out.contains("7.323") && out.contains("6.576"), "{out}");
}

fn shipped_vietnamese() -> Translator {
    let po = include_str!("../../../../../lang/vi.po");
    Translator::new(Catalog::from_po("vi", po).expect("lang/vi.po parses"))
}

#[test]
fn shipped_vietnamese_translates_reported_turn_and_limit_issues() {
    let tr = shipped_vietnamese();

    // 1. Resume line splits cleanly and translates engine prompt
    let resume = "Resuming pickup drive: 19 tons of shipping containers to Visalia Intermodal Ramp in Visalia, California. 3.3 kilometers remaining of 3.3 to pickup at Visalia Intermodal Ramp in Visalia, California. 0.0 of 2 hours. 5:01 PM. Automatic transmission. Simulated weather: fog, 15 C, visibility 0.482803 kilometers. Press E to start the engine.";
    let s2 = tr.translate(resume);
    assert!(s2.contains("Nhấn E để đề nổ máy."), "{s2}");
    assert!(s2.contains("Tiếp tục chuyến chạy đi nhận hàng"), "{s2}");
    assert!(s2.contains("Hộp số tự động"), "{s2}");

    // 2. Turn navigation cues with distance tail
    let turn = "Turn right onto South Bridge Street, then 1 kilometer on it. Advise 18 km/h.";
    let s3 = tr.translate(turn);
    assert!(
        s3.contains("Rẽ phải vào South Bridge Street, sau đó chạy 1 kilômét trên tuyến đó."),
        "{s3}"
    );
    assert!(s3.contains("Khuyến nghị 18 km/h."), "{s3}");

    // 3. Speed limit variations
    let spd = tr.translate("Speed limit 40 km/h.");
    assert_eq!(spd, "Giới hạn tốc độ 40 km/h.");
    let reduced = tr.translate("Speed limit reduced to 40 km/h.");
    assert_eq!(reduced, "Giới hạn tốc độ giảm xuống 40 km/h.");
    let raised = tr.translate("Speed limit raised to 55 km/h.");
    assert_eq!(raised, "Giới hạn tốc độ tăng lên 55 km/h.");
    let approaching =
        tr.translate("Speed limit reduced to 40 km/h approaching Visalia for 1 kilometer.");
    assert_eq!(
        approaching,
        "Giới hạn tốc độ giảm xuống 40 km/h khi vào gần Visalia trong 1 kilômét."
    );
    let weigh = tr.translate("Speed limit reduced to 40 km/h for the weigh station.");
    assert_eq!(weigh, "Giới hạn tốc độ giảm xuống 40 km/h trước trạm cân.");
    let grade = tr.translate("Speed limit reduced to 40 km/h for the downhill grade.");
    assert_eq!(
        grade,
        "Giới hạn tốc độ giảm xuống 40 km/h trước đoạn dốc xuống."
    );
}

#[test]
fn shipped_russian_keeps_clock_lines_off_the_backup_summary() {
    let po = include_str!("../../../../../lang/ru.po");
    let tr = Translator::new(Catalog::from_po("ru", po).expect("lang/ru.po parses"));
    assert_eq!(tr.translate("It is 9:00."), "Сейчас 9:00.");
    assert_eq!(tr.translate("It is night."), "Сейчас ночь.");
}

/// Every shipped catalog, as a player gets it.
fn shipped_catalogs() -> Vec<(&'static str, Translator)> {
    [
        ("de", include_str!("../../../../../lang/de.po")),
        ("es", include_str!("../../../../../lang/es.po")),
        ("fr", include_str!("../../../../../lang/fr.po")),
        ("it", include_str!("../../../../../lang/it.po")),
        ("ja", include_str!("../../../../../lang/ja.po")),
        ("pl", include_str!("../../../../../lang/pl.po")),
        ("pt_BR", include_str!("../../../../../lang/pt_BR.po")),
        ("ru", include_str!("../../../../../lang/ru.po")),
        ("tr", include_str!("../../../../../lang/tr.po")),
        ("vi", include_str!("../../../../../lang/vi.po")),
        ("zh_CN", include_str!("../../../../../lang/zh_CN.po")),
    ]
    .into_iter()
    .map(|(code, po)| {
        let catalog = Catalog::from_po(code, po).expect("the shipped catalog parses");
        (code, Translator::new(catalog))
    })
    .collect()
}

/// The line a resumed drive opens with ends in the truck's state: cruise,
/// the engine, the parking brake. Every language says all of it. As holes
/// side by side at the end of the line, a translated line split them
/// mid-sentence, and "Press E to start the engine." stayed in English in
/// ten languages (found before 1.9.7; Vietnamese reported it in PR 625).
#[test]
fn every_shipped_language_says_the_whole_resume_line() {
    let long = "Resuming loaded delivery: 19 tons of shipping containers to Visalia Intermodal \
                Ramp in Visalia, California. 12 miles remaining of 40. 0.5 of 6 hours. 5:01 PM. \
                Automatic transmission. Simulated weather: fog, 59 F, visibility 0.3 miles.";
    let short = "Resuming loaded delivery: Visalia Intermodal Ramp in Visalia, California. 12 \
                 miles remaining of 40. 0.5 of 6 hours used. 5:01 PM. automatic. Simulated \
                 weather: fog, 59 F, visibility 0.3 miles.";
    let lines = [
        format!("{long} Press E to start the engine."),
        format!("{long} Press E to start the engine. At air ready, P releases the parking brake."),
        format!("{long} Engine off."),
        format!("{long} Engine idling."),
        format!("{long} Engine off. At air ready, P releases the parking brake."),
        format!("{long} Cruise paused. C cancels it. Engine idling."),
        format!("{long} Cruise paused. Engine off."),
        format!("{short} Engine off, air 90 psi, parking brake set."),
        format!("{short} Engine idling, air ready, parking brake released."),
        format!("{short} Cruise paused. Engine idling, air ready, parking brake released."),
        format!("{short} Cruise paused. C cancels it. Engine off, air 90 psi, parking brake set."),
    ];
    // Phrases of the English line that no translation contains.
    let english = [
        "Resuming",
        "remaining",
        "hours",
        "Automatic transmission",
        " automatic.",
        "Simulated",
        "Press E",
        "engine",
        "Engine",
        "air ready",
        "brake",
        "Cruise paused",
        "cancels",
    ];
    for (code, tr) in shipped_catalogs() {
        for line in &lines {
            let said = tr.translate(line);
            for word in english {
                assert!(!said.contains(word), "{code} kept \"{word}\": {said}");
            }
        }
    }
}
#[test]
fn shipped_catalogs_keep_loose_frames_off_whole_lines() {
    // Loose frames like "{chain_repair_links} left." and "{city} Travel
    // Center" once took lines they were never written for: a left curve call
    // said "Sharp remaining" with no direction, a brand stop became a city,
    // and a carrier name was cut in two. Those entries are marked "leave
    // empty" in every language; these lines pin what they broke.
    for (code, tr) in shipped_catalogs() {
        let curve = tr.translate("Sharp left, half a mile. Advise 35 miles per hour.");
        assert!(
            curve.starts_with("Sharp left, half a mile."),
            "{code} lost the curve direction: {curve}"
        );
        let start = tr.translate("Truck starts at Northstar Freight Lines Chicago terminal.");
        assert!(
            start.contains("Northstar Freight Lines Chicago terminal"),
            "{code} cut the terminal name apart: {start}"
        );
        let fuel = tr.translate("Fuel low: 15 percent. Next fuel: Pilot Travel Center, 23 miles.");
        assert!(
            fuel.contains("Pilot Travel Center"),
            "{code} renamed the truck stop: {fuel}"
        );
        let deer = tr.translate("Brake now! A deer crossing the road.");
        assert!(
            !deer.contains("A deer"),
            "{code} left the hazard half English: {deer}"
        );
    }
}
