//! Port of `tests/test_online_journal.py` and the journal half of
//! `tests/test_delivery_summary_sharing.py`.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use ff_core::sim::real_traffic::{wall_clock, Clock};
use freight_fate::net::testing::ClosureTransport;
use freight_fate::net::{Event, NetError, SharedTransport};
use freight_fate::online_journal::{
    py_json_dumps, queue_achievement, queue_career_milestones, queue_delivery, stable_event_id,
    CareerFacts, DeliveryFacts, JournalOutbox, OutboxItem,
};
use freight_fate::online_presence::OnlineIdentity;

fn identity() -> OnlineIdentity {
    OnlineIdentity::new("driver-1234", &format!("ffd_{}", "a".repeat(64)))
}

/// `Job(CARGO_CATALOG["general"], 20, "chicago_il_us", "terminal",
/// "denver_co_us", 1000, 2000, 20)` under `Profile(name="Road Star")`.
fn facts(deliveries: i64) -> DeliveryFacts {
    DeliveryFacts {
        profile_name: "Road Star".to_string(),
        deliveries,
        cargo_key: "general".to_string(),
        cargo_label: "General freight".to_string(),
        job_origin: "chicago_il_us".to_string(),
        job_destination: "denver_co_us".to_string(),
        distance_mi: 1000.0,
        weight_tons: 20.0,
        team_note: None,
    }
}

fn ok_transport() -> SharedTransport {
    Arc::new(ClosureTransport(
        |_u: &str, _p: Option<&Value>, _h: &[(String, String)], _m: Option<&str>| {
            Ok(json!({"ok": true}))
        },
    ))
}

fn outbox_in(dir: &tempfile::TempDir, transport: SharedTransport) -> JournalOutbox {
    JournalOutbox::with(
        Some(identity()),
        true,
        &dir.path().join("outbox.json"),
        transport,
        wall_clock(),
    )
}

#[test]
fn test_stable_event_id_is_deterministic_and_fact_sensitive() {
    let a = stable_event_id("delivery", &[json!("job-1"), json!(4)]);
    assert_eq!(a, stable_event_id("delivery", &[json!("job-1"), json!(4)]));
    assert_ne!(a, stable_event_id("delivery", &[json!("job-2"), json!(4)]));
}

#[test]
fn test_stable_event_id_matches_the_python_layout() {
    // Values produced by the Python module for the same facts; the ids must
    // agree or every event already journaled by the Python build would post
    // a second time from the Rust one.
    assert_eq!(
        stable_event_id("delivery", &[json!("job-1"), json!(4)]),
        "delivery-9d6c9fbe0e4df51d6cba2d11"
    );
    assert_eq!(
        stable_event_id(
            "delivery",
            &[
                json!("Road Star"),
                json!(1),
                json!("general"),
                json!("chicago_il_us"),
                json!("denver_co_us"),
                json!(1000.0)
            ]
        ),
        "delivery-36b5b69d37c81f30cf8da288"
    );
    assert_eq!(
        stable_event_id("achievement", &[json!("first_delivery")]),
        "achievement-5aedb71e0d6d7b6e050d519d"
    );
    assert_eq!(
        stable_event_id(
            "career",
            &[json!("first_delivery"), json!("Road Star"), json!(1)]
        ),
        "career-6123b4ba95016f8f1edf14cf"
    );
    assert_eq!(
        stable_event_id("x", &[json!("café"), json!(1.5), Value::Null, json!(true)]),
        "x-4155e1efd23faad54532c9ca"
    );
    assert_eq!(
        py_json_dumps(&json!(["x", "café", 1.5, null, true, 1000.0, "a\"b\\c\n"])),
        r#"["x","caf\u00e9",1.5,null,true,1000.0,"a\"b\\c\n"]"#
    );
}

