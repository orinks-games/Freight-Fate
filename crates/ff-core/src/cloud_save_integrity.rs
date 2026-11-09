//! Verify orinks.net-signed private cloud revisions before restore.
//!
//! Port of `freight_fate/cloud_save_integrity.py`. The canonical JSON form
//! both sides sign is hand-written here (see [`canonical_profile`]): the
//! server builds its copy with `JSON.stringify`, and the signature only
//! verifies when the two agree on every byte.

use std::collections::BTreeMap;
use std::fmt;

use base64::Engine as _;
use ed25519_dalek::{Signature, VerifyingKey};
use serde_json::Value;

use crate::profile_invariants::{check_profile_invariants, spoken_rejection};

/// Signing keys by key id, raw 32-byte ed25519 public keys (base64 here,
/// decoded by [`public_keys`]).
///
/// The `2026-08-staging` key was dropped at the 1.9 cutover (2026-09-20)
/// alongside the base-URL flip in `online_presence`. It signed against the
/// staged orinks-net deployment while the 1.9 test line pointed there, and
/// leaving it in a production build would mean a payload signed by the
/// staging key still verifying on a player's machine -- which is the whole
/// reason the checklist pairs its removal with the flip. Builds already
/// issued to staging testers still carry it and still reach staging, so
/// nothing in their hands stops working.
pub const PUBLIC_KEYS_B64: &[(&str, &str)] =
    &[("2026-07", "RJ1PR6fVDk98eb3uMysfmvzfURO/wPkLX5O52OapNoY=")];

pub const SUPPORTED_VALIDATOR_VERSION: i64 = 1;

/// The built-in key table, decoded.
pub fn public_keys() -> BTreeMap<String, Vec<u8>> {
    PUBLIC_KEYS_B64
        .iter()
        .map(|(id, b64)| {
            let raw = base64::engine::general_purpose::STANDARD
                .decode(b64)
                .expect("built-in signing key is valid base64");
            (id.to_string(), raw)
        })
        .collect()
}

/// Why a cloud revision was refused. `code` is the stable machine label
/// (`unverified`, `update_required`, `integrity_failed`, `invalid_profile`);
/// `message` is the spoken sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloudSaveIntegrityError {
    pub code: String,
    pub message: String,
}

impl CloudSaveIntegrityError {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.to_string(),
            message: message.to_string(),
        }
    }
}

impl fmt::Display for CloudSaveIntegrityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for CloudSaveIntegrityError {}

/// Serialize a float exactly as JavaScript's `JSON.stringify` does.
///
/// The server signs the canonical form it built with JSON.stringify, where
/// numbers carry no int/float distinction: 6.0 prints as "6", tiny values
/// stay decimal down to 1e-6, exponents are never zero-padded, and -0
/// prints as "0". Python's repr disagrees on all four, so leaning on
/// json.dumps there made every server signature unverifiable (the ".0" on
/// whole floats alone broke every real profile).
pub fn js_number(value: f64) -> Result<String, String> {
    if value.is_nan() || value.is_infinite() {
        return Err("NaN and infinity cannot appear in a profile".to_string());
    }
    if value == 0.0 {
        return Ok("0".to_string());
    }
    let sign = if value < 0.0 { "-" } else { "" };
    // Rust's `{:e}` yields the shortest round-trip digits, the same digits
    // ECMAScript's Number::toString picks; only the layout rules differ.
    let sci = format!("{:e}", value.abs());
    let (mantissa_text, exponent_text) = sci.split_once('e').expect("LowerExp has an exponent");
    let exponent: i32 = exponent_text
        .parse()
        .expect("LowerExp exponent is an integer");
    let mut mantissa: String = mantissa_text.chars().filter(|c| *c != '.').collect();
    // Normalise like Decimal.normalize(): strip trailing zeros from the digits.
    while mantissa.len() > 1 && mantissa.ends_with('0') {
        mantissa.pop();
    }
    let k = mantissa.len() as i32;
    let n = exponent + 1; // value == 0.mantissa * 10**n
    let body = if k <= n && n <= 21 {
        format!("{}{}", mantissa, "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &mantissa[..n as usize], &mantissa[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{}", "0".repeat((-n) as usize), mantissa)
    } else {
        let tail = if k > 1 {
            format!(".{}", &mantissa[1..])
        } else {
            String::new()
        };
        format!(
            "{}{}e{}{}",
            &mantissa[..1],
            tail,
            if n > 0 { "+" } else { "-" },
            (n - 1).abs()
        )
    };
    Ok(format!("{sign}{body}"))
}

