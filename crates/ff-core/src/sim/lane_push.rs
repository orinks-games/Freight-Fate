//! One lane change per push of the controller's left stick or a tilt of the
//! phone, with lane keeping on full.
//!
//! Lane keeping on full holds the lane, so an analog input does not steer.
//! Pushed past half way it asks for one lane change that way, as a tap of an
//! arrow key does, and it has to come back inside a quarter before it asks
//! again, so a stick or a phone held over changes one lane, not every lane.
//! The stick and tilt share the one value [`LanePush::next`] reads: whichever
//! of them is steering this frame.

/// How far over the input goes to ask for a lane change.
pub const LANE_PUSH: f64 = 0.5;
/// How near the centre it must return before the next one.
pub const LANE_RESET: f64 = 0.25;

/// Which way a push asked for a lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaneSide {
    Left,
    Right,
}

/// Where the analog input stands between lane changes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LanePush {
    /// Back near the centre: the next push past half way asks.
    #[default]
    Centred,
    /// A push left asked for its lane; the input has not come back yet.
    AskedLeft,
    /// The same to the right.
    AskedRight,
}

impl LanePush {
    /// The one place the push moves: this frame's analog value, from -1
    /// (full left) to 1 (full right). Returns the new state and the lane
    /// change to make, if this frame asked for one.
    pub fn next(self, value: f64) -> (LanePush, Option<LaneSide>) {
        if value.abs() < LANE_RESET {
            return (LanePush::Centred, None);
        }
        if value.abs() < LANE_PUSH || self != LanePush::Centred {
            return (self, None);
        }
        if value < 0.0 {
            (LanePush::AskedLeft, Some(LaneSide::Left))
        } else {
            (LanePush::AskedRight, Some(LaneSide::Right))
        }
    }

    /// The name the agent server's `status` reports.
    pub fn key(self) -> &'static str {
        match self {
            LanePush::Centred => "centred",
            LanePush::AskedLeft => "asked_left",
            LanePush::AskedRight => "asked_right",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use LanePush::*;
    use LaneSide::*;

    #[test]
    fn the_transition_table() {
        let table: &[(LanePush, f64, LanePush, Option<LaneSide>)] = &[
            (Centred, 0.0, Centred, None),
            (Centred, 0.3, Centred, None),
            (Centred, -0.49, Centred, None),
            (Centred, 0.5, AskedRight, Some(Right)),
            (Centred, -0.5, AskedLeft, Some(Left)),
            (Centred, 1.0, AskedRight, Some(Right)),
            (AskedRight, 1.0, AskedRight, None),
            (AskedRight, 0.3, AskedRight, None),
            (AskedRight, 0.24, Centred, None),
            // A jump across without a frame near the centre asks nothing.
            (AskedRight, -0.8, AskedRight, None),
            (AskedLeft, -0.6, AskedLeft, None),
            (AskedLeft, 0.0, Centred, None),
            (AskedLeft, -0.2, Centred, None),
        ];
        for &(from, value, to, side) in table {
            assert_eq!(from.next(value), (to, side), "{from:?} at {value}");
        }
    }

    #[test]
    fn a_held_push_asks_once() {
        let mut push = LanePush::default();
        let mut asked = 0;
        for _ in 0..100 {
            let (next, side) = push.next(0.9);
            push = next;
            asked += usize::from(side.is_some());
        }
        assert_eq!(asked, 1);
    }
}
