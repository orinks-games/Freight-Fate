use std::io::{BufRead, Write};
use std::sync::mpsc;

use serde_json::{json, Map, Value};

use crate::states::base::{Key, Mods};

use super::{Command, CruiseTarget, KeySpec, Lifeline, Request};
use crate::bindings::Action;

const SERVER_NAME: &str = "freight-fate-agent";
const PROTOCOL_VERSION: &str = "2025-06-18";
const REPLY_TIMEOUT_SECONDS: u64 = 330;
// -- the MCP stdio thread -------------------------------------------------------------

fn respond_raw(out: &mut dyn Write, value: &Value) -> bool {
    writeln!(out, "{value}").and_then(|_| out.flush()).is_ok()
}

/// The `lockstep` tool's reply.
pub(super) fn lockstep_reply(on: bool) -> String {
    if on {
        "Lockstep is on: the world waits between tool calls. Time passes only inside \
         wait, pedal, wait_for, and the frames a call scripts."
    } else {
        "Lockstep is off: the road runs on the wall clock again."
    }
    .to_string()
}

fn tool_text(text: String) -> Value {
    json!({"content": [{"type": "text", "text": text}]})
}

fn tool_error(text: String) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": true})
}

fn parse_key(name: &str) -> Option<(Key, Option<char>)> {
    let lowered = name.to_ascii_lowercase();
    if lowered.chars().count() == 1 {
        let ch = lowered.chars().next().unwrap();
        if ch.is_ascii_alphanumeric() {
            return Some((Key::from_char(ch), Some(ch)));
        }
    }
    let key = match lowered.as_str() {
        "up" => Key::Up,
        "down" => Key::Down,
        "left" => Key::Left,
        "right" => Key::Right,
        "enter" | "return" => Key::Return,
        "escape" | "esc" => Key::Escape,
        "space" => Key::Space,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "f1" => Key::F1,
        "f2" => Key::F2,
        "control" | "ctrl" => Key::LCtrl,
        "shift" => Key::LShift,
        "+" | "plus" => return Some((Key::Plus, Some('+'))),
        "-" | "minus" => return Some((Key::Minus, Some('-'))),
        "comma" => Key::Comma,
        "period" => Key::Period,
        _ => return None,
    };
    Some((key, None))
}

