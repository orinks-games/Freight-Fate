//! `ios-signing`: find the iOS TestFlight signing profile through the App
//! Store Connect API.
//!
//! Used by `.github/workflows/ios-testflight.yml`. The Apple distribution
//! certificate and its private key are stored once, as the repository
//! secrets IOS_DISTRIBUTION_P12_BASE64 and IOS_DISTRIBUTION_P12_PASSWORD,
//! and every run signs with that same certificate. Nothing here creates or
//! revokes a certificate, so Apple sends no "certificate revoked" mail after
//! a run.
//!
//! Sub-commands (the API key comes from the environment, never the command
//! line):
//!
//! * `prepare`: check the key, find the stored certificate on the team by
//!   its serial number, find or register the bundle id, then reuse the App
//!   Store profile that names that certificate (making one only when none is
//!   active) and write it under `--out`.
//! * `check-app`: check that the App Store Connect app record exists.
//! * `next-build`: print the next build number for TestFlight.
//! * `distribute`: add an uploaded build to an external tester group
//!   (Friends) and submit it to Apple's beta review; internal testers get
//!   every build without this.
//! * `cleanup`: delete the local profile and certificate files; nothing in
//!   App Store Connect is touched.
//!
//! Environment: ASC_KEY_ID, ASC_ISSUER_ID, ASC_KEY_PATH (the downloaded
//! .p8). Signing goes through the `openssl` command. Nothing here prints a
//! key, a token, a certificate or a profile; error messages carry Apple's
//! error title and detail and nothing else from a response.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use crate::cli::{Args, Spec};
use crate::http;
use crate::util;

const API: &str = "https://api.appstoreconnect.apple.com";
const APP_NAME: &str = "Freight Fate";
const CERTIFICATE_TYPES: &[&str] = &["DISTRIBUTION", "IOS_DISTRIBUTION"];
const PROFILE_TYPE: &str = "IOS_APP_STORE";
/// Apple allows at most 20 minutes.
const TOKEN_LIFETIME: i64 = 15 * 60;

const CERT_FILE: &str = "distribution.cert.pem";
const PROFILE_FILE: &str = "profile.mobileprovision";
const P12_FILE: &str = "distribution.p12";

/// A failure. `status` is set for an HTTP refusal from the API, so callers
/// can tell a missing record (404) or a rejected key (401, 403) apart.
#[derive(Debug)]
struct SigningError {
    message: String,
    status: Option<u16>,
    method: String,
    path: String,
    detail: String,
}

impl SigningError {
    fn new(message: impl Into<String>) -> Self {
        SigningError {
            message: message.into(),
            status: None,
            method: String::new(),
            path: String::new(),
            detail: String::new(),
        }
    }
}

type Result<T> = std::result::Result<T, SigningError>;

const ACCESS_HELP: &str = "The App Store Connect API key was rejected. Check that ASC_KEY_ID, \
ASC_ISSUER_ID and ASC_KEY_P8_BASE64 belong to the same key, that the key has not been revoked, \
and that its access is App Manager or Admin (Users and Access, Integrations, App Store Connect \
API).";

const STORED_CERT_HELP: &str = "Make a new Apple Distribution certificate from the same private \
key and store it again as IOS_DISTRIBUTION_P12_BASE64 and IOS_DISTRIBUTION_P12_PASSWORD \
(docs/ios-testflight.md).";

fn missing_app_help(bundle: &str) -> String {
    format!(
        "There is no App Store Connect app record for bundle id {bundle}, the id \
         `cargo tools build-ios` stamps into the app. The bundle id in App Store Connect must \
         match it exactly (App Store Connect, the app, App Information). Nothing was built."
    )
}

/// Reword a 401 or 403 into what to fix; other failures pass through.
fn explain(error: SigningError) -> SigningError {
    match error.status {
        Some(401) => SigningError::new(ACCESS_HELP),
        Some(403) => SigningError::new(format!(
            "The App Store Connect API key is not allowed to do this ({} {}). Give the key App \
             Manager or Admin access (Users and Access, Integrations, App Store Connect API), \
             or make a new key with that access. Apple said: {}",
            error.method, error.path, error.detail
        )),
        _ => error,
    }
}

