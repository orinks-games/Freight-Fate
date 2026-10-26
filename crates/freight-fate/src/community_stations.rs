//! Community stations on the network side: downloading the list of radio
//! stations players suggested and the owner accepted, and sending a
//! player's own suggestion.
//!
//! The list is fetched once per launch on its own thread and saved in the
//! saves folder, and a drive reads whatever copy is there when it starts
//! (`ff_core::radio::community`). Nothing waits on the fetch: a first launch
//! with no network simply has no community stations until the next one, and
//! an offline launch keeps the last list it had. It is skipped with the
//! orinks.net services switched off, which is also how a playtest sandbox
//! stays off the site.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use ff_core::models::profile::data_dir;
use ff_core::radio::community::{parse_community_stations, COMMUNITY_STATIONS_FILE};
use ff_core::settings::Settings;
use serde_json::{Map, Value};

use crate::net::{self, NetError, Tier, Transport};
use crate::online_presence::{base_url, default_transport, request_headers, OnlineIdentity};

/// Where the last good list is kept.
pub fn cache_path() -> PathBuf {
    data_dir().join(COMMUNITY_STATIONS_FILE)
}

/// The list's address on whichever Orinks site this build talks to.
pub fn stations_url() -> String {
    format!("{}/api/freight-fate/stations", base_url())
}

/// Fetch the list and replace the saved copy. Returns how many stations it
/// holds, or `None` when the site could not be reached or answered with
/// something that is not a station list (the old copy is kept then).
pub fn refresh(transport: &dyn Transport, path: &Path) -> Option<usize> {
    let reply = transport
        .call(&stations_url(), None, &[], None)
        .map_err(|e| log::info!("Community station list not fetched: {e}"))
        .ok()?;
    let text = reply.to_string();
    let count = parse_community_stations(&text)?.len();
    let temp = path.with_extension("json.tmp");
    let saved = std::fs::write(&temp, &text).and_then(|()| std::fs::rename(&temp, path));
    if let Err(e) = saved {
        log::warn!("Community station list not saved: {e}");
        let _ = std::fs::remove_file(&temp);
        return None;
    }
    log::info!("Community station list: {count} stations");
    Some(count)
}

/// Start the launch fetch, when this process is the game and the orinks.net
/// services are on. Bounded by the Orinks tier's timeout; the thread is
/// detached, so quitting never waits on it.
pub fn refresh_in_background(settings: &Settings) {
    if !settings.online_services || !net::real_network_allowed() {
        return;
    }
    let path = cache_path();
    let transport = default_transport();
    let spawned = std::thread::Builder::new()
        .name("community-stations".to_string())
        .spawn(move || {
            refresh(transport.as_ref(), &path);
        });
    if let Err(e) = spawned {
        log::warn!("Community station list fetch not started: {e}");
    }
}

// -- suggesting ---------------------------------------------------------------------------

/// What the player filled in. Optional fields are empty strings.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StationSuggestion {
    /// "terrestrial" or "web".
    pub kind: String,
    pub name: String,
    pub stream_url: String,
    pub call_sign: String,
    pub state: String,
    pub city: String,
    pub frequency: String,
    pub genre: String,
}

impl StationSuggestion {
    /// The request body, in the site's field names.
    pub fn payload(&self, driver_id: &str) -> Value {
        let mut body = Map::new();
        body.insert("driverId".into(), Value::from(driver_id));
        for (key, value) in [
            ("kind", &self.kind),
            ("name", &self.name),
            ("streamUrl", &self.stream_url),
            ("callSign", &self.call_sign),
            ("state", &self.state),
            ("city", &self.city),
            ("frequency", &self.frequency),
            ("genre", &self.genre),
        ] {
            if !value.trim().is_empty() {
                body.insert(key.into(), Value::from(value.trim()));
            }
        }
        Value::Object(body)
    }
}

/// The site's answer to a suggestion: whether it was taken, and the
/// sentence to say either way.
#[derive(Debug, Clone, PartialEq)]
pub struct SuggestionOutcome {
    pub accepted: bool,
    pub message: String,
}

