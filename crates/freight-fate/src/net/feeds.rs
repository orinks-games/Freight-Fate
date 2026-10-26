//! An error text for the live-data feeds that a player's log can be
//! diagnosed from.
//!
//! Live weather went unavailable on one Windows machine while another NWS
//! client worked beside it (2026-10-19), and the log held only "The
//! connection timed out." or the like, with no address, no step and no
//! server reason. [`feed_error_text`] keeps the speakable sentence first and
//! adds the request line and the server's own words after it.

use serde_json::Value;

use super::{describe_error, NetError};

/// The most of a server's error body a log line carries.
const BODY_EXCERPT_CHARS: usize = 300;

/// `describe_error`'s sentence, then the request and what the server or
/// the socket said, for the log:
/// `The server answered with error 403. (GET api.weather.gov/points/1,2:
/// Forbidden: blocked)`.
///
/// The query string is left out: some feed addresses carry an access key
/// there, and a player's log is meant to be shared.
pub fn feed_error_text(method: &str, url: &str, error: &NetError) -> String {
    let address = url
        .split_once("://")
        .map_or(url, |(_, rest)| rest)
        .split(['?', '#'])
        .next()
        .unwrap_or_default();
    let reason = match error {
        NetError::Http { body, .. } => http_reason(body),
        NetError::CertVerification(m)
        | NetError::Tls(m)
        | NetError::HostNotFound(m)
        | NetError::Timeout(m)
        | NetError::Connection(m) => m.trim().to_string(),
        NetError::Other { type_name, .. } => type_name.clone(),
    };
    let sentence = describe_error(error);
    if reason.is_empty() {
        format!("{sentence} ({method} {address})")
    } else {
        format!("{sentence} ({method} {address}: {reason})")
    }
}

/// The server's reason from an error body: NWS answers problem+json, whose
/// `title` and `detail` say it plainly; anything else is a short excerpt
/// with its whitespace collapsed (an HTML "Access Denied" page included).
fn http_reason(body: &[u8]) -> String {
    if let Ok(Value::Object(doc)) = serde_json::from_slice::<Value>(body) {
        let field = |name: &str| {
            doc.get(name)
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|s| !s.is_empty())
        };
        let parts: Vec<&str> = [field("title"), field("detail")]
            .into_iter()
            .flatten()
            .collect();
        if !parts.is_empty() {
            return excerpt(&parts.join(": "));
        }
    }
    excerpt(&String::from_utf8_lossy(body))
}

fn excerpt(text: &str) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(BODY_EXCERPT_CHARS).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_text_names_the_request_and_the_nws_reason() {
        let error = NetError::http_json(
            403,
            &serde_json::json!({
                "title": "Forbidden",
                "detail": "Access to this resource is blocked",
                "correlationId": "abc",
            }),
        );
        assert_eq!(
            feed_error_text(
                "GET",
                "https://api.weather.gov/points/39.7392,-104.9903",
                &error
            ),
            "The server answered with error 403. (GET api.weather.gov/points/39.7392,-104.9903: \
             Forbidden: Access to this resource is blocked)"
        );
    }

    #[test]
    fn error_text_keeps_access_keys_out_of_the_log() {
        let error = NetError::Timeout("timeout: global".to_string());
        let text = feed_error_text("GET", "https://511.example/api?key=SECRET&x=1", &error);
        assert_eq!(
            text,
            "The connection timed out. (GET 511.example/api: timeout: global)"
        );
    }

    #[test]
    fn error_text_collapses_an_html_refusal() {
        let error = NetError::Http {
            code: 403,
            body: b"<HTML><HEAD>\n<TITLE>Access Denied</TITLE>\n</HEAD></HTML>".to_vec(),
        };
        let text = feed_error_text("GET", "https://api.weather.gov/stations", &error);
        assert!(text.ends_with(
            "(GET api.weather.gov/stations: <HTML><HEAD> <TITLE>Access Denied</TITLE> </HEAD></HTML>)"
        ));
    }
}
