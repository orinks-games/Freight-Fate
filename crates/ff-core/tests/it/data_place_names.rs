//! Town names the road speaks are names of places that still exist.
//!
//! Some corridor checkpoints were named from the USGS place list, which keeps
//! retired census names and old village sites at the same spot as the town
//! that replaced them. "Knife River", Montana was the 2010 census name for
//! Savage, so the drive said both at the same mile. A check against the
//! 2024 Census place list and the current OpenStreetMap nodes found these;
//! this keeps them from coming back with a re-bake.

use crate::data_support::world;

/// Places the map once named that are not towns now, and what is there.
const RETIRED: &[(&str, &str)] = &[
    (
        "Knife River",
        "Savage, Montana: 2010 census name, dropped in 2020",
    ),
    ("Ore'q", "Orick, California: a former Yurok village site"),
    (
        "West Side Highway",
        "Lexington, Washington: a road, not a town",
    ),
    (
        "Picnic Point-North Lynnwood",
        "split into Picnic Point and North Lynnwood for 2020",
    ),
    (
        "Lysander New Community",
        "a 1970s planned town near Radisson, New York, removed from the map",
    ),
];

fn key(name: &str) -> String {
    name.to_lowercase()
        .replace(['.', '-', '\u{2013}'], " ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn test_no_place_is_named_after_a_town_that_is_gone() {
    let mut found = Vec::new();
    for leg in &world().legs {
        let names = leg
            .checkpoints()
            .iter()
            .map(|c| c.name.as_str())
            .chain(leg.landmarks().iter().map(|l| l.name.as_str()));
        for name in names {
            if let Some((_, why)) = RETIRED.iter().find(|(r, _)| *r == name) {
                found.push(format!("{}:{} names {name} ({why})", leg.a, leg.b));
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn test_a_town_is_not_spoken_twice_under_two_spellings() {
    let mut found = Vec::new();
    for leg in &world().legs {
        for cp in leg.checkpoints() {
            if cp.checkpoint_type != "place" {
                continue;
            }
            for lm in leg.landmarks() {
                if lm.category == "village"
                    && lm.name != cp.name
                    && key(&lm.name) == key(&cp.name)
                    && (lm.at_mi - cp.at_mi).abs() <= 1.0
                {
                    found.push(format!(
                        "{}:{} {} at {} and {} at {}",
                        leg.a, leg.b, cp.name, cp.at_mi, lm.name, lm.at_mi
                    ));
                }
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}
