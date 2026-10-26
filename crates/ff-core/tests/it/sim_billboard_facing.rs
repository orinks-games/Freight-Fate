//! A placed billboard faces the traffic it was written for.
//!
//! Its copy says "ahead" or "next exit", so heard from the other side of the
//! road it is false: the Wall Drug countdown used to play after the exit to
//! anyone driving east from Rapid City, and "Meridian is ahead" played just
//! after leaving Meridian. These drive each direction through the two
//! countdowns and check every sign is read on the way to its attraction.

use ff_core::data::world_models::{Landmark, Route};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::vehicle::TruckState;

use crate::sim_support::{route_from_cities, weather, with_corridor, world};

fn trip_with(opts: TripOptions, route: Route) -> Trip {
    let mut truck = TruckState::default();
    truck.transmission.automatic = true;
    Trip::new(
        route,
        truck,
        weather("great_lakes", 1),
        TripOptions {
            world: Some(world()),
            ..opts
        },
    )
}

fn trip_on(route: Route) -> Trip {
    trip_with(TripOptions::seeded(1), route)
}

fn trip_through(cities: &[&str]) -> Trip {
    trip_on(route_from_cities(world(), cities))
}

/// The placed signs whose name starts with `series`, in the order heard.
fn countdown(trip: &Trip, series: &str) -> Vec<(f64, String, String)> {
    let mut signs: Vec<_> = trip
        .landmarks
        .iter()
        .filter(|c| c.category == "billboard_sign" && c.key.contains(series))
        .map(|c| (c.at_mi, c.key.clone(), c.spoken.clone()))
        .collect();
    signs.sort_by(|a, b| a.0.total_cmp(&b.0));
    signs
}

fn assert_counts_down(cities: &[&str], signs: &[(f64, String, String)]) {
    let order: Vec<_> = signs
        .iter()
        .map(|(_, key, _)| {
            ["sign one", "sign two", "sign three"]
                .into_iter()
                .find(|n| key.contains(n))
                .unwrap_or(key)
        })
        .collect();
    assert_eq!(
        order,
        ["sign one", "sign two", "sign three"],
        "{cities:?}: {signs:?}"
    );
    assert!(signs[2].2.contains("next exit"), "{cities:?}: {signs:?}");
}

#[test]
fn test_wall_drug_counts_down_to_the_exit_from_either_side() {
    for cities in [
        ["mitchell_sd_us", "rapid_city_sd_us"],
        ["rapid_city_sd_us", "mitchell_sd_us"],
        ["sioux_falls_sd_us", "rapid_city_sd_us"],
        ["rapid_city_sd_us", "sioux_falls_sd_us"],
    ] {
        let trip = trip_through(&cities);
        let wall = trip
            .stops
            .iter()
            .find(|s| s.name == "Wall Drug")
            .unwrap_or_else(|| panic!("{cities:?}: no Wall Drug stop"))
            .at_mi;
        let signs = countdown(&trip, "Wall Drug (");
        assert_counts_down(&cities, &signs);
        let (next_exit, _, _) = &signs[2];
        assert!(
            *next_exit < wall && wall - next_exit < 3.0,
            "{cities:?}: next exit read at mile {next_exit:.1}, Wall Drug at {wall:.1}"
        );
    }
}

#[test]
fn test_south_of_the_border_counts_down_both_ways() {
    for cities in [
        [
            "savannah_ga_us",
            "florence_sc_us",
            "lumberton_nc_us",
            "fayetteville_nc_us",
        ],
        [
            "fayetteville_nc_us",
            "lumberton_nc_us",
            "florence_sc_us",
            "savannah_ga_us",
        ],
    ] {
        let trip = trip_through(&cities);
        assert_counts_down(&cities, &countdown(&trip, "South of the Border ("));
    }
}

#[test]
fn test_a_placed_sign_is_not_read_from_the_far_side_of_the_road() {
    let meridian_ahead = |trip: &Trip| {
        trip.landmarks
            .iter()
            .any(|c| c.spoken.contains("Meridian is ahead"))
    };
    assert!(meridian_ahead(&trip_through(&[
        "mobile_al_us",
        "meridian_ms_us"
    ])));
    assert!(!meridian_ahead(&trip_through(&[
        "meridian_ms_us",
        "mobile_al_us"
    ])));

    // Written westbound, on a leg stored eastbound: heard leaving Amarillo,
    // a mile and a half before the ranch, not on the far side of the state.
    let trip = trip_through(&["amarillo_tx_us", "tucumcari_nm_us"]);
    let ranch = countdown(&trip, "Cadillac Ranch");
    assert_eq!(ranch.len(), 1, "{ranch:?}");
    assert!(ranch[0].0 < 10.0, "{ranch:?}");
}