// --- JWT -------------------------------------------------------------------

fn b64url(data: &[u8]) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(data)
}

/// Convert an ASN.1 DER ECDSA signature to the fixed-width r||s JWT form.
fn der_to_raw_signature(der: &[u8], size: usize) -> Result<Vec<u8>> {
    let bad = || SigningError::new("openssl returned a signature in an unexpected format.");
    if der.len() < 8 || der[0] != 0x30 {
        return Err(bad());
    }
    let mut pos = if der[1] & 0x80 != 0 {
        2 + usize::from(der[1] & 0x7f)
    } else {
        2
    };
    let mut raw = Vec::with_capacity(size * 2);
    for _ in 0..2 {
        if der.get(pos) != Some(&0x02) {
            return Err(bad());
        }
        let length = usize::from(*der.get(pos + 1).ok_or_else(bad)?);
        let value = der.get(pos + 2..pos + 2 + length).ok_or_else(bad)?;
        let trimmed: &[u8] = {
            let first = value.iter().position(|&b| b != 0).unwrap_or(value.len());
            &value[first..]
        };
        if trimmed.len() > size {
            return Err(SigningError::new(
                "openssl returned a signature of an unexpected size.",
            ));
        }
        raw.extend(std::iter::repeat_n(0u8, size - trimmed.len()));
        raw.extend_from_slice(trimmed);
        pos += 2 + length;
    }
    Ok(raw)
}

fn sign_es256(signing_input: &[u8], key_path: &Path) -> Result<Vec<u8>> {
    let refused = || {
        SigningError::new(
            "openssl could not sign with the App Store Connect key. Check that \
             ASC_KEY_P8_BASE64 holds the base64 of the downloaded AuthKey .p8 file.",
        )
    };
    let mut child = Command::new("openssl")
        .args(["dgst", "-sha256", "-sign"])
        .arg(key_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| refused())?;
    child
        .stdin
        .take()
        .ok_or_else(refused)?
        .write_all(signing_input)
        .map_err(|_| refused())?;
    let out = child.wait_with_output().map_err(|_| refused())?;
    if !out.status.success() {
        return Err(refused());
    }
    der_to_raw_signature(&out.stdout, 32)
}

fn make_token(key_id: &str, issuer: &str, key_path: &Path) -> Result<String> {
    let issued = chrono::Utc::now().timestamp();
    let header = json!({"alg": "ES256", "kid": key_id, "typ": "JWT"});
    let payload = json!({
        "iss": issuer,
        "iat": issued,
        "exp": issued + TOKEN_LIFETIME,
        "aud": "appstoreconnect-v1",
    });
    let signing_input = format!(
        "{}.{}",
        b64url(header.to_string().as_bytes()),
        b64url(payload.to_string().as_bytes())
    );
    let signature = sign_es256(signing_input.as_bytes(), key_path)?;
    Ok(format!("{signing_input}.{}", b64url(&signature)))
}

// --- HTTP ------------------------------------------------------------------

/// `application/x-www-form-urlencoded`, as Python's `urlencode` writes it.
fn urlencode(pairs: &[(&str, &str)]) -> String {
    let enc = |s: &str| {
        let mut out = String::new();
        for b in s.bytes() {
            match b {
                b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'_' | b'.' | b'-' | b'~' => {
                    out.push(b as char)
                }
                b' ' => out.push('+'),
                other => out.push_str(&format!("%{other:02X}")),
            }
        }
        out
    };
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", enc(k), enc(v)))
        .collect::<Vec<_>>()
        .join("&")
}

