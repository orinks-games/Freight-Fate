//! The gesture recognizer for touch screens without UIKit (Android).
//!
//! iOS recognizes gestures with UIKit's recognizers (`ios/ff_touch.m`); this
//! is the same set, rule for rule, over raw finger events, so an Android
//! player meets the iPhone's gestures exactly: the core rule (a flick is one
//! step, a swipe and hold is continuous), the deep swipe, the still hold,
//! the second finger while a pedal is held, the two- and three-finger taps
//! and flicks, the two-finger scrub, the horn hold and the key turn. The
//! thresholds are the iOS ones and the same settings tune them.
//!
//! Positions are in points (density-independent pixels on Android: 1/160 of
//! an inch, near enough an iPhone point), times in seconds on one clock.
//! Nothing here touches a platform, so every rule is tested on the desktop.
//!
//! One touch sequence, from the first finger down to the last finger up, is
//! one [`Phase`] machine. UIKit decides between recognizers by having one
//! wait for another to fail; here the phase decides, and every transition
//! happens in [`Recognizer::down`], [`Recognizer::moved`],
//! [`Recognizer::up`], [`Recognizer::cancel`] or [`Recognizer::tick`].

use std::collections::BTreeMap;

use super::{Gesture, MAX_TRAVEL_POINTS};

mod geometry;
use geometry::{along, centroid, direction, pair_angle, wrap_degrees, Direction, Point};

/// How long a still pair of fingers waits before it is the horn.
const HOLD_S: f64 = 0.35;
/// How long a lifted tap waits for its partner before it counts as one tap.
const DOUBLE_TAP_S: f64 = 0.28;
/// The longest a finger may stay down and still be a tap.
const TAP_MAX_S: f64 = 0.4;
/// How far a holding finger may wander and still be holding still.
const HOLD_SLOP: f32 = 12.0;
/// How far a tapping finger may wander and still be a tap.
const TAP_SLOP: f32 = 16.0;
/// A finger still travelling this fraction of a flick per half window when
/// the flick window ends is mid-flick, not holding...
const FLICK_MOVING_FRACTION: f32 = 0.4;
/// ...for up to this many more half windows.
const FLICK_EXTENSIONS: u8 = 3;
/// The two-finger scrub: this many turns of the fingers' sideways travel,
/// each leg at least `SCRUB_LEG` long, all within `SCRUB_S` of the first
/// touch; a turn counts once the fingers come back `SCRUB_TURN` from the
/// leg's furthest point. Decided when the fingers lift.
const SCRUB_REVERSALS: u8 = 2;
const SCRUB_LEG: f32 = 20.0;
const SCRUB_TURN: f32 = 8.0;
const SCRUB_S: f64 = 1.0;
/// A multi-finger flick must cover its distance this soon after the
/// fingers settle; slower travel is no flick.
const MULTI_SWIPE_S: f64 = 0.5;

/// The player's thresholds, clamped as the iOS layer clamps them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tuning {
    pub flick_points: f32,
    pub flick_s: f64,
    pub deep_factor: f32,
    pub still_s: f64,
    pub rotate_degrees: f32,
}

impl Default for Tuning {
    fn default() -> Self {
        Self::new(30.0, 150.0, 3.0, 500.0, 30.0)
    }
}

impl Tuning {
    /// From the settings' units (points, milliseconds, a factor, degrees).
    pub fn new(
        flick_points: f64,
        flick_ms: f64,
        deep_factor: f64,
        still_ms: f64,
        rotate_degrees: f64,
    ) -> Self {
        Self {
            flick_points: flick_points.clamp(10.0, 120.0) as f32,
            flick_s: (flick_ms / 1000.0).clamp(0.05, 0.6),
            deep_factor: deep_factor.clamp(1.5, 6.0) as f32,
            still_s: (still_ms / 1000.0).clamp(0.2, 2.0),
            rotate_degrees: rotate_degrees.clamp(10.0, 90.0) as f32,
        }
    }
}

/// What a single finger's hold is doing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Hold {
    Gas,
    Brake,
    Left,
    Right,
    /// The deep swipe down: emergency or parking brake.
    Deep,
    /// Held still: straighten the wheel.
    Still,
}

