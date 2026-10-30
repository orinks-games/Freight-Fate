//! The in-game updater, on Hearken's signed engine updater
//! (`hearken-update`).
//!
//! The engine does the checks, the download and the swap; this module says
//! where Freight Fate publishes its updates and what this copy is. The
//! spoken screens are in `states/update.rs`.
//!
//! - **What this copy is** comes from the build: `cargo tools build-release`
//!   bakes the game's public update key, its update version and channel
//!   into `hearken-update` (the `HEARKEN_UPDATE_*` variables). A build
//!   without a key, such as `cargo run`, never updates itself.
//! - **Channels**: `stable` follows tagged releases, `dev` the tester
//!   snapshots (`1.9-tester-YYYYMMDD`). A snapshot's update version is the
//!   next patch after the Cargo version, as a pre-release stamped with its
//!   commit's time (`1.9.6-dev.202610300537`), so each newer snapshot sorts
//!   above the last and below the stable it leads to.
//! - **Where**: each release carries a signed manifest per platform,
//!   `update-<channel>-<platform>.json`, beside its archive, and for a zip
//!   a signed file index, so a player downloads only the files that changed.
//!   Stable manifests are read from the latest release, snapshots from the
//!   newest releases that carry one.
//! - **A mirror**: a player who cannot reach GitHub can name an HTTPS
//!   mirror prefix; every request goes through it. The manifests are
//!   signed, so a mirror can withhold an update but never change one.
//!
//! The old updater's releases stay as they were: the same archives under
//! the same names, so copies from before this one still find and install
//! the next release, and from then on update through this module.

use std::fs;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use once_cell::sync::Lazy;
use serde_json::Value;

pub use hearken_update::{
    Blocked, DownloadError, Fetch, Host, Install, Limits, Phase, Running, Staged, Update,
    UpdateSource,
};
use hearken_update::{
    Body, FetchError, Found, GitHubReleases, HttpFetch, ManifestUrl, Query, RangeBody, SourceError,
};

pub const REPO: &str = "orinks-games/Freight-Fate";
pub const APP_NAME: &str = "FreightFate";
/// The package name the manifests are signed for.
pub const GAME: &str = "freight-fate";

pub const CHANNELS: [&str; 2] = ["stable", "dev"];

/// Where stable manifests are: the latest release's assets. GitHub
/// redirects these to the release's own files, and the archive and file
/// index are found beside the manifest.
pub const STABLE_MANIFESTS: &str = "https://github.com/orinks-games/Freight-Fate/releases/latest/download/update-{channel}-{platform}.json";

/// The platform name an AppImage copy is published under: the AppImage is
/// its own archive, beside the tarball of the same platform.
pub const APPIMAGE_PLATFORM_SUFFIX: &str = "-appimage";

/// Validate and normalize a player-supplied GitHub mirror prefix.
///
/// Mirrors receive the original URL immediately after this prefix, so
/// exactly one trailing slash keeps concatenation predictable.
pub fn normalize_mirror_prefix(prefix: &str) -> Result<String, &'static str> {
    let prefix = prefix.trim();
    if prefix.is_empty() {
        return Err("Type an HTTPS mirror address.");
    }
    let parsed = url::Url::parse(prefix).map_err(|_| "Type a valid HTTPS mirror address.")?;
    if parsed.scheme() != "https" || parsed.host_str().is_none() {
        return Err("The mirror address must start with https://.");
    }
    if parsed.query().is_some() || parsed.fragment().is_some() {
        return Err("The mirror address cannot contain a query or fragment.");
    }
    Ok(format!("{}/", prefix.trim_end_matches('/')))
}

/// Prepend `mirror_prefix` to a URL, or leave it direct when disabled.
pub fn mirror_url(url: &str, mirror_prefix: Option<&str>) -> Result<String, &'static str> {
    match mirror_prefix {
        Some(prefix) => Ok(format!("{}{url}", normalize_mirror_prefix(prefix)?)),
        None => Ok(url.to_string()),
    }
}

// -- the process ------------------------------------------------------------

/// `sys.platform`, as the build identity branches on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
    Other,
}

impl Platform {
    pub fn current() -> Self {
        if cfg!(target_os = "windows") {
            Platform::Windows
        } else if cfg!(target_os = "macos") {
            Platform::MacOs
        } else if cfg!(target_os = "linux") {
            Platform::Linux
        } else {
            Platform::Other
        }
    }
}