fn error_text(body: &[u8]) -> String {
    let Ok(parsed) = serde_json::from_slice::<Value>(body) else {
        return String::new();
    };
    let Some(errors) = parsed.get("errors").and_then(Value::as_array) else {
        return String::new();
    };
    errors
        .iter()
        .take(3)
        .map(|e| {
            let field = |k: &str| match e.get(k) {
                Some(Value::String(s)) => s.clone(),
                Some(Value::Null) | None => String::new(),
                Some(other) => other.to_string(),
            };
            format!("{}: {}", field("title"), field("detail"))
                .trim_matches(|c| c == ':' || c == ' ')
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("; ")
}

struct Api {
    token: String,
    agent: ureq::Agent,
}

impl Api {
    fn call(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let url = if path.starts_with("http") {
            path.to_string()
        } else {
            format!("{API}{path}")
        };
        let auth = format!("Bearer {}", self.token);
        let data = body.map(|b| b.to_string().into_bytes());
        let mut headers = vec![("Authorization", auth.as_str())];
        if data.is_some() {
            headers.push(("Content-Type", "application/json"));
        }
        let response = http::request(&self.agent, method, &url, &headers, data.as_deref())
            .map_err(|_| {
                SigningError::new("Could not reach the App Store Connect API. Run it again.")
            })?;
        if !(200..300).contains(&response.status) {
            let short_path = path.split('?').next().unwrap_or(path).to_string();
            let detail = error_text(&response.body);
            return Err(SigningError {
                message: format!(
                    "{method} {short_path} failed with HTTP {}. {detail}",
                    response.status
                )
                .trim()
                .to_string(),
                status: Some(response.status),
                method: method.to_string(),
                path: short_path,
                detail,
            });
        }
        if response.body.is_empty() {
            return Ok(json!({}));
        }
        serde_json::from_slice(&response.body)
            .map_err(|_| SigningError::new("App Store Connect sent a reply that is not JSON."))
    }
}

fn data(reply: &Value) -> &[Value] {
    reply
        .get("data")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn text<'a>(value: &'a Value, path: &[&str]) -> &'a str {
    let mut v = value;
    for key in path {
        match v.get(key) {
            Some(next) => v = next,
            None => return "",
        }
    }
    v.as_str().unwrap_or("")
}

fn id_of(value: &Value) -> Result<String> {
    value
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .ok_or_else(|| SigningError::new("App Store Connect sent a record with no id."))
}

// --- Signing material ------------------------------------------------------

fn normalize_serial(serial: &str) -> String {
    let trimmed = serial.to_uppercase().trim_start_matches('0').to_string();
    if trimmed.is_empty() {
        "0".into()
    } else {
        trimmed
    }
}

/// The serial number of a PEM certificate, as uppercase hex without leading
/// zeros.
fn certificate_serial(cert_path: &Path) -> Result<String> {
    let out = Command::new("openssl")
        .args(["x509", "-in"])
        .arg(cert_path)
        .args(["-noout", "-serial"])
        .stderr(Stdio::null())
        .output();
    let stdout = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    };
    match stdout.split_once('=') {
        Some((_, serial)) => Ok(normalize_serial(serial)),
        None => Err(SigningError::new(
            "openssl could not read the stored distribution certificate. Check that \
             IOS_DISTRIBUTION_P12_BASE64 holds the base64 of a .p12 with the certificate in it.",
        )),
    }
}

/// The team's certificate with this serial number; refuses a missing or
/// expired one.
fn find_certificate(api: &Api, serial: &str) -> Result<Value> {
    let query = urlencode(&[("filter[serialNumber]", serial), ("limit", "200")]);
    let reply = api.call("GET", &format!("/v1/certificates?{query}"), None)?;
    let now = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
    for cert in data(&reply) {
        if normalize_serial(text(cert, &["attributes", "serialNumber"])) != serial {
            continue;
        }
        let kind = text(cert, &["attributes", "certificateType"]);
        if !CERTIFICATE_TYPES.contains(&kind) {
            return Err(SigningError::new(format!(
                "The stored certificate (serial {serial}) is a {kind} certificate, not a \
                 distribution one. {STORED_CERT_HELP}"
            )));
        }
        let expires = text(cert, &["attributes", "expirationDate"]);
        if !expires.is_empty() && expires.get(..19).unwrap_or(expires) <= now.as_str() {
            return Err(SigningError::new(format!(
                "The stored distribution certificate expired on {}. {STORED_CERT_HELP}",
                expires.get(..10).unwrap_or(expires)
            )));
        }
        return Ok(cert.clone());
    }
    Err(SigningError::new(format!(
        "The stored distribution certificate (serial {serial}) is not on the team any more; \
         it was revoked or has expired. {STORED_CERT_HELP}"
    )))
}

