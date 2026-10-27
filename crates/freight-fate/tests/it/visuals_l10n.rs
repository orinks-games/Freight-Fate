//! The visual display draws its own labels (the dashboard's telltales, the
//! road signs, the menu panel's hints) in the player's language, through
//! the same catalogs the speech uses.

use std::sync::Arc;

use ff_core::l10n::{Catalog, Translator};
use ff_core::message_log::MessageLog;
use freight_fate::playtest::harness::{PlaytestHarness, StartDelivery};
use freight_fate::visuals::scene_drive::SignKind;
use freight_fate::visuals::text::Text;
use freight_fate::visuals::{Panel, Scene, Screen, Visuals};

fn catalog(code: &str, po: &str) -> Arc<Translator> {
    Arc::new(Translator::new(
        Catalog::from_po(code, po).expect("the catalog parses"),
    ))
}

fn spanish() -> Arc<Translator> {
    catalog("es", include_str!("../../../../lang/es.po"))
}

fn text_in(translator: &Arc<Translator>) -> Text {
    let mut text = Text::new();
    text.set_translator(translator);
    text
}

#[test]
fn the_dashboard_and_sign_labels_are_drawn_in_spanish() {
    let text = text_in(&spanish());
    for (english, spanish) in [
        ("SPEED", "VEL."),
        ("LIMIT", "MÁXIMA"),
        ("PULL OVER", "APÁRTATE"),
        ("PARK BRAKE", "FRENO ESTAC."),
        ("CRUISE 55", "CRUCERO 55"),
        ("JAKE 2", "FRENO MOTOR 2"),
        ("TRAFFIC LEFT", "TRÁFICO A LA IZQUIERDA"),
        ("FUEL", "COMB."),
        ("40% damage", "40% daño"),
        ("1450 rpm", "1450 rpm"),
        ("WEIGH STATION", "BÁSCULA"),
        ("1/2 MILE", "1/2 MILLA"),
        ("ROAD WORK", "DE OBRAS"),
        ("WELCOME TO", "BIENVENIDO A"),
        ("3 of 9", "3 de 9"),
        ("Escape to go back", "Escape para volver"),
    ] {
        assert_eq!(text.tr_drawn(english), spanish, "{english}");
    }
}

#[test]
fn english_draws_the_labels_unchanged() {
    let text = Text::new();
    assert_eq!(text.tr_drawn("CRUISE 55"), "CRUISE 55");
    assert_eq!(text.tr_drawn("SPEED"), "SPEED");
}

#[test]
fn every_shipped_catalog_draws_the_road_work_sign_in_its_language() {
    let catalogs = [
        ("de", include_str!("../../../../lang/de.po")),
        ("es", include_str!("../../../../lang/es.po")),
        ("fr", include_str!("../../../../lang/fr.po")),
        ("it", include_str!("../../../../lang/it.po")),
        ("ja", include_str!("../../../../lang/ja.po")),
        ("pl", include_str!("../../../../lang/pl.po")),
        ("pt_BR", include_str!("../../../../lang/pt_BR.po")),
        ("ru", include_str!("../../../../lang/ru.po")),
        ("zh_CN", include_str!("../../../../lang/zh_CN.po")),
    ];
    for (code, po) in catalogs {
        let text = text_in(&catalog(code, po));
        for label in [
            "ROAD WORK",
            "WEIGH STATION",
            "TRAFFIC BOTH SIDES",
            "CHECK ENGINE",
        ] {
            assert_ne!(text.tr_drawn(label), label, "{code}: {label}");
        }
    }
}

