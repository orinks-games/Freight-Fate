//! Arriving: the facility gate, the missed destination exit, the dock menu,
//! and the state's own visible lines and presence.

use crate::speech::EventPriority;
use ff_core::models::business::player_pays_operating_costs;
use ff_core::models::trucks::TRUCK_CATALOG;
use ff_core::pyfmt::{fmt_grouped, round_py_int};
use ff_core::sim::hos::{clock_text, time_of_day};
use ff_core::sim::trip_models::{RoadStop, Zone};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::discord_presence::{driving_presence, PresenceState};
use crate::online_presence::RADIO_CLAUSE;
use crate::states::base::TimedMessageState;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_menu_states::{replace_drive_with, DriveRef, FacilityArrivalState};
use crate::states::driving_updates::live;

impl DrivingState {
    /// Once-per-threshold low-fuel cue at about 15 percent remaining, naming
    /// the next fuel stop on the route (or that none is listed).
    ///
    /// Speaks on the Safety channel (heard at quiet and urgent_only), plays
    /// the same short UI warning other mid-drive safety cues use, and latches
    /// until a refill climbs the tank back above the line. Empty-tank rescue
    /// keeps its own path; this cue stays silent once the tank is dry.
    pub fn check_low_fuel_warning(&mut self, ctx: &mut GameContext) {
        let fraction = self.trip.truck.fuel_fraction();
        if self.trip.truck.fuel_gal <= 0.0 {
            // Empty tank is the rescue line, not a second low-fuel cue.
            return;
        }
        if fraction > LOW_FUEL_WARN_FRACTION {
            self.low_fuel_said = false;
            return;
        }
        if self.low_fuel_said {
            return;
        }
        self.low_fuel_said = true;
        let pct = fraction * 100.0;
        let message = format!("Fuel low: {pct:.0} percent.");
        let message = format!("{message} {}", self.next_fuel_text(ctx));
        ctx.audio.play("ui/warning");
        ctx.say_event_with(message, SayEvent::new().category(SpeechCategory::Safety));
    }

    /// Miles per gallon this run is really getting: its own average once it
    /// has driven and burned enough to measure, else [`ASSUMED_RANGE_MPG`].
    /// The run's average already carries the load, the climbs and the
    /// idling, which a flat figure cannot.
    pub fn range_mpg(&self) -> f64 {
        let used = self.trip.fuel_used_gal;
        let driven = self.trip.position_mi;
        if used >= RANGE_MEASURE_MIN_GAL && driven >= RANGE_MEASURE_MIN_MI {
            (driven / used).clamp(RANGE_MPG_FLOOR, RANGE_MPG_CEILING)
        } else {
            ASSUMED_RANGE_MPG
        }
    }

    /// Miles the fuel in the tank reaches at [`Self::range_mpg`].
    pub fn fuel_range_mi(&self) -> f64 {
        self.trip.truck.fuel_gal.max(0.0) * self.range_mpg()
    }

