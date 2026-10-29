//! The roadside outcomes: a speeding stop, a non-speeding enforcement stop,
//! a CDL pulled at speed with no stop, and the felony stop that ends a run.
//!
//! [`RoadsideExit`] is Python's `_RoadsideExitMixin`: a trait with provided
//! methods, no state of its own, shared by the first three screens.

use ff_core::models::enforcement;
use ff_core::pyfmt::{fmt_f, fmt_grouped};
use ff_core::pyrandom::PyRandom;
use ff_core::sim::hos;
use ff_core::sim::roadside_inspection::InspectionLevel;

use crate::app::{GameContext, Say};
use crate::discord_presence::PresenceState;
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::city::CityMenuState;
use crate::states::driving::DrivingState;
use crate::states::driving_core::{
    advance_rest_clock, career_citations, citation_fine, clock_text, construction_zone_fine_clause,
    hos_mut_of, hos_of, profile_mut_of, profile_of, shut_down_engine, wake_air_instruction,
    DRIVE_PHASE_DELIVERY, FAILURE_TO_STOP_DAMAGE_PCT, FAILURE_TO_STOP_PROCESSING_MIN,
    INSPECTION_MIN, PULL_OVER_CLEAN_STOP_WARN_CHANCE,
};
use crate::states::driving_menu_states::DriveRef;
use crate::states::driving_updates::pending::EnforcementStopParams;

/// How a roadside stop lets the player go -- which depends on whether they
/// may still legally drive.
///
/// An ordinary ticket ends with merging back up to speed. A stop that just
/// pulled the license cannot: the driver is not allowed to move the truck, so
/// offering the highway would be the game inviting them to break the rule it
/// just enforced, and there would be no way off the shoulder at all. In that
/// case the run ends here the way the felony stop already ends -- the load
/// goes back to dispatch and the driver is released to the terminal, where
/// "Wait out the CDL suspension" is waiting for them.
pub trait RoadsideExit: Menu {
    fn drive(&self) -> &DriveRef;

    fn licence_pulled(&self, ctx: &GameContext) -> bool {
        let Some(profile) = ctx.profile.as_ref() else {
            return false;
        };
        profile.driving_record.lifetime_disqualified
            || profile.driving_record.suspended(profile.game_hours)
    }

    fn roadside_exit_item(&self, ctx: &GameContext, highway_help: &str) -> MenuItem<Self> {
        if self.licence_pulled(ctx) {
            return MenuItem::new("Return to terminal", |s: &mut Self, ctx| {
                s.end_run_suspended(ctx)
            })
            .help("License pulled, so the truck stays put. Dispatch takes the load back.");
        }
        MenuItem::new("Pull back onto the highway", |s: &mut Self, ctx| {
            s.go_back(ctx)
        })
        .help(highway_help)
    }

    /// Said as part of the outcome: why the run stops and what happens next.
    ///
    /// Takes the drive rather than reaching for it through [`RoadsideExit::drive`]:
    /// the only caller is `resolve`, which runs in the constructor while the
    /// push helper is still holding the drive, so a second borrow was always
    /// going to fail. It did, silently, and every suspended stop on a loaded
    /// run told the driver "There is no loaded trailer to hand back" with a
    /// full trailer behind them.
    fn suspended_exit_text(&self, ctx: &GameContext, d: &DrivingState) -> String {
        let profile = profile_of(ctx);
        let load = if d.phase == DRIVE_PHASE_DELIVERY && !d.job.bobtail {
            format!(
                "Dispatch takes the {} load back and reassigns it",
                d.job.cargo.label
            )
        } else {
            "There is no loaded trailer to hand back, and the assignment is canceled".to_string()
        };
        let terminal = ctx
            .world
            .home_terminal(&profile.current_city)
            .map(|t| t.spoken_name())
            .unwrap_or_else(|_| "the terminal".to_string());
        if profile.driving_record.lifetime_disqualified {
            return format!(
                " The license is gone for good, so the truck stays here. {load}, and a relief \
                 driver takes the truck in. You are released to {terminal}."
            );
        }
        format!(
            " The license is pulled as of now, so the truck stays here. {load}, and a relief \
             driver takes the truck in. You are released to {terminal} to wait the suspension \
             out."
        )
    }

