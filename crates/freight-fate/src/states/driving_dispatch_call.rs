//! Offline dispatch calls from a stopped drive.

use chrono::Utc;
use ff_core::models::cargo_condition::cargo_condition_text;
use ff_core::models::exchange::{stable_id, DispatchCallRequest, DispatchCallResponse, JsonObject};
use ff_core::pyfmt::fmt_grouped;
use ff_core::sim::hos::duration_text;
use serde_json::{json, Value};

use crate::app::GameContext;
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{
    advance_rest_clock, clock_text, hos_mut_of, hos_of, player_pays_operating_costs,
    profile_mut_of, profile_of, FIELD_REPAIR_DAMAGE_PCT, MECHANIC_CALLOUT_FEE, MECHANIC_WAIT_MIN,
    ROAD_CHAIN_SET_COST,
};
use crate::states::driving_menu_states::DriveRef;
use crate::states::driving_pause_states::perform_roadside_mechanic;

pub const CALL_DELAY: &str = "delay";
pub const CALL_HOURS: &str = "hours";
pub const CALL_ROAD: &str = "road_conditions";
pub const CALL_TRUCK: &str = "truck_trouble";
pub const CALL_LOAD: &str = "load_trouble";

pub struct DispatchCallState {
    menu: MenuCore<Self>,
    driving: DriveRef,
}

impl DispatchCallState {
    pub fn new(driving: DriveRef) -> Self {
        Self {
            menu: MenuCore::new("Call dispatch")
                .with_intro_help("Choose what you need to report. Escape hangs up."),
            driving,
        }
    }

    fn answer(&mut self, ctx: &mut GameContext, kind: &'static str) {
        let result = self.driving.clone().call(self, ctx, |_s, ctx, driving| {
            let request = dispatch_request(driving, ctx, kind);
            let response = local_dispatch_response(&request, driving, ctx);
            if kind != CALL_TRUCK {
                return Some(format!("Dispatch: {}", response.message));
            }
            let mut answer = format!("Dispatch: {}", response.message);
            if driving.trip.truck.damage_pct > FIELD_REPAIR_DAMAGE_PCT {
                let repair = perform_roadside_mechanic(driving, ctx)?;
                answer.push(' ');
                answer.push_str(&repair);
            }
            if visible_trailer_defect(driving, ctx).is_some() {
                answer.push(' ');
                answer.push_str(&repair_trailer_on_the_road(driving, ctx));
            }
            if chainless_in_chain_law(driving, ctx) {
                answer.push(' ');
                answer.push_str(&bring_chains_on_the_road(driving, ctx));
            }
            Some(answer)
        });
        if let Some(Some(answer)) = result {
            ctx.audio.play("ui/notify");
            ctx.say(&answer);
        }
    }
}

impl Menu for DispatchCallState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new("Report a delay", |s: &mut Self, ctx| {
                s.answer(ctx, CALL_DELAY)
            })
            .help("Ask dispatch whether the load still has time or needs a late update."),
            MenuItem::new("Ask about hours", |s: &mut Self, ctx| {
                s.answer(ctx, CALL_HOURS)
            })
            .help("Ask what legal limit comes next and how soon."),
            MenuItem::new("Ask about the road ahead", |s: &mut Self, ctx| {
                s.answer(ctx, CALL_ROAD)
            })
            .help("Ask about remaining distance and active weather warnings."),
            MenuItem::new("Report truck trouble", |s: &mut Self, ctx| {
                s.answer(ctx, CALL_TRUCK)
            })
            .help("Report damage, a trailer fault, or missing chains. Dispatch can send road service."),
            MenuItem::new("Report load trouble", |s: &mut Self, ctx| {
                s.answer(ctx, CALL_LOAD)
            })
            .help("Report the freight's current condition and ask how to proceed."),
        ]
    }
}

impl_state_for_menu!(DispatchCallState);