/// `json.dumps(value, ensure_ascii=True)` for a lone string: JSON.stringify
/// plus the server's non-ASCII escape pass, byte for byte. Every code point
/// outside `' '..='~'` becomes a lowercase `\uXXXX` (surrogate pairs above
/// the BMP), with the short escapes JSON defines for the usual controls.
pub fn ascii_json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for ch in text.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            ' '..='~' => out.push(ch),
            _ => {
                let mut units = [0u16; 2];
                for unit in ch.encode_utf16(&mut units) {
                    out.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
    }
    out.push('"');
    out
}

fn canonical_value(value: &Value, out: &mut String) -> Result<(), String> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::String(text) => out.push_str(&ascii_json_string(text)),
        Value::Number(number) => {
            if let Some(int) = number.as_i64() {
                // The server parsed every number into a double; an int the
                // double cannot hold exactly would canonicalize differently
                // there, so honest profiles must stay inside the safe-integer
                // range.
                if int.unsigned_abs() > (1u64 << 53) {
                    return Err("integer outside the JSON-safe range".to_string());
                }
                out.push_str(&int.to_string());
            } else if number.as_u64().is_some() {
                // Anything past i64::MAX is past 2**53 as well.
                return Err("integer outside the JSON-safe range".to_string());
            } else {
                let float = number
                    .as_f64()
                    .ok_or_else(|| "unsupported profile value: number".to_string())?;
                out.push_str(&js_number(float)?);
            }
        }
        Value::Object(map) => {
            // Keys sorted by code point, as Python's sorted() orders str.
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push_str(&ascii_json_string(key));
                out.push(':');
                canonical_value(&map[*key], out)?;
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                canonical_value(item, out)?;
            }
            out.push(']');
        }
    }
    Ok(())
}

/// The byte form both sides sign: key-sorted, ASCII-escaped JSON with
/// numbers laid out by JavaScript's rules (see [`js_number`]) -- the server
/// builds its copy with JSON.stringify, and the signature only verifies
/// when the two agree on every byte.
pub fn canonical_profile(payload: &Value) -> Result<Vec<u8>, CloudSaveIntegrityError> {
    let mut out = String::new();
    canonical_value(payload, &mut out).map_err(|_| {
        CloudSaveIntegrityError::new("invalid_profile", "Unsupported profile data.")
    })?;
    Ok(out.into_bytes())
}

/// Python truthiness of a metadata value, for the `all((...))` gate.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) | Some(Value::Bool(false)) => false,
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Number(n)) => n.as_f64().is_some_and(|f| f != 0.0),
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(o)) => !o.is_empty(),
        Some(Value::Bool(true)) => true,
    }
}

