//! The driving-assist fields: their values, the presets, and lending
//! them to a co-driver for a shift.

use super::{
    driving_assist_preset, static_mode, AssistValue, Settings, DRIVING_ASSIST_FIELDS,
    DRIVING_ASSIST_PRESETS,
};
use AssistValue::{Flag, Mode};

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
        true
    }

    /// Re-derive the preset row from the real fields: the one preset whose
    /// mapping matches exactly, or "custom".
    pub fn refresh_driving_assistance_preset(&mut self) -> &'static str {
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
