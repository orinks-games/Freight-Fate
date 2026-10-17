//! The watch itself: the held siren, the road sample a post reads, and the
//! per-frame draw that turns an observation into a pull-over.

use ff_core::models::enforcement::{
    chain_law_fine, FOLLOWING_TOO_CLOSE_FINE, LANE_MISUSE_FINE, LIGHTS_FINE, UNSAFE_DAMAGE_FINE,
};
use ff_core::pyfmt::{py_str_float, round_py_n};
use ff_core::pyrandom::PyRandom;
use ff_core::sim::enforcement_observe::{
    observe, Observation, RoadSample, COVER_RADIUS_MI, COVER_SPEED_TOLERANCE_MPH, TAILGATE_GAP_S,
    WHAT_CHAINS, WHAT_DAMAGE, WHAT_EQUIPMENT, WHAT_FOLLOWING, WHAT_LIGHTS, WHAT_SPEEDING,
};
use ff_core::sim::enforcement_posts::{
    post_seed, EnforcementPost, METHOD_PACING, PACING_WINDOW_MI,
};
use ff_core::sim::roadside_inspection::{
    DAMAGE_FINDING, TIRES_WORN_FINDING, TIRE_OUT_OF_SERVICE_PCT,
};

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use super::{DEFERRED_STOP_MAX_MI, POST_MARKER_LEAD_MI};

impl DrivingState {
    // -- the siren -----------------------------------------------------------

    /// Keep the cruiser audible for as long as it is behind you.
    ///
    /// The old build played one mono, centred, fixed-level wail and never
    /// repeated it, and the whole pull-over update contained no audio at all.
    /// Miss that one shot and there was no ongoing evidence of a police car
    /// anywhere in the encounter.
    pub fn hold_stop_siren(&mut self, ctx: &mut GameContext) {
        // Pan is confirmation of side and nothing more -- "behind you" is in
        // the spoken instruction, because stereo cannot carry front from back
        // and plenty of players run a single earbud. It closes toward centre
        // as the cruiser comes up, so the pan agrees with the level rise
        // rather than fighting it.
        let closing = 1.0f64.min(self.siren.elapsed_s / SIREN_RISE_S.max(1e-6));
        self.siren.hold(ctx.audio.as_mut(), -0.5 * (1.0 - closing));
    }

    /// Every path out of a stop comes through here.
    pub fn end_stop_audio(&mut self, ctx: &mut GameContext) {
        self.siren.stop(ctx.audio.as_mut());
        self.restore_radio_after_stop(ctx);
    }

    // -- the sample ----------------------------------------------------------

