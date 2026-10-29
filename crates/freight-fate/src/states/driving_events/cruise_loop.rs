//! The adaptive-cruise loop itself: the gap, the posted-limit lookahead, the
//! grade preview, and holding the target from above.

use crate::speech::EventPriority;
use ff_core::sim::transmission::JAKE_MAX_RPM;
use ff_core::sim::trip::{is_highway_zone_reason, LIMIT_WARNING_MAX_LEAD_MI};
use ff_core::speech_pacing::SpeechCategory;
use ff_core::speech_text::SpokenMessage;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_stops::assist_full_decel_mps2;

/// Which case of the grade preview produced the bias, so the cue can say
/// what the road is doing rather than one vague "the road ahead" line.
/// `EasingForDescent` carries the coming downgrade as a positive grade
/// fraction (0.05 is five percent).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PccReason {
    None,
    Building,
    EasingForDescent(f64),
    HoldingOverCrest,
}

impl DrivingState {
    /// Seconds of room adaptive cruise leaves to the vehicle ahead.
    ///
    /// The driver's chosen cushion is the floor; weather only ever adds to it.
    /// Someone who picked "close" on a clear day still gets the full wet-road
    /// opening when it rains, and someone who picked "far" never has it
    /// quietly shortened back to the middle.
    pub fn acc_gap_seconds(&self, ctx: &GameContext) -> f64 {
        let chosen =
            acc_gap_seconds(&ctx.settings.acc_following_gap).unwrap_or(ACC_BASE_GAP_SECONDS);
        let effects = self.trip.weather.effects();
        let mut gap = chosen;
        if effects.grip < 0.9 {
            gap += (0.9 - effects.grip) * 4.2;
        }
        if effects.visibility_mi < 3.0 {
            gap += (3.0 - effects.visibility_mi) * 0.5;
        }
        6.0f64.min(chosen.max(gap))
    }

    /// `_acc_weather_gap_text()`, with the quiet rungs' short form.
    pub fn acc_weather_gap_text(&self) -> Option<SpokenMessage> {
        let effects = self.trip.weather.effects();
        if effects.grip < 0.9 {
            return Some(SpokenMessage::new("Wet roads, longer gap."));
        }
        if effects.visibility_mi < 3.0 {
            return Some(SpokenMessage::new("Low visibility, longer gap."));
        }
        None
    }

    /// Distance ACC needs to ease down to a specific lower limit.
    ///
    /// Everything here runs on the REAL clock. The truck's physics steps in
    /// real seconds and only the distance it covers is multiplied by
    /// `effective_time_scale` (`Trip::update`), so the comfort deceleration
    /// and the working setpoint's walk at `CRUISE_ACCEL_MPH_PER_S` both cost
    /// `effective_time_scale` times the road under compression. So does the
    /// quarter-mile margin, which is the time the brake trim needs to taper
    /// onto the number. Sized on physics and margin at real time, cruise at
    /// ten times reached a drop from 55 to 35 in the rain at 42, and the
    /// overspeed warning fired for a speed cruise itself had chosen (agent
    /// drive into Wausau on WI-29, 2026-10-27). The same real-seconds
    /// conversion the "drops to X" pacenote uses
    /// (`Trip::limit_drop_warning_lead_mi`).
    pub fn acc_limit_lookahead_mi(&self, speed_mph: f64, target_mph: f64) -> f64 {
        let speed_mps = 0.0f64.max(speed_mph * 0.44704);
        let target_mps = 0.0f64.max(target_mph * 0.44704);
        if target_mps >= speed_mps {
            return ACC_LIMIT_LOOKAHEAD_MIN_MI;
        }
        let scale = self.trip.effective_time_scale().max(1.0);
        let braking_m = (speed_mps * speed_mps - target_mps * target_mps)
            / (2.0 * ACC_LIMIT_COMFORT_DECEL_MPS2);
        let braking_mi = 0.0f64.max(braking_m / 1609.344) * scale;
        let ramp_real_s = (speed_mph - target_mph) / CRUISE_ACCEL_MPH_PER_S;
        let mean_mph = (speed_mph + target_mph) / 2.0;
        let ramp_mi = ramp_real_s * mean_mph * scale / 3600.0;
        let margin_mi = ACC_LIMIT_LOOKAHEAD_MIN_MI * scale;
        ACC_LIMIT_LOOKAHEAD_MIN_MI.max(
            self.acc_limit_lookahead_max_mi()
                .min(braking_mi.max(ramp_mi) + margin_mi),
        )
    }

    /// How far ahead the posted-limit scan looks.
    ///
    /// The real-time ceiling opens up with compression so the ramp distance
    /// above can fit, but never past the spoken limit warning's own ceiling:
    /// cruise must not ease for a number the driver has not been told about.
    pub fn acc_limit_lookahead_max_mi(&self) -> f64 {
        let scale = self.trip.effective_time_scale().max(1.0);
        LIMIT_WARNING_MAX_LEAD_MI.min(ACC_LIMIT_LOOKAHEAD_MAX_MI * scale)
    }

    /// Lowest posted limit close enough that ACC should start slowing now.
    pub fn acc_posted_limit_ahead(&mut self, ctx: &mut GameContext) -> (f64, Option<String>) {
        let start = self.trip.position_mi;
        let end = self
            .trip
            .total_miles()
            .min(start + self.acc_limit_lookahead_max_mi());
        let (under_wheels, under_reason) = self.trip.speed_limit_at(start);
        let (mut lowest_limit, mut lowest_reason) = (under_wheels, under_reason);
        // Keep aiming at a drop already being slowed for. The window below
        // is a braking distance sized from the truck's speed, so it
        // retracts as cruise slows: without this the drop fell back out of
        // sight, the cap lifted, and cruise wound the truck up again --
        // then found the drop again a moment later (see `acc_limit_hold`).
        let mut lowest_at = start;
        match self.acc_limit_hold.clone() {
            Some((at_mi, limit, reason)) if start < at_mi && limit < lowest_limit => {
                lowest_limit = limit;
                lowest_reason = reason;
                lowest_at = at_mi;
            }
            Some(_) => self.acc_limit_hold = None,
            None => {}
        }
        let speed = self.trip.truck.speed_mph();
        let mut probe = start + ACC_LIMIT_LOOKAHEAD_STEP_MI;
        while probe <= end + 1e-6 {
            let (limit, reason) = self.trip.speed_limit_at(probe);
            let cap_mph = limit + ACC_LIMIT_OFFSET_MPH;
            let braking_mi = self.acc_limit_lookahead_mi(speed, cap_mph);
            if limit < lowest_limit && probe - start <= braking_mi {
                lowest_limit = limit;
                lowest_reason = reason;
                lowest_at = probe;
            }
            probe += ACC_LIMIT_LOOKAHEAD_STEP_MI;
        }
        if lowest_limit < under_wheels {
            self.acc_limit_hold = Some((lowest_at, lowest_limit, lowest_reason.clone()));
        }
        if let Some(restricted) = self.restricted_zone_limit_ahead(ctx) {
            if restricted.0 <= lowest_limit {
                return (restricted.0, Some(restricted.1));
            }
        }
        (lowest_limit, lowest_reason)
    }