impl Hold {
    fn began(self) -> Gesture {
        match self {
            Hold::Gas => Gesture::HoldUpperBegan,
            Hold::Brake => Gesture::HoldLowerBegan,
            Hold::Left => Gesture::HoldLeftBegan,
            Hold::Right => Gesture::HoldRightBegan,
            Hold::Deep => Gesture::DeepSwipeDownBegan,
            Hold::Still => Gesture::StillHoldBegan,
        }
    }

    /// The direction a hold's length is measured along; a still hold has
    /// none.
    fn direction(self) -> Option<Direction> {
        match self {
            Hold::Gas => Some(Direction::Up),
            Hold::Brake | Hold::Deep => Some(Direction::Down),
            Hold::Left => Some(Direction::Left),
            Hold::Right => Some(Direction::Right),
            Hold::Still => None,
        }
    }

    /// A pedal takes a second finger's gestures; nothing else does.
    fn pedal(self) -> Option<Pedal> {
        match self {
            Hold::Gas => Some(Pedal::Gas),
            Hold::Brake => Some(Pedal::Brake),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pedal {
    Gas,
    Brake,
}

impl Pedal {
    fn tap(self) -> Gesture {
        match self {
            Pedal::Gas => Gesture::UpperHoldTap,
            Pedal::Brake => Gesture::LowerHoldTap,
        }
    }

    fn double_tap(self) -> Gesture {
        match self {
            Pedal::Gas => Gesture::UpperHoldDoubleTap,
            Pedal::Brake => Gesture::LowerHoldDoubleTap,
        }
    }

    fn swipe(self, direction: Direction) -> Gesture {
        match (self, direction) {
            (Pedal::Gas, Direction::Up) => Gesture::UpperHoldSwipeUp,
            (Pedal::Gas, Direction::Down) => Gesture::UpperHoldSwipeDown,
            (Pedal::Gas, Direction::Left) => Gesture::UpperHoldSwipeLeft,
            (Pedal::Gas, Direction::Right) => Gesture::UpperHoldSwipeRight,
            (Pedal::Brake, Direction::Up) => Gesture::LowerHoldSwipeUp,
            (Pedal::Brake, Direction::Down) => Gesture::LowerHoldSwipeDown,
            (Pedal::Brake, Direction::Left) => Gesture::LowerHoldSwipeLeft,
            (Pedal::Brake, Direction::Right) => Gesture::LowerHoldSwipeRight,
        }
    }
}

/// A one-finger stroke that has crossed the flick distance and has the
/// flick window to lift in.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Crossing {
    direction: Direction,
    /// Where the finger was when the current window began.
    sample: Point,
    deadline: f64,
    extensions: u8,
    /// The furthest the finger has gone along `direction`, in points.
    travel: f32,
}

/// A tap that lifted and is waiting for a partner.
#[derive(Clone, Copy, Debug, PartialEq)]
struct PendingTap {
    fingers: u8,
    deadline: f64,
}

/// A second finger on the screen while a pedal is held.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Second {
    id: i64,
    start: Point,
    at: Point,
    began: f64,
    direction: Option<Direction>,
    /// When a sideways second stroke becomes steering.
    steer_deadline: Option<f64>,
    steering: bool,
}

/// The sideways travel of two fingers, for the scrub.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
struct Scrub {
    leg_start: f32,
    extreme: f32,
    sign: i8,
    reversals: u8,
}

impl Scrub {
    fn new(x: f32) -> Self {
        Self {
            leg_start: x,
            extreme: x,
            ..Self::default()
        }
    }

