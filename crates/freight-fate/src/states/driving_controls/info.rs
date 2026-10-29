//! The road and speed keys: Space, S, D, A, G, U, F, V and I. The clock and
//! the hours keys are in [`super::clock`]; the Tab status browse and the two
//! readouts it shares with these are in [`super::status`].

use ff_core::data::curves::RouteCurve;
use ff_core::sim::trip::Trip;
use ff_core::sim::trip_models::RoadStop;

use crate::app::{GameContext, Say};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use super::{SAFE_SPEED_CURVE_MI, UPCOMING_MAX_CLAUSES};

impl DrivingState {
    /// `_keeper_holding_text()`: what the speed keeper is holding RIGHT NOW,
    /// for a readout.
    ///
    /// The keeper's set speed and the speed it is holding are two numbers
    /// whenever it is easing for something ahead -- a corner's advise, a lower
    /// limit, the gate zone -- and the readout spoke only the first. S said
    /// "holding 25" while the truck held 15 down a facility street, which
    /// reads as the keeper ignoring the road (New Haven log; owner, Spokane,
    /// 2026-08-22: "the truck slows to 15 while the speed stays 25"). Say the
    /// live number, and the set one only when it differs.
    pub fn keeper_holding_text(&self, ctx: &GameContext) -> String {
        let held = self.keeper_mph.unwrap_or(0.0);
        if let Some((at_mi, eased, why)) = self.keeper_ease_target.as_ref() {
            if self.trip.position_mi < *at_mi && *eased < held {
                let reason = match why.as_str() {
                    "turn" => "for the corner".to_string(),
                    "posted limit" => "for the lower limit".to_string(),
                    other => format!("for the {other}"),
                };
                return format!(
                    "speed keeper holding {} {reason}, set {}",
                    ctx.settings.speed_text(*eased),
                    ctx.settings.speed_text(held)
                );
            }
        }
        // A lead vehicle setting the number: the loop publishes what it held
        // and why, as cruise does, so the readout names the car instead of
        // answering with a set speed the road is not allowing (a slow car
        // held 51 through twenty miles of "holding 58", 2026-09-11).
        if !self.keeper_held_reason.is_empty() {
            if let Some(live) = self.keeper_held_mph.filter(|live| *live < held - 0.5) {
                return format!(
                    "speed keeper holding {} {}, set {}",
                    ctx.settings.speed_text(live),
                    self.keeper_held_reason,
                    ctx.settings.speed_text(held)
                );
            }
        }
        format!("speed keeper holding {}", ctx.settings.speed_text(held))
    }

    /// What adaptive cruise is holding RIGHT NOW, for a readout.
    ///
    /// The same two numbers the keeper's readout above already separates, for
    /// the other controller. The engage and resume line has named the number
    /// the truck will actually hold since 2026-08-20 (Brandon: cruise said
    /// "resuming at 70" and held 23 through a zone); the status keys never
    /// did. Owner's own session log, New York, 2026-08-23 -- one second apart:
    ///
    ///   Open road. Adaptive cruise resuming at 33 miles per hour for the ramp.
    ///   44 miles per hour, gear 9, 1324 RPM, ... adaptive cruise set at 80
    ///   miles per hour
    ///
    /// A driver pressing the status key mid-ramp was answered with a number
    /// nothing on the road was going to allow. Say the live one, and the set
    /// one only when a ceiling is holding it down.
    ///
    /// The number comes from the loop, which publishes what it actually held
    /// after every cap -- the armed ramp, a bend, a lower posted limit, a
    /// zone, the grade, the weather, the lead vehicle. Re-deriving it from a
    /// key press would mean re-running look-aheads that latch state.
    pub fn cruise_holding_text(&self, ctx: &GameContext) -> String {
        let set = self.cruise_mph.unwrap_or(0.0);
        if let Some(held) = self.cruise_held_mph {
            // Half a mile an hour under is the same number once spoken.
            if held < set - 0.5 {
                let why = if self.cruise_held_reason.is_empty() {
                    String::new()
                } else {
                    format!(" {}", self.cruise_held_reason)
                };
                return format!(
                    "adaptive cruise holding {}{why}, set {}",
                    ctx.settings.speed_text(held),
                    ctx.settings.speed_text(set)
                );
            }
        }
        format!("adaptive cruise set at {}", ctx.settings.speed_text(set))
    }

