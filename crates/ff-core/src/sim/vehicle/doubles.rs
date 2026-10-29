//! What the hooked trailers are: one box, or a set of doubles. No saved
//! state -- the driving layer works it out from the job every time a drive
//! starts, the same way the tank's `liquid` is.
//!
//! A set of doubles changes three things on the truck (the numbers and their
//! sources live in `models::doubles`):
//!
//! - **gross weight**: both trailers and the converter dolly are tare, and
//!   the legal gross is the route's cap rather than always 80,000 lb;
//! - **the rear trailer**: a quick steer at speed swings it harder than the
//!   tractor (rearward amplification), and a crosswind pushes the set around
//!   more than a single box;
//! - **reverse**: a set of doubles is not backed.

use super::roll::ROLL_WARN_SHARE;
use super::{
    TruckState, CARGO_CORNER_PCT_PER_G_S, KG_PER_LB, LEGAL_GVW_KG, REFERENCE_CARGO_KG,
    TRAILER_TARE_KG,
};
use crate::data::corners::TRUCK_ROLLOVER_G;
use crate::data::lcv_turnpikes::{cargo_requires_lcv_turnpike, filter_lcv_turnpike_routes};
use crate::data::world::World;
use crate::data::world_models::Route;
use crate::models::doubles::{
    doubles_trailer_for_cargo, legacy_trip_legal_gvw_lb, legal_gvw_lb_for_route,
    rearward_amplification, sway_reference_gross_lb, trailer_set_extra_tare_kg, trailer_units,
    DOUBLES_NO_REVERSE_TEXT, LIGHT_SET_SWAY_MAX, REAR_TRAILER_FREIGHT_SHARE,
};
use crate::pyfmt::fmt_grouped;
use crate::sim::transmission::ShiftResult;

/// Below this the rear trailer's swing is not worth a word: yard and street
/// speeds, where a set is steered around corners, not whipped. ASSUMED; the
/// SAE J2179 lane-change test FHWA cites for rearward amplification is run
/// at 55 mph.
pub const REAR_WHIP_MIN_MPH: f64 = 40.0;

/// The trailers on the fifth wheel, as far as weight and handling go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrailerSet {
    /// 1 for a single trailer, 2 for doubles.
    pub units: u8,
    /// Empty weight of everything behind the tractor, kilograms.
    pub tare_kg: f64,
    /// Legal gross vehicle weight for this set on this route, kilograms.
    pub legal_gvw_kg: f64,
    /// Rear trailer's lateral acceleration over the tractor's in a quick
    /// lane change. 1.0 for a single trailer.
    pub rearward_amplification: f64,
    /// The fixed combination mass the crosswind gust is taken to push,
    /// kilograms (`models::doubles::sway_reference_gross_lb`).
    pub sway_reference_kg: f64,
}

impl Default for TrailerSet {
    fn default() -> Self {
        TrailerSet {
            units: 1,
            tare_kg: TRAILER_TARE_KG,
            legal_gvw_kg: LEGAL_GVW_KG,
            rearward_amplification: 1.0,
            sway_reference_kg: LEGAL_GVW_KG,
        }
    }
}

impl TrailerSet {
    /// The set a trailer program hooks, held to `legal_gvw_lb` on its route.
    pub fn for_trailer(trailer_key: &str, legal_gvw_lb: f64) -> Self {
        TrailerSet {
            units: trailer_units(trailer_key),
            tare_kg: TRAILER_TARE_KG + trailer_set_extra_tare_kg(trailer_key, TRAILER_TARE_KG),
            legal_gvw_kg: legal_gvw_lb * KG_PER_LB,
            rearward_amplification: rearward_amplification(trailer_key),
            sway_reference_kg: sway_reference_gross_lb(trailer_key) * KG_PER_LB,
        }
    }

