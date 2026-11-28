//! The spoken route POI menu: actions come from the corridor metadata
//! (`RestStopState`).

use ff_core::data::amenities::{classify_brand, spoken_amenities};
use ff_core::data::buffs::{buffs_for_stop, Buff};
use ff_core::models::solvency;
use ff_core::pyfmt::{fmt_f, fmt_grouped, round_py_int, round_py_n};
use ff_core::sim::alertness::Alertness;
use ff_core::sim::hos::{self, FullNight, RestartPlace};
use ff_core::sim::roadside_inspection::InspectionLevel;
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::vehicle::TruckState;
use serde_json::json;

use crate::app::{GameContext, Say};
use crate::discord_presence::PresenceState;
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::city::PayDebtState;
use crate::states::driving::{DrivingState, StopVisit};
use crate::states::driving_core::{
    advance_on_duty_clock, advance_rest_clock, clock_text, deadline_text, full_night_of,
    hos_mut_of, hos_of, pay_advance_grant, pay_advance_unavailable_reason,
    player_pays_operating_costs, poi_ambient_key, profile_mut_of, profile_of, record_inspection,
    road_repair_cost, settle_lift_before_sleep, shut_down_engine, wake_air_instruction,
    FacilityEngine, RigBuff, FIELD_REPAIR_DAMAGE_PCT, MECHANIC_CALLOUT_FEE, MECHANIC_WAIT_MIN,
    MOTEL_COST, ROAD_BRAKE_COST_PER_PCT, ROAD_BRAKE_MIN, ROAD_TIRE_COST_PER_PCT, ROAD_TIRE_MIN,
    ROAD_TIRE_SPECIALIST_COST_PER_PCT, ROAD_TIRE_SPECIALIST_MIN, WALK_AROUND_MIN, WAVE_THROUGH_MIN,
};
use crate::states::driving_menu_states::{keep_rows, DriveRef};
use crate::states::driving_rest_states::back_on_the_road_line;
use crate::states::driving_rest_states::fuel_pump::FuelPump;
use crate::states::driving_rest_states::loyalty::LoyaltyRewardsState;
use crate::states::driving_rest_states::rest_preview::{sleep_preview, SleepChoice};
use crate::states::driving_rest_states::short_stay::{ShortStay, BREAK_ROW, FOOD_ROW};

mod cat_scale;
mod cattle;
mod chains;
pub(super) mod restart;
mod services;

const REST_STOP_INTRO_HELP: &str =
    "Select a choice. Sleep reads a preview. Select again to sleep. Breaks and sleep move the clock and the deadline.";

/// One-time menu focus when the driver arrives at a selected rest stop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RestFocus {
    Default,
    Break,
    Sleep,
    /// The fuel row, for a stop planned for fuel.
    Fuel,
    /// The food and coffee row, for a stop planned for food.
    Food,
}

/// Which wear meter a road shop is selling a job on.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WearMeter {
    Tire,
    Brake,
}

pub struct RestStopState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    pub stop: RoadStop,
    preferred_rest: RestFocus,
    pending_sleep: Option<SleepChoice>,
    /// The drive's [`StopVisit`] as of the last row build; written through
    /// the drive, so reopening the menu keeps it.
    visit: StopVisit,
}

impl RestStopState {
    pub fn new(ctx: &GameContext, stop: RoadStop, preferred_rest: RestFocus) -> Self {
        RestStopState {
            menu: MenuCore::new(&stop.spoken_name_opening()).with_intro_help(REST_STOP_INTRO_HELP),
            driving: DriveRef::active(ctx),
            stop,
            preferred_rest,
            pending_sleep: None,
            visit: StopVisit::default(),
        }
    }

    /// The same screen over a drive the caller already shares (tests).
    pub fn with_drive(driving: DriveRef, stop: RoadStop, prefer_sleep: bool) -> Self {
        RestStopState {
            menu: MenuCore::new(&stop.spoken_name_opening()).with_intro_help(REST_STOP_INTRO_HELP),
            driving,
            stop,
            preferred_rest: if prefer_sleep {
                RestFocus::Sleep
            } else {
                RestFocus::Default
            },
            pending_sleep: None,
            visit: StopVisit::default(),
        }
    }

    /// `enter()` run while the drive is still in hand -- see `drive_ref`.
    pub fn enter_over_drive(&mut self, ctx: &mut GameContext, driving: &mut DrivingState) {
        self.pending_sleep = None;
        let items = self.rows(ctx, driving);
        self.menu.items = items;
        self.place_cursor(ctx);
        if let Some(key) = self.menu.open_sound_key.clone() {
            ctx.audio.play(&key);
        }
        self.announce_over_drive(ctx, driving);
    }

    fn place_cursor(&mut self, ctx: &GameContext) {
        if self.preferred_rest != RestFocus::Default {
            let focus = self.preferred_rest;
            let wanted = |text: &str| match focus {
                RestFocus::Break => text == BREAK_ROW,
                // The fuel row's label carries the gallons and the price.
                RestFocus::Fuel => text.starts_with("Refuel ") || text.starts_with("Fuel: "),
                RestFocus::Food => text == FOOD_ROW,
                RestFocus::Sleep | RestFocus::Default => {
                    text == "Sleep 10 hours" || text == "Sleep 8 hours"
                }
            };
            let index = self
                .menu
                .items
                .iter()
                .position(|item| wanted(&item.text(self, ctx)))
                .unwrap_or(0);
            self.menu.index = index;
            // This is an arrival hint, not a permanent focus policy.
            // Returning from a submenu must preserve the row the player
            // invoked.
            self.preferred_rest = RestFocus::Default;
        } else {
            self.menu.index = self.menu.index.min(self.menu.items.len().saturating_sub(1));
        }
    }

