//! Ported from `tests/test_achievements.py` (the pure parts),
//! `tests/test_career_arc_achievements.py` (the catalog checks) and
//! `tests/test_achievement_flavor_relocated.py`. Everything that drove a
//! `DrivingState`/`ArrivalState` through `App()` is listed `#[ignore]` with
//! its Python body summarised, until the app shell lands.

use std::collections::HashSet;

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::*;
use crate::speech_text::achievement_announced;

/// The two profile attributes the module touches, as a bare test double.
#[derive(Default)]
struct TestProfile {
    achievements: Vec<String>,
    achievement_stats: Map<String, Value>,
}

impl AchievementProfile for TestProfile {
    fn achievements(&self) -> &[String] {
        &self.achievements
    }
    fn achievements_mut(&mut self) -> &mut Vec<String> {
        &mut self.achievements
    }
    fn achievement_stats_mut(&mut self) -> &mut Map<String, Value> {
        &mut self.achievement_stats
    }
}

/// The generator's framing: categories then badges, fields joined by
/// U+001F, records ended by U+001E, the two tables split by U+001D.
fn catalog_digest() -> String {
    let mut hasher = Sha256::new();
    for category in &CATEGORIES {
        hasher.update([category.id, category.title, category.description].join("\u{1f}"));
        hasher.update(b"\x1e");
    }
    hasher.update(b"\x1d");
    for achievement in &ACHIEVEMENTS {
        hasher.update(
            [
                achievement.id,
                achievement.name,
                achievement.description,
                achievement.category,
                achievement.inspiration,
                if achievement.hidden { "1" } else { "0" },
            ]
            .join("\u{1f}"),
        );
        hasher.update(b"\x1e");
    }
    hex::encode(hasher.finalize())
}

#[test]
fn the_generated_catalog_matches_the_python_source_digest() {
    assert_eq!(catalog_digest(), CATALOG_DIGEST);
    assert_eq!(ACHIEVEMENTS.len(), 184);
    assert_eq!(CATEGORIES.len(), 7);
}

#[test]
fn test_achievement_copy_is_allusive_and_speech_sized() {
    for achievement in &ACHIEVEMENTS {
        let (artist, _title) = achievement
            .inspiration
            .split_once(" - ")
            .unwrap_or_else(|| panic!("{} has no artist - title", achievement.id));
        let visible = format!("{} {}", achievement.name, achievement.description).to_lowercase();
        assert!(!achievement.description.contains('\n'));
        let length = achievement.description.chars().count();
        assert!((40..=140).contains(&length), "{}: {length}", achievement.id);
        assert!(
            achievement.description.matches('.').count() >= 1,
            "{}",
            achievement.id
        );
        assert!(!achievement.description.contains('"'), "{}", achievement.id);
        assert!(!achievement.inspiration.is_empty());
        // Artists stay out of player-facing text. Song titles are allowed to
        // appear (many are just place names, like Jackson or Abilene); the
        // no-lyrics rule lives in the catalog's copy note.
        assert!(
            !visible.contains(&artist.to_lowercase()),
            "{}: {artist}",
            achievement.id
        );
    }

    let mut profile = TestProfile::default();
    let message = award(&mut profile, ACHIEVEMENTS[0].id).unwrap().message;
    assert!(message.normal.starts_with("New achievement!"));
}

#[test]
fn test_catalog_tops_one_hundred_with_unique_ids() {
    let ids: Vec<&str> = ACHIEVEMENTS.iter().map(|a| a.id).collect();
    let unique: HashSet<&str> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len());
    assert!(ids.len() > 100);
}

#[test]
fn test_every_achievement_has_exactly_one_valid_category() {
    for achievement in &ACHIEVEMENTS {
        assert!(
            category_by_id(achievement.category).is_some(),
            "{}",
            achievement.id
        );
    }
}

#[test]
fn test_every_category_is_non_empty() {
    for category in &CATEGORIES {
        assert!(
            !achievements_in_category(category.id).is_empty(),
            "{}",
            category.id
        );
    }
}

#[test]
fn test_category_copy_is_speech_sized_and_quote_free() {
    let mut seen_ids = HashSet::new();
    for category in categories() {
        assert!(seen_ids.insert(category.id), "category ids must be unique");
        assert!(!category.title.contains('\n'));
        assert!(!category.description.contains('\n'));
        assert!(!category.title.contains('"'));
        assert!(!category.description.contains('"'));
        assert!(!category.title.is_empty());
        let length = category.description.chars().count();
        assert!((20..=120).contains(&length), "{}", category.id);
    }
}

