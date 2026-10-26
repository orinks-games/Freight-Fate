//! Descent control: the speed adaptive cruise holds a downgrade at, what it
//! says about it, and when a hill has beaten it.
//!
//! Split out of `cruise_loop` (which runs the pedal) because the number it
//! works to is its own question: how fast can THIS truck, at THIS weight,
//! hold THIS grade. The answer is `TruckState::safe_descent_mph`, the Grade
//! Severity Rating System's rule run on the truck's own brake heat model
//! (see `ff_core::sim::vehicle::descent` for the method and its sources).

use ff_core::sim::transmission::JAKE_MAX_RPM;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

impl DrivingState {
    /// The speed descent control is actually working to: the set speed
    /// under the posted cap cruise keeps, the grade's safe descent speed,
    /// and any ceiling a brake or the interactive level put on this grade.
    ///
    /// The posted cap is here because it is what the truck holds. "Descent
    /// control holding 85 miles per hour" was said on a 5.8 percent grade
    /// while cruise held 68 for the limit (owner's playtest, I-70 east of
    /// the Eisenhower tunnel, 2026-09-24): the set speed is a number nothing
    /// on the road was going to allow, and on a seven percent grade it names
    /// a runaway.
    pub fn descent_hold_mph(&self) -> f64 {
        let mut target = self.cruise_mph.unwrap_or(CRUISE_MIN_MPH);
        for cap in [
            self.descent_posted_cap_mph,
            self.cruise_descent_mph,
            self.descent_safe_mph,
        ]
        .into_iter()
        .flatten()
        {
            target = target.min(cap);
        }
        target
    }

    /// How far the truck is over the top speed of the gear descent control
    /// holds it in, or None when no gear is held. That top is the revs a
    /// guard band under the retarder's ceiling: past it the truck is over the
    /// only number the held gear can keep, whatever the target says, and the
    /// retarder answers it before the box's protective upshift or cruise's
    /// revs guard has to. Measured against the target alone, a light load
    /// under 55 on Siskiyou's 6.3 percent had its retarder stepped down to
    /// nothing and rode eighth's ceiling at 50 on a snub every two seconds,
    /// and the J key's manager let a loaded truck on I-70's 4.9 percent spin
    /// into that upshift at 71 while it waited for 70 plus a mile an hour
    /// (bend sweep and descent bench, 2026-09-25).
    pub fn held_gear_overspeed_mph(&self) -> Option<f64> {
        let truck = &self.trip.truck;
        let rpm = truck.coupled_rpm(None);
        (truck.descent_gear_hold && truck.transmission.automatic && rpm > 0.0).then(|| {
            let top_rpm = JAKE_MAX_RPM - 2.0 * DESCENT_RPM_GUARD;
            truck.speed_mph() * (1.0 - top_rpm / rpm)
        })
    }

    /// The safe descent speed for the hill under and just ahead of the
    /// truck, or None where the grade needs none (or descent control is off).
    ///
    /// The steepest grade inside the advisory's own look-ahead, so the truck
    /// is already at the number when the steep pitch starts -- gear and speed
    /// chosen at the top, which is the one rule every mountain-driving manual
    /// agrees on -- and holds it until the steep part is behind it. Cached
    /// on that grade, because the answer only changes when the hill does.
    pub fn refresh_descent_safe(&mut self, ctx: &GameContext) {
        // The posted cap cruise keeps here, for `descent_hold_mph`.
        let (limit, reason) = self.trip.speed_limit_at(self.trip.position_mi);
        let restricted = reason
            .as_deref()
            .is_some_and(|reason| RESTRICTED_ZONE_REASONS.contains(&reason));
        self.descent_posted_cap_mph = Some(if restricted {
            limit
        } else {
            limit + ACC_LIMIT_OFFSET_MPH
        });
        if ctx.settings.descent_speed_control == "off" {
            self.descent_safe_mph = None;
            // Forget the grade too, or turning it back on mid-grade finds
            // the same key and never sets a safe speed again (2026-09-28).
            self.descent_safe_key = None;
            return;
        }
        let Some(steepest) = self.steep_descent_ahead() else {
            self.descent_safe_mph = None;
            self.descent_safe_key = None;
            return;
        };
        let key = (steepest * 1000.0).round() as i64;
        if self.descent_safe_key == Some(key) {
            return;
        }
        self.descent_safe_key = Some(key);
        self.descent_safe_mph = self
            .trip
            .truck
            .safe_descent_mph(steepest, CRUISE_BRAKE_OVER_MPH);
    }

