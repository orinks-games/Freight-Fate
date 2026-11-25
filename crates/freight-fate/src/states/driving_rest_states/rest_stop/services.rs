use super::*;

impl RestStopState {
    pub(super) fn repair(&mut self, ctx: &mut GameContext) {
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
                advance_on_duty_clock(d, ctx, 60.0, None, "");
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
                advance_on_duty_clock(d, ctx, 60.0, None, "");
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

    pub(super) fn roadside_assistance(&mut self, ctx: &mut GameContext) {
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
            advance_on_duty_clock(d, ctx, MECHANIC_WAIT_MIN, None, "");
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

    pub(super) fn tire_label(&self, ctx: &GameContext, d: &DrivingState) -> String {
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

    pub(super) fn service_wear(&mut self, ctx: &mut GameContext, meter: WearMeter) {
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
            advance_on_duty_clock(d, ctx, minutes, Some("on_duty_not_driving"), &duty_note);
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

    pub(super) fn buff_label(&self, buff: &Buff) -> String {
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
    pub(super) fn buy_buff(&mut self, ctx: &mut GameContext, buff_id: &str) {
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
            let no_lift = buff.stimulant && profile_of(ctx).stimulant_loan().is_open();
            if rig_buff {
                d.rig_buffs.insert(
                    buff.group.clone(),
                    RigBuff {
                        id: buff.id.clone(),
                        label: buff.label.clone(),
                        rate: buff.rate,
                    },
                );
                advance_on_duty_clock(
                    d,
                    ctx,
                    buff.stop_minutes,
                    Some("on_duty_not_driving"),
                    &buff.label.to_lowercase(),
                );
            } else {
                let now_h = d.absolute_game_hour(ctx, None);
                let expires_h = now_h + buff.duration_game_h;
                if buff.stimulant {
                    // The lift is a loan due as the drink wears off; its
                    // own worn-off line marks the payback.
                    profile_mut_of(ctx).take_stimulant(
                        buff.fatigue_instant,
                        now_h,
                        buff.duration_game_h,
                        &buff.worn_off_line(),
                    );
                } else if buff.fatigue_instant > 0.0 {
                    let p = profile_mut_of(ctx);
                    p.fatigue = (p.fatigue - buff.fatigue_instant).max(0.0);
                }
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
                if no_lift {
                    "No lift while the last stimulant lasts."
                } else {
                    &buff.purchased
                },
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

    pub(super) fn inspect(&mut self, ctx: &mut GameContext) {
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
    pub(super) fn check_in_pending(&self) -> bool {
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
    pub(super) fn check_in(&mut self, ctx: &mut GameContext) -> Option<(String, bool)> {
        let stop = self.stop.clone();
        let result = self.driving.clone().with(ctx, |d, ctx| {
            d.stop_visit(&stop).inspected = true;
            d.note_road_tally_scale(&stop);
            ctx.audio.play("ui/notify");
            // A valid decal is waved through on sight (CVSA Operational
            // Policy 5), unless the record is targeted.
            if d.decal_waves_through(ctx) {
                advance_on_duty_clock(d, ctx, WAVE_THROUGH_MIN, None, "");
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
                advance_on_duty_clock(d, ctx, WAVE_THROUGH_MIN, None, "");
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
            let out_of_service_before = d.out_of_service_count;
            let report = d.inspection_report(ctx, InspectionLevel::Full);
            let mut outcome = d.settle_inspection(ctx, &report);
            // The lane's own result first, then what the officer does about
            // a severely tired driver.
            let fatigue = d.officer_sees_fatigue(ctx, out_of_service_before);
            if !fatigue.is_empty() {
                outcome.push(' ');
                outcome.push_str(&fatigue);
            }
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
    pub(super) fn walk_around(&mut self, ctx: &mut GameContext) {
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            let lines = d.walk_around_lines(ctx);
            let minutes = d.hand_work_min(ctx, walk_around_minutes(d));
            let doubles = d.trip.truck.doubles_hooked();
            advance_on_duty_clock(
                d,
                ctx,
                minutes,
                Some("on_duty_not_driving"),
                "pre-trip walk-around",
            );
            // The doubles line names both trailers and the dolly once, at
            // its start, so a clean set needs no second list.
            let body = if lines.is_empty() && doubles {
                "Nothing to write up.".to_string()
            } else if lines.is_empty() {
                "Nothing to write up: tires, brakes, lights and the trailer would all pass."
                    .to_string()
            } else {
                lines.join(" ")
            };
            // One whole sentence per case: a fragment spliced into a shell
            // would go out in English whatever language the player reads.
            if doubles {
                format!(
                    "Walk-around done, both trailers and the converter dolly, {} minutes. \
                     {body} It is {}. {}",
                    fmt_f(minutes, 0),
                    clock_text(d.trip.local_hour()),
                    deadline_text(d, ctx)
                )
            } else {
                format!(
                    "Walk-around done, {} minutes. {body} It is {}. {}",
                    fmt_f(minutes, 0),
                    clock_text(d.trip.local_hour()),
                    deadline_text(d, ctx)
                )
            }
        }) else {
            return;
        };
        ctx.audio.play("ui/notify");
        ctx.say(&text);
        ctx.say_with(self.current_text(ctx), Say::queued().review(false));
    }
}
