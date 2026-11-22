//! Noting what the drive met for the road journal's delivery
//! (`ff_core::sim::road_tally`). Read-only on the drive: nothing here
//! speaks, plays or changes the truck.

use serde_json::Value;

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use ff_core::sim::road_tally::crossed;
use ff_core::sim::trip_models::RoadStop;

/// Below this the truck is parked or creeping out of a lot, and the sky
/// over it is not weather it drove through.
const MOVING_MPH: f64 = 1.0;

/// The career's clean-inspection count (the `inspections_passed` stat that
/// `record_inspection` raises at every one).
pub fn career_inspections(ctx: &GameContext) -> i64 {
    ctx.profile
        .as_ref()
        .and_then(|profile| profile.achievement_stats.get("inspections_passed"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
}

impl DrivingState {
    /// Note this frame's weather, chain law, and the work zones and open
    /// scales the truck passed between `before_mi` and where it is now.
    /// Keys carry the route's ends, so a street chain's mileposts never
    /// stand for the highway's.
    pub fn note_road_tally(&mut self, before_mi: f64) {
        let now_mi = self.trip.position_mi;
        if self.trip.truck.speed_mph() >= MOVING_MPH {
            let key = self.trip.weather.current.key();
            self.road_tally.note_weather(key);
            if self.trip.chain_control.level() > 0 && self.trip.chain_law_area_at(now_mi).is_some()
            {
                self.road_tally.note_chain_law();
            }
        }
        if now_mi <= before_mi {
            return;
        }
        let route = self.road_tally_route();
        let zones: Vec<String> = self
            .trip
            .zones
            .iter()
            .filter(|zone| zone.reason == "construction")
            .filter(|zone| crossed(before_mi, now_mi, zone.start_mi))
            .map(|zone| format!("{route}@{:.2}", zone.start_mi))
            .collect();
        for key in zones {
            self.road_tally.note_work_zone(key);
        }
        let scales: Vec<String> = self
            .trip
            .stops
            .iter()
            .filter(|stop| stop.stop_type == "weigh_station")
            .filter(|stop| crossed(before_mi, now_mi, stop.at_mi))
            .filter(|stop| self.scale_is_open(stop))
            .map(|stop| self.road_tally_scale_key(stop))
            .collect();
        for key in scales {
            self.road_tally.note_scale(key);
        }
    }

    /// A scale checked in at counts once, whether or not the truck's
    /// milepost also crossed it.
    pub fn note_road_tally_scale(&mut self, stop: &RoadStop) {
        let key = self.road_tally_scale_key(stop);
        self.road_tally.note_scale(key);
    }

    /// The highway's ends while the truck is on a stop's streets, since a
    /// scale is checked in at from there.
    fn road_tally_route(&self) -> String {
        let trip = self.highway_trip.as_ref().unwrap_or(&self.trip);
        let cities = &trip.route.cities;
        format!(
            "{}>{}",
            cities.first().map(String::as_str).unwrap_or(""),
            cities.last().map(String::as_str).unwrap_or("")
        )
    }

    fn road_tally_scale_key(&self, stop: &RoadStop) -> String {
        format!(
            "{}@{}",
            self.road_tally_route(),
            self.weigh_station_key(stop)
        )
    }
}
