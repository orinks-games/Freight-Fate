//! The orders the achievements screens can list in.
//!
//! One choice sorts both lists: the categories on the categories screen and
//! the badges inside each category. Earned first is the default, so a player
//! opening the screen hears what they have before what is left. Ties keep the
//! catalog's own order, which is also the "Original order" choice.
//!
//! The earned badges come in as an [`EarnedOrder`]: each earned id with a
//! number that grows with how recently it was earned, which is all "Newest"
//! needs. A career passes its award order; the account ledger passes its
//! earned times.

use std::cmp::Reverse;
use std::collections::HashMap;

use super::{entry_text, Achievement, AchievementCategory, HIDDEN_NAME};

/// Each earned achievement id, with a number that is larger the more
/// recently it was earned. Ids missing from the map are locked.
pub type EarnedOrder = HashMap<String, i64>;

/// A career's earned order: its award list, oldest first.
pub fn career_order(awarded: &[String]) -> EarnedOrder {
    awarded
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), i as i64))
        .collect()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum AchievementSort {
    /// Earned badges first; categories with the most earned first.
    #[default]
    EarnedFirst,
    /// The most recently earned badges first, then the locked ones;
    /// categories by their most recent badge.
    Newest,
    /// Locked badges first; categories with the most left to earn first.
    LockedFirst,
    /// By name. Hidden badges still locked go last, since their spoken
    /// name is the same keeping-the-secret line for every one of them.
    AToZ,
    /// The catalog's order.
    Original,
}

impl AchievementSort {
    /// Every choice, in the order Left and Right step through them.
    pub const ALL: [AchievementSort; 5] = [
        AchievementSort::EarnedFirst,
        AchievementSort::Newest,
        AchievementSort::LockedFirst,
        AchievementSort::AToZ,
        AchievementSort::Original,
    ];

