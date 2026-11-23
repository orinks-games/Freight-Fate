//! The truck lot at a road stop, as the truck pulls in.
//!
//! A full lot turns away the overnight stay and nothing else. The pumps,
//! the food counter, a 30-minute break and a save at the stop all fit in the
//! time a driver waits for a space to turn over, so they stay on the menu.
//! Before this was one enum, a full lot opened its own menu with only the
//! pumps and the ways to sleep elsewhere: a stop planned and announced as a
//! break stop had no break row and no save row (player report, 2026-11-23),
//! while a co-driver at the same lot took the break all the same.

use crate::sim::hos;

/// What the lot has room for, decided once as the truck pulls in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopLot {
    /// Room for any stay the stop offers.
    Open,
    /// The overnight spaces are taken: no sleep here, every short stay
    /// still open.
    FullTonight,
}

/// The stop actions that need an overnight space.
const OVERNIGHT_ACTIONS: &[&str] = &["park", "sleep"];

impl StopLot {
    /// The lot at a stop whose actions are `actions`, reached at
    /// `local_hour`. Only a stop that offers sleep can be full: a fuel
    /// station has no overnight lot to fill.
    pub fn on_arrival(
        actions: &[String],
        trip_seed: i64,
        stop_mi: f64,
        local_hour: f64,
        spaces: i64,
    ) -> Self {
        let sleeps = actions.iter().any(|action| action == "sleep");
        if sleeps && hos::parking_is_full(trip_seed, stop_mi, local_hour, spaces) {
            StopLot::FullTonight
        } else {
            StopLot::Open
        }
    }

    /// Whether the stop menu offers `action`, one of the stop's own
    /// `actions`, with the lot like this.
    pub fn offers(self, actions: &[String], action: &str) -> bool {
        if !actions.iter().any(|a| a == action) {
            return false;
        }
        match self {
            StopLot::Open => true,
            StopLot::FullTonight => !OVERNIGHT_ACTIONS.contains(&action),
        }
    }

    pub fn is_full(self) -> bool {
        self == StopLot::FullTonight
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actions(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn offer_table() {
        let rest_area = actions(&["park", "save", "break", "sleep"]);
        let travel_center = actions(&["park", "save", "fuel", "food", "break", "sleep"]);
        #[rustfmt::skip]
        let table: &[(StopLot, &Vec<String>, &str, bool)] = &[
            (StopLot::Open,        &rest_area,     "break", true),
            (StopLot::Open,        &rest_area,     "save",  true),
            (StopLot::Open,        &rest_area,     "sleep", true),
            (StopLot::Open,        &rest_area,     "fuel",  false),
            (StopLot::FullTonight, &rest_area,     "break", true),
            (StopLot::FullTonight, &rest_area,     "save",  true),
            (StopLot::FullTonight, &rest_area,     "sleep", false),
            (StopLot::FullTonight, &rest_area,     "park",  false),
            (StopLot::FullTonight, &travel_center, "fuel",  true),
            (StopLot::FullTonight, &travel_center, "food",  true),
            (StopLot::FullTonight, &travel_center, "break", true),
            (StopLot::FullTonight, &travel_center, "save",  true),
            (StopLot::FullTonight, &travel_center, "sleep", false),
        ];
        for (lot, stop, action, offered) in table {
            assert_eq!(lot.offers(stop, action), *offered, "{lot:?} {action}");
        }
    }

    #[test]
    fn only_a_stop_with_sleep_can_be_full() {
        let fuel_station = actions(&["park", "save", "fuel", "break"]);
        let rest_area = actions(&["park", "save", "break", "sleep"]);
        // Find a seed whose lot is full at 2 AM, then check the fuel
        // station on the same roll stays open.
        let seed = (0..500)
            .find(|seed| StopLot::on_arrival(&rest_area, *seed, 40.0, 2.0, 0).is_full())
            .expect("some seed fills the lot at 2 AM");
        assert_eq!(
            StopLot::on_arrival(&fuel_station, seed, 40.0, 2.0, 0),
            StopLot::Open
        );
        // Daytime lots are never full.
        assert_eq!(
            StopLot::on_arrival(&rest_area, seed, 40.0, 12.0, 0),
            StopLot::Open
        );
    }
}
