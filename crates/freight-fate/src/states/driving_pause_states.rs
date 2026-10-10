//! The pause menu and its abandon-job guard (port of
//! `freight_fate/states/driving_pause_states.py`).

use ff_core::pyfmt::{fmt_f, fmt_grouped};
use ff_core::sim::hos::{duration_text, HosClock};

use crate::app::{GameContext, Say};
use crate::controller::{ControllerAction, ControllerButton};
use crate::discord_presence::PresenceState;
use crate::impl_state_for_menu;
use crate::online_presence::PAUSED_ACTIVITY;
use crate::states::base::{end_sentence, InputEvent, Menu, MenuCore, MenuItem, SimpleMenuState};
use crate::states::city::CityMenuState;
use crate::states::driving::DrivingState;
use crate::states::driving_core::{
    advance_rest_clock, clock_text, deadline_text, hos_mut_of, is_night,
    player_pays_operating_costs, profile_mut_of, profile_of, road_repair_cost,
    secure_truck_for_stopped_menu, CHAIN_INSTALL_FATIGUE, CHAIN_INSTALL_MIN,
    CHAIN_INSTALL_NIGHT_FATIGUE, CHAIN_INSTALL_NIGHT_MULT, CHAIN_REMOVE_FATIGUE, CHAIN_REMOVE_MIN,
    CHAIN_SAFE_MPH, DRIVE_PHASE_PICKUP, FIELD_REPAIR_DAMAGE_PCT, MECHANIC_CALLOUT_FEE,
    MECHANIC_WAIT_MIN,
};
use crate::states::driving_dispatch_call::DispatchCallState;
use crate::states::driving_menu_states::{keep_rows, push_over_drive, DriveRef};
use crate::states::driving_rest_states::ShoulderSleepConfirmationState;
use crate::states::learn_sounds::LearnSoundsState;
use crate::states::main_menu::{controls_help_page, HelpState, MainMenuState, SettingsState};
use crate::states::online_states::DriversOnlineState;

/// Discard unsaved driver state before returning to the title.
///
/// The active-trip snapshot represents the last durable route checkpoint.
/// HOS and fatigue are mutated directly on the profile while driving, so they
/// must be restored before a later application shutdown saves the profile.
pub fn restore_checkpoint_driver_state(ctx: &mut GameContext, _driving: &mut DrivingState) {
    if ctx.profile.is_none() {
        return;
    }
    let Some(snapshot) = profile_of(ctx).active_trip.clone() else {
        return;
    };
    let Some(snapshot) = snapshot.as_object().cloned() else {
        return;
    };
    if let Some(saved_hos) = snapshot.get("hos") {
        if saved_hos.is_object() {
            // `driving.hos` was an ALIAS of `profile.hos` in Python, so
            // writing the profile is writing both.
            profile_mut_of(ctx).hos = HosClock::from_dict(saved_hos);
        }
    }
    if let Some(saved_fatigue) = snapshot.get("fatigue").and_then(|v| v.as_f64()) {
        profile_mut_of(ctx).fatigue = saved_fatigue.clamp(0.0, 100.0);
    }
}

pub struct PauseMenuState {
    menu: MenuCore<Self>,
    driving: DriveRef,
}

impl PauseMenuState {
    pub fn new(ctx: &GameContext) -> Self {
        PauseMenuState {
            menu: MenuCore::new("Paused"),
            driving: DriveRef::active(ctx),
        }
    }

    /// The same screen over a drive the caller already shares (tests).
    pub fn with_drive(driving: DriveRef) -> Self {
        PauseMenuState {
            menu: MenuCore::new("Paused"),
            driving,
        }
    }

