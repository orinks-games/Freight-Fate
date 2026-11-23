use super::*;

impl Inner {
    pub(super) fn enabled(&self) -> bool {
        self.enabled.load(Ordering::SeqCst)
    }

    /// Drop conflicts for careers this computer no longer has.
    ///
    /// A conflict is a choice between this machine's save and the cloud's.
    /// With no local save there is no "keep mine" to offer, so the record
    /// cannot be resolved by anything the player does -- and the two heals
    /// in [`Inner::upload_slot`] never reach it, because both run on the way
    /// to an upload and a career with no save on disk never queues one. The
    /// Online hub went on saying "<career> is waiting for you to choose
    /// which copy to keep" for a career that was gone from the cloud AND
    /// from the computer, with no way to clear it (Shane, 2026-09-20).
    ///
    /// Forgetting rather than hiding matters: a stale row left in
    /// `cloud_saves.json` would attach itself to the next career started
    /// under the same name, and block ITS backups against a revision that
    /// was never its own.
    pub(super) fn forget_conflicts_with_no_local_save(&self) {
        // Strip the known suffixes rather than taking file_stem(): a career
        // named "Run 1.9" would otherwise read back as "Run 1" and its live
        // conflict would be forgotten as stale.
        let on_disk: std::collections::BTreeSet<String> = Profile::list_saves()
            .iter()
            .filter_map(|path| path.file_name().map(|n| n.to_string_lossy().to_string()))
            .filter_map(|name| {
                name.strip_suffix(SAVE_SUFFIX)
                    .or_else(|| name.strip_suffix(LEGACY_SAVE_SUFFIX))
                    .map(str::to_string)
            })
            .collect();
        for (name, entry) in self.sync_state.slots() {
            if slot_conflict(&entry).is_none() || on_disk.contains(&name) {
                continue;
            }
            log::info!(
                "Cloud sync state for {name}: a conflict was waiting for a career this \
computer no longer has; forgetting it so the slot starts clean"
            );
            self.sync_state.forget(&name);
        }
    }

    /// One line per known slot at startup: the kept session logs only go
    /// back two sessions, so a stall whose conflict was recorded earlier
    /// would otherwise leave no trace in the log a tester shares.
    pub(super) fn log_sync_state(&self) {
        let slots = self.sync_state.slots();
        if slots.is_empty() {
            log::info!("Cloud sync state: no careers have synced from this computer yet");
            return;
        }
        for (name, entry) in slots {
            let synced = match json_int(entry.get("revision")) {
                Some(revision) => format!("last synced revision {revision}"),
                None => "no revision synced yet".to_string(),
            };
            match slot_conflict(&entry) {
                None => log::info!("Cloud sync state for {name}: {synced}"),
                Some(conflict) => log::info!(
                    "Cloud sync state for {name}: {synced}; a conflict against cloud \
revision {} is waiting in the Cloud backup menu",
                    conflict
                        .get("latestRevision")
                        .map(py_str)
                        .unwrap_or_else(|| "None".to_string())
                ),
            }
        }
    }

    pub(super) fn stop_worker(&self) {
        self.stop.set();
        self.wake.set();
        let handle = self.thread.lock().unwrap().take();
        if let Some(handle) = handle {
            join_with_timeout(handle, Duration::from_secs(2));
        }
        self.started.store(false, Ordering::SeqCst);
        self.stop.clear();
    }

    pub(super) fn run(&self) {
        while !self.stop.is_set() {
            self.pump(false);
            wait_seconds(&self.wake, self.worker_wait());
            self.wake.clear();
        }
    }

    pub(super) fn worker_wait(&self) -> f64 {
        let now = (self.clock)();
        let st = self.state.lock().unwrap();
        if st.pending.is_empty() {
            return WORKER_TICK_S;
        }
        let oldest = st
            .pending
            .values()
            .map(|p| p.queued_at)
            .fold(f64::INFINITY, f64::min);
        if let Some(retry_at) = st.retry_at {
            return (retry_at - now).max(0.05);
        }
        (self.debounce - (now - oldest)).max(0.05)
    }

