//! The pure parts of `tests/test_radio.py`, `test_radio_regional.py`,
//! `test_radio_favorites.py`, `test_radio_imported.py`,
//! `test_radio_multi_site.py` and `test_radio_playlists.py`.
//!
//! Everything that drove `App()` / `DrivingState` (the reception tick, the
//! fringe renderer, playlist playback, the settings round trip) is the game
//! crate's; `test_radio_streaming.py` is entirely the BASS backend and has
//! no pure half.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use super::playlists::py_path_string;
use super::*;

const DALLAS: (f64, f64) = (32.7767, -96.7970);
const CHICAGO: (f64, f64) = (41.8781, -87.6298);

fn catalog() -> Vec<RadioStation> {
    default_radio_catalog().to_vec()
}

fn curated() -> Vec<RadioStation> {
    load_radio_catalog(&default_data_root()).unwrap()
}

fn station(station_id: &str) -> RadioStation {
    default_radio_catalog()
        .iter()
        .find(|s| s.id == station_id)
        .cloned()
        .unwrap_or_else(|| panic!("{station_id} not in the catalog"))
}

fn ids(stations: &[RadioStation]) -> Vec<&str> {
    stations.iter().map(|s| s.id.as_str()).collect()
}

/// Move due north by `miles` from `position`.
///
/// Exact for a pure meridian offset: the haversine formula collapses to
/// the great-circle arc R * dlat when dlon is 0, so this needs no
/// small-angle approximation.
fn north_of(position: (f64, f64), miles: f64) -> (f64, f64) {
    (
        position.0 + (miles / EARTH_RADIUS_MI).to_degrees(),
        position.1,
    )
}

#[derive(Default)]
struct RecordingBackend {
    fail_ids: HashSet<String>,
    played: Vec<(String, f64)>,
    stopped: usize,
}

impl RecordingBackend {
    fn failing(ids: &[&str]) -> Self {
        Self {
            fail_ids: ids.iter().map(|s| s.to_string()).collect(),
            ..Default::default()
        }
    }
}

impl RadioPlaybackBackend for RecordingBackend {
    fn play_station(
        &mut self,
        station: &RadioStation,
        volume: f64,
    ) -> Result<(), RadioPlaybackError> {
        if self.fail_ids.contains(&station.id) {
            return Err(RadioPlaybackError("station failed".into()));
        }
        self.played.push((station.id.clone(), volume));
        Ok(())
    }
    fn stop_radio(&mut self) {
        self.stopped += 1;
    }
}

// -- test_radio.py -------------------------------------------------------------

#[test]
fn test_catalog_loads_structured_regional_and_afn_stations() {
    let catalog = curated();
    let ids = ids(&catalog);
    let afn: Vec<&RadioStation> = catalog.iter().filter(|s| s.source_type == "afn").collect();
    let locals: Vec<&RadioStation> = catalog
        .iter()
        .filter(|s| s.source_type == "local")
        .collect();

    assert!(catalog.len() >= 20);
    assert!(ids.contains(&SAFE_ROUTE_PLAYLIST));
    assert!(ids.contains(&SAFE_FALLBACK_STATION_ID));
    assert!(afn.len() >= 5);
    for expected in [
        "afn-aviano",
        "afn-bavaria",
        "afn-benelux",
        "afn-tokyo",
        "afn-guantanamo-bay",
        "afn-incirlik",
        "afn-kaiserslautern",
        "afn-humphreys",
        "afn-daegu",
        "afn-bahrain",
        "afn-naples",
        "afn-rota",
        "afn-sigonella",
        "afn-souda-bay",
        "afn-spangdahlem",
        "afn-stuttgart",
        "afn-vicenza",
        "afn-wiesbaden",
    ] {
        assert!(ids.contains(&expected), "{expected}");
    }
    let regions: HashSet<&str> = locals.iter().map(|s| s.region.as_str()).collect();
    assert!(regions.len() >= 7);
    assert!(afn
        .iter()
        .chain(locals.iter())
        .all(|s| !s.stream_url.is_empty()));
    assert!(afn
        .iter()
        .chain(locals.iter())
        .all(|s| !s.stream_format.is_empty()));
    // A local stream can rot off the air (WABE 2026-07-14), but going dark
    // is a documented state, never a silent one: unsupported locals carry
    // notes saying why, and the dial stays overwhelmingly alive.
    let dark_locals: Vec<&&RadioStation> = locals.iter().filter(|s| !s.supported).collect();
    assert!(dark_locals.iter().all(|s| !s.notes.is_empty()));
    assert!(dark_locals.len() <= locals.len() / 10);
    assert!(afn.iter().filter(|s| s.supported).count() >= 15);
    assert!(locals.iter().all(|s| s.lat.is_some() && s.lon.is_some()));
    assert!(locals.iter().all(|s| s.range_miles > 0.0));
}

#[test]
fn test_radio_defaults_to_full_dial_on_builtin_station() {
    let mut radio = RadioState::new(catalog());

    assert!(radio.enabled);
    assert_eq!(radio.current_station().id, SAFE_ROUTE_PLAYLIST);
    assert_eq!(radio.volume, 0.25);
    // Streamer-safe mode is the opt-out, not the default: the full dial,
    // real public streams included, is the out-of-the-box experience.
    assert!(!radio.streamer_safe);
    assert!(radio
        .available_stations()
        .iter()
        .any(|s| s.source_type == "afn"));
    let status = radio.status_text();
    assert!(status.contains("streamer-safe off"));
    assert!(status.contains("always available"));
}

#[test]
fn test_streamer_safe_mode_hides_real_streams() {
    let mut radio = RadioState::new(catalog()).with_streamer_safe(true);
    assert!(!radio
        .available_stations()
        .iter()
        .any(|s| s.source_type == "afn"));

    radio.streamer_safe = false;

    assert!(radio
        .available_stations()
        .iter()
        .any(|s| s.id == "afn-tokyo"));
    assert!(radio
        .available_stations()
        .iter()
        .filter(|s| s.real_stream)
        .all(|s| !s.safe_for_streaming));
}

struct FakeSettings {
    radio_enabled: bool,
    radio_station_id: String,
    radio_volume: f64,
    radio_streamer_safe: bool,
}

impl RadioSettingsAccess for FakeSettings {
    fn radio_enabled(&self) -> bool {
        self.radio_enabled
    }
    fn radio_station_id(&self) -> String {
        self.radio_station_id.clone()
    }
    fn radio_volume(&self) -> f64 {
        self.radio_volume
    }
    fn radio_streamer_safe(&self) -> bool {
        self.radio_streamer_safe
    }
    fn synth_music(&self) -> bool {
        false
    }
    fn set_radio_enabled(&mut self, enabled: bool) {
        self.radio_enabled = enabled;
    }
    fn set_radio_station_id(&mut self, station_id: &str) {
        self.radio_station_id = station_id.to_string();
    }
}

#[test]
fn test_radio_persists_enabled_station_and_volume() {
    // The Settings save/load round trip is the settings port's; the radio
    // side reads the fields back through RadioSettingsAccess.
    let mut settings = FakeSettings {
        radio_enabled: false,
        radio_station_id: "ff-night-line".into(),
        radio_volume: 0.4,
        radio_streamer_safe: true,
    };
    let radio = RadioState::from_settings(catalog(), &settings, &[]);
    assert!(!radio.enabled);
    assert_eq!(radio.station_id, "ff-night-line");
    assert_eq!(radio.volume, 0.4);
    assert!(radio.streamer_safe);

    let mut on = RadioState::new(catalog()).with_station_id("afn-tokyo");
    on.enabled = true;
    on.write_settings(&mut settings);
    assert!(settings.radio_enabled);
    assert_eq!(settings.radio_station_id, "afn-tokyo");
}

#[test]
fn test_regional_station_filtering_uses_simulated_truck_position() {
    let radio = RadioState::new(catalog()).with_position(Some((47.61, -122.33)));
    let stations = radio.available_stations();
    let ids = ids(&stations);

    assert!(ids.contains(&"kexp-seattle"));
    assert!(!ids.contains(&"wbur-boston"));
    let kexp = stations.iter().find(|s| s.id == "kexp-seattle").unwrap();
    assert_eq!(
        estimate_signal(kexp, radio.position, None).signal_label(),
        "strong signal"
    );
}

#[test]
fn test_tuning_uses_receivable_stations_not_global_catalog() {
    let mut radio = RadioState::new(catalog()).with_position(Some((47.61, -122.33)));
    let mut backend = RecordingBackend::default();
    let receivable: HashSet<String> = radio
        .receivable_stations()
        .into_iter()
        .map(|r| r.station.id)
        .collect();

    // The guarantee is that the dial is drawn from what the truck can actually
    // receive. That is two claims, and neither needs the whole ring: the
    // receivable set carries Seattle and not Boston, and every press lands
    // inside that set. Together they say no amount of tuning reaches Boston.
    assert!(receivable.contains("kexp-seattle"));
    assert!(!receivable.contains("wbur-boston"));

    let mut seen = Vec::new();
    for _ in 0..50 {
        let action = radio.tune(1, Some(&mut backend));
        seen.push(action.station.id);
    }

    assert!(!seen.is_empty(), "tuning produced no station at all");
    let off_dial: Vec<&String> = seen.iter().filter(|id| !receivable.contains(*id)).collect();
    assert!(
        off_dial.is_empty(),
        "tuning reached stations the truck cannot receive: {off_dial:?}"
    );
}

#[test]
fn test_ff_music_stations_receivable_everywhere_in_every_mode() {
    // No truck position and streamer-safe on: the strictest possible dial
    // must still carry every Freight Fate original-music station.
    let state = RadioState::new(catalog()).with_streamer_safe(true);
    let names: HashSet<String> = state
        .receivable_stations()
        .into_iter()
        .map(|r| r.station.name)
        .collect();
    for expected in [
        "The Rawhide 98.1",
        "Big Sky Country 99.3",
        "The Delta 94.3",
        "Nashville After Hours 92.9",
        "Freight Fate Roadhouse",
    ] {
        assert!(names.contains(expected), "{expected}");
    }
}

#[test]
fn test_ff_music_stations_share_the_ff_dial_group() {
    let playlist_backed: Vec<&RadioStation> = default_radio_catalog()
        .iter()
        .filter(|s| !s.playlist.is_empty() && !s.real_stream && s.id != SAFE_ROUTE_PLAYLIST)
        .collect();
    // 19 since 2026-10-06: Channel 3000 (87.7) joined the Freight Fate
    // stations. Its "playlist" names its own schedule rather than a music
    // pool, but it is ours, plays no stream, and leaves the dial with the
    // others in Synthesized music mode.
    assert_eq!(playlist_backed.len(), 19);
    assert!(playlist_backed.iter().all(|s| dial_group(s) == 1));
    assert!(playlist_backed.iter().all(|s| s.always_available));
}

#[test]
fn test_channel_3000_does_not_head_the_ff_stations() {
    // No call sign used to sort it ahead of FFN and FFR, so the category
    // jump landed on Channel 3000 (owner, 2026-10-13). It follows the
    // lettered stations now, and the Roadhouse family leads.
    let radio = RadioState::new(catalog()).with_position(Some(DALLAS));
    let ff: Vec<String> = radio
        .receivable_stations()
        .into_iter()
        .filter(|r| dial_group(&r.station) == 1)
        .map(|r| r.station.id)
        .collect();
    assert_eq!(
        ff.last().map(String::as_str),
        Some(crate::channel3000::CHANNEL_3000_ID)
    );
    assert_eq!(ff.first().map(String::as_str), Some("ff-night-line"));
}

