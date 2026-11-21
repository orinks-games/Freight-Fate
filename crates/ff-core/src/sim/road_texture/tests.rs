use super::*;
use crate::data::pavement::Surface;

fn rng() -> PyRandom {
    PyRandom::new_from_i64(7)
}

fn read(surface: Surface, faulting: Option<f64>) -> Option<Reading> {
    Some(Reading { surface, faulting })
}

#[test]
fn test_the_surface_transition_table() {
    use Surface::*;
    let mut r = rng();
    let travel = |metres, reading| RoadSurfaceEvent::Travel { metres, reading };
    let jpc = read(JointedPlainConcrete, Some(0.02));
    let invented_j = RoadSurface::Invented(Invented::Jointed { left_m: 100.0 });
    let invented_s = RoadSurface::Invented(Invented::Smooth { left_m: 100.0 });
    // (from, event, kind after, origin after)
    let rows: [(RoadSurface, RoadSurfaceEvent, &str, &str); 9] = [
        // A reading wins over invented road of either kind.
        (
            invented_s,
            travel(10.0, jpc),
            "jointed",
            "read jointed_plain_concrete",
        ),
        (
            invented_j,
            travel(10.0, read(Asphalt, None)),
            "smooth",
            "read asphalt",
        ),
        // Read road follows the reading.
        (
            RoadSurface::Read(jpc.unwrap()),
            travel(10.0, read(ContinuousConcrete, None)),
            "smooth",
            "read continuous_concrete",
        ),
        (
            RoadSurface::Read(jpc.unwrap()),
            travel(10.0, read(AsphaltOverJointedConcrete, None)),
            "jointed",
            "read asphalt_over_jointed_concrete",
        ),
        // Leaving read road starts invented road.
        (
            RoadSurface::Read(jpc.unwrap()),
            travel(10.0, None),
            "",
            "invented",
        ),
        // Invented road keeps its stretch, then alternates.
        (invented_j, travel(40.0, None), "jointed", "invented"),
        (invented_s, travel(100.0, None), "jointed", "invented"),
        (invented_j, travel(150.0, None), "smooth", "invented"),
        // A hold wins over the layer.
        (RoadSurface::held(false), travel(1e9, jpc), "smooth", "held"),
    ];
    for (from, event, kind, origin) in rows {
        let to = from.next(event, &mut r);
        if !kind.is_empty() {
            assert_eq!(to.name(), kind, "{from:?} on {event:?}");
        }
        assert_eq!(to.origin(), origin, "{from:?} on {event:?}");
    }
    // The invented stretch spends the overflow on the next one.
    let RoadSurface::Invented(to) = invented_s.next(travel(130.0, None), &mut r) else {
        panic!("still invented");
    };
    assert!((JOINTED_M.0 - 30.0..=JOINTED_M.1 - 30.0).contains(&to.left_m()));
    let RoadSurface::Invented(kept) = invented_j.next(travel(40.0, None), &mut r) else {
        panic!("still invented");
    };
    assert_eq!(kept.left_m(), 60.0);
}

#[test]
fn test_which_surfaces_have_seams_and_how_hard() {
    use Surface::*;
    let factor = |surface, faulting| RoadSurface::Read(Reading { surface, faulting }).seam_factor();
    for s in [
        JointedPlainConcrete,
        JointedReinforcedConcrete,
        UnbondedJointedConcreteOverlay,
        BondedConcreteOverlay,
    ] {
        assert_eq!(factor(s, None), Some(1.0), "{s:?}");
    }
    assert_eq!(
        factor(AsphaltOverJointedConcrete, Some(0.3)),
        Some(OVERLAY_SEAM_FACTOR)
    );
    for s in [
        Asphalt,
        AsphaltOverAsphalt,
        AsphaltOverContinuousConcrete,
        ContinuousConcrete,
        Unpaved,
        Other,
    ] {
        assert_eq!(factor(s, Some(0.3)), None, "{s:?}");
    }
    // The faulting classes of 23 CFR 490.313.
    let rows = [
        (None, 1.0),
        (Some(0.0), 1.0),
        (Some(0.099), 1.0),
        (Some(0.10), FAIR_SEAM_FACTOR),
        (Some(0.15), FAIR_SEAM_FACTOR),
        (Some(0.151), POOR_SEAM_FACTOR),
        (Some(0.8), POOR_SEAM_FACTOR),
    ];
    for (faulting, want) in rows {
        assert_eq!(
            factor(JointedPlainConcrete, faulting),
            Some(want),
            "{faulting:?}"
        );
    }
    assert_eq!(RoadSurface::held(true).seam_factor(), Some(1.0));
    assert_eq!(RoadSurface::held(false).seam_factor(), None);
    const { assert!(OVERLAY_SEAM_FACTOR < 1.0 && 1.0 < FAIR_SEAM_FACTOR) };
    const { assert!(FAIR_SEAM_FACTOR < POOR_SEAM_FACTOR) };
}

