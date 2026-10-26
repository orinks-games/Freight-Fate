//! Freight market: per-cargo-class pay multipliers that drift day by day
//! (port of `freight_fate/models/market.py`).
//!
//! Each cargo class carries a multiplier between 0.8 and 1.3 applied to job
//! pay. Multipliers move once per in-game day with a seeded random walk, so a
//! profile's market history is deterministic: replaying the same seed over the
//! same days always lands on the same numbers. The whole state is persisted on
//! the player profile.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::pyfmt::round_py_n;
use crate::pyrandom::PyRandom;

/// What the market watch says when no cargo class stands out.
pub const STEADY_SUMMARY: &str = "Freight market is steady across the board.";

pub const MARKET_MIN: f64 = 0.8;
pub const MARKET_MAX: f64 = 1.3;
pub const DAILY_DRIFT: f64 = 0.06;

/// Cargo classes tracked by the market (mirrors `jobs.CARGO_CATALOG`; kept as a
/// literal so this module needs no imports from the job layer).
pub const MARKET_CARGO_KEYS: [&str; 16] = [
    "general",
    "retail",
    "parcel",
    "container",
    "bulk",
    "grain",
    "farm_inputs",
    "construction",
    "lumber_paper",
    "automotive",
    "machinery",
    "steel",
    "food",
    "refrigerated",
    "chemicals",
    "electronics",
];

/// Spoken one-word market condition for a multiplier.
pub fn market_condition(multiplier: f64) -> &'static str {
    if multiplier >= 1.07 {
        return "tight";
    }
    if multiplier <= 0.97 {
        return "loose";
    }
    "steady"
}

fn clamp(value: f64) -> f64 {
    value.clamp(MARKET_MIN, MARKET_MAX)
}

/// Raw save shape of a [`Market`]; deserialising goes through
/// [`Market::from_parts`] so the `__post_init__` fill runs on load.
#[derive(Deserialize)]
struct MarketData {
    #[serde(default = "fresh_seed")]
    seed: i64,
    #[serde(default)]
    day: i64,
    #[serde(default)]
    multipliers: IndexMap<String, f64>,
}

/// `random.randrange(2**31)` on the global generator: a fresh market's seed,
/// deliberately irreproducible.
fn fresh_seed() -> i64 {
    PyRandom::new_unseeded().randrange(1 << 31)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(from = "MarketData")]
pub struct Market {
    pub seed: i64,
    pub day: i64,
    /// Insertion-ordered, like the Python dict it round-trips with.
    pub multipliers: IndexMap<String, f64>,
}

impl From<MarketData> for Market {
    fn from(data: MarketData) -> Self {
        Market::from_parts(data.seed, data.day, data.multipliers)
    }
}

impl Default for Market {
    fn default() -> Self {
        Self::new()
    }
}

impl Market {
    /// `Market()`: a fresh seed, day 0, the seeded starting multipliers.
    pub fn new() -> Self {
        Self::from_parts(fresh_seed(), 0, IndexMap::new())
    }

    /// `Market(seed=...)`.
    pub fn with_seed(seed: i64) -> Self {
        Self::from_parts(seed, 0, IndexMap::new())
    }