#[test]
fn test_no_regional_signal_still_has_safe_and_afn_fallback_choices() {
    // The doubled radio reach (RADIO_REACH_MULT, 2026-08-13) closed the old
    // US-50 Nevada dead zone -- Reno and Las Vegas's community stations now
    // blanket the interior Great Basin. The Denali Highway is still real
    // radio darkness: no curated local station's doubled contour reaches
    // interior Alaska.
    let radio = RadioState::new(catalog()).with_position(Some((63.2, -147.0)));
    let stations = radio.available_stations();

    assert!(stations.iter().any(|s| s.id == SAFE_ROUTE_PLAYLIST));
    assert!(stations.iter().any(|s| s.source_type == "afn"));
    assert!(!stations.iter().any(|s| s.source_type == "local"));
}

#[test]
fn test_dead_stream_hands_over_inside_its_own_band() {
    // A stream that refuses to play must not drop the player to the silent
    // fallback while its band still has stations: the radio hands over to
    // the next receivable station in the same dial category.
    let mut radio = RadioState::new(catalog()).with_station_id("afn-tokyo");
    let mut backend = RecordingBackend::failing(&["afn-tokyo"]);

    let action = radio.play(Some(&mut backend), "");

    assert!(action.fallback_used);
    assert_ne!(action.station.id, "afn-tokyo");
    assert_eq!(action.station.source_type, "afn"); // same band as the dead stream
    assert_eq!(radio.station_id, action.station.id);
    assert_eq!(backend.played, vec![(action.station.id.clone(), 0.25)]);
    assert!(action.message.contains("off the air"));
    assert!(action.message.to_lowercase().contains("handover"));
}

#[test]
fn test_dead_stream_leaves_the_dial_for_the_session() {
    let mut radio = RadioState::new(catalog()).with_station_id("afn-tokyo");
    let mut backend = RecordingBackend::failing(&["afn-tokyo"]);
    radio.play(Some(&mut backend), "");

    let ids: HashSet<String> = radio
        .receivable_stations()
        .into_iter()
        .map(|r| r.station.id)
        .collect();
    assert!(!ids.contains("afn-tokyo"));
    // Tuning back to it lands elsewhere instead of retrying the dead stream.
    let action = radio.select_station("afn-tokyo", Some(&mut backend));
    assert_ne!(action.station.id, "afn-tokyo");
}

#[test]
fn test_dead_stream_with_empty_band_still_reaches_the_fallback() {
    let mut radio = RadioState::new(catalog()).with_station_id("afn-tokyo");
    let afn_ids: Vec<String> = default_radio_catalog()
        .iter()
        .filter(|s| s.source_type == "afn")
        .map(|s| s.id.clone())
        .collect();
    let afn_refs: Vec<&str> = afn_ids.iter().map(String::as_str).collect();
    let mut backend = RecordingBackend::failing(&afn_refs);

    // Every AFN stream is dead: repeated failures burn through the band and
    // the last handover lands on the safe fallback station.
    let mut action = radio.play(Some(&mut backend), "");
    for _ in 0..afn_ids.len() {
        if action.station.source_type != "afn" {
            break;
        }
        action = radio.play(Some(&mut backend), "");
    }

    assert_eq!(action.station.id, SAFE_FALLBACK_STATION_ID);
    assert_eq!(radio.station_id, SAFE_FALLBACK_STATION_ID);
}

#[test]
fn test_spoken_status_includes_signal_source_safety_and_volume() {
    let mut radio = RadioState::new(catalog())
        .with_station_id("kexp-seattle")
        .with_position(Some((47.61, -122.33)))
        .with_volume(0.35);

    let text = radio.status_text();

    assert!(text.contains("KEXP"));
    assert!(text.contains("strong signal"));
    assert!(text.contains("Volume 35 percent"));
    assert!(text.contains("streamer-safe off"));
    assert!(text.contains("Source:"));
}

/// A two-leg route with checked-in geometry, standing in for the world's
/// Seattle -> Portland route the Python test used.
struct FixtureRoute;

impl RadioRoute for FixtureRoute {
    fn leg_count(&self) -> usize {
        1
    }
    fn city(&self, index: usize) -> String {
        ["Seattle", "Portland"][index].to_string()
    }
    fn leg_miles(&self, _index: usize) -> f64 {
        174.0
    }
    fn leg_a(&self, _index: usize) -> String {
        "Seattle".to_string()
    }
    fn leg_route_points(&self, _index: usize) -> Vec<RoutePoint> {
        vec![
            RoutePoint {
                lat: 47.6062,
                lon: -122.3321,
                at_mi: 0.0,
            },
            RoutePoint {
                lat: 47.0379,
                lon: -122.9007,
                at_mi: 60.0,
            },
            RoutePoint {
                lat: 45.5152,
                lon: -122.6784,
                at_mi: 174.0,
            },
        ]
    }
    fn leg_elevation_samples(&self, _index: usize) -> Vec<ElevationSample> {
        vec![
            ElevationSample {
                at_mi: 0.0,
                elevation_ft: 175.0,
            },
            ElevationSample {
                at_mi: 174.0,
                elevation_ft: 50.0,
            },
        ]
    }
}

#[test]
fn test_truck_position_uses_route_geometry() {
    let cities = |name: &str| match name {
        "Seattle" => Some((47.6062, -122.3321)),
        "Portland" => Some((45.5152, -122.6784)),
        _ => None,
    };
    let position = truck_position(Some(&FixtureRoute), 174.0 / 2.0, &cities).unwrap();
    let (lat, lon) = position;
    assert!((44.0..=48.5).contains(&lat));
    assert!((-124.0..=-121.0).contains(&lon));
    // Reversed traversal reads the same geometry from the far end.
    let elev = truck_elevation_ft(Some(&FixtureRoute), 87.0).unwrap();
    assert!((50.0..=175.0).contains(&elev));
    assert!(truck_position(None, 10.0, &cities).is_none());
}

