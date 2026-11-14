//! The manual-playtest sandbox must never carry the real account into a
//! drive (port of `tests/test_playtest_sandbox.py`).
//!
//! `playtest::sandbox` exists for one guarantee: a throwaway career driven in
//! the sandbox cannot back itself up, cannot heartbeat onto the drivers
//! board, and cannot touch the public profile. That guarantee is not a
//! property of the tool's intentions -- it is the fact that the identity
//! loader finds no `online.json` there. These pin both halves: the seeding
//! never copies an identity in, and the audit refuses a sandbox that has one
//! anyway.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use freight_fate::app::testing::{env_lock, EnvGuard, TempDir};
use freight_fate::online_presence::{self, IdentityStore};
use freight_fate::playtest::sandbox;

/// The environment lock, plus putting back the variables `prepare` sets.
///
/// `sandbox::prepare` points the whole process at the sandbox, which is the
/// job -- for the game. In a test binary it outlived the lock: the guard
/// dropped, the temporary sandbox was deleted, and the variable went on
/// naming that deleted directory for every test that ran afterwards. Anything
/// that then asked where saves live got an answer belonging to a case that
/// had already finished. The site roots it now sets are restored the same way.
struct SandboxEnv {
    _lock: EnvGuard,
    previous: Vec<(&'static str, Option<std::ffi::OsString>)>,
}

const RESTORED: [&str; 3] = [
    "FREIGHT_FATE_DATA_DIR",
    online_presence::ONLINE_URL_ENV,
    online_presence::ONLINE_API_URL_ENV,
];

impl Drop for SandboxEnv {
    fn drop(&mut self) {
        for (name, value) in self.previous.drain(..) {
            match value {
                Some(old) => std::env::set_var(name, old),
                None => std::env::remove_var(name),
            }
        }
    }
}

fn sandbox_env() -> SandboxEnv {
    let lock = env_lock();
    SandboxEnv {
        _lock: lock,
        previous: RESTORED
            .iter()
            .map(|name| (*name, std::env::var_os(name)))
            .collect(),
    }
}

/// A stand-in for the owner's `saves/`: settings, careers, identity.
fn fake_real_saves(root: &Path) -> PathBuf {
    let source = root.join("saves");
    std::fs::create_dir_all(source.join("profiles")).unwrap();
    write(
        &source.join("settings.json"),
        &json!({"cloud_saves": true, "online_presence": true, "master_volume": 0.5}).to_string(),
    );
    write(
        &source.join("online.json"),
        &json!({"driver_id": "d".repeat(16), "driver_token": "t".repeat(40)}).to_string(),
    );
    write(&source.join("online.json.pre-clerk.bak"), "{}");
    write(&source.join("online.token"), &"t".repeat(40));
    write(
        &source.join("cloud_saves.json"),
        &json!({"slots": {}}).to_string(),
    );
    write(&source.join("profiles").join("Playtest.ffsave"), "career");
    write(
        &source.join("profiles").join("Old.json.bak"),
        "not a career",
    );
    source
}

fn write(path: &Path, text: &str) {
    std::fs::write(path, text).unwrap();
}

fn read_settings(path: &Path) -> serde_json::Map<String, Value> {
    let text = std::fs::read_to_string(path).unwrap();
    serde_json::from_str::<Value>(&text)
        .unwrap()
        .as_object()
        .unwrap()
        .clone()
}

#[test]
fn test_seeding_carries_careers_but_never_the_identity() {
    let _guard = sandbox_env();
    let root = TempDir::new("ff-sandbox");
    let source = fake_real_saves(root.path());
    let sandbox = root.path().join("sandbox");

    sandbox::prepare(&sandbox, false, true, &source).unwrap();

    assert!(sandbox.join("profiles").join("Playtest.ffsave").is_file());
    // The stale `.json.bak` leftovers are not careers the game loads, so a
    // sandbox does not want them cluttering its career list either.
    assert!(!sandbox.join("profiles").join("Old.json.bak").exists());
    assert!(!sandbox.join("online.json").exists());
    assert!(!sandbox.join("online.token").exists());
    assert!(!sandbox.join("cloud_saves.json").exists());
    assert!(sandbox::audit(&sandbox).is_empty());
}

/// The agent's online session reaches staging with a driver of its own. The
/// retired `--online` mode copied the real `online.json` in; a source build
/// then handed anyone an agent that backed up to production as themselves.
#[test]
fn test_the_staging_session_takes_no_identity_and_talks_only_to_staging() {
    let _guard = sandbox_env();
    let root = TempDir::new("ff-sandbox");
    let source = fake_real_saves(root.path());
    let staging = root.path().join("staging");

    sandbox::prepare_staging(&staging, false, &source).unwrap();
    let url = online_presence::base_url();

    assert_eq!(url, sandbox::STAGING_URL);
    assert!(!staging.join("online.json").exists());
    assert!(!staging.join("online.token").exists());
    assert!(!staging.join("cloud_saves.json").exists());
    assert!(!staging.join("profiles").exists());
    assert_eq!(
        read_settings(&staging.join("settings.json"))["cloud_saves"],
        true
    );
}

/// A plain sandbox reaches no Orinks site at all. Leaving the identity out
/// stopped backups and heartbeats, but Set up this computer with orinks.net
/// needs no identity: an agent drive on 2026-11-12 asked production for an
/// activation code and got one. Every Orinks address the game builds now
/// names the reserved, unresolvable `.invalid` host instead, even when the
/// shell carried a site or a presence backend of its own.
#[test]
fn test_the_plain_sandbox_cannot_reach_any_orinks_site() {
    let _guard = sandbox_env();
    std::env::set_var(
        online_presence::ONLINE_URL_ENV,
        online_presence::PRODUCTION_BASE_URL,
    );
    std::env::set_var(
        online_presence::ONLINE_API_URL_ENV,
        online_presence::PRODUCTION_PRESENCE_API_URL,
    );
    let root = TempDir::new("ff-sandbox");
    let source = fake_real_saves(root.path());
    let sandbox = root.path().join("sandbox");

    sandbox::prepare(&sandbox, false, true, &source).unwrap();

    let addresses = [
        online_presence::base_url(),
        online_presence::presence_api_url("presence"),
        online_presence::presence_api_url("directory"),
        online_presence::setup_page_url(),
    ];
    for address in &addresses {
        assert!(
            address.starts_with(sandbox::OFFLINE_URL),
            "{address} leaves the sandbox"
        );
        assert!(!address.contains("orinks.net"), "{address}");
        assert!(!address.contains("convex"), "{address}");
    }
    assert!(sandbox::OFFLINE_URL.ends_with(".invalid"));
}

#[test]
fn test_the_seeded_settings_have_every_publishing_switch_off() {
    let _guard = sandbox_env();
    let root = TempDir::new("ff-sandbox");
    let source = fake_real_saves(root.path());
    let sandbox = root.path().join("sandbox");

    sandbox::prepare(&sandbox, false, true, &source).unwrap();

    let settings = read_settings(&sandbox.join("settings.json"));
    for key in sandbox::OFFLINE_SETTINGS {
        assert_eq!(settings.get(key), Some(&Value::Bool(false)), "{key}");
    }
    // The live-data feeds carry no identity and are what a sandbox drive is
    // there to hear, so they are on whatever the real settings said, and a
    // later prepare puts them back on if a session turned one off.
    for key in sandbox::LIVE_SETTINGS {
        assert_eq!(settings.get(key), Some(&Value::Bool(true)), "{key}");
    }
    let mut flipped = settings.clone();
    flipped.insert("real_traffic".to_string(), Value::Bool(false));
    std::fs::write(
        sandbox.join("settings.json"),
        serde_json::to_string_pretty(&Value::Object(flipped)).unwrap(),
    )
    .unwrap();
    sandbox::prepare(&sandbox, false, true, &source).unwrap();
    let settings = read_settings(&sandbox.join("settings.json"));
    assert_eq!(settings.get("real_traffic"), Some(&Value::Bool(true)));
    // Everything else is copied through, because the point of seeding real
    // settings is that the drive reproduces what a player would get.
    assert_eq!(
        settings.get("master_volume").and_then(Value::as_f64),
        Some(0.5)
    );
}

#[test]
fn test_no_careers_leaves_the_sandbox_empty() {
    let _guard = sandbox_env();
    let root = TempDir::new("ff-sandbox");
    let source = fake_real_saves(root.path());
    let sandbox = root.path().join("sandbox");

    sandbox::prepare(&sandbox, false, false, &source).unwrap();

    assert!(!sandbox.join("profiles").exists());
}

/// The audit is the guard, so it has to fail on a sandbox somebody signed in
/// by hand -- including the backup spellings of the file.
#[test]
fn test_the_audit_names_an_identity_that_got_in_somehow() {
    let _guard = sandbox_env();
    let root = TempDir::new("ff-sandbox");
    let source = fake_real_saves(root.path());
    let sandbox = root.path().join("sandbox");
    sandbox::prepare(&sandbox, false, true, &source).unwrap();

    write(&sandbox.join("online.json.playtest1.bak"), "{}");

    let problems = sandbox::audit(&sandbox);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("online.json.playtest1.bak")),
        "{problems:?}"
    );
}

