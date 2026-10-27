//! Port of `tests/test_controls_reference.py`: the in-game controls
//! reference, reachable from the pause menu, opens to keys. The pause-menu
//! tests still read a real drive; listed here, ignored, so the suites diff
//! by name.

use crate::states_driving_menus_support::*;
use ff_core::sim::hos;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Menu;
use freight_fate::states::driving_core::profile_mut_of;
use freight_fate::states::driving_pause_states::{mechanic_label, PauseMenuState};
use freight_fate::states::main_menu::{controls_help_page, help_pages, HelpState, HELP_PAGES};

#[test]
fn test_controls_help_page_points_at_the_driving_keys() {
    let app = TestApp::new();
    let idx = controls_help_page();
    let (title, lines) = help_pages(&app.ctx).swap_remove(idx);
    assert_eq!(title, "Driving information keys");
    // The new keys are documented there.
    let joined = lines.join(" ");
    assert!(joined.contains("Space speaks your speed"));
    assert!(joined.contains("speed-control mode"));
    assert!(joined.contains("RPM with a manual transmission"));
    assert!(joined.contains("S speaks the posted speed limit"));
    assert!(joined.contains("R speaks how far along you are"));
    assert!(joined.contains("A repeats the last driving announcement"));
    // The CB repeat is documented where players go looking for keys, not
    // only in the F1 layout (issue 156).
    assert!(joined.contains("Alt C repeats the last CB chatter"));
    // U stopped being a recital of everything ahead (2026-08-15): the exit
    // cue, the traffic-pressure advisory and two of the three bends were
    // already other keys' answers, so the help now promises only the road
    // nothing else covers. Pinned on the promise, not the old phrasing.
    assert!(joined.contains("U speaks the road ahead that no other key answers"));
    // And U must never advertise police activity again (owner ruling): the
    // check is scoped to U's own line, because a sibling line legitimately
    // explains what the hours-of-service keys do with enforcement off.
    let u_line = lines
        .iter()
        .find(|line| line.starts_with("U speaks"))
        .unwrap()
        .to_lowercase();
    let _ = HELP_PAGES;
    for word in ["patrol", "police", "bear"] {
        assert!(!u_line.contains(word));
    }
    assert!(lines
        .iter()
        .any(|line| line.contains('X') && line.to_lowercase().contains("signal")));
    assert!(lines
        .iter()
        .all(|line| !line.contains("X takes the next announced exit")));
    assert!(joined.contains("Left or Right Control stops the driving event voice"));
}

#[test]
fn test_help_pages_explain_t_roadside_sleep_and_poi_priority() {
    let app = TestApp::new();
    let joined = help_pages(&app.ctx)
        .into_iter()
        .flat_map(|(_title, lines)| lines)
        .collect::<Vec<String>>()
        .join(" ");
    assert!(joined.contains("T opens the emergency shoulder-sleep warning"));
    assert!(joined.contains("nearby route points always take priority"));
    assert!(joined.contains("T or the pause menu offers emergency shoulder sleep"));
    assert!(joined.contains(
        "right bumper plus D-pad down opens route-stop actions or emergency shoulder sleep"
    ));
}

#[test]
fn test_help_state_opens_to_a_chosen_page() {
    let page = controls_help_page();
    assert_eq!(HelpState::at_page(page).page, page);
    // Out-of-range requests clamp instead of crashing.
    assert!(HelpState::at_page(9999).page < HELP_PAGES.len());
}

/// The rows of the pause menu over a real drive, as spoken.
fn pause_labels(app: &mut TestApp, drive: &freight_fate::app::SharedState) -> Vec<String> {
    let mut pause = PauseMenuState::with_drive(drive_ref(drive));
    pause
        .build_items(&mut app.ctx)
        .iter()
        .map(|item| item.text(&pause, &app.ctx))
        .collect()
}

#[test]
fn test_pause_menu_offers_controls_and_help() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    assert!(pause_labels(&mut app, &drive).contains(&"Controls and help".to_string()));
}

#[test]
fn test_pause_menu_emergency_shoulder_sleep_sits_between_mechanic_and_settings() {
    // build_items() inserts this item at a hardcoded index; pin its position
    // so the next inserted item shifts this test instead of silently
    // misplacing the item (it happened once already, index 4 -> 5).
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);
    // Python replaced `emergency_shoulder_sleep_reason` with a lambda. There
    // is no seam here, so the real condition is arranged instead: stopped,
    // nowhere near a route point, and too tired to keep going.
    with_drive(&drive, |d| {
        d.trip.truck.velocity_mps = 0.0;
        d.trip.stops.clear();
    });
    profile_mut_of(&mut app.ctx).fatigue = hos::FATIGUE_SEVERE;

    let labels = pause_labels(&mut app, &drive);

    let idx = labels
        .iter()
        .position(|l| l == "Emergency shoulder sleep")
        .unwrap_or_else(|| panic!("{labels:?}"));
    let mechanic = with_drive(&drive, |d| mechanic_label(d));
    assert_eq!(labels[idx - 1], mechanic, "{labels:?}");
    assert_eq!(labels[idx + 1], "Settings", "{labels:?}");
}
