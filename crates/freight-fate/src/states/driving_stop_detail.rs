//! Full details for one upcoming route stop, and the guard in front of
//! moving a planned stop (port of
//! `freight_fate/states/driving_stop_detail.py`).

use ff_core::pyfmt::fmt_f;
use ff_core::sim::trip::ETA_MIN_MPH;
use ff_core::sim::trip_models::RoadStop;

use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving_core::{hos_of, join_phrase, POI_ACTION_LABELS, POI_SERVICE_LABELS};
use crate::states::driving_menu_states::DriveRef;

/// Matches `Trip.eta_game_hours`: parked or crawling assumes a highway pace.
pub const FALLBACK_MPH: f64 = 55.0;

const STOP_DETAIL_INTRO_HELP: &str =
    "Up and down arrows review each line. Home and End jump to the first and last. Enter repeats \
     a line or activates a button. Escape returns to the map.";

/// Full details for one upcoming route stop, opened from the Map screen.
///
/// Mirrors the dispatch board's job details view: each fact is its own menu
/// row, then the plan/cancel buttons, then Back.
pub struct StopDetailState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    pub stop: RoadStop,
}

impl StopDetailState {
    pub fn new(driving: DriveRef, stop: RoadStop) -> Self {
        StopDetailState {
            menu: MenuCore::new("Stop details").with_intro_help(STOP_DETAIL_INTRO_HELP),
            driving,
            stop,
        }
    }

    fn detail_lines(&mut self, ctx: &mut GameContext) -> Vec<String> {
        let stop = self.stop.clone();
        self.driving
            .with(ctx, |d, ctx| {
                let ahead = (stop.at_mi - d.trip.position_mi).max(0.0);
                let mut lines = vec![format!(
                    "Stop: {}{}.",
                    d.trip.planned_prefix(&stop),
                    stop.spoken_name()
                )];
                if !stop.exit_label.is_empty() {
                    lines.push(format!("Exit: {}.", stop.exit_label));
                }
                lines.push(format!(
                    "Distance: {} ahead.",
                    ctx.settings.distance_text(ahead, false)
                ));
                let offers: Vec<String> = stop
                    .actions
                    .iter()
                    .filter_map(|action| {
                        POI_ACTION_LABELS
                            .iter()
                            .find(|(key, _)| key == action)
                            .map(|(_, label)| label.to_string())
                    })
                    .collect();
                let services: Vec<String> = stop
                    .services
                    .iter()
                    .map(|service| {
                        POI_SERVICE_LABELS
                            .iter()
                            .find(|(key, _)| key == service)
                            .map(|(_, label)| label.to_string())
                            .unwrap_or_else(|| service.replace('_', " "))
                    })
                    .collect();
                if !offers.is_empty() {
                    lines.push(format!("Offers: {}.", join_phrase(&offers)));
                }
                if !services.is_empty() {
                    lines.push(format!("Listed services: {}.", join_phrase(&services)));
                }
                if offers.is_empty() && services.is_empty() {
                    lines.push("Services not listed.".to_string());
                }
                let parking_text = stop.parking_text();
                if !parking_text.is_empty() {
                    lines.push(format!("Parking: {parking_text}."));
                }
                lines.push(eta_line(d, ctx, ahead));
                lines
            })
            .unwrap_or_default()
    }

    fn plan(&mut self, ctx: &mut GameContext) {
        let stop = self.stop.clone();
        let needs_confirm = self
            .driving
            .read(|d| d.trip.planned_stop_key.is_some() && !d.trip.is_planned(&stop))
            .unwrap_or(false);
        if needs_confirm {
            // A different stop is already planned: confirm before moving it.
            let state = ConfirmMovePlanState::new(self.driving.clone(), self.stop.clone());
            ctx.push_state(state);
            return;
        }
        self.set_planned_stop(ctx);
    }

    fn set_planned_stop(&mut self, ctx: &mut GameContext) {
        let key = self.stop.key();
        self.driving.read(|d| d.trip.planned_stop_key = Some(key));
        ctx.audio.play("ui/notify");
        self.refresh(ctx, true);
        let name = self.stop.spoken_name();
        ctx.say(&format!("Planned stop set, {name}."));
    }

    fn cancel(&mut self, ctx: &mut GameContext) {
        self.driving.read(|d| d.trip.planned_stop_key = None);
        self.refresh(ctx, true);
        ctx.say("Planned stop canceled.");
    }
}

/// Same rule as `Trip.eta_game_hours`, over the distance to this stop.
fn eta_line(d: &mut crate::states::driving::DrivingState, ctx: &GameContext, ahead: f64) -> String {
    let speed = d.trip.truck.speed_mph();
    let (mph, basis) = if speed >= ETA_MIN_MPH {
        (speed, "at your current speed")
    } else {
        (FALLBACK_MPH, "at a typical highway pace")
    };
    let eta_h = ahead / mph.max(1.0);
    let hos_note = hos_of(ctx).arrival_note(&ctx.settings.hos_mode, eta_h * 60.0);
    format!(
        "Estimated time to reach it: {} hours {basis}.{hos_note}",
        fmt_f(eta_h, 1)
    )
}

