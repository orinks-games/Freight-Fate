//! Truck rules at a real tunnel or bridge on a leg.
//!
//! A crossing is eager leg data, not lazy corridor detail: dispatch and the
//! route graph read it to plan a legal road before the truck leaves, so it
//! has to be known without parsing a corridor. The words travel with the
//! rule and its cited source, the way `TunnelData` carries its own.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use super::DataError;

/// Which placarded loads a crossing turns away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HazmatRule {
    /// No placarded-load rule is modelled at this crossing.
    None,
    /// The classes in `banned_classes` are banned outright and those in
    /// `bulk_banned_classes` in a cargo tank. A placarded load whose class
    /// is not known (an old save) matches no class list, only the rules for
    /// every placard, tank or bulk flammable load.
    Classes,
    /// Every placarded load is banned.
    All,
}

/// A US DOT hazard class or division (49 CFR 173.2), as the 49 CFR 172.101
/// table assigns it to a material and the placard shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum HazardClass {
    #[serde(rename = "1.1")]
    Explosive11,
    #[serde(rename = "1.2")]
    Explosive12,
    #[serde(rename = "1.3")]
    Explosive13,
    #[serde(rename = "1.4")]
    Explosive14,
    #[serde(rename = "1.5")]
    Explosive15,
    #[serde(rename = "1.6")]
    Explosive16,
    #[serde(rename = "2.1")]
    FlammableGas,
    #[serde(rename = "2.2")]
    NonflammableGas,
    #[serde(rename = "2.3")]
    PoisonGas,
    #[serde(rename = "3")]
    FlammableLiquid,
    #[serde(rename = "4.1")]
    FlammableSolid,
    #[serde(rename = "4.2")]
    SpontaneouslyCombustible,
    #[serde(rename = "4.3")]
    DangerousWhenWet,
    #[serde(rename = "5.1")]
    Oxidizer,
    #[serde(rename = "5.2")]
    OrganicPeroxide,
    #[serde(rename = "6.1")]
    Poison,
    #[serde(rename = "6.2")]
    Infectious,
    #[serde(rename = "7")]
    Radioactive,
    #[serde(rename = "8")]
    Corrosive,
    #[serde(rename = "9")]
    Miscellaneous,
}

impl HazardClass {
    /// The class or division number the placard shows, e.g. `2.3`.
    pub fn number(self) -> &'static str {
        match self {
            HazardClass::Explosive11 => "1.1",
            HazardClass::Explosive12 => "1.2",
            HazardClass::Explosive13 => "1.3",
            HazardClass::Explosive14 => "1.4",
            HazardClass::Explosive15 => "1.5",
            HazardClass::Explosive16 => "1.6",
            HazardClass::FlammableGas => "2.1",
            HazardClass::NonflammableGas => "2.2",
            HazardClass::PoisonGas => "2.3",
            HazardClass::FlammableLiquid => "3",
            HazardClass::FlammableSolid => "4.1",
            HazardClass::SpontaneouslyCombustible => "4.2",
            HazardClass::DangerousWhenWet => "4.3",
            HazardClass::Oxidizer => "5.1",
            HazardClass::OrganicPeroxide => "5.2",
            HazardClass::Poison => "6.1",
            HazardClass::Infectious => "6.2",
            HazardClass::Radioactive => "7",
            HazardClass::Corrosive => "8",
            HazardClass::Miscellaneous => "9",
        }
    }

    /// Class 1, every division.
    pub fn explosive(self) -> bool {
        matches!(
            self,
            HazardClass::Explosive11
                | HazardClass::Explosive12
                | HazardClass::Explosive13
                | HazardClass::Explosive14
                | HazardClass::Explosive15
                | HazardClass::Explosive16
        )
    }

    /// The class in plain words, as a refusal names it.
    pub fn spoken(self) -> &'static str {
        match self {
            c if c.explosive() => "explosives",
            HazardClass::FlammableGas => "flammable gas",
            HazardClass::NonflammableGas => "nonflammable gas",
            HazardClass::PoisonGas => "poison gas",
            HazardClass::FlammableLiquid => "flammable liquid",
            HazardClass::FlammableSolid => "flammable solid",
            HazardClass::SpontaneouslyCombustible => "spontaneously combustible",
            HazardClass::DangerousWhenWet => "dangerous when wet",
            HazardClass::Oxidizer => "an oxidizer",
            HazardClass::OrganicPeroxide => "an organic peroxide",
            HazardClass::Poison => "poison",
            HazardClass::Infectious => "infectious",
            HazardClass::Radioactive => "radioactive",
            HazardClass::Corrosive => "corrosive",
            _ => "a miscellaneous hazardous material",
        }
    }
}

