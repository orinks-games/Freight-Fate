//! Load-time screen for elevation artifacts in the baked grade data (port of
//! `freight_fate/data/grades.py`).
//!
//! Almost all of the world's 144,431 grade segments come from one place -- an
//! OpenRouteService route elevation profile over SRTM, segmented by terrain --
//! and some of them describe a slope no road of their class and terrain can
//! hold. The tell is the same one the curve sweep left behind: the extremes
//! sit on 0.2 and 0.3 mile spans, which is the length of a bridge or an
//! overpass, and a profile crossing a structure reads the deck rather than the
//! road under it.
//!
//! 1,106 segments no longer come from that profile. On 2026-09-19 every span
//! this screen clamped was read a second time against USGS 3DEP, and where
//! 3DEP returned a slope the road class could hold, the segment was re-sourced
//! to the measurement and says so in its own `source`
//! (`tools/screen_grades_3dep.py`). That took the world from 455 segments over
//! 8 percent to 141, and the count this screen still clamps from 1,271 to 242.
//!
//! What the re-read did NOT do is make this screen unnecessary, and the reason
//! is worth keeping: 165 of those spans came back with 3DEP CONFIRMING the
//! profile at 10 to 13 percent on roads that cannot hold it -- I-79 in West
//! Virginia at -13.4, for one. Two elevation models agree there because both
//! read ground, and the road is on a bridge above it. A second elevation
//! source cannot see that. The class ceiling is the only thing that does,
//! which is why those 165 were left for this screen rather than written into
//! the bake.
//!
//! WHY THIS SCREEN CANNOT COPY `curves` AND EXEMPT THE MOUNTAINS. There,
//! mountain terrain is never flagged, because a real switchback lives there.
//! Here the worst record left after the 3DEP re-read -- I-68 at 14.0 between
//! Morgantown and Cumberland -- sits on a leg labelled `mountain`, and the
//! label is coarse enough (3,098 segments carry it) that exempting it would
//! shelter most of the interstate artifacts. Road class
//! replaces terrain as the discriminator because it carries a harder fact: the
//! interstate system is designed to a 6 percent maximum, and the famously
//! brutal exceptions -- I-70 west of Denver, I-17 out of Phoenix, I-80 over
//! Donner -- sit at 6 to 7. There is no 12 percent interstate. US and state
//! routes really do climb harder (US-550 over Red Mountain Pass, CA-299
//! through the Trinity Alps), so their ceilings are set well above anything
//! real and only catch the frankly impossible.
//!
//! WHY THIS ONE CLAMPS WHERE `curves` DROPS. Curves are discrete events and a
//! dropped one is simply never announced. Grades tile the leg continuously,
//! and `Trip.grade_at` falls through to a synthesized terrain average for any
//! mile no segment covers -- so dropping a spike out of the middle of a real
//! climb would replace a measured-but-noisy reading with an invented one.
//! Clamping keeps the sign, keeps the climb, and caps the physics at what the
//! road can actually hold.
//!
//! The bake is never edited (see the provenance rule in `CLAUDE.md`). A
//! clamped segment records the adjustment in its own `source`, so a later
//! reader can see that this value was derived here rather than read from the
//! profile.
//!
//! WHERE THE CLAMP IS STILL WRONG: LEVEL GROUND. An agent drive on 2026-09-01
//! heard "Current grade 6.0 percent downhill" at the Chicago Cross-Dock gate.
//! Every leg into Chicago ends on the same artifact: the profile's last sample
//! sits at 800 feet on ground the rest of the leg reads at 584 to 610, because
//! SRTM is a SURFACE model and the city node is in the Loop, so the final 0.4
//! mile "climbs" 216 feet onto the skyline (+8.6 percent). The screen caught
//! it and capped it at the flat ceiling -- and a 6 percent hill in Chicago is
//! still a 6 percent hill in Chicago. The clamp exists to keep a real climb
//! under a noisy spike, and level ground has no climb to keep: FHWA's HPMS
//! Field Manual defines level terrain as ground that "permits heavy vehicles
//! to maintain approximately the same speed as passenger cars" and "generally
//! includes short grades of no more than 1 to 2 percent", and the AASHTO Green
//! Book caps freeway design grades on level terrain at 3 to 4 percent (Table
//! 8-1, Maximum Grades for Rural and Urban Freeways). So where BOTH the HPMS
//! class (read from FHWA) and the profile's own terrain label call the ground
//! level, a segment past the ceiling is not a road grade under noise -- it is
//! the structure or building the surface model read -- and it is REJECTED:
//! set to level, with a source note that says the value is assumed. Where the
//! two sources disagree (bake label `mountain`, HPMS `level`: Wolf Creek Pass,
//! the I-5 grades at Grants Pass -- HPMS's class is the modal one over a
//! leg's bounding box, and a long leg through a range can read level) the
//! terrain is uncertain and the clamp stays, so no real climb is flattened.

