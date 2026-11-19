//! `SyncState`: what this machine knows about each cloud slot, persisted
//! next to the saves. Re-exported from `crate::cloud_saves`.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde_json::{json, Map, Value};

/// What this machine knows about each cloud slot, persisted next to the
/// saves: the last revision it synced (uploaded or restored) and the content
/// hash at that point, so unchanged profiles skip the upload entirely.
///
/// A `conflict` entry means the server refused an upload because another
/// machine advanced the slot; it clears when the player resolves the slot
/// from the Cloud backup menu.
pub struct SyncState {
    path: PathBuf,
    inner: Mutex<SyncInner>,
}

#[derive(Default)]
struct SyncInner {
    slots: BTreeMap<String, Map<String, Value>>,
    loaded: bool,
}

impl SyncState {
    /// The sync state kept in `data_dir/cloud_saves.json`.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join("cloud_saves.json"),
            inner: Mutex::new(SyncInner::default()),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn ensure_loaded(&self, inner: &mut SyncInner) {
        if inner.loaded {
            return;
        }
        inner.loaded = true;
        let Ok(text) = fs::read_to_string(&self.path) else {
            return;
        };
        let Ok(data) = serde_json::from_str::<Value>(&text) else {
            return;
        };
        if let Some(Value::Object(slots)) = data.get("slots") {
            for (k, v) in slots {
                if let Value::Object(entry) = v {
                    inner.slots.insert(k.clone(), entry.clone());
                }
            }
        }
    }

    fn persist(&self, inner: &SyncInner) {
        let result = (|| -> std::io::Result<()> {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent)?;
            }
            let tmp = self.path.with_extension("json.tmp");
            let slots: Map<String, Value> = inner
                .slots
                .iter()
                .map(|(k, v)| (k.clone(), Value::Object(v.clone())))
                .collect();
            let text = serde_json::to_string_pretty(&json!({"slots": slots}))
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            fs::write(&tmp, text)?;
            fs::rename(&tmp, &self.path)
        })();
        if let Err(e) = result {
            log::debug!("Could not persist cloud sync state: {e}");
        }
    }

    /// What is known about one slot (`{}` when nothing).
    pub fn slot(&self, name: &str) -> Map<String, Value> {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        inner.slots.get(name).cloned().unwrap_or_default()
    }

    /// Every known slot.
    pub fn slots(&self) -> BTreeMap<String, Map<String, Value>> {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        inner.slots.clone()
    }

    pub fn record_synced(&self, name: &str, revision: i64, content_hash: &str) {
        self.record_synced_with_driver(name, revision, content_hash, None);
    }

    pub fn record_synced_for_driver(
        &self,
        name: &str,
        revision: i64,
        content_hash: &str,
        driver_id: &str,
    ) {
        self.record_synced_with_driver(name, revision, content_hash, Some(driver_id));
    }

    fn record_synced_with_driver(
        &self,
        name: &str,
        revision: i64,
        content_hash: &str,
        driver_id: Option<&str>,
    ) {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        let mut entry = Map::new();
        entry.insert("revision".to_string(), Value::from(revision));
        entry.insert("hash".to_string(), Value::from(content_hash));
        if let Some(driver_id) = driver_id {
            entry.insert("driverId".to_string(), Value::from(driver_id));
        }
        inner.slots.insert(name.to_string(), entry);
        self.persist(&inner);
    }

    pub fn record_conflict(&self, name: &str, latest: &Map<String, Value>) {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        let entry = inner.slots.entry(name.to_string()).or_default();
        let mut conflict = Map::new();
        for key in ["latestRevision", "latestCreatedAt", "latestSummary"] {
            conflict.insert(
                key.to_string(),
                latest.get(key).cloned().unwrap_or(Value::Null),
            );
        }
        entry.insert("conflict".to_string(), Value::Object(conflict));
        self.persist(&inner);
    }

    pub fn clear_conflict(&self, name: &str) {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        let removed = match inner.slots.get_mut(name) {
            Some(entry) => entry.remove("conflict").is_some(),
            None => false,
        };
        if removed {
            self.persist(&inner);
        }
    }

    /// Drop everything known about a slot, conflict included. Called after
    /// the cloud copy is deleted so the next local save starts a fresh slot
    /// instead of naming a revision that no longer exists.
    pub fn forget(&self, name: &str) {
        let mut inner = self.inner.lock().unwrap();
        self.ensure_loaded(&mut inner);
        if inner.slots.remove(name).is_some() {
            self.persist(&inner);
        }
    }
}

/// The `conflict` entry of a slot, when one is recorded.
pub(crate) fn slot_conflict(slot: &Map<String, Value>) -> Option<Map<String, Value>> {
    match slot.get("conflict") {
        Some(Value::Object(conflict)) => Some(conflict.clone()),
        _ => None,
    }
}

pub(crate) fn json_int(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => n.as_i64(),
        _ => None,
    }
}
