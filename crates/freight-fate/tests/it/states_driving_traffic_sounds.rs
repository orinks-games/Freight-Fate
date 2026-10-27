//! `states/driving_traffic_sounds.rs`: the traffic you hear follows the NPC
//! vehicles the bubble actually holds (owner, 2026-10-08).

use ff_core::models::business::LEASED_OWNER_OPERATOR;
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::sim::cross_traffic::{CrossTraffic, CrossVehicle, CROSS_CLASSES};
use ff_core::sim::traffic_manager::TrafficVehicle;

use freight_fate::app::testing::{AudioLog, RecordedLoop, TestApp};
use freight_fate::audio::{CH_TRAFFIC_BED, CH_TRAFFIC_SOUNDS};
use freight_fate::states::driving::DrivingState;
use freight_fate::states::driving_core::*;
use freight_fate::states::driving_traffic_sounds::traffic_sound_key;

const FRAME: f64 = 1.0 / 60.0;

/// Buffalo to Rochester on the Thruway, the truck at mile 20 at 60.
fn a_freeway_drive(app: &mut TestApp) -> (DrivingState, AudioLog) {
    let world = app.ctx.world;
    let mut profile = Profile::named_in("Listener", "Buffalo");
    profile.tutorial_done = true;
    profile.business_status = LEASED_OWNER_OPERATOR.to_string();
    app.ctx.profile = Some(profile);
    let route = world
        .supported_route("Buffalo", "Rochester", None)
        .expect("the world routes")
        .expect("Buffalo to Rochester has a route");
    let job = Job::new(
        CARGO_CATALOG
            .get("general")
            .expect("the general cargo type"),
        12.0,
        "Buffalo",
        "yard",
        "Rochester",
        200.0,
        900.0,
        12.0,
    );
    let mut drive = DrivingState::new(
        &mut app.ctx,
        job,
        route,
        Some(99),
        DRIVE_PHASE_DELIVERY,
        Some(10.0),
    );
    drive.trip.truck.set_air_ready(false);
    drive.trip.position_mi = 20.0;
    drive.trip.truck.velocity_mps = 60.0 / 2.23694;
    drive.trip.set_npc_vehicles(Vec::new());
    drive.trip.traffic_manager.player_lane = 0;
    let log = app.record_audio();
    (drive, log)
}

fn car(
    key: &str,
    ft_ahead: f64,
    mph: f64,
    lane: i64,
    class: &str,
    drive: &DrivingState,
) -> TrafficVehicle {
    TrafficVehicle::new(
        key,
        drive.trip.position_mi + ft_ahead / 5280.0,
        mph,
        mph,
        -lane,
        "passing",
        class,
    )
    .with_lane(lane)
}

fn sounds(log: &AudioLog) -> Vec<RecordedLoop> {
    let log = log.borrow();
    CH_TRAFFIC_SOUNDS
        .iter()
        .filter_map(|ch| log.loops.get(ch).cloned())
        .collect()
}

fn sound_on(log: &AudioLog, channel: u32) -> Option<RecordedLoop> {
    log.borrow().loops.get(&channel).cloned()
}

#[test]
fn test_a_car_alongside_on_the_left_is_heard_on_the_left() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_freeway_drive(&mut app);
    drive.trip.traffic_manager.vehicles = vec![car("probe:left", 0.0, 70.0, 1, "car", &drive)];
    drive.update_traffic_sounds(&mut app.ctx, FRAME);
    let heard = sounds(&log);
    assert_eq!(heard.len(), 1, "{heard:?}");
    assert_eq!(heard[0].key, "traffic/car_loop");
    assert!(heard[0].pan < -0.5, "{heard:?}");
    assert!(heard[0].volume > 0.3, "{heard:?}");
}

#[test]
fn test_each_class_has_its_own_sound() {
    for (class, expected) in [
        ("car", "traffic/car_loop"),
        ("box truck", "traffic/box_truck_loop"),
        ("service vehicle", "traffic/box_truck_loop"),
        ("semi", "traffic/semi_loop"),
        ("state trooper", "traffic/trooper_loop"),
    ] {
        assert_eq!(traffic_sound_key(class), expected, "{class}");
    }
    // Every crossroad class too: none falls back to the car by accident.
    for (class, _, _) in CROSS_CLASSES {
        let key = traffic_sound_key(class);
        assert!(class == "car" || key != "traffic/car_loop", "{class}");
    }
}

#[test]
fn test_every_sound_is_in_the_sound_pack() {
    let pack = ff_core::assets_pack::open_default().expect("the committed sound pack");
    let mut keys: Vec<&str> = CROSS_CLASSES
        .iter()
        .map(|(class, _, _)| traffic_sound_key(class))
        .collect();
    keys.extend(["state trooper", "semi", "box truck"].map(traffic_sound_key));
    keys.push("traffic/highway_bed");
    for key in keys {
        assert!(
            pack.has(&format!("{key}.ogg")),
            "{key} is not in sounds.pak"
        );
    }
}