fn tools_list() -> Value {
    let tool = |name: &str, description: &str, properties: Value, required: &[&str]| {
        json!({
            "name": name,
            "description": description,
            "inputSchema": {"type": "object", "properties": properties, "required": required},
        })
    };
    json!({"tools": [
        tool(
            "press",
            "Tap a key, as a player would: letters a-z, digits, up, down, left, right, \
             enter, escape, space, tab, backspace, home, end, pageup, pagedown, f1, f2, \
             control, plus, minus, comma, period. Use modifiers for chords such as Alt+A, \
             Shift+K, or Ctrl+Plus. Or name a driving control by its shortcut id \
             (engine, cruise, take_exit, rest, fuel, ...) to press whatever key this \
             player has it on, chord included. The game \
             starts at its real title menu; menus use arrows and enter, and the drive \
             uses the game's own key bindings. After pressing, wait a beat and listen.",
            json!({
                "key": {"type": "string"},
                "modifiers": {
                    "type": "array",
                    "items": {"type": "string", "enum": ["shift", "ctrl", "alt"]},
                    "uniqueItems": true,
                    "description": "optional modifier keys held for this tap"
                },
                "times": {"type": "integer", "description": "repeat count, default 1, max 50"},
            }),
            &["key"],
        ),
        tool(
            "hold",
            "Hold a key down (throttle, brake, steering, and the manual-transmission Shift \
             clutch are hold keys at the wheel). Pair with release.",
            json!({"key": {"type": "string"}}),
            &["key"],
        ),
        tool(
            "release",
            "Release a key held with hold.",
            json!({"key": {"type": "string"}}),
            &["key"],
        ),
        tool(
            "wait",
            "Let the game run for this many real seconds (max 300) with the controls \
             as they stand, then hear everything from that stretch. This is how road \
             time passes.",
            json!({"seconds": {"type": "number"}}),
            &["seconds"],
        ),
        tool(
            "pedal",
            "Hold a key for a bounded number of real seconds and let the game itself \
             lift it -- the throttle (up) or brake (down) for a measured tap. Use this \
             instead of hold and release for pedals: the round trip between the two \
             is a second or more, and at standard pacing that is twenty seconds of \
             road. Replies once the key has lifted, with everything heard meanwhile.",
            json!({
                "key": {"type": "string", "description": "up (throttle), down (brake), or any key"},
                "seconds": {"type": "number", "description": "real seconds down, 0.05 to 30"},
            }),
            &["key", "seconds"],
        ),
        tool(
            "wait_for",
            "Let the game run until something arrives: a spoken line or sound whose \
             text contains `text` (case-insensitive), or a menu on screen when `menu` \
             is true, or `seconds` of real time (max 300), whichever comes first. \
             It also stops early whenever the cab cuts in on the driving channel \
             (an interrupting instruction or warning), so it can be answered: act \
             on it, then wait again. Anything heard since the last listen counts, \
             so a line already spoken answers at once. Replies with everything \
             heard, and says if the cab or the clock ended it first. Use it to \
             drive to the next event instead of waiting blind.",
            json!({
                "text": {"type": "string", "description": "text to listen for"},
                "menu": {"type": "boolean", "description": "return as soon as a menu is on screen"},
                "seconds": {"type": "number", "description": "give up after this many real seconds"},
            }),
            &["seconds"],
        ),
        tool(
            "select",
            "Choose a menu row by part of its label (case-insensitive): the same \
             Home, Down and Enter a player presses. Errors with the rows when no row \
             matches or no menu is up. Wait a moment, then listen.",
            json!({"label": {"type": "string"}}),
            &["label"],
        ),
        tool(
            "cruise",
            "Adaptive cruise, the way a player sets it: K to engage if nothing is \
             holding speed, then the dial walked one mile per hour at a time to the \
             target -- a number, \"limit\" for the posted limit enforcement is \
             reading, or \"off\". Replies with what was heard once the dial settles. \
             In a zone the speed keeper holds instead, and the reply says so.",
            json!({"target": {"description": "a number in miles per hour, \"limit\", or \"off\""}}),
            &["target"],
        ),
        tool(
            "status",
            "The wheel's readouts in one call -- speed, speed limit, grade, what is \
             coming up, route status, the clock, fuel -- pressed as a player would \
             and returned together with anything else heard meanwhile.",
            json!({}),
            &[],
        ),
        tool(
            "listen",
            "Everything audible since the last listen: spoken lines on both channels \
             (exactly what the verbosity setting allowed), earcons and cues with their \
             stereo side, sound beds, horn, radio, weather, and where the engine pitch \
             went. This is the whole game; there is no screen.",
            json!({}),
            &[],
        ),
        tool(
            "menu",
            "The rows of the menu currently on screen and which has focus, as a screen \
             reader user would arrow through them. Errors when no menu is up.",
            json!({}),
            &[],
        ),
        tool(
            "observe",
            "INSPECTOR, not ears: a bounded ground-truth snapshot of the drive (mile, \
             brakes, assists, hazard, damage). For judging and diagnosis. If you \
             needed this to drive, the spoken surface failed -- report that.",
            json!({}),
            &[],
        ),
        tool(
            "start_at",
            "Skip the menus: stage a drive at a discovered road feature and take the \
             wheel right there -- the same finder --playtest-road --find uses. \
             feature must be one of: downgrade, upgrade, zone, limit-drop, stop, \
             scale, curve, interchange, toll, chain-law, destination, departure. \
             Same seed, same road. pick chooses among multiple matches (1-based). \
             unit plants a staffed enforcement unit unit_ahead_mi up the road (default \
             3) so an encounter can be staged on purpose; the road's own seeded posts \
             stay. Listen after staging for the truck's actual starting condition.",
            json!({
                "feature": {"type": "string"},
                "origin": {"type": "string", "description": "search one corridor from this city (fast and thorough)"},
                "destination": {"type": "string", "description": "with origin: the corridor's far end"},
                "seed": {"type": "integer", "description": "default 7"},
                "pick": {"type": "integer", "description": "1-based match index, default 1"},
                "unit": {"type": "string", "description": "plant a staffed enforcement unit ahead of the start: one of median_crossover, roving_patrol, work_zone_post, scale_apron_post, fixed_scale, urban_unit, cmv_unit, chain_control"},
                "unit_ahead_mi": {"type": "number", "description": "how far up the road the planted unit sits, default 3"},
            }),
            &["feature"],
        ),
        tool(
            "scenario",
            "Put the sandbox career in any situation and reopen the terminal on it. \
             Every field is optional: city (any world city, the career moves there \
             with no miles driven), level (1 to 30), deliveries, money, reputation, \
             business (company, leased or independent), endorsements (a list of \
             credential keys bought outright, replacing what was held), hour (local \
             clock 0 to 24, moved forward to), fuel_pct, damage_pct, tire_wear_pct and \
             brake_wear_pct (0 to 100, what a roadside inspection reads), citations and \
             out_of_service_events (counts on the record, dated now; with damage they set the \
             safety band that decides who is pulled into the inspection lane), \
             rested (true takes a full sleep), hos_driving_min, hos_duty_min and \
             hos_since_break_min (sandbox HOS counters in game minutes), clear_load (true drops a load in \
             progress first), market_seed and board_seed (the dispatch board rolls \
             from them), settings (an object of setting name to value, for this \
             session), and name (the career created when none is loaded; default \
             Playtest). Nothing is refused for being unreasonable: this is the \
             audited sandbox. Listen afterwards for the terminal.",
            json!({
                "name": {"type": "string"},
                "city": {"type": "string"},
                "level": {"type": "integer"},
                "deliveries": {"type": "integer"},
                "money": {"type": "number"},
                "reputation": {"type": "number"},
                "business": {"type": "string", "enum": ["company", "leased", "independent"]},
                "endorsements": {"type": "array", "items": {"type": "string"}},
                "hour": {"type": "number"},
                "fuel_pct": {"type": "number"},
                "damage_pct": {"type": "number"},
                "tire_wear_pct": {"type": "number"},
                "brake_wear_pct": {"type": "number"},
                "citations": {"type": "integer"},
                "out_of_service_events": {"type": "integer"},
                "rested": {"type": "boolean"},
                "hos_driving_min": {"type": "number"},
                "hos_duty_min": {"type": "number"},
                "hos_since_break_min": {"type": "number"},
                "clear_load": {"type": "boolean"},
                "market_seed": {"type": "integer"},
                "board_seed": {"type": "integer"},
                "settings": {"type": "object"},
            }),
            &[],
        ),
        tool(
            "operator_keys",
            "Hand the keyboard to the human at the computer, or take it back. With live \
             true the game window comes up and their keys reach the game, so they can \
             drive alongside the agent or take the wheel; with live false the window is \
             minimized and their keys are dropped again. Only at the owner's request: \
             while it is live, anything they type anywhere with the game focused is \
             truck input.",
            json!({
                "live": {"type": "boolean", "description": "true to let the operator's keyboard in, false to shut it out again"},
            }),
            &["live"],
        ),
        tool(
            "key_probe",
            "Measure what the operator's keyboard delivers to the game, screen reader and \
             all -- for a JAWS machine, where held arrows arrive as instant press-and-\
             release pairs. Needs operator_keys live true. Call with action start, ask \
             the operator to hold each arrow key for several seconds, then call with \
             action report: it says how the keys arrived, how long the game read each \
             one held against how long the keyboard did, and how many re-sent pairs the \
             held-key tracker missed and why (a slow frame, or a pair split across two \
             frames).",
            json!({
                "action": {"type": "string", "enum": ["start", "report"]},
            }),
            &["action"],
        ),
        tool(
            "lockstep",
            "Freeze the world between tool calls (on true), so the road waits while you \
             think: time passes only inside wait, pedal and wait_for, and for the few \
             frames a press or cruise call scripts. Use it to answer a cue in time \
             where a round trip would otherwise cost road -- steering with lane keeping \
             off, braking for a hazard. Off (false) runs the road on the wall clock \
             again, which is the default and what an owner driving alongside needs.",
            json!({
                "on": {"type": "boolean", "description": "true to freeze the world between calls, false for the live road"},
            }),
            &["on"],
        ),
        tool(
            "quit_game",
            "Quit the game and end the session (the sandboxed career saves on the way \
             out, as a real quit does).",
            json!({}),
            &[],
        ),
    ]})
}

