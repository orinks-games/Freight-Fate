//! The driver tablet: the launcher and one app of spoken lines at a time
//! (`DriverAppsState` / `DriverAppScreenState`).

use ff_core::sim::hos;
use ff_core::sim::trip_models::RoadStop;

use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving_menu_states::DriveRef;
use crate::states::driving_radio_app::RadioAppState;

/// Accessible driver tablet launcher.
pub struct DriverAppsState {
    menu: MenuCore<Self>,
    driving: DriveRef,
}

const APPS: [(&str, &str, &str); 7] = [
    ("Radio", "radio", "Tune, search, and save radio stations."),
    (
        "Navigation",
        "navigation",
        "GPS guidance, route progress, and exit context.",
    ),
    (
        "Weather",
        "weather",
        "Conditions, forecast, and safe-speed guidance.",
    ),
    (
        "Traffic",
        "traffic",
        "Traffic pace and reported slowdowns ahead.",
    ),
    (
        "Truck stops",
        "truck_stops",
        "Upcoming route stops and their services.",
    ),
    (
        "Road chatter",
        "road_chatter",
        "Local driver reports and road chatter.",
    ),
    ("ELD", "eld", "Hours of service and legal-stop guidance."),
];

const APPS_INTRO_HELP: &str = "Enter opens the app. Escape returns to the status screens.";

impl DriverAppsState {
    pub fn new(driving: DriveRef) -> Self {
        DriverAppsState {
            menu: MenuCore::new("Driver apps").with_intro_help(APPS_INTRO_HELP),
            driving,
        }
    }

    fn open_app(&mut self, ctx: &mut GameContext, app_key: &str) {
        if app_key == "radio" {
            let state = RadioAppState::new(self.driving.clone());
            ctx.push_state(state);
            return;
        }
        let state = DriverAppScreenState::new(ctx, self.driving.clone(), app_key);
        ctx.push_state(state);
    }
}

impl Menu for DriverAppsState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = APPS
            .iter()
            .map(|(label, key, help_text)| {
                let key = key.to_string();
                MenuItem::new(*label, move |s: &mut Self, ctx: &mut GameContext| {
                    s.open_app(ctx, &key)
                })
                .help(*help_text)
            })
            .collect();
        items.push(
            MenuItem::new("Back to status screens", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Return to the status screen list."),
        );
        items
    }
}

impl_state_for_menu!(DriverAppsState);

/// One driver tablet app as a reviewable spoken list.
pub struct DriverAppScreenState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    pub app_key: String,
    weather_status: Option<&'static str>,
    weather_refresh_failed: bool,
    weather_refresh_issue_announced: bool,
}

const APP_INTRO_HELP: &str =
    "Up and down review the lines. Enter repeats a line. Escape returns to Driver apps.";

fn app_title(app_key: &str) -> &'static str {
    match app_key {
        "navigation" => "Navigation",
        "weather" => "Weather",
        "traffic" => "Traffic",
        "truck_stops" => "Truck stops",
        "road_chatter" => "Road chatter",
        "eld" => "ELD",
        _ => "Driver app",
    }
}

impl DriverAppScreenState {
    pub fn new(ctx: &mut GameContext, driving: DriveRef, app_key: &str) -> Self {
        let weather = if app_key == "weather" {
            driving.with(ctx, |d, _| {
                (
                    d.trip.weather.source_status(),
                    d.trip.weather.live_weather_refresh_failed(),
                )
            })
        } else {
            None
        };
        DriverAppScreenState {
            menu: MenuCore::new(app_title(app_key)).with_intro_help(APP_INTRO_HELP),
            driving,
            app_key: app_key.to_string(),
            weather_status: weather.map(|(status, _)| status),
            weather_refresh_failed: weather.is_some_and(|(_, failed)| failed),
            weather_refresh_issue_announced: false,
        }
    }

    fn lines(&mut self, ctx: &mut GameContext) -> Vec<String> {
        match self.app_key.as_str() {
            "weather" => self.weather_lines(ctx),
            "traffic" => self.traffic_lines(ctx),
            "truck_stops" => self.truck_stop_lines(ctx),
            "road_chatter" => self.road_chatter_lines(ctx),
            "eld" => self.eld_lines(ctx),
            _ => self.navigation_lines(ctx),
        }
    }

    fn navigation_lines(&mut self, ctx: &mut GameContext) -> Vec<String> {
        self.driving
            .with(ctx, |d, ctx| {
                let imperial = ctx.settings.imperial_units;
                vec![
                    format!("Navigation: {}", d.trip.next_navigation_context(imperial)),
                    format!("Route progress: {}", d.trip.progress_summary(imperial)),
                    format!("Next listed exit: {}", d.trip.next_exit_context()),
                ]
            })
            .unwrap_or_default()
    }

