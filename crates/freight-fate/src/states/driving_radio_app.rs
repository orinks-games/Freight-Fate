//! The Radio app on the driver tablet: search the dial, tune by name, keep
//! favorites (port of `freight_fate/states/driving_radio_app.py`).
//!
//! The dial keys walk the band one station at a time and the Tab radio screen
//! only reports; neither lets a driver ask for a station by name, and with
//! five thousand web stations behind the terrestrial band that is the
//! difference between finding one and never hearing it. This app is three
//! short lists and a text field over the existing `RadioState`: nothing here
//! talks to the network, and every tune goes through the same
//! `select_station` the dial uses, so the streamer-safe gate, the session's
//! dead-stream ban, and the engine-power rule all hold.
//!
//! Owner, 2026-08-22: "add a way to search and tune into stations in the
//! radio tablet app ... and favorite stations in the app too."

use ff_core::models::profile;
use ff_core::radio::{RadioReception, RadioStation, PLAYLISTS_DIR_NAME, RADIO_SEARCH_LIMIT};

use crate::app::{GameContext, Say};
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving::DrivingState;
use crate::states::driving_menu_states::DriveRef;
use crate::states::text_entry::{TextEntry, TextEntryCore};
use crate::{impl_state_for_menu, impl_state_for_text_entry};

/// One search or favorites row: the station, and how it comes in here.
pub type StationRow = (RadioStation, Option<RadioReception>);

/// `data_dir() / "Playlists"`.
fn personal_playlists_dir() -> std::path::PathBuf {
    profile::data_dir().join(PLAYLISTS_DIR_NAME)
}

/// One spoken row: name, band, and how it comes in here.
fn hit_label(
    driving: &mut DrivingState,
    station: &RadioStation,
    reception: Option<&RadioReception>,
) -> String {
    let band = driving.radio.band_name(station).to_string();
    let mut bits = vec![station.display_name(), band];
    match reception {
        None => bits.push("out of range here".to_string()),
        Some(reception) => bits.push(reception.signal_label().to_string()),
    }
    if station.id == driving.radio.current_station().id {
        bits.push("tuned now".to_string());
    }
    if driving.radio.is_favorite(station) {
        bits.push("favorite".to_string());
    }
    bits.join(", ")
}

/// The app's front page: what is playing, and the three ways in.
pub struct RadioAppState {
    menu: MenuCore<Self>,
    driving: DriveRef,
}

const RADIO_APP_INTRO_HELP: &str = "Enter on a station tunes it. Escape returns to Driver apps.";

impl RadioAppState {
    pub fn new(driving: DriveRef) -> Self {
        RadioAppState {
            menu: MenuCore::new("Radio").with_intro_help(RADIO_APP_INTRO_HELP),
            driving,
        }
    }

    fn favorite_label(&self, ctx: &mut GameContext) -> String {
        self.driving
            .with(ctx, |d, _| {
                let station = d.radio.current_station();
                if station.fallback {
                    return "Favorites: the safety fallback is always on the dial".to_string();
                }
                if d.radio.is_favorite(&station) {
                    return format!("Remove {} from favorites", station.display_name());
                }
                format!("Save {} to favorites", station.display_name())
            })
            .unwrap_or_default()
    }

    fn toggle_favorite(&mut self, ctx: &mut GameContext) {
        // The same toggle the O key uses, so the profile is written once.
        self.driving
            .with(ctx, |d, ctx| d.toggle_radio_favorite(ctx));
        self.refresh(ctx, true);
    }

    fn toggle_power(&mut self, ctx: &mut GameContext) {
        self.driving.with(ctx, |d, ctx| d.toggle_radio(ctx));
        self.refresh(ctx, true);
    }

    /// Synthesized with streamer-safe mode on: the Roadhouse is the only
    /// station, so there is no list to open or search. Does nothing and
    /// says nothing (owner ruling, 2026-09-21).
    fn dial_locked(ctx: &mut GameContext) -> bool {
        ctx.settings.synth_music && ctx.settings.radio_streamer_safe
    }

    fn open_list(&mut self, ctx: &mut GameContext, kind: &str) {
        if Self::dial_locked(ctx) {
            return;
        }
        let state = RadioStationListState::new(self.driving.clone(), kind);
        ctx.push_state(state);
    }
}

