//! The recognizer against scripted fingers: one test per row of the gesture
//! tables in `docs/ios/README.md`, plus the edges between them.

use super::*;
use crate::touch::Gesture::*;

/// One finger's straight line in a [`Script::drag`]: its id, from, to.
type Stroke = (i64, (f32, f32), (f32, f32));

/// A clock and a recognizer; every call advances time and ticks.
struct Script {
    r: Recognizer,
    t: f64,
}

impl Script {
    fn new() -> Self {
        Self {
            r: Recognizer::default(),
            t: 10.0,
        }
    }

    fn down(&mut self, id: i64, x: f32, y: f32) -> &mut Self {
        self.r.down(id, x, y, self.t);
        self
    }

    fn up(&mut self, id: i64, x: f32, y: f32) -> &mut Self {
        self.r.up(id, x, y, self.t);
        self
    }

    /// Let `seconds` pass, ticking every frame (60 a second).
    fn wait(&mut self, seconds: f64) -> &mut Self {
        let end = self.t + seconds;
        while self.t < end {
            self.t = (self.t + 1.0 / 60.0).min(end);
            self.r.tick(self.t);
        }
        self
    }

    /// Move fingers in a straight line over `seconds`, a frame at a time.
    /// Each entry is (id, from, to).
    fn drag(&mut self, fingers: &[Stroke], seconds: f64) -> &mut Self {
        let frames = ((seconds * 60.0).round() as usize).max(1);
        for frame in 1..=frames {
            self.t += seconds / frames as f64;
            let k = frame as f32 / frames as f32;
            for &(id, (x0, y0), (x1, y1)) in fingers {
                self.r
                    .moved(id, x0 + (x1 - x0) * k, y0 + (y1 - y0) * k, self.t);
            }
            self.r.tick(self.t);
        }
        self
    }

    fn take(&mut self) -> Vec<Gesture> {
        self.r.drain()
    }
}

#[test]
fn a_tap_waits_for_its_partner_then_counts_once() {
    let mut s = Script::new();
    s.down(1, 100.0, 100.0).wait(0.05).up(1, 101.0, 100.0);
    assert_eq!(s.take(), vec![], "a tap waits to see if a second follows");
    s.wait(0.3);
    assert_eq!(s.take(), vec![Tap]);
}

#[test]
fn two_quick_taps_are_a_double_tap() {
    let mut s = Script::new();
    s.down(1, 100.0, 100.0).wait(0.05).up(1, 100.0, 100.0);
    s.wait(0.1)
        .down(1, 104.0, 102.0)
        .wait(0.05)
        .up(1, 104.0, 102.0);
    s.wait(0.5);
    assert_eq!(s.take(), vec![DoubleTap]);
}

#[test]
fn a_flick_is_one_step() {
    let mut s = Script::new();
    s.down(1, 200.0, 400.0)
        .drag(&[(1, (200.0, 400.0), (200.0, 340.0))], 0.08)
        .up(1, 200.0, 340.0);
    assert_eq!(s.take(), vec![SwipeUp]);
    s.wait(1.0);
    assert_eq!(s.take(), vec![], "nothing follows a flick");
}

#[test]
fn a_swipe_and_hold_is_continuous_until_the_lift() {
    let mut s = Script::new();
    s.down(1, 200.0, 400.0)
        .drag(&[(1, (200.0, 400.0), (200.0, 360.0))], 0.08)
        .wait(0.3);
    assert_eq!(s.take(), vec![HoldUpperBegan], "gas while the finger stays");
    s.wait(2.0);
    assert_eq!(s.take(), vec![]);
    s.up(1, 200.0, 360.0);
    assert_eq!(s.take(), vec![HoldEnded]);
}

#[test]
fn every_direction_holds_its_own_control() {
    for ((dx, dy), began) in [
        ((0.0, -40.0), HoldUpperBegan),
        ((0.0, 40.0), HoldLowerBegan),
        ((-40.0, 0.0), HoldLeftBegan),
        ((40.0, 0.0), HoldRightBegan),
    ] {
        let mut s = Script::new();
        s.down(1, 300.0, 300.0)
            .drag(&[(1, (300.0, 300.0), (300.0 + dx, 300.0 + dy))], 0.08)
            .wait(0.3);
        assert_eq!(s.take(), vec![began]);
    }
}

#[test]
fn a_long_brisk_flick_is_not_mistaken_for_a_hold() {
    let mut s = Script::new();
    // Crosses 30 points early and keeps travelling well past the window.
    s.down(1, 200.0, 600.0)
        .drag(&[(1, (200.0, 600.0), (200.0, 300.0))], 0.3)
        .up(1, 200.0, 300.0);
    assert_eq!(s.take(), vec![SwipeUp]);
}

