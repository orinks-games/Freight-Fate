use super::*;

impl ArrivalState {
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
                "Pay for driving empty: {} dollars. You now have {} dollars. ",
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
                "Drove without a trailer to {} in {} hours. It is {}. {pay_clause}Parked at {}. \
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
            "Claim against your own business"
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
                "Receiver took the load but noted damage, {} at {} percent. Claim about {} \
                 dollars, {} off this settlement. {claim_holder}.",
                cargo_condition_text(cargo.condition_pct, liquid),
                fmt_f(cargo.condition_pct, 0),
                fmt_grouped(cargo.claim_value, 0),
                fmt_grouped(cargo.pay_loss, 0)
            );
        }
        format!(
            "The receiver noted damage on the paperwork: load arrived {} at {} percent. {} dollars held \
             back.",
            cargo_condition_text(cargo.condition_pct, liquid),
            fmt_f(cargo.condition_pct, 0),
            fmt_grouped(cargo.pay_loss, 0)
        )
    }

    pub(super) fn settle(
        &mut self,
        ctx: &mut GameContext,
        d: &mut DrivingState,
        arrival_hours: f64,
    ) {
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
        let on_time = hours <= job.deadline_game_h;
        // A rest call traded the bonus for the moved appointment: the load
        // pays its base rate, with no on-time bonus, and no late penalty for
        // the hours the call moved it by. Past the moved appointment it is
        // late like any other load.
        let base_rate = d.rest_call == RestCall::Used;
        let gross_base = if base_rate {
            job.payout(hours, trip_damage, 0.0)
        } else {
            job.payout_default(hours, trip_damage)
        };
        // Whether the pay plan's on-time share is paid.
        let pay_on_time = on_time && !base_rate;
        let toll_expense = d.trip.toll_expense();
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
            pay_on_time,
            driver_charges,
            &terms,
        );
        let no_on_time_bonus_business = ff_core::models::business::build_business_settlement(
            &business_status,
            &job,
            job.payout(hours, trip_damage, 0.0),
            pay_on_time,
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
                    "Manual transmission bonus: {} dollars.",
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
            conditions: d
                .road_tally
                .conditions(crate::states::driving_updates::road_tally::career_inspections(ctx)),
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
                "Delivered {} tons of {} to {} in {} hours, {}.{receiver_service_clause} {}. {}: {} dollars. \
                 Charges {}: tolls {}, extra charges {}. {accessorial_clause} Business status: {}. Costs {}. \
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
        // Bare names: the route reads as one chain, and a state on only the
        // one city whose name is shared elsewhere sounded like a mistake.
        let cities: Vec<String> = unique_consecutive(
            &d.route
                .cities
                .iter()
                .map(|c| ctx.world.spoken_city(c, Some(false)))
                .collect::<Vec<_>>(),
        );
        let mut lines = vec![
            // The first row is the one read on arrival: where, whether on
            // time, and what it paid. The rows below itemize it.
            // The screen's title already says "Delivery complete".
            if on_time {
                format!(
                    "On time in {}. Net {} dollars.",
                    job.spoken_destination(),
                    fmt_grouped(net_pay, 0)
                )
            } else {
                format!(
                    "Late in {}. Net {} dollars.",
                    job.spoken_destination(),
                    fmt_grouped(net_pay, 0)
                )
            },
            format!(
                "Load: {} tons of {}.",
                fmt_f(job.weight_tons, 0),
                job.cargo.label
            ),
            format!("Trip time: {} hours.", fmt_f(hours, 1)),
            format!(
                "It is {}.",
                clock_text(to_local(game_hours, destination_timezone))
            ),
            {
                let city = job.spoken_destination();
                let town = city.split(", ").next().unwrap_or(city);
                if self.terminal.name.contains(town) {
                    format!("Parked at {}.", self.terminal.name)
                } else {
                    format!("Parked at {} in {city}.", self.terminal.name)
                }
            },
            format!(
                "{}: {} dollars.",
                pay_label(&business_status),
                fmt_grouped(gross_pay, 0)
            ),
            format!("Business status: {}.", business.status_label),
        ];
        // Each carrier-ledger item on its own row, by what it is: detention
        // is money paid for waiting, not a negative charge, and a zero toll
        // is nothing to read.
        let mut ledger_lines: Vec<String> = Vec::new();
        if toll_expense >= 1.0 {
            ledger_lines.push(format!("Tolls: {} dollars.", fmt_grouped(toll_expense, 0)));
        }
        for charge in &accessorials {
            if charge.amount.abs() >= 1.0 {
                let mut label = charge.label.chars();
                let label = label
                    .next()
                    .map(|c| c.to_uppercase().chain(label).collect::<String>())
                    .unwrap_or_default();
                ledger_lines.push(format!(
                    "{label}: {} dollars.",
                    fmt_grouped(charge.amount.abs(), 0)
                ));
            }
        }
        let status_row = lines.len() - 1;
        lines.splice(status_row..status_row, ledger_lines);
        if receiver_service_hours >= 0.05 {
            lines.insert(
                2,
                format!(
                    "Unloading took {} minutes.",
                    fmt_f(receiver_service_hours * 60.0, 0)
                ),
            );
        }
        lines.extend(business_cost_lines);
        lines.extend(charge_lines);
        lines.extend(advance_lines);
        lines.extend(self.team_line.clone());
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
}
