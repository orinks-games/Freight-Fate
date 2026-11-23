//! The driving-assist fields: their values, the presets, and lending
//! them to a co-driver for a shift.

use super::{
    driving_assist_preset, static_mode, AssistValue, Settings, DRIVING_ASSIST_FIELDS,
    DRIVING_ASSIST_PRESETS,
};
use AssistValue::{Flag, Mode};

/// Player-selectable independent steering-assist levels.
pub const STEERING_ASSIST_MODES: [&str; 3] = ["manual", "partial", "full"];

/// Player-selectable independent speed-assist levels.
pub const SPEED_ASSIST_MODES: [&str; 4] = ["manual", "light", "standard", "strong"];

/// Explicit enum for the independent steering-assist toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SteeringAssistMode {
    Manual,
    Partial,
    Full,
    Custom,
}

impl SteeringAssistMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Partial => "partial",
            Self::Full => "full",
            Self::Custom => "custom",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "manual" => Some(Self::Manual),
            "partial" => Some(Self::Partial),
            "full" => Some(Self::Full),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    /// Cycle through selectable modes (`Manual -> Partial -> Full -> Manual`).
    pub fn next(self, direction: i64) -> Self {
        let index = match self {
            Self::Manual => 0,
            Self::Partial => 1,
            Self::Full => 2,
            Self::Custom => {
                return if direction >= 0 {
                    Self::Manual
                } else {
                    Self::Full
                }
            }
        };
        let next = (index + direction).rem_euclid(STEERING_ASSIST_MODES.len() as i64) as usize;
        Self::from_tag(STEERING_ASSIST_MODES[next]).unwrap_or(Self::Full)
    }
}

/// Explicit enum for the independent speed-assist toggle.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpeedAssistMode {
    Manual,
    Light,
    Standard,
    Strong,
    Custom,
}

impl SpeedAssistMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manual => "manual",
            Self::Light => "light",
            Self::Standard => "standard",
            Self::Strong => "strong",
            Self::Custom => "custom",
        }
    }

    pub fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "manual" => Some(Self::Manual),
            "light" => Some(Self::Light),
            "standard" => Some(Self::Standard),
            "strong" => Some(Self::Strong),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    /// Cycle through selectable modes (`Manual -> Light -> Standard -> Strong`).
    pub fn next(self, direction: i64) -> Self {
        let index = match self {
            Self::Manual => 0,
            Self::Light => 1,
            Self::Standard => 2,
            Self::Strong => 3,
            Self::Custom => {
                return if direction >= 0 {
                    Self::Manual
                } else {
                    Self::Strong
                }
            }
        };
        let next = (index + direction).rem_euclid(SPEED_ASSIST_MODES.len() as i64) as usize;
        Self::from_tag(SPEED_ASSIST_MODES[next]).unwrap_or(Self::Strong)
    }
}

/// The player's assist fields set aside while a co-driver drives, and the
/// values the co-driver was handed, so a change the player makes during
/// the shift can be told apart and kept.
#[derive(Clone, Debug, PartialEq)]
pub struct AssistStash {
    own: AssistSet,
    lent: AssistSet,
}

/// Every field a co-driver handover changes.
#[derive(Clone, Debug, PartialEq)]
struct AssistSet {
    values: [AssistValue; 9],
    descent_speed_control: String,
    lane_keeping: String,
    preset: String,
    steering_assist_mode: String,
    speed_assist_mode: String,
    automatic_transmission: bool,
}

impl Settings {
    /// The preset fields' current values, in DRIVING_ASSIST_FIELDS order.
    pub fn assist_values(&self) -> [AssistValue; 9] {
        [
            Flag(self.automatic_emergency_braking),
            Flag(self.lane_departure_warning),
            Flag(self.stop_and_go_assist),
            Mode(static_mode(&self.descent_speed_control)),
            Flag(self.exit_speed_assist),
            Flag(self.destination_approach_assist),
            Flag(self.curve_speed_assist),
            Flag(self.route_transition_assist),
            Mode(static_mode(&self.lane_keeping)),
        ]
    }

    /// `getattr(settings, field)` for one preset field.
    pub fn assist_value(&self, field: &str) -> Option<AssistValue> {
        DRIVING_ASSIST_FIELDS
            .iter()
            .position(|name| *name == field)
            .map(|index| self.assist_values()[index])
    }

