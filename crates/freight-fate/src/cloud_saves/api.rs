//! The Orinks cloud save API calls (used by the service worker and, via
//! menus, worker threads): upload, list, public-career choice, delete,
//! download and the verified restore. Re-exported from `crate::cloud_saves`.

use std::fmt;
use std::path::PathBuf;

use base64::Engine;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use super::sync_state::{json_int, SyncState};
use super::{
    cloud_content, profile_dict_from_content, CloudAuthError, PublicKeys, MAX_UPLOAD_BYTES,
};
use crate::meaningful_play::MeaningfulPlayStamp;
use crate::net::{NetError, Transport};
use crate::online_presence::{base_url, py_str, truthy, OnlineIdentity};
use ff_core::cloud_save_integrity::{verify_cloud_revision_with, CloudSaveIntegrityError};
use ff_core::models::career::level_for_xp;
use ff_core::models::profile::newer_career_line;
use ff_core::pyfmt::fmt_grouped;

fn saves_url() -> String {
    format!("{}/api/freight-fate/saves", base_url())
}

/// `urllib.parse.quote(text)`: percent-encode everything but the unreserved
/// characters and `/`.
pub fn url_quote(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// Whether orinks.net refused this machine's credentials.
///
/// Two different situations arrive here and the site does not distinguish
/// them: a retired token (this computer signed out from the account's
/// computer list) and a driver record that no longer exists at all. Both
/// answer `404 {"error": "driver_not_found"}` -- observed on the staging
/// site on 2026-08-11, after the deployment behind it was rebuilt and every
/// driver issued before the move stopped resolving. AUTH_HELP therefore
/// covers both, since the recovery differs: activating this computer again
/// for the first, a whole new account for the second.
fn auth_refused(code: u16, body: &Map<String, Value>) -> bool {
    code == 401
        || matches!(
            body.get("error").and_then(Value::as_str),
            Some("unauthorized") | Some("driver_not_found")
        )
}

/// The backup a marked snapshot arrived as a copy of: the recorded origin of
/// its career, sent only while the snapshot still carries the mark and only
/// in the exact shape of a content hash.
fn copied_from(profile_dict: &Value) -> Option<String> {
    if !truthy(profile_dict.get("integrity_modified")) {
        return None;
    }
    let name = profile_dict.get("name")?.as_str()?;
    ff_core::models::profile::origin::origin_for(name).filter(|hash| {
        hash.len() == 64 && hash.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    })
}

/// One upload attempt. Returns the reply dict on success (`ok`, `revision`,
/// `contentHash`), or a dict with `ok=false` and a `reason` (`conflict`
/// carries the server's latest revision details). Network trouble is
/// `reason="error"`.
pub fn upload_save(
    identity: &OnlineIdentity,
    save_name: &str,
    profile_dict: &Value,
    parent_revision: Option<i64>,
    summary: &str,
    meaningful_play: Option<&MeaningfulPlayStamp>,
    transport: &dyn Transport,
) -> Map<String, Value> {
    let (content, content_hash) = cloud_content(profile_dict);
    if content.len() > MAX_UPLOAD_BYTES {
        log::warn!(
            "Cloud backup of {save_name} skipped: {} bytes exceeds the limit",
            content.len()
        );
        return failure("too_large");
    }
    let version = json_int(profile_dict.get("version")).unwrap_or(0);
    let mut payload = json!({
        "driverId": identity.driver_id,
        "saveName": save_name,
        "saveVersion": version,
        "parentRevision": parent_revision,
        "contentHash": content_hash,
        "content": base64::engine::general_purpose::STANDARD.encode(&content),
        "summary": summary,
        "meaningfulPlay": meaningful_play,
        // Tells the server this build understands a career declined after
        // review, so it answers `review_declined` instead of a legacy reason.
        "reviewAware": true,
    });
    if let Some(origin) = copied_from(profile_dict) {
        payload["copiedFrom"] = Value::String(origin);
    }
    let reply = match transport.call(&saves_url(), Some(&payload), &identity.auth_headers(), None) {
        Ok(reply) => reply,
        Err(e @ NetError::Http { .. }) => {
            let code = e.http_code().unwrap_or(0);
            let body = e.error_body();
            if code == 409 && body.get("error").and_then(Value::as_str) == Some("conflict") {
                let mut out = failure("conflict");
                for (k, v) in body {
                    out.insert(k, v);
                }
                return out;
            }
            let reason = match body.get("error") {
                Some(Value::String(s)) if !s.is_empty() => s.clone(),
                Some(other) if truthy(Some(other)) => py_str(other),
                _ => format!("http_{code}"),
            };
            log::warn!("Cloud backup of {save_name} failed: {reason}");
            return failure(&reason);
        }
        Err(e) => {
            log::debug!("Cloud backup of {save_name} failed: {e}");
            return failure("error");
        }
    };
    if truthy(reply.get("ok")) {
        if let Some(revision) = json_int(reply.get("revision")) {
            let mut out = Map::new();
            out.insert("ok".to_string(), Value::Bool(true));
            out.insert("revision".to_string(), Value::from(revision));
            out.insert("contentHash".to_string(), Value::from(content_hash));
            if let Some(name) = reply.get("evictedSaveName").and_then(Value::as_str) {
                out.insert("evictedSaveName".to_string(), Value::from(name));
            }
            // The owner accepted a held career: the caller clears its mark.
            if reply.get("clearIntegrityFlag") == Some(&Value::Bool(true)) {
                out.insert("clearIntegrityFlag".to_string(), Value::Bool(true));
            }
            return out;
        }
    }
    failure("error")
}

fn failure(reason: &str) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("ok".to_string(), Value::Bool(false));
    out.insert("reason".to_string(), Value::from(reason));
    out
}

