//! The read/write views the other model modules defined for a live profile,
//! implemented for the real `Profile`. Each method is the attribute read or
//! assignment the Python made.

use serde_json::{Map, Value};

use super::Profile;
use crate::achievements::AchievementProfile;
use crate::models::business::{
    authority_activation_eligibility, authority_readiness_eligibility, owner_operator_eligibility,
    BusinessProfile,
};
use crate::models::career::{Career, CareerProfile};
use crate::models::carrier_fleet::{assigned_truck_key, equipment_hold_clause};
use crate::models::enforcement::{DrivingRecord, StandingProfile};
use crate::models::jobs::Job;
use crate::models::safety_record::{SafetyRecordProfile, SAFETY_RECORD_WINDOW_DAYS};
use crate::models::save_migration::json_i64;
use crate::models::solvency::SolvencyProfile;
use crate::models::start_options::StartProfile;
use crate::models::trailer_yard::TrailerOwner;
use crate::models::trucks::truck_model;
use crate::music::MenuMusicProfile;

impl StandingProfile for Profile {
    /// Dispatch trust, the termination floor and the business gates all read
    /// the record-adjusted number, never the raw ledger.
    fn career_reputation(&self) -> f64 {
        self.standing()
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
        self.player_calendar_hours()
    }
    fn calendar_is_live(&self) -> bool {
        self.live_calendar
    }
    fn driving_record(&self) -> Option<&DrivingRecord> {
        Some(&self.driving_record)
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
    /// `TRUCK_CATALOG.get(key).price`.
    fn truck_catalog_price(&self, key: &str) -> f64 {
        truck_model(key).map(|m| m.price).unwrap_or(0.0)
    }
    /// `carrier_fleet.equipment_hold_clause(profile)`.
    fn equipment_hold_clause(&self) -> String {
        equipment_hold_clause(self)
    }
}

impl SolvencyProfile for Profile {
    fn set_money(&mut self, money: f64) {
        self.money = money;
        self.money_guard.resync(money);
    }
    fn set_fines_owed(&mut self, fines_owed: f64) {
        self.fines_owed = fines_owed;
    }
    fn driving_record_mut(&mut self) -> &mut DrivingRecord {
        &mut self.driving_record
    }
    fn set_carrier(&mut self, key: &str, name: &str) {
        self.carrier_key = key.to_string();
        self.carrier_name = name.to_string();
    }
    fn set_pay_advance(&mut self, amount: f64) {
        self.pay_advance = amount;
    }
    fn set_pay_advance_used_for_load(&mut self, used: bool) {
        self.pay_advance_used_for_load = used;
    }
    fn clear_dispatch_board_cache(&mut self) {
        self.dispatch_board_cache = None;
    }
    fn set_owned_trucks(&mut self, trucks: Vec<String>) {
        self.owned_trucks = trucks;
    }
    fn set_owned_trailers(&mut self, trailers: Vec<String>) {
        self.owned_trailers = trailers;
    }
    fn set_business_status(&mut self, status: &str) {
        self.business_status = status.to_string();
    }
    fn set_authority_readiness(&mut self, ready: bool) {
        self.authority_readiness = ready;
    }
    fn set_weigh_station_transponder(&mut self, on: bool) {
        self.weigh_station_transponder = on;
    }
    fn set_truck(&mut self, key: &str) {
        self.truck = key.to_string();
    }
    fn owned_trucks(&self) -> Vec<String> {
        self.owned_trucks.clone()
    }
    fn owned_trailers(&self) -> Vec<String> {
        self.owned_trailers.clone()
    }
    fn active_truck_key(&self) -> String {
        Profile::active_truck_key(self)
    }
    /// `TRUCK_CATALOG[key].label`.
    fn truck_catalog_label(&self, key: &str) -> String {
        truck_model(key)
            .map(|m| m.label.to_string())
            .unwrap_or_else(|| key.to_string())
    }
    /// `carrier_fleet.assigned_truck_key(profile)`.
    fn assigned_truck_key(&self) -> String {
        assigned_truck_key::<_, Job>(self, None).to_string()
    }
}

impl SafetyRecordProfile for Profile {
    /// The raw ledger on purpose: the selection score already weighs the
    /// citations and violations themselves, so feeding it the record-adjusted
    /// number would count every one of them twice.
    fn career_reputation(&self) -> f64 {
        self.career.reputation
    }
    // Counted inside the screening window, so an old record fades off the
    // scale the way it fades off reputation. Mid-drive `game_hours` is the
    // trip's start, which only ever keeps an event a trip longer.
    fn record_citations(&self) -> i64 {
        self.driving_record
            .citations_within(self.game_hours, SAFETY_RECORD_WINDOW_DAYS)
    }
    fn record_serious_violation_count(&self) -> i64 {
        self.driving_record
            .serious_within(self.game_hours, SAFETY_RECORD_WINDOW_DAYS)
    }
    fn out_of_service_events(&self) -> i64 {
        self.driving_record
            .out_of_service_within(self.game_hours, SAFETY_RECORD_WINDOW_DAYS)
    }
    fn record_fatigue_events(&self) -> i64 {
        self.driving_record
            .fatigue_within(self.game_hours, SAFETY_RECORD_WINDOW_DAYS)
    }
    fn record_crashes(&self) -> i64 {
        self.driving_record
            .crashes_within(self.game_hours, SAFETY_RECORD_WINDOW_DAYS)
    }
    fn inspections_passed(&self) -> i64 {
        json_i64(self.achievement_stats.get("inspections_passed"), 0)
    }
    fn set_selection_score(&mut self, score: f64) {
        self.selection_score = score;
    }
}

impl CareerProfile for Profile {
    fn career(&self) -> &Career {
        &self.career
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn active_truck_key(&self) -> String {
        Profile::active_truck_key(self)
    }
    fn owner_operator_eligible(&self) -> bool {
        owner_operator_eligibility(self).0
    }
    fn authority_readiness_eligible(&self) -> bool {
        authority_readiness_eligibility(self).0
    }
    fn authority_activation_eligible(&self) -> bool {
        authority_activation_eligibility(self).0
    }
    fn owner_operator_declined(&self) -> bool {
        self.owner_operator_declined
    }
}

impl BusinessProfile for Profile {
    fn authority_readiness(&self) -> bool {
        self.authority_readiness
    }
    fn weigh_station_transponder(&self) -> bool {
        self.weigh_station_transponder
    }
    fn pay_advance(&self) -> f64 {
        self.pay_advance
    }
    fn start_mode(&self) -> &str {
        &self.start_mode
    }
    fn active_trailer_programs(&self) -> Vec<String> {
        Profile::active_trailer_programs(self)
    }
    fn cdl_clear(&self) -> bool {
        !self.driving_record.suspended(self.game_hours)
    }
}

impl TrailerOwner for Profile {
    fn owns_equipment(&self) -> bool {
        Profile::owns_equipment(self)
    }
    fn visible_owned_trailers(&self) -> Vec<String> {
        Profile::visible_owned_trailers(self)
    }
}

impl StartProfile for Profile {
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
        self.money_guard.resync(money);
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
        Profile::active_truck_key(self)
    }
    fn owned_trucks(&self) -> Vec<String> {
        self.owned_trucks.clone()
    }
    fn provision_truck_condition(&mut self, key: &str) {
        Profile::provision_truck_condition(self, key, None);
    }
    fn truck_fuel_tank_gal(&self) -> f64 {
        self.truck_specs().fuel_tank_gal
    }
    fn set_truck_fuel_gal(&mut self, gallons: f64) {
        Profile::set_truck_fuel_gal(self, gallons);
    }
    fn set_truck_damage_pct(&mut self, pct: f64) {
        Profile::set_truck_damage_pct(self, pct);
    }
    fn clear_active_trip(&mut self) {
        self.active_trip = None;
    }
    fn clear_dispatch_board_cache(&mut self) {
        self.dispatch_board_cache = None;
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

impl AchievementProfile for Profile {
    fn achievements(&self) -> &[String] {
        &self.achievements
    }
    fn achievements_mut(&mut self) -> &mut Vec<String> {
        &mut self.achievements
    }
    fn achievement_stats_mut(&mut self) -> &mut Map<String, Value> {
        &mut self.achievement_stats
    }
}

impl MenuMusicProfile for Profile {
    fn game_hours(&self) -> f64 {
        self.game_hours
    }
    fn level(&self) -> i64 {
        self.career.level()
    }
    fn deliveries(&self) -> i64 {
        self.career.deliveries
    }
    fn total_miles(&self) -> f64 {
        self.career.total_miles
    }
    fn owned_truck_count(&self) -> usize {
        self.visible_owned_trucks().len()
    }
    fn active_truck_key(&self) -> String {
        Profile::active_truck_key(self)
    }
    fn name(&self) -> String {
        self.name.clone()
    }
    fn current_city(&self) -> String {
        self.current_city.clone()
    }
    fn business_status(&self) -> String {
        self.business_status.clone()
    }
}

impl Profile {}