#[test]
fn test_a_long_invented_roll_alternates_and_a_seed_replays_it() {
    let walk = |seed| {
        let mut r = PyRandom::new_from_i64(seed);
        let mut s = RoadSurface::first(None, &mut r);
        let mut kinds = vec![s.name()];
        for _ in 0..2_000 {
            s = s.next(
                RoadSurfaceEvent::Travel {
                    metres: 25.0,
                    reading: None,
                },
                &mut r,
            );
            if kinds.last() != Some(&s.name()) {
                kinds.push(s.name());
            }
        }
        kinds
    };
    let a = walk(3);
    assert!(a.len() > 10, "50 km has many stretches: {a:?}");
    assert!(a.windows(2).all(|w| w[0] != w[1]));
    assert_eq!(a, walk(3));
}

#[test]
fn test_a_seam_is_two_hits_and_a_bridge_joint_three() {
    let mut hits = AxleHits::default();
    let steer = hits.cross(Crossing::Seam, 1.5);
    assert_eq!(steer.axle, Axle::Steer);
    assert!(hits.travel(AXLE_GAP_M - 0.1).is_empty());
    let drive = hits.travel(0.2);
    assert_eq!(drive.len(), 1);
    assert_eq!(drive[0].axle, Axle::Drive);
    assert_eq!(
        drive[0].factor, 1.5,
        "the drives hit as hard as the steer axle"
    );
    assert!(hits.is_empty());

    let mut hits = AxleHits::default();
    assert_eq!(hits.cross(Crossing::BridgeJoint, 1.0).axle, Axle::Steer);
    let behind: Vec<Axle> = (0..100)
        .flat_map(|_| hits.travel(0.5))
        .map(|h| h.axle)
        .collect();
    assert_eq!(behind, [Axle::Drive, Axle::Trailer]);
    // One long frame lands both, in order.
    let mut hits = AxleHits::default();
    hits.cross(Crossing::BridgeJoint, 1.0);
    let both: Vec<Axle> = hits.travel(50.0).into_iter().map(|h| h.axle).collect();
    assert_eq!(both, [Axle::Drive, Axle::Trailer]);
}

#[test]
fn test_the_axle_gap_stretches_as_the_truck_slows() {
    let at_65 = axle_gap_s(29.0);
    let at_35 = axle_gap_s(15.6);
    assert!((at_65 - 0.179).abs() < 0.001, "{at_65}");
    assert!((at_35 - 0.333).abs() < 0.001, "{at_35}");
}

#[test]
fn test_slab_spaced_seams_interleave_and_every_hit_lands_in_order() {
    // At the slab spacing the drives of one seam land after the steer axle
    // has reached the next: steer, steer, drive, steer, drive...
    let spacing = (SEAM_SPACING_M.0 + SEAM_SPACING_M.1) / 2.0;
    const { assert!(SEAM_SPACING_M.1 < AXLE_GAP_M) };
    for speed in [15.6, 29.0] {
        let step = speed / 240.0;
        let mut hits = AxleHits::default();
        let mut at = 0.0;
        let mut next_seam = 0.0;
        let mut steer_at: Vec<f64> = Vec::new();
        let mut drive_at: Vec<f64> = Vec::new();
        let mut order: Vec<Axle> = Vec::new();
        while at < 60.0 {
            for h in hits.travel(step) {
                order.push(h.axle);
                drive_at.push(at);
            }
            at += step;
            if at >= next_seam && next_seam < 40.0 {
                order.push(hits.cross(Crossing::Seam, 1.0).axle);
                steer_at.push(at);
                next_seam += spacing;
            }
        }
        assert_eq!(steer_at.len(), drive_at.len(), "every seam's drives land");
        assert_eq!(&order[..3], [Axle::Steer, Axle::Steer, Axle::Drive]);
        for (s, d) in steer_at.iter().zip(&drive_at) {
            assert!(
                (d - s - AXLE_GAP_M).abs() <= step * 1.01,
                "{speed}: {s} -> {d}"
            );
        }
    }
}
