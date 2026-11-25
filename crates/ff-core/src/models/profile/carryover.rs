//! A 1.9 career carried into 2.0 on a `Profile`: the home fix the copy
//! makes, the one reader and writer of `carryover`, and the "Choose a
//! carrier" choice (see `models::carryover`).

use serde_json::{Map, Value};

use super::{older_career_line, Profile, CREATED_LINE};
use crate::data::world::{get_world, World};
use crate::models::business_constants::is_owner_operator;
use crate::models::carriers::{Carrier, CarrierOpening};
use crate::models::carryover::{carrier_reshaped, Carryover, CarryoverEvent};
use crate::models::start_options::{
    hiring_carrier_for_option, start_options_for_home_city, CareerStartOption,
};

/// One row of the "Choose a carrier" menu.
#[derive(Debug, Clone)]
pub struct CarrierChoiceRow {
    pub carrier: &'static Carrier,
    /// The start option whose summary describes this carrier's work.
    pub option: &'static CareerStartOption,
    /// The terminal the driver would be based at.
    pub terminal_city: String,
    /// The driver's own carrier: staying.
    pub stay: bool,
}

/// A 1.9 career arriving in 2.0 as a dict, a cloud backup restored here:
/// the copy-forward's stamp and carry-over written into it, once (the stamp
/// means a second pass finds nothing to do). True when it was a 1.9 career.
pub fn carry_over_1_9_dict(map: &mut Map<String, Value>) -> bool {
    if older_career_line(map).is_none() {
        return false;
    }
    let mut profile = Profile::from_dict(map);
    profile.carry_over_from_1_9();
    map.insert("created_line".into(), Value::from(CREATED_LINE));
    let carried = profile.to_dict();
    for key in ["carryover", "home_city", "home_terminal_city"] {
        if let Some(value) = carried.get(key) {
            map.insert(key.into(), value.clone());
        }
    }
    true
}

impl Profile {
    /// Move the carry-over along. Returns the new state, or `None` when the
    /// event changes nothing from here.
    pub fn apply_carryover_event(&mut self, event: CarryoverEvent) -> Option<Carryover> {
        let next = self.carryover.next(event)?;
        self.carryover = next;
        Some(next)
    }

    /// Any carrier change, by any path: a choice still owed is settled with
    /// the carrier that caused it.
    pub(crate) fn note_carrier_changed(&mut self) {
        self.apply_carryover_event(CarryoverEvent::CarrierChanged);
    }

    /// The copy from 1.9 (or a career an earlier 2.0 build copied): fix the
    /// home and send `CopiedFrom19`. The truck never moves.
    ///
    /// Home is the city the first delivery loaded in (1.9's "Home terminal"
    /// pick in nearly every career), written to `home_city` because the
    /// player chose it. With no such record the truck's city stands in, and
    /// `home_city` stays blank (#255: a derived city never becomes the
    /// driver's home). A carrier that no longer hires there keeps that city
    /// as the home terminal city until the choice is made, never another
    /// carrier's terminal.
    pub fn carry_over_from_1_9(&mut self) {
        let world = get_world();
        let first_delivery = self
            .achievement_stats
            .get("home_city")
            .and_then(Value::as_str)
            .map(|city| world.resolve_city_key(city))
            .filter(|key| world.cities.contains_key(key));
        let lives = match &first_delivery {
            Some(city) => {
                self.home_city = city.clone();
                city.clone()
            }
            None if !self.home_city.trim().is_empty() => world.resolve_city_key(&self.home_city),
            None => world.resolve_city_key(&self.current_city),
        };
        let hiring = self.hiring_carrier();
        self.home_terminal_city = hiring
            .and_then(|c| c.home_terminal_city(world, &lives))
            .unwrap_or_else(|| lives.clone());
        self.apply_carryover_event(CarryoverEvent::CopiedFrom19 {
            carrier_reshaped: carrier_reshaped(hiring),
        });
    }

    /// The choice is owed: the state says so and the carrier that caused it
    /// is still the driver's. A state left behind by a change no path
    /// reported (own authority, say) reads as not due.
    pub fn carrier_choice_due(&self) -> bool {
        self.carryover == Carryover::CarrierChoiceDue && carrier_reshaped(self.hiring_carrier())
    }

    /// Settle a choice whose carrier is gone. True when it changed.
    pub fn settle_stale_carrier_choice(&mut self) -> bool {
        if self.carryover == Carryover::CarrierChoiceDue && !self.carrier_choice_due() {
            self.note_carrier_changed();
            return true;
        }
        false
    }

    /// Where the driver lives, for the choice: the home city, else the
    /// home terminal city the copy set from it.
    pub fn carryover_home(&self) -> String {
        get_world().resolve_city_key(&self.driver_home_city())
    }

