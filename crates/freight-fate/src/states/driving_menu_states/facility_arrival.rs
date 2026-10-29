//! The destination facility menu (`FacilityArrivalState`).
//!
//! Python composed this out of `FacilityEngineMixin` and `MenuState`. Here
//! it is one struct that implements
//! [`crate::states::driving_core::FacilityEngine`], whose default methods
//! ARE the mixin: the same one row that changes face, worded the same way,
//! shared with the pickup side.

use ff_core::models::business::{build_business_settlement, SettlementTerms};
use ff_core::models::trailer_yard::{delivery_plan, DeliveryPlan};
use ff_core::music::{select_menu_music_sequence, MenuMusicProfile};
use ff_core::pyfmt::{fmt_f, fmt_grouped, round_py_n};
use ff_core::sim::vehicle::TruckState;

use crate::app::GameContext;
use crate::audio::facility_ambient_key;
use crate::discord_presence::PresenceState;
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem, TimedMessageState};
use crate::states::city_pickup::sync_facility_engine_audio;
use crate::states::driving::DrivingState;
use crate::states::driving_core::{
    advance_rest_clock, carrier_accessorial_charges, charge_summary, charge_total,
    has_weigh_station_transponder, hos_mut_of, is_owner_operator, profile_of, wallet_delta,
    FacilityEngine, DOCKING_MAX_MPH, UNLOADING_WAIT_S,
};
use crate::states::driving_menu_states::{keep_rows, settlement_hours, DriveRef};

pub struct FacilityArrivalState {
    menu: MenuCore<Self>,
    driving: DriveRef,
}

const FACILITY_INTRO_HELP: &str =
    "Arrows navigate, Enter selects. Dock and deliver completes the job.";

impl FacilityArrivalState {
    pub fn new(ctx: &GameContext) -> Self {
        FacilityArrivalState {
            menu: MenuCore::new("Destination facility")
                .with_open_sound(Some("facility/dock_gate"))
                .with_intro_help(FACILITY_INTRO_HELP),
            driving: DriveRef::active(ctx),
        }
    }

    /// The same screen over a drive the caller already shares (tests).
    pub fn with_drive(driving: DriveRef) -> Self {
        FacilityArrivalState {
            menu: MenuCore::new("Destination facility")
                .with_open_sound(Some("facility/dock_gate"))
                .with_intro_help(FACILITY_INTRO_HELP),
            driving,
        }
    }

    /// `enter()` run while the drive is still in hand -- see `drive_ref`.
    pub fn enter_over_drive(&mut self, ctx: &mut GameContext, driving: &mut DrivingState) {
        let sequence =
            select_menu_music_sequence(ctx.profile.as_ref().map(|p| p as &dyn MenuMusicProfile));
        let refs: Vec<&str> = sequence.iter().map(String::as_str).collect();
        ctx.play_music_sequence("menu", &refs);
        sync_facility_engine_audio(ctx, &driving.trip.truck);
        let items = self.rows(ctx, driving);
        self.menu.items = items;
        self.menu.index = self.menu.index.min(self.menu.items.len().saturating_sub(1));
        if let Some(key) = self.menu.open_sound_key.clone() {
            ctx.audio.play(&key);
        }
        self.announce_over_drive(ctx, driving);
    }

    fn announce_over_drive(&mut self, ctx: &mut GameContext, driving: &mut DrivingState) {
        ctx.audio
            .set_ambient(Some(facility_ambient_key(&driving.job.destination_type)));
        let facility = driving.destination_facility_text(ctx);
        let current = self.current_text(ctx);
        ctx.say(&format!("At {facility}. {current}"));
    }

    pub fn facility(&self, ctx: &mut GameContext) -> String {
        self.driving
            .with(ctx, |d, ctx| d.destination_facility_text(ctx))
            .unwrap_or_default()
    }

    /// How the freight comes off here: dropped in the yard, or a live dock.
    ///
    /// Recomputed from the job rather than stored, exactly like the pickup
    /// side, so it survives a save and never disagrees with itself.
    fn delivery_plan(&self, ctx: &mut GameContext) -> DeliveryPlan {
        self.driving
            .with(ctx, |d, ctx| delivery_plan(&d.job, profile_of(ctx)))
            .unwrap_or(DeliveryPlan {
                mode: "live",
                minutes: 0.0,
                keeps_trailer: true,
                reason: "",
            })
    }