    pub(super) fn pump(&self, force: bool) {
        if !self.enabled() || self.identity.lock().unwrap().is_none() {
            return;
        }
        let now = (self.clock)();
        let due: Vec<(String, Pending)> = {
            let st = self.state.lock().unwrap();
            if !force && st.retry_at.is_some_and(|at| now < at) {
                return;
            }
            st.pending
                .iter()
                .filter(|(_, p)| force || now - p.queued_at >= self.debounce)
                .map(|(name, p)| (name.clone(), p.clone()))
                .collect()
        };
        for (name, pending) in due {
            if self.stop.is_set() && !force {
                return;
            }
            self.upload_slot(&name, pending);
        }
    }

    /// Drop a handled snapshot -- unless a newer save replaced it while
    /// the upload was in flight, which must stay queued.
    pub(super) fn done_with(&self, name: &str, snapshot: &Arc<Value>) {
        let mut st = self.state.lock().unwrap();
        if let Some(current) = st.pending.get(name) {
            if Arc::ptr_eq(&current.snapshot, snapshot) {
                st.pending.remove(name);
            }
        }
    }

    /// Record a terminal upload result under the attempt token its
    /// snapshot was queued with (0 for background saves, which no poller
    /// ever matches). Uploads run outside the lock, so an upload already in
    /// flight when a newer manual attempt starts finishes carrying its own
    /// older token: it must neither answer for the newer attempt nor
    /// overwrite the newer attempt's recorded result.
    pub(super) fn note_outcome(&self, name: &str, token: i64, outcome: &str) {
        let mut st = self.state.lock().unwrap();
        let keep = match st.outcomes.get(name) {
            Some((current, _)) => token >= *current,
            None => true,
        };
        if keep {
            st.outcomes
                .insert(name.to_string(), (token, outcome.to_string()));
        }
    }

    /// Record a terminal upload refusal, and queue its spoken line when
    /// the background queue owes one.
    ///
    /// The token gates the speaking, never the bookkeeping: every terminal
    /// refusal records its cause, manual or background, so whichever
    /// channel told the player first -- this one, or the Save game watch
    /// (states/city.py) that owns token > 0 attempts -- the player hears
    /// each standing cause exactly once. Only a background attempt
    /// (token 0) appends the line; a manual refusal is the menu's to
    /// speak. A recorded cause stays silent until it changes or the slot
    /// uploads successfully (see announce_recovery). The auth cause
    /// dedupes machine-wide under AUTH_ANNOUNCED_KEY: a paused sign-in
    /// belongs to this computer, not to whichever career saved first.
    /// Transient network failures never arrive here -- they retry
    /// silently.
    pub(super) fn announce_refusal(&self, name: &str, token: i64, cause: &str, message: &str) {
        let key = if cause == "auth" {
            AUTH_ANNOUNCED_KEY
        } else {
            name
        };
        let mut st = self.state.lock().unwrap();
        if st.announced_causes.get(key).map(String::as_str) == Some(cause) {
            return;
        }
        st.announced_causes
            .insert(key.to_string(), cause.to_string());
        if token == 0 {
            st.announcements.push(message.to_string());
        }
    }

    /// An upload was accepted: say so, and clear any recorded refusal so
    /// later trouble speaks afresh.
    ///
    /// Success used to be silent unless it followed an announced refusal,
    /// which left the ordinary case with nothing to hear at all. A save at a
    /// rest stop backs up in the background, and a driver who cannot see the
    /// status line got the same nothing whether the career reached the server
    /// or never left the machine -- silence reading as failure is the whole
    /// complaint behind the refusal lines above, and it applied just as much
    /// to the path that worked (owner, 2026-08-15).
    ///
    /// Same principle as announce_refusal: the token gates the speaking,
    /// never the bookkeeping. Only a background save (token 0) speaks here --
    /// a manual save's outcome is already spoken by the Save game watch
    /// (states/city.py `_backup_outcome_text`), and a second line for the
    /// same event would say it twice.
    ///
    /// `uploaded` marks a real accepted upload, which also proves this
    /// computer's sign-in works, so it re-arms the machine-wide auth
    /// announcement -- silently, since the auth line named no career and
    /// reconnecting speaks its own confirmation. The "unchanged" path never
    /// contacts the server: it leaves the auth record alone, and it stays
    /// silent unless it is clearing a refusal, because nothing was sent and
    /// claiming a fresh backup would be untrue.
    pub(super) fn announce_success(&self, name: &str, token: i64, uploaded: bool) {
        let mut st = self.state.lock().unwrap();
        if uploaded {
            st.announced_causes.remove(AUTH_ANNOUNCED_KEY);
        }
        let recovered = st.announced_causes.remove(name).is_some();
        if token != 0 {
            return;
        }
        if recovered {
            // Answers a refusal the driver heard: never thinned by the
            // cadence setting.
            st.announcements.push(recovery_status(name));
        } else if uploaded {
            let first_this_session = st.all_clear_spoken.insert(name.to_string());
            let speak = match st.backup_announcements {
                BackupAnnouncements::Every => true,
                BackupAnnouncements::Once => first_this_session,
                BackupAnnouncements::Off => false,
            };
            if speak {
                st.announcements.push(backup_status(name));
            }
        }
    }

