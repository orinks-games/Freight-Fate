//! Profile persistence, migration and enumeration.
use super::*;
use indexmap::IndexMap;

impl Profile {
    // -- persistence -----------------------------------------------------------

    /// `profile.path`: where this profile saves.
    pub fn path(&self) -> PathBuf {
        save_path_for(&self.name)
    }

    /// Write the signed, packed save atomically; returns its path.
    pub fn save(&self) -> std::io::Result<PathBuf> {
        let path = self.path();
        if !self.money_guard.matches(self.money) {
            origin::forget_origin(&self.name);
        }
        sealed::write(&path, &encode_save_bytes(&self.to_dict()))?;
        // A converted legacy save keeps one plain-JSON copy as .json.bak so an
        // older game version can be rolled back to; the live file is packed.
        let legacy = path.with_extension("json");
        if legacy.exists() {
            let _ = std::fs::rename(&legacy, legacy.with_extension("json.bak"));
        }
        notify_save_listener(self);
        Ok(path)
    }

    /// `Profile.load(path)` for the running build.
    pub fn load(path: &Path) -> Result<Profile, LoadError> {
        Self::load_with(path, is_frozen())
    }

    /// `Profile.load(path)` with `is_frozen()` pinned (the skip-signing flag
    /// is honored only from source).
    pub fn load_with(path: &Path, frozen: bool) -> Result<Profile, LoadError> {
        Self::load_with_persistence(path, frozen, true)
    }

    pub(super) fn load_with_persistence(
        path: &Path,
        frozen: bool,
        persist: bool,
    ) -> Result<Profile, LoadError> {
        let (raw, needs_sealing) = sealed::read(path)?;
        let (data, packed) = match decode_save_bytes(&raw) {
            Ok(decoded) => decoded,
            Err(err) => {
                // Unreadable beyond repair: move it aside so the picker's spoken
                // warning stays truthful and the file is not re-tried every visit.
                if persist {
                    let _ = signing::quarantine(path);
                }
                return Err(err.into());
            }
        };
        if let Some(line) = newer_career_line(&data) {
            return Err(NewerCareerError {
                name: data
                    .get("name")
                    .filter(|v| py_truthy(v))
                    .map(py_str)
                    .unwrap_or_else(|| "Driver".to_string()),
                path: Some(path.to_path_buf()),
                line: line.to_string(),
            }
            .into());
        }
        if is_pre_1_9_save(&data) {
            // A career from the 1.8 line or earlier. 1.9 starts everyone fresh
            // (the rebalanced arc cannot absorb old-scale careers), so refuse
            // before any signature check, migration, or resave can touch the
            // file: it stays intact on disk, still playable by the build that
            // wrote it.
            let name = match data.get("name") {
                Some(v) if py_truthy(v) => py_str(v),
                _ => "Driver".to_string(),
            };
            return Err(LegacyCareerError {
                name,
                path: Some(path.to_path_buf()),
            }
            .into());
        }
        let signed = data.contains_key(SIGNATURE_FIELD);
        let skip = signing_checks_disabled_when(frozen);
        let mut resign = false;
        let mut tampered = false;
        if signed && !is_signature_valid(&data) {
            if skip {
                // Re-save below so the file gets a valid local signature and
                // keeps loading once the dev flag is off again.
                log::warn!(
                    "Signature check skipped for {} (FREIGHT_FATE_SKIP_SAVE_SIGNING)",
                    path.display()
                );
                resign = true;
            } else {
                tampered = true;
            }
        } else if !signed && !skip {
            // Every build of the 1.9 line signs what it writes, and a save
            // from before the line was refused above, so a save that reaches
            // here unsigned had its signature taken off by hand -- packed or
            // plain. Plain unsigned JSON used to keep an amnesty as the shape
            // of saves from before signing; none of those load any more, and
            // the amnesty had become the easy way to edit a career: write the
            // numbers as JSON and the game signed them for you.
            tampered = true;
        }
        let mut profile = Profile::from_dict(&data);
        let money_impossible =
            plausibility::money_is_impossible(profile.money, profile.career.total_earnings);
        if !skip && money_impossible {
            // Signed or not: a balance rewritten while the game ran is signed
            // by the game itself, so the file looks honest and the number
            // does not.
            log::warn!(
                "{}: balance {} is more than {} of lifetime earnings could hold",
                path.display(),
                profile.money,
                profile.career.total_earnings
            );
            tampered = true;
        }
        if !persist && (tampered || resign || !signed) {
            return Err(ProfileIntegrityError(
                "The source career's integrity could not be verified.".into(),
            )
            .into());
        }
        if persist && (tampered || resign) {
            // Only a copy from another computer (a good file whose signature
            // this data directory cannot check) records where it came from:
            // the raw dict, before migration or marking, is what the other
            // computer backed up. Any other reason forgets an earlier record.
            let copied = signed
                && !skip
                && !money_impossible
                && !data.get("integrity_modified").is_some_and(py_truthy);
            if copied {
                let (_, hash) = origin::cloud_content(&Value::Object(data.clone()));
                origin::record_origin(&profile.name, &hash);
            } else {
                origin::forget_origin(&profile.name);
            }
        }
        // A 1.9 career an earlier 2.0 build copied, before the carry-over
        // existed: the same home fix and choice, once (the resave writes the
        // field).
        if persist
            && !data.contains_key("carryover")
            && profile.created_line == CREATED_LINE
            && profile.home_city.trim().is_empty()
            && super::copy_forward::copied_by_an_earlier_build(&profile.name)
        {
            profile.carry_over_from_1_9();
            profile.needs_migration_resave = true;
        }
        if tampered && !profile.integrity_modified {
            profile.integrity_modified = true;
            profile.integrity_notice_pending = true;
        }
        if persist
            && (profile.needs_migration_resave
                || resign
                || !signed
                || tampered
                || !packed
                || needs_sealing)
        {
            profile.save()?;
        }
        Ok(profile)
    }

    /// Every save on disk, newest first; a packed save shadows a leftover
    /// legacy twin of the same career.
    pub fn list_saves() -> Vec<PathBuf> {
        let dir = profiles_dir();
        // Before the listing: a career an older release set aside is a career.
        sealed::restore_set_aside(&dir);
        let mut found: IndexMap<String, PathBuf> = IndexMap::new();
        let entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| rd.flatten().map(|e| e.path()).collect())
            .unwrap_or_default();
        for suffix in [LEGACY_SAVE_SUFFIX, SAVE_SUFFIX] {
            for path in &entries {
                let name = path.file_name().map(|n| n.to_string_lossy().to_string());
                let Some(name) = name else { continue };
                if let Some(stem) = name.strip_suffix(suffix) {
                    found.insert(stem.to_string(), path.clone());
                }
            }
        }
        let mut saves: Vec<PathBuf> = found.into_values().collect();
        saves.sort_by_key(|p| {
            std::cmp::Reverse(
                p.metadata()
                    .and_then(|m| m.modified())
                    .unwrap_or(std::time::SystemTime::UNIX_EPOCH),
            )
        });
        saves
    }

    /// Remove this profile's save files (packed and legacy).
    pub fn delete(&self) {
        let path = self.path();
        if sealed::remove(&path).is_ok() && sealed::remove(&path.with_extension("json")).is_ok() {
            origin::forget_origin(&self.name);
        }
    }
}