#[test]
fn test_prepare_clears_what_the_last_sandbox_session_wrote() {
    // The sandboxed game stamps meaningful_play.json on every accepted job,
    // online or not; the next session's audit named it and refused to boot
    // (every second agent session, 2026-09-01). Preparing again clears the
    // last session's own leftovers before seeding, and the audit that follows
    // is clean -- the identity in the real saves still never gets in.
    let _guard = sandbox_env();
    let root = TempDir::new("ff-sandbox");
    let source = fake_real_saves(root.path());
    let sandbox = root.path().join("sandbox");
    sandbox::prepare(&sandbox, false, true, &source).unwrap();
    write(&sandbox.join("meaningful_play.json"), "{}");
    write(&sandbox.join("online-outbox.json"), "[]");
    let before = sandbox::audit(&sandbox);
    assert!(
        !before.is_empty(),
        "the leftovers are what the audit refuses"
    );

    sandbox::prepare(&sandbox, false, true, &source).unwrap();

    assert!(!sandbox.join("meaningful_play.json").exists());
    assert!(!sandbox.join("online-outbox.json").exists());
    assert!(sandbox::audit(&sandbox).is_empty());
    assert!(
        source.join("online.json").exists(),
        "the real saves are never touched"
    );
}

#[test]
fn test_the_audit_names_pending_meaningful_play_intent() {
    let _guard = sandbox_env();
    let root = TempDir::new("ff-sandbox");
    let source = fake_real_saves(root.path());
    let sandbox = root.path().join("sandbox");
    sandbox::prepare(&sandbox, false, true, &source).unwrap();

    write(&sandbox.join("meaningful_play.json"), "{}");

    let problems = sandbox::audit(&sandbox);
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("meaningful_play.json")),
        "{problems:?}"
    );
}

