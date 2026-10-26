//! `GameContext.say` / `say_event` and everything they run: the speech
//! ladder, the anti-backlog pacer, the audio duck, the transcript, and the
//! reviewable message log. The single most intricate piece of logic in the
//! app shell, reproduced line for line from `freight_fate/app.py`.
//!
//! Python defaults -> Rust: `say(text)` is `say_with(text, Say::new())`
//! (`interrupt=True, review=True, category=None`); `say_event(text)` is
//! `say_event_with(text, SayEvent::new())` (`interrupt=True, review=True,
//! priority=None, key=None, force=False, category=None, valid=None`). Both
//! take a plain `&str` or a `SpokenMessage` normal/terse pair through
//! [`IntoSpoken`].

use ff_core::message_log::MessageCategory;
use ff_core::speech_pacing::{Disposition, SpeechCategory};
use ff_core::speech_text::SpokenMessage;

use crate::audio::SPEECH_DUCK_LEVEL;
use crate::speech::{Cut, EventPriority, PacerConfig, Protect, Valid};

use super::GameContext;

/// The logger every spoken line lands in too, so a logged playtest reads as
/// a transcript of what the player heard -- the most faithful record for an
/// audio-first game. Same name as the Python logger so the lines grep alike.
pub const TRANSCRIPT_TARGET: &str = "freight_fate.transcript";

/// The event pacer's tuning: Hearken's defaults, plus the three values
/// Freight Fate's own pacer had. A CRITICAL line delivered queued (an
/// engine stall behind the line speaking) waits as long as chatter does
/// rather than flushing whatever is speaking; a ROUTE line waits under a
/// second; and a Chinese, Japanese or Korean character counts as three
/// English letters of speaking time, so a translated line is not mistaken
/// for a short one.
pub const FF_PACER: PacerConfig = PacerConfig {
    critical_wait_s: 3.0,
    important_wait_s: 0.8,
    wide_char_weight: 3.0,
    ..PacerConfig::DEFAULT
};

/// A CONFIRMATION never takes the pacer's hand-back slot: it reports an
/// outcome the player has usually heard in full, and handing it back after
/// the line that cut it repeats it, burying what came next ("The road
/// narrows to two lanes." heard twice around "Automatic braking.", QA
/// drive 2026-10-13). Every other line, categorised or not, may.
fn protect_for(category: Option<SpeechCategory>) -> Protect {
    if category == Some(SpeechCategory::Confirmation) {
        Protect::No
    } else {
        Protect::Yes
    }
}

macro_rules! transcript {
    ($($arg:tt)*) => {
        log::info!(target: TRANSCRIPT_TARGET, $($arg)*)
    };
}

/// How many hand-backs `GameContext::handed_back` keeps. An audit trail for a
/// bench run, not a history: a long session must not grow it without end.
const HANDED_BACK_KEPT: usize = 512;

/// The keyword arguments of `GameContext.say`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Say {
    pub interrupt: bool,
    pub review: bool,
    pub category: Option<SpeechCategory>,
}

impl Default for Say {
    fn default() -> Self {
        Self::new()
    }
}

impl Say {
    /// The Python defaults: `interrupt=True, review=True, category=None`.
    pub const fn new() -> Self {
        Self {
            interrupt: true,
            review: true,
            category: None,
        }
    }

    /// `interrupt=False`.
    pub const fn queued() -> Self {
        Self {
            interrupt: false,
            review: true,
            category: None,
        }
    }

    pub const fn interrupt(mut self, interrupt: bool) -> Self {
        self.interrupt = interrupt;
        self
    }

    pub const fn review(mut self, review: bool) -> Self {
        self.review = review;
        self
    }

    pub const fn category(mut self, category: SpeechCategory) -> Self {
        self.category = Some(category);
        self
    }
}

/// The keyword arguments of `GameContext.say_event`.
#[derive(Default)]
pub struct SayEvent {
    pub interrupt: bool,
    pub review: bool,
    pub priority: Option<EventPriority>,
    pub key: Option<String>,
    pub force: bool,
    pub category: Option<SpeechCategory>,
    /// Return a projected completion receipt for a keyed warning.
    pub receipt: bool,
    /// A zero-argument "is this line still true?" consulted if the line is
    /// cut mid-sentence and offered a rescue.
    pub valid: Option<Valid>,
}

