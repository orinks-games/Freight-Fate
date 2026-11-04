//! Brand-derived truck-stop amenities (port of
//! `freight_fate/data/amenities.py`).
//!
//! Turns a truck stop's *name* into the real brand behind it, the service
//! tier, and the signature service that brand is known for -- tire care at
//! Love's, the repair shop at a TravelCenters of America, showers and fuel
//! deals at Pilot/Flying J. It is derived at runtime from the stop name, so it
//! needs no change to the world data: any new stop the map's truck-stop sweep
//! discovers is classified the moment it appears (a fresh Love's is a tire
//! stop with no extra tagging), and the amenities layer can never drift out of
//! sync with the map.
//!
//! The point, beyond flavor, is a planning decision: "I need tires, find me a
//! Love's" / "the rig needs the shop, find a TA". Brand identity teaches real
//! trucking knowledge just by playing, and later feeds a service-stop buff
//! system (fatigue, tire, wear, morale axes -- never the legal duty clock).
//!
//! Everything here is player-facing speech: full brand names, no codes, no
//! raw map tags, no bare initialisms a screen reader would spell out.
//!
//! Sources: public brand service listings -- Love's tire care / Speedco quick
//! lube, TravelCenters of America truck service and Petro Iron Skillet, the
//! Pilot Flying J shower network. Real brand names are used nominatively,
//! matching the stop-name conventions already in the world data. "Big Buck's"
//! is an original parody of the well-known Texas travel-center chain that
//! famously bans big rigs; the parody keeps the joke and drops the trademark.

pub const AMENITIES_SOURCE: &str =
    "Brand service identity derived at runtime from the stop name; grounded in \
     public brand service listings (Love's/Speedco tire care, TravelCenters of \
     America truck service, Pilot Flying J shower network).";

/// Spoken label for each signature service key. Kept here (not shared with
/// the generic POI service labels) because these are brand differentiators
/// phrased for a driver, e.g. a Love's "specialty," not a bare checklist entry.
pub const SIGNATURE_SERVICE_LABELS: &[(&str, &str)] = &[
    ("tires", "tire care and quick lube"),
    ("showers", "showers"),
    ("repair", "a truck repair shop"),
    ("restaurant", "a sit-down restaurant"),
    ("barbecue", "smoked barbecue and brisket"),
    ("souvenirs", "souvenirs and road snacks"),
    ("coffee", "five-cent coffee"),
    ("ice_water", "free ice water"),
    ("cat_scale", "a CAT Scale"),
    ("laundry", "public laundry facilities"),
    ("game_room", "a game room"),
    ("barber", "a barber shop"),
    ("premium_wifi", "premium wifi"),
    ("check_cashing", "check cashing services"),
    ("def", "diesel exhaust fluid lanes"),
    ("atm", "ATM services"),
];

/// The spoken label for a signature service key.
pub fn signature_service_label(key: &str) -> Option<&'static str> {
    SIGNATURE_SERVICE_LABELS
        .iter()
        .find(|(k, _)| *k == key)
        .map(|(_, label)| *label)
}

/// A recognized truck-stop brand and what it is known for.
///
/// `tier` is a coarse class the future buff system reads: `travel_center`
/// for a full-service major chain, `landmark` for a destination stop like
/// Big Buck's. `signature` lists the service keys this brand is the place to
/// go for. `bans_big_rigs` marks a stop a Class-8 truck cannot pull into with
/// a trailer (the Big Buck's gag) -- reachable only bobtail.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Brand {
    pub key: &'static str,
    pub spoken: &'static str,
    pub tier: &'static str,
    pub signature: &'static [&'static str],
    pub keywords: &'static [&'static str],
    pub bans_big_rigs: bool,
}

