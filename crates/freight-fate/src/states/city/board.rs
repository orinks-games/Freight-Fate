//! The dispatch board and its per-job detail reader (`JobBoardState`,
//! `JobDetailState`).

use std::collections::HashSet;

use serde_json::Value;

use ff_core::models::business::{
    build_business_settlement, has_weigh_station_transponder, is_owner_operator,
    BusinessSettlement, SettlementTerms, INDEPENDENT_AUTHORITY,
};
use ff_core::models::career_objectives::career_objective;
use ff_core::models::career_training::{
    is_company_training_profile, training_guidance, training_recommendation_score, TrainingStage,
};
use ff_core::models::carrier_fleet::{
    assignment_reason_text, equipment_held_back, equipment_hold_text, slip_seats,
};
use ff_core::models::dispatch_policy::{
    declines_remaining, dispatch_policy, DECLINE_REPUTATION_PENALTY, SENIOR_LOAD_CHOICE_LEVEL,
};
use ff_core::models::enforcement;
use ff_core::models::jobs::{lane_key, route_drive_hours, Job};
use ff_core::models::profile::Profile;
use ff_core::models::trailers::{
    compatible_with_programs, owned_trailer_for_cargo, required_program_text,
};
use ff_core::playtest_levers::forced_dispatch_destination;
use ff_core::pyfmt::{fmt_f, fmt_grouped};
use ff_core::sim::hos::limits;
use ff_core::sim::real_weather_alerts::{alerts_chain_law_level, warm_route_alerts};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::{rig_must_chain, ChainCorridorAhead, CHAIN_LAW_LEAD_MI};
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::{effects, WeatherSystem};

use crate::app::{GameContext, Say, SharedState};
use crate::impl_state_for_menu;
use crate::meaningful_play::MeaningfulPlayReason;
use crate::states::base::{InputEvent, Key, Menu, MenuCore, MenuItem};
use crate::states::city::{
    base_menu_handle_event, first_dispatch_done, held_load, home_terminal, launch_driving, profile,
    profile_mut, sleeps_needed, DrivingLaunch, LaunchAnnouncement, DRIVE_PHASE_DELIVERY,
    DRIVE_PHASE_PICKUP, PICKUP_CHECK_IN_MIN, PICKUP_LOADING_MIN,
};
use crate::states::city_pickup::warm_route_feeds;
use crate::states::city_pickup::{
    pickup_snapshot, PickupFacilityState, PickupOptions, PickupSnapshotOptions,
};

pub(crate) mod briefing;
mod details;
mod weight;
pub use details::{describe_job, JobDetailState};

/// The board's class-level `intro_help` (the browsable board; an assigned
/// board swaps in its own on construction).
pub const JOB_BOARD_INTRO_HELP: &str =
    "Enter accepts. F1 is job details. Tab is the market. Escape goes back.";

const ASSIGNED_INTRO_HELP: &str =
    "Assigned load. Enter accepts. Decline costs dispatch trust until your next promotion. F1 \
     is details. Escape goes back.";

/// How long dispatch keeps assigning loads: a new hire waits for level 8, but
/// a driver at level 8 or above is only here because standing slipped.
fn assignment_ends(p: &Profile) -> String {
    if p.career.level() < SENIOR_LOAD_CHOICE_LEVEL {
        format!("until level {SENIOR_LOAD_CHOICE_LEVEL}")
    } else {
        "until your standing recovers".to_string()
    }
}

// -- shared job wording ----------------------------------------------------------------

fn settlement_for(p: &Profile, job: &Job, with_reputation: bool) -> BusinessSettlement {
    let owned: Vec<String> = p.visible_owned_trailers();
    let owned_refs: Vec<&str> = owned.iter().map(String::as_str).collect();
    build_business_settlement(
        &p.business_status,
        job,
        job.pay,
        true,
        0.0,
        &SettlementTerms {
            carrier_key: Some(&p.carrier_key),
            owned_trailers: &owned_refs,
            reputation: if with_reputation {
                Some(p.standing())
            } else {
                None
            },
            transponder: has_weigh_station_transponder(p),
            record_surcharge: enforcement::record_insurance_surcharge(p),
        },
    )
}

/// Minutes of shift the hours warning refuses to plan away.
///
/// The route estimate already runs slower than the posted limits
/// (`DEADLINE_PLANNING_SPEED_FACTOR`), but a plan that lands on the last
/// minute of the window still loses to rain, town limits and a slow ramp.
/// Half an hour is the smallest unit a dispatcher plans in (the break
/// requirement is 30 minutes), so a load that fits with less than that to
/// spare is called what it is: one that needs a rest first.
pub const HOS_FIT_CUSHION_MIN: f64 = 30.0;

/// Whether the job's origin facility is the yard the truck is parked in.
///
/// The home terminal is a real facility in the city's list (a template
/// company yard where the data has none), so the board can hand out loads
/// that ship from it. Name and city both have to match: a same-named yard
/// in another city is a drive away.
pub fn job_origin_is_this_yard(ctx: &GameContext, job: &Job, terminal_name: &str) -> bool {
    if job.bobtail {
        return false;
    }
    let here = ctx.world.resolve_city_key(&profile(ctx).current_city);
    ctx.world.resolve_city_key(&job.origin) == here
        && job.origin_location.trim() == terminal_name.trim()
}