    fn track(&mut self, x: f32) {
        if self.sign == 0 {
            self.sign = if x > self.leg_start {
                1
            } else if x < self.leg_start {
                -1
            } else {
                0
            };
            self.extreme = x;
            return;
        }
        let sign = f32::from(self.sign);
        if sign * (x - self.extreme) > 0.0 {
            self.extreme = x;
        } else if sign * (self.extreme - x) >= SCRUB_TURN {
            if (self.extreme - self.leg_start).abs() >= SCRUB_LEG {
                self.reversals = self.reversals.saturating_add(1);
            }
            self.leg_start = self.extreme;
            self.extreme = x;
            self.sign = -self.sign;
        }
    }
}

/// Two or three fingers, deciding what they are.
#[derive(Clone, Debug, PartialEq)]
struct Multi {
    /// The most fingers down at once in this sequence.
    fingers: u8,
    began: f64,
    /// When the finger count last rose: multi-finger flicks and the horn
    /// measure from here.
    settled: f64,
    starts: BTreeMap<i64, Point>,
    /// Every finger stayed within the tap distance.
    tap_still: bool,
    /// Every finger stayed within the hold distance (for the horn).
    hold_still: bool,
    horn: bool,
    horn_cancelled: bool,
    /// The two fingers' starting angle, in degrees.
    angle_start: Option<f32>,
    rotated: bool,
    turning: bool,
    scrub: Option<Scrub>,
    /// A sideways two-finger flick, sent at the lift unless it was a scrub.
    side_flick: Option<Direction>,
    /// A flick was sent: the rest of the sequence is spent.
    flicked: bool,
}

/// One touch sequence's state.
#[derive(Clone, Debug, PartialEq)]
enum Phase {
    /// No finger down.
    Idle,
    /// One finger down, nothing decided yet.
    One {
        id: i64,
        start: Point,
        at: Point,
        began: f64,
        crossing: Option<Crossing>,
        still_deadline: Option<f64>,
    },
    /// One finger holding: a pedal, a steering hold, the deep swipe or the
    /// still hold, with a pedal's second finger.
    Holding {
        id: i64,
        start: Point,
        hold: Hold,
        /// The furthest the finger has gone along the hold's direction.
        travel: f32,
        second: Option<Second>,
        second_tap: Option<(Pedal, f64)>,
    },
    /// Two or three fingers.
    Multi(Box<Multi>),
    /// The sequence is decided or spent; wait for every finger to lift.
    Spent,
}

/// Raw finger events in, [`Gesture`]s out.
#[derive(Clone, Debug)]
pub struct Recognizer {
    tuning: Tuning,
    phase: Phase,
    /// Fingers down now, by id, with where they are.
    fingers: BTreeMap<i64, Point>,
    /// A lifted tap waiting for its partner; it outlives the sequence.
    pending_tap: Option<PendingTap>,
    /// Practice gestures is open: each directional hold reports its length
    /// as it lifts.
    report_travel: bool,
    out: Vec<Gesture>,
}

impl Default for Recognizer {
    fn default() -> Self {
        Self::new(Tuning::default())
    }
}

impl Recognizer {
    pub fn new(tuning: Tuning) -> Self {
        Self {
            tuning,
            phase: Phase::Idle,
            fingers: BTreeMap::new(),
            pending_tap: None,
            report_travel: false,
            out: Vec::new(),
        }
    }

    pub fn set_tuning(&mut self, tuning: Tuning) {
        self.tuning = tuning;
    }

    /// Report how far each directional hold went (Practice gestures), as
    /// `ios/ff_touch.m` does.
    pub fn set_report_travel(&mut self, on: bool) {
        self.report_travel = on;
    }

    /// A hold's length as it lifts, when Practice gestures asks for it.
    fn report_travel(&mut self, hold: Hold, travel: f32) {
        if !self.report_travel || hold.direction().is_none() {
            return;
        }
        let points = travel.round().clamp(0.0, f32::from(MAX_TRAVEL_POINTS));
        // In range by the clamp just above.
        self.emit(Gesture::Travel(points as u16));
    }

    /// The gestures recognized since the last call, oldest first.
    pub fn drain(&mut self) -> Vec<Gesture> {
        std::mem::take(&mut self.out)
    }

    /// Whether a timer is waiting, so the caller knows to keep ticking.
    pub fn busy(&self) -> bool {
        self.pending_tap.is_some() || !matches!(self.phase, Phase::Idle)
    }

    fn emit(&mut self, gesture: Gesture) {
        self.out.push(gesture);
    }

    /// Send a waiting one- or two-finger tap as the single tap it was.
    fn flush_pending_tap(&mut self) {
        if let Some(tap) = self.pending_tap.take() {
            self.emit(if tap.fingers == 1 {
                Gesture::Tap
            } else {
                Gesture::TwoFingerTap
            });
        }
    }

