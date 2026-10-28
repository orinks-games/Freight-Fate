//! The achievements browser: pick a career, then a category, then read its
//! badges (the `Achievement*State` classes of `main_menu.py`).

use ff_core::achievements::{
    earned_ids, entry_text, Achievement, AchievementCategory, AchievementProfile, ACHIEVEMENTS,
};
use ff_core::models::profile::Profile;

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::states::achievement_sort::{
    category_sort, chosen_sort, sorted_achievements, sorted_categories, step_category_sort,
    step_chosen_sort, SortOverrides, CATEGORIES_SORT_HELP, CATEGORY_SORT_HELP,
};
use crate::states::base::{Key, Label, Menu, MenuCore, MenuItem};
use crate::states::main_menu::loadable_saves;

fn earned_count(profile: &Profile) -> usize {
    earned_ids(profile as &dyn AchievementProfile).len()
}

pub struct AchievementCareerState {
    menu: MenuCore<Self>,
}

impl AchievementCareerState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Achievements").with_intro_help(
                "Pick a saved career. Enter opens its achievements, Escape goes back.",
            ),
        }
    }
}

impl Default for AchievementCareerState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for AchievementCareerState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let first = self.menu.items.first().map(|item| item.text(self, ctx));
        if first.as_deref().is_none_or(|text| text == "Back") {
            ctx.say("Achievements. No saved careers yet.");
            return;
        }
        let text = format!("Achievements. {}", self.current_text(ctx));
        ctx.say(&text);
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items = Vec::new();
        for (_path, profile) in loadable_saves() {
            let earned = earned_count(&profile);
            let total = ACHIEVEMENTS.len();
            let name = profile.name.clone();
            items.push(
                MenuItem::new(
                    format!("{name}: {earned} of {total} earned"),
                    move |_s: &mut Self, ctx| {
                        ctx.push_state(AchievementsState::new(profile.clone()))
                    },
                )
                .help(format!("Achievements for {name}.")),
            );
        }
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }
}

impl_state_for_menu!(AchievementCareerState);

/// The category menu: pick a category, then browse its achievements. The
/// sort row orders the categories and, by default, the badges inside them.
pub struct AchievementsState {
    menu: MenuCore<Self>,
    pub profile: Profile,
    overrides: SortOverrides,
}

/// Where the sort row sits on the categories screen: right after Summary.
const CATEGORIES_SORT_ROW: usize = 1;

impl AchievementsState {
    pub fn new(profile: Profile) -> Self {
        Self {
            menu: MenuCore::new(&format!("Achievements for {}", profile.name))
                .with_intro_help("Up and Down pick a category, Enter opens it, Escape goes back."),
            profile,
            overrides: SortOverrides::default(),
        }
    }

    fn summary_label(&self) -> String {
        format!(
            "Summary: {} of {} earned",
            earned_count(&self.profile),
            ACHIEVEMENTS.len()
        )
    }

    fn summary(&mut self, ctx: &mut GameContext) {
        ctx.say(&format!(
            "{} has earned {} of {} achievements.",
            self.profile.name,
            earned_count(&self.profile),
            ACHIEVEMENTS.len()
        ));
    }

    fn step_sort(&mut self, ctx: &mut GameContext, direction: i64) {
        step_chosen_sort(ctx, direction);
        self.refresh(ctx, true);
    }

    fn on_sort_row(&self) -> bool {
        self.menu.index == CATEGORIES_SORT_ROW
    }
}

