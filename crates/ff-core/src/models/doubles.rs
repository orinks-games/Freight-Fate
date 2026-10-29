//! Sets of doubles: what a second trailer and a converter dolly add.
//!
//! Two programs hook two trailers behind a converter dolly: `double_van`
//! (STAA twin 28-foot pups for `parcel_doubles`) and `turnpike_double`
//! (48-foot LCV turnpike doubles for `turnpike_doubles`). This module holds
//! the numbers that set them apart from a single trailer, each marked the way
//! the rest of the sim marks its figures: READ straight from the cited source,
//! DERIVED from READ figures by the arithmetic shown, or ASSUMED where no
//! public figure was found (those are open questions for realism review).
//!
//! * **Hook-up time.** Two trailers and a dolly are two hook-ups, not one, and
//!   the pre-trip walk-around covers both trailers and the dolly.
//! * **Tare.** Both trailers and the dolly count in the gross weight.
//! * **Rearward amplification.** In a quick lane change the rear trailer of a
//!   set swings harder than the tractor that started it.
//! * **Legal gross.** STAA pups stay at the federal 80,000 lb; turnpike
//!   doubles take the recorded per-state turnpike cap.

use crate::data::lcv_turnpikes::{cargo_requires_lcv_turnpike, lcv_turnpike_gvw_cap_lb};
use crate::models::trailers::{trailer_keys_for_cargo, trailer_type};

/// International avoirdupois pound (the same constant as `sim::vehicle`).
const KG_PER_LB: f64 = 0.45359237;

/// Federal gross vehicle weight limit on the Interstate, pounds. STAA twin
/// 28-foot doubles run under the same 80,000 lb cap as a single.
pub const STANDARD_GVW_LB: f64 = 80_000.0;

/// The second hook-up: roll the converter dolly under the lead trailer, back
/// the tractor and lead trailer under the pup, couple the dolly's fifth
/// wheel, hang the glad hands and light cord between the boxes, and raise the
/// pup's landing gear. ASSUMED: no public time study was found. Set well
/// under the 25 minutes the game gives a whole drop and hook
/// (`trailer_yard::DROP_HOOK_MIN`), since the paperwork is already done.
pub const DOUBLES_SECOND_HOOK_MIN: f64 = 15.0;

/// The longer pre-trip: the second trailer's lamps, tires and brakes, and the
/// dolly's drawbar, safety chains, fifth wheel, tires and brakes. ASSUMED: no
/// public time study was found. Two thirds of the game's own 15 minute
/// walk-around (`driving_core::WALK_AROUND_MIN`), which already covers a
/// tractor and one trailer.
pub const DOUBLES_WALK_AROUND_EXTRA_MIN: f64 = 10.0;

/// Breaking the set at the receiver: drop the rear pup, pull the converter
/// dolly out from under the lead trailer and park it, then drop the lead.
/// ASSUMED at about 15 minutes, the figure the realism review set; no public
/// time study was found.
pub const DOUBLES_BREAK_SET_MIN: f64 = 15.0;

/// A single-axle converter dolly, pounds. READ: Silver Eagle Manufacturing,
/// Value Added converter dolly model VAST-20 spec sheet ("Weight: 2,740
/// pounds", GVWR 20,000 lb), Section VA, updated 2005-04-06,
/// silvereaglemfg.com/wp-content/uploads/Section-1-VA_Dolly.pdf.
pub const SINGLE_AXLE_DOLLY_TARE_LB: f64 = 2_740.0;

/// FHWA 2014 CTSW Study, Modal Shift Comparative Analysis Technical Report,
/// Table 4 "Assumed Tare Weights": tractor plus two 28-foot trailers (2-S1-2),
/// dry van, 30,350 lb. READ.
pub const FHWA_2014_TWIN_28_TARE_LB: f64 = 30_350.0;
/// Same table: tractor plus three 28-foot trailers (2-S1-2-2), 39,275 lb.
/// READ. The table's note puts the scenario triple on a non-sleeper cab-over
/// tractor; that the twin control rides the same tractor is ASSUMED (both
/// are LTL linehaul sets), and is an open question for realism review.
pub const FHWA_2014_TRIPLE_28_TARE_LB: f64 = 39_275.0;

