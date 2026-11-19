//! The geometry under the gesture recognizer: points, the four
//! directions, and the measures the multi-finger gestures take.

use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Point {
    pub(super) x: f32,
    pub(super) y: f32,
}

impl Point {
    pub(super) fn minus(self, other: Point) -> (f32, f32) {
        (self.x - other.x, self.y - other.y)
    }

    pub(super) fn distance(self, other: Point) -> f32 {
        let (dx, dy) = self.minus(other);
        dx.hypot(dy)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// The dominant direction of a movement; screen y grows downwards.
pub(super) fn direction(dx: f32, dy: f32) -> Direction {
    if dx.abs() > dy.abs() {
        if dx > 0.0 {
            Direction::Right
        } else {
            Direction::Left
        }
    } else if dy > 0.0 {
        Direction::Down
    } else {
        Direction::Up
    }
}

/// How far `at` is from `start` along `direction`; screen y grows
/// downwards.
pub(super) fn along(direction: Direction, start: Point, at: Point) -> f32 {
    let (dx, dy) = at.minus(start);
    match direction {
        Direction::Up => -dy,
        Direction::Down => dy,
        Direction::Left => -dx,
        Direction::Right => dx,
    }
}

pub(super) fn centroid(fingers: &BTreeMap<i64, Point>) -> Point {
    let count = fingers.len().max(1) as f32;
    let (x, y) = fingers
        .values()
        .fold((0.0, 0.0), |(x, y), p| (x + p.x, y + p.y));
    Point {
        x: x / count,
        y: y / count,
    }
}

/// The angle of the line from the first finger to the second, in degrees.
pub(super) fn pair_angle(fingers: &BTreeMap<i64, Point>) -> Option<f32> {
    let mut points = fingers.values();
    let (a, b) = (points.next()?, points.next()?);
    let (dx, dy) = b.minus(*a);
    Some(dy.atan2(dx).to_degrees())
}

/// An angle difference folded into -180..=180.
pub(super) fn wrap_degrees(mut degrees: f32) -> f32 {
    while degrees > 180.0 {
        degrees -= 360.0;
    }
    while degrees < -180.0 {
        degrees += 360.0;
    }
    degrees
}
