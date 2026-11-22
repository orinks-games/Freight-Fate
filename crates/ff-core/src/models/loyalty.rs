//! Truck stop loyalty programs: points, rewards, and redemption (port of
//! `freight_fate/models/loyalty.py`).
//!
//! Real-world truck stop loyalty programs like Pilot Pro Rewards and TA UltraONE
//! give drivers points per gallon fueled, with redemption options for showers,
//! parking, food, and other services. This system mimics that behavior for
//! gameplay depth and strategic fueling decisions.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::data::amenities::{classify_brand, BRANDS};
use crate::models::save_migration::{json_f64, json_i64};
use crate::pyfmt::fmt_f;

/// Point earning rates (points per gallon) by brand tier.
pub const POINTS_PER_GALLON: &[(&str, f64)] = &[
    ("travel_center", 1.0), // Major chains like Pilot, TA, Love's
    ("landmark", 1.5),      // Premium stops like Big Buck's
    ("generic", 0.5),       // Unbranded stops
];

/// Points required for rewards.
pub const REWARD_COSTS: &[(&str, i64)] = &[
    ("shower", 50),  // Points needed for a free shower
    ("parking", 30), // Points needed for parking discount
    ("food", 25),    // Points needed for food discount
    ("laundry", 40), // Points needed for laundry discount
];

/// Shower credits earned per fueling threshold (gallons).
pub const SHOWER_CREDIT_GALLONS: f64 = 50.0;

const GENERIC_RATE: f64 = 0.5;

fn points_per_gallon(tier: &str) -> Option<f64> {
    POINTS_PER_GALLON
        .iter()
        .find(|(key, _)| *key == tier)
        .map(|(_, rate)| *rate)
}

/// `REWARD_COSTS.get(reward_type, 0)`.
pub fn reward_cost(reward_type: &str) -> i64 {
    REWARD_COSTS
        .iter()
        .find(|(key, _)| *key == reward_type)
        .map(|(_, cost)| *cost)
        .unwrap_or(0)
}

/// One recorded fueling session (the dict Python appends to
/// `fueling_history`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct FuelingSession {
    pub gallons: f64,
    pub points_earned: f64,
    pub brand_key: Option<String>,
    pub stop_name: String,
    pub location: String,
}

/// What [`LoyaltyAccount::add_fueling`] reports back.
#[derive(Debug, Clone, PartialEq)]
pub struct FuelingResult {
    pub points_earned: f64,
    /// The balance after the session. Python omits this key when no gallons
    /// were pumped; here it is simply the unchanged balance.
    pub total_points: f64,
    pub rewards: Vec<String>,
}

/// What [`LoyaltyAccount::redeem_reward`] reports back.
#[derive(Debug, Clone, PartialEq)]
pub struct RedeemResult {
    pub success: bool,
    pub points_remaining: f64,
    /// `"insufficient_points"` on failure.
    pub reason: Option<String>,
    pub reward_type: Option<String>,
    pub points_spent: Option<i64>,
}

/// A driver's loyalty program account across all truck stop brands.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LoyaltyAccount {
    pub total_points: f64,
    pub total_gallons_fueled: f64,
    pub shower_credits: i64,
    /// Points per brand.
    pub brand_points: IndexMap<String, f64>,
    /// Track fueling sessions.
    pub fueling_history: Vec<FuelingSession>,
}

impl LoyaltyAccount {
    pub fn new() -> Self {
        Self::default()
    }

    /// `LoyaltyAccount.to_dict()`: the save shape, as JSON.
    pub fn to_dict(&self) -> Value {
        serde_json::to_value(self).expect("a loyalty account serialises")
    }

    /// `LoyaltyAccount.from_dict(data)`: anything that is not an object is a
    /// fresh account; numeric fields coerce as `float()` / `int()` would.
    pub fn from_dict(data: &Value) -> Self {
        let Some(map) = data.as_object() else {
            return Self::new();
        };
        let brand_points = map
            .get("brand_points")
            .and_then(Value::as_object)
            .map(|obj| {
                obj.iter()
                    .map(|(k, v)| (k.clone(), json_f64(Some(v), 0.0)))
                    .collect()
            })
            .unwrap_or_default();
        let fueling_history = map
            .get("fueling_history")
            .and_then(Value::as_array)
            .map(|items| {
                items
                    .iter()
                    .map(|item| serde_json::from_value(item.clone()).unwrap_or_default())
                    .collect()
            })
            .unwrap_or_default();
        Self {
            total_points: json_f64(map.get("total_points"), 0.0),
            total_gallons_fueled: json_f64(map.get("total_gallons_fueled"), 0.0),
            shower_credits: json_i64(map.get("shower_credits"), 0),
            brand_points,
            fueling_history,
        }
    }

