use std::collections::HashSet;
use std::time::{SystemTime, UNIX_EPOCH};

use ff_core::cloud_resume::{decide, CheckState, Decision, Event};
use ff_core::models::profile::{is_pre_1_9_save, Profile};

use super::*;
use crate::states::cloud_save_states::{created_at_ms, install_restored_profile};

#[derive(Default)]
pub(super) struct Resume {
    state: CheckState,
    lines: Vec<String>,
}

impl CloudSaves {
    pub fn configure_resume(&self, enabled: bool) {
        let mut resume = self.inner.resume.lock().unwrap();
        resume.state = resume.state.next(Event::Configure {
            enabled: enabled && self.enabled(),
        });
    }

    pub fn resume_pending(&self) -> bool {
        matches!(
            self.resume_state(),
            CheckState::Checking | CheckState::Cancelling
        )
    }

    pub fn resume_state(&self) -> CheckState {
        self.inner.resume.lock().unwrap().state
    }

    pub fn take_resume_lines(&self) -> Vec<String> {
        std::mem::take(&mut self.inner.resume.lock().unwrap().lines)
    }

    /// Called once per main-menu entry. All disk and network work stays on the worker.
    pub fn check_resume(&self, enabled: bool) {
        {
            let mut resume = self.inner.resume.lock().unwrap();
            if matches!(resume.state, CheckState::Checking | CheckState::Cancelling) {
                return;
            }
            resume.state = resume.state.next(Event::Configure {
                enabled: enabled && self.enabled(),
            });
            resume.state = resume.state.next(Event::Enter);
            if resume.state != CheckState::Checking {
                return;
            }
        }
        let service = self.clone();
        let data_dir = ff_core::models::profile::data_dir();
        if self.inner.threaded {
            if let Err(error) =
                thread::Builder::new()
                    .name("cloud-resume".into())
                    .spawn(move || {
                        ff_core::settings::paths::set_thread_data_dir(Some(data_dir));
                        service.run_resume();
                    })
            {
                log::warn!("Cloud resume could not start: {error}");
                let mut resume = self.inner.resume.lock().unwrap();
                resume.state = resume.state.next(Event::Failed);
            }
        } else {
            service.run_resume();
        }
    }

    fn run_resume(&self) {
        let result = self.resume_careers();
        let mut resume = self.inner.resume.lock().unwrap();
        match result {
            Ok((pulled, conflicts)) => {
                resume.state = resume.state.next(Event::Completed { pulled, conflicts });
            }
            Err(error) => {
                log::warn!("Cloud resume check failed: {error}");
                resume.state = resume.state.next(Event::Failed);
            }
        }
    }

