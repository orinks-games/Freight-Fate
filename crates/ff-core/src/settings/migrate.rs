//! The settings file: reading it, every migration `from_dict` runs, and the
//! atomic write. Port of `Settings.load`/`from_dict`/`save` in
//! `freight_fate/settings.py`.

use std::io;
use std::path::PathBuf;

use serde_json::{Map, Value};

use super::{
    data_dir, lane_keeping_from_legacy, lane_keeping_to_legacy, Settings, ACC_GAP_CHOICES,
    ACC_GAP_DEFAULT, BACKUP_ANNOUNCEMENT_MODES, DESCENT_SPEED_CONTROL_MODES, DRIVING_ASSIST_FIELDS,
    DRIVING_ASSIST_PRESETS, LANE_CUE_LOUDNESS_MODES, LANE_KEEPING_FALLBACK, LANE_KEEPING_MODES,
    LANE_KEEPING_RENAME_NOTICES, PACE_RETIRED_NOTICES, PEDAL_LATCH_MODES, PLACE_CALLOUT_MODES,
    PROFILE_SHARING_CONSENT_VERSION, RETIRED_TIME_SCALE, SETTINGS_VERSION, TIME_SCALE_FALLBACK,
    UPDATE_CHANNELS,
};
use crate::pyfmt::py_str_float;
use crate::sim::hos::HOS_MODES;
use crate::speech_pacing::{DEFAULT_DRIVING_SPEECH, DRIVING_SPEECH_MODES};

pub const SETTINGS_FILE_NAME: &str = "settings.json";

/// How a raw JSON value lands on a typed field -- the Python `setattr` copy
/// and the type check its migration later ran, folded into one step per
/// field kind. Each takes the field (already holding its default), the raw
/// value, and the key (for the level warning).
pub(crate) mod coerce {
    use crate::settings::ScoreDisplay;
    use serde_json::Value;

    /// A string the value checks never accept: what a non-string lands on
    /// for a field whose migration checks membership, so the check then
    /// takes the same fallback Python took for a value of the wrong type.
    /// Never escapes `from_dict`, which checks every such field.
    pub(crate) const JUNK: &str = "\u{1}unreadable";

    /// A bool whose migration type-checked it (non-bool -> the default).
    pub(crate) fn bool_strict(target: &mut bool, value: &Value, _key: &str) {
        if let Value::Bool(b) = value {
            *target = *b;
        }
    }

    /// A bool Python never type-checked: the game read it with `if`, so a
    /// value of another shape acts by its truthiness.
    pub(crate) fn bool_truthy(target: &mut bool, value: &Value, _key: &str) {
        *target = match value {
            Value::Null => false,
            Value::Bool(b) => *b,
            Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
            Value::String(s) => !s.is_empty(),
            Value::Array(a) => !a.is_empty(),
            Value::Object(o) => !o.is_empty(),
        };
    }

    /// A string whose migration checks membership: a non-string becomes
    /// [`JUNK`] so that check takes its fallback.
    pub(crate) fn str_checked(target: &mut String, value: &Value, _key: &str) {
        *target = match value {
            Value::String(s) => s.clone(),
            _ => JUNK.to_string(),
        };
    }

    /// A string nothing checks: a non-string keeps the default (Python
    /// carried the value as-is; nothing player-facing compared it).
    pub(crate) fn str_plain(target: &mut String, value: &Value, _key: &str) {
        if let Value::String(s) = value {
            *target = s.clone();
        }
    }

    pub(crate) fn score_display(target: &mut ScoreDisplay, value: &Value, _key: &str) {
        *target = match value.as_str() {
            Some("Both") | Some("both") => ScoreDisplay::Both,
            Some("Percent") | Some("percent") => ScoreDisplay::Percent,
            Some("Description") | Some("description") => ScoreDisplay::Description,
            _ => ScoreDisplay::Both,
        };
    }