    /// Record a fueling session and award loyalty points.
    ///
    /// Returns a summary of points earned and any rewards unlocked.
    /// `brand_key` of `None` or `""` asks the stop name to identify the brand.
    pub fn add_fueling(
        &mut self,
        gallons: f64,
        brand_key: Option<&str>,
        stop_name: &str,
        location: &str,
    ) -> FuelingResult {
        if gallons <= 0.0 {
            return FuelingResult {
                points_earned: 0.0,
                total_points: self.total_points,
                rewards: Vec::new(),
            };
        }

        // Determine point rate based on brand
        let mut rate = points_per_gallon("generic").unwrap_or(GENERIC_RATE);
        let mut brand_key: Option<String> = brand_key.map(str::to_string);

        let key_given = brand_key.as_deref().is_some_and(|key| !key.is_empty());
        if !key_given && !stop_name.is_empty() {
            // Try to detect brand from stop name if brand_key not provided
            if let Some(brand) = classify_brand(stop_name) {
                brand_key = Some(brand.key.to_string());
                rate = points_per_gallon(brand.tier).unwrap_or(GENERIC_RATE);
            }
        } else if key_given {
            // If brand_key is provided, look up the tier
            let key = brand_key.as_deref().unwrap_or_default();
            if let Some(brand) = BRANDS.iter().find(|brand| brand.key == key) {
                rate = points_per_gallon(brand.tier).unwrap_or(GENERIC_RATE);
            }
        }

        let points_earned = gallons * rate;
        self.total_points += points_earned;
        self.total_gallons_fueled += gallons;

        // Track brand-specific points
        if let Some(key) = brand_key.as_deref().filter(|key| !key.is_empty()) {
            *self.brand_points.entry(key.to_string()).or_insert(0.0) += points_earned;
        }

        // Check for shower credits
        let mut rewards_unlocked = Vec::new();
        if gallons >= SHOWER_CREDIT_GALLONS {
            self.shower_credits += 1;
            rewards_unlocked.push("shower_credit".to_string());
        }

        // Record fueling session
        self.fueling_history.push(FuelingSession {
            gallons,
            points_earned,
            brand_key,
            stop_name: stop_name.to_string(),
            location: location.to_string(),
        });

        FuelingResult {
            points_earned,
            total_points: self.total_points,
            rewards: rewards_unlocked,
        }
    }

    /// Check if the driver has enough points for a reward.
    pub fn can_redeem(&self, reward_type: &str) -> bool {
        self.total_points >= reward_cost(reward_type) as f64
    }

    /// Redeem points for a reward.
    ///
    /// Returns success status and updated point balance.
    pub fn redeem_reward(&mut self, reward_type: &str) -> RedeemResult {
        let cost = reward_cost(reward_type);
        if !self.can_redeem(reward_type) {
            return RedeemResult {
                success: false,
                points_remaining: self.total_points,
                reason: Some("insufficient_points".to_string()),
                reward_type: None,
                points_spent: None,
            };
        }
        self.total_points -= cost as f64;
        RedeemResult {
            success: true,
            points_remaining: self.total_points,
            reason: None,
            reward_type: Some(reward_type.to_string()),
            points_spent: Some(cost),
        }
    }

    /// Use a shower credit if available.
    pub fn use_shower_credit(&mut self) -> bool {
        if self.shower_credits > 0 {
            self.shower_credits -= 1;
            return true;
        }
        false
    }

    /// Generate a spoken summary of loyalty status.
    pub fn summary(&self) -> String {
        format!(
            "Loyalty points: {}. Total gallons fueled: {}. Shower credits available: {}.",
            fmt_f(self.total_points, 0),
            fmt_f(self.total_gallons_fueled, 0),
            self.shower_credits
        )
    }
}

/// Generate spoken text for loyalty earnings after fueling.
pub fn loyalty_earnings_text(gallons: f64, points_earned: f64, rewards: &[String]) -> String {
    loyalty_earnings_text_in(gallons, points_earned, rewards, None)
}