impl SayEvent {
    /// The Python defaults: `interrupt=True, review=True`, nothing else.
    pub fn new() -> Self {
        Self {
            interrupt: true,
            review: true,
            ..Default::default()
        }
    }

    /// `interrupt=False`.
    pub fn queued() -> Self {
        Self::new().interrupt(false)
    }

    pub fn interrupt(mut self, interrupt: bool) -> Self {
        self.interrupt = interrupt;
        self
    }

    pub fn review(mut self, review: bool) -> Self {
        self.review = review;
        self
    }

    pub fn priority(mut self, priority: EventPriority) -> Self {
        self.priority = Some(priority);
        self
    }

    pub fn key(mut self, key: &str) -> Self {
        self.key = Some(key.to_string());
        self
    }

    pub fn force(mut self, force: bool) -> Self {
        self.force = force;
        self
    }

    pub fn category(mut self, category: SpeechCategory) -> Self {
        self.category = Some(category);
        self
    }

    pub fn receipt(mut self) -> Self {
        self.receipt = true;
        self
    }

    pub fn valid(mut self, valid: impl Fn() -> bool + 'static) -> Self {
        self.valid = Some(Box::new(valid));
        self
    }
}

/// What `say` / `say_event` accept as text: a plain string, or a
/// `SpokenMessage` normal/terse pair resolved in the delivery layer.
pub struct Spoken {
    pub message: SpokenMessage,
    /// Whether this came in as a pair (only a pair is dropped whole when its
    /// chosen rendering is empty).
    pub is_pair: bool,
}

pub trait IntoSpoken {
    fn into_spoken(self) -> Spoken;
}

impl IntoSpoken for &str {
    fn into_spoken(self) -> Spoken {
        Spoken {
            message: SpokenMessage::new(self),
            is_pair: false,
        }
    }
}

impl IntoSpoken for String {
    fn into_spoken(self) -> Spoken {
        Spoken {
            message: SpokenMessage::new(self),
            is_pair: false,
        }
    }
}

impl IntoSpoken for &String {
    fn into_spoken(self) -> Spoken {
        Spoken {
            message: SpokenMessage::new(self.as_str()),
            is_pair: false,
        }
    }
}

impl IntoSpoken for SpokenMessage {
    fn into_spoken(self) -> Spoken {
        Spoken {
            message: self,
            is_pair: true,
        }
    }
}

impl IntoSpoken for &SpokenMessage {
    fn into_spoken(self) -> Spoken {
        Spoken {
            message: self.clone(),
            is_pair: true,
        }
    }
}

impl GameContext {
    /// Whether the driving speech rung may silence anything yet.
    ///
    /// First-run teaching outranks the rung, exactly as it outranks terse
    /// (research doc R15). A player who picks the quietest setting before
    /// their first drive is the one who most needs to be told the status,
    /// help, and hazard keys exist -- silence them and they can never pull
    /// information nobody told them about. The gate is `tutorial_done`
    /// itself, so finishing the walkthrough and then choosing a quiet rung
    /// resurrects nothing.
    ///
    /// The default with no profile is deliberately `true`: no profile means
    /// nobody is on a first drive, so the rung applies normally.
    pub fn ladder_applies(&self) -> bool {
        self.profile
            .as_ref()
            .is_none_or(|profile| profile.tutorial_done)
    }

    /// Whether a driving line of this category reaches the voice on the
    /// player's rung.
    pub fn event_speaks(&self, category: Option<SpeechCategory>) -> bool {
        !self.ladder_applies() || self.settings.speaks(category)
    }

    /// Play the notification sound that announces a driving line, only if
    /// the line itself will be spoken. The tone belongs to the words: a
    /// rung that drops the words drops their tone too, so quiet and urgent
    /// only are not a run of bloops announcing nothing (owner, 2026-10-03).
    /// The road's own sounds -- traffic going by, the engine, a siren --
    /// never go through here.
    pub fn play_event_cue(
        &mut self,
        category: Option<SpeechCategory>,
        key: &str,
        volume: f64,
        pan: f64,
    ) {
        if self.event_speaks(category) {
            self.audio.play_with(key, volume, pan);
        }
    }