    /// Read the result before changing time, fatigue, money, or the ELD.
    /// A second Enter on the same row accepts; moving focus cancels it.
    fn confirm_sleep(
        &mut self,
        ctx: &mut GameContext,
        choice: SleepChoice,
        fatigue_floor: f64,
    ) -> bool {
        if self.pending_sleep == Some(choice) {
            self.pending_sleep = None;
            return false;
        }
        self.pending_sleep = Some(choice);
        let gains_nothing = {
            let p = profile_of(ctx);
            p.hos.driving_min <= 0.0 && p.hos.duty_min <= 0.0 && p.fatigue <= fatigue_floor
        };
        let Some(preview) = self.driving.read(|d| sleep_preview(d, ctx, choice)) else {
            self.pending_sleep = None;
            return true;
        };
        ctx.audio.play("ui/warning");
        if gains_nothing {
            ctx.say(&format!("You are already rested. {preview}"));
        } else {
            ctx.say(&preview);
        }
        true
    }

    fn announce_over_drive(&mut self, ctx: &mut GameContext, d: &mut DrivingState) {
        ctx.set_ambient(Some(poi_ambient_key(&self.stop, d.trip.local_hour())));
        let mut parts = vec![format!("{}.", self.stop.spoken_name_opening())];
        if self.stop.stop_type == "weigh_station" {
            // An inspection, not hospitality: no parking chatter, no live lot
            // lookup, no brand amenities. The one-template announce read a
            // scale its truck-stop script -- "no truck parking... Loyalty
            // program: Loyalty points: 0" -- at an open scale (owner
            // playtest, 2026-08-20).
            //
            // A closed scale's first row is its own name ("I-40 Weigh
            // Station is closed"), so leading with the name as well read it
            // twice in one breath (QA, 2026-10-07). The row says it once.
            let row = self.current_text(ctx);
            if row.starts_with(&self.stop.spoken_name()) {
                // Closed: the row is the whole news.
                ctx.say(&row);
                return;
            }
            parts.push("Inspection station.".to_string());
            parts.push(format!("It is {}.", clock_text(d.trip.local_hour())));
            parts.push(row);
            ctx.say(&parts.join(" "));
            return;
        }
        let parking_text = self.stop.parking_text();
        if !parking_text.is_empty() {
            parts.push(format!("{parking_text}."));
        }
        if self.stop.fuel_callout() == Some(false) {
            // Said on arrival too: a driver who pulled in low needs to know
            // before the menu, not by finding no Refuel row (issue #272).
            // Only where pumps could be expected, as in the stop callout.
            parts.push("No fuel here.".to_string());
        }
        // Check real-time parking availability if enabled
        if ctx.settings.real_parking && d.trip.parking_provider.is_some() {
            if let Some(availability) = check_parking_availability(d) {
                parts.push(availability);
            }
        }
        let brand_text = spoken_amenities(&self.stop.name, &self.stop.stop_type);
        if !brand_text.is_empty() {
            parts.push(format!("{brand_text}."));
        }
        parts.push(format!("It is {}.", clock_text(d.trip.local_hour())));
        parts.push(self.current_text(ctx));
        ctx.say(&parts.join(" "));
    }

