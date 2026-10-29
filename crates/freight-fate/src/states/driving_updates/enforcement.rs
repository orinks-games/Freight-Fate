//! The dash overspeed alert, what opens a pull-over (a trooper's clock, a
//! bypassed scale, unsafe equipment), and the weigh-station machinery.
//! The stop's own lifecycle, once the lights are on, is in `stops.rs`.

use crate::speech::EventPriority;
use ff_core::models::business::has_weigh_station_transponder;
use ff_core::models::enforcement::{UNSAFE_DAMAGE_FINE, WEIGH_STATION_BYPASS_FINE};
use ff_core::pyrandom::PyRandom;
use ff_core::settings::short_distance_text_for;
use ff_core::sim::trip_models::RoadStop;
use ff_core::speech_pacing::SpeechCategory;
use ff_core::speech_text::overspeed_nag;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_enforcement::SCALE_REMINDER_REAL_LEAD_S;
use crate::states::driving_team::notes::NoteWeight;
use crate::states::driving_updates::{limit_drop_speech_latency_s, live};

impl DrivingState {
    /// The dash alert, and the braking grace a dropped limit earns.
    ///
    /// This used to be where speeding was charged: hold nine over for six
    /// real seconds with no patrol anywhere on the route and the drive banked
    /// a silent "speeding strike", billed at the dock as a
    /// driver-responsibility charge. That was a fine from an officer who was
    /// never there, and it is gone. Speeding costs exactly what a trooper who
    /// saw it decides it costs, and nothing otherwise.
    ///
    /// What survives is the part that was always about fairness rather than
    /// money: a limit that drops under a loaded truck earns real braking
    /// seconds before anything counts, because enforcement tickets sustained
    /// disregard, not the transition. That grace now gates the enforcement
    /// watch's over-limit distance, which is the measure an officer actually
    /// reads.
    ///
    /// One carve-over from those fining days had to go with them: a missed
    /// destination exit used to return early here, so nothing was charged
    /// while the driver looped back. Once this became the DASH, that same
    /// line switched the speed warning off from the first miss all the way to
    /// the dock -- while the enforcement watch, which never checked the flag,
    /// went on accruing over-limit distance. The driver kept the ticket and
    /// lost the warning, which is the wrong half to lose: the loop-back is
    /// exactly the stretch where a driver has to shed speed to make the exit
    /// this time. Tyler Rodick drove the Hattiesburg approach at 89 with
    /// nothing said, lap after lap (2026-08-26).
    pub fn update_speeding(&mut self, ctx: &mut GameContext, dt: f64, accelerator_held: bool) {
        if self.ramp_mi.is_some() {
            return; // the ramp is off the highway and unpatrolled
        }
        if self.pull_over.is_some() {
            return; // already stopped; the dash has nothing to add
        }
        let (limit, _) = self.trip.speed_limit_at(self.trip.position_mi);
        if self.trip.in_unannounced_posting() {
            // The dash would name a limit the road never got to say.
            return;
        }
        self.update_overspeed_warning(ctx, dt, limit);
        // About 2 mph per second of comfortable braking sets the window,
        // capped so the grace cannot be used to coast through a whole
        // restricted zone.
        if self
            .enforced_limit_prev
            .is_some_and(|previous| limit < previous)
        {
            let grace = (self.trip.truck.speed_mph() - limit) / 2.0;
            self.limit_drop_grace_s = self.limit_drop_grace_s.max(15.0f64.min(grace));
            // The zone-entry line is queued at ROUTE and may lag its boundary
            // by the ROUTE wait budget. A driver still on the throttle inside
            // that window has simply not been told yet, so the throttle
            // check must not arm until the line has had time to speak.
            self.limit_drop_throttle_exempt_s = limit_drop_speech_latency_s();
        }
        self.enforced_limit_prev = Some(limit);
        if self.limit_drop_grace_s > 0.0 {
            self.limit_drop_grace_s = (self.limit_drop_grace_s - dt).max(0.0);
            // Staying on the throttle through the drop is disregard, not
            // compliance: the grace collapses. Read the current key/trigger
            // position, not the smoothed truck throttle, which is still
            // ramping down just after the driver lifts off -- and only once
            // the announcement's speech-latency window above has passed.
            if self.limit_drop_throttle_exempt_s > 0.0 {
                self.limit_drop_throttle_exempt_s =
                    (self.limit_drop_throttle_exempt_s - dt).max(0.0);
            } else if accelerator_held {
                self.limit_drop_grace_s = 0.0;
            }
        }
    }

