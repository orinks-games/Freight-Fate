//! Suggest a station, from the Radio app: a player names a radio station
//! that is missing from the dial and sends it to orinks.net, where the
//! stream is played for a few seconds and checked against the dial before
//! the owner reviews it. Accepted stations come back to every player in the
//! community station list (`crate::community_stations`).
//!
//! Two screens. A short menu asks whether the station broadcasts on AM or
//! FM; then one text field walks the questions in turn (name, stream
//! address, and for a broadcast station its call sign, state, city and
//! frequency, then its format). Optional questions are skipped with an empty
//! Enter. The site checks every answer and says what it thinks in a sentence
//! the game speaks as it comes; when it turns down one answer, the field goes
//! back to that question with everything typed so far still filled in.
//!
//! Owner, 2026-10-09: "add to the form whether this is a terrestrial
//! station, and if so, which state its in etc."

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::app::{GameContext, Say};
use crate::community_stations::{
    default_sender, suggestion_outcome, StationSuggestion, SuggestionOutcome, SuggestionSender,
};
use crate::online_presence::OnlineIdentity;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::online_states::{load_identity, run_worker, Mailbox};
use crate::states::text_entry::{TextEntry, TextEntryCore};
use crate::{impl_state_for_menu, impl_state_for_text_entry};

/// Said instead of opening the screens when they could not send anything.
pub const SUGGEST_NEEDS_ONLINE: &str =
    "Suggesting a station needs Online services on, in the Online menu.";
pub const SUGGEST_NEEDS_SETUP: &str =
    "Suggesting a station needs this computer set up with orinks.net, in the Online menu.";

/// Open the suggestion screens, or say why they cannot open.
pub fn open_station_suggestion(ctx: &mut GameContext) {
    if !ctx.settings.online_services {
        ctx.audio.play("ui/error");
        ctx.say(SUGGEST_NEEDS_ONLINE);
        return;
    }
    let Some(identity) = load_identity() else {
        ctx.audio.play("ui/error");
        ctx.say(SUGGEST_NEEDS_SETUP);
        return;
    };
    ctx.push_state(SuggestKindState::new(identity, default_sender()));
}

// -- the kind ------------------------------------------------------------------------------

/// "How does it broadcast?" The answer decides which questions follow.
pub struct SuggestKindState {
    menu: MenuCore<Self>,
    identity: OnlineIdentity,
    sender: SuggestionSender,
}

impl SuggestKindState {
    pub const TITLE: &'static str = "Suggest a station. How does it broadcast?";

    pub fn new(identity: OnlineIdentity, sender: SuggestionSender) -> Self {
        Self {
            menu: MenuCore::new(Self::TITLE),
            identity,
            sender,
        }
    }

    fn choose(&mut self, ctx: &mut GameContext, kind: &str) {
        ctx.audio.play("ui/menu_select");
        let entry =
            StationSuggestionEntryState::new(kind, self.identity.clone(), self.sender.clone());
        ctx.replace_state(entry);
    }
}

impl Menu for SuggestKindState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        vec![
            MenuItem::new("On AM or FM, and online", |s: &mut Self, ctx| {
                s.choose(ctx, "terrestrial")
            })
            .help("A broadcast station. You'll be asked its call sign and state."),
            MenuItem::new("Online only", |s: &mut Self, ctx| s.choose(ctx, "web"))
                .help("A station that streams on the internet and has no AM or FM signal."),
            MenuItem::new("Back", |s: &mut Self, ctx| s.go_back(ctx)).help("Go back."),
        ]
    }
}

impl_state_for_menu!(SuggestKindState);

// -- the questions ----------------------------------------------------------------------------

/// Said when the stream address is not a web address at all. The same
/// sentence the site gives, so a player hears it at the question rather than
/// after the last one; the site still checks that the address plays.
pub const NOT_A_STREAM_ADDRESS: &str = "The stream address should start with http or https.";

/// The site's own test of the address's shape: an http or https address
/// with a host and no sign-in part. Anything else could never play.
pub fn is_stream_address(text: &str) -> bool {
    url::Url::parse(text).is_ok_and(|url| {
        matches!(url.scheme(), "http" | "https")
            && url.host_str().is_some_and(|host| !host.is_empty())
            && url.username().is_empty()
            && url.password().is_none()
    })
}