    /// Whether the driver's carrier still hires where they live.
    pub fn carrier_still_serves_home(&self) -> bool {
        let home = self.carryover_home();
        self.hiring_carrier()
            .is_some_and(|c| c.hires_in(get_world(), &home))
    }

    /// What the terminal says on entering while the choice is due.
    pub fn carrier_choice_notice(&self) -> Option<String> {
        let carrier = self.hiring_carrier()?;
        let world = get_world();
        let area = &carrier.lane_area;
        let line = if self.carrier_still_serves_home() {
            format!(
                "{} now runs freight only in {area}. Stay with them or choose another \
                 carrier. Your money, level, trucks and driving record stay.",
                carrier.name
            )
        } else {
            format!(
                "{} now runs freight only in {area}. It no longer dispatches from {}. Choose a \
                 carrier to keep driving. Your money, level, trucks and driving record stay.",
                carrier.name,
                world.spoken_city(&self.carryover_home(), None)
            )
        };
        Some(line)
    }

    /// The menu's carrier rows: staying first, then every other carrier
    /// that hires where the driver lives, in the start picker's order.
    pub fn carrier_choice_rows(&self, world: &World) -> Vec<CarrierChoiceRow> {
        let Some(own) = self.hiring_carrier() else {
            return Vec::new();
        };
        let home = self.carryover_home();
        let mut rows = Vec::new();
        let stay_terminal = if own.hires_in(world, &home) {
            own.hiring_terminal_city(world, &home)
        } else {
            own.nearest_terminal_city(world, &home)
        };
        let own_option = crate::models::start_options::option_for_carrier(&own.key, &own.name);
        if let Some(terminal_city) = stay_terminal {
            rows.push(CarrierChoiceRow {
                carrier: own,
                option: own_option,
                terminal_city,
                stay: true,
            });
        }
        for option in start_options_for_home_city(world, &home) {
            if !option.is_company_driver() {
                continue;
            }
            let Some(carrier) = hiring_carrier_for_option(option) else {
                continue;
            };
            if carrier.key == own.key || rows.iter().any(|r| r.carrier.key == carrier.key) {
                continue;
            }
            let Some(terminal_city) = carrier.hiring_terminal_city(world, &home) else {
                continue;
            };
            rows.push(CarrierChoiceRow {
                carrier,
                option,
                terminal_city,
                stay: false,
            });
        }
        rows
    }

    /// Make the choice. Returns the line to speak.
    ///
    /// Only the seat and the home move. Money, level, reputation, trucks,
    /// upgrades, credentials, achievements and the driving record stay; a
    /// company driver joining another carrier goes through
    /// [`Profile::join_carrier_opening`] as any hire does.
    pub fn choose_carrier(&mut self, world: &World, row: &CarrierChoiceRow) -> String {
        let owner = is_owner_operator(&self.business_status);
        let city = world.spoken_city(&row.terminal_city, None);
        let name = row.carrier.name.clone();
        let terminal = world.resolve_city_key(&row.terminal_city);
        self.dispatch_board_cache = None;
        if !row.stay {
            if owner {
                self.carrier_key = row.carrier.key.clone();
                self.carrier_name = name.clone();
                self.home_terminal_city = terminal;
                self.note_carrier_changed();
                return format!(
                    "You are now leased on to {name}, out of their {city} terminal. Your truck \
                     stays where it is parked. The dispatch board is open again."
                );
            }
            let opening = CarrierOpening {
                carrier: row.carrier,
                terminal_city: terminal,
                home_city: self.carryover_home(),
                moves_home: false,
            };
            self.join_carrier_opening(&opening);
            return format!(
                "{name} has taken you on. You start at their {city} terminal, where your \
                 assigned truck is waiting. The dispatch board is open again."
            );
        }
        let moves_home = !self.carrier_still_serves_home();
        if moves_home {
            self.home_city = terminal.clone();
        }
        self.home_terminal_city = terminal.clone();
        self.apply_carryover_event(CarryoverEvent::StayedWithCarrier);
        if owner {
            return if moves_home {
                format!(
                    "You stay leased on to {name}. Your home is now {city}. Your truck stays \
                     where it is parked. The dispatch board is open again."
                )
            } else {
                format!("You stay leased on to {name}. The dispatch board is open again.")
            };
        }
        let already_there = world.resolve_city_key(&self.current_city) == terminal;
        if self.active_trip.is_none() {
            self.current_city = terminal.clone();
            // As joining a carrier does: the terminal is where the truck
            // starts, not a facility it is parked at.
            self.parked_facility.clear();
        }
        if moves_home {
            format!(
                "You stay with {name}. Your home is now {city}. You start at their {city} \
                 terminal, where your assigned truck is waiting. The dispatch board is open \
                 again."
            )
        } else if already_there {
            format!("You stay with {name}. The dispatch board is open again.")
        } else {
            format!(
                "You stay with {name}. You start at their {city} terminal, where your assigned \
                 truck is waiting. The dispatch board is open again."
            )
        }
    }
}
