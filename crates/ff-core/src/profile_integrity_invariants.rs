//! Catalog snapshot shared with the orinks.net cloud-save validator.
//!
//! Port of `freight_fate/profile_integrity_invariants.py`. This is a
//! source-tree-only export for the validator; never called by the game at
//! runtime, so frozen builds (which carry no world_data tree) are
//! unaffected.
//!
//! The Python module reads every figure straight off the models package
//! (`ACHIEVEMENTS`, `Career`, `Profile`, `TRUCK_CATALOG`, ...). The Rust
//! rendering takes those figures as a [`CatalogInputs`] argument so the
//! module itself stays free of the model layer; [`CatalogInputs::current`]
//! is the one that reads the real shipped catalogs, and it is what the
//! `ff-invariants` binary writes the file from. City labels are read from
//! the world data under the `data_root` the caller passes
//! ([`world_data_root`] resolves the shipped one).
//!
//! Anything that renders this file for the server must go through
//! `current()`: a hand-assembled `CatalogInputs` is a fixture, and a
//! fixture shipped to the validator is a validator that convicts honest
//! players (or acquits edited careers) the moment it disagrees with the
//! game.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Map, Value};

use crate::cloud_save_integrity::ascii_json_string;
use crate::pyfmt::py_str_float;

/// Signature keys ride inside the saved file but never inside a cloud
/// upload -- the upload strips them and the server signs its own revision
/// instead.
pub const LOCAL_ONLY_FIELDS: [&str; 2] = ["_signature", "_signature_version"];

/// `_json_number`: a whole float prints as an int in the export.
pub fn json_number(value: f64) -> Value {
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 9.0e15 {
        Value::from(value as i64)
    } else {
        Value::from(value)
    }
}

/// One credential row: key, the carrier-sponsored level (None for a
/// credential only ever earned through its course -- the site must not
/// level-derive those), the spoken label, and the ladder tier.
#[derive(Debug, Clone, PartialEq)]
pub struct EndorsementRow {
    pub key: String,
    pub level: Option<i64>,
    pub label: String,
    pub tier: String,
}

/// One carrier fleet band: the first level it applies to and its label.
#[derive(Debug, Clone, PartialEq)]
pub struct FleetTierRow {
    pub min_level: i64,
    pub label: String,
}

/// One company-driver carrier, as its public page on orinks.net shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct CarrierRow {
    pub key: String,
    pub name: String,
    /// The city the home-terminal menu opens on; the driver can pick any.
    pub suggested_home_terminal: String,
    pub description: String,
    pub pay_share: f64,
    pub min_per_mile: f64,
    pub stop_pay: f64,
    pub on_time_bonus_share: f64,
    /// `DispatchProfile::summary`, the game's own words.
    pub dispatch: String,
    /// Cargo labels the carrier's board favors, in the start option's order.
    pub favored_freight: Vec<String>,
}

/// One public trailer catalog row.
#[derive(Debug, Clone, PartialEq)]
pub struct TrailerRow {
    pub key: String,
    pub label: String,
    pub purchase_price: f64,
}

