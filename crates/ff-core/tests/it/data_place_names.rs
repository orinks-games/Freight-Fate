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

/// No landmark line says "X ahead" (owner rule, 2026-11-09): a museum is
/// "Passing ...". Billboard copy is the one exception, being the board's own
/// words.
#[test]
fn no_landmark_line_says_ahead() {
    let mut found: Vec<String> = Vec::new();
    for leg in &world().legs {
        for l in leg.landmarks() {
            if l.category == "billboard_sign" {
                continue;
            }
            if l.spoken.ends_with(" ahead") || l.spoken.starts_with("Museum ahead:") {
                found.push(format!("{}:{} {}", leg.a, leg.b, l.spoken));
            }
        }
    }
    assert!(
        found.is_empty(),
        "{} lines, first {:#?}",
        found.len(),
        &found[..found.len().min(5)]
    );
}

/// A character a game voice reads in a Latin name: ASCII, Latin-1 and the
/// Latin Extended letters, combining accents, Latin Extended Additional, and
/// general punctuation.
fn readable(c: char) -> bool {
    matches!(c,
        '\u{0}'..='\u{7f}'
        | '\u{a0}'..='\u{24f}'
        | '\u{300}'..='\u{36f}'
        | '\u{1e00}'..='\u{1eff}'
        | '\u{2000}'..='\u{206f}')
}

/// No spoken name holds a script the voice cannot read (2026-11-22): Cree
/// syllabics in "ᐸᐢᒁᐤ Pasqua First Nation" came out as silence or as
/// character names. The map tools keep `name:en`, or the Latin part.
#[test]
fn no_spoken_name_holds_a_script_the_voice_cannot_read() {
    let mut found: Vec<String> = Vec::new();
    for leg in &world().legs {
        let names = leg
            .landmarks()
            .iter()
            .flat_map(|l| [l.name.as_str(), l.spoken.as_str()])
            .chain(leg.checkpoints().iter().map(|c| c.name.as_str()))
            .chain(leg.interchanges().iter().map(|i| i.name.as_str()))
            .chain(leg.toll_events().iter().map(|t| t.name.as_str()))
            .chain(leg.counties().iter().map(|c| c.name.as_str()))
            .chain(leg.state_crossings().iter().map(|s| s.place.as_str()))
            .chain(leg.stops.iter().map(|s| s.name.as_str()));
        for name in names {
            if !name.chars().all(readable) {
                found.push(format!("{}:{} {name}", leg.a, leg.b));
            }
        }
    }
    found.sort();
    found.dedup();
    assert!(
        found.is_empty(),
        "{} names: {:#?}",
        found.len(),
        &found[..found.len().min(10)]
    );
}

/// Trail passes beside the highway are not named as if the road went over
/// them (2026-11-22): "Approaching Saddleback Pass" came on Highway 1 by
/// Lake Louise for a hiking pass 3.9 km off. The map tool keeps a pass only
/// within 0.4 mile of the leg's road.
#[test]
fn a_trail_pass_beside_the_highway_is_not_named() {
    const TRAIL_PASSES: &[&str] = &["Saddleback Pass", "Balu Pass", "Firebrand Pass"];
    let mut found = Vec::new();
    for leg in &world().legs {
        for l in leg.landmarks() {
            if l.category == "mountain_pass" && TRAIL_PASSES.contains(&l.name.as_str()) {
                found.push(format!("{}:{} {}", leg.a, leg.b, l.name));
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}
