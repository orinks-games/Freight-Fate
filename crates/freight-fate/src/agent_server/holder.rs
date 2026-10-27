//! Who holds the one-game-at-a-time lock, when it is an agent server: a
//! small record beside the session log, so a new server refused the lock
//! can tell a live session from a leftover whose MCP client is gone.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct HolderRecord {
    pub pid: u32,
    pub parent_pid: Option<u32>,
    pub started: u64,
    pub exe: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum HolderVerdict {
    NoRecord,
    DeadRecord { pid: u32 },
    Live { pid: u32 },
    Stale { pid: u32, parent_pid: u32 },
}

pub fn judge(
    record: Option<HolderRecord>,
    alive: impl Fn(u32) -> bool,
    self_pid: u32,
) -> HolderVerdict {
    let Some(record) = record else {
        return HolderVerdict::NoRecord;
    };
    if record.pid == self_pid {
        HolderVerdict::NoRecord
    } else if !alive(record.pid) {
        HolderVerdict::DeadRecord { pid: record.pid }
    } else if let Some(parent_pid) = record.parent_pid.filter(|pid| !alive(*pid)) {
        HolderVerdict::Stale {
            pid: record.pid,
            parent_pid,
        }
    } else {
        HolderVerdict::Live { pid: record.pid }
    }
}

pub fn read(path: &Path) -> Option<HolderRecord> {
    std::fs::read(path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
}
pub fn write(path: &Path, record: &HolderRecord) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec(record).map_err(std::io::Error::other)?;
    std::fs::write(path, bytes)
}
pub fn remove_if_ours(path: &Path, pid: u32) {
    if read(path).is_some_and(|record| record.pid == pid) {
        let _ = std::fs::remove_file(path);
    }
}
pub fn remove(path: &Path) {
    let _ = std::fs::remove_file(path);
}
pub fn path() -> std::path::PathBuf {
    ff_core::settings::game_root()
        .join("logs")
        .join("agent-server.json")
}
pub fn current() -> HolderRecord {
    HolderRecord {
        pid: std::process::id(),
        parent_pid: super::lifeline::parent_pid(),
        started: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |time| time.as_secs()),
        exe: std::env::current_exe()
            .map_or_else(|_| String::new(), |path| path.display().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(pid: u32, parent_pid: Option<u32>) -> HolderRecord {
        HolderRecord {
            pid,
            parent_pid,
            started: 0,
            exe: String::new(),
        }
    }

    #[test]
    fn judge_distinguishes_record_states() {
        assert_eq!(judge(None, |_| true, 1), HolderVerdict::NoRecord);
        assert_eq!(
            judge(Some(record(1, None)), |_| true, 1),
            HolderVerdict::NoRecord
        );
        assert_eq!(
            judge(Some(record(2, None)), |_| false, 1),
            HolderVerdict::DeadRecord { pid: 2 }
        );
        assert_eq!(
            judge(Some(record(2, Some(3))), |_| true, 1),
            HolderVerdict::Live { pid: 2 }
        );
        assert_eq!(
            judge(Some(record(2, Some(3))), |pid| pid == 2, 1),
            HolderVerdict::Stale {
                pid: 2,
                parent_pid: 3
            }
        );
        assert_eq!(
            judge(Some(record(2, None)), |_| true, 1),
            HolderVerdict::Live { pid: 2 }
        );
    }

    #[test]
    fn record_round_trips() {
        let path = std::env::temp_dir().join(format!("freight-fate-holder-{}", std::process::id()));
        let expected = record(42, Some(7));
        write(&path, &expected).unwrap();
        assert_eq!(read(&path).unwrap().pid, 42);
        let _ = std::fs::remove_file(path);
    }
}
