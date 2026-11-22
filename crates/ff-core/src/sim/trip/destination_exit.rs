//! Which interchange is the destination exit, off the trip and the world
//! alone. It lives here rather than in the driving state because the trip
//! needs the answer before the drive starts: a work zone keeps its closure
//! off the lane the destination exit is taken from (`Trip::exit_windows`).

use crate::data::world::World;
use crate::sim::trip_models::DESTINATION_EXIT_SCAN_WINDOW_MI;

use super::Trip;

/// The winning destination exit on a trip, off the trip alone.
///
/// Free rather than a method because the playtest road finder has to point at
/// exactly the interchange the drive will announce, and it runs against the
/// world data with no `App` and so no `DrivingState` to ask. Two copies of
/// this ranking would be two answers, and a finder that lands somewhere the
/// game does not call the destination exit is worse than no finder.
///
/// `trip.route` is the drive's own route (`DrivingState` clones it into the
/// trip at construction), so reading it here is the same scan.
pub fn scan_destination_exit(
    world: &World,
    trip: &Trip,
    include_past: bool,
) -> Option<(f64, String, String)> {
    let route = &trip.route;
    if route.legs.is_empty() {
        return None;
    }
    // Matched against real interchange sign text, so compare the spoken
    // city name ("Nashville"), never the slug key.
    let destination = world
        .spoken_city(
            route.cities.last().map(String::as_str).unwrap_or(""),
            Some(false),
        )
        .to_lowercase();
    let mainline_cities: Vec<String> = route
        .cities
        .iter()
        .map(|c| world.spoken_city(c, Some(false)))
        .collect();
    let scan_floor = trip.total_miles() - DESTINATION_EXIT_SCAN_WINDOW_MI;
    // (legs from the end, distance from the leg's destination end, whether
    // the sign does NOT name the destination, route mile, label, phrase)
    let mut candidates: Vec<(usize, f64, bool, f64, String, String)> = Vec::new();
    for i in (0..route.legs.len()).rev() {
        let leg = &route.legs[i];
        if trip.leg_starts[i] + leg.miles < scan_floor {
            // This leg ends before the final approach; every earlier leg
            // is farther out still.
            break;
        }
        let forward = route.cities.get(i).map(String::as_str) == Some(leg.a.as_str());
        let target = if forward { leg.miles } else { 0.0 };
        let along = |at_mi: f64| if forward { at_mi } else { leg.miles - at_mi };
        // On a leg that crosses into another country, an exit before the
        // last line is still in the country being left: Detroit's exit 47B
        // sits nearer the Windsor end than any numbered exit in Windsor, and
        // taking it would end the trip short of the bridge and the customs
        // booth. Only exits past the line can be the destination exit.
        let country = |key: &str| world.cities.get(key).map(|c| c.country.as_str());
        let border_floor = if country(&leg.a) != country(&leg.b) {
            leg.state_crossings()
                .iter()
                .map(|crossing| along(crossing.at_mi))
                .fold(0.0, f64::max)
        } else {
            0.0
        };
        for ix in leg.interchanges() {
            let exit_label = ix.exit_label();
            if exit_label.is_empty() {
                continue;
            }
            let offset = along(ix.at_mi);
            if offset < border_floor {
                continue;
            }
            let route_mile = trip.leg_starts[i] + offset;
            if route_mile < scan_floor {
                continue;
            }
            if !include_past && route_mile <= trip.position_mi + 0.05 {
                continue;
            }
            let dist_from_destination = (ix.at_mi - target).abs();
            // An exit nearer the city the leg leaves than the one it reaches
            // is the leaving city's exit: on a short leg the scan window
            // reaches back to it. Flagstaff to Sedona (29 miles) crowned
            // I-17 exit 337, 4.7 miles out of Flagstaff, and the drive
            // would have settled 24 miles short (2026-11-22).
            if dist_from_destination > leg.miles - dist_from_destination {
                continue;
            }
            let matches_destination = ix
                .destinations
                .iter()
                .any(|part| part.to_lowercase().contains(&destination));
            let siblings: Vec<String> = leg
                .interchanges()
                .iter()
                .filter(|other| other.at_mi != ix.at_mi)
                .flat_map(|other| other.destinations.iter().cloned())
                .collect();
            candidates.push((
                route.legs.len() - 1 - i,
                dist_from_destination,
                !matches_destination,
                route_mile,
                exit_label,
                ix.spoken_phrase_on(&leg.highway, &mainline_cities, &siblings),
            ));
        }
    }
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then(a.1.total_cmp(&b.1))
            .then(a.2.cmp(&b.2))
            .then(a.3.total_cmp(&b.3))
            .then(a.4.cmp(&b.4))
            .then(a.5.cmp(&b.5))
    });
    let winner = &candidates[0];
    Some((winner.3, winner.4.clone(), winner.5.clone()))
}
