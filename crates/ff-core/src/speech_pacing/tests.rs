//! Ported from the pure (no `App()`) tests of
//! `tests/test_event_speech_pacer.py` and `tests/test_driving_speech_ladder.py`.
use super::*;
use std::cell::Cell;
use std::rc::Rc;

/// A controllable clock: `clock.now.set(...)` to advance.
struct FakeClock {
    now: Rc<Cell<f64>>,
}

impl FakeClock {
    fn at(start: f64) -> Self {
        Self {
            now: Rc::new(Cell::new(start)),
        }
    }

    fn clock(&self) -> Clock {
        let now = Rc::clone(&self.now);
        Box::new(move || now.get())
    }

    fn advance(&self, seconds: f64) {
        self.now.set(self.now.get() + seconds);
    }
}

fn make_pacer() -> (EventSpeechPacer, FakeClock) {
    let clock = FakeClock::at(100.0);
    (EventSpeechPacer::with_clock(clock.clock()), clock)
}

fn flush(pacer: &mut EventSpeechPacer, text: &str) -> bool {
    pacer.should_flush(text, EventPriority::Ambient, None, None)
}

fn flush_at(pacer: &mut EventSpeechPacer, text: &str, priority: EventPriority) -> bool {
    pacer.should_flush(text, priority, None, None)
}

fn interrupt(pacer: &mut EventSpeechPacer, text: &str) -> Option<Cut> {
    pacer.note_interrupt(text, EventPriority::Critical, None, None)
}

fn cut(text: &str, priority: EventPriority) -> Option<Cut> {
    Some((text.to_string(), priority))
}

fn always(value: Rc<Cell<bool>>) -> Option<Valid> {
    Some(Box::new(move || value.get()))
}

// ~10 seconds at the default 13 chars per second
const LONG_LINE: &str = "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx";

#[test]
fn test_long_line_is_130_chars() {
    assert_eq!(LONG_LINE.len(), 130);
}

#[test]
fn test_quiet_channel_queues_normally() {
    let (mut pacer, _) = make_pacer();
    assert!(!flush(&mut pacer, "Slow down for the dock."));
}

#[test]
fn test_backlog_past_the_threshold_flushes() {
    let (mut pacer, _) = make_pacer();
    // First long line starts immediately; the second waits ~10s behind
    // it -- far past the 3-second staleness budget.
    assert!(!flush(&mut pacer, LONG_LINE));
    assert!(flush(&mut pacer, "At the dock."));
}

#[test]
fn test_flush_restarts_the_projection() {
    let (mut pacer, _) = make_pacer();
    flush(&mut pacer, LONG_LINE);
    assert!(flush(&mut pacer, "At the dock."));
    // The flush purged the channel: the very next line queues normally.
    assert!(!flush(&mut pacer, "Delivering."));
}

#[test]
fn test_interrupt_resets_to_truth() {
    let (mut pacer, _) = make_pacer();
    flush(&mut pacer, LONG_LINE);
    interrupt(&mut pacer, "Collision!");
    // The interrupting line purged the backlog; a short queued follow-up
    // starts right behind it, inside the staleness budget.
    assert!(!flush(&mut pacer, "Total damage 12 percent."));
}

#[test]
fn test_projection_expires_with_real_time() {
    let (mut pacer, clock) = make_pacer();
    flush(&mut pacer, LONG_LINE);
    clock.advance(30.0); // the voice long since finished speaking
    assert!(!flush(&mut pacer, "Exit ahead."));
}

#[test]
fn test_reset_clears_the_projection() {
    let (mut pacer, _) = make_pacer();
    flush(&mut pacer, LONG_LINE);
    pacer.reset();
    assert!(!flush(&mut pacer, "At the dock."));
}

const CHATTER: &str = "Rain easing off, roads still wet."; // ~2.4s estimated

#[test]
fn test_would_start_stale_is_a_pure_reading() {
    let (mut pacer, _) = make_pacer();
    flush(&mut pacer, LONG_LINE);
    assert!(pacer.would_start_stale(CHATTER, EventPriority::Ambient));
    // Pure: asking did not extend the projection or consume anything.
    assert!(pacer.would_start_stale(CHATTER, EventPriority::Ambient));
    let (mut quiet, _) = make_pacer();
    assert!(!quiet.would_start_stale(CHATTER, EventPriority::Ambient));
}

/// The first line back from a pause must purge and speak, never drop.
#[test]
fn test_the_post_pause_purge_is_not_read_as_staleness() {
    let (mut pacer, clock) = make_pacer();
    flush(&mut pacer, LONG_LINE);
    pacer.pause();
    clock.advance(45.0);
    assert!(!pacer.would_start_stale(CHATTER, EventPriority::Ambient));
    assert!(flush(&mut pacer, CHATTER));
}

#[test]
fn test_many_short_lines_stay_within_budget_then_flush() {
    let (mut pacer, _) = make_pacer();
    let line = "Passing the fuel island."; // ~2.3s estimated
    let verdicts: Vec<bool> = (0..4).map(|_| flush(&mut pacer, line)).collect();
    // The first few fit inside the budget; the backlog eventually crosses it.
    assert!(!verdicts[0]);
    assert!(verdicts[1..].contains(&true));
}

