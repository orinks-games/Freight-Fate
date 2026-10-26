//! The learn-sounds catalog: every entry plays something real and says
//! what it means.
//!
//! The data invariants are pinned here.
use super::*;
use std::collections::HashSet;

#[test]
fn mechanical_blinker_is_taught_separately_from_the_i_key_locator() {
    let blinker = entry_by_name("Mechanical blinker").unwrap();
    assert!(blinker
        .plays
        .iter()
        .all(|cue| cue.key == "vehicle/turn_signal"));
    let locator = entry_by_name("Lane locator").unwrap();
    assert!(locator
        .plays
        .iter()
        .all(|cue| cue.key == "vehicle/lane_locator"));
}

#[test]
fn test_catalog_has_categories_with_entries() {
    assert!(!CATALOG.is_empty(), "the catalog is empty");
    for category in CATALOG {
        assert!(!category.name.is_empty(), "a category has no name");
        assert!(
            !category.entries.is_empty(),
            "{} has no entries",
            category.name
        );
    }
}

#[test]
fn test_every_entry_names_itself_plays_something_and_explains_itself() {
    for entry in catalog_entries() {
        assert!(!entry.name.trim().is_empty(), "an entry has no name");
        assert!(!entry.plays.is_empty(), "{} plays nothing", entry.name);
        assert!(
            !entry.meaning.trim().is_empty(),
            "{} has no meaning text",
            entry.name
        );
    }
}

#[test]
fn test_no_entry_text_carries_a_run_of_spaces() {
    // Five entries lost their backslash line continuations and shipped
    // fourteen-space runs to the Learn game sounds screen, which reads them
    // to speech and braille as written (review I9, 2026-09-19). A literal
    // split across lines has to be joined with `\`; this catches the next
    // one that is not.
    for entry in catalog_entries() {
        for (what, text) in [
            ("name", entry.name),
            ("meaning", entry.meaning),
            ("when", entry.when),
        ] {
            assert!(
                !text.contains("  "),
                "{}: its {what} has a run of spaces: {text:?}",
                entry.name
            );
        }
    }
}

#[test]
fn test_engine_lean_when_text_says_what_the_director_does() {
    // Review I10: the text taught that the lean never happens with the
    // lane-departure warning off, which was false for every bend. It now
    // has to name the four facts the code has: turns in every mode, drift
    // only with the warning on, the tone replacing it, and the reversal.
    let when = entry_by_name("The engine lean")
        .expect("The engine lean is catalogued")
        .when;
    for fact in [
        "Every bend and street corner, in every lane keeping mode",
        "lane departure warning is on",
        "set to tone",
        "Steering guide row reverses",
    ] {
        assert!(when.contains(fact), "missing {fact:?} in {when:?}");
    }
    assert!(!when.contains("never happens"), "{when:?}");
}

#[test]
fn test_lane_category_teaches_the_edge_ladder_in_order() {
    let lane = CATALOG
        .iter()
        .find(|c| c.name == "Lane and steering")
        .unwrap();
    let names: Vec<&str> = lane.entries.iter().map(|e| e.name).collect();
    let index = |n: &str| names.iter().position(|x| *x == n).unwrap();
    assert!(index("Rumble strip, clipped") < index("Rumble strip"));
    assert!(index("Rumble strip") < index("Off the pavement"));
}

// Entries where the side is a property of the truck's position, not of the
// event: whichever side of the lane you are on, or whichever side a police
// vehicle went by on, both are ordinary. Demoing one of them teaches half the
// cue. Turn left, turn right and the siren are deliberately NOT here -- their
// side IS the information.
const BOTH_SIDES_ENTRIES: &[&str] = &[
    "Mechanical blinker",
    "The engine lean",
    "Where you sit in the lane",
    "Rumble strip, clipped",
    "Rumble strip",
    "Off the pavement",
    "Lane line crossed",
    "Lane locator",
    "Signal tone",
    "Police car going by",
];

#[test]
fn test_directional_entries_demo_both_sides() {
    for name in BOTH_SIDES_ENTRIES {
        let entry = entry_by_name(name).unwrap();
        let mut pans: Vec<f64> = entry.plays.iter().map(|c| c.pan).collect();
        pans.sort_by(|a, b| a.partial_cmp(b).unwrap());
        assert!(
            pans[0] < 0.0 && 0.0 < pans[pans.len() - 1],
            "{name} must demo left and right"
        );
    }
}

const EXPECTED_CATEGORIES: &[&str] = &[
    "Lane and steering",
    "Air and brakes",
    "Engine brake, speed and shifting",
    "Ramps and stop bars",
    "Hazards and the road",
    "Enforcement",
    "The load",
];

#[test]
fn test_all_seven_categories_are_present_in_order() {
    let names: Vec<&str> = CATALOG.iter().map(|c| c.name).collect();
    assert_eq!(names, EXPECTED_CATEGORIES);
}