    /// The nearest fuel stop ahead this rig can use, and the miles to it.
    pub fn next_fuel_stop(&self) -> Option<(&RoadStop, f64)> {
        self.trip
            .stops
            .iter()
            .filter(|stop| stop.sells_fuel() && stop.accessible_to(self.trip.bobtail))
            .map(|stop| (stop, stop.at_mi - self.trip.position_mi))
            .filter(|(_, ahead)| *ahead >= 0.0)
            .min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// Miles to the nearest fuel stop ahead this rig can use, if any.
    pub fn next_fuel_stop_mi(&self) -> Option<f64> {
        self.next_fuel_stop().map(|(_, ahead)| ahead)
    }

    /// The fuel range, the miles it has to cover, and the next fuel stop
    /// when that comes before the destination.
    pub fn fuel_range_need(&self) -> (f64, f64, Option<f64>) {
        let remaining = self.trip.remaining_miles();
        let next_fuel = self.next_fuel_stop_mi().filter(|ahead| *ahead < remaining);
        (
            self.fuel_range_mi(),
            next_fuel.unwrap_or(remaining),
            next_fuel,
        )
    }

    /// Warns once when the fuel range falls short of the next fuel stop, or
    /// of the destination when no fuel stop comes first. Latched until the
    /// range clears what it needs by a fifth, so a refill re-arms it and the
    /// run's average wobbling across the line does not.
    pub fn check_fuel_range_warning(&mut self, ctx: &mut GameContext) {
        if self.trip.truck.fuel_gal <= 0.0 {
            return;
        }
        let (range, needed, next_fuel) = self.fuel_range_need();
        if range >= needed * 1.2 {
            self.fuel_range_short_said = false;
            return;
        }
        if range >= needed || self.fuel_range_short_said {
            return;
        }
        self.fuel_range_short_said = true;
        let range_text = ctx.settings.distance_text(range, false);
        let terse = self.terse_speech(ctx);
        let message = match next_fuel {
            Some(ahead) => {
                let ahead_text = ctx.settings.distance_text(ahead, false);
                if terse {
                    format!("Fuel range {range_text}, next fuel {ahead_text}.")
                } else {
                    format!(
                        "Fuel range about {range_text}, short of the next fuel stop, \
                         {ahead_text} ahead."
                    )
                }
            }
            None if terse => format!("Fuel range {range_text}, short of the destination."),
            None => format!(
                "Fuel range about {range_text}, short of the destination, with no fuel \
                 stop ahead on this route."
            ),
        };
        ctx.audio.play("ui/warning");
        ctx.say_event_with(message, SayEvent::new().category(SpeechCategory::Safety));
    }

    /// Where the next fuel is, for the low fuel warning: the nearest fuel
    /// stop ahead this rig can use before the destination, by its spoken
    /// name and distance, or that there is none. No range math here: the
    /// fuel range warning owns that, and F speaks the range.
    pub fn next_fuel_text(&self, ctx: &GameContext) -> String {
        let remaining = self.trip.remaining_miles();
        match self
            .next_fuel_stop()
            .filter(|(_, ahead)| *ahead < remaining)
        {
            Some((stop, ahead)) => format!(
                "Next fuel: {}, {}.",
                stop.spoken_name(),
                ctx.settings.distance_text(ahead, false)
            ),
            None => "No fuel stop before the destination.".to_string(),
        }
    }

    /// `_handle_out_of_fuel()`.
    pub fn handle_out_of_fuel(&mut self, ctx: &mut GameContext) {
        if self.rescue_offered {
            return;
        }
        self.rescue_offered = true;
        let fee = 750.0;
        let billing = {
            let profile = profile_mut_of(ctx);
            if player_pays_operating_costs(&profile.business_status) {
                profile.spend(fee); // can go negative: the rescue is not optional
                format!("for {} dollars", fmt_grouped(fee, 0))
            } else {
                // the carrier pays for company fuel, but a preventable service
                // call goes straight onto the driver's record
                profile.career.reputation = 0.0f64.max(profile.career.reputation - 2.0);
                "on the carrier account, and dispatch noted the service call".to_string()
            }
        };
        self.trip.truck.refuel(Some(30.0));
        self.trip.truck.recover_from_fuel_depletion();
        self.cancel_cruise(ctx, false);
        self.rescue_offered = false;
        ctx.audio.play("ui/error");
        // A repair bill and an instruction, spoken to a truck already coasted
        // to a stop: nothing act-now left, so it queues on ROUTE's
        // never-dropped contract instead of purging the channel.
        let engine = ctx.control_hint("engine");
        let mut opts = SayEvent::queued().priority(EventPriority::Important);
        opts.category = Some(SpeechCategory::Safety);
        ctx.say_event_with(
            format!(
                "Out of fuel. Roadside rescue brought thirty gallons {billing}. Press {engine} \
                 to restart the engine."
            ),
            opts,
        );
    }

    /// `_arrive()`: the pickup or delivery arrival screen.
    pub fn arrive(&mut self, ctx: &mut GameContext) {
        self.replace_with_arrival_state(ctx);
    }

    /// `_handle_missed_destination_exit()`.
    pub fn handle_missed_destination_exit(&mut self, ctx: &mut GameContext) {
        let exit_details = self.destination_exit_details(ctx, true);
        self.trip.finished = false;
        self.exit_stop = None;
        self.exit_signal_on = false;
        self.exit_signal_canceled = false;
        // A cancel belongs to the approach it was made on. Left standing, the
        // rebuilt exit carries the same key, the scanner skips it, and lane
        // keeping never takes the exit this loop-back says it will.
        self.canceled_exit_key = None;
        self.cancel_cruise(ctx, false);
        let exit_at = match exit_details {
            Some(details) => details.0,
            // Rural approaches carry no baked interchange, so the details
            // scan finds nothing -- but the exit the player just missed was
            // the synthetic one _destination_exit_stop places a mile before
            // route end, and the loop-back must return to it. Without this
            // the second miss stranded the trip at 0 miles remaining with
            // no exit left to signal for (owner playtest, Sedona to Camp
            // Verde on AZ-260, 2026-07-18).
            None => 0.0f64.max(self.trip.total_miles() - DESTINATION_EXIT_BEFORE_END_MI),
        };
        // Every miss loops back. The say-once latch must never swallow
        // this reposition: when it did, the second miss stranded the trip
        // pinned at the end of the route with no exit left to signal for,
        // cruise dying every frame (playtest transcript, 2026-07-16).
        self.missed_destination_exit_said = true;
        self.trip.game_minutes += EXIT_MISS_LOOP_MIN;
        // The loop-back is a real drive: hours, fatigue, and idle fuel move
        // with the clock, exactly as the facility-gate miss charges them.
        self.charge_scripted_loop(ctx, EXIT_MISS_LOOP_MIN);
        // Drop back a full exit window, not a fixed mile: under time
        // compression one mile passes in a few real seconds, making the
        // re-approach unwinnable before it was heard.
        self.trip.position_mi = 0.0f64.max(exit_at - self.exit_window_mi());
        self.destination_exit_announced_key = String::new();
        self.destination_exit_response_s = 0.0;
        self.destination_exit_cache = None;
        let automated = ctx.settings.lane_is_automated();
        let reroute_text = if self.terse_speech(ctx) {
            // Terse holds on to consequences, and who is driving the second
            // approach is one: the signal reset with the miss, so when the
            // lane work is theirs they still need to hear that arming it is
            // on them again -- and when it is not, that it is not.
            if automated {
                "Destination exit ahead again.".to_string()
            } else {
                format!(
                    "Destination exit ahead again. Press {} to signal.",
                    ctx.control_hint("take_exit")
                )
            }
        } else if automated {
            // The other half. This line used to name the take-exit control in
            // every steering mode, so the last thing a full-lane-keeping
            // driver heard before the truck took the ramp for them was an
            // instruction to take it themselves -- which is what "I was about
            // to hit X when the truck took the exit" is describing. Never
            // name a control the driver's own settings have taken off them.
            "You turned around, and the destination exit is ahead again.".to_string()
        } else {
            format!(
                "You turned around, and the destination exit is ahead again. Press {} to signal \
                 for it.",
                ctx.control_hint("take_exit")
            )
        };
        ctx.audio.play("ui/warning");
        self.set_status("Destination exit missed. Use the next safe turnaround.");
        // A mandatory-stop miss, not an optional one: the route just changed
        // and this names the maneuver that still gets the load delivered, so
        // it must survive quiet/urgent_only as words.
        let facility = self.destination_facility_text(ctx);
        let mut opts = SayEvent::new();
        opts.category = Some(SpeechCategory::Navigation);
        ctx.say_event_with(
            format!("Missed the destination exit for {facility}. {reroute_text}"),
            opts,
        );
    }

    /// The assisted stop at a facility entrance: the assist takes the pedals,
    /// brings the truck to rest, sets the parking brake and holds there for
    /// the driver's key. `true` means the assist owns this frame and the
    /// gate's manual flow must not run.
    ///
    /// Shared by the dock gate and the pickup gate. It lived only in
    /// `handle_arrival_gate` until 2026-09-20, so the setting that promises
    /// to stop at "the selected facility arrival point" stopped the truck at
    /// a delivery and not at a pickup: `handle_pickup_gate` only OPENED the
    /// check-in once the truck was already under docking speed, and nothing
    /// ever put the brake on. The owner drove into Oshkosh Dry Warehouse and
    /// stopped the truck himself (2026-09-20), which is the same report the
    /// delivery side answered in August. `open_ready_facility_arrival` has
    /// always dispatched the confirm key by phase, so the pickup needed the
    /// hold and nothing else.
    pub(crate) fn hold_at_facility_entrance(&mut self, ctx: &mut GameContext) -> bool {
        if !ctx.settings.destination_approach_assist {
            return false;
        }
        // A ramp-end arrival may already have started the automatic
        // pull-in before this frame reaches the finished-trip handler.
        // Do not queue the manual "Press Enter" hold prompt behind the
        // truthful dock-opening line: even though its validity gate keeps
        // it out of speech, it leaves a contradictory transcript and can
        // be handed to other announcement consumers as actionable text.
        if self.arrival_menu_open {
            return true;
        }
        // An arrival pause, held until the departure resumes it. Ending the
        // session here lost the driver's set speed at every assisted pickup
        // (tester report, 2026-09-28).
        if self.speed_control_armed {
            self.pause_speed_control(ctx, false);
        } else {
            self.cancel_cruise(ctx, false);
        }
        self.trip.truck.throttle = 0.0;
        self.trip.truck.brake = 1.0;
        if self.trip.truck.speed_mph() <= 0.5 && !self.arrival_full_stop_said {
            self.arrival_full_stop_said = true;
            self.trip.truck.set_parking_brake();
            // Only while the key still does something. A cut line is
            // handed back so it finishes, and this one asks for a
            // keypress -- handed back after the driver has pressed it
            // and the dock menu is open, it asks for a press that has
            // already happened (Shane, 2026-08-21, three of these at
            // one dock). Third line in this family after the scale and
            // the destination exit; the rule is the same every time.
            //
            // Rust: the live `_arrival_menu_open` read cannot ride a
            // 'static closure, so it goes through `live` like the scale
            // reminder's mile does. The flag is stamped where it moves as
            // well as per frame, because the drive stops ticking the
            // moment the dock menu takes over -- which is exactly the
            // moment this gate has to notice.
            self.refresh_live_facts();
            let mut opts = SayEvent::new().valid(|| !live::arrival_menu_open());
            opts.category = Some(SpeechCategory::Navigation);
            ctx.say_event_with(
                format!(
                    "Holding at the entrance. Press {} to continue.",
                    ctx.control_hint("confirm")
                ),
                opts,
            );
        }
        true
    }

    /// `_handle_arrival_gate()`.
    pub fn handle_arrival_gate(&mut self, ctx: &mut GameContext) {
        if self.hold_at_facility_entrance(ctx) {
            return;
        }
        if self.trip.truck.speed_mph() <= DOCKING_MAX_MPH && self.trip.truck.parking_brake {
            self.open_facility_arrival(ctx);
            return;
        }
        if self.trip.truck.speed_mph() <= DELIVERY_PARK_MPH {
            self.handle_arrival_creep(ctx);
            return;
        }
        // Above the gate zone's posted limit with the warning heard and the
        // reaction window spent: the entrance is missed, not still ahead.
        // (See driving_facility_gate.py; the assist branch above brakes the
        // truck itself and must never reach this.)
        if self.gate_miss_pending() {
            self.handle_missed_facility_gate(ctx);
            return;
        }
        let facility = self.destination_facility_text(ctx);
        if self.arrival_stop_said {
            let message = if self.terse_speech(ctx) {
                format!("At {facility}. Stop to dock.")
            } else {
                format!("Still at {facility}. Stop to dock.")
            };
            self.remind_arrival_gate(ctx, "Destination gate: stop to dock.", &message, false);
            return;
        }
        self.arrival_stop_said = true;
        self.gate_reminder_s = GATE_REMINDER_INTERVAL_S;
        // The speed keeper holds a facility gate zone's own 15 right up to
        // the gate, and the gate is a stop: it hands the pedals back here,
        // and the line says so, or the driver who trusted it through the
        // streets hears "slow down" from a truck that was doing exactly
        // what they had told it to (agent playtest, 2026-09-02, four
        // loop-backs on one delivery).
        let keeper_held = self.keeper_mph.is_some();
        self.cancel_cruise(ctx, false);
        ctx.audio.play("ui/warning");
        self.set_status("Destination ahead. Stop at the gate.");
        let mut message = if self.terse_speech(ctx) {
            format!("Destination ahead: {facility}.")
        } else {
            format!("Destination ahead, {facility}. Stop at the gate.")
        };
        if keeper_held {
            message.push_str(" Speed keeper off.");
        }
        self.seed_gate_grace_at_gate(ctx, &message);
        if keeper_held {
            // The pedals were the assist's until this very line: the
            // reaction window starts here, whatever the pre-gate warning's
            // window did while the keeper was driving.
            self.gate_speed_warned = true;
            self.gate_grace_s = self
                .gate_grace_s
                .max(self.gate_miss_grace_seconds(ctx, &message));
        }
        // Rescued only until the gate's own stop line has landed (see the
        // pickup gate's twin): rescued behind it, this told a truck at the
        // gate to slow down for the gate.
        let mut opts = SayEvent::new().valid(|| !live::gate_stop_prompted());
        opts.category = Some(SpeechCategory::Navigation);
        ctx.say_event_with(message, opts);
    }

    /// Whether T means "enter this facility" instead of "plan a sleep stop".
    pub fn manual_facility_arrival_ready(&self, ctx: &GameContext) -> bool {
        !ctx.settings.destination_approach_assist
            && self.trip.truck.speed_mph() <= DOCKING_MAX_MPH
            && self.trip.truck.parking_brake
            && self.arrival_gate_query_text(ctx).is_some()
    }

    /// Whether Enter or controller A may finish an assisted stop.
    pub fn assisted_facility_confirmation_ready(&self, ctx: &GameContext) -> bool {
        ctx.settings.destination_approach_assist
            && self.arrival_full_stop_said
            && self.trip.truck.speed_mph() <= DOCKING_MAX_MPH
            && self.trip.truck.parking_brake
            && self.arrival_gate_query_text(ctx).is_some()
    }

    /// Open the check-in or dock flow belonging to the current drive phase.
    pub fn open_ready_facility_arrival(&mut self, ctx: &mut GameContext) {
        if self.phase == DRIVE_PHASE_PICKUP {
            self.open_pickup_arrival(ctx);
        } else {
            self.open_facility_arrival(ctx);
        }
    }

    /// Repeat a gate's stop instruction while the truck rolls past it.
    ///
    /// The gate warnings latch after speaking once, which is right for a
    /// driver who is slowing -- but a driver who rolls on hears nothing again
    /// for the rest of the drive, with any re-armed cruise happily holding
    /// highway speed at a dead-end. Re-speak on a calm cadence and drop the
    /// cruise each time; the reminder stops the moment the truck slows into
    /// the gate's own creep-and-dock flow.
    pub fn remind_arrival_gate(
        &mut self,
        ctx: &mut GameContext,
        status: &str,
        message: &str,
        pickup: bool,
    ) {
        if self.gate_reminder_s > 0.0 {
            return;
        }
        self.gate_reminder_s = GATE_REMINDER_INTERVAL_S;
        if pickup {
            self.pause_speed_control(ctx, false);
        } else {
            self.cancel_cruise(ctx, false);
        }
        ctx.audio.play("ui/warning");
        self.set_status(status);
        let mut opts = SayEvent::new();
        opts.category = Some(SpeechCategory::Navigation);
        ctx.say_event_with(message.to_string(), opts);
    }

    /// The gate's instruction when the trip has ended at one, else None.
    ///
    /// Mirrors the update loop's gate dispatch so the info keys agree with
    /// what the gate handlers are actually waiting for.
    pub fn arrival_gate_query_text(&self, ctx: &GameContext) -> Option<String> {
        if !self.trip.finished
            || self.arrival_menu_open
            || self.departure_chain
            || self.stop_chain.is_some()
        {
            return None;
        }
        if self.phase == DRIVE_PHASE_PICKUP {
            return Some(format!(
                "At {}. Stop to check in.",
                self.pickup_facility_text(ctx)
            ));
        }
        if self.ramp_mi.is_some() || !self.destination_exit_taken {
            return None;
        }
        Some(format!(
            "At {}. Stop to dock.",
            self.destination_facility_text(ctx)
        ))
    }

    /// `_handle_arrival_creep()`.
    pub fn handle_arrival_creep(&mut self, ctx: &mut GameContext) {
        if self.arrival_full_stop_said {
            return;
        }
        self.arrival_full_stop_said = true;
        live::set_gate_stop_prompted(true);
        self.cancel_cruise(ctx, false);
        ctx.audio.play_with("ui/notify", 0.7, 0.0);
        self.set_status("Destination gate: stop to dock.");
        let facility = self.destination_facility_text(ctx);
        let message = if ctx.settings.destination_approach_assist {
            format!("At {facility}. Stop to dock.")
        } else {
            format!("At {facility}. Stop and set the parking brake.")
        };
        self.say_route_navigation(ctx, &message);
    }

    /// `_open_facility_arrival()`.
    pub fn open_facility_arrival(&mut self, ctx: &mut GameContext) {
        if self.arrival_menu_open {
            return;
        }
        self.arrival_menu_open = true;
        // A co-driver at the wheel wakes the player for the receiver: the
        // dock is theirs, on their own logbook.
        if self.co_driver_driving() {
            self.co_driver_summon(ctx, "gate");
        }
        // The frame loop stops here, so the gate on "press Enter to continue"
        // would keep reading the last tick's answer without this.
        live::set_arrival_menu_open(true);
        self.cancel_cruise(ctx, false);
        self.trip.truck.brake = 1.0;
        self.trip.truck.set_parking_brake();
        // A dock gate is a menu-driven stop like a roadside inspection: the
        // frame loop that eases revs down between frames stops the instant
        // the dock menu takes over, so without this the engine audio froze
        // at whatever rev the approach left it at, all the way through the
        // stop.
        self.settle_engine_to_idle(ctx);
        advance_rest_clock(self, ctx, STOP_PULL_IN_MIN, None, "");
        hos_mut_of(ctx).on_duty(STOP_PULL_IN_MIN);
        self.set_status("Pulling into destination. Dock menu opening.");

        let facility = self.destination_facility_text(ctx);
        // Python's `complete()` closed over `self`, so the drive stayed
        // reachable even though `replace_state` had just taken it off the
        // stack. A 'static callback cannot close over `&mut self`, so take
        // the drive's own handle NOW, while it is still the active state,
        // and carry that into the callback. Looking the drive up on the
        // stack from inside the callback cannot work: the replace below is
        // exactly what removed it, so the dock menu never opened and the
        // game sat on "Pulling into destination" forever.
        let drive = DriveRef::active(ctx);
        ctx.replace_state(
            TimedMessageState::new(
                "Pulling into destination",
                &format!("Pulling into {facility}."),
                "Pulling into the destination facility.",
                STOP_PULL_IN_WAIT_S,
                move |ctx: &mut GameContext| {
                    let handle = drive.clone();
                    drive.with(ctx, |drive, ctx| {
                        drive.set_status("Parked at destination. Dock and deliver.");
                        // `FacilityArrivalState(self.ctx, self)`: the drive
                        // it covers is the one that pulled in, not whatever
                        // the stack happens to be showing now.
                        let mut state = FacilityArrivalState::with_drive(handle);
                        state.enter_over_drive(ctx, drive);
                        replace_drive_with(ctx, state);
                    });
                },
            )
            .sound_key(Some("ui/notify")),
        );
    }

    /// `_destination_facility_text()`.
    pub fn destination_facility_text(&self, _ctx: &GameContext) -> String {
        self.job.destination_facility_text()
    }

    /// `_objective_text()`.
    pub fn objective_text(&self, ctx: &GameContext) -> String {
        if self.phase == DRIVE_PHASE_PICKUP {
            return format!("pickup at {}", self.pickup_facility_text(ctx));
        }
        format!("deliver to {}", self.destination_facility_text(ctx))
    }

    /// `presence()`: broad, privacy-safe activity for Discord Rich Presence.
    /// The tractor named on the drivers board and in Discord presence: the
    /// one the driver is IN. `active_truck_key`, never the raw
    /// `profile.truck` field -- for a company driver that field is
    /// save-compat storage, and reading it had the board naming Brandon's
    /// old yard mule while he drove the fleet's presidential sleeper
    /// (reported 2026-08-31).
    pub fn presence_truck_label(profile: &ff_core::models::profile::Profile) -> &'static str {
        TRUCK_CATALOG
            .get(profile.active_truck_key().as_str())
            .map(|truck| truck.label)
            .unwrap_or("")
    }

