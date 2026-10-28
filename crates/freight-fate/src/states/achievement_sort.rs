//! The sort row both achievements browsers share: the career one under the
//! main menu and the account one under Online.
//!
//! The categories screen holds the player's chosen order, saved in settings,
//! and lists its categories in it. Each category opens in that order too,
//! unless the player picked another order inside that category; those
//! picks last while the categories screen stays open.

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

use ff_core::achievements::{
    achievements_in_category, categories, Achievement, AchievementCategory, AchievementSort,
};

use crate::app::GameContext;
use crate::states::base::end_sentence;

/// Per-category orders picked inside a category, shared by the categories
/// screen and the category it opened.
pub type SortOverrides = Rc<RefCell<HashMap<&'static str, AchievementSort>>>;

pub const CATEGORIES_SORT_HELP: &str =
    "Left and Right change the order of the categories and the achievements in each one.";

pub const CATEGORY_SORT_HELP: &str = "Left and Right change the order of this category only.";

/// The order the categories screen uses, from settings.
pub fn chosen_sort(ctx: &GameContext) -> AchievementSort {
    AchievementSort::from_key(&ctx.settings.achievement_sort)
}

/// Step the saved order and say the new one.
pub fn step_chosen_sort(ctx: &mut GameContext, direction: i64) {
    let sort = chosen_sort(ctx).step(direction);
    ctx.settings.achievement_sort = sort.key().to_string();
    if let Err(e) = ctx.settings.save() {
        log::warn!("Could not save settings: {e}");
    }
    announce(ctx, sort);
}

/// The order one category lists in: its own pick, or the screen's.
pub fn category_sort(
    ctx: &GameContext,
    overrides: &SortOverrides,
    category: &AchievementCategory,
) -> AchievementSort {
    overrides
        .borrow()
        .get(category.id)
        .copied()
        .unwrap_or_else(|| chosen_sort(ctx))
}

/// Step one category's order and say the new one. Landing back on the
/// screen's order drops the pick, so the category follows the screen again.
pub fn step_category_sort(
    ctx: &mut GameContext,
    overrides: &SortOverrides,
    category: &'static AchievementCategory,
    direction: i64,
) {
    let sort = category_sort(ctx, overrides, category).step(direction);
    if sort == chosen_sort(ctx) {
        overrides.borrow_mut().remove(category.id);
    } else {
        overrides.borrow_mut().insert(category.id, sort);
    }
    announce(ctx, sort);
}

fn announce(ctx: &mut GameContext, sort: AchievementSort) {
    ctx.audio.play("ui/menu_select");
    ctx.say(&end_sentence(sort.name()));
}

/// One categories-screen row: the category, its badges in catalog order,
/// and how many of them are earned.
pub struct CategoryRow {
    pub category: &'static AchievementCategory,
    pub achievements: Vec<&'static Achievement>,
    pub earned: usize,
}

/// Every category with its counts, in `sort`'s order.
pub fn sorted_categories(sort: AchievementSort, earned: &HashSet<String>) -> Vec<CategoryRow> {
    let mut rows: Vec<CategoryRow> = categories()
        .iter()
        .map(|category| {
            let achievements = achievements_in_category(category.id);
            let done = achievements
                .iter()
                .filter(|a| earned.contains(a.id))
                .count();
            CategoryRow {
                category,
                achievements,
                earned: done,
            }
        })
        .collect();
    sort.sort_categories(
        &mut rows,
        |row| row.category,
        |row| (row.earned, row.achievements.len()),
    );
    rows
}

/// One category's badges in `sort`'s order.
pub fn sorted_achievements(
    sort: AchievementSort,
    achievements: &[&'static Achievement],
    earned: &HashSet<String>,
) -> Vec<&'static Achievement> {
    let mut list = achievements.to_vec();
    sort.sort_achievements(&mut list, earned);
    list
}