    /// `pedal_latch` shipped as a bool, then a three-way that also caught
    /// the throttle. Bool true and the old "assists first" / "latch first"
    /// strings land on "on" (brake latch only); false and "off" stay off.
    pub(crate) fn pedal_latch(target: &mut String, value: &Value, key: &str) {
        match value {
            Value::Bool(true) => *target = "on".to_string(),
            Value::Bool(false) => *target = "off".to_string(),
            other => str_checked(target, other, key),
        }
    }

    /// An int whose migration rejected bools and non-ints (JSON `2.0` is a
    /// float to Python's json module too).
    pub(crate) fn int_strict(target: &mut i64, value: &Value, _key: &str) {
        if let Some(i) = value.as_i64() {
            *target = i;
        }
    }

    /// An int nothing type-checked; an integer-valued float compares equal
    /// to the int in Python, so it counts.
    pub(crate) fn int_lenient(target: &mut i64, value: &Value, _key: &str) {
        if let Some(i) = value.as_i64() {
            *target = i;
        } else if let Some(f) = value.as_f64() {
            if f.is_finite() && f.fract() == 0.0 {
                *target = f as i64;
            }
        }
    }

    /// `time_scale`: a number; nothing else is a pacing.
    pub(crate) fn float_plain(target: &mut f64, value: &Value, _key: &str) {
        if let Some(f) = value.as_f64() {
            *target = f;
        }
    }

    /// A volume or voice level. A level that is not a number -- null, true,
    /// a list, a word -- used to raise straight out of load() and take the
    /// game's whole startup with it. It falls back to the default instead.
    /// A bool counts as damage, not as a level: false would read as
    /// silence. A numeric string is a level (`float("0.5")`). The 0..1 clamp
    /// is the migration's, after every field is in.
    pub(crate) fn level(target: &mut f64, value: &Value, key: &str) {
        let parsed = match value {
            Value::Number(n) => n.as_f64(),
            Value::String(s) => python_float(s),
            _ => None,
        };
        match parsed {
            Some(f) => *target = f,
            None => log::warn!(
                "Setting {key} is not a level ({}); using the default",
                super::py_repr(value)
            ),
        }
    }

    /// `float(text)`: surrounding whitespace allowed, underscores between
    /// digits allowed, inf/nan spellings accepted.
    fn python_float(text: &str) -> Option<f64> {
        let trimmed = text.trim();
        if trimmed.is_empty() || trimmed.starts_with('_') || trimmed.ends_with('_') {
            return None;
        }
        if trimmed.contains("__") {
            return None;
        }
        let cleaned: String = trimmed.chars().filter(|c| *c != '_').collect();
        let lowered = cleaned.to_ascii_lowercase();
        match lowered.trim_start_matches(['+', '-']) {
            "inf" | "infinity" => {
                return Some(if lowered.starts_with('-') {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                })
            }
            "nan" => return Some(f64::NAN),
            _ => {}
        }
        cleaned.parse::<f64>().ok()
    }
}

/// Python `repr()` of a JSON value, for the level warning.
fn py_repr(value: &Value) -> String {
    match value {
        Value::Null => "None".to_string(),
        Value::Bool(true) => "True".to_string(),
        Value::Bool(false) => "False".to_string(),
        Value::String(s) => format!("{s:?}").replace('"', "'"),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => i.to_string(),
            (None, Some(f)) => py_str_float(f),
            _ => n.to_string(),
        },
        other => other.to_string(),
    }
}

/// Python `json.dump` string escaping with `ensure_ascii=True`: everything
/// outside printable ASCII becomes `\uXXXX` (surrogate pairs above the BMP),
/// so the file the Python build wrote and the one this writes are the same
/// bytes.
fn py_json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(ch),
            _ => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