/// Lines that used to be read anywhere in their state, now placed at the one
/// place they name: heard once each way past it.
#[test]
fn test_moved_pool_lines_are_heard_at_their_place_both_ways() {
    for (series, a, b) in [
        ("Cabazon Dinosaurs (", "riverside_ca_us", "indio_ca_us"),
        ("Alien Fresh Jerky (", "barstow_ca_us", "las_vegas_nv_us"),
        (
            "Little America (",
            "rock_springs_wy_us",
            "salt_lake_city_ut_us",
        ),
        ("The Grapevine (", "bakersfield_ca_us", "los_angeles_ca_us"),
        ("Hope, Arkansas (", "little_rock_ar_us", "texarkana_ar_us"),
        (
            "World's Largest Rocking Chair (",
            "indianapolis_in_us",
            "st_louis_mo_us",
        ),
    ] {
        for cities in [[a, b], [b, a]] {
            let heard = countdown(&trip_through(&cities), series);
            assert_eq!(heard.len(), 1, "{cities:?}: {heard:?}");
        }
    }
}

/// A placed billboard the landmark spacing thins away is authored copy nobody
/// ever hears. Every one must survive a drive down its own leg in the
/// direction it faces unless its leg-mile falls inside a statutory scenic ban.
#[test]
#[cfg_attr(
    ci_quick,
    ignore = "sweep: every placed billboard on every leg, both ways"
)]
fn test_every_placed_billboard_is_heard_on_its_own_leg() {
    let codes: std::collections::HashMap<_, _> = world()
        .cities
        .values()
        .map(|c| (c.state.clone(), c.state_code.clone()))
        .collect();
    let mut silent = Vec::new();
    let mut banned = Vec::new();
    for leg in &world().legs {
        for forward in [true, false] {
            let names: Vec<(String, bool)> = leg
                .landmarks()
                .iter()
                .filter(|l| l.category == "billboard_sign" && l.applies_to_direction(forward))
                .map(|l| {
                    (
                        l.name.clone(),
                        leg.billboard_bans()
                            .iter()
                            .any(|ban| ban.from_mi <= l.at_mi && l.at_mi < ban.to_mi),
                    )
                })
                .collect();
            if names.is_empty() {
                continue;
            }
            let (a, b) = if forward {
                (&leg.a, &leg.b)
            } else {
                (&leg.b, &leg.a)
            };
            let trip = trip_on(Route::new(vec![a.clone(), b.clone()], vec![leg.clone()]));
            for (name, scenic_ban) in names {
                let suffix = format!(":{name}");
                if scenic_ban {
                    assert!(
                        trip.landmarks.iter().all(|c| !c.key.ends_with(&suffix)),
                        "{a} -> {b}: scenic-banned sign {name} was heard"
                    );
                    continue;
                }
                match trip.landmarks.iter().find(|c| c.key.ends_with(&suffix)) {
                    None => silent.push(format!("{a} -> {b}: {name}")),
                    Some(callout) => {
                        let state = trip.state_at(Some(callout.at_mi));
                        let code = codes.get(&state).cloned().unwrap_or(state);
                        if ["ME", "VT", "AK", "HI"].contains(&code.as_str()) {
                            banned.push(format!("{a} -> {b}: {name} in {code}"));
                        }
                    }
                }
            }
        }
    }
    assert!(
        silent.is_empty(),
        "{} placed billboards are never heard:\n{}",
        silent.len(),
        silent.join("\n")
    );
    assert!(
        banned.is_empty(),
        "billboards in ban states:\n{}",
        banned.join("\n")
    );
}

#[test]
fn test_washington_i90_scenic_span_silences_billboards() {
    let leg = world()
        .legs
        .iter()
        .find(|leg| {
            [leg.a.as_str(), leg.b.as_str()].contains(&"seattle_wa_us")
                && [leg.a.as_str(), leg.b.as_str()].contains(&"spokane_wa_us")
        })
        .expect("Seattle–Spokane I-90 leg");
    let ban = leg
        .billboard_bans()
        .iter()
        .find(|ban| ban.name.contains("East Sunset Way") && ban.name.contains("Thorp Road"))
        .expect("Washington I-90 scenic span");
    assert!(
        ban.source.contains("RCW 47.39.020"),
        "scenic span needs its statutory source: {}",
        ban.source
    );
    let scenic_sign = with_corridor(leg, |detail| {
        detail.landmarks.push(Landmark {
            name: "Scenic span test sign".to_string(),
            at_mi: (ban.from_mi + ban.to_mi) / 2.0,
            category: "billboard_sign".to_string(),
            kind: "point".to_string(),
            spoken: "test sign".to_string(),
            ..Landmark::default()
        });
    });

    for forward in [true, false] {
        let (from, to, span_from, span_to) = if forward {
            (leg.a.clone(), leg.b.clone(), ban.from_mi, ban.to_mi)
        } else {
            (
                leg.b.clone(),
                leg.a.clone(),
                leg.miles - ban.to_mi,
                leg.miles - ban.from_mi,
            )
        };
        let trip = trip_on(Route::new(
            vec![from.clone(), to.clone()],
            vec![scenic_sign.clone().into()],
        ));
        let in_span = |at_mi: f64| span_from <= at_mi && at_mi < span_to;
        assert!(
            trip.billboards.iter().any(|callout| !in_span(callout.at_mi)),
            "{from} -> {to}: seeded trip should still schedule pool billboards outside the scenic span"
        );
        assert!(
            trip.billboards
                .iter()
                .all(|callout| !in_span(callout.at_mi)),
            "{from} -> {to}: random-pool billboard landed inside {span_from}/{span_to} miles"
        );
        assert!(
            trip.landmarks
                .iter()
                .filter(|callout| callout.category == "billboard_sign")
                .all(|callout| !in_span(callout.at_mi)),
            "{from} -> {to}: placed billboard landed inside {span_from}/{span_to} miles"
        );
        assert!(
            trip.landmarks
                .iter()
                .all(|callout| !callout.key.ends_with(":Scenic span test sign")),
            "{from} -> {to}: placed scenic-banned test sign was heard"
        );
    }
}