    /// Escape never drives off on a pulled license: it closes out the run
    /// the way the exit row does. Escape used to pop back onto the highway
    /// and the drive carried on with the CDL suspended. Returns whether it
    /// closed the run out.
    fn end_run_if_licence_pulled(&mut self, ctx: &mut GameContext) -> bool {
        if !self.licence_pulled(ctx) {
            return false;
        }
        self.end_run_suspended(ctx);
        true
    }

    /// Close out the run from the shoulder and release to the terminal.
    fn end_run_suspended(&mut self, ctx: &mut GameContext) {
        self.drive().clone().with(ctx, |d, ctx| {
            profile_mut_of(ctx).store_truck_condition(&d.trip.truck);
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
        let city = CityMenuState::new(ctx, false);
        ctx.reset_to(city);
    }
}

// -- TrafficStopState ---------------------------------------------------------------------

const TRAFFIC_STOP_INTRO_HELP: &str = "Enter or Escape pulls back onto the highway.";

/// A roadside traffic stop after a speeding pull-over: a spoken license and
/// logbook check, an on-the-spot ticket or a warning, then back to the road.
pub struct TrafficStopState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    pub signaled: bool,
    pub over: f64,
    pub limit: f64,
    pub clean_stop: bool,
    /// The driver kept rolling through a failure-to-stop warning before
    /// finally pulling in: reckless-class behavior, not just speed.
    pub warned: bool,
    /// Whether the trooper clocked this speed inside roadwork, carried from
    /// where the violation happened rather than read at the shoulder.
    pub construction_zone: bool,
    outcome_text: String,
    presence_detail: String,
}

impl TrafficStopState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        ctx: &mut GameContext,
        driving: &mut DrivingState,
        signaled: bool,
        over: f64,
        limit: f64,
        clean_stop: bool,
        warned: bool,
        construction_zone: bool,
    ) -> Self {
        let presence_detail = driving
            .presence_state(ctx)
            .map(|p| p.detail)
            .unwrap_or_default();
        let mut state = TrafficStopState {
            menu: MenuCore::new("Traffic stop").with_intro_help(TRAFFIC_STOP_INTRO_HELP),
            driving: DriveRef::active(ctx),
            signaled,
            over,
            limit,
            clean_stop,
            warned,
            construction_zone,
            outcome_text: String::new(),
            presence_detail,
        };
        state.resolve(ctx, driving);
        state
    }

    pub fn outcome_text(&self) -> &str {
        &self.outcome_text
    }

    /// Decide the outcome and apply any ticket immediately.
    fn resolve(&mut self, ctx: &mut GameContext, d: &mut DrivingState) {
        let rep = profile_of(ctx).standing();
        let first = d.speeding_tickets == 0;
        // A warning for a first, marginal stop, or for a well-regarded driver
        // who pulled over promptly and wasn't egregiously over; otherwise a
        // ticket.
        let new_driver =
            d.new_driver_warning(ctx) && !self.warned && !enforcement::is_serious_speed(self.over);
        let warning = new_driver
            || (first && self.over < 15.0)
            || (rep >= 70.0 && self.signaled && self.over < 20.0);
        let over_text = ctx.settings.speed_text(self.over);
        let limit_text = ctx.settings.speed_text(self.limit);
        if warning {
            self.outcome_text = format!(
                "{over_text} over the {limit_text} limit. The trooper lets you off with a \
                 warning."
            );
            return;
        }
        // A prompt, fully-compliant stop earns a small chance the trooper lets
        // a ticket slide with a warning instead.
        // Named seed, quantised on where the stop happened: reloading the save
        // must not re-roll whether the trooper let it slide.
        let waiver_key = format!(
            "{}:police:waiver:{}",
            d.trip_seed,
            fmt_f(d.trip.position_mi, 1)
        );
        let waiver_roll = PyRandom::new_from_str(&waiver_key).random();
        if self.clean_stop && waiver_roll < PULL_OVER_CLEAN_STOP_WARN_CHANCE {
            self.outcome_text = format!(
                "{over_text} over the {limit_text} limit. You pulled over promptly, so the \
                 trooper lets it go with a warning."
            );
            return;
        }
        // Priced by how far over the limit, how many citations the career
        // already carries, and whether it happened in a construction zone,
        // against the real state fine schedules.
        let fine = enforcement::speeding_citation_fine(
            self.over,
            career_citations(profile_of(ctx)),
            self.construction_zone,
        );
        d.speeding_tickets += 1;
        d.ticket_fines_paid += fine;
        let hit = hos::HOS_REPUTATION_HIT * if self.signaled { 0.7 } else { 1.0 };
        {
            let p = profile_mut_of(ctx);
            p.spend(fine);
            // The hit comes off the delivery ledger. `rep` is the shown
            // standing, ledger minus the record; writing it back subtracted
            // the record from the ledger for good (2026-09-28).
            p.career.reputation = (p.career.reputation - hit).max(0.0);
        }
        ctx.audio.play("ui/error");
        let serious = enforcement::is_serious_speed(self.over) || self.warned;
        let reason = format!("Speeding, {over_text} over the {limit_text} limit");
        let ladder = d.log_enforcement(ctx, fine, serious, false, &reason);
        self.outcome_text = format!(
            "{over_text} over the {limit_text} limit. Speeding ticket: {} dollars, paid on the \
             spot, and a reputation hit.{}",
            fmt_grouped(fine, 0),
            construction_zone_fine_clause(self.construction_zone)
        );
        if !ladder.is_empty() {
            self.outcome_text.push_str(&format!(" {ladder}"));
        }
        if self.licence_pulled(ctx) {
            let tail = self.suspended_exit_text(ctx, d);
            self.outcome_text.push_str(&tail);
        }
    }
}