/// Check metadata and signature; on success hand back the signed payload
/// (a clone, so the caller can verify it again before committing a cloud
/// restore to disk) after running the caller's own load hook and the hard
/// profile invariants.
///
/// `load_hook` stands in for `Profile.from_dict`: it is handed the payload
/// and returns `Err` when the profile cannot be loaded. The invariants in
/// `profile_invariants` always run; see that module's doc for why a valid
/// signature is not a pardon.
pub fn verify_cloud_revision_with(
    payload: &Value,
    metadata: &Value,
    public_keys: Option<&BTreeMap<String, Vec<u8>>>,
    load_hook: &dyn Fn(&Value) -> Result<(), String>,
) -> Result<Value, CloudSaveIntegrityError> {
    let key_id = metadata.get("keyId");
    let validator_version = metadata.get("validatorVersion");
    let signature_text = metadata.get("sig");
    let signed_at = metadata.get("signedAt");
    if !(truthy(key_id) && truthy(validator_version) && truthy(signature_text) && truthy(signed_at))
    {
        return Err(CloudSaveIntegrityError::new(
            "unverified",
            "orinks.net has not confirmed this backup.",
        ));
    }
    let built_in;
    let keys = match public_keys {
        Some(keys) => keys,
        None => {
            built_in = self::public_keys();
            &built_in
        }
    };
    let key_bytes = match key_id.and_then(Value::as_str).and_then(|id| keys.get(id)) {
        Some(bytes) => bytes,
        None => {
            return Err(CloudSaveIntegrityError::new(
                "update_required",
                "The backup uses a newer signing key.",
            ))
        }
    };
    // `isinstance(validator_version, int)`: a JSON integer, not a float or
    // a bool.
    let version = match validator_version {
        Some(Value::Number(n)) if n.is_i64() || n.is_u64() => n.as_i64().unwrap_or(i64::MAX),
        _ => 0,
    };
    if version < 1 {
        return Err(CloudSaveIntegrityError::new(
            "unverified",
            "The validator version is invalid.",
        ));
    }
    if version > SUPPORTED_VALIDATOR_VERSION {
        return Err(CloudSaveIntegrityError::new(
            "update_required",
            "The backup needs a newer game version.",
        ));
    }
    let signature_text = match (signature_text, signed_at) {
        (Some(Value::String(sig)), Some(Value::String(at))) if !at.is_empty() => sig,
        _ => {
            return Err(CloudSaveIntegrityError::new(
                "unverified",
                "The backup signature metadata is incomplete.",
            ))
        }
    };
    let unreadable =
        || CloudSaveIntegrityError::new("integrity_failed", "The backup signature is unreadable.");
    // base64.b64decode(..., validate=True): strict alphabet, padding required.
    let signature = base64::engine::general_purpose::STANDARD
        .decode(signature_text)
        .map_err(|_| unreadable())?;
    if signature.len() != 64 {
        return Err(unreadable());
    }
    let signature = Signature::from_slice(&signature).map_err(|_| unreadable())?;
    let invalid =
        || CloudSaveIntegrityError::new("integrity_failed", "The backup signature is invalid.");
    let key_array: [u8; 32] = key_bytes.as_slice().try_into().map_err(|_| invalid())?;
    let verifying_key = VerifyingKey::from_bytes(&key_array).map_err(|_| invalid())?;
    let canonical = canonical_profile(payload)?;
    verifying_key
        .verify_strict(&canonical, &signature)
        .map_err(|_| invalid())?;
    // Profile.from_dict normalizes nested save structures in place. Keep
    // the signed payload byte-for-byte stable so callers can verify it
    // again before committing a cloud restore to disk.
    let profile = payload.clone();
    load_hook(&profile).map_err(|_| {
        CloudSaveIntegrityError::new("invalid_profile", "The backup cannot be loaded.")
    })?;
    // Defense in depth behind the signature: a payload blessed by an older
    // validator (or a compromised one) still has to satisfy the invariants
    // every honest save obeys -- see profile_invariants and its doc.
    let violations = check_profile_invariants(&profile);
    if !violations.is_empty() {
        return Err(CloudSaveIntegrityError::new(
            "invalid_profile",
            &spoken_rejection(&violations),
        ));
    }
    Ok(profile)
}