/// Bowlin's The Thing counts down from both sides of Exit 322, and a rig with
/// a trailer can pull in to see it but only a bobtail can fuel there.
#[test]
fn test_the_thing_counts_down_both_ways_and_fuels_bobtails_only() {
    for (cities, series, count) in [
        (
            ["las_cruces_nm_us", "tucson_az_us"],
            "The Thing (countdown",
            3,
        ),
        (["el_paso_tx_us", "tucson_az_us"], "The Thing (countdown", 3),
        (
            ["tucson_az_us", "las_cruces_nm_us"],
            "The Thing (eastbound",
            2,
        ),
        (["tucson_az_us", "el_paso_tx_us"], "The Thing (eastbound", 2),
    ] {
        let trip = trip_through(&cities);
        let visit = trip
            .stops
            .iter()
            .find(|s| s.name == "The Thing")
            .unwrap_or_else(|| panic!("{cities:?}: no stop at The Thing"));
        assert!(!visit.actions.iter().any(|a| a == "fuel"), "{cities:?}");
        let pumps = |trip: &Trip| trip.stops.iter().any(|s| s.name == "Shell at The Thing");
        assert!(!pumps(&trip), "{cities:?}: a trailer was offered the pumps");
        let bobtail = trip_with(
            TripOptions {
                bobtail: true,
                ..TripOptions::seeded(1)
            },
            route_from_cities(world(), &cities),
        );
        assert!(
            pumps(&bobtail),
            "{cities:?}: a bobtail was not offered the pumps"
        );

        let signs = countdown(&trip, series);
        assert_eq!(signs.len(), count, "{cities:?}: {signs:?}");
        for (at, _, text) in &signs {
            assert!(*at < visit.at_mi, "{cities:?}: {text} read after the exit");
        }
    }
}

/// US 1 south of Florida City carries no billboards (the 18-Mile Stretch's
/// conservation land, then the Florida Keys Scenic Highway and Monroe
/// County's ban on off-premises signs). The Keys signs kept their copy as
/// roadside callouts, owner 2026-09-30; each must still be heard.
#[test]
fn test_the_keys_hear_their_sights_but_carry_no_billboards() {
    let trip = trip_through(&["miami_fl_us", "key_west_fl_us"]);
    let in_the_keys = |at: f64| at > 40.0;
    let billboards: Vec<_> = trip
        .landmarks
        .iter()
        .filter(|c| c.category == "billboard_sign" && in_the_keys(c.at_mi))
        .map(|c| c.key.clone())
        .collect();
    assert!(billboards.is_empty(), "{billboards:?}");
    for sight in [
        "Giant Lobster Betsy",
        "Theater of the Sea",
        "Seven Mile Bridge",
        "No Name Pub",
        "Key Lime Pie on a Stick",
        "Hemingway House",
    ] {
        let suffix = format!(":{sight}");
        let heard = trip
            .landmarks
            .iter()
            .find(|c| c.key.ends_with(&suffix))
            .unwrap_or_else(|| panic!("{sight} is never heard"));
        assert!(!heard.spoken.starts_with("Billboard"), "{}", heard.spoken);
    }
}

#[test]
fn test_a_placed_sign_ends_in_one_period_not_two() {
    // Heard on the agent drive to Uvalde (2026-10-06): the Gruene Hall sign's
    // copy already ends in a sentence, and placing it added a second period,
    // which a screen reader can voice as "dot dot".
    for cities in [
        ["san_antonio_tx_us", "austin_tx_us"],
        ["austin_tx_us", "san_antonio_tx_us"],
    ] {
        let trip = trip_through(&cities);
        assert!(
            trip.landmarks
                .iter()
                .any(|c| c.spoken.contains("Gruene Hall")),
            "{cities:?}: no Gruene Hall sign"
        );
        for callout in &trip.landmarks {
            assert!(
                !callout.spoken.ends_with(".."),
                "{cities:?}: {}",
                callout.spoken
            );
            assert!(
                callout.spoken.ends_with(['.', '!', '?']),
                "{cities:?}: {}",
                callout.spoken
            );
        }
    }
}
