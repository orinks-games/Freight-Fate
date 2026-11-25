//! The speed a truck will bring to a bend, for judging when its approach call
//! is due (`Trip::next_curve_approach`) and whether a queued call is still
//! true when it is spoken.
/// The speed a truck at `speed_mph`, gaining `gaining_mph_per_s`, will carry
/// into a bend `ahead_mi` up the road.
///
/// The speed now, unless it is gaining: then what that acceleration makes of
/// the road between, v^2 + 2ad, never past `target_mph` (the speed cruise or
/// the keeper is taking it to). Judging the approach call on the speed now
/// missed every sharp bend the truck was still climbing to: out of the
/// Greensboro yard (missing-roads pass, 2026-11-23) cruise was taking a truck
/// at 32 up to 45, a 30 mph bend 0.15 mile ahead was never over the call
/// speed while it was ahead, and the first word of it was "too fast" with the
/// truck already in it.
///
/// Only under cruise or the keeper. With no set speed the driver has the
/// pedal and the speed now is what the call is judged on: projected from a
/// standing start, a truck pulling away in low gear "reached" highway speed
/// a mile out, the bend was called at 1.2 miles with nothing to advise, and
/// with its call spent it was never called again (agent drive out of Pampa,
/// 2026-11-23).
pub fn bend_approach_speed_mph(
    speed_mph: f64,
    gaining_mph_per_s: f64,
    ahead_mi: f64,
    target_mph: Option<f64>,
) -> f64 {
    let Some(target_mph) = target_mph else {
        return speed_mph;
    };
    if gaining_mph_per_s <= 0.0 || ahead_mi <= 0.0 {
        return speed_mph;
    }
    // mph^2 = mph^2 + 2 * (mph/s) * (miles * 3600 s/h)
    let reached = (speed_mph * speed_mph + 2.0 * gaining_mph_per_s * ahead_mi * 3600.0).sqrt();
    speed_mph.max(reached.min(target_mph))
}

#[cfg(test)]
mod tests {
    use super::bend_approach_speed_mph;

    #[test]
    fn a_truck_holding_speed_brings_its_speed_now() {
        assert_eq!(bend_approach_speed_mph(40.0, 0.0, 0.3, Some(45.0)), 40.0);
        assert_eq!(bend_approach_speed_mph(40.0, -1.5, 0.3, None), 40.0);
    }

    #[test]
    fn a_gaining_truck_brings_more_but_never_past_its_set_speed() {
        // 32 mph gaining 1 mph/s over 0.15 mile: sqrt(32^2 + 2 * 1 * 540).
        let free = bend_approach_speed_mph(32.0, 1.0, 0.15, Some(80.0));
        assert!((free - (32.0f64 * 32.0 + 1080.0).sqrt()).abs() < 1e-9);
        assert_eq!(bend_approach_speed_mph(32.0, 1.0, 0.15, Some(40.0)), 40.0);
        // A set speed under the truck's own never projects it slower.
        assert_eq!(bend_approach_speed_mph(32.0, 1.0, 0.15, Some(25.0)), 32.0);
    }

    #[test]
    fn with_no_set_speed_the_speed_now_is_what_counts() {
        assert_eq!(bend_approach_speed_mph(0.0, 2.0, 1.2, None), 0.0);
        assert_eq!(bend_approach_speed_mph(32.0, 1.0, 0.15, None), 32.0);
    }

    #[test]
    fn a_bend_behind_brings_the_speed_now() {
        assert_eq!(bend_approach_speed_mph(30.0, 1.0, 0.0, Some(45.0)), 30.0);
    }
}
