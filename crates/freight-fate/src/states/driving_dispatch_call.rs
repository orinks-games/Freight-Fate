//! Offline dispatch calls from a stopped drive.

use chrono::Utc;
use ff_core::models::cargo_condition::{cargo_condition_text, cargo_outcome, CARGO_OUTCOME_CLEAN};
use ff_core::models::exchange::{stable_id, DispatchCallRequest, DispatchCallResponse, JsonObject};
use ff_core::settings::distance_text_for;
use ff_core::sim::hos::duration_text;
use std::sync::mpsc::TryRecvError;

use serde_json::{json, Value};

use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::remote_dispatch::{self, CallFacts, CallTask, WorkerEvent};
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{hos_of, profile_of, FIELD_REPAIR_DAMAGE_PCT};
use crate::states::driving_menu_states::DriveRef;
use crate::states::driving_pause_states::perform_roadside_mechanic;
use crate::states::online_states::load_identity;

pub const CALL_DELAY: &str = "delay";
pub const CALL_HOURS: &str = "hours";
pub const CALL_ROAD: &str = "road_conditions";
pub const CALL_TRUCK: &str = "truck_trouble";
pub const CALL_LOAD: &str = "load_trouble";
const FALLBACK_LINE: &str = "No dispatcher picked up, so the office is handling it.";

struct PendingCall {
    task: CallTask,
    request: DispatchCallRequest,
    local: DispatchCallResponse,
    facts: CallFacts,
    remaining_text: String,
    claim_announced: bool,
}

pub struct DispatchCallState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    pending: Option<PendingCall>,
}

impl DispatchCallState {
    pub fn new(driving: DriveRef) -> Self {
        Self {
            menu: MenuCore::new("Call dispatch")
                .with_intro_help("Choose what you need to report. Escape hangs up."),
            driving,
            pending: None,
        }
    }

    fn answer(&mut self, ctx: &mut GameContext, kind: &'static str) {
        if self.pending.is_some() {
            ctx.say("Still waiting for a dispatcher.");
            return;
        }
        let identity = ctx
            .online_enabled(ctx.settings.remote_dispatch_calls)
            .then(load_identity)
            .flatten();
        let Some(identity) = identity else {
            self.answer_locally(ctx, kind);
            return;
        };
        let prepared = self.driving.clone().call(self, ctx, |_s, ctx, driving| {
            let request = dispatch_request(driving, ctx, kind);
            let local = local_dispatch_response(&request, driving, ctx);
            let facts = call_facts(driving, ctx);
            let remaining_text =
                remaining_phrase(driving.trip.remaining_miles(), ctx.settings.imperial_units);
            Some((request, local, facts, remaining_text))
        });
        let Some(Some((request, local, facts, remaining_text))) = prepared else {
            return;
        };
        ctx.say("Calling dispatch.");
        match remote_dispatch::spawn(
            identity,
            request.request_id.clone(),
            kind.to_string(),
            facts,
        ) {
            Ok(task) => {
                self.pending = Some(PendingCall {
                    task,
                    request,
                    local,
                    facts,
                    remaining_text,
                    claim_announced: false,
                });
            }
            Err(_) => {
                ctx.say(FALLBACK_LINE);
                self.speak_response(ctx, &local, true);
            }
        }
    }

    fn answer_locally(&mut self, ctx: &mut GameContext, kind: &'static str) {
        let result = self.driving.clone().call(self, ctx, |_s, ctx, driving| {
            let request = dispatch_request(driving, ctx, kind);
            let response = local_dispatch_response(&request, driving, ctx);
            if kind == CALL_TRUCK && driving.trip.truck.damage_pct > FIELD_REPAIR_DAMAGE_PCT {
                let repair = perform_roadside_mechanic(driving, ctx)?;
                return Some(format!("Dispatch: {} {repair}", response.message));
            }
            Some(format!("Dispatch: {}", response.message))
        });
        if let Some(Some(answer)) = result {
            ctx.audio.play("ui/notify");
            ctx.say(&answer);
        }
    }

