//! Source-backed city service and local route helpers for world data (port of
//! `freight_fate/data/world_services.py` -- Python's `WorldServiceMixin`,
//! here a second `impl World` block).

use once_cell::sync::Lazy;
use regex::Regex;

use super::world::World;
use super::world_constants::{
    lookup, CITY_SERVICE_ORDER, CITY_SERVICE_SOURCE_NOTES, FACILITY_APPROACH_MILES,
    FACILITY_APPROACH_ROADS, FACILITY_APPROACH_TRUSTED_MAX_MI,
};
use super::world_models::{
    CityService, DataError, Driveway, FacilityApproach, FacilityEndpoint, Leg, LocalApproach,
    LocalGeometry, LocalGeometrySegment, Route,
};
use super::world_parsing::crc32;
use crate::pyfmt::round_py_n;

static ROAD_REF_LIST: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\(([^()]*;[^()]*)\)").expect("valid regex"));

/// Josh's ruling (2026-07-24): local deadheads run 1 to 9 miles. The bake
/// already floors facilities at 2.1; this caps the synthetic single-leg
/// top end until the placement audit re-geocodes the misplaced pins.
pub const SYNTHETIC_APPROACH_CAP_MI: f64 = 9.0;

/// The label the local-road builders retired on 2026-08-25 for a road OSM
/// holds no name for. It was baked into `facility_approaches.json` as if it
/// were a name, and that file was never re-run after the builders changed,
/// so 3,230 turn-level segments still carry it verbatim -- "Turn left onto
/// unnamed public road" on the way into Gary Cross-Dock (agent drive,
/// 2026-09-01). Read aloud it tells the driver nothing but the absence of
/// a name, which is exactly what the builders stopped saying.
const RETIRED_UNNAMED_ROAD: &str = "unnamed public road";

/// What a road with no usable name is spoken as (`docs/ontology.md`, the
/// same stand-in `tools/build_facility_approaches.py::clean_segment` picks
/// for a segment that carried no road at all). A stale record cannot say
/// whether the road is a service way or a residential street -- the class
/// is not baked -- so this is the no-information wording; the re-baked
/// data says which of the two it really is.
pub const UNNAMED_STREET: &str = "a side street";

/// Trim raw OSM ref lists out of player-facing street text, and retire the
/// pre-2026-08-25 "unnamed public road" label wherever a stale bake still
/// carries it.
///
/// Source-backed street names sometimes carry the full multi-ref
/// parenthetical straight from the map tags -- "North Michigan Street
/// (SR 933;BUS US 31)". Read aloud, the semicolon list is tag soup, so
/// keep the first ref only: "North Michigan Street (SR 933)".
pub fn spoken_road_text(text: &str) -> String {
    let text = if text.contains(RETIRED_UNNAMED_ROAD) {
        text.replace(RETIRED_UNNAMED_ROAD, UNNAMED_STREET)
    } else {
        text.to_string()
    };
    if text.is_empty() || !text.contains(';') {
        return text;
    }
    let text = ROAD_REF_LIST
        .replace_all(&text, |caps: &regex::Captures| {
            let first = caps[1].split(';').next().unwrap_or("").trim().to_string();
            format!("({first})")
        })
        .into_owned();
    first_of_a_bare_list(&text)
}

/// The same trim for a list that is not in parentheses: a road with no name
/// of its own is baked under its refs ("Continue onto I 70 BUS;US 6;US 50."),
/// and a street the map gives two names carries both ("Ellsworth Street
/// Southwest;Albany-Corvallis Highway (US 20)"). Keep the first, and keep
/// whatever follows the list: the parenthetical ref, the cue's full stop.
fn first_of_a_bare_list(text: &str) -> String {
    let mut out = text.to_string();
    while let Some(start) = out.find(';') {
        let rest = &out[start..];
        let end = rest
            .find(" (")
            .or_else(|| rest.strip_suffix('.').map(str::len))
            .unwrap_or(rest.len());
        let kept = out[..start].trim_end().len();
        out.replace_range(kept..start + end, "");
    }
    out
}

