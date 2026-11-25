use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use super::*;

const COPIED: &str = "copied-from-1.9.json";

/// Import once per career without giving the old file to a mutating load.
pub fn copy_forward_1_9_careers() -> io::Result<usize> {
    let source = data_dir().join("profiles");
    let entries = match std::fs::read_dir(&source) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
        Err(e) => return Err(e),
    };
    let marker = career_data_dir().join(COPIED);
    let mut copied: BTreeSet<String> = match std::fs::read(&marker) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(io::Error::other)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => BTreeSet::new(),
        Err(e) => return Err(e),
    };
    // Never create or replace the 1.9 signing key while looking for imports.
    let key = match std::fs::read_to_string(signing::secret_path())
        .ok()
        .and_then(|s| hex::decode(s.trim()).ok())
    {
        Some(key) => key,
        None => return Ok(0),
    };
    let mut candidates = indexmap::IndexMap::new();
    for (career, path) in sealed::set_aside_candidates(&source) {
        if let Some(stem) = career.strip_suffix(SAVE_SUFFIX) {
            candidates.insert(stem.to_string(), (path, Some(career)));
        }
    }
    let paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    for suffix in [LEGACY_SAVE_SUFFIX, SAVE_SUFFIX] {
        for path in &paths {
            if let Some(stem) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(suffix))
            {
                candidates.insert(stem.to_string(), (path.clone(), None));
            }
        }
    }
    let mut count = 0;
    for (stem, (source_path, restored_name)) in &candidates {
        if copied.contains(stem) {
            continue;
        }
        let staging = if restored_name.is_some() {
            std::fs::create_dir_all(career_data_dir())?;
            Some(tempfile::tempdir_in(career_data_dir())?)
        } else {
            None
        };
        let staged_path = match (&staging, restored_name) {
            (Some(dir), Some(name)) => {
                let path = dir.path().join(name);
                if std::fs::copy(source_path, &path).is_err() {
                    continue;
                }
                path
            }
            _ => source_path.clone(),
        };
        let path = &staged_path;
        let Ok((raw, _)) = sealed::read(path) else {
            continue;
        };
        let Ok((data, _)) = decode_save_bytes(&raw) else {
            continue;
        };
        if data.get("created_line").and_then(Value::as_str) != Some("1.9") {
            continue;
        }
        let Some(Value::String(signature)) = data.get(SIGNATURE_FIELD) else {
            continue;
        };
        if signature != &signature_for_with_secret(&data, None, &key) {
            continue;
        }
        let Some(name) = data
            .get("name")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        let slot = signing::sanitized_stem(name);
        if copied.contains(&slot) {
            continue;
        }
        if find_save_path(name).is_none() {
            let modified = source_path.metadata()?.modified()?;
            let mut profile = match Profile::load_with_persistence(path, true, false) {
                Ok(profile) if profile.created_line == "1.9" && profile.name == name => profile,
                _ => continue,
            };
            profile.created_line = CREATED_LINE.to_string();
            profile.carry_over_from_1_9();
            let copied_path = profile.save()?;
            std::fs::OpenOptions::new()
                .write(true)
                .open(copied_path)?
                .set_modified(modified)?;
            origin::copy_from_1_9(&profile.name);
            count += 1;
        }
        copied.insert(slot);
        write_marker(&marker, &copied)?;
    }
    Ok(count)
}

/// Whether a 2.0 save with no `carryover` field is a 1.9 career an
/// earlier 2.0 build copied: its slot is in the copy record and its home
/// city is blank. The record alone is not enough (it also lists a 1.9 slot
/// that found a 2.0 career of the same name already there and was not
/// copied); a 2.0 start always writes a home city, so the pair picks out the
/// copies. Wrong only for a career started from a build that saved no home
/// city, under the same name as a 1.9 career.
pub(super) fn copied_by_an_earlier_build(name: &str) -> bool {
    let Ok(bytes) = std::fs::read(career_data_dir().join(COPIED)) else {
        return false;
    };
    serde_json::from_slice::<BTreeSet<String>>(&bytes)
        .is_ok_and(|copied| copied.contains(&signing::sanitized_stem(name)))
}

