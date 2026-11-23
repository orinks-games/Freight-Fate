//! Legs the map no longer has, and the chain of legs that replaced each.
//!
//! A saved trip stores its route as a city sequence. When a corridor build
//! splits a long leg at new towns (Career 2.0's western corridor put Hope,
//! Cache Creek, Williams Lake and Quesnel on the old Surrey to Prince George
//! leg), a trip saved on the
//! old leg names two cities that are no longer neighbours. The resume path
//! swaps such a hop for its replacement chain, so the truck resumes at the
//! same mile on the same road instead of the trip being thrown away.
//!
//! Only a hop with no direct leg is expanded; nothing here changes a route
//! the map can still build.

/// `(from, to, replacement chain from first to last, both ends included)`.
/// Each entry is also read backwards for the reverse direction.
pub const RETIRED_LEGS: &[(&str, &str, &[&str])] = &[
    (
        "surrey_bc_ca",
        "prince_george_bc_ca",
        &[
            "surrey_bc_ca",
            "abbotsford_bc_ca",
            "chilliwack_bc_ca",
            "hope_bc_ca",
            "cache_creek_bc_ca",
            "williams_lake_bc_ca",
            "quesnel_bc_ca",
            "prince_george_bc_ca",
        ],
    ),
    // The Florida and Georgia missing roads (2026-11-22) split West Palm
    // Beach to Palm Bay at Port Saint Lucie, which it passed 1.2 miles off.
    // The chain is 51 plus 65 miles against the old leg's 110.
    (
        "west_palm_beach_fl_us",
        "palm_bay_fl_us",
        &[
            "west_palm_beach_fl_us",
            "port_saint_lucie_fl_us",
            "palm_bay_fl_us",
        ],
    ),
    // The Alabama, Mississippi and South Carolina missing roads
    // (2026-11-23) split Jackson to Baton Rouge (I-55 then I-12, 175 mi) at
    // Hammond, where the two roads meet. The chain is 133 plus 50 miles.
    (
        "jackson_ms_us",
        "baton_rouge_la_us",
        &["jackson_ms_us", "hammond_la_us", "baton_rouge_la_us"],
    ),
    // I-5, 121 mi: missing roads pass, 2026-11-22.
    (
        "los_angeles_ca_us",
        "san_diego_ca_us",
        &[
            "los_angeles_ca_us",
            "santa_ana_ca_us",
            "oceanside_ca_us",
            "san_diego_ca_us",
        ],
    ),
    // CA-12, 106 mi: missing roads pass, 2026-11-22.
    (
        "santa_rosa_ca_us",
        "stockton_ca_us",
        &["santa_rosa_ca_us", "fairfield_ca_us", "stockton_ca_us"],
    ),
    // US-97, 141 mi: missing roads pass, 2026-11-22.
    (
        "redding_ca_us",
        "klamath_falls_or_us",
        &["redding_ca_us", "mount_shasta_ca_us", "klamath_falls_or_us"],
    ),
    // I-5, 94 mi: missing roads pass, 2026-11-22.
    (
        "oxnard_ca_us",
        "santa_ana_ca_us",
        &["oxnard_ca_us", "los_angeles_ca_us", "santa_ana_ca_us"],
    ),
    // CA-99, 220 mi: missing roads pass, 2026-11-22.
    (
        "los_angeles_ca_us",
        "fresno_ca_us",
        &["los_angeles_ca_us", "valencia_ca_us", "fresno_ca_us"],
    ),
    // US-89A, 98 mi: missing roads pass, 2026-11-22.
    (
        "prescott_az_us",
        "flagstaff_az_us",
        &["prescott_az_us", "camp_verde_az_us", "flagstaff_az_us"],
    ),
    // I-680, 117 mi: missing roads pass, 2026-11-22.
    (
        "san_jose_ca_us",
        "sacramento_ca_us",
        &["san_jose_ca_us", "fairfield_ca_us", "sacramento_ca_us"],
    ),
    // I-80, 88 mi: missing roads pass, 2026-11-22.
    (
        "sacramento_ca_us",
        "san_francisco_ca_us",
        &["sacramento_ca_us", "fairfield_ca_us", "san_francisco_ca_us"],
    ),
    // I-5, 84 mi: missing roads pass, 2026-11-22.
    (
        "oceanside_ca_us",
        "los_angeles_ca_us",
        &["oceanside_ca_us", "santa_ana_ca_us", "los_angeles_ca_us"],
    ),
    // I-80, 95 mi: missing roads pass, 2026-11-22.
    (
        "santa_rosa_ca_us",
        "sacramento_ca_us",
        &["santa_rosa_ca_us", "fairfield_ca_us", "sacramento_ca_us"],
    ),
    // CA-14, 103 mi: missing roads pass, 2026-11-22.
    (
        "oxnard_ca_us",
        "lancaster_ca_us",
        &["oxnard_ca_us", "valencia_ca_us", "lancaster_ca_us"],
    ),
    // CA-58, 89 mi: missing roads pass, 2026-11-22.
    (
        "lancaster_ca_us",
        "bakersfield_ca_us",
        &["lancaster_ca_us", "mojave_ca_us", "bakersfield_ca_us"],
    ),
    // CA-58, 95 mi: missing roads pass, 2026-11-22.
    (
        "lancaster_ca_us",
        "barstow_ca_us",
        &["lancaster_ca_us", "mojave_ca_us", "barstow_ca_us"],
    ),
    // US-395, 137.0 mi: missing roads pass, 2026-11-22.
    (
        "ridgecrest_ca_us",
        "bishop_ca_us",
        &["ridgecrest_ca_us", "lone_pine_ca_us", "bishop_ca_us"],
    ),
    // CA-41, 167 mi: missing roads pass, 2026-11-22.
    (
        "visalia_ca_us",
        "santa_maria_ca_us",
        &[
            "visalia_ca_us",
            "san_luis_obispo_ca_us",
            "santa_maria_ca_us",
        ],
    ),
    // I-8, 186 mi: missing roads pass, 2026-11-22.
    (
        "yuma_az_us",
        "phoenix_az_us",
        &["yuma_az_us", "buckeye_az_us", "phoenix_az_us"],
    ),
    // Great Lakes missing roads (2026-11-22): legs split at a city they pass.
    (
        "bloomington_il_us",
        "chicago_il_us",
        &["bloomington_il_us", "joliet_il_us", "chicago_il_us"],
    ),
    (
        "chicago_il_us",
        "st_louis_mo_us",
        &[
            "chicago_il_us",
            "joliet_il_us",
            "bloomington_il_us",
            "springfield_il_us",
            "st_louis_mo_us",
        ],
    ),
    (
        "eau_claire_wi_us",
        "wausau_wi_us",
        &["eau_claire_wi_us", "chippewa_falls_wi_us", "wausau_wi_us"],
    ),
    (
        "indianapolis_in_us",
        "south_bend_in_us",
        &["indianapolis_in_us", "kokomo_in_us", "south_bend_in_us"],
    ),
    (
        "ann_arbor_mi_us",
        "fort_wayne_in_us",
        &["ann_arbor_mi_us", "jackson_mi_us", "fort_wayne_in_us"],
    ),
    // Northeast missing roads (2026-11-23): legs split at a city they pass.
    (
        "newport_ri_us",
        "new_haven_ct_us",
        &["newport_ri_us", "new_london_ct_us", "new_haven_ct_us"],
    ),
    (
        "allentown_pa_us",
        "baltimore_md_us",
        &[
            "allentown_pa_us",
            "reading_pa_us",
            "lancaster_pa_us",
            "baltimore_md_us",
        ],
    ),
    (
        "albany_ny_us",
        "montpelier_vt_us",
        &["albany_ny_us", "rutland_vt_us", "montpelier_vt_us"],
    ),
    (
        "new_york_ny_us",
        "albany_ny_us",
        &["new_york_ny_us", "newburgh_ny_us", "albany_ny_us"],
    ),
    (
        "buffalo_ny_us",
        "new_york_ny_us",
        &["buffalo_ny_us", "newburgh_ny_us", "new_york_ny_us"],
    ),
    (
        "rochester_ny_us",
        "new_york_ny_us",
        &["rochester_ny_us", "newburgh_ny_us", "new_york_ny_us"],
    ),
    (
        "meadville_pa_us",
        "pittsburgh_pa_us",
        &[
            "meadville_pa_us",
            "cranberry_township_pa_us",
            "pittsburgh_pa_us",
        ],
    ),
    (
        "youngstown_oh_us",
        "pittsburgh_pa_us",
        &[
            "youngstown_oh_us",
            "cranberry_township_pa_us",
            "pittsburgh_pa_us",
        ],
    ),
    (
        "pittsburgh_pa_us",
        "cleveland_oh_us",
        &[
            "pittsburgh_pa_us",
            "cranberry_township_pa_us",
            "cleveland_oh_us",
        ],
    ),
    // Upper Midwest and Northern Plains missing roads (2026-11-23): legs
    // split at a city they pass.
    (
        "hibbing_mn_us",
        "minneapolis_mn_us",
        &["hibbing_mn_us", "grand_rapids_mn_us", "minneapolis_mn_us"],
    ),
    (
        "mankato_mn_us",
        "rochester_mn_us",
        &["mankato_mn_us", "owatonna_mn_us", "rochester_mn_us"],
    ),
    (
        "des_moines_ia_us",
        "chicago_il_us",
        &["des_moines_ia_us", "aurora_il_us", "chicago_il_us"],
    ),
    // I-44, 713 mi: missing roads pass (Texas and Louisiana), 2026-11-23.
    // The chain runs over the I-35 and I-44 legs already on the map.
    (
        "dallas_tx_us",
        "st_louis_mo_us",
        &[
            "dallas_tx_us",
            "denton_tx_us",
            "ardmore_ok_us",
            "oklahoma_city_ok_us",
            "tulsa_ok_us",
            "joplin_mo_us",
            "springfield_mo_us",
            "rolla_mo_us",
            "st_louis_mo_us",
        ],
    ),
    // I-35, 200 mi: missing roads pass, 2026-11-23.
    (
        "fort_worth_tx_us",
        "oklahoma_city_ok_us",
        &[
            "fort_worth_tx_us",
            "denton_tx_us",
            "ardmore_ok_us",
            "oklahoma_city_ok_us",
        ],
    ),
    // US-287, 133 mi: missing roads pass, 2026-11-23.
    (
        "wichita_falls_tx_us",
        "mckinney_tx_us",
        &["wichita_falls_tx_us", "denton_tx_us", "mckinney_tx_us"],
    ),
    // I-35, 128 mi: missing roads pass, 2026-11-23.
    (
        "mckinney_tx_us",
        "waco_tx_us",
        &["mckinney_tx_us", "dallas_tx_us", "waco_tx_us"],
    ),
    // I-35, 109 mi: missing roads pass, 2026-11-23. Denton to Ardmore and
    // Dallas to Denton ride the same road.
    (
        "ardmore_ok_us",
        "dallas_tx_us",
        &["ardmore_ok_us", "denton_tx_us", "dallas_tx_us"],
    ),
    // Appalachia and Mid-South missing roads (2026-11-23): legs split at a
    // city they pass.
    (
        "johnson_city_tn_us",
        "pikeville_ky_us",
        &["johnson_city_tn_us", "kingsport_tn_us", "pikeville_ky_us"],
    ),
    (
        "asheville_nc_us",
        "winston_salem_nc_us",
        &["asheville_nc_us", "hickory_nc_us", "winston_salem_nc_us"],
    ),
    (
        "charleston_wv_us",
        "richmond_va_us",
        &[
            "charleston_wv_us",
            "beckley_wv_us",
            "staunton_va_us",
            "charlottesville_va_us",
            "richmond_va_us",
        ],
    ),
];