    /// How far along the whole run the truck is, streets included, in 0..1.
    ///
    /// The active trip is only part of the run while a street chain is on:
    /// the streets out of the origin yard before the highway, or the streets
    /// in to the dock after it, with the highway trip parked in
    /// `highway_trip` either way. Read on its own, the active trip had the
    /// board saying "100% there" for the whole last-mile chain and racing
    /// 0 to 100 on the two miles out of the yard, so every percent the game
    /// publishes -- the drivers board, Discord, Trip status, the R key --
    /// measures against the highway and the streets together.
    pub fn journey_progress_fraction(&self) -> f64 {
        let (mut done, mut total) = (self.trip.position_mi, self.trip.total_miles());
        if let Some(highway) = &self.highway_trip {
            done += highway.position_mi;
            total += highway.total_miles();
        }
        if total <= 0.0 {
            return 0.0;
        }
        (done / total).clamp(0.0, 1.0)
    }

    /// `journey_progress_fraction` as the whole percent the readouts speak.
    ///
    /// Never 100 before the gate: the last mile of streets is a tenth of a
    /// percent of a long run, and rounding said "100 percent there" for all
    /// of it. 100 means arrived.
    pub fn journey_progress_percent(&self) -> i64 {
        let fraction = self.journey_progress_fraction();
        let pct = round_py_int(100.0 * fraction).clamp(0, 100);
        if fraction < 1.0 {
            pct.min(99)
        } else {
            pct
        }
    }

