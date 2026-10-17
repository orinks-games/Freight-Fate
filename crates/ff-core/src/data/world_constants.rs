//! Shared constants for the world data model (port of
//! `freight_fate/data/world_constants.py`).
//!
//! Python kept these as module-level dicts, sets and tuples; here the same
//! tables are `&'static` slices of pairs with small lookup helpers, and the
//! two large city-key gates are hashed once on first use.

/// Value for `key` in a `(key, value)` table, or `None`.
pub fn lookup<'a, V: Copy>(table: &'a [(&'a str, V)], key: &str) -> Option<V> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

/// Whether `key` is in a string-set table.
pub fn set_contains(table: &[&str], key: &str) -> bool {
    table.contains(&key)
}

pub const STOP_TYPE_LABELS: &[(&str, &str)] = &[
    ("truck_stop", "truck stop"),
    ("travel_center", "travel center"),
    ("fuel_station", "truck fuel station"),
    ("service_plaza", "service plaza"),
    ("public_rest_area", "public rest area"),
    ("truck_parking", "truck parking"),
    ("weigh_station", "weigh station"),
    ("repair_shop", "repair shop"),
];

pub const PARKING_CERTAINTY_LABELS: &[(&str, &str)] = &[
    ("confirmed", "confirmed truck parking"),
    ("likely", ""),
    ("limited", "limited truck parking"),
    ("unknown", "parking not verified"),
    ("none", "no truck parking"),
];

pub const STOP_CURATION_LEVELS: &[&str] = &["curated", "placeholder"];

pub const STOP_DIRECTIONS: &[&str] = &["both", "forward", "reverse"];

// Can the rig physically get in? A separate axis from parking certainty above:
// parking says whether there is room to STOP, this says whether a combination
// vehicle can enter the lot at all. A car-scale convenience store may sell
// diesel and still have no way for a 70-foot rig to turn around in it.
//   tractor_trailer -- announced and usable normally
//   bobtail_only    -- on the map, but only reachable running tractor-only;
//                      an empty trailer is still a trailer
//   none            -- landmark only, never a stop
pub const VEHICLE_ACCESS_LEVELS: &[&str] = &["tractor_trailer", "bobtail_only", "none"];

pub const DEFAULT_VEHICLE_ACCESS: &str = "tractor_trailer";

/// Whether a stop with this access level is usable by the current rig.
///
/// One rule, shared by the world data model and the runtime road stop, so
/// announcements, exit arming, HOS planning, and the tablet can never
/// disagree about whether a stop is real for this player.
pub fn vehicle_access_allows(access: &str, bobtail: bool) -> bool {
    if access == "bobtail_only" {
        return bobtail;
    }
    access != "none"
}

/// National truck-stop chains, lower-cased. A stop whose name starts with
/// one of these is read as truck-serving whatever else its record left
/// unsaid: the chain's business is trucks. Read from the operator's own
/// knowledge of the industry, not from the record.
pub const TRUCK_STOP_CHAINS: &[&str] = &[
    "love's",
    "pilot",
    "flying j",
    "ta ",
    "travelcenters",
    "petro",
    "road ranger",
    "one9",
    "sapp bros",
    // The map spells it out: "Sapp Brothers Travel Center".
    "sapp brothers",
    "bosselman",
    "iowa 80",
    "little america",
    "ambest",
    "roady's",
    "stamart",
    "onvo",
    "kwik trip",
    "kwik star",
];

/// Words in a stop's own name that say trucks are served there.
pub const TRUCK_STOP_NAME_WORDS: &[&str] = &[
    "truck",
    "travel",
    "plaza",
    "rest area",
    "service area",
    "traffic center",
    "fuel center",
    "welcome center",
];

/// The access level a stop's own record supports.
///
/// The map's curation tool typed a good many convenience stations as travel
/// centers and left their access at the `tractor_trailer` default with
/// parking merely "likely": a Circle K or a QuikTrip announced and armed like
/// a truck stop (owner, 2026-09-12: "stops on the map like Circle K where a
/// truck would never park"). A silent upstream is not a reading, so this
/// screens that default at load and never edits the bake: a fuel-type stop
/// with no confirmed parking, no surveyed spaces, no truck facility in its
/// services, no truck-stop chain at the head of its name and no truck word
/// in it is read as `bobtail_only`, which is what the same tool gave the
/// stations it did recognise as generic. Everything else is returned as
/// recorded, explicit values included.
pub fn screened_vehicle_access<'a>(
    name: &str,
    stop_type: &str,
    parking: &str,
    parking_spaces: i64,
    services: &[String],
    vehicle_access: &'a str,
) -> &'a str {
    if vehicle_access != DEFAULT_VEHICLE_ACCESS {
        return vehicle_access;
    }
    if !matches!(
        stop_type,
        "travel_center" | "service_plaza" | "fuel_station"
    ) {
        return vehicle_access;
    }
    if parking == "confirmed" || parking_spaces > 0 {
        return vehicle_access;
    }
    if services.iter().any(|s| s == "scale" || s == "showers") {
        return vehicle_access;
    }
    let lower = name.trim().to_lowercase();
    if TRUCK_STOP_CHAINS
        .iter()
        .any(|chain| lower.starts_with(chain) || lower == chain.trim())
        || TRUCK_STOP_NAME_WORDS
            .iter()
            .any(|word| lower.contains(word))
    {
        return vehicle_access;
    }
    "bobtail_only"
}