    /// A finger touched the screen.
    pub fn down(&mut self, id: i64, x: f32, y: f32, t: f64) {
        self.tick(t);
        let at = Point { x, y };
        self.fingers.insert(id, at);
        let phase = std::mem::replace(&mut self.phase, Phase::Spent);
        self.phase = match phase {
            Phase::Idle => Phase::One {
                id,
                start: at,
                at,
                began: t,
                crossing: None,
                still_deadline: Some(t + self.tuning.still_s),
            },
            // Two fingers down together are a two-finger gesture, not a
            // stroke, until the stroke has become a hold.
            Phase::One { began, .. } => self.multi_from_fingers(began, t),
            Phase::Holding {
                id: first,
                start,
                hold,
                travel,
                second: None,
                second_tap,
            } if hold.pedal().is_some() => Phase::Holding {
                id: first,
                start,
                hold,
                travel,
                second: Some(Second {
                    id,
                    start: at,
                    at,
                    began: t,
                    direction: None,
                    steer_deadline: None,
                    steering: false,
                }),
                second_tap,
            },
            Phase::Multi(multi) if !multi.horn && !multi.rotated && !multi.flicked => {
                if self.fingers.len() > 3 {
                    Phase::Spent
                } else {
                    self.multi_from_fingers(multi.began, t)
                }
            }
            other => other,
        };
    }

    fn multi_from_fingers(&mut self, began: f64, t: f64) -> Phase {
        let starts = self.fingers.clone();
        let fingers = starts.len().min(3) as u8;
        let angle_start = (fingers == 2).then(|| pair_angle(&starts)).flatten();
        let scrub = (fingers == 2).then(|| Scrub::new(centroid(&starts).x));
        Phase::Multi(Box::new(Multi {
            fingers,
            began,
            settled: t,
            starts,
            tap_still: true,
            hold_still: true,
            horn: false,
            horn_cancelled: false,
            angle_start,
            rotated: false,
            turning: false,
            scrub,
            side_flick: None,
            flicked: false,
        }))
    }

    /// A finger moved.
    pub fn moved(&mut self, id: i64, x: f32, y: f32, t: f64) {
        self.tick(t);
        let at = Point { x, y };
        if let Some(finger) = self.fingers.get_mut(&id) {
            *finger = at;
        } else {
            return;
        }
        let tuning = self.tuning;
        let mut phase = std::mem::replace(&mut self.phase, Phase::Spent);
        match &mut phase {
            Phase::One {
                id: first,
                start,
                at: now,
                crossing,
                still_deadline,
                ..
            } if *first == id => {
                *now = at;
                let distance = at.distance(*start);
                if still_deadline.is_some() && distance > HOLD_SLOP {
                    *still_deadline = None;
                }
                if crossing.is_none() && distance >= tuning.flick_points {
                    let (dx, dy) = at.minus(*start);
                    *crossing = Some(Crossing {
                        direction: direction(dx, dy),
                        sample: at,
                        deadline: t + tuning.flick_s,
                        extensions: 0,
                        travel: 0.0,
                    });
                }
                if let Some(crossing) = crossing.as_mut() {
                    crossing.travel = crossing.travel.max(along(crossing.direction, *start, at));
                }
            }
            Phase::Holding {
                id: first,
                start,
                hold,
                travel,
                second,
                second_tap,
            } => {
                if *first == id {
                    if let Some(direction) = hold.direction() {
                        *travel = travel.max(along(direction, *start, at));
                    }
                    // Only a stroke that has outlasted the flick window (the
                    // brake hold has begun) becomes the deep swipe; a fast
                    // long flick down stays a flick.
                    let dy = at.y - start.y;
                    if *hold == Hold::Brake && dy >= tuning.flick_points * tuning.deep_factor {
                        if let Some((pedal, _)) = second_tap.take() {
                            self.out.push(pedal.tap());
                        }
                        end_second(second, &mut self.out);
                        *hold = Hold::Deep;
                        self.out.push(Gesture::DeepSwipeDownBegan);
                    }
                } else if let Some(second) = second.as_mut().filter(|s| s.id == id) {
                    second.at = at;
                    if hold.pedal().is_some() && second.direction.is_none() {
                        let (dx, dy) = at.minus(second.start);
                        if dx.hypot(dy) >= tuning.flick_points {
                            let dir = direction(dx, dy);
                            second.direction = Some(dir);
                            if matches!(dir, Direction::Left | Direction::Right) {
                                second.steer_deadline = Some(t + tuning.flick_s);
                            }
                        }
                    }
                }
            }
            Phase::Multi(multi) => {
                let fingers = self.fingers.clone();
                self.multi_moved(multi, &fingers, t);
            }
            _ => {}
        }
        self.phase = phase;
    }