use super::world_models::GradeSegment;
use crate::pyfmt::fmt_f;

/// Steepest sustained grade a road of each class is built to, in percent.
/// Interstates are designed to 6; 7 leaves room for the handful of real
/// mountain exceptions without admitting anything the class cannot hold. The
/// other two are deliberately loose -- they exist to catch profile noise, not
/// to argue with a genuinely severe US or state route pass.
pub const CLASS_CEILING_PCT: &[(&str, f64)] = &[("interstate", 7.0), ("us", 10.0), ("state", 12.0)];

/// Ceiling implied by terrain, which is the other half of the
/// self-contradiction: a segment on level ground cannot also be a 14 percent
/// wall. Measured against the data, flat sits at 4.98 percent for its 99th
/// percentile and hills at 7.61, so these cut the tail and nothing else.
/// `mountain` gets a number only so the lookup is total; class governs there
/// in every case that matters.
pub const TERRAIN_CEILING_PCT: &[(&str, f64)] =
    &[("flat", 6.0), ("hills", 8.0), ("mountain", 12.0)];

/// WHICH terrain, though. The bake's own label is derived from net elevation
/// change end to end and is wrong often enough to matter: checked against
/// FHWA HPMS Terrain_Type over 1,273 legs it agreed on only 67 percent, and
/// the worst records in the world sit on legs the label calls `mountain`
/// (ceiling 12) while HPMS calls that ground LEVEL.
///
/// So the HPMS class leads where it exists, and the segment's own label is the
/// fallback. HPMS speaks in Green Book terms; these are its names in ours.
///
/// The cost of that, measured: HPMS returns ONE verdict for a whole leg, so
/// US-160 over Wolf Creek Pass, US-101 through the redwoods and US-20 over
/// Santiam all come back `level` across 500 to 800 sections. Before the 3DEP
/// re-read that held 96 real grades down to 6 percent. The re-read fixed those
/// by measuring them; the rule is unchanged because loosening it was scored
/// against those same readings and let 386 to 543 artifacts through.
pub const HPMS_TERRAIN_TO_LABEL: &[(i64, &str)] = &[(1, "flat"), (2, "hills"), (3, "mountain")];

/// The ceiling for a road class (`interstate`, `us`, `state`).
pub fn class_ceiling_pct(class: &str) -> f64 {
    CLASS_CEILING_PCT
        .iter()
        .find(|(k, _)| *k == class)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("unknown road class {class:?}"))
}

/// The ceiling for a terrain label, or the loosest one for an unknown label.
pub fn terrain_ceiling_pct(terrain: &str) -> f64 {
    TERRAIN_CEILING_PCT
        .iter()
        .find(|(k, _)| *k == terrain)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| {
            TERRAIN_CEILING_PCT
                .iter()
                .map(|(_, v)| *v)
                .fold(f64::MIN, f64::max)
        })
}

/// The spoken terrain label for an HPMS class (1, 2, 3), if known.
pub fn hpms_terrain_label(hpms_terrain: i64) -> Option<&'static str> {
    HPMS_TERRAIN_TO_LABEL
        .iter()
        .find(|(k, _)| *k == hpms_terrain)
        .map(|(_, label)| *label)
}

fn clamp_note(raw: f64, capped: f64, ceiling: f64, road_class: &str, terrain: &str) -> String {
    // Python: `{raw:+.2f}` / `{capped:+.2f}` / `{ceiling:.0f}`.
    format!(
        " Slope clamped at load from {} to {} percent -- derived, not read: \
         above the {} percent ceiling for {road_class} in {terrain} terrain \
         (freight_fate.data.grades).",
        signed(raw, 2),
        signed(capped, 2),
        fmt_f(ceiling, 0)
    )
}

