//! The co-driver never crosses a border (owner ruling, 2026-11-28). Short
//! of the line into a customs booth's country they bring the truck to a
//! stop and hand the wheel back, so the player crosses the line and clears
//! customs exactly as on a solo crossing: the crossing call, the units and
//! hours rules lines, the booth's notice, the stop and the clearance.
//!
//! The same lines bound a slept shift: the jump ends short of the handback
//! point, and short of any line where the hours rules change (60 north has
//! no booth), so the time jumped is always counted under one rule set.

use ff_core::sim::hos::HosRules;
use ff_core::sim::trip_models::BorderBooth;

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

/// How far short of a customs booth the co-driver has the truck stopped:
/// past the booth's notice at a standing start (2 miles), so the player
/// hears it.
pub(crate) const CUSTOMS_HANDBACK_LEAD_MI: f64 = WEIGH_STATION_NOTICE_MI + 1.0;
/// How far short of the line into the booth's country the truck stops, so
/// the crossing call is the player's.
const CUSTOMS_LINE_LEAD_MI: f64 = 0.25;
/// The co-driver is on the brake from this far short of the handback point:
/// room to stop from highway speed on the service brake.
pub(crate) const CUSTOMS_BRAKE_ROOM_MI: f64 = 0.5;
/// How far ahead the live drive looks for a booth: beyond the widest gap
/// between a line and its booth on the map (the Alaska line to Beaver
/// Creek, about 20 miles).
const CUSTOMS_SEARCH_MI: f64 = 60.0;

/// The country a rule set belongs to.
fn rules_country(rules: HosRules) -> &'static str {
    if rules.is_canadian() {
        "CA"
    } else {
        "US"
    }
}

/// A line ahead on the trip where the state or province changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RulesLine {
    pub at_mi: f64,
    pub from: HosRules,
    pub into: HosRules,
}

impl DrivingState {
    /// The lines ahead of the truck where the hours rules change, nearest
    /// first, judged from the rules of the place the truck is in (what the
    /// frame sets both logbooks to).
    pub(crate) fn rules_lines_ahead(&self, ctx: &GameContext) -> Vec<RulesLine> {
        let here = self.trip.position_mi;
        let mut cues: Vec<(f64, &str)> = self
            .trip
            .navigation_cues
            .iter()
            .filter(|cue| cue.kind == "state_crossing" && cue.at_mi > here)
            .filter_map(|cue| {
                // "state:{leg}:{mile}:{into_state}"
                let into = cue.key.splitn(4, ':').nth(3)?;
                Some((cue.at_mi, into))
            })
            .collect();
        cues.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut rules = HosRules::for_jurisdiction(&self.current_jurisdiction(ctx));
        let mut lines = Vec::new();
        for (at_mi, into) in cues {
            let next = HosRules::for_jurisdiction(into);
            if next != rules {
                lines.push(RulesLine {
                    at_mi,
                    from: rules,
                    into: next,
                });
                rules = next;
            }
        }
        lines
    }

    /// The first customs booth ahead the truck has not stopped at, run or
    /// been cleared through.
    pub(crate) fn customs_booth_ahead(&self) -> Option<&BorderBooth> {
        let here = self.trip.position_mi;
        self.trip
            .border_booths
            .iter()
            .filter(|booth| {
                booth.at_mi > here
                    && !self
                        .enforcement_events
                        .contains(&format!("border-booth:{}", booth.key))
            })
            .min_by(|a, b| a.at_mi.total_cmp(&b.at_mi))
    }

    /// The mile the co-driver has the truck stopped by for the next customs
    /// booth: [`CUSTOMS_HANDBACK_LEAD_MI`] short of it, and short of the
    /// line into its country when that is still ahead. None with no booth
    /// ahead.
    pub fn customs_handback_mi(&self, ctx: &GameContext) -> Option<f64> {
        let booth = self.customs_booth_ahead()?;
        let line = self
            .rules_lines_ahead(ctx)
            .into_iter()
            .rev()
            .find(|line| {
                line.at_mi <= booth.at_mi
                    && rules_country(line.into) == booth.entering_country
                    && rules_country(line.from) != booth.entering_country
            })
            .map(|line| line.at_mi - CUSTOMS_LINE_LEAD_MI);
        let short_of_booth = booth.at_mi - CUSTOMS_HANDBACK_LEAD_MI;
        Some(line.map_or(short_of_booth, |line| line.min(short_of_booth)))
    }

    /// Whether the co-driver is braking for customs or hands back for it
    /// now: within [`CUSTOMS_BRAKE_ROOM_MI`] of the handback point, or past
    /// it.
    pub(crate) fn customs_handback_due(&self, ctx: &GameContext) -> bool {
        // Judged every frame the co-driver drives: a booth still far off
        // costs one pass over the booths.
        let near = self
            .customs_booth_ahead()
            .is_some_and(|booth| booth.at_mi - self.trip.position_mi <= CUSTOMS_SEARCH_MI);
        near && self
            .customs_handback_mi(ctx)
            .is_some_and(|stop_by| stop_by - self.trip.position_mi <= CUSTOMS_BRAKE_ROOM_MI)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rule_set_names_its_country() {
        assert_eq!(rules_country(HosRules::Us), "US");
        assert_eq!(rules_country(HosRules::Alaska), "US");
        assert_eq!(rules_country(HosRules::CanadaSouth60), "CA");
        assert_eq!(rules_country(HosRules::CanadaNorth60), "CA");
    }
}
