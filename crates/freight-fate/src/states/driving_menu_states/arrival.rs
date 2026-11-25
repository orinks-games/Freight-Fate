//! The delivery settlement (`ArrivalState`): what the run earned, what it
//! cost, and everything the career learned from it.
//!
//! The screen keeps no handle on the drive. Every number is read out of the
//! drive once, in [`ArrivalState::new`] (Python's `__init__` -> `_settle`),
//! and the rows are the resulting lines -- so the settlement cannot change
//! under the player while they arrow through it.

use std::time::{SystemTime, UNIX_EPOCH};

use ff_core::models::border_freight::yard_drop_line;
use ff_core::models::business::{
    company_driver_pay, display_rank_for, uses_company_career_ranks_for, SettlementTerms,
    COMPANY_DRIVER,
};
use ff_core::models::career::{standing_xp_rate, xp_rate_settlement_clause};
use ff_core::models::career_ladder::relabel_level_up_announcements;
use ff_core::models::cargo_condition::{
    cargo_condition_text, pay_loss_share, settle_cargo, CargoSettlement, CargoVocabulary,
};
use ff_core::models::carrier_fleet::{
    assigned_truck_key, equipment_held_back, fleet_tier_for_level, fleet_upgrade_announcement,
    withheld_promotion_text, WITHHELD_UNLOCK_TAIL,
};
use ff_core::models::credentials::MANUAL_SPEC_DIFFERENTIAL;
use ff_core::models::enforcement;
use ff_core::models::home_base::{self, ParkedAt, ParkedInputs};
use ff_core::models::jobs::{lane_key, Job};
use ff_core::models::solvency::{self, deductions_from_settlement};
use ff_core::models::trucks::truck_model;
use ff_core::pyfmt::{fmt_f, fmt_grouped, round_py_n};
use ff_core::sim::fatigue_costs::RestCall;
use ff_core::sim::timezones::to_local;

use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::meaningful_play::MeaningfulPlayReason;
use crate::online_journal::{queue_career_milestones, queue_delivery, CareerFacts, DeliveryFacts};
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::city::CityMenuState;
use crate::states::driving::DrivingState;
use crate::states::driving_core::{
    carrier_accessorial_charges, charge_summary, charge_total, clock_text,
    has_weigh_station_transponder, is_owner_operator, pay_label, profile_mut_of, profile_of,
    reputation_pay_bonus, unique_consecutive, wallet_delta, xp_class_multiplier, xp_streak_bonus,
};
use crate::states::driving_damage::{damage_summary_line, preventable_damage_charge};
use crate::states::driving_menu_states::badges::{
    award_arrival_achievements, award_bobtail_road_badges,
};
use crate::states::driving_menu_states::{
    DELIVERY_SETTLEMENT_MAX_AVERAGE_MPH, ROAD_GRIME_PER_MILE, SETTLEMENT_LOW_FUEL_FRACTION,
};

/// `_settlement_hours(driving)`: no run settles at a faster average than a
/// truck can actually hold.
pub fn settlement_hours(driving: &DrivingState) -> f64 {
    let driven_hours = driving.trip.game_minutes / 60.0;
    let minimum_hours = driving.job.credited_mi() / DELIVERY_SETTLEMENT_MAX_AVERAGE_MPH;
    driven_hours.max(minimum_hours)
}

const ARRIVAL_INTRO_HELP: &str =
    "Up and down review the summary. Enter repeats the line. Escape continues.";

mod settlement;

pub struct ArrivalState {
    menu: MenuCore<Self>,
    pub summary_parts: Vec<String>,
    achievement_messages: Vec<String>,
    new_achievement_names: Vec<String>,
    announcements: Vec<String>,
    pub summary_lines: Vec<String>,
    pub terminal: ParkedAt,
    /// Miles the co-driver drove on this load; 0 when the player drove it.
    pub(crate) team_miles: f64,
    /// The road journal's line for a team load.
    pub team_note: Option<String>,
    /// The settlement row for the co-driver's share of the pay.
    pub(crate) team_line: Option<String>,
}

/// The facility a finished drive leaves the truck at: the receiver for a
/// delivery, none for an empty reposition.
fn arrival_parked_facility(job: &Job) -> String {
    if job.bobtail {
        String::new()
    } else {
        job.destination_location.clone()
    }
}

impl ArrivalState {
    pub fn new(ctx: &mut GameContext, driving: &mut DrivingState) -> Self {
        let arrival_hours = settlement_hours(driving);
        Self::new_at(ctx, driving, arrival_hours)
    }

