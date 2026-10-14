//! Ported from the `Settings`-only parts of `tests/test_settings_menu.py`,
//! `tests/test_driving_speech_ladder.py`, `tests/test_metric_units.py`,
//! `tests/test_metric_readouts.py`, `tests/test_models.py`,
//! `tests/test_lane_keeping.py`, `tests/test_pedal_latch.py`,
//! `tests/test_village_callouts.py`, `tests/test_roadside_chatter.py`,
//! `tests/test_lane_guide_tone.py`, `tests/test_updater.py` and
//! `tests/test_radio.py`. The `isolated_data_dir` fixture is
//! [`with_data_dir`]: a tempdir in `FREIGHT_FATE_DATA_DIR`, under the
//! process-wide env lock.

// The Python tests build `Settings()` and assign fields one by one; the
// ported bodies keep that shape so they read against the originals.
#![allow(clippy::field_reassign_with_default)]

use std::path::Path;

use serde_json::{json, Map, Value};

use super::paths::ENV_LOCK;
use super::*;
use crate::speech_pacing::{Disposition, SpeechCategory};

/// `isolated_data_dir`: run `body` with settings pointed at a fresh tempdir.
///
/// The directory is pinned on THIS THREAD rather than in the process
/// environment, so these cases are isolated from each other without being
/// serialised against each other. The read guard is still taken because that
/// isolation only holds while nobody rewrites the process environment
/// underneath it, and a couple of cases elsewhere in the crate do exactly
/// that -- they take the write guard and get the process to themselves.
fn with_data_dir<T>(body: impl FnOnce(&Path) -> T) -> T {
    let _guard = ENV_LOCK.read().unwrap_or_else(|e| e.into_inner());
    let tmp = tempfile::tempdir().unwrap();
    let data = tmp.path().join("data");
    let previous = paths::set_thread_data_dir(Some(data.clone()));
    let result = body(&data);
    paths::set_thread_data_dir(previous);
    result
}

fn write_settings_file(text: &str) {
    let path = Settings::path();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

fn from_json(value: Value) -> Settings {
    let Value::Object(map) = value else {
        panic!("a JSON object")
    };
    Settings::from_dict(Some(&map))
}

fn metric() -> Settings {
    let mut s = Settings::default();
    s.imperial_units = false;
    s
}

fn imperial() -> Settings {
    let mut s = Settings::default();
    s.imperial_units = true;
    s
}

#[test]
fn old_stopping_toggles_migrate_to_the_one_facility_assist() {
    for old in [
        json!({"driving_assistance_preset": "custom", "destination_approach_assist": true}),
        json!({"driving_assistance_preset": "custom", "selected_stop_assist": true}),
        json!({"destination_approach_assist": true}),
        json!({"selected_stop_assist": true}),
    ] {
        let migrated = from_json(old);
        assert!(migrated.destination_approach_assist);
        assert!(!migrated.selected_stop_assist);
    }
}

// -- the field table -----------------------------------------------------------

#[test]
fn the_struct_carries_the_persisted_fields_in_python_order() {
    // 73 came over from the Python dataclass; backup_announcements,
    // duty_notifications and braille_only (2026-09-02) and real_fuel_prices
    // (2026-09-12), the two shortcut tables (2026-09-14),
    // radio_shuffle_playlists and steering_guide_inverted (2026-09-18), and
    // synth_music and music_seed (2026-09-21), touch_bindings (2026-09-29)
    // and assist_preset_chosen (2026-09-30) were added on the Rust side;
    // lane_centering_assist was retired for 1.9.
    assert_eq!(Settings::FIELD_NAMES.len(), 86);
    assert_eq!(Settings::FIELD_NAMES[0], "online_services");
    assert_eq!(Settings::FIELD_NAMES[80], "settings_layout_notice_from");
    let pairs = Settings::default().ordered_values();
    assert_eq!(pairs.len(), 86);
    for ((name, _), field) in pairs.iter().zip(Settings::FIELD_NAMES) {
        assert_eq!(name, field);
    }
    // lane_keeping_unreadable is a ClassVar in Python: never persisted.
    assert!(!Settings::FIELD_NAMES.contains(&"lane_keeping_unreadable"));
    assert!(Settings::default()
        .field_value("lane_keeping_unreadable")
        .is_none());
}

#[test]
fn the_defaults_match_the_python_dataclass() {
    let s = Settings::default();
    let expected: Value = serde_json::from_str(
        r#"{
        "online_services": true, "imperial_units": true, "engine_voice": "real",
        "jake_voice": "real", "acc_following_gap": "normal", "automatic_transmission": true,
        "automatic_direction_changes": "simple", "time_scale": 10.0,
        "pace_retired_notice_left": 0, "real_weather": false, "real_traffic": false,
        "real_parking": false, "real_fuel_prices": true,
        "live_weather_controls_calendar": true,
        "hos_mode": "realistic", "hos_planning_hints": false,
        "lane_keeping": "full", "lane_keeping_rename_notice_left": 0,
        "lane_cue_loudness": "standard", "lane_guide_tone": false,
        "driving_assistance_preset": "all", "automatic_emergency_braking": true,
        "lane_departure_warning": true, "stop_and_go_assist": true,
        "descent_speed_control": "interactive",
        "exit_speed_assist": true, "destination_approach_assist": true,
        "selected_stop_assist": false, "curve_speed_assist": true,
        "route_transition_assist": true, "speed_keeper": true, "predictive_cruise": true,
        "pedal_latch": "on", "curve_callouts": true, "master_volume": 1.0,
        "sfx_volume": 0.8, "music_volume": 0.5, "synth_music": false, "music_seed": 48213,
        "radio_volume": 0.25, "radio_enabled": true,
        "radio_station_id": "route_playlist", "radio_streamer_safe": false,
        "radio_shuffle_playlists": false,
        "weather_volume": 0.65, "engine_volume": 0.55, "ui_volume": 0.9,
        "duck_audio_for_speech": false, "driving_speech": "standard", "chatter_parks": true,
        "chatter_rivers": true, "chatter_passes": true, "chatter_museums": true,
        "chatter_billboards": true, "place_callouts": "sparse",
        "announce_menu_position": true, "backup_announcements": "every",
        "sapi_events": true, "event_backend": "SAPI", "braille_only": false,
        "speech_rate": 0.5, "speech_pitch": 0.5, "speech_volume": 1.0, "speech_voice": "",
        "update_channel": "", "skipped_update": "", "discord_presence": true,
        "online_presence": false, "remote_dispatch_calls": false,
        "duty_notifications": false,
        "profile_sharing_consent_version": 0,
        "profile_sharing_pending_off": false, "cloud_saves": false,
        "mastodon_sharing": false, "mastodon_linked": false, "mastodon_linked_handle": "",
        "controller_enabled": true, "haptics_enabled": true, "online_offer_seen": false,
        "settings_version": 3, "settings_layout_notice_from": -1,
        "key_bindings": "", "pad_bindings": "", "touch_bindings": "",
        "steering_guide_inverted": false, "assist_preset_chosen": false
    }"#,
    )
    .unwrap();
    let Value::Object(expected) = expected else {
        unreachable!()
    };
    assert_eq!(expected.len(), 86);
    for (name, value) in s.ordered_values() {
        assert_eq!(Some(&value), expected.get(name), "{name}");
    }
    assert!(!s.lane_keeping_unreadable);
}

