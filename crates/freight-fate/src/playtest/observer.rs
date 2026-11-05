//! A conservative real-window policy for a discovered road playtest.
//!
//! The observer never changes a drive, vehicle or assist field. It makes the
//! same paced keyboard decisions a player can make, queues them for the next
//! normal app frame, and lets the existing keeper/cruise systems own every
//! throttle and merge decision after that.

use crate::app::PlayerInputFrame;
use crate::states::base::{InputEvent, Key};

use super::road::Hit;

/// A clean stop made by the observer, either at its requested observation
/// point or before a situation that needs a person to make a safety choice.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObserverOutcome {
    Complete(String),
    Boundary(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    RoadArm,
    RoadReleaseArm,
    RoadWatch,
    DepartureWaitForAir,
    DepartureReleaseBrake,
    DepartureReleaseBrakeKey,
    DepartureAccelerate,
    DepartureReleaseAccelerator,
    DepartureWatch,
    NeutralizeReleaseControls,
    NeutralizeBrake,
    NeutralizeReleaseBrake,
    Settling,
}

/// A finite, data-driven policy for one selected [`Hit`].
pub struct AutonomousObserver {
    hit: Hit,
    phase: Phase,
    elapsed_s: f64,
    settle_s: f64,
    outcome: Option<ObserverOutcome>,
}

impl AutonomousObserver {
    /// Some ad-hoc mile selections have no distance ahead of them. A generic
    /// observer cannot prove a safe action for those without inventing a
    /// feature-specific scenario, so report the boundary before opening a
    /// window.
    pub fn new(hit: Hit) -> Result<Self, String> {
        if hit.origin_location.is_none() && hit.at_mi <= 0.0 {
            return Err(
                "AI boundary: this selected mile has no approach distance. Choose a discovered \
                 feature with --pick N so the observer has a player-visible stopping point."
                    .to_string(),
            );
        }
        let phase = if hit.origin_location.is_some() {
            Phase::DepartureWaitForAir
        } else {
            Phase::RoadArm
        };
        Ok(Self {
            hit,
            phase,
            elapsed_s: 0.0,
            settle_s: 0.0,
            outcome: None,
        })
    }

    pub fn outcome(&self) -> Option<&ObserverOutcome> {
        self.outcome.as_ref()
    }

    /// Decide the next normal player control frame. It never reaches a drive
    /// directly: full lane keeping owns steering, and the observer sends only
    /// player keys through the regular event path.
    pub fn step(&mut self, input: &mut PlayerInputFrame<'_>, dt: f64) -> bool {
        if matches!(
            self.phase,
            Phase::NeutralizeReleaseControls
                | Phase::NeutralizeBrake
                | Phase::NeutralizeReleaseBrake
                | Phase::Settling
        ) {
            return self.neutralize(input, dt);
        }
        if self.outcome.is_some() {
            return false;
        }
        self.elapsed_s += dt;
        if self.elapsed_s > 180.0 {
            self.boundary("the real-time safety budget expired before a safe handoff");
            return self.neutralize(input, dt);
        }
        let Some(drive) = input.driving_observation() else {
            self.boundary("the selected scenario left the driving state");
            return self.neutralize(input, dt);
        };
        if !drive.lane_keeping_full {
            self.boundary("full lane keeping is no longer active, so steering needs a human");
            return self.neutralize(input, dt);
        }
        if drive.hazard_active {
            self.boundary("a live hazard needs a human avoidance decision");
            return self.neutralize(input, dt);
        }
        if drive.pull_over_active {
            self.boundary("an enforcement stop needs a human compliance decision");
            return self.neutralize(input, dt);
        }
        if drive.off_pavement {
            self.boundary("the truck left the pavement and needs a human recovery decision");
            return self.neutralize(input, dt);
        }
        if drive.truck_damage_pct > 0.0 || drive.cargo_damage_pct > 0.0 {
            self.boundary("the truck or load took damage and needs a human assessment");
            return self.neutralize(input, dt);
        }

        match self.phase {
            Phase::RoadArm => {
                if !drive.speed_control_armed {
                    input.queue_player_input(InputEvent::key(Key::K));
                    self.phase = Phase::RoadReleaseArm;
                } else {
                    self.phase = Phase::RoadWatch;
                }
            }
            Phase::RoadReleaseArm => {
                input.queue_player_input(InputEvent::key_up(Key::K));
                self.phase = Phase::RoadWatch;
            }
            Phase::RoadWatch => {
                if drive.position_mi >= self.hit.at_mi {
                    self.complete("the discovered road feature reached its observation point");
                }
            }
            Phase::DepartureWaitForAir => {
                if !drive.parking_brake {
                    self.phase = Phase::DepartureAccelerate;
                } else if drive.air_ready {
                    input.queue_player_input(InputEvent::key(Key::P));
                    self.phase = Phase::DepartureReleaseBrake;
                }
            }
            Phase::DepartureReleaseBrake => {
                input.queue_player_input(InputEvent::key_up(Key::P));
                self.phase = Phase::DepartureReleaseBrakeKey;
            }
            Phase::DepartureReleaseBrakeKey => {
                self.phase = Phase::DepartureAccelerate;
            }
            Phase::DepartureAccelerate => {
                input.queue_player_input(InputEvent::key(Key::Up));
                self.phase = Phase::DepartureReleaseAccelerator;
            }
            Phase::DepartureReleaseAccelerator => {
                if drive.keeper_active || drive.cruise_active {
                    input.queue_player_input(InputEvent::key_up(Key::Up));
                    self.phase = Phase::DepartureWatch;
                }
            }
            Phase::DepartureWatch => {
                if !drive.departure_chain && drive.cruise_active {
                    self.complete("the facility departure handed from the cruise to cruise");
                }
            }
            Phase::NeutralizeReleaseControls
            | Phase::NeutralizeBrake
            | Phase::NeutralizeReleaseBrake
            | Phase::Settling => unreachable!("handled before a driving observation"),
        }
        if self.outcome.is_some() {
            self.neutralize(input, dt)
        } else {
            true
        }
    }