/// [`loyalty_earnings_text`] with the fuel in the player's units:
/// `volume` is the amount as spoken ("341 liters"); `None` keeps gallons.
pub fn loyalty_earnings_text_in(
    gallons: f64,
    points_earned: f64,
    rewards: &[String],
    volume: Option<&str>,
) -> String {
    let mut parts = vec![match volume {
        Some(volume) => format!("{volume} fueled"),
        None => format!("{} gallons fueled", fmt_f(gallons, 0)),
    }];
    if points_earned > 0.0 {
        parts.push(format!("{} loyalty points earned", fmt_f(points_earned, 0)));
    }
    if rewards.iter().any(|reward| reward == "shower_credit") {
        parts.push("shower credit earned".to_string());
    }
    format!("{}.", parts.join(", "))
}

/// Generate spoken text for reward cost.
pub fn reward_cost_text(reward_type: &str) -> String {
    let cost = reward_cost(reward_type);
    let label = match reward_type {
        "shower" => "free shower",
        "parking" => "parking discount",
        "food" => "food discount",
        "laundry" => "laundry discount",
        other => other,
    };
    format!("{label} costs {cost} loyalty points.")
}

#[cfg(test)]
mod tests {
    //! Ported from `tests/test_loyalty.py` and `tests/test_loyalty_integration.py`.

    use super::*;

    fn fuel(account: &mut LoyaltyAccount, gallons: f64, stop_name: &str) -> FuelingResult {
        account.add_fueling(gallons, None, stop_name, "")
    }

    #[test]
    fn test_new_account_starts_empty() {
        let account = LoyaltyAccount::new();
        assert_eq!(account.total_points, 0.0);
        assert_eq!(account.total_gallons_fueled, 0.0);
        assert_eq!(account.shower_credits, 0);
        assert!(account.brand_points.is_empty());
        assert!(account.fueling_history.is_empty());
    }

    #[test]
    fn test_fueling_awards_points() {
        let mut account = LoyaltyAccount::new();
        let result = fuel(&mut account, 50.0, "Pilot Travel Center");
        assert_eq!(result.points_earned, 50.0); // 1 point per gallon for travel centers
        assert_eq!(account.total_points, 50.0);
        assert_eq!(account.total_gallons_fueled, 50.0);
    }

    #[test]
    fn test_landmark_brand_awards_bonus_points() {
        let mut account = LoyaltyAccount::new();
        let result = fuel(&mut account, 50.0, "Big Buck's Travel Center");
        assert_eq!(result.points_earned, 75.0); // 1.5 points per gallon for landmarks
        assert_eq!(account.total_points, 75.0);
    }

    #[test]
    fn test_generic_stop_awards_half_points() {
        let mut account = LoyaltyAccount::new();
        let result = fuel(&mut account, 50.0, "Downtown Fuel Mart");
        assert_eq!(result.points_earned, 25.0); // 0.5 points per gallon for generic
        assert_eq!(account.total_points, 25.0);
    }

    #[test]
    fn test_shower_credit_at_threshold() {
        let mut account = LoyaltyAccount::new();
        let result = fuel(&mut account, 50.0, "Pilot Travel Center");
        assert!(result.rewards.iter().any(|r| r == "shower_credit"));
        assert_eq!(account.shower_credits, 1);
    }

    #[test]
    fn test_no_shower_credit_below_threshold() {
        let mut account = LoyaltyAccount::new();
        let result = fuel(&mut account, 49.0, "Pilot Travel Center");
        assert!(!result.rewards.iter().any(|r| r == "shower_credit"));
        assert_eq!(account.shower_credits, 0);
    }

    #[test]
    fn test_multiple_shower_credits() {
        let mut account = LoyaltyAccount::new();
        fuel(&mut account, 50.0, "Pilot Travel Center");
        fuel(&mut account, 60.0, "Flying J Travel Center");
        assert_eq!(account.shower_credits, 2);
    }

    #[test]
    fn test_brand_specific_points_tracking() {
        let mut account = LoyaltyAccount::new();
        fuel(&mut account, 50.0, "Pilot Travel Center");
        fuel(&mut account, 30.0, "Love's Travel Stop");
        assert_eq!(account.brand_points["pilot"], 50.0);
        assert_eq!(account.brand_points["loves"], 30.0);
    }

