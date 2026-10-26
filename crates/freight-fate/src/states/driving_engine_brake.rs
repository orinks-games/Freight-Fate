//! No-engine-brake zones: town noise ordinances around route cities (port of
//! `freight_fate/states/driving_engine_brake.py`, the `EngineBrakeZoneMixin`).
//!
//! Engine brakes are legal under federal and state law; what exists in the
//! real world is hundreds of municipal noise ordinances, posted NO ENGINE
//! BRAKE at the city limits, with exemptions for emergencies -- and the
//! legitimate use of the retarder, holding a heavy truck back on a real
//! downgrade, stays legal everywhere. Fines run roughly 100 to 500 dollars and
//! escalate for repeat offenders (LegalClarity municipal-ordinance survey,
//! 2026; e.g. Snyder, Texas ordinance 1043). The game maps those ordinances
//! onto the same urban radius that already lowers the speed limit near a route
//! city.
//!
//! A blind player cannot see the sign, so the spoken cue is the sign: the zone
//! announces itself ahead when the retarder is on, a violation warns with a
//! grace window before any money moves, and the citation names the amount and
//! the reason.

use ff_core::pyfmt::fmt_grouped;
use ff_core::sim::vehicle::{AMBIENT_C, BRAKE_COOL_BASE_PER_S, BRAKE_COOL_SPEED_PER_S};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

/// How far out the spoken NO ENGINE BRAKE sign reads when the retarder is on.
pub const JAKE_ZONE_WARN_MI: f64 = 2.0;
/// Real seconds between the violation warning and the citation -- time to hear
/// the warning and reach the switch, mirroring the hazard reaction windows.
pub const JAKE_ZONE_GRACE_S: f64 = 10.0;
/// Municipal-ordinance range, escalating per citation and capped at the top.
pub const JAKE_ZONE_FINES: [f64; 3] = [150.0, 300.0, 500.0];
/// Where the spoken road stops being level and starts being a grade.
///
/// PROVENANCE, because this number spent two weeks doing a job it was never
/// derived for: it is `GRADE_WARN_CLEAR_PCT`, and that is the RELEASE edge of
/// the spoken grade advisory's hysteresis pair -- the advisory speaks at
/// `GRADE_WARN_PCT` (3 percent) and goes quiet again under this. It is a
/// speech constant. Nothing about brakes, weight or heat went into it and it
/// carries no external source; it was chosen so the ordinance carve-out agreed
/// with what the G key calls the road, which is a fine reason for a carve-out
/// and no reason at all for a control threshold. It is also the number that
/// put the retarder on every shallow dip in the road.
///
/// It keeps both of the jobs it was fit for -- the town-ordinance exemption
/// below, where no money moves any differently for it, and the geometric
/// [`DrivingState::on_downgrade`] -- and has lost the one it was not. Whether
/// an assist may RAISE the retarder is now
/// [`DrivingState::retarder_warranted`], which is derived.
pub const JAKE_ZONE_EXEMPT_GRADE_PCT: f64 = 2.0;
/// Below this the retarder is not barking at road speed; a truck creeping a
/// gate queue or parked with the switch on is not what the ordinance is for.
pub const JAKE_ZONE_MIN_MPH: f64 = 10.0;

