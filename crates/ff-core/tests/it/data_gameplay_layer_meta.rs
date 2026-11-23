//! Every gameplay layer file (`data/world_data/<country>/gameplay/*.jsonl`)
//! opens with exactly one meta row, `{"meta": {...}}`, and no other row is
//! one. The meta's `data_version` is the hash the layer tools write:
//! `sha256:` and the first 12 hex digits of SHA-256 over the row lines
//! joined by newlines. A leg-by-leg merge on 2026-11-23 wrote the hash
//! beside `meta` instead of inside it, and a tool run later put a fresh
//! meta row above that one.

use sha2::{Digest, Sha256};
use std::path::Path;

#[test]
fn every_gameplay_layer_has_one_meta_row_first_and_its_hash_matches_the_rows() {
    let world_data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/world_data");
    let mut files = Vec::new();
    for country in std::fs::read_dir(&world_data).expect("world_data") {
        let gameplay = country.expect("country dir").path().join("gameplay");
        let Ok(entries) = std::fs::read_dir(&gameplay) else {
            continue;
        };
        for entry in entries {
            let path = entry.expect("layer file").path();
            if path.extension().is_some_and(|e| e == "jsonl") {
                files.push(path);
            }
        }
    }
    assert!(!files.is_empty(), "no gameplay layers under {world_data:?}");
    for path in files {
        let text = std::fs::read_to_string(&path).expect("layer text");
        let lines: Vec<&str> = text.lines().collect();
        let name = path.display();
        let first: serde_json::Value = serde_json::from_str(lines[0]).expect("meta row parses");
        let keys: Vec<&String> = first.as_object().expect("meta row object").keys().collect();
        assert_eq!(keys, ["meta"], "{name}: the first line holds only the meta");
        for (i, line) in lines.iter().enumerate().skip(1) {
            let row: serde_json::Value = serde_json::from_str(line).expect("row parses");
            assert!(
                row.get("meta").is_none() && row.get("leg").is_some(),
                "{name}: line {} is not a leg row",
                i + 1
            );
        }
        let digest = Sha256::digest(lines[1..].join("\n").as_bytes());
        let hash = format!("sha256:{}", &hex::encode(digest)[..12]);
        assert_eq!(
            first["meta"]["data_version"].as_str(),
            Some(hash.as_str()),
            "{name}: data_version does not hash the rows"
        );
    }
}