    fn weather_lines(&mut self, ctx: &mut GameContext) -> Vec<String> {
        self.driving
            .with(ctx, |d, ctx| {
                let imperial = ctx.settings.imperial_units;
                let mut source = d.trip.weather.source_label().to_string();
                if d.trip.weather.live_weather_refreshing() {
                    source += ". Live weather is updating for your current location";
                } else if d.trip.weather.live_weather_refresh_failed() {
                    source += ". The latest live weather check failed";
                }
                let mut lines = vec![
                    format!("Weather source: {source}."),
                    format!(
                        "Observation age: {}.",
                        d.trip.weather.observation_age_value()
                    ),
                    format!(
                        "Conditions: {}.",
                        d.trip.weather.source_conditions(imperial)
                    ),
                    format!(
                        "Safe speed guidance: about {}.",
                        ctx.settings
                            .speed_text(d.trip.weather.effects().safe_speed_mph)
                    ),
                ];
                if d.trip.weather.has_simulated_forecast() {
                    let forecast: Vec<String> = d
                        .trip
                        .weather
                        .forecast(2)
                        .iter()
                        .map(|kind| kind.value().to_string())
                        .collect();
                    lines.push(format!("Forecast ahead: {}.", forecast.join(", then ")));
                } else {
                    lines.push("Forecast ahead: unavailable.".to_string());
                }
                // A warning was spoken once as it arrived and then lived only
                // in message review; a player asked how to hear a special
                // weather statement again while driving (Mastodon, 2026-10-05).
                if !d.trip.live_alerts.is_empty() {
                    let alerts: Vec<String> = d
                        .trip
                        .live_alerts
                        .iter()
                        .map(|alert| alert.spoken())
                        .collect();
                    lines.push(format!("Weather alerts: {}.", alerts.join("; ")));
                }
                lines
            })
            .unwrap_or_default()
    }

    fn traffic_lines(&mut self, ctx: &mut GameContext) -> Vec<String> {
        let next = self.next_traffic_line(ctx);
        self.driving
            .with(ctx, |d, ctx| {
                if d.trip.traffic_context().is_some() {
                    return vec![d.trip.npc_traffic_status()];
                }
                vec![next.unwrap_or_else(|| {
                    format!(
                        "Traffic: no reported pinch in the next {}.",
                        ctx.settings.distance_text(20.0, false)
                    )
                })]
            })
            .unwrap_or_default()
    }

