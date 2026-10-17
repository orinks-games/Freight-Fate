//! Dispatch descriptions and the reviewable job-detail screen.

use ff_core::models::business::pay_label;
use ff_core::models::jobs::{credentials_clause, facility_text, DescribeOptions, Job};
use ff_core::pyfmt::{fmt_f, fmt_grouped};
use ff_core::sim::timezones::{appointment_text, city_zone};

use crate::app::{GameContext, SharedState};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::city::{
    base_menu_current_help, profile, PICKUP_CHECK_IN_MIN, PICKUP_LOADING_MIN,
};

use super::briefing;
use super::weight::load_weight_margin;
use super::{
    locked_reason, market_preview, pay_basis_note, pickup_drive_hours, settlement_for,
    trailer_note, JobBoardState,
};

/// Board line for a carrier-assigned reposition.
fn describe_reposition(ctx: &GameContext, total: usize, job: &Job, index: Option<usize>) -> String {
    let prefix = match index {
        Some(i) => format!("Job {i} of {total}: "),
        None => String::new(),
    };
    format!(
        "{prefix}Reposition, empty, to {}, {}. Pays {} dollars.",
        job.spoken_destination(),
        ctx.settings.distance_text(job.distance_mi, false),
        fmt_grouped(job.pay, 0),
    )
}

/// Job-board description. `index` is the one-based board position.
pub fn describe_job(ctx: &GameContext, total: usize, job: &Job, index: Option<usize>) -> String {
    if job.bobtail {
        return describe_reposition(ctx, total, job, index);
    }
    let p = profile(ctx);
    let business = settlement_for(p, job, true);
    let note = trailer_note(p, job);
    let weight = load_weight_margin(p, job);
    let preview = if weight.is_empty() {
        market_preview(&business)
    } else {
        format!("{} {weight}", market_preview(&business))
    };
    let distance = ctx.settings.distance_text(job.distance_mi, false);
    let text = job.describe(&DescribeOptions {
        index,
        total: index.map(|_| total),
        pay_label: pay_label(&p.business_status),
        trailer_note: &note,
        display_pay: Some(business.gross_pay),
        market_preview: &preview,
        distance_text: &distance,
    });
    // A load dispatch relayed from a nearby city: the deadhead comes first,
    // and the driver hears so before the load itself.
    let here = ctx.world.resolve_city_key(&p.current_city);
    if ctx.world.resolve_city_key(&job.origin) == here {
        return text;
    }
    let deadhead = ctx
        .world
        .supported_route(&here, &job.origin, None)
        .ok()
        .flatten()
        .map(|route| ctx.settings.distance_text(route.miles(), false))
        .unwrap_or_else(|| "a drive".to_string());
    let waiting = format!(
        "Load in {}, {deadhead} deadhead first. ",
        job.spoken_origin()
    );
    match index {
        Some(_) => text.replacen(": ", &format!(": {waiting}"), 1),
        None => format!("{waiting}{text}"),
    }
}

const JOB_DETAIL_INTRO_HELP: &str =
    "Up and down review the lines, Home and End jump to the ends. Enter repeats a line, or \
     accepts on Accept this dispatch. Escape returns to the dispatch board.";

pub struct JobDetailState {
    menu: MenuCore<Self>,
    /// The board this job came from, for `Accept this dispatch`.
    board: SharedState,
    pub job: Job,
    job_index: usize,
}

impl JobDetailState {
    /// `job_index` is the job's position on its board.
    pub fn new(board: SharedState, job: Job, job_index: usize) -> Self {
        JobDetailState {
            menu: MenuCore::new("Job details").with_intro_help(JOB_DETAIL_INTRO_HELP),
            board,
            job,
            job_index,
        }
    }

    fn accept(&mut self, ctx: &mut GameContext) {
        ctx.pop_state();
        let board = self.board.clone();
        let index = self.job_index;
        if let Ok(mut state) = board.try_borrow_mut() {
            if let Some(board) = state.as_any_mut().downcast_mut::<JobBoardState>() {
                board.accept(ctx, index);
            }
        };
    }