    /// What a load of this cargo hooks on this route: a set of doubles with
    /// the route's legal gross for doubles freight, the stock single
    /// otherwise. The legal gross reads the states of every city the route
    /// passes (`models::doubles::legal_gvw_lb_for_route`).
    ///
    /// None when the set is not legal on this route at any weight: turnpike
    /// doubles on a route through a state with no recorded LCV cap, or with
    /// no route at all. Callers refuse that route or offer no job.
    pub fn for_cargo_on_route(
        cargo_key: &str,
        world: &World,
        route: Option<&Route>,
    ) -> Option<Self> {
        let Some(trailer_key) = doubles_trailer_for_cargo(cargo_key) else {
            return Some(TrailerSet::default());
        };
        let states: Vec<&str> = route
            .map(|r| {
                r.cities
                    .iter()
                    .map(|key| world.cities.get(key).map_or("", |c| c.state_code.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        legal_gvw_lb_for_route(cargo_key, states)
            .map(|cap_lb| TrailerSet::for_trailer(trailer_key, cap_lb))
    }

    /// What a load of this cargo hooks between two cities, priced on the
    /// routes dispatch will offer for it. Turnpike doubles look at the same
    /// three options the route menu does, keep the ones on the LCV turnpike
    /// allowlist with a recorded cap in every state, and take the lowest cap
    /// among them, so whichever lane is driven the load is legal. None when
    /// no such lane exists. Everything else reads the shortest route.
    pub fn for_cargo_between(
        cargo_key: &str,
        world: &World,
        origin: &str,
        destination: &str,
    ) -> Option<Self> {
        if !cargo_requires_lcv_turnpike(cargo_key) {
            let route = world
                .supported_route(origin, destination, None)
                .ok()
                .flatten();
            return Self::for_cargo_on_route(cargo_key, world, route.as_ref());
        }
        let routes = world.supported_route_options(origin, destination, 3).ok()?;
        filter_lcv_turnpike_routes(&routes)
            .iter()
            .filter_map(|route| Self::for_cargo_on_route(cargo_key, world, Some(route)))
            .min_by(|a, b| a.legal_gvw_kg.total_cmp(&b.legal_gvw_kg))
    }

    /// The set for a trip already under way that [`Self::for_cargo_on_route`]
    /// refuses: turnpike doubles dispatched under the old rules on a lane
    /// through a state with no recorded cap. The trip is grandfathered and
    /// finishes clean (`models::doubles::legacy_trip_legal_gvw_lb`): the
    /// lowest recorded cap among its capped states, or 80,000 lb.
    pub fn legacy_trip_on_route(cargo_key: &str, world: &World, route: Option<&Route>) -> Self {
        if let Some(set) = Self::for_cargo_on_route(cargo_key, world, route) {
            return set;
        }
        let trailer_key = doubles_trailer_for_cargo(cargo_key).unwrap_or("");
        let states: Vec<&str> = route
            .map(|r| {
                r.cities
                    .iter()
                    .map(|key| world.cities.get(key).map_or("", |c| c.state_code.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        TrailerSet::for_trailer(trailer_key, legacy_trip_legal_gvw_lb(states))
    }

    pub fn is_doubles(&self) -> bool {
        self.units >= 2
    }

    /// The legal gross, pounds.
    pub fn legal_gvw_lb(&self) -> f64 {
        self.legal_gvw_kg / KG_PER_LB
    }
}

impl TruckState {
    /// A set of doubles is on the fifth wheel.
    pub fn doubles_hooked(&self) -> bool {
        self.trailer_attached && self.trailer_set.is_doubles()
    }

    /// Reverse is refused with doubles hooked; None when it may go in.
    pub fn doubles_reverse_refusal(&self) -> Option<&'static str> {
        self.doubles_hooked().then_some(DOUBLES_NO_REVERSE_TEXT)
    }

    /// The same refusal as a gearbox answer: not engaged, nothing ground.
    pub(super) fn doubles_reverse_shift_result(&self) -> Option<ShiftResult> {
        self.doubles_reverse_refusal().map(|message| ShiftResult {
            ok: false,
            message: message.to_string(),
            grind: false,
        })
    }

    /// How much harder a gust shoves this set than a loaded one: the same
    /// wind force on less mass is more acceleration (a = F / m). The lane
    /// model's gust is taken as the push on a combination at the program's
    /// fixed reference mass (`TrailerSet::sway_reference_kg`; an ASSUMED
    /// calibration), never the route's cap, so a set sways the same on
    /// every corridor. The ratio is clamped to 1.0 ..=
    /// [`LIGHT_SET_SWAY_MAX`]: a set at or over its reference reads 1.0 (a
    /// heavier set is not made steadier than the reference), and an empty
    /// set reads at most 2.0. 1.0 for a single trailer, whose own drift the
    /// lane model already carries.
    pub fn light_set_wind_mult(&self) -> f64 {
        if !self.doubles_hooked() {
            return 1.0;
        }
        (self.trailer_set.sway_reference_kg / self.gross_mass_kg().max(1.0))
            .clamp(1.0, LIGHT_SET_SWAY_MAX)
    }

    /// The rear trailer's lateral acceleration, g: the tractor's quick-steer
    /// acceleration plus the crosswind gust's shove, both amplified down the
    /// set (rearward amplification), the gust also scaled up for a light set
    /// ([`Self::light_set_wind_mult`]). A single trailer tracks the tractor,
    /// so it reads the steer alone. The amplification never reaches the
    /// tractor's own lane drift: it is a property of the rear trailer.
    pub fn rear_trailer_lateral_g(&self, steer_lateral_g: f64, wind_lateral_g: f64) -> f64 {
        if !self.doubles_hooked() {
            return steer_lateral_g.abs();
        }
        (steer_lateral_g.abs() + wind_lateral_g.abs() * self.light_set_wind_mult())
            * self.trailer_set.rearward_amplification
    }

    /// The swing at which the rear trailer of a set whips, g: the roll
    /// model's warning share of the threshold, with the threshold floored at
    /// the loaded trailer's. A light pup is no harder to whip over than a
    /// loaded one -- empty pups blow over and whip MORE easily -- so a light
    /// load must never raise this.
    pub fn rear_trailer_whip_threshold_g(&self) -> f64 {
        ROLL_WARN_SHARE * self.roll_threshold_g().min(TRUCK_ROLLOVER_G)
    }

    /// How far past [`Self::rear_trailer_whip_threshold_g`] the rear trailer
    /// is swinging, in g; zero or less is fine. Always zero for a single
    /// trailer and below [`REAR_WHIP_MIN_MPH`].
    pub fn rear_trailer_whip_excess_g(&self, steer_lateral_g: f64, wind_lateral_g: f64) -> f64 {
        if !self.doubles_hooked() || self.speed_mph() < REAR_WHIP_MIN_MPH {
            return 0.0;
        }
        self.rear_trailer_lateral_g(steer_lateral_g, wind_lateral_g)
            - self.rear_trailer_whip_threshold_g()
    }

    /// Freight in the rear trailer works against its straps when a quick
    /// steer or a gust whips it past the threshold, at the rate a bend taken
    /// too fast would cost (`update_cargo`), on the rear trailer's share of
    /// the load. Returns whether it whipped this frame.
    pub fn update_rear_trailer(
        &mut self,
        dt: f64,
        steer_lateral_g: f64,
        wind_lateral_g: f64,
    ) -> bool {
        let excess = self.rear_trailer_whip_excess_g(steer_lateral_g, wind_lateral_g);
        if excess <= 0.0 {
            return false;
        }
        if self.cargo_kg > 0.0 {
            self.add_cargo_damage(
                excess
                    * CARGO_CORNER_PCT_PER_G_S
                    * REAR_TRAILER_FREIGHT_SHARE
                    * self.cargo_fragility
                    * dt,
            );
        }
        true
    }

    /// The CAT Scale ticket. A single trailer is broken out by axle group
    /// ([`super::AxleLoads::ticket_text`]). A set of doubles stands on
    /// single axles the three-group split does not model, so the game reads
    /// its gross against this set's legal gross for the route and nothing
    /// else; a real ticket prints each platform, which is a ROADMAP debt.
    pub fn scale_ticket_text(&self) -> String {
        if !self.doubles_hooked() {
            return self.axle_loads().ticket_text();
        }
        let lb = |kg: f64| (kg / KG_PER_LB).round();
        let gross = lb(self.gross_mass_kg());
        let limit = self.trailer_set.legal_gvw_lb().round();
        let verdict = if gross > limit {
            format!(
                "{} pounds over the {} pound gross limit for this set on this route.",
                fmt_grouped(gross - limit, 0),
                fmt_grouped(limit, 0)
            )
        } else {
            format!(
                "Legal under the {} pound gross limit for this set on this route.",
                fmt_grouped(limit, 0)
            )
        };
        format!(
            "Gross {} pounds, both trailers and the converter dolly included. {verdict}",
            fmt_grouped(gross, 0)
        )
    }
}

/// Tractor, the program's trailing units and a full tank for these specs.
/// The doubles counterpart of [`super::combination_tare_kg`].
pub fn combination_tare_for_trailer_kg(specs: &super::TruckSpecs, trailer_key: &str) -> f64 {
    (specs.mass_kg - REFERENCE_CARGO_KG).max(0.0)
        + trailer_set_extra_tare_kg(trailer_key, TRAILER_TARE_KG)
}

#[cfg(test)]
mod tests {
    use super::super::{TruckSpecs, KG_PER_TON, REVERSE_ENGAGE_MAX_MPH};
    use super::*;
    use crate::models::doubles::{
        PUP_TRAILER_TARE_LB, SINGLE_AXLE_DOLLY_TARE_LB, TURNPIKE_DOUBLE_EXTRA_OVER_SINGLE_LB,
    };
    use crate::sim::transmission::REVERSE;

    fn pups() -> TruckState {
        TruckState {
            trailer_set: TrailerSet::for_trailer("double_van", 80_000.0),
            ..TruckState::default()
        }
    }

    fn turnpike(cap_lb: f64) -> TruckState {
        TruckState {
            trailer_set: TrailerSet::for_trailer("turnpike_double", cap_lb),
            ..TruckState::default()
        }
    }

    #[test]
    fn gross_counts_both_trailers_and_the_dolly() {
        let single = TruckState::default();
        let set = pups();
        let extra_lb = (set.tare_kg() - single.tare_kg()) / KG_PER_LB;
        // Two pups and a dolly, less the stock van they replace.
        let want =
            2.0 * PUP_TRAILER_TARE_LB + SINGLE_AXLE_DOLLY_TARE_LB - TRAILER_TARE_KG / KG_PER_LB;
        assert!((extra_lb - want).abs() < 0.01, "{extra_lb} vs {want}");
        let lcv = turnpike(127_400.0);
        let lcv_extra_lb = (lcv.tare_kg() - single.tare_kg()) / KG_PER_LB;
        assert!((lcv_extra_lb - TURNPIKE_DOUBLE_EXTRA_OVER_SINGLE_LB).abs() < 0.01);
        // Same cargo, heavier gross.
        assert!(lcv.gross_mass_kg() > single.gross_mass_kg());
        // The dispatch tare agrees with the live truck at a full tank.
        let specs = TruckSpecs::default();
        let dispatch = combination_tare_for_trailer_kg(&specs, "turnpike_double");
        assert!((dispatch - lcv.tare_kg()).abs() < 1e-6);
        // Dropping the set takes all of it off, not just one van's worth.
        let mut bobtail = turnpike(127_400.0);
        bobtail.trailer_attached = false;
        assert!((bobtail.tare_kg() - (single.tare_kg() - TRAILER_TARE_KG)).abs() < 1e-6);
    }

    #[test]
    fn the_legal_gross_is_the_route_cap() {
        let mut lcv = turnpike(127_400.0);
        // 100,000 lb gross: over 80,000, under the Ohio/Indiana/Mass cap.
        lcv.cargo_kg = 100_000.0 * KG_PER_LB - lcv.tare_kg();
        assert!(!lcv.is_over_legal_gvw());
        assert!(lcv.gross_weight_margin_kg() > 0.0);
        // The same load under Kansas's 120,000 is still legal; at 125,000 lb
        // it is over Kansas but under the Thruway's 143,000.
        let mut ks = turnpike(120_000.0);
        ks.cargo_kg = 125_000.0 * KG_PER_LB - ks.tare_kg();
        assert!(ks.is_over_legal_gvw());
        let mut ny = turnpike(143_000.0);
        ny.cargo_kg = 125_000.0 * KG_PER_LB - ny.tare_kg();
        assert!(!ny.is_over_legal_gvw());
        // Twin 28s at 80,001 lb are over: they never get the turnpike cap.
        let mut staa = pups();
        staa.cargo_kg = 80_001.0 * KG_PER_LB - staa.tare_kg();
        assert!(staa.is_over_legal_gvw());
        assert!(staa.gross_weight_margin_kg() < 0.0);
    }

    #[test]
    fn the_scale_ticket_for_doubles_reads_the_route_limit() {
        let mut lcv = turnpike(127_400.0);
        lcv.cargo_kg = 100_000.0 * KG_PER_LB - lcv.tare_kg();
        let text = lcv.scale_ticket_text();
        assert!(text.starts_with("Gross 100,000 pounds"), "{text}");
        assert!(
            text.contains("Legal under the 127,400 pound gross limit"),
            "{text}"
        );
        lcv.cargo_kg = 128_400.0 * KG_PER_LB - lcv.tare_kg();
        let text = lcv.scale_ticket_text();
        assert!(
            text.contains("1,000 pounds over the 127,400 pound gross limit"),
            "{text}"
        );
        // A single keeps the axle-group ticket.
        let single = TruckState::default();
        assert_eq!(
            single.scale_ticket_text(),
            single.axle_loads().ticket_text()
        );
    }

    #[test]
    fn reverse_is_refused_with_doubles_hooked() {
        let mut set = pups();
        set.velocity_mps = 0.0;
        let result = set.request_gear(REVERSE);
        assert!(!result.ok);
        assert!(!result.grind);
        assert_eq!(result.message, DOUBLES_NO_REVERSE_TEXT);
        assert!(!set.transmission.in_reverse());
        // A single trailer and a bobtail are not refused for doubles (the
        // gearbox may still want the clutch; that is its own answer).
        let mut single = TruckState::default();
        assert!(single.speed_mph() <= REVERSE_ENGAGE_MAX_MPH);
        assert_ne!(
            single.request_gear(REVERSE).message,
            DOUBLES_NO_REVERSE_TEXT
        );
        let mut dropped = pups();
        dropped.trailer_attached = false;
        assert_ne!(
            dropped.request_gear(REVERSE).message,
            DOUBLES_NO_REVERSE_TEXT
        );
    }

    #[test]
    fn a_quick_steer_swings_the_rear_pup_harder() {
        let mut set = pups();
        set.cargo_kg = REFERENCE_CARGO_KG;
        set.velocity_mps = 60.0 / 2.23694;
        let single = TruckState {
            velocity_mps: set.velocity_mps,
            ..TruckState::default()
        };
        // The driver's steering cap at highway speed is 0.2 g on the tractor.
        let steer_g = 0.2;
        assert!((set.rear_trailer_lateral_g(steer_g, 0.0) - 0.34).abs() < 1e-9);
        assert_eq!(single.rear_trailer_lateral_g(steer_g, 0.0), steer_g);
        // Full load: the pup passes the threshold a single never reaches.
        assert!(set.rear_trailer_whip_excess_g(steer_g, 0.0) > 0.0);
        assert!(single.rear_trailer_whip_excess_g(steer_g, 0.0) <= 0.0);
        // A gentle lane change is fine even on pups.
        assert!(set.rear_trailer_whip_excess_g(0.1, 0.0) <= 0.0);
        // Turnpike doubles amplify less: the same steer stays under.
        let mut lcv = turnpike(127_400.0);
        lcv.cargo_kg = REFERENCE_CARGO_KG;
        lcv.velocity_mps = set.velocity_mps;
        assert!(lcv.rear_trailer_whip_excess_g(steer_g, 0.0) <= 0.0);
        // At street speed nothing is said.
        set.velocity_mps = 25.0 / 2.23694;
        assert_eq!(set.rear_trailer_whip_excess_g(steer_g, 0.0), 0.0);
    }

    #[test]
    fn an_empty_set_whips_no_later_than_a_loaded_one() {
        let speed = 60.0 / 2.23694;
        let mut loaded = pups();
        loaded.cargo_kg = REFERENCE_CARGO_KG;
        loaded.velocity_mps = speed;
        let mut empty = pups();
        empty.cargo_kg = 0.0;
        empty.velocity_mps = speed;
        // A light load never raises the threshold.
        assert!(empty.rear_trailer_whip_threshold_g() <= loaded.rear_trailer_whip_threshold_g());
        // The smallest steer that whips each set, found by stepping up.
        let first_whip = |t: &TruckState| {
            (1..=400)
                .map(|n| f64::from(n) * 0.001)
                .find(|g| t.rear_trailer_whip_excess_g(*g, 0.0) > 0.0)
                .expect("some steer whips a set of doubles")
        };
        assert!(first_whip(&empty) <= first_whip(&loaded));
        // In a crosswind the light set is pushed harder, so it whips on a
        // smaller steer than the loaded one.
        let gust = 0.03;
        assert!(empty.light_set_wind_mult() > loaded.light_set_wind_mult());
        let first_whip_in_wind = |t: &TruckState| {
            (1..=400)
                .map(|n| f64::from(n) * 0.001)
                .find(|g| t.rear_trailer_whip_excess_g(*g, gust) > 0.0)
                .expect("some steer whips a set of doubles")
        };
        assert!(first_whip_in_wind(&empty) < first_whip_in_wind(&loaded));
    }

    #[test]
    fn a_whip_costs_the_rear_trailer_freight_and_a_single_nothing() {
        let mut set = pups();
        set.cargo_kg = REFERENCE_CARGO_KG;
        set.velocity_mps = 60.0 / 2.23694;
        assert!(set.update_rear_trailer(1.0, 0.2, 0.0));
        assert!(set.cargo_damage_pct > 0.0);
        let mut single = TruckState {
            velocity_mps: set.velocity_mps,
            ..TruckState::default()
        };
        assert!(!single.update_rear_trailer(1.0, 0.2, 0.0));
        assert_eq!(single.cargo_damage_pct, 0.0);
    }

    #[test]
    fn a_crosswind_sways_the_rear_trailer_not_the_tractor() {
        // The gust reaches the rear trailer amplified; a single trailer,
        // tracking the tractor, reads none of it here -- its drift is the
        // lane model's, unchanged for doubles.
        let set = pups();
        let single = TruckState::default();
        let gust = 0.05;
        assert!(set.rear_trailer_lateral_g(0.0, gust) >= gust * 1.7 - 1e-12);
        assert_eq!(single.rear_trailer_lateral_g(0.0, gust), 0.0);
        assert_eq!(single.light_set_wind_mult(), 1.0);
    }

    #[test]
    fn the_same_set_sways_the_same_under_any_turnpike_cap() {
        // The gust's reference mass is fixed per program, not the route's
        // cap: the same load sways identically on a NY (143,000 lb) lane and
        // a KS (120,000 lb) lane.
        let speed = 60.0 / 2.23694;
        let mut ny = turnpike(143_000.0);
        let mut ks = turnpike(120_000.0);
        for set in [&mut ny, &mut ks] {
            set.cargo_kg = 20.0 * KG_PER_TON;
            set.velocity_mps = speed;
        }
        assert_eq!(ny.light_set_wind_mult(), ks.light_set_wind_mult());
        assert_eq!(
            ny.rear_trailer_lateral_g(0.1, 0.04),
            ks.rear_trailer_lateral_g(0.1, 0.04)
        );
        assert_eq!(
            ny.rear_trailer_whip_excess_g(0.1, 0.04),
            ks.rear_trailer_whip_excess_g(0.1, 0.04)
        );
        assert_eq!(
            ny.trailer_set.sway_reference_kg,
            127_400.0 * KG_PER_LB,
            "turnpike doubles sway against one fixed reference"
        );
        assert_eq!(pups().trailer_set.sway_reference_kg, 80_000.0 * KG_PER_LB);
    }

    #[test]
    fn an_empty_set_sways_at_most_twice_a_loaded_one() {
        for mut set in [pups(), turnpike(143_000.0), turnpike(120_000.0)] {
            set.cargo_kg = 0.0;
            let empty = set.light_set_wind_mult();
            assert!(empty > 1.0, "an empty set takes the gust harder");
            assert!(empty <= LIGHT_SET_SWAY_MAX + 1e-12, "{empty}");
            assert!((LIGHT_SET_SWAY_MAX - 2.0).abs() < 1e-12);
            // At or over the reference mass the factor is 1.0, never less.
            set.cargo_kg = 60.0 * KG_PER_TON;
            assert_eq!(set.light_set_wind_mult(), 1.0);
        }
    }
}
