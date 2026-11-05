//! The county runs along each leg, which Alt+5 reads.

use crate::data_support::world;
use ff_core::data::counties::county_at;

#[test]
fn every_leg_names_a_county_from_its_start() {
    let world = world();
    let mut bare = Vec::new();
    for leg in &world.legs {
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
