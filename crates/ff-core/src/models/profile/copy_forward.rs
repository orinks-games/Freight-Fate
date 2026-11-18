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
    let paths: Vec<_> = entries.flatten().map(|e| e.path()).collect();
    for suffix in [LEGACY_SAVE_SUFFIX, SAVE_SUFFIX] {
        for path in &paths {
            if let Some(stem) = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_suffix(suffix))
            {
                candidates.insert(stem.to_string(), path.clone());
            }
        }
    }
    let mut count = 0;
    for (stem, path) in &candidates {
        if copied.contains(stem) {
            continue;
        }
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
            let modified = path.metadata()?.modified()?;
            let mut profile = match Profile::load_with_persistence(path, true, false) {
                Ok(profile) if profile.created_line == "1.9" && profile.name == name => profile,
                _ => continue,
            };
            profile.created_line = CREATED_LINE.to_string();
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

fn write_marker(path: &Path, copied: &BTreeSet<String>) -> io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(copied).map_err(io::Error::other)?)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::super::tests::with_data_dir;
    use super::*;
    use serde_json::json;

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
            let (path, mut data) = old_save("Migrant", true);
            data.remove("home_terminal_city");
            data.insert(SIGNATURE_FIELD.into(), json!(signature_for(&data, None)));
            std::fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
            let before = std::fs::read(&path).unwrap();
            assert_eq!(copy_forward_1_9_careers().unwrap(), 1);
            let loaded = Profile::load(&save_path_for("Migrant")).unwrap();
            assert!(!loaded.home_terminal_city.is_empty());
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