    /// The action that ends this delivery, named the way the menu names it.
    fn finish_instruction(&self, ctx: &mut GameContext) -> &'static str {
        if self.delivery_plan(ctx).is_drop_hook() {
            "Drop the loaded trailer"
        } else {
            "Dock and deliver"
        }
    }

    fn rows(&mut self, ctx: &mut GameContext, driving: &mut DrivingState) -> Vec<MenuItem<Self>> {
        let plan = delivery_plan(&driving.job, profile_of(ctx));
        let primary = if plan.is_drop_hook() {
            MenuItem::new(
                "Drop the loaded trailer and hook an empty",
                |s: &mut Self, ctx| s.dock(ctx),
            )
            .help(
                "The receiver takes the whole trailer. Quicker than a dock, and a write-up \
                 leaves with the trailer.",
            )
        } else {
            MenuItem::new("Dock and deliver", |s: &mut Self, ctx| s.dock(ctx))
                .help("Completes this delivery.")
        };
        vec![
            primary,
            self.facility_engine_item_for(driving.trip.truck.engine_on),
            MenuItem::new("Check paperwork", |s: &mut Self, ctx| s.paperwork(ctx))
                .help("Review pay, deadline, cargo condition, and charges."),
            MenuItem::new("Check arrival status", |s: &mut Self, ctx| s.status(ctx))
                .help("Hear the facility, cargo, speed, and next step."),
        ]
    }

    /// [`FacilityEngine::facility_engine_item`] with the engine state passed
    /// in, for the one build that happens while the drive is still in hand.
    fn facility_engine_item_for(&self, engine_on: bool) -> MenuItem<Self> {
        if engine_on {
            MenuItem::new(
                crate::states::driving_core::FACILITY_ENGINE_SHUT_DOWN_ITEM,
                |s: &mut Self, ctx| s.toggle_facility_engine(ctx),
            )
            .help("No fuel burned and no idle noise while parked.")
        } else {
            MenuItem::new(
                crate::states::driving_core::FACILITY_ENGINE_START_ITEM,
                |s: &mut Self, ctx| s.toggle_facility_engine(ctx),
            )
            .help("Starts the engine. The parking brake needs 100 psi of air.")
        }
    }

    fn dock(&mut self, ctx: &mut GameContext) {
        let plan = self.delivery_plan(ctx);
        let facility = self.facility(ctx);
        // The delivery appointment is met when the receiver accepts the
        // truck at the dock, not when the receiver finishes unloading it.
        // Keep that instant before the live-unload clock advances below.
        let Some(arrival_hours) = self.driving.read(|d| settlement_hours(d)) else {
            return;
        };
        let Some(speed) = self.driving.read(|d| d.trip.truck.speed_mph()) else {
            return;
        };
        if speed > DOCKING_MAX_MPH {
            ctx.audio.play("ui/error");
            ctx.say("Stop before docking.");
            return;
        }
        let (weight_tons, cargo_label) = self
            .driving
            .read(|d| (d.job.weight_tons, d.job.cargo.label.to_string()))
            .unwrap_or_default();
        self.driving.read(|d| {
            d.trip.truck.throttle = 0.0;
            d.trip.truck.brake = 1.0;
            d.trip.truck.set_parking_brake();
            d.set_status(if plan.is_drop_hook() {
                "In the yard. Dropping the trailer."
            } else {
                "Docked. Unloading cargo."
            });
        });

        let drive = self.driving.clone();
        let drop_hook = plan.is_drop_hook();
        let minutes = plan.minutes;
        let defect = self.hooked_defect(ctx);
        let complete = move |ctx: &mut GameContext| {
            drive.with(ctx, |d, ctx| {
                // The rest clock burns the dock wait's idle fuel. The
                // settlement already reports the tank, so this one is felt
                // rather than announced.
                advance_rest_clock(d, ctx, minutes, None, "");
                hos_mut_of(ctx).on_duty(minutes);
                d.set_status(if drop_hook {
                    "Trailer dropped. Hooked to an empty, paperwork signed."
                } else {
                    "Unloaded. Delivery paperwork signed."
                });
            });
            if drop_hook {
                ctx.award_achievement("first_delivery_drop");
                if defect {
                    // The write-up leaves with the trailer, which is the only
                    // honest way to be rid of one.
                    ctx.award_achievement("dropped_the_bad_one");
                }
            }
            drive.with(ctx, |d, ctx| {
                d.replace_with_arrival_state_at(ctx, arrival_hours)
            });
        };

        let (title, message, status) = if drop_hook {
            (
                "Dropping the trailer",
                format!(
                    "Dropping the loaded trailer at {facility}, {} tons of {cargo_label}. \
                     Hooking an empty.",
                    fmt_f(weight_tons, 0)
                ),
                "Dropping the trailer.",
            )
        } else {
            (
                "Unloading cargo",
                format!(
                    "Docked at {facility}. Unloading {} tons of {cargo_label}.",
                    fmt_f(weight_tons, 0)
                ),
                "Unloading cargo.",
            )
        };
        ctx.replace_state(
            TimedMessageState::new(title, &message, status, UNLOADING_WAIT_S, complete)
                .sound_key(Some("poi/dock_and_deliver")),
        );
    }

    /// Whether the loaded trailer dropped here still carries its write-up.
    ///
    /// Asks the drive, which knows a box refused at the shipper or repaired
    /// on the road is no longer the bad one. Reading the origin yard's plan
    /// directly handed the badge for leaving a bad trailer behind to a driver
    /// who had refused it at pickup and hauled a sound one.
    fn hooked_defect(&self, ctx: &mut GameContext) -> bool {
        self.driving
            .with(ctx, |d, ctx| {
                d.hooked_trailer_defect(ctx)
                    .is_some_and(|defect| !defect.is_empty())
            })
            .unwrap_or(false)
    }

    fn paperwork(&mut self, ctx: &mut GameContext) {
        let facility = self.facility(ctx);
        let finish = self.finish_instruction(ctx);
        let Some(text) = self.driving.with(ctx, |d, ctx| {
            let job = &d.job;
            let hours = d.trip.game_minutes / 60.0;
            let remaining = job.deadline_game_h - hours;
            let trip_damage = (d.trip.truck.damage_pct - d.start_damage).max(0.0);
            let estimated_pay = job.payout_default(hours, trip_damage);
            let tolls = d.trip.toll_expense();
            let profile = profile_of(ctx);
            let accessorials = carrier_accessorial_charges(job, Some(profile));
            let carrier_charges = tolls + charge_total(&accessorials);
            // Only what a previous settlement could not collect. Speeding
            // itself is charged on the shoulder by the trooper who saw it, or
            // not at all.
            let driver_charges = profile.fines_owed;
            let owner_op = is_owner_operator(&profile.business_status);
            // A company driver's settlement pays wages, not the carrier's
            // gross: the board quoted 224 dollars for a load this line then
            // called 330 of "net driver pay" (agent playtest, 2026-09-02).
            // The same builder the board's estimate and the settlement use.
            let driver_gross = if owner_op {
                estimated_pay
            } else {
                let owned: Vec<String> = profile.visible_owned_trailers();
                let owned_refs: Vec<&str> = owned.iter().map(String::as_str).collect();
                build_business_settlement(
                    &profile.business_status,
                    job,
                    estimated_pay,
                    remaining >= 0.0,
                    0.0,
                    &SettlementTerms {
                        carrier_key: Some(&profile.carrier_key),
                        owned_trailers: &owned_refs,
                        reputation: Some(profile.standing()),
                        transponder: has_weigh_station_transponder(profile),
                        record_surcharge: ff_core::models::enforcement::record_insurance_surcharge(
                            profile,
                        ),
                    },
                )
                .net_before_advance
            };
            let mut net_estimated_pay = (driver_gross - driver_charges).max(0.0);
            if owner_op {
                net_estimated_pay =
                    (net_estimated_pay + wallet_delta(&accessorials, tolls)).max(0.0);
            }
            let advance_due = round_py_n(profile.pay_advance.min(net_estimated_pay), 2);
            net_estimated_pay = round_py_n(net_estimated_pay - advance_due, 2);
            let advance_note = if advance_due > 0.0 {
                format!(
                    " Pay advance of {} dollars repaid from this settlement.",
                    fmt_grouped(advance_due, 0)
                )
            } else {
                String::new()
            };
            let timing = if remaining >= 0.0 {
                format!("{} hours to the deadline", fmt_f(remaining, 1))
            } else {
                format!("{} hours past the deadline", fmt_f(-remaining, 1))
            };
            let cargo_condition = if trip_damage > 1.0 {
                format!(
                    "Truck damage this run {} percent, may reduce pay.",
                    fmt_f(trip_damage, 0)
                )
            } else {
                "Cargo condition: no new damage recorded.".to_string()
            };
            let charge_fate = if owner_op {
                "Those charges come off this settlement."
            } else {
                "Those charges do not reduce driver pay."
            };
            format!(
                "Paperwork, {facility}: {} tons of {}. Rate {}, gross {}. Charges {}: tolls {}, \
                 accessorials {}. {charge_fate} Fines {}. Net about {}.{advance_note} {timing}. \
                 {cargo_condition} {finish} to settle.",
                fmt_f(job.weight_tons, 0),
                job.cargo.label,
                fmt_grouped(job.pay, 0),
                fmt_grouped(estimated_pay, 0),
                fmt_grouped(carrier_charges, 0),
                fmt_grouped(tolls, 0),
                charge_summary(&accessorials),
                fmt_grouped(driver_charges, 0),
                fmt_grouped(net_estimated_pay, 0),
            )
        }) else {
            return;
        };
        ctx.say(&text);
    }

    fn status(&mut self, ctx: &mut GameContext) {
        let facility = self.facility(ctx);
        let finish = self.finish_instruction(ctx);
        let Some(text) = self.driving.with(ctx, |d, ctx| {
            format!(
                "At {facility}. {} tons of {}. Speed {}. {}. Stop, then {finish}.",
                fmt_f(d.job.weight_tons, 0),
                d.job.cargo.label,
                ctx.settings.speed_text(d.trip.truck.speed_mph()),
                if d.trip.truck.engine_on {
                    "Engine running"
                } else {
                    "Engine off"
                }
            )
        }) else {
            return;
        };
        ctx.say(&text);
    }
}

