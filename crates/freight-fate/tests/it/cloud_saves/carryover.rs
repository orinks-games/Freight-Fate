//! A 1.9 cloud backup restored in 2.0 lands the way a copied 1.9 career
//! does: stamped 2.0, its home taken from the first delivery, and the
//! carrier choice owed only where its carrier no longer runs nationwide.

use super::*;
use ed25519_dalek::{Signer, SigningKey};
use ff_core::cloud_save_integrity::canonical_profile;
use ff_core::models::carryover::Carryover;
use ff_core::models::profile::is_pre_1_9_save;

const KEY_ID: &str = "carryover-test-key";

fn signer() -> SigningKey {
    SigningKey::from_bytes(&[29; 32])
}

/// A 1.9 career as its cloud backup holds it: no carry-over field and no
/// home city, both of which 1.9 never wrote.
fn backup_1_9(name: &str, carrier: (&str, &str), first_delivery: &str) -> Value {
    let mut p = Profile::named_in(name, "miami_fl_us");
    p.carrier_key = carrier.0.into();
    p.carrier_name = carrier.1.into();
    let mut d = p.to_dict();
    for key in ["carryover", "home_city", "home_terminal_city", "_signature"] {
        d.remove(key);
    }
    d.insert("created_line".into(), json!("1.9"));
    d.insert(
        "achievement_stats".into(),
        json!({ "home_city": first_delivery }),
    );
    Value::Object(d)
}

fn keys() -> PublicKeys {
    let mut keys = PublicKeys::new();
    keys.insert(KEY_ID.into(), signer().verifying_key().to_bytes().to_vec());
    keys
}

/// Sign the backup with the test key, download it and restore it.
fn restore_backup(dict: &Value) -> Profile {
    let (content, _) = cloud_content(dict);
    let portable = profile_dict_from_content(&content).unwrap();
    let signature = signer().sign(&canonical_profile(&portable).unwrap());
    let sig = base64::engine::general_purpose::STANDARD.encode(signature.to_bytes());
    let mut reply = make_cloud_reply(dict, &sig, 3);
    reply["keyId"] = json!(KEY_ID);
    let name = dict["name"].as_str().unwrap();
    let payload = download_save(
        &identity(),
        name,
        None,
        FakeTransport::replying(reply).as_ref(),
        Some(&keys()),
    )
    .unwrap()
    .expect("a payload");
    let hooks = RestoreHooks {
        is_legacy: &|v: &Value| v.as_object().is_none_or(is_pre_1_9_save),
        write: &install_restored_profile,
    };
    let path = restore_to_disk(&payload, None, &hooks, Some(&keys())).unwrap();
    Profile::load(&path).unwrap()
}

#[test]
fn a_restored_prairie_link_backup_owes_the_carrier_choice() {
    let _app = TestApp::new();
    let p = restore_backup(&backup_1_9(
        "Plains Backup",
        ("prairie_link", "Prairie Link Regional"),
        "miami_fl_us",
    ));
    assert_eq!(p.created_line, "2.0");
    assert_eq!(p.carryover, Carryover::CarrierChoiceDue);
    assert!(p.carrier_choice_due());
    assert_eq!(p.home_city, "miami_fl_us");
    assert_eq!(p.home_terminal_city, "miami_fl_us");
    assert!(p.carrier_choice_notice().is_some());
    // Loading again changes nothing: the carry-over ran once.
    let again = Profile::load(&ff_core::models::profile::save_path_for("Plains Backup")).unwrap();
    assert_eq!(again.carryover, Carryover::CarrierChoiceDue);
    assert!(!again.needs_migration_resave);
}

#[test]
fn a_restored_national_backup_is_settled_with_its_first_delivery_home() {
    let _app = TestApp::new();
    let p = restore_backup(&backup_1_9(
        "North Backup",
        ("northstar", "Northstar Freight"),
        "chicago_il_us",
    ));
    assert_eq!(p.created_line, "2.0");
    assert_eq!(p.carryover, Carryover::Settled);
    assert!(!p.carrier_choice_due());
    assert_eq!(p.home_city, "chicago_il_us");
    assert_eq!(p.home_terminal_city, "chicago_il_us");
    assert_eq!(p.current_city, "miami_fl_us");
}