/// Serve MCP on stdin/stdout, forwarding tool calls into the game loop.
/// Runs on its own thread; returns when stdin closes or the game quits.
pub fn serve(requests: mpsc::Sender<Request>, lifeline: Lifeline) {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    serve_lines_with_lifeline(stdin.lock(), &mut stdout, &requests, &lifeline);
}

/// The MCP loop over any reader and writer: the handshake (`initialize`,
/// `tools/list`, `ping`) is answered right here; only a `tools/call` goes
/// through `requests` to the game loop, and the first one is what boots it.
pub fn serve_lines<R: BufRead, W: Write>(reader: R, out: &mut W, requests: &mpsc::Sender<Request>) {
    serve_lines_inner(reader, out, requests, None);
}

/// The testable MCP loop with a lifeline that is cut when the transport ends.
pub fn serve_lines_with_lifeline<R: BufRead, W: Write>(
    reader: R,
    out: &mut W,
    requests: &mpsc::Sender<Request>,
    lifeline: &Lifeline,
) {
    serve_lines_inner(reader, out, requests, Some(lifeline));
}

fn serve_lines_inner<R: BufRead, W: Write>(
    reader: R,
    out: &mut W,
    requests: &mpsc::Sender<Request>,
    lifeline: Option<&Lifeline>,
) {
    let mut quit = false;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            if !respond_raw(
                out,
                &json!({
                    "jsonrpc": "2.0", "id": null,
                    "error": {"code": -32700, "message": "parse error"}
                }),
            ) {
                if let Some(lifeline) = lifeline {
                    lifeline.cut("the MCP client stopped reading");
                }
                return;
            }
            continue;
        };
        let method = message.get("method").and_then(Value::as_str).unwrap_or("");
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let Some(id) = message.get("id").cloned() else {
            continue; // notification
        };
        let reply = match method {
            "initialize" => json!({
                "protocolVersion": params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or(PROTOCOL_VERSION),
                "capabilities": {"tools": {}},
                "serverInfo": {"name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION")},
                "instructions": "Freight Fate, played by ear. No game is running yet: the \
                    first tool call other than quit_game boots the real game in its \
                    playtest sandbox (a few seconds), then answers. One game at a \
                    time, so a human already playing makes that first call fail; \
                    try again once they quit. At the wheel, drive with pedal (a \
                    measured tap the game itself lifts), cruise (K and the dial to \
                    a number, the posted limit, or off) and wait_for (run until a \
                    line is heard or a menu opens); menus take select by label. \
                    Raw press, hold and release remain for everything else.",
            }),
            "ping" => json!({}),
            "tools/list" => tools_list(),
            "tools/call" => {
                let name = params.get("name").and_then(Value::as_str).unwrap_or("");
                let args = params
                    .get("arguments")
                    .and_then(Value::as_object)
                    .cloned()
                    .unwrap_or_default();
                match build_command(name, &args) {
                    Err(text) => tool_error(text),
                    Ok(command) => {
                        let (reply_tx, reply_rx) = mpsc::channel();
                        if requests
                            .send(Request {
                                command,
                                reply: reply_tx,
                            })
                            .is_err()
                        {
                            tool_error("The game has exited.".to_string())
                        } else {
                            match reply_rx
                                .recv_timeout(std::time::Duration::from_secs(REPLY_TIMEOUT_SECONDS))
                            {
                                Ok(Ok(text)) => tool_text(text),
                                Ok(Err(text)) => tool_error(text),
                                Err(_) => {
                                    tool_error("The game loop did not answer in time.".to_string())
                                }
                            }
                        }
                    }
                }
            }
            _ => {
                if !respond_raw(
                    out,
                    &json!({
                        "jsonrpc": "2.0", "id": id,
                        "error": {"code": -32601, "message": format!("unknown method {method}")}
                    }),
                ) {
                    if let Some(lifeline) = lifeline {
                        lifeline.cut("the MCP client stopped reading");
                    }
                    return;
                }
                continue;
            }
        };
        if !respond_raw(out, &json!({"jsonrpc": "2.0", "id": id, "result": reply})) {
            if let Some(lifeline) = lifeline {
                lifeline.cut("the MCP client stopped reading");
            }
            return;
        }
        if method == "tools/call" && params.get("name").and_then(Value::as_str) == Some("quit_game")
        {
            // A quit ends the process; nothing after it is read.
            quit = true;
            break;
        }
    }
    if let Some(lifeline) = lifeline {
        lifeline.cut(if quit {
            "the MCP client ended the session"
        } else {
            "the MCP client closed stdin"
        });
    }
}