    /// `setattr(settings, field, value)` for one preset field; `false` when
    /// `field` is not a preset field or `value` is the wrong shape for it.
    pub fn set_assist_value(&mut self, field: &str, value: AssistValue) -> bool {
        match (field, value) {
            ("automatic_emergency_braking", Flag(v)) => self.automatic_emergency_braking = v,
            ("lane_departure_warning", Flag(v)) => self.lane_departure_warning = v,
            ("stop_and_go_assist", Flag(v)) => self.stop_and_go_assist = v,
            ("descent_speed_control", Mode(v)) => self.descent_speed_control = v.to_string(),
            ("exit_speed_assist", Flag(v)) => self.exit_speed_assist = v,
            ("destination_approach_assist", Flag(v)) => self.destination_approach_assist = v,
            ("curve_speed_assist", Flag(v)) => self.curve_speed_assist = v,
            ("route_transition_assist", Flag(v)) => self.route_transition_assist = v,
            ("lane_keeping", Mode(v)) => self.lane_keeping = v.to_string(),
            _ => return false,
        }
        true
    }

    /// Hand every assist to a co-driver taking the wheel: the "all" preset's
    /// values, with the player's own set aside. Idempotent.
    pub fn hand_assists_to_co_driver(&mut self) {
        if self.co_driver_stash.is_some() {
            return;
        }
        let own = self.assist_snapshot();
        self.automatic_transmission = true;
        if let Some(values) = driving_assist_preset("all") {
            for (field, value) in DRIVING_ASSIST_FIELDS.iter().zip(values.iter()) {
                self.set_assist_value(field, *value);
            }
        }
        self.co_driver_stash = Some(Box::new(AssistStash {
            own,
            lent: self.assist_snapshot(),
        }));
    }

    /// Put the player's own assists back. Idempotent. A field the player
    /// changed during the shift keeps the change; the rest go back to what
    /// they were before the co-driver took the wheel.
    pub fn take_assists_back(&mut self) {
        let Some(stash) = self.co_driver_stash.take() else {
            return;
        };
        let now = self.assist_snapshot();
        for (i, field) in DRIVING_ASSIST_FIELDS.iter().enumerate() {
            if now.values[i] == stash.lent.values[i] {
                self.set_assist_value(field, stash.own.values[i]);
            }
        }
        if now.descent_speed_control == stash.lent.descent_speed_control {
            self.descent_speed_control = stash.own.descent_speed_control;
        }
        if now.lane_keeping == stash.lent.lane_keeping {
            self.lane_keeping = stash.own.lane_keeping;
        }
        if now.automatic_transmission == stash.lent.automatic_transmission {
            self.automatic_transmission = stash.own.automatic_transmission;
        }
        if now.preset == stash.lent.preset {
            self.driving_assistance_preset = stash.own.preset;
        }
        if now.steering_assist_mode == stash.lent.steering_assist_mode {
            self.steering_assist_mode = stash.own.steering_assist_mode;
        }
        if now.speed_assist_mode == stash.lent.speed_assist_mode {
            self.speed_assist_mode = stash.own.speed_assist_mode;
        }
        if now.values != stash.lent.values {
            // Some of the player's own changes stand: name the preset the
            // fields now make up.
            self.refresh_driving_assistance_preset();
        }
    }

    fn assist_snapshot(&self) -> AssistSet {
        AssistSet {
            values: self.assist_values(),
            descent_speed_control: self.descent_speed_control.clone(),
            lane_keeping: self.lane_keeping.clone(),
            preset: self.driving_assistance_preset.clone(),
            steering_assist_mode: self.steering_assist_mode.clone(),
            speed_assist_mode: self.speed_assist_mode.clone(),
            automatic_transmission: self.automatic_transmission,
        }
    }