    /// `Market(seed, day, multipliers)` including `__post_init__`.
    pub fn from_parts(seed: i64, day: i64, mut multipliers: IndexMap<String, f64>) -> Self {
        let missing: Vec<&str> = MARKET_CARGO_KEYS
            .iter()
            .copied()
            .filter(|key| !multipliers.contains_key(*key))
            .collect();
        if !missing.is_empty() {
            // Careers saved before a cargo-class expansion carry multipliers only
            // for the classes that existed then. Newly tracked classes get their
            // seeded starting values -- the same draw a fresh market would make --
            // so every career trades the full catalog and stays deterministic.
            let mut rng = PyRandom::new_from_i64(seed);
            let seeded: Vec<(&str, f64)> = MARKET_CARGO_KEYS
                .iter()
                .map(|key| (*key, round_py_n(rng.uniform(0.9, 1.15), 3)))
                .collect();
            for key in missing {
                let value = seeded
                    .iter()
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| *v)
                    .expect("every tracked class was seeded");
                multipliers.insert(key.to_string(), value);
            }
        }
        Self {
            seed,
            day,
            multipliers,
        }
    }

    pub fn multiplier(&self, cargo_key: &str) -> f64 {
        self.multipliers.get(cargo_key).copied().unwrap_or(1.0)
    }

    pub fn condition(&self, cargo_key: &str) -> &'static str {
        market_condition(self.multiplier(cargo_key))
    }

    /// Walk the market forward to the given in-game day.
    ///
    /// Each elapsed day every class drifts by a step drawn from a generator
    /// seeded with (profile seed, day), so catching up several days at once
    /// gives the same result as advancing one day at a time.
    pub fn advance_to(&mut self, day: i64) -> bool {
        let mut changed = false;
        while self.day < day {
            self.day += 1;
            let mut rng = PyRandom::new_from_i128(self.seed as i128 * 1_000_003 + self.day as i128);
            let mut keys: Vec<String> = self.multipliers.keys().cloned().collect();
            keys.sort();
            for key in keys {
                let step = rng.uniform(-DAILY_DRIFT, DAILY_DRIFT);
                let current = self.multipliers[&key];
                self.multipliers
                    .insert(key, round_py_n(clamp(current + step), 3));
            }
            changed = true;
        }
        changed
    }

    /// The board's entry line: the standout cargo classes, or nothing when
    /// the market is steady (Tab still answers with [`Self::summary`]).
    pub fn headline(&self) -> String {
        let summary = self.summary();
        if summary == STEADY_SUMMARY {
            String::new()
        } else {
            summary
        }
    }

    /// Spoken job-board headline naming the standout cargo classes.
    pub fn summary(&self) -> String {
        let mut items: Vec<(&String, f64)> =
            self.multipliers.iter().map(|(k, v)| (k, *v)).collect();
        items.sort_by(|a, b| a.0.cmp(b.0));
        let mut tight: Vec<(&String, f64)> =
            items.iter().copied().filter(|kv| kv.1 >= 1.07).collect();
        // Python's `sorted(..., reverse=True)` keeps equal keys in their
        // original order, as a stable descending sort does.
        tight.sort_by(|a, b| b.1.partial_cmp(&a.1).expect("finite multipliers"));
        let mut loose: Vec<(&String, f64)> =
            items.iter().copied().filter(|kv| kv.1 <= 0.97).collect();
        loose.sort_by(|a, b| a.1.partial_cmp(&b.1).expect("finite multipliers"));
        if tight.is_empty() && loose.is_empty() {
            return STEADY_SUMMARY.to_string();
        }
        let parts: Vec<String> = tight
            .iter()
            .take(2)
            .chain(loose.iter().take(2))
            .map(|(key, mult)| format!("{} {}", key.replace('_', " "), market_condition(*mult)))
            .collect();
        format!("Market watch: {}.", parts.join(", "))
    }
}

#[cfg(test)]
mod tests {
    //! Ported from `tests/test_market.py`.

    use super::*;

    fn assert_in_bounds(market: &Market) {
        for (key, mult) in &market.multipliers {
            assert!((MARKET_MIN..=MARKET_MAX).contains(mult), "{key} {mult}");
        }
    }

    fn key_set(market: &Market) -> std::collections::BTreeSet<&str> {
        market.multipliers.keys().map(String::as_str).collect()
    }