pub fn dispatch_request(
    driving: &DrivingState,
    ctx: &GameContext,
    kind: &str,
) -> DispatchCallRequest {
    let profile = profile_of(ctx);
    let departure_hour = profile.game_hours - driving.trip.game_minutes / 60.0;
    let contract_id = stable_id(
        "contract",
        &[
            json!("freight_fate"),
            json!(profile.name),
            json!(driving.job.origin),
            json!(driving.job.destination),
            json!(driving.job.cargo.key),
            json!(departure_hour),
        ],
    )
    .unwrap_or_default();
    let leg_id = stable_id(
        "leg",
        &[
            json!(contract_id),
            json!(driving.phase),
            json!(driving.trip.current_leg_index()),
        ],
    )
    .unwrap_or_default();
    let now = Utc::now().to_rfc3339();
    let request_id = stable_id(
        "dispatch_request",
        &[json!(contract_id), json!(leg_id), json!(kind), json!(now)],
    )
    .unwrap_or_default();
    let summary = request_summary(driving, ctx, kind);
    let context = JsonObject::from_iter([
        (
            "remaining_miles".to_string(),
            json!(driving.trip.remaining_miles()),
        ),
        (
            "elapsed_hours".to_string(),
            json!(driving.trip.game_minutes / 60.0),
        ),
        (
            "deadline_hours".to_string(),
            json!(driving.job.deadline_game_h),
        ),
        (
            "truck_damage_pct".to_string(),
            json!(driving.trip.truck.damage_pct),
        ),
        (
            "cargo_damage_pct".to_string(),
            json!(driving.trip.truck.cargo_damage_pct),
        ),
        (
            "active_weather_alerts".to_string(),
            Value::Array(
                driving
                    .trip
                    .live_alerts
                    .iter()
                    .map(|alert| Value::String(alert.spoken()))
                    .collect(),
            ),
        ),
    ]);
    DispatchCallRequest {
        request_id,
        contract_id,
        leg_id,
        source_game: "freight_fate".to_string(),
        driver_id: profile.name.clone(),
        kind: kind.to_string(),
        created_at: now,
        summary,
        context,
        ..DispatchCallRequest::default()
    }
}

pub fn local_dispatch_response(
    request: &DispatchCallRequest,
    driving: &DrivingState,
    ctx: &GameContext,
) -> DispatchCallResponse {
    let (decision, message, effects) = match request.kind.as_str() {
        CALL_DELAY => delay_response(driving),
        CALL_HOURS => hours_response(ctx),
        CALL_ROAD => road_response(driving),
        CALL_TRUCK => truck_response(driving, ctx),
        CALL_LOAD => load_response(driving),
        _ => (
            "acknowledged",
            "I have the update. Continue safely and call again if the situation changes."
                .to_string(),
            JsonObject::new(),
        ),
    };
    let responded_at = Utc::now().to_rfc3339();
    let response_id = stable_id(
        "dispatch_response",
        &[json!(request.request_id), json!("local"), json!(decision)],
    )
    .unwrap_or_default();
    DispatchCallResponse {
        response_id,
        request_id: request.request_id.clone(),
        source_game: "freight_fate".to_string(),
        responder_id: "local_dispatch".to_string(),
        decision: decision.to_string(),
        responded_at,
        message,
        effects,
        ..DispatchCallResponse::default()
    }
}

fn request_summary(driving: &DrivingState, ctx: &GameContext, kind: &str) -> String {
    match kind {
        CALL_DELAY => format!(
            "{} hours used of a {} hour delivery window.",
            driving.trip.game_minutes / 60.0,
            driving.job.deadline_game_h
        ),
        CALL_HOURS => hos_of(ctx).summary(&ctx.settings.hos_mode),
        CALL_ROAD => format!("{} miles remain.", driving.trip.remaining_miles()),
        CALL_TRUCK => {
            let mut summary = format!("Truck damage is {} percent.", driving.trip.truck.damage_pct);
            if let Some(defect) = visible_trailer_defect(driving, ctx) {
                summary.push_str(&format!(" Trailer: {defect}."));
            }
            if chainless_in_chain_law(driving, ctx) {
                summary.push_str(" No usable chains under an active chain law.");
            }
            summary
        }
        CALL_LOAD => format!(
            "Cargo condition is {} percent.",
            driving.trip.truck.cargo_damage_pct
        ),
        _ => "Driver requested dispatch assistance.".to_string(),
    }
}