    /// `enter()` run while the drive is still in hand -- see `drive_ref`.
    pub fn enter_over_drive(&mut self, ctx: &mut GameContext, driving: &mut DrivingState) {
        ctx.audio.play("ui/pause");
        ctx.audio.stop_world();
        // Everything the road had handed the voice belongs to the mile the
        // player just stopped on. Left alone it sits in the event channel's
        // own queue and is performed over the pause menu, or replayed in full
        // on resume (tester transcript, 2026-08-11).
        driving.prepare_warning_speech_pause(ctx);
        ctx.pause_event_speech();
        driving.pending_ambient_events.clear();
        driving.forget_stopped_loops();
        let items = self.rows(ctx, driving);
        self.menu.items = items;
        self.menu.index = self.menu.index.min(self.menu.items.len().saturating_sub(1));
        if let Some(key) = self.menu.open_sound_key.clone() {
            ctx.audio.play(&key);
        }
        self.announce_entry(ctx);
    }

    fn rows(&mut self, ctx: &mut GameContext, d: &mut DrivingState) -> Vec<MenuItem<Self>> {
        let drive_label = if d.phase == DRIVE_PHASE_PICKUP {
            "pickup drive"
        } else {
            "delivery"
        };
        let mut items =
            vec![
            MenuItem::new("Resume driving", |s: &mut Self, ctx| s.resume(ctx))
                .help(format!("Return to the active {drive_label}.")),
            MenuItem::new("Trip status", |s: &mut Self, ctx| s.status(ctx))
                .help("Cargo, objective, route progress, and time used."),
            MenuItem::new("Controls and help", |_s: &mut Self, ctx: &mut GameContext| {
                let page = controls_help_page();
                let state = HelpState::at_page(page);
                ctx.push_state(state);
            })
            .help(
                "The how-to-play reference at the driving keys. Left and Right change pages, Up \
                 and Down read lines.",
            ),
            MenuItem::new("Learn game sounds", |_s: &mut Self, ctx: &mut GameContext| {
                let state = LearnSoundsState::new();
                ctx.push_state(state);
            })
            .help("Plays any sound the road uses and says what it means."),
            MenuItem::new(mechanic_label(d), |s: &mut Self, ctx| s.mechanic(ctx)).help(
                "A mobile mechanic patches the truck up enough to drive on. Far dearer than a \
                 garage, an hour and a half, and billed even into debt.",
            ),
            MenuItem::new("Settings", |_s: &mut Self, ctx: &mut GameContext| {
                let state = SettingsState::new();
                ctx.push_state(state);
            })
            .help("Units, transmission, volumes, weather, voices, update channel, and pacing."),
        ];
        if d.trip.truck.speed_mph() <= 3.0 {
            items.insert(
                2,
                MenuItem::new("Call dispatch", |s: &mut Self, ctx| {
                    ctx.push_state(DispatchCallState::new(s.driving.clone()));
                })
                .help("Report a delay, hours, road conditions, truck trouble, or load trouble."),
            );
        }
        if d.trip.truck.chains_on {
            items.push(
                MenuItem::new(
                    format!(
                        "Remove snow chains: about {} minutes",
                        fmt_f(CHAIN_REMOVE_MIN, 0)
                    ),
                    |s: &mut Self, ctx| s.remove_chains(ctx),
                )
                .help("Chains grind apart fast on bare pavement."),
            );
        } else if profile_of(ctx).chains_owned()
            && profile_of(ctx).chain_wear_pct() < 100.0
            // Offered only where chains would matter: snow or ice under the
            // truck, or a chain law posted on the run. Every pause in an
            // October in Chicago used to offer them (owner, 2026-10-01); the
            // help already says they ride in the side box until a pass calls.
            && (matches!(d.trip.truck.surface.as_str(), "snow" | "ice")
                || d.trip.chain_law_level() > 0)
        {
            items.push(
                MenuItem::new(install_chains_label(d), |s: &mut Self, ctx| {
                    s.install_chains(ctx)
                })
                .help(
                    "Chains bite snow and ice. Chain speed is about thirty miles per hour. Pull \
                     them once the road is bare. In the dark, installing takes longer and more \
                     out of you.",
                ),
            );
        }
        items.push(
            MenuItem::new("Drivers on duty", |_s: &mut Self, ctx: &mut GameContext| {
                let state = DriversOnlineState::new(ctx);
                ctx.push_state(state);
            })
            .help(
                "Who is hauling right now on orinks.net. Viewing the list shares nothing about \
                 you.",
            ),
        );
        items.push(
            MenuItem::new("Abandon job", |s: &mut Self, ctx| s.abandon(ctx)).help(
                "Costs five hundred dollars and reputation, and returns you to the origin city.",
            ),
        );
        items.push(
            MenuItem::new("Quit to main menu", |s: &mut Self, ctx| s.quit_to_menu(ctx)).help(
                "Saves happen only at a stop. The drive resumes from the last stop. Abandon job \
                 drops the load.",
            ),
        );
        if d.emergency_shoulder_sleep_reason(ctx).is_some() {
            let settings_index = items
                .iter()
                .position(|item| item.text(self, ctx) == "Settings")
                .unwrap_or(items.len());
            items.insert(
                settings_index,
                MenuItem::new("Emergency shoulder sleep", |s: &mut Self, ctx| {
                    s.emergency_shoulder_sleep(ctx)
                })
                .help(
                    "Poor sleep on the shoulder. Resets hours of service. Fatigue remains, \
                     possible ticket, possible minor truck damage, and the deadline keeps \
                     running.",
                ),
            );
        }
        items
    }

