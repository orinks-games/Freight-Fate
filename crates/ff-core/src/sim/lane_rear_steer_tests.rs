//! Partial lane keeping easing the driver's steer for a set of doubles
//! (`LaneKeeping::rear_steer_limit`).

use super::*;

/// Loaded twin pups: 0.9 of the 0.30 g whip threshold over an
/// amplification of 1.7, the figures `TruckState::rear_trailer_steer_limit`
/// gives them.
const PUPS: RearSteerLimit = RearSteerLimit {
    steer_g: 0.9 * 0.30 / 1.7,
    gust_gain: 1.0,
};

/// What one frame of a run saw.
struct Frame {
    transient: f64,
    allowance: f64,
    wind_g: f64,
}

/// Drive `steps` of (steer, seconds) then `tail` seconds hands off, on a
/// straight road, at `mph`, 20 frames a second. Returns every frame, the
/// lanes crossed (left positive) and the time of the first crossing.
fn drive(
    lane: &mut LaneKeeping,
    steps: &[(f64, f64)],
    mph: f64,
    wind: f64,
    assist: &str,
    limit: Option<RearSteerLimit>,
) -> (Vec<Frame>, i64, Option<f64>) {
    let dt = 0.05;
    let mut frames = Vec::new();
    let mut crossed = 0;
    let mut first = None;
    let mut t = 0.0;
    let mut all = steps.to_vec();
    all.push((0.0, 6.0));
    for (steer, seconds) in all {
        for _ in 0..(seconds / dt).round() as usize {
            lane.steering = steer;
            lane.rear_steer_limit = limit;
            lane.update(
                dt,
                mph / MPH_PER_MPS,
                RoadConditions {
                    wind,
                    ..RoadConditions::default()
                },
                assist,
                false,
            );
            t += dt;
            if lane.crossed != 0 {
                crossed += lane.crossed;
                first.get_or_insert(t);
            }
            frames.push(Frame {
                transient: lane.transient_lateral_g(),
                allowance: limit.map_or(f64::MAX, |l| l.allowance(lane.wind_lateral_g)),
                wind_g: lane.wind_lateral_g,
            });
        }
    }
    (frames, crossed, first)
}

/// Holds of 0.5, 1.0, 1.5 and 2.5 s, left a second then right a second, and
/// 0.3 s pulses. Steer -1 is left.
fn step_lists() -> Vec<Vec<(f64, f64)>> {
    let mut lists: Vec<Vec<(f64, f64)>> = [0.5, 1.0, 1.5, 2.5]
        .iter()
        .map(|&hold| vec![(-1.0, hold)])
        .collect();
    lists.push(vec![(-1.0, 1.0), (1.0, 1.0)]);
    lists.push([(-1.0, 0.3), (0.0, 0.3)].repeat(6));
    lists
}

#[test]
fn an_eased_steer_never_passes_the_allowance() {
    for steps in step_lists() {
        let mut lane = LaneKeeping::new(Some(7));
        let (frames, crossed, _) = drive(&mut lane, &steps, 60.0, 0.0, "partial", Some(PUPS));
        for (i, f) in frames.iter().enumerate() {
            assert!(
                f.transient <= f.allowance + 1e-12,
                "{steps:?} frame {i}: transient {} over allowance {}",
                f.transient,
                f.allowance
            );
        }
        if steps == [(-1.0, 2.5)] {
            assert_eq!(crossed, 1, "a 2.5 s hold crosses one lane");
            assert_eq!(lane.lane, 1);
        }
    }
}

#[test]
fn in_a_strong_crosswind_the_eased_steer_still_leaves_the_rear_trailer_under_its_whip() {
    // An empty set takes the gust twice as hard. What the whip test reads,
    // steer plus gust amplified, stays under the 0.30 g threshold.
    let empty = RearSteerLimit {
        gust_gain: 2.0,
        ..PUPS
    };
    for seed in 0..20 {
        for steps in step_lists() {
            let mut lane = LaneKeeping::new(Some(seed));
            let (frames, _, _) = drive(&mut lane, &steps, 60.0, 1.0, "partial", Some(empty));
            let worst = frames
                .iter()
                .map(|f| (f.transient + f.wind_g * empty.gust_gain) * 1.7)
                .fold(0.0, f64::max);
            assert!(worst < 0.30, "seed {seed} {steps:?}: {worst}");
        }
    }
}

#[test]
fn with_no_limit_the_steer_is_what_it_always_was() {
    // A limit too wide ever to bind runs bit for bit with no limit at all,
    // so the clamp is the only change; a real one does change the drive.
    let wide = RearSteerLimit {
        steer_g: 10.0,
        gust_gain: 1.0,
    };
    for steps in step_lists() {
        let mut free = LaneKeeping::new(Some(3));
        let mut unbound = LaneKeeping::new(Some(3));
        let (a, _, _) = drive(&mut free, &steps, 60.0, 0.6, "partial", None);
        let (b, _, _) = drive(&mut unbound, &steps, 60.0, 0.6, "partial", Some(wide));
        assert_eq!(free.offset, unbound.offset, "{steps:?}");
        assert_eq!(free.yaw_rad, unbound.yaw_rad, "{steps:?}");
        assert!(a.iter().zip(&b).all(|(x, y)| x.transient == y.transient));
    }
    let peak = |steps: &[(f64, f64)], limit| {
        let mut lane = LaneKeeping::new(Some(3));
        drive(&mut lane, steps, 60.0, 0.0, "partial", limit)
            .0
            .iter()
            .map(|f| f.transient)
            .fold(0.0, f64::max)
    };
    // Today's snap back whips loaded pups (0.30 g over 1.7 is 0.176 g).
    assert!(peak(&[(-1.0, 2.5)], None) > 0.30 / 1.7);
    assert!(peak(&[(-1.0, 2.5)], Some(PUPS)) <= PUPS.steer_g + 1e-12);
}

#[test]
fn a_held_lane_change_under_the_easing_still_takes_under_four_seconds() {
    // From key down to the line committed, holding the key until then.
    for mph in [45.0, 60.0, 70.0] {
        let time_to_cross = |limit| {
            let mut lane = LaneKeeping::new(Some(7));
            drive(&mut lane, &[(-1.0, 6.0)], mph, 0.0, "partial", limit).2
        };
        let before = time_to_cross(None).expect("crosses today");
        let after = time_to_cross(Some(PUPS)).expect("crosses eased");
        assert!(after < 4.0, "{mph} mph: {after} s");
        assert!(
            after >= before,
            "{mph} mph: {after} s eased, {before} s free"
        );
    }
}