    fn update_worker(&mut self, ctx: &mut GameContext) {
        let mut received = Vec::new();
        let mut disconnected = false;
        if let Some(pending) = &self.pending {
            loop {
                match pending.task.events.try_recv() {
                    Ok(event) => received.push(event),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        disconnected = true;
                        break;
                    }
                }
            }
        }
        for event in received {
            match event {
                WorkerEvent::Claimed => {
                    if let Some(pending) = &mut self.pending {
                        if !pending.claim_announced {
                            pending.claim_announced = true;
                            ctx.say("A dispatcher picked up.");
                        }
                    }
                }
                WorkerEvent::Answered(decision) => {
                    if let Some(pending) = self.pending.take() {
                        let queued = pending.claim_announced;
                        let response = remote_dispatch_response(
                            &pending.request,
                            &pending.local,
                            pending.facts,
                            &pending.remaining_text,
                            &decision,
                        );
                        self.speak_response(ctx, &response, queued);
                    }
                }
                WorkerEvent::Failed => {
                    if let Some(pending) = self.pending.take() {
                        self.fallback(ctx, pending);
                    }
                }
                WorkerEvent::Cancelled => {
                    self.pending = None;
                }
            }
        }
        if disconnected {
            if let Some(pending) = self.pending.take() {
                self.fallback(ctx, pending);
            }
        }
    }

    fn fallback(&mut self, ctx: &mut GameContext, pending: PendingCall) {
        ctx.say(FALLBACK_LINE);
        self.speak_response(ctx, &pending.local, true);
    }

    fn speak_response(
        &mut self,
        ctx: &mut GameContext,
        response: &DispatchCallResponse,
        queued: bool,
    ) {
        let answer = self.driving.clone().call(self, ctx, |_s, ctx, driving| {
            if roadside_mechanic_needed(&response.decision, driving.trip.truck.damage_pct) {
                let repair = perform_roadside_mechanic(driving, ctx)?;
                return Some(format!("Dispatch: {} {repair}", response.message));
            }
            Some(format!("Dispatch: {}", response.message))
        });
        if let Some(Some(answer)) = answer {
            ctx.audio.play("ui/notify");
            if queued {
                ctx.say_with(&answer, Say::queued());
            } else {
                ctx.say(&answer);
            }
        }
    }
}

fn roadside_mechanic_needed(decision: &str, damage_pct: f64) -> bool {
    decision == "repair_authorized" && damage_pct > FIELD_REPAIR_DAMAGE_PCT
}