    fn truck_stop_lines(&mut self, ctx: &mut GameContext) -> Vec<String> {
        self.driving
            .with(ctx, |d, ctx| {
                let stops = upcoming_stops(d, 100.0, 3);
                if stops.is_empty() {
                    return vec![format!(
                        "Truck stops: no listed route stop in the next {}.",
                        ctx.settings.distance_text(100.0, false)
                    )];
                }
                stops
                    .into_iter()
                    .map(|stop| {
                        let ahead = (stop.at_mi - d.trip.position_mi).max(0.0);
                        format!(
                            "Truck stops: {} in {}; {}.",
                            stop.spoken_name(),
                            ctx.settings.distance_text(ahead, false),
                            crate::states::driving_core::poi_offers_text(&stop)
                        )
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn road_chatter_lines(&mut self, ctx: &mut GameContext) -> Vec<String> {
        vec![
            self.road_chatter_line(ctx),
            "Road chatter: reports are informal and may be stale.".to_string(),
        ]
    }

    fn eld_lines(&mut self, ctx: &mut GameContext) -> Vec<String> {
        self.driving
            .with(ctx, |d, ctx| {
                let summary = d.player_hos_now(ctx).summary(&ctx.settings.hos_mode);
                let summary = summary.trim_end_matches('.');
                let mut lines = vec![format!("ELD: {summary}")];
                lines.extend(d.team_eld_line(ctx));
                let context = d.hos_route_context(ctx);
                if context.is_empty() {
                    lines.push("ELD route note: no legal stop warning.".to_string());
                } else {
                    lines.push(format!("ELD route note: {context}"));
                }
                lines.push(
                    "ELD keys: Alt A time at the wheel, Alt S when the break is due, \
                     Alt D what ends this shift."
                        .to_string(),
                );
                lines
            })
            .unwrap_or_default()
    }

    fn next_traffic_line(&mut self, ctx: &mut GameContext) -> Option<String> {
        self.driving.with(ctx, |d, _ctx| {
            let pos = d.trip.position_mi;
            for lead in d.trip.npc_vehicles() {
                let ahead = lead.position_mi - pos;
                if (0.0..=20.0).contains(&ahead) {
                    return Some(format!(
                        "Traffic: {}, {} ahead, {}.",
                        lead.status_label(),
                        d.trip.gap_text(ahead),
                        d.trip.speed_text(lead.speed_mph)
                    ));
                }
            }
            None
        })?
    }

    fn road_chatter_line(&mut self, ctx: &mut GameContext) -> String {
        self.driving
            .with(ctx, |d, ctx| {
                if hos::HOS_NON_ENFORCED_MODES.contains(&ctx.settings.hos_mode.as_str()) {
                    return "Road chatter: enforcement reports are quiet in this mode.".to_string();
                }
                // Full detail whatever the enforcement-presence setting is:
                // presence governs ambience, and never information the player
                // asked a key for.
                let pos = d.trip.position_mi;
                for patrol in d.trip.patrols() {
                    if patrol.end_mi() < pos {
                        continue;
                    }
                    let ahead = (patrol.watch_start_mi() - pos).max(0.0);
                    if ahead <= 25.0 {
                        return "Road chatter: enforcement reported somewhere ahead.".to_string();
                    }
                }
                "Road chatter: no enforcement reports nearby.".to_string()
            })
            .unwrap_or_default()
    }
}

fn upcoming_stops(
    d: &crate::states::driving::DrivingState,
    within_mi: f64,
    limit: usize,
) -> Vec<RoadStop> {
    let mut stops: Vec<RoadStop> = d
        .trip
        .stops
        .iter()
        .filter(|stop| {
            let ahead = stop.at_mi - d.trip.position_mi;
            (0.0..=within_mi).contains(&ahead)
        })
        .cloned()
        .collect();
    stops.sort_by(|a, b| a.at_mi.total_cmp(&b.at_mi));
    stops.truncate(limit);
    stops
}

fn say_item(line: String) -> MenuItem<DriverAppScreenState> {
    let spoken = line.clone();
    MenuItem::new(
        line,
        move |_s: &mut DriverAppScreenState, ctx: &mut GameContext| ctx.say(&spoken),
    )
    .help("Repeat this app line.")
}

impl Menu for DriverAppScreenState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        // The Python weather rows carried a callable label so a line could
        // change while the app was open. Here the update hook below refreshes
        // the rows instead, which is the same thing a row at a time and keeps
        // the label types simple.
        let mut items: Vec<MenuItem<Self>> = self.lines(ctx).into_iter().map(say_item).collect();
        items.push(
            MenuItem::new("Back to Driver apps", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Return to the driver tablet app list."),
        );
        items
    }

    /// Keep asynchronous live weather current while its app is open.
    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        if self.app_key != "weather" {
            return;
        }
        let Some((changed, status, refresh_failed)) = self.driving.with(ctx, |d, _| {
            let changed = d.trip.weather.update(0.0);
            (
                changed,
                d.trip.weather.source_status(),
                d.trip.weather.live_weather_refresh_failed(),
            )
        }) else {
            return;
        };
        let refresh_failure_started = refresh_failed && !self.weather_refresh_failed;
        // Same rule as the trip: a new cell loading while simulated weather
        // is in use is not a status change.
        let status = if status == "loading" && self.weather_status == Some("fallback") {
            "fallback"
        } else {
            status
        };
        if changed.is_none() && Some(status) == self.weather_status && !refresh_failure_started {
            self.weather_refresh_failed = refresh_failed;
            return;
        }
        let previous_status = self.weather_status;
        self.weather_status = Some(status);
        self.weather_refresh_failed = refresh_failed;
        if !matches!(status, "live" | "last_known" | "fallback") {
            return;
        }
        let refreshing = self
            .driving
            .with(ctx, |d, _| d.trip.weather.live_weather_refreshing())
            .unwrap_or(false);
        let suppress_routine_refresh = (status == "last_known" && refreshing)
            || (status == "live"
                && previous_status == Some("last_known")
                && !self.weather_refresh_issue_announced);
        if (changed.is_some() || previous_status != Some(status) || refresh_failure_started)
            && !suppress_routine_refresh
        {
            let prefix = if changed.is_some() {
                "Weather updated"
            } else {
                "Weather status changed"
            };
            let lead = self
                .driving
                .with(ctx, |d, ctx| {
                    d.trip.weather.report_lead(ctx.settings.imperial_units)
                })
                .unwrap_or_default();
            ctx.say_with(format!("{prefix}. {lead}."), Say::queued());
            self.weather_refresh_issue_announced = status == "last_known" && refresh_failed;
            // The active tablet consumed this provider update. Keep the
            // paused trip's source tracker aligned so returning to driving
            // does not repeat a generic readiness announcement.
            let announced = self.weather_refresh_issue_announced;
            self.driving.with(ctx, |d, _| {
                d.trip.weather_source_status = status;
                d.trip.weather_refresh_issue_announced = announced;
                if matches!(status, "live" | "fallback") {
                    d.trip.weather_location_refreshing = false;
                }
            });
            if matches!(status, "live" | "fallback") {
                self.weather_refresh_issue_announced = false;
            }
        }
    }
}

impl_state_for_menu!(DriverAppScreenState);
