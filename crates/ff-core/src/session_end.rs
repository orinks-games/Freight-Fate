//! Windows session-ending queries can be cancelled before confirmation.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowsSession {
    Running,
    Queried,
    Ending,
}

#[derive(Clone, Copy, Debug)]
pub enum SessionEvent {
    Query,
    End(bool),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionAction {
    Allow,
    Cancel,
    End,
    None,
}

impl WindowsSession {
    pub fn next(self, event: SessionEvent) -> (Self, SessionAction) {
        match (self, event) {
            (Self::Ending, SessionEvent::Query) => (self, SessionAction::Allow),
            (Self::Ending, SessionEvent::End(_)) => (self, SessionAction::None),
            (_, SessionEvent::Query) => (Self::Queried, SessionAction::Allow),
            (_, SessionEvent::End(false)) => (Self::Running, SessionAction::Cancel),
            (_, SessionEvent::End(true)) => (Self::Ending, SessionAction::End),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{SessionAction as A, SessionEvent as E, WindowsSession as S};

    #[test]
    fn queries_can_be_cancelled_but_confirmation_ends_only_once() {
        let cases = [
            (S::Running, E::Query, S::Queried, A::Allow),
            (S::Queried, E::Query, S::Queried, A::Allow),
            (S::Queried, E::End(false), S::Running, A::Cancel),
            (S::Running, E::End(false), S::Running, A::Cancel),
            (S::Running, E::End(true), S::Ending, A::End),
            (S::Queried, E::End(true), S::Ending, A::End),
            (S::Ending, E::Query, S::Ending, A::Allow),
            (S::Ending, E::End(true), S::Ending, A::None),
            (S::Ending, E::End(false), S::Ending, A::None),
        ];
        for (state, event, next, action) in cases {
            assert_eq!(state.next(event), (next, action), "{state:?}, {event:?}");
        }
    }
}
