//! The owner-operator buy-in's tractor list and prices.
//!
//! The buy-in buys a sleeper tractor the driver picks, at its catalog price,
//! with `OWNER_OPERATOR_WORKING_CAPITAL` left in the bank. Day cabs are left
//! out (a leased-on owner-operator runs over the road), and so is anything
//! the catalog prices at nothing (the standard rig).

use crate::models::business::OWNER_OPERATOR_WORKING_CAPITAL;
use crate::models::trucks::{TruckModel, CAB_SLEEPER, TRUCK_CATALOG};

/// Every tractor the buy-in offers, cheapest first. Ties keep catalog order.
pub fn buy_in_tractors() -> Vec<&'static TruckModel> {
    let mut list: Vec<&'static TruckModel> = TRUCK_CATALOG
        .values()
        .filter(|m| m.cab == CAB_SLEEPER && m.price > 0.0)
        .collect();
    list.sort_by(|a, b| a.price.total_cmp(&b.price));
    list
}

/// The price of the cheapest tractor the buy-in offers.
pub fn cheapest_buy_in_price() -> f64 {
    buy_in_tractors().first().map_or(0.0, |m| m.price)
}

/// Cash a driver needs on hand to buy `price` worth of tractor and keep the
/// working capital in the bank.
pub fn buy_in_cash_needed(price: f64) -> f64 {
    price + OWNER_OPERATOR_WORKING_CAPITAL
}

/// Whether `money` buys this tractor with the working capital left over.
pub fn can_afford_buy_in(money: f64, model: &TruckModel) -> bool {
    money >= buy_in_cash_needed(model.price)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::trucks::CAB_DAY;

    #[test]
    fn the_list_is_priced_sleepers_cheapest_first() {
        let list = buy_in_tractors();
        assert!(!list.is_empty());
        assert!(list.iter().all(|m| m.cab == CAB_SLEEPER && m.price > 0.0));
        assert!(list.windows(2).all(|w| w[0].price <= w[1].price));
        assert!(list.iter().any(|m| m.key == "heavy_hauler"));
        assert!(!list.iter().any(|m| m.key == "rig"));
        assert!(!list.iter().any(|m| m.cab == CAB_DAY));
        assert_eq!(cheapest_buy_in_price(), list[0].price);
    }

    #[test]
    fn affording_a_tractor_keeps_the_working_capital() {
        let first = buy_in_tractors()[0];
        assert!(can_afford_buy_in(first.price + 10_000.0, first));
        assert!(!can_afford_buy_in(first.price + 9_999.0, first));
    }
}
