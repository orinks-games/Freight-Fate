//! Persistent player achievements and notification helpers.
//!
//! Port of `freight_fate/achievements.py`. The 173-badge catalog itself lives
//! in [`catalog`], generated string-for-string from the Python source; this
//! file is the API around it: categories, the hidden-badge menu text, the
//! award, and the small stat helpers every trigger site counts with.
//!
//! The Python module reached into `Profile` for two attributes
//! (`achievements`, the earned id list, and `achievement_stats`, the
//! free-form counter dict). Here those are the [`AchievementProfile`] trait,
//! which the profile model implements; the trigger sites in the states call
//! `ctx.award_achievement(id)`, which ends in [`award`].

use std::collections::HashSet;

use serde_json::{Map, Value};

use crate::pyfmt::py_str_float;
use crate::speech_text::{achievement_unlocked, SpokenMessage};

pub mod catalog;
pub mod sort;

pub use catalog::{ACHIEVEMENTS, CATALOG_DIGEST, CATEGORIES, HIDDEN_HELP, HIDDEN_NAME};
pub use sort::{career_order, AchievementSort, EarnedOrder};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Achievement {
    pub id: &'static str,
    pub name: &'static str,
    pub description: &'static str,
    pub category: &'static str,
    pub inspiration: &'static str,
    /// Hidden badges keep their name and description to themselves until
    /// earned; the achievements screen speaks HIDDEN_NAME/HIDDEN_HELP for
    /// them instead, so the surprise survives a locked-list browse.
    pub hidden: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AchievementCategory {
    pub id: &'static str,
    pub title: &'static str,
    pub description: &'static str,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AchievementAward {
    pub achievement: &'static Achievement,
    pub message: SpokenMessage,
}

/// The two profile attributes the Python module read and wrote by name:
/// `profile.achievements` (earned ids, in award order) and
/// `profile.achievement_stats` (the free-form counter dict the trigger sites
/// keep their tallies in). The profile model implements this; tests use a
/// bare struct.
pub trait AchievementProfile {
    /// `profile.achievements`.
    fn achievements(&self) -> &[String];
    /// `profile.achievements`, for the award to append to.
    fn achievements_mut(&mut self) -> &mut Vec<String>;
    /// `profile.achievement_stats`. The Python `_stats` replaced anything
    /// that was not a dict with an empty one; the profile model keeps the
    /// field an object from load onward, so the slot is the map itself.
    fn achievement_stats_mut(&mut self) -> &mut Map<String, Value>;
}

/// `CATEGORIES`, as the Python accessor returned it.
pub fn categories() -> &'static [AchievementCategory] {
    &CATEGORIES
}

/// `CATEGORY_BY_ID[category_id]`.
pub fn category_by_id(category_id: &str) -> Option<&'static AchievementCategory> {
    CATEGORIES
        .iter()
        .find(|category| category.id == category_id)
}

/// `ACHIEVEMENT_BY_ID[achievement_id]`.
pub fn achievement_by_id(achievement_id: &str) -> Option<&'static Achievement> {
    ACHIEVEMENTS
        .iter()
        .find(|achievement| achievement.id == achievement_id)
}

pub fn achievements_in_category(category_id: &str) -> Vec<&'static Achievement> {
    ACHIEVEMENTS
        .iter()
        .filter(|achievement| achievement.category == category_id)
        .collect()
}

/// (name, detail) for a menu row, respecting hidden achievements.
///
/// Unlocked badges always speak their real name and story. Locked badges
/// speak their real name too (the title reads as the goal) unless the badge
/// is hidden, in which case it speaks the single keeping-the-secret line
/// until it is earned.
pub fn entry_text(achievement: &Achievement, unlocked: bool) -> (&'static str, &'static str) {
    if unlocked || !achievement.hidden {
        return (achievement.name, achievement.description);
    }
    (HIDDEN_NAME, HIDDEN_HELP)
}

pub fn earned_ids(profile: &dyn AchievementProfile) -> HashSet<String> {
    profile.achievements().iter().cloned().collect()
}