    #[test]
    fn test_fueling_history_recorded() {
        let mut account = LoyaltyAccount::new();
        account.add_fueling(50.0, None, "Pilot Travel Center", "Springfield, IL");
        assert_eq!(account.fueling_history.len(), 1);
        let entry = &account.fueling_history[0];
        assert_eq!(entry.gallons, 50.0);
        assert_eq!(entry.brand_key.as_deref(), Some("pilot")); // detected from the stop name
        assert_eq!(entry.stop_name, "Pilot Travel Center");
        assert_eq!(entry.location, "Springfield, IL");
    }

    #[test]
    fn test_can_redeem_checks_point_balance() {
        let mut account = LoyaltyAccount::new();
        fuel(&mut account, 50.0, "Pilot Travel Center");
        assert!(account.can_redeem("shower")); // 50 points needed, have 50
        assert!(account.can_redeem("parking")); // 30 points needed, have 50
        assert!(account.can_redeem("food")); // 25 points needed, have 50
    }

    #[test]
    fn test_redeem_reward_deducts_points() {
        let mut account = LoyaltyAccount::new();
        fuel(&mut account, 50.0, "Pilot Travel Center");
        let result = account.redeem_reward("shower");
        assert!(result.success);
        assert_eq!(result.points_spent, Some(50));
        assert_eq!(account.total_points, 0.0);
    }

    #[test]
    fn test_redeem_insufficient_points_fails() {
        let mut account = LoyaltyAccount::new();
        fuel(&mut account, 20.0, "Independent Truck Stop"); // Generic stop = 0.5 rate
        let result = account.redeem_reward("shower");
        assert!(!result.success);
        assert_eq!(result.reason.as_deref(), Some("insufficient_points"));
        assert_eq!(account.total_points, 10.0); // Points unchanged (20 * 0.5)
    }

    #[test]
    fn test_use_shower_credit() {
        let mut account = LoyaltyAccount::new();
        fuel(&mut account, 50.0, "Pilot Travel Center");
        assert!(account.use_shower_credit());
        assert_eq!(account.shower_credits, 0);
        // Second use fails
        assert!(!account.use_shower_credit());
    }

    #[test]
    fn test_loyalty_summary_text() {
        let mut account = LoyaltyAccount::new();
        fuel(&mut account, 50.0, "Pilot Travel Center");
        let summary = account.summary();
        assert!(summary.contains("Loyalty points:"));
        assert!(summary.contains("Total gallons fueled:"));
        assert!(summary.contains("Shower credits available:"));
        assert!(summary.contains("50"));
        assert!(summary.contains('1'));
        assert_eq!(
            summary,
            "Loyalty points: 50. Total gallons fueled: 50. Shower credits available: 1."
        );
    }

    #[test]
    fn test_loyalty_earnings_text() {
        let text = loyalty_earnings_text(50.0, 50.0, &["shower_credit".to_string()]);
        assert!(text.contains("50 gallons fueled"));
        assert!(text.contains("50 loyalty points earned"));
        assert!(text.contains("shower credit earned"));
    }

    #[test]
    fn test_loyalty_earnings_text_no_rewards() {
        let text = loyalty_earnings_text(30.0, 30.0, &[]);
        assert!(text.contains("30 gallons fueled"));
        assert!(text.contains("30 loyalty points earned"));
        assert!(!text.contains("shower credit"));
    }

    #[test]
    fn test_reward_cost_text() {
        let text = reward_cost_text("shower");
        assert!(text.contains("free shower"));
        assert!(text.contains("50 loyalty points"));
        assert_eq!(text, "free shower costs 50 loyalty points.");
    }

    #[test]
    fn test_zero_gallons_no_points() {
        let mut account = LoyaltyAccount::new();
        let result = fuel(&mut account, 0.0, "Pilot Travel Center");
        assert_eq!(result.points_earned, 0.0);
        assert_eq!(account.total_points, 0.0);
    }

    #[test]
    fn test_negative_gallons_no_points() {
        let mut account = LoyaltyAccount::new();
        let result = fuel(&mut account, -10.0, "Pilot Travel Center");
        assert_eq!(result.points_earned, 0.0);
        assert_eq!(account.total_points, 0.0);
    }