    pub fn presence_state(&self, ctx: &GameContext) -> Option<PresenceState> {
        let fraction = self.journey_progress_fraction();
        let moving = self.trip.truck.speed_mph() >= 1.0;
        let truck_label = ctx
            .profile
            .as_ref()
            .map(Self::presence_truck_label)
            .unwrap_or("");
        Some(driving_presence(
            self.phase,
            self.job.spoken_origin(),
            self.job.spoken_destination(),
            self.job.cargo.label,
            fraction,
            moving,
            truck_label,
        ))
    }

    /// `online_presence()`: the drivers-board snapshot.
    ///
    /// The drivers board line adds what the cab radio is playing; Discord
    /// presence (above) does not, so the clause rides only the board copy.
    /// Station display names are curated public catalog data (call sign and
    /// name), never a stream URL, and the clause disappears the moment the
    /// radio is switched off -- the board only ever hears what a passenger in
    /// the cab would.
    pub fn online_presence_state(&self, ctx: &GameContext) -> Option<PresenceState> {
        let base = self.presence_state(ctx)?;
        if !self.radio.enabled {
            return Some(base);
        }
        // The station the cab is playing, by the id the playback seam
        // recorded, not what the dial resolves to at this instant: between
        // the truck crossing a range contour and the reception tick that
        // retunes, the resolving read already answers with the fallback,
        // and the board then names a station nobody in the cab can hear
        // (owner, I-35, 2026-09-16: "listening to the Eagle" while KVSC
        // played). `tuned_station` stays as the answer when nothing has been
        // played yet; it reads without re-pointing the dial, and the
        // handover is left to the tick. Both are `&self`: resolving on a
        // whole clone of the radio (757 stations and their identity map,
        // sixty times a second) measured 2.4 ms of every frame.
        let station = self
            .radio
            .station_by_id(&self.radio_station_id)
            .cloned()
            .unwrap_or_else(|| self.radio.tuned_station());
        let mut clause = format!("{RADIO_CLAUSE}{}", station.display_name());
        // And the song, when the stream says: broadcast metadata the station
        // itself publishes to every listener, so no more private than the
        // station name. The tick's copy, not a fresh read -- presence is
        // built often, and a title that changes mid-second can wait for the
        // next tick.
        if station.real_stream {
            if let Some(title) = self.radio_now_playing.as_ref() {
                if !title.is_empty() {
                    clause = format!("{clause}: {title}");
                }
            }
        }
        let detail = if base.detail.is_empty() {
            clause
        } else {
            format!("{}, {clause}", base.detail)
        };
        Some(PresenceState::new(&base.activity, &detail))
    }