    fn all_keys() -> std::collections::BTreeSet<&'static str> {
        MARKET_CARGO_KEYS.iter().copied().collect()
    }

    const LEGACY_KEYS: [&str; 8] = [
        "bulk",
        "container",
        "electronics",
        "food",
        "general",
        "machinery",
        "refrigerated",
        "retail",
    ];

    #[test]
    fn test_initial_multipliers_cover_all_cargo_classes() {
        let m = Market::with_seed(42);
        assert_eq!(key_set(&m), all_keys());
        assert_in_bounds(&m);
    }

    #[test]
    fn test_legacy_market_gains_newly_tracked_cargo_classes() {
        // A career saved before a cargo-class expansion carries only the classes
        // that existed then (8 in the wild). Loading it must fill in the rest.
        let kept: IndexMap<String, f64> =
            LEGACY_KEYS.iter().map(|k| (k.to_string(), 1.25)).collect();
        let mut m = Market::from_parts(42, 12, kept.clone());
        assert_eq!(key_set(&m), all_keys());
        // The classes the career already tracked keep their drifted values.
        for key in LEGACY_KEYS {
            assert_eq!(m.multipliers[key], kept[key]);
        }
        assert_in_bounds(&m);
        // The fill is deterministic: the same seed always draws the same values.
        let again = Market::from_parts(42, 12, kept.clone());
        assert_eq!(again.multipliers, m.multipliers);
        // And the migrated market keeps drifting every class from here on.
        m.advance_to(13);
        assert_eq!(key_set(&m), all_keys());
        assert_in_bounds(&m);
    }

    #[test]
    fn test_drift_stays_within_bounds_over_a_long_career() {
        let mut m = Market::with_seed(7);
        m.advance_to(400);
        assert_eq!(m.day, 400);
        assert_in_bounds(&m);
    }

    #[test]
    fn test_drift_is_deterministic_stepwise_vs_jump() {
        let mut a = Market::with_seed(99);
        let mut b = Market::with_seed(99);
        for day in 1..31 {
            a.advance_to(day);
        }
        b.advance_to(30);
        assert_eq!(a.multipliers, b.multipliers);
        assert_eq!(a.day, 30);
        assert_eq!(b.day, 30);
    }

    #[test]
    fn test_different_seeds_diverge() {
        let mut a = Market::with_seed(1);
        let mut b = Market::with_seed(2);
        a.advance_to(10);
        b.advance_to(10);
        assert_ne!(a.multipliers, b.multipliers);
    }

    #[test]
    fn test_advance_reports_whether_anything_changed() {
        let mut m = Market::with_seed(5);
        assert!(!m.advance_to(0));
        assert!(m.advance_to(2));
        assert!(!m.advance_to(2));
    }

    #[test]
    fn test_market_condition_labels() {
        assert_eq!(market_condition(1.3), "tight");
        assert_eq!(market_condition(1.1), "tight");
        assert_eq!(market_condition(1.0), "steady");
        assert_eq!(market_condition(0.95), "loose");
        assert_eq!(market_condition(0.8), "loose");
    }

    #[test]
    fn test_summary_names_the_standouts() {
        let mut m = Market::with_seed(1);
        m.multipliers.insert("electronics".into(), 1.3);
        m.multipliers.insert("bulk".into(), 0.8);
        let summary = m.summary();
        assert!(summary.contains("electronics tight"));
        assert!(summary.contains("bulk loose"));
    }

    #[test]
    fn summary_is_steady_when_nothing_stands_out() {
        let mut m = Market::with_seed(1);
        for key in MARKET_CARGO_KEYS {
            m.multipliers.insert(key.into(), 1.0);
        }
        assert_eq!(m.summary(), "Freight market is steady across the board.");
        m.multipliers.insert("steel".into(), 1.2);
        assert_eq!(m.summary(), "Market watch: steel tight.");
    }

    fn uniform_market(mult: f64) -> Market {
        let mut market = Market::with_seed(0);
        market.multipliers = MARKET_CARGO_KEYS
            .iter()
            .map(|k| (k.to_string(), mult))
            .collect();
        market
    }

    #[test]
    fn test_job_pay_scales_with_market() {
        use crate::data::world::get_world;
        use crate::models::jobs::{JobBoard, OfferOptions};
        let tight = uniform_market(1.3);
        let loose = uniform_market(0.8);
        let no_endorsements: &[&str] = &[];
        // identical board seeds generate the same jobs, so pay isolates the market
        let jobs_tight = JobBoard::seeded(get_world(), 11).offers(
            "Chicago",
            no_endorsements,
            OfferOptions {
                market: Some(&tight),
                ..Default::default()
            },
        );
        let jobs_loose = JobBoard::seeded(get_world(), 11).offers(
            "Chicago",
            no_endorsements,
            OfferOptions {
                market: Some(&loose),
                ..Default::default()
            },
        );
        assert!(!jobs_tight.is_empty() && jobs_tight.len() == jobs_loose.len());
        for (jh, jc) in jobs_tight.iter().zip(&jobs_loose) {
            assert_eq!(jh.cargo.key, jc.cargo.key);
            assert!((jh.pay / jc.pay - 1.3 / 0.8).abs() < 0.01);
            assert!(jh.describe_plain().contains("Market tight."));
            assert!(jc.describe_plain().contains("Market loose."));
        }
    }

    #[test]
    fn test_steady_market_is_not_called_out_per_job() {
        use crate::data::world::get_world;
        use crate::models::jobs::{JobBoard, OfferOptions};
        let steady = uniform_market(1.0);
        let no_endorsements: &[&str] = &[];
        let jobs = JobBoard::seeded(get_world(), 11).offers(
            "Chicago",
            no_endorsements,
            OfferOptions {
                market: Some(&steady),
                ..Default::default()
            },
        );
        assert!(!jobs.is_empty());
        assert!(jobs
            .iter()
            .all(|j| !j.describe_plain().contains("Market is")));
    }

    #[test]
    fn test_profile_persists_market_state() {
        use crate::models::profile::{tests::with_data_dir, Profile};
        with_data_dir(|_| {
            let mut p = Profile::named("Market Test");
            p.market.advance_to(5);
            let snapshot = p.market.multipliers.clone();
            let path = p.save().unwrap();
            let loaded = Profile::load(&path).unwrap();
            assert_eq!(loaded.market.seed, p.market.seed);
            assert_eq!(loaded.market.day, 5);
            assert_eq!(loaded.market.multipliers, snapshot);
        });
    }

    #[test]
    fn test_profile_load_migrates_legacy_market() {
        use crate::models::profile::{tests::with_data_dir, Profile};
        // Saved by a build that only knew 8 cargo classes; loading must fill in
        // the full catalog without touching the classes the save already had.
        with_data_dir(|_| {
            let mut p = Profile::named("Legacy Market");
            let legacy: IndexMap<String, f64> = [
                "bulk",
                "container",
                "electronics",
                "food",
                "general",
                "machinery",
                "refrigerated",
                "retail",
            ]
            .iter()
            .map(|key| (key.to_string(), p.market.multipliers[*key]))
            .collect();
            p.market.multipliers = legacy.clone();
            let path = p.save().unwrap();
            let loaded = Profile::load(&path).unwrap();
            let mut keys: Vec<&str> = loaded
                .market
                .multipliers
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            let mut expected: Vec<&str> = MARKET_CARGO_KEYS.to_vec();
            expected.sort_unstable();
            assert_eq!(keys, expected);
            for (key, value) in &legacy {
                assert_eq!(loaded.market.multipliers[key], *value);
            }
        });
    }

    // -- serde shape ---------------------------------------------------------

    #[test]
    fn market_round_trips_through_json_and_fills_on_load() {
        let m = Market::with_seed(42);
        let text = serde_json::to_string(&m).unwrap();
        let back: Market = serde_json::from_str(&text).unwrap();
        assert_eq!(back, m);
        // The key order is the Python dict's: catalog order for a fresh market.
        let keys: Vec<&str> = back.multipliers.keys().map(String::as_str).collect();
        assert_eq!(keys, MARKET_CARGO_KEYS.to_vec());
        // A legacy 8-class save fills the rest on load, like __post_init__.
        let legacy: Market = serde_json::from_str(
            r#"{"seed": 42, "day": 12, "multipliers": {"bulk": 1.25, "general": 1.25}}"#,
        )
        .unwrap();
        assert_eq!(key_set(&legacy), all_keys());
        assert_eq!(legacy.multipliers["bulk"], 1.25);
        assert_eq!(legacy.day, 12);
    }

    #[test]
    fn seeded_fill_matches_cpython() {
        // python: Market(seed=42).multipliers["general"] == round(0.9 + 0.25 *
        // 0.6394267984578837, 3) == 1.06 (first draw of Random(42)).
        let m = Market::with_seed(42);
        assert_eq!(m.multipliers["general"], 1.06);
    }
}