#[test]
fn test_a_pass_swells_alongside_and_drops_in_pitch_going_away() {
    // The sound follows the car: quiet behind, loudest beside the cab,
    // quiet again ahead -- and closing reads higher than leaving.
    let mut app = TestApp::new();
    let (mut drive, log) = a_freeway_drive(&mut app);
    let mut samples = Vec::new();
    for ft in [-300.0, -100.0, 0.0, 100.0, 300.0] {
        drive.trip.traffic_manager.vehicles = vec![car("probe:pass", ft, 75.0, 1, "semi", &drive)];
        drive.update_traffic_sounds(&mut app.ctx, FRAME);
        let heard = sounds(&log);
        assert_eq!(heard.len(), 1, "{ft}: {heard:?}");
        samples.push(heard[0].clone());
    }
    assert!(samples[0].volume < samples[1].volume && samples[1].volume < samples[2].volume);
    assert!(samples[2].volume > samples[3].volume && samples[3].volume > samples[4].volume);
    assert!(samples[0].rate > samples[4].rate, "{samples:?}");
    // Hard left beside the cab, nearer the middle far up or down the road.
    assert!(samples[2].pan < samples[0].pan && samples[2].pan < samples[4].pan);
}

#[test]
fn test_a_sound_stays_on_its_vehicle_and_goes_quiet_when_it_leaves() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_freeway_drive(&mut app);
    drive.trip.traffic_manager.vehicles = vec![
        car("probe:a", -40.0, 70.0, 1, "car", &drive),
        car("probe:b", 200.0, 60.0, 0, "semi", &drive),
    ];
    drive.update_traffic_sounds(&mut app.ctx, FRAME);
    let slots = drive.traffic_sounds.clone();
    let semi_slot = slots
        .iter()
        .position(|slot| slot.as_deref() == Some("main:probe:b"))
        .expect("the semi has a sound");
    // The car pulls up past the semi: nearest-first order changes, slots do not.
    drive.trip.traffic_manager.vehicles[0].position_mi = drive.trip.position_mi + 250.0 / 5280.0;
    drive.update_traffic_sounds(&mut app.ctx, FRAME);
    assert_eq!(drive.traffic_sounds, slots);
    assert_eq!(
        sound_on(&log, CH_TRAFFIC_SOUNDS[semi_slot]).map(|v| v.key),
        Some("traffic/semi_loop".to_string())
    );
    // The semi drops a long way back: its sound stops.
    drive.trip.traffic_manager.vehicles[1].position_mi = drive.trip.position_mi - 0.5;
    drive.update_traffic_sounds(&mut app.ctx, FRAME);
    assert!(sound_on(&log, CH_TRAFFIC_SOUNDS[semi_slot]).is_none());
    assert_eq!(sounds(&log).len(), 1);
}

#[test]
fn test_only_the_nearest_three_have_sounds() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_freeway_drive(&mut app);
    drive.trip.traffic_manager.vehicles = (0..6)
        .map(|i| {
            car(
                &format!("probe:{i}"),
                30.0 + 50.0 * i as f64,
                70.0,
                1,
                "car",
                &drive,
            )
        })
        .collect();
    drive.update_traffic_sounds(&mut app.ctx, FRAME);
    assert_eq!(sounds(&log).len(), 3);
    let followed: Vec<String> = drive.traffic_sounds.iter().flatten().cloned().collect();
    for near in ["main:probe:0", "main:probe:1", "main:probe:2"] {
        assert!(followed.iter().any(|id| id == near), "{followed:?}");
    }
}

#[test]
fn test_on_an_exit_ramp_the_mainline_is_off_to_the_left_and_fades() {
    // The odometer holds on a ramp, so a car in the truck's own lane used to
    // "pass" straight through the cab, dead centre.
    let mut app = TestApp::new();
    let (mut drive, log) = a_freeway_drive(&mut app);
    drive.ramp_mi = Some(0.4);
    drive.trip.traffic_manager.vehicles = vec![car("probe:main", 0.0, 65.0, 0, "car", &drive)];
    drive.update_traffic_sounds(&mut app.ctx, FRAME);
    let at_the_gore = sounds(&log);
    assert_eq!(at_the_gore.len(), 1);
    assert!(at_the_gore[0].pan < -0.5, "{at_the_gore:?}");
    // A few hundred feet down the ramp the freeway is fainter...
    for _ in 0..240 {
        drive.update_traffic_sounds(&mut app.ctx, FRAME);
    }
    let down_the_ramp = sounds(&log);
    assert!(
        down_the_ramp.is_empty() || down_the_ramp[0].volume < at_the_gore[0].volume / 2.0,
        "{at_the_gore:?} then {down_the_ramp:?}"
    );
    // ...and back on the mainline the sounds read lanes again.
    drive.ramp_mi = None;
    drive.update_traffic_sounds(&mut app.ctx, FRAME);
    assert_eq!(drive.traffic_ramp_rolled_ft, 0.0);
}

fn a_crossing(position_ft: f64, from_side: &'static str) -> CrossTraffic {
    let mut bubble = CrossTraffic::new(1, "stop", false);
    bubble.vehicles = vec![CrossVehicle {
        position_mi: position_ft / 5280.0,
        speed_mph: 35.0,
        target_mph: 35.0,
        vehicle_class: "pickup",
        length_mi: 18.0 / 5280.0,
        from_side,
        crossed: false,
        committed: false,
        sound_started: false,
        id: 7,
    }];
    bubble
}

