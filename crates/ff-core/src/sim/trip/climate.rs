//! Which climate row the simulated temperature reads along the route.

use super::Trip;
use crate::data::climate::climate_region;

impl Trip {
    /// The climate key at a route mile: [`climate_region`] at the truck's
    /// state and position, else the nearer city's climate, else the
    /// freight region (which is the climate key in the lower 48).
    pub fn climate_at(&self, mile: f64) -> String {
        let state = self.state_at(Some(mile));
        let (lat, lon) = self.latlon_at(Some(mile));
        if !state.is_empty() && (lat != 0.0 || lon != 0.0) {
            let code = self
                .state_codes
                .get(&state)
                .cloned()
                .unwrap_or_else(|| state.clone());
            if code.len() == 2 {
                if let Ok(climate) = climate_region(&code, lat, lon) {
                    return climate.to_string();
                }
            }
        }
        let (leg_i, leg_start) = self.leg_at_mile(mile);
        let leg = &self.route.legs[leg_i];
        let nearer = if mile - leg_start < leg.miles / 2.0 {
            leg_i
        } else {
            leg_i + 1
        };
        self.route
            .cities
            .get(nearer)
            .and_then(|c| self.world.cities.get(c))
            .map(|c| c.climate().to_string())
            .unwrap_or_else(|| self.region_at(mile))
    }
}
