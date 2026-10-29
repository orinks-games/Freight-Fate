//! `Profile.to_dict` / `Profile.from_dict`: the save dict and the tolerant
//! read of it (the serialization half of `profile.py`).

use indexmap::IndexMap;
use serde_json::{Map, Value};

use super::{
    condition, signature_for, ConditionRecord, Profile, SAFETY_RECORD_BASELINE, SAVE_VERSION,
    SIGNATURE_FIELD, SIGNATURE_VERSION, SIGNATURE_VERSION_FIELD,
};
use crate::models::career::Career;
use crate::models::enforcement::{seed_record_from_save, DrivingRecord};
use crate::models::jobs::{py_str, py_truthy};
use crate::models::loyalty::LoyaltyAccount;
use crate::models::market::Market;
use crate::models::money_guard::MoneyGuard;
use crate::models::save_migration::{json_f64, json_i64, migrate_save_data};
use crate::sim::hos::{DutyLog, HosClock};

fn string_list(value: Option<&Value>) -> Option<Vec<String>> {
    match value {
        Some(Value::Array(items)) => Some(items.iter().map(py_str).collect()),
        _ => None,
    }
}

fn string_field(value: Option<&Value>) -> Option<String> {
    match value {
        None => None,
        Some(Value::Null) => Some(String::new()),
        Some(v) => Some(py_str(v)),
    }
}

impl Profile {
    // -- serialization ---------------------------------------------------------

    /// `dataclasses.asdict(self)` plus the version and signature fields.
    pub fn to_dict(&self) -> Map<String, Value> {
        let mut d = self.to_unsigned_dict();
        d.insert("version".to_string(), Value::from(SAVE_VERSION));
        d.insert(
            SIGNATURE_VERSION_FIELD.to_string(),
            Value::from(SIGNATURE_VERSION),
        );
        let signature = signature_for(&d, None);
        d.insert(SIGNATURE_FIELD.to_string(), Value::String(signature));
        d
    }

    /// `dataclasses.asdict(self)`: the fields only, in declaration order.
    pub fn to_unsigned_dict(&self) -> Map<String, Value> {
        let mut d = Map::new();
        let strings = |items: &[String]| {
            Value::Array(items.iter().map(|s| Value::from(s.as_str())).collect())
        };
        d.insert("name".into(), Value::from(self.name.as_str()));
        d.insert("money".into(), Value::from(self.money));
        d.insert(
            "current_city".into(),
            Value::from(self.current_city.as_str()),
        );
        d.insert(
            "created_line".into(),
            Value::from(self.created_line.as_str()),
        );
        d.insert(
            "migration_notice_pending".into(),
            Value::from(self.migration_notice_pending),
        );
        // A balance rewritten in memory between transactions is caught here
        // too: the money guard's shadow disagrees with the field, and the
        // save goes to disk already carrying the modified mark.
        let memory_diverged = !self.money_guard.matches(self.money);
        d.insert(
            "integrity_modified".into(),
            Value::from(self.integrity_modified || memory_diverged),
        );
        d.insert(
            "integrity_notice_pending".into(),
            Value::from(
                self.integrity_notice_pending || (memory_diverged && !self.integrity_modified),
            ),
        );
        d.insert(
            "hos_key_notice_left".into(),
            Value::from(self.hos_key_notice_left),
        );
        d.insert("game_hours".into(), Value::from(self.game_hours));
        d.insert(
            "calendar_offset_days".into(),
            Value::from(self.calendar_offset_days),
        );
        d.insert("tutorial_done".into(), Value::from(self.tutorial_done));
        // The tractor the driver is actually in, not the raw field: a company
        // driver's assignment follows their level and the field only changes
        // when dispatch writes a slip seat, a spare or a status change into
        // it, so a promoted driver kept an old yard's truck in the save for
        // good. Reading it back yields the same key, so this is idempotent.
        d.insert("truck".into(), Value::from(self.active_truck_key()));
        d.insert("owned_trucks".into(), strings(&self.owned_trucks));
        let conditions: Map<String, Value> = self
            .truck_conditions
            .iter()
            .map(|(k, v)| (k.clone(), Value::Object(v.clone())))
            .collect();
        d.insert("truck_conditions".into(), Value::Object(conditions));
        let upgrades: Map<String, Value> = self
            .upgrades
            .iter()
            .map(|(k, v)| (k.clone(), Value::from(*v)))
            .collect();
        d.insert("upgrades".into(), Value::Object(upgrades));
        d.insert(
            "active_trip".into(),
            self.active_trip.clone().unwrap_or(Value::Null),
        );
        d.insert(
            "dispatch_board_cache".into(),
            self.dispatch_board_cache.clone().unwrap_or(Value::Null),
        );
        d.insert("fatigue".into(), Value::from(self.fatigue));
        d.insert(
            "active_buffs".into(),
            Value::Array(self.active_buffs.clone()),
        );
        d.insert("pay_advance".into(), Value::from(self.pay_advance));
        d.insert("fines_owed".into(), Value::from(self.fines_owed));
        d.insert(
            "pay_advance_used_for_load".into(),
            Value::from(self.pay_advance_used_for_load),
        );
        d.insert(
            "business_status".into(),
            Value::from(self.business_status.as_str()),
        );
        d.insert(
            "carrier_name".into(),
            Value::from(self.carrier_name.as_str()),
        );
        d.insert("carrier_key".into(), Value::from(self.carrier_key.as_str()));
        d.insert("start_mode".into(), Value::from(self.start_mode.as_str()));
        d.insert(
            "authority_readiness".into(),
            Value::from(self.authority_readiness),
        );
        d.insert(
            "weigh_station_transponder".into(),
            Value::from(self.weigh_station_transponder),
        );
        d.insert("trailer_programs".into(), strings(&self.trailer_programs));
        d.insert("owned_trailers".into(), strings(&self.owned_trailers));
        d.insert(
            "owner_operator_declined".into(),
            Value::from(self.owner_operator_declined),
        );
        d.insert("career".into(), {
            let mut career = serde_json::to_value(&self.career).expect("a career serialises");
            // The number the public profile shows, computed here rather
            // than kept current in memory: the record and the clock both
            // move it, and the save is the only reader.
            career["standing"] = Value::from(self.standing());
            career
        });
        d.insert(
            "driving_record".into(),
            serde_json::to_value(&self.driving_record).expect("a driving record serialises"),
        );
        d.insert("selection_score".into(), Value::from(self.selection_score));
        d.insert(
            "out_of_service_events".into(),
            Value::from(self.out_of_service_events),
        );
        d.insert(
            "market".into(),
            serde_json::to_value(&self.market).expect("a market serialises"),
        );
        d.insert("hos".into(), self.hos.to_dict());
        d.insert("duty_log".into(), self.duty_log.to_dict());
        d.insert("loyalty".into(), self.loyalty.to_dict());
        d.insert("achievements".into(), strings(&self.achievements));
        d.insert(
            "achievement_stats".into(),
            Value::Object(self.achievement_stats.clone()),
        );
        d.insert("radio_favorites".into(), strings(&self.radio_favorites));
        d.insert("recent_lanes".into(), strings(&self.recent_lanes));
        d
    }

