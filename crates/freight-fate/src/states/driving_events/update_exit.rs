//! The per-frame advance of an armed exit or an active ramp, the destination
//! terminal loop-back, and the planned-stop stopping assist.

use ff_core::sim::trip_models::RoadStop;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use crate::states::driving_stops::assist_servo_brake;

impl DrivingState {
    /// Advance an armed exit or an active ramp; opens the stop menu.
    pub fn update_exit(&mut self, ctx: &mut GameContext, moved_mi: f64, dt: f64) {
        self.update_exit_with_input(ctx, moved_mi, dt, false);
    }

    /// Advance exits with the frame's keyboard/controller accelerator intent.
    pub fn update_exit_with_input(
        &mut self,
        ctx: &mut GameContext,
        moved_mi: f64,
        dt: f64,
        accelerating: bool,
    ) {
        // Real time from the gore to the terminal: while the ramp ends in
        // a live light or sign, the clock must not compress the seconds
        // the driver needs to brake for it.
        //
        // The same law on the way back UP. The acceleration lane out of a
        // facility's streets is a real length of road -- Green Book Table
        // 10-3, sized so a vehicle can build speed on it -- and it is spent
        // by the ground the truck covers, which is compressed. So the truck
        // built speed in real seconds while the lane ran out seven times too
        // fast. Measured over twenty-five chain departures: Abilene's 1790
        // feet went by in 12.8 real seconds, where a truck merely HOLDING the
        // 27 miles an hour it reached needs 45; it arrived at the taper 48
        // under the Interstate it was joining, and was told so as though that
        // were its own doing. Tapers came out at 14 to 28 miles an hour,
        // median 27 under the road, twenty-three of the twenty-five more than
        // 10 under. Nothing about the lane was wrong; the clock under it was.
        //
        // And the same law on the way OFF, on every exit whatever ends it:
        // the deceleration lane, the ramp curve and the run to the entrance
        // are real lengths of road too. On a free-flowing ramp at five times
        // the clock the lane went by in about two real seconds and a truck
        // met a 25 mph loop at 55 (review of the realistic exit, 2026-09-24);
        // see `on_laid_out_ramp` for the entrance.
        //
        // And while the exit lane stands open beside the truck: the seconds
        // between "Exit lane opening" and the steer into it are the driver's
        // to use, past the gore marker included.
        self.trip.controlled_ramp = self.controlled_ramp_reason().is_some();
        // The run-in to the dock, which is the destination ramp AND the
        // facility's own streets where it has them. Both are real time.
        //
        // The ramp half was here from the start; the street chain was not,
        // and that omission is the whole of "the approach assist works on
        // some legs and not others" (owner, 2026-08-24). Compression does not
        // speed the clock up, it moves the truck further per second --
        // `position_mi += velocity * dt * scale` -- so on a compressed road
        // the ground closes four to seven times faster than the brakes can
        // shed speed. The arrival assist prices its shed in real metres and
        // real seconds, which is exactly right on the ramp (pinned to real
        // time by this flag) and hopelessly optimistic on a chain, where the
        // scale also SNAPPED back from 1 to 7 the instant a corner released
        // and ate the last two hundred metres of the approach in a couple of
        // seconds. Measured over fifty destinations: every plain facility
        // docked at a crawl, while street-chain facilities crossed their own
        // gate at up to 24.7 miles an hour with the brake on the floor.
        //
        // That was the old physics, speed changing on the real clock while
        // the road passed on the compressed one. Now the truck brakes on the
        // clock that moves it, so a chain needs real time only where a turn
        // does: from the gate's brake point in. Pinned for all of it, five
        // miles of Abilene streets took ten real minutes at 30 (owner,
        // 2026-09-23: "Are we moving? Real-time is so slow at this speed").
        // Its turns pace themselves (`pace_clock_for_turn`).
        self.trip.dock_run_in = ((self.surface_chain || self.stop_chain.is_some())
            && self.at_the_gates_brake_point())
            || (self.ramp_mi.is_some()
                && self
                    .ramp_stop
                    .as_ref()
                    .is_some_and(|stop| stop.stop_type == "delivery_destination"));
        // And the road left to an exit the driver has signalled for, which the
        // clock reads to decide whether the approach itself is close enough to
        // be driven in real time. None once the ramp is taken -- from there
        // `controlled_ramp` owns the clock -- or once the exit is behind.
        // A canceled signal is not an approach. The stop itself is kept until
        // the truck passes it, so the game can say the exit went by unused --
        // but reading THAT as "still approaching an exit" held the clock in
        // real time all the way to a stop the driver had already begged off,
        // which from the seat is the road refusing to speed back up (Shane P,
        // 2026-08-21).
        let ahead_to_exit = match (&self.exit_stop, self.ramp_mi, self.exit_signal_canceled) {
            (Some(stop), None, false) => Some(stop.at_mi - self.trip.position_mi),
            _ => None,
        };
        self.trip.exit_approach_mi = ahead_to_exit.filter(|ahead| *ahead > 0.0);
        // A facility's own lights and signs, on its streets.
        self.update_street_controls(ctx, accelerating);
        if self.stop_chain.is_some() {
            // A selected stop's own assist brakes for its lot.
            self.update_selected_stop_assist(ctx);
            return;
        }
        if self.ramp_mi.is_some() {
            self.update_active_ramp(ctx, moved_mi, dt, accelerating);
            return;
        }
        self.update_armed_exit(ctx);
    }

