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
use ff_core::radio::{
    search_page_bounds, search_page_count, RadioReception, RadioStation, PLAYLISTS_DIR_NAME,
    RADIO_SEARCH_PAGE_SIZE,
};

use crate::app::{GameContext, Say};
use crate::states::base::{Key, Menu, MenuCore, MenuItem};
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
            MenuItem::new("Browse by category", |s: &mut Self, ctx| {
                if Self::dial_locked(ctx) {
                    return;
                }
                let state = RadioCategoryState::new(s.driving.clone());
                ctx.push_state(state);
            })
            .help(
                "Browse all stations, or one category such as Terrestrial or Web radio. Stations \
                 in range list first.",
            ),
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
    /// Zero-based page of a search's hits.
    page: usize,
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
            page: 0,
        }
    }

    /// The browse variant: one category (or the whole dial) as a paged list.
    pub fn browse(driving: DriveRef, label: &str, hits: Vec<StationRow>) -> Self {
        RadioStationListState {
            menu: MenuCore::new(label).with_intro_help(STATION_LIST_INTRO_HELP),
            driving,
            kind: "browse".to_string(),
            query: String::new(),
            hits: Some(hits),
            page: 0,
        }
    }

    /// Search and browse lists page; favorites and in-range lists do not.
    fn paged(&self) -> bool {
        self.kind == "search" || self.kind == "browse"
    }

    /// The search variant: a fixed list of hits and how many there were.
    pub fn search(driving: DriveRef, query: &str, hits: Vec<StationRow>, _total: usize) -> Self {
        RadioStationListState {
            menu: MenuCore::new(&list_title("search", query))
                .with_intro_help(STATION_LIST_INTRO_HELP),
            driving,
            kind: "search".to_string(),
            query: query.to_string(),
            hits: Some(hits),
            page: 0,
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
            _ => {
                let hits = self.hits.as_deref().unwrap_or_default();
                let (start, end) =
                    search_page_bounds(hits.len(), self.page, RADIO_SEARCH_PAGE_SIZE);
                hits[start..end].to_vec()
            }
        }
    }

    /// How many hits the search holds in all.
    fn hit_count(&self) -> usize {
        self.hits.as_ref().map_or(0, Vec::len)
    }

    fn page_count(&self) -> usize {
        search_page_count(self.hit_count(), RADIO_SEARCH_PAGE_SIZE)
    }

    /// "Results 26 to 50 of 52."
    fn page_text(&self) -> String {
        let total = self.hit_count();
        let (start, end) = search_page_bounds(total, self.page, RADIO_SEARCH_PAGE_SIZE);
        format!("Results {} to {} of {}.", start + 1, end, total)
    }

    /// Move to another page of a search; says the range, then the first row.
    fn change_page(&mut self, ctx: &mut GameContext, forward: bool) {
        if !self.paged() {
            return;
        }
        let last = self.page_count() - 1;
        if (forward && self.page >= last) || (!forward && self.page == 0) {
            ctx.audio.play("ui/error");
            let edge = if forward {
                "Last page of results."
            } else {
                "First page of results."
            };
            ctx.say_with(
                format!("{edge} {}", self.page_text()),
                Say::new().review(false),
            );
            return;
        }
        self.page = if forward {
            self.page + 1
        } else {
            self.page - 1
        };
        ctx.audio.play("ui/menu_move");
        self.refresh(ctx, false);
        let current = self.current_text(ctx);
        // Logged on its own (the row is not), so the copy key and message
        // review can reach the range.
        let page = self.page_text();
        ctx.message_log
            .add(&page, ff_core::message_log::MessageCategory::General);
        ctx.say_with(format!("{page} {current}"), Say::new().review(false));
    }

    fn next_page_preview(&self) -> String {
        let (start, end) =
            search_page_bounds(self.hit_count(), self.page + 1, RADIO_SEARCH_PAGE_SIZE);
        format!("Results {} to {} of {}", start + 1, end, self.hit_count())
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
        if self.paged() && self.page_count() > 1 {
            if self.page + 1 < self.page_count() {
                items.push(
                    MenuItem::new(
                        format!("Next page. {}", self.next_page_preview()),
                        |s: &mut Self, ctx| s.change_page(ctx, true),
                    )
                    .help("Shows the next results. PageDown does the same."),
                );
            }
            if self.page > 0 {
                items.push(
                    MenuItem::new("Previous page", |s: &mut Self, ctx| {
                        s.change_page(ctx, false)
                    })
                    .help("Shows the earlier results. PageUp does the same."),
                );
            }
        }
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

    fn handle_extra_key(&mut self, ctx: &mut GameContext, key: Key) -> bool {
        if !self.paged() {
            return false;
        }
        match key {
            Key::PageDown => self.change_page(ctx, true),
            Key::PageUp => self.change_page(ctx, false),
            _ => return false,
        }
        true
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let rows = self.rows();
        let title = self.menu.title.clone();
        if self.paged() && self.hit_count() > RADIO_SEARCH_PAGE_SIZE {
            // Paged, and said so: a list that quietly stops reads as "that
            // is everything" to someone who cannot see a scrollbar.
            // Each count is a whole sentence, so a language that inflects
            // the noun for the number has the number beside it.
            let count = self.hit_count();
            let page = self.page_text();
            ctx.say(&if self.kind == "browse" {
                format!("{title}. {count} stations. {page} PageDown for the next page.")
            } else {
                format!("{title}. {count} matches. {page} PageDown for the next page.")
            });
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
        let Some((hits, total)) = self.driving.read(|d| d.radio.search(&query, usize::MAX)) else {
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

/// The category browser: all stations, or one dial category with its count.
pub struct RadioCategoryState {
    menu: MenuCore<Self>,
    driving: DriveRef,
}

impl RadioCategoryState {
    pub fn new(driving: DriveRef) -> Self {
        RadioCategoryState {
            menu: MenuCore::new("Browse by category")
                .with_intro_help("Enter opens a category. Escape goes back."),
            driving,
        }
    }

    fn open(&mut self, ctx: &mut GameContext, group: Option<i32>, label: &str) {
        let Some(hits) = self.driving.read(|d| d.radio.browse(group)) else {
            return;
        };
        if hits.is_empty() {
            ctx.audio.play("ui/error");
            ctx.say_with(format!("No stations in {label}."), Say::new().review(false));
            return;
        }
        ctx.audio.play("ui/menu_select");
        let state = RadioStationListState::browse(self.driving.clone(), label, hits);
        ctx.push_state(state);
    }
}

impl Menu for RadioCategoryState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let categories = self
            .driving
            .read(|d| d.radio.browse_categories())
            .unwrap_or_default();
        let all: usize = self
            .driving
            .read(|d| d.radio.browse(None).len())
            .unwrap_or(0);
        let _ = ctx;
        let mut items = vec![MenuItem::new(
            format!("All stations: {all}"),
            |s: &mut Self, ctx: &mut GameContext| s.open(ctx, None, "All stations"),
        )
        .help("Every station on the dial. Stations in range list first.")];
        for (group, name, count) in categories {
            items.push(
                MenuItem::new(
                    format!("{name}: {count}"),
                    move |s: &mut Self, ctx: &mut GameContext| s.open(ctx, Some(group), name),
                )
                .help("Enter lists this category. Stations in range list first."),
            );
        }
        items.push(MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)).help("Go back."));
        items
    }
}

impl_state_for_menu!(RadioCategoryState);
