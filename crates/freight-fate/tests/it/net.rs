//! Port of `tests/test_net.py`: the shared TLS client and speakable
//! network error descriptions.

use freight_fate::net::{self, describe_error, NetError, Tier};

#[test]
fn test_ssl_context_verifies_and_has_certifi_authorities() {
    // The Python test counted certifi's roots in the OpenSSL store. The Rust
    // client verifies through the platform verifier instead: there is no
    // bundle to count, only the fact that every tier builds a TLS-capable
    // agent at all (a misconfigured TLS provider panics at build time).
    for tier in [Tier::Orinks, Tier::GitHub, Tier::Feeds] {
        let agent = net::agent(tier);
        assert!(!agent.config().https_only()); // plain http allowed for the dev override
        assert_eq!(
            agent.config().timeouts().global,
            Some(std::time::Duration::from_secs_f64(tier.timeout_s()))
        );
    }
    assert_eq!(Tier::Orinks.timeout_s(), 10.0);
    assert_eq!(Tier::GitHub.timeout_s(), 15.0);
    assert_eq!(Tier::Feeds.timeout_s(), 20.0);
}

#[test]
fn test_ssl_context_is_cached() {
    let first = net::agent(Tier::Orinks) as *const _;
    let second = net::agent(Tier::Orinks) as *const _;
    assert!(std::ptr::eq(first, second));
}

#[test]
fn test_describe_error_speaks_http_codes() {
    let e = NetError::http(403);
    assert_eq!(describe_error(&e), "The server answered with error 403.");
}

#[test]
fn test_describe_error_unwraps_urlerror_reasons() {
    let cert = NetError::CertVerification("unable to get local issuer certificate".to_string());
    assert_eq!(
        describe_error(&cert),
        "The secure connection could not be verified."
    );
    let dns = NetError::HostNotFound("getaddrinfo failed".to_string());
    assert_eq!(
        describe_error(&dns),
        "The server address could not be found."
    );
}

#[test]
fn test_describe_error_common_failures() {
    assert_eq!(
        describe_error(&NetError::Timeout(String::new())),
        "The connection timed out."
    );
    assert_eq!(
        describe_error(&NetError::Connection(String::new())),
        "The connection was refused or dropped."
    );
    assert_eq!(
        describe_error(&NetError::Tls(String::new())),
        "The secure connection failed."
    );
}

#[test]
fn test_describe_error_falls_back_to_the_message() {
    let e = NetError::other(
        "FileNotFoundError",
        "FreightFate folder missing from update.zip",
    );
    assert_eq!(
        describe_error(&e),
        "FreightFate folder missing from update.zip."
    );
    assert_eq!(
        describe_error(&NetError::other("ValueError", "")),
        "ValueError."
    );
}

#[test]
fn test_io_errors_classify_into_the_python_families() {
    use std::io;
    let timeout: NetError = io::Error::new(io::ErrorKind::TimedOut, "timed out").into();
    assert_eq!(describe_error(&timeout), "The connection timed out.");
    let reset: NetError = io::Error::new(io::ErrorKind::ConnectionReset, "reset").into();
    assert_eq!(
        describe_error(&reset),
        "The connection was refused or dropped."
    );
    let cert: NetError = io::Error::other("invalid peer certificate: UnknownIssuer").into();
    assert_eq!(
        describe_error(&cert),
        "The secure connection could not be verified."
    );
    let other: NetError = io::Error::other("disk on fire").into();
    assert_eq!(describe_error(&other), "disk on fire.");
}

#[test]
fn test_http_error_carries_the_server_body() {
    let e = NetError::http_json(
        409,
        &serde_json::json!({"error": "conflict", "latestRevision": 5}),
    );
    assert_eq!(e.http_code(), Some(409));
    assert_eq!(e.error_body()["error"], "conflict");
    assert!(e.body_excerpt().contains("conflict"));
    assert_eq!(NetError::http(500).error_body().len(), 0);
}