/// Driving hours from the board to the shipper's dock: nothing for a load
/// from this yard, else the facility approach, after the corridor to the
/// origin city when dispatch relayed the load from a nearby one.
fn pickup_drive_hours(ctx: &GameContext, job: &Job) -> f64 {
    if job_origin_is_this_yard(ctx, job, &home_terminal(ctx).name) {
        return 0.0;
    }
    let here = ctx.world.resolve_city_key(&profile(ctx).current_city);
    let origin = ctx.world.resolve_city_key(&job.origin);
    let corridor_h = if origin != here {
        ctx.world
            .supported_route(&here, &origin, None)
            .ok()
            .flatten()
            .map(|corridor| route_drive_hours(Some(&corridor), 0.0, Some(ctx.world)))
            .unwrap_or(0.0)
    } else {
        0.0
    };
    corridor_h
        + ctx
            .world
            .facility_approach_route_with(
                &job.origin,
                &job.origin_location,
                ctx.settings.facility_streets,
            )
            .map(|approach| route_drive_hours(Some(&approach), 0.0, Some(ctx.world)))
            .unwrap_or(0.0)
}

/// `JobBoardState._locked_reason`: why this driver cannot take the job, or "".
pub fn locked_reason(p: &Profile, job: &Job) -> String {
    let endorsements: Vec<&str> = p.career.endorsements().into_iter().collect();
    let programs = p.active_trailer_programs();
    let program_refs: Vec<&str> = programs.iter().map(String::as_str).collect();
    job.locked_reason(
        &endorsements,
        p.career.level(),
        Some(&program_refs),
        !is_owner_operator(&p.business_status),
    )
}

/// `JobBoardState._trailer_note`.
pub fn trailer_note(p: &Profile, job: &Job) -> String {
    if !is_owner_operator(&p.business_status) {
        return String::new();
    }
    let cargo_key = job.cargo.key;
    if p.business_status == INDEPENDENT_AUTHORITY {
        if let Some(owned) = owned_trailer_for_cargo(cargo_key, p.visible_owned_trailers()) {
            return format!("Owned trailer: {}.", owned.label);
        }
        if compatible_with_programs(cargo_key, p.active_trailer_programs()) {
            return format!("Trailer program: {}.", required_program_text(cargo_key));
        }
        return format!(
            "Needs {} trailer program or owned trailer.",
            required_program_text(cargo_key)
        );
    }
    if compatible_with_programs(cargo_key, p.active_trailer_programs()) {
        return format!("Trailer program: {}.", required_program_text(cargo_key));
    }
    format!(
        "Needs {} trailer program.",
        required_program_text(cargo_key)
    )
}

/// What the listed amounts on the board mean for this business status:
/// the help on a job's pay line (F1 on it in the job details).
pub(super) fn pay_basis_note(status: &str) -> &'static str {
    if status == INDEPENDENT_AUTHORITY {
        "Listed amounts are direct freight gross. Insurance, compliance, trailer, truck, and \
         factoring costs come out at settlement."
    } else if is_owner_operator(status) {
        "Listed amounts are owner-operator gross revenue. Trailer program needs are listed on \
         each job."
    } else {
        "Listed amounts are carrier gross, your settlement pays driver wages. Routing is \
         assigned until you run your own truck."
    }
}

fn market_preview(business: &BusinessSettlement) -> String {
    let charges = business.business_charge_total();
    if charges > 0.0 {
        return format!(
            "Take-home about {} dollars after {} dollars costs.",
            fmt_grouped(business.net_before_advance, 0),
            fmt_grouped(charges, 0)
        );
    }
    format!(
        "Your pay about {} dollars.",
        fmt_grouped(business.net_before_advance, 0)
    )
}

/// Stop planning's no-chains reminder, while the driver is still at the
/// terminal and its garage is open, with no usable chains aboard: a chain
/// law in effect right now on a chain area of the loaded route that chains
/// this rig (`posted_chain_law_on_route`), or a carry-rule corridor in
/// season (`Trip::chain_carry_corridors`). Speech only; a second Enter
/// accepts as usual.
fn chain_reminder_before_accepting(ctx: &mut GameContext, job: &Job) -> Option<String> {
    {
        let p = profile(ctx);
        if !ctx.settings.hos_planning_hints
            || job.bobtail
            || (p.chains_owned() && p.chain_wear_pct() < 100.0)
        {
            return None;
        }
    }
    let route = ctx
        .world
        .supported_route(&job.origin, &job.destination, None)
        .ok()??;
    // The sky here is never the offline forecast: that draw is unseeded
    // until the drive starts, so it would name a law the road won't post.
    let trip = Trip::new(
        route,
        TruckState::default(),
        WeatherSystem::new("", Some(1), None, None, false),
        TripOptions {
            seed: Some(1),
            career_hours: Some(profile(ctx).game_hours),
            world: Some(ctx.world),
            ..Default::default()
        },
    );
    if let Some(ahead) = posted_chain_law_on_route(ctx, &trip) {
        let highway = ahead.highway;
        return Some(format!(
            "A chain law is in effect on {highway} on this route. You have no chains. The \
             garage here sells them. Enter again to accept."
        ));
    }
    let ahead = trip.chain_carry_corridors().into_iter().next()?;
    let highway = ahead.highway;
    Some(format!(
        "Trucks on {highway} must carry chains this season. You have no chains. The garage \
         here sells them. Enter again to accept."
    ))
}