#[test]
fn missing_remote_dispatch_setting_defaults_off() {
    assert!(!Settings::default().remote_dispatch_calls);
    assert!(!from_json(json!({"online_presence": true})).remote_dispatch_calls);
}

#[test]
fn hos_planning_hints_default_off_and_round_trip_as_a_boolean() {
    assert!(!Settings::default().hos_planning_hints);
    let mut enabled = Settings::default();
    enabled.hos_planning_hints = true;
    let stored: Value = serde_json::from_str(&enabled.to_file_text()).unwrap();
    assert_eq!(stored["hos_planning_hints"], true);
    assert!(from_json(stored).hos_planning_hints);
    assert!(!from_json(json!({"hos_planning_hints": "true"})).hos_planning_hints);
}

#[test]
fn the_file_text_is_what_json_dump_wrote() {
    let s = Settings::default();
    let text = s.to_file_text();
    assert!(text.starts_with("{\n  \"online_services\": true,\n  \"imperial_units\": true,\n"));
    assert!(text.ends_with(
        "  \"pad_bindings\": \"\",\n  \"touch_bindings\": \"\",\n  \"steering_guide_inverted\": false,\n  \
         \"assist_preset_chosen\": false,\n  \"steering_assist\": \"off\"\n}"
    ));
    assert!(text.contains("\n  \"time_scale\": 10.0,\n"));
    assert!(text.contains("\n  \"radio_volume\": 0.25,\n"));
    // ensure_ascii: a non-ASCII voice name is escaped the way Python wrote it.
    let mut s = Settings::default();
    s.speech_voice = "Zoë \"quoted\" \\ tab\t 😀".to_string();
    assert!(s
        .to_file_text()
        .contains("\"speech_voice\": \"Zo\\u00eb \\\"quoted\\\" \\\\ tab\\t \\ud83d\\ude00\""));
    // ...and reads back as itself.
    let reloaded = from_json(serde_json::from_str(&s.to_file_text()).unwrap());
    assert_eq!(reloaded.speech_voice, s.speech_voice);
}

#[test]
fn from_dict_none_and_empty_dict_are_different_things() {
    // No readable file: every default stands, preset row says realistic.
    let fresh = Settings::from_dict(None);
    assert_eq!(fresh, Settings::default());
    assert_eq!(fresh.settings_layout_notice_from, -1);
    // A file that exists but says nothing: pre-preset, pre-rename, pre-reorg.
    let empty = Settings::from_dict(Some(&Map::new()));
    assert_eq!(empty.lane_keeping, "full");
    assert_eq!(empty.driving_assistance_preset, "custom");
    assert!(!empty.automatic_emergency_braking);
    assert_eq!(empty.descent_speed_control, "off");
    assert_eq!(empty.settings_layout_notice_from, 0);
    assert_eq!(empty.lane_keeping_rename_notice_left, 0);
    assert!(!empty.lane_keeping_unreadable);
}

#[test]
fn unknown_keys_are_ignored_and_wrong_shapes_take_the_python_fallbacks() {
    let s = from_json(json!({
        "no_such_setting": 1,
        "imperial_units": 0,
        "online_services": "yes",
        "engine_voice": 5,
        "hos_mode": null,
        "lane_keeping": 5,
        "driving_assistance_preset": "custom",
        "lane_cue_loudness": ["subtle"],
        "descent_speed_control": 7,
        "acc_following_gap": true,
        "pedal_latch": 3,
        "event_backend": "",
        "radio_station_id": 12,
        "mastodon_linked_handle": 4,
        "profile_sharing_consent_version": 3.0,
        "online_presence": true,
        "time_scale": "40",
        "pace_retired_notice_left": 2.0,
        "lane_keeping_rename_notice_left": true,
        "settings_layout_notice_from": "1",
        "controller_enabled": 1,
        "cloud_saves": "true",
    }));
    assert!(!s.imperial_units); // truthiness of 0
    assert!(s.online_services); // truthiness of "yes"
    assert_eq!(s.engine_voice, "real");
    assert_eq!(s.hos_mode, "realistic");
    assert_eq!(s.lane_keeping, "full");
    assert!(s.lane_keeping_unreadable);
    assert!(!s.lane_departure_warning);
    assert_eq!(s.lane_cue_loudness, "standard");
    assert_eq!(s.descent_speed_control, "realistic");
    assert_eq!(s.acc_following_gap, "normal");
    assert_eq!(s.pedal_latch, "on");
    assert_eq!(s.event_backend, "SAPI");
    assert_eq!(s.radio_station_id, "route_playlist");
    assert_eq!(s.mastodon_linked_handle, "");
    // 3.0 == 3 in Python, so the consent stands and online_presence survives.
    assert_eq!(s.profile_sharing_consent_version, 3);
    assert!(s.online_presence);
    assert_eq!(s.time_scale, 10.0);
    assert_eq!(s.pace_retired_notice_left, 0);
    assert_eq!(s.lane_keeping_rename_notice_left, 0);
    assert_eq!(s.settings_layout_notice_from, 0); // reset to -1, then the missing version owes 0
    assert!(s.controller_enabled);
    assert!(!s.cloud_saves);
    assert_eq!(s.settings_version, SETTINGS_VERSION);
}

#[test]
fn consent_version_mismatch_forces_profile_sharing_off() {
    assert!(!from_json(json!({"online_presence": true})).online_presence);
    assert!(
        !from_json(json!({"online_presence": true, "profile_sharing_consent_version": 2}))
            .online_presence
    );
    assert!(
        from_json(json!({"online_presence": true, "profile_sharing_consent_version": 3}))
            .online_presence
    );
}

#[test]
fn notice_counters_clamp_to_their_budgets() {
    let s =
        from_json(json!({"pace_retired_notice_left": 9, "lane_keeping_rename_notice_left": -4}));
    assert_eq!(s.pace_retired_notice_left, PACE_RETIRED_NOTICES);
    assert_eq!(s.lane_keeping_rename_notice_left, 0);
}