/// Ordered most-specific keyword first so a combined name (e.g. "TA Petro")
/// resolves deterministically. Keywords are matched as lowercased substrings
/// of the stop name; none is a bare initialism that could collide with a place
/// name (mirrors the truck-POI keyword discipline in
/// tools/enrich_routes_pois.py).
pub const BRANDS: &[Brand] = &[
    Brand {
        key: "speedco",
        spoken: "Speedco",
        tier: "travel_center",
        signature: &["tires"],
        keywords: &["speedco"],
        bans_big_rigs: false,
    },
    Brand {
        key: "loves",
        spoken: "Love's",
        tier: "travel_center",
        signature: &["tires"],
        keywords: &["love's", "loves travel"],
        bans_big_rigs: false,
    },
    Brand {
        key: "petro",
        spoken: "Petro",
        tier: "travel_center",
        signature: &["repair", "restaurant"],
        keywords: &["petro stopping", "petro travel"],
        bans_big_rigs: false,
    },
    Brand {
        key: "ta",
        spoken: "TravelCenters of America",
        tier: "travel_center",
        signature: &["repair"],
        keywords: &["travelcenters", "ta travel", "ta petro"],
        bans_big_rigs: false,
    },
    Brand {
        key: "flying_j",
        spoken: "Flying J",
        tier: "travel_center",
        // No CAT Scale here or at Pilot: a stop's own listed services say
        // whether it has one, so a brand-wide claim could name a scale the
        // stop's menu does not offer.
        signature: &["showers", "laundry", "premium_wifi", "game_room"],
        keywords: &["flying j"],
        bans_big_rigs: false,
    },
    Brand {
        key: "pilot",
        spoken: "Pilot",
        tier: "travel_center",
        signature: &["showers", "laundry", "premium_wifi"],
        keywords: &["pilot"],
        bans_big_rigs: false,
    },
    Brand {
        key: "big_bucks",
        spoken: "Big Buck's",
        tier: "landmark",
        signature: &["barbecue", "souvenirs"],
        keywords: &["big buck", "buc-ee", "bucee", "buckee"],
        bans_big_rigs: true,
    },
    Brand {
        key: "wall_drug",
        spoken: "Wall Drug",
        tier: "landmark",
        signature: &["coffee", "ice_water"],
        keywords: &["wall drug"],
        bans_big_rigs: false,
    },
];

/// Join spoken fragments with an Oxford `and` (mirrors the driving HUD).
fn join(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [only] => only.clone(),
        _ => format!(
            "{}, and {}",
            parts[..parts.len() - 1].join(", "),
            parts[parts.len() - 1]
        ),
    }
}

/// Recognize the truck-stop brand from a stop name, or `None` if generic.
///
/// Independent stops and unbranded rest areas return `None` -- they keep the
/// plain listed services from the world data with no brand embellishment.
pub fn classify_brand(name: &str) -> Option<&'static Brand> {
    // Python: `" ".join(str(name).lower().split())`.
    let low = name
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    BRANDS
        .iter()
        .find(|brand| brand.keywords.iter().any(|keyword| low.contains(keyword)))
}

/// The service keys a stop's brand is the place to go for (empty if generic).
///
/// The service-planning and future buff systems read this to answer "which
/// stop for tires / the shop / a shower" without re-parsing the name.
pub fn signature_services(name: &str) -> &'static [&'static str] {
    classify_brand(name).map(|b| b.signature).unwrap_or(&[])
}

/// A spoken clause about the brand's specialty, or `""` for a generic stop.
///
/// Appended to the stop's listed services in the driving route info, so a
/// Love's reads as a tire stop and a landmark like Big Buck's announces that
/// big rigs cannot pull in. `stop_type` is accepted for future tier-aware
/// phrasing.
pub fn spoken_amenities(name: &str, _stop_type: &str) -> String {
    let Some(brand) = classify_brand(name) else {
        return String::new();
    };
    let labels: Vec<String> = brand
        .signature
        .iter()
        .map(|key| {
            signature_service_label(key)
                .map(str::to_string)
                .unwrap_or_else(|| key.replace('_', " "))
        })
        .collect();
    let phrase = join(&labels);
    if brand.tier == "landmark" {
        let mut clause = format!(
            "{} is a roadside landmark known for {}",
            brand.spoken, phrase
        );
        if brand.bans_big_rigs {
            clause.push_str("; no big rigs allowed, so you can only stop here without a trailer");
        }
        return clause;
    }
    if phrase.is_empty() {
        return String::new();
    }
    format!("{} specialty: {}", brand.spoken, phrase)
}

#[cfg(test)]
mod tests {
    //! Port of `tests/test_amenities.py` (the `_poi_offers_text` integration
    //! case belongs to the driving layer).
    use super::*;

    // Real-world stop names as the map's truck-stop sweep would store them, paired
    // with the brand key and the signature service the player learns to seek there.
    const BRANDED_STOPS: &[(&str, &str, &str)] = &[
        ("Love's Travel Stop #472", "loves", "tires"),
        ("Speedco Lube and Tire", "speedco", "tires"),
        ("Pilot Travel Center", "pilot", "showers"),
        ("Flying J Travel Center", "flying_j", "showers"),
        ("TA Travel Center", "ta", "repair"),
        ("TravelCenters of America", "ta", "repair"),
        ("Petro Stopping Center", "petro", "repair"),
    ];