    /// Everything a post could notice about the truck, read defensively.
    pub fn road_sample(&mut self, post: &EnforcementPost) -> RoadSample {
        let position = self.trip.position_mi;
        let (limit, _) = self.trip.speed_limit_at(position);
        // `float(getattr(effects, "visibility_mi", 10.0) or 10.0)`: a zero
        // reading is falsy in Python and falls back to the clear-air default.
        let visibility = self.trip.weather.effects().visibility_mi;
        let visibility = if visibility == 0.0 { 10.0 } else { visibility };
        let gap_s = self.trip.traffic_context().map(|c| c.gap_seconds());
        // The law binds on the posted grade and nowhere else: the same test
        // the entry citation makes (`update_chain_law`). Snow anywhere used
        // to let a scale or a patrol in Chicago write "running the chain
        // control without chains" (seasonal audit, 2026-10-01).
        // Colorado Level 1 leaves a tandem-drive truck unchained, and
        // Wyoming Level 1 takes snow tires (`rig_compliant_at`).
        // A truck already cited in this chain law area carries the ticket:
        // a patrol further up the same control does not write it again
        // (the checkpoint's own rule, `update_chain_law`).
        let chain_level = match self.trip.chain_law_area_at(position) {
            Some(area)
                if !self.trip.rig_compliant_at(position)
                    && !self
                        .chain_law_cited
                        .contains(&(area as i64, self.trip.chain_law_level())) =>
            {
                self.trip.chain_law_level()
            }
            _ => 0,
        };
        let night = is_night(self.trip.local_hour());
        let pack_neighbours = self.trip.traffic_manager.pack_neighbours(
            position,
            self.trip.truck.speed_mph(),
            COVER_RADIUS_MI,
            COVER_SPEED_TOLERANCE_MPH,
        );
        let crest_between = self.crest_between(position, post.at_mi);
        RoadSample {
            position_mi: position,
            speed_mph: self.trip.truck.speed_mph(),
            limit_mph: limit,
            // A parallel change is reworking damage into bands; read it
            // defensively so neither side can break the other.
            // Damage already written up this drive is on the paperwork.
            damage_pct: if self.written_up.contains(WHAT_DAMAGE) {
                0.0
            } else {
                self.trip.truck.damage_pct
            },
            visibility_mi: visibility,
            night,
            // The truck carries no running-lights switch, so the Python
            // `getattr(truck, "lights_on", True)` always answered True.
            lights_on: true,
            chains_required: chain_level > 0,
            chains_on: self.trip.truck.chains_on || chain_level == 0,
            following_gap_s: gap_s,
            closed_up_mi: self.closed_up_mi,
            // Same story: nothing ever set `_left_lane_restricted` on the
            // drive, so the defensive read was always False.
            left_lane_restricted: false,
            in_left_lane: self.lane.lane > 0,
            pack_neighbours,
            crest_between,
            paced_mi: self.paced_mi(post),
            over_limit_mi: self.over_limit_mi,
            // Worn tires already written up stay unremarked until they reach
            // the line that parks the truck.
            tire_wear_pct: if self.written_up.contains(TIRES_WORN_FINDING)
                && self.trip.truck.tire_wear_pct < TIRE_OUT_OF_SERVICE_PCT
            {
                0.0
            } else {
                self.trip.truck.tire_wear_pct
            },
            trailer_defect: self.visible_trailer_defect.0.clone(),
        }
    }

    /// Keep the trailer defect a passing trooper could see current: the
    /// pickup plan is read again every half mile, not every frame, and only
    /// the visible items count -- a brake out of adjustment is under the
    /// trailer, where the scale finds it and a unit driving past does not.
    pub fn refresh_visible_trailer_defect(&mut self, ctx: &GameContext) {
        let position = self.trip.position_mi;
        if (position - self.visible_trailer_defect.1).abs() < 0.5
            && !self.written_up.contains(&self.visible_trailer_defect.0)
        {
            return;
        }
        let defect = self
            .citable_trailer_defect(ctx)
            .filter(|defect| !defect.contains("brake"))
            .unwrap_or_default();
        self.visible_trailer_defect = (defect, position);
    }

    /// Whether the road hides the post from an optical method.
    ///
    /// A crest is a grade sign change across the intervening stretch -- the
    /// road goes up and then comes down between you and the officer, so
    /// neither of you can see the other. A hard bend does the same thing, and
    /// the curve bake already knows where those are.
    pub fn crest_between(&self, position_mi: f64, post_mi: f64) -> bool {
        let (low, high) = if position_mi <= post_mi {
            (position_mi, post_mi)
        } else {
            (post_mi, position_mi)
        };
        if high - low < 0.05 {
            return false;
        }
        let grades: Vec<f64> = [0.0, 0.25, 0.5, 0.75, 1.0]
            .iter()
            .map(|f| self.trip.grade_at(low + (high - low) * f))
            .collect();
        let max = grades.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        let min = grades.iter().copied().fold(f64::INFINITY, f64::min);
        // The bend clause reads `getattr(curve, "radius_m", 1e9)`, and a baked
        // RouteCurve carries `min_radius_ft` and no `radius_m` at all -- so
        // the fallback wins on every curve and `abs(1e9) < 400.0` is never
        // true. Reproduced as the no-op it is rather than silently promoted to
        // a live rule that would start blocking lidar on every bend.
        max > 0.015 && min < -0.015
    }

    // -- the watch -----------------------------------------------------------

