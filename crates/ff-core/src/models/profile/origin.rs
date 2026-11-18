//! Where a marked career came from, when it arrived as a copy.
//!
//! A save copied to another computer fails its signature there (the key is
//! per data directory) and is marked. When that was the ONLY reason, the load
//! records the cloud-content hash of the file exactly as it arrived, so the
//! cloud backup can name the backup it is a copy of. Anything that could be
//! an edit afterwards forgets it. The record lives in
//! `career_data_dir()/integrity_origins.json` as `{"<career name>": "<hash>"}`.

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use super::paths::data_dir_if_allowed;
use super::signing::py_json_dumps_compact;
use super::{SIGNATURE_FIELD, SIGNATURE_VERSION_FIELD};

/// The profile's integrity-signature fields. Stripped from cloud content:
/// the signature only verifies on the machine that wrote it.
pub const SIGNATURE_FIELDS: [&str; 2] = [SIGNATURE_FIELD, SIGNATURE_VERSION_FIELD];

const ORIGINS_FILE: &str = "integrity_origins.json";

// Read-modify-write of the sidecar from the loop and the cloud worker.
static LOCK: Mutex<()> = Mutex::new(());

/// The upload form of a profile snapshot: signature-stripped JSON,
/// gzipped deterministically, plus its sha256 hex digest.
pub fn cloud_content(profile_dict: &Value) -> (Vec<u8>, String) {
    let portable = match profile_dict {
        Value::Object(map) => Value::Object(
            map.iter()
                .filter(|(k, _)| !SIGNATURE_FIELDS.contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        ),
        other => other.clone(),
    };
    let mut raw = String::new();
    py_json_dumps_compact(&portable, &mut raw);
    let mut encoder = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), flate2::Compression::best());
    let _ = encoder.write_all(raw.as_bytes());
    let content = encoder.finish().unwrap_or_default();
    let digest = hex::encode(Sha256::digest(&content));
    (content, digest)
}

fn origins_path() -> Option<PathBuf> {
    data_dir_if_allowed().map(|dir| {
        let dir = dir.join(super::CREATED_LINE);
        let _ = std::fs::create_dir_all(&dir);
        dir.join(ORIGINS_FILE)
    })
}

fn read_origins(path: &PathBuf) -> Map<String, Value> {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|v| match v {
            Value::Object(map) => Some(map),
            _ => None,
        })
        .unwrap_or_default()
}

fn edit_origins(edit: impl FnOnce(&mut Map<String, Value>) -> bool) {
    let Some(path) = origins_path() else { return };
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut origins = read_origins(&path);
    if !edit(&mut origins) {
        return;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string(&Value::Object(origins)).unwrap_or_default();
    if let Err(e) = std::fs::write(&tmp, text).and_then(|()| std::fs::rename(&tmp, &path)) {
        log::warn!("Could not update {}: {e}", path.display());
    }
}

/// Remember that career `name` arrived as the cloud content hashing to `hash`.
pub fn record_origin(name: &str, hash: &str) {
    edit_origins(|origins| {
        origins.insert(name.to_string(), Value::from(hash));
        true
    });
}

/// The recorded arrival hash of career `name`, if any.
pub fn origin_for(name: &str) -> Option<String> {
    let path = origins_path()?;
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    read_origins(&path)
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string)
}

pub(super) fn copy_from_1_9(name: &str) {
    let path = super::data_dir().join(ORIGINS_FILE);
    let hash = read_origins(&path)
        .get(name)
        .and_then(Value::as_str)
        .map(str::to_string);
    if let Some(hash) = hash {
        record_origin(name, &hash);
    }
}

/// Drop career `name`'s record; a no-op when it has none.
pub fn forget_origin(name: &str) {
    edit_origins(|origins| origins.remove(name).is_some());
}