/// Maneuver direction baked in a local segment cue: "left", "right",
/// "ahead", or "" for a directionless legacy cue ("Turn onto"). The one
/// reader of a cue's side, shared by the earcon and the reversed chain, so
/// the two can never disagree about which way a corner goes.
///
/// The maneuver is the cue's opening words, never a word anywhere in it:
/// read as a substring, "Continue onto North Brighton Boulevard" was a right
/// turn and chimed in the right ear for a street that runs straight on
/// (player report, 2026-10-14).
pub fn cue_direction(cue: &str) -> &'static str {
    let lowered = cue.trim().to_lowercase();
    let mut words = lowered.split_whitespace();
    match (words.next(), words.next()) {
        (Some("continue" | "start"), _) => "ahead",
        (Some(_), Some("left")) => "left",
        (Some(_), Some("right")) => "right",
        _ => "",
    }
}

/// A baked facility or road-stop chain as a drivable same-city route, one
/// leg a street, each carrying its spoken cue, turn angle and street detail.
/// The streets from the chain's driveway on are its yard or lot
/// (`Leg::local_yard`).
pub(crate) fn local_chain_route(
    city: &str,
    segments: &[LocalGeometrySegment],
    driveway: Option<&Driveway>,
) -> Route {
    let yard_from = driveway.and_then(|driveway| driveway_leg_index(segments, driveway.at_mi));
    let legs: Vec<Leg> = segments
        .iter()
        .enumerate()
        .map(|(i, segment)| {
            Leg::local(
                city,
                segment.miles,
                &spoken_road_text(&segment.road),
                &spoken_road_text(&segment.cue),
                segment.speed_mph,
            )
            .with_turn_deg(segment.turn_deg)
            .with_street(segment.limit.clone(), segment.controls.clone())
            .with_yard(yard_from.is_some_and(|from| i >= from))
        })
        .collect();
    let cities = vec![city.to_string(); legs.len() + 1];
    Route::from_legs(cities, legs)
}

/// The leg a chain's driveway starts, when it stands on a leg boundary (the
/// bake's segments and its driveway are both rounded to the hundredth), and
/// never the first leg: a chain that starts in the yard has no street.
fn driveway_leg_index(segments: &[LocalGeometrySegment], at_mi: f64) -> Option<usize> {
    let mut start = 0.0;
    for (i, segment) in segments.iter().enumerate() {
        if i > 0 && (start - at_mi).abs() <= 0.015 {
            return Some(i);
        }
        start += segment.miles;
    }
    None
}

/// The same street chain driven outbound: leg order reversed, and each
/// junction's turn direction flipped (an inbound right turn is an outbound
/// left at the same corner). Near-straight boundaries stay "Continue onto";
/// directionless legacy cues stay directionless.
fn reversed_local_legs(city: &str, legs: &[std::sync::Arc<Leg>]) -> Vec<Leg> {
    let mut out = Vec::with_capacity(legs.len());
    for (i, src) in legs.iter().rev().enumerate() {
        // Outbound, the junction onto this leg is the one the inbound drive
        // crossed *leaving* it, so everything about that corner -- the hand it
        // turns and how sharply -- is baked on the leg AFTER this one. The
        // first leg out is started on, not turned onto, so it has no corner.
        let inbound = (i > 0).then(|| &legs[legs.len() - i]);
        let cue = match inbound {
            None => format!("Start on {}.", src.highway),
            Some(inbound) => match cue_direction(&inbound.local_cue) {
                "left" => format!("Turn right onto {}.", src.highway),
                "right" => format!("Turn left onto {}.", src.highway),
                "ahead" => format!("Continue onto {}.", src.highway),
                _ => format!("Turn onto {}.", src.highway),
            },
        };
        out.push(
            Leg::local(city, src.miles, &src.highway, &cue, src.local_speed_mph)
                // The same corner driven the other way, so its angle is
                // unchanged -- only the hand it falls on flips, and the cue
                // above has already flipped that. It has to be read off the
                // SAME leg the cue was, though: read off `src` instead, every
                // outbound corner was priced and leaned from its neighbour's
                // shape, a "Continue onto" inherited a corner's angle, and the
                // last corner of every departure read as unmeasured.
                .with_turn_deg(inbound.map_or(0.0, |inbound| inbound.local_turn_deg))
                // A street's limit and the yard are the same whichever way
                // they are driven; its READ controls face one direction of
                // travel and are not carried (a stop sign facing the inbound
                // truck says nothing about the outbound one).
                .with_street(src.local_limit.clone(), Vec::new())
                .with_yard(src.local_yard),
        );
    }
    out
}