/// Said when the site could not be reached or gave no sentence of its own.
pub const SUGGESTION_UNREACHABLE: &str =
    "The suggestion couldn't be sent. Check your connection and try again.";

/// Read the site's reply (or refusal) into what the player hears.
pub fn suggestion_outcome(reply: Result<Value, NetError>) -> SuggestionOutcome {
    let (accepted, body) = match reply {
        Ok(Value::Object(body)) => (body.get("ok") == Some(&Value::Bool(true)), body),
        Ok(_) => (false, Map::new()),
        Err(e) => {
            log::info!("Station suggestion not sent: {e}");
            (false, e.error_body())
        }
    };
    let message = body
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|m| !m.is_empty() && m.len() <= 300)
        .map(str::to_string);
    match message {
        Some(message) => SuggestionOutcome { accepted, message },
        None => SuggestionOutcome {
            accepted: false,
            message: SUGGESTION_UNREACHABLE.to_string(),
        },
    }
}

/// Sends one suggestion and returns the raw reply. A seam so tests never
/// reach the site.
pub type SuggestionSender =
    Arc<dyn Fn(&Value, &OnlineIdentity) -> Result<Value, NetError> + Send + Sync>;

/// The real sender: a POST on the long-wait tier, because the site plays the
/// stream before it answers.
pub fn default_sender() -> SuggestionSender {
    Arc::new(|payload: &Value, identity: &OnlineIdentity| {
        let bypass = std::env::var("FREIGHT_FATE_ONLINE_BYPASS").ok();
        let headers = request_headers(&identity.auth_headers(), bypass.as_deref());
        net::request_json(
            Tier::StationSuggestion,
            Some("POST"),
            &stations_url(),
            Some(payload),
            &headers,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_payload_leaves_out_what_was_skipped() {
        let suggestion = StationSuggestion {
            kind: "web".into(),
            name: " Night Owl ".into(),
            stream_url: "https://s.example/live".into(),
            ..Default::default()
        };
        assert_eq!(
            suggestion.payload("night-owl-1234"),
            json!({
                "driverId": "night-owl-1234", "kind": "web", "name": "Night Owl",
                "streamUrl": "https://s.example/live",
            })
        );
    }

    #[test]
    fn the_site_sentence_is_what_the_player_hears() {
        let taken = suggestion_outcome(Ok(json!({"ok": true, "message": "Thanks."})));
        assert_eq!(
            taken,
            SuggestionOutcome {
                accepted: true,
                message: "Thanks.".into()
            }
        );
        let refused = suggestion_outcome(Err(NetError::http_json(
            422,
            &json!({"ok": false, "reason": "duplicate_catalog", "message": "That station is already on the dial."}),
        )));
        assert!(!refused.accepted);
        assert_eq!(refused.message, "That station is already on the dial.");
    }

    #[test]
    fn no_sentence_means_it_was_not_sent() {
        for reply in [
            Err(NetError::http(502)),
            Ok(json!({"ok": true})),
            Ok(json!("nope")),
        ] {
            assert_eq!(
                suggestion_outcome(reply),
                SuggestionOutcome {
                    accepted: false,
                    message: SUGGESTION_UNREACHABLE.into()
                }
            );
        }
    }

    struct Reply(Result<Value, NetError>);

    impl Transport for Reply {
        fn call(
            &self,
            _url: &str,
            _payload: Option<&Value>,
            _headers: &[(String, String)],
            _method: Option<&str>,
        ) -> Result<Value, NetError> {
            self.0.clone()
        }
    }

    #[test]
    fn a_good_list_replaces_the_copy_and_a_bad_one_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(COMMUNITY_STATIONS_FILE);
        let list = json!({"schema": 1, "stations": [{
            "id": "community-a", "name": "Night Owl", "stream_url": "https://s.example/live",
            "source_type": "web",
        }]});
        assert_eq!(refresh(&Reply(Ok(list)), &path), Some(1));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert_eq!(refresh(&Reply(Err(NetError::http(503))), &path), None);
        assert_eq!(refresh(&Reply(Ok(json!({"error": "x"}))), &path), None);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), saved);
    }
}
