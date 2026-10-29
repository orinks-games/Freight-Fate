//! A stand-in for `Profile(name=..., current_city=...)` and `Job(...)` for the
//! career-side tests, carrying the fields those modules read and write with
//! the Python defaults. The `Profile` methods the modules call back into
//! (`active_truck_key`, `take_slip_seat`, `truck_specs`, the business
//! eligibility gates, ...) are mirrored here from `profile.py` /
//! `business.py` until `models::profile` and `models::business` land.

use std::collections::HashMap;

use crate::models::business_constants::{is_owner_operator, COMPANY_DRIVER, INDEPENDENT_AUTHORITY};
use crate::models::career::{Career, CareerProfile, JobView};
use crate::models::career_ladder::STARTER_CARRIER_NAME;
use crate::models::carrier_fleet::{assigned_truck_key, slip_seat_pool, slip_seats};
use crate::models::enforcement::{DrivingRecord, StandingProfile};
use crate::models::start_options::{StartProfile, DEFAULT_START_KEY};
use crate::models::trailer_yard::TrailerOwner;
use crate::models::trailers::{normalized_trailer_programs, DEFAULT_TRAILER_PROGRAMS};
use crate::models::trucks::{build_truck_specs, truck_model, TruckCondition, NO_UPGRADES};
use crate::sim::vehicle::TruckSpecs;

