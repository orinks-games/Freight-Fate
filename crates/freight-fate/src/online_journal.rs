//! Port of `freight_fate/online_journal.py` — quiet, durable publishing of
//! allowlisted Freight Fate profile facts.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::net::{NetError, SharedTransport};
use crate::online_presence::{base_url, default_transport, truthy, OnlineIdentity};
use ff_core::pyfmt::{py_str_float, round_py_int, round_py_n};
use ff_core::sim::real_traffic::{wall_clock, Clock};

pub const MAX_OUTBOX_ITEMS: usize = 100;
pub const BASE_RETRY_S: f64 = 30.0;
pub const MAX_RETRY_S: f64 = 3600.0;

/// `json.dumps(value, ensure_ascii=True, separators=(",", ":"))`, byte for
/// byte, for the scalar-and-list shapes the event ids are built from.
/// Objects serialise in key order (serde's map is sorted; the Python sites
/// never pass one).
pub fn py_json_dumps(value: &Value) -> String {
    let mut out = String::new();
    dump_into(value, &mut out);
    out
}

fn dump_into(value: &Value, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else if let Some(f) = n.as_f64() {
                if f.is_nan() {
                    out.push_str("NaN");
                } else if f.is_infinite() {
                    out.push_str(if f > 0.0 { "Infinity" } else { "-Infinity" });
                } else {
                    out.push_str(&py_str_float(f));
                }
            }
        }
        Value::String(s) => dump_str(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                dump_into(item, out);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                dump_str(k, out);
                out.push(':');
                dump_into(v, out);
            }
            out.push('}');
        }
    }
}

/// Python's `ensure_ascii` string escaping: everything outside space..`~`
/// becomes `\uXXXX` (surrogate pairs past the BMP), with the short escapes
/// for the usual control characters.
fn dump_str(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(ch),
            _ => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
}

/// `stable_event_id(kind, *parts)`: the kind plus the first 24 hex
/// characters of the sha256 of the compact JSON list `[kind, *parts]`.
pub fn stable_event_id(kind: &str, parts: &[Value]) -> String {
    let mut list = vec![Value::from(kind)];
    list.extend(parts.iter().cloned());
    let canonical = py_json_dumps(&Value::Array(list));
    let digest = Sha256::digest(canonical.as_bytes());
    format!("{kind}-{}", &hex::encode(digest)[..24])
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutboxItem {
    pub endpoint: String,
    pub payload: Value,
    pub event_id: String,
    #[serde(default)]
    pub attempts: i64,
    #[serde(default)]
    pub next_attempt_at: f64,
}

impl OutboxItem {
    pub fn new(endpoint: &str, payload: Value, event_id: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            payload,
            event_id: event_id.to_string(),
            attempts: 0,
            next_attempt_at: 0.0,
        }
    }
}

struct Inner {
    identity: Mutex<Option<OnlineIdentity>>,
    enabled: AtomicBool,
    path: PathBuf,
    transport: SharedTransport,
    clock: Clock,
    items: Mutex<Vec<OutboxItem>>,
    // One sender at a time. A settlement can queue a delivery, a level up,
    // and several achievements in the same breath, each asking to flush --
    // left unguarded that is a thread per call, all walking the same due
    // list, posting the same events twice and colliding on the server's
    // per-driver write counter. Asking while a flush runs sets the rerun
    // flag instead, so the last caller's items still go out.
    flush_state: Mutex<(bool, bool)>, // (flushing, flush_again)
}

/// The durable outbox: events wait on disk until orinks.net takes them.
#[derive(Clone)]
pub struct JournalOutbox {
    inner: Arc<Inner>,
}

impl JournalOutbox {
    /// `JournalOutbox(identity, enabled, path)` with the real transport and
    /// wall clock.
    pub fn new(identity: Option<OnlineIdentity>, enabled: bool, path: &Path) -> Self {
        Self::with(identity, enabled, path, default_transport(), wall_clock())
    }