    fn detail_lines(&self, ctx: &GameContext) -> Vec<String> {
        let job = &self.job;
        if job.bobtail {
            return self.reposition_detail_lines(ctx);
        }
        let p = profile(ctx);
        let business = settlement_for(p, job, true);
        let dollars_per_mile = business.gross_pay / job.distance_mi.max(1.0);
        let s = &ctx.settings;
        let world = ctx.world;
        let destination_text = facility_text(
            &job.destination_type,
            &job.destination_location,
            &world.spoken_city(&job.destination, Some(true)),
            &job.destination_locality,
        );
        let zone = world
            .city(&job.destination)
            .map(|city| city_zone(city))
            .unwrap_or(ff_core::sim::timezones::EASTERN);
        // The delivery clock starts when the loaded truck leaves the shipper,
        // after the drive there and the check-in and loading: a relayed
        // load's deadhead can run longer than the deadline's slack.
        let loaded_departure_h =
            pickup_drive_hours(ctx, job) + (PICKUP_CHECK_IN_MIN + PICKUP_LOADING_MIN) / 60.0;
        let mut lines = vec![
            format!("Cargo: {}.", job.cargo.label),
            format!("Origin: {}.", job.origin_facility_text()),
            format!("Destination: {destination_text}."),
            format!("Distance: {}.", s.distance_text(job.distance_mi, false)),
            format!(
                "{}: {} dollars.",
                pay_label(&p.business_status),
                fmt_grouped(business.gross_pay, 0)
            ),
            format!(
                "Dollars per {}: {}.",
                s.distance_unit_text(false),
                fmt_f(s.per_distance(dollars_per_mile), 2)
            ),
            format!(
                "Deadline {} hours after loading, about {}.",
                fmt_f(job.deadline_game_h, 0),
                appointment_text(p.game_hours, loaded_departure_h + job.deadline_game_h, zone)
            ),
        ];
        lines.extend(briefing::hours_lines(ctx, job));
        lines.push(briefing::fuel_line(ctx, job));
        lines.push(format!("Equipment: {}.", job.equipment_text()));
        let weight = load_weight_margin(p, job);
        if !weight.is_empty() {
            lines.insert(4, weight);
        }
        let trailer = trailer_note(p, job);
        if !trailer.is_empty() {
            lines.push(format!("Trailer: {trailer}"));
        }
        let locked = locked_reason(p, job);
        if !locked.is_empty() {
            lines.push(format!("Locked: {locked}"));
        } else if !job.cargo.credentials.is_empty() {
            lines.push(format!(
                "Cleared for it: you hold {}.",
                credentials_clause(job.cargo.credentials)
            ));
        }
        lines
    }

    fn reposition_detail_lines(&self, ctx: &GameContext) -> Vec<String> {
        let job = &self.job;
        vec![
            "Reposition, empty.".to_string(),
            format!(
                "Destination: {}.",
                ctx.world.spoken_city(&job.destination, Some(true))
            ),
            format!(
                "Distance: {}.",
                ctx.settings.distance_text(job.distance_mi, false)
            ),
            format!(
                "Pay: {} dollars, the empty-mile rate.",
                fmt_grouped(job.pay, 0)
            ),
            "No cargo, no trailer program, no endorsement needed.".to_string(),
        ]
    }
}

impl Menu for JobDetailState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let current = self.current_text(ctx);
        ctx.say(&format!("Job details. {current}"));
    }

    fn current_help(&self, ctx: &GameContext) -> String {
        format!(
            "{JOB_DETAIL_INTRO_HELP} {}",
            base_menu_current_help(self, ctx)
        )
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let status = profile(ctx).business_status.clone();
        let pay_line = format!("{}: ", pay_label(&status));
        let mut items: Vec<MenuItem<Self>> = self
            .detail_lines(ctx)
            .into_iter()
            .map(|line| {
                // The pay line's help says what the listed amount is, which
                // the board's entry line no longer explains.
                let help = if !self.job.bobtail && line.starts_with(&pay_line) {
                    pay_basis_note(&status)
                } else {
                    "Enter repeats this line."
                };
                let spoken = line.clone();
                MenuItem::new(line, move |_s: &mut Self, ctx| ctx.say(&spoken)).help(help)
            })
            .collect();
        let locked = locked_reason(profile(ctx), &self.job);
        if !locked.is_empty() {
            let spoken = locked.clone();
            items.push(
                MenuItem::new(
                    format!("Cannot accept this dispatch: {locked}"),
                    move |_s: &mut Self, ctx| ctx.say(&spoken),
                )
                .help(format!("This dispatch is locked. {locked}")),
            );
        } else {
            items.push(
                MenuItem::new("Accept this dispatch", |s: &mut Self, ctx| s.accept(ctx))
                    .help("Accepts and begins the pickup drive."),
            );
        }
        items.push(
            MenuItem::new("Back to dispatch board", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Back without accepting."),
        );
        items
    }
}

impl_state_for_menu!(JobDetailState);
