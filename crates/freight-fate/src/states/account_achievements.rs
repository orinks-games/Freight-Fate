//! Read-only category browser for achievements earned across this installation.

use ff_core::achievements::{
    entry_text, Achievement, AchievementCategory, EarnedOrder, ACHIEVEMENTS,
};

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::states::achievement_sort::{
    category_sort, chosen_sort, sorted_achievements, sorted_categories, step_category_sort,
    step_chosen_sort, SortOverrides, CATEGORIES_SORT_HELP, CATEGORY_SORT_HELP,
};
use crate::states::base::{Key, Label, Menu, MenuCore, MenuItem};

const CONTROLS: &str = "Enter opens a category. Inside one, Enter repeats the achievement \
and Escape returns to the categories. From the categories, Escape returns to Online. This \
read-only collection combines every career on this installation.";

/// The account's earned badges, ranked by when they were first earned. A
/// badge the ledger has no trustworthy time for counts as the oldest.
fn account_order(ctx: &GameContext) -> EarnedOrder {
    let ledger = &ctx.account_achievements;
    ledger
        .ids()
        .into_iter()
        .map(|id| {
            let rank = ledger.earned_at(&id).unwrap_or(i64::MIN);
            (id, rank)
        })
        .collect()
}

pub struct AccountAchievementsState {
    menu: MenuCore<Self>,
    overrides: SortOverrides,
}

/// The sort row is the first row of both screens, above the list it sorts.
const SORT_ROW: usize = 0;

impl AccountAchievementsState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Account achievements").with_intro_help(CONTROLS),
            overrides: SortOverrides::default(),
        }
    }

    fn step_sort(&mut self, ctx: &mut GameContext, direction: i64) {
        step_chosen_sort(ctx, direction);
        self.refresh(ctx, true);
    }

    fn on_sort_row(&self) -> bool {
        self.menu.index == SORT_ROW
    }

    fn earned(ctx: &GameContext) -> EarnedOrder {
        account_order(ctx)
    }

    fn summary_label(ctx: &GameContext) -> String {
        format!(
            "Summary: {} of {} earned across every career",
            ctx.account_achievements.ids().len(),
            ACHIEVEMENTS.len()
        )
    }
}

impl Default for AccountAchievementsState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for AccountAchievementsState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let earned = ctx.account_achievements.ids().len();
        ctx.say(&format!(
            "Account achievements. {earned} of {} earned across every career on this installation. {}",
            ACHIEVEMENTS.len(),
            self.current_text(ctx)
        ));
    }

    fn handle_extra_key(&mut self, ctx: &mut GameContext, key: Key) -> bool {
        let direction = match key {
            Key::Right => 1,
            Key::Left => -1,
            _ => return false,
        };
        if !self.on_sort_row() {
            return false;
        }
        self.step_sort(ctx, direction);
        true
    }

    fn adjust(&mut self, ctx: &mut GameContext, direction: i64) {
        if self.on_sort_row() {
            self.step_sort(ctx, direction);
        }
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let earned = Self::earned(ctx);
        let mut items = vec![MenuItem::new(
            Label::dynamic(|_s: &Self, ctx| chosen_sort(ctx).label().to_string()),
            |s: &mut Self, ctx| s.step_sort(ctx, 1),
        )
        .help(CATEGORIES_SORT_HELP)
        .select_sound(None)];
        for row in sorted_categories(chosen_sort(ctx), &earned) {
            let category: &'static AchievementCategory = row.category;
            let done = row.earned;
            let achievements = row.achievements;
            items.push(
                MenuItem::new(
                    format!("{}. {done} of {}", category.title, achievements.len()),
                    move |s: &mut Self, ctx| {
                        ctx.push_state(
                            AccountAchievementCategoryState::new(category, achievements.clone())
                                .with_overrides(s.overrides.clone()),
                        )
                    },
                )
                .help(category.description),
            );
        }
        items.push(
            MenuItem::new(
                Label::dynamic(|_s: &Self, ctx| Self::summary_label(ctx)),
                |_s: &mut Self, ctx| ctx.say(&Self::summary_label(ctx)),
            )
            .help("The account-wide earned count."),
        );
        items.push(
            MenuItem::new("Controls and account scope", |_s: &mut Self, ctx| {
                ctx.say(CONTROLS)
            })
            .help(CONTROLS),
        );
        items.push(MenuItem::new("Back to Online", |s: &mut Self, ctx| {
            s.go_back(ctx)
        }));
        items
    }
}

