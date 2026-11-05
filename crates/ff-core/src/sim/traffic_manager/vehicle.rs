//! The bubble's vehicles and the situations they raise (the data half of
//! `freight_fate/sim/traffic_manager.py`).

use crate::sim::trip_models::NPCVehicle;
use crate::speech_text::SpokenMessage;

/// One NPC in the traffic bubble.
///
/// `lane` is the absolute lane index (0 = right lane, counting leftward),
/// matching the player's `LaneKeeping.lane`. `relative_lane` keeps the
/// legacy spoken-text surface (negative = left of the player) and is
/// recomputed against the player's lane every manager update.
#[derive(Debug, Clone, PartialEq)]
pub struct TrafficVehicle {
    pub key: String,
    pub position_mi: f64,
    pub speed_mph: f64,
    pub target_speed_mph: f64,
    pub relative_lane: i64,
    pub intent: String,
    pub vehicle_class: String,
    pub length_mi: f64,
    pub lane: i64,
    /// The route mile this vehicle leaves the highway at, for bubble
    /// traffic: without it a slower vehicle ahead was permanent.
    pub exit_at_mi: Option<f64>,
    /// How this driver runs relative to whatever is posted, as a share of
    /// it (1.0 is the limit itself). Drawn once from the intent's band;
    /// `None` for a vehicle whose speed did not come from a draw at all (a
    /// marked unit, an injected jam, a vehicle a test placed by hand), which
    /// is left alone.
    ///
    /// The speed itself is NOT a property of the vehicle: the number posted
    /// changes along a leg -- through every town a US route passes -- and a
    /// speed drawn once at the spawn cell and kept forever is a town speed
    /// carried out onto the open road, or a highway speed carried into town.
    /// Keeping the SHARE and re-reading the posting under the wheels is what
    /// makes a vehicle drive the road it is on, at a pace that fits it.
    pub limit_share: Option<f64>,
    /// The limiter this vehicle's machine carries, drawn once from its
    /// class's band. `None` for a class that is not governed.
    pub governor_mph: Option<f64>,
    /// Conditions coming off the top that belong to this vehicle rather than
    /// to the road. The bubble draws none (rush hour is denser, not slower;
    /// a jam's pace comes from its zone), so this is for a vehicle placed by
    /// hand. Weather is read live instead, so a clearing sky lifts the whole
    /// road rather than only what spawns next.
    pub slowdown_mph: f64,
}

impl TrafficVehicle {
    pub fn new(
        key: &str,
        position_mi: f64,
        speed_mph: f64,
        target_speed_mph: f64,
        relative_lane: i64,
        intent: &str,
        vehicle_class: &str,
    ) -> Self {
        TrafficVehicle {
            key: key.to_string(),
            position_mi,
            speed_mph,
            target_speed_mph,
            relative_lane,
            intent: intent.to_string(),
            vehicle_class: vehicle_class.to_string(),
            length_mi: 0.25,
            lane: 0,
            exit_at_mi: None,
            limit_share: None,
            governor_mph: None,
            slowdown_mph: 0.0,
        }
    }

    /// Record what the speed draw decided, so the target can be re-read
    /// against the road the vehicle is on rather than the road it was drawn
    /// on. See [`TrafficVehicle::limit_share`].
    pub fn with_speed_draw(
        mut self,
        limit_share: f64,
        governor_mph: Option<f64>,
        slowdown_mph: f64,
    ) -> Self {
        self.limit_share = Some(limit_share);
        self.governor_mph = governor_mph;
        self.slowdown_mph = slowdown_mph;
        self
    }

    pub fn with_lane(mut self, lane: i64) -> Self {
        self.lane = lane;
        self
    }

    pub fn with_exit_at(mut self, exit_at_mi: Option<f64>) -> Self {
        self.exit_at_mi = exit_at_mi;
        self
    }

    pub fn at_mi(&self) -> f64 {
        self.position_mi
    }

    pub fn end_mi(&self) -> f64 {
        self.position_mi + self.length_mi
    }