#[test]
fn test_catalog_entries_have_spoken_identity() {
    // Every station in the catalog carries what the dial has to say.
    let mut problems: Vec<String> = Vec::new();
    for station in default_radio_catalog() {
        let where_ = if !station.id.is_empty() {
            station.id.clone()
        } else if !station.name.is_empty() {
            station.name.clone()
        } else {
            "<unnamed station>".to_string()
        };
        if station.id.is_empty() {
            problems.push(format!("{where_}: no id"));
        }
        if station.name.is_empty() {
            problems.push(format!("{where_}: no name"));
        }
        // Web stations are named, not lettered; everything else leads with a
        // call sign, and display_name copes with either shape. Channel 3000
        // is named too: it is a television channel heard on the radio, and
        // a made-up call sign would squat a real FCC one.
        if station.call_sign.is_empty()
            && station.source_type != "web"
            && station.id != crate::channel3000::CHANNEL_3000_ID
        {
            problems.push(format!("{where_}: no call sign and not a web station"));
        }
        let display = station.display_name();
        if display.is_empty() {
            problems.push(format!("{where_}: no display name"));
        } else if display.starts_with(',') {
            problems.push(format!("{where_}: display name starts with a comma"));
        }
        if station.format.is_empty() {
            problems.push(format!("{where_}: no format"));
        }
        if station.source.is_empty() {
            problems.push(format!("{where_}: no source"));
        }
    }
    assert!(
        problems.is_empty(),
        "{} catalog entries are missing spoken identity:\n{}",
        problems.len(),
        problems
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn test_the_dial_does_nothing_while_the_radio_is_switched_off() {
    // A switched-off radio does not tune, the way a real one does not.
    let mut radio = RadioState::new(catalog());
    radio.enabled = false;
    let mut backend = RecordingBackend::default();
    let before = radio.station_id.clone();

    let tuned = radio.tune(1, Some(&mut backend));
    assert_eq!(tuned.message, "Radio off.");
    assert!(!tuned.enabled);
    assert_eq!(radio.station_id, before, "the dial must not move while off");

    let jumped = radio.tune_category(1, Some(&mut backend));
    assert_eq!(jumped.message, "Radio off.");
    assert_eq!(radio.station_id, before);

    assert!(
        backend.played.is_empty(),
        "nothing may play while the radio is off"
    );

    // Switching on lands on the station that was already tuned, untouched by
    // the presses that did nothing.
    let switched = radio.toggle(Some(&mut backend));
    assert!(switched.enabled);
    assert_eq!(radio.station_id, before);
}

#[test]
fn test_the_dial_still_works_normally_once_the_radio_is_on() {
    // The rule is about being switched off, not a new restriction on tuning.
    let mut radio = RadioState::new(catalog());
    assert!(radio.enabled);
    let before = radio.station_id.clone();
    let mut backend = RecordingBackend::default();
    radio.tune(1, Some(&mut backend));
    assert_ne!(radio.station_id, before);
}

#[test]
fn test_the_game_may_still_move_the_dial_off_a_lost_station_while_off() {
    // select_station is not a dial key: it is the game retuning for cause.
    let mut radio = RadioState::new(catalog());
    radio.enabled = false;
    let mut backend = RecordingBackend::default();

    let action = radio.select_station(SAFE_ROUTE_PLAYLIST, Some(&mut backend));

    assert_eq!(radio.station_id, SAFE_ROUTE_PLAYLIST);
    assert!(!action.enabled);
    assert!(backend.played.is_empty());
    assert!(action.message.starts_with("Radio off. Selected "));
}

// -- test_radio_regional.py (pure parts) -----------------------------------------

fn ranged_fixture(
    station_id: &str,
    lat: f64,
    lon: f64,
    range_miles: f64,
    site_elev_ft: Option<f64>,
    playlist: &str,
) -> RadioStation {
    RadioStation {
        lat: Some(lat),
        lon: Some(lon),
        range_miles,
        site_elev_ft,
        playlist: playlist.to_string(),
        ..RadioStation::new(
            station_id,
            "Fixture FM",
            "KFIX",
            "country",
            "reception fixture",
        )
    }
}

fn dallas_fixture(range_miles: f64) -> RadioStation {
    ranged_fixture("kfix-dallas", DALLAS.0, DALLAS.1, range_miles, None, "")
}

#[test]
fn test_regional_stations_are_streamer_safe_fiction_available_everywhere() {
    let regional: Vec<&RadioStation> = default_radio_catalog()
        .iter()
        .filter(|s| s.source_type == "regional")
        .collect();
    assert!(regional.len() >= 10);
    for station in regional {
        assert!(!station.real_stream);
        assert!(station.safe_for_streaming);
        assert!(station.supported);
        // Every player hears the FF music: no transmitter bubble, no mode gate.
        assert!(station.always_available);
        assert!(!crate::music::station_playlist(&station.playlist).is_empty());
        // US call-sign convention: K west of the Mississippi, W east
        assert!(station.call_sign.starts_with('K') || station.call_sign.starts_with('W'));
    }
}

#[test]
fn test_builtin_stations_have_hosts_and_playlists() {
    let roadhouse = station("route_playlist");
    let nightline = station("ff-night-line");
    assert_eq!(roadhouse.playlist, "route");
    assert_eq!(roadhouse.host, "roadhouse");
    assert_eq!(nightline.playlist, "night");
    assert_eq!(nightline.host, "nightline");
    assert!(!crate::music::station_host_segments("roadhouse").is_empty());
    assert!(!crate::music::station_host_segments("nightline").is_empty());
}

#[test]
fn test_new_afn_globals_are_cataloged_with_checked_sources() {
    for station_id in ["afn-global-fans", "afn-global-holiday", "afn-mach-5"] {
        let station = station(station_id);
        assert!(station.real_stream);
        assert!(station.stream_url.starts_with("http"));
        assert!(station.source.contains("Radio Browser"));
        assert!(station.always_available);
    }
}

#[test]
fn test_effective_range_doubles_the_published_contour() {
    // Compression compensation (owner design 2026-08-13): the truck covers
    // road miles far faster than a real cab, so the published FM contour is
    // doubled before any distance math touches it.
    let station = dallas_fixture(40.0);
    assert_eq!(
        effective_range_miles(&station, None),
        40.0 * RADIO_REACH_MULT
    );
    assert_eq!(effective_range_miles(&station, None), 80.0);

    // Range-less (built-in) stations are untouched: 0 * mult is still 0.
    let builtin = dallas_fixture(0.0);
    assert_eq!(effective_range_miles(&builtin, None), 0.0);
}

#[test]
fn test_signal_volume_factor_holds_clean_through_most_of_the_contour() {
    // Fixture range_miles=40.0 -> 80 game-miles of reach (RADIO_REACH_MULT).
    let station = dallas_fixture(40.0);
    let at_tower = estimate_signal(&station, Some(DALLAS), None);
    assert_eq!(signal_volume_factor(&at_tower), 1.0);

    // Clean through 80% of the contour (64 of 80 game-miles).
    let clean = estimate_signal(&station, Some(north_of(DALLAS, 64.0)), None);
    assert_eq!(signal_volume_factor(&clean), 1.0);

    // Fading past 85% (70 of 80 game-miles): off full quieting, not yet static.
    let fading = estimate_signal(&station, Some(north_of(DALLAS, 70.0)), None);
    let f = signal_volume_factor(&fading);
    assert!(0.0 < f && f < 1.0);

    // deep fringe (76 of 80 game-miles): the program sinks under the rising
    // static but a trace survives while the station is technically in range
    // (owner's smear rule)
    let deep_fringe = estimate_signal(&station, Some(north_of(DALLAS, 76.0)), None);
    let f = signal_volume_factor(&deep_fringe);
    assert!(0.1 < f && f < 0.6);

    let gone = estimate_signal(&station, Some(CHICAGO), None);
    assert_eq!(gone.signal, 0.0);
    assert_eq!(signal_volume_factor(&gone), 0.0);

    let roadhouse = default_radio_catalog()
        .iter()
        .find(|s| s.id == "route_playlist")
        .unwrap();
    let always = estimate_signal(roadhouse, None, None);
    assert_eq!(signal_volume_factor(&always), 1.0);
}

#[test]
fn test_signal_volume_factor_is_continuous_at_the_new_joins() {
    // Hand-pin the curve at the exact join points, bypassing lat/lon
    // geometry. The owner's smear ruling: static rises TO program level,
    // never on top of a still-loud one -- these two branches must agree
    // exactly where they meet.
    let station = dallas_fixture(120.0);
    let factor = |signal: f64| {
        signal_volume_factor(&RadioReception::new(
            station.clone(),
            Some(10.0),
            signal,
            "in range",
        ))
    };

    // Full-volume join: right at the threshold is still clean, a hair
    // below starts fading.
    assert_eq!(factor(0.20), 1.0);
    assert!(factor(0.1999) < 1.0);

    // Static join: the fringe formula and the deep-floor formula meet at
    // the same value -- static rises TO program level, never above it.
    let edge = factor(0.12);
    assert!((edge - 0.72).abs() < 1e-6);
    assert!(factor(0.1201) > edge); // just inside the fringe: a hair louder
    assert!(factor(0.1199) < edge); // just past: sinking, never a jump up

    // Deep floor: keeps sinking, never below the floor, never silent while
    // still technically in range.
    assert!((factor(0.005) - SIGNAL_DEEP_FLOOR).abs() < 1e-9);
}

#[test]
fn test_elevation_extends_fm_range_like_the_rim() {
    // the owner's ham anchor
    // From high ground you receive far past the flat contour: line-of-sight
    // FM, 4/3-earth radio horizon. Desert Rock Phoenix (site 1086 ft, range
    // 125 mi, held to the 150 mi flat ceiling) at ~220 miles: silent on the
    // flats, clear from ~7000 ft, where the lift buys about 95 miles more.
    let station = ranged_fixture("kfix-phoenix", 33.4484, -112.074, 125.0, Some(1086.0), "");
    let far_north = north_of((33.4484, -112.074), 220.0);

    let flat = estimate_signal(&station, Some(far_north), station.site_elev_ft);
    assert_eq!(flat.signal, 0.0);
    assert_eq!(flat.reason, "out of range");

    let rim = estimate_signal(&station, Some(far_north), Some(7000.0));
    assert!(rim.signal > 0.0);

    // no elevation data behaves exactly like the flat model
    let unknown = estimate_signal(&station, Some(far_north), None);
    assert_eq!(unknown.signal, 0.0);
}

#[test]
fn test_no_station_spans_three_states() {
    // the cap on the doubled reach
    let modest = ranged_fixture("kfix-modest", 39.0, -98.0, 25.0, None, "");
    let giant = ranged_fixture("kfix-giant", 39.0, -98.0, 175.0, None, "");
    // A normal station still gets the full doubling it was given.
    assert_eq!(reach_mi(&modest), 50.0);
    assert_eq!(reach_mi(&giant), RADIO_MAX_REACH_MI);

    // And the far side of the cap really is off the dial, not merely quiet.
    let beyond = north_of((39.0, -98.0), RADIO_MAX_REACH_MI + 20.0);
    assert_eq!(estimate_signal(&giant, Some(beyond), None).signal, 0.0);
}

#[test]
fn test_below_the_tower_site_is_neutral_never_a_penalty() {
    // A mountain-top transmitter looks straight down into its own valley:
    // every in-market listener sits below the site, and that must never
    // shrink the contour (KJZZ on South Mountain serving Phoenix).
    let station = ranged_fixture("kfix-denver", 39.7392, -104.9903, 125.0, Some(5280.0), "");
    let at_100mi = (39.7392 + 1.45, -104.9903);

    let at_site_level = estimate_signal(&station, Some(at_100mi), station.site_elev_ft);
    let below_site = estimate_signal(&station, Some(at_100mi), Some(3800.0));
    assert!((below_site.signal - at_site_level.signal).abs() < 1e-9);
    assert!(below_site.signal > 0.0);
}

#[test]
fn test_fringe_factor_is_monotonic_toward_the_range_edge() {
    let station = dallas_fixture(120.0);
    let factors: Vec<f64> = [0.0, 0.6, 1.2, 1.8]
        .iter()
        .map(|east| {
            signal_volume_factor(&estimate_signal(
                &station,
                Some((DALLAS.0, DALLAS.1 + east)),
                None,
            ))
        })
        .collect();
    let mut sorted = factors.clone();
    sorted.sort_by(|a, b| b.partial_cmp(a).unwrap());
    assert_eq!(factors, sorted);
}

#[test]
fn test_ranged_station_receivable_only_near_its_market() {
    let dallas_fix = dallas_fixture(120.0);
    let chicago_fix = ranged_fixture("wfix-chicago", CHICAGO.0, CHICAGO.1, 120.0, None, "");
    let mut all = catalog();
    all.push(dallas_fix);
    all.push(chicago_fix);
    let mut radio = RadioState::new(all).with_position(Some(DALLAS));
    let near_dallas: HashSet<String> = radio
        .receivable_stations()
        .into_iter()
        .map(|r| r.station.id)
        .collect();
    assert!(near_dallas.contains("kfix-dallas"));
    assert!(!near_dallas.contains("wfix-chicago"));

    radio.update_position(Some(CHICAGO), None);
    let near_chicago: HashSet<String> = radio
        .receivable_stations()
        .into_iter()
        .map(|r| r.station.id)
        .collect();
    assert!(near_chicago.contains("wfix-chicago"));
    assert!(!near_chicago.contains("kfix-dallas"));
}

fn terrestrial_at(station_id: &str, call_sign: &str, degrees_east: f64) -> RadioStation {
    RadioStation {
        lat: Some(DALLAS.0),
        lon: Some(DALLAS.1 + degrees_east),
        range_miles: 120.0,
        ..RadioStation::new(
            station_id,
            &format!("{call_sign} FM"),
            call_sign,
            "country",
            "reception fixture",
        )
    }
}

#[test]
fn test_a_lost_station_hands_the_dial_to_the_strongest_clean_local_signal() {
    // Brandon, 2026-09-17. Contours here are 150 miles; a degree of longitude
    // at Dallas is about 58.
    let lost = terrestrial_at("kold-dallas", "KOLD", 0.0);
    let near = terrestrial_at("kner-dallas", "KNER", 0.3);
    let far = terrestrial_at("kfar-dallas", "KFAR", 1.7);
    let edge = terrestrial_at("kedg-dallas", "KEDG", 2.5);
    let mut all: Vec<RadioStation> = catalog()
        .into_iter()
        .filter(|station| dial_group(station) != TERRESTRIAL_GROUP)
        .collect();
    all.extend([lost.clone(), near, far, edge]);
    let mut radio = RadioState::new(all).with_position(Some(DALLAS));

    let landing = radio
        .strongest_terrestrial(&lost)
        .expect("a station in range");
    assert_eq!(landing.station.id, "kner-dallas");

    // A stream that would not open is off the dial, so the next one takes it.
    radio.mark_unplayable("kner-dallas");
    let landing = radio
        .strongest_terrestrial(&lost)
        .expect("a station in range");
    assert_eq!(landing.station.id, "kfar-dallas");

    // The static smear at the edge of a contour is not a landing: the driver
    // would be handed a station that fades again a few miles on.
    radio.mark_unplayable("kfar-dallas");
    let edge_signal = radio
        .receivable_stations()
        .into_iter()
        .find(|r| r.station.id == "kedg-dallas")
        .expect("the edge station is still receivable")
        .signal;
    assert!(edge_signal > 0.0 && edge_signal < STATIC_SIGNAL_THRESHOLD);
    assert!(radio.strongest_terrestrial(&lost).is_none());
}

#[test]
fn test_no_imported_station_name_has_a_stripped_apostrophe() {
    // "Birmingham s Beautiful QEZ" is what a stripped apostrophe sounds like.
    let catalog = default_radio_catalog();
    let stranded: Vec<&str> = catalog
        .iter()
        .filter(|station| {
            let words: Vec<&str> = station.name.split_whitespace().collect();
            words
                .windows(3)
                .any(|w| w[1] == "s" && w[0].chars().all(|c| c.is_ascii_alphabetic()))
        })
        .map(|station| station.name.as_str())
        .collect();
    assert!(stranded.is_empty(), "{stranded:?}");
}

#[test]
fn test_a_dial_sweep_keeps_its_order_while_the_truck_moves() {
    // Two stations whose signals cross as the truck moves: stepping the
    // dial used to re-sort on every press, so the band could hand back the
    // station just left and skip the next one.
    let east = terrestrial_at("fix-east", "KEEE", 0.30);
    let west = terrestrial_at("fix-west", "KWWW", -0.30);
    let mut radio =
        RadioState::new(vec![east, west]).with_position(Some((DALLAS.0, DALLAS.1 - 0.02)));
    radio.station_id = "fix-west".to_string();
    // Nearer the western tower, west leads the band; the first press steps
    // to east.
    let first = radio.tune(1, None);
    assert_eq!(first.station.id, "fix-east");
    // Drift east a few hundred feet, enough to flip the raw signal order
    // but not to end the sweep: the next press must go on to west, not
    // re-sort and land on east again.
    radio.update_position(Some((DALLAS.0, DALLAS.1 + 0.02)), None);
    let second = radio.tune(1, None);
    assert_eq!(second.station.id, "fix-west");
    // A real move ends the sweep and the band is rebuilt strongest-first,
    // which now puts east ahead.
    radio.update_position(Some((DALLAS.0, DALLAS.1 + 0.2)), None);
    let ids: Vec<String> = radio
        .sweep_receptions()
        .into_iter()
        .map(|r| r.station.id)
        .collect();
    assert_eq!(ids[0], "fix-east");
}

#[test]
fn test_terrestrial_category_sorts_strongest_signal_first() {
    // Call signs deliberately disagree with signal order: the old call-sign
    // sort opened the band on the fringe station at the start of every run.
    let near = terrestrial_at("fix-near", "WZZZ", 0.0);
    let mid = terrestrial_at("fix-mid", "KMMM", 0.9);
    let far = terrestrial_at("fix-far", "KAAA", 1.8);
    let radio = RadioState::new(vec![far, mid, near]).with_position(Some(DALLAS));

    let ids: Vec<String> = radio
        .receivable_stations()
        .into_iter()
        .map(|r| r.station.id)
        .collect();

    assert_eq!(ids, vec!["fix-near", "fix-mid", "fix-far"]);
}

#[test]
fn test_power_on_retunes_a_fringe_memory_to_the_strongest_signal() {
    let strong = terrestrial_at("fix-strong", "WZZZ", 0.0);
    // ~232 mi east: past the doubled 240 mi reach's clean threshold, still
    // technically in range.
    let fringe = terrestrial_at("fix-fringe", "KAAA", 4.0);
    let mut radio = RadioState::new(vec![strong, fringe])
        .with_station_id("fix-fringe")
        .with_position(Some(DALLAS));

    radio.toggle(None); // off
    let action = radio.toggle(None); // back on: the fringe memory does not play clean

    assert!(action.enabled);
    assert_eq!(action.station.id, "fix-strong");
}

#[test]
fn test_power_on_keeps_a_station_that_still_plays_clean() {
    // A full-volume terrestrial memory holds the dial even against a
    // stronger neighbor; so does any always-available choice.
    let strongest = terrestrial_at("fix-strongest", "KAAA", 0.0);
    let clean = terrestrial_at("fix-clean", "WZZZ", 0.35); // ~20 mi: full volume
    let mut radio = RadioState::new(vec![strongest, clean])
        .with_station_id("fix-clean")
        .with_position(Some(DALLAS));
    radio.toggle(None);
    assert_eq!(radio.toggle(None).station.id, "fix-clean");

    let mut playlist = RadioState::new(catalog())
        .with_station_id(SAFE_ROUTE_PLAYLIST)
        .with_position(Some(DALLAS));
    playlist.toggle(None);
    assert_eq!(playlist.toggle(None).station.id, SAFE_ROUTE_PLAYLIST);
}

// -- test_radio_favorites.py ------------------------------------------------------

fn dallas_radio() -> RadioState {
    RadioState::new(catalog()).with_position(Some(DALLAS))
}

#[test]
fn test_toggle_saves_and_unsaves_with_spoken_confirmation() {
    let mut radio = dallas_radio();
    let station = radio
        .available_stations()
        .into_iter()
        .find(|s| s.source_type == "imported")
        .unwrap();
    radio.station_id = station.id.clone();
    let message = radio.toggle_favorite();
    assert_eq!(
        message,
        format!("Saved {} to favorites.", station.display_name())
    );
    assert!(radio.favorite_ids.contains(&station.id));
    let message = radio.toggle_favorite();
    assert_eq!(
        message,
        format!("Removed {} from favorites.", station.display_name())
    );
    assert!(!radio.favorite_ids.contains(&station.id));
}

#[test]
fn test_favorite_pulls_a_web_station_ahead_of_terrestrial() {
    let mut radio = dallas_radio();
    let web = radio
        .available_stations()
        .into_iter()
        .find(|s| s.source_type == "web")
        .unwrap();
    radio.favorite_ids.insert(web.id.clone());
    let stations = radio.available_stations();
    let favorite_index = stations.iter().position(|s| s.id == web.id).unwrap();
    let first_terrestrial = stations
        .iter()
        .position(|s| {
            // Playlist-backed fictional stations are Freight Fate stations now,
            // sorted ahead of favorites by design; terrestrial means the real dial.
            matches!(s.source_type.as_str(), "imported" | "local" | "regional")
                && s.playlist.is_empty()
        })
        .unwrap();
    assert!(favorite_index < first_terrestrial);
    assert_eq!(radio.group(&web), FAVORITES_GROUP);
    assert_eq!(dial_category_name(FAVORITES_GROUP), "Favorites");
}

#[test]
fn test_category_jump_lands_on_favorites() {
    let mut radio = dallas_radio();
    let web = radio
        .available_stations()
        .into_iter()
        .find(|s| s.source_type == "web")
        .unwrap();
    radio.favorite_ids.insert(web.id.clone());
    let mut seen = HashSet::new();
    let mut action = None;
    for _ in 0..DIAL_CATEGORY_NAMES.len() {
        let a = radio.tune_category(1, None);
        let current = radio.current_station();
        seen.insert(radio.group(&current));
        let landed = current.id == web.id;
        action = Some(a);
        if landed {
            break;
        }
    }
    assert_eq!(radio.current_station().id, web.id);
    assert!(seen.contains(&FAVORITES_GROUP));
    assert!(action.unwrap().message.contains("Favorites"));
}

const SEATTLE: (f64, f64) = (47.6062, -122.3321);

/// A Dallas station that comes in strong in Dallas, and is saved.
fn dallas_favorite(radio: &mut RadioState) -> RadioStation {
    let station = radio
        .receivable_stations()
        .into_iter()
        .find(|r| r.station.source_type == "imported" && r.signal >= 0.8)
        .unwrap()
        .station;
    radio.station_id = station.id.clone();
    radio.toggle_favorite();
    station
}

#[test]
fn test_an_out_of_range_station_that_is_not_saved_stays_off_the_dial() {
    let mut radio = dallas_radio();
    let dallas_local = radio
        .available_stations()
        .into_iter()
        .find(|s| s.source_type == "imported")
        .unwrap();
    radio.update_position(Some(SEATTLE), None);
    assert!(!radio
        .available_stations()
        .iter()
        .any(|s| s.id == dallas_local.id));
}

#[test]
fn test_a_favorite_plays_clean_from_anywhere() {
    let mut radio = dallas_radio();
    let favorite = dallas_favorite(&mut radio);
    radio.update_position(Some(SEATTLE), None);
    let on_dial = radio
        .receivable_stations()
        .into_iter()
        .find(|r| r.station.id == favorite.id)
        .expect("a favorite stays on the dial outside its area");
    assert!(on_dial.streamed);
    assert_eq!(on_dial.signal_label(), "streaming");
    assert_eq!(signal_volume_factor(&on_dial), 1.0);
    assert_eq!(radio.group(&favorite), FAVORITES_GROUP);
    // Still tuned: nothing faded out.
    assert_eq!(radio.current_station().id, favorite.id);
    let mut backend = RecordingBackend::default();
    let action = radio.play(Some(&mut backend), "");
    assert!(!action.fallback_used);
    assert_eq!(backend.played.last().unwrap().0, favorite.id);
    assert!(action.message.contains("streaming"), "{}", action.message);
}

#[test]
fn test_a_favorite_keeps_its_real_signal_at_home() {
    let mut radio = dallas_radio();
    let favorite = dallas_favorite(&mut radio);
    let reception = radio.current_reception();
    assert_eq!(reception.station.id, favorite.id);
    assert!(!reception.streamed);
    assert_eq!(reception.signal_label(), "strong signal");
}

#[test]
fn test_a_favorite_streams_once_its_signal_thins() {
    // Where it would start to fade, it streams instead: a favorite has no
    // fringe to crackle through.
    let mut radio = dallas_radio();
    let favorite = dallas_favorite(&mut radio);
    let edge = effective_range_miles(&favorite, None);
    let (lat, lon) = (favorite.lat.unwrap(), favorite.lon.unwrap());
    radio.update_position(Some(north_of((lat, lon), edge * 0.97)), None);
    let physical = estimate_signal(&favorite, radio.position, None);
    assert!(physical.signal > 0.0 && physical.signal < SIGNAL_FULL_VOLUME);
    let reception = radio.current_reception();
    assert!(reception.streamed);
    assert_eq!(signal_volume_factor(&reception), 1.0);
}

#[test]
fn test_unsaving_a_far_favorite_lets_it_fade() {
    let mut radio = dallas_radio();
    let favorite = dallas_favorite(&mut radio);
    radio.update_position(Some(SEATTLE), None);
    radio.toggle_favorite();
    assert_ne!(radio.current_station().id, favorite.id);
}

#[test]
fn test_a_streamed_favorite_is_not_the_strongest_signal_here() {
    let mut radio = dallas_radio();
    let favorite = dallas_favorite(&mut radio);
    radio.update_position(Some(SEATTLE), None);
    let seattle_local = radio
        .receivable_stations()
        .into_iter()
        .find(|r| r.station.source_type == "imported" && !r.streamed)
        .unwrap()
        .station;
    let next = radio.strongest_terrestrial(&seattle_local).unwrap();
    assert_ne!(next.station.id, favorite.id);
    assert!(!next.streamed);
}

#[test]
fn test_power_on_keeps_a_far_favorite() {
    let mut radio = dallas_radio();
    let favorite = dallas_favorite(&mut radio);
    radio.update_position(Some(SEATTLE), None);
    radio.enabled = false;
    radio.toggle(None);
    assert_eq!(radio.current_station().id, favorite.id);
}

#[test]
fn test_streamer_safe_mode_still_hides_a_far_favorite() {
    let mut radio = dallas_radio();
    let favorite = dallas_favorite(&mut radio);
    radio.update_position(Some(SEATTLE), None);
    radio.streamer_safe = true;
    assert!(!radio
        .available_stations()
        .iter()
        .any(|s| s.id == favorite.id));
}

#[test]
fn test_streamer_safe_mode_still_hides_favorited_real_streams() {
    let mut radio = dallas_radio();
    let web = radio
        .available_stations()
        .into_iter()
        .find(|s| s.source_type == "web")
        .unwrap();
    radio.favorite_ids.insert(web.id.clone());
    radio.streamer_safe = true;
    assert!(!radio.available_stations().iter().any(|s| s.id == web.id));
}

#[test]
fn test_the_safety_fallback_cannot_be_favorited() {
    // The SILENT satellite, by its flag -- fallback_station() now resolves
    // to the audible Eagle first, and the Eagle is an ordinary station a
    // player may favorite like any other.
    let mut radio = dallas_radio();
    let fallback = radio
        .catalog
        .iter()
        .find(|s| s.fallback)
        .expect("the catalog carries the silent satellite")
        .clone();
    radio.station_id = fallback.id.clone();
    radio.update_position(None, None);
    // Force current onto the fallback by making nothing else receivable.
    radio.set_catalog(vec![fallback]);
    assert_eq!(
        radio.toggle_favorite(),
        "The safety fallback is always on the dial."
    );
    assert!(radio.favorite_ids.is_empty());
}

/// Losing a station lands on the Eagle, not on silence (owner, 2026-08-31);
/// streamer-safe mode still lands on the silent satellite, whose guarantee
/// is exactly that nothing licensable plays.
#[test]
fn test_fallback_is_the_eagle_unless_streamer_safe() {
    let mut radio = dallas_radio();
    assert_eq!(radio.fallback_station().id, AUDIBLE_FALLBACK_STATION_ID);

    radio.streamer_safe = true;
    assert_eq!(radio.fallback_station().id, SAFE_FALLBACK_STATION_ID);
}

/// An Eagle that failed to open must not be re-tuned forever: once struck
/// unplayable, the fallback moves on to the silent satellite.
#[test]
fn test_fallback_skips_an_unplayable_eagle() {
    let mut radio = dallas_radio();
    radio
        .unplayable_ids
        .insert(AUDIBLE_FALLBACK_STATION_ID.to_string());
    assert_eq!(radio.fallback_station().id, SAFE_FALLBACK_STATION_ID);
}

#[test]
fn test_favorites_ride_the_profile() {
    // The Profile round trip is the models port's; the radio side takes
    // the profile's list as plain ids.
    let settings = FakeSettings {
        radio_enabled: true,
        radio_station_id: "route_playlist".into(),
        radio_volume: 0.25,
        radio_streamer_safe: false,
    };
    let radio = RadioState::from_settings(catalog(), &settings, &["rb-someid".to_string()]);
    assert_eq!(radio.favorite_ids, HashSet::from(["rb-someid".to_string()]));
}

// -- test_radio_imported.py (pure parts) ------------------------------------------

fn curated_ids() -> HashSet<String> {
    curated().into_iter().map(|s| s.id).collect()
}

fn imported() -> Vec<RadioStation> {
    default_radio_catalog()
        .iter()
        .filter(|s| s.source_type == "imported")
        .cloned()
        .collect()
}

/// The automated web tier only: the curated catalog carries a handful of web
/// stations of its own now (Radiostorm's four channels), and those are held
/// to the curated catalog's standards, not the directory's.
fn web() -> Vec<RadioStation> {
    let curated = curated_ids();
    default_radio_catalog()
        .iter()
        .filter(|s| s.source_type == "web" && !curated.contains(&s.id))
        .cloned()
        .collect()
}

#[test]
fn test_imported_tier_loads_underneath_curated() {
    // Floors, not exact counts. The tier shrinks when a reachability sweep
    // drops streams that have gone off the air and grows when directory
    // stations are placed at their FCC transmitter. It should stay a tier,
    // not dwindle to a handful.
    let imported = imported();
    assert!(imported.len() >= 1200);
    assert!(imported.iter().all(|s| s.real_stream));
    assert!(!imported.iter().any(|s| s.safe_for_streaming));
    assert!(imported
        .iter()
        .all(|s| s.stream_url.starts_with("http://") || s.stream_url.starts_with("https://")));
    assert!(imported.iter().all(|s| s.lat.is_some() && s.lon.is_some()));
    assert!(imported.iter().all(|s| s.range_miles > 0.0));
    let ids: HashSet<&str> = default_radio_catalog()
        .iter()
        .map(|s| s.id.as_str())
        .collect();
    assert_eq!(ids.len(), default_radio_catalog().len());
}

#[test]
fn test_web_tier_is_always_available_and_gated() {
    let web = web();
    assert!(web.len() >= 3500);
    assert!(web.iter().all(|s| s.always_available));
    assert!(web.iter().all(|s| s.real_stream));
    assert!(!web.iter().any(|s| s.safe_for_streaming));
    assert!(web.iter().all(|s| !s.name.is_empty()));
    assert!(!web.iter().any(|s| !s.call_sign.is_empty()));
    // display_name copes with the missing call sign: no leading comma.
    assert!(!web.iter().any(|s| s.display_name().starts_with(',')));
}

#[test]
fn test_imported_urls_never_duplicate_the_dial() {
    let curated_urls: HashSet<String> = curated()
        .iter()
        .filter(|s| !s.stream_url.is_empty())
        .map(|s| normalize_stream_url(&s.stream_url))
        .collect();
    let imported_urls: Vec<String> = imported()
        .iter()
        .chain(web().iter())
        .map(|s| normalize_stream_url(&s.stream_url))
        .collect();
    let unique: HashSet<&String> = imported_urls.iter().collect();
    assert_eq!(imported_urls.len(), unique.len());
    assert!(!imported_urls.iter().any(|u| curated_urls.contains(u)));
}

#[test]
fn test_one_live365_station_is_one_stream_whatever_url_it_arrived_as() {
    let canonical = normalize_stream_url("https://streaming.live365.com/b09584_128mp3");
    assert_eq!(canonical, "streaming.live365.com/b09584");
    for alias in [
        "http://streaming.live365.com/b09584_128mp3",
        "http://streaming.live365.com/b09584_64aac",
        "https://ais-sa5.cdnstream1.com/b09584_128mp3",
        "https://das-edge14-live365-dal02.cdnstream.com/b09584",
        "https://streaming.live365.com/b09584?listenerId=Live365-AdBlock",
    ] {
        assert_eq!(normalize_stream_url(alias), canonical, "{alias}");
    }
    // A different station id stays a different station, and the fold never
    // reaches past Live365: two unrelated hosts sharing a path stay apart.
    assert_ne!(
        normalize_stream_url("https://streaming.live365.com/b09585_128mp3"),
        canonical
    );
    assert_ne!(
        normalize_stream_url("https://ice41.securenetsystems.net/1069_128"),
        normalize_stream_url("https://das-edge27-sa23-lax02.cdnstream.com/1069_128")
    );
}

#[test]
fn test_normalize_strips_scheme_and_trailing_slash_and_folds_host_case() {
    assert_eq!(
        normalize_stream_url("HTTPS://Example.COM/Live/"),
        "example.com/Live"
    );
    assert_eq!(normalize_stream_url("http://example.com"), "example.com");
    // The Shoutcast mount is the same stream as the root a player is served.
    assert_eq!(
        normalize_stream_url("http://example.com:8000/;"),
        normalize_stream_url("http://example.com:8000/")
    );
    assert_eq!(
        normalize_stream_url("http://example.com/stream/;"),
        "example.com/stream"
    );
    assert_eq!(
        normalize_stream_url("  http://example.com/a/b  "),
        "example.com/a/b"
    );
}

#[test]
fn test_directory_stations_sit_at_their_licensed_transmitter() {
    let fcc_placed: Vec<RadioStation> = imported()
        .into_iter()
        .filter(|s| s.id.starts_with("rb-fcc-"))
        .collect();
    assert!(fcc_placed.len() >= 500);
    assert!(fcc_placed
        .iter()
        .all(|s| s.lat.is_some() && s.lon.is_some()));
    assert!(fcc_placed.iter().all(|s| !s.call_sign.is_empty()));
    assert!(fcc_placed.iter().all(|s| s.range_miles > 0.0));
    // Continental US plus Alaska and Hawaii, and nothing in the ocean off
    // the coast of Africa: a transposed sign puts a station at (0, 0).
    assert!(fcc_placed
        .iter()
        .all(|s| (17.0..72.0).contains(&s.lat.unwrap())));
    assert!(fcc_placed
        .iter()
        .all(|s| (-180.0..-64.0).contains(&s.lon.unwrap())));
    // And the real data spreads across the bands rather than piling on one.
    let ranges: HashSet<u64> = fcc_placed.iter().map(|s| s.range_miles.to_bits()).collect();
    assert!(ranges.len() >= 4);
}

#[test]
fn test_live365_stations_are_stored_at_live365s_own_address() {
    assert_eq!(
        canonical_stream_url("http://ais-edge104-live365-dal02.cdnstream.com/a89824"),
        "https://streaming.live365.com/a89824"
    );
    assert_eq!(
        canonical_stream_url(
            "https://ais-edge105-live365-dal02.cdnstream.com/a02627?filetype=.mp3&_=1"
        ),
        "https://streaming.live365.com/a02627"
    );
    // The mount is kept exactly: a bitrate variant is still its own mount.
    assert_eq!(
        canonical_stream_url("http://streaming.live365.com/a86427_2"),
        "https://streaming.live365.com/a86427_2"
    );
    // Everything that is not a Live365 mount comes back untouched, including
    // the numeric mounts other broadcasters run on the same CDN.
    for other in [
        "https://ice41.securenetsystems.net/KAJN",
        "http://das-edge27-sa23-lax02.cdnstream.com/1069_128",
        "http://crystalout.surfernetwork.com:8001/KADA_MP3",
    ] {
        assert_eq!(canonical_stream_url(other), other);
    }
    for station in imported().iter().chain(web().iter()) {
        let host = station
            .stream_url
            .split_once("//")
            .map(|(_, rest)| rest)
            .unwrap_or(&station.stream_url)
            .split('/')
            .next()
            .unwrap()
            .to_lowercase();
        assert!(
            !host.contains("live365") || host == "streaming.live365.com",
            "{}",
            station.stream_url
        );
    }
}

#[test]
fn test_radiostorm_channels_are_curated_and_listed_once() {
    let curated: Vec<RadioStation> = curated()
        .into_iter()
        .filter(|s| s.id.starts_with("radiostorm-"))
        .collect();
    let names: HashSet<&str> = curated.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(
        names,
        HashSet::from([
            "Radiostorm At Work 104",
            "Radiostorm Rock 104",
            "Radiostorm Oldies 104",
            "Radiostorm Comedy 104",
        ])
    );
    assert!(curated.iter().all(|s| s.source_type == "web"));
    assert!(curated
        .iter()
        .all(|s| s.always_available && s.real_stream && s.supported));
    assert!(!curated.iter().any(|s| s.safe_for_streaming));
    let identities: Vec<String> = curated
        .iter()
        .map(|s| normalize_stream_url(&s.stream_url))
        .collect();
    let identity_set: HashSet<&String> = identities.iter().collect();
    assert_eq!(identities.len(), identity_set.len());
    // One dial listing each, and the directory's own rows for those four
    // channels are gone.
    let radio = dallas_radio();
    let listed: Vec<RadioStation> = radio
        .available_stations()
        .into_iter()
        .filter(|s| identity_set.contains(&normalize_stream_url(&s.stream_url)))
        .collect();
    assert_eq!(
        listed.len(),
        4,
        "{:?}",
        listed.iter().map(|s| &s.name).collect::<Vec<_>>()
    );
    let listed_ids: HashSet<&str> = listed.iter().map(|s| s.id.as_str()).collect();
    let curated_ids: HashSet<&str> = curated.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(listed_ids, curated_ids);
}

#[test]
fn test_terrestrial_names_never_lead_with_dial_junk() {
    // "& FM 95.7 & FM 93.1" reached a real drive's readout (2026-08-07):
    // the source cleaner strands conjunctions and band-first frequencies.
    let junk = regex::Regex::new(r"(?i)^(?:(?:FM|AM)\s*)?\d{2,4}(?:\.\d)?\s*(?:FM|AM)?$").unwrap();
    for station in imported() {
        assert!(
            !["&", ",", "-", "/", "and ", "And "]
                .iter()
                .any(|p| station.name.starts_with(p)),
            "{}",
            station.name
        );
        assert!(!junk.is_match(&station.name), "{}", station.name);
    }
}

#[test]
fn test_a_name_that_repeats_the_call_sign_speaks_once() {
    let doubled: Vec<RadioStation> = imported()
        .into_iter()
        .filter(|s| s.name.to_uppercase() == s.call_sign.to_uppercase())
        .collect();
    assert!(
        !doubled.is_empty(),
        "the catalog carries call-sign-only stations"
    );
    for station in doubled.iter().take(20) {
        assert_eq!(station.display_name(), station.call_sign);
    }
}

#[test]
fn test_a_name_that_says_its_call_sign_is_spoken_as_named() {
    let named = |call: &str, name: &str| RadioStation::new("x", name, call, "news", "test");
    assert_eq!(named("KXLU", "KXLU 88.9").display_name(), "KXLU 88.9");
    assert_eq!(
        named("KING-FM", "Classical KING FM 98.1").display_name(),
        "Classical KING FM 98.1"
    );
    assert_eq!(named("AFN", "AFN Tokyo").display_name(), "AFN Tokyo");
    assert_eq!(
        named("KAAY AM", "AM 1090 KAAY").display_name(),
        "AM 1090 KAAY"
    );
    // A call sign inside a longer word is not the call sign.
    assert_eq!(
        named("SAT", "Safe Satellite Fallback").display_name(),
        "SAT, Safe Satellite Fallback"
    );
    assert_eq!(
        named("SDPB", "Sioux Falls").display_name(),
        "SDPB, Sioux Falls"
    );
    for station in default_radio_catalog() {
        let display = station.display_name();
        let base = station
            .call_sign
            .split(|c: char| !c.is_alphanumeric())
            .next()
            .unwrap_or_default();
        if let Some(rest) = display.strip_prefix(base).filter(|_| !base.is_empty()) {
            assert!(
                !(rest.starts_with(", ") && rest.contains(base)),
                "{}: the call sign is spoken twice in {display:?}",
                station.id
            );
        }
    }
}

#[test]
fn test_web_station_names_carry_no_stream_jargon() {
    let jargon = regex::Regex::new(r"(?i)\b(?:kbps|kbit|aac|mp3|\d{2,3}\s?kb?)\b").unwrap();
    for station in web() {
        assert!(!jargon.is_match(&station.name), "{}", station.name);
    }
}

#[test]
fn test_web_band_sits_last_on_the_dial_and_jumpable() {
    let groups: HashSet<i32> = web().iter().map(dial_group).collect();
    assert_eq!(groups, HashSet::from([9]));
    assert_eq!(dial_category_name(9), "Web radio");
    // Everything with a place or a story sorts ahead of the web band.
    assert!(default_radio_catalog()
        .iter()
        .filter(|s| s.source_type != "web")
        .all(|s| dial_group(s) < 9));
    let radio = dallas_radio();
    let stations = radio.available_stations();
    let first_web = stations
        .iter()
        .position(|s| s.source_type == "web")
        .unwrap();
    assert!(stations[first_web..].iter().all(|s| s.source_type == "web"));
}

#[test]
fn test_streamer_safe_mode_hides_the_web_tier_too() {
    let radio = RadioState::new(catalog())
        .with_streamer_safe(true)
        .with_position(Some(DALLAS));
    assert!(!radio
        .available_stations()
        .iter()
        .any(|s| s.source_type == "web"));
}

#[test]
fn test_a_curated_web_station_does_not_reserve_every_call_sign_less_import() {
    // Web stations are named, not lettered. Curating four of them put an
    // empty string in the reserved call-sign set, which matched every
    // imported web station and silently emptied the whole band.
    let curated = curated();
    assert!(
        curated.iter().any(|s| s.call_sign.is_empty()),
        "the curated catalog carries web stations"
    );
    let imported = load_imported_stations(&default_data_root(), &curated).unwrap();
    assert!(imported.iter().filter(|s| s.source_type == "web").count() >= 3500);
}

#[test]
fn test_curated_call_signs_always_win() {
    let curated_bases: HashSet<String> = curated()
        .iter()
        .map(|s| call_sign_base(&s.call_sign))
        .collect();
    assert!(!imported()
        .iter()
        .any(|s| curated_bases.contains(&call_sign_base(&s.call_sign))));
    assert_eq!(call_sign_base("WNYC-FM"), "WNYC");
    assert_eq!(call_sign_base("wnyc fm"), "WNYC");
    assert_eq!(call_sign_base("  "), "");
}

#[test]
fn test_streamer_safe_mode_hides_every_imported_station() {
    let radio = RadioState::new(catalog())
        .with_streamer_safe(true)
        .with_position(Some(DALLAS));
    let stations = radio.available_stations();
    assert!(!stations.iter().any(|s| s.source_type == "imported"));
    // The built-in stations still fill the dial for a streaming driver.
    assert!(stations.iter().any(|s| !s.real_stream));
}

#[test]
fn test_imported_tier_plays_out_of_the_box() {
    let radio = RadioState::new(catalog()).with_position(Some(DALLAS));
    assert!(radio
        .available_stations()
        .iter()
        .any(|s| s.source_type == "imported"));
}

#[test]
fn test_imported_stations_come_in_near_their_transmitter() {
    let mut radio = dallas_radio();
    let in_reach: Vec<RadioStation> = radio
        .available_stations()
        .into_iter()
        .filter(|s| s.source_type == "imported")
        .collect();
    assert!(
        !in_reach.is_empty(),
        "Dallas should receive imported broadcast stations"
    );
    // And they stay local: none of them is receivable from the other coast.
    radio.update_position(Some((47.6062, -122.3321)), None); // Seattle
    let seattle_ids: HashSet<String> = radio
        .available_stations()
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert!(!in_reach.iter().all(|s| seattle_ids.contains(&s.id)));
}

#[test]
fn test_imported_station_spoken_text_is_clean() {
    // Curated stations may speak their website as a source note; the imported
    // tier's spoken lines carry no URLs or stream jargon at all.
    let mut radio = RadioState::new(imported()).with_position(Some(DALLAS));
    for line in radio.station_list_lines(30, None) {
        assert!(!line.to_lowercase().contains("http"));
        assert!(!line.contains('\n') && !line.contains('\t'));
    }
    for station in imported() {
        assert!(!station.call_sign.contains('-'));
        assert!(!station.format.is_empty());
        assert!(!station.source.is_empty());
    }
}

#[test]
fn test_station_list_lines_speak_distance_and_source() {
    let near = terrestrial_at("fix-near", "WZZZ", 0.0);
    let mut radio = RadioState::new(vec![near])
        .with_station_id("fix-near")
        .with_position(Some(DALLAS));
    let lines = radio.station_list_lines(12, None);
    assert_eq!(
        lines,
        vec!["current, WZZZ FM: country, strong signal, 0 miles away. Source: reception fixture."]
    );
    let spoken = radio.station_list_lines(12, Some(&|miles: f64| format!("{miles:.1} mi")));
    assert!(spoken[0].contains(", 0.0 mi away"));
}

// -- test_radio_multi_site.py -----------------------------------------------------

const SITE_A_POS: (f64, f64) = (40.000, -75.000);
const SITE_B_POS: (f64, f64) = (40.000, -74.850); // ~7 miles east of Site A; ranges overlap both ways

fn fixture_site_a() -> RadioStation {
    RadioStation {
        stream_url: "https://fixture.test/stream".into(),
        stream_format: "mp3".into(),
        lat: Some(SITE_A_POS.0),
        lon: Some(SITE_A_POS.1),
        range_miles: 25.0,
        real_stream: true,
        safe_for_streaming: false,
        supported: true,
        ..RadioStation::new(
            "fixture-site-a",
            "Fixture Public Radio",
            "KFIX",
            "news",
            "fixture network",
        )
    }
}

// Same stream, a different real transmitter: same identity as Site A.
fn fixture_site_b() -> RadioStation {
    RadioStation {
        id: "fixture-site-b".into(),
        lat: Some(SITE_B_POS.0),
        lon: Some(SITE_B_POS.1),
        ..fixture_site_a()
    }
}

// A genuinely different station in the same dial category, for the dead-
// stream handover test: distinct stream_url, so never grouped with A/B.
fn fixture_lone() -> RadioStation {
    RadioStation {
        id: "fixture-lone-station".into(),
        name: "Fixture Lone Station".into(),
        call_sign: "KLON".into(),
        stream_url: "https://fixture.test/lone-stream".into(),
        range_miles: 60.0,
        ..fixture_site_a()
    }
}

/// The safety sentinels only -- not the full real catalog.
fn fixture_catalog() -> Vec<RadioStation> {
    let mut catalog: Vec<RadioStation> = default_radio_catalog()
        .iter()
        .filter(|s| s.id == SAFE_ROUTE_PLAYLIST || s.id == SAFE_FALLBACK_STATION_ID)
        .cloned()
        .collect();
    catalog.extend([fixture_site_a(), fixture_site_b(), fixture_lone()]);
    catalog
}

fn identity_receptions(radio: &RadioState) -> Vec<RadioReception> {
    let identity = station_identity(&fixture_site_a());
    radio
        .receivable_stations()
        .into_iter()
        .filter(|r| station_identity(&r.station) == identity)
        .collect()
}

#[test]
fn test_multi_site_station_lists_once_on_the_dial() {
    let radio = RadioState::new(fixture_catalog()).with_position(Some(SITE_A_POS));
    assert_eq!(identity_receptions(&radio).len(), 1);
}

#[test]
fn test_multi_site_station_lists_the_strongest_site() {
    let at_a = RadioState::new(fixture_catalog()).with_position(Some(SITE_A_POS));
    assert_eq!(identity_receptions(&at_a)[0].station.id, "fixture-site-a");

    let at_b = RadioState::new(fixture_catalog()).with_position(Some(SITE_B_POS));
    assert_eq!(identity_receptions(&at_b)[0].station.id, "fixture-site-b");
}

#[test]
fn test_multi_site_station_hands_over_as_the_truck_moves() {
    let mut radio = RadioState::new(fixture_catalog())
        .with_station_id("fixture-site-a")
        .with_position(Some(SITE_A_POS));
    assert_eq!(radio.current_station().id, "fixture-site-a");

    radio.update_position(Some(SITE_B_POS), None);
    let current = radio.current_station();

    // The handover is automatic -- no re-tune needed -- and it persists on
    // station_id so the next call (menu redraw, play(), status_text) agrees.
    assert_eq!(current.id, "fixture-site-b");
    assert_eq!(radio.station_id, "fixture-site-b");
    // Still exactly one dial entry at the new position, not two mid-transition.
    let receptions = identity_receptions(&radio);
    assert_eq!(receptions.len(), 1);
    assert_eq!(receptions[0].station.id, "fixture-site-b");
}

#[test]
fn test_tuned_station_reads_the_dial_without_moving_it() {
    // `tuned_station` is what the presence builders read sixty times a
    // second. It must give the same answer as `current_station` -- through
    // a handover and through a fallback, the two paths that make
    // `current_station` need `&mut` -- while leaving station_id where it
    // found it. Before it existed the caller cloned the whole radio to get
    // this, which cost 2.4 ms of every driven frame.
    let mut radio = RadioState::new(fixture_catalog())
        .with_station_id("fixture-site-a")
        .with_position(Some(SITE_A_POS));
    assert_eq!(radio.tuned_station().id, radio.current_station().id);

    // A handover: site B is the loud one now.
    radio.update_position(Some(SITE_B_POS), None);
    radio.station_id = "fixture-site-a".to_string();
    let read = radio.tuned_station();
    assert_eq!(read.id, "fixture-site-b");
    assert_eq!(
        radio.station_id, "fixture-site-a",
        "reading the dial must not re-point it"
    );
    assert_eq!(radio.current_station().id, read.id);
    assert_eq!(radio.station_id, "fixture-site-b"); // the writer still writes

    // A fallback: nothing on the dial answers to that id at all.
    radio.station_id = "not-a-station".to_string();
    let read = radio.tuned_station();
    assert_eq!(read.id, radio.fallback_station().id);
    assert_eq!(
        radio.station_id, "not-a-station",
        "reading the dial must not re-point it"
    );
    assert_eq!(radio.current_station().id, read.id);
    assert_eq!(radio.station_id, read.id);
}

#[test]
fn test_multi_site_dead_stream_still_hands_over_to_a_different_station() {
    // Only Site A's id ever gets a play attempt (it's strongest at
    // SITE_A_POS); the assertion on the eventual station proves the failure
    // cascades to Site B too, rather than the radio quietly retrying the
    // same dead stream under B's id.
    let mut radio = RadioState::new(fixture_catalog())
        .with_station_id("fixture-site-a")
        .with_position(Some(SITE_A_POS));
    let mut backend = RecordingBackend::failing(&["fixture-site-a"]);

    let action = radio.play(Some(&mut backend), "");

    assert!(action.fallback_used);
    assert_eq!(action.station.id, "fixture-lone-station"); // same band, not a sibling site
    assert_eq!(radio.station_id, "fixture-lone-station");
    assert_eq!(
        backend.played,
        vec![("fixture-lone-station".to_string(), radio.volume)]
    );
    // The whole identity is off the dial, not just the site that failed.
    assert!(radio.unplayable_ids.contains("fixture-site-a"));
    assert!(radio.unplayable_ids.contains("fixture-site-b"));
    assert!(identity_receptions(&radio).is_empty());
}

#[test]
fn test_multi_site_favorite_survives_a_handover() {
    let mut radio = RadioState::new(fixture_catalog())
        .with_station_id("fixture-site-a")
        .with_position(Some(SITE_A_POS));
    radio.toggle_favorite();
    assert!(radio.favorite_ids.contains("fixture-site-a"));
    assert!(radio.favorite_ids.contains("fixture-site-b")); // saved as one station, not one site

    radio.update_position(Some(SITE_B_POS), None);
    let current = radio.current_station();
    assert_eq!(current.id, "fixture-site-b");
    assert!(radio.favorite_ids.contains(&current.id));
}

#[test]
fn test_first_refusal_retries_before_the_second_writes_the_station_off() {
    // owner, 2026-08-22: one miss is a slow server, not a dead station.
    struct FlakyOnce {
        failed_once: bool,
        played: Vec<String>,
    }
    impl RadioPlaybackBackend for FlakyOnce {
        fn play_station(
            &mut self,
            station: &RadioStation,
            _volume: f64,
        ) -> Result<(), RadioPlaybackError> {
            if station.id == "fixture-lone-station" && !self.failed_once {
                self.failed_once = true;
                return Err(RadioPlaybackError("slow".into()));
            }
            self.played.push(station.id.clone());
            Ok(())
        }
        fn stop_radio(&mut self) {}
    }
    let mut radio = RadioState::new(fixture_catalog())
        .with_station_id("fixture-lone-station")
        .with_position(Some(SITE_A_POS));
    let mut backend = FlakyOnce {
        failed_once: false,
        played: Vec::new(),
    };
    let action = radio.play(Some(&mut backend), "Tuned to KLON, Fixture Lone Station.");
    assert!(action.retried);
    assert!(!action.fallback_used);
    assert_eq!(
        action.message,
        "KLON, Fixture Lone Station is slow to answer. Trying again."
    );
    assert_eq!(backend.played, vec!["fixture-lone-station"]);
    assert!(!radio.unplayable_ids.contains("fixture-lone-station"));
}

#[test]
fn test_play_message_never_names_the_station_twice() {
    let mut radio = RadioState::new(fixture_catalog())
        .with_station_id("fixture-lone-station")
        .with_position(Some(SITE_A_POS));
    let action = radio.tune(0, None);
    assert_eq!(
        action.message,
        "Tuned to KLON, Fixture Lone Station. news. strong signal."
    );
    let action = radio.play(None, "");
    assert_eq!(
        action.message,
        "KLON, Fixture Lone Station. news. strong signal."
    );
}

// -- test_radio_playlists.py (pure parts) ----------------------------------------

fn write(path: &Path, text: &str) -> PathBuf {
    std::fs::write(path, text).unwrap();
    path.to_path_buf()
}

fn joined(base: &Path, parts: &[&str]) -> String {
    let mut path = base.to_path_buf();
    for part in parts {
        path.push(part);
    }
    py_path_string(&path.to_string_lossy())
}

#[test]
fn test_parse_m3u_resolves_paths_and_reads_title() {
    let tmp = tempfile::tempdir().unwrap();
    let m3u = write(
        &tmp.path().join("road.m3u"),
        &[
            "#EXTM3U",
            "#PLAYLIST: Norm's Road Mix",
            "#EXTINF:245,Artist - Song",
            "songs/first.mp3",
            "",
            r"C:\music\second.flac",
            "https://example.com/stream.mp3",
            "# a comment",
            "third.opus",
        ]
        .join("\n"),
    );
    let (entries, title) = parse_m3u(&m3u);
    assert_eq!(title, "Norm's Road Mix");
    // Files and streams keep the player's own order: a mixed playlist is a
    // sequence they chose, not two piles.
    assert_eq!(
        entries,
        vec![
            joined(tmp.path(), &["songs", "first.mp3"]),
            r"C:\music\second.flac".to_string(),
            "https://example.com/stream.mp3".to_string(),
            joined(tmp.path(), &["third.opus"]),
        ]
    );
}

#[test]
fn test_a_stream_only_m3u_still_builds_a_station() {
    // The owner's bug: an internet-radio export is all URLs.
    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("webmix.m3u"),
        "#EXTM3U\n#EXTINF:-1,Some Station\nhttp://example.com/one\nhttps://example.com/two\n",
    );
    let stations = load_personal_playlists(tmp.path());
    assert_eq!(stations.len(), 1);
    let station = &stations[0];
    assert_eq!(
        station.playlist_entries,
        vec!["http://example.com/one", "https://example.com/two"]
    );
    assert_eq!(station.name, "webmix");
}

#[test]
fn test_parse_pls_reads_numbered_entries_in_order() {
    let tmp = tempfile::tempdir().unwrap();
    let pls = write(
        &tmp.path().join("stations.pls"),
        &[
            "[playlist]",
            "NumberOfEntries=3",
            "File2=https://example.com/second",
            "Title2=Second",
            "File1=songs/first.mp3",
            "Title1=First",
            "File3=/music/third.flac",
            "Version=2",
        ]
        .join("\n"),
    );
    let (entries, title) = parse_playlist_file(&pls);
    assert_eq!(
        entries,
        vec![
            joined(tmp.path(), &["songs", "first.mp3"]),
            "https://example.com/second".to_string(),
            py_path_string("/music/third.flac"),
        ]
    );
    // Several entries means Title1 titles a track, not the playlist.
    assert_eq!(title, "");
}

#[test]
fn test_a_one_station_pls_is_named_by_its_title() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("station.pls"),
        "[playlist]\nFile1=http://example.com/live\nTitle1=Night Owl Radio\nLength1=-1\n",
    );
    let stations = load_personal_playlists(tmp.path());
    assert_eq!(stations.len(), 1);
    assert_eq!(stations[0].name, "Night Owl Radio");
    assert_eq!(
        stations[0].playlist_entries,
        vec!["http://example.com/live"]
    );
}

