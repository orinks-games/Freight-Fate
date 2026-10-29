//! A stand-in for `Profile(name=..., current_city="Buffalo")` for the
//! enforcement and solvency tests, carrying exactly the fields those modules
//! read and write, with the Python defaults. The tiny truck catalogue mirrors
//! the four `TRUCK_CATALOG` entries the Python tests name (`trucks.py`
//! prices) until `models::trucks` lands.

use crate::models::business_constants::{is_owner_operator, COMPANY_DRIVER};
use crate::models::career_ladder::STARTER_CARRIER_NAME;
use crate::models::enforcement::{DrivingRecord, StandingProfile};
use crate::models::solvency::SolvencyProfile;

/// `(key, price, label)` for the tractors the tests drive.
pub(crate) const FAKE_TRUCK_CATALOG: &[(&str, f64, &str)] = &[
    ("rig", 0.0, "standard rig"),
    ("hand_me_down_sleeper", 31_000.0, "hand-me-down sleeper"),
    ("highline_sleeper", 82_000.0, "highline sleeper"),
    ("presidential_sleeper", 185_000.0, "presidential sleeper"),
];

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct FakeProfile {
    pub reputation: f64,
    pub deliveries: i64,
    pub total_earnings: f64,
    pub game_hours: f64,
    pub calendar_offset_days: f64,
    /// `None` is the tests' `SimpleNamespace()` with no record at all.
    pub driving_record: Option<DrivingRecord>,
    pub business_status: String,
    pub carrier_key: String,
    pub carrier_name: String,
    pub money: f64,
    pub fines_owed: f64,
    pub truck: String,
    pub owned_trucks: Vec<String>,
    pub owned_trailers: Vec<String>,
    pub pay_advance: f64,
    pub pay_advance_used_for_load: bool,
    pub authority_readiness: bool,
    pub weigh_station_transponder: bool,
    pub dispatch_board_cached: bool,
    /// What `carrier_fleet.equipment_hold_clause` would say.
    pub hold_clause: String,
}

impl Default for FakeProfile {
    fn default() -> Self {
        Self {
            reputation: 50.0,
            deliveries: 0,
            total_earnings: 0.0,
            game_hours: 6.0,
            calendar_offset_days: 0.0,
            driving_record: Some(DrivingRecord::new()),
            business_status: COMPANY_DRIVER.to_string(),
            carrier_key: "northstar".to_string(),
            carrier_name: STARTER_CARRIER_NAME.to_string(),
            money: 5000.0,
            fines_owed: 0.0,
            truck: "rig".to_string(),
            owned_trucks: Vec::new(),
            owned_trailers: Vec::new(),
            pay_advance: 0.0,
            pay_advance_used_for_load: false,
            authority_readiness: false,
            weigh_station_transponder: false,
            dispatch_board_cached: true,
            hold_clause: String::new(),
        }
    }
}

impl FakeProfile {
    pub fn new() -> Self {
        Self::default()
    }

    /// The record every real profile has.
    pub fn record(&self) -> &DrivingRecord {
        self.driving_record.as_ref().expect("a driving record")
    }

    pub fn record_mut(&mut self) -> &mut DrivingRecord {
        self.driving_record.as_mut().expect("a driving record")
    }
}

impl StandingProfile for FakeProfile {
    fn career_reputation(&self) -> f64 {
        self.reputation
    }
    fn career_deliveries(&self) -> i64 {
        self.deliveries
    }
    fn career_total_earnings(&self) -> f64 {
        self.total_earnings
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
        FAKE_TRUCK_CATALOG
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, price, _)| *price)
            .unwrap_or(0.0)
    }
    fn equipment_hold_clause(&self) -> String {
        self.hold_clause.clone()
    }
}

impl SolvencyProfile for FakeProfile {
    fn set_money(&mut self, money: f64) {
        self.money = money;
    }
    fn set_fines_owed(&mut self, fines_owed: f64) {
        self.fines_owed = fines_owed;
    }
    fn driving_record_mut(&mut self) -> &mut DrivingRecord {
        self.record_mut()
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
        self.dispatch_board_cached = false;
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
        // Profile.active_truck_key: an owner-operator drives their own truck;
        // a company driver whatever the carrier assigned.
        if is_owner_operator(&self.business_status) {
            self.truck.clone()
        } else {
            self.assigned_truck_key()
        }
    }
    fn truck_catalog_label(&self, key: &str) -> String {
        FAKE_TRUCK_CATALOG
            .iter()
            .find(|(k, _, _)| *k == key)
            .map(|(_, _, label)| (*label).to_string())
            .unwrap_or_else(|| panic!("{key:?} is not in the truck catalog"))
    }
    fn assigned_truck_key(&self) -> String {
        "rig".to_string()
    }
}