// -- one moment, said once (tester transcript, 2026-08-11) --------------
//
// A sideswipe arrived as three identical lines inside six tenths of a
// second, and a load's condition was read out unchanged every few
// seconds for the rest of the drive. The pacer now knows what the player
// has already heard.

const SIDESWIPE: &str = "You sideswiped a box truck in the right lane! The truck took damage, now 13 percent. Check your mirrors before moving over.";

#[test]
fn test_identical_line_inside_the_window_is_a_repeat() {
    let (mut pacer, clock) = make_pacer();
    assert!(!pacer.is_repeat(SIDESWIPE, None, false, None));
    pacer.note_spoken(SIDESWIPE, None);
    clock.advance(0.6); // the burst the tester heard
    assert!(pacer.is_repeat(SIDESWIPE, None, false, None));
}

#[test]
fn test_the_same_line_is_news_again_once_the_window_passes() {
    let (mut pacer, clock) = make_pacer();
    pacer.note_spoken(SIDESWIPE, None);
    clock.advance(EventSpeechPacer::REPEAT_WINDOW_S + 0.1);
    assert!(!pacer.is_repeat(SIDESWIPE, None, false, None));
}

#[test]
fn test_a_line_the_player_asked_for_is_never_a_repeat() {
    let (mut pacer, _) = make_pacer();
    pacer.note_spoken(SIDESWIPE, None);
    assert!(!pacer.is_repeat(SIDESWIPE, None, true, None));
}

/// A state of the world speaks when it starts and when it worsens.
#[test]
fn test_standing_condition_repeats_only_when_it_changes() {
    let (mut pacer, clock) = make_pacer();
    let at_45 = "The load has shifted hard and is badly damaged, 45 percent.";
    let at_60 = "The load has shifted hard and is badly damaged, 60 percent.";

    assert!(!pacer.is_repeat(at_45, Some("cargo_condition"), false, None));
    pacer.note_spoken(at_45, Some("cargo_condition"));

    // Minutes later, still the same load in the same state: nothing new to say.
    clock.advance(300.0);
    assert!(pacer.is_repeat(at_45, Some("cargo_condition"), false, None));

    // The damage has moved. That is news, and it speaks.
    assert!(!pacer.is_repeat(at_60, Some("cargo_condition"), false, None));
    pacer.note_spoken(at_60, Some("cargo_condition"));
    clock.advance(300.0);
    assert!(pacer.is_repeat(at_60, Some("cargo_condition"), false, None));
}

#[test]
fn test_a_cleared_condition_announces_itself_afresh() {
    let (mut pacer, clock) = make_pacer();
    let redline = "Redline. Engine wear 4 percent.";
    pacer.note_spoken(redline, Some("engine_redline"));
    clock.advance(300.0);
    assert!(pacer.is_repeat(redline, Some("engine_redline"), false, None));
    // The engine came off the limiter and went back on: a fresh event.
    pacer.forget_condition("engine_redline");
    assert!(!pacer.is_repeat(redline, Some("engine_redline"), false, None));
}

/// The pacer half of `test_raising_the_rung_still_speaks_an_active_silenced_condition`
/// (an `App()` test in the ladder file): a silenced occurrence marks its
/// own namespace and never the one `is_repeat` reads.
#[test]
fn test_a_silenced_occurrence_never_reads_as_heard() {
    let (mut pacer, clock) = make_pacer();
    let lockout = "Parking brake set. Press P to release it.";
    assert!(!pacer.is_silenced_repeat(lockout, Some("air_brake_lockout"), None));
    pacer.note_silenced(lockout, Some("air_brake_lockout"));
    clock.advance(60.0);
    // The earcon is deduped for the silenced branch...
    assert!(pacer.is_silenced_repeat(lockout, Some("air_brake_lockout"), None));
    // ...and the speaking path still finds the line unheard.
    assert!(!pacer.is_repeat(lockout, Some("air_brake_lockout"), false, None));
    // Clearing the condition clears both namespaces.
    pacer.forget_condition("air_brake_lockout");
    assert!(!pacer.is_silenced_repeat(lockout, Some("air_brake_lockout"), None));
}

// -- stepping off the road ----------------------------------------------

#[test]
fn test_pause_arms_a_purge_so_the_backlog_is_not_replayed() {
    let (mut pacer, clock) = make_pacer();
    flush(&mut pacer, LONG_LINE); // a line still speaking when the player pauses
    pacer.pause();
    clock.advance(45.0); // a while in the pause menu
                         // Back on the road: the first line purges the channel rather than
                         // falling in behind whatever the voice was still holding.
    assert!(flush(
        &mut pacer,
        "Speed limit reduced to 55 miles per hour."
    ));
}

#[test]
fn test_resume_purges_once_then_paces_normally() {
    let (mut pacer, _) = make_pacer();
    pacer.pause();
    pacer.resume();
    assert!(flush(&mut pacer, "Rest area in two miles."));
    // The purge is spent; the channel is trusted again from here.
    assert!(!flush(&mut pacer, "Weigh station ahead."));
}

// -- the stop the player planned ----------------------------------------