impl RoadsideExit for TrafficStopState {
    fn drive(&self) -> &DriveRef {
        &self.driving
    }
}

impl Menu for TrafficStopState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![self.roadside_exit_item(ctx, "Merge back up to speed.")]
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let polite = if self.signaled {
            " You signaled and pulled over promptly."
        } else {
            ""
        };
        let outcome = self.outcome_text.clone();
        let current = self.current_text(ctx);
        ctx.say_with(
            format!(
                "You stop on the shoulder for a license and logbook check.{polite} {outcome} \
                 {current}"
            ),
            Say::new(),
        );
    }

    fn presence(&self, _ctx: &GameContext) -> Option<PresenceState> {
        Some(PresenceState::new("Pulled over", &self.presence_detail))
    }

    fn online_presence(&self, ctx: &GameContext) -> Option<PresenceState> {
        self.presence(ctx)
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        if self.end_run_if_licence_pulled(ctx) {
            return;
        }
        ctx.pop_state();
        ctx.say_with("Back on the highway. Watch your speed.", Say::new());
    }
}

impl_state_for_menu!(TrafficStopState);

// -- EnforcementStopState -----------------------------------------------------------------

const ENFORCEMENT_STOP_INTRO_HELP: &str = "Enter or Escape pulls back onto the highway.";

/// Roadside enforcement stop for non-speeding violations.
pub struct EnforcementStopState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    pub summary: String,
    pub construction_zone: bool,
    pub fine: f64,
    pub reputation_hit: f64,
    pub signaled: bool,
    pub return_message: String,
    pub out_of_service: bool,
    pub warned: bool,
    /// A scale bypass got caught precisely because the inspection was
    /// skipped. The trooper does not just write the ticket and wave you on --
    /// they do the inspection right there, on the shoulder, the same
    /// `INSPECTION_MIN` the check-in lane would have cost you.
    pub inspection_on_stop: bool,
    /// A routine roadside inspection at this level: the report, not the
    /// caller, decides what it costs, and a clean one costs nothing.
    pub inspection_level: Option<InspectionLevel>,
    outcome_text: String,
    /// Whether the stop has been told once already. See `announce_entry`: the
    /// fine is charged here, in `resolve`, exactly once, and a later telling
    /// has to sound like history rather than a fresh charge.
    stop_announced: bool,
    presence_detail: String,
}

impl EnforcementStopState {
    pub fn new(
        ctx: &mut GameContext,
        driving: &mut DrivingState,
        params: EnforcementStopParams,
    ) -> Self {
        let presence_detail = driving
            .presence_state(ctx)
            .map(|p| p.detail)
            .unwrap_or_default();
        // Repeat offenders pay more for the same stop and nothing caps it, and
        // a construction zone doubles whatever that came to -- one schedule
        // for every citation in the game, priced in models/enforcement.
        let fine = citation_fine(
            params.fine,
            career_citations(profile_of(ctx)),
            params.construction_zone,
            None,
        );
        let mut state = EnforcementStopState {
            menu: MenuCore::new(&params.title).with_intro_help(ENFORCEMENT_STOP_INTRO_HELP),
            driving: DriveRef::active(ctx),
            summary: params.summary,
            construction_zone: params.construction_zone,
            fine,
            reputation_hit: params.reputation_hit,
            signaled: params.signaled,
            return_message: params.return_message,
            out_of_service: params.out_of_service,
            warned: params.warned,
            inspection_on_stop: params.inspection_on_stop,
            inspection_level: params.inspection_level,
            outcome_text: String::new(),
            stop_announced: false,
            presence_detail,
        };
        state.resolve(ctx, driving);
        state
    }

