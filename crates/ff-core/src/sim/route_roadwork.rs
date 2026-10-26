//! Live construction along a candidate route, read the way a dispatcher
//! reads 511 before choosing a lane: which of the sensible routes carries
//! reported construction, how many minutes it costs, and whether the road is
//! closed outright.
//!
//! Dispatch has no map of roadwork of its own. The zones a drive meets are
//! drawn when the trip starts, and the only construction known before the
//! wheels turn is what the state 511 feeds report. So this module reads the
//! same provider cache the trip will read, on the two or three route options
//! `World::supported_route_options` already ranks by distance, and re-ranks
//! them by drive time plus the construction delay. A route through a
//! reported full closure sorts last whatever its time: a dispatcher does not
//! send a truck at a closed road while another way exists.
//!
//! The active National Weather Service warnings ride the same ranking
//! (`real_weather_alerts`): a blizzard, ice storm, hurricane or tornado
//! warning on a route sorts it last like a closure, and a winter storm,
//! high wind, flash flood or dense fog warning prices the alerted stretch at
//! that condition's safe speed.
//!
//! Every number here is derived from the feed and the deadline model, and
//! says so: the delay through a zone is its length at the zone's limit minus
//! the same length at the route's own planned pace. No queue penalty is
//! guessed for a lane closure, because the feeds carry no demand to derive
//! one from; a closure costs only its slower stretch until they do.

use std::collections::HashSet;

use crate::data::world::World;
use crate::data::world_models::Route;
use crate::models::jobs::route_drive_hours;
use crate::pyfmt::fmt_f;
use crate::settings::Settings;
use crate::sim::real_traffic::caltrans::construction_feed_keys;
use crate::sim::real_weather_alerts::{
    scan_route_weather, weather_brief, RouteWeather, WeatherAlertsProvider,
};
use crate::sim::trip::Trip;
use crate::sim::trip_models::ZONE_MIN_GAP_MI;
use crate::sim::trip_route_helpers::{leg_states, leg_track, runs_against_travel, snap_to_leg};
use crate::sim::trip_traffic::TrafficProvider;

/// How far from the road a 511 event still counts as being on it, the same
/// radius the trip uses when it places its zones.
const NEAR_ROUTE_MI: f64 = 3.0;

/// A second route that is quicker by less than this is a coin toss, and the
/// shorter one keeps its place.
const TIE_MINUTES: f64 = 1.0;

/// One reported construction event snapped onto a route.
#[derive(Debug, Clone, PartialEq)]
pub struct ConstructionSpot {
    /// The road as the world names it (`I-70`).
    pub road: String,
    /// The route city nearest the spot.
    pub near_city: String,
    /// The feed's closure kind: "full closure", "single lane", "alternating",
    /// "shoulder", or empty when it did not say.
    pub closure: String,
    pub length_mi: f64,
    pub limit_mph: f64,
    pub route_mile: f64,
    /// Hours lost against the route's planned pace; derived, never guessed.
    pub delay_h: f64,
}

impl ConstructionSpot {
    /// The closure in a driver's words.
    pub fn closure_words(&self) -> &'static str {
        match self.closure.as_str() {
            "full closure" => "the road closed",
            "single lane" => "one lane closed",
            "alternating" => "alternating one-way traffic",
            "shoulder" => "shoulder work",
            _ => "a construction zone",
        }
    }
}

/// Everything 511 reports along one route option.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RouteConstruction {
    pub spots: Vec<ConstructionSpot>,
    /// The sum of the spots' delays, in hours.
    pub delay_h: f64,
    /// A reported full closure somewhere on the route.
    pub blocked: bool,
    /// The deadline model's drive hours for the route with no construction.
    pub drive_h: f64,
}

impl RouteConstruction {
    pub fn total_h(&self) -> f64 {
        self.drive_h + self.delay_h
    }
}

/// Dispatch's ranking of the route options, quickest first.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DispatchRouting {
    /// Indices into the routes given, dispatch's pick first.
    pub order: Vec<usize>,
    /// One construction report per route given, in the routes' original order.
    pub reports: Vec<RouteConstruction>,
    /// One weather report per route given, in the routes' original order.
    pub weather: Vec<RouteWeather>,
}

