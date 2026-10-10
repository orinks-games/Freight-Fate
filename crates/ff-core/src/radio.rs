//! In-cab radio catalog, reception, safety gates, and playback state.
//!
//! Port of `freight_fate/radio.py`, split three ways: this file carries the
//! station model, the catalog loaders, the stream-URL identity rules, the
//! dial categories and the simulated FM physics; [`playlists`] reads the
//! player's own `.m3u` / `.m3u8` / `.pls` files into stations; [`state`] is
//! the mutable [`RadioState`] with the dial, the streamer-safe gate and the
//! two-strike fallback.
//!
//! What stayed behind: the Python module imported `models.profile` for the
//! favourites and the Playlists folder, and `data.data_resources` for the
//! catalog JSON. Here the catalog is read from an explicit data root
//! ([`load_full_catalog`]; [`default_radio_catalog`] finds the shipped one),
//! favourites arrive as a plain list of ids, and the Playlists directory is
//! a parameter -- the game crate wires all three.

pub mod community;
pub mod playlists;
pub mod state;
mod synth_dial;
#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;

pub use playlists::{
    absolute_anywhere, is_stream_entry, load_personal_playlists, parse_m3u, parse_playlist_file,
    parse_pls, PERSONAL_PLAYLIST_SOURCE_TYPE, PLAYLISTS_DIR_NAME, PLAYLIST_SUFFIXES,
};
pub use state::{RadioSettingsAccess, RadioState, FAVORITES_GROUP, TERRESTRIAL_GROUP};

pub const SAFE_ROUTE_PLAYLIST: &str = "route_playlist";
pub const SAFE_FALLBACK_STATION_ID: &str = "ff-safety-satellite";
/// Where the dial lands when a station is lost and a real stream is still
/// allowed: AFN Humphreys The Eagle, the catalog's one Eagle station --
/// always available, no range, so it is receivable anywhere on the map.
/// Owner's call (2026-08-31): losing a station must not mean silence. The
/// silent satellite above keeps its job for streamer-safe mode, and for
/// the day the Eagle itself will not open.
pub const AUDIBLE_FALLBACK_STATION_ID: &str = "afn-humphreys";
pub const RADIO_CATALOG_RESOURCE: &str = "radio_catalog.json";
/// How many search hits the Radio app lists. A screen reader walks a list one
/// row at a time, so past this a search is a narrower search, not a longer
/// list; the app says how many more there were.
pub const RADIO_SEARCH_LIMIT: usize = 40;
pub const RADIO_IMPORTED_RESOURCE: &str = "radio_imported.json";
pub const EARTH_RADIUS_MI: f64 = 3958.8;

/// One station on the dial.
#[derive(Debug, Clone, PartialEq)]
pub struct RadioStation {
    pub id: String,
    pub name: String,
    pub call_sign: String,
    pub format: String,
    pub source: String,
    pub source_type: String,
    pub stream_url: String,
    pub stream_format: String,
    pub codec: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub range_miles: f64,
    pub market: String,
    pub region: String,
    pub country: String,
    pub safe_for_streaming: bool,
    pub real_stream: bool,
    pub always_available: bool,
    pub fallback: bool,
    pub supported: bool,
    pub track_key: String,
    /// music.STATION_PLAYLISTS pool for built-in rotation
    pub playlist: String,
    /// music.STATION_HOST_SEGMENTS voice between songs
    pub host: String,
    pub notes: String,
    /// FM physics fields. frequency_mhz drives the picket-fence flutter rate
    /// (2v/lambda); 0.0 means unknown and the mid-band default applies --
    /// wavelength varies only ~10 percent across 88-108, so a default is
    /// honest.
    pub frequency_mhz: f64,
    /// The tower site's ground elevation; None skips the elevation term of
    /// the range model entirely.
    pub site_elev_ft: Option<f64>,
    /// Personal playlist stations only: what the player's playlist file lists,
    /// in playlist order. An entry is either a resolved media file path or an
    /// internet station's URL ([`is_stream_entry`] tells them apart), because a
    /// playlist may hold both and the order is the player's own.
    pub playlist_entries: Vec<String>,
}

