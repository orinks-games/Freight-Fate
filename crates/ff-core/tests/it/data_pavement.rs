//! The HPMS pavement layer: surface runs and structures along each leg.

use std::sync::Arc;

use crate::data_support::world;
use ff_core::data::pavement::{StructureKind, SurfaceGroup};
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
    // HPMS covers only US roads, so a leg touching Canada has no layer.
    let us_legs = world
        .legs
        .iter()
        .filter(|l| !l.a.ends_with("_ca") && !l.b.ends_with("_ca"))
        .count();
    for leg in &world.legs {
        let Some(p) = leg.pavement() else { continue };
        with += 1;
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