#[cfg(test)]
mod screen_tests {
    use super::*;

    fn screen(
        name: &str,
        stop_type: &str,
        parking: &str,
        spaces: i64,
        services: &[&str],
    ) -> &'static str {
        let services: Vec<String> = services.iter().map(|s| s.to_string()).collect();
        screened_vehicle_access(
            name,
            stop_type,
            parking,
            spaces,
            &services,
            DEFAULT_VEHICLE_ACCESS,
        )
    }

    #[test]
    fn test_a_convenience_station_typed_as_a_travel_center_is_bobtail_only() {
        assert_eq!(
            screen(
                "QuikTrip",
                "travel_center",
                "likely",
                0,
                &["diesel", "parking"]
            ),
            "bobtail_only"
        );
        assert_eq!(
            screen("Circle K", "service_plaza", "likely", 0, &["diesel"]),
            "bobtail_only"
        );
        assert_eq!(
            screen("Speedway", "fuel_station", "limited", 0, &["diesel"]),
            "bobtail_only"
        );
        assert_eq!(
            screen("Horner Industrial Group", "travel_center", "likely", 0, &[]),
            "bobtail_only"
        );
    }

    #[test]
    fn test_evidence_of_trucks_keeps_the_recorded_access() {
        assert_eq!(
            screen(
                "Love's Travel Stop",
                "travel_center",
                "likely",
                0,
                &["diesel"]
            ),
            "tractor_trailer"
        );
        assert_eq!(
            screen("Pilot", "service_plaza", "likely", 0, &["diesel"]),
            "tractor_trailer"
        );
        assert_eq!(
            screen("TA Ontario", "travel_center", "likely", 0, &[]),
            "tractor_trailer"
        );
        assert_eq!(
            screen("Plaza 23 Truck Stop", "travel_center", "likely", 0, &[]),
            "tractor_trailer"
        );
        assert_eq!(
            screen("Allentown Service Plaza", "service_plaza", "likely", 0, &[]),
            "tractor_trailer"
        );
        assert_eq!(
            screen("Shell", "travel_center", "confirmed", 0, &["diesel"]),
            "tractor_trailer"
        );
        assert_eq!(
            screen("Shell", "travel_center", "likely", 40, &["diesel"]),
            "tractor_trailer"
        );
        assert_eq!(
            screen("Shell", "travel_center", "likely", 0, &["diesel", "scale"]),
            "tractor_trailer"
        );
        // Types that serve trucks by definition are never screened.
        assert_eq!(
            screen("Bay 2", "public_rest_area", "likely", 0, &[]),
            "tractor_trailer"
        );
        assert_eq!(
            screen("Bay 2", "weigh_station", "none", 0, &[]),
            "tractor_trailer"
        );
    }

    #[test]
    fn test_explicit_values_pass_through_untouched() {
        let services: Vec<String> = Vec::new();
        assert_eq!(
            screened_vehicle_access(
                "Circle K",
                "fuel_station",
                "limited",
                0,
                &services,
                "bobtail_only"
            ),
            "bobtail_only"
        );
        assert_eq!(
            screened_vehicle_access("Circle K", "fuel_station", "limited", 0, &services, "none"),
            "none"
        );
    }
}

// Alternate routes should feel like real dispatch choices, not graph leftovers.
// A little extra mileage is fine for traffic, weather, grades, or avoiding a
// metro corridor; hundreds of out-of-direction miles on a short lane are not.
pub const ALTERNATE_ROUTE_EXTRA_RATIO: f64 = 0.22;
pub const ALTERNATE_ROUTE_MIN_EXTRA_MILES: f64 = 75.0;
pub const ALTERNATE_ROUTE_MAX_EXTRA_MILES: f64 = 550.0;

