//! What a tank load sounds like from the driver's seat (port of
//! `freight_fate/states/driving_liquid.py`, the `LiquidLoadMixin`).
//!
//! The physics live in `sim::surge`. This is the part the player actually
//! meets: a gated cue layer that is completely silent on steady cruise and on
//! every other kind of freight, and that comes alive from the moment the liquid
//! starts running until it has settled again.
//!
//! Three decisions shape all of it.
//!
//! **The wave is sonified by its speed, not its force.** The oscillator's
//! velocity leads its displacement by a quarter period, and the force the truck
//! feels goes with displacement. So playing the liquid's *motion* means the
//! sound peaks a quarter cycle -- one and a half to three seconds, on these
//! tanks -- before the shove arrives. That warning is not predicted, estimated
//! or faked; it is the same relationship that makes a swing loudest at the
//! bottom and highest at the ends. Nothing is being modelled twice, and there
//! is nothing for a driver to learn to distrust.
//!
//! **Rate carries the danger.** A half-full smooth bore rolls slowly: a long,
//! late, heavy wave. A baffled tank slaps quickly and dies. The player hears
//! which one they are hauling in the tempo of wash and hit rather than in a
//! level, because tempo survives a mono speaker, a low effects volume, and the
//! jake brake running at full gain on top of it -- and level does not.
//!
//! **Nothing is panned.** Left and right are already fully spoken for: the road
//! bed's pan is the lane-guidance instrument and the edge ladder's is the drift
//! side. A tanker drifting in a bend would otherwise put three separate things
//! on one axis. Fore and aft is not a decision the driver makes anyway -- only
//! the timing and the size of the wave are -- so those are what get encoded.
//!
//! The sound sits high on purpose. The engine and the road own everything below
//! a kilohertz, and the jake is loudest at exactly the moment surge matters, so
//! this layer plays the *surface* of the liquid -- the wash and the slap against
//! the head, up where nothing else lives.

use ff_core::models::cargo_condition::cargo_condition_text;
use ff_core::sim::surge::LiquidLoad;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::audio::CH_SURGE;
use crate::states::driving::DrivingState;

/// The wash is the liquid on the move. Below this it is not doing anything a
/// driver needs to hear, and holding a bed under that floor would just be one
/// more thing making noise in a cab that already has plenty.
pub const SURGE_WASH_FLOOR: f64 = 0.10;
pub const SURGE_WASH_GAIN: f64 = 0.55;

/// The hit is the wave arriving. It is the load-bearing event -- it has to
/// survive a mono speaker at a low effects volume -- so it is the loudest thing
/// this layer does, and it is a one-shot rather than part of any bed.
pub const SURGE_HIT_GAIN: f64 = 0.85;
pub const SURGE_HIT_FLOOR: f64 = 0.18; // weaker arrivals than this are not worth a sound

/// The first real wave of a run gets spoken; after that the audio carries it.
/// A driver does not need to be told about the liquid every time they brake.
pub const SURGE_SPEAK_REACH: f64 = 0.55;
/// And the load settling gets spoken too. Without a downward line there is no
/// way to know the wave has damped -- silence is ambiguous between "settled"
/// and "the cue layer stopped working", and for a blind driver that is not a
/// distinction to leave hanging.
pub const SURGE_SETTLE_SPEAK_DELAY_S: f64 = 1.5;

/// A bend with liquid running sideways in it is the tanker rollover case, and
/// it gets its own voice: baffles do nothing about lateral surge, so this can
/// happen on the load that has been forgiving all day.
pub const SURGE_LATERAL_WARN: f64 = 0.45;
pub const SURGE_LATERAL_COOLDOWN_S: f64 = 20.0;

impl DrivingState {
    /// `_liquid()`: the tank aboard, or None for every other kind of freight.
    pub fn liquid(&self) -> Option<&LiquidLoad> {
        self.trip.truck.liquid.as_ref()
    }

    /// `_liquid_audio_ready()`: whether the surge assets are in this build,
    /// checked once.
    ///
    /// They are baked by `sound-test/liquid_surge.py` into the sound pack.
    /// A build made before that bake should fall back to the spoken layer in
    /// silence rather than log a missing asset on every frame.
    pub fn liquid_audio_ready(&mut self, ctx: &mut GameContext) -> bool {
        match self.liquid_audio_ok {
            Some(ready) => ready,
            None => {
                let ready = ctx.audio.has_asset("vehicle/liquid_wash");
                self.liquid_audio_ok = Some(ready);
                ready
            }
        }
    }

    // -- per-frame cue layer ------------------------------------------------------

