//! What the limit is where the map has no posting: a coverage gap between
//! two baked postings, or a leg with none at all.
//!
//! The highway/region heuristic answers there, but on its own it jumps the
//! limit up to the open-road number between two lower postings: the Tri-State
//! in Lake County read 60, then 70 for six untagged miles, then 55. Two rules
//! keep it honest.
//!
//! - **A gap between two postings takes at most the higher of them**, when
//!   both are highway-speed postings, or when both are at least 45 and the
//!   gap is short. Calibrated against the baked profile itself (2026-10-19
//!   planet data): hiding a tagged stretch between two postings of 55 or
//!   more, the road inside was posted above the higher of them for most of
//!   its length 0.1% of the time over 5 miles, 0.4% over 10, 1.9% over 40.
//!   With a 45 or 50 at one end it was 2.6% over 5 miles and 5.1% over 10,
//!   rising to 13.4% over 40, so that case stops at 10 miles. With a town
//!   limit under 45 at either end (the NY-12 lesson: a village 30 whose
//!   tagging ends) the heuristic answers unchanged, since that is how a road
//!   leaves town.
//! - **The state's truck cap binds the heuristic too.** A California rig is
//!   held to 55 on every road, posted or not.

use crate::data::world_models::{Leg, SpeedLimitSample};
use crate::sim::trip_models::{leg_state_at, statutory_truck_caps};

/// Both postings around a gap at or above this: the gap is highway between
/// them, whatever its length.
pub const GAP_HOLD_HIGHWAY_MPH: f64 = 55.0;
/// Both postings at or above this, and the gap no longer than
/// [`GAP_HOLD_SHORT_MAX_MI`]: the gap is the same road between them.
pub const GAP_HOLD_SHORT_MIN_MPH: f64 = 45.0;
pub const GAP_HOLD_SHORT_MAX_MI: f64 = 10.0;

/// The index of the posting in force at a leg offset (the last at or before
/// it, the first before any).
fn posting_index(samples: &[SpeedLimitSample], offset_mi: f64) -> Option<usize> {
    if samples.is_empty() {
        return None;
    }
    Some(
        samples
            .iter()
            .rposition(|s| s.at_mi <= offset_mi)
            .unwrap_or(0),
    )
}

/// The highest limit a coverage gap may take at a leg offset, or `None`
/// where the offset is not inside a gap held by the postings around it.
pub fn gap_ceiling_mph(leg: &Leg, offset_mi: f64) -> Option<f64> {
    let samples = leg.speed_limits();
    let i = posting_index(samples, offset_mi)?;
    if samples[i].mph.is_some() || i == 0 {
        return None;
    }
    let before = samples[i - 1].mph?;
    let after = samples.get(i + 1)?;
    let after_mph = after.mph?;
    let low = before.min(after_mph);
    let length = after.at_mi - samples[i].at_mi;
    let held = low >= GAP_HOLD_HIGHWAY_MPH
        || (low >= GAP_HOLD_SHORT_MIN_MPH && length <= GAP_HOLD_SHORT_MAX_MI);
    held.then(|| before.max(after_mph))
}

/// The heuristic limit where a leg has no posting, with the gap ceiling
/// applied. `heuristic` is the highway/region number for the spot.
pub fn unposted_limit(leg: &Leg, offset_mi: f64, heuristic: f64) -> f64 {
    match gap_ceiling_mph(leg, offset_mi) {
        Some(ceiling) => heuristic.min(ceiling),
        None => heuristic,
    }
}

/// The statutory truck cap for this road class in the state at a leg
/// offset, or `None` where the state has no split.
pub fn unposted_truck_cap(leg: &Leg, offset_mi: f64) -> Option<(f64, String)> {
    let state = leg_state_at(leg, offset_mi);
    let (cap, _) = statutory_truck_caps(&state, &leg.highway)?;
    Some((cap, state))
}

/// Whether a leg offset has no posting in force: a coverage gap, or a leg
/// with no profile at all.
pub fn is_unposted(leg: &Leg, offset_mi: f64) -> bool {
    let samples = leg.speed_limits();
    posting_index(samples, offset_mi).is_none_or(|i| samples[i].mph.is_none())
}
