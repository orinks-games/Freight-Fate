//! `snapshot()` / `from_snapshot()` and the live-stop round trip
//! (`_PULL_OVER_FIELDS`, `_pull_over_snapshot`, `_restore_pull_over`) from
//! `freight_fate/states/driving.py`.

use serde_json::{json, Map, Value};

use ff_core::models::jobs::{
    remaining_route_hos_plan, ACTIVE_TRIP_FAIRNESS_SLACK, DEADLINE_DISPATCH_MIN_SLACK_H,
};
use ff_core::pyfmt::round_py_n;
use ff_core::sim::hos::limits;
use ff_core::sim::hos::HosClock;

use crate::app::GameContext;
use crate::states::driving_core::*;
use crate::states::driving_events::stop_plan::StopPlanPurpose;

use super::DrivingState;

/// Bumped when the meaning of a snapshot's deadline changes. A snapshot
/// written under an older model gets the one-time fair-deadline floor on
/// resume; one at the current model keeps the deadline exactly as saved.
pub const ACTIVE_TRIP_DEADLINE_MODEL: i64 = 2;

fn f(data: &Map<String, Value>, key: &str, fallback: f64) -> f64 {
    data.get(key).and_then(Value::as_f64).unwrap_or(fallback)
}

fn i(data: &Map<String, Value>, key: &str, fallback: i64) -> i64 {
    data.get(key).and_then(Value::as_i64).unwrap_or(fallback)
}

fn b(data: &Map<String, Value>, key: &str, fallback: bool) -> bool {
    data.get(key).and_then(Value::as_bool).unwrap_or(fallback)
}

