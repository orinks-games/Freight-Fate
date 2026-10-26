//! The `--agent-server` handshake happens without a game. Every Claude Code
//! session start spawns each configured MCP server just to ask for its
//! tool list, and the first shipped server booted the real game (sandbox,
//! single-instance lock, window, audio, speech) BEFORE it read a byte of
//! stdin -- so enabling the server launched a game window into every
//! session and held the one-game-at-a-time lock against the owner (found
//! live, 2026-09-01). Now the handshake is answered from the serve thread
//! alone, the game boots at the first play request, and a client that
//! hangs up takes the game down with it.

use std::io::Cursor;
use std::sync::mpsc;

use freight_fate::agent_server::{
    await_play_request, await_play_request_for, build_command, install_ears, policy,
    policy_with_lifeline, serve_lines, serve_lines_with_lifeline, spawn_watch_with, Command,
    CruiseTarget, Ears, Lifeline,
};
use freight_fate::app::testing::TestApp;
use freight_fate::states::base::Key;

const FRAME: f64 = 1.0 / 60.0;

fn rpc(id: u64, method: &str, params: &str) -> String {
    format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"{method}","params":{params}}}"#)
}

fn call(id: u64, tool: &str, args: &str) -> String {
    rpc(
        id,
        "tools/call",
        &format!(r#"{{"name":"{tool}","arguments":{args}}}"#),
    )
}

fn results(out: &[u8]) -> Vec<serde_json::Value> {
    String::from_utf8_lossy(out)
        .lines()
        .map(|line| serde_json::from_str(line).expect("one JSON-RPC message per line"))
        .collect()
}

#[test]
fn the_handshake_never_asks_for_a_game() {
    let script = [
        rpc(1, "initialize", r#"{"protocolVersion":"2025-06-18"}"#),
        rpc(2, "tools/list", "{}"),
        rpc(3, "ping", "{}"),
    ]
    .join("\n");
    let (tx, rx) = mpsc::channel();
    let mut out = Vec::new();
    serve_lines(Cursor::new(script), &mut out, &tx);

    let answered = results(&out);
    assert_eq!(answered.len(), 3, "every handshake message is answered");
    assert_eq!(
        answered[0]["result"]["serverInfo"]["name"],
        "freight-fate-agent"
    );
    assert!(
        answered[0]["result"]["instructions"]
            .as_str()
            .is_some_and(|text| text.contains("boots")),
        "the client is told the game boots at the first play call"
    );
    assert!(answered[1]["result"]["tools"].is_array());
    assert!(
        matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)),
        "no handshake message reaches the game loop"
    );
}

#[test]
fn the_first_play_call_is_what_wakes_the_game() {
    let script = [
        rpc(1, "initialize", r#"{"protocolVersion":"2025-06-18"}"#),
        rpc(2, "tools/list", "{}"),
        call(3, "press", r#"{"key":"enter"}"#),
    ]
    .join("\n");
    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let mut out = Vec::new();
        serve_lines(Cursor::new(script), &mut out, &tx);
        out
    });

    let request = await_play_request(&rx).expect("the press is a play request");
    assert!(matches!(request.command(), Command::Press { .. }));
    request.answer(Ok("the game is up".to_string()));

    let answered = results(&server.join().unwrap());
    assert_eq!(answered.len(), 3);
    assert_eq!(
        answered[2]["result"]["content"][0]["text"],
        "the game is up"
    );
}

#[test]
fn quitting_before_the_game_exists_ends_the_server() {
    // An idle server still holds the release executable open, so a quit with
    // no game up has to end the process too, not wait for the next call.
    let script = [call(1, "quit_game", "{}"), call(2, "listen", "{}")].join("\n");
    let (tx, rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let mut out = Vec::new();
        serve_lines(Cursor::new(script), &mut out, &tx);
        out
    });

    assert!(
        await_play_request(&rx).is_none(),
        "a quit with no game ends the server instead of booting one"
    );

    let answered = results(&server.join().unwrap());
    assert_eq!(
        answered.len(),
        1,
        "nothing after the quit is read: {answered:?}"
    );
    let quit_text = answered[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(
        quit_text.contains("server has ended"),
        "the quit is answered before the process goes: {quit_text:?}"
    );
}