impl World {
    /// Service POIs available for local city driving.
    ///
    /// Source-backed entries from `city_services.json` are preferred per
    /// service key. Missing keys stay available as representative fallback
    /// services so the existing offline menu contract remains complete.
    /// `CityService.city` carries the canonical city key so it round-trips
    /// through the other service lookups; spoken text uses `name`.
    pub fn city_services(&self, city: &str) -> Result<Vec<CityService>, DataError> {
        let city_key = self.resolve_city_key(city);
        let Some(city_obj) = self.cities.get(&city_key) else {
            return Err(DataError::key(format!("Unknown city: {city}")));
        };
        let source_entries = self.city_service_data()?.get(&city_key);
        let mut services = Vec::with_capacity(CITY_SERVICE_ORDER.len());
        for key in CITY_SERVICE_ORDER {
            let Some(raw) = source_entries.and_then(|entries| entries.get(*key)) else {
                services.push(self.fallback_city_service(&city_key, key)?);
                continue;
            };
            let mut kind = raw.kind.as_deref().unwrap_or(key).trim().to_string();
            if kind.is_empty() {
                kind = key.to_string();
            }
            services.push(CityService {
                key: key.to_string(),
                name: raw.name.trim().to_string(),
                city: city_key.clone(),
                state: city_obj.state.clone(),
                kind,
                source_note: raw.source_note.trim().to_string(),
                lat: raw.lat,
                lon: raw.lon,
                approach_miles: round_py_n(raw.approach_miles, 1),
                approach_road: raw.approach_road.trim().to_string(),
                source_type: raw
                    .source_type
                    .as_deref()
                    .unwrap_or("osm")
                    .trim()
                    .to_string(),
                source_ref: raw.source_ref.trim().to_string(),
                fallback: raw.fallback,
                fallback_reason: raw.fallback_reason.trim().to_string(),
            });
        }
        Ok(services)
    }

    fn fallback_city_service(&self, city_key: &str, key: &str) -> Result<CityService, DataError> {
        let city_obj = &self.cities[city_key];
        let terminal = self.home_terminal(city_key)?;
        let name = match key {
            "freight_market" => format!("{} Freight Market Office", city_obj.name),
            "garage" => format!("{} Garage", terminal.name),
            "truck_dealer" => format!("{} Truck Dealer", city_obj.name),
            other => return Err(DataError::key(py_key(other))),
        };
        Ok(CityService {
            key: key.to_string(),
            name,
            city: city_key.to_string(),
            state: city_obj.state.clone(),
            kind: key.to_string(),
            source_note: lookup(CITY_SERVICE_SOURCE_NOTES, key)
                .unwrap_or_default()
                .to_string(),
            fallback_reason: "No checked-in source-backed city service entry for this role."
                .to_string(),
            ..CityService::default()
        })
    }

    pub fn city_service(&self, city: &str, key: &str) -> Result<CityService, DataError> {
        self.city_services(city)?
            .into_iter()
            .find(|service| service.key == key)
            .ok_or_else(|| DataError::key(format!("Unknown service in {city}: {key}")))
    }

    pub fn local_approach(&self, target_id: &str) -> Result<Option<&LocalApproach>, DataError> {
        Ok(self.local_approaches()?.get(target_id))
    }

    pub fn local_geometry(&self, target_id: &str) -> Result<Option<&LocalGeometry>, DataError> {
        Ok(self.local_geometries()?.get(target_id))
    }

    pub fn facility_approach(
        &self,
        city: &str,
        location_name: &str,
    ) -> Result<Option<&LocalApproach>, DataError> {
        let location = self.facility_location(city, location_name)?;
        self.local_approach(&format!("facility:{}", location.id))
    }

    pub fn facility_endpoint(
        &self,
        city: &str,
        location_name: &str,
    ) -> Result<Option<&FacilityEndpoint>, DataError> {
        let location = self.facility_location(city, location_name)?;
        Ok(self.facility_endpoints()?.get(&location.id))
    }

    pub fn facility_source_approach(
        &self,
        city: &str,
        location_name: &str,
    ) -> Result<Option<&FacilityApproach>, DataError> {
        let location = self.facility_location(city, location_name)?;
        Ok(self.facility_approaches()?.get(&location.id))
    }

