//! The exit ramp a stop is reached by, laid out from the Green Book: its
//! deceleration lane, its curve, and its run down to the stop bar (the
//! realistic exit redesign, 2026-09-24; the pieces are in
//! `trip_models::ramps`).

use crate::sim::trip_models::{
    deceleration_lane_mi, exit_ramp_layout, ramp_speed_mph, ExitRampLayout, RoadStop,
};

use super::Trip;

impl Trip {
    /// What the exit ramp serving `stop` is sized from: the corridor's own
    /// limit at the gore standing in for the highway design speed, the ramp's
    /// speed (the controlling curve's), and the mainline grade the
    /// deceleration lane runs beside, in percent.
    fn exit_ramp_inputs(&self, stop: &RoadStop) -> (f64, f64, f64) {
        let ramp_mph = self.ramp_speed_at(stop.interchange_mi.unwrap_or(stop.at_mi));
        let highway_mph = self.corridor_limit_at(stop.at_mi);
        let grade_pct = self.grade_at(stop.at_mi) * 100.0;
        (highway_mph, ramp_mph, grade_pct)
    }

    /// Gore-to-stop-bar length of the exit ramp serving `stop`, in miles.
    ///
    /// The one place a ramp's length comes from. The Green Book deceleration
    /// lane for this corridor and ramp speed, then the exit's own ramp as
    /// OpenStreetMap measures it from the gore to the crossroad (the bake
    /// starts at the gore, so the lane goes in front). With no measured length
    /// it is the sourced default: the lane, the ramp speed's own curve, and a
    /// DERIVED climb plus an ASSUMED queue (see `exit_ramp_layout`).
    pub fn ramp_length_mi(&self, stop: &RoadStop) -> f64 {
        let (highway_mph, ramp_mph, grade_pct) = self.exit_ramp_inputs(stop);
        let measured = self
            .ramp_length_mi_at(stop.interchange_mi.unwrap_or(stop.at_mi))
            .map(|osm_mi| deceleration_lane_mi(highway_mph, ramp_mph, grade_pct) + osm_mi);
        exit_ramp_layout(highway_mph, ramp_mph, grade_pct, measured).length_mi()
    }

    /// The exit ramp serving `stop`, piece by piece, fitted to
    /// [`Self::ramp_length_mi`].
    pub fn exit_ramp_layout(&self, stop: &RoadStop) -> ExitRampLayout {
        let (highway_mph, ramp_mph, grade_pct) = self.exit_ramp_inputs(stop);
        let mut layout = exit_ramp_layout(
            highway_mph,
            ramp_mph,
            grade_pct,
            Some(self.ramp_length_mi(stop)),
        );
        let design_mph = self.ramp_design_mph_at(stop.interchange_mi.unwrap_or(stop.at_mi));
        layout.tangent_mph = design_mph.max(layout.curve_mph);
        layout
    }

    /// DERIVED: the speed the ramp nearest `route_mile` is built for, the
    /// AASHTO share of the corridor's limit (`ramp_speed_mph`), whatever its
    /// advisory sign says.
    pub fn ramp_design_mph_at(&self, route_mile: f64) -> f64 {
        let directional = self
            .interchange_at(route_mile, 2.0)
            .is_some_and(|ix| ix.ramp_far_end == "motorway");
        ramp_speed_mph(self.corridor_limit_at(route_mile), directional)
    }
}