#[test]
fn test_outbox_persists_deduplicates_and_retries() {
    let calls = Arc::new(Mutex::new(0usize));
    let now = Arc::new(Mutex::new(100.0f64));
    let transport: SharedTransport = {
        let calls = Arc::clone(&calls);
        Arc::new(ClosureTransport(
            move |_u: &str, _p: Option<&Value>, _h: &[(String, String)], _m: Option<&str>| {
                let mut calls = calls.lock().unwrap();
                *calls += 1;
                if *calls == 1 {
                    return Err(NetError::other("OSError", "offline"));
                }
                Ok(json!({"ok": true}))
            },
        ))
    };
    let clock: Clock = {
        let now = Arc::clone(&now);
        Arc::new(move || *now.lock().unwrap())
    };
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("outbox.json");
    let outbox = JournalOutbox::with(
        Some(identity()),
        true,
        &path,
        Arc::clone(&transport),
        clock.clone(),
    );
    assert!(outbox.enqueue("/events", json!({"value": 1}), "evt-1"));
    assert!(!outbox.enqueue("/events", json!({"value": 1}), "evt-1"));
    assert_eq!(outbox.flush(), 0);
    let restored = JournalOutbox::with(Some(identity()), true, &path, transport, clock);
    assert_eq!(restored.items()[0].attempts, 1);
    assert_eq!(restored.flush(), 0);
    *now.lock().unwrap() = restored.items()[0].next_attempt_at;
    assert_eq!(restored.flush(), 1);
    assert!(restored.items().is_empty());
}

#[test]
fn test_disabled_outbox_never_queues_or_posts() {
    let dir = tempfile::tempdir().unwrap();
    let transport: SharedTransport = Arc::new(ClosureTransport(
        |_u: &str, _p: Option<&Value>, _h: &[(String, String)], _m: Option<&str>| {
            panic!("the disabled outbox must never post")
        },
    ));
    let outbox = JournalOutbox::with(
        None,
        false,
        &dir.path().join("outbox.json"),
        transport,
        wall_clock(),
    );
    assert!(!outbox.enqueue("/events", json!({}), "evt"));
    assert_eq!(outbox.flush(), 0);
}

#[test]
fn test_delivery_payload_is_structured_and_duplicate_completion_is_suppressed() {
    let dir = tempfile::tempdir().unwrap();
    let outbox = outbox_in(&dir, ok_transport());
    let facts = facts(1);
    assert!(queue_delivery(
        &outbox,
        &facts,
        "Chicago, Illinois",
        "Denver, Colorado",
        true,
        123,
        true,
    ));
    assert!(!queue_delivery(
        &outbox,
        &facts,
        "Chicago, Illinois",
        "Denver, Colorado",
        true,
        456,
        true,
    ));
    let payload = outbox.items()[0].payload.clone();
    assert_eq!(payload["payload"]["weightPounds"], 40_000);
    assert_eq!(
        payload["payload"]["notableCondition"],
        "Delivered without new truck damage"
    );
    assert!(payload.get("summary").is_none());
    assert_eq!(payload["payload"]["distanceMiles"], 1000.0);
    assert_eq!(payload["payload"]["cargo"], "General freight");
}

#[test]
fn test_achievement_payload_uses_official_definition_and_deduplicates() {
    let dir = tempfile::tempdir().unwrap();
    let outbox = outbox_in(&dir, ok_transport());
    // ACHIEVEMENT_BY_ID["first_delivery"]'s name and description travel
    // verbatim from the catalog the caller hands in.
    assert!(queue_achievement(
        &outbox,
        "first_delivery",
        "Signed On",
        "Complete your first delivery.",
        123
    ));
    assert!(!queue_achievement(
        &outbox,
        "first_delivery",
        "Signed On",
        "Complete your first delivery.",
        456
    ));
    assert_eq!(outbox.items()[0].payload["name"], "Signed On");
    assert_eq!(
        outbox.items()[0].payload["description"],
        "Complete your first delivery."
    );
    assert_eq!(
        outbox.items()[0].event_id,
        "achievement-5aedb71e0d6d7b6e050d519d"
    );
}

#[test]
fn test_permanent_consent_error_is_dropped_without_retrying() {
    let dir = tempfile::tempdir().unwrap();
    let denied: SharedTransport = Arc::new(ClosureTransport(
        |_u: &str, _p: Option<&Value>, _h: &[(String, String)], _m: Option<&str>| {
            Err(NetError::http_json(
                403,
                &json!({"error": "sharing_not_enabled"}),
            ))
        },
    ));
    let outbox = outbox_in(&dir, denied);
    outbox.enqueue("/events", json!({}), "evt");
    assert_eq!(outbox.flush(), 0);
    assert!(outbox.items().is_empty());
}

