//! Freight Fate — the game crate: audio, speech, input, network, states and
//! the application shell, on top of `ff_core`.

pub mod account_achievements;
#[cfg(feature = "agent-server")]
pub mod agent_server;
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
pub mod online_journal;
pub mod online_presence;
pub mod playtest;
pub mod single_instance;
pub mod speech;
pub mod states;
pub mod touch;
pub mod updater;
