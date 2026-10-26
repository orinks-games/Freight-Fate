//! The ambient line queue: lines that wait for the road to go quiet, and the
//! drain that ages them out rather than performing them late.

use crate::speech::EventPriority;
use ff_core::message_log::MessageCategory;
use ff_core::sim::driving_modes::tuning_for_time_scale;
use ff_core::sim::trip_models::{TripEvent, TripEventKind};
use ff_core::speech_pacing::SpeechCategory;
use ff_core::speech_text::SpokenMessage;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{AmbientRender, PendingAmbient};

use super::{AMBIENT_QUEUE_MAX, AMBIENT_QUEUE_MAX_AGE_S};

/// The keyword arguments of `_speak_ambient_event`.
#[derive(Default)]
pub struct Ambient {
    pub sound: Option<String>,
    pub log: bool,
    pub category: Option<SpeechCategory>,
    pub key: Option<String>,
    pub priority: Option<EventPriority>,
    pub render: Option<AmbientRender>,
}

impl Ambient {
    /// The Python defaults: `sound=None, log=True, category=None, key=None,
    /// render=None`.
    pub fn new() -> Self {
        Self {
            log: true,
            ..Default::default()
        }
    }

    pub fn sound(mut self, sound: Option<&str>) -> Self {
        self.sound = sound.map(str::to_string);
        self
    }

    pub fn log(mut self, log: bool) -> Self {
        self.log = log;
        self
    }

    pub fn category(mut self, category: Option<SpeechCategory>) -> Self {
        self.category = category;
        self
    }

    pub fn key(mut self, key: Option<String>) -> Self {
        self.key = key;
        self
    }

    pub fn priority(mut self, priority: EventPriority) -> Self {
        self.priority = Some(priority);
        self
    }

    pub fn render(mut self, render: Option<AmbientRender>) -> Self {
        self.render = render;
        self
    }
}

impl DrivingState {
    /// Log an ambient line the moment it queues, not when it is spoken.
    ///
    /// The one-deep slot below can still let a hazard wipe this line
    /// outright, or a later ambient line overwrite it, before it is ever
    /// spoken. Either way the review buffer already has it -- a chimed
    /// line must never come up empty there (tester Sarah, US-12 East,
    /// 2026-08-14: a lane closure dinged and vanished, spoken nowhere and
    /// reviewable nowhere; she runs terse speech, which makes this the
    /// ONLY record of what the earcon was for). A line this speech mode
    /// drops whole -- an empty terse rendering, an earcon or silence
    /// carrying it instead -- keeps SpokenMessage's own contract and stays
    /// out of review too, same as it always has. Anything that does get
    /// logged is the full, normal wording regardless of speech mode: a
    /// driver who opens review after hearing the terse form is asking for
    /// the detail terse left out, not a repeat of the short version.
    pub fn log_ambient_event(
        &self,
        ctx: &mut GameContext,
        message: &SpokenMessage,
        category: Option<SpeechCategory>,
    ) {
        if message.normal.is_empty() {
            return;
        }
        if ctx.ladder_applies() && !ctx.settings.speaks(category) {
            // The rung carries it as a sound or as nothing; `say_event`
            // keeps such a line out of review, and queueing it first must
            // not file it anyway.
            return;
        }
        if message.terse.is_some() && message.render(self.terse_speech(ctx)).is_empty() {
            return;
        }
        let line = ctx.tr(&message.normal);
        ctx.message_log.add(&line, MessageCategory::Event);
    }

    /// `_speak_ambient_event(message, sound=None, *, log=True, category=None,
    /// key=None, priority=None, render=None)`.
    pub fn speak_ambient_event(
        &mut self,
        ctx: &mut GameContext,
        message: SpokenMessage,
        opts: Ambient,
    ) {
        let Ambient {
            sound,
            log,
            category,
            key,
            priority,
            render,
        } = opts;
        if log {
            // The drain call below passes log=False so a line that does
            // make it to speech is not entered into review twice.
            self.log_ambient_event(ctx, &message, category);
        }
        if self.hazard_deadline.is_some() || self.ambient_event_cooldown_s > 0.0 {
            // Queued, not stored in place. This used to be a single slot, and
            // two things fell through it: a later ambient line overwrote
            // whatever was waiting, and a hazard threw the slot away outright.
            // On an interstate that meant a mapped state line was lost every
            // single time -- the crossing the driver most wants and the one
            // the map is most sure of. The trip compensated by prefixing
            // "Crossing into Ohio." to the next city line, a duplicate kept
            // deliberately because silence at a state line is worse
            // (trip_road_events._check_cities).
            //
            // A queue makes the line survive both. What keeps it from becoming
            // a recital is age, not capacity: a line that waited out a long
            // hazard is dropped in _update_ambient_events rather than
            // performed late. Full queue drops the OLDEST for the same reason.
            //
            // A line ABOUT something still ahead restates itself as the truck
            // closes on it -- "CB chatter in 5 miles", then "in 4". Queueing
            // both would say five when the driver is at four, which is worse
            // than the overwrite ever was: wrong, not merely late. So a keyed
            // line replaces the one already waiting under that key, in place,
            // keeping the wait it had already served. Only what it SAYS
            // changed; it has been waiting the whole time.
            let text = message.render(self.terse_speech(ctx)).to_string();
            if let Some(key) = key.as_deref() {
                if let Some(waiting) = self
                    .pending_ambient_events
                    .iter_mut()
                    .find(|waiting| waiting.key.as_deref() == Some(key))
                {
                    waiting.message = text;
                    waiting.sound = sound;
                    waiting.category = category;
                    waiting.priority = priority;
                    waiting.render = render;
                    return;
                }
            }
            let mut pending = PendingAmbient::new(text);
            pending.sound = sound;
            pending.category = category;
            pending.key = key;
            pending.priority = priority;
            pending.render = render;
            self.pending_ambient_events.push_back(pending);
            while self.pending_ambient_events.len() > AMBIENT_QUEUE_MAX {
                self.pending_ambient_events.pop_front();
            }
            return;
        }
        if let Some(sound) = sound.as_deref() {
            // The line's own tone: nothing when the rung drops the words.
            ctx.play_event_cue(category, sound, 1.0, 0.0);
        }
        let mut opts = SayEvent::queued();
        opts.review = false;
        opts.category = category;
        opts.priority = priority;
        ctx.say_event_with(message, opts);
        self.ambient_event_cooldown_s =
            tuning_for_time_scale(self.trip.time_scale).ambient_spacing_s;
    }

