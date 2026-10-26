//! `fold-community-stations`: copy the radio stations players suggested
//! and the owner accepted into the shipped catalog.
//!
//! Accepted stations reach players two ways. The game downloads the site's
//! list at launch (`crates/freight-fate/src/community_stations.rs`), which
//! is how a new one arrives without an update. And every build ships a
//! copy, written here to `data/radio_community.json`, so a first launch, an
//! offline one, or a player with the orinks.net services switched off still
//! has every station accepted up to that build. The snapshot workflow
//! (build-career-1.9.yml) runs this as each build is cut and ships the
//! result without committing it, so the committed copy is only the fallback
//! for a build that cannot reach the site.
//!
//! The file mirrors the site exactly: a station the owner accepted appears,
//! one whose stream has died off the site's list disappears. Rows are
//! checked the way the game checks the download (`ff_core::radio::community`),
//! which repeats the check at load, so a bad row is refused here, where
//! someone reads the output, rather than only skipped in a player's log.
//!
//! ```text
//! cargo tools fold-community-stations
//! cargo tools fold-community-stations --check
//! cargo tools fold-community-stations --input saved-list.json
//! ```

use std::collections::HashSet;
use std::path::PathBuf;
use std::time::Duration;

use crate::cli::{Args, Spec};
use crate::http;
use crate::pyjson::{self, Object, Options, Value};
use crate::util::{self, Result};

const STATIONS_URL: &str = "https://orinks.net/api/freight-fate/stations";
const USER_AGENT: &str = "FreightFate/1.9 (catalog build)";
const ID_PREFIX: &str = "community-";
/// The game's ceiling on one list (MAX_COMMUNITY_STATIONS).
const MAX_STATIONS: usize = 2000;

const NOTES: &str = "Radio stations players suggested on orinks.net and the owner accepted, \
copied from the site by `cargo tools fold-community-stations`. Do not edit by hand: the next \
fold replaces this file with the site's list.";

/// The fields the game reads from a community row. Anything else the site
/// sends is left behind rather than shipped unread.
const FIELDS: &[&str] = &[
    "id",
    "name",
    "call_sign",
    "format",
    "source",
    "source_type",
    "station_type",
    "stream_url",
    "stream_format",
    "real_stream",
    "safe_for_streaming",
    "supported",
    "always_available",
    "market",
    "region",
    "lat",
    "lon",
    "range_miles",
    "frequency_mhz",
];

/// A JSON number as Python's `isinstance(x, (int, float))` sees it, which
/// counts `True` and `False` too.
fn number(value: Option<&Value>) -> Option<f64> {
    match value? {
        Value::Bool(b) => Some(f64::from(u8::from(*b))),
        other => other.as_f64(),
    }
}

fn located(row: &Object) -> bool {
    match (
        number(row.get("lat")),
        number(row.get("lon")),
        number(row.get("range_miles")),
    ) {
        (Some(lat), Some(lon), Some(miles)) => {
            (-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon) && miles > 0.0
        }
        _ => false,
    }
}

fn repr(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".into(),
        Some(Value::Str(s)) => crate::l10n::check::py_repr(s),
        Some(other) => other.py_str(),
    }
}

/// One site row as the shipped row, or an error naming what is wrong.
fn station(row: &Value) -> Result<Object> {
    let Some(row) = row.as_object() else {
        return Err(format!("a station row is not an object: {}", row.py_str()));
    };
    let id = match row.get("id").and_then(Value::as_str) {
        Some(id) if id.starts_with(ID_PREFIX) => id,
        _ => {
            return Err(format!(
                "station id {} does not start with '{ID_PREFIX}'",
                repr(row.get("id"))
            ))
        }
    };
    if !row
        .get("name")
        .and_then(Value::as_str)
        .is_some_and(|n| !util::py_strip(n).is_empty())
    {
        return Err(format!("{id} has no name"));
    }
    let stream_ok = row
        .get("stream_url")
        .and_then(Value::as_str)
        .is_some_and(|s| {
            let s = util::py_strip(s).to_lowercase();
            s.starts_with("http://") || s.starts_with("https://")
        });
    if !stream_ok {
        return Err(format!(
            "{id} has no http or https stream: {}",
            repr(row.get("stream_url"))
        ));
    }
    let mut kept = Object::new();
    for field in FIELDS {
        if let Some(value) = row.get(field) {
            kept.insert(*field, value.clone());
        }
    }
    // What the game would force anyway, written down so the file says what
    // the game does with it.
    kept.insert("real_stream", Value::Bool(true));
    kept.insert("safe_for_streaming", Value::Bool(false));
    kept.insert("supported", Value::Bool(true));
    let imported = row.get("source_type").and_then(Value::as_str) == Some("imported");
    if imported && located(row) {
        kept.insert("source_type", Value::from("imported"));
        kept.insert("station_type", Value::from("imported"));
        kept.insert("always_available", Value::Bool(false));
    } else {
        for field in ["lat", "lon", "range_miles", "frequency_mhz"] {
            kept.remove(field);
        }
        kept.insert("source_type", Value::from("web"));
        kept.insert("station_type", Value::from("web"));
        kept.insert("always_available", Value::Bool(true));
    }
    Ok(kept)
}