/// The process facts the build identity reads, gathered so tests can fake
/// them.
#[derive(Debug, Clone)]
pub struct UpdaterEnv {
    pub platform: Platform,
    /// The running executable.
    pub executable: PathBuf,
}

impl UpdaterEnv {
    pub fn current() -> Self {
        Self {
            platform: Platform::current(),
            executable: std::env::current_exe().unwrap_or_default(),
        }
    }

    /// A fake process for tests.
    pub fn fake(platform: Platform, executable: &Path) -> Self {
        Self {
            platform,
            executable: executable.to_path_buf(),
        }
    }
}

// -- build identity -----------------------------------------------------------

/// What this running copy of the game is, from its `build_info.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildInfo {
    /// "v1.5.0", "nightly-20260611", or "1.9-tester-20260828"
    pub tag: String,
    /// "stable" or "dev"
    pub channel: String,
    /// "2026-06-11" (UTC date); "" when unknown
    pub built_at: String,
    /// The commit this copy was built from; "" for builds stamped before
    /// 2026-10-03.
    pub commit: String,
}

impl BuildInfo {
    pub fn new(tag: &str, channel: &str, built_at: &str) -> Self {
        Self {
            tag: tag.to_string(),
            channel: channel.to_string(),
            built_at: built_at.to_string(),
            commit: String::new(),
        }
    }
}

/// True when running as a packaged build rather than a source checkout.
///
/// A packaged build is the `FreightFate` executable sitting beside its
/// `build_info.json` (or the shipped `freight_fate` data folder, or an
/// `_internal` folder from the older PyInstaller layout). A `cargo run`
/// binary in `target/` has none of those and stays a source checkout.
pub fn is_frozen_in(env: &UpdaterEnv) -> bool {
    let root = resource_root_in(env);
    let exe_name = env
        .executable
        .file_stem()
        .map(|s| s.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    exe_name == APP_NAME.to_ascii_lowercase()
        && (root.join("build_info.json").exists()
            || root.join("freight_fate").exists()
            || root.join("_internal").exists())
}

/// Whether this build may download and apply its own updates. An iOS app
/// cannot replace itself: the App Store or TestFlight does that.
pub const SELF_UPDATES: bool = hearken_update::SELF_UPDATES;

pub fn is_frozen() -> bool {
    is_frozen_in(&UpdaterEnv::current())
}

pub use hearken_update::{agnow_value_set, managed_by_agnow};

/// Whether this copy may check for, download and apply its own updates:
/// not on iPhone and iPad ([`SELF_UPDATES`]) and not on AG Now
/// ([`managed_by_agnow`]), where something else does the updating.
pub fn self_updates() -> bool {
    SELF_UPDATES && !managed_by_agnow()
}

/// Folder containing immutable packaged assets and build metadata.
fn resource_root_in(env: &UpdaterEnv) -> PathBuf {
    ff_core::data::data_resources::resource_dir_for_executable(
        &env.executable,
        env.platform == Platform::MacOs,
    )
}

/// Read build_info.json from the install folder. Returns `None` when
/// running from source. Frozen builds that predate the stamp fall back to a
/// stable identity derived from the package version.
pub fn load_build_info_in(env: &UpdaterEnv, version: &str) -> Option<BuildInfo> {
    if !is_frozen_in(env) {
        return None;
    }
    let path = resource_root_in(env).join("build_info.json");
    let data: Option<Value> = fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok());
    match data {
        Some(data) => Some(build_info_from_dict(&data, version)),
        None => Some(BuildInfo::new(&format!("v{version}"), "stable", "")),
    }
}

/// `(version asked for, the answer)`.
type BuildInfoCache = Option<(String, Option<BuildInfo>)>;

static BUILD_INFO_CACHE: Lazy<Mutex<BuildInfoCache>> = Lazy::new(|| Mutex::new(None));

/// [`load_build_info_in`] for the real process; cached, since menu labels
/// ask every frame and the answer never changes mid-session.
pub fn load_build_info(version: &str) -> Option<BuildInfo> {
    let mut cache = BUILD_INFO_CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((cached_version, info)) = cache.as_ref() {
        if cached_version == version {
            return info.clone();
        }
    }
    let info = load_build_info_in(&UpdaterEnv::current(), version);
    *cache = Some((version.to_string(), info.clone()));
    info
}