    /// The value stored in the settings file.
    pub fn key(self) -> &'static str {
        match self {
            AchievementSort::EarnedFirst => "earned",
            AchievementSort::Newest => "newest",
            AchievementSort::LockedFirst => "locked",
            AchievementSort::AToZ => "name",
            AchievementSort::Original => "original",
        }
    }

    /// The stored value read back; anything unknown is the default.
    pub fn from_key(key: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|sort| sort.key() == key)
            .unwrap_or_default()
    }

    /// The spoken value, read alone after Left or Right.
    pub fn name(self) -> &'static str {
        match self {
            AchievementSort::EarnedFirst => "Earned first",
            AchievementSort::Newest => "Newest",
            AchievementSort::LockedFirst => "Locked first",
            AchievementSort::AToZ => "A to Z",
            AchievementSort::Original => "Original order",
        }
    }

    /// The menu row. Each is a whole literal so a catalog can translate it.
    pub fn label(self) -> &'static str {
        match self {
            AchievementSort::EarnedFirst => "Sort: Earned first",
            AchievementSort::Newest => "Sort: Newest",
            AchievementSort::LockedFirst => "Sort: Locked first",
            AchievementSort::AToZ => "Sort: A to Z",
            AchievementSort::Original => "Sort: Original order",
        }
    }

    /// The next choice, wrapping, in `direction` (positive steps forward).
    pub fn step(self, direction: i64) -> Self {
        let count = Self::ALL.len() as i64;
        let here = Self::ALL.iter().position(|s| *s == self).unwrap_or(0) as i64;
        let next = (here + direction.signum()).rem_euclid(count);
        Self::ALL[next as usize]
    }

    /// Sort one category's badges in place. Stable, so ties keep the
    /// catalog order the list came in.
    pub fn sort_achievements(self, achs: &mut [&'static Achievement], earned: &EarnedOrder) {
        let is_earned = |a: &Achievement| earned.contains_key(a.id);
        match self {
            AchievementSort::EarnedFirst => achs.sort_by_key(|a| !is_earned(a)),
            AchievementSort::Newest => {
                achs.sort_by_key(|a| Reverse(earned.get(a.id).map(|&rank| Some(rank))))
            }
            AchievementSort::LockedFirst => achs.sort_by_key(|a| is_earned(a)),
            AchievementSort::AToZ => achs.sort_by_cached_key(|a| {
                let (name, _) = entry_text(a, is_earned(a));
                (name == HIDDEN_NAME, name.to_lowercase())
            }),
            AchievementSort::Original => {}
        }
    }

    /// Sort the categories in place, given each one's (earned, total)
    /// count and the rank of its most recently earned badge. Stable, so
    /// ties keep the catalog order.
    pub fn sort_categories<T>(
        self,
        rows: &mut [T],
        category: impl Fn(&T) -> &'static AchievementCategory,
        counts: impl Fn(&T) -> (usize, usize),
        newest: impl Fn(&T) -> Option<i64>,
    ) {
        match self {
            AchievementSort::EarnedFirst => rows.sort_by_key(|r| Reverse(counts(r).0)),
            AchievementSort::Newest => rows.sort_by_key(|r| Reverse(newest(r))),
            AchievementSort::LockedFirst => rows.sort_by_key(|r| {
                let (done, total) = counts(r);
                Reverse(total.saturating_sub(done))
            }),
            AchievementSort::AToZ => rows.sort_by_cached_key(|r| category(r).title.to_lowercase()),
            AchievementSort::Original => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::achievements::{achievements_in_category, categories};

    fn earned(ids: &[&str]) -> EarnedOrder {
        let ids: Vec<String> = ids.iter().map(|s| s.to_string()).collect();
        career_order(&ids)
    }

    #[test]
    fn stored_values_round_trip_and_junk_is_the_default() {
        for sort in AchievementSort::ALL {
            assert_eq!(AchievementSort::from_key(sort.key()), sort);
        }
        assert_eq!(
            AchievementSort::from_key("junk"),
            AchievementSort::EarnedFirst
        );
        assert_eq!(AchievementSort::default(), AchievementSort::EarnedFirst);
    }

    #[test]
    fn stepping_wraps_both_ways() {
        assert_eq!(
            AchievementSort::EarnedFirst.step(-1),
            AchievementSort::Original
        );
        assert_eq!(
            AchievementSort::Original.step(1),
            AchievementSort::EarnedFirst
        );
        assert_eq!(
            AchievementSort::EarnedFirst.step(1),
            AchievementSort::Newest
        );
    }

    #[test]
    fn earned_first_and_locked_first_keep_catalog_order_within_each_half() {
        let catalog = achievements_in_category("road");
        let last = catalog.last().unwrap().id;
        let first = catalog[0].id;
        let got = earned(&[last]);

        let mut list = catalog.clone();
        AchievementSort::EarnedFirst.sort_achievements(&mut list, &got);
        assert_eq!(list[0].id, last);
        assert_eq!(list[1].id, first);

        let mut list = catalog.clone();
        AchievementSort::LockedFirst.sort_achievements(&mut list, &got);
        assert_eq!(list.last().unwrap().id, last);
        assert_eq!(list[0].id, first);

        let mut list = catalog.clone();
        AchievementSort::Original.sort_achievements(&mut list, &got);
        assert_eq!(list, catalog);
    }

    #[test]
    fn a_to_z_puts_hidden_locked_badges_last() {
        let mut list = achievements_in_category("hidden");
        let one = list[0].id;
        AchievementSort::AToZ.sort_achievements(&mut list, &earned(&[one]));
        assert_eq!(
            list[0].id, one,
            "the one earned hidden badge has a real name"
        );
        for a in &list[1..] {
            assert!(a.hidden);
        }

        let mut road = achievements_in_category("road");
        AchievementSort::AToZ.sort_achievements(&mut road, &earned(&[]));
        let names: Vec<String> = road
            .iter()
            .filter(|a| !a.hidden)
            .map(|a| a.name.to_lowercase())
            .collect();
        let mut sorted = names.clone();
        sorted.sort();
        assert_eq!(names, sorted);
    }

    #[test]
    fn categories_sort_by_counts_and_title() {
        let cats: Vec<&'static AchievementCategory> = categories().iter().take(3).collect();
        let counts = |c: &&'static AchievementCategory| match c.id {
            id if id == cats[0].id => (1, 10),
            id if id == cats[1].id => (5, 6),
            _ => (0, 4),
        };
        let mut rows = cats.clone();
        AchievementSort::EarnedFirst.sort_categories(&mut rows, |c| c, counts, |_| None);
        assert_eq!(rows, vec![cats[1], cats[0], cats[2]]);

        let mut rows = cats.clone();
        AchievementSort::LockedFirst.sort_categories(&mut rows, |c| c, counts, |_| None);
        assert_eq!(rows, vec![cats[0], cats[2], cats[1]]);

        let mut rows = cats.clone();
        AchievementSort::AToZ.sort_categories(&mut rows, |c| c, counts, |_| None);
        let titles: Vec<&str> = rows.iter().map(|c| c.title).collect();
        let mut sorted = titles.clone();
        sorted.sort_by_key(|t| t.to_lowercase());
        assert_eq!(titles, sorted);
    }

    #[test]
    fn newest_lists_the_latest_award_first_then_the_locked_badges() {
        let catalog = achievements_in_category("road");
        let (a, b, c) = (catalog[0].id, catalog[3].id, catalog[1].id);
        // Awarded in the order a, b, c: c is the newest.
        let mut list = catalog.clone();
        AchievementSort::Newest.sort_achievements(&mut list, &earned(&[a, b, c]));
        let first: Vec<&str> = list.iter().take(3).map(|x| x.id).collect();
        assert_eq!(first, vec![c, b, a]);
        assert_eq!(
            list[3].id, catalog[2].id,
            "locked badges keep catalog order"
        );

        let cats: Vec<&'static AchievementCategory> = categories().iter().take(3).collect();
        let newest = |c: &&'static AchievementCategory| match c.id {
            id if id == cats[0].id => Some(1),
            id if id == cats[2].id => Some(7),
            _ => None,
        };
        let mut rows = cats.clone();
        AchievementSort::Newest.sort_categories(&mut rows, |c| c, |_| (0, 1), newest);
        assert_eq!(rows, vec![cats[2], cats[0], cats[1]]);
    }
}