    /// Local approach road a HIGHWAY run has to cover to reach this gate.
    ///
    /// The arrival zones size the destination approach from this rather than
    /// from a flat mileage: the facilities differ hugely, and a number that
    /// fits a dock two ramps off the interstate is a crawl for one sitting on
    /// the frontage road.
    ///
    /// `None` means "no usable geometry, size it synthetically", and it is
    /// the answer in two different cases. A facility with a genuine
    /// turn-level street chain has that chain driven as a route of its own
    /// once the highway run ends, so counting its mileage here as well would
    /// slow the freeway for road the truck has not reached. A facility whose
    /// record is a fallback, or whose endpoint estimate is longer than any
    /// real approach road (the misplaced pins the synthetic cap already
    /// guards), has nothing worth believing.
    pub fn facility_approach_miles(
        &self,
        city: &str,
        location_name: &str,
    ) -> Result<Option<f64>, DataError> {
        let Ok(location) = self.facility_location(city, location_name) else {
            return Ok(None);
        };
        let approach = self.facility_approaches()?.get(&location.id);
        if let Some(approach) = approach {
            if approach.turn_level && !approach.segments.is_empty() {
                return Ok(None); // its own street chain covers this road
            }
            if !approach.fallback && approach.total_miles > 0.0 {
                return Ok(Some(approach.total_miles));
            }
        }
        if let Some(endpoint) = self.facility_endpoints()?.get(&location.id) {
            if endpoint.source_backed && !endpoint.fallback {
                let miles = endpoint.approach_miles;
                if 0.0 < miles && miles <= FACILITY_APPROACH_TRUSTED_MAX_MI {
                    return Ok(Some(miles));
                }
            }
        }
        Ok(None)
    }

    pub fn facility_geometry(
        &self,
        city: &str,
        location_name: &str,
    ) -> Result<Option<&LocalGeometry>, DataError> {
        let location = self.facility_location(city, location_name)?;
        self.local_geometry(&format!("facility:{}", location.id))
    }

    /// The facility's street chain from one ramp terminal -- the OSM node an
    /// exit ramp ends at (`Trip::ramp_terminal_node_at`) -- to the facility,
    /// whole, or None when no chain was baked from that terminal.
    pub fn facility_exit_route(
        &self,
        city: &str,
        location_name: &str,
        terminal_node: i64,
    ) -> Result<Option<Route>, DataError> {
        let city = self.resolve_city_key(city);
        let location = self.facility_location(&city, location_name)?;
        Ok(self
            .facility_approaches()?
            .get(&location.id)
            .and_then(|approach| {
                approach
                    .exit_chains
                    .iter()
                    .find(|chain| chain.terminal_node == terminal_node)
            })
            .map(|chain| local_chain_route(&city, &chain.segments, chain.driveway.as_ref())))
    }

    /// Where a facility's street chain leaves the public street: the chain
    /// from `terminal_node`, or the default chain for None. None when that
    /// chain ends on a public street, or there is no such chain.
    pub fn facility_driveway(
        &self,
        city: &str,
        location_name: &str,
        terminal_node: Option<i64>,
    ) -> Result<Option<Driveway>, DataError> {
        let location = self.facility_location(city, location_name)?;
        let Some(approach) = self.facility_approaches()?.get(&location.id) else {
            return Ok(None);
        };
        Ok(match terminal_node {
            None => approach.driveway.clone(),
            Some(node) => approach
                .exit_chains
                .iter()
                .find(|chain| chain.terminal_node == node)
                .and_then(|chain| chain.driveway.clone()),
        })
    }

    /// The facility's street chain driven outbound -- gate toward the
    /// highway on-ramp -- or `None` when the facility has no genuine
    /// multi-segment turn-level chain (those keep the scripted departure).
    /// Mirrors the arrival-side chain gating in the driving layer.
    pub fn facility_departure_route(
        &self,
        city: &str,
        location_name: &str,
    ) -> Result<Option<Route>, DataError> {
        let route = self.facility_approach_route(city, location_name)?;
        if route.legs.len() < 2 {
            return Ok(None);
        }
        if !route.legs.iter().any(|leg| leg.local_speed_mph > 0.0) {
            return Ok(None);
        }
        let city = self.resolve_city_key(city);
        let legs = reversed_local_legs(&city, &route.legs);
        let cities = vec![city; legs.len() + 1];
        Ok(Some(Route::from_legs(cities, legs)))
    }