    fn resume_careers(&self) -> Result<(usize, usize), String> {
        let identity = self.identity().ok_or("not connected")?;
        let list = list_saves(&identity, self.transport().as_ref())
            .map_err(|error| error.to_string())?
            .ok_or("site unreachable")?;
        let mut seen = HashSet::new();
        let latest: HashMap<_, _> = list
            .saves
            .iter()
            .filter_map(|entry| {
                let name = entry.get("saveName")?.as_str()?;
                seen.insert(name).then_some((name, entry))
            })
            .collect();
        let mut pulled = 0;
        let mut conflicts = 0;
        let mut careers: Vec<_> = Profile::list_saves()
            .into_iter()
            .filter_map(|path| match Profile::load(&path) {
                Ok(profile) => Some((path, profile)),
                Err(error) => {
                    log::debug!("Cloud resume skipped {}: {error}", path.display());
                    None
                }
            })
            .collect();
        careers.sort_by_key(|(_, profile)| {
            latest
                .get(cloud_slot_name(&profile.name).as_str())
                .and_then(|entry| json_int(entry.get("createdAt")))
                .unwrap_or(0)
        });
        for (path, profile) in careers {
            if !self.enabled() || self.resume_state() != CheckState::Checking {
                break;
            }
            let result = (|| -> Result<(), String> {
                let name = cloud_slot_name(&profile.name);
                if path != profile.path() {
                    log::debug!("Cloud resume skipped renamed local file {}", path.display());
                    return Ok(());
                }
                let Some(entry) = latest.get(name.as_str()) else {
                    return Ok(());
                };
                let remote = json_int(entry.get("revision"));
                let decision_for = |profile: &Profile| {
                    let slot = self.sync_state().slot(&name);
                    let (_, hash) = cloud_content(&Value::Object(profile.to_dict()));
                    decide(
                        (slot.get("driverId").and_then(Value::as_str)
                            == Some(identity.driver_id.as_str()))
                        .then(|| json_int(slot.get("revision")))
                        .flatten(),
                        slot.get("hash").and_then(Value::as_str) != Some(hash.as_str()),
                        remote,
                    )
                };
                let mut decision = decision_for(&profile);
                if let Decision::PullCloud { revision } = decision {
                    let payload = download_save(
                        &identity,
                        &name,
                        Some(revision),
                        self.transport().as_ref(),
                        self.inner.public_keys.as_ref(),
                    )
                    .map_err(|error| error.to_string())?
                    .ok_or("backup unavailable")?;
                    if payload.get("saveName").and_then(Value::as_str) != Some(name.as_str())
                        || json_int(payload.get("revision")) != Some(revision)
                        || payload
                            .get("profile")
                            .and_then(|p| p.get("name"))
                            .and_then(Value::as_str)
                            != Some(profile.name.as_str())
                    {
                        return Err("backup does not match requested career".into());
                    }
                    let _transfer = self.inner.transfer.write().unwrap();
                    let account = self.inner.identity.lock().unwrap();
                    if account.as_ref().map(|id| &id.driver_id) != Some(&identity.driver_id) {
                        return Ok(());
                    }
                    let current = Profile::load(&path).map_err(|error| error.to_string())?;
                    decision = decision_for(&current);
                    if matches!(decision, Decision::PullCloud { .. })
                        && self.enabled()
                        && self.resume_state() == CheckState::Checking
                    {
                        let is_legacy =
                            |value: &Value| value.as_object().is_none_or(is_pre_1_9_save);
                        let installed_path = restore_to_disk(
                            &payload,
                            Some(self.sync_state()),
                            &RestoreHooks {
                                is_legacy: &is_legacy,
                                write: &install_restored_profile,
                            },
                            self.inner.public_keys.as_ref(),
                        )
                        .map_err(|error| error.to_string())?;
                        let installed =
                            Profile::load(&installed_path).map_err(|error| error.to_string())?;
                        let (_, hash) = cloud_content(&Value::Object(installed.to_dict()));
                        self.sync_state().record_synced_for_driver(
                            &name,
                            revision,
                            &hash,
                            &identity.driver_id,
                        );
                        if let Some(modified) = created_at_ms(entry)
                            .and_then(|ms| Duration::try_from_secs_f64(ms / 1000.0).ok())
                            .and_then(|elapsed| UNIX_EPOCH.checked_add(elapsed))
                            .filter(|time| *time <= SystemTime::now())
                        {
                            if let Ok(file) =
                                std::fs::File::options().write(true).open(&installed_path)
                            {
                                let _ = file.set_modified(modified);
                            }
                        }
                        self.inner.state.lock().unwrap().pending.remove(&name);
                        pulled += 1;
                        self.inner
                            .resume
                            .lock()
                            .unwrap()
                            .lines
                            .push(resume_status(&profile.name));
                    }
                }
                if decision == Decision::Conflict {
                    let _transfer = self.inner.transfer.write().unwrap();
                    let account = self.inner.identity.lock().unwrap();
                    if account.as_ref().map(|id| &id.driver_id) != Some(&identity.driver_id) {
                        return Ok(());
                    }
                    let current = Profile::load(&path).map_err(|error| error.to_string())?;
                    if decision_for(&current) != Decision::Conflict
                        || self.resume_state() != CheckState::Checking
                    {
                        return Ok(());
                    }
                    let mut latest = Map::new();
                    for (target, source) in [
                        ("latestRevision", "revision"),
                        ("latestCreatedAt", "createdAt"),
                        ("latestSummary", "summary"),
                    ] {
                        latest.insert(
                            target.into(),
                            entry.get(source).cloned().unwrap_or(Value::Null),
                        );
                    }
                    self.sync_state().record_conflict(&name, &latest);
                    conflicts += 1;
                    self.inner
                        .resume
                        .lock()
                        .unwrap()
                        .lines
                        .push(resume_conflict_status(&profile.name));
                }
                Ok(())
            })();
            if let Err(error) = result {
                log::warn!("Cloud resume skipped {}: {error}", profile.name);
            }
        }
        Ok((pulled, conflicts))
    }
}
