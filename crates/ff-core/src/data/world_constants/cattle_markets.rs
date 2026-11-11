//! Where cattle are bought, fed and processed: the cities that get a sale
//! barn, a feed yard or a beef plant.
//!
//! These are stamped as representative template facilities, as the other
//! types are, and they are stamped even in a stand-in market: a cattle town
//! with nothing else surveyed still has its auction, its yards or its plant.
//! Each city is listed for a business the trade knows it for; the names are
//! the basis, not claims the game makes:
//!
//! - Beef plants: Tyson at Amarillo, Holcomb (Garden City), Lexington and
//!   Dakota City (across the river from Sioux City); National Beef at Dodge
//!   City and Liberal; Cargill at Dodge City; JBS at Grand Island; Greater
//!   Omaha Packing at Omaha. Read from the CME's approved stockyard and
//!   slaughter plant list and National Beef's own plant list.
//! - Feed yards: the Texas Panhandle (Hereford calls itself the Beef Capital
//!   of the World for its feedyards; Amarillo, Lubbock), southwest Kansas
//!   (Garden City, Dodge City, Liberal), central and western Nebraska
//!   (Lexington, Kearney, Grand Island, North Platte, Scottsbluff) and
//!   eastern New Mexico (Clovis). Read from the Hereford Economic
//!   Development Corporation and USDA cattle-on-feed state reports; the
//!   city-level picks are from the trade's well-known names and were not
//!   checked against a register.
//! - Sale barns: cities with a USDA AMS market news auction report, among
//!   them Joplin Regional Stockyards (Carthage), Oklahoma National
//!   Stockyards (Oklahoma City), Public Auction Yards (Billings), and the
//!   auctions at Springfield, Sioux Falls, Salina, Dodge City, Miles City,
//!   San Angelo, Abilene, Amarillo, Roswell and North Platte.

/// `(city key, cattle facility types)`, sorted by city key.
pub const CATTLE_FACILITY_CITIES: &[(&str, &[&str])] = &[
    ("abilene_tx_us", &["sale_barn"]),
    ("amarillo_tx_us", &["sale_barn", "feed_yard", "beef_plant"]),
    ("billings_mt_us", &["sale_barn"]),
    ("clovis_nm_us", &["feed_yard"]),
    (
        "dodge_city_ks_us",
        &["sale_barn", "feed_yard", "beef_plant"],
    ),
    ("garden_city_ks_us", &["feed_yard", "beef_plant"]),
    ("grand_island_ne_us", &["feed_yard", "beef_plant"]),
    ("hereford_tx_us", &["feed_yard"]),
    ("joplin_mo_us", &["sale_barn"]),
    ("kearney_ne_us", &["feed_yard"]),
    ("lexington_ne_us", &["feed_yard", "beef_plant"]),
    ("liberal_ks_us", &["feed_yard", "beef_plant"]),
    ("lubbock_tx_us", &["feed_yard"]),
    ("miles_city_mt_us", &["sale_barn"]),
    ("north_platte_ne_us", &["sale_barn", "feed_yard"]),
    ("oklahoma_city_ok_us", &["sale_barn"]),
    ("omaha_ne_us", &["beef_plant"]),
    ("roswell_nm_us", &["sale_barn"]),
    ("salina_ks_us", &["sale_barn"]),
    ("san_angelo_tx_us", &["sale_barn"]),
    ("scottsbluff_ne_us", &["feed_yard"]),
    ("sioux_city_ia_us", &["beef_plant"]),
    ("sioux_falls_sd_us", &["sale_barn"]),
    ("springfield_mo_us", &["sale_barn"]),
];

/// The cattle facility types stamped in this city, if any.
pub fn cattle_facility_types(city_key: &str) -> &'static [&'static str] {
    CATTLE_FACILITY_CITIES
        .iter()
        .find(|(key, _)| *key == city_key)
        .map(|(_, types)| *types)
        .unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cattle_cities_are_sorted_and_unique() {
        for pair in CATTLE_FACILITY_CITIES.windows(2) {
            assert!(pair[0].0 < pair[1].0, "{} before {}", pair[0].0, pair[1].0);
        }
    }

    #[test]
    fn every_listed_type_is_a_cattle_type() {
        for (_, types) in CATTLE_FACILITY_CITIES {
            for facility_type in *types {
                assert!(
                    matches!(*facility_type, "sale_barn" | "feed_yard" | "beef_plant"),
                    "{facility_type}"
                );
            }
        }
    }
}