    /// Build settlement with an appointment time captured at receiver
    /// check-in. `driving` may already include later unloading time.
    pub fn new_at(ctx: &mut GameContext, driving: &mut DrivingState, arrival_hours: f64) -> Self {
        // Where the truck ends up: the carrier terminal in the home terminal
        // city, else the receiver it just delivered to (a reposition has no
        // facility), else the city's public lot -- never a yard elsewhere.
        let terminal = {
            let p = profile_of(ctx);
            let facility = arrival_parked_facility(&driving.job);
            home_base::parked_at(
                ctx.world,
                ParkedInputs {
                    carrier_key: &p.carrier_key,
                    carrier_name: &p.carrier_name,
                    business_status: &p.business_status,
                    home_terminal_city: &p.home_terminal_city,
                    current_city: &driving.job.destination,
                    parked_facility: &facility,
                },
            )
        };
        let mut state = ArrivalState {
            menu: MenuCore::new("Delivery complete").with_intro_help(ARRIVAL_INTRO_HELP),
            summary_parts: Vec::new(),
            achievement_messages: Vec::new(),
            new_achievement_names: Vec::new(),
            announcements: Vec::new(),
            summary_lines: Vec::new(),
            terminal,
            team_miles: 0.0,
            team_note: None,
            team_line: None,
        };
        state.settle(ctx, driving, arrival_hours);
        state
    }

    pub fn title(&self) -> &str {
        &self.menu.title
    }

    /// `enter()`, run while the drive is still in hand -- see `drive_ref`.
    /// Nothing here reads the drive; the settlement already captured it.
    pub fn enter_over_drive(&mut self, ctx: &mut GameContext) {
        // The settlement just moved the driver to a new city: start its
        // dispatch board's route work now, while the summary is read, so
        // the board opens without the wait (2026-09-28).
        let city = profile_of(ctx).current_city.clone();
        crate::states::city::warm_dispatch_board(ctx.world, &city);
        Menu::enter(self, ctx);
    }

    /// Say what a balance owed did to this settlement, and warn once a rung.
    ///
    /// Rungs move on the same discipline as the trust band: spoken when they
    /// change, never on a timer, and never repeated.
    fn debt_settlement_lines(&mut self, ctx: &mut GameContext, d: &DrivingState) {
        let written_off = solvency::apply_hard_cap(profile_mut_of(ctx));
        if written_off >= 1.0 {
            self.summary_parts.push(format!(
                "{} wrote off {} dollars of what you owe. The balance holds where it is.",
                profile_of(ctx).carrier_name,
                fmt_grouped(written_off, 0)
            ));
        }
        let rung = solvency::debt_rung(profile_of(ctx));
        let was = profile_of(ctx).driving_record.debt_rung_heard;
        if rung == was {
            return;
        }
        profile_mut_of(ctx).driving_record.debt_rung_heard = rung;
        if rung == 0 {
            if was > 0 {
                self.summary_parts.push(
                    "Square with your carrier again. Nothing owed, every settlement reaches \
                     you whole."
                        .to_string(),
                );
            }
            return;
        }
        let terse = d.terse_speech(ctx);
        let line = solvency::debt_warning_line(profile_of(ctx), terse);
        if !line.is_empty() {
            self.summary_parts.push(line);
        }
    }

    /// Stop a level-up promising a tractor the yard is not handing over.
    fn rewrite_unlock_promise(&self, ctx: &GameContext, announcements: &mut [String]) {
        if announcements.is_empty() {
            return;
        }
        let unlock = display_rank_for(profile_of(ctx)).unlock;
        let target = format!("Unlock: {unlock}");
        for message in announcements.iter_mut() {
            if message.starts_with("Level up!") && message.contains(&target) {
                *message = message.replace(&target, &format!("{target} {WITHHELD_UNLOCK_TAIL}"));
                return;
            }
        }
    }

    /// Swap the carrier tractor when a level-up crosses a fleet tier.
    ///
    /// The carrier hands the new unit over road-ready, so the profile's
    /// equipment condition resets with it -- company repairs are carrier
    /// billed anyway, this just skips the paperwork.
    ///
    /// Unless the yard is not handing it over. A driver whose standing holds
    /// them below their level keeps the truck they are in, untouched: the
    /// road-ready reset belongs to the tractor changing hands, and handing
    /// someone a spotless lesser truck would tell them something happened
    /// when nothing did.
    fn handle_fleet_promotion(
        &mut self,
        ctx: &mut GameContext,
        previous_level: i64,
        announcements: &mut [String],
    ) -> Vec<String> {
        let p = profile_of(ctx);
        let level = p.career.level();
        if p.owns_equipment() || level <= previous_level {
            return Vec::new();
        }
        if fleet_tier_for_level(previous_level).key == fleet_tier_for_level(level).key {
            return Vec::new();
        }
        if equipment_held_back(profile_of(ctx)) {
            self.rewrite_unlock_promise(ctx, announcements);
            return vec![withheld_promotion_text(profile_of(ctx))];
        }
        let key = assigned_truck_key(profile_of(ctx), None::<&Job>);
        let Some(model) = truck_model(key) else {
            return Vec::new();
        };
        {
            let p = profile_mut_of(ctx);
            p.set_truck_fuel_gal(model.specs.fuel_tank_gal);
            p.set_truck_damage_pct(0.0);
            p.set_tire_wear_pct(0.0);
            p.set_road_grime_pct(0.0);
        }
        let flagship = fleet_tier_for_level(level).key == "first_pick";
        let mut badges: Vec<&str> = vec!["fleet_upgrade"];
        if flagship {
            badges.push("fleet_flagship");
        }
        for badge in badges {
            if let Some(result) = ctx.award_achievement_with(badge, false, false) {
                self.record_badge(
                    result.award.message.normal.clone(),
                    result.award.achievement.name.to_string(),
                );
            }
        }
        vec![fleet_upgrade_announcement(profile_of(ctx))]
    }

