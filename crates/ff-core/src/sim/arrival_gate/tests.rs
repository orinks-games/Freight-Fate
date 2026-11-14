//! Transition table for the stop at a facility gate.

use super::*;
use GateStop::*;

fn reading(speed_mph: f64, parking_brake: bool, quiet_s: f64) -> GateEvent {
    GateEvent::Reading {
        speed_mph,
        parking_brake,
        quiet_s,
    }
}

fn confirm(speed_mph: f64, parking_brake: bool, assist: bool) -> GateEvent {
    GateEvent::Confirm {
        speed_mph,
        parking_brake,
        assist,
    }
}

const FAST: f64 = 10.0;
const CREEP: f64 = 2.0;
const REST: f64 = 0.0;

#[test]
fn gate_transition_table() {
    let rolling = Rolling { crept: false };
    let rolled_on = Rolling { crept: true };
    // (from, event, to, cue)
    let rows: &[(GateStop, GateEvent, GateStop, Option<GateCue>)] = &[
        // Arriving: the first line rolling, the creep line slow or stopped.
        (
            Arriving,
            reading(FAST, false, 0.1),
            rolling,
            Some(GateCue::StopAtGate),
        ),
        (
            Arriving,
            reading(CREEP, false, 0.1),
            Creeping,
            Some(GateCue::Creep),
        ),
        (
            Arriving,
            reading(REST, false, 0.1),
            Creeping,
            Some(GateCue::Creep),
        ),
        (
            Arriving,
            reading(REST, true, 0.1),
            Open,
            Some(GateCue::Open),
        ),
        // Rolling: silent while rolling, the creep line on slowing.
        (rolling, reading(FAST, false, 0.1), rolling, None),
        (
            rolling,
            reading(CREEP, false, 0.1),
            Creeping,
            Some(GateCue::Creep),
        ),
        (
            rolling,
            reading(REST, false, 0.1),
            Creeping,
            Some(GateCue::Creep),
        ),
        (rolling, reading(REST, true, 0.1), Open, Some(GateCue::Open)),
        // The park line itself is creeping, not rolling.
        (
            rolling,
            reading(DELIVERY_PARK_MPH, false, 0.1),
            Creeping,
            Some(GateCue::Creep),
        ),
        // Creeping: rest starts the brake count, rolling on is silent.
        (Creeping, reading(CREEP, false, 0.1), Creeping, None),
        (
            Creeping,
            reading(REST, false, 0.1),
            Stopped { rest_s: 0.1 },
            None,
        ),
        (
            Creeping,
            reading(DOCKING_MAX_MPH, false, 0.1),
            Stopped { rest_s: 0.1 },
            None,
        ),
        (
            Creeping,
            reading(REST, true, 0.1),
            Open,
            Some(GateCue::Open),
        ),
        (Creeping, reading(FAST, false, 0.1), rolled_on, None),
        // Stopped: the brake line once the rest delay has passed.
        (
            Stopped { rest_s: 1.0 },
            reading(REST, false, 1.0),
            Stopped { rest_s: 2.0 },
            None,
        ),
        (
            Stopped { rest_s: 2.5 },
            reading(REST, false, 0.5),
            StoppedTold,
            Some(GateCue::SetBrake),
        ),
        (
            Stopped { rest_s: 2.9 },
            reading(REST, true, 0.5),
            Open,
            Some(GateCue::Open),
        ),
        (
            Stopped { rest_s: 2.9 },
            reading(CREEP, false, 0.5),
            Creeping,
            None,
        ),
        (
            Stopped { rest_s: 2.9 },
            reading(FAST, false, 0.5),
            rolled_on,
            None,
        ),
        // No time passing, no line.
        (
            Stopped { rest_s: 2.9 },
            reading(REST, false, 0.0),
            Stopped { rest_s: 2.9 },
            None,
        ),
        // StoppedTold: said once, silent creeping, re-armed only by rolling.
        (StoppedTold, reading(REST, false, 5.0), StoppedTold, None),
        (StoppedTold, reading(CREEP, false, 5.0), StoppedTold, None),
        (
            StoppedTold,
            reading(REST, true, 0.1),
            Open,
            Some(GateCue::Open),
        ),
        (StoppedTold, reading(FAST, false, 0.1), rolled_on, None),
        // Rolled on after the creep line: no second creep line.
        (rolled_on, reading(FAST, false, 0.1), rolled_on, None),
        (rolled_on, reading(CREEP, false, 0.1), Creeping, None),
        (
            rolled_on,
            reading(REST, false, 0.1),
            Stopped { rest_s: 0.1 },
            None,
        ),
        (
            rolled_on,
            reading(REST, true, 0.1),
            Open,
            Some(GateCue::Open),
        ),
        // The confirm key, assist off: at rest opens, brake or no brake.
        (
            Arriving,
            confirm(REST, false, false),
            Open,
            Some(GateCue::Open),
        ),
        (
            Creeping,
            confirm(REST, false, false),
            Open,
            Some(GateCue::Open),
        ),
        (
            Stopped { rest_s: 1.0 },
            confirm(REST, false, false),
            Open,
            Some(GateCue::Open),
        ),
        (
            StoppedTold,
            confirm(REST, false, false),
            Open,
            Some(GateCue::Open),
        ),
        (Creeping, confirm(CREEP, false, false), Creeping, None),
        (rolling, confirm(FAST, false, false), rolling, None),
        // The confirm key, assist on: only the hold opens.
        (Arriving, confirm(REST, true, true), Arriving, None),
        (Creeping, confirm(REST, true, true), Creeping, None),
        (
            Holding,
            confirm(REST, true, true),
            Open,
            Some(GateCue::Open),
        ),
        (Holding, confirm(REST, false, true), Holding, None),
        (Holding, confirm(CREEP, true, true), Holding, None),
        // The assist: holds once at rest.
        (
            Arriving,
            GateEvent::AssistReading { speed_mph: FAST },
            Arriving,
            None,
        ),
        (
            Arriving,
            GateEvent::AssistReading { speed_mph: REST },
            Holding,
            Some(GateCue::Hold),
        ),
        (
            Creeping,
            GateEvent::AssistReading { speed_mph: REST },
            Holding,
            Some(GateCue::Hold),
        ),
        (
            Holding,
            GateEvent::AssistReading { speed_mph: REST },
            Holding,
            None,
        ),
        // Open stays open; only a reset leaves it.
        (Open, reading(FAST, false, 0.1), Open, None),
        (Open, confirm(REST, false, false), Open, None),
        (
            Open,
            GateEvent::AssistReading { speed_mph: REST },
            Open,
            None,
        ),
        // Reset from anywhere.
        (Open, GateEvent::Reset, Arriving, None),
        (rolled_on, GateEvent::Reset, Arriving, None),
        (StoppedTold, GateEvent::Reset, Arriving, None),
        (Holding, GateEvent::Reset, Arriving, None),
        (Stopped { rest_s: 2.0 }, GateEvent::Reset, Arriving, None),
    ];
    for &(from, event, to, cue) in rows {
        assert_eq!(from.next(event), (to, cue), "{from:?} on {event:?}");
    }
}