/// Record the badge on the profile and build its full message. `None` when
/// the profile already holds it -- or, where Python raised `KeyError`, when
/// the id is not in the catalog (logged; a trigger site naming a badge that
/// does not exist is a programming error, not a player event).
pub fn award(
    profile: &mut dyn AchievementProfile,
    achievement_id: &str,
) -> Option<AchievementAward> {
    let Some(achievement) = achievement_by_id(achievement_id) else {
        log::error!("award: no achievement with id {achievement_id:?}");
        return None;
    };
    if earned_ids(profile).contains(achievement.id) {
        return None;
    }
    profile.achievements_mut().push(achievement.id.to_string());
    // A normal/terse pair: mid-drive a terse player hears the earcon and the
    // name alone; the flavor text waits in the log and the achievements menu.
    Some(AchievementAward {
        achievement,
        message: achievement_unlocked(achievement.name, achievement.description),
    })
}

/// Python `str(value)` for the JSON scalars a stat list can hold. Lists and
/// dicts inside a stat list have no Python-repr equivalent here; they become
/// their compact JSON text (no real save carries them).
fn py_str(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::Null => "None".to_string(),
        Value::Number(n) => match (n.as_i64(), n.as_u64(), n.as_f64()) {
            (Some(i), _, _) => i.to_string(),
            (None, Some(u), _) => u.to_string(),
            (None, None, Some(f)) => py_str_float(f),
            (None, None, None) => n.to_string(),
        },
        other => other.to_string(),
    }
}

/// The stored list for `key`, normalised to strings (and written back that
/// way), or an empty one when the slot is missing or not a list.
pub fn list_stat(profile: &mut dyn AchievementProfile, key: &str) -> Vec<String> {
    let stats = profile.achievement_stats_mut();
    let values: Vec<String> = match stats.get(key) {
        Some(Value::Array(raw)) => raw.iter().map(py_str).collect(),
        _ => Vec::new(),
    };
    stats.insert(
        key.to_string(),
        Value::Array(values.iter().cloned().map(Value::String).collect()),
    );
    values
}

/// Append `value` to the list stat if it is not already there; the list's
/// new length is the tally the badge thresholds compare against.
pub fn add_unique_stat(profile: &mut dyn AchievementProfile, key: &str, value: &str) -> usize {
    let mut values = list_stat(profile, key);
    if !values.iter().any(|existing| existing == value) {
        values.push(value.to_string());
    }
    let count = values.len();
    profile.achievement_stats_mut().insert(
        key.to_string(),
        Value::Array(values.into_iter().map(Value::String).collect()),
    );
    count
}

/// Python `int(value)`, with `TypeError`/`ValueError` reading as 0.
fn py_int_or_zero(value: Option<&Value>) -> i64 {
    match value {
        None | Some(Value::Null) | Some(Value::Array(_)) | Some(Value::Object(_)) => 0,
        Some(Value::Bool(b)) => i64::from(*b),
        Some(Value::Number(n)) => {
            if let Some(i) = n.as_i64() {
                i
            } else if let Some(f) = n.as_f64() {
                if f.is_finite() {
                    f.trunc() as i64
                } else {
                    0
                }
            } else {
                0
            }
        }
        Some(Value::String(s)) => {
            // int() accepts surrounding whitespace and digit-group
            // underscores ("1_000"); a decimal point or anything else is a
            // ValueError.
            let trimmed = s.trim();
            let cleaned: String = trimmed.chars().filter(|c| *c != '_').collect();
            let digits_ok = !cleaned.is_empty()
                && !trimmed.starts_with('_')
                && !trimmed.ends_with('_')
                && !trimmed.contains("__");
            if digits_ok {
                cleaned.parse::<i64>().unwrap_or(0)
            } else {
                0
            }
        }
    }
}

pub fn int_stat(profile: &mut dyn AchievementProfile, key: &str) -> i64 {
    py_int_or_zero(profile.achievement_stats_mut().get(key))
}

pub fn increment_stat(profile: &mut dyn AchievementProfile, key: &str) -> i64 {
    let value = int_stat(profile, key) + 1;
    profile
        .achievement_stats_mut()
        .insert(key.to_string(), Value::from(value));
    value
}

/// Zero a running counter, for the streaks a single bad day ends.
pub fn reset_stat(profile: &mut dyn AchievementProfile, key: &str) {
    profile
        .achievement_stats_mut()
        .insert(key.to_string(), Value::from(0));
}

/// Python truthiness of a JSON value (`bool(value)`).
fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

pub fn bool_stat(profile: &mut dyn AchievementProfile, key: &str) -> bool {
    profile
        .achievement_stats_mut()
        .get(key)
        .is_some_and(py_truthy)
}

pub fn set_bool_stat(profile: &mut dyn AchievementProfile, key: &str) {
    profile
        .achievement_stats_mut()
        .insert(key.to_string(), Value::Bool(true));
}

#[cfg(test)]
mod tests;
