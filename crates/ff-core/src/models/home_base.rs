//! Where the truck is parked, and the career's carrier home terminal.
//!
//! The hub's "parked at" line comes from where the truck really is:
//!
//! 0. a generic impound lot in the current city after a police tow
//!    (`parked_facility` is [`IMPOUND_LOT_FACILITY`]), before anything else;
//! 1. the hiring carrier's own terminal ("{Carrier} {City} terminal") when
//!    the truck is in the career's `home_terminal_city`;
//! 2. otherwise the facility the truck last delivered or dropped at, when it
//!    is in the current city;
//! 3. otherwise the current city's `travel_center`, then `truck_parking` pin;
//! 4. otherwise just the city (no invented yard).
//!
//! It is never a yard in another city, never a world `terminal` or
//! `company_yard` pin standing in as a home, and never a bare "Terminal".

use crate::data::world::World;
use crate::data::world_models::HomeTerminal;
use crate::models::carriers::{hiring_carrier, Carrier};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParkedKind {
    /// The hiring carrier's own terminal in the home terminal city.
    CarrierTerminal,
    /// The facility the truck just delivered or dropped at.
    Facility,
    /// A public `travel_center` or `truck_parking` lot in the city.
    PublicLot,
    /// The city itself: nothing honest to name.
    City,
    /// A generic impound lot after a police tow ([`IMPOUND_LOT_FACILITY`]).
    Impound,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParkedAt {
    pub kind: ParkedKind,
    /// The place name; for [`ParkedKind::City`] the spoken city.
    pub name: String,
    /// Resolved city key the truck is in.
    pub city_key: String,
}

impl ParkedAt {
    /// " at {name}" for a named place, "" when only the city is known, so a
    /// caller can write `format!("Parked{} in the ...", p.at_clause())`.
    pub fn at_clause(&self) -> String {
        match self.kind {
            ParkedKind::City => String::new(),
            ParkedKind::Impound => " at an impound lot".to_string(),
            _ => format!(" at {}", self.name),
        }
    }

    /// "at {name} in {city}" or "in {city}".
    pub fn phrase(&self, world: &World) -> String {
        let city = world.spoken_city(&self.city_key, None);
        match self.kind {
            ParkedKind::City => format!("in {city}"),
            ParkedKind::Impound => format!("at an impound lot in {city}"),
            _ => format!("at {} in {city}", self.name),
        }
    }

    /// "at {name} in the {city} service area", or "in the {city} service
    /// area" when only the city is known, so a sentence reads "Parked {}."
    /// without inventing a place.
    pub fn service_area_phrase(&self, world: &World) -> String {
        let area = city_service_area(&world.spoken_city(&self.city_key, None));
        match self.kind {
            ParkedKind::City => format!("in {area}"),
            // An impound lot is in the city, never a "service area".
            ParkedKind::Impound => format!("at an impound lot in {}", self.city_text(world)),
            _ => format!("at {} in {area}", self.name),
        }
    }

    /// The city an impound lot is in, the way the tow summary says it:
    /// "Buffalo, New York", "The Dalles, Oregon".
    fn city_text(&self, world: &World) -> String {
        impound_lot_text(world, &self.city_key)
            .trim_start_matches("an impound lot in ")
            .to_string()
    }

    /// The name a logbook line or menu title uses.
    pub fn place(&self) -> &str {
        &self.name
    }
}

/// "the {city} service area", dropping the article when the spoken city
/// already starts with one: "The Dalles service area", never "the The
/// Dalles service area".
pub fn city_service_area(city: &str) -> String {
    format!("{} service area", crate::speech_text::the_city(city))
}

/// `parked_facility` value for a truck towed to an impound lot after a
/// police arrest. Not a world pin: the map has no real tow or impound yards,
/// so the lot is generic and always in the stop city. Old saves never carry
/// it, and it round-trips as the plain string it is.
pub const IMPOUND_LOT_FACILITY: &str = "impound_lot";

/// How a tow summary names the lot: "an impound lot in Buffalo, New York",
/// the city and state the way the hub says them.
pub fn impound_lot_text(world: &World, city_key: &str) -> String {
    let key = world.resolve_city_key(city_key);
    match world.cities.get(&key) {
        Some(city) if !city.state.is_empty() => {
            format!("an impound lot in {}, {}", city.name, city.state)
        }
        Some(city) => format!("an impound lot in {}", city.name),
        None => format!("an impound lot in {}", world.spoken_city(city_key, None)),
    }
}

/// The city a roadside stop at `(lat, lon)` happened in: the nearest world
/// city, among the cities of `state_hint` (a state or province code or
/// name, from the road the truck was on) when any match, so a stop never
/// lands across a state line or a border. `None` only for an empty world.
pub fn stop_city(world: &World, lat: f64, lon: f64, state_hint: &str) -> Option<String> {
    use crate::data::world::air_miles;
    let hint = state_hint.trim();
    let in_state = |c: &crate::data::world_models::City| {
        !hint.is_empty()
            && (c.state_code.eq_ignore_ascii_case(hint) || c.state.eq_ignore_ascii_case(hint))
    };
    let any_in_state = world.cities.values().any(in_state);
    world
        .cities
        .iter()
        .filter(|(_, c)| !any_in_state || in_state(c))
        .min_by(|(_, a), (_, b)| {
            air_miles(lat, lon, a.lat, a.lon).total_cmp(&air_miles(lat, lon, b.lat, b.lon))
        })
        .map(|(key, _)| key.clone())
}

/// The career fields [`parked_at`] reads.
#[derive(Debug, Clone, Copy)]
pub struct ParkedInputs<'a> {
    pub carrier_key: &'a str,
    pub carrier_name: &'a str,
    pub business_status: &'a str,
    pub home_terminal_city: &'a str,
    pub current_city: &'a str,
    pub parked_facility: &'a str,
}

