//! Mutable in-cab radio state: the dial, the streamer-safe gate, favourites,
//! multi-site handover and the two-strike fallback.
//!
//! The `RadioState` half of `freight_fate/radio.py`.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use indexmap::IndexMap;

use super::playlists::{load_personal_playlists, PERSONAL_PLAYLIST_SOURCE_TYPE};
use super::{
    dial_category_name, dial_group, estimate_signal, identity_siblings, station_distance_miles,
    station_identity, RadioAction, RadioPlaybackBackend, RadioReception, RadioStation,
    SAFE_ROUTE_PLAYLIST, SIGNAL_FULL_VOLUME, STATIC_SIGNAL_THRESHOLD,
};
use crate::pyfmt::{fmt_f, round_py_int};

/// Saved stations pull forward to this dial group ([`RadioState::group`]): with
/// thousands of stations on the dial, the driver's own picks sit right after
/// their playlists, one category jump from anywhere.
pub const FAVORITES_GROUP: i32 = 3;
/// Real ranged stations. Unlike every other group, order here is signal, not
/// call sign: a category jump at the start of a run should land on the
/// station that plays clean, not on whichever fringe call sign sorts first.
pub const TERRESTRIAL_GROUP: i32 = 4;

/// The radio fields of the settings file, as `RadioState.from_settings` /
/// `apply_settings` / `write_settings` read and write them. The `settings`
/// port implements this.
pub trait RadioSettingsAccess {
    fn radio_enabled(&self) -> bool;
    fn radio_station_id(&self) -> String;
    fn radio_volume(&self) -> f64;
    fn radio_streamer_safe(&self) -> bool;
    /// Music source Synthesized.
    fn synth_music(&self) -> bool;
    fn set_radio_enabled(&mut self, enabled: bool);
    fn set_radio_station_id(&mut self, station_id: &str);
}

/// One search or favourites hit: the station and its reception here
/// (`None` when out of range).
pub type StationHit = (RadioStation, Option<RadioReception>);

/// Mutable in-cab radio state. Streamer-safe mode is the one licensing
/// gate: off by default (owner ruling, 2026-08-12), so the full dial plays
/// out of the box, and turning it on is the explicit choice a streamer
/// makes to keep licensed audio off their broadcast.
#[derive(Debug, Clone)]
pub struct RadioState {
    /// The dial's stations. Replace through [`RadioState::set_catalog`] so the
    /// multi-site identity map follows it.
    pub catalog: Vec<RadioStation>,
    pub enabled: bool,
    pub station_id: String,
    pub volume: f64,
    pub streamer_safe: bool,
    /// Music source Synthesized: see `synth_dial.rs`.
    pub synth_music: bool,
    pub position: Option<(f64, f64)>,
    pub elevation_ft: Option<f64>,
    pub favorite_ids: HashSet<String>,
    /// Stations that refused to play this session: off the dial until the
    /// next session rather than a dead stop on every pass of the band.
    pub unplayable_ids: HashSet<String>,
    /// Connects that failed this session, by station id. One failure is a
    /// slow server or a dropped packet, not a dead station: the ban above
    /// waits for a second (owner, 2026-08-22, Darren Duff radio written
    /// off while it was still answering).
    connect_failures: HashMap<String, u32>,
    /// Multi-site stations (KZYX, WNPN/ripr, KENW, SDPB...): every row
    /// sharing a normalized stream URL, keyed by that identity. Built
    /// once per catalog -- the catalog itself never changes after
    /// construction -- so tuning and reception lookups stay O(sites),
    /// not O(catalog), on every call.
    identity_siblings: HashMap<String, Vec<RadioStation>>,
    /// The dial order the current sweep is stepping through, and where the
    /// truck was when it was built. The terrestrial band runs strongest
    /// first, and a moving truck's signals drift between presses, so an
    /// order rebuilt on every step revisited one station and skipped
    /// another (WRND twice in nine presses, 2026-09-16). A sweep keeps its
    /// order until the truck has moved `DIAL_SWEEP_MI`, the radio is
    /// switched, or a category jump starts a new one.
    sweep_order: Vec<String>,
    sweep_anchor: Option<(f64, f64)>,
}

/// How far the truck moves before a dial sweep's order is rebuilt.
pub const DIAL_SWEEP_MI: f64 = 2.0;

impl RadioState {
    /// A radio over `catalog` with the Python defaults: on, tuned to the
    /// route playlist, volume 0.25, streamer-safe off, no position, no
    /// favourites. Chain the `with_*` setters for the keyword arguments.
    pub fn new(catalog: Vec<RadioStation>) -> Self {
        let identity_siblings = identity_siblings(&catalog);
        Self {
            catalog,
            enabled: true,
            station_id: SAFE_ROUTE_PLAYLIST.to_string(),
            volume: 0.25,
            streamer_safe: false,
            synth_music: false,
            position: None,
            elevation_ft: None,
            favorite_ids: HashSet::new(),
            unplayable_ids: HashSet::new(),
            connect_failures: HashMap::new(),
            identity_siblings,
            sweep_order: Vec::new(),
            sweep_anchor: None,
        }
    }

    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    pub fn with_station_id(mut self, station_id: &str) -> Self {
        self.station_id = station_id.to_string();
        self
    }