fn ensure_bundle_id(api: &Api, identifier: &str) -> Result<(String, bool)> {
    let query = urlencode(&[("filter[identifier]", identifier), ("limit", "200")]);
    let reply = api.call("GET", &format!("/v1/bundleIds?{query}"), None)?;
    for item in data(&reply) {
        if text(item, &["attributes", "identifier"]) == identifier {
            return Ok((id_of(item)?, false));
        }
    }
    let body = json!({"data": {
        "type": "bundleIds",
        "attributes": {"identifier": identifier, "name": APP_NAME, "platform": "IOS"},
    }});
    let reply = api.call("POST", "/v1/bundleIds", Some(&body))?;
    Ok((id_of(&reply["data"])?, true))
}

/// An active App Store profile for this bundle id that names this
/// certificate; the one that expires last.
fn find_profile(api: &Api, bundle_id: &str, certificate_id: &str) -> Result<Option<Value>> {
    let query = urlencode(&[
        ("filter[profileType]", PROFILE_TYPE),
        ("filter[profileState]", "ACTIVE"),
        ("include", "bundleId,certificates"),
        ("limit", "200"),
        ("limit[certificates]", "50"),
    ]);
    let reply = api.call("GET", &format!("/v1/profiles?{query}"), None)?;
    let mut best: Option<&Value> = None;
    for profile in data(&reply) {
        let bundle = text(profile, &["relationships", "bundleId", "data", "id"]);
        let certs: Vec<&str> = profile
            .pointer("/relationships/certificates/data")
            .and_then(Value::as_array)
            .map(|a| a.iter().map(|c| text(c, &["id"])).collect())
            .unwrap_or_default();
        if bundle != bundle_id || !certs.contains(&certificate_id) {
            continue;
        }
        if text(profile, &["attributes", "profileContent"]).is_empty() {
            continue;
        }
        let expires = text(profile, &["attributes", "expirationDate"]);
        if best.is_none_or(|b| expires > text(b, &["attributes", "expirationDate"])) {
            best = Some(profile);
        }
    }
    Ok(best.cloned())
}

fn create_profile(api: &Api, name: &str, bundle_id: &str, certificate_id: &str) -> Result<Value> {
    let body = json!({"data": {
        "type": "profiles",
        "attributes": {"name": name, "profileType": PROFILE_TYPE},
        "relationships": {
            "bundleId": {"data": {"type": "bundleIds", "id": bundle_id}},
            "certificates": {"data": [{"type": "certificates", "id": certificate_id}]},
        },
    }});
    Ok(api.call("POST", "/v1/profiles", Some(&body))?["data"].clone())
}

fn find_app_id(api: &Api, identifier: &str) -> Result<Option<String>> {
    let query = urlencode(&[("filter[bundleId]", identifier)]);
    let reply = api.call("GET", &format!("/v1/apps?{query}"), None)?;
    for app in data(&reply) {
        let bundle = app.pointer("/attributes/bundleId").and_then(Value::as_str);
        if bundle.is_none_or(|b| b == identifier) {
            return Ok(Some(id_of(app)?));
        }
    }
    Ok(None)
}

fn version_key(text: &str) -> Option<Vec<u64>> {
    text.split('.').map(|p| p.trim().parse().ok()).collect()
}

/// (short version, build number) of every build App Store Connect lists.
fn uploaded_builds(api: &Api, app_id: &str) -> Result<Vec<(String, String)>> {
    let query = urlencode(&[
        ("filter[app]", app_id),
        ("include", "preReleaseVersion"),
        ("fields[builds]", "version,preReleaseVersion"),
        ("fields[preReleaseVersions]", "version"),
        ("sort", "-uploadedDate"),
        ("limit", "200"),
    ]);
    let reply = api.call("GET", &format!("/v1/builds?{query}"), None)?;
    let included = reply
        .get("included")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let short_of = |id: &str| -> String {
        included
            .iter()
            .filter(|i| text(i, &["type"]) == "preReleaseVersions")
            .find(|i| text(i, &["id"]) == id)
            .map(|i| text(i, &["attributes", "version"]).to_string())
            .unwrap_or_default()
    };
    Ok(data(&reply)
        .iter()
        .map(|build| {
            let related = text(build, &["relationships", "preReleaseVersion", "data", "id"]);
            (
                short_of(related),
                text(build, &["attributes", "version"]).to_string(),
            )
        })
        .collect())
}

