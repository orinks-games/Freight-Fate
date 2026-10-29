//! Canonical region taxonomy and coordinate-based classification (port of
//! `freight_fate/data/regions.py`).
//!
//! A city's `region` is *derived* from its state and coordinates at build
//! time and baked into the world data. [`classify_region`] is the single
//! source of truth: the stored-equals-derived test asserts every city's stored
//! region equals `classify_region(state, lat, lon)`, so a misclassification
//! (such as Reno being tagged the Rockies) cannot recur as the map grows.
//!
//! The taxonomy blends NOAA climate regions (which drive weather flavor) with
//! USGS physiographic provinces (which drive grades and hazards), refined for
//! freight character. See `docs/osm-routing-plan.md` for the full design and
//! the rationale behind each region.
//!
//! The classifier is deliberately a state map plus a few coordinate split
//! rules for states that span more than one region, rather than a heavyweight
//! GIS polygon set: it is deterministic, dependency-free, has no runtime cost,
//! and is exactly testable against the known cities. Single-region assignments
//! for large states and the split thresholds are approximations refined as the
//! map grows; the per-city test is the guardrail.

use super::world_models::DataError;

/// Canonical region keys. Every runtime region table -- weather weights
/// (sim/weather), hazards (sim/trip), fuel price (models/economy), market tags
/// (data/world_constants) -- plus the spoken labels below must cover each of
/// these keys. The regions tests enforce full coverage.
pub const REGIONS: &[&str] = &[
    "northeast",
    "appalachia",
    "great_lakes",
    "upper_midwest",
    "corn_belt",
    "heartland",
    "southern_plains",
    "mid_south",
    "atlantic_southeast",
    "gulf_coast",
    "florida",
    "rockies",
    "great_basin",
    "desert_southwest",
    "california",
    "pacific_northwest",
];

/// Spoken/displayed names, read aloud in the home-terminal picker, so they
/// want natural phrasing ("at ... in the Great Basin", "... in California").
pub const REGION_LABELS: &[(&str, &str)] = &[
    ("northeast", "the Northeast"),
    ("appalachia", "Appalachia"),
    ("great_lakes", "the Great Lakes"),
    ("upper_midwest", "the Upper Midwest"),
    ("corn_belt", "the Corn Belt"),
    ("heartland", "the Heartland"),
    ("southern_plains", "the Southern Plains"),
    ("mid_south", "the Mid-South"),
    ("atlantic_southeast", "the Atlantic Southeast"),
    ("gulf_coast", "the Gulf Coast"),
    ("florida", "Florida"),
    ("rockies", "the Rockies"),
    ("great_basin", "the Great Basin"),
    ("desert_southwest", "the Desert Southwest"),
    ("california", "California"),
    ("pacific_northwest", "the Pacific Northwest"),
];

/// The spoken label for a region key, if it is canonical.
pub fn region_label(region: &str) -> Option<&'static str> {
    REGION_LABELS
        .iter()
        .find(|(key, _)| *key == region)
        .map(|(_, label)| *label)
}

/// States that fall entirely within one region, keyed by 2-letter code (the
/// form city data stores since the slug-key migration). States that span more
/// than one region (Texas, Nevada, Tennessee, Pennsylvania, New York) are
/// handled by coordinate split rules in [`classify_region`] and are
/// intentionally absent here. The lower 48 are covered so future cities
/// classify without code changes.
pub const STATE_REGION: &[(&str, &str)] = &[
    // Northeast
    ("ME", "northeast"),
    ("NH", "northeast"),
    ("VT", "northeast"),
    ("MA", "northeast"),
    ("RI", "northeast"),
    ("CT", "northeast"),
    ("NJ", "northeast"),
    ("DE", "northeast"),
    // Maryland is split by longitude in classify_region (western MD -> appalachia).
    ("DC", "northeast"),
    // Appalachia
    ("WV", "appalachia"),
    // Great Lakes / industrial Midwest. Ohio and Illinois are split by latitude
    // in classify_region (their Lake shore stays great_lakes, interior ->
    // corn_belt). Indiana is split by latitude too (Evansville -> mid_south,
    // Indianapolis -> corn_belt, the north -> great_lakes).
    // Michigan is split in classify_region: the Upper Peninsula -> upper_midwest.
    // Minnesota and Wisconsin are the colder Upper Midwest.
    ("WI", "upper_midwest"),
    ("MN", "upper_midwest"),
    // Heartland (corn belt + Missouri/Mississippi valley + northern plains)
    ("MO", "heartland"),
    ("IA", "heartland"),
    ("NE", "heartland"),
    ("ND", "heartland"),
    ("SD", "heartland"),
    // Southern Plains
    ("KS", "southern_plains"),
    ("OK", "southern_plains"),
    // Mid-South (interior Dixie / Cumberland / Ozark fringe).
    // Alabama and Mississippi are split by latitude in classify_region (their
    // Gulf coastal strip -> gulf_coast).
    ("KY", "mid_south"),
    ("AR", "mid_south"),
    // Atlantic Southeast (Piedmont + southern Atlantic coastal plain).
    // Virginia and North Carolina are split by longitude in classify_region
    // (their western Blue Ridge / Great Valley -> appalachia).
    ("SC", "atlantic_southeast"),
    ("GA", "atlantic_southeast"),
    // Gulf Coast
    ("LA", "gulf_coast"),
    // Florida
    ("FL", "florida"),
    // Rockies
    ("CO", "rockies"),
    ("WY", "rockies"),
    ("MT", "rockies"),
    ("UT", "rockies"),
    // Great Basin (Snake River Plain + Basin and Range)
    ("ID", "great_basin"),
    // Desert Southwest
    ("AZ", "desert_southwest"),
    ("NM", "desert_southwest"),
    // California
    ("CA", "california"),
    // Pacific Northwest
    ("OR", "pacific_northwest"),
    ("WA", "pacific_northwest"),
];