impl DispatchRouting {
    /// The route dispatch picked, or 0 when nothing was given.
    pub fn pick(&self) -> usize {
        self.order.first().copied().unwrap_or(0)
    }

    /// True when live construction moved dispatch off the shortest route.
    pub fn detoured(&self) -> bool {
        self.pick() != 0
    }

    pub fn report(&self, index: usize) -> Option<&RouteConstruction> {
        self.reports.get(index)
    }

    pub fn weather_report(&self, index: usize) -> Option<&RouteWeather> {
        self.weather.get(index)
    }

    /// Drive hours plus every delay dispatch counts, for one route.
    pub fn total_h(&self, index: usize) -> f64 {
        self.report(index).map(|r| r.total_h()).unwrap_or(0.0)
            + self.weather_report(index).map(|w| w.delay_h).unwrap_or(0.0)
    }

    /// A closed road or a warning dispatch will not drive into, on one route.
    pub fn is_blocked(&self, index: usize) -> bool {
        self.report(index).is_some_and(|r| r.blocked)
            || self.weather_report(index).is_some_and(|w| w.avoid)
    }

    /// True when something on the routes (construction or a warning) was
    /// found at all, so a re-ranking could have happened.
    pub fn found_anything(&self) -> bool {
        self.reports.iter().any(|r| !r.spots.is_empty())
            || self.weather.iter().any(|w| !w.alerts.is_empty())
    }
}

/// The 511 feed keys a route's construction is read from, in road order,
/// for warming the provider's cache before dispatch needs an answer: each
/// state it runs in, and in California the districts its legs pass through.
pub fn route_state_keys(route: &Route) -> Vec<String> {
    let mut seen: HashSet<String> = HashSet::new();
    let mut keys = Vec::new();
    for (i, leg) in route.legs.iter().enumerate() {
        let forward = route.cities[i] == leg.a;
        let track = leg_track(leg);
        for state in leg_states(leg, forward) {
            for key in construction_feed_keys(&state, &track, NEAR_ROUTE_MI) {
                if seen.insert(key.clone()) {
                    keys.push(key);
                }
            }
        }
    }
    keys
}

/// Read the provider's construction events onto a route.
pub fn scan_route_construction(
    route: &Route,
    provider: &dyn TrafficProvider,
    world: &World,
) -> RouteConstruction {
    let drive_h = route_drive_hours(Some(route), 0.0, Some(world));
    let mut report = RouteConstruction {
        drive_h,
        ..RouteConstruction::default()
    };
    let miles = route.miles();
    if miles <= 0.0 || drive_h <= 0.0 {
        return report;
    }
    let pace_mph = miles / drive_h;
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut seen_spans: Vec<(f64, f64)> = Vec::new();
    let mut leg_start = 0.0;
    for (i, leg) in route.legs.iter().enumerate() {
        let forward = route.cities[i] == leg.a;
        let track = leg_track(leg);
        if track.is_empty() {
            leg_start += leg.miles;
            continue;
        }
        // Every state the leg runs in: a leg into California reads Caltrans
        // for its California miles.
        let events: Vec<_> = leg_states(leg, forward)
            .iter()
            .flat_map(|state| {
                provider.get_construction_near_route(
                    state,
                    &track,
                    Some(&leg.highway),
                    NEAR_ROUTE_MI,
                )
            })
            .collect();
        for event in events {
            if seen_ids.contains(&event.id) {
                continue;
            }
            let (Some(lat), Some(lon)) = (event.latitude, event.longitude) else {
                continue;
            };
            // Off this leg (the next one may still hold it), or on the far
            // carriageway from the truck.
            let Some(route_mile) = snap_to_leg(lat, lon, leg, forward, leg_start)
                .filter(|snap| !runs_against_travel(&event.direction, snap.bearing_deg))
                .map(|snap| snap.mile)
            else {
                continue;
            };
            seen_ids.insert(event.id.clone());
            let length_mi = Trip::construction_zone_length(&event);
            let start_mi = (route_mile - length_mi / 2.0).max(0.0);
            let end_mi = miles.min(start_mi + length_mi);
            if seen_spans
                .iter()
                .any(|(s, e)| start_mi < e + ZONE_MIN_GAP_MI && end_mi > s - ZONE_MIN_GAP_MI)
            {
                continue;
            }
            seen_spans.push((start_mi, end_mi));
            let limit_mph = Trip::construction_zone_speed(&event);
            // Derived: the zone's length at its limit, less the same length at
            // the pace the deadline model already plans this route at.
            let delay_h = (length_mi / limit_mph - length_mi / pace_mph).max(0.0);
            let offset = route_mile - leg_start;
            let near_key = if (offset < leg.miles / 2.0) == forward {
                &leg.a
            } else {
                &leg.b
            };
            let near_city = world
                .cities
                .get(near_key)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| near_key.clone());
            let blocked = event.closure == "full closure";
            report.blocked |= blocked;
            report.delay_h += delay_h;
            report.spots.push(ConstructionSpot {
                road: leg.highway.clone(),
                near_city,
                closure: event.closure.clone(),
                length_mi,
                limit_mph,
                route_mile,
                delay_h,
            });
        }
        leg_start += leg.miles;
    }
    report
        .spots
        .sort_by(|a, b| a.route_mile.total_cmp(&b.route_mile));
    report
}

