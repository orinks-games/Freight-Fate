//! Live driving status: the screen list, and one screen of lines at a time
//! (`DrivingStatusState` / `DrivingStatusScreenState`).

use ff_core::models::enforcement;
use ff_core::models::profile;
use ff_core::models::solvency::debt_line;
use ff_core::pyfmt::{fmt_f, fmt_grouped};
use ff_core::radio::{PERSONAL_PLAYLIST_SOURCE_TYPE, PLAYLISTS_DIR_NAME};
use ff_core::settings::Settings;
use ff_core::sim::trip_models::RoadStop;

use crate::app::{GameContext, Say};
use crate::bindings::Action;
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving_core::{
    clock_text, deadline_appointment, join_phrase, poi_offers_text, profile_of, unique_consecutive,
    KG_PER_TON,
};
use crate::states::driving_menu_states::apps::DriverAppsState;
use crate::states::driving_menu_states::commands::DrivingCommandsState;
use crate::states::driving_menu_states::DriveRef;
use crate::states::driving_radio_app::RadioStationListState;
use crate::states::driving_stop_detail::StopDetailState;

/// Live driving status, grouped into screens you open one at a time.
///
/// A tabbed layout (Right/Left to cycle Route, Driver, Map) needs visible
/// tabs to make sense, so each screen is its own spoken submenu instead,
/// matching the rest of the game's menus.
pub struct DrivingStatusState {
    menu: MenuCore<Self>,
    driving: DriveRef,
}

const SCREENS: [(&str, &str); 5] = [
    ("Route", "route"),
    ("Driver", "driver"),
    ("Map", "map"),
    ("Radio", "radio"),
    ("Driver apps", "apps"),
];

const STATUS_INTRO_HELP: &str =
    "Up and down pick a screen, Enter opens it, Escape returns to driving.";

impl DrivingStatusState {
    pub fn new(ctx: &GameContext) -> Self {
        DrivingStatusState {
            menu: MenuCore::new("Driving status").with_intro_help(STATUS_INTRO_HELP),
            driving: DriveRef::active(ctx),
        }
    }

    /// The same screen built over a drive the caller already shares.
    pub fn with_drive(driving: DriveRef) -> Self {
        DrivingStatusState {
            menu: MenuCore::new("Driving status").with_intro_help(STATUS_INTRO_HELP),
            driving,
        }
    }

    fn open(&mut self, ctx: &mut GameContext, screen: &str) {
        if screen == "apps" {
            let state = DriverAppsState::new(self.driving.clone());
            ctx.push_state(state);
            return;
        }
        let state = DrivingStatusScreenState::new(self.driving.clone(), screen);
        ctx.push_state(state);
    }
}

impl Menu for DrivingStatusState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = SCREENS
            .iter()
            .map(|(label, key)| {
                let key = key.to_string();
                MenuItem::new(*label, move |s: &mut Self, ctx: &mut GameContext| {
                    s.open(ctx, &key)
                })
                .help(format!("Open the {} status screen.", label.to_lowercase()))
            })
            .collect();
        // The pad has no F2, and its buttons are all taken, so a controller
        // driver reaches the controls with no button of their own here. The
        // list takes this screen's place: a command it runs answers over the
        // road, not over the status menu.
        items.push(
            MenuItem::new("Driving commands", |s: &mut Self, ctx: &mut GameContext| {
                let state = DrivingCommandsState::with_drive(s.driving.clone());
                ctx.replace_state(state);
            })
            .help("Every driving command by name. Choose one to run it and return to driving."),
        );
        items.push(
            MenuItem::new("Back to driving", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Close status and resume driving."),
        );
        items
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
        ctx.say_with("Back to driving.", Say::queued());
    }
}

impl_state_for_menu!(DrivingStatusState);

/// One screen of live driving status as a reviewable list of lines.
pub struct DrivingStatusScreenState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    pub screen: String,
}