    /// `_update_ambient_events(dt)`.
    pub fn update_ambient_events(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.ambient_event_cooldown_s > 0.0 {
            self.ambient_event_cooldown_s = 0.0f64.max(self.ambient_event_cooldown_s - dt);
        }
        // Age everything waiting, including through a hazard: a hazard no
        // longer discards the queue, so this is what stops a line describing
        // a mile the truck left long ago from being spoken as if it were now.
        // Already logged when it queued, so an aged-out line is still in
        // review -- dropped from the ear, not from the record.
        for pending in self.pending_ambient_events.iter_mut() {
            pending.waited_s += dt;
        }
        while self
            .pending_ambient_events
            .front()
            .is_some_and(|pending| pending.waited_s >= AMBIENT_QUEUE_MAX_AGE_S)
        {
            self.pending_ambient_events.pop_front();
        }
        if self.hazard_deadline.is_some() {
            return;
        }
        if self.ambient_event_cooldown_s > 0.0 || self.pending_ambient_events.is_empty() {
            return;
        }
        let pending = self
            .pending_ambient_events
            .pop_front()
            .expect("checked above");
        let mut message = pending.message.clone();
        if let Some(render) = pending.render.as_ref() {
            // Say the distance as of NOW, not as of when it queued.
            match render(self, ctx) {
                // the moment passed while it waited; drop, not lie
                None => return,
                Some(text) => message = text,
            }
        }
        // Already logged the moment it queued; speaking it now must not log
        // it a second time.
        let mut opts = Ambient::new()
            .sound(pending.sound.as_deref())
            .log(false)
            .category(pending.category)
            .key(pending.key.clone());
        if let Some(priority) = pending.priority {
            opts = opts.priority(priority);
        }
        self.speak_ambient_event(ctx, SpokenMessage::new(message), opts);
    }

    /// What standing thing this ambient line is about, if any.
    ///
    /// Only the lines that count DOWN toward something get one. A landmark,
    /// a lane count, a billboard, a state line: each is its own moment, and
    /// two of them are two things to say, so they queue. A patrol post, a
    /// traffic pressure or a toll is one thing said again at a nearer
    /// distance, and the nearer wording replaces the further one.
    pub fn ambient_key(&self, event: &TripEvent) -> Option<String> {
        if let Some(post) = event.data.cb_patrol.as_ref() {
            return Some(format!("cb:{}:{}", post.leg_index, post.at_mi));
        }
        if let Some(pressure) = event.data.traffic_pressure.as_ref() {
            return Some(format!("pressure:{}:{}", pressure.kind, pressure.start_mi));
        }
        if let Some(cue) = event.data.cue.as_ref() {
            if cue.kind == "toll" {
                return Some(format!("toll:{}", cue.key));
            }
        }
        if event.kind == TripEventKind::WeatherChange {
            // The weather is one standing condition; a newer reading of it
            // replaces an older one rather than being read out in sequence.
            return Some("weather".to_string());
        }
        None
    }

    /// `_should_space_ambient_event(event)`.
    pub fn should_space_ambient_event(&self, event: &TripEvent) -> bool {
        if event.kind == TripEventKind::WeatherChange {
            return true;
        }
        if event.kind == TripEventKind::StopAhead {
            // Travel-plaza and rest-stop notices are informational: they queue
            // behind whatever route speech just played instead of stacking on
            // it -- at departure that keeps the merge instruction in front.
            //
            // The stop the player PLANNED is not a notice, it is the drive.
            // Held in the one-deep ambient slot it was overwritten by the next
            // piece of chatter, or thrown away outright by the next hazard,
            // and the player drove past a stop they had chosen (tester Darren,
            // 2026-08-11). It never goes through the ambient channel.
            return !event.data.planned.unwrap_or(false);
        }
        if event.kind == TripEventKind::GpsCue {
            let toll = event
                .data
                .cue
                .as_ref()
                .is_some_and(|cue| cue.kind == "toll");
            return event.data.cb_patrol.is_some() || event.data.traffic_pressure.is_some() || toll;
        }
        false
    }
}