    /// `lines()`: the visible mirror of the speech.
    pub fn visible_lines(&self, ctx: &GameContext) -> Vec<String> {
        let t = &self.trip.truck;
        let (limit, reason) = self.display_speed_limit();
        let gear = if t.transmission.in_neutral() {
            "N".to_string()
        } else {
            t.transmission.gear.to_string()
        };
        let title = if self.phase == DRIVE_PHASE_PICKUP {
            format!(
                "Deadheading to pickup at {}",
                self.pickup_facility_text(ctx)
            )
        } else {
            format!("Driving loaded to {}", self.job.spoken_destination())
        };
        let s = &ctx.settings;
        let decimals = if self.phase == DRIVE_PHASE_PICKUP {
            1
        } else {
            0
        };
        let remaining = format!(
            "{} of {} {}",
            s.distance_value(self.trip.remaining_miles(), decimals, false),
            s.distance_value(self.trip.total_miles(), decimals, false),
            s.distance_unit_text(true)
        );
        let reason_text = match reason {
            Some(reason) => format!(", {reason}"),
            None => String::new(),
        };
        let cruise = match self.cruise_mph {
            Some(mph) => format!("   CRUISE {mph:.0}"),
            None => String::new(),
        };
        let air_state = if t.air_low_warning() {
            "LOW AIR"
        } else if t.air_ready() {
            "air ready"
        } else {
            "building"
        };
        let brake_state = if t.spring_brakes_active() {
            "spring brakes"
        } else if t.parking_brake {
            "parking set"
        } else {
            "parking released"
        };
        let calendar = self.calendar_phrase(ctx);
        let calendar = if calendar.is_empty() {
            "unknown".to_string()
        } else {
            calendar
        };
        let fatigue = ctx
            .profile
            .as_ref()
            .map(|profile| profile.fatigue)
            .unwrap_or(0.0);
        vec![
            title,
            String::new(),
            format!(
                "Speed: {} (limit {}{reason_text})   Lane: {}",
                s.hud_speed_text(t.speed_mph()),
                s.distance_value(limit, 0, false),
                self.lane.lane_name()
            ),
            format!(
                "Gear: {gear}   RPM: {:.0}   {}{cruise}",
                t.rpm,
                if t.engine_on {
                    "ENGINE ON"
                } else {
                    "engine off"
                }
            ),
            format!(
                "Air: {:.0} psi   {air_state}   {brake_state}",
                t.air_pressure_psi()
            ),
            format!(
                "Fuel: {:.0}%   Damage: {:.0}%",
                t.fuel_fraction() * 100.0,
                t.damage_pct
            ),
            format!("Remaining: {remaining}"),
            format!("Weather: {}", self.trip.weather.current.value()),
            format!("Date: {calendar}"),
            format!(
                "Clock: {} {} ({})   Fatigue: {fatigue:.0}%",
                clock_text(self.trip.local_hour()),
                self.clock_zone_label(ctx),
                time_of_day(self.trip.local_hour())
            ),
            String::new(),
            self.status_text.clone(),
        ]
    }

    /// `speed_limit_at(position)` for the window, without the congestion
    /// recompute `Trip::speed_limit_at` performs (`lines()` is `&self`).
    pub(crate) fn display_speed_limit(&self) -> (f64, Option<String>) {
        let mile = self.trip.position_mi;
        let mut best: Option<&Zone> = None;
        for zone in &self.trip.zones {
            if !(zone.start_mi <= mile && mile <= zone.end_mi) {
                continue;
            }
            if best.is_none_or(|found| zone.limit_mph < found.limit_mph) {
                best = Some(zone);
            }
        }
        match best {
            Some(zone) => (zone.limit_mph, Some(zone.reason.clone())),
            None => (self.trip.corridor_limit_at(mile), None),
        }
    }
}
