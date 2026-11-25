//! The board's side of the lane law (`cross_border` has the table): which
//! cities a carrier may send a load to, what each lane is, and freight that
//! stays in the state it ships from.

use super::{Candidate, JobBoard};
use crate::models::jobs::cross_border::{self, CarrierBase, LaneKind};
use crate::models::jobs::{CargoType, MIN_JOB_DISTANCE_MI};

/// A location trait: a load shipped from this site ends in the site's own
/// state or province. The Port of Alaska and the Alaska distribution yards
/// are where freight that came north by sea is handed to Alaska carriers;
/// it does not leave the state by road again.
pub const SHIPS_IN_STATE: &str = "ships_in_state";

impl<'w> JobBoard<'w> {
    fn country_of(&self, key: &str) -> &str {
        self.world
            .cities
            .get(key)
            .map(|c| c.country.as_str())
            .unwrap_or("US")
    }

    /// Whether the supported road between two cities in one country leaves
    /// that country on the way, from the shared cache.
    pub(crate) fn route_enters_other_country(&self, origin: &str, destination: &str) -> bool {
        self.candidate_set(origin).through.contains(destination)
    }

    /// The lane between two cities for a carrier based in `base`.
    pub(crate) fn lane_for_base(
        &self,
        base: CarrierBase,
        origin: &str,
        destination: &str,
    ) -> Option<LaneKind> {
        let origin_country = self.country_of(origin);
        let destination_country = self.country_of(destination);
        let through = origin_country.eq_ignore_ascii_case(destination_country)
            && self.route_enters_other_country(origin, destination);
        cross_border::lane_kind(base, origin_country, destination_country, through)
    }

    /// The lane between two cities for this carrier.
    pub(crate) fn lane(
        &self,
        carrier_key: &str,
        origin: &str,
        destination: &str,
    ) -> Option<LaneKind> {
        self.lane_for_base(cross_border::carrier_base(carrier_key), origin, destination)
    }

    /// Whether this carrier may haul a load between the two cities.
    pub(crate) fn carrier_may_haul(
        &self,
        carrier_key: &str,
        origin: &str,
        destination: &str,
    ) -> bool {
        cross_border::lane_permitted(
            self.lane(carrier_key, origin, destination),
            cross_border::carrier_runs_to_canada(carrier_key),
        )
    }

    /// The candidates from `city` on lanes this carrier may haul.
    pub(crate) fn candidates_for(&self, city: &str, carrier_key: &str) -> Vec<Candidate> {
        let set = self.candidate_set(city);
        let base = cross_border::carrier_base(carrier_key);
        let authority = cross_border::carrier_runs_to_canada(carrier_key);
        let origin_country = self.country_of(city);
        set.all
            .iter()
            .filter(|c| {
                let destination_country = self.country_of(&c.0);
                let through = set.through.contains(&c.0);
                cross_border::lane_permitted(
                    cross_border::lane_kind(base, origin_country, destination_country, through),
                    authority,
                )
            })
            .cloned()
            .collect()
    }

    /// Whether this cargo may ride the lane between the two cities for this
    /// carrier, before its material is drawn.
    pub(crate) fn cargo_fits(
        &self,
        carrier_key: &str,
        origin: &str,
        destination: &str,
        cargo: &CargoType,
    ) -> bool {
        self.lane(carrier_key, origin, destination)
            .is_some_and(|kind| cross_border::cargo_type_fits_lane(kind, cargo))
    }

    /// Whether two cities are in the same state or province.
    pub(crate) fn same_state(&self, a: &str, b: &str) -> bool {
        let state = |key: &str| {
            self.world
                .cities
                .get(key)
                .map(|c| (c.country.as_str(), c.state_code.as_str()))
        };
        matches!((state(a), state(b)), (Some(x), Some(y)) if x == y)
    }

    /// The destination spread for a `ships_in_state` site: the in-state part
    /// of the candidates, drawn the way the board draws its own spread
    /// (inside the cap, else the nearest few), so a board at a marked site
    /// is as full as any other.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn in_state_spread(
        &mut self,
        city: &str,
        candidates: &[Candidate],
        cap: f64,
        level: i64,
        count: usize,
        carrier_key: &str,
        came_from: Option<&str>,
    ) -> Vec<Candidate> {
        let in_state: Vec<Candidate> = candidates
            .iter()
            .filter(|c| c.1 >= MIN_JOB_DISTANCE_MI && self.same_state(city, &c.0))
            .cloned()
            .collect();
        let mut reachable: Vec<Candidate> =
            in_state.iter().filter(|c| c.1 <= cap).cloned().collect();
        if reachable.is_empty() {
            let mut sorted = in_state;
            sorted.sort_by(|a, b| a.1.total_cmp(&b.1));
            sorted.truncate(4);
            reachable = sorted;
        }
        if reachable.is_empty() {
            return reachable;
        }
        self.spread_destinations(city, &reachable, level, count, carrier_key, came_from)
    }
}