pub(crate) fn reason_of(result: &Map<String, Value>) -> Option<&str> {
    result.get("reason").and_then(Value::as_str)
}

/// The kept cloud revisions for a driver and which career fronts the public
/// profile.
#[derive(Debug, Clone, PartialEq)]
pub struct SavesList {
    /// newest first
    pub saves: Vec<Value>,
    /// `None` when no career is designated or the server predates the choice
    pub public_save_name: Option<String>,
}

/// All kept cloud revisions for this driver (`saves`, newest first) plus
/// which career fronts the public profile (`publicSaveName`, `None` when no
/// career is designated or the server predates the choice) -- or `None` when
/// the site is unreachable. `Err(CloudAuthError)` when the server answers
/// but refuses the credentials. Called from menu worker threads only.
pub fn list_saves(
    identity: &OnlineIdentity,
    transport: &dyn Transport,
) -> Result<Option<SavesList>, CloudAuthError> {
    let url = format!("{}?driverId={}", saves_url(), identity.driver_id);
    let reply = match transport.call(&url, None, &identity.auth_headers(), None) {
        Ok(reply) => reply,
        Err(e @ NetError::Http { .. }) => {
            let code = e.http_code().unwrap_or(0);
            if auth_refused(code, &e.error_body()) || code == 404 {
                log::warn!(
                    "Cloud save list refused (HTTP {code}): this computer's sign-in is no longer accepted"
                );
                return Err(CloudAuthError);
            }
            log::warn!("Cloud save list failed: HTTP {code}");
            return Ok(None);
        }
        Err(e) => {
            log::debug!("Cloud save list failed: {e}");
            return Ok(None);
        }
    };
    let Some(Value::Array(saves)) = reply.get("saves") else {
        return Ok(None);
    };
    let public = reply
        .get("publicSaveName")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok(Some(SavesList {
        saves: saves.clone(),
        public_save_name: public,
    }))
}