/// (next number, highest number seen). Counts only plain integer build
/// numbers and refuses a short version lower than one already uploaded.
fn next_build_number(
    builds: &[(String, String)],
    short_version: &str,
    run_number: u64,
) -> Result<(u64, u64)> {
    let wanted = version_key(short_version);
    let mut highest = 0;
    for (short, number) in builds {
        if let (Some(w), Some(seen)) = (&wanted, version_key(short)) {
            if seen > *w {
                return Err(SigningError::new(format!(
                    "TestFlight already has version {short}, which is higher than this \
                     build's {short_version}. Apple refuses an upload to a closed version. \
                     Raise the version in the root Cargo.toml first (see \
                     docs/ios-testflight.md)."
                )));
            }
        }
        if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) {
            highest = highest.max(number.parse().unwrap_or(0));
        }
    }
    Ok((highest.max(run_number) + 1, highest))
}

// --- External testers ------------------------------------------------------

/// The uploaded build with this build number, once Apple has processed it.
fn find_build(api: &Api, app_id: &str, number: &str) -> Result<Value> {
    let query = urlencode(&[
        ("filter[app]", app_id),
        ("filter[version]", number),
        ("limit", "10"),
    ]);
    let attempts = 30;
    let mut state = "not listed yet".to_string();
    for attempt in 0..attempts {
        let reply = api.call("GET", &format!("/v1/builds?{query}"), None)?;
        if let Some(build) = data(&reply).first() {
            state = text(build, &["attributes", "processingState"]).to_string();
            if state == "VALID" {
                return Ok(build.clone());
            }
            if state == "FAILED" || state == "INVALID" {
                break;
            }
        }
        if attempt + 1 < attempts {
            std::thread::sleep(Duration::from_secs(20));
        }
    }
    Err(SigningError::new(format!(
        "Build {number} is {state} in App Store Connect, so it was not sent to external \
         testers. Internal testers still get it once Apple finishes processing."
    )))
}

fn find_external_group(api: &Api, app_id: &str, name: &str) -> Result<Value> {
    let reply = api.call(
        "GET",
        &format!("/v1/apps/{app_id}/betaGroups?limit=200"),
        None,
    )?;
    for group in data(&reply) {
        let internal = group
            .pointer("/attributes/isInternalGroup")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if text(group, &["attributes", "name"]) == name && !internal {
            return Ok(group.clone());
        }
    }
    Err(SigningError::new(format!(
        "There is no external TestFlight group named {name:?} for this app (App Store Connect, \
         the app, TestFlight, External Testing). The build was uploaded for internal testers \
         only."
    )))
}

fn review_submitted(api: &Api, build_id: &str) -> Result<bool> {
    match api.call(
        "GET",
        &format!("/v1/builds/{build_id}/betaAppReviewSubmission"),
        None,
    ) {
        Ok(reply) => Ok(reply.get("data").is_some_and(|d| !d.is_null())),
        Err(e) if e.status == Some(404) => Ok(false),
        Err(e) => Err(e),
    }
}

/// Add build `number` to the external group and submit it; returns a
/// summary.
fn distribute(api: &Api, bundle_id: &str, number: &str, group_name: &str) -> Result<String> {
    let app_id = find_app_id(api, bundle_id)?
        .ok_or_else(|| SigningError::new(missing_app_help(bundle_id)))?;
    let group = find_external_group(api, &app_id, group_name)?;
    let build_id = id_of(&find_build(api, &app_id, number)?)?;
    api.call(
        "POST",
        &format!("/v1/betaGroups/{}/relationships/builds", id_of(&group)?),
        Some(&json!({"data": [{"type": "builds", "id": build_id}]})),
    )?;
    if review_submitted(api, &build_id)? {
        return Ok(format!(
            "Build {number} added to {group_name}; it was already submitted for beta review."
        ));
    }
    api.call(
        "POST",
        "/v1/betaAppReviewSubmissions",
        Some(&json!({"data": {
            "type": "betaAppReviewSubmissions",
            "relationships": {"build": {"data": {"type": "builds", "id": build_id}}},
        }})),
    )?;
    Ok(format!(
        "Build {number} added to {group_name} and submitted for beta review. Its testers get \
         it once Apple approves it."
    ))
}