/// The economy and catalog figures the export carries, read off the models
/// package. Field names follow the Python constants they come from.
#[derive(Debug, Clone, PartialEq)]
pub struct CatalogInputs {
    /// `achievement.id` for every entry of `ACHIEVEMENTS`.
    pub achievement_ids: Vec<String>,
    /// Achievement key and player-facing title from `ACHIEVEMENTS`.
    pub achievement_labels: Vec<(String, String)>,
    /// Achievement key, category key and player-facing description from
    /// `ACHIEVEMENTS`: what the public profile says a badge was for. Hidden
    /// badges are included -- a badge on a profile has been earned, and the
    /// hidden flag only guards the locked list in the game.
    pub achievement_details: Vec<(String, String, String)>,
    /// Category key and title from `CATEGORIES`, in menu order.
    pub achievement_categories: Vec<(String, String)>,
    /// Career titles from `CAREER_RANKS`, in exact level order.
    pub career_titles: Vec<String>,
    /// Company-path titles from `COMPANY_CAREER_RANKS`, in exact level order.
    pub company_career_titles: Vec<String>,
    /// Career-start key and carrier name from `START_OPTIONS`.
    pub carrier_labels: Vec<(String, String)>,
    /// `company_start_options()`: the carriers a company driver can work for.
    pub carriers: Vec<CarrierRow>,
    /// `REPUTATION_BONUS_MAX_SHARE`.
    pub reputation_bonus_max_share: f64,
    /// `ASSIGNED_REPOSITION_PAY_FRACTION`.
    pub assigned_reposition_pay_fraction: f64,
    /// Public trailer details from `TRAILER_CATALOG`.
    pub trailers: Vec<TrailerRow>,
    /// `STARTING_MONEY`.
    pub starting_money: f64,
    /// `max(option.starting_money for option in all_start_options())`.
    pub starting_money_max: f64,
    /// `PAY_ADVANCE_LIMIT`.
    pub pay_advance_limit: f64,
    /// `XP_PER_MILE_ON_TIME`.
    pub xp_per_mile_on_time: f64,
    /// `XP_SPECIALTY_MULT`.
    pub xp_specialty_mult: f64,
    /// `XP_STREAK_MAX_BONUS`.
    pub xp_streak_max_bonus: f64,
    /// `XP_CLEAN_BONUS`.
    pub xp_clean_bonus: f64,
    /// `DELIVERY_COMPLETION_XP`.
    pub delivery_completion_xp: f64,
    /// `LEVEL_XP`, one threshold per level.
    pub level_xp: Vec<i64>,
    /// `MARKET_CARGO_KEYS`.
    pub market_cargo_keys: Vec<String>,
    /// `Profile.__dataclass_fields__` (the `version` key is added here).
    pub profile_fields: Vec<String>,
    /// `Career.__dataclass_fields__`.
    pub career_fields: Vec<String>,
    /// `ENDORSEMENT_LEVELS` joined with `ENDORSEMENT_LABELS_SPOKEN`.
    pub endorsements: Vec<EndorsementRow>,
    /// `FLEET_TIERS`, in catalog order.
    pub fleet_tiers: Vec<FleetTierRow>,
    /// The keys of the record `_fresh_condition()` writes.
    pub truck_condition_fields: Vec<String>,
    /// `SAVE_VERSION`.
    pub source_save_version: i64,
    /// `TRUCK_CATALOG`: key, label, price, in catalog order.
    pub trucks: Vec<(String, String, f64)>,
    /// `UPGRADE_CATALOG`: key and per-tier prices, in catalog order.
    pub upgrade_prices: Vec<(String, Vec<f64>)>,
}