/// Tester Darren: planned stops get lost in the traffic chatter.
///
/// Two pacers in identical states, so the only thing under test is the
/// priority the line was submitted with.
#[test]
fn test_route_priority_will_not_wait_out_a_backlog_of_chatter() {
    let stop_line = "Planned stop, Iowa 80 Truckstop at Exit 284 in five miles.";

    let (mut ambient, _) = make_pacer();
    flush(&mut ambient, CHATTER);
    // Behind one piece of chatter, another informational line is content
    // to wait its turn -- nothing is lost by hearing it a few seconds later.
    assert!(!flush_at(&mut ambient, stop_line, EventPriority::Ambient));

    let (mut route, _) = make_pacer();
    flush(&mut route, CHATTER);
    // The same line as a planned stop has an exit to make, so it goes in
    // front of the chatter instead of behind it.
    assert!(flush_at(&mut route, stop_line, EventPriority::Route));
}

#[test]
fn test_route_priority_still_queues_behind_a_quiet_channel() {
    let (mut pacer, _) = make_pacer();
    // Nothing is speaking: a route line has no reason to cut anything off.
    assert!(!flush_at(
        &mut pacer,
        "Rest area in two miles.",
        EventPriority::Route
    ));
}

// -- the line the interrupt stepped on (tester report, 2026-08-12) -------
//
// A hazard, a curve call, or an info key landing while the voice was
// mid-way through "Open weigh station ahead" destroyed the announcement
// outright: the purge that delivers an interrupting line took the queue
// with it, and nothing gave the cut line back. A tester blew a weigh
// station that way. The pacer now hands the cut ROUTE or CRITICAL line
// back so it queues right behind the line that cut it -- safety line
// first, then the line it stepped on.

const STOP_LINE: &str = "Planned stop, Iowa 80 Truckstop at Exit 284 in five miles.";
const HAZARD: &str = "Hazard! Stopped traffic ahead.";

#[test]
fn test_interrupt_hands_back_a_cut_route_line() {
    let (mut pacer, _) = make_pacer();
    flush_at(&mut pacer, STOP_LINE, EventPriority::Route);
    assert_eq!(
        interrupt(&mut pacer, HAZARD),
        cut(STOP_LINE, EventPriority::Route)
    );
}

#[test]
fn test_a_route_line_that_finished_is_not_handed_back() {
    let (mut pacer, clock) = make_pacer();
    flush_at(&mut pacer, STOP_LINE, EventPriority::Route);
    clock.advance(30.0); // the voice long since read it out in full
    assert_eq!(interrupt(&mut pacer, HAZARD), None);
}

#[test]
fn test_ambient_chatter_is_never_handed_back() {
    let (mut pacer, _) = make_pacer();
    flush(&mut pacer, CHATTER); // AMBIENT: missing it costs the player nothing
    assert_eq!(interrupt(&mut pacer, HAZARD), None);
}

#[test]
fn test_a_critical_line_cut_by_another_critical_is_handed_back() {
    let (mut pacer, _) = make_pacer();
    let first = "Emergency vehicle approaching from behind. Move right.";
    assert_eq!(interrupt(&mut pacer, first), None); // quiet channel: nothing was cut
    assert_eq!(
        interrupt(&mut pacer, HAZARD),
        cut(first, EventPriority::Critical)
    );
}

/// The one-line ping-pong: A cutting A must not requeue A behind A.
#[test]
fn test_a_line_interrupting_itself_is_not_handed_back() {
    let (mut pacer, _) = make_pacer();
    flush_at(&mut pacer, STOP_LINE, EventPriority::Route);
    assert_eq!(interrupt(&mut pacer, STOP_LINE), None);
}

// -- how much of the cut line was heard (owner ruling, 2026-09-01) ------
//
// The agent drives that day heard every interrupt as a stutter: "Start
// on West 14th Avenue" twice, "Out of the gate and onto city streets"
// twice, a whole state-crossing welcome twice in a tester's log -- each
// a ROUTE line cut in its last words and then said again from the top.
// A cut early in the line still owes the player the instruction; past
// MOSTLY_HEARD_FRACTION of it the line is dropped and named in the
// transcript, and the message log keeps the words.

/// Advance the clock to this fraction of the line's estimated duration
/// after it started speaking (queued on a quiet channel, so it started
/// the moment it was submitted).
fn heard(clock: &FakeClock, text: &str, fraction: f64) {
    clock.advance(EventSpeechPacer::duration_s(text) * fraction);
}

#[test]
fn test_a_route_line_cut_early_is_still_handed_back() {
    let (mut pacer, clock) = make_pacer();
    flush_at(&mut pacer, STOP_LINE, EventPriority::Route);
    heard(&clock, STOP_LINE, 0.2);
    assert_eq!(
        interrupt(&mut pacer, HAZARD),
        cut(STOP_LINE, EventPriority::Route)
    );
    assert_eq!(pacer.take_mostly_heard(), None);
}

#[test]
fn test_a_route_line_cut_in_its_last_words_is_dropped_and_named() {
    let (mut pacer, clock) = make_pacer();
    flush_at(&mut pacer, STOP_LINE, EventPriority::Route);
    heard(&clock, STOP_LINE, 0.8);
    assert_eq!(
        interrupt(&mut pacer, HAZARD),
        None,
        "a line the player had mostly heard was said again from the top"
    );
    // Named once for the transcript, then the slot is empty.
    assert_eq!(pacer.take_mostly_heard(), Some(STOP_LINE.to_string()));
    assert_eq!(pacer.take_mostly_heard(), None);
}

