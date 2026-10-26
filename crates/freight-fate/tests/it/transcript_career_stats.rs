//! The terminal's Career stats screen: a reviewable menu with rest status
//! (port of `tests/test_career_stats.py`).

use crate::states_main_menu_support::*;
use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::{Key, Menu, SimpleMenuState};
use freight_fate::states::career_stats::CareerStatsState;
use freight_fate::states::city::CityMenuState;

#[test]
fn test_career_stats_is_a_reviewable_menu_with_rest_status() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Stats", "Austin"));
    {
        let p = app.ctx.profile.as_mut().unwrap();
        p.career.deliveries = 4;
        p.career.on_time_deliveries = 3;
        p.career.total_miles = 1234.0;
        p.career.total_earnings = 5678.0;
    }
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);

    select::<CityMenuState>(&mut app, "Career stats");
    assert!(is::<CareerStatsState>(&app));
    let rows = labels::<CareerStatsState>(&app);
    assert!(rows.iter().any(|l| l.starts_with("Level 1 driver")));
    // A level 1 driver holds nothing; the line still exists so the screen
    // always answers "what am I cleared to haul?"
    assert!(rows.iter().any(|l| l == "Endorsements: none yet"));
    // Brandon, 2026-08-22: the hold spoke at the dispatch hand-over and at a
    // level-up that brought no truck, and nowhere the player could go and
    // ASK. A driver in good standing hears what earns the next fleet instead
    // of a hold that is not there (`equipment_status_lines`).
    assert!(
        rows.iter().any(|l| l.starts_with("Truck: ")
            && l.contains("At level 4, dispatch moves you up to the regional fleet.")),
        "{rows:?}"
    );
    assert!(rows
        .iter()
        .any(|l| l == "Deliveries: 4, 75 percent on time"));
    assert!(rows.iter().any(|l| l == "Lifetime miles: 1,234"));
    assert!(rows.iter().any(|l| l == "Lifetime earnings: 5,678 dollars"));
    assert!(rows.iter().any(|l| l == "Rest: fully rested"));
    assert!(rows.iter().any(|l| l.starts_with("Hours:")));
    assert_eq!(rows.last().map(String::as_str), Some("Back"));

    // Enter repeats the current line without leaving the screen.
    key(&mut app, Key::Return);
    assert!(is::<CareerStatsState>(&app));

    // A tired driver hears fatigue instead of "fully rested".
    {
        let p = app.ctx.profile.as_mut().unwrap();
        p.fatigue = 40.0;
        p.hos.drive(120.0);
    }
    with_state_mut::<CareerStatsState, _>(&mut app, |s, ctx| s.refresh(ctx, true));
    let rows = labels::<CareerStatsState>(&app);
    assert!(rows.iter().any(|l| l == "Rest: fatigue 40 percent"));
    assert!(!rows.iter().any(|l| l == "Rest: fully rested"));

    // Credentials are a reviewable record, not a one-time level-up
    // announcement: a driver holding reefer and high-value hears both on
    // the certificates line, and the endorsements line stays honest about
    // holding none.
    {
        let p = app.ctx.profile.as_mut().unwrap();
        p.career
            .purchased_endorsements
            .extend(["refrigerated".to_string(), "high_value".to_string()]);
    }
    with_state_mut::<CareerStatsState, _>(&mut app, |s, ctx| s.refresh(ctx, true));
    let rows = labels::<CareerStatsState>(&app);
    assert!(rows
        .iter()
        .any(|l| l == "Certificates: high-value, refrigerated"));
    assert!(rows.iter().any(|l| l == "Endorsements: none yet"));

    // A check in flight is reviewable too, with the days it has left.
    {
        let p = app.ctx.profile.as_mut().unwrap();
        let ready = p.game_hours + 12.0 * 24.0;
        p.career
            .pending_credentials
            .push(ff_core::models::career::PendingCredential {
                key: "hazmat".to_string(),
                ready_at_h: ready,
            });
    }
    with_state_mut::<CareerStatsState, _>(&mut app, |s, ctx| s.refresh(ctx, true));
    let rows = labels::<CareerStatsState>(&app);
    assert!(rows
        .iter()
        .any(|l| l == "hazmat endorsement background check in progress, about 12 days left"));

    key(&mut app, Key::Escape);
    assert!(is::<CityMenuState>(&app));
}

