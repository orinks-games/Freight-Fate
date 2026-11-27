//! What the live report measured beyond the sky, read through the
//! weather system for the tracked point.

use super::{WeatherKind, WeatherSystem};
use crate::sim::weather_report::ObservationDetails;

impl WeatherSystem {
    /// What the live report measured as of the last poll, while the sky is
    /// live; for readouts that cannot poll the provider.
    pub fn live_report(&self) -> Option<&ObservationDetails> {
        self.live.then_some(self.live_report.as_ref()).flatten()
    }

    /// What else the live report for the tracked point measured, while the
    /// sky is live (not last-known or simulated).
    pub fn live_details(&mut self) -> Option<ObservationDetails> {
        if self.source_status() != "live" {
            return None;
        }
        let city = self.city.as_deref()?;
        let provider = self.provider.as_deref_mut()?;
        provider.get_details(city)
    }

    /// The live report's sustained wind in km/h, while the sky is live.
    pub fn live_wind_kmh(&mut self) -> Option<f64> {
        if self.source_status() != "live" {
            return None;
        }
        let city = self.city.as_deref()?;
        let provider = self.provider.as_deref_mut()?;
        provider.get_wind_kmh(city)
    }

    /// The condition the live report observed, before any driving
    /// adjustment.
    pub fn live_observed_kind(&self) -> Option<WeatherKind> {
        self.live_raw
    }
}
