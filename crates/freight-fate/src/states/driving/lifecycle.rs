//! `enter` / `exit` and the facade's own helpers -- everything
//! `freight_fate/states/driving.py` defined itself rather than inheriting
//! from a mixin (`_terse_speech`, `_absolute_game_hour`, `_logbook_location`,
//! the entry-status sentences, and the retiring spoken instructions).

use ff_core::achievements::add_unique_stat;
use ff_core::sim::hos::{clock_text, is_night};
use ff_core::sim::weather::WeatherKind;
use ff_core::spoken_advice::{instruction_retired, note_demonstrated};

use crate::app::{GameContext, Say};
use crate::states::driving_core::*;

use super::DrivingState;

impl DrivingState {
    // -- small facade helpers ----------------------------------------------------

    /// `_terse_speech()`.
    pub fn terse_speech(&self, ctx: &GameContext) -> bool {
        ctx.settings.renders_terse()
    }

    /// `_absolute_game_hour(trip_minutes=None)`: the career clock plus this
    /// trip's elapsed time.
    pub fn absolute_game_hour(&self, ctx: &GameContext, trip_minutes: Option<f64>) -> f64 {
        let trip_minutes = trip_minutes.unwrap_or(self.trip.game_minutes);
        profile_of(ctx).game_hours + trip_minutes / 60.0
    }

    /// `_logbook_location()`: where a duty-log entry says the truck was.
    pub fn logbook_location(&self, ctx: &GameContext) -> String {
        if self.phase == DRIVE_PHASE_PICKUP {
            return format!("local route to {}", self.pickup_facility_text(ctx));
        }
        // The surface chain once off the highway.
        let route = &self.trip.route;
        if route.legs.is_empty() {
            return self.job.destination.clone();
        }
        let index = self.trip.current_leg_index().min(route.legs.len() - 1);
        let leg = &route.legs[index];
        if self.surface_chain {
            return format!("{} in {}", leg.highway, self.job.destination);
        }
        if self.departure_chain {
            return format!("{} in {}", leg.highway, self.job.origin);
        }
        let start = route.cities.get(index).cloned().unwrap_or_default();
        let end = route.cities.get(index + 1).cloned().unwrap_or_default();
        format!("{} from {start} to {end}", leg.highway)
    }

    /// `_instruction_retired(action)` (research doc R7).
    pub fn instruction_retired(&self, ctx: &mut GameContext, action: &str) -> bool {
        let binding = ctx.control_hint(action);
        let automatic = self.trip.truck.transmission.automatic;
        let Some(profile) = ctx.profile.as_mut() else {
            return false;
        };
        instruction_retired(profile, action, &binding, automatic)
    }

    /// `_note_instruction_demonstrated(action)`: record that the player
    /// performed `action`, toward retiring its spoken instruction.
    pub fn note_instruction_demonstrated(&self, ctx: &mut GameContext, action: &str) {
        let binding = ctx.control_hint(action);
        let automatic = self.trip.truck.transmission.automatic;
        let Some(profile) = ctx.profile.as_mut() else {
            return;
        };
        note_demonstrated(profile, action, &binding, automatic);
    }

    /// Set the take-exit hint the stop callout speaks: the named control
    /// while it is still being taught, and nothing once the player has armed
    /// the signal enough times (research doc R7). Named through control_hint
    /// so a remapped key or a controller says the right thing, or re-teaches.
    pub fn refresh_exit_hint(&mut self, ctx: &mut GameContext) {
        self.trip.exit_hint = if self.instruction_retired(ctx, "take_exit") {
            String::new()
        } else {
            ctx.control_hint("take_exit")
        };
    }

    /// `_engine_entry_instruction()`.
    pub fn engine_entry_instruction(&self, ctx: &mut GameContext) -> String {
        if self.trip.truck.engine_on {
            return "Engine idling.".to_string();
        }
        // Once the player has started the engine a few times, the key prompt
        // is noise -- the state alone is enough, and the help key still names
        // the control on demand (research doc R7). A remapped key or a switch
        // of transmission re-arms it.
        if self.instruction_retired(ctx, "engine") {
            return "Engine off.".to_string();
        }
        format!("Press {} to start the engine.", ctx.control_hint("engine"))
    }

    /// The "F1 lists the controls" pointer, retired once the player has
    /// opened the controls help a few times (research doc R7).
    pub fn help_hint_tail(&self, ctx: &mut GameContext) -> String {
        if self.instruction_retired(ctx, "help") {
            return String::new();
        }
        format!("{} lists the controls.", ctx.control_hint("help"))
    }

    /// `_resumed_speed_control_status()`.
    ///
    /// The state, and the key that cancels it until the player has pressed
    /// that key a few times. The open-road target is on Space and Tab.
    pub fn resumed_speed_control_status(&self, ctx: &mut GameContext) -> String {
        if !self.speed_control_armed {
            return String::new();
        }
        if self.instruction_retired(ctx, "cruise_set") {
            return "Automatic speed control paused. ".to_string();
        }
        format!(
            "Automatic speed control paused. {} cancels it. ",
            ctx.control_hint("cruise_set")
        )
    }

