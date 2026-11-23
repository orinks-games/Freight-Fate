//! The limit through the town at either end of a leg, where the map read
//! no posting for the streets.
//!
//! A leg runs from one city's centre to the next, so its first and last
//! miles are town streets. The speed-limit bake reads OpenStreetMap postings
//! along the route and keeps a step function; the streets in town are often
//! untagged, so a profile commonly starts at the first posting on the
//! highway out of town (65 at mile 3.3 out of Marshalltown) and ends with
//! the last highway posting carried on into the next town. Read as a step
//! function, both put the open-road number on the town streets: the driver
//! heard "Speed limit 65" in Marshalltown and the speed keeper took the
//! truck to 42 mph a quarter mile short of two 20 mph corners.
//!
//! A silent upstream is not a reading. Where the map read nothing, the law
//! still gives a number: each state's code sets a default limit for a
//! business, residence or urban district that governs where no sign is
//! posted ([`crate::data::street_limits`], with citations). That is the
//! number used here, ASSUMED in the provenance sense, because nothing says
//! which district the street is in, only that the leg is in town.
//!
//! ## Where town ends
//!
//! From the route's own bends, read at load and never baked in:
//!
//! - **Leaving town** (offset 0 up to the first posting): the town stretch
//!   runs to the end of the last town bend that starts before the first
//!   posting and within [`URBAN_RADIUS_MI`]. Past it, still short of the
//!   first posting, the stretch is unposted and the ordinary unposted rule
//!   answers (the highway figure, the urban 55 near a city), never above the
//!   first posting itself.
//! - **Coming into town** (the far end): the last posting began on the
//!   highway; where it is a highway-speed number (at least
//!   [`GAP_HOLD_HIGHWAY_MPH`]) or a coverage gap, the town stretch starts at
//!   the first town bend past it within the radius and runs to the city. A
//!   town posting read on the way in (35, 45) is left alone.
//!
//! A town bend is a connector arc (the route on a ramp or on a road less
//! important than the leg's highway: the town it threads on the way in or
//! out, see [`crate::data::curves::classify_connector`]) or any bend advised
//! at [`TOWN_BEND_MAX_ADVISORY_MPH`] or less.
//!
//! How well that separates, measured 2026-11-23 on the dev tree over the
//! legs posted from mile 0 (where the map does say what the town is), in
//! 0.05-mile steps over each leg's first 6 miles: inside the stretch the
//! rule calls town, 73% of the road is posted 45 or less and 23% 55 or more;
//! past it, 64% is posted 55 or more. On legs that start in town, the rule
//! ends the stretch a mean 1.4 miles early (those miles get the unposted
//! rule, at most 55 near a city) and holds a mean 0.6 miles of open road at
//! the town figure. Of the bends advised 25 or less within 6 miles of a leg
//! end, 79% sit under a posting of 45 or less. Every such bend before a
//! leg's first posting is inside the stretch by construction, which is the
//! case that rolled trucks.

use crate::data::curves::CurveRecord;
use crate::data::street_limits::{load_street_limits, FALLBACK_MPH};
use crate::data::world_models::Leg;
use crate::sim::limit_gaps::GAP_HOLD_HIGHWAY_MPH;
use crate::sim::trip_models::{leg_state_at, URBAN_RADIUS_MI};

/// A bend advised at this or less is a street corner, not a highway curve.
/// It is the threshold the defect was measured with; of the mainline bends
/// this tight within 6 miles of a leg end on fully posted legs, three in
/// four are under a town posting.
pub const TOWN_BEND_MAX_ADVISORY_MPH: i64 = 25;

/// One end of a leg treated as town, in leg offsets (A-to-B miles).
#[derive(Debug, Clone, PartialEq)]
pub struct TownStretch {
    pub from_mi: f64,
    pub to_mi: f64,
    /// The district limit for the state at that end, or the fallback.
    pub mph: f64,
    /// Where the number comes from, always marked assumed.
    pub source: String,
}