    /// `_speak_speed()`: Space.
    ///
    /// Three short sentences rather than one run-on: the gauges, then the
    /// controller, then the air. The run-on named "automatic speed control"
    /// ahead of the controller that already names itself, spoke the default
    /// open-road target nobody set, and listed four normal air states every
    /// time (bscross32, 2026-10: "why did I need all of that in one
    /// message?"). What is normal goes unsaid; Tab still lists all of it.
    pub fn speak_speed(&mut self, ctx: &mut GameContext) {
        let gear = self.gear_text();
        // The open-road target only when one was set, and not when it is the
        // number the keeper was just said to be set at.
        let keeper_target = match self.speed_control_target_mph {
            Some(mph) if self.keeper_mph.is_none_or(|set| (set - mph).abs() >= 0.5) => {
                format!(" Open-road target {}.", ctx.settings.speed_text(mph))
            }
            _ => String::new(),
        };
        let control = if self.cruise_mph.is_some() {
            format!(" {}.", capitalize_first(&self.cruise_holding_text(ctx)))
        } else if self.keeper_mph.is_some() {
            format!(
                " {}.{keeper_target}",
                capitalize_first(&self.keeper_holding_text(ctx))
            )
        } else if self.speed_control_armed {
            format!(" Automatic speed control paused.{keeper_target}")
        } else {
            String::new()
        };
        let speed = ctx.settings.speed_text(self.trip.truck.speed_mph());
        // The tachometer matters to a driver who shifts; an automatic
        // shifts for them.
        let rpm = if self.trip.truck.transmission.automatic {
            String::new()
        } else {
            format!(", {:.0} RPM", self.trip.truck.rpm)
        };
        let air = match self.air_brief_text() {
            air if air.is_empty() => air,
            air => format!(" {}.", capitalize_first(&air)),
        };
        // With the signal on, how far to the exit is the number the driver is
        // waiting on, and until now only the countdown anchors said it (owner,
        // driving, 2026-09-24: "I should be able to see how far away from the
        // exit I am"). Asked for, never volunteered.
        // "set" until the blinker runs, "on" once it clicks, as the X line.
        let armed = match self.armed_exit() {
            Some(stop) => format!(
                " Signal {} for {}, {}.",
                self.signal_verb(),
                self.armed_exit_name(ctx, &stop),
                ctx.settings
                    .distance_text(stop.at_mi - self.trip.position_mi, true)
            ),
            None => String::new(),
        };
        ctx.say(&format!("{speed}, {gear}{rpm}.{control}{air}{armed}"));
    }

    /// The exit the signal is on for, while the truck is still short of its
    /// gore. `None` once on the ramp, where the ramp readouts take over.
    fn armed_exit(&self) -> Option<RoadStop> {
        if !self.exit_signal_on || self.exit_signal_canceled || self.ramp_mi.is_some() {
            return None;
        }
        self.exit_stop
            .clone()
            .filter(|stop| stop.at_mi > self.trip.position_mi)
    }

