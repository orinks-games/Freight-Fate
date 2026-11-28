//! The co-driver clears customs alone (owner ruling, 2026-11-28 13:00 UTC).
//! On All assists the truck stops at the booth as it does for a solo
//! driver; the clearance's effects follow with no menu
//! (`DrivingState::clear_customs`), charged to the co-driver's logbook with
//! the player's berth time running, and one note tells the player. The
//! officer's lines and the booth's notice are not said for their crossing.
//!
//! A slept shift's jump ends short of any line where the hours rules change
//! and short of the booth, so both are driven live and the time on each
//! side is counted under the right rules.

use ff_core::sim::hos::HosRules;
use ff_core::sim::trip_models::BorderBooth;

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use super::notes::NoteWeight;

/// A line ahead on the trip where the state or province changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct RulesLine {
    pub at_mi: f64,
    pub from: HosRules,
    pub into: HosRules,
}

/// The co-driver's note for a clearance at `booth_name` into `country`
/// ("CA" or "US").
pub fn customs_note(
    co_driver_name: &str,
    booth_name: &str,
    country: &str,
    referred: bool,
) -> String {
    let minutes = format!("{BORDER_SECONDARY_EXTRA_MIN:.0}");
    match (country == "CA", referred) {
        (true, false) => format!("{co_driver_name} cleared customs into Canada at {booth_name}."),
        (false, false) => {
            format!("{co_driver_name} cleared customs into the United States at {booth_name}.")
        }
        (true, true) => format!(
            "{co_driver_name} cleared customs into Canada at {booth_name} after a \
             {minutes}-minute secondary inspection."
        ),
        (false, true) => format!(
            "{co_driver_name} cleared customs into the United States at {booth_name} after a \
             {minutes}-minute secondary inspection."
        ),
    }
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

    /// The co-driver at a customs booth, the truck stopped: the clearance
    /// with no menu, and one note. They release the brake and drive on.
    pub(crate) fn co_driver_clears_customs(&mut self, ctx: &mut GameContext, booth: &BorderBooth) {
        let Some(co_driver_name) = self.co_driver_at_wheel_name() else {
            return;
        };
        let referred = self.clear_customs(ctx, booth);
        // The booth is a stop the truck drives on from, not an arrival: the
        // paused speed control comes back once the truck is rolling, as it
        // does at a ramp's light. Held the arrival way it never came back
        // under the co-driver, who only arms a disarmed one, and the truck
        // ran 83 in a 50 work zone on the bare accelerator (agent drive,
        // 2026-11-28).
        if self.speed_control_paused_at_stop {
            self.speed_control_transit_pause = true;
        }
        let line = customs_note(
            co_driver_name,
            &booth.name,
            &booth.entering_country,
            referred,
        );
        self.co_driver_note(ctx, line, NoteWeight::Wake);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_note_names_the_country_and_a_secondary_inspection() {
        assert_eq!(
            customs_note("Dana", "Abbotsford-Huntingdon Port of Entry", "CA", false),
            "Dana cleared customs into Canada at Abbotsford-Huntingdon Port of Entry."
        );
        assert_eq!(
            customs_note("Dana", "Sumas Port of Entry", "US", true),
            "Dana cleared customs into the United States at Sumas Port of Entry after a \
             45-minute secondary inspection."
        );
    }
}