#[test]
fn test_the_threshold_is_the_owner_set_half() {
    assert_eq!(EventSpeechPacer::MOSTLY_HEARD_FRACTION, 0.5);
    let (mut pacer, clock) = make_pacer();
    flush_at(&mut pacer, STOP_LINE, EventPriority::Route);
    heard(&clock, STOP_LINE, 0.45);
    assert_eq!(
        interrupt(&mut pacer, HAZARD),
        cut(STOP_LINE, EventPriority::Route)
    );
    let (mut pacer, clock) = make_pacer();
    flush_at(&mut pacer, STOP_LINE, EventPriority::Route);
    heard(&clock, STOP_LINE, 0.55);
    assert_eq!(interrupt(&mut pacer, HAZARD), None);
    assert_eq!(pacer.take_mostly_heard(), Some(STOP_LINE.to_string()));
}

/// A line still waiting behind a backlog has been heard for no time at
/// all, however long ago it was submitted: never dropped.
#[test]
fn test_a_line_cut_before_it_started_is_handed_back_whatever_its_age() {
    let (mut pacer, clock) = make_pacer();
    flush(&mut pacer, LONG_LINE); // ~10 s of chatter at the voice
    let warning = "Emergency vehicle approaching from behind. Move right.";
    pacer.note_queued(warning, EventPriority::Critical, None, None);
    clock.advance(8.0); // older than its own duration, and not yet begun
    assert_eq!(
        interrupt(&mut pacer, HAZARD),
        cut(warning, EventPriority::Critical)
    );
    assert_eq!(pacer.take_mostly_heard(), None);
}

/// The stale-flush path drops a mostly-heard safety call the same way,
/// rather than restarting it behind the flush -- when there is a stale
/// backlog behind it to flush at all.
#[test]
fn test_a_stale_flush_drops_a_safety_call_in_its_last_words() {
    let clock = FakeClock::at(0.0);
    let mut pacer = EventSpeechPacer::with_clock(clock.clock());
    let brake = "Brake now! Stopped traffic ahead in both lanes, and a trooper on the shoulder.";
    pacer.note_interrupt(brake, EventPriority::Critical, None, None);
    // Chatter queued behind it: a real backlog.
    pacer.note_queued(CHATTER, EventPriority::Ambient, None, None);
    heard(&clock, brake, 0.8);
    assert!(flush_at(
        &mut pacer,
        "Merge onto US-40 west toward Salt Lake City.",
        EventPriority::Route
    ));
    assert_eq!(pacer.take_flush_cut(), None);
    assert_eq!(pacer.take_mostly_heard(), Some(brake.to_string()));
}

/// With nothing queued behind it, a line still speaking is not a backlog.
/// "Light red." used to flush the take line in its last words and drop its
/// tail, and a route confirmation flushed it in its first words and had it
/// said again from the top ahead of itself -- "Exit speed 49. You take exit
/// 113" twice (agent drives, 2026-09-24). The new line waits its turn.
#[test]
fn test_a_route_line_waits_out_the_one_line_still_speaking() {
    for fraction in [0.1, 0.8] {
        let clock = FakeClock::at(0.0);
        let mut pacer = EventSpeechPacer::with_clock(clock.clock());
        let take = "Exit speed 49. You take exit 113 for Casa Grande. A quarter mile of ramp, \
                    traffic light at the end.";
        pacer.note_interrupt(take, EventPriority::Critical, None, None);
        heard(&clock, take, fraction);
        assert!(
            !flush_at(&mut pacer, "Light red.", EventPriority::Route),
            "at {fraction} the take line was cut for a line that could wait"
        );
        assert_eq!(pacer.take_flush_cut(), None, "at {fraction}");
        assert_eq!(pacer.take_mostly_heard(), None, "at {fraction}");
        // It starts where the take line ends, not after a replay of it.
        assert!(pacer.busy());
        heard(&clock, take, 1.0 - fraction);
        clock.advance(EventSpeechPacer::duration_s("Light red.") + 0.01);
        assert!(!pacer.busy(), "at {fraction}");
    }
}

/// A line the road has since made untrue is not worth waiting for: the
/// gap came, so "assistance is holding for your gap" is flushed and dropped,
/// never handed back to land right before "Gap in traffic" (agent drive,
/// yield at exit 255, 2026-09-24).
#[test]
fn test_a_superseded_line_is_flushed_and_never_handed_back() {
    let clock = FakeClock::at(0.0);
    let mut pacer = EventSpeechPacer::with_clock(clock.clock());
    let waiting = Rc::new(Cell::new(true));
    let hold = "Stopped at the yield. A car crossing from the left; assistance is holding for \
                your gap.";
    pacer.should_flush(
        hold,
        EventPriority::Route,
        None,
        always(Rc::clone(&waiting)),
    );
    clock.advance(0.1);
    waiting.set(false); // the gap came
    assert!(flush_at(
        &mut pacer,
        "Gap in traffic. Clear; pull ahead onto the streets.",
        EventPriority::Route
    ));
    assert_eq!(pacer.take_flush_cut(), None);
}