/// Feed readings until a cue or a state change, returning every cue heard.
fn run(mut gate: GateStop, events: &[GateEvent]) -> (GateStop, Vec<GateCue>) {
    let mut cues = Vec::new();
    for &event in events {
        let (next, cue) = gate.next(event);
        gate = next;
        cues.extend(cue);
    }
    (gate, cues)
}

#[test]
fn the_brake_line_is_said_once_per_stop_and_re_arms_after_rolling() {
    let frame = 1.0 / 60.0;
    let mut events = vec![reading(FAST, false, frame), reading(CREEP, false, frame)];
    // Ten seconds at rest, brake off.
    events.extend(std::iter::repeat_n(reading(REST, false, frame), 600));
    let (gate, cues) = run(Arriving, &events);
    assert_eq!(gate, StoppedTold);
    assert_eq!(
        cues,
        vec![GateCue::StopAtGate, GateCue::Creep, GateCue::SetBrake]
    );
    // Roll off, stop again: the brake line once more, and no creep line.
    let mut again = vec![reading(FAST, false, frame), reading(CREEP, false, frame)];
    again.extend(std::iter::repeat_n(reading(REST, false, frame), 600));
    let (gate, cues) = run(gate, &again);
    assert_eq!(gate, StoppedTold);
    assert_eq!(cues, vec![GateCue::SetBrake]);
}

