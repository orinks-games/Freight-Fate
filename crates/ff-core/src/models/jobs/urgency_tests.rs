//! Hot loads and flexible deliveries on real boards.

use super::*;
use crate::data::world::get_world;
use crate::models::dispatch_fit::{recommend, DispatchGoal, FitReason, LoadFacts};

fn boards(city: &str, level: i64, count: i64) -> Vec<Job> {
    let mut jobs = Vec::new();
    for seed in 0..count {
        jobs.extend(JobBoard::seeded(get_world(), seed).offers(
            city,
            &["refrigerated", "heavy_haul", "high_value"],
            OfferOptions {
                count: 5,
                level,
                ..Default::default()
            },
        ));
    }
    jobs
}

#[test]
fn the_mix_leaves_average_pay_within_one_percent() {
    let expected = HOT_LOAD_SHARE * HOT_LOAD_PAY_MULT
        + FLEXIBLE_LOAD_SHARE * FLEXIBLE_LOAD_PAY_MULT
        + (1.0 - HOT_LOAD_SHARE - FLEXIBLE_LOAD_SHARE);
    assert!((expected - 1.0).abs() < 0.01, "{expected}");
    let jobs = boards("Chicago", 12, 200);
    let paid: f64 = jobs.iter().map(|j| j.pay).sum();
    let standard: f64 = jobs.iter().map(|j| j.pay / j.urgency.pay_mult()).sum();
    let moved = paid / standard - 1.0;
    assert!(moved.abs() < 0.01, "average pay moved {moved}");
    assert!(jobs.iter().any(|j| j.urgency == LoadUrgency::Hot));
    assert!(jobs.iter().any(|j| j.urgency == LoadUrgency::Flexible));
}

#[test]
fn hot_loads_are_tight_and_flexible_loads_are_wide() {
    let world = get_world();
    for job in boards("Chicago", 12, 40) {
        if job.team && job.urgency == LoadUrgency::Flexible {
            // Held short of a solo sleep; its own test below.
            continue;
        }
        let route = world
            .supported_route(&job.origin, &job.destination, None)
            .ok()
            .flatten();
        let required = if job.team {
            team_required_hours(job.distance_mi, route.as_ref(), Some(world))
        } else {
            required_hours(job.distance_mi, route.as_ref(), Some(world), None)
        };
        let slack = (job.deadline_game_h - DEADLINE_DISPATCH_MIN_SLACK_H) / required;
        let (lo, hi) = match job.urgency {
            LoadUrgency::Hot => (HOT_LOAD_SLACK, HOT_LOAD_SLACK),
            LoadUrgency::Standard => STANDARD_LOAD_SLACK_RANGE,
            LoadUrgency::Flexible => FLEXIBLE_LOAD_SLACK_RANGE,
        };
        // The deadline is rounded to a tenth of an hour.
        let tolerance = 0.06 / required;
        assert!(
            slack >= lo - tolerance && slack <= hi + tolerance,
            "{:?} {} to {}: slack {slack}",
            job.urgency,
            job.origin,
            job.destination
        );
    }
}

#[test]
fn the_board_row_and_its_terms_say_the_urgency() {
    let jobs = boards("Chicago", 12, 40);
    for job in &jobs {
        let row = job.describe_plain();
        match job.urgency {
            LoadUrgency::Hot => assert!(row.contains(" hours. Hot load."), "{row}"),
            LoadUrgency::Flexible => {
                assert!(row.contains(" hours. Flexible delivery."), "{row}")
            }
            LoadUrgency::Standard => {
                assert!(
                    !row.contains("Hot load") && !row.contains("Flexible"),
                    "{row}"
                )
            }
        }
        if job.team {
            let team = row.find("Team load").expect("the team sentence");
            if let Some(urgency) = row.find("Hot load").or_else(|| row.find("Flexible")) {
                assert!(urgency < team, "{row}");
            }
        }
    }
}

#[test]
fn a_relayed_load_is_always_standard() {
    let mut board = JobBoard::seeded(get_world(), 7);
    board.standard_only = true;
    let jobs = board.offers("Chicago", &["refrigerated"], OfferOptions::level(12));
    assert!(!jobs.is_empty());
    assert!(jobs.iter().all(|j| j.urgency == LoadUrgency::Standard));
}

