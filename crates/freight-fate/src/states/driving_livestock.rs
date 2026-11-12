//! Live cattle from the driver's seat.
//!
//! The five machines live in `ff_core::sim::livestock`; this is where the
//! player meets them. The footing is heard, not announced: each state has
//! its own sound behind the cab, and the load readout names it on request.
//! The 28-hour limit, the hours-of-service circles and the first hot stop
//! each say one line when they change. Every line reports a change that has
//! just happened.
//!
//! The cattle recordings are not in every sound pack yet. Until they are,
//! the two footing states that cost condition (losing footing, one down)
//! fall back to one short line each, the way the tank layer falls back when
//! its surge sounds are missing.

use ff_core::data::world_constants::{lookup, LOCATION_TYPE_LABELS};
use ff_core::sim::livestock::{
    Exemption, Footing, Heat, LivestockChange, LivestockLoad, LivestockTick, LoadPhase,
};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::audio::CH_SURGE;
use crate::speech::EventPriority;
use crate::states::driving::DrivingState;

/// The cattle's bed shares the tank's channel: a load is a tank or cattle,
/// never both.
const CH_CATTLE: u32 = CH_SURGE;
const CATTLE_BED_GAIN: f64 = 0.45;
const CATTLE_HIT_GAIN: f64 = 0.85;

/// The bed for each footing state, and whether a one-shot marks the entry.
fn bed_for(footing: Footing) -> &'static str {
    match footing {
        Footing::Settled => "livestock/settled_loop",
        Footing::Restless => "livestock/restless_loop",
        Footing::Scrambling => "livestock/scramble_loop",
        Footing::OneDown => "livestock/down_loop",
    }
}

/// "sale barn", "feed yard", "beef plant".
fn place_label(facility_type: &str) -> &'static str {
    lookup(LOCATION_TYPE_LABELS, facility_type).unwrap_or("shipper")
}

impl DrivingState {
    pub fn livestock(&self) -> Option<&LivestockLoad> {
        self.trip.truck.livestock.as_ref()
    }

    /// Whether the cattle recordings are in this build, checked once.
    fn livestock_audio_ready(&mut self, ctx: &mut GameContext) -> bool {
        match self.livestock_audio_ok {
            Some(ready) => ready,
            None => {
                let ready = ctx.audio.has_asset("livestock/settled_loop");
                self.livestock_audio_ok = Some(ready);
                ready
            }
        }
    }

    /// One frame of the cattle on the road.
    pub fn update_livestock(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.trip.truck.livestock.is_none() {
            return;
        }
        if !self.livestock_departure_said {
            self.livestock_departure_said = true;
            self.say_livestock_departure(ctx);
        }
        let (lat, lon) = self.trip.latlon_at(None);
        let truck = &self.trip.truck;
        let tick = LivestockTick {
            real_s: dt,
            game_min: dt * self.trip.effective_time_scale() / 60.0,
            decel_g: truck.last_decel_g,
            lateral_g: truck.roll_lateral_g(),
            speed_mph: truck.speed_mph(),
            outside_temp_f: truck.ambient_temp_c * 9.0 / 5.0 + 32.0,
            lat,
            lon,
        };
        let Some(load) = self.trip.truck.livestock.as_mut() else {
            return;
        };
        let step = load.tick(tick);
        if step.condition_pct > 0.0 {
            self.trip.truck.add_cargo_damage(step.condition_pct);
        }
        self.voice_livestock_changes(ctx, &step.changes, false);
        self.update_livestock_bed(ctx);
    }

    /// Time spent parked off the physics -- a rest, a repair, a wait. The
    /// limit, the heat and an animal down all keep costing through it.
    pub fn advance_livestock_parked(&mut self, ctx: &mut GameContext, minutes: f64) {
        if self.trip.truck.livestock.is_none() || minutes <= 0.0 {
            return;
        }
        let (lat, lon) = self.trip.latlon_at(None);
        let temp_f = self.trip.truck.ambient_temp_c * 9.0 / 5.0 + 32.0;
        let Some(load) = self.trip.truck.livestock.as_mut() else {
            return;
        };
        let step = load.tick(LivestockTick::parked(minutes, temp_f, lat, lon));
        if step.condition_pct > 0.0 {
            self.trip.truck.add_cargo_damage(step.condition_pct);
        }
        self.voice_livestock_changes(ctx, &step.changes, true);
    }

    /// Walk the trailer: what the driver found, and the animal that was
    /// down got back on its feet.
    pub fn check_the_cattle(&mut self) -> Option<String> {
        let load = self.trip.truck.livestock.as_mut()?;
        Some(if load.check() {
            "1 down in the rear compartment. You got it back on its feet.".to_string()
        } else {
            "All standing.".to_string()
        })
    }

    fn say_livestock_departure(&mut self, ctx: &mut GameContext) {
        let Some(load) = self.livestock() else {
            return;
        };
        if load.exemption != Exemption::NearPickup {
            return;
        }
        let line = format!(
            "Your hours of service do not count within 150 air miles of the {}.",
            place_label(&self.job.origin_type)
        );
        ctx.say_event_with(
            line,
            SayEvent::queued()
                .interrupt(false)
                .priority(EventPriority::Important)
                .category(SpeechCategory::Status),
        );
    }