// How long a baked posting must hold to count as a sign rather than a way
// boundary. OSM splits a way wherever any tag changes, so the maxspeed profile
// carries postings a few hundred feet long; under time compression those go by
// in a second and read as the limit flickering for no reason.
//
// Measured in REAL seconds, never in miles -- the same law the keeper ease, the
// turn call and the zone warning already follow. A mile is not one experience:
// at 70 it is under three real seconds and reads as a blink, at 30 it is over
// ten and reads as a town. A mile-based bar was the 2026-08-11 attempt at this
// and left 803 postings the truck crossed in under three real seconds.
pub const LIMIT_DWELL_REAL_S: f64 = 6.0;
// ...unless a place on the road explains it, and the drop is to a speed a place
// posts. A village main street really is short, so length alone would delete
// the signs along with the noise. Shaving five off a highway limit beside a
// village is not the village's doing, though -- that pass is what kept a
// quarter-mile 80-to-75-to-80 on I-44 -- so the exception is a lower bar for a
// town speed, never a free pass.
pub const LIMIT_PLACE_DWELL_REAL_S: f64 = 3.0;
pub const LIMIT_PLACE_NEAR_MI: f64 = 1.0;
pub const LIMIT_PLACE_TOWN_MPH: f64 = 45.0;
pub const LIMIT_EXPLAINING_CATEGORIES: &[&str] = &["village"];
// The drop-and-return postings `tools/screen_limit_artifacts.py` found off the
// leg's own highway (a ramp or a city street the route geometry ran over) are
// folded back into their neighbours at load. Both numbers come from a sample of
// 100 postings checked by hand against OpenStreetMap (2026-10-17): the longest
// artifact ran 2.5 miles, and keeping any posting with a village or town within
// a quarter mile folded none of the 49 real zones.
pub const LIMIT_ARTIFACT_MAX_RUN_MI: f64 = 2.5;
pub const LIMIT_ARTIFACT_PLACE_GUARD_MI: f64 = 0.25;
pub const LIMIT_ARTIFACT_PLACE_CHECKPOINTS: &[&str] = &["place", "village", "town", "hamlet"];
// Real seconds become miles at the pacing the game actually runs, which the
// data layer cannot ask the sim for (it never imports it) and must not ask the
// player's settings for (the world is parsed once, and world data has to be the
// same for everybody). These mirror the standard pace and the compression ramp
// in sim/trip_models.py; test_maxspeed pins them together so they cannot drift.
pub const LIMIT_DWELL_REFERENCE_SCALE: f64 = 20.0;
pub const LIMIT_DWELL_LOW_SPEED_SCALE: f64 = 4.0;
pub const LIMIT_DWELL_FULL_COMPRESSION_MPH: f64 = 50.0;
// What an untagged stretch is assumed to be driven at, for sizing only.
pub const LIMIT_DWELL_FALLBACK_MPH: f64 = 55.0;

pub const POI_DENSITY_SHORT_LEG_MILES: f64 = 160.0;
pub const POI_DENSITY_MEDIUM_LEG_MILES: f64 = 320.0;

pub const POI_ACTIONS: &[&str] = &[
    "break",
    "food",
    "fuel",
    "inspect",
    "park",
    "repair",
    "roadside_assistance",
    "save",
    "sleep",
    "towing",
];

pub const RAW_POI_TEXT_MARKERS: &[&str] = &[
    "osm_id",
    "openstreetmap id",
    "amenity=",
    "highway=",
    "operator=",
    "node/",
    "way/",
    "relation/",
];

pub const TOLL_METHOD_LABELS: &[(&str, &str)] = &[
    ("cash_card", "cash or card"),
    ("ticket_system", "ticket system"),
    ("transponder", "transponder"),
    ("open_road", "open-road tolling"),
    ("toll_by_plate", "toll by plate"),
    ("ezpass", "E-ZPass"),
];

pub const CITY_SERVICE_SOURCE_NOTES: &[(&str, &str)] = &[
    ("freight_market", "Representative city service POI derived from the metro freight market and checked-in facility taxonomy."),
    ("garage", "Representative terminal garage service POI derived from the home terminal."),
    ("truck_dealer", "Representative truck dealer service POI for the metro service area."),
];

pub const CITY_SERVICE_LABELS: &[(&str, &str)] = &[
    ("freight_market", "freight market office"),
    ("garage", "garage"),
    ("truck_dealer", "truck dealer"),
];

pub const CITY_SERVICE_ORDER: &[&str] = &["freight_market", "garage", "truck_dealer"];

pub const CITY_SERVICE_SOURCE_TYPES: &[&str] = &["fallback", "operator", "ors", "osm"];

pub const DEFAULT_POI_ACTIONS: &[(&str, &[&str])] = &[
    (
        "truck_stop",
        &["park", "save", "fuel", "food", "break", "sleep"],
    ),
    (
        "travel_center",
        &["park", "save", "fuel", "food", "break", "sleep"],
    ),
    ("fuel_station", &["park", "save", "fuel", "break"]),
    (
        "service_plaza",
        &["park", "save", "fuel", "food", "break", "sleep"],
    ),
    ("public_rest_area", &["park", "save", "break", "sleep"]),
    ("truck_parking", &["park", "save", "break", "sleep"]),
    ("weigh_station", &["inspect"]),
    ("repair_shop", &["park", "save", "repair"]),
];

pub const SOURCE_BACKED_POI_ACTIONS: &[&str] = &["repair", "roadside_assistance", "towing"];