    /// An achievement earned at the wheel. It is flavor: urgent only keeps
    /// the drive silent for it, chime and name both, and the full record
    /// waits in message review and the achievements menu (speech mode
    /// audit, 2026-10-03). Quiet still hears the bare name.
    pub fn award_driving_achievement(&mut self, achievement_id: &str) {
        // The co-driver's driving earns the player nothing.
        if self.co_driver_away {
            return;
        }
        let announce = self.event_speaks(Some(SpeechCategory::Status));
        let awarded = self.award_achievement_with(achievement_id, false, announce);
        if let (false, Some(awarded)) = (announce, awarded) {
            let line = self.tr(&awarded.award.message.normal);
            self.message_log.add(&line, MessageCategory::General);
        }
    }

    /// `say(text)`: interrupt, review, no category.
    pub fn say(&mut self, text: &str) {
        self.say_with(text, Say::new());
    }

    /// `say(text, interrupt=..., review=..., category=...)`.
    pub fn say_with(&mut self, text: impl IntoSpoken, say: Say) {
        let spoken = text.into_spoken();
        let Say {
            mut interrupt,
            review,
            category,
        } = say;
        if self.co_driver_away && !self.speech_requested {
            if let Some(text) = self.render_spoken(&spoken) {
                transcript!("[co-driver] quiet: {}", text);
            }
            return;
        }
        if !self.settings.speaks(category) && self.ladder_applies() && !self.speech_requested {
            // Suppressed categories stay out of message review.
            //
            // A line the player ASKED for is exempt (`speech_requested`, set
            // by `player_asked()` around key and button handling) -- the
            // same escape `say_event` has carried as `force` since the
            // ladder shipped, missing here. Without it, pressing the cruise
            // dial at quiet answered with a chime and no number: the rung
            // was silencing an answer to a question the player had just
            // asked, which is not what a rung is for (owner, 2026-08-17).
            // The RENDERING still follows the rung, so quiet answers the
            // dial with "62" rather than a sentence.
            let text = self.render_silenced(&spoken);
            if self.event_pacer.is_silenced_repeat(&text, None, None) {
                // A keyless standing condition (no `key=` reaches this
                // method) re-fires on a timer while the drive is otherwise
                // unchanged. The gate above never used to consult the pacer,
                // so a silenced repeat still hit the earcon on every frame --
                // the quiet rung machine-gunning a sound where coaching says
                // one sentence and falls silent. The plain repeat window is
                // enough here; there is no `key=` to track a condition by.
                // `is_silenced_repeat`/`note_silenced` are a namespace of
                // their own (not `is_repeat`/`note_spoken`): a silenced
                // occurrence must never write into the state the SPEAKING
                // path reads, or raising the rung mid-drive while this same
                // condition is still active would find its own silenced text
                // already on file and go quiet for the sentence the rung now
                // promises.
                return;
            }
            self.event_pacer.note_silenced(&text, None);
            transcript!(
                "[ladder] {} silenced: {}",
                self.settings.driving_speech,
                text
            );
            return;
        }
        // A normal/terse pair resolves here, in the delivery layer, so
        // coverage never again depends on a call-site branch (research doc,
        // R5). An empty rendering is a line terse mode drops whole.
        let Some(text) = self.render_spoken(&spoken) else {
            return;
        };
        transcript!("{}", text);
        if interrupt && !self.speech_requested && self.top_paces_main_speech() {
            // At the wheel, the main channel queues instead of cutting: an
            // achievement or assist notice must not stamp on the line the
            // player was mid-way through hearing. Menus and readers keep
            // their default interrupt -- a screen over the drive is the top
            // state, so navigation there still cancels speech the way every
            // screen reader does. A queued line also cannot purge a shared
            // voice, so the rescue below has nothing to hand back.
            //
            // A line the player ASKED for is exempt (`speech_requested`).
            // The rule above was aimed at lines nobody asked for, but it was
            // applied to every main-channel line at the wheel, so pressing a
            // key stopped cutting the speech in progress -- which is what
            // 1.8 does and what every screen reader does (Sarah R. via the
            // owner, 2026-08-16). Answering the key you just pressed is the
            // whole contract of an info key.
            interrupt = false;
        }
        let mut cut = None;
        if interrupt && self.event_voice_shares_main() {
            // In this configuration the main channel is also carrying the
            // road, so an info reply's interrupt can land on a ROUTE or
            // CRITICAL event line mid-sentence (a tester blew a weigh station
            // this way). The reply still answers first; the cut line queues
            // right behind it.
            cut = self.event_pacer.note_channel_purged();
        }
        self.speech.say(&text, interrupt);
        self.log_mostly_heard_cut();
        if let Some(cut) = cut {
            self.requeue_cut_event(cut);
        }
        if review {
            self.message_log.add(&text, MessageCategory::General);
        }
    }

