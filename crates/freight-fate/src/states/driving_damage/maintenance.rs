//! Service thresholds, warnings, readouts, and roadside cost.

use crate::speech::{DeliveryStatus, EventPriority};
use ff_core::sim::vehicle::{
    TruckState, COMPONENT_SERVICE_LIMIT_PCT, COMPONENT_SERVICE_WARNING_PCT,
};
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::city_garage::{
    BRAKE_SERVICE_COST_PER_PCT, ENGINE_OVERHAUL_COST_PER_PCT, TIRE_SERVICE_COST_PER_PCT,
};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{BREAKDOWN_CALLOUT_FEE, DAMAGE_OUT_OF_SERVICE_PCT};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MaintenanceComponent {
    Tires,
    Brakes,
    Engine,
}

pub(super) const MAINTENANCE_COMPONENTS: [MaintenanceComponent; 3] = [
    MaintenanceComponent::Tires,
    MaintenanceComponent::Brakes,
    MaintenanceComponent::Engine,
];

impl MaintenanceComponent {
    fn warning_key(self) -> &'static str {
        match self {
            Self::Tires => "maintenance-warning:tires",
            Self::Brakes => "maintenance-warning:brakes",
            Self::Engine => "maintenance-warning:engine",
        }
    }

    pub(super) fn label(self) -> &'static str {
        match self {
            Self::Tires => "Tires",
            Self::Brakes => "Brakes",
            Self::Engine => "Engine",
        }
    }

    pub(super) fn wear(self, truck: &TruckState) -> f64 {
        match self {
            Self::Tires => truck.tire_wear_pct,
            Self::Brakes => truck.brake_wear_pct,
            Self::Engine => truck.engine_wear_pct,
        }
    }

    pub(super) fn clear(self, truck: &mut TruckState) {
        match self {
            Self::Tires => truck.tire_wear_pct = 0.0,
            Self::Brakes => truck.brake_wear_pct = 0.0,
            Self::Engine => truck.engine_wear_pct = 0.0,
        }
    }

    fn cost_per_pct(self) -> f64 {
        match self {
            Self::Tires => TIRE_SERVICE_COST_PER_PCT,
            Self::Brakes => BRAKE_SERVICE_COST_PER_PCT,
            Self::Engine => ENGINE_OVERHAUL_COST_PER_PCT,
        }
    }

    fn warning_effect(self) -> &'static str {
        match self {
            Self::Tires => "Worn tires reduce grip.",
            Self::Brakes => "Worn brakes reduce stopping force and fade sooner.",
            Self::Engine => "A worn engine loses power and burns more fuel.",
        }
    }

    pub(super) fn failure_name(self) -> &'static str {
        match self {
            Self::Tires => "Tire wear",
            Self::Brakes => "Brake wear",
            Self::Engine => "Engine wear",
        }
    }

    fn garage_action(self) -> &'static str {
        match self {
            Self::Tires => "Replace the tires in the garage",
            Self::Brakes => "Have the brakes relined in the garage",
            Self::Engine => "Get an engine overhaul in the garage",
        }
    }

    pub(super) fn service_action(self) -> &'static str {
        match self {
            Self::Tires => "replace the tires",
            Self::Brakes => "reline the brakes",
            Self::Engine => "overhaul the engine",
        }
    }

    pub(super) fn service_done(self) -> &'static str {
        match self {
            Self::Tires => "replaced the tires",
            Self::Brakes => "relined the brakes",
            Self::Engine => "overhauled the engine",
        }
    }
}

pub(super) fn natural_list(mut parts: Vec<String>) -> String {
    match parts.len() {
        0 => String::new(),
        1 => parts.remove(0),
        2 => format!("{} and {}", parts[0], parts[1]),
        _ => {
            let last = parts.pop().expect("a final list item");
            format!("{}, and {last}", parts.join(", "))
        }
    }
}

/// A component condition row for terminal and in-drive status screens.
pub fn maintenance_status_line(component: MaintenanceComponent, wear: f64) -> String {
    if wear >= COMPONENT_SERVICE_LIMIT_PCT {
        return format!(
            "{}: service required at {:.0} percent wear. {} before the truck can continue.",
            component.label(),
            COMPONENT_SERVICE_LIMIT_PCT,
            component.garage_action()
        );
    }
    if wear >= COMPONENT_SERVICE_WARNING_PCT {
        return format!(
            "{}: {wear:.0} percent worn. Service soon; service is required at {:.0} percent. {}.",
            component.label(),
            COMPONENT_SERVICE_LIMIT_PCT,
            component.garage_action()
        );
    }
    format!("{}: {wear:.0} percent worn.", component.label())
}