impl Menu for StopDetailState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = self
            .detail_lines(ctx)
            .into_iter()
            .map(|line| {
                let spoken = line.clone();
                MenuItem::new(line, move |_s: &mut Self, ctx: &mut GameContext| {
                    ctx.say(&spoken)
                })
                .help("Enter repeats this line.")
            })
            .collect();
        let stop = self.stop.clone();
        let planned = self
            .driving
            .read(|d| d.trip.planned_stop_key.clone())
            .flatten();
        let is_planned = self
            .driving
            .read(|d| d.trip.is_planned(&stop))
            .unwrap_or(false);
        if is_planned {
            items.push(
                MenuItem::new(
                    format!("Cancel planned stop at {}", self.stop.name),
                    |s: &mut Self, ctx| s.cancel(ctx),
                )
                .help("Forgets this planned stop."),
            );
        } else {
            let move_note = if planned.is_some() {
                " Your current planned stop moves here after you confirm."
            } else {
                ""
            };
            items.push(
                MenuItem::new(
                    format!("Plan to stop at {}", self.stop.name),
                    |s: &mut Self, ctx| s.plan(ctx),
                )
                .help(format!("Makes this your planned stop.{move_note}"))
                // `plan` (or the move confirmation) plays its own chime.
                .select_sound(None),
            );
        }
        items.push(
            MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Return to the map screen."),
        );
        items
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let current = self.current_text(ctx);
        ctx.say(&format!("Stop details. {STOP_DETAIL_INTRO_HELP} {current}"));
    }

    fn current_help(&self, ctx: &GameContext) -> String {
        let core = self.menu();
        let base = if core.items.is_empty() {
            core.intro_help.clone()
        } else {
            let item = &core.items[core.index];
            let help = item.help_text(self, ctx);
            if help.is_empty() {
                format!("{}.", item.text(self, ctx))
            } else {
                help
            }
        };
        format!("{STOP_DETAIL_INTRO_HELP} {base}")
    }
}

impl_state_for_menu!(StopDetailState);

/// Yes/No guard shown when planning a stop while another stop is planned.
///
/// Names the current planned stop and how far ahead it is, then asks whether
/// to move the plan here. Lands on "Yes" so one Enter completes the move the
/// player just asked for.
pub struct ConfirmMovePlanState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    pub stop: RoadStop,
}

const MOVE_PLAN_INTRO_HELP: &str = "Use up and down arrows to navigate, Enter to select. \
                                    Escape keeps your current planned stop.";

impl ConfirmMovePlanState {
    pub fn new(driving: DriveRef, stop: RoadStop) -> Self {
        ConfirmMovePlanState {
            menu: MenuCore::new("Move planned stop?").with_intro_help(MOVE_PLAN_INTRO_HELP),
            driving,
            stop,
        }
    }

    fn planned_stop(&self) -> Option<RoadStop> {
        self.driving
            .read(|d| d.trip.planned_stop().cloned())
            .flatten()
    }

    fn ahead_text(&self, ctx: &GameContext, stop: &RoadStop) -> String {
        let ahead = self
            .driving
            .read(|d| (stop.at_mi - d.trip.position_mi).max(0.0))
            .unwrap_or(0.0);
        ctx.settings.distance_text(ahead, false)
    }

    fn confirm(&mut self, ctx: &mut GameContext) {
        let key = self.stop.key();
        self.driving.read(|d| d.trip.planned_stop_key = Some(key));
        // Popping re-enters the detail screen, which now shows the Cancel
        // button.
        ctx.pop_state();
        ctx.audio.play("ui/notify");
        let name = self.stop.spoken_name();
        ctx.say_with(
            format!("Planned stop moved to {name}."),
            // interrupt: cut the detail screen's re-entry announcement
            Say::new(),
        );
    }
}

impl Menu for ConfirmMovePlanState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let keep_name = match self.planned_stop() {
            Some(stop) => stop.spoken_name(),
            None => self
                .driving
                .read(|d| d.trip.planned_stop_label())
                .unwrap_or_default(),
        };
        vec![
            MenuItem::new(
                format!("Yes, move plan to {}", self.stop.spoken_name()),
                |s: &mut Self, ctx| s.confirm(ctx),
            )
            .help("Move your planned stop to this one."),
            MenuItem::new(
                format!("No, keep planned stop at {keep_name}"),
                |s: &mut Self, ctx| s.go_back(ctx),
            )
            .help("Return to the stop details without moving your plan."),
        ]
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let where_ = match self.planned_stop() {
            Some(current) => format!(
                "{}, {} ahead",
                current.spoken_name(),
                self.ahead_text(ctx, &current)
            ),
            None => self
                .driving
                .read(|d| d.trip.planned_stop_label())
                .unwrap_or_default(),
        };
        let title = self.menu.title.clone();
        let target = self.stop.spoken_name();
        let stop = self.stop.clone();
        let ahead = self.ahead_text(ctx, &stop);
        let current = self.current_text(ctx);
        ctx.say(&format!(
            "{title} You already have a planned stop at {where_}. Move your plan to {target}, \
             {ahead} ahead? {current}"
        ));
    }
}

impl_state_for_menu!(ConfirmMovePlanState);