    /// The parking-brake pointer at the end of the resume line, retired once
    /// the player has released the brake a few times.
    pub fn parking_brake_hint_tail(&self, ctx: &mut GameContext) -> String {
        if self.instruction_retired(ctx, "parking_brake") {
            return String::new();
        }
        format!(
            "At air ready, {} releases the parking brake.",
            ctx.control_hint("parking_brake")
        )
    }

    /// The transmission as a whole sentence.
    pub fn transmission_sentence(&self) -> &'static str {
        if self.trip.truck.transmission.automatic {
            "Automatic transmission."
        } else {
            "Manual transmission."
        }
    }

    /// `_parked_entry_status()`.
    pub fn parked_entry_status(&self) -> String {
        let truck = &self.trip.truck;
        let engine = if truck.engine_on {
            "Engine idling"
        } else {
            "Engine off"
        };
        let air = if truck.air_ready() {
            "air ready".to_string()
        } else {
            format!("air {:.0} psi", truck.air_pressure_psi())
        };
        let brake = if truck.parking_brake {
            "parking brake set"
        } else {
            "parking brake released"
        };
        format!("{engine}, {air}, {brake}.")
    }

    /// `_record_weather_achievement(event=True)`.
    pub fn record_weather_achievement(&self, ctx: &mut GameContext) {
        if ctx.profile.is_none() {
            return;
        }
        let kind = self.trip.weather.current;
        let seen = add_unique_stat(
            ctx.profile.as_mut().expect("checked above"),
            "weather_seen",
            kind.name(),
        );
        match kind {
            WeatherKind::Rain | WeatherKind::HeavyRain => {
                ctx.award_driving_achievement("rain_driver");
            }
            WeatherKind::Snow | WeatherKind::Ice | WeatherKind::Wind => {
                ctx.award_driving_achievement("winter_or_wind");
            }
            WeatherKind::Fog | WeatherKind::Thunderstorm => {
                ctx.award_driving_achievement("low_visibility");
            }
            _ => {}
        }
        if kind == WeatherKind::Thunderstorm {
            ctx.award_driving_achievement("storm_driving");
        }
        if seen >= WeatherKind::ALL.len() {
            ctx.award_driving_achievement("weather_collector");
        }
    }

    // -- lifecycle ----------------------------------------------------------------

    /// `enter()`: the objective, the clock, the weather and the truck's
    /// state, spoken once when the drive first becomes active. Re-entry (a
    /// menu popping back to the road) is deliberately silent.
    pub fn enter_drive(&mut self, ctx: &mut GameContext) {
        if self.entered_once {
            return;
        }
        self.entered_once = true;
        self.log_assist_configuration(ctx);
        self.log_pace_configuration(ctx);
        self.refresh_exit_hint(ctx);
        ctx.clear_music_rotation();
        ctx.audio.stop_music_with(800);
        self.play_radio_current(ctx);
        let sound = self.trip.weather.effects().sound;
        ctx.audio.set_weather(sound);
        let wind = self.trip.weather.effects().wind;
        ctx.audio.set_wind(wind);
        let mode = if self.trip.truck.transmission.automatic {
            "automatic"
        } else {
            "manual"
        };
        let now = clock_text(self.trip.local_hour());
        let imperial = ctx.settings.imperial_units;
        if self.resumed {
            let hours_used = self.trip.game_minutes / 60.0;
            let (drive_name, destination) = if self.phase == DRIVE_PHASE_PICKUP {
                ("pickup drive", self.pickup_facility_text(ctx))
            } else {
                ("loaded delivery", self.job.spoken_destination().to_string())
            };
            let progress = if self.phase == DRIVE_PHASE_PICKUP {
                self.pickup_progress_summary(ctx)
            } else {
                self.trip.progress_summary(imperial)
            };
            let speed_control = self.resumed_speed_control_status(ctx);
            let report_lead = self.trip.weather.report_lead(imperial);
            if self.terse_speech(ctx) {
                let parked = self.parked_entry_status();
                let text = format!(
                    "Resuming {drive_name}: {destination}. {progress} {hours_used:.1} of {:.0} \
                     hours used. {now}. {mode}. {report_lead}. {speed_control}{parked}",
                    self.job.deadline_game_h
                );
                ctx.say_with(text, Say::queued());
            } else {
                let engine = self.engine_entry_instruction(ctx);
                let transmission = self.transmission_sentence();
                let brake = self.parking_brake_hint_tail(ctx);
                let text = format!(
                    "Resuming {drive_name}: {:.0} tons of {} to {destination}. {progress} \
                     {hours_used:.1} of {:.0} hours. {now}. {transmission} {report_lead}. \
                     {speed_control}{engine} {brake}",
                    self.job.weight_tons, self.job.cargo.label, self.job.deadline_game_h,
                );
                let text = text.trim_end().to_string();
                ctx.say_with(text, Say::queued());
            }
        } else {
            let objective = if self.phase == DRIVE_PHASE_PICKUP {
                format!(
                    "Pickup dispatch: deadhead from the terminal to {}. ",
                    self.pickup_facility_text(ctx)
                )
            } else {
                format!(
                    "Loaded for {}. {} ",
                    self.destination_facility_text(ctx),
                    self.trip.progress_summary(imperial)
                )
            };
            let report_lead = self.trip.weather.report_lead(imperial);
            if self.terse_speech(ctx) {
                let parked = self.parked_entry_status();
                let text = format!("{objective}{now}. {mode}. {report_lead}. {parked}");
                ctx.say_with(text, Say::queued());
            } else {
                let engine = self.engine_entry_instruction(ctx);
                let transmission = self.transmission_sentence();
                let help = self.help_hint_tail(ctx);
                let text = format!(
                    "At the wheel. {objective}It is {now}. {transmission} {report_lead}. {engine} \
                     {help}"
                );
                ctx.say_with(text.trim_end().to_string(), Say::queued());
            }
        }
        if self.tutorial.is_some() {
            let mut tutorial = self.tutorial.take().expect("checked above");
            tutorial.begin(ctx);
            self.tutorial = Some(tutorial);
        }
        if self.phase == DRIVE_PHASE_DELIVERY {
            self.record_weather_achievement(ctx);
            if !self.trip.truck.transmission.automatic {
                ctx.award_driving_achievement("manual_driver");
            }
        }
    }

    /// `exit()`: hand the cab back -- horn off, liquid cues stopped, the
    /// radio's dial written to settings, the world's audio silenced.
    pub fn exit_drive(&mut self, ctx: &mut GameContext) {
        // The drive is over or set aside: the wheel and the logbook are the
        // player's again before anything is saved.
        self.take_wheel_back(ctx);
        ctx.audio.horn_stop();
        // The siren's dead-man's switch only ticks from inside update(), so
        // once this state stops running it can never time itself out --
        // leave to the menu mid pull-over and it outlived every drive that
        // followed, until the game closed. Stop it explicitly, the same call
        // a resolved stop already makes (`end_stop_audio`).
        self.siren.stop(ctx.audio.as_mut());
        self.stop_liquid_cues(ctx);
        {
            let mut settings = RadioSettingsMut(&mut ctx.settings);
            self.radio.write_settings(&mut settings);
        }
        let _ = ctx.settings.save();
        // The engine is handed back centred. The backend keeps the engine's
        // pan across stops and drives on purpose, so a drive that ended
        // mid-lean used to leave it leaning for whatever ran the engine next
        // (review I7, 2026-09-19). The tracker goes back to "nothing written"
        // with it, so a drive that is re-entered writes its first frame too.
        ctx.audio.set_engine_pan(0.0);
        self.engine_guide_pan_applied = None;
        self.road_pan_applied = None;
        ctx.audio.stop_world();
        self.forget_stopped_loops();
        ctx.audio.stop_music_with(600);
        ctx.apply_volumes();
    }

    /// Clear the latches of every cue loop `stop_world` just silenced.
    ///
    /// Each of these loops starts once on its latch rather than every frame,
    /// so a latch left set kept its sound off after the world came back: the
    /// lane guide tone stayed silent through the next drift after a pause,
    /// which is the tone saying "centred" to a driver who was not. Every
    /// place that stops the world under a live drive calls this after it.
    pub fn forget_stopped_loops(&mut self) {
        self.reverse_cue_active = false;
        self.air_cue_active = false;
        self.jake_cue_key = None;
        self.lane_guide_tone_on = false;
        self.lane_guide_pan_applied = 0.0;
    }

    /// `is_night(self.trip.local_hour)`: the drive's own day/night flag, for
    /// the music rotation.
    pub fn night_now(&self) -> bool {
        is_night(self.trip.local_hour())
    }

    /// The driving-assist configuration, written to the session log once
    /// per drive. Not spoken. A tester's "the assist ran the light" is
    /// unanswerable from a log that never says which assists were on: the
    /// Tyler report of 2026-09-03 replayed clean on the bench with every
    /// assist on, and the log could not say whether the player's were.
    fn log_assist_configuration(&self, ctx: &GameContext) {
        let s = &ctx.settings;
        let fields: Vec<String> = ff_core::settings::DRIVING_ASSIST_FIELDS
            .iter()
            .zip(s.assist_values().iter())
            .map(|(field, value)| match value {
                ff_core::settings::AssistValue::Flag(on) => {
                    format!("{field}={}", if *on { "on" } else { "off" })
                }
                ff_core::settings::AssistValue::Mode(mode) => format!("{field}={mode}"),
            })
            .collect();
        log::info!(
            "driving assists: preset {}, {}, selected_stop_assist={}, speed_keeper={}, \
             predictive_cruise={}, driving_speech={}",
            s.driving_assistance_preset,
            fields.join(", "),
            if s.selected_stop_assist { "on" } else { "off" },
            if s.speed_keeper { "on" } else { "off" },
            if s.predictive_cruise { "on" } else { "off" },
            s.driving_speech,
        );
    }
}
