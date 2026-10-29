//! The spoken route POI menu: actions come from the corridor metadata
//! (`RestStopState`).

use ff_core::achievements::increment_stat;
use ff_core::data::amenities::{classify_brand, spoken_amenities};
use ff_core::data::buffs::{buffs_for_stop, Buff};
use ff_core::models::solvency;
use ff_core::pyfmt::{fmt_f, fmt_grouped, round_py_int, round_py_n};
use ff_core::sim::hos;
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
    advance_rest_clock, clock_text, deadline_text, hos_mut_of, hos_of, pay_advance_grant,
    pay_advance_unavailable_reason, player_pays_operating_costs, poi_ambient_key, profile_mut_of,
    profile_of, record_inspection, road_repair_cost, shut_down_engine, wake_air_instruction,
    FacilityEngine, RigBuff, FIELD_REPAIR_DAMAGE_PCT, MECHANIC_CALLOUT_FEE, MECHANIC_WAIT_MIN,
    MOTEL_COST, ROAD_BRAKE_COST_PER_PCT, ROAD_BRAKE_MIN, ROAD_TIRE_COST_PER_PCT, ROAD_TIRE_MIN,
    ROAD_TIRE_SPECIALIST_COST_PER_PCT, ROAD_TIRE_SPECIALIST_MIN, WALK_AROUND_MIN, WAVE_THROUGH_MIN,
};
use crate::states::driving_menu_states::{keep_rows, DriveRef};
use crate::states::driving_rest_states::back_on_the_road_line;
use crate::states::driving_rest_states::fuel_pump::FuelPump;
use crate::states::driving_rest_states::loyalty::LoyaltyRewardsState;
use crate::states::driving_rest_states::rest_preview::{sleep_preview, SleepChoice};

mod cat_scale;
mod chains;