    fn signal_verb(&self) -> &'static str {
        if self.exit_blinker_on() {
            "on"
        } else {
            "set"
        }
    }

    /// The armed exit as Space and U name it, in the signal-on line's shapes:
    /// "exit 263, Main Street" for the destination, "exit 42, Flying J" or
    /// "the Flying J exit" for a stop. The signal-on line already gave the
    /// stop's full name, so the plain one is enough here.
    fn armed_exit_name(&mut self, ctx: &mut GameContext, stop: &RoadStop) -> String {
        if stop.stop_type != "delivery_destination" {
            return if stop.exit_label.is_empty() {
                format!("the {} exit", stop.name)
            } else {
                format!("{}, {}", stop.exit_label, stop.name)
            };
        }
        let phrase = self.exit_phrase_of(ctx, stop);
        let labeled = if phrase.is_empty() {
            stop.exit_label.clone()
        } else {
            phrase
        };
        if labeled.is_empty() {
            format!("the destination exit for {}", stop.name)
        } else {
            labeled
        }
    }

    /// `_speak_speed_limit()`: S -- the posted limit here, the zone if any,
    /// and how far over you are.
    ///
    /// On a signal-controlled ramp the light IS the law here, so S answers
    /// with the light and the distance to the stop bar instead -- the driver
    /// could never ask "where is the bar" before this (owner playtest,
    /// 2026-07-19: five stop-and-listen hops over 1300 feet).
    pub fn speak_speed_limit(&mut self, ctx: &mut GameContext) {
        if let Some(light) = self.ramp_light_query_text(ctx) {
            ctx.say(&light);
            return;
        }
        // Same idea at a facility gate: once the route has ended, the posted
        // limit stopped mattering -- the only thing S can honestly answer is
        // the gate and what to do about it. Without this, a driver rolling
        // past the entrance heard "42 miles per hour, limit 45" and nothing
        // about the delivery behind them (playtest 2026-07-22).
        if let Some(gate) = self.arrival_gate_query_text(ctx) {
            ctx.say(&gate);
            return;
        }
        let position = self.trip.position_mi;
        let (limit, reason) = self.trip.speed_limit_at(position);
        // A street is not a zone: its limit is its own (`spoken_zone`).
        let zone = match reason {
            Some(reason) if !ff_core::sim::trip::is_street_zone_reason(&reason) => {
                format!(", in a {reason} zone")
            }
            _ => String::new(),
        };
        let over = self.trip.truck.speed_mph() - limit;
        let comparison = if over >= 1.0 {
            format!(" About {} over.", ctx.settings.speed_text(over))
        } else {
            String::new()
        };
        // Split-limit states post one number for cars and a lower one for
        // rigs, so S saying only the truck figure reads as a wrong map to
        // anyone who remembers the shield (player report, 2026-07-19).
        // Name the state once and the 55 under a 65 sign explains itself.
        let (is_truck_limit, cap_state) = self.trip.truck_limit_at(position);
        let split = match cap_state {
            Some(state) => format!(" {state} holds trucks to this."),
            None => String::new(),
        };
        // A posted 55 through hairpin country is honest -- the yellow
        // diamond is advisory, not the limit -- but S saying only "55"
        // mid-canyon reads as nonsense, so name the bend's number too.
        // Mainline bends only: a connector arc at an interchange is excluded
        // from the curve call, the servo and the cargo model by design, so
        // naming its number here told the driver about a bend nothing acts on
        // (owner drive, I-30 to I-35 at Fort Worth).
        let curve = self.binding_mainline_curve();
        let advisory = match curve {
            Some(curve) if (curve.advisory_mph as f64) < limit => format!(
                " The bend here advises {}.",
                ctx.settings.speed_text(curve.advisory_mph as f64)
            ),
            _ => String::new(),
        };
        let lead = if is_truck_limit {
            "Truck limit"
        } else {
            "Speed limit"
        };
        let limit_text = ctx.settings.speed_text(limit);
        ctx.say(&format!(
            "{lead} {limit_text}{zone}.{split}{comparison}{advisory}"
        ));
    }

    /// The bend under the wheels, or the next one close ahead -- whichever
    /// binds. D asks this way: a connector arc counts while the truck is
    /// inside one, because the safe speed through it is real.
    fn binding_curve(&self) -> Option<RouteCurve> {
        if let Some(curve) = self.trip.curve_at(self.trip.position_mi) {
            return Some(curve);
        }
        self.trip
            .curves_within(SAFE_SPEED_CURVE_MI)
            .first()
            .copied()
    }

    /// The same question for S, mainline bends only. The posted-limit
    /// readout names a bend to explain a limit the assists honour; a
    /// connector arc is not one of those, so it is left out the way
    /// `curves_within` already leaves it out of the bends ahead.
    fn binding_mainline_curve(&self) -> Option<RouteCurve> {
        if let Some(curve) = self
            .trip
            .curve_at(self.trip.position_mi)
            .filter(|curve| !curve.connector)
        {
            return Some(curve);
        }
        self.trip
            .curves_within(SAFE_SPEED_CURVE_MI)
            .first()
            .copied()
    }

    /// `_speak_safe_speed()`: D -- one number, the speed that is safe right
    /// here, right now.
    ///
    /// Sits next to S on purpose: S answers "what is posted", D answers "what
    /// should I actually be doing". Weather grip, a steep downgrade, an
    /// approaching curve and, once on the ramp, the exit speed are baked into
    /// the math, never into the sentence, so
    /// the answer survives being heard exactly once at speed. Repeatable free.
    pub fn speak_safe_speed(&mut self, ctx: &mut GameContext) {
        let position = self.trip.position_mi;
        let (limit, _) = self.trip.speed_limit_at(position);
        let mut safe = limit.min(self.trip.weather.effects().safe_speed_mph);
        let mut context = "";
        // A steep downgrade has a safe speed of its own for this truck at this
        // weight -- the one descent control holds. Without it D answered the
        // posted 65 on a seven percent grade while descent control held 45.
        if let Some(descent) = self.safe_descent_here_mph() {
            if descent < safe {
                safe = descent;
                context = " for the grade";
            }
        }
        // The bend under the wheels, or the next one close ahead: whichever
        // binds, its advisory is the number that keeps the truck on the road.
        // Connector arcs count when the truck is inside one.
        if let Some(curve) = self.binding_curve() {
            if (curve.advisory_mph as f64) < safe {
                safe = curve.advisory_mph as f64;
                context = " for the bend";
            }
        }
        // On the ramp the exit speed governs. An armed exit still ahead does
        // not: the gore takes road speed, and the exit speed is braked for
        // past it (realistic exit, 2026-09-24).
        if self.ramp_mi.is_some() {
            safe = safe.min(self.ramp_cap_here_mph());
            context = ", the exit speed";
        }
        let spoken = ctx.settings.speed_text(safe);
        ctx.say(&format!("Safe speed {spoken}{context}."));
    }

    /// `_speak_last_announcement()`: A -- replay the last driving
    /// announcement, for one you missed.
    pub fn speak_last_announcement(&mut self, ctx: &mut GameContext) {
        if self.last_event_message.is_empty() {
            ctx.say_with(
                "No recent announcement to repeat.",
                Say::new().review(false),
            );
            return;
        }
        ctx.stop_event_speech();
        // Already in the log from when it was first announced; logging the
        // replay too would leave a duplicate for the reviewer to step over.
        let message = self.last_event_message.clone();
        ctx.say_with(message, Say::new().review(false));
    }

    /// Alt C -- the last CB call, said again.
    ///
    /// A narrower question than A's "what was the last thing the road said",
    /// and a different one: CB chatter is about what is sitting up the road,
    /// and it goes past once. A single landmark, lane count or weather
    /// change after it takes the A slot away, which leaves a driver who
    /// missed the call nothing to act on (Sarah A., issue 156).
    ///
    /// Only the voice comes back. The squelch that marks a CB call is an
    /// earcon for the moment it arrived, and the moment is not what is being
    /// asked for.
    ///
    /// The distance is re-derived at the truck's position NOW, so a repeat
    /// two miles later says two miles, not the four it said at the time; a
    /// post already behind says so instead of counting down to nothing.
    pub fn speak_last_cb_chatter(&mut self, ctx: &mut GameContext) {
        let Some(recall) = self.last_cb_chatter.clone() else {
            ctx.say_with("No CB chatter to repeat.", Say::new().review(false));
            return;
        };
        let text = match recall.post.as_ref() {
            // Chatter with no post behind it carries no distance to go
            // stale, so it comes back word for word.
            None => recall.text.clone(),
            Some(post) => {
                let ahead = post.watch_start_mi() - self.trip.position_mi;
                if ahead <= 0.0 {
                    // Past it. The CB's own words would now be a lie, and
                    // "you have passed it" is the fact the driver is after.
                    format!(
                        "The CB called an enforcement post {}. You have passed it.",
                        Trip::cb_side(post)
                    )
                } else if post.tableau {
                    self.trip.cb_tableau_message(post, ahead)
                } else {
                    self.trip.cb_patrol_message(post, ahead)
                }
            }
        };
        // Asking for a repeat is a deliberate act: the event voice gives way
        // rather than talking over the line that was asked for.
        ctx.stop_event_speech();
        // Already in the log from when the CB first said it; logging the
        // repeat would leave the reviewer a duplicate to step over.
        ctx.say_with(text, Say::new().review(false));
    }

    /// `_toggle_lane_locator()`: I -- a periodic panned tock marking where
    /// the truck sits in its lane.
    ///
    /// Opt-in and player-summoned, which is what keeps it inside the community
    /// ruling against continuous steering tones: nothing plays unless the
    /// driver asked, and I again turns it off.
    pub fn toggle_lane_locator(&mut self, ctx: &mut GameContext) {
        if ctx.settings.lane_is_automated() {
            ctx.say("Lane keeping on full.");
            return;
        }
        self.lane_locator_on = !self.lane_locator_on;
        self.lane_locator_timer = 0.0;
        let state = if self.lane_locator_on { "on" } else { "off" };
        ctx.say(&format!("Lane locator {state}."));
    }

    /// `_speak_grade()`: G -- the grade under the wheels, the next one, and
    /// what they mean.
    ///
    /// The verdict comes from the sim's own net-force balance, so the spoken
    /// answer to "why am I slowing down" is the same physics the wheels feel
    /// -- including whether the jake has the descent or is about to lose it.
    /// The next grade ahead comes with it, so one press answers both "what am
    /// I on" and "what is coming" -- down to the gentler pull the speed
    /// preview plans for, which is quieter than the steep bar but is the
    /// reason the truck just said it was building speed.
    pub fn speak_grade(&mut self, ctx: &mut GameContext) {
        let grade = self.trip.truck.grade;
        let mut parts: Vec<String> = Vec::new();
        if grade.abs() < 0.005 {
            parts.push("Level road.".to_string());
        } else {
            let direction = if grade > 0.0 { "uphill" } else { "downhill" };
            let mut lead = format!("Grade {:.1} percent {direction}", grade.abs() * 100.0);
            // How far the slope keeps its character: the same run the grade
            // ahead is measured by ("running 2 miles"), so one pitch has one
            // length whichever sentence names it. A grade gentler than the
            // steep line runs until the road stops going its way at all.
            let sign = if grade > 0.0 { 1 } else { -1 };
            let floor_pct = if grade.abs() * 100.0 >= GRADE_WARN_CLEAR_PCT {
                GRADE_WARN_CLEAR_PCT
            } else {
                GRADE_READOUT_LEVEL_PCT
            };
            let run_mi = self.grade_run_over_mi(self.trip.position_mi, sign, floor_pct);
            if (1.0..GRADE_WARN_SCAN_MI).contains(&run_mi) {
                lead.push_str(&format!(" for another {}", self.trip.distance_text(run_mi)));
            }
            parts.push(format!("{lead}."));
        }
        let truck = &self.trip.truck;
        if truck.velocity_mps > 0.5 {
            // The same one reading the descent control judges a hill by, so a
            // driver can never be told the assist has lost a grade the G key
            // calls held (see `DrivingState::descent_is_beaten`).
            let accel_mph_s = truck.net_accel_mph_per_s();
            let stage = truck.engine_brake_stage;
            if grade > 0.005 {
                if accel_mph_s < -GRADE_HOLDING_MPH_PER_S {
                    parts.push("Losing speed.".to_string());
                } else if accel_mph_s > GRADE_HOLDING_MPH_PER_S {
                    parts.push("Gaining speed.".to_string());
                } else {
                    parts.push("Holding speed.".to_string());
                }
            } else if grade < -0.005 {
                if truck.jake_slipping() {
                    parts.push(
                        "The jake is sliding the drive wheels; back it off a stage.".to_string(),
                    );
                } else if accel_mph_s > GRADE_HOLDING_MPH_PER_S {
                    if stage > 0 {
                        let losing = if truck.transmission.automatic {
                            "snub the brakes"
                        } else {
                            "gear down or snub the brakes"
                        };
                        parts.push(format!("Jake stage {stage} is not holding it; {losing}."));
                    } else if truck.throttle <= 0.05 {
                        parts.push("Speed building. Set the jake.".to_string());
                    }
                } else if stage > 0 && truck.jake_brake_force() > 0.0 {
                    parts.push(format!("Jake stage {stage} has it."));
                } else {
                    parts.push("Holding speed.".to_string());
                }
            }
        }
        parts.push(self.next_grade_text(ctx));
        let joined: Vec<String> = parts.into_iter().filter(|part| !part.is_empty()).collect();
        ctx.say(&joined.join(" "));
    }

    /// `_next_grade_text()`: the next grade worth planning for, and how far
    /// off it is.
    pub fn next_grade_text(&self, ctx: &GameContext) -> String {
        let position = self.trip.position_mi;
        let mut probe = position + GRADE_WARN_STEP_MI;
        let here = self.trip.grade_at(position) * 100.0;
        let mut here_sign = if here >= GRADE_WARN_CLEAR_PCT {
            1
        } else if here <= -GRADE_WARN_CLEAR_PCT {
            -1
        } else {
            0
        };
        let mut here_pct = if here_sign != 0 { here.abs() } else { 0.0 };
        let horizon = self.trip.total_miles().min(position + GRADE_WARN_SCAN_MI);
        while probe < horizon {
            let pct = self.trip.grade_at(probe) * 100.0;
            let sign = if pct > 0.0 { 1 } else { -1 };
            // The grade already under the wheels is the first sentence's job;
            // this one starts at the next change of character -- or at the
            // point the same grade turns into a materially worse one, which
            // never changes sign and so was never spoken at all.
            let turned = sign != here_sign;
            let steepened = !turned && pct.abs() >= here_pct + GRADE_WARN_STEEPEN_PCT;
            if pct.abs() >= GRADE_WARN_PCT && (turned || steepened) {
                let run_mi = self.grade_run_mi(probe, sign);
                // Same filter the advisory uses: a third-of-a-mile dip is not
                // the next grade, it is a bump in this one.
                if run_mi >= GRADE_WARN_MIN_RUN_MI {
                    let direction = if sign > 0 {
                        "uphill grade"
                    } else {
                        "downhill grade"
                    };
                    let ahead = probe - position;
                    let distance = if ahead >= GRADE_WARN_MIN_RUN_MI {
                        format!("in {}", self.trip.distance_text(ahead))
                    } else {
                        "just ahead".to_string()
                    };
                    let running = format!("running {}.", self.trip.distance_text(run_mi));
                    if steepened {
                        return format!(
                            "It steepens to {:.1} percent {distance}, {running}",
                            pct.abs()
                        );
                    }
                    return format!(
                        "Next, a {:.1} percent {direction} {distance}, {running}",
                        pct.abs()
                    );
                }
            }
            if pct.abs() < GRADE_WARN_CLEAR_PCT {
                here_sign = 0;
                here_pct = 0.0;
            }
            probe += GRADE_WARN_STEP_MI;
        }
        let scanned = 0.0f64.max(GRADE_WARN_SCAN_MI.min(self.trip.total_miles() - position));
        // "Nothing steep ahead" in the same breath as a six percent grade
        // under the wheels reads as the game contradicting itself.
        let nothing = if here.abs() >= GRADE_WARN_PCT {
            "Nothing else steep".to_string()
        } else {
            "Nothing steep".to_string()
        };
        let upcoming = self.mild_grade_text(ctx, here);
        if !upcoming.is_empty() {
            return upcoming;
        }
        // Under a mile of road left to scan -- the streets to a gate -- whole
        // miles said "Nothing steep in the next 0 miles" (live drive into
        // Abilene, 2026-09-24): the rest of the route is all there is.
        if scanned < 1.0 {
            return format!("{nothing} ahead.");
        }
        format!(
            "{nothing} in the next {}.",
            self.trip.distance_text(scanned)
        )
    }

    /// The grade the preview is planning for,
    /// announced directly when none passes the sustained
    /// steep scan.
    ///
    /// Automatic speed control banks momentum for a two percent pull and says
    /// so; with nothing steep in fifteen miles, G had nothing to say back and
    /// the two answers looked like a bug (tester report, 2026-08-15). Read off
    /// the preview's own scan so both describe the same hill.
    pub fn mild_grade_text(&self, ctx: &GameContext, here_pct: f64) -> String {
        let Some((grade, ahead_mi)) = self.preview_grade_ahead() else {
            return String::new();
        };
        // A grade already under the wheels is the first sentence's job.
        if here_pct.abs() >= PCC_GRADE_MIN * 100.0 && (grade > 0.0) == (here_pct > 0.0) {
            return String::new();
        }
        let pct = grade.abs() * 100.0;
        let direction = if grade > 0.0 {
            "uphill grade"
        } else {
            "downhill grade"
        };
        let where_text = ctx.settings.short_distance_text(ahead_mi);
        format!("Next, a {pct:.1} percent {direction} starts in {where_text}.")
    }

    /// `_speak_upcoming(within_mi=15.0)`: U -- the road ahead that no other
    /// key already answers.
    ///
    /// Deliberately four clauses at the very most. Every other key on the
    /// wheel answers one question, and U was reciting all of them: the listed
    /// exit is on the status screen, the posted limit and the bend under the
    /// wheels are S, the safe number is D, the grade is G, and the route with
    /// its planned stop is R. What is left is what nothing else says -- the
    /// ramp control ahead, the next imposed limit, the next stop, and the next
    /// bend that will demand slowing (owner report, 2026-08-15: the drive is
    /// far too chatty).
    ///
    /// Enforcement is deliberately absent. Patrols and their CB reports reach
    /// the player on the CB, where the owner ruled they belong; U is the road,
    /// not the police.
    ///
    /// On a signal-controlled ramp the nearest upcoming thing is always the
    /// stop bar, so it leads the readout with the light's phase.
    pub fn speak_upcoming(&mut self, ctx: &mut GameContext, within_mi: f64) {
        let pos = self.trip.position_mi;
        let mut parts: Vec<String> = Vec::new();
        if let Some(light) = self.ramp_light_query_text(ctx) {
            parts.push(light.trim_end_matches('.').to_lowercase());
        }
        // The exit the signal is on for leads: it is the one thing on this
        // road the driver has already committed to (owner, 2026-09-24).
        let armed = self.armed_exit();
        if let Some(stop) = armed.as_ref() {
            let name = self.armed_exit_name(ctx, stop);
            parts.push(format!(
                "signal {} for {name}, in {}{}",
                self.signal_verb(),
                ctx.settings.distance_text(stop.at_mi - pos, true),
                Self::ramp_ending_clause(&self.ramp_control_for(ctx, stop, None))
            ));
        }
        let is_armed = |at_mi: f64| {
            armed
                .as_ref()
                .is_some_and(|a| (a.at_mi - at_mi).abs() < 0.001)
        };
        if let Some(zone) = self.trip.next_zone_within(within_mi) {
            let paired = if zone.reason == "construction merge" {
                self.trip
                    .zones
                    .iter()
                    .find(|z| z.reason == "construction" && (z.start_mi - zone.end_mi).abs() < 0.01)
                    .cloned()
            } else {
                None
            };
            if let Some(paired) = paired {
                // Which way to merge is the zone's to say. This line used to
                // read "merge left" whatever was coned off, so half the time
                // the readout sent the driver into the closed lane; where
                // nothing is closed it now says so instead of inventing a
                // merge.
                let merge = if zone.closed_side.is_some() {
                    let (shut, keep) = Trip::closure_phrases(&zone);
                    format!("{shut} lane closed, merge {keep}")
                } else {
                    "all lanes open".to_string()
                };
                parts.push(format!(
                    "reduced speed for construction in {}, {merge}, speed limit {}, then construction zone {}",
                    ctx.settings.distance_text(zone.start_mi - pos, true),
                    ctx.settings.speed_text(zone.limit_mph),
                    ctx.settings.speed_text(paired.limit_mph)
                ));
            } else {
                // The gate zone runs up to the gate, so its start is not where
                // the gate is: "facility gate in 0.3 miles" here and "three
                // quarters of a mile to the gate" from R (agent drive A,
                // 2026-09-24). The gate's clause reads the gate.
                let ahead = match self.gate_distance_mi(ctx) {
                    Some(gate) if zone.reason == "facility gate" => gate,
                    _ => zone.start_mi - pos,
                };
                // The next street's number, not a zone by name.
                if ff_core::sim::trip::is_street_zone_reason(&zone.reason) {
                    parts.push(format!(
                        "speed limit {} in {}",
                        ctx.settings.speed_text(zone.limit_mph),
                        ctx.settings.distance_text(ahead, true)
                    ));
                } else {
                    parts.push(format!(
                        "{} in {}, speed limit {}",
                        zone.reason,
                        ctx.settings.distance_text(ahead, true),
                        ctx.settings.speed_text(zone.limit_mph)
                    ));
                }
            }
        }
        // Every distance here is PRECISE. Whole miles bottom out at "0
        // miles" -- `distance_text`'s own docstring says the precise form
        // exists "where whole numbers would read as zero or lie by half a
        // mile" -- and this readout is exactly that case, because a driver
        // presses U most on the crawl into a facility. Shane P got "facility
        // gate in 0 miles", and before that watched it sit on "2 miles" for
        // three minutes while he closed on it (2026-08-23). The bend clause
        // below always asked for precise; the rest did not.
        //
        // Off the highway there is no next highway stop: on an exit ramp or
        // the streets, "Coming up: Flying J in 0.1 miles, where the ramp ends
        // at a traffic light" named a stop the truck had already left the
        // road for, and a ramp end that was a stop sign (agent drive, exit
        // 286A, 2026-09-23).
        let on_the_highway = self.ramp_mi.is_none() && !self.on_local_streets();
        // The destination exit is the most notable thing on the road, and it
        // is not one of the trip's stops: "Nothing notable in the next 15
        // miles." 1.2 miles before it (agent drives A and B, 2026-09-24).
        let destination = self
            .destination_exit_stop(ctx)
            .filter(|exit| on_the_highway && exit.at_mi - pos <= within_mi);
        if let Some(exit) = destination.as_ref().filter(|exit| !is_armed(exit.at_mi)) {
            let phrase = self.exit_phrase_of(ctx, exit);
            let labeled = if phrase.is_empty() {
                exit.exit_label.clone()
            } else {
                phrase
            };
            let named = if labeled.is_empty() {
                format!("the destination exit for {}", exit.name)
            } else {
                format!("the destination exit, {labeled},")
            };
            parts.push(format!(
                "{named} in {}{}",
                ctx.settings.distance_text(exit.at_mi - pos, true),
                Self::ramp_ending_clause(&self.ramp_control_for(ctx, exit, None))
            ));
        }
        if let Some(stop) = self
            .trip
            .upcoming_stop(within_mi)
            .cloned()
            .filter(|stop| on_the_highway && !is_armed(stop.at_mi))
            // Past the destination exit is road this truck is not driving.
            .filter(|stop| {
                destination
                    .as_ref()
                    .is_none_or(|exit| stop.at_mi < exit.at_mi)
            })
        {
            // The ramp's ending is part of the plan: a stop sign first heard
            // mid-ramp is too late to brake for.
            let ending = Self::ramp_ending_clause(&self.ramp_control_for(ctx, &stop, None));
            parts.push(format!(
                "{}{} in {}{ending}",
                self.trip.planned_prefix(&stop),
                stop.spoken_name(),
                ctx.settings.distance_text(stop.at_mi - pos, true)
            ));
        }
        // Traffic pressure is gone from this key: two of its three sources
        // restate the clause printed right beside them here -- the
        // construction taper's own squeeze, and the exit traffic for the stop
        // just named -- and the route merge speaks its own advisory on the
        // approach.
        //
        // The next listed exit is gone too. It had its own key until
        // 2026-08-17; now it is reference material on the status screen, which
        // is the same argument for keeping it out of here.
        //
        // The next bend that demands slowing stays, but one of them, not
        // three. S names the bend already under the wheels, D folds it into
        // the safe-speed number, and the pacenotes call each one before it
        // arrives; three bends with their advisories was the paragraph.
        let (limit, _) = self.trip.speed_limit_at(pos);
        let bend = self
            .trip
            .curves_within(within_mi)
            .into_iter()
            .find(|c| (c.advisory_mph as f64) < limit && c.severity() != "gentle");
        if let Some(bend) = bend {
            parts.push(format!(
                "{} in {}, advise {}",
                self.pacenote_phrase(&bend).to_lowercase(),
                ctx.settings.distance_text(bend.start_mi - pos, true),
                ctx.settings.speed_text(bend.advisory_mph as f64)
            ));
        }
        if parts.is_empty() {
            ctx.say(&format!(
                "Nothing notable in the next {}.",
                ctx.settings.distance_text(within_mi, false)
            ));
            return;
        }
        // The clause list cannot outgrow this today; the cap is here so a
        // later addition has to take something out rather than quietly growing
        // the readout back into a paragraph.
        parts.truncate(UPCOMING_MAX_CLAUSES);
        ctx.say(&format!("Coming up: {}.", parts.join(". ")));
    }

    /// ", where the ramp ends at a traffic light", or "" for a free-flowing
    /// ramp.
    fn ramp_ending_clause(control: &str) -> &'static str {
        match control {
            "signal" => ", where the ramp ends at a traffic light",
            "stop" => ", where the ramp ends at a stop sign",
            "yield" => ", where the ramp ends at a yield",
            "roundabout" => ", where the ramp ends at a roundabout",
            _ => "",
        }
    }

    /// `_speak_fuel()`: F.
    pub fn speak_fuel(&mut self, ctx: &mut GameContext) {
        let range_mi = self.fuel_range_mi();
        let fraction = self.trip.truck.fuel_fraction() * 100.0;
        let gallons = self.trip.truck.fuel_gal;
        ctx.say(&format!(
            "Fuel {fraction:.0} percent, {gallons:.0} gallons. Range about {}.",
            ctx.settings.distance_text(range_mi, false)
        ));
    }

    /// `_speak_weather()`: V -- conditions first, because the answer must lead
    /// for braille displays.
    pub fn speak_weather(&mut self, ctx: &mut GameContext) {
        let imperial = ctx.settings.imperial_units;
        let safe_speed = ctx
            .settings
            .speed_text(self.trip.weather.effects().safe_speed_mph);
        let tod = time_of_day(self.trip.local_hour());
        let mut parts = vec![format!("{}.", self.trip.weather.report_lead(imperial))];
        parts.push(format!("Safe speed about {safe_speed}."));
        if self.trip.weather.has_simulated_forecast() {
            let ahead: Vec<&str> = self
                .trip
                .weather
                .forecast(2)
                .into_iter()
                .map(|kind| kind.value())
                .collect();
            parts.push(format!("Ahead: {}.", ahead.join(", then ")));
        }
        parts.push(format!("It is {tod}."));
        ctx.say(&parts.join(" "));
    }
}

/// "speed keeper holding ..." as the start of its own sentence.
fn capitalize_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}
