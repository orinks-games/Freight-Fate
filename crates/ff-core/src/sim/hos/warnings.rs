//! The hours-of-service countdown spoken while driving: which marks have
//! been crossed, and the one line a crossing earns.

use super::clock::{HosClock, HosLimit};
use super::WARNING_THRESHOLDS_MIN;
use crate::pyfmt::fmt_f;

fn threshold_phrase(threshold: f64) -> &'static str {
    if threshold == 120.0 {
        "2 hours"
    } else if threshold == 60.0 {
        "1 hour"
    } else if threshold == 30.0 {
        "30 minutes"
    } else {
        panic!("no phrase for HOS warning threshold {threshold}")
    }
}

impl HosClock {
    /// Speak the countdown again after a rest that did NOT reset the shift.
    ///
    /// Warnings fire once per threshold per shift -- correct while driving,
    /// but a long non-qualifying sleep (a pending sleeper split) left the
    /// marks in place, so the driver woke to silence and drove straight
    /// into a window violation with no countdown (owner, 2026-07-24).
    pub fn re_arm_warnings(&mut self) {
        self.warned.retain(|w| {
            !(w.starts_with("drive:") || w.starts_with("duty:") || w.starts_with("break:"))
        });
    }

    /// Newly crossed warning messages; each threshold fires once.
    ///
    /// Call this every frame while driving. Crossing several thresholds at
    /// once (a long menu action, say) speaks only the most urgent one, but
    /// marks them all so nothing fires late.
    pub fn check_warnings(&mut self, mode: &str) -> Vec<String> {
        self.check_warnings_with(mode, &WARNING_THRESHOLDS_MIN, |_| false)
    }

    /// Whether a countdown mark in `marks` has been reached and not yet
    /// spent: the cheap question to ask before working out whether the
    /// drive fits.
    pub fn mark_due(&self, mode: &str, marks: &[f64]) -> bool {
        self.statuses(mode).iter().any(|s| {
            s.remaining_min > 0.0
                && marks.iter().any(|t| {
                    s.remaining_min <= *t
                        && !self
                            .warned
                            .contains(&format!("{}:{}", s.kind, fmt_f(*t, 0)))
                })
        })
    }

    /// [`Self::check_warnings`] speaking only at `marks` (a driving mode's
    /// own, out of [`WARNING_THRESHOLDS_MIN`]), and silent for a limit the
    /// drive will finish inside: `fits` answers that for the limit as it
    /// stands. A mark passed in silence is spent all the same. A violation
    /// always speaks.
    pub fn check_warnings_with(
        &mut self,
        mode: &str,
        marks: &[f64],
        mut fits: impl FnMut(&HosLimit) -> bool,
    ) -> Vec<String> {
        let mut candidates: Vec<(i32, f64, String)> = Vec::new();
        // Driving time and the duty window both end in the same 10-hour
        // sleep, so one countdown covers them: the nearer limit speaks, and
        // each mark it crosses is spent for the other too. Two countdowns
        // gave a long run six notices on the way to one sleep (player
        // report, 2026-10-17).
        let statuses = self.statuses(mode);
        let sleep_owner = statuses
            .iter()
            .filter(|s| s.kind == "drive" || s.kind == "duty")
            .fold(None::<&HosLimit>, |best, s| match best {
                Some(b) if b.remaining_min <= s.remaining_min => Some(b),
                _ => Some(s),
            })
            .map(|s| s.kind);
        for HosLimit {
            kind,
            remaining_min: rem,
            due,
        } in statuses
        {
            if rem <= 0.0 {
                let key = format!("{kind}:violation");
                if !self.warned.contains(&key) {
                    self.warned.push(key);
                    for t in WARNING_THRESHOLDS_MIN {
                        // swallow the lead-up ones
                        let k = format!("{kind}:{}", fmt_f(t, 0));
                        if !self.warned.contains(&k) {
                            self.warned.push(k);
                        }
                    }
                    let priority = match kind {
                        "drive" => 0,
                        "duty" => 1,
                        "break" => 2,
                        _ => 9,
                    };
                    candidates.push((
                        priority,
                        rem,
                        format!(
                            "Hours of service violation: {due}. \
                             Driving on risks fines at inspections."
                        ),
                    ));
                }
                continue;
            }
            let shares_sleep = kind == "drive" || kind == "duty";
            if shares_sleep && sleep_owner != Some(kind) {
                continue;
            }
            let crossed: Vec<f64> = marks
                .iter()
                .copied()
                .filter(|t| rem <= *t && !self.warned.contains(&format!("{kind}:{}", fmt_f(*t, 0))))
                .collect();
            if !crossed.is_empty() {
                let spent = if shares_sleep {
                    vec!["drive", "duty"]
                } else {
                    vec![kind]
                };
                for t in &crossed {
                    for spent_kind in &spent {
                        let key = format!("{spent_kind}:{}", fmt_f(*t, 0));
                        if !self.warned.contains(&key) {
                            self.warned.push(key);
                        }
                    }
                }
                let limit = HosLimit {
                    kind,
                    remaining_min: rem,
                    due,
                };
                if fits(&limit) {
                    continue;
                }
                // Named by the nearest mark the clock has actually reached, so
                // a jump past a driving mode's own mark still says how much
                // is left.
                let smallest = WARNING_THRESHOLDS_MIN
                    .iter()
                    .copied()
                    .chain(crossed.iter().copied())
                    .filter(|t| rem <= *t)
                    .fold(f64::INFINITY, f64::min);
                let phrase = threshold_phrase(smallest);
                candidates.push((10, rem, format!("Hours of service: {phrase} until {due}.")));
            }
        }
        if candidates.is_empty() {
            return Vec::new();
        }
        // Python's min over (priority, remaining) tuples: the first on a tie.
        let mut best = 0;
        for i in 1..candidates.len() {
            let (p, r, _) = &candidates[i];
            let (bp, br, _) = &candidates[best];
            if p < bp || (p == bp && r < br) {
                best = i;
            }
        }
        vec![candidates.swap_remove(best).2]
    }
}
