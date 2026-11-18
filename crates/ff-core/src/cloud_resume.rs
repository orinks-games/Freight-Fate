//! Decisions and lifecycle for opt-in cross-device career resume.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    NotTracked,
    UpToDate,
    PullCloud { revision: i64 },
    Conflict,
}

pub fn decide(synced: Option<i64>, dirty: bool, remote: Option<i64>) -> Decision {
    match (synced, remote) {
        (Some(local), Some(remote)) if remote > local => {
            if dirty {
                Decision::Conflict
            } else {
                Decision::PullCloud { revision: remote }
            }
        }
        (Some(_), Some(_)) => Decision::UpToDate,
        _ => Decision::NotTracked,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CheckState {
    #[default]
    Off,
    Idle,
    Checking,
    Cancelling,
    Done {
        pulled: usize,
        conflicts: usize,
    },
    Failed,
}

#[derive(Clone, Copy, Debug)]
pub enum Event {
    Configure { enabled: bool },
    Enter,
    Completed { pulled: usize, conflicts: usize },
    Failed,
}

impl CheckState {
    pub fn next(self, event: Event) -> Self {
        match (self, event) {
            (Self::Checking, Event::Configure { enabled: false }) => Self::Cancelling,
            (Self::Cancelling, Event::Completed { .. } | Event::Failed) => Self::Off,
            (Self::Cancelling, _) => self,
            (_, Event::Configure { enabled: false }) => Self::Off,
            (Self::Off, Event::Configure { enabled: true }) => Self::Idle,
            (Self::Idle | Self::Done { .. } | Self::Failed, Event::Enter) => Self::Checking,
            (Self::Checking, Event::Completed { pulled, conflicts }) => {
                Self::Done { pulled, conflicts }
            }
            (Self::Checking, Event::Failed) => Self::Failed,
            _ => self,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_table() {
        for (synced, dirty, remote, expected) in [
            (None, false, Some(2), Decision::NotTracked),
            (None, true, Some(2), Decision::NotTracked),
            (Some(1), false, None, Decision::NotTracked),
            (None, false, None, Decision::NotTracked),
            (Some(2), false, Some(2), Decision::UpToDate),
            (Some(2), true, Some(2), Decision::UpToDate),
            (Some(3), false, Some(2), Decision::UpToDate),
            (Some(1), false, Some(2), Decision::PullCloud { revision: 2 }),
            (Some(1), true, Some(2), Decision::Conflict),
        ] {
            assert_eq!(decide(synced, dirty, remote), expected);
        }
    }

    #[test]
    fn transition_table() {
        use CheckState::*;
        for (state, event, expected) in [
            (Off, Event::Enter, Off),
            (Off, Event::Configure { enabled: true }, Idle),
            (Idle, Event::Enter, Checking),
            (Checking, Event::Enter, Checking),
            (
                Checking,
                Event::Completed {
                    pulled: 2,
                    conflicts: 1,
                },
                Done {
                    pulled: 2,
                    conflicts: 1,
                },
            ),
            (Checking, Event::Failed, Failed),
            (Failed, Event::Enter, Checking),
            (
                Done {
                    pulled: 0,
                    conflicts: 0,
                },
                Event::Enter,
                Checking,
            ),
            (Checking, Event::Configure { enabled: false }, Cancelling),
            (Cancelling, Event::Enter, Cancelling),
            (
                Cancelling,
                Event::Completed {
                    pulled: 0,
                    conflicts: 0,
                },
                Off,
            ),
            (
                Off,
                Event::Completed {
                    pulled: 1,
                    conflicts: 0,
                },
                Off,
            ),
            (Checking, Event::Configure { enabled: true }, Checking),
        ] {
            assert_eq!(state.next(event), expected);
        }
    }
}
