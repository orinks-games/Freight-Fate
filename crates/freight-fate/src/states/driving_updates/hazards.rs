//! The braking budget, automatic emergency braking, hazard resolution, the
//! horn's one real power, the routine speed announcement, and the grade
//! advisory.

use crate::speech::EventPriority;
use ff_core::pyrandom::PyRandom;
use ff_core::sim::driving_modes::tuning_for_time_scale;
use ff_core::sim::trip_models::HAZARDS;
use ff_core::sim::vehicle::BrakeApplication;
use ff_core::speech_pacing::SpeechCategory;
use ff_core::speech_text::terse_silent;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_updates::live;

impl DrivingState {
    pub fn update_announcements(&mut self, ctx: &mut GameContext, dt: f64) {
        if ctx.settings.renders_terse() {
            return;
        }
        self.speed_announce_timer += dt;
        let interval = tuning_for_time_scale(self.trip.time_scale).routine_speech_interval_s;
        if self.speed_announce_timer >= interval {
            self.speed_announce_timer = 0.0;
            let mph = self.trip.truck.speed_mph();
            if (mph - self.last_announced_mph).abs() >= 5.0 && mph > 1.0 {
                self.last_announced_mph = mph;
                ctx.say_event_with(
                    ctx.settings.speed_text(mph),
                    SayEvent::queued().category(SpeechCategory::Status),
                );
            }
        }
    }

    /// Seconds of full service braking to reach the given safe speed.
    ///
    /// Uses the braking the truck can actually deliver right now -- fade,
    /// wear, load, and grip -- helped uphill and hurt downhill. The rated
    /// spec number engaged the assist two seconds before a collision on
    /// hot brakes (playtest transcript, 2026-07-16).
    pub fn brake_budget_s(&self, target_mph: f64) -> f64 {
        let t = &self.trip.truck;
        t.braking_time_for_s(
            t.velocity_mps,
            target_mph / MPH_PER_MPS,
            true,
            BrakeApplication::Service(1.0),
        )
    }

    /// Seconds of emergency braking to reach the given safe speed.
    pub fn emergency_brake_budget_s(&self, target_mph: f64) -> f64 {
        let t = &self.trip.truck;
        t.braking_time_for_s(
            t.velocity_mps,
            target_mph / MPH_PER_MPS,
            true,
            BrakeApplication::Emergency,
        )
    }

    /// Time-to-hazard at which automatic braking has to take the truck.
    ///
    /// The physics budget plus its lead: braking heats the brakes, so the
    /// stop the budget just predicted gets slower while it happens.
    pub fn aeb_engage_s(&self, target_mph: f64) -> f64 {
        self.brake_budget_s(target_mph) * AEB_BUDGET_MARGIN + AEB_LEAD_S
    }

    /// Time-to-hazard that leaves the driver `window_s` of their own.
    ///
    /// Built forward from the moment the assist must act rather than back
    /// from raw braking physics. The old form -- budget plus slack -- made
    /// the driver's window a remainder: the assist's engage margin scales
    /// with the budget, so speed, grade, brake heat, wear and grip all came
    /// out of the driver's time instead of the truck's. At 65 mph on a
    /// traffic warning that remainder was half a second, and on hot brakes
    /// it was already spent when the words started (Munchkinbear, 2026-08-11).
    ///
    /// `shape` defaults to the currently pending hazard's own, but a hazard
    /// arming while another is still live needs its OWN budget computed on
    /// ITS OWN shape -- before the pending one is folded in -- so the caller
    /// can pass it explicitly.
    pub fn hazard_deadline_for(&self, window_s: f64, shape: Option<HazardShape>) -> f64 {
        let shape = shape.unwrap_or_else(|| self.hazard_shape());
        let mut window = window_s.max(HAZARD_MIN_REACTION_S);
        if shape.dodgeable {
            // The warning offers a lane change; leave room to actually make
            // one. ONLY where one can actually be made: this allowance used
            // to be handed to every lead-vehicle hazard whatever the road,
            // so the driver with one lane and nowhere to go got two and a
            // half seconds of grace for a move they could not make, and the
            // assist waited it out while the truck kept closing. That put
            // the driver who could NOT go around nearer the vehicle in front
            // before anything acted than the one who could (owner,
            // 2026-08-24).
            window += LANE_TAP_CHANGE_S;
        }
        let target_mph = self.hazard_target_mph(Some(shape));
        self.aeb_engage_s(target_mph) + window
    }

