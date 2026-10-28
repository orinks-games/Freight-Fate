//! The Sort rows of the main-menu achievements screens: one order for the
//! categories screen, saved in settings, which each category follows unless
//! the player picks another order inside it.

use ff_core::achievements::{achievement_by_id, achievements_in_category, categories};
use ff_core::models::profile::Profile;
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Key;
use freight_fate::states::main_menu::{AchievementCategoryState, AchievementsState};

use crate::states_main_menu_support::*;

/// A career holding one badge, "Signed, Sealed, Hauled", in Out on the Road.
fn categories_screen() -> TestApp {
    let mut app = TestApp::new();
    let mut profile = Profile::named("Sorted Driver");
    profile.achievements.push("first_delivery".to_string());
    app.push_state(AchievementsState::new(profile));
    app
}

fn road_title() -> &'static str {
    let category = achievement_by_id("first_delivery").unwrap().category;
    categories()
        .iter()
        .find(|c| c.id == category)
        .unwrap()
        .title
}

#[test]
fn earned_first_is_the_default_and_leads_with_the_earned_category() {
    let app = categories_screen();
    let rows = labels::<AchievementsState>(&app);
    // Sort is the first row, the sorted categories follow, and Summary
    // sits after them above Back.
    assert_eq!(rows[0], "Sort: Earned first");
    assert_eq!(
        current_label::<AchievementsState>(&app),
        "Sort: Earned first"
    );
    assert!(
        rows[1].starts_with(&format!("{}. 1 of", road_title())),
        "{rows:?}"
    );
    assert!(rows[rows.len() - 2].starts_with("Summary:"), "{rows:?}");
    assert_eq!(rows[rows.len() - 1], "Back");
    assert_eq!(app.ctx.settings.achievement_sort, "earned");
}

#[test]
fn left_and_right_step_the_order_say_it_and_save_it() {
    let mut app = categories_screen();
    move_to::<AchievementsState>(&mut app, "Sort:");

    app.clear_speech();
    key(&mut app, Key::Right);
    assert_eq!(app.main_lines().last().unwrap(), "Locked first.");
    assert_eq!(
        current_label::<AchievementsState>(&app),
        "Sort: Locked first"
    );
    assert_eq!(app.ctx.settings.achievement_sort, "locked");
    // The most left to earn leads.
    let rows = labels::<AchievementsState>(&app);
    let left: Vec<usize> = rows[1..1 + categories().len()]
        .iter()
        .map(|row| {
            let (_, counts) = row.rsplit_once(". ").unwrap();
            let (done, total) = counts.split_once(" of ").unwrap();
            total.parse::<usize>().unwrap() - done.parse::<usize>().unwrap()
        })
        .collect();
    assert!(left.windows(2).all(|w| w[0] >= w[1]), "{rows:?}");

    key(&mut app, Key::Right);
    assert_eq!(current_label::<AchievementsState>(&app), "Sort: A to Z");
    let titles: Vec<String> = labels::<AchievementsState>(&app)[1..1 + categories().len()]
        .iter()
        .map(|r| r.to_lowercase())
        .collect();
    let mut sorted = titles.clone();
    sorted.sort();
    assert_eq!(titles, sorted);

    // Enter steps forward too; Left steps back and wraps.
    key(&mut app, Key::Return);
    assert_eq!(
        current_label::<AchievementsState>(&app),
        "Sort: Original order"
    );
    let rows = labels::<AchievementsState>(&app);
    assert!(rows[1].starts_with(categories()[0].title), "{rows:?}");
    key(&mut app, Key::Right);
    key(&mut app, Key::Left);
    assert_eq!(
        current_label::<AchievementsState>(&app),
        "Sort: Original order"
    );

    // Left and Right on any other row change nothing.
    key(&mut app, Key::Up);
    key(&mut app, Key::Right);
    assert_eq!(app.ctx.settings.achievement_sort, "original");
}

#[test]
fn a_category_follows_the_screen_until_it_picks_its_own_order() {
    let mut app = categories_screen();
    let road = achievements_in_category(achievement_by_id("first_delivery").unwrap().category);

    select::<AchievementsState>(&mut app, road_title());
    let rows = labels::<AchievementCategoryState>(&app);
    assert_eq!(rows[0], "Sort: Earned first");
    assert_eq!(
        current_label::<AchievementCategoryState>(&app),
        "Sort: Earned first"
    );
    assert!(rows[1].starts_with("Earned: Signed"), "{rows:?}");
    assert_eq!(rows[road.len() + 1], "Back to the categories");

    // Locked first, for this category only.
    move_to::<AchievementCategoryState>(&mut app, "Sort:");
    app.clear_speech();
    key(&mut app, Key::Right);
    assert_eq!(app.main_lines().last().unwrap(), "Locked first.");
    let rows = labels::<AchievementCategoryState>(&app);
    assert!(rows[road.len()].starts_with("Earned: Signed"), "{rows:?}");
    assert_eq!(app.ctx.settings.achievement_sort, "earned");

    // Leaving and coming back keeps the category's pick.
    key(&mut app, Key::Escape);
    assert!(is::<AchievementsState>(&app));
    select::<AchievementsState>(&mut app, road_title());
    assert_eq!(
        labels::<AchievementCategoryState>(&app)[0],
        "Sort: Locked first"
    );

    // Another category still follows the screen.
    key(&mut app, Key::Escape);
    let other = categories()
        .iter()
        .find(|c| c.title != road_title())
        .unwrap();
    select::<AchievementsState>(&mut app, other.title);
    assert_eq!(
        labels::<AchievementCategoryState>(&app)[0],
        "Sort: Earned first"
    );

    // Stepping back round to the screen's order drops the pick.
    key(&mut app, Key::Escape);
    select::<AchievementsState>(&mut app, road_title());
    move_to::<AchievementCategoryState>(&mut app, "Sort:");
    key(&mut app, Key::Left);
    assert_eq!(
        current_label::<AchievementCategoryState>(&app),
        "Sort: Earned first"
    );
    key(&mut app, Key::Escape);
    move_to::<AchievementsState>(&mut app, "Sort:");
    key(&mut app, Key::Right);
    select::<AchievementsState>(&mut app, road_title());
    assert_eq!(
        labels::<AchievementCategoryState>(&app)[0],
        "Sort: Locked first",
        "with no pick of its own, the category follows the new screen order"
    );
}