impl Menu for RadioAppState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        // The dial re-reads the Playlists folder here for the same reason the
        // Tab radio screen does: this is where a player comes looking.
        self.driving.with(ctx, |d, ctx| {
            d.radio.reload_personal_playlists(&personal_playlists_dir());
            d.sync_radio_settings(ctx);
        });
        let items = self.build_items(ctx);
        self.menu.items = items;
        self.menu.index = self.menu.index.min(self.menu.items.len().saturating_sub(1));
        if let Some(key) = self.menu.open_sound_key.clone() {
            ctx.audio.play(&key);
        }
        self.announce_entry(ctx);
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        // Python read these labels through callables so a row could change
        // while the screen was open; every action here calls `refresh`, so
        // rebuilt strings say the same thing.
        let now_playing = self
            .driving
            .with(ctx, |d, ctx| d.radio_now_playing_text(ctx))
            .unwrap_or_default();
        let (enabled, tuned) = self
            .driving
            .with(ctx, |d, _| {
                (d.radio.enabled, d.radio.current_station().display_name())
            })
            .unwrap_or((false, String::new()));
        let favorites = self
            .driving
            .with(ctx, |d, _| d.radio.favorites().len())
            .unwrap_or(0);
        let in_range = self
            .driving
            .with(ctx, |d, _| d.radio.receivable_stations().len())
            .unwrap_or(0);
        let favorite_label = self.favorite_label(ctx);
        let repeat = now_playing.clone();
        vec![
            MenuItem::new(now_playing, move |_s: &mut Self, ctx: &mut GameContext| {
                ctx.say(&repeat)
            })
            .help("Now playing on the tuned station. Enter asks again."),
            MenuItem::new(
                format!(
                    "Radio: {}, tuned to {tuned}",
                    if enabled { "on" } else { "off" }
                ),
                |s: &mut Self, ctx| s.toggle_power(ctx),
            )
            .help("Enter switches the radio on or off."),
            MenuItem::new(favorite_label, |s: &mut Self, ctx| s.toggle_favorite(ctx))
                .help("Saves or removes the tuned station in favorites."),
            MenuItem::new(
                format!("Favorites: {favorites} saved"),
                |s: &mut Self, ctx| s.open_list(ctx, "favorites"),
            )
            .help("Your saved stations. Enter on one tunes it."),
            MenuItem::new("Search stations", |s: &mut Self, ctx| {
                if Self::dial_locked(ctx) {
                    return;
                }
                let state = RadioSearchEntryState::new(s.driving.clone());
                ctx.push_state(state);
            })
            .help("Type part of a name, call sign, or format. Matches list nearest signal first."),
            MenuItem::new(
                format!("Stations in range: {in_range}"),
                |s: &mut Self, ctx| s.open_list(ctx, "range"),
            )
            .help("Every station that comes in here. Enter on one tunes it."),
            MenuItem::new("Suggest a station", |_s: &mut Self, ctx| {
                crate::states::driving_radio_suggest::open_station_suggestion(ctx)
            })
            .help(
                "Send a station that is missing from the dial. Accepted stations join the dial \
                 for every driver.",
            ),
            MenuItem::new("Back to Driver apps", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Return to the driver tablet app list."),
        ]
    }
}

impl_state_for_menu!(RadioAppState);

/// A tunable list: favorites, the stations in range, or a search's hits.
pub struct RadioStationListState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    pub kind: String,
    pub query: String,
    hits: Option<Vec<StationRow>>,
    total: usize,
}

const STATION_LIST_INTRO_HELP: &str = "Enter tunes the station. Escape goes back.";

fn list_title(kind: &str, query: &str) -> String {
    match kind {
        "favorites" => "Favorite stations".to_string(),
        "range" => "Stations in range".to_string(),
        _ => format!("Stations matching {query}"),
    }
}

impl RadioStationListState {
    pub fn new(driving: DriveRef, kind: &str) -> Self {
        RadioStationListState {
            menu: MenuCore::new(&list_title(kind, "")).with_intro_help(STATION_LIST_INTRO_HELP),
            driving,
            kind: kind.to_string(),
            query: String::new(),
            hits: None,
            total: 0,
        }
    }

    /// The search variant: a fixed list of hits and how many there were.
    pub fn search(driving: DriveRef, query: &str, hits: Vec<StationRow>, total: usize) -> Self {
        RadioStationListState {
            menu: MenuCore::new(&list_title("search", query))
                .with_intro_help(STATION_LIST_INTRO_HELP),
            driving,
            kind: "search".to_string(),
            query: query.to_string(),
            hits: Some(hits),
            total,
        }
    }

    fn rows(&self) -> Vec<StationRow> {
        match self.kind.as_str() {
            "favorites" => self
                .driving
                .read(|d| d.radio.favorites())
                .unwrap_or_default(),
            "range" => self
                .driving
                .read(|d| {
                    d.radio
                        .receivable_stations()
                        .into_iter()
                        .map(|r| (r.station.clone(), Some(r)))
                        .collect()
                })
                .unwrap_or_default(),
            _ => self.hits.clone().unwrap_or_default(),
        }
    }