    /// Grade every tenth of a mile over the road ahead.
    ///
    /// Real predictive cruise plans against a stored road profile a mile or
    /// two out (Volvo I-See, Detroit Intelligent Powertrain Management). The
    /// baked grade segments are the same thing at the same resolution -- a
    /// median half a mile, ninety-odd segments a leg -- so the preview is a
    /// straight read of data the trip already carries, no new bake.
    pub fn grade_samples(&self, distance_mi: f64) -> Vec<f64> {
        let start = self.trip.position_mi;
        let end = self.trip.total_miles().min(start + distance_mi);
        let mut samples = Vec::new();
        let mut probe = start + PCC_PREVIEW_STEP_MI;
        while probe <= end + 1e-6 {
            samples.push(self.trip.grade_at(probe));
            probe += PCC_PREVIEW_STEP_MI;
        }
        samples
    }

    /// Mean grade over the road ahead, or 0.0 with nothing to read.
    ///
    /// The crest test uses this on a short horizon: near the top, the road
    /// just ahead has already gone flat. Judged on the full preview instead,
    /// a three-mile pull read as cresting from a mile and a half out and the
    /// truck stopped recovering for half the hill (bench, 2026-07-25).
    pub fn grade_preview(&self, distance_mi: f64) -> f64 {
        let samples = self.grade_samples(distance_mi);
        if samples.is_empty() {
            0.0
        } else {
            samples.iter().sum::<f64>() / samples.len() as f64
        }
    }

    /// Steepest sustained climb and descent inside the preview.
    ///
    /// Windowed rather than averaged over the whole preview: a half-mile four
    /// percent hill inside a mile and a half of otherwise flat road averages
    /// out to nothing, and short hills are exactly where banked momentum pays
    /// -- long enough to hurt, short enough that speed carried in still
    /// reaches the top (bench, 2026-07-25: averaging skipped the half-mile
    /// hills entirely). A window rather than a bare maximum so a single
    /// tenth-mile spike is not mistaken for a grade.
    pub fn grade_extremes_ahead(&self) -> (f64, f64) {
        let samples = self.grade_samples(PCC_PREVIEW_MI);
        let window = 1.max((PCC_GRADE_WINDOW_MI / PCC_PREVIEW_STEP_MI).round() as usize);
        if samples.len() < window {
            if samples.is_empty() {
                return (0.0, 0.0);
            }
            let max = samples.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            let min = samples.iter().copied().fold(f64::INFINITY, f64::min);
            return (max, min);
        }
        let means = window_means(&samples, window);
        let max = means.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let min = means.iter().copied().fold(f64::INFINITY, f64::min);
        (max, min)
    }

    /// The first sustained grade inside the preview, and how far off it is.
    ///
    /// The same windowed read the preview plans against, so the G key and the
    /// preview cue describe one road. Predictive cruise banks momentum from
    /// one and a half percent up and the steep advisory only speaks at three,
    /// so the truck could say it was building speed for the grade ahead while
    /// G answered that nothing steep was coming for fifteen miles -- both
    /// true, and together they read as broken (tester report, 2026-08-15).
    pub fn preview_grade_ahead(&self) -> Option<(f64, f64)> {
        let samples = self.grade_samples(PCC_PREVIEW_MI);
        let window = 1.max((PCC_GRADE_WINDOW_MI / PCC_PREVIEW_STEP_MI).round() as usize);
        if samples.len() < window {
            return None;
        }
        let means = window_means(&samples, window);
        // The steepest window, not the first one over the bar: on the run into
        // Asheville the first was a 1.5 percent lift a mile out and the cue was
        // already building for the 3.7 percent pull behind it, so naming the
        // first put two different numbers on one hill (sweep, 2026-08-15).
        let peak = (0..means.len())
            .max_by(|a, b| means[*a].abs().total_cmp(&means[*b].abs()))
            .expect("a non-empty window list");
        if means[peak].abs() < PCC_GRADE_MIN {
            return None;
        }
        let sign = if means[peak] > 0.0 { 1.0 } else { -1.0 };
        let mut start = peak;
        while start > 0 && means[start - 1] * sign >= PCC_GRADE_MIN {
            start -= 1;
        }
        Some((means[peak], (start + 1) as f64 * PCC_PREVIEW_STEP_MI))
    }

    /// Speed to add or give up for the grade the truck is about to reach.
    ///
    /// Three behaviors, all of them what a real predictive system does:
    ///
    /// Bank momentum before a climb. Entering a pull two or three mph faster
    /// means carrying more speed the whole way up and holding a taller gear
    /// for longer -- the truck arrives at the top sooner having done the same
    /// work, instead of meeting the hill at exactly the set speed and
    /// immediately falling behind it.
    ///
    /// Give up the last few mph at a crest. Holding full throttle to the top
    /// of a pull buys seconds and costs a downshift that upshifts again over
    /// the summit; letting it sag inside a band leaves the truck in the gear
    /// it is already turning.
    ///
    /// Do not accelerate into a descent cruise is about to brake away. Speed
    /// added just before a downgrade comes straight back out through the
    /// retarder and the drums, which in this truck means real heat and real
    /// air -- so the preview shaves instead of adding.
    /// The bias and which preview case produced it: `Building` banks
    /// momentum for a climb, `EasingForDescent` shaves before a downgrade
    /// cruise would otherwise brake away, `HoldingOverCrest` stops reaching
    /// for speed the summit hands back. `None` means flat reading or a
    /// nearer owner of the speed.
    pub fn predictive_cruise_bias(&self, ctx: &GameContext, target_mph: f64) -> (f64, PccReason) {
        if !ctx.settings.predictive_cruise {
            return (0.0, PccReason::None);
        }
        // Following a lead, capped for a ramp or a bend, or already fighting a
        // lower posted limit: something closer than the horizon owns the speed.
        if self.acc_following || self.cruise_exit_mph.is_some() {
            return (0.0, PccReason::None);
        }
        let (climb_ahead, descent_ahead) = self.grade_extremes_ahead();
        let here = self.trip.truck.grade;
        let speed = self.trip.truck.speed_mph();
        if descent_ahead <= -PCC_GRADE_MIN && climb_ahead < PCC_GRADE_MIN {
            // A downgrade is coming and no pull stands between here and it.
            // Shave in proportion to how steep, so the truck rolls onto the
            // grade at or under the set speed instead of arriving over it and
            // spending the retarder to get back down.
            return (
                -PCC_DESCENT_SHAVE_MPH.min(PCC_DESCENT_SHAVE_MPH * (-descent_ahead / 0.05)),
                PccReason::EasingForDescent(-descent_ahead),
            );
        }
        if here >= PCC_GRADE_MIN && self.grade_preview(PCC_CREST_WINDOW_MI) < PCC_GRADE_MIN {
            // On a pull whose top is inside the crest window. Stop reaching for
            // speed the summit is about to hand back for nothing: hold what
            // the truck has rather than spending the last of the climb at full
            // throttle recovering it, and taking a downshift to do it.
            //
            // It asks the truck to hold, never to slow: the bias can only ever
            // bring the target down to the speed already on the clock. An
            // earlier cut of this gave up a flat four mph and cost a 2 percent
            // pull three miles an hour it had been holding comfortably (bench,
            // 2026-07-25) -- the allowance is a ceiling on the giveaway, not
            // the giveaway itself.
            if speed < target_mph - 0.5 {
                return (
                    (-PCC_CREST_SAG_MPH).max(speed - target_mph),
                    PccReason::HoldingOverCrest,
                );
            }
            return (0.0, PccReason::HoldingOverCrest);
        }
        if here < PCC_GRADE_MIN && climb_ahead >= PCC_GRADE_MIN {
            // Level ground now, a pull inside the preview: bank what the grade
            // is about to take. Scaled by the climb, capped so cruise never
            // reads as running away with the truck.
            return (
                PCC_PREBUILD_MPH.min(PCC_PREBUILD_MPH * (climb_ahead / 0.04)),
                PccReason::Building,
            );
        }
        (0.0, PccReason::None)
    }

