//! Planning a stop by what it is for: the purpose list T opens while rolling,
//! and the plan each row makes.
//!
//! The driver names the need (fuel, a break, sleep, both fuel and sleep, or
//! food) and the plan is the next stop ahead that meets it. With hours of
//! service planning hints on, a break or sleep plan takes the stop the hours
//! advice recommends, as T did before it asked.

use ff_core::sim::trip_models::RoadStop;

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_menu_states::StopPurposeState;
use crate::states::driving_team::pilot::FUEL_STOP_FRACTION;

use super::stops::{HosRestChoice, HosRestPurpose};

/// What the driver wants the planned stop for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopPlanPurpose {
    Fuel,
    /// The 30-minute break.
    Break,
    Sleep,
    FuelAndSleep,
    Food,
}

impl StopPlanPurpose {
    /// The purpose list's rows, in order.
    pub const ALL: [StopPlanPurpose; 5] = [
        StopPlanPurpose::Fuel,
        StopPlanPurpose::Break,
        StopPlanPurpose::Sleep,
        StopPlanPurpose::FuelAndSleep,
        StopPlanPurpose::Food,
    ];

    /// The row in the purpose list: the purpose, then the distance to its
    /// stop, or `None` when no stop ahead serves it. One whole literal per
    /// row, so a catalog translates each as a unit.
    pub fn row_label(self, distance: Option<&str>) -> String {
        match (self, distance) {
            (StopPlanPurpose::Fuel, Some(distance)) => format!("Fuel: {distance}"),
            (StopPlanPurpose::Break, Some(distance)) => format!("Break: {distance}"),
            (StopPlanPurpose::Sleep, Some(distance)) => format!("Sleep: {distance}"),
            (StopPlanPurpose::FuelAndSleep, Some(distance)) => {
                format!("Fuel and sleep: {distance}")
            }
            (StopPlanPurpose::Food, Some(distance)) => format!("Food and coffee: {distance}"),
            (StopPlanPurpose::Fuel, None) => "Fuel: none ahead".to_string(),
            (StopPlanPurpose::Break, None) => "Break: none ahead".to_string(),
            (StopPlanPurpose::Sleep, None) => "Sleep: none ahead".to_string(),
            (StopPlanPurpose::FuelAndSleep, None) => "Fuel and sleep: none ahead".to_string(),
            (StopPlanPurpose::Food, None) => "Food and coffee: none ahead".to_string(),
        }
    }

    /// The row's F1 help.
    pub fn help(self) -> &'static str {
        match self {
            StopPlanPurpose::Fuel => "Plans the next stop that sells diesel.",
            StopPlanPurpose::Break => {
                "Plans a stop for your 30-minute break, the recommended one with planning hints \
                 on."
            }
            StopPlanPurpose::Sleep => {
                "Plans a stop with parking for a 10-hour sleep, the recommended one with planning \
                 hints on."
            }
            StopPlanPurpose::FuelAndSleep => "Plans the next stop where you can fuel and sleep.",
            StopPlanPurpose::Food => "Plans the next stop with food and coffee.",
        }
    }

    /// The name a saved drive stores it under.
    pub fn save_key(self) -> &'static str {
        match self {
            StopPlanPurpose::Fuel => "fuel",
            StopPlanPurpose::Break => "break",
            StopPlanPurpose::Sleep => "sleep",
            StopPlanPurpose::FuelAndSleep => "fuel_and_sleep",
            StopPlanPurpose::Food => "food",
        }
    }

    pub fn from_save_key(key: &str) -> Option<StopPlanPurpose> {
        StopPlanPurpose::ALL
            .into_iter()
            .find(|purpose| purpose.save_key() == key)
    }

    /// Whether the purpose needs diesel, so the plan owes the fuel range.
    fn wants_fuel(self) -> bool {
        matches!(self, StopPlanPurpose::Fuel | StopPlanPurpose::FuelAndSleep)
    }

    /// The line spoken when nothing ahead serves the purpose.
    fn none_ahead(self) -> &'static str {
        match self {
            StopPlanPurpose::Fuel => "No fuel stop ahead.",
            StopPlanPurpose::Break => "No break stop ahead.",
            StopPlanPurpose::Sleep => "No sleep stop ahead. Shoulder sleep available.",
            StopPlanPurpose::FuelAndSleep => "No stop with fuel and sleep ahead.",
            StopPlanPurpose::Food => "No food stop ahead.",
        }
    }
}