#[test]
fn a_client_that_hangs_up_before_playing_ends_the_wait() {
    let (tx, rx) = mpsc::channel::<freight_fate::agent_server::Request>();
    drop(tx);
    assert!(
        await_play_request(&rx).is_none(),
        "stdin closing with no play request means no game, ever"
    );
}

#[test]
fn eof_cuts_the_lifeline() {
    let (tx, _rx) = mpsc::channel();
    let lifeline = Lifeline::new();
    serve_lines_with_lifeline(Cursor::new(""), &mut Vec::new(), &tx, &lifeline);
    assert!(lifeline.is_cut());
}

struct BrokenPipe;
impl std::io::Write for BrokenPipe {
    fn write(&mut self, _bytes: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::from(std::io::ErrorKind::BrokenPipe))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn broken_output_cuts_the_lifeline_without_reading_later_calls() {
    let (tx, rx) = mpsc::channel();
    let lifeline = Lifeline::new();
    serve_lines_with_lifeline(
        Cursor::new(format!(
            "{}\n{}",
            rpc(1, "ping", "{}"),
            call(2, "listen", "{}")
        )),
        &mut BrokenPipe,
        &tx,
        &lifeline,
    );
    assert!(lifeline.is_cut());
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
}

#[test]
fn watch_wakes_a_waiting_server_when_the_lifeline_is_cut() {
    let (tx, rx) = mpsc::channel();
    let lifeline = Lifeline::new();
    lifeline.cut("test client left");
    let watch = spawn_watch_with(lifeline, tx, || true, std::time::Duration::from_millis(10));
    assert!(await_play_request_for(&rx, std::time::Duration::from_millis(100)).is_none());
    watch.join().unwrap();
}

#[test]
fn dead_parent_cuts_the_lifeline() {
    let (tx, _rx) = mpsc::channel();
    let lifeline = Lifeline::new();
    let watch = spawn_watch_with(
        lifeline.clone(),
        tx,
        || false,
        std::time::Duration::from_millis(10),
    );
    watch.join().unwrap();
    assert_eq!(
        lifeline.reason().as_deref(),
        Some("the process that started the server has exited")
    );
}

#[test]
fn a_cut_lifeline_ends_a_pending_wait() {
    let mut app = TestApp::new();
    let (tx, rx) = mpsc::channel();
    let lifeline = Lifeline::new();
    let (reply, answer) = mpsc::channel();
    tx.send(freight_fate::agent_server::Request::for_test(
        Command::Wait { seconds: 30.0 },
        reply,
    ))
    .unwrap();
    let mut agent = policy_with_lifeline(Ears::shared(), rx, None, lifeline.clone());
    app.run_with_player_input(Some(1), |input, dt| agent.step(input, dt));
    lifeline.cut("test client left");
    app.run_with_player_input(Some(1), |input, dt| agent.step(input, dt));
    assert!(answer
        .try_recv()
        .expect("the pending wait is answered at once")
        .unwrap_err()
        .contains("MCP client is gone"));
}

#[test]
fn an_idle_server_ends_instead_of_holding_the_executable() {
    let (_tx, rx) = mpsc::channel::<freight_fate::agent_server::Request>();
    assert!(
        await_play_request_for(&rx, std::time::Duration::from_millis(20)).is_none(),
        "a session that never plays lets its server go, though stdin stays open"
    );
}

#[test]
fn a_client_that_hangs_up_mid_game_quits_it() {
    let mut app = TestApp::new();
    let (tx, rx) = mpsc::channel();
    let ears = Ears::shared();
    let mut agent = policy(ears, rx, None);
    drop(tx);
    let mut frames = 0;
    app.run_with_player_input(Some(30), |input, dt| {
        frames += 1;
        agent.step(input, dt)
    });
    assert_eq!(
        frames, 1,
        "the game loop ends on the first frame after the client is gone"
    );
}

#[test]
fn a_tap_lands_as_a_finger_tap_not_a_screen_reader_hold() {
    // Two frames, down then up. A press and release inside one frame is
    // what JAWS re-injects for a held key, and the held-key tracker holds
    // such a key for the repeat delay: a tapped brake read as the
    // reverse-selection hold and a tapped P fought the approach assist
    // (found live, 2026-09-01).
    let mut app = TestApp::new();
    let (tx, rx) = mpsc::channel();
    let server_tx = tx.clone(); // the test keeps `tx` so the client never "hangs up"
    let server = std::thread::spawn(move || {
        let mut out = Vec::new();
        serve_lines(
            Cursor::new(call(1, "press", r#"{"key":"space"}"#)),
            &mut out,
            &server_tx,
        );
        out
    });
    let first = await_play_request(&rx).expect("the press wakes the game");
    let mut agent = policy(Ears::shared(), rx, Some(first));
    let mut readings = Vec::new();
    app.run_with_player_input(Some(4), |input, dt| {
        readings.push(input.key_reads_pressed(Key::Space));
        agent.step(input, dt)
    });
    drop(tx);
    let _ = server.join();
    // Frame 1 reads the request, frame 2 sends the down, frame 3 the up.
    assert_eq!(
        readings,
        [false, false, true, false],
        "down for exactly one frame, then released for good (no half-second pulse)"
    );
}

// -- the tools that drive the whole game ---------------------------------------------
//
// Each of these exists because the raw keys could not do the job at pace: a
// hold-then-release through the client is a second or more of round trip,
// and at standard pacing that is twenty seconds of road (agent drive,
// 2026-09-02: every throttle tap overshot the limit and every brake landed
// late; four loop-backs at one gate).

/// The first tool call of a session, served to a policy over a live app.
fn wake(
    script: String,
) -> (
    mpsc::Sender<freight_fate::agent_server::Request>,
    std::thread::JoinHandle<Vec<u8>>,
    freight_fate::agent_server::AgentPolicy,
) {
    let (tx, rx) = mpsc::channel();
    let server_tx = tx.clone();
    let server = std::thread::spawn(move || {
        let mut out = Vec::new();
        serve_lines(Cursor::new(script), &mut out, &server_tx);
        out
    });
    let first = await_play_request(&rx).expect("the call wakes the game");
    (tx, server, policy(Ears::shared(), rx, Some(first)))
}

fn reply_text(answered: &[serde_json::Value]) -> String {
    answered[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_string()
}

#[test]
fn a_pedal_is_lifted_by_the_loop_not_the_client() {
    let mut app = TestApp::new();
    let (tx, server, mut agent) = wake(call(1, "pedal", r#"{"key":"up","seconds":0.05}"#));
    let mut readings = Vec::new();
    // Frames are clocked at 60 fps by hand, so the pedal's seconds are frames.
    app.run_with_player_input(Some(60), |input, _dt| {
        readings.push(input.key_reads_pressed(Key::Up));
        agent.step(input, FRAME)
    });
    drop(tx);
    let answered = results(&server.join().unwrap());
    let down = readings.iter().filter(|held| **held).count();
    assert!(
        (2..=6).contains(&down),
        "held for about three frames: {readings:?}"
    );
    assert!(
        !readings[readings.len() - 1],
        "and lifted with no release call"
    );
    assert_eq!(answered.len(), 1, "the reply arrived once the pedal was up");
    assert!(answered[0]["result"]["isError"].is_null(), "{answered:?}");
}

#[test]
fn wait_for_returns_the_moment_a_menu_is_on_screen() {
    let mut app = TestApp::new();
    let (tx, server, mut agent) = wake(call(1, "wait_for", r#"{"menu":true,"seconds":100}"#));
    let mut frames = 0;
    app.run_with_player_input(Some(10), |input, _dt| {
        frames += 1;
        agent.step(input, FRAME)
    });
    drop(tx);
    let answered = results(&server.join().unwrap());
    assert_eq!(
        answered.len(),
        1,
        "the title menu is a menu: answered at once"
    );
    assert!(
        !reply_text(&answered).contains("clock ran out"),
        "{}",
        reply_text(&answered)
    );
}

#[test]
fn wait_for_says_when_the_clock_ran_out() {
    let mut app = TestApp::new();
    let (tx, server, mut agent) = wake(call(
        1,
        "wait_for",
        r#"{"text":"never spoken","seconds":0.1}"#,
    ));
    app.run_with_player_input(Some(20), |input, _dt| agent.step(input, FRAME));
    drop(tx);
    let answered = results(&server.join().unwrap());
    assert_eq!(answered.len(), 1);
    assert!(
        reply_text(&answered).contains("clock ran out"),
        "{}",
        reply_text(&answered)
    );
}

#[test]
fn wait_for_hears_a_line_through_the_ears() {
    // The ears are the tee over the real speech sink; a menu move speaks
    // the row it lands on, and the wait releases on it.
    let mut app = TestApp::new();
    let ears = install_ears(&mut app);
    let (tx, rx) = mpsc::channel();
    let server_tx = tx.clone();
    let script = [
        call(1, "press", r#"{"key":"down"}"#),
        call(2, "wait_for", r#"{"text":"2 of","seconds":100}"#),
    ]
    .join("\n");
    let server = std::thread::spawn(move || {
        let mut out = Vec::new();
        serve_lines(Cursor::new(script), &mut out, &server_tx);
        out
    });
    let first = await_play_request(&rx).expect("the press wakes the game");
    let mut agent = policy(ears, rx, Some(first));
    app.run_with_player_input(Some(30), |input, _dt| agent.step(input, FRAME));
    drop(tx);
    let answered = results(&server.join().unwrap());
    assert_eq!(answered.len(), 2, "{answered:?}");
    let heard = answered[1]["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or_default();
    assert!(heard.contains("2 of"), "{heard}");
    assert!(!heard.contains("clock ran out"), "{heard}");
}

#[test]
fn select_walks_to_the_row_by_part_of_its_label() {
    let mut app = TestApp::new();
    let (tx, server, mut agent) = wake(call(1, "select", r#"{"label":"how to play"}"#));
    let mut downs = 0;
    let mut enters = 0;
    app.run_with_player_input(Some(40), |input, _dt| {
        if input.key_reads_pressed(Key::Down) {
            downs += 1;
        }
        if input.key_reads_pressed(Key::Return) {
            enters += 1;
        }
        agent.step(input, FRAME)
    });
    drop(tx);
    let answered = results(&server.join().unwrap());
    let text = reply_text(&answered);
    assert!(text.starts_with("Selecting row "), "{text}");
    assert!(text.contains("How to play"), "{text}");
    assert!(downs >= 1, "arrowed down to it");
    assert_eq!(enters, 1, "and pressed Enter once");
}

#[test]
fn select_names_the_rows_when_nothing_matches() {
    let mut app = TestApp::new();
    let (tx, server, mut agent) = wake(call(1, "select", r#"{"label":"no such row"}"#));
    app.run_with_player_input(Some(3), |input, _dt| agent.step(input, FRAME));
    drop(tx);
    let answered = results(&server.join().unwrap());
    assert_eq!(answered[0]["result"]["isError"], true, "{answered:?}");
    let text = reply_text(&answered);
    assert!(text.contains("The rows are:"), "{text}");
    assert!(text.contains("1. "), "{text}");
}

#[test]
fn cruise_off_the_road_is_refused_not_hung() {
    let mut app = TestApp::new();
    let (tx, server, mut agent) = wake(call(1, "cruise", r#"{"target":55}"#));
    app.run_with_player_input(Some(3), |input, _dt| agent.step(input, FRAME));
    drop(tx);
    let answered = results(&server.join().unwrap());
    assert_eq!(answered[0]["result"]["isError"], true, "{answered:?}");
    assert!(reply_text(&answered).contains("Not at the wheel"));
}

#[test]
fn cruise_targets_parse_as_a_number_the_limit_or_off() {
    let target = |args: &str| {
        let args = serde_json::from_str(args).unwrap();
        match build_command("cruise", &args) {
            Ok(Command::Cruise { target }) => Ok(target),
            Ok(_) => panic!("not a cruise command"),
            Err(text) => Err(text),
        }
    };
    assert_eq!(target(r#"{"target":55}"#), Ok(CruiseTarget::Mph(55.0)));
    assert_eq!(target(r#"{"target":"60"}"#), Ok(CruiseTarget::Mph(60.0)));
    assert_eq!(target(r#"{"target":"limit"}"#), Ok(CruiseTarget::Limit));
    assert_eq!(target(r#"{"target":"OFF"}"#), Ok(CruiseTarget::Off));
    assert!(target(r#"{"target":"fast"}"#).is_err());
    assert!(target(r#"{"target":0}"#).is_err());
    assert!(target(r#"{}"#).is_err());
}

#[test]
fn operator_keys_parse_as_a_boolean_and_refuse_anything_else() {
    let live = |args: &str| {
        let args = serde_json::from_str(args).unwrap();
        match build_command("operator_keys", &args) {
            Ok(Command::OperatorKeys { live }) => Ok(live),
            Ok(_) => panic!("not an operator_keys command"),
            Err(text) => Err(text),
        }
    };
    assert_eq!(live(r#"{"live":true}"#), Ok(true));
    assert_eq!(live(r#"{"live":false}"#), Ok(false));
    assert!(live(r#"{"live":"yes"}"#).is_err());
    assert!(live(r#"{}"#).is_err());
}

#[test]
fn lockstep_holds_the_world_between_calls_and_lets_a_wait_run() {
    // The agent's thinking time is a round trip of a second or two; with
    // lane keeping off that was the truck across half a lane before its
    // answer to a cue landed (agent drive, 2026-09-23). Lockstep holds the
    // world whenever no call is working, and a wait still runs its frames.
    let script = [
        call(1, "lockstep", r#"{"on":true}"#),
        call(2, "wait", r#"{"seconds":0.1}"#),
    ]
    .join("\n");
    let mut app = TestApp::new();
    let (tx, server, mut agent) = wake(script);
    let mut held = Vec::new();
    app.run_with_player_input(Some(600), |input, _dt| {
        let running = agent.step(input, FRAME);
        held.push(input.world_is_held());
        running
    });
    drop(tx);
    let answered = results(&server.join().unwrap());
    assert_eq!(answered.len(), 2, "both calls answered: {answered:?}");
    assert!(held[0], "idle after switching on: the world waits");
    let ran = held.iter().filter(|h| !**h).count();
    assert!(
        (5..=7).contains(&ran),
        "a 0.1 s wait runs about six frames; ran {ran}"
    );
    assert!(held[held.len() - 1], "and the world waits again after it");
}

#[test]
fn lockstep_parses_as_a_boolean_and_is_listed() {
    let on = |args: &str| {
        let args = serde_json::from_str(args).unwrap();
        match build_command("lockstep", &args) {
            Ok(Command::Lockstep { on }) => Ok(on),
            Ok(_) => panic!("not a lockstep command"),
            Err(text) => Err(text),
        }
    };
    assert_eq!(on(r#"{"on":true}"#), Ok(true));
    assert_eq!(on(r#"{"on":false}"#), Ok(false));
    assert!(on(r#"{}"#).is_err());
}

#[test]
fn pedal_and_wait_for_refuse_a_missing_duration() {
    let args = serde_json::from_str(r#"{"key":"up"}"#).unwrap();
    assert!(build_command("pedal", &args).is_err());
    let args = serde_json::from_str(r#"{"text":"Arrived"}"#).unwrap();
    assert!(build_command("wait_for", &args).is_err());
    let args = serde_json::from_str(r#"{"label":"  "}"#).unwrap();
    assert!(build_command("select", &args).is_err());
}

#[test]
fn key_probe_parses_start_and_report_and_refuses_anything_else() {
    for (action, start) in [("start", true), ("report", false)] {
        let mut args = serde_json::Map::new();
        args.insert("action".into(), action.into());
        match build_command("key_probe", &args) {
            Ok(Command::KeyProbe { start: got }) => assert_eq!(got, start),
            _ => panic!("{action} is not a key_probe command"),
        }
    }
    assert!(build_command("key_probe", &serde_json::Map::new()).is_err());
}

#[test]
fn the_tool_list_carries_the_driving_tools() {
    let script = rpc(1, "tools/list", "{}");
    let (tx, _rx) = mpsc::channel();
    let mut out = Vec::new();
    serve_lines(Cursor::new(script), &mut out, &tx);
    let answered = results(&out);
    let names: Vec<String> = answered[0]["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .map(|tool| tool["name"].as_str().unwrap_or_default().to_string())
        .collect();
    for name in [
        "pedal",
        "wait_for",
        "select",
        "cruise",
        "status",
        "operator_keys",
        "lockstep",
        "key_probe",
    ] {
        assert!(names.iter().any(|n| n == name), "{name} in {names:?}");
    }
}
