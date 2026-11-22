//! A village the road names is on the road's side of the Canada–United
//! States border, and a city's own neighbourhoods are not named as towns
//! before it.
//!
//! The village layer once named places across the water from the road:
//! Youngstown, New York from the QEW, Waddington, New York from Highway 401,
//! Wyandotte, Michigan from Highway 3 out of Windsor. The map tool now reads
//! the province boundaries and drops a place on the other side from the
//! road; this keeps the ones it dropped from coming back with a re-bake.

use crate::data_support::world;

/// `(village, one end, other end, the country it is in)`, for the places
/// dropped on 2026-11-22 from legs that run in the other country there.
const ACROSS_THE_BORDER: &[(&str, &str, &str, &str)] = &[
    ("Alexandria Bay", "brockville_on_ca", "kingston_on_ca", "US"),
    ("Allen Park", "chatham_on_ca", "windsor_on_ca", "US"),
    (
        "Bay Mills",
        "sault_ste_marie_mi_us",
        "sault_ste_marie_on_ca",
        "US",
    ),
    ("Bay Mills", "sault_ste_marie_on_ca", "wawa_on_ca", "US"),
    ("Brimley", "sault_ste_marie_on_ca", "wawa_on_ca", "US"),
    ("Cape Vincent", "belleville_on_ca", "kingston_on_ca", "US"),
    ("Cape Vincent", "brockville_on_ca", "kingston_on_ca", "US"),
    ("Cape Vincent", "kingston_on_ca", "watertown_ny_us", "US"),
    ("Clayton", "brockville_on_ca", "kingston_on_ca", "US"),
    ("Columbus", "estevan_sk_ca", "minot_nd_us", "US"),
    ("Corunna", "detroit_mi_us", "port_huron_mi_us", "CA"),
    ("Corunna", "flint_mi_us", "port_huron_mi_us", "CA"),
    ("Corunna", "port_huron_mi_us", "sarnia_on_ca", "CA"),
    ("Courtright", "detroit_mi_us", "port_huron_mi_us", "CA"),
    ("Courtright", "flint_mi_us", "port_huron_mi_us", "CA"),
    ("Courtright", "port_huron_mi_us", "sarnia_on_ca", "CA"),
    (
        "Crystal Beach",
        "buffalo_ny_us",
        "niagara_falls_on_ca",
        "CA",
    ),
    ("Dafter", "sault_ste_marie_on_ca", "wawa_on_ca", "US"),
    ("Dearborn", "chatham_on_ca", "windsor_on_ca", "US"),
    ("Ecorse", "chatham_on_ca", "windsor_on_ca", "US"),
    ("Ecorse", "detroit_mi_us", "windsor_on_ca", "US"),
    ("Everson", "abbotsford_bc_ca", "chilliwack_bc_ca", "US"),
    ("Everson", "abbotsford_bc_ca", "surrey_bc_ca", "US"),
    ("Everson", "abbotsford_bc_ca", "vancouver_bc_ca", "US"),
    (
        "Grand Island",
        "niagara_falls_on_ca",
        "st_catharines_on_ca",
        "US",
    ),
    (
        "Grandyle Village",
        "niagara_falls_on_ca",
        "st_catharines_on_ca",
        "US",
    ),
    ("Grosse Pointe", "chatham_on_ca", "windsor_on_ca", "US"),
    (
        "Grosse Pointe Farms",
        "chatham_on_ca",
        "windsor_on_ca",
        "US",
    ),
    ("Grosse Pointe Park", "chatham_on_ca", "windsor_on_ca", "US"),
    ("Hamtramck", "chatham_on_ca", "windsor_on_ca", "US"),
    ("Harper Woods", "chatham_on_ca", "windsor_on_ca", "US"),
    ("Heuvelton", "brockville_on_ca", "ottawa_on_ca", "US"),
    ("Highland Park", "chatham_on_ca", "windsor_on_ca", "US"),
    ("Hogansburg", "brockville_on_ca", "cornwall_on_ca", "US"),
    (
        "Island View",
        "fort_frances_on_ca",
        "thunder_bay_on_ca",
        "US",
    ),
    ("Lewiston", "hamilton_on_ca", "st_catharines_on_ca", "US"),
    (
        "Lewiston",
        "niagara_falls_on_ca",
        "st_catharines_on_ca",
        "US",
    ),
    ("Lincoln Park", "chatham_on_ca", "windsor_on_ca", "US"),
    ("Lisbon", "brockville_on_ca", "cornwall_on_ca", "US"),
    ("Lisbon", "brockville_on_ca", "ottawa_on_ca", "US"),
    ("Louisville", "brockville_on_ca", "cornwall_on_ca", "US"),
    ("Lynden", "abbotsford_bc_ca", "chilliwack_bc_ca", "US"),
    ("Lynden", "abbotsford_bc_ca", "surrey_bc_ca", "US"),
    ("Lynden", "abbotsford_bc_ca", "vancouver_bc_ca", "US"),
    ("Maple Falls", "abbotsford_bc_ca", "chilliwack_bc_ca", "US"),
    ("Massena", "brockville_on_ca", "cornwall_on_ca", "US"),
    ("Melvindale", "chatham_on_ca", "windsor_on_ca", "US"),
    ("Mooretown", "detroit_mi_us", "port_huron_mi_us", "CA"),
    ("Mooretown", "flint_mi_us", "port_huron_mi_us", "CA"),
    ("Mooretown", "port_huron_mi_us", "sarnia_on_ca", "CA"),
    ("Nooksack", "abbotsford_bc_ca", "chilliwack_bc_ca", "US"),
    ("Nooksack", "abbotsford_bc_ca", "surrey_bc_ca", "US"),
    ("Nooksack", "abbotsford_bc_ca", "vancouver_bc_ca", "US"),
    (
        "North Tonawanda",
        "niagara_falls_on_ca",
        "st_catharines_on_ca",
        "US",
    ),
    ("Northgate", "estevan_sk_ca", "minot_nd_us", "CA"),
    ("Ogdensburg", "brockville_on_ca", "cornwall_on_ca", "US"),
    ("Ogdensburg", "brockville_on_ca", "ottawa_on_ca", "US"),
    ("Oroville", "castlegar_bc_ca", "osoyoos_bc_ca", "US"),
    ("Oroville", "osoyoos_bc_ca", "penticton_bc_ca", "US"),
    ("Oroville", "osoyoos_bc_ca", "princeton_bc_ca", "US"),
    ("Ridgeway", "buffalo_ny_us", "niagara_falls_on_ca", "CA"),
    ("River Rouge", "chatham_on_ca", "windsor_on_ca", "US"),
    ("Riverview", "detroit_mi_us", "windsor_on_ca", "US"),
    ("Saint Clair", "london_on_ca", "sarnia_on_ca", "US"),
    (
        "Sanborn",
        "niagara_falls_on_ca",
        "st_catharines_on_ca",
        "US",
    ),
    ("Southgate", "detroit_mi_us", "windsor_on_ca", "US"),
    ("St. Regis", "brockville_on_ca", "cornwall_on_ca", "US"),
    ("St. Regis", "cornwall_on_ca", "montreal_qc_ca", "US"),
    ("Stevensville", "buffalo_ny_us", "niagara_falls_on_ca", "CA"),
    (
        "Tonawanda",
        "niagara_falls_on_ca",
        "st_catharines_on_ca",
        "US",
    ),
    ("Trenton", "detroit_mi_us", "windsor_on_ca", "US"),
    ("Waddington", "brockville_on_ca", "cornwall_on_ca", "US"),
    ("Wyandotte", "chatham_on_ca", "windsor_on_ca", "US"),
    ("Wyandotte", "detroit_mi_us", "windsor_on_ca", "US"),
    ("Youngstown", "hamilton_on_ca", "st_catharines_on_ca", "US"),
    (
        "Youngstown",
        "niagara_falls_on_ca",
        "st_catharines_on_ca",
        "US",
    ),
];