#[test]
fn urgency_leaves_the_board_posting_the_same_loads() {
    // The draw comes from the load, so the board's own dice post the same
    // cargo, lanes and weights with or without it.
    let plain = {
        let mut b = JobBoard::seeded(get_world(), 11);
        b.standard_only = true;
        b.offers("Chicago", &["refrigerated"], OfferOptions::level(12))
    };
    let posted = JobBoard::seeded(get_world(), 11).offers(
        "Chicago",
        &["refrigerated"],
        OfferOptions::level(12),
    );
    assert_eq!(plain.len(), posted.len());
    for (a, b) in plain.iter().zip(&posted) {
        assert_eq!(
            (&a.destination, a.cargo.key, a.weight_tons),
            (&b.destination, b.cargo.key, b.weight_tons)
        );
        let ratio = b.pay / a.pay;
        assert!((ratio - b.urgency.pay_mult()).abs() < 0.001, "{ratio}");
    }
}

#[test]
fn an_old_payload_loads_as_standard_and_a_hot_load_keeps_its_terms() {
    let job = boards("Chicago", 12, 40)
        .into_iter()
        .find(|j| j.urgency == LoadUrgency::Hot)
        .expect("a hot load");
    let mut payload = job_payload(&job);
    let back = job_from_payload(&payload).unwrap();
    assert_eq!(back.urgency, LoadUrgency::Hot);
    assert_eq!(back.deadline_game_h, job.deadline_game_h);
    assert_eq!(back.pay, job.pay);
    payload.remove("urgency");
    let old = job_from_payload(&payload).unwrap();
    assert_eq!(old.urgency, LoadUrgency::Standard);
    assert_eq!(old.deadline_game_h, job.deadline_game_h);
}

#[test]
fn a_driver_whose_standing_slipped_is_steered_to_the_flexible_load() {
    // Two loads of about the same length, one flexible: the one with the
    // most room on its deadline is the one that repairs standing.
    let world = get_world();
    let jobs = boards("Chicago", 12, 200);
    let facts = |job: &Job| {
        let route = world
            .supported_route(&job.origin, &job.destination, None)
            .ok()
            .flatten();
        let hours = required_hours(job.distance_mi, route.as_ref(), Some(world), None);
        LoadFacts {
            take_home: job.pay,
            trip_h: hours,
            deadline_room_h: job.deadline_game_h - hours,
            ..Default::default()
        }
    };
    let pair = jobs.iter().filter(|j| !j.team).find_map(|flexible| {
        if flexible.urgency != LoadUrgency::Flexible {
            return None;
        }
        jobs.iter()
            .filter(|j| !j.team && j.urgency != LoadUrgency::Flexible)
            .find(|other| (other.distance_mi / flexible.distance_mi - 1.0).abs() < 0.1)
            .map(|other| (other, flexible))
    });
    let (other, flexible) = pair.expect("a flexible load beside one of its length");
    let board = [facts(other), facts(flexible)];
    let pick = recommend(&board, DispatchGoal::RepairStanding).expect("a clear pick");
    assert_eq!(pick.index, 1, "{:?}", board);
    assert_eq!(pick.reason, FitReason::MostDeadlineRoom);
}

#[test]
fn a_flexible_team_load_still_leaves_no_room_for_a_solo_sleep() {
    let world = get_world();
    let jobs = boards("Chicago", 12, 200);
    let flexible: Vec<&Job> = jobs
        .iter()
        .filter(|j| j.team && j.urgency == LoadUrgency::Flexible)
        .collect();
    assert!(!flexible.is_empty());
    for job in flexible {
        let route = world
            .supported_route(&job.origin, &job.destination, None)
            .ok()
            .flatten();
        let solo = required_hours(job.distance_mi, route.as_ref(), Some(world), None);
        assert!(
            job.deadline_game_h < solo,
            "{} h against {solo} h",
            job.deadline_game_h
        );
    }
}