impl CatalogInputs {
    /// The figures the shipped game actually awards, read off the live
    /// catalogs -- the Rust equivalent of the module-level imports at the top
    /// of `profile_integrity_invariants.py`.
    ///
    /// This is the only assembly the exporter may use. Every field below
    /// names the Python constant it stands in for, so a balance pass that
    /// moves a constant moves the export with it and the validator on
    /// orinks.net never falls behind the build players are running.
    pub fn current() -> Self {
        use crate::achievements::{ACHIEVEMENTS, CATEGORIES};
        use crate::models::business::REPUTATION_BONUS_MAX_SHARE;
        use crate::models::career::{
            Career, DELIVERY_COMPLETION_XP, LEVEL_XP, XP_CLEAN_BONUS, XP_PER_MILE_ON_TIME,
            XP_SPECIALTY_MULT, XP_STREAK_MAX_BONUS,
        };
        use crate::models::career_ladder::{CAREER_RANKS, COMPANY_CAREER_RANKS};
        use crate::models::carrier_fleet::FLEET_TIERS;
        use crate::models::credentials::CREDENTIALS;
        use crate::models::economy::PAY_ADVANCE_LIMIT;
        use crate::models::jobs::{cargo_type, ASSIGNED_REPOSITION_PAY_FRACTION};
        use crate::models::market::MARKET_CARGO_KEYS;
        use crate::models::profile::{
            fresh_condition, PROFILE_FIELDS, SAVE_VERSION, STARTING_MONEY,
        };
        use crate::models::start_options::{
            all_start_options, company_start_options, NORTHSTAR_PAY,
        };
        use crate::models::trailers::TRAILER_CATALOG;
        use crate::models::trucks::{TRUCK_CATALOG, UPGRADE_CATALOG};

        // `sorted(Career.__dataclass_fields__)`. The Rust dataclass is the
        // serde shape of the same struct -- serialising the default is what
        // guarantees the exported list is the key set a save actually
        // carries, rather than a second list that can drift from it.
        let career_json =
            serde_json::to_value(Career::default()).expect("Career serialises to a JSON object");
        let career_fields: Vec<String> = career_json
            .as_object()
            .expect("Career serialises to a JSON object")
            .keys()
            .cloned()
            .collect();

        let endorsements: Vec<EndorsementRow> = CREDENTIALS
            .iter()
            .map(|cred| EndorsementRow {
                key: cred.key.to_string(),
                level: cred.grant_level,
                label: cred.label.to_string(),
                tier: cred.tier.as_str().to_string(),
            })
            .collect();

        CatalogInputs {
            achievement_ids: ACHIEVEMENTS
                .iter()
                .map(|badge| badge.id.to_string())
                .collect(),
            achievement_labels: ACHIEVEMENTS
                .iter()
                .map(|badge| (badge.id.to_string(), badge.name.to_string()))
                .collect(),
            achievement_details: ACHIEVEMENTS
                .iter()
                .map(|badge| {
                    (
                        badge.id.to_string(),
                        badge.category.to_string(),
                        badge.description.to_string(),
                    )
                })
                .collect(),
            achievement_categories: CATEGORIES
                .iter()
                .map(|category| (category.id.to_string(), category.title.to_string()))
                .collect(),
            career_titles: CAREER_RANKS
                .iter()
                .map(|rank| rank.title.to_string())
                .collect(),
            company_career_titles: COMPANY_CAREER_RANKS
                .iter()
                .map(|rank| rank.title.to_string())
                .collect(),
            carrier_labels: all_start_options()
                .iter()
                .map(|option| (option.key.to_string(), option.carrier_name.to_string()))
                .collect(),
            carriers: company_start_options()
                .into_iter()
                .map(|option| {
                    let pay = option.company_pay.unwrap_or(NORTHSTAR_PAY);
                    CarrierRow {
                        key: option.key.to_string(),
                        name: option.carrier_name.to_string(),
                        suggested_home_terminal: option.default_city.to_string(),
                        description: option.help_text.to_string(),
                        pay_share: pay.pay_share,
                        min_per_mile: pay.min_per_mile,
                        stop_pay: pay.stop_pay,
                        on_time_bonus_share: pay.on_time_bonus_share,
                        dispatch: option.dispatch.summary(),
                        favored_freight: option
                            .cargo_weight_bonus
                            .iter()
                            .map(|(key, _)| {
                                cargo_type(key)
                                    .expect("a start option favors a catalog cargo")
                                    .label
                                    .to_string()
                            })
                            .collect(),
                    }
                })
                .collect(),
            reputation_bonus_max_share: REPUTATION_BONUS_MAX_SHARE,
            assigned_reposition_pay_fraction: ASSIGNED_REPOSITION_PAY_FRACTION,
            trailers: TRAILER_CATALOG
                .iter()
                .map(|trailer| TrailerRow {
                    key: trailer.key.to_string(),
                    label: trailer.label.to_string(),
                    purchase_price: trailer.purchase_price,
                })
                .collect(),
            starting_money: STARTING_MONEY,
            starting_money_max: all_start_options()
                .iter()
                .map(|option| option.starting_money)
                .fold(f64::NEG_INFINITY, f64::max),
            pay_advance_limit: PAY_ADVANCE_LIMIT,
            xp_per_mile_on_time: XP_PER_MILE_ON_TIME,
            xp_specialty_mult: XP_SPECIALTY_MULT,
            xp_streak_max_bonus: XP_STREAK_MAX_BONUS,
            xp_clean_bonus: XP_CLEAN_BONUS,
            delivery_completion_xp: DELIVERY_COMPLETION_XP,
            level_xp: LEVEL_XP.iter().map(|xp| *xp as i64).collect(),
            market_cargo_keys: MARKET_CARGO_KEYS
                .iter()
                .map(|key| key.to_string())
                .collect(),
            profile_fields: PROFILE_FIELDS
                .iter()
                .map(|field| field.to_string())
                .collect(),
            career_fields,
            endorsements,
            fleet_tiers: FLEET_TIERS
                .iter()
                .map(|tier| FleetTierRow {
                    min_level: tier.min_level,
                    label: tier.label.to_string(),
                })
                .collect(),
            // `sorted(_fresh_condition())` -- the keys off a record the game
            // really writes, not a second list beside it. The Python comment
            // on `truck_condition_fields` below is the whole reason: the
            // export once came off a dataclass that had stopped matching the
            // record, and told the server five legitimate keys were unknown.
            truck_condition_fields: fresh_condition(0.0).keys().cloned().collect(),
            source_save_version: SAVE_VERSION,
            trucks: TRUCK_CATALOG
                .iter()
                .map(|(key, truck)| (key.to_string(), truck.label.to_string(), truck.price))
                .collect(),
            upgrade_prices: UPGRADE_CATALOG
                .iter()
                .map(|upgrade| (upgrade.key.to_string(), upgrade.prices.to_vec()))
                .collect(),
        }
    }
}

