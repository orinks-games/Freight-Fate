//! The load screen for ramp advisories the bake read in the wrong unit.
//!
//! OpenStreetMap reads a bare `maxspeed:advisory` number as km/h, and the
//! bake did too. US mappers often leave the "mph" off, so a ramp signed
//! 15 mph went into the tree as 9.321 and a truck was held to 9 mph down
//! the whole quarter mile of Cleveland's exit 170A (agent drive,
//! 2026-10-24). 64 advisories in the tree have the pattern.
//!
//! The screen is for self-contradiction, not extremity: a US advisory sign
//! posts whole miles per hour in steps of 5, so a value that is not a whole
//! number but is exactly a whole multiple of 5 km/h converted was a bare
//! number read as km/h, and the number on the sign is that whole one. It
//! runs at load, so the record keeps what the bake read.

/// The 0.621 371 192 the bake multiplied a bare number by.
const MPH_PER_KMH: f64 = 0.621_371_192;

/// `mph` as the sign posts it.
pub(super) fn screen_bare_kmh_advisory(mph: f64) -> f64 {
    if (mph - mph.round()).abs() < 1e-6 {
        return mph;
    }
    let posted = mph / MPH_PER_KMH;
    let whole = posted.round();
    // The tree keeps three decimals, so a conversion lands within 0.001
    // mph of the product: 0.002 km/h.
    if (posted - whole).abs() < 0.002 && whole % 5.0 == 0.0 {
        whole
    } else {
        mph
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bare_number_read_as_kmh_is_the_mph_on_the_sign() {
        assert_eq!(screen_bare_kmh_advisory(9.321), 15.0);
        assert_eq!(screen_bare_kmh_advisory(27.962), 45.0);
        assert_eq!(screen_bare_kmh_advisory(15.534), 25.0);
    }

    #[test]
    fn a_reading_in_mph_is_left_alone() {
        assert_eq!(screen_bare_kmh_advisory(25.0), 25.0);
        assert_eq!(screen_bare_kmh_advisory(32.0), 32.0);
        // Not a conversion of a whole 5 km/h step.
        assert_eq!(screen_bare_kmh_advisory(22.4), 22.4);
    }
}