/// [`verify_cloud_revision_with`] with no extra load hook: signature plus
/// the hard profile invariants.
pub fn verify_cloud_revision(
    payload: &Value,
    metadata: &Value,
    public_keys: Option<&BTreeMap<String, Vec<u8>>>,
) -> Result<Value, CloudSaveIntegrityError> {
    verify_cloud_revision_with(payload, metadata, public_keys, &|_| Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use serde_json::json;

    fn signing_key() -> SigningKey {
        // Deterministic test key; any 32 bytes make a valid seed.
        SigningKey::from_bytes(&[7u8; 32])
    }

    fn test_keys(key_id: &str) -> BTreeMap<String, Vec<u8>> {
        let mut keys = BTreeMap::new();
        keys.insert(
            key_id.to_string(),
            signing_key().verifying_key().to_bytes().to_vec(),
        );
        keys
    }

    fn signed_envelope(payload: &Value, key_id: &str) -> Value {
        let signature = signing_key().sign(&canonical_profile(payload).unwrap());
        json!({
            "keyId": key_id,
            "validatorVersion": 1,
            "sig": base64::engine::general_purpose::STANDARD.encode(signature.to_bytes()),
            "signedAt": "2026-07-13T00:00:00Z",
        })
    }

    // -- cross-language canonicalization (the byte form both sides sign) ----

    #[test]
    fn test_canonical_profile_matches_the_server_byte_for_byte() {
        // Expected string produced by the server's own canonicalization
        // (JSON.stringify over sorted keys, then the non-ASCII escape pass) run
        // in Node. The signature only verifies when we lay out numbers the
        // same way: whole floats lose their ".0", decimals hold down to 1e-6,
        // exponents are unpadded, and negative zero prints as "0".
        let payload = json!({
            "b": [1.5, 2.0, 1e-7, 0.00001],
            "a": {"x": -0.0, "y": 129881.73999999999, "z": 29571.0},
            "n": null,
            "s": "café — truck",
            "t": true,
            "big": 1e21,
            "tiny": 8.673617379884035e-19,
            "whole": 6.0,
        });
        let expected = concat!(
            "{\"a\":{\"x\":0,\"y\":129881.73999999999,\"z\":29571},",
            "\"b\":[1.5,2,1e-7,0.00001],\"big\":1e+21,\"n\":null,",
            "\"s\":\"caf\\u00e9 \\u2014 truck\",\"t\":true,",
            "\"tiny\":8.673617379884035e-19,\"whole\":6}"
        );
        assert_eq!(canonical_profile(&payload).unwrap(), expected.as_bytes());
    }

    #[test]
    fn test_whole_float_profile_signature_round_trips() {
        // The exact shape that broke every real restore: ordinary careers carry
        // whole floats (total_miles, reputation), which the old canonical form
        // rendered as "29571.0" while the server had signed "29571".
        let payload = json!({
            "name": "Road Star",
            "money": 77_000.0,
            "career": {"total_miles": 29571.0, "reputation": 50.0},
        });
        let keys = test_keys("test-key");
        let restored = verify_cloud_revision(
            &payload,
            &signed_envelope(&payload, "test-key"),
            Some(&keys),
        )
        .unwrap();
        assert_eq!(restored["money"], json!(77_000.0));
        assert_eq!(
            canonical_profile(&restored).unwrap(),
            canonical_profile(&payload).unwrap()
        );
    }

    #[test]
    fn js_number_lays_out_like_javascript() {
        for (value, expected) in [
            (6.0, "6"),
            (-0.0, "0"),
            (1.5, "1.5"),
            (1e-7, "1e-7"),
            (0.00001, "0.00001"),
            (1e21, "1e+21"),
            (1e20, "100000000000000000000"),
            (123456789012345680000.0, "123456789012345680000"),
            (8.673617379884035e-19, "8.673617379884035e-19"),
            (-2.5e-7, "-2.5e-7"),
            (0.000001, "0.000001"),
            (1e22, "1e+22"),
        ] {
            assert_eq!(js_number(value).unwrap(), expected, "{value}");
        }
        assert!(js_number(f64::NAN).is_err());
        assert!(js_number(f64::INFINITY).is_err());
    }

    #[test]
    fn ascii_json_string_matches_json_dumps_ensure_ascii() {
        assert_eq!(
            ascii_json_string("café — truck"),
            "\"caf\\u00e9 \\u2014 truck\""
        );
        assert_eq!(
            ascii_json_string("a\"b\\c\n\t\u{7f}"),
            "\"a\\\"b\\\\c\\n\\t\\u007f\""
        );
        // Astral code points become surrogate pairs, lowercase hex.
        assert_eq!(ascii_json_string("🚚"), "\"\\ud83d\\ude9a\"");
    }

    #[test]
    fn integers_outside_the_safe_range_are_refused() {
        let payload = json!({"n": 9007199254740993i64});
        let err = canonical_profile(&payload).unwrap_err();
        assert_eq!(err.code, "invalid_profile");
        assert_eq!(err.message, "Unsupported profile data.");
        assert!(canonical_profile(&json!({"n": 9007199254740992i64})).is_ok());
    }

    // -- the metadata gate in verify_cloud_revision -------------------------

    #[test]
    fn test_download_rejects_missing_or_future_verification_metadata() {
        let payload = json!({"name": "Road Star"});
        let keys = test_keys("test-key");
        let mut unsigned = signed_envelope(&payload, "test-key");
        unsigned.as_object_mut().unwrap().remove("sig");
        let missing = verify_cloud_revision(&payload, &unsigned, Some(&keys)).unwrap_err();
        assert_eq!(missing.code, "unverified");
        assert_eq!(missing.message, "orinks.net has not confirmed this backup.");

        let mut future = signed_envelope(&payload, "test-key");
        future["validatorVersion"] = json!(2);
        let newer = verify_cloud_revision(&payload, &future, Some(&keys)).unwrap_err();
        assert_eq!(newer.code, "update_required");
        assert_eq!(newer.message, "The backup needs a newer game version.");
    }

    #[test]
    fn test_download_rejects_payload_changed_after_server_signing() {
        let signed = json!({"name": "Road Star", "money": 77_000.0});
        let changed = json!({"name": "Road Star", "money": 88_000.0});
        let keys = test_keys("test-key");
        let tampered =
            verify_cloud_revision(&changed, &signed_envelope(&signed, "test-key"), Some(&keys))
                .unwrap_err();
        assert_eq!(tampered.code, "integrity_failed");
        assert_eq!(tampered.message, "The backup signature is invalid.");
    }

    #[test]
    fn unknown_key_id_asks_for_an_update() {
        let payload = json!({"name": "Road Star"});
        let keys = test_keys("test-key");
        let err = verify_cloud_revision(&payload, &signed_envelope(&payload, "other"), Some(&keys))
            .unwrap_err();
        assert_eq!(err.code, "update_required");
        assert_eq!(err.message, "The backup uses a newer signing key.");
    }

    #[test]
    fn unreadable_signatures_are_named_as_such() {
        let payload = json!({"name": "Road Star"});
        let keys = test_keys("test-key");
        let mut envelope = signed_envelope(&payload, "test-key");
        envelope["sig"] = json!("not base64!");
        let err = verify_cloud_revision(&payload, &envelope, Some(&keys)).unwrap_err();
        assert_eq!(err.code, "integrity_failed");
        assert_eq!(err.message, "The backup signature is unreadable.");
        envelope["sig"] = json!(base64::engine::general_purpose::STANDARD.encode([1u8; 10]));
        let short = verify_cloud_revision(&payload, &envelope, Some(&keys)).unwrap_err();
        assert_eq!(short.message, "The backup signature is unreadable.");
    }

    #[test]
    fn bad_validator_versions_are_unverified() {
        let payload = json!({"name": "Road Star"});
        let keys = test_keys("test-key");
        let mut envelope = signed_envelope(&payload, "test-key");
        envelope["validatorVersion"] = json!(-1);
        let err = verify_cloud_revision(&payload, &envelope, Some(&keys)).unwrap_err();
        assert_eq!(err.code, "unverified");
        assert_eq!(err.message, "The validator version is invalid.");
        envelope["validatorVersion"] = json!("1");
        let err = verify_cloud_revision(&payload, &envelope, Some(&keys)).unwrap_err();
        assert_eq!(err.message, "The validator version is invalid.");
    }

    #[test]
    fn the_load_hook_failure_reads_as_an_unloadable_backup() {
        let payload = json!({"name": "Road Star"});
        let keys = test_keys("test-key");
        let err = verify_cloud_revision_with(
            &payload,
            &signed_envelope(&payload, "test-key"),
            Some(&keys),
            &|_| Err("boom".to_string()),
        )
        .unwrap_err();
        assert_eq!(err.code, "invalid_profile");
        assert_eq!(err.message, "The backup cannot be loaded.");
    }

    #[test]
    fn the_built_in_key_table_decodes_to_raw_ed25519_keys() {
        let keys = public_keys();
        assert_eq!(keys.len(), 1);
        for (id, raw) in &keys {
            assert_eq!(raw.len(), 32, "{id}");
        }
        assert!(keys.contains_key("2026-07"));
        // Dropped at the 1.9 cutover with the base-URL flip: a shipped build
        // must not verify a revision signed by the staging deployment.
        assert!(!keys.contains_key("2026-08-staging"));
    }
}
