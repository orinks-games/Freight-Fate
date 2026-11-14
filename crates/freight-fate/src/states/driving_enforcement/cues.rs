//! The enforcement earcons: the signature marker, the marked-unit pass and
//! the weigh-station approach bed.

use ff_core::sim::enforcement_posts::{EnforcementPost, KIND_FIXED_SCALE, KIND_SCALE_APRON};

use crate::app::GameContext;
use crate::audio::CH_SCALE;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use super::{
    PASS_BASE_VOLUME, PASS_PAN, PASS_TRIGGER_MI, SCALE_BED_CLOSED_MAX_VOLUME, SCALE_BED_FADE_MS,
    SCALE_BED_MIN_VOLUME, SCALE_BED_OPEN_MAX_VOLUME, SCALE_BED_START_MI,
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
                // Busy with somebody else: silent, like an empty post. Its
                // siren and "not you" line read as the player's own stop
                // (owner, 2026-11-05).
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