fn s(data: &Map<String, Value>, key: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn strings(data: &Map<String, Value>, key: &str) -> Vec<String> {
    data.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .map(|item| match item {
                    Value::String(text) => text.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn warning_levels(data: &Map<String, Value>, key: &str) -> Option<[u8; 3]> {
    let items = data.get(key)?.as_array()?;
    (items.len() == 3).then(|| {
        std::array::from_fn(|index| items[index].as_u64().unwrap_or_default().min(2) as u8)
    })
}

impl DrivingState {
    /// Every field the live stop is judged by, as one object. Kept in one
    /// place so the snapshot and the restore cannot drift apart and quietly
    /// reintroduce the reload-cancels-the-stop exploit.
    fn pull_over_snapshot(&self) -> Value {
        let Some(stage) = self.pull_over.as_ref() else {
            return Value::Null;
        };
        json!({
            "_pull_over": stage,
            "_pull_over_start_mi": self.pull_over_start_mi,
            "_pull_over_signaled": self.pull_over_signaled,
            "_pull_over_over": self.pull_over_over,
            "_pull_over_limit": self.pull_over_limit,
            "_pull_over_kind": self.pull_over_kind,
            "_pull_over_title": self.pull_over_title,
            "_pull_over_summary": self.pull_over_summary,
            "_pull_over_fine": self.pull_over_fine,
            "_pull_over_reputation_hit": self.pull_over_reputation_hit,
            "_pull_over_return": self.pull_over_return,
            "_pull_over_construction_zone": self.pull_over_construction_zone,
            "_pull_over_warning_level": self.pull_over_warning_level,
            "_pull_over_compliance": self.pull_over_compliance,
            "_pull_over_elapsed": self.pull_over_elapsed,
            "_pull_over_prev_mph": self.pull_over_prev_mph,
            "_pull_over_coast_s": self.pull_over_coast_s,
            "_pull_over_signal_boost": self.pull_over_signal_boost,
            "_pull_over_nosignal_hit": self.pull_over_nosignal_hit,
            "_pull_over_grace_s": self.pull_over_grace_s,
            "_pull_over_forced_s": self.pull_over_forced_s,
        })
    }

    /// Bring a stop back exactly as it was, mid-stop compliance and all.
    fn restore_pull_over(&mut self, data: Option<&Value>) {
        let Some(data) = data.and_then(Value::as_object) else {
            return;
        };
        let stage = data.get("_pull_over").and_then(Value::as_str);
        let Some(stage) = stage.filter(|text| !text.is_empty()) else {
            return;
        };
        self.pull_over = Some(stage.to_string());
        self.pull_over_start_mi = f(data, "_pull_over_start_mi", self.pull_over_start_mi);
        self.pull_over_signaled = b(data, "_pull_over_signaled", self.pull_over_signaled);
        self.pull_over_over = f(data, "_pull_over_over", self.pull_over_over);
        self.pull_over_limit = f(data, "_pull_over_limit", self.pull_over_limit);
        if data.contains_key("_pull_over_kind") {
            self.pull_over_kind = s(data, "_pull_over_kind");
        }
        if data.contains_key("_pull_over_title") {
            self.pull_over_title = s(data, "_pull_over_title");
        }
        if data.contains_key("_pull_over_summary") {
            self.pull_over_summary = s(data, "_pull_over_summary");
        }
        self.pull_over_fine = f(data, "_pull_over_fine", self.pull_over_fine);
        self.pull_over_reputation_hit = f(
            data,
            "_pull_over_reputation_hit",
            self.pull_over_reputation_hit,
        );
        if data.contains_key("_pull_over_return") {
            self.pull_over_return = s(data, "_pull_over_return");
        }
        self.pull_over_construction_zone = b(
            data,
            "_pull_over_construction_zone",
            self.pull_over_construction_zone,
        );
        self.pull_over_warning_level = i(
            data,
            "_pull_over_warning_level",
            self.pull_over_warning_level,
        );
        self.pull_over_compliance = f(data, "_pull_over_compliance", self.pull_over_compliance);
        self.pull_over_elapsed = f(data, "_pull_over_elapsed", self.pull_over_elapsed);
        self.pull_over_prev_mph = f(data, "_pull_over_prev_mph", self.pull_over_prev_mph);
        self.pull_over_coast_s = f(data, "_pull_over_coast_s", self.pull_over_coast_s);
        self.pull_over_signal_boost =
            b(data, "_pull_over_signal_boost", self.pull_over_signal_boost);
        self.pull_over_nosignal_hit =
            b(data, "_pull_over_nosignal_hit", self.pull_over_nosignal_hit);
        self.pull_over_grace_s = f(data, "_pull_over_grace_s", self.pull_over_grace_s);
        self.pull_over_forced_s = f(data, "_pull_over_forced_s", self.pull_over_forced_s);
    }

    /// Everything needed to resume this active drive from a save.
    pub fn snapshot(&self, ctx: &GameContext) -> Value {
        let kind = if self.phase == DRIVE_PHASE_PICKUP {
            "pickup_drive"
        } else {
            "delivery"
        };
        let route_kind = if self.phase == DRIVE_PHASE_PICKUP {
            // A relayed load's deadhead: a corridor into the shipper's city
            // with its approach on the end. A plain approach names its own
            // city and no other, however many street legs it has.
            let first = self.route.cities.first();
            if self.route.cities.iter().any(|city| Some(city) != first) {
                "deadhead_approach"
            } else {
                "facility_approach"
            }
        } else {
            "corridor_itinerary"
        };
        let tolls: Vec<Value> = self
            .trip
            .toll_charges
            .iter()
            .map(|charge| json!({"name": charge.name(), "amount": charge.amount}))
            .collect();
        let mut enforcement_events: Vec<String> = self.enforcement_events.iter().cloned().collect();
        enforcement_events.sort();
        let mut jake_zone_grace_used: Vec<String> =
            self.jake_zone_grace_used.iter().cloned().collect();
        jake_zone_grace_used.sort();
        let planned_stop_label = self.trip.planned_stop_label();
        let mut out = Map::new();
        out.insert("kind".to_string(), json!(kind));
        out.insert(
            "deadline_model".to_string(),
            json!(ACTIVE_TRIP_DEADLINE_MODEL),
        );
        out.insert("job".to_string(), json!(job_payload(&self.job)));
        out.insert("route_cities".to_string(), json!(self.route.cities.clone()));
        out.insert("route_kind".to_string(), json!(route_kind));
        out.insert("navigation_schema".to_string(), json!(1));
        out.insert("trailer_refused".to_string(), json!(self.trailer_refused));
        out.insert("trailer_repaired".to_string(), json!(self.trailer_repaired));
        out.insert("trip_seed".to_string(), json!(self.trip_seed));
        out.insert("start_hour".to_string(), json!(self.trip.start_hour));
        // On a road stop's streets the saved place is the highway's, at the
        // stop's exit, where a ramp in progress is saved too.
        let position_mi = match (&self.stop_chain, &self.highway_trip) {
            (Some(_), Some(highway)) => highway.position_mi,
            _ => self.trip.position_mi,
        };
        out.insert("position_mi".to_string(), json!(position_mi));
        out.insert("game_minutes".to_string(), json!(self.trip.game_minutes));
        out.insert("toll_charges".to_string(), json!(tolls));
        out.insert("start_damage".to_string(), json!(self.start_damage));
        out.insert("damage_band".to_string(), json!(self.damage_band));
        out.insert(
            "worst_damage_band".to_string(),
            json!(self.worst_damage_band),
        );
        out.insert(
            "preventable_damage_pct".to_string(),
            json!(self.trip.truck.preventable_damage_pct),
        );
        out.insert(
            "equipment_event_logged".to_string(),
            json!(self.equipment_event_logged),
        );
        out.insert(
            "cargo_damage_pct".to_string(),
            json!(self.trip.truck.cargo_damage_pct),
        );
        out.insert("cargo_cue_at".to_string(), json!(self.cargo_cue_at));
        out.insert(
            "cargo_coaching_said".to_string(),
            json!(self.cargo_coaching_said),
        );
        out.insert("limp_cap_mph".to_string(), json!(self.limp_cap_mph));
        out.insert(
            "out_of_service_creep_s".to_string(),
            json!(self.out_of_service_creep_s),
        );
        out.insert("start_wear".to_string(), json!({"tire": self.start_tire_wear, "brake": self.start_brake_wear, "engine": self.start_engine_wear}));
        out.insert(
            "maintenance_levels".to_string(),
            json!(self.maintenance_levels),
        );
        out.insert("rig_buffs".to_string(), json!(self.rig_buffs));
        out.insert("stop_visit".to_string(), json!(self.stop_visit));
        out.insert("drove_automatic".to_string(), json!(self.drove_automatic));
        out.insert(
            "speed_control_armed".to_string(),
            json!(self.speed_control_armed),
        );
        out.insert(
            "speed_control_target_mph".to_string(),
            json!(self.speed_control_target_mph),
        );
        out.insert(
            "air_brake".to_string(),
            json!(self.trip.truck.air_brake_snapshot()),
        );
        out.insert("engine_on".to_string(), json!(self.trip.truck.engine_on));
        out.insert("chains_on".to_string(), json!(self.trip.truck.chains_on));
        out.insert(
            "chain_checkpoint_hold".to_string(),
            json!(self.chain_checkpoint_hold),
        );
        // [area, level] pairs; absent on older saves, which re-roll the same
        // seeded staffing and find nothing cited.
        let mut cited: Vec<(i64, i64)> = self.chain_law_cited.iter().copied().collect();
        cited.sort_unstable();
        out.insert("chain_law_cited".to_string(), json!(cited));
        let mut written_up: Vec<&String> = self.written_up.iter().collect();
        written_up.sort_unstable();
        out.insert("written_up".to_string(), json!(written_up));
        let mut staffed: Vec<(i64, i64, bool)> = self
            .chain_checkpoint_staffed
            .iter()
            .map(|(&(area, level), &on)| (area, level, on))
            .collect();
        staffed.sort_unstable();
        out.insert("chain_checkpoint_staffed".to_string(), json!(staffed));
        out.insert(
            "chain_wear_pct".to_string(),
            json!(self.trip.truck.chain_wear_pct),
        );
        out.insert("hos".to_string(), json!(hos_of(ctx).to_dict()));
        out.insert("fatigue".to_string(), json!(profile_of(ctx).fatigue));
        out.insert("hos_fine_count".to_string(), json!(self.hos_fine_count));
        out.insert("enforcement_events".to_string(), json!(enforcement_events));
        out.insert(
            "out_of_service_count".to_string(),
            json!(self.out_of_service_count),
        );
        out.insert("speeding_tickets".to_string(), json!(self.speeding_tickets));
        out.insert(
            "ticket_fines_paid".to_string(),
            json!(self.ticket_fines_paid),
        );
        out.insert(
            "failure_to_stop_count".to_string(),
            json!(self.failure_to_stop_count),
        );
        out.insert(
            "record_events".to_string(),
            json!(self.record_events.clone()),
        );
        out.insert("fatigue_events".to_string(), json!(self.fatigue_events));
        out.insert(
            "jake_zone_grace_used".to_string(),
            json!(jake_zone_grace_used),
        );
        out.insert("pull_over".to_string(), json!(self.pull_over_snapshot()));
        out.insert("jake_zone_fines".to_string(), json!(self.jake_zone_fines));
        out.insert("jake_fines_paid".to_string(), json!(self.jake_fines_paid));
        out.insert("gate_miss_count".to_string(), json!(self.gate_miss_count));
        out.insert("turn_miss_count".to_string(), json!(self.turn_miss_count));
        out.insert("lane_offset".to_string(), json!(self.lane.offset));
        out.insert("lane_index".to_string(), json!(self.lane.lane));
        out.insert("surface_chain".to_string(), json!(self.surface_chain));
        out.insert("departure_chain".to_string(), json!(self.departure_chain));
        out.insert(
            "facility_streets".to_string(),
            json!(self.facility_streets_on),
        );
        out.insert(
            "planned_stop_key".to_string(),
            json!(self.trip.planned_stop_key),
        );
        out.insert(
            "selected_stop_key".to_string(),
            json!(self.selected_stop_key),
        );
        out.insert(
            "selected_stop_break".to_string(),
            json!(self.selected_stop_break),
        );
        out.insert(
            "selected_stop_purpose".to_string(),
            json!(self.selected_stop_purpose.map(StopPlanPurpose::save_key)),
        );
        // Kept for a save opened by an older build, which knows only the name.
        out.insert(
            "planned_stop".to_string(),
            if planned_stop_label.is_empty() {
                Value::Null
            } else {
                Value::String(planned_stop_label)
            },
        );
        if let Some(team) = &self.team {
            out.insert("team".to_string(), team.snapshot(ctx));
        }
        Value::Object(out)
    }

    /// Rebuild a saved active drive; `None` if the snapshot is unreadable.
    pub fn from_snapshot(ctx: &mut GameContext, data: &Value) -> Option<DrivingState> {
        let data = data.as_object()?;
        let job_payload_data = data.get("job").and_then(Value::as_object)?;
        let kind = s(data, "kind");
        let phase = if kind == "pickup_drive" {
            DRIVE_PHASE_PICKUP
        } else {
            DRIVE_PHASE_DELIVERY
        };
        // The Facility streets setting the drive started under; a save from
        // before the setting was driven on the streets.
        let facility_streets = b(data, "facility_streets", true);
        let route = if phase == DRIVE_PHASE_PICKUP {
            let origin = job_payload_data.get("origin").and_then(Value::as_str)?;
            let location = job_payload_data
                .get("origin_location")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let approach = ctx
                .world
                .facility_approach_route_with(origin, location, facility_streets)
                .ok();
            if s(data, "route_kind") == "deadhead_approach" {
                // A relayed load: the corridor the save names, then the
                // shipper's approach rebuilt fresh, joined as it was driven.
                // The approach's own duplicated end city comes off first.
                let mut cities = strings(data, "route_cities");
                while cities.len() > 2 && cities.last() == cities.get(cities.len() - 2) {
                    cities.pop();
                }
                let corridor = ctx.world.route_from_cities(&cities)?;
                match approach {
                    Some(approach) => corridor.then(&approach),
                    None => corridor,
                }
            } else {
                approach?
            }
        } else {
            let cities = strings(data, "route_cities");
            ctx.world.route_from_cities(&cities)?
        };
        // Pre-slug saves store display names; canonicalize before any
        // world lookup so an old trip resumes instead of being dropped.
        let mut job = job_from_payload(job_payload_data)?;
        normalize_job_cities(&mut job, ctx.world);
        let position_mi = f(data, "position_mi", 0.0);
        let game_minutes = f(data, "game_minutes", 0.0);
        // fair_active_deadline is a one-time compatibility floor, but it
        // used to run on every resume: a late run could buy hours simply by
        // saving at a stop and continuing. Snapshots now carry the deadline
        // model they were written under, so the floor is applied once to a
        // save that predates the marker and never again.
        let deadline_model = i(data, "deadline_model", 0);
        if deadline_model < 1 {
            job.deadline_game_h = fair_active_deadline(
                &job,
                &route,
                game_minutes / 60.0,
                position_mi,
                Some(ctx.world),
            );
        }
        // Version 1 snapshots predate the loaded-departure HOS reconciliation.
        // Repair only a still-on-time delivery whose remaining legal route
        // requires sleep and will overrun its saved deadline. This runs once;
        // the resume owner persists the new marker immediately.
        if deadline_model < ACTIVE_TRIP_DEADLINE_MODEL
            && phase == DRIVE_PHASE_DELIVERY
            && limits(&ctx.settings.hos_mode).is_some()
        {
            if let Some(clock) = ctx.profile.as_ref().map(|p| &p.hos) {
                let hours_used = game_minutes / 60.0;
                let hours_left = job.deadline_game_h - hours_used;
                if hours_left > 0.0 {
                    let legal =
                        remaining_route_hos_plan(&route, position_mi, Some(ctx.world), clock);
                    if legal.sleeps > 0 && legal.total_h() > hours_left {
                        let floor = hours_used
                            + legal.total_h() * ACTIVE_TRIP_FAIRNESS_SLACK
                            + DEADLINE_DISPATCH_MIN_SLACK_H;
                        job.deadline_game_h = round_py_n(job.deadline_game_h.max(floor), 1);
                        job.deadline_covers_rest = true;
                    }
                }
            }
        }

        let trip_seed = data.get("trip_seed").and_then(Value::as_i64)?;
        let start_hour = data
            .get("start_hour")
            .and_then(Value::as_f64)
            .unwrap_or_else(|| profile_of(ctx).game_hours % 24.0);
        let mut state =
            DrivingState::new(ctx, job, route, Some(trip_seed), phase, Some(start_hour));
        // Restore the route position before rebuilding the session-only
        // calendar anchor.  A real-time save may be beyond a time-zone
        // crossing, so the departure clock must use the zone at the saved
        // position rather than the route's starting zone.
        state.trip.restore(position_mi, game_minutes);
        if ctx.settings.time_scale == 1.0 {
            let local_start =
                (start_hour + state.trip.current_timezone().offset_h).rem_euclid(24.0);
            profile_mut_of(ctx).sync_calendar_hour_to(local_start);
        }
        state.resumed = true;
        state.facility_streets_on = facility_streets;
        state.start_damage = f(data, "start_damage", state.start_damage);
        // A save from before the damage bands carries neither key: derive
        // the announced band from the damage it does carry, so a resumed
        // limping truck does not re-announce a band the player already
        // heard, and leave the cap to open itself at the resume speed.
        state.damage_band = i(data, "damage_band", state.trip.truck.damage_band() as i64) as i32;
        state.limp_cap_mph = data.get("limp_cap_mph").and_then(Value::as_f64);
        state.out_of_service_creep_s = f(data, "out_of_service_creep_s", 0.0);
        state.worst_damage_band = i(data, "worst_damage_band", state.damage_band as i64) as i32;
        state.trip.truck.preventable_damage_pct = f(data, "preventable_damage_pct", 0.0);
        state.equipment_event_logged = b(data, "equipment_event_logged", false);
        state.trip.truck.cargo_damage_pct = f(data, "cargo_damage_pct", 0.0);
        state.cargo_cue_at = f(data, "cargo_cue_at", 0.0);
        state.cargo_coaching_said = b(data, "cargo_coaching_said", false);
        // Saves from before the wear meters count deltas from the resume
        // point: the truck just loaded the profile's wear, so the run
        // simply reports a little less instead of failing to load.
        let start_wear = data.get("start_wear").and_then(Value::as_object).cloned();
        let start_wear = start_wear.unwrap_or_default();
        state.start_tire_wear = f(&start_wear, "tire", state.trip.truck.tire_wear_pct);
        state.start_brake_wear = f(&start_wear, "brake", state.trip.truck.brake_wear_pct);
        state.start_engine_wear = f(&start_wear, "engine", state.trip.truck.engine_wear_pct);
        state.maintenance_levels = warning_levels(data, "maintenance_levels")
            .unwrap_or_else(|| state.maintenance_wear_levels());
        // Chains stay on the drives across a save; absent on older saves.
        state.trip.truck.chains_on = b(data, "chains_on", false);
        state.chain_checkpoint_hold = b(data, "chain_checkpoint_hold", false);
        state.chain_law_cited = data
            .get("chain_law_cited")
            .and_then(|v| serde_json::from_value::<Vec<(i64, i64)>>(v.clone()).ok())
            .unwrap_or_default()
            .into_iter()
            .collect();
        // Absent on older saves: nothing written up yet.
        state.written_up = data
            .get("written_up")
            .and_then(|v| serde_json::from_value::<Vec<String>>(v.clone()).ok())
            .unwrap_or_default()
            .into_iter()
            .collect();
        state.chain_checkpoint_staffed = data
            .get("chain_checkpoint_staffed")
            .and_then(|v| serde_json::from_value::<Vec<(i64, i64, bool)>>(v.clone()).ok())
            .unwrap_or_default()
            .into_iter()
            .map(|(area, level, on)| ((area, level), on))
            .collect();
        // The chain meter rides with the drive: the profile only catches up
        // at a stop, so a set that snapped (or wore) since then came back as
        // it was at the last stop. An older save keeps the profile's value.
        state.trip.truck.chain_wear_pct =
            f(data, "chain_wear_pct", state.trip.truck.chain_wear_pct);
        state.trailer_refused = b(data, "trailer_refused", false);
        state.trailer_repaired = b(data, "trailer_repaired", false);
        state.rig_buffs = data
            .get("rig_buffs")
            .cloned()
            .and_then(|value| serde_json::from_value::<RigBuffs>(value).ok())
            .unwrap_or_default();
        // Parked at a stop through a save: the fuel still pays for the shower.
        state.stop_visit = data
            .get("stop_visit")
            .cloned()
            .and_then(|value| serde_json::from_value(value).ok())
            .unwrap_or_default();
        state.drove_automatic = b(data, "drove_automatic", false);
        // A team load resumes with the player at the wheel.
        state.team = data
            .get("team")
            .and_then(Value::as_object)
            .and_then(|team| crate::states::driving_team::TeamDrive::from_snapshot(ctx, team));
        // "speeding_strikes" was a required snapshot field until the silent
        // at-delivery speeding charge was removed. Snapshots written before
        // that still carry it; the key is simply no longer consulted.
        // Ignored, not migrated: the charge it stood for no longer exists to
        // migrate into.
        let target = data.get("speed_control_target_mph").and_then(Value::as_f64);
        let armed = b(data, "speed_control_armed", false);
        state.restore_speed_control_session(ctx, armed, target);
        // WeatherSystem starts at the profile's pre-trip calendar time, and
        // the profile clock only moves when the run ends. Add the elapsed
        // trip time back so the spoken date, season, and simulated weather
        // use the same instant as the trip clock.
        if state.trip.weather.game_hours.is_some() {
            state.trip.weather.game_hours =
                Some(profile_of(ctx).calendar_game_hours() + game_minutes / 60.0);
        }
        let mut planned_key = data
            .get("planned_stop_key")
            .and_then(Value::as_str)
            .filter(|key| !key.is_empty())
            .map(str::to_string);
        if planned_key.is_none() {
            // Saved before plans carried a stop identity: a bare name cannot
            // say which namesake was meant, so take the soonest reachable.
            let legacy_name = data
                .get("planned_stop")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty());
            planned_key = legacy_name.and_then(|name| state.trip.resolve_stop_key(name));
        }
        state.trip.planned_stop_key = planned_key;
        let selected_key = data
            .get("selected_stop_key")
            .and_then(Value::as_str)
            .filter(|key| !key.is_empty())
            .map(str::to_string);
        state.selected_stop_key = match (&selected_key, &state.trip.planned_stop_key) {
            (Some(selected), Some(planned)) if selected == planned => selected_key,
            _ => None,
        };
        state.selected_stop_break =
            state.selected_stop_key.is_some() && b(data, "selected_stop_break", false);
        state.selected_stop_purpose = state
            .selected_stop_key
            .as_ref()
            .and(data.get("selected_stop_purpose"))
            .and_then(Value::as_str)
            .and_then(StopPlanPurpose::from_save_key);
        let tolls: Vec<Value> = data
            .get("toll_charges")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        state.trip.restore_toll_charges(&tolls);
        if b(data, "surface_chain", false) {
            // The save was made on the facility's street chain: re-enter it
            // (deterministic rebuild; the chain shares the restored toll
            // ledger). If the data no longer offers a chain, fall back to the
            // highway route just short of the destination exit so the player
            // simply takes it again.
            state.destination_exit_taken = true;
            if state.begin_surface_chain(ctx, false) {
                state.trip.restore(position_mi, game_minutes);
            } else {
                state.destination_exit_taken = false;
                let short_of_the_exit = (state.trip.total_miles() - 2.0).max(0.0);
                state.trip.restore(short_of_the_exit, game_minutes);
            }
        } else if b(data, "departure_chain", false) {
            // Saved on the origin facility's outbound streets: re-enter the
            // departure chain at the saved distance. If the data no longer
            // offers one, the highway trip simply starts from the top -- the
            // saved street miles were the first miles anyway.
            if state.begin_departure_chain(ctx, false) {
                state.trip.restore(position_mi, game_minutes);
            }
        }
        state.departure_checked = true;
        // Everything behind the resume point was heard before the save.
        state.settle_enforcement_at(state.trip.position_mi);
        let air_brake = data.get("air_brake").cloned().unwrap_or(Value::Null);
        state
            .trip
            .truck
            .restore_air_brake_snapshot(&air_brake, true);
        if b(data, "engine_on", false) {
            state.trip.truck.start_engine();
        }
        state.air_ready_said = state.trip.truck.air_ready();
        state.low_air_said = state.trip.truck.air_low_warning();
        // Already under the line on resume: do not re-announce a cue the
        // driver heard before the save. Above the line clears the latch.
        state.low_fuel_said = state.trip.truck.fuel_fraction() <= LOW_FUEL_WARN_FRACTION;
        let (range, needed, _) = state.fuel_range_need();
        state.fuel_range_short_said = range < needed;
        state.spring_brake_said = state.trip.truck.spring_brakes_active();
        // HOS and fatigue: absent in pre-1.5 snapshots, defaulting to a
        // fresh clock and a rested driver.
        if let Some(clock) = data.get("hos").map(HosClock::from_dict) {
            profile_mut_of(ctx).hos = clock;
        }
        {
            let profile = profile_mut_of(ctx);
            profile.fatigue = f(data, "fatigue", profile.fatigue).clamp(0.0, 100.0);
        }
        state.hos_fine_count = i(data, "hos_fine_count", 0);
        state.enforcement_events = strings(data, "enforcement_events").into_iter().collect();
        state.out_of_service_count = i(data, "out_of_service_count", 0);
        state.speeding_tickets = i(data, "speeding_tickets", 0);
        state.ticket_fines_paid = f(data, "ticket_fines_paid", 0.0);
        state.failure_to_stop_count = i(data, "failure_to_stop_count", 0);
        state.record_events = strings(data, "record_events");
        state.fatigue_events = i(data, "fatigue_events", 0);
        state.jake_zone_grace_used = strings(data, "jake_zone_grace_used").into_iter().collect();
        state.restore_pull_over(data.get("pull_over"));
        state.jake_zone_fines = i(data, "jake_zone_fines", 0);
        state.jake_fines_paid = f(data, "jake_fines_paid", 0.0);
        state.gate_miss_count = i(data, "gate_miss_count", 0);
        state.turn_miss_count = i(data, "turn_miss_count", 0);
        state.lane.offset = f(data, "lane_offset", 0.0);
        state.lane.lane = i(data, "lane_index", 0).max(0);
        Some(state)
    }
}