/// The first chain area on `trip`'s route where a chain law is in effect
/// now and chains this rig (`rig_must_chain`). The level is read from the
/// live sources only: the Weather Service alerts cached for the cities at
/// either end of the area's leg (`warm_route_alerts`, the same cache
/// dispatch reads at the pickup), and with live weather on, those cities'
/// observed snow or ice. With live sources off there is nothing to read,
/// so the board reminds about carry-rule corridors only.
fn posted_chain_law_on_route(ctx: &mut GameContext, trip: &Trip) -> Option<ChainCorridorAhead> {
    let alerts = ctx.weather_alerts_provider_arc();
    if let Some(alerts) = &alerts {
        warm_route_alerts(&trip.route, alerts, ctx.world);
    }
    let world = ctx.world;
    let cities = trip.route.cities.clone();
    let mut sky_level = |key: &str| -> i64 {
        let Some(city) = world.cities.get(key) else {
            return 0;
        };
        let alert_level = alerts
            .as_ref()
            .and_then(|a| a.get(city.lat, city.lon))
            .map_or(0, |found| alerts_chain_law_level(&found));
        let (lat, lon) = (city.lat, city.lon);
        let observed = ctx.real_weather_provider().and_then(|provider| {
            // Keyed as the pickup's route weather is (`city_pickup`).
            provider.request(key, lat, lon);
            provider.get(key)
        });
        let weather_level = match observed.map(|kind| effects(kind).surface) {
            Some("ice") => 2,
            Some("snow") => 1,
            _ => 0,
        };
        alert_level.max(weather_level)
    };
    let areas = trip.chain_corridor_areas(0.0, trip.route.miles(), |_, _| true);
    for area in areas {
        let (leg, _) = trip.leg_at_mile(area.start_mi + CHAIN_LAW_LEAD_MI);
        let level = cities
            .get(leg..=leg + 1)
            .unwrap_or_default()
            .iter()
            .map(|key| sky_level(key))
            .max()
            .unwrap_or(0);
        if level > 0 && rig_must_chain(Some(&area.state), level) {
            return Some(area);
        }
    }
    None
}

// -- JobBoardState ------------------------------------------------------------------------

pub struct JobBoardState {
    menu: MenuCore<Self>,
    pub jobs: Vec<Job>,
    /// Index of the job whose hours warning was already heard once.
    confirm_risky_job: Option<usize>,
    /// Index of the job whose no-chains reminder was already heard once.
    confirm_chain_job: Option<usize>,
    session_declined: HashSet<usize>,
    assigned_queue: Vec<usize>,
}

impl JobBoardState {
    pub fn new(ctx: &GameContext, jobs: Vec<Job>) -> Self {
        let mut state = JobBoardState {
            menu: MenuCore::new("Dispatch board").with_intro_help(JOB_BOARD_INTRO_HELP),
            jobs,
            confirm_risky_job: None,
            confirm_chain_job: None,
            session_declined: HashSet::new(),
            assigned_queue: Vec::new(),
        };
        if dispatch_policy(profile(ctx)).assigns_load {
            state.assigned_queue = state.assignment_queue(ctx);
        }
        if state.assigned_mode() {
            state.menu.intro_help = ASSIGNED_INTRO_HELP.to_string();
        } else if let Some(held) = held_load::held_index(profile(ctx)) {
            // The load the driver went to sleep for is where they left it.
            state.menu.index = held.min(state.jobs.len().saturating_sub(1));
        } else if let Some(recommended) = state.recommended_job_index(ctx) {
            if state.recommendation_label(ctx).is_some() {
                state.menu.index = recommended;
            }
        }
        state
    }

    /// Dispatch picks the load: new company hires get an assignment,
    /// not a browsable board. Falls back to browsing when nothing on the
    /// board is unlocked, so the player can still hear what is there.
    pub fn assigned_mode(&self) -> bool {
        !self.assigned_queue.is_empty()
    }

    pub fn intro_help(&self) -> &str {
        &self.menu.intro_help
    }

    /// `_assigned_job`: the load dispatch is offering right now.
    pub fn assigned_job(&self) -> &Job {
        &self.jobs[self.assigned_queue[0]]
    }

    /// Whether the load dispatch is offering is staged in the home yard --
    /// the case `accept` answers with the shipping office in place of a
    /// deadhead. For the test rigs whose subject is the deadhead itself.
    pub fn assigned_load_is_staged_here(&self, ctx: &GameContext) -> bool {
        job_origin_is_this_yard(ctx, self.assigned_job(), &home_terminal(ctx).name)
    }

    /// `_assigned_queue`: the order dispatch will offer the board in.
    pub fn assigned_queue(&self) -> &[usize] {
        &self.assigned_queue
    }

    fn describe(&self, ctx: &GameContext, job: &Job, index: Option<usize>) -> String {
        describe_job(ctx, self.jobs.len(), job, index)
    }

    fn announce_assignment(&mut self, ctx: &mut GameContext) {
        let p = profile(ctx);
        let remaining = declines_remaining(p);
        // How assignment works, and what listed amounts mean, are on the
        // rows' help and the job details: the entry line is only what is
        // news today.
        let decline_note = if self.assigned_queue.len() < 2 {
            String::new()
        } else if remaining > 0 {
            format!(
                "{remaining} decline{} left before your next promotion. ",
                if remaining != 1 { "s" } else { "" }
            )
        } else {
            "No declines left until your next promotion. ".to_string()
        };
        let hos_note = if self.job_exceeds_current_hos(ctx, self.assigned_job()) {
            "This assignment needs an extra rest on your current hours. "
        } else {
            ""
        };
        let market = p.market.headline();
        let market = if market.is_empty() {
            market
        } else {
            format!("{market} ")
        };
        let current = self.current_text(ctx);
        ctx.say(&format!(
            "Dispatch board. {decline_note}{hos_note}{market}{current}"
        ));
    }