/// Weekday hours a crossing is closed to some hazard classes, and what the
/// truck is told while it waits them out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HazmatHours {
    pub classes: Vec<HazardClass>,
    /// `HH:MM` to `HH:MM`, Monday to Friday.
    pub weekday_closed: Vec<[String; 2]>,
    pub text: String,
    pub source_url: String,
}

impl HazmatHours {
    /// Minutes until a closed window ends, or None when the load may cross
    /// at this local time of day. `weekend` is the career calendar's.
    pub fn wait_minutes(
        &self,
        load: &CrossingLoad,
        time: chrono::NaiveTime,
        weekend: bool,
    ) -> Option<i64> {
        use chrono::NaiveTime;
        if weekend || !load.may_be(&self.classes) {
            return None;
        }
        self.weekday_closed.iter().find_map(|[from, to]| {
            let from = NaiveTime::parse_from_str(from, "%H:%M").ok()?;
            let to = NaiveTime::parse_from_str(to, "%H:%M").ok()?;
            (from <= time && time < to).then(|| (to - time).num_minutes().max(1))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RoadCrossing {
    /// `tunnel` | `bridge`
    pub kind: String,
    /// Spoken name.
    pub name: String,
    pub hazmat: HazmatRule,
    /// Under `HazmatRule::Classes`: banned in any packaging.
    #[serde(default)]
    pub banned_classes: Vec<HazardClass>,
    /// Under `HazmatRule::Classes`: banned in a cargo tank (bulk).
    #[serde(default)]
    pub bulk_banned_classes: Vec<HazardClass>,
    /// Under `HazmatRule::Classes`: allowed only in amounts far below a
    /// placarded truckload, so a placarded load of them is refused.
    #[serde(default)]
    pub limited_classes: Vec<HazardClass>,
    /// Spoken when a `limited_classes` load is refused.
    #[serde(default)]
    pub limited_text: String,
    /// Placarded flammable liquid in bulk (a fuel tanker) is banned.
    pub bulk_flammable_banned: bool,
    /// Every cargo tank vehicle is banned, whatever it carries.
    pub tank_banned: bool,
    /// Regulated material carried without placards, in pounds; None means
    /// no such limit is modelled.
    pub regulated_lb_limit: Option<u32>,
    pub max_length_ft: Option<f64>,
    pub max_width_ft: Option<f64>,
    pub max_height_ft: Option<f64>,
    /// The cargo rule as spoken when it turns a load away.
    pub hazmat_text: String,
    /// The size rule as spoken; empty when no size limit is modelled.
    pub size_text: String,
    pub source_url: String,
    pub access_date: String,
    /// Which values were read from the source, derived, or assumed.
    pub basis: String,
    /// What sustained wind does to trucks here; None where none is published.
    #[serde(default)]
    pub wind: Option<WindRule>,
    /// Loads the crossing's own staff escort across in any weather; None
    /// where no escort rule is published.
    #[serde(default)]
    pub escort: Option<EscortRule>,
    /// Weekday hours the crossing is closed to some classes; None where
    /// none are published.
    #[serde(default)]
    pub hazmat_hours: Option<HazmatHours>,
}

/// A crossing that escorts some loads across, from its published rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EscortRule {
    /// Every placarded load is escorted.
    pub placarded: bool,
    /// Rigs grossing more than this many pounds are escorted.
    pub gross_over_lb: Option<u32>,
    /// Placarded loads of these classes are escorted.
    #[serde(default)]
    pub classes: Vec<HazardClass>,
    /// The rule as spoken when it applies.
    pub text: String,
    pub source_url: String,
}

impl EscortRule {
    pub fn applies(&self, load: &CrossingLoad) -> bool {
        (self.placarded && load.placarded)
            || self.gross_over_lb.is_some_and(|lb| load.gross_lb > lb)
            || (!self.classes.is_empty() && load.may_be(&self.classes))
    }
}

/// A crossing's published wind levels, in sustained miles per hour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WindRule {
    /// High-profile vehicles wait for an escort from this speed.
    pub escort_mph: Option<f64>,
    /// A tractor-trailer carrying less than `light_trailer_min_payload_lb`
    /// is turned away from this speed.
    pub light_trailer_mph: Option<f64>,
    #[serde(default)]
    pub light_trailer_min_payload_lb: u32,
    /// Every truck, with or without a trailer, is turned away from here.
    pub trucks_closed_mph: f64,
    /// The crossing closes to all traffic from here.
    pub closed_mph: f64,
    /// Spoken when an escort is required.
    #[serde(default)]
    pub escort_text: String,
    /// Spoken when the truck is turned away for wind.
    pub refused_text: String,
}

/// What the wind at a crossing means for this truck right now.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum WindDecision<'a> {
    Cross,
    Escort(&'a str),
    Refused(&'a str),
}

impl WindRule {
    pub fn decide(&self, load: &CrossingLoad, wind_mph: f64) -> WindDecision<'_> {
        let light = load.trailer && load.payload_lb < self.light_trailer_min_payload_lb;
        if wind_mph >= self.closed_mph
            || wind_mph >= self.trucks_closed_mph
            || (light && self.light_trailer_mph.is_some_and(|mph| wind_mph >= mph))
        {
            return WindDecision::Refused(&self.refused_text);
        }
        if self.escort_mph.is_some_and(|mph| wind_mph >= mph) {
            return WindDecision::Escort(&self.escort_text);
        }
        WindDecision::Cross
    }
}

/// The truck and load a crossing judges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CrossingLoad {
    pub length_ft: f64,
    pub width_ft: f64,
    pub height_ft: f64,
    pub placarded: bool,
    pub bulk_flammable: bool,
    pub tank: bool,
    pub regulated_lb: u32,
    /// A trailer is hooked up.
    pub trailer: bool,
    /// Cargo on board, in pounds.
    pub payload_lb: u32,
    /// The whole rig as it would cross a scale, in pounds; 0 when unknown.
    pub gross_lb: u32,
    /// The placarded load's hazard class; None when it is not known.
    pub hazard_class: Option<HazardClass>,
}

impl CrossingLoad {
    /// A rig with nothing regulated on it.
    pub fn plain(length_ft: f64, width_ft: f64, height_ft: f64) -> Self {
        CrossingLoad {
            length_ft,
            width_ft,
            height_ft,
            placarded: false,
            bulk_flammable: false,
            tank: false,
            regulated_lb: 0,
            trailer: false,
            payload_lb: 0,
            gross_lb: 0,
            hazard_class: None,
        }
    }

