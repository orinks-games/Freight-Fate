//! What a touch gesture is for, and what a double tap answers at the wheel.
//!
//! Two small machines the iPhone and iPad game used to keep implicitly.
//!
//! [`TouchMode`] is which kind of screen is on top. Every screen reports
//! one, and the app routes each gesture by it alone: the drive and the
//! Driving School drive are both [`TouchMode::Wheel`], so a lesson gets the
//! same deep swipe, still hold and parking brake release as a delivery. It
//! used to be read off the screen's type, and the lesson fell through.
//!
//! [`RoadOffer`] is what the road is asking the driver for right now, which
//! is what a double tap at the wheel answers. It is derived each time from
//! what the drive already keeps (the facility gate, a trooper's pull-over,
//! an exit inside its window), so it has no flag of its own to go stale.

/// Which kind of screen a gesture lands on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TouchMode {
    /// A menu, reader or prompt: gestures are the keys a keyboard player
    /// would press.
    #[default]
    Menu,
    /// A text field: the on-screen keyboard types, and a three-finger tap
    /// reads the field back.
    Field,
    /// At the wheel, in a delivery or a Driving School lesson.
    Wheel,
    /// Practice gestures: every gesture is named and nothing moves.
    Practice,
}

impl TouchMode {
    pub const ALL: [TouchMode; 4] = [
        TouchMode::Menu,
        TouchMode::Field,
        TouchMode::Wheel,
        TouchMode::Practice,
    ];

    /// The name the agent server's `status` reports.
    pub fn key(self) -> &'static str {
        match self {
            TouchMode::Menu => "menu",
            TouchMode::Field => "field",
            TouchMode::Wheel => "wheel",
            TouchMode::Practice => "practice",
        }
    }
}

/// What a double tap at the wheel answers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RoadOffer {
    /// Nothing is asking. A double tap runs the exit control, which says
    /// there is no exit to signal for.
    #[default]
    None,
    /// An exit or stop is inside the exit window, or its signal is on.
    Exit,
    /// A trooper is asking the truck to pull over.
    PullOver,
    /// The trip has ended at a facility gate, which is waiting to open.
    Arrival,
    /// Stopped on a stop's ramp, at its end or on a scale: a double tap
    /// opens the stop or checks in, as the rest stop command does.
    Stop,
}

/// The facts [`RoadOffer::from_road`] reads, as the drive keeps them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RoadFacts {
    /// The facility gate is asking (the trip ended there, nothing open).
    pub gate_asking: bool,
    /// A trooper's pull-over is under way.
    pub pull_over: bool,
    /// An exit or stop the take-exit control would act on.
    pub exit_in_window: bool,
    /// Stopped on a stop's ramp where the rest stop command opens the stop
    /// or checks in at the scale.
    pub stop_waiting: bool,
}

impl RoadOffer {
    /// What the road is asking for. The gate outranks a trooper, which
    /// outranks a stop the truck is waiting at, which outranks an exit: a
    /// trip that has ended has no exit left to take, and a pull-over is the
    /// exit control's first answer anyway.
    pub fn from_road(facts: RoadFacts) -> RoadOffer {
        if facts.gate_asking {
            RoadOffer::Arrival
        } else if facts.pull_over {
            RoadOffer::PullOver
        } else if facts.stop_waiting {
            RoadOffer::Stop
        } else if facts.exit_in_window {
            RoadOffer::Exit
        } else {
            RoadOffer::None
        }
    }

    /// The name the agent server's `status` reports.
    pub fn key(self) -> &'static str {
        match self {
            RoadOffer::None => "none",
            RoadOffer::Exit => "exit",
            RoadOffer::PullOver => "pull_over",
            RoadOffer::Arrival => "arrival",
            RoadOffer::Stop => "stop",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_combination_of_facts_has_one_offer() {
        // The first fact that holds, in the order gate, pull-over, stop,
        // exit, decides; with none, nothing is asking.
        for bits in 0..16u8 {
            let facts = RoadFacts {
                gate_asking: bits & 8 != 0,
                pull_over: bits & 4 != 0,
                stop_waiting: bits & 2 != 0,
                exit_in_window: bits & 1 != 0,
            };
            let expected = if facts.gate_asking {
                RoadOffer::Arrival
            } else if facts.pull_over {
                RoadOffer::PullOver
            } else if facts.stop_waiting {
                RoadOffer::Stop
            } else if facts.exit_in_window {
                RoadOffer::Exit
            } else {
                RoadOffer::None
            };
            assert_eq!(RoadOffer::from_road(facts), expected, "{facts:?}");
        }
        // The table the order was chosen by.
        let stop_only = RoadFacts {
            stop_waiting: true,
            ..RoadFacts::default()
        };
        assert_eq!(RoadOffer::from_road(stop_only), RoadOffer::Stop);
        let stop_and_exit = RoadFacts {
            exit_in_window: true,
            ..stop_only
        };
        assert_eq!(RoadOffer::from_road(stop_and_exit), RoadOffer::Stop);
        let trooper = RoadFacts {
            pull_over: true,
            ..stop_only
        };
        assert_eq!(RoadOffer::from_road(trooper), RoadOffer::PullOver);
    }

    #[test]
    fn status_names_are_distinct() {
        let mut modes: Vec<&str> = TouchMode::ALL.iter().map(|m| m.key()).collect();
        modes.dedup();
        assert_eq!(modes.len(), TouchMode::ALL.len());
        assert_eq!(TouchMode::default(), TouchMode::Menu);
        assert_eq!(RoadOffer::default().key(), "none");
        let offers = [
            RoadOffer::None,
            RoadOffer::Exit,
            RoadOffer::PullOver,
            RoadOffer::Arrival,
            RoadOffer::Stop,
        ];
        let mut keys: Vec<&str> = offers.iter().map(|o| o.key()).collect();
        keys.sort_unstable();
        keys.dedup();
        assert_eq!(keys.len(), offers.len());
    }
}