    /// `_update_liquid_cues(dt)`.
    pub fn update_liquid_cues(&mut self, ctx: &mut GameContext, dt: f64) {
        let Some(liquid) = self.liquid().cloned() else {
            return;
        };
        let axis = &liquid.longitudinal;
        if !self.liquid_audio_ready(ctx) {
            self.update_liquid_speech(ctx, dt, &liquid);
            return;
        }

        // The wash: how fast the liquid is running, which is what leads.
        let motion = axis.motion();
        if motion >= SURGE_WASH_FLOOR {
            ctx.audio.start_loop_with(
                CH_SURGE,
                "vehicle/liquid_wash",
                SURGE_WASH_GAIN * motion,
                90,
            );
            ctx.audio
                .set_loop_volume(CH_SURGE, SURGE_WASH_GAIN * motion);
            self.liquid_wash_on = true;
        } else if self.liquid_wash_on {
            ctx.audio.stop_loop_with(CH_SURGE, 260);
            self.liquid_wash_on = false;
        }

        // The hit: the wave reaching the end of its run and turning over.
        if axis.struck && axis.strike_strength >= SURGE_HIT_FLOOR {
            ctx.audio.play_with(
                "vehicle/liquid_hit",
                SURGE_HIT_GAIN * axis.strike_strength,
                0.0,
            );
        }
        if liquid.lateral.struck && liquid.lateral.strike_strength >= SURGE_HIT_FLOOR {
            // Side to side gets a different voice, because it means something
            // different: this is the one that rolls trucks over.
            ctx.audio.play_with(
                "vehicle/liquid_hit_lateral",
                SURGE_HIT_GAIN * liquid.lateral.strike_strength,
                0.0,
            );
        }

        self.update_liquid_speech(ctx, dt, &liquid);
    }

    /// `_update_liquid_speech(dt, liquid)`.
    pub fn update_liquid_speech(&mut self, ctx: &mut GameContext, dt: f64, liquid: &LiquidLoad) {
        let terse = self.terse_speech(ctx);
        let reach = liquid.longitudinal.reach();

        if liquid.lateral.reach() >= SURGE_LATERAL_WARN {
            if self.liquid_lateral_cooldown_s <= 0.0 {
                self.liquid_lateral_cooldown_s = SURGE_LATERAL_COOLDOWN_S;
                ctx.say_event_with(
                    "Load running sideways. Ease off.",
                    SayEvent::new().category(SpeechCategory::Safety),
                );
            }
        } else {
            self.liquid_lateral_cooldown_s = 0.0f64.max(self.liquid_lateral_cooldown_s - dt);
        }

        if reach >= SURGE_SPEAK_REACH && !self.liquid_surge_said {
            self.liquid_surge_said = true;
            self.liquid_settled_said = false;
            ctx.say_event_with(
                "Load running forward.",
                SayEvent::queued().category(SpeechCategory::Status),
            );
            return;
        }

        // Settled: said once, after the wave has actually stayed down, so a
        // single quiet frame between cycles cannot claim it.
        if !self.liquid_surge_said {
            return;
        }
        if liquid.settled() {
            self.liquid_settle_timer_s += dt;
            if self.liquid_settle_timer_s >= SURGE_SETTLE_SPEAK_DELAY_S {
                self.liquid_surge_said = false;
                self.liquid_settle_timer_s = 0.0;
                if !self.liquid_settled_said {
                    self.liquid_settled_said = true;
                    let text = if terse {
                        "Load settled."
                    } else {
                        "The load has settled."
                    };
                    ctx.say_event_with(
                        text,
                        SayEvent::queued().category(SpeechCategory::Confirmation),
                    );
                }
            }
        } else {
            self.liquid_settle_timer_s = 0.0;
        }
    }

    /// `_stop_liquid_cues()`: drop the bed on any transition out of driving.
    /// A continuous sound must never outlive the thing it belongs to.
    pub fn stop_liquid_cues(&mut self, ctx: &mut GameContext) {
        if self.liquid_wash_on {
            ctx.audio.stop_loop_with(CH_SURGE, 120);
            self.liquid_wash_on = false;
        }
    }

    // -- spoken on demand ---------------------------------------------------------

    /// `liquid_status_clause()`: what is in the tank and how it will behave,
    /// for the status screens.
    ///
    /// A driver who cannot see the trailer has to be able to ask what they
    /// are hauling and get the answer that matters -- not the product name,
    /// which they already know, but whether this one will come back at them.
    pub fn liquid_status_clause(&self) -> String {
        let Some(liquid) = self.liquid() else {
            return String::new();
        };
        let tank = liquid.describe_tank();
        let fill = liquid.describe_fill();
        let behaviour = if liquid.baffled {
            "Baffles will damp the wave in a couple of cycles."
        } else {
            "Smooth bore: nothing inside to slow the wave down."
        };
        format!(
            "Tank trailer, {fill}, {tank}. {behaviour} One surge cycle takes about {:.0} seconds.",
            liquid.period_s()
        )
    }

    /// `liquid_condition_clause()`: the load's condition in the words that fit
    /// a tank.
    pub fn liquid_condition_clause(&self) -> String {
        if self.liquid().is_none() {
            return String::new();
        }
        let condition = self.trip.truck.cargo_damage_pct;
        if condition < 1.0 {
            return "settled".to_string();
        }
        format!(
            "{}, {condition:.0} percent",
            cargo_condition_text(condition, true)
        )
    }

    // The pickup walk-around says what is in the tank and how it will behave;
    // that lives in states/city_pickup.rs, where the driver is standing next to
    // the trailer and the truck does not exist yet.
}
