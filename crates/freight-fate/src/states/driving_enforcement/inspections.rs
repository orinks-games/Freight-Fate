//! Roadside inspections in the game: the scale-house lane, the routine Level
//! 3 stop a trooper runs on a legal driver, the repairs an out-of-service item
//! forces on the shoulder, the decal, and the driver's own walk-around. What
//! an inspector finds and what it costs is `ff_core::sim::roadside_inspection`;
//! this file is where the truck is parked while it happens.

use ff_core::models::safety_record::{refresh_selection_score, safety_band, BAND_TARGETED};
use ff_core::pyfmt::{fmt_f, fmt_grouped};
use ff_core::sim::enforcement_observe::WHAT_DAMAGE;
use ff_core::sim::hos;
use ff_core::sim::roadside_inspection::{
    decal_valid, inspect, roadcheck_blitz, roadside_inspection_scale, walk_around, InspectionInput,
    InspectionLevel, InspectionReport, Repair, DAMAGE_FINDING, DECAL_VALID_HOURS,
};

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;
use crate::states::driving_rest_states::record_hours;

/// How often a routine inspection stops this driver, from the safety record,
/// the hours mode and the calendar: `(scale, roadcheck week)`. Zero when
/// there is no career to inspect.
pub fn roadside_inspection_scale_for(ctx: &mut GameContext, damage_pct: f64) -> (f64, bool) {
    let relaxed = ctx.settings.hos_mode == "relaxed";
    let Some(profile) = ctx.profile.as_mut() else {
        return (0.0, false);
    };
    let score = refresh_selection_score(profile, damage_pct);
    let blitz = roadcheck_blitz(profile.player_calendar_hours());
    (
        roadside_inspection_scale(safety_band(score), relaxed, blitz),
        blitz,
    )
}

impl DrivingState {
    /// Re-read the odds after anything that moves the record.
    pub fn refresh_roadside_inspection_scale(&mut self, ctx: &mut GameContext) {
        let (scale, blitz) = roadside_inspection_scale_for(ctx, self.trip.truck.damage_pct);
        self.trip.roadside_inspection_scale = scale;
        self.trip.roadcheck_blitz = blitz;
    }

    /// Whether the decal on the windshield waves this truck past the lane: a
    /// valid sticker and a record that is not targeted.
    pub fn decal_waves_through(&self, ctx: &mut GameContext) -> bool {
        let damage = self.trip.truck.damage_pct;
        let hours = record_hours(ctx, self);
        let Some(profile) = ctx.profile.as_mut() else {
            return false;
        };
        if !decal_valid(profile.driving_record.decal_until_h, hours) {
            return false;
        }
        safety_band(refresh_selection_score(profile, damage)) != BAND_TARGETED
    }

    /// What the officer finds on this truck and this driver at `level`.
    pub fn inspection_report(&self, ctx: &GameContext, level: InspectionLevel) -> InspectionReport {
        let mode = ctx.settings.hos_mode.clone();
        let over_hours = !hos::HOS_NON_ENFORCED_MODES.contains(&mode.as_str())
            && hos_of(ctx).in_violation(&mode);
        let trailer = self.citable_trailer_defect(ctx);
        let mut report = inspect(&InspectionInput {
            level,
            tire_wear_pct: self.trip.truck.tire_wear_pct,
            brake_wear_pct: self.trip.truck.brake_wear_pct,
            damage_pct: self.trip.truck.damage_pct,
            trailer_defect: trailer.as_deref(),
            over_hours,
        });
        // A fault already on this drive's paperwork is not written again: the
        // driver can't fix a lamp or a worn tire between shops, and the report
        // in the cab says the carrier knows (49 CFR 396.9(d) gives it fifteen
        // days to certify the repair). Anything that parks the truck still
        // counts, because that gets fixed where it stands.
        report
            .findings
            .retain(|f| f.out_of_service || !self.written_up.contains(&f.what));
        report
    }

