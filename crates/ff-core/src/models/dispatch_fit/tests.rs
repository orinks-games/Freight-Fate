use super::*;

fn load(take_home: f64, trip_h: f64) -> LoadFacts {
    LoadFacts {
        take_home,
        trip_h,
        deadline_room_h: 4.0,
        ..LoadFacts::default()
    }
}

const EARN: DispatchGoal = DispatchGoal::Earn { after_costs: false };

#[test]
fn earn_ranks_by_take_home_per_trip_hour() {
    // The long load pays the most but needs a sleep: 1,800 over 30 hours is
    // 60 an hour, the short one 500 over 5 is 100 an hour.
    let board = [load(1800.0, 30.0), load(500.0, 5.0), load(900.0, 12.0)];
    assert_eq!(
        recommend(&board, EARN),
        Some(Recommendation {
            index: 1,
            reason: FitReason::BestPayForHours
        })
    );
    let owner = DispatchGoal::Earn { after_costs: true };
    assert_eq!(
        recommend(&board, owner).map(|r| r.reason),
        Some(FitReason::BestNetForHours)
    );
}

#[test]
fn a_close_second_recommends_nothing() {
    // 100 an hour against 97: inside the 5 percent lead.
    let board = [load(1000.0, 10.0), load(970.0, 10.0), load(200.0, 10.0)];
    assert_eq!(recommend(&board, EARN), None);
    // 100 against 94 stands out.
    let board = [load(1000.0, 10.0), load(940.0, 10.0)];
    assert_eq!(recommend(&board, EARN).map(|r| r.index), Some(0));
}

#[test]
fn repair_standing_ranks_by_deadline_room_for_the_hours() {
    let mut tight = load(2000.0, 10.0);
    tight.deadline_room_h = 1.0;
    let mut roomy = load(500.0, 10.0);
    roomy.deadline_room_h = 6.0;
    let board = [tight, roomy];
    assert_eq!(
        recommend(&board, DispatchGoal::RepairStanding),
        Some(Recommendation {
            index: 1,
            reason: FitReason::MostDeadlineRoom
        })
    );
}

#[test]
fn locked_loads_are_never_recommended() {
    let mut best = load(5000.0, 5.0);
    best.locked = true;
    let board = [best, load(500.0, 5.0), load(200.0, 5.0)];
    assert_eq!(recommend(&board, EARN).map(|r| r.index), Some(1));
    let mut all_locked = board.clone();
    for f in &mut all_locked {
        f.locked = true;
    }
    assert_eq!(recommend(&all_locked, EARN), None);
}

#[test]
fn the_lane_rule_moves_the_pick_and_drops_the_superlative() {
    let mut back = load(2000.0, 10.0);
    back.recent_lane = true;
    let board = [back, load(1000.0, 10.0), load(500.0, 10.0)];
    assert_eq!(
        recommend(&board, EARN),
        Some(Recommendation {
            index: 1,
            reason: FitReason::Plain
        })
    );
    // Every lane recent: the top score stands, with its reason.
    let mut all_recent = board.clone();
    for f in &mut all_recent {
        f.recent_lane = true;
    }
    assert_eq!(
        recommend(&all_recent, EARN),
        Some(Recommendation {
            index: 0,
            reason: FitReason::BestPayForHours
        })
    );
}

#[test]
fn training_keeps_its_score_and_always_recommends() {
    let mut a = load(0.0, 1.0);
    a.training_score = 100.0;
    let mut b = load(0.0, 1.0);
    b.training_score = 99.0;
    let board = [a, b];
    let goal = DispatchGoal::Training(TrainingStage::TrainerReminders);
    assert_eq!(
        recommend(&board, goal),
        Some(Recommendation {
            index: 1,
            reason: FitReason::Training
        })
    );
}

#[test]
fn the_goal_follows_the_drivers_situation() {
    use TrainingStage::*;
    assert_eq!(
        dispatch_goal(Some(FirstDispatch), 50.0, false, false),
        DispatchGoal::Training(FirstDispatch)
    );
    assert_eq!(
        dispatch_goal(Some(NormalGuidance), 69.0, false, false),
        DispatchGoal::RepairStanding
    );
    assert_eq!(
        dispatch_goal(Some(NormalGuidance), 90.0, true, false),
        DispatchGoal::RepairStanding
    );
    assert_eq!(
        dispatch_goal(Some(NormalGuidance), 70.0, false, false),
        EARN
    );
    assert_eq!(
        dispatch_goal(None, 80.0, false, true),
        DispatchGoal::Earn { after_costs: true }
    );
    // An owner-operator is never in company training.
    assert_eq!(
        dispatch_goal(Some(TrustBuilding), 80.0, false, true),
        DispatchGoal::Earn { after_costs: true }
    );
}

#[test]
fn a_zero_hour_trip_does_not_divide_by_zero() {
    assert!(fit_score(&load(100.0, 0.0), EARN).is_finite());
}

#[test]
fn assignment_order_keeps_every_partition() {
    // Scores: 0 -> 50, 1 -> 100, 2 -> 80, 3 -> 90, 4 -> 70 an hour.
    let board = [
        load(500.0, 10.0),
        load(1000.0, 10.0),
        load(800.0, 10.0),
        load(900.0, 10.0),
        load(700.0, 10.0),
    ];
    let none = HashSet::new();
    assert_eq!(assignment_order(&board, EARN, &none, None), [1, 3, 2, 4, 0]);
    // Declined loads go to the back.
    let declined: HashSet<usize> = [1].into();
    assert_eq!(
        assignment_order(&board, EARN, &declined, None),
        [3, 2, 4, 0, 1]
    );
    // Recent lanes wait behind fresh ones.
    let mut recent = board.clone();
    recent[3].recent_lane = true;
    assert_eq!(
        assignment_order(&recent, EARN, &none, None),
        [1, 2, 4, 0, 3]
    );
    // The relayed load, then the held one, then the forced one go first.
    let mut pinned = board.clone();
    pinned[0].relay = true;
    assert_eq!(
        assignment_order(&pinned, EARN, &none, None),
        [0, 1, 3, 2, 4]
    );
    assert_eq!(
        assignment_order(&pinned, EARN, &none, Some(4)),
        [4, 0, 1, 3, 2]
    );
    assert_eq!(
        assignment_order(&pinned, EARN, &[4].into(), Some(4)),
        [0, 1, 3, 2, 4],
        "a declined held load is not pinned"
    );
    pinned[2].forced = true;
    assert_eq!(
        assignment_order(&pinned, EARN, &none, Some(4)),
        [2, 4, 0, 1, 3]
    );
    // Locked loads are never assigned.
    let mut locked = board.clone();
    locked[1].locked = true;
    assert_eq!(assignment_order(&locked, EARN, &none, None), [3, 2, 4, 0]);
}

#[test]
fn a_lone_load_is_not_a_recommendation() {
    let mut locked = load(900.0, 5.0);
    locked.locked = true;
    assert_eq!(recommend(&[load(500.0, 5.0)], EARN), None);
    assert_eq!(recommend(&[load(500.0, 5.0), locked], EARN), None);
    let goal = DispatchGoal::Training(TrainingStage::FirstDispatch);
    assert_eq!(
        recommend(&[load(500.0, 5.0)], goal).map(|r| r.index),
        Some(0)
    );
}