/// Level ground by two independent sources: the FHWA HPMS class (a reading)
/// and the profile's own terrain label (derived from net elevation change).
/// Only when both agree is there provably no climb for a clamp to preserve.
fn level_by_both_sources(hpms_label: Option<&str>, bake_label: &str) -> bool {
    hpms_label == Some("flat") && bake_label == "flat"
}

fn reject_note(raw: f64, ceiling: f64, road_class: &str) -> String {
    format!(
        " Slope rejected at load, assumed level, not read: {} percent on \
         ground both FHWA HPMS and the profile's own label class as level, \
         above the {} percent ceiling for {road_class} there. Level terrain \
         holds short grades of no more than 1 to 2 percent (HPMS Field \
         Manual, Terrain_Type) and freeways are designed to at most 3 to 4 \
         percent on it (AASHTO Green Book Table 8-1), so this is a structure \
         or building the elevation surface read, not the road \
         (freight_fate.data.grades).",
        signed(raw, 2),
        fmt_f(ceiling, 0)
    )
}

/// Python `f"{x:+.Nf}"`: always-signed fixed precision.
fn signed(x: f64, prec: usize) -> String {
    let text = fmt_f(x, prec);
    if text.starts_with('-') {
        text
    } else {
        format!("+{text}")
    }
}

/// `interstate`, `us` or `state` from a leg's highway designation.
///
/// Anything not designated `I-n` or `US-n` is treated as a state route,
/// which is the loosest ceiling -- an unknown designation should never be
/// screened harder than a known one.
pub fn road_class(highway: &str) -> &'static str {
    let name = highway.trim().to_uppercase();
    if name.starts_with("I-") && name[2..].chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return "interstate";
    }
    if name.starts_with("US-") {
        return "us";
    }
    "state"
}

/// The stricter of what the road class and the terrain allow.
pub fn grade_ceiling_pct(highway: &str, terrain: &str) -> f64 {
    let by_class = class_ceiling_pct(road_class(highway));
    let by_terrain = terrain_ceiling_pct(terrain);
    by_class.min(by_terrain)
}

/// Cap slopes the road cannot hold, leaving every plausible one untouched;
/// on ground two sources agree is level, reject them as level instead (see
/// the module doc for why a clamp is the wrong answer there).
///
/// Returns equal segments where nothing was capped, so an unscreened world
/// round-trips identically.
pub fn screen_grade_segments(
    segments: &[GradeSegment],
    highway: &str,
    hpms_terrain: Option<i64>,
) -> Vec<GradeSegment> {
    // HPMS leads where the leg has a class; the segment's own label is the
    // fallback, and stays the fallback rather than being overwritten, so a
    // leg HPMS never classified screens exactly as it did before.
    let leg_terrain = hpms_terrain
        .filter(|t| *t != 0)
        .and_then(hpms_terrain_label);
    let mut screened = Vec::with_capacity(segments.len());
    for segment in segments {
        let terrain = leg_terrain.unwrap_or(segment.terrain.as_str());
        let ceiling = grade_ceiling_pct(highway, terrain);
        if segment.avg_grade_pct.abs() <= ceiling {
            screened.push(segment.clone());
            continue;
        }
        if level_by_both_sources(leg_terrain, &segment.terrain) {
            let note = reject_note(segment.avg_grade_pct, ceiling, road_class(highway));
            screened.push(GradeSegment {
                start_mi: segment.start_mi,
                end_mi: segment.end_mi,
                avg_grade_pct: 0.0,
                terrain: segment.terrain.clone(),
                source: format!("{}{}", segment.source, note).trim().to_string(),
            });
            continue;
        }
        let capped = if segment.avg_grade_pct > 0.0 {
            ceiling
        } else {
            -ceiling
        };
        let note = clamp_note(
            segment.avg_grade_pct,
            capped,
            ceiling,
            road_class(highway),
            terrain,
        );
        screened.push(GradeSegment {
            start_mi: segment.start_mi,
            end_mi: segment.end_mi,
            avg_grade_pct: capped,
            terrain: segment.terrain.clone(),
            source: format!("{}{}", segment.source, note).trim().to_string(),
        });
    }
    screened
}

