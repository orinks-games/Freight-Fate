//! Region taxonomy: derivation, coverage, and stored-equals-derived guard
//! (the world-backed and cross-module half of `tests/test_regions.py`; the
//! classifier cases live inline in `regions.rs`).

use crate::data_support::world;
use ff_core::data::regions::{classify_region, REGIONS};

#[test]
fn test_stored_region_matches_derived() {
    // Every city's baked region must equal classify_region for its coords.
    //
    // This is the guard that keeps a misclassification (such as Reno being tagged
    // the Rockies) from ever recurring as the map grows.
    let world = world();
    let mut mismatches = Vec::new();
    for (name, city) in &world.cities {
        let derived = classify_region(&city.state_code, city.lat, city.lon).unwrap();
        if city.region != derived {
            mismatches.push(format!(
                "{name}: stored {:?} != derived {derived:?}",
                city.region
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "Stored regions out of sync with classifier:\n{}",
        mismatches.join("\n")
    );
}

#[test]
fn test_every_stored_region_is_canonical() {
    for (name, city) in &world().cities {
        assert!(
            REGIONS.contains(&city.region.as_str()),
            "{name} has non-canonical region {:?}",
            city.region
        );
    }
}

#[test]
fn test_reno_is_great_basin_not_rockies() {
    // The bug this work fixed: Reno is Great Basin / eastern Sierra, not Rockies.
    let world = world();
    assert_eq!(world.cities["reno_nv_us"].region, "great_basin");
    assert_eq!(world.cities["boise_id_us"].region, "great_basin");
}

/// The REGION_MARKET_TAGS and REGION_LABELS halves are checked inline in
/// `regions.rs`; these two tables belong to other modules.
#[test]
fn test_every_region_covered_in_flavor_tables() {
    use ff_core::models::economy::REGION_FUEL_PRICE;
    use ff_core::sim::weather::REGION_WEIGHTS;

    let missing: Vec<&&str> = REGIONS
        .iter()
        .filter(|r| !REGION_WEIGHTS.iter().any(|(key, _)| key == *r))
        .collect();
    assert!(
        missing.is_empty(),
        "REGION_WEIGHTS is missing regions: {missing:?}"
    );
    let missing: Vec<&&str> = REGIONS
        .iter()
        .filter(|r| !REGION_FUEL_PRICE.iter().any(|(key, _)| key == *r))
        .collect();
    assert!(
        missing.is_empty(),
        "REGION_FUEL_PRICE is missing regions: {missing:?}"
    );
}

#[test]
fn test_every_region_has_local_hazard_flavor() {
    // Every canonical region is named by at least one region-specific hazard,
    // so no region falls back to only the nationwide staples.
    use ff_core::sim::trip_models::HAZARDS;

    let tagged: std::collections::HashSet<&str> = HAZARDS
        .iter()
        .filter_map(|hazard| hazard.regions)
        .flat_map(|regions| regions.iter().copied())
        .collect();
    let missing: Vec<&str> = REGIONS
        .iter()
        .copied()
        .filter(|region| !tagged.contains(region))
        .collect();
    assert!(
        missing.is_empty(),
        "regions with no local hazard flavor: {missing:?}"
    );
}

#[test]
fn test_hazard_region_tags_are_canonical() {
    use ff_core::sim::trip_models::HAZARDS;

    for hazard in HAZARDS {
        for region in hazard.regions.unwrap_or(&[]) {
            assert!(
                REGIONS.contains(region),
                "{:?} tags unknown region {region:?}",
                hazard.text
            );
        }
    }
}

#[test]
fn test_every_city_has_a_climate_row() {
    // The simulated temperature reads `City::climate`: every city must land
    // on a real row, never the default, and the lower 48 (Alaska aside)
    // must read its freight region unchanged.
    use ff_core::sim::season::{region_climate, DEFAULT_CLIMATE};
    let world = world();
    let mut bad = Vec::new();
    for (name, city) in &world.cities {
        let climate = city.climate();
        if climate != "heartland" && region_climate(climate) == DEFAULT_CLIMATE {
            bad.push(format!("{name}: {climate} has no row"));
        }
        if city.country == "US" && city.state_code != "AK" && climate != city.region {
            bad.push(format!(
                "{name}: climate {climate} != region {}",
                city.region
            ));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}
