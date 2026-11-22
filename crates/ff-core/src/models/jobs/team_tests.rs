//! Team loads: booked for two drivers taking turns.

use super::*;
use crate::data::world::get_world;

fn long_offers() -> Vec<Job> {
    let mut jobs = Vec::new();
    for seed in 0..40 {
        jobs.extend(JobBoard::seeded(get_world(), seed).offers(
            "Chicago",
            &["refrigerated", "heavy_haul", "high_value"],
            OfferOptions {
                count: 5,
                level: 6,
                ..Default::default()
            },
        ));
    }
    jobs
}

#[test]
fn a_team_change_is_far_shorter_than_a_solo_sleep() {
    let solo = required_hours(1500.0, None, None, None);
    let team = team_required_hours(1500.0, None, None);
    assert!(team + 9.0 < solo, "team {team} h, solo {solo} h");
    // A load short enough to drive alone in one shift costs the same either way.
    let short_solo = required_hours(250.0, None, None, None);
    assert!((team_required_hours(250.0, None, None) - short_solo).abs() < 1e-9);
}

#[test]
fn the_board_books_some_long_loads_for_a_team_at_better_pay() {
    let jobs = long_offers();
    let team: Vec<&Job> = jobs.iter().filter(|j| j.team).collect();
    assert!(!team.is_empty(), "no team load in {} offers", jobs.len());
    for job in &team {
        let route = get_world()
            .supported_route(&job.origin, &job.destination, None)
            .ok()
            .flatten();
        let plan = plan_hos(job.distance_mi, route.as_ref(), Some(get_world()), None);
        assert!(
            plan.sleeps > 0,
            "{} to {} needs no sleep",
            job.origin,
            job.destination
        );
        assert!(!job.deadline_covers_rest);
        let solo = required_hours(job.distance_mi, route.as_ref(), Some(get_world()), None);
        assert!(
            job.deadline_game_h < solo,
            "{} to {}: team deadline {} h leaves room for a solo run of {} h",
            job.origin,
            job.destination,
            job.deadline_game_h,
            solo
        );
        assert!(job.describe_plain().contains("Team load"));
    }
}

#[test]
fn a_team_load_keeps_its_flag_through_a_save() {
    let job = long_offers()
        .into_iter()
        .find(|j| j.team)
        .expect("a team load");
    let payload = job_payload(&job);
    let back = job_from_payload(&payload).unwrap();
    assert!(back.team);
    let mut solo = job.clone();
    solo.team = false;
    let payload = job_payload(&solo);
    assert!(!job_from_payload(&payload).unwrap().team);
}

#[test]
fn a_team_deadline_never_leaves_room_for_a_solo_sleep_at_any_urgency() {
    use crate::models::jobs::board::{team_load_deadline_hours, TEAM_SOLO_MARGIN_H};
    use crate::models::jobs::urgency::{
        FLEXIBLE_LOAD_SLACK_RANGE, HOT_LOAD_SLACK, STANDARD_LOAD_SLACK_RANGE,
    };
    let world = get_world();
    // Chicago to Lamar: one sleep for a solo driver, and the standard load
    // the Ontario and Quebec cities brought onto Chicago's board was booked
    // at 33.3 hours against a solo 32.0.
    let route = world
        .supported_route("chicago_il_us", "lamar_co_us", None)
        .unwrap()
        .expect("Chicago reaches Lamar");
    let miles = route.miles();
    assert!(plan_hos(miles, Some(&route), Some(world), None).sleeps > 0);
    let solo = required_hours(miles, Some(&route), Some(world), None);
    // (urgency, widest slack, carrier deadline slack): the widest window each
    // class can draw, under the plain carrier and the most generous one.
    for (urgency, slack) in [
        ("hot", HOT_LOAD_SLACK),
        ("standard", STANDARD_LOAD_SLACK_RANGE.1),
        ("flexible", FLEXIBLE_LOAD_SLACK_RANGE.1),
    ] {
        for carrier_slack in [1.0, 1.08] {
            let hours = round_py_n(
                team_load_deadline_hours(miles, slack, carrier_slack, Some(&route), world),
                1,
            );
            assert!(
                hours <= solo - TEAM_SOLO_MARGIN_H + 0.05,
                "{urgency} team load at carrier slack {carrier_slack}: {hours} h against a solo {solo} h"
            );
            assert!(
                hours >= team_required_hours(miles, Some(&route), Some(world)),
                "{urgency}: {hours} h is short of the team's own hours"
            );
        }
    }
}