const REST_STOP_INTRO_HELP: &str =
    "Select a choice. Sleep reads a preview; select again to sleep. Breaks and sleep move the clock and the deadline.";

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
            menu: MenuCore::new(&stop.spoken_name()).with_intro_help(REST_STOP_INTRO_HELP),
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
            menu: MenuCore::new(&stop.spoken_name()).with_intro_help(REST_STOP_INTRO_HELP),
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
                RestFocus::Break => text == "Take a 30-minute break",
                // The fuel row's label carries the gallons and the price.
                RestFocus::Fuel => text.starts_with("Refuel ") || text.starts_with("Fuel: "),
                RestFocus::Food => text == "Food and coffee break",
                RestFocus::Sleep | RestFocus::Default => text == "Sleep 10 hours",
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
        ctx.audio
            .set_ambient(Some(poi_ambient_key(&self.stop, d.trip.local_hour())));
        let mut parts = vec![format!("{}.", self.stop.spoken_name())];
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
                parts.clear();
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
            let summary = profile_of(ctx).loyalty.summary();
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
                MenuItem::new(label, |s: &mut Self, ctx| s.refuel(ctx)).help(
                    "Fills the tank at the posted diesel price plus a 35 dollar service fee. Buys \
                     what you can afford if cash is short. Engine must be off.",
                ),
            );
        }
        if self.has_cat_scale() {
            items.push(self.cat_scale_item(ctx, d));
        }
        if self.sells_chains(ctx, d) {
            items.push(self.chains_item(ctx));
        }
        if has("food") {
            items.push(
                MenuItem::new("Food and coffee break", |s: &mut Self, ctx| {
                    s.food_break(ctx)
                })
                .help(
                    "Fifteen minutes off duty. Coffee eases fatigue a little, but does not \
                     satisfy the 30-minute break rule.",
                ),
            );
        }
        if has("break") {
            items.push(
                MenuItem::new("Take a 30-minute break", |s: &mut Self, ctx| {
                    s.take_break(ctx)
                })
                .help(
                    "Satisfies the 30-minute break rule and eases fatigue. Clock and deadline \
                     advance half an hour.",
                ),
            );
        }
        if has("sleep") {
            for hours in [2, 3, 7, 8] {
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
            let (label, help) = match hos_of(ctx).reset_minutes_left() {
                Some(left) => (
                    format!(
                        "Sleep {} more to finish a 10-hour reset",
                        hos::duration_text(left / 60.0)
                    ),
                    "Your rest since you parked counts. Full reset, fresh hours of service and \
                     zero fatigue.",
                ),
                None => (
                    "Sleep 10 hours".to_string(),
                    "Full reset, fresh hours of service and zero fatigue. Clock and deadline \
                     advance 10 hours.",
                ),
            };
            items.push(MenuItem::new(label, |s: &mut Self, ctx| s.sleep(ctx)).help(help));
        } else if !is_scale {
            // No proper sleeper facility here, but you can always bed down in
            // the lot -- a legal reset, just cramped and poor rest. Except at
            // a scale: nobody sleeps in an active inspection facility, and
            // there is no motel on the far side of the platform (the scale
            // was offering both, plus a loyalty program -- owner playtest,
            // 2026-08-20).
            items.push(
                MenuItem::new("Sleep 10 hours in the lot", |s: &mut Self, ctx| {
                    s.emergency_lot_sleep(ctx)
                })
                .help(
                    "A legal 10-hour reset with poor rest, you wake still tired. Clock and \
                     deadline advance 10 hours.",
                ),
            );
            items.push(
                MenuItem::new(
                    format!(
                        "Motel room: sleep 10 hours for {} dollars",
                        fmt_f(MOTEL_COST, 0)
                    ),
                    |s: &mut Self, ctx| s.motel_sleep(ctx),
                )
                .help(
                    "A real bed, paid from your own pocket. Legal reset, you wake fresh. Clock \
                     and deadline advance 10 hours.",
                ),
            );
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
                .help("Nobody is at the scale house. There is nothing to check in at.")
            };
            items.push(item);
        }
        items.push(
            MenuItem::new("Walk around the truck", |s: &mut Self, ctx| {
                s.walk_around(ctx)
            })
            .help(
                "A pre-trip walk-around: what an inspector would find on the tractor and the \
                 trailer. Fifteen minutes on duty.",
            ),
        );
        if has("save") {
            items.push(
                MenuItem::new("Save at this stop", |s: &mut Self, ctx| {
                    s.save_here(ctx, false)
                })
                .help("Saves the drive at this stop."),
            );
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

    fn take_break(&mut self, ctx: &mut GameContext) {
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            advance_rest_clock(d, ctx, 30.0, None, "");
            hos_mut_of(ctx).take_break(30.0);
            {
                let p = profile_mut_of(ctx);
                p.fatigue = hos::rest_break(p.fatigue);
            }
            format!(
                "You took a 30-minute break. It is {}. Break requirement reset. {}",
                clock_text(d.trip.local_hour()),
                deadline_text(d, ctx)
            )
        }) else {
            return;
        };
        // Twenty-five proper breaks. The 30-minute one is the break that
        // counts -- the 15-minute food and coffee stop eases fatigue but
        // resets nothing, so it is not one of these. Counted before the
        // save, so a quit afterwards cannot lose it.
        let breaks = increment_stat(profile_mut_of(ctx), "breaks_taken");
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        ctx.award_achievement("break_taken");
        if breaks >= 25 {
            ctx.award_achievement("coffee_regular");
        }
        // The break counts toward a 10-hour reset; the reset row says so.
        self.refresh(ctx, true);
    }

    fn food_break(&mut self, ctx: &mut GameContext) {
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            advance_rest_clock(d, ctx, 15.0, Some("off_duty"), "food and coffee");
            hos_mut_of(ctx).take_break(15.0);
            {
                let p = profile_mut_of(ctx);
                p.fatigue = hos::rest_coffee_break(p.fatigue);
            }
            format!(
                "Coffee break. {}. Eases fatigue; does not count as your 30-minute break. {}",
                clock_text(d.trip.local_hour()),
                deadline_text(d, ctx)
            )
        }) else {
            return;
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        self.refresh(ctx, true);
    }

    fn sleeper_split_rest(&mut self, ctx: &mut GameContext, hours: i64) {
        if self.confirm_sleep(ctx, SleepChoice::Sleeper(hours), 0.0) {
            return;
        }
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            let minutes = (hours * 60) as f64;
            let engine_off = shut_down_engine(d, ctx);
            advance_rest_clock(d, ctx, minutes, None, "");
            let completed = hos_mut_of(ctx).sleeper_split_rest(minutes);
            let full_reset = hos_of(ctx)
                .history
                .last()
                .is_some_and(|event| event.source == "full_reset");
            {
                let p = profile_mut_of(ctx);
                p.fatigue = if full_reset {
                    hos::rest_sleep(p.fatigue)
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
                    .split_pending_summary()
                    .unwrap_or("Sleeper berth rest recorded.")
                    .to_string();
                if hos::HOS_NON_ENFORCED_MODES.contains(&mode.as_str()) {
                    format!("{pending} ")
                } else {
                    let duty_limit = hos::limits(&mode).map(|(_, duty, _)| duty).unwrap_or(0.0);
                    let duty_left_h = (duty_limit - hos_of(ctx).duty_min).max(0.0) / 60.0;
                    let window = if duty_left_h <= 0.0 {
                        let reset = match hos_of(ctx).reset_minutes_left() {
                            Some(left) => format!(
                                "sleep {} more here to finish a 10-hour reset",
                                hos::duration_text(left / 60.0)
                            ),
                            None => "take a full 10-hour reset".to_string(),
                        };
                        format!(
                            "Warning: this sleep did NOT reset your hours, and your duty window \
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
                                "This sleep did NOT reset your hours, but your duty window \
                                 paused while you slept. It closes in {} hours, at {closes}. ",
                                fmt_f(duty_left_h, 1)
                            )
                        } else {
                            format!(
                                "This sleep did NOT reset your hours. Your duty window closes \
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
        ctx.award_achievement("slept_on_route");
        self.refresh(ctx, true);
    }

    fn sleep(&mut self, ctx: &mut GameContext) {
        if self.confirm_sleep(ctx, SleepChoice::Sleeper(10), 0.0) {
            return;
        }
        let before_fatigue = profile_of(ctx).fatigue;
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            let engine_off = shut_down_engine(d, ctx);
            let owed = hos_of(ctx).reset_minutes_left();
            advance_rest_clock(d, ctx, owed.unwrap_or(hos::SLEEP_MIN), None, "");
            hos_mut_of(ctx).sleep();
            {
                let p = profile_mut_of(ctx);
                p.fatigue = hos::rest_sleep(p.fatigue);
            }
            let slept = match owed {
                Some(left) => format!(
                    "You slept {} more, 10 hours of rest in a row, and woke rested.",
                    hos::duration_text(left / 60.0)
                ),
                None => "You slept 10 hours and woke rested.".to_string(),
            };
            format!(
                "{engine_off}{slept} It is {}. Driving and duty clocks reset. {}{}",
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
        ctx.award_achievement("slept_on_route");
        if before_fatigue < hos::FATIGUE_SEVERE {
            ctx.award_achievement("sleep_before_exhaustion");
        }
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
            advance_rest_clock(d, ctx, hos::SLEEP_MIN, None, "");
            hos_mut_of(ctx).sleep();
            profile_mut_of(ctx).fatigue = 0.0;
            let money = profile_of(ctx).money();
            format!(
                "{engine_off}Motel, {} dollars, 10 hours. {}. Driving and duty clocks reset. \
                 {} dollars. {}{}",
                fmt_grouped(MOTEL_COST, 0),
                clock_text(d.trip.local_hour()),
                fmt_grouped(money, 0),
                deadline_text(d, ctx),
                wake_air_instruction(d)
            )
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
    }

    /// Bed down in a break/fuel stop's lot when out of hours: a legal HOS
    /// reset, but cramped poor rest (no proper sleeper), so you wake still
    /// tired. No shoulder fine -- a lot is more legitimate than the freeway.
    fn emergency_lot_sleep(&mut self, ctx: &mut GameContext) {
        if self.confirm_sleep(ctx, SleepChoice::Lot, hos::FATIGUE_SHOULDER_FLOOR) {
            return;
        }
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            let engine_off = shut_down_engine(d, ctx);
            advance_rest_clock(d, ctx, hos::SLEEP_MIN, None, "");
            hos_mut_of(ctx).sleep();
            {
                let p = profile_mut_of(ctx);
                p.fatigue = hos::rest_shoulder(p.fatigue);
            }
            format!(
                "{engine_off}Slept 10 hours in the lot. {}. Driving and duty clocks reset; rest was poor. \
                 {}{}",
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
        ctx.award_achievement("slept_on_route");
    }

    fn repair(&mut self, ctx: &mut GameContext) {
        let damage = self
            .driving
            .read(|d| d.trip.truck.damage_pct)
            .unwrap_or(0.0);
        if damage < 1.0 {
            ctx.say("The truck does not need repair.");
            return;
        }
        let carrier = !player_pays_operating_costs(&profile_of(ctx).business_status);
        if !carrier {
            let cost = ff_core::models::economy::Economy::repair_cost(damage);
            if profile_of(ctx).money() < cost {
                ctx.audio.play("ui/error");
                ctx.say(&format!(
                    "Repair costs {} dollars. You cannot afford it.",
                    fmt_grouped(cost, 0)
                ));
                return;
            }
            profile_mut_of(ctx).spend(cost);
            let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
                d.trip.truck.damage_pct = 0.0;
                advance_rest_clock(d, ctx, 60.0, None, "");
                hos_mut_of(ctx).on_duty(60.0);
                let money = profile_of(ctx).money();
                format!(
                    "Truck repaired for {} dollars. It is {}. You have {} dollars. {}",
                    fmt_grouped(cost, 0),
                    clock_text(d.trip.local_hour()),
                    fmt_grouped(money, 0),
                    deadline_text(d, ctx)
                )
            }) else {
                return;
            };
            self.save_here(ctx, true);
            ctx.audio.play("ui/notify");
            ctx.say(&text);
        } else {
            let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
                d.trip.truck.damage_pct = 0.0;
                advance_rest_clock(d, ctx, 60.0, None, "");
                hos_mut_of(ctx).on_duty(60.0);
                format!(
                    "Shop repaired {} percent damage on the carrier account. It is {}. {}",
                    fmt_f(damage, 0),
                    clock_text(d.trip.local_hour()),
                    deadline_text(d, ctx)
                )
            }) else {
                return;
            };
            self.save_here(ctx, true);
            ctx.audio.play("ui/notify");
            ctx.say(&text);
        }
        ctx.award_achievement("garage_repair");
        if damage >= 75.0 {
            ctx.award_achievement("deep_repair");
        }
    }

    fn roadside_assistance(&mut self, ctx: &mut GameContext) {
        let damage = self
            .driving
            .read(|d| d.trip.truck.damage_pct)
            .unwrap_or(0.0);
        if damage < 1.0 {
            ctx.say("The truck does not need roadside assistance.");
            return;
        }
        // A field patch only brings damage down to its own floor; under it,
        // the call-out bought nothing and the line claimed a repair.
        if damage <= FIELD_REPAIR_DAMAGE_PCT {
            ctx.say(&format!(
                "A roadside patch cannot bring the truck under {} percent damage. A repair \
                 shop can.",
                fmt_f(FIELD_REPAIR_DAMAGE_PCT, 0)
            ));
            return;
        }
        let cost = road_repair_cost(damage, FIELD_REPAIR_DAMAGE_PCT, MECHANIC_CALLOUT_FEE);
        let carrier_paid = !player_pays_operating_costs(&profile_of(ctx).business_status);
        if !carrier_paid {
            let money = profile_of(ctx).money();
            if money < cost {
                ctx.audio.play("ui/error");
                ctx.say(&format!(
                    "Roadside assistance costs {} dollars and you have {} dollars.",
                    fmt_grouped(cost, 0),
                    fmt_grouped(money, 0)
                ));
                return;
            }
            profile_mut_of(ctx).spend(cost);
        }
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            d.trip.truck.damage_pct = damage.min(FIELD_REPAIR_DAMAGE_PCT);
            advance_rest_clock(d, ctx, MECHANIC_WAIT_MIN, None, "");
            hos_mut_of(ctx).on_duty(MECHANIC_WAIT_MIN);
            let billing = if carrier_paid {
                "on the carrier breakdown account".to_string()
            } else {
                format!("for {} dollars", fmt_grouped(cost, 0))
            };
            format!(
                "Roadside assistance patched the truck to {} percent damage {billing}. It is {}. \
                 {}",
                fmt_f(d.trip.truck.damage_pct, 0),
                clock_text(d.trip.local_hour()),
                deadline_text(d, ctx)
            )
        }) else {
            return;
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        ctx.award_achievement("roadside_fix");
    }

    fn tire_bay(&self) -> bool {
        classify_brand(&self.stop.name).is_some_and(|b| b.signature.contains(&"tires"))
    }

    fn tire_rate(&self) -> f64 {
        if self.tire_bay() {
            ROAD_TIRE_SPECIALIST_COST_PER_PCT
        } else {
            ROAD_TIRE_COST_PER_PCT
        }
    }

    fn tire_label(&self, ctx: &GameContext, d: &DrivingState) -> String {
        let wear = d.trip.truck.tire_wear_pct;
        if wear < 1.0 {
            return "Tires: top shape".to_string();
        }
        if !player_pays_operating_costs(&profile_of(ctx).business_status) {
            return format!(
                "Replace tires: {} percent wear, carrier billed",
                fmt_f(wear, 0)
            );
        }
        let cost = round_py_n(wear * self.tire_rate(), 2);
        format!(
            "Replace tires: {} percent wear for {} dollars",
            fmt_f(wear, 0),
            fmt_grouped(cost, 0)
        )
    }

    fn service_wear(&mut self, ctx: &mut GameContext, meter: WearMeter) {
        let (cost_per_pct, minutes, duty_note, fresh_say, service_noun, carrier_done, done_say) =
            match meter {
                WearMeter::Tire => (
                    self.tire_rate(),
                    if self.tire_bay() {
                        ROAD_TIRE_SPECIALIST_MIN
                    } else {
                        ROAD_TIRE_MIN
                    },
                    "road tire service",
                    "The tires are already in top shape.",
                    "tire service",
                    "replaced the tires",
                    "Tires replaced.",
                ),
                WearMeter::Brake => (
                    ROAD_BRAKE_COST_PER_PCT,
                    ROAD_BRAKE_MIN,
                    "road brake service",
                    "The brakes are already in top shape.",
                    "a brake job",
                    "relined the brakes",
                    "Brakes relined.",
                ),
            };
        self.road_wear_service(
            ctx,
            meter,
            cost_per_pct,
            minutes,
            duty_note,
            fresh_say,
            service_noun,
            carrier_done,
            done_say,
        );
    }

    /// Brand-shop wear service on the road, all-or-nothing like road repair.
    ///
    /// Partial service stays a terminal-garage courtesy; a road shop sells
    /// the whole job or none of it.
    #[allow(clippy::too_many_arguments)]
    fn road_wear_service(
        &mut self,
        ctx: &mut GameContext,
        meter: WearMeter,
        cost_per_pct: f64,
        minutes: f64,
        duty_note: &str,
        fresh_say: &str,
        service_noun: &str,
        carrier_done: &str,
        done_say: &str,
    ) {
        let wear = self
            .driving
            .read(|d| match meter {
                WearMeter::Tire => d.trip.truck.tire_wear_pct,
                WearMeter::Brake => d.trip.truck.brake_wear_pct,
            })
            .unwrap_or(0.0);
        if wear < 1.0 {
            ctx.say(fresh_say);
            return;
        }
        let carrier = !player_pays_operating_costs(&profile_of(ctx).business_status);
        let cost = round_py_n(wear * cost_per_pct, 2);
        if !carrier && profile_of(ctx).money() < cost {
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "{} costs {} dollars here. You cannot afford it.",
                py_capitalize(service_noun),
                fmt_grouped(cost, 0)
            ));
            return;
        }
        if !carrier {
            profile_mut_of(ctx).spend(cost);
        }
        let duty_note = duty_note.to_string();
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            match meter {
                WearMeter::Tire => d.trip.truck.tire_wear_pct = 0.0,
                WearMeter::Brake => d.trip.truck.brake_wear_pct = 0.0,
            }
            advance_rest_clock(d, ctx, minutes, Some("on_duty_not_driving"), &duty_note);
            hos_mut_of(ctx).on_duty(minutes);
            if carrier {
                format!(
                    "The shop {carrier_done} at {} percent wear on the carrier account. It is \
                     {}. {}",
                    fmt_f(wear, 0),
                    clock_text(d.trip.local_hour()),
                    deadline_text(d, ctx)
                )
            } else {
                let money = profile_of(ctx).money();
                format!(
                    "{done_say} {} dollars. It is {}. You have {} dollars. {}",
                    fmt_grouped(cost, 0),
                    clock_text(d.trip.local_hour()),
                    fmt_grouped(money, 0),
                    deadline_text(d, ctx)
                )
            }
        }) else {
            return;
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        self.refresh(ctx, true);
    }

    fn buff_price(&self, buff: &Buff) -> f64 {
        if buff.free_with_fuel && (self.visit.fueled || self.visit.free_shower) {
            return 0.0;
        }
        buff.price
    }

    /// Spoken cost for a buff row or purchase line.
    ///
    /// Free showers after fuel keep the fuel wording; catalog-free items (Wall
    /// Drug ice water) just say free. Sub-dollar prices speak in cents so
    /// five-cent coffee is not rounded to "0 dollars".
    fn format_buff_price_amount(price: f64) -> String {
        if price < 1.0 {
            let cents = round_py_int(price * 100.0).max(1);
            if cents == 1 {
                "1 cent".to_string()
            } else {
                format!("{cents} cents")
            }
        } else {
            format!("{} dollars", fmt_grouped(price, 0))
        }
    }

    fn buff_label(&self, buff: &Buff) -> String {
        let price = self.buff_price(buff);
        if price <= 0.0 {
            if buff.free_with_fuel && self.visit.fueled {
                return format!("{}: free with your fuel purchase", buff.label);
            }
            if buff.free_with_fuel {
                return format!("{}: free with your loyalty reward", buff.label);
            }
            return format!("{}: free", buff.label);
        }
        format!("{}: {}", buff.label, Self::format_buff_price_amount(price))
    }

    /// Apply a consumable buff purchase (`data/buffs.rs`).
    ///
    /// Food, drink, and showers are personal money even for company drivers
    /// -- the carrier pays for the truck, not your dinner. Rig care (lube,
    /// tires) is truck work, so the carrier covers it.
    fn buy_buff(&mut self, ctx: &mut GameContext, buff_id: &str) {
        let action_refs: Vec<&str> = self.stop.actions.iter().map(String::as_str).collect();
        let Some(buff) = buffs_for_stop(&self.stop.name, &action_refs)
            .into_iter()
            .find(|b| b.id == buff_id)
        else {
            return;
        };
        let price = self.buff_price(buff);
        let rig_buff = buff.group == "engine" || buff.group == "tire";
        let carrier_pays =
            rig_buff && !player_pays_operating_costs(&profile_of(ctx).business_status);
        if !carrier_pays && profile_of(ctx).money() < price {
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "The {} costs {} and you have {} dollars.",
                buff.label.to_lowercase(),
                Self::format_buff_price_amount(price),
                fmt_grouped(profile_of(ctx).money(), 0)
            ));
            return;
        }
        if !carrier_pays {
            profile_mut_of(ctx).spend(price);
        }
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            if rig_buff {
                d.rig_buffs.insert(
                    buff.group.clone(),
                    RigBuff {
                        id: buff.id.clone(),
                        label: buff.label.clone(),
                        rate: buff.rate,
                    },
                );
                advance_rest_clock(
                    d,
                    ctx,
                    buff.stop_minutes,
                    Some("on_duty_not_driving"),
                    &buff.label.to_lowercase(),
                );
                hos_mut_of(ctx).on_duty(buff.stop_minutes);
            } else {
                if buff.fatigue_instant > 0.0 {
                    let p = profile_mut_of(ctx);
                    p.fatigue = (p.fatigue - buff.fatigue_instant).max(0.0);
                }
                let expires_h = d.absolute_game_hour(ctx, None) + buff.duration_game_h;
                profile_mut_of(ctx).add_timed_buff(json!({
                    "id": buff.id,
                    "label": buff.label,
                    "group": buff.group,
                    "rate": buff.rate,
                    "expires_h": expires_h,
                    "worn_off": buff.worn_off,
                }));
                // Real off-duty time: a 30-minute meal also satisfies the
                // break rule, same as any other break. Never extra hours.
                advance_rest_clock(
                    d,
                    ctx,
                    buff.stop_minutes,
                    Some("off_duty"),
                    &buff.label.to_lowercase(),
                );
                hos_mut_of(ctx).take_break(buff.stop_minutes);
            }
            let from_reward = buff.free_with_fuel && price <= 0.0 && !self.visit.fueled;
            if from_reward {
                // A redeemed shower is one shower.
                d.stop_visit(&self.stop).free_shower = false;
            }
            let billing = if carrier_pays {
                "Billed to the carrier.".to_string()
            } else if price <= 0.0 {
                if from_reward {
                    "Free with your loyalty reward.".to_string()
                } else if buff.free_with_fuel {
                    "Free with your fuel purchase.".to_string()
                } else {
                    "Free.".to_string()
                }
            } else {
                format!(
                    "{}. You have {} dollars.",
                    Self::format_buff_price_amount(price),
                    fmt_grouped(profile_of(ctx).money(), 0)
                )
            };
            format!(
                "{} {billing} It is {}. {}",
                buff.purchased,
                clock_text(d.trip.local_hour()),
                deadline_text(d, ctx)
            )
        }) else {
            return;
        };
        self.save_here(ctx, true);
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        self.refresh(ctx, true);
    }

    fn inspect(&mut self, ctx: &mut GameContext) {
        // Nobody is in a closed scale house: the row says so and nothing
        // else happens. It used to run the check-in all the same -- marking
        // the stop inspected, playing the notify tone and saving -- for a
        // scale that settles nothing (QA, 2026-10-07). No time, no record.
        let open = self
            .driving
            .read(|d| d.scale_is_open(&self.stop))
            .unwrap_or(false);
        if !open {
            ctx.say(&format!(
                "{} is closed. Pull back onto the highway.",
                self.stop.spoken_name()
            ));
            return;
        }
        let Some((text, waved)) = self.check_in(ctx) else {
            return;
        };
        self.refresh(ctx, true);
        ctx.say(&text);
        ctx.say_with(self.current_text(ctx), Say::queued().review(false));
        if waved {
            record_inspection(ctx);
        }
    }

    /// Whether this is an open scale the driver has not checked in at yet.
    fn check_in_pending(&self) -> bool {
        self.stop.actions.iter().any(|a| a == "inspect")
            && !self.visit.inspected
            && self
                .driving
                .read(|d| d.scale_is_open(&self.stop))
                .unwrap_or(false)
    }

    /// The scale check-in itself, settled and saved: the spoken result, and
    /// whether the lane waved the truck through.
    ///
    /// Open scales only: `inspect` answers a closed one before this runs,
    /// and Back runs it only while `check_in_pending`, which is open-only.
    fn check_in(&mut self, ctx: &mut GameContext) -> Option<(String, bool)> {
        let stop = self.stop.clone();
        let result = self.driving.clone().with(ctx, |d, ctx| {
            d.stop_visit(&stop).inspected = true;
            ctx.audio.play("ui/notify");
            // A valid decal is waved through on sight (CVSA Operational
            // Policy 5), unless the record is targeted.
            if d.decal_waves_through(ctx) {
                advance_rest_clock(d, ctx, WAVE_THROUGH_MIN, None, "");
                hos_mut_of(ctx).on_duty(WAVE_THROUGH_MIN);
                return (
                    format!(
                        "Inspection check-in complete at {}. The inspection decal on the \
                         windshield gets you waved straight back onto the highway. It is {}. {}",
                        stop.spoken_name(),
                        clock_text(d.trip.local_hour()),
                        deadline_text(d, ctx)
                    ),
                    true,
                );
            }
            // Whether the screening lane waves you through or pulls you in is
            // the safety record's job. A clean career is waved through nearly
            // every time; a career carrying citations, out-of-service history
            // and a beaten-up truck is pulled in at every open scale.
            if !d.scale_selects_driver(ctx, &stop) {
                advance_rest_clock(d, ctx, WAVE_THROUGH_MIN, None, "");
                hos_mut_of(ctx).on_duty(WAVE_THROUGH_MIN);
                return (
                    format!(
                        "Inspection check-in complete at {}. Officers wave you straight back \
                         onto the highway. {} It is {}. {}",
                        stop.spoken_name(),
                        d.safety_record_line(ctx),
                        clock_text(d.trip.local_hour()),
                        deadline_text(d, ctx)
                    ),
                    true,
                );
            }
            // The lane is a real Level 1: driver, paperwork, walk-around and
            // under the truck, settled where it stands.
            let report = d.inspection_report(ctx, InspectionLevel::Full);
            let outcome = d.settle_inspection(ctx, &report);
            (
                format!(
                    "Inspection check-in complete at {}. Officers pull you into the inspection \
                     lane. {outcome} {} It is {}. {}",
                    stop.spoken_name(),
                    d.safety_record_line(ctx),
                    clock_text(d.trip.local_hour()),
                    deadline_text(d, ctx)
                ),
                false,
            )
        })?;
        // The fine, the citation and the done check-in are on disk before a
        // quit could take them back.
        self.save_here(ctx, true);
        Some(result)
    }

    /// The driver's own pre-trip: what an inspector would find on the
    /// tractor and the hooked trailer, before an inspector does.
    fn walk_around(&mut self, ctx: &mut GameContext) {
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            let lines = d.walk_around_lines(ctx);
            advance_rest_clock(
                d,
                ctx,
                WALK_AROUND_MIN,
                Some("on_duty_not_driving"),
                "pre-trip walk-around",
            );
            hos_mut_of(ctx).on_duty(WALK_AROUND_MIN);
            let body = if lines.is_empty() {
                "Nothing to write up: tires, brakes, lights and the trailer would all pass."
                    .to_string()
            } else {
                lines.join(" ")
            };
            format!(
                "Walk-around done, {} minutes. {body} It is {}. {}",
                fmt_f(WALK_AROUND_MIN, 0),
                clock_text(d.trip.local_hour()),
                deadline_text(d, ctx)
            )
        }) else {
            return;
        };
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        ctx.say_with(self.current_text(ctx), Say::queued().review(false));
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
        7 => "Pairs with 3 more hours at sleep-capable parking.",
        _ => "Pairs with 2 more hours at sleep-capable parking.",
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

impl Menu for RestStopState {
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