    /// The live hazard's own shape, for the assist to budget against.
    pub fn hazard_shape(&self) -> HazardShape {
        HazardShape {
            dodgeable: self.hazard_dodgeable,
            in_lane: self.hazard_in_lane,
            lead_mph: self.hazard_lead_mph,
            brake_to_mph: self.hazard_brake_to_mph,
        }
    }

    /// Whether a lane change already in progress will land in time.
    ///
    /// A driver mid-drift has answered the warning, and grabbing the truck
    /// out from under them is the assist overriding the very move it asked
    /// for. Only while the move can still land: a dodge that no longer
    /// beats the hazard is not a plan, and braking is what is left.
    pub fn dodge_still_beats_the_hazard(&self) -> bool {
        if self.lane_change_target.is_none() || !self.hazard_dodgeable {
            return false;
        }
        let Some(deadline) = self.hazard_deadline else {
            return false;
        };
        self.lane_change_timer <= deadline
    }

    /// Put the assist's held application back on the pedal before physics.
    ///
    /// The input pass ramps the service brake down every frame nobody is on
    /// it, and writes the emergency flag straight from the B key -- both
    /// before `truck.update()` runs, and both ahead of `update_hazard`,
    /// which is the frame's last word on the hazard. An assist that only
    /// wrote the pedal from there handed the drums an application a frame's
    /// ramp short of the full one its budget assumed, framerate-dependently
    /// so; its emergency flag never survived to be read at all; and the air
    /// system, which charges a whole brake application every time the pedal
    /// RISES, was billed for the difference again and again. Re-asserted here
    /// beside the other assists' floors, one held stop costs one application.
    pub fn apply_hazard_brake(&mut self) {
        if self.aeb_brake <= 0.0 {
            return;
        }
        // Never brake against our own throttle: a hazard assist that has taken
        // the truck has taken the throttle with it.
        self.trip.truck.throttle = 0.0;
        self.trip.truck.brake = self.trip.truck.brake.max(self.aeb_brake);
        if self.aeb_emergency {
            self.trip.truck.emergency_brake = true;
        }
    }

    /// Hand the pedal back, and forget what the last stop measured.
    ///
    /// The assist releases what the assist applied. The input pass also
    /// stomps the emergency flag from the B key every frame, but nothing says
    /// a frame of input runs between engage and clear -- and an application
    /// with no owner left the truck standing on everything for good. A
    /// driver-held B is untouched: only the assist's own flag is dropped.
    pub fn release_hazard_brake(&mut self) {
        if self.aeb_emergency {
            self.trip.truck.emergency_brake = false;
        }
        self.aeb_brake = 0.0;
        self.aeb_emergency = false;
        self.aeb_hold_s = 0.0;
        self.aeb_losing_s = 0.0;
        self.aeb_decel_mps2 = 0.0;
        self.aeb_last_speed_mps = None;
        self.automatic_braking_announced = false;
        self.automatic_braking_escalated = false;
    }

    /// Smooth the deceleration the truck is actually making right now.
    ///
    /// The budget answers what a full application ought to deliver. What the
    /// escalation needs is a different question nobody can predict: whether
    /// the stop already underway is going to get there. Measured off the
    /// truck's own speed and smoothed just enough that a shift, a gust or a
    /// single long frame is not read as a losing stop.
    pub fn track_assisted_deceleration(&mut self, dt: f64) {
        let speed = 0.0f64.max(self.trip.truck.velocity_mps);
        let last = self.aeb_last_speed_mps;
        self.aeb_last_speed_mps = Some(speed);
        let Some(last) = last else {
            return;
        };
        if self.aeb_brake <= 0.0 || dt <= 0.0 {
            return;
        }
        self.aeb_hold_s += dt;
        let sample = (last - speed) / dt;
        let blend = 1.0f64.min(dt / AEB_DECEL_SMOOTHING_S);
        self.aeb_decel_mps2 += (sample - self.aeb_decel_mps2) * blend;
    }