/// `str(value)` of a JSON scalar, `""` for null/missing, for the stamp's
/// loosely typed fields.
fn stamp_str(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(false)) => String::new(),
        Some(other) => crate::online_presence::py_str(other),
    }
}

/// Whether `tag` names a snapshot: a 1.8 `nightly-YYYYMMDD` or a 1.9
/// `1.9-tester-YYYYMMDD`.
fn is_snapshot_tag(tag: &str) -> bool {
    let date = tag
        .strip_prefix("1.9-tester-")
        .or_else(|| tag.strip_prefix("nightly-"));
    date.is_some_and(|d| d.len() == 8 && d.bytes().all(|b| b.is_ascii_digit()))
}

/// Normalize a packaged build stamp, preserving useful partial data.
pub fn build_info_from_dict(data: &Value, version: &str) -> BuildInfo {
    let Value::Object(map) = data else {
        return BuildInfo::new(&format!("v{version}"), "stable", "");
    };
    let mut tag = stamp_str(map.get("tag"));
    if tag.is_empty() {
        tag = format!("v{version}");
    }
    let mut channel = stamp_str(map.get("channel"));
    if !CHANNELS.contains(&channel.as_str()) {
        channel = if is_snapshot_tag(&tag) {
            "dev".to_string()
        } else {
            "stable".to_string()
        };
    }
    BuildInfo {
        tag,
        channel,
        built_at: stamp_str(map.get("built_at")),
        commit: stamp_str(map.get("commit")),
    }
}

/// The effective update channel: the player's explicit choice, else
/// whatever channel this build came from.
pub fn resolve_channel(setting: &str, build: Option<&BuildInfo>) -> String {
    if CHANNELS.contains(&setting) {
        return setting.to_string();
    }
    if let Some(build) = build {
        if CHANNELS.contains(&build.channel.as_str()) {
            return build.channel.clone();
        }
    }
    "stable".to_string()
}

/// Player-facing wording for the game's own version: '1.8.6.dev0' becomes
/// '1.8.6 development build' so spoken menus never read packaging jargon.
pub fn spoken_version(version: &str) -> String {
    match version.split_once(".dev") {
        Some((base, _)) => format!("{base} development build"),
        None => version.to_string(),
    }
}

// -- the update identity --------------------------------------------------------

/// What this copy is to the updater, as the release build baked it in;
/// `None` for any build made without the update key.
///
/// An AppImage copy is published under its own platform name
/// (`linux-x64-appimage`), since its update is the AppImage file itself,
/// not the tarball beside it.
pub fn running() -> Option<Running> {
    running_for(Running::baked()?, &Host::current())
}

/// `running` for `host`: the AppImage platform when it runs from one.
pub fn running_for(mut running: Running, host: &Host) -> Option<Running> {
    if matches!(host.install(), Install::AppImage { .. })
        && !running.platform.ends_with(APPIMAGE_PLATFORM_SUFFIX)
    {
        running.platform.push_str(APPIMAGE_PLATFORM_SUFFIX);
    }
    Some(running)
}

/// Why this copy cannot update itself, if it cannot.
pub fn blocked(running: Option<&Running>, host: &Host) -> Option<Blocked> {
    hearken_update::blocked(running, host, managed_by_agnow())
}

/// Spoken: why this copy does not update itself.
pub fn blocked_line(why: Blocked) -> &'static str {
    match why {
        Blocked::Store => "On iPhone and iPad, updates come through the App Store or TestFlight.",
        Blocked::AgNow => "AG Now keeps this copy up to date.",
        Blocked::DevBuild | Blocked::CargoBuild => {
            "Updates are only available in the packaged game. This copy runs from source; \
             update it with git."
        }
    }
}

/// The update version a snapshot's spoken title is made from:
/// `1.9.6-dev.202610300537` is the 1.9 tester snapshot of 2026-10-30.
fn snapshot_date(version: &str) -> Option<String> {
    let (_, pre) = version.split_once('-')?;
    let stamp = pre.strip_prefix("dev.")?;
    let date = stamp.get(..8)?;
    date.bytes()
        .all(|b| b.is_ascii_digit())
        .then(|| format!("{}-{}-{}", &date[..4], &date[4..6], &date[6..]))
}

/// The spoken name of an update: "Freight Fate version 1.9.6", or for a
/// snapshot "Freight Fate 1.9 tester snapshot 2026-10-30".
pub fn update_title(version: &str) -> String {
    match snapshot_date(version) {
        Some(date) => {
            let mut parts = version.split(['.', '-']);
            let (major, minor) = (parts.next().unwrap_or("1"), parts.next().unwrap_or("9"));
            format!("Freight Fate {major}.{minor} tester snapshot {date}")
        }
        None => format!("Freight Fate version {}", version.trim_start_matches('v')),
    }
}

