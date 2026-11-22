//! The first-run tutorial and the instructor hook a driving-school lesson
//! shares with it.

use ff_core::first_run::{FirstRun, FirstRunCue, FirstRunEvent, FirstRunStep, ROLLING_MPH};
use ff_core::sim::vehicle::TruckState;

use crate::app::{GameContext, Say};

use super::profile_mut_of;

/// What a drive calls on its instructor, if it has one: the first-run
/// [`Tutorial`], or a driving-school lesson that duck-typed it in Python.
pub trait Instructor {
    fn begin(&mut self, ctx: &mut GameContext);
    /// True when the next engine start earns the instructor's own line,
    /// which then says what to do at air ready: the plain "Engine running."
    /// line keeps only the readout, so the instruction is said once.
    fn speaks_engine_start(&self) -> bool {
        false
    }
    fn on_engine_started(&mut self, ctx: &mut GameContext);
    /// Air reached the release threshold, and "Air ready." was said.
    fn on_air_ready(&mut self) {}
    /// True when the instructor spoke its own release line, which then
    /// stands in for the plain confirmation.
    fn on_parking_brake_released(&mut self, ctx: &mut GameContext) -> bool;
    fn on_gear_engaged(&mut self, ctx: &mut GameContext);
    fn update(&mut self, ctx: &mut GameContext, dt: f64, truck: &TruckState);
}

/// First-drive guidance, spoken step by step as the player succeeds.
///
/// Deliberately verbosity-blind (research doc, R15): terse speech is a
/// filter on running commentary, and first-run teaching is not commentary.
/// A brand-new player who picks terse before their first drive -- exactly
/// the player who hates chatty games -- must still be told the status,
/// help, and hazard keys exist, or they can never pull information they
/// were never told about. The gate is `tutorial_done` itself: this type
/// is only constructed while the walkthrough is unfinished, and finishing
/// it then flipping terse on resurrects nothing.
///
/// The steps and the reminder timing are [`FirstRun`], in `ff-core`; this
/// type puts each cue in words for the player's keys or gestures.
#[derive(Debug, Default)]
pub struct Tutorial {
    run: FirstRun,
}

impl Tutorial {
    pub fn new() -> Self {
        Self::default()
    }

    /// Where the walkthrough is.
    pub fn step(&self) -> FirstRunStep {
        self.run.step()
    }

    /// The words for `cue`.
    fn say_cue(ctx: &mut GameContext, cue: FirstRunCue, truck: Option<&TruckState>) {
        let touch = ctx.touch_hints();
        let automatic = ctx.settings.automatic_transmission;
        let text = match cue {
            // On touch, gas releases the parking brake and drives, and a
            // shift works its own clutch.
            FirstRunCue::AfterEngine if touch && automatic => {
                "At air ready, swipe up and hold to release the parking brake and drive."
                    .to_string()
            }
            FirstRunCue::AfterEngine if touch => {
                "At air ready, swipe up and hold to release the parking brake, then flick up with \
                 a second finger for first gear."
                    .to_string()
            }
            FirstRunCue::AfterEngine if automatic => format!(
                "At air ready, press {} to release the parking brake, then hold {} to \
                 accelerate.",
                ctx.control_hint("parking_brake"),
                ctx.control_hint("accelerate")
            ),
            FirstRunCue::AfterEngine => format!(
                "At air ready, press {} to release the parking brake, then hold {}, select \
                 {} for first gear, and release the clutch.",
                ctx.control_hint("parking_brake"),
                ctx.control_hint("clutch"),
                ctx.control_hint("gear_first")
            ),
            // On touch the gas that released it is already held.
            FirstRunCue::BrakeReleasedAutomatic if touch => "Parking brake released.".to_string(),
            FirstRunCue::BrakeReleasedAutomatic => format!(
                "Parking brake released. Hold {} to accelerate.",
                ctx.control_hint("accelerate")
            ),
            FirstRunCue::BrakeReleasedManual => {
                "Parking brake released. Shift into first gear.".to_string()
            }
            FirstRunCue::InGear if touch => "In gear. Swipe up and hold to accelerate.".to_string(),
            FirstRunCue::InGear => format!(
                "In gear. Hold {} to accelerate.",
                ctx.control_hint("accelerate")
            ),
            FirstRunCue::Rolling if touch => format!(
                "Rolling. {} for your speed. Tap with three fingers, then double tap, for a \
                 full report. Flick up with two fingers for all the controls. Brake hard at \
                 a hazard warning. Swipe down deep and hold to stop fast.",
                ctx.touch_instruction(crate::bindings::Action::Speed)
            ),
            FirstRunCue::Rolling => format!(
                "Rolling. {} for your speed, {} for a full report, {} for all the controls. Brake \
                 hard at a hazard warning. {} stops fast.",
                ctx.control_hint("speed"),
                ctx.control_hint("status_menu"),
                ctx.control_hint("help"),
                ctx.control_hint("emergency_brake")
            ),
            FirstRunCue::Reminder(step) => Self::reminder_text(ctx, step, truck),
        };
        ctx.say_with(text, Say::queued());
    }