    fn mechanic(&mut self, ctx: &mut GameContext) {
        let done = self
            .driving
            .clone()
            .call(self, ctx, |_s, ctx, d| perform_roadside_mechanic(d, ctx));
        // `refresh()` rebuilds these rows through the same drive the closure
        // was holding, so it has to run after the borrow is back. Called from
        // inside, the rebuild found the drive busy and left the pause menu
        // with no rows at all -- "No options available" after a repair that
        // in fact worked.
        let Some(Some(text)) = done else {
            return;
        };
        self.refresh(ctx, true);
        ctx.say(&text);
    }

    fn install_chains(&mut self, ctx: &mut GameContext) {
        // Same rule as `mechanic`: rebuild the rows once the drive is free.
        let done = self.driving.clone().call(self, ctx, |_s, ctx, d| {
            if d.trip.truck.speed_mph() > 3.0 {
                ctx.say("Come to a complete stop first.");
                return None;
            }
            let night = is_night(d.trip.local_hour());
            let minutes = CHAIN_INSTALL_MIN * if night { CHAIN_INSTALL_NIGHT_MULT } else { 1.0 };
            let fatigue = if night {
                CHAIN_INSTALL_NIGHT_FATIGUE
            } else {
                CHAIN_INSTALL_FATIGUE
            };
            advance_rest_clock(d, ctx, minutes, Some("on_duty_not_driving"), "chain up");
            hos_mut_of(ctx).on_duty(minutes);
            {
                let p = profile_mut_of(ctx);
                p.fatigue = (p.fatigue + fatigue).min(100.0);
            }
            d.trip.truck.chains_on = true;
            d.chains_fast_active = false;
            ctx.audio.play("ui/notify");
            let effort = if night {
                "In the dark, it took more out of you. "
            } else {
                ""
            };
            let bare = if d.trip.truck.surface != "snow" && d.trip.truck.surface != "ice" {
                " The road here is bare, they grind fast until the snow."
            } else {
                ""
            };
            let text = format!(
                "Chains on in {} minutes. {effort}Chain speed {}.{bare} It is {}. {}",
                fmt_f(minutes, 0),
                ctx.settings.speed_text(CHAIN_SAFE_MPH),
                clock_text(d.trip.local_hour()),
                deadline_text(d, ctx)
            );
            Some(text)
        });
        let Some(Some(text)) = done else {
            return;
        };
        self.refresh(ctx, true);
        ctx.say(&text);
    }

