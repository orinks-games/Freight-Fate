use super::*;

#[test]
fn test_download_verifies_the_content_hash() {
    let (dict, sig) = basic_profile();
    let good = make_cloud_reply(&dict, sig, 3);
    let payload = download(good.clone()).unwrap().expect("a payload");
    assert_eq!(payload["profile"]["name"], "Road Star");

    let mut tampered = good;
    tampered["contentHash"] = json!("0".repeat(64));
    assert!(download(tampered).unwrap().is_none());
}

#[test]
fn test_download_rejects_missing_or_future_verification_metadata() {
    let (dict, sig) = basic_profile();
    let mut unsigned = make_cloud_reply(&dict, sig, 3);
    unsigned.as_object_mut().unwrap().remove("sig");
    match download(unsigned) {
        Err(DownloadError::Integrity(e)) => assert_eq!(e.code, "unverified"),
        other => panic!("expected an unverified error, got {other:?}"),
    }

    let mut future = make_cloud_reply(&dict, sig, 3);
    future["validatorVersion"] = json!(2);
    match download(future) {
        Err(DownloadError::Integrity(e)) => assert_eq!(e.code, "update_required"),
        other => panic!("expected update_required, got {other:?}"),
    }
}

#[test]
fn test_download_rejects_payload_changed_after_server_signing() {
    let (dict, sig) = money77_profile();
    let mut reply = make_cloud_reply(&dict, sig, 3);
    let changed = json!({"name": "Road Star", "money": 88000.0, "version": 7});
    let (content, content_hash) = cloud_content(&changed);
    reply["content"] = json!(base64::engine::general_purpose::STANDARD.encode(&content));
    reply["contentHash"] = json!(content_hash);

    match download(reply) {
        Err(DownloadError::Integrity(e)) => assert_eq!(e.code, "integrity_failed"),
        other => panic!("expected integrity_failed, got {other:?}"),
    }
}

#[test]
fn test_restore_verifies_then_writes_a_locally_signed_save() {
    // The cloud copy came from another machine. Its portable payload has an
    // orinks.net signature; the writer hook then signs and installs it.
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let (dict, sig) = money77_profile();
    let reply = make_cloud_reply(&dict, sig, 4);
    let payload = download(reply).unwrap().expect("a payload");

    // An older local save exists and must survive as the fallback file.
    let local = json!({"name": "Road Star", "money": 5.0, "version": 7});
    let local_path = writer.write(&local).unwrap();

    let sync_state = SyncState::new(dir.path());
    let path = restore(&writer, &payload, Some(&sync_state)).unwrap();
    assert_eq!(path, local_path);

    let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(restored["money"], 77_000.0);
    let backup = path.with_extension("ffsave.bak");
    assert!(backup.exists());
    let old: Value = serde_json::from_str(&fs::read_to_string(&backup).unwrap()).unwrap();
    assert_eq!(old["money"], 5.0);

    // The restored revision is the next upload's parent, so continuing this
    // career does not immediately conflict with the copy just downloaded.
    assert_eq!(sync_state.slot("Road Star")["revision"], 4);
}

#[test]
fn test_unverified_restore_changes_neither_disk_nor_sync_state() {
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let local = json!({"name": "Road Star", "money": 5.0, "version": 7});
    let path = writer.write(&local).unwrap();
    let before = fs::read(&path).unwrap();
    let (dict, sig) = money77_profile();
    let mut payload = download(make_cloud_reply(&dict, sig, 3))
        .unwrap()
        .expect("a payload");
    payload["sig"] = json!(base64::engine::general_purpose::STANDARD.encode([b'x'; 64]));
    let sync_state = SyncState::new(dir.path());

    match restore(&writer, &payload, Some(&sync_state)) {
        Err(RestoreError::Integrity(e)) => {
            assert!(e.message.contains("signature"), "{}", e.message)
        }
        other => panic!("expected a signature failure, got {other:?}"),
    }

    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(sync_state.slots().is_empty());
    assert_eq!(writer.written.lock().unwrap().len(), 1);
}

#[test]
fn test_upload_rejects_oversized_content() {
    let mut huge = profile("Road Star", 5000.0);
    // Incompressible padding: repeated text would gzip under the cap.
    let mut noise = String::with_capacity(2 * 1024 * 1024);
    let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
    while noise.len() < 2 * 1024 * 1024 {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        noise.push_str(&format!("{x:016x}"));
    }
    huge["achievement_stats"] = json!({"pad": noise});
    let result = upload_save(
        &identity(),
        "Road Star",
        &huge,
        None,
        "too big",
        None,
        FakeTransport::revisions().as_ref(),
    );
    assert_eq!(result["ok"], false);
    assert_eq!(result["reason"], "too_large");
}