    /// A short, drivable local route from the company terminal to a facility.
    pub fn facility_approach_route(
        &self,
        city: &str,
        location_name: &str,
    ) -> Result<Route, DataError> {
        self.facility_approach_route_with(city, location_name, true)
    }

    /// `facility_approach_route`, or with `streets` false the single
    /// synthetic access-road leg every facility had in 1.8, even one with a
    /// turn-level street chain (the Facility streets setting).
    pub fn facility_approach_route_with(
        &self,
        city: &str,
        location_name: &str,
        streets: bool,
    ) -> Result<Route, DataError> {
        let city = self.resolve_city_key(city);
        let location = self.facility_location(&city, location_name)?;
        if let Some(source_approach) = self
            .facility_approaches()?
            .get(&location.id)
            .filter(|_| streets)
        {
            if source_approach.turn_level && !source_approach.segments.is_empty() {
                return Ok(local_chain_route(
                    &city,
                    &source_approach.segments,
                    source_approach.driveway.as_ref(),
                ));
            }
        }
        let endpoint = self.facility_endpoints()?.get(&location.id);
        let approach = self.local_approach(&format!("facility:{}", location.id))?;
        let (mut miles, road) = match (endpoint, approach) {
            (Some(endpoint), _) if endpoint.source_backed && !endpoint.fallback => (
                endpoint.approach_miles,
                approach
                    .map(|a| a.road.clone())
                    .unwrap_or_else(|| endpoint.approach_road.clone()),
            ),
            (_, Some(approach)) => (approach.approach_miles, approach.road.clone()),
            _ => {
                let base_miles =
                    lookup(FACILITY_APPROACH_MILES, &location.facility_type).unwrap_or(4.0);
                let seed = crc32(
                    format!("{city}:{}:{}", location.name, location.facility_type).as_bytes(),
                );
                let offset = f64::from(seed % 7) * 0.25;
                (
                    round_py_n(base_miles + offset, 1),
                    lookup(FACILITY_APPROACH_ROADS, &location.facility_type)
                        .unwrap_or("facility access road")
                        .to_string(),
                )
            }
        };
        // Sanity clamp for the synthetic single-leg approach: 776 baked
        // records carry up to the bake tool's 35-mile cap because the
        // facility's geocoded pin landed tens of miles from its city --
        // Josh drew a 35-mile straight deadhead in Kenosha (2026-07-24).
        // Until the placement audit re-geocodes them (roadmap), no local
        // deadhead crawls half a county; real multi-leg street chains
        // above are never clamped.
        miles = miles.min(SYNTHETIC_APPROACH_CAP_MI);
        let leg = Leg::new(
            &city,
            &city,
            miles,
            &spoken_road_text(&road),
            "flat",
            Vec::new(),
        );
        Ok(Route::from_legs(vec![city.clone(), city], vec![leg]))
    }
}

fn py_key(key: &str) -> String {
    super::world_parsing::py_repr_str(key)
}

#[cfg(test)]
mod tests {
    //! The pure part of `tests/test_street_turns.py`; the world-backed check
    //! lives in `crates/ff-core/tests/data_street_turns.rs`.
    use super::*;

    #[test]
    fn test_spoken_road_text_trims_osm_ref_lists() {
        assert_eq!(
            spoken_road_text("Turn left onto North Michigan Street (SR 933;BUS US 31)."),
            "Turn left onto North Michigan Street (SR 933)."
        );
        assert_eq!(
            spoken_road_text("Slater Street SW (US 19; US 82; GA 3; GA 133; GA 520)"),
            "Slater Street SW (US 19)"
        );
        // Single refs and plain names pass through untouched.
        assert_eq!(
            spoken_road_text("Main Street (SR 26)"),
            "Main Street (SR 26)"
        );
        assert_eq!(
            spoken_road_text("Richard G. Hatcher Boulevard"),
            "Richard G. Hatcher Boulevard"
        );
    }