    fn remove_chains(&mut self, ctx: &mut GameContext) {
        // Same rule as `mechanic`: rebuild the rows once the drive is free.
        let done = self.driving.clone().call(self, ctx, |_s, ctx, d| {
            if d.trip.truck.speed_mph() > 3.0 {
                ctx.say("Come to a complete stop first.");
                return None;
            }
            advance_rest_clock(
                d,
                ctx,
                CHAIN_REMOVE_MIN,
                Some("on_duty_not_driving"),
                "remove chains",
            );
            hos_mut_of(ctx).on_duty(CHAIN_REMOVE_MIN);
            {
                let p = profile_mut_of(ctx);
                p.fatigue = (p.fatigue + CHAIN_REMOVE_FATIGUE).min(100.0);
            }
            d.trip.truck.chains_on = false;
            ctx.audio.play("ui/notify");
            let wear = d.trip.truck.chain_wear_pct;
            let state_word = if wear >= 75.0 {
                "The set is about done.".to_string()
            } else if wear >= 1.0 {
                format!("The set is {} percent worn.", fmt_f(wear, 0))
            } else {
                "The set is still fresh.".to_string()
            };
            let text = format!(
                "Chains off in {} minutes. {state_word} It is {}. {}",
                fmt_f(CHAIN_REMOVE_MIN, 0),
                clock_text(d.trip.local_hour()),
                deadline_text(d, ctx)
            );
            Some(text)
        });
        let Some(Some(text)) = done else {
            return;
        };
        self.refresh(ctx, true);
        ctx.say(&text);
    }

    fn emergency_shoulder_sleep(&mut self, ctx: &mut GameContext) {
        let reason = self
            .driving
            .with(ctx, |d, ctx| d.emergency_shoulder_sleep_reason(ctx))
            .flatten();
        let Some(reason) = reason else {
            ctx.say("Emergency shoulder sleep is not available.");
            self.refresh(ctx, true);
            return;
        };
        let secured = self
            .driving
            .with(ctx, secure_truck_for_stopped_menu)
            .unwrap_or(false);
        if !secured {
            ctx.say("Come to a complete stop first.");
            self.refresh(ctx, true);
            return;
        }
        let anchor = self.driving.read(|d| d.trip.position_mi).unwrap_or(0.0);
        let state =
            ShoulderSleepConfirmationState::from_menu(self.driving.clone(), &reason, Some(anchor));
        ctx.push_state(state);
    }

    fn resume(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/unpause");
        // Anything that reached the voice while the menu was up describes a
        // road the player was not on. They come back to the road as it is
        // now.
        ctx.resume_event_speech();
        // The player may have lost the thread across the pause: bring each
        // facility's full name back once (research doc R6).
        self.driving.read(|d| {
            d.trip.reset_facility_mentions();
            d.pending_ambient_events.clear();
            // And the open scale still ahead, once (tester log, 2026-10-07).
            d.note_scale_reannounce(None);
        });
        ctx.pop_state();
        ctx.say_with("Resumed.", Say::queued().review(false));
    }

    fn status(&mut self, ctx: &mut GameContext) {
        let Some(lines) = self.driving.with(ctx, |d, ctx| trip_status_lines(d, ctx)) else {
            return;
        };
        ctx.push_state(SimpleMenuState::readout("Trip status", lines));
    }

    fn abandon(&mut self, ctx: &mut GameContext) {
        // Abandoning is destructive and one keystroke away, so confirm first.
        let state = AbandonJobConfirmationState::new(self.driving.clone());
        ctx.push_state(state);
    }

    fn quit_to_menu(&mut self, ctx: &mut GameContext) {
        // A quit while MOVING discards real progress -- saving happens only
        // at stops -- and the old warning spoke WHILE the quit executed,
        // too late to matter: the owner quit at 76 on I-80 and lost 67
        // miles back to the trip start (2026-07-27). Moving quits now ask
        // first and name the cost; parked quits stay instant.
        let moving = self
            .driving
            .read(|d| d.trip.truck.speed_mph() > 1.0)
            .unwrap_or(false);
        if moving {
            let state = QuitWhileMovingConfirmationState::new(self.driving.clone());
            ctx.push_state(state);
            return;
        }
        execute_quit_to_menu(ctx, &self.driving);
    }
}