impl Default for RadioStation {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            call_sign: String::new(),
            format: String::new(),
            source: String::new(),
            source_type: "local".to_string(),
            stream_url: String::new(),
            stream_format: String::new(),
            codec: String::new(),
            lat: None,
            lon: None,
            range_miles: 0.0,
            market: String::new(),
            region: String::new(),
            country: "US".to_string(),
            safe_for_streaming: true,
            real_stream: false,
            always_available: false,
            fallback: false,
            supported: true,
            track_key: String::new(),
            playlist: String::new(),
            host: String::new(),
            notes: String::new(),
            frequency_mhz: 0.0,
            site_elev_ft: None,
            playlist_entries: Vec::new(),
        }
    }
}

impl RadioStation {
    /// The five positional fields of the Python dataclass; everything else
    /// takes its default (`..RadioStation::new(...)` to override).
    pub fn new(id: &str, name: &str, call_sign: &str, format: &str, source: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            call_sign: call_sign.to_string(),
            format: format.to_string(),
            source: source.to_string(),
            ..Default::default()
        }
    }

    pub fn display_name(&self) -> String {
        // Web stations have no call sign; they are named, not lettered.
        if self.call_sign.is_empty() {
            return self.name.clone();
        }
        // A name that only repeats the call sign is spoken once, not twice.
        if self.name.is_empty() || self.name.to_uppercase() == self.call_sign.to_uppercase() {
            return self.call_sign.clone();
        }
        // A name that already says the call sign ("KXLU 88.9", "Classical
        // KING FM 98.1", "AFN Tokyo") is the station's own branding: leading
        // with the letters again made every seek say them twice.
        if self.name_says_call_sign() {
            return self.name.clone();
        }
        format!("{}, {}", self.call_sign, self.name)
    }

    /// Whether the name carries the call sign, suffix aside (KING for
    /// KING-FM or "KING FM"), as a word of its own: SAT is not in "Satellite".
    fn name_says_call_sign(&self) -> bool {
        let base = self
            .call_sign
            .split(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap_or_default()
            .to_uppercase();
        !base.is_empty()
            && self
                .name
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| word.to_uppercase() == base)
    }

    pub fn satellite(&self) -> bool {
        self.source_type == "afn" || self.source_type == "satellite"
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadioReception {
    pub station: RadioStation,
    pub distance_miles: Option<f64>,
    pub signal: f64,
    pub reason: String,
    pub fallback: bool,
}

impl RadioReception {
    pub fn new(
        station: RadioStation,
        distance_miles: Option<f64>,
        signal: f64,
        reason: &str,
    ) -> Self {
        Self {
            station,
            distance_miles,
            signal,
            reason: reason.to_string(),
            fallback: false,
        }
    }

    pub fn signal_label(&self) -> &'static str {
        if self.fallback {
            return "fallback";
        }
        if self.station.always_available {
            return "always available";
        }
        if self.signal >= 0.8 {
            return "strong signal";
        }
        if self.signal >= 0.45 {
            return "fair signal";
        }
        if self.signal > 0.0 {
            return "fringe signal";
        }
        "out of range"
    }
}

/// Raised when a station cannot play and radio should fall back safely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RadioPlaybackError(pub String);

impl std::fmt::Display for RadioPlaybackError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for RadioPlaybackError {}

/// The playback side the radio state drives: `_DrivingRadioBackend` in the
/// game crate routes a station to a stream, a playlist, the built-in
/// rotation or silence.
pub trait RadioPlaybackBackend {
    fn play_station(
        &mut self,
        station: &RadioStation,
        volume: f64,
    ) -> Result<(), RadioPlaybackError>;
    fn stop_radio(&mut self);
}

#[derive(Debug, Clone, PartialEq)]
pub struct RadioAction {
    pub message: String,
    pub station: RadioStation,
    pub enabled: bool,
    pub reception: RadioReception,
    pub fallback_used: bool,
    /// The station did not answer and a fresh connect has been started in its
    /// place; the message says so. Spoken by the reconnect tick like a
    /// handover, so a silent radio is never a silent mystery.
    pub retried: bool,
}

#[derive(Debug)]
pub enum CatalogError {
    Missing(PathBuf),
    Unreadable(PathBuf, String),
    Empty,
    DuplicateIds,
    ImportedCollision,
}

impl std::fmt::Display for CatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing(path) => {
                write!(f, "{} is missing from this build", path.display())
            }
            Self::Unreadable(path, err) => write!(f, "{}: {err}", path.display()),
            Self::Empty => f.write_str("radio catalog is empty"),
            Self::DuplicateIds => f.write_str("radio catalog contains duplicate station ids"),
            Self::ImportedCollision => {
                f.write_str("imported stations collide with curated station ids")
            }
        }
    }
}

impl std::error::Error for CatalogError {}