/// Every share bonus in `record_delivery` taken at once, at its best.
///
/// Both bonuses multiply the whole award, flat completion XP included, so
/// this factor belongs to both terms below.
pub fn xp_best_case_multiplier(inputs: &CatalogInputs) -> f64 {
    (1.0 + inputs.xp_streak_max_bonus) * (1.0 + inputs.xp_clean_bonus)
}

/// The most XP one mile can teach, taking every bonus at its best.
///
/// The validator's ceiling is `deliveries * flat + miles * this`. It has to
/// sit at or above what the game can actually award, because anything lower
/// convicts honest drivers -- a copied 1.2 here was below even the base
/// on-time rate on this line. Recompute it from the real constants whenever
/// the XP model grows a term.
pub fn xp_per_mile_max(inputs: &CatalogInputs) -> f64 {
    inputs.xp_per_mile_on_time * inputs.xp_specialty_mult * xp_best_case_multiplier(inputs)
}

/// XP a settled load teaches regardless of distance, at its best.
pub fn xp_flat_per_delivery(inputs: &CatalogInputs) -> f64 {
    inputs.delivery_completion_xp * xp_best_case_multiplier(inputs)
}

/// Top-level keys a cloud upload carries, straight off the dataclass.
///
/// The validator checks uploads against an exact field list. Hand-keeping
/// that list on the server means it silently falls behind the moment a field
/// is added or removed here -- and the failure is a flat schema rejection
/// that reads to the player as "your backup is broken", not as version skew.
/// Export it instead, so the two sides cannot drift.
pub fn profile_fields(inputs: &CatalogInputs) -> Vec<String> {
    let mut fields: Vec<String> = inputs
        .profile_fields
        .iter()
        .cloned()
        .chain(std::iter::once("version".to_string()))
        .filter(|field| !LOCAL_ONLY_FIELDS.contains(&field.as_str()))
        .collect();
    fields.sort();
    fields.dedup();
    fields
}

/// Keys inside one owned truck's condition record.
///
/// Same reason as `profile_fields`, one level down: the validator checks each
/// record against an exact list, and this record is where new per-truck state
/// lands (brake and engine wear, traction gear). A hand-kept copy on the
/// server would reject the next build's saves the moment one is added.
///
/// Read from the record the game actually writes, not from the TruckCondition
/// dataclass. On this line the records are plain dicts built by
/// `_fresh_condition`, and they outgrew that dataclass when the physics arc
/// added brake wear, engine wear, and traction gear -- it kept four fields
/// while a real record carries nine. Exporting the dataclass therefore told
/// the server that five legitimate keys were unknown, which would have failed
/// every 1.9 save on the exact-field check the moment the two sides met.
pub fn truck_condition_fields(inputs: &CatalogInputs) -> Vec<String> {
    let mut fields = inputs.truck_condition_fields.clone();
    fields.sort();
    fields
}