#[test]
fn levels_coerce_strings_and_clamp() {
    let s = from_json(json!({
        "master_volume": "0.3", "sfx_volume": 2, "music_volume": -1.0,
        "radio_volume": " 0.5 ", "weather_volume": "1_0", "engine_volume": "loud",
    }));
    assert_eq!(s.master_volume, 0.3);
    assert_eq!(s.sfx_volume, 1.0);
    assert_eq!(s.music_volume, 0.0);
    assert_eq!(s.radio_volume, 0.5);
    assert_eq!(s.weather_volume, 1.0);
    assert_eq!(s.engine_volume, Settings::default().engine_volume);
}

#[test]
fn settings_layout_notice_keeps_the_oldest_owed_version() {
    // A player two layouts behind who already owed version 1 keeps 1.
    let s = from_json(json!({"settings_version": 2, "settings_layout_notice_from": 1}));
    assert_eq!(s.settings_layout_notice_from, 1);
    // A file without the key owes 0 even if it claimed a notice already spoken.
    let s = from_json(json!({"settings_layout_notice_from": -1}));
    assert_eq!(s.settings_layout_notice_from, 0);
    // test_a_player_one_layout_behind_hears_only_the_newer_notice (load half)
    assert_eq!(
        from_json(json!({"settings_version": 1})).settings_layout_notice_from,
        1
    );
    // test_a_player_two_layouts_behind_hears_the_driving_speech_notice (load half)
    assert_eq!(
        from_json(json!({"settings_version": 2})).settings_layout_notice_from,
        2
    );
    // test_gameplay_reorg_notice_fires_once_for_a_pre_reorg_settings_file (load half)
    assert_eq!(
        from_json(json!({"imperial_units": true})).settings_layout_notice_from,
        0
    );
}

#[test]
fn set_field_and_field_value_round_trip_by_name() {
    let mut s = Settings::default();
    assert!(s.set_field("imperial_units", &json!(false)));
    assert!(!s.imperial_units);
    assert_eq!(s.field_value("imperial_units"), Some(json!(false)));
    assert!(!s.set_field("speech_verbosity", &json!(0)));
    assert_eq!(s.field_value("speech_verbosity"), None);
}

// -- tests/test_settings_menu.py -------------------------------------------------

