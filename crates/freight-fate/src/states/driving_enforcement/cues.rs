//! The enforcement earcons: the signature marker, the marked-unit pass, the
//! tableau's siren and shoulder pass, and the weigh-station approach bed.

use ff_core::pyrandom::PyRandom;
use ff_core::sim::enforcement_posts::{
    post_seed, EnforcementPost, KIND_FIXED_SCALE, KIND_SCALE_APRON, TABLEAU_SIREN_LEAD_MI,
};
use ff_core::speech_pacing::SpeechCategory;
use ff_core::speech_text::SpokenMessage;

use crate::app::{GameContext, SayEvent};
use crate::audio::CH_SCALE;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use super::{
    PASS_BASE_VOLUME, PASS_PAN, PASS_TRIGGER_MI, SCALE_BED_CLOSED_MAX_VOLUME, SCALE_BED_FADE_MS,
    SCALE_BED_MIN_VOLUME, SCALE_BED_OPEN_MAX_VOLUME, SCALE_BED_START_MI, TABLEAU_ANOTHER_LINE,
    TABLEAU_INTRO_LINE, TABLEAU_INTRO_REASONS, TABLEAU_PASS_VOLUME, TABLEAU_REPEAT_WINDOW_MI,
    TABLEAU_SHOULDER_PAN, TABLEAU_SIREN_VOLUME,
};

impl DrivingState {
    // -- cues ----------------------------------------------------------------

    /// `_play_enforcement_marker(*, volume=0.8, pan=0.0)`.
    pub fn play_enforcement_marker(&mut self, ctx: &mut GameContext, volume: f64, pan: f64) {
        self.duck_radio_for_cue(ctx);
        ctx.audio.play_with(SIGNATURE_KEY, volume, pan);
    }

    /// The guarantee: a staffed post makes a sound before it can see you.
    ///
    /// Fires at every presence level, because this cue is not ambience -- it
    /// is the only reason the post is allowed to cost the player anything.
    pub fn mark_post_audible(&mut self, ctx: &mut GameContext, post_id: &str) {
        if self.marked_post_ids.contains(post_id) {
            return;
        }
        self.marked_post_ids.insert(post_id.to_string());
        // The flag and the sound are set together, on purpose. `announced` is
        // what `observe` checks before it will look at the driver at all, so
        // it must mean "this post has made a noise", not "the trip meant to
        // make one". A post that could set the flag without playing anything
        // would be a post that tickets a player it never spoke to.
        if let Some(post) = self.trip.post_mut(post_id) {
            post.announced = true;
        }
        self.play_enforcement_marker(ctx, 0.75, 0.0);
    }

    /// The oncoming pass: marker first, vehicle 200 ms behind it.
    ///
    /// Both backends apply pan once at trigger, so a real Doppler sweep is not
    /// buildable from code -- the sweep has to be baked into the asset, and the
    /// pan here is a static confirmation of side, never the carrier of the
    /// meaning. The two-element shape is what makes it survive: the civilian
    /// and trooper pass clips differ only by a chirp buried inside the whoosh,
    /// which is gone under engine, road, weather and radio, but a marker
    /// arriving first at its own level is not.
    pub fn play_marked_unit_pass(&mut self, ctx: &mut GameContext, post: &EnforcementPost) {
        let side = if post.kind == KIND_FIXED_SCALE || post.kind == KIND_SCALE_APRON {
            PASS_PAN
        } else {
            -PASS_PAN
        };
        let volume = PASS_BASE_VOLUME * 1.4f64.min(self.ambience_scale());
        self.play_enforcement_marker(ctx, 1.0f64.min(volume), side);
        self.schedule_sound(
            PASS_MARKER_LEAD_S,
            "traffic/trooper_pass",
            1.0f64.min(volume),
            side,
        );
    }

    /// Fire a pass earcon for every post the truck has just gone by.
    pub fn update_marked_unit_passes(&mut self, ctx: &mut GameContext, previous_mi: f64) {
        let position = self.trip.position_mi;
        let crossed: Vec<usize> = self
            .trip
            .posts
            .iter()
            .enumerate()
            .filter(|(_, post)| {
                let trigger = post.at_mi + PASS_TRIGGER_MI;
                previous_mi < trigger && trigger <= position
            })
            .map(|(i, _)| i)
            .collect();
        for i in crossed {
            let post = &self.trip.posts[i];
            let id = post.id();
            if self.passed_post_ids.contains(&id) {
                continue;
            }
            self.passed_post_ids.insert(id);
            if post.tableau {
                // The tableau already gets its own richer pass -- the siren
                // lead and the stopped pair hard on the shoulder -- so the
                // anonymous marked-unit pass would only double it up.
                continue;
            }
            if !post.staffed {
                // An empty crossover is silent, always. It used to speak at the
                // top presence setting, and that is the cue that taught players
                // the police do not enforce: by ear it is identical to a
                // staffed unit, so a driver heard a trooper go by while
                // speeding and nothing happened -- because there was nobody in
                // the car (owner and Shane P., 2026-08-16). A marked unit you
                // can hear is now always one that can act.
                continue;
            }
            if post.is_scale() {
                continue; // the scale bed already covers the approach
            }
            let post = post.clone();
            self.play_marked_unit_pass(ctx, &post);
        }
    }

