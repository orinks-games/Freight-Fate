//! Each 2.0 badge, earned from the event that awards it, on the real map.

use std::collections::BTreeSet;

use serde_json::{Map, Value};

use super::*;
use crate::achievements::{achievement_by_id, ACHIEVEMENTS};
use crate::data::world::get_world;
use crate::sim::hos::HosClock;

#[derive(Default)]
struct TestProfile {
    achievements: Vec<String>,
    achievement_stats: Map<String, Value>,
}

impl AchievementProfile for TestProfile {
    fn achievements(&self) -> &[String] {
        &self.achievements
    }
    fn achievements_mut(&mut self) -> &mut Vec<String> {
        &mut self.achievements
    }
    fn achievement_stats_mut(&mut self) -> &mut Map<String, Value> {
        &mut self.achievement_stats
    }
}

fn route(from: &str, to: &str) -> Route {
    get_world()
        .shortest_route(from, to, None, false)
        .expect("known cities")
        .expect("a road between them")
}

fn delivery<'a>(route: &'a Route, season: &'a str) -> DriveEnd<'a> {
    DriveEnd {
        route,
        destination: route.cities.last().map(String::as_str).unwrap_or(""),
        bobtail: false,
        team: false,
        season,
        spring_weight_limit: false,
        border_yard_drop: false,
        cattle: None,
        carrier_key: "",
        game_hours: 0.0,
    }
}

fn earned(end: &DriveEnd) -> Vec<&'static str> {
    drive_end_badges(&mut TestProfile::default(), get_world(), end)
}

#[test]
fn every_new_badge_is_in_the_catalog() {
    for id in [
        "alaska_delivery",
        "anchorage_delivery",
        "deadhorse_delivery",
        "whittier_tunnel",
        "alaska_highway_whole",
        "fairbanks_spring",
        "dalton_winter",
        "spring_weight_load",
        "first_customs",
        "restart_34",
        "restart_at_home",
        "carrier_first_week",
        "carrier_rehired",
        "carrier_step_up",
        "cattle_load",
        "cattle_ten",
        "laredo_transfer",
    ] {
        assert!(achievement_by_id(id).is_some(), "{id}");
    }
}

#[test]
fn no_song_inspires_two_badges() {
    let mut seen = std::collections::HashSet::new();
    for achievement in &ACHIEVEMENTS {
        let song = achievement.inspiration.to_lowercase();
        assert!(
            seen.insert(song),
            "{}: {}",
            achievement.id,
            achievement.inspiration
        );
    }
}

#[test]
fn a_delivery_to_anchorage_is_an_alaska_delivery() {
    let r = route(FAIRBANKS, ANCHORAGE);
    let ids = earned(&delivery(&r, "summer"));
    assert!(ids.contains(&"alaska_delivery"));
    assert!(ids.contains(&"anchorage_delivery"));
    let lower48 = route("seattle_wa_us", "portland_or_us");
    assert!(!earned(&delivery(&lower48, "summer")).contains(&"alaska_delivery"));
}

#[test]
fn deadhorse_in_winter_earns_the_dalton_and_the_top_of_the_world() {
    let r = route(FAIRBANKS, DEADHORSE);
    let ids = earned(&delivery(&r, "winter"));
    assert!(ids.contains(&"deadhorse_delivery"));
    assert!(ids.contains(&"dalton_winter"));
    // Summer, or the way back down, is not a winter run up the Dalton.
    assert!(!earned(&delivery(&r, "summer")).contains(&"dalton_winter"));
    let down = route(DEADHORSE, FAIRBANKS);
    assert!(!earned(&delivery(&down, "winter")).contains(&"dalton_winter"));
}

#[test]
fn fairbanks_counts_in_spring_only() {
    let r = route(ANCHORAGE, FAIRBANKS);
    assert!(earned(&delivery(&r, "spring")).contains(&"fairbanks_spring"));
    assert!(!earned(&delivery(&r, "summer")).contains(&"fairbanks_spring"));
}