#[test]
fn test_hidden_achievement_keeps_its_secret_until_earned() {
    let hidden: Vec<&Achievement> = ACHIEVEMENTS.iter().filter(|a| a.hidden).collect();
    // the deep-cuts bucket carries at least one hidden badge
    assert!(!hidden.is_empty());
    for achievement in hidden {
        let (locked_name, locked_detail) = entry_text(achievement, false);
        assert_eq!(locked_name, HIDDEN_NAME);
        assert_eq!(locked_detail, HIDDEN_HELP);
        let (earned_name, earned_detail) = entry_text(achievement, true);
        assert_eq!(earned_name, achievement.name);
        assert_eq!(earned_detail, achievement.description);
    }
}

#[test]
fn test_non_hidden_locked_achievement_speaks_its_own_title() {
    let visible = ACHIEVEMENTS.iter().find(|a| !a.hidden).unwrap();
    let (name, detail) = entry_text(visible, false);
    assert_eq!(name, visible.name);
    assert_eq!(detail, visible.description);
}

#[test]
fn test_increment_stat_counts_and_survives_bad_values() {
    let mut profile = TestProfile::default();
    assert_eq!(increment_stat(&mut profile, "inspections_passed"), 1);
    assert_eq!(increment_stat(&mut profile, "inspections_passed"), 2);
    profile
        .achievement_stats
        .insert("inspections_passed".into(), json!("corrupt"));
    assert_eq!(int_stat(&mut profile, "inspections_passed"), 0);
    assert_eq!(increment_stat(&mut profile, "inspections_passed"), 1);
}

#[test]
fn int_stat_reads_the_json_shapes_python_int_accepts() {
    let mut profile = TestProfile::default();
    for (value, expected) in [
        (json!(7), 7),
        (json!(3.9), 3),
        (json!(true), 1),
        (json!("12"), 12),
        (json!(" 12 "), 12),
        (json!("1_000"), 1000),
        (json!("3.5"), 0),
        (json!(null), 0),
        (json!([1]), 0),
        (json!({"a": 1}), 0),
    ] {
        profile.achievement_stats.insert("k".into(), value.clone());
        assert_eq!(int_stat(&mut profile, "k"), expected, "{value}");
    }
    assert_eq!(int_stat(&mut profile, "missing"), 0);
}

#[test]
fn list_stats_normalise_to_strings_and_count_unique_values() {
    let mut profile = TestProfile::default();
    profile
        .achievement_stats
        .insert("states".into(), json!(["Ohio", 7, true, null, 2.5]));
    assert_eq!(
        list_stat(&mut profile, "states"),
        vec!["Ohio", "7", "True", "None", "2.5"]
    );
    // The normalised list is written back, as the Python list_stat did.
    assert_eq!(
        profile.achievement_stats["states"],
        json!(["Ohio", "7", "True", "None", "2.5"])
    );
    assert_eq!(add_unique_stat(&mut profile, "states", "Ohio"), 5);
    assert_eq!(add_unique_stat(&mut profile, "states", "Indiana"), 6);
    profile
        .achievement_stats
        .insert("states".into(), json!("corrupt"));
    assert_eq!(list_stat(&mut profile, "states"), Vec::<String>::new());
    assert_eq!(add_unique_stat(&mut profile, "states", "Ohio"), 1);
}

#[test]
fn bool_stats_follow_python_truthiness() {
    let mut profile = TestProfile::default();
    assert!(!bool_stat(&mut profile, "seen"));
    set_bool_stat(&mut profile, "seen");
    assert!(bool_stat(&mut profile, "seen"));
    profile.achievement_stats.insert("seen".into(), json!(0));
    assert!(!bool_stat(&mut profile, "seen"));
    profile
        .achievement_stats
        .insert("seen".into(), json!("yes"));
    assert!(bool_stat(&mut profile, "seen"));
    reset_stat(&mut profile, "streak");
    assert_eq!(profile.achievement_stats["streak"], json!(0));
}

#[test]
fn award_records_once_and_refuses_an_unknown_id() {
    let mut profile = TestProfile::default();
    let first = award(&mut profile, "first_delivery").unwrap();
    assert_eq!(first.achievement.id, "first_delivery");
    assert_eq!(
        first.message,
        achievement_unlocked(first.achievement.name, first.achievement.description)
    );
    assert!(award(&mut profile, "first_delivery").is_none());
    assert_eq!(profile.achievements, vec!["first_delivery"]);
    assert_eq!(
        earned_ids(&profile),
        HashSet::from(["first_delivery".to_string()])
    );
    assert!(award(&mut profile, "no_such_badge").is_none());
}

