//! Versioned, headless contracts shared by Freight Fate universe games.
//!
//! These records transfer identity, work and immutable outcomes. They never
//! transfer a game's save, wallet, active trip or simulation state.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, FixedOffset};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use thiserror::Error;

pub const DRIVER_EXCHANGE_VERSION: u32 = 1;
pub const DISPATCH_CALL_VERSION: u32 = 1;
pub const INTERCHANGE_FORMAT: &str = "freightverse.interchange";
pub const INTERCHANGE_VERSION: u32 = 1;

pub const LEG_STARTED: &str = "leg_started";
pub const LEG_COMPLETED: &str = "leg_completed";
pub const LEG_FAILED: &str = "leg_failed";

const BUSINESS_STATUSES: &[&str] = &[
    "company_driver",
    "leased_owner_operator",
    "independent_authority",
];

pub type JsonObject = Map<String, Value>;

/// Portable career identity. Game-owned money, equipment and active work are
/// intentionally absent.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DriverRecord {
    pub exchange_version: u32,
    pub driver_id: String,
    pub name: String,
    pub origin_game: String,
    pub exported_at: String,
    pub home_city: String,
    pub business_status: String,
    pub carrier_key: String,
    pub carrier_name: String,
    pub xp: f64,
    pub reputation: f64,
    pub deliveries: u64,
    pub on_time_deliveries: u64,
    pub on_time_streak: u64,
    pub total_miles: f64,
    pub total_earnings: f64,
    pub purchased_endorsements: Vec<String>,
}

impl Default for DriverRecord {
    fn default() -> Self {
        Self {
            exchange_version: DRIVER_EXCHANGE_VERSION,
            driver_id: String::new(),
            name: "Driver".to_string(),
            origin_game: "unknown".to_string(),
            exported_at: String::new(),
            home_city: String::new(),
            business_status: "company_driver".to_string(),
            carrier_key: String::new(),
            carrier_name: String::new(),
            xp: 0.0,
            reputation: 50.0,
            deliveries: 0,
            on_time_deliveries: 0,
            on_time_streak: 0,
            total_miles: 0.0,
            total_earnings: 0.0,
            purchased_endorsements: Vec::new(),
        }
    }
}

impl DriverRecord {
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.exchange_version == 0 {
            problems.push("missing or invalid exchange_version".to_string());
        }
        required_text(&self.driver_id, "driver_id", &mut problems);
        required_text(&self.name, "name", &mut problems);
        timestamp(&self.exported_at, "exported_at", &mut problems);
        if !BUSINESS_STATUSES.contains(&self.business_status.as_str()) {
            problems.push(format!(
                "unknown business_status {:?}",
                self.business_status
            ));
        }
        for (label, value) in [
            ("xp", self.xp),
            ("reputation", self.reputation),
            ("total_miles", self.total_miles),
            ("total_earnings", self.total_earnings),
        ] {
            if !value.is_finite() || value < 0.0 {
                problems.push(format!("invalid {label}"));
            }
        }
        problems
    }

    pub fn on_time_share(&self) -> f64 {
        if self.deliveries == 0 {
            1.0
        } else {
            self.on_time_deliveries as f64 / self.deliveries as f64
        }
    }
}

/// One mode-specific movement between stable shared facility IDs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct TransportLeg {
    pub leg_id: String,
    pub mode: String,
    pub sequence: u32,
    pub origin_id: String,
    pub destination_id: String,
    pub details: JsonObject,
}

/// A shipment plan published by the game that owns the work.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct ShipmentContract {
    pub contract_id: String,
    pub source_game: String,
    pub created_at: String,
    pub cargo_key: String,
    pub weight_tons: f64,
    pub legs: Vec<TransportLeg>,
    pub terms: JsonObject,
}

/// An immutable fact emitted by the game that performed a leg.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct UniverseEvent {
    pub event_id: String,
    pub contract_id: String,
    pub leg_id: String,
    pub kind: String,
    pub occurred_at: String,
    pub source_game: String,
    pub data: JsonObject,
}

/// A driver's structured request for dispatch help.
///
/// The request carries only the facts needed to make a dispatch decision.
/// It never transfers the game's active trip or save state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DispatchCallRequest {
    pub dispatch_call_version: u32,
    pub request_id: String,
    pub contract_id: String,
    pub leg_id: String,
    pub source_game: String,
    pub driver_id: String,
    pub kind: String,
    pub created_at: String,
    pub summary: String,
    pub context: JsonObject,
}