#[test]
fn test_playlist_entries_are_absolute_on_the_machine_that_wrote_them() {
    // A Windows playlist read on Linux keeps its drive paths.
    for (line, absolute) in [
        ("songs/first.mp3", false),
        ("third.opus", false),
        ("../next door/track.mp3", false),
        ("/home/driver/music/song.mp3", true),
        (r"C:\music\second.flac", true),
        ("D:/media/third.flac", true),
        (r"\\media-box\share\fourth.mp3", true),
    ] {
        assert_eq!(absolute_anywhere(line), absolute, "{line}");
    }
}

#[test]
fn test_parse_m3u_survives_a_missing_file() {
    let tmp = tempfile::tempdir().unwrap();
    assert_eq!(
        parse_m3u(&tmp.path().join("gone.m3u")),
        (Vec::new(), String::new())
    );
}

#[test]
fn test_load_personal_playlists_builds_stations() {
    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("b-mix.m3u"),
        "#PLAYLIST:Night Drive\none.mp3\n",
    );
    write(&tmp.path().join("a-mix.m3u"), "two.mp3\nthree.mp3\n");
    write(&tmp.path().join("empty.m3u"), "#EXTM3U\n");
    let stations = load_personal_playlists(tmp.path());
    let names: Vec<&str> = stations.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["a-mix", "Night Drive"]);
    for station in &stations {
        assert_eq!(station.source_type, PERSONAL_PLAYLIST_SOURCE_TYPE);
        assert!(station.always_available);
        assert!(!station.safe_for_streaming);
        assert!(!station.playlist_entries.is_empty());
        assert!(station.display_name().starts_with("Playlist, "));
    }
    assert_ne!(stations[0].id, stations[1].id);
}