    /// The silenced branch's rendering: the rung's rendering, falling back
    /// to the normal text when terse drops the line whole.
    fn render_silenced(&self, spoken: &Spoken) -> String {
        self.tr(&self.render_silenced_english(spoken))
    }

    fn render_silenced_english(&self, spoken: &Spoken) -> String {
        if spoken.is_pair {
            let rendered = spoken.message.render(self.settings.renders_terse());
            if rendered.is_empty() {
                spoken.message.normal.clone()
            } else {
                rendered.to_string()
            }
        } else {
            spoken.message.normal.clone()
        }
    }

    /// The speaking branch's rendering; `None` for a pair terse drops whole.
    /// The line is translated here, after the speech mode picked it, so
    /// the pacer, message review and the transcript all hold what the
    /// player actually heard.
    fn render_spoken(&self, spoken: &Spoken) -> Option<String> {
        self.render_spoken_english(spoken)
            .map(|text| self.tr(&text))
    }

    fn render_spoken_english(&self, spoken: &Spoken) -> Option<String> {
        if spoken.is_pair {
            let rendered = spoken.message.render(self.settings.renders_terse());
            if rendered.is_empty() {
                return None;
            }
            Some(rendered.to_string())
        } else {
            Some(spoken.message.normal.clone())
        }
    }

    /// True when driving events speak on the main channel.
    ///
    /// Either the player chose the main voice for events (Settings >
    /// Speech), or the dedicated-voice preference found no separate backend
    /// to bind, so `say_event` falls back to the main channel.
    pub fn event_voice_shares_main(&self) -> bool {
        if !self.settings.sapi_events {
            return true;
        }
        !self.speech.has_separate_event_voice()
    }

    /// Finish delivering a ROUTE or CRITICAL line an interrupt cut short.
    ///
    /// Straight to the voice, queued: the line already passed the pacer,
    /// the transcript, and the message log when it was first submitted, so
    /// this is the same delivery completing rather than a new event. The
    /// pacer tracks it again, so a further genuine interrupt can hand it
    /// back a second time; it cannot ping-pong forever, because a requeue
    /// is never interrupting and an identical interrupting line is never
    /// handed back behind itself.
    fn requeue_cut_event(&mut self, cut: Cut) {
        self.deliver_cut_event(cut, false);
    }

    /// Deliver a handed-back line: queued behind whatever cut it, or, for a
    /// flush, interrupting in front of the line that flushed (see the flush
    /// in `say_event_with`).
    fn deliver_cut_event(&mut self, cut: Cut, interrupt: bool) {
        let (text, priority) = cut;
        // Name the line. A bare "cut line requeued" says a hand-back
        // happened and not what came back, which is the one thing an audit
        // needs: four separate lines have now been found coming back after
        // their moment had passed (the scale exit, the destination exit,
        // the hazard dodge call, the dock hold prompt), each by a tester
        // hearing it rather than by anyone reading a bench transcript. With
        // the text here, a playtest log answers the question by itself --
        // grep the requeues, read each one, ask whether it was still true.
        transcript!("[pacer] cut line requeued: {}", text);
        if self.handed_back.len() == HANDED_BACK_KEPT {
            self.handed_back.pop_front();
        }
        self.handed_back.push_back(text.clone());
        if interrupt {
            self.event_pacer.note_ahead(&text);
        } else {
            self.event_pacer
                .note_queued(&text, priority, Protect::Yes, None);
        }
        self.event_pacer.resume_delivery(&text);
        if self.settings.sapi_events {
            self.speech.say_event(&text, interrupt);
        } else {
            self.speech.say(&text, false);
        }
    }