    /// Whether the stop actually underway is going to miss the hazard.
    ///
    /// Not a prediction. The time left, measured against the deceleration the
    /// truck is making with everything already on, and asked to keep the same
    /// fifth of the stop in hand that the engage point was given. A full
    /// service application that is delivering can never trip this: the assist
    /// engages with that margin and a delivering truck holds it, because road
    /// and air drag add to the budget rather than taking from it. What trips
    /// it is losing ground -- drums cooking under the very application meant
    /// to save the stop, a grade steepening under the wheels, grip that is not
    /// there. Then, and only then, the assist uses the hardest stop the rig
    /// has: the same one the B key gives the driver, and what the driver
    /// facing an unavoidable collision would do.
    pub fn service_braking_is_losing(&mut self, dt: f64) -> bool {
        if self.aeb_emergency {
            return true; // earned once, held to the end of the stop
        }
        let Some(deadline) = self.hazard_deadline else {
            self.aeb_losing_s = 0.0;
            return false;
        };
        if self.aeb_hold_s < AEB_DECEL_SMOOTHING_S {
            self.aeb_losing_s = 0.0;
            return false;
        }
        let over_mps =
            0.0f64.max((self.trip.truck.speed_mph() - self.hazard_target_mph(None)) / MPH_PER_MPS);
        let left_s = deadline.max(0.0);
        if self.aeb_decel_mps2 * left_s >= over_mps * AEB_BUDGET_MARGIN {
            self.aeb_losing_s = 0.0;
            return false;
        }
        self.aeb_losing_s += dt;
        self.aeb_losing_s >= AEB_ESCALATE_CONFIRM_S
    }

    /// The speed that resolves the active hazard by brake alone.
    ///
    /// KEYED ON WHAT THE HAZARD IS, never on whether there is a lane to take
    /// instead. Those are separate questions and this used to answer both
    /// with one flag; see [`HazardShape`]. Defaults to the live hazard's own
    /// shape; see `hazard_deadline_for` for why a caller would pass one.
    ///
    /// A VEHICLE IS NOT A FIXED OBJECT. Brake lights ahead used to fall under
    /// the same near-stop rule as a tyre carcass, and with automatic braking
    /// the truck obeys without the driver choosing it, so Brandon was dragged
    /// from 70 to nearly stopped sixteen times in ninety minutes on an open
    /// 75 interstate, each costing more than a minute to climb back: "cruise
    /// control still drops speed dramatically... and never comes back up"
    /// (2026-08-23). Matching the vehicle ahead is what a driver actually
    /// does, and it clears the hazard honestly -- you are no longer closing
    /// on it. Never below the creep floor, so a lead that has itself stopped
    /// still asks for a stop.
    ///
    /// A FIXED OBJECT IN THE LANE cannot be rolled over at the moving-hazard
    /// safe speed whether or not a lane is open beside it: brake alone means
    /// nearly stopping. That is why this reads `in_lane` and not `dodgeable`
    /// -- a carcass on a one-lane road became a 25 mph problem the moment
    /// "dodgeable" started meaning "there is somewhere to go".
    ///
    /// ANYTHING THAT SPANS THE ROAD -- fog, ice, a crosswind -- takes the
    /// moving-hazard safe speed, because there is no object in the lane to
    /// creep past. An animal spans it too, but brakes to its own catalog
    /// speed (`ANIMAL_BRAKE_TO_MPH`).
    pub fn hazard_target_mph(&self, shape: Option<HazardShape>) -> f64 {
        let shape = shape.unwrap_or_else(|| self.hazard_shape());
        if let Some(lead_mph) = shape.lead_mph {
            return HAZARD_CREEP_MPH.max(lead_mph);
        }
        // Something a truck passes slowly: a tarp it straddles, water it
        // eases through, a car on the shoulder it gives room to.
        if let Some(mph) = shape.brake_to_mph {
            return mph;
        }
        if shape.in_lane {
            return HAZARD_CREEP_MPH;
        }
        // In weather that takes grip or sight, something across the road
        // asks for the speed the weather is safe at, a little under: braking
        // a truck to 25 for a gust or a hail band it was already driving
        // slow for was the "too much braking" (owner, 2026-10-19). The same
        // test of bad weather the cruise caps on.
        let effects = self.trip.weather.effects();
        if effects.is_adverse() {
            let weather_mph = effects.safe_speed_mph - HAZARD_WEATHER_MARGIN_MPH;
            return HAZARD_SAFE_MPH.max(weather_mph.min(HAZARD_SPANNING_MAX_MPH));
        }
        HAZARD_SAFE_MPH
    }

    // -- grades ---------------------------------------------------------------------

