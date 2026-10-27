//! The ladder's tests. The pacer's (`busy()` with the projection, and the
//! priority order) moved to Hearken with the pacer.
use crate::speech_pacing::*;

// -- the S4 driving verbosity ladder (tests/test_driving_speech_ladder.py)

/// "coaching" was a fourth rung and is gone (2026-08-17).
///
/// It never differed from standard at the voice -- measured on two
/// scenarios, byte-identical transcripts -- because its two table cells
/// only bite where a coaching tip repeats and the game has exactly one
/// line in that category. A setting that offers a choice and changes
/// nothing audible is worse than one fewer choice in a game played by
/// ear. The CATEGORY survives; the rung does not.
#[test]
fn test_the_ladder_has_three_named_rungs() {
    assert_eq!(DRIVING_SPEECH_MODES, ["standard", "quiet", "urgent_only"]);
    assert!(disposition_row("coaching").is_none());
}

#[test]
fn test_every_rung_rules_on_every_category() {
    for mode in DRIVING_SPEECH_MODES {
        for category in SpeechCategory::ALL {
            assert!(Disposition::ALL.contains(&disposition_for(mode, Some(category))));
        }
    }
}

#[test]
fn test_safety_and_immediate_navigation_speak_at_every_rung() {
    // R1's never-dropped contract outranks the ladder. A rung may
    // shorten these; it may never silence them.
    for mode in DRIVING_SPEECH_MODES {
        for category in [SpeechCategory::Safety, SpeechCategory::Navigation] {
            assert!(matches!(
                disposition_for(mode, Some(category)),
                Disposition::Full | Disposition::Terse
            ));
        }
    }
}

#[test]
fn test_an_untagged_line_speaks_at_every_rung() {
    // A call site nobody has classified yet must be too loud, never silent.
    for mode in DRIVING_SPEECH_MODES {
        assert!(matches!(
            disposition_for(mode, None),
            Disposition::Full | Disposition::Terse
        ));
    }
}

#[test]
fn test_the_table_reads_exactly_as_the_spec_says() {
    assert_eq!(
        *disposition_row("standard").unwrap(),
        [
            (SpeechCategory::Safety, Disposition::Full),
            (SpeechCategory::Money, Disposition::Full),
            (SpeechCategory::Navigation, Disposition::Full),
            (SpeechCategory::NavigationAdvisory, Disposition::Full),
            (SpeechCategory::Coaching, Disposition::FirstOccurrence),
            (SpeechCategory::Confirmation, Disposition::Full),
            (SpeechCategory::Status, Disposition::Transitions),
            (SpeechCategory::Traffic, Disposition::Full),
        ]
    );
    assert_eq!(
        *disposition_row("quiet").unwrap(),
        [
            (SpeechCategory::Safety, Disposition::Terse),
            (SpeechCategory::Money, Disposition::Terse),
            (SpeechCategory::Navigation, Disposition::Terse),
            (SpeechCategory::NavigationAdvisory, Disposition::Terse),
            (SpeechCategory::Coaching, Disposition::Silent),
            (SpeechCategory::Confirmation, Disposition::Terse),
            (SpeechCategory::Status, Disposition::Terse),
            (SpeechCategory::Traffic, Disposition::Silent),
        ]
    );
    assert_eq!(
        *disposition_row("urgent_only").unwrap(),
        [
            (SpeechCategory::Safety, Disposition::Terse),
            (SpeechCategory::Money, Disposition::Silent),
            (SpeechCategory::Navigation, Disposition::Terse),
            (SpeechCategory::NavigationAdvisory, Disposition::Silent),
            (SpeechCategory::Coaching, Disposition::Silent),
            (SpeechCategory::Confirmation, Disposition::Silent),
            (SpeechCategory::Status, Disposition::Silent),
            (SpeechCategory::Traffic, Disposition::Silent),
        ]
    );
}