fn delay_response(driving: &DrivingState) -> (&'static str, String, JsonObject) {
    let hours_used = driving.trip.game_minutes / 60.0;
    let hours_left = driving.job.deadline_game_h - hours_used;
    let travel_hours = driving.trip.remaining_miles() / 50.0;
    let margin = hours_left - travel_hours;
    if hours_left <= 0.0 {
        return (
            "late_update",
            format!(
                "The appointment is already late. I am marking the delay. Keep it safe and send \
                 another update at the next stop; {} remain.",
                driving.trip.gap_text(driving.trip.remaining_miles())
            ),
            JsonObject::from_iter([("late_update_recorded".to_string(), json!(true))]),
        );
    }
    if margin < 1.0 {
        return (
            "watch",
            format!(
                "The load is tight, with about {} before the appointment. Do not trade safety or \
                 legal hours for it. Update me at the next stop.",
                duration_text(hours_left * 60.0)
            ),
            JsonObject::from_iter([("late_update_recorded".to_string(), json!(true))]),
        );
    }
    (
        "continue",
        format!(
            "You still have about {} before the appointment. Continue safely and call again if \
             that margin changes.",
            duration_text(hours_left * 60.0)
        ),
        JsonObject::new(),
    )
}

fn hours_response(ctx: &GameContext) -> (&'static str, String, JsonObject) {
    let Some(limit) = hos_of(ctx).next_limit(&ctx.settings.hos_mode) else {
        return (
            "information",
            "Hours enforcement is off in this driving mode. Plan the rest you need for fatigue."
                .to_string(),
            JsonObject::new(),
        );
    };
    let message = if limit.remaining_min <= 0.0 {
        format!("You are out of legal time. Park safely now; {}.", limit.due)
    } else {
        format!(
            "Your next limit is in about {}: {}.",
            duration_text(limit.remaining_min),
            limit.due
        )
    };
    (
        if limit.remaining_min <= 0.0 {
            "stop"
        } else {
            "plan_rest"
        },
        message,
        JsonObject::from_iter([
            ("limit_kind".to_string(), json!(limit.kind)),
            ("remaining_minutes".to_string(), json!(limit.remaining_min)),
        ]),
    )
}

fn road_response(driving: &DrivingState) -> (&'static str, String, JsonObject) {
    let remaining = driving.trip.gap_text(driving.trip.remaining_miles());
    if driving.trip.live_alerts.is_empty() {
        return (
            "continue",
            format!(
                "I have no active weather warning on your current stretch. {remaining} remain on \
                 this route."
            ),
            JsonObject::new(),
        );
    }
    let alerts = driving
        .trip
        .live_alerts
        .iter()
        .map(|alert| alert.spoken())
        .collect::<Vec<_>>()
        .join(", ");
    (
        "caution",
        format!(
            "The active warning is {alerts}. Stay with the planned route unless the road closes; \
             {remaining} remain."
        ),
        JsonObject::from_iter([("weather_alerts".to_string(), json!(alerts))]),
    )
}

/// The trailer fault the driver can see from the ground and report: a lamp or
/// a tire, not a brake adjustment, which takes someone under the trailer.
fn visible_trailer_defect(driving: &DrivingState, ctx: &GameContext) -> Option<String> {
    driving
        .hooked_trailer_defect(ctx)
        .filter(|defect| !defect.contains("brake"))
}

/// Under an active chain law here with no set that can go on.
fn chainless_in_chain_law(driving: &DrivingState, ctx: &GameContext) -> bool {
    let here = driving.trip.position_mi;
    driving.trip.chain_law_area_at(here).is_some()
        && driving.trip.chain_law_level() > 0
        && !driving.trip.rig_compliant_at(here)
        && !driving.usable_chains_aboard(ctx)
}