#[test]
fn every_shipped_catalog_names_the_curve_in_its_own_words() {
    let catalogs = [
        ("de", include_str!("../../../../lang/de.po")),
        ("es", include_str!("../../../../lang/es.po")),
        ("fr", include_str!("../../../../lang/fr.po")),
        ("it", include_str!("../../../../lang/it.po")),
        ("ja", include_str!("../../../../lang/ja.po")),
        ("pl", include_str!("../../../../lang/pl.po")),
        ("pt_BR", include_str!("../../../../lang/pt_BR.po")),
        ("ru", include_str!("../../../../lang/ru.po")),
        ("zh_CN", include_str!("../../../../lang/zh_CN.po")),
    ];
    for (code, po) in catalogs {
        let text = text_in(&catalog(code, po));
        let kmh = text.tr_drawn("km/h");
        for severity in ["hairpin", "sharp", "moderate", "gentle"] {
            let drawn = text.tr_drawn(&format!("35 mph {severity}"));
            assert!(drawn.contains("35"), "{code}: {drawn}");
            assert!(!drawn.contains(severity), "{code}: {drawn}");
            // A metric driver reads the same bend in km/h.
            let drawn = text.tr_drawn(&format!("56 km/h {severity}"));
            assert!(drawn.contains("56"), "{code}: {drawn}");
            assert!(drawn.contains(&kmh), "{code}: {drawn}");
            assert!(!drawn.contains(severity), "{code}: {drawn}");
        }
    }
    let text = text_in(&spanish());
    assert_eq!(text.tr_drawn("35 mph sharp"), "35 mph cerrada");
    assert_eq!(text.tr_drawn("20 mph hairpin"), "20 mph horquilla");
    assert_eq!(text.tr_drawn("56 km/h sharp"), "56 km/h cerrada");
}

#[test]
fn spanish_speaks_the_middle_lane_as_before_and_draws_it_central() {
    let es = spanish();
    // The spoken lines fill the shared lane name into "carril de la ...".
    assert_eq!(
        es.translate("Changing to the middle lane for the exit."),
        "Cambiando al carril de la mitad para la salida."
    );
    assert_eq!(
        es.translate("The middle lane is closed."),
        "El carril de la mitad está cerrado."
    );
    // The display's lane line has its own entry for the middle lane.
    let text = text_in(&es);
    assert_eq!(
        text.tr_drawn("Middle lane  ·  lane keeping full"),
        "Carril central  ·  mant. de carril completo"
    );
    // Every lane has a whole line of its own, never the spoken lane name.
    for (english, spanish) in [
        (
            "Left lane  ·  lane keeping off",
            "Carril izquierdo  ·  mant. de carril desactivado",
        ),
        (
            "Right lane  ·  lane keeping partial",
            "Carril derecho  ·  mant. de carril parcial",
        ),
        (
            "Single lane  ·  lane keeping full",
            "Carril único  ·  mant. de carril completo",
        ),
    ] {
        assert_eq!(text.tr_drawn(english), spanish);
    }
}

#[test]
fn a_spanish_drive_captures_its_signs_and_distance_in_spanish() {
    let mut h = PlaytestHarness::new();
    h.start_delivery(StartDelivery::named("Visual Display"));
    h.app.ctx.set_language("es");
    h.with_drive(|d, _| {
        d.departure_checked = true;
        d.trip.position_mi = 24.0;
    });
    h.drive_frames(2);
    let scene = Scene::capture(&h.app.ctx);
    let Screen::Drive { view, .. } = &scene.screen else {
        panic!("at the wheel, the road view shows");
    };
    assert!(
        view.hud.remaining_text.starts_with("Quedan"),
        "{}",
        view.hud.remaining_text
    );
    for sign in &view.signs {
        if let SignKind::Service { lines } = &sign.kind {
            assert!(
                !lines.iter().any(|l| l == "1 MILE" || l == "REST AREA"),
                "{lines:?}"
            );
        }
    }
}

#[test]
fn a_language_change_draws_the_same_menu_again() {
    let mut visuals = Visuals::new();
    let log = MessageLog::new();
    let panel = Panel::from_lines(&[
        "Freight Fate".to_string(),
        String::new(),
        "> New career".to_string(),
        "  Quit".to_string(),
    ])
    .expect("a menu");
    let scene = Scene {
        screen: Screen::Panel(panel),
        achievement: None,
    };
    assert!(visuals.build(scene.clone(), &log, (900, 640), false));
    assert!(!visuals.build(scene.clone(), &log, (900, 640), false));
    let es = spanish();
    visuals.set_translator(&es);
    assert!(
        visuals.build(scene.clone(), &log, (900, 640), false),
        "the hint line is now Spanish"
    );
    visuals.set_translator(&es);
    assert!(!visuals.build(scene, &log, (900, 640), false));
}