    pub fn with_gross(mut self, gross_lb: u32) -> Self {
        self.gross_lb = gross_lb;
        self
    }

    pub fn with_trailer(mut self, trailer: bool) -> Self {
        self.trailer = trailer;
        self
    }

    /// A stock rig: the full combination with a trailer, the tractor alone
    /// without one, at the default width and height.
    pub fn standard_rig(trailer: bool) -> Self {
        use crate::sim::cross_traffic::{COMBINATION_LENGTH_FT, TRACTOR_LENGTH_FT};
        let length_ft = if trailer {
            COMBINATION_LENGTH_FT
        } else {
            TRACTOR_LENGTH_FT
        };
        CrossingLoad::plain(length_ft, 8.5, 13.5).with_trailer(trailer)
    }

    /// The catalog's regulated freight: `hazardous` is placarded packaged
    /// material of the class `with_hazard_class` names, `fuel_bulk` is
    /// placarded flammable liquid in a cargo tank, and `chemicals` ride
    /// without placards but still count by weight.
    pub fn with_cargo(mut self, cargo_key: &str, tank: bool, cargo_lb: u32) -> Self {
        self.placarded = matches!(cargo_key, "hazardous" | "fuel_bulk");
        self.bulk_flammable = cargo_key == "fuel_bulk";
        self.hazard_class = self.bulk_flammable.then_some(HazardClass::FlammableLiquid);
        self.tank = tank;
        self.payload_lb = cargo_lb;
        self.regulated_lb = if matches!(cargo_key, "hazardous" | "fuel_bulk" | "chemicals") {
            cargo_lb
        } else {
            0
        };
        self
    }

