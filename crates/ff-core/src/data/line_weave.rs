//! The load-time screen for a road that weaves along a state or province
//! line.
//!
//! The Alaska Highway rides the 60th parallel past Lower Post, and Fort
//! Nelson to Watson Lake crosses the British Columbia and Yukon line five
//! times in 36 miles. The crossings are true and stay in the data; spoken
//! one by one they would switch the hours rules back and forth and call
//! "Crossing into Yukon" four times. This screen keeps the crossings that
//! mean a real change of jurisdiction.
//!
//! Rule: on a leg that crosses the same line more than twice, a stay on the
//! far side shorter than [`MIN_WEAVE_STAY_MI`] is dropped together with its
//! return. A leg that crosses a line once or twice is untouched, so a true
//! in-and-out (the 40 miles of British Columbia between Watson Lake and
//! Whitehorse) stays, and so does every US leg.
//!
//! The threshold is calibrated on the only weave in the map: its short stays
//! are 1.3 to 4.8 miles, and the next real stays on the same legs are 27.7
//! and 40 miles, so 5 miles separates them with room on both sides. It is a
//! screen at load, never an edit of the data.

use super::world_models::{StateCrossing, StateMileage};

/// The shortest stay across a weaving line that counts as being there.
pub const MIN_WEAVE_STAY_MI: f64 = 5.0;

fn line_key(c: &StateCrossing) -> (String, String) {
    let (a, b) = (c.from_state.clone(), c.state.clone());
    if a <= b {
        (a, b)
    } else {
        (b, a)
    }
}

/// The crossings with each short weave stay and its return removed.
pub fn screen_weave(crossings: Vec<StateCrossing>) -> Vec<StateCrossing> {
    let weaving = |c: &StateCrossing| {
        let key = line_key(c);
        crossings.iter().filter(|o| line_key(o) == key).count() > 2
    };
    let flags: Vec<bool> = crossings.iter().map(weaving).collect();
    let mut kept: Vec<(StateCrossing, bool)> = Vec::new();
    for (c, weaves) in crossings.into_iter().zip(flags) {
        if let Some((last, last_weaves)) = kept.last() {
            let returns = c.from_state == last.state && c.state == last.from_state;
            if weaves && *last_weaves && returns && c.at_mi - last.at_mi < MIN_WEAVE_STAY_MI {
                kept.pop();
                continue;
            }
        }
        kept.push((c, weaves));
    }
    kept.into_iter().map(|(c, _)| c).collect()
}

/// State miles rebuilt from screened crossings, one row per stay. Called
/// only when the screen removed something.
pub fn miles_from_crossings(
    crossings: &[StateCrossing],
    start_state: &str,
    leg_miles: f64,
) -> Vec<StateMileage> {
    let mut out = Vec::new();
    let mut state = start_state.to_string();
    let mut from = 0.0;
    for c in crossings {
        out.push(StateMileage {
            state: state.clone(),
            miles: ((c.at_mi - from) * 10.0).round() / 10.0,
        });
        state = c.state.clone();
        from = c.at_mi;
    }
    out.push(StateMileage {
        state,
        miles: ((leg_miles - from) * 10.0).round() / 10.0,
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(at: f64, from: &str, to: &str) -> StateCrossing {
        StateCrossing {
            at_mi: at,
            from_state: from.into(),
            state: to.into(),
            place: String::new(),
            source: String::new(),
        }
    }

    fn at(list: &[StateCrossing]) -> Vec<f64> {
        list.iter().map(|c| c.at_mi).collect()
    }

    const BC: &str = "British Columbia";
    const YT: &str = "Yukon";

    #[test]
    fn the_screen_keeps_only_real_changes_of_jurisdiction() {
        // (case, crossings, kept at)
        let cases: Vec<(&str, Vec<StateCrossing>, Vec<f64>)> = vec![
            (
                "Fort Nelson to Watson Lake: one entry into Yukon, at Lower Post",
                vec![
                    c(276.2, BC, YT),
                    c(281.0, YT, BC),
                    c(282.6, BC, YT),
                    c(283.9, YT, BC),
                    c(311.7, BC, YT),
                ],
                vec![311.7],
            ),
            (
                "Watson Lake to Fort Nelson: one entry into British Columbia",
                vec![
                    c(7.3, YT, BC),
                    c(35.0, BC, YT),
                    c(36.4, YT, BC),
                    c(38.0, BC, YT),
                    c(42.8, YT, BC),
                ],
                vec![7.3],
            ),
            (
                "Watson Lake to Whitehorse: in and out of British Columbia stays",
                vec![c(96.6, YT, BC), c(136.8, BC, YT)],
                vec![96.6, 136.8],
            ),
            (
                "Two crossings of one line a mile apart are not a weave",
                vec![c(10.0, "Ohio", "Indiana"), c(11.0, "Indiana", "Ohio")],
                vec![10.0, 11.0],
            ),
            (
                "Different lines never pair up",
                vec![
                    c(5.0, "Idaho", "Montana"),
                    c(6.0, "Montana", "Wyoming"),
                    c(7.0, "Wyoming", "Montana"),
                ],
                vec![5.0, 6.0, 7.0],
            ),
            ("No crossings", vec![], vec![]),
        ];
        for (case, crossings, kept) in cases {
            assert_eq!(at(&screen_weave(crossings)), kept, "{case}");
        }
    }

    #[test]
    fn screened_miles_follow_the_kept_crossings() {
        let kept = screen_weave(vec![
            c(276.2, BC, YT),
            c(281.0, YT, BC),
            c(282.6, BC, YT),
            c(283.9, YT, BC),
            c(311.7, BC, YT),
        ]);
        let miles = miles_from_crossings(&kept, BC, 319.0);
        let rows: Vec<(&str, f64)> = miles.iter().map(|m| (m.state.as_str(), m.miles)).collect();
        assert_eq!(rows, vec![(BC, 311.7), (YT, 7.3)]);
    }
}
