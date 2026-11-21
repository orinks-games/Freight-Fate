//! Which load dispatch recommends, and the order it assigns them in.
//!
//! A carrier's planner values a load by what the truck earns for the time
//! it takes, not by its length. Dispatch scores each unlocked load by what
//! the driver takes home per hour of the whole trip on their current clock:
//! the empty drive to the shipper, check-in and loading, the drive, its
//! breaks and its sleeps. A long load that needs a 10-hour sleep has to pay
//! for those hours; a load that fits the hours left today scores well.
//!
//! The driver's situation picks the goal ([`DispatchGoal`]): a new hire in
//! training keeps the training score, a driver whose standing slipped is
//! steered to the load with the most room on its deadline, and everyone
//! else to the best take-home for the hours.
//!
//! Headless: the game measures each load into a [`LoadFacts`] (routes,
//! settlement, clock) and this module only compares numbers. Design:
//! `docs/dispatch-realism-design.md`, section "Posted order and a
//! recommendation with a reason (2026-11-21)".

use std::collections::HashSet;

use crate::models::career_training::TrainingStage;

#[cfg(test)]
mod tests;

/// How far the best score must lead the second for dispatch to recommend
/// anything, as a share of the second. A design choice, not a measurement:
/// a recommendation should mean one load stands out, and inside 5 percent
/// two loads are the same load to a planner, so the board says nothing and
/// the cursor opens on the first row.
pub const CLEAR_LEAD: f64 = 0.05;

/// Service score below which dispatch plans for standing instead of pay.
/// The same line the career objective uses for "Build dispatcher trust"
/// (`career_objectives::company_driver_objective`).
pub const REPAIR_STANDING_BELOW: f64 = 70.0;

/// The shortest trip a score divides by, in hours: a load from this yard to
/// the next town is still at least a quarter hour of work, and a zero here
/// would make any pay infinite.
const MIN_TRIP_H: f64 = 0.25;

/// One load on the board, measured by the game for dispatch to compare.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct LoadFacts {
    /// What the driver keeps: the company wage, or an owner-operator's net
    /// before any advance (the board's "about N to you" figure).
    pub take_home: f64,
    /// Hours from accepting to delivering, on the driver's current clock:
    /// the empty drive, check-in and loading, then the drive with its breaks
    /// and sleeps (a team load swaps drivers instead of sleeping).
    pub trip_h: f64,
    /// Hours to spare on the delivery window once the loaded drive is
    /// planned on the clock the driver will have at the shipper.
    pub deadline_room_h: f64,
    /// The lane, in either direction, is one the driver just ran.
    pub recent_lane: bool,
    /// The driver lacks a credential, level or trailer for it.
    pub locked: bool,
    /// [`crate::models::career_training::training_recommendation_score`]:
    /// lower fits a trainee better. Read only by [`DispatchGoal::Training`].
    pub training_score: f64,
    /// Dispatch relayed it from a nearby city because the board here was
    /// thin: the assignment whenever dispatch assigns.
    pub relay: bool,
    /// Its destination is the playtest lever's forced one.
    pub forced: bool,
}

/// What dispatch is planning for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchGoal {
    /// A company new hire: short runs, easy deadlines, plain freight.
    Training(TrainingStage),
    /// Service slipped or load choice was revoked: the load most likely to
    /// arrive on time.
    RepairStanding,
    /// Best take-home for the hours. `after_costs` for owner-operators and
    /// own authority, whose take-home is a net.
    Earn { after_costs: bool },
}

/// Why a row is marked. Each spoken reason is said only when it is true of
/// that load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitReason {
    /// The training guidance's own label, as before.
    Training,
    BestPayForHours,
    BestNetForHours,
    MostDeadlineRoom,
    /// The lane rule moved the pick off the board's top score, so no
    /// superlative is true of it.
    Plain,
}

impl FitReason {
    /// The name the agent server's `status` reports.
    pub fn key(self) -> &'static str {
        match self {
            FitReason::Training => "training",
            FitReason::BestPayForHours => "best_pay_for_hours",
            FitReason::BestNetForHours => "best_net_for_hours",
            FitReason::MostDeadlineRoom => "most_deadline_room",
            FitReason::Plain => "plain",
        }
    }
}

/// The load dispatch recommends, by board index, and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Recommendation {
    pub index: usize,
    pub reason: FitReason,
}