#[test]
fn the_brake_line_waits_the_whole_rest_delay() {
    let frame = 0.1;
    let mut gate = Creeping;
    let mut rested = 0.0;
    loop {
        let (next, cue) = gate.next(reading(REST, false, frame));
        gate = next;
        rested += frame;
        if cue.is_some() {
            assert_eq!(cue, Some(GateCue::SetBrake));
            break;
        }
        assert!(rested < GATE_BRAKE_PROMPT_S + frame, "never said");
    }
    // The first reading at rest counts its own frame.
    assert!(rested >= GATE_BRAKE_PROMPT_S - 1e-9, "{rested}");
}

#[test]
fn only_quiet_seconds_count_toward_the_brake_line() {
    // At rest under a spoken line for a minute: the delay has not started.
    let spoken = std::iter::repeat_n(reading(REST, false, 0.0), 3600);
    let mut events = vec![reading(CREEP, false, 0.0)];
    events.extend(spoken);
    let (gate, cues) = run(Arriving, &events);
    assert_eq!(gate, Stopped { rest_s: 0.0 });
    assert_eq!(cues, vec![GateCue::Creep]);
    // The voice goes quiet: the whole delay from there.
    let frame = 0.1;
    let short = (GATE_BRAKE_PROMPT_S / frame) as usize - 1;
    let quiet: Vec<GateEvent> = std::iter::repeat_n(reading(REST, false, frame), short).collect();
    let (gate, cues) = run(gate, &quiet);
    assert!(matches!(gate, Stopped { .. }), "{gate:?}");
    assert!(cues.is_empty(), "{cues:?}");
    let (gate, cues) = run(gate, &[reading(REST, false, frame); 2]);
    assert_eq!(gate, StoppedTold);
    assert_eq!(cues, vec![GateCue::SetBrake]);
}

#[test]
fn setting_the_brake_or_confirming_inside_the_delay_is_silent() {
    let frame = 0.1;
    let rest = std::iter::repeat_n(reading(REST, false, frame), 20);
    let mut braked: Vec<GateEvent> = vec![reading(CREEP, false, frame)];
    braked.extend(rest.clone());
    braked.push(reading(REST, true, frame));
    assert_eq!(
        run(Arriving, &braked),
        (Open, vec![GateCue::Creep, GateCue::Open])
    );

    let mut confirmed: Vec<GateEvent> = vec![reading(CREEP, false, frame)];
    confirmed.extend(rest);
    confirmed.push(confirm(REST, false, false));
    assert_eq!(
        run(Arriving, &confirmed),
        (Open, vec![GateCue::Creep, GateCue::Open])
    );
}

#[test]
fn prompted_and_told_follow_the_old_latches() {
    assert!(!Arriving.stop_prompted());
    assert!(!Rolling { crept: false }.stop_prompted());
    for state in [
        Rolling { crept: true },
        Creeping,
        Stopped { rest_s: 0.0 },
        StoppedTold,
        Holding,
        Open,
    ] {
        assert!(state.stop_prompted(), "{state:?}");
        assert!(state.told_to_stop(), "{state:?}");
    }
    assert!(!Arriving.told_to_stop());
    assert!(Rolling { crept: false }.told_to_stop());
    assert!(Holding.is_holding());
    assert!(Rolling { crept: true }.is_rolling());
    assert!(!Creeping.is_rolling());
}