/// Python `str(value)` for the JSON values the catalog carries.
fn py_str(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Bool(b)) => (if *b { "True" } else { "False" }).to_string(),
        Some(other) => other.to_string(),
    }
}

/// Python `str(row[key])` with a default when the key is absent.
fn str_or(row: &Value, key: &str, default: &str) -> String {
    match row.get(key) {
        None => default.to_string(),
        some => py_str(some),
    }
}

/// Python `float(value)`; strings that hold a number parse like Python's.
fn py_float(value: &Value) -> Option<f64> {
    match value {
        Value::Number(n) => n.as_f64(),
        Value::String(s) => s.trim().parse::<f64>().ok(),
        Value::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
        _ => None,
    }
}

fn float_or(row: &Value, key: &str, default: f64) -> f64 {
    row.get(key).and_then(py_float).unwrap_or(default)
}

/// Python truthiness of a JSON value.
fn py_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

fn bool_or(row: &Value, key: &str, default: bool) -> bool {
    row.get(key).map(py_truthy).unwrap_or(default)
}

fn optional_float(row: &Value, key: &str) -> Option<f64> {
    match row.get(key) {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) if s.is_empty() => None,
        Some(value) => py_float(value),
    }
}

pub(crate) fn station_from_dict(row: &Value) -> RadioStation {
    RadioStation {
        id: py_str(row.get("id")),
        name: py_str(row.get("name")),
        call_sign: py_str(row.get("call_sign")),
        format: py_str(row.get("format")),
        source: py_str(row.get("source")),
        source_type: str_or(row, "source_type", "local"),
        stream_url: str_or(row, "stream_url", ""),
        stream_format: str_or(row, "stream_format", ""),
        codec: str_or(row, "codec", ""),
        lat: optional_float(row, "lat"),
        lon: optional_float(row, "lon"),
        range_miles: float_or(row, "range_miles", 0.0),
        market: str_or(row, "market", ""),
        region: str_or(row, "region", ""),
        country: str_or(row, "country", "US"),
        safe_for_streaming: bool_or(row, "safe_for_streaming", true),
        real_stream: bool_or(row, "real_stream", false),
        always_available: bool_or(row, "always_available", false),
        fallback: bool_or(row, "fallback", false),
        supported: bool_or(row, "supported", true),
        track_key: str_or(row, "track_key", ""),
        playlist: str_or(row, "playlist", ""),
        host: str_or(row, "host", ""),
        notes: str_or(row, "notes", ""),
        frequency_mhz: float_or(row, "frequency_mhz", 0.0),
        site_elev_ft: optional_float(row, "site_elev_ft"),
        playlist_entries: Vec::new(),
    }
}

fn read_stations(path: &Path) -> Result<Option<Vec<RadioStation>>, CatalogError> {
    // Through the data-resource reader, not `fs` directly: a release ships
    // the baked container instead of the loose catalogs, and the reader is
    // where that fallback lives.
    let Some(text) = crate::data::data_resources::read_text_at(path) else {
        return Ok(None);
    };
    let data: Value = serde_json::from_str(&text)
        .map_err(|e| CatalogError::Unreadable(path.to_path_buf(), e.to_string()))?;
    let rows = data
        .get("stations")
        .and_then(Value::as_array)
        .ok_or_else(|| CatalogError::Unreadable(path.to_path_buf(), "no stations".into()))?;
    Ok(Some(rows.iter().map(station_from_dict).collect()))
}

fn ids_unique(stations: &[RadioStation]) -> bool {
    let mut seen = std::collections::HashSet::with_capacity(stations.len());
    stations.iter().all(|s| seen.insert(s.id.as_str()))
}

/// The curated catalog (`radio_catalog.json`) under `data_root`.
pub fn load_radio_catalog(data_root: &Path) -> Result<Vec<RadioStation>, CatalogError> {
    let path = data_root.join(RADIO_CATALOG_RESOURCE);
    let stations = read_stations(&path)?.ok_or_else(|| CatalogError::Missing(path.clone()))?;
    if stations.is_empty() {
        return Err(CatalogError::Empty);
    }
    if !ids_unique(&stations) {
        return Err(CatalogError::DuplicateIds);
    }
    Ok(stations)
}

/// WNYC-FM, "WNYC FM" and WNYC are one place on the dial.
pub fn call_sign_base(call_sign: &str) -> String {
    if call_sign.trim().is_empty() {
        return String::new();
    }
    call_sign
        .replace('-', " ")
        .split_whitespace()
        .next()
        .map(|first| first.to_uppercase())
        .unwrap_or_default()
}