    fn tune(
        &mut self,
        ctx: &mut GameContext,
        station: &RadioStation,
        reception: Option<&RadioReception>,
    ) {
        if reception.is_none() {
            ctx.audio.play("ui/error");
            ctx.say(&format!("{} is out of range here.", station.display_name()));
            return;
        }
        ctx.audio.play("ui/menu_select");
        let station_id = station.id.clone();
        let message = self
            .driving
            .with(ctx, |d, ctx| d.tune_radio_to(ctx, &station_id))
            .unwrap_or_default();
        // Defensive: a locked dial answers with an empty message (the list
        // that reaches this row is never open while locked, but a command
        // that returns nothing must never be spoken as nothing).
        if !message.is_empty() {
            ctx.say(&message);
        }
        self.refresh(ctx, true);
    }
}

impl Menu for RadioStationListState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let rows = self.rows();
        let mut items: Vec<MenuItem<Self>> = rows
            .into_iter()
            .map(|(station, reception)| {
                let label = self
                    .driving
                    .with(ctx, |d, _| hit_label(d, &station, reception.as_ref()))
                    .unwrap_or_default();
                MenuItem::new(label, move |s: &mut Self, ctx: &mut GameContext| {
                    s.tune(ctx, &station, reception.as_ref())
                })
                .help("Enter tunes this station.")
                .select_sound(None)
            })
            .collect();
        if items.is_empty() {
            let label = if self.kind != "favorites" {
                "No stations here yet."
            } else {
                "No favorites saved yet. The Radio app saves the tuned station."
            };
            items.push(
                MenuItem::new(label, |s: &mut Self, ctx: &mut GameContext| {
                    let text = s.current_text(ctx);
                    ctx.say(&text);
                })
                .help("Nothing to tune on this list."),
            );
        }
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)).help("Go back."));
        items
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let rows = self.rows();
        let title = self.menu.title.clone();
        if self.kind == "search" && self.total > rows.len() {
            // Capped, and said so: a list that quietly stops at forty reads
            // as "that is everything" to someone who cannot see a scrollbar.
            ctx.say(&format!(
                "{title}. {} matches, the first {} listed.",
                self.total,
                rows.len()
            ));
        } else if rows.is_empty() {
            ctx.say(&format!("{title}. Nothing here yet."));
        } else {
            ctx.say(&format!("{title}. {} stations.", rows.len()));
        }
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }
}

impl_state_for_menu!(RadioStationListState);

/// Type part of a station name; Enter lists the matches.
pub struct RadioSearchEntryState {
    entry: TextEntryCore,
    driving: DriveRef,
}

impl RadioSearchEntryState {
    pub fn new(driving: DriveRef) -> Self {
        let mut entry = TextEntryCore::new("Search stations", "Search");
        entry.max_len = 40;
        RadioSearchEntryState { entry, driving }
    }
}

impl TextEntry for RadioSearchEntryState {
    fn entry(&self) -> &TextEntryCore {
        &self.entry
    }

    fn entry_mut(&mut self) -> &mut TextEntryCore {
        &mut self.entry
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        if crate::states::text_entry::touch_prompt(ctx) {
            ctx.say(&format!(
                "Search stations. Type part of a name, call sign, or format. {}",
                crate::states::text_entry::TOUCH_FIELD_HELP
            ));
            return;
        }
        ctx.say(
            "Search stations. Type part of a name, call sign, or format, then Enter. Left and \
             Right review the letters, Home and End jump to the ends. Escape cancels.",
        );
    }

    fn confirm(&mut self, ctx: &mut GameContext) {
        let query = self
            .entry
            .text()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if query.is_empty() {
            ctx.audio.play("ui/error");
            ctx.say_with(
                "Type something to search for first.",
                Say::new().review(false),
            );
            return;
        }
        let Some((hits, total)) = self
            .driving
            .read(|d| d.radio.search(&query, RADIO_SEARCH_LIMIT))
        else {
            return;
        };
        if hits.is_empty() {
            ctx.audio.play("ui/error");
            ctx.say_with(
                format!("No stations match {query}."),
                Say::new().review(false),
            );
            return;
        }
        ctx.audio.play("ui/menu_select");
        let state = RadioStationListState::search(self.driving.clone(), &query, hits, total);
        ctx.push_state(state);
    }
}

impl_state_for_text_entry!(RadioSearchEntryState);