    fn rows(&mut self, ctx: &mut GameContext, d: &mut DrivingState) -> Vec<MenuItem<Self>> {
        self.visit = d.stop_visit(&self.stop).clone();
        let actions: Vec<String> = self.stop.actions.clone();
        let has = |name: &str| actions.iter().any(|a| a == name);
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        let is_scale = self.stop.stop_type == "weigh_station";

        // Loyalty program status -- at hospitality stops. A scale hands out
        // citations, not points.
        if !is_scale {
            let summary = {
                let loyalty = &profile_of(ctx).loyalty;
                let volume = (!ctx.settings.imperial())
                    .then(|| ctx.settings.volume_text(loyalty.total_gallons_fueled));
                loyalty.summary_in(volume.as_deref())
            };
            items.push(
                MenuItem::new(
                    format!("Loyalty program: {summary}"),
                    |s: &mut Self, ctx| s.loyalty_menu(ctx),
                )
                .help("Loyalty points, shower credits, and rewards."),
            );
        }

        if has("fuel") {
            // Kill switch while parked: the road's engine key is out of reach
            // under this menu, and the fuel island refuses a running tractor.
            items.push(self.facility_engine_item_for(d.trip.truck.engine_on));
            let label = self.fuel_label(ctx, d);
            items.push(
                MenuItem::new(label, |s: &mut Self, ctx| s.refuel(ctx))
                    .help(self.fuel_help(ctx, d)),
            );
        }
        if self.has_cat_scale() {
            items.push(self.cat_scale_item(ctx, d));
        }
        if self.sells_chains(ctx, d) {
            items.push(self.chains_item(ctx));
        }
        if has("food") {
            items.push(Self::food_item());
        }
        if has("break") {
            items.push(Self::break_item());
        }
        if has("sleep") {
            // Under Canadian rules 8 hours in the berth is the whole reset,
            // so the 8-hour split row would be the reset row twice.
            let night = full_night_of(ctx, d);
            let canadian_reset = night == FullNight::EightHours;
            let split_hours: &[i64] = if canadian_reset {
                &[2, 3, 7]
            } else {
                &[2, 3, 7, 8]
            };
            for &hours in split_hours {
                items.push(
                    MenuItem::new(
                        format!("Sleep {hours} hours in sleeper berth"),
                        move |s: &mut Self, ctx: &mut GameContext| s.sleeper_split_rest(ctx, hours),
                    )
                    .help(sleeper_split_help(hours)),
                );
            }
            // A rest already under way counts toward the ten hours, so the
            // reset row only asks for what is left of it.
            let (label, help) = match (hos_of(ctx).reset_minutes_left(night), canadian_reset) {
                (Some(left), false) => (
                    format!(
                        "Sleep {} more to finish a 10-hour reset",
                        hos::duration_text(left / 60.0)
                    ),
                    "Your rest since you parked counts. Full reset and fresh hours of service. \
                     Sleep takes off 8 points of fatigue an hour.",
                ),
                (Some(left), true) => (
                    format!(
                        "Sleep {} more to finish an 8-hour reset",
                        hos::duration_text(left / 60.0)
                    ),
                    "Your rest since you parked counts. Full reset and fresh hours of service. \
                     Sleep takes off 8 points of fatigue an hour.",
                ),
                (None, false) => (
                    "Sleep 10 hours".to_string(),
                    "Full reset and fresh hours of service. Sleep takes off 8 points of fatigue \
                     an hour. Clock and deadline advance 10 hours.",
                ),
                (None, true) => (
                    "Sleep 8 hours".to_string(),
                    "Full reset and fresh hours of service. Sleep takes off 8 points of fatigue \
                     an hour. Clock and deadline advance 8 hours.",
                ),
            };
            items.push(MenuItem::new(label, |s: &mut Self, ctx| s.sleep(ctx)).help(help));
            items.extend(Self::restart_item(ctx, RestartPlace::Sleeper));
        } else if !is_scale && (has("park") || has("break")) {
            // No proper sleeper facility here, but you can always bed down in
            // the lot -- a legal reset, just cramped and poor rest. Except at
            // a scale: nobody sleeps in an active inspection facility, and
            // there is no motel on the far side of the platform (the scale
            // was offering both, plus a loyalty program -- owner playtest,
            // 2026-08-20). And only where the stop has a lot to stand in (it
            // parks or takes a break): a fuel-only stop, a cardlock or a lone
            // pump, has neither a lot to sleep in nor a motel, so both rows
            // are absent and nothing is said.
            let night = full_night_of(ctx, d);
            let canadian_reset = night == FullNight::EightHours;
            let (lot_label, lot_help) = if canadian_reset {
                (
                    "Sleep 8 hours in the lot",
                    "A legal 8-hour reset with poor rest: 5 points of fatigue off an hour, \
                     never below 30. Clock and deadline advance 8 hours.",
                )
            } else {
                (
                    "Sleep 10 hours in the lot",
                    "A legal 10-hour reset with poor rest: 5 points of fatigue off an hour, \
                     never below 30. Clock and deadline advance 10 hours.",
                )
            };
            items.push(
                MenuItem::new(lot_label, |s: &mut Self, ctx| s.emergency_lot_sleep(ctx))
                    .help(lot_help),
            );
            items.extend(Self::restart_item(ctx, RestartPlace::Lot));
            let (motel_label, motel_help) = if canadian_reset {
                (
                    format!(
                        "Motel room: sleep 8 hours for {} dollars",
                        fmt_f(MOTEL_COST, 0)
                    ),
                    "A real bed, paid from your own pocket. Legal reset, and sleep takes off 8 \
                     points of fatigue an hour. Clock and deadline advance 8 hours.",
                )
            } else {
                (
                    format!(
                        "Motel room: sleep 10 hours for {} dollars",
                        fmt_f(MOTEL_COST, 0)
                    ),
                    "A real bed, paid from your own pocket. Legal reset, and sleep takes off 8 \
                     points of fatigue an hour. Clock and deadline advance 10 hours.",
                )
            };
            items.push(
                MenuItem::new(motel_label, |s: &mut Self, ctx| s.motel_sleep(ctx)).help(motel_help),
            );
            items.extend(Self::restart_item(ctx, RestartPlace::Motel));
        }
        if has("repair") {
            items.push(
                MenuItem::new("Use repair service", |s: &mut Self, ctx| s.repair(ctx))
                    .help("Shop repair of truck damage."),
            );
        }
        if !solvency::out_of_pocket_options(profile_of(ctx)).is_empty() {
            let owed = solvency::money_text(solvency::debt_owed(profile_of(ctx)));
            items.push(
                MenuItem::new(
                    format!("Pay down what you owe: {owed} owed"),
                    |_s: &mut Self, ctx: &mut GameContext| {
                        let state = PayDebtState::new();
                        ctx.push_state(state);
                    },
                )
                .help("Your own cash toward the balance you owe."),
            );
        }
        let brand = classify_brand(&self.stop.name);
        if let Some(brand) = brand {
            if brand.tier == "travel_center" {
                let tire_help = if brand.signature.contains(&"tires") {
                    format!(
                        "{} has a tire bay, close to the terminal garage price and fast. \
                         Company drivers bill the carrier, owner-operators pay.",
                        brand.spoken
                    )
                } else {
                    format!(
                        "{} mounts tires at a markup over the terminal garage. Love's and \
                         Speedco do it cheaper and faster.",
                        brand.spoken
                    )
                };
                items.push(
                    MenuItem::new(self.tire_label(ctx, d), |s: &mut Self, ctx| {
                        s.service_wear(ctx, WearMeter::Tire)
                    })
                    .help(tire_help),
                );
                if brand.signature.contains(&"repair") {
                    items.push(
                        MenuItem::new(brake_label(ctx, d), |s: &mut Self, ctx| {
                            s.service_wear(ctx, WearMeter::Brake)
                        })
                        .help(format!(
                            "{} relines brake shoes at a markup over the terminal garage. \
                             Company drivers bill the carrier, owner-operators pay.",
                            brand.spoken
                        )),
                    );
                }
            }
        }
        let action_refs: Vec<&str> = actions.iter().map(String::as_str).collect();
        let mut stop_buffs = buffs_for_stop(&self.stop.name, &action_refs);
        if brand.is_some_and(|b| b.bans_big_rigs) && !d.job.bobtail {
            // The famous ban: with a trailer on you never got past the lot.
            stop_buffs = Vec::new();
        }
        for buff in stop_buffs {
            let label = self.buff_label(buff);
            let id = buff.id.clone();
            items.push(
                MenuItem::new(label, move |s: &mut Self, ctx: &mut GameContext| {
                    s.buy_buff(ctx, &id)
                })
                .help(buff.help.clone()),
            );
        }
        if has("roadside_assistance") {
            items.push(
                MenuItem::new("Call roadside assistance", |s: &mut Self, ctx| {
                    s.roadside_assistance(ctx)
                })
                .help("A field repair from the listed roadside service."),
            );
        }
        if has("towing") {
            items.push(
                MenuItem::new("Request towing service", |s: &mut Self, ctx| {
                    s.roadside_assistance(ctx)
                })
                .help("Roadside help from the listed towing service."),
            );
        }
        if has("inspect") && !self.visit.inspected {
            // Only an open scale has anyone to check in with. A closed one
            // used to offer the check-in all the same and answer it as an
            // open one did, waving a tester "straight back onto the highway"
            // six miles short of the open scale he had been warned about
            // (log, 2026-10-07). The closed row says so instead.
            let item = if d.scale_is_open(&self.stop) {
                MenuItem::new("Check in at inspection station", |s: &mut Self, ctx| {
                    s.inspect(ctx)
                })
                .help("Records the inspection check-in.")
            } else {
                MenuItem::new(
                    format!("{} is closed", self.stop.spoken_name()),
                    |s: &mut Self, ctx| s.inspect(ctx),
                )
                .help("The weigh station is closed.")
            };
            items.push(item);
        }
        if d.livestock().is_some_and(|load| load.aboard()) {
            items.push(self.check_cattle_item());
        }
        let walk_min = fmt_f(d.hand_work_min(ctx, walk_around_minutes(d)), 0);
        if d.trip.truck.doubles_hooked() {
            // A set of doubles is two trailers and a dolly to walk: the row
            // shows the longer time so it is on screen as well as spoken.
            items.push(
                MenuItem::new(
                    format!(
                        "Walk around the truck, both trailers and the dolly, {walk_min} minutes"
                    ),
                    |s: &mut Self, ctx| s.walk_around(ctx),
                )
                .help(
                    "A pre-trip walk-around: what an inspector would find on the tractor, both \
                     trailers and the converter dolly. Longer than a single trailer's.",
                ),
            );
        } else {
            items.push(
                MenuItem::new("Walk around the truck", |s: &mut Self, ctx| {
                    s.walk_around(ctx)
                })
                .help(format!(
                    "A pre-trip walk-around: what an inspector would find on the tractor and the \
                     trailer. {walk_min} minutes on duty."
                )),
            );
        }
        if has("save") {
            items.push(Self::save_item());
        }
        if self.pay_advance_available(ctx) {
            items.push(
                MenuItem::new(self.pay_advance_label(ctx), |s: &mut Self, ctx| {
                    s.request_pay_advance(ctx)
                })
                .help("Cash against this load, repaid from the delivery settlement."),
            );
        }
        items.push(
            MenuItem::new("Back to the road", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Leave this stop and return to driving."),
        );
        items
    }

    fn pay_advance_label(&self, ctx: &GameContext) -> String {
        let p = profile_of(ctx);
        let grant = pay_advance_grant(p.money(), p.pay_advance, p.pay_advance_used_for_load);
        if grant > 0.0 {
            return format!("Request pay advance: {} dollars", fmt_grouped(grant, 0));
        }
        "Request pay advance".to_string()
    }

    fn pay_advance_available(&self, ctx: &GameContext) -> bool {
        let p = profile_of(ctx);
        pay_advance_grant(p.money(), p.pay_advance, p.pay_advance_used_for_load) > 0.0
    }

    fn request_pay_advance(&mut self, ctx: &mut GameContext) {
        let (grant, reason) = {
            let p = profile_of(ctx);
            (
                pay_advance_grant(p.money(), p.pay_advance, p.pay_advance_used_for_load),
                pay_advance_unavailable_reason(
                    p.money(),
                    p.pay_advance,
                    p.pay_advance_used_for_load,
                ),
            )
        };
        if grant <= 0.0 {
            ctx.audio.play("ui/error");
            ctx.say(&reason);
            return;
        }
        let (money, advance) = {
            let p = profile_mut_of(ctx);
            p.earn(grant);
            p.pay_advance = round_py_n(p.pay_advance + grant, 2);
            p.pay_advance_used_for_load = true;
            (p.money(), p.pay_advance)
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        let destination = self
            .driving
            .read(|d| d.job.destination.clone())
            .unwrap_or_default();
        ctx.say(&format!(
            "Advance {} dollars on the {destination} load, due at delivery. You have {}, owe \
             {}.",
            fmt_grouped(grant, 0),
            fmt_grouped(money, 0),
            fmt_grouped(advance, 0)
        ));
        self.refresh(ctx, true);
    }

    /// Show loyalty program details and redemption options.
    fn loyalty_menu(&mut self, ctx: &mut GameContext) {
        let state = LoyaltyRewardsState::new(self.driving.clone(), self.stop.clone());
        ctx.push_state(state);
    }

    fn sleeper_split_rest(&mut self, ctx: &mut GameContext, hours: i64) {
        if self.confirm_sleep(ctx, SleepChoice::Sleeper(hours), 0.0) {
            return;
        }
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            let minutes = (hours * 60) as f64;
            let engine_off = shut_down_engine(d, ctx);
            advance_rest_clock(d, ctx, minutes, Some("sleeper_berth"), "");
            settle_lift_before_sleep(d, ctx);
            let completed = hos_mut_of(ctx).sleeper_split_rest(minutes);
            let full_reset = hos_of(ctx)
                .history
                .last()
                .is_some_and(|event| event.source == "full_reset");
            {
                let p = profile_mut_of(ctx);
                p.fatigue = if full_reset {
                    hos::rest_sleep(p.fatigue, minutes)
                } else {
                    hos::rest_sleeper_split(p.fatigue, minutes, completed)
                };
            }
            let mode = ctx.settings.hos_mode.clone();
            let status = if full_reset {
                "Driving and duty clocks reset. ".to_string()
            } else if completed {
                format!("Sleeper split credited. {} ", hos_of(ctx).summary(&mode))
            } else {
                // A rest that did NOT reset the shift leads with that
                // consequence: the old wording buried "split pending" in one
                // clause and the owner drove into a window violation
                // believing he had hours left (2026-07-24). The countdown
                // warnings re-arm too, so the 60- and 30-minute window calls
                // speak again after waking.
                hos_mut_of(ctx).re_arm_warnings();
                let pending = hos_of(ctx)
                    .split_pending_spoken()
                    .unwrap_or("Sleeper berth rest recorded.")
                    .to_string();
                if hos::HOS_NON_ENFORCED_MODES.contains(&mode.as_str()) {
                    format!("{pending} ")
                } else {
                    let duty_limit = hos::limits(&mode).map(|(_, duty, _)| duty).unwrap_or(0.0);
                    let duty_left_h = (duty_limit - hos_of(ctx).duty_min).max(0.0) / 60.0;
                    let window = if duty_left_h <= 0.0 {
                        let night = full_night_of(ctx, d);
                        let canadian_reset = night == FullNight::EightHours;
                        let reset = match (hos_of(ctx).reset_minutes_left(night), canadian_reset) {
                            (Some(left), false) => format!(
                                "sleep {} more here to finish a 10-hour reset",
                                hos::duration_text(left / 60.0)
                            ),
                            (Some(left), true) => format!(
                                "sleep {} more here to finish an 8-hour reset",
                                hos::duration_text(left / 60.0)
                            ),
                            (None, false) => "take a full 10-hour reset".to_string(),
                            (None, true) => "take a full 8-hour reset".to_string(),
                        };
                        format!(
                            "Warning: this sleep did not reset your hours, and your duty window \
                             has closed. Finish the split or {reset} before driving. "
                        )
                    } else {
                        let closes = clock_text((d.trip.local_hour() + duty_left_h) % 24.0);
                        if minutes >= hos::SPLIT_LONG_MIN {
                            // The long half of a split stops the duty window
                            // while it runs (FMCSA 395.1(g)(1)(iii)(B)), so
                            // the driver wakes with the hours they went to
                            // bed with, and is told so.
                            format!(
                                "This sleep did not reset your hours, but your duty window \
                                 paused while you slept. It closes in {} hours, at {closes}. ",
                                fmt_f(duty_left_h, 1)
                            )
                        } else {
                            format!(
                                "This sleep did not reset your hours. Your duty window closes \
                                 in {} hours, at {closes}. ",
                                fmt_f(duty_left_h, 1)
                            )
                        }
                    };
                    format!("{window}{pending} ")
                }
            };
            format!(
                "{engine_off}You slept {hours} hours in the sleeper berth. It is {}. {status}{}{}",
                clock_text(d.trip.local_hour()),
                deadline_text(d, ctx),
                wake_air_instruction(d)
            )
        }) else {
            return;
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        // No Five-by-Two here: the badge is a full ten hours asleep in the
        // lot, and a split rest is the night broken in two.
        ctx.award_restart_badges(false);
        self.refresh(ctx, true);
    }

    fn sleep(&mut self, ctx: &mut GameContext) {
        if self.confirm_sleep(ctx, SleepChoice::Sleeper(10), 0.0) {
            return;
        }
        let before_fatigue = profile_of(ctx).fatigue;
        // Finishing a reset already under way sleeps only what is left of it,
        // which is not the full ten hours the badge asks for.
        let stays = self
            .driving
            .read(|d| d.trip.road_ahead_stays_in_canada())
            .unwrap_or(false);
        let night = hos_of(ctx).full_night(stays);
        let full_night = hos_of(ctx).reset_minutes_left(night).is_none();
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            let engine_off = shut_down_engine(d, ctx);
            let owed = hos_of(ctx).reset_minutes_left(night);
            let canadian_reset = night == FullNight::EightHours;
            let minutes = owed.unwrap_or(night.minutes());
            advance_rest_clock(d, ctx, minutes, Some("sleeper_berth"), "");
            settle_lift_before_sleep(d, ctx);
            let cycle_before = hos_of(ctx).cycle_used_min();
            hos_mut_of(ctx).rest_reset(night);
            let clocks = clocks_reset_text(hos_of(ctx).cycle_restarted_since(cycle_before));
            // Sleep takes fatigue off by the hour, so a driver who went to
            // bed exhausted can wake with some left.
            let rested = {
                let p = profile_mut_of(ctx);
                p.fatigue = hos::rest_sleep(p.fatigue, minutes);
                Alertness::from_fatigue(p.fatigue) == Alertness::Rested
            };
            let slept = match (owed, rested, canadian_reset) {
                (Some(left), true, false) => format!(
                    "You slept {} more, 10 hours of rest in a row, and woke rested.",
                    hos::duration_text(left / 60.0)
                ),
                (Some(left), false, false) => format!(
                    "You slept {} more, 10 hours of rest in a row, and woke still tired.",
                    hos::duration_text(left / 60.0)
                ),
                (Some(left), true, true) => format!(
                    "You slept {} more, 8 hours of rest in a row, and woke rested.",
                    hos::duration_text(left / 60.0)
                ),
                (Some(left), false, true) => format!(
                    "You slept {} more, 8 hours of rest in a row, and woke still tired.",
                    hos::duration_text(left / 60.0)
                ),
                (None, true, false) => "You slept 10 hours and woke rested.".to_string(),
                (None, false, false) => "You slept 10 hours and woke still tired.".to_string(),
                (None, true, true) => "You slept 8 hours and woke rested.".to_string(),
                (None, false, true) => "You slept 8 hours and woke still tired.".to_string(),
            };
            // Said after the clocks when the night left the cycle used up.
            let clocks = format!("{clocks}{}", restart::still_used_up(ctx));
            format!(
                "{engine_off}{slept} It is {}. {clocks} {}{}",
                clock_text(d.trip.local_hour()),
                deadline_text(d, ctx),
                wake_air_instruction(d)
            )
        }) else {
            return;
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        // Five-by-Two is ten hours in the bunk: the 8-hour Canadian night
        // does not earn it (director, 2026-11-25).
        if full_night && night == FullNight::TenHours {
            ctx.award_achievement("slept_on_route");
        }
        if Alertness::from_fatigue(before_fatigue) < Alertness::Severe {
            ctx.award_achievement("sleep_before_exhaustion");
        }
        ctx.award_restart_badges(false);
        self.refresh(ctx, true);
    }