/// Return the canonical region for a city by state code and coordinates.
///
/// `state` is the 2-letter code stored in city data ("TX"). Multi-region
/// states are split first by a coordinate threshold; every other state maps
/// directly. Errors for an unmapped state so map expansion cannot silently
/// mis-derive a region.
pub fn classify_region(state: &str, lat: f64, lon: f64) -> Result<&'static str, DataError> {
    match state {
        "TX" => {
            if lon <= -104.0 {
                return Ok("desert_southwest"); // El Paso and far-west Texas
            }
            if lat <= 31.0 {
                return Ok("gulf_coast"); // Houston, San Antonio, south Texas
            }
            Ok("southern_plains") // Dallas, Amarillo, north Texas
        }
        // Reno and northern Nevada are Great Basin; Las Vegas is Mojave desert.
        "NV" => Ok(if lat >= 38.0 {
            "great_basin"
        } else {
            "desert_southwest"
        }),
        // The Gulf coastal strip (Mobile, Gulfport) is Gulf Coast; the
        // interior of both states is Mid-South.
        "AL" | "MS" => Ok(if lat <= 31.0 {
            "gulf_coast"
        } else {
            "mid_south"
        }),
        // East Tennessee is Appalachian; middle and west are Mid-South.
        "TN" => Ok(if lon >= -85.0 {
            "appalachia"
        } else {
            "mid_south"
        }),
        "PA" => {
            // Erie sits on the Lake Erie shore -- lake-effect Great Lakes country,
            // like Buffalo and Cleveland on either side -- not Appalachian.
            if lat >= 42.0 {
                return Ok("great_lakes");
            }
            // Western Pennsylvania is Appalachian; the southeast is the Northeast.
            Ok(if lon <= -78.0 {
                "appalachia"
            } else {
                "northeast"
            })
        }
        "IN" => {
            // Far-southern Indiana on the Ohio River (Evansville) is Mid-South;
            // central Indiana (Indianapolis) is the Corn Belt; the north (Fort
            // Wayne, South Bend) is the industrial Great Lakes Midwest.
            if lat <= 38.5 {
                return Ok("mid_south");
            }
            Ok(if lat <= 40.5 {
                "corn_belt"
            } else {
                "great_lakes"
            })
        }
        // Chicagoland and northern Illinois are Great Lakes; central and
        // southern Illinois (Peoria, Springfield, the ADM corn country) are the
        // Corn Belt.
        "IL" => Ok(if lat >= 41.5 {
            "great_lakes"
        } else {
            "corn_belt"
        }),
        // The Lake Erie shore (Cleveland, Toledo, Akron) is Great Lakes; central
        // and southern Ohio (Columbus, Dayton, Cincinnati) are the Corn Belt.
        "OH" => Ok(if lat >= 40.5 {
            "great_lakes"
        } else {
            "corn_belt"
        }),
        "MI" => {
            // The Upper Peninsula -- Lake Superior northwoods, iron/timber country
            // bordering Wisconsin -- is Upper Midwest; the Lower Peninsula (Detroit,
            // Grand Rapids, the auto belt) is Great Lakes. The Straits of Mackinac
            // split them: north of ~45.8 lat, or the western UP that dips south of
            // that along Lake Michigan (lon <= -87).
            if lat >= 45.8 || lon <= -87.0 {
                return Ok("upper_midwest");
            }
            Ok("great_lakes")
        }
        // Western New York (Buffalo) is lake-effect Great Lakes country.
        "NY" => Ok(if lon <= -78.0 {
            "great_lakes"
        } else {
            "northeast"
        }),
        // The western Blue Ridge (Asheville) is Appalachian; the Piedmont and
        // coastal plain are the Atlantic Southeast.
        "NC" => Ok(if lon <= -82.0 {
            "appalachia"
        } else {
            "atlantic_southeast"
        }),
        // West of the Blue Ridge -- the I-81 Great Valley (Roanoke, Harrisonburg,
        // Winchester) -- is Appalachian; the Piedmont and Tidewater are not.
        "VA" => Ok(if lon <= -78.0 {
            "appalachia"
        } else {
            "atlantic_southeast"
        }),
        // Western Maryland (Hagerstown, the Cumberland valley) is Appalachian;
        // the rest is the Northeast corridor.
        "MD" => Ok(if lon <= -77.5 {
            "appalachia"
        } else {
            "northeast"
        }),
        _ => STATE_REGION
            .iter()
            .find(|(code, _)| *code == state)
            .map(|(_, region)| *region)
            .ok_or_else(|| {
                DataError::Value(format!(
                    "No region mapping for state {}; add it to STATE_REGION or \
                     a coordinate split rule in classify_region (see \
                     docs/osm-routing-plan.md).",
                    crate::data::world_parsing::py_repr_str(state)
                ))
            }),
    }
}