    pub fn with_volume(mut self, volume: f64) -> Self {
        self.volume = Self::clamp_volume(volume);
        self
    }

    pub fn with_streamer_safe(mut self, streamer_safe: bool) -> Self {
        self.streamer_safe = streamer_safe;
        self
    }

    pub fn with_position(mut self, position: Option<(f64, f64)>) -> Self {
        self.position = position;
        self
    }

    pub fn with_favorites(mut self, favorite_ids: &[String]) -> Self {
        self.favorite_ids = favorite_ids.iter().cloned().collect();
        self
    }

    /// `RadioState.from_settings(settings, profile)`: the catalog (shipped
    /// plus personal playlists) comes from the caller, the favourites are
    /// the profile's `radio_favorites`.
    pub fn from_settings(
        catalog: Vec<RadioStation>,
        settings: &dyn RadioSettingsAccess,
        favorites: &[String],
    ) -> Self {
        Self::new(catalog)
            .with_enabled(settings.radio_enabled())
            .with_station_id(&settings.radio_station_id())
            .with_volume(settings.radio_volume())
            .with_streamer_safe(settings.radio_streamer_safe())
            .with_synth_music(settings.synth_music())
            .with_favorites(favorites)
    }

    /// Replace the dial and rebuild the multi-site identity map.
    pub fn set_catalog(&mut self, catalog: Vec<RadioStation>) {
        self.identity_siblings = identity_siblings(&catalog);
        self.catalog = catalog;
    }

    /// Re-read the Playlists folder so the dial matches it right now.
    ///
    /// Playlists used to be read once, when the drive began: a player who
    /// fixed a playlist file mid-run had to start another drive before the
    /// radio would even look again. The dial screen is the cheap place to
    /// re-read -- it opens rarely, and it is exactly where a player goes
    /// when their playlist is missing from the dial.
    pub fn reload_personal_playlists(&mut self, directory: &Path) {
        let kept: Vec<RadioStation> = self
            .catalog
            .iter()
            .filter(|s| s.source_type != PERSONAL_PLAYLIST_SOURCE_TYPE)
            .cloned()
            .collect();
        let kept_ids: HashSet<&str> = kept.iter().map(|s| s.id.as_str()).collect();
        let stale: HashSet<String> = self
            .catalog
            .iter()
            .filter(|s| !kept_ids.contains(s.id.as_str()))
            .map(|s| s.id.clone())
            .collect();
        let loaded = load_personal_playlists(directory);
        let loaded_ids: HashSet<String> = loaded.iter().map(|s| s.id.clone()).collect();
        let mut catalog = kept;
        catalog.extend(loaded);
        self.set_catalog(catalog);
        // A playlist that would not play earlier deserves another chance once
        // its file has been edited; the session-long ban is for dead streams.
        for station_id in stale.iter().chain(loaded_ids.iter()) {
            self.unplayable_ids.remove(station_id);
            self.connect_failures.remove(station_id);
        }
    }

    pub fn apply_settings(&mut self, settings: &dyn RadioSettingsAccess) {
        self.volume = Self::clamp_volume(settings.radio_volume());
        self.streamer_safe = settings.radio_streamer_safe();
        self.synth_music = settings.synth_music();
    }

    pub fn write_settings(&self, settings: &mut dyn RadioSettingsAccess) {
        settings.set_radio_enabled(self.enabled);
        settings.set_radio_station_id(&self.station_id);
    }

    pub fn update_position(&mut self, position: Option<(f64, f64)>, elevation_ft: Option<f64>) {
        self.position = position;
        self.elevation_ft = elevation_ft;
    }

    pub fn receivable_stations(&self) -> Vec<RadioReception> {
        let receptions = self
            .catalog
            .iter()
            .filter(|s| self.station_allowed(s))
            .map(|s| self.reception(s));
        let receivable: Vec<RadioReception> = receptions
            .filter(|r| r.signal > 0.0 || r.station.always_available)
            .collect();
        let mut receivable = self.collapse_identity_sites(receivable);
        receivable.sort_by(|a, b| {
            let (ga, sa, ca) = self.reception_sort_key(a);
            let (gb, sb, cb) = self.reception_sort_key(b);
            ga.cmp(&gb)
                .then(sa.partial_cmp(&sb).unwrap_or(std::cmp::Ordering::Equal))
                .then_with(|| ca.cmp(&cb))
        });
        if receivable.is_empty() {
            vec![self.fallback_reception()]
        } else {
            receivable
        }
    }

    /// One dial entry per multi-site identity: whichever site is loudest.
    ///
    /// KZYX/WNPN/KENW/SDPB-style entries are several real transmitters
    /// broadcasting the same stream; each keeps feeding its own reception
    /// physics, but only the strongest one currently in range gets a line
    /// on the dial -- the rest would just repeat it under a different name.
    fn collapse_identity_sites(&self, receptions: Vec<RadioReception>) -> Vec<RadioReception> {
        if self.identity_siblings.is_empty() {
            return receptions;
        }
        let mut solo: Vec<RadioReception> = Vec::new();
        let mut best: IndexMap<String, RadioReception> = IndexMap::new();
        for reception in receptions {
            let identity = station_identity(&reception.station);
            if !self.identity_siblings.contains_key(&identity) {
                solo.push(reception);
                continue;
            }
            match best.get(&identity) {
                None => {
                    best.insert(identity, reception);
                }
                Some(current) if reception.signal > current.signal => {
                    best.insert(identity, reception);
                }
                Some(_) => {}
            }
        }
        solo.extend(best.into_values());
        solo
    }