/// `"<spoken city>, <state name>"` per city slug, read from `us/cities.json`
/// and `geo.json` under `data_root` (the `world_data` tree).
pub fn city_labels(data_root: &Path) -> Result<BTreeMap<String, String>, String> {
    let read = |relative: &str| -> Result<Value, String> {
        let path = data_root.join(relative);
        let text = fs::read_to_string(&path)
            .map_err(|err| format!("cannot read {}: {err}", path.display()))?;
        serde_json::from_str(&text).map_err(|err| format!("cannot parse {relative}: {err}"))
    };
    let cities_doc = read("us/cities.json")?; // runtime-data-ok
    let geo_doc = read("geo.json")?; // runtime-data-ok
    let cities = cities_doc
        .get("cities")
        .and_then(Value::as_object)
        .ok_or("us/cities.json has no cities table")?;
    let states = geo_doc
        .pointer("/countries/US/states")
        .and_then(Value::as_object)
        .ok_or("geo.json has no US states table")?;
    let mut labels = BTreeMap::new();
    for (slug, city) in cities {
        let spoken = city
            .get("spoken_city")
            .and_then(Value::as_str)
            .ok_or_else(|| format!("city {slug} has no spoken_city"))?;
        let state_code = city.get("state").and_then(Value::as_str).unwrap_or("");
        let state_name = states
            .get(state_code)
            .and_then(Value::as_str)
            .unwrap_or(state_code);
        let label = format!("{spoken}, {state_name}");
        let label = label.trim_end_matches([',', ' ']).to_string();
        labels.insert(slug.clone(), label);
    }
    Ok(labels)
}