    /// `JournalOutbox(identity, enabled, path, transport=..., clock=...)`.
    pub fn with(
        identity: Option<OnlineIdentity>,
        enabled: bool,
        path: &Path,
        transport: SharedTransport,
        clock: Clock,
    ) -> Self {
        let me = Self {
            inner: Arc::new(Inner {
                identity: Mutex::new(identity),
                enabled: AtomicBool::new(enabled),
                path: path.to_path_buf(),
                transport,
                clock,
                items: Mutex::new(Vec::new()),
                flush_state: Mutex::new((false, false)),
            }),
        };
        me.inner.load();
        me
    }

    pub fn enabled(&self) -> bool {
        self.inner.enabled.load(Ordering::SeqCst)
    }

    pub fn identity(&self) -> Option<OnlineIdentity> {
        self.inner.identity.lock().unwrap().clone()
    }

    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// A snapshot of the queued items, oldest first.
    pub fn items(&self) -> Vec<OutboxItem> {
        self.inner.items.lock().unwrap().clone()
    }

    /// `box.items.append(item); box._save()` -- queue an item without the
    /// enabled/identity/dedupe checks. For tests that stage a pre-existing
    /// outbox.
    pub fn insert_item(&self, item: OutboxItem) {
        let mut items = self.inner.items.lock().unwrap();
        items.push(item);
        self.inner.save(&items);
    }

    pub fn set_enabled(&self, enabled: bool) {
        let enabled = enabled && self.inner.identity.lock().unwrap().is_some();
        self.inner.enabled.store(enabled, Ordering::SeqCst);
        if !enabled {
            let mut items = self.inner.items.lock().unwrap();
            items.clear();
            self.inner.save(&items);
        }
    }

    pub fn set_identity(&self, identity: Option<OnlineIdentity>) {
        let none = identity.is_none();
        *self.inner.identity.lock().unwrap() = identity;
        if none {
            self.inner.enabled.store(false, Ordering::SeqCst);
        }
    }

    /// Queue one event. False when the journal is off, the identity is
    /// missing, or the event id is already queued.
    pub fn enqueue(&self, endpoint: &str, payload: Value, event_id: &str) -> bool {
        if !self.enabled() || self.inner.identity.lock().unwrap().is_none() {
            return false;
        }
        let mut items = self.inner.items.lock().unwrap();
        if items.iter().any(|item| item.event_id == event_id) {
            return false;
        }
        items.push(OutboxItem::new(endpoint, payload, event_id));
        if items.len() > MAX_OUTBOX_ITEMS {
            let drop = items.len() - MAX_OUTBOX_ITEMS;
            items.drain(..drop);
        }
        self.inner.save(&items);
        true
    }

    /// Attempt due items once. Never raises or blocks gameplay callers.
    /// Returns how many were sent.
    pub fn flush(&self) -> usize {
        self.inner.flush()
    }

    /// Ask for a flush without blocking, and without racing one already in
    /// flight. Anything queued before this call still goes out: a request
    /// made mid-flush schedules one more pass rather than a second sender.
    pub fn flush_async(&self) {
        {
            let mut state = self.inner.flush_state.lock().unwrap();
            if state.0 {
                state.1 = true;
                return;
            }
            state.0 = true;
        }
        let inner = Arc::clone(&self.inner);
        let spawned = thread::Builder::new()
            .name("online-journal".to_string())
            .spawn(move || inner.flush_until_idle());
        if spawned.is_err() {
            let mut state = self.inner.flush_state.lock().unwrap();
            *state = (false, false);
        }
    }
}

impl Inner {
    fn load(&self) {
        let loaded: Vec<OutboxItem> = (|| {
            let text = fs::read_to_string(&self.path).ok()?;
            let raw: Value = serde_json::from_str(&text).ok()?;
            let items = raw.get("items")?.as_array()?;
            let mut out = Vec::with_capacity(items.len());
            for item in items {
                out.push(serde_json::from_value::<OutboxItem>(item.clone()).ok()?);
            }
            Some(out)
        })()
        .unwrap_or_default();
        let start = loaded.len().saturating_sub(MAX_OUTBOX_ITEMS);
        *self.items.lock().unwrap() = loaded[start..].to_vec();
    }