/// One JSON scalar the way `json.dump` spells it.
fn py_json_scalar(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(true) => "true".to_string(),
        Value::Bool(false) => "false".to_string(),
        Value::Number(n) => match (n.as_i64(), n.as_f64()) {
            (Some(i), _) => i.to_string(),
            (None, Some(f)) => py_str_float(f),
            _ => n.to_string(),
        },
        Value::String(s) => py_json_string(s),
        other => other.to_string(),
    }
}

/// The file text for `pairs`, exactly as `json.dump(data, f, indent=2)`
/// writes a flat object: two-space indent, `": "` separator, no trailing
/// newline.
pub fn py_json_dump_flat(pairs: &[(&str, Value)]) -> String {
    if pairs.is_empty() {
        return "{}".to_string();
    }
    let mut out = String::from("{\n");
    for (index, (key, value)) in pairs.iter().enumerate() {
        out.push_str("  ");
        out.push_str(&py_json_string(key));
        out.push_str(": ");
        out.push_str(&py_json_scalar(value));
        if index + 1 < pairs.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push('}');
    out
}

/// Parse a settings file's text the way `load` did: a decode error reads as
/// no data at all (`None`, every default stands), a document that is not an
/// object reads as an empty one (`{}`, a file that exists but says nothing)
/// -- the two are different things to `from_dict`.
pub fn parse_settings_text(text: &str) -> Option<Map<String, Value>> {
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(map)) => Some(map),
        Ok(_) => {
            log::warn!("Settings file is not a settings object; using defaults");
            Some(Map::new())
        }
        Err(err) => {
            log::warn!("Could not read settings; using defaults: {err}");
            None
        }
    }
}

fn value_is_int(value: Option<&Value>, wanted: i64) -> bool {
    match value {
        Some(Value::Number(n)) => n.as_i64() == Some(wanted) || n.as_f64() == Some(wanted as f64),
        _ => false,
    }
}

/// Python `max(0.0, min(1.0, value))`, including what it does with NaN
/// (`min` keeps its first argument when the comparison is false).
fn py_clamp_unit(value: f64) -> f64 {
    let upper = if value < 1.0 { value } else { 1.0 };
    if upper > 0.0 {
        upper
    } else {
        0.0
    }
}

impl Settings {
    /// `data_dir() / "settings.json"`.
    pub fn path() -> PathBuf {
        data_dir().join(SETTINGS_FILE_NAME)
    }

    /// Every persisted field plus the compatibility `steering_assist` key,
    /// in the order `save` writes them.
    ///
    /// Compatibility write, for one release only. A 1.8.x build installed
    /// alongside this one shares the same settings.json and still reads
    /// `steering_assist`; if the key vanished it would fall back to its own
    /// default and quietly change what the truck does over there. A reader
    /// may tolerate keys it does not know, but a writer must not drop a key
    /// another reader still needs. Remove this once 1.9 is the oldest build
    /// players run -- no earlier than the release after 1.9.0.
    pub fn file_pairs(&self) -> Vec<(&'static str, Value)> {
        let mut pairs = self.ordered_values();
        pairs.push((
            "steering_assist",
            Value::from(lane_keeping_to_legacy(&self.lane_keeping)),
        ));
        pairs
    }

    /// The settings file's text, byte for byte what the Python build wrote.
    pub fn to_file_text(&self) -> String {
        py_json_dump_flat(&self.player_owned().file_pairs())
    }

    /// Write the file atomically: `settings.json.tmp`, then rename over.
    pub fn save(&self) -> io::Result<()> {
        let path = Self::path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut tmp = path.clone();
        tmp.set_extension("json.tmp");
        std::fs::write(&tmp, self.to_file_text())?;
        std::fs::rename(&tmp, &path)
    }

    /// Read the settings file and run every migration. A missing or
    /// unreadable file is a fresh install (every default stands); a file
    /// that is not a settings object is an empty one.
    pub fn load() -> Settings {
        let data = match std::fs::read_to_string(Self::path()) {
            Ok(text) => parse_settings_text(&text),
            Err(err) if err.kind() == io::ErrorKind::NotFound => None,
            Err(err) => {
                log::warn!("Could not read settings; using defaults: {err}");
                None
            }
        };
        Self::from_dict(data.as_ref())
    }