    fn build_assignment_items(&mut self, ctx: &GameContext) -> Vec<MenuItem<Self>> {
        let index = self.assigned_queue[0];
        let job = &self.jobs[index];
        let assignment_help = if job.bobtail {
            "Dispatch assigned this reposition, an empty deadhead with no pickup facility. \
             Route inspection after accepting covers rest, fuel, toll, weather, and \
             restrictions."
        } else {
            "Dispatch assigned this load. Accepting starts the deadhead from your terminal to \
             its origin facility. Route inspection after pickup covers rest, fuel, toll, \
             weather, and restrictions."
        };
        let mut items = vec![MenuItem::new(
            format!(
                "Accept assigned dispatch: {}",
                self.describe(ctx, job, None)
            ),
            move |s: &mut Self, ctx| s.accept(ctx, index),
        )
        .help(assignment_help)];
        let remaining = declines_remaining(profile(ctx));
        if self.assigned_queue.len() > 1 && remaining > 0 {
            items.push(
                MenuItem::new(
                    format!(
                        "Decline and request another load: \
                         {remaining} decline{} left",
                        if remaining != 1 { "s" } else { "" }
                    ),
                    |s: &mut Self, ctx| s.decline_assignment(ctx),
                )
                .help(
                    "Dispatch draws another load. Costs reputation from a budget that refills \
                     at the next level.",
                ),
            );
        }
        if self.assigned_queue.len() > 1 {
            items.push(
                MenuItem::new("Review the rest of today's board", |s: &mut Self, ctx| {
                    s.review_locked_board(ctx)
                })
                .help(format!(
                    "The other loads dispatch posted today. Assigned loads only {}.",
                    assignment_ends(profile(ctx))
                )),
            );
        }
        items.push(MenuItem::new("Back to terminal", |s: &mut Self, ctx| {
            s.go_back(ctx)
        }));
        items
    }

    /// Speak the pool behind the assignment, as flavor (owner design,
    /// 2026-07-24): the offer pool widens with every level, and growth the
    /// driver cannot hear is not a reward yet. On demand, never automatic.
    pub fn review_locked_board(&mut self, ctx: &mut GameContext) {
        let lines: Vec<String> = self.assigned_queue[1..]
            .iter()
            .map(|&i| &self.jobs[i])
            .map(|job| {
                if job.bobtail {
                    format!(
                        "a reposition to {}, \
                         {}, no cargo",
                        job.spoken_destination(),
                        ctx.settings.distance_text(job.distance_mi, false)
                    )
                } else {
                    format!(
                        "{} tons of {} to \
                         {}, {}",
                        fmt_f(job.weight_tons, 0),
                        job.cargo.label,
                        job.spoken_destination(),
                        ctx.settings.distance_text(job.distance_mi, false)
                    )
                }
            })
            .collect();
        ctx.say(&format!("Also posted today: {}.", lines.join("; ")));
    }

    /// Board indices dispatch already re-drew past, remembered with the
    /// cached board so leaving and reopening does not re-offer them.
    fn declined_indices(&self, ctx: &GameContext) -> HashSet<usize> {
        let mut declined = self.session_declined.clone();
        if let Some(cache) = profile(ctx)
            .dispatch_board_cache
            .as_ref()
            .and_then(Value::as_object)
        {
            if let Some(list) = cache.get("declined").and_then(Value::as_array) {
                for index in list {
                    if let Some(n) = index.as_f64() {
                        declined.insert(n as usize);
                    }
                }
            }
        }
        declined
    }

    fn remember_decline(&mut self, ctx: &mut GameContext, index: usize) {
        self.session_declined.insert(index);
        if let Some(cache) = profile_mut(ctx)
            .dispatch_board_cache
            .as_mut()
            .and_then(Value::as_object_mut)
        {
            let mut declined: Vec<i64> = cache
                .get("declined")
                .and_then(Value::as_array)
                .map(|list| {
                    list.iter()
                        .filter_map(Value::as_f64)
                        .map(|n| n as i64)
                        .collect()
                })
                .unwrap_or_default();
            if !declined.contains(&(index as i64)) {
                declined.push(index as i64);
            }
            cache.insert("declined".into(), Value::from(declined));
        }
    }

    pub fn decline_assignment(&mut self, ctx: &mut GameContext) {
        if declines_remaining(profile(ctx)) <= 0 {
            ctx.audio.play("ui/error");
            ctx.say("No declines left until your next promotion.");
            return;
        }
        {
            let p = profile_mut(ctx);
            p.career.dispatch_declines_used += 1;
            p.career.reputation = (p.career.reputation - DECLINE_REPUTATION_PENALTY).max(0.0);
        }
        let declined = self.assigned_queue[0];
        self.remember_decline(ctx, declined);
        self.assigned_queue = self.assignment_queue(ctx);
        ctx.save_profile();
        ctx.audio.play("ui/notify");
        self.refresh(ctx, false);
        let remaining = declines_remaining(profile(ctx));
        let note = if remaining > 0 {
            format!(
                "You have {remaining} decline{} left.",
                if remaining != 1 { "s" } else { "" }
            )
        } else {
            "That was your last decline until your next promotion.".to_string()
        };
        let described = self.describe(ctx, self.assigned_job(), None);
        ctx.say(&format!(
            "Load declined, on your service record. {note} New assignment: {described}"
        ));
    }

    fn job_label(&self, ctx: &GameContext, job: &Job, index: usize) -> String {
        let label = self.describe(ctx, job, Some(index));
        if self.recommended_job_index(ctx) == Some(index - 1) {
            let Some(recommendation) = self.recommendation_label(ctx) else {
                return label;
            };
            return format!("Recommended dispatch, {recommendation}: {label}");
        }
        label
    }

    fn recommendation_label(&self, ctx: &GameContext) -> Option<String> {
        if let Some(label) = self.training_recommendation_label(ctx) {
            return Some(label);
        }
        let p = profile(ctx);
        if first_dispatch_done(p) {
            return Some(career_objective(p).recommendation);
        }
        if is_company_training_profile(p) {
            return Some(career_objective(p).recommendation);
        }
        None
    }

    fn training_recommendation_label(&self, ctx: &GameContext) -> Option<String> {
        let p = profile(ctx);
        if !is_company_training_profile(p) {
            return None;
        }
        let guidance = training_guidance(p);
        if guidance.stage == TrainingStage::FirstDispatch && !first_dispatch_done(p) {
            return Some(guidance.recommendation_label);
        }
        None
    }