    /// How many of the voice's deliveries containing `phrase` were the pacer
    /// finishing a cut line rather than the game announcing something.
    pub fn handed_back_count(&self, phrase: &str) -> usize {
        self.handed_back
            .iter()
            .filter(|line| line.contains(phrase))
            .count()
    }

    /// Name the ROUTE or CRITICAL line the latest cut destroyed because the
    /// player had already heard most of it (the pacer's ruling, not a
    /// requeue: saying it again whole is the stutter the 1 September drives
    /// were full of). Same audit value as the requeue line above -- grep the
    /// drops, read each one, ask whether its last words mattered. The
    /// message log still holds it.
    fn log_mostly_heard_cut(&mut self) {
        if let Some(text) = self.event_pacer.take_mostly_heard() {
            transcript!("[pacer] cut line mostly heard, dropped: {}", text);
        }
    }

    /// Forget what has been said once, at a leg boundary.
    ///
    /// FIRST_OCCURRENCE is "speaks the first time per leg", so a new leg is
    /// a fresh road and the tip is worth one more telling.
    pub fn reset_ladder_leg_memory(&mut self) {
        self.ladder_said.clear();
    }

    /// Whether this rung's disposition drops this line as already-said.
    ///
    /// FIRST_OCCURRENCE: this LINE said once per leg, then nothing. Keyed on
    /// the key and the text together, not the key alone: the disposition
    /// exists for a coaching tip that repeats word for word, and keying on
    /// the condition instead meant a keyed readout whose number moves was
    /// said once and swallowed for the rest of the leg.
    ///
    /// TRANSITIONS: "enter, worsen, and clear only". A `key` names a standing
    /// condition, and status lines carry the state they are reporting in
    /// their own text -- so a line identical to the last one under this key
    /// is the condition re-asserting itself and a changed one is the
    /// transition.
    ///
    /// A line with NO key is not a standing condition; it is a discrete
    /// moment, and its call site has already decided this occurrence is
    /// worth saying. The rung has nothing to compare it against, so it does
    /// not suppress it. This fallback used to key on the text itself, which
    /// turned "enter, worsen, and clear" into "this exact sentence once per
    /// leg, ever" -- 310 lines dropped in one leg of a tester's log on
    /// STANDARD, the default rung (Darren, 2026-08-19). The pacer's own
    /// repeat window still stops the same line landing twice in a breath.
    fn ladder_repeats(
        &mut self,
        text: &str,
        category: Option<SpeechCategory>,
        key: Option<&str>,
    ) -> bool {
        match self.settings.speech_disposition(category) {
            Disposition::FirstOccurrence => {
                let slot = (key.unwrap_or("").to_string(), text.to_string());
                if self.ladder_said.contains(&slot) {
                    return true;
                }
                self.ladder_said.insert(slot);
                false
            }
            Disposition::Transitions => {
                let Some(key) = key else {
                    return false;
                };
                if self.ladder_last.get(key).is_some_and(|last| last == text) {
                    return true;
                }
                self.ladder_last.insert(key.to_string(), text.to_string());
                false
            }
            _ => false,
        }
    }

    /// `say_event(text)`: interrupt, review, no pacing arguments.
    pub fn say_event(&mut self, text: &str) {
        self.say_event_with(text, SayEvent::new());
    }