/// One 28-foot pup, pounds. DERIVED: a triple is a twin plus one more pup
/// and one more single-axle dolly on the same tractor (see above), so the
/// FHWA triple less the FHWA twin is one pup and one dolly (39,275 - 30,350
/// = 8,925 lb); less the Silver Eagle dolly's 2,740 lb leaves 6,185 lb.
pub const PUP_TRAILER_TARE_LB: f64 =
    FHWA_2014_TRIPLE_28_TARE_LB - FHWA_2014_TWIN_28_TARE_LB - SINGLE_AXLE_DOLLY_TARE_LB;

/// FHWA 2000 CTSW Study, Volume III, Chapter V, Table V-3: empty weight of a
/// five-axle semitrailer, 30,500 lb. READ.
pub const FHWA_2000_FIVE_AXLE_SEMI_TARE_LB: f64 = 30_500.0;
/// Same table: empty weight of a nine-axle turnpike double, 46,700 lb. READ.
pub const FHWA_2000_TURNPIKE_DOUBLE_TARE_LB: f64 = 46_700.0;

/// What a turnpike double's two 48-foot vans and tandem converter dolly weigh
/// beyond the single van they replace, pounds. DERIVED: both FHWA vehicles
/// pull a three-axle tractor, so the difference of the two empty weights is
/// the difference of the trailing units: 46,700 - 30,500 = 16,200 lb. The
/// split between the two vans and the dolly is not published there, so it
/// is carried as one figure.
pub const TURNPIKE_DOUBLE_EXTRA_OVER_SINGLE_LB: f64 =
    FHWA_2000_TURNPIKE_DOUBLE_TARE_LB - FHWA_2000_FIVE_AXLE_SEMI_TARE_LB;

/// Rearward amplification of STAA doubles with two 28-foot trailers: the rear
/// trailer's lateral acceleration over the tractor's in a quick lane change.
/// READ: FHWA 2000 CTSW Study, Volume III, Chapter VIII, p. VIII-9
/// ("Semitrailer combinations have an RA equal to 1.0 ... Currently designed
/// STAA doubles (two 28-foot trailers) have RAs on the order of 1.7").
pub const PUP_REARWARD_AMPLIFICATION: f64 = 1.7;

/// Rearward amplification of a turnpike double. DERIVED: Figure VIII-11 of
/// the same chapter charts the nine-axle turnpike double 35.89 percent worse
/// than the five-axle semitrailer (RA 1.0) in rearward amplification, so
/// about 1.36. The STAA A-train double charts 79.93 percent worse there,
/// which agrees with the chapter's "on the order of 1.7" in the text.
pub const TURNPIKE_DOUBLE_REARWARD_AMPLIFICATION: f64 = 1.3589;

/// The share of the freight riding in the rear trailer of a set. DERIVED:
/// the two trailers of both programs are the same length, so half.
pub const REAR_TRAILER_FREIGHT_SHARE: f64 = 0.5;

/// Spoken and shown when reverse is refused with doubles hooked.
pub const DOUBLES_NO_REVERSE_TEXT: &str = "Reverse refused. You don't back a set of \
     doubles: the converter dolly is a second pivot and the rear trailer jackknifes within \
     a few feet. Pull forward and go around instead.";

/// Spoken and shown when a quick steer swings the rear trailer hard.
pub const REAR_TRAILER_WHIP_TEXT: &str = "The rear trailer whipped on that steer and \
     shifted its freight. On doubles the back trailer swings harder than the tractor, so ease \
     into lane changes.";

/// The whip line for an empty set, which has no freight to shift.
pub const REAR_TRAILER_WHIP_EMPTY_TEXT: &str = "The rear trailer whipped on that steer. On \
     doubles the back trailer swings harder than the tractor, so ease into lane changes.";

/// The whip line, spoken and shown, for a set with or without freight.
pub fn rear_trailer_whip_text(loaded: bool) -> &'static str {
    if loaded {
        REAR_TRAILER_WHIP_TEXT
    } else {
        REAR_TRAILER_WHIP_EMPTY_TEXT
    }
}

/// The trailer program a cargo class hooks, when it is exactly one.
fn only_trailer_key(cargo_key: &str) -> Option<&'static str> {
    match trailer_keys_for_cargo(cargo_key) {
        [only] => Some(only),
        _ => None,
    }
}

