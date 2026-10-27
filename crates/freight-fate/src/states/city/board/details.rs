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

use super::weight::load_weight_margin;
use super::{
    locked_reason, market_preview, pickup_drive_hours, settlement_for, trailer_note, JobBoardState,
};

/// Board line for a carrier-assigned reposition.
fn describe_reposition(ctx: &GameContext, total: usize, job: &Job, index: Option<usize>) -> String {
    let prefix = match index {
        Some(i) => format!("Job {i} of {total}: "),
        None => String::new(),
    };
    format!(
        "{prefix}Carrier-assigned reposition: drive empty to {}, {}. No cargo. Pays {} \
         dollars, the empty-mile rate. The {} dispatch board opens on arrival.",
        job.spoken_destination(),
        ctx.settings.distance_text(job.distance_mi, false),
        fmt_grouped(job.pay, 0),
        job.spoken_destination()
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
    let preview = format!(
        "{} {}",
        market_preview(&business),
        load_weight_margin(ctx.world, p, job)
    );
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
        "Load waiting in {}, {deadhead} deadhead first, paid at the empty-mile rate. ",
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
        let mut lines = vec![format!("Cargo: {}.", job.spoken_cargo_label())];
        if let Some(material) = job.hazmat {
            lines.push(format!("Material: {}.", material.card_text()));
        }
        lines.extend([
            format!("Origin: {}.", job.origin_facility_text()),
            format!("Destination: {destination_text}."),
            format!("Distance: {}.", s.distance_text(job.distance_mi, false)),
            load_weight_margin(ctx.world, p, job),
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
                "Deadline: {} hours once loaded, deliver by about {}.",
                fmt_f(job.deadline_game_h, 0),
                appointment_text(p.game_hours, loaded_departure_h + job.deadline_game_h, zone)
            ),
            format!("Equipment: {}.", job.equipment_text()),
            format!("Trailer: {}", trailer_note(p, job)),
        ]);
        let locked = locked_reason(p, job);
        if !locked.is_empty() {
            lines.push(format!("Locked: {locked}"));
        } else if !job.cargo.credentials.is_empty() {
            lines.push(format!(
                "Cleared for it: you hold {}.",
                credentials_clause(job.cargo.credentials)
            ));
        }
        lines.push(
            "Route details happen after pickup: rest, fuel, tolls, weather, and stops.".to_string(),
        );
        lines
    }

    fn reposition_detail_lines(&self, ctx: &GameContext) -> Vec<String> {
        let job = &self.job;
        vec![
            "Carrier-assigned reposition: empty to a nearby city where freight is thicker."
                .to_string(),
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
            "Route details happen after accepting: rest, fuel, tolls, weather, and stops."
                .to_string(),
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
        ctx.say(&format!("Job details. {JOB_DETAIL_INTRO_HELP} {current}"));
    }

    fn current_help(&self, ctx: &GameContext) -> String {
        format!(
            "{JOB_DETAIL_INTRO_HELP} {}",
            base_menu_current_help(self, ctx)
        )
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = self
            .detail_lines(ctx)
            .into_iter()
            .map(|line| {
                let spoken = line.clone();
                MenuItem::new(line, move |_s: &mut Self, ctx| ctx.say(&spoken))
                    .help("Enter repeats this line.")
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
