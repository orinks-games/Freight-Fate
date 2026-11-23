//! Fresh revision checks before a manual backup can report matching copies.

use super::*;

pub(super) fn backup_conflict_outcome(
    slot: &Map<String, Value>,
    content_hash: &str,
    latest_revision: Option<i64>,
    token: i64,
) -> &'static str {
    if token != 0
        && slot.get("hash").and_then(Value::as_str) == Some(content_hash)
        && json_int(slot.get("revision"))
            .zip(latest_revision)
            .is_some_and(|(synced, latest)| latest > synced)
    {
        "stale"
    } else {
        "conflict"
    }
}

impl Inner {
    pub(super) fn finish_auth_backup(&self, name: &str, snapshot: &Arc<Value>, token: i64) {
        self.set_status(AUTH_PAUSED_STATUS);
        self.done_with(name, snapshot);
        self.note_outcome(name, token, "auth");
        self.announce_refusal(name, token, "auth", AUTH_PAUSED_STATUS);
    }

    pub(super) fn finish_cached_manual_backup(
        &self,
        name: &str,
        snapshot: &Arc<Value>,
        token: i64,
        content_hash: &str,
        slot: &mut Map<String, Value>,
    ) -> bool {
        let Some(identity) = self.identity.lock().unwrap().clone() else {
            self.finish_auth_backup(name, snapshot, token);
            return true;
        };
        let list = match list_saves(&identity, self.transport.as_ref()) {
            Ok(Some(list)) => list,
            Err(CloudAuthError) => {
                self.finish_auth_backup(name, snapshot, token);
                return true;
            }
            Ok(None) => {
                self.state.lock().unwrap().retry_at = Some((self.clock)() + self.retry);
                self.note_outcome(name, token, "network");
                return true;
            }
        };
        let latest = list
            .saves
            .iter()
            .filter(|entry| entry.get("saveName").and_then(Value::as_str) == Some(name))
            .max_by_key(|entry| json_int(entry.get("revision")));
        let Some(latest) = latest else {
            self.sync_state.forget(name);
            slot.clear();
            return false;
        };
        let Some(revision) = json_int(latest.get("revision")) else {
            self.state.lock().unwrap().retry_at = Some((self.clock)() + self.retry);
            self.note_outcome(name, token, "network");
            return true;
        };
        if Some(revision) == json_int(slot.get("revision")) {
            self.done_with(name, snapshot);
            self.note_outcome(name, token, "unchanged");
            self.announce_success(name, token, false);
        } else {
            let conflict = serde_json::json!({
                "latestRevision": revision,
                "latestCreatedAt": latest.get("createdAt"),
                "latestSummary": latest.get("summary"),
            });
            self.sync_state
                .record_conflict(name, conflict.as_object().unwrap());
            self.done_with(name, snapshot);
            let outcome = backup_conflict_outcome(slot, content_hash, Some(revision), token);
            self.note_outcome(name, token, outcome);
            if outcome == "stale" {
                self.set_slot_status(name, STALE_BACKUP_STATUS);
            } else {
                self.set_slot_status(name, &conflict_status(name));
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::testing::FakeTransport;
    use serde_json::json;

    #[test]
    fn only_unchanged_manual_copies_with_advanced_cloud_revisions_are_stale() {
        let slot = json!({"hash": "synced", "revision": 1});
        for (hash, latest, token, expected) in [
            ("synced", Some(2), 1, "stale"),
            ("synced", Some(2), 0, "conflict"),
            ("edited", Some(2), 1, "conflict"),
            ("synced", Some(1), 1, "conflict"),
            ("synced", Some(0), 1, "conflict"),
            ("synced", None, 1, "conflict"),
        ] {
            assert_eq!(
                backup_conflict_outcome(slot.as_object().unwrap(), hash, latest, token),
                expected
            );
        }
        assert_eq!(
            backup_conflict_outcome(&Map::new(), "synced", Some(2), 1),
            "conflict"
        );
    }

    #[test]
    fn identity_lost_after_dispatch_finishes_cached_and_uncached_manual_attempts() {
        for cached in [true, false] {
            let dir = tempfile::tempdir().unwrap();
            let transport = FakeTransport::new();
            let identity = OnlineIdentity::new("test-driver", &"t".repeat(48));
            let service = CloudSaves::new(CloudSavesOptions {
                enabled: true,
                identity: Some(identity.clone()),
                transport: transport.clone(),
                threaded: false,
                data_dir: dir.path().into(),
                ..CloudSavesOptions::default()
            });
            let snapshot = Arc::new(json!({"name": "Road Star", "money": 4500.0}));
            if cached {
                let (_, hash) = cloud_content(&snapshot);
                service.sync_state().record_synced_for_driver(
                    "Road Star",
                    1,
                    &hash,
                    &identity.driver_id,
                );
            }
            let pending = Pending {
                snapshot,
                meaningful_play: None,
                queued_at: 0.0,
                token: 1,
            };
            service
                .inner
                .state
                .lock()
                .unwrap()
                .pending
                .insert("Road Star".to_string(), pending.clone());
            *service.inner.identity.lock().unwrap() = None;

            service.inner.upload_slot("Road Star", pending);

            assert_eq!(service.outcome_for("Road Star", 1).as_deref(), Some("auth"));
            assert_eq!(service.status(), AUTH_PAUSED_STATUS);
            assert!(!service
                .inner
                .state
                .lock()
                .unwrap()
                .pending
                .contains_key("Road Star"));
            assert_eq!(transport.request_count(), 0);
        }
    }

    #[test]
    fn restoring_one_career_does_not_clear_another_careers_conflict_status() {
        let dir = tempfile::tempdir().unwrap();
        let service = CloudSaves::new(CloudSavesOptions {
            enabled: true,
            identity: Some(OnlineIdentity::new("test-driver", &"t".repeat(48))),
            transport: FakeTransport::new(),
            threaded: false,
            data_dir: dir.path().into(),
            ..CloudSavesOptions::default()
        });
        let other_status = conflict_status("Night Runs");
        service.inner.set_slot_status("Night Runs", &other_status);

        service.restore_succeeded("Road Star");
        assert_eq!(service.status(), other_status);

        service.restore_succeeded("Night Runs");
        assert_eq!(service.status(), "Cloud backup is ready.");
    }
}