    /// Settle the report where the truck stands: the clock, the money, the
    /// record, any out-of-service repair, the decal. Returns the spoken
    /// outcome.
    pub fn settle_inspection(
        &mut self,
        ctx: &mut GameContext,
        report: &InspectionReport,
    ) -> String {
        let level = report.level.spoken();
        let minutes = report.minutes();
        advance_rest_clock(
            self,
            ctx,
            minutes,
            Some("on_duty_not_driving"),
            "roadside inspection",
        );
        hos_mut_of(ctx).on_duty(minutes);
        let mut text = String::new();
        if report.clean() && !self.written_up.is_empty() {
            // Only the faults already on this drive's paperwork: nothing new
            // to cite, and no decal for a truck with a write-up open.
            text.push_str(&format!(
                "{level}, {} minutes. Nothing new to write up.",
                fmt_f(minutes, 0)
            ));
            self.refresh_roadside_inspection_scale(ctx);
            return text;
        }
        if report.clean() {
            record_inspection(ctx);
            text.push_str(&format!("Clean {level}, {} minutes.", fmt_f(minutes, 0)));
            if report.earns_decal() {
                let until = record_hours(ctx, self) + DECAL_VALID_HOURS;
                profile_mut_of(ctx).driving_record.decal_until_h = until;
                text.push_str(
                    " The officer puts an inspection decal on the windshield: for the next three \
                     months an open scale waves you through unless your record is targeted.",
                );
            }
            self.refresh_roadside_inspection_scale(ctx);
            return text;
        }
        text.push_str(&format!(
            "{level}, {} minutes. Written up for {}.",
            fmt_f(minutes, 0),
            report.spoken_findings()
        ));
        for finding in report.findings.iter().filter(|f| !f.out_of_service) {
            self.written_up.insert(finding.what.clone());
            if finding.what == DAMAGE_FINDING {
                // The same fault a patrol reads as unsafe equipment.
                self.written_up.insert(WHAT_DAMAGE.to_string());
            }
        }
        // A new driver is warned for what the truck can still run with; an
        // item that parks it is fined at any level.
        let warned = self.new_driver_warning(ctx);
        let fine = if warned {
            report
                .findings
                .iter()
                .filter(|f| f.out_of_service)
                .map(|f| f.fine)
                .sum()
        } else {
            report.total_fine()
        };
        if warned && fine <= 0.0 {
            text.push_str(" The officer lets it go with a warning.");
        }
        if fine > 0.0 {
            // One citation for the inspection, at the sum of its items: the
            // record counts stops, not lines on a form.
            self.ticket_fines_paid += fine;
            {
                let p = profile_mut_of(ctx);
                p.spend(fine);
                p.career.reputation = (p.career.reputation - hos::HOS_REPUTATION_HIT).max(0.0);
            }
            let reason = format!("roadside inspection: {}", report.spoken_findings());
            let ladder = self.log_enforcement(ctx, fine, false, false, &reason);
            text.push_str(&format!(
                " Fined {} dollars, and the citation goes on your record.",
                fmt_grouped(fine, 0)
            ));
            if !ladder.is_empty() {
                text.push(' ');
                text.push_str(&ladder);
            }
        }
        for repair in report.repairs() {
            let line = self.roadside_out_of_service_repair(ctx, repair);
            text.push(' ');
            text.push_str(&line);
        }
        if report
            .findings
            .iter()
            .any(|f| f.out_of_service && f.repair == Repair::None)
        {
            // The hours order is served in time, right here.
            let mode = ctx.settings.hos_mode.clone();
            let oos_minutes = hos_of(ctx).out_of_service_minutes(&mode);
            self.place_out_of_service_minutes(ctx, oos_minutes);
            let served = if oos_minutes < hos::SLEEP_MIN {
                "thirty minutes"
            } else {
                "ten hours"
            };
            text.push_str(&format!(
                " Out of service for the hours violation: {served} parked right here."
            ));
        }
        self.refresh_roadside_inspection_scale(ctx);
        text
    }

    /// An out-of-service item: the truck does not move until it is fixed, so
    /// the roadside mechanic comes to it, at road-shop prices.
    fn roadside_out_of_service_repair(&mut self, ctx: &mut GameContext, repair: Repair) -> String {
        let carrier_paid = !player_pays_operating_costs(&profile_of(ctx).business_status);
        let (what, cost) = match repair {
            Repair::Tires => {
                let wear = self.trip.truck.tire_wear_pct;
                self.trip.truck.tire_wear_pct = 0.0;
                (
                    "fitted new tires",
                    (wear * ROAD_TIRE_COST_PER_PCT).max(ROAD_TIRE_MIN),
                )
            }
            Repair::Brakes => {
                let wear = self.trip.truck.brake_wear_pct;
                self.trip.truck.brake_wear_pct = 0.0;
                (
                    "adjusted and relined the brakes",
                    (wear * ROAD_BRAKE_COST_PER_PCT).max(ROAD_BRAKE_MIN),
                )
            }
            Repair::Damage => {
                let damage = self.trip.truck.damage_pct;
                self.trip.truck.damage_pct = damage.min(FIELD_REPAIR_DAMAGE_PCT);
                (
                    "patched the damage",
                    road_repair_cost(damage, FIELD_REPAIR_DAMAGE_PCT, MECHANIC_CALLOUT_FEE),
                )
            }
            Repair::Trailer => {
                self.trailer_repaired = true;
                ("fixed the trailer's defect", MECHANIC_CALLOUT_FEE)
            }
            Repair::None => return String::new(),
        };
        if !carrier_paid {
            profile_mut_of(ctx).spend(cost);
        }
        advance_rest_clock(
            self,
            ctx,
            MECHANIC_WAIT_MIN,
            Some("on_duty_not_driving"),
            "out-of-service repair",
        );
        hos_mut_of(ctx).on_duty(MECHANIC_WAIT_MIN);
        let billing = if carrier_paid {
            "on the carrier breakdown account".to_string()
        } else {
            format!("for {} dollars", fmt_grouped(cost, 0))
        };
        format!(
            "Out of service until repaired: a roadside mechanic {what} {billing}, {} minutes.",
            fmt_f(MECHANIC_WAIT_MIN, 0)
        )
    }

    /// The driver's own pre-trip: the same items an inspector reads, as
    /// plain lines. Empty means nothing to write up.
    pub fn walk_around_lines(&self, ctx: &GameContext) -> Vec<String> {
        let trailer = self.hooked_trailer_defect(ctx);
        walk_around(
            self.trip.truck.tire_wear_pct,
            self.trip.truck.brake_wear_pct,
            self.trip.truck.damage_pct,
            trailer.as_deref(),
        )
    }

    /// A trooper pulls in behind a legal driver: lights, signal, shoulder,
    /// and a Level 3 once the truck is stopped.
    pub fn begin_routine_inspection(&mut self, ctx: &mut GameContext) {
        let blitz = if self.trip.roadcheck_blitz {
            " It is Roadcheck week and every inspector is out."
        } else {
            ""
        };
        let summary = format!(
            "Routine roadside inspection, Level 3: license, medical card, logbook and the \
             load's paperwork.{blitz}"
        );
        let lights = format!(
            "Lights behind you for a routine inspection. Signal with {} and stop on the \
             shoulder.",
            ctx.control_hint("take_exit")
        );
        self.begin_enforcement_pull_over(
            ctx,
            "roadside_inspection",
            "Roadside inspection",
            &summary,
            0.0,
            0.0,
            "Back on the highway.",
            &lights,
        );
    }
}