    /// One frame of the enforcement layer: cues, sampling, and the draw.
    ///
    /// **The pacing mismatch, and how it is solved.** Speeding used to be
    /// judged on a six REAL second hold, but the old patrol window could last
    /// 7.6 real seconds at standard pacing and 3.8 at the fastest -- so at the
    /// fastest pacing a speeder could be structurally unable to be caught
    /// inside a patrol at all. Decompressing the clock inside every post's
    /// reach was the other option and was rejected: with a post every
    /// thirty-odd miles it would have put a dozen miles of a five-hundred mile
    /// run onto the real clock, which is a different game.
    ///
    /// Instead, observation is DISTANCE-quantised. A post reads a speed the
    /// way a radar does -- over a stretch of road (`OBSERVE_HOLD_MI`, about
    /// four hundred feet) -- and a stretch of road is the same stretch at
    /// every pacing, at every frame rate, and after a reload. The real-time
    /// hold is gone entirely along with the silent at-delivery charge it
    /// served; there is nothing left for it to disagree with.
    pub fn update_enforcement_watch(&mut self, ctx: &mut GameContext, dt: f64) {
        self.service_pending_sounds(ctx, dt);
        self.service_radio_cue_duck(ctx, dt);
        self.siren.service(ctx.audio.as_mut(), dt);
        let previous_mi = self.enforcement_prev_mi;
        let position = self.trip.position_mi;
        self.enforcement_prev_mi = position;
        let moved = (position - previous_mi).max(0.0);
        let (limit, _) = self.trip.speed_limit_at(position);
        if self.trip.truck.speed_mph() <= limit + OBSERVE_LEEWAY_MPH {
            self.over_limit_mi = 0.0;
        } else if self.trip.in_unannounced_posting() {
            // A posting too short to be spoken at this pace: nobody told the
            // driver, so nothing accrues against it.
            self.over_limit_mi = 0.0;
        } else if self.limit_drop_grace_s > 0.0 {
            // A limit that just dropped under a loaded truck: the driver is
            // braking and has not disregarded anything yet. Nothing accrues,
            // so no post can read a speed out of the transition itself.
            self.over_limit_mi = 0.0;
        } else if self.speed_control_engaged()
            && self.trip.truck.brake > 0.0
            && self.trip.truck.throttle <= 0.05
        {
            // An automatic speed control is already braking the truck down.
            // Nothing about that is disregard either -- and the rule has to
            // cover every assist that brakes, not just the adaptive-cruise
            // limit cap: the destination-exit ease used to accrue over-limit
            // distance while the assist was doing exactly what it was asked
            // to, which would have ticketed the most cautious drivers in the
            // game for using the feature.
            self.over_limit_mi = 0.0;
        } else {
            self.over_limit_mi += moved;
        }
        self.accrue_following_gap(moved);
        self.track_pacing(moved);
        self.update_scale_bed(ctx);
        if self.ramp_mi.is_none() {
            self.update_marked_unit_passes(ctx, previous_mi);
            self.update_tableaus(ctx, previous_mi);
        }
        let mut id = std::mem::take(&mut self.post_id_scratch);
        let mut audible: Vec<String> = Vec::new();
        for post in &self.trip.posts {
            if !(post.staffed && position >= post.watch_start_mi() - POST_MARKER_LEAD_MI) {
                continue;
            }
            post.write_id(&mut id);
            if !self.marked_post_ids.contains(id.as_str()) {
                audible.push(id.clone());
            }
        }
        self.post_id_scratch = id;
        for post_id in audible {
            self.mark_post_audible(ctx, &post_id);
        }
        if self.enforcement_bypassed(ctx) {
            return;
        }
        self.refresh_visible_trailer_defect(ctx);
        if self.enforcement_busy() {
            // Defer, never drop. The look is TAKEN here and held: the officer
            // saw what they saw, and only the lights wait for the cab to be
            // quiet. Recording post ids instead threw the look away, because
            // by the time a hazard window closes the truck is miles past a
            // one-mile radar reach and nothing is watching it any more.
            self.hold_observation();
            return;
        }
        if let Some(held) = self.take_held_observation() {
            self.begin_observed_stop(ctx, &held);
            return;
        }
        self.run_observations(ctx);
    }

    /// Whether cruise or the speed keeper currently owns the throttle.
    pub fn speed_control_engaged(&self) -> bool {
        self.cruise_mph.is_some() || self.speed_control_armed
    }

