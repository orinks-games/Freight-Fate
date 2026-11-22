//! Settings help on a touch screen, for the rows whose help names keys.
//!
//! Each row keeps its desktop help as it was; [`MenuItem::touch_help`] puts
//! one of these in its place while prompts name gestures. A command the
//! player can move to another gesture is named the way it is set now, through
//! [`GameContext::touch_instruction`].

use super::*;
use crate::bindings::Action;

/// The driving assistance preset, with a flick for the lane change.
pub(super) const ASSIST_PRESET: &str = "Realistic turns on every assist except facility \
     stopping assistance, sets descent speed control to Light, and leaves lane keeping off. \
     Balanced adds partial lane keeping, Standard descent speed control, and facility stopping \
     assistance. All assists turns on every assist, sets descent speed control to Strong, and \
     sets lane keeping to full: the truck holds the lane, a flick left or right changes lanes, \
     and your destination exit is taken for you. Changing one assist makes this Custom. You \
     still choose routes and handle yards and docks. Presets never change pacing, hours rules, \
     transmission, weather, or hazards.";

/// The driving assist rows [`assist`] has a touch help for.
pub(super) const ASSISTS_WITH_TOUCH_HELP: [&str; 3] =
    ["pedal_latch", "curve_callouts", "speed_keeper"];

/// A driving assist row's touch help, for the rows whose help names a key.
pub(super) fn assist(field: &str, ctx: &GameContext) -> Option<String> {
    Some(match field {
        "pedal_latch" => "Brake briefly, lift, then swipe down and hold again for half a \
             second. A click means the brake now holds by itself. Brake again or accelerate \
             to release it. The gas never latches. Presets never change this."
            .to_string(),
        "curve_callouts" => {
            let upcoming = ctx.touch_instruction(Action::Upcoming);
            format!(
                "Bends that demand slowing are called before they arrive, like Sharp left, \
                 half a mile, advise 35. Bends you are already slow enough for stay silent. \
                 {upcoming} to list the next few either way. Presets never change this."
            )
        }
        "speed_keeper" => {
            let cruise = ctx.touch_instruction(Action::Cruise);
            format!(
                "In low-speed zones, like facility roads, gates, and construction zones, \
                 cruise can hold your current speed. {cruise} to set it. It hands back to \
                 normal cruise on open roads. It eases off early for the next turn or the next \
                 lower limit. Braking cancels it. Presets never change this."
            )
        }
        _ => return None,
    })
}

/// Lane keeping, with the flicks that change lanes on full. `lean` is what
/// leans and which way to steer, as the Audio rows have it.
pub(super) fn lane_keeping(lean: &str) -> String {
    format!(
        "How much of the steering the truck does. Full holds the lane, leaves a lane that a \
         work zone closes, and takes your exits and open weigh stations without a signal, \
         including the destination exit. On full, flick left or right to change lanes. \
         Partial follows bends but lets the truck drift, and lane changes and speed are yours. \
         Off drifts like a real truck. Bends are yours unless curve assistance is on, and every \
         exit needs a signal and the exit lane. On partial or off, {lean}. Road noise sits \
         where you are in the lane. At the lane's edge you hear a stutter as a tire touches the \
         rumble strip, a buzz when it is fully on it, and gravel off the pavement. The Steering \
         guide and Lane guide sound rows, under Audio, change which way to steer and what \
         leans. Realistic sets this off, Balanced partial, All assists full."
    )
}

/// Hours of service planning hints.
pub(super) fn hos_hints(ctx: &GameContext) -> String {
    let rest = ctx.touch_instruction(Action::Rest);
    format!(
        "Early advice to stop for a break or sleep while you still have time, plus the last \
         stop you can legally reach. {rest} to plan the recommended stop while rolling. Do it \
         again to cancel. With Driving speech on Standard, one hint comes before the next \
         hours warning. On Quiet or Urgent only, there is no hint. It also warns about chain \
         laws. At the dispatch board, it warns when the load crosses a road that needs chains \
         and the terminal garage can still sell you a set. On the road, it warns when a posted \
         chain law covers your truck. It never buys anything. Hours warnings and readouts \
         work whether this is on or off."
    )
}

/// Lane and edge cue volume: the lane locator without its key.
pub(super) const CUE_VOLUME: &str = "How loud the lane and edge cues are next to everything \
     else: the rumble strip, the gravel off the pavement, the lane locator, and the transverse \
     strips before a dangerous bend. It adjusts only these cues, on top of Gameplay cues \
     volume. Quieter sits under the engine, standard matches it, louder cuts through.";

/// Music seed.
pub(super) const MUSIC_SEED: &str = "Double tap to type a seed, a whole number. Flick up or \
     down to roll a new one. Every synthesized piece changes with it.";

/// Driving hints.
pub(super) const DRIVING_HINTS: &str = "On, driving lines name the gesture for the next step, \
     like Double tap to signal for the exit. Off, they leave the gesture out. Driving school \
     always names it.";

/// Roadside chatter.
pub(super) const CHATTER: &str = "Extra callouts between directions: parks, rivers, mountain \
     passes, museums and billboards. Flick up to turn all on, or down to turn all off. The \
     switches below pick each kind. Safety and navigation are never affected. Town names are \
     the Place callouts row below.";

/// A settings row with a touch help that reads the bindings as they stand.
pub(super) fn with_touch(row: Row, touch: impl Fn(&GameContext) -> String + 'static) -> Row {
    row.touch_help(Label::dynamic(move |_s, ctx| touch(ctx)))
}
