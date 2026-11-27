use super::*;

fn at(phase: DayPhase, temperature_c: Option<f64>, weather: WeatherKind) -> OutsideConditions {
    OutsideConditions {
        phase,
        temperature_c,
        weather,
    }
}

#[test]
fn the_scene_moves_by_its_transition_table() {
    use DayPhase::*;
    use OutsideScene::*;
    use WeatherKind::*;
    let rows: [(OutsideScene, OutsideConditions, OutsideScene); 16] = [
        (Quiet, at(Day, Some(20.0), Clear), Birds),
        (Quiet, at(Dawn, Some(0.0), Cloudy), Birds),
        (Quiet, at(Day, Some(-0.5), Clear), Quiet),
        (Quiet, at(Night, Some(15.0), Clear), Insects),
        (Quiet, at(Dusk, Some(10.0), Rain), Insects),
        (Quiet, at(Night, Some(9.9), Clear), Quiet),
        (Quiet, at(Night, None, Fog), Insects),
        (Quiet, at(Day, None, Wind), Birds),
        (Birds, at(Day, Some(25.0), HeavyRain), Quiet),
        (Insects, at(Night, Some(25.0), Thunderstorm), Quiet),
        (Birds, at(Day, Some(5.0), Snow), Quiet),
        (Insects, at(Night, Some(12.0), Ice), Quiet),
        (Birds, at(Dusk, Some(20.0), Clear), Insects),
        (Insects, at(Dawn, Some(20.0), Clear), Birds),
        (Insects, at(Dawn, Some(-1.0), Clear), Quiet),
        (Birds, at(Night, Some(9.0), Clear), Quiet),
    ];
    for (from, now, to) in rows {
        assert_eq!(from.next(now), to, "{from:?} at {now:?}");
    }
    assert_eq!(
        [Quiet, Birds, Insects].map(OutsideScene::name),
        ["quiet", "birds", "insects"]
    );
}

#[test]
fn a_playing_scene_holds_two_degrees_under_its_line() {
    use DayPhase::*;
    use OutsideScene::*;
    let night = |t| at(Night, Some(t), WeatherKind::Clear);
    assert_eq!(Insects.next(night(9.0)), Insects);
    assert_eq!(Insects.next(night(8.0)), Insects);
    assert_eq!(Insects.next(night(7.9)), Quiet);
    assert_eq!(
        Quiet.next(night(9.0)),
        Quiet,
        "starting needs the line itself"
    );
    let day = |t| at(Day, Some(t), WeatherKind::Clear);
    assert_eq!(Birds.next(day(-2.0)), Birds);
    assert_eq!(Birds.next(day(-2.1)), Quiet);
    assert_eq!(Quiet.next(day(-1.0)), Quiet);
}

#[test]
fn the_phase_follows_the_clock() {
    assert_eq!(DayPhase::at_hour(6.0), DayPhase::Dawn);
    assert_eq!(DayPhase::at_hour(12.0), DayPhase::Day);
    assert_eq!(DayPhase::at_hour(20.0), DayPhase::Dusk);
    assert_eq!(DayPhase::at_hour(23.0), DayPhase::Night);
    assert_eq!(DayPhase::at_hour(3.0), DayPhase::Night);
}

#[test]
fn each_scene_plays_its_bed() {
    assert_eq!(scene_bed(OutsideScene::Insects), Some(INSECTS));
    assert_eq!(scene_bed(OutsideScene::Birds), Some(BIRDS));
    assert_eq!(scene_bed(OutsideScene::Quiet), None);
}

#[test]
fn every_bed_has_a_sealed_twin_by_name() {
    for bed in [INSECTS, BIRDS, RIVER] {
        assert_eq!(bed.sealed, format!("{}_sealed", bed.open));
        assert!(bed.open.starts_with("ambient/"));
    }
}

