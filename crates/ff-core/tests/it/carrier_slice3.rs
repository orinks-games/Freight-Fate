//! Carrier slice 3 (docs/carrier-slice3-plan.md): the regionals, the Alaska
//! regional and the locals. Coverage of the lower 48, the Alaska wall, the
//! let-go fallback rule (plan section 5) and the tier distance bands.

use ff_core::data::world::World;
use ff_core::models::carriers::{
    carrier, carrier_catalog, fallback_carrier_for, is_offerable_home_city,
    openings_for_unassigned, solvency_fallback_carrier, Carrier,
};
use ff_core::models::start_options::start_options_for_home_city;

use crate::data_support::world;

fn state(world: &World, key: &str) -> String {
    world.cities[key].state_code.to_ascii_uppercase()
}

fn is_us(world: &World, key: &str) -> bool {
    world.cities[key].country.eq_ignore_ascii_case("us")
}

fn lower_48_cities(world: &World) -> Vec<String> {
    let mut keys: Vec<String> = world
        .cities
        .keys()
        .filter(|k| is_us(world, k) && !matches!(state(world, k).as_str(), "AK" | "HI"))
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn alaska_cities(world: &World) -> Vec<String> {
    let mut keys: Vec<String> = world
        .cities
        .keys()
        .filter(|k| is_us(world, k) && state(world, k) == "AK")
        .cloned()
        .collect();
    keys.sort();
    keys
}

/// A carrier with any terminal in Alaska is an Alaska carrier; every other
/// carrier is a lower-48 carrier.
fn is_alaska_carrier(world: &World, c: &Carrier) -> bool {
    c.terminal_city_keys.iter().any(|t| state(world, t) == "AK")
}

#[test]
fn test_catalog_counts_by_tier() {
    let catalog = carrier_catalog();
    let count = |tier: &str| catalog.values().filter(|c| c.tier == tier).count();
    assert_eq!(catalog.len(), 24);
    assert_eq!(count("national"), 3);
    assert_eq!(count("regional"), 18);
    assert_eq!(count("local"), 3);
    let world = world();
    let alaska: Vec<&str> = catalog
        .values()
        .filter(|c| is_alaska_carrier(world, c))
        .map(|c| c.key.as_str())
        .collect();
    assert_eq!(alaska, ["chatanika_freight", "knik_arm_cartage"]);
}

#[test]
fn test_every_lower_48_city_is_hired_into() {
    let world = world();
    let cities = lower_48_cities(world);
    assert_eq!(cities.len(), 626, "the lower-48 map city count moved");
    let mut by_any = 0;
    let mut by_regional = 0;
    let mut missing_regional = Vec::new();
    for city in &cities {
        if is_offerable_home_city(world, city)
            && !start_options_for_home_city(world, city).is_empty()
        {
            by_any += 1;
        }
        if carrier_catalog()
            .values()
            .any(|c| c.tier == "regional" && c.hires_in(world, city))
        {
            by_regional += 1;
        } else {
            missing_regional.push(city.clone());
        }
    }
    assert_eq!(by_any, 626);
    assert_eq!(
        by_regional, 626,
        "no regional hires in {missing_regional:?}"
    );
}

#[test]
fn test_no_national_hires_outside_the_lower_48() {
    let world = world();
    for c in carrier_catalog().values().filter(|c| c.is_national()) {
        for (key, city) in &world.cities {
            let st = city.state_code.to_ascii_uppercase();
            if !city.country.eq_ignore_ascii_case("us") || matches!(st.as_str(), "AK" | "HI") {
                assert!(!c.hires_in(world, key), "{} hires in {key}", c.key);
            }
        }
    }
}

#[test]
fn test_regional_and_local_hires_stay_in_country_and_radius() {
    let world = world();
    for c in carrier_catalog().values().filter(|c| !c.is_national()) {
        let radius = c
            .hiring_radius_mi
            .expect("regional and local have a radius");
        for (key, city) in &world.cities {
            let Some(terminal) = c.hiring_terminal_city(world, key) else {
                continue;
            };
            assert!(c.terminal_city_keys.contains(&terminal));
            assert!(world.cities[&terminal]
                .country
                .eq_ignore_ascii_case(&city.country));
            let mi = c.hiring_distance_mi(world, key).unwrap();
            assert!(mi <= radius + 1e-9, "{} hires {key} at {mi} mi", c.key);
        }
    }
}

#[test]
fn test_alaska_is_served_by_alaska_carriers_only() {
    let world = world();
    for city in alaska_cities(world) {
        if world.cities[&city].no_freight {
            assert!(
                !is_offerable_home_city(world, &city),
                "{city} has no freight"
            );
            continue;
        }
        assert!(
            is_offerable_home_city(world, &city) || city == "deadhorse_ak_us",
            "{city} not offered"
        );
        for c in carrier_catalog().values() {
            if c.hires_in(world, &city) {
                assert!(is_alaska_carrier(world, c), "{} hires in {city}", c.key);
            }
        }
        for option in start_options_for_home_city(world, &city) {
            let c = carrier(option.key).expect("company start");
            assert!(is_alaska_carrier(world, c), "{} offered in {city}", c.key);
        }
    }
    for city in ["whitehorse_yt_ca", "surrey_bc_ca"] {
        assert!(!is_offerable_home_city(world, city), "{city}");
    }
}

#[test]
fn test_no_alaska_driver_goes_to_a_lower_48_carrier() {
    let world = world();
    let mut firers: Vec<&str> = carrier_catalog().keys().map(String::as_str).collect();
    firers.push("");
    for city in alaska_cities(world) {
        for firer in &firers {
            if let Some(c) = fallback_carrier_for(world, &city, firer) {
                assert!(
                    is_alaska_carrier(world, c),
                    "{city} fired by {firer} -> {}",
                    c.key
                );
                assert_ne!(c.key, solvency_fallback_carrier().key);
            }
            for opening in openings_for_unassigned(world, &city, firer) {
                assert!(
                    is_alaska_carrier(world, opening.carrier),
                    "{city} fired by {firer} offered {}",
                    opening.carrier.key
                );
                assert_eq!(state(world, &opening.terminal_city), "AK");
            }
        }
    }
}

#[test]
fn test_fallback_is_never_the_firer_and_prefers_the_nearest_regional_or_local() {
    let world = world();
    let lower_48 = lower_48_cities(world);
    for firer in carrier_catalog().keys() {
        for city in lower_48.iter().chain(alaska_cities(world).iter()) {
            let Some(c) = fallback_carrier_for(world, city, firer) else {
                continue;
            };
            assert_ne!(&c.key, firer, "{city}");
            assert!(c.hires_in(world, city), "{} does not hire in {city}", c.key);
            if c.is_national() {
                // Only when no regional or local other than the firer hires.
                assert_eq!(c.key, solvency_fallback_carrier().key);
                assert!(!carrier_catalog()
                    .values()
                    .any(|o| !o.is_national() && &o.key != firer && o.hires_in(world, city)));
            }
        }
        // Every lower-48 driver lands somewhere: the regional coverage and
        // the last-chance national guarantee it.
        for city in &lower_48 {
            assert!(
                fallback_carrier_for(world, city, firer).is_some(),
                "{city} fired by {firer} has no seat"
            );
        }
    }
}

#[test]
fn test_fallback_examples() {
    let world = world();
    // Chicago, fired by Northstar: the Chicago local's terminal is in town.
    assert_eq!(
        fallback_carrier_for(world, "chicago_il_us", "northstar").map(|c| c.key.as_str()),
        Some("des_plaines_cartage")
    );
    // Anchorage, fired by the Anchorage local: the Alaska regional.
    assert_eq!(
        fallback_carrier_for(world, "anchorage_ak_us", "knik_arm_cartage").map(|c| c.key.as_str()),
        Some("chatanika_freight")
    );
    // Fairbanks, fired by the Alaska regional: nobody else hires there, so
    // the driver is home with no carrier, and the openings offer the
    // Anchorage local with a move.
    assert!(fallback_carrier_for(world, "fairbanks_ak_us", "chatanika_freight").is_none());
    let openings = openings_for_unassigned(world, "fairbanks_ak_us", "chatanika_freight");
    assert!(!openings.is_empty());
    let knik = openings
        .iter()
        .find(|o| o.carrier.key == "knik_arm_cartage")
        .expect("Knik Arm opening");
    assert!(knik.moves_home);
    assert_eq!(knik.terminal_city, "anchorage_ak_us");
    assert_eq!(knik.home_city, "anchorage_ak_us");
    assert!(openings
        .iter()
        .all(|o| o.carrier.key != "chatanika_freight"));
    // Great Lakes Training, fired by itself, still finds a regional.
    assert!(fallback_carrier_for(world, "milwaukee_wi_us", "great_lakes_training").is_some());
}

#[test]
fn test_tier_distance_bands() {
    for c in carrier_catalog().values() {
        match c.tier.as_str() {
            "local" => {
                assert_eq!(c.hiring_radius_mi, Some(50.0), "{}", c.key);
                assert_eq!(
                    (c.run_band.min_mi, c.run_band.max_mi),
                    (25.0, 150.0),
                    "{}",
                    c.key
                );
            }
            "regional" => {
                assert_eq!(c.hiring_radius_mi, Some(250.0), "{}", c.key);
                assert_eq!(
                    (c.run_band.min_mi, c.run_band.max_mi),
                    (150.0, 600.0),
                    "{}",
                    c.key
                );
            }
            "national" => {
                assert_eq!(c.hiring_radius_mi, None, "{}", c.key);
                assert_eq!(
                    (c.run_band.min_mi, c.run_band.max_mi),
                    (400.0, 3000.0),
                    "{}",
                    c.key
                );
            }
            other => panic!("{} has unknown tier {other}", c.key),
        }
        // The board never offers a load outside the band.
        assert!(!c.run_band_allows(c.run_band.max_mi + 1.0), "{}", c.key);
        assert_eq!(c.effective_distance_cap(10_000.0), c.run_band.max_mi);
    }
}

/// Terminal cities are real map cities, and carrier-owned terminals need no
/// world yard pin. A home terminal is the carrier's own yard, synthesized
/// from `terminal_city_keys` as "{Carrier} {City} terminal"; world
/// `company_yard` and `terminal` pins are freight endpoints and never a
/// home (carrier slice 1).
#[test]
fn test_terminal_cities_are_map_cities_with_carrier_owned_terminals() {
    let world = world();
    for c in carrier_catalog().values() {
        assert!(!c.terminal_city_keys.is_empty(), "{}", c.key);
        for t in &c.terminal_city_keys {
            let city = world
                .cities
                .get(t)
                .unwrap_or_else(|| panic!("{}: {t} not a map city", c.key));
            let terminal = c
                .home_terminal(world, t)
                .unwrap_or_else(|| panic!("{}: no carrier-owned terminal in {t}", c.key));
            assert_eq!(terminal.name, format!("{} {} terminal", c.name, city.name));
            assert_eq!(terminal.city, city.name);
        }
    }
    // No world pin needed: Fairbanks has no world company_yard or terminal
    // pin, and Chatanika's Fairbanks terminal is still its own yard.
    assert!(world.default_facility("fairbanks_ak_us").is_err());
    let chatanika = carrier("chatanika_freight").expect("chatanika");
    assert_eq!(
        chatanika
            .home_terminal(world, "fairbanks_ak_us")
            .map(|t| t.name)
            .as_deref(),
        Some("Chatanika Freight Lines Fairbanks terminal")
    );
}

#[test]
fn test_carrier_names_are_unique_and_speakable() {
    let mut names: Vec<&str> = carrier_catalog()
        .values()
        .map(|c| c.name.as_str())
        .collect();
    for name in &names {
        assert!(!name.contains("St."), "{name}: spell Saint out");
    }
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(names.len(), before);
    for c in carrier_catalog().values().filter(|c| !c.is_national()) {
        assert!(
            !c.lane_area.is_empty(),
            "{} needs a spoken lane area",
            c.key
        );
    }
}

mod profile_fallback {
    use ff_core::models::carriers::openings_for_unassigned;
    use ff_core::models::enforcement::{carrier_termination_due, kept_on_sufferance};
    use ff_core::models::profile::Profile;
    use ff_core::models::solvency::{apply_company_termination, hard_capped, no_seat_line};

    use crate::data_support::world;

    fn driver(home: &str, carrier: &str, name: &str) -> Profile {
        let mut p = Profile::named("Test Driver");
        p.business_status = "company_driver".to_string();
        p.carrier_key = carrier.to_string();
        p.carrier_name = name.to_string();
        p.home_city = home.to_string();
        p.home_terminal_city = home.to_string();
        p.current_city = home.to_string();
        p
    }

    #[test]
    fn test_let_go_lands_at_the_nearest_hiring_terminal_on_sufferance() {
        let mut p = driver("chicago_il_us", "northstar", "Northstar Freight Lines");
        p.career.reputation = 5.0;
        assert!(carrier_termination_due(&p));
        let taken = p.take_fallback_after_let_go("northstar").expect("a seat");
        assert_eq!(taken.key, "des_plaines_cartage");
        assert_eq!(p.carrier_key, "des_plaines_cartage");
        assert_eq!(p.carrier_name, "Des Plaines River Cartage");
        assert_eq!(p.home_terminal_city, "chicago_il_us");
        assert_eq!(p.home_city, "chicago_il_us");
        assert_eq!(p.driving_record.let_go_by, "northstar");
        assert!(kept_on_sufferance(&p));
        // Kept on knowing the record: the same record does not end this seat.
        assert!(!carrier_termination_due(&p));
        assert!(hard_capped(&p));
    }

    #[test]
    fn test_let_go_with_no_seat_leaves_the_driver_home_unassigned() {
        let mut p = driver(
            "fairbanks_ak_us",
            "chatanika_freight",
            "Chatanika Freight Lines",
        );
        p.current_city = "anchorage_ak_us".to_string();
        assert!(p.take_fallback_after_let_go("chatanika_freight").is_none());
        assert!(p.is_unassigned_company_driver());
        assert!(p.carrier_key.is_empty() && p.carrier_name.is_empty());
        assert_eq!(p.current_city, "fairbanks_ak_us");
        assert_eq!(p.home_terminal_city, "fairbanks_ak_us");
        assert!(
            !carrier_termination_due(&p),
            "no carrier left to let them go"
        );

        // Applying: the Anchorage local, with a move to Anchorage.
        let openings = openings_for_unassigned(world(), &p.driver_home_city(), "chatanika_freight");
        let knik = openings
            .iter()
            .find(|o| o.carrier.key == "knik_arm_cartage")
            .expect("Knik Arm opening");
        p.join_carrier_opening(knik);
        assert_eq!(p.carrier_key, "knik_arm_cartage");
        assert_eq!(p.carrier_name, "Knik Arm Cartage");
        assert_eq!(p.home_city, "anchorage_ak_us");
        assert_eq!(p.home_terminal_city, "anchorage_ak_us");
        assert_eq!(p.current_city, "anchorage_ak_us");
        assert!(!p.is_unassigned_company_driver());
    }

    #[test]
    fn test_joining_with_a_bad_record_is_on_sufferance() {
        let mut p = driver("fairbanks_ak_us", "", "");
        p.career.reputation = 5.0;
        p.driving_record.let_go_by = "chatanika_freight".to_string();
        let openings = openings_for_unassigned(world(), "fairbanks_ak_us", "chatanika_freight");
        p.join_carrier_opening(&openings[0]);
        assert!(kept_on_sufferance(&p));
        assert!(!carrier_termination_due(&p));
    }

    #[test]
    fn test_debt_termination_uses_the_fallback_rule() {
        let mut p = driver("chicago_il_us", "northstar", "Northstar Freight Lines");
        p.set_money(-9_000.0);
        let lines = apply_company_termination(&mut p);
        assert_eq!(p.carrier_key, "des_plaines_cartage");
        assert!(lines
            .iter()
            .any(|l| l.contains("Des Plaines River Cartage")));

        let mut ak = driver(
            "fairbanks_ak_us",
            "chatanika_freight",
            "Chatanika Freight Lines",
        );
        ak.set_money(-9_000.0);
        let lines = apply_company_termination(&mut ak);
        assert!(ak.is_unassigned_company_driver());
        assert!(lines
            .iter()
            .any(|l| *l == no_seat_line("your home in Fairbanks")));
        assert!(!lines.iter().any(|l| l.contains("dispatch board whenever")));
    }

    #[test]
    fn test_an_old_save_without_a_home_city_keeps_it_blank() {
        use ff_core::models::business::{business_status_summary, carrier_name};
        use ff_core::models::career_objectives::{career_objective, NO_CARRIER_OBJECTIVE};

        // Parked in Milwaukee off Northstar's Chicago terminal, home blank.
        let mut p = driver("", "northstar", "Northstar Freight Lines");
        p.home_terminal_city = "chicago_il_us".to_string();
        p.current_city = "milwaukee_wi_us".to_string();
        p.set_money(-9_000.0);
        let lines = apply_company_termination(&mut p);
        assert!(p.home_city.is_empty(), "home_city written: {}", p.home_city);
        assert_eq!(p.carrier_key, "des_plaines_cartage");
        let said = lines.join(" ");
        assert!(said.contains("near your terminal in Chicago"), "{said}");
        assert!(!said.contains("your home"), "{said}");

        // No seat and no saved home: nothing written into home or current.
        let mut ak = driver("", "chatanika_freight", "Chatanika Freight Lines");
        ak.home_terminal_city = "fairbanks_ak_us".to_string();
        ak.current_city = "healy_ak_us".to_string();
        ak.set_money(-9_000.0);
        let lines = apply_company_termination(&mut ak);
        assert!(ak.is_unassigned_company_driver());
        assert!(ak.home_city.is_empty());
        assert_eq!(ak.current_city, "healy_ak_us");
        assert!(lines
            .iter()
            .any(|l| *l == no_seat_line("your terminal in Fairbanks")));
        assert!(!lines.join(" ").contains("your home"));

        // And nothing names a carrier now that there is none.
        assert_eq!(carrier_name(&ak), "");
        assert!(!business_status_summary(&ak).contains("Northstar"));
        assert_eq!(career_objective(&ak).terminal_text, NO_CARRIER_OBJECTIVE);
    }

    #[test]
    fn test_a_clean_record_on_sufferance_says_nothing_at_the_hub() {
        use ff_core::models::enforcement::record_consequence_text;
        for (key, name, home) in [
            ("knik_arm_cartage", "Knik Arm Cartage", "anchorage_ak_us"),
            (
                "des_plaines_cartage",
                "Des Plaines River Cartage",
                "chicago_il_us",
            ),
        ] {
            let mut p = driver(home, key, name);
            p.driving_record.sufferance_carrier_key = key.to_string();
            assert!(kept_on_sufferance(&p), "{name}");
            assert_eq!(record_consequence_text(&p), "", "{name}");
        }
    }

    #[test]
    fn test_a_reset_with_no_carrier_takes_a_carrier_that_hires_there() {
        use ff_core::models::start_options::{
            hiring_carrier_for_option, reset_start_for_no_carrier,
        };
        let w = world();
        let (option, city) =
            reset_start_for_no_carrier(w, &["fairbanks_ak_us"]).expect("an Alaska start");
        assert_eq!(city, "fairbanks_ak_us");
        let carrier = hiring_carrier_for_option(option).expect("a hiring carrier");
        assert!(carrier.hires_in(w, "fairbanks_ak_us"));
        assert!(option.is_company_driver());

        let (option, _) = reset_start_for_no_carrier(w, &["chicago_il_us"]).expect("a start");
        assert!(hiring_carrier_for_option(option)
            .expect("a hiring carrier")
            .hires_in(w, "chicago_il_us"));

        // A blank home skips to the next city.
        let (_, city) = reset_start_for_no_carrier(w, &["", "anchorage_ak_us"]).expect("a start");
        assert_eq!(city, "anchorage_ak_us");
    }
}