/// One question in the walk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    Name,
    StreamUrl,
    CallSign,
    State,
    City,
    Frequency,
    Genre,
}

impl Question {
    fn heading(self) -> &'static str {
        match self {
            Question::Name => "Station name",
            Question::StreamUrl => "Stream address",
            Question::CallSign => "Call sign",
            Question::State => "State",
            Question::City => "City",
            Question::Frequency => "Frequency",
            Question::Genre => "Format",
        }
    }

    /// What is said after the heading when the question comes up.
    fn hint(self) -> &'static str {
        match self {
            Question::Name => "",
            Question::StreamUrl => {
                "The direct link to the audio stream, not the station's web page. streamurl.link finds one. Control V pastes."
            }
            Question::CallSign => "Like W X Y Z, or K A B C dash F M.",
            Question::State => "The state it broadcasts from.",
            Question::City => "Optional. Enter skips.",
            Question::Frequency => "Optional, like 101.5 or 1090. Enter skips.",
            Question::Genre => "Optional, like classic country. Enter skips.",
        }
    }

    fn required(self) -> bool {
        matches!(
            self,
            Question::Name | Question::StreamUrl | Question::CallSign | Question::State
        )
    }

    fn max_len(self) -> usize {
        match self {
            Question::StreamUrl => 500,
            Question::CallSign | Question::Frequency => 16,
            Question::State => 32,
            Question::Genre => 40,
            Question::Name | Question::City => 60,
        }
    }

    fn answer(self, suggestion: &mut StationSuggestion) -> &mut String {
        match self {
            Question::Name => &mut suggestion.name,
            Question::StreamUrl => &mut suggestion.stream_url,
            Question::CallSign => &mut suggestion.call_sign,
            Question::State => &mut suggestion.state,
            Question::City => &mut suggestion.city,
            Question::Frequency => &mut suggestion.frequency,
            Question::Genre => &mut suggestion.genre,
        }
    }

    /// The question a site refusal is about, if it is about one.
    pub fn for_reason(reason: &str) -> Option<Question> {
        Some(match reason {
            "invalid_name" | "name_too_long" => Question::Name,
            "invalid_call_sign" => Question::CallSign,
            "invalid_state" => Question::State,
            "invalid_frequency" => Question::Frequency,
            "city_too_long" => Question::City,
            "genre_too_long" => Question::Genre,
            r if r == "invalid_stream_url"
                || r.starts_with("stream_")
                || r.starts_with("duplicate_")
                || r == "already_declined" =>
            {
                Question::StreamUrl
            }
            _ => return None,
        })
    }
}

/// The questions for a kind of station, in order.
pub fn questions(kind: &str) -> &'static [Question] {
    if kind == "terrestrial" {
        &[
            Question::Name,
            Question::StreamUrl,
            Question::CallSign,
            Question::State,
            Question::City,
            Question::Frequency,
            Question::Genre,
        ]
    } else {
        &[Question::Name, Question::StreamUrl, Question::Genre]
    }
}

/// The site's answer and, when it turned something down, which reason.
type Answer = (SuggestionOutcome, String);

/// The text field that walks the questions, then sends.
pub struct StationSuggestionEntryState {
    entry: TextEntryCore,
    suggestion: StationSuggestion,
    step: usize,
    identity: OnlineIdentity,
    sender: SuggestionSender,
    sending: bool,
    answer: Mailbox<Answer>,
    answered: Arc<AtomicBool>,
    /// False in tests: the send runs inline, and the next update reads it.
    pub threaded: bool,
}

impl StationSuggestionEntryState {
    pub fn new(kind: &str, identity: OnlineIdentity, sender: SuggestionSender) -> Self {
        let mut state = Self {
            entry: TextEntryCore::new("Suggest a station", ""),
            suggestion: StationSuggestion {
                kind: kind.to_string(),
                ..Default::default()
            },
            step: 0,
            identity,
            sender,
            sending: false,
            answer: Mailbox::new(),
            answered: Arc::new(AtomicBool::new(false)),
            threaded: true,
        };
        state.load_step(0);
        state
    }

    /// The suggestion as answered so far.
    pub fn suggestion(&self) -> &StationSuggestion {
        &self.suggestion
    }