/// The static `intro_help` the Python class carried became a property so the
/// Map screen can mention opening a stop's details. The radio screen stays.
fn screen_intro_help(screen: &str) -> &'static str {
    if screen == "map" {
        return "Up and down review the lines. Enter repeats a line, or opens details on a \
                stop line. Escape goes back.";
    }
    "Up and down review the lines. Enter repeats a line. Escape goes back."
}

fn screen_title(screen: &str) -> &'static str {
    match screen {
        "route" => "Route",
        "driver" => "Driver",
        "map" => "Map",
        "radio" => "Radio",
        _ => "Status",
    }
}

impl DrivingStatusScreenState {
    pub fn new(driving: DriveRef, screen: &str) -> Self {
        DrivingStatusScreenState {
            menu: MenuCore::new(screen_title(screen)).with_intro_help(screen_intro_help(screen)),
            driving,
            screen: screen.to_string(),
        }
    }

    fn lines(&mut self, ctx: &mut GameContext) -> Vec<String> {
        match self.screen.as_str() {
            "driver" => self.driver_lines(ctx),
            // No map branch: build_items sends the Map screen through
            // map_items now, so its stop lines can open a details view.
            "radio" => self.radio_lines(ctx),
            _ => self
                .driving
                .with(ctx, |d, ctx| d.status_lines(ctx))
                .unwrap_or_default(),
        }
    }

    fn driver_lines(&mut self, ctx: &mut GameContext) -> Vec<String> {
        self.driving
            .clone()
            .call(self, ctx, |_s, ctx, d| {
                let hours_used = d.trip.game_minutes / 60.0;
                let deadline = d.job.deadline_game_h - hours_used;
                let deadline_text = if deadline >= 0.0 {
                    format!(
                        "{} hours before the deadline, due {}",
                        fmt_f(deadline, 1),
                        deadline_appointment(d, ctx)
                    )
                } else {
                    format!("{} hours past the deadline", fmt_f(-deadline, 1))
                };
                let load_line = format!(
                    "Load: {} tons of {}, gross {} tons, freight {}",
                    fmt_f(d.job.weight_tons, 0),
                    d.job.cargo.label,
                    fmt_f(d.trip.truck.gross_mass_kg() / KG_PER_TON, 0),
                    // The player cannot see the trailer; the dock's verdict
                    // must not be the first they hear of what the freight is
                    // in.
                    crate::states::driving_damage::cargo_status_clause(&d.trip.truck)
                );
                // A tank load behaves unlike anything else on the roster, and
                // the driver cannot see the trailer -- so how it will behave
                // is answerable here, on demand, rather than only at pickup.
                // Empty for other freight.
                let tank_line = d.liquid_status_clause();
                // The clock and its zone are one reading ("8:02 AM Central
                // Time"), so a translation finds the zone as a whole.
                let clock = format!(
                    "{} {}",
                    clock_text(d.trip.local_hour()),
                    d.clock_zone_label(ctx)
                );
                let time_line = format!("Time: {clock}, {deadline_text}");
                let band =
                    crate::states::driving_damage::damage_band_clause(&ctx.settings, &d.trip.truck);
                let damage_band_suffix = if band.is_empty() {
                    String::new()
                } else {
                    format!(", {band}")
                };
                let objective = d.objective_text(ctx);
                let gear = d.gear_text();
                let hours = d.player_hos_now(ctx).summary(&ctx.settings.hos_mode);
                let profile = profile_of(ctx);
                // What is owed sits next to what is held, so a driver can ask
                // the same question mid-run that the terminal answers between
                // them.
                let owed = debt_line(profile);
                // The trust line only joins the driving screen when it is
                // doing something. A driver in full trust already hears it on
                // the career stats screen and does not need it twice.
                let trust = if enforcement::standing_band(profile) != enforcement::TRUST_FULL {
                    enforcement::dispatch_trust_line_with_display(
                        profile,
                        ctx.settings.score_display,
                    )
                } else {
                    String::new()
                };
                // Named as the driver's own: a player name is kept as it is.
                let driver_name = &profile.name;
                let mut lines = vec![
                    format!("Driver: {driver_name}"),
                    format!("Money: {} dollars", fmt_grouped(profile.money(), 0)),
                    format!(
                        "Reputation: {}",
                        enforcement::reputation_text(
                            profile.standing(),
                            ctx.settings.score_display
                        )
                    ),
                ];
                if !owed.is_empty() {
                    lines.push(owed);
                }
                lines.push(load_line);
                if !tank_line.is_empty() {
                    lines.push(format!("Tank: {tank_line}"));
                }
                lines.push(format!("Objective: {objective}"));
                // The band rides with the number: hearing "78 percent"
                // without "limp mode" leaves the player to work out why the
                // truck is slow.
                lines.push(format!(
                    "Truck: fuel {} percent, damage {} percent{damage_band_suffix}",
                    fmt_f(d.trip.truck.fuel_fraction() * 100.0, 0),
                    fmt_f(d.trip.truck.damage_pct, 0)
                ));
                lines.push(format!(
                    "Transmission: {}, {gear}",
                    if d.trip.truck.transmission.automatic {
                        "automatic"
                    } else {
                        "manual"
                    }
                ));
                // Only when the driver changed something: a sealed, quiet
                // cab is the default and not news.
                if let Some(cab) = d.cab.status_line() {
                    lines.push(cab.to_string());
                }
                lines.push(format!("Fatigue: {} percent", fmt_f(profile.fatigue, 0)));
                // Where the driver stands is spoken when it changes and
                // whenever it is asked for, never on a timer.
                lines.push(enforcement::standing_text(profile));
                if !trust.is_empty() {
                    lines.push(trust);
                }
                let summary = hours.trim_end_matches('.');
                lines.push(format!("Hours: {summary}"));
                lines.extend(d.team_status_lines(ctx));
                lines.push(time_line);
                lines
            })
            .unwrap_or_default()
    }