    pub fn outcome_text(&self) -> &str {
        &self.outcome_text
    }

    fn resolve(&mut self, ctx: &mut GameContext, d: &mut DrivingState) {
        if let Some(level) = self.inspection_level {
            // The stop is the inspection: what it costs comes off the
            // report, and a clean one costs the minutes and nothing else.
            let report = d.inspection_report(ctx, level);
            self.fine = report.total_fine();
            self.outcome_text = d.settle_inspection(ctx, &report);
            if self.licence_pulled(ctx) {
                let tail = self.suspended_exit_text(ctx, d);
                self.outcome_text.push_str(&tail);
            }
            return;
        }
        if d.new_driver_warning(ctx)
            && !self.warned
            && !self.out_of_service
            && !self.inspection_on_stop
        {
            self.fine = 0.0;
            self.outcome_text = "The officer lets you go with a warning.".to_string();
            return;
        }
        d.ticket_fines_paid += self.fine;
        let hit = self.reputation_hit * if self.signaled { 0.8 } else { 1.0 };
        {
            let p = profile_mut_of(ctx);
            p.spend(self.fine);
            p.career.reputation = (p.career.reputation - hit).max(0.0);
        }
        ctx.audio.play("ui/error");
        let reason = self.menu.title.clone();
        let ladder = d.log_enforcement(ctx, self.fine, self.warned, false, &reason);
        self.outcome_text = format!(
            "Fine: {} dollars, paid on the spot, and a reputation hit.{}",
            fmt_grouped(self.fine, 0),
            construction_zone_fine_clause(self.construction_zone)
        );
        if !ladder.is_empty() {
            self.outcome_text.push_str(&format!(" {ladder}"));
        }
        if self.out_of_service {
            // The ten hours pass HERE, parked on the shoulder with the
            // officer's order in hand -- never as a silent mid-drive jump.
            // Capture the plain-language WHY before the reset wipes the
            // ledger: the stop must explain itself completely (owner ask,
            // 2026-07-24).
            let mode = ctx.settings.hos_mode.clone();
            let causes = hos_of(ctx).violation_causes(&mode);
            let why = if causes.is_empty() {
                String::new()
            } else {
                format!(" The order stands because {}.", causes.join(" and "))
            };
            // Ten hours parked is a real overnight fast-forward, the same as
            // every other sleep path -- the engine must not idle through the
            // whole order (log, 2026-08-12: it did, and the audio froze at
            // pre-stop revs for the entire ten hours). A missed 30-minute
            // break is not overnight: leave the engine running.
            let minutes = hos_of(ctx).out_of_service_minutes(&mode);
            let break_only = minutes < hos::SLEEP_MIN;
            let engine_off = if break_only {
                String::new()
            } else {
                shut_down_engine(d, ctx)
            };
            d.place_out_of_service_minutes(ctx, minutes);
            let lead = if engine_off.trim().is_empty() {
                String::new()
            } else {
                format!(" {}", engine_off.trim())
            };
            let oos_line = if break_only {
                format!(
                    "{lead}{why} Out of service: thirty minutes parked on the shoulder. It is \
                     {}, the 30-minute break is satisfied, and the delivery deadline kept \
                     counting.{}",
                    clock_text(d.trip.local_hour()),
                    wake_air_instruction(d)
                )
            } else {
                format!(
                    "{lead}{why} Out of service: ten hours parked on the shoulder. It is {}, \
                     driving and duty clocks reset, you wake rested, and the delivery deadline kept \
                     counting.{}",
                    clock_text(d.trip.local_hour()),
                    wake_air_instruction(d)
                )
            };
            self.outcome_text.push_str(&oos_line);
        }
        if self.inspection_on_stop {
            // Reuses the same check-in cost the scale itself would have
            // charged -- the driver dodged the lane, not the inspection.
            advance_rest_clock(
                d,
                ctx,
                INSPECTION_MIN,
                Some("on_duty_not_driving"),
                "weigh station bypass inspection",
            );
            hos_mut_of(ctx).on_duty(INSPECTION_MIN);
            self.outcome_text.push_str(&format!(
                " The skipped scale's full inspection runs here on the shoulder: {} minutes.",
                fmt_f(INSPECTION_MIN, 0)
            ));
        }
        if self.licence_pulled(ctx) {
            let tail = self.suspended_exit_text(ctx, d);
            self.outcome_text.push_str(&tail);
        }
    }
}

