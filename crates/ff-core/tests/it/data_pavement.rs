//! The HPMS pavement layer: surface runs and structures along each leg.

use std::sync::Arc;

use crate::data_support::world;
use ff_core::data::pavement::{Deck, Grooves, StructureKind, SurfaceGroup};
use ff_core::data::world_models::Leg;

fn leg<'a>(legs: &'a [Arc<Leg>], a: &str, b: &str) -> &'a Leg {
    legs.iter()
        .find(|leg| leg.a == a && leg.b == b)
        .unwrap_or_else(|| panic!("the {a} to {b} leg"))
}

#[test]
fn most_legs_have_a_layer_and_the_screens_drop_little() {
    let world = world();
    let (mut with, mut records, mut screened) = (0, 0, 0);
    // HPMS covers only US roads: a leg touching Canada has at most its US
    // miles read, so the share counts the legs wholly in the US.
    let us_legs = world
        .legs
        .iter()
        .filter(|l| !l.a.ends_with("_ca") && !l.b.ends_with("_ca"))
        .count();
    for leg in &world.legs {
        let Some(p) = leg.pavement() else { continue };
        if !leg.a.ends_with("_ca") && !leg.b.ends_with("_ca") {
            with += 1;
        }
        records += p.surface.len() + p.structures.len() + p.screened;
        screened += p.screened;
        for (read, assumed) in [
            (p.read_share_forward, p.inventory_share_forward),
            (p.read_share_reverse, p.inventory_share_reverse),
        ] {
            assert!(assumed <= read + 1e-9, "{} to {}", leg.a, leg.b);
        }
    }
    assert!(with * 10 >= us_legs * 9, "{with} of {us_legs} US legs");
    assert!(
        screened * 200 <= records,
        "{screened} of {records} records screened"
    );
}

#[test]
fn one_way_never_reads_two_surfaces_at_once() {
    let world = world();
    for leg in &world.legs {
        let Some(p) = leg.pavement() else { continue };
        for forward in [true, false] {
            let mut runs: Vec<_> = p.surface.iter().filter(|r| r.ways.holds(forward)).collect();
            runs.sort_by(|a, b| a.from_mi.total_cmp(&b.from_mi));
            for pair in runs.windows(2) {
                assert!(
                    pair[1].from_mi >= pair[0].to_mi - 1e-6,
                    "{} to {} overlaps at {}",
                    leg.a,
                    leg.b,
                    pair[1].from_mi
                );
            }
        }
    }
}

#[test]
fn i70_east_of_columbus_is_asphalt_both_ways() {
    let world = world();
    let p = leg(&world.legs, "columbus_oh_us", "zanesville_oh_us")
        .pavement()
        .expect("HPMS reads Ohio's I-70");
    for forward in [true, false] {
        let run = p.surface_at(20.0, forward).expect("a reading at mile 20");
        assert_eq!(run.surface.group(), SurfaceGroup::Asphalt);
    }
}

#[test]
fn the_turnpike_tunnels_west_of_carlisle_are_on_the_leg() {
    // Blue Mountain and Kittatinny, then Tuscarora, each way. HPMS 2020
    // has no row for the Allegheny Mountain Tunnel (see ROADMAP).
    let world = world();
    let p = leg(&world.legs, "harrisburg_pa_us", "pittsburgh_pa_us")
        .pavement()
        .expect("HPMS reads the Turnpike");
    for forward in [true, false] {
        let tunnels: Vec<(f64, f64)> = p
            .structures_for(forward)
            .into_iter()
            .filter(|s| s.2 == StructureKind::Tunnel)
            .map(|s| (s.0, s.1))
            .collect();
        assert!(tunnels.len() >= 3, "{tunnels:?}");
        assert!(
            tunnels
                .iter()
                .any(|t| t.0 > 57.0 && t.1 < 61.0 && t.1 - t.0 > 0.9),
            "Tuscarora: {tunnels:?}"
        );
        assert_eq!(p.structure_at(59.0, forward), Some(StructureKind::Tunnel));
    }
}

#[test]
fn most_long_bridges_carry_a_deck_reading_and_none_contradicts_itself() {
    let world = world();
    let (mut long, mut decked, mut grates) = (0, 0, 0);
    for leg in &world.legs {
        let Some(p) = leg.pavement() else { continue };
        for (a, z, kind) in p.structures_for(true) {
            if kind == StructureKind::Tunnel || z - a < 0.05 {
                continue;
            }
            long += 1;
            if p.deck_at((a + z) / 2.0, true).is_some() {
                decked += 1;
            }
        }
        for s in &p.structures {
            assert!(
                s.kind != StructureKind::Tunnel || s.deck.is_none(),
                "{} to {}",
                leg.a,
                leg.b
            );
            if s.deck == Some(Deck::OpenGrating) {
                grates += 1;
            }
        }
    }
    // 80.7 percent on the 2026-11-22 write.
    assert!(decked * 4 >= long * 3, "{decked} of {long} long bridges");
    assert!((5..=40).contains(&grates), "{grates} open grating records");
}

#[test]
fn the_main_street_bridge_grating_is_its_lift_span() {
    let world = world();
    let p = leg(&world.legs, "jacksonville_fl_us", "miami_fl_us")
        .pavement()
        .expect("HPMS reads US-1 in Jacksonville");
    let s = p
        .structures
        .iter()
        .find(|s| s.nbi.as_deref() == Some("12-720022"))
        .expect("the Main Street Bridge, NBI 12-720022");
    let (a, z) = s.grate.expect("a lift bridge has a movable span");
    assert!(s.from_mi < a && z < s.to_mi, "{a} {z} inside {s:?}");
    let mid = (a + z) / 2.0;
    for forward in [true, false] {
        assert!(p.on_grating(mid, forward));
        assert!(!p.on_grating(s.from_mi + 0.01, forward), "the fixed spans");
        assert_eq!(p.cross_grooved_at(mid, forward, "Florida"), None);
    }
}

#[test]
fn some_concrete_reads_grooved_by_each_route_and_california_pavement_never() {
    let world = world();
    let (mut derived, mut assumed, mut california) = (0, 0, 0);
    for leg in &world.legs {
        let Some(p) = leg.pavement() else { continue };
        for r in &p.surface {
            let mid = (r.from_mi + r.to_mi) / 2.0;
            if p.structure_at(mid, true).is_some() || !r.ways.holds(true) {
                continue;
            }
            if p.cross_grooved_at(mid, true, "California").is_some() {
                california += 1;
            }
            if p.cross_grooved_at(mid, true, "Ohio") == Some(Grooves::DerivedFromBuildYear) {
                derived += 1;
            }
        }
        for (a, z, _) in p.structures_for(true) {
            if p.cross_grooved_at((a + z) / 2.0, true, "Ohio") == Some(Grooves::AssumedDeck) {
                assumed += 1;
            }
        }
    }
    assert_eq!(california, 0);
    assert!(derived > 100, "{derived} runs grooved by build year");
    assert!(assumed > 1000, "{assumed} concrete decks");
}