/// Where the truck is parked (see the module docs for the order).
pub fn parked_at(world: &World, inputs: ParkedInputs<'_>) -> ParkedAt {
    let here = world.resolve_city_key(inputs.current_city);
    // A towed truck is in the impound lot, whatever else this city has --
    // the carrier terminal included.
    if inputs.parked_facility.trim() == IMPOUND_LOT_FACILITY {
        return ParkedAt {
            kind: ParkedKind::Impound,
            // The bare city, as the hub's own line says it ("Buffalo");
            // the hub adds the state after the service area.
            name: format!(
                "{} impound lot",
                world
                    .cities
                    .get(&here)
                    .map(|c| c.name.clone())
                    .unwrap_or_else(|| world.spoken_city(&here, None))
            ),
            city_key: here,
        };
    }
    let carrier = hiring_carrier(
        inputs.carrier_key,
        inputs.carrier_name,
        inputs.business_status,
    );
    if let Some(terminal) = carrier_terminal_here(world, carrier, inputs.home_terminal_city, &here)
    {
        return ParkedAt {
            kind: ParkedKind::CarrierTerminal,
            name: terminal,
            city_key: here,
        };
    }
    let Some(city) = world.cities.get(&here) else {
        return ParkedAt {
            kind: ParkedKind::City,
            name: inputs.current_city.to_string(),
            city_key: here,
        };
    };
    let facility = inputs.parked_facility.trim();
    if !facility.is_empty() {
        if let Some(loc) = city
            .locations
            .iter()
            .find(|l| l.name == facility || l.id == facility)
        {
            return ParkedAt {
                kind: ParkedKind::Facility,
                name: loc.name.clone(),
                city_key: here,
            };
        }
    }
    for lot_type in ["travel_center", "truck_parking"] {
        if let Some(loc) = city.locations.iter().find(|l| l.facility_type == lot_type) {
            return ParkedAt {
                kind: ParkedKind::PublicLot,
                name: loc.name.clone(),
                city_key: here,
            };
        }
    }
    ParkedAt {
        kind: ParkedKind::City,
        name: world.spoken_city(&here, Some(true)),
        city_key: here,
    }
}

