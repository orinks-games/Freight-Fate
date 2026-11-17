//! Guest drivers: going online with no orinks.net account.
//!
//! orinks.net makes the guest (`POST /api/freight-fate/guest`) and names it
//! from word lists, like "Copper Fox 12", so a player never types a name the
//! board would have to screen. The guest gets an ordinary driver identity
//! (`OnlineIdentity`, saved the usual way), so the drivers board and Profile
//! sharing work unchanged. A guest is on the board and nowhere else: profiles
//! need an account, so the road journal, achievements and cloud backup are
//! refused by the server and never sent from here (`profile_identity`).
//!
//! Which identity is a guest is remembered here, in `online-guest.json` beside
//! `online.json`, with the name the server gave it. The record counts only
//! while its driver id matches the saved identity, so connecting an account
//! (which saves a different identity, or the same id now owned by the account)
//! and then dropping the record is all it takes to stop being a guest.
//!
//! Connecting an account sends the guest's own token with the activation
//! request (`online_activation::start_activation_as`). An account with no
//! driver yet takes the guest over, name and all; one that has a driver
//! replaces it, and orinks.net deletes the guest.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use crate::net::{NetError, Transport};
use crate::online_activation::machine_key;
use crate::online_presence::{base_url, OnlineIdentity};

/// The file beside `online.json` that marks the identity as a guest.
pub const GUEST_FILE: &str = "online-guest.json";

/// A guest driver this computer made: its id and the name orinks.net gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestRecord {
    pub driver_id: String,
    pub display_name: String,
}

fn guest_path(data_dir: &Path) -> PathBuf {
    data_dir.join(GUEST_FILE)
}

/// The guest record on disk, whether or not it still matches the identity.
pub fn load_guest(data_dir: &Path) -> Option<GuestRecord> {
    let text = fs::read_to_string(guest_path(data_dir)).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let driver_id = value.get("driver_id")?.as_str()?.trim().to_string();
    let display_name = value.get("display_name")?.as_str()?.trim().to_string();
    if driver_id.is_empty() || display_name.is_empty() {
        return None;
    }
    Some(GuestRecord {
        driver_id,
        display_name,
    })
}

pub fn save_guest(data_dir: &Path, record: &GuestRecord) -> io::Result<()> {
    fs::create_dir_all(data_dir)?;
    let text = serde_json::to_string_pretty(&json!({
        "driver_id": record.driver_id,
        "display_name": record.display_name,
    }))
    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(guest_path(data_dir), text)
}

pub fn clear_guest(data_dir: &Path) {
    match fs::remove_file(guest_path(data_dir)) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => log::warn!("Could not remove the guest driver record: {e}"),
    }
}

/// The guest `identity` is signed in as, or `None` for an account driver
/// (or no driver at all).
pub fn guest_for(identity: Option<&OnlineIdentity>, data_dir: &Path) -> Option<GuestRecord> {
    let identity = identity?;
    load_guest(data_dir).filter(|record| record.driver_id == identity.driver_id)
}

/// The identity the road journal and achievements may post as: `identity`,
/// unless it is a guest's. Profiles need an account, so a guest has none.
pub fn profile_identity(
    identity: Option<OnlineIdentity>,
    data_dir: &Path,
) -> Option<OnlineIdentity> {
    identity.filter(|identity| guest_for(Some(identity), data_dir).is_none())
}

/// What asking for a guest came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestCreate {
    /// A new guest: the identity to save and the name to say.
    Ready {
        identity: OnlineIdentity,
        display_name: String,
    },
    /// Too many new guests from this address just now.
    RateLimited,
    /// Anything else: the network, the site, a reply that made no sense.
    Failed,
}

fn reply_text(reply: &Value, key: &str) -> Option<String> {
    reply
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// Ask orinks.net for a new guest driver for this computer.
///
/// Runs on a worker thread; never panics and never logs the token.
pub fn create_guest(transport: &dyn Transport, data_dir: &Path) -> GuestCreate {
    let reply = match transport.call(
        &format!("{}/api/freight-fate/guest", base_url()),
        Some(&json!({"machine_key": machine_key(data_dir)})),
        &[],
        None,
    ) {
        Ok(reply) => reply,
        Err(NetError::Http { code: 429, .. }) => return GuestCreate::RateLimited,
        Err(e) => {
            log::warn!("Guest driver request failed: {e}");
            return GuestCreate::Failed;
        }
    };
    match (
        reply_text(&reply, "driver_id"),
        reply_text(&reply, "token"),
        reply_text(&reply, "display_name"),
    ) {
        (Some(driver_id), Some(token), Some(display_name)) => GuestCreate::Ready {
            identity: OnlineIdentity::new(&driver_id, &token),
            display_name,
        },
        _ => {
            log::warn!("Guest driver request returned a malformed reply");
            GuestCreate::Failed
        }
    }
}

/// What asking for a new guest name came to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuestRename {
    Renamed(String),
    RateLimited,
    /// The driver is an account driver now (connected from another
    /// computer, say), or orinks.net no longer knows this token.
    NotAGuest,
    Failed,
}

/// Ask orinks.net for another word-list name for the guest `identity`.
pub fn rename_guest(identity: &OnlineIdentity, transport: &dyn Transport) -> GuestRename {
    let reply = match transport.call(
        &format!("{}/api/freight-fate/guest/rename", base_url()),
        Some(&json!({"driverId": identity.driver_id})),
        &identity.auth_headers(),
        None,
    ) {
        Ok(reply) => reply,
        Err(NetError::Http { code: 429, .. }) => return GuestRename::RateLimited,
        Err(NetError::Http {
            code: 401 | 404 | 422,
            ..
        }) => return GuestRename::NotAGuest,
        Err(e) => {
            log::warn!("Guest rename failed: {e}");
            return GuestRename::Failed;
        }
    };
    match reply_text(&reply, "display_name") {
        Some(name) => GuestRename::Renamed(name),
        None => {
            log::warn!("Guest rename returned a malformed reply");
            GuestRename::Failed
        }
    }
}