impl Menu for AchievementsState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let text = format!(
            "Achievements for {}. {} of {} earned. {}",
            self.profile.name,
            earned_count(&self.profile),
            ACHIEVEMENTS.len(),
            self.current_text(ctx)
        );
        ctx.say(&text);
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
        let earned = earned_ids(&self.profile as &dyn AchievementProfile);
        let mut items = vec![
            MenuItem::new(
                Label::dynamic(|s: &Self, _ctx| s.summary_label()),
                |s: &mut Self, ctx| s.summary(ctx),
            )
            .help("The total earned."),
            MenuItem::new(
                Label::dynamic(|_s: &Self, ctx| chosen_sort(ctx).label().to_string()),
                |s: &mut Self, ctx| s.step_sort(ctx, 1),
            )
            .help(CATEGORIES_SORT_HELP)
            .select_sound(None),
        ];
        debug_assert_eq!(items.len(), CATEGORIES_SORT_ROW + 1);
        for row in sorted_categories(chosen_sort(ctx), &earned) {
            let c: &'static AchievementCategory = row.category;
            let done = row.earned;
            let achs = row.achievements;
            let a = achs.clone();
            items.push(
                MenuItem::new(
                    format!("{}. {done} of {}", c.title, achs.len()),
                    move |s: &mut Self, ctx| {
                        ctx.push_state(
                            AchievementCategoryState::new(s.profile.clone(), c, a.clone())
                                .with_overrides(s.overrides.clone()),
                        )
                    },
                )
                .help(c.description),
            );
        }
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)));
        items
    }
}

impl_state_for_menu!(AchievementsState);

/// One category's achievements: earned ones tell their story, locked ones
/// show their goal, and hidden ones keep the secret until earned. The sort
/// row after the badges changes this category's order alone.
pub struct AchievementCategoryState {
    menu: MenuCore<Self>,
    pub profile: Profile,
    pub category: &'static AchievementCategory,
    /// The category's badges in catalog order; each build sorts a copy.
    pub achs: Vec<&'static Achievement>,
    overrides: SortOverrides,
}

impl AchievementCategoryState {
    pub fn new(
        profile: Profile,
        category: &'static AchievementCategory,
        achs: Vec<&'static Achievement>,
    ) -> Self {
        Self {
            menu: MenuCore::new(category.title)
                .with_intro_help("Up and Down move, Enter repeats the entry, Escape goes back."),
            profile,
            category,
            achs,
            overrides: SortOverrides::default(),
        }
    }

    /// Share the categories screen's per-category picks.
    pub fn with_overrides(mut self, overrides: SortOverrides) -> Self {
        self.overrides = overrides;
        self
    }

    fn earned_count(&self) -> usize {
        let earned = earned_ids(&self.profile as &dyn AchievementProfile);
        self.achs.iter().filter(|a| earned.contains(a.id)).count()
    }

    fn step_sort(&mut self, ctx: &mut GameContext, direction: i64) {
        step_category_sort(ctx, &self.overrides, self.category, direction);
        self.refresh(ctx, true);
    }

    fn on_sort_row(&self) -> bool {
        self.menu.index == self.achs.len()
    }
}

impl Menu for AchievementCategoryState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let text = format!(
            "{}. {} of {} earned. {}",
            self.category.title,
            self.earned_count(),
            self.achs.len(),
            self.current_text(ctx)
        );
        ctx.say(&text);
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
        let earned = earned_ids(&self.profile as &dyn AchievementProfile);
        let sort = category_sort(ctx, &self.overrides, self.category);
        let mut items = Vec::new();
        for achievement in sorted_achievements(sort, &self.achs, &earned) {
            let unlocked = earned.contains(achievement.id);
            let (name, description) = entry_text(achievement, unlocked);
            let (label, help_text) = if unlocked {
                (format!("Earned: {name} - {description}"), description)
            } else if achievement.hidden {
                (format!("Locked: {name}"), description)
            } else {
                // Locked, non-hidden entries show only the title; the
                // description stays hidden until the achievement is earned.
                (format!("Locked: {name}"), "Keep playing to unlock it.")
            };
            let spoken = label.clone();
            items.push(
                MenuItem::new(label, move |_s: &mut Self, ctx| ctx.say(&spoken)).help(help_text),
            );
        }
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
        items.push(MenuItem::new(
            "Back to the categories",
            |s: &mut Self, ctx| s.go_back(ctx),
        ));
        items
    }
}

impl_state_for_menu!(AchievementCategoryState);