    /// A paid bed where the parking is rough: a legal reset with real rest.
    ///
    /// Lodging is personal money even for company drivers -- the carrier pays
    /// for the truck, not the room.
    fn motel_sleep(&mut self, ctx: &mut GameContext) {
        let money = profile_of(ctx).money();
        if money < MOTEL_COST {
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "A motel room costs {} dollars and you have {}.",
                fmt_grouped(MOTEL_COST, 0),
                fmt_grouped(money, 0)
            ));
            return;
        }
        if self.confirm_sleep(ctx, SleepChoice::Motel, 0.0) {
            return;
        }
        profile_mut_of(ctx).spend(MOTEL_COST);
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            // A motel bed is still a real sleep: no truck idles all night
            // just because the driver is not in it. Every other sleep option
            // shuts the engine down first; the motel room was the one path
            // that skipped it while still sending the driver off to bed.
            let engine_off = shut_down_engine(d, ctx);
            let night = full_night_of(ctx, d);
            let reset_min = night.minutes();
            let canadian_reset = night == FullNight::EightHours;
            advance_rest_clock(d, ctx, reset_min, None, "");
            settle_lift_before_sleep(d, ctx);
            let cycle_before = hos_of(ctx).cycle_used_min();
            hos_mut_of(ctx).rest_reset(night);
            let clocks = clocks_reset_text(hos_of(ctx).cycle_restarted_since(cycle_before));
            {
                let p = profile_mut_of(ctx);
                p.fatigue = hos::rest_sleep(p.fatigue, reset_min);
            }
            let money = profile_of(ctx).money();
            let clocks = format!("{clocks}{}", restart::still_used_up(ctx));
            if canadian_reset {
                format!(
                    "{engine_off}Motel, {} dollars, 8 hours. {}. {clocks} \
                     {} dollars. {}{}",
                    fmt_grouped(MOTEL_COST, 0),
                    clock_text(d.trip.local_hour()),
                    fmt_grouped(money, 0),
                    deadline_text(d, ctx),
                    wake_air_instruction(d)
                )
            } else {
                format!(
                    "{engine_off}Motel, {} dollars, 10 hours. {}. {clocks} \
                     {} dollars. {}{}",
                    fmt_grouped(MOTEL_COST, 0),
                    clock_text(d.trip.local_hour()),
                    fmt_grouped(money, 0),
                    deadline_text(d, ctx),
                    wake_air_instruction(d)
                )
            }
        }) else {
            return;
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        // No Five-by-Two here: the badge is ten hours IN THE BUNK, and a
        // motel bed is the night you specifically did not spend in it (owner
        // report, 2026-08-20). The cramped-lot sleep keeps the award -- the
        // stop has no beds, so the lot night IS a bunk night.
        ctx.award_restart_badges(false);
        // The lot's restart row reads the rest so far, or goes once done.
        self.refresh(ctx, true);
    }

    /// Bed down in a break/fuel stop's lot when out of hours: a legal HOS
    /// reset, but cramped poor rest (no proper sleeper), so you wake still
    /// tired. No shoulder fine -- a lot is more legitimate than the freeway.
    fn emergency_lot_sleep(&mut self, ctx: &mut GameContext) {
        if self.confirm_sleep(ctx, SleepChoice::Lot, hos::FATIGUE_SHOULDER_FLOOR) {
            return;
        }
        let ten_hours = self
            .driving
            .read(|d| d.trip.road_ahead_stays_in_canada())
            .is_some_and(|stays| hos_of(ctx).full_night(stays) == FullNight::TenHours);
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            let engine_off = shut_down_engine(d, ctx);
            let night = full_night_of(ctx, d);
            let reset_min = night.minutes();
            let canadian_reset = night == FullNight::EightHours;
            advance_rest_clock(d, ctx, reset_min, None, "");
            settle_lift_before_sleep(d, ctx);
            let cycle_before = hos_of(ctx).cycle_used_min();
            hos_mut_of(ctx).rest_reset(night);
            let clocks = clocks_reset_text(hos_of(ctx).cycle_restarted_since(cycle_before));
            {
                let p = profile_mut_of(ctx);
                p.fatigue = hos::rest_poor_sleep(p.fatigue, reset_min);
            }
            let clocks = format!("{clocks}{}", restart::still_used_up(ctx));
            if canadian_reset {
                format!(
                    "{engine_off}Slept 8 hours in the lot. {}. {clocks} Rest was poor. \
                     {}{}",
                    clock_text(d.trip.local_hour()),
                    deadline_text(d, ctx),
                    wake_air_instruction(d)
                )
            } else {
                format!(
                    "{engine_off}Slept 10 hours in the lot. {}. {clocks} Rest was poor. \
                     {}{}",
                    clock_text(d.trip.local_hour()),
                    deadline_text(d, ctx),
                    wake_air_instruction(d)
                )
            }
        }) else {
            return;
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        if ten_hours {
            ctx.award_achievement("slept_on_route");
        }
        ctx.award_restart_badges(false);
        // The restart row reads the rest so far, or goes once it is done.
        self.refresh(ctx, true);
    }

    /// [`FacilityEngine::facility_engine_item`] with engine state already in hand
    /// (rows build inside a DriveRef borrow).
    fn facility_engine_item_for(&self, engine_on: bool) -> MenuItem<Self> {
        if engine_on {
            MenuItem::new(
                crate::states::driving_core::FACILITY_ENGINE_SHUT_DOWN_ITEM,
                |s: &mut Self, ctx| s.toggle_facility_engine(ctx),
            )
            .help("Engine off while parked, no fuel burned. Required before the fuel island.")
        } else {
            MenuItem::new(
                crate::states::driving_core::FACILITY_ENGINE_START_ITEM,
                |s: &mut Self, ctx| s.toggle_facility_engine(ctx),
            )
            .help("Starts the engine. The parking brake needs 100 psi of air.")
        }
    }
}