    /// Whether the truck's speed is the assist's choice, not the driver's.
    ///
    /// This used to ask whether the assist was actively BRAKING, which
    /// covered adaptive cruise recovering a gap and nothing else. The speed
    /// KEEPER does not follow traffic at all -- `driving_speed_control.rs` has
    /// no notion of a lead vehicle; it holds the posted number -- so in a work
    /// zone it will sit at the sign's 55 while the line ahead bunches up,
    /// closing the gap with the throttle open and never braking. The old test
    /// read that as the driver's disregard and fined them for it.
    ///
    /// Darren, twice: 1,200 dollars on I-75 (2026-08-18), which is what the
    /// carve-out was written for, and 2,400 in an I-94 work zone (2026-08-24)
    /// with the keeper holding 55 -- doubled, because it was a construction
    /// zone. The rule the first fix wrote down is the right one and was drawn
    /// too narrowly: "the driver cannot even choose the gap... ticketing them
    /// for it fined them for using the feature."
    ///
    /// So the question is who has the pedal. `cruise_applied` is what the
    /// assist asked for; a driver pressing past that is closing the gap
    /// themselves and owns it.
    pub fn assist_owns_the_pedal(&self) -> bool {
        if !self.speed_control_engaged() {
            return false;
        }
        self.trip.truck.throttle <= 0.05_f64.max(self.cruise_applied + 0.02)
    }

    /// Road covered while genuinely closed up on the vehicle ahead.
    ///
    /// The mirror of the over-limit accumulator above, and it exists for the
    /// same reason: a post should read a following distance, not a frame. A
    /// gap dips under `TAILGATE_GAP_S` whenever the lead brakes harder than
    /// the truck comfortably can, which at a work-zone taper is every time --
    /// and before this, that single dip was a citation.
    ///
    /// The assist carve-out is the same one over-limit already has, and it
    /// matters more here: adaptive cruise targets `ACC_BASE_GAP_SECONDS`
    /// (three seconds, well clear of the 1.2 that draws a ticket) but can only
    /// close the gap back up at its own comfort deceleration, so while it is
    /// braking, the gap it is recovering is its doing and not the driver's.
    /// The driver cannot even choose the gap -- a selectable following
    /// distance is still an open roadmap item -- so ticketing them for it
    /// fined them for using the feature (tester Darren, I-75, 2026-08-18:
    /// 1,200 dollars for a work-zone taper).
    pub fn accrue_following_gap(&mut self, moved: f64) {
        let gap_s = self.trip.traffic_context().map(|c| c.gap_seconds());
        let Some(gap_s) = gap_s else {
            self.closed_up_mi = 0.0;
            return;
        };
        if !(0.0 < gap_s && gap_s < TAILGATE_GAP_S) {
            self.closed_up_mi = 0.0;
            return;
        }
        if self.assist_owns_the_pedal() {
            // An assist owns the speed, so the gap is its doing. Not
            // disregard.
            self.closed_up_mi = 0.0;
            return;
        }
        self.closed_up_mi += moved;
    }

    /// How much road each roving unit has held station behind the truck over.
    ///
    /// Road, not real seconds. The old real-time version could not be
    /// satisfied at any compression the game offers -- the 1-mile window past
    /// a post is 5.5 real seconds at 65 mph and 10x, against a 20-second gate
    /// -- so a roving patrol never once clocked anybody on a highway
    /// (measured 2026-08-16: 315 looks, zero catches over 2,000 miles).
    pub fn track_pacing(&mut self, moved: f64) {
        let position = self.trip.position_mi;
        let mut id = std::mem::take(&mut self.post_id_scratch);
        for post in &self.trip.posts {
            if !(post.method == METHOD_PACING && post.staffed) {
                continue;
            }
            let behind = position - post.at_mi;
            if 0.0 < behind && behind <= PACING_WINDOW_MI {
                post.write_id(&mut id);
                match self.pacing_mi.get_mut(id.as_str()) {
                    Some(paced) => *paced += moved,
                    None => {
                        self.pacing_mi.insert(id.clone(), 0.0 + moved);
                    }
                }
            } else if behind > PACING_WINDOW_MI && !self.pacing_mi.is_empty() {
                post.write_id(&mut id);
                self.pacing_mi.remove(id.as_str());
            }
        }
        self.post_id_scratch = id;
    }