/// How many trailers a program hooks (1 for an unknown key).
pub fn trailer_units(trailer_key: &str) -> u8 {
    trailer_type(trailer_key).map_or(1, |t| t.unit_count.max(1))
}

/// Whether a program is a set of doubles.
pub fn is_doubles_trailer(trailer_key: &str) -> bool {
    trailer_units(trailer_key) >= 2
}

/// The doubles program a cargo class runs on, or None for single-trailer
/// freight.
pub fn doubles_trailer_for_cargo(cargo_key: &str) -> Option<&'static str> {
    only_trailer_key(cargo_key).filter(|key| is_doubles_trailer(key))
}

/// Whether a cargo class runs on a set of doubles.
pub fn is_doubles_cargo(cargo_key: &str) -> bool {
    doubles_trailer_for_cargo(cargo_key).is_some()
}

/// Extra on-duty minutes a set of doubles adds at the hook: the second
/// hook-up plus the longer walk-around. Zero for single-trailer freight.
pub fn doubles_hook_extra_min(cargo_key: &str) -> f64 {
    if is_doubles_cargo(cargo_key) {
        DOUBLES_SECOND_HOOK_MIN + DOUBLES_WALK_AROUND_EXTRA_MIN
    } else {
        0.0
    }
}

/// Extra on-duty minutes a set of doubles adds at the receiver: breaking the
/// set. Zero for single-trailer freight.
pub fn doubles_break_extra_min(cargo_key: &str) -> f64 {
    if is_doubles_cargo(cargo_key) {
        DOUBLES_BREAK_SET_MIN
    } else {
        0.0
    }
}

/// Extra on-duty minutes a walk-around takes with a set of doubles hooked.
pub fn walk_around_extra_min(trailer_units: u8) -> f64 {
    if trailer_units >= 2 {
        DOUBLES_WALK_AROUND_EXTRA_MIN
    } else {
        0.0
    }
}

/// How much heavier a program's trailing units are than the single dry van
/// the stock combination tare already carries, in kilograms. Zero for single
/// trailers (the game prices every single box at the one stock tare).
pub fn trailer_set_extra_tare_kg(trailer_key: &str, single_trailer_tare_kg: f64) -> f64 {
    match trailer_key {
        "double_van" => {
            let set_lb = 2.0 * PUP_TRAILER_TARE_LB + SINGLE_AXLE_DOLLY_TARE_LB;
            set_lb * KG_PER_LB - single_trailer_tare_kg
        }
        "turnpike_double" => TURNPIKE_DOUBLE_EXTRA_OVER_SINGLE_LB * KG_PER_LB,
        _ => 0.0,
    }
}

/// Rearward amplification for a program: 1.0 for a single trailer.
pub fn rearward_amplification(trailer_key: &str) -> f64 {
    match trailer_key {
        "double_van" => PUP_REARWARD_AMPLIFICATION,
        "turnpike_double" => TURNPIKE_DOUBLE_REARWARD_AMPLIFICATION,
        _ => 1.0,
    }
}

/// The legal gross weight in pounds for a cargo class on a route through
/// these states (two-letter codes, any order, repeats fine).
///
/// Turnpike doubles take the lowest recorded turnpike cap among the states
/// the route touches, so a Thruway-to-Mass-Pike run is held to the Mass Pike
/// figure. A state with no recorded cap, or no states at all, is None: two
/// 48-foot trailers are not legal off the LCV turnpikes at any length or
/// weight, so such a route is refused rather than held to 80,000 lb.
/// Everything else, STAA twin 28s included, is 80,000 lb.
pub fn legal_gvw_lb_for_route<'a, I>(cargo_key: &str, state_codes: I) -> Option<f64>
where
    I: IntoIterator<Item = &'a str>,
{
    if !cargo_requires_lcv_turnpike(cargo_key) {
        return Some(STANDARD_GVW_LB);
    }
    let mut cap: Option<f64> = None;
    for state in state_codes {
        let state_cap = f64::from(lcv_turnpike_gvw_cap_lb(state)?);
        cap = Some(cap.map_or(state_cap, |c: f64| c.min(state_cap)));
    }
    cap
}