    fn map_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let Some((lines_and_stops, planned)) =
            self.driving.clone().call(self, ctx, |_s, ctx, d| {
                let settings = &ctx.settings;
                let route = &d.route;
                let mut rows: Vec<MapRow> = Vec::new();
                // route.cities holds slug keys; speak the composed names instead.
                let cities: Vec<String> = unique_consecutive(
                    &route
                        .cities
                        .iter()
                        .map(|c| ctx.world.spoken_city(c, None))
                        .collect::<Vec<_>>(),
                );
                rows.push(MapRow::Say(format!("Route: {}", cities.join(" to "))));
                rows.push(MapRow::Say(format!(
                    "Highways: {}",
                    join_phrase(&route.highways())
                )));
                rows.push(MapRow::Say(format!(
                    "Progress: {} driven, {} remaining",
                    settings.distance_text(d.trip.position_mi, false),
                    settings.distance_text(d.trip.remaining_miles(), false)
                )));
                rows.push(MapRow::Say(format!(
                    "Guidance: {}",
                    d.trip.next_navigation_context(settings.imperial_units)
                )));
                let upcoming: Vec<RoadStop> = d
                    .trip
                    .stops
                    .iter()
                    .filter(|stop| stop.at_mi >= d.trip.position_mi - 0.05)
                    .take(5)
                    .cloned()
                    .collect();
                if upcoming.is_empty() {
                    rows.push(MapRow::Say(
                        "Stops: none listed before the destination.".to_string(),
                    ));
                } else {
                    for stop in upcoming {
                        let ahead = (stop.at_mi - d.trip.position_mi).max(0.0);
                        let label = format!(
                            "Stop in {}: {}{}; {}.",
                            settings.distance_text(ahead, false),
                            d.trip.planned_prefix(&stop),
                            stop.spoken_name(),
                            poi_offers_text(&stop)
                        );
                        rows.push(MapRow::Stop(label, stop));
                    }
                }
                let next_cues: Vec<_> = d
                    .trip
                    .navigation_cues
                    .iter()
                    .filter(|cue| cue.at_mi > d.trip.position_mi + 0.05 && cue.kind != "rest_stop")
                    .take(4)
                    .cloned()
                    .collect();
                for cue in next_cues {
                    let ahead = (cue.at_mi - d.trip.position_mi).max(0.0);
                    let speed = match cue.speed_mph {
                        Some(mph) => format!(" at {}", settings.speed_text(mph)),
                        None => String::new(),
                    };
                    rows.push(MapRow::Say(format!(
                        "Map point in {}: {}{speed}.",
                        settings.distance_text(ahead, false),
                        cue.text
                    )));
                }
                if d.route.estimated_tolls() > 0.0 {
                    rows.push(MapRow::Say(format!(
                        "Estimated tolls, carrier-paid: {} dollars.",
                        fmt_grouped(d.route.estimated_tolls(), 0)
                    )));
                }
                let planned = d.trip.planned_stop_label();
                (rows, planned)
            })
        else {
            return Vec::new();
        };
        let mut items: Vec<MenuItem<Self>> = lines_and_stops
            .into_iter()
            .map(|row| match row {
                MapRow::Say(line) => say_item(line),
                MapRow::Stop(label, stop) => {
                    MenuItem::new(label, move |s: &mut Self, ctx: &mut GameContext| {
                        s.open_stop(ctx, &stop)
                    })
                    .help("Enter opens details, distance, estimated arrival, and stop planning.")
                }
            })
            .collect();
        if !planned.is_empty() {
            items.push(
                MenuItem::new(
                    format!("Cancel planned stop at {planned}"),
                    |s: &mut Self, ctx| s.cancel_planned_stop(ctx),
                )
                .help("Forgets the planned stop."),
            );
        }
        items
    }

    fn open_stop(&mut self, ctx: &mut GameContext, stop: &RoadStop) {
        let state = StopDetailState::new(self.driving.clone(), stop.clone());
        ctx.push_state(state);
    }

    fn cancel_planned_stop(&mut self, ctx: &mut GameContext) {
        self.driving.read(|d| d.trip.planned_stop_key = None);
        self.refresh(ctx, true);
        ctx.say("Planned stop canceled.");
    }

    fn radio_lines(&mut self, ctx: &mut GameContext) -> Vec<String> {
        self.driving
            .clone()
            .call(self, ctx, |_s, ctx, d| {
                // Opening the dial re-reads the Playlists folder: a playlist
                // added or repaired mid-run used to need a whole new drive
                // before the radio would look at the folder again, and this
                // screen is where a player goes to find out why their
                // playlist is not on the dial.
                d.radio.reload_personal_playlists(&personal_playlists_dir());
                d.sync_radio_settings(ctx);
                let position = d.radio.position;
                let engine_on = d.trip.truck.engine_on;
                let radio_enabled = d.radio.enabled;
                let mut lines = vec![d.radio.status_text()];
                if !engine_on {
                    lines.push("The engine is off. The radio has no power.".to_string());
                }
                if engine_on && radio_enabled {
                    lines.push(d.radio_now_playing_text(ctx));
                }
                let locked = d.radio.station_locked();
                if locked {
                    // Synthesized with streamer-safe mode on: the station keys
                    // do nothing, so the screen names only what still works.
                    // This is an information screen the player asked for, not
                    // a key response, so it still reports the lock in words --
                    // it just no longer claims the keys "say so".
                    lines.push(
                        "Streamer-safe mode on, with Music source set to Synthesized.".to_string(),
                    );
                    lines.push(format!(
                        "Streamer-safe mode keeps the radio on the Roadhouse. Station keys do \
                         nothing. {} turns the radio on or off. Shift with Page Down and Page \
                         Up, or semicolon and apostrophe, changes radio volume by 10 percent.",
                        ctx.bindings.spoken(Action::Radio)
                    ));
                } else {
                    lines.push(if !ctx.settings.radio_streamer_safe {
                        "Streamer-safe mode off. Real public streams and personal playlists are \
                         on the dial."
                            .to_string()
                    } else {
                        "Streamer-safe mode on. Real public streams and personal playlists are \
                         hidden."
                            .to_string()
                    });
                    if ctx.settings.synth_music && !ctx.settings.radio_streamer_safe {
                        lines.push(
                            "Music source Synthesized: Freight Fate's own stations are off \
                             the dial."
                                .to_string(),
                        );
                    }
                    // Players who saw the shuffle setting asked how to get
                    // their own music on at all (issue #289): with nothing
                    // loaded, say where the files go.
                    let has_playlists = d
                        .radio
                        .catalog
                        .iter()
                        .any(|s| s.source_type == PERSONAL_PLAYLIST_SOURCE_TYPE);
                    if !ctx.settings.radio_streamer_safe && !has_playlists {
                        lines.push(format!(
                            "No playlists of your own yet. Put M3U, M3U8 or PLS playlist files \
                             in {}.",
                            personal_playlists_dir().display()
                        ));
                    }
                    lines.push(format!(
                        "Page Down and Page Up tune stations, or semicolon and apostrophe. With \
                         Control they jump categories. With Shift they change radio volume by \
                         10 percent. {} saves the station as a favorite. {} toggles the radio.",
                        ctx.bindings.spoken(Action::RadioFavorite),
                        ctx.bindings.spoken(Action::Radio)
                    ));
                }
                if !locked && !d.radio.favorite_ids.is_empty() {
                    lines.push(format!("Favorites saved: {}.", d.radio.favorite_ids.len()));
                }
                if let Some((lat, lon)) = position {
                    lines.push(format!(
                        "Approximate truck radio position: {}, {}.",
                        fmt_f(lat, 2),
                        fmt_f(lon, 2)
                    ));
                }
                lines.push("Receivable stations:".to_string());
                let imperial = ctx.settings.imperial_units;
                let units = Settings {
                    imperial_units: imperial,
                    ..Settings::default()
                };
                let distance_text = |miles: f64| units.distance_text(miles, false);
                lines.extend(d.radio.station_list_lines(16, Some(&distance_text)));
                lines
            })
            .unwrap_or_default()
    }
}