/// A city's own neighbourhoods that OpenStreetMap maps as villages, and the
/// city whose legs must not name them (the map tool's list).
const NEIGHBOURHOODS: &[(&str, &str)] = &[
    ("Steele Creek", "fairbanks_ak_us"),
    ("Farmers Loop", "fairbanks_ak_us"),
];

#[test]
fn no_village_is_named_from_the_other_side_of_the_border() {
    let mut found = Vec::new();
    for leg in &world().legs {
        for l in leg.landmarks() {
            if l.category != "village" {
                continue;
            }
            let hit = ACROSS_THE_BORDER.iter().find(|(name, a, b, _)| {
                *name == l.name && ((leg.a == *a && leg.b == *b) || (leg.a == *b && leg.b == *a))
            });
            if let Some((_, _, _, country)) = hit {
                found.push(format!("{}:{} names {} ({country})", leg.a, leg.b, l.name));
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

#[test]
fn a_city_neighbourhood_is_not_named_on_a_leg_to_its_city() {
    let mut found = Vec::new();
    for leg in &world().legs {
        for l in leg.landmarks() {
            let hit = NEIGHBOURHOODS
                .iter()
                .any(|(name, city)| *name == l.name && (leg.a == *city || leg.b == *city));
            if l.category == "village" && hit {
                found.push(format!("{}:{} names {}", leg.a, leg.b, l.name));
            }
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}
