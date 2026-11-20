use super::*;

fn bridge(from_mi: f64, to_mi: f64) -> JointSpan {
    JointSpan::from_structure(from_mi, to_mi, StructureKind::Bridge).expect("long enough")
}

#[test]
fn test_which_structures_get_joints() {
    use StructureKind::*;
    let rows = [
        (Bridge, 0.06, true),
        (Bridge, MIN_SPAN_MI, true),
        (Bridge, 0.049, false),
        (Bridge, 0.01, false),
        (Causeway, 2.0, true),
        (Tunnel, 1.0, false),
    ];
    for (kind, span, jointed) in rows {
        assert_eq!(
            JointSpan::from_structure(10.0, 10.0 + span, kind).is_some(),
            jointed,
            "{kind:?} {span} mi"
        );
    }
    let r = JointSpan::river(5.0);
    assert_eq!(
        (r.from_mi, r.to_mi),
        (5.0 - RIVER_SPAN_MI, 5.0 + RIVER_SPAN_MI)
    );
}

/// Roll across `spans` at `mph` with route miles `scale` times faster than
/// the wheels, in 1/60 s frames; the route miles of every joint heard.
fn drive(spans: &[JointSpan], mph: f64, scale: f64, from: f64, to: f64) -> Vec<f64> {
    let dt = 1.0 / 60.0;
    let step = mph / 3600.0 * scale * dt;
    let mut at = from;
    let mut since = f64::INFINITY;
    let mut heard = Vec::new();
    while at < to {
        let (joints, after) = joints_heard(spans, at, at + step, since + dt);
        heard.extend(joints.iter().map(|j| j.at_mi));
        since = after;
        at += step;
    }
    heard
}

#[test]
fn test_the_thinning_transition_table() {
    // (spans, mph, time scale, joints heard)
    let long = [bridge(10.0, 11.0)];
    let short = [bridge(10.0, 10.06)];
    let rows: [(&[JointSpan], f64, f64, usize); 7] = [
        // A mile-long bridge is heard at both ends at either pacing.
        (&long, 65.0, 20.0, 2),
        (&long, 65.0, 10.0, 2),
        // A 0.06-mile bridge only at its near end.
        (&short, 65.0, 20.0, 1),
        (&short, 65.0, 10.0, 1),
        // ...but at a crawl the far end is long enough after the near one.
        (&short, 10.0, 10.0, 2),
        // Two bridges in a row: the second's near end lands on the first's
        // far end and is dropped; its far end is heard.
        (&[bridge(10.0, 10.6), bridge(10.62, 11.6)], 65.0, 20.0, 3),
        // An assumed river span keeps both joints, as it always had.
        (&[JointSpan::river(10.0)], 65.0, 20.0, 2),
    ];
    for (spans, mph, scale, want) in rows {
        let heard = drive(spans, mph, scale, 9.0, 12.0);
        assert_eq!(
            heard.len(),
            want,
            "{spans:?} at {mph} mph x{scale}: {heard:?}"
        );
    }
}

#[test]
fn test_joints_come_in_route_order_and_never_backwards() {
    let spans = [bridge(10.0, 11.0), bridge(12.0, 13.0)];
    let (j, since) = joints_heard(&spans, 9.0, 14.0, 10.0);
    // One long frame reaches all four ends; only the first is heard.
    assert_eq!(j.iter().map(|j| j.at_mi).collect::<Vec<_>>(), [10.0]);
    assert_eq!(since, 0.0);
    let (j, since) = joints_heard(&spans, 11.5, 10.5, 10.0);
    assert!(j.is_empty());
    assert_eq!(since, 10.0);
    // An end exactly where the frame starts was heard last frame.
    let (j, _) = joints_heard(&spans, 10.0, 10.5, 10.0);
    assert!(j.is_empty());
    assert_eq!(span_at(&spans, 10.5), Some(&spans[0]));
    assert_eq!(span_at(&spans, 11.5), None);
}