#[test]
fn a_finger_held_still_straightens_the_wheel() {
    let mut s = Script::new();
    s.down(1, 200.0, 200.0).wait(0.45);
    assert_eq!(s.take(), vec![]);
    s.wait(0.1);
    assert_eq!(s.take(), vec![StillHoldBegan]);
    s.up(1, 203.0, 201.0);
    assert_eq!(s.take(), vec![HoldEnded]);
}

#[test]
fn the_deep_swipe_needs_the_brake_hold_first() {
    let mut s = Script::new();
    s.down(1, 200.0, 100.0)
        .drag(&[(1, (200.0, 100.0), (200.0, 140.0))], 0.08)
        .wait(0.3);
    assert_eq!(s.take(), vec![HoldLowerBegan]);
    s.drag(&[(1, (200.0, 140.0), (200.0, 200.0))], 0.2);
    assert_eq!(s.take(), vec![DeepSwipeDownBegan]);
    s.up(1, 200.0, 200.0);
    assert_eq!(s.take(), vec![HoldEnded]);

    // A fast long flick down is a flick, never the deep swipe.
    let mut s = Script::new();
    s.down(1, 200.0, 100.0)
        .drag(&[(1, (200.0, 100.0), (200.0, 300.0))], 0.12)
        .up(1, 200.0, 300.0);
    assert_eq!(s.take(), vec![SwipeDown]);
}

#[test]
fn practice_hears_each_holds_furthest_travel_as_it_lifts() {
    let mut s = Script::new();
    s.r.set_report_travel(true);
    // Up 70 points, back to 50, lifted: the furthest point counts.
    s.down(1, 200.0, 400.0)
        .drag(&[(1, (200.0, 400.0), (200.0, 360.0))], 0.08)
        .wait(0.3)
        .drag(&[(1, (200.0, 360.0), (200.0, 330.0))], 0.1)
        .drag(&[(1, (200.0, 330.0), (200.0, 350.0))], 0.1)
        .up(1, 200.0, 350.0);
    assert_eq!(s.take(), vec![HoldUpperBegan, Travel(70), HoldEnded]);

    // Sideways holds measure along their own direction.
    let mut s = Script::new();
    s.r.set_report_travel(true);
    s.down(1, 300.0, 300.0)
        .drag(&[(1, (300.0, 300.0), (255.0, 305.0))], 0.08)
        .wait(0.3)
        .up(1, 255.0, 305.0);
    assert_eq!(s.take(), vec![HoldLeftBegan, Travel(45), HoldEnded]);

    // A still hold has no direction, and a flick is not a hold.
    let mut s = Script::new();
    s.r.set_report_travel(true);
    s.down(1, 200.0, 200.0).wait(0.6).up(1, 203.0, 201.0);
    s.down(2, 200.0, 600.0)
        .drag(&[(2, (200.0, 600.0), (200.0, 300.0))], 0.3)
        .up(2, 200.0, 300.0);
    assert_eq!(s.take(), vec![StillHoldBegan, HoldEnded, SwipeUp]);
}

#[test]
fn travel_is_only_reported_while_practice_asks() {
    let mut s = Script::new();
    s.down(1, 200.0, 400.0)
        .drag(&[(1, (200.0, 400.0), (200.0, 300.0))], 0.08)
        .wait(0.3)
        .up(1, 200.0, 300.0);
    assert_eq!(s.take(), vec![HoldUpperBegan, HoldEnded]);
}

/// Hold gas with finger 1 and return the script, the hold reported.
fn holding_gas() -> Script {
    let mut s = Script::new();
    s.down(1, 200.0, 500.0)
        .drag(&[(1, (200.0, 500.0), (200.0, 460.0))], 0.08)
        .wait(0.3);
    assert_eq!(s.take(), vec![HoldUpperBegan]);
    s
}

#[test]
fn a_second_finger_tap_on_the_gas_is_its_own_command() {
    let mut s = holding_gas();
    s.down(2, 600.0, 300.0)
        .wait(0.05)
        .up(2, 600.0, 300.0)
        .wait(0.4);
    assert_eq!(s.take(), vec![UpperHoldTap]);
    // A double tap of the second finger.
    s.down(2, 600.0, 300.0).wait(0.05).up(2, 600.0, 300.0);
    s.wait(0.1)
        .down(2, 600.0, 300.0)
        .wait(0.05)
        .up(2, 600.0, 300.0);
    s.wait(0.4);
    assert_eq!(s.take(), vec![UpperHoldDoubleTap]);
    s.up(1, 200.0, 460.0);
    assert_eq!(s.take(), vec![HoldEnded]);
}