/// Rank the route options the way dispatch does: quickest first counting
/// reported construction and the active weather warnings, a closed road or a
/// warning nobody drives into last, and the shorter route keeping its place
/// when the difference is under a minute. With no providers the order is
/// the one given.
pub fn choose_dispatch_route(
    routes: &[Route],
    provider: Option<&dyn TrafficProvider>,
    alerts: Option<&WeatherAlertsProvider>,
    world: &World,
) -> DispatchRouting {
    let reports: Vec<RouteConstruction> = routes
        .iter()
        .map(|route| match provider {
            Some(provider) => scan_route_construction(route, provider, world),
            None => RouteConstruction {
                drive_h: route_drive_hours(Some(route), 0.0, Some(world)),
                ..RouteConstruction::default()
            },
        })
        .collect();
    let weather: Vec<RouteWeather> = routes
        .iter()
        .zip(&reports)
        .map(|(route, report)| match alerts {
            Some(alerts) => {
                let pace = if report.drive_h > 0.0 {
                    route.miles() / report.drive_h
                } else {
                    0.0
                };
                scan_route_weather(route, alerts, world, pace)
            }
            None => RouteWeather::default(),
        })
        .collect();
    let mut routing = DispatchRouting {
        order: (0..routes.len()).collect(),
        reports,
        weather,
    };
    if routing.found_anything() {
        let mut order = routing.order.clone();
        order.sort_by(|&a, &b| {
            // An open road before a closed one, whatever the clock says.
            routing
                .is_blocked(a)
                .cmp(&routing.is_blocked(b))
                .then_with(|| {
                    let (ta, tb) = (routing.total_h(a), routing.total_h(b));
                    let gap_min = (ta - tb) * 60.0;
                    if gap_min.abs() < TIE_MINUTES {
                        a.cmp(&b)
                    } else {
                        ta.total_cmp(&tb)
                    }
                })
        });
        routing.order = order;
    }
    routing
}

fn minutes_words(hours: f64) -> String {
    let minutes = (hours * 60.0).round();
    if minutes < 1.0 {
        "under a minute".to_string()
    } else if minutes == 1.0 {
        "about a minute".to_string()
    } else {
        format!("about {} minutes", fmt_f(minutes, 0))
    }
}

/// One spot in dispatch's words: "one lane closed on I-70 near Topeka, 5
/// miles, 45 miles per hour, about 3 minutes".
pub fn spot_text(spot: &ConstructionSpot, settings: &Settings) -> String {
    format!(
        "{} on {} near {}, {}, {}, {}",
        spot.closure_words(),
        spot.road,
        spot.near_city,
        settings.distance_text(spot.length_mi, false),
        settings.speed_text(spot.limit_mph),
        minutes_words(spot.delay_h)
    )
}