/// The export document, keyed exactly as the validator reads it.
pub fn invariant_data(data_root: &Path, inputs: &CatalogInputs) -> Result<Value, String> {
    let mut achievement_ids = inputs.achievement_ids.clone();
    achievement_ids.sort();
    let achievement_labels: Map<String, Value> = inputs
        .achievement_labels
        .iter()
        .map(|(key, label)| (key.clone(), Value::from(label.clone())))
        .collect();
    let achievement_details: Map<String, Value> = inputs
        .achievement_details
        .iter()
        .map(|(key, category, description)| {
            let mut row = Map::new();
            row.insert("category".into(), Value::from(category.clone()));
            row.insert("description".into(), Value::from(description.clone()));
            (key.clone(), Value::Object(row))
        })
        .collect();
    let achievement_categories: Vec<Value> = inputs
        .achievement_categories
        .iter()
        .map(|(key, title)| {
            let mut row = Map::new();
            row.insert("key".into(), Value::from(key.clone()));
            row.insert("title".into(), Value::from(title.clone()));
            Value::Object(row)
        })
        .collect();
    let carrier_labels: Map<String, Value> = inputs
        .carrier_labels
        .iter()
        .map(|(key, label)| (key.clone(), Value::from(label.clone())))
        .collect();
    let carriers: Map<String, Value> = inputs
        .carriers
        .iter()
        .map(|carrier| {
            let mut pay = Map::new();
            pay.insert("payShare".into(), json_number(carrier.pay_share));
            pay.insert("minPerMile".into(), json_number(carrier.min_per_mile));
            pay.insert("stopPay".into(), json_number(carrier.stop_pay));
            pay.insert(
                "onTimeBonusShare".into(),
                json_number(carrier.on_time_bonus_share),
            );
            let mut row = Map::new();
            row.insert("name".into(), Value::from(carrier.name.clone()));
            row.insert(
                "suggestedHomeTerminal".into(),
                Value::from(carrier.suggested_home_terminal.clone()),
            );
            row.insert(
                "description".into(),
                Value::from(carrier.description.clone()),
            );
            row.insert("pay".into(), Value::Object(pay));
            row.insert("dispatch".into(), Value::from(carrier.dispatch.clone()));
            row.insert(
                "favoredFreight".into(),
                Value::from(carrier.favored_freight.clone()),
            );
            (carrier.key.clone(), Value::Object(row))
        })
        .collect();
    let trailer_catalog: Map<String, Value> = inputs
        .trailers
        .iter()
        .map(|trailer| {
            let mut row = Map::new();
            row.insert("label".into(), Value::from(trailer.label.clone()));
            row.insert("purchasePrice".into(), json_number(trailer.purchase_price));
            (trailer.key.clone(), Value::Object(row))
        })
        .collect();
    let mut market_cargo_keys = inputs.market_cargo_keys.clone();
    market_cargo_keys.sort();
    let mut career_fields = inputs.career_fields.clone();
    career_fields.sort();

    let mut endorsements = Map::new();
    let mut rows: Vec<&EndorsementRow> = inputs.endorsements.iter().collect();
    rows.sort_by(|a, b| a.key.cmp(&b.key));
    for row in rows {
        let mut entry = Map::new();
        // A course-only credential carries no level at all: the site's
        // held-check is `level >= entry.level || purchased`, and a missing
        // key comparing false is what keeps a background-checked credential
        // from being credited to every driver of some level.
        if let Some(level) = row.level {
            entry.insert("level".to_string(), Value::from(level));
        }
        entry.insert("label".to_string(), Value::from(row.label.clone()));
        entry.insert("tier".to_string(), Value::from(row.tier.clone()));
        endorsements.insert(row.key.clone(), Value::Object(entry));
    }

    let fleet_tiers: Vec<Value> = inputs
        .fleet_tiers
        .iter()
        .map(|tier| {
            let mut entry = Map::new();
            entry.insert("minLevel".to_string(), Value::from(tier.min_level));
            entry.insert("label".to_string(), Value::from(tier.label.clone()));
            Value::Object(entry)
        })
        .collect();

    let mut truck_labels = Map::new();
    let mut truck_prices = Map::new();
    for (key, label, price) in &inputs.trucks {
        truck_labels.insert(key.clone(), Value::from(label.clone()));
        truck_prices.insert(key.clone(), json_number(*price));
    }
    let mut upgrade_prices = Map::new();
    for (key, prices) in &inputs.upgrade_prices {
        upgrade_prices.insert(
            key.clone(),
            Value::Array(prices.iter().map(|p| json_number(*p)).collect()),
        );
    }

    let labels = city_labels(data_root)?;
    let city_labels: Map<String, Value> = labels
        .into_iter()
        .map(|(slug, label)| (slug, Value::from(label)))
        .collect();

    let mut out = Map::new();
    out.insert("achievementIds".into(), Value::from(achievement_ids));
    out.insert(
        "achievementLabels".into(),
        Value::Object(achievement_labels),
    );
    out.insert(
        "achievementDetails".into(),
        Value::Object(achievement_details),
    );
    out.insert(
        "achievementCategories".into(),
        Value::Array(achievement_categories),
    );
    out.insert(
        "careerTitles".into(),
        Value::from(inputs.career_titles.clone()),
    );
    out.insert(
        "companyCareerTitles".into(),
        Value::from(inputs.company_career_titles.clone()),
    );
    // Where a driver still on carrier wages leaves the owner-operator titles
    // for the company ones (career_ladder::uses_company_career_ranks).
    out.insert(
        "companyRankForkLevel".into(),
        Value::from(crate::models::career_ladder::COMPANY_RANK_FORK_LEVEL),
    );
    out.insert("carrierLabels".into(), Value::Object(carrier_labels));
    // The public carrier pages: each company carrier's wage plan, dispatch
    // leanings and favored freight, plus the two pay terms every carrier
    // shares. Exported so a balance pass moves the site with the game.
    out.insert("carriers".into(), Value::Object(carriers));
    let mut company_pay = Map::new();
    company_pay.insert(
        "reputationBonusMaxShare".into(),
        json_number(inputs.reputation_bonus_max_share),
    );
    company_pay.insert(
        "assignedRepositionPayFraction".into(),
        json_number(inputs.assigned_reposition_pay_fraction),
    );
    out.insert("companyPay".into(), Value::Object(company_pay));
    out.insert("cityLabels".into(), Value::Object(city_labels));
    // The economy terms the cloud-save validator needs to tell an edited
    // career from an honest one. They ship as data for the same reason the
    // field lists do: a copy kept on the server falls behind the next
    // balance pass, and every honest player on the new build then hears
    // that their backup was rejected. See the money and XP checks in
    // convex/freightFateSharedProfileValidation.ts.
    out.insert("startingMoney".into(), json_number(inputs.starting_money));
    // The most cash any career-start option hands over. The money ceiling
    // must credit this, not the company-driver default: the owner-operator
    // start opens with 18,000 dollars, and a ceiling built on 5,000
    // rejected every honest owner-operator backup until their earnings
    // eventually outgrew the gap. Wrong in the generous direction is the
    // survivable wrong here, so the validator uses the maximum rather
    // than a per-carrier lookup that would break on a start option the
    // server has not heard of yet.
    out.insert(
        "startingMoneyMax".into(),
        json_number(inputs.starting_money_max),
    );
    out.insert(
        "payAdvanceLimit".into(),
        json_number(inputs.pay_advance_limit),
    );
    out.insert("xpPerMileMax".into(), json_number(xp_per_mile_max(inputs)));
    out.insert(
        "xpFlatPerDelivery".into(),
        json_number(xp_flat_per_delivery(inputs)),
    );
    out.insert("levelXp".into(), Value::from(inputs.level_xp.clone()));
    out.insert("marketCargoKeys".into(), Value::from(market_cargo_keys));
    out.insert("profileFields".into(), Value::from(profile_fields(inputs)));
    out.insert("careerFields".into(), Value::from(career_fields));
    // Public-profile display data: orinks.net derives each driver's
    // endorsements (level-earned plus self-paid courses) and, for company
    // drivers, the carrier fleet tier straight from the validated career.
    // Exported rather than copied so the site's projection moves with the
    // next balance pass instead of drifting behind it.
    out.insert("endorsements".into(), Value::Object(endorsements));
    out.insert("fleetTiers".into(), Value::Array(fleet_tiers));
    out.insert(
        "truckConditionFields".into(),
        Value::from(truck_condition_fields(inputs)),
    );
    out.insert(
        "sourceSaveVersion".into(),
        Value::from(inputs.source_save_version),
    );
    out.insert("truckLabels".into(), Value::Object(truck_labels));
    out.insert("truckPrices".into(), Value::Object(truck_prices));
    out.insert("trailerCatalog".into(), Value::Object(trailer_catalog));
    out.insert("upgradePrices".into(), Value::Object(upgrade_prices));
    Ok(Value::Object(out))
}