// The business gates (`business.py`), for the fake's eligibility answers.
use crate::models::business::{
    AUTHORITY_ACTIVATION_COST, AUTHORITY_ACTIVATION_DELIVERIES, AUTHORITY_ACTIVATION_LEVEL,
    AUTHORITY_ACTIVATION_REPUTATION, AUTHORITY_ACTIVATION_WORKING_CAPITAL,
    AUTHORITY_READY_DELIVERIES, AUTHORITY_READY_LEVEL, AUTHORITY_READY_REPUTATION,
    AUTHORITY_READY_RESERVE, AUTHORITY_READY_WORKING_CAPITAL, OWNER_OPERATOR_BUY_IN,
    OWNER_OPERATOR_DELIVERIES, OWNER_OPERATOR_LEVEL, OWNER_OPERATOR_REPUTATION,
    OWNER_OPERATOR_WORKING_CAPITAL,
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FakeProfile {
    pub name: String,
    pub career: Career,
    pub business_status: String,
    pub start_mode: String,
    pub carrier_key: String,
    pub carrier_name: String,
    pub money: f64,
    pub fines_owed: f64,
    pub game_hours: f64,
    pub calendar_offset_days: f64,
    pub driving_record: Option<DrivingRecord>,
    pub truck: String,
    pub owned_trucks: Vec<String>,
    pub owned_trailers: Vec<String>,
    pub trailer_programs: Vec<String>,
    pub upgrades: HashMap<String, i64>,
    pub truck_conditions: HashMap<String, TruckCondition>,
    pub pay_advance: f64,
    pub pay_advance_used_for_load: bool,
    pub authority_readiness: bool,
    pub owner_operator_declined: bool,
    pub has_active_trip: bool,
    pub dispatch_board_cached: bool,
    pub achievements: Vec<String>,
}

impl Default for FakeProfile {
    fn default() -> Self {
        FakeProfile {
            name: String::new(),
            career: Career::new(),
            business_status: COMPANY_DRIVER.to_string(),
            start_mode: "company_driver".to_string(),
            carrier_key: DEFAULT_START_KEY.to_string(),
            carrier_name: STARTER_CARRIER_NAME.to_string(),
            money: 5000.0,
            fines_owed: 0.0,
            game_hours: 6.0,
            calendar_offset_days: 0.0,
            driving_record: Some(DrivingRecord::new()),
            truck: "rig".to_string(),
            owned_trucks: Vec::new(),
            owned_trailers: Vec::new(),
            trailer_programs: Vec::new(),
            upgrades: HashMap::new(),
            truck_conditions: HashMap::new(),
            pay_advance: 0.0,
            pay_advance_used_for_load: false,
            authority_readiness: false,
            owner_operator_declined: false,
            has_active_trip: false,
            dispatch_board_cached: false,
            achievements: Vec::new(),
        }
    }
}

impl FakeProfile {
    /// `Profile(name=name)`.
    pub fn named(name: &str) -> Self {
        FakeProfile {
            name: name.to_string(),
            ..Self::default()
        }
    }

    /// `Profile.take_slip_seat(job)`.
    pub fn take_slip_seat<J: JobView>(&mut self, job: &J) -> String {
        if self.owns_equipment() || !slip_seats(self) {
            return CareerProfile::active_truck_key(self);
        }
        let key = assigned_truck_key(self, Some(job));
        self.truck = key.to_string();
        key.to_string()
    }

    /// `Profile.truck_specs()`.
    pub fn truck_specs(&self) -> TruckSpecs {
        if self.owns_equipment() {
            build_truck_specs(&CareerProfile::active_truck_key(self), &self.upgrades)
        } else {
            build_truck_specs(&CareerProfile::active_truck_key(self), &NO_UPGRADES)
        }
    }

    /// `Profile.visible_owned_trucks()`.
    pub fn visible_owned_trucks(&self) -> Vec<String> {
        if self.owns_equipment() {
            self.owned_trucks.clone()
        } else {
            Vec::new()
        }
    }

    /// `Profile.active_trailer_programs()`.
    pub fn active_trailer_programs(&self) -> Vec<String> {
        if !self.owns_equipment() {
            return Vec::new();
        }
        let programs: Vec<String> = normalized_trailer_programs(&self.trailer_programs)
            .into_iter()
            .map(str::to_string)
            .collect();
        if self.business_status == INDEPENDENT_AUTHORITY {
            let mut combined = programs.clone();
            for key in normalized_trailer_programs(&self.owned_trailers) {
                if !combined.iter().any(|k| k == key) {
                    combined.push(key.to_string());
                }
            }
            if !combined.is_empty() {
                return combined;
            }
        }
        if programs.is_empty() {
            DEFAULT_TRAILER_PROGRAMS
                .iter()
                .map(|k| k.to_string())
                .collect()
        } else {
            programs
        }
    }

    fn condition_mut(&mut self) -> &mut TruckCondition {
        let key = CareerProfile::active_truck_key(self);
        self.truck_conditions.entry(key).or_default()
    }

    pub fn truck_fuel_gal(&self) -> f64 {
        self.truck_conditions
            .get(&CareerProfile::active_truck_key(self))
            .map(|c| c.fuel_gal)
            .unwrap_or(150.0)
    }

    pub fn truck_damage_pct(&self) -> f64 {
        self.truck_conditions
            .get(&CareerProfile::active_truck_key(self))
            .map(|c| c.damage_pct)
            .unwrap_or(0.0)
    }

    /// `business.owner_operator_eligibility(profile)`.
    pub fn owner_operator_eligibility(&self) -> bool {
        if is_owner_operator(&self.business_status) {
            return false;
        }
        self.career.level() >= OWNER_OPERATOR_LEVEL
            && self.career.deliveries >= OWNER_OPERATOR_DELIVERIES
            && self.career.reputation >= OWNER_OPERATOR_REPUTATION
            && self.money >= OWNER_OPERATOR_BUY_IN + OWNER_OPERATOR_WORKING_CAPITAL
            && self.pay_advance < 1.0
    }

    /// `business.authority_readiness_eligibility(profile)`.
    pub fn authority_readiness_eligibility(&self) -> bool {
        if self.authority_readiness || !is_owner_operator(&self.business_status) {
            return false;
        }
        self.career.level() >= AUTHORITY_READY_LEVEL
            && self.career.deliveries >= AUTHORITY_READY_DELIVERIES
            && self.career.reputation >= AUTHORITY_READY_REPUTATION
            && self.money >= AUTHORITY_READY_RESERVE + AUTHORITY_READY_WORKING_CAPITAL
            && self.pay_advance < 1.0
    }

    /// `business.authority_activation_eligibility(profile)`.
    pub fn authority_activation_eligibility(&self) -> bool {
        if self.business_status == INDEPENDENT_AUTHORITY
            || !is_owner_operator(&self.business_status)
        {
            return false;
        }
        let specialty = self
            .active_trailer_programs()
            .iter()
            .any(|p| p != "dry_van");
        self.authority_readiness
            && self.career.level() >= AUTHORITY_ACTIVATION_LEVEL
            && self.career.deliveries >= AUTHORITY_ACTIVATION_DELIVERIES
            && self.career.reputation >= AUTHORITY_ACTIVATION_REPUTATION
            && specialty
            && self.money >= AUTHORITY_ACTIVATION_COST + AUTHORITY_ACTIVATION_WORKING_CAPITAL
            && self.pay_advance < 1.0
    }
}

impl StandingProfile for FakeProfile {
    fn career_reputation(&self) -> f64 {
        self.career.reputation
    }
    fn career_deliveries(&self) -> i64 {
        self.career.deliveries
    }
    fn career_total_earnings(&self) -> f64 {
        self.career.total_earnings
    }
    fn game_hours(&self) -> f64 {
        self.game_hours
    }
    fn calendar_now_hours(&self) -> f64 {
        self.game_hours + self.calendar_offset_days * 24.0
    }
    fn driving_record(&self) -> Option<&DrivingRecord> {
        self.driving_record.as_ref()
    }
    fn business_status(&self) -> &str {
        &self.business_status
    }
    fn carrier_key(&self) -> &str {
        &self.carrier_key
    }
    fn carrier_name(&self) -> &str {
        &self.carrier_name
    }
    fn money(&self) -> f64 {
        self.money
    }
    fn fines_owed(&self) -> f64 {
        self.fines_owed
    }
    fn truck(&self) -> &str {
        &self.truck
    }
    fn truck_catalog_price(&self, key: &str) -> f64 {
        truck_model(key).map(|m| m.price).unwrap_or(0.0)
    }
}

impl CareerProfile for FakeProfile {
    fn career(&self) -> &Career {
        &self.career
    }
    fn name(&self) -> &str {
        &self.name
    }
    /// `Profile.active_truck_key()`.
    fn active_truck_key(&self) -> String {
        if self.owns_equipment() {
            return self.truck.clone();
        }
        // A slip-seating driver keeps the tractor dispatch handed them for
        // this run, as long as it is still one of this driver's spares.
        if slip_seats(self) && slip_seat_pool(self).contains(&self.truck.as_str()) {
            return self.truck.clone();
        }
        assigned_truck_key::<_, FakeJob>(self, None).to_string()
    }
    fn owner_operator_eligible(&self) -> bool {
        self.owner_operator_eligibility()
    }
    fn authority_readiness_eligible(&self) -> bool {
        self.authority_readiness_eligibility()
    }
    fn authority_activation_eligible(&self) -> bool {
        self.authority_activation_eligibility()
    }
    fn owner_operator_declined(&self) -> bool {
        self.owner_operator_declined
    }
}

impl TrailerOwner for FakeProfile {
    /// `Profile.owns_equipment()`.
    fn owns_equipment(&self) -> bool {
        is_owner_operator(&self.business_status)
    }
    /// `Profile.visible_owned_trailers()`.
    fn visible_owned_trailers(&self) -> Vec<String> {
        if self.business_status != INDEPENDENT_AUTHORITY {
            return Vec::new();
        }
        normalized_trailer_programs(&self.owned_trailers)
            .into_iter()
            .map(str::to_string)
            .collect()
    }
}

impl StartProfile for FakeProfile {
    fn set_carrier_key(&mut self, key: &str) {
        self.carrier_key = key.to_string();
    }
    fn set_start_mode(&mut self, mode: &str) {
        self.start_mode = mode.to_string();
    }
    fn set_carrier_name(&mut self, name: &str) {
        self.carrier_name = name.to_string();
    }
    fn set_money(&mut self, money: f64) {
        self.money = money;
    }
    fn set_business_status(&mut self, status: &str) {
        self.business_status = status.to_string();
    }
    fn set_truck(&mut self, key: &str) {
        self.truck = key.to_string();
    }
    fn set_owned_trucks(&mut self, trucks: Vec<String>) {
        self.owned_trucks = trucks;
    }
    fn set_owned_trailers(&mut self, trailers: Vec<String>) {
        self.owned_trailers = trailers;
    }
    fn set_trailer_programs(&mut self, programs: Vec<String>) {
        self.trailer_programs = programs;
    }
    fn clear_upgrades(&mut self) {
        self.upgrades.clear();
    }
    fn clear_truck_conditions(&mut self) {
        self.truck_conditions.clear();
    }
    fn active_truck_key(&self) -> String {
        CareerProfile::active_truck_key(self)
    }
    fn owned_trucks(&self) -> Vec<String> {
        self.owned_trucks.clone()
    }
    fn provision_truck_condition(&mut self, key: &str) {
        // `_fresh_condition(_truck_tank_gal(key))`: a full tank for the model.
        self.truck_conditions
            .insert(key.to_string(), TruckCondition::fresh(key, &NO_UPGRADES));
    }
    fn truck_fuel_tank_gal(&self) -> f64 {
        self.truck_specs().fuel_tank_gal
    }
    fn set_truck_fuel_gal(&mut self, gallons: f64) {
        self.condition_mut().fuel_gal = gallons;
    }
    fn set_truck_damage_pct(&mut self, pct: f64) {
        self.condition_mut().damage_pct = pct;
    }
    fn clear_active_trip(&mut self) {
        self.has_active_trip = false;
    }
    fn clear_dispatch_board_cache(&mut self) {
        self.dispatch_board_cached = false;
    }
    fn set_pay_advance(&mut self, amount: f64) {
        self.pay_advance = amount;
    }
    fn set_pay_advance_used_for_load(&mut self, used: bool) {
        self.pay_advance_used_for_load = used;
    }
    fn career_mut(&mut self) -> &mut Career {
        &mut self.career
    }
}

/// `Job(cargo, weight_tons, origin, origin_location, destination,
/// distance_mi, pay, deadline_game_h)` plus the facility fields the yard
/// reads, defaulting like the tests' `SimpleNamespace` jobs.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FakeJob {
    pub cargo_key: String,
    pub weight_tons: f64,
    pub distance_mi: f64,
    pub deadline_game_h: f64,
    pub origin_type: String,
    pub origin_facility_id: String,
    pub origin_location: String,
    pub destination_type: String,
    pub destination_facility_id: String,
    pub destination_location: String,
}

