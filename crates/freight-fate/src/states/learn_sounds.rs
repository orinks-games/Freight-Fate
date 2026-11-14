//! Learn game sounds: hear a cue and what it means, before it matters (port
//! of `freight_fate/states/learn_sounds.py`).
//!
//! Two screens. The first lists the catalog's categories; the second lists
//! the cues inside one and plays them on request.
//!
//! Arrowing speaks the name and nothing else, the way every other menu in
//! the game behaves. Enter plays the cue, and Enter again replays it. A cue
//! that fired while its own name was being spoken would teach the player the
//! collision rather than the cue, and holding Down would machine-gun the
//! audio, so nothing here plays on movement.

use std::cell::RefCell;

use ff_core::lane_guide_tone::register_lane_guide_tone;
use ff_core::sound_catalog::demo::{DemoAudio, SoundDemo};
use ff_core::sound_catalog::{SoundCategory, SoundEntry, CATALOG};

use crate::app::{GameContext, Say};
use crate::audio::{asset_length_s, Audio};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving_core::siren::register_enforcement_sounds;

/// The audio facade as the demo sequencer drives it. It lives beside its
/// only user.
pub struct DemoAudioBridge<'a>(RefCell<&'a mut dyn Audio>);

impl<'a> DemoAudioBridge<'a> {
    pub fn new(audio: &'a mut dyn Audio) -> Self {
        Self(RefCell::new(audio))
    }
}

impl DemoAudio for DemoAudioBridge<'_> {
    fn hold_alert(&mut self, key: &str, volume: f64) {
        self.0.get_mut().hold_alert_with(key, volume, 60);
    }

    fn release_alert(&mut self) {
        self.0.get_mut().release_alert();
    }

    fn set_loop_pan(&mut self, channel: i32, pan: f64) {
        self.0.get_mut().set_loop_pan(channel as u32, pan);
    }

    fn play(&mut self, key: &str, volume: f64, pan: f64) {
        self.0.get_mut().play_with(key, volume, pan);
    }

    fn has_asset(&self, key: &str) -> bool {
        self.0.borrow_mut().has_asset(key)
    }
}

/// The category list.
pub struct LearnSoundsState {
    menu: MenuCore<Self>,
}

impl LearnSoundsState {
    pub fn new() -> Self {
        Self {
            menu: MenuCore::new("Learn game sounds").with_intro_help(
                "Up and Down choose a group of sounds, Enter opens it, Escape goes back. \
                 Inside a group, Enter plays a sound and F1 explains it.",
            ),
        }
    }

    fn summary(category: &SoundCategory) -> String {
        let names: Vec<&str> = category.entries.iter().take(3).map(|e| e.name).collect();
        if names.is_empty() {
            String::new()
        } else {
            format!("Starting with {}.", names.join(", "))
        }
    }
}

impl Default for LearnSoundsState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for LearnSoundsState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        // Escape has always worked, but a row you can arrow onto is how every
        // other menu in the game offers the way out, and it is the only one a
        // player finds without having heard the intro (owner, 2026-08-16).
        let mut items: Vec<MenuItem<Self>> = CATALOG
            .iter()
            .map(|category| {
                let c: SoundCategory = *category;
                MenuItem::new(category.name, move |_s: &mut Self, ctx| {
                    ctx.push_state(LearnSoundCategoryState::new(c))
                })
                .help(format!(
                    "{} sounds. {}",
                    category.entries.len(),
                    Self::summary(category)
                ))
            })
            .collect();
        items.push(
            MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Leave Learn game sounds."),
        );
        items
    }
}

impl_state_for_menu!(LearnSoundsState);

/// The cues inside one category, and the demo that plays them.
pub struct LearnSoundCategoryState {
    menu: MenuCore<Self>,
    pub category: SoundCategory,
    pub demo: SoundDemo,
}

impl LearnSoundCategoryState {
    pub fn new(category: SoundCategory) -> Self {
        Self {
            menu: MenuCore::new(category.name).with_intro_help(
                "Up and Down choose a sound, Enter plays it, and F1 explains it and when \
                 you hear it. Escape goes back. Moving to another sound or going back \
                 stops a sound that keeps running. A short sound finishes on its own.",
            ),
            category,
            demo: SoundDemo::new(asset_length_s),
        }
    }