    /// (score, index) for each unlocked job; lower scores fit better.
    fn scored_candidates(&self, ctx: &GameContext) -> Vec<(f64, usize)> {
        let p = profile(ctx);
        let mut candidates: Vec<(f64, usize)> = Vec::new();
        for (index, job) in self.jobs.iter().enumerate() {
            if !locked_reason(p, job).is_empty() {
                continue;
            }
            if is_owner_operator(&p.business_status) {
                let business = settlement_for(p, job, false);
                candidates.push((-business.net_before_advance, index));
            } else if training_guidance(p).stage != TrainingStage::NormalGuidance {
                candidates.push((training_recommendation_score(p, job), index));
            } else {
                candidates.push((job.distance_mi, index));
            }
        }
        candidates
    }

    fn recommended_job_index(&self, ctx: &GameContext) -> Option<usize> {
        let mut candidates = self.scored_candidates(ctx);
        sort_scored(&mut candidates);
        candidates.first().map(|c| c.1)
    }

    /// Unlocked jobs in the order dispatch would assign them, best first.
    ///
    /// Declined loads move to the back: dispatch re-offers them only after
    /// the fresh candidates run out, and reopening the board does not put a
    /// refused load straight back on the driver.
    fn assignment_queue(&self, ctx: &GameContext) -> Vec<usize> {
        let mut candidates = self.scored_candidates(ctx);
        sort_scored(&mut candidates);
        let ordered: Vec<usize> = candidates.into_iter().map(|c| c.1).collect();
        let declined = self.declined_indices(ctx);
        let mut fresh: Vec<usize> = ordered
            .iter()
            .copied()
            .filter(|i| !declined.contains(i))
            .collect();
        let reoffered: Vec<usize> = ordered
            .iter()
            .copied()
            .filter(|i| declined.contains(i))
            .collect();
        // Lane variety: dispatch prefers a lane the driver has not just run.
        // A stable partition, so score order still rules inside each group,
        // and when every candidate is a recent lane nothing changes -- the
        // nudge can delay a repeat, never block dispatch.
        let recent: HashSet<&String> = profile(ctx).recent_lanes.iter().collect();
        if !recent.is_empty() {
            let world = ctx.world;
            let is_recent = |i: &usize| recent.contains(&lane_key(world, &self.jobs[*i]));
            let mut not_recent: Vec<usize> =
                fresh.iter().copied().filter(|i| !is_recent(i)).collect();
            let recent_ones: Vec<usize> = fresh.iter().copied().filter(is_recent).collect();
            not_recent.extend(recent_ones);
            fresh = not_recent;
        }
        let mut queue = fresh;
        queue.extend(reoffered);
        // A load dispatch relayed from a nearby city IS the assignment: the
        // board here was thin, which is the only reason it is on it.
        let here = ctx.world.resolve_city_key(&profile(ctx).current_city);
        if let Some(pos) = queue.iter().position(|&index| {
            let job = &self.jobs[index];
            !job.bobtail && ctx.world.resolve_city_key(&job.origin) != here
        }) {
            let index = queue.remove(pos);
            queue.insert(0, index);
        }
        // The load the driver slept for comes back as the assignment,
        // unless they have since declined it.
        if let Some(held) = held_load::held_index(profile(ctx)) {
            if let Some(pos) = queue.iter().position(|&index| index == held) {
                if !declined.contains(&held) {
                    let index = queue.remove(pos);
                    queue.insert(0, index);
                }
            }
        }
        let forced = forced_dispatch_destination();
        if !forced.is_empty() && !queue.is_empty() {
            // Playtest lever: dispatch assigns the forced-destination load
            // first, so a tester is not stuck with pot luck.
            let key = ctx.world.resolve_city_key(&forced);
            if let Some(pos) = queue
                .iter()
                .position(|&index| ctx.world.resolve_city_key(&self.jobs[index].destination) == key)
            {
                let index = queue.remove(pos);
                queue.insert(0, index);
            }
        }
        queue
    }

    /// `_locked_reason(job)`.
    pub fn locked_reason(&self, ctx: &GameContext, job: &Job) -> String {
        locked_reason(profile(ctx), job)
    }

    /// The job the current menu row refers to, in either board mode.
    fn focused_job(&self) -> Option<usize> {
        if self.assigned_mode() {
            return if self.menu.index == 0 {
                Some(self.assigned_queue[0])
            } else {
                None
            };
        }
        if self.menu.index < self.jobs.len() {
            return Some(self.menu.index);
        }
        None
    }

    fn needs_hos_confirmation(&self, ctx: &GameContext, index: usize) -> bool {
        self.job_exceeds_current_hos(ctx, &self.jobs[index])
            && self.confirm_risky_job != Some(index)
    }

