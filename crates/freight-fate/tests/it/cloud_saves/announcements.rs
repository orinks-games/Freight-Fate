use super::*;

// -- background refusals speak everywhere (owner decision, 2026-08-15: automatic --
// -- saves upload silently, so a blind player never heard a career stop backing up)

#[test]
fn test_background_rejection_announces_exactly_once_across_retries() {
    let transport = FakeTransport::failing(rejected_error("impossible_money"));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    let lines = service.take_announcements();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("Road Star: backup not accepted."));
    assert!(lines[0].contains("flagged it for review"));
    // The raw reason code stays log-only, exactly as in the status line.
    assert!(!lines[0].contains("impossible_money"));

    // Every later save refused for the same cause stays silent.
    for i in 1..=3 {
        service.queue_backup("Road Star", profile("Road Star", 5000.0 + i as f64));
        drain(&service, &clock);
    }
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_background_refusal_speaks_again_when_the_cause_changes() {
    let transport = FakeTransport::failing(rejected_error("invalid_schema"));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert!(service.take_announcements()[0].contains("needs an update"));

    transport.set_error(Some(rejected_error("impossible_money")));
    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);

    let lines = service.take_announcements();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("flagged it for review"));
}

#[test]
fn test_a_background_backup_says_it_was_backed_up() {
    // The ordinary case, and the one that used to say nothing: a save at a
    // rest stop uploads on the background queue, and a driver who cannot see
    // the status line heard the same silence whether it reached the server or
    // never left the machine (owner, 2026-08-15).
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up."]
    );
    assert_eq!(backup_status("Road Star"), "Road Star is backed up.");
    assert_eq!(
        recovery_status("Road Star"),
        "Road Star is backed up again."
    );
}

#[test]
fn test_once_a_session_speaks_each_career_first_backup_only() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service_announcing(&transport, &clock, BackupAnnouncements::Once);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up."]
    );

    // Every later accepted upload of that career this session is silent.
    for i in 1..=3 {
        service.queue_backup("Road Star", profile("Road Star", 5000.0 + i as f64));
        drain(&service, &clock);
    }
    assert!(service.take_announcements().is_empty());

    // Another career still gets its first all-clear.
    service.queue_backup("Night Owl", profile("Night Owl", 6000.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Night Owl is backed up."]
    );
}

#[test]
fn test_all_clear_off_stays_silent_but_still_answers_a_spoken_refusal() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service_announcing(&transport, &clock, BackupAnnouncements::Off);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());

    // Refusals are never part of the quiet: the driver must hear a career
    // stop backing up, and then hear it recover.
    transport.set_error(Some(rejected_error("impossible_money")));
    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements().len(), 1);

    transport.set_error(None);
    service.queue_backup("Road Star", profile("Road Star", 5002.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up again."]
    );
}

#[test]
fn test_all_clear_cadence_can_change_mid_session() {
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock); // "every" by default

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements().len(), 1);

    service.set_backup_announcements(BackupAnnouncements::Off);
    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());

    service.set_backup_announcements(BackupAnnouncements::Every);
    service.queue_backup("Road Star", profile("Road Star", 5002.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up."]
    );
}

#[test]
fn test_backup_announcement_modes_read_from_the_setting() {
    assert_eq!(
        BackupAnnouncements::from_setting("every"),
        BackupAnnouncements::Every
    );
    assert_eq!(
        BackupAnnouncements::from_setting("once"),
        BackupAnnouncements::Once
    );
    assert_eq!(
        BackupAnnouncements::from_setting("off"),
        BackupAnnouncements::Off
    );
    // Anything else keeps the owner's 2026-08-15 default: say it every time.
    assert_eq!(
        BackupAnnouncements::from_setting("loud"),
        BackupAnnouncements::Every
    );
}

#[test]
fn test_a_background_backup_speaks_the_cloud_career_eviction() {
    let transport = FakeTransport::replying(json!({
        "ok": true,
        "revision": 1,
        "evictedSaveName": "Old Road"
    }));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    assert_eq!(
        service.take_announcements(),
        vec![
            "Road Star is backed up.",
            "Cloud backup removed Old Road, the least recently played cloud career. Your local career was not deleted."
        ]
    );
}

#[test]
fn test_an_untrusted_evicted_name_is_not_spoken() {
    let transport = FakeTransport::replying(json!({
        "ok": true,
        "revision": 1,
        "evictedSaveName": "Bad\nName"
    }));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up."]
    );
}

#[test]
fn test_a_manual_backup_carries_the_exact_eviction_outcome() {
    let transport = FakeTransport::replying(json!({
        "ok": true,
        "revision": 1,
        "evictedSaveName": "Old Road"
    }));
    let service = make_service(&transport, &ManualClock::new());

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();

    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("accepted:evicted:Old Road")
    );
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_a_manual_backup_stays_silent_so_the_menu_watch_can_speak() {
    // states/city.py waits on the result and says "Backed up to the cloud."
    // itself; a second line from the queue would say it twice for one save.
    let transport = FakeTransport::revisions();
    let service = make_service(&transport, &ManualClock::new());

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();
    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("accepted")
    );
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_nothing_to_send_stays_silent() {
    // The cloud already holds this exact save, so no upload ran. Claiming a
    // fresh backup would be untrue, and at a parked rest stop it would repeat
    // on every autosave.
    let transport = FakeTransport::revisions();
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up."]
    );

    service.queue_backup("Road Star", profile("Road Star", 5000.0)); // unchanged since the accepted revision
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_success_after_a_spoken_refusal_announces_recovery_and_rearms() {
    let transport = FakeTransport::revisions();
    transport.set_error(Some(rejected_error("impossible_money")));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements().len(), 1);

    transport.set_error(None);
    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up again."]
    );

    // A clean backup with nothing to recover from says the plain line rather
    // than the "again" one: nothing was wrong, so there is nothing to be over.
    service.queue_backup("Road Star", profile("Road Star", 5002.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Road Star is backed up."]
    );

    // ...and the dedupe is re-armed: the same refusal speaks once more.
    transport.set_error(Some(rejected_error("impossible_money")));
    service.queue_backup("Road Star", profile("Road Star", 5003.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements().len(), 1);
}

#[test]
fn test_background_auth_refusal_speaks_the_reconnect_line_once() {
    let transport = FakeTransport::failing(auth_error(401, None));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements(), vec![AUTH_PAUSED_STATUS]);

    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());
    assert!(AUTH_HELP.starts_with("orinks.net no longer accepts this computer's sign-in."));
}

