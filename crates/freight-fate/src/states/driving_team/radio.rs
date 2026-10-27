//! The co-driver and the radio (owner rules, 2026-10-17).
//!
//! - A radio that was off when they took the wheel stays off.
//! - A radio that was on is theirs to play with: they reach for the
//!   stations they like, flip through the dial, and sometimes switch it
//!   off for a while.
//! - The player always wins. Once the player changes the station or the
//!   power during the shift, the co-driver leaves the radio alone.

use ff_core::models::co_driver::{CoDriver, RadioHabits};
use ff_core::pyrandom::PyRandom;

use crate::app::{GameContext, TRANSCRIPT_TARGET};
use crate::states::driving::DrivingState;

/// The co-driver's listening habits over one shift.
pub struct RadioMood {
    habits: RadioHabits,
    /// The radio was off at the handover, or the player has touched it.
    pub hands_off: bool,
    /// Game minutes until they next reach for the dial.
    next_turn_min: f64,
    /// They switched it off themselves, and switch it back on after this.
    quiet_left_min: Option<f64>,
    last_game_min: Option<f64>,
    rng: PyRandom,
}

impl RadioMood {
    pub fn new(co_driver: &CoDriver, radio_on: bool, seed: i64) -> Self {
        let mut rng = PyRandom::new_from_str(&format!("co-driver-radio:{}:{seed}", co_driver.key));
        // The first turn comes early: a new driver at the wheel finds
        // their own station.
        let next_turn_min = 2.0 + rng.random() * 10.0;
        Self {
            habits: co_driver.habits,
            hands_off: !radio_on,
            next_turn_min,
            quiet_left_min: None,
            last_game_min: None,
            rng,
        }
    }

    fn next_gap(&mut self) -> f64 {
        self.habits.flip_every_min * (0.5 + self.rng.random())
    }
}

/// What the co-driver does with the radio this time.
enum Turn {
    Quiet,
    Tune(String),
    Flip(i64),
    Wake,
}

impl DrivingState {
    /// The player changed the station or the power themselves.
    pub fn player_touched_radio(&mut self, ctx: &GameContext) {
        if !ctx.speech_requested {
            return; // the co-driver's own hand
        }
        if let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) {
            wheel.radio.hands_off = true;
        }
    }

    /// The co-driver's hand on the radio, once a frame while they drive.
    pub fn update_co_driver_radio(&mut self, ctx: &mut GameContext) {
        let now = self.trip.game_minutes;
        let engine_on = self.trip.truck.engine_on;
        let enabled = self.radio.enabled;
        let current = self.radio_station_id.clone();
        let Some(mood) = self
            .team
            .as_mut()
            .and_then(|team| team.wheel.as_mut())
            .map(|wheel| &mut wheel.radio)
        else {
            return;
        };
        let elapsed = mood.last_game_min.map_or(0.0, |last| (now - last).max(0.0));
        mood.last_game_min = Some(now);
        if mood.hands_off || !engine_on {
            return;
        }
        let turn = if let Some(left) = mood.quiet_left_min.as_mut() {
            *left -= elapsed;
            if *left > 0.0 {
                return;
            }
            mood.quiet_left_min = None;
            mood.next_turn_min = mood.next_gap();
            Turn::Wake
        } else {
            mood.next_turn_min -= elapsed;
            if mood.next_turn_min > 0.0 {
                return;
            }
            mood.next_turn_min = mood.next_gap();
            if !enabled {
                // Off, and not by their hand: the player's choice stands.
                mood.hands_off = true;
                return;
            }
            if mood.rng.random() < mood.habits.quiet_chance {
                mood.quiet_left_min = Some(mood.habits.quiet_min);
                Turn::Quiet
            } else if let Some(station) = {
                // Scanned only when they reach for the dial: the scan walks
                // every station, and once a tick it was most of the cost of
                // a slept-through shift's sped-up stops.
                let receivable: Vec<String> = self
                    .radio
                    .receivable_stations()
                    .into_iter()
                    .filter(|reception| !reception.fallback)
                    .map(|reception| reception.station.id)
                    .collect();
                mood.habits
                    .leans
                    .iter()
                    .find(|id| receivable.iter().any(|r| r == *id))
            }
            .filter(|id| **id != current)
            {
                Turn::Tune(station.to_string())
            } else if mood.habits.leans.contains(&current.as_str()) {
                // Already on a favourite: leave it.
                return;
            } else {
                Turn::Flip(if mood.rng.random() < 0.5 { 1 } else { -1 })
            }
        };
        match turn {
            Turn::Quiet => {
                log::info!(target: TRANSCRIPT_TARGET, "[co-driver] switches the radio off");
                self.toggle_radio(ctx);
            }
            Turn::Wake => {
                if !self.radio.enabled {
                    log::info!(target: TRANSCRIPT_TARGET, "[co-driver] switches the radio back on");
                    self.toggle_radio(ctx);
                }
            }
            Turn::Tune(station) => {
                log::info!(target: TRANSCRIPT_TARGET, "[co-driver] tunes to {station}");
                self.tune_radio_to(ctx, &station);
            }
            Turn::Flip(direction) => {
                log::info!(target: TRANSCRIPT_TARGET, "[co-driver] flips the dial");
                self.tune_radio(ctx, direction);
            }
        }
    }
}