    /// True when hours already spent this shift force an extra 10-hour
    /// rest the job would not need on a fresh clock. Multi-shift routes
    /// budget their own sleeps into the deadline, so a rested driver is
    /// never warned just because the run is long.
    fn job_exceeds_current_hos(&self, ctx: &GameContext, job: &Job) -> bool {
        let p = profile(ctx);
        let Some((drive_limit, duty_limit, _break_after)) = limits(&ctx.settings.hos_mode) else {
            return false;
        };
        let Ok(Some(route)) = ctx
            .world
            .supported_route(&job.origin, &job.destination, None)
        else {
            return false;
        };
        // Pickup check-in and loading are on-duty work before the first route
        // mile; being over 30 non-driving minutes, they also reset the break
        // clock, so only the drive and duty limits matter here.
        let pickup_work_min = PICKUP_CHECK_IN_MIN + PICKUP_LOADING_MIN;
        // The deadhead to the pickup is driving too. Counting the loaded
        // route alone let a 5-hour job through on 4 h 17 m of window
        // (Chippewa Falls to Duluth, owner, 2026-09-12): it fit by minutes on
        // paper, the pickup and the yard roads ate them, and the drive ended
        // with a forced 10-hour sleep 5 hours past the deadline.
        let drive_h =
            pickup_drive_hours(ctx, job) + route_drive_hours(Some(&route), 0.0, Some(ctx.world));
        let shift_h = drive_limit / 60.0;
        let fresh_first_h = (drive_limit - HOS_FIT_CUSHION_MIN)
            .min(duty_limit - pickup_work_min - HOS_FIT_CUSHION_MIN)
            / 60.0;
        let current_first_h = (drive_limit - p.hos.driving_min - HOS_FIT_CUSHION_MIN)
            .min(duty_limit - p.hos.duty_min - pickup_work_min - HOS_FIT_CUSHION_MIN)
            .max(0.0)
            / 60.0;
        sleeps_needed(drive_h, current_first_h, shift_h)
            > sleeps_needed(drive_h, fresh_first_h, shift_h)
    }

    fn hos_board_note(&self, ctx: &GameContext) -> String {
        if self.jobs.is_empty() {
            return String::new();
        }
        let risky = self
            .jobs
            .iter()
            .filter(|job| self.job_exceeds_current_hos(ctx, job))
            .count();
        if risky == self.jobs.len() {
            return "Every load needs an extra rest on your current hours. ".to_string();
        }
        if risky > 0 {
            if risky == 1 {
                return "1 load needs an extra rest on your current hours. ".to_string();
            }
            return format!("{risky} loads need an extra rest on your current hours. ");
        }
        String::new()
    }

    /// Drop a dead offer and rebuild the rows: a cached board can outlive
    /// its facilities (a data update may retire one), and the next board
    /// visit rebuilds.
    fn drop_dead_offer(&mut self, ctx: &mut GameContext, index: usize) {
        profile_mut(ctx).dispatch_board_cache = None;
        self.jobs.remove(index);
        self.confirm_risky_job = None;
        self.confirm_chain_job = None;
        self.assigned_queue = if dispatch_policy(profile(ctx)).assigns_load {
            self.assignment_queue(ctx)
        } else {
            Vec::new()
        };
        self.refresh(ctx, false);
        ctx.audio.play("ui/warning");
    }

    /// `_accept(job)`: take the job at `index` off the board.
    pub fn accept(&mut self, ctx: &mut GameContext, index: usize) {
        let Some(job) = self.jobs.get(index).cloned() else {
            return;
        };
        {
            let p = profile(ctx);
            if p.driving_record.suspended(p.game_hours) {
                let line = enforcement::suspension_refusal_line(p);
                ctx.audio.play("ui/error");
                ctx.say(&line);
                return;
            }
        }
        let locked = locked_reason(profile(ctx), &job);
        if !locked.is_empty() {
            ctx.audio.play("ui/error");
            if locked.contains("trailer program") {
                if profile(ctx).business_status == INDEPENDENT_AUTHORITY {
                    ctx.say(&format!(
                        "{locked} Open Garage, Trailers to lease support or \
                         buy a matching trailer."
                    ));
                } else {
                    ctx.say(&format!("{locked} Open Garage, Trailers to add it."));
                }
            } else {
                ctx.say(&format!(
                    "{locked} Courses are under Licenses and training at the terminal."
                ));
            }
            return;
        }
        // A confirmation belongs to the job it was heard for: another
        // selection starts over. Both stand until this job is accepted, so
        // the hours warning is not re-run after the chain reminder.
        if self.confirm_risky_job.is_some_and(|i| i != index) {
            self.confirm_risky_job = None;
        }
        if self.confirm_chain_job.is_some_and(|i| i != index) {
            self.confirm_chain_job = None;
        }
        if self.needs_hos_confirmation(ctx, index) {
            self.confirm_risky_job = Some(index);
            held_load::hold_for_rest(profile_mut(ctx), &job);
            ctx.save_profile();
            ctx.audio.play("ui/warning");
            let summary = profile(ctx).hos.summary(&ctx.settings.hos_mode);
            ctx.say(&format!(
                "Needs an extra rest on this shift's hours. {summary} Enter again to accept."
            ));
            return;
        }
        if self.confirm_chain_job != Some(index) {
            if let Some(reminder) = chain_reminder_before_accepting(ctx, &job) {
                self.confirm_chain_job = Some(index);
                ctx.audio.play("ui/warning");
                ctx.say(&reminder);
                return;
            }
        }
        self.confirm_risky_job = None;
        self.confirm_chain_job = None;
        if job.bobtail {
            self.accept_reposition(ctx, index);
            return;
        }
        // A load dispatch relayed from a nearby city: the deadhead there and
        // the local approach to its shipper are one drive, so the pickup
        // opens on arrival exactly as a same-city deadhead's does.
        let here = ctx.world.resolve_city_key(&profile(ctx).current_city);
        if ctx.world.resolve_city_key(&job.origin) != here {
            self.accept_relay(ctx, index, job, &here);
            return;
        }
        let route = ctx.world.facility_approach_route_with(
            &job.origin,
            &job.origin_location,
            ctx.settings.facility_streets,
        );
        let Ok(route) = route else {
            // A cached board can outlive its facilities (a data update may
            // retire one, e.g. a template gated out by geography). Drop the
            // dead offer instead of crashing; the next board visit rebuilds.
            self.drop_dead_offer(ctx, index);
            ctx.say("That load's facility is no longer on the network. Dispatch pulled the offer.");
            return;
        };
        let terminal = home_terminal(ctx);
        // The load is staged in the very yard the truck is parked in --
        // which is where a new hire's assigned loads are. Driving a
        // two-mile "deadhead from the terminal to the terminal" was the
        // first thing the agent playtest did on 2026-09-02, on every
        // assigned load. The shipping office is a walk across the yard, so
        // the pickup opens right here.
        if job_origin_is_this_yard(ctx, &job, &terminal.name) {
            self.accept_at_home_yard(ctx, job);
            return;
        }
        // Junior drivers slip-seat: the yard picks the tractor for the load
        // before the keys change hands, so the truck is decided before the
        // trip snapshot is taken.
        let equipment_note = slip_seat_note(ctx, &job);
        profile_mut(ctx).dispatch_board_cache = None;
        let line = format!(
            "Accepted.{equipment_note} Deadhead {} on {} to pickup at {}.",
            ctx.settings.distance_text(route.miles(), true),
            route.highways().first().cloned().unwrap_or_default(),
            job.origin_facility_text()
        );
        ctx.mark_meaningful_play(MeaningfulPlayReason::JobAccepted);
        // The deadhead is time enough for 511 to answer before the pickup.
        warm_route_feeds(ctx, &job.origin, &job.destination);
        launch_driving(
            ctx,
            DrivingLaunch::new(
                job,
                route,
                DRIVE_PHASE_PICKUP,
                LaunchAnnouncement::Line(line),
            ),
        );
        // first_dispatch is retired as an award (folded into "first_day" at
        // pickup completion, see city_pickup.py); the catalog entry and id
        // stay so the cloud validator's allow-list never sees a removed id.
    }

