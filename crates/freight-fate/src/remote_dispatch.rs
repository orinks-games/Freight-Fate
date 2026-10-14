//! Off-loop caller for remote dispatch calls.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::net::{SharedTransport, Transport};
use crate::online_presence::{self, OnlineIdentity};

pub const POLL_INTERVAL: Duration = Duration::from_secs(3);
pub const RING_WINDOW: Duration = Duration::from_secs(60);
pub const CLAIMED_WINDOW: Duration = Duration::from_secs(90);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub poll_interval: Duration,
    pub ring_window: Duration,
    pub claimed_window: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self {
            poll_interval: POLL_INTERVAL,
            ring_window: RING_WINDOW,
            claimed_window: CLAIMED_WINDOW,
        }
    }
}

pub trait Clock: Send + Sync {
    fn now(&self) -> Duration;
    fn wait(&self, duration: Duration);
}

struct WallClock(Instant);

impl Clock for WallClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }

    fn wait(&self, duration: Duration) {
        thread::sleep(duration);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkerEvent {
    Claimed,
    Answered(String),
    Failed,
    Cancelled,
}

pub struct CallTask {
    pub events: Receiver<WorkerEvent>,
    cancel: Arc<AtomicBool>,
}

impl CallTask {
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    #[cfg(test)]
    pub(crate) fn from_parts(events: Receiver<WorkerEvent>, cancel: Arc<AtomicBool>) -> Self {
        Self { events, cancel }
    }
}

pub fn spawn(
    identity: OnlineIdentity,
    request_id: String,
    kind: String,
    facts: CallFacts,
) -> std::io::Result<CallTask> {
    spawn_with(
        identity,
        request_id,
        kind,
        facts,
        online_presence::default_transport(),
        Arc::new(WallClock(Instant::now())),
        Timing::default(),
    )
}

pub fn spawn_with(
    identity: OnlineIdentity,
    request_id: String,
    kind: String,
    facts: CallFacts,
    transport: SharedTransport,
    clock: Arc<dyn Clock>,
    timing: Timing,
) -> std::io::Result<CallTask> {
    let (sender, events) = mpsc::sync_channel(4);
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    thread::Builder::new()
        .name("remote-dispatch-call".to_string())
        .spawn(move || {
            run(
                &identity,
                &request_id,
                &kind,
                &facts,
                transport.as_ref(),
                clock.as_ref(),
                timing,
                &worker_cancel,
                &sender,
            )
        })?;
    Ok(CallTask { events, cancel })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CallFacts {
    pub remaining_miles: f64,
    pub hours_left: f64,
    pub truck_damage_pct: f64,
    pub cargo_damage_pct: f64,
    pub hos_remaining_minutes: Option<f64>,
    pub weather_alerts: usize,
}

impl CallFacts {
    pub fn payload(&self) -> Value {
        let mut facts = Map::from_iter([
            ("remainingMiles".to_string(), json!(self.remaining_miles)),
            ("hoursLeft".to_string(), json!(self.hours_left)),
            ("truckDamagePct".to_string(), json!(self.truck_damage_pct)),
            ("cargoDamagePct".to_string(), json!(self.cargo_damage_pct)),
            ("weatherAlerts".to_string(), json!(self.weather_alerts)),
        ]);
        if let Some(minutes) = self.hos_remaining_minutes {
            facts.insert("hosRemainingMinutes".to_string(), json!(minutes));
        }
        Value::Object(facts)
    }
}

fn decision_is_valid(kind: &str, decision: &str) -> bool {
    match kind {
        "delay" => matches!(decision, "continue" | "watch" | "late_update"),
        "hours" => matches!(decision, "plan_rest" | "stop"),
        "road_conditions" => matches!(decision, "continue" | "caution"),
        "truck_trouble" => matches!(decision, "monitor" | "repair_authorized"),
        "load_trouble" => matches!(decision, "continue" | "protect_load"),
        _ => false,
    }
}

#[allow(clippy::too_many_arguments)]
fn run(
    identity: &OnlineIdentity,
    request_id: &str,
    kind: &str,
    facts: &CallFacts,
    transport: &dyn Transport,
    clock: &dyn Clock,
    timing: Timing,
    cancel: &AtomicBool,
    events: &SyncSender<WorkerEvent>,
) {
    if cancel.load(Ordering::SeqCst) {
        return;
    }
    let base = online_presence::base_url();
    let headers = identity.auth_headers();
    let create_url = format!("{base}/api/freight-fate/dispatch-calls");
    let created = transport.call(
        &create_url,
        Some(&json!({
            "driverId": identity.driver_id,
            "requestId": request_id,
            "kind": kind,
            "facts": facts.payload()
        })),
        &headers,
        None,
    );
    let Ok(created) = created else {
        let _ = events.send(WorkerEvent::Failed);
        return;
    };
    let Some(call_id) = created.get("callId").and_then(Value::as_str) else {
        let _ = events.send(WorkerEvent::Failed);
        return;
    };
    if call_id.is_empty() {
        let _ = events.send(WorkerEvent::Failed);
        return;
    }

    let cancel_url = format!("{base}/api/freight-fate/dispatch-calls/cancel");
    if created.get("ringUntil").and_then(Value::as_i64).is_none() {
        fail_call(transport, &cancel_url, identity, call_id, &headers, events);
        return;
    }

    let started = clock.now();
    let status_url = format!("{base}/api/freight-fate/dispatch-calls/status");
    let mut claimed_until = None;
    let mut announced_claim = false;
    let mut consecutive_poll_failures = 0;
    loop {
        if cancel.load(Ordering::SeqCst) {
            cancel_call(transport, &cancel_url, identity, call_id, &headers);
            let _ = events.send(WorkerEvent::Cancelled);
            return;
        }
        let deadline = claimed_until.unwrap_or(started + timing.ring_window);
        let now = clock.now();
        if now >= deadline {
            fail_call(transport, &cancel_url, identity, call_id, &headers, events);
            return;
        }
        clock.wait(timing.poll_interval.min(deadline - now));
        if cancel.load(Ordering::SeqCst) {
            continue;
        }
        let status = transport.call(
            &status_url,
            Some(&json!({"driverId": identity.driver_id, "callId": call_id})),
            &headers,
            None,
        );
        if cancel.load(Ordering::SeqCst) {
            cancel_call(transport, &cancel_url, identity, call_id, &headers);
            let _ = events.send(WorkerEvent::Cancelled);
            return;
        }
        let status = match status {
            Ok(status) => {
                consecutive_poll_failures = 0;
                status
            }
            Err(error) if retry_status_poll(&error) && consecutive_poll_failures < 2 => {
                consecutive_poll_failures += 1;
                continue;
            }
            Err(_) => {
                fail_call(transport, &cancel_url, identity, call_id, &headers, events);
                return;
            }
        };
        let Some(state) = status.get("status").and_then(Value::as_str) else {
            fail_call(transport, &cancel_url, identity, call_id, &headers, events);
            return;
        };
        match state {
            "ringing" if claimed_until.is_none() => {}
            "claimed" => {
                if claimed_until.is_none() {
                    claimed_until = Some(clock.now() + timing.claimed_window);
                }
                if !announced_claim {
                    announced_claim = true;
                    if events.send(WorkerEvent::Claimed).is_err() {
                        cancel_call(transport, &cancel_url, identity, call_id, &headers);
                        return;
                    }
                }
            }
            "answered" => {
                if cancel.load(Ordering::SeqCst) {
                    cancel_call(transport, &cancel_url, identity, call_id, &headers);
                    let _ = events.send(WorkerEvent::Cancelled);
                    return;
                }
                if !announced_claim && events.send(WorkerEvent::Claimed).is_err() {
                    return;
                }
                let Some(decision) = status.get("decision").and_then(Value::as_str) else {
                    fail_call(transport, &cancel_url, identity, call_id, &headers, events);
                    return;
                };
                if decision_is_valid(kind, decision) {
                    let _ = events.send(WorkerEvent::Answered(decision.to_string()));
                } else {
                    fail_call(transport, &cancel_url, identity, call_id, &headers, events);
                }
                return;
            }
            _ => {
                fail_call(transport, &cancel_url, identity, call_id, &headers, events);
                return;
            }
        }
    }
}

fn retry_status_poll(error: &crate::net::NetError) -> bool {
    match error {
        crate::net::NetError::Http { code, .. } => !(400..500).contains(code),
        crate::net::NetError::Other { type_name, .. } => {
            !matches!(type_name.as_str(), "JSONDecodeError" | "UnicodeDecodeError")
        }
        _ => true,
    }
}

fn fail_call(
    transport: &dyn Transport,
    cancel_url: &str,
    identity: &OnlineIdentity,
    call_id: &str,
    headers: &[(String, String)],
    events: &SyncSender<WorkerEvent>,
) {
    cancel_call(transport, cancel_url, identity, call_id, headers);
    let _ = events.send(WorkerEvent::Failed);
}

fn cancel_call(
    transport: &dyn Transport,
    url: &str,
    identity: &OnlineIdentity,
    call_id: &str,
    headers: &[(String, String)],
) {
    let _ = transport.call(
        url,
        Some(&json!({"driverId": identity.driver_id, "callId": call_id})),
        headers,
        None,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicU64;
    use std::sync::Mutex;

    type CapturedRequest = (String, Value, Vec<(String, String)>);

    struct FakeClock(AtomicU64);

    impl FakeClock {
        fn new() -> Self {
            Self(AtomicU64::new(0))
        }
    }

    impl Clock for FakeClock {
        fn now(&self) -> Duration {
            Duration::from_nanos(self.0.load(Ordering::SeqCst))
        }

        fn wait(&self, duration: Duration) {
            self.0
                .fetch_add(duration.as_nanos() as u64, Ordering::SeqCst);
        }
    }

    struct CancellingClock {
        nanos: AtomicU64,
        cancel: Arc<AtomicBool>,
    }

    impl Clock for CancellingClock {
        fn now(&self) -> Duration {
            Duration::from_nanos(self.nanos.load(Ordering::SeqCst))
        }

        fn wait(&self, duration: Duration) {
            self.nanos
                .fetch_add(duration.as_nanos() as u64, Ordering::SeqCst);
            self.cancel.store(true, Ordering::SeqCst);
        }
    }

    struct FakeTransport {
        replies: Mutex<VecDeque<Result<Value, crate::net::NetError>>>,
        requests: Mutex<Vec<CapturedRequest>>,
    }

    impl FakeTransport {
        fn new(replies: impl IntoIterator<Item = Result<Value, crate::net::NetError>>) -> Self {
            Self {
                replies: Mutex::new(replies.into_iter().collect()),
                requests: Mutex::new(Vec::new()),
            }
        }
    }

    impl Transport for FakeTransport {
        fn call(
            &self,
            url: &str,
            payload: Option<&Value>,
            headers: &[(String, String)],
            _method: Option<&str>,
        ) -> Result<Value, crate::net::NetError> {
            self.requests.lock().unwrap().push((
                url.to_string(),
                payload.cloned().unwrap_or(Value::Null),
                headers.to_vec(),
            ));
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(json!({"status": "ringing"})))
        }
    }

    fn identity() -> OnlineIdentity {
        OnlineIdentity {
            driver_id: "driver-12345678".to_string(),
            driver_token: "token-123456789012345678901234".to_string(),
        }
    }

    fn facts() -> CallFacts {
        CallFacts {
            remaining_miles: 50.0,
            hours_left: 3.5,
            truck_damage_pct: 0.0,
            cargo_damage_pct: 0.0,
            hos_remaining_minutes: None,
            weather_alerts: 0,
        }
    }

    fn timing() -> Timing {
        Timing {
            poll_interval: Duration::from_secs(3),
            ring_window: Duration::from_secs(6),
            claimed_window: Duration::from_secs(6),
        }
    }

    fn run_fake(
        replies: impl IntoIterator<Item = Result<Value, crate::net::NetError>>,
        timing: Timing,
        cancel: &AtomicBool,
    ) -> (Vec<WorkerEvent>, Vec<CapturedRequest>) {
        let transport = FakeTransport::new(replies);
        let (sender, receiver) = mpsc::sync_channel(4);
        run(
            &identity(),
            "request-1",
            "delay",
            &facts(),
            &transport,
            &FakeClock::new(),
            timing,
            cancel,
            &sender,
        );
        (
            receiver.try_iter().collect(),
            transport.requests.into_inner().unwrap(),
        )
    }

    #[test]
    fn request_contains_only_the_expected_facts_and_identity_fields() {
        let (events, requests) = run_fake(
            [
                Ok(json!({"callId": "call-123456789012345678901234", "ringUntil": 10})),
                Ok(json!({"status": "answered", "decision": "continue"})),
            ],
            timing(),
            &AtomicBool::new(false),
        );
        assert_eq!(
            events,
            vec![
                WorkerEvent::Claimed,
                WorkerEvent::Answered("continue".to_string())
            ]
        );
        let body = &requests[0].1;
        assert_eq!(body["driverId"], identity().driver_id);
        assert_eq!(body["requestId"], "request-1");
        assert_eq!(body["kind"], "delay");
        assert_eq!(
            body.as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            ["driverId", "facts", "kind", "requestId"]
        );
        assert_eq!(
            body["facts"]
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            [
                "cargoDamagePct",
                "hoursLeft",
                "remainingMiles",
                "truckDamagePct",
                "weatherAlerts"
            ]
        );
        assert_eq!(requests[0].2, identity().auth_headers());
        assert!(requests[0].0.ends_with("/api/freight-fate/dispatch-calls"));
    }

    #[test]
    fn facts_include_hos_only_when_present() {
        let mut fact = facts();
        fact.hos_remaining_minutes = Some(75.0);
        assert_eq!(fact.payload()["hosRemainingMinutes"], 75.0);
        assert_eq!(
            fact.payload()
                .as_object()
                .unwrap()
                .keys()
                .cloned()
                .collect::<Vec<_>>(),
            [
                "cargoDamagePct",
                "hosRemainingMinutes",
                "hoursLeft",
                "remainingMiles",
                "truckDamagePct",
                "weatherAlerts"
            ]
        );
    }

    #[test]
    fn ring_timeout_falls_back_and_claimed_answer_window_expires() {
        let ring = run_fake(
            [Ok(
                json!({"callId": "call-123456789012345678901234", "ringUntil": 10}),
            )],
            timing(),
            &AtomicBool::new(false),
        );
        assert_eq!(ring.0, vec![WorkerEvent::Failed]);
        assert!(ring
            .1
            .last()
            .unwrap()
            .0
            .ends_with("/api/freight-fate/dispatch-calls/cancel"));
        let claimed = run_fake(
            std::iter::once(Ok(
                json!({"callId": "call-123456789012345678901234", "ringUntil": 10}),
            ))
            .chain(std::iter::repeat_n(Ok(json!({"status": "claimed"})), 3)),
            timing(),
            &AtomicBool::new(false),
        );
        assert_eq!(claimed.0, vec![WorkerEvent::Claimed, WorkerEvent::Failed]);
    }

    #[test]
    fn two_transient_poll_errors_then_answered_uses_the_answer() {
        let mut retry_timing = timing();
        retry_timing.ring_window = Duration::from_secs(12);
        let result = run_fake(
            [
                Ok(json!({"callId": "call-123456789012345678901234", "ringUntil": 10})),
                Err(crate::net::NetError::http(500)),
                Err(crate::net::NetError::http(500)),
                Ok(json!({"status": "answered", "decision": "continue"})),
            ],
            retry_timing,
            &AtomicBool::new(false),
        );
        assert_eq!(
            result.0,
            vec![
                WorkerEvent::Claimed,
                WorkerEvent::Answered("continue".to_string())
            ]
        );
    }

    #[test]
    fn three_transient_poll_errors_fail_and_cancel() {
        let mut retry_timing = timing();
        retry_timing.ring_window = Duration::from_secs(12);
        let result = run_fake(
            [
                Ok(json!({"callId": "call-123456789012345678901234", "ringUntil": 10})),
                Err(crate::net::NetError::http(500)),
                Err(crate::net::NetError::http(500)),
                Err(crate::net::NetError::http(500)),
            ],
            retry_timing,
            &AtomicBool::new(false),
        );
        assert_eq!(result.0, vec![WorkerEvent::Failed]);
        assert!(result
            .1
            .last()
            .unwrap()
            .0
            .ends_with("/api/freight-fate/dispatch-calls/cancel"));
    }

    #[test]
    fn http_errors_malformed_status_and_unknown_decisions_fail() {
        for replies in [
            vec![Err(crate::net::NetError::http(500))],
            vec![Err(crate::net::NetError::http(401))],
            vec![Err(crate::net::NetError::http(302))],
            vec![Err(crate::net::NetError::Other {
                type_name: "JSONDecodeError".to_string(),
                message: "malformed JSON".to_string(),
            })],
            vec![
                Ok(json!({"callId": "call-123456789012345678901234", "ringUntil": 10})),
                Err(crate::net::NetError::http(500)),
            ],
            vec![
                Ok(json!({"callId": "call-123456789012345678901234", "ringUntil": 10})),
                Err(crate::net::NetError::http(401)),
            ],
            vec![
                Ok(json!({"callId": "call-123456789012345678901234", "ringUntil": 10})),
                Err(crate::net::NetError::http(302)),
            ],
            vec![
                Ok(json!({"callId": "call-123456789012345678901234", "ringUntil": 10})),
                Err(crate::net::NetError::Other {
                    type_name: "JSONDecodeError".to_string(),
                    message: "malformed JSON".to_string(),
                }),
            ],
            vec![
                Ok(json!({"callId": "call-123456789012345678901234", "ringUntil": 10})),
                Ok(json!({"status": "unrecognized"})),
            ],
            vec![
                Ok(json!({"callId": "call-123456789012345678901234", "ringUntil": 10})),
                Ok(json!({"status": "answered", "decision": "invented"})),
            ],
        ] {
            let (events, requests) = run_fake(replies, timing(), &AtomicBool::new(false));
            assert!(
                matches!(
                    events.as_slice(),
                    [WorkerEvent::Failed] | [WorkerEvent::Claimed, WorkerEvent::Failed]
                ),
                "{events:?}"
            );
            if requests.len() > 1 {
                assert!(
                    requests
                        .last()
                        .unwrap()
                        .0
                        .ends_with("/api/freight-fate/dispatch-calls/cancel"),
                    "{requests:?}"
                );
            }
        }
    }

    #[test]
    fn cancellation_posts_cancel_from_the_worker() {
        let cancel = Arc::new(AtomicBool::new(false));
        let transport = FakeTransport::new([Ok(json!({
            "callId": "call-123456789012345678901234",
            "ringUntil": 10
        }))]);
        let (sender, receiver) = mpsc::sync_channel(4);
        run(
            &identity(),
            "request-1",
            "delay",
            &facts(),
            &transport,
            &CancellingClock {
                nanos: AtomicU64::new(0),
                cancel: Arc::clone(&cancel),
            },
            timing(),
            &cancel,
            &sender,
        );
        assert_eq!(
            receiver.try_iter().collect::<Vec<_>>(),
            vec![WorkerEvent::Cancelled]
        );
        let requests = transport.requests.into_inner().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests[1]
            .0
            .ends_with("/api/freight-fate/dispatch-calls/cancel"));
    }
}