static URL_SCHEME_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^[a-z][a-z0-9+.\-]*://").unwrap());

// Live365 hands the same station out under its own name and under whichever
// CDN edge answered that day (ais-sa5.cdnstream1.com, das-edge14-live365-
// dal02.cdnstream.com, the legacy edge4.peta.live365.net), and at several
// bitrates off one station id. The mount name carries the id, so the id is
// the station: b09584_128mp3 and b09584_64aac are one station at two
// bitrates, not two stations.
static LIVE365_HOST_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)^(?:streaming\.live365\.com|(?:ais|das)-[\w.-]*\.cdnstream1?\.com|[\w-]+\.peta\.live365\.net)$",
    )
    .unwrap()
});
static LIVE365_MOUNT_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^([ab]\d{4,7})(?:_[0-9a-z]+)?$").unwrap());
const LIVE365_CANONICAL_HOST: &str = "streaming.live365.com";

/// A stream URL with scheme and a trailing slash stripped, for dedup only.
///
/// The same live stream is sometimes registered under both `http://` and
/// `https://` (and sometimes with a bare trailing slash); an exact-string
/// comparison treats those as different streams and let one station land on
/// the dial twice under two names (WHYY 90.9, 2026-08-12 field report).
/// Scheme and host are case-insensitive by spec, so only that part is
/// folded -- a genuinely case-sensitive path is never merged with a
/// different one. Live365 mounts fold further, onto the station id in the
/// mount name: the directory carries the same station under several CDN
/// edge hosts and bitrates, which put Radiostorm's At Work, Oldies and
/// Comedy channels on the web band twice each. A trailing `;` goes with the
/// slash: `host/;` is the Shoutcast mount of the stream `host/` serves to a
/// player, and repairing one directory listing to it while its twin kept
/// the bare root put fifteen stations on the dial twice (sweep, 2026-10-03).
/// Never stored or spoken --
/// comparison only. Shared with tools/import_radio_catalog.py's build-time
/// collision check, so both layers agree on what counts as "the same
/// stream".
pub fn normalize_stream_url(url: &str) -> String {
    let mut url = url.trim();
    if let Some(m) = URL_SCHEME_RE.find(url) {
        url = &url[m.end()..];
    }
    let url = url.trim_end_matches(['/', ';']);
    let (host, rest) = url.split_once('/').unwrap_or((url, ""));
    let host = host.to_lowercase();
    if LIVE365_HOST_RE.is_match(&host) {
        let mount_part = rest.split_once('?').map(|(m, _)| m).unwrap_or(rest);
        if let Some(caps) = LIVE365_MOUNT_RE.captures(mount_part) {
            return format!("{LIVE365_CANONICAL_HOST}/{}", caps[1].to_lowercase());
        }
    }
    if rest.is_empty() {
        host
    } else {
        format!("{host}/{rest}")
    }
}

/// A Live365 stream pointed at Live365's own address, not one CDN edge.
///
/// The directory records whichever edge host answered the day it checked
/// (`ais-edge104-live365-dal02.cdnstream.com`), sometimes carrying the
/// checker's own player and ad-block tokens in the query. Those hostnames
/// come and go; `streaming.live365.com` is the address the station
/// publishes, and it redirects to a live edge at play time. Anything that
/// is not a Live365 mount is returned exactly as it came in.
pub fn canonical_stream_url(url: &str) -> String {
    let stripped = url.trim();
    let rest = match URL_SCHEME_RE.find(stripped) {
        Some(m) => &stripped[m.end()..],
        None => stripped,
    };
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if !LIVE365_HOST_RE.is_match(&host.to_lowercase()) {
        return url.to_string();
    }
    let mount = path
        .split_once('?')
        .map(|(m, _)| m)
        .unwrap_or(path)
        .trim_end_matches('/');
    if !LIVE365_MOUNT_RE.is_match(mount) {
        return url.to_string();
    }
    format!("https://{LIVE365_CANONICAL_HOST}/{mount}")
}

