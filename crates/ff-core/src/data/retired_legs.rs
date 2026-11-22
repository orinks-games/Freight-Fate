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
