//! The new-career pickers: Alaska's own row in the home region list, and
//! one select sound for each pick (2.0 review, 2026-11-21).

use crate::states_main_menu_support::*;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Key;
use freight_fate::states::main_menu::{CareerStartState, HomeCityState, HomeTerminalState};

fn select_sounds(log: &freight_fate::app::testing::AudioLog) -> usize {
    log.borrow()
        .played
        .iter()
        .filter(|(key, _, _)| key == "ui/menu_select")
        .count()
}

#[test]
fn test_alaska_has_its_own_home_region_row() {
    let mut app = TestApp::new();
    let picker = HomeTerminalState::new(&app.ctx, "North");
    app.push_state(picker);
    let rows = labels::<HomeTerminalState>(&app);
    assert!(
        rows.iter().any(|row| row.starts_with("Alaska, ")),
        "{rows:#?}"
    );
    select::<HomeTerminalState>(&mut app, "Alaska");
    assert!(is::<HomeCityState>(&app));
    let cities = labels::<HomeCityState>(&app);
    assert!(
        cities.iter().any(|c| c.starts_with("Anchorage")),
        "{cities:#?}"
    );
    assert!(cities.iter().all(|c| c.ends_with("Alaska")), "{cities:#?}");

    // The Pacific Northwest keeps Washington and Oregon, without Alaska.
    key(&mut app, Key::Escape);
    select::<HomeTerminalState>(&mut app, "Pacific Northwest");
    let cities = labels::<HomeCityState>(&app);
    assert!(
        cities.iter().any(|c| c.starts_with("Seattle")),
        "{cities:#?}"
    );
    assert!(!cities.iter().any(|c| c.ends_with("Alaska")), "{cities:#?}");
}

#[test]
fn test_picking_a_home_city_and_a_carrier_plays_one_select_sound_each() {
    let mut app = TestApp::new();
    let picker = HomeTerminalState::new(&app.ctx, "One Click");
    app.push_state(picker);
    let log = app.record_audio();
    key(&mut app, Key::Return); // the default region
    assert!(is::<HomeCityState>(&app));
    assert_eq!(select_sounds(&log), 1, "region");
    key(&mut app, Key::Return); // the default city
    assert!(is::<CareerStartState>(&app));
    assert_eq!(select_sounds(&log), 2, "home city");
    key(&mut app, Key::Return); // the default carrier
    assert!(app.ctx.profile.is_some());
    assert_eq!(select_sounds(&log), 3, "carrier");
}