#[test]
fn test_the_dodge_outcome_ships_as_a_learnable_success_fail_pair() {
    // R14: an earcon that reports an outcome ships as a distinct success/fail
    // pair, both learnable. Terse mode leans on the hazard-clear chime as the
    // whole 'you cleared it' confirmation, so its opposite -- the collision --
    // must be catalogued alongside it, not left as an implicit sound.
    let keys = catalog_keys();
    assert!(keys.contains("events/hazard_clear"));
    assert!(keys.contains("vehicle/collision"));
    // Neither half may quietly fall back to the "self-explanatory" exclusion.
    assert!(!is_excluded("vehicle/collision"));
    assert!(!is_excluded("events/hazard_clear"));
}

#[test]
fn test_no_entry_name_repeats_across_the_catalog() {
    let names: Vec<&str> = catalog_entries().map(|e| e.name).collect();
    let unique: HashSet<&str> = names.iter().cloned().collect();
    assert_eq!(names.len(), unique.len(), "two entries share a name");
}

#[test]
fn test_held_cues_declare_a_duration_and_one_shots_do_not_linger() {
    for entry in catalog_entries() {
        for cue in entry.plays {
            assert!(cue.hold_s >= 0.0);
            assert!(
                cue.hold_s <= 6.0,
                "{} holds {} too long",
                entry.name,
                cue.key
            );
        }
    }
}

#[test]
fn test_the_emergency_brake_entry_declares_a_fallback() {
    // vehicle/ebrake ships only in the licensed overlay; a clean clone must
    // still hear something rather than learning that the cue is silent.
    let entry = entry_by_name("Emergency brake").unwrap();
    let cue = entry.plays[0];
    assert_eq!(cue.key, "vehicle/ebrake");
    assert_eq!(cue.fallback, "vehicle/brake_air");
}

#[test]
fn test_every_exclusion_carries_a_reason() {
    for (key, reason) in SELF_EXPLANATORY {
        assert!(
            !reason.trim().is_empty(),
            "{key} is excluded with no reason given"
        );
    }
}

#[test]
fn test_nothing_is_both_taught_and_excluded() {
    let both: Vec<&str> = catalog_keys()
        .into_iter()
        .filter(|k| is_excluded(k))
        .collect();
    assert!(both.is_empty(), "catalogued and excluded at once: {both:?}");
}

#[test]
fn test_a_folder_glob_excludes_the_whole_folder() {
    assert!(is_excluded("weather/rain_loop"));
    assert!(is_excluded("music/open_road"));
    assert!(!is_excluded("engine/jake_1600"));
    assert!(is_excluded("engine/idle"));
}

// Entries whose cue a setting can silence, delay or change the meaning of.
// Nothing in the data says so -- the gating lives at the call site -- so this
// list is kept by hand: catalogue a cue that a setting governs, and add its
// name here in the same change.
const SETTINGS_GATED_ENTRIES: &[&str] = &[
    "Mechanical blinker",
    "Blinker off",
    "The engine lean",
    "Where you sit in the lane",
    "Rumble strip, clipped",
    "Rumble strip",
    "Off the pavement",
    "Back in the lane",
    "Lane locator",
    "Overspeed chime",
    "Gear grind",
    "Police car going by",
];

#[test]
fn test_every_settings_gated_entry_says_when_it_sounds() {
    for name in SETTINGS_GATED_ENTRIES {
        let entry = entry_by_name(name)
            .unwrap_or_else(|| panic!("{name} is no longer in the catalog; fix this list"));
        assert!(
            !entry.when.trim().is_empty(),
            "{name} only sounds under some settings, so it must say which. \
             A player told a cue means one thing, whose settings mean it means \
             another, has been taught something false."
        );
    }
}

#[test]
fn test_the_enforcement_entries_match_what_the_road_plays() {
    // The warning and the pass are different cues and must stay so.
    //
    // The catalog once taught the pass as the thing "heard before it can see
    // you". It is not: it fires a twentieth of a mile PAST the post. The cue
    // that arrives first is the marker, and these two entries are only worth
    // having if each keeps the recipe of the thing it names.
    //
    // The road's own constants (driving_siren.SIGNATURE_KEY /
    // PASS_MARKER_LEAD_S, driving_enforcement.PASS_BASE_VOLUME / PASS_PAN)
    // are pinned by value here until the states port lands.
    const SIGNATURE_KEY: &str = "enforcement/signature";
    const PASS_MARKER_LEAD_S: f64 = 0.2;
    const PASS_BASE_VOLUME: f64 = 0.7;
    const PASS_PAN: f64 = 0.55;

    let enforcement = CATALOG.iter().find(|c| c.name == "Enforcement").unwrap();
    let by_name = |n: &str| enforcement.entries.iter().find(|e| e.name == n).unwrap();

    let marker = by_name("Enforcement marker");
    let keys: Vec<&str> = marker.plays.iter().map(|c| c.key).collect();
    assert_eq!(keys, vec![SIGNATURE_KEY]);
    assert_eq!(
        marker.plays[0].volume, 0.75,
        "the marker's own level in _play_enforcement_marker"
    );
    assert_eq!(marker.plays[0].pan, 0.0, "the pre-post marker is centered");

    let passing = by_name("Police car going by");
    assert_eq!(
        passing.plays.len() % 2,
        0,
        "the pass is marker-then-vehicle pairs"
    );
    for pair in passing.plays.chunks(2) {
        let (lead, behind) = (pair[0], pair[1]);
        assert_eq!(
            lead.key, SIGNATURE_KEY,
            "the marker leads the whoosh, never the other way"
        );
        assert_eq!(behind.key, "traffic/trooper_pass");
        assert!(((behind.delay_s - lead.delay_s) - PASS_MARKER_LEAD_S).abs() < 1e-3);
        assert_eq!(
            lead.pan, behind.pan,
            "both halves of one pass come from one side"
        );
        assert_eq!(lead.pan.abs(), PASS_PAN);
        assert_eq!(lead.volume, PASS_BASE_VOLUME);
        assert_eq!(behind.volume, PASS_BASE_VOLUME);
    }
}

