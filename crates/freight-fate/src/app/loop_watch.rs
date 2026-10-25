//! A watch on the game loop from a thread of its own.
//!
//! A player whose game stops answering cannot tell us where it stopped, and
//! a frozen game loop cannot write that down itself. The loop marks each
//! part of a frame as it enters it; this thread checks the marks twice a
//! second and writes to the log when one part has held the loop longer than
//! [`STALL_AFTER`], and again when the loop moves on. The log is then the
//! report: which part of the frame, after which event, for how long.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How long one part of a frame may hold the loop before it is logged.
/// Windows marks a window "Not Responding" after five seconds without
/// reading its messages, so this matches what the player sees.
pub const STALL_AFTER: Duration = Duration::from_secs(5);

/// How often the watch thread looks.
const LOOK_EVERY: Duration = Duration::from_millis(500);

/// The part of a frame the loop is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Phase {
    /// Reading the window's events from SDL.
    Events = 0,
    /// Handling a key, controller or other input event.
    Input = 1,
    /// Handling the window losing focus.
    FocusLost = 2,
    /// Handling the window getting focus back.
    FocusGained = 3,
    /// The per-frame update: speech, audio, services, the screen's update.
    Update = 4,
    /// Drawing the window.
    Render = 5,
    /// Between frames: the frame-rate sleep and the input policy.
    BetweenFrames = 6,
}

impl Phase {
    fn from_u8(value: u8) -> Self {
        match value {
            0 => Self::Events,
            1 => Self::Input,
            2 => Self::FocusLost,
            3 => Self::FocusGained,
            4 => Self::Update,
            5 => Self::Render,
            _ => Self::BetweenFrames,
        }
    }

    /// The phase as the log writes it.
    pub fn words(self) -> &'static str {
        match self {
            Self::Events => "reading window events",
            Self::Input => "handling input",
            Self::FocusLost => "handling the window losing focus",
            Self::FocusGained => "handling the window regaining focus",
            Self::Update => "the frame update",
            Self::Render => "drawing the window",
            Self::BetweenFrames => "between frames",
        }
    }
}

/// What the watch thread found.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Report {
    /// The loop has been in `phase` for `held` without moving.
    Stuck { phase: Phase, held: Duration },
    /// The loop moved on after at least `held` in `phase`.
    Moving { phase: Phase, held: Duration },
}

fn log_report(report: Report) {
    match report {
        Report::Stuck { phase, held } => log::warn!(
            "game loop stuck for {:.0} s in {} (the window reads as not responding)",
            held.as_secs_f64(),
            phase.words()
        ),
        Report::Moving { phase, held } => log::warn!(
            "game loop moving again after about {:.0} s in {}",
            held.as_secs_f64(),
            phase.words()
        ),
    }
}

struct Shared {
    start: Instant,
    /// Milliseconds after `start` of the last mark.
    marked_ms: AtomicU64,
    phase: AtomicU8,
    stop: AtomicBool,
}

impl Shared {
    fn now_ms(&self) -> u64 {
        u64::try_from(self.start.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

/// The game loop's half of the watch: [`LoopWatch::mark`] at each part of
/// the frame, [`LoopWatch::shutdown`] when the loop ends.
pub struct LoopWatch {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl LoopWatch {
    /// Start the watch thread, logging what it finds. `None` when the
    /// thread cannot be spawned; the game runs unwatched.
    pub fn start() -> Option<Self> {
        Self::start_with(STALL_AFTER, log_report)
    }

    /// [`LoopWatch::start`] with the threshold and the reporter chosen.
    pub fn start_with(
        stall_after: Duration,
        mut report: impl FnMut(Report) + Send + 'static,
    ) -> Option<Self> {
        let shared = Arc::new(Shared {
            start: Instant::now(),
            marked_ms: AtomicU64::new(0),
            phase: AtomicU8::new(Phase::BetweenFrames as u8),
            stop: AtomicBool::new(false),
        });
        let watched = Arc::clone(&shared);
        let look = LOOK_EVERY
            .min(stall_after / 2)
            .max(Duration::from_millis(1));
        let thread = std::thread::Builder::new()
            .name("loop-watch".into())
            .spawn(move || {
                // The mark the current stall was reported against.
                let mut stuck_at: Option<(u64, Phase, Duration)> = None;
                while !watched.stop.load(Ordering::Acquire) {
                    std::thread::sleep(look);
                    let marked = watched.marked_ms.load(Ordering::Acquire);
                    let phase = Phase::from_u8(watched.phase.load(Ordering::Acquire));
                    let held = Duration::from_millis(watched.now_ms().saturating_sub(marked));
                    match stuck_at {
                        Some((at, phase, held)) if at != marked => {
                            report(Report::Moving { phase, held });
                            stuck_at = None;
                        }
                        Some((at, phase, _)) => stuck_at = Some((at, phase, held)),
                        None if held >= stall_after && !watched.stop.load(Ordering::Acquire) => {
                            report(Report::Stuck { phase, held });
                            stuck_at = Some((marked, phase, held));
                        }
                        None => {}
                    }
                }
            })
            .ok()?;
        Some(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// The loop is entering `phase`.
    pub fn mark(&self, phase: Phase) {
        self.shared.phase.store(phase as u8, Ordering::Release);
        self.shared
            .marked_ms
            .store(self.shared.now_ms(), Ordering::Release);
    }

    /// Stop watching. The thread wakes at most [`LOOK_EVERY`] later, so the
    /// join is bounded by that. Safe to call more than once.
    pub fn shutdown(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for LoopWatch {
    /// Signal only: the thread ends on its next look, without a join here.
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn watch(stall_after_ms: u64) -> (LoopWatch, mpsc::Receiver<Report>) {
        let (tx, rx) = mpsc::channel();
        let watch = LoopWatch::start_with(Duration::from_millis(stall_after_ms), move |r| {
            let _ = tx.send(r);
        })
        .expect("watch thread");
        (watch, rx)
    }

    #[test]
    fn a_held_phase_is_reported_once_then_its_recovery() {
        let (mut watch, rx) = watch(40);
        watch.mark(Phase::FocusGained);
        let stuck = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("stuck report");
        assert!(
            matches!(stuck, Report::Stuck { phase: Phase::FocusGained, held } if held >= Duration::from_millis(40)),
            "{stuck:?}"
        );
        // Still held: no second report.
        assert!(rx.recv_timeout(Duration::from_millis(120)).is_err());
        watch.mark(Phase::Update);
        let moving = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("moving report");
        assert!(
            matches!(moving, Report::Moving { phase: Phase::FocusGained, held } if held >= Duration::from_millis(40)),
            "{moving:?}"
        );
        watch.shutdown();
    }

    #[test]
    fn a_loop_that_keeps_marking_is_never_reported() {
        let (mut watch, rx) = watch(200);
        let until = Instant::now() + Duration::from_millis(600);
        while Instant::now() < until {
            watch.mark(Phase::Update);
            std::thread::sleep(Duration::from_millis(10));
        }
        watch.shutdown();
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn shutdown_twice_is_harmless() {
        let (mut watch, _rx) = watch(1000);
        watch.shutdown();
        watch.shutdown();
    }
}
