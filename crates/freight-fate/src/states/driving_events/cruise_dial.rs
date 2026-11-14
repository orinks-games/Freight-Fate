//! The cruise dial's answers, as the driving state speaks them: the one way
//! the dial's speech machine (`ff_core::sim::cruise_dial`) moves, the line it
//! holds while the dial is still turning, and the tick that lets it go.

use ff_core::sim::cruise_dial::{DialCue, DialEvent};
use ff_core::speech_text::SpokenMessage;

use crate::app::{GameContext, Say};
use crate::states::driving::DrivingState;

impl DrivingState {
    /// The only way `self.dial` moves.
    pub fn step_dial(&mut self, event: DialEvent) -> Option<DialCue> {
        let (next, cue) = self.dial.next(event);
        self.dial = next;
        cue
    }

    /// The dial's answer to one press: spoken now when the dial was still,
    /// otherwise kept as the newest line until the dial settles.
    pub fn say_dial(&mut self, ctx: &mut GameContext, message: SpokenMessage, say: Say) {
        self.dial_line = Some((message, say));
        if self.step_dial(DialEvent::Step).is_some() {
            self.speak_dial_line(ctx);
        }
    }

    /// One frame of real time for the dial: once it has been still long
    /// enough, the line it landed on is spoken.
    pub fn settle_dial(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.step_dial(DialEvent::Tick { dt }).is_some() {
            self.speak_dial_line(ctx);
        }
    }

    fn speak_dial_line(&mut self, ctx: &mut GameContext) {
        if let Some((message, say)) = self.dial_line.take() {
            ctx.say_with(message, say);
        }
    }
}
