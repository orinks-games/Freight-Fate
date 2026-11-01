//! Home menu presentation and safe automatic update offers.
use super::*;

impl Menu for MainMenuState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        let (sequence, ambience) = reuse_loadable_saves_scan(|| {
            // The base `Menu::enter`: rebuild the rows, play the open sound,
            // announce.
            self.refresh(ctx, true);
            if let Some(key) = self.menu.open_sound_key.clone() {
                ctx.audio.play(&key);
            }
            self.announce_entry(ctx);
            let newest;
            let profile: Option<&Profile> = match ctx.profile.as_ref() {
                Some(p) => Some(p),
                None => {
                    newest = loadable_saves().into_iter().next().map(|(_, p)| p);
                    newest.as_ref()
                }
            };
            let profile = profile.map(|p| p as &dyn MenuMusicProfile);
            (
                select_menu_music_sequence(profile),
                select_menu_ambience(profile),
            )
        });
        let refs: Vec<&str> = sequence.iter().map(String::as_str).collect();
        ctx.play_music_sequence("menu", &refs);
        ctx.audio.set_ambient(Some(ambience));
        if ctx.settings.update_channel.is_empty() {
            let build = updater::load_build_info(version());
            let channel = updater::resolve_channel("", build.as_ref());
            if channel != "stable" {
                ctx.settings.update_channel = channel;
                if let Err(error) = ctx.settings.save() {
                    log::warn!("Could not keep update channel: {error}");
                }
            }
        }
        let armed = UPDATE_CHECK
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .0
            .is_some();
        if !armed {
            Self::arm_update_check(&ctx.settings);
        }
    }

    fn exit(&mut self, ctx: &mut GameContext) {
        ctx.audio.set_ambient(None);
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        // The sound device now opens on a worker thread, so its verdict can
        // land a beat AFTER this menu's greeting already asked. Same rule
        // as at entry: silence with no word is never allowed.
        if ctx.audio.take_silence_notice() {
            ctx.say(
                "Game sounds could not start on this computer: the voice, but \
                 no engine, traffic, or alert sounds. Check that sound works \
                 elsewhere, then start Freight Fate again.",
            );
        }
        if let Some(line) = updater::take_recovery_notice() {
            ctx.say(&line);
        }
        if crate::online_presence::take_secret_store_timeout_notice() {
            ctx.say(
                "The system password store did not answer in time, so online \
                 features may need you to link your account again under Online. \
                 If a permission dialog is still open, allow Freight Fate and \
                 restart.",
            );
        }
        let info = {
            let mut guard = UPDATE_CHECK.lock().unwrap_or_else(|e| e.into_inner());
            let (checker, prompted) = &mut *guard;
            let Some(checker) = checker else {
                return;
            };
            if *prompted || !checker.is_done() {
                return;
            }
            *prompted = true;
            checker.result()
        };
        if let Some(info) = info {
            if info.tag != ctx.settings.skipped_update {
                ctx.push_state(UpdatePromptState::at_start(info));
            }
        }
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let mut warning = String::new();
        let invalid = last_invalid_saves();
        if !invalid.is_empty() {
            let count = invalid.len();
            warning = if count == 1 {
                format!("{count} saved career could not be read and was moved aside. ")
            } else {
                format!("{count} saved careers could not be read and were moved aside. ")
            };
        }
        LAST_SAVE_ERRORS.with(|errors| {
            for error in errors.borrow().iter() {
                warning.push_str(&format!("{error} "));
            }
        });
        if ctx.settings.lane_keeping_unreadable {
            // Falling back to full lane keeping is the right answer and a
            // silent one is not: it deletes the destination-exit decision
            // outright, and nothing later in the drive would explain why.
            ctx.settings.lane_keeping_unreadable = false;
            warning.push_str(
                "Your lane keeping setting could not be read, so it is set to \
                 full: the truck holds the lane and takes your exits. Change it \
                 in Settings, Gameplay, Driving assistance. ",
            );
        }
        if ctx.audio.take_silence_notice() {
            // The game starts anyway when the sound device will not open --
            // that is the design, and a driver who can still hear the voice
            // can still work. What must not happen is the player being left
            // to guess: with no engine, no traffic and no alerts, and nothing
            // said, a broken sound setup is indistinguishable from a broken
            // game. Reported on Linux, where the device open failed and the
            // whole drive ran silent without a word.
            warning.push_str(
                "Game sounds could not start on this computer: the voice, but \
                 no engine, traffic, or alert sounds. Check that sound works \
                 elsewhere, then start Freight Fate again. ",
            );
        }
        if crate::online_presence::take_secret_store_timeout_notice() {
            // A Keychain ACL prompt after a new Mac signature can block the
            // driver-token read at launch. We stop waiting after a few
            // seconds so the menu opens; say why online may need a re-link.
            warning.push_str(
                "The system password store did not answer in time, so online \
                 features may need you to link your account again under Online. \
                 If a permission dialog is still open, allow Freight Fate and \
                 restart. ",
            );
        }
        if loadable_saves().is_empty() && !legacy_saves().is_empty() {
            // Every saved career predates 1.9, so there is no Continue item
            // where the player expects one. Say where the careers went before
            // that silence reads as data loss; once a 1.9 career exists, the
            // labels in Choose career carry the explanation instead.
            warning.push_str(
                "Your saved careers are from an earlier version of Freight \
                 Fate; they are listed under Choose career. ",
            );
        }
        let text = format!(
            "Freight Fate {}. {warning}",
            updater::spoken_version(version())
        );
        ctx.say(text.trim_end());
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        let saves = loadable_saves();
        if let Some((latest_path, latest_profile)) = saves.first() {
            items.push(
                MenuItem::new(
                    format!(
                        "Continue latest career: {}",
                        career_summary(ctx, latest_path, latest_profile, false)
                    ),
                    |s: &mut Self, ctx| s.continue_latest(ctx),
                )
                .help(format!("Load the newest save for {}.", latest_profile.name)),
            );
        }
        if !saves.is_empty() || !legacy_saves().is_empty() {
            // Careers from earlier versions cannot continue, but they still
            // belong on the list: even with nothing loadable, Choose career is
            // where a player finds them and hears what happened.
            items.push(
                MenuItem::new("Choose career", |_s: &mut Self, ctx| {
                    ctx.push_state(LoadDriverState::new())
                })
                .help("Any saved career, not only the newest."),
            );
        }
        if !saves.is_empty() || !legacy_saves().is_empty() {
            items.push(
                MenuItem::new("Manage careers", |_s: &mut Self, ctx| {
                    ctx.push_state(ManageCareersState::new())
                })
                .help("Reset or delete saved careers."),
            );
        }
        items.push(
            MenuItem::new("New career", |_s: &mut Self, ctx| {
                ctx.push_state(NameEntryState::new())
            })
            .help("Start a fresh career."),
        );
        items.push(
            MenuItem::new("Achievements", |_s: &mut Self, ctx| {
                ctx.push_state(AchievementCareerState::new())
            })
            .help("Earned and locked achievements for a saved career."),
        );
        items.push(
            MenuItem::new("Online", |_s: &mut Self, ctx| {
                let hub = OnlineHubState::new(ctx);
                ctx.push_state(hub)
            })
            .help(
                "Drivers on duty, your orinks.net account, cloud backup \
                 and restore, and Discord presence.",
            ),
        );
        items.push(
            MenuItem::new("How to play", |_s: &mut Self, ctx| {
                ctx.push_state(HelpState::new())
            })
            .help("The controls and the goal of the game."),
        );
        items.push(
            MenuItem::new("Learn game sounds", |_s: &mut Self, ctx| {
                ctx.push_state(LearnSoundsState::new())
            })
            .help("Any sound the road uses, and what it means."),
        );
        items.push(
            MenuItem::new("Practice keys and buttons", |_s: &mut Self, ctx| {
                ctx.push_state(InputPracticeState::new())
            })
            .help("Names what each key or controller button does while driving."),
        );
        items.push(
            MenuItem::new("Settings", |_s: &mut Self, ctx| {
                ctx.push_state(SettingsState::new())
            })
            .help(if !updater::self_updates() {
                "Units, transmission mode, volumes, weather, voices, and trip pacing."
            } else {
                "Units, transmission mode, volumes, weather, voices, \
                 update channel, and trip pacing."
            }),
        );
        items.push(
            MenuItem::new("Report a problem", |s: &mut Self, ctx| s.report_issue(ctx))
                .help("The bug report page on GitHub, in your web browser."),
        );
        // An iPhone app is never quit from inside; the system closes it.
        if !cfg!(target_os = "ios") {
            items.push(
                MenuItem::new("Quit", |_s: &mut Self, ctx| ctx.quit()).help("Exit the game."),
            );
        }
        items
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        if cfg!(target_os = "ios") {
            return;
        }
        ctx.audio.play("ui/menu_back");
        ctx.push_state(ConfirmQuitState::new());
    }

    fn presence(&self, _ctx: &GameContext) -> Option<PresenceState> {
        Some(PresenceState::activity("In the main menu"))
    }
}