/// Choose which career fronts the driver's public profile (`None` returns
/// to the server's first-uploader rule). `true` on success, `false` when
/// the site could not be reached or refused. `Err(CloudAuthError)` when the
/// server answers but refuses the credentials. Called from menu worker
/// threads only.
pub fn set_public_save(
    identity: &OnlineIdentity,
    save_name: Option<&str>,
    transport: &dyn Transport,
) -> Result<bool, CloudAuthError> {
    let url = format!("{}/public-career", saves_url());
    let payload = json!({"driverId": identity.driver_id, "saveName": save_name});
    match transport.call(&url, Some(&payload), &identity.auth_headers(), None) {
        Ok(reply) => Ok(truthy(reply.get("ok"))),
        Err(e @ NetError::Http { .. }) => {
            let code = e.http_code().unwrap_or(0);
            if auth_refused(code, &e.error_body()) {
                log::warn!(
                    "Public career choice refused (HTTP {code}): this computer's sign-in is no longer accepted"
                );
                return Err(CloudAuthError);
            }
            log::warn!("Public career choice failed: HTTP {code}");
            Ok(false)
        }
        Err(e) => {
            log::debug!("Public career choice failed: {e}");
            Ok(false)
        }
    }
}

/// Remove every kept cloud revision of one slot from the account. `true` on
/// success, `false` when the site could not be reached or refused.
/// `Err(CloudAuthError)` when the server answers but refuses the
/// credentials. Called from menu worker threads only.
pub fn delete_save(
    identity: &OnlineIdentity,
    save_name: &str,
    transport: &dyn Transport,
) -> Result<bool, CloudAuthError> {
    let url = format!(
        "{}?driverId={}&saveName={}",
        saves_url(),
        identity.driver_id,
        url_quote(save_name)
    );
    match transport.call(&url, None, &identity.auth_headers(), Some("DELETE")) {
        Ok(reply) => Ok(truthy(reply.get("ok"))),
        Err(e @ NetError::Http { .. }) => {
            let code = e.http_code().unwrap_or(0);
            if auth_refused(code, &e.error_body()) {
                log::warn!(
                    "Cloud delete of {save_name} refused (HTTP {code}): this computer's sign-in is no longer accepted"
                );
                return Err(CloudAuthError);
            }
            log::warn!("Cloud delete of {save_name} failed: HTTP {code}");
            Ok(false)
        }
        Err(e) => {
            log::debug!("Cloud delete of {save_name} failed: {e}");
            Ok(false)
        }
    }
}

/// Why a download could not be used: refused credentials, or a revision
/// that failed the server-signature check.
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadError {
    Auth(CloudAuthError),
    Integrity(CloudSaveIntegrityError),
}

impl fmt::Display for DownloadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DownloadError::Auth(e) => write!(f, "{e}"),
            DownloadError::Integrity(e) => write!(f, "{}", e.message),
        }
    }
}

impl std::error::Error for DownloadError {}

