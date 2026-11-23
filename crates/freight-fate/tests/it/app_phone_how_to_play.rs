//! How to play on a phone: every page in gestures by touch, the desktop
//! pages unchanged, and the Controller page only with a controller.

use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{InputEvent, Key};
use freight_fate::states::main_menu::{
    controls_help_page, help_pages, render_help_line, HelpState, HELP_PAGES,
};

/// Plays this test's thread as a phone until dropped.
struct Phone;

impl Phone {
    fn on(app: &mut TestApp) -> Self {
        freight_fate::device::force_phone(true);
        app.ctx.controller.restart_input(true);
        Phone
    }
}

impl Drop for Phone {
    fn drop(&mut self) {
        freight_fate::device::force_phone(false);
    }
}

/// Words a phone player with no keyboard should never hear in How to play.
const KEY_WORDS: &[&str] = &[
    "Enter",
    "Escape",
    "F1",
    "F2",
    "Page Up",
    "Page Down",
    "Control ",
    "Shift ",
    "arrow",
    "Comma",
    "Period",
    "brackets",
    "Press ",
    "press ",
    "keypad",
    " key",
    "Hold a ",
    "hold a swipe",
    "Tap a ",
    "a tap of",
    "braille",
    "Updates",
    "Quit",
    "Discord",
    "computer",
    "from the three-finger tap list",
];

#[test]
fn every_page_by_touch_names_gestures_not_keys() {
    let mut app = TestApp::new();
    let _phone = Phone::on(&mut app);
    let pages = help_pages(&app.ctx);
    assert!(
        !pages.iter().any(|(title, _)| *title == "Controller"),
        "the Controller page shows with no controller"
    );
    for (title, lines) in &pages {
        assert!(!title.contains("keys"), "{title}");
        for line in lines {
            for word in KEY_WORDS {
                assert!(!line.contains(word), "{title}: {word:?} in {line}");
            }
            assert!(!line.contains("{{"), "{title}: unfilled name in {line}");
        }
    }
}

#[test]
fn the_touch_pages_name_the_plain_gesture() {
    let mut app = TestApp::new();
    let _phone = Phone::on(&mut app);
    let text: String = help_pages(&app.ctx)
        .into_iter()
        .flat_map(|(_, lines)| lines)
        .collect::<Vec<_>>()
        .join("\n");
    for line in [
        "With cruise off, flick up to turn it on.",
        "Flick left or right to change one lane.",
        "Tap for your speed, gear, and speed-control mode.",
        "Flick up with three fingers for how far along you are",
        "With cruise on, flick up or flick down to move the cruise speed to the next multiple of five.",
        "Keep driving and follow the turnaround guidance. Flick up with three fingers for the distance left.",
    ] {
        assert!(text.contains(line), "missing: {line}");
    }
}

#[test]
fn a_moved_gesture_is_followed_in_the_touch_pages() {
    use freight_fate::bindings::{Action, TouchCommand};
    use freight_fate::touch::Gesture;
    let mut app = TestApp::new();
    let _phone = Phone::on(&mut app);
    app.ctx
        .bindings
        .set_touch_command(Gesture::Tap, TouchCommand::Action(Action::Fuel));
    let (_, lines) = freight_fate::states::main_menu::help_page(&app.ctx, controls_help_page());
    assert!(
        lines[0].starts_with(
            "Tap with three fingers and open Speed and cruise for Speed for your speed"
        ),
        "{}",
        lines[0]
    );
}

#[test]
fn a_desktop_run_reads_the_desktop_pages() {
    let app = TestApp::new();
    let pages = help_pages(&app.ctx);
    assert_eq!(pages.len(), HELP_PAGES.len());
    for ((title, lines), (desktop_title, desktop_lines)) in pages.iter().zip(HELP_PAGES) {
        assert_eq!(title, desktop_title);
        let expected: Vec<String> = desktop_lines
            .iter()
            .map(|line| render_help_line(&app.ctx, line))
            .collect();
        assert_eq!(lines, &expected, "{title}");
    }
}

#[test]
fn the_page_count_matches_the_pages_listed() {
    let mut app = TestApp::new();
    let _phone = Phone::on(&mut app);
    let count = help_pages(&app.ctx).len();
    assert_eq!(count, HELP_PAGES.len() - 1);
    app.push_state(HelpState::new());
    app.clear_speech();
    // Three-finger flick down is End: the last page listed.
    app.dispatch_to_state(&InputEvent::key(Key::End));
    let said = app.main_lines().join(" ");
    assert!(
        said.contains(&format!("Page {count} of {count}: Common questions.")),
        "{said}"
    );
    app.clear_speech();
    app.dispatch_to_state(&InputEvent::key(Key::Home));
    let said = app.main_lines().join(" ");
    assert!(
        said.contains(&format!("Page 1 of {count}: Main menu.")),
        "{said}"
    );
}

#[test]
fn the_controls_page_by_touch_is_the_driving_gestures_page() {
    let mut app = TestApp::new();
    let _phone = Phone::on(&mut app);
    app.push_state(HelpState::at_page(controls_help_page()));
    let said = app.main_lines().join(" ");
    assert!(said.contains("Driving gestures."), "{said}");
}