/// The station's identity for dial-listing purposes.
///
/// Curated data can carry the same live stream under several rows on
/// purpose -- real multi-site coverage, one row per transmitter/translator
/// (KZYX Willits/Ukiah, WNPN/ripr Newport/Providence, KENW's three New
/// Mexico sites, SDPB's statewide network). Those rows stay in the data
/// unchanged: each still feeds its own reception physics. But they are one
/// station, not several, so the dial lists them once, at whichever site is
/// strongest from the truck's current position. Two rows sharing a
/// normalized stream URL share an identity; a station with no stream URL
/// (built-in, no real_stream) is never grouped -- its id already is one.
pub fn station_identity(station: &RadioStation) -> String {
    if !station.stream_url.is_empty() {
        return normalize_stream_url(&station.stream_url);
    }
    format!("id:{}", station.id)
}

/// Every station sharing a dial identity with at least one other, keyed
/// by that identity. Solo stations (the overwhelming majority) are left out
/// entirely so every lookup against this map is a cheap membership check.
pub(crate) fn identity_siblings(catalog: &[RadioStation]) -> HashMap<String, Vec<RadioStation>> {
    let mut groups: HashMap<String, Vec<RadioStation>> = HashMap::new();
    for station in catalog {
        groups
            .entry(station_identity(station))
            .or_default()
            .push(station.clone());
    }
    groups.retain(|_, stations| stations.len() > 1);
    groups
}

/// The automated tier under the curated catalog, if this build carries one.
///
/// tools/import_radio_catalog.py drops call-sign collisions when the file is
/// built; the filter here repeats that against the curated catalog actually
/// loaded, so hand-adding a curated station never puts its call sign on the
/// dial twice while the imported file waits for a rebuild.
///
/// "No call sign" is not a call sign to collide with: web stations are
/// named rather than lettered, so the moment the curated catalog grew web
/// entries of its own, an empty string in the reserved set matched every
/// imported web station and emptied the whole band.
pub fn load_imported_stations(
    data_root: &Path,
    curated: &[RadioStation],
) -> Result<Vec<RadioStation>, CatalogError> {
    let Some(imported) = read_stations(&data_root.join(RADIO_IMPORTED_RESOURCE))? else {
        return Ok(Vec::new());
    };
    let reserved: std::collections::HashSet<String> = curated
        .iter()
        .map(|s| call_sign_base(&s.call_sign))
        .filter(|base| !base.is_empty())
        .collect();
    Ok(imported
        .into_iter()
        .filter(|station| !reserved.contains(&call_sign_base(&station.call_sign)))
        .collect())
}

/// Curated plus imported (`DEFAULT_RADIO_CATALOG` in Python), plus the
/// accepted community stations this build ships.
pub fn load_full_catalog(data_root: &Path) -> Result<Vec<RadioStation>, CatalogError> {
    let curated = load_radio_catalog(data_root)?;
    let imported = load_imported_stations(data_root, &curated)?;
    let mut stations = curated;
    stations.extend(imported);
    let community = community::load_shipped_community_stations(data_root, &stations);
    stations.extend(community);
    if !ids_unique(&stations) {
        return Err(CatalogError::ImportedCollision);
    }
    Ok(stations)
}

/// Where the shipped data tree lives: the same root every other data
/// loader uses (`FREIGHT_FATE_DATA_ROOT` override, `<exe dir>/freight_fate/data`
/// in a frozen build, the repo's `data/` from source).
/// `FREIGHT_FATE_DATA_DIR` is the *saves* directory and must not be read
/// here: tests that point it at a temp dir broke every radio test.
pub fn default_data_root() -> PathBuf {
    crate::data::data_resources::data_root().to_path_buf()
}

static DEFAULT_CATALOG: Lazy<Vec<RadioStation>> = Lazy::new(|| {
    load_full_catalog(&default_data_root()).unwrap_or_else(|err| panic!("radio catalog: {err}"))
});

/// The shipped catalog, curated plus imported, loaded once from
/// [`default_data_root`]. Panics if the build has no catalog, as the Python
/// import did.
pub fn default_radio_catalog() -> &'static [RadioStation] {
    &DEFAULT_CATALOG
}

/// Dial order and category identity, shared by sort and category jump.
pub fn dial_group(station: &RadioStation) -> i32 {
    if station.id == SAFE_ROUTE_PLAYLIST {
        return 0;
    }
    if station.source_type == "built_in" {
        return 1;
    }
    if station.source_type == PERSONAL_PLAYLIST_SOURCE_TYPE {
        return 2;
    }
    if station.fallback {
        return 8;
    }
    // Freight Fate's own music stations (playlist-backed, no stream) sit with
    // Roadhouse and Night Line: always on the dial, everywhere, in every mode.
    if !station.playlist.is_empty() && !station.real_stream {
        return 1;
    }
    if matches!(
        station.source_type.as_str(),
        "local" | "regional" | "imported"
    ) {
        return TERRESTRIAL_GROUP;
    }
    if station.source_type == "afn" {
        return 5;
    }
    if station.source_type == "satellite" {
        return 6;
    }
    if station.source_type == "international" {
        return 7;
    }
    // Web radio sits last on the dial, past everything with a place or a
    // story: thousands of stations, in listener-vote order (the dial sort is
    // stable and their call signs are empty), one category jump to skip.
    if station.source_type == "web" {
        return 9;
    }
    10
}

