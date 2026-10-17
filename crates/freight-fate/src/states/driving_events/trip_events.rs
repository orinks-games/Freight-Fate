//! `_handle_trip_event` and everything that decides whether a trip event
//! speaks at all, in what category, and at what priority.

use crate::speech::EventPriority;
use ff_core::data::state_welcome::welcome_sign;
use ff_core::data::world_parsing::crc32;
use ff_core::models::trailer_yard::pickup_plan;
use ff_core::pyrandom::PyRandom;
use ff_core::sim::driving_modes::tuning_for_time_scale;
use ff_core::sim::trip_models::{TripEvent, TripEventKind};
use ff_core::speech_pacing::SpeechCategory;
use ff_core::speech_text::{
    cruise_curve_easing, cruise_curve_paused, cruise_curve_paused_assisted, curve_assist_slowing,
    roadside_chatter, stop_callout, SpokenMessage, StopCalloutParts,
};

use crate::app::{GameContext, Say, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_turns::{is_judged_turn, TURN_COMMIT_TAIL_MI};
use crate::states::driving_updates::live;

use super::ambient::Ambient;
use super::event_category_for_kind;

/// How fast the vehicle this hazard is about is going, or None.
///
/// A traffic hazard carries the lead's own context; debris, an animal or a
/// stopped truck carries nothing, and None is what says "this thing is not
/// going anywhere".
fn hazard_lead_speed(event: &TripEvent) -> Option<f64> {
    let speed = event.data.traffic.as_ref()?.lead.speed_mph;
    if speed.is_nan() {
        return None;
    }
    Some(0.0f64.max(speed))
}

impl DrivingState {
    /// `_handle_trip_event(event)`: everything the road just said, delivered.
    pub fn handle_trip_event(&mut self, ctx: &mut GameContext, event: &TripEvent) {
        if self.should_ignore_destination_exit_gps_cue(ctx, event) {
            return;
        }
        if self.should_ignore_untaken_destination_facility_event(event) {
            return;
        }
        if self.should_ignore_unreachable_zone_cue(ctx, event) {
            return;
        }
        if self.should_ignore_unsignalled_exit_pressure(ctx, event) {
            return;
        }
        if self.corner_already_called(event) {
            return;
        }
        if self.limit_change_of_road_left(ctx, event) {
            return;
        }
        if matches!(
            event.kind,
            TripEventKind::Billboard | TripEventKind::Landmark
        ) && self.in_exit_approach()
        {
            // Roadside colour waits out the last mile to an exit being taken:
            // a billboard read 0.3 miles before the gore talked over the
            // countdown and the take line (agent drive B, 2026-09-24).
            return;
        }
        let kind = event.kind;
        // The route's own call names the turn too, so when it is the first
        // word about a corner, the corner's chime goes out with it.
        if kind == TripEventKind::GpsCue
            && (ctx.settings.speaks(Self::event_category(event)) || !ctx.ladder_applies())
        {
            if let Some(cue) = event.data.cue.as_ref().filter(|cue| is_judged_turn(cue)) {
                self.chime_corner(ctx, cue);
            }
        }
        let sound = route_event_sound(event);
        let mut message = event.message.clone();
        // Cue strings stay "billed to carrier settlement" so trip-cue tests
        // stay green. Owner-ops hear the money move at speak time.
        if ctx
            .profile
            .as_ref()
            .is_some_and(|p| is_owner_operator(&p.business_status))
        {
            message.normal = message
                .normal
                .replace("billed to carrier settlement", "charged to your settlement");
            if let Some(terse) = message.terse.as_mut() {
                *terse = terse.replace(", carrier.", ", your settlement.");
            }
        }
        if kind == TripEventKind::Lane && self.terse_speech(ctx) {
            // lane-count callouts are a normal-verbosity nicety, muted whole
            return;
        }
        if matches!(kind, TripEventKind::Landmark | TripEventKind::Billboard) {
            // Ambient roadside color, filtered by the player's chatter
            // switches at speak time so a mid-trip settings change applies
            // immediately. A muted callout is dropped whole -- it never
            // becomes the A-key replay either.
            //
            // The switch decides WHAT is heard and verbosity decides how much
            // is said about it; the two are separate axes. Terse used to mute
            // roadside chatter wholesale, which left a terse player five
            // switches that were on, looked live, and did nothing at all
            // (owner, 2026-08-15). An enabled category now speaks in either
            // mode, in terse as its short form.
            let category = event.data.category.clone().unwrap_or_default();
            if !ctx.settings.chatter_enabled(&category) {
                return;
            }
            // Town and village names answer to the place-callouts ladder, not
            // the chatter switches: sparse keeps only the names that explain
            // a speed limit change, all adds the towns the route passes. That
            // ladder is untouched here, terse muting included -- these are
            // places, not chatter, and they are already at their short form.
            if category == "village" {
                if self.terse_speech(ctx) {
                    return;
                }
                let mode = ctx.settings.place_callouts.clone();
                if mode == "off" {
                    return;
                }
                if mode == "sparse" && !event.data.explains_limit.unwrap_or(false) {
                    return;
                }
            } else {
                message = roadside_chatter(event.text(), &category);
            }
        }
        if kind == TripEventKind::Checkpoint && ctx.settings.place_callouts != "all" {
            // Curated route-town markers ("Passing X on I-40") are places,
            // not safety -- only the loudest place tier speaks them.
            return;
        }
        if kind == TripEventKind::GpsCue {
            if let Some(cue) = event.data.cue.as_ref() {
                if cue.kind == "checkpoint" {
                    // The two-mile advance for a place earns nothing at any tier:
                    // a town is not actionable the way an exit or toll is.
                    return;
                }
            }
        }
        if !message.normal.is_empty() && kind != TripEventKind::Hazard {
            self.last_event_message = message.normal.clone(); // replayable with A
        }
        if let Some(post) = event.data.cb_patrol.as_ref() {
            // Recorded when the CB call is HANDLED, not when the voice finally
            // gets to it: the ambient queue can drop a line that waited out a
            // hazard, and a driver who heard the squelch and nothing else is
            // precisely who reaches for Alt C. Same reasoning as
            // `log_ambient_event`, which files the line at queue time for the
            // same reason.
            self.last_cb_chatter = Some(CbChatterRecall {
                text: message.normal.clone(),
                post: Some(post.clone()),
            });
        }
        let category = Self::event_category(event);
        // An event's tone announces its words, so a rung that drops the
        // words drops the tone (owner, 2026-10-03). A passing vehicle's
        // whoosh is the road itself and keeps playing.
        let silenced = !ctx.event_speaks(category);
        let sound = sound.filter(|key| !(silenced && key.starts_with("events/")));
        match kind {
            TripEventKind::Hazard => self.handle_hazard_event(ctx, event, sound, message),
            TripEventKind::Inspection => self.handle_inspection(ctx, event),
            TripEventKind::WeatherChange => {
                let key = self.ambient_key(event);
                self.speak_ambient_event(ctx, message, Ambient::new().category(category).key(key));
                self.record_weather_achievement(ctx);
            }
            TripEventKind::TollCharged => {
                // Money is a consequence, not chatter: the charged line rides
                // ROUTE's never-dropped contract instead of the one-deep ambient
                // slot, where the next hazard or piece of chatter could silently
                // destroy it. (The toll-ahead heads-up stays ambient.)
                if !silenced {
                    ctx.audio.play(sound.unwrap_or("ui/notify"));
                }
                let mut opts = SayEvent::queued().priority(EventPriority::Important);
                opts.category = category;
                ctx.say_event_with(message, opts);
                ctx.award_driving_achievement("toll_paid");
            }
            TripEventKind::StateCrossing => {
                self.handle_state_crossing(ctx, event, sound, message, category)
            }
            TripEventKind::TimezoneCrossing => {
                if let Some(sound) = sound {
                    ctx.audio.play(sound);
                }
                let terse = self.terse_speech(ctx);
                let mut opts = SayEvent::queued();
                opts.category = category;
                ctx.say_event_with(timezone_crossing_message(event, terse), opts);
            }
            TripEventKind::Curve => self.handle_curve_event(ctx, event, message, category),
            TripEventKind::Landmark | TripEventKind::Billboard => {
                let sign = event.data.category.as_deref().unwrap_or_default();
                let message = self.billboard_message(ctx, sign, message);
                self.speak_ambient_event(ctx, message, Ambient::new().category(category));
            }
            TripEventKind::Lane => {
                // Road-status color: how many lanes the road just became. Ambient,
                // so it yields to safety cues and is muted whole in terse speech.
                self.speak_ambient_event(ctx, message, Ambient::new().category(category));
            }
            // handled by _arrive()
            TripEventKind::Arrived => {}
            _ => {
                if self.event_disables_cruise(ctx, event) {
                    self.cancel_cruise_for_restricted_area(ctx, event, message, category);
                } else {
                    self.speak_plain_route_event(ctx, event, sound, message, category);
                }
            }
        }
        if kind == TripEventKind::ZoneEnter {
            if !silenced {
                ctx.audio.play(sound.unwrap_or("ui/notify"));
            }
            let reason = event
                .data
                .zone
                .as_ref()
                .map(|zone| zone.reason.clone())
                .unwrap_or_default();
            if reason == "construction" {
                self.construction_seen = true;
                ctx.award_driving_achievement("construction_zone");
            } else if reason == "heavy traffic" {
                self.traffic_seen = true;
                ctx.award_driving_achievement("traffic_slowing");
            }
        }
        if kind == TripEventKind::GpsCue {
            let traffic_cue = event
                .data
                .cue
                .as_ref()
                .is_some_and(|cue| cue.kind == "traffic");
            if traffic_cue || event.data.traffic_pressure.is_some() {
                self.traffic_seen = true;
                ctx.award_driving_achievement("traffic_slowing");
            }
        }
        if self.construction_seen && self.traffic_seen {
            ctx.award_driving_achievement("jam_and_cones");
        }
    }

    /// The HAZARD branch of `_handle_trip_event`.
    fn handle_hazard_event(
        &mut self,
        ctx: &mut GameContext,
        event: &TripEvent,
        sound: Option<&'static str>,
        message: SpokenMessage,
    ) {
        if self.ramp_mi.is_some() {
            return; // off the highway: the hazard passes you by
        }
        // The queue is NOT discarded here any more. A hazard blocks the
        // drain while it is live and the waiting lines age out on their
        // own if it runs long; a short one no longer costs the driver the
        // state line they were crossing when it fired.
        ctx.audio.play(sound.unwrap_or("ui/warning"));
        ctx.controller.rumble.hazard(); // 750 ms right->left sweep
                                        // The deadline is the moment the assist has to act plus the time
                                        // that is the driver's own. The rolled window covers hearing the
                                        // warning and getting on the pedal, and fatigue eats into that
                                        // part only -- a drowsy driver reacts late, but the truck stops
                                        // no slower, and no driver reacts below the human floor.
                                        // A dodgeable hazard sits in the lane you are in *now*; ending up
                                        // in any other lane before the deadline clears it, if that lane
                                        // is actually open (see _finish_lane_change). By brake alone it
                                        // takes nearly a stop, so its deadline budgets the longer stop.
        let name = event
            .data
            .name
            .clone()
            .filter(|name| !name.is_empty())
            .unwrap_or_else(|| "it".to_string());
        let dodgeable = event.data.dodgeable.unwrap_or(false);
        // What the thing IS, kept apart from whether there is anywhere to
        // go. An event that says nothing falls back to the dodgeable flag,
        // which is what the older callers meant by it.
        let in_lane = event.data.in_lane.unwrap_or(dodgeable);
        let lead_mph = hazard_lead_speed(event);
        let shape = HazardShape {
            dodgeable,
            in_lane,
            lead_mph,
        };
        let slack = event.data.deadline_s.unwrap_or(4.0);
        let reaction = tuning_for_time_scale(self.trip.time_scale).reaction_window;
        // Computed on THIS hazard's own shape, before it is folded with
        // whatever else may be pending -- its budget (the lane-tap allowance
        // included, and the speed braking alone has to reach) is a property
        // of itself, not of the combined wording the fold branch below
        // settles on.
        let fatigue = profile_of(ctx).fatigue;
        let new_deadline = self.hazard_deadline_for(
            slack * reaction * hos::reaction_window_mult(fatigue),
            Some(shape),
        );
        if self.hazard_deadline.is_none() {
            // A fresh hazard starts the assist from an open pedal, with
            // nothing measured yet from the last one.
            self.hazard_names = vec![name];
            self.horn_scare_tried = false;
            self.hazard_dodgeable = dodgeable;
            self.hazard_in_lane = in_lane;
            self.hazard_lead_mph = lead_mph;
            self.hazard_deadline = Some(new_deadline);
            self.hazard_lane = self.lane.lane;
            self.release_hazard_brake();
        } else if self.trip.truck.speed_mph() <= self.hazard_target_mph(None) {
            // A hazard is already pending, but the driver has already
            // outrun it -- it earns its own clean resolution line before
            // this one starts, instead of being silently dropped by the
            // overwrite this used to be (Shane's deer, 2026-08-14).
            self.clear_hazard(ctx);
            self.hazard_names = vec![name];
            self.horn_scare_tried = false;
            self.hazard_dodgeable = dodgeable;
            self.hazard_in_lane = in_lane;
            self.hazard_lead_mph = lead_mph;
            self.hazard_deadline = Some(new_deadline);
            self.hazard_lane = self.lane.lane;
            self.release_hazard_brake();
        } else {
            // Still live: fold the new one in rather than clobber it.
            // Any non-dodgeable hazard in the mix means "ease around" is
            // the wrong promise for the group, so it always wins the
            // wording; the shorter deadline is the one still governing
            // how much time is actually left.
            self.hazard_names.push(name);
            // Folding a second hazard in: the slower demand wins, and a
            // thing that is not moving has no lead speed at all, so it
            // takes the group back to the near-stop rule.
            self.hazard_lead_mph = match (self.hazard_lead_mph, lead_mph) {
                (Some(live), Some(folded)) => Some(live.min(folded)),
                _ => None,
            };
            // Either one sitting in the lane takes the group to the near
            // stop: the object is still there whatever else arrived with it.
            self.hazard_in_lane = self.hazard_in_lane || in_lane;
            self.hazard_dodgeable = self.hazard_dodgeable && dodgeable;
            self.hazard_deadline = self.hazard_deadline.map(|live| live.min(new_deadline));
        }
        // _hazard_lane is stamped by the two FRESH branches above and by
        // nothing else. Re-stamping it here put the hazard in whatever
        // lane the truck had just reached, so a hazard folding in while
        // the driver was answering the last one moved with them: dodge,
        // get re-armed in the new lane, dodge again. "The repeating
        // happened every time I was changing lanes until the two-three
        // repeats are done" -- Shane, 2026-08-21, and that is the loop.
        // The lane belongs to the hazard, not to the truck.
        self.hazard_slow_hint_said = false;
        // Lane keeping on full answers a slow vehicle by passing it, and the
        // call says so instead of asking the driver to change lanes.
        let mut category = Self::event_category(event);
        let message = match event.data.pass_message.clone() {
            Some(pass) if self.hazard_dodgeable && self.pass_for_hazard(ctx, event) => {
                // Lane keeping is answering it, so nothing is asked of the
                // driver: going around a slow truck is traffic, words at
                // standard only. Automatic braking is still SAFETY if the
                // pass cannot be made.
                category = Some(SpeechCategory::Traffic);
                pass
            }
            _ => message,
        };
        // A dodgeable hazard leaves the wheel alone: adaptive cruise or
        // the keeper stays armed through the lane change that answers it,
        // and only braking -- the driver's own, or the automatic brake
        // taking over near the deadline (see ``_update_hazard``) --
        // cancels the session. A hazard with no dodge in it (or one
        // folded in with a brake-only hazard, which always wins the
        // group's wording, see above) has no such answer, so hands go
        // back to the pedals right away (Shane, 2026-08-14: a lane change
        // was killing cruise outright, not just easing off the lane being
        // passed -- that narrower bug is 3cbdcffb).
        let speed_control_was_active = !self.hazard_dodgeable
            && (self.speed_control_armed || self.cruise_mph.is_some() || self.keeper_mph.is_some());
        if speed_control_was_active {
            // Hands back on the wheel to brake -- but the session stays
            // armed: a highway hazard pauses cruise, and it resumes once the
            // hazard is over and the truck is rolling off the brakes (owner
            // ruling, 2026-09-01, heard live on I-90: "hazard disabled auto
            // speed control"). The resume path holds off while the hazard
            // is live, so this cannot re-engage into it.
            self.cancel_cruise(ctx, true);
            self.cancel_keeper(ctx, true);
        }
        // The normal/terse pair rides the event from the sim layer; the
        // delivery layer picks the rendering (R5), so no rewriting here.
        let message = if speed_control_was_active {
            message.plus("Automatic speed control paused.")
        } else {
            message
        };
        self.last_event_message = message.normal.clone();
        // A hazard call may only come back while the hazard is still
        // live. An interrupting line hands back what it cut so the cut
        // line finishes rather than vanishing, which is what rescued
        // "you swerve around the brake lights" -- but handed back after
        // the truck is clear, "Change lanes or brake!" tells the driver
        // to dodge something that is no longer there. Same rule the
        // scale and destination exit instructions already carry: a
        // rescued line has to still be TRUE (Shane, 2026-08-21, on the
        // retread debris call).
        //
        // Rust: `valid` outlives the borrow of `self`, so it reads the drive
        // through `live`, exactly as the scale reminder does. It used to
        // project the deadline forward onto the wall clock instead, which
        // answers "still live" for a hazard the driver has already dodged
        // (and for the whole of any run the wall clock outpaces) -- the
        // question Python asks is whether a hazard is armed at all, not how
        // much of its window is nominally left.
        self.refresh_live_facts();
        let mut opts = SayEvent::new().valid(live::hazard_active);
        opts.category = category;
        ctx.say_event_with(message, opts);
    }

    /// The STATE_CROSSING branch of `_handle_trip_event`.
    fn handle_state_crossing(
        &mut self,
        ctx: &mut GameContext,
        event: &TripEvent,
        sound: Option<&'static str>,
        message: SpokenMessage,
        category: Option<SpeechCategory>,
    ) {
        let state = event
            .data
            .cue
            .as_ref()
            .map(|cue| cue.near_text.clone())
            .unwrap_or_else(|| event.text().to_string());
        if let Some(profile) = ctx.profile.as_mut() {
            add_unique_stat(profile, "states_crossed", &state);
        }
        let mut message = message;
        // The welcome sign: authored content that sat unwired since it
        // shipped ("the placement that actually speaks these is
        // gameplay-layer follow-on" -- its own docstring), until Brandon
        // asked why state signs are not read (2026-08-20). The into
        // state rides the cue id's last segment, built from into_state
        // in _build_navigation_cues. Rides the billboard chatter switch
        // -- it is literally roadside signage -- and picks seeded so a
        // replayed crossing reads the same sign.
        if ctx.settings.chatter_billboards {
            if let Some(cue) = event.data.cue.as_ref() {
                let into_state = cue.key.rsplit(':').next().unwrap_or("").to_string();
                // crc32, not hash(): str hash is randomized per process and
                // would pick a different sign on every launch.
                let mut rng =
                    PyRandom::new_from_i64(self.trip_seed ^ crc32(into_state.as_bytes()) as i64);
                let sign = welcome_sign(&into_state, &mut rng);
                if !sign.is_empty() {
                    message = SpokenMessage::pair(
                        format!("{} {sign}", message.normal),
                        message.terse.map(|terse| format!("{terse} {sign}")),
                    );
                }
            }
        }
        self.speak_ambient_event(
            ctx,
            message,
            Ambient::new()
                .sound(sound)
                .category(category)
                .priority(EventPriority::Important),
        );
        ctx.award_driving_achievement("state_crossing");
    }

    /// The CURVE branch of `_handle_trip_event`.
    fn handle_curve_event(
        &mut self,
        ctx: &mut GameContext,
        event: &TripEvent,
        message: SpokenMessage,
        category: Option<SpeechCategory>,
    ) {
        // Curve approach warnings are critical navigation cues: they
        // preempt ambient chatter and play on the event voice.
        if self.hazard_deadline.is_some() || self.ramp_mi.is_some() {
            return;
        }
        // Curve callouts off silences the WORDS and the cue, never the
        // assist: this used to return here, so a driver with callouts off
        // and curve speed assistance on got neither -- cruise carried the
        // owner's truck into a 35 mph bend at 90 km/h and shifted the load
        // 31 percent across two bends before anyone knew (agent drive,
        // 2026-09-01, the owner's own settings).
        let announce = ctx.settings.curve_callouts;
        let curve = event.data.curve;
        // The number spoken for this load, as the pacenote speaks it.
        let advisory = curve
            .as_ref()
            .map_or(event.data.advisory_mph.unwrap_or(0.0), |c| {
                self.spoken_advisory_mph(c) as f64
            });
        let ahead = event.data.ahead_mi.unwrap_or(0.0);
        let speed = self.trip.truck.speed_mph();
        let message = match curve.as_ref() {
            Some(curve) => self.pacenote_text(ctx, curve, ahead, speed),
            None => message,
        };
        self.last_event_message = message.normal.clone();
        // A rescued curve call has to still be true when it comes back:
        // past the bend, or already slowed for it, and the words are a
        // lie by the time they are spoken (Shane P, 2026-08-21).
        //
        // Rust: the predicate is a live read of the trip, which a 'static
        // closure cannot borrow, so the bend's numbers ride the gate and the
        // readings come from `live`.
        let curve_valid = self.curve_call_still_true(curve.as_ref());
        self.refresh_live_facts();
        // A curve call sounds like any other announcement until it has
        // a signature: a short cue panned to the curve's side marks
        // "road shape ahead", never a steering command -- the owner
        // steered a lane change off a bare "Sharp left" (playtest,
        // 2026-07-18). One-shot, not the continuous steering tone the
        // community ruled out. Placeholder sound until a dedicated cue
        // is auditioned (docs/sound-hunt-brief.md, need 1).
        // The panned curve chime that used to fire here is gone (owner,
        // 2026-09-18): a one-shot beep says a bend is coming and then stops
        // saying anything, and the driver still has the whole bend to steer.
        // The engine's lean carries that continuously now, so the chime was
        // noise stacked in front of the guide rather than help.
        //
        // A call cuts chatter, never a warning or another call still being
        // spoken: it falls in behind them. Cutting, a run of esses played
        // backwards -- the next bend's call over this one's, this one handed
        // back after it -- and a too-fast warning cut by a call came back
        // after the call, late (bend sweep, US-62, 2026-09-24).
        let say_curve = |ctx: &mut GameContext, text: SpokenMessage| {
            if !announce {
                return;
            }
            let interrupt = !ctx.event_voice_critical();
            let mut opts = SayEvent::new()
                .interrupt(interrupt)
                .priority(EventPriority::Critical);
            opts.category = category;
            if let Some(valid) = curve_valid {
                opts = opts.valid(move || valid.holds());
            }
            ctx.say_event_with(text, opts);
        };
        // The CHAIN's number and extent, not just this bend's. A call
        // that carries a linked follower ("then sharp left, advise 30")
        // is the follower's only warning -- the trip suppresses its own
        // call so the pair is one sentence -- so easing to the first
        // bend's 40 and releasing at the first bend's end took the truck
        // into the follower ten miles an hour too fast, with nothing
        // left to warn it. Darren's load shifted 12 percent on exactly
        // that pair on NY-12 (2026-08-23), and the spoken line had named
        // 30 the whole time: the words were right and the assist was
        // not. The cruise pause below keys its resume to the same extent,
        // and the approach servo holds to it.
        //
        // And never faster than the bend lets THIS load through at no cost:
        // an advisory is built for a full trailer at 0.30 g, so on a bend
        // whose sign rounds up, or with a part-filled tank, the sign asks more
        // than the load can give (`driving_rollover`). A servo band under it,
        // so the hold's own band stays clear too (`curve_hold_mph`).
        let chain = curve.as_ref().map(|curve| {
            let mut hold_mph = self.curve_hold_mph(ctx, curve);
            let mut hold_to_mi = curve.start_mi.max(curve.end_mi);
            if let Some(linked) = self.pacenote_linked(curve) {
                hold_mph = hold_mph.min(self.curve_hold_mph(ctx, &linked));
                hold_to_mi = hold_to_mi.max(linked.start_mi).max(linked.end_mi);
            }
            (hold_mph, hold_to_mi)
        });
        // THE APPROACH SERVO, in every driving mode. With curve speed
        // assistance on and the truck over the chain's number, the assist
        // takes the brakes from here: down to the advisory by the bend's
        // start, held through the chain and its commit tail, then released.
        // Until tonight the only proactive move was cruise's easing cap, so
        // a manual driver or the speed keeper got the words and nothing
        // else, and cruise's own bounded ramp could still arrive hot -- the
        // owner's US-83 bend, 35 mph at 90 km/h with every assist on, load
        // shifted 31 percent (2026-09-01). The driver's own brake cancels it
        // for the bend, as everywhere. `spoke` is whether THIS call names the
        // assist, so a release can be paired to it; cruise's easing line
        // below names cruise instead, and the servo stays its silent backstop.
        let servo =
            chain.filter(|(hold_mph, _)| ctx.settings.curve_speed_assist && speed > *hold_mph);
        let cruise_above = self.cruise_mph.is_some_and(|set| set > advisory + 5.0);
        let cruise_easing = cruise_above && advisory >= CRUISE_MIN_MPH;
        if let Some((hold_mph, hold_to_mi)) = servo {
            let start_mi = curve
                .as_ref()
                .map_or(self.trip.position_mi + ahead, |c| c.start_mi);
            self.arm_curve_servo(
                hold_mph,
                start_mi,
                hold_to_mi + TURN_COMMIT_TAIL_MI,
                announce && !cruise_easing,
            );
        }
        // A curve well above the cruise set point: with curve speed
        // assistance on, the bend is cruise's job -- cap the working
        // target to the advisory the way an armed exit caps for its
        // ramp, and climb back silently past the bend. Cancel to manual
        // only when the advisory sits below what cruise can hold at all
        // (owner direction, 2026-07-22 playtest: all-assists drivers
        // must not be dropped to the pedals for an ordinary bend).
        if cruise_above {
            let assisted = ctx.settings.curve_speed_assist && curve.is_some() && cruise_easing;
            if assisted {
                let (hold_mph, hold_to_mi) = chain.expect("assisted needs a curve");
                self.cruise_curve_mph = Some(hold_mph);
                self.cruise_curve_end_mi = Some(hold_to_mi);
                // Terse speaks the pacenote alone: its advisory number is
                // the number cruise is easing to, and the deceleration
                // itself is audible (R4's curve-composite row).
                let text = cruise_curve_easing(&message, &ctx.settings.speed_text(advisory));
                say_curve(ctx, text);
            } else {
                // Under cruise's floor (or with the assist off): the bend is
                // the driver's, but the SESSION is not over. This used to
                // disarm, and the truck coasted out of the bend with cruise
                // gone for good -- the same failure the hazard pause removed
                // (owner ruling, 2026-09-01: on the highway a hazard or a
                // curve pauses adaptive cruise and it comes back). Keep the
                // session armed and name the mile the pause lifts at: the
                // chain's end plus the commit tail, so the resume path cannot
                // wind the truck back up mid-corner. The driver's own brake
                // through the bend cancels nothing here -- cruise is already
                // off the pedal -- and the resume waits for them to be off
                // it again. A bare pacenote with no bend geometry has no end
                // to key on; the bend's own start is the honest fallback.
                let position = self.trip.position_mi;
                let hold_to_mi = chain.map_or(position + ahead, |(_, end)| end);
                self.cancel_cruise(ctx, true);
                self.cruise_resume_after_mi = Some(hold_to_mi + TURN_COMMIT_TAIL_MI);
                // With the servo armed the bend is not the driver's after
                // all: the assist brings the truck down and cruise comes back
                // past it. Say that, not the handback.
                let text = if servo.is_some() {
                    cruise_curve_paused_assisted(&message)
                } else {
                    cruise_curve_paused(&message)
                };
                say_curve(ctx, text);
            }
        } else {
            // Interrupting chatter: a pacenote queued behind landmark chatter
            // arrived with the bend three seconds away instead of a
            // quarter mile (owner's AZ-260 log, 2026-07-19 -- the words
            // were honest when emitted and stale when finally spoken).
            // Ambient lines can wait; the road cannot (`say_curve`).
            //
            // Manual pedals or the speed keeper: the pacenote carries the
            // assist clause in the same breath, never as a second line.
            let text = if servo.is_some() {
                curve_assist_slowing(&message)
            } else {
                message
            };
            say_curve(ctx, text);
        }
        // Open the re-arm window: if Ctrl silences this call before it
        // finishes, it gets one refreshed re-speak (owner worry,
        // 2026-07-20 -- his stop-speech reflex vs a safety cue).
        if let Some(curve) = curve {
            self.critical_curve = Some(curve);
            self.critical_call_age_s = 0.0;
            self.critical_respeak_at = None;
        }
    }

    /// The `else` branch of `_handle_trip_event`: zone entries, checkpoints,
    /// and zone-ahead/traffic warnings.
    ///
    /// These used to interrupt here like a collision would. They are act-soon,
    /// not act-now: they ride ROUTE's short patience (queued, stale means
    /// flush, requeued if cut, never dropped) so each one stops being a chance
    /// to erase a warning mid-word (research doc, R1). They bypass the
    /// one-deep ambient slot exactly as they did when they interrupted;
    /// everything else keeps its spacing.
    fn speak_plain_route_event(
        &mut self,
        ctx: &mut GameContext,
        event: &TripEvent,
        sound: Option<&'static str>,
        message: SpokenMessage,
        category: Option<SpeechCategory>,
    ) {
        let kind = event.kind;
        let priority = self.event_priority(event);
        if !Self::demoted_from_interrupt(event) && self.should_space_ambient_event(event) {
            let mut render = None;
            if kind == TripEventKind::StopAhead {
                if let Some(stop) = event.data.stop.clone() {
                    // The queue's age cap is real seconds; the distance in
                    // this line decays in game miles. Re-render at delivery
                    // so a wait never makes it lie -- "Pilot in 5 miles"
                    // was performed with two left (Brandon, 2026-08-20).
                    //
                    // Rust: the closure gets `&DrivingState`, so the facility
                    // name is resolved here (`name_facility` records the
                    // mention) rather than at delivery.
                    let typed_name = self.trip.name_facility(&stop.name, &stop.spoken_name());
                    let exit_hint = self.trip.exit_hint.clone();
                    render = Some(std::rc::Rc::new(
                        move |drive: &DrivingState, ctx: &GameContext| {
                            let ahead = stop.at_mi - drive.trip.position_mi;
                            if ahead <= 0.0 {
                                return None;
                            }
                            let parts = StopCalloutParts {
                                planned_prefix: drive.trip.planned_prefix(&stop),
                                typed_name: &typed_name,
                                plain_name: &stop.name,
                                exit_label: &stop.exit_label,
                                distance: &drive.trip.ahead_text(ahead),
                                parking_normal: &stop.parking_text(),
                                parking_certainty: &stop.parking,
                                exit_hint: &exit_hint,
                            };
                            // The rung's rendering, as the line had when it
                            // queued: re-rendered as the normal form, quiet
                            // read one stop short and the next one in full
                            // with its key hint (speech mode audit,
                            // 2026-10-03).
                            let text = stop_callout(&parts)
                                .render(drive.terse_speech(ctx))
                                .to_string();
                            (!text.is_empty()).then_some(text)
                        },
                    ) as std::rc::Rc<_>);
                }
            }
            let key = self.ambient_key(event);
            let sound = if kind == TripEventKind::ZoneEnter {
                None
            } else {
                sound
            };
            self.speak_ambient_event(
                ctx,
                message,
                Ambient::new()
                    .sound(sound)
                    .category(category)
                    .key(key)
                    .render(render),
            );
            return;
        }
        if let Some(sound) = sound {
            // A turn earcon belongs to the words that name the turn. The
            // maneuver lead is NAVIGATION_ADVISORY, which the quiet rungs
            // cut, and this played the chime anyway -- so at urgent only a
            // driver heard a turn announced by sound with nothing said about
            // it, on top of the rung's own stand-in note (owner,
            // 2026-09-20). Only the turn cue is held back: the road's other
            // sounds, a passing truck above all, are the road itself and
            // were never a substitute for a sentence (owner, 2026-08-17:
            // "sound is enough").
            // A JUDGED corner -- one with a real side to it -- belongs to the
            // turn flow, which sounds it once with the first words about it
            // (`chime_corner`). Its route cues fire at the lead and at the
            // corner itself, and the owner heard three chimes on one turn
            // out of Houston (2026-09-20). One corner, one chime.
            //
            // A cue with no side to it -- "continue onto", "straight on" --
            // never reaches the turn flow, so it keeps its own sound and only
            // has to be told when the words it belongs to were silenced.
            let judged_turn = event.data.cue.as_ref().is_some_and(is_judged_turn);
            let is_turn_cue = event
                .data
                .cue
                .as_ref()
                .is_some_and(|cue| cue.kind == "local_turn");
            let spoken = ctx.settings.speaks(category) || !ctx.ladder_applies();
            if kind != TripEventKind::ZoneEnter && !judged_turn && (spoken || !is_turn_cue) {
                ctx.audio
                    .play_with(sound, 1.0, route_event_sound_pan(event));
            }
        }
        let mut opts = SayEvent::queued().priority(priority);
        opts.category = category;
        if event.data.limit_change.unwrap_or(false) {
            // A mainline limit cut by the take line is about a road the truck
            // has just left: "Speed limit raised to 75." came back after "You
            // take exit 167" (agent drive, Edwards, 2026-09-24).
            //
            // An arrival line also has to name the limit still in force: a
            // short 45 between two 55s, cut by the next line and handed back,
            // said "reduced to 45" after "raised to 55".
            self.refresh_live_facts();
            let named = event.data.limit_mph;
            opts = opts.valid(move || live::limit_line_holds(named));
        }
        ctx.say_event_with(message, opts);
        // Any spoken route line pushes spaced ambient chatter back, so
        // an informational notice never lands on top of a navigation
        // instruction the player needs to act on.
        self.ambient_event_cooldown_s =
            tuning_for_time_scale(self.trip.time_scale).ambient_spacing_s;
    }

    /// The route's quarter-mile lead for a street turn whose own approach
    /// call has already been spoken: that call named the side, the street,
    /// the distance and the advise speed, so the lead only says the same
    /// thing again a few seconds later (owner, Abilene streets, 2026-09-23:
    /// "announced at least twice before the turn. Necessary?").
    ///
    /// And the route's call AT the turn, when the turn's own call already
    /// said "now": the street chain's briefing ends "Then turn left now onto
    /// North 1st Street", and "Turn left onto North 1st Street" followed it
    /// straight away (agent drive, exit 286A, 2026-09-23).
    fn corner_already_called(&self, event: &TripEvent) -> bool {
        if event.kind != TripEventKind::GpsCue {
            return false;
        }
        let Some(cue) = event.data.cue.as_ref().filter(|cue| is_judged_turn(cue)) else {
            return false;
        };
        if event.data.advance.unwrap_or(false) {
            self.turn_advised.contains(&cue.key)
        } else {
            self.turn_called_now.contains(&cue.key)
        }
    }

    /// `_should_ignore_destination_exit_gps_cue(event)`.
    pub fn should_ignore_destination_exit_gps_cue(
        &mut self,
        ctx: &mut GameContext,
        event: &TripEvent,
    ) -> bool {
        if self.phase != DRIVE_PHASE_DELIVERY || event.kind != TripEventKind::GpsCue {
            return false;
        }
        let Some(cue) = event.data.cue.as_ref() else {
            return false;
        };
        if cue.kind != "interchange" {
            return false;
        }
        let Some(stop) = self.destination_exit_stop(ctx) else {
            return false;
        };
        (cue.at_mi - stop.at_mi).abs() <= 0.15
    }

    /// Drop the heads-up for a zone the delivery will never drive into.
    ///
    /// The facility gate zone covers the last half mile of the route, but a
    /// delivery leaves the highway at the destination exit at least a mile
    /// before that, so its 15 mile per hour limit was announced two miles out
    /// and then never took effect -- the driver slowed for a sign that never
    /// came (playtest transcript, 2026-07-20). Pickup legs and facility
    /// approach chains do drive to the gate, and keep their warning.
    pub fn should_ignore_unreachable_zone_cue(
        &mut self,
        ctx: &mut GameContext,
        event: &TripEvent,
    ) -> bool {
        if self.phase != DRIVE_PHASE_DELIVERY || event.kind != TripEventKind::GpsCue {
            return false;
        }
        let Some(zone) = event.data.zone.as_ref() else {
            return false;
        };
        let start_mi = zone.start_mi;
        self.destination_exit_stop(ctx)
            .is_some_and(|stop| start_mi >= stop.at_mi)
    }

    /// Exit traffic is news only to a driver taking that exit.
    ///
    /// Every route stop grows an exit-traffic pressure a couple of miles
    /// ahead of itself, and each one announced itself in turn -- so a
    /// corridor thick with truck stops narrated the traffic at exit after
    /// exit the driver had no intention of using (owner, 2026-08-15). The
    /// advisory earns its words only for somebody about to move right, so it
    /// speaks for a signaled exit and for one lane keeping is taking on the
    /// driver's behalf, and stays silent for the rest of them.
    ///
    /// The trip marks the pressure announced whether or not it is spoken, so
    /// arming an exit late cannot dump a stale advisory afterwards; signal
    /// before the window arrives and the whole call comes as usual. Nothing
    /// else changes -- the traffic is still there, still crowds the exit
    /// lane, and still explains a missed exit afterwards.
    ///
    /// Merging traffic and construction-taper calls are not gated: they warn
    /// about the road the truck is already on, not about a turn-off it is
    /// free to ignore.
    pub fn should_ignore_unsignalled_exit_pressure(
        &self,
        ctx: &GameContext,
        event: &TripEvent,
    ) -> bool {
        let Some(pressure) = event.data.traffic_pressure.as_ref() else {
            return false;
        };
        if pressure.kind != "exit" {
            return false;
        }
        let Some(stop) = self.exit_stop.as_ref() else {
            return true;
        };
        if !self.exit_intent_ready(ctx, stop) {
            return true;
        }
        !(pressure.start_mi <= stop.at_mi && stop.at_mi <= pressure.end_mi)
    }

    /// A construction-taper merge call: the lane it warns about really is
    /// closing, not routine traffic colour. It used to ride the same one-deep
    /// ambient slot as roadside chatter, where a hazard or the next piece of
    /// colour could erase it before it ever spoke (tester Sarah, US-12 East,
    /// 2026-08-14).
    pub fn is_lane_closure_pressure(event: &TripEvent) -> bool {
        event
            .data
            .traffic_pressure
            .as_ref()
            .is_some_and(|pressure| pressure.kind == "construction_merge")
    }

    /// The act-soon kinds R1 moved out of CRITICAL. As interrupts they never
    /// went near the one-deep ambient slot, and demotion must not start
    /// routing them through it -- a slot overwrite or a hazard would silently
    /// destroy them. ROUTE's queue is their delivery.
    pub fn demoted_from_interrupt(event: &TripEvent) -> bool {
        if matches!(
            event.kind,
            TripEventKind::ZoneEnter | TripEventKind::Checkpoint
        ) {
            return true;
        }
        if event.kind == TripEventKind::GpsCue {
            if event.data.zone.is_some() {
                return true;
            }
            if event
                .data
                .cue
                .as_ref()
                .is_some_and(|cue| cue.kind == "traffic")
            {
                return true;
            }
            if Self::is_lane_closure_pressure(event) {
                return true;
            }
        }
        false
    }

    /// Act NOW or lose something: the hazard call is the only trip event left
    /// in the class. Zone entries, checkpoints, and zone-ahead/traffic
    /// warnings are act-soon -- they ride ROUTE's short patience and its
    /// never-dropped, requeued-if-cut contract instead of purging the channel,
    /// because every interrupt is a chance to erase a warning the player still
    /// needed (speech priority research, R1).
    pub fn is_critical_event(event: &TripEvent) -> bool {
        event.kind == TripEventKind::Hazard
    }

    /// What this announcement is ABOUT, for the driving speech ladder.
    ///
    /// Deliberately separate from [`DrivingState::event_priority`]: urgency
    /// decides how long a line waits, category decides whether the player's
    /// rung speaks it at all.
    ///
    /// `None` means "not the ladder's business" and the gate passes the line
    /// straight through. Two different things read as None and both are
    /// correct. Flavor -- billboards, landmarks, the place and border
    /// callouts -- answers to the chatter switches and the place-callouts
    /// ladder, and the owner set those separately (2026-08-15); a rung must
    /// never be able to silence them. And a kind nobody has classified yet
    /// also reads None, so the failure mode of a new event kind is a line too
    /// many rather than a warning the ladder ate.
    ///
    /// The navigation/status split is where "act-now cues only" lives: the
    /// stop, exit, or turn the player must act on is NAVIGATION; the weather
    /// turning and the road's general state are STATUS and fall silent at the
    /// quietest rung. Between them sits NAVIGATION_ADVISORY -- the lead
    /// announcement, the bend coming, the stop still miles off. Spoken at
    /// quiet, a tone at urgent_only, which is what makes those two rungs
    /// different settings.
    pub fn event_category(event: &TripEvent) -> Option<SpeechCategory> {
        if event.kind == TripEventKind::GpsCue && event.data.limit_change.unwrap_or(false) {
            // "Speed limit raised to 55" is the road's state; S answers it on
            // demand. The other GPS cues -- merge onto this highway, take that
            // exit -- are the turn itself and stay NAVIGATION.
            return Some(SpeechCategory::Status);
        }
        if event.kind == TripEventKind::GpsCue && event.data.advance.unwrap_or(false) {
            // "In a mile, take exit 42" -- the heads-up. The near call that
            // follows at the exit itself is the one you cannot recover from
            // and stays NAVIGATION, spoken at every rung.
            return Some(SpeechCategory::NavigationAdvisory);
        }
        if event.kind == TripEventKind::GpsCue && event.data.npc_vehicle.is_some() {
            // A traffic advisory: "Merging car, 2.2 miles". Awareness of the
            // road around you, which the pass-by and engine sounds already
            // carry, and no action attached at that distance (owner,
            // 2026-08-17: "sound is enough"). The act-now half of traffic is a
            // HAZARD event -- "Change lanes or brake! Merging traffic right
            // ahead" -- which is SAFETY and speaks at every rung. TRAFFIC,
            // not STATUS: quiet speaks status, and a player found quiet still
            // reading out every slow box truck and its speed (2026-10-03).
            return Some(SpeechCategory::Traffic);
        }
        if event.kind == TripEventKind::GpsCue {
            if let Some(pressure) = event.data.traffic_pressure.as_ref() {
                return Some(match pressure.kind.as_str() {
                    // "Merge right early" at a taper is a lane the driver has
                    // to leave, the one pressure with an action in it.
                    "construction_merge" => SpeechCategory::Navigation,
                    // Exit traffic and a merge ahead say which lane to hold:
                    // a heads-up, spoken at quiet and a tone at urgent only.
                    "exit" | "route_merge" => SpeechCategory::NavigationAdvisory,
                    // A traffic pack is the road filling up; cruise slows for
                    // it on its own.
                    _ => SpeechCategory::Traffic,
                });
            }
            if event.data.cb_patrol.is_some() {
                // The CB's word on a trooper miles ahead: worth hearing at
                // quiet, a tone at urgent only. The speed warning is what
                // stops a ticket, and it speaks at every rung.
                return Some(SpeechCategory::NavigationAdvisory);
            }
            if event
                .data
                .cue
                .as_ref()
                .is_some_and(|cue| cue.kind == "toll")
            {
                // The toll is charged whatever the driver does; the heads-up
                // asks nothing of them.
                return Some(SpeechCategory::NavigationAdvisory);
            }
            if let Some(zone) = event.data.zone.as_ref() {
                // A zone a few miles ahead. Only a construction lane closure
                // asks the driver to move; the rest is a lower number coming,
                // which the assists ease for and the speed warning enforces.
                let closure = zone.reason == "construction" && zone.closed_side.is_some();
                return Some(if closure {
                    SpeechCategory::Navigation
                } else {
                    SpeechCategory::NavigationAdvisory
                });
            }
        }
        event_category_for_kind(event.kind)
    }

    /// How long this announcement is willing to wait behind other speech.
    ///
    /// ROUTE is act-soon plus every consequence that must be heard: the stop
    /// the player planned, zone entries and checkpoints, zone-ahead and
    /// traffic warnings, a construction taper's lane-closure merge call, and
    /// money (a charged toll could otherwise age out silently, making normal
    /// mode lossier than terse mode's "what it cost" guarantee -- the
    /// toll-ahead heads-up stays AMBIENT, since losing the preview costs
    /// nothing once the charge is guaranteed). Everything else waits its turn.
    pub fn event_priority(&self, event: &TripEvent) -> EventPriority {
        if Self::is_critical_event(event) {
            return EventPriority::Critical;
        }
        if matches!(
            event.kind,
            TripEventKind::ZoneEnter | TripEventKind::Checkpoint | TripEventKind::TollCharged
        ) {
            return EventPriority::Important;
        }
        if event.kind == TripEventKind::GpsCue {
            if event.data.zone.is_some() {
                return EventPriority::Important;
            }
            // A limit change is what enforcement reads, and each one is said
            // once: the advance "drops to 55" marks 55 as announced, so when
            // the pacer dropped it as stale chatter the boundary stayed silent
            // too and the truck ran a 55 it never heard of (agent drive, exit
            // 286A into Abilene, 2026-09-23).
            if event.data.limit_change.unwrap_or(false) {
                return EventPriority::Important;
            }
            let cue_kind = event
                .data
                .cue
                .as_ref()
                .map(|cue| cue.kind.as_str())
                .unwrap_or("");
            if cue_kind == "traffic" {
                return EventPriority::Important;
            }
            if Self::is_lane_closure_pressure(event) {
                return EventPriority::Important;
            }
            // The direction itself: which way onto the highway, which way
            // through an interchange, which way down a street. Lose one and
            // the driver goes the wrong way, so none of them may age out.
            // The route-start merge -- "Merge onto I-70 West toward
            // Silverthorne; 67 miles", the first instruction of the whole run
            // -- was dropped as stale chatter on the owner's Denver playtest.
            // The ADVANCE half stays ambient: a heads-up that arrives late is
            // worse than one that never comes, which is the lesson the turn
            // approach cue already carries.
            if !event.data.advance.unwrap_or(false)
                && matches!(cue_kind, "onramp" | "maneuver" | "local_turn")
            {
                return EventPriority::Important;
            }
        }
        if event.kind == TripEventKind::StopAhead || event.data.planned.unwrap_or(false) {
            return EventPriority::Important;
        }
        EventPriority::Ambient
    }

    /// `_should_ignore_untaken_destination_facility_event(event)`.
    pub fn should_ignore_untaken_destination_facility_event(&self, event: &TripEvent) -> bool {
        if self.phase != DRIVE_PHASE_DELIVERY || self.destination_exit_taken {
            return false;
        }
        let Some(zone) = event.data.zone.as_ref() else {
            return false;
        };
        matches!(
            zone.reason.as_str(),
            "destination approach" | "facility access road" | "facility gate" | "yard"
        )
    }

    /// `_event_disables_cruise(event)`.
    pub fn event_disables_cruise(&self, ctx: &GameContext, event: &TripEvent) -> bool {
        if self.cruise_mph.is_none() {
            return false;
        }
        if event.kind == TripEventKind::ZoneEnter {
            return true;
        }
        if event.kind != TripEventKind::GpsCue {
            return false;
        }
        let Some(zone) = event.data.zone.as_ref() else {
            return false;
        };
        // An armed speed-control session stays on for the advance warning so
        // cruise can slow for the lower limit, then hands off at zone entry.
        if self.speed_control_armed && ctx.settings.speed_keeper {
            return false;
        }
        matches!(zone.reason.as_str(), "construction" | "heavy traffic")
    }

    /// `_cancel_cruise_for_restricted_area(event)`.
    pub fn cancel_cruise_for_restricted_area(
        &mut self,
        ctx: &mut GameContext,
        event: &TripEvent,
        message: SpokenMessage,
        category: Option<SpeechCategory>,
    ) {
        let zone = event.data.zone.clone();
        if let (true, true, Some(zone)) =
            (self.speed_control_armed, ctx.settings.speed_keeper, zone)
        {
            self.cancel_cruise(ctx, true);
            self.engage_keeper(
                ctx,
                zone.limit_mph,
                &zone.reason,
                Some(zone.limit_mph),
                false,
            );
            ctx.play_event_cue(category, "ui/notify", 1.0, 0.0);
            let keeper = ctx.settings.speed_text(self.keeper_mph.unwrap_or(0.0));
            let message = message.plus(&format!("Speed keeper holding {keeper}."));
            let mut opts = SayEvent::queued().priority(EventPriority::Important);
            opts.category = category;
            ctx.say_event_with(message, opts);
            return;
        }
        self.cancel_cruise(ctx, false);
        ctx.audio.play("ui/notify");
        // A restricted area (construction, heavy traffic) is act-soon: ROUTE
        // priority gives chatter under a second before going in front of it,
        // without an interrupt that could cut a real warning mid-word.
        //
        // The pedals are the driver's again, so it is SAFETY whatever the
        // event was, the same class as "Stopped traffic ahead; adaptive
        // cruise canceled". As a zone entry (STATUS) urgent only dropped it
        // whole, and the cruise let go without a word; quiet now says it
        // short.
        let message = message.plus("Cruise off.");
        let mut opts = SayEvent::queued().priority(EventPriority::Important);
        opts.category = Some(SpeechCategory::Safety);
        ctx.say_event_with(message, opts);
    }

    /// What an inspector would write up on the trailer, if anything.
    ///
    /// Only on the loaded run. The pickup plan names the box waiting in the
    /// shipper's yard, and on the deadhead there the truck is pulling its own
    /// empty (or nothing): reading the plan then wrote a driver up twice for a
    /// lamp on a trailer still parked a state away (player report,
    /// 2026-10-17).
    pub fn hooked_trailer_defect(&self, ctx: &GameContext) -> Option<String> {
        if ctx.profile.is_none()
            || self.trailer_refused
            || self.trailer_repaired
            || self.phase != DRIVE_PHASE_DELIVERY
            || !self.trip.truck.trailer_attached
        {
            return None;
        }
        let plan = pickup_plan(&self.job, profile_of(ctx));
        plan.trailer
            .as_ref()
            .and_then(|trailer| trailer.defect())
            .map(str::to_string)
    }

    /// The trailer defect an officer can still cite: the hooked one, unless
    /// an earlier stop this drive already wrote it up.
    pub fn citable_trailer_defect(&self, ctx: &GameContext) -> Option<String> {
        self.hooked_trailer_defect(ctx)
            .filter(|defect| !self.written_up.contains(defect))
    }

    /// Route-backed enforcement with stable evidence and no duplicate fines.
    pub fn handle_inspection(&mut self, ctx: &mut GameContext, event: &TripEvent) {
        let event_key = event.data.key.clone().unwrap_or_else(|| {
            format!(
                "{}:{}:{}",
                event.text(),
                ff_core::pyfmt::fmt_f(self.trip.position_mi, 1),
                self.hos_fine_count
            )
        });
        if self.enforcement_events.contains(&event_key) {
            return;
        }
        self.enforcement_events.insert(event_key);
        match event.data.context.as_deref() {
            // CB heads-up, not a stop: Roadcheck week is on.
            Some("roadcheck_notice") => {
                let mut opts = SayEvent::queued().priority(EventPriority::Important);
                opts.category = Self::event_category(event);
                ctx.say_event_with(event.text(), opts);
                return;
            }
            // A legal driver's routine Level 3: the stop is the inspection.
            Some("routine_inspection") => {
                self.begin_routine_inspection(ctx);
                return;
            }
            _ => {}
        }
        let mode = ctx.settings.hos_mode.clone();
        let fine = hos::hos_fine(&mode, self.hos_fine_count);
        self.hos_fine_count += 1;
        let mut evidence: Vec<String> = event.data.evidence.clone().unwrap_or_default();
        // A trailer hooked out of a drop yard came with whatever the last
        // driver left on it, and an inspector finds what a walk-around would
        // have. This is drop-and-hook's real cost, arriving at the worst moment.
        if let Some(defect) = self.citable_trailer_defect(ctx) {
            if !defect.is_empty() {
                self.written_up.insert(defect.clone());
                evidence.push(defect);
            }
        }
        if evidence.is_empty() {
            evidence = vec!["HOS/ELD violation".to_string()];
        }
        let evidence_text = evidence.join(", ");
        ctx.audio.play("ui/error");
        ctx.controller.rumble.alert();
        let serious_hos = !hos::HOS_NON_ENFORCED_MODES.contains(&mode.as_str())
            && hos_of(ctx).in_violation(&mode);
        if serious_hos {
            // A serious violation is a REAL roadside stop: lights, signal,
            // brake to the shoulder, and the out-of-service order passes
            // while the truck is actually stopped. A missed 30-minute break
            // is thirty minutes on the shoulder, not a 10-hour reset. Drive
            // or duty still takes the ten hours. Fine and reputation are
            // applied by the stop itself, not here.
            let break_only = hos_of(ctx).is_break_only_violation(&mode);
            let oos_phrase = if break_only {
                "thirty minutes"
            } else {
                "ten hours"
            };
            let return_message = if break_only {
                "Back on the highway after the 30-minute break. Keep the logbook clean."
            } else {
                "Back on the highway with a reset clock. Keep the logbook clean."
            };
            let summary = format!(
                "{} Evidence: {evidence_text}. The officer writes the order: out of service, \
                 {oos_phrase}, right here.",
                event.text()
            );
            let lights = format!(
                "Lights and siren behind you for a log check. Signal with {} and stop on the \
                 shoulder.",
                ctx.control_hint("take_exit")
            );
            self.begin_enforcement_pull_over(
                ctx,
                "hos_out_of_service",
                "Log check",
                &summary,
                fine,
                hos::HOS_REPUTATION_HIT,
                return_message,
                &lights,
            );
            // A failed log check is not a passed inspection: no badge, and
            // no clean-inspection credit on the safety record (2026-09-28).
            return;
        }
        {
            let profile = profile_mut_of(ctx);
            profile.spend(fine); // can go negative; never a game over
            profile.career.reputation =
                0.0f64.max(profile.career.reputation - hos::HOS_REPUTATION_HIT);
        }
        let message = format!(
            "{} Evidence: {evidence_text}. Fined {} dollars, and your reputation took a hit.",
            event.text(),
            ff_core::pyfmt::fmt_grouped(fine, 0)
        );
        // A fine is money, not an act-now warning: ROUTE's never-dropped
        // queue instead of an interrupt that could erase one.
        let mut opts = SayEvent::queued().priority(EventPriority::Important);
        opts.category = Self::event_category(event);
        ctx.say_event_with(message, opts);
    }

    /// `_place_out_of_service()`. A full 10-hour reset: fatigue, drive, or
    /// duty. A missed 30-minute break uses [`Self::place_out_of_service_minutes`].
    pub fn place_out_of_service(&mut self, ctx: &mut GameContext) {
        self.place_out_of_service_minutes(ctx, OUT_OF_SERVICE_MIN);
    }

    /// Park for `minutes`. A full 10-hour order resets the clock and fatigue;
    /// a 30-minute break order only takes the missed break.
    pub fn place_out_of_service_minutes(&mut self, ctx: &mut GameContext, minutes: f64) {
        // Written now, served in full below before this returns: the truck is
        // never under an order it could drive on, so driving under one (49 CFR
        // 383.51 Table 4) has no path in the game.
        let written_h = crate::states::driving_rest_states::record_hours(ctx, self);
        advance_rest_clock(self, ctx, minutes, None, "");
        if minutes >= hos::SLEEP_MIN {
            hos_mut_of(ctx).sleep();
            let profile = profile_mut_of(ctx);
            profile.fatigue = hos::rest_sleep(profile.fatigue);
        } else {
            hos_mut_of(ctx).take_break(minutes);
            let profile = profile_mut_of(ctx);
            profile.fatigue = hos::rest_break(profile.fatigue);
        }
        self.out_of_service_count += 1;
        // The career count the safety record scores (models/safety_record,
        // OUT_OF_SERVICE_WEIGHT). The trip tally above was the only one ever
        // kept, in Python and in the port, so no order ever reached the
        // record and the scale kept waving the driver through.
        let profile = profile_mut_of(ctx);
        profile.out_of_service_events += 1;
        profile.driving_record.out_of_service_times.push(written_h);
        let snapshot = self.snapshot(ctx);
        profile_mut_of(ctx).active_trip = Some(snapshot);
        ctx.save_profile();
    }

    /// `_set_status(text)`.
    pub fn set_status(&mut self, text: impl Into<String>) {
        self.status_text = text.into();
    }

    /// `ctx.say(text)` with the driving layer's plain default, kept as one
    /// call so the port reads like the Python it came from.
    pub(crate) fn say_plain(&self, ctx: &mut GameContext, text: impl Into<String>) {
        ctx.say_with(text.into(), Say::new());
    }
}