impl RoadsideExit for EnforcementStopState {
    fn drive(&self) -> &DriveRef {
        &self.driving
    }
}

impl Menu for EnforcementStopState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![self.roadside_exit_item(ctx, "Merge back up to speed.")]
    }

    /// The stop, said once as it happens and afterwards as history.
    ///
    /// `resolve` runs in the constructor and charges the fine exactly once,
    /// but this line was word-for-word identical every time it was spoken, so
    /// a second telling was indistinguishable from a second charge -- and a
    /// driver with no screen has no other way to tell them apart. Tester
    /// Darren's log has it twice, three seconds apart, on a 1,200 dollar
    /// work-zone citation (I-75, 2026-08-18).
    ///
    /// Not silenced, because re-reading the stop is the only route back to
    /// the detail. Led in the past tense instead, so the money is plainly
    /// already spent.
    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let polite = if self.signaled {
            " You signaled and pulled over promptly."
        } else {
            ""
        };
        let summary = self.summary.clone();
        let outcome = self.outcome_text.clone();
        let current = self.current_text(ctx);
        if self.stop_announced {
            ctx.say_with(
                format!("Stop already settled. {summary} {outcome} {current}"),
                Say::new(),
            );
            return;
        }
        self.stop_announced = true;
        ctx.say_with(
            format!(
                "You stop on the shoulder for an enforcement inspection.{polite} {summary} \
                 {outcome} {current}"
            ),
            Say::new(),
        );
    }

    fn presence(&self, _ctx: &GameContext) -> Option<PresenceState> {
        Some(PresenceState::new("Pulled over", &self.presence_detail))
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        if self.end_run_if_licence_pulled(ctx) {
            return;
        }
        ctx.pop_state();
        let message = self.return_message.clone();
        ctx.say_with(message, Say::new());
    }
}

impl_state_for_menu!(EnforcementStopState);

// -- LicencePulledState -------------------------------------------------------------------

const LICENCE_PULLED_INTRO_HELP: &str = "Enter or Escape returns to the terminal.";

/// A CDL pulled mid-drive with no officer on the shoulder: a second run off
/// the road asleep, or the barrels, on top of a serious violation already on
/// the record.
///
/// The roadside screens already end the run when the stop pulls the
/// license. These two happen at speed, and the drive used to carry straight
/// on with a suspended CDL. This screen is the roadside ending without the
/// stop: the truck stops on the shoulder and the same exit closes out the
/// run.
pub struct LicencePulledState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    outcome_text: String,
}

impl LicencePulledState {
    pub fn new(ctx: &mut GameContext, driving: &mut DrivingState) -> Self {
        let title = {
            let record = &profile_of(ctx).driving_record;
            if record.lifetime_disqualified {
                "CDL disqualified for life".to_string()
            } else {
                format!("CDL {}", enforcement::status_verb(record))
            }
        };
        let mut state = LicencePulledState {
            menu: MenuCore::new(&title).with_intro_help(LICENCE_PULLED_INTRO_HELP),
            driving: DriveRef::active(ctx),
            outcome_text: String::new(),
        };
        let tail = state.suspended_exit_text(ctx, driving);
        state.outcome_text = format!("You pull onto the shoulder and stop.{tail}");
        state
    }

    pub fn outcome_text(&self) -> &str {
        &self.outcome_text
    }
}

impl RoadsideExit for LicencePulledState {
    fn drive(&self) -> &DriveRef {
        &self.driving
    }
}

impl Menu for LicencePulledState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![self.roadside_exit_item(ctx, "Merge back up to speed.")]
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let title = self.menu.title.clone();
        let outcome = self.outcome_text.clone();
        let current = self.current_text(ctx);
        // Queued: the event line that pulled the license is still being said.
        ctx.say_with(format!("{title}. {outcome} {current}"), Say::queued());
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        if !self.end_run_if_licence_pulled(ctx) {
            ctx.pop_state();
        }
    }
}

impl_state_for_menu!(LicencePulledState);

// -- FelonyStopState ----------------------------------------------------------------------

const FELONY_INTRO_HELP: &str = "Enter or Escape continues from the terminal.";