// --- Commands --------------------------------------------------------------

fn connect() -> Result<Api> {
    let var = |name: &str| {
        std::env::var(name).map_err(|_| {
            SigningError::new(format!(
                "{name} is not set; the App Store Connect key is needed."
            ))
        })
    };
    let key_id = var("ASC_KEY_ID")?;
    let issuer = var("ASC_ISSUER_ID")?;
    let key_path = PathBuf::from(var("ASC_KEY_PATH")?);
    if key_id.is_empty() || issuer.is_empty() {
        return Err(SigningError::new(
            "ASC_KEY_ID or ASC_ISSUER_ID is empty; set the repository secrets.",
        ));
    }
    if !key_path
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() > 0)
    {
        return Err(SigningError::new(
            "The App Store Connect key file is missing or empty (ASC_KEY_P8_BASE64).",
        ));
    }
    Ok(Api {
        token: make_token(&key_id, &issuer, &key_path)?,
        agent: http::agent(Some(Duration::from_secs(60))),
    })
}

fn prepare(args: &Args, bundle_id: &str, out: &Path) -> Result<()> {
    std::fs::create_dir_all(out).map_err(|e| SigningError::new(e.to_string()))?;
    let cert_path = args
        .path("cert")
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| out.join(CERT_FILE));
    if !cert_path
        .metadata()
        .is_ok_and(|m| m.is_file() && m.len() > 0)
    {
        return Err(SigningError::new(format!(
            "The stored distribution certificate was not exported to {}. Check \
             IOS_DISTRIBUTION_P12_BASE64 and IOS_DISTRIBUTION_P12_PASSWORD.",
            cert_path.display()
        )));
    }
    let serial = certificate_serial(&cert_path)?;
    let api = connect()?;
    let run = || -> Result<()> {
        let cert = find_certificate(&api, &serial)?;
        let cert_id = id_of(&cert)?;
        let expires = text(&cert, &["attributes", "expirationDate"]);
        println!(
            "Key accepted. Signing with the stored distribution certificate (id {cert_id}, \
             expires {}).",
            expires.get(..10).unwrap_or(expires)
        );
        let (bundle, created) = ensure_bundle_id(&api, bundle_id)?;
        println!(
            "Bundle id {bundle_id}: {}.",
            if created {
                "registered now"
            } else {
                "already registered"
            }
        );
        let profile = match find_profile(&api, &bundle, &cert_id)? {
            Some(profile) => {
                println!(
                    "Reusing the App Store profile {:?} (id {}).",
                    text(&profile, &["attributes", "name"]),
                    id_of(&profile)?
                );
                profile
            }
            None => {
                let name = format!("Freight Fate App Store {cert_id}");
                let profile = create_profile(&api, &name, &bundle, &cert_id)?;
                println!(
                    "Made an App Store profile for the stored certificate (id {}). Later runs \
                     reuse it.",
                    id_of(&profile)?
                );
                profile
            }
        };
        let content = base64::engine::general_purpose::STANDARD
            .decode(text(&profile, &["attributes", "profileContent"]))
            .map_err(|_| SigningError::new("The profile Apple sent is not valid base64."))?;
        std::fs::write(out.join(PROFILE_FILE), content)
            .map_err(|e| SigningError::new(e.to_string()))
    };
    run().map_err(explain)
}

fn next_build(args: &Args, bundle_id: &str) -> Result<()> {
    let run_number: u64 = args
        .number("run-number", 0)
        .map_err(|e| SigningError::new(e.0))?;
    let version = super::project_version().map_err(SigningError::new)?;
    let short_version = super::ios::store_version(&version);
    let api = connect()?;
    let builds = (|| {
        let app_id = find_app_id(&api, bundle_id)?
            .ok_or_else(|| SigningError::new(missing_app_help(bundle_id)))?;
        uploaded_builds(&api, &app_id)
    })()
    .map_err(explain)?;
    let (number, highest) = next_build_number(&builds, &short_version, run_number)?;
    eprintln!(
        "Version {short_version}: {} earlier builds, highest build number {highest}; this \
         build will be {number}.",
        builds.len()
    );
    println!("{number}");
    Ok(())
}