#[test]
fn test_background_conflict_announces_the_choice_line_once() {
    let transport = FakeTransport::failing(conflict_error(Some(5)));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec![conflict_status("Road Star")]
    );

    // Later saves blocked by the same recorded conflict stay silent.
    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_background_network_trouble_stays_a_silent_retry() {
    // Transient failures retry on their own; the manual save path already
    // voices them on demand, so the global channel says nothing.
    let transport = FakeTransport::failing(NetError::other("OSError", "no route"));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_manual_refusal_never_reaches_the_global_channel() {
    // The terminal's Save game item polls and speaks its own outcome
    // (states/city.py); announcing it here too would say the refusal twice.
    let transport = FakeTransport::failing(rejected_error("invalid_achievement"));
    let service = make_service(&transport, &ManualClock::new());

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();

    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("rejected:invalid_achievement")
    );
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_manual_success_after_a_spoken_refusal_recovers_silently_and_rearms() {
    let transport = FakeTransport::revisions();
    transport.set_error(Some(rejected_error("impossible_money")));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements().len(), 1);

    // The player hits Save game and the upload goes through: the city
    // watch speaks "Backed up to the cloud." on its own, so the global
    // channel must not add a second line for the same event.
    transport.set_error(None);
    let token = service
        .backup_now("Road Star", profile("Road Star", 5001.0))
        .unwrap();
    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("accepted")
    );
    assert!(service.take_announcements().is_empty());

    // The dedupe still re-armed: the same refusal speaks once more.
    transport.set_error(Some(rejected_error("impossible_money")));
    service.queue_backup("Road Star", profile("Road Star", 5002.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements().len(), 1);
}

#[test]
fn test_manual_refusal_seeds_the_dedupe_for_the_background_channel() {
    // A conflict spoken by the manual Save game watch (states/city.py) is
    // already in the player's ear; the next automatic save blocked by the
    // same standing conflict must not repeat the identical sentence.
    let transport = FakeTransport::failing(conflict_error(Some(5)));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    let token = service
        .backup_now("Road Star", profile("Road Star", 5000.0))
        .unwrap();
    assert_eq!(
        service.outcome_for("Road Star", token).as_deref(),
        Some("conflict")
    );
    assert!(service.take_announcements().is_empty()); // the menu watch owns this one

    service.queue_backup("Road Star", profile("Road Star", 5001.0));
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());
}

#[test]
fn test_auth_announcement_is_one_per_outage_not_per_career() {
    let transport = FakeTransport::revisions();
    transport.set_error(Some(auth_error(401, None)));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);

    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements(), vec![AUTH_PAUSED_STATUS]);

    // The sign-in belongs to this computer, not to the career: loading
    // another career during the same outage must not repeat the line.
    service.queue_backup("Night Owl", profile("Night Owl", 5000.0));
    drain(&service, &clock);
    assert!(service.take_announcements().is_empty());

    // A successful upload proves the sign-in works again. It says the plain
    // backup line, not a recovery one: the refusal was announced against this
    // computer's sign-in rather than either career, so no slot has anything to
    // be over -- but the upload did happen, and every accepted background
    // backup says so.
    transport.set_error(None);
    service.queue_backup("Night Owl", profile("Night Owl", 5000.0));
    drain(&service, &clock);
    assert_eq!(
        service.take_announcements(),
        vec!["Night Owl is backed up."]
    );

    // ...and a fresh outage after that speaks afresh.
    transport.set_error(Some(auth_error(401, None)));
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    assert_eq!(service.take_announcements(), vec![AUTH_PAUSED_STATUS]);
}

#[test]
fn test_take_announcements_drains_the_queue() {
    let transport = FakeTransport::failing(rejected_error("impossible_money"));
    let clock = ManualClock::new();
    let service = make_service(&transport, &clock);
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);

    assert_eq!(service.take_announcements().len(), 1);
    assert!(service.take_announcements().is_empty());
}

/// The worker thread never speaks; the app's main loop drains
/// take_announcements and delivers on the normal announcement channel -- the
/// same polled pattern as the controller-disconnect notice.
#[test]
fn test_app_loop_speaks_queued_background_refusals() {
    let mut app = TestApp::new();
    let clock = ManualClock::new();
    let transport = FakeTransport::failing(rejected_error("impossible_money"));
    let service = make_service(&transport, &clock);
    app.ctx.services.cloud = service.clone();
    service.queue_backup("Road Star", profile("Road Star", 5000.0));
    drain(&service, &clock);
    app.clear_speech();

    app.run(Some(3)); // run() shuts the app down on its way out

    let refusals = app
        .main_lines()
        .into_iter()
        .filter(|t| t.starts_with("Road Star: backup not accepted."))
        .count();
    assert_eq!(refusals, 1);
}