pub const FREIGHT_LOCATION_TYPES: &[&str] = &[
    "air_cargo",
    "automotive_plant",
    "chemical_petroleum_terminal",
    "cold_storage",
    "company_yard",
    "construction_materials_yard",
    "cross_dock",
    "distribution",
    "dry_warehouse",
    "farm_elevator",
    "food_processor",
    "food_terminal",
    "grocery_retail_dc",
    "industrial_park",
    "intermodal",
    "intermodal_ramp",
    "lumber_paper",
    "manufacturing",
    "manufacturing_plant",
    "metro_market",
    "mine_quarry",
    "parcel_hub",
    "port",
    "port_terminal",
    "rail",
    "retail_distribution",
    "steel_industrial",
    "terminal",
    "warehouse",
];

pub const LOCATION_TYPE_LABELS: &[(&str, &str)] = &[
    ("air_cargo", "air cargo area"),
    ("automotive_plant", "automotive plant"),
    (
        "chemical_petroleum_terminal",
        "chemical and petroleum terminal",
    ),
    ("cold_storage", "cold storage"),
    ("company_yard", "company yard"),
    ("construction_materials_yard", "construction materials yard"),
    ("cross_dock", "cross-dock"),
    ("distribution", "distribution center"),
    ("dry_warehouse", "dry warehouse"),
    ("farm_elevator", "farm elevator"),
    ("food_terminal", "food terminal"),
    ("food_processor", "food processor"),
    (
        "grocery_retail_dc",
        "grocery and retail distribution center",
    ),
    ("industrial_park", "industrial park"),
    ("intermodal", "intermodal yard"),
    ("intermodal_ramp", "intermodal ramp"),
    ("lumber_paper", "lumber and paper facility"),
    ("manufacturing", "manufacturing plant"),
    ("manufacturing_plant", "manufacturing plant"),
    ("metro_market", "metro freight market"),
    ("mine_quarry", "mine or quarry"),
    ("parcel_hub", "parcel hub"),
    ("port", "port"),
    ("port_terminal", "port terminal"),
    ("rail", "rail yard"),
    ("retail_distribution", "retail distribution hub"),
    ("steel_industrial", "steel and industrial plant"),
    ("terminal", "freight terminal"),
    ("warehouse", "warehouse"),
];

pub const FACILITY_APPROACH_MILES: &[(&str, f64)] = &[
    ("air_cargo", 7.0),
    ("automotive_plant", 4.5),
    ("chemical_petroleum_terminal", 6.0),
    ("cold_storage", 4.0),
    ("company_yard", 2.5),
    ("construction_materials_yard", 3.5),
    ("cross_dock", 3.5),
    ("distribution", 4.0),
    ("dry_warehouse", 3.5),
    ("farm_elevator", 5.0),
    ("food_terminal", 3.5),
    ("food_processor", 4.5),
    ("grocery_retail_dc", 4.0),
    ("industrial_park", 5.0),
    ("intermodal", 6.0),
    ("intermodal_ramp", 6.0),
    ("lumber_paper", 5.5),
    ("manufacturing", 4.5),
    ("manufacturing_plant", 4.5),
    ("metro_market", 3.0),
    ("mine_quarry", 7.0),
    ("parcel_hub", 4.0),
    ("port", 8.0),
    ("port_terminal", 8.0),
    ("rail", 5.5),
    ("retail_distribution", 4.0),
    ("steel_industrial", 5.5),
    ("terminal", 3.0),
    ("warehouse", 3.5),
];

// How much of a facility's own recorded approach the arrival speed zones will
// believe. Measured off the records themselves: the road-snapped turn-level
// chains, the only approaches that follow real streets end to end, reach 2.49
// miles at the ninetieth percentile. The straight-line endpoint estimates run
// far past that on a long tail of geocoding noise -- 98 of them sit exactly on
// the bake tool's 35-mile cap -- and past this line a record is describing a
// pin in the wrong place rather than a road anybody drives.
pub const FACILITY_APPROACH_TRUSTED_MAX_MI: f64 = 2.5;

pub const FACILITY_APPROACH_ROADS: &[(&str, &str)] = &[
    ("air_cargo", "airport cargo access road"),
    ("automotive_plant", "assembly plant access road"),
    ("chemical_petroleum_terminal", "terminal access road"),
    ("cold_storage", "cold storage access road"),
    ("company_yard", "company yard access road"),
    ("construction_materials_yard", "materials yard access road"),
    ("cross_dock", "cross-dock access road"),
    ("distribution", "distribution center access road"),
    ("dry_warehouse", "warehouse access road"),
    ("farm_elevator", "elevator access road"),
    ("food_terminal", "food terminal access road"),
    ("food_processor", "food plant access road"),
    ("grocery_retail_dc", "distribution center access road"),
    ("industrial_park", "industrial park access road"),
    ("intermodal", "intermodal yard access road"),
    ("intermodal_ramp", "intermodal ramp access road"),
    ("lumber_paper", "mill access road"),
    ("manufacturing", "plant access road"),
    ("manufacturing_plant", "plant access road"),
    ("metro_market", "local freight access road"),
    ("mine_quarry", "quarry access road"),
    ("parcel_hub", "parcel hub access road"),
    ("port", "port access road"),
    ("port_terminal", "port terminal access road"),
    ("rail", "rail yard access road"),
    ("retail_distribution", "retail distribution access road"),
    ("steel_industrial", "industrial plant access road"),
    ("terminal", "terminal access road"),
    ("warehouse", "warehouse access road"),
];