    #[test]
    fn test_classify_known_brands() {
        for (name, key, _) in BRANDED_STOPS {
            let brand = classify_brand(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(brand.key, *key, "{name}");
        }
    }

    #[test]
    fn test_signature_service_is_the_differentiator() {
        for (name, _, service) in BRANDED_STOPS {
            assert!(signature_services(name).contains(service), "{name}");
        }
    }

    #[test]
    fn test_generic_stop_has_no_brand() {
        assert!(classify_brand("Interstate 40 corridor rest area").is_none());
        assert!(classify_brand("Downtown Fuel Mart").is_none());
        assert!(signature_services("Municipal Truck Parking").is_empty());
        assert_eq!(spoken_amenities("Municipal Truck Parking", ""), "");
    }

    #[test]
    fn test_spoken_amenities_reads_cleanly() {
        let text = spoken_amenities("Love's Travel Stop #472", "");
        assert!(text.contains("Love's"));
        assert!(text.contains("tire care and quick lube"));
        // Player-facing speech: no raw map tags, codes, or stray markers.
        for marker in ["amenity=", "osm", "node/", "_", "#"] {
            assert!(!text.contains(marker));
        }
    }

    #[test]
    fn test_big_bucks_landmark_announces_the_big_rig_ban() {
        let text = spoken_amenities("Big Buck's Travel Center", "");
        let brand = classify_brand("Big Buck's Travel Center").unwrap();
        assert_eq!(brand.tier, "landmark");
        assert!(brand.bans_big_rigs);
        assert!(text.contains("landmark"));
        assert!(text.contains("big rigs") && text.contains("bobtail"));
    }

    #[test]
    fn test_real_bucees_name_classifies_as_the_landmark() {
        // The sweep excludes Buc-ee's from truck stops, but if a landmark stop is
        // ever placed, the parody brand must own the no-big-rigs gag.
        for name in ["Buc-ee's", "Bucees", "Buckee's Beaver Stop"] {
            let brand = classify_brand(name).unwrap_or_else(|| panic!("{name}"));
            assert_eq!(brand.key, "big_bucks", "{name}");
        }
    }

    #[test]
    fn test_every_signature_key_has_a_spoken_label() {
        for brand in BRANDS {
            for key in brand.signature {
                assert!(signature_service_label(key).is_some(), "{key}");
            }
        }
    }

    #[test]
    fn test_new_amenities_have_spoken_labels() {
        // New realistic amenities have proper spoken labels.
        for amenity in [
            "cat_scale",
            "laundry",
            "game_room",
            "barber",
            "premium_wifi",
            "check_cashing",
            "def",
            "atm",
        ] {
            let label = signature_service_label(amenity)
                .unwrap_or_else(|| panic!("Missing label for {amenity}"));
            // Cat is a brand name and should be preserved as-is
            if amenity == "cat_scale" {
                assert!(label.contains("Cat") || label.contains("CAT"));
            }
            // Wi-Fi should be readable (WiFi, Wi-Fi, or wifi are all acceptable)
            if amenity == "premium_wifi" {
                assert!(
                    label.contains("Wi-Fi") || label.contains("WiFi") || label.contains("wifi")
                );
            }
        }
    }

    #[test]
    fn test_pilot_has_enhanced_amenities() {
        let pilot = classify_brand("Pilot Travel Center").unwrap();
        for key in ["showers", "laundry", "premium_wifi"] {
            assert!(pilot.signature.contains(&key));
        }
        assert!(!pilot.signature.contains(&"cat_scale"));
    }

    #[test]
    fn test_flying_j_has_enhanced_amenities() {
        let flying_j = classify_brand("Flying J Travel Center").unwrap();
        for key in ["showers", "laundry", "premium_wifi", "game_room"] {
            assert!(flying_j.signature.contains(&key));
        }
    }

    #[test]
    fn test_wall_drug_is_a_landmark_without_a_big_rig_ban() {
        let brand = classify_brand("Wall Drug").unwrap();
        assert_eq!(brand.key, "wall_drug");
        assert_eq!(brand.tier, "landmark");
        assert!(!brand.bans_big_rigs);
        assert!(brand.signature.contains(&"coffee"));
        assert!(brand.signature.contains(&"ice_water"));
        let text = spoken_amenities("Wall Drug", "");
        assert!(text.contains("Wall Drug"));
        assert!(text.contains("landmark"));
        assert!(text.contains("five-cent coffee"));
        assert!(text.contains("free ice water"));
        assert!(!text.contains("big rigs"));
    }
}
