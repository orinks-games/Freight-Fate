//! A career that is over: the lifetime disqualification ends the driving and
//! keeps the data, and only the player's own close-out removes it.

use crate::states_city_support::*;
use ff_core::models::business::owner_operator_eligibility;
use ff_core::models::enforcement::{self, SETBACK_DISQUALIFICATION};
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Menu;
use freight_fate::states::career_setback::CareerSetbackNoticeState;
use freight_fate::states::city::{CityMenuState, CloseOutCareerState};
use freight_fate::states::driving_rest_states::{major_offense_text, suspension_text};
use freight_fate::states::main_menu::MainMenuState;

const DAY: f64 = 24.0;

/// Dale, two major offenses in: disqualified for life.
fn ended_career(app: &mut TestApp) {
    career(app, "Dale", "Buffalo");
    let p = profile_mut(app);
    p.game_hours = 400.0 * DAY;
    p.driving_record.record_major_offense(100.0 * DAY);
    p.driving_record.record_major_offense(390.0 * DAY);
    assert!(p.driving_record.lifetime_disqualified);
}

#[test]
fn test_the_terminal_says_the_career_is_over_and_offers_the_close_out_last() {
    let mut app = TestApp::new();
    ended_career(&mut app);
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);
    assert!(is::<CityMenuState>(&app));

    let greeting = app.main_lines().join(" ");
    assert!(
        greeting.contains("this career cannot drive again"),
        "{greeting}"
    );
    assert!(greeting.contains("Close out this career"), "{greeting}");

    let rows = labels::<CityMenuState>(&app);
    assert_eq!(
        rows.last().map(String::as_str),
        Some("Close out this career")
    );
    assert!(!rows.iter().any(|r| r == "Wait out the CDL suspension"));
}

#[test]
fn test_a_working_career_is_never_offered_the_close_out() {
    let mut app = TestApp::new();
    career(&mut app, "Dale", "Buffalo");
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);
    let rows = labels::<CityMenuState>(&app);
    assert!(
        !rows.iter().any(|r| r == "Close out this career"),
        "{rows:?}"
    );
    assert!(!app.main_lines().join(" ").contains("career is over"));
}

#[test]
fn test_the_disqualification_notice_reads_once_at_the_terminal() {
    let mut app = TestApp::new();
    ended_career(&mut app);
    {
        let record = &mut profile_mut(&mut app).driving_record;
        record.setback_notice_kind = SETBACK_DISQUALIFICATION.to_string();
        record.setback_notice_lines = enforcement::disqualification_notice_lines();
    }
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);
    assert!(is::<CareerSetbackNoticeState>(&app));
    with_state::<CareerSetbackNoticeState, _>(&app, |s, _| {
        assert_eq!(s.title(), "Your driving career is over");
        assert!(s.lines.iter().any(|l| l.contains("Close out this career")));
        assert!(s.lines.iter().any(|l| l.contains("Nothing is taken away")));
    });
    with_state_mut::<CareerSetbackNoticeState, _>(&mut app, Menu::go_back);
    assert!(is::<CityMenuState>(&app));
    assert!(profile(&app).driving_record.setback_notice_lines.is_empty());
}

/// What still works after a suspension or disqualification names only rows a
/// company driver has: the truck dealer is an owner-operator row.
#[test]
fn test_what_still_works_names_no_row_a_company_driver_lacks() {
    let mut app = TestApp::new();
    ended_career(&mut app);
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);
    let rows = labels::<CityMenuState>(&app);
    assert!(!rows.iter().any(|r| r == "Truck dealer"), "{rows:?}");
    let hours = profile(&app).game_hours;
    for text in [
        enforcement::disqualification_notice_lines().join(" "),
        suspension_text(&app.ctx, hours, "suspended"),
        major_offense_text(&app.ctx, enforcement::SUSPENSION_LIFETIME, hours),
    ] {
        assert!(!text.contains("truck dealer"), "{text}");
    }
}

#[test]
fn test_closing_out_removes_the_save_and_leaves_for_the_title_menu() {
    let mut app = TestApp::new();
    ended_career(&mut app);
    let path = profile(&app).save().expect("the save writes");
    assert!(path.exists());
    let city = CityMenuState::new(&app.ctx, false);
    app.push_state(city);

    activate::<CityMenuState>(&mut app, "Close out this career");
    assert!(is::<CloseOutCareerState>(&app));
    let intro = app.main_lines().join(" ");
    assert!(intro.contains("for good"), "{intro}");
    assert!(
        intro.contains("achievements and road journal stay"),
        "{intro}"
    );

    // Saying no keeps everything.
    activate::<CloseOutCareerState>(&mut app, "No, keep this career");
    assert!(is::<CityMenuState>(&app));
    assert!(path.exists());
    assert!(app.ctx.profile.is_some());

    // Saying yes, on a computer with no cloud sign-in, removes the local
    // save, says the cloud was never involved, and lands on the title menu.
    activate::<CityMenuState>(&mut app, "Close out this career");
    with_state_mut::<CloseOutCareerState, _>(&mut app, |s, _| s.threaded = false);
    activate::<CloseOutCareerState>(&mut app, "Yes, close out Dale");
    assert!(is::<MainMenuState>(&app));
    assert!(app.ctx.profile.is_none());
    assert!(!path.exists());
    let said = app.main_lines().join(" ");
    assert!(said.contains("Dale closed out"), "{said}");
    assert!(said.contains("never set up"), "{said}");
}

#[test]
fn test_a_disqualified_driver_cannot_buy_into_owner_operator() {
    let mut app = TestApp::new();
    ended_career(&mut app);
    let (ok, reasons) = owner_operator_eligibility(profile(&app));
    assert!(!ok);
    assert!(
        reasons.iter().any(|r| r.contains("clear CDL")),
        "{reasons:?}"
    );
}