/// The spots in dispatch's words, joined with semicolons. Spots that would
/// read the same are said once with a count: one 511 feed listed seven
/// separate stretches near Des Moines, and dispatch read the same closure
/// seven times in one line (agent drive, 2026-10-17).
pub fn spots_text(spots: &[ConstructionSpot], settings: &Settings) -> String {
    let mut groups: Vec<(String, usize, &ConstructionSpot)> = Vec::new();
    for spot in spots {
        let text = spot_text(spot, settings);
        match groups.iter_mut().find(|(t, _, _)| *t == text) {
            Some(group) => group.1 += 1,
            None => groups.push((text, 1, spot)),
        }
    }
    groups
        .into_iter()
        .map(|(text, count, spot)| {
            if count == 1 {
                return text;
            }
            format!(
                "{} on {} near {} in {count} places, {} each, {}, {} each",
                spot.closure_words(),
                spot.road,
                spot.near_city,
                settings.distance_text(spot.length_mi, false),
                settings.speed_text(spot.limit_mph),
                minutes_words(spot.delay_h)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

/// The city a detour route goes by way of, spoken, or empty.
fn by_way_of(route: &Route, world: &World) -> String {
    route
        .cities
        .get(1)
        .filter(|_| route.cities.len() > 2)
        .map(|key| {
            world
                .cities
                .get(key)
                .map(|c| c.name.clone())
                .unwrap_or_else(|| key.clone())
        })
        .map(|name| format!(" by way of {name}"))
        .unwrap_or_default()
}

/// Why the shortest route lost: the closed road or the warning first, else
/// the construction it costs, else the weather it costs.
fn avoided_reason(routing: &DispatchRouting) -> Option<(String, bool)> {
    let construction = routing.report(0)?;
    let weather = routing.weather_report(0)?;
    if let Some(spot) = construction.spots.first().filter(|_| construction.blocked) {
        return Some((
            format!(
                "The road is closed on {} near {}.",
                spot.road, spot.near_city
            ),
            true,
        ));
    }
    if let Some(alert) = weather.avoided() {
        return Some((
            format!(
                "A {} covers the road near {}.",
                alert.alert.event, alert.near_city
            ),
            true,
        ));
    }
    if let Some(spot) = construction.spots.first() {
        return Some((
            format!(
                "{} on {} near {}",
                spot.closure_words(),
                spot.road,
                spot.near_city
            ),
            false,
        ));
    }
    weather
        .alerts
        .iter()
        .find(|a| a.delay_h > 0.0)
        .map(|alert| {
            (
                format!("the {} near {}", alert.alert.event, alert.near_city),
                false,
            )
        })
}

/// What dispatch says about the route it picked, before naming the
/// destination. Empty when nothing was reported on any option.
pub fn dispatch_route_line(
    routes: &[Route],
    routing: &DispatchRouting,
    world: &World,
    settings: &Settings,
) -> String {
    let pick = routing.pick();
    let (Some(picked), Some(report), Some(weather)) = (
        routes.get(pick),
        routing.report(pick),
        routing.weather_report(pick),
    ) else {
        return String::new();
    };
    let mut parts: Vec<String> = Vec::new();
    if routing.detoured() {
        if let (Some(shortest), Some((reason, hard))) = (routes.first(), avoided_reason(routing)) {
            let extra_mi = picked.miles() - shortest.miles();
            let adds = if extra_mi > 0.5 {
                format!("adds {} and ", settings.distance_text(extra_mi, false))
            } else {
                String::new()
            };
            if hard {
                // A closed road or a warning is not a time saving; the detour
                // costs what it costs against the road as it would have run.
                let open_h = routing.report(0).map(|r| r.drive_h).unwrap_or(0.0);
                let extra_h = routing.total_h(pick) - open_h;
                let via = by_way_of(picked, world);
                let cost = if extra_h * 60.0 >= 0.5 {
                    format!("This way {adds}takes {}.", minutes_words(extra_h))
                } else if extra_mi > 0.5 {
                    format!("This way adds {}.", settings.distance_text(extra_mi, false))
                } else {
                    "This way costs no time.".to_string()
                };
                parts.push(format!(
                    "{reason} Dispatch is routing you around it{via}. {cost}"
                ));
            } else {
                let saved_h = routing.total_h(0) - routing.total_h(pick);
                parts.push(format!(
                    "Dispatch is routing you around {reason}. This way {adds}saves {}.",
                    minutes_words(saved_h)
                ));
            }
        }
    }
    let has_cost = !report.spots.is_empty()
        || weather.alerts.iter().any(|a| {
            a.delay_h > 0.0 || a.effect == crate::sim::real_weather_alerts::AlertEffect::Avoid
        });
    if !report.spots.is_empty() {
        parts.push(format!(
            "Construction reported on the way: {}.",
            spots_text(&report.spots, settings)
        ));
    }
    let brief = weather_brief(weather);
    if !brief.is_empty() {
        parts.push(brief);
    }
    if !routing.detoured() && has_cost {
        parts.push("No quicker way around.".to_string());
    }
    parts.join(" ")
}

/// What the route planning screen says about dispatch's ranking, for a
/// driver who chooses their own route. Empty unless something moved the
/// recommendation off the shortest way; the per-option notes carry the rest.
pub fn route_planning_note(routes: &[Route], routing: &DispatchRouting, world: &World) -> String {
    if !routing.detoured() {
        return String::new();
    }
    let pick = routing.pick();
    let (Some(picked), Some((reason, hard))) = (routes.get(pick), avoided_reason(routing)) else {
        return String::new();
    };
    if hard {
        return format!(
            "{reason} Route 1 goes around it{}.",
            by_way_of(picked, world)
        );
    }
    let saved_h = routing.total_h(0) - routing.total_h(pick);
    format!(
        "Route 1 avoids {reason} and saves {}.",
        minutes_words(saved_h)
    )
}

/// The note appended to one route option on the route planning screen.
/// Empty when 511 reported nothing on it.
pub fn route_option_note(report: &RouteConstruction, settings: &Settings) -> String {
    if report.spots.is_empty() {
        return String::new();
    }
    format!("Construction: {}.", spots_text(&report.spots, settings))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::world::get_world;
    use crate::sim::real_traffic::{wall_time, RealTrafficProvider, TrafficData};
    use crate::sim::real_traffic_parsers::TrafficEvent;

    /// The fixture pair: I-71 straight down from Columbus, or I-70 and I-75
    /// by way of Dayton, 18 miles and 25 minutes longer. Ohio has no live
    /// 511 API, so the provider serves exactly what the test seeds.
    fn ohio_routes() -> Vec<Route> {
        get_world()
            .supported_route_options("Columbus", "Cincinnati", 3)
            .expect("Columbus to Cincinnati routes")
    }

    fn construction(id: &str, closure: &str, lat: f64, lon: f64, location: &str) -> TrafficEvent {
        let mut event = TrafficEvent::new(id, "construction", "medium", "Roadwork", "Test");
        event.latitude = Some(lat);
        event.longitude = Some(lon);
        event.closure = closure.to_string();
        event.location_text = location.to_string();
        event.work_type = "construction".to_string();
        event
    }

    fn seeded(state: &str, events: Vec<TrafficEvent>) -> RealTrafficProvider {
        let provider = RealTrafficProvider::offline();
        let now = wall_time();
        provider.seed_cache(
            &format!("{state}:construction"),
            TrafficData::new(state, events, now, now, "test"),
        );
        provider
    }

    /// A point the I-71 leg's own geometry passes through, between the two
    /// cities, so the event snaps onto the shortest route and no other.
    const ON_I71: (f64, f64) = (39.53665, -83.78303);

    #[test]
    fn test_no_provider_keeps_the_distance_order_and_says_nothing() {
        let world = get_world();
        let routes = ohio_routes();
        let routing = choose_dispatch_route(&routes, None, None, world);
        assert_eq!(routing.order, vec![0, 1, 2]);
        assert!(!routing.detoured());
        assert!(routing
            .reports
            .iter()
            .all(|r| r.spots.is_empty() && r.drive_h > 0.0));
        assert_eq!(
            dispatch_route_line(&routes, &routing, world, &Settings::default()),
            ""
        );
    }

    #[test]
    fn test_a_reported_closure_sends_dispatch_around_by_the_next_route() {
        let world = get_world();
        let routes = ohio_routes();
        let provider = seeded(
            "ohio",
            vec![construction(
                "closed",
                "full closure",
                ON_I71.0,
                ON_I71.1,
                "",
            )],
        );
        let routing = choose_dispatch_route(&routes, Some(&provider), None, world);
        assert!(routing.reports[0].blocked);
        assert!(!routing.reports[1].blocked);
        assert_eq!(routing.pick(), 1, "{:?}", routing.order);
        let line = dispatch_route_line(&routes, &routing, world, &Settings::default());
        assert!(
            line.starts_with("The road is closed on I-71 near "),
            "{line}"
        );
        assert!(
            line.contains("Dispatch is routing you around it by way of Dayton."),
            "{line}"
        );
        assert!(
            line.contains("This way adds 18 miles and takes about "),
            "{line}"
        );
        assert!(!line.contains("Construction reported on the way"), "{line}");
        let note = route_planning_note(&routes, &routing, world);
        assert!(
            note.starts_with("The road is closed on I-71 near "),
            "{note}"
        );
        assert!(
            note.ends_with("Route 1 goes around it by way of Dayton."),
            "{note}"
        );
    }

    #[test]
    fn test_a_short_lane_closure_stays_on_the_shortest_route_and_is_announced() {
        let world = get_world();
        let routes = ohio_routes();
        let provider = seeded(
            "ohio",
            vec![construction("lane", "single lane", ON_I71.0, ON_I71.1, "")],
        );
        let routing = choose_dispatch_route(&routes, Some(&provider), None, world);
        assert_eq!(routing.pick(), 0, "{:?}", routing.order);
        let report = &routing.reports[0];
        assert_eq!(report.spots.len(), 1);
        assert!(!report.blocked);
        // 5 miles at 45 against the route's own pace: a couple of minutes,
        // never the 25 the Dayton way costs.
        let delay_min = report.delay_h * 60.0;
        assert!(delay_min > 1.0 && delay_min < 4.0, "{delay_min}");
        let line = dispatch_route_line(&routes, &routing, world, &Settings::default());
        assert!(
            line.starts_with("Construction reported on the way: one lane closed on I-71 near "),
            "{line}"
        );
        assert!(
            line.contains("5 miles, 45 miles per hour, about "),
            "{line}"
        );
        assert!(line.ends_with("No quicker way around."), "{line}");
        let note = route_option_note(report, &Settings::default());
        assert!(
            note.starts_with("Construction: one lane closed on I-71 near "),
            "{note}"
        );
        assert!(route_option_note(&routing.reports[1], &Settings::default()).is_empty());
    }

    #[test]
    fn test_a_lane_closure_costing_more_than_the_next_route_moves_dispatch_over() {
        // Chicago to Indianapolis: I-65 straight through, or I-90 then I-65
        // through Gary and Lafayette, two miles and two minutes longer. A
        // twenty-mile single-lane stretch costs more than that by more than
        // the one-minute margin a coin toss keeps the shorter route on.
        let world = get_world();
        let routes = world
            .supported_route_options("Chicago", "Indianapolis", 3)
            .expect("Chicago to Indianapolis routes");
        assert!(routes.len() >= 2);
        let gap_min = (route_drive_hours(Some(&routes[1]), 0.0, Some(world))
            - route_drive_hours(Some(&routes[0]), 0.0, Some(world)))
            * 60.0;
        assert!(gap_min > TIE_MINUTES && gap_min < 5.0, "{gap_min}");
        let provider = seeded(
            "illinois",
            vec![construction(
                "long-lane",
                "single lane",
                40.62512,
                -86.99675,
                "Between milepost 152 and 172",
            )],
        );
        let routing = choose_dispatch_route(&routes, Some(&provider), None, world);
        assert_eq!(routing.reports[0].spots.len(), 1);
        assert!((routing.reports[0].spots[0].length_mi - 20.0).abs() < 1e-9);
        assert_eq!(routing.pick(), 1, "{:?}", routing.order);
        let line = dispatch_route_line(&routes, &routing, world, &Settings::default());
        assert!(
            line.starts_with("Dispatch is routing you around one lane closed on I-65 near "),
            "{line}"
        );
        assert!(line.contains("saves "), "{line}");
        let note = route_planning_note(&routes, &routing, world);
        assert!(
            note.starts_with("Route 1 avoids one lane closed on I-65 near "),
            "{note}"
        );
        assert!(route_planning_note(
            &routes,
            &choose_dispatch_route(&routes, None, None, world),
            world
        )
        .is_empty());
    }

    #[test]
    fn test_a_blizzard_warning_on_the_shortest_route_sends_dispatch_around_it() {
        // No construction anywhere. A Blizzard Warning at Cincinnati, where
        // every option ends, so no route can avoid it and the order holds.
        use crate::sim::real_weather_alerts::{WeatherAlert, WeatherAlertsProvider};
        let world = get_world();
        let routes = ohio_routes();
        let alerts = WeatherAlertsProvider::offline();
        let cincinnati = world.cities.get(&routes[0].cities[1]).expect("Cincinnati");
        alerts.seed(
            cincinnati.lat,
            cincinnati.lon,
            vec![WeatherAlert {
                id: "blizzard".into(),
                event: "Blizzard Warning".into(),
                severity: "Extreme".into(),
                headline: "Blizzard Warning".into(),
                area: "Hamilton".into(),
                description: "Whiteout conditions.".into(),
            }],
        );
        let routing = choose_dispatch_route(&routes, None, Some(&alerts), world);
        // Every route ends at Cincinnati, so every route carries the warning
        // and none can avoid it: the order stays the shortest first.
        assert!(routing.weather.iter().all(|w| w.avoid));
        assert_eq!(routing.pick(), 0, "{:?}", routing.order);
        let line = dispatch_route_line(&routes, &routing, world, &Settings::default());
        assert!(
            line.starts_with("Weather alerts on the way: Blizzard Warning near Cincinnati."),
            "{line}"
        );
        assert!(line.ends_with("No quicker way around."), "{line}");

        // Now the warning sits on Columbus only for the direct run: seed a
        // Winter Storm Warning at Columbus (both routes start there) and a
        // Blizzard at nobody. The slow warning costs the same on both, so the
        // shorter route keeps its place.
        let alerts = WeatherAlertsProvider::offline();
        let columbus = world.cities.get(&routes[0].cities[0]).expect("Columbus");
        alerts.seed(
            columbus.lat,
            columbus.lon,
            vec![WeatherAlert {
                id: "snow".into(),
                event: "Winter Storm Warning".into(),
                severity: "Severe".into(),
                headline: "Winter Storm Warning".into(),
                area: "Franklin".into(),
                description: "Heavy snow.".into(),
            }],
        );
        let routing = choose_dispatch_route(&routes, None, Some(&alerts), world);
        assert_eq!(routing.pick(), 0, "{:?}", routing.order);
        assert!(routing.weather[0].delay_h > 0.0);
        let line = dispatch_route_line(&routes, &routing, world, &Settings::default());
        assert!(
            line.starts_with(
                "Weather alerts on the way: Winter Storm Warning near Columbus, about "
            ),
            "{line}"
        );
    }

    #[test]
    fn test_a_tornado_warning_on_the_middle_city_moves_dispatch_to_the_clear_route() {
        // The avoid rule needs a warning only the shortest route carries.
        // Dayton to Cleveland: the shortest way passes Mansfield, the next
        // one passes Akron instead.
        use crate::sim::real_weather_alerts::{WeatherAlert, WeatherAlertsProvider};
        let world = get_world();
        let routes = world
            .supported_route_options("Dayton", "Cleveland", 3)
            .expect("Dayton to Cleveland routes");
        // Route 0 goes Columbus, Mansfield, Cleveland; route 1 goes Columbus,
        // Akron, Cleveland. Mansfield is on the shortest route only.
        let mansfield = routes[0]
            .cities
            .iter()
            .find(|c| c.starts_with("mansfield"))
            .expect("Mansfield on the shortest route");
        assert!(!routes[1].cities.contains(mansfield));
        let city = world.cities.get(mansfield).expect("Mansfield");
        let alerts = WeatherAlertsProvider::offline();
        alerts.seed(
            city.lat,
            city.lon,
            vec![WeatherAlert {
                id: "tornado".into(),
                event: "Tornado Warning".into(),
                severity: "Extreme".into(),
                headline: "Tornado Warning".into(),
                area: "Richland".into(),
                description: "A tornado is on the ground.".into(),
            }],
        );
        let routing = choose_dispatch_route(&routes, None, Some(&alerts), world);
        assert!(routing.weather[0].avoid);
        assert!(!routing.weather[1].avoid);
        assert_eq!(routing.pick(), 1, "{:?}", routing.order);
        let line = dispatch_route_line(&routes, &routing, world, &Settings::default());
        assert!(
            line.starts_with("A Tornado Warning covers the road near Mansfield. Dispatch is routing you around it by way of Columbus."),
            "{line}"
        );
        assert!(line.contains("This way adds "), "{line}");
        let note = route_planning_note(&routes, &routing, world);
        assert_eq!(
            note,
            "A Tornado Warning covers the road near Mansfield. Route 1 goes around it by way of Columbus."
        );
    }

    #[test]
    fn test_route_state_keys_name_each_state_once_in_road_order() {
        let world = get_world();
        let routes = world
            .supported_route_options("Chicago", "Indianapolis", 3)
            .expect("routes");
        assert_eq!(route_state_keys(&routes[1]), vec!["illinois", "indiana"]);
        assert_eq!(route_state_keys(&ohio_routes()[1]), vec!["ohio"]);
    }

    fn route(cities: &[&str]) -> Route {
        get_world()
            .route_from_cities(cities)
            .unwrap_or_else(|| panic!("{cities:?} is on the map"))
    }

    #[test]
    fn test_route_state_keys_warm_california_by_district() {
        // The bare state key fetches nothing in California: the warm-up has
        // to name the district feeds dispatch will read.
        assert_eq!(
            route_state_keys(&route(&["las_vegas_nv_us", "los_angeles_ca_us"])),
            vec!["nevada", "california/d7", "california/d8"]
        );
    }

    /// District 7's closures from the parser fixture, read at its own hour.
    fn district_7() -> RealTrafficProvider {
        let events = crate::sim::real_traffic_parsers::parse_lcs_csv(
            include_bytes!("real_traffic_parsers/fixtures/lcs_sample.csv"),
            1_790_272_983.0,
        );
        seeded("california/d7", events)
    }

    #[test]
    fn test_a_closure_between_route_points_is_found_where_it_lies() {
        // I-5 north from Los Angeles has route points 28 miles apart. The
        // Castaic lane closure lies 10 miles from the nearest of them and
        // under half a mile off the road between, 38.5 miles out.
        let world = get_world();
        let north = route(&["los_angeles_ca_us", "bakersfield_ca_us"]);
        let report = scan_route_construction(&north, &district_7(), world);
        assert_eq!(report.spots.len(), 1, "{:?}", report.spots);
        assert_eq!(report.spots[0].road, "I-5");
        assert!(
            (report.spots[0].route_mile - 38.5).abs() < 1.0,
            "{:?}",
            report.spots
        );
    }

    #[test]
    fn test_a_closure_on_the_far_carriageway_is_not_on_the_route() {
        // The Castaic closure holds I-5 North; a truck headed south to Los
        // Angeles drives the other side of the median.
        let world = get_world();
        let south = route(&["bakersfield_ca_us", "los_angeles_ca_us"]);
        let report = scan_route_construction(&south, &district_7(), world);
        assert!(report.spots.is_empty(), "{:?}", report.spots);
    }

    #[test]
    fn test_spots_that_read_the_same_are_said_once_with_a_count() {
        let spot = |route_mile: f64| ConstructionSpot {
            road: "I-80".to_string(),
            near_city: "Des Moines".to_string(),
            closure: "single lane".to_string(),
            length_mi: 5.0,
            limit_mph: 45.0,
            route_mile,
            delay_h: 0.05,
        };
        let mut other = spot(90.0);
        other.near_city = "Iowa City".to_string();
        let spots = vec![spot(10.0), spot(20.0), spot(30.0), other];
        assert_eq!(
            spots_text(&spots, &Settings::default()),
            "one lane closed on I-80 near Des Moines in 3 places, 5 miles each, 45 miles per \
             hour, about 3 minutes each; one lane closed on I-80 near Iowa City, 5 miles, 45 \
             miles per hour, about 3 minutes"
        );
    }
}