#[test]
fn hold_tap_lift_runs_the_tap_with_the_pedal_down() {
    let mut s = holding_gas();
    s.down(2, 600.0, 300.0).wait(0.05).up(2, 600.0, 300.0);
    s.up(1, 200.0, 460.0);
    assert_eq!(s.take(), vec![UpperHoldTap, HoldEnded]);
}

#[test]
fn a_second_finger_flick_shifts_and_a_side_hold_steers() {
    let mut s = holding_gas();
    s.down(2, 600.0, 300.0)
        .drag(&[(2, (600.0, 300.0), (600.0, 250.0))], 0.08)
        .up(2, 600.0, 250.0);
    assert_eq!(s.take(), vec![UpperHoldSwipeUp]);
    s.down(2, 600.0, 300.0)
        .drag(&[(2, (600.0, 300.0), (550.0, 300.0))], 0.08)
        .wait(0.3);
    assert_eq!(s.take(), vec![SecondSteerLeftBegan]);
    s.up(2, 550.0, 300.0);
    assert_eq!(s.take(), vec![SecondSteerEnded]);
    s.up(1, 200.0, 460.0);
    assert_eq!(s.take(), vec![HoldEnded]);
}

#[test]
fn a_brake_holds_second_finger_has_the_brake_commands() {
    let mut s = Script::new();
    s.down(1, 200.0, 100.0)
        .drag(&[(1, (200.0, 100.0), (200.0, 140.0))], 0.08)
        .wait(0.3);
    assert_eq!(s.take(), vec![HoldLowerBegan]);
    s.down(2, 600.0, 300.0)
        .wait(0.05)
        .up(2, 600.0, 300.0)
        .wait(0.4);
    assert_eq!(s.take(), vec![LowerHoldTap]);
}

#[test]
fn two_finger_taps_stop_speech_and_double_is_the_magic_tap() {
    let mut s = Script::new();
    s.down(1, 200.0, 300.0).down(2, 300.0, 300.0).wait(0.08);
    s.up(1, 200.0, 300.0).up(2, 300.0, 300.0).wait(0.4);
    assert_eq!(s.take(), vec![TwoFingerTap]);

    for _ in 0..2 {
        s.down(1, 200.0, 300.0).down(2, 300.0, 300.0).wait(0.08);
        s.up(1, 200.0, 300.0).up(2, 300.0, 300.0).wait(0.1);
    }
    s.wait(0.4);
    assert_eq!(s.take(), vec![MagicTap]);
}

#[test]
fn a_three_finger_tap_is_immediate() {
    let mut s = Script::new();
    s.down(1, 200.0, 300.0)
        .down(2, 300.0, 300.0)
        .down(3, 400.0, 300.0)
        .wait(0.08)
        .up(1, 200.0, 300.0)
        .up(2, 300.0, 300.0)
        .up(3, 400.0, 300.0);
    assert_eq!(s.take(), vec![ThreeFingerTap]);
}

#[test]
fn two_finger_flicks_up_and_down_arrive_mid_stroke() {
    let mut s = Script::new();
    s.down(1, 200.0, 300.0).down(2, 300.0, 300.0).drag(
        &[
            (1, (200.0, 300.0), (200.0, 360.0)),
            (2, (300.0, 300.0), (300.0, 360.0)),
        ],
        0.1,
    );
    assert_eq!(s.take(), vec![TwoFingerSwipeDown], "before the lift");
    s.up(1, 200.0, 360.0).up(2, 300.0, 360.0).wait(0.5);
    assert_eq!(s.take(), vec![]);
}

#[test]
fn two_finger_side_flicks_wait_for_the_lift() {
    let mut s = Script::new();
    s.down(1, 200.0, 300.0).down(2, 200.0, 400.0).drag(
        &[
            (1, (200.0, 300.0), (260.0, 300.0)),
            (2, (200.0, 400.0), (260.0, 400.0)),
        ],
        0.1,
    );
    assert_eq!(s.take(), vec![]);
    s.up(1, 260.0, 300.0).up(2, 260.0, 400.0);
    assert_eq!(s.take(), vec![TwoFingerSwipeRight]);
}

#[test]
fn the_two_finger_scrub_goes_back() {
    let mut s = Script::new();
    let (a, b) = ((300.0, 300.0), (300.0, 400.0));
    s.down(1, a.0, a.1).down(2, b.0, b.1);
    for (from, to) in [(300.0, 340.0), (340.0, 290.0), (290.0, 340.0)] {
        s.drag(
            &[(1, (from, a.1), (to, a.1)), (2, (from, b.1), (to, b.1))],
            0.12,
        );
    }
    s.up(1, 340.0, a.1).up(2, 340.0, b.1);
    assert_eq!(s.take(), vec![Escape]);
}

