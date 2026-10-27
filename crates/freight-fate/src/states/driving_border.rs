use ff_core::achievements::{increment_stat, int_stat};
use ff_core::models::enforcement::{
    BORDER_CA_FIRST_FINE_USD, BORDER_CA_SECOND_FINE_USD, BORDER_CA_THIRD_FINE_USD,
    BORDER_US_FIRST_FINE, BORDER_US_REPEAT_FINE,
};
use ff_core::pyfmt::fmt_grouped;
use ff_core::pyrandom::PyRandom;
use ff_core::sim::hos;
use ff_core::sim::trip_models::BorderBooth;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::impl_state_for_menu;
use crate::speech::EventPriority;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{
    advance_rest_clock, hos_mut_of, profile_mut_of, BORDER_PRIMARY_MIN, BORDER_SECONDARY_EXTRA_MIN,
    BORDER_SECONDARY_REFERRAL_CHANCE, WEIGH_STATION_BYPASS_MPH,
};
use crate::states::driving_menu_states::{push_over_drive, DriveRef};
use crate::states::driving_updates::live;

const US_BORDER_STAT: &str = "border_reports_missed_us";
const CA_BORDER_STAT: &str = "border_reports_missed_ca";

impl DrivingState {
    pub fn check_border_approach(&mut self, ctx: &mut GameContext) {
        let position = self.trip.position_mi;
        let lookahead = self.scale_notice_lookahead_mi(ctx).max(2.0);
        let booths = self.trip.border_booths.clone();

        for booth in booths {
            let ahead = booth.at_mi - position;
            if ahead <= 0.0 {
                continue;
            }
            let notice_key = format!("border-notice:{}", booth.key);
            if ahead <= lookahead && !self.enforcement_events.contains(&notice_key) {
                self.enforcement_events.insert(notice_key);
                self.refresh_live_facts();
                ctx.audio
                    .play_with("events/weigh_station_warning", 0.7, 0.0);
                let text = format!(
                    "{} ahead in {}, {}. Every truck stops at the customs booth. Come to a full \
                     stop before it.",
                    booth.name,
                    ctx.settings.short_distance_text(ahead),
                    booth.agency
                );
                let at_mi = booth.at_mi;
                ctx.say_event_with(
                    text,
                    SayEvent::queued()
                        .priority(EventPriority::Important)
                        .category(SpeechCategory::Navigation)
                        .valid(move || live::position_mi() < at_mi),
                );
            }

            let reminder_key = format!("border-reminder:{}", booth.key);
            if ahead <= 0.5 && !self.enforcement_events.contains(&reminder_key) {
                self.enforcement_events.insert(reminder_key);
                self.refresh_live_facts();
                let at_mi = booth.at_mi;
                ctx.say_event_with(
                    "Customs booth in half a mile. Stop at the booth.",
                    SayEvent::queued()
                        .priority(EventPriority::Important)
                        .category(SpeechCategory::Navigation)
                        .valid(move || live::position_mi() < at_mi),
                );
            }
        }
    }

    pub fn check_border_booth_crossing(&mut self, ctx: &mut GameContext, previous_mi: f64) -> bool {
        let position = self.trip.position_mi;
        let booth = self
            .trip
            .border_booths
            .iter()
            .find(|booth| {
                previous_mi < booth.at_mi
                    && booth.at_mi <= position
                    && !self
                        .enforcement_events
                        .contains(&format!("border-booth:{}", booth.key))
            })
            .cloned();
        let Some(booth) = booth else {
            return false;
        };

        self.enforcement_events
            .insert(format!("border-booth:{}", booth.key));
        let running = self.trip.truck.speed_mph() > WEIGH_STATION_BYPASS_MPH;
        if !running {
            self.stop_for_border(ctx);
            self.open_border_clearance(ctx, booth);
            return true;
        }

        if self.enforcement_bypassed(ctx) {
            if self.pull_over.is_some() {
                self.pending_border_clearance = Some(booth);
            } else {
                self.stop_for_border(ctx);
                self.open_border_clearance(ctx, booth);
            }
            return true;
        }

        if self.pull_over.is_some() {
            self.pending_border_clearance = Some(booth);
            return true;
        }

        // AMPS retains C023 records for 12 months; the game simplifies that
        // to lifetime per-profile counts for each country.
        let (fine, penalty) = border_penalty_and_record(ctx, &booth);
        self.pending_border_clearance = Some(booth.clone());
        let summary = format!(
            "At {}, {} stopped the truck for running the customs booth: {}.",
            booth.name, booth.agency, penalty
        );
        let lights_message = format!(
            "Customs officers are following. Signal with {} and stop on the shoulder.",
            ctx.control_hint("take_exit")
        );
        self.begin_enforcement_pull_over(
            ctx,
            "border_port_running",
            "Ran the port of entry",
            &summary,
            fine,
            hos::HOS_REPUTATION_HIT,
            "Officers walk you back to the customs booth.",
            &lights_message,
        );
        true
    }