    #[test]
    fn test_spoken_road_text_keeps_the_first_of_a_list_outside_parentheses() {
        // A road baked under its refs because it has no name of its own, and
        // a street the map gives two names. 79 such strings reached the
        // facility approaches in the 2026-09-17 sweep; read aloud each was
        // tag soup. This used to assert the opposite on an invented sentence
        // ("no parens; still fine"), before the data showed the real case.
        assert_eq!(
            spoken_road_text("Continue onto I 70 BUS;US 6;US 50."),
            "Continue onto I 70 BUS."
        );
        assert_eq!(
            spoken_road_text("I 75 Business; US 41; GA 7"),
            "I 75 Business"
        );
        assert_eq!(
            spoken_road_text("Ellsworth Street Southwest;Albany-Corvallis Highway (US 20)"),
            "Ellsworth Street Southwest (US 20)"
        );
        assert_eq!(
            spoken_road_text("Turn right onto Main Street;Old Post Road (US 11;NY 12)."),
            "Turn right onto Main Street (US 11)."
        );
    }

    #[test]
    fn test_spoken_road_text_retires_unnamed_public_road() {
        // Heard on the 2026-09-01 agent drive into Gary Cross-Dock: the
        // facility approach bake predates the builders' class labels and
        // still carries the retired wording as a name, in both the road
        // and the cue sentence around it.
        assert_eq!(spoken_road_text("unnamed public road"), "a side street");
        assert_eq!(
            spoken_road_text("Turn left onto unnamed public road."),
            "Turn left onto a side street."
        );
        assert_eq!(
            spoken_road_text("Start on unnamed public road."),
            "Start on a side street."
        );
        assert_eq!(
            spoken_road_text("Use unnamed public road for the local approach."),
            "Use a side street for the local approach."
        );
        // The class labels the builders write now pass through untouched.
        assert_eq!(spoken_road_text("a service road"), "a service road");
        assert_eq!(
            spoken_road_text("Turn right onto a side street."),
            "Turn right onto a side street."
        );
    }

    #[test]
    fn reversed_chain_flips_each_junction() {
        let city = "south_bend_in_us";
        let legs = vec![
            std::sync::Arc::new(Leg::local(
                city,
                0.15,
                "East Navarre Street",
                "Start on East Navarre Street.",
                25.0,
            )),
            std::sync::Arc::new(Leg::local(
                city,
                0.2,
                "North Michigan Street",
                "Turn left onto North Michigan Street.",
                25.0,
            )),
            std::sync::Arc::new(Leg::local(
                city,
                0.5,
                "South Michigan Street",
                "Continue onto South Michigan Street.",
                30.0,
            )),
        ];
        let out = reversed_local_legs(city, &legs);
        let cues: Vec<&str> = out.iter().map(|l| l.local_cue.as_str()).collect();
        assert_eq!(
            cues,
            vec![
                "Start on South Michigan Street.",
                "Continue onto North Michigan Street.",
                "Turn right onto East Navarre Street.",
            ]
        );
        assert_eq!(out[2].local_speed_mph, 25.0);
    }

    #[test]
    fn reversed_chain_carries_each_corners_own_angle() {
        // The angle has to travel with the cue, not with the leg it is baked
        // on. Read off the wrong leg, the 112-degree corner onto North
        // Michigan came out on the "Continue onto" that is not a corner at
        // all, and the real outbound corner read as unmeasured -- so it was
        // priced and leaned as a square one.
        let city = "south_bend_in_us";
        let legs = vec![
            std::sync::Arc::new(Leg::local(
                city,
                0.15,
                "East Navarre Street",
                "Start on East Navarre Street.",
                25.0,
            )),
            std::sync::Arc::new(
                Leg::local(
                    city,
                    0.2,
                    "North Michigan Street",
                    "Turn left onto North Michigan Street.",
                    25.0,
                )
                .with_turn_deg(112.0),
            ),
            std::sync::Arc::new(Leg::local(
                city,
                0.5,
                "South Michigan Street",
                "Continue onto South Michigan Street.",
                30.0,
            )),
        ];

        let out = reversed_local_legs(city, &legs);

        // Started on, so no corner; the near-straight boundary is not one
        // either; and the turn back onto East Navarre is the 112 the inbound
        // drive measured leaving it.
        let angles: Vec<f64> = out.iter().map(|leg| leg.local_turn_deg).collect();
        assert_eq!(angles, vec![0.0, 0.0, 112.0]);
    }
}