/// One tool call as the game loop will see it, or why it cannot be.
pub fn build_command(name: &str, args: &Map<String, Value>) -> Result<Command, String> {
    let key_arg = |args: &Map<String, Value>| -> Result<KeySpec, String> {
        let name = args.get("key").and_then(Value::as_str).unwrap_or("");
        if let Some((key, text)) = parse_key(name) {
            return Ok(KeySpec::Key { key, text });
        }
        Action::from_id(&name.to_ascii_lowercase())
            .filter(|action| action.on_keyboard())
            .map(KeySpec::Action)
            .ok_or_else(|| format!("{name:?} is not a key or a driving control this server knows"))
    };
    let modifiers = |args: &Map<String, Value>| -> Result<Mods, String> {
        let mut mods = Mods::NONE;
        let Some(values) = args.get("modifiers") else {
            return Ok(mods);
        };
        let values = values
            .as_array()
            .ok_or_else(|| "modifiers must be an array".to_string())?;
        for value in values {
            let name = value.as_str().unwrap_or("").to_ascii_lowercase();
            match name.as_str() {
                "shift" => mods.shift = true,
                "control" | "ctrl" => mods.ctrl = true,
                "alt" => mods.alt = true,
                _ => return Err(format!("{name:?} is not a modifier this server knows")),
            }
        }
        Ok(mods)
    };
    match name {
        "press" => Ok(Command::Press {
            key: key_arg(args)?,
            mods: modifiers(args)?,
            times: args.get("times").and_then(Value::as_i64).unwrap_or(1),
        }),
        "hold" => Ok(Command::Hold {
            key: key_arg(args)?,
        }),
        "release" => Ok(Command::Release {
            key: key_arg(args)?,
        }),
        "wait" => {
            let seconds = args.get("seconds").and_then(Value::as_f64).unwrap_or(0.0);
            if seconds <= 0.0 {
                return Err("wait needs a positive number of seconds".to_string());
            }
            Ok(Command::Wait { seconds })
        }
        "pedal" => {
            let key = key_arg(args)?;
            let seconds = args.get("seconds").and_then(Value::as_f64).unwrap_or(0.0);
            if seconds <= 0.0 {
                return Err("pedal needs a positive number of seconds".to_string());
            }
            Ok(Command::Pedal { key, seconds })
        }
        "wait_for" => {
            let seconds = args.get("seconds").and_then(Value::as_f64).unwrap_or(0.0);
            if seconds <= 0.0 {
                return Err("wait_for needs a positive number of seconds".to_string());
            }
            let text = args
                .get("text")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|text| !text.is_empty())
                .map(str::to_string);
            let menu = args.get("menu").and_then(Value::as_bool).unwrap_or(false);
            Ok(Command::WaitFor {
                text,
                menu,
                seconds,
            })
        }
        "select" => {
            let label = args
                .get("label")
                .and_then(Value::as_str)
                .map(str::trim)
                .unwrap_or("");
            if label.is_empty() {
                return Err("select needs part of a row's label".to_string());
            }
            Ok(Command::Select {
                label: label.to_string(),
            })
        }
        "cruise" => {
            let target = match args.get("target") {
                Some(Value::Number(number)) => number
                    .as_f64()
                    .filter(|mph| *mph > 0.0)
                    .map(CruiseTarget::Mph)
                    .ok_or_else(|| "cruise needs a speed above zero".to_string())?,
                Some(Value::String(word)) => match word.trim().to_ascii_lowercase().as_str() {
                    "limit" | "posted" => CruiseTarget::Limit,
                    "off" | "cancel" => CruiseTarget::Off,
                    other => match other.parse::<f64>() {
                        Ok(mph) if mph > 0.0 => CruiseTarget::Mph(mph),
                        _ => {
                            return Err(format!(
                            "{other:?} is not a cruise target; use a number, \"limit\", or \"off\""
                        ))
                        }
                    },
                },
                _ => {
                    return Err("cruise needs a target: a number, \"limit\", or \"off\"".to_string())
                }
            };
            Ok(Command::Cruise { target })
        }
        "status" => Ok(Command::Status),
        "listen" => Ok(Command::Listen),
        "menu" => Ok(Command::Menu),
        "observe" => Ok(Command::Observe),
        "start_at" => {
            let feature = args
                .get("feature")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let seed = args.get("seed").and_then(Value::as_i64).unwrap_or(7);
            let pick = args.get("pick").and_then(Value::as_u64).unwrap_or(1) as usize;
            let origin = args
                .get("origin")
                .and_then(Value::as_str)
                .map(str::to_string);
            let destination = args
                .get("destination")
                .and_then(Value::as_str)
                .map(str::to_string);
            let unit = match args.get("unit").and_then(Value::as_str) {
                None => None,
                Some(kind) => {
                    let kind = kind.trim().to_ascii_lowercase();
                    if !ff_core::sim::enforcement_posts::POST_KINDS.contains(&kind.as_str()) {
                        return Err(format!(
                            "unit must be one of {}",
                            ff_core::sim::enforcement_posts::POST_KINDS.join(", ")
                        ));
                    }
                    let ahead = args
                        .get("unit_ahead_mi")
                        .and_then(Value::as_f64)
                        .unwrap_or(3.0);
                    if ahead < 0.5 {
                        return Err("unit_ahead_mi must be at least half a mile".to_string());
                    }
                    Some((kind, ahead))
                }
            };
            let (hit, opts, found, picked) =
                discover(&feature, origin, destination, seed, pick, unit)?;
            Ok(Command::StageHit {
                hit: Box::new(hit),
                found,
                picked,
                opts: Box::new(opts),
            })
        }
        "scenario" => crate::playtest::scenario::Scenario::from_json(args)
            .map(|scenario| Command::Scenario(Box::new(scenario))),
        "operator_keys" => match args.get("live").and_then(Value::as_bool) {
            Some(live) => Ok(Command::OperatorKeys { live }),
            None => Err("operator_keys needs live: true or false".to_string()),
        },
        "key_probe" => match args.get("action").and_then(Value::as_str) {
            Some("start") => Ok(Command::KeyProbe { start: true }),
            Some("report") => Ok(Command::KeyProbe { start: false }),
            _ => Err("key_probe needs action: \"start\" or \"report\"".to_string()),
        },
        "lockstep" => match args.get("on").and_then(Value::as_bool) {
            Some(on) => Ok(Command::Lockstep { on }),
            None => Err("lockstep needs on: true or false".to_string()),
        },
        "quit_game" => Ok(Command::Quit),
        other => Err(format!("unknown tool {other}")),
    }
}

