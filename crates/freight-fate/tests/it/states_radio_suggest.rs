//! Suggest a station, from the Radio app: the questions a player is walked
//! through, what is sent, and what is said with the site's answer.

use std::sync::{Arc, Mutex};

use freight_fate::app::testing::TestApp;
use freight_fate::community_stations::SuggestionSender;
use freight_fate::net::NetError;
use freight_fate::online_presence::OnlineIdentity;
use freight_fate::states::base::{InputEvent, Key, Mods, State};
use freight_fate::states::driving_radio_suggest::{
    open_station_suggestion, Question, StationSuggestionEntryState, SuggestKindState,
    NOT_A_STREAM_ADDRESS, SUGGEST_NEEDS_ONLINE, SUGGEST_NEEDS_SETUP,
};
use serde_json::{json, Value};

/// A placeholder under the suggestion screens, so closing them has
/// somewhere to land.
struct Under;

impl State for Under {}

type Sent = Arc<Mutex<Vec<Value>>>;

/// A sender that records each payload and answers from `replies` in turn.
fn sender(replies: Vec<Result<Value, NetError>>) -> (SuggestionSender, Sent) {
    let sent: Sent = Arc::default();
    let replies = Mutex::new(replies.into_iter());
    let log = Arc::clone(&sent);
    let send: SuggestionSender = Arc::new(move |payload: &Value, identity: &OnlineIdentity| {
        assert_eq!(identity.driver_id, "night-owl-1234");
        log.lock().unwrap().push(payload.clone());
        replies
            .lock()
            .unwrap()
            .next()
            .expect("a reply for every send")
    });
    (send, sent)
}

fn identity() -> OnlineIdentity {
    OnlineIdentity::new("night-owl-1234", &"t".repeat(32))
}

fn last(app: &TestApp) -> String {
    app.main_lines().last().cloned().unwrap_or_default()
}

fn top_is<T: 'static>(app: &TestApp) -> bool {
    app.ctx
        .state()
        .is_some_and(|state| state.borrow().as_any().is::<T>())
}

fn with_entry<R>(app: &mut TestApp, f: impl FnOnce(&mut StationSuggestionEntryState) -> R) -> R {
    let state = app.ctx.state().expect("a state");
    let mut borrowed = state.borrow_mut();
    f(borrowed
        .as_any_mut()
        .downcast_mut::<StationSuggestionEntryState>()
        .expect("the suggestion field is on top"))
}

fn answer(app: &mut TestApp, text: &str) {
    for ch in text.chars() {
        app.handle_event(&InputEvent::typed(ch));
    }
    app.handle_event(&InputEvent::key(Key::Return));
}

/// Open the kind menu and choose a row: the first is a broadcast station,
/// the second an online-only one.
fn start(app: &mut TestApp, send: SuggestionSender, row: &str) {
    app.ctx.push_state(Under);
    app.ctx.push_state(SuggestKindState::new(identity(), send));
    if row == "Online only" {
        app.handle_event(&InputEvent::key(Key::Down));
    }
    app.handle_event(&InputEvent::key(Key::Return));
    with_entry(app, |e| e.threaded = false);
}

#[test]
fn a_web_station_asks_three_questions_and_says_the_sites_answer() {
    let mut app = TestApp::new();
    let (send, sent) = sender(vec![Ok(
        json!({"ok": true, "message": "Thanks, it is waiting for review."}),
    )]);
    start(&mut app, send, "Online only");
    assert!(top_is::<StationSuggestionEntryState>(&app));
    assert!(last(&app).starts_with("Station name."), "{}", last(&app));

    answer(&mut app, "Night Owl Radio");
    assert!(
        last(&app).starts_with("Stream address. The direct link"),
        "{}",
        last(&app)
    );
    app.ctx
        .clipboard
        .set_text("  https://s.example/live\nsecond line");
    app.handle_event(&InputEvent::key_mods(Key::V, Mods::CTRL));
    assert_eq!(last(&app), "Pasted https://s.example/live");
    app.handle_event(&InputEvent::key(Key::Return));
    assert!(last(&app).starts_with("Format. Optional"), "{}", last(&app));
    app.handle_event(&InputEvent::key(Key::Return));
    assert_eq!(last(&app), "Checking the stream.");

    app.tick(0.016);
    assert_eq!(last(&app), "Thanks, it is waiting for review.");
    assert!(top_is::<Under>(&app));
    assert_eq!(
        sent.lock().unwrap().as_slice(),
        [json!({
            "driverId": "night-owl-1234", "kind": "web", "name": "Night Owl Radio",
            "streamUrl": "https://s.example/live",
        })]
    );
}

#[test]
fn a_broadcast_station_also_asks_where_it_is() {
    let mut app = TestApp::new();
    let (send, sent) = sender(vec![Ok(json!({"ok": true, "message": "Thanks."}))]);
    start(&mut app, send, "On AM or FM, and online");
    for text in [
        "The Cat",
        "https://icecast.example/thecat",
        "KWSC",
        "Nebraska",
        "",
        "91.9",
        "",
    ] {
        answer(&mut app, text);
    }
    app.tick(0.016);
    assert_eq!(
        sent.lock().unwrap().as_slice(),
        [json!({
            "driverId": "night-owl-1234", "kind": "terrestrial", "name": "The Cat",
            "streamUrl": "https://icecast.example/thecat", "callSign": "KWSC",
            "state": "Nebraska", "frequency": "91.9",
        })]
    );
}