// -- where updates are --------------------------------------------------------

/// Freight Fate's releases: stable manifests from the latest release, and
/// snapshot manifests from the newest releases that carry one.
pub struct ReleaseSource {
    stable: ManifestUrl,
    snapshots: GitHubReleases,
}

impl ReleaseSource {
    pub fn new() -> Self {
        let (owner, repo) = REPO.split_once('/').expect("owner/repo");
        Self {
            stable: ManifestUrl::new(STABLE_MANIFESTS).expect("an HTTPS manifest URL"),
            snapshots: GitHubReleases::new(owner, repo).expect("a GitHub name"),
        }
    }
}

impl Default for ReleaseSource {
    fn default() -> Self {
        Self::new()
    }
}

impl UpdateSource for ReleaseSource {
    fn find(&self, query: &Query, fetch: &dyn Fetch) -> Result<Option<Found>, SourceError> {
        if query.channel == "stable" {
            self.stable.find(query, fetch)
        } else {
            self.snapshots.find(query, fetch)
        }
    }

    fn describe(&self) -> String {
        format!("github.com/{REPO}")
    }
}

/// HTTPS through the engine's client, every request sent through the
/// player's mirror when one is set.
pub struct MirrorFetch {
    inner: HttpFetch,
    prefix: Option<String>,
}

impl MirrorFetch {
    /// A fetch through `prefix`, or straight to each host with `None`. An
    /// invalid prefix is refused up front.
    pub fn new(prefix: Option<&str>) -> Result<Self, &'static str> {
        let prefix = prefix.map(normalize_mirror_prefix).transpose()?;
        Ok(Self {
            inner: HttpFetch::default(),
            prefix,
        })
    }

    /// The URL to send, once the game may reach the network at all (see
    /// [`crate::net::require_real_network`]): a test never sends one.
    fn url(&self, url: &str) -> String {
        let url = match &self.prefix {
            Some(prefix) => format!("{prefix}{url}"),
            None => url.to_string(),
        };
        crate::net::require_real_network("GET", &url);
        url
    }
}

impl Fetch for MirrorFetch {
    fn get(&self, url: &str, headers: &[(&str, &str)], limit: u64) -> Result<Vec<u8>, FetchError> {
        self.inner.get(&self.url(url), headers, limit)
    }

    fn open(&self, url: &str) -> Result<Body, FetchError> {
        self.inner.open(&self.url(url))
    }

    fn supports_ranges(&self) -> bool {
        self.inner.supports_ranges()
    }

    fn open_range(&self, url: &str, range: Range<u64>) -> Result<Option<RangeBody>, FetchError> {
        self.inner.open_range(&self.url(url), range)
    }
}

/// The fetch for the player's settings.
pub fn fetch_for(mirror_prefix: Option<&str>) -> Result<Arc<dyn Fetch>, &'static str> {
    Ok(Arc::new(MirrorFetch::new(mirror_prefix)?))
}

// -- what an update is ------------------------------------------------------------

/// An update to offer, with what the screens say about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpdateInfo {
    /// The engine's checked update: its signed manifest, where its archive
    /// is, and which files this copy can keep.
    pub update: Update,
    /// The version it installs: what Skip this version remembers.
    pub tag: String,
    /// Spoken name, e.g. "Freight Fate version 1.9.6".
    pub title: String,
    /// The release notes as speakable lines.
    pub notes: Vec<String>,
}

impl UpdateInfo {
    pub fn new(update: Update) -> Self {
        let version = update.version().to_string();
        Self {
            title: update_title(&version),
            notes: update.manifest.note_lines(),
            tag: version,
            update,
        }
    }

    /// Bytes the player will download: only the changed files when this
    /// copy has the rest.
    pub fn download_size(&self) -> u64 {
        self.update.download_size()
    }
}