    /// Track what the preview is doing, in silence.
    ///
    /// It used to name each hill ("Easing off for a 1.7 percent downhill
    /// grade."); players heard that as chatter about how cruise works, which
    /// the manual covers (player report and owner, 2026-10-14). The phase is
    /// still tracked for the radio and the badge.
    /// `delta` is the change the target actually took after the caps
    /// clamped it: a shave that the posted cap neutralised is its own phase.
    /// `reason` names which preview case produced it.
    pub fn track_predictive_cruise(&mut self, delta: f64, reason: PccReason) {
        let phase = match reason {
            PccReason::Building if delta > 0.5 => "building",
            PccReason::EasingForDescent(_) if delta <= -0.5 => "easing",
            PccReason::HoldingOverCrest => "holding",
            PccReason::EasingForDescent(_) => "easing-capped",
            PccReason::Building => "building-capped",
            PccReason::None => "",
        };
        if phase != self.pcc_phase {
            self.pcc_phase = phase.to_string();
        }
    }

    /// Hold speed when clear, and follow slower modeled traffic when present.
    pub fn update_cruise(
        &mut self,
        ctx: &mut GameContext,
        dt: f64,
        braking: bool,
        accelerating: bool,
        clutch_disengaged: bool,
    ) {
        if self.cruise_mph.is_none() {
            return;
        }
        // A limp-mode cap under the set speed is invisible from the seat: the
        // truck simply never reaches its number. Name it, once per engagement.
        self.announce_limp_cruise_cap(ctx);
        self.acc_follow_cue_s = 0.0f64.max(self.acc_follow_cue_s - dt);
        if self.update_descent_control(ctx, dt, braking) {
            return;
        }
        let t = &self.trip.truck;
        if braking || t.emergency_brake || t.air_brakes_holding() || !t.engine_on || t.stalled {
            self.cancel_cruise(ctx, false);
            // ROUTE, not the ambient default: the automation just released the
            // throttle (automation-handoff sweep, 2026-08-20, the deferred
            // 2026-08-15 audit).
            self.say_route_confirmation(ctx, "Cruise off.");
            return;
        }
        let position = self.trip.position_mi;
        let (limit, zone_reason) = self.trip.speed_limit_at(position);
        // A highway work zone or heavy traffic stays on cruise, capped at its
        // limit below. Handed to the keeper partway down a grade, it left
        // descent control behind and the co-driver crawling (issue #518).
        let zone_reason = zone_reason.filter(|reason| !is_highway_zone_reason(reason));
        if let Some(zone_reason) = zone_reason {
            if self.speed_control_armed && ctx.settings.speed_keeper {
                self.cancel_cruise(ctx, true);
                self.engage_keeper(ctx, limit, &zone_reason, Some(limit), false);
                // ROUTE, not the ambient default: cruise handing off to the
                // keeper is the automation changing which system holds the
                // throttle (automation-handoff sweep, 2026-08-20, the deferred
                // 2026-08-15 audit).
                let held = ctx.settings.speed_text(self.keeper_mph.unwrap_or(0.0));
                // A street is not a zone and the yard is the yard
                // (`spoken_zone`): "Facility Access Road zone" named a city
                // street (live drive into Abilene, 2026-09-24).
                let message = match ff_core::sim::trip::spoken_zone(&zone_reason) {
                    Some(zone) if zone.ends_with(" zone") => format!(
                        "{} zone. Speed keeper holding {held}.",
                        title_case(&zone_reason)
                    ),
                    _ => super::cruise::keeper_holding_line(&held, &zone_reason),
                };
                let terse = format!(
                    "Speed keeper holding {}.",
                    ctx.settings.speed_value(self.keeper_mph.unwrap_or(0.0))
                );
                self.say_route_confirmation(ctx, SpokenMessage::with_terse(message, terse));
                return;
            }
        }
        if accelerating {
            return; // manual override; cruise resumes when the key lifts
        }
        if clutch_disengaged {
            // Clutch in / mid-shift: driveline is open, so any applied throttle
            // only free-revs the engine. Cut throttle to idle and hold the
            // integrator; the applied throttle ramps back up from zero once the
            // clutch engages again.
            self.trip.truck.throttle = 0.0;
            self.cruise_applied = 0.0;
            // A snub under way stays on through the shift. Let off for the
            // second the box takes to find a gear, a truck on a steep grade
            // gained two or three miles an hour a shift and landed the lower
            // gear past its governor (bench, twelve percent, 2026-09-24).
            // On a manual it lets go on the same edge it would in gear: the
            // clutch or neutral lasts as long as the driver holds it, and the
            // snub rode it down to a stop. An automatic's shift is a second,
            // and its snub may be guarding revs rather than speed, so it rides
            // the shift out.
            let held = self
                .cruise_held_mph
                .or(self.cruise_mph)
                .unwrap_or(CRUISE_MIN_MPH);
            if !self.trip.truck.transmission.automatic
                && self.trip.truck.speed_mph() <= held - CRUISE_SNUB_UNDER_MPH
            {
                self.cruise_snubbing = false;
            }
            if self.cruise_snubbing {
                let weather_brake: f64 = if self.trip.weather.effects().grip < 0.7 {
                    0.45
                } else {
                    0.65
                };
                let snub = weather_brake.min(CRUISE_SNUB_BRAKE);
                self.trip.truck.brake = self.trip.truck.brake.max(snub);
            }
            return;
        }
        self.run_cruise_loop(ctx, dt);
    }