impl DrivingState {
    /// Warn, then fine, for engine braking inside a town's ban zone.
    pub fn update_engine_brake_zone(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.ramp_mi.is_some() || self.trip.finished || self.pull_over.is_some() {
            return;
        }
        let city = self.trip.engine_brake_ban_at(self.trip.position_mi);
        match &city {
            Some(city) => {
                // A real driver flips engine-brake mode off at the town line;
                // the assists do the same before any driver-fault question is
                // asked.
                let city = city.clone();
                self.release_assist_jake_in_zone(ctx, &city);
            }
            None => self.assist_zone_cue_key = None,
        }
        // The switch alone is not the offense; the bark is. The vehicle already
        // knows whether the retarder is genuinely retarding (stage selected,
        // engine on, off the fuel, in gear), so ask it rather than re-deriving.
        // Force at the wheels, not torque at the crank: with the clutch in,
        // the jake retards nothing and the game plays no bark for it.
        let barking = self.trip.truck.jake_brake_force() > 0.0
            && self.trip.truck.speed_mph() >= JAKE_ZONE_MIN_MPH;
        // Cruise and the curve assist raise the retarder themselves and release
        // it themselves -- and inside a zone they have just released it above,
        // so an assist bark never reaches the fine path. Auto mode on an AMT is
        // different: the driver armed the stalk with J, so its bark is theirs.
        let driver_owns_jake = barking && self.cruise_jake_stage == 0 && !self.curve_assist_jake;
        let Some(city) = city else {
            self.jake_violation_deadline_s = None;
            self.jake_citation_latched = false;
            self.maybe_warn_jake_zone_ahead(ctx, driver_owns_jake);
            return;
        };
        if !driver_owns_jake || self.jake_zone_exempt() {
            self.jake_violation_deadline_s = None;
            if !self.trip.truck.engine_brake() {
                // The engagement is over; a fresh one can earn a fresh citation.
                self.jake_citation_latched = false;
            }
            return;
        }
        if self.jake_citation_latched {
            return; // one citation per continuous engagement
        }
        if self.jake_violation_deadline_s.is_none() {
            if self.jake_zone_grace_used.contains(&city) {
                // The grace window is one chance to comply, not a renewable
                // exemption. Flicking the switch off just before the timer
                // expires and straight back on used to draw warnings forever
                // and never a fine; coming back on the jake in the same town
                // is now an immediate citation.
                self.jake_citation_latched = true;
                self.fine_engine_braking(ctx, &city);
                return;
            }
            self.jake_zone_grace_used.insert(city.clone());
            self.jake_violation_deadline_s = Some(JAKE_ZONE_GRACE_S);
            self.speak_jake_zone_warning(ctx);
            return;
        }
        let remaining = self.jake_violation_deadline_s.unwrap_or(0.0) - dt;
        self.jake_violation_deadline_s = Some(remaining);
        if remaining <= 0.0 {
            self.jake_violation_deadline_s = None;
            self.jake_citation_latched = true;
            self.fine_engine_braking(ctx, &city);
        }
    }

    /// May cruise or the curve assist raise the retarder here?
    ///
    /// Not inside a town's ban zone -- the assists respect the posted sign the
    /// way a driver flips engine-brake mode off in town -- except where the
    /// ordinance's own carve-outs apply: a real downgrade or an emergency,
    /// where the retarder is the safe tool everywhere.
    pub fn assist_jake_allowed(&mut self, _ctx: &mut GameContext) -> bool {
        if self
            .trip
            .engine_brake_ban_at(self.trip.position_mi)
            .is_none()
        {
            return true;
        }
        self.jake_zone_exempt()
    }

    /// Is the road under the truck a downgrade at all?
    ///
    /// The GEOMETRIC question, and only that: is the road going downhill.
    /// `JAKE_ZONE_EXEMPT_GRADE_PCT` is the same line the ordinance carve-out
    /// and the spoken G readout already draw between level road and a grade,
    /// so a driver hearing "level" never hears a retarder answering a grade.
    ///
    /// This is NOT the question "may an assist raise the retarder" -- that is
    /// [`Self::retarder_warranted`], and the two were the same predicate until
    /// 2026-08-24, which is why the jake barked at every one and two percent
    /// dip in the road (owner report: "the jake activates on every single
    /// descent it seems, even shallow descent like 1-3 percent"). The pair is
    /// deliberate hysteresis: the retarder comes UP only where the drums
    /// cannot hold the hill, and stays up until the road is genuinely level,
    /// because a retarder that lets go the moment a grade eases a point and
    /// grabs again a hundred yards later is the loudest thing on the road.
    ///
    /// So this predicate still answers the HOLD, the release, the ordinance
    /// carve-out, and "is a bend's speed cap the grade's doing or the
    /// corner's".
    pub fn on_downgrade(&self) -> bool {
        self.trip.grade_at(self.trip.position_mi) * 100.0 <= -JAKE_ZONE_EXEMPT_GRADE_PCT
    }

