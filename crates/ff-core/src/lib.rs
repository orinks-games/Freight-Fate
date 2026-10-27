//! Freight Fate core: everything that needs no window, no audio device, no
//! screen reader and no network. World data, the simulation, the career
//! models, and the spoken-text rules the rest of the game renders.
//!
//! Ported module by module from the retired Python game's `freight_fate`
//! package; each module here keeps the name of the Python module it
//! replaces.

// Python-compatibility shims (no Python original).
pub mod pyfmt;
pub mod pyrandom;

// Top-level modules of the Python package.
pub mod achievements;
pub mod assets_pack;
pub mod audio_fades;
pub mod audio_loops;
pub mod cab_filter;
pub mod cab_sounds;
pub mod channel3000;
pub mod cloud_save_integrity;
pub mod engine_audio;
pub mod input_hints;
pub mod l10n;
pub mod lane_guide_tone;
pub mod message_log;
pub mod music;
pub mod music_synth;
pub mod playtest_levers;
pub mod profile_integrity_invariants;
pub mod profile_invariants;
pub mod radio;
pub mod radio_content;
pub mod radio_rotation;
pub mod rumble;
pub mod settings;
pub mod sound_catalog;
pub mod speech_pacing;
pub mod speech_text;
pub mod spoken_advice;
pub mod units;
pub mod wav;

// Packages.
pub mod data;
pub mod models;
pub mod sim;