impl FacilityEngine for RestStopState {
    fn facility_engine_on(&self, _ctx: &GameContext) -> bool {
        self.driving
            .read(|d| d.trip.truck.engine_on)
            .unwrap_or(false)
    }

    fn with_facility_truck<R>(
        &mut self,
        ctx: &mut GameContext,
        f: impl FnOnce(&mut GameContext, &mut TruckState) -> R,
    ) -> R {
        self.driving
            .clone()
            .call(self, ctx, |_s, ctx, d| f(ctx, &mut d.trip.truck))
            .expect("the rest stop keeps the drive under it")
    }
}

/// Check real-time parking availability for this stop.
fn check_parking_availability(d: &mut DrivingState) -> Option<String> {
    let provider = d.trip.parking_provider.clone()?;
    // Ask about the state and position the truck is actually at; the provider
    // answers empty for states without a live TPIMS feed.
    let state = d.trip.state_at(None);
    if state.is_empty() {
        return None;
    }
    let (latitude, longitude) = d.trip.latlon_at(None);
    let locations = provider.get_available_locations_near(&state, latitude, longitude, 25.0);
    if locations.is_empty() {
        return Some("No real-time parking data nearby.".to_string());
    }
    // Find the closest location with available spaces
    let total_available: i64 = locations.iter().map(|loc| loc.available.unwrap_or(0)).sum();
    if total_available > 0 {
        Some(format!(
            "Real-time parking: {total_available} spaces available nearby."
        ))
    } else {
        Some("Real-time parking: nearby lots are full.".to_string())
    }
}