#[test]
fn test_an_unusable_playlist_warns_and_builds_no_station() {
    // Silence was the whole diagnosis before: no station, no log, no word.
    // (The log lines themselves are asserted by the Python suite's caplog;
    // here the dial outcome is pinned.)
    let tmp = tempfile::tempdir().unwrap();
    write(
        &tmp.path().join("empty.m3u"),
        "#EXTM3U\n# nothing but comments\n",
    );
    write(
        &tmp.path().join("good.m3u"),
        "one.mp3\nhttps://example.com/live\n",
    );
    let stations = load_personal_playlists(tmp.path());
    let names: Vec<&str> = stations.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, vec!["good"]);
}

#[test]
fn test_load_personal_playlists_creates_the_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let target = tmp.path().join("Playlists");
    assert!(!target.exists());
    assert!(load_personal_playlists(&target).is_empty());
    assert!(target.is_dir(), "an empty folder invites dropping files in");
}

#[test]
fn test_same_titles_get_distinct_station_ids() {
    let tmp = tempfile::tempdir().unwrap();
    write(&tmp.path().join("one.m3u"), "#PLAYLIST:Mix\na.mp3\n");
    write(&tmp.path().join("two.m3u"), "#PLAYLIST:Mix\nb.mp3\n");
    let ids: Vec<String> = load_personal_playlists(tmp.path())
        .into_iter()
        .map(|s| s.id)
        .collect();
    assert_eq!(ids.len(), 2);
    assert_eq!(ids.iter().collect::<HashSet<_>>().len(), 2);
}