    /// How to get down a hill, in terms of the controls this driver has.
    ///
    /// An automatic has no gear selection -- W, Q, N and Backspace are all
    /// manual-only -- so telling that driver to pick a gear names a control
    /// they do not have. What they do have is the same one a real automated
    /// box gives them: brake, and the transmission holds a lower gear for
    /// them (`auto_shift` picks the tallest gear landing in the 1050-1700
    /// band while braking, and never upshifts off the pedal).
    ///
    /// And in terms of the state they are in: with the jake already on (or
    /// the automatic jake armed), J turns it OFF, so "Set the engine brake
    /// with J" told the driver to switch off the thing the hill needs (agent
    /// drive, Silverthorne to Edwards, 2026-09-24). Then the jake clause
    /// goes, and on an automatic nothing is left to advise.
    ///
    /// And only while it is still teaching. A driver who has switched the
    /// engine brake on a few times knows the hill wants it, and hearing how on
    /// every downgrade got old (bscross32, 2026-10: "I'll decide how to handle
    /// it"). Once the J key is retired the call is just the grade and its
    /// length. A remapped key or a new transmission teaches it again.
    pub fn descend_advice(&self, ctx: &mut GameContext) -> String {
        if self.instruction_retired(ctx, "engine_brake") {
            return String::new();
        }
        let jake_set = self.trip.truck.engine_brake() || self.auto_jake;
        let automatic = self.trip.truck.transmission.automatic;
        if jake_set {
            return if automatic {
                String::new()
            } else {
                "Pick your gear before it starts.".to_string()
            };
        }
        format!("Set the jake with {}.", ctx.control_hint("engine_brake"))
    }

    /// How far a grade of this sign keeps its character from `start_mi`.
    ///
    /// Sampled at the stride the baked grade segments use, so the answer is
    /// the run the road data actually has rather than an interpolation of it.
    ///
    /// The ONE length a steep grade has. The advisory, the G key's "running
    /// 2 miles" for the grade ahead and its "for another" for the grade
    /// underneath all read it here. The G key used to measure until the
    /// road stopped going downhill at all, so one seven percent pitch was
    /// "running 2 miles" from the top and "for another 10 miles" a minute
    /// later, on it (agent drive into Denver, 2026-09-24): the gentle grade
    /// below it was being counted as the seven percent.
    pub fn grade_run_mi(&self, start_mi: f64, sign: i32) -> f64 {
        self.grade_run_over_mi(start_mi, sign, GRADE_WARN_CLEAR_PCT)
    }

    /// [`Self::grade_run_mi`] with the floor named: the run lasts while the
    /// grade holds at least `floor_pct` in the direction of `sign`. Only a
    /// grade already gentler than the steep line's release is measured on a
    /// lower floor, and it has no steep run to disagree with.
    pub fn grade_run_over_mi(&self, start_mi: f64, sign: i32, floor_pct: f64) -> f64 {
        let mut run = 0.0;
        let mut probe = start_mi;
        while run < GRADE_WARN_SCAN_MI {
            probe += GRADE_WARN_STEP_MI;
            if probe >= self.trip.total_miles() {
                break;
            }
            if self.trip.grade_at(probe) * sign as f64 * 100.0 < floor_pct {
                break;
            }
            run += GRADE_WARN_STEP_MI;
        }
        run
    }