    fn voice_livestock_changes(
        &mut self,
        ctx: &mut GameContext,
        changes: &[LivestockChange],
        parked: bool,
    ) {
        let audio = self.livestock_audio_ready(ctx);
        // A long rest can pass several limit marks; only the last is news.
        let last_limit = changes.iter().rev().find_map(|change| match change {
            LivestockChange::Limit(mark) => mark.warning(),
            _ => None,
        });
        for change in changes {
            match *change {
                LivestockChange::Footing { to, .. } => {
                    if audio && !parked {
                        if to == Footing::Scrambling {
                            ctx.audio
                                .play_with("livestock/gate_hit", CATTLE_HIT_GAIN, 0.0);
                            if !self.livestock_hint_said {
                                self.livestock_hint_said = true;
                                ctx.say_event_with(
                                    "That clatter is the cattle losing their footing. Brake \
                                     and steer more gently.",
                                    SayEvent::queued()
                                        .interrupt(false)
                                        .category(SpeechCategory::Coaching),
                                );
                            }
                        }
                        continue;
                    }
                    let line = match to {
                        Footing::Scrambling => "Cattle losing their footing.",
                        Footing::OneDown => "One of the cattle is down.",
                        _ => continue,
                    };
                    ctx.say_event_with(
                        line,
                        SayEvent::new()
                            .priority(EventPriority::Important)
                            .category(SpeechCategory::Safety),
                    );
                }
                LivestockChange::Exemption { from, to } => {
                    let line = match (from, to) {
                        (Exemption::NearPickup, Exemption::Counting) => format!(
                            "150 air miles from the {}. Your hours of service are counting.",
                            place_label(&self.job.origin_type)
                        ),
                        (Exemption::Counting, Exemption::NearDelivery) => format!(
                            "150 air miles from the {}. Your hours of service have stopped \
                             counting.",
                            place_label(&self.job.destination_type)
                        ),
                        // Straight from one circle to the other: nothing
                        // changed for the driver. Loading and unloading are
                        // said where they happen.
                        _ => continue,
                    };
                    ctx.say_event_with(
                        line,
                        SayEvent::queued()
                            .interrupt(false)
                            .priority(EventPriority::Important)
                            .category(SpeechCategory::Status),
                    );
                }
                LivestockChange::Heat { to, .. } => {
                    let Some(load) = self.trip.truck.livestock.as_mut() else {
                        continue;
                    };
                    if to == Heat::HotStanding && !load.hot_stop_said {
                        load.hot_stop_said = true;
                        let temp_f = self.trip.truck.ambient_temp_c * 9.0 / 5.0 + 32.0;
                        ctx.say_event_with(
                            format!(
                                "{:.0} degrees. The cattle heat up while the truck stands.",
                                temp_f
                            ),
                            SayEvent::queued()
                                .interrupt(false)
                                .priority(EventPriority::Important)
                                .category(SpeechCategory::Safety),
                        );
                    }
                }
                LivestockChange::Limit(_) | LivestockChange::Phase(_) => {}
            }
        }
        if let Some(line) = last_limit {
            ctx.say_event_with(
                line,
                SayEvent::queued()
                    .interrupt(false)
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Status),
            );
        }
    }

    /// The bed behind the cab follows the footing.
    fn update_livestock_bed(&mut self, ctx: &mut GameContext) {
        if !self.livestock_audio_ready(ctx) {
            return;
        }
        let Some(footing) = self.livestock().map(|load| load.footing) else {
            return;
        };
        let key = bed_for(footing);
        if self.livestock_bed != Some(key) {
            ctx.audio
                .start_loop_with(CH_CATTLE, key, CATTLE_BED_GAIN, 400);
            self.livestock_bed = Some(key);
        }
    }

    /// Drop the bed on any transition out of driving; the next frame of
    /// driving starts it again.
    pub fn stop_livestock_cues(&mut self, ctx: &mut GameContext) {
        if self.livestock_bed.take().is_some() {
            ctx.audio.stop_loop_with(CH_CATTLE, 200);
        }
    }

    /// "19 hours left on the 28-hour limit." or "Past the 28-hour limit."
    pub fn livestock_limit_left_text(&self) -> String {
        let Some(load) = self.livestock() else {
            return String::new();
        };
        let left = load.limit_left_min();
        if load.phase == LoadPhase::OverLimit || left <= 0.0 {
            "Past the 28-hour limit.".to_string()
        } else {
            format!("{} left on the 28-hour limit.", hours_text(left / 60.0))
        }
    }

    /// The load readout: "Cattle settled. 9 hours aboard, 19 hours left."
    pub fn livestock_status_clause(&self) -> String {
        let Some(load) = self.livestock() else {
            return String::new();
        };
        let aboard = load.hours_aboard();
        let aboard_text = hours_text(aboard);
        let limit = if load.phase == LoadPhase::OverLimit || load.limit_left_min() <= 0.0 {
            "past the 28-hour limit".to_string()
        } else {
            format!("{} left", hours_text(load.limit_left_min() / 60.0))
        };
        format!(
            "{}. {} head. {aboard_text} aboard, {limit}.",
            load.footing.spoken(),
            load.head
        )
    }
}

/// "9 hours", "1 hour", "40 minutes".
fn hours_text(hours: f64) -> String {
    let hours = hours.max(0.0);
    if hours < 1.0 {
        let minutes = (hours * 60.0).round().max(1.0) as i64;
        return if minutes == 1 {
            "1 minute".to_string()
        } else {
            format!("{minutes} minutes")
        };
    }
    let whole = hours.floor() as i64;
    if whole == 1 {
        "1 hour".to_string()
    } else {
        format!("{whole} hours")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hours_read_in_whole_units() {
        assert_eq!(hours_text(0.0), "1 minute");
        assert_eq!(hours_text(0.5), "30 minutes");
        assert_eq!(hours_text(1.2), "1 hour");
        assert_eq!(hours_text(9.9), "9 hours");
    }

    #[test]
    fn the_places_have_their_spoken_names() {
        assert_eq!(place_label("sale_barn"), "sale barn");
        assert_eq!(place_label("feed_yard"), "feed yard");
        assert_eq!(place_label("beef_plant"), "beef plant");
    }
}