/// `(facility type, ships, receives)`.
pub const FACILITY_CARGO_ROLES: &[(&str, &[&str], &[&str])] = &[
    // Placarded and bulk-fuel freight (2026-09-16): a fuel farm, a fleet
    // island, a quarry's equipment tank and an elevator's farm-diesel plant
    // all take bulk fuel; assembly plants take placarded batteries, paints
    // and inflators; ag retailers ship placarded anhydrous ammonia and crop
    // chemicals; steel mills move industrial gases and acids both ways.
    // Before that, placarded loads only moved between chemical terminals
    // and manufacturing plants, and bulk fuel only terminal to terminal.
    (
        "air_cargo",
        &["electronics", "parcel", "general"],
        &["electronics", "parcel", "general", "fuel_bulk"],
    ),
    (
        "automotive_plant",
        &["automotive", "machinery"],
        &["steel", "machinery", "electronics", "general", "hazardous"],
    ),
    (
        "chemical_petroleum_terminal",
        &["chemicals", "bulk", "fuel_bulk", "hazardous"],
        &["chemicals", "bulk", "general", "fuel_bulk", "hazardous"],
    ),
    (
        "cold_storage",
        &["food", "refrigerated"],
        &["food", "refrigerated"],
    ),
    (
        "company_yard",
        &["general", "retail", "parcel"],
        &["general", "retail", "parcel", "fuel_bulk"],
    ),
    (
        "construction_materials_yard",
        &["construction", "bulk", "lumber_paper"],
        &["construction", "bulk", "steel", "lumber_paper", "fuel_bulk"],
    ),
    (
        "cross_dock",
        &[
            "general",
            "retail",
            "parcel",
            "container",
            "parcel_doubles",
            "turnpike_doubles",
        ],
        &[
            "general",
            "retail",
            "parcel",
            "container",
            "parcel_doubles",
            "turnpike_doubles",
        ],
    ),
    (
        "distribution",
        &["food", "general", "retail", "refrigerated", "parcel"],
        &["food", "general", "retail", "refrigerated", "parcel"],
    ),
    (
        "dry_warehouse",
        &["general", "retail", "bulk", "machinery", "construction"],
        &["general", "retail", "bulk", "machinery", "construction"],
    ),
    (
        "farm_elevator",
        &["grain", "bulk", "hazardous"],
        &["farm_inputs", "general", "fuel_bulk", "hazardous"],
    ),
    (
        "food_terminal",
        &["food", "refrigerated", "grain", "liquid_food"],
        &["food", "refrigerated", "grain", "liquid_food"],
    ),
    (
        "food_processor",
        &["food", "refrigerated", "liquid_food"],
        &[
            "grain",
            "food",
            "refrigerated",
            "farm_inputs",
            "liquid_food",
        ],
    ),
    (
        "grocery_retail_dc",
        &["retail", "food", "refrigerated", "general"],
        &["retail", "food", "refrigerated", "general"],
    ),
    (
        "industrial_park",
        &["bulk", "machinery", "retail", "construction"],
        &["bulk", "machinery", "retail", "construction"],
    ),
    (
        "intermodal",
        &["bulk", "container", "general", "automotive", "retail"],
        &["bulk", "container", "general", "automotive", "retail"],
    ),
    (
        "intermodal_ramp",
        &["container", "general", "retail", "automotive", "parcel"],
        &[
            "container",
            "general",
            "retail",
            "automotive",
            "parcel",
            "port_container",
        ],
    ),
    (
        "lumber_paper",
        &["lumber_paper", "construction"],
        &["bulk", "machinery", "chemicals"],
    ),
    (
        "manufacturing",
        &["bulk", "electronics", "machinery", "automotive"],
        &["bulk", "electronics", "machinery", "steel", "general"],
    ),
    (
        "manufacturing_plant",
        &["machinery", "electronics", "general", "hazardous"],
        &["bulk", "steel", "electronics", "general", "hazardous"],
    ),
    (
        "metro_market",
        &["general", "retail"],
        &["general", "retail"],
    ),
    (
        "mine_quarry",
        &["bulk", "construction"],
        &[
            "machinery",
            "chemicals",
            "farm_inputs",
            "hazardous",
            "fuel_bulk",
        ],
    ),
    (
        "parcel_hub",
        &[
            "parcel",
            "electronics",
            "general",
            "parcel_doubles",
            "turnpike_doubles",
        ],
        &[
            "parcel",
            "electronics",
            "general",
            "parcel_doubles",
            "turnpike_doubles",
        ],
    ),
    (
        "port",
        &[
            "bulk",
            "container",
            "electronics",
            "machinery",
            "automotive",
            "port_container",
        ],
        &[
            "bulk",
            "container",
            "electronics",
            "machinery",
            "automotive",
            "port_container",
        ],
    ),
    (
        "port_terminal",
        &[
            "container",
            "bulk",
            "automotive",
            "chemicals",
            "lumber_paper",
            "port_container",
            "fuel_bulk",
            "hazardous",
        ],
        &[
            "container",
            "bulk",
            "automotive",
            "chemicals",
            "lumber_paper",
            "port_container",
            "fuel_bulk",
            "hazardous",
        ],
    ),
    (
        "rail",
        &["bulk", "container", "machinery", "grain"],
        &["bulk", "container", "machinery", "grain", "port_container"],
    ),
    (
        "retail_distribution",
        &["general", "retail", "parcel"],
        &["general", "retail", "parcel"],
    ),
    (
        "steel_industrial",
        &["steel", "machinery", "bulk", "hazardous"],
        &["bulk", "chemicals", "construction", "hazardous"],
    ),
    (
        "terminal",
        &["electronics", "general", "retail", "parcel"],
        &["electronics", "general", "retail", "parcel"],
    ),
    (
        "warehouse",
        &["bulk", "general", "machinery", "retail", "construction"],
        &["bulk", "general", "machinery", "retail", "construction"],
    ),
];