    /// Accept a load dispatch relayed from a nearby city (`jobs::relay`):
    /// the corridor to that city and its shipper's own approach, joined as
    /// one pickup drive, so the deadhead is driven, paid and timed as part
    /// of the assignment and the pickup opens on arrival.
    fn accept_relay(&mut self, ctx: &mut GameContext, index: usize, job: Job, here: &str) {
        let origin = ctx.world.resolve_city_key(&job.origin);
        let corridor = ctx
            .world
            .supported_route(here, &origin, None)
            .ok()
            .flatten();
        let Some(corridor) = corridor else {
            self.drop_dead_offer(ctx, index);
            ctx.say("That load's city is no longer on the network. Dispatch pulled the offer.");
            return;
        };
        // A shipper with no baked approach is reached at the city itself.
        let route = match ctx.world.facility_approach_route_with(
            &origin,
            &job.origin_location,
            ctx.settings.facility_streets,
        ) {
            Ok(approach) => corridor.then(&approach),
            Err(_) => corridor.clone(),
        };
        let equipment_note = slip_seat_note(ctx, &job);
        profile_mut(ctx).dispatch_board_cache = None;
        // The facility text already names its city; whole miles for a run
        // this long (heard live: "in Las Vegas, Nevada in Las Vegas, Nevada"
        // and "211.0 miles", 2026-09-12).
        let line = format!(
            "Accepted.{equipment_note} Deadhead {} on {} to {}.",
            ctx.settings.distance_text(corridor.miles(), false),
            corridor.highways().first().cloned().unwrap_or_default(),
            job.origin_facility_text(),
        );
        ctx.mark_meaningful_play(MeaningfulPlayReason::JobAccepted);
        warm_route_feeds(ctx, &job.origin, &job.destination);
        launch_driving(
            ctx,
            DrivingLaunch::new(
                job,
                route,
                DRIVE_PHASE_PICKUP,
                LaunchAnnouncement::Line(line),
            ),
        );
    }

    /// Accept a load staged at the home yard: no deadhead, the shipping
    /// office is here. Same bookkeeping as the deadhead launch (the slip-seat
    /// draw, the board cache, a trip snapshot so a save resumes at the
    /// pickup), then the pickup facility itself instead of a drive to it.
    fn accept_at_home_yard(&mut self, ctx: &mut GameContext, job: Job) {
        warm_route_feeds(ctx, &job.origin, &job.destination);
        let equipment_note = slip_seat_note(ctx, &job);
        profile_mut(ctx).dispatch_board_cache = None;
        let snapshot = pickup_snapshot(&job, &PickupSnapshotOptions::default());
        profile_mut(ctx).active_trip = Some(Value::Object(snapshot));
        ctx.save_profile();
        ctx.mark_meaningful_play(MeaningfulPlayReason::JobAccepted);
        ctx.say(&format!(
            "Accepted.{equipment_note} The load is here in the yard."
        ));
        let state = PickupFacilityState::new(ctx, job, PickupOptions::default());
        ctx.push_state(state);
    }

    /// Accept a carrier-ASSIGNED reposition straight off the board.
    ///
    /// There is no pickup facility to deadhead to -- the whole job IS the
    /// deadhead -- so this skips facility_approach_route and the pickup
    /// phase entirely, the same city-to-city drive BobtailDestState._start
    /// builds for a self-serve bobtail.
    fn accept_reposition(&mut self, ctx: &mut GameContext, index: usize) {
        let job = self.jobs[index].clone();
        let route = ctx
            .world
            .supported_route(&job.origin, &job.destination, None)
            .ok()
            .flatten();
        let Some(route) = route else {
            // A cached board can outlive the world it was built from, same
            // as the facility case above.
            self.drop_dead_offer(ctx, index);
            ctx.say("That route is no longer on the network. Dispatch pulled the offer.");
            return;
        };
        profile_mut(ctx).dispatch_board_cache = None;
        let line = format!(
            "Reposition to {}, {} on {}. Pays {} dollars.",
            job.spoken_destination(),
            ctx.settings.distance_text(route.miles(), true),
            route.highways().first().cloned().unwrap_or_default(),
            fmt_grouped(job.pay, 0),
        );
        ctx.mark_meaningful_play(MeaningfulPlayReason::JobAccepted);
        launch_driving(
            ctx,
            DrivingLaunch::new(
                job,
                route,
                DRIVE_PHASE_DELIVERY,
                LaunchAnnouncement::Line(line),
            ),
        );
    }
}