    /// The steepest grade (a fraction, negative) under the truck and inside
    /// the grade advisory's look-ahead, when it is a descent-control grade.
    pub fn steep_descent_ahead(&self) -> Option<f64> {
        let here = self.trip.position_mi;
        let end = self.trip.total_miles().min(here + GRADE_WARN_LOOKAHEAD_MI);
        let mut steepest = self.trip.grade_at(here);
        let mut probe = here + GRADE_WARN_STEP_MI / 2.0;
        while probe <= end {
            steepest = steepest.min(self.trip.grade_at(probe));
            probe += GRADE_WARN_STEP_MI / 2.0;
        }
        // The same entry line the control itself engages at: shallower than
        // this is not a descent-control grade at all.
        (steepest <= -DESCENT_CONTROL_GRADE).then_some(steepest)
    }

    /// The safe descent speed for the hill here, worked out fresh -- for the
    /// D key, which answers whatever the descent-control setting is.
    pub fn safe_descent_here_mph(&self) -> Option<f64> {
        let steepest = self.steep_descent_ahead()?;
        self.trip
            .truck
            .safe_descent_mph(steepest, CRUISE_BRAKE_OVER_MPH)
    }

    /// Has the hill actually beaten descent control, or is it holding it?
    ///
    /// "Descent control can't hold this grade. Brake." is the
    /// loudest thing the assist says, and it used to be a single frame's
    /// arithmetic: speed over the ceiling by ten. On the interactive level
    /// that ceiling is [`DESCENT_SAFE_MAX_MPH`], imposed the moment a
    /// downgrade starts, so on a 75 mph road with cruise set at 80 the sum was
    /// already true on the first frame of every dip -- before the control had
    /// done anything, and while it was about to do it. The owner heard it
    /// three times in a minute on I-70 west of Vail (2026-08-24) and the G key
    /// answered "Level road" in between: the dips are a quarter of a mile, and
    /// the truck was braking hard through every one of them.
    ///
    /// So being over the number is only the first of three, and the other two
    /// are the ones that make the sentence true:
    ///
    /// * still genuinely over what the control is working to, by
    ///   [`DESCENT_BEATEN_MPH`];
    /// * still GAINING speed with everything applied -- the same net-force
    ///   verdict the spoken G readout gives the driver
    ///   ([`TruckState::net_accel_mph_per_s`] against
    ///   [`GRADE_HOLDING_MPH_PER_S`]), so the warning and the readout can
    ///   never contradict each other about one moment of road;
    /// * and holding that for [`DESCENT_BEATEN_S`], because one frame is a
    ///   grade boundary, not a runaway.
    ///
    /// The mirror of `say_cruise_out_of_truck` on the climb side, which was
    /// given these same three guards in 2026-07 for the same reason.
    fn descent_is_beaten(&mut self, dt: f64) -> bool {
        let over = self.trip.truck.speed_mph() - self.descent_hold_mph();
        let gaining = self.trip.truck.net_accel_mph_per_s() > GRADE_HOLDING_MPH_PER_S;
        if over <= DESCENT_BEATEN_MPH || !gaining {
            self.descent_beaten_s = 0.0;
            return false;
        }
        self.descent_beaten_s += dt;
        self.descent_beaten_s >= DESCENT_BEATEN_S
    }

