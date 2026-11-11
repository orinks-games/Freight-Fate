//! The load as a whole: the five machines moving together through
//! [`LivestockLoad::tick`].

use super::*;

const AMARILLO: (f64, f64) = (35.222, -101.8313);
const OMAHA: (f64, f64) = (41.2565, -95.9345);

fn load() -> LivestockLoad {
    let mut load = LivestockLoad::booked(CattleKind::FeederCalves, 22.2, AMARILLO, OMAHA, 7);
    load.load();
    load
}

fn moving(real_s: f64, decel_g: f64, lateral_g: f64) -> LivestockTick {
    LivestockTick {
        real_s,
        game_min: real_s / 60.0,
        decel_g,
        lateral_g,
        speed_mph: 60.0,
        outside_temp_f: 70.0,
        lat: AMARILLO.0,
        lon: AMARILLO.1,
    }
}

#[test]
fn loading_starts_the_limit_and_the_exemption() {
    let mut load = LivestockLoad::booked(CattleKind::FeederCalves, 22.2, AMARILLO, OMAHA, 7);
    assert!(!load.aboard());
    let changes = load.load();
    assert_eq!(load.phase, LoadPhase::Aboard);
    assert_eq!(load.limit, LimitMark::Running);
    assert_eq!(load.exemption, Exemption::NearPickup);
    assert_eq!(load.head, 89);
    assert!(changes.contains(&LivestockChange::Phase(LoadPhase::Aboard)));
    assert!(load.hours_exempt());
}

#[test]
fn a_booked_load_does_not_tick() {
    let mut load = LivestockLoad::booked(CattleKind::FedCattle, 20.0, AMARILLO, OMAHA, 1);
    let step = load.tick(moving(10.0, 0.9, 0.0));
    assert!(step.changes.is_empty());
    assert_eq!(step.condition_pct, 0.0);
    assert_eq!(load.aboard_min, 0.0);
}

#[test]
fn smooth_driving_costs_nothing() {
    let mut load = load();
    let mut lost = 0.0;
    for _ in 0..600 {
        lost += load.tick(moving(1.0, 0.05, 0.08)).condition_pct;
    }
    assert_eq!(lost, 0.0);
    assert_eq!(load.footing, Footing::Settled);
}

#[test]
fn a_firm_stop_unsettles_them_and_calm_settles_them_again() {
    let mut load = load();
    let step = load.tick(moving(1.0, 0.2, 0.0));
    assert_eq!(load.footing, Footing::Restless);
    assert_eq!(step.condition_pct, 0.0);
    assert!(step.changes.contains(&LivestockChange::Footing {
        from: Footing::Settled,
        to: Footing::Restless
    }));
    for _ in 0..(FOOTING_CALM_MIN as usize * 60 + 1) {
        load.tick(moving(1.0, 0.0, 0.0));
    }
    assert_eq!(load.footing, Footing::Settled);
}

#[test]
fn a_hard_stop_throws_them_and_costs_condition_while_it_lasts() {
    let mut load = load();
    let step = load.tick(moving(1.0, 0.4, 0.0));
    assert_eq!(load.footing, Footing::Scrambling);
    assert!(step.condition_pct > SCRAMBLE_PCT_PER_S);
    let step = load.tick(moving(1.0, 0.0, 0.0));
    assert_eq!(load.footing, Footing::Restless);
    assert_eq!(step.condition_pct, 0.0);
}

#[test]
fn a_long_scramble_puts_one_down_the_same_way_for_the_same_seed() {
    let run = |seed: i64| {
        let mut load = LivestockLoad::booked(CattleKind::FedCattle, 20.0, AMARILLO, OMAHA, seed);
        load.load();
        let mut seconds = 0;
        while load.footing != Footing::OneDown && seconds < 600 {
            load.tick(moving(1.0, 0.5, 0.0));
            seconds += 1;
        }
        (load.footing, seconds)
    };
    let (footing, seconds) = run(11);
    assert_eq!(footing, Footing::OneDown);
    assert_eq!(run(11), (footing, seconds));
}