fn run_command(command: &str, args: &Args) -> Result<()> {
    let bundle_id = args.string("bundle-id", super::ios::BUNDLE_ID);
    let out = PathBuf::from(args.string("out", "build/ios-signing"));
    match command {
        "prepare" => prepare(args, &bundle_id, &out),
        "check-app" => {
            let api = connect()?;
            if find_app_id(&api, &bundle_id).map_err(explain)?.is_none() {
                return Err(SigningError::new(missing_app_help(&bundle_id)));
            }
            println!("App record for {bundle_id} exists.");
            Ok(())
        }
        "next-build" => next_build(args, &bundle_id),
        "distribute" => {
            let number = args.string("build-number", "");
            if number.is_empty() {
                return Err(SigningError::new("distribute needs --build-number."));
            }
            let group = args.string("group", "Friends");
            let api = connect()?;
            println!(
                "{}",
                distribute(&api, &bundle_id, &number, &group).map_err(explain)?
            );
            Ok(())
        }
        "cleanup" => {
            for name in [CERT_FILE, PROFILE_FILE, P12_FILE] {
                let _ = std::fs::remove_file(out.join(name));
            }
            Ok(())
        }
        other => Err(SigningError::new(format!("unknown sub-command {other:?}"))),
    }
}

const USAGE: &str = "usage: ios-signing {prepare,check-app,next-build,distribute,cleanup} \
[--bundle-id ID] [--out DIR] [--label L] [--run-number N] [--cert PEM] [--build-number N] \
[--group NAME]";

pub fn main(argv: &[String]) -> util::Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &[
                "bundle-id",
                "out",
                "label",
                "run-number",
                "cert",
                "build-number",
                "group",
            ],
            flags: &[],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let Some(command) = args.positionals.first() else {
        return Err(USAGE.into());
    };
    match run_command(command, &args) {
        Ok(()) => Ok(0),
        Err(error) => {
            println!("::error::{}", error.message);
            Ok(1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn der_signatures_become_fixed_width() {
        // r has a leading zero byte (high bit set), s is short.
        let mut der = vec![0x30, 0x00, 0x02, 33, 0x00];
        der.extend([0x80u8; 32]);
        der.extend([0x02, 2, 0x01, 0x02]);
        der[1] = (der.len() - 2) as u8;
        let raw = der_to_raw_signature(&der, 32).unwrap();
        assert_eq!(raw.len(), 64);
        assert_eq!(&raw[..32], &[0x80u8; 32]);
        assert_eq!(&raw[62..], &[0x01, 0x02]);
        assert!(der_to_raw_signature(&[0x31; 10], 32).is_err());
    }

    #[test]
    fn build_numbers_rise_past_every_upload() {
        let builds = vec![
            ("1.9.0".to_string(), "12".to_string()),
            ("1.9.0".to_string(), "1.9.0.dev0".to_string()),
        ];
        assert_eq!(next_build_number(&builds, "1.9.0", 3).unwrap(), (13, 12));
        assert_eq!(next_build_number(&builds, "1.9.0", 40).unwrap(), (41, 12));
        let closed = vec![("1.10.0".to_string(), "1".to_string())];
        assert!(next_build_number(&closed, "1.9.0", 0).is_err());
    }

    #[test]
    fn query_strings_match_urlencode() {
        assert_eq!(
            urlencode(&[("filter[app]", "1 2"), ("limit", "200")]),
            "filter%5Bapp%5D=1+2&limit=200"
        );
        assert_eq!(normalize_serial("00ab"), "AB");
        assert_eq!(normalize_serial("000"), "0");
    }

    #[test]
    fn api_errors_keep_titles_only() {
        let body = br#"{"errors":[{"title":"Forbidden","detail":"no"},{"title":"X"}]}"#;
        assert_eq!(error_text(body), "Forbidden: no; X");
        assert_eq!(error_text(b"not json"), "");
    }
}