fn sleeper_split_help(hours: i64) -> String {
    let pair = match hours {
        2 => "Pairs with 8 hours in the sleeper berth.",
        3 => "Pairs with 7 hours in the sleeper berth.",
        7 => "Pairs with 3 more hours at parking where you can sleep.",
        _ => "Pairs with 2 more hours at parking where you can sleep.",
    };
    format!("{pair} Clock and deadline advance {hours} hours.")
}

fn brake_label(ctx: &GameContext, d: &DrivingState) -> String {
    let wear = d.trip.truck.brake_wear_pct;
    if wear < 1.0 {
        return "Brakes: top shape".to_string();
    }
    if !player_pays_operating_costs(&profile_of(ctx).business_status) {
        return format!("Brake job: {} percent wear, carrier billed", fmt_f(wear, 0));
    }
    let cost = round_py_n(wear * ROAD_BRAKE_COST_PER_PCT, 2);
    format!(
        "Brake job: {} percent wear for {} dollars",
        fmt_f(wear, 0),
        fmt_grouped(cost, 0)
    )
}

/// Python `str.capitalize()`.
fn py_capitalize(text: &str) -> String {
    ff_core::data::world_models::py_capitalize(text)
}

impl FuelPump for RestStopState {
    fn drive(&self) -> &DriveRef {
        &self.driving
    }