/// The stop a purpose would plan, and how to say it.
pub struct StopPlanChoice {
    pub stop: RoadStop,
    prefix: &'static str,
    /// The stop serves the 30-minute break itself.
    for_break: bool,
}

fn has(stop: &RoadStop, action: &str) -> bool {
    stop.actions.iter().any(|a| a == action)
}

impl DrivingState {
    /// T while rolling, once nothing else claims the key: ask what the stop
    /// is for.
    pub fn push_stop_purpose_menu(&mut self, ctx: &mut GameContext) {
        let rows: Vec<(StopPlanPurpose, Option<f64>)> = StopPlanPurpose::ALL
            .into_iter()
            .map(|purpose| {
                let ahead = self
                    .stop_plan_choice(ctx, purpose)
                    .map(|choice| 0.0f64.max(choice.stop.at_mi - self.trip.position_mi));
                (purpose, ahead)
            })
            .collect();
        let focus = self.likely_stop_purpose(ctx);
        // Nothing the list's entry reads lives on the drive, so the ordinary
        // push works from inside the drive's key handler.
        let state = StopPurposeState::new(ctx, rows, focus);
        ctx.push_state(state);
    }

    /// The row the purpose list opens on: fuel when the tank is low or will
    /// not reach the next pumps, else what the hours advice calls for.
    fn likely_stop_purpose(&self, ctx: &GameContext) -> StopPlanPurpose {
        let (range, needed, _) = self.fuel_range_need();
        if self.trip.truck.fuel_fraction() < FUEL_STOP_FRACTION || range < needed {
            return StopPlanPurpose::Fuel;
        }
        match self.hos_rest_choice(ctx) {
            HosRestChoice::Recommended {
                purpose: HosRestPurpose::Break | HosRestPurpose::SleepForBreak,
                ..
            } => StopPlanPurpose::Break,
            HosRestChoice::Recommended { .. } | HosRestChoice::NoReachable => {
                StopPlanPurpose::Sleep
            }
            HosRestChoice::NotNeeded => StopPlanPurpose::Fuel,
        }
    }

    /// The next usable stop ahead that `serves`, nearest first.
    fn next_stop_where(&self, serves: impl Fn(&RoadStop) -> bool) -> Option<RoadStop> {
        self.trip
            .stops
            .iter()
            .filter(|stop| {
                stop.at_mi > self.trip.position_mi
                    && stop.accessible_to(self.trip.bobtail)
                    && serves(stop)
            })
            .min_by(|a, b| a.at_mi.total_cmp(&b.at_mi))
            .cloned()
    }