/// One cloud revision, decoded and hash-verified: a dict with the slot
/// metadata plus `profile` (the profile dict) -- or `None` on any failure.
/// `keys` overrides the shipped signing-key table (tests). Called from menu
/// worker threads only.
pub fn download_save(
    identity: &OnlineIdentity,
    save_name: &str,
    revision: Option<i64>,
    transport: &dyn Transport,
    keys: Option<&PublicKeys>,
) -> Result<Option<Value>, DownloadError> {
    let mut url = format!(
        "{}/content?driverId={}&saveName={}",
        saves_url(),
        identity.driver_id,
        url_quote(save_name)
    );
    if let Some(revision) = revision {
        url.push_str(&format!("&revision={revision}"));
    }
    let reply = match transport.call(&url, None, &identity.auth_headers(), None) {
        Ok(reply) => reply,
        Err(e @ NetError::Http { .. }) => {
            let code = e.http_code().unwrap_or(0);
            if auth_refused(code, &e.error_body()) {
                log::warn!(
                    "Cloud save download of {save_name} refused (HTTP {code}): this computer's sign-in is no longer accepted"
                );
                return Err(DownloadError::Auth(CloudAuthError));
            }
            log::warn!("Cloud save download of {save_name} failed: HTTP {code}");
            return Ok(None);
        }
        Err(e) => {
            log::debug!("Cloud save download failed: {e}");
            return Ok(None);
        }
    };
    let content = match reply.get("content").and_then(Value::as_str).map(|text| {
        base64::engine::general_purpose::STANDARD
            .decode(text.trim())
            .or_else(|_| base64::engine::general_purpose::STANDARD_NO_PAD.decode(text.trim()))
    }) {
        Some(Ok(bytes)) => bytes,
        other => {
            log::debug!("Cloud save download failed: {other:?}");
            return Ok(None);
        }
    };
    let digest = hex::encode(Sha256::digest(&content));
    if reply.get("contentHash").and_then(Value::as_str) != Some(digest.as_str()) {
        log::warn!("Cloud save download of {save_name} failed its integrity check");
        return Ok(None);
    }
    let profile_dict = match profile_dict_from_content(&content) {
        Ok(profile) => profile,
        Err(e) => {
            log::warn!("Cloud save download of {save_name} unusable: {e}");
            return Ok(None);
        }
    };
    if let Err(e) = verify_cloud_revision_with(&profile_dict, &reply, keys, &|_| Ok(())) {
        log::warn!("Cloud save download of {save_name} unusable: {}", e.message);
        return Err(DownloadError::Integrity(e));
    }
    let get = |key: &str| reply.get(key).cloned().unwrap_or(Value::Null);
    let payload = json!({
        "saveName": reply.get("saveName").cloned().unwrap_or_else(|| Value::from(save_name)),
        "revision": get("revision"),
        "saveVersion": get("saveVersion"),
        "summary": reply.get("summary").cloned().unwrap_or_else(|| Value::from("")),
        "createdAt": get("createdAt"),
        "contentHash": get("contentHash"),
        "sig": get("sig"),
        "keyId": get("keyId"),
        "signedAt": get("signedAt"),
        "validatorVersion": get("validatorVersion"),
        // Absolution from the server, carried only on a reply whose revision
        // signature just verified above. The flag rides outside that signature,
        // so it is not proof of anything on its own -- but the worst a forged
        // one can do is clear a local advisory mark, and shared features read
        // the server's verdict rather than this flag.
        "clearIntegrityFlag": reply.get("clearIntegrityFlag") == Some(&Value::Bool(true)),
        "profile": profile_dict,
    });
    Ok(Some(payload))
}

/// Why a restore was refused before anything touched disk.
#[derive(Debug, Clone, PartialEq)]
pub enum RestoreError {
    /// `LegacyCareerError(name)`: a career from the 1.8 line does not restore here.
    LegacyCareer(String),
    NewerCareer {
        name: String,
        line: String,
    },
    /// The revision failed the server-signature check.
    Integrity(CloudSaveIntegrityError),
    /// The caller's writer could not install the file.
    Write(String),
}

impl fmt::Display for RestoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RestoreError::LegacyCareer(name) => {
                write!(
                    f,
                    "{name} was created by an earlier version of Freight Fate"
                )
            }
            RestoreError::NewerCareer { name, line } => {
                write!(f, "{name}: career created on the newer {line} line")
            }
            RestoreError::Integrity(e) => write!(f, "{}", e.message),
            RestoreError::Write(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for RestoreError {}

/// The profile-side pieces [`restore_to_disk`] writes through, because
/// `Profile` lives outside this crate.
pub struct RestoreHooks<'a> {
    /// `is_pre_1_9_save(profile_dict)`: the 1.9 load gate.
    pub is_legacy: &'a dyn Fn(&Value) -> bool,
    /// Build the `Profile` from the verified dict, sign it for this
    /// installation, and atomically install it over the local file (keeping
    /// the old file as `.ffsave.bak`); returns the installed path.
    pub write: &'a dyn Fn(&Value) -> Result<PathBuf, String>,
}