    /// Driving event announcements (hazards, warnings, weather, ...).
    ///
    /// `valid`, when given, answers "is this line still true?" -- consulted
    /// if the line is cut mid-sentence and offered a rescue. A line whose
    /// moment has passed (the scale is behind the truck, the damage total
    /// has moved on) is dropped instead of replayed verbatim; message review
    /// holds the words either way.
    ///
    /// With the dedicated SAPI event voice enabled, events speak on their
    /// own channel, where `interrupt` only cuts off a previous event -- so
    /// an urgent cue can still jump ahead of a stale one without touching
    /// the screen reader. With it disabled the player has chosen to hear
    /// events through their screen reader. Urgent events first flush stale
    /// game speech, then speak as a fresh queued utterance so old messages
    /// do not bury the warning.
    ///
    /// Every line first passes the pacer, which knows what the player has
    /// already heard. A line identical to one spoken a moment ago is dropped
    /// outright -- one moment noticed on several frames is still one moment.
    /// `key` marks a standing condition (a damaged load, an engine at
    /// redline) rather than an event: it speaks when it starts and again
    /// only when what it says has changed. `force` is for a line the player
    /// asked for and must hear whether or not it repeats.
    ///
    /// Queued events then ride an anti-backlog projection either way: a
    /// line that would start speaking well after the moment it described
    /// flushes the expired backlog and speaks now instead of joining the
    /// recital. `priority` sets how long it is willing to wait first --
    /// route announcements give ambient chatter well under a second before
    /// going in front of it.
    ///
    /// An interrupting line's purge also lands on whatever the voice was
    /// mid-way through. When that was a ROUTE or CRITICAL line still
    /// plausibly speaking, the pacer hands it back and it is resubmitted
    /// queued, right behind the interrupting line -- safety line first,
    /// then the line it stepped on, never dropped.
    ///
    /// `review` keeps a line out of the reviewable message log. An assist
    /// that interrupts to say it is acting would otherwise land exactly
    /// where the warning it cut off should be, so the review keys that
    /// exist to rescue an interrupted line would hand back the interruption
    /// instead.
    ///
    /// `text` may be a `SpokenMessage` normal/terse pair; the player's
    /// speech mode picks the rendering here, so terse coverage is a property
    /// of the message definition rather than of the call site. A pair whose
    /// terse rendering is empty is dropped whole in terse mode: not spoken,
    /// not logged, exactly like a muted chatter line.
    pub fn say_event_with(&mut self, text: impl IntoSpoken, opts: SayEvent) {
        let spoken = text.into_spoken();
        let SayEvent {
            mut interrupt,
            review,
            priority,
            key,
            force,
            category,
            receipt,
            valid,
        } = opts;
        let key = key.as_deref();
        if self.co_driver_away && !force && !self.speech_requested {
            // Nobody is listening for the road while the co-driver drives.
            // A line waiting on its delivery receipt is released, so a stop
            // plan never stalls behind speech that will not come.
            if let Some(text) = self.render_spoken(&spoken) {
                if receipt {
                    if let Some(key) = key {
                        self.event_pacer.complete_delivery(key, &text);
                    }
                }
                transcript!("[co-driver] quiet: {}", text);
            }
            return;
        }
        if !self.settings.speaks(category) && !force && self.ladder_applies() {
            // Suppressed categories stay out of message review. Status keys
            // still answer on demand. `force`
            // is a line the player asked for and must hear. Nothing sounds
            // in place of the words (owner, 2026-10-03).
            let text = self.render_silenced(&spoken);
            if self.event_pacer.is_silenced_repeat(&text, key, None) {
                // A keyed standing condition (an engine held at redline, a
                // locked-out air brake) re-fires on a timer by design while
                // the condition holds -- that is what keeps it audible when
                // the rung speaks it. But this gate used to return before
                // ever reaching the pacer, so a silenced repeat still played
                // the earcon and logged the line on every re-fire: the quiet
                // rung machine-gunning a sound where coaching says one
                // sentence and falls silent for the rest of the drive.
                // `is_silenced_repeat`/`note_silenced` deliberately do NOT
                // share `is_repeat`/`conditions` with the speaking path
                // below: if a silenced occurrence wrote `conditions[key]`,
                // raising the rung mid-drive while this same condition was
                // still active would have the speaking path's own `is_repeat`
                // see its silenced text already on file, read the now-audible
                // occurrence as an unchanged repeat, and skip it -- the rung
                // the player raised specifically to hear this condition going
                // quiet instead.
                return;
            }
            self.event_pacer.note_silenced(&text, key);
            if receipt {
                if let Some(key) = key {
                    self.event_pacer.complete_delivery(key, &text);
                }
            }
            transcript!(
                "[ladder] {} silenced: {}",
                self.settings.driving_speech,
                text
            );
            return;
        }
        let Some(text) = self.render_spoken(&spoken) else {
            return;
        };
        if !force && self.ladder_applies() && self.ladder_repeats(&text, category, key) {
            // Said once already; keep the original review entry without duplicates.
            transcript!(
                "[ladder] {} already said: {}",
                self.settings.driving_speech,
                text
            );
            return;
        }
        if self.event_pacer.is_repeat(&text, key, force, None) {
            // Already in the player's ear. Not spoken, not logged, not
            // reviewable: as far as the drive is concerned it never happened
            // a second time.
            return;
        }
        let priority = priority.unwrap_or(if interrupt {
            EventPriority::Critical
        } else {
            EventPriority::Ambient
        });
        if !interrupt
            && priority == EventPriority::Ambient
            && self.settings.sapi_events
            && self.event_pacer.would_start_stale(&text, priority)
        {
            // Chatter that would start speaking after the moment it
            // described is dropped, not promoted to an interrupt: losing it
            // costs the player nothing (the enum's own words), and the old
            // stale-flush made the least important class the only one
            // guaranteed to preempt. Keep these eligible but delayed events in
            // review for recovery; preference-suppressed categories are omitted.
            // Not marked heard: an identical later moment speaks fresh.
            transcript!("[pacer] stale ambient dropped: {}", text);
            if review {
                self.message_log.add(&text, MessageCategory::Event);
            }
            return;
        }
        transcript!("[event] {}", text);
        self.event_pacer.note_spoken(&text, key);
        let mut cut = None;
        let protect = protect_for(category);
        if self.settings.sapi_events {
            if interrupt {
                cut = self
                    .event_pacer
                    .note_interrupt(&text, priority, protect, valid);
            } else if self
                .event_pacer
                .should_flush(&text, priority, protect, valid)
            {
                // The channel is backed up past the point of truth: purging
                // and speaking fresh IS the queued line's honest delivery.
                // The purge can still have cut a line that was genuinely
                // mid-sentence, so it is handed back and requeued behind this
                // one exactly as an interrupt's would be -- never dropped.
                transcript!("[pacer] stale event backlog flushed");
                cut = self.event_pacer.take_flush_cut();
                interrupt = true;
                // What a flush hands back came BEFORE this line and is still
                // true, so it is spoken first and this line queues behind
                // it. Requeued behind instead, the road played backwards:
                // "assistance is taking you onto the streets" after
                // "assistance released" (agent drive, 2026-09-23).
                if let Some(first) = cut.take() {
                    self.deliver_cut_event(first, true);
                    interrupt = false;
                }
            }
            self.speech.say_event(&text, interrupt);
        } else {
            if interrupt {
                cut = self
                    .event_pacer
                    .note_interrupt(&text, priority, protect, valid);
                self.speech.stop_main();
            } else {
                self.event_pacer
                    .note_queued(&text, priority, protect, valid);
            }
            self.speech.say(&text, false);
        }
        if receipt {
            if let Some(key) = key {
                self.event_pacer.track_delivery(key, &text);
            }
        }
        self.engage_speech_duck();
        self.log_mostly_heard_cut();
        if let Some(cut) = cut {
            self.requeue_cut_event(cut);
        }
        if review {
            self.message_log.add(&text, MessageCategory::Event);
        }
    }