/// Dispatch sends the roadside mechanic to the trailer: the fault is fixed
/// where the truck is parked, so no later officer finds it.
fn repair_trailer_on_the_road(driving: &mut DrivingState, ctx: &mut GameContext) -> String {
    let carrier_paid = !player_pays_operating_costs(&profile_of(ctx).business_status);
    if !carrier_paid {
        profile_mut_of(ctx).spend(MECHANIC_CALLOUT_FEE);
    }
    driving.trailer_repaired = true;
    advance_rest_clock(
        driving,
        ctx,
        MECHANIC_WAIT_MIN,
        Some("on_duty_not_driving"),
        "trailer repair",
    );
    hos_mut_of(ctx).on_duty(MECHANIC_WAIT_MIN);
    let billing = if carrier_paid {
        "on the carrier breakdown account".to_string()
    } else {
        format!("for {} dollars", fmt_grouped(MECHANIC_CALLOUT_FEE, 0))
    };
    format!(
        "A roadside mechanic fixed the trailer {billing}. It took an hour and a half. It is {}.",
        clock_text(driving.trip.local_hour())
    )
}

/// Road service brings a new set of chains to where the truck is parked: the
/// set at the truck-stop price plus the callout, and the same wait as a
/// mechanic. Like the mechanic, it comes whether or not the cash covers it.
fn bring_chains_on_the_road(driving: &mut DrivingState, ctx: &mut GameContext) -> String {
    let carrier_paid = !player_pays_operating_costs(&profile_of(ctx).business_status);
    let cost = ROAD_CHAIN_SET_COST + MECHANIC_CALLOUT_FEE;
    if !carrier_paid {
        profile_mut_of(ctx).spend(cost);
    }
    driving.stow_new_chain_set(ctx);
    advance_rest_clock(
        driving,
        ctx,
        MECHANIC_WAIT_MIN,
        Some("on_duty_not_driving"),
        "chain delivery",
    );
    hos_mut_of(ctx).on_duty(MECHANIC_WAIT_MIN);
    let billing = if carrier_paid {
        "on the carrier breakdown account".to_string()
    } else {
        format!(
            "for {} dollars. You have {} dollars",
            fmt_grouped(cost, 0),
            fmt_grouped(profile_of(ctx).money(), 0)
        )
    };
    format!(
        "Road service brought a new set of snow chains {billing}. It took an hour and a half. \
         It is {}. Put them on from the pause menu.",
        clock_text(driving.trip.local_hour())
    )
}

fn truck_response(driving: &DrivingState, ctx: &GameContext) -> (&'static str, String, JsonObject) {
    let damage = driving.trip.truck.damage_pct;
    let trailer = visible_trailer_defect(driving, ctx);
    let chainless = chainless_in_chain_law(driving, ctx);
    if damage > FIELD_REPAIR_DAMAGE_PCT {
        return (
            "repair_authorized",
            "Roadside repair is authorized. Secure the truck while the mechanic works.".to_string(),
            JsonObject::from_iter([("roadside_repair_authorized".to_string(), json!(true))]),
        );
    }
    if let Some(defect) = trailer {
        return (
            "repair_authorized",
            format!(
                "I have the {defect}. I'm sending a mechanic out to fix it where you're parked."
            ),
            JsonObject::from_iter([("trailer_repair_authorized".to_string(), json!(true))]),
        );
    }
    if chainless {
        return (
            "repair_authorized",
            "You have no chains that can go on, and the chain law is up. I'm sending road \
             service out with a new set."
                .to_string(),
            JsonObject::from_iter([("chain_delivery_authorized".to_string(), json!(true))]),
        );
    }
    (
        "monitor",
        format!(
            "I have the truck report at {damage:.0} percent damage. It does not need a \
             roadside callout yet. Watch the gauges and stop if it worsens."
        ),
        JsonObject::new(),
    )
}

fn load_response(driving: &DrivingState) -> (&'static str, String, JsonObject) {
    let condition = driving.trip.truck.cargo_damage_pct;
    let words = cargo_condition_text(condition, driving.trip.truck.liquid.is_some());
    if condition < 1.0 {
        return (
            "continue",
            format!("The freight is {words}. Continue and protect it through the next stop."),
            JsonObject::new(),
        );
    }
    (
        "protect_load",
        format!(
            "I have the freight report as {words}, {condition:.0} percent. Slow the handling down \
             and avoid any hard stop or sharp bend you can safely avoid."
        ),
        JsonObject::from_iter([("cargo_exception_recorded".to_string(), json!(true))]),
    )
}
