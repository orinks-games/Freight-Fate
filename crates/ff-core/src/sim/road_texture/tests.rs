use super::*;

fn rng() -> PyRandom {
    PyRandom::new_from_i64(7)
}

#[test]
fn test_the_surface_transition_table() {
    let mut r = rng();
    let travel = |metres| RoadSurfaceEvent::Travel { metres };
    // (from, travel, expected kind after, expected left when it is the same stretch)
    let cases: [(RoadSurface, f64, &str, Option<f64>); 6] = [
        (
            RoadSurface::Smooth { left_m: 100.0 },
            40.0,
            "smooth",
            Some(60.0),
        ),
        (
            RoadSurface::Jointed { left_m: 100.0 },
            40.0,
            "jointed",
            Some(60.0),
        ),
        (
            RoadSurface::Smooth { left_m: 100.0 },
            100.0,
            "jointed",
            None,
        ),
        (
            RoadSurface::Jointed { left_m: 100.0 },
            150.0,
            "smooth",
            None,
        ),
        (
            RoadSurface::Jointed { left_m: 100.0 },
            0.0,
            "jointed",
            Some(100.0),
        ),
        (RoadSurface::held(true), 1e9, "jointed", Some(f64::INFINITY)),
    ];
    for (from, metres, kind, left) in cases {
        let to = from.next(travel(metres), &mut r);
        assert_eq!(to.name(), kind, "{from:?} + {metres} m");
        match left {
            Some(left) => assert_eq!(to.left_m(), left, "{from:?} + {metres} m"),
            None => {
                let (lo, hi) = if to.is_jointed() { JOINTED_M } else { SMOOTH_M };
                // The overflow past the old stretch is spent on the new one.
                let spent = metres - from.left_m();
                assert!(
                    (lo - spent..=hi - spent).contains(&to.left_m()),
                    "{from:?} + {metres} m -> {to:?}"
                );
            }
        }
    }
}

#[test]
fn test_a_long_roll_alternates_and_a_seed_replays_it() {
    let walk = |seed| {
        let mut r = PyRandom::new_from_i64(seed);
        let mut s = RoadSurface::first(&mut r);
        let mut kinds = vec![s.name()];
        for _ in 0..2_000 {
            s = s.next(RoadSurfaceEvent::Travel { metres: 25.0 }, &mut r);
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
    let steer = hits.cross(Crossing::Seam);
    assert_eq!(steer.axle, Axle::Steer);
    assert!(hits.travel(AXLE_GAP_M - 0.1).is_empty());
    let drive = hits.travel(0.2);
    assert_eq!(drive.len(), 1);
    assert_eq!(drive[0].axle, Axle::Drive);
    assert!(hits.is_empty());

    let mut hits = AxleHits::default();
    assert_eq!(hits.cross(Crossing::BridgeJoint).axle, Axle::Steer);
    let behind: Vec<Axle> = (0..100)
        .flat_map(|_| hits.travel(0.5))
        .map(|h| h.axle)
        .collect();
    assert_eq!(behind, [Axle::Drive, Axle::Trailer]);
    // One long frame lands both, in order.
    let mut hits = AxleHits::default();
    hits.cross(Crossing::BridgeJoint);
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