/// How the Radio screen's favorites count begins; Enter on it opens the list.
const FAVORITES_SAVED_PREFIX: &str = "Favorites saved:";

enum MapRow {
    Say(String),
    Stop(String, RoadStop),
}

fn say_item(line: String) -> MenuItem<DrivingStatusScreenState> {
    let spoken = line.clone();
    MenuItem::new(
        line,
        move |_s: &mut DrivingStatusScreenState, ctx: &mut GameContext| ctx.say(&spoken),
    )
    .help("Repeat this status line.")
}

impl Menu for DrivingStatusScreenState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items = if self.screen == "map" {
            self.map_items(ctx)
        } else {
            let radio = self.screen == "radio";
            self.lines(ctx)
                .into_iter()
                .map(|line| {
                    if radio && line.starts_with(FAVORITES_SAVED_PREFIX) {
                        // The count reads like a row you can open, and the
                        // Radio app's count opens the list, so this one does.
                        MenuItem::new(line, |s: &mut Self, ctx: &mut GameContext| {
                            let state = RadioStationListState::new(s.driving.clone(), "favorites");
                            ctx.push_state(state);
                        })
                        .help("Your saved stations. Enter on one tunes it.")
                    } else {
                        say_item(line)
                    }
                })
                .collect()
        };
        items.push(
            MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Back to the status screens."),
        );
        items
    }
}

impl_state_for_menu!(DrivingStatusScreenState);

/// `data_dir()` joined with the Playlists folder name
/// (`radio.personal_playlists_dir`).
fn personal_playlists_dir() -> std::path::PathBuf {
    profile::data_dir().join(PLAYLISTS_DIR_NAME)
}