    /// Say the number descent control holds -- once per number.
    ///
    /// Once per CHANGE, not once per grade: the owner's drive into Denver
    /// heard "holding 85" twice four minutes apart for two pitches of one
    /// mountain, and a rolling road enters and leaves the control on every
    /// dip. A number already said is not news; a new one is, whenever it
    /// comes, which is why this has no cooldown of its own.
    ///
    /// And only descent control's OWN number: the hill's safe descent speed,
    /// interactive's ceiling, or a brake's capture. Where none of them binds,
    /// cruise is holding its own target down the grade, and "holding 70" on
    /// a 65 road named the limit plus five -- a number cruise already had
    /// and the hill never asked for (descent bench, 2026-09-24).
    fn say_descent_hold(&mut self, ctx: &mut GameContext) {
        let hold = self.descent_hold_mph();
        let own_number = [self.cruise_descent_mph, self.descent_safe_mph]
            .into_iter()
            .flatten()
            .any(|cap| (cap - hold).abs() < 0.01);
        let held = hold.round();
        if !own_number || self.descent_said_mph == Some(held) || self.terse_speech(ctx) {
            return;
        }
        self.descent_said_mph = Some(held);
        // ROUTE, not the ambient default: names an automation that just took
        // the brakes for a grade (automation-handoff sweep, 2026-08-20, the
        // deferred 2026-08-15 audit).
        let spoken = ctx.settings.speed_text(held);
        self.say_route_confirmation(ctx, format!("Descent control holding {spoken}."));
    }