impl TownStretch {
    fn contains(&self, offset_mi: f64) -> bool {
        self.from_mi <= offset_mi && offset_mi <= self.to_mi
    }
}

/// The town stretches of one leg, at its A end and its B end.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TownEnds {
    pub start: Option<TownStretch>,
    pub end: Option<TownStretch>,
}

impl TownEnds {
    /// The town stretch covering a leg offset, if any.
    pub fn stretch_at(&self, offset_mi: f64) -> Option<&TownStretch> {
        [self.start.as_ref(), self.end.as_ref()]
            .into_iter()
            .flatten()
            .filter(|s| s.contains(offset_mi))
            .min_by(|a, b| a.mph.total_cmp(&b.mph))
    }

    /// `limit` held to the town figure where the offset is in town.
    pub fn cap(&self, offset_mi: f64, limit: f64) -> f64 {
        match self.stretch_at(offset_mi) {
            Some(stretch) => limit.min(stretch.mph),
            None => limit,
        }
    }
}

fn is_town_bend(curve: &CurveRecord) -> bool {
    curve.connector || curve.advisory_mph <= TOWN_BEND_MAX_ADVISORY_MPH
}

/// The district limit for a state, and its source line.
pub fn town_mph(state: &str) -> (f64, String) {
    let table = load_street_limits();
    match (table.statutory_mph(state), table.get(state)) {
        (Some(mph), Some(row)) => (
            mph,
            format!(
                "assumed: no posting read on the town streets; {state}'s district limit, {}",
                row.citation
            ),
        ),
        _ => (
            FALLBACK_MPH,
            format!(
                "assumed: no posting read on the town streets and no district limit on file \
                 for {}; the street fallback",
                if state.is_empty() {
                    "this state"
                } else {
                    state
                }
            ),
        ),
    }
}

/// Where the town ends at each end of a leg, from its speed profile and its
/// bends (`curves` in bake direction, connectors included).
pub fn town_ends(leg: &Leg, curves: &[CurveRecord]) -> TownEnds {
    let samples = leg.speed_limits();
    let (Some(first), Some(last)) = (samples.first(), samples.last()) else {
        // No profile at all: the unposted rule answers everywhere, and its
        // urban figure already covers the town.
        return TownEnds::default();
    };
    let start = (first.at_mi > 0.0)
        .then(|| {
            let reach = first.at_mi.min(URBAN_RADIUS_MI);
            curves
                .iter()
                .filter(|c| is_town_bend(c) && c.start_mi < reach)
                .map(|c| c.end_mi.max(c.start_mi))
                .fold(None, |end: Option<f64>, e| {
                    Some(end.map_or(e, |m| m.max(e)))
                })
                .map(|end| end.min(first.at_mi))
        })
        .flatten()
        .filter(|to| *to > 0.0)
        .map(|to_mi| {
            let (mph, source) = town_mph(&leg_state_at(leg, 0.0));
            TownStretch {
                from_mi: 0.0,
                to_mi,
                mph,
                source,
            }
        });
    let highway_tail = last.mph.is_none_or(|mph| mph >= GAP_HOLD_HIGHWAY_MPH);
    let end = highway_tail
        .then(|| {
            let after = last.at_mi.max(leg.miles - URBAN_RADIUS_MI);
            curves
                .iter()
                .filter(|c| is_town_bend(c) && c.start_mi.min(c.end_mi) >= after)
                .map(|c| c.start_mi.min(c.end_mi))
                .fold(None, |s: Option<f64>, x| Some(s.map_or(x, |m| m.min(x))))
        })
        .flatten()
        .filter(|from| *from < leg.miles)
        .map(|from_mi| {
            let (mph, source) = town_mph(&leg_state_at(leg, leg.miles));
            TownStretch {
                from_mi,
                to_mi: leg.miles,
                mph,
                source,
            }
        });
    TownEnds { start, end }
}
