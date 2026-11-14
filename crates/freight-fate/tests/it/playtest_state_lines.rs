//! The mapped state line, heard on a whole delivery (a case of
//! `tests/test_playtest_harness.py`).
//!
//! # One route per test, on purpose
//!
//! Each case here drives a complete 500-mile delivery, and the four of them
//! together were once a single test with a loop in it. That one test took
//! 54.8 seconds of the suite's 123 -- and because it is one test, it is one
//! thread: it set the floor for the whole run no matter how many cores the
//! machine had. Four tests over the same four routes is the same coverage on
//! four threads, and a failure now names the route it happened on instead of
//! whichever iteration got there first.

use freight_fate::playtest::{PlaytestHarness, RouteSetup};

/// The mapped state line is announced, once, ahead of the city it precedes.
///
/// Two surfaces, because the drive has two. The transcript is the EAR: what
/// the voice actually read out. `ctx.message_log` is the RECORD: every road
/// line the drive produced, in order, whether or not the voice got to it --
/// which is what the review keys are for.
///
/// The surveyed crossing is checked at the ear, because that is the line
/// this test is about and the player does hear it. The city passing line is
/// checked at the record, because it is chatter and now competes for the
/// voice on the same real-time budget a player's drive gives it (see
/// `PlaytestHarness`'s clock): a route waypoint fires its junction
/// instructions -- "keep right for I-24 East toward Atlanta", "continue on
/// I-24 for 247 miles toward Atlanta" -- in the same few seconds, they own
/// the channel, and "Passing Nashville, Tennessee." is dropped as stale
/// ambient behind them in every seeded run of this route. That is the pacer
/// doing its job on the least urgent and most redundant of the three lines,
/// so the assertion follows the record rather than pretending the ear got
/// it. What this test is about survives untouched: the crossing is spoken,
/// once, at the surveyed mile, and the city line never carries the
/// unmapped-route fallback prefix.
fn assert_state_line_is_authoritative(
    cities: Vec<&str>,
    state: &str,
    passing_city: &str,
    expected_crossings: usize,
) {
    {
        let mut harness = PlaytestHarness::new();
        // Seeded: the road's random furniture (patrols, chatter, weather)
        // decides how busy the voice is around the city, and this test is
        // about wording and order, not about that lottery.
        harness.start_route(
            cities[0],
            cities[cities.len() - 1],
            RouteSetup::seeded(4242)
                .named(&format!("{state} narration"))
                .cities(&cities),
        );
        // Keep the whole run's record instead of the last 200 lines: a
        // 500-mile delivery otherwise evicts the boundary before the dock.
        harness.app.ctx.message_log.limit = 100_000;
        let result = harness.drive_delivery_to_completion();
        let record: Vec<String> = harness
            .app
            .ctx
            .message_log
            .messages
            .iter()
            .map(|message| message.text.clone())
            .collect();

        // The ear: the surveyed crossing, spoken, once.
        let crossings = result
            .transcript
            .iter()
            .filter(|line| line.contains(&format!("Crossing into {state}")))
            .count();
        assert_eq!(
            crossings,
            expected_crossings,
            "{}",
            result.transcript_text()
        );

        // The record: one city line, in the mapped wording, after the
        // boundary it belongs behind.
        let passing_phrase = format!("Passing {passing_city}, {state}.");
        let passing = record
            .iter()
            .position(|line| line.contains(&passing_phrase))
            .unwrap_or_else(|| panic!("the city was never announced: {record:?}"));
        assert_eq!(
            record
                .iter()
                .filter(|line| line.contains(&passing_phrase))
                .count(),
            1,
            "{record:?}"
        );
        assert!(
            !record.iter().any(
                |line| line.contains(&format!("Crossing into {state}. Passing {passing_city}"))
            ),
            "the mapped crossing was repeated as a prefix on the city line: {record:?}"
        );
        if expected_crossings > 0 {
            let boundary = record
                .iter()
                .position(|line| line.contains(&format!("Crossing into {state} near ")))
                .unwrap_or_else(|| panic!("the mapped boundary was never announced: {record:?}"));
            assert!(boundary < passing, "{record:?}");
            // And it is the SURVEYED wording that reached the player, not a
            // bare fallback.
            assert!(
                result
                    .transcript_text()
                    .contains(&format!("Crossing into {state} near ")),
                "{}",
                result.transcript_text()
            );
        }
    }
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "sweep: a whole 500-mile delivery, 10-40 s"
)]
fn test_mapped_state_lines_are_authoritative_northbound_to_atlanta() {
    assert_state_line_is_authoritative(
        vec!["Indianapolis", "Nashville", "Atlanta"],
        "Tennessee",
        "Nashville",
        1,
    );
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "sweep: a whole 500-mile delivery, 10-40 s"
)]
fn test_mapped_state_lines_are_authoritative_southbound_to_indianapolis() {
    assert_state_line_is_authoritative(
        vec!["Atlanta", "Nashville", "Indianapolis"],
        "Tennessee",
        "Nashville",
        1,
    );
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "sweep: a whole 500-mile delivery, 10-40 s"
)]
fn test_mapped_state_lines_are_authoritative_westbound_to_albuquerque() {
    assert_state_line_is_authoritative(
        vec!["Shreveport", "Dallas", "Albuquerque"],
        "Texas",
        "Dallas",
        1,
    );
}

/// No mapped boundary on this route, so nothing is lost and the fallback is
/// the only announcement either way.
#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "sweep: a whole 500-mile delivery, 10-40 s"
)]
fn test_mapped_state_lines_are_authoritative_within_one_state() {
    assert_state_line_is_authoritative(
        vec!["Dallas", "San Antonio", "Houston"],
        "Texas",
        "San Antonio",
        0,
    );
}