/// The quit itself, shared by the parked path and the confirmed moving one.
fn execute_quit_to_menu(ctx: &mut GameContext, driving: &DriveRef) {
    // Saving happens only at stops, so a mid-drive quit writes nothing:
    // the on-disk save still points at your last stop, and Continue
    // resumes the leg from there. In-progress leg driving is
    // intentionally not preserved.
    driving.with(ctx, |d, ctx| restore_checkpoint_driver_state(ctx, d));
    let drive_label = driving
        .read(|d| {
            if d.phase == DRIVE_PHASE_PICKUP {
                "pickup drive"
            } else {
                "delivery"
            }
        })
        .unwrap_or("delivery");
    ctx.say(&format!(
        "Returning to the title. This {drive_label} resumes from your last stop, not from here."
    ));
    ctx.reset_to(MainMenuState::new());
}

/// Miles this quit would throw away: how far the truck is past the position
/// the on-disk save would resume from (the last stop's snapshot, or the
/// trip start when no snapshot was ever written).
fn miles_lost_on_quit(ctx: &GameContext, driving: &DriveRef) -> f64 {
    let position = driving.read(|d| d.trip.position_mi).unwrap_or(0.0);
    let saved = ctx
        .profile
        .as_ref()
        .and_then(|p| p.active_trip.as_ref())
        .and_then(|snapshot| snapshot.get("position_mi"))
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    (position - saved).max(0.0)
}

pub struct QuitWhileMovingConfirmationState {
    menu: MenuCore<Self>,
    driving: DriveRef,
}

impl QuitWhileMovingConfirmationState {
    pub fn new(driving: DriveRef) -> Self {
        QuitWhileMovingConfirmationState {
            menu: MenuCore::new("Quit while driving?")
                .with_intro_help("Enter selects, Escape cancels."),
            driving,
        }
    }

    fn lost_text(&self, ctx: &GameContext) -> String {
        let lost = miles_lost_on_quit(ctx, &self.driving);
        if lost < 1.0 {
            "less than a mile".to_string()
        } else {
            ctx.settings.distance_text(lost, false)
        }
    }

    fn confirm(&mut self, ctx: &mut GameContext) {
        execute_quit_to_menu(ctx, &self.driving);
    }
}

impl Menu for QuitWhileMovingConfirmationState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/warning");
        let lost = self.lost_text(ctx);
        let current = self.current_text(ctx);
        ctx.say(&format!(
            "Still moving, and the game saves only at a stop. Quitting loses {lost} since your \
             last stop. {current}"
        ));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let lost = self.lost_text(ctx);
        vec![
            MenuItem::new("Keep driving", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Back to the pause menu."),
            MenuItem::new(
                format!("Quit anyway and lose {lost}"),
                |s: &mut Self, ctx| s.confirm(ctx),
            )
            .help(
                "Return to the title. The drive resumes from your last stop, \
                 not from here.",
            ),
        ]
    }
}

impl_state_for_menu!(QuitWhileMovingConfirmationState);

pub fn perform_roadside_mechanic(d: &mut DrivingState, ctx: &mut GameContext) -> Option<String> {
    let damage = d.trip.truck.damage_pct;
    if damage <= FIELD_REPAIR_DAMAGE_PCT {
        ctx.say(&format!(
            "A roadside mechanic helps once damage is past {} percent.",
            fmt_f(FIELD_REPAIR_DAMAGE_PCT, 0)
        ));
        return None;
    }
    if d.trip.truck.speed_mph() > 3.0 {
        ctx.say("Come to a complete stop first.");
        return None;
    }
    let cost = road_repair_cost(damage, FIELD_REPAIR_DAMAGE_PCT, MECHANIC_CALLOUT_FEE);
    let carrier_paid = !player_pays_operating_costs(&profile_of(ctx).business_status);
    if !carrier_paid {
        profile_mut_of(ctx).spend(cost);
    }
    let money = profile_of(ctx).money();
    d.trip.truck.damage_pct = FIELD_REPAIR_DAMAGE_PCT;
    advance_rest_clock(d, ctx, MECHANIC_WAIT_MIN, None, "");
    hos_mut_of(ctx).on_duty(MECHANIC_WAIT_MIN);
    ctx.audio.play("ui/notify");
    let billing = if carrier_paid {
        "on the carrier breakdown account".to_string()
    } else {
        format!(
            "for {} dollars. You have {} dollars",
            fmt_grouped(cost, 0),
            fmt_grouped(money, 0)
        )
    };
    Some(format!(
        "A mobile mechanic patched the truck up to {} percent damage {billing}. Repair took an \
         hour and a half. It is {}. {}",
        fmt_f(FIELD_REPAIR_DAMAGE_PCT, 0),
        clock_text(d.trip.local_hour()),
        deadline_text(d, ctx)
    ))
}