#[test]
fn whittier_is_reached_only_through_the_tunnel() {
    let r = route(ANCHORAGE, WHITTIER);
    assert!(
        r.legs.iter().any(|leg| leg.corridor().tunnel.is_some()),
        "the road to Whittier runs the tunnel"
    );
    let mut bobtail = delivery(&r, "summer");
    bobtail.bobtail = true;
    let ids = earned(&bobtail);
    assert!(ids.contains(&"whittier_tunnel"));
    // An empty run delivers nothing.
    assert!(!ids.contains(&"alaska_delivery"));
    bobtail.team = true;
    assert!(!earned(&bobtail).contains(&"whittier_tunnel"));
}

#[test]
fn the_alaska_highway_counts_every_leg_across_drives() {
    let world = get_world();
    let mut profile = TestProfile::default();
    // Half the road on one drive, the rest on another, the second one
    // driven the other way.
    let south = route(ALASKA_HIGHWAY_START, "watson_lake_yt_ca");
    let ids = drive_end_badges(&mut profile, world, &delivery(&south, "summer"));
    assert!(!ids.contains(&"alaska_highway_whole"));
    let north = route(ALASKA_HIGHWAY_END, "watson_lake_yt_ca");
    let ids = drive_end_badges(&mut profile, world, &delivery(&north, "summer"));
    assert!(
        ids.contains(&"alaska_highway_whole"),
        "{:?}",
        profile.achievement_stats
    );
}

#[test]
fn a_through_leg_and_its_split_legs_both_join_the_road() {
    let pairs = |legs: &[(&str, &str)]| -> BTreeSet<String> {
        legs.iter().map(|(a, b)| leg_pair(a, b)).collect()
    };
    let south = [
        (ALASKA_HIGHWAY_START, "fort_st_john_bc_ca"),
        ("fort_st_john_bc_ca", "fort_nelson_bc_ca"),
        ("fort_nelson_bc_ca", "watson_lake_yt_ca"),
        ("watson_lake_yt_ca", "whitehorse_yt_ca"),
    ];
    let north = ("tok_ak_us", ALASKA_HIGHWAY_END);
    // Whitehorse to Tok by the through leg.
    let mut through = pairs(&south);
    through.extend(pairs(&[("whitehorse_yt_ca", "tok_ak_us"), north]));
    assert!(alaska_highway_joined(&through));
    // The same stretch by the split legs, with the through leg never driven.
    let mut split = pairs(&south);
    split.extend(pairs(&[
        ("whitehorse_yt_ca", "haines_junction_yt_ca"),
        ("haines_junction_yt_ca", "tok_ak_us"),
        north,
    ]));
    assert!(alaska_highway_joined(&split));
    // A gap anywhere is not the whole road, nor is a side road.
    let mut gap = pairs(&south);
    gap.extend(pairs(&[("haines_junction_yt_ca", "tok_ak_us"), north]));
    assert!(!alaska_highway_joined(&gap));
    assert!(!alaska_highway_joined(&pairs(&south)));
}

#[test]
fn the_map_carries_the_whole_alaska_highway() {
    let all = alaska_highway_pairs(get_world());
    assert!(alaska_highway_joined(&all), "{all:?}");
}

#[test]
fn a_team_load_does_not_count_alaska_highway_legs() {
    let world = get_world();
    let mut profile = TestProfile::default();
    let whole = route("dawson_creek_bc_ca", "delta_junction_ak_us");
    let mut end = delivery(&whole, "summer");
    end.team = true;
    assert!(!drive_end_badges(&mut profile, world, &end).contains(&"alaska_highway_whole"));
    end.team = false;
    assert!(drive_end_badges(&mut profile, world, &end).contains(&"alaska_highway_whole"));
}