impl DrivingState {
    /// Current warning rung for tires, brakes, and engine: clear, warning, or limit.
    pub fn maintenance_wear_levels(&self) -> [u8; 3] {
        let mut levels = [0; 3];
        for (index, component) in MAINTENANCE_COMPONENTS.iter().copied().enumerate() {
            let wear = component.wear(&self.trip.truck);
            levels[index] = if wear >= COMPONENT_SERVICE_LIMIT_PCT {
                2
            } else if wear >= COMPONENT_SERVICE_WARNING_PCT {
                1
            } else {
                0
            };
        }
        levels
    }

    pub(super) fn maintenance_failures(&self) -> Vec<MaintenanceComponent> {
        MAINTENANCE_COMPONENTS
            .iter()
            .copied()
            .filter(|component| component.wear(&self.trip.truck) >= COMPONENT_SERVICE_LIMIT_PCT)
            .collect()
    }

    fn settle_maintenance_warnings(&mut self, ctx: &mut GameContext, interrupt_pending: bool) {
        for (index, component) in MAINTENANCE_COMPONENTS.iter().copied().enumerate() {
            let pending = self.maintenance_pending_levels[index];
            if pending == 0 {
                continue;
            }
            match ctx.event_delivery_status(component.warning_key()) {
                Some(DeliveryStatus::Pending) if !interrupt_pending => continue,
                Some(DeliveryStatus::Completed) => self.maintenance_levels[index] = pending,
                Some(DeliveryStatus::Pending | DeliveryStatus::Interrupted) | None => {
                    ctx.reset_event_condition(component.warning_key());
                }
            }
            self.maintenance_pending_levels[index] = 0;
        }
    }

    /// The player cut a wear warning off: it counts as heard, so the
    /// component is not warned about again at this level.
    pub(crate) fn acknowledge_maintenance_warnings(&mut self) {
        for index in 0..MAINTENANCE_COMPONENTS.len() {
            let pending = self.maintenance_pending_levels[index];
            if pending != 0 {
                self.maintenance_levels[index] = pending;
                self.maintenance_pending_levels[index] = 0;
            }
        }
    }

    pub fn prepare_warning_speech_pause(&mut self, ctx: &mut GameContext) {
        self.settle_last_hos_stop_warning(ctx, true);
        self.settle_maintenance_warnings(ctx, true);
    }

    /// Speak each advance warning once per wear episode. A repair below the
    /// warning threshold rearms that component.
    pub(super) fn update_maintenance_warnings(&mut self, ctx: &mut GameContext) -> bool {
        self.settle_maintenance_warnings(ctx, false);
        let levels = self.maintenance_wear_levels();
        let mut limit_crossed = false;
        let mut warning_slot_open = !ctx.event_delivery_pending();
        for (index, component) in MAINTENANCE_COMPONENTS.iter().copied().enumerate() {
            let previous = self.maintenance_levels[index];
            let current = levels[index];
            if self.maintenance_pending_levels[index] > current {
                self.maintenance_pending_levels[index] = 0;
                ctx.reset_event_condition(component.warning_key());
            }
            if current < previous {
                self.maintenance_levels[index] = current;
                self.maintenance_pending_levels[index] = 0;
                ctx.reset_event_condition(component.warning_key());
                continue;
            }
            if current <= previous {
                continue;
            }
            if current == 2 {
                self.maintenance_pending_levels[index] = 0;
                ctx.reset_event_condition(component.warning_key());
                self.maintenance_levels[index] = current;
                limit_crossed = true;
                continue;
            }
            if self.maintenance_pending_levels[index] != 0 {
                continue;
            }
            if !warning_slot_open {
                continue;
            }
            let wear = component.wear(&self.trip.truck);
            let message = format!(
                "{}: {wear:.0} percent worn. Service soon. {} {} before {:.0} percent wear.",
                component.label(),
                component.warning_effect(),
                component.garage_action(),
                COMPONENT_SERVICE_LIMIT_PCT
            );
            let key = component.warning_key();
            ctx.reset_event_condition(key);
            self.maintenance_pending_levels[index] = current;
            ctx.audio.play("ui/warning");
            ctx.say_event_with(
                message,
                SayEvent::queued()
                    .priority(EventPriority::Important)
                    .key(key)
                    .category(SpeechCategory::Safety)
                    .receipt(),
            );
            warning_slot_open = false;
        }
        limit_crossed
    }

    pub(super) fn roadside_service_cost(&self) -> f64 {
        let damage_failed = self.trip.truck.damage_pct >= DAMAGE_OUT_OF_SERVICE_PCT;
        let base = if damage_failed {
            self.roadside_repair_cost()
        } else {
            BREAKDOWN_CALLOUT_FEE
        };
        base + self
            .maintenance_failures()
            .iter()
            .map(|component| component.wear(&self.trip.truck) * component.cost_per_pct())
            .sum::<f64>()
    }
}