    /// The dash overspeed alert: speak once, then chime until compliant.
    ///
    /// Arms at OVERSPEED_WARN_MPH over the limit -- above the pace predictive
    /// cruise itself holds, and inside the enforcement leeway, so an attentive
    /// driver hears the dash before any strike clock matters and never hears
    /// it for a speed the truck chose. The first trigger speaks the limit;
    /// while the truck stays over, the chime repeats on its interval. Actively
    /// braking down quiets the nag (the driver is already complying), and
    /// settling back under the limit disarms it for the next episode.
    ///
    /// This had a setting -- on / urgent only / off -- and the setting existed
    /// because the alert armed at exactly cruise's own 5-over pace, so it
    /// chimed at drivers who had done nothing. With the threshold above that
    /// pace there is nothing left to turn off: it now speaks only when the
    /// driver is genuinely heading for a citation.
    pub fn update_overspeed_warning(&mut self, ctx: &mut GameContext, dt: f64, limit: f64) {
        let speed = self.trip.truck.speed_mph();
        if self.overspeed_active {
            if speed <= limit + OVERSPEED_WARN_MPH - OVERSPEED_RESET_MPH {
                self.overspeed_active = false;
                self.refresh_live_facts();
                self.log_overspeed("disarmed", speed, limit);
                return;
            }
            let braking_down = self.trip.truck.brake > 0.0 && self.trip.truck.throttle <= 0.05;
            // The further over, the faster the ding: cadence slides from
            // polite to urgent as the overage approaches OVERSPEED_URGENT_MPH.
            let urgency = ((speed - limit - OVERSPEED_WARN_MPH)
                / (OVERSPEED_URGENT_MPH - OVERSPEED_WARN_MPH))
                .clamp(0.0, 1.0);
            let interval = OVERSPEED_CHIME_REPEAT_S
                - urgency * (OVERSPEED_CHIME_REPEAT_S - OVERSPEED_CHIME_FAST_S);
            self.overspeed_chime_timer += dt;
            if self.overspeed_chime_timer >= interval && !braking_down {
                self.overspeed_chime_timer = 0.0;
                ctx.audio.play_with("vehicle/overspeed_chime", 0.55, 0.0);
                self.log_overspeed("chime", speed, limit);
            }
            return;
        }
        if speed > limit + OVERSPEED_WARN_MPH {
            self.overspeed_active = true;
            self.overspeed_chime_timer = 0.0;
            self.refresh_live_facts();
            self.log_overspeed("armed", speed, limit);
            ctx.audio.play_with("vehicle/overspeed_chime", 0.65, 0.0);
            let message = overspeed_nag(
                &ctx.settings.speed_text(limit),
                &ctx.settings.speed_value(limit),
            );
            ctx.say_event_with(
                message,
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Navigation)
                    // Only while the truck is still over. Terse renders this
                    // as "Limit 35." and a cut line is handed back to finish,
                    // so a driver who lifted off in the meantime got told off
                    // for a speed they were no longer doing -- and told a limit
                    // that may by then belong to road behind them. Found by
                    // driving it: `playtest_road --find limit-drop` requeued
                    // this exact line (2026-08-21).
                    .valid(live::overspeed_active),
            );
        }
    }

    /// Every arm, chime and disarm, with the numbers behind it.
    ///
    /// A driver who hears the alert cannot see which limit it is measuring
    /// against, and from a bug report neither can we: a tester reporting a
    /// chime at five over could be five over a number he never saw drop
    /// (Shane, 2026-08-15). The log carries the speed, the limit in force,
    /// the mile it came from and the zone that set it, so a session can be
    /// read back instead of argued about. Transitions and chimes only --
    /// three or four lines an episode, not a per-frame trace.
    pub fn log_overspeed(&mut self, event: &str, speed: f64, limit: f64) {
        let (_, reason) = self.trip.speed_limit_at(self.trip.position_mi);
        let zone = match reason {
            Some(reason) => format!("zone: {reason}"),
            None => "no zone".to_string(),
        };
        log::info!(
            "overspeed {event}: {speed:.1} mph, limit {limit:.0} ({:+.1} over, arms at {:+.0}), \
             mile {:.2}, {zone}",
            speed - limit,
            OVERSPEED_WARN_MPH,
            self.trip.position_mi,
        );
    }

    /// A trooper has lit you up: announce it and wait for the stop.
    pub fn begin_pull_over(&mut self, ctx: &mut GameContext, limit: f64) {
        self.pull_over = Some(PULL_OVER_LIGHTS.to_string());
        self.pull_over_start_mi = self.trip.position_mi;
        self.pull_over_signaled = false;
        self.pull_over_limit = limit;
        self.pull_over_over = 0.0f64.max(self.trip.truck.speed_mph() - limit);
        self.pull_over_kind = "speeding".to_string();
        self.pull_over_title = "Traffic stop".to_string();
        self.pull_over_summary = String::new();
        self.pull_over_fine = 0.0;
        self.pull_over_reputation_hit = 0.0;
        self.pull_over_return = "Back on the highway. Watch your speed.".to_string();
        // Where the violation happened, not where the truck finally stops: a
        // driver clocked in the cones does not get out of the doubled fine by
        // coasting past the last barrel before pulling over.
        self.pull_over_construction_zone = self.trip.in_construction_zone();
        self.pull_over_warning_level = 0;
        self.reset_pull_over_tracker();
        self.pull_over_compliance = PULL_OVER_START_COMPLIANCE;
        self.pull_over_prev_mph = self.trip.truck.speed_mph();
        // The signal key is named until the exit signal has been used a few
        // times (the same counter the stop callouts retire on).
        let speed = ctx.settings.speed_text(self.trip.truck.speed_mph());
        let zone = ctx.settings.speed_text(limit);
        let message = if self.instruction_retired(ctx, "take_exit") {
            format!("Lights and siren behind you. {speed} in a {zone} zone. Signal and stop on the shoulder.")
        } else {
            format!(
                "Lights and siren behind you. {speed} in a {zone} zone. Signal with {} and stop on \
                 the shoulder.",
                ctx.control_hint("take_exit")
            )
        };
        self.arm_pull_over(ctx, &message);
        ctx.controller.rumble.alert();
    }

    /// Shared start for every stop: hands back on the wheel, real clock,
    /// and no judgement until the player has heard the whole instruction.
    ///
    /// The old code started draining compliance the instant the siren played.
    /// Holding a steady speed -- which is what cruise, the speed keeper, or
    /// simply listening looks like -- drained it to zero about five seconds
    /// in, while a thirty-four word instruction was still being spoken. That
    /// charged attentive drivers with a felony for doing nothing wrong.
    pub fn arm_pull_over(&mut self, ctx: &mut GameContext, message: &str) {
        self.trip.pull_over_active = true;
        self.refresh_live_facts();
        self.disarm_speed_control(ctx); // hands back on the wheel to brake
        self.pull_over_grace_s = self.pull_over_grace_seconds(ctx, message);
        // Commit the encounter to the save before a word of it is spoken, so
        // neither a crash nor a quit-to-menu can make it never have happened.
        self.enforcement_events
            .insert(format!("stop:{:.1}", self.trip.position_mi));
        if ctx.profile.is_some() {
            let snapshot = self.snapshot(ctx);
            profile_mut_of(ctx).active_trip = Some(snapshot);
            ctx.save_profile();
        }
        // Cut the radio outright rather than ducking it. The catalog ships
        // dozens of always-available police and fire scanner streams, so a
        // siren over programme material is genuinely ambiguous -- and the
        // sudden silence is itself an unmistakable cue that something has
        // taken the cab over.
        self.cut_radio_for_stop(ctx);
        // Lead with the synthesized enforcement signature, then hold the real
        // siren underneath it. The signature says "this is the game telling
        // you about enforcement"; the siren says what it is.
        self.play_enforcement_marker(ctx, 0.9, 0.0);
        self.hold_stop_siren(ctx);
        ctx.say_event_with(
            message.to_string(),
            SayEvent::new()
                .category(SpeechCategory::Navigation)
                // Only while the stop is still unresolved. This names a
                // maneuver with a consequence on it -- signal, brake, pull
                // onto the shoulder -- and a cut line is handed back so it can
                // finish. Handed back after the truck is stopped and the
                // trooper is at the window, it demands a pull-over that
                // already happened. Found by driving it: `playtest_road --find
                // scale` requeued exactly this line (2026-08-21).
                .valid(live::pull_over_active),
        );
        // One demand at a time: an exit armed for a ramp must not keep
        // announcing and steering for it under the trooper's lights -- that
        // is how a scale bypass became a failure-to-stop cascade.
        if self.stand_down_exit_for_stop(ctx) {
            // ROUTE, not the ambient default: names an automation standing
            // down, right after an interrupting enforcement line that could
            // otherwise bump it stale (automation-handoff sweep, 2026-08-20,
            // the deferred 2026-08-15 audit).
            ctx.say_event_with(
                "Exit approach canceled; plan it again after the stop.",
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Confirmation),
            );
        }
    }

    /// Real seconds to hear the instruction and get a hand to the wheel.
    pub fn pull_over_grace_seconds(&self, ctx: &GameContext, message: &str) -> f64 {
        let speech_rate = if ctx.settings.sapi_events && ctx.speech.event_supports_rate() {
            ctx.settings.speech_rate
        } else {
            0.0
        };
        ramp_arrival_grace_seconds(message, speech_rate)
    }

    pub fn enforcement_bypassed(&self, ctx: &GameContext) -> bool {
        hos::HOS_NON_ENFORCED_MODES.contains(&ctx.settings.hos_mode.as_str())
    }

    pub fn weigh_station_key(&self, stop: &RoadStop) -> String {
        format!("weigh:{}:{:.1}", stop.name, stop.at_mi)
    }

    pub fn check_weigh_station_enforcement(&mut self, ctx: &mut GameContext, previous_mi: f64) {
        // One demand on the driver at a time. This guarded on the stop and the
        // ramp but not on a running hazard deadline, so a scale could speak
        // over a braking window the player had two seconds to make.
        if self.enforcement_bypassed(ctx) {
            return;
        }
        if self.enforcement_busy() {
            // The reminder cannot speak while the cab is taken; one that
            // lands late because of it is the game's late, not the driver's.
            self.note_scale_reminders_held_by_game();
            return;
        }
        if self.departure_ramp_mi.is_some() {
            // Pulling out of a facility, slow on purpose: a reminder held
            // under the bypass speed here is not a crawl past the scale.
            self.note_scale_reminders_held_by_game();
        }
        let stops: Vec<RoadStop> = self
            .trip
            .stops
            .iter()
            .filter(|stop| stop.stop_type == "weigh_station")
            .cloned()
            .collect();
        for stop in stops {
            let ahead = stop.at_mi - self.trip.position_mi;
            let key = self.weigh_station_key(&stop);
            if ahead > 0.0
                && ahead <= self.scale_notice_lookahead_mi(ctx)
                && !self.weigh_station_noticed.contains(&key)
                && self.scale_is_open(&stop)
            {
                // Only an OPEN scale is spoken. A closed one gets the thinner,
                // drier approach bed and nothing said -- the swell says
                // "scale", and the absence of speech is what says "closed".
                // A set, not one key: two open scales inside the lookahead
                // used to take turns overwriting a single key, each frame
                // re-announcing the other, and the nearer one's reminder
                // never fired because its key was never the one held.
                self.weigh_station_noticed.insert(key.clone());
                if ahead <= self.scale_reminder_mi() {
                    // First heard inside the reminder window: the reminder
                    // rides right behind it, and any shortfall in its real
                    // seconds is the game's.
                    self.scale_reminder_held_by_game.insert(key.clone());
                }
                // Its own earcon, not the shared inspection cue: testers
                // could not tell "the scale is ahead" apart from "you are
                // being looked at for something else" (owner ruling,
                // 2026-08-14). The low thump-then-beep reads as the scale on
                // its own, before a word is spoken.
                ctx.audio
                    .play_with("events/weigh_station_warning", 0.7, 0.0);
                // No keys: the old line hard-coded "press T", and T at speed
                // planned a sleep stop past the scale -- the instruction
                // itself marched a tester into the bypass charge (report,
                // 2026-08-12). The reminder nearer the scale names the exit.
                // A transponder truck's verdict is rolled BEFORE the notice
                // is worded (silent, deterministic, so this is the same
                // verdict either way): a green truck used to be told to pull
                // in and then, one sentence later, that it needed no exit -- the notice contradicting itself a breath
                // too late (adversarial battery). A red truck, and every
                // truck without a transponder, still gets the pull-in
                // instruction, which stays true for them.
                let mut verdict = String::new();
                if ctx
                    .profile
                    .as_ref()
                    .is_some_and(has_weigh_station_transponder)
                {
                    verdict = self.roll_transponder_verdict(&stop, &key);
                }
                let instruction = if verdict == "green" {
                    "Transponder bypass."
                } else {
                    "All trucks must pull in."
                };
                // Bound here and moved into the gate below, not read from the
                // sweep when the gate runs: the validity test has to compare
                // against the distance this line actually SPOKE, and the loop
                // rebinds both at the next stop. (Python bound them as lambda
                // defaults for the same reason -- the closure would otherwise
                // capture the loop variable, which ruff B023 names.)
                let announced = ctx.settings.short_distance_text(ahead);
                let scale_mi = stop.at_mi;
                let imperial = ctx.settings.imperial_units;
                self.refresh_live_facts();
                ctx.say_event_with(
                    // short_distance_text, not distance_text: the plain form
                    // rounds to whole miles, so a scale first seen inside half
                    // a mile announced itself "in 0 miles" and the reminder
                    // that followed said "in half a mile" -- the distance
                    // appeared to run backwards while the scale was still
                    // ahead (gate harness, 2026-08-15). This is the same
                    // rounding that made the route key say "0 miles" to a
                    // gate; the colloquial form is what the reminder below
                    // already speaks, so the two now agree.
                    // No mainline speed demand: a real scale has its own
                    // deceleration ramp, and "slow below fifteen" spoken here
                    // had the owner crawling an open interstate at twenty for
                    // five miles, obeying the sentence to the letter
                    // (playtest, 2026-08-20). The bypass judgment never
                    // needed it -- taking the scale's exit is what counts --
                    // and the ramp glide owns the slowing.
                    format!(
                        "Open weigh station ahead in {announced}, {}. {instruction}",
                        stop.name
                    ),
                    SayEvent::queued()
                        .priority(EventPriority::Important)
                        .category(SpeechCategory::Navigation)
                        // valid: this sentence names a distance, and a
                        // distance is a claim about now. The reminder in
                        // `check_scale_reminder` already dies once the exit is
                        // behind the truck; this one has to die sooner,
                        // because it goes wrong while the scale is still
                        // AHEAD -- handed back after the half-mile reminder it
                        // told the driver the scale was four miles off, the
                        // two lines contradicting each other one after the
                        // other (Python adversarial battery,
                        // scale_bypass_to_the_end). The test is the line's OWN
                        // words rather than a chosen tolerance: while the road
                        // left still speaks as the phrase already spoken,
                        // replaying it says nothing untrue, and the moment it
                        // does not, it does.
                        //
                        // Rust: a `valid` gate is `'static`, so it reads the
                        // drive through `live` and re-words through
                        // `short_distance_text_for`, the settings method's own
                        // body with the unit setting passed by value.
                        .valid(move || {
                            let left = scale_mi - live::position_mi();
                            left > 0.0 && short_distance_text_for(left, imperial) == announced
                        }),
                );
                // The verdict line itself queues right behind the notice
                // (both ROUTE, interrupt=False): green says keep rolling,
                // red says pull in.
                if !verdict.is_empty() {
                    self.speak_transponder_verdict(ctx, &stop, &verdict);
                }
            }
            self.check_scale_reminder(ctx, &stop, ahead, &key);
            if self.trip.scale_reminder_hold_mi == Some(stop.at_mi)
                && (ahead <= 0.0 || self.exit_is_armed_for(&stop))
            {
                // Past the gore, or signalled for its ramp, where the armed
                // exit's own real-time window takes the clock over.
                self.trip.scale_reminder_hold_mi = None;
            }
            if self.enforcement_events.contains(&key) {
                continue;
            }
            let crossed = previous_mi < stop.at_mi && stop.at_mi <= self.trip.position_mi;
            if crossed
                && self.scale_is_open(&stop)
                && self.trip.truck.speed_mph() > WEIGH_STATION_BYPASS_MPH
            {
                if self
                    .weigh_station_transponder_verdict
                    .get(&key)
                    .map(String::as_str)
                    == Some("green")
                {
                    // Weigh-in-motion cleared this truck. Rolling past at
                    // mainline speed is exactly what a green light
                    // authorizes, not a bypass to defer or fine.
                    self.enforcement_events.insert(key);
                    continue;
                }
                if self.exit_is_armed_for(&stop) {
                    // Signaled for this scale's own ramp. Whether that is a
                    // check-in or a miss is not decided here: the exit watch
                    // settles it later in this same frame, and until it has,
                    // ramp speed over the bypass threshold proves nothing --
                    // the gore is crossed at ramp speed by definition. A
                    // tester was fined for blowing past a scale while he was
                    // on its ramp at eighteen (log, 2026-08-10).
                    self.weigh_station_pending = Some(stop.clone());
                    continue;
                }
                if !self.scale_bypass_judgeable(&key) {
                    // Audible before it can bite. The driver was never told
                    // about this scale, or heard "Signal for the scale exit"
                    // too few real seconds ago to have acted on it because
                    // the game held it back -- a notice that latched late, a
                    // cab taken inside the window. That is the game's miss,
                    // not the driver's bypass. A reminder made late by the
                    // driver's own crawl does not land here; see
                    // `scale_bypass_judgeable`.
                    let heard = if self.weigh_station_reminder_key == key {
                        format!(
                            "reminder only {:.1} real s before, held back by the game",
                            self.weigh_station_reminder_age_s
                        )
                    } else {
                        "never announced".to_string()
                    };
                    log::info!(
                        "scale crossing not judged: {} at mile {:.2}, {heard}",
                        stop.name,
                        stop.at_mi
                    );
                    self.enforcement_events.insert(key);
                    continue;
                }
                self.enforcement_events.insert(key);
                self.charge_weigh_station_bypass(ctx, &stop);
            }
        }
    }

    /// Roll and speak the weigh-in-motion verdict for a transponder truck.
    ///
    /// Fires once, at the same point the open-scale notice latches. Seeded
    /// off the trip seed and this stop's own key -- the same named-draw
    /// shape as `charge_weigh_station_bypass` -- so a reload cannot
    /// re-roll a scale already passed.
    pub fn resolve_transponder_verdict(
        &mut self,
        ctx: &mut GameContext,
        stop: &RoadStop,
        key: &str,
    ) {
        let verdict = self.roll_transponder_verdict(stop, key);
        self.speak_transponder_verdict(ctx, stop, &verdict);
    }

    /// The weigh-in-motion verdict for this scale, rolled once, silent.
    ///
    /// Split out of `resolve_transponder_verdict` so the open-scale
    /// notice can be worded by the verdict BEFORE either is spoken: a green
    /// truck used to be told "Signal for the scale exit" and then, one
    /// sentence later, that it needed no exit -- a contradiction the notice
    /// resolved a breath too late (adversarial battery). Idempotent: a
    /// stored verdict is returned, never re-rolled.
    pub fn roll_transponder_verdict(&mut self, _stop: &RoadStop, key: &str) -> String {
        if let Some(held) = self.weigh_station_transponder_verdict.get(key) {
            if !held.is_empty() {
                return held.clone();
            }
        }
        let verdict = if self.cargo_is_overweight() {
            // A truck over the legal 80,000 lb GVW is always red-lighted;
            // no roll needed.
            "red".to_string()
        } else {
            let mut rng =
                PyRandom::new_from_str(&format!("{}:scale-transponder:{key}", self.trip_seed));
            if rng.random() < WEIGH_STATION_TRANSPONDER_BYPASS_SHARE {
                "green".to_string()
            } else {
                "red".to_string()
            }
        };
        self.weigh_station_transponder_verdict
            .insert(key.to_string(), verdict.clone());
        verdict
    }

    pub fn speak_transponder_verdict(
        &mut self,
        ctx: &mut GameContext,
        stop: &RoadStop,
        verdict: &str,
    ) {
        let scale_mi = stop.at_mi;
        if verdict == "green" {
            if let Some(name) = self.co_driver_at_wheel_name() {
                let line = format!("The transponder cleared {name} past {}.", stop.name);
                self.co_driver_note(ctx, line, NoteWeight::Passing);
            }
        }
        if verdict == "green" {
            ctx.audio.play_with("events/scale_green", 0.8, 0.0);
            // valid: never rescued. One verdict, one line (the adversarial
            // watcher's rule) -- a hazard cutting it used to make the
            // clearance speak twice, and the scale_green tone already
            // carried the verdict; the status keys can re-answer.
            ctx.say_event_with(
                "Green light. Cleared past the scale.",
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Navigation)
                    .valid(|| false),
            );
        } else {
            ctx.audio.play_with("events/scale_red", 0.7, 0.0);
            // A red is an instruction that demands action, so a cut one IS
            // rescued -- but only while the scale is still ahead to pull
            // in to.
            self.refresh_live_facts();
            // Named, like the notice it follows: with two scales near, "the
            // scale" did not say which one the light was for.
            ctx.say_event_with(
                format!("Red light. Pull in to {}.", stop.name),
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .category(SpeechCategory::Navigation)
                    .valid(move || live::position_mi() < scale_mi),
            );
        }
    }

    /// Whether this load is over the federal 80,000 lb GVW cap.
    ///
    /// Tractor, trailer, cargo, and remaining diesel all count. An overweight
    /// truck is always red-lighted at a transponder scale.
    pub fn cargo_is_overweight(&self) -> bool {
        self.trip.truck.is_over_legal_gvw()
    }

    /// Whether this stop's own exit is the one the driver is committed to.
    pub fn exit_is_armed_for(&self, stop: &RoadStop) -> bool {
        let active = self.ramp_stop.as_ref().or(self.exit_stop.as_ref());
        active.is_some_and(|active| active.key() == stop.key())
    }

    /// Judge a deferred scale crossing now that the exit watch has run.
    ///
    /// On the scale's own ramp, the driver pulled into the inspection lane
    /// and owes nothing. Anything else -- too fast for the ramp, out of the
    /// exit lane, the signal canceled at the gore -- is the same bypass it
    /// would have been with no signal at all, so arming the exit and then
    /// driving on buys nothing.
    pub fn resolve_weigh_station_bypass(&mut self, ctx: &mut GameContext) {
        let Some(stop) = self.weigh_station_pending.take() else {
            return;
        };
        let key = self.weigh_station_key(&stop);
        if self.enforcement_events.contains(&key) {
            return;
        }
        self.enforcement_events.insert(key.clone());
        if self
            .ramp_stop
            .as_ref()
            .is_some_and(|ramp| ramp.key() == stop.key())
        {
            return; // pulled in; the scale gets its look at the check-in
        }
        if self.pull_over.is_some() {
            return; // already stopped this frame; one demand on the driver
        }
        if self.weigh_station_reminder_key == key
            && self.weigh_station_reminder_age_s < SCALE_REMINDER_REAL_LEAD_S
            && self.scale_reminder_held_by_game.contains(&key)
        {
            // Signalled in answer to a reminder the game held back until too
            // late to make the ramp: the same miss on the game's side as an
            // unarmed crossing inside the reminder's real seconds. A driver
            // who armed before any reminder was needed knew about the scale
            // all along, and a reminder late only because of their own crawl
            // buys nothing either; a missed ramp is still theirs.
            log::info!(
                "scale crossing not judged: {} at mile {:.2}, armed after a reminder only \
                 {:.1} real s before",
                stop.name,
                stop.at_mi,
                self.weigh_station_reminder_age_s
            );
            return;
        }
        self.charge_weigh_station_bypass(ctx, &stop);
    }

    pub fn charge_weigh_station_bypass(&mut self, ctx: &mut GameContext, stop: &RoadStop) {
        // Caught, not certain -- steep, per WEIGH_STATION_BYPASS_CATCH_CHANCE.
        // Named, seeded, and settled once per scale: a reload cannot re-roll
        // whether the bypass unit got you. Missing it is silent by design --
        // getting away with it is part of the tension.
        let key = self.weigh_station_key(stop);
        let mut rng = PyRandom::new_from_str(&format!("{}:scale-bypass:{key}", self.trip_seed));
        if rng.random() >= WEIGH_STATION_BYPASS_CATCH_CHANCE {
            return;
        }
        let lights_message = format!(
            "Scale bypass enforcement. Lights and siren behind you. Signal with {} and stop on \
             the shoulder.",
            ctx.control_hint("take_exit")
        );
        self.begin_enforcement_pull_over(
            ctx,
            "weigh_station_bypass",
            "Weigh station bypass stop",
            &format!(
                "Scale officers saw you blow past {} instead of pulling into the inspection lane.",
                stop.spoken_name()
            ),
            WEIGH_STATION_BYPASS_FINE,
            hos::HOS_REPUTATION_HIT,
            "Back on the highway. Watch for the next open scale.",
            &lights_message,
        );
    }

    pub fn check_unsafe_damage_enforcement(&mut self, ctx: &mut GameContext) {
        if self.enforcement_bypassed(ctx) || self.enforcement_busy() {
            return;
        }
        if self.trip.truck.damage_pct < UNSAFE_DAMAGE_STOP_PCT
            || self.trip.truck.speed_mph() <= DOCKING_MAX_MPH
        {
            return;
        }
        let Some(reason) = self
            .trip
            .active_post_at(self.trip.position_mi)
            .map(|post| post.reason().to_string())
        else {
            return;
        };
        let key = format!("unsafe_damage:{:.1}", self.trip.position_mi);
        if key == self.unsafe_damage_stop_key || self.enforcement_events.contains(&key) {
            return;
        }
        self.unsafe_damage_stop_key = key.clone();
        self.enforcement_events.insert(key);
        let summary = format!(
            "A trooper in this {reason} saw visible truck damage at {:.0} percent and ordered a \
             roadside safety inspection.",
            self.trip.truck.damage_pct
        );
        let lights_message = format!(
            "Unsafe equipment stop. Lights and siren behind you. Signal with {} and stop on the \
             shoulder.",
            ctx.control_hint("take_exit")
        );
        self.begin_enforcement_pull_over(
            ctx,
            "unsafe_damage",
            "Unsafe equipment stop",
            &summary,
            UNSAFE_DAMAGE_FINE,
            hos::HOS_REPUTATION_HIT,
            "Back on the highway. Repair the truck at the next safe stop.",
            &lights_message,
        );
    }

    #[allow(clippy::too_many_arguments)]
    pub fn begin_enforcement_pull_over(
        &mut self,
        ctx: &mut GameContext,
        kind: &str,
        title: &str,
        summary: &str,
        fine: f64,
        reputation_hit: f64,
        return_message: &str,
        lights_message: &str,
    ) {
        self.pull_over = Some(PULL_OVER_LIGHTS.to_string());
        self.pull_over_start_mi = self.trip.position_mi;
        self.pull_over_signaled = false;
        self.pull_over_limit = 0.0;
        self.pull_over_over = 0.0;
        self.pull_over_kind = kind.to_string();
        self.pull_over_title = title.to_string();
        self.pull_over_summary = summary.to_string();
        self.pull_over_fine = fine;
        self.pull_over_reputation_hit = reputation_hit;
        self.pull_over_return = return_message.to_string();
        // Captured with the observation, for the same reason as the speeding
        // stop: the zone that matters is the one the violation happened in.
        self.pull_over_construction_zone = self.trip.in_construction_zone();
        self.pull_over_warning_level = 0;
        self.reset_pull_over_tracker();
        self.pull_over_compliance = PULL_OVER_START_COMPLIANCE;
        self.pull_over_prev_mph = self.trip.truck.speed_mph();
        self.arm_pull_over(ctx, lights_message);
    }

    /// X during a pull-over: signal and ease over (better demeanor).
    pub fn signal_pull_over(&mut self, ctx: &mut GameContext) {
        if self.pull_over.as_deref() == Some(PULL_OVER_LIGHTS) {
            self.pull_over = Some(PULL_OVER_STOPPING.to_string());
            self.pull_over_signaled = true;
            // A one-time compliance bump for signaling. Guarded so that if an
            // unsignal is ever added, toggling can never re-earn the boost.
            if !self.pull_over_signal_boost {
                self.pull_over_signal_boost = true;
                self.pull_over_compliance =
                    1.0f64.min(self.pull_over_compliance + PULL_OVER_SIGNAL_BOOST);
            }
            ctx.audio.play_with("vehicle/signal_tone", 0.7, 0.6);
            ctx.say("Signaling onto the shoulder.");
        } else {
            ctx.say("Pulling over to the shoulder.");
        }
    }
}
