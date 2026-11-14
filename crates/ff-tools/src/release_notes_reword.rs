//! Telling a reworded changelog bullet from a new one.
//!
//! A snapshot lists the bullets that were not published before. Matching on
//! exact text makes every reword look new: the 2026-11-05 text pass renamed
//! a setting inside the PR 512 bullet, and that night's snapshot announced
//! it again along with five other reworded bullets. A bullet now counts as
//! published when most of its words match a published one.

use std::collections::HashSet;

/// Word overlap (shared words over all words) at or above which two bullets
/// are one change reworded. Calibrated on that text pass, against the
/// changelog at the previous snapshot: the six bullets it only reworded
/// scored 0.76 to 0.96 against their old text, the bullet that merged an
/// old fix with a new one scored 0.54, and none of the 24 bullets new in
/// that snapshot scored above 0.24 against anything published.
pub const REWORD_THRESHOLD: f64 = 0.6;

fn words(normalized: &str) -> HashSet<String> {
    normalized
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_string)
        .collect()
}

/// Share of words two bullets have in common, from 0 to 1.
pub fn overlap(a: &HashSet<String>, b: &HashSet<String>) -> f64 {
    let union = a.union(b).count();
    if union == 0 {
        return 0.0;
    }
    a.intersection(b).count() as f64 / union as f64
}

/// Bullets already published, normalized by `normalize_entry`.
pub struct Published {
    exact: HashSet<String>,
    words: Vec<(String, HashSet<String>)>,
}

impl Published {
    pub fn new(entries: HashSet<String>) -> Self {
        let words = entries.iter().map(|e| (e.clone(), words(e))).collect();
        Self {
            exact: entries,
            words,
        }
    }

    /// The published bullet `normalized` repeats: itself when the text is
    /// unchanged, or the closest reword at or above the threshold.
    pub fn match_for(&self, normalized: &str) -> Option<&str> {
        if let Some(same) = self.exact.get(normalized) {
            return Some(same);
        }
        let mine = words(normalized);
        self.words
            .iter()
            .map(|(text, theirs)| (overlap(&mine, theirs), text))
            .filter(|(score, _)| *score >= REWORD_THRESHOLD)
            .max_by(|a, b| a.0.total_cmp(&b.0))
            .map(|(_, text)| text.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn published(entries: &[&str]) -> Published {
        Published::new(entries.iter().map(|e| e.to_string()).collect())
    }

    #[test]
    fn a_renamed_setting_is_still_the_same_bullet() {
        let old = "a menu letter can now choose its item at once. turn on menu accelerator keys in settings, gameplay, controls; a letter two items share still only moves the selection. thanks to trssharp (pr 512).";
        let new = "a menu letter can now choose its item at once. turn on letters run menu items in settings, gameplay, controls; a letter two items share still only moves the selection. thanks to trssharp (pr 512).";
        assert_eq!(published(&[old]).match_for(new), Some(old));
    }

    #[test]
    fn a_new_bullet_on_the_same_subject_is_new() {
        let old = "adaptive cruise now slows for a lower speed limit in time. on the relaxed and standard pace it reached the sign still over the limit.";
        let new = "cruise no longer crawls toward an exit for a lower limit miles ahead. it keeps the road's limit until it needs to slow for the drop.";
        assert_eq!(published(&[old]).match_for(new), None);
    }

    #[test]
    fn unchanged_text_matches_exactly() {
        assert_eq!(published(&["- same"]).match_for("- same"), Some("- same"));
        assert_eq!(published(&[]).match_for("anything"), None);
    }
}