/// Find one road feature, bounded so it answers in seconds. Runs against
/// world data alone -- safe on any thread, never inside the game loop
/// (an in-frame search froze the whole game, found live 2026-08-30).
/// With no endpoints the sweep is sampled; with endpoints it is capped at
/// thirty corridors, because an origin alone still fans out to every
/// reachable city and a full fan is minutes of search (that is
/// `--playtest-road --scan`'s job).
pub(super) fn discover(
    feature: &str,
    origin: Option<String>,
    destination: Option<String>,
    seed: i64,
    pick: usize,
    unit: Option<(String, f64)>,
) -> Result<
    (
        crate::playtest::road::Hit,
        crate::playtest::road::RoadOptions,
        usize,
        usize,
    ),
    String,
> {
    use crate::playtest::road;
    // An unknown term would "search" and find nothing every time; name the
    // real vocabulary instead ("steep grade" cost a session to this).
    if !road::FEATURES.contains(&feature) {
        return Err(format!(
            "{feature:?} is not a road feature the finder knows. The features are: {}.",
            road::FEATURES.join(", ")
        ));
    }
    let opts = road::RoadOptions {
        feature: feature.to_string(),
        origin: origin.clone(),
        destination,
        seed: Some(seed),
        trip_seed: Some(seed),
        pick,
        unit,
        sandbox: false, // the whole server already runs sandboxed
        ..Default::default()
    };
    let world = ff_core::data::world::get_world();
    let mut pairs = if opts.origin.is_some() || opts.destination.is_some() {
        road::route_pairs(world, &opts)
    } else {
        road::sampled_world_pairs(world, 30)
    };
    // Nearest corridors first when an origin anchors the search: they are
    // the likeliest to be wanted and the fastest to walk, so the cap keeps
    // the search local instead of taking thirty alphabetical strangers.
    if let Some(anchor) = opts
        .origin
        .as_deref()
        .and_then(|name| world.cities.get(&world.resolve_city_key(name)))
        .map(|c| (c.lat, c.lon))
    {
        let distance = |key: &str| -> f64 {
            world
                .cities
                .get(&world.resolve_city_key(key))
                .map_or(f64::MAX, |c| {
                    ((c.lat - anchor.0).powi(2) + (c.lon - anchor.1).powi(2)).sqrt()
                })
        };
        pairs.sort_by(|x, y| {
            distance(&x.1)
                .partial_cmp(&distance(&y.1))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
    }
    pairs.truncate(30);
    if pairs.is_empty() {
        return Err("No routes matched those options.".to_string());
    }
    eprintln!(
        "[agent-server] searching {} corridor(s) for {feature:?}...",
        pairs.len()
    );
    let started = std::time::Instant::now();
    let hits = road::find_feature_seeded(world, &pairs, feature, &opts);
    eprintln!(
        "[agent-server] search finished in {:.1}s: {} match(es)",
        started.elapsed().as_secs_f64(),
        hits.len()
    );
    if hits.is_empty() {
        return Err(format!(
            "No road feature matching {feature:?} was found in the sampled \
             routes; try another term, another seed, or name an origin \
             city to search a specific corridor."
        ));
    }
    let index = pick.saturating_sub(1).min(hits.len() - 1);
    let hit = hits[index].clone();
    let found = hits.len();
    Ok((hit, opts, found, index + 1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn press_parser_reaches_the_function_key_help_surface() {
        assert_eq!(parse_key("f1"), Some((Key::F1, None)));
        assert_eq!(parse_key("F2"), Some((Key::F2, None)));
    }

    #[test]
    fn press_parser_reaches_speech_stop_and_speed_adjustment_keys() {
        assert_eq!(parse_key("control"), Some((Key::LCtrl, None)));
        assert_eq!(parse_key("ctrl"), Some((Key::LCtrl, None)));
        assert_eq!(parse_key("shift"), Some((Key::LShift, None)));
        assert_eq!(parse_key("plus"), Some((Key::Plus, Some('+'))));
        assert_eq!(parse_key("minus"), Some((Key::Minus, Some('-'))));
    }

    #[test]
    fn press_command_carries_modifier_chords_to_the_game() {
        let args = serde_json::from_value(json!({
            "key": "a",
            "modifiers": ["alt", "shift"]
        }))
        .unwrap();

        let command = build_command("press", &args).unwrap();

        let Command::Press { key, mods, times } = command else {
            panic!("press tool did not build a press command");
        };
        assert_eq!(
            key,
            KeySpec::Key {
                key: Key::A,
                text: Some('a')
            }
        );
        assert_eq!(
            mods,
            Mods {
                shift: true,
                ctrl: false,
                alt: true,
            }
        );
        assert_eq!(times, 1);
    }

    #[test]
    fn press_command_takes_a_driving_control_by_id() {
        let args = serde_json::from_value(json!({"key": "engine"})).unwrap();
        let Command::Press { key, .. } = build_command("press", &args).unwrap() else {
            panic!("press tool did not build a press command");
        };
        assert_eq!(key, KeySpec::Action(Action::Engine));
        // A pad-only control has no key to press.
        let args = serde_json::from_value(json!({"key": "cruise_up"})).unwrap();
        let error = match build_command("press", &args) {
            Ok(_) => panic!("a pad-only control was accepted as a key"),
            Err(error) => error,
        };
        assert!(error.contains("not a key or a driving control"), "{error}");
    }

    #[test]
    fn press_command_rejects_unknown_modifiers() {
        let args = serde_json::from_value(json!({
            "key": "a",
            "modifiers": ["meta"]
        }))
        .unwrap();

        let error = match build_command("press", &args) {
            Ok(_) => panic!("unknown modifier was accepted"),
            Err(error) => error,
        };
        assert_eq!(error, "\"meta\" is not a modifier this server knows");
    }

    #[test]
    fn scale_discovery_only_stages_scales_open_in_the_built_drive() {
        let (hit, _opts, found, picked) =
            discover("scale", None, None, 83, usize::MAX, None).unwrap();

        assert!(found > 0);
        assert_eq!(picked, found);
        assert!(hit.label.starts_with("OPEN scale:"), "{}", hit.label);
    }
}