    fn multi_moved(&mut self, multi: &mut Multi, fingers: &BTreeMap<i64, Point>, t: f64) {
        let tuning = self.tuning;
        for (id, at) in fingers {
            if let Some(start) = multi.starts.get(id) {
                let distance = at.distance(*start);
                if distance > TAP_SLOP {
                    multi.tap_still = false;
                }
                if distance > HOLD_SLOP {
                    multi.hold_still = false;
                }
            }
        }
        // After a flick or a key turn the sequence is spent; a horn still
        // watches for the turn that cancels it.
        if multi.flicked || multi.rotated {
            return;
        }
        if multi.fingers == 2 {
            if let Some(scrub) = multi.scrub.as_mut() {
                scrub.track(centroid(fingers).x);
            }
            if let (Some(start), Some(now)) = (multi.angle_start, pair_angle(fingers)) {
                let turned = wrap_degrees(now - start);
                if turned.abs() >= tuning.rotate_degrees / 3.0 {
                    multi.turning = true;
                    if multi.horn && !multi.horn_cancelled {
                        multi.horn_cancelled = true;
                        self.emit(Gesture::HoldEnded);
                    }
                }
                if !multi.rotated && turned.abs() >= tuning.rotate_degrees {
                    multi.rotated = true;
                    self.flush_pending_tap();
                    // Screen y grows downwards, so a growing angle turns
                    // clockwise: the key turned right.
                    self.emit(if turned > 0.0 {
                        Gesture::RotateRight
                    } else {
                        Gesture::RotateLeft
                    });
                    return;
                }
            }
        }
        if multi.horn || multi.turning || t - multi.settled > MULTI_SWIPE_S {
            return;
        }
        let start = centroid(&multi.starts);
        let now = centroid(fingers);
        let (dx, dy) = now.minus(start);
        if dx.hypot(dy) < tuning.flick_points {
            return;
        }
        let dir = direction(dx, dy);
        let gesture = match (multi.fingers, dir) {
            (2, Direction::Up) => Gesture::TwoFingerSwipeUp,
            (2, Direction::Down) => Gesture::TwoFingerSwipeDown,
            // The message flicks wait for the scrub to fail, at the lift.
            (2, side) => {
                multi.side_flick.get_or_insert(side);
                return;
            }
            (_, Direction::Up) => Gesture::ThreeFingerSwipeUp,
            (_, Direction::Down) => Gesture::ThreeFingerSwipeDown,
            (_, Direction::Left) => Gesture::ThreeFingerSwipeLeft,
            (_, Direction::Right) => Gesture::ThreeFingerSwipeRight,
        };
        multi.flicked = true;
        self.flush_pending_tap();
        self.emit(gesture);
    }

    /// A finger lifted.
    pub fn up(&mut self, id: i64, x: f32, y: f32, t: f64) {
        self.moved(id, x, y, t);
        if self.fingers.remove(&id).is_none() {
            return;
        }
        let tuning = self.tuning;
        let phase = std::mem::replace(&mut self.phase, Phase::Spent);
        self.phase = match phase {
            Phase::One {
                id: first,
                start,
                began,
                crossing,
                ..
            } if first == id => {
                let at = Point { x, y };
                if let Some(crossing) = crossing {
                    self.flush_pending_tap();
                    self.emit(match crossing.direction {
                        Direction::Up => Gesture::SwipeUp,
                        Direction::Down => Gesture::SwipeDown,
                        Direction::Left => Gesture::SwipeLeft,
                        Direction::Right => Gesture::SwipeRight,
                    });
                } else if at.distance(start) <= TAP_SLOP && t - began <= TAP_MAX_S {
                    self.tapped(1, t);
                } else {
                    self.flush_pending_tap();
                }
                Phase::Idle
            }
            Phase::Holding {
                id: first,
                hold,
                travel,
                mut second,
                mut second_tap,
                start,
            } => {
                if first == id {
                    if let Some((pedal, _)) = second_tap.take() {
                        self.emit(pedal.tap());
                    }
                    end_second(&mut second, &mut self.out);
                    self.report_travel(hold, travel);
                    self.emit(Gesture::HoldEnded);
                    self.after_lift()
                } else {
                    if let Some(lifted) = second.filter(|s| s.id == id) {
                        second = None;
                        self.second_lifted(lifted, hold, &mut second_tap, t, tuning);
                    }
                    Phase::Holding {
                        id: first,
                        start,
                        hold,
                        travel,
                        second,
                        second_tap,
                    }
                }
            }
            Phase::Multi(multi) => self.multi_lifted(*multi, t),
            Phase::Spent => self.after_lift(),
            other => other,
        };
    }

