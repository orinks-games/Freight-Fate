//! Additive world overlay loader (port of `tests/test_world_overlay.py`).
//!
//! The overlay lets online-fetched cities and legs be cached and merged on top
//! of the checked-in base for later offline play, without ever overriding the
//! base or changing the offline/deterministic path when no overlay is present.

use std::collections::HashSet;
use std::path::Path;

use crate::data_support::data_dir;
use ff_core::data::world::World;
use serde_json::json;

fn helena_city() -> serde_json::Value {
    json!({
        "state": "Montana",
        "region": "rockies",
        "lat": 46.5891,
        "lon": -112.0391,
        "locations": [{"name": "Helena Freight Terminal", "type": "terminal"}],
    })
}

fn load(overlay: Option<&Path>) -> World {
    World::load_with_overlay(&data_dir(), overlay).unwrap()
}

fn write_overlay(dir: &tempfile::TempDir, overlay: &serde_json::Value) -> std::path::PathBuf {
    let path = dir.path().join("overlay.json");
    std::fs::write(&path, serde_json::to_string(overlay).unwrap()).unwrap();
    path
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "slow: loads the whole world twice"
)]
fn test_load_without_overlay_matches_base() {
    let base = load(None);
    let missing = load(Some(Path::new("definitely-not-a-real-overlay.json")));
    let base_keys: HashSet<&String> = base.cities.keys().collect();
    let missing_keys: HashSet<&String> = missing.cities.keys().collect();
    assert_eq!(missing_keys, base_keys);
    assert_eq!(missing.legs.len(), base.legs.len());
}

#[test]
fn test_overlay_adds_new_city_and_leg() {
    let base = load(None);
    assert!(!base.cities.contains_key("Helena"));
    let overlay = json!({
        "cities": {"Helena": helena_city()},
        "legs": [{
            "from": "Helena", "to": "Salt Lake City", "miles": 480,
            "highway": "I-15", "terrain": "mountain",
        }],
    });
    let dir = tempfile::tempdir().unwrap();
    let path = write_overlay(&dir, &overlay);

    let world = load(Some(&path));
    assert!(world.cities.contains_key("Helena"));
    assert_eq!(world.cities.len(), base.cities.len() + 1);
    assert_eq!(world.legs.len(), base.legs.len() + 1);
    // the new city is wired into the routable network
    assert!(!world.neighbors("Helena").is_empty());
    assert!(world
        .shortest_route("Helena", "Salt Lake City", None, false)
        .unwrap()
        .is_some());
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "slow: loads the whole world again"
)]
fn test_overlay_cannot_override_base_city() {
    let base = load(None);
    let overlay = json!({
        "cities": {
            "Chicago": {
                "state": "Nowhere", "region": "rockies", "lat": 0.0, "lon": 0.0,
                "locations": [{"name": "Bogus Yard", "type": "terminal"}],
            }
        },
        "legs": [],
    });
    let dir = tempfile::tempdir().unwrap();
    let path = write_overlay(&dir, &overlay);

    let world = load(Some(&path));
    // base Chicago wins; the overlay's pre-slug name aliases the base city and
    // its bogus definition is ignored
    assert_eq!(world.cities.len(), base.cities.len());
    assert_eq!(
        world.city("Chicago").unwrap().state,
        base.city("Chicago").unwrap().state
    );
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "slow: loads the whole world again"
)]
fn test_overlay_does_not_duplicate_an_existing_leg() {
    let base = load(None);
    let leg = &base.legs[0];
    // re-add the same leg with endpoints reversed; it must not be duplicated
    let overlay = json!({
        "cities": {},
        "legs": [{
            "from": leg.b, "to": leg.a, "miles": leg.miles,
            "highway": leg.highway, "terrain": leg.terrain,
        }],
    });
    let dir = tempfile::tempdir().unwrap();
    let path = write_overlay(&dir, &overlay);

    let world = load(Some(&path));
    assert_eq!(world.legs.len(), base.legs.len());
}