    pub fn lane_text(&self) -> &'static str {
        if self.relative_lane < 0 {
            "left lane"
        } else if self.relative_lane > 0 {
            "right lane"
        } else {
            "your lane"
        }
    }

    /// The legacy NPC behavior name for this intent.
    pub fn behavior(&self) -> String {
        match self.intent.as_str() {
            "cruising" => "steady_truck",
            "following" => "slow_car",
            "merging" => "merging_vehicle",
            "braking" => "braking_traffic",
            "passing" => "passing_vehicle",
            "patrolling" => "marked_unit",
            other => other,
        }
        .to_string()
    }

    /// What the hazard call names this vehicle as. An ordinary vehicle the
    /// truck is closing on is a slow one of its own class: the old labels
    /// called every cruiser "steady truck traffic" and every slow-role
    /// vehicle a "slow car", so the call said "Slow car right ahead" about a
    /// semi (census sweep, 2026-11-04).
    pub fn reason(&self) -> String {
        if self.vehicle_class == "state trooper" {
            return "state trooper ahead".to_string();
        }
        match self.intent.as_str() {
            "cruising" | "following" => {
                let class = self.vehicle_class.trim();
                let class = if class.is_empty() { "vehicle" } else { class };
                format!("slow {class} ahead")
            }
            "merging" => "merging traffic".to_string(),
            "braking" => "brake lights ahead".to_string(),
            "passing" => "passing traffic".to_string(),
            "patrolling" => "marked unit ahead".to_string(),
            _ => "traffic ahead".to_string(),
        }
    }

    /// The vehicle and what it is doing, for a status row or navigation cue
    /// that supplies its own distance grammar. Unlike the legacy
    /// [`Self::reason`], this keeps the class the traffic model actually drew.
    pub fn status_label(&self) -> String {
        let class = self.vehicle_class.trim();
        let class = if class.is_empty() { "vehicle" } else { class };
        if class == "state trooper" {
            return class.to_string();
        }
        if self.intent == "merging" && self.lane < 0 {
            return format!("Merging {class} on the right ramp");
        }
        match self.intent.as_str() {
            "cruising" => class.to_string(),
            "following" => format!("Slow {class}"),
            "merging" => format!("Merging {class}"),
            "braking" => format!("Brake lights, {class}"),
            _ => class.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::TrafficVehicle;

    #[test]
    fn status_label_matches_traffic_cue_vocabulary() {
        let label = |intent| {
            TrafficVehicle::new("test", 0.0, 0.0, 0.0, 0, intent, "box truck").status_label()
        };

        assert_eq!(label("following"), "Slow box truck");
        assert_eq!(label("merging"), "Merging box truck");
        assert_eq!(label("braking"), "Brake lights, box truck");
        assert_eq!(label("cruising"), "box truck");
        assert_eq!(label("passing"), "box truck");
    }

    #[test]
    fn yielding_merger_status_names_the_right_ramp() {
        let vehicle =
            TrafficVehicle::new("test", 0.3, 42.0, 42.0, 1, "merging", "car").with_lane(-1);

        assert_eq!(vehicle.status_label(), "Merging car on the right ramp");
    }
}

impl From<NPCVehicle> for TrafficVehicle {
    /// The trip's legacy `NPCVehicle` on the bubble's runtime surface: the
    /// behavior maps back to an intent and there is no exit mile to read.
    fn from(npc: NPCVehicle) -> Self {
        TrafficVehicle {
            key: npc.key.clone(),
            position_mi: npc.position_mi,
            speed_mph: npc.speed_mph,
            target_speed_mph: npc.target_speed_mph,
            relative_lane: npc.relative_lane,
            intent: npc.intent().to_string(),
            vehicle_class: "vehicle".to_string(),
            length_mi: npc.length_mi,
            lane: npc.lane,
            exit_at_mi: None,
            limit_share: None,
            governor_mph: None,
            slowdown_mph: 0.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TrafficSituation {
    pub kind: String,
    pub vehicle: TrafficVehicle,
    pub message: SpokenMessage,
    pub interrupt: bool,
}

/// Why traffic is braking, said in the driver's words, keyed by the Zone
/// reason the trip stamped on that mile. Empty for anything not mile-mapped:
/// phantom waves are real, and an invented crash would be worse than silence
/// (Brandon, 2026-08-20).
pub fn braking_cause_line(reason: &str) -> &'static str {
    BRAKING_CAUSE_LINES
        .iter()
        .find(|(key, _)| *key == reason)
        .map(|(_, line)| *line)
        .unwrap_or("")
}

/// The table [`braking_cause_line`] reads, as the Python `BRAKING_CAUSE_LINES`
/// dict is. A table rather than `match` arms so the KEYS can be enumerated:
/// the adversarial battery checks both directions against the real Zone
/// reasons -- a generated reason with no line here loses its explanation
/// silently, and a key here that no zone ever stamps is dead vocabulary that
/// looks like coverage. Neither direction is checkable against arms.
pub static BRAKING_CAUSE_LINES: &[(&str, &str)] = &[
    ("construction", "Road work is the cause."),
    ("construction merge", "Road work is the cause."),
    ("heavy traffic", "Traffic is backing up ahead."),
];

/// A (start, end, reason, pace) span where traffic has a reason to be on the
/// brakes, handed over by the trip. The Python tuples came in two-, three-
/// and four-element shapes; the missing slots read as no reason / no pace.
#[derive(Debug, Clone, PartialEq)]
pub struct BrakingZone {
    pub start_mi: f64,
    pub end_mi: f64,
    pub reason: String,
    pub pace_mph: Option<f64>,
}

impl BrakingZone {
    pub fn new(start_mi: f64, end_mi: f64, reason: &str, pace_mph: Option<f64>) -> Self {
        BrakingZone {
            start_mi,
            end_mi,
            reason: reason.to_string(),
            pace_mph,
        }
    }

    pub fn span(start_mi: f64, end_mi: f64) -> Self {
        BrakingZone::new(start_mi, end_mi, "", None)
    }
}