    /// The descent-control half of `_update_cruise`; true when it returns.
    pub(crate) fn update_descent_control(
        &mut self,
        ctx: &mut GameContext,
        dt: f64,
        braking: bool,
    ) -> bool {
        let descent_level = ctx.settings.descent_speed_control.clone();
        self.refresh_descent_safe(ctx);
        // Engages at the trigger and lets go only where the road is no longer
        // a grade at all: the same hysteresis pair the retarder uses. A 2.4
        // percent stretch between the 5.8 and the 7.0 on I-70 released the
        // control, its cap and its retarder, and took them all up again a
        // mile later (owner's playtest, 2026-09-24).
        let descending = descent_level != "off"
            && (self.trip.truck.grade <= -DESCENT_CONTROL_GRADE
                || (self.descent_control_active && self.on_downgrade()));
        self.trip.truck.descent_gear_hold = descending && self.cruise_mph.is_some();
        if descending && self.cruise_mph.is_some() {
            if braking && matches!(descent_level.as_str(), "balanced" | "interactive") {
                self.descent_control_active = true;
                let new_target = CRUISE_MIN_MPH.max(self.trip.truck.speed_mph());
                let previous = self.cruise_descent_mph;
                self.descent_capture_active = true;
                // A CAP FOR THIS GRADE, never a rewrite of the driver's set
                // speed -- the same correction the interactive branch below
                // already carries, which this one was missed out of. Assigning
                // into cruise_mph made every brake on a downgrade permanent
                // and cumulative: 65 becomes 55 on one hill, 49 on the next,
                // and cruise never climbs back on the flat because 49 IS the
                // set speed now. Brandon drove a whole run pinned at "forty
                // nine mph or lower and losing speed" (2026-08-23).
                //
                // Taking the lower of any cap already standing keeps a
                // deliberate brake from being undone by the automatic cap a
                // frame later; the whole thing is released together when the
                // grade ends.
                self.cruise_descent_mph = Some(match previous {
                    Some(previous) => previous.min(new_target),
                    None => new_target,
                });
                // The working setpoint still follows the truck down now, so
                // cruise does not fight the brake the driver is holding.
                self.cruise_working_mph = Some(new_target);
                let held = self.descent_hold_mph().round();
                if self
                    .descent_said_mph
                    .is_none_or(|said| (held - said).abs() >= 2.0)
                {
                    self.descent_said_mph = Some(held);
                    let spoken = ctx.settings.speed_text(held);
                    let mut opts = SayEvent::queued();
                    opts.category = Some(SpeechCategory::Confirmation);
                    ctx.say_event_with(
                        format!("Descent control holding {spoken} for this grade."),
                        opts,
                    );
                }
                return true;
            }
            self.descent_capture_active = false;
            if descent_level == "interactive" {
                // A cap that lives as long as the grade does, not a rewrite
                // of the driver's set speed. It used to assign straight into
                // _cruise_mph, so one 3 percent dip on a 65 road knocked
                // cruise down to 55 permanently -- on the flat, uphill, the
                // rest of the run (bench trace, 2026-07-25: 62 set, 55 held
                // ever after). The driver's number now survives the hill.
                // Never above a cap the driver's own brake already set on
                // this grade: capture is an instruction, not a suggestion.
                // Set before the number is spoken, so the first line names it.
                self.cruise_descent_mph = Some(
                    DESCENT_SAFE_MAX_MPH
                        .min(self.cruise_descent_mph.unwrap_or(DESCENT_SAFE_MAX_MPH)),
                );
            }
            if !self.descent_control_active {
                self.descent_control_active = true;
                self.say_descent_hold(ctx);
            } else if self
                .descent_safe_mph
                .is_some_and(|safe| (safe - self.descent_hold_mph()).abs() < 0.01)
            {
                // The hill itself changed the number: a steeper pitch ahead
                // brought the safe speed down, or the steep part is behind.
                self.say_descent_hold(ctx);
            }
            let mut limit_state = String::new();
            let mut limit_message = String::new();
            // Mid-shift the clutch lets the revs fall to idle; that is the
            // shift, not a gear too tall, and warning on it spoke at every
            // manual shift on the hill, the right downshift included.
            let tr = &self.trip.truck.transmission;
            let mid_shift = tr.clutch > 0.5 || tr.shifting();
            if !tr.automatic && !mid_shift && self.trip.truck.rpm < 1100.0 {
                limit_state = "gear".to_string();
                limit_message = "Descent control needs a lower gear.".to_string();
                self.descent_beaten_s = 0.0; // a different limit; not this count
            } else if self.trip.truck.grip < 0.55 {
                limit_state = "traction".to_string();
                limit_message = "Low traction limits descent control.".to_string();
                self.descent_beaten_s = 0.0;
            } else {
                // The retarder is staged against the overspeed further down,
                // not pinned open here. Selecting all three stages the moment
                // the grade passed 2.5 percent over-retarded every descent
                // gentler than the one that balances full jake: a 4 percent
                // grade settled seven mph under the set speed and stayed
                // there, with cruise at full throttle fighting its own
                // engine brake (bench trace, 2026-07-25: 62 set, 54.9 held).
                if descent_level == "interactive" {
                    let safe_target = self.descent_hold_mph();
                    let speed = self.trip.truck.speed_mph();
                    if speed > safe_target + 7.0 {
                        // Faded in across the mile an hour under the old
                        // +8 edge, which switched straight to a third of
                        // the pedal and pumped it on a grade the retarder
                        // could not hold: air is charged per application.
                        let feather = (speed - safe_target - 7.0).min(1.0);
                        let brake = feather * 0.7f64.min((speed - safe_target) / 25.0);
                        self.trip.truck.brake = self.trip.truck.brake.max(brake);
                    }
                }
                if self.descent_is_beaten(dt) {
                    limit_state = "grade".to_string();
                    limit_message = "Descent control can't hold this grade. Brake.".to_string();
                }
            }
            if limit_state != self.descent_limit_state {
                self.descent_limit_state = limit_state;
                if !limit_message.is_empty() {
                    self.say_safety_interrupt(ctx, &limit_message);
                }
            }
        } else if self.descent_control_active {
            self.descent_control_active = false;
            self.descent_limit_state = String::new();
            self.descent_beaten_s = 0.0;
            self.descent_capture_active = false;
            // The grade is behind us; so is its cap.
            self.cruise_descent_mph = None;
            // Release only the retarder cruise itself raised: the driver's own
            // jake switch survives the road levelling out.
            if self.cruise_jake_stage > 0 {
                self.cruise_jake_stage = 0;
                self.trip.truck.engine_brake_stage = 0;
            }
        }
        false
    }
}
