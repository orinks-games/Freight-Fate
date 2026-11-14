//! The county runs along each leg, which Alt+5 reads.

use crate::data_support::world;
use ff_core::data::counties::county_at;

/// The legs with an archived U.S. route, in either direction: the county
/// layer is derived from that route, so a leg without one (Alaska, Canada)
/// has nothing to place a county line on.
fn archived_routes() -> std::collections::HashSet<String> {
    let dir =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/world_data/us/geometry");
    let mut legs = std::collections::HashSet::new();
    for entry in std::fs::read_dir(&dir).expect("the geometry archive") {
        let text = std::fs::read_to_string(entry.expect("an archive file").path())
            .expect("an archive shard");
        for line in text.lines().filter(|l| !l.starts_with("{\"meta\"")) {
            let Ok(row) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            if let Some(leg) = row.get("leg").and_then(|v| v.as_str()) {
                legs.insert(leg.to_string());
            }
        }
    }
    legs
}

#[test]
fn every_leg_names_a_county_from_its_start() {
    let world = world();
    let routes = archived_routes();
    let mut bare = Vec::new();
    for leg in &world.legs {
        let archived = routes.contains(&format!("{}:{}", leg.a, leg.b))
            || routes.contains(&format!("{}:{}", leg.b, leg.a));
        if !archived {
            continue;
        }
        let runs = leg.counties();
        if runs.first().is_none_or(|run| run.at_mi != 0.0) {
            bare.push(format!("{} to {}", leg.a, leg.b));
        }
    }
    assert!(bare.is_empty(), "legs without a county at mile 0: {bare:?}");
}

#[test]
fn aurora_to_milwaukee_crosses_lake_county_before_the_state_line() {
    // I-294 and I-94 north of Chicago: Cook, then Lake, then Kenosha just
    // past the Wisconsin line.
    let world = world();
    let leg = world
        .legs
        .iter()
        .find(|leg| leg.a == "aurora_il_us" && leg.b == "milwaukee_wi_us")
        .expect("the Aurora to Milwaukee leg");
    assert_eq!(county_at(leg.counties(), 30.0), "Cook County");
    assert_eq!(county_at(leg.counties(), 60.0), "Lake County");
    let line = leg.state_crossings()[0].at_mi;
    assert_eq!(county_at(leg.counties(), line - 0.5), "Lake County");
    assert_eq!(county_at(leg.counties(), line + 0.5), "Kenosha County");
}