/// `(ships, receives)` for a facility type, or `None` for an unknown type.
pub fn facility_cargo_roles(
    facility_type: &str,
) -> Option<(&'static [&'static str], &'static [&'static str])> {
    FACILITY_CARGO_ROLES
        .iter()
        .find(|(k, _, _)| *k == facility_type)
        .map(|(_, ships, receives)| (*ships, *receives))
}

pub const FACILITY_SOURCE_NOTES: &[(&str, &str)] = &[
    ("air_cargo", "Representative air-cargo facility; guided by FAF modal and commodity framing."),
    ("automotive_plant", "Representative automotive facility; guided by FAF commodity and metro-market framing."),
    ("chemical_petroleum_terminal", "Representative chemical or petroleum terminal; guided by FAF commodity framing."),
    ("cold_storage", "Representative cold-storage facility; guided by FAF food flows and USDA refrigerated transport context."),
    ("company_yard", "Representative company terminal or yard for the metro service area."),
    ("construction_materials_yard", "Representative construction materials yard; guided by FAF construction-sector freight framing."),
    ("cross_dock", "Representative cross-dock facility; guided by FAF metro logistics and border/gateway flows."),
    ("distribution", "Curated representative distribution facility in the metro freight market."),
    ("dry_warehouse", "Representative dry warehouse; guided by FAF metro-market freight flows."),
    ("farm_elevator", "Representative farm elevator or ag terminal; guided by USDA grain truck indicators and FAF agriculture flows."),
    ("food_terminal", "Curated representative food terminal in the metro freight market."),
    ("food_processor", "Representative food processor; guided by FAF food flows and USDA agricultural transport context."),
    ("grocery_retail_dc", "Representative grocery and retail DC; guided by FAF commodity and metro-market framing."),
    ("industrial_park", "Curated representative industrial facility in the metro freight market."),
    ("intermodal", "Curated representative intermodal facility in the metro freight market."),
    ("intermodal_ramp", "Representative rail/intermodal ramp; guided by FAF all-mode freight flow framing."),
    ("lumber_paper", "Representative lumber or paper facility; guided by FAF commodity framing."),
    ("manufacturing", "Curated representative manufacturing facility in the metro freight market."),
    ("manufacturing_plant", "Representative manufacturing plant; guided by FAF manufacturing-sector freight framing."),
    ("metro_market", "Legacy bare-city load fallback for save compatibility."),
    ("mine_quarry", "Representative mine or quarry; guided by FAF extraction-sector freight framing."),
    ("parcel_hub", "Representative parcel hub; guided by metro logistics and air/intermodal freight patterns."),
    ("port", "Curated representative port facility in the metro freight market."),
    ("port_terminal", "Representative port terminal; guided by MARAD and BTS port performance datasets."),
    ("rail", "Curated representative rail facility in the metro freight market."),
    ("retail_distribution", "Curated representative retail distribution facility in the metro freight market."),
    ("steel_industrial", "Representative steel or industrial facility; guided by FAF commodity framing."),
    ("terminal", "Curated representative freight terminal in the metro freight market."),
    ("warehouse", "Curated representative warehouse in the metro freight market."),
];

pub const FACILITY_LEVEL_UNLOCKS: &[(&str, i64)] = &[
    ("automotive_plant", 2),
    ("chemical_petroleum_terminal", 4),
    ("cold_storage", 2),
    ("food_processor", 2),
    ("lumber_paper", 2),
    ("manufacturing_plant", 2),
    ("mine_quarry", 3),
    ("steel_industrial", 3),
];