#[test]
fn the_water_moves_by_its_transition_table() {
    use Water::*;
    let tick = |dt_real_s, near| WaterEvent::Tick { dt_real_s, near };
    let rows: [(Water, WaterEvent, Water); 7] = [
        (Away, tick(0.1, false), Away),
        (
            Away,
            tick(0.1, true),
            Over {
                hold_s: RIVER_HOLD_S,
            },
        ),
        (
            Over { hold_s: 1.0 },
            tick(0.1, true),
            Over {
                hold_s: RIVER_HOLD_S,
            },
        ),
        (Over { hold_s: 4.0 }, tick(1.0, false), Over { hold_s: 3.0 }),
        (Over { hold_s: 1.0 }, tick(1.0, false), Away),
        (Over { hold_s: 0.5 }, tick(2.0, false), Away),
        (Over { hold_s: 2.0 }, tick(0.0, false), Over { hold_s: 2.0 }),
    ];
    for (from, event, to) in rows {
        assert_eq!(from.next(event), to, "{from:?} on {event:?}");
    }
    assert_eq!([Away.name(), Over { hold_s: 1.0 }.name()], ["away", "over"]);
}

#[test]
fn a_crossing_plays_for_the_hold_after_the_truck_leaves_it() {
    let mut water = Water::Away;
    water = water.next(WaterEvent::Tick {
        dt_real_s: 0.1,
        near: true,
    });
    let mut seconds = 0.0;
    while water.is_over() {
        water = water.next(WaterEvent::Tick {
            dt_real_s: 0.1,
            near: false,
        });
        seconds += 0.1;
    }
    assert!((seconds - RIVER_HOLD_S).abs() < 0.11, "{seconds}");
}

#[test]
fn near_river_is_the_span_either_side_of_a_point() {
    let rivers = [10.0, 25.0];
    assert!(near_river(&rivers, 10.0));
    assert!(near_river(&rivers, 9.86));
    assert!(near_river(&rivers, 10.14));
    assert!(!near_river(&rivers, 9.8));
    assert!(!near_river(&rivers, 17.0));
    assert!(near_river(&rivers, 25.1));
    assert!(!near_river(&[], 10.0));
}

#[test]
fn the_mix_follows_the_glass_and_the_road() {
    assert_eq!(
        bed_mix(1.0, 0.0, 1.0, 1.0),
        BedMix {
            sealed: 1.0,
            open: 0.0
        }
    );
    assert_eq!(
        bed_mix(1.0, 1.0, 1.0, 1.0),
        BedMix {
            sealed: 0.0,
            open: 1.0
        }
    );
    let half = bed_mix(0.8, 0.5, 0.5, 0.75);
    assert!((half.open - 0.2).abs() < 1e-9, "{half:?}");
    assert!((half.sealed - 0.3).abs() < 1e-9, "{half:?}");
    assert_eq!(road_mask(0.0), 1.0);
    assert_eq!(road_mask(30.0), 0.5);
    assert_eq!(road_mask(45.0), 0.5);
    assert!((road_mask(15.0) - 0.75).abs() < 1e-9);
    assert_eq!(road_mask_to(30.0, 0.8), 0.8);
    assert_eq!(road_mask_to(0.0, 0.8), 1.0);
}

#[test]
fn the_closed_window_ducks_insects_below_road_at_highway_speed() {
    const { assert!(INSECTS.sealed_road_floor < ROAD_FLOOR && INSECTS.sealed_road_floor > 0.0) };
    assert_eq!(BIRDS.sealed_road_floor, ROAD_FLOOR);
}

#[test]
fn the_fader_fades_one_bed_out_before_the_next_comes_in() {
    let f = BedFader::default().next(Some(BIRDS), 1.5, 3.0, 3.0);
    assert_eq!(f.bed, Some(BIRDS));
    assert!((f.level - 0.5).abs() < 1e-9);
    let f = f.next(Some(BIRDS), 3.0, 3.0, 3.0);
    assert_eq!(f.level, 1.0);
    // Night falls: the birds fade out first, the insects wait for them.
    let f = f.next(Some(INSECTS), 1.5, 3.0, 3.0);
    assert_eq!(f.bed, Some(BIRDS));
    assert!((f.level - 0.5).abs() < 1e-9);
    let f = f.next(Some(INSECTS), 1.5, 3.0, 3.0);
    assert_eq!(f, BedFader::default());
    let f = f.next(Some(INSECTS), 0.3, 3.0, 3.0);
    assert_eq!(f.bed, Some(INSECTS));
    // Quiet: fade out and let go.
    let f = f.next(None, 3.0, 3.0, 3.0);
    assert_eq!(f, BedFader::default());
}
