//! AG Now launches every session with `AGNOW=1` and updates the game
//! itself, so the in-game updater reads that variable and stands down.

use std::ffi::OsStr;

use freight_fate::updater::agnow_value_set;

#[test]
fn agnow_one_turns_the_updater_off() {
    assert!(agnow_value_set(Some(OsStr::new("1"))));
    assert!(agnow_value_set(Some(OsStr::new(" 1 "))));
    assert!(agnow_value_set(Some(OsStr::new("true"))));
}

#[test]
fn agnow_missing_empty_or_zero_leaves_the_updater_on() {
    assert!(!agnow_value_set(None));
    assert!(!agnow_value_set(Some(OsStr::new(""))));
    assert!(!agnow_value_set(Some(OsStr::new("  "))));
    assert!(!agnow_value_set(Some(OsStr::new("0"))));
}