impl_state_for_menu!(AccountAchievementsState);

/// One category's account-wide achievements, under a sort row that follows
/// the categories screen's order unless it picked another for this category.
pub struct AccountAchievementCategoryState {
    menu: MenuCore<Self>,
    pub category: &'static AchievementCategory,
    /// The category's badges in catalog order; each build sorts a copy.
    pub achievements: Vec<&'static Achievement>,
    overrides: SortOverrides,
}

impl AccountAchievementCategoryState {
    pub fn new(
        category: &'static AchievementCategory,
        achievements: Vec<&'static Achievement>,
    ) -> Self {
        Self {
            menu: MenuCore::new(category.title).with_intro_help(
                "Enter repeats the achievement. Escape returns to the account categories.",
            ),
            category,
            achievements,
            overrides: SortOverrides::default(),
        }
    }

    /// Share the categories screen's per-category picks.
    pub fn with_overrides(mut self, overrides: SortOverrides) -> Self {
        self.overrides = overrides;
        self
    }

    fn earned(ctx: &GameContext) -> EarnedOrder {
        account_order(ctx)
    }

    fn step_sort(&mut self, ctx: &mut GameContext, direction: i64) {
        step_category_sort(ctx, &self.overrides, self.category, direction);
        self.refresh(ctx, true);
    }

    fn on_sort_row(&self) -> bool {
        self.menu.index == SORT_ROW
    }
}

impl Menu for AccountAchievementCategoryState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let earned = Self::earned(ctx);
        let done = self
            .achievements
            .iter()
            .filter(|achievement| earned.contains_key(achievement.id))
            .count();
        ctx.say(&format!(
            "Account achievements, {}. {done} of {} earned. {}",
            self.category.title,
            self.achievements.len(),
            self.current_text(ctx)
        ));
    }

    fn handle_extra_key(&mut self, ctx: &mut GameContext, key: Key) -> bool {
        let direction = match key {
            Key::Right => 1,
            Key::Left => -1,
            _ => return false,
        };
        if !self.on_sort_row() {
            return false;
        }
        self.step_sort(ctx, direction);
        true
    }

    fn adjust(&mut self, ctx: &mut GameContext, direction: i64) {
        if self.on_sort_row() {
            self.step_sort(ctx, direction);
        }
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let earned = Self::earned(ctx);
        let sort = category_sort(ctx, &self.overrides, self.category);
        let mut items = Vec::with_capacity(self.achievements.len() + 2);
        items.push(
            MenuItem::new(
                Label::dynamic(|s: &Self, ctx| {
                    category_sort(ctx, &s.overrides, s.category)
                        .label()
                        .to_string()
                }),
                |s: &mut Self, ctx| s.step_sort(ctx, 1),
            )
            .help(CATEGORY_SORT_HELP)
            .select_sound(None),
        );
        for achievement in sorted_achievements(sort, &self.achievements, &earned) {
            let unlocked = earned.contains_key(achievement.id);
            let (name, description) = entry_text(achievement, unlocked);
            let (label, help) = if unlocked {
                (format!("Earned: {name}. {description}"), description)
            } else if achievement.hidden {
                (format!("Locked: {name}"), description)
            } else {
                (format!("Locked: {name}"), "Keep playing to unlock it.")
            };
            let spoken = label.clone();
            items.push(MenuItem::new(label, move |_s: &mut Self, ctx| ctx.say(&spoken)).help(help));
        }
        items.push(MenuItem::new(
            "Back to account categories",
            |s: &mut Self, ctx| s.go_back(ctx),
        ));
        items
    }
}

impl_state_for_menu!(AccountAchievementCategoryState);