    // -- the tableau ---------------------------------------------------------

    /// The reliable "not you" line, with a seeded pinch of why.
    ///
    /// Deterministic per tableau (named, seeded like the scale-selection and
    /// observation draws elsewhere in this module): a reload never changes
    /// whether a given post's line carries a reason or which one. About half
    /// the time it names one of a small fixed set of reasons; the rest of the
    /// time it stays the bare fact. Terse mode always gets the bare fact --
    /// the reason is colour, not information a terse driver is shortchanged
    /// without.
    pub fn tableau_intro_message(&self, post: &EnforcementPost) -> SpokenMessage {
        let mut rng = PyRandom::new_from_str(&post_seed(
            Some(self.trip_seed),
            &post.id(),
            "tableau_intro",
        ));
        if rng.random() >= 0.5 {
            return SpokenMessage::new(TABLEAU_INTRO_LINE);
        }
        let reason = *rng.choice(&TABLEAU_INTRO_REASONS);
        let normal = format!("A trooper has somebody stopped on the shoulder {reason}, not you.");
        SpokenMessage::with_terse(normal, TABLEAU_INTRO_LINE)
    }

    /// The siren of a trooper working somebody else, heard before you reach
    /// them.
    ///
    /// Same two-element shape as the marked-unit pass -- the marker leads,
    /// then the vehicle -- with the siren asset standing in for the whoosh.
    /// This is the one enforcement sound that means "not about you": a trooper
    /// who already has a customer is off the hunt.
    ///
    /// The siren alone reads as easily as a driver's own pull-over starting,
    /// so it now says whose stop this is, every time, on top of the audio -- a
    /// tester mistook it for their own until the line was added.
    pub fn play_tableau_siren_pass(&mut self, ctx: &mut GameContext, post: &EnforcementPost) {
        let volume = 1.0f64.min(TABLEAU_SIREN_VOLUME * 1.4f64.min(self.ambience_scale()));
        self.play_enforcement_marker(ctx, volume, TABLEAU_SHOULDER_PAN);
        self.schedule_sound(
            PASS_MARKER_LEAD_S,
            "events/police_siren",
            volume,
            TABLEAU_SHOULDER_PAN,
        );
        // ROUTE: this line exists solely to stop the siren reading as YOUR
        // pull-over (the tester misread that created it). A busy channel
        // dropping the explainer recreates exactly that confusion, so it
        // waits its turn instead of dying stale.
        let mut message = self.tableau_intro_message(post);
        // A second stopped pair a few miles after the first: the same line
        // word for word sounded like the first one repeating (owner's drive
        // across Ohio, 2026-10-25, two troopers a minute apart).
        let id = post.id();
        let another_close_behind = self.trip.posts.iter().any(|other| {
            other.id() != id
                && self.tableau_siren_ids.contains(&other.id())
                && (post.at_mi - TABLEAU_REPEAT_WINDOW_MI..post.at_mi).contains(&other.at_mi)
        });
        if another_close_behind {
            message = SpokenMessage::new(TABLEAU_ANOTHER_LINE);
        }
        ctx.say_event_with(
            message,
            SayEvent::queued()
                .priority(EventPriority::Important)
                .category(SpeechCategory::Status),
        );
    }

    /// The stopped pair, panned hard to the shoulder as you go by.
    ///
    /// Reuses the pass-by vocabulary already used for a marked unit and for
    /// ordinary traffic: a cruiser and the car it stopped, both parked hard
    /// right, gone in a moment because that is exactly how long you are
    /// alongside a parked pair at highway speed. No marker, no radio duck --
    /// it is news, not a warning.
    pub fn play_tableau_pass(&mut self, ctx: &mut GameContext) {
        let volume = 1.0f64.min(TABLEAU_PASS_VOLUME * 1.4f64.min(self.ambience_scale()));
        ctx.audio
            .play_with("traffic/trooper_pass", volume, TABLEAU_SHOULDER_PAN);
        ctx.audio
            .play_with("traffic/car_pass", volume * 0.85, TABLEAU_SHOULDER_PAN);
    }

