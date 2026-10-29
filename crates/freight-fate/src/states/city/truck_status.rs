//! The terminal's truck-status reader: one navigable fact per row.

use ff_core::models::business::carrier_name;
use ff_core::models::carrier_fleet::{equipment_status_lines, fleet_assignment_text, slip_seats};
use ff_core::models::trucks::{truck_model, truck_model_or_panic};
use ff_core::pyfmt::fmt_f;

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::city::profile;
use crate::states::driving_damage::{maintenance_status_line, MaintenanceComponent};

/// The Assignment row's help while the driver slip-seats.
const SLIP_SEAT_HELP: &str = "Until level 9, dispatch matches a spare to each load, and each \
     spare keeps its own fuel and wear. From level 9 you keep one truck for every load until \
     dispatch moves you up to the next fleet.";

/// Reviewable terminal status for the active tractor.
pub struct TruckStatusState {
    menu: MenuCore<Self>,
}

impl TruckStatusState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Truck status").with_intro_help(
                "Up and down review the lines. Enter repeats a line. Escape returns to the \
                 terminal.",
            ),
        }
    }

    /// The active tractor's status, split into independently reviewable facts.
    pub fn status_lines(ctx: &GameContext) -> Vec<String> {
        let p = profile(ctx);
        let specs = p.truck_specs();
        let truck =
            truck_model(&p.active_truck_key()).unwrap_or_else(|| truck_model_or_panic("rig"));
        let fuel_pct = p.truck_fuel_gal() / specs.fuel_tank_gal * 100.0;
        let damage = p.truck_damage_pct();
        let condition = if damage < 5.0 {
            "excellent"
        } else if damage < 20.0 {
            "good"
        } else if damage < 50.0 {
            "worn"
        } else {
            "poor"
        };
        let compound = if p.tire_type() == "winter" {
            "winter"
        } else {
            "all-season"
        };
        let chains = if !p.chains_owned() {
            "Snow chains: none aboard.".to_string()
        } else if p.chain_wear_pct() >= 100.0 {
            "Snow chains: the set aboard is snapped scrap.".to_string()
        } else if p.chain_wear_pct() >= 1.0 {
            format!(
                "Snow chains: aboard, {} percent worn.",
                fmt_f(p.chain_wear_pct(), 0)
            )
        } else {
            "Snow chains: aboard and fresh.".to_string()
        };

        let mut lines = if p.owns_equipment() {
            vec![
                format!("Assignment: owned tractor, {}.", truck.label),
                "Eligibility: owned tractor, carrier fleet rules do not apply.".to_string(),
            ]
        } else {
            // How slip-seating works is the row's F1 help, not the row.
            let assignment = format!(
                "Assignment: assigned {} tractor. {}",
                carrier_name(p),
                fleet_assignment_text(p)
            );
            let mut lines = vec![assignment];
            lines.extend(
                equipment_status_lines(p)
                    .into_iter()
                    .map(|line| format!("Eligibility: {line}")),
            );
            lines
        };

        let tire_wear = p.tire_wear_pct();
        let tire_line = if tire_wear >= ff_core::sim::vehicle::COMPONENT_SERVICE_WARNING_PCT {
            format!(
                "{} Compound: {compound}.",
                maintenance_status_line(MaintenanceComponent::Tires, tire_wear)
            )
        } else {
            format!(
                "Tire wear: {} percent, {compound} compound.",
                fmt_f(tire_wear, 0)
            )
        };
        let brake_wear = p.brake_wear_pct();
        let brake_line = if brake_wear >= ff_core::sim::vehicle::COMPONENT_SERVICE_WARNING_PCT {
            maintenance_status_line(MaintenanceComponent::Brakes, brake_wear)
        } else {
            format!("Brake wear: {} percent.", fmt_f(brake_wear, 0))
        };
        let engine_wear = p.engine_wear_pct();
        let engine_line = if engine_wear >= ff_core::sim::vehicle::COMPONENT_SERVICE_WARNING_PCT {
            maintenance_status_line(MaintenanceComponent::Engine, engine_wear)
        } else {
            format!("Engine wear: {} percent.", fmt_f(engine_wear, 0))
        };
        lines.extend([
            format!(
                "Fuel: {} percent, {} gallons of {}.",
                fmt_f(fuel_pct, 0),
                fmt_f(p.truck_fuel_gal(), 0),
                fmt_f(specs.fuel_tank_gal, 0),
            ),
            format!(
                "Tractor condition: {condition}, {} percent damage.",
                fmt_f(damage, 0)
            ),
            tire_line,
            brake_line,
            engine_line,
            format!("Road grime: {} percent.", fmt_f(p.road_grime_pct(), 0)),
            chains,
        ]);
        lines
    }
}

impl Default for TruckStatusState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for TruckStatusState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let slip_seating = {
            let p = profile(ctx);
            !p.owns_equipment() && slip_seats(p)
        };
        let mut items: Vec<MenuItem<Self>> = Self::status_lines(ctx)
            .into_iter()
            .map(|line| {
                let help = if slip_seating && line.starts_with("Assignment: ") {
                    SLIP_SEAT_HELP
                } else {
                    "Repeat this truck-status line."
                };
                let spoken = line.clone();
                MenuItem::new(line, move |_s: &mut Self, ctx| ctx.say(&spoken)).help(help)
            })
            .collect();
        items.push(
            MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Back to the terminal menu."),
        );
        items
    }
}

impl_state_for_menu!(TruckStatusState);