#[test]
fn test_invalid_automatic_direction_setting_falls_back_to_simple() {
    with_data_dir(|_| {
        write_settings_file(r#"{"automatic_direction_changes": "mystery"}"#);
        assert_eq!(Settings::load().automatic_direction_changes, "simple");
    });
}

#[test]
fn test_invalid_jake_voice_setting_falls_back_to_real() {
    with_data_dir(|_| {
        write_settings_file(r#"{"jake_voice": "mystery"}"#);
        assert_eq!(Settings::load().jake_voice, "real");
    });
}

#[test]
fn test_driving_assistance_presets_apply_complete_mappings() {
    let mut settings = Settings::default();
    for (preset, expected) in DRIVING_ASSIST_PRESETS {
        assert!(settings.apply_driving_assistance_preset(preset));
        assert_eq!(settings.assist_values(), expected);
        for (field, value) in DRIVING_ASSIST_FIELDS.iter().zip(expected) {
            assert_eq!(settings.assist_value(field), Some(value));
        }
        assert_eq!(settings.driving_assistance_preset, preset);
    }
    assert!(!settings.apply_driving_assistance_preset("no such preset"));
}

#[test]
fn test_a_fresh_install_is_the_all_assists_preset() {
    // The shipped defaults ARE a named preset, and the row says so -- the
    // point of the 2026-08-09 ruling, which the preset must keep honouring
    // whichever preset that is. For months the row read "Realistic" while
    // lane keeping was fully automated, because the preset could not see that
    // field, so a player reading the row believed something false.
    //
    // It moved from Realistic to Balanced on 2026-09-18, when the lane model
    // grew a heading: "off" stopped being a mild drift and became a
    // continuous driving task, so shipping it would hand every new driver a
    // job they cannot yet do. Partial steers for the error and still leaves
    // them something to feel.
    //
    // And to All assists on 2026-09-30: too many first drives still went
    // wrong at the wheel, street corners above all, so the truck steers until
    // the driver chooses to.
    let mut settings = Settings::default();
    assert_eq!(settings.lane_keeping, "full");
    assert!(settings.lane_is_automated());
    assert_eq!(settings.driving_assistance_preset, "all");
    assert_eq!(settings.refresh_driving_assistance_preset(), "all");
}

#[test]
fn test_an_unreadable_lane_value_still_falls_back_to_full_not_the_default() {
    // A corrupt value is not a fresh install. We do not know what the player
    // chose, so the fallback stays the most assisted mode and announces
    // itself -- handing a blind player a manual steering task they never
    // opted into is the worse failure. A new career, by contrast, gets the
    // documented default.
    assert_eq!(LANE_KEEPING_FALLBACK, "full");
}

#[test]
fn test_every_preset_owns_lane_keeping() {
    // Lane keeping used to sit outside the presets, so All assists forced it
    // and no other preset could hand it back. Each preset now names a value.
    let mut settings = Settings::default();
    for (preset, expected) in [
        ("all", "full"),
        ("balanced", "partial"),
        ("realistic", "off"),
    ] {
        settings.apply_driving_assistance_preset(preset);
        assert_eq!(settings.lane_keeping, expected);
        assert_eq!(settings.driving_assistance_preset, preset);
    }
}

#[test]
fn test_the_preset_row_can_no_longer_lie_about_lane_keeping() {
    // Regression: the row read "Realistic" over fully automated lane keeping
    // -- the one row a player checks to learn how much the truck is doing
    // could not see the biggest thing it was doing.
    let mut settings = Settings::default();
    settings.apply_driving_assistance_preset("realistic");
    assert_eq!(settings.refresh_driving_assistance_preset(), "realistic");
    settings.lane_keeping = "full".to_string();
    assert_eq!(settings.refresh_driving_assistance_preset(), "custom");
    settings.apply_driving_assistance_preset("all");
    assert_eq!(settings.refresh_driving_assistance_preset(), "all");
    settings.lane_keeping = "off".to_string();
    assert_eq!(settings.refresh_driving_assistance_preset(), "custom");
}

#[test]
fn test_driving_assistance_presets_survive_reload() {
    with_data_dir(|_| {
        for (preset, expected) in DRIVING_ASSIST_PRESETS {
            let mut settings = Settings::default();
            settings.apply_driving_assistance_preset(preset);
            settings.save().unwrap();
            let loaded = Settings::load();
            assert_eq!(loaded.driving_assistance_preset, preset);
            assert_eq!(loaded.assist_values(), expected);
        }
    });
}

#[test]
fn test_legacy_settings_preserve_lane_keeping_choice() {
    with_data_dir(|_| {
        write_settings_file(r#"{"steering_assist": "off"}"#);
        let loaded = Settings::load();
        // Legacy "off" was the truck holding the lane for you: "full" now.
        assert_eq!(loaded.lane_keeping, "full");
        assert!(!loaded.lane_departure_warning);
        assert!(!loaded.automatic_emergency_braking);
        assert!(!loaded.stop_and_go_assist);
        assert_eq!(loaded.descent_speed_control, "off");
        assert_eq!(loaded.driving_assistance_preset, "custom");
        // Not the class default, which is on since the Balanced fresh
        // install: this save predates the setting and never opted in, so the
        // truck must not start braking itself at the gate.
        assert!(!loaded.destination_approach_assist);
    });
}

#[test]
fn test_a_legacy_save_that_opted_into_stopping_keeps_it() {
    // The other half of the same rule. Either old toggle counts as the opt-in
    // the blanket "everything off" is not allowed to take back.
    with_data_dir(|_| {
        write_settings_file(r#"{"steering_assist": "off", "selected_stop_assist": true}"#);
        let loaded = Settings::load();
        assert!(loaded.destination_approach_assist);
        assert!(!loaded.automatic_emergency_braking);
    });
}

#[test]
fn test_every_legacy_lane_value_migrates_without_changing_difficulty() {
    // The whole point of the rename: the words move, the truck does not.
    with_data_dir(|_| {
        for (legacy, expected) in [("off", "full"), ("light", "partial"), ("realistic", "off")] {
            write_settings_file(&format!(
                r#"{{"steering_assist": "{legacy}", "driving_assistance_preset": "custom"}}"#
            ));
            let loaded = Settings::load();
            assert_eq!(loaded.lane_keeping, expected);
            // A player who saw the old name is owed the explanation, and
            // only them.
            assert_eq!(loaded.lane_keeping_rename_notice_left, 3);
        }
    });
}

#[test]
fn test_unknown_lane_value_falls_back_to_full_and_says_so() {
    // Landing on "off" instead would start drift, rumble strips, off-road
    // damage AND stop granting the destination exit -- with no audible cause.
    with_data_dir(|_| {
        write_settings_file(
            r#"{"steering_assist": "sideways", "driving_assistance_preset": "custom"}"#,
        );
        let loaded = Settings::load();
        assert_eq!(loaded.lane_keeping, "full");
        assert!(loaded.lane_keeping_unreadable);
        // Nothing to explain about a rename that never applied to this value.
        assert_eq!(loaded.lane_keeping_rename_notice_left, 0);
    });
}

#[test]
fn test_a_fresh_install_hears_no_rename_notice() {
    with_data_dir(|_| {
        let path = Settings::path();
        if path.exists() {
            std::fs::remove_file(&path).unwrap();
        }
        let loaded = Settings::load();
        // The shipped default, which the preset promises. It is "full", the
        // same as the unreadable-value fallback, but the fallback also
        // announces itself and a fresh install must not.
        assert_eq!(loaded.lane_keeping, "full");
        assert_eq!(loaded.lane_keeping_rename_notice_left, 0);
        assert!(!loaded.lane_keeping_unreadable);
    });
}

#[test]
fn test_settings_still_write_the_legacy_lane_key_for_1_8_builds() {
    // A 1.8.x build shares this settings.json and still reads
    // ``steering_assist``; dropping the key would reset it to its own
    // default and change what the truck does over there.
    with_data_dir(|_| {
        for (mode, legacy) in LANE_KEEPING_TO_LEGACY {
            let mut settings = Settings::default();
            settings.lane_keeping = mode.to_string();
            settings.save().unwrap();
            let written: Value =
                serde_json::from_str(&std::fs::read_to_string(Settings::path()).unwrap()).unwrap();
            assert_eq!(written["lane_keeping"], mode);
            assert_eq!(written["steering_assist"], legacy);
        }
    });
}

#[test]
fn test_a_current_version_settings_file_hears_no_notice() {
    with_data_dir(|_| {
        write_settings_file(&format!(r#"{{"settings_version": {SETTINGS_VERSION}}}"#));
        assert_eq!(Settings::load().settings_layout_notice_from, -1);
    });
}

#[test]
fn test_settings_version_is_written_to_disk() {
    with_data_dir(|_| {
        Settings::default().save().unwrap();
        let written: Value =
            serde_json::from_str(&std::fs::read_to_string(Settings::path()).unwrap()).unwrap();
        assert_eq!(written["settings_version"], SETTINGS_VERSION);
    });
}

#[test]
fn test_realistic_pacing_migrates_to_standard_and_is_explained() {
    // Owner ruling, 2026-08-19: Realistic was the fastest pacing on the row,
    // not the truest to life, so the row now offers Relaxed and Standard only.
    //
    // A player who had chosen it gets standard -- but their game clock now
    // runs at half the rate they set, so a driving day takes twice the real
    // time. Nothing else would tell them: the row reads Standard as though
    // they had picked it. Same notice shape as the lane-keeping rename.
    assert!(!TIME_SCALES.contains(&40.0));
    with_data_dir(|_| {
        write_settings_file(r#"{"time_scale": 40.0}"#);
        let loaded = Settings::load();
        assert_eq!(loaded.time_scale, 20.0);
        assert_eq!(loaded.pace_retired_notice_left, PACE_RETIRED_NOTICES);
    });
}

#[test]
fn test_retired_lane_centering_assist_is_dropped_on_load_and_save() {
    // Owner 1.9: retire the phantom row. Old saves still carry the key;
    // load must ignore it, and the next save must not write it back.
    with_data_dir(|_| {
        write_settings_file(
            r#"{"lane_centering_assist": true, "lane_keeping": "partial", "driving_assistance_preset": "balanced"}"#,
        );
        let loaded = Settings::load();
        assert!(!Settings::FIELD_NAMES.contains(&"lane_centering_assist"));
        loaded.save().unwrap();
        let saved: Value =
            serde_json::from_str(&std::fs::read_to_string(Settings::path()).unwrap()).unwrap();
        assert!(
            saved.get("lane_centering_assist").is_none(),
            "retired key must not be rewritten: {saved}"
        );
    });
}

#[test]
fn test_a_pacing_the_row_still_offers_is_left_alone() {
    // The migration is for the one retired value, not a clamp on the field.
    //
    // A hand-edited custom scale has always run at whatever it says, and the
    // row has always been able to read it back as "N times". Turning this
    // into a general clamp would silently reset those saves too.
    with_data_dir(|_| {
        for saved in [1.0, 10.0, 20.0, 30.0] {
            write_settings_file(&format!(r#"{{"time_scale": {saved:?}}}"#));
            let loaded = Settings::load();
            assert_eq!(loaded.time_scale, saved, "{saved}");
            assert_eq!(loaded.pace_retired_notice_left, 0, "{saved}");
        }
    });
}

/// Fields with no consumer anywhere outside settings.py and the settings
/// menu. Each one needs a reason to be here, because "a menu row and nothing
/// else" is exactly what a phantom setting looks like (lane_centering_assist
/// was one until it was retired for 1.9).
///
/// Internal flags -- machinery the player never chooses, read inside
/// settings.py or by the settings menu itself.
const SETTINGS_INTERNAL_FLAGS: [&str; 11] = [
    // The layout-migration pair: which menu shape wrote the file, and which
    // layout notices are still owed. Read by Settings.load and the Gameplay
    // submenu, never by the game.
    "settings_version",
    "settings_layout_notice_from",
    // How many times the Lane keeping row still explains its own rename.
    // Counted down by the row that speaks it.
    "lane_keeping_rename_notice_left",
    // Same shape, for the Driving mode row explaining that Realistic pacing
    // was retired and their save landed on Standard. Armed by Settings.load,
    // counted down by the row that speaks it.
    "pace_retired_notice_left",
    // The preset row's own state. apply_/refresh_driving_assistance_preset
    // write it; it names a combination of the real fields rather than doing
    // anything itself.
    "driving_assistance_preset",
    // The roadside-chatter switches. Nothing reads these by name: the drive
    // asks settings.chatter_enabled(category), which maps a bake category to
    // its switch through CHATTER_CATEGORY_FIELDS.
    "chatter_parks",
    "chatter_rivers",
    "chatter_passes",
    "chatter_museums",
    "chatter_billboards",
    // The driving-speech rung. Nothing reads this by name either: call sites
    // ask settings.speaks(category), settings.speech_disposition(category),
    // or settings.renders_terse(), all of which read the field inside
    // settings.
    "driving_speech",
];

/// Pending features -- a real row a player can set, for behaviour that does
/// not exist yet. Different from an internal flag: a player CAN choose it
/// and hear nothing happen, so the help text must say so plainly. Empty
/// after lane_centering_assist was retired for 1.9 (owner: retire, do not
/// implement).
const SETTINGS_PENDING_FEATURES: [&str; 0] = [];

/// Every Settings field must reach the game, or be listed above with a
/// reason. A field whose only appearances are its own definition and a menu
/// row promises a blind player something the truck never does. Scans the
/// Rust sources of both crates the way the Python test scanned
/// `src/freight_fate`; until the states are ported most consumers do not
/// exist yet, so it waits.
#[test]
#[ignore = "wrong crate: ff-core cannot see the game crate, so this case belongs in crates/freight-fate/tests/ -- the states that consume the settings"]
fn test_no_settings_field_is_a_phantom() {
    let root = game_root().join("crates");
    let mut sources = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                out.push((name, text));
            }
        }
    }
    walk(&root, &mut sources);
    let sources: Vec<String> = sources
        .into_iter()
        .filter(|(name, _)| {
            !["settings.rs", "migrate.rs", "tests.rs", "main_menu.rs"].contains(&name.as_str())
        })
        .map(|(_, text)| text)
        .collect();
    let known: Vec<&str> = SETTINGS_INTERNAL_FLAGS
        .iter()
        .chain(SETTINGS_PENDING_FEATURES.iter())
        .copied()
        .collect();
    let phantoms: Vec<&str> = Settings::FIELD_NAMES
        .iter()
        .copied()
        .filter(|field| !known.contains(field))
        .filter(|field| {
            let pattern = regex::Regex::new(&format!(r"\b{field}\b")).unwrap();
            !sources.iter().any(|text| pattern.is_match(text))
        })
        .collect();
    assert!(
        phantoms.is_empty(),
        "These settings have no consumer outside settings and the settings menu: {phantoms:?}"
    );
    // The allow-lists must not outlive their entries either.
    for name in known {
        assert!(Settings::FIELD_NAMES.contains(&name), "{name}");
    }
}

// -- tests/test_driving_speech_ladder.py ----------------------------------------

#[test]
fn test_the_default_rung_is_standard() {
    assert_eq!(Settings::default().driving_speech, "standard");
}

#[test]
fn test_a_saved_terse_player_lands_on_quiet() {
    let s = from_json(json!({"speech_verbosity": 0}));
    assert_eq!(s.driving_speech, "quiet");
    // False == 0 in Python, so a bool-valued verbosity counts too.
    assert_eq!(
        from_json(json!({"speech_verbosity": false})).driving_speech,
        "quiet"
    );
}

#[test]
fn test_a_saved_normal_player_lands_on_standard() {
    let s = from_json(json!({"speech_verbosity": 1}));
    assert_eq!(s.driving_speech, "standard");
}

#[test]
fn test_a_nonsense_saved_verbosity_lands_on_standard() {
    let s = from_json(json!({"speech_verbosity": 7}));
    assert_eq!(s.driving_speech, "standard");
}

#[test]
fn test_a_settings_file_that_already_has_a_rung_is_left_alone() {
    // The migration must not re-run against a file that has moved on, or a
    // player who chose urgent_only would be dragged back to quiet on the
    // next launch of a build that still saw a stale speech_verbosity.
    let s = from_json(json!({"speech_verbosity": 0, "driving_speech": "urgent_only"}));
    assert_eq!(s.driving_speech, "urgent_only");
}

#[test]
fn test_an_unreadable_rung_falls_back_to_standard() {
    let s = from_json(json!({"driving_speech": "loud please"}));
    assert_eq!(s.driving_speech, "standard");
    // The removed coaching rung lands on the setting it already sounded like.
    assert_eq!(
        from_json(json!({"driving_speech": "coaching"})).driving_speech,
        "standard"
    );
}

#[test]
fn test_the_settings_object_answers_for_a_category() {
    let mut s = Settings::default();
    s.driving_speech = "urgent_only".to_string();
    assert!(s.speaks(Some(SpeechCategory::Safety)));
    assert!(!s.speaks(Some(SpeechCategory::Status)));
    assert!(s.speaks(None));
    assert!(s.renders_terse());

    s.driving_speech = "coaching".to_string();
    assert!(s.speaks(Some(SpeechCategory::Status)));
    assert!(!s.renders_terse());
}

#[test]
fn test_verbosity_is_gone() {
    // 11 references across 7 src files, all replaced -- a leftover reader
    // would silently see normal for every player.
    assert!(!Settings::FIELD_NAMES.contains(&"speech_verbosity"));
}

#[test]
fn test_flavor_is_independent_of_the_rung() {
    // The owner's directive of 2026-08-15, as an executable assertion: the
    // ladder governs information, the chatter switches govern colour, and
    // neither may grow a dependency on the other.
    let mut s = Settings::default();
    s.driving_speech = "urgent_only".to_string();
    s.set_all_chatter(true);
    assert!(s.chatter_enabled("billboard"));

    s.driving_speech = "coaching".to_string();
    s.set_all_chatter(false);
    assert!(!s.chatter_enabled("billboard"));
}

#[test]
fn test_quiet_keeps_short_cab_confirmations() {
    // Owner playtest, 2026-08-17: "quiet still feels busy". (The source
    // scan of the driving states for the three cab lines goes with the
    // states port.)
    let mut quiet = Settings::default();
    quiet.driving_speech = "quiet".to_string();
    assert_eq!(
        quiet.speech_disposition(Some(SpeechCategory::Confirmation)),
        Disposition::Terse
    );
}

#[test]
fn test_the_cruise_dial_answers_with_the_number_alone_at_quiet() {
    // Walking the dial is a rapid run of presses, and the unit never changes
    // between them -- so at quiet the figure is the whole message (owner,
    // 2026-08-17).
    use crate::speech_text::SpokenMessage;
    let line = SpokenMessage::with_terse("Adaptive cruise 62 miles per hour.", "62.");
    for rung in ["coaching", "standard"] {
        let mut s = Settings::default();
        s.driving_speech = rung.to_string();
        assert_eq!(
            line.render(s.renders_terse()),
            "Adaptive cruise 62 miles per hour."
        );
    }
    for rung in ["quiet", "urgent_only"] {
        let mut s = Settings::default();
        s.driving_speech = rung.to_string();
        assert_eq!(line.render(s.renders_terse()), "62.");
    }
}

// -- tests/test_metric_units.py / test_metric_readouts.py -----------------------

#[test]
fn test_distance_text_units_and_precision() {
    let s = Settings::default();
    assert_eq!(s.distance_text(38.0, false), "38 miles");
    assert_eq!(s.distance_text(1.0, false), "1 mile");
    assert_eq!(s.distance_text(1.0, true), "1.0 mile");
    assert_eq!(s.distance_text(1.2, true), "1.2 miles");

    let m = metric();
    assert_eq!(m.distance_text(38.0, false), "61 kilometers");
    assert_eq!(m.distance_text(1.2, true), "1.9 kilometers");
    assert_eq!(m.distance_text(1.0 / 1.609344, true), "1.0 kilometer");
    assert_eq!(m.distance_text(0.62, false), "1 kilometer");
}

#[test]
fn test_distance_value_and_unit_pair_up_for_two_number_readouts() {
    let s = metric();
    assert_eq!(s.distance_value(100.0, 0, false), "161");
    assert_eq!(s.distance_value(100.0, 1, false), "160.9");
    assert_eq!(s.distance_value(1000.0, 0, true), "1,609");
    assert_eq!(s.distance_unit_text(true), "kilometers");
    assert_eq!(s.distance_unit_text(false), "kilometer");
}

#[test]
fn test_per_distance_rescales_a_per_mile_rate() {
    // $3.22 a mile is $2.00 a kilometer -- the rate falls, it does not rise.
    assert_eq!(imperial().per_distance(3.218688), 3.218688);
    assert_eq!(
        crate::pyfmt::round_py_n(metric().per_distance(3.218688), 2),
        2.0
    );
}

#[test]
fn speed_value_short_distance_and_gap_follow_the_unit() {
    let s = Settings::default();
    assert_eq!(s.speed_value(65.4), "65");
    assert_eq!(s.speed_value(-0.3), "0");
    assert_eq!(metric().speed_value(60.0), "97");
    assert_eq!(s.short_distance_text(0.2), "a quarter mile");
    assert_eq!(s.short_distance_text(0.05), "a quarter mile");
    assert_eq!(s.short_distance_text(0.5), "half a mile");
    assert_eq!(s.short_distance_text(0.75), "three quarters of a mile");
    assert_eq!(s.short_distance_text(1.0), "one mile");
    assert_eq!(s.short_distance_text(1.2), "1.2 miles");
    let m = metric();
    assert_eq!(m.short_distance_text(0.25), "400 meters");
    assert_eq!(m.short_distance_text(0.01), "100 meters");
    assert_eq!(m.short_distance_text(0.6), "1.0 kilometer");
    assert_eq!(s.gap_text(2.0), "2.0 miles");
    assert_eq!(m.gap_text(2.0), "3.2 kilometers");
    assert_eq!(s.hud_speed_text(55.0), "55 mph");
    assert_eq!(m.hud_speed_text(55.0), "89 km/h");
}

// -- tests/test_models.py --------------------------------------------------------

#[test]
fn test_settings_roundtrip() {
    with_data_dir(|_| {
        let mut s = Settings::default();
        s.imperial_units = false;
        s.music_volume = 0.3;
        s.radio_volume = 0.2;
        s.radio_enabled = false;
        s.radio_station_id = "ff-night-line".to_string();
        s.radio_streamer_safe = true;
        s.weather_volume = 0.4;
        s.engine_volume = 0.7;
        s.ui_volume = 0.8;
        s.sapi_events = false;
        s.save().unwrap();
        let loaded = Settings::load();
        assert!(!loaded.imperial_units);
        assert_eq!(loaded.music_volume, 0.3);
        assert_eq!(loaded.radio_volume, 0.2);
        assert!(!loaded.radio_enabled);
        assert_eq!(loaded.radio_station_id, "ff-night-line");
        assert!(loaded.radio_streamer_safe);
        assert_eq!(loaded.weather_volume, 0.4);
        assert_eq!(loaded.engine_volume, 0.7);
        assert_eq!(loaded.ui_volume, 0.8);
        assert!(!loaded.sapi_events);
        // Everything else survives the trip too, and the tmp file is gone.
        assert_eq!(loaded, s);
        assert!(!Settings::path().with_extension("json.tmp").exists());
    });
}

#[test]
fn test_sapi_events_default_on() {
    assert!(Settings::default().sapi_events);
}

#[test]
fn test_music_volume_defaults_to_half() {
    assert_eq!(Settings::default().music_volume, 0.5);
}

#[test]
fn test_radio_defaults_are_full_dial_and_quiet() {
    let s = Settings::default();
    assert!(s.radio_enabled);
    assert_eq!(s.radio_volume, 0.25);
    // Streamer-safe mode is the opt-out a broadcaster takes, not the default.
    assert!(!s.radio_streamer_safe);
    // A playlist file plays in its own order unless the player asks otherwise.
    assert!(!s.radio_shuffle_playlists);
}

#[test]
fn test_split_audio_volume_defaults_prioritize_cues_over_background() {
    let s = Settings::default();
    assert!(s.ui_volume > s.music_volume);
    assert!(s.ui_volume > s.radio_volume);
    assert!(s.ui_volume > s.weather_volume);
    assert!(s.ui_volume > s.engine_volume);
}

#[test]
fn test_legacy_hos_off_setting_loads_as_realistic() {
    // The 1.5.0 player-facing "off" mode is gone; legacy saves fall through
    // to the realistic default rather than silently disabling enforcement.
    with_data_dir(|_| {
        let mut s = Settings::default();
        s.hos_mode = "off".to_string();
        s.save().unwrap();
        let loaded = Settings::load();
        assert_eq!(loaded.hos_mode, "realistic");
    });
}

#[test]
fn test_damaged_settings_fall_back_to_defaults() {
    // A settings file damaged by a crash mid-write, or hand-edited into a
    // bad shape, must not take the game's startup with it -- and must not
    // read as silence. Anything that is not a level falls back to the
    // default.
    with_data_dir(|_| {
        let defaults = Settings::default();
        for payload in [
            r#"{"sfx_volume": null, "master_volume": null}"#,
            r#"{"sfx_volume": false, "master_volume": false}"#,
            r#"{"sfx_volume": "loud", "master_volume": [1]}"#,
            r#"{"sfx_volume": 0.8, "master_vol"#, // truncated mid-write
            "[1, 2, 3]",                          // not a settings object at all
            "",
        ] {
            write_settings_file(payload);
            let loaded = Settings::load();
            assert_eq!(loaded.master_volume, defaults.master_volume, "{payload:?}");
            assert_eq!(loaded.sfx_volume, defaults.sfx_volume, "{payload:?}");
            assert_eq!(loaded.speech_volume, defaults.speech_volume, "{payload:?}");
        }
    });
}

#[test]
fn a_file_that_is_not_an_object_reads_as_an_empty_one() {
    // Python: `json.load` gave a list, which `load` turned into `{}` -- a
    // file that exists but says nothing, so the pre-preset migration runs.
    assert_eq!(parse_settings_text("[1, 2, 3]"), Some(Map::new()));
    assert_eq!(parse_settings_text("{not json"), None);
    assert_eq!(parse_settings_text(""), None);
    with_data_dir(|_| {
        write_settings_file("[1, 2, 3]");
        let loaded = Settings::load();
        assert_eq!(loaded.driving_assistance_preset, "custom");
        assert_eq!(loaded.lane_keeping, "full");
        // Unparseable text is not a file at all, so this reads as a fresh
        // install and lands on the shipped defaults -- All assists since
        // 2026-09-30 -- rather than on the pre-preset migration above.
        write_settings_file("{not json");
        let loaded = Settings::load();
        assert_eq!(loaded.driving_assistance_preset, "all");
        assert_eq!(loaded.lane_keeping, "full");
    });
}

#[test]
fn test_settings_keep_a_level_the_player_really_set() {
    // The fallback must not undo a deliberate choice: zero stays zero.
    with_data_dir(|_| {
        let mut s = Settings::default();
        s.sfx_volume = 0.0;
        s.master_volume = 0.25;
        s.save().unwrap();
        let loaded = Settings::load();
        assert_eq!(loaded.sfx_volume, 0.0);
        assert_eq!(loaded.master_volume, 0.25);
    });
}

#[test]
fn test_settings_survive_corrupt_file() {
    with_data_dir(|_| {
        let s = Settings::default();
        s.save().unwrap();
        std::fs::write(Settings::path(), "{not json").unwrap();
        let loaded = Settings::load();
        assert!(loaded.imperial_units); // defaults
    });
}

#[test]
fn test_unit_formatting() {
    let mut s = Settings::default();
    assert!(s.speed_text(60.0).contains("miles per hour"));
    s.imperial_units = false;
    assert_eq!(s.speed_text(60.0), "97 kilometers per hour");
}

// -- tests/test_lane_keeping.py --------------------------------------------------

#[test]
fn test_lane_keeping_setting_is_validated() {
    with_data_dir(|_| {
        let mut settings = Settings::default();
        settings.lane_keeping = "off".to_string();
        settings.save().unwrap();
        assert_eq!(Settings::load().lane_keeping, "off");
        settings.lane_keeping = "bogus".to_string();
        settings.save().unwrap();
        // An unreadable value takes the fallback that changes the least about
        // what the truck does, and says so rather than taking it in silence.
        let loaded = Settings::load();
        assert_eq!(loaded.lane_keeping, "full");
        assert!(loaded.lane_keeping_unreadable);
    });
}

#[test]
fn test_lane_keeping_predicates_follow_the_mode() {
    let mut settings = Settings::default();
    settings.lane_keeping = "full".to_string();
    assert!(settings.lane_is_automated());
    assert!(!settings.lane_is_manual());
    for mode in ["partial", "off"] {
        settings.lane_keeping = mode.to_string();
        assert!(!settings.lane_is_automated());
        assert!(settings.lane_is_manual());
    }
}

#[test]
fn lane_keeping_labels_carry_their_clause_and_the_fallback_covers_junk() {
    let mut settings = Settings::default();
    for (mode, label) in LANE_KEEPING_LABELS {
        settings.lane_keeping = mode.to_string();
        assert_eq!(settings.lane_keeping_label(), label);
    }
    settings.lane_keeping = "sideways".to_string();
    assert_eq!(
        settings.lane_keeping_label(),
        "full, the truck holds the lane and takes your exits"
    );
}

// -- tests/test_pedal_latch.py ---------------------------------------------------

#[test]
fn test_legacy_bool_settings_migrate_to_modes() {
    // The throttle latch is gone: a bool true, and the old three-way
    // "assists first" / "latch first" strings, all land on brake-only on.
    assert_eq!(from_json(json!({"pedal_latch": true})).pedal_latch, "on");
    assert_eq!(from_json(json!({"pedal_latch": false})).pedal_latch, "off");
    assert_eq!(
        from_json(json!({"pedal_latch": "latch first"})).pedal_latch,
        "on"
    );
    assert_eq!(
        from_json(json!({"pedal_latch": "assists first"})).pedal_latch,
        "on"
    );
    assert_eq!(
        from_json(json!({"pedal_latch": "sideways"})).pedal_latch,
        "on"
    );
}

// -- tests/test_village_callouts.py ---------------------------------------------

#[test]
fn test_one_alpha_day_village_bool_migrates() {
    // The village switch shipped briefly as a chatter bool: an explicit off
    // carries over as silence, an untouched on takes the new default.
    with_data_dir(|_| {
        write_settings_file(r#"{"chatter_villages": false}"#);
        assert_eq!(Settings::load().place_callouts, "off");
        write_settings_file(r#"{"chatter_villages": true}"#);
        assert_eq!(Settings::load().place_callouts, "sparse");
        write_settings_file(r#"{"place_callouts": "junk"}"#);
        assert_eq!(Settings::load().place_callouts, "sparse");
        write_settings_file(r#"{"chatter_villages": false, "place_callouts": "all"}"#);
        assert_eq!(Settings::load().place_callouts, "all");
    });
}

#[test]
fn test_backup_announcements_screens_to_every_time() {
    // MariahL, 2026-09-02: the all-clear cadence row. A missing or junk
    // value keeps the owner's default of speaking after every save.
    with_data_dir(|_| {
        write_settings_file(r#"{}"#);
        assert_eq!(Settings::load().backup_announcements, "every");
        write_settings_file(r#"{"backup_announcements": "once"}"#);
        assert_eq!(Settings::load().backup_announcements, "once");
        write_settings_file(r#"{"backup_announcements": "off"}"#);
        assert_eq!(Settings::load().backup_announcements, "off");
        write_settings_file(r#"{"backup_announcements": "loud"}"#);
        assert_eq!(Settings::load().backup_announcements, "every");
        write_settings_file(r#"{"backup_announcements": 3}"#);
        assert_eq!(Settings::load().backup_announcements, "every");
    });
}

// -- tests/test_roadside_chatter.py ---------------------------------------------

#[test]
fn test_chatter_settings_map_categories() {
    let mut s = Settings::default();
    assert_eq!(s.chatter_summary(), "everything");
    for category in [
        "national_park",
        "national_forest",
        "wilderness",
        "protected_area",
        "river",
        "mountain_pass",
        "highway_marker",
        "museum",
        "billboard",
        "billboard_sign",
    ] {
        assert!(s.chatter_enabled(category), "{category}");
    }

    s.chatter_parks = false;
    assert!(!s.chatter_enabled("national_forest"));
    assert!(!s.chatter_enabled("protected_area"));
    assert!(s.chatter_enabled("river"));
    assert_eq!(s.chatter_summary(), "custom");

    s.set_all_chatter(false);
    assert_eq!(s.chatter_summary(), "off");
    assert!(!s.chatter_enabled("billboard"));
    // Placed billboard signs ride the same billboards switch as the random pool.
    assert!(!s.chatter_enabled("billboard_sign"));
    // An unknown future category speaks rather than silently vanishing.
    assert!(s.chatter_enabled("meteor_crater"));

    s.set_all_chatter(true);
    assert_eq!(s.chatter_summary(), "everything");
}

#[test]
fn test_chatter_settings_survive_save_and_load() {
    with_data_dir(|_| {
        let mut s = Settings::default();
        s.chatter_billboards = false;
        s.chatter_rivers = false;
        s.save().unwrap();

        let loaded = Settings::load();
        assert!(!loaded.chatter_billboards);
        assert!(!loaded.chatter_rivers);
        assert!(loaded.chatter_parks);
    });
}

// -- tests/test_lane_guide_tone.py ----------------------------------------------

#[test]
fn test_the_default_is_the_road_bed_not_the_tone() {
    // The ruling's line: a tone is chosen, never given.
    assert!(!Settings::default().lane_guide_tone);
}

#[test]
fn test_an_unreadable_setting_falls_to_the_bed() {
    // A broken settings file must not be able to choose a tone for someone
    // the tone would hurt.
    for junk in [json!("yes"), json!(1), json!(null), json!("true")] {
        let s = from_json(json!({"lane_guide_tone": junk}));
        assert!(!s.lane_guide_tone, "{junk}");
    }
    assert!(from_json(json!({"lane_guide_tone": true})).lane_guide_tone);
}

// -- tests/test_profile_sharing_sync.py (the consent-version gate) ---------------

/// Only the current consent version may keep sharing switched on: an older
/// grant is stale, and a newer one is a file this build has never asked for.
#[test]
fn test_only_current_profile_sharing_consent_can_remain_on() {
    with_data_dir(|data| {
        for (version, expected) in [
            (None, false),
            (Some(0), false),
            (Some(1), false),
            (Some(2), false),
            (Some(3), true),
            (Some(4), false),
        ] {
            let mut raw = serde_json::json!({"online_presence": true});
            if let Some(version) = version {
                raw["profile_sharing_consent_version"] = serde_json::json!(version);
            }
            std::fs::create_dir_all(data).unwrap();
            std::fs::write(data.join("settings.json"), raw.to_string()).unwrap();
            assert_eq!(
                Settings::load().online_presence,
                expected,
                "consent version {version:?}"
            );
        }
    });
}

// -- tests/test_updater.py -------------------------------------------------------

#[test]
fn test_settings_default_and_validation() {
    with_data_dir(|_| {
        let mut s = Settings::default();
        assert_eq!(s.update_channel, "");
        assert_eq!(s.skipped_update, "");
        s.update_channel = "weird".to_string();
        s.save().unwrap();
        let loaded = Settings::load();
        assert_eq!(loaded.update_channel, ""); // invalid value reset
    });
}

// -- tests/test_radio.py (the Settings half) ------------------------------------

#[test]
fn test_radio_persists_enabled_station_and_volume() {
    with_data_dir(|_| {
        let mut settings = Settings::default();
        settings.radio_enabled = false;
        settings.radio_station_id = "ff-night-line".to_string();
        settings.radio_volume = 0.4;
        settings.radio_streamer_safe = true;
        settings.save().unwrap();

        let loaded = Settings::load();
        // (`RadioState::from_settings(loaded)` is the radio port's half.)
        assert!(!loaded.radio_enabled);
        assert_eq!(loaded.radio_station_id, "ff-night-line");
        assert_eq!(loaded.radio_volume, 0.4);
        assert!(loaded.radio_streamer_safe);
    });
}

// -- App()-bound --------------------------------------------------------------------

// `test_settings_menu_saves_each_change` is live in `crates/freight-fate/tests/states_main_menu_settings.rs`.

// `test_gameplay_reorg_notice_fires_once_for_a_pre_reorg_settings_file` is live in `crates/freight-fate/tests/states_main_menu_settings.rs`.

// `test_the_driving_mode_row_explains_the_retired_pacing` is live in `crates/freight-fate/tests/states_main_menu_settings.rs`.

// `test_lane_keeping_row_explains_its_rename_to_returning_players` is live in `crates/freight-fate/tests/states_main_menu_settings.rs`.