    /// Would holding this descent on the service brakes alone cook them?
    ///
    /// The question an assist has to answer before reaching for the retarder,
    /// and the honest answer is not a grade at all. What sends a driver to the
    /// engine brake is not slope, it is whether the foundation brakes can
    /// absorb the descent without fading -- which is grade, WEIGHT and SPEED
    /// together, with length deciding only how soon.
    ///
    /// # The rule, and where it comes from
    ///
    /// FHWA's Grade Severity Rating System (GSRS, FHWA-RD-86-045, the model
    /// behind runaway-ramp placement and posted downgrade speeds) rates a
    /// grade by exactly this: it takes grade percent, grade length and gross
    /// vehicle weight, computes the heat the brakes must absorb against the
    /// heat they shed, and caps the drum temperature. AASHTO's Green Book
    /// downgrade controls and the MUTCD steep-grade signs quote percent AND
    /// miles for the same reason. Jacobs, who build the retarder, say what it
    /// is for: SUSTAINED speed control, "not a substitute for a service
    /// braking system".
    ///
    /// The truck already carries the whole of that model. `update_temps` heats
    /// the drums by the power the shoes dissipate and cools them toward
    /// ambient at a rate that rises with road speed:
    ///
    /// ```text
    ///   dT/dt = P_brake / C  -  (T - T_ambient) * k(v)
    ///   k(v)  = BRAKE_COOL_BASE_PER_S + BRAKE_COOL_SPEED_PER_S * sqrt(v)
    ///   C     = specs.brake_thermal_mass_j_per_c
    /// ```
    ///
    /// Held at a steady speed down a steady grade, the drums must absorb
    /// exactly what gravity gives them less what rolling resistance and drag
    /// already take -- which is `-resistance_force()`, the truck's own number,
    /// so no second copy of the physics is written here. That power settles
    /// the drums at
    ///
    /// ```text
    ///   T_settle = T_ambient + P_brake / (C * k(v))
    /// ```
    ///
    /// and the criterion is `T_settle >= brake_fade_onset_c()`: at or above it
    /// the drums are on a one-way trip to the temperature where they stop
    /// answering, and only the hill's length decides when they arrive. Below
    /// it they level off cooler than fade and hold the grade INDEFINITELY --
    /// ten miles or a hundred, the answer does not change. That is what makes
    /// the steady-state test the right separator rather than an energy budget:
    /// length cannot turn a grade the drums can hold into one they cannot, and
    /// GSRS says the same thing when it finds no speed reduction is warranted
    /// however long the grade runs.
    ///
    /// # The arithmetic, for the truck this game hands out
    ///
    /// Default specs: 36,000 kg rated gross, Cd 0.65 over 10 m2, rolling
    /// 0.0065, C = 180,000 J/degC, fade onset 400 degC (per model 400 to 480,
    /// less up to 150 degC on worn shoes). At 80,000 lb and 55 mph the drums
    /// settle at fade on a 4.2 percent grade. The line moves with the load and
    /// barely at all with speed:
    ///
    /// ```text
    ///   gross weight        45 mph   55 mph   62 mph   70 mph
    ///   80,000 lb (36.0 t)   4.27%    4.15%    4.14%    4.20%
    ///   71,650 lb (32.5 t)   4.67%    4.52%    4.52%    4.59%
    ///   55,100 lb (25.0 t)   5.87%    5.69%    5.68%    5.77%
    ///   empty + trailer      9.65%    9.33%    9.32%    9.47%
    /// ```
    ///
    /// So it is a curve in weight, not a number: a grossed-out truck reaches
    /// for the retarder just past four percent, an empty one essentially never
    /// does, and that is right -- an empty rig genuinely does not need it. The
    /// owner's guessed six percent is about right for a light load and much
    /// too late for a heavy one; the two percent that shipped was never a
    /// braking number at all (see `JAKE_ZONE_EXEMPT_GRADE_PCT`).
    ///
    /// # Measured, not argued
    ///
    /// `states_driving_jake_line` drives a grossed-out truck fifteen minutes
    /// down each of these grades at 55 mph with NO retarder available at all
    /// and reads the drums:
    ///
    /// ```text
    ///   grade   held mph   peak drum   predicted   fade at   warranted?
    ///   -1.0%       54.2      20 degC    20 degC   400 degC      no
    ///   -2.0%       55.3     113 degC   110 degC   400 degC      no
    ///   -3.0%       54.8     241 degC   245 degC   399 degC      no
    ///   -4.0%       54.4     368 degC   379 degC   399 degC      no
    ///   -4.5%       54.2     430 degC   446 degC   398 degC     yes
    ///   -5.0%       54.0     493 degC   512 degC   398 degC     yes
    ///   -6.0%       57.9     592 degC   657 degC   397 degC     yes
    /// ```
    ///
    /// The drums cross fade between four and four and a half percent, which is
    /// where the arithmetic said they would, and the predicate says yes for
    /// exactly the rows that cooked. One percent never warms the shoes at all
    /// -- the hill does not even overcome drag and rolling resistance, so the
    /// truck holds it on nothing.
    ///
    /// A second, independent cross-check: the curve assist's bench trace of
    /// 2026-08-11 held a six percent descent on the drums alone and went "past
    /// fade in four and a half minutes". The closed form puts fade on that
    /// grade at 258 seconds.
    ///
    /// # Sustained, not a dip
    ///
    /// The retarder is for sustained speed control, so the descent has to last
    /// long enough to be one. `GRADE_WARN_MIN_RUN_MI` is the run the grade
    /// advisories already calibrated against the baked corridors -- the
    /// mountain data is full of punchy quarter-mile dips, and unfiltered they
    /// buried the hills that matter. The same filter here keeps a blip in the
    /// road profile from putting a bark in the player's ears.
    pub fn retarder_warranted(&self) -> bool {
        // Cheap gates first: this runs every frame for every assist, and on
        // level road the arithmetic below must never be reached.
        if !self.on_downgrade() {
            return false;
        }
        let truck = &self.trip.truck;
        let speed_mps = truck.velocity_mps.abs();
        if speed_mps <= 1.0 {
            return false; // parked or crawling: nothing for a retarder to hold
        }
        // What the drums must hold to keep this speed on this grade. The
        // truck's own resistance model, negated: on a descent steep enough to
        // pull the truck along it is negative, and its magnitude IS the
        // service-brake force equilibrium asks for. Borrowed rather than
        // rewritten so there is one copy of the aerodynamics and one copy of
        // the rolling resistance in the game.
        //
        // `Trip::update` writes `truck.grade` from the mile the truck was at
        // when the frame started, so on the one frame a grade begins this is
        // still reading the flat behind it while `on_downgrade` above already
        // reads the slope. A sixtieth of a second on a grade that has to run
        // three quarters of a mile to qualify at all; the next frame agrees.
        let hold_force = -truck.resistance_force();
        if hold_force <= 0.0 {
            return false; // the hill does not even overcome drag and rolling
        }
        let cooling_per_c = truck.specs.brake_thermal_mass_j_per_c
            * (BRAKE_COOL_BASE_PER_S + BRAKE_COOL_SPEED_PER_S * speed_mps.sqrt());
        if cooling_per_c <= 0.0 {
            return true; // drums that shed no heat cannot hold anything
        }
        let settle_c = AMBIENT_C + hold_force * speed_mps / cooling_per_c;
        if settle_c < truck.brake_fade_onset_c() {
            return false;
        }
        self.descent_runs_at_least(GRADE_WARN_MIN_RUN_MI)
    }