fn carrier_terminal_here(
    world: &World,
    carrier: Option<&Carrier>,
    home_terminal_city: &str,
    here: &str,
) -> Option<String> {
    let carrier = carrier?;
    if world.resolve_city_key(home_terminal_city) != here {
        return None;
    }
    carrier.terminal_name(world, here)
}

/// The career's carrier home terminal, when it has a hiring carrier. A stale
/// `home_terminal_city` (not one of this carrier's terminal cities, e.g.
/// after a carrier change) reads as that carrier's nearest terminal to it,
/// within the hiring radius for a regional.
pub fn carrier_home_terminal(
    world: &World,
    carrier_key: &str,
    carrier_name: &str,
    business_status: &str,
    home_terminal_city: &str,
) -> Option<HomeTerminal> {
    let carrier = hiring_carrier(carrier_key, carrier_name, business_status)?;
    carrier
        .home_terminal(world, home_terminal_city)
        .or_else(|| {
            let home = carrier.home_terminal_city(world, home_terminal_city)?;
            carrier.home_terminal(world, &home)
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::world::get_world;
    use crate::models::business::{COMPANY_DRIVER, INDEPENDENT_AUTHORITY};

    fn inputs<'a>(
        carrier_key: &'a str,
        business_status: &'a str,
        home: &'a str,
        here: &'a str,
        facility: &'a str,
    ) -> ParkedInputs<'a> {
        ParkedInputs {
            carrier_key,
            carrier_name: "",
            business_status,
            home_terminal_city: home,
            current_city: here,
            parked_facility: facility,
        }
    }

    fn has_lot(city: &crate::data::world_models::City, kind: &str) -> bool {
        city.locations.iter().any(|l| l.facility_type == kind)
    }

    #[test]
    fn test_the_stop_city_is_the_nearest_city_even_beside_a_travel_center() {
        let world = get_world();
        let healy = &world.cities["healy_ak_us"];
        assert!(has_lot(healy, "travel_center"));
        let key = stop_city(world, healy.lat + 0.02, healy.lon, "AK").expect("a city");
        assert_eq!(key, "healy_ak_us");
    }

    #[test]
    fn test_the_stop_city_stays_on_its_side_of_the_blaine_border() {
        let world = get_world();
        let blaine = &world.cities["blaine_wa_us"];
        let surrey = &world.cities["surrey_bc_ca"];
        // Just north of the midpoint, nearer Surrey: the road still says
        // Washington until the crossing, so the stop is in Blaine.
        let lat = (blaine.lat + surrey.lat) / 2.0 + 0.02;
        let lon = (blaine.lon + surrey.lon) / 2.0;
        assert_eq!(
            stop_city(world, lat, lon, &blaine.state_code).as_deref(),
            Some("blaine_wa_us")
        );
        assert_eq!(
            stop_city(world, lat, lon, &surrey.state_code).as_deref(),
            Some("surrey_bc_ca")
        );
        // With no state from the road, plain nearest.
        assert_eq!(
            stop_city(world, blaine.lat, blaine.lon, "").as_deref(),
            Some("blaine_wa_us")
        );
    }

    #[test]
    fn test_an_impound_lot_wins_over_the_carrier_terminal_and_every_pin() {
        let world = get_world();
        let p = parked_at(
            world,
            inputs(
                "northstar",
                COMPANY_DRIVER,
                "chicago_il_us",
                "chicago_il_us",
                IMPOUND_LOT_FACILITY,
            ),
        );
        assert_eq!(p.kind, ParkedKind::Impound);
        assert_eq!(p.name, "Chicago impound lot");
        assert_eq!(p.at_clause(), " at an impound lot");
        assert_eq!(
            p.service_area_phrase(world),
            "at an impound lot in Chicago, Illinois"
        );
        assert!(world.cities["chicago_il_us"]
            .locations
            .iter()
            .all(|l| l.name != IMPOUND_LOT_FACILITY && l.id != IMPOUND_LOT_FACILITY));
        assert_eq!(
            impound_lot_text(world, "chicago_il_us"),
            "an impound lot in Chicago, Illinois"
        );
        let dalles = parked_at(
            world,
            inputs(
                "northstar",
                COMPANY_DRIVER,
                "chicago_il_us",
                "the_dalles_or_us",
                IMPOUND_LOT_FACILITY,
            ),
        );
        assert_eq!(
            dalles.service_area_phrase(world),
            "at an impound lot in The Dalles, Oregon"
        );
        assert_eq!(
            impound_lot_text(world, "the_dalles_or_us"),
            "an impound lot in The Dalles, Oregon"
        );
    }

    #[test]
    fn test_city_service_area_drops_the_article_for_the_dalles() {
        assert_eq!(city_service_area("The Dalles"), "The Dalles service area");
        assert_eq!(city_service_area("Chicago"), "the Chicago service area");
        // Only a leading "The " word counts, not a city that merely starts
        // with the letters.
        assert_eq!(city_service_area("Theodore"), "the Theodore service area");
        let world = get_world();
        let p = parked_at(
            world,
            inputs(
                "northstar",
                COMPANY_DRIVER,
                "chicago_il_us",
                "the_dalles_or_us",
                "",
            ),
        );
        assert_eq!(p.city_key, "the_dalles_or_us");
        let phrase = p.service_area_phrase(world);
        assert!(phrase.ends_with("in The Dalles service area"), "{phrase}");
        assert!(!phrase.contains("the The"), "{phrase}");
    }

    #[test]
    fn test_parked_at_carrier_terminal_in_home_terminal_city() {
        let world = get_world();
        // Delivered facility in the home city still reads the carrier yard:
        // home logic follows home_terminal_city.
        let p = parked_at(
            world,
            inputs(
                "northstar",
                COMPANY_DRIVER,
                "chicago_il_us",
                "Chicago",
                "Chicago Dry Warehouse",
            ),
        );
        assert_eq!(p.kind, ParkedKind::CarrierTerminal);
        assert_eq!(p.name, "Northstar Freight Lines Chicago terminal");
        assert_eq!(p.city_key, "chicago_il_us");
        assert_eq!(
            p.phrase(world),
            "at Northstar Freight Lines Chicago terminal in Chicago"
        );
    }

    #[test]
    fn test_parked_at_delivered_facility_away_from_home() {
        let world = get_world();
        let milwaukee = &world.cities["milwaukee_wi_us"];
        let facility = milwaukee
            .locations
            .iter()
            .find(|l| !matches!(l.facility_type.as_str(), "company_yard" | "terminal"))
            .expect("a Milwaukee facility");
        let p = parked_at(
            world,
            inputs(
                "northstar",
                COMPANY_DRIVER,
                "chicago_il_us",
                "milwaukee_wi_us",
                &facility.name,
            ),
        );
        assert_eq!(p.kind, ParkedKind::Facility);
        assert_eq!(p.name, facility.name);
        // A delivered yard pin is honest too: the truck really is there.
        let yard = world.yard_pin_name("milwaukee_wi_us").expect("yard pin");
        let p = parked_at(
            world,
            inputs(
                "northstar",
                COMPANY_DRIVER,
                "chicago_il_us",
                "milwaukee_wi_us",
                yard,
            ),
        );
        assert_eq!((p.kind, p.name.as_str()), (ParkedKind::Facility, yard));
    }

    #[test]
    fn test_parked_at_ignores_a_facility_in_another_city() {
        let world = get_world();
        let p = parked_at(
            world,
            inputs(
                "northstar",
                COMPANY_DRIVER,
                "chicago_il_us",
                "healy_ak_us",
                "Chicago Dry Warehouse",
            ),
        );
        assert_ne!(p.kind, ParkedKind::Facility);
        assert!(!p.name.contains("Chicago"), "{}", p.name);
    }

    #[test]
    fn test_parked_at_travel_center_then_truck_parking() {
        let world = get_world();
        let p = parked_at(
            world,
            inputs(
                "northstar",
                COMPANY_DRIVER,
                "chicago_il_us",
                "healy_ak_us",
                "",
            ),
        );
        assert_eq!(p.kind, ParkedKind::PublicLot);
        assert!(p.name.starts_with("Fisher Fuel"), "{}", p.name);
        let (key, city) = world
            .cities
            .iter()
            .find(|(_, c)| has_lot(c, "truck_parking") && !has_lot(c, "travel_center"))
            .expect("a city with only truck parking");
        let lot = city
            .locations
            .iter()
            .find(|l| l.facility_type == "truck_parking")
            .unwrap();
        let p = parked_at(
            world,
            inputs("northstar", COMPANY_DRIVER, "chicago_il_us", key, ""),
        );
        assert_eq!(
            (p.kind, p.name.as_str()),
            (ParkedKind::PublicLot, lot.name.as_str())
        );
    }

    #[test]
    fn test_parked_at_city_when_nothing_honest_to_name() {
        let world = get_world();
        let (key, city) = world
            .cities
            .iter()
            .find(|(k, c)| {
                k.as_str() != "chicago_il_us"
                    && !has_lot(c, "travel_center")
                    && !has_lot(c, "truck_parking")
            })
            .expect("a city with no public lot");
        let p = parked_at(
            world,
            inputs("northstar", COMPANY_DRIVER, "chicago_il_us", key, ""),
        );
        assert_eq!(p.kind, ParkedKind::City);
        assert_eq!(p.at_clause(), "");
        assert!(p.phrase(world).starts_with("in "), "{}", p.phrase(world));
        // Never the city's own yard pin standing in as a home.
        if let Some(yard) = world.yard_pin_name(key) {
            assert_ne!(p.name, yard);
        }
        assert_ne!(p.name, "Terminal");
        assert!(p.name.contains(&city.name));
    }

    #[test]
    fn test_parked_at_never_a_carrier_terminal_without_a_carrier_or_off_its_map() {
        let world = get_world();
        // Own authority: no carrier yard, even in Chicago.
        let p = parked_at(
            world,
            inputs(
                "northstar",
                INDEPENDENT_AUTHORITY,
                "chicago_il_us",
                "Chicago",
                "",
            ),
        );
        assert_ne!(p.kind, ParkedKind::CarrierTerminal);
        assert_ne!(Some(p.name.as_str()), world.yard_pin_name("chicago_il_us"));
        // Home city is not one of this carrier's terminal cities.
        let p = parked_at(
            world,
            inputs(
                "great_lakes_training",
                COMPANY_DRIVER,
                "chicago_il_us",
                "Chicago",
                "",
            ),
        );
        assert_ne!(p.kind, ParkedKind::CarrierTerminal);
        // Truck is away from home: the home terminal is not where it is.
        let p = parked_at(
            world,
            inputs(
                "northstar",
                COMPANY_DRIVER,
                "chicago_il_us",
                "Milwaukee",
                "",
            ),
        );
        assert_ne!(p.kind, ParkedKind::CarrierTerminal);
        assert!(!p.name.contains("Northstar"), "{}", p.name);
    }

    #[test]
    fn test_carrier_home_terminal_reads_stale_home_as_nearest_terminal() {
        let world = get_world();
        let t = carrier_home_terminal(
            world,
            "great_lakes_training",
            "",
            COMPANY_DRIVER,
            "chicago_il_us",
        )
        .expect("great lakes terminal");
        assert_eq!(t.name, "Great Lakes Training Transport Milwaukee terminal");
        assert!(carrier_home_terminal(
            world,
            "northstar",
            "",
            INDEPENDENT_AUTHORITY,
            "chicago_il_us"
        )
        .is_none());
    }
}