    /// The speed loop proper: caps, the lead, the pedal.
    fn run_cruise_loop(&mut self, ctx: &mut GameContext, dt: f64) {
        let set_mph = self.cruise_mph.expect("checked by the caller");
        // Ease the working setpoint toward the set speed at a bounded rate, in
        // both directions, and chase that rather than the set speed itself. A
        // resume to a far target climbs a couple of mph a second instead of
        // putting the whole error on the pedal at once; a drop in the set speed
        // backs off just as gently. Everything below still caps this working
        // target down for a lead, a ramp, a curve, a limit, or a grade.
        if self.cruise_working_mph.is_none() {
            let speed = self.trip.truck.speed_mph();
            self.cruise_working_mph = Some(CRUISE_MIN_MPH.max(set_mph.min(speed)));
        }
        let step = CRUISE_ACCEL_MPH_PER_S * dt;
        let working = self.cruise_working_mph.expect("set above");
        self.cruise_working_mph = Some(if working < set_mph {
            set_mph.min(working + step)
        } else if working > set_mph {
            set_mph.max(working - step)
        } else {
            working
        });
        let mut target_mph = self.cruise_working_mph.expect("set above");
        // Lane keeping easing off under a vehicle pacing the truck in the
        // right lane, so it can drop in behind it. Silent: the move back
        // that follows is the report.
        if let Some(ease) = self.pass_return_ease_mph {
            target_mph = target_mph.min(ease);
        }
        let exit_cap = self.ramp_approach_cap_mph();
        let exit_capped = exit_cap.is_some_and(|cap| cap < target_mph);
        if exit_capped {
            target_mph = exit_cap.expect("checked above");
        }
        // A pacenote capped cruise for a bend: hold the advisory until the
        // curve's footprint is behind the truck, then climb back silently --
        // announcing every release would chant through a curve cluster.
        if self
            .cruise_curve_end_mi
            .is_some_and(|end| self.trip.position_mi > end)
        {
            self.cruise_curve_mph = None;
            self.cruise_curve_end_mi = None;
        }
        // And whatever curve assistance's servo is holding a bend at. The
        // cap above comes only from a call that found cruise set well over
        // the sign; a load whose own number sits under the sign, or a bend
        // the servo armed without a call, left cruise throttling to its set
        // speed against the servo's brake the whole way through the chain
        // (bend sweep, Siskiyou, 2026-09-24: half the pedal against a fifth
        // of the brake, for a mile).
        let servo_mph = self
            .curve_servo
            .as_ref()
            .filter(|servo| self.trip.position_mi <= servo.hold_to_mi)
            .map(|servo| servo.target_mph);
        let curve_mph = self
            .cruise_curve_mph
            .into_iter()
            .chain(servo_mph)
            .reduce(f64::min);
        let curve_capped = curve_mph.is_some_and(|curve| curve < target_mph);
        if let Some(curve) = curve_mph.filter(|_| curve_capped) {
            target_mph = curve;
        }
        // Descent control's ceilings, which last exactly as long as the grade:
        // a brake's capture, interactive mode's own, and the safe descent
        // speed of the hill under and just ahead of the truck.
        for descent in [self.cruise_descent_mph, self.descent_safe_mph]
            .into_iter()
            .flatten()
        {
            target_mph = target_mph.min(descent);
        }
        // Predictive ACC: never carry the driver past the posted limit. With real
        // OSM limits baked per leg, a held set speed would otherwise sail through
        // urban drops and corridor limit changes straight into speeding strikes,
        // tickets, and trooper stops -- all of which now exist. The "Speed limit X"
        // cue still names the number; this cue says cruise is handling it.
        let (posted, limit_reason) = self.acc_posted_limit_ahead(ctx);
        let restricted = limit_reason.as_deref().is_some_and(is_highway_zone_reason);
        let cap_mph = if restricted {
            posted
        } else {
            posted + ACC_LIMIT_OFFSET_MPH
        };
        // Measured against the working target, not the set speed, so this cap
        // can only ever lower it. Against the set speed it overwrote a stricter
        // ramp cap: cruise announced it was easing to 45 for the exit and then
        // held the 60 the posted limit allowed, missing the exit and costing
        // the driver a twenty-minute loop back.
        let limit_capped = cap_mph < target_mph;
        // Which limit, and whether it is in force yet: "holding 65 for the
        // lower limit, set 70" in a 70 read as cruise ignoring the road, when
        // it was slowing for a 60 sign ahead (owner, 2026-10-25).
        let limit_reason_text = match limit_reason.as_deref() {
            Some("construction" | "construction merge") if restricted => {
                "through the construction zone".to_string()
            }
            Some(reason) if restricted => format!("for the {reason}"),
            _ if posted < self.trip.speed_limit_at(self.trip.position_mi).0 => {
                format!("for the {} limit ahead", ctx.settings.speed_value(posted))
            }
            _ => format!("for the {} limit", ctx.settings.speed_value(posted)),
        };
        if limit_capped {
            // Take the lower of the two caps. A posted limit above ramp speed
            // must not undo an armed exit's cap and send the truck past its
            // ramp at the corridor limit.
            target_mph = target_mph.min(cap_mph);
            // Once per cap, not once per frame it happens to be in force. The
            // advance-warning window scales with speed, so as cruise slows for
            // a work zone the zone slips out of the window and back in, and a
            // plain on/off latch recited the same easing line all the way to
            // the barrels.
            if self
                .acc_limit_cap_said
                .is_none_or(|said| cap_mph < said - 0.5)
            {
                self.acc_limit_cap_said = Some(cap_mph);
                // The cause only where the speed limit cue does not already
                // carry it: a plain posted drop is its own announcement.
                let reason = match limit_reason.as_deref() {
                    Some("construction") => Some("Construction zone ahead"),
                    Some("heavy traffic") => Some("Heavy traffic ahead"),
                    _ => None,
                };
                // ROUTE, not the ambient default. An assist saying it is
                // about to change how fast the truck is going is a
                // consequence, not colour -- the same reasoning that moved
                // the toll charge off the ambient channel. As AMBIENT this
                // was droppable as stale chatter, and it WAS dropped:
                // tester Darren's I-75 log, 2026-08-18, "[pacer] stale
                // ambient dropped: Construction zone ahead; adaptive cruise
                // easing to 45 miles per hour" -- seventeen seconds before
                // a trooper stopped him over the gap the easing was
                // closing. The truck slowed itself and never said why.
                let eased = ctx.settings.speed_value(cap_mph);
                let message = match reason {
                    Some(reason) => format!("{reason}, cruise easing to {eased}."),
                    None => format!("Cruise easing to {eased}."),
                };
                self.say_route_confirmation(ctx, message);
                // This line already named the number for a plain posted-limit
                // drop; the arrival "Speed limit reduced to X" would otherwise
                // repeat it a moment (or, under compression, an instant)
                // later -- the owner's live-playtest complaint (2026-08-12).
                if !restricted {
                    self.trip.note_limit_preannounced(cap_mph);
                }
            }
        } else if cap_mph >= set_mph {
            // Back out on the open road at the set speed: the next drop is
            // news again.
            self.acc_limit_cap_said = None;
        }
        self.acc_limit_capped = limit_capped;
        // The weather's safe speed, enforced like any other road fact. The
        // number was computed and SPOKEN as guidance since live weather
        // shipped, and consumed by nothing: cruise held a set seventy
        // through a thunderstorm until the driver tapped it down by hand --
        // which is what the owner's own storm playtest was actually showing
        // (2026-08-20, Brandon's suggestion). Same once-per-cap latch as
        // the posted limit above; releases as the weather lifts.
        // Only weather that actually degrades the road caps: grip under 1.0
        // or meaningfully shortened sight lines. CLEAR and CLOUDY carry a 70
        // in safe_speed_mph as a GUIDANCE number for the status keys, and
        // capping at it made every 75-and-up western limit unreachable in
        // perfect weather (caught by the full sweep: a resume test stalled
        // at exactly 70.0).
        let effects = self.trip.weather.effects();
        let adverse = effects.is_adverse();
        let safe_mph = effects.safe_speed_mph;
        if adverse && safe_mph < target_mph {
            target_mph = safe_mph;
            if self
                .acc_weather_cap_said
                .is_none_or(|said| safe_mph < said - 0.5)
            {
                self.acc_weather_cap_said = Some(safe_mph);
                let kind = self.trip.weather.current.value();
                self.say_route_confirmation(
                    ctx,
                    format!(
                        "{}, cruise easing to {}.",
                        capitalize_first(kind),
                        ctx.settings.speed_value(safe_mph)
                    ),
                );
            }
        } else if safe_mph >= set_mph {
            // Weather no longer binds at this set speed: the next front is
            // news again.
            self.acc_weather_cap_said = None;
        }
        // The preview goes on last so it can only ever move the number the
        // caps already agreed on, and it is clamped against the posted cap:
        // banking momentum for a hill must never bank it past the limit.
        let (bias, reason) = self.predictive_cruise_bias(ctx, target_mph);
        // The cue hears the clamped delta, not the raw bias: a shave the cap
        // cancelled is not easing and stays silent.
        let mut delta = 0.0;
        if bias != 0.0 {
            let applied = CRUISE_MIN_MPH.max((target_mph + bias).min(cap_mph));
            delta = applied - target_mph;
            target_mph = applied;
        }
        self.track_predictive_cruise(delta, reason);
        let context = self.trip.traffic_context();
        let mut following = false;
        if let Some(context) = context.as_ref() {
            let desired_gap = self.acc_gap_seconds(ctx);
            let reason = self.acc_weather_gap_text();
            if let Some(reason) = reason {
                if !self.acc_weather_gap_said && context.gap_seconds() <= desired_gap + 1.5 {
                    self.acc_weather_gap_said = true;
                    self.say_route_confirmation(ctx, reason);
                }
            }
            let lead_mph = context.lead.speed_mph;
            if lead_mph <= 5.0
                && !ctx.settings.stop_and_go_assist
                && context.closing_mph > 0.5
                && context.gap_mi / context.closing_mph * 3600.0 <= ACC_STOPPED_CANCEL_S
            {
                self.cancel_cruise(ctx, false);
                // Handing the truck back is the least droppable line the
                // assist has: a driver who does not hear it believes the
                // cruise is still holding the gap.
                let mut opts = SayEvent::queued().priority(EventPriority::Important);
                opts.category = Some(SpeechCategory::Safety);
                ctx.say_event_with("Stopped traffic ahead; adaptive cruise canceled.", opts);
                return;
            }
            // Approach control: a slower lead constrains the target only once the
            // gap actually matters. Distance beyond the desired gap converts to
            // allowed closing speed at a gentle planned deceleration, so the truck
            // closes smoothly and settles onto the lead's speed at the desired
            // gap. A slower vehicle merely existing in the traffic bubble must
            // not drag the target down: matching a distant lead's speed parks the
            // truck at the bubble edge, where the lead drifts in and out of range
            // and the follow cue re-announces itself forever.
            let headway_mi = desired_gap * lead_mph.max(5.0) / 3600.0;
            let approach_m = 0.0f64.max(context.gap_mi - headway_mi) * 1609.344;
            let closing_allowed_mph =
                (2.0 * ACC_FOLLOW_DECEL_MPS2 * approach_m).sqrt() * MPH_PER_MPS;
            let follow_mph = lead_mph + closing_allowed_mph;
            if follow_mph < target_mph - 0.5 || context.gap_seconds() <= desired_gap + 1.0 {
                target_mph = target_mph.min(follow_mph);
                following = true;
            }
        }
        if following && !self.acc_following && self.acc_follow_cue_s <= 0.0 {
            self.acc_follow_cue_s = ACC_FOLLOW_CUE_COOLDOWN_S;
            ctx.play_event_cue(Some(SpeechCategory::Traffic), "ui/notify", 0.55, 0.0);
            // Settling in behind a slower vehicle is the assist doing its
            // job, and the notify tone marks it: words at standard only.
            self.say_route_line(ctx, "Traffic ahead, slowing.", SpeechCategory::Traffic);
        }
        self.acc_following = following;
        // Publish what cruise is really holding, for the status keys. The
        // engage and resume line has named the number the truck will hold
        // since 2026-08-20 (Brandon: cruise said "resuming at 70" and held 23
        // through a zone); Space and the status screen still read out the set
        // speed. Owner's own session log, New York, 2026-08-23, one second
        // apart: "Adaptive cruise resuming at 33 miles per hour for the ramp"
        // and then "44 miles per hour, ... adaptive cruise set at 80 miles per
        // hour". A driver checking mid-ramp was answered with a number nothing
        // on the road was going to allow.
        //
        // Recorded here rather than re-derived in the readout because this is
        // the only place that knows which cap won, and because asking again
        // from a key press would re-run look-aheads that latch state.
        self.cruise_held_mph = Some(target_mph);
        self.cruise_held_reason = if following {
            "for the traffic ahead".to_string()
        } else if exit_capped {
            "for the exit".to_string()
        } else if curve_capped {
            "for the bend".to_string()
        } else if limit_capped {
            limit_reason_text
        } else if [self.cruise_descent_mph, self.descent_safe_mph]
            .into_iter()
            .flatten()
            .any(|descent| (descent - target_mph).abs() < 0.01)
        {
            "for the grade".to_string()
        } else {
            // The weather cap, or nothing at all. Holding a number without
            // naming a reason is still true; inventing one would not be.
            String::new()
        };
        let error = target_mph - self.trip.truck.speed_mph();
        // Feed-forward first: the truck's own physics knows what throttle
        // balances the grade under the wheels, so cruise answers a hill as it
        // arrives. P and I only trim from there.
        let mut hold = self.trip.truck.hold_throttle();
        let mut trim = (self.cruise_trim + error * CRUISE_I_GAIN * dt)
            .clamp(-CRUISE_TRIM_LIMIT, CRUISE_TRIM_LIMIT);
        if error < 0.0 {
            // Over the target, cruise comes off the fuel: feeding the grade-hold
            // value into a truck that also needs to lose speed is a truck
            // fighting itself, and the speeding-strike grace only forgives a
            // cruise genuinely off the throttle. Eased out across a band rather
            // than switched -- a hard cut at the boundary chattered the pedal on
            // and off at steady state, and the engine voice shows every bit of
            // that.
            hold *= 0.0f64.max(1.0 + error / CRUISE_COAST_MPH);
            if error <= -CRUISE_COAST_MPH {
                trim = trim.min(0.0);
            }
        }
        let mut demand = hold + error * CRUISE_P_GAIN + trim;
        // Off the throttle as the engine nears the governor. On a downgrade
        // gravity does the accelerating, and cruise adding fuel into a coupled
        // RPM already climbing toward redline is what over-revved the engine
        // and charged wear during the automatic box's between-shift hold. Taper
        // demand to nothing across the top of the RPM range so descent control
        // and the retarder own the grade and cruise simply lifts -- it never
        // fights the retarder, it just stops feeding the over-rev.
        let ceiling_rpm = self.trip.truck.specs.max_rpm;
        let band = ceiling_rpm * CRUISE_RPM_CEILING_BAND;
        let ceiling_factor = if band <= 0.0 {
            1.0
        } else {
            ((ceiling_rpm - self.trip.truck.coupled_rpm(None)) / band).clamp(0.0, 1.0)
        };
        demand *= ceiling_factor;
        // Curve assistance on the brake for a bend: cruise gives no more than
        // what holds the grade, and lets go of any trim it wound up. Both hold
        // the same number, the servo from above and cruise from below, and on
        // a downgrade the servo holds the hill a band under it -- so the two
        // met in the middle, half the throttle against a sixth of the brake
        // for a mile down US-50's 4.7 percent with a half-full tank, the pedal
        // flickering on every breath and the tanks at 48 psi by the bottom
        // (bend sweep, 2026-09-24). Not a chop to nothing: on a climb that
        // threw a half-full tank's liquid against the head every time the
        // servo touched the pedal.
        let servo_braking = self.curve_servo.as_ref().is_some_and(|s| s.brake > 0.0);
        if servo_braking {
            demand = demand.min(hold);
            self.cruise_trim = self.cruise_trim.min(0.0);
        }
        // No fuel against a retarder that is holding a downgrade. Any
        // throttle cuts the jake, so cruise trimming up to its number a mile
        // an hour under it switched the engine brake off and on every few
        // seconds -- the growl dropping out and coming back, which is what
        // the owner heard looping. Under the number by more than the
        // retarder's own release band, the stage steps down first (see
        // `hold_cruise_from_above`); only a truck with no stage left is fed.
        //
        // Nor above a steep hill's safe descent speed: a truck a mile an hour
        // under it on seven percent is about to be over it on gravity alone,
        // and the fuel only bought an upshift out of the retarder's gear.
        //
        // Nor anywhere descent control holds a grade that pulls the truck
        // along by itself: gravity is already bringing it up to its number,
        // and fuel there bought an upshift over the crest that the retarder
        // took straight back a few seconds later (bench, 2026-09-24).
        let gravity_pulls = self.descent_control_active && self.trip.truck.resistance_force() < 0.0;
        let retarding_grade = gravity_pulls
            || ((self.trip.truck.engine_brake_stage > 0 || self.descent_safe_mph.is_some())
                && self.on_downgrade()
                && error < CRUISE_JAKE_UNDER_MPH);
        if retarding_grade {
            demand = 0.0;
            self.cruise_trim = self.cruise_trim.min(0.0);
        }
        // Anti-windup: a grade the engine cannot pull, or a downgrade gravity
        // owns, pins the pedal at one end for as long as it lasts. Integrating
        // through that buries the trim at its limit, and the truck then sags or
        // overshoots for seconds after the road levels out while it unwinds.
        // Only take the new trim when it can still move the pedal -- and the RPM
        // ceiling holding the pedal down counts as pinned just as much as the
        // floor or the roof does.
        let saturated = servo_braking
            || retarding_grade
            || (demand <= 0.0 && error < 0.0)
            || (demand >= 1.0 && error > 0.0)
            || (ceiling_factor < 1.0 && error > 0.0);
        if !saturated {
            self.cruise_trim = trim;
        }
        self.cruise_throttle = demand.clamp(0.0, 1.0);
        // Ramp the applied throttle up to the held integrator value rather than
        // snapping, so cruise eases back in after a clutch release; drops (traffic
        // or a lower limit) still apply immediately. On a steady frame the applied
        // throttle already equals _cruise_throttle, so this holds as before.
        if self.cruise_throttle > self.cruise_applied {
            let load_fraction = (self.trip.truck.cargo_kg / REFERENCE_CARGO_KG).clamp(0.0, 1.0);
            let mut recovery_rate = 0.7 + 0.8 * (1.0 - load_fraction);
            recovery_rate += 0.6f64.min(0.0f64.max(error) / 15.0);
            self.cruise_applied = self
                .cruise_throttle
                .min(self.cruise_applied + dt * recovery_rate);
        } else {
            self.cruise_applied = self.cruise_throttle;
        }
        self.trip.truck.throttle = self.cruise_applied;
        self.say_cruise_out_of_truck(ctx, dt, error);
        // Every reason the working target sits below the set speed except one
        // is a target speed to arrive at, and the drums are what arrive: a
        // lead vehicle, an armed exit's ramp cap, a lower posted limit or a
        // construction zone, and now a bend's advisory. The exception is a
        // grade, which is sustained speed control and the retarder's own job
        // -- so a bend on a downgrade still retards, because that is the
        // grade's doing and not the corner's. See _on_downgrade for the rule
        // and _update_lane for the same rule in the curve assist.
        //
        // A lower posted limit on a downgrade is the grade's too. Held as a
        // target to arrive at, the cap for "limit plus five" put a tenth of
        // the drums on a 76,000 lb truck for three miles of 4.9 and 5.8
        // percent with the retarder never raised, and the shoes climbed past
        // 300 C (the owner's run into Denver on the bench, 2026-09-24).
        let closing =
            following || exit_capped || ((limit_capped || curve_capped) && !self.on_downgrade());
        self.hold_cruise_from_above(ctx, dt, error, closing);
    }