    pub(super) fn announce_eviction(&self, outcome: &str, token: i64) {
        if token != 0 {
            return;
        }
        let Some(name) = outcome.strip_prefix("accepted:evicted:") else {
            return;
        };
        self.state
            .lock()
            .unwrap()
            .announcements
            .push(eviction_status(name));
    }

    pub(super) fn note_absolved(&self, name: &str, result: &Map<String, Value>) {
        if truthy(result.get("clearIntegrityFlag")) {
            self.state.lock().unwrap().absolved.push(name.to_string());
        }
    }

    pub(super) fn set_status(&self, message: &str) {
        let mut state = self.state.lock().unwrap();
        state.status = message.to_string();
        state.status_slot = None;
    }

    pub(super) fn set_slot_status(&self, name: &str, message: &str) {
        let mut state = self.state.lock().unwrap();
        state.status = message.to_string();
        state.status_slot = Some(name.to_string());
    }

    pub(super) fn clear_slot_status(&self, name: &str) {
        let mut state = self.state.lock().unwrap();
        if state.status_slot.as_deref() == Some(name) {
            state.status = "Cloud backup is ready.".to_string();
            state.status_slot = None;
        }
    }

    pub(super) fn upload_slot(&self, name: &str, pending: Pending) {
        let _transfer = self.transfer.read().unwrap();
        if !self
            .state
            .lock()
            .unwrap()
            .pending
            .get(name)
            .is_some_and(|current| Arc::ptr_eq(&current.snapshot, &pending.snapshot))
        {
            return;
        }
        let Pending {
            snapshot,
            meaningful_play,
            token,
            ..
        } = pending;
        let mut slot = self.sync_state.slot(name);
        let (_, content_hash) = cloud_content(&snapshot);
        match slot_conflict(&slot) {
            Some(conflict) if json_int(conflict.get("latestRevision")).is_none() => {
                // Recorded by an older build against an empty cloud slot (wiped
                // deployment, or deleted from another machine). No newer save
                // exists to protect, so start the slot over instead of staying
                // silent forever.
                self.sync_state.forget(name);
                slot = Map::new();
            }
            Some(conflict) => {
                // A known conflict names a real cloud revision -- but that copy
                // may have vanished since it was recorded (deployment reset, or
                // the slot deleted from another machine), and then there is
                // nothing left to protect. Re-check before staying silent.
                if self.cloud_slot_exists(name) {
                    // Still there: the player resolves it from the Cloud backup
                    // menu. Drop the snapshot -- the local file is still the
                    // source of truth for "keep mine".
                    self.done_with(name, &snapshot);
                    let outcome = manual::backup_conflict_outcome(
                        &slot,
                        &content_hash,
                        json_int(conflict.get("latestRevision")),
                        token,
                    );
                    self.note_outcome(name, token, outcome);
                    let message = if outcome == "stale" {
                        STALE_BACKUP_STATUS.to_string()
                    } else {
                        conflict_status(name)
                    };
                    self.set_slot_status(name, &message);
                    self.announce_refusal(name, token, outcome, &message);
                    return;
                }
                log::info!(
                    "Cloud backup of {name} was blocked by a conflict whose cloud \
copy no longer exists; restarting the slot fresh"
                );
                self.sync_state.forget(name);
                slot = Map::new();
            }
            None => {}
        }
        if slot.get("hash").and_then(Value::as_str) == Some(content_hash.as_str()) {
            if token != 0 {
                if self.finish_cached_manual_backup(
                    name,
                    &snapshot,
                    token,
                    &content_hash,
                    &mut slot,
                ) {
                    return;
                }
            } else {
                self.done_with(name, &snapshot);
                self.note_outcome(name, token, "unchanged");
                self.announce_success(name, token, false);
                return;
            }
        }
        let identity = match self.identity.lock().unwrap().clone() {
            Some(id) => id,
            None => {
                self.finish_auth_backup(name, &snapshot, token);
                return;
            }
        };
        let result = upload_save(
            &identity,
            name,
            &snapshot,
            json_int(slot.get("revision")),
            &backup_summary(&snapshot),
            meaningful_play.as_ref(),
            self.transport.as_ref(),
        );
        if truthy(result.get("ok")) {
            let revision = json_int(result.get("revision")).unwrap_or(0);
            self.sync_state.record_synced_for_driver(
                name,
                revision,
                result
                    .get("contentHash")
                    .and_then(Value::as_str)
                    .unwrap_or(""),
                &identity.driver_id,
            );
            self.done_with(name, &snapshot);
            self.note_absolved(name, &result);
            if let Some(stamp) = &meaningful_play {
                self.meaningful_play
                    .clear_if_accepted(name, &stamp.operation_id);
            }
            self.state.lock().unwrap().retry_at = None;
            self.set_status("Latest backup accepted and server-verified.");
            let outcome = accepted_outcome(&result);
            self.note_outcome(name, token, &outcome);
            self.announce_success(name, token, true);
            self.announce_eviction(&outcome, token);
            log::info!("Cloud backup of {name} uploaded as revision {revision}");
            return;
        }
        if reason_of(&result) == Some("conflict") {
            if json_int(result.get("latestRevision")).is_none() {
                // The cloud slot is empty -- the staging deployment was wiped,
                // or the slot was deleted from another machine -- so there is
                // no newer save to protect. Drop the stale revision and let the
                // retry pass re-create the slot from this machine's save.
                self.sync_state.forget(name);
                self.state.lock().unwrap().retry_at = Some((self.clock)() + self.retry);
                self.note_outcome(name, token, "network");
                log::info!(
                    "Cloud backup of {name} named a revision the cloud no longer \
has; restarting the slot fresh"
                );
                return;
            }
            self.sync_state.record_conflict(name, &result);
            self.done_with(name, &snapshot);
            self.note_outcome(name, token, "conflict");
            self.set_slot_status(name, &conflict_status(name));
            self.announce_refusal(name, token, "conflict", &conflict_status(name));
            log::warn!(
                "Cloud backup of {name} skipped: the cloud copy is newer (revision {})",
                result
                    .get("latestRevision")
                    .map(py_str)
                    .unwrap_or_else(|| "None".to_string())
            );
            return;
        }
        let reason = reason_of(&result).map(str::to_string);
        let family = classify_upload_failure(reason.as_deref());
        if family == "auth" {
            // The credentials were retired (usually by connecting another
            // computer); every retry would fail identically, and the player
            // can only fix it by reconnecting.
            self.set_status(AUTH_PAUSED_STATUS);
            self.done_with(name, &snapshot);
            self.note_outcome(name, token, "auth");
            self.announce_refusal(name, token, "auth", AUTH_PAUSED_STATUS);
            return;
        }
        if family == "rejected" {
            // Not transient: retrying with the same inputs cannot succeed.
            // The raw reason code is logged for review but never spoken --
            // only the honest, career-named story below is.
            let reason_text = reason.clone().unwrap_or_default();
            log::warn!("Cloud backup of {name} was rejected: {reason_text}");
            let status = rejection_status(name, reason.as_deref());
            self.set_slot_status(name, &status);
            self.done_with(name, &snapshot);
            let cause = format!("rejected:{reason_text}");
            self.note_outcome(name, token, &cause);
            self.announce_refusal(name, token, &cause, &status);
            return;
        }
        // Transient (network, 5xx): keep the snapshot, back off.
        self.state.lock().unwrap().retry_at = Some((self.clock)() + self.retry);
        self.note_outcome(name, token, "network");
    }

    /// Whether the cloud still holds any revision of this slot. Errs on
    /// the side of `true`: an unreachable or refusing server must keep the
    /// conflict guard in place.
    pub(super) fn cloud_slot_exists(&self, name: &str) -> bool {
        let identity = match self.identity.lock().unwrap().clone() {
            Some(id) => id,
            None => return true,
        };
        match list_saves(&identity, self.transport.as_ref()) {
            Err(CloudAuthError) => true,
            Ok(None) => true,
            Ok(Some(list)) => list
                .saves
                .iter()
                .any(|entry| entry.get("saveName").and_then(Value::as_str) == Some(name)),
        }
    }
}