#[test]
fn test_the_hand_back_happens_at_most_once_per_cut() {
    let (mut pacer, _) = make_pacer();
    flush_at(&mut pacer, STOP_LINE, EventPriority::Route);
    assert_eq!(
        interrupt(&mut pacer, HAZARD),
        cut(STOP_LINE, EventPriority::Route)
    );
    // The slot emptied with the hand-back and now holds the hazard: a
    // further interrupt rescues the line it lands on, never the stop line
    // twice.
    assert_eq!(
        interrupt(&mut pacer, "Sharp curve ahead."),
        cut(HAZARD, EventPriority::Critical)
    );
}

/// The trooper-escalation loop from the 21 August build note: a rescued
/// line is requeued and re-protected, so a CHAIN of urgent lines used to
/// replay it after every one -- "Signal for the scale exit" spoke five
/// times. One rescue per line per window; the second cut drops it.
#[test]
fn test_a_rescued_line_is_not_rescued_again_by_the_next_cut() {
    let (mut pacer, _) = make_pacer();
    let escalations: Vec<String> = (1..6)
        .map(|n| format!("Failure to stop, warning {n}."))
        .collect();
    flush_at(&mut pacer, STOP_LINE, EventPriority::Route);
    let mut rescues = 0;
    for warning in &escalations {
        if let Some((text, priority)) = interrupt(&mut pacer, warning) {
            if text == STOP_LINE {
                rescues += 1;
                // the app requeues the rescue behind the warning,
                // re-protecting it
                pacer.note_queued(&text, priority, None, None);
            }
        }
    }
    assert_eq!(rescues, 1, "the stop line was replayed {rescues} times");
}

/// The cap is a window, not a life sentence: a genuinely new moment that
/// happens to use the same words is cut and rescued like any other.
#[test]
fn test_the_same_words_minutes_later_earn_a_fresh_rescue() {
    let (mut pacer, clock) = make_pacer();
    flush_at(&mut pacer, STOP_LINE, EventPriority::Route);
    let rescued = interrupt(&mut pacer, HAZARD);
    assert_eq!(rescued, cut(STOP_LINE, EventPriority::Route));
    let (text, priority) = rescued.unwrap();
    pacer.note_queued(&text, priority, None, None);
    clock.advance(EventSpeechPacer::RESCUE_ONCE_WINDOW_S + 1.0);
    flush_at(&mut pacer, STOP_LINE, EventPriority::Route);
    assert_eq!(
        interrupt(&mut pacer, HAZARD),
        cut(STOP_LINE, EventPriority::Route)
    );
}

/// It answers something the player did; it is not a warning to rescue.
///
/// Confirmations default to CRITICAL, so they used to qualify -- and then
/// the next interrupting line on the main channel handed the FINISHED
/// confirmation back to be requeued, where it resurfaced after, and could
/// bury, the line the player had actually just asked for. The adversarial
/// harness found it on settings_flips_mid_drive; pressed keys
/// interrupting again (2026-08-16) turned it from rare into every info
/// key.
#[test]
fn test_a_confirmation_never_takes_the_hand_back_slot() {
    let (mut pacer, _) = make_pacer();
    let confirmation = "Transmission changed to manual.";
    assert_eq!(
        pacer.note_interrupt(
            confirmation,
            EventPriority::Critical,
            Some(SpeechCategory::Confirmation),
            None
        ),
        None
    );
    // The S query that follows gets the channel to itself.
    assert_eq!(interrupt(&mut pacer, HAZARD), None);
}

/// The slot still does its job for the lines it was built for.
#[test]
fn test_a_warning_is_still_handed_back_after_the_confirmation_rule() {
    let (mut pacer, _) = make_pacer();
    flush_at(&mut pacer, STOP_LINE, EventPriority::Route);
    assert_eq!(
        interrupt(&mut pacer, HAZARD),
        cut(STOP_LINE, EventPriority::Route)
    );
}

/// The class promises a ROUTE or CRITICAL line still speaking is handed
/// back, never dropped. `note_interrupt` honoured that; `should_flush`
/// did not -- its purge cleared the protected slot outright.
///
/// An engine stall was stepped on exactly that way by the route-start
/// merge cue on the owner's Denver playtest: the stall spoke CRITICAL,
/// the merge cue flushed a moment later, and the stall was gone with no
/// requeue. It was latent until that cue stopped being AMBIENT, because
/// as chatter it had been dropped before ever reaching the flush.
///
/// Narrowly CRITICAL: a backlog of stale ROUTE announcements really does
/// describe road already driven, and rescuing those turned one flush
/// into a recital of everything it had purged.
#[test]
fn test_a_stale_flush_never_steps_on_a_safety_call() {
    let clock = FakeClock::at(0.0);
    let mut pacer = EventSpeechPacer::with_clock(clock.clock());

    // A safety call starts speaking, chatter queued behind it.
    pacer.note_interrupt(
        "Brake now! Stopped traffic ahead.",
        EventPriority::Critical,
        None,
        None,
    );
    pacer.note_queued(CHATTER, EventPriority::Ambient, None, None);
    // A route line arrives while it is still mid-sentence, far enough
    // behind the projection to flush.
    clock.advance(0.05);
    assert!(flush_at(
        &mut pacer,
        "Merge onto US-40 west toward Salt Lake City.",
        EventPriority::Route
    ));
    let cut = pacer.take_flush_cut();
    let cut = cut.expect("the safety call was purged with no requeue");
    assert_eq!(cut.0, "Brake now! Stopped traffic ahead.");
    // Collected once only.
    assert_eq!(pacer.take_flush_cut(), None);
}