pub fn mechanic_label(d: &DrivingState) -> String {
    let damage = d.trip.truck.damage_pct;
    if damage <= FIELD_REPAIR_DAMAGE_PCT {
        return "Call a roadside mechanic: not needed yet".to_string();
    }
    let cost = road_repair_cost(damage, FIELD_REPAIR_DAMAGE_PCT, MECHANIC_CALLOUT_FEE);
    format!("Call a roadside mechanic: {} dollars", fmt_grouped(cost, 0))
}

fn install_chains_label(d: &DrivingState) -> String {
    let night = is_night(d.trip.local_hour());
    let minutes = CHAIN_INSTALL_MIN * if night { CHAIN_INSTALL_NIGHT_MULT } else { 1.0 };
    let when = if night { " in the dark" } else { "" };
    format!(
        "Install snow chains{when}: about {} minutes",
        fmt_f(minutes, 0)
    )
}

/// The pause menu's Trip status, one line each: the load, where it is going,
/// how far along, the hours, and the air.
pub fn trip_status_lines(d: &DrivingState, ctx: &GameContext) -> Vec<String> {
    let hours_used = d.trip.game_minutes / 60.0;
    if d.phase == DRIVE_PHASE_PICKUP {
        return vec![
            format!("Driving to pickup at {}.", d.pickup_facility_text(ctx)),
            format!(
                "{} tons of {} are assigned for {}.",
                fmt_f(d.job.weight_tons, 0),
                d.job.cargo.label,
                d.job.spoken_destination()
            ),
            d.pickup_progress_summary(ctx),
            format!("{} used.", duration_text(hours_used)),
            format!("{}.", d.air_status_text(false)),
        ];
    }
    vec![
        format!(
            "Hauling {} tons of {} to {}.",
            fmt_f(d.job.weight_tons, 0),
            d.job.cargo.label,
            d.job.spoken_destination()
        ),
        d.trip.progress_summary(ctx.settings.imperial_units),
        format!(
            "{} used of {}.",
            duration_text(hours_used),
            duration_text(d.job.deadline_game_h)
        ),
        format!("{}.", d.air_status_text(false)),
    ]
}

