//! A staged drive hands the truck over mid-road: the first frames must not
//! re-announce what the drop already covered (a traffic zone underfoot, a
//! bend call whose window the start landed inside).

use ff_core::data::world::get_world;
use freight_fate::playtest::harness::PlaytestHarness;
use freight_fate::playtest::road::{find_feature_seeded, Hit, RoadOptions};

fn curve_options() -> RoadOptions {
    RoadOptions {
        feature: "curve".to_string(),
        seed: Some(7),
        trip_seed: Some(7),
        ..Default::default()
    }
}

fn curve_hit(origin: &str, destination: &str) -> Option<Hit> {
    let opts = curve_options();
    let pairs = vec![(origin.to_string(), destination.to_string())];
    find_feature_seeded(get_world(), &pairs, "curve", &opts)
        .into_iter()
        .next()
}

#[test]
fn a_staged_drive_does_not_rehear_the_zone_or_the_bend_it_landed_in() {
    // Denver -> Grand Junction, seed 7: the drop put the truck inside a
    // heavy-traffic zone and inside the first bend's call window, so frame
    // one spoke "Traffic is backing up ahead." and a curve call for road
    // the staged drive never showed (probe, 2026-09-26).
    let opts = curve_options();
    let hit = curve_hit("Denver", "Grand Junction").expect("a Denver bend");
    let mut harness = PlaytestHarness::new();
    harness.start_road_feature(&hit, &opts);
    harness.clear_speech();
    harness.drive_frames(2);
    let lines = harness.transcript();
    let text = lines.join("\n").to_lowercase();
    assert!(!text.contains("traffic is backing up"), "{text}");
    assert!(
        !lines.iter().any(|line| {
            let line = line.to_lowercase();
            line.contains("curve") && line.contains("advisory")
        }),
        "a bend call went out for a window the drop landed inside: {text}"
    );
    let profile = harness.app.ctx.profile.as_ref().expect("a career");
    assert!(
        !profile.achievements.iter().any(|a| a == "traffic_slowing"),
        "the staged start awarded Traffic Jam"
    );
}
