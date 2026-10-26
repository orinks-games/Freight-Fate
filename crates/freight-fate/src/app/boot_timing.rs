//! Where a launch and a quit actually spend their time.
//!
//! Freight Fate boots into a menu a player cannot see, so "it is taking a
//! while" is never a thing they can diagnose and rarely a thing they even
//! report. The only way anyone finds a stall is from the session log, and a
//! log that says what loaded but never says how long it took cannot show one.
//!
//! Each seam of the launch (and of the quit, which is the half that has bitten
//! us) calls [`mark`]; it writes one INFO line naming the phase, the
//! milliseconds that phase took, and the milliseconds since the process
//! started. Read down the `phase:` lines in `logs/game.log` and the slow step
//! is the one with the big number.
//!
//! The clock starts in `main`, before anything else runs, so the first mark
//! also charges process creation and dynamic linking to the launch rather than
//! quietly dropping them.

use std::sync::Mutex;
use std::time::Instant;

/// Process start, and the end of the phase most recently marked.
static CLOCK: Mutex<Option<(Instant, Instant)>> = Mutex::new(None);

/// Start the phase clock. Called once, first thing in `main`; a second call
/// is ignored so a test binary or an embedded harness cannot restart it
/// halfway through a run.
pub fn start() {
    let mut guard = CLOCK.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        let now = Instant::now();
        *guard = Some((now, now));
    }
}

/// Close off a phase: log what it cost and how far into the run we are.
///
/// A no-op until [`start`] has been called, so nothing but the real binary
/// pays for this and the test suite's many `App`s stay silent.
pub fn mark(phase: &str) {
    let Some((begun, previous)) = ({
        let mut guard = CLOCK.lock().unwrap_or_else(|e| e.into_inner());
        match guard.as_mut() {
            Some((begun, last)) => {
                let now = Instant::now();
                let previous = std::mem::replace(last, now);
                Some((*begun, previous))
            }
            None => None,
        }
    }) else {
        return;
    };
    let now = Instant::now();
    log::info!(
        "phase: {phase} {} ms (elapsed {} ms)",
        now.duration_since(previous).as_millis(),
        now.duration_since(begun).as_millis(),
    );
    // Force the line out before the next phase can hang: a Mac Keychain
    // prompt after "world" left game.log with no further marks (issue 266).
    log::logger().flush();
}