/// Failure-to-stop outcome after the player ignores an active siren.
pub struct FelonyStopState {
    menu: MenuCore<Self>,
    pub load_lost: bool,
    summary: String,
    standing_text: String,
}

impl FelonyStopState {
    pub fn new(ctx: &mut GameContext, driving: &mut DrivingState) -> Self {
        let load_lost = driving.phase == DRIVE_PHASE_DELIVERY && !driving.job.bobtail;
        let mut state = FelonyStopState {
            menu: MenuCore::new("Felony stop").with_intro_help(FELONY_INTRO_HELP),
            load_lost,
            summary: String::new(),
            standing_text: String::new(),
        };
        state.resolve(ctx, driving);
        state
    }

    pub fn summary(&self) -> &str {
        &self.summary
    }

    fn resolve(&mut self, ctx: &mut GameContext, d: &mut DrivingState) {
        d.failure_to_stop_count += 1;
        // The felony is priced like every other citation: the 5,000-dollar
        // statutory top of range, scaled by what this driver already has on
        // the record and by where the chase was.
        let zone = d.trip.in_construction_zone();
        let fine = citation_fine(
            enforcement::FAILURE_TO_STOP_FINE,
            career_citations(profile_of(ctx)),
            zone,
            None,
        );
        d.ticket_fines_paid += fine;
        {
            let p = profile_mut_of(ctx);
            p.spend(fine);
            p.career.reputation = (p.career.reputation - hos::HOS_REPUTATION_HIT * 3.0).max(0.0);
        }
        // The part that used to go nowhere: fleeing a stop in a commercial
        // vehicle is a major offense, and the license answers for it.
        self.standing_text = d.log_enforcement(ctx, fine, false, true, "Ran from a traffic stop");
        // add_damage, not a raw assignment: spike damage has to cross the
        // bands so a spiked truck can go out of service like any other wreck.
        d.trip.truck.add_damage(FAILURE_TO_STOP_DAMAGE_PCT, true);
        d.trip.truck.velocity_mps = 0.0;
        d.trip.truck.throttle = 0.0;
        d.trip.truck.brake = 1.0;
        d.trip.truck.set_parking_brake();
        advance_rest_clock(
            d,
            ctx,
            FAILURE_TO_STOP_PROCESSING_MIN,
            Some("on_duty_not_driving"),
            "felony failure-to-stop enforcement",
        );
        hos_mut_of(ctx).on_duty(FAILURE_TO_STOP_PROCESSING_MIN);
        profile_mut_of(ctx).store_truck_condition(&d.trip.truck);
        let hours = d.trip.game_minutes / 60.0;
        let market_day = {
            let p = profile_mut_of(ctx);
            p.game_hours += hours;
            p.market_day()
        };
        {
            let p = profile_mut_of(ctx);
            p.market.advance_to(market_day);
            p.active_trip = None;
            p.pay_advance_used_for_load = false;
        }
        ctx.save_profile();

        let load_text = if self.load_lost {
            format!(
                "Dispatch cancels the {} load. No pay for this run.",
                d.job.cargo.label
            )
        } else {
            "No loaded trailer to lose, but the assignment is canceled.".to_string()
        };
        let terminal = ctx
            .world
            .home_terminal(&profile_of(ctx).current_city)
            .map(|t| t.spoken_name())
            .unwrap_or_else(|_| "the terminal".to_string());
        self.summary = format!(
            "Troopers laid spike strips across the lane after you kept driving with lights and \
             siren behind you. Felony failure-to-stop fine: {} dollars, paid on the spot, with a \
             major reputation hit.{} Spike strips added {} percent truck damage, processing \
             took {} hours. {load_text} You are released back to {terminal}.",
            fmt_grouped(fine, 0),
            construction_zone_fine_clause(zone),
            fmt_f(FAILURE_TO_STOP_DAMAGE_PCT, 0),
            fmt_f(FAILURE_TO_STOP_PROCESSING_MIN / 60.0, 0)
        );
        if !self.standing_text.is_empty() {
            self.summary.push_str(&format!(" {}", self.standing_text));
        }
    }
}

impl Menu for FelonyStopState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new("Return to terminal", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Continue from the city terminal."),
        ]
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let title = self.menu.title.clone();
        let summary = self.summary.clone();
        let current = self.current_text(ctx);
        ctx.say_with(format!("{title}. {summary} {current}"), Say::new());
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        let city = CityMenuState::new(ctx, false);
        ctx.reset_to(city);
    }
}

impl_state_for_menu!(FelonyStopState);
