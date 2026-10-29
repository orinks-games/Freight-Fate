//! The delivery settlement (`ArrivalState`): what the run earned, what it
//! cost, and everything the career learned from it.
//!
//! The screen keeps no handle on the drive. Every number is read out of the
//! drive once, in [`ArrivalState::new`] (Python's `__init__` -> `_settle`),
//! and the rows are the resulting lines -- so the settlement cannot change
//! under the player while they arrow through it.

use std::time::{SystemTime, UNIX_EPOCH};

use ff_core::data::world_models::HomeTerminal;
use ff_core::models::business::{display_rank_for, uses_company_career_ranks_for, SettlementTerms};
use ff_core::models::career::{standing_xp_rate, xp_rate_settlement_clause};
use ff_core::models::career_ladder::relabel_level_up_announcements;
use ff_core::models::cargo_condition::{cargo_condition_text, settle_cargo, CargoSettlement};
use ff_core::models::carrier_fleet::{
    assigned_truck_key, equipment_held_back, fleet_tier_for_level, fleet_upgrade_announcement,
    withheld_promotion_text, WITHHELD_UNLOCK_TAIL,
};
use ff_core::models::credentials::MANUAL_SPEC_DIFFERENTIAL;
use ff_core::models::enforcement;
use ff_core::models::jobs::{lane_key, Job};
use ff_core::models::solvency::{self, deductions_from_settlement};
use ff_core::models::trucks::truck_model;
use ff_core::pyfmt::{fmt_f, fmt_grouped, round_py_n};
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
use crate::states::driving_menu_states::badges::award_arrival_achievements;
use crate::states::driving_menu_states::{
    DELIVERY_SETTLEMENT_MAX_AVERAGE_MPH, ROAD_GRIME_PER_MILE, SETTLEMENT_LOW_FUEL_FRACTION,
};

/// `_settlement_hours(driving)`: no run settles at a faster average than a
/// truck can actually hold.
pub fn settlement_hours(driving: &DrivingState) -> f64 {
    let driven_hours = driving.trip.game_minutes / 60.0;
    let minimum_hours = driving.job.distance_mi / DELIVERY_SETTLEMENT_MAX_AVERAGE_MPH;
    driven_hours.max(minimum_hours)
}

const ARRIVAL_INTRO_HELP: &str =
    "Up and down review the summary. Enter repeats the line. Escape continues.";

pub struct ArrivalState {
    menu: MenuCore<Self>,
    pub summary_parts: Vec<String>,
    achievement_messages: Vec<String>,
    new_achievement_names: Vec<String>,
    announcements: Vec<String>,
    pub summary_lines: Vec<String>,
    pub terminal: HomeTerminal,
    /// Miles the co-driver drove on this load; 0 when the player drove it.
    pub(crate) team_miles: f64,
    /// The road journal's line for a team load.
    pub team_note: Option<String>,
    /// The settlement row for the co-driver's share of the pay.
    pub(crate) team_line: Option<String>,
}

impl ArrivalState {
    pub fn new(ctx: &mut GameContext, driving: &mut DrivingState) -> Self {
        let arrival_hours = settlement_hours(driving);
        Self::new_at(ctx, driving, arrival_hours)
    }