    fn save(&self, items: &[OutboxItem]) {
        let result = (|| -> std::io::Result<()> {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)?;
            }
            let temp = self.path.with_extension("tmp");
            let doc = json!({"version": 1, "items": items});
            let text = serde_json::to_string_pretty(&doc)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            fs::write(&temp, text)?;
            fs::rename(&temp, &self.path)
        })();
        if let Err(e) = result {
            log::debug!("Road journal outbox could not be saved: {e}");
        }
    }

    fn flush(&self) -> usize {
        let identity = match self.identity.lock().unwrap().clone() {
            Some(id) if self.enabled.load(Ordering::SeqCst) => id,
            _ => return 0,
        };
        let mut sent = 0;
        let now = (self.clock)();
        let due: Vec<OutboxItem> = self
            .items
            .lock()
            .unwrap()
            .iter()
            .filter(|item| item.next_attempt_at <= now)
            .cloned()
            .collect();
        for item in due {
            let mut payload = Map::new();
            payload.insert(
                "driverId".to_string(),
                Value::from(identity.driver_id.as_str()),
            );
            if let Value::Object(extra) = &item.payload {
                for (k, v) in extra {
                    payload.insert(k.clone(), v.clone());
                }
            }
            let reply = self.transport.call(
                &format!("{}{}", base_url(), item.endpoint),
                Some(&Value::Object(payload)),
                &identity.auth_headers(),
                None,
            );
            let ok = match reply {
                Ok(reply) => truthy(reply.get("ok")),
                Err(NetError::Http { code, .. }) => {
                    // Authentication, consent, and validation failures cannot heal
                    // through retries. Rate limiting and server failures can.
                    if matches!(code, 400 | 401 | 403 | 404) {
                        let mut items = self.items.lock().unwrap();
                        items.retain(|value| value.event_id != item.event_id);
                        self.save(&items);
                        continue;
                    }
                    log::debug!("Road journal post failed: HTTP {code}");
                    false
                }
                Err(e) => {
                    log::debug!("Road journal post failed: {e}");
                    false
                }
            };
            let mut items = self.items.lock().unwrap();
            let Some(index) = items.iter().position(|v| v.event_id == item.event_id) else {
                continue;
            };
            if ok {
                items.remove(index);
                sent += 1;
            } else {
                let current = &mut items[index];
                current.attempts += 1;
                let exponent = (current.attempts - 1).clamp(0, 7) as i32;
                current.next_attempt_at = now + MAX_RETRY_S.min(BASE_RETRY_S * 2f64.powi(exponent));
            }
            self.save(&items);
        }
        sent
    }

    fn flush_until_idle(&self) {
        loop {
            self.flush();
            let mut state = self.flush_state.lock().unwrap();
            if !state.1 {
                state.0 = false;
                return;
            }
            state.1 = false;
        }
    }
}

// -- event payload builders ---------------------------------------------------------

/// The allowlisted facts of a completed delivery the journal publishes:
/// the profile and job fields `queue_delivery` reads (`Profile` and `Job` are not part of this crate).
#[derive(Debug, Clone, PartialEq)]
pub struct DeliveryFacts {
    /// `profile.name`
    pub profile_name: String,
    /// `profile.career.deliveries`
    pub deliveries: i64,
    /// `job.cargo.key`
    pub cargo_key: String,
    /// `job.cargo.label`
    pub cargo_label: String,
    /// `job.origin` (the world node id)
    pub job_origin: String,
    /// `job.destination` (the world node id)
    pub job_destination: String,
    /// `job.distance_mi`
    pub distance_mi: f64,
    /// `job.weight_tons`
    pub weight_tons: f64,
    /// On a team load, who drove how much of it; said in place of the
    /// condition line.
    pub team_note: Option<String>,
}