    /// Step the game mix back while the event voice speaks (R13).
    ///
    /// XAG 105's guideline made a setting: engine, weather, and the radio
    /// drop to half volume so a warning survives a loud cab without the
    /// voice itself getting louder. Restoration is edge-driven, not polled:
    /// `update_speech_duck` runs once per frame from the main loop and
    /// restores the mix when the pacer's projection -- the same estimate the
    /// staleness budget runs on -- says the voice is done.
    fn engage_speech_duck(&mut self) {
        if !self.settings.duck_audio_for_speech {
            return;
        }
        self.speech_ducked = true;
        self.audio.set_speech_duck(SPEECH_DUCK_LEVEL);
    }

    /// Per-frame: bring the mix back once the event voice falls silent.
    pub fn update_speech_duck(&mut self) {
        if !self.speech_ducked {
            return;
        }
        if self.event_pacer.busy() {
            return;
        }
        self.speech_ducked = false;
        self.audio.set_speech_duck(1.0);
    }

    /// Whether the mix is currently stepped back under speech.
    pub fn speech_ducked(&self) -> bool {
        self.speech_ducked
    }

    /// A standing condition has cleared; let it announce itself afresh.
    pub fn reset_event_condition(&mut self, key: &str) {
        self.event_pacer.forget_condition(key);
        // TRANSITIONS means enter, worsen and clear. A condition that
        // genuinely cleared and comes back is an ENTER, even though its text
        // is word-for-word what was said last time -- so the last-text
        // memory has to be dropped here too, or standard swallows the second
        // real warning.
        self.ladder_last.remove(key);
    }