    /// Does the downgrade under the wheels keep going for `want_mi`?
    ///
    /// Sampled at the stride the baked grade segments use and stopped the
    /// moment the answer is known, so the common case is three lookups. The
    /// "still the same grade" test is `GRADE_WARN_CLEAR_PCT`, the same line
    /// [`Self::grade_run_mi`] uses, so a six percent pitch that eases to three
    /// halfway down is still one descent rather than two.
    fn descent_runs_at_least(&self, want_mi: f64) -> bool {
        let total = self.trip.total_miles();
        let mut probe = self.trip.position_mi;
        let mut run = 0.0;
        while run < want_mi {
            probe += GRADE_WARN_STEP_MI;
            if probe >= total {
                return false;
            }
            if self.trip.grade_at(probe) * 100.0 > -GRADE_WARN_CLEAR_PCT {
                return false;
            }
            run += GRADE_WARN_STEP_MI;
        }
        true
    }

    /// Is the road under the truck a real UPGRADE?
    ///
    /// The mirror of [`Self::on_downgrade`], on the same line, and the one
    /// road no retarder belongs on at all. A hill takes the speed off by
    /// itself; a driver climbing one wants power. Overspeed carried into an
    /// upgrade is the hill's to eat, never the jake's -- a real driver powers
    /// up a grade and does not bark the retarder at it (Brandon, 2026-08-20,
    /// which is the rule adaptive cruise's raise gate has followed since, and
    /// the owner again on 2026-08-24: "on uphill ascents the truck should gain
    /// speed instead of using the engine brakes").
    pub fn on_climb(&self) -> bool {
        self.trip.grade_at(self.trip.position_mi) * 100.0 >= JAKE_ZONE_EXEMPT_GRADE_PCT
    }