    /// The placarded load's class, where the job names one. Packaged
    /// freight with no known class (an old save) is carried as unplacarded:
    /// no crossing takes it for a class it may not be.
    pub fn with_hazard_class(mut self, class: Option<HazardClass>) -> Self {
        if self.placarded && class.is_some() {
            self.hazard_class = class;
        } else if self.placarded && class.is_none() && !self.bulk_flammable {
            self.placarded = false;
        }
        self
    }

    /// A placarded load that is, or for all the crossing can tell may be,
    /// of one of these classes.
    pub fn may_be(&self, classes: &[HazardClass]) -> bool {
        self.placarded
            && !classes.is_empty()
            && self.hazard_class.is_some_and(|c| classes.contains(&c))
    }

    /// Whether any crossing rule could apply beyond size.
    pub fn regulated(&self) -> bool {
        self.placarded || self.tank || self.regulated_lb > 0
    }
}

impl RoadCrossing {
    /// The spoken reason this crossing turns the load away, or None.
    /// Size is judged before cargo, as at a real portal.
    pub fn refusal(&self, load: &CrossingLoad) -> Option<&str> {
        let over = |limit: Option<f64>, value: f64| limit.is_some_and(|l| value > l);
        if over(self.max_length_ft, load.length_ft)
            || over(self.max_width_ft, load.width_ft)
            || over(self.max_height_ft, load.height_ft)
        {
            return Some(&self.size_text);
        }
        let hazmat = load.placarded
            && match self.hazmat {
                HazmatRule::All => true,
                HazmatRule::Classes => self.class_banned(load),
                HazmatRule::None => load.bulk_flammable && self.bulk_flammable_banned,
            };
        let regulated = self
            .regulated_lb_limit
            .is_some_and(|limit| load.regulated_lb > limit);
        if hazmat || regulated || (load.tank && self.tank_banned) {
            return Some(&self.hazmat_text);
        }
        if self.hazmat == HazmatRule::Classes && load.may_be(&self.limited_classes) {
            return Some(&self.limited_text);
        }
        None
    }

    fn class_banned(&self, load: &CrossingLoad) -> bool {
        load.may_be(&self.banned_classes)
            || (load.tank && load.may_be(&self.bulk_banned_classes))
            || (load.bulk_flammable && self.bulk_flammable_banned)
    }

    /// `refusal`, naming the load's class first where a class rule turned
    /// a packaged load away, since the rule lists several.
    pub fn refusal_spoken(&self, load: &CrossingLoad) -> Option<String> {
        let reason = self.refusal(load)?;
        match load.hazard_class {
            Some(class)
                if self.hazmat == HazmatRule::Classes
                    && !load.tank
                    && ((reason == self.hazmat_text && self.banned_classes.contains(&class))
                        || (reason == self.limited_text
                            && self.limited_classes.contains(&class))) =>
            {
                Some(format!("This load is {}. {reason}", class.spoken()))
            }
            _ => Some(reason.to_string()),
        }
    }

