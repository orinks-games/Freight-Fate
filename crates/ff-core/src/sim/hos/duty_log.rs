//! The in-cab Record of Duty Status: `DutySegment` rows in a rolling `DutyLog`.

use serde_json::{json, Value};

use super::pyjson::{py_float_or, py_iter, py_max, py_min, py_str, py_str_or, py_truthy};
use super::{is_duty_status, DUTY_STATUSES, RODS_WINDOW_HOURS};

/// One Record of Duty Status row, in absolute career-clock hours.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DutySegment {
    pub status: String,
    pub start_hour: f64,
    pub end_hour: f64,
    pub location: String,
    pub note: String,
}

impl DutySegment {
    pub fn new(status: &str, start_hour: f64, end_hour: f64, location: &str, note: &str) -> Self {
        Self {
            status: status.to_string(),
            start_hour,
            end_hour,
            location: location.to_string(),
            note: note.to_string(),
        }
    }

    pub fn duration_hours(&self) -> f64 {
        py_max(0.0, self.end_hour - self.start_hour)
    }

    pub fn to_dict(&self) -> Value {
        json!({
            "status": self.status,
            "start_hour": self.start_hour,
            "end_hour": self.end_hour,
            "location": self.location,
            "note": self.note,
        })
    }

    /// `None` for anything that is not a readable row.
    pub fn from_dict(data: &Value) -> Option<Self> {
        let obj = data.as_object()?;
        let status = py_str_or(obj.get("status"), "");
        if !is_duty_status(&status) {
            return None;
        }
        let start = py_float_or(obj.get("start_hour"), 0.0)?;
        let mut end = py_float_or(obj.get("end_hour"), start)?;
        if !start.is_finite() || !end.is_finite() {
            return None;
        }
        if end < start {
            end = start;
        }
        // `str(data.get("location", "") or "unknown location")`
        let location = match obj.get("location") {
            Some(value) if py_truthy(value) => py_str(value),
            _ => "unknown location".to_string(),
        };
        let note = match obj.get("note") {
            Some(value) if py_truthy(value) => py_str(value),
            _ => String::new(),
        };
        Some(Self {
            status,
            start_hour: py_max(0.0, start),
            end_hour: py_max(0.0, end),
            location,
            note,
        })
    }
}

/// Hours per duty status over a window, what `DutyLog.totals_since` returned
/// as a dict keyed by status.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct DutyTotals {
    pub driving: f64,
    pub on_duty_not_driving: f64,
    pub off_duty: f64,
    pub sleeper_berth: f64,
}

impl DutyTotals {
    /// `totals[status]`; an unknown status is 0, where Python would KeyError.
    pub fn get(&self, status: &str) -> f64 {
        match status {
            "driving" => self.driving,
            "on_duty_not_driving" => self.on_duty_not_driving,
            "off_duty" => self.off_duty,
            "sleeper_berth" => self.sleeper_berth,
            _ => 0.0,
        }
    }

    fn add(&mut self, status: &str, hours: f64) {
        match status {
            "driving" => self.driving += hours,
            "on_duty_not_driving" => self.on_duty_not_driving += hours,
            "off_duty" => self.off_duty += hours,
            "sleeper_berth" => self.sleeper_berth += hours,
            _ => {}
        }
    }

    /// The statuses in ledger order, for callers that iterated the dict.
    pub fn entries(&self) -> [(&'static str, f64); 4] {
        [
            (DUTY_STATUSES[0], self.driving),
            (DUTY_STATUSES[1], self.on_duty_not_driving),
            (DUTY_STATUSES[2], self.off_duty),
            (DUTY_STATUSES[3], self.sleeper_berth),
        ]
    }
}

/// Rolling in-cab Record of Duty Status.
///
/// Kept separate from `HosClock`: the clock remains the aggregate rules
/// engine, while the logbook records the chronological status rows a driver
/// and trooper can review.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct DutyLog {
    pub segments: Vec<DutySegment>,
}

impl DutyLog {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(
        &mut self,
        status: &str,
        start_hour: f64,
        end_hour: f64,
        location: &str,
        note: &str,
    ) {
        if !is_duty_status(status) {
            return;
        }
        let start = py_max(0.0, start_hour);
        let end = py_max(start, end_hour);
        if !start.is_finite() || !end.is_finite() {
            return;
        }
        let location = if location.is_empty() {
            "unknown location"
        } else {
            location
        };
        if end == start {
            return;
        }
        if let Some(last) = self.segments.last_mut() {
            if last.status == status && last.location == location && last.note == note {
                last.end_hour = py_max(last.end_hour, end);
                self.prune(end);
                return;
            }
            if last.end_hour < start {
                last.end_hour = start;
            }
        }
        self.segments
            .push(DutySegment::new(status, start, end, location, note));
        self.prune(end);
    }