    /// Drop an assist-raised retarder at the town line.
    ///
    /// Runs every frame inside a zone; releasing is idempotent and the raise
    /// gates keep the assists from reaching for it again. Spoken once per
    /// zone, only when a retarder audibly stops -- the note cutting out with
    /// no explanation is the confusing part -- and never in terse speech,
    /// which skips advisory-class cues (no money is at risk here).
    pub fn release_assist_jake_in_zone(&mut self, ctx: &mut GameContext, city: &str) {
        let cruise_owns = self.cruise_jake_stage > 0;
        if (!cruise_owns && !self.curve_assist_jake) || !self.trip.truck.engine_brake() {
            return;
        }
        if self.jake_zone_exempt() {
            return; // a real downgrade or emergency: safety wins in town too
        }
        self.trip.truck.engine_brake_stage = 0;
        if cruise_owns {
            self.cruise_jake_stage = 0;
        }
        self.curve_assist_jake = false;
        if self.assist_zone_cue_key.as_deref() == Some(city) || self.terse_speech(ctx) {
            return;
        }
        self.assist_zone_cue_key = Some(city.to_string());
        let spoken = ctx.world.spoken_city(city, Some(false));
        let message = format!("No engine brake zone, {spoken}.");
        ctx.play_event_cue(Some(SpeechCategory::Confirmation), "ui/notify", 0.6, 0.0);
        // ROUTE, not the ambient default: an assist silently switching how it
        // is braking is a consequence, not colour, same class as the
        // adaptive-cruise easing line (automation-handoff sweep, 2026-08-20,
        // the deferred 2026-08-15 audit).
        ctx.say_event_with(
            message,
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Confirmation),
        );
    }

    /// The ordinance's own carve-outs: emergencies and real downgrades.
    pub fn jake_zone_exempt(&self) -> bool {
        // A live hazard warning or the emergency brake is the game's "avoiding
        // imminent danger" -- every well-drafted ordinance excuses that.
        if self.trip.truck.emergency_brake || self.hazard_deadline.is_some() {
            return true;
        }
        self.on_downgrade()
    }

    /// Read the NO ENGINE BRAKE sign out loud while it still helps.
    ///
    /// Only when the driver's own retarder is on: with it off there is nothing
    /// to do, and every route city would otherwise add a callout on top of the
    /// urban limit changes already announced there. Terse speech skips the
    /// advisory -- the in-zone warning still comes before any fine, so a terse
    /// driver risks nothing but a later cue.
    pub fn maybe_warn_jake_zone_ahead(&mut self, ctx: &mut GameContext, driver_owns_jake: bool) {
        if !driver_owns_jake || self.terse_speech(ctx) {
            return;
        }
        let Some((start_mi, city)) = self.trip.next_engine_brake_ban(JAKE_ZONE_WARN_MI) else {
            return;
        };
        let key = format!("{start_mi:.1}");
        if self.jake_zone_warned_key.as_deref() == Some(key.as_str()) {
            return;
        }
        self.jake_zone_warned_key = Some(key);
        let distance = ctx
            .settings
            .distance_text(start_mi - self.trip.position_mi, false);
        let spoken = ctx.world.spoken_city(&city, Some(false));
        ctx.audio.play_with("ui/notify", 0.6, 0.0);
        ctx.say_event_with(
            format!("No engine brake zone in {distance}, coming into {spoken}."),
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Navigation),
        );
    }

    /// The warning that always precedes the first fine -- the audio sign.
    pub fn speak_jake_zone_warning(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/warning");
        ctx.controller.rumble.alert();
        ctx.say_event_with(
            "No engine brake zone. Switch it off.",
            SayEvent::new().category(SpeechCategory::Navigation),
        );
    }

    /// The citation: paid on the spot, escalating like repeat offenses do.
    pub fn fine_engine_braking(&mut self, ctx: &mut GameContext, city: &str) {
        if self.new_driver_warning(ctx) {
            ctx.audio.play("ui/warning");
            ctx.say_event_with(
                "A local officer warns you about the engine brake.",
                SayEvent::new().category(SpeechCategory::Navigation),
            );
            return;
        }
        let index = (self.jake_zone_fines.max(0) as usize).min(JAKE_ZONE_FINES.len() - 1);
        let fine = JAKE_ZONE_FINES[index];
        self.jake_zone_fines += 1;
        self.jake_fines_paid += fine;
        let hours = crate::states::driving_rest_states::record_hours(ctx, self);
        {
            let profile = profile_mut_of(ctx);
            profile.spend(fine); // can go negative; never a game over
                                 // A municipal noise ordinance is not an FMCSA serious violation,
                                 // so it never moves the suspension ladder -- but it is still a
                                 // citation on the record, and the next citation of any kind costs
                                 // more for it.
            profile.driving_record.record_citation_at(fine, hours);
            profile.driving_record.note(
                ff_core::models::enforcement::RECORD_CITATION,
                "Engine brake in a no engine brake zone",
                fine,
                hours,
                city,
            );
        }
        ctx.audio.play("ui/error");
        ctx.controller.rumble.alert();
        let message = format!("Engine brake citation: {} dollars.", fmt_grouped(fine, 0));
        ctx.say_event_with(message, SayEvent::new().category(SpeechCategory::Money));
    }
}