impl Default for DispatchCallRequest {
    fn default() -> Self {
        Self {
            dispatch_call_version: DISPATCH_CALL_VERSION,
            request_id: String::new(),
            contract_id: String::new(),
            leg_id: String::new(),
            source_game: String::new(),
            driver_id: String::new(),
            kind: String::new(),
            created_at: String::new(),
            summary: String::new(),
            context: JsonObject::new(),
        }
    }
}

impl DispatchCallRequest {
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.dispatch_call_version == 0 {
            problems.push("missing or invalid dispatch_call_version".to_string());
        }
        required_text(&self.request_id, "request_id", &mut problems);
        required_text(&self.source_game, "source_game", &mut problems);
        required_text(&self.driver_id, "driver_id", &mut problems);
        required_text(&self.kind, "kind", &mut problems);
        required_text(&self.summary, "summary", &mut problems);
        timestamp(&self.created_at, "created_at", &mut problems);
        problems
    }
}

/// A dispatcher's immutable answer to one call.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DispatchCallResponse {
    pub dispatch_call_version: u32,
    pub response_id: String,
    pub request_id: String,
    pub source_game: String,
    pub responder_id: String,
    pub decision: String,
    pub responded_at: String,
    pub message: String,
    pub effects: JsonObject,
}

impl Default for DispatchCallResponse {
    fn default() -> Self {
        Self {
            dispatch_call_version: DISPATCH_CALL_VERSION,
            response_id: String::new(),
            request_id: String::new(),
            source_game: String::new(),
            responder_id: String::new(),
            decision: String::new(),
            responded_at: String::new(),
            message: String::new(),
            effects: JsonObject::new(),
        }
    }
}

impl DispatchCallResponse {
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.dispatch_call_version == 0 {
            problems.push("missing or invalid dispatch_call_version".to_string());
        }
        required_text(&self.response_id, "response_id", &mut problems);
        required_text(&self.request_id, "request_id", &mut problems);
        required_text(&self.source_game, "source_game", &mut problems);
        required_text(&self.responder_id, "responder_id", &mut problems);
        required_text(&self.decision, "decision", &mut problems);
        required_text(&self.message, "message", &mut problems);
        timestamp(&self.responded_at, "responded_at", &mut problems);
        problems
    }
}

/// One producer's contracts and events in a versioned JSON envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExchangeManifest {
    pub format: String,
    pub interchange_version: u32,
    pub producer: String,
    pub exported_at: String,
    pub contracts: Vec<ShipmentContract>,
    pub events: Vec<UniverseEvent>,
}

impl Default for ExchangeManifest {
    fn default() -> Self {
        Self {
            format: INTERCHANGE_FORMAT.to_string(),
            interchange_version: INTERCHANGE_VERSION,
            producer: String::new(),
            exported_at: String::new(),
            contracts: Vec::new(),
            events: Vec::new(),
        }
    }
}