/// `json.dumps(value, indent=2, sort_keys=True)`: two-space indent, keys
/// sorted, non-ASCII escaped, floats in Python repr.
fn dump_sorted(value: &Value, indent: usize, out: &mut String) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else {
                out.push_str(&py_str_float(n.as_f64().unwrap_or(0.0)));
            }
        }
        Value::String(s) => out.push_str(&ascii_json_string(s)),
        Value::Array(items) => {
            if items.is_empty() {
                out.push_str("[]");
                return;
            }
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('\n');
                out.push_str(&" ".repeat(indent + 2));
                dump_sorted(item, indent + 2, out);
            }
            out.push('\n');
            out.push_str(&" ".repeat(indent));
            out.push(']');
        }
        Value::Object(map) => {
            if map.is_empty() {
                out.push_str("{}");
                return;
            }
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            out.push('{');
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                out.push('\n');
                out.push_str(&" ".repeat(indent + 2));
                out.push_str(&ascii_json_string(key));
                out.push_str(": ");
                dump_sorted(&map[*key], indent + 2, out);
            }
            out.push('\n');
            out.push_str(&" ".repeat(indent));
            out.push('}');
        }
    }
}

/// The export as the validator's JSON file: `json.dumps(..., indent=2,
/// sort_keys=True)` plus a trailing newline.
pub fn rendered_invariants(data_root: &Path, inputs: &CatalogInputs) -> Result<String, String> {
    let data = invariant_data(data_root, inputs)?;
    let mut out = String::new();
    dump_sorted(&data, 0, &mut out);
    out.push('\n');
    Ok(out)
}

/// The `world_data` tree the shipped export reads its city labels from.
///
/// Python resolves this as `Path(__file__).parent / "data" / "world_data"`;
/// here it hangs off the same [`data_root`](crate::data::data_resources::data_root)
/// every other data reader uses, so `FREIGHT_FATE_DATA_ROOT` points the
/// exporter at a checkout the same way it points the game at one.
pub fn world_data_root() -> PathBuf {
    crate::data::data_resources::data_root().join("world_data")
}

/// `invariant_data()` with no arguments: the shipped catalogs, the shipped
/// world data. This is the production path.
pub fn current_invariant_data() -> Result<Value, String> {
    invariant_data(&world_data_root(), &CatalogInputs::current())
}

/// `rendered_invariants()` with no arguments: the exact bytes `ff-invariants`
/// writes, and the exact bytes the orinks.net validator is built from.
pub fn current_rendered_invariants() -> Result<String, String> {
    rendered_invariants(&world_data_root(), &CatalogInputs::current())
}

#[cfg(test)]
mod tests;
