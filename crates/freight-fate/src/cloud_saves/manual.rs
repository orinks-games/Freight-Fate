//! Fresh revision checks before a manual backup can report matching copies.

use super::*;

impl Inner {
    pub(super) fn finish_cached_manual_backup(
        &self,
        name: &str,
        snapshot: &Arc<Value>,
        token: i64,
        slot: &mut Map<String, Value>,
    ) -> bool {
        let Some(identity) = self.identity.lock().unwrap().clone() else {
            return true;
        };
        let list = match list_saves(&identity, self.transport.as_ref()) {
            Ok(Some(list)) => list,
            Err(CloudAuthError) => {
                self.set_status(AUTH_PAUSED_STATUS);
                self.done_with(name, snapshot);
                self.note_outcome(name, token, "auth");
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
            self.note_outcome(name, token, "conflict");
            self.set_status(&conflict_status(name));
        }
        true
    }
}