/// The other half, and why the rescue is narrow. A route announcement
/// the player has been listening to for a while has said its piece and
/// describes road already driven; handing its tail back would perform
/// the very backlog the flush purged.
#[test]
fn test_a_stale_flush_still_discards_an_aged_route_backlog() {
    let clock = FakeClock::at(0.0);
    let mut pacer = EventSpeechPacer::with_clock(clock.clock());

    let stop = "Next stop in 5 miles: service plaza."; // ~3.2 s spoken
    pacer.note_queued(stop, EventPriority::Route, None, None);
    pacer.note_queued(CHATTER, EventPriority::Ambient, None, None); // and chatter behind it
                                                                    // Well past the pre-utterance pause: the voice has been reading this
                                                                    // line aloud, and it is still mid-sentence when the flush lands.
    clock.advance(2.0);
    assert!(
        flush_at(
            &mut pacer,
            "Zone ahead; speed limit 45.",
            EventPriority::Route
        ),
        "the backlog was not deep enough to flush"
    );
    assert_eq!(
        pacer.take_flush_cut(),
        None,
        "an aged route backlog was resurrected"
    );
}

/// The owner's 23 August drive, three route lines inside 37 ms: the
/// ramp-exit briefing was purged 22 ms into its own delivery -- by this
/// pacer's own duration model, before the voice had uttered a character
/// -- and the turn it named was never spoken at all. A line that young
/// is not a stale backlog; it is the same instant of road as the lines
/// after it, which queue behind it: nothing purged, nothing handed back to
/// be logged and submitted a second time (every burst of the live drive into
/// Abilene, 2026-09-24, and "Off the ramp" lost under the second flush).
#[test]
fn test_a_burst_of_route_lines_in_one_instant_queues_whole() {
    let clock = FakeClock::at(0.0);
    let mut pacer = EventSpeechPacer::with_clock(clock.clock());

    let briefing = "Off the ramp and onto city streets: start on unnamed \
                        public road. Then turn right now onto Halleck Street. \
                        1 mile to the facility gate.";
    pacer.note_queued(briefing, EventPriority::Route, None, None);
    // The next route lines land in the same frame and the one after.
    clock.advance(0.022);
    let zone = "Speed limit 25.";
    assert!(!flush_at(&mut pacer, zone, EventPriority::Route));
    clock.advance(0.015);
    assert!(!flush_at(
        &mut pacer,
        "Slow semi 1.7 miles ahead, at 42 miles per hour.",
        EventPriority::Route
    ));
    assert_eq!(pacer.take_flush_cut(), None, "a burst line was purged");
    // All of it is still to be spoken, in the order it came.
    heard(&clock, briefing, 1.0);
    heard(&clock, zone, 0.9);
    assert!(pacer.busy());
}

/// The same instant only: a backlog behind a line that has been speaking
/// past its pause is a backlog, and a route line still flushes it.
#[test]
fn test_a_backlog_older_than_the_pause_still_flushes() {
    let clock = FakeClock::at(0.0);
    let mut pacer = EventSpeechPacer::with_clock(clock.clock());
    let stop = "Next stop in 5 miles: service plaza.";
    pacer.note_queued(stop, EventPriority::Route, None, None);
    pacer.note_queued("Speed limit 45.", EventPriority::Route, None, None);
    clock.advance(EventSpeechPacer::BASE_UTTERANCE_S + 0.1);
    assert!(flush_at(
        &mut pacer,
        "Zone ahead; speed limit 45.",
        EventPriority::Route
    ));
}

/// A flush that hands a line back speaks it FIRST, so the line that flushed
/// has not started while the hand-back plays. It used to be dated as
/// speaking from the flush, so the next route line or warning dropped it as
/// heard before the player got a word of it.
#[test]
fn test_the_line_behind_a_hand_back_is_rescued_while_the_hand_back_plays() {
    let stop = "Next stop in 5 miles: service plaza.";
    let zone = "Zone ahead; speed limit 45.";
    let first_flush = || {
        let clock = FakeClock::at(0.0);
        let mut pacer = EventSpeechPacer::with_clock(clock.clock());
        pacer.note_queued(CHATTER, EventPriority::Ambient, None, None);
        pacer.note_queued(stop, EventPriority::Route, None, None);
        clock.advance(EventSpeechPacer::BASE_UTTERANCE_S + 0.1);
        assert!(flush_at(&mut pacer, zone, EventPriority::Route));
        // The stop line had not started, so it comes back and is said first.
        assert_eq!(pacer.take_flush_cut(), cut(stop, EventPriority::Route));
        pacer.note_ahead(stop);
        (pacer, clock)
    };

    // A second route line while the stop line is still playing.
    let (mut pacer, clock) = first_flush();
    clock.advance(1.0);
    assert!(flush_at(
        &mut pacer,
        "Speed limit 45.",
        EventPriority::Route
    ));
    assert_eq!(pacer.take_flush_cut(), cut(zone, EventPriority::Route));

    // A warning past half the zone line's length, still inside the stop line.
    let (mut pacer, clock) = first_flush();
    heard(&clock, zone, 0.6);
    assert_eq!(
        interrupt(&mut pacer, HAZARD),
        cut(zone, EventPriority::Route)
    );
    assert_eq!(pacer.take_mostly_heard(), None);
}