/// The legal gross, pounds, for a turnpike-doubles trip already under way on
/// a lane dispatched under the old rules, before routes through uncapped
/// states were refused. That trip finishes clean, the way a double_van
/// lessee's trip under way does: it takes the lowest recorded cap among the
/// states on its route that have one, or 80,000 lb if none does. New jobs on
/// such lanes are still refused ([`legal_gvw_lb_for_route`]).
pub fn legacy_trip_legal_gvw_lb<'a, I>(state_codes: I) -> f64
where
    I: IntoIterator<Item = &'a str>,
{
    state_codes
        .into_iter()
        .filter_map(lcv_turnpike_gvw_cap_lb)
        .map(f64::from)
        .reduce(f64::min)
        .unwrap_or(STANDARD_GVW_LB)
}

/// The combination mass the lane model's crosswind gust is taken to push,
/// pounds, per doubles program: a fixed reference, not the route's cap, so
/// the same set sways the same wherever it runs. Parcel pups: the 80,000 lb
/// federal gross they run at. Turnpike doubles: 127,400 lb, the OH/IN/MA
/// turnpike cap, as one constant for every corridor. The push itself is not
/// calibrated against any measurement (ROADMAP debt).
pub fn sway_reference_gross_lb(trailer_key: &str) -> f64 {
    match trailer_key {
        "turnpike_double" => TURNPIKE_DOUBLE_SWAY_REFERENCE_LB,
        _ => STANDARD_GVW_LB,
    }
}

/// The turnpike-doubles sway reference, pounds (see
/// [`sway_reference_gross_lb`]).
pub const TURNPIKE_DOUBLE_SWAY_REFERENCE_LB: f64 = 127_400.0;

/// The most a light set's gust sway is scaled up over the reference. An
/// empty set's reference-over-actual mass ratio is about 2.4 for pups and
/// 2.6 for turnpike doubles in this game; the ratio is clamped at 2.0 so the
/// uncalibrated push is not magnified past a round factor. ASSUMED.
pub const LIGHT_SET_SWAY_MAX: f64 = 2.0;

/// The walk-around and hook-up clause for a set of doubles, spoken and shown
/// at the shipper. Empty for single-trailer freight.
pub fn doubles_hook_text(cargo_key: &str) -> String {
    if !is_doubles_cargo(cargo_key) {
        return String::new();
    }
    format!(
        "A set of doubles is two hook-ups, about {} minutes more, and a walk-around of \
         both trailers and the converter dolly, about {} minutes more: {} extra minutes \
         on duty.",
        DOUBLES_SECOND_HOOK_MIN as i64,
        DOUBLES_WALK_AROUND_EXTRA_MIN as i64,
        doubles_hook_extra_min(cargo_key) as i64
    )
}

/// The clause for breaking a set of doubles, spoken and shown at the
/// receiver. Empty for single-trailer freight.
pub fn doubles_break_text(cargo_key: &str) -> String {
    if !is_doubles_cargo(cargo_key) {
        return String::new();
    }
    format!(
        "Breaking a set of doubles is dropping the rear trailer, unhooking and parking \
         the converter dolly, then dropping the lead trailer: about {} extra minutes on duty.",
        doubles_break_extra_min(cargo_key) as i64
    )
}

