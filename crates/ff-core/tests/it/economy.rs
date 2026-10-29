//! Canadian diesel prices and the unchanged US regional fuel table.

use ff_core::models::economy::{
    cad_per_litre, canada_diesel_usd_per_gal, Economy, REGION_FUEL_PRICE,
};
use ff_core::pyfmt::round_py_n;

#[test]
fn test_canadian_diesel_prices_convert_for_codes_and_names() {
    let bc = canada_diesel_usd_per_gal("BC").expect("British Columbia has a price");
    let yukon = canada_diesel_usd_per_gal("YT").expect("Yukon has a price");
    assert_eq!(round_py_n(bc, 2), 4.83);
    assert_eq!(round_py_n(yukon, 2), 4.56);
    assert_eq!(canada_diesel_usd_per_gal("British Columbia"), Some(bc));
    assert_eq!(canada_diesel_usd_per_gal("Yukon"), Some(yukon));
    assert!(canada_diesel_usd_per_gal("WA").is_none());
    assert!(canada_diesel_usd_per_gal("AK").is_none());
    assert!(canada_diesel_usd_per_gal("Washington").is_none());
    assert!(canada_diesel_usd_per_gal("Alaska").is_none());
    assert!(canada_diesel_usd_per_gal("bc").is_none());
    assert!(canada_diesel_usd_per_gal("british columbia").is_none());
    assert!((cad_per_litre(bc) - 1.78475).abs() <= 1e-4);
}

#[test]
fn test_non_canadian_fuel_prices_keep_the_seeded_us_table() {
    let economy = Economy::new(Some(7));
    assert_eq!(REGION_FUEL_PRICE.len(), 16);
    for (region, _) in REGION_FUEL_PRICE {
        let original = economy.fuel_price(region);
        assert_eq!(economy.fuel_price_at(region, "WA"), original);
        assert_eq!(economy.fuel_price_at(region, "AK"), original);
    }
}

#[test]
fn test_offline_canadian_price_uses_the_regional_market_wobble() {
    let economy = Economy::new(Some(7));
    let base = canada_diesel_usd_per_gal("BC").expect("British Columbia has a price");
    let price = economy.fuel_price_at("pacific_northwest", "BC");
    assert!((round_py_n(base * 0.92, 2)..=round_py_n(base * 1.10, 2)).contains(&price));
}

#[test]
fn test_live_canadian_price_tracks_the_live_us_national_price() {
    let mut economy = Economy::new(Some(7));
    let live = 5.967;
    let base = canada_diesel_usd_per_gal("BC").expect("British Columbia has a price");
    economy.set_live_national_price(Some(live));
    assert_eq!(
        economy.fuel_price_at("pacific_northwest", "BC"),
        round_py_n(
            (live + (base - Economy::table_national_price())).max(0.5),
            2
        )
    );
}