    fn reminder_text(ctx: &GameContext, step: FirstRunStep, truck: Option<&TruckState>) -> String {
        let touch = ctx.touch_hints();
        let brake_set = truck.is_some_and(|t| t.parking_brake);
        let air_ready = truck.is_some_and(|t| t.air_ready());
        if step == FirstRunStep::StartEngine {
            return if touch {
                "Reminder: turn two fingers right to start the engine.".to_string()
            } else {
                format!(
                    "Reminder: press {} to start the engine.",
                    ctx.control_hint("engine")
                )
            };
        }
        if brake_set && air_ready && touch {
            "Reminder: air is ready. Swipe up and hold to release the parking brake and drive."
                .to_string()
        } else if brake_set && air_ready {
            // The timer does not know the compressor finished: "wait for
            // air" right after "Air ready" contradicted the truck (agent
            // drive, 2026-09-01). Air is up, so the only step left is P.
            format!(
                "Reminder: air is ready. Press {} to release the parking brake.",
                ctx.control_hint("parking_brake")
            )
        } else if brake_set && touch {
            "Reminder: wait for air pressure to reach 100 psi, then swipe up and hold to \
             release the parking brake and drive."
                .to_string()
        } else if brake_set {
            format!(
                "Reminder: wait for air pressure to reach 100 psi, then press {} to release \
                 the parking brake.",
                ctx.control_hint("parking_brake")
            )
        } else if touch {
            "Reminder: hold a pedal and flick up with a second finger for first gear.".to_string()
        } else {
            format!(
                "Reminder: hold {}, select {}, then release the clutch.",
                ctx.control_hint("clutch"),
                ctx.control_hint("gear_first")
            )
        }
    }
}

impl Instructor for Tutorial {
    fn begin(&mut self, ctx: &mut GameContext) {
        let text = if ctx.touch_hints() {
            "Your first run. Turn two fingers right to start the engine.".to_string()
        } else {
            format!(
                "Your first run. Press {} to start the engine.",
                ctx.control_hint("engine")
            )
        };
        ctx.say_with(text, Say::queued());
    }

    fn speaks_engine_start(&self) -> bool {
        self.run.step() == FirstRunStep::StartEngine
    }

    fn on_engine_started(&mut self, ctx: &mut GameContext) {
        if let Some(cue) = self.run.next(FirstRunEvent::EngineStarted) {
            Self::say_cue(ctx, cue, None);
        }
    }

    fn on_air_ready(&mut self) {
        self.run.next(FirstRunEvent::AirReady);
    }

    fn on_parking_brake_released(&mut self, ctx: &mut GameContext) -> bool {
        let automatic = ctx.settings.automatic_transmission;
        match self
            .run
            .next(FirstRunEvent::ParkingBrakeReleased { automatic })
        {
            Some(cue) => {
                Self::say_cue(ctx, cue, None);
                true
            }
            None => false,
        }
    }

    fn on_gear_engaged(&mut self, ctx: &mut GameContext) {
        if let Some(cue) = self.run.next(FirstRunEvent::GearEngaged) {
            Self::say_cue(ctx, cue, None);
        }
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64, truck: &TruckState) {
        if truck.speed_mph() > ROLLING_MPH {
            if let Some(cue) = self.run.next(FirstRunEvent::Rolling) {
                Self::say_cue(ctx, cue, Some(truck));
                profile_mut_of(ctx).tutorial_done = true;
                ctx.save_profile();
                return;
            }
        }
        if let Some(cue) = self.run.next(FirstRunEvent::Elapsed(dt)) {
            Self::say_cue(ctx, cue, Some(truck));
        }
    }
}
