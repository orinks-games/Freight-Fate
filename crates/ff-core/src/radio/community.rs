//! Community stations: radio stations players suggested on orinks.net and
//! the owner accepted, downloaded at launch so a new station reaches the
//! dial without a game release.
//!
//! The game crate fetches the list off-loop and keeps the last good copy in
//! the saves folder ([`COMMUNITY_STATIONS_FILE`]); this module only reads
//! that copy and decides which rows may join the dial. A server row is data
//! from the network, so nothing in it is trusted to change how the dial
//! behaves: every row is a real stream that is never streamer-safe, and one
//! that claims a transmitter must carry all of it or it plays everywhere, the
//! way web radio does. The shipped catalog always wins: a community row that
//! names a station the dial already has (same id, same stream, same call
//! sign) is dropped, so folding accepted stations into a release never puts
//! one on the dial twice.
//!
//! Accepted stations also ship: `tools/fold_community_stations.py` copies the
//! site's list into `data/radio_community.json` before each nightly, so a
//! first launch, an offline one, or a copy with the orinks.net services off
//! still has every station accepted up to its build. That file is the third
//! tier of the shipped catalog, under the curated and imported ones
//! ([`load_shipped_community_stations`]), and goes through the same checks
//! as the download: it is the site's data, copied.

use std::collections::HashSet;
use std::path::Path;

use serde_json::Value;

use super::{call_sign_base, station_from_dict, station_identity, RadioStation};

/// The shipped copy of the accepted list, under the data root.
pub const RADIO_COMMUNITY_RESOURCE: &str = "radio_community.json";

/// The downloaded list's file name in the saves folder.
pub const COMMUNITY_STATIONS_FILE: &str = "community_stations.json";

/// Every community station id starts with this, so a row can never take an
/// id the shipped catalog or a playlist uses.
pub const COMMUNITY_ID_PREFIX: &str = "community-";

/// The most rows one list may put on the dial. Far above what review will
/// accept in years; it exists so a broken or hostile reply cannot make the
/// dial unusable.
pub const MAX_COMMUNITY_STATIONS: usize = 2000;

/// One server row as a dial station, or `None` when it is not a usable one.
fn community_station(row: &Value) -> Option<RadioStation> {
    let mut station = station_from_dict(row);
    let stream = station.stream_url.trim().to_ascii_lowercase();
    if !station.id.starts_with(COMMUNITY_ID_PREFIX)
        || station.name.trim().is_empty()
        || !(stream.starts_with("http://") || stream.starts_with("https://"))
    {
        return None;
    }
    station.real_stream = true;
    station.supported = true;
    station.safe_for_streaming = false;
    station.fallback = false;
    station.playlist.clear();
    station.host.clear();
    station.track_key.clear();
    let located = station.lat.is_some_and(|lat| (-90.0..=90.0).contains(&lat))
        && station
            .lon
            .is_some_and(|lon| (-180.0..=180.0).contains(&lon))
        && station.range_miles > 0.0;
    if station.source_type == "imported" && located {
        station.always_available = false;
    } else {
        station.source_type = "web".to_string();
        station.always_available = true;
        station.lat = None;
        station.lon = None;
        station.range_miles = 0.0;
        station.frequency_mhz = 0.0;
        station.site_elev_ft = None;
    }
    Some(station)
}

/// The rows of a downloaded list that are usable stations, in list order.
/// `None` when the text is not a station list at all.
pub fn parse_community_stations(text: &str) -> Option<Vec<RadioStation>> {
    let data: Value = serde_json::from_str(text).ok()?;
    let rows = data.get("stations")?.as_array()?;
    Some(
        rows.iter()
            .filter_map(community_station)
            .take(MAX_COMMUNITY_STATIONS)
            .collect(),
    )
}

/// The saved copy, or nothing when there is none or it cannot be read.
pub fn load_community_stations(path: &Path) -> Vec<RadioStation> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_community_stations(&text).unwrap_or_else(|| {
        log::warn!(
            "Community station list {} is unreadable; skipping it",
            path.display()
        );
        Vec::new()
    })
}

/// The shipped tier: the accepted stations this build carries that are not
/// already on `dial` (the curated and imported tiers). A build without the
/// file has none; a broken one is logged and skipped, since the dial works
/// without it.
pub fn load_shipped_community_stations(
    data_root: &Path,
    dial: &[RadioStation],
) -> Vec<RadioStation> {
    let path = data_root.join(RADIO_COMMUNITY_RESOURCE);
    let Some(text) = crate::data::data_resources::read_text_at(&path) else {
        return Vec::new();
    };
    match parse_community_stations(&text) {
        Some(stations) => new_community_stations(dial, stations),
        None => {
            log::warn!("{} is unreadable; skipping it", path.display());
            Vec::new()
        }
    }
}