#[test]
fn a_required_answer_cannot_be_skipped() {
    let mut app = TestApp::new();
    let (send, _) = sender(vec![]);
    start(&mut app, send, "Online only");
    app.handle_event(&InputEvent::key(Key::Return));
    assert_eq!(last(&app), "Station name is needed.");
    assert_eq!(with_entry(&mut app, |e| e.question()), Question::Name);
}

#[test]
fn a_stream_address_that_is_not_a_web_address_is_turned_down_at_once() {
    let mut app = TestApp::new();
    let (send, sent) = sender(vec![Ok(json!({"ok": true, "message": "Thanks."}))]);
    start(&mut app, send, "Online only");
    answer(&mut app, "Night Owl Radio");
    for junk in [
        "weklsetr",
        "ftp://s.example/live",
        "https://user:pw@s.example/",
    ] {
        answer(&mut app, junk);
        assert_eq!(last(&app), NOT_A_STREAM_ADDRESS, "{junk}");
        assert_eq!(with_entry(&mut app, |e| e.question()), Question::StreamUrl);
        for _ in 0..junk.len() {
            app.handle_event(&InputEvent::key(Key::Backspace));
        }
    }
    answer(&mut app, "http://s.example:8000/live");
    assert!(last(&app).starts_with("Format. Optional"), "{}", last(&app));
    assert!(sent.lock().unwrap().is_empty());
}

#[test]
fn an_address_typed_by_hand_names_its_punctuation() {
    let mut app = TestApp::new();
    let (send, _) = sender(vec![]);
    start(&mut app, send, "Online only");
    answer(&mut app, "Night Owl Radio");
    let mut heard = Vec::new();
    for ch in "http://a.b".chars() {
        app.handle_event(&InputEvent::typed(ch));
        heard.push(last(&app));
    }
    assert_eq!(
        heard,
        ["h", "t", "t", "p", "colon", "slash", "slash", "a", "dot", "b"]
    );
    app.handle_event(&InputEvent::key(Key::Return));
    assert!(last(&app).starts_with("Format. Optional"), "{}", last(&app));
}

#[test]
fn a_turned_down_answer_goes_back_to_its_question_with_the_rest_kept() {
    let mut app = TestApp::new();
    let (send, sent) = sender(vec![
        Err(NetError::http_json(
            422,
            &json!({"ok": false, "reason": "stream_web_page", "message": "That address is a web page, not a stream."}),
        )),
        Ok(json!({"ok": true, "message": "Thanks."})),
    ]);
    start(&mut app, send, "Online only");
    for text in ["Night Owl Radio", "https://nightowl.example", "jazz"] {
        answer(&mut app, text);
    }
    app.tick(0.016);
    assert!(top_is::<StationSuggestionEntryState>(&app));
    assert_eq!(with_entry(&mut app, |e| e.question()), Question::StreamUrl);
    assert!(
        last(&app).starts_with("That address is a web page, not a stream. Stream address."),
        "{}",
        last(&app)
    );
    assert!(
        last(&app).ends_with("Filled in: https://nightowl.example."),
        "{}",
        last(&app)
    );

    // Fix the address; the format answer is still there for Enter to keep.
    for _ in 0.."https://nightowl.example".len() {
        app.handle_event(&InputEvent::key(Key::Backspace));
    }
    answer(&mut app, "https://nightowl.example/live.mp3");
    app.handle_event(&InputEvent::key(Key::Return));
    app.tick(0.016);
    assert_eq!(sent.lock().unwrap()[1]["genre"], json!("jazz"));
    assert!(top_is::<Under>(&app));
}

#[test]
fn a_refusal_about_no_answer_closes_the_screens() {
    let mut app = TestApp::new();
    let (send, _) = sender(vec![Err(NetError::http_json(
        429,
        &json!({"ok": false, "reason": "daily_limit", "message": "That's all the suggestions for today."}),
    ))]);
    start(&mut app, send, "Online only");
    for text in ["Night Owl Radio", "https://s.example/live", ""] {
        answer(&mut app, text);
    }
    app.tick(0.016);
    assert_eq!(last(&app), "That's all the suggestions for today.");
    assert!(top_is::<Under>(&app));
}

#[test]
fn the_screens_say_why_they_cannot_open() {
    let mut app = TestApp::new();
    app.ctx.settings.online_services = false;
    open_station_suggestion(&mut app.ctx);
    assert_eq!(last(&app), SUGGEST_NEEDS_ONLINE);

    app.ctx.settings.online_services = true;
    open_station_suggestion(&mut app.ctx);
    assert_eq!(last(&app), SUGGEST_NEEDS_SETUP);
}