fn playlist_station(files: &[&str]) -> RadioStation {
    RadioStation {
        source_type: PERSONAL_PLAYLIST_SOURCE_TYPE.into(),
        safe_for_streaming: false,
        always_available: true,
        playlist_entries: files.iter().map(|f| f.to_string()).collect(),
        ..RadioStation::new(
            "playlist-test",
            "Test Mix",
            "Playlist",
            "personal playlist",
            "your playlist file test.m3u",
        )
    }
}

#[test]
fn test_personal_playlists_ride_the_streamer_safe_gate() {
    let mut catalog = catalog();
    catalog.push(playlist_station(&["a.mp3"]));
    let safe = RadioState::new(catalog.clone()).with_streamer_safe(true);
    assert!(!safe
        .available_stations()
        .iter()
        .any(|s| s.id == "playlist-test"));
    // Streamer-safe off is enough on its own: it is the one licensing gate.
    let open_dial = RadioState::new(catalog);
    assert!(open_dial
        .available_stations()
        .iter()
        .any(|s| s.id == "playlist-test"));
}

#[test]
fn test_a_refused_playlist_says_its_tracks_would_not_open() {
    // "Off the air" is a broadcast's failure, not a folder's.
    struct RefusingBackend;
    impl RadioPlaybackBackend for RefusingBackend {
        fn play_station(
            &mut self,
            station: &RadioStation,
            _volume: f64,
        ) -> Result<(), RadioPlaybackError> {
            if station.source_type == PERSONAL_PLAYLIST_SOURCE_TYPE {
                return Err(RadioPlaybackError(
                    "no playable entry in this playlist".into(),
                ));
            }
            Ok(())
        }
        fn stop_radio(&mut self) {}
    }
    let mut catalog = catalog();
    catalog.push(playlist_station(&["a.mp3"]));
    let mut state = RadioState::new(catalog);
    // The first refusal earns an immediate retry; that refuses too, so the
    // same call writes the playlist off.
    let action = state.select_station("playlist-test", Some(&mut RefusingBackend));
    assert!(!action.retried);
    assert!(action
        .message
        .contains("None of the tracks in Playlist, Test Mix would open"));
    assert!(!action.message.contains("off the air"));
    assert!(action.fallback_used);
}