/// The community stations that are not already on `dial`, nor repeats of
/// one another.
pub fn new_community_stations(
    dial: &[RadioStation],
    community: Vec<RadioStation>,
) -> Vec<RadioStation> {
    let mut ids: HashSet<String> = dial.iter().map(|s| s.id.clone()).collect();
    let mut streams: HashSet<String> = dial.iter().map(station_identity).collect();
    let mut call_signs: HashSet<String> = dial
        .iter()
        .map(|s| call_sign_base(&s.call_sign))
        .filter(|base| !base.is_empty())
        .collect();
    let mut kept = Vec::new();
    for station in community {
        let call_sign = call_sign_base(&station.call_sign);
        if ids.contains(&station.id)
            || streams.contains(&station_identity(&station))
            || (!call_sign.is_empty() && call_signs.contains(&call_sign))
        {
            continue;
        }
        ids.insert(station.id.clone());
        streams.insert(station_identity(&station));
        if !call_sign.is_empty() {
            call_signs.insert(call_sign);
        }
        kept.push(station);
    }
    kept
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row(id: &str, name: &str, stream: &str) -> Value {
        json!({
            "id": id, "name": name, "call_sign": "", "format": "eclectic",
            "source_type": "web", "stream_url": stream, "stream_format": "mp3",
            "real_stream": true, "safe_for_streaming": false, "always_available": true,
        })
    }

    fn list(rows: Vec<Value>) -> String {
        json!({ "schema": 1, "stations": rows }).to_string()
    }

    #[test]
    fn web_rows_join_the_web_band_everywhere() {
        let stations = parse_community_stations(&list(vec![row(
            "community-abc",
            "Night Owl Radio",
            "https://s.example/live",
        )]))
        .unwrap();
        assert_eq!(stations.len(), 1);
        let station = &stations[0];
        assert_eq!(station.name, "Night Owl Radio");
        assert_eq!(station.source_type, "web");
        assert!(station.always_available && station.real_stream);
        assert!(!station.safe_for_streaming);
    }

    #[test]
    fn a_row_cannot_pose_as_something_else() {
        let mut sneaky = row("community-x", "Safe Satellite", "https://s.example/a");
        sneaky["safe_for_streaming"] = json!(true);
        sneaky["fallback"] = json!(true);
        sneaky["playlist"] = json!("route");
        let stations = parse_community_stations(&list(vec![
            sneaky,
            row("route_playlist", "Roadhouse", "https://s.example/b"),
            row("community-y", "No Stream", "file:///etc/passwd"),
            row("community-z", "  ", "https://s.example/c"),
        ]))
        .unwrap();
        assert_eq!(stations.len(), 1);
        let station = &stations[0];
        assert!(!station.safe_for_streaming && !station.fallback);
        assert!(station.playlist.is_empty());
    }

    #[test]
    fn a_terrestrial_row_needs_its_whole_transmitter() {
        let mut located = row("community-t1", "The Cat", "https://s.example/cat");
        located["source_type"] = json!("imported");
        located["call_sign"] = json!("KWSC-FM");
        located["lat"] = json!(42.24);
        located["lon"] = json!(-97.01);
        located["range_miles"] = json!(30.0);
        let mut half = located.clone();
        half["id"] = json!("community-t2");
        half["stream_url"] = json!("https://s.example/other");
        half["range_miles"] = json!(0.0);
        let stations = parse_community_stations(&list(vec![located, half])).unwrap();
        assert_eq!(stations[0].source_type, "imported");
        assert!(!stations[0].always_available);
        assert_eq!(stations[0].lat, Some(42.24));
        assert_eq!(stations[1].source_type, "web");
        assert!(stations[1].always_available);
        assert_eq!(stations[1].lat, None);
    }

    #[test]
    fn the_shipped_dial_wins_every_collision() {
        let dial = vec![
            RadioStation {
                stream_url: "http://s.example/live/".to_string(),
                ..RadioStation::new("rb-1", "Directory Copy", "", "", "")
            },
            RadioStation::new("wnyc", "WNYC", "WNYC-FM", "", ""),
        ];
        let mut call_sign_twin = row("community-c", "WNYC again", "https://s.example/wnyc");
        call_sign_twin["call_sign"] = json!("WNYC");
        let community = parse_community_stations(&list(vec![
            row("community-a", "Same Stream", "https://s.example/live"),
            call_sign_twin,
            row("community-b", "Fresh", "https://fresh.example/live"),
            row("community-b2", "Fresh Twin", "http://fresh.example/live/"),
        ]))
        .unwrap();
        let kept = new_community_stations(&dial, community);
        let names: Vec<&str> = kept.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["Fresh"]);
    }

    #[test]
    fn the_shipped_tier_reads_like_the_download() {
        let dir = tempfile::tempdir().unwrap();
        assert!(load_shipped_community_stations(dir.path(), &[]).is_empty());
        let mut sneaky = row("community-s", "Shipped", "https://s.example/shipped");
        sneaky["safe_for_streaming"] = json!(true);
        std::fs::write(
            dir.path().join(RADIO_COMMUNITY_RESOURCE),
            list(vec![
                sneaky,
                row("community-d", "Dial Twin", "https://s.example/dial"),
            ]),
        )
        .unwrap();
        let dial = vec![RadioStation {
            stream_url: "https://s.example/dial".to_string(),
            ..RadioStation::new("rb-1", "Directory Copy", "", "", "")
        }];
        let shipped = load_shipped_community_stations(dir.path(), &dial);
        assert_eq!(shipped.len(), 1);
        assert_eq!(shipped[0].name, "Shipped");
        assert!(!shipped[0].safe_for_streaming);
    }

    #[test]
    fn a_missing_or_broken_copy_is_no_stations() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(COMMUNITY_STATIONS_FILE);
        assert!(load_community_stations(&path).is_empty());
        std::fs::write(&path, "<html>").unwrap();
        assert!(load_community_stations(&path).is_empty());
        assert_eq!(parse_community_stations("{\"error\": \"down\"}"), None);
    }
}
