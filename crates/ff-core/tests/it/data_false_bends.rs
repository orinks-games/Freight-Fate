//! The fifth curve screen against the shipped map (`data::curve_road`): a
//! sharp bend whose own road does not turn is gone, and the real sharp
//! corners of the map are all still there.

use std::collections::HashMap;

use ff_core::data::curves::{build_from_sources, leg_curves, load, CurveRecord, SHARP_MAX_MPH};

use crate::data_support::{data_dir, world};

fn has_bend(leg: &str, start_mi: f64, direction: char) -> bool {
    leg_curves(leg, true)
        .iter()
        .any(|r| (r.start_mi - start_mi).abs() < 1e-9 && r.direction == direction)
}

/// Carriageway steps the sweep had baked as sharp bends, each checked
/// against OpenStreetMap on 2026-11-23: at every one the route passes where
/// a one-way carriageway of the named road meets its two-way section.
#[test]
fn carriageway_steps_are_no_longer_sharp_bends() {
    for (leg, start, dir, road) in [
        (
            "enid_ok_us:oklahoma_city_ok_us",
            68.95,
            'L',
            "OK 3, Northwest Expressway",
        ),
        (
            "palestine_tx_us:lufkin_tx_us",
            79.00,
            'L',
            "US 69 in Lufkin",
        ),
        (
            "tulsa_ok_us:bartlesville_ok_us",
            41.54,
            'L',
            "US 75, Washington Boulevard",
        ),
        (
            "bentonville_ar_us:harrison_ar_us",
            2.78,
            'R',
            "US 62 and AR 12 in Rogers",
        ),
        (
            "amarillo_tx_us:dumas_tx_us",
            0.77,
            'L',
            "US 87 and US 287 in Amarillo",
        ),
        (
            "roanoke_va_us:greensboro_nc_us",
            10.73,
            'L',
            "US 220, Franklin Road",
        ),
    ] {
        assert!(
            !has_bend(leg, start, dir),
            "{road}: {leg} at mile {start} still has its {dir} bend"
        );
    }
}

/// Real sharp corners in eleven states, each turning on the archived road
/// within five percent of what its record says, and each still a sharp or
/// hairpin call.
#[test]
fn real_sharp_corners_survive_the_road_turn_screen() {
    for (leg, start, dir, place) in [
        (
            "globe_az_us:show_low_az_us",
            37.62,
            'L',
            "US 60, Salt River Canyon",
        ),
        (
            "durango_co_us:montrose_co_us",
            58.64,
            'R',
            "US 550, Red Mountain Pass",
        ),
        (
            "farmington_nm_us:durango_co_us",
            46.08,
            'R',
            "US 550 south of Durango",
        ),
        (
            "eureka_ca_us:redding_ca_us",
            98.23,
            'R',
            "CA 299 over the Coast Ranges",
        ),
        (
            "bentonville_ar_us:harrison_ar_us",
            35.05,
            'L',
            "US 62 in the Ozarks",
        ),
        (
            "morristown_tn_us:johnson_city_tn_us",
            6.29,
            'L',
            "Morristown to Johnson City",
        ),
        (
            "asheville_nc_us:greenville_sc_us",
            2.11,
            'L',
            "leaving Asheville on US 25",
        ),
        (
            "binghamton_ny_us:utica_ny_us",
            84.66,
            'R',
            "NY 12 into Utica",
        ),
        (
            "iron_mountain_mi_us:houghton_mi_us",
            28.64,
            'L',
            "US 141 in the Upper Peninsula",
        ),
        (
            "portland_me_us:montpelier_vt_us",
            38.46,
            'L',
            "US 302 west of Portland",
        ),
        ("ada_ok_us:mcalester_ok_us", 0.91, 'R', "leaving Ada"),
    ] {
        let kept: Option<CurveRecord> = leg_curves(leg, true)
            .into_iter()
            .find(|r| (r.start_mi - start).abs() < 1e-9 && r.direction == dir);
        let kept =
            kept.unwrap_or_else(|| panic!("{place}: {leg} lost its {dir} bend at mile {start}"));
        assert!(
            kept.advisory_mph <= SHARP_MAX_MPH,
            "{place}: still a sharp call"
        );
    }
}

/// The share the meta line reports is the share the screen takes, give or
/// take two points, so a map change that moves it far has to say so.
#[test]
fn the_road_turn_screen_share_in_the_meta_is_current() {
    let read = |rel: &str| std::fs::read_to_string(data_dir().join(rel)).unwrap_or_default();
    let us_curves = read("world_data/us/gameplay/curves.jsonl");
    let meta: serde_json::Value =
        serde_json::from_str(us_curves.lines().next().unwrap_or("{}")).expect("meta line");
    // Every country's rows, joined as the loader joins them, so the build
    // below and `load()` judge the same records.
    let gameplay = |file| {
        ff_core::data::data_resources::country_gameplay_text_at(&data_dir(), file)
            .unwrap_or_default()
    };
    let curves = gameplay("curves.jsonl");
    let artifacts = gameplay("curve_artifacts.jsonl");
    let entry = &meta["meta"]["screens"]["road_turn"];
    let recorded = entry["rejected_share"]
        .as_f64()
        .expect("curves.jsonl meta names the road turn screen's share");

    // The same build with no archive skips the fifth screen alone.
    let without = build_from_sources(&curves, &artifacts, &[], world());
    let sharp = |table: &HashMap<String, Vec<CurveRecord>>| {
        table
            .values()
            .flatten()
            .filter(|r| !r.connector && r.advisory_mph <= SHARP_MAX_MPH)
            .count()
    };
    let judged = sharp(&without);
    let rejected = judged - sharp(load());
    let share = rejected as f64 / judged as f64;
    assert!(
        (share - recorded).abs() < 0.02,
        "the meta says {recorded}; the screen takes {rejected} of {judged} ({share:.3})"
    );
}