    /// One row naming the run's new achievements, not one paragraph each.
    ///
    /// The full flavor for every badge stays in the achievements menu and the
    /// message log, so the settlement reports the count and the names and
    /// leaves the story for a parked, unhurried read (research doc R9).
    fn achievement_summary_line(&self) -> Option<String> {
        let names = &self.new_achievement_names;
        if names.is_empty() {
            return None;
        }
        if names.len() == 1 {
            return Some(format!("New achievement! {}.", names[0]));
        }
        Some(format!("New achievements! {}.", names.join(", ")))
    }

    /// The badge sweep's two collectors, for `badges.rs`.
    pub(crate) fn record_badge(&mut self, message: String, name: String) {
        self.achievement_messages.push(message);
        self.new_achievement_names.push(name);
    }

    fn copy_summary(&mut self, ctx: &mut GameContext) {
        let mut parts = vec![format!("Freight Fate: {}.", self.menu.title)];
        parts.extend(self.summary_lines.clone());
        // A blank line, then the hashtag, the way a social post ends.
        parts.push(String::new());
        parts.push("#FreightFateRuns".to_string());
        let text = parts.join("\n");
        if ctx.write_clipboard_text(&text) {
            ctx.audio.play("ui/menu_select");
            ctx.say_with(
                "Delivery summary copied to clipboard.",
                Say::new().review(false),
            );
        } else {
            ctx.audio.play("ui/error");
            ctx.say_with("Could not copy to the clipboard.", Say::new().review(false));
        }
    }

    fn continue_to_terminal(&mut self, ctx: &mut GameContext) {
        let city = CityMenuState::new(ctx, false);
        ctx.replace_state(city);
    }
}

impl Menu for ArrivalState {
    /// A prompt to answer: Escape answers it, so touch adds no Back row.
    fn touch_back_row(&self) -> bool {
        false
    }

    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = self
            .summary_lines
            .iter()
            .map(|line| {
                let spoken = line.clone();
                MenuItem::new(line.clone(), move |_s: &mut Self, ctx: &mut GameContext| {
                    ctx.say(&spoken)
                })
                .help("Repeat this settlement line.")
            })
            .collect();
        items.push(
            MenuItem::new("Copy delivery summary to clipboard", |s: &mut Self, ctx| {
                s.copy_summary(ctx)
            })
            .help("Copies the settlement lines as plain text.")
            .select_sound(None),
        );
        items.push(MenuItem::new(
            format!("Continue to {}", self.terminal.name),
            |s: &mut Self, ctx| s.continue_to_terminal(ctx),
        ));
        items
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        ctx.audio.stop_world();
        ctx.audio.play("ui/job_complete");
        if !self.announcements.is_empty() || !self.achievement_messages.is_empty() {
            ctx.audio.play("ui/level_up");
        }
        ctx.audio.play("ui/cash");
        let items = self.build_items(ctx);
        self.menu.items = items;
        self.menu.index = self.menu.index.min(self.menu.items.len().saturating_sub(1));
        self.announce_entry(ctx);
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let title = self.menu.title.clone();
        let current = self.current_text(ctx);
        ctx.say_with(format!("{title}. {current}"), Say::queued());
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        self.continue_to_terminal(ctx);
    }

    fn lines(&self, ctx: &GameContext) -> Vec<String> {
        let mut out = vec![self.menu.title.clone(), String::new()];
        out.extend(self.summary_lines.clone());
        out.push(String::new());
        for (i, item) in self.menu.items.iter().enumerate() {
            let marker = if i == self.menu.index { "> " } else { "  " };
            out.push(format!("{marker}{}", item.text(self, ctx)));
        }
        out
    }
}

impl_state_for_menu!(ArrivalState);