/// The file to ship for one reply from the site.
fn fold(reply: &Value) -> Result<Value> {
    let Some(rows) = reply.get("stations").and_then(Value::as_array) else {
        return Err("the reply is not a station list".into());
    };
    let mut stations = rows.iter().map(station).collect::<Result<Vec<_>>>()?;
    if stations.len() > MAX_STATIONS {
        return Err(format!(
            "{} stations is more than the game takes ({MAX_STATIONS})",
            stations.len()
        ));
    }
    let id = |s: &Object| {
        s.get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    let ids: HashSet<String> = stations.iter().map(id).collect();
    if ids.len() != stations.len() {
        return Err("the list names one station twice".into());
    }
    stations.sort_by_key(id);
    Ok(Value::Object(crate::pyobj! {
        "schema" => 1i64,
        "notes" => NOTES,
        "stations" => stations.into_iter().map(Value::Object).collect::<Vec<_>>(),
    }))
}

fn render(catalog: &Value) -> String {
    pyjson::dumps(catalog, &Options::indent(1).sorted().unicode()) + "\n"
}

fn fetch(url: &str, timeout: f64) -> Result<Value> {
    let agent = http::agent(Some(Duration::from_secs_f64(timeout)));
    let response = http::request(
        &agent,
        "GET",
        url,
        &[("User-Agent", USER_AGENT), ("Accept", "application/json")],
        None,
    )
    .map_err(|e| format!("could not read {url}: {e}"))?;
    if !(200..300).contains(&response.status) {
        return Err(format!(
            "could not read {url}: HTTP Error {}",
            response.status
        ));
    }
    let text =
        String::from_utf8(response.body).map_err(|e| format!("could not read {url}: {e}"))?;
    pyjson::loads(&text).map_err(|e| format!("could not read {url}: {e}"))
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["url", "input", "output", "timeout"],
            flags: &["check"],
        },
    )
    .map_err(|e| {
        format!("{e}\nusage: fold-community-stations [--url URL] [--input FILE] [--output FILE] [--timeout S] [--check]")
    })?;
    let output = args
        .path("output")
        .unwrap_or_else(|| util::repo_root().join("data").join("radio_community.json"));
    let timeout: f64 = args.number("timeout", 60.0).map_err(|e| e.0)?;
    let folded = match args.path("input") {
        Some(input) => pyjson::load_path(&input),
        None => fetch(&args.string("url", STATIONS_URL), timeout),
    }
    .and_then(|reply| fold(&reply));
    let text = match folded {
        Ok(catalog) => render(&catalog),
        Err(error) => {
            // The shipped file is left as it was: an unreachable site must
            // never empty the dial's accepted stations.
            eprintln!("Not folded: {error}");
            return Ok(2);
        }
    };
    let name = output
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned();
    let current = util::read_text_lf(&output).unwrap_or_default();
    let count = pyjson::loads(&text)
        .ok()
        .and_then(|v| v.get("stations").and_then(Value::as_array).map(Vec::len))
        .unwrap_or(0);
    if text == current {
        println!("{name} is current: {count} stations");
        return Ok(0);
    }
    if args.flag("check") {
        println!("{name} is out of date: the site lists {count} stations");
        return Ok(1);
    }
    util::write_text(&output, &text)?;
    println!("Wrote {name}: {count} stations");
    Ok(0)
}

/// Where the committed copy lives, for tests.
#[allow(dead_code)]
fn committed() -> PathBuf {
    util::repo_root().join("data").join("radio_community.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(rows: &str) -> Value {
        pyjson::loads(&format!("{{\"stations\": {rows}}}")).unwrap()
    }

    #[test]
    fn rows_are_checked_and_trimmed_to_what_the_game_reads() {
        let folded = fold(&reply(
            r#"[{"id": "community-b", "name": "B", "stream_url": "https://b", "extra": 1,
                 "source_type": "imported", "lat": 40, "lon": -90, "range_miles": 30},
                {"id": "community-a", "name": "A", "stream_url": "http://a", "lat": 1}]"#,
        ))
        .unwrap();
        let text = render(&folded);
        assert!(text.find("community-a") < text.find("community-b"));
        assert!(!text.contains("extra"));
        let stations = folded.get("stations").and_then(Value::as_array).unwrap();
        assert_eq!(
            stations[0].get("station_type").and_then(Value::as_str),
            Some("web")
        );
        assert!(stations[0].get("lat").is_none());
        assert_eq!(
            stations[1].get("always_available"),
            Some(&Value::Bool(false))
        );
    }

    #[test]
    fn bad_rows_refuse_the_whole_list() {
        assert!(fold(&reply(
            r#"[{"id": "x", "name": "A", "stream_url": "http://a"}]"#
        ))
        .is_err());
        assert!(fold(&reply(
            r#"[{"id": "community-a", "name": " ", "stream_url": "http://a"}]"#
        ))
        .is_err());
        let twice = r#"[{"id": "community-a", "name": "A", "stream_url": "http://a"},
                        {"id": "community-a", "name": "A", "stream_url": "http://a"}]"#;
        assert!(fold(&reply(twice)).unwrap_err().contains("twice"));
    }

    #[test]
    fn the_committed_copy_is_in_the_shipped_shape() {
        let text = util::read_text_lf(&committed()).unwrap();
        let catalog = pyjson::loads(&text).unwrap();
        let rows = Value::Object(crate::pyobj! {
            "stations" => catalog.get("stations").cloned().unwrap_or(Value::Array(Vec::new())),
        });
        assert_eq!(render(&fold(&rows).unwrap()), text);
    }
}