impl ExchangeManifest {
    pub fn validate(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.format != INTERCHANGE_FORMAT {
            problems.push(format!("format must be {INTERCHANGE_FORMAT:?}"));
        }
        if self.interchange_version == 0 {
            problems.push("missing or invalid interchange_version".to_string());
        }
        required_text(&self.producer, "manifest producer", &mut problems);
        timestamp(&self.exported_at, "manifest exported_at", &mut problems);

        let mut contract_ids = BTreeSet::new();
        for (index, contract) in self.contracts.iter().enumerate() {
            let label = format!("contracts[{index}]");
            required_text(
                &contract.contract_id,
                &format!("{label} contract_id"),
                &mut problems,
            );
            if !contract.contract_id.is_empty()
                && !contract_ids.insert(contract.contract_id.as_str())
            {
                problems.push(format!("duplicate contract_id {:?}", contract.contract_id));
            }
            required_text(
                &contract.source_game,
                &format!("{label} source_game"),
                &mut problems,
            );
            if !self.producer.is_empty()
                && !contract.source_game.is_empty()
                && contract.source_game != self.producer
            {
                problems.push(format!("{label} source_game must match manifest producer"));
            }
            timestamp(
                &contract.created_at,
                &format!("{label} created_at"),
                &mut problems,
            );
            required_text(
                &contract.cargo_key,
                &format!("{label} cargo_key"),
                &mut problems,
            );
            if !contract.weight_tons.is_finite() || contract.weight_tons <= 0.0 {
                problems.push(format!("{label} weight_tons must be a positive number"));
            }
            if contract.legs.is_empty() {
                problems.push(format!("{label} legs must be a non-empty array"));
            }
            let mut leg_ids = BTreeSet::new();
            let mut sequences = BTreeSet::new();
            for (leg_index, leg) in contract.legs.iter().enumerate() {
                let leg_label = format!("{label}.legs[{leg_index}]");
                required_text(&leg.leg_id, &format!("{leg_label} leg_id"), &mut problems);
                if !leg.leg_id.is_empty() && !leg_ids.insert(leg.leg_id.as_str()) {
                    problems.push(format!("{label} has duplicate leg_id {:?}", leg.leg_id));
                }
                required_text(&leg.mode, &format!("{leg_label} mode"), &mut problems);
                required_text(
                    &leg.origin_id,
                    &format!("{leg_label} origin_id"),
                    &mut problems,
                );
                required_text(
                    &leg.destination_id,
                    &format!("{leg_label} destination_id"),
                    &mut problems,
                );
                if leg.sequence == 0 {
                    problems.push(format!("{leg_label} sequence must be a positive integer"));
                } else if !sequences.insert(leg.sequence) {
                    problems.push(format!(
                        "{label} has duplicate leg sequence {}",
                        leg.sequence
                    ));
                }
            }
        }

        let mut event_ids = BTreeSet::new();
        for (index, event) in self.events.iter().enumerate() {
            let label = format!("events[{index}]");
            required_text(&event.event_id, &format!("{label} event_id"), &mut problems);
            if !event.event_id.is_empty() && !event_ids.insert(event.event_id.as_str()) {
                problems.push(format!("duplicate event_id {:?}", event.event_id));
            }
            required_text(
                &event.contract_id,
                &format!("{label} contract_id"),
                &mut problems,
            );
            required_text(&event.leg_id, &format!("{label} leg_id"), &mut problems);
            required_text(&event.kind, &format!("{label} kind"), &mut problems);
            required_text(
                &event.source_game,
                &format!("{label} source_game"),
                &mut problems,
            );
            if !self.producer.is_empty()
                && !event.source_game.is_empty()
                && event.source_game != self.producer
            {
                problems.push(format!("{label} source_game must match manifest producer"));
            }
            timestamp(
                &event.occurred_at,
                &format!("{label} occurred_at"),
                &mut problems,
            );
        }
        problems
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContractSnapshot {
    pub contract_id: String,
    pub state: String,
    pub leg_states: BTreeMap<String, String>,
    pub applied_event_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExchangeDirectory {
    pub manifests: Vec<ExchangeManifest>,
    pub contracts: Vec<ShipmentContract>,
    pub snapshots: Vec<ContractSnapshot>,
}

#[derive(Debug, Error, PartialEq)]
pub enum ExchangeError {
    #[error("namespace must be a non-empty identifier")]
    InvalidNamespace,
    #[error("conflicting {kind} id {id:?}")]
    ConflictingId { kind: &'static str, id: String },
    #[error("event {event_id:?} references unknown contract_id")]
    UnknownContract { event_id: String },
    #[error("event {event_id:?} references unknown leg_id")]
    UnknownLeg { event_id: String },
    #[error("could not serialize exchange record: {0}")]
    Serialization(String),
}

/// Return a stable cross-game ID from canonical JSON inputs.
pub fn stable_id(namespace: &str, parts: &[Value]) -> Result<String, ExchangeError> {
    let prefix = namespace.trim().to_lowercase();
    if !valid_namespace(&prefix) {
        return Err(ExchangeError::InvalidNamespace);
    }
    let mut canonical = Vec::with_capacity(parts.len() + 1);
    canonical.push(Value::String(prefix.clone()));
    canonical.extend_from_slice(parts);
    let bytes = serde_json::to_vec(&canonical)
        .map_err(|error| ExchangeError::Serialization(error.to_string()))?;
    let digest = hex::encode(Sha256::digest(bytes));
    Ok(format!("{prefix}_{}", &digest[..24]))
}

/// Select only the transport modes a consumer knows. Future modes are
/// intentionally ignored rather than rejected.
pub fn compatible_legs<'a>(
    contracts: &'a [ShipmentContract],
    modes: &BTreeSet<&str>,
) -> Vec<(&'a ShipmentContract, &'a TransportLeg)> {
    let mut legs = contracts
        .iter()
        .flat_map(|contract| {
            contract
                .legs
                .iter()
                .filter(|leg| modes.contains(leg.mode.as_str()))
                .map(move |leg| (contract, leg))
        })
        .collect::<Vec<_>>();
    legs.sort_by(|left, right| {
        left.0
            .contract_id
            .cmp(&right.0.contract_id)
            .then(left.1.sequence.cmp(&right.1.sequence))
            .then(left.1.leg_id.cmp(&right.1.leg_id))
    });
    legs
}

/// Project append-only events into deterministic lifecycle state.
///
/// Repeated identical IDs are idempotent. Reusing an ID for different
/// content is rejected. The first terminal event for a leg wins.
pub fn reconcile(
    contracts: &[ShipmentContract],
    events: &[UniverseEvent],
) -> Result<Vec<ContractSnapshot>, ExchangeError> {
    let contracts = unique_by_id(contracts, |contract| &contract.contract_id, "contract")?;
    let events = unique_by_id(events, |event| &event.event_id, "event")?;
    let mut states = contracts
        .iter()
        .map(|(contract_id, contract)| {
            (
                contract_id.clone(),
                contract
                    .legs
                    .iter()
                    .map(|leg| (leg.leg_id.clone(), "pending".to_string()))
                    .collect::<BTreeMap<_, _>>(),
            )
        })
        .collect::<BTreeMap<_, _>>();
    let mut applied = contracts
        .keys()
        .map(|contract_id| (contract_id.clone(), Vec::new()))
        .collect::<BTreeMap<_, _>>();
    let mut ordered_events = events.values().collect::<Vec<_>>();
    ordered_events.sort_by(|left, right| {
        parsed_timestamp(&left.occurred_at)
            .cmp(&parsed_timestamp(&right.occurred_at))
            .then(left.event_id.cmp(&right.event_id))
    });

    for event in ordered_events {
        let Some(contract_states) = states.get_mut(&event.contract_id) else {
            return Err(ExchangeError::UnknownContract {
                event_id: event.event_id.clone(),
            });
        };
        let Some(current) = contract_states.get_mut(&event.leg_id) else {
            return Err(ExchangeError::UnknownLeg {
                event_id: event.event_id.clone(),
            });
        };
        if !matches!(
            event.kind.as_str(),
            LEG_STARTED | LEG_COMPLETED | LEG_FAILED
        ) {
            continue;
        }
        if matches!(current.as_str(), "completed" | "failed") {
            continue;
        }
        *current = match event.kind.as_str() {
            LEG_STARTED => "in_progress",
            LEG_COMPLETED => "completed",
            LEG_FAILED => "failed",
            _ => unreachable!(),
        }
        .to_string();
        applied
            .get_mut(&event.contract_id)
            .expect("states and applied have the same contract IDs")
            .push(event.event_id.clone());
    }

    Ok(states
        .into_iter()
        .map(|(contract_id, leg_states)| {
            let values = leg_states
                .values()
                .map(String::as_str)
                .collect::<BTreeSet<_>>();
            let state = if values.contains("failed") {
                "failed"
            } else if values == BTreeSet::from(["completed"]) {
                "completed"
            } else if values == BTreeSet::from(["pending"]) {
                "open"
            } else {
                "in_progress"
            };
            ContractSnapshot {
                applied_event_ids: applied.remove(&contract_id).unwrap_or_default(),
                contract_id,
                state: state.to_string(),
                leg_states,
            }
        })
        .collect())
}

fn unique_by_id<'a, T: Serialize>(
    items: &'a [T],
    id: impl Fn(&'a T) -> &'a str,
    kind: &'static str,
) -> Result<BTreeMap<String, &'a T>, ExchangeError> {
    let mut unique = BTreeMap::new();
    let mut fingerprints = BTreeMap::new();
    for item in items {
        let item_id = id(item);
        let fingerprint = serde_json::to_vec(item)
            .map_err(|error| ExchangeError::Serialization(error.to_string()))?;
        if fingerprints
            .get(item_id)
            .is_some_and(|existing| existing != &fingerprint)
        {
            return Err(ExchangeError::ConflictingId {
                kind,
                id: item_id.to_string(),
            });
        }
        fingerprints.insert(item_id.to_string(), fingerprint);
        unique.insert(item_id.to_string(), item);
    }
    Ok(unique)
}

fn valid_namespace(namespace: &str) -> bool {
    let mut chars = namespace.chars();
    matches!(chars.next(), Some(first) if first.is_ascii_alphabetic() || first == '_')
        && chars
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn required_text(value: &str, label: &str, problems: &mut Vec<String>) {
    if value.trim().is_empty() {
        problems.push(format!("missing or invalid {label}"));
    }
}

fn timestamp(value: &str, label: &str, problems: &mut Vec<String>) {
    if DateTime::parse_from_rfc3339(value).is_err() {
        problems.push(format!(
            "{label} must be an ISO-8601 timestamp with timezone"
        ));
    }
}

fn parsed_timestamp(value: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_rfc3339(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn leg(id: &str, sequence: u32, mode: &str) -> TransportLeg {
        TransportLeg {
            leg_id: id.to_string(),
            mode: mode.to_string(),
            sequence,
            origin_id: "chicago_il_us:yard".to_string(),
            destination_id: "gary_in_us:mill".to_string(),
            ..TransportLeg::default()
        }
    }

    fn contract() -> ShipmentContract {
        ShipmentContract {
            contract_id: "contract-1".to_string(),
            source_game: "dispatch".to_string(),
            created_at: "2026-10-07T04:00:00Z".to_string(),
            cargo_key: "steel".to_string(),
            weight_tons: 20.0,
            legs: vec![leg("road-1", 1, "road"), leg("rail-1", 2, "rail")],
            terms: JsonObject::from_iter([("pay".to_string(), json!(4200))]),
        }
    }

    fn event(id: &str, leg_id: &str, kind: &str, occurred_at: &str) -> UniverseEvent {
        UniverseEvent {
            event_id: id.to_string(),
            contract_id: "contract-1".to_string(),
            leg_id: leg_id.to_string(),
            kind: kind.to_string(),
            occurred_at: occurred_at.to_string(),
            source_game: "freight_fate".to_string(),
            ..UniverseEvent::default()
        }
    }

    #[test]
    fn stable_id_matches_the_python_freightverse_fixture() {
        assert_eq!(
            stable_id("contract", &[json!("dispatch"), json!("load-42"), json!(3)]).unwrap(),
            "contract_ac5bf9b809abf468a67b7822"
        );
        assert_eq!(
            stable_id("bad namespace", &[]),
            Err(ExchangeError::InvalidNamespace)
        );
    }

    #[test]
    fn unknown_fields_and_transport_modes_are_forward_compatible() {
        let manifest: ExchangeManifest = serde_json::from_value(json!({
            "format": INTERCHANGE_FORMAT,
            "interchange_version": 2,
            "producer": "dispatch",
            "exported_at": "2026-10-07T04:00:00Z",
            "future_envelope_field": true,
            "contracts": [{
                "contract_id": "contract-1",
                "source_game": "dispatch",
                "created_at": "2026-10-07T04:00:00Z",
                "cargo_key": "steel",
                "weight_tons": 20,
                "future_contract_field": "ignored",
                "legs": [{
                    "leg_id": "air-1",
                    "mode": "air",
                    "sequence": 1,
                    "origin_id": "a",
                    "destination_id": "b",
                    "future_leg_field": 3
                }]
            }]
        }))
        .unwrap();
        assert!(manifest.validate().is_empty());
        assert!(compatible_legs(&manifest.contracts, &BTreeSet::from(["road"])).is_empty());
    }

    #[test]
    fn manifest_validation_collects_structural_problems() {
        let manifest = ExchangeManifest {
            producer: "dispatch".to_string(),
            exported_at: "2026-10-07T04:00:00Z".to_string(),
            contracts: vec![ShipmentContract {
                source_game: "freight_fate".to_string(),
                legs: vec![TransportLeg::default()],
                ..ShipmentContract::default()
            }],
            ..ExchangeManifest::default()
        };
        let problems = manifest.validate();
        assert!(problems
            .iter()
            .any(|problem| problem.contains("contract_id")));
        assert!(problems
            .iter()
            .any(|problem| problem.contains("source_game must match")));
        assert!(problems
            .iter()
            .any(|problem| problem.contains("weight_tons")));
        assert!(problems.iter().any(|problem| problem.contains("sequence")));
    }

    #[test]
    fn reconciliation_is_idempotent_and_first_terminal_event_wins() {
        let contract = contract();
        let started = event("event-1", "road-1", LEG_STARTED, "2026-10-07T04:01:00Z");
        let completed = event("event-2", "road-1", LEG_COMPLETED, "2026-10-07T04:02:00Z");
        let late_failure = event("event-3", "road-1", LEG_FAILED, "2026-10-07T04:03:00Z");
        let snapshots = reconcile(
            &[contract.clone(), contract],
            &[started.clone(), started, completed, late_failure],
        )
        .unwrap();
        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].state, "in_progress");
        assert_eq!(snapshots[0].leg_states["road-1"], "completed");
        assert_eq!(snapshots[0].leg_states["rail-1"], "pending");
        assert_eq!(snapshots[0].applied_event_ids, vec!["event-1", "event-2"]);
    }

    #[test]
    fn conflicting_reused_ids_are_rejected() {
        let first = contract();
        let mut conflicting = first.clone();
        conflicting.cargo_key = "grain".to_string();
        assert!(matches!(
            reconcile(&[first, conflicting], &[]),
            Err(ExchangeError::ConflictingId {
                kind: "contract",
                ..
            })
        ));
    }

    #[test]
    fn driver_exchange_excludes_game_owned_state_and_defaults_future_records() {
        let record: DriverRecord = serde_json::from_value(json!({
            "exchange_version": 2,
            "driver_id": "driver-1",
            "name": "Alex",
            "origin_game": "freight_fate",
            "exported_at": "2026-10-07T04:00:00Z",
            "future_field": "ignored"
        }))
        .unwrap();
        assert!(record.validate().is_empty());
        assert_eq!(record.reputation, 50.0);
        assert_eq!(record.on_time_share(), 1.0);
        let payload = serde_json::to_value(record).unwrap();
        assert!(payload.get("money").is_none());
        assert!(payload.get("active_trip").is_none());
        assert!(payload.get("truck").is_none());
    }

    #[test]
    fn dispatch_calls_are_forward_compatible_and_exclude_active_trip_state() {
        let request: DispatchCallRequest = serde_json::from_value(json!({
            "dispatch_call_version": 2,
            "request_id": "dispatch_request-1",
            "contract_id": "contract-1",
            "leg_id": "road-1",
            "source_game": "freight_fate",
            "driver_id": "driver-1",
            "kind": "delay",
            "created_at": "2026-10-07T06:00:00Z",
            "summary": "Running forty minutes behind.",
            "context": {
                "delay_minutes": 40,
                "hours_remaining": 3.5
            },
            "future_field": "ignored"
        }))
        .unwrap();
        assert!(request.validate().is_empty());
        let payload = serde_json::to_value(request).unwrap();
        assert!(payload.get("active_trip").is_none());
        assert!(payload.get("profile").is_none());
        assert!(payload.get("wallet").is_none());

        let response: DispatchCallResponse = serde_json::from_value(json!({
            "dispatch_call_version": 2,
            "response_id": "dispatch_response-1",
            "request_id": "dispatch_request-1",
            "source_game": "dispatch",
            "responder_id": "dispatcher-7",
            "decision": "continue",
            "responded_at": "2026-10-07T06:01:00Z",
            "message": "Continue and send an update after the next stop.",
            "effects": {
                "appointment_extension_minutes": 0
            },
            "future_field": true
        }))
        .unwrap();
        assert!(response.validate().is_empty());
    }

    #[test]
    fn dispatch_call_validation_collects_missing_identity_and_timestamps() {
        let request = DispatchCallRequest::default();
        let problems = request.validate();
        assert!(problems
            .iter()
            .any(|problem| problem.contains("request_id")));
        assert!(problems.iter().any(|problem| problem.contains("driver_id")));
        assert!(problems
            .iter()
            .any(|problem| problem.contains("created_at")));

        let response = DispatchCallResponse::default();
        let problems = response.validate();
        assert!(problems
            .iter()
            .any(|problem| problem.contains("response_id")));
        assert!(problems
            .iter()
            .any(|problem| problem.contains("request_id")));
        assert!(problems
            .iter()
            .any(|problem| problem.contains("responded_at")));
    }
}