    #[test]
    fn test_realistic_fueling_scenario() {
        let mut account = LoyaltyAccount::new();

        // Fuel at Pilot (travel center) - good rate
        let result1 = fuel(&mut account, 75.0, "Pilot Travel Center");
        assert_eq!(result1.points_earned, 75.0);
        assert_eq!(account.shower_credits, 1);

        // Fuel at generic stop - lower rate but necessary
        let result2 = fuel(&mut account, 40.0, "Independent Truck Stop");
        assert_eq!(result2.points_earned, 20.0); // 0.5 rate
        assert_eq!(account.shower_credits, 1); // No change (below threshold)

        // Fuel at Big Buck's (landmark) - bonus rate
        let result3 = fuel(&mut account, 60.0, "Big Buck's");
        assert_eq!(result3.points_earned, 90.0); // 1.5 rate
        assert_eq!(account.shower_credits, 2);

        // Total points and strategic redemption
        assert_eq!(account.total_points, 185.0);
        assert!(account.can_redeem("shower"));
        assert!(account.can_redeem("parking"));
        assert!(account.can_redeem("food"));

        // Redeem shower
        let shower_result = account.redeem_reward("shower");
        assert!(shower_result.success);
        assert_eq!(account.total_points, 135.0); // 185 - 50
    }

    #[test]
    fn test_loyalty_serialization() {
        let mut account = LoyaltyAccount::new();
        account.add_fueling(50.0, None, "Pilot Travel Center", "Springfield, IL");

        let data = account.to_dict();
        assert_eq!(data["total_points"], 50.0);
        assert_eq!(data["shower_credits"], 1);
        assert_eq!(data["fueling_history"].as_array().unwrap().len(), 1);

        let restored = LoyaltyAccount::from_dict(&data);
        assert_eq!(restored.total_points, 50.0);
        assert_eq!(restored.shower_credits, 1);
        assert_eq!(restored.fueling_history.len(), 1);
        assert_eq!(restored, account);
    }

    #[test]
    fn test_loyalty_from_empty_dict() {
        let account = LoyaltyAccount::from_dict(&serde_json::json!({}));
        assert_eq!(account.total_points, 0.0);
        assert_eq!(account.shower_credits, 0);
        assert!(account.brand_points.is_empty());
        assert!(account.fueling_history.is_empty());
        // And anything that is not a dict at all.
        assert_eq!(
            LoyaltyAccount::from_dict(&serde_json::json!(null)),
            LoyaltyAccount::new()
        );
        assert_eq!(
            LoyaltyAccount::from_dict(&serde_json::json!("nope")),
            LoyaltyAccount::new()
        );
    }

    #[test]
    fn from_dict_coerces_like_python() {
        // float("12") / int(3.0) style coercion of a hand-edited save.
        let account = LoyaltyAccount::from_dict(&serde_json::json!({
            "total_points": "12",
            "shower_credits": 3.0,
            "brand_points": {"pilot": 4},
            "fueling_history": [{"gallons": 5, "stop_name": "X"}],
        }));
        assert_eq!(account.total_points, 12.0);
        assert_eq!(account.shower_credits, 3);
        assert_eq!(account.brand_points["pilot"], 4.0);
        assert_eq!(account.fueling_history[0].gallons, 5.0);
        assert_eq!(account.fueling_history[0].stop_name, "X");
        assert_eq!(account.fueling_history[0].brand_key, None);
    }

    #[test]
    fn explicit_brand_key_sets_the_rate_and_is_recorded() {
        let mut account = LoyaltyAccount::new();
        let result = account.add_fueling(10.0, Some("big_bucks"), "Some Name", "");
        assert_eq!(result.points_earned, 15.0);
        assert_eq!(account.brand_points["big_bucks"], 15.0);
        let unknown = account.add_fueling(10.0, Some("nobody"), "Pilot Travel Center", "");
        assert_eq!(unknown.points_earned, 5.0); // an unknown key stays generic
        assert_eq!(account.brand_points["nobody"], 5.0);
    }

    // -- tests/test_loyalty_integration.py -----------------------------------

    // The five `tests/test_loyalty_integration.py` cases run against the real
    // `Profile` in `models::profile::tests` (same names).

    #[test]
    fn metric_earnings_name_the_fuel_in_the_players_units() {
        let text = loyalty_earnings_text_in(50.0, 50.0, &[], Some("189 liters"));
        assert!(text.starts_with("189 liters fueled"), "{text}");
        assert!(!text.contains("gallon"), "{text}");
    }
}