/// Write a downloaded cloud save over the local profile file.
///
/// Verification and construction happen before touching disk. The current
/// local file (if any) is kept beside it as `.ffsave.bak`. The replacement
/// is atomically installed with this machine's HMAC signature, and the old
/// file is put back if installation fails. Sync state changes only after
/// success.
pub fn restore_to_disk(
    payload: &Value,
    sync_state: Option<&SyncState>,
    hooks: &RestoreHooks<'_>,
    keys: Option<&PublicKeys>,
) -> Result<PathBuf, RestoreError> {
    let profile_dict = payload.get("profile").cloned().unwrap_or(Value::Null);
    if let Some(line) = profile_dict.as_object().and_then(newer_career_line) {
        return Err(RestoreError::NewerCareer {
            name: profile_dict
                .get("name")
                .and_then(Value::as_str)
                .filter(|name| !name.is_empty())
                .unwrap_or("Driver")
                .to_string(),
            line: line.to_string(),
        });
    }
    // Careers created before the 1.9 line do not restore here, for the same
    // reason the load gate refuses their local files: 1.9 starts everyone
    // fresh. Checked before anything touches disk; the cloud copy stays in
    // the account, still restorable by the 1.8 builds that made it.
    if (hooks.is_legacy)(&profile_dict) {
        let name = match profile_dict.get("name") {
            Some(Value::String(s)) if !s.is_empty() => s.clone(),
            _ => "Driver".to_string(),
        };
        return Err(RestoreError::LegacyCareer(name));
    }
    let mut profile = verify_cloud_revision_with(&profile_dict, payload, keys, &|_| Ok(()))
        .map_err(RestoreError::Integrity)?;
    // Absolution. The server grants this only on a revision it signed and
    // fully validated, so a career that was marked purely for moving between
    // computers stops carrying the mark. The signature is verified above,
    // before this is read -- an unsigned or failed reply never gets here, and
    // a career that really was edited fails validation instead of arriving
    // with the flag set.
    if payload.get("clearIntegrityFlag") == Some(&Value::Bool(true)) {
        if let Value::Object(map) = &mut profile {
            map.insert("integrity_modified".to_string(), Value::Bool(false));
            map.insert("integrity_notice_pending".to_string(), Value::Bool(false));
        }
    }
    let path = (hooks.write)(&profile).map_err(RestoreError::Write)?;
    // The restored career is the cloud's copy now, not the one that arrived
    // from another computer.
    if let Some(name) = profile.get("name").and_then(Value::as_str) {
        ff_core::models::profile::origin::forget_origin(name);
    }
    if let Some(sync_state) = sync_state {
        if let Some(revision) = json_int(payload.get("revision")) {
            let (_, content_hash) = cloud_content(&profile_dict);
            let save_name = payload
                .get("saveName")
                .and_then(Value::as_str)
                .unwrap_or("");
            sync_state.record_synced(save_name, revision, &content_hash);
            sync_state.clear_conflict(save_name);
        }
    }
    Ok(path)
}

/// A short spoken line describing a snapshot, shown in the restore menu.
pub fn backup_summary(profile_dict: &Value) -> String {
    let name = match profile_dict.get("name") {
        Some(Value::Null) | None => "Driver".to_string(),
        Some(v) => py_str(v),
    };
    let mut bits = vec![name];
    let career = profile_dict.get("career");
    let xp = career
        .and_then(|c| if c.is_object() { c.get("xp") } else { None })
        .and_then(as_number);
    if let Some(xp) = xp {
        bits.push(format!("level {}", level_for_xp(xp)));
    }
    if let Some(money) = profile_dict.get("money").and_then(as_number) {
        bits.push(format!("{} dollars", fmt_grouped(money, 0)));
    }
    bits.join(", ")
}

/// `isinstance(value, int | float)` (bools excluded), as a float.
fn as_number(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        _ => None,
    }
}