/// The goal for a driver: training first, then standing, then pay.
pub fn dispatch_goal(
    training: Option<TrainingStage>,
    service: f64,
    load_choice_revoked: bool,
    owner_operator: bool,
) -> DispatchGoal {
    if let Some(stage) = training.filter(|s| *s != TrainingStage::NormalGuidance) {
        if !owner_operator {
            return DispatchGoal::Training(stage);
        }
    }
    if service < REPAIR_STANDING_BELOW || load_choice_revoked {
        return DispatchGoal::RepairStanding;
    }
    DispatchGoal::Earn {
        after_costs: owner_operator,
    }
}

/// The load's score under `goal`; larger fits better.
pub fn fit_score(facts: &LoadFacts, goal: DispatchGoal) -> f64 {
    let hours = facts.trip_h.max(MIN_TRIP_H);
    match goal {
        DispatchGoal::Training(_) => -facts.training_score,
        DispatchGoal::RepairStanding => facts.deadline_room_h / hours,
        DispatchGoal::Earn { .. } => facts.take_home / hours,
    }
}

/// Unlocked loads, best score first; ties keep board order.
fn ranked(facts: &[LoadFacts], goal: DispatchGoal) -> Vec<(f64, usize)> {
    let mut scored: Vec<(f64, usize)> = facts
        .iter()
        .enumerate()
        .filter(|(_, f)| !f.locked)
        .map(|(i, f)| (fit_score(f, goal), i))
        .collect();
    scored.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
    scored
}

/// The load dispatch recommends, or `None` when nothing stands out.
///
/// The best score among unlocked loads on lanes the driver has not just
/// run, either way; every unlocked load when every lane is recent. Outside
/// training, a board with one unlocked load, or a best that leads the
/// runner-up by less than [`CLEAR_LEAD`], recommends nothing.
pub fn recommend(facts: &[LoadFacts], goal: DispatchGoal) -> Option<Recommendation> {
    let all = ranked(facts, goal);
    let fresh: Vec<(f64, usize)> = all
        .iter()
        .copied()
        .filter(|(_, i)| !facts[*i].recent_lane)
        .collect();
    let pool = if fresh.is_empty() { &all } else { &fresh };
    let &(best, index) = pool.first()?;
    if !matches!(goal, DispatchGoal::Training(_)) {
        // One load to take is not a pick dispatch made.
        if all.len() < 2 {
            return None;
        }
        if let Some(&(second, _)) = pool.get(1) {
            if best - second < CLEAR_LEAD * second.abs() {
                return None;
            }
        }
    }
    let reason = match goal {
        DispatchGoal::Training(_) => FitReason::Training,
        _ if all.first().map(|c| c.1) != Some(index) => FitReason::Plain,
        DispatchGoal::RepairStanding => FitReason::MostDeadlineRoom,
        DispatchGoal::Earn { after_costs: false } => FitReason::BestPayForHours,
        DispatchGoal::Earn { after_costs: true } => FitReason::BestNetForHours,
    };
    Some(Recommendation { index, reason })
}

/// Unlocked loads in the order dispatch assigns them, best first.
///
/// Score order, then stable partitions, each moving its loads ahead of the
/// rest without reordering either side: loads not yet declined before
/// declined ones (a refused load comes back only after the fresh ones run
/// out), lanes not just run before recent ones (fresh loads only), then to
/// the front the relayed load, the held load unless declined, and the
/// playtest lever's forced destination, in that order so the last wins.
pub fn assignment_order(
    facts: &[LoadFacts],
    goal: DispatchGoal,
    declined: &HashSet<usize>,
    held: Option<usize>,
) -> Vec<usize> {
    let ordered: Vec<usize> = ranked(facts, goal).into_iter().map(|c| c.1).collect();
    let (fresh, reoffered): (Vec<usize>, Vec<usize>) =
        ordered.iter().partition(|i| !declined.contains(i));
    let (mut queue, recent): (Vec<usize>, Vec<usize>) =
        fresh.iter().partition(|i| !facts[**i].recent_lane);
    queue.extend(recent);
    queue.extend(reoffered);
    let to_front = |queue: &mut Vec<usize>, pos: Option<usize>| {
        if let Some(pos) = pos {
            let index = queue.remove(pos);
            queue.insert(0, index);
        }
    };
    let relay = queue.iter().position(|&i| facts[i].relay);
    to_front(&mut queue, relay);
    if let Some(held) = held.filter(|h| !declined.contains(h)) {
        let pos = queue.iter().position(|&i| i == held);
        to_front(&mut queue, pos);
    }
    let forced = queue.iter().position(|&i| facts[i].forced);
    to_front(&mut queue, forced);
    queue
}