    /// The stop `purpose` would plan from here, if any.
    pub fn stop_plan_choice(
        &self,
        ctx: &GameContext,
        purpose: StopPlanPurpose,
    ) -> Option<StopPlanChoice> {
        let sleeps = |stop: &RoadStop| has(stop, "sleep") && stop.parking != "none";
        let plain = |stop: RoadStop, prefix| StopPlanChoice {
            stop,
            prefix,
            for_break: false,
        };
        match purpose {
            StopPlanPurpose::Fuel => self
                .next_stop_where(RoadStop::sells_fuel)
                .map(|stop| plain(stop, "Fuel stop planned")),
            StopPlanPurpose::FuelAndSleep => self
                .next_stop_where(|stop| stop.sells_fuel() && sleeps(stop))
                .map(|stop| plain(stop, "Fuel and sleep stop planned")),
            StopPlanPurpose::Food => self
                .next_stop_where(|stop| has(stop, "food"))
                .map(|stop| plain(stop, "Food stop planned")),
            StopPlanPurpose::Sleep => match self.hos_rest_choice(ctx) {
                HosRestChoice::Recommended {
                    stop,
                    purpose: HosRestPurpose::Sleep,
                    fallback,
                } => Some(plain(
                    *stop,
                    if fallback {
                        "Sleep stop planned, the last one before your sleep limit"
                    } else {
                        "Sleep stop planned"
                    },
                )),
                HosRestChoice::NoReachable => self.next_stop_where(sleeps).map(|stop| {
                    plain(
                        stop,
                        "Nearest sleep stop planned, but none is reachable before your hours \
                         limit. Stop somewhere safe sooner",
                    )
                }),
                _ => self
                    .next_stop_where(sleeps)
                    .map(|stop| plain(stop, "Sleep stop planned")),
            },
            StopPlanPurpose::Break => match self.hos_rest_choice(ctx) {
                HosRestChoice::Recommended {
                    stop,
                    purpose: HosRestPurpose::Break,
                    fallback,
                } => Some(StopPlanChoice {
                    stop: *stop,
                    prefix: if fallback {
                        "Break stop planned, the last one before your break limit"
                    } else {
                        "Break stop planned"
                    },
                    for_break: true,
                }),
                HosRestChoice::Recommended {
                    stop,
                    purpose: HosRestPurpose::SleepForBreak,
                    fallback,
                } => Some(plain(
                    *stop,
                    if fallback {
                        "Sleep stop planned for your break, the last one before your break limit"
                    } else {
                        "Sleep stop planned for your break"
                    },
                )),
                _ => {
                    let break_stop =
                        self.next_stop_where(|stop| has(stop, "break") && stop.parking != "none");
                    let sleep_stop = self.next_stop_where(sleeps);
                    match (break_stop, sleep_stop) {
                        (Some(stop), Some(sleep)) if sleep.at_mi < stop.at_mi => {
                            Some(plain(sleep, "Sleep stop planned for your break"))
                        }
                        (Some(stop), _) => Some(StopPlanChoice {
                            stop,
                            prefix: "Break stop planned",
                            for_break: true,
                        }),
                        (None, Some(sleep)) => {
                            Some(plain(sleep, "Sleep stop planned for your break"))
                        }
                        (None, None) => None,
                    }
                }
            },
        }
    }

    /// Plan the stop a row of the purpose list asked for, and say it.
    pub fn plan_stop_for(&mut self, ctx: &mut GameContext, purpose: StopPlanPurpose) {
        let Some(choice) = self.stop_plan_choice(ctx, purpose) else {
            let line = purpose.none_ahead();
            self.set_status(line);
            self.say_plain(ctx, line);
            return;
        };
        let candidate = choice.stop;
        if let Some(current) = self.trip.planned_stop().cloned() {
            if current.key() != candidate.key() {
                self.set_status(format!("Planned stop remains {}.", current.spoken_name()));
                self.say_plain(
                    ctx,
                    format!(
                        "Planned stop stays {}. {} is also ahead.",
                        current.spoken_name(),
                        candidate.spoken_name()
                    ),
                );
                return;
            }
        }
        self.trip.planned_stop_key = Some(candidate.key());
        self.selected_stop_key = Some(candidate.key());
        self.selected_stop_break = choice.for_break;
        self.selected_stop_purpose = Some(purpose);
        self.selected_stop_assist_armed = false;
        self.selected_stop_assist_said = false;
        let range = self.fuel_range_mi();
        let note = if purpose.wants_fuel() && candidate.at_mi - self.trip.position_mi > range {
            let range_text = ctx.settings.distance_text(range, false);
            let line = format!("Fuel range only {range_text}.");
            format!(" {line}")
        } else {
            String::new()
        };
        self.speak_selected_rest_stop(ctx, &candidate, choice.prefix, &note);
    }
}
