//! Low bridges and weight limits stay in the world data but make no GPS
//! call: routing already avoids anything a legal truck cannot pass, so
//! Joplin to Tulsa used to speak up to 122 lines that changed nothing
//! (owner, 2026-11-22).

use crate::sim_support::*;

#[test]
fn restriction_records_make_no_cue_and_no_spoken_line() {
    let mut trip = make_trip(world(), "Joplin", "Tulsa", 2);
    let records: usize = trip
        .route
        .legs
        .iter()
        .map(|leg| leg.restrictions().len())
        .sum();
    assert!(records > 0, "the I-44 legs still carry restriction records");
    assert!(
        trip.navigation_cues
            .iter()
            .all(|cue| cue.kind != "restriction"),
        "no cue is made from a restriction record"
    );

    let total = trip.total_miles();
    let mut spoken = Vec::new();
    let mut mi = 0.0;
    while mi < total {
        trip.position_mi = mi;
        spoken.extend(trip.update(0.0).iter().map(|e| e.text().to_string()));
        mi += 0.25;
    }
    let restriction_lines: Vec<&String> = spoken
        .iter()
        .filter(|line| {
            let lower = line.to_lowercase();
            lower.contains("low bridge")
                || lower.contains("weight limit")
                || lower.contains("your route clears")
        })
        .collect();
    assert!(restriction_lines.is_empty(), "{restriction_lines:?}");
}
