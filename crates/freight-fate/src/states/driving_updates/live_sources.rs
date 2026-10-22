//! The live-data sources a drive can switch to mid-trip: real weather, real
//! traffic, real truck parking.
//!
//! `GameContext` only hands out borrows of its session-long providers, and a
//! `WeatherSystem`/`Trip` has to OWN its provider, so a switch flipped
//! mid-drive builds this drive's own -- the same class, transport and
//! behaviour, exactly as `driving/init.rs` does at construction.
//!
//! Lifted out of `radio.rs`, which shared the settings pass with these and
//! nothing else.

use std::sync::Arc;

use ff_core::sim::real_weather::RealWeatherProvider;
use ff_core::sim::real_weather_alerts::{alerts_chain_law_level, ALERT_POLL_MI};
use ff_core::sim::weather::WeatherProvider;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::net::UreqTransport;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

/// How far the truck rolls on before a point that has not answered is given
/// up on and the next one asked about instead.
const ALERT_ANSWER_MI: f64 = 2.0;

impl DrivingState {
    /// Read the Weather Service warnings for where the truck is, every
    /// [`ALERT_POLL_MI`] of road, and say a new one once: "Weather alert:
    /// High Wind Warning, gusts to 60 miles per hour." The trip keeps the
    /// current set so the chain law can follow a winter warning. Off with
    /// real weather off, and never blocking: the provider answers from its
    /// cache and fetches in the background, so a point that has not
    /// answered yet is asked again a mile on.
    pub fn sync_weather_alerts(&mut self, ctx: &mut GameContext) {
        if !ctx.settings.real_weather {
            if !self.trip.live_alerts.is_empty() {
                self.trip.live_alerts.clear();
            }
            self.alerts_pending = None;
            return;
        }
        let Some(provider) = ctx.weather_alerts_provider_arc() else {
            return;
        };
        let position = self.trip.position_mi;
        // Ask about a new point every ALERT_POLL_MI, and keep reading the
        // point already asked about until it answers: the cache is keyed to
        // the point asked, and a moving truck never asks the same one twice,
        // so reading the point just requested would never find anything (the
        // Pittsburgh drive, 2026-09-12: every poll came back empty).
        if self.alerts_pending.is_none() && position >= self.alerts_next_poll_mi {
            let (lat, lon) = self.trip.latlon_at(None);
            self.alerts_next_poll_mi = position + ALERT_POLL_MI;
            if lat == 0.0 && lon == 0.0 {
                // A synthetic route with no geometry has nowhere to ask about.
                return;
            }
            provider.request(lat, lon);
            self.alerts_pending = Some((lat, lon, position));
        }
        let Some((lat, lon, asked_at)) = self.alerts_pending else {
            return;
        };
        let Some(alerts) = provider.get(lat, lon) else {
            if position - asked_at > ALERT_ANSWER_MI {
                // The feed did not answer for this point in time; the next
                // point is asked a mile on rather than eight.
                self.alerts_pending = None;
                self.alerts_next_poll_mi = position + 1.0;
            }
            return;
        };
        self.alerts_pending = None;
        // A real winter warning in a career that is in July on its own
        // calendar is neither said nor allowed to post a chain law: the sky
        // is already kept out of season snow, and the warning has to agree
        // with it (seasonal audit, 2026-10-01).
        let alerts: Vec<_> = if self.trip.weather.winter_fits_calendar() {
            alerts
        } else {
            alerts
                .into_iter()
                .filter(|alert| alerts_chain_law_level(std::slice::from_ref(alert)) == 0)
                .collect()
        };
        // One line for everything new, not one per warning: a second safety
        // line in the same frame cuts the first, and the ladder then repeats
        // the cut one, so the driver would hear the wind warning twice.
        let fresh: Vec<String> = alerts
            .iter()
            .filter(|alert| self.alerts_said.insert(alert.id.clone()))
            .map(|alert| alert.spoken())
            .collect();
        if !fresh.is_empty() {
            ctx.audio.play("ui/warning");
            let line = if fresh.len() == 1 {
                format!("Weather alert: {}.", fresh[0])
            } else {
                format!("Weather alerts: {}.", fresh.join("; "))
            };
            ctx.say_event_with(line, SayEvent::new().category(SpeechCategory::Safety));
        }
        self.trip.live_alerts = alerts;
    }

    /// Re-read the Weather Service warnings where the truck stands, for a
    /// truck that is not moving (a wait at a chain checkpoint):
    /// `sync_weather_alerts` only asks again after `ALERT_POLL_MI` of road,
    /// so a parked truck would keep a warning that has since expired. Asks
    /// the provider (never blocking) and takes whatever it has cached for
    /// this point, silently; with real weather off, the alerts are cleared.
    pub fn refresh_weather_alerts_here(&mut self, ctx: &mut GameContext) {
        if !ctx.settings.real_weather {
            self.trip.live_alerts.clear();
            return;
        }
        let Some(provider) = ctx.weather_alerts_provider_arc() else {
            return;
        };
        let (lat, lon) = self.trip.latlon_at(None);
        if lat == 0.0 && lon == 0.0 {
            return;
        }
        provider.request(lat, lon);
        let Some(alerts) = provider.get(lat, lon) else {
            return;
        };
        let winter_fits = self.trip.weather.winter_fits_calendar();
        self.trip.live_alerts = alerts
            .into_iter()
            .filter(|alert| winter_fits || alerts_chain_law_level(std::slice::from_ref(alert)) == 0)
            .collect();
    }

    pub fn sync_weather_source(&mut self, ctx: &mut GameContext) {
        let real = ctx.settings.real_weather;
        let controls_calendar = ctx.settings.live_weather_controls_calendar;
        if real == self.weather_source_real
            && controls_calendar == self.live_weather_controls_calendar
        {
            return;
        }
        self.weather_source_real = real;
        self.live_weather_controls_calendar = controls_calendar;
        self.trip.weather.provider = if real {
            Some(
                Box::new(RealWeatherProvider::with_nws(Arc::new(UreqTransport)))
                    as Box<dyn WeatherProvider>,
            )
        } else {
            None
        };
        self.trip.weather.live_weather_controls_calendar = controls_calendar;
        if !controls_calendar {
            // Include time already driven when the active trip switches back
            // to the independent in-game calendar.
            self.trip.weather.game_hours =
                Some(profile_of(ctx).calendar_game_hours() + self.trip.game_minutes / 60.0);
        }
        if !real {
            self.trip.weather.live = false;
        }
        let effects = self.trip.weather.effects();
        ctx.audio.set_weather(effects.sound);
        ctx.audio.set_wind(effects.wind);
    }
}