    /// `Profile.from_dict(d)`: tolerant of missing, extra and malformed
    /// fields, running the save migrations first.
    pub fn from_dict(d: &Map<String, Value>) -> Profile {
        let (mut d, mut migrated) = migrate_save_data(d.clone());
        // A 1.9 career from before the created-on marker existed: the load
        // gate already vouched for it by save version, so stamp the explicit
        // marker on the resave and the version threshold is needed only once.
        if !d.contains_key("created_line") {
            migrated = true;
        }
        d.remove("version");
        d.remove(SIGNATURE_FIELD);
        d.remove(SIGNATURE_VERSION_FIELD);
        // Pre-11 saves stored one flat condition set; fan it out per truck so
        // each owned tractor keeps its own wear, damage, and fuel from here on.
        if !matches!(d.get("truck_conditions"), Some(Value::Object(_))) {
            let fanned = condition::migrate_flat_conditions(&d);
            let map: Map<String, Value> = fanned
                .into_iter()
                .map(|(k, v)| (k, Value::Object(v)))
                .collect();
            d.insert("truck_conditions".to_string(), Value::Object(map));
        }
        // Grime moved onto the truck after those records already existed, so an
        // alpha save can be fanned out yet still carry the flat field. Matched
        // on shape rather than save version for exactly that reason, and run
        // after the fan-out so both paths land on the same records.
        if condition::migrate_profile_wide_grime(&mut d) {
            migrated = true;
        }
        let career: Career = match d.get("career") {
            Some(Value::Object(_)) => {
                serde_json::from_value(d["career"].clone()).unwrap_or_default()
            }
            _ => Career::new(),
        };
        // A career from before the enforcement record existed is seeded from
        // whatever offenses the save still holds -- no amnesty -- and hears a
        // one-time explanation of where it stands.
        let mut driving_record: DrivingRecord = match d.get("driving_record") {
            Some(Value::Object(_)) => {
                serde_json::from_value(d["driving_record"].clone()).unwrap_or_default()
            }
            Some(_) => DrivingRecord::new(),
            None => seed_record_from_save(&d),
        };
        // A save from before the carrier's record review existed: the review
        // starts counting from this load, so an old serious violation cannot
        // hold the equipment or end the employment on the first boot of the
        // build that introduced it. A record that carries the field keeps it.
        let review_known = d
            .get("driving_record")
            .and_then(Value::as_object)
            .is_some_and(|record| record.contains_key("review_started_h"));
        if !review_known {
            driving_record.review_started_h = json_f64(d.get("game_hours"), 0.0);
        }
        let market: Market = match d.get("market") {
            Some(Value::Object(_)) => {
                serde_json::from_value(d["market"].clone()).unwrap_or_else(|_| Market::new())
            }
            _ => Market::new(),
        };
        // absent in v2 saves: fresh clock
        let hos = HosClock::from_dict(d.get("hos").unwrap_or(&Value::Null));
        let duty_log = DutyLog::from_dict(d.get("duty_log").unwrap_or(&Value::Null));
        let loyalty = LoyaltyAccount::from_dict(d.get("loyalty").unwrap_or(&Value::Null));

        let defaults = Profile::default();
        let f = |key: &str, default: f64| json_f64(d.get(key), default);
        let i = |key: &str, default: i64| json_i64(d.get(key), default);
        let b = |key: &str, default: bool| d.get(key).map(py_truthy).unwrap_or(default);
        let s = |key: &str, default: &str| {
            string_field(d.get(key)).unwrap_or_else(|| default.to_string())
        };
        let list = |key: &str| string_list(d.get(key)).unwrap_or_default();
        let opt = |key: &str| match d.get(key) {
            None | Some(Value::Null) => None,
            Some(v) => Some(v.clone()),
        };

        let mut truck_conditions: IndexMap<String, ConditionRecord> = IndexMap::new();
        if let Some(Value::Object(records)) = d.get("truck_conditions") {
            for (key, record) in records {
                if let Value::Object(record) = record {
                    truck_conditions.insert(key.clone(), record.clone());
                }
            }
        }
        let active_buffs = match d.get("active_buffs") {
            Some(Value::Array(items)) => items.clone(),
            _ => Vec::new(),
        };
        let achievement_stats = match d.get("achievement_stats") {
            Some(Value::Object(map)) => map.clone(),
            _ => Map::new(),
        };

        // Read once so the money guard is seeded with the balance the load
        // gate already vouched for.
        let money = f("money", defaults.money);

        Profile {
            name: s("name", &defaults.name),
            money,
            money_guard: MoneyGuard::seeded(money),
            current_city: s("current_city", &defaults.current_city),
            created_line: s("created_line", &defaults.created_line),
            migration_notice_pending: b("migration_notice_pending", false),
            integrity_modified: b("integrity_modified", false),
            integrity_notice_pending: b("integrity_notice_pending", false),
            hos_key_notice_left: i("hos_key_notice_left", defaults.hos_key_notice_left),
            game_hours: f("game_hours", defaults.game_hours),
            calendar_offset_days: i("calendar_offset_days", 0),
            calendar_offset_hours: 0.0,
            live_calendar: false,
            tutorial_done: b("tutorial_done", false),
            truck: s("truck", &defaults.truck),
            owned_trucks: list("owned_trucks"),
            truck_conditions,
            upgrades: condition::upgrades_from_value(d.get("upgrades")),
            active_trip: opt("active_trip"),
            dispatch_board_cache: opt("dispatch_board_cache"),
            fatigue: f("fatigue", 0.0),
            active_buffs,
            pay_advance: f("pay_advance", 0.0),
            fines_owed: f("fines_owed", 0.0),
            pay_advance_used_for_load: b("pay_advance_used_for_load", false),
            business_status: s("business_status", &defaults.business_status),
            carrier_name: s("carrier_name", &defaults.carrier_name),
            carrier_key: s("carrier_key", &defaults.carrier_key),
            start_mode: s("start_mode", &defaults.start_mode),
            authority_readiness: b("authority_readiness", false),
            weigh_station_transponder: b("weigh_station_transponder", false),
            trailer_programs: list("trailer_programs"),
            owned_trailers: list("owned_trailers"),
            owner_operator_declined: b("owner_operator_declined", false),
            career,
            driving_record,
            selection_score: f("selection_score", SAFETY_RECORD_BASELINE),
            out_of_service_events: i("out_of_service_events", 0),
            market,
            hos,
            duty_log,
            loyalty,
            achievements: list("achievements"),
            achievement_stats,
            radio_favorites: list("radio_favorites"),
            recent_lanes: list("recent_lanes"),
            // A save that had to be migrated on load is rewritten on the next
            // save, so the conversion is not redone on every launch.
            needs_migration_resave: migrated,
        }
    }
}