#[allow(clippy::too_many_arguments)]
pub fn queue_delivery(
    outbox: &JournalOutbox,
    facts: &DeliveryFacts,
    origin: &str,
    destination: &str,
    on_time: bool,
    occurred_at_ms: i64,
    undamaged: bool,
) -> bool {
    let distance = round_py_n(facts.distance_mi, 1);
    let event_id = stable_event_id(
        "delivery",
        &[
            Value::from(facts.profile_name.as_str()),
            Value::from(facts.deliveries),
            Value::from(facts.cargo_key.as_str()),
            Value::from(facts.job_origin.as_str()),
            Value::from(facts.job_destination.as_str()),
            Value::from(distance),
        ],
    );
    let mut inner = Map::new();
    inner.insert("version".to_string(), json!(1));
    inner.insert("cargo".to_string(), Value::from(facts.cargo_label.as_str()));
    inner.insert(
        "weightPounds".to_string(),
        Value::from(round_py_int(facts.weight_tons * 2000.0)),
    );
    inner.insert("origin".to_string(), Value::from(origin));
    inner.insert("destination".to_string(), Value::from(destination));
    inner.insert("distanceMiles".to_string(), Value::from(distance));
    inner.insert("onTime".to_string(), Value::from(on_time));
    if let Some(note) = &facts.team_note {
        inner.insert("notableCondition".to_string(), Value::from(note.as_str()));
    } else if undamaged {
        inner.insert(
            "notableCondition".to_string(),
            Value::from("Delivered without new truck damage"),
        );
    }
    let payload = json!({
        "eventId": event_id,
        "occurredAt": occurred_at_ms,
        "payload": Value::Object(inner),
    });
    outbox.enqueue("/api/freight-fate/events/delivery", payload, &event_id)
}

/// `queue_achievement(outbox, achievement, earned_at_ms=...)` with the
/// achievement's id, name and description passed in.
pub fn queue_achievement(
    outbox: &JournalOutbox,
    achievement_id: &str,
    name: &str,
    description: &str,
    earned_at_ms: i64,
) -> bool {
    let event_id = stable_event_id("achievement", &[Value::from(achievement_id)]);
    outbox.enqueue(
        "/api/freight-fate/events/achievement",
        json!({
            "eventId": event_id,
            "achievementKey": achievement_id,
            "name": name,
            "description": description,
            "earnedAt": earned_at_ms,
        }),
        &event_id,
    )
}

/// The career facts `queue_career_milestones` reads off the profile.
#[derive(Debug, Clone, PartialEq)]
pub struct CareerFacts {
    pub profile_name: String,
    pub deliveries: i64,
    pub level: i64,
}

/// Queue the milestones a settlement proved: the first delivery, and a
/// level above `previous_level`. Returns how many were queued.
pub fn queue_career_milestones(
    outbox: &JournalOutbox,
    facts: &CareerFacts,
    previous_level: i64,
    occurred_at_ms: i64,
) -> usize {
    let mut milestones: Vec<(&str, Option<i64>)> = Vec::new();
    if facts.deliveries == 1 {
        milestones.push(("first_delivery", None));
    }
    if facts.level > previous_level {
        milestones.push(("career_level", Some(facts.level)));
    }
    let mut queued = 0;
    for (milestone_type, level) in milestones {
        let event_id = stable_event_id(
            "career",
            &[
                Value::from(milestone_type),
                Value::from(facts.profile_name.as_str()),
                Value::from(level.unwrap_or(facts.deliveries)),
            ],
        );
        let mut payload = Map::new();
        payload.insert("eventId".to_string(), Value::from(event_id.as_str()));
        payload.insert("milestoneType".to_string(), Value::from(milestone_type));
        if let Some(level) = level {
            payload.insert("level".to_string(), Value::from(level));
        }
        payload.insert("occurredAt".to_string(), Value::from(occurred_at_ms));
        if outbox.enqueue(
            "/api/freight-fate/events/career-milestone",
            Value::Object(payload),
            &event_id,
        ) {
            queued += 1;
        }
    }
    queued
}
