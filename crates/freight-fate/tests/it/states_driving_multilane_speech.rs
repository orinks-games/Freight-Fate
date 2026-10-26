//! The route-start merge instruction is never dropped (the driving-events
//! case of `tests/test_multilane_speech.py`; the lane counts, the summary
//! and the callouts are in `crates/ff-core/tests/sim_multilane_speech.rs`).

use crate::states_driving_menus_support::*;
use ff_core::sim::trip_models::{NavigationCue, TripEvent, TripEventData, TripEventKind};
use ff_core::speech_text::SpokenMessage;
use freight_fate::app::testing::TestApp;
use freight_fate::speech::EventPriority;

fn cue_event(kind: &str, advance: bool) -> TripEvent {
    let cue = NavigationCue::new(&format!("{kind}:0:10:x"), kind, 10.0, "a direction", "");
    TripEvent {
        kind: TripEventKind::GpsCue,
        message: SpokenMessage::new("a direction".to_string()),
        data: TripEventData {
            cue: Some(cue),
            advance: if advance { Some(true) } else { None },
            ..Default::default()
        },
    }
}

/// The first instruction of the whole run -- "Merge onto I-70 West toward
/// Silverthorne; 67 miles" -- was dropped as stale chatter on the owner's
/// Denver playtest.
///
/// Which way onto the highway, which way through an interchange, which way
/// down a street: lose one and the driver goes the wrong way. The ADVANCE
/// half stays droppable, because a heads-up arriving late is worse than one
/// that never comes.
///
/// Python read this off the source of `_event_priority`; here the priority
/// itself is asked, which is what the source text stood in for.
#[test]
fn test_the_route_start_merge_instruction_is_never_dropped() {
    let mut app = TestApp::new();
    let drive = a_drive(&mut app);

    for kind in ["onramp", "maneuver", "local_turn"] {
        let priority = with_drive(&drive, |d| d.event_priority(&cue_event(kind, false)));
        assert_eq!(
            priority,
            EventPriority::Important,
            "{kind} cues can still age out"
        );
        // The advance half keeps its exemption: still ambient, still droppable.
        let advance = with_drive(&drive, |d| d.event_priority(&cue_event(kind, true)));
        assert_eq!(
            advance,
            EventPriority::Ambient,
            "the advance half lost its exemption"
        );
    }
}