#[test]
fn test_runtime_opt_out_clears_queue_and_reenable_cannot_publish_it() {
    let dir = tempfile::tempdir().unwrap();
    let posted = Arc::new(Mutex::new(Vec::<String>::new()));
    let transport: SharedTransport = {
        let posted = Arc::clone(&posted);
        Arc::new(ClosureTransport(
            move |url: &str, _p: Option<&Value>, _h: &[(String, String)], _m: Option<&str>| {
                posted.lock().unwrap().push(url.to_string());
                Ok(json!({"ok": true}))
            },
        ))
    };
    let path = dir.path().join("outbox.json");
    let outbox = JournalOutbox::with(Some(identity()), true, &path, transport, wall_clock());
    outbox.insert_item(OutboxItem::new("/events", json!({}), "waiting"));
    outbox.set_enabled(false);
    assert!(!outbox.enqueue("/events", json!({}), "off"));
    assert!(outbox.items().is_empty());
    let on_disk: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(on_disk["items"], json!([]));
    outbox.set_enabled(true);
    outbox.flush();
    assert!(posted.lock().unwrap().is_empty());
}

#[test]
fn test_first_delivery_and_level_up_queue_only_proven_milestones() {
    let dir = tempfile::tempdir().unwrap();
    let outbox = outbox_in(&dir, ok_transport());
    // Profile(name="Road Star"), deliveries 1, xp 2_000 -> level 2 on the
    // career ladder.
    let career = CareerFacts {
        profile_name: "Road Star".to_string(),
        deliveries: 1,
        level: ff_core::models::career::level_for_xp(2_000.0),
    };
    assert!(career.level > 1);
    assert_eq!(queue_career_milestones(&outbox, &career, 1, 123), 2);
    let kinds: std::collections::BTreeSet<String> = outbox
        .items()
        .iter()
        .map(|item| item.payload["milestoneType"].as_str().unwrap().to_string())
        .collect();
    assert_eq!(
        kinds,
        ["first_delivery", "career_level"]
            .into_iter()
            .map(String::from)
            .collect()
    );
    assert_eq!(
        queue_career_milestones(&outbox, &career, career.level, 456),
        0
    );
}

#[test]
fn test_one_sender_at_a_time_even_when_several_events_ask_at_once() {
    // A settlement queues a delivery, a level up, and achievements together,
    // and each asks to flush. Sending them from a thread apiece posts the same
    // events twice and makes a driver's own writes collide on the server.
    let in_flight = Arc::new(Mutex::new(Vec::<String>::new()));
    let peak = Arc::new(Mutex::new(0usize));
    let posted = Arc::new(Mutex::new(Vec::<String>::new()));
    let release = Arc::new(Event::new());
    let transport: SharedTransport = {
        let in_flight = Arc::clone(&in_flight);
        let peak = Arc::clone(&peak);
        let posted = Arc::clone(&posted);
        let release = Arc::clone(&release);
        Arc::new(ClosureTransport(
            move |_u: &str, payload: Option<&Value>, _h: &[(String, String)], _m: Option<&str>| {
                let id = payload.unwrap()["eventId"].as_str().unwrap().to_string();
                {
                    let mut flight = in_flight.lock().unwrap();
                    flight.push(id.clone());
                    let mut peak = peak.lock().unwrap();
                    *peak = (*peak).max(flight.len());
                    posted.lock().unwrap().push(id.clone());
                }
                // Hold the first post open long enough that any second sender would
                // overlap it, so a regression shows up as overlap rather than timing.
                release.wait(Duration::from_secs(1));
                in_flight.lock().unwrap().retain(|v| v != &id);
                Ok(json!({"ok": true}))
            },
        ))
    };
    let dir = tempfile::tempdir().unwrap();
    let outbox = outbox_in(&dir, transport);
    for index in 0..4 {
        assert!(outbox.enqueue(
            "/events",
            json!({"eventId": format!("evt-{index}")}),
            &format!("evt-{index}")
        ));
    }
    for _ in 0..4 {
        outbox.flush_async();
    }
    release.set();

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if outbox.items().is_empty() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    assert!(
        outbox.items().is_empty(),
        "every queued event should have gone out"
    );
    assert_eq!(
        *peak.lock().unwrap(),
        1,
        "posts were in flight at once; senders raced"
    );
    let mut sent = posted.lock().unwrap().clone();
    sent.sort();
    assert_eq!(
        sent,
        vec!["evt-0", "evt-1", "evt-2", "evt-3"],
        "events were posted more than once"
    );
}