impl Menu for PauseMenuState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let built = self
            .driving
            .clone()
            .call(self, ctx, |s, ctx, d| s.rows(ctx, d));
        keep_rows(built, &self.driving, &self.menu.items)
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/pause");
        ctx.audio.stop_world();
        self.driving.read(|d| d.prepare_warning_speech_pause(ctx));
        ctx.pause_event_speech();
        self.driving.read(|d| {
            d.pending_ambient_events.clear();
            d.forget_stopped_loops();
        });
        let items = self.build_items(ctx);
        self.menu.items = items;
        self.menu.index = self.menu.index.min(self.menu.items.len().saturating_sub(1));
        if let Some(key) = self.menu.open_sound_key.clone() {
            ctx.audio.play(&key);
        }
        self.announce_entry(ctx);
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        // Pausing and resuming is where the player is, not something that
        // happened on the road. Logging it would leave a "Paused." between
        // every pair of announcements for anyone who checks the menu mid-run.
        let title = end_sentence(&self.menu.title);
        ctx.say_with(title, Say::new().review(false));
        let current = self.current_text(ctx);
        ctx.say_with(current, Say::queued().review(false));
    }

    fn presence(&self, ctx: &GameContext) -> Option<PresenceState> {
        let detail = self
            .driving
            .read(|d| d.presence_state(ctx).map(|p| p.detail).unwrap_or_default())
            .unwrap_or_default();
        Some(PresenceState::new(PAUSED_ACTIVITY, &detail))
    }

    fn online_presence(&self, ctx: &GameContext) -> Option<PresenceState> {
        // A pause is not the end of a shift: the player stays on the public
        // drivers list, shown as paused over the drive's own detail, and
        // nobody's duty watch calls them off duty for a bathroom break. The
        // service posts this once and then stops heartbeating (see
        // PAUSED_ACTIVITY); a pause left for half an hour ages off the list
        // like a parked truck.
        let detail = self
            .driving
            .read(|d| {
                d.online_presence_state(ctx)
                    .map(|p| p.detail)
                    .unwrap_or_default()
            })
            .unwrap_or_default();
        Some(PresenceState::new(PAUSED_ACTIVITY, &detail))
    }

    fn handle_controller(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        // Start pauses and unpauses, so it resumes from the pause menu too.
        if let InputEvent::ControllerButtonDown { button, .. } = event {
            if *button == ControllerButton::Start {
                self.resume(ctx);
                return;
            }
        }
        // A trait default cannot be called from its own override, so the
        // fall-through is spelled out (`city::base_menu_handle_event` exists
        // for the key half for the same reason).
        match ctx.controller.menu_action(event) {
            Some(ControllerAction::MenuDown) => self.move_by(ctx, 1),
            Some(ControllerAction::MenuUp) => self.move_by(ctx, -1),
            Some(ControllerAction::AdjustRight) => self.adjust(ctx, 1),
            Some(ControllerAction::AdjustLeft) => self.adjust(ctx, -1),
            Some(ControllerAction::Confirm) => self.activate(ctx),
            Some(ControllerAction::Back) => self.go_back(ctx),
            Some(ControllerAction::Help) => {
                let help = self.current_help(ctx);
                ctx.say(&help);
            }
            None => {}
        }
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        self.resume(ctx);
    }
}

impl_state_for_menu!(PauseMenuState);

/// Yes/No guard in front of abandoning a job. Lands on "No" so giving up the
/// load takes a deliberate arrow to "Yes".
pub struct AbandonJobConfirmationState {
    menu: MenuCore<Self>,
    driving: DriveRef,
}

/// Abandoning a real load costs 500 dollars and 5 reputation (breach of a
/// paying contract). An assigned reposition carries no freight and no pay, so
/// there is no contract to breach and no money at stake -- but walking off a
/// dispatch ASSIGNMENT still costs the carrier's trust, the same shape as
/// declining one outright (`dispatch_policy::DECLINE_REPUTATION_PENALTY`),
/// just heavier, because this one was already accepted and driven before it
/// was abandoned.
pub const ASSIGNED_REPOSITION_ABANDON_REPUTATION_PENALTY: f64 = 3.0;

const ABANDON_INTRO_HELP: &str = "Enter selects, Escape cancels.";

impl AbandonJobConfirmationState {
    pub fn new(driving: DriveRef) -> Self {
        AbandonJobConfirmationState {
            menu: MenuCore::new("Abandon job?").with_intro_help(ABANDON_INTRO_HELP),
            driving,
        }
    }

    /// An empty reposition has no load and no contract -- there is nothing to
    /// breach. The hours still pass either way.
    pub fn is_bobtail(&self) -> bool {
        self.driving.read(|d| d.job.bobtail).unwrap_or(false)
    }

    /// Dispatch sent the driver empty (`job.assigned`), as opposed to a
    /// self-serve bobtail the driver chose from the menu. There is still no
    /// freight and no pay to lose, but walking away from an ASSIGNMENT is
    /// walking away from dispatch, not just from a drive.
    pub fn is_assigned_reposition(&self) -> bool {
        self.is_bobtail() && self.driving.read(|d| d.job.assigned).unwrap_or(false)
    }