/// The receiver's screen line for breaking a set of doubles, or None for a
/// single trailer.
pub fn doubles_break_line(cargo_key: &str) -> Option<String> {
    is_doubles_cargo(cargo_key).then(|| {
        format!(
            "Doubles: drop the rear trailer, park the converter dolly, drop the lead, {} \
             extra minutes on duty",
            doubles_break_extra_min(cargo_key) as i64
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_two_doubles_programs_are_doubles() {
        assert!(is_doubles_cargo("parcel_doubles"));
        assert!(is_doubles_cargo("turnpike_doubles"));
        assert_eq!(
            doubles_trailer_for_cargo("parcel_doubles"),
            Some("double_van")
        );
        assert_eq!(
            doubles_trailer_for_cargo("turnpike_doubles"),
            Some("turnpike_double")
        );
        for single in ["general", "parcel", "food", "fuel_bulk", "steel"] {
            assert!(!is_doubles_cargo(single), "{single}");
            assert_eq!(doubles_hook_extra_min(single), 0.0);
        }
    }

    #[test]
    fn doubles_add_a_second_hook_up_and_a_longer_walk_around() {
        assert_eq!(doubles_hook_extra_min("parcel_doubles"), 25.0);
        assert_eq!(doubles_hook_extra_min("turnpike_doubles"), 25.0);
        assert_eq!(walk_around_extra_min(2), 10.0);
        assert_eq!(walk_around_extra_min(1), 0.0);
        let text = doubles_hook_text("parcel_doubles");
        assert!(text.contains("two hook-ups"), "{text}");
        assert!(text.contains("25 extra minutes"), "{text}");
        assert!(doubles_hook_text("general").is_empty());
    }

    #[test]
    fn the_tare_figures_follow_their_sources() {
        assert_eq!(PUP_TRAILER_TARE_LB, 6_185.0);
        assert_eq!(TURNPIKE_DOUBLE_EXTRA_OVER_SINGLE_LB, 16_200.0);
        // Both pups and the dolly, against the stock 6,400 kg van.
        let set_lb = 2.0 * 6_185.0 + 2_740.0;
        let extra = trailer_set_extra_tare_kg("double_van", 6_400.0);
        assert!((extra - (set_lb * KG_PER_LB - 6_400.0)).abs() < 1e-9);
        let lcv = trailer_set_extra_tare_kg("turnpike_double", 6_400.0);
        assert!((lcv - 16_200.0 * KG_PER_LB).abs() < 1e-9);
        assert_eq!(trailer_set_extra_tare_kg("dry_van", 6_400.0), 0.0);
    }

    #[test]
    fn pups_amplify_more_than_turnpike_doubles_and_singles_not_at_all() {
        assert_eq!(rearward_amplification("dry_van"), 1.0);
        assert_eq!(rearward_amplification("double_van"), 1.7);
        let lcv = rearward_amplification("turnpike_double");
        assert!(lcv > 1.0 && lcv < 1.7, "{lcv}");
    }

    #[test]
    fn the_refused_reverse_reads_exactly() {
        assert_eq!(
            DOUBLES_NO_REVERSE_TEXT,
            "Reverse refused. You don't back a set of doubles: the converter dolly is a \
             second pivot and the rear trailer jackknifes within a few feet. Pull forward and \
             go around instead."
        );
    }

    #[test]
    fn legal_gross_follows_the_route_states() {
        // Twin 28s stay at the federal cap wherever they run.
        assert_eq!(
            legal_gvw_lb_for_route("parcel_doubles", ["OH", "IN"]),
            Some(80_000.0)
        );
        assert_eq!(legal_gvw_lb_for_route("general", ["NY"]), Some(80_000.0));
        // Turnpike doubles take the recorded turnpike caps.
        assert_eq!(
            legal_gvw_lb_for_route("turnpike_doubles", ["OH", "IN"]),
            Some(127_400.0)
        );
        assert_eq!(
            legal_gvw_lb_for_route("turnpike_doubles", ["MA"]),
            Some(127_400.0)
        );
        assert_eq!(
            legal_gvw_lb_for_route("turnpike_doubles", ["NY", "NY"]),
            Some(143_000.0)
        );
        assert_eq!(
            legal_gvw_lb_for_route("turnpike_doubles", ["KS"]),
            Some(120_000.0)
        );
        // A Thruway run into Massachusetts is held to the lower cap.
        assert_eq!(
            legal_gvw_lb_for_route("turnpike_doubles", ["NY", "MA"]),
            Some(127_400.0)
        );
        // A state with no recorded cap, or no route: refused, never held to
        // 80,000 lb -- 48-foot doubles are not legal there at any weight.
        assert_eq!(
            legal_gvw_lb_for_route("turnpike_doubles", ["NY", "PA"]),
            None
        );
        assert_eq!(
            legal_gvw_lb_for_route("turnpike_doubles", std::iter::empty()),
            None
        );
        // Twin 28s keep 80,000 lb with or without a route.
        assert_eq!(
            legal_gvw_lb_for_route("parcel_doubles", std::iter::empty()),
            Some(80_000.0)
        );
    }

    #[test]
    fn a_legacy_trip_takes_its_lowest_recorded_cap_or_eighty_thousand() {
        assert_eq!(legacy_trip_legal_gvw_lb(["NY", "PA"]), 143_000.0);
        assert_eq!(legacy_trip_legal_gvw_lb(["NY", "PA", "KS"]), 120_000.0);
        assert_eq!(legacy_trip_legal_gvw_lb(["IL", "WI"]), 80_000.0);
        assert_eq!(legacy_trip_legal_gvw_lb([]), 80_000.0);
    }
}