    /// Fire the siren lead and the shoulder pass for every tableau post.
    ///
    /// Deferred, then dropped for the trip, never layered on top of the
    /// player's own encounter. While the cab already has a demand on the
    /// driver -- their own pull-over, a hazard, a ramp, a microsleep, the
    /// arrival menu -- this is skipped outright: a trigger mile crossed during
    /// a busy frame is simply never revisited once the cab is quiet again, so
    /// the cue is lost rather than replayed out of place. A post that has
    /// already had its own look at the player (`declined`, set whether it let
    /// them go or wrote them up) never runs its tableau cue either: the story
    /// is already about the player, not "somebody else."
    pub fn update_tableaus(&mut self, ctx: &mut GameContext, previous_mi: f64) {
        if self.enforcement_busy() {
            return;
        }
        let position = self.trip.position_mi;
        let crosses = |trigger: f64| previous_mi < trigger && trigger <= position;
        let crossed: Vec<(usize, bool, bool)> = self
            .trip
            .posts
            .iter()
            .enumerate()
            .filter(|(_, post)| post.tableau && !post.declined)
            .map(|(i, post)| {
                (
                    i,
                    crosses(post.at_mi - TABLEAU_SIREN_LEAD_MI),
                    crosses(post.at_mi + PASS_TRIGGER_MI),
                )
            })
            .filter(|&(_, siren, pass)| siren || pass)
            .collect();
        for (i, siren, pass) in crossed {
            let post = self.trip.posts[i].clone();
            let id = post.id();
            if siren && !self.tableau_siren_ids.contains(&id) {
                self.tableau_siren_ids.insert(id.clone());
                self.play_tableau_siren_pass(ctx, &post);
            }
            if pass && !self.tableau_pass_ids.contains(&id) {
                self.tableau_pass_ids.insert(id);
                self.play_tableau_pass(ctx);
            }
        }
    }

    /// The weigh-station approach bed, swelling on the real clock.
    ///
    /// Open and closed are NOT two different ambiences. Two lot beds differing
    /// by activity level is exactly the discrimination that fails against a
    /// road bed, and it would be competing with the truck-stop and
    /// facility-gate ambiences besides. The swell says "scale". Open adds a
    /// spoken line, because an open scale costs money and time and has earned
    /// speech. Closed says nothing at all, and the silence is the answer.
    ///
    /// Whether a trooper is sitting on a closed apron stays unknowable. A
    /// sighted driver cannot reliably see that either, so it is fair tension
    /// rather than hidden information.
    pub fn update_scale_bed(&mut self, ctx: &mut GameContext) {
        let position = self.trip.position_mi;
        let mut nearest: Option<(f64, usize)> = None;
        for (i, post) in self.trip.posts.iter().enumerate() {
            if !post.is_scale() {
                continue;
            }
            let ahead = post.at_mi - position;
            if (-0.3..=SCALE_BED_START_MI).contains(&ahead)
                && nearest.is_none_or(|(best, _)| ahead < best)
            {
                nearest = Some((ahead, i));
            }
        }
        let Some((ahead, index)) = nearest else {
            if !self.scale_bed_key.is_empty() {
                self.scale_bed_key = String::new();
                self.scale_bed_volume = 0.0;
                ctx.audio.stop_loop_with(CH_SCALE, SCALE_BED_FADE_MS);
            }
            return;
        };
        let post = &self.trip.posts[index];
        let closeness = 1.0 - (ahead.max(0.0) / SCALE_BED_START_MI).clamp(0.0, 1.0);
        let mut ceiling = if post.kind == KIND_FIXED_SCALE {
            SCALE_BED_OPEN_MAX_VOLUME
        } else {
            SCALE_BED_CLOSED_MAX_VOLUME
        };
        ceiling *= self.ambience_scale();
        let volume = SCALE_BED_MIN_VOLUME + (ceiling - SCALE_BED_MIN_VOLUME).max(0.0) * closeness;
        post.write_id(&mut self.scale_bed_key);
        self.scale_bed_volume = volume;
        // start_loop dedupes on a running key, so this doubles as the level
        // update and self-heals if anything stopped the channel.
        ctx.audio.start_loop_with(
            CH_SCALE,
            "poi/weigh_station_lane",
            volume,
            SCALE_BED_FADE_MS,
        );
    }
}
