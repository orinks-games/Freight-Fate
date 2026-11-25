//! What the co-driver tells the player (owner, 2026-10-19): what they did
//! and why, in one line each, and a message review of their own.
//!
//! The road's lines are quiet while the co-driver drives, so these are the
//! player's only news of the shift. Each goes in the message log under the
//! co-driver's category, where `[` and `]` reach it. Riding along, the
//! player also hears it, after whatever is speaking. Asleep, they hear
//! nothing; the lines that matter on waking ride the wake-up summary.

use ff_core::message_log::MessageCategory;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, Say, SayEvent, TRANSCRIPT_TARGET};
use crate::states::driving::DrivingState;
use crate::states::driving_updates::live;

use super::pilot::StopPurpose;
use super::TeamMode;
use ff_core::sim::chain_work::ChainStopKind;

/// How much a note matters once the player wakes up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteWeight {
    /// News in passing: heard riding along, kept in the review.
    Passing,
    /// Something the player should know on waking too: a fine, damage, an
    /// inspection.
    Wake,
}

impl DrivingState {
    /// The co-driver's name while they hold the wheel.
    pub fn co_driver_at_wheel_name(&self) -> Option<&'static str> {
        let team = self.team.as_ref()?;
        team.wheel.as_ref()?;
        Some(team.co_driver.name)
    }

    /// One line from the co-driver about the shift. Does nothing unless they
    /// hold the wheel.
    pub fn co_driver_note(&mut self, ctx: &mut GameContext, line: String, weight: NoteWeight) {
        let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) else {
            return;
        };
        if weight == NoteWeight::Wake {
            wheel.log.notes.push(line.clone());
        }
        let mode = wheel.mode;
        log::info!(target: TRANSCRIPT_TARGET, "[co-driver] note: {line}");
        // Asleep, the review keeps what the wake-up summary does: a shift's
        // worth of plans and lane changes is not news in the morning.
        if mode == TeamMode::RideAlong || weight == NoteWeight::Wake {
            ctx.message_log.add(&line, MessageCategory::CoDriver);
        }
        if mode == TeamMode::RideAlong {
            // Already in the review under its own category, so not again
            // under General.
            ctx.player_asked(|ctx| ctx.say_with(line, Say::queued().review(false)));
        }
    }

    /// A hazard call while the co-driver drives: riding along, the player
    /// hears it with its warning sound, as from the passenger seat. Asleep,
    /// nothing; how it ended is noted once it is behind the truck.
    pub fn co_driver_hazard_call(&mut self, ctx: &mut GameContext, sound: &str, line: &str) {
        let riding = self
            .team
            .as_ref()
            .and_then(|team| team.wheel.as_ref())
            .is_some_and(|wheel| wheel.mode == TeamMode::RideAlong);
        if !riding {
            return;
        }
        log::info!(target: TRANSCRIPT_TARGET, "[co-driver] hazard: {line}");
        ctx.message_log.add(line, MessageCategory::CoDriver);
        // The driving channel, as the player's own call would be: Safety on
        // the ladder, and dropped if the hazard is over before it is said.
        let line = line.to_string();
        ctx.player_asked(|ctx| {
            ctx.audio.play(sound);
            ctx.say_event_with(
                line,
                SayEvent::new()
                    .review(false)
                    .category(SpeechCategory::Safety)
                    .valid(live::hazard_active),
            );
        });
    }

    /// The co-driver has chosen a stop: where, how far, and what for.
    pub fn co_driver_note_plan(
        &mut self,
        ctx: &mut GameContext,
        place: &str,
        ahead_mi: f64,
        purpose: StopPurpose,
    ) {
        let Some(co_driver_name) = self.co_driver_at_wheel_name() else {
            return;
        };
        let why = match purpose {
            StopPurpose::Fuel => "for fuel",
            StopPurpose::Break => "for their 30-minute break",
            StopPurpose::ShiftEnd if self.player_rest_complete(ctx) => {
                "to park and hand you the wheel"
            }
            StopPurpose::ShiftEnd => "to park for the end of their shift",
        };
        let distance = ctx.settings.short_distance_text(ahead_mi.max(0.0));
        let line = format!("{co_driver_name} plans to stop at {place} in {distance}, {why}.");
        self.co_driver_note(ctx, line, NoteWeight::Passing);
    }

    /// A hazard the co-driver dealt with, once it is behind the truck: the
    /// same clear line the player hears driving, and for an animal, that it
    /// got across.
    pub fn co_driver_note_hazard(
        &mut self,
        ctx: &mut GameContext,
        names: &str,
        animal: bool,
        clear_text: &str,
    ) {
        if self.co_driver_at_wheel_name().is_none() {
            return;
        }
        let line = if animal {
            format!("{} got across ahead of the truck.", capitalize(names))
        } else {
            clear_text.to_string()
        };
        self.co_driver_note(ctx, line, NoteWeight::Passing);
    }

    /// Chains on at a chain checkpoint, with the co-driver at the wheel.
    /// The work is theirs, so their hours carry it and the player's fatigue
    /// does not (`do_install_chains`).
    pub fn co_driver_chain_up(&mut self, ctx: &mut GameContext) {
        let Some(co_driver_name) = self.co_driver_at_wheel_name() else {
            return;
        };
        if self.do_install_chains(ctx).is_some() {
            let line = format!("{co_driver_name} put the chains on at the chain control.");
            self.co_driver_note(ctx, line, NoteWeight::Wake);
        }
    }

    /// A chain stop the co-driver finished (`update_chain_stop_assist`):
    /// one note per job, for the wake-up summary.
    pub fn co_driver_chain_note(
        &mut self,
        ctx: &mut GameContext,
        kind: ChainStopKind,
        on_grade: bool,
    ) {
        let Some(co_driver_name) = self.co_driver_at_wheel_name() else {
            return;
        };
        let line = match kind {
            ChainStopKind::Install if on_grade => {
                format!("{co_driver_name} put the chains on when the chain law posted.")
            }
            ChainStopKind::Install => {
                format!("{co_driver_name} put the chains on before the chain control.")
            }
            ChainStopKind::Remove => format!("{co_driver_name} took the chains off on bare road."),
            ChainStopKind::Tighten => format!("{co_driver_name} tightened a slapping chain."),
            ChainStopKind::Repair => {
                self.co_driver_saw_damage();
                format!(
                    "{co_driver_name} fixed a broken chain with a spare link. Damage {:.0} percent.",
                    self.trip.truck.damage_pct
                )
            }
        };
        self.co_driver_note(ctx, line, NoteWeight::Wake);
    }

    /// A chain broke under the co-driver with no spare link to fix it.
    pub fn co_driver_no_spare_link(&mut self, ctx: &mut GameContext) {
        if self.co_driver_at_wheel_name().is_none() {
            return;
        }
        self.co_driver_saw_damage();
        let line = format!(
            "A chain broke and there is no spare link. Damage {:.0} percent.",
            self.trip.truck.damage_pct
        );
        self.co_driver_note(ctx, line, NoteWeight::Wake);
    }

    /// The co-driver knows about the truck's damage as it is now, so it is
    /// not also a reason to hand back.
    pub fn co_driver_saw_damage(&mut self) {
        let damage = self.trip.truck.damage_pct;
        if let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) {
            wheel.damage_seen = damage;
        }
    }

    /// The truck struck an animal with the co-driver at the wheel: news, not
    /// a reason to stop. The co-driver drives on unless the damage is more
    /// than a strike does.
    pub fn co_driver_animal_strike(&mut self, ctx: &mut GameContext, animal: &str) {
        let Some(co_driver_name) = self.co_driver_at_wheel_name() else {
            return;
        };
        self.co_driver_saw_damage();
        let damage = self.trip.truck.damage_pct;
        let line =
            format!("{co_driver_name} drove on after a collision with {animal}. Damage {damage:.0} percent.");
        self.co_driver_note(ctx, line, NoteWeight::Wake);
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}