impl Default for FakeJob {
    fn default() -> Self {
        FakeJob {
            cargo_key: "general".to_string(),
            weight_tons: 12.0,
            distance_mi: 0.0,
            deadline_game_h: 8.0,
            origin_type: String::new(),
            origin_facility_id: String::new(),
            origin_location: String::new(),
            destination_type: String::new(),
            destination_facility_id: String::new(),
            destination_location: String::new(),
        }
    }
}

impl FakeJob {
    /// `SimpleNamespace(distance_mi=..., weight_tons=...)`.
    pub fn sized(distance_mi: f64, weight_tons: f64) -> Self {
        FakeJob {
            distance_mi,
            weight_tons,
            ..Self::default()
        }
    }

    /// The training tests' `_job(miles, deadline_h, cargo)`.
    pub fn lane(miles: f64, deadline_h: f64, cargo: &str) -> Self {
        FakeJob {
            cargo_key: cargo.to_string(),
            distance_mi: miles,
            deadline_game_h: deadline_h,
            ..Self::default()
        }
    }
}

impl JobView for FakeJob {
    fn distance_mi(&self) -> f64 {
        self.distance_mi
    }
    fn weight_tons(&self) -> f64 {
        self.weight_tons
    }
    fn deadline_game_h(&self) -> f64 {
        self.deadline_game_h
    }
    fn cargo_key(&self) -> &str {
        &self.cargo_key
    }
    fn origin_type(&self) -> &str {
        &self.origin_type
    }
    fn origin_facility_id(&self) -> &str {
        &self.origin_facility_id
    }
    fn origin_location(&self) -> &str {
        &self.origin_location
    }
    fn destination_type(&self) -> &str {
        &self.destination_type
    }
    fn destination_facility_id(&self) -> &str {
        &self.destination_facility_id
    }
    fn destination_location(&self) -> &str {
        &self.destination_location
    }
}