/// The scrub's second is timed from the second finger, as UIKit's is: a
/// first finger that rested a moment before its partner landed does not
/// use the scrub's time up (parity with `ios/ff_touch.m`, 2026-11-22).
#[test]
fn the_scrub_is_timed_from_the_second_finger() {
    let mut s = Script::new();
    let (a, b) = ((300.0, 300.0), (300.0, 400.0));
    s.down(1, a.0, a.1).wait(0.3).down(2, b.0, b.1);
    for (from, to) in [(300.0, 340.0), (340.0, 290.0), (290.0, 340.0)] {
        s.drag(
            &[(1, (from, a.1), (to, a.1)), (2, (from, b.1), (to, b.1))],
            0.28,
        );
    }
    s.up(1, 340.0, a.1).up(2, 340.0, b.1);
    assert_eq!(s.take(), vec![Escape]);
}

#[test]
fn a_slow_two_finger_side_drag_is_not_a_scrub() {
    let mut s = Script::new();
    s.down(1, 300.0, 300.0).down(2, 300.0, 400.0).drag(
        &[
            (1, (300.0, 300.0), (360.0, 300.0)),
            (2, (300.0, 400.0), (360.0, 400.0)),
        ],
        0.15,
    );
    s.up(1, 360.0, 300.0).up(2, 360.0, 400.0);
    assert_eq!(s.take(), vec![TwoFingerSwipeRight]);
}

#[test]
fn two_fingers_held_sound_the_horn() {
    let mut s = Script::new();
    s.down(1, 200.0, 300.0).down(2, 300.0, 300.0).wait(0.3);
    assert_eq!(s.take(), vec![]);
    s.wait(0.1);
    assert_eq!(s.take(), vec![HornHoldBegan]);
    s.up(1, 200.0, 300.0);
    assert_eq!(s.take(), vec![HoldEnded]);
    s.up(2, 300.0, 300.0).wait(0.5);
    assert_eq!(s.take(), vec![]);
}

/// Turn two fingers about their midpoint by `degrees` (clockwise on screen
/// when positive) over `seconds`.
fn turn(s: &mut Script, degrees: f32, seconds: f64) {
    let frames = (seconds * 60.0) as usize;
    let (cx, cy, r) = (300.0_f32, 300.0_f32, 60.0_f32);
    for frame in 1..=frames {
        s.t += seconds / frames as f64;
        let a = (degrees * frame as f32 / frames as f32).to_radians();
        s.r.moved(1, cx - r * a.cos(), cy - r * a.sin(), s.t);
        s.r.moved(2, cx + r * a.cos(), cy + r * a.sin(), s.t);
        s.r.tick(s.t);
    }
}

#[test]
fn turning_two_fingers_is_the_engine_key() {
    let mut s = Script::new();
    s.down(1, 240.0, 300.0).down(2, 360.0, 300.0);
    turn(&mut s, 40.0, 0.25);
    assert_eq!(s.take(), vec![RotateRight]);
    s.up(1, 0.0, 0.0).up(2, 0.0, 0.0).wait(0.5);
    assert_eq!(s.take(), vec![], "one turn, one key");

    let mut s = Script::new();
    s.down(1, 240.0, 300.0).down(2, 360.0, 300.0);
    turn(&mut s, -40.0, 0.25);
    assert_eq!(s.take(), vec![RotateLeft]);
}

#[test]
fn turning_cancels_the_horn() {
    let mut s = Script::new();
    s.down(1, 240.0, 300.0).down(2, 360.0, 300.0).wait(0.4);
    assert_eq!(s.take(), vec![HornHoldBegan]);
    turn(&mut s, 40.0, 0.3);
    assert_eq!(s.take(), vec![HoldEnded, RotateRight]);
    s.up(1, 0.0, 0.0).up(2, 0.0, 0.0);
    assert_eq!(s.take(), vec![], "the turn already ended the horn");
}

#[test]
fn three_finger_flicks_go_every_way() {
    for ((dx, dy), gesture) in [
        ((0.0, -50.0), ThreeFingerSwipeUp),
        ((0.0, 50.0), ThreeFingerSwipeDown),
        ((-50.0, 0.0), ThreeFingerSwipeLeft),
        ((50.0, 0.0), ThreeFingerSwipeRight),
    ] {
        let mut s = Script::new();
        let starts = [(1, 200.0, 300.0), (2, 300.0, 300.0), (3, 400.0, 300.0)];
        for &(id, x, y) in &starts {
            s.down(id, x, y);
        }
        let moves: Vec<_> = starts
            .iter()
            .map(|&(id, x, y)| (id, (x, y), (x + dx, y + dy)))
            .collect();
        s.drag(&moves, 0.1);
        assert_eq!(s.take(), vec![gesture]);
    }
}