    /// Which of the ramp laws above holds the clock at real time this frame,
    /// by name, or None. `trip.controlled_ramp` is this being Some; the name
    /// is what the session log gives for the pin (issue 293).
    pub fn controlled_ramp_reason(&self) -> Option<&'static str> {
        if self.departure_ramp_mi.is_some() {
            return Some("departure acceleration lane");
        }
        if self.departure_merge_recovery {
            return Some("departure merge recovery");
        }
        if self.lane.exit_lane_open {
            return Some("exit lane open");
        }
        if self.on_laid_out_ramp() {
            return Some("exit ramp");
        }
        if self.street_control_on_real_time() {
            return Some("street control");
        }
        self.ramp_mi?;
        if self
            .ramp_stop
            .as_ref()
            .is_some_and(|stop| stop.stop_type == "weigh_station")
        {
            return Some("weigh station ramp");
        }
        let controlled_terminal = matches!(
            self.ramp_control.as_str(),
            "signal" | "stop" | "yield" | "roundabout"
        );
        (controlled_terminal && !self.ramp_terminal_done).then_some("ramp terminal")
    }

    /// The `_ramp_mi is not None` half of `_update_exit`.
    fn update_active_ramp(
        &mut self,
        ctx: &mut GameContext,
        moved_mi: f64,
        dt: f64,
        accelerating: bool,
    ) {
        let ramp_mi = self.ramp_mi.expect("checked by the caller") - moved_mi;
        self.ramp_mi = Some(ramp_mi);
        // Not in the deceleration lane: the gore line has just named the
        // terminal, the exit speed is the job there, and the terminal's
        // servo (which waits for this callout) must not brake for a bar
        // while the lane's own braking is under way.
        if !self.ramp_light_announced
            && ramp_mi <= RAMP_CONTROL_ANNOUNCE_MI
            && !self.in_deceleration_lane()
        {
            self.announce_ramp_terminal(ctx);
            // A countdown mark the truck is already inside when the callout
            // lands is behind it: on a short or slow ramp the callout comes
            // at 900 feet, and "1000 feet." there is a distance that is not
            // true (review of the realistic exit, 2026-09-24).
            let gap_mi = ramp_mi - RAMP_ACCESS_MI;
            let unit_mi = if ctx.settings.imperial_units {
                1.0 / 5280.0
            } else {
                1.0 / 1609.344
            };
            for mark in self.ramp_bar_milestones(ctx) {
                if gap_mi < mark as f64 * unit_mi {
                    self.ramp_gap_milestones_said.insert(mark);
                }
            }
        }
        self.update_ramp_terminal_assist_with_input(ctx, accelerating);
        if self.update_selected_stop_assist(ctx) {
            return;
        }
        if !self.ramp_terminal_done && (ramp_mi <= RAMP_ACCESS_MI || self.stopped_at_the_bar()) {
            self.update_ramp_terminal(ctx);
        }
        if ramp_mi > 0.0 {
            return;
        }
        let stop = self
            .ramp_stop
            .clone()
            .expect("a ramp always carries a stop");
        if stop.stop_type == "delivery_destination"
            && self.ramp_terminal_done
            && self.begin_surface_chain(ctx, true)
        {
            // The street chain is a DRIVING continuation: hand off at
            // whatever legal speed the terminal let through. Gating the
            // handoff on docking speed marooned a green-light roll past
            // the end of the ramp -- the streets refused to start until
            // the driver stopped dead in the road (owner playtest,
            // 2026-07-24). The scripted dock-menu arrival below still
            // rightly waits for a crawl.
            self.ramp_mi = None;
            self.ramp_stop = None;
            self.ramp_control = String::new();
            return;
        }
        if stop.stop_type != "delivery_destination"
            && self.ramp_terminal_done
            && self.begin_stop_chain(ctx, &stop)
        {
            // A road stop past its own streets: the ramp's end is where they
            // begin, driven on at whatever the terminal let through.
            self.ramp_mi = None;
            self.ramp_stop = None;
            self.ramp_control = String::new();
            return;
        }
        if self.trip.truck.speed_mph() <= DOCKING_MAX_MPH {
            self.ramp_mi = None;
            self.ramp_stop = None;
            self.ramp_control = String::new();
            if stop.stop_type == "delivery_destination" {
                if self.begin_surface_chain(ctx, true) {
                    return;
                }
                self.trip.position_mi = self.trip.total_miles();
                self.trip.finished = true;
                self.open_facility_arrival(ctx);
            } else {
                self.open_poi_stop(ctx, &stop, true, None);
            }
            return;
        }
        if stop.stop_type == "delivery_destination"
            && self.destination_arrival_active
            && self.trip.truck.speed_mph() <= DELIVERY_PARK_MPH
        {
            // The destination approach assist is walking the truck over
            // the point and its own full brake at the point lands it a
            // moment later, so "come to a complete stop" here would be an
            // instruction the truck is already carrying out -- and at the
            // quiet rung an interrupt for it. The dock line that follows
            // names the place. A truck that somehow keeps rolling still
            // meets the blown-stop rule below once it is clear past.
            return;
        }
        if !self.ramp_end_said {
            if stop.stop_type == "delivery_destination"
                && !self.surface_chain
                && self.surface_chain_route(ctx).is_some()
            {
                // The facility has a street chain, so "you are at X"
                // here is a lie by two miles: the driver was told they
                // had arrived and then handed turn-by-turn streets
                // (owner log, 2026-07-23, Sacramento Dry Warehouse).
                // The chain's own "off the ramp and onto city streets"
                // line follows and says it right. The said-latch stays
                // open so the blown-stop rule below can never fire on a
                // chain facility: the streets are still the way in.
                return;
            }
            self.ramp_end_said = true;
            let terse = self.terse_speech(ctx);
            let message = if stop.stop_type != "delivery_destination" {
                let place = stop.spoken_name();
                // The assist is already braking the truck onto the scale:
                // "come to a stop" would ask for what it is doing.
                if self.scale_stop_is_assisted(ctx, &stop) {
                    format!("At {place}.")
                } else if terse {
                    format!("At {place}. Stop now.")
                } else {
                    format!("At {place}. Come to a stop.")
                }
            } else {
                let place = &stop.name;
                if terse {
                    format!("At {place}.")
                } else {
                    format!("At {place}. Come to a stop.")
                }
            };
            // Both kinds of ramp stop open the same real-time reaction
            // window. The destination opened none at all, so its grace sat
            // at zero forever and nothing downstream could ever read it as
            // spent (owner playtest, Buffalo to Albany, 2026-08-12).
            self.ramp_arrival_grace_s = self.ramp_arrival_grace_for(ctx, &message);
            let mut opts = SayEvent::new();
            opts.category = Some(SpeechCategory::Navigation);
            ctx.say_event_with(message, opts);
            return;
        }
        self.ramp_arrival_grace_s = 0.0f64.max(self.ramp_arrival_grace_s - dt);
        // Rolled clear past the end of the ramp without ever stopping. Both
        // the distance and the real-time grace must expire, so trip pacing
        // cannot consume the player's spoken-cue reaction window.
        if ramp_mi > -RAMP_OVERSHOOT_MI
            || self.ramp_arrival_grace_s > 0.0
            || self.trip.truck.parking_brake
        {
            return;
        }
        if stop.stop_type == "delivery_destination" {
            // The destination terminal used to be the one blown stop with
            // no consequence at all: the arrival line was spoken once, the
            // ramp counted down past it forever, and the player circled
            // with the route status frozen and nothing said until they quit
            // to the menu (owner playtest, Buffalo to Albany, 2026-08-12).
            self.loop_back_to_destination_terminal(ctx, &stop);
            return;
        }
        // A route POI is blown, so give the highway back instead of leaving
        // a stuck, unpatrolled ramp lingering for miles.
        self.ramp_mi = None;
        self.ramp_stop = None;
        self.ramp_end_said = false;
        self.ramp_arrival_grace_s = 0.0;
        let planned = self.trip.is_planned(&stop);
        if planned {
            self.trip.planned_stop_key = None;
        }
        if self.is_selected_stop(Some(&stop)) {
            self.clear_selected_stop_intent();
        }
        let exit_ref = if stop.exit_label.is_empty() {
            format!("the exit for {}", stop.spoken_name())
        } else {
            format!("{} for {}", stop.exit_label, stop.spoken_name())
        };
        let mut line = if self.terse_speech(ctx) {
            format!("Drove past {}; you never stopped.", stop.spoken_name())
        } else {
            format!("Drove past {exit_ref} without stopping.")
        };
        if planned {
            line.push_str(" Plan canceled.");
        }
        self.say_confirmation_interrupt(ctx, &line);
    }

    /// What to do about the exit that just went by, in one clause.
    ///
    /// An optional stop really can be picked up from a later exit, so "take a
    /// later exit" is true for one. The DESTINATION exit has no
    /// later exit to recover at: the route runs on to its end and the scripted
    /// loop-back through the safe turnaround brings this same exit back, which
    /// is what the manual has always described. Sending a driver looking for
    /// another exit sends them off the route, and then the loop-back a mile
    /// later reads as a reroute that was promised and never came (Tyler
    /// Rodick, Hattiesburg, 2026-08-26: "it never rerouted me").
    fn missed_exit_recovery(stop: &RoadStop) -> &'static str {
        if stop.stop_type == "delivery_destination" {
            "Destination exit comes around again."
        } else {
            "Take a later exit."
        }
    }

    /// The same promise as a tail, for the two lines that already say the
    /// truck stayed on the highway. Empty for an optional stop, whose lines
    /// deliberately leave the driver to choose whether to come back at all.
    fn missed_destination_note(stop: &RoadStop) -> &'static str {
        if stop.stop_type == "delivery_destination" {
            " The exit comes around again."
        } else {
            ""
        }
    }

    /// The `_ramp_mi is None` half of `_update_exit`.
    fn update_armed_exit(&mut self, ctx: &mut GameContext) {
        if self.exit_signal_canceled {
            self.exit_stop = None;
            self.cruise_exit_mph = None;
            self.reset_exit_lane_state();
            return;
        }
        let Some(stop) = self.exit_stop.clone() else {
            return;
        };
        // Steered into the exit lane where it opened: that lane is the start
        // of the ramp, so the truck is on it now, a few hundred feet short of
        // the gore marker or not.
        let steered_in = self.exit_lane_entered && self.exit_taper_said;
        if self.trip.position_mi < stop.at_mi && !steered_in {
            self.update_exit_countdown(ctx, &stop);
            return;
        }
        // In the right lane with the signal on and the exit lane open beside
        // it: the lane runs on past the marker, so the steer into it is still
        // the driver's to make until the gore window closes.
        if self.exit_lane_due(ctx, &stop)
            && self.trip.position_mi <= stop.at_mi + EXIT_COMMIT_WINDOW_MI
        {
            return;
        }
        self.exit_stop = None;
        // The exit is settled either way now, so the ramp cap comes off:
        // taking it pauses the session for the ramp, and missing it must not
        // leave automatic control crawling at ramp speed down the open highway.
        self.cruise_exit_mph = None;
        self.exit_signal_canceled = false;
        // Lined up but carried past the whole window in one step (a resumed
        // or compressed frame). A truck that never took the lane hears why
        // below instead.
        if self.trip.position_mi > stop.at_mi + EXIT_COMMIT_WINDOW_MI && self.exit_lane_ready() {
            self.reset_exit_lane_state();
            self.exit_signal_on = false;
            if self.is_selected_stop(Some(&stop)) {
                self.clear_selected_stop_intent();
            }
            let pressure = self.active_exit_pressure(&stop);
            let message = if pressure.is_some_and(|pressure| pressure.intensity >= 0.35) {
                "Missed the exit in heavy traffic."
            } else {
                "Missed the exit."
            };
            let note = Self::missed_destination_note(&stop);
            self.say_confirmation_event(ctx, &format!("{message}{note}"));
            return;
        }
        if !self.exit_intent_ready(ctx, &stop) {
            self.reset_exit_lane_state();
            self.exit_signal_on = false;
            if self.is_selected_stop(Some(&stop)) {
                self.clear_selected_stop_intent();
            }
            let place = self.missed_exit_phrase(ctx, &stop);
            let recovery = Self::missed_exit_recovery(&stop);
            self.say_confirmation_event(ctx, &format!("Missed {place}, no signal. {recovery}"));
            return;
        }
        if !self.exit_lane_ready() {
            self.reset_exit_lane_state();
            self.exit_signal_on = false;
            if self.is_selected_stop(Some(&stop)) {
                self.clear_selected_stop_intent();
            }
            let missed = self.missed_exit_phrase(ctx, &stop);
            let recovery = Self::missed_exit_recovery(&stop);
            let pressure = self.active_exit_pressure(&stop);
            if pressure.is_some() {
                self.say_confirmation_event(
                    ctx,
                    &format!("Boxed out of the exit lane, missed {missed}. {recovery}"),
                );
            } else {
                self.say_confirmation_event(
                    ctx,
                    &format!("Missed {missed}, not in the exit lane. {recovery}"),
                );
            }
            return;
        }
        // The stop goes in by hand: _exit_stop was cleared above, and without
        // it the gate fell back to the flat 45 and refused a truck doing the
        // road speed it had just been told was fine -- the third gate the
        // roadmap note said to look for (2026-08-21).
        if self.trip.truck.speed_mph() <= self.gore_acceptance_mph(Some(&stop)) {
            self.take_the_ramp(ctx, &stop);
        } else {
            let missed = self.missed_exit_phrase(ctx, &stop);
            let note = Self::missed_destination_note(&stop);
            let mut line = format!("Too fast for the ramp, missed {missed}.{note}");
            if self.trip.is_planned(&stop) {
                // Fold the plan cancellation into this one line so the driver
                // hears a single cue, and clear it here so _check_stops doesn't
                // also emit a "drove past your planned stop" warning next tick.
                self.trip.planned_stop_key = None;
                line.push_str(" Plan canceled.");
            }
            if self.is_selected_stop(Some(&stop)) {
                self.clear_selected_stop_intent();
            }
            self.say_confirmation_interrupt(ctx, &line);
            self.exit_signal_on = false;
            self.reset_exit_lane_state();
        }
    }

    /// The truck makes the gore: onto the ramp.
    fn take_the_ramp(&mut self, ctx: &mut GameContext, stop: &ff_core::sim::trip_models::RoadStop) {
        // A truck that steered in at the taper is up to 300 feet short of the
        // gore, and the ramp holds the highway odometer where it stands. The
        // exit lane leads to the gore, so the highway rejoins there: otherwise
        // the scale the truck just checked in at was crossed again on the way
        // out and charged as a bypass.
        self.trip.position_mi = self.trip.position_mi.max(stop.at_mi);
        // Whether the one-mile call already gave the exit speed, read before
        // the reset below clears the anchors.
        let speed_given_at_the_mile = self.exit_countdown_said.contains(&1.0);
        self.reset_exit_lane_state();
        self.exit_signal_on = false;
        // Gore to stop bar, then the terminal-to-driveway stretch: the
        // deceleration lane, the ramp curve and the run to the bar, laid out
        // from the Green Book (see `driving_events/decel_lane.rs`).
        let layout = self.trip.exit_ramp_layout(stop);
        self.ramp_layout = Some(layout);
        self.decel_lane_brake = 0.0;
        self.decel_lane_assist_said = false;
        self.ramp_mi = Some(layout.length_mi() + RAMP_ACCESS_MI);
        // Real time from this frame on, not from the next pass over the flag
        // above: the lane is short enough that one compressed frame counts.
        self.trip.controlled_ramp = true;
        self.ramp_stop = Some(stop.clone());
        self.ramp_end_said = false;
        self.ramp_arrival_grace_s = 0.0;
        self.destination_exit_taken = stop.stop_type == "delivery_destination";
        if self.destination_exit_taken {
            self.post_gate_zone(ctx);
        }
        // The ramp is a single lane peeling off the right side. Squarely in
        // it and pointing along it: both sides of a ramp are road edges, so a
        // truck placed here still carrying its mainline heading is off one of
        // them within a second.
        self.lane.recentre(0);
        self.lane_change_target = None;
        self.merge_deadline = None;
        self.begin_ramp_terminal(ctx, stop);
        // The ramp takes the pedals back, but the SESSION rides along: a
        // ramp terminal is a transit stop, so automatic speed control
        // returns on its own once the bar is honored and the ramp is
        // behind the truck. Disarming here is why both controllers stayed
        // dead until the player pressed resume (Shane, 2026-08-15). The
        // resume helper still refuses the whole ramp, so nothing
        // re-engages between here and the bar.
        //
        // A destination exit is the exception: that ramp ends at the gate,
        // and winding the truck back up on it is exactly what drove a
        // playtest past the terminal at 66 mph. It holds like any other
        // arrival, until the player departs with the next load.
        let transit = stop.stop_type != "delivery_destination";
        self.pause_speed_control(ctx, transit);
        ctx.audio.play_with("ui/notify", 0.7, 0.0);
        let take = if stop.stop_type == "delivery_destination" {
            // Just the number. Where the exit leads and whose it is were
            // said on every call from 5 miles out, and saying them again
            // here buried the ramp's length and what ends it (owner,
            // 2026-10-25: the move off the highway said too much).
            if stop.exit_label.is_empty() {
                String::new()
            } else {
                format!("{}.", stop.exit_label)
            }
        } else if stop.exit_label.is_empty() {
            format!("Exit for {}.", stop.spoken_name())
        } else {
            format!("{} for {}.", stop.exit_label, stop.spoken_name())
        };
        // A sentence of its own after the exit speed: the label reads
        // "exit 42" mid-sentence and leads this one.
        let take = {
            let mut chars = take.chars();
            match chars.next() {
                Some(first) => {
                    " ".to_string() + &first.to_uppercase().collect::<String>() + chars.as_str()
                }
                None => take,
            }
        };
        let scale_ramp = stop.stop_type == "weigh_station";
        // In the driver's units: a metric cab heard "half a mile" here in
        // an otherwise all-kilometer run (agent drive, 2026-09-01). The
        // length is this ramp's, gore to bar, not a flat half mile.
        let ramp = {
            let text = ctx.settings.short_distance_text(layout.length_mi());
            let mut chars = text.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => text,
            }
        };
        // The exit speed, once, where its sign stands: along the deceleration
        // lane the truck has just entered (MUTCD 11th ed. 2C.12, W13-2). The
        // number governs the curve at the end of this lane, and the lane is
        // where a truck at road speed sheds to it. It LEADS the line: a
        // driver doing their own braking has a few seconds of lane, and the
        // number is what they brake on (review, 2026-09-24). Never a number
        // the load aboard cannot take the ramp curve at (`spoken_exit_mph`).
        //
        // Not twice when the exit speed assist does the braking, though: the
        // one-mile call gave the same number a minute earlier, and a driver
        // who is not braking for it has no use for it again (owner,
        // 2026-10-25). The ramp line leads with the exit then.
        let exit_speed = if speed_given_at_the_mile && ctx.settings.exit_speed_assist {
            String::new()
        } else {
            format!(
                "Exit speed {}.",
                ctx.settings
                    .speed_value(self.spoken_exit_mph(self.armed_ramp_mph(Some(stop))))
            )
        };
        let message = if self.terse_speech(ctx) {
            let mut terminal = match self.ramp_control.as_str() {
                "signal" => " Traffic light at the end.",
                "stop" => " Stop sign at the end.",
                "yield" => " Yield at the end.",
                "roundabout" => " Roundabout at the end.",
                _ => "",
            };
            if scale_ramp {
                terminal = " The scale is at the end.";
            }
            format!("{exit_speed}{take} {ramp} of ramp.{terminal}")
                .trim_start()
                .to_string()
        } else {
            let mut ending = match self.ramp_control.as_str() {
                "signal" => "light at the end",
                "stop" => "stop sign at the end",
                // A yield named as "stop at the end" was an instruction the
                // rule at the line does not give (every-assist audit,
                // 2026-09-24).
                "yield" => "yield at the end",
                "roundabout" => "roundabout at the end",
                _ => "stop at the end",
            };
            if scale_ramp {
                ending = "the scale at the end, stop at the bar";
            }
            if ending == "stop at the end" && self.ramp_continues_to_destination_streets(ctx) {
                // Streets follow this ramp: its end is not where to stop.
                format!("{exit_speed}{take} {ramp} of ramp.")
                    .trim_start()
                    .to_string()
            } else {
                format!("{exit_speed}{take} {ramp} of ramp, {ending}.")
                    .trim_start()
                    .to_string()
            }
        };
        // On the ramp from this line on: a mainline line it cuts is not
        // handed back (see `speak_plain_route_event`).
        self.refresh_live_facts();
        let mut opts = SayEvent::new();
        opts.category = Some(SpeechCategory::Navigation);
        ctx.say_event_with(message, opts);
    }

    /// Real reaction seconds after `message` at the player's own rate.
    ///
    /// A screen-reader-owned voice reads at a rate the game cannot see, so
    /// the slowest assumption stands in for it.
    pub fn ramp_arrival_grace_for(&self, ctx: &GameContext, message: &str) -> f64 {
        let speech_rate = if ctx.settings.sapi_events && ctx.speech.event_supports_rate() {
            ctx.settings.speech_rate
        } else {
            0.0
        };
        ramp_arrival_grace_seconds(message, speech_rate)
    }

    /// How much road a loop-back puts back in front of the entrance.
    ///
    /// Sized in real seconds at the current pace, never a fixed stretch: once
    /// the terminal is behind the truck the ramp runs on the compressed clock
    /// again, and a fixed retry distance would be gone before the fresh cue
    /// could be heard -- the lesson the missed-exit and missed-gate loops both
    /// already carry. Bounded by the road it lives on: never shorter than the
    /// terminal-to-driveway stretch, never longer than the ramp itself.
    pub fn destination_terminal_retry_mi(&self) -> f64 {
        let speed = self.trip.truck.speed_mph().max(self.armed_ramp_mph(None));
        let miles = EXIT_WARNING_REAL_S * speed * self.trip.effective_time_scale() / 3600.0;
        let ramp_mi = match (self.ramp_layout, self.ramp_stop.as_ref()) {
            (Some(layout), _) => layout.length_mi(),
            (None, Some(stop)) => self.trip.ramp_length_mi(stop),
            (None, None) => 0.0,
        } + RAMP_ACCESS_MI;
        RAMP_ACCESS_MI.max(miles.min(ramp_mi))
    }

    /// Blown the destination terminal at speed: the scripted loop-back.
    ///
    /// The fourth instance of a pattern the blown ramp POI, the missed
    /// destination exit, and the missed facility gate already share, and the
    /// one place it was missing. Only the no-chain terminal reaches here: a
    /// facility with a street chain hands off at legal speed and never blows.
    ///
    /// The turnaround comes back to the facility, not back up the ramp, so
    /// the light or sign the driver already honored is not re-run -- only the
    /// entrance is ahead again. The clock keeps running through every loop;
    /// the lost time is the consequence, never a fine.
    pub fn loop_back_to_destination_terminal(
        &mut self,
        ctx: &mut GameContext,
        stop: &ff_core::sim::trip_models::RoadStop,
    ) {
        self.ramp_terminal_miss_count += 1;
        self.trip.game_minutes += RAMP_TERMINAL_MISS_LOOP_MIN;
        self.charge_scripted_loop(ctx, RAMP_TERMINAL_MISS_LOOP_MIN);
        self.ramp_mi = Some(self.destination_terminal_retry_mi());
        // The turnaround comes back on surface road: no deceleration lane or
        // ramp curve lies between it and the entrance.
        self.ramp_layout = None;
        // The say-once latch must never swallow the reposition: when the
        // missed-exit loop let it, a second miss stranded the trip with
        // nothing left to aim at. The arrival line speaks fresh instead.
        self.ramp_end_said = false;
        self.ramp_arrival_grace_s = 0.0;
        // A fresh approach: the arrival assist announces itself again when it
        // takes the pedals, rather than riding the terminal's release line.
        // That needs the latch itself let go too -- left set, the assist
        // never re-announced and held the retry at the facility-lane walk
        // from the turnaround on, with nothing to shed for.
        self.approach_pull_ahead = false;
        self.destination_arrival_active = false;
        self.destination_assist_brake = 0.0;
        // Automatic speed control is what drove this miss, so the whole
        // session goes -- not just the active controller. Left armed, the
        // resume helper would wind the truck straight back up to speed on the
        // re-approach and blow the same entrance again.
        self.cancel_cruise(ctx, false);
        self.cancel_keeper(ctx, false);
        let place = stop.name.clone();
        let mut message = format!("Drove past {place}. It is ahead again.");
        if self.ramp_terminal_miss_count >= 2 {
            // The identical core line keeps the flow predictable by ear; a
            // repeat miss earns help, not scolding.
            message.push_str(&format!(
                " Brake with {} well before it.",
                ctx.control_hint("brake")
            ));
        }
        ctx.audio.play("ui/warning");
        self.set_status(format!("Drove past {place}. Use the next safe turnaround."));
        // The mandatory destination terminal, not an optional stop: names the
        // loop-back maneuver that still delivers the load, so it must survive
        // quiet/urgent_only as words.
        let mut opts = SayEvent::new();
        opts.category = Some(SpeechCategory::Navigation);
        ctx.say_event_with(message, opts);
    }

    /// Brake an explicitly selected optional stop at its entrance, and an
    /// open scale lane keeping on full signalled for at its bar.
    ///
    /// Where the stop has its own streets past the ramp, its entrance is
    /// their end, the lot, and the assist brakes for that instead.
    pub fn update_selected_stop_assist(&mut self, ctx: &mut GameContext) -> bool {
        let on_streets = self.stop_chain.is_some();
        let Some(stop) = self.ramp_stop.clone().or_else(|| self.stop_chain.clone()) else {
            return false;
        };
        let selected = self.selected_stop_assist_armed
            && self.is_selected_stop(Some(&stop))
            && ctx.settings.selected_stop_assist;
        if !(selected || self.scale_stop_is_assisted(ctx, &stop))
            || (!on_streets && !self.ramp_terminal_done)
        {
            return false;
        }
        if !on_streets
            && (self.ramp_mi.is_some_and(|mi| mi <= -RAMP_OVERSHOOT_MI)
                || self.stop_chain_route(&stop).is_some())
        {
            return false;
        }
        let gap_mi = if on_streets {
            self.trip.remaining_miles()
        } else {
            0.0f64.max(self.ramp_mi.unwrap_or(0.0))
        };
        let speed = self.trip.truck.speed_mph();
        if speed <= DOCKING_MAX_MPH && gap_mi <= 0.08 {
            self.trip.truck.throttle = 0.0;
            self.trip.truck.brake = 1.0;
            self.trip.truck.set_parking_brake();
            self.finish_stop_chain(ctx);
            self.ramp_mi = None;
            self.ramp_stop = None;
            self.ramp_control = String::new();
            self.exit_signal_on = false;
            self.cruise_exit_mph = None;
            self.reset_exit_lane_state();
            self.open_poi_stop(ctx, &stop, true, None);
            return true;
        }
        if speed <= DOCKING_MAX_MPH {
            // Stopped short of the entrance: never trap the driver in a brake
            // hold. The ramp guidance tells them to pull ahead.
            self.trip.truck.brake = 0.0;
            return false;
        }
        let gap_m = 0.5f64.max(gap_mi * 1609.344);
        let v_mps = 0.0f64.max(self.trip.truck.velocity_mps);
        let needed = (v_mps * v_mps) / (2.0 * gap_m);
        if self.selected_stop_assist_brake <= 0.0
            && needed < RAMP_ASSIST_DECEL_START_MPS2
            && gap_mi > 0.08
        {
            return false;
        }
        self.trip.truck.throttle = 0.0;
        self.selected_stop_assist_brake =
            assist_servo_brake(self.selected_stop_assist_brake, needed, &self.trip.truck);
        self.trip.truck.brake = self.trip.truck.brake.max(self.selected_stop_assist_brake);
        if !self.selected_stop_assist_said {
            self.selected_stop_assist_said = true;
            // Silent: the driver selected this stop and the braking is
            // felt; the entrance itself is what the ramp guidance names.
            self.pause_speed_control(ctx, false);
        }
        false
    }

    /// One interrupting CONFIRMATION line at the default priority.
    fn say_confirmation_event(&self, ctx: &mut GameContext, message: &str) {
        let mut opts = SayEvent::new();
        opts.category = Some(SpeechCategory::Confirmation);
        ctx.say_event_with(message.to_string(), opts);
    }
}