    pub fn validate(&self, context: &str) -> Result<(), DataError> {
        let bad = |what: &str| DataError::value(format!("{context} crossing {what}"));
        if !matches!(self.kind.as_str(), "tunnel" | "bridge") {
            return Err(bad("has an unknown kind"));
        }
        if self.name.trim().is_empty() || self.basis.trim().is_empty() {
            return Err(bad("has no name or basis"));
        }
        if self.source_url.trim().is_empty() || !self.source_url.starts_with("https://") {
            return Err(bad("has no source URL"));
        }
        NaiveDate::parse_from_str(&self.access_date, "%Y-%m-%d")
            .map_err(|_| bad("has an invalid access date"))?;
        let sized = [self.max_length_ft, self.max_width_ft, self.max_height_ft];
        if sized.iter().flatten().any(|v| !v.is_finite() || *v <= 0.0) {
            return Err(bad("has an invalid size limit"));
        }
        if sized.iter().any(Option::is_some) && self.size_text.trim().is_empty() {
            return Err(bad("has a size limit with no spoken rule"));
        }
        let cargo_rule = self.hazmat != HazmatRule::None
            || self.bulk_flammable_banned
            || self.tank_banned
            || self.regulated_lb_limit.is_some();
        if cargo_rule && self.hazmat_text.trim().is_empty() {
            return Err(bad("has a cargo rule with no spoken rule"));
        }
        if self.hazmat == HazmatRule::Classes
            && self.banned_classes.is_empty()
            && self.bulk_banned_classes.is_empty()
            && self.limited_classes.is_empty()
        {
            return Err(bad("bans classes without naming any"));
        }
        if !self.limited_classes.is_empty() && self.limited_text.trim().is_empty() {
            return Err(bad("limits classes with no spoken rule"));
        }
        if let Some(hours) = &self.hazmat_hours {
            let times_ok = hours
                .weekday_closed
                .iter()
                .flatten()
                .all(|t| chrono::NaiveTime::parse_from_str(t, "%H:%M").is_ok());
            if hours.classes.is_empty()
                || hours.weekday_closed.is_empty()
                || !times_ok
                || hours.text.trim().is_empty()
                || !hours.source_url.starts_with("https://")
            {
                return Err(bad("has class hours with no classes, times or spoken rule"));
            }
        }
        if let Some(wind) = &self.wind {
            let speeds = [
                wind.escort_mph,
                wind.light_trailer_mph,
                Some(wind.trucks_closed_mph),
                Some(wind.closed_mph),
            ];
            if speeds.iter().flatten().any(|v| !v.is_finite() || *v <= 0.0)
                || wind.trucks_closed_mph > wind.closed_mph
            {
                return Err(bad("has invalid wind speeds"));
            }
            if wind.refused_text.trim().is_empty()
                || (wind.escort_mph.is_some() && wind.escort_text.trim().is_empty())
            {
                return Err(bad("has a wind rule with no spoken rule"));
            }
        }
        if let Some(escort) = &self.escort {
            if escort.text.trim().is_empty()
                || escort.source_url.trim().is_empty()
                || (!escort.placarded
                    && escort.gross_over_lb.is_none()
                    && escort.classes.is_empty())
            {
                return Err(bad(
                    "has an escort rule with nothing to escort or no spoken rule",
                ));
            }
        }
        if !cargo_rule
            && sized.iter().all(Option::is_none)
            && self.wind.is_none()
            && self.escort.is_none()
            && self.hazmat_hours.is_none()
        {
            return Err(bad("models no rule"));
        }
        let wind_texts = self
            .wind
            .iter()
            .flat_map(|w| [&w.escort_text, &w.refused_text])
            .chain(self.escort.iter().map(|e| &e.text))
            .chain(self.hazmat_hours.iter().map(|h| &h.text));
        for text in [&self.hazmat_text, &self.size_text, &self.limited_text]
            .into_iter()
            .chain(wind_texts)
        {
            if text.contains('$') {
                return Err(bad("speaks money as a symbol"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn crossing(hazmat: HazmatRule) -> RoadCrossing {
        RoadCrossing {
            kind: "tunnel".into(),
            name: "Test Tunnel".into(),
            hazmat,
            bulk_flammable_banned: false,
            tank_banned: false,
            regulated_lb_limit: None,
            max_length_ft: None,
            max_width_ft: None,
            max_height_ft: None,
            hazmat_text: "No hazardous materials.".into(),
            size_text: String::new(),
            source_url: "https://example.gov/rule".into(),
            access_date: "2026-10-11".into(),
            basis: "read".into(),
            wind: None,
            escort: None,
            banned_classes: Vec::new(),
            bulk_banned_classes: Vec::new(),
            limited_classes: Vec::new(),
            limited_text: String::new(),
            hazmat_hours: None,
        }
    }

    fn rig() -> CrossingLoad {
        CrossingLoad::plain(73.5, 8.5, 13.5)
    }

    #[test]
    fn all_placarded_turns_away_both_placarded_loads_only() {
        let c = crossing(HazmatRule::All);
        assert!(c
            .refusal(&rig().with_cargo("hazardous", false, 20_000))
            .is_some());
        assert!(c
            .refusal(&rig().with_cargo("fuel_bulk", true, 40_000))
            .is_some());
        assert!(c
            .refusal(&rig().with_cargo("chemicals", false, 20_000))
            .is_none());
        assert!(c
            .refusal(&rig().with_cargo("general", false, 20_000))
            .is_none());
    }

    #[test]
    fn class_bans_refuse_only_the_named_classes_and_unknown_loads() {
        let mut c = crossing(HazmatRule::Classes);
        c.banned_classes = vec![HazardClass::Explosive11, HazardClass::PoisonGas];
        c.bulk_banned_classes = vec![HazardClass::FlammableLiquid];
        let packaged = |class| {
            rig()
                .with_cargo("hazardous", false, 20_000)
                .with_hazard_class(Some(class))
        };
        assert!(c.refusal(&packaged(HazardClass::PoisonGas)).is_some());
        assert!(c.refusal(&packaged(HazardClass::Explosive11)).is_some());
        assert!(c.refusal(&packaged(HazardClass::Explosive14)).is_none());
        assert!(c.refusal(&packaged(HazardClass::FlammableLiquid)).is_none());
        assert!(
            c.refusal(&rig().with_cargo("hazardous", false, 20_000))
                .is_none(),
            "an old save's load names no class, so no class list matches it"
        );
        assert!(c
            .refusal(&rig().with_cargo("fuel_bulk", true, 40_000))
            .is_some());
        c.bulk_banned_classes.clear();
        assert!(c
            .refusal(&rig().with_cargo("fuel_bulk", true, 40_000))
            .is_none());
        c.bulk_flammable_banned = true;
        assert!(c
            .refusal(&rig().with_cargo("fuel_bulk", true, 40_000))
            .is_some());
        assert!(rig()
            .with_cargo("general", false, 20_000)
            .with_hazard_class(Some(HazardClass::PoisonGas))
            .hazard_class
            .is_none());
    }

    #[test]
    fn a_quantity_limited_class_is_refused_as_over_the_limits() {
        let mut c = crossing(HazmatRule::Classes);
        c.banned_classes = vec![HazardClass::PoisonGas];
        c.limited_classes = vec![HazardClass::FlammableLiquid];
        c.limited_text = "A placarded truckload is over the quantity limits.".into();
        let load = |class| {
            rig()
                .with_cargo("hazardous", false, 20_000)
                .with_hazard_class(Some(class))
        };
        assert_eq!(
            c.refusal_spoken(&load(HazardClass::FlammableLiquid))
                .as_deref(),
            Some(
                "This load is flammable liquid. A placarded truckload is over the quantity limits."
            )
        );
        assert_eq!(
            c.refusal_spoken(&load(HazardClass::PoisonGas)).as_deref(),
            Some("This load is poison gas. No hazardous materials.")
        );
        assert!(c.refusal(&load(HazardClass::NonflammableGas)).is_none());
        assert_eq!(
            c.refusal(&rig().with_cargo("hazardous", false, 20_000)),
            None,
            "an old save's load names no class and is not assumed to be limited"
        );
        assert!(c.validate("test").is_ok());
        c.limited_text.clear();
        assert!(c.validate("test").is_err());
    }

    #[test]
    fn a_class_refusal_names_the_class_once() {
        let mut c = crossing(HazmatRule::Classes);
        c.banned_classes = vec![HazardClass::Explosive11, HazardClass::PoisonGas];
        let gas = rig()
            .with_cargo("hazardous", false, 20_000)
            .with_hazard_class(Some(HazardClass::PoisonGas));
        assert_eq!(
            c.refusal_spoken(&gas).as_deref(),
            Some("This load is poison gas. No hazardous materials.")
        );
        let blast = gas.with_hazard_class(Some(HazardClass::Explosive11));
        assert_eq!(
            c.refusal_spoken(&blast).as_deref(),
            Some("This load is explosives. No hazardous materials.")
        );
        let unknown = rig()
            .with_cargo("hazardous", false, 20_000)
            .with_hazard_class(None);
        assert!(
            !unknown.placarded,
            "an unknown class is carried unplacarded"
        );
        assert_eq!(c.refusal_spoken(&unknown), None);
        let all = crossing(HazmatRule::All);
        assert_eq!(all.refusal_spoken(&unknown), None);
        assert_eq!(
            all.refusal_spoken(&gas).as_deref(),
            Some("No hazardous materials."),
            "a ban on every placard needs no class"
        );
    }

    #[test]
    fn class_hours_close_weekday_rush_hours_only() {
        let hours = HazmatHours {
            classes: vec![HazardClass::Explosive11, HazardClass::Explosive14],
            weekday_closed: vec![
                ["06:30".into(), "09:30".into()],
                ["16:00".into(), "19:00".into()],
            ],
            text: "Closed.".into(),
            source_url: "https://example.gov/rule".into(),
        };
        let at = |h: u32, m: u32| chrono::NaiveTime::from_hms_opt(h, m, 0).unwrap();
        let blast = rig()
            .with_cargo("hazardous", false, 20_000)
            .with_hazard_class(Some(HazardClass::Explosive11));
        assert_eq!(hours.wait_minutes(&blast, at(7, 0), false), Some(150));
        assert_eq!(hours.wait_minutes(&blast, at(18, 59), false), Some(1));
        assert_eq!(hours.wait_minutes(&blast, at(9, 30), false), None);
        assert_eq!(hours.wait_minutes(&blast, at(12, 0), false), None);
        assert_eq!(hours.wait_minutes(&blast, at(7, 0), true), None);
        let paint = blast.with_hazard_class(Some(HazardClass::FlammableLiquid));
        assert_eq!(hours.wait_minutes(&paint, at(7, 0), false), None);
        let unknown = rig()
            .with_cargo("hazardous", false, 20_000)
            .with_hazard_class(None);
        assert_eq!(
            hours.wait_minutes(&unknown, at(7, 0), false),
            None,
            "a load with no class is not taken for explosives"
        );
        let plain = rig().with_cargo("general", false, 20_000);
        assert_eq!(hours.wait_minutes(&plain, at(7, 0), false), None);
    }

    #[test]
    fn tank_regulated_weight_and_size_rules() {
        let mut c = crossing(HazmatRule::None);
        c.tank_banned = true;
        assert!(c
            .refusal(&rig().with_cargo("liquid_food", true, 40_000))
            .is_some());
        c.tank_banned = false;
        c.regulated_lb_limit = Some(400);
        assert!(c
            .refusal(&rig().with_cargo("chemicals", false, 401))
            .is_some());
        assert!(c
            .refusal(&rig().with_cargo("chemicals", false, 400))
            .is_none());
        c.max_height_ft = Some(13.5);
        c.size_text = "Too tall.".into();
        assert!(c.refusal(&rig()).is_none());
        assert_eq!(
            c.refusal(&CrossingLoad::plain(73.5, 8.5, 14.0)),
            Some("Too tall.")
        );
    }

    #[test]
    fn validation_requires_a_source_and_a_spoken_rule() {
        let mut c = crossing(HazmatRule::All);
        assert!(c.validate("t").is_ok());
        c.access_date = "yesterday".into();
        assert!(c.validate("t").is_err());
        let mut c = crossing(HazmatRule::All);
        c.hazmat_text.clear();
        assert!(c.validate("t").is_err());
        let mut c = crossing(HazmatRule::None);
        assert!(c.validate("t").is_err());
        c.max_height_ft = Some(13.5);
        assert!(c.validate("t").is_err());
        c.size_text = "Over 13 feet 6 inches needs a permit.".into();
        assert!(c.validate("t").is_ok());
    }

    #[test]
    fn wind_levels_escort_then_refuse_light_then_every_truck() {
        let rule = WindRule {
            escort_mph: Some(35.0),
            light_trailer_mph: Some(40.0),
            light_trailer_min_payload_lb: 15_000,
            trucks_closed_mph: 50.0,
            closed_mph: 60.0,
            escort_text: "escort".into(),
            refused_text: "refused".into(),
        };
        let empty = CrossingLoad::standard_rig(true);
        let loaded = CrossingLoad::standard_rig(true).with_cargo("general", false, 30_000);
        let bobtail = CrossingLoad::standard_rig(false);
        assert_eq!(rule.decide(&empty, 30.0), WindDecision::Cross);
        assert_eq!(rule.decide(&empty, 36.0), WindDecision::Escort("escort"));
        assert_eq!(rule.decide(&empty, 42.0), WindDecision::Refused("refused"));
        assert_eq!(rule.decide(&loaded, 42.0), WindDecision::Escort("escort"));
        assert_eq!(rule.decide(&bobtail, 42.0), WindDecision::Escort("escort"));
        assert_eq!(rule.decide(&loaded, 50.0), WindDecision::Refused("refused"));
        assert_eq!(
            rule.decide(&bobtail, 61.0),
            WindDecision::Refused("refused")
        );
    }
}
