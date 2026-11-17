//! Transition table for the no engine brake zone notice.

use super::*;
use ZoneNotice::*;

fn inside(city: &str, told: bool, assist_cued: bool) -> ZoneNotice {
    Inside {
        city: city.to_string(),
        told,
        assist_cued,
    }
}

fn at(city: &str) -> ZoneEvent {
    ZoneEvent::Inside {
        city: city.to_string(),
    }
}

fn approaching(start_mi: f64) -> ZoneEvent {
    ZoneEvent::Approaching { start_mi }
}

#[test]
fn zone_notice_transition_table() {
    let heads_up = HeadsUp { start_tenths: 125 };
    // (from, event, terse, to, cue)
    let rows: Vec<(ZoneNotice, ZoneEvent, bool, ZoneNotice, Option<ZoneCue>)> = vec![
        // The heads-up, once per zone, never in terse speech.
        (
            Untold,
            approaching(12.5),
            false,
            heads_up.clone(),
            Some(ZoneCue::HeadsUp),
        ),
        (Untold, approaching(12.5), true, Untold, None),
        (
            heads_up.clone(),
            approaching(12.5),
            false,
            heads_up.clone(),
            None,
        ),
        (
            heads_up.clone(),
            approaching(30.0),
            false,
            HeadsUp { start_tenths: 300 },
            Some(ZoneCue::HeadsUp),
        ),
        // Crossing the town line carries whether the driver was told.
        (
            Untold,
            at("Waco"),
            false,
            inside("Waco", false, false),
            None,
        ),
        (
            heads_up.clone(),
            at("Waco"),
            false,
            inside("Waco", true, false),
            None,
        ),
        (
            inside("Waco", true, true),
            at("Waco"),
            false,
            inside("Waco", true, true),
            None,
        ),
        // A zone running into the next town's: told stays, the assist line
        // is per town.
        (
            inside("Waco", true, true),
            at("Temple"),
            false,
            inside("Temple", true, false),
            None,
        ),
        // The assist line, once per town, never in terse speech.
        (
            inside("Waco", false, false),
            ZoneEvent::AssistReleased,
            false,
            inside("Waco", true, true),
            Some(ZoneCue::AssistReleased),
        ),
        (
            inside("Waco", true, true),
            ZoneEvent::AssistReleased,
            false,
            inside("Waco", true, true),
            None,
        ),
        (
            inside("Waco", false, false),
            ZoneEvent::AssistReleased,
            true,
            inside("Waco", false, false),
            None,
        ),
        // The warning to switch it off tells the driver too.
        (
            inside("Waco", false, false),
            ZoneEvent::Warned,
            false,
            inside("Waco", true, false),
            None,
        ),
        // Leaving: the end line only for a zone the driver heard about.
        (
            inside("Waco", true, false),
            ZoneEvent::Outside,
            false,
            Untold,
            Some(ZoneCue::Ends),
        ),
        (
            inside("Waco", true, false),
            ZoneEvent::Outside,
            true,
            Untold,
            None,
        ),
        (
            inside("Waco", false, false),
            ZoneEvent::Outside,
            false,
            Untold,
            None,
        ),
        // Outside with nothing inside to end: nothing moves.
        (Untold, ZoneEvent::Outside, false, Untold, None),
        (
            heads_up.clone(),
            ZoneEvent::Outside,
            false,
            heads_up.clone(),
            None,
        ),
        (Untold, ZoneEvent::Warned, false, Untold, None),
        (Untold, ZoneEvent::AssistReleased, false, Untold, None),
    ];
    for (from, event, terse, to, cue) in rows {
        let label = format!("{from:?} + {event:?} (terse {terse})");
        assert_eq!(from.next(event, terse), (to, cue), "{label}");
    }
}

#[test]
fn zone_notice_keys() {
    assert_eq!(Untold.key(), "untold");
    assert_eq!(HeadsUp { start_tenths: 1 }.key(), "heads_up");
    assert_eq!(inside("Waco", false, false).key(), "inside_untold");
    assert_eq!(inside("Waco", true, false).key(), "inside_told");
}