    /// Build settings from a parsed settings file, running every migration.
    ///
    /// Split out of `load` so the migrations are testable without a
    /// filesystem. `data` is `None` when there was no readable file and
    /// every default stands -- several migrations below distinguish that
    /// from an empty dict (a file that exists but says nothing), and that
    /// distinction must survive the split.
    pub fn from_dict(data: Option<&Map<String, Value>>) -> Settings {
        let mut s = Settings::default();
        if let Some(data) = data {
            for (key, value) in data {
                s.assign_raw(key, value);
            }
            // The two old entrance-stopping toggles are now one player-facing
            // facility assist. Preserve either opt-in, then retire the hidden
            // rest-stop flag so turning the unified setting off stays off on
            // the next load.
            s.destination_approach_assist = s.destination_approach_assist || s.selected_stop_assist;
            s.selected_stop_assist = false;
            // The former board-only opt-in covered less information. Never
            // silently expand it into public Profile sharing.
            if !value_is_int(
                data.get("profile_sharing_consent_version"),
                PROFILE_SHARING_CONSENT_VERSION,
            ) {
                s.online_presence = false;
            }
        }
        // ``steering_assist`` became ``lane_keeping`` in 1.9. A save that
        // already carries the new key is read as-is; anything older has its
        // legacy value carried across to the mode that behaves identically,
        // so the truck does exactly what it did yesterday. A player who
        // cannot see the row change must never find the steering task in
        // their hands because a setting was renamed.
        if let Some(data) = data {
            if !data.contains_key("lane_keeping") {
                let legacy = data.get("steering_assist");
                let mapped = legacy
                    .and_then(Value::as_str)
                    .and_then(lane_keeping_from_legacy);
                if let Some(mode) = mapped {
                    s.lane_keeping = mode.to_string();
                    // This player had the old row under the old name. The
                    // row owes them an explanation, and only them.
                    s.lane_keeping_rename_notice_left = LANE_KEEPING_RENAME_NOTICES;
                } else if legacy.is_some() {
                    // The old key is there but says nothing we recognise.
                    // Taking the fallback silently would delete the
                    // destination-exit decision without a sound.
                    s.lane_keeping = LANE_KEEPING_FALLBACK.to_string();
                    s.lane_keeping_unreadable = true;
                } else {
                    s.lane_keeping = LANE_KEEPING_FALLBACK.to_string();
                }
            }
        }
        // Legacy 1.5.0 saves carried a player-selectable "off" mode. It is no
        // longer offered, so such saves fall through to the realistic default
        // below. debug_off stays valid as an internal dev/test bypass only.
        if !HOS_MODES.contains(&s.hos_mode.as_str()) {
            s.hos_mode = "realistic".to_string();
        }
        // Realistic pacing is no longer offered. A save that chose it lands
        // on standard rather than keeping a value the row cannot show, and
        // arms the notice: the game clock now runs at half the rate this
        // player set it to, which they would otherwise discover as their
        // hours-of-service day lasting twice as long for no stated reason.
        // Only the exact retired value migrates -- a hand-edited custom
        // scale still runs at whatever it says, as it always has.
        if s.time_scale == RETIRED_TIME_SCALE {
            s.time_scale = TIME_SCALE_FALLBACK;
            s.pace_retired_notice_left = PACE_RETIRED_NOTICES;
        }
        s.pace_retired_notice_left = s.pace_retired_notice_left.clamp(0, PACE_RETIRED_NOTICES);
        if !LANE_CUE_LOUDNESS_MODES.contains(&s.lane_cue_loudness.as_str()) {
            s.lane_cue_loudness = "standard".to_string();
        }
        // (A non-bool lane_guide_tone already fell to the bed in the raw
        // copy: an unreadable value falls to the bed, never to the tone --
        // the ruling's line is that a tone is chosen, so a broken settings
        // file must not be able to choose one.)
        if !LANE_KEEPING_MODES.contains(&s.lane_keeping.as_str()) {
            s.lane_keeping = LANE_KEEPING_FALLBACK.to_string();
            s.lane_keeping_unreadable = true;
            s.lane_departure_warning = false;
        }
        s.lane_keeping_rename_notice_left = s
            .lane_keeping_rename_notice_left
            .clamp(0, LANE_KEEPING_RENAME_NOTICES);
        if let Some(raw) = data.filter(|data| !data.contains_key("driving_assistance_preset")) {
            s.lane_departure_warning = s.lane_keeping != "full";
            for field in DRIVING_ASSIST_FIELDS {
                match field {
                    "descent_speed_control" => s.descent_speed_control = "off".to_string(),
                    // The migrated lane-keeping mode IS this save's current
                    // difficulty. The blanket "everything off" below must
                    // not reach it, or a pre-preset save would change what
                    // the truck does the moment it is opened.
                    "lane_departure_warning" | "lane_keeping" => {}
                    // Facility stopping was an explicit opt-in on every
                    // pre-preset save (its merged rest-stop half too), so the
                    // blanket off must not take it back -- but it must be the
                    // FILE's opt-in, never the class default. That default is
                    // on since the Balanced fresh install, and a save written
                    // before the preset existed has neither key, so reaching
                    // for the default handed every migrating driver an assist
                    // that takes the pedals at the gate, with every other
                    // assist forced off and nothing said.
                    "destination_approach_assist" => {
                        let opted_in = |key: &str| raw.get(key) == Some(&Value::Bool(true));
                        s.destination_approach_assist = opted_in("destination_approach_assist")
                            || opted_in("selected_stop_assist");
                    }
                    other => {
                        s.set_assist_value(other, super::AssistValue::Flag(false));
                    }
                }
            }
            s.driving_assistance_preset = "custom".to_string();
        }
        // (The assist bools and selected_stop_assist were type-checked in
        // the raw copy: a non-bool holds the class default.)
        if !DESCENT_SPEED_CONTROL_MODES.contains(&s.descent_speed_control.as_str()) {
            s.descent_speed_control = "realistic".to_string();
        }
        let preset_known = DRIVING_ASSIST_PRESETS
            .iter()
            .any(|(name, _)| *name == s.driving_assistance_preset)
            || s.driving_assistance_preset == "custom";
        if !preset_known {
            s.driving_assistance_preset = "custom".to_string();
        }
        if data.is_none_or(|data| data.contains_key("driving_assistance_preset")) {
            s.refresh_driving_assistance_preset();
        }
        if !["simple", "deliberate"].contains(&s.automatic_direction_changes.as_str()) {
            s.automatic_direction_changes = "simple".to_string();
        }
        if !["real", "classic"].contains(&s.jake_voice.as_str()) {
            s.jake_voice = "real".to_string();
        }
        if !ACC_GAP_CHOICES
            .iter()
            .any(|(name, _)| *name == s.acc_following_gap)
        {
            s.acc_following_gap = ACC_GAP_DEFAULT.to_string();
        }
        // Latching brake used to be a three-way that also caught the
        // throttle. The throttle half is gone; anything that was on
        // stays a latched brake.
        if s.pedal_latch == "assists first" || s.pedal_latch == "latch first" {
            s.pedal_latch = "on".to_string();
        }
        if !PEDAL_LATCH_MODES.contains(&s.pedal_latch.as_str()) {
            s.pedal_latch = "on".to_string();
        }
        // The two-value verbosity became a four-rung ladder (S4). A terse
        // player asked for less and lands on quiet; everyone else on
        // standard, which is what normal already was. Keyed on the absence
        // of the new field, so a player who has since picked a rung is
        // never dragged back by a stale verbosity left in the file.
        if let Some(data) = data {
            if !data.contains_key("driving_speech") {
                let terse = match data.get("speech_verbosity") {
                    // `== 0` in Python: 0, 0.0 and False all compare equal.
                    Some(Value::Bool(false)) => true,
                    other => value_is_int(other, 0),
                };
                s.driving_speech = if terse { "quiet" } else { "standard" }.to_string();
            }
        }
        if !DRIVING_SPEECH_MODES.contains(&s.driving_speech.as_str()) {
            // Also the migration for a saved "coaching" (the rung removed on
            // 2026-08-17): it was indistinguishable from standard at the
            // voice, and standard is the default, so a player who had it
            // lands on the setting they were already hearing.
            s.driving_speech = DEFAULT_DRIVING_SPEECH.to_string();
        }
        if !UPDATE_CHANNELS.contains(&s.update_channel.as_str()) {
            s.update_channel = String::new();
        }
        if s.event_backend.is_empty() || s.event_backend == coerce::JUNK {
            s.event_backend = "SAPI".to_string();
        }
        // (controller_enabled, haptics_enabled and the chatter switches were
        // type-checked in the raw copy.)
        // The village switch shipped for one alpha day as a chatter bool. An
        // explicit off carries over as silence; an untouched on takes the
        // new default ladder rather than pinning that player to the loudest
        // tier.
        if let Some(data) = data {
            if !data.contains_key("place_callouts")
                && data.get("chatter_villages") == Some(&Value::Bool(false))
            {
                s.place_callouts = "off".to_string();
            }
        }
        if !PLACE_CALLOUT_MODES.contains(&s.place_callouts.as_str()) {
            s.place_callouts = "sparse".to_string();
        }
        if !BACKUP_ANNOUNCEMENT_MODES.contains(&s.backup_announcements.as_str()) {
            s.backup_announcements = "every".to_string();
        }
        // (cloud_saves, live_weather_controls_calendar and
        // duck_audio_for_speech were type-checked in the raw copy.)
        for level in [
            &mut s.master_volume,
            &mut s.sfx_volume,
            &mut s.music_volume,
            &mut s.radio_volume,
            &mut s.weather_volume,
            &mut s.engine_volume,
            &mut s.ui_volume,
            &mut s.speech_rate,
            &mut s.speech_pitch,
            &mut s.speech_volume,
        ] {
            *level = py_clamp_unit(*level);
        }
        // The language setting arrived with the translations. A player who
        // already had a settings file has been hearing English, so that is
        // what they keep until they choose otherwise; only a fresh install
        // follows the operating system's language.
        if data.is_some_and(|data| !data.contains_key("language")) {
            s.language = crate::l10n::ENGLISH.to_string();
        }
        if s.radio_station_id.is_empty() || s.radio_station_id == coerce::JUNK {
            s.radio_station_id = "route_playlist".to_string();
        }
        // Settings-menu layout migration. A file written under an older
        // layout (an older settings_version, or none at all) records the
        // version it came from, and the Gameplay submenu later speaks every
        // notice above it; a fresh install (no file to read) writes the
        // current version and stays silent. Not tied to any one field -- it
        // tracks the menu shape.
        if let Some(data) = data {
            let saved_version = data
                .get("settings_version")
                .and_then(Value::as_i64)
                .unwrap_or(0);
            if saved_version < SETTINGS_VERSION {
                // The oldest layout still owed wins: a player who is two
                // reorganizations behind hears both, in order.
                s.settings_layout_notice_from = if s.settings_layout_notice_from < 0 {
                    saved_version
                } else {
                    s.settings_layout_notice_from.min(saved_version)
                };
            }
        }
        s.settings_version = SETTINGS_VERSION;
        debug_assert!(
            s.ordered_values()
                .iter()
                .all(|(_, value)| value.as_str() != Some(coerce::JUNK)),
            "a junk marker escaped from_dict"
        );
        s
    }
}