#[test]
fn a_pending_tap_is_sent_before_what_follows() {
    let mut s = Script::new();
    s.down(1, 100.0, 100.0)
        .wait(0.05)
        .up(1, 100.0, 100.0)
        .wait(0.1);
    s.down(1, 200.0, 400.0)
        .drag(&[(1, (200.0, 400.0), (200.0, 340.0))], 0.08)
        .up(1, 200.0, 340.0);
    assert_eq!(s.take(), vec![Tap, SwipeUp]);
}

#[test]
fn losing_the_screen_lets_go_of_every_hold() {
    let mut s = holding_gas();
    s.down(2, 600.0, 300.0)
        .drag(&[(2, (600.0, 300.0), (550.0, 300.0))], 0.08)
        .wait(0.3);
    assert_eq!(s.take(), vec![SecondSteerLeftBegan]);
    s.r.cancel();
    assert_eq!(s.take(), vec![SecondSteerEnded, HoldEnded]);
    assert!(!s.r.busy());
}

#[test]
fn a_fourth_finger_spends_the_sequence() {
    let mut s = Script::new();
    for id in 1..=4 {
        s.down(id, 100.0 * id as f32, 300.0);
    }
    s.wait(0.05);
    for id in 1..=4 {
        s.up(id, 100.0 * id as f32, 300.0);
    }
    s.wait(0.5);
    assert_eq!(s.take(), vec![]);
}

#[test]
fn tuning_is_clamped_like_the_ios_layer() {
    let tuning = Tuning::new(1.0, 5000.0, 10.0, 0.0, 200.0);
    assert_eq!(tuning.flick_points, 10.0);
    assert_eq!(tuning.flick_s, 0.6);
    assert_eq!(tuning.deep_factor, 6.0);
    assert_eq!(tuning.still_s, 0.2);
    assert_eq!(tuning.rotate_degrees, 90.0);
    assert_eq!(
        Tuning::default(),
        Tuning::new(30.0, 150.0, 3.0, 500.0, 30.0)
    );
}

#[test]
fn the_phase_reads_where_the_sequence_stands() {
    let mut s = Script::new();
    assert_eq!(s.r.phase_key(), "idle");
    s.down(1, 200.0, 400.0);
    assert_eq!(s.r.phase_key(), "deciding");
    s.drag(&[(1, (200.0, 400.0), (200.0, 440.0))], 0.08)
        .wait(0.3);
    assert_eq!(s.r.phase_key(), "brake");
    s.down(2, 300.0, 300.0);
    assert_eq!(s.r.phase_key(), "brake", "a second finger rides the pedal");
    s.up(1, 200.0, 440.0);
    assert_eq!(s.r.phase_key(), "spent");
    s.up(2, 300.0, 300.0);
    assert_eq!(s.r.phase_key(), "idle");
    s.down(1, 100.0, 100.0).down(2, 100.0, 200.0);
    assert_eq!(s.r.phase_key(), "two_fingers");
    s.wait(0.4);
    assert_eq!(s.r.phase_key(), "horn");
    s.up(1, 100.0, 100.0).up(2, 100.0, 200.0);
    s.down(1, 100.0, 100.0).wait(0.6);
    assert_eq!(s.r.phase_key(), "still_hold");
}

#[test]
fn feeding_events_is_the_same_as_calling_down_move_and_up() {
    use crate::touch::{FingerEvent, FingerKind};
    let mut r = Recognizer::default();
    let at = |kind, y: f32, t| FingerEvent {
        finger: 3,
        kind,
        x: 200.0,
        y,
        t,
    };
    r.feed(at(FingerKind::Down, 400.0, 1.0));
    r.feed(at(FingerKind::Move, 360.0, 1.05));
    r.feed(at(FingerKind::Up, 340.0, 1.08));
    assert_eq!(r.drain(), vec![SwipeUp]);
    r.feed(at(FingerKind::Down, 400.0, 2.0));
    r.feed(at(FingerKind::Move, 440.0, 2.05));
    r.tick(2.4);
    r.feed(at(FingerKind::Cancel, 0.0, 2.5));
    assert_eq!(r.drain(), vec![HoldLowerBegan, HoldEnded]);
}