    fn after_lift(&self) -> Phase {
        if self.fingers.is_empty() {
            Phase::Idle
        } else {
            Phase::Spent
        }
    }

    fn second_lifted(
        &mut self,
        second: Second,
        hold: Hold,
        second_tap: &mut Option<(Pedal, f64)>,
        t: f64,
        tuning: Tuning,
    ) {
        if second.steering {
            self.emit(Gesture::SecondSteerEnded);
            return;
        }
        let Some(pedal) = hold.pedal() else {
            return;
        };
        let (dx, dy) = second.at.minus(second.start);
        let distance = dx.hypot(dy);
        if second.direction.is_some() || distance >= tuning.flick_points {
            if let Some((waiting, _)) = second_tap.take() {
                self.emit(waiting.tap());
            }
            self.emit(pedal.swipe(direction(dx, dy)));
        } else if distance <= TAP_SLOP && t - second.began <= TAP_MAX_S {
            if second_tap.take().is_some() {
                self.emit(pedal.double_tap());
            } else {
                *second_tap = Some((pedal, t + DOUBLE_TAP_S));
            }
        }
    }

    fn multi_lifted(&mut self, multi: Multi, t: f64) -> Phase {
        if multi.horn {
            if !multi.horn_cancelled {
                self.emit(Gesture::HoldEnded);
            }
            return self.after_lift();
        }
        // Decide at the first lift: a scrub, a side flick, or a tap.
        let decided = multi.flicked || multi.rotated;
        if !decided {
            let scrubbed = multi.fingers == 2
                && multi
                    .scrub
                    .is_some_and(|scrub| scrub.reversals >= SCRUB_REVERSALS)
                && t - multi.began <= SCRUB_S;
            if scrubbed {
                self.flush_pending_tap();
                self.emit(Gesture::Escape);
            } else if let Some(side) = multi.side_flick {
                self.flush_pending_tap();
                self.emit(if side == Direction::Left {
                    Gesture::TwoFingerSwipeLeft
                } else {
                    Gesture::TwoFingerSwipeRight
                });
            } else if multi.tap_still && !multi.turning && t - multi.began <= TAP_MAX_S {
                if multi.fingers == 3 {
                    self.flush_pending_tap();
                    self.emit(Gesture::ThreeFingerTap);
                } else {
                    self.tapped(2, t);
                }
            } else {
                self.flush_pending_tap();
            }
        }
        self.after_lift()
    }

    /// A tap of `fingers` lifted: the second of a pair is a double tap,
    /// otherwise it waits for a partner.
    fn tapped(&mut self, fingers: u8, t: f64) {
        match self.pending_tap.take() {
            Some(waiting) if waiting.fingers == fingers => self.emit(if fingers == 1 {
                Gesture::DoubleTap
            } else {
                Gesture::MagicTap
            }),
            other => {
                self.pending_tap = other;
                self.flush_pending_tap();
                self.pending_tap = Some(PendingTap {
                    fingers,
                    deadline: t + DOUBLE_TAP_S,
                });
            }
        }
    }