    /// Record a classified interval without filling gaps in earlier rows.
    /// Adjacent rows with identical details coalesce so frame updates stay O(1).
    pub fn record_exact(
        &mut self,
        status: &str,
        start_hour: f64,
        end_hour: f64,
        location: &str,
        note: &str,
    ) {
        if !is_duty_status(status) {
            return;
        }
        let mut start = py_max(0.0, start_hour);
        let end = py_max(start, end_hour);
        if !start.is_finite() || !end.is_finite() || end == start {
            return;
        }
        let location = if location.is_empty() {
            "unknown location"
        } else {
            location
        };
        if let Some(last) = self.segments.last_mut() {
            if start < last.end_hour {
                if end <= last.end_hour {
                    return;
                }
                start = last.end_hour;
            }
            if last.status == status
                && last.location == location
                && last.note == note
                && (last.end_hour - start).abs() <= 1e-9
            {
                last.end_hour = end;
                return;
            }
        }
        self.segments
            .push(DutySegment::new(status, start, end, location, note));
        self.prune(end);
    }

    /// Drop rows older than the RODS window (`prune(now_hour)` in Python).
    pub fn prune(&mut self, now_hour: f64) {
        self.prune_keeping(now_hour, RODS_WINDOW_HOURS);
    }

    pub fn prune_keeping(&mut self, now_hour: f64, keep_hours: f64) {
        let cutoff = py_max(0.0, now_hour - keep_hours);
        let mut kept: Vec<DutySegment> = Vec::new();
        for mut segment in self.segments.drain(..) {
            if segment.end_hour <= cutoff {
                continue;
            }
            if segment.start_hour < cutoff {
                segment.start_hour = cutoff;
            }
            kept.push(segment);
        }
        self.segments = kept;
    }

    /// Rewind the ledger to a restored save boundary, clipping an interval
    /// that crosses it and dropping entries from the discarded future.
    pub fn truncate_after(&mut self, end_hour: f64) {
        if !end_hour.is_finite() {
            return;
        }
        let mut kept = Vec::with_capacity(self.segments.len());
        for mut segment in self.segments.drain(..) {
            if segment.start_hour >= end_hour {
                continue;
            }
            if segment.end_hour > end_hour {
                segment.end_hour = end_hour;
            }
            if segment.end_hour > segment.start_hour {
                kept.push(segment);
            }
        }
        self.segments = kept;
    }

    pub fn totals_since(&self, start_hour: f64, end_hour: f64) -> DutyTotals {
        let mut totals = DutyTotals::default();
        for segment in &self.segments {
            let start = py_max(start_hour, segment.start_hour);
            let end = py_min(end_hour, segment.end_hour);
            if end > start {
                totals.add(&segment.status, end - start);
            }
        }
        totals
    }

    /// The last `count` rows (`recent(8)` by default in Python).
    pub fn recent(&self, count: usize) -> &[DutySegment] {
        let skip = self.segments.len().saturating_sub(count);
        &self.segments[skip..]
    }

    pub fn current_status(&self) -> &str {
        match self.segments.last() {
            Some(segment) => &segment.status,
            None => "off_duty",
        }
    }

    pub fn to_dict(&self) -> Value {
        json!({
            "segments": self.segments.iter().map(DutySegment::to_dict).collect::<Vec<_>>(),
        })
    }

    /// Tolerant load; a `segments` value Python could not iterate (a number,
    /// a bool, `None`) raised there and reads as an empty log here.
    pub fn from_dict(data: &Value) -> Self {
        let Some(obj) = data.as_object() else {
            return Self::new();
        };
        let mut segments: Vec<DutySegment> = py_iter(obj.get("segments"))
            .unwrap_or_default()
            .iter()
            .filter_map(DutySegment::from_dict)
            .collect();
        // Python's sort is stable; the rows are finite so the order is total.
        segments.sort_by(|a, b| {
            a.start_hour
                .partial_cmp(&b.start_hour)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        Self { segments }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_after_leaves_empty_logs_and_ignores_non_finite_boundaries() {
        let mut empty = DutyLog::new();
        empty.truncate_after(12.0);
        assert!(empty.segments.is_empty());

        let mut log = DutyLog::new();
        log.record_exact("driving", 1.0, 2.0, "route", "");
        let original = log.clone();

        log.truncate_after(f64::NAN);
        assert_eq!(log, original);
        log.truncate_after(f64::INFINITY);
        assert_eq!(log, original);
    }
}