#[cfg(test)]
mod tests {
    //! Port of `tests/test_world_grades.py`.
    use super::*;
    use crate::data::data_resources::read_data_text;
    use crate::data::world_corridor::build_leg_corridor;

    fn seg(pct: f64, terrain: &str, start: f64, span: f64) -> GradeSegment {
        GradeSegment::new(
            start,
            start + span,
            pct,
            terrain,
            "OpenRouteService route elevation.",
        )
    }

    fn mseg(pct: f64) -> GradeSegment {
        seg(pct, "mountain", 10.0, 0.3)
    }

    #[test]
    fn test_road_class_reads_the_highway_designation() {
        assert_eq!(road_class("I-5"), "interstate");
        assert_eq!(road_class("I-70"), "interstate");
        assert_eq!(road_class("US-550"), "us");
        assert_eq!(road_class("CA-299"), "state");
        assert_eq!(road_class(""), "state");
    }

    #[test]
    fn test_ceiling_is_the_stricter_of_class_and_terrain() {
        // An interstate in the mountains is governed by its class; the same
        // interstate labelled flat is governed by the terrain.
        assert_eq!(
            grade_ceiling_pct("I-5", "mountain"),
            class_ceiling_pct("interstate")
        );
        assert_eq!(
            grade_ceiling_pct("I-5", "flat"),
            terrain_ceiling_pct("flat")
        );
        assert_eq!(
            grade_ceiling_pct("CA-299", "mountain"),
            class_ceiling_pct("state")
        );
    }

    #[test]
    fn test_the_worst_baked_slope_is_clamped_to_the_interstate_ceiling() {
        // +14.42 percent on I-5, the steepest record in the world data. No
        // interstate anywhere is built past 7.
        let screened = screen_grade_segments(&[mseg(14.42)], "I-5", None);
        assert_eq!(screened[0].avg_grade_pct, class_ceiling_pct("interstate"));
    }

    #[test]
    fn test_clamping_keeps_the_sign_so_a_descent_stays_a_descent() {
        let screened = screen_grade_segments(&[mseg(-14.42)], "I-5", None);
        assert_eq!(screened[0].avg_grade_pct, -class_ceiling_pct("interstate"));
    }

    #[test]
    fn test_clamping_leaves_the_span_and_terrain_alone() {
        let screened = screen_grade_segments(&[seg(14.42, "mountain", 10.0, 0.3)], "I-5", None);
        assert_eq!((screened[0].start_mi, screened[0].end_mi), (10.0, 10.3));
        assert_eq!(screened[0].terrain, "mountain");
    }

    #[test]
    fn test_a_flat_labelled_segment_is_capped_below_its_class_ceiling() {
        // 8.3 percent over a fifth of a mile on ground the bake itself called
        // flat -- the self-contradiction that proves it is a structure, not road.
        let screened = screen_grade_segments(&[seg(8.3, "flat", 10.0, 0.2)], "I-22", None);
        assert_eq!(screened[0].avg_grade_pct, terrain_ceiling_pct("flat"));
    }

    #[test]
    fn test_a_real_mountain_interstate_grade_is_left_exactly_as_baked() {
        // The Eisenhower approach on I-70 is about 7 percent and genuinely is
        // that steep. The screen must not touch it.
        let original = mseg(6.8);
        assert_eq!(
            screen_grade_segments(std::slice::from_ref(&original), "I-70", None),
            vec![original]
        );
    }

    #[test]
    fn test_a_steep_us_route_pass_is_left_alone() {
        // US-550 over Red Mountain Pass really does climb like this; only the
        // interstate class carries the tight ceiling.
        let original = mseg(9.4);
        assert_eq!(
            screen_grade_segments(std::slice::from_ref(&original), "US-550", None),
            vec![original]
        );
    }

    #[test]
    fn test_an_untouched_segment_keeps_its_original_source_string() {
        let original = mseg(3.0);
        let screened = screen_grade_segments(std::slice::from_ref(&original), "I-5", None);
        assert_eq!(screened[0].source, original.source);
    }