    fn complete(&mut self, detail: &str) {
        self.outcome = Some(ObserverOutcome::Complete(detail.to_string()));
        self.begin_neutralizing();
    }

    fn boundary(&mut self, detail: &str) {
        self.outcome = Some(ObserverOutcome::Boundary(detail.to_string()));
        self.begin_neutralizing();
    }

    fn begin_neutralizing(&mut self) {
        self.phase = Phase::NeutralizeReleaseControls;
        self.settle_s = 0.0;
    }

    /// Stop contributing controls before the speech drain. Every event here
    /// is an ordinary player input: releases first, then Down arrow cancels
    /// the active speed-control session and brakes briefly before release.
    fn neutralize(&mut self, input: &mut PlayerInputFrame<'_>, dt: f64) -> bool {
        match self.phase {
            Phase::NeutralizeReleaseControls => {
                for key in [Key::Up, Key::P, Key::K] {
                    input.queue_player_input(InputEvent::key_up(key));
                }
                self.phase = Phase::NeutralizeBrake;
                true
            }
            Phase::NeutralizeBrake => {
                input.queue_player_input(InputEvent::key(Key::Down));
                self.phase = Phase::NeutralizeReleaseBrake;
                true
            }
            Phase::NeutralizeReleaseBrake => {
                input.queue_player_input(InputEvent::key_up(Key::Down));
                self.phase = Phase::Settling;
                true
            }
            Phase::Settling => self.settle(input, dt),
            _ => unreachable!("normal driving phases do not neutralize"),
        }
    }

    /// Keep real frames alive until the driving event voice clears, so the
    /// normal shutdown path cannot cut the keeper or cruise announcement.
    /// The cap is a declared boundary for a stalled voice, not a silent wait.
    fn settle(&mut self, input: &mut PlayerInputFrame<'_>, dt: f64) -> bool {
        const MIN_DRAIN_S: f64 = 0.25;
        const MAX_DRAIN_S: f64 = 10.0;

        self.settle_s += dt;
        if self.settle_s >= MAX_DRAIN_S {
            self.outcome = Some(ObserverOutcome::Boundary(
                "player-facing driving speech did not settle before the safety cap".to_string(),
            ));
            return false;
        }
        self.settle_s < MIN_DRAIN_S || input.event_speech_busy()
    }
}
