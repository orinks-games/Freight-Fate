//! The agricultural hours-of-service exemption, as it applies to livestock.
//!
//! Read: hours of service do not apply to a driver hauling livestock within
//! 150 air miles of the source (49 CFR 395.1(k)(1)), nor within 150 air
//! miles of the final destination (395.1(k)(4), added by section 23018 of
//! the 2021 infrastructure law). FMCSA: the rules begin at the time and place
//! the truck passes the radius, and time inside it does not count toward the
//! daily and weekly limits.
//!
//! Assumed: the exemption covers the loaded trip only, and the planting and
//! harvesting condition the rule opens with is treated as met all year.
//! Freezing the 14-hour window while exempt is a simplification of the real
//! rule.

use serde::{Deserialize, Serialize};

/// Derived: an air mile is a nautical mile, so 150 air miles is
/// 150 x 1.15078 = 172.6 statute miles in a straight line.
pub const EXEMPT_RADIUS_MI: f64 = 172.6;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exemption {
    NotInForce,
    /// Within 150 air miles of the pickup.
    NearPickup,
    /// Between the circles: the ordinary rules count.
    Counting,
    /// Within 150 air miles of the delivery.
    NearDelivery,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExemptionEvent {
    /// The cattle are aboard.
    Loaded,
    /// Straight-line statute miles from the pickup and to the delivery.
    Position {
        from_pickup_mi: f64,
        to_delivery_mi: f64,
    },
    /// The unloading started.
    Unloading,
}

impl Exemption {
    pub fn next(self, event: ExemptionEvent) -> Exemption {
        use Exemption::*;
        match (self, event) {
            (_, ExemptionEvent::Unloading) => NotInForce,
            (NotInForce, ExemptionEvent::Loaded) => NearPickup,
            (
                NearPickup,
                ExemptionEvent::Position {
                    from_pickup_mi,
                    to_delivery_mi,
                },
            ) if from_pickup_mi > EXEMPT_RADIUS_MI => {
                // Where the circles overlap the truck passes straight from
                // one to the other, and nothing changes for the driver.
                if to_delivery_mi <= EXEMPT_RADIUS_MI {
                    NearDelivery
                } else {
                    Counting
                }
            }
            // Counting never goes back to near the pickup: a road that bends
            // back inside the radius does not stop the clock again.
            (Counting, ExemptionEvent::Position { to_delivery_mi, .. })
                if to_delivery_mi <= EXEMPT_RADIUS_MI =>
            {
                NearDelivery
            }
            (state, _) => state,
        }
    }

    /// The driver's hours are not counting.
    pub fn exempt(self) -> bool {
        matches!(self, Exemption::NearPickup | Exemption::NearDelivery)
    }

    pub fn key(self) -> &'static str {
        match self {
            Exemption::NotInForce => "not_in_force",
            Exemption::NearPickup => "near_pickup",
            Exemption::Counting => "counting",
            Exemption::NearDelivery => "near_delivery",
        }
    }

    pub fn from_key(key: &str) -> Option<Exemption> {
        [
            Exemption::NotInForce,
            Exemption::NearPickup,
            Exemption::Counting,
            Exemption::NearDelivery,
        ]
        .into_iter()
        .find(|state| state.key() == key)
    }
}

#[cfg(test)]
mod tests {
    use super::Exemption::*;
    use super::*;

    fn at(from_pickup_mi: f64, to_delivery_mi: f64) -> ExemptionEvent {
        ExemptionEvent::Position {
            from_pickup_mi,
            to_delivery_mi,
        }
    }

    #[test]
    fn the_exemption_moves_only_along_its_table() {
        let table = [
            (NotInForce, ExemptionEvent::Loaded, NearPickup),
            (NotInForce, at(500.0, 10.0), NotInForce),
            (NearPickup, at(100.0, 600.0), NearPickup),
            (NearPickup, at(172.6, 600.0), NearPickup),
            (NearPickup, at(173.0, 600.0), Counting),
            // The circles overlap: straight across, no counting.
            (NearPickup, at(173.0, 150.0), NearDelivery),
            (Counting, at(400.0, 300.0), Counting),
            (Counting, at(100.0, 300.0), Counting),
            (Counting, at(600.0, 172.0), NearDelivery),
            (NearDelivery, at(600.0, 400.0), NearDelivery),
            (NearPickup, ExemptionEvent::Loaded, NearPickup),
            (Counting, ExemptionEvent::Loaded, Counting),
            // Unloading ends it from anywhere.
            (NearPickup, ExemptionEvent::Unloading, NotInForce),
            (Counting, ExemptionEvent::Unloading, NotInForce),
            (NearDelivery, ExemptionEvent::Unloading, NotInForce),
        ];
        for (from, event, to) in table {
            assert_eq!(from.next(event), to, "{from:?} on {event:?}");
        }
    }

    #[test]
    fn only_the_two_circles_stop_the_hours() {
        assert!(!NotInForce.exempt());
        assert!(NearPickup.exempt());
        assert!(!Counting.exempt());
        assert!(NearDelivery.exempt());
    }

    #[test]
    fn the_radius_is_150_nautical_miles_in_statute_miles() {
        assert!((150.0 * 1.150_779 - EXEMPT_RADIUS_MI).abs() < 0.05);
    }
}