fn write_marker(path: &Path, copied: &BTreeSet<String>) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(copied).map_err(io::Error::other)?)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::super::tests::with_data_dir;
    use super::*;
    use crate::models::carryover::Carryover;
    use serde_json::json;

    /// A signed 1.9 save shaped by `edit`, copied forward, and loaded back.
    fn copied(name: &str, edit: impl FnOnce(&mut Map<String, Value>)) -> Profile {
        let (path, mut data) = old_save(name, false);
        data.remove("home_terminal_city");
        data.remove("home_city");
        data.remove("carryover");
        edit(&mut data);
        data.remove(SIGNATURE_FIELD);
        data.insert(SIGNATURE_FIELD.into(), json!(signature_for(&data, None)));
        std::fs::write(&path, encode_save_bytes(&data)).unwrap();
        assert_eq!(copy_forward_1_9_careers().unwrap(), 1);
        Profile::load(&save_path_for(name)).unwrap()
    }

    fn first_delivery(data: &mut Map<String, Value>, city: &str) {
        data.insert("achievement_stats".into(), json!({ "home_city": city }));
    }

    #[test]
    fn a_northstar_career_takes_its_home_from_the_first_delivery_and_its_truck_stays() {
        with_data_dir(|_| {
            let p = copied("North", |d| {
                d.insert("carrier_key".into(), json!("northstar"));
                d.insert("current_city".into(), json!("miami_fl_us"));
                first_delivery(d, "chicago_il_us");
            });
            assert_eq!(p.carryover, Carryover::Settled);
            assert_eq!(p.home_city, "chicago_il_us");
            assert_eq!(p.home_terminal_city, "chicago_il_us");
            assert_eq!(p.current_city, "miami_fl_us");
        });
    }

    #[test]
    fn a_prairie_link_career_far_from_its_lanes_owes_the_choice_and_borrows_no_terminal() {
        with_data_dir(|_| {
            let p = copied("Plains", |d| {
                d.insert("carrier_key".into(), json!("prairie_link"));
                d.insert("carrier_name".into(), json!("Prairie Link Regional"));
                d.insert("current_city".into(), json!("miami_fl_us"));
            });
            assert_eq!(p.carryover, Carryover::CarrierChoiceDue);
            assert!(p.carrier_choice_due());
            assert_eq!(p.home_city, "");
            assert_eq!(p.home_terminal_city, "miami_fl_us");
            assert_eq!(p.current_city, "miami_fl_us");
            assert!(!p.carrier_still_serves_home());
            assert!(p
                .carrier_home_terminal(crate::data::world::get_world())
                .is_none());
            // Reloading never swaps in the fallback carrier's terminal.
            let again = Profile::load(&save_path_for("Plains")).unwrap();
            assert_eq!(again.home_terminal_city, "miami_fl_us");
            assert!(!again.needs_migration_resave);
            let notice = again.carrier_choice_notice().unwrap();
            assert_eq!(
                notice,
                "Prairie Link Regional now runs freight only in the central plains around \
                 Kansas City, Omaha, and Wichita. It no longer dispatches from Miami. Choose a \
                 carrier to keep driving. Your money, level, trucks and driving record stay."
            );
        });
    }

    #[test]
    fn a_prairie_link_career_in_range_owes_the_choice_at_its_own_terminal() {
        with_data_dir(|_| {
            let p = copied("Topeka", |d| {
                d.insert("carrier_key".into(), json!("prairie_link"));
                d.insert("carrier_name".into(), json!("Prairie Link Regional"));
                d.insert("current_city".into(), json!("topeka_ks_us"));
            });
            assert_eq!(p.carryover, Carryover::CarrierChoiceDue);
            assert!(p.carrier_still_serves_home());
            assert!(crate::models::carriers::carrier("prairie_link")
                .unwrap()
                .terminal_city_keys
                .contains(&p.home_terminal_city));
            assert!(p
                .carrier_choice_notice()
                .unwrap()
                .contains("Stay with them or choose another carrier."));
        });
    }

    #[test]
    fn an_own_authority_career_is_settled() {
        with_data_dir(|_| {
            let p = copied("Indie", |d| {
                d.insert("business_status".into(), json!("independent_authority"));
                d.insert("carrier_key".into(), json!("prairie_link"));
                d.insert("current_city".into(), json!("miami_fl_us"));
            });
            assert_eq!(p.carryover, Carryover::Settled);
            assert!(!p.carrier_choice_due());
        });
    }

    #[test]
    fn a_career_an_earlier_build_copied_takes_the_same_path_once() {
        with_data_dir(|_| {
            // What an earlier 2.0 build wrote: a copy with no carryover field,
            // homed on the fallback carrier's terminal, its slot recorded.
            let mut p = Profile::named_in("Early", "miami_fl_us");
            p.carrier_key = "prairie_link".into();
            p.carrier_name = "Prairie Link Regional".into();
            p.home_city.clear();
            p.home_terminal_city = "chicago_il_us".into();
            let mut data = p.to_dict();
            data.remove("carryover");
            data.insert(SIGNATURE_FIELD.into(), json!(signature_for(&data, None)));
            let path = save_path_for("Early");
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            sealed::write(&path, &encode_save_bytes(&data)).unwrap();
            std::fs::write(career_data_dir().join(COPIED), br#"["Early"]"#).unwrap();
            let loaded = Profile::load(&path).unwrap();
            assert_eq!(loaded.carryover, Carryover::CarrierChoiceDue);
            assert_eq!(loaded.home_terminal_city, "miami_fl_us");
            // Settled once: a later load reads the saved field and leaves it.
            let mut settled = loaded.clone();
            settled.carryover = Carryover::Settled;
            settled.save().unwrap();
            assert_eq!(Profile::load(&path).unwrap().carryover, Carryover::Settled);

            // A 2.0 start under a recorded 1.9 name keeps its home and stays
            // native.
            let mut native = Profile::named_in("Native", "topeka_ks_us");
            native.carrier_key = "prairie_link".into();
            native.home_city = "topeka_ks_us".into();
            let mut data = native.to_dict();
            data.remove("carryover");
            data.insert(SIGNATURE_FIELD.into(), json!(signature_for(&data, None)));
            let path = save_path_for("Native");
            sealed::write(&path, &encode_save_bytes(&data)).unwrap();
            std::fs::write(career_data_dir().join(COPIED), br#"["Early","Native"]"#).unwrap();
            assert_eq!(Profile::load(&path).unwrap().carryover, Carryover::Native);
        });
    }

    fn old_save(name: &str, plain: bool) -> (std::path::PathBuf, Map<String, Value>) {
        let mut p = Profile::named(name);
        p.created_line = "1.9".into();
        p.spend(123.0);
        p.achievements = vec!["first_delivery".into()];
        let data = p.to_dict();
        let dir = data_dir().join("profiles");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!(
            "{name}{}",
            if plain {
                LEGACY_SAVE_SUFFIX
            } else {
                SAVE_SUFFIX
            }
        ));
        let bytes = if plain {
            serde_json::to_vec(&data).unwrap()
        } else {
            encode_save_bytes(&data)
        };
        std::fs::write(&path, bytes).unwrap();
        (path, data)
    }

    #[test]
    fn plain_and_packed_careers_copy_without_touching_the_original_or_its_bookkeeping() {
        with_data_dir(|root| {
            for plain in [false, true] {
                let name = if plain { "Plain" } else { "Packed" };
                let (old, data) = old_save(name, plain);
                let bytes = std::fs::read(&old).unwrap();
                let key = std::fs::read(signing::secret_path()).unwrap();
                let origins = root.join("integrity_origins.json");
                std::fs::write(&origins, b"old origins").unwrap();
                assert_eq!(copy_forward_1_9_careers().unwrap(), 1);
                let path = find_save_path(name).unwrap();
                assert_eq!(
                    path,
                    root.join("2.0")
                        .join("profiles")
                        .join(format!("{name}.ffsave"))
                );
                let mut loaded = Profile::load(&path).unwrap();
                assert_eq!(loaded.created_line, "2.0");
                assert_eq!(loaded.money(), data["money"].as_f64().unwrap());
                assert_eq!(loaded.achievements, ["first_delivery"]);
                assert!(!loaded.integrity_modified);
                loaded.spend(50.0);
                loaded.save().unwrap();
                let newer = std::fs::read(&path).unwrap();
                assert_eq!(copy_forward_1_9_careers().unwrap(), 0);
                assert_eq!(std::fs::read(&path).unwrap(), newer);
                loaded.delete();
                assert_eq!(copy_forward_1_9_careers().unwrap(), 0);
                assert!(find_save_path(name).is_none());
                assert_eq!(std::fs::read(&old).unwrap(), bytes);
                assert_eq!(std::fs::read(signing::secret_path()).unwrap(), key);
                assert_eq!(std::fs::read(&origins).unwrap(), b"old origins");
                assert_eq!(
                    std::fs::read_dir(old.parent().unwrap()).unwrap().count(),
                    if plain { 2 } else { 1 }
                );
            }
        });
    }

    #[test]
    fn copy_applies_normal_migrations_and_never_overwrites_an_existing_counterpart() {
        with_data_dir(|_| {
            // A 1.9 save holds neither a home city nor a home terminal, so
            // the copy homes at the carrier's terminal nearest the truck.
            let (path, mut data) = old_save("Migrant", true);
            data.remove("home_terminal_city");
            data.remove("home_city");
            data.insert("carrier_key".into(), json!("northstar"));
            data.insert("current_city".into(), json!("seattle_wa_us"));
            data.remove(SIGNATURE_FIELD);
            data.insert(SIGNATURE_FIELD.into(), json!(signature_for(&data, None)));
            std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
            let before = std::fs::read(&path).unwrap();
            assert_eq!(copy_forward_1_9_careers().unwrap(), 1);
            let loaded = Profile::load(&save_path_for("Migrant")).unwrap();
            assert_eq!(loaded.home_terminal_city, "portland_or_us");
            assert_eq!(loaded.current_city, "seattle_wa_us");
            // With no first-delivery record the truck's city only picks the
            // terminal: the home city stays blank.
            assert_eq!(loaded.home_city, "");
            assert_eq!(loaded.carryover, Carryover::Settled);
            assert!(!loaded.needs_migration_resave);
            assert_eq!(std::fs::read(&path).unwrap(), before);

            old_save("Existing", false);
            let current = Profile::named("Existing").save().unwrap();
            let before = std::fs::read(&current).unwrap();
            assert_eq!(copy_forward_1_9_careers().unwrap(), 0);
            assert_eq!(std::fs::read(&current).unwrap(), before);
            Profile::named("Existing").delete();
            assert_eq!(copy_forward_1_9_careers().unwrap(), 0);
        });
    }

    #[test]
    fn legacy_future_unsigned_unreadable_and_tampered_sources_are_left_alone() {
        with_data_dir(|_| {
            let mut originals = Vec::new();
            for name in [
                "Legacy",
                "Future",
                "Unsigned",
                "Unreadable",
                "Tampered",
                "Impossible",
            ] {
                let (path, mut data) = old_save(name, false);
                match name {
                    "Legacy" => {
                        data.remove("created_line");
                        data.insert("version".into(), json!(5));
                    }
                    "Future" => {
                        data.insert("created_line".into(), json!("2.1"));
                    }
                    "Tampered" => {
                        data.insert("money".into(), json!(1234));
                    }
                    "Impossible" => {
                        data.insert("money".into(), json!(1e15));
                    }
                    _ => {}
                }
                if name == "Unsigned" {
                    data.remove(SIGNATURE_FIELD);
                }
                if matches!(name, "Legacy" | "Future" | "Impossible") {
                    data.insert(SIGNATURE_FIELD.into(), json!(signature_for(&data, None)));
                }
                let bytes = if name == "Unreadable" {
                    b"broken".to_vec()
                } else {
                    encode_save_bytes(&data)
                };
                std::fs::write(&path, &bytes).unwrap();
                originals.push((path, bytes));
            }
            assert_eq!(copy_forward_1_9_careers().unwrap(), 0);
            assert!(Profile::list_saves().is_empty());
            for (path, bytes) in originals {
                assert_eq!(std::fs::read(path).unwrap(), bytes);
            }
        });
    }

    #[test]
    fn empty_start_and_unreadable_marker_do_not_import_anything() {
        with_data_dir(|_| {
            assert_eq!(copy_forward_1_9_careers().unwrap(), 0);
            let (old, _) = old_save("Driver", false);
            let before = std::fs::read(&old).unwrap();
            std::fs::write(career_data_dir().join(COPIED), b"broken").unwrap();
            assert!(copy_forward_1_9_careers().is_err());
            assert!(Profile::list_saves().is_empty());
            assert_eq!(std::fs::read(old).unwrap(), before);
        });
    }

    #[test]
    fn copied_origin_is_isolated_and_survives_the_release_line_change() {
        with_data_dir(|root| {
            old_save("Arrived", false);
            let old_record = root.join("integrity_origins.json");
            let bytes = br#"{"Arrived":"old-upload-hash"}"#;
            std::fs::write(&old_record, bytes).unwrap();
            assert_eq!(copy_forward_1_9_careers().unwrap(), 1);
            assert_eq!(
                origin::origin_for("Arrived").as_deref(),
                Some("old-upload-hash")
            );
            origin::forget_origin("Arrived");
            assert!(origin::origin_for("Arrived").is_none());
            assert_eq!(std::fs::read(old_record).unwrap(), bytes);
        });
    }

    #[test]
    fn imported_careers_keep_source_recency_for_continue() {
        with_data_dir(|_| {
            let (recent, _) = old_save("Recent", false);
            let (older, _) = old_save("Older", false);
            let recent_time = std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000);
            let older_time = recent_time - std::time::Duration::from_secs(3600);
            for (path, time) in [(&recent, recent_time), (&older, older_time)] {
                std::fs::OpenOptions::new()
                    .write(true)
                    .open(path)
                    .unwrap()
                    .set_modified(time)
                    .unwrap();
            }
            assert_eq!(copy_forward_1_9_careers().unwrap(), 2);
            let recent_copy = save_path_for("Recent");
            let older_copy = save_path_for("Older");
            assert_eq!(
                Profile::list_saves(),
                [recent_copy.clone(), older_copy.clone()]
            );
            for (source, copy) in [(&recent, &recent_copy), (&older, &older_copy)] {
                assert_eq!(
                    source.metadata().unwrap().modified().unwrap(),
                    copy.metadata().unwrap().modified().unwrap()
                );
                Profile::load(copy).unwrap();
            }
            assert_eq!(Profile::list_saves(), [recent_copy, older_copy]);
        });
    }
}