impl FacilityEngine for FacilityArrivalState {
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
        // The drive under this menu owns the truck, and this screen holds
        // the only handle on it (it REPLACED the drive on the stack), so the
        // borrow is always available here.
        self.driving
            .clone()
            .call(self, ctx, |_s, ctx, d| f(ctx, &mut d.trip.truck))
            .expect("the destination facility menu keeps the drive it replaced")
    }
}

impl Menu for FacilityArrivalState {
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
        let sequence =
            select_menu_music_sequence(ctx.profile.as_ref().map(|p| p as &dyn MenuMusicProfile));
        let refs: Vec<&str> = sequence.iter().map(String::as_str).collect();
        ctx.play_music_sequence("menu", &refs);
        self.driving
            .read(|d| sync_facility_engine_audio(ctx, &d.trip.truck));
        let items = self.build_items(ctx);
        self.menu.items = items;
        self.menu.index = self.menu.index.min(self.menu.items.len().saturating_sub(1));
        if let Some(key) = self.menu.open_sound_key.clone() {
            ctx.audio.play(&key);
        }
        self.announce_entry(ctx);
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let destination_type = self
            .driving
            .read(|d| d.job.destination_type.clone())
            .unwrap_or_default();
        ctx.audio
            .set_ambient(Some(facility_ambient_key(&destination_type)));
        let facility = self.facility(ctx);
        let current = self.current_text(ctx);
        ctx.say(&format!("At {facility}. {current}"));
    }

    fn presence(&self, _ctx: &GameContext) -> Option<PresenceState> {
        let (label, destination) = self
            .driving
            .read(|d| {
                (
                    d.job.cargo.label.to_string(),
                    d.job.spoken_destination().to_string(),
                )
            })
            .unwrap_or_default();
        Some(PresenceState::new(
            "Delivering",
            &format!("{label} to {destination}"),
        ))
    }

    fn online_presence(&self, ctx: &GameContext) -> Option<PresenceState> {
        self.presence(ctx)
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        let finish = self.finish_instruction(ctx);
        ctx.say(&format!("At destination. {finish} to finish."));
    }

    fn lines(&self, ctx: &GameContext) -> Vec<String> {
        let facility = self
            .driving
            .read(|d| d.destination_facility_text(ctx))
            .unwrap_or_default();
        let (speed, engine_on) = self
            .driving
            .read(|d| (d.trip.truck.speed_mph(), d.trip.truck.engine_on))
            .unwrap_or((0.0, false));
        let mut out = vec![
            self.menu.title.clone(),
            String::new(),
            format!("Facility: {facility}"),
            format!("Speed: {}", ctx.settings.hud_speed_text(speed)),
            format!("Engine: {}", if engine_on { "running" } else { "off" }),
            "Stopping required before delivery settlement.".to_string(),
            String::new(),
        ];
        for (i, item) in self.menu.items.iter().enumerate() {
            let marker = if i == self.menu.index { "> " } else { "  " };
            out.push(format!("{marker}{}", item.text(self, ctx)));
        }
        out
    }
}

impl_state_for_menu!(FacilityArrivalState);