#[test]
fn test_cross_traffic_sweeps_from_the_side_it_came_from() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_freeway_drive(&mut app);
    drive.ramp_mi = Some(RAMP_ACCESS_MI); // at the bar
    drive.ramp_terminal_done = false;
    let mut heard = Vec::new();
    for ft in [-200.0, 0.0, 200.0] {
        drive.cross_bubble = Some(a_crossing(ft, "left"));
        drive.update_traffic_sounds(&mut app.ctx, FRAME);
        let crossing: Vec<RecordedLoop> = sounds(&log)
            .into_iter()
            .filter(|v| v.key == "traffic/pickup_loop")
            .collect();
        assert_eq!(crossing.len(), 1, "{ft}: {crossing:?}");
        heard.push(crossing[0].clone());
    }
    assert!(heard[0].pan < -0.5 && heard[2].pan > 0.5, "{heard:?}");
    assert!(heard[1].pan.abs() < 0.1, "{heard:?}");
    assert!(heard[1].volume > heard[0].volume && heard[1].volume > heard[2].volume);
    assert!(heard[0].rate > heard[2].rate, "{heard:?}");
    assert!(drive
        .cross_bubble
        .as_ref()
        .is_some_and(|b| b.vehicles[0].sound_started));

    // From the right it is the mirror image.
    drive.cross_bubble = Some(a_crossing(-200.0, "right"));
    drive.update_traffic_sounds(&mut app.ctx, FRAME);
    let from_right = sounds(&log)
        .into_iter()
        .find(|v| v.key == "traffic/pickup_loop")
        .expect("the pickup is heard");
    assert!(from_right.pan > 0.5, "{from_right:?}");
}

#[test]
fn test_the_interstate_has_distant_traffic_under_the_sounds() {
    let mut app = TestApp::new();
    let (mut drive, log) = a_freeway_drive(&mut app);
    assert!(
        drive.traffic_bed_target() > 0.0,
        "Buffalo to Rochester runs the Thruway"
    );
    for _ in 0..600 {
        drive.update_traffic_sounds(&mut app.ctx, FRAME);
    }
    let bed = sound_on(&log, CH_TRAFFIC_BED).expect("the freeway bed is playing");
    assert_eq!(bed.key, "traffic/highway_bed");
    assert!(
        (bed.volume - drive.traffic_bed_target()).abs() < 0.02,
        "{bed:?}"
    );
    drive.reset_traffic_sounds(&mut app.ctx);
    assert!(sound_on(&log, CH_TRAFFIC_BED).is_none());
}

#[test]
fn test_a_car_one_lane_over_is_on_its_side_well_before_it_draws_level() {
    // The true angle put a car one lane over and 100 feet up the road 10
    // percent off centre: the owner heard traffic as mono (2026-10-09).
    let mut app = TestApp::new();
    let (mut drive, log) = a_freeway_drive(&mut app);
    drive.trip.traffic_manager.vehicles = vec![car("probe:ahead", 100.0, 70.0, 1, "car", &drive)];
    drive.update_traffic_sounds(&mut app.ctx, FRAME);
    let ahead = sounds(&log);
    assert!(ahead[0].pan < -0.3, "{ahead:?}");
    // The same car the same distance behind is on the same side, quieter:
    // stereo alone cannot say which end of the truck it is at.
    drive.trip.traffic_manager.vehicles = vec![car("probe:behind", -100.0, 70.0, 1, "car", &drive)];
    drive.update_traffic_sounds(&mut app.ctx, FRAME);
    let behind = sounds(&log);
    assert!(behind[0].pan < -0.3, "{behind:?}");
    assert!(
        behind[0].volume < ahead[0].volume * 0.8,
        "{ahead:?} {behind:?}"
    );
}

#[test]
fn test_the_car_the_game_calls_out_ahead_takes_a_sound_first() {
    // With the lane beside full, nearer cars took all three sounds and the
    // car the game had just named stayed silent (owner drive, 2026-10-09).
    let mut app = TestApp::new();
    let (mut drive, log) = a_freeway_drive(&mut app);
    drive.trip.traffic_manager.vehicles = vec![
        car("probe:beside0", 100.0, 70.0, 1, "car", &drive),
        car("probe:beside1", 200.0, 70.0, 1, "car", &drive),
        car("probe:beside2", -150.0, 70.0, 1, "car", &drive),
        car("probe:lead", 2000.0, 45.0, 0, "box truck", &drive),
    ];
    drive.update_traffic_sounds(&mut app.ctx, FRAME);
    let followed: Vec<String> = drive.traffic_sounds.iter().flatten().cloned().collect();
    assert!(
        followed.iter().any(|id| id == "main:probe:lead"),
        "{followed:?}"
    );
    assert!(
        sounds(&log)
            .iter()
            .any(|s| s.key == "traffic/box_truck_loop"),
        "{:?}",
        sounds(&log)
    );
}