fn sort_scored(candidates: &mut [(f64, usize)]) {
    candidates.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(&b.1))
    });
}

/// Draw the assigned tractor and say why, or nothing if it is not new.
///
/// Silent when the truck has not changed: a driver who drew the same
/// spare three loads running does not need telling three times.
///
/// A driver past slip-seating has ONE truck and draws nothing, so there
/// is normally nothing to say -- but if the yard is holding them below
/// the tractor their level earns, that is the one thing they will want
/// explained, and this used to be silent about it. A level 11 driver
/// capped to a regional truck by their standing got handed a yard mule
/// every run with no reason given anywhere near the moment it happened;
/// the explanation existed, but only on the standing screen, which you
/// have to already suspect the answer to go and read (Brandon,
/// 2026-08-21: "what gives?").
fn slip_seat_note(ctx: &mut GameContext, job: &Job) -> String {
    let p = profile_mut(ctx);
    if p.owns_equipment() {
        return String::new();
    }
    if !slip_seats(p) {
        // No draw to announce, so say only the part that is news: why the
        // iron is short of the level, and what gives it back.
        return if equipment_held_back(p) {
            format!(" {}", equipment_hold_text(p, true))
        } else {
            String::new()
        };
    }
    let before = p.active_truck_key();
    let key = p.take_slip_seat(job);
    if key == before {
        return String::new();
    }
    format!(" {}", assignment_reason_text::<Profile>(&key, Some(p)))
}

impl Menu for JobBoardState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let n = self.jobs.len();
        let plural = if n != 1 { "es" } else { "" };
        // A board the player cannot take work from explains itself before it
        // lists anything. An unexplained empty or refusing board is exactly the
        // kind of silence this game does not do.
        {
            let p = profile(ctx);
            if p.driving_record.suspended(p.game_hours) {
                let line = enforcement::suspension_board_line(p);
                ctx.say(&line);
                return;
            }
        }
        if n == 0 {
            ctx.say("Dispatch board. No jobs available.");
        } else if self.assigned_mode() {
            self.announce_assignment(ctx);
        } else {
            let p = profile(ctx);
            // What listed amounts mean is on the job details' pay line, and
            // the objective in Career plan: the entry line is today's board.
            let hos_note = self.hos_board_note(ctx);
            let market = p.market.headline();
            ctx.say(
                format!("Dispatch board. {n} dispatch{plural} available. {hos_note}{market}")
                    .trim_end(),
            );
            let current = self.current_text(ctx);
            ctx.say_with(current, Say::queued().review(false));
        }
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        if self.assigned_mode() {
            return self.build_assignment_items(ctx);
        }
        let p = profile(ctx);
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        for (i, job) in self.jobs.iter().enumerate() {
            let locked = locked_reason(p, job);
            let mut label = self.job_label(ctx, job, i + 1);
            if !locked.is_empty() {
                label = label.replacen("Job ", "Locked job ", 1);
            }
            let help_text = if job.bobtail {
                format!(
                    "Carrier-assigned reposition: an empty deadhead to \
                     {}. Route inspection after accepting \
                     covers rest, fuel, toll, weather, and restrictions.",
                    job.spoken_destination()
                )
            } else {
                format!(
                    "Load offer from {} to \
                     {}. Route inspection after \
                     pickup covers rest, fuel, toll, weather, and restrictions.",
                    job.origin_facility_text(),
                    job.destination_facility_text()
                )
            };
            items.push(
                MenuItem::new(label, move |s: &mut Self, ctx| s.accept(ctx, i)).help(help_text),
            );
        }
        items.push(MenuItem::new("Back to terminal", |s: &mut Self, ctx| {
            s.go_back(ctx)
        }));
        items
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        if let Some((key, _, _)) = event.key_down() {
            if key == Key::F1 && !self.jobs.is_empty() {
                if let Some(index) = self.focused_job() {
                    let board: SharedState = ctx.state().expect("the board is the active state");
                    let job = self.jobs[index].clone();
                    ctx.push_state(JobDetailState::new(board, job, index));
                    return;
                }
            }
            if key == Key::Tab {
                let summary = profile(ctx).market.summary();
                ctx.say(&summary);
                return;
            }
        }
        base_menu_handle_event(self, ctx, event);
    }
}

impl_state_for_menu!(JobBoardState);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::testing::TestApp;
    use ff_core::models::jobs::cargo_type;

    fn load(pickup: &str) -> Job {
        Job::new(
            cargo_type("general").expect("general freight"),
            12.0,
            "Denver",
            pickup,
            "Silverthorne",
            70.0,
            900.0,
            9.0,
        )
    }

    #[test]
    fn test_dropping_a_dead_offer_clears_both_confirmations() {
        // The rows shift up, so a confirmation heard for one row must not
        // let the load that moves into it skip its own warning.
        let mut app = TestApp::new();
        app.ctx.profile = Some(Profile::named_in("Chain Board", "Denver"));
        let jobs = vec![load("Denver Gone-Away Storage"), load("Denver Cross-Dock")];
        let mut board = JobBoardState::new(&app.ctx, jobs);
        board.confirm_risky_job = Some(0);
        board.confirm_chain_job = Some(0);
        board.drop_dead_offer(&mut app.ctx, 0);
        assert_eq!(board.jobs.len(), 1);
        assert_eq!(board.confirm_risky_job, None);
        assert_eq!(board.confirm_chain_job, None);
    }
}