#[test]
fn test_the_jake_ring_is_catalogued_by_hand() {
    // Built by f-string at the call site, so the Python scanner cannot see it.
    // Catalogued explicitly, which is why this asserts rather than trusts.
    assert!(catalog_keys().iter().any(|k| k.starts_with("engine/jake_")));
}

#[test]
fn test_descriptions_stay_player_facing() {
    let banned = [
        "src/",
        ".py",
        "CH_",
        "audio.play",
        "TODO",
        "FIXME",
        "changelog",
        "pytest",
    ];
    for entry in catalog_entries() {
        let text = format!("{} {}", entry.meaning, entry.when);
        for word in banned {
            assert!(
                !text.contains(word),
                "{} says {word:?} to the player",
                entry.name
            );
        }
    }
}

fn entry(name: &str) -> &'static SoundEntry {
    entry_by_name(name).unwrap()
}

#[test]
fn test_the_engine_lean_is_taught_as_a_cue_you_steer_toward() {
    // The lane guide is a pursuit instrument and the rumble strip is not.
    //
    // Its target is `curve_steer - offset` (sim/lane_guidance), so drifting
    // right leans the bed left and following the lean is what recovers the
    // lane -- the opposite of the rumble strip, which sounds from the side
    // being drifted toward and is steered away from. Prose is the only place
    // that difference can live, and getting it backwards would teach a blind
    // driver to steer off the road, so it is pinned here rather than trusted.
    let lean = entry("The engine lean");
    assert!(lean.meaning.contains("Steer toward the engine"));
    // And the bed, which the guide used to ride, must NOT read as something
    // to follow now that it only reports where the truck already is.
    let seat = entry("Where you sit in the lane");
    assert!(
        !seat.meaning.contains("Steer toward"),
        "the position readout must not be taught as a pursuit cue"
    );
    for rung in ["Rumble strip, clipped", "Rumble strip"] {
        assert!(
            entry(rung).meaning.to_lowercase().contains("away"),
            "{rung} must keep telling the player to steer away from it, \
             or the two opposite conventions blur together"
        );
    }
}

#[test]
fn test_the_weigh_station_bed_demos_louder_than_the_road_plays_it() {
    // The one cue whose road level makes it undemonstrable.
    //
    // Mixed to sit *under* engine and tyre noise, it works on the road by
    // swelling against them. Played in a silent menu at that same level it is a
    // featureless hiss and reads as nothing happening (Shane, 2026-08-15: "I
    // press enter on it and I get silence"). The Learn game sounds screen has to
    // play it above its road ceiling to demonstrate it at all, so that is
    // pinned here rather than left to drift back down.
    //
    // driving_enforcement.SCALE_BED_OPEN_MAX_VOLUME, pinned by value until
    // the states port lands.
    const SCALE_BED_OPEN_MAX_VOLUME: f64 = 0.55;

    let entry = catalog_entries()
        .find(|e| e.plays.iter().any(|c| c.key == "poi/weigh_station_lane"))
        .unwrap();
    let cue = entry
        .plays
        .iter()
        .find(|c| c.key == "poi/weigh_station_lane")
        .unwrap();

    assert!(
        cue.volume > SCALE_BED_OPEN_MAX_VOLUME,
        "the weigh station bed must demo above the loudest the road ever \
         plays it ({SCALE_BED_OPEN_MAX_VOLUME}), or the screen teaches the \
         player that a real cue is silent"
    );
    // Long enough to register as a sound rather than a blip of hiss.
    assert!(cue.hold_s >= 5.0);
}

#[test]
fn test_the_lane_guide_tone_is_learnable_like_every_other_cue() {
    // R14: a sound a player cannot look up is information removed.
    let matched = catalog_entries()
        .find(|e| e.plays.iter().any(|c| c.key == "guide/lane_guide_tone"))
        .expect("the guide tone is not in Learn game sounds");
    // And it says it is the non-default, so a player auditioning sounds is
    // not left wondering why they have never heard it.
    assert!(matched.meaning.to_lowercase().contains("default"));
}
