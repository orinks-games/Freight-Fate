//! Spoken curve calls -- the co-driver reads the road ahead (port of
//! `freight_fate/states/driving_pacenotes.py`, the `DrivingPacenoteMixin`).
//!
//! The first audible slice of the steering-by-ear design (see
//! docs/steering-sound-rfc.md): plain-language pacenotes from the baked
//! curve geometry, called early enough to brake before the bend, and only
//! when the bend actually demands slowing at the truck's current speed. A
//! gentle sweep taken at a legal speed stays silent -- the road only speaks
//! when it has something to say.
//!
//! Grammar: "Sharp left, half a mile. Advise 35." Severity comes from the
//! baked advisory speed (the number a posted yellow diamond would show);
//! the advisory sentence is included only when the truck is above it.
//! Curves that follow within a breath get a linked tail: "then right."

use ff_core::data::curves::RouteCurve;
use ff_core::speech_text::SpokenMessage;

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_updates::live;

/// The live half of a rescued curve call's validity gate.
///
/// Python handed `say_event` the predicate itself and re-ran it when the
/// rescue fired. A `'static` validity closure cannot borrow the drive, so the
/// bend's own two numbers ride the gate and the readings come from `live` --
/// the same mechanism the scale reminder uses. Copy, because the curve
/// callout submits the same gate from more than one branch.
#[derive(Clone, Copy, Debug)]
pub struct CurveStillTrue {
    start_mi: f64,
    floor_mph: f64,
    /// A sharp or hairpin bend, whose call is judged on the speed the truck
    /// will bring to it (`Trip::next_curve_approach`).
    sharp: bool,
}

impl CurveStillTrue {
    /// The bend still ahead, and the truck still carrying more speed than it
    /// advises.
    pub fn holds(&self) -> bool {
        let ahead = self.start_mi - live::position_mi();
        if ahead <= 0.0 {
            return false;
        }
        let speed = if self.sharp {
            live::approach_speed_mph(ahead)
        } else {
            live::speed_mph()
        };
        speed > self.floor_mph
    }
}

/// A following curve starting within this gap after the called one gets a
/// "then left/right" tail instead of its own later call.
pub const PACENOTE_LINK_GAP_MI: f64 = 0.3;
pub const PACENOTE_MARGIN_MPH: f64 = 3.0;
/// Below this the quarter-mile rounding would LIE upward: a bend 200 feet
/// out spoken as "a quarter mile" reads as time the driver does not have
/// (owner's AZ-260 log, 2026-07-19). Say "just ahead" instead.
pub const PACENOTE_JUST_AHEAD_MI: f64 = 0.15;

/// `_SEVERITY_PHRASE`.
pub fn severity_phrase(severity: &str) -> &'static str {
    match severity {
        "hairpin" => "Hairpin",
        "sharp" => "Sharp",
        "moderate" => "Curve",
        "gentle" => "Gentle bend",
        // Python indexed the dict: an unknown severity was a KeyError, and
        // the curve table only ever produces the four above.
        other => panic!("unknown curve severity: {other}"),
    }
}

/// `_DIRECTION_WORD`.
pub fn direction_word(direction: char) -> &'static str {
    match direction {
        'L' => "left",
        'R' => "right",
        other => panic!("unknown curve direction: {other}"),
    }
}

impl DrivingState {
    /// `_pacenote_phrase(curve)`.
    pub fn pacenote_phrase(&self, curve: &RouteCurve) -> String {
        format!(
            "{} {}",
            severity_phrase(curve.severity()),
            direction_word(curve.direction)
        )
    }