    /// Demo one entry, or say why it cannot be demonstrated.
    ///
    /// A cue that ships only in the licensed sound overlay resolves to
    /// nothing on a clean build. Playing nothing at all would teach the
    /// player that the real cue is silent, so the screen says so instead.
    pub fn play_entry(&mut self, ctx: &mut GameContext, entry: &SoundEntry) {
        // The bridge borrows `ctx.audio`, so the question is asked and the
        // borrow released inside its own scope -- `ctx.say` below needs the
        // context back.
        let playable = {
            let audio = DemoAudioBridge::new(ctx.audio.as_mut());
            self.demo.can_play(&audio, entry)
        };
        if !playable {
            ctx.say(&format!(
                "{} is not in this copy of the game. F1 still explains it.",
                entry.name
            ));
            return;
        }
        let mut audio = DemoAudioBridge::new(ctx.audio.as_mut());
        self.demo.start(&mut audio, entry);
    }

    /// Stop the demo against the context's audio.
    pub fn stop_demo(&mut self, ctx: &mut GameContext) {
        let mut audio = DemoAudioBridge::new(ctx.audio.as_mut());
        self.demo.stop(&mut audio);
    }
}

impl Menu for LearnSoundCategoryState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    /// Open the screen, ready to play everything the catalog names.
    ///
    /// The enforcement signature and the two ladder earcons are synthesized
    /// rather than shipped, and nothing else publishes the earcons at all --
    /// the ladder does not yet sound anything in gameplay, only this screen
    /// does. Opening this screen from the main menu would otherwise land on
    /// an entry that resolved to nothing -- a cue demonstrated as silence,
    /// which is the one thing this screen must never do. Registering is
    /// idempotent and cheap, so both entry points simply do it on the way in.
    ///
    /// Stopping the demo here covers re-entry: a screen pushed over this one
    /// freezes the demo's clock, and coming back re-announces the title while
    /// a held cue would otherwise pick its hold straight back up.
    fn enter(&mut self, ctx: &mut GameContext) {
        register_enforcement_sounds();
        register_lane_guide_tone();
        self.stop_demo(ctx);
        // The base `Menu::enter`: rebuild the rows, play the open sound,
        // announce.
        self.refresh(ctx, true);
        if let Some(key) = self.menu.open_sound_key.clone() {
            ctx.audio.play(&key);
        }
        self.announce_entry(ctx);
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = self
            .category
            .entries
            .iter()
            .map(|entry| {
                let e: SoundEntry = *entry;
                MenuItem::new(entry.name, move |s: &mut Self, ctx| s.play_entry(ctx, &e))
                    .help(
                        format!("{} {}", entry.meaning, entry.when)
                            .trim()
                            .to_string(),
                    )
                    // The demo IS the confirmation; a menu click over the top
                    // of a cue the player is trying to learn defeats the
                    // screen.
                    .select_sound(None)
            })
            .collect();
        // Selecting this runs go_back, which stops a held demo on the way
        // out, so the row cannot leave a cue ringing behind it.
        items.push(
            MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Back to the list of sound groups."),
        );
        items
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        let mut audio = DemoAudioBridge::new(ctx.audio.as_mut());
        self.demo.update(&mut audio, dt);
    }

    /// Stop any running demo before speaking the newly selected entry.
    ///
    /// `move_by` (arrows), `jump` (Home/End) and `first_letter_jump`
    /// (typing a letter) are three separate routes into a changed
    /// selection, but the menu base funnels all three through this one
    /// hook before it speaks. Stopping the demo here, rather than in each
    /// route, means a held cue can never keep ringing under a name it
    /// does not belong to -- and a future navigation route inherits the
    /// rule for free instead of needing to remember it.
    fn speak_current(&mut self, ctx: &mut GameContext) {
        self.stop_demo(ctx);
        let text = self.current_text(ctx);
        ctx.say_with(text, Say::new().review(false));
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        self.stop_demo(ctx);
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
    }

    fn exit(&mut self, ctx: &mut GameContext) {
        self.stop_demo(ctx);
    }
}

impl_state_for_menu!(LearnSoundCategoryState);