    fn stop_for_border(&mut self, ctx: &mut GameContext) {
        self.pause_speed_control(ctx, false);
        self.trip.truck.velocity_mps = 0.0;
        self.trip.truck.throttle = 0.0;
        self.trip.truck.brake = 1.0;
        self.trip.truck.set_parking_brake();
        self.settle_engine_to_idle(ctx);
    }

    pub fn open_border_clearance(&mut self, ctx: &mut GameContext, booth: BorderBooth) {
        self.pending_border_clearance = None;
        let mut state = BorderClearanceState::new(ctx, booth, self.trip_seed);
        state.enter(ctx);
        push_over_drive(ctx, state);
    }
}

fn border_penalty_and_record(ctx: &mut GameContext, booth: &BorderBooth) -> (f64, String) {
    match booth.entering_country.as_str() {
        "US" => {
            let profile = profile_mut_of(ctx);
            let prior = int_stat(profile, US_BORDER_STAT);
            let (fine, penalty) = if prior == 0 {
                (
                    BORDER_US_FIRST_FINE,
                    format!(
                        "a {} US dollar civil penalty",
                        fmt_grouped(BORDER_US_FIRST_FINE, 0)
                    ),
                )
            } else {
                (
                    BORDER_US_REPEAT_FINE,
                    format!(
                        "a {} US dollar civil penalty",
                        fmt_grouped(BORDER_US_REPEAT_FINE, 0)
                    ),
                )
            };
            increment_stat(profile, US_BORDER_STAT);
            (fine, penalty)
        }
        "CA" => {
            let profile = profile_mut_of(ctx);
            let prior = int_stat(profile, CA_BORDER_STAT);
            let (fine, cad) = match prior {
                0 => (BORDER_CA_FIRST_FINE_USD, 2_000.0),
                1 => (BORDER_CA_SECOND_FINE_USD, 4_000.0),
                _ => (BORDER_CA_THIRD_FINE_USD, 8_000.0),
            };
            let penalty = format!(
                "a {} Canadian dollar penalty, charged as {} US dollars",
                fmt_grouped(cad, 0),
                fmt_grouped(fine, 0)
            );
            increment_stat(profile, CA_BORDER_STAT);
            (fine, penalty)
        }
        _ => (0.0, String::new()),
    }
}

pub fn border_secondary_referral(trip_seed: i64, key: &str) -> bool {
    let mut rng = PyRandom::new_from_str(&format!("{}:border-secondary:{key}", trip_seed));
    rng.random() < BORDER_SECONDARY_REFERRAL_CHANCE
}

pub struct BorderClearanceState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    booth: BorderBooth,
    trip_seed: i64,
}

impl BorderClearanceState {
    pub fn new(ctx: &GameContext, booth: BorderBooth, trip_seed: i64) -> Self {
        Self {
            menu: MenuCore::new(&booth.name),
            driving: DriveRef::active(ctx),
            booth,
            trip_seed,
        }
    }

    fn opening_line(&self) -> String {
        format!(
            "{}, {}. The officer asks for your passport and checks the eManifest your carrier \
             filed before arrival.",
            self.booth.name, self.booth.agency
        )
    }

    fn answer_officer(&mut self, ctx: &mut GameContext) {
        let referred = border_secondary_referral(self.trip_seed, &self.booth.key);
        let extra_minutes = if referred {
            BORDER_SECONDARY_EXTRA_MIN
        } else {
            0.0
        };
        let minutes = BORDER_PRIMARY_MIN + extra_minutes;
        let drive = self.driving.clone().with(ctx, |driving, ctx| {
            advance_rest_clock(
                driving,
                ctx,
                minutes,
                Some("on_duty_not_driving"),
                "customs clearance",
            );
            hos_mut_of(ctx).on_duty(minutes);
            driving.trip.truck.velocity_mps = 0.0;
            driving.trip.truck.throttle = 0.0;
            driving.trip.truck.brake = 1.0;
            driving.trip.truck.set_parking_brake();
            driving.settle_engine_to_idle(ctx);
            if ctx.profile.is_some() {
                let snapshot = driving.snapshot(ctx);
                profile_mut_of(ctx).active_trip = Some(snapshot);
                ctx.save_profile();
            }
        });
        if drive.is_none() {
            return;
        }

        let country = if self.booth.entering_country == "CA" {
            "Canada"
        } else {
            "the United States"
        };
        let result = if referred {
            format!(
                "Referred to secondary inspection. Officers examine the trailer for 45 minutes, \
                 then clear you. Welcome to {country}."
            )
        } else {
            format!("Cleared. Welcome to {country}.")
        };
        ctx.pop_state();
        ctx.say(&result);
    }
}

impl Menu for BorderClearanceState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![MenuItem::new(
            "Answer the officer's questions and hand over your documents",
            |state: &mut Self, ctx| state.answer_officer(ctx),
        )]
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        ctx.say(&self.opening_line());
        let action = self.current_text(ctx);
        ctx.say(&action);
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        ctx.say("Customs has to clear you before you drive on.");
    }
}

impl_state_for_menu!(BorderClearanceState);