    /// Say plainly when the hill has beaten cruise.
    ///
    /// The descent side has said "cannot hold this grade" for a while; the
    /// climb side said nothing at all, so the truck just quietly sank. A
    /// sighted driver reads that off the tach in a second. A blind driver has
    /// the engine note and the downshifts, which say the truck is working but
    /// not that it is losing -- and losing is the part that decides whether to
    /// take it over by hand.
    ///
    /// Only once the pedal is genuinely on the floor and the truck is still
    /// falling past the droop band, so a normal pull that cruise recovers from
    /// on its own stays quiet.
    pub fn say_cruise_out_of_truck(&mut self, ctx: &mut GameContext, dt: f64, error: f64) {
        self.climb_cue_s = 0.0f64.max(self.climb_cue_s - dt);
        // error is target minus speed, so a positive error is the truck
        // sitting below the number cruise is working to. The three ported
        // guards (see CRUISE_GRADE_BEATEN_* in driving_core): a real grade,
        // not mid-shift, and the condition holding rather than one frame.
        let beaten = self.cruise_applied >= CRUISE_FLOORED_THROTTLE
            && self.trip.truck.grade * 100.0 >= CRUISE_GRADE_BEATEN_PCT
            && error > CRUISE_DROOP_MPH;
        if !beaten {
            self.climb_beaten_s = 0.0;
            if error < CRUISE_DROOP_MPH * 0.5 {
                self.climb_cue_said = false; // back on its number: arm again
            }
            return;
        }
        if self.trip.truck.transmission.shifting() {
            return; // an open driveline is no evidence either way; hold the count
        }
        self.climb_beaten_s += dt;
        if self.climb_beaten_s < CRUISE_GRADE_BEATEN_S {
            return;
        }
        if self.climb_cue_said || self.climb_cue_s > 0.0 || self.terse_speech(ctx) {
            return;
        }
        self.climb_cue_said = true;
        self.climb_cue_s = CLIMB_CUE_COOLDOWN_S;
        let held = ctx.settings.speed_text(self.trip.truck.speed_mph());
        let mut opts = SayEvent::queued();
        opts.category = Some(SpeechCategory::Status);
        ctx.say_event_with(format!("Cruise down to {held} on this grade."), opts);
    }