#[cfg(test)]
mod tests {
    //! Pure parts of `tests/test_regions.py`; the world-backed and cross-module
    //! coverage checks live in `crates/ff-core/tests/data_regions.rs`.
    use super::*;
    use crate::data::world_constants::{lookup, MARKET_TAG_FACILITY_TYPES, REGION_MARKET_TAGS};

    #[test]
    fn test_every_region_covered_in_flavor_tables() {
        // REGION_WEIGHTS and REGION_FUEL_PRICE live in sim/weather and
        // models/economy; the two tables this crate owns are checked here.
        let missing: Vec<_> = REGIONS
            .iter()
            .filter(|r| lookup(REGION_MARKET_TAGS, r).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "REGION_MARKET_TAGS is missing regions: {missing:?}"
        );
        let missing: Vec<_> = REGIONS
            .iter()
            .filter(|r| region_label(r).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "REGION_LABELS is missing regions: {missing:?}"
        );
    }

    #[test]
    fn test_market_tags_are_valid() {
        for (region, tags) in REGION_MARKET_TAGS {
            for tag in *tags {
                assert!(
                    lookup(MARKET_TAG_FACILITY_TYPES, tag).is_some(),
                    "{region} market tag {tag:?} has no facility-type mapping"
                );
            }
        }
    }

    #[test]
    fn test_classifier_splits_multi_region_states() {
        // Texas spans three regions by coordinate.
        assert_eq!(classify_region("TX", 29.76, -95.37).unwrap(), "gulf_coast"); // Houston
        assert_eq!(
            classify_region("TX", 32.78, -96.80).unwrap(),
            "southern_plains"
        ); // Dallas
        assert_eq!(
            classify_region("TX", 31.76, -106.48).unwrap(),
            "desert_southwest"
        ); // El Paso
           // Nevada: northern Great Basin vs southern Mojave desert.
        assert_eq!(
            classify_region("NV", 39.53, -119.81).unwrap(),
            "great_basin"
        ); // Reno
        assert_eq!(
            classify_region("NV", 36.17, -115.14).unwrap(),
            "desert_southwest"
        ); // Las Vegas
           // Pennsylvania, New York, Tennessee splits.
        assert_eq!(classify_region("PA", 40.44, -80.00).unwrap(), "appalachia"); // Pittsburgh
        assert_eq!(classify_region("PA", 39.95, -75.17).unwrap(), "northeast"); // Philadelphia
        assert_eq!(classify_region("NY", 42.89, -78.88).unwrap(), "great_lakes"); // Buffalo
        assert_eq!(classify_region("NY", 40.71, -74.01).unwrap(), "northeast"); // New York
        assert_eq!(classify_region("TN", 35.96, -83.92).unwrap(), "appalachia"); // Knoxville
        assert_eq!(classify_region("TN", 36.16, -86.78).unwrap(), "mid_south"); // Nashville
    }

    #[test]
    fn test_classifier_rejects_unmapped_state() {
        assert!(classify_region("Atlantis", 0.0, 0.0).is_err());
    }

    #[test]
    fn test_single_region_states_are_canonical() {
        for (state, region) in STATE_REGION {
            assert!(
                REGIONS.contains(region),
                "{state} maps to non-canonical region {region:?}"
            );
        }
    }
}