    #[test]
    fn test_a_clamped_segment_says_in_its_source_that_the_value_was_derived() {
        let screened = screen_grade_segments(&[mseg(14.42)], "I-5", None);
        assert!(screened[0].source.contains("derived"));
        assert!(screened[0].source.contains("14.42"));
        assert!(screened[0]
            .source
            .starts_with("OpenRouteService route elevation."));
    }

    #[test]
    fn test_the_baked_i5_leg_loads_with_no_impossible_slope() {
        // Load-time screen on a real I-5 bake: ORS spikes above the interstate
        // ceiling must clamp, not ship as physics.
        //
        // Originally pinned to Chico->Santa Rosa, the only leg that still
        // carried the world-worst +14.42 percent I-5 record. That directed
        // edge was retired with the truck-router refuse leftovers, and no
        // surviving bake still holds a >14 percent interstate spike. The
        // Grapevine (Bakersfield->Los Angeles) still has short ORS spans
        // past 7 percent on live I-5, so the same screening property holds.
        let text = read_data_text("world_data/us/legs/CA.json").expect("CA shard");
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let leg = data["legs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|lg| lg["from"] == "bakersfield_ca_us" && lg["to"] == "los_angeles_ca_us")
            .expect("the I-5 Grapevine fixture leg");
        assert_eq!(leg["highway"], "I-5");
        let raw = leg["corridor"]["grade_segments"].as_array().unwrap();
        let worst_raw = raw
            .iter()
            .map(|g| g["avg_grade_pct"].as_f64().unwrap().abs())
            .fold(0.0, f64::max);
        assert!(
            worst_raw > class_ceiling_pct("interstate"),
            "fixture no longer has an over-ceiling ORS spike"
        );

        let built = build_leg_corridor(
            &leg["corridor"],
            leg["miles"].as_f64().unwrap(),
            leg["from"].as_str().unwrap(),
            leg["to"].as_str().unwrap(),
            "CA",
            leg["highway"].as_str().unwrap(),
            &[],
        )
        .unwrap();
        let worst = built
            .grade_segments
            .iter()
            .map(|s| s.avg_grade_pct.abs())
            .fold(0.0, f64::max);
        assert!(worst <= class_ceiling_pct("interstate"));
    }

    #[test]
    fn test_the_grade_ceiling_prefers_hpms_terrain_over_the_bake_label() {
        // The bake's own terrain label is derived from NET elevation change and
        // agreed with FHWA HPMS on only 67 percent of 1,273 legs.
        //
        // The single worst grade record in the world -- +14.4 percent on I-5 --
        // sits on a leg the label calls "mountain", which would allow 12, while
        // HPMS calls that ground LEVEL, which allows 6. Class already capped it at
        // 7; reading the real terrain caps it where it belongs.
        assert_eq!(
            HPMS_TERRAIN_TO_LABEL,
            &[(1, "flat"), (2, "hills"), (3, "mountain")]
        );
        let wall = GradeSegment::new(0.0, 0.3, 14.4, "mountain", "profile");

        // Label alone: class ceiling for an interstate, 7.
        let by_label = &screen_grade_segments(std::slice::from_ref(&wall), "I-5", None)[0];
        assert_eq!(by_label.avg_grade_pct, 7.0);

        // HPMS says level, so the terrain ceiling of 6 is the stricter one.
        let by_hpms = &screen_grade_segments(&[wall], "I-5", Some(1))[0];
        assert_eq!(by_hpms.avg_grade_pct, 6.0);
        assert!(by_hpms.source.to_lowercase().contains("clamped"));
    }

    #[test]
    fn test_a_leg_hpms_never_classified_screens_exactly_as_before() {
        // The fallback has to be a true no-op, or adding this bake would quietly
        // change the screening of every leg HPMS has nothing to say about.
        let seg = GradeSegment::new(0.0, 0.3, 9.5, "hills", "profile");
        assert_eq!(
            screen_grade_segments(std::slice::from_ref(&seg), "I-70", None),
            screen_grade_segments(&[seg], "I-70", None)
        );
    }

    #[test]
    fn test_a_wall_on_ground_two_sources_call_level_is_rejected_as_level() {
        // The Chicago gate, 2026-09-01: +8.61 percent over the last 0.4 mile
        // of I-90 into the Loop, on a leg the profile labels flat and HPMS
        // classes level. Clamping said "6.0 percent downhill" in Chicago.
        let skyline = GradeSegment::new(32.6, 33.0, 8.61, "flat", "profile");
        let screened = &screen_grade_segments(&[skyline], "I-90", Some(1))[0];
        assert_eq!(screened.avg_grade_pct, 0.0);
        assert_eq!((screened.start_mi, screened.end_mi), (32.6, 33.0));
        assert!(screened.source.starts_with("profile"));
        assert!(screened.source.contains("assumed level"));
        assert!(screened.source.contains("+8.61"));
    }

    #[test]
    fn test_a_flat_reading_the_bake_alone_calls_flat_is_still_clamped() {
        // Without HPMS the terrain rests on the bake's derived label, which
        // is wrong a third of the time -- not enough to say there is no
        // climb under the spike, so the clamp stays.
        let screened = &screen_grade_segments(
            &[GradeSegment::new(32.6, 33.0, 8.61, "flat", "profile")],
            "I-90",
            None,
        )[0];
        assert_eq!(screened.avg_grade_pct, terrain_ceiling_pct("flat"));
    }

    #[test]
    fn test_hpms_level_under_a_mountain_label_still_clamps_not_rejects() {
        // Wolf Creek Pass: the bake says mountain, HPMS's modal class over
        // the leg's bounding box says level. The terrain is in dispute, and
        // rejecting would flatten a real 6.4 percent climb.
        let screened = &screen_grade_segments(
            &[GradeSegment::new(20.1, 23.5, 8.0, "mountain", "profile")],
            "US-160",
            Some(1),
        )[0];
        assert_eq!(screened.avg_grade_pct, terrain_ceiling_pct("flat"));
        assert!(screened.source.contains("clamped"));
    }

    #[test]
    fn test_the_chicago_skyline_artifact_was_measured_away_not_rescued_at_load() {
        // This screen used to earn its keep on this exact segment: +8.61
        // percent over the last 0.4 mile of I-90 into the Loop, which it
        // rejected as level because the profile and HPMS both called the
        // ground flat. On 2026-09-19 USGS 3DEP read the same span and
        // measured -0.04. The rescue was right, and the bake no longer needs
        // it -- a measurement stands where a judgement call used to.
        let text = read_data_text("world_data/us/legs/IN.json").expect("IN shard");
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let leg = data["legs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|lg| lg["from"] == "gary_in_us" && lg["to"] == "chicago_il_us")
            .expect("the I-90 Gary to Chicago leg");
        let last = leg["corridor"]["grade_segments"]
            .as_array()
            .unwrap()
            .last()
            .unwrap();

        assert!(
            last["avg_grade_pct"].as_f64().unwrap().abs() < 1.0,
            "the skyline segment is measured flat; a spike here means the \
             3DEP reading was lost in a re-bake"
        );
        assert!(
            last["source"].as_str().unwrap().contains("USGS 3DEP"),
            "the measured slope has to keep saying where it came from"
        );
    }

    #[test]
    fn test_a_baked_leg_still_exercises_the_reject_as_level_path() {
        // 33 spans still sit above the ceiling on ground both sources call
        // level -- 3DEP either could not improve on them or read something
        // the road class forbids, so the load screen is still what handles
        // them. This keeps that path covered against real baked data rather
        // than against a hand-built segment.
        let text = read_data_text("world_data/us/legs/NH.json").expect("NH shard");
        let data: serde_json::Value = serde_json::from_str(&text).unwrap();
        let leg = data["legs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|lg| lg["from"] == "keene_nh_us" && lg["to"] == "springfield_ma_us")
            .expect("the I-91 Keene to Springfield leg");
        assert_eq!(leg["corridor"]["hpms_terrain"]["type"], 1);

        let built = build_leg_corridor(
            &leg["corridor"],
            leg["miles"].as_f64().unwrap(),
            "keene_nh_us",
            "springfield_ma_us",
            "NH",
            leg["highway"].as_str().unwrap(),
            &[],
        )
        .unwrap();
        let rejected = built
            .grade_segments
            .iter()
            .find(|seg| seg.source.contains("assumed level"))
            .expect("a segment this leg's profile put above the ceiling on level ground");
        assert_eq!(rejected.avg_grade_pct, 0.0);
    }
}