    /// How `station` comes in here.
    ///
    /// A favorite plays from anywhere (Orinks, 2026-10-12): a driver who
    /// saved a station away from home still wants it once its signal is
    /// gone. A saved ranged station keeps its real signal while that signal
    /// is clean, and past that point the radio plays the station's stream
    /// at full strength, so a favorite never fades, crackles or drops out.
    pub fn reception(&self, station: &RadioStation) -> RadioReception {
        let physical = estimate_signal(station, self.position, self.elevation_ft);
        if physical.signal >= SIGNAL_FULL_VOLUME || !self.plays_anywhere(station) {
            return physical;
        }
        RadioReception {
            station: station.clone(),
            distance_miles: None,
            signal: 1.0,
            reason: "favorite".to_string(),
            fallback: false,
            streamed: true,
        }
    }

    /// A saved station with a coverage area: one [`RadioState::reception`]
    /// may stream.
    fn plays_anywhere(&self, station: &RadioStation) -> bool {
        station.range_miles > 0.0
            && !station.always_available
            && !self.favorite_ids.is_empty()
            && self.is_favorite(station)
    }

    /// Where the dial goes when `lost` drops out of range: the strongest
    /// terrestrial station the truck can hear from here, or `None` when there
    /// is nothing on the air (Brandon, 2026-09-17: a driver listening to
    /// local radio wants the next town's station, not to be sent back to the
    /// in-house ones).
    ///
    /// Only a clean signal counts, and a streamed favorite is not a signal
    /// here at all. Below [`STATIC_SIGNAL_THRESHOLD`] a station
    /// is the static smear at the edge of its own contour, and landing there
    /// would trade one fading station for another that fades a few miles on.
    /// A sibling site of the lost station is not a landing either: a stronger
    /// site of the same station is a handover, which the dial already does.
    pub fn strongest_terrestrial(&self, lost: &RadioStation) -> Option<RadioReception> {
        let lost_identity = station_identity(lost);
        self.receivable_stations()
            .into_iter()
            .filter(|r| dial_group(&r.station) == TERRESTRIAL_GROUP && !r.streamed)
            .filter(|r| r.signal >= STATIC_SIGNAL_THRESHOLD)
            .filter(|r| r.station.id != lost.id && station_identity(&r.station) != lost_identity)
            .max_by(|a, b| {
                a.signal
                    .partial_cmp(&b.signal)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    pub fn available_stations(&self) -> Vec<RadioStation> {
        self.receivable_stations()
            .into_iter()
            .map(|r| r.station)
            .collect()
    }

    fn hit_order(a: &StationHit, b: &StationHit) -> std::cmp::Ordering {
        let none_a = a.1.is_none();
        let none_b = b.1.is_none();
        let sig_a = -a.1.as_ref().map(|r| r.signal).unwrap_or(0.0);
        let sig_b = -b.1.as_ref().map(|r| r.signal).unwrap_or(0.0);
        none_a
            .cmp(&none_b)
            .then(
                sig_a
                    .partial_cmp(&sig_b)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then_with(|| {
                a.0.display_name()
                    .to_lowercase()
                    .cmp(&b.0.display_name().to_lowercase())
            })
    }

    /// Stations on the whole dial whose name, call sign, or format
    /// contains `query`: `(hits, total)`, hits capped at `limit`.
    ///
    /// The whole allowed dial, not only what is in range -- a driver
    /// searching for a web station they heard about should find it from
    /// anywhere, and a terrestrial one they cannot get yet is still worth
    /// knowing about. In-range hits come first, strongest signal first;
    /// the rest follow by name, each paired with None. Streamer-safe mode
    /// and the session's dead-stream ban apply exactly as they do to the
    /// dial, and a multi-site station is one hit, its best site.
    pub fn search(&self, query: &str, limit: usize) -> (Vec<StationHit>, usize) {
        let needle = query
            .to_lowercase()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if needle.is_empty() {
            return (Vec::new(), 0);
        }
        let receivable: HashMap<String, RadioReception> = self
            .receivable_stations()
            .into_iter()
            .map(|r| (r.station.id.clone(), r))
            .collect();
        let mut hits: IndexMap<String, StationHit> = IndexMap::new();
        for station in &self.catalog {
            if !self.station_allowed(station) {
                continue;
            }
            let haystack = format!(
                "{} {} {} {}",
                station.display_name(),
                station.name,
                station.call_sign,
                station.format
            )
            .to_lowercase();
            if !haystack.contains(&needle) {
                continue;
            }
            let identity = station_identity(station);
            let reception = receivable.get(&station.id).cloned();
            let replace = match hits.get(&identity) {
                None => true,
                Some((_, kept)) => reception.is_some() && kept.is_none(),
            };
            if replace {
                hits.insert(identity, (station.clone(), reception));
            }
        }
        let mut ordered: Vec<StationHit> = hits.into_values().collect();
        ordered.sort_by(Self::hit_order);
        let total = ordered.len();
        ordered.truncate(limit);
        (ordered, total)
    }

    /// The dial categories the Radio app can browse, in dial order, each with
    /// how many distinct stations it holds: `(group, name, count)`. Only
    /// categories with at least one allowed station appear. Favorites and the
    /// route playlist are not here; favorites has its own list.
    pub fn browse_categories(&self) -> Vec<(i32, &'static str, usize)> {
        let mut counts: std::collections::BTreeMap<i32, std::collections::HashSet<String>> =
            std::collections::BTreeMap::new();
        for station in &self.catalog {
            if !self.station_allowed(station) {
                continue;
            }
            let group = dial_group(station);
            if group == 0 || group == FAVORITES_GROUP {
                continue;
            }
            counts
                .entry(group)
                .or_default()
                .insert(station_identity(station));
        }
        counts
            .into_iter()
            .map(|(group, ids)| (group, dial_category_name(group), ids.len()))
            .collect()
    }

    /// Every allowed station, or only one dial category's (`Some(group)`),
    /// each paired with its reception here (None when out of range). A
    /// multi-site station is one row, its best site. In-range stations come
    /// first, then out-of-range ones; inside each, dial category order puts
    /// local terrestrial ahead of web radio, then strongest signal, then name.
    pub fn browse(&self, group: Option<i32>) -> Vec<StationHit> {
        let receivable: HashMap<String, RadioReception> = self
            .receivable_stations()
            .into_iter()
            .map(|r| (r.station.id.clone(), r))
            .collect();
        let mut seen: IndexMap<String, StationHit> = IndexMap::new();
        for station in &self.catalog {
            if !self.station_allowed(station) {
                continue;
            }
            let station_group = dial_group(station);
            if station_group == 0 && group.is_none() {
                continue;
            }
            if group.is_some_and(|g| g != station_group) {
                continue;
            }
            let identity = station_identity(station);
            let reception = receivable.get(&station.id).cloned();
            let replace = match seen.get(&identity) {
                None => true,
                Some((_, kept)) => reception.is_some() && kept.is_none(),
            };
            if replace {
                seen.insert(identity, (station.clone(), reception));
            }
        }
        let mut ordered: Vec<StationHit> = seen.into_values().collect();
        ordered.sort_by(|a, b| {
            a.1.is_none()
                .cmp(&b.1.is_none())
                .then(dial_group(&a.0).cmp(&dial_group(&b.0)))
                .then_with(|| Self::hit_order(a, b))
        });
        ordered
    }

    /// Saved stations, each with its reception here (None when out of
    /// range), in-range first, then by name -- the same shape as search.
    pub fn favorites(&self) -> Vec<StationHit> {
        let receivable: HashMap<String, RadioReception> = self
            .receivable_stations()
            .into_iter()
            .map(|r| (r.station.id.clone(), r))
            .collect();
        let mut seen: IndexMap<String, StationHit> = IndexMap::new();
        for station in &self.catalog {
            if !self.favorite_ids.contains(&station.id) || !self.station_allowed(station) {
                continue;
            }
            let identity = station_identity(station);
            let reception = receivable.get(&station.id).cloned();
            let replace = match seen.get(&identity) {
                None => true,
                Some((_, kept)) => reception.is_some() && kept.is_none(),
            };
            if replace {
                seen.insert(identity, (station.clone(), reception));
            }
        }
        let mut ordered: Vec<StationHit> = seen.into_values().collect();
        ordered.sort_by(Self::hit_order);
        ordered
    }

    pub fn is_favorite(&self, station: &RadioStation) -> bool {
        if self.favorite_ids.contains(&station.id) {
            return true;
        }
        if self.identity_siblings.is_empty() {
            return false;
        }
        self.identity_siblings
            .get(&station_identity(station))
            .is_some_and(|sites| sites.iter().any(|s| self.favorite_ids.contains(&s.id)))
    }

    /// The dial category a station sits in, as the dial speaks it.
    pub fn band_name(&self, station: &RadioStation) -> &'static str {
        dial_category_name(dial_group(station))
    }

    pub fn station_list_lines(
        &mut self,
        limit: usize,
        distance_text: Option<&dyn Fn(f64) -> String>,
    ) -> Vec<String> {
        let current_id = self.current_station().id;
        let mut lines = Vec::new();
        for reception in self.receivable_stations().into_iter().take(limit) {
            let station = &reception.station;
            let selected = if station.id == current_id {
                "current, "
            } else {
                ""
            };
            let mut distance = String::new();
            if let Some(miles) = reception.distance_miles {
                let spoken = match distance_text {
                    Some(f) => f(miles),
                    None => format!("{} miles", fmt_f(miles, 0)),
                };
                distance = format!(", {spoken} away");
            }
            lines.push(format!(
                "{selected}{}: {}, {}{distance}. Source: {}.",
                station.display_name(),
                station.format,
                reception.signal_label(),
                station.source
            ));
        }
        lines
    }

    pub fn current_station(&mut self) -> RadioStation {
        let station = self.tuned_station();
        // The write-back: a sibling handover or a fallback re-points the
        // dial, and on the ordinary path this assigns the id it already
        // holds. That is the whole reason this call needs `&mut self`.
        self.station_id = station.id.clone();
        station
    }

    /// Which station the dial is on RIGHT NOW, without re-pointing it.
    ///
    /// Same answer as [`RadioState::current_station`]; it just does not
    /// persist the handover or the fallback, so a read that only wants the
    /// station's name does not need `&mut`.
    ///
    /// It exists because the alternative callers reached for was
    /// `radio.clone().current_station()` -- a deep copy of the whole dial
    /// (757 stations plus the identity map) to throw away one field's
    /// write. In the online-presence builder, which `App::tick` runs every
    /// frame, that clone was 2.4 ms per frame on a mountain drive: 97% of
    /// the entire frame, and fourteen per cent of the 60 Hz budget spent
    /// copying a catalog nobody read. See
    /// `crates/freight-fate/tests/frame_time/`.
    pub fn tuned_station(&self) -> RadioStation {
        if let Some(station) = self.station_by_id(&self.station_id).cloned() {
            if self.station_allowed(&station) {
                let reception = estimate_signal(&station, self.position, self.elevation_ft);
                if reception.signal > 0.0
                    || station.always_available
                    || self.plays_anywhere(&station)
                {
                    if let Some(handover) = self.identity_handover(&station, reception.signal) {
                        return handover;
                    }
                    return station;
                }
            }
        }
        self.fallback_station()
    }

    /// The stronger sibling site for a multi-site station, if one beats it now.
    ///
    /// Tuning in on KZYX/WNPN/KENW/SDPB-style entries points station_id at
    /// one literal site's id; as the truck moves, this keeps that id
    /// pointed at whichever site is actually loudest, so the tuned station
    /// just gets stronger the way any single station would -- it never
    /// needs a second dial entry to hand over to, and it never sits stuck
    /// on a fading site while a clearer one of the same identity is in
    /// range.
    fn identity_handover(
        &self,
        station: &RadioStation,
        current_signal: f64,
    ) -> Option<RadioStation> {
        let siblings = self.identity_siblings.get(&station_identity(station))?;
        let mut best_station = station;
        let mut best_signal = current_signal;
        for candidate in siblings {
            if candidate.id == station.id || !self.station_allowed(candidate) {
                continue;
            }
            let candidate_signal =
                estimate_signal(candidate, self.position, self.elevation_ft).signal;
            if candidate_signal > best_signal {
                best_station = candidate;
                best_signal = candidate_signal;
            }
        }
        if best_station.id != station.id {
            Some(best_station.clone())
        } else {
            None
        }
    }

    pub fn current_reception(&mut self) -> RadioReception {
        let station = self.current_station();
        if station.fallback {
            return self.fallback_reception();
        }
        self.reception(&station)
    }

    pub fn fallback_station(&self) -> RadioStation {
        // The audible fallback first: losing a station must not mean
        // silence (owner, 2026-08-31). `station_allowed` keeps this honest
        // -- streamer-safe mode filters the real stream out, and an Eagle
        // that failed to open sits in `unplayable_ids` -- so both of those
        // still land on the silent satellite below, whose guarantee is
        // exactly that it always "plays".
        if let Some(station) = self.synth_fallback() {
            return station;
        }
        if let Some(station) = self
            .catalog
            .iter()
            .find(|s| s.id == super::AUDIBLE_FALLBACK_STATION_ID && self.station_allowed(s))
        {
            return station.clone();
        }
        if let Some(station) = self
            .catalog
            .iter()
            .find(|s| s.fallback && self.station_allowed(s))
        {
            return station.clone();
        }
        if let Some(station) = self
            .catalog
            .iter()
            .find(|s| !s.real_stream && self.station_allowed(s))
        {
            return station.clone();
        }
        self.catalog[0].clone()
    }

    pub fn fallback_reception(&self) -> RadioReception {
        let station = self.fallback_station();
        RadioReception {
            station,
            distance_miles: None,
            signal: 1.0,
            reason: "fallback".to_string(),
            fallback: true,
            streamed: false,
        }
    }

    pub fn status_text(&mut self) -> String {
        let reception = self.current_reception();
        let station = &reception.station;
        let state = if self.enabled { "on" } else { "off" };
        let safety = if self.streamer_safe {
            "streamer-safe"
        } else {
            "streamer-safe off"
        };
        format!(
            "Radio {state}. {}, {}. {}. Volume {} percent. {safety}. Source: {}.",
            station.display_name(),
            station.format,
            reception.signal_label(),
            round_py_int(self.volume * 100.0),
            station.source
        )
    }

    pub fn toggle(&mut self, backend: Option<&mut dyn RadioPlaybackBackend>) -> RadioAction {
        self.enabled = !self.enabled;
        self.end_sweep();
        if !self.enabled {
            Self::stop(backend);
            let station = self.current_station();
            let reception = self.current_reception();
            return RadioAction {
                message: "Radio off.".to_string(),
                station,
                enabled: false,
                reception,
                fallback_used: false,
                retried: false,
            };
        }
        self.power_on_retune();
        self.play(backend, "Radio on.")
    }

    /// Power-on lands on a station that plays clean.
    ///
    /// The remembered station keeps the dial only while it still comes in
    /// at full volume -- playlists and other always-available choices
    /// always do. A fringe or out-of-range memory retunes to the strongest
    /// ranged signal on the dial instead (owner ruling, 2026-08-12).
    fn power_on_retune(&mut self) {
        let reception = self.current_reception();
        if !reception.fallback && reception.signal >= SIGNAL_FULL_VOLUME {
            return;
        }
        let ranged: Vec<RadioReception> = self
            .receivable_stations()
            .into_iter()
            .filter(|r| {
                !r.fallback
                    && !r.streamed
                    && !r.station.always_available
                    && r.station.range_miles > 0.0
            })
            .collect();
        let Some(best) = ranged
            .iter()
            .max_by(|a, b| a.signal.partial_cmp(&b.signal).unwrap())
        else {
            return;
        };
        if reception.fallback || best.signal > reception.signal {
            self.station_id = best.station.id.clone();
        }
    }

    pub fn tune(
        &mut self,
        direction: i64,
        backend: Option<&mut dyn RadioPlaybackBackend>,
    ) -> RadioAction {
        // A switched-off radio does not tune, the way a real one does not
        // (Darren, 2026-08-16; owner ruling the same day). The dial used to
        // pick a station silently and hold it for power-on, which was
        // deliberate but read as a dead key -- and the fix for that reading
        // is the behaviour matching the expectation, not a longer sentence.
        // The dial says why rather than going silent: nothing happening with
        // no explanation is the one outcome a screen reader user cannot tell
        // from a broken key.
        if let Some(refused) = self.locked_action() {
            return refused;
        }
        if !self.enabled {
            return self.dial_is_off();
        }
        let receptions = self.sweep_receptions();
        let current = self.current_station();
        let index = receptions
            .iter()
            .position(|r| r.station.id == current.id)
            .unwrap_or(0) as i64;
        let len = receptions.len() as i64;
        let reception = &receptions[(index + direction).rem_euclid(len) as usize];
        self.station_id = reception.station.id.clone();
        let prefix = format!("Tuned to {}.", reception.station.display_name());
        self.play(backend, &prefix)
    }

    /// The receivable stations in this sweep's order.
    ///
    /// Built strongest-first when a sweep starts and held while the truck
    /// stays within `DIAL_SWEEP_MI` of where it started; a station that has
    /// since come into range joins at the end of its category, one that has
    /// dropped out is skipped. So every press visits the next station, and
    /// no station twice, however the signals drift under a moving truck.
    pub fn sweep_receptions(&mut self) -> Vec<RadioReception> {
        let receptions = self.receivable_stations();
        let moved = match (self.sweep_anchor, self.position) {
            (Some((alat, alon)), Some((plat, plon))) => {
                let anchor = RadioStation {
                    lat: Some(alat),
                    lon: Some(alon),
                    ..RadioStation::default()
                };
                station_distance_miles(&anchor, Some((plat, plon))).unwrap_or(0.0)
            }
            (None, _) => f64::INFINITY,
            (Some(_), None) => 0.0,
        };
        if self.sweep_order.is_empty() || moved >= DIAL_SWEEP_MI {
            self.sweep_order = receptions.iter().map(|r| r.station.id.clone()).collect();
            self.sweep_anchor = self.position;
            return receptions;
        }
        let rank = |id: &str| self.sweep_order.iter().position(|known| known == id);
        let mut ordered: Vec<(usize, usize, RadioReception)> = receptions
            .into_iter()
            .enumerate()
            .map(|(fresh, r)| (rank(&r.station.id).unwrap_or(usize::MAX), fresh, r))
            .collect();
        ordered.sort_by_key(|(known, fresh, _)| (*known, *fresh));
        ordered.into_iter().map(|(_, _, r)| r).collect()
    }

    /// Forget the current sweep: the next dial press builds a fresh order.
    fn end_sweep(&mut self) {
        self.sweep_order.clear();
        self.sweep_anchor = None;
    }

    /// The reply to any dial key while the radio is switched off.
    fn dial_is_off(&mut self) -> RadioAction {
        let station = self.current_station();
        let reception = self.current_reception();
        RadioAction {
            message: "Radio off.".to_string(),
            station,
            enabled: false,
            reception,
            fallback_used: false,
            retried: false,
        }
    }

    /// Jump to the first station of the previous/next dial category.
    ///
    /// Twenty-five AFN entries in a row buried the terrestrial section for
    /// anyone tuning linearly (owner, 2026-07-20); this is the escape. Only
    /// categories with a receivable station exist to jump to, and the spoken
    /// line leads with the category so the landing is oriented.
    ///
    /// Inert with the radio switched off, exactly like the plain dial keys:
    /// it is the same control one layer up.
    pub fn tune_category(
        &mut self,
        direction: i64,
        backend: Option<&mut dyn RadioPlaybackBackend>,
    ) -> RadioAction {
        if let Some(refused) = self.locked_action() {
            return refused;
        }
        if !self.enabled {
            return self.dial_is_off();
        }
        self.end_sweep();
        let receptions = self.receivable_stations();
        let mut groups: Vec<i32> = Vec::new();
        for reception in &receptions {
            let group = self.group(&reception.station);
            if !groups.contains(&group) {
                groups.push(group);
            }
        }
        let current = self.current_station();
        let current_group = self.group(&current);
        let target = match groups.iter().position(|g| *g == current_group) {
            Some(index) => {
                let len = groups.len() as i64;
                groups[(index as i64 + direction).rem_euclid(len) as usize]
            }
            None => groups[0],
        };
        let reception = receptions
            .iter()
            .find(|r| self.group(&r.station) == target)
            .expect("the target group came from these receptions");
        self.station_id = reception.station.id.clone();
        let label = dial_category_name(target);
        let prefix = format!("{label}. Tuned to {}.", reception.station.display_name());
        self.play(backend, &prefix)
    }

    pub fn select_station(
        &mut self,
        station_id: &str,
        backend: Option<&mut dyn RadioPlaybackBackend>,
    ) -> RadioAction {
        // The Roadhouse passes: it is where the game itself moves a locked dial.
        if station_id != SAFE_ROUTE_PLAYLIST {
            if let Some(refused) = self.locked_action() {
                return refused;
            }
        }
        let station = match self.station_by_id(station_id).cloned() {
            Some(station) if self.station_allowed(&station) => station,
            _ => return self.play(backend, "Radio fallback."),
        };
        self.station_id = station.id.clone();
        if !self.enabled {
            // Not a dial key: this path is the game moving the dial off a
            // station the player may no longer have (streamer-safe, or a
            // signal lost mid-drive), so it still moves while switched
            // off. It only reports; it never promises playback.
            let phrase = Self::station_phrase(&self.reception(&station));
            let reception = self.current_reception();
            return RadioAction {
                message: format!("Radio off. Selected {phrase}."),
                station,
                enabled: false,
                reception,
                fallback_used: false,
                retried: false,
            };
        }
        let prefix = format!("Selected {}.", station.display_name());
        self.play(backend, &prefix)
    }

    pub fn play(
        &mut self,
        backend: Option<&mut dyn RadioPlaybackBackend>,
        prefix: &str,
    ) -> RadioAction {
        let reception = self.current_reception();
        let station = reception.station.clone();
        if !self.enabled {
            Self::stop(backend);
            return RadioAction {
                message: "Radio off.".to_string(),
                station,
                enabled: false,
                reception,
                fallback_used: false,
                retried: false,
            };
        }
        let Some(backend) = backend else {
            return RadioAction {
                message: Self::play_message(prefix, &reception, ""),
                station,
                enabled: true,
                reception,
                fallback_used: false,
                retried: false,
            };
        };
        if backend.play_reception(&reception, self.volume).is_err() {
            // One refusal is not a dead station. The first time a stream
            // fails to come up this session it gets a fresh connect on the
            // spot and the radio says so; only a second failure writes it
            // off (below), because a small Icecast host behind a home line
            // can simply be slow, and a radio that hands over at the first
            // miss teaches the player that the station is gone when it is
            // not (owner, 2026-08-22, Darren Duff radio).
            let failures = self.connect_failures.get(&station.id).copied().unwrap_or(0) + 1;
            self.connect_failures.insert(station.id.clone(), failures);
            if failures < 2 && Self::retry_connect(backend, &reception, self.volume) {
                return RadioAction {
                    message: format!(
                        "{} is slow to answer. Trying again.",
                        station.display_name()
                    ),
                    station,
                    enabled: true,
                    reception,
                    fallback_used: false,
                    retried: true,
                };
            }
            // A stream that refuses to play twice leaves the dial for the rest
            // of the session (it returns next session; streams have bad days),
            // and the radio hands over to the next station on the same band
            // rather than dropping the player to the silent fallback.
            let original = reception;
            self.mark_unplayable(&original.station.id.clone());
            let replacement = self
                .same_band_replacement(&original.station)
                .unwrap_or_else(|| self.fallback_reception());
            self.station_id = replacement.station.id.clone();
            if backend.play_reception(&replacement, self.volume).is_err() {
                backend.stop_radio();
            }
            let prefix = if replacement.fallback {
                "Radio fallback."
            } else {
                "Radio handover."
            };
            return RadioAction {
                message: Self::play_message(
                    prefix,
                    &replacement,
                    &Self::refusal_clause(&original.station),
                ),
                station: replacement.station.clone(),
                enabled: true,
                reception: replacement,
                fallback_used: true,
                retried: false,
            };
        }
        RadioAction {
            message: Self::play_message(prefix, &reception, ""),
            station,
            enabled: true,
            reception,
            fallback_used: false,
            retried: false,
        }
    }

    /// Start one more connect for a station that just refused; true if
    /// the backend accepted the attempt (a streaming backend answers later,
    /// so acceptance is all that can be known here).
    fn retry_connect(
        backend: &mut dyn RadioPlaybackBackend,
        reception: &RadioReception,
        volume: f64,
    ) -> bool {
        backend.play_reception(reception, volume).is_ok()
    }

    /// Why a station just left the dial, in the player's own terms.
    ///
    /// "Off the air" is true of a dead broadcast and false of a playlist:
    /// a playlist whose tracks will not open is a folder problem the player
    /// can go and fix, and saying so is the difference between a fixable
    /// fault and a mystery.
    fn refusal_clause(station: &RadioStation) -> String {
        if station.source_type == PERSONAL_PLAYLIST_SOURCE_TYPE {
            return format!(
                "None of the tracks in {} would open; it is off the dial for the rest of this session.",
                station.display_name()
            );
        }
        format!(
            "{} is off the air; it is off the dial for the rest of this session.",
            station.display_name()
        )
    }

    /// Take a station that refused to play off the dial for this session.
    ///
    /// A multi-site station's sites all serve the same stream URL, so a
    /// dead stream is dead at every site: the whole identity goes off the
    /// dial together, or the handover in current_station/receivable_stations
    /// would just find a sibling site pointed at the same dead URL and try
    /// it again.
    pub fn mark_unplayable(&mut self, station_id: &str) {
        self.unplayable_ids.insert(station_id.to_string());
        let Some(station) = self.station_by_id(station_id) else {
            return;
        };
        if let Some(siblings) = self.identity_siblings.get(&station_identity(station)) {
            for sibling in siblings {
                self.unplayable_ids.insert(sibling.id.clone());
            }
        }
    }

    /// The next receivable station in the failed station's dial category.
    fn same_band_replacement(&self, failed: &RadioStation) -> Option<RadioReception> {
        let group = dial_group(failed);
        self.receivable_stations().into_iter().find(|reception| {
            !reception.fallback
                && reception.station.id != failed.id
                && dial_group(&reception.station) == group
        })
    }

    pub(super) fn station_allowed(&self, station: &RadioStation) -> bool {
        if !station.supported {
            return false;
        }
        if self.unplayable_ids.contains(&station.id) || !self.synth_allows(station) {
            return false;
        }
        if !station.real_stream && station.source_type != PERSONAL_PLAYLIST_SOURCE_TYPE {
            return true;
        }
        // Real streams and personal media ride the same gate: the game
        // cannot vouch for their licensing, and streamer-safe mode is the
        // one switch that keeps such audio off a broadcast.
        !self.streamer_safe
    }

    /// The catalog row for an id, exactly as checked in: no range check, no
    /// handover, no fallback. This is how the drive names the station its
    /// cab is actually playing, which the resolving reads above cannot
    /// answer once the dial has moved on.
    pub fn station_by_id(&self, station_id: &str) -> Option<&RadioStation> {
        self.catalog.iter().find(|s| s.id == station_id)
    }

    fn clamp_volume(volume: f64) -> f64 {
        volume.clamp(0.0, 1.0)
    }

    fn station_phrase(reception: &RadioReception) -> String {
        format!(
            "{}, {}, {}",
            reception.station.display_name(),
            reception.station.format,
            reception.signal_label()
        )
    }

    fn play_message(prefix: &str, reception: &RadioReception, extra: &str) -> String {
        let station = &reception.station;
        // A prefix like "Tuned to <station>." already names the station;
        // repeating the name right after would speak it twice in a row.
        let display_name = station.display_name();
        let name = if prefix.contains(&display_name) {
            String::new()
        } else {
            display_name
        };
        let parts: Vec<String> = [
            prefix,
            name.as_str(),
            station.format.as_str(),
            reception.signal_label(),
            extra,
        ]
        .into_iter()
        .filter(|part| !part.is_empty())
        .map(|part| part.trim_end_matches('.').to_string())
        .collect();
        format!("{}.", parts.join(". "))
    }

    /// The station's dial group, with saved stations pulled forward.
    ///
    /// Only later categories promote: the route playlist, Freight Fate
    /// stations, and personal playlists already sit ahead of Favorites, and
    /// demoting them there would reorder the front of the dial for no gain.
    pub fn group(&self, station: &RadioStation) -> i32 {
        let group = dial_group(station);
        if group > FAVORITES_GROUP && self.is_favorite(station) {
            return FAVORITES_GROUP;
        }
        group
    }

    /// station.id plus every sibling site's id sharing its identity.
    ///
    /// A multi-site station is favorited or unfavorited as one station, not
    /// per site: whichever site happens to be loudest when the driver saves
    /// it should not be the only one that counts as saved once the truck
    /// moves on and a sibling site takes over the dial listing.
    fn identity_ids(&self, station: &RadioStation) -> Vec<String> {
        match self.identity_siblings.get(&station_identity(station)) {
            Some(siblings) => siblings.iter().map(|s| s.id.clone()).collect(),
            None => vec![station.id.clone()],
        }
    }

    /// Save or unsave the current station; the spoken confirmation.
    pub fn toggle_favorite(&mut self) -> String {
        if self.station_locked() {
            return String::new();
        }
        let station = self.current_station();
        if station.fallback {
            return "The safety fallback is always on the dial.".to_string();
        }
        let ids = self.identity_ids(&station);
        if ids.iter().any(|id| self.favorite_ids.contains(id)) {
            for id in &ids {
                self.favorite_ids.remove(id);
            }
            return format!("Removed {} from favorites.", station.display_name());
        }
        self.favorite_ids.extend(ids);
        format!("Saved {} to favorites.", station.display_name())
    }

    fn reception_sort_key<'a>(&self, reception: &'a RadioReception) -> (i32, f64, (bool, &'a str)) {
        let station = &reception.station;
        let group = self.group(station);
        // Terrestrial runs strongest-first; every other group keeps call-sign
        // order (web radio relies on the sort staying stable -- its call
        // signs are empty and its catalog order is listener-vote order).
        // A station with no call sign follows the lettered ones, so Channel
        // 3000 does not head the Freight Fate stations (owner, 2026-10-13).
        let signal = if group == TERRESTRIAL_GROUP {
            -reception.signal
        } else {
            0.0
        };
        let call_sign = station.call_sign.as_str();
        (group, signal, (call_sign.is_empty(), call_sign))
    }

    fn stop(backend: Option<&mut dyn RadioPlaybackBackend>) {
        if let Some(backend) = backend {
            backend.stop_radio();
        }
    }
}