#[test]
fn test_upload_preserves_the_optional_evicted_save_name() {
    let transport = FakeTransport::replying(json!({
        "ok": true,
        "revision": 4,
        "evictedSaveName": "Old Road"
    }));
    let result = upload_save(
        &identity(),
        "Road Star",
        &profile("Road Star", 5000.0),
        Some(3),
        "Road Star",
        None,
        transport.as_ref(),
    );

    assert_eq!(result["ok"], true);
    assert_eq!(result["evictedSaveName"], "Old Road");
}

#[test]
fn test_upload_remains_compatible_when_the_server_omits_eviction() {
    let transport = FakeTransport::replying(json!({"ok": true, "revision": 4}));
    let result = upload_save(
        &identity(),
        "Road Star",
        &profile("Road Star", 5000.0),
        Some(3),
        "Road Star",
        None,
        transport.as_ref(),
    );

    assert_eq!(result["ok"], true);
    assert!(!result.contains_key("evictedSaveName"));
}

// -- cross-language canonicalization: see ff_core::cloud_save_integrity tests -----

#[test]
fn test_whole_float_profile_signature_round_trips() {
    // The exact shape that broke every real restore: ordinary careers carry
    // whole floats (total_miles, reputation), which the old canonical form
    // rendered as "29571.0" while the server had signed "29571". The 77000.0
    // here was signed as 77000 by the Python reference.
    let (dict, sig) = money77_profile();
    let payload = download(make_cloud_reply(&dict, sig, 3))
        .unwrap()
        .expect("a payload");
    assert_eq!(payload["profile"]["money"], 77_000.0);
}

#[test]
fn test_server_absolution_clears_the_modified_mark_on_restore() {
    // A career marked only because it moved computers gets released.
    //
    // The server grants this on a revision it signed and fully validated, so
    // honest machine-movers stop carrying the mark forever. A profile that
    // really was edited fails validation and never gets the signal.
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let (dict, sig) = marked_profile();
    let mut reply = make_cloud_reply(&dict, sig, 4);
    reply["clearIntegrityFlag"] = json!(true);
    let payload = download(reply).unwrap().expect("a payload");

    let sync_state = SyncState::new(dir.path());
    let path = restore(&writer, &payload, Some(&sync_state)).unwrap();
    let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();

    assert_eq!(restored["integrity_modified"], false);
    assert_eq!(restored["integrity_notice_pending"], false);
    assert_eq!(restored["money"], 77_000.0);
}

#[test]
fn test_a_restore_without_absolution_leaves_the_mark_alone() {
    // Absence of the signal is not permission to clear the mark.
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let (dict, sig) = marked_profile();
    let payload = download(make_cloud_reply(&dict, sig, 4))
        .unwrap()
        .expect("a payload");

    let path = restore(&writer, &payload, Some(&SyncState::new(dir.path()))).unwrap();
    let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(restored["integrity_modified"], true);
}

// -- the 1.9 cutover gate (careers from the 1.8 line do not restore here) --------

#[test]
fn test_newer_cloud_restore_refuses_without_touching_disk_or_sync_state() {
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let local = json!({"name": "Road Star", "money": 5.0, "version": 7});
    let path = writer.write(&local).unwrap();
    let before = fs::read(&path).unwrap();
    let (dict, sig) = money77_profile();
    let mut payload = download(make_cloud_reply(&dict, sig, 3)).unwrap().unwrap();
    payload["profile"]["created_line"] = json!("2.0");
    let cloud_before = payload.clone();
    let sync_state = SyncState::new(dir.path());
    sync_state.record_synced("Road Star", 2, "previous hash");
    let sync_before = fs::read(sync_state.path()).unwrap();
    let slots_before = sync_state.slots();
    let files_before = fs::read_dir(dir.path()).unwrap().count();
    assert_eq!(
        restore(&writer, &payload, Some(&sync_state)),
        Err(RestoreError::NewerCareer {
            name: "Road Star".to_string(),
            line: "2.0".to_string(),
        })
    );
    assert_eq!(payload, cloud_before);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(sync_state.path()).unwrap(), sync_before);
    assert_eq!(sync_state.slots(), slots_before);
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), files_before);
    assert_eq!(writer.written.lock().unwrap().len(), 1);
    assert!(!path.with_extension("ffsave.bak").exists());
    fs::remove_file(&path).unwrap();
    let empty_state = SyncState::new(&dir.path().join("empty"));
    assert!(matches!(
        restore(&writer, &payload, Some(&empty_state)),
        Err(RestoreError::NewerCareer { .. })
    ));
    assert!(!path.exists());
    assert!(!empty_state.path().exists());
    assert!(empty_state.slots().is_empty());
}

