//! Freight Fate — the game crate: audio, speech, input, network, states and
//! the application shell, on top of `ff_core`.

pub mod account_achievements;
#[cfg(feature = "agent-server")]
pub mod agent_server;
#[cfg(target_os = "android")]
pub mod android;
pub mod app;
pub mod audio;
pub mod bindings;
pub mod browser;
pub mod cloud_saves;
pub mod community_stations;
pub mod controller;
pub mod discord_presence;
pub mod duty_watch;
pub mod jaws_script;
pub mod meaningful_play;
pub mod net;
pub mod online_activation;
pub mod online_guest;
pub mod online_journal;
pub mod online_presence;
pub mod playtest;
pub mod problem_reports;
pub mod single_instance;
pub mod speech;
pub mod states;
pub mod touch;
pub mod updater;
#[cfg(feature = "visuals")]
pub mod visuals;

/// Whether the radio reads the player's own playlists from the Playlists
/// folder. Phones and tablets leave them out: the folder sits inside the
/// app's private storage, where the player cannot put files, and the
/// YouTube entries need a yt-dlp the phone cannot run.
pub const PERSONAL_PLAYLISTS: bool = !cfg!(touch_screen);