    /// Build settlement with an appointment time captured at receiver
    /// check-in. `driving` may already include later unloading time.
    pub fn new_at(ctx: &mut GameContext, driving: &mut DrivingState, arrival_hours: f64) -> Self {
        let terminal = ctx
            .world
            .home_terminal(&driving.job.destination)
            .unwrap_or_else(|_| {
                HomeTerminal::new("Terminal", &driving.job.destination, "", "yard")
            });
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

    /// Empty reposition run: relocate to the destination city.
    ///
    /// A self-serve bobtail (owner-operators only) carries no pay. A
    /// carrier-ASSIGNED reposition (`job.assigned`) still pays -- at the
    /// reduced empty-mile rate `make_reposition_job` already baked into
    /// `job.pay` -- and still earns mileage XP the way any other completed
    /// drive does, because it IS a completed dispatch assignment, just an
    /// empty one.
    fn settle_bobtail(
        &mut self,
        ctx: &mut GameContext,
        d: &mut DrivingState,
        hours: f64,
        trip_damage: f64,
    ) {
        self.menu.title = "Repositioned".to_string();
        let job = d.job.clone();
        let destination_timezone = d.trip.destination_timezone();
        let fuel_fraction = d.trip.truck.fuel_fraction();
        {
            let p = profile_mut_of(ctx);
            p.current_city = job.destination.clone();
            let driver_charges = p.fines_owed;
            if driver_charges != 0.0 {
                p.spend(driver_charges);
                p.fines_owed = 0.0;
                self.summary_parts.push(format!(
                    "Fines carried over from earlier loads: {} dollars, settled.",
                    fmt_grouped(driver_charges, 0)
                ));
            }
        }
        profile_mut_of(ctx).store_truck_condition(&d.trip.truck);
        let mut announcements: Vec<String> = Vec::new();
        let pay_clause = if job.assigned {
            let on_time = hours <= job.deadline_game_h;
            let (previous_level, money) = {
                let p = profile_mut_of(ctx);
                p.earn(job.pay);
                (p.career.level(), p.money())
            };
            let standing = enforcement::standing_band(profile_of(ctx));
            announcements = profile_mut_of(ctx).career.record_delivery(
                job.distance_mi,
                job.pay,
                on_time,
                trip_damage,
                1.0,
                standing_xp_rate(standing),
            );
            {
                let p = profile_of(ctx);
                relabel_level_up_announcements(
                    &mut announcements,
                    previous_level,
                    p.career.level(),
                    uses_company_career_ranks_for(p),
                );
            }
            let promotion = self.handle_fleet_promotion(ctx, previous_level, &mut announcements);
            announcements.extend(promotion);
            format!(
                "Reposition pay at the empty-mile rate: {} dollars. You now have {} dollars. ",
                fmt_grouped(job.pay, 0),
                fmt_grouped(money, 0)
            )
        } else {
            "No load and no pay. ".to_string()
        };
        let (game_hours, market_day) = {
            let p = profile_mut_of(ctx);
            p.game_hours += hours;
            (p.game_hours, p.market_day())
        };
        {
            let p = profile_mut_of(ctx);
            p.market.advance_to(market_day);
            p.active_trip = None;
            p.pay_advance_used_for_load = false;
        }
        {
            let cleared = {
                let p = profile_mut_of(ctx);
                let now = p.game_hours;
                let lines = p.career.activate_pending(now, true);
                if !lines.is_empty() {
                    p.dispatch_board_cache = None;
                }
                lines
            };
            announcements.extend(cleared);
        }
        ctx.save_profile();
        self.summary_parts.insert(
            0,
            format!(
                "Bobtailed empty to {} in {} hours. It is {}. {pay_clause}Parked at {}. \
                 {} dispatch board available. Fuel {} percent.",
                job.spoken_destination(),
                fmt_f(hours, 1),
                clock_text(to_local(game_hours, destination_timezone)),
                self.terminal.name,
                job.spoken_destination(),
                fmt_f(fuel_fraction * 100.0, 0)
            ),
        );
        if trip_damage > 1.0 {
            self.summary_parts.push(format!(
                "The empty run added {} percent truck damage.",
                fmt_f(trip_damage, 0)
            ));
        }
        self.summary_parts.extend(announcements);
        if let Some(result) = ctx.award_achievement_with("bobtail_done", false, false) {
            self.summary_parts.push(result.award.message.normal.clone());
        }
        // The arrival screen and announcement read summary_lines, not parts.
        self.summary_lines = self.summary_parts.clone();
    }

    /// What the receiver found, what it cost, and who carries the claim.
    ///
    /// Always in that order, and always with the money named, because this
    /// is the largest single thing that can happen to a run and the player
    /// has no way to see the trailer.
    fn cargo_settlement_line(
        &self,
        ctx: &mut GameContext,
        d: &DrivingState,
        cargo: &CargoSettlement,
        liquid: bool,
    ) -> String {
        let owner_op = is_owner_operator(&profile_of(ctx).business_status);
        let claim_holder = if owner_op {
            "Claim against your own authority"
        } else {
            "Carrier carries the claim, on your record"
        };
        if d.terse_speech(ctx) {
            let head = match cargo.outcome.as_str() {
                "exception" => "Exception on the bill.",
                "claim" => "Freight claim.",
                _ => "Load refused.",
            };
            let claim = if cargo.claim_value >= 1.0 {
                format!(" Claim {} dollars.", fmt_grouped(cargo.claim_value, 0))
            } else {
                String::new()
            };
            return format!(
                "{head} Load {}, {} percent. Pay down {} dollars.{claim}",
                cargo_condition_text(cargo.condition_pct, liquid),
                fmt_f(cargo.condition_pct, 0),
                fmt_grouped(cargo.pay_loss, 0)
            );
        }
        if cargo.rejected() {
            return format!(
                "Receiver refused the load, {} at {} percent. No pay, {} dollars gone, claim \
                 about {}. {claim_holder}.",
                cargo_condition_text(cargo.condition_pct, liquid),
                fmt_f(cargo.condition_pct, 0),
                fmt_grouped(cargo.pay_loss, 0),
                fmt_grouped(cargo.claim_value, 0)
            );
        }
        if cargo.outcome == "claim" {
            return format!(
                "Receiver took the load with an exception, {} at {} percent. Claim about {} \
                 dollars, {} off this settlement. {claim_holder}.",
                cargo_condition_text(cargo.condition_pct, liquid),
                fmt_f(cargo.condition_pct, 0),
                fmt_grouped(cargo.claim_value, 0),
                fmt_grouped(cargo.pay_loss, 0)
            );
        }
        format!(
            "Exception on the bill of lading, load arrived {} at {} percent. {} dollars held \
             back.",
            cargo_condition_text(cargo.condition_pct, liquid),
            fmt_f(cargo.condition_pct, 0),
            fmt_grouped(cargo.pay_loss, 0)
        )
    }

    fn settle(&mut self, ctx: &mut GameContext, d: &mut DrivingState, arrival_hours: f64) {
        let job = d.job.clone();
        let elapsed_hours = settlement_hours(d);
        let trip_damage = (d.trip.truck.damage_pct - d.start_damage).max(0.0);
        if job.bobtail {
            self.settle_bobtail(ctx, d, elapsed_hours, trip_damage);
            return;
        }
        // Appointment performance and haul pay stop at receiver check-in.
        // The post-check-in clock still advances below so unloading remains
        // real calendar, market, HOS, idle-fuel, and player-visible time.
        let hours = arrival_hours;
        let gross_base = job.payout_default(hours, trip_damage);
        let toll_expense = d.trip.toll_expense();
        let on_time = hours <= job.deadline_game_h;
        let accessorials = carrier_accessorial_charges(&job, Some(profile_of(ctx)));
        let carrier_charges = toll_expense + charge_total(&accessorials);
        // What a previous load could not cover is no longer piled onto this
        // settlement whole. It is a balance owed, and a balance owed is
        // recovered at a capped share further down -- taking all of it here
        // is what made every settlement after the first shortfall pay zero.
        let carried_balance = profile_of(ctx).fines_owed.max(0.0);
        // Nothing is billed here for speeding a trooper did not see: that
        // charge is gone, and it was never anybody's to bill.
        let mut driver_charges = 0.0;
        // A company driver who brings a truck back damaged does not get a
        // clean settlement: the carrier eats the repair, and the driver eats
        // the deductible and the safety bonus. An owner-operator has already
        // paid the whole repair themselves, so nobody charges them twice.
        let (damage_deductible, damage_reputation_hit, damage_reason) =
            if is_owner_operator(&profile_of(ctx).business_status) {
                (0.0, 0.0, String::new())
            } else {
                preventable_damage_charge(d)
            };
        driver_charges += damage_deductible;
        // The dock inspects before it signs. Under the Carmack Amendment the
        // carrier owes the value of freight it damages, and a receiver may
        // refuse a bad load outright -- which is why this is the largest
        // consequence in the game and comes off the top of the haul.
        let cargo = settle_cargo(d.trip.truck.cargo_damage_pct, gross_base);
        let liquid = d.trip.truck.liquid.is_some();
        driver_charges += cargo.pay_loss;

        let owned_trailers: Vec<String> = profile_of(ctx).owned_trailers.clone();
        let owned_refs: Vec<&str> = owned_trailers.iter().map(String::as_str).collect();
        let carrier_key = profile_of(ctx).carrier_key.clone();
        let reputation_before = profile_of(ctx).standing();
        let transponder = has_weigh_station_transponder(profile_of(ctx));
        let record_surcharge = enforcement::record_insurance_surcharge(profile_of(ctx));
        let business_status = profile_of(ctx).business_status.clone();
        let terms = SettlementTerms {
            carrier_key: Some(carrier_key.as_str()),
            owned_trailers: &owned_refs,
            reputation: Some(reputation_before),
            transponder,
            record_surcharge,
        };
        let business = ff_core::models::business::build_business_settlement(
            &business_status,
            &job,
            gross_base,
            on_time,
            driver_charges,
            &terms,
        );
        let no_on_time_bonus_business = ff_core::models::business::build_business_settlement(
            &business_status,
            &job,
            job.payout(hours, trip_damage, 0.0),
            on_time,
            driver_charges,
            &terms,
        );
        let trust_bonus = if is_owner_operator(&business_status) {
            0.0
        } else {
            reputation_pay_bonus(business.gross_pay, Some(reputation_before))
        };
        let deadline_terms = SettlementTerms {
            carrier_key: Some(carrier_key.as_str()),
            owned_trailers: &owned_refs,
            reputation: None,
            transponder,
            record_surcharge,
        };
        let deadline_business = ff_core::models::business::build_business_settlement(
            &business_status,
            &job,
            job.payout_default(job.deadline_game_h, trip_damage),
            true,
            driver_charges,
            &deadline_terms,
        );
        let gross_pay = business.gross_pay;
        let on_time_bonus_paid = (gross_pay - no_on_time_bonus_business.gross_pay).max(0.0);
        let early_bonus = (gross_pay - deadline_business.gross_pay).max(0.0);
        // Anything this load could not cover is carried, not forgiven: a load
        // too cheap to pay its charges used to be told it paid them in full.
        // The carried balance from earlier loads is added back after
        // collection below, so it is never taken twice.
        profile_mut_of(ctx).fines_owed = business.uncollected_charges;
        if !cargo.clean() {
            let line = self.cargo_settlement_line(ctx, d, &cargo, liquid);
            self.summary_parts.push(line);
            let p = profile_mut_of(ctx);
            p.career.reputation = (p.career.reputation - cargo.reputation_hit).max(0.0);
            ff_core::achievements::increment_stat(profile_mut_of(ctx), "cargo_claims");
        }
        // A balance carried from an earlier load is NOT reported here. It is
        // recovered further down at the capped share, and says so there --
        // billing it in both places is what used to zero a settlement twice.
        if damage_deductible >= 1.0 {
            {
                let p = profile_mut_of(ctx);
                p.career.reputation = (p.career.reputation - damage_reputation_hit).max(0.0);
            }
            d.record_equipment_event(ctx);
            self.summary_parts.push(format!(
                "Preventable damage, {damage_reason}. Carrier covers the repair. Deductible {} \
                 dollars, safety bonus void. Reputation down {}.",
                fmt_grouped(damage_deductible, 0),
                fmt_f(damage_reputation_hit, 0)
            ));
        }
        if business.uncollected_charges > 0.0 {
            let paid_now = driver_charges - business.uncollected_charges;
            self.summary_parts.push(format!(
                "This load covered {} dollars of those charges. {} dollars stays owed, paid \
                 down at a quarter of each settlement at most.",
                fmt_grouped(paid_now, 0),
                fmt_grouped(business.uncollected_charges, 0)
            ));
        }
        // Tickets from being pulled over were already paid on the spot;
        // report them for transparency but don't deduct again at settlement.
        if d.speeding_tickets != 0 {
            self.summary_parts.push(format!(
                "On-the-spot speeding tickets this trip: {}, already paid, {} dollars.",
                d.speeding_tickets,
                fmt_grouped(d.ticket_fines_paid, 0)
            ));
        }
        // Engine brake citations were also paid on the spot; reported here
        // for the same transparency, never deducted again.
        if d.jake_zone_fines != 0 {
            self.summary_parts.push(format!(
                "Engine brake citations this trip: {}, already paid, {} dollars.",
                d.jake_zone_fines,
                fmt_grouped(d.jake_fines_paid, 0)
            ));
        }
        // Anything the trip did to your standing is said once more here, so a
        // suspension heard out on the road is not the last word on it.
        for line in &d.record_events {
            self.summary_parts.push(line.clone());
        }
        if !business.business_charges.is_empty() {
            self.summary_parts.push(format!(
                "Owner-operator business costs: {}.",
                business.business_charge_summary()
            ));
        }
        let mut net_pay = business.net_before_advance;
        // What the load earned the driver, before any advance comes back out
        // of it -- which is exactly what the business settlement already
        // calls net_before_advance. An advance is those same dollars paid
        // early, so lifetime earnings book the whole settlement; book only
        // the remainder and the advanced money becomes cash the career cannot
        // account for, which reads as an edited save to cloud upload
        // screening. Accessorials and tolls are cash on the existing ledger,
        // not a second settlement, so they never fold into career earnings.
        //
        // The manual-spec differential: trained out of the automatic-only
        // restriction, and actually rowing the gears on this run -- the
        // whole run, not just the gate (`drove_automatic`). Added
        // before settled_pay is taken, so the cash and the career's booked
        // earnings agree -- unbooked cash reads as an edited save to cloud
        // upload screening.
        if profile_of(ctx)
            .career
            .endorsements()
            .contains("manual_transmission")
            && !d.trip.truck.transmission.automatic
            && !d.drove_automatic
        {
            let manual_bonus = round_py_n(net_pay * MANUAL_SPEC_DIFFERENTIAL, 2).max(0.0);
            if manual_bonus >= 1.0 {
                net_pay = round_py_n(net_pay + manual_bonus, 2);
                self.summary_parts.push(format!(
                    "Manual-spec differential: {} dollars.",
                    fmt_grouped(manual_bonus, 0)
                ));
            }
        }
        // A team load: the co-driver's share of their miles comes off here.
        let team_xp_rate = self.settle_team_share(ctx, d, &business_status, &mut net_pay);
        let settled_pay = net_pay;
        // Owner-ops ARE the carrier: detention, lumpers, washouts, and tolls
        // already sit on the existing ledger. Apply that ledger to the wallet
        // here. Detention is a negative charge, so this pays them; the rest
        // charges them. Company drivers keep the spoken ledger and the money
        // stays put.
        if is_owner_operator(&business_status) {
            net_pay = round_py_n(net_pay + wallet_delta(&accessorials, toll_expense), 2);
        }
        // A balance owed and an advance come out of the same capped share, so
        // a driver who is carrying both still finishes the run with money.
        // With nothing owed this is the arithmetic it has always been.
        let (collected, advance_repaid) =
            deductions_from_settlement(carried_balance, profile_of(ctx).pay_advance, net_pay);
        if collected > 0.0 {
            net_pay = round_py_n(net_pay - collected, 2);
            self.summary_parts.push(format!(
                "Balance owed: {} dollars of this settlement paid it down, a quarter at most.",
                fmt_grouped(collected, 0)
            ));
        }
        if advance_repaid > 0.0 {
            net_pay = round_py_n(net_pay - advance_repaid, 2);
            let pay_advance = {
                let p = profile_mut_of(ctx);
                p.pay_advance = round_py_n(p.pay_advance - advance_repaid, 2);
                p.pay_advance
            };
            let outstanding = if pay_advance >= 1.0 {
                format!(
                    " {} dollars of advance still outstanding.",
                    fmt_grouped(pay_advance, 0)
                )
            } else {
                String::new()
            };
            self.summary_parts.push(format!(
                "Pay advance repaid from this settlement: {} dollars.{outstanding}",
                fmt_grouped(advance_repaid, 0)
            ));
        }
        // What is left of the old balance rides on with whatever this load
        // could not cover.
        {
            let p = profile_mut_of(ctx);
            p.fines_owed = round_py_n(p.fines_owed + (carried_balance - collected).max(0.0), 2);
            p.earn(net_pay);
            p.current_city = job.destination.clone();
        }
        let lane = lane_key(ctx.world, &job);
        profile_mut_of(ctx).remember_lane(&lane);
        // Tire, brake, and engine wear now come off the truck itself -- the
        // physics accrued them mile by mile during the run. Grime stays a
        // simple per-mile film; it has no physics to earn it.
        profile_mut_of(ctx).store_truck_condition(&d.trip.truck);
        let road_grime_added = (job.distance_mi * ROAD_GRIME_PER_MILE).min(100.0);
        {
            let p = profile_mut_of(ctx);
            let grime = (p.road_grime_pct() + road_grime_added).min(100.0);
            p.set_road_grime_pct(grime);
        }
        let previous_level = profile_of(ctx).career.level();
        // Where the driver stands with the carrier, read after the money has
        // moved so a settlement that pays the balance down counts immediately.
        let standing = enforcement::standing_band(profile_of(ctx));
        let mut announcements = profile_mut_of(ctx).career.record_delivery(
            job.distance_mi,
            // The whole settlement, not what survived the advance repayment.
            settled_pay,
            on_time,
            trip_damage,
            xp_class_multiplier(job.cargo),
            standing_xp_rate(standing) * team_xp_rate,
        );
        {
            let p = profile_of(ctx);
            relabel_level_up_announcements(
                &mut announcements,
                previous_level,
                p.career.level(),
                uses_company_career_ranks_for(p),
            );
        }
        let promotion = self.handle_fleet_promotion(ctx, previous_level, &mut announcements);
        announcements.extend(promotion);
        let mut xp_bonus_notes: Vec<String> = Vec::new();
        if xp_class_multiplier(job.cargo) > 1.0 {
            xp_bonus_notes.push("demanding freight".to_string());
        }
        let on_time_streak = profile_of(ctx).career.on_time_streak;
        let streak_bonus = if on_time {
            xp_streak_bonus(on_time_streak)
        } else {
            0.0
        };
        if streak_bonus > 0.0 {
            xp_bonus_notes.push(format!("a {on_time_streak}-delivery on-time streak"));
        }
        // The slower rate rides the line that is already there rather than
        // arriving as its own announcement, and it never names a multiplier:
        // a number the player cannot check turns every settlement into
        // arithmetic.
        let rate_clause = xp_rate_settlement_clause(standing);
        if !xp_bonus_notes.is_empty() {
            let tail = if rate_clause.is_empty() {
                String::new()
            } else {
                format!(", {rate_clause}")
            };
            self.summary_parts.push(format!(
                "Career experience bonus for {}{tail}.",
                xp_bonus_notes.join(" and ")
            ));
        } else if !rate_clause.is_empty() {
            self.summary_parts
                .push(format!("Career experience is coming in {rate_clause}."));
        }
        self.debt_settlement_lines(ctx, d);
        if trust_bonus >= 1.0 {
            self.summary_parts.push(format!(
                "Dispatch trust bonus: {} dollars for your reputation: {}",
                fmt_grouped(trust_bonus, 0),
                enforcement::reputation_text(reputation_before, ctx.settings.score_display)
            ));
        }
        let (game_hours, market_day) = {
            let p = profile_mut_of(ctx);
            p.game_hours += elapsed_hours;
            (p.game_hours, p.market_day())
        };
        {
            let p = profile_mut_of(ctx);
            p.market.advance_to(market_day);
            p.active_trip = None;
            p.pay_advance_used_for_load = false;
        }
        // A background check that came due mid-run clears here: the wait is
        // paperwork time, not a hold on driving, so the news arrives with
        // the settlement (and is repeated at the next terminal, since a
        // grant buried in delivery chatter is easy to miss).
        {
            let cleared = {
                let p = profile_mut_of(ctx);
                let now = p.game_hours;
                let lines = p.career.activate_pending(now, true);
                if !lines.is_empty() {
                    p.dispatch_board_cache = None;
                }
                lines
            };
            announcements.extend(cleared);
        }
        ctx.mark_meaningful_play(MeaningfulPlayReason::DeliveryCompleted);
        ctx.save_profile();
        let occurred_at_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let delivery_facts = DeliveryFacts {
            profile_name: profile_of(ctx).name.clone(),
            deliveries: profile_of(ctx).career.deliveries,
            cargo_key: job.cargo.key.to_string(),
            cargo_label: job.cargo.label.to_string(),
            job_origin: job.origin.clone(),
            job_destination: job.destination.clone(),
            distance_mi: job.distance_mi,
            weight_tons: job.weight_tons,
            team_note: self.team_note.clone(),
        };
        let origin_name = ctx.world.spoken_city(&job.origin, None);
        let destination_name = ctx.world.spoken_city(&job.destination, None);
        if queue_delivery(
            &ctx.services.journal,
            &delivery_facts,
            &origin_name,
            &destination_name,
            on_time,
            occurred_at_ms,
            trip_damage <= 1.0,
        ) {
            ctx.services.journal.flush_async();
        }
        let career_facts = CareerFacts {
            profile_name: profile_of(ctx).name.clone(),
            deliveries: profile_of(ctx).career.deliveries,
            level: profile_of(ctx).career.level(),
        };
        if queue_career_milestones(
            &ctx.services.journal,
            &career_facts,
            previous_level,
            occurred_at_ms,
        ) > 0
        {
            ctx.services.journal.flush_async();
        }

        let destination_timezone = d.trip.destination_timezone();
        let money = profile_of(ctx).money();
        let receiver_service_hours = (elapsed_hours - hours).max(0.0);
        let receiver_service_clause = if receiver_service_hours >= 0.05 {
            format!(
                " Receiver service after check-in took {} hours.",
                fmt_f(receiver_service_hours, 1)
            )
        } else {
            String::new()
        };
        let accessorial_clause = if is_owner_operator(&business_status) {
            "These came off this settlement."
        } else {
            "Billed to the carrier, not deducted from driver pay."
        };
        self.summary_parts.insert(
            0,
            format!(
                "Delivered {} tons of {} to {} in {} hours, {}.{receiver_service_clause} {}. {} {} dollars. \
                 Charges {}: tolls {}, accessorials {}. {accessorial_clause} Business status: {}. Costs {}. \
                 Fines {}. Net {}, you have {}. Parked at {}.",
                fmt_f(job.weight_tons, 0),
                job.cargo.label,
                job.spoken_destination(),
                fmt_f(hours, 1),
                if on_time { "on time" } else { "late" },
                clock_text(to_local(game_hours, destination_timezone)),
                pay_label(&business_status),
                fmt_grouped(gross_pay, 0),
                fmt_grouped(carrier_charges, 0),
                fmt_grouped(toll_expense, 0),
                charge_summary(&accessorials),
                business.status_label,
                fmt_grouped(business.business_charge_total(), 0),
                fmt_grouped(driver_charges, 0),
                fmt_grouped(net_pay, 0),
                fmt_grouped(money, 0),
                self.terminal.name,
            ),
        );
        if on_time_bonus_paid >= 1.0 {
            self.summary_parts.push(format!(
                "On-time delivery bonus: {} dollars.",
                fmt_grouped(on_time_bonus_paid, 0)
            ));
        }
        if early_bonus >= 1.0 {
            self.summary_parts.push(format!(
                "Early delivery bonus: {} dollars.",
                fmt_grouped(early_bonus, 0)
            ));
        }
        if let Some(damage_line) = damage_summary_line(&ctx.settings, &d.trip.truck, trip_damage) {
            self.summary_parts.push(damage_line);
        }
        let mut wear_parts: Vec<String> = Vec::new();
        for (added, meter) in [
            (
                (d.trip.truck.tire_wear_pct - d.start_tire_wear).max(0.0),
                "tire wear",
            ),
            (
                (d.trip.truck.brake_wear_pct - d.start_brake_wear).max(0.0),
                "brake wear",
            ),
            (
                (d.trip.truck.engine_wear_pct - d.start_engine_wear).max(0.0),
                "engine wear",
            ),
            (road_grime_added, "road grime"),
        ] {
            if added >= 0.1 {
                wear_parts.push(format!("{} percent {meter}", fmt_f(added, 1)));
            }
        }
        if !wear_parts.is_empty() {
            let joined = if wear_parts.len() > 1 {
                format!(
                    "{}, and {}",
                    wear_parts[..wear_parts.len() - 1].join(", "),
                    wear_parts[wear_parts.len() - 1]
                )
            } else {
                wear_parts[0].clone()
            };
            self.summary_parts.push(format!("The run added {joined}."));
        }
        self.summary_parts.extend(announcements.clone());
        award_arrival_achievements(
            self,
            ctx,
            d,
            on_time,
            trip_damage,
            toll_expense,
            d.route.miles(),
            d.speeding_tickets,
            gross_pay,
        );
        let achievement_summary = self.achievement_summary_line();
        if let Some(line) = achievement_summary.clone() {
            self.summary_parts.push(line);
        }
        let timing = if on_time { "On time" } else { "Late" };
        let mut bonus_lines: Vec<String> = Vec::new();
        if on_time_bonus_paid >= 1.0 {
            bonus_lines.push(format!(
                "On-time delivery bonus: {} dollars.",
                fmt_grouped(on_time_bonus_paid, 0)
            ));
        }
        if early_bonus >= 1.0 {
            bonus_lines.push(format!(
                "Early delivery bonus: {} dollars.",
                fmt_grouped(early_bonus, 0)
            ));
        }
        if bonus_lines.is_empty() {
            bonus_lines.push("No delivery bonus on this run.".to_string());
        }
        // Condition rows carry information only when something happened: a
        // run that added no damage, a near-full tank, and an undamaged truck
        // are the unremarkable default, and the review keys still reach the
        // full state on demand (research doc R10).
        let mut condition_lines: Vec<String> = Vec::new();
        if trip_damage > 1.0 {
            condition_lines.push(format!(
                "Truck damage added on this run: {} percent.",
                fmt_f(trip_damage, 0)
            ));
        }
        if d.trip.truck.fuel_fraction() < SETTLEMENT_LOW_FUEL_FRACTION {
            condition_lines.push(format!(
                "Fuel remaining: {} percent.",
                fmt_f(d.trip.truck.fuel_fraction() * 100.0, 0)
            ));
        }
        if d.trip.truck.damage_pct >= 1.0 {
            condition_lines.push(format!(
                "Truck damage now: {} percent.",
                fmt_f(d.trip.truck.damage_pct, 0)
            ));
        }
        // Achievements collapse to one row that names them; the full flavor
        // for each waits in the achievements menu and the message log, so the
        // settlement is not six paragraphs of comedy read at a parked truck
        // (research doc R9).
        let mut career_lines = announcements.clone();
        if let Some(line) = achievement_summary {
            career_lines.push(line);
        }
        let mut advance_lines: Vec<String> = Vec::new();
        if advance_repaid > 0.0 {
            advance_lines.push(format!(
                "Pay advance repaid: {} dollars.",
                fmt_grouped(advance_repaid, 0)
            ));
        }
        let pay_advance = profile_of(ctx).pay_advance;
        if pay_advance >= 1.0 {
            advance_lines.push(format!(
                "Pay advance still outstanding: {} dollars.",
                fmt_grouped(pay_advance, 0)
            ));
        }
        // This load's own driver-responsibility charges (a damage deductible,
        // a freight claim) are a different thing from a balance carried in
        // from an earlier load, and each has to be able to read zero without
        // the other's number standing in for it. The reviewable list used to
        // print the first under the second's label, so a settlement that
        // quietly took money off a balance announced "Fines carried over from
        // earlier loads: 0 dollars".
        let mut charge_lines: Vec<String> = Vec::new();
        if driver_charges >= 1.0 {
            charge_lines.push(format!(
                "Driver-responsibility charges this load: {} dollars.",
                fmt_grouped(driver_charges, 0)
            ));
        }
        if collected >= 1.0 {
            charge_lines.push(format!(
                "Balance owed: {} dollars of this settlement paid it down, leaving {} dollars owed.",
                fmt_grouped(collected, 0),
                fmt_grouped(profile_of(ctx).fines_owed, 0)
            ));
        }
        if charge_lines.is_empty() {
            charge_lines.push("No driver-responsibility charges and no balance owed.".to_string());
        }
        let business_cost_lines: Vec<String> = if business.business_charges.is_empty() {
            Vec::new()
        } else {
            vec![
                format!(
                    "Business costs: {} dollars.",
                    fmt_grouped(business.business_charge_total(), 0)
                ),
                format!(
                    "Business cost detail: {}.",
                    business.business_charge_summary()
                ),
            ]
        };
        let cities: Vec<String> = unique_consecutive(
            &d.route
                .cities
                .iter()
                .map(|c| ctx.world.spoken_city(c, None))
                .collect::<Vec<_>>(),
        );
        let mut lines = vec![
            format!(
                "Delivered {} tons of {} to {}.",
                fmt_f(job.weight_tons, 0),
                job.cargo.label,
                job.spoken_destination()
            ),
            format!(
                "Trip time: {} hours, {}.",
                fmt_f(hours, 1),
                timing.to_lowercase()
            ),
            format!(
                "It is {}.",
                clock_text(to_local(game_hours, destination_timezone))
            ),
            format!(
                "Parked at {} for {} service area.",
                self.terminal.name,
                ff_core::speech_text::the_city(job.spoken_destination())
            ),
            format!(
                "{}: {} dollars.",
                pay_label(&business_status),
                fmt_grouped(gross_pay, 0)
            ),
            format!(
                "Carrier-paid or reimbursed charges: {} dollars, including tolls {} and \
                 accessorials {}.",
                fmt_grouped(carrier_charges, 0),
                fmt_grouped(toll_expense, 0),
                charge_summary(&accessorials)
            ),
            format!("Business status: {}.", business.status_label),
        ];
        if receiver_service_hours >= 0.05 {
            lines.insert(
                2,
                format!(
                    "Receiver service after check-in: {} hours.",
                    fmt_f(receiver_service_hours, 1)
                ),
            );
        }
        lines.extend(business_cost_lines);
        lines.extend(charge_lines);
        lines.extend(advance_lines);
        lines.extend(self.team_line.clone());
        lines.push(format!(
            "Net driver pay: {} dollars.",
            fmt_grouped(net_pay, 0)
        ));
        lines.push(format!(
            "Money after settlement: {} dollars.",
            fmt_grouped(profile_of(ctx).money(), 0)
        ));
        lines.extend(bonus_lines);
        lines.push(format!("Route: {}.", cities.join(" to ")));
        lines.push(format!(
            "Distance credited: {}.",
            ctx.settings.distance_text(job.distance_mi, false)
        ));
        lines.extend(condition_lines);
        lines.extend(career_lines);
        self.summary_lines = lines;
        self.announcements = announcements;
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