#[test]
fn spring_weight_limits_and_the_laredo_yard_follow_the_load() {
    let r = route("valdez_ak_us", "glennallen_ak_us");
    let mut end = delivery(&r, "spring");
    assert!(!earned(&end).contains(&"spring_weight_load"));
    end.spring_weight_limit = true;
    assert!(earned(&end).contains(&"spring_weight_load"));
    let laredo = route("san_antonio_tx_us", "laredo_tx_us");
    let mut end = delivery(&laredo, "summer");
    assert!(!earned(&end).contains(&"laredo_transfer"));
    end.border_yard_drop = true;
    assert!(earned(&end).contains(&"laredo_transfer"));
}

fn cattle(hours_aboard: f64, one_down: bool, clean: bool) -> Option<CattleDelivery> {
    Some(CattleDelivery {
        hours_aboard,
        limit_hours: 28.0,
        one_down,
        clean,
    })
}

#[test]
fn rawhide_needs_the_limit_kept_and_every_animal_standing() {
    let r = route("amarillo_tx_us", "dallas_tx_us");
    let mut end = delivery(&r, "summer");
    end.cattle = cattle(9.0, false, true);
    assert!(earned(&end).contains(&"cattle_load"));
    for spoiled in [
        cattle(29.0, false, true),
        cattle(9.0, true, true),
        cattle(9.0, false, false),
    ] {
        end.cattle = spoiled;
        assert!(!earned(&end).contains(&"cattle_load"), "{spoiled:?}");
    }
}

#[test]
fn cattle_call_counts_every_cattle_load_delivered() {
    let world = get_world();
    let r = route("amarillo_tx_us", "dallas_tx_us");
    let mut profile = TestProfile::default();
    let mut end = delivery(&r, "summer");
    // Other freight does not count.
    drive_end_badges(&mut profile, world, &end);
    end.cattle = cattle(30.0, true, false);
    for load in 1..=CATTLE_CALL_LOADS {
        let ids = drive_end_badges(&mut profile, world, &end);
        assert_eq!(
            ids.contains(&"cattle_ten"),
            load == CATTLE_CALL_LOADS,
            "{load}"
        );
    }
}

#[test]
fn workin_for_a_livin_takes_five_loads_across_a_week_for_one_carrier() {
    let mut profile = TestProfile::default();
    let day = 24.0;
    // Five loads in four days is not yet a week.
    for i in 0..5 {
        assert!(!carrier_run(
            &mut profile,
            "prairie_link",
            100.0 + i as f64 * day
        ));
    }
    // The sixth, a week after the first, is.
    assert!(carrier_run(&mut profile, "prairie_link", 100.0 + 7.0 * day));
    // A new carrier starts the run over.
    let mut profile = TestProfile::default();
    for i in 0..4 {
        carrier_run(&mut profile, "prairie_link", i as f64 * day);
    }
    assert!(!carrier_run(&mut profile, "northstar", 7.0 * day));
    // Own authority hauls for no carrier.
    assert!(!carrier_run(&mut profile, "", 30.0 * day));
}

#[test]
fn get_back_reads_the_record() {
    assert!(rehired_after_let_go(
        1,
        "great_lakes_training",
        "prairie_link"
    ));
    // Never let go, or let go and with no carrier yet.
    assert!(!rehired_after_let_go(0, "prairie_link", ""));
    assert!(!rehired_after_let_go(1, "", "prairie_link"));
}

#[test]
fn a_restart_takes_thirty_four_hours_off_in_a_row() {
    let mut clock = HosClock::default();
    clock.on_duty(60.0);
    // Three nights is thirty hours off: not yet a restart.
    for _ in 0..3 {
        clock.sleep();
    }
    assert!(!clock.restart_complete());
    assert!(restart_badges(clock.restart_complete(), true).is_empty());
    clock.sleep();
    assert!(clock.restart_complete());
    assert_eq!(restart_badges(true, false), vec!["restart_34"]);
    assert_eq!(
        restart_badges(true, true),
        vec!["restart_34", "restart_at_home"]
    );
    // Any time on duty starts the count over.
    clock.on_duty(15.0);
    assert!(!clock.restart_complete());
}