    fn stop(&self) -> &RoadStop {
        &self.stop
    }
}

impl ShortStay for RestStopState {}

impl Menu for RestStopState {
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

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let built = self
            .driving
            .clone()
            .call(self, ctx, |s, ctx, d| s.rows(ctx, d));
        keep_rows(built, &self.driving, &self.menu.items)
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        self.pending_sleep = None;
        let items = self.build_items(ctx);
        self.menu.items = items;
        self.place_cursor(ctx);
        if let Some(key) = self.menu.open_sound_key.clone() {
            ctx.audio.play(&key);
        }
        self.announce_entry(ctx);
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        self.driving
            .clone()
            .call(self, ctx, |s, ctx, d| s.announce_over_drive(ctx, d));
    }

    // Moving off a sleep item withdraws its pending double-select
    // confirmation, so a stale preview can never sleep you
    // silently later.
    fn move_by(&mut self, ctx: &mut GameContext, delta: i64) {
        self.pending_sleep = None;
        let core = self.menu_mut();
        if core.items.is_empty() {
            return;
        }
        let n = core.items.len() as i64;
        core.index = ((core.index as i64 + delta).rem_euclid(n)) as usize;
        ctx.audio.play("ui/menu_move");
        self.speak_current(ctx);
    }

    fn jump(&mut self, ctx: &mut GameContext, index: usize) {
        self.pending_sleep = None;
        let core = self.menu_mut();
        if core.items.is_empty() {
            return;
        }
        core.index = index.min(core.items.len() - 1);
        ctx.audio.play("ui/menu_move");
        self.speak_current(ctx);
    }

    fn first_letter_jump(&mut self, ctx: &mut GameContext, ch: &str) {
        self.pending_sleep = None;
        let n = self.menu().items.len();
        if n == 0 {
            return;
        }
        let start = self.menu().index;
        for offset in 1..=n {
            let i = (start + offset) % n;
            if self.menu().items[i]
                .text(self, ctx)
                .to_lowercase()
                .starts_with(ch)
            {
                self.jump(ctx, i);
                return;
            }
        }
    }

    fn presence(&self, ctx: &GameContext) -> Option<PresenceState> {
        let detail = self
            .driving
            .read(|d| d.presence_state(ctx).map(|p| p.detail).unwrap_or_default())
            .unwrap_or_default();
        Some(PresenceState::new("Resting at a stop", &detail))
    }

    fn online_presence(&self, ctx: &GameContext) -> Option<PresenceState> {
        self.presence(ctx)
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        // Leaving an open scale runs the check-in first (owner ruling,
        // 2026-09-28): Back used to skip the inspection a flagged record is
        // pulled in for.
        let checked = if self.check_in_pending() {
            self.check_in(ctx)
        } else {
            None
        };
        // Leaving one scale with another open one still ahead: say which
        // was which once the truck is back on the road (tester log,
        // 2026-10-07: a closed scale six miles short of the announced one).
        let here = (self.stop.stop_type == "weigh_station").then(|| self.stop.spoken_name());
        let engine_on = self
            .driving
            .read(|d| {
                d.note_scale_reannounce(here.as_deref());
                d.trip.truck.engine_on
            })
            .unwrap_or(false);
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
        let back = back_on_the_road_line(engine_on);
        match checked {
            Some((text, waved)) => {
                ctx.say(&text);
                ctx.say_with(back, Say::queued());
                if waved {
                    record_inspection(ctx);
                }
            }
            None => ctx.say_with(back, Say::new()),
        }
    }
}

impl_state_for_menu!(RestStopState);

/// How long the driver's own walk-around takes with what is hooked now: the
/// stock walk-around, and more for a set of doubles
/// (`models::doubles::walk_around_extra_min`).
pub fn walk_around_minutes(d: &DrivingState) -> f64 {
    let units = if d.trip.truck.doubles_hooked() {
        d.trip.truck.trailer_set.units
    } else {
        1
    };
    WALK_AROUND_MIN + ff_core::models::doubles::walk_around_extra_min(units)
}

/// What a full night resets. A sleep that finished a cycle restart says so
/// (R1 review, 2026-11-22): before, the cycle cleared without a word, and
/// the logbook was the only place to learn the week had started over.
fn clocks_reset_text(cycle_restarted: bool) -> &'static str {
    if cycle_restarted {
        "Driving, duty and cycle clocks reset."
    } else {
        "Driving and duty clocks reset."
    }
}