pub const DIAL_CATEGORY_NAMES: &[(i32, &str)] = &[
    (0, "Route playlist"),
    (1, "Freight Fate stations"),
    (2, "Your playlists"),
    (3, "Favorites"),
    (4, "Terrestrial"),
    (5, "AFN"),
    (6, "Satellite"),
    (7, "International"),
    (8, "Fallback"),
    (9, "Web radio"),
    (10, "Other stations"),
];

/// `DIAL_CATEGORY_NAMES.get(group, "Radio")`.
pub fn dial_category_name(group: i32) -> &'static str {
    DIAL_CATEGORY_NAMES
        .iter()
        .find(|(g, _)| *g == group)
        .map(|(_, name)| *name)
        .unwrap_or("Radio")
}

pub fn station_distance_miles(station: &RadioStation, position: Option<(f64, f64)>) -> Option<f64> {
    let (plat, plon) = position?;
    let (slat, slon) = (station.lat?, station.lon?);
    let (lat1, lon1) = (plat.to_radians(), plon.to_radians());
    let (lat2, lon2) = (slat.to_radians(), slon.to_radians());
    let dlat = lat2 - lat1;
    let dlon = lon2 - lon1;
    let a = (dlat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    Some(EARTH_RADIUS_MI * 2.0 * a.sqrt().atan2((1.0 - a).sqrt()))
}

// FM is line-of-sight, and height is range: the 4/3-earth radio horizon is
// d_miles ~ 1.23 * sqrt(height_ft). Standing above a station's tower site
// extends its contour by that term -- the owner's ham anchor: from the
// Mogollon Rim (~7000 ft) both Phoenix (~1100 ft) and Flagstaff come in
// clearly at distances a flat radius refuses. Sitting BELOW the site is
// NEUTRAL, never a penalty: a mountain-top transmitter looks straight down
// into its valley (that height is why they put it there), and the published
// range already assumes ordinary low receivers. True canyon shadowing needs
// a path profile we do not carry -- roadmap follow-up, not a proxy that
// would punish every in-market listener under a mountain site.
pub const RADIO_HORIZON_MI_PER_SQRT_FT: f64 = 1.23;

// Compression compensation for ranged stations (owner design 2026-08-13):
// the truck spends road miles 10-40x faster than a real cab, so a real
// 40-mile FM contour was two real minutes of program. Doubling the reach
// keeps radio regional (no station spans three states) while a median
// station now survives about seven real minutes at Relaxed. Applied in
// reach_mi so every consumer -- range check, signal curve, elevation
// lift -- agrees on one number.
pub const RADIO_REACH_MULT: f64 = 2.0;

// The ceiling that keeps the doubling honest (owner, 2026-08-18). A handful
// of big curated stations carry 90-175 mile base ranges, and doubling those
// put Houston, Tulsa, Austin and Oklahoma City on the Dallas dial from
// 200-240 miles out -- the three-state span RADIO_REACH_MULT's own note says
// it exists to prevent. Half the terrestrial band at a metro was fringe the
// driver had to seek past to reach the stations actually worth hearing.
// 150 rather than something tighter because 120 starts emptying rural dials,
// and a thin dial is exactly where a distant station earns its place.
pub const RADIO_MAX_REACH_MI: f64 = 150.0;

pub fn reach_mi(station: &RadioStation) -> f64 {
    (station.range_miles * RADIO_REACH_MULT).min(RADIO_MAX_REACH_MI)
}

/// How far this station reaches from the truck's current ground height.
///
/// High ground still receives far past the flat contour -- that is the
/// owner's ham anchor and it survives the cap: the elevation lift is added
/// on top, so a rim at 7000 feet hears a station the flats cannot. What it
/// no longer does is compound with a doubled 175-mile range.
pub fn effective_range_miles(station: &RadioStation, elevation_ft: Option<f64>) -> f64 {
    let (Some(elevation_ft), Some(site_elev_ft)) = (elevation_ft, station.site_elev_ft) else {
        return reach_mi(station);
    };
    if station.range_miles <= 0.0 {
        return reach_mi(station);
    }
    let lift = (elevation_ft - site_elev_ft).max(0.0);
    reach_mi(station) + RADIO_HORIZON_MI_PER_SQRT_FT * lift.sqrt()
}

pub fn estimate_signal(
    station: &RadioStation,
    position: Option<(f64, f64)>,
    elevation_ft: Option<f64>,
) -> RadioReception {
    if station.always_available {
        return RadioReception::new(station.clone(), None, 1.0, "always available");
    }
    if station.range_miles <= 0.0 {
        return RadioReception::new(station.clone(), None, 1.0, "built-in");
    }
    let Some(distance) = station_distance_miles(station, position) else {
        return RadioReception::new(station.clone(), None, 0.0, "no truck position");
    };
    let range_miles = effective_range_miles(station, elevation_ft);
    if distance > range_miles {
        return RadioReception::new(station.clone(), Some(distance), 0.0, "out of range");
    }
    // Signal is intentionally simple and monotonic. Future FCC contours can
    // replace range_miles without changing the state/menu layer.
    let signal = (1.0 - (distance / range_miles).powf(1.4)).max(0.05);
    RadioReception::new(station.clone(), Some(distance), signal, "in range")
}

// Below this signal the audio starts to thin out. Entering the fringe the
// program holds a listenable level (worth chasing toward its city); past the
// static threshold it keeps sinking toward the deep floor while the noise
// rises to take its place -- the owner's ruling (2026-07-24): the two smear
// together, static going TO program level, never bombarding on top of a
// still-loud program. The deep floor keeps a trace of program in the noise
// while the station is technically in range. Retuned for the doubled reach
// (2026-08-13): with the contour twice as wide, the old 60%-of-range full-
// volume cutoff spent most of a station's new reach visibly fading, when
// the point of RADIO_REACH_MULT was more clean program, not more fringe.
// Clean program now holds through ~85% of the contour and static is pushed
// out to the outer edge, where it belongs.
pub const SIGNAL_FULL_VOLUME: f64 = 0.20; // clean program through ~85% of the contour
pub const SIGNAL_FRINGE_FLOOR: f64 = 0.3;
pub const SIGNAL_DEEP_FLOOR: f64 = 0.08;
pub const STATIC_SIGNAL_THRESHOLD: f64 = 0.12; // static smear lives in the outer edge only

/// How much of the radio volume the current signal supports.
///
/// Satellite/built-in sources always play at full volume. Ranged stations
/// hold full volume through most of their contour, fade toward the fringe
/// floor as the truck drives away, keep sinking under the rising static in
/// the deep fringe, and go silent past the range edge.
pub fn signal_volume_factor(reception: &RadioReception) -> f64 {
    let station = &reception.station;
    if reception.fallback || station.always_available || station.range_miles <= 0.0 {
        return 1.0;
    }
    let signal = reception.signal;
    if signal <= 0.0 {
        return 0.0;
    }
    if signal >= SIGNAL_FULL_VOLUME {
        return 1.0;
    }
    if signal >= STATIC_SIGNAL_THRESHOLD {
        return SIGNAL_FRINGE_FLOOR + (1.0 - SIGNAL_FRINGE_FLOOR) * (signal / SIGNAL_FULL_VOLUME);
    }
    let edge = SIGNAL_FRINGE_FLOOR
        + (1.0 - SIGNAL_FRINGE_FLOOR) * (STATIC_SIGNAL_THRESHOLD / SIGNAL_FULL_VOLUME);
    (edge * (signal / STATIC_SIGNAL_THRESHOLD).powf(0.8)).max(SIGNAL_DEEP_FLOOR)
}

/// One checked-in point of a leg's geometry, leg-local mileposts in the
/// a->b direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoutePoint {
    pub lat: f64,
    pub lon: f64,
    pub at_mi: f64,
}

