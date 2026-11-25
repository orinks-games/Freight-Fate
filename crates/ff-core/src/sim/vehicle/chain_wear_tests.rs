//! Chain wear by the mile: chains used the way the game asks cost the base
//! rate; bare road past its grace and driving over chain speed wear fast
//! (owner report, 2026-11-23: one clean use at Donner cost 19 percent).

use super::*;

/// Wear in percent for `miles` at `mph` on `surface`, in tenth-second
/// frames, starting `bare_mi` into a bare stretch.
fn wear_over(surface: &str, mph: f64, miles: f64, bare_mi: f64) -> f64 {
    let mut t = TruckState::default();
    t.transmission.automatic = true;
    t.start_engine();
    t.chains_on = true;
    t.surface = surface.to_string();
    t.velocity_mps = mph / 2.23694;
    t.chain_bare_mi = bare_mi;
    let frames = (miles * 1609.344 / t.velocity_mps / 0.1).round() as usize;
    for _ in 0..frames {
        t.update_wear(0.1);
    }
    t.chain_wear_pct
}

#[test]
fn chain_wear_per_mile_table() {
    // (surface, mph, starting bare miles, wear per mile in percent)
    let base = CHAIN_WEAR_PCT_PER_MILE;
    let bare = base * CHAIN_WEAR_BARE_MULT;
    let cases: &[(&str, f64, f64, f64)] = &[
        // Snow at the cruise cap, and a hair over it: the base rate.
        ("snow", 30.0, 0.0, base),
        ("snow", 31.5, 0.0, base),
        // Snow clearly over chain speed.
        ("snow", 40.0, 0.0, base * CHAIN_WEAR_OVERSPEED_MULT),
        // Bare road inside the grace, then past it.
        ("dry", 30.0, 0.0, base),
        ("dry", 30.0, CHAIN_BARE_GRACE_MI, bare),
        (
            "dry",
            40.0,
            CHAIN_BARE_GRACE_MI,
            bare * CHAIN_WEAR_OVERSPEED_MULT,
        ),
    ];
    for &(surface, mph, bare_mi, want) in cases {
        // A mile inside the grace stays inside it: start no later than 1.0.
        let got = wear_over(surface, mph, 1.0, bare_mi);
        assert!(
            (got - want).abs() < want * 0.02,
            "{surface} at {mph} from {bare_mi} bare: {got} per mile, want {want}"
        );
        assert!(
            (chain_wear_pct_per_mile(
                if surface == "snow" {
                    0.0
                } else {
                    bare_mi + 0.5
                },
                mph
            ) - want)
                .abs()
                < 1e-9
        );
    }
}

#[test]
fn the_bare_stretch_counts_road_miles_and_ends_on_snow() {
    let mut t = TruckState::default();
    t.start_engine();
    t.chains_on = true;
    t.surface = "dry".to_string();
    t.velocity_mps = 30.0 / 2.23694;
    for _ in 0..600 {
        t.update_wear(0.1); // a minute at 30 mph: half a mile
    }
    assert!((t.chain_bare_mi - 0.5).abs() < 0.01, "{}", t.chain_bare_mi);
    t.surface = "snow".to_string();
    t.update_wear(0.1);
    assert_eq!(t.chain_bare_mi, 0.0);
    t.surface = "dry".to_string();
    t.update_wear(0.1);
    t.chains_on = false;
    t.update_wear(0.1);
    assert_eq!(t.chain_bare_mi, 0.0);
}

#[test]
fn chains_left_on_bare_road_at_chain_speed_snap_about_14_miles_on() {
    // The driver who ignores "Bare road." still gets the slap warning (85
    // percent) and the snap, later than before: 2 miles at the base rate,
    // then 8 percent a mile.
    let mut t = TruckState::default();
    t.start_engine();
    t.chains_on = true;
    t.surface = "dry".to_string();
    t.velocity_mps = 30.0 / 2.23694;
    let mut miles = 0.0;
    let mut slap_at = None;
    while t.chains_on && miles < 40.0 {
        t.update_wear(0.1);
        miles += t.velocity_mps * 0.1 / 1609.344;
        if slap_at.is_none() && t.chain_wear_pct >= 85.0 {
            slap_at = Some(miles);
        }
    }
    assert!(t.chains_just_snapped);
    assert!((miles - 14.45_f64).abs() < 0.2, "snapped at {miles}");
    let slap_at = slap_at.expect("the slap comes first");
    assert!((slap_at - 12.6_f64).abs() < 0.2, "slap at {slap_at}");
    assert!((t.damage_pct - CHAIN_SNAP_DAMAGE_PCT).abs() < 1e-9);
}