/// The replacement chain for a retired hop `a` to `b`, in travel order.
pub fn replacement_chain(a: &str, b: &str) -> Option<Vec<String>> {
    RETIRED_LEGS.iter().find_map(|(from, to, chain)| {
        if *from == a && *to == b {
            Some(chain.iter().map(|c| (*c).to_string()).collect())
        } else if *from == b && *to == a {
            Some(chain.iter().rev().map(|c| (*c).to_string()).collect())
        } else {
            None
        }
    })
}

/// `cities` with every retired hop the map cannot build swapped for its
/// replacement chain. `has_hop` says whether the map has a direct leg.
pub fn expand_retired_hops(
    cities: Vec<String>,
    has_hop: impl Fn(&str, &str) -> bool,
) -> Vec<String> {
    let Some(first) = cities.first().cloned() else {
        return cities;
    };
    let mut out = vec![first];
    for pair in cities.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        match replacement_chain(a, b) {
            Some(chain) if !has_hop(a, b) && chain.windows(2).all(|w| has_hop(&w[0], &w[1])) => {
                out.extend(chain.into_iter().skip(1));
            }
            _ => out.push(b.clone()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|c| (*c).to_string()).collect()
    }

    #[test]
    fn a_retired_hop_becomes_its_chain_both_ways() {
        let retired = |a: &str, b: &str| {
            (a == "surrey_bc_ca" && b == "prince_george_bc_ca")
                || (a == "prince_george_bc_ca" && b == "surrey_bc_ca")
        };
        let has = |a: &str, b: &str| !retired(a, b);
        let out = expand_retired_hops(
            s(&["dawson_creek_bc_ca", "prince_george_bc_ca", "surrey_bc_ca"]),
            has,
        );
        assert_eq!(
            out,
            s(&[
                "dawson_creek_bc_ca",
                "prince_george_bc_ca",
                "quesnel_bc_ca",
                "williams_lake_bc_ca",
                "cache_creek_bc_ca",
                "hope_bc_ca",
                "chilliwack_bc_ca",
                "abbotsford_bc_ca",
                "surrey_bc_ca",
            ])
        );
    }

    #[test]
    fn a_hop_the_map_still_has_is_left_alone() {
        let out = expand_retired_hops(s(&["surrey_bc_ca", "prince_george_bc_ca"]), |_, _| true);
        assert_eq!(out, s(&["surrey_bc_ca", "prince_george_bc_ca"]));
    }

    #[test]
    fn a_chain_with_a_missing_link_is_not_used() {
        let has = |a: &str, b: &str| {
            !(a == "surrey_bc_ca" && b == "prince_george_bc_ca")
                && !(a == "hope_bc_ca" && b == "cache_creek_bc_ca")
        };
        let out = expand_retired_hops(s(&["surrey_bc_ca", "prince_george_bc_ca"]), has);
        assert_eq!(out, s(&["surrey_bc_ca", "prince_george_bc_ca"]));
    }
}