    /// Road `post` has paced the truck over, from [`Self::track_pacing`].
    pub fn paced_mi(&mut self, post: &EnforcementPost) -> f64 {
        if self.pacing_mi.is_empty() {
            return 0.0;
        }
        post.write_id(&mut self.post_id_scratch);
        self.pacing_mi
            .get(self.post_id_scratch.as_str())
            .copied()
            .unwrap_or(0.0)
    }

    /// Take this mile's look and act on it.
    pub fn run_observations(&mut self, ctx: &mut GameContext) {
        if let Some(found) = self.observed_now() {
            self.begin_observed_stop(ctx, &found);
        }
    }

    /// Take the look the busy cab cannot act on yet, and keep it.
    pub fn hold_observation(&mut self) {
        if self.held_observation.is_some() {
            return; // one held look at a time; the first officer has the claim
        }
        let Some(found) = self.observed_now() else {
            return;
        };
        self.deferred_post_ids.insert(found.post.id());
        self.held_observation = Some((found, self.trip.position_mi));
    }

    /// The held look, if the officer could still plausibly be behind you.
    pub fn take_held_observation(&mut self) -> Option<Observation> {
        let (found, seen_mi) = self.held_observation.take()?;
        self.deferred_post_ids.remove(&found.post.id());
        if self.trip.position_mi - seen_mi > DEFERRED_STOP_MAX_MI {
            return None;
        }
        Some(found)
    }

    /// Ask every post watching this mile what it sees, best first.
    ///
    /// Returns the observation that survived its seeded roll, or `None`.
    pub fn observed_now(&mut self) -> Option<Observation> {
        let position = self.trip.position_mi;
        let watching: Vec<EnforcementPost> = self
            .trip
            .posts_watching(position)
            .into_iter()
            .cloned()
            .collect();
        if watching.is_empty() {
            return None;
        }
        let mut best: Option<Observation> = None;
        for post in &watching {
            let sample = self.road_sample(post);
            let found = observe(post, &sample);
            // The look, for a tester's log: which post, what it could read,
            // and what it made of it. Never spoken.
            log::debug!(
                target: "freight_fate::enforcement",
                "look: {} {} at {:.2} (truck {:.2}) announced={} declined={} tires={:.0} trailer={:?} -> {}",
                post.kind,
                post.method,
                post.at_mi,
                sample.position_mi,
                post.announced,
                post.declined,
                sample.tire_wear_pct,
                sample.trailer_defect,
                found
                    .as_ref()
                    .map(|f| format!("{} {:.2}", f.what, f.confidence))
                    .unwrap_or_else(|| "nothing".to_string())
            );
            if let Some(found) = found {
                if best
                    .as_ref()
                    .is_none_or(|b| found.confidence > b.confidence)
                {
                    best = Some(found);
                }
            }
        }
        let best = best?;
        let post_id = best.post.id();
        // The named, seeded, POSITION-quantised draw. Never time-quantised:
        // identical driving through identical road has to produce an identical
        // outcome whatever the frame rate, and a reload must not re-roll
        // whether a trooper was looking at you.
        // `f"{what}:{round(position, 1)}"` -- a Python float renders through
        // `str()`, so an integral mile is "10.0" and never "10". The seed
        // string is the draw, so the rendering has to match byte for byte.
        let violation_key = format!("{}:{}", best.what, py_str_float(round_py_n(position, 1)));
        let roll = PyRandom::new_from_str(&post_seed(
            Some(self.trip_seed),
            &post_id,
            &format!("observe:{violation_key}"),
        ))
        .random();
        log::info!(
            target: "freight_fate::enforcement",
            "{} at {:.1}: {} ({}) confidence {:.2}, roll {:.2}",
            best.post.kind,
            position,
            best.what,
            best.detail,
            best.confidence,
            roll
        );
        if roll >= best.confidence {
            // Noticed and let go. A post does not re-decide: this is what
            // makes "five over near a post is ignored" a state rather than a
            // rare piece of bad luck the player can never learn from.
            if let Some(post) = self.trip.post_mut(&post_id) {
                post.declined = true;
            }
            return None;
        }
        Some(best)
    }

