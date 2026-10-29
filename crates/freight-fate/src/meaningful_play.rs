//! Pending intent that an accepted cloud snapshot may become the public career.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_json::Value;

static OPERATION_SEQUENCE: AtomicU64 = AtomicU64::new(1);
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);
const FILE_VERSION: u32 = 1;

/// Hash only career progress that may make a save meaningfully publishable.
///
/// Dispatch offers and one-time announcement acknowledgements are serialized
/// so menus remain stable and quiet, but browsing them is not play and must
/// not promote a public career. The underlying business, equipment, trip,
/// market, credential, driving-record, and achievement state remains included.
pub fn meaningful_profile_hash(profile_dict: &Value) -> String {
    let mut projected = profile_dict.clone();
    if let Value::Object(map) = &mut projected {
        for field in [
            "dispatch_board_cache",
            "migration_notice_pending",
            "integrity_modified",
            "integrity_notice_pending",
            "hos_key_notice_left",
        ] {
            map.remove(field);
        }
        if let Some(Value::Object(career)) = map.get_mut("career") {
            career.remove("unacknowledged_grants");
        }
        if let Some(Value::Object(record)) = map.get_mut("driving_record") {
            for field in [
                "trust_band_heard",
                "debt_rung_heard",
                "setback_notice_kind",
                "setback_notice_lines",
                "notice_pending",
            ] {
                record.remove(field);
            }
        }
    }
    crate::cloud_saves::cloud_content(&projected).1
}

/// The closed set of gameplay events the server accepts as meaningful play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MeaningfulPlayReason {
    JobAccepted,
    DriveStarted,
    DeliveryCompleted,
    EquipmentChanged,
    BusinessChanged,
    ChangedSave,
}

/// The retry-stable metadata attached to one pending meaningful upload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeaningfulPlayStamp {
    pub operation_id: String,
    pub occurred_at: i64,
    pub reason: MeaningfulPlayReason,
}

/// Thread-safe pending meaningful-play intents, one newest intent per save.
#[derive(Debug, Clone)]
pub struct MeaningfulPlayTracker {
    inner: Arc<TrackerInner>,
}

#[derive(Debug)]
struct TrackerInner {
    path: Option<PathBuf>,
    state: Mutex<TrackerState>,
}

#[derive(Debug, Default)]
struct TrackerState {
    pending: HashMap<String, MeaningfulPlayStamp>,
    loaded: bool,
    writes_enabled: bool,
}

#[derive(Debug, Serialize, Deserialize)]
struct MeaningfulPlayFile {
    version: u32,
    pending: HashMap<String, MeaningfulPlayStamp>,
}

impl Default for MeaningfulPlayTracker {
    fn default() -> Self {
        Self {
            inner: Arc::new(TrackerInner {
                path: None,
                state: Mutex::new(TrackerState {
                    loaded: true,
                    writes_enabled: true,
                    ..TrackerState::default()
                }),
            }),
        }
    }
}

impl MeaningfulPlayTracker {
    /// Load and persist pending intent beside the cloud sync ledger.
    pub fn new(data_dir: &Path) -> Self {
        Self {
            inner: Arc::new(TrackerInner {
                path: Some(data_dir.join("meaningful_play.json")),
                state: Mutex::new(TrackerState::default()),
            }),
        }
    }

    /// Replace this save's pending intent with a newly identified event.
    pub fn mark(&self, save_name: &str, reason: MeaningfulPlayReason) {
        let occurred_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_millis() as i64)
            .unwrap_or(0);
        let sequence = OPERATION_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let operation_id = format!(
            "{occurred_at:016x}-{:08x}-{sequence:016x}",
            std::process::id()
        );
        let mut state = self.inner.state.lock().unwrap();
        self.ensure_loaded(&mut state);
        if !state.writes_enabled {
            return;
        }
        let mut candidate = state.pending.clone();
        candidate.insert(
            save_name.to_string(),
            MeaningfulPlayStamp {
                operation_id,
                occurred_at,
                reason,
            },
        );
        match self.persist(&candidate) {
            Ok(()) => state.pending = candidate,
            Err(error) => log::warn!("Could not persist meaningful-play intent: {error}"),
        }
    }

    /// Read the pending stamp without consuming it so retries reuse it.
    pub fn for_upload(&self, save_name: &str) -> Option<MeaningfulPlayStamp> {
        let mut state = self.inner.state.lock().unwrap();
        self.ensure_loaded(&mut state);
        state.pending.get(save_name).cloned()
    }

    /// Clear only the exact operation the server accepted.
    pub fn clear_if_accepted(&self, save_name: &str, operation_id: &str) {
        let mut state = self.inner.state.lock().unwrap();
        self.ensure_loaded(&mut state);
        if !state.writes_enabled
            || !state
                .pending
                .get(save_name)
                .is_some_and(|stamp| stamp.operation_id == operation_id)
        {
            return;
        }
        let mut candidate = state.pending.clone();
        candidate.remove(save_name);
        match self.persist(&candidate) {
            Ok(()) => state.pending = candidate,
            Err(error) => log::warn!("Could not clear accepted meaningful-play intent: {error}"),
        }
    }

    fn ensure_loaded(&self, state: &mut TrackerState) {
        if state.loaded {
            return;
        }
        state.loaded = true;
        let Some(path) = &self.inner.path else {
            state.writes_enabled = true;
            return;
        };
        match fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<MeaningfulPlayFile>(&bytes) {
                Ok(file) if file.version == FILE_VERSION => {
                    state.pending = file.pending;
                    state.writes_enabled = true;
                }
                Ok(file) => log::warn!(
                    "Preserving unsupported meaningful-play ledger version {} at {}",
                    file.version,
                    path.display()
                ),
                Err(error) => log::warn!(
                    "Preserving unreadable meaningful-play ledger at {}: {error}",
                    path.display()
                ),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                state.writes_enabled = true;
            }
            Err(error) => log::warn!(
                "Preserving unreadable meaningful-play ledger at {}: {error}",
                path.display()
            ),
        }
    }

    fn persist(&self, pending: &HashMap<String, MeaningfulPlayStamp>) -> io::Result<()> {
        let Some(path) = &self.inner.path else {
            return Ok(());
        };
        let temp = fresh_temp_path(path);
        let result = (|| -> io::Result<()> {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)?;
            }
            let document = MeaningfulPlayFile {
                version: FILE_VERSION,
                pending: pending.clone(),
            };
            let text = serde_json::to_string_pretty(&document)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            let mut file = options.open(&temp)?;
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            drop(file);
            fs::rename(&temp, path)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
}

/// A same-directory, create-new temp name so the final rename is atomic and
/// concurrent game instances never share a staging file.
fn fresh_temp_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let nonce = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    path.with_file_name(format!(
        ".{name}.{}-{stamp}-{nonce}.tmp",
        std::process::id()
    ))
}