/// One elevation sample of a leg's profile, leg-local mileposts a->b.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ElevationSample {
    pub at_mi: f64,
    pub elevation_ft: f64,
}

/// The slice of a route the radio reads to place the truck. The Python
/// functions duck-typed `route.legs[i].miles / .a / .route_points /
/// .elevation_samples` and `route.cities`; the data module's route type
/// implements this.
pub trait RadioRoute {
    fn leg_count(&self) -> usize;
    /// `route.cities[index]`.
    fn city(&self, index: usize) -> String;
    fn leg_miles(&self, index: usize) -> f64;
    /// `leg.a`: the city the leg's mileposts count from.
    fn leg_a(&self, index: usize) -> String;
    fn leg_route_points(&self, index: usize) -> Vec<RoutePoint>;
    fn leg_elevation_samples(&self, index: usize) -> Vec<ElevationSample>;
}

/// Approximate current lat/lon from checked-in route/city coordinates.
/// `city_latlon` is the world's city lookup (`world.cities.get(name)`).
pub fn truck_position(
    route: Option<&dyn RadioRoute>,
    position_mi: f64,
    city_latlon: &dyn Fn(&str) -> Option<(f64, f64)>,
) -> Option<(f64, f64)> {
    let route = route?;
    let count = route.leg_count();
    if count == 0 {
        return None;
    }
    let mut remaining = position_mi.max(0.0);
    for i in 0..count {
        let leg_miles = route.leg_miles(i).max(0.0);
        if remaining <= leg_miles || i == count - 1 {
            let local = leg_miles.min(remaining);
            return leg_position(route, i, local, city_latlon);
        }
        remaining -= leg_miles;
    }
    None
}