    /// Turn a confirmed observation into the pull-over that already exists.
    pub fn begin_observed_stop(&mut self, ctx: &mut GameContext, observation: &Observation) {
        let post_id = observation.post.id();
        let reason = observation.post.reason();
        self.cut_radio_for_stop(ctx);
        self.over_limit_mi = 0.0;
        self.closed_up_mi = 0.0;
        if let Some(post) = self.trip.post_mut(&post_id) {
            post.declined = true;
        }
        if observation.what == WHAT_SPEEDING {
            let position = self.trip.position_mi;
            let (limit, _) = self.trip.speed_limit_at(position);
            self.begin_pull_over(ctx, limit);
            return;
        }
        if observation.what == WHAT_CHAINS {
            if let Some(area) = self.trip.chain_law_area_at(self.trip.position_mi) {
                self.chain_law_cited
                    .insert((area as i64, self.trip.chain_law_level()));
            }
        }
        if observation.what == WHAT_DAMAGE {
            self.written_up.insert(WHAT_DAMAGE.to_string());
            self.written_up.insert(DAMAGE_FINDING.to_string());
        }
        if observation.what == WHAT_EQUIPMENT {
            // The rolling look found something: the stop is a Level 2
            // walk-around, and the report prices it, not this stop.
            let summary = format!(
                "A trooper on this {reason} looked the truck over as they passed and saw {}. \
                 They are pulling you in for a walk-around inspection.",
                observation.detail
            );
            let lights_message = format!(
                "Lights and siren behind you. A trooper on this {reason} saw {} and wants a look \
                 at the truck. Signal with {} and stop on the shoulder.",
                observation.detail,
                ctx.control_hint("take_exit")
            );
            self.begin_enforcement_pull_over(
                ctx,
                "roadside_walkaround",
                "Roadside inspection",
                &summary,
                0.0,
                0.0,
                "Back on the highway.",
                &lights_message,
            );
            return;
        }
        let (summary, fine, return_message) = self.observed_stop_terms(observation);
        let lights_message = format!(
            "Lights and siren behind you. A trooper on this {reason} saw {}. Signal with {} and \
             stop on the shoulder.",
            observation.what,
            ctx.control_hint("take_exit")
        );
        self.begin_enforcement_pull_over(
            ctx,
            "observed",
            "Roadside pull-over",
            &summary,
            fine,
            hos::HOS_REPUTATION_HIT,
            &return_message,
            &lights_message,
        );
    }

    pub fn observed_stop_terms(&self, observation: &Observation) -> (String, f64, String) {
        let reason = observation.post.reason();
        let what = observation.what.as_str();
        if what == WHAT_DAMAGE {
            return (
                format!(
                    "A trooper on this {reason} saw visible truck damage at {:.0} percent and \
                     ordered a roadside safety inspection.",
                    self.trip.truck.damage_pct
                ),
                UNSAFE_DAMAGE_FINE,
                "Back on the highway. Repair the truck at the next safe stop.".to_string(),
            );
        }
        if what == WHAT_CHAINS {
            return (
                format!(
                    "A trooper on this {reason} saw you running the chain control without chains \
                     on the drives."
                ),
                chain_law_fine(self.trip.state_code_at(self.trip.position_mi).as_deref()),
                "Back on the highway. Chain up before the next control.".to_string(),
            );
        }
        if what == WHAT_FOLLOWING {
            return (
                format!(
                    "A trooper on this {reason} watched you close right up on the vehicle ahead."
                ),
                FOLLOWING_TOO_CLOSE_FINE,
                "Back on the highway. Leave yourself a gap.".to_string(),
            );
        }
        if what == WHAT_LIGHTS {
            return (
                format!("A trooper on this {reason} saw you running dark."),
                LIGHTS_FINE,
                "Back on the highway. Keep your lights on after dark.".to_string(),
            );
        }
        (
            format!("A trooper on this {reason} pulled you over for {what}."),
            LANE_MISUSE_FINE,
            "Back on the highway. Keep right except to pass.".to_string(),
        )
    }
}
