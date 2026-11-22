//! Bridge joints: where along the route the truck crosses an expansion
//! joint, and which of them it hears.
//!
//! Joints sit at the ends of the pavement layer's bridges and causeways
//! (FHWA HPMS Structure_Type, read) wherever the leg has the layer. A river
//! point with no structure near it, and every river on a leg without the
//! layer, keeps a joint at each edge of its river span, which is assumed.
//!
//! Structures sit on route miles, and route miles pass 10 or 20 times faster
//! than the wheels turn, so a joint at both ends of every structure would be
//! a barrage. Two rules thin them, both for pacing and both assumed for the
//! ear: [`MIN_SPAN_MI`] and [`MIN_JOINT_GAP_S`].

use crate::data::pavement::StructureKind;
use crate::sim::outside::RIVER_SPAN_MI;

/// A bridge or causeway shorter than this has no joint.
///
/// A thinning rule for pacing and for culverts, chosen from the span
/// distribution of the 81,066 structures (2026-11-20): 38,389 are under
/// 0.02 mile and 28,977 more under 0.05, nearly all culverts, which HPMS
/// counts as bridges, and single overpasses. It leaves about 13,700. It is
/// not a claim that shorter bridges have no joints.
pub const MIN_SPAN_MI: f64 = 0.05;
/// Real seconds that must pass after a joint before the next one is heard.
///
/// Assumed for the ear. It also decides whether a bridge sounds at both
/// ends: at 65 mph a route mile lasts about 2.8 real seconds at standard
/// pacing and 5.5 at relaxed, so a bridge is heard at both ends from about
/// 0.55 mile at standard and 0.27 at relaxed, and a shorter one only at its
/// near end. It is judged on the real time that passed, so a slower pacing
/// or a slower truck hears more.
pub const MIN_JOINT_GAP_S: f64 = 1.5;

/// Where a jointed span comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpanOrigin {
    /// A bridge or causeway the layer reads.
    Structure(StructureKind),
    /// A river point with no structure the layer reads near it: a joint at
    /// each edge of the river span, assumed.
    RiverAssumed,
    /// A named bridge (`corridor.bridges`) where the layer reads no
    /// structure: its whole derived span, a joint at each end.
    NamedBridge,
}

/// A stretch with a joint at each end, in route miles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JointSpan {
    pub from_mi: f64,
    pub to_mi: f64,
    pub origin: SpanOrigin,
}

impl JointSpan {
    /// The assumed span around a river point.
    pub fn river(at_mi: f64) -> JointSpan {
        JointSpan {
            from_mi: at_mi - RIVER_SPAN_MI,
            to_mi: at_mi + RIVER_SPAN_MI,
            origin: SpanOrigin::RiverAssumed,
        }
    }

    /// A named bridge's whole span.
    pub fn named_bridge(from_mi: f64, to_mi: f64) -> JointSpan {
        JointSpan {
            from_mi,
            to_mi,
            origin: SpanOrigin::NamedBridge,
        }
    }

    /// Whether a structure of `kind` running `from_mi` to `to_mi` gets
    /// joints: bridges and causeways from [`MIN_SPAN_MI`]. A tunnel has none.
    pub fn from_structure(from_mi: f64, to_mi: f64, kind: StructureKind) -> Option<JointSpan> {
        let jointed = matches!(kind, StructureKind::Bridge | StructureKind::Causeway);
        (jointed && to_mi - from_mi >= MIN_SPAN_MI).then_some(JointSpan {
            from_mi,
            to_mi,
            origin: SpanOrigin::Structure(kind),
        })
    }
}

/// One joint the truck reaches, at a span's end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Joint {
    pub at_mi: f64,
    pub origin: SpanOrigin,
}

/// The joints the truck hears rolling from `from_mi` to `to_mi` this frame,
/// in the order it reaches them, and the real seconds since the last joint
/// it heard afterwards.
///
/// `spans` are in route miles; `since_last_s` already counts this frame.
/// A joint inside [`MIN_JOINT_GAP_S`] of the last one heard is dropped,
/// except at an assumed river span, which keeps the two joints it always
/// had. Rolling backwards crosses nothing.
pub fn joints_heard(
    spans: &[JointSpan],
    from_mi: f64,
    to_mi: f64,
    since_last_s: f64,
) -> (Vec<Joint>, f64) {
    let mut since = since_last_s;
    if to_mi <= from_mi {
        return (Vec::new(), since);
    }
    let mut reached: Vec<Joint> = spans
        .iter()
        .flat_map(|s| {
            [s.from_mi, s.to_mi].map(|at_mi| Joint {
                at_mi,
                origin: s.origin,
            })
        })
        .filter(|j| from_mi < j.at_mi && j.at_mi <= to_mi)
        .collect();
    reached.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
    let mut heard = Vec::new();
    for joint in reached {
        if since >= MIN_JOINT_GAP_S
            || matches!(joint.origin, SpanOrigin::RiverAssumed | SpanOrigin::NamedBridge)
        {
            heard.push(joint);
            since = 0.0;
        }
    }
    (heard, since)
}

/// The span under `at_mi`, if any.
pub fn span_at(spans: &[JointSpan], at_mi: f64) -> Option<&JointSpan> {
    spans
        .iter()
        .find(|s| s.from_mi <= at_mi && at_mi <= s.to_mi)
}

#[cfg(test)]
mod tests;