/// Current ground elevation from the leg's elevation samples, if any.
pub fn truck_elevation_ft(route: Option<&dyn RadioRoute>, position_mi: f64) -> Option<f64> {
    let route = route?;
    let count = route.leg_count();
    if count == 0 {
        return None;
    }
    let mut remaining = position_mi.max(0.0);
    for i in 0..count {
        let leg_miles = route.leg_miles(i).max(0.0);
        if remaining <= leg_miles || i == count - 1 {
            let local = leg_miles.min(remaining);
            return leg_elevation(route, i, local);
        }
        remaining -= leg_miles;
    }
    None
}

fn leg_elevation(route: &dyn RadioRoute, index: usize, local_mi: f64) -> Option<f64> {
    let samples = route.leg_elevation_samples(index);
    if samples.is_empty() {
        return None;
    }
    let total = route.leg_miles(index).max(0.01);
    let forward = route.city(index) == route.leg_a(index);
    // Sample mileposts are leg-local in the a->b direction; a reversed
    // traversal reads the profile from the far end, same as route points.
    let at = if forward { local_mi } else { total - local_mi };
    if samples.len() == 1 || at <= samples[0].at_mi {
        return Some(samples[0].elevation_ft);
    }
    for pair in samples.windows(2) {
        let (prev, cur) = (pair[0], pair[1]);
        if at <= cur.at_mi {
            let span = (cur.at_mi - prev.at_mi).max(0.01);
            let t = ((at - prev.at_mi) / span).clamp(0.0, 1.0);
            return Some(prev.elevation_ft + (cur.elevation_ft - prev.elevation_ft) * t);
        }
    }
    Some(samples[samples.len() - 1].elevation_ft)
}

fn leg_position(
    route: &dyn RadioRoute,
    index: usize,
    local_mi: f64,
    city_latlon: &dyn Fn(&str) -> Option<(f64, f64)>,
) -> Option<(f64, f64)> {
    let points = route.leg_route_points(index);
    let forward = route.city(index) == route.leg_a(index);
    if !points.is_empty() {
        let ordered: Vec<RoutePoint> = if forward {
            points
        } else {
            points.into_iter().rev().collect()
        };
        let total = route.leg_miles(index).max(0.01);
        if ordered.len() == 1 {
            return Some((ordered[0].lat, ordered[0].lon));
        }
        let mut last = ordered[0];
        for point in &ordered[1..] {
            let mut start = if forward {
                last.at_mi
            } else {
                total - last.at_mi
            };
            let mut end = if forward {
                point.at_mi
            } else {
                total - point.at_mi
            };
            if end < start {
                std::mem::swap(&mut start, &mut end);
            }
            if start <= local_mi && local_mi <= end {
                let span = (end - start).max(0.01);
                let t = ((local_mi - start) / span).clamp(0.0, 1.0);
                return Some((
                    last.lat + (point.lat - last.lat) * t,
                    last.lon + (point.lon - last.lon) * t,
                ));
            }
            last = *point;
        }
    }
    let a = city_latlon(&route.city(index))?;
    let b = city_latlon(&route.city(index + 1))?;
    let miles = route.leg_miles(index);
    let t = if miles <= 0.0 {
        0.0
    } else {
        (local_mi / miles).clamp(0.0, 1.0)
    };
    Some((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t))
}