#[test]
fn test_retired_first_run_badges_still_export_to_the_cloud_validator() {
    // Deleting an id breaks the allow-list for anyone who already earned it.
    // first_dispatch, first_pickup, and air_ready are retired as awards but
    // keep their catalog entries and ids for exactly this reason. (The
    // second half -- that the invariant bundle's achievementIds carries
    // them -- lives with profile_integrity_invariants, which takes the id
    // list as an input.)
    for badge_id in ["first_dispatch", "first_pickup", "air_ready"] {
        assert!(achievement_by_id(badge_id).is_some(), "{badge_id}");
    }
}

#[test]
fn test_the_funny_ones_are_actually_in_the_catalog() {
    // They are jokes, but they are shipped jokes and they follow the copy rules.
    for badge_id in [
        "sixty_nine_mph",
        "eighty_eight_mph",
        "fifty_five_mph",
        "ten_four_day",
        "sixteen_tons",
        "brake_smoke",
        "one_for_the_road",
    ] {
        let badge = achievement_by_id(badge_id).unwrap();
        // artist and title, both named
        assert!(badge.inspiration.matches(" - ").count() >= 1);
        assert!(!badge.description.trim().is_empty());
    }

    // The two jokes that turn on a specific number keep their number: the
    // copy is the whole badge, and a reworded one stops being the joke.
    let double_nickel = achievement_by_id("fifty_five_mph").unwrap();
    assert!(double_nickel.hidden);
    // The whole badge is the wink: the game it is winking at, and the number
    // it sends you to instead -- which is the badge sitting next to it.
    assert!(double_nickel.description.contains("Jim Kitchen"));
    assert!(double_nickel.description.contains("88"));
    let ten_four = achievement_by_id("ten_four_day").unwrap();
    assert!(ten_four.hidden);
    assert!(ten_four.description.contains("fourth of October"));
    assert!(ten_four.description.contains("ten-four"));
    // And it alludes to its own song, which is how the catalog talks. Owner
    // ruling while it was being written: a fresh one, by an artist the
    // catalog has not leaned on already -- Convoy is cited twice elsewhere
    // and Dave Dudley three times over, and a wink shared with three other
    // badges is not a wink.
    assert!(ten_four.description.contains("white knight"));
    let (artist, _) = ten_four.inspiration.split_once(" - ").unwrap();
    assert_eq!(
        ACHIEVEMENTS
            .iter()
            .filter(|badge| badge.inspiration.starts_with(artist))
            .count(),
        1,
        "{} shares its artist with another badge",
        ten_four.id
    );
}

#[test]
fn test_every_badge_cites_a_song() {
    // The catalog's whole voice rests on this; a new one must not break it.
    for badge in &ACHIEVEMENTS {
        let (artist, title) = badge.inspiration.split_once(" - ").unwrap();
        assert!(
            !artist.trim().is_empty() && !title.trim().is_empty(),
            "{}",
            badge.id
        );
    }
}

// -- tests/test_career_arc_achievements.py --------------------------------------

#[test]
fn test_new_arc_badges_exist_with_song_inspirations() {
    for badge_id in [
        "level_five",
        "level_ten",
        "level_fifteen",
        "level_twenty_five",
        "level_thirty",
        "fleet_upgrade",
        "fleet_flagship",
        "owner_operator_buyin",
        "authority_active",
        "three_trucks",
        "twenty_five_cities",
        "seventy_five_cities",
        "hundred_fifty_cities",
        "fifteen_states",
        "thirty_states",
        "dakota_delivery",
        "montana_delivery",
        "new_england_delivery",
        "self_paid_course",
    ] {
        assert!(achievement_by_id(badge_id).is_some(), "{badge_id}");
    }
    assert!(ACHIEVEMENTS.len() >= 130);
}

// -- tests/test_achievement_flavor_relocated.py ---------------------------------

#[test]
fn test_live_announce_is_name_only_in_both_modes() {
    let pair = achievement_unlocked("Night Owl", "You ran the small hours and kept it clean.");
    let live = achievement_announced("Night Owl");
    // The stored record keeps the flavor for the log and the menu...
    assert!(pair.normal.contains("kept it clean"));
    // ...but the live announce never speaks it, in either speech mode.
    assert_eq!(live.normal, "New achievement! Night Owl.");
    assert_eq!(live.render(true), "Night Owl.");
    assert!(!live.normal.contains("kept it clean"));
}

// -- App()-bound: moved to crates/freight-fate/tests/app_achievements.rs ----------
//
// Everything `tests/test_achievements.py` drove through `App()` --
// `award_achievement`, the delivery settlement's badges, the badge trackers at
// the wheel, and the main-menu achievements screens -- now runs live in
// `crates/freight-fate/tests/app_achievements.rs`. It cannot run here:
// `ff-core` does not depend on the game crate, so the eighteen cases that used
// to sit below as `#[ignore]`d stubs were placeholders that could never have
// been filled in this file.