/// Darren and Jerry, 2026-08-21: the repeat the build note describes at
/// a scale happens in a work zone too.
///
/// The cap is keyed on the line's own words, so it was always going to
/// cover this -- but "was always going to" is not a test, and the report
/// named a place the suite had never driven. These are the real
/// work-zone lines, behind the run of urgent lines a busy taper produces.
#[test]
fn test_a_construction_zone_line_is_rescued_once_like_any_other() {
    let work_zone = "Work zone active. The right lane is closed; keep left and watch the barrels. Speed limit 45.";
    let urgent = [
        "Brake lights ahead!",
        "Cones in your lane!",
        "Flagger stopping traffic!",
        "Truck merging left!",
        "Barrels in the shoulder!",
    ];
    let (mut pacer, _) = make_pacer();
    flush_at(&mut pacer, work_zone, EventPriority::Route);
    let mut rescues = 0;
    for warning in urgent {
        if let Some((text, priority)) = interrupt(&mut pacer, warning) {
            if text == work_zone {
                rescues += 1;
                pacer.note_queued(&text, priority, None, None);
            }
        }
    }
    assert_eq!(
        rescues, 1,
        "the work zone line was replayed {rescues} times"
    );
}

/// The other half of a work zone: the taper's own merge instruction,
/// which is the line a driver can least afford to hear four times while
/// deciding which way to go.
#[test]
fn test_the_merge_taper_line_is_rescued_once_too() {
    let taper =
        "Reduced speed for construction. The right lane closes ahead; merge left now. Speed limit 55.";
    let (mut pacer, _) = make_pacer();
    flush_at(&mut pacer, taper, EventPriority::Route);
    let mut rescues = 0;
    for n in 1..5 {
        if let Some((text, priority)) = interrupt(&mut pacer, &format!("Hazard {n}!")) {
            if text == taper {
                rescues += 1;
                pacer.note_queued(&text, priority, None, None);
            }
        }
    }
    assert_eq!(rescues, 1, "the taper line was replayed {rescues} times");
}

/// A cut line comes back so it can finish -- but only while it is still
/// true. "Move right for the exit lane" handed back after the gore is
/// behind the truck instructs a maneuver that no longer exists, which is
/// the build note's own complaint about being told to signal for an exit
/// when there is no exit left to take.
#[test]
fn test_a_rescued_line_dies_when_its_moment_has_passed() {
    let exit_line = "Exit 14A, half a mile ahead. Move right for the exit lane.";
    let still_ahead = Rc::new(Cell::new(true));
    let (mut pacer, _) = make_pacer();
    flush_at(&mut pacer, exit_line, EventPriority::Route);
    pacer.track(
        exit_line,
        EventPriority::Route,
        None,
        always(Rc::clone(&still_ahead)),
    );
    // Cut while the exit is still ahead: handed back, as it should be.
    assert_eq!(
        interrupt(&mut pacer, "Brake lights ahead!"),
        cut(exit_line, EventPriority::Route)
    );

    // Now the truck is past it. The same cut must NOT bring it back.
    still_ahead.set(false);
    let (mut pacer2, _) = make_pacer();
    flush_at(&mut pacer2, exit_line, EventPriority::Route);
    pacer2.track(
        exit_line,
        EventPriority::Route,
        None,
        always(Rc::clone(&still_ahead)),
    );
    assert_eq!(interrupt(&mut pacer2, "Brake lights ahead!"), None);
}

/// Shane, 2026-08-21, on "Change lanes or brake! Retread debris from a
/// blown tire.": the line repeated two or three times.
///
/// A cut line is handed back so it finishes -- that is what rescued the
/// missing "you swerve around the brake lights". But a dodge call handed
/// back after the truck is clear tells the driver to swerve around
/// something that is no longer there. Same rule the scale and the
/// destination exit already carry: a rescued line has to still be true.
#[test]
fn test_a_hazard_call_does_not_come_back_once_the_hazard_is_clear() {
    let hazard_line = "Change lanes or brake! Retread debris from a blown tire. Left lane open.";
    let live = Rc::new(Cell::new(true));
    let (mut pacer, _) = make_pacer();
    flush_at(&mut pacer, hazard_line, EventPriority::Critical);
    pacer.track(
        hazard_line,
        EventPriority::Critical,
        None,
        always(Rc::clone(&live)),
    );
    // Still live: cut by something louder, handed back to finish.
    assert_eq!(
        interrupt(&mut pacer, "Deer in the road!"),
        cut(hazard_line, EventPriority::Critical)
    );

    // Cleared: the same cut must not bring the dodge call back.
    live.set(false);
    let (mut pacer2, _) = make_pacer();
    flush_at(&mut pacer2, hazard_line, EventPriority::Critical);
    pacer2.track(
        hazard_line,
        EventPriority::Critical,
        None,
        always(Rc::clone(&live)),
    );
    assert_eq!(interrupt(&mut pacer2, "Deer in the road!"), None);
}