#[test]
fn test_reloading_the_folder_puts_a_new_playlist_on_the_dial() {
    // A playlist added mid-run used to need a whole new drive to be seen.
    let tmp = tempfile::tempdir().unwrap();
    let mut state = RadioState::new(catalog());
    state.mark_unplayable("playlist-late-mix");
    write(
        &tmp.path().join("late-mix.m3u"),
        "https://example.com/live\n",
    );
    state.reload_personal_playlists(tmp.path());
    let playlists: Vec<&RadioStation> = state
        .catalog
        .iter()
        .filter(|s| s.source_type == PERSONAL_PLAYLIST_SOURCE_TYPE)
        .collect();
    assert_eq!(
        playlists
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["late-mix"]
    );
    // An earlier refusal must not keep the repaired playlist off the dial.
    assert!(!state.unplayable_ids.contains("playlist-late-mix"));
    assert!(state
        .available_stations()
        .iter()
        .any(|s| s.id == "playlist-late-mix"));
    // Reloading twice does not stack duplicate stations.
    state.reload_personal_playlists(tmp.path());
    assert_eq!(
        state
            .catalog
            .iter()
            .filter(|s| s.source_type == PERSONAL_PLAYLIST_SOURCE_TYPE)
            .count(),
        1
    );
}

#[test]
fn test_playlists_sit_between_built_in_and_terrestrial_on_the_dial() {
    let mut catalog = catalog();
    catalog.push(playlist_station(&["a.mp3"]));
    let state = RadioState::new(catalog).with_position(Some((33.45, -112.07))); // Phoenix: terrestrial in range
    let groups: Vec<i32> = state
        .receivable_stations()
        .iter()
        .map(|r| dial_group(&r.station))
        .collect();
    let mut sorted = groups.clone();
    sorted.sort();
    assert_eq!(groups, sorted, "dial order is category order");
    assert!(groups.contains(&2), "the personal playlist is on the dial");
    let index = |g: i32| groups.iter().position(|x| *x == g).unwrap();
    assert!(index(2) > index(1));
    // Terrestrial moved to group 4 when Favorites took 3.
    assert!(index(2) < index(4));
}

