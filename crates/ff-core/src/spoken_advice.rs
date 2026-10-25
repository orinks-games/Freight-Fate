//! Retiring instructions the player has demonstrated (research doc R7; port
//! of `freight_fate/spoken_advice.py`).
//!
//! Some spoken prompts teach a control the game names on every wheel entry or
//! every stop callout, forever: "Press E to start the engine", "F1 lists the
//! controls", "Press X to signal for the exit". Once a player has done the
//! thing a few times the instruction is noise, and the pull-model query keys
//! keep the information reachable, so retiring the auto-nag loses nothing.
//!
//! Retirement is gated on a small persisted counter, keyed to the *control
//! binding it was earned under*, so a remapped key re-teaches the new hint
//! afresh -- the standing rule that spoken advice never names a control the
//! current settings do not give this driver, cutting both ways (never name a
//! wrong key, never go silent where the key is now different). The binding is
//! captured as the hint phrase itself: change the device, or remap the key,
//! and the phrase changes, which changes the key, which resets the count to
//! zero.
//!
//! The transmission used to be part of the key too, so a switch to manual
//! re-taught "Press X to signal for the exit" three more times although no
//! hint's key or wording differs between the two (owner, 2026-10-25). Counts
//! saved under the old per-transmission keys still count.
//!
//! The counters live in the profile's `achievement_stats` dict (already
//! persisted and general-purpose) under a `hint:` namespace, so no new save
//! field is needed.

use crate::achievements::{int_stat, AchievementProfile};
use serde_json::Value;

/// Spoken until the player has performed the action this many times under the
/// current binding and transmission. Small: three unremarkable repetitions is
/// enough to show the control is known.
pub const RETIRE_AFTER: i64 = 3;

fn counter_key(action: &str, binding: &str) -> String {
    format!("hint:{action}:{binding}")
}

/// The count so far, including one saved under the retired
/// per-transmission keys.
fn demonstrated(profile: &mut dyn AchievementProfile, action: &str, binding: &str) -> i64 {
    let key = counter_key(action, binding);
    ["", ":auto", ":manual"]
        .iter()
        .map(|suffix| int_stat(profile, &format!("{key}{suffix}")))
        .max()
        .unwrap_or(0)
}

/// Whether `action`'s spoken instruction has been earned into silence for the
/// current `binding`.
pub fn instruction_retired(
    profile: &mut dyn AchievementProfile,
    action: &str,
    binding: &str,
) -> bool {
    demonstrated(profile, action, binding) >= RETIRE_AFTER
}

/// Record that the player just performed `action` under this binding.
/// Returns the new count. Stops counting past the retirement threshold so
/// the stored number never runs away.
pub fn note_demonstrated(profile: &mut dyn AchievementProfile, action: &str, binding: &str) -> i64 {
    let count = (demonstrated(profile, action, binding) + 1).min(RETIRE_AFTER);
    profile
        .achievement_stats_mut()
        .insert(counter_key(action, binding), Value::from(count));
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::profile::Profile;

    #[test]
    fn a_hint_retires_after_three_demonstrations_under_one_binding() {
        let mut p = Profile::named("Hints");
        assert!(!instruction_retired(&mut p, "engine_start", "Press E"));
        for n in 1..=3 {
            assert_eq!(note_demonstrated(&mut p, "engine_start", "Press E"), n);
        }
        assert!(instruction_retired(&mut p, "engine_start", "Press E"));
        // The counter never runs away past the threshold.
        assert_eq!(
            note_demonstrated(&mut p, "engine_start", "Press E"),
            RETIRE_AFTER
        );
        assert_eq!(p.achievement_stats["hint:engine_start:Press E"], 3);
        // A remapped key re-teaches afresh.
        assert!(!instruction_retired(&mut p, "engine_start", "Press Q"));
    }

    #[test]
    fn a_count_saved_under_one_transmission_still_counts() {
        // Counts used to be kept per transmission; a hint the player retired
        // on the automatic stays retired, and a part count carries on.
        let mut p = Profile::named("Hints");
        p.achievement_stats
            .insert("hint:take_exit:X:auto".to_string(), Value::from(3));
        assert!(instruction_retired(&mut p, "take_exit", "X"));
        p.achievement_stats
            .insert("hint:engine:E:manual".to_string(), Value::from(2));
        assert!(!instruction_retired(&mut p, "engine", "E"));
        assert_eq!(note_demonstrated(&mut p, "engine", "E"), 3);
        assert!(instruction_retired(&mut p, "engine", "E"));
    }
}