impl Menu for DispatchCallState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        self.update_worker(ctx);
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        if let Some(pending) = self.pending.take() {
            pending.task.cancel();
            ctx.say("Hung up.");
        }
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
    }

    fn exit(&mut self, _ctx: &mut GameContext) {
        if let Some(pending) = &self.pending {
            pending.task.cancel();
        }
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
            .help("Report truck damage. Dispatch can authorize the roadside mechanic."),
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
        CALL_DELAY => delay_response(driving, ctx.settings.imperial_units),
        CALL_HOURS => hours_response(ctx),
        CALL_ROAD => road_response(driving, ctx.settings.imperial_units),
        CALL_TRUCK => truck_response(driving),
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

fn call_facts(driving: &DrivingState, ctx: &GameContext) -> CallFacts {
    CallFacts {
        remaining_miles: driving.trip.remaining_miles(),
        hours_left: driving.job.deadline_game_h - driving.trip.game_minutes / 60.0,
        truck_damage_pct: driving.trip.truck.damage_pct,
        cargo_damage_pct: driving.trip.truck.cargo_damage_pct,
        hos_remaining_minutes: hos_of(ctx)
            .next_limit(&ctx.settings.hos_mode)
            .map(|limit| limit.remaining_min),
        weather_alerts: driving.trip.live_alerts.len(),
    }
}

fn caution_rank(kind: &str, decision: &str) -> Option<i16> {
    match (kind, decision) {
        ("delay", "continue")
        | ("hours", "plan_rest")
        | ("road_conditions", "continue")
        | ("truck_trouble", "monitor")
        | ("load_trouble", "continue") => Some(0),
        ("delay", "watch")
        | ("road_conditions", "caution")
        | ("truck_trouble", "repair_authorized")
        | ("load_trouble", "protect_load")
        | ("hours", "stop") => Some(1),
        ("delay", "late_update") => Some(2),
        _ => None,
    }
}

pub fn effective_decision(kind: &str, local: &str, remote: &str) -> Option<String> {
    let remote_rank = caution_rank(kind, remote)?;
    let local_rank = caution_rank(kind, local).unwrap_or(-1);
    Some(if remote_rank > local_rank {
        remote.to_string()
    } else {
        local.to_string()
    })
}

fn remote_message(
    request: &DispatchCallRequest,
    local: &DispatchCallResponse,
    facts: CallFacts,
    remaining: &str,
    effective: &str,
) -> String {
    if effective == local.decision {
        return local.message.clone();
    }
    match (request.kind.as_str(), effective) {
        ("delay", "watch") => format!(
            "The load is tight, with about {} before the appointment. Do not trade safety or \
             legal hours for it. Update me at the next stop.",
            duration_text(facts.hours_left * 60.0)
        ),
        ("delay", "late_update") => format!(
            "I'm marking this load as running late. Keep it safe and update me at the next stop; \
             {remaining}."
        ),
        ("hours", "stop") => match facts.hos_remaining_minutes {
            Some(minutes) => format!(
                "Park at the next safe spot and take your rest; your next limit is in about {}.",
                duration_text(minutes)
            ),
            None => "Park at the next safe spot and take your rest; hours enforcement is off in \
                     this driving mode."
                .to_string(),
        },
        ("road_conditions", "caution") => format!(
            "There's no active warning on my board, but take it easy out there; {remaining}."
        ),
        ("truck_trouble", "repair_authorized")
            if facts.truck_damage_pct <= FIELD_REPAIR_DAMAGE_PCT =>
        {
            "I can authorize a roadside repair, but the truck doesn't need one yet. Watch the \
             gauges and stop if it worsens."
                .to_string()
        }
        ("load_trouble", "protect_load") if facts.cargo_damage_pct < 1.0 => {
            "Slow the handling down and protect the freight through the next stop.".to_string()
        }
        _ => local.message.clone(),
    }
}

fn remote_dispatch_response(
    request: &DispatchCallRequest,
    local: &DispatchCallResponse,
    facts: CallFacts,
    remaining: &str,
    remote: &str,
) -> DispatchCallResponse {
    let effective = effective_decision(&request.kind, &local.decision, remote)
        .unwrap_or_else(|| local.decision.clone());
    let message = remote_message(request, local, facts, remaining, &effective);
    let effects = if effective == local.decision {
        local.effects.clone()
    } else {
        match (request.kind.as_str(), effective.as_str()) {
            ("delay", "watch" | "late_update") => {
                JsonObject::from_iter([("late_update_recorded".to_string(), json!(true))])
            }
            ("load_trouble", "protect_load") => {
                JsonObject::from_iter([("cargo_exception_recorded".to_string(), json!(true))])
            }
            ("truck_trouble", "repair_authorized")
                if facts.truck_damage_pct > FIELD_REPAIR_DAMAGE_PCT =>
            {
                JsonObject::from_iter([("roadside_repair_authorized".to_string(), json!(true))])
            }
            ("hours", "stop") | ("road_conditions", "caution") => local.effects.clone(),
            _ => JsonObject::new(),
        }
    };
    let responded_at = Utc::now().to_rfc3339();
    let response_id = stable_id(
        "dispatch_response",
        &[
            json!(request.request_id),
            json!("remote_dispatch"),
            json!(effective),
        ],
    )
    .unwrap_or_default();
    DispatchCallResponse {
        response_id,
        request_id: request.request_id.clone(),
        source_game: "remote_dispatch".to_string(),
        responder_id: "remote_dispatch".to_string(),
        decision: effective,
        responded_at,
        message,
        effects,
        ..DispatchCallResponse::default()
    }
}

fn request_summary(driving: &DrivingState, ctx: &GameContext, kind: &str) -> String {
    match kind {
        CALL_DELAY => {
            let used = hours_value(driving.trip.game_minutes / 60.0);
            let unit = if used == "1" { "hour" } else { "hours" };
            format!(
                "{used} {unit} used of a {} hour delivery window.",
                hours_value(driving.job.deadline_game_h)
            )
        }
        CALL_HOURS => hos_of(ctx).summary(&ctx.settings.hos_mode),
        CALL_ROAD => format!(
            "{}.",
            remaining_phrase(driving.trip.remaining_miles(), ctx.settings.imperial_units)
        ),
        CALL_TRUCK => format!(
            "Truck damage is {:.0} percent.",
            driving.trip.truck.damage_pct
        ),
        CALL_LOAD => format!(
            "Cargo damage is {:.0} percent.",
            driving.trip.truck.cargo_damage_pct
        ),
        _ => "Driver requested dispatch assistance.".to_string(),
    }
}

/// "312 kilometers remain", "1 mile remains": the distance left in the
/// player's unit, with one decimal only while the decimal still matters.
/// Every call line that says how far is left goes through this, so the
/// office and a remote dispatcher read the same distance the same way.
pub fn remaining_phrase(miles: f64, imperial: bool) -> String {
    let left = distance_text_for(miles, imperial, miles < 10.0);
    let verb = if left.ends_with('s') {
        "remain"
    } else {
        "remains"
    };
    format!("{left} {verb}")
}

/// Hours for a spoken readout: a whole number without a trailing ".0"
/// ("a 14 hour delivery window"), one decimal otherwise ("1.7 hours").
fn hours_value(hours: f64) -> String {
    let tenths = (hours * 10.0).round() / 10.0;
    if tenths.fract() == 0.0 {
        format!("{tenths:.0}")
    } else {
        format!("{tenths:.1}")
    }
}

fn delay_response(driving: &DrivingState, imperial: bool) -> (&'static str, String, JsonObject) {
    let hours_used = driving.trip.game_minutes / 60.0;
    let hours_left = driving.job.deadline_game_h - hours_used;
    let travel_hours = driving.trip.remaining_miles() / 50.0;
    let margin = hours_left - travel_hours;
    if hours_left <= 0.0 {
        return (
            "late_update",
            format!(
                "The appointment is already late. I am marking the delay. Keep it safe and send \
                 another update at the next stop; {}.",
                remaining_phrase(driving.trip.remaining_miles(), imperial)
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

fn road_response(driving: &DrivingState, imperial: bool) -> (&'static str, String, JsonObject) {
    let remaining = remaining_phrase(driving.trip.remaining_miles(), imperial);
    if driving.trip.live_alerts.is_empty() {
        return (
            "continue",
            format!(
                "I have no active weather warning on your current stretch. {remaining} on this \
                 route."
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
             {remaining}."
        ),
        JsonObject::from_iter([("weather_alerts".to_string(), json!(alerts))]),
    )
}

fn truck_response(driving: &DrivingState) -> (&'static str, String, JsonObject) {
    let damage = driving.trip.truck.damage_pct;
    if damage <= FIELD_REPAIR_DAMAGE_PCT {
        return (
            "monitor",
            format!(
                "I have the truck report at {damage:.0} percent damage. It does not need a \
                 roadside callout yet. Watch the gauges and stop if it worsens."
            ),
            JsonObject::new(),
        );
    }
    (
        "repair_authorized",
        "Roadside repair is authorized. Secure the truck while the mechanic works.".to_string(),
        JsonObject::from_iter([("roadside_repair_authorized".to_string(), json!(true))]),
    )
}

fn load_response(driving: &DrivingState) -> (&'static str, String, JsonObject) {
    let damage = driving.trip.truck.cargo_damage_pct;
    let words = cargo_condition_text(damage, driving.trip.truck.liquid.is_some());
    if damage < 1.0 {
        return (
            "continue",
            format!("The freight is {words}. Continue and protect it through the next stop."),
            JsonObject::new(),
        );
    }
    // The condition word and the number say one thing: under the exception
    // line the load is "shifted but sound", and calling that same load
    // "damaged" in the next breath contradicted it ("as shifted but sound,
    // 3 percent damaged"), while above it the word already says damaged.
    let report = if cargo_outcome(damage) == CARGO_OUTCOME_CLEAN {
        format!("I have the freight report as {words}, with only {damage:.0} percent cargo damage.")
    } else {
        format!("I have the freight report as {words}, with {damage:.0} percent cargo damage.")
    };
    (
        "protect_load",
        format!(
            "{report} Slow the handling down and avoid any hard stop or sharp bend you can \
             safely avoid."
        ),
        JsonObject::from_iter([("cargo_exception_recorded".to_string(), json!(true))]),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};

    use crate::playtest::harness::{PlaytestHarness, StartDelivery};
    use crate::remote_dispatch::WorkerEvent;
    use crate::states::driving_core::MECHANIC_WAIT_MIN;

    fn request(kind: &str) -> DispatchCallRequest {
        DispatchCallRequest {
            request_id: "request-1".to_string(),
            kind: kind.to_string(),
            ..DispatchCallRequest::default()
        }
    }

    fn response(decision: &str, message: &str) -> DispatchCallResponse {
        DispatchCallResponse {
            decision: decision.to_string(),
            message: message.to_string(),
            ..DispatchCallResponse::default()
        }
    }

    fn facts() -> CallFacts {
        CallFacts {
            remaining_miles: 80.0,
            hours_left: 2.0,
            truck_damage_pct: 0.0,
            cargo_damage_pct: 0.0,
            hos_remaining_minutes: Some(90.0),
            weather_alerts: 0,
        }
    }

    fn remote_repair_response(damage_pct: f64) -> DispatchCallResponse {
        let mut facts = facts();
        facts.truck_damage_pct = damage_pct;
        remote_dispatch_response(
            &request(CALL_TRUCK),
            &response("monitor", "Local answer."),
            facts,
            "80 miles remain",
            "repair_authorized",
        )
    }

    #[test]
    fn remaining_phrase_uses_the_unit_whole_numbers_and_agreement() {
        assert_eq!(remaining_phrase(194.1, false), "312 kilometers remain");
        assert_eq!(remaining_phrase(194.1, true), "194 miles remain");
        assert_eq!(remaining_phrase(1.0, true), "1.0 mile remains");
        assert_eq!(remaining_phrase(4.24, true), "4.2 miles remain");
        assert_eq!(remaining_phrase(10.4, true), "10 miles remain");
    }

    #[test]
    fn remote_decisions_use_caution_order_and_escalated_lines() {
        assert_eq!(
            effective_decision("delay", "watch", "continue"),
            Some("watch".to_string())
        );
        let local = remote_dispatch_response(
            &request(CALL_DELAY),
            &response("watch", "Local watch."),
            facts(),
            "80 miles remain",
            "continue",
        );
        assert_eq!(local.decision, "watch");
        assert_eq!(local.message, "Local watch.");
        let remote = remote_dispatch_response(
            &request(CALL_DELAY),
            &response("continue", "local continue"),
            facts(),
            "80 miles remain",
            "late_update",
        );
        assert_eq!(
            remote.message,
            "I'm marking this load as running late. Keep it safe and update me at the next stop; \
             80 miles remain."
        );
        assert_eq!(remote.decision, "late_update");
        assert_eq!(remote.responder_id, "remote_dispatch");
        assert_eq!(remote.source_game, "remote_dispatch");
        assert_eq!(
            remote.effects.get("late_update_recorded"),
            Some(&json!(true))
        );

        let watch = remote_dispatch_response(
            &request(CALL_DELAY),
            &response("continue", "local continue"),
            facts(),
            "80 miles remain",
            "watch",
        );
        assert_eq!(
            watch.effects.get("late_update_recorded"),
            Some(&json!(true))
        );

        let caution = remote_dispatch_response(
            &request(CALL_ROAD),
            &response("continue", "local continue"),
            facts(),
            "80 miles remain",
            "caution",
        );
        assert_eq!(
            caution.message,
            "There's no active warning on my board, but take it easy out there; 80 miles remain."
        );
        assert!(caution.effects.is_empty());
    }

    #[test]
    fn legal_stop_cannot_be_downgraded_and_repair_threshold_is_preserved() {
        assert_eq!(
            effective_decision("hours", "stop", "plan_rest"),
            Some("stop".to_string())
        );
        let stop = remote_dispatch_response(
            &request(CALL_HOURS),
            &response("stop", "You are out of legal time. Park safely now."),
            facts(),
            "80 miles remain",
            "plan_rest",
        );
        assert_eq!(stop.decision, "stop");
        assert_eq!(stop.message, "You are out of legal time. Park safely now.");

        let mut local_hours = response("plan_rest", "Local rest plan.");
        local_hours.effects = JsonObject::from_iter([
            ("limit_kind".to_string(), json!("driving")),
            ("remaining_minutes".to_string(), json!(12.0)),
        ]);
        let escalated_stop = remote_dispatch_response(
            &request(CALL_HOURS),
            &local_hours,
            facts(),
            "80 miles remain",
            "stop",
        );
        assert_eq!(escalated_stop.decision, "stop");
        assert_eq!(escalated_stop.effects, local_hours.effects);

        assert!(!roadside_mechanic_needed(
            "repair_authorized",
            FIELD_REPAIR_DAMAGE_PCT
        ));
        assert!(roadside_mechanic_needed(
            "repair_authorized",
            FIELD_REPAIR_DAMAGE_PCT + 1.0
        ));

        let mut truck_facts = facts();
        truck_facts.truck_damage_pct = FIELD_REPAIR_DAMAGE_PCT;
        let truck = remote_dispatch_response(
            &request(CALL_TRUCK),
            &response("monitor", "local monitor"),
            truck_facts,
            "80 miles remain",
            "repair_authorized",
        );
        assert_eq!(
            truck.message,
            "I can authorize a roadside repair, but the truck doesn't need one yet. Watch the \
             gauges and stop if it worsens."
        );
        assert!(truck.effects.is_empty());
        assert!(!roadside_mechanic_needed(
            &truck.decision,
            truck_facts.truck_damage_pct
        ));
        let authorized = remote_repair_response(FIELD_REPAIR_DAMAGE_PCT + 1.0);
        assert_eq!(
            authorized.effects.get("roadside_repair_authorized"),
            Some(&json!(true))
        );

        let mut load_facts = facts();
        load_facts.cargo_damage_pct = 0.5;
        let load = remote_dispatch_response(
            &request(CALL_LOAD),
            &response("continue", "local continue"),
            load_facts,
            "80 miles remain",
            "protect_load",
        );
        assert_eq!(
            load.message,
            "Slow the handling down and protect the freight through the next stop."
        );
        assert_eq!(
            load.effects.get("cargo_exception_recorded"),
            Some(&json!(true))
        );
    }

    #[test]
    fn remote_repair_below_threshold_skips_cost_and_above_threshold_runs() {
        let mut harness = PlaytestHarness::new();
        harness.start_delivery(StartDelivery::default());
        let driving = harness.shared_driving().unwrap();
        let mut state = DispatchCallState::new(DriveRef::of(&driving));

        harness.with_drive(|driving, _| {
            driving.trip.truck.damage_pct = FIELD_REPAIR_DAMAGE_PCT;
            driving.truck_mut().velocity_mps = 0.0;
        });
        let before = harness.with_drive(|driving, ctx| {
            (
                driving.trip.truck.damage_pct,
                driving.trip.game_minutes,
                profile_of(ctx).money(),
            )
        });
        let response = remote_repair_response(FIELD_REPAIR_DAMAGE_PCT);
        harness.clear_speech();
        state.speak_response(&mut harness.app.ctx, &response, true);
        let after = harness.with_drive(|driving, ctx| {
            (
                driving.trip.truck.damage_pct,
                driving.trip.game_minutes,
                profile_of(ctx).money(),
            )
        });
        assert_eq!(after, before);
        assert!(harness
            .transcript()
            .join(" ")
            .contains("I can authorize a roadside repair, but the truck doesn't need one yet."));

        harness.with_drive(|driving, _| {
            driving.trip.truck.damage_pct = FIELD_REPAIR_DAMAGE_PCT + 20.0;
            driving.truck_mut().velocity_mps = 0.0;
        });
        let before = harness.with_drive(|driving, _| driving.trip.game_minutes);
        let response = remote_repair_response(FIELD_REPAIR_DAMAGE_PCT + 20.0);
        harness.clear_speech();
        state.speak_response(&mut harness.app.ctx, &response, true);
        let after = harness
            .with_drive(|driving, _| (driving.trip.truck.damage_pct, driving.trip.game_minutes));
        assert_eq!(after.0, FIELD_REPAIR_DAMAGE_PCT);
        assert_eq!(after.1, before + MECHANIC_WAIT_MIN);
        assert!(harness
            .transcript()
            .join(" ")
            .contains("A mobile mechanic patched the truck"));
    }

    fn pending(task: CallTask, local: DispatchCallResponse) -> PendingCall {
        PendingCall {
            task,
            request: request(CALL_DELAY),
            local,
            facts: facts(),
            remaining_text: "80 miles remain".to_string(),
            claim_announced: false,
        }
    }

    #[test]
    fn a_failed_worker_speaks_fallback_before_the_local_answer() {
        let mut harness = PlaytestHarness::new();
        harness.start_delivery(StartDelivery::default());
        harness.clear_speech();
        let (sender, receiver) = mpsc::sync_channel(2);
        sender.send(WorkerEvent::Failed).unwrap();
        drop(sender);
        let task = CallTask::from_parts(receiver, Arc::new(AtomicBool::new(false)));
        let mut state = DispatchCallState::new(DriveRef::of(&harness.shared_driving().unwrap()));
        state.pending = Some(pending(task, response("continue", "Local answer.")));
        state.update_worker(&mut harness.app.ctx);
        assert_eq!(
            harness.transcript(),
            vec![
                FALLBACK_LINE.to_string(),
                "Dispatch: Local answer.".to_string()
            ]
        );
    }

    #[test]
    fn a_claimed_call_is_announced_once_before_its_answer() {
        let mut harness = PlaytestHarness::new();
        harness.start_delivery(StartDelivery::default());
        harness.clear_speech();
        let (sender, receiver) = mpsc::sync_channel(4);
        sender.send(WorkerEvent::Claimed).unwrap();
        sender
            .send(WorkerEvent::Answered("continue".to_string()))
            .unwrap();
        drop(sender);
        let task = CallTask::from_parts(receiver, Arc::new(AtomicBool::new(false)));
        let mut state = DispatchCallState::new(DriveRef::of(&harness.shared_driving().unwrap()));
        state.pending = Some(pending(task, response("continue", "Local answer.")));
        state.update_worker(&mut harness.app.ctx);
        assert_eq!(
            harness.transcript(),
            vec![
                "A dispatcher picked up.".to_string(),
                "Dispatch: Local answer.".to_string()
            ]
        );
    }

    #[test]
    fn escape_cancels_and_does_not_speak_an_answer() {
        let mut harness = PlaytestHarness::new();
        harness.clear_speech();
        let (_sender, receiver) = mpsc::sync_channel(1);
        let cancelled = Arc::new(AtomicBool::new(false));
        let task = CallTask::from_parts(receiver, Arc::clone(&cancelled));
        let mut state = DispatchCallState::new(DriveRef::empty());
        state.pending = Some(pending(task, response("continue", "Do not speak this.")));
        Menu::go_back(&mut state, &mut harness.app.ctx);
        assert!(cancelled.load(Ordering::SeqCst));
        assert_eq!(harness.transcript(), vec!["Hung up."]);
    }

    #[test]
    fn another_call_while_pending_says_still_waiting() {
        let mut harness = PlaytestHarness::new();
        let (_sender, receiver) = mpsc::sync_channel(1);
        let task = CallTask::from_parts(receiver, Arc::new(AtomicBool::new(false)));
        let mut state = DispatchCallState::new(DriveRef::empty());
        state.pending = Some(pending(task, response("continue", "Do not speak this.")));

        state.answer(&mut harness.app.ctx, CALL_HOURS);

        assert_eq!(
            harness.transcript(),
            vec!["Still waiting for a dispatcher."]
        );
    }
}