    /// The player stepped off the road: silence the road and drop its
    /// backlog.
    ///
    /// Stopping the channel is what actually purges the voice's own queue --
    /// without it, everything the road had submitted before the pause is
    /// still in there and gets performed the moment the player comes back
    /// (tester transcript, 2026-08-11).
    pub fn pause_event_speech(&mut self) {
        self.event_pacer.pause();
        self.speech.stop_event();
    }

    /// Back at the wheel; nothing from before the pause is news.
    pub fn resume_event_speech(&mut self) {
        self.event_pacer.resume();
        self.speech.stop_event();
    }

    /// Mark everything spoken inside `f` as an answer to a control press.
    ///
    /// Wrapped around the driving state's input handlers rather than added
    /// to each readout, for the same reason the pacing rule itself is
    /// central: there are twenty-odd info keys and they gain new siblings
    /// every week, and a rule that has to be remembered per call site is
    /// one that will be missed. Anything spoken from a key press is by
    /// definition something the player asked for.
    ///
    /// Re-entrant and restoring, so a handler that opens a menu which
    /// speaks does not leave the flag stuck on for the ambient lines that
    /// follow.
    pub fn player_asked<R>(&mut self, f: impl FnOnce(&mut GameContext) -> R) -> R {
        let previous = self.player_asked_begin();
        let result = f(self);
        self.player_asked_end(previous);
        result
    }

    /// The two halves of `player_asked` for a handler that cannot take a
    /// closure (a state method that needs `self` and `ctx` both): call
    /// `begin`, keep the token, call `end` with it on every return path.
    pub fn player_asked_begin(&mut self) -> bool {
        let previous = self.speech_requested;
        self.speech_requested = true;
        self.sync_cue_mute();
        previous
    }

    pub fn player_asked_end(&mut self, previous: bool) {
        self.speech_requested = previous;
        self.sync_cue_mute();
    }

    /// The co-driver has the wheel and the player is not pressing anything:
    /// the game's own cues stay quiet with the words they announce (see
    /// `Audio::set_cues_muted`). Call after `co_driver_away` changes.
    pub fn sync_cue_mute(&mut self) {
        let muted = self.co_driver_away && !self.speech_requested;
        self.audio.set_cues_muted(muted);
    }

    /// Whether a control press is being handled right now.
    pub fn speech_requested(&self) -> bool {
        self.speech_requested
    }

    /// The leg-scoped FIRST_OCCURRENCE memory: `(key, text)` pairs.
    pub fn ladder_said(&self) -> &std::collections::HashSet<(String, String)> {
        &self.ladder_said
    }

    /// The TRANSITIONS memory: last text per condition key.
    pub fn ladder_last(&self) -> &std::collections::HashMap<String, String> {
        &self.ladder_last
    }

    /// Whether the event voice is mid-delivery right now.
    ///
    /// The same projection the audio duck restores on, so a control that
    /// has to decide "is there anything to shut up" is trusting the estimate
    /// the mix already trusts rather than inventing a second one.
    pub fn event_voice_busy(&mut self) -> bool {
        self.event_pacer.busy()
    }

    /// Whether the event voice is mid-way through a warning to act on now.
    pub fn event_voice_critical(&mut self) -> bool {
        self.event_pacer.speaking_critical()
    }

    pub fn event_delivery_status(&mut self, key: &str) -> Option<crate::speech::DeliveryStatus> {
        self.event_pacer.delivery_status(key)
    }

    pub fn event_delivery_pending(&mut self) -> bool {
        self.event_pacer.delivery_pending()
    }

    pub fn stop_event_speech(&mut self) {
        self.event_pacer.reset();
        self.speech.stop_event();
    }

    /// Silence all in-progress speech on both channels (main and event).
    ///
    /// Menus and readers speak through the main channel, so the
    /// driving-only `stop_event_speech` does not quiet them. This silences
    /// everything so a single key works as a "stop talking" everywhere in
    /// the game.
    pub fn stop_speech(&mut self) {
        self.event_pacer.reset();
        self.speech.stop_main();
        self.speech.stop_event();
    }
}