#[test]
fn test_the_audit_names_a_publishing_switch_turned_back_on() {
    let _guard = sandbox_env();
    let root = TempDir::new("ff-sandbox");
    let source = fake_real_saves(root.path());
    let sandbox = root.path().join("sandbox");
    sandbox::prepare(&sandbox, false, true, &source).unwrap();

    let mut settings = read_settings(&sandbox.join("settings.json"));
    settings.insert("cloud_saves".to_string(), Value::Bool(true));
    write(
        &sandbox.join("settings.json"),
        &Value::Object(settings).to_string(),
    );

    let problems = sandbox::audit(&sandbox);
    assert!(
        problems.iter().any(|p| p.contains("cloud_saves")),
        "{problems:?}"
    );
}

/// The guarantee itself, through the game's own loader.
///
/// Every cloud backup, presence heartbeat and profile update in the game
/// hangs off the identity store returning something. In a sandbox it returns
/// nothing, so those are branches the drive never takes.
#[test]
fn test_a_sandbox_data_dir_has_no_driver_at_all() {
    let _guard = sandbox_env();
    let root = TempDir::new("ff-sandbox");
    let source = fake_real_saves(root.path());
    let sandbox = root.path().join("sandbox");
    sandbox::prepare(&sandbox, false, true, &source).unwrap();

    let store = IdentityStore::platform(&sandbox);
    // The token cache outlives one identity, and a cached token would mask
    // exactly the failure this test is here to catch.
    store.clear_cache();

    assert_eq!(store.path(), sandbox.join("online.json"));
    assert!(store.load().is_none());
}

/// Seeding copies out of `saves/`; it must never write back into it.
#[test]
fn test_the_real_saves_are_only_ever_read() {
    let _guard = sandbox_env();
    let root = TempDir::new("ff-sandbox");
    let source = fake_real_saves(root.path());
    let before = snapshot(&source);

    sandbox::prepare(&root.path().join("sandbox"), false, true, &source).unwrap();

    assert_eq!(snapshot(&source), before);
}

fn snapshot(dir: &Path) -> Vec<(PathBuf, std::time::SystemTime)> {
    let mut out = Vec::new();
    collect(dir, &mut out);
    out.sort();
    out
}

fn collect(dir: &Path, out: &mut Vec<(PathBuf, std::time::SystemTime)>) {
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, out);
        } else {
            let modified = entry
                .metadata()
                .and_then(|m| m.modified())
                .unwrap_or(std::time::UNIX_EPOCH);
            out.push((path, modified));
        }
    }
}