    /// `_pacenote_text(curve, ahead_mi, speed_mph)`: the curve call.
    pub fn pacenote_text(
        &self,
        ctx: &GameContext,
        curve: &RouteCurve,
        ahead_mi: f64,
        speed_mph: f64,
    ) -> SpokenMessage {
        let s = &ctx.settings;
        let distance = if ahead_mi < PACENOTE_JUST_AHEAD_MI {
            "just ahead".to_string()
        } else {
            s.short_distance_text(ahead_mi)
        };
        let mut call = format!("{}, {distance}.", self.pacenote_phrase(curve));
        // The terse half of the pair. Curve calls read the same at every rung
        // until this existed: the ladder asked TERSE for them and got the
        // identical full sentence back, which is why the quiet rung still
        // "felt like standard" through bends (owner playtest, 2026-08-17).
        // Direction and the advisory speed are the two things a driver acts
        // on, so they are what terse keeps; the distance goes, because the
        // call only fires inside the lookahead anyway.
        let mut terse = self.pacenote_phrase(curve);
        // The number this load can take the bend at, which is the sign's
        // except where the sign asks more than the load gives (`driving_rollover`).
        let advisory = self.spoken_advisory_mph(curve);
        // Named whenever the truck is over it by the margin, or over the
        // speed the bend starts costing this load: a call made for the load
        // has to carry the number that answers it.
        if speed_mph > advisory as f64 + PACENOTE_MARGIN_MPH
            || speed_mph > self.trip.bend_costs_above_mph(curve)
        {
            call += &format!(" Advise {}.", s.speed_text(advisory as f64));
            terse += &format!(", {}", s.speed_text(advisory as f64));
        }
        if let Some(linked) = self.pacenote_linked(curve) {
            // The tail is the follower's ONLY call (the trip suppresses its
            // own), so a sharper follower must say so: "then right" hiding a
            // hairpin undersells the road, and a tighter advisory rides along.
            let mut tail = direction_word(linked.direction).to_string();
            if matches!(linked.severity(), "hairpin" | "sharp") {
                tail = format!("{} {tail}", linked.severity());
            }
            let linked_advisory = self.spoken_advisory_mph(&linked);
            if linked_advisory < advisory {
                tail += &format!(", advise {}", s.speed_text(linked_advisory as f64));
            }
            call += &format!(" Then {tail}.");
            terse += &format!(", then {}", direction_word(linked.direction));
        }
        SpokenMessage::with_terse(call, format!("{terse}."))
    }

    /// `_curve_call_still_true(curve)`: a test for whether a rescued curve
    /// call is still worth speaking.
    ///
    /// A call cut off mid-sentence is offered back once so the road's
    /// information is not lost to an interruption. Offered back after the
    /// bend, though, it tells the driver to slow for a corner they are
    /// already through -- the same stale-rescue fault as the weigh station
    /// and the debris call, and the reason a curve call plus its cruise
    /// clause came back repeatedly through one bend (Shane P, 2026-08-21).
    ///
    /// The test is the one the refreshed re-speak already applies: the bend
    /// still ahead, and the truck still carrying more speed than it advises.
    /// Returns None when there is no curve to test, which leaves the rescue
    /// ungated exactly as before.
    ///
    /// Rust: Python returned the predicate itself, re-evaluated when the
    /// rescue fired, so this returns [`CurveStillTrue`] -- the bend's numbers
    /// plus a live reading -- rather than an answer taken at submission time.
    /// A snapshot was always true, because a curve call is only made when the
    /// bend is ahead and the truck is fast, so the gate never refused
    /// anything.
    pub fn curve_call_still_true(&self, curve: Option<&RouteCurve>) -> Option<CurveStillTrue> {
        let curve = curve?;
        Some(CurveStillTrue {
            start_mi: curve.start_mi,
            floor_mph: (curve.advisory_mph as f64 + PACENOTE_MARGIN_MPH)
                .min(self.trip.bend_costs_above_mph(curve)),
            sharp: matches!(curve.severity(), "sharp" | "hairpin"),
        })
    }

    /// `_pacenote_linked(curve)`: the next curve when it follows within a
    /// breath of this one.
    pub fn pacenote_linked(&self, curve: &RouteCurve) -> Option<RouteCurve> {
        self.trip
            .curves_within(curve.end_mi - self.trip.position_mi + PACENOTE_LINK_GAP_MI)
            .into_iter()
            .find(|other| other.start_mi > curve.end_mi)
    }
}