#[test]
fn one_down_holds_until_the_check_and_costs_by_the_minute() {
    let mut load = load();
    load.stage(None, Some(Footing::OneDown), None, None, None);
    let step = load.tick(LivestockTick::parked(60.0, 70.0, AMARILLO.0, AMARILLO.1));
    assert_eq!(load.footing, Footing::OneDown);
    assert!((step.condition_pct - DOWN_PCT_PER_MIN * 60.0).abs() < 1e-9);
    load.tick(moving(5.0, 0.6, 0.0));
    assert_eq!(load.footing, Footing::OneDown);
    assert!(load.check());
    assert_eq!(load.footing, Footing::Restless);
    assert!(!load.check());
}

#[test]
fn the_limit_warns_at_each_mark_and_then_costs_by_the_hour() {
    let mut load = load();
    let mut marks = Vec::new();
    for _ in 0..(29 * 60) {
        let step = load.tick(LivestockTick::parked(1.0, 70.0, AMARILLO.0, AMARILLO.1));
        for change in step.changes {
            if let LivestockChange::Limit(mark) = change {
                marks.push(mark);
            }
        }
    }
    assert_eq!(
        marks,
        vec![
            LimitMark::TwoHoursLeft,
            LimitMark::OneHourLeft,
            LimitMark::ThirtyMinutesLeft,
            LimitMark::Over
        ]
    );
    assert_eq!(load.phase, LoadPhase::OverLimit);
    let step = load.tick(LivestockTick::parked(60.0, 70.0, AMARILLO.0, AMARILLO.1));
    assert!((step.condition_pct - OVER_LIMIT_PCT_PER_H).abs() < 1e-9);
}

#[test]
fn a_night_parked_jumps_straight_to_the_furthest_mark() {
    let mut load = load();
    load.stage(None, None, None, None, Some(18.0));
    let step = load.tick(LivestockTick::parked(
        10.0 * 60.0,
        70.0,
        AMARILLO.0,
        AMARILLO.1,
    ));
    let limits: Vec<_> = step
        .changes
        .iter()
        .filter_map(|change| match change {
            LivestockChange::Limit(mark) => Some(*mark),
            _ => None,
        })
        .collect();
    assert_eq!(limits, vec![LimitMark::Over]);
}

#[test]
fn heat_costs_only_while_the_truck_stands() {
    let hot = danger_temp_f() + 4.0;
    let mut load = load();
    let mut tick = moving(60.0, 0.0, 0.0);
    tick.outside_temp_f = hot;
    let step = load.tick(tick);
    assert_eq!(load.heat, Heat::HotMoving);
    assert_eq!(step.condition_pct, 0.0);
    let step = load.tick(LivestockTick::parked(30.0, hot, AMARILLO.0, AMARILLO.1));
    assert_eq!(load.heat, Heat::HotStanding);
    assert_eq!(load.footing, Footing::Restless);
    assert!(step.condition_pct > HEAT_PCT_PER_MIN * 30.0);
    // Restless cattle do not settle while the heat holds them.
    load.tick(LivestockTick::parked(30.0, hot, AMARILLO.0, AMARILLO.1));
    assert_eq!(load.footing, Footing::Restless);
}

#[test]
fn the_circles_move_the_exemption_once_each_way() {
    let mut load = load();
    let mut seen = Vec::new();
    // Amarillo to Omaha in a straight line, a hundred steps.
    for i in 0..=100 {
        let f = i as f64 / 100.0;
        let mut tick = moving(1.0, 0.0, 0.0);
        tick.lat = AMARILLO.0 + (OMAHA.0 - AMARILLO.0) * f;
        tick.lon = AMARILLO.1 + (OMAHA.1 - AMARILLO.1) * f;
        for change in load.tick(tick).changes {
            if let LivestockChange::Exemption { to, .. } = change {
                seen.push(to);
            }
        }
    }
    assert_eq!(seen, vec![Exemption::Counting, Exemption::NearDelivery]);
    let changes = load.start_unloading();
    assert_eq!(load.exemption, Exemption::NotInForce);
    assert_eq!(load.limit, LimitMark::NotRunning);
    assert!(changes.contains(&LivestockChange::Phase(LoadPhase::Unloading)));
    load.delivered();
    assert_eq!(load.phase, LoadPhase::Delivered);
}

#[test]
fn a_load_survives_a_save_round_trip() {
    let mut load = load();
    load.tick(moving(1.0, 0.4, 0.0));
    let json = serde_json::to_value(&load).expect("serialises");
    let back: LivestockLoad = serde_json::from_value(json).expect("deserialises");
    assert_eq!(back, load);
}