    fn abandon_help_text(&self) -> &'static str {
        if self.is_assigned_reposition() {
            return "Costs reputation, no money, and returns you to the origin city.";
        }
        if self.is_bobtail() {
            return "No freight, no penalty, returns you to the origin city.";
        }
        "Costs five hundred dollars and reputation, and returns you to the origin city."
    }

    fn confirm(&mut self, ctx: &mut GameContext) {
        let bobtail = self.is_bobtail();
        let assigned_reposition = self.is_assigned_reposition();
        if assigned_reposition {
            let p = profile_mut_of(ctx);
            p.career.reputation =
                (p.career.reputation - ASSIGNED_REPOSITION_ABANDON_REPUTATION_PENALTY).max(0.0);
        } else if !bobtail {
            let p = profile_mut_of(ctx);
            p.spend(500.0);
            p.career.reputation = (p.career.reputation - 5.0).max(0.0);
        }
        self.driving.with(ctx, |d, ctx| {
            profile_mut_of(ctx).store_truck_condition(&d.trip.truck);
            // the hours spent on the failed run still happened: keep the
            // world clock consistent with the HOS and fatigue already accrued
            let hours = d.trip.game_minutes / 60.0;
            let market_day = {
                let p = profile_mut_of(ctx);
                p.game_hours += hours;
                p.market_day()
            };
            let p = profile_mut_of(ctx);
            p.market.advance_to(market_day);
            p.active_trip = None;
            p.pay_advance_used_for_load = false;
        });
        ctx.save_profile();
        ctx.pop_state(); // close this confirmation
        ctx.pop_state(); // close the pause menu
        let city = CityMenuState::new(ctx, false);
        ctx.replace_state(city);
        let where_ = ctx
            .world
            .spoken_city(&profile_of(ctx).current_city.clone(), None);
        // interrupt so this overrides any menu re-announcement during unwind
        if assigned_reposition {
            ctx.say(&format!(
                "Dispatch assignment abandoned. Reputation down, no fine. Back in {where_}."
            ));
        } else if bobtail {
            ctx.say(&format!(
                "Reposition called off. No freight, no penalty, the hours still count. Back in \
                 {where_}."
            ));
        } else {
            ctx.say(&format!(
                "Job abandoned. Penalty of five hundred dollars paid. Back in {where_}."
            ));
        }
    }
}

impl Menu for AbandonJobConfirmationState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let help = self.abandon_help_text();
        vec![
            MenuItem::new("No, keep driving", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Return to the pause menu and keep this job."),
            MenuItem::new("Yes, abandon the job", |s: &mut Self, ctx| s.confirm(ctx)).help(help),
        ]
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let title = self.menu.title.clone();
        let city = ctx
            .world
            .spoken_city(&profile_of(ctx).current_city.clone(), None);
        let current = self.current_text(ctx);
        if self.is_assigned_reposition() {
            ctx.say(&format!(
                "{title} No freight, no fine, but reputation takes a hit. Back to {city}. \
                 {current}"
            ));
            return;
        }
        if self.is_bobtail() {
            ctx.say(&format!(
                "{title} Running empty, turning back costs only the time already spent. Back to \
                 {city}. {current}"
            ));
            return;
        }
        ctx.say(&format!(
            "{title} Penalty of five hundred dollars, a reputation hit, and back to {city}. {current}"
        ));
    }
}

impl_state_for_menu!(AbandonJobConfirmationState);

// -- what the drive pushes ------------------------------------------------------------
//
// The block `driving_controls/pending.rs` held for this module.

impl DrivingState {
    /// `ctx.push_state(PauseMenuState(ctx, self))`: Escape, Start, and the
    /// controller-disconnect pause.
    pub fn push_pause_menu(&mut self, ctx: &mut GameContext) {
        let mut state = PauseMenuState::new(ctx);
        state.enter_over_drive(ctx, self);
        push_over_drive(ctx, state);
    }
}