pub const BASE_MARKET_FACILITY_TYPES: &[&str] = &[
    "company_yard",
    "dry_warehouse",
    "cross_dock",
    "grocery_retail_dc",
];

pub const REGION_MARKET_TAGS: &[(&str, &[&str])] = &[
    ("northeast", &["port", "intermodal", "industrial", "retail"]),
    ("appalachia", &["industrial", "mining", "manufacturing"]),
    (
        "great_lakes",
        &["intermodal", "manufacturing", "automotive", "agriculture"],
    ),
    (
        "upper_midwest",
        &["agriculture", "food", "manufacturing", "intermodal"],
    ),
    (
        "corn_belt",
        &["agriculture", "food", "manufacturing", "intermodal"],
    ),
    ("heartland", &["agriculture", "intermodal", "food"]),
    (
        "southern_plains",
        &["energy", "agriculture", "intermodal", "retail"],
    ),
    ("mid_south", &["parcel", "manufacturing", "food"]),
    (
        "atlantic_southeast",
        &["port", "manufacturing", "retail", "food"],
    ),
    ("gulf_coast", &["port", "energy", "chemical", "food"]),
    ("florida", &["port", "food", "retail", "cold_chain"]),
    ("rockies", &["mining", "intermodal", "construction"]),
    ("great_basin", &["intermodal", "mining", "retail"]),
    (
        "desert_southwest",
        &["border", "construction", "food", "mining"],
    ),
    ("california", &["port", "food", "retail", "intermodal"]),
    (
        "pacific_northwest",
        &["port", "lumber", "agriculture", "intermodal"],
    ),
];

// Keyed by 2-letter state code, matching migrated city data.
pub const STATE_MARKET_TAGS: &[(&str, &[&str])] = &[
    ("AR", &["agriculture", "food"]),
    ("CA", &["port", "food", "cold_chain"]),
    ("CO", &["mining", "construction"]),
    ("FL", &["port", "food", "cold_chain"]),
    ("GA", &["port", "food", "parcel"]),
    ("ID", &["agriculture", "food"]),
    ("IL", &["intermodal", "agriculture"]),
    ("IN", &["manufacturing", "automotive"]),
    ("IA", &["agriculture", "food"]),
    ("KS", &["agriculture", "manufacturing"]),
    ("KY", &["parcel", "automotive"]),
    ("LA", &["port", "energy"]),
    ("MI", &["automotive", "manufacturing"]),
    ("MN", &["agriculture", "lumber"]),
    ("MO", &["agriculture", "intermodal"]),
    ("NE", &["agriculture", "food"]),
    ("NM", &["mining", "border"]),
    ("NY", &["port", "retail"]),
    ("NC", &["manufacturing", "food"]),
    ("OH", &["manufacturing", "automotive"]),
    ("OK", &["energy", "agriculture"]),
    ("OR", &["port", "lumber", "food"]),
    ("PA", &["industrial", "manufacturing"]),
    ("TN", &["parcel", "manufacturing"]),
    ("TX", &["energy", "border", "port", "retail"]),
    ("UT", &["mining", "intermodal"]),
    ("VA", &["port", "manufacturing"]),
    ("WA", &["port", "lumber", "food"]),
    ("WI", &["food", "manufacturing", "lumber"]),
    ("WY", &["mining", "energy"]),
];