/// Quiet is meant to be quiet: the traffic around the truck is words at
/// standard and nothing at either quieter rung (player report, 2026-10-03,
/// on a quiet rung still reading out every slow box truck and its speed).
#[test]
fn test_traffic_around_the_truck_speaks_only_at_standard() {
    assert_eq!(
        disposition_for("standard", Some(SpeechCategory::Traffic)),
        Disposition::Full
    );
    for rung in ["quiet", "urgent_only"] {
        assert_eq!(
            disposition_for(rung, Some(SpeechCategory::Traffic)),
            Disposition::Silent,
            "{rung}"
        );
    }
}

#[test]
fn test_an_unknown_rung_falls_back_to_standard() {
    assert_eq!(
        disposition_for("nonsense", Some(SpeechCategory::Status)),
        Disposition::Transitions
    );
}

/// A guard against the shape of the bug, not just this instance of it.
///
/// If a later change makes every category quiet and urgent_only disagree
/// on inaudible at quiet, the two settings become indistinguishable again
/// however different the table looks.
#[test]
fn test_the_two_quietest_rungs_are_not_the_same_setting() {
    let quiet = disposition_row("quiet").unwrap();
    let urgent = disposition_row("urgent_only").unwrap();
    let audible = [
        Disposition::Full,
        Disposition::Terse,
        Disposition::FirstOccurrence,
        Disposition::Transitions,
    ];
    let differ: Vec<SpeechCategory> = quiet
        .iter()
        .filter(|(category, disposition)| row_disposition(urgent, *category) != Some(*disposition))
        .map(|(category, _)| *category)
        .collect();
    assert!(!differ.is_empty(), "the rungs are identical");
    assert!(
        differ
            .iter()
            .any(|c| audible.contains(&row_disposition(quiet, *c).unwrap())),
        "quiet and urgent_only differ only in categories quiet already \
             silences, so a player switching between them hears no change"
    );
}

/// What a rung leaves out is silent (owner, 2026-10-03): no stand-in
/// sound. Every cell is either spoken in some form or silent.
#[test]
fn test_every_cell_speaks_or_is_silent() {
    for mode in DRIVING_SPEECH_MODES {
        for (category, disposition) in disposition_row(mode).unwrap() {
            assert!(
                Disposition::ALL.contains(disposition),
                "{mode} {category:?}"
            );
        }
    }
    assert_eq!(Disposition::ALL.len(), 5);
}

/// Quiet and urgent_only must not be able to drop a line as already-said.
///
/// Both quieter rungs deliver every category TERSE, EARCON, or SILENT --
/// the earcon/silent gate returns before `_ladder_repeats` is reached,
/// and a TERSE line is never dropped -- so the suppression that swallowed
/// 310 lines on standard could not touch them. That is a property of the
/// TABLE, not of the code, and it is exactly the kind of thing a later
/// table edit changes without anyone noticing: moving one quiet cell to
/// TRANSITIONS would silently hand quiet a leg-scoped memory it has never
/// had. Pinned so that edit has to be deliberate.
#[test]
fn test_only_standard_can_reach_the_already_said_gate() {
    let reaches_the_gate = [Disposition::FirstOccurrence, Disposition::Transitions];
    for rung in ["quiet", "urgent_only"] {
        let offenders: Vec<_> = disposition_row(rung)
            .unwrap()
            .iter()
            .filter(|(_, disposition)| reaches_the_gate.contains(disposition))
            .collect();
        assert!(
            offenders.is_empty(),
            "{rung} now reaches the already-said gate for {offenders:?}; \
                 that gate is standard's alone (Darren, 2026-08-19)"
        );
    }
}

#[test]
fn test_enum_values_round_trip() {
    for category in SpeechCategory::ALL {
        assert_eq!(SpeechCategory::from_value(category.value()), Some(category));
    }
    assert_eq!(SpeechCategory::from_value("flavor"), None);
    assert_eq!(Disposition::FirstOccurrence.value(), "first");
}