#[test]
fn test_tune_category_leaps_and_speaks_the_category() {
    let mut state = RadioState::new(catalog()).with_position(Some((33.45, -112.07)));
    let action = state.tune_category(1, None);
    assert!(action
        .message
        .starts_with("Freight Fate stations. Tuned to "));
    let action = state.tune_category(1, None);
    assert!(action.message.starts_with("Terrestrial. Tuned to "));
    // And back down the same rung.
    let action = state.tune_category(-1, None);
    assert!(action
        .message
        .starts_with("Freight Fate stations. Tuned to "));
}

#[test]
fn test_tune_category_wraps_and_never_lands_mid_category() {
    let mut state = RadioState::new(catalog()); // the out-of-the-box dial
    let receptions = state.receivable_stations();
    let mut first_by_group: IndexMapLite = IndexMapLite::default();
    for reception in &receptions {
        first_by_group
            .insert_if_absent(dial_group(&reception.station), reception.station.id.clone());
    }
    let mut seen = Vec::new();
    for _ in 0..first_by_group.len() {
        let action = state.tune_category(1, None);
        seen.push(action.station.id);
    }
    // One full lap visits each category's first station exactly once --
    // except the silent satellite's slot, which now plays (and reports)
    // the audible fallback instead of silence.
    seen.sort();
    let mut expected: Vec<String> = first_by_group.values();
    for id in &mut expected {
        if id == SAFE_FALLBACK_STATION_ID {
            *id = state.fallback_station().id;
        }
    }
    expected.sort();
    assert_eq!(seen, expected);
}

/// The little ordered map the category-lap test needs.
#[derive(Default)]
struct IndexMapLite(Vec<(i32, String)>);

impl IndexMapLite {
    fn insert_if_absent(&mut self, key: i32, value: String) {
        if !self.0.iter().any(|(k, _)| *k == key) {
            self.0.push((key, value));
        }
    }
    fn len(&self) -> usize {
        self.0.len()
    }
    fn values(&self) -> Vec<String> {
        self.0.iter().map(|(_, v)| v.clone()).collect()
    }
}

#[test]
fn test_search_finds_the_whole_dial_in_range_first() {
    let mut all = catalog();
    all.push(terrestrial_at("fix-near", "WZZZ", 0.0));
    let radio = RadioState::new(all).with_position(Some(DALLAS));
    let (hits, total) = radio.search("WZZZ FM", 40);
    assert!(total >= 1);
    assert_eq!(hits[0].0.id, "fix-near");
    assert!(hits[0].1.is_some());
    assert_eq!(radio.search("   ", 40), (Vec::new(), 0));
    let (capped, total) = radio.search("radio", 5);
    assert_eq!(capped.len(), 5);
    assert!(total > 5);
    assert_eq!(radio.band_name(&hits[0].0), "Terrestrial");
}

#[test]
fn search_page_bounds_cover_every_hit_once() {
    for total in [0usize, 1, 25, 26, 40, 41, 50, 51, 52] {
        let pages = search_page_count(total, 25);
        let mut covered = 0;
        for page in 0..pages {
            let (start, end) = search_page_bounds(total, page, 25);
            assert_eq!(start, covered, "total {total} page {page}");
            covered = end;
        }
        assert_eq!(covered, total);
    }
    assert_eq!(search_page_bounds(52, 1, 25), (25, 50));
    assert_eq!(search_page_bounds(52, 9, 25), (50, 52));
}

fn web_fixture(station_id: &str, name: &str, url: &str) -> RadioStation {
    RadioStation {
        source_type: "web".to_string(),
        stream_url: url.to_string(),
        real_stream: true,
        ..RadioStation::new(station_id, name, "", "pop", "browse fixture")
    }
}

#[test]
fn browse_lists_in_range_first_terrestrial_before_web_and_dedupes() {
    let mut all = vec![
        terrestrial_at("br-far", "KFAR", 20.0),
        terrestrial_at("br-near", "KNEA", 0.0),
        web_fixture("br-web-a", "Zed Web", "https://example.test/live"),
        web_fixture("br-web-b", "Zed Web copy", "https://example.test/live"),
    ];
    all.extend(catalog());
    let radio = RadioState::new(all).with_position(Some(DALLAS));
    let everything = radio.browse(None);
    let pos = |id: &str| everything.iter().position(|(s, _)| s.id == id).unwrap();
    // Local terrestrial ahead of web, and in-range ahead of out-of-range.
    assert!(pos("br-near") < pos("br-web-a"));
    assert!(pos("br-web-a") < pos("br-far"));
    assert!(everything[pos("br-near")].1.is_some());
    assert!(everything[pos("br-far")].1.is_none());
    // The repeated stream is one row.
    let copies = everything
        .iter()
        .filter(|(s, _)| s.id == "br-web-a" || s.id == "br-web-b")
        .count();
    assert_eq!(copies, 1);
}

#[test]
fn browse_by_category_keeps_to_one_group_with_out_of_range_included() {
    let mut all = vec![
        terrestrial_at("br-far", "KFAR", 20.0),
        terrestrial_at("br-near", "KNEA", 0.0),
        web_fixture("br-web", "Zed Web", "https://example.test/live"),
    ];
    all.extend(catalog());
    let radio = RadioState::new(all).with_position(Some(DALLAS));
    let terrestrial = radio.browse(Some(TERRESTRIAL_GROUP));
    assert!(terrestrial
        .iter()
        .all(|(s, _)| dial_group(s) == TERRESTRIAL_GROUP));
    let first_out = terrestrial.iter().position(|(_, r)| r.is_none()).unwrap();
    assert!(terrestrial[..first_out].iter().all(|(_, r)| r.is_some()));
    assert!(terrestrial.iter().any(|(s, _)| s.id == "br-far"));
    let categories = radio.browse_categories();
    let (_, name, count) = categories
        .iter()
        .find(|(g, _, _)| *g == TERRESTRIAL_GROUP)
        .copied()
        .unwrap();
    assert_eq!(name, "Terrestrial");
    assert_eq!(count, terrestrial.len());
    assert!(categories.iter().any(|(g, _, _)| *g == 9));
    assert!(categories.windows(2).all(|w| w[0].0 < w[1].0));
}