    /// The question on screen.
    pub fn question(&self) -> Question {
        questions(&self.suggestion.kind)[self.step]
    }

    pub fn is_sending(&self) -> bool {
        self.sending
    }

    /// Put question `step` in the field, with any earlier answer to it.
    fn load_step(&mut self, step: usize) {
        self.step = step;
        let question = self.question();
        let previous = question.answer(&mut self.suggestion).clone();
        self.entry.field_label = question.heading().to_string();
        self.entry.max_len = question.max_len();
        self.entry.set_text(&previous);
    }

    fn prompt(&self) -> String {
        let question = self.question();
        let typed = self.entry.text();
        let mut text = format!("{}.", question.heading());
        if !question.hint().is_empty() {
            text.push(' ');
            text.push_str(question.hint());
        }
        if !typed.is_empty() {
            text.push_str(&format!(" Filled in: {typed}."));
        }
        text
    }

    fn send(&mut self) {
        self.sending = true;
        let payload = self.suggestion.payload(&self.identity.driver_id);
        let identity = self.identity.clone();
        let sender = self.sender.clone();
        let answer = self.answer.clone();
        let answered = Arc::clone(&self.answered);
        run_worker(self.threaded, "station-suggestion", move || {
            let reply = sender(&payload, &identity);
            let reason = match &reply {
                Ok(body) => body
                    .get("reason")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string(),
                Err(e) => e
                    .error_body()
                    .get("reason")
                    .and_then(|r| r.as_str())
                    .unwrap_or("")
                    .to_string(),
            };
            answer.post((suggestion_outcome(reply), reason));
            answered.store(true, Ordering::SeqCst);
        });
    }

    /// The site has answered: say it, then close or go back to the answer
    /// it turned down.
    fn take_answer(&mut self, ctx: &mut GameContext) {
        if !self.answered.load(Ordering::SeqCst) {
            return;
        }
        let Some((outcome, reason)) = self.answer.take() else {
            return;
        };
        self.sending = false;
        self.answered.store(false, Ordering::SeqCst);
        if outcome.accepted {
            ctx.audio.play("ui/menu_select");
            ctx.say(&outcome.message);
            ctx.pop_state_with(true, false);
            return;
        }
        ctx.audio.play("ui/error");
        let back_to = Question::for_reason(&reason).and_then(|q| {
            questions(&self.suggestion.kind)
                .iter()
                .position(|&x| x == q)
        });
        match back_to {
            Some(step) => {
                self.load_step(step);
                ctx.say(&format!("{} {}", outcome.message, self.prompt()));
            }
            None => {
                ctx.say(&outcome.message);
                ctx.pop_state_with(true, false);
            }
        }
    }
}

impl TextEntry for StationSuggestionEntryState {
    fn entry(&self) -> &TextEntryCore {
        &self.entry
    }

    fn entry_mut(&mut self) -> &mut TextEntryCore {
        &mut self.entry
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        ctx.say(&format!(
            "{} Type each answer, then Enter. Left and Right review the letters, Escape \
             cancels.{}",
            self.prompt(),
            crate::states::text_entry::KEYBOARD_HINT
        ));
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        ctx.update_music_rotation(dt);
        self.take_answer(ctx);
    }

    fn input_paused(&self) -> bool {
        self.sending
    }

    fn confirm(&mut self, ctx: &mut GameContext) {
        let question = self.question();
        let typed = self
            .entry
            .text()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if question.required() && typed.is_empty() {
            ctx.audio.play("ui/error");
            ctx.say_with(
                format!("{} is needed.", question.heading()),
                Say::new().review(false),
            );
            return;
        }
        if question == Question::StreamUrl && !is_stream_address(&typed) {
            ctx.audio.play("ui/error");
            ctx.say_with(NOT_A_STREAM_ADDRESS, Say::new().review(false));
            return;
        }
        *question.answer(&mut self.suggestion) = typed;
        if self.step + 1 < questions(&self.suggestion.kind).len() {
            ctx.audio.play("ui/menu_select");
            self.load_step(self.step + 1);
            let prompt = self.prompt();
            ctx.say(&prompt);
            return;
        }
        ctx.audio.play("ui/menu_select");
        ctx.say("Checking the stream.");
        self.send();
    }
}

impl_state_for_text_entry!(StationSuggestionEntryState);