/// Look for an update on `channel`, and which of this copy's files it can
/// keep. Blocking: run it off the game loop. `Ok(None)` is up to date.
pub fn check_for_update(
    channel: &str,
    mirror_prefix: Option<&str>,
    cancelled: &AtomicBool,
) -> Result<Option<UpdateInfo>, String> {
    let host = Host::current();
    let running = running();
    if let Some(why) = blocked(running.as_ref(), &host) {
        return Err(blocked_line(why).to_string());
    }
    let running = running.expect("not blocked");
    let fetch = fetch_for(mirror_prefix).map_err(str::to_string)?;
    check_with(
        &ReleaseSource::new(),
        fetch.as_ref(),
        &running,
        &host.install(),
        channel,
        cancelled,
    )
}

/// [`check_for_update`] against any source, fetch and install, for tests.
pub fn check_with(
    source: &dyn UpdateSource,
    fetch: &dyn Fetch,
    running: &Running,
    install: &Install,
    channel: &str,
    cancelled: &AtomicBool,
) -> Result<Option<UpdateInfo>, String> {
    match hearken_update::check(source, fetch, running, channel) {
        Ok(None) => Ok(None),
        Ok(Some(mut update)) => {
            update.plan = hearken_update::plan_until(&update, fetch, install, cancelled);
            Ok(Some(UpdateInfo::new(update)))
        }
        Err(e) => {
            log::warn!("Update check of {} failed: {e}", source.describe());
            Err(check_error_line(&e))
        }
    }
}

/// Spoken: why a check found nothing it could offer.
pub fn check_error_line(e: &hearken_update::CheckError) -> String {
    use hearken_update::{CheckError, Refusal};
    match e {
        CheckError::Source(_) => "Could not reach the update server.".to_string(),
        CheckError::Refused(Refusal::Manifest(_)) => {
            "The update server offered an update that is not signed by Freight Fate, \
             so it was ignored."
                .to_string()
        }
        CheckError::Refused(_) => {
            "The update server offered an update for another copy of the game, \
             so it was ignored."
                .to_string()
        }
        CheckError::BadRunningVersion(_) => {
            "This copy does not know its own version, so it cannot update itself.".to_string()
        }
    }
}

// -- installing it ---------------------------------------------------------------

/// The prefix of this game's staging folders under the system's temporary
/// folder.
pub const STAGING_PREFIX: &str = "freight-fate-update";

/// A new staging folder for one download.
pub fn make_staging_dir() -> io::Result<PathBuf> {
    hearken_update::make_staging_dir(STAGING_PREFIX)
}

/// Download `info` into `staging` and unpack it, ready to replace
/// `install`: only the changed files when it can, else the whole archive.
#[allow(clippy::too_many_arguments)]
pub fn download_and_stage(
    info: &UpdateInfo,
    fetch: &dyn Fetch,
    staging: &Path,
    install: &Install,
    progress: &mut dyn FnMut(u64, u64),
    phase: &mut dyn FnMut(Phase),
    cancelled: &AtomicBool,
) -> Result<Staged, DownloadError> {
    hearken_update::download_and_stage(
        &info.update,
        fetch,
        staging,
        install,
        Limits::default(),
        progress,
        phase,
        cancelled,
    )
}

pub use hearken_update::{apply_and_restart, can_auto_apply, stash_for_manual_install};

/// At start, before anything is read: put back an update that was stopped
/// part-way. `Some` line to say when there was one.
pub fn recover_interrupted() -> Option<String> {
    let host = Host::current();
    if blocked(running().as_ref(), &host).is_some() {
        return None;
    }
    match hearken_update::recover_interrupted(&host.install()) {
        Ok(hearken_update::Recovery::Nothing) => None,
        Ok(hearken_update::Recovery::Restored) => Some(
            "The last update did not finish, so the game was put back as it was. \
             Check for updates in Settings to try again."
                .to_string(),
        ),
        Ok(hearken_update::Recovery::NoBackup) => Some(
            "The last update did not finish and could not be undone. Download the game \
             again from github.com/orinks-games/Freight-Fate/releases."
                .to_string(),
        ),
        Err(e) => {
            log::warn!("Could not check for an unfinished update: {e}");
            None
        }
    }
}

static RECOVERY_NOTICE: Mutex<Option<String>> = Mutex::new(None);

/// [`recover_interrupted`] at start, keeping what to say for the main menu.
pub fn note_interrupted_update() {
    if let Some(line) = recover_interrupted() {
        log::warn!("{line}");
        *RECOVERY_NOTICE.lock().unwrap_or_else(|e| e.into_inner()) = Some(line);
    }
}

/// What the start-up recovery found, once.
pub fn take_recovery_notice() -> Option<String> {
    RECOVERY_NOTICE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
}