#[test]
fn test_career_stats_count_and_list_the_places_delivered_to() {
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Places", "Austin"));
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);

    // A new career has been nowhere yet, and the list says so.
    select::<CityMenuState>(&mut app, "Career stats");
    let rows = labels::<CareerStatsState>(&app);
    assert!(rows.iter().any(|l| l == "States visited: 0"), "{rows:?}");
    assert!(rows.iter().any(|l| l == "Cities visited: 0"), "{rows:?}");
    select::<CareerStatsState>(&mut app, "Places visited");
    assert_eq!(
        labels::<SimpleMenuState>(&app),
        ["No deliveries yet.", "Back"]
    );
    key(&mut app, Key::Escape);

    // The save keeps city keys; a key the world no longer has is left out
    // rather than counted or read aloud.
    {
        let p = app.ctx.profile.as_mut().unwrap();
        p.achievement_stats.insert(
            "cities_delivered".to_string(),
            serde_json::json!([
                "chicago_il_us",
                "denver_co_us",
                "rockford_il_us",
                "gone_xx_us"
            ]),
        );
    }
    with_state_mut::<CareerStatsState, _>(&mut app, |s, ctx| s.refresh(ctx, true));
    let rows = labels::<CareerStatsState>(&app);
    assert!(rows.iter().any(|l| l == "States visited: 2"), "{rows:?}");
    assert!(rows.iter().any(|l| l == "Cities visited: 3"), "{rows:?}");
    select::<CareerStatsState>(&mut app, "Places visited");
    assert_eq!(
        labels::<SimpleMenuState>(&app),
        ["Colorado: Denver", "Illinois: Chicago, Rockford", "Back"]
    );
}

#[test]
fn test_career_stats_lists_the_co_drivers_only_once_you_have_teamed() {
    use ff_core::models::business_constants::{COMPANY_DRIVER, LEASED_OWNER_OPERATOR};
    use ff_core::models::co_driver::record_team_load;
    let mut app = TestApp::new();
    app.ctx.profile = Some(Profile::named_in("Team", "Austin"));
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);

    // A career that has never teamed has no story to offer.
    select::<CityMenuState>(&mut app, "Career stats");
    let rows = labels::<CareerStatsState>(&app);
    assert!(!rows.iter().any(|l| l == "Co-drivers"), "{rows:?}");

    {
        let stats = &mut app.ctx.profile.as_mut().unwrap().achievement_stats;
        record_team_load(stats, "dan", COMPANY_DRIVER);
        record_team_load(stats, "june", LEASED_OWNER_OPERATOR);
        record_team_load(stats, "june", LEASED_OWNER_OPERATOR);
    }
    app.ctx.profile.as_mut().unwrap().business_status = LEASED_OWNER_OPERATOR.to_string();
    with_state_mut::<CareerStatsState, _>(&mut app, |s, ctx| s.refresh(ctx, true));
    select::<CareerStatsState>(&mut app, "Co-drivers");
    let lines = labels::<SimpleMenuState>(&app);
    assert_eq!(lines.len(), 3, "{lines:?}");
    assert!(
        lines[0].starts_with("Dan, 1 team load. The most careful driver in your training class."),
        "{lines:?}"
    );
    assert!(
        lines[1].starts_with("June, your hire, 2 team loads. Grew up waiting tables"),
        "{lines:?}"
    );
    assert_eq!(lines[2], "Back");

    // Back on a carrier's payroll, June is no longer their hire.
    key(&mut app, Key::Escape);
    app.ctx.profile.as_mut().unwrap().business_status = COMPANY_DRIVER.to_string();
    select::<CareerStatsState>(&mut app, "Co-drivers");
    let lines = labels::<SimpleMenuState>(&app);
    assert!(lines[1].starts_with("June, 2 team loads."), "{lines:?}");
}