    /// The touches were taken away (the app lost focus): every hold ends.
    pub fn cancel(&mut self) {
        let phase = std::mem::replace(&mut self.phase, Phase::Idle);
        match phase {
            Phase::Holding { mut second, .. } => {
                end_second(&mut second, &mut self.out);
                self.emit(Gesture::HoldEnded);
            }
            Phase::Multi(multi) if multi.horn && !multi.horn_cancelled => {
                self.emit(Gesture::HoldEnded);
            }
            _ => {}
        }
        self.fingers.clear();
        self.pending_tap = None;
    }

    /// Let time pass: the flick window, the still hold, the horn, a waiting
    /// tap and a second finger's steering all run on it.
    pub fn tick(&mut self, t: f64) {
        if let Some(tap) = self.pending_tap {
            // A finger already down again may still be the partner.
            if t >= tap.deadline && matches!(self.phase, Phase::Idle) {
                self.flush_pending_tap();
            }
        }
        let tuning = self.tuning;
        let phase = std::mem::replace(&mut self.phase, Phase::Spent);
        self.phase = match phase {
            Phase::One {
                id,
                start,
                at,
                began,
                crossing: Some(crossing),
                still_deadline,
            } if t >= crossing.deadline => {
                // Judge by whether the finger is still travelling, not by
                // the clock alone: only a stroke that has slowed or stopped
                // becomes a hold.
                let moved = at.distance(crossing.sample);
                if crossing.extensions < FLICK_EXTENSIONS
                    && moved >= tuning.flick_points * FLICK_MOVING_FRACTION
                {
                    Phase::One {
                        id,
                        start,
                        at,
                        began,
                        crossing: Some(Crossing {
                            sample: at,
                            deadline: t + tuning.flick_s / 2.0,
                            extensions: crossing.extensions + 1,
                            ..crossing
                        }),
                        still_deadline,
                    }
                } else {
                    let hold = match crossing.direction {
                        Direction::Up => Hold::Gas,
                        Direction::Down => Hold::Brake,
                        Direction::Left => Hold::Left,
                        Direction::Right => Hold::Right,
                    };
                    self.flush_pending_tap();
                    self.emit(hold.began());
                    Phase::Holding {
                        id,
                        start,
                        hold,
                        travel: crossing.travel,
                        second: None,
                        second_tap: None,
                    }
                }
            }
            Phase::One {
                id,
                start,
                crossing: None,
                still_deadline: Some(deadline),
                ..
            } if t >= deadline => {
                self.flush_pending_tap();
                self.emit(Gesture::StillHoldBegan);
                Phase::Holding {
                    id,
                    start,
                    hold: Hold::Still,
                    travel: 0.0,
                    second: None,
                    second_tap: None,
                }
            }
            Phase::Holding {
                id,
                start,
                hold,
                travel,
                mut second,
                mut second_tap,
            } => {
                if let Some((pedal, deadline)) = second_tap {
                    if t >= deadline {
                        second_tap = None;
                        self.emit(pedal.tap());
                    }
                }
                if let Some(s) = second.as_mut() {
                    if s.steer_deadline.is_some_and(|deadline| t >= deadline) {
                        s.steer_deadline = None;
                        if let Some((pedal, _)) = second_tap.take() {
                            self.emit(pedal.tap());
                        }
                        s.steering = true;
                        self.emit(if s.direction == Some(Direction::Left) {
                            Gesture::SecondSteerLeftBegan
                        } else {
                            Gesture::SecondSteerRightBegan
                        });
                    }
                }
                Phase::Holding {
                    id,
                    start,
                    hold,
                    travel,
                    second,
                    second_tap,
                }
            }
            Phase::Multi(mut multi)
                if multi.fingers == 2
                    && !multi.horn
                    && !multi.rotated
                    && !multi.flicked
                    && multi.hold_still
                    && t - multi.settled >= HOLD_S =>
            {
                multi.horn = true;
                self.flush_pending_tap();
                self.emit(Gesture::HornHoldBegan);
                Phase::Multi(multi)
            }
            other => other,
        };
    }
}

/// Let go of a pedal's second finger, ending its steering if it steers.
fn end_second(second: &mut Option<Second>, out: &mut Vec<Gesture>) {
    if let Some(s) = second.take() {
        if s.steering {
            out.push(Gesture::SecondSteerEnded);
        }
    }
}

#[cfg(test)]
#[path = "recognizer_tests.rs"]
mod tests;