// Keyed by the stable city slug (see data/legacy_aliases.py for old names).
//
// Elberton quarries and cuts better than a third of the monumental granite
// made in the United States; block and finished stone out, quarry machinery
// and abrasives in. The region tag alone would read it as a generic
// southeastern retail town.
pub const CITY_MARKET_TAGS: &[(&str, &[&str])] = &[
    ("atlanta_ga_us", &["air", "parcel", "food"]),
    ("baltimore_md_us", &["port", "intermodal"]),
    ("birmingham_al_us", &["steel", "manufacturing"]),
    ("buffalo_ny_us", &["border", "industrial"]),
    ("charlotte_nc_us", &["intermodal", "retail"]),
    (
        "chicago_il_us",
        &["intermodal", "air", "food", "parcel", "port"],
    ),
    ("cincinnati_oh_us", &["intermodal", "manufacturing"]),
    ("cleveland_oh_us", &["steel", "port"]),
    ("dallas_tx_us", &["intermodal", "parcel", "retail"]),
    ("denver_co_us", &["intermodal", "construction", "mining"]),
    ("detroit_mi_us", &["automotive", "border", "port"]),
    (
        "elberton_ga_us",
        &["mining", "construction", "manufacturing"],
    ),
    ("el_paso_tx_us", &["border", "cross_dock"]),
    ("fresno_ca_us", &["agriculture", "food", "cold_chain"]),
    ("green_bay_wi_us", &["port"]),
    ("houston_tx_us", &["port", "energy", "chemical"]),
    ("indianapolis_in_us", &["parcel", "intermodal"]),
    ("jacksonville_fl_us", &["port", "cold_chain"]),
    ("kansas_city_mo_us", &["intermodal", "agriculture"]),
    ("las_vegas_nv_us", &["retail", "construction"]),
    ("los_angeles_ca_us", &["port", "intermodal", "food", "air"]),
    ("louisville_ky_us", &["parcel", "air"]),
    (
        "memphis_tn_us",
        &["parcel", "air", "intermodal", "river_port"],
    ),
    ("miami_fl_us", &["port", "air", "cold_chain"]),
    ("milwaukee_wi_us", &["port", "food"]),
    ("minneapolis_mn_us", &["agriculture", "lumber"]),
    ("new_orleans_la_us", &["port", "energy", "agriculture"]),
    ("new_york_ny_us", &["port", "air", "retail"]),
    ("omaha_ne_us", &["agriculture", "food"]),
    ("philadelphia_pa_us", &["port", "industrial"]),
    ("phoenix_az_us", &["air", "retail", "construction"]),
    ("pittsburgh_pa_us", &["steel", "industrial"]),
    ("portland_or_us", &["port", "lumber"]),
    ("reno_nv_us", &["intermodal", "retail"]),
    ("richmond_va_us", &["port", "manufacturing"]),
    ("sacramento_ca_us", &["food", "agriculture"]),
    ("salt_lake_city_ut_us", &["intermodal", "mining"]),
    ("san_antonio_tx_us", &["border", "retail"]),
    ("san_diego_ca_us", &["port", "border"]),
    ("savannah_ga_us", &["port", "intermodal"]),
    ("seattle_wa_us", &["port", "air", "lumber"]),
    ("spokane_wa_us", &["agriculture", "lumber"]),
    (
        "st_louis_mo_us",
        &["river_port", "agriculture", "intermodal"],
    ),
    ("tampa_fl_us", &["port", "cold_chain"]),
    ("toledo_oh_us", &["port"]),
    ("tulsa_ok_us", &["energy", "manufacturing"]),
    ("wichita_ks_us", &["manufacturing", "air"]),
];

pub const MARKET_TAG_FACILITY_TYPES: &[(&str, &[&str])] = &[
    ("agriculture", &["farm_elevator", "food_processor"]),
    ("air", &["air_cargo"]),
    ("automotive", &["automotive_plant"]),
    ("border", &["cross_dock", "dry_warehouse"]),
    ("chemical", &["chemical_petroleum_terminal"]),
    ("cold_chain", &["cold_storage"]),
    ("construction", &["construction_materials_yard"]),
    ("cross_dock", &["cross_dock"]),
    ("energy", &["chemical_petroleum_terminal"]),
    ("food", &["food_processor", "cold_storage"]),
    ("industrial", &["steel_industrial", "manufacturing_plant"]),
    ("intermodal", &["intermodal_ramp"]),
    ("lumber", &["lumber_paper"]),
    ("manufacturing", &["manufacturing_plant"]),
    ("mining", &["mine_quarry"]),
    ("parcel", &["parcel_hub"]),
    ("port", &["port_terminal"]),
    ("retail", &["grocery_retail_dc"]),
    ("river_port", &["port_terminal", "farm_elevator"]),
    ("steel", &["steel_industrial"]),
];

mod stand_in_markets;
pub use stand_in_markets::STAND_IN_MARKET_CITY_KEYS;

mod template_gates;
pub use template_gates::*;

/// `{city}` in a template is replaced by the spoken city name.
pub const FACILITY_NAME_TEMPLATES: &[(&str, &str)] = &[
    ("air_cargo", "{city} Air Cargo Center"),
    ("automotive_plant", "{city} Auto Assembly Supplier Park"),
    ("chemical_petroleum_terminal", "{city} Energy Terminal"),
    ("cold_storage", "{city} Cold Storage"),
    ("company_yard", "{city} Company Yard"),
    ("construction_materials_yard", "{city} Materials Yard"),
    ("cross_dock", "{city} Cross-Dock"),
    ("dry_warehouse", "{city} Dry Warehouse"),
    ("farm_elevator", "{city} Grain Elevator"),
    ("food_processor", "{city} Food Processing Plant"),
    ("grocery_retail_dc", "{city} Grocery Distribution Center"),
    ("intermodal_ramp", "{city} Intermodal Ramp"),
    ("lumber_paper", "{city} Lumber and Paper Yard"),
    ("manufacturing_plant", "{city} Manufacturing Plant"),
    ("mine_quarry", "{city} Quarry"),
    ("parcel_hub", "{city} Parcel Hub"),
    ("port_terminal", "{city} Port Terminal"),
    ("steel_industrial", "{city} Steel and Industrial Works"),
];

pub const RAW_FACILITY_TEXT_MARKERS: &[&str] = &[
    "osm_id",
    "openstreetmap id",
    "amenity=",
    "highway=",
    "operator=",
    "node/",
    "way/",
    "relation/",
    "place_id",
    "wikidata=",
    "naics=",
];