/// Found by driving it, not by a report: `playtest_road --find scale`
/// requeued both of these.
///
/// "Signal and brake to a stop on the shoulder" handed back after the
/// truck IS stopped demands a pull-over that already happened, and its
/// escalation threatens spike strips and felony charges over a stop the
/// driver made. These carry the heaviest consequence of any line in the
/// game, so they are the last ones that should be able to speak out of
/// their moment.
#[test]
fn test_an_enforcement_stop_instruction_dies_once_the_truck_has_stopped() {
    let stop_call = "Scale bypass enforcement. Lights and siren behind you: signal with X and brake to a stop on the shoulder.";
    let final_call = "Final failure-to-stop warning. Brake to a full stop now or troopers will end the stop with spike strips and felony charges.";

    fn rescued(line: &str, stop_live: bool) -> Option<Cut> {
        let (mut pacer, _) = make_pacer();
        flush_at(&mut pacer, line, EventPriority::Critical);
        pacer.track(
            line,
            EventPriority::Critical,
            None,
            Some(Box::new(move || stop_live)),
        );
        interrupt(&mut pacer, "Deer in the road!")
    }

    for line in [stop_call, final_call] {
        // Stop still running: handed back so it finishes, which is the point.
        assert_eq!(rescued(line, true), cut(line, EventPriority::Critical));
        // Stop over: it must not come back and demand it again.
        assert_eq!(rescued(line, false), None, "{line}");
    }
}

/// `playtest_road --find limit-drop` requeued "Limit 35."
///
/// Terse renders the overspeed warning as the bare limit, and a cut line
/// is handed back to finish -- so a driver who lifted off in the meantime
/// was told off for a speed they were no longer doing, quoting a limit
/// that by then might belong to road behind them.
#[test]
fn test_the_overspeed_nag_dies_once_the_driver_has_slowed() {
    let nag = "Limit 35.";
    let over = Rc::new(Cell::new(false));
    let (mut pacer, _) = make_pacer();
    flush_at(&mut pacer, nag, EventPriority::Route);
    pacer.track(nag, EventPriority::Route, None, always(over));
    assert_eq!(interrupt(&mut pacer, "Brake now!"), None);
}

mod settings;

/// A confirmation delivered queued does not take the rescue slot, any more
/// than one delivered interrupting or on the main channel does: the urgent
/// line that cuts it does not hand it back to be said a second time (QA
/// drive, 2026-10-13: "The road narrows to two lanes." heard twice around
/// "Automatic braking.").
#[test]
fn test_a_queued_confirmation_is_not_handed_back() {
    let (mut pacer, _clock) = make_pacer();
    let narrows = "The road narrows to two lanes.";
    pacer.should_flush(
        narrows,
        EventPriority::Route,
        Some(SpeechCategory::Confirmation),
        None,
    );
    assert_eq!(interrupt(&mut pacer, "Automatic braking."), None);

    // The same line queued as a route call is still rescued.
    let (mut pacer, _clock) = make_pacer();
    pacer.should_flush(narrows, EventPriority::Route, None, None);
    assert_eq!(
        interrupt(&mut pacer, "Automatic braking."),
        cut(narrows, EventPriority::Route)
    );
}

/// A confirmation queued behind a protected line leaves it protected: the
/// turn call it followed is still handed back when braking cuts it, as it
/// was before queued lines carried their category.
#[test]
fn test_a_queued_confirmation_leaves_the_turn_call_ahead_of_it_protected() {
    let (mut pacer, _clock) = make_pacer();
    let turn = "Exit 50B in half a mile, keep right.";
    pacer.should_flush(
        turn,
        EventPriority::Route,
        Some(SpeechCategory::Navigation),
        None,
    );
    pacer.should_flush(
        "Cruise set to 60.",
        EventPriority::Ambient,
        Some(SpeechCategory::Confirmation),
        None,
    );
    assert_eq!(
        interrupt(&mut pacer, "Automatic braking."),
        cut(turn, EventPriority::Route)
    );
}

/// The same for an hours-of-service warning followed by a confirmation at
/// ROUTE priority: the confirmation used to take the slot itself, and the
/// warning was lost.
#[test]
fn test_a_route_confirmation_leaves_the_warning_ahead_of_it_protected() {
    let (mut pacer, _clock) = make_pacer();
    let hos = "You have 30 minutes of driving left before your 11 hour limit.";
    pacer.should_flush(
        hos,
        EventPriority::Route,
        Some(SpeechCategory::Safety),
        None,
    );
    pacer.should_flush(
        "Staying in the left lane.",
        EventPriority::Route,
        Some(SpeechCategory::Confirmation),
        None,
    );
    assert_eq!(
        interrupt(&mut pacer, "Automatic braking."),
        cut(hos, EventPriority::Route)
    );
}