    /// The settings as the player chose them: these, with any co-driver
    /// override undone. What the settings file holds.
    pub fn player_owned(&self) -> std::borrow::Cow<'_, Settings> {
        if self.co_driver_stash.is_none() {
            return std::borrow::Cow::Borrowed(self);
        }
        let mut own = self.clone();
        own.take_assists_back();
        std::borrow::Cow::Owned(own)
    }

    /// Set every preset field to the named preset's values. `false` (and no
    /// change) for a name that is not a preset -- Python raised `KeyError`.
    pub fn apply_driving_assistance_preset(&mut self, preset: &str) -> bool {
        let Some(values) = driving_assist_preset(preset) else {
            return false;
        };
        for (field, value) in DRIVING_ASSIST_FIELDS.iter().zip(values.iter()) {
            self.set_assist_value(field, *value);
        }
        self.driving_assistance_preset = preset.to_string();
        self.refresh_assist_modes();
        true
    }

    /// Apply an independent steering-assist mode without touching speed assists.
    pub fn apply_steering_assist_mode(&mut self, mode: &str) -> bool {
        let lane_keeping = match mode {
            "manual" => "off",
            "partial" => "partial",
            "full" => "full",
            _ => return false,
        };
        self.lane_keeping = lane_keeping.to_string();
        self.lane_departure_warning = true;
        self.steering_assist_mode = mode.to_string();
        self.refresh_driving_assistance_preset();
        true
    }

    /// Apply an independent speed-assist mode without touching steering assists.
    pub fn apply_speed_assist_mode(&mut self, mode: &str) -> bool {
        match mode {
            "manual" => {
                self.automatic_emergency_braking = false;
                self.stop_and_go_assist = false;
                self.descent_speed_control = "off".to_string();
                self.exit_speed_assist = false;
                self.destination_approach_assist = false;
                self.curve_speed_assist = false;
                self.route_transition_assist = false;
            }
            "light" => {
                self.automatic_emergency_braking = true;
                self.stop_and_go_assist = true;
                self.descent_speed_control = "realistic".to_string();
                self.exit_speed_assist = true;
                self.destination_approach_assist = false;
                self.curve_speed_assist = true;
                self.route_transition_assist = true;
            }
            "standard" => {
                self.automatic_emergency_braking = true;
                self.stop_and_go_assist = true;
                self.descent_speed_control = "balanced".to_string();
                self.exit_speed_assist = true;
                self.destination_approach_assist = true;
                self.curve_speed_assist = true;
                self.route_transition_assist = true;
            }
            "strong" => {
                self.automatic_emergency_braking = true;
                self.stop_and_go_assist = true;
                self.descent_speed_control = "interactive".to_string();
                self.exit_speed_assist = true;
                self.destination_approach_assist = true;
                self.curve_speed_assist = true;
                self.route_transition_assist = true;
            }
            _ => return false,
        }
        self.speed_assist_mode = mode.to_string();
        self.refresh_driving_assistance_preset();
        true
    }

    /// Derive the independent steering and speed assist mode labels from the
    /// underlying assist fields.
    pub fn refresh_assist_modes(&mut self) {
        self.steering_assist_mode = if self.lane_departure_warning {
            match self.lane_keeping.as_str() {
                "off" => "manual",
                "partial" => "partial",
                "full" => "full",
                _ => "custom",
            }
        } else {
            "custom"
        }
        .to_string();

        let all_speed_off = !self.automatic_emergency_braking
            && !self.stop_and_go_assist
            && self.descent_speed_control == "off"
            && !self.exit_speed_assist
            && !self.destination_approach_assist
            && !self.curve_speed_assist
            && !self.route_transition_assist;
        let core_speed_on = self.automatic_emergency_braking
            && self.stop_and_go_assist
            && self.exit_speed_assist
            && self.curve_speed_assist
            && self.route_transition_assist;

        self.speed_assist_mode = if all_speed_off {
            "manual"
        } else if core_speed_on
            && !self.destination_approach_assist
            && self.descent_speed_control == "realistic"
        {
            "light"
        } else if core_speed_on
            && self.destination_approach_assist
            && self.descent_speed_control == "balanced"
        {
            "standard"
        } else if core_speed_on
            && self.destination_approach_assist
            && self.descent_speed_control == "interactive"
        {
            "strong"
        } else {
            "custom"
        }
        .to_string();
    }

    /// Re-derive the preset row from the real fields: the one preset whose
    /// mapping matches exactly, or "custom".
    pub fn refresh_driving_assistance_preset(&mut self) -> &'static str {
        self.refresh_assist_modes();
        let values = self.assist_values();
        let mut matches = DRIVING_ASSIST_PRESETS
            .iter()
            .filter(|(_, mapping)| *mapping == values)
            .map(|(name, _)| *name);
        let name = match (matches.next(), matches.next()) {
            (Some(only), None) => only,
            _ => "custom",
        };
        self.driving_assistance_preset = name.to_string();
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steering_and_speed_mode_cycling() {
        assert_eq!(
            SteeringAssistMode::Manual.next(1),
            SteeringAssistMode::Partial
        );
        assert_eq!(
            SteeringAssistMode::Partial.next(1),
            SteeringAssistMode::Full
        );
        assert_eq!(SteeringAssistMode::Full.next(1), SteeringAssistMode::Manual);
        assert_eq!(
            SteeringAssistMode::Manual.next(-1),
            SteeringAssistMode::Full
        );
        assert_eq!(
            SteeringAssistMode::Custom.next(1),
            SteeringAssistMode::Manual
        );

        assert_eq!(SpeedAssistMode::Manual.next(1), SpeedAssistMode::Light);
        assert_eq!(SpeedAssistMode::Light.next(1), SpeedAssistMode::Standard);
        assert_eq!(SpeedAssistMode::Standard.next(1), SpeedAssistMode::Strong);
        assert_eq!(SpeedAssistMode::Strong.next(1), SpeedAssistMode::Manual);
        assert_eq!(SpeedAssistMode::Manual.next(-1), SpeedAssistMode::Strong);
        assert_eq!(SpeedAssistMode::Custom.next(1), SpeedAssistMode::Manual);
    }

    #[test]
    fn independent_steering_and_speed_toggles_pair_cleanly() {
        let mut s = Settings::default();
        assert_eq!(s.driving_assistance_preset, "all");
        assert_eq!(s.steering_assist_mode, "full");
        assert_eq!(s.speed_assist_mode, "strong");

        // Pair manual speed/jake braking with full lane-centering.
        assert!(s.apply_speed_assist_mode("manual"));
        assert_eq!(s.speed_assist_mode, "manual");
        assert_eq!(s.steering_assist_mode, "full");
        assert_eq!(s.lane_keeping, "full");
        assert_eq!(s.descent_speed_control, "off");
        assert!(!s.automatic_emergency_braking);
        assert!(!s.curve_speed_assist);
        assert_eq!(s.driving_assistance_preset, "custom");

        // Pair adaptive/strong speed with manual steering.
        assert!(s.apply_speed_assist_mode("strong"));
        assert!(s.apply_steering_assist_mode("manual"));
        assert_eq!(s.speed_assist_mode, "strong");
        assert_eq!(s.steering_assist_mode, "manual");
        assert_eq!(s.lane_keeping, "off");
        assert_eq!(s.descent_speed_control, "interactive");
        assert!(s.automatic_emergency_braking);
        assert_eq!(s.driving_assistance_preset, "custom");

        // Presets still work and re-sync both independent modes.
        assert!(s.apply_driving_assistance_preset("balanced"));
        assert_eq!(s.steering_assist_mode, "partial");
        assert_eq!(s.speed_assist_mode, "standard");
        assert!(s.apply_driving_assistance_preset("realistic"));
        assert_eq!(s.steering_assist_mode, "manual");
        assert_eq!(s.speed_assist_mode, "light");
    }

    #[test]
    fn legacy_save_migrates_independent_assist_modes_cleanly() {
        let raw: serde_json::Map<String, serde_json::Value> = serde_json::from_str(
            r#"{
                "driving_assistance_preset": "balanced",
                "automatic_emergency_braking": true,
                "lane_departure_warning": true,
                "stop_and_go_assist": true,
                "descent_speed_control": "balanced",
                "exit_speed_assist": true,
                "destination_approach_assist": true,
                "curve_speed_assist": true,
                "route_transition_assist": true,
                "lane_keeping": "partial"
            }"#,
        )
        .unwrap();
        let migrated = Settings::from_dict(Some(&raw));
        assert_eq!(migrated.driving_assistance_preset, "balanced");
        assert_eq!(migrated.steering_assist_mode, "partial");
        assert_eq!(migrated.speed_assist_mode, "standard");
    }
}