    /// Bring the truck back down to the target: retarder first, drums last.
    ///
    /// Cutting fuel was cruise's whole answer to being over the target, which
    /// works on the flat and fails on every downgrade. Anything gentler than
    /// the descent assist's 2.5 percent trigger got no retarder at all, so
    /// gravity carried the truck past the set speed and simply held it there
    /// -- and the service brake only ever came out while a cap or a lead was
    /// already pulling the target down. Cruise now stages the retarder against
    /// the overspeed rather than leaving it off or pinning it open.
    ///
    /// Closing on a lead, easing down to a lower posted limit, or shedding
    /// speed for a bend or a ramp keeps the old proportional service-brake
    /// trim and no retarder at all. That is deliberate: each of those is a
    /// target speed to arrive at, which wants the precise control only the
    /// drums give, and the jake is a loud device besides -- reaching for it
    /// on every piece of traffic would put a stage change in the player's
    /// ears several times a mile for a job the drums do quietly.
    pub fn hold_cruise_from_above(
        &mut self,
        ctx: &mut GameContext,
        dt: f64,
        error: f64,
        closing: bool,
    ) {
        let over = -error;
        self.cruise_jake_cooldown_s = 0.0f64.max(self.cruise_jake_cooldown_s - dt);
        if closing {
            if over > CRUISE_CLOSE_FEATHER_FROM_MPH {
                let weather_brake: f64 = if self.trip.weather.effects().grip < 0.7 {
                    0.45
                } else {
                    0.65
                };
                // Net of whatever the retarder is already doing. On a downgrade
                // the stage below is kept up on purpose -- dropping it would
                // hand the whole grade to the drums -- but it is already buying
                // real deceleration, and this snub was sized as though it were
                // the only thing slowing the truck. Stacked on a retarder doing
                // real work, the two together cleared the freight's hard-brake
                // line on an ordinary ramp or following-distance close: the
                // same insult to the load as an emergency stop, for ACC just
                // catching up to a lead.
                let jake_decel_mps2 =
                    self.trip.truck.jake_brake_force() / self.trip.truck.gross_mass_kg().max(1.0);
                let jake_fraction = jake_decel_mps2 / assist_full_decel_mps2(&self.trip.truck);
                // Feathered in over the mile an hour below the old 2 mph
                // edge. Switched on at 2 over, the snub started at a
                // fifteenth of the pedal, so a target sliding down ahead of
                // the truck (an exit's glide, a closing lead) held it on the
                // edge and cruise pumped the brake ten times a second: each
                // new application costs air, and the tanks lost the race to
                // the compressor on the mainline (agent drive D, 2026-09-24).
                let feather = (over - CRUISE_CLOSE_FEATHER_FROM_MPH).min(1.0);
                let wanted = feather * weather_brake.min(over / 30.0) - jake_fraction;
                if wanted > 0.0 {
                    self.trip.truck.brake = self.trip.truck.brake.max(wanted);
                }
            }
            // Hand the speed over cleanly: give back a retarder cruise itself
            // raised on the grade that has just run out, rather than letting
            // it ride on into the bend or the queue. On a real downgrade it
            // stays up -- there the retarder is holding the truck, and
            // dropping it puts the whole grade onto the drums. The driver's
            // own jake switch is never touched.
            if self.cruise_jake_stage > 0 && !self.on_downgrade() {
                self.cruise_jake_stage = 0;
                self.trip.truck.engine_brake_stage = 0;
            }
            self.cruise_snubbing = false;
            return;
        }
        // A gear held for a descent has a top speed of its own; past it the
        // retarder answers before the revs guard below needs the drums.
        let over = self
            .held_gear_overspeed_mph()
            .map_or(over, |past_top| over.max(past_top));
        let rpm_now = self.trip.truck.coupled_rpm(None);
        // Cruise reaches for the retarder only where a real one would: the
        // engine-brake stalk has to permit it. Descent control set to off is
        // the driver saying they manage grades themselves, and a real truck's
        // cruise does not flip the stalk on for you. Town no-engine-brake
        // zones close the stalk too (unless a real downgrade exempts them --
        // see driving_engine_brake). The drums below still answer either way,
        // so losing the retarder never costs the ability to hold the speed.
        // Only on a real downgrade -- which is _on_downgrade's own doctrine,
        // written on the predicate and never consulted here: holding a load
        // back on a grade is what the retarder is built for; slowing to a
        // target -- a storm's safe speed, a zone, a lead -- is the drums'
        // job. Without this gate the governor raised the jake wherever
        // overspeed appeared: on flat soaked I-24 for a thunderstorm ease
        // (owner playtest, 2026-08-20), and on UPGRADES, barking away speed
        // that the climb itself was about to eat -- the opposite of what a
        // real driver does with a hill in front of the hood (Brandon,
        // 2026-08-20). The slick-surface question resolves itself: no flat
        // raises means no storm-ease raises, and a retarder already holding
        // a real grade stays up, wet or dry, because dropping it puts the
        // whole hill onto the drums.
        //
        // RAISING and HOLDING are two different questions and used to be one
        // predicate. `on_downgrade` is geometry -- two percent, which is the
        // spoken grade advisory's release edge and was never a braking number
        // -- so cruise reached for the retarder on every shallow dip in the
        // road ("the jake activates on every single descent it seems, even
        // shallow descent like 1-3 percent", owner, 2026-08-24). It comes up
        // now only where `retarder_warranted` says the DRUMS cannot hold the
        // hill: grade against weight against speed, measured on the truck's
        // own brake heat model, which puts the line just past four percent at
        // eighty thousand pounds and past nine percent empty. It stays up
        // until the road is level again, which is the hysteresis that keeps a
        // rolling grade from chattering a loud device.
        let stalk_open =
            ctx.settings.descent_speed_control != "off" && self.assist_jake_allowed(ctx);
        let still_a_grade = stalk_open && self.on_downgrade();
        let may_retard = still_a_grade && self.retarder_warranted();
        let ceiling = JAKE_STAGES.min(0.max(self.auto_jake_max_stage()));
        // The stage is STEPPED, not read off the overspeed. It used to be one
        // stage per mile an hour over, so a truck wobbling a mile an hour
        // either side of its number walked the retarder 3, 0, 2, 1, 2, 1
        // inside half a minute down a 5.8 percent grade (owner's playtest,
        // I-70 into Denver, 2026-09-24) -- and every change is heard. Now:
        //
        // * a hill steep enough to have a safe descent speed gets the full
        //   retarder at once, the way a driver sets it at the top, and the
        //   drums snub the rest;
        // * otherwise a stage goes up while the truck is over its number and
        //   not already slowing, and down only once it is well under;
        // * a step the other way waits out `CRUISE_JAKE_REVERSE_S`, so the
        //   retarder cannot hunt between two stages;
        // * and it comes off entirely only when the grade itself is over.
        let current = self.cruise_jake_stage;
        let slowing = self.trip.truck.net_accel_mph_per_s() < -GRADE_HOLDING_MPH_PER_S;
        let speed_now = self.trip.truck.speed_mph();
        // Over the number of a steep pitch in sight, on a downgrade, the speed
        // comes off on the engine brake whatever this easier stretch would
        // warrant on its own. Between the 5.8 and the 7.0 on I-70 the
        // retarder stayed down on a 2.4 percent stretch and the drums alone
        // took the truck from 63 to the 45 the pitch ahead needed (descent
        // bench, 2026-09-24). Level road is still the drums' (see above).
        let over_hill_number = still_a_grade
            && self
                .descent_safe_mph
                .is_some_and(|safe| speed_now > safe + CRUISE_JAKE_OVER_MPH);
        let steep_hill = (may_retard && self.descent_safe_mph.is_some()) || over_hill_number;
        let mut wanted = current;
        if !still_a_grade {
            wanted = 0;
        } else if steep_hill {
            wanted = ceiling;
        } else if may_retard && over > CRUISE_JAKE_OVER_MPH && !slowing {
            wanted = current + 1;
        } else if over < -CRUISE_JAKE_UNDER_MPH && self.descent_safe_mph.is_none() {
            // Not with a steep pitch in sight: the easier quarter mile
            // between two of them is not the bottom of the hill, and a
            // retarder let down there only comes straight back up.
            wanted = current - 1;
        }
        wanted = wanted.clamp(0, ceiling);
        // Never reach for a retarder the driver's own jake switch is holding,
        // and never release one either -- only what cruise raised itself. The
        // J key's retarder manager is the driver's too: it holds its own
        // stage, and cruise only snubs below.
        let driver_owns_jake = self.auto_jake
            || (self.cruise_jake_stage == 0 && self.trip.truck.engine_brake_stage > 0);
        self.cruise_jake_reverse_s = 0.0f64.max(self.cruise_jake_reverse_s - dt);
        if wanted != current && !driver_owns_jake {
            let step = (wanted - current).signum();
            let reversing = step != self.cruise_jake_last_step && self.cruise_jake_last_step != 0;
            let released = wanted == 0 && !still_a_grade;
            // Setting full retard at the top of a steep hill is one decision,
            // not a step, and it cannot wait: a stage let down on the easier
            // stretch above a seven percent pitch held the truck at nothing
            // while it ran from 44 to 62 (bench, 2026-09-24).
            let set_for_hill = steep_hill && wanted == ceiling;
            let settled = self.cruise_jake_cooldown_s <= 0.0
                && (!reversing || self.cruise_jake_reverse_s <= 0.0);
            if released || set_for_hill || settled {
                self.cruise_jake_stage = wanted;
                self.trip.truck.engine_brake_stage = wanted;
                self.cruise_jake_cooldown_s = CRUISE_JAKE_STEP_S;
                self.cruise_jake_reverse_s = CRUISE_JAKE_REVERSE_S;
                self.cruise_jake_last_step = if released { 0 } else { step };
            }
        }
        // Holding a grade. The drums only come out once the retarder is doing
        // everything it can -- or once it is clear there is no retarder coming,
        // which is the whole of it when the stalk is off -- and then as a snub
        // that finishes and lets go.
        //
        // `may_retard`, deliberately, not `still_a_grade`: on a shallow descent
        // no retarder is coming, so the drums are the answer and they must not
        // stand around waiting for a stage that will never be raised. This is
        // what keeps the truck ON its number down a two or three percent grade
        // now that the jake stays out of it -- and it reaches for them sooner
        // than the old code did, which waited for the retarder to max out
        // first. A stage already up under the hysteresis above satisfies
        // `jake_maxed` on its own, so the drums stay available there too.
        let jake_ceiling = if may_retard || over_hill_number {
            ceiling
        } else {
            0
        };
        let stage_now = if self.auto_jake {
            self.trip.truck.engine_brake_stage
        } else {
            self.cruise_jake_stage
        };
        let jake_maxed = stage_now >= 1.max(jake_ceiling) || jake_ceiling <= 0;
        // The gear is held with the brakes, not given up. An automatic spun
        // past its retarder ceiling upshifts to save the engine, which halves
        // the jake and is the start of a runaway: the agent's drive down the
        // seven percent went from 45 to 55 that way (2026-09-24). So a snub
        // also starts when the revs close on that ceiling, whatever the
        // overspeed says -- while the retarder is ON. With no stage up there
        // is no retarder gear to keep, and guarding one anyway put a bobtail
        // on a snub every two seconds down Red Mountain's 8.4 percent (bend
        // sweep, 2026-09-25); the box's own protective upshift is the answer.
        // And a hill's safe descent speed is a ceiling, not a preference: past
        // it by the snub band the drums come out whatever the retarder is
        // doing, the way the CDL manual's snub braking reads -- at the safe
        // speed, brake down below it, release.
        let past_safe_descent = self
            .descent_safe_mph
            .is_some_and(|safe| speed_now > safe + CRUISE_BRAKE_OVER_MPH);
        let guarding_gear = self.trip.truck.transmission.automatic
            && self.on_downgrade()
            && self.trip.truck.engine_brake_stage > 0;
        let revs_at_ceiling = guarding_gear
            && self.trip.truck.throttle <= 0.05
            && rpm_now >= JAKE_MAX_RPM - DESCENT_RPM_GUARD;
        if self.cruise_snubbing {
            // A revs snub runs until they are a guard band clear of the
            // ceiling again, so it is one application rather than a flutter.
            let revs_still_high = revs_at_ceiling
                || (guarding_gear && rpm_now >= JAKE_MAX_RPM - 2.0 * DESCENT_RPM_GUARD);
            self.cruise_snubbing = over > -CRUISE_SNUB_UNDER_MPH || revs_still_high;
        } else if (jake_maxed && over > CRUISE_BRAKE_OVER_MPH)
            || revs_at_ceiling
            || past_safe_descent
        {
            self.cruise_snubbing = true;
        }
        if self.cruise_snubbing {
            let weather_brake: f64 = if self.trip.weather.effects().grip < 0.7 {
                0.45
            } else {
                0.65
            };
            self.trip.truck.brake = self
                .trip
                .truck
                .brake
                .max(weather_brake.min(CRUISE_SNUB_BRAKE));
        }
    }
}

/// Rolling means of `window` consecutive samples.
fn window_means(samples: &[f64], window: usize) -> Vec<f64> {
    (0..=samples.len() - window)
        .map(|i| samples[i..i + window].iter().sum::<f64>() / window as f64)
        .collect()
}

/// Python's `str.title()` for the one-word zone reasons the cue uses.
fn title_case(text: &str) -> String {
    text.split(' ')
        .map(capitalize_first)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Python's `str.capitalize()`.
fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase(),
    }
}