#[test]
fn test_newer_cloud_refusal_is_spoken_and_preserves_local_save() {
    let mut app = TestApp::new();
    let _identity = save_identity(&app);
    let transport = FakeTransport::revisions();
    install_cloud(&mut app, transport.clone(), true);
    let p = ff_core::models::profile::Profile::named("Future Driver");
    let path = p.save().unwrap();
    let mut dict = p.to_dict();
    dict.insert("created_line".into(), json!("2.0"));
    fs::write(&path, ff_core::models::profile::encode_save_bytes(&dict)).unwrap();
    let before = fs::read(&path).unwrap();
    let mut slot = CloudSlotState::new(&mut app.ctx, "Future Driver", vec![], None, None);
    for backup in [false, true] {
        app.clear_speech();
        if backup {
            slot.start_backup_now(&mut app.ctx);
        } else {
            slot.start_keep_mine(&mut app.ctx);
        }
        assert!(app
            .main_lines()
            .iter()
            .any(|line| line.contains("was made in Freight Fate 2.0")));
        assert_eq!(fs::read(&path).unwrap(), before);
        assert!(!slot.busy);
    }
    assert!(transport.posts().is_empty());
    slot.busy = true;
    slot.outcome.post("newer_refused:2.0".to_string());
    let slot = push(&mut app, slot);
    app.clear_speech();
    with_state::<CloudSlotState, _>(&slot, |s| State::update(s, &mut app.ctx, 0.0));
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.contains("This career was made in Freight Fate 2.0")));
    assert!(app
        .main_lines()
        .iter()
        .any(|line| line.contains("local save and the cloud backup are untouched")));
    assert_eq!(fs::read(&path).unwrap(), before);
}

#[test]
fn test_legacy_cloud_restore_refuses_without_touching_anything() {
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let local = json!({"name": "Road Star", "money": 5.0, "version": 7});
    let path = writer.write(&local).unwrap();
    let before = fs::read(&path).unwrap();
    let (dict, sig) = legacy_profile();
    let payload = download(make_cloud_reply(&dict, sig, 3))
        .unwrap()
        .expect("a payload");
    let sync_state = SyncState::new(dir.path());

    assert_eq!(
        restore(&writer, &payload, Some(&sync_state)),
        Err(RestoreError::LegacyCareer("Road Star".to_string()))
    );

    // Refused before anything was written: the local save is byte-for-byte
    // intact, no fallback file appeared, and the sync state never moved.
    // (The cloud copy is only ever read here; deleting is a separate,
    // confirmed menu action.)
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!path.with_extension("ffsave.bak").exists());
    assert!(sync_state.slots().is_empty());
    assert_eq!(writer.written.lock().unwrap().len(), 1);
}

#[test]
fn test_current_line_backup_without_marker_still_restores() {
    // A 1.9 tester's backup from before the created-on marker existed: the
    // save version vouches for it, exactly as the local load gate does.
    let dir = tempfile::tempdir().unwrap();
    let writer = DiskWriter::new(dir.path());
    let (dict, sig) = money77_profile();
    let payload = download(make_cloud_reply(&dict, sig, 3))
        .unwrap()
        .expect("a payload");
    let path = restore(&writer, &payload, Some(&SyncState::new(dir.path()))).unwrap();
    let restored: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(restored["money"], 77_000.0);
}

#[test]
fn test_legacy_snapshot_is_labeled_and_refused_before_the_confirm_step() {
    let mut app = TestApp::new();
    let _identity = save_identity(&app);
    let entry = json!({
        "saveName": "Old Timer",
        "revision": 3,
        "saveVersion": 5,
        "createdAt": 1_700_000_000_000i64,
        "summary": "Old Timer, level 50",
    });
    let slot = CloudSlotState::new(&mut app.ctx, "Old Timer", vec![entry], None, None);
    let slot = push(&mut app, slot);
    let restore_items: Vec<String> = menu_labels::<CloudSlotState>(&slot, &app.ctx)
        .into_iter()
        .filter(|t| t.starts_with("Restore"))
        .collect();
    assert!(restore_items
        .iter()
        .any(|t| t.contains("from an earlier version of Freight Fate")));

    move_to::<CloudSlotState>(&mut app, &slot, "Restore");
    press(&mut app, Key::Return);

    // No confirmation screen opened, nothing was downloaded or deleted; the
    // refusal is spoken kindly and the menu stays put.
    assert!(std::rc::Rc::ptr_eq(&app.state().unwrap(), &slot));
    let spoken = app.main_lines();
    assert!(spoken.iter().any(|t| t.contains(LEGACY_BACKUP_NOTICE)));
    assert!(spoken
        .iter()
        .any(|t| t.contains("stays safe in your orinks.net account")));
}