    /// Call out a steep grade before the truck is committed to it.
    ///
    /// A downgrade is the one piece of road a driver has to plan for -- gear
    /// and retarder chosen at the top, not halfway down -- and nothing spoke
    /// it. Cruise would quietly run well over the set speed and the first
    /// news of the hill was the speeding warning (playtest, 2026-07-27).
    /// One advisory per grade, cleared once the road flattens out.
    ///
    /// Terse speech gets none of them. A driver on terse has asked for the
    /// road to stay quiet, and the grade is available on demand from the G
    /// key any time they want it -- so this is exactly the kind of unrequested
    /// commentary the setting exists to remove. Cruise still speaks up when a
    /// grade has beaten it, terse or not: that one is not commentary, it is
    /// the controller reporting it has stopped doing its job.
    pub fn update_grade_advisory(&mut self, ctx: &mut GameContext) {
        if self.terse_speech(ctx) {
            return;
        }
        if self.trip.finished || self.trip.truck.speed_mph() < GRADE_WARN_MIN_MPH {
            return;
        }
        // Sampling the road profile is a scan over the leg's baked segments, so
        // it runs per tenth of a mile rather than per frame. The advisory looks
        // three quarters of a mile ahead; a tenth of that is no delay at all.
        if (self.trip.position_mi - self.grade_scan_mi).abs() < GRADE_WARN_RESCAN_MI {
            return;
        }
        self.grade_scan_mi = self.trip.position_mi;
        let here_pct = self.trip.grade_at(self.trip.position_mi) * 100.0;
        let ahead_mi = self.trip.position_mi + GRADE_WARN_LOOKAHEAD_MI;
        let ahead_pct = if ahead_mi < self.trip.total_miles() {
            self.trip.grade_at(ahead_mi) * 100.0
        } else {
            here_pct
        };
        // Take whichever of here and just-ahead is steeper, so a grade that
        // starts under the wheels is called out as promptly as one seen coming.
        let from_ahead = ahead_pct.abs() >= here_pct.abs();
        let pct = if from_ahead { ahead_pct } else { here_pct };
        if pct.abs() < GRADE_WARN_CLEAR_PCT {
            // Level both here and just ahead: between hills, so the next one
            // earns a cue. Clearing on the flat under the wheels alone re-armed
            // the advisory on every frame of the approach to a hill, which
            // spoke it over and over until the wheels reached the slope.
            self.grade_warned_sign = 0;
            return;
        }
        if pct.abs() < GRADE_WARN_PCT {
            return;
        }
        let sign = if pct > 0.0 { 1 } else { -1 };
        if self.grade_warned_sign == sign {
            return;
        }
        let run_mi = self.grade_run_mi(
            if from_ahead {
                ahead_mi
            } else {
                self.trip.position_mi
            },
            sign,
        );
        if run_mi < GRADE_WARN_MIN_RUN_MI {
            // A dip, not a hill. Deliberately without latching: a short blip
            // must not swallow the advisory for the real grade behind it.
            return;
        }
        self.grade_warned_sign = sign;
        // The scan gives up at its horizon, so say so rather than claiming the
        // grade ends exactly there.
        let about = if run_mi >= GRADE_WARN_SCAN_MI {
            "at least "
        } else {
            ""
        };
        let length = format!(" for {about}{}", self.trip.distance_text(run_mi));
        let direction = if sign > 0 {
            "uphill grade"
        } else {
            "downhill grade"
        };
        ctx.audio.play_with("ui/notify", 0.55, 0.0);
        // An uphill grade needs no advice: the truck slowing on it is the
        // expected thing, and saying so was one more sentence to sit through.
        let advice = if sign < 0 {
            self.descend_advice(ctx)
        } else {
            String::new()
        };
        ctx.say_event_with(
            format!(
                "{:.1} percent {direction} ahead{length}. {advice}",
                pct.abs()
            )
            .trim_end()
            .to_string(),
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Navigation),
        );
    }

    /// The horn's one real power: moving an animal off the road.
    ///
    /// Shane's ask (2026-08-20), and it is what the air horn is FOR on a
    /// real highway -- but it works the way animals work, not the way a
    /// button works. Livestock, dogs, and coyotes mostly move; deer and
    /// elk freeze as often as they bolt, so the horn is a bonus, never the
    /// plan. One attempt
    /// per hazard: an animal that ignored the first blast has decided.
    /// Seeded on the hazard so a save-scummed retry hears the same deer
    /// make the same choice.
    pub fn horn_scare_animals(&mut self, ctx: &mut GameContext) {
        if self.hazard_deadline.is_none() || self.horn_scare_tried {
            return;
        }
        let names: Vec<String> = self
            .hazard_names
            .iter()
            .filter(|name| {
                HAZARDS
                    .iter()
                    .any(|hazard| hazard.animal && hazard.name == name.as_str())
            })
            .cloned()
            .collect();
        if names.is_empty() {
            return; // a ladder does not care how loud you are
        }
        self.horn_scare_tried = true;
        let mut rng =
            PyRandom::new_from_i64((self.trip_seed << 8) ^ (self.trip.position_mi * 50.0) as i64);
        let mut cleared = true;
        for name in &names {
            let freeze_prone = name == "the deer" || name == "the elk";
            let threshold = if freeze_prone { 0.4 } else { 0.7 };
            if rng.random() >= threshold {
                cleared = false;
            }
        }
        if !cleared || names.len() != self.hazard_names.len() {
            // Frozen in the headlights, or something unscareable is out
            // there too. Say nothing: the hazard machinery's own countdown
            // is still the instruction, and a "it did not work" line would
            // talk over the braking the driver should be doing.
            return;
        }
        let text = format!(
            "The horn does it, {} clears the road.",
            self.hazard_names_text()
        );
        self.finish_hazard_clear(ctx, &text);
    }

    /// An animal the truck reached before slowing to its brake-to speed:
    /// most get across, and the rest are struck. A strike is a dent and a
    /// grille, not the wreck a fixed object at speed is: the truck keeps
    /// going and the freight is untouched (owner, 2026-10-19).
    ///
    /// The odds are assumed, not measured: at 65 mph the horn's own split,
    /// where deer and elk freeze more often than they bolt, and better the
    /// slower the truck is going, so braking for it counts even when it
    /// falls short. The damage scales with the animal's size and the
    /// truck's speed. Seeded on the hazard, so a reload hears the same
    /// outcome.
    pub fn animal_outcome(&mut self, ctx: &mut GameContext) {
        let names = self.hazard_names.clone();
        let mut rng = PyRandom::new_from_i64(
            (self.trip_seed << 9) ^ (self.hazard_arm_mi * 50.0) as i64 ^ 0x5eed,
        );
        let slowed = (1.0 - self.trip.truck.speed_mph() / 65.0).clamp(0.0, 1.0);
        let struck = names.iter().find(|name| {
            let freeze_prone = name.as_str() == "the deer" || name.as_str() == "the elk";
            let at_speed = if freeze_prone { 0.6 } else { 0.75 };
            let gets_across = at_speed + (1.0 - at_speed) * slowed;
            rng.random() >= gets_across
        });
        let Some(struck) = struck.cloned() else {
            self.clear_hazard(ctx);
            return;
        };
        let size_pct = match struck.as_str() {
            "the elk" | "the livestock" => ANIMAL_STRIKE_LARGE_PCT,
            "the deer" => ANIMAL_STRIKE_DEER_PCT,
            _ => ANIMAL_STRIKE_SMALL_PCT,
        };
        let pct = size_pct * (self.trip.truck.speed_mph() / 65.0).clamp(0.2, 1.2);
        self.hazard_deadline = None;
        self.hazard_lead_mph = None;
        self.hazard_in_lane = false;
        self.hazard_animal = false;
        self.hazard_brake_to_mph = None;
        self.hazard_names.clear();
        self.refresh_live_facts(); // same reason as `finish_hazard_clear`
        self.release_hazard_brake();
        ctx.audio.play_with("vehicle/collision", 0.6, 0.0);
        ctx.controller.rumble.impact(0.3);
        self.trip.truck.add_damage(pct, false);
        let damage = self.trip.truck.damage_pct;
        let message = format!("Collision with {struck}. Damage {damage:.0} percent.");
        self.last_event_message = message.clone();
        self.co_driver_animal_strike(ctx, &struck);
        let spoken_pct = damage;
        ctx.say_event_with(
            message,
            SayEvent::new()
                .category(SpeechCategory::Safety)
                .valid(move || (live::damage_pct() - spoken_pct).abs() < 0.5),
        );
    }

    /// The pending hazard(s), joined for a resolution line.
    ///
    /// Falls back to "it" when nothing was recorded -- a hazard armed by
    /// test or tool code that pokes `hazard_deadline` directly rather
    /// than going through `handle_trip_event` -- so the old generic
    /// wording still comes out rather than an empty name.
    pub fn hazard_names_text(&self) -> String {
        let names = &self.hazard_names;
        match names.len() {
            0 => "it".to_string(),
            1 => names[0].clone(),
            2 => format!("{} and {}", names[0], names[1]),
            _ => format!(
                "{}, and {}",
                names[..names.len() - 1].join(", "),
                names[names.len() - 1]
            ),
        }
    }

    /// The hazard is behind the truck. Only the outcome: the driver did the
    /// braking or the easing around and does not need it narrated back --
    /// and narrating it is how this line went untrue before (a truck that
    /// matched a vehicle's speed was told it had nearly stopped and eased
    /// around; Brandon, 2026-08-23).
    pub fn hazard_resolution_text(&self) -> String {
        let names = self.hazard_names_text();
        if names == "it" {
            return "Clear.".to_string();
        }
        format!("Clear of {names}.")
    }

    /// Common tail of every way a pending hazard can resolve: brake,
    /// swerve, or an earlier hazard outrun before a new one armed.
    pub fn finish_hazard_clear(&mut self, ctx: &mut GameContext, message_text: &str) {
        // What the co-driver did about it, read before the resets below.
        let co_driver_news = self
            .co_driver_driving()
            .then(|| (self.hazard_names_text(), self.hazard_animal));
        self.hazard_deadline = None;
        // The lead belonged to the hazard that just ended; a fresh one must
        // not inherit it, or a tyre carcass would clear at the speed of a
        // truck that is long gone. Same for what it was: a hazard that spans
        // the road must not inherit the near stop the last object asked for.
        self.hazard_lead_mph = None;
        self.hazard_in_lane = false;
        self.hazard_animal = false;
        self.hazard_brake_to_mph = None;
        // Everything below here can offer the hazard call its rescue -- the
        // clear line's own flush, or any urgent line that lands before the
        // next tick. That gate asks whether a hazard is still armed, so the
        // reading has to move with the deadline rather than wait for the next
        // frame: judged on last tick's answer it replays "Change lanes or
        // brake!" for a hazard that has just been dodged.
        self.refresh_live_facts();
        self.release_hazard_brake();
        self.hazard_slow_hint_said = false;
        ctx.audio.play_with("events/hazard_clear", 0.75, 0.0);
        ctx.controller
            .rumble
            .alert_with(0.4, ff_core::rumble::ALERT_DURATION_MS);
        let message = terse_silent(message_text);
        self.last_event_message = message.normal.clone();
        // ROUTE, not the ambient default: this is the outcome of a SAFETY
        // event the driver just acted on, and at AMBIENT it queued behind
        // the urgent call that preceded it and was dropped as stale -- in
        // STANDARD mode, where confirmations are supposed to speak in full
        // (Shane, Killeen-Del Rio run, 2026-08-20: found the swerve-clear
        // only in the review keys). Same promotion the creep guidance and
        // the ramp-light family already earned for the same failure.
        ctx.say_event_with(
            message,
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Confirmation),
        );
        ctx.award_driving_achievement("hazard_avoided");
        self.hazard_names.clear();
        if let Some((names, animal)) = co_driver_news {
            self.co_driver_note_hazard(ctx, &names, animal, message_text);
        }
    }

    /// Speak and reset the pending hazard(s) as cleared by braking.
    ///
    /// Shared by the per-frame resolution below and an early resolution
    /// triggered from `handle_trip_event` when a fresh hazard arms
    /// while an earlier one was already outrun -- either way the driver
    /// gets exactly one clean "you made it" line naming what it was for.
    ///
    /// In terse the hazard-clear earcon IS the confirmation; the words are
    /// congratulation, and the failure outcome stays distinct as the
    /// collision sound plus its spoken damage line (R4, R14).
    pub fn clear_hazard(&mut self, ctx: &mut GameContext) {
        let text = self.hazard_resolution_text();
        self.finish_hazard_clear(ctx, &text);
    }

    pub fn update_hazard(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.hazard_deadline.is_none() {
            return;
        }
        let target = self.hazard_target_mph(None);
        if self.trip.truck.speed_mph() <= target {
            self.clear_hazard(ctx);
            return;
        }
        // Old instinct says 25 clears everything; for a fixed object it no
        // longer does. Braking past the moving-hazard speed with the object
        // still in the lane earns the how-to once, so the quiet is never
        // read as an already-cleared hazard.
        // "It is still in your lane. Nearly stop." belongs to a THING sitting
        // in the lane, which is the case whose target really is the near
        // stop. A vehicle is cleared by matching it, so telling the driver to
        // nearly stop for one would be an instruction the assist is not
        // following.
        if self.hazard_in_lane
            && self.hazard_lead_mph.is_none()
            && self.hazard_brake_to_mph.is_none()
            && !self.hazard_slow_hint_said
            && self.trip.truck.speed_mph() <= HAZARD_SAFE_MPH
        {
            self.hazard_slow_hint_said = true;
            // "Or change lanes" only names a maneuver this road offers, and
            // names the side it is open on -- the same reading as the call
            // itself (owner, 2026-09-01). A one-lane stretch, a two-lane one
            // with the other lane coned off, or both neighbours held by
            // traffic, gets nearly-stop as the whole answer.
            let side = self.trip.open_side_at(None);
            let hint = if side.is_open() {
                format!(
                    "It is still in your lane. Nearly stop, or change lanes. {}",
                    self.trip.open_side_spoken(side)
                )
            } else {
                "It is still in your lane. Nearly stop.".to_string()
            };
            // ROUTE: not interrupting (it follows the hazard call rather
            // than cutting it) but never droppable. A live hazard still in
            // your lane telling you to nearly stop is the last line in the
            // game that may be binned as stale chatter -- and the stale-drop
            // branch tests PRIORITY, never category, so SAFETY at the ambient
            // default was droppable in exactly the busy moment it matters.
            ctx.say_event_with(
                hint,
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Safety),
            );
        }
        let deadline = self.hazard_deadline.unwrap_or(0.0) - dt;
        self.hazard_deadline = Some(deadline);
        self.track_assisted_deceleration(dt);
        // An animal is braked for firmly in the lane, to the speed its
        // catalog row asks for, and never with the emergency application:
        // a loaded truck stood on its brakes for a deer is how a following
        // car ends up in the trailer (owner, 2026-10-19).
        let assist_may_act =
            ctx.settings.automatic_emergency_braking && !self.dodge_still_beats_the_hazard();
        if !assist_may_act {
            // A driver mid-drift has answered the warning, and an assist the
            // driver has switched off has no truck to take.
            self.release_hazard_brake();
        } else if self.aeb_brake > 0.0 || deadline <= self.aeb_engage_s(target) {
            if self.aeb_brake <= 0.0 {
                // Seed the measurement with the stop the budget has just
                // promised, so the smoothing starts from an honest prior
                // instead of climbing out of a standstill and reading the
                // first fifth of a second of a good stop as a failure.
                self.aeb_decel_mps2 = 0.0f64
                    .max(self.trip.truck.full_service_decel_mps2() + G * self.trip.truck.grade);
                self.aeb_hold_s = 0.0;
                self.aeb_losing_s = 0.0;
            }
            // Full SERVICE braking, and once it is on it stays on until the
            // hazard is answered. Deciding it afresh every frame is what fanned
            // the pedal: the assist's own braking retreats the very threshold
            // that engaged it, so it let go, the threshold came back, and the
            // air system was charged a whole brake application every time round
            // -- which is how ordinary assisted driving ran the tanks down.
            self.aeb_brake = 1.0;
            // And the emergency application stays a genuine last resort, judged
            // on the deceleration the truck is actually making rather than on
            // the one a full application ought to deliver.
            if self.service_braking_is_losing(dt) && !self.hazard_animal {
                self.aeb_emergency = true;
            }
            self.apply_hazard_brake();
            if !self.automatic_braking_announced {
                self.automatic_braking_announced = true;
                // Kept out of the reviewable log: this line interrupts the
                // hazard warning, and the review keys exist to give that
                // warning back, not the assist that talked over it.
                ctx.say_event_with(
                    "Automatic braking.",
                    SayEvent::new()
                        .review(false)
                        // The assist reporting that IT acted, not a demand on
                        // the driver -- the braking is audible and the hazard
                        // warning that preceded it carried the action. At
                        // quiet, where the rule is "speak what the player must
                        // do something about" (owner, 2026-08-17), that makes
                        // it an earcon. The hazard call itself stays SAFETY
                        // and speaks at every rung.
                        .category(SpeechCategory::Confirmation),
                );
            } else if self.aeb_emergency && !self.automatic_braking_escalated {
                self.automatic_braking_escalated = true;
                ctx.say_event_with(
                    "Emergency braking engaged.",
                    SayEvent::new()
                        .review(false)
                        .category(SpeechCategory::Safety),
                );
            }
            if self.cruise_mph.is_some() {
                // The session stays armed: a hazard stop on the highway
                // pauses cruise, and the resume path brings it back once
                // the truck is rolling and off the brakes. Disarming here
                // left the truck coasting to 5 km/h on open I-90 with the
                // driver still believing cruise had it (owner ruling,
                // 2026-09-01: a highway hazard must not disable cruise).
                self.cancel_cruise(ctx, true);
            }
        }
        if self.hazard_deadline.unwrap_or(0.0) <= 0.0 && self.hazard_animal {
            self.animal_outcome(ctx);
            return;
        }
        if self.hazard_deadline.unwrap_or(0.0) <= 0.0 {
            self.hazard_deadline = None;
            self.refresh_live_facts(); // same reason as `finish_hazard_clear`
            self.release_hazard_brake();
            ctx.audio.play("vehicle/collision");
            let mut severity = 1.0f64.min(self.trip.truck.speed_mph() / 70.0);
            severity *= tuning_for_time_scale(self.trip.time_scale).collision_damage;
            ctx.controller.rumble.impact(severity);
            self.trip.truck.apply_collision(severity, true);
            let damage = self.trip.truck.damage_pct;
            let mut message = format!("Collision! Damage {damage:.0} percent.");
            // A dodgeable hazard's announcement leaves the session armed --
            // see `handle_trip_event` -- on the promise that only braking
            // ends it: the driver's own, or AEB's (which already cancels the
            // instant it takes the pedal, above). With AEB off and no dodge
            // and no brake, neither of those ever fires, and the hazard rode
            // cruise straight into the collision with the session still
            // showing armed (reviewer-caught regression on the announce-time
            // fix, 2026-08-14). The deadline lapsing un-dodged is the third
            // way the promise ends, whatever the AEB setting -- the hazard
            // stopped being answerable the moment it hit the truck.
            if self.speed_control_armed || self.cruise_mph.is_some() || self.keeper_mph.is_some() {
                self.disarm_speed_control(ctx);
                message.push_str(" Cruise off.");
            }
            self.last_event_message = message.clone();
            // valid: a damage total is only true at the moment it was
            // computed. A rescue replaying it after ANOTHER collision stated
            // a percentage the truck had already left behind (adversarial
            // battery: "spoke 44% while the truck was at 51%").
            let spoken_pct = self.trip.truck.damage_pct;
            self.refresh_live_facts();
            ctx.say_event_with(
                message,
                SayEvent::new()
                    .category(SpeechCategory::Safety)
                    .valid(move || (live::damage_pct() - spoken_pct).abs() < 0.5),
            );
        }
    }
}
