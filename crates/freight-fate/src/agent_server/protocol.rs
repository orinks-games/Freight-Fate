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

/// The `scenario` tool's fields: one `json!` for the whole tool list would
/// pass the macro's recursion limit, so the co-driver's counters, the chain
/// fields and the assist states are added to the rest here.
fn scenario_properties() -> Value {
    let mut properties = json!({
        "name": {"type": "string"},
        "city": {"type": "string"},
        "level": {"type": "integer"},
        "deliveries": {"type": "integer"},
        "money": {"type": "number"},
        "reputation": {"type": "number"},
        "business": {"type": "string", "enum": ["company", "leased", "independent"]},
        "business_stage": {"type": "string", "enum": ["company_driver", "company_by_choice", "leased_on", "leased_on_prepared", "own_authority"], "description": "the ownership ladder stage; wins over business"},
        "owned_trucks": {"type": "array", "items": {"type": "string"}, "description": "truck catalog keys the career owns, replacing what it held; the first is driven"},
        "endorsements": {"type": "array", "items": {"type": "string"}},
        "hour": {"type": "number"},
        "date": {"type": "string", "description": "the calendar date the career shows, MM-DD (05-01): dated rules such as spring weight limits and tunnel seasons read it; career time and hours do not move"},
        "fuel_pct": {"type": "number"},
        "window": {"type": "string", "enum": ["up", "down"]},
        "road_surface": {"type": "string", "enum": ["jointed", "smooth"]},
        "tire_song": {"type": "string", "enum": ["quiet", "hum"]},
        "weather": {"type": "string", "description": "the simulated sky every drive starts under, forced: snow, ice, cloudy, clear, rain, heavy_rain, thunderstorm, fog or wind; free unlocks it"},
        "damage_pct": {"type": "number"},
        "tire_wear_pct": {"type": "number"},
        "brake_wear_pct": {"type": "number"},
        "citations": {"type": "integer"},
        "out_of_service_events": {"type": "integer"},
        "rested": {"type": "boolean"},
        "first_run": {"type": "boolean"},
        "touch_gestures": {"type": "boolean"},
        "tilt_steering": {"type": "boolean"},
        "phone": {"type": "boolean", "description": "true plays this run as an iPhone with no hardware keyboard: the phone's menus, settings rows and help, and prompts that name gestures; false goes back to the desktop"},
        "fatigue": {"type": "number", "description": "0 to 100; 40 worn, 60 drowsy, 80 severe"},
        "hos_driving_min": {"type": "number"},
        "hos_duty_min": {"type": "number"},
        "hos_since_break_min": {"type": "number"},
        "hos_cycle_min": {"type": "number", "description": "on-duty minutes on the 70-hour, 8-day cycle, all ending now"},
        "hos_restart_off_min": {"type": "number", "description": "off-duty minutes already rested in a row toward the cycle restart, set after hos_cycle_min"},
        "clear_load": {"type": "boolean"},
        "market_seed": {"type": "integer"},
        "board_seed": {"type": "integer"},
        "board_load_to": {"type": "string", "description": "a city or a facility by name (SCI Cross Dock): the next dispatch board adds a load delivered there as its last row"},
        "came_from": {"type": "string"},
        "newspaper": {"type": "string", "description": "a file holding a driver newspaper issue in the site's body (crates/ff-core/src/newspaper/fixture_issue.json is a made-up one) for the game to hold, or none for no issue"},
        "settings": {"type": "object"},
    });
    for key in [
        "co_driver_driving_min",
        "co_driver_duty_min",
        "co_driver_since_break_min",
        "co_driver_rest_min",
    ] {
        properties[key] = json!({"type": "number"});
    }
    properties["carrier"] = json!({"type": "string", "description": "a carriers.json key (prairie_link, northstar, ...): the career drives for it, or is leased to it as an owner-operator; the home terminal is re-derived, the truck stays"});
    properties["home_city"] = json!({"type": "string", "description": "the driver's home city; empty string for a blank home, as on a 1.9 save"});
    properties["first_delivery_city"] = json!({"type": "string", "description": "the city the first delivery loaded in, as a 1.9 save records it; empty string clears the record"});
    properties["copied_from_1_9"] = json!({"type": "boolean", "description": "true runs the 1.9 copy's carry-over on the career as staged (home fix and carryover state), after carrier, home_city and first_delivery_city"});
    properties["carryover"] = json!({"type": "string", "enum": ["native", "settled", "carrier_choice_due"], "description": "the carry-over state, set directly, last"});
    properties["xp"] =
        json!({"type": "number", "description": "exact career experience; wins over level"});
    properties["banked_levels_heard"] = json!({"type": "boolean", "description": "false leaves the one-time terminal line for experience past level 30 owed"});
    properties["chains"] = json!({"type": "boolean"});
    properties["chain_wear_pct"] = json!({"type": "number"});
    properties["chain_repair_links"] = json!({"type": "integer"});
    properties["abs"] = json!({"type": "string", "enum": ["inactive", "modulating"]});
    properties["drive_traction"] = json!({"type": "string", "enum": ["grip", "spinning"]});
    properties["trailer_sway"] =
        json!({"type": "string", "enum": ["steady", "swaying", "off_tracking"]});
    properties["mirror_glance"] = json!({"type": "string", "enum": ["forward", "left", "right"]});
    properties["predictive_cruise"] = json!({
        "type": "string",
        "enum": ["cruising", "building_for_climb", "holding_crest", "jake_anticipating", "resuming_smooth"]
    });
    properties["assisted_lane_change"] = json!({
        "type": "string",
        "enum": ["idle", "blind_spot_left", "blind_spot_right", "waiting_for_gap_left", "waiting_for_gap_right", "gliding_left", "gliding_right"]
    });
    properties["lane_keep_availability"] = json!({
        "type": "string",
        "enum": ["off", "active", "dropped_out_snow"]
    });
    properties
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
            "gesture",
            "Send an iPhone touch gesture by name, through the same translator and \
             dispatch the touch screen uses, then reply with what was heard. Names: tap, \
             double_tap, flick_up/down/left/right, two_finger_tap, two_finger_double_tap, \
             two_finger_scrub, two_finger_flick_up/down/left/right, two_finger_hold, \
             two_finger_turn_right/left, three_finger_tap, three_finger_flick_up/down/\
             left/right, gas_hold, brake_hold, steer_left_hold, steer_right_hold, \
             deep_swipe, still_hold, lift, gas_hold_tap, gas_hold_double_tap, \
             gas_hold_flick_up/down/left/right, the same six for brake_hold_, \
             second_finger_steer_left/right, second_finger_lift, and keyboard_disconnected \
             (spoken hints then name gestures, as on a phone) or keyboard_connected. A \
             hold stays down until \
             lift, or give seconds and the game lifts it, as pedal does. A deep swipe \
             grows out of a brake hold, so send brake_hold first, then deep_swipe. \
             status reports the touch mode and, at the wheel, what a double tap answers. \
             length (points) on a timed one-finger hold, a deep_swipe or a lift sends \
             how far the finger travelled as it lifts, which Practice gestures reads.",
            json!({
                "name": {"type": "string"},
                "seconds": {"type": "number", "description": "for a hold: real seconds before the game lifts it, 0.05 to 30"},
                "length": {"type": "integer", "description": "for a one-finger hold, deep_swipe or lift: points travelled, sent as the finger lifts"},
            }),
            &["name"],
        ),
        tool(
            "fingers",
            "Touch the screen with raw fingers, read by the shared gesture recognizer \
             (Android's, and the iPhone's with Touch gestures, New gesture reading on), \
             then reply with what was heard. events is a list of {finger, kind, x, y, \
             at_ms}: finger is any id that stays the same from down to up; kind is down, \
             move, up or cancel; x and y are points (y grows downwards, an iPhone screen \
             is about 390 by 844); at_ms is milliseconds from the start of the call, in \
             order. Each event is handed over at its own time and the recognizer ticks \
             every frame, so a stroke held past the flick window becomes a hold. Moves \
             are not filled in: send one every 16 ms or so for a stroke. A finger left \
             down stays down until a later call lifts it. In lockstep the clock runs \
             only inside calls. A desktop run, phone: true included, has no native \
             recognizer, so this tool always uses the shared one, whatever the setting \
             says; gesture sends a recognized gesture by name instead. status reports \
             the recognizer's phase on the STATE line.",
            json!({
                "events": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "finger": {"type": "integer"},
                            "kind": {"type": "string", "enum": ["down", "move", "up", "cancel"]},
                            "x": {"type": "number"},
                            "y": {"type": "number"},
                            "at_ms": {"type": "number"},
                        },
                        "required": ["finger", "kind", "at_ms"],
                    },
                },
            }),
            &["events"],
        ),
        tool(
            "tilt",
            "Hold the phone at an angle, standing in for its motion sensor the way \
             gesture stands in for the screen: degrees from flat, right edge down \
             positive, for seconds (0.05 to 30), then back to where it was, or to \
             then. silent sends no readings for the seconds instead. The phone is \
             held level (0) from the start. With tilt steering on (scenario \
             tilt_steering), level is taken from the pose when the drive starts or \
             resumes, and status reports the tilt state, level and angle. Replies \
             with what was heard once the time is up.",
            json!({
                "degrees": {"type": "number", "description": "angle from flat, -90 to 90; right edge down is positive"},
                "seconds": {"type": "number", "description": "real seconds to hold it, 0.05 to 30"},
                "then": {"type": "number", "description": "the angle to hold afterwards; default: where it was before"},
                "silent": {"type": "boolean", "description": "send no readings for the seconds"},
            }),
            &["seconds"],
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
            "Cruise, the way a player sets it: K to engage if nothing is \
             holding speed, then the dial walked one mile per hour at a time to the \
             target -- a number, \"limit\" for the posted limit enforcement is \
             reading, or \"off\". Replies with what was heard once the dial settles. \
             In a zone the cruise holds instead, and the reply says so.",
            json!({"target": {"description": "a number in miles per hour, \"limit\", or \"off\""}}),
            &["target"],
        ),
        tool(
            "status",
            "The wheel's readouts in one call -- speed, speed limit, grade, what is \
             coming up, route status, the clock, fuel -- pressed as a player would \
             and returned together with anything else heard meanwhile. Also reports \
             CLOUD RESUME: Off, Idle, Checking, Cancelling, Done with counts, or Failed. At the wheel \
             it ends with one STATE line, not spoken: alertness (the fatigue band: \
             rested, worn, drowsy or severe), nod (awake, nodding or recovering, \
             with its timer), nod_misses (missed nods in a row toward the forced \
             stop), lift_owed (fatigue a coffee or energy drink will take back, \
             and when), rest_call (this load's call to dispatch for rest: \
             available, used, or none with no load on) and, while the trip has \
             ended at a facility gate, gate (arriving, rolling, \
             rolling_after_creep, creeping, stopped, stopped_told, holding or \
             open), on a load with a co-driver co_driver_shift (bunk, driving, \
             ending_rested, ending_last_lot, ending_hours_short or shoulder: where \
             their shift stands) and co_driver_left (the driving time or duty \
             window their own logbook has left, in minutes, or hours off) and off_wheel_restart_rest_left (the off-duty minutes the restart still needs on the logbook off the wheel), and jake_zone (what the driver has heard about the no \
             engine brake zone ahead or under the truck: untold, heads_up, \
             inside_untold or inside_told), touch_mode (menu, field, wheel or \
             practice: how a gesture is read on this screen), road_offer (what a \
             double tap answers: none, exit, pull_over or arrival) and touch_clutch \
             (out, shifting, or neutral once it has taken neutral for an engine \
             lugging in a tall gear: the clutch a touch shift works in a manual \
             truck), chain_control (clear, level_1 or level_2: the chain law posted \
             at the truck, which follows snow or ice on the road), full_night \
             (ten_hours or eight_hours: the full night a rest stop offers, 8 hours \
             only under Canadian rules on a road that stays in Canada) and chains \
             (stowed, on_snow, on_bare inside the 2 mile bare-road grace, on_bare_wearing past it, or broken), chain_worker (driver, assist \
             or co_driver: who does the chain work, whoever holds the wheel) and \
             chain_stop (none, install, remove, tighten or repair: the roadside \
             chain stop the assist or the co-driver is making), chain_wear (the set's \
             wear in percent, at the truck's exact speed in mph), air (the air supply in psi), weather_service (nws, eccc or none: the live weather service for the truck's position), weather_station (the live report's station, its distance and how the condition was decided, or none), \
             brake (the service brake demand, 0 to 1, with \"cruise snubbing\" while \
             cruise holds the drums on), jake (the engine brake stage, 0 for off), and app (active, inactive or \
             background: where a phone app is, moved by app_event, with \
             sound_taken after it while an interruption holds the sound). \
             Off the wheel the \
             STATE line carries touch_mode and app, and on the dispatch board also \
             board_mode (assigned or browse), board_offer (the row dispatch \
             assigned or recommends, or none), board_reason (assigned, none, \
             training, best_pay_for_hours, best_net_for_hours, \
             most_deadline_room or plain) and board_gate (clear, or hours_heard \
             or chains_heard with the row whose warning was heard); there it presses \
             no keys, since they are menu keys. A SPEECH line follows it \
             everywhere: main_backend (the saved Main voice choice, automatic \
             when none), main (the voice bound now) and event (the event \
             voice bound now, or main voice when events share it). A BUSINESS line \
             gives business_stage (company_driver, company_by_choice, leased_on, \
             leased_on_prepared or own_authority) and buy_in_paid (what the buy-in \
             paid for the first tractor, 0 when none is on record). A CARRYOVER line \
             gives the carry-over state of a career copied from 1.9 (native, settled \
             or carrier_choice_due), the carrier key, home_city (blank when none is \
             saved) and home_terminal_city. An HOURS line gives cycle_left (cycle \
             minutes left, or unenforced), restart_offer (none, fresh or finish: the \
             restart row on offer) and restart_rest_left (off-duty minutes the cycle \
             restart still needs). An INPUT line \
             ends it: phone (whether this run plays as a phone), device (keyboard, \
             controller or touch: the input prompts name), last_used, and keyboard \
             and controller (connected or none).",
            json!({}),
            &[],
        ),
        tool(
            "app_event",
            "Send a phone app's lifecycle notification, as an iPhone delivers it, then \
             reply with what was heard. Names: will_resign_active (a call, Siri, \
             Control Center, the app switcher, the lock button; the window's focus is \
             lost with it), did_enter_background, will_enter_foreground, \
             did_become_active (focus comes back with it), audio_interruption_began, \
             audio_interruption_ended and output_device_lost (headphones out). Leaving \
             is will_resign_active then did_enter_background; coming back is \
             will_enter_foreground then did_become_active; Control Center is \
             will_resign_active then did_become_active. status reports app (active, \
             inactive or background, and sound_taken while an interruption holds the \
             sound).",
            json!({"name": {"type": "string"}}),
            &["name"],
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
             scale, curve, interchange, toll, chain-law, destination, departure, river, \
             bridge (one with joints, longest first), jointed (road with seams, longest \
             first), border (a customs booth at a land crossing; pass origin and \
             destination, e.g. bellingham_wa_us and surrey_bc_ca), state-line (a state \
             or province line, in the order driven), grooved (concrete with grooves \
             across the lane, longest first), grating (an open steel grate deck the tires hum on). \
             Same seed, same road. pick chooses among multiple matches (1-based). \
             unit plants a staffed enforcement unit unit_ahead_mi up the road (default \
             3) so an encounter can be staged on purpose; the road's own seeded posts \
             stay. cargo_type and cargo_tons choose the load (a catalog cargo key, and \
             tons aboard, default 20); hazmat names the material a placarded load is \
             (paint, chlorine_cylinders, blasting_explosives, ...), and hour sets the \
             Eastern career clock at the start (default 9; 24 and up are later days, \
             day 0 is a Wednesday). lead_mi starts the truck that many miles before \
             the feature instead of the finder's own lead. A scenario that staged the \
             co-driver's hours (co_driver_*) makes the staged load a team load with \
             them aboard. Listen after staging for the truck's actual starting \
             condition.",
            json!({
                "feature": {"type": "string"},
                "origin": {"type": "string", "description": "search one corridor from this city (fast and thorough)"},
                "destination": {"type": "string", "description": "with origin: the corridor's far end"},
                "seed": {"type": "integer", "description": "default 7"},
                "pick": {"type": "integer", "description": "1-based match index, default 1"},
                "unit": {"type": "string", "description": "plant a staffed enforcement unit ahead of the start: one of median_crossover, roving_patrol, work_zone_post, scale_apron_post, fixed_scale, urban_unit, cmv_unit, chain_control"},
                "unit_ahead_mi": {"type": "number", "description": "how far up the road the planted unit sits, default 3"},
                "cargo_type": {"type": "string", "description": "catalog cargo key the truck carries, e.g. hazardous or fuel_bulk; default: what the departure facility ships"},
                "cargo_tons": {"type": "number", "description": "tons aboard, default 20"},
                "hazmat": {"type": "string", "description": "hazardous material key, e.g. paint, chlorine_cylinders, consumer_fireworks, blasting_explosives, gasoline; sets cargo_type to match when it is not given"},
                "hour": {"type": "number", "description": "career clock at the start, Eastern hours from day 0 (a Wednesday), default 9"},
                "cattle": {"type": "object", "description": "with cargo_type livestock: stage the load. Fields: hours_aboard (number), footing (settled, restless, scrambling, one_down), exemption (not_in_force, near_pickup, counting, near_delivery), heat (cool, hot_moving, hot_standing)"},
                "lead_mi": {"type": "number", "description": "miles before the feature to start, default the finder's own"},
                "weather": {"type": "string", "description": "force the simulated sky for this and later drives: snow, ice, cloudy, clear, rain, heavy_rain, thunderstorm, fog or wind; free unlocks it"},
            }),
            &["feature"],
        ),
        tool(
            "scenario",
            "Put the sandbox career in any situation and reopen the terminal on it. \
             Every field is optional: city (any world city, the career moves there \
             with no miles driven), level (1 to 40), xp (exact experience, wins over \
             level), total_miles (lifetime miles on the record), banked_levels_heard (false leaves the one-time line for experience \
             past level 30 owed; staging a level or xp marks it heard otherwise), deliveries, money, reputation, \
             business (company, leased or independent) or business_stage \
             (company_driver, company_by_choice, leased_on, leased_on_prepared or \
             own_authority; wins over business), carrier (a carriers.json key), home_city \
             and first_delivery_city (empty for blank), copied_from_1_9 (true runs the \
             1.9 copy's carry-over on the staged career), carryover (native, settled or \
             carrier_choice_due, set directly), owned_trucks (truck keys owned, the \
             first driven, with fresh wear and a full tank), endorsements (a list of \
             credential keys bought outright, replacing what was held), hour (local \
             clock 0 to 24, moved forward to), fuel_pct, damage_pct, tire_wear_pct and \
             brake_wear_pct (0 to 100, what a roadside inspection reads), chains (true \
             puts a set of snow chains aboard, false takes them away), chain_wear_pct \
             (0 to 100, how worn the set is; near 100 a set snaps soon) and \
             chain_repair_links (spare links for a broken chain), citations and \
             out_of_service_events (counts on the record, dated now; with damage they set the \
             safety band that decides who is pulled into the inspection lane), \
             rested (true takes a full sleep), first_run (true teaches the first-run \
             walkthrough on the next drive), touch_gestures (true opens the iPhone \
             Touch gestures settings screen over the terminal), phone (true plays the \
             run as an iPhone with no hardware keyboard, false as the desktop again; \
             applied first), tilt_steering (true or false: the Touch gestures switch; \
             the tilt tool is the phone's motion sensor), fatigue (0 to 100, set after rested; \
             the next drive starts in that band and gives its cue), hos_driving_min, hos_duty_min and \
             hos_since_break_min (sandbox HOS counters in game minutes), hos_cycle_min \
             (on-duty minutes on the 70-hour, 8-day cycle, all ending now), \
             hos_restart_off_min (off-duty minutes already rested in a row toward the \
             cycle restart, set after hos_cycle_min), \
             co_driver_driving_min, co_driver_duty_min, co_driver_since_break_min and \
             co_driver_rest_min (the co-driver's own logbook in game minutes, rest being \
             their unbroken sleeper time toward 10 hours: put on the load in progress \
             when it has a co-driver, otherwise on the next co-driver paired, so a drive \
             can start with the co-driver short of hours), clear_load (true drops a load in \
             progress first), market_seed and board_seed (the dispatch board rolls \
             from them), came_from (the city the truck delivered from to get here, \
             so the next board is shaped by where it came from), settings (an \
             object of setting name to value, for this \
             session), window (up or down: every drive from here on starts with \
             the driver window there), road_surface (jointed or smooth: every \
             drive from here on keeps that surface the whole way, over what the \
             pavement layer reads), tire_song (quiet or hum: every drive \
             from here on holds that song the whole way), abs (inactive or modulating), \
             drive_traction (grip or spinning), trailer_sway (steady, swaying or off_tracking), \
             mirror_glance (forward, left or right), and name (the career created when none is \
             loaded; default Playtest). Nothing is refused for being unreasonable: this is the \
             audited sandbox. Listen afterwards for the terminal.",
            scenario_properties(),
            &[],
        ),
        tool(
            "hazard",
            "Fire a road hazard on the drive right now, by name, instead of waiting for \
             the road to draw one: tire debris, debris, ladder, lumber, mattress, boxes, \
             tarp, tumbleweeds, rockfall, stopped vehicle, lane closure, stopped traffic, \
             deer, elk, dog, and the rest of the catalog. The call, the lane answer, the \
             assists and the seeded outcome are the drawn hazard's own. Errors when not \
             at the wheel.",
            json!({
                "name": {"type": "string", "description": "the hazard's spoken name, with or without \"the\""},
            }),
            &["name"],
        ),
        tool(
            "weather",
            "Force the simulated sky on the drive under way, and on every drive staged \
             after it: snow, ice, cloudy, clear, rain, heavy_rain, thunderstorm, fog or \
             wind, or free to unlock it. The change lands on the next frame the way the \
             sky changes on its own: the weather line, the road surface, and the chain \
             law that follows the road.",
            json!({
                "kind": {"type": "string", "description": "a condition, or free"},
            }),
            &["kind"],
        ),
        tool(
            "nod",
            "Make the driver nod off on the drive right now instead of waiting for \
             severe fatigue to count one down: the reaction window opens on the next \
             rolling frame with its warning, and steering or braking inside it catches \
             it, exactly as a counted nod. A miss counts toward the forced stop. \
             Errors when not at the wheel or while the co-driver drives.",
            json!({}),
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
        "gesture" => {
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            let gesture = crate::touch::Gesture::from_name(&name).ok_or_else(|| {
                format!(
                    "{name:?} is not a gesture this server knows. The names are: {}.",
                    crate::touch::Gesture::names()
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            let seconds = match args.get("seconds") {
                None | Some(Value::Null) => None,
                Some(value) => match value.as_f64() {
                    Some(seconds) if seconds > 0.0 => Some(seconds),
                    _ => return Err("seconds must be a positive number".to_string()),
                },
            };
            let length = match args.get("length") {
                None | Some(Value::Null) => None,
                Some(value) => match value.as_u64() {
                    Some(points) => Some(
                        u16::try_from(points)
                            .unwrap_or(u16::MAX)
                            .min(crate::touch::MAX_TRAVEL_POINTS),
                    ),
                    None => return Err("length must be a whole number of points".to_string()),
                },
            };
            Ok(Command::Gesture {
                gesture,
                seconds,
                length,
            })
        }
        "fingers" => Ok(Command::Fingers {
            events: fingers_arg(args)?,
        }),
        "tilt" => {
            let seconds = args.get("seconds").and_then(Value::as_f64).unwrap_or(0.0);
            if seconds <= 0.0 {
                return Err("tilt needs a positive number of seconds".to_string());
            }
            let angle = |name: &str| -> Result<Option<f64>, String> {
                match args.get(name) {
                    None | Some(Value::Null) => Ok(None),
                    Some(value) => match value.as_f64() {
                        Some(degrees) if degrees.is_finite() => {
                            Ok(Some(degrees.clamp(-90.0, 90.0)))
                        }
                        _ => Err(format!("{name} must be a number of degrees")),
                    },
                }
            };
            let silent = args.get("silent").and_then(Value::as_bool).unwrap_or(false);
            let degrees = angle("degrees")?;
            if silent == degrees.is_some() {
                return Err("tilt needs degrees, or silent: true, but not both".to_string());
            }
            Ok(Command::Tilt {
                degrees,
                seconds,
                then: angle("then")?,
            })
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
        "app_event" => {
            let name = args
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            let event = ff_core::app_lifecycle::AppEvent::from_key(&name).ok_or_else(|| {
                format!(
                    "{name:?} is not an app event. The names are: {}.",
                    ff_core::app_lifecycle::AppEvent::ALL
                        .iter()
                        .map(|event| event.key())
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            Ok(Command::AppEvent { event })
        }
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
            let cargo = match args.get("cargo_type").and_then(Value::as_str) {
                None => None,
                Some(key) => {
                    let key = key.trim().to_ascii_lowercase();
                    if ff_core::models::jobs::cargo_type(&key).is_none() {
                        return Err(format!("{key:?} is not in the cargo catalog"));
                    }
                    Some(key)
                }
            };
            let tons = args.get("cargo_tons").and_then(Value::as_f64);
            if tons.is_some_and(|t| !(t > 0.0 && t <= 40.0)) {
                return Err("cargo_tons must be more than 0 and at most 40".to_string());
            }
            let hazmat = match args.get("hazmat").and_then(Value::as_str) {
                None => None,
                Some(key) => {
                    let key = key.trim().to_ascii_lowercase();
                    let Some(material) = ff_core::models::jobs::hazmat::hazmat_commodity(&key)
                    else {
                        return Err(format!(
                            "{key:?} is not a hazardous material the game knows"
                        ));
                    };
                    if cargo.as_deref().is_some_and(|c| c != material.cargo) {
                        return Err(format!("{key} rides as {} cargo", material.cargo));
                    }
                    Some(material)
                }
            };
            let cargo = cargo.or(hazmat.map(|m| m.cargo.to_string()));
            let hour = args.get("hour").and_then(Value::as_f64);
            if hour.is_some_and(|h| !(0.0..24.0 * 7.0).contains(&h)) {
                return Err("hour must be from 0 up to 168, a week of career clock".to_string());
            }
            let lead = match args.get("lead_mi").and_then(Value::as_f64) {
                Some(mi) if mi < 0.0 => return Err("lead_mi must not be negative".to_string()),
                lead => lead,
            };
            let (hit, mut opts, found, picked) =
                discover(&feature, origin, destination, seed, pick, unit)?;
            if let Some(key) = cargo {
                opts.cargo_type = key.clone();
                opts.departure_cargo = Some(key);
            }
            if let Some(tons) = tons {
                opts.cargo = tons;
            }
            if let Some(material) = hazmat {
                opts.departure_hazmat = Some(material.key.to_string());
            }
            if lead.is_some() {
                opts.lead = lead;
            }
            // The career clock at the start: 9 AM unless asked (the outside
            // beds, night driving).
            if hour.is_some() {
                opts.hour = hour;
            }
            if let Some(cattle) = args.get("cattle") {
                opts.cattle = Some(cattle_stage(cattle)?);
            }
            let weather = args
                .get("weather")
                .and_then(Value::as_str)
                .map(crate::playtest::scenario::parse_weather)
                .transpose()?;
            Ok(Command::StageHit {
                hit: Box::new(hit),
                found,
                picked,
                opts: Box::new(opts),
                weather,
            })
        }
        "scenario" => crate::playtest::scenario::Scenario::from_json(args)
            .map(|scenario| Command::Scenario(Box::new(scenario))),
        "hazard" => match args.get("name").and_then(Value::as_str) {
            Some(name) if !name.trim().is_empty() => Ok(Command::Hazard {
                name: name.trim().to_string(),
            }),
            _ => Err("hazard needs name: the hazard's spoken name".to_string()),
        },
        "nod" => Ok(Command::Nod),
        "weather" => match args.get("kind").and_then(Value::as_str) {
            Some(kind) => {
                crate::playtest::scenario::parse_weather(kind).map(|kind| Command::Weather { kind })
            }
            None => Err("weather needs kind: a condition, or free".to_string()),
        },
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

/// The `cattle` object of `start_at`, read strictly.
fn cattle_stage(value: &Value) -> Result<crate::playtest::road::CattleStage, String> {
    use ff_core::sim::livestock::{Exemption, Footing, Heat};
    let Some(map) = value.as_object() else {
        return Err("cattle must be an object".to_string());
    };
    let word = |key: &str| map.get(key).and_then(Value::as_str).map(str::to_string);
    let footing = match word("footing") {
        None => None,
        Some(w) => Some(Footing::from_key(&w).ok_or_else(|| format!("unknown footing {w}"))?),
    };
    let exemption = match word("exemption") {
        None => None,
        Some(w) => Some(Exemption::from_key(&w).ok_or_else(|| format!("unknown exemption {w}"))?),
    };
    let heat = match word("heat") {
        None => None,
        Some(w) => Some(Heat::from_key(&w).ok_or_else(|| format!("unknown heat {w}"))?),
    };
    let hours_aboard = match map.get("hours_aboard") {
        None => None,
        Some(v) => Some(
            v.as_f64()
                .filter(|h| (0.0..=48.0).contains(h))
                .ok_or("hours_aboard must be a number from 0 to 48")?,
        ),
    };
    Ok(crate::playtest::road::CattleStage {
        footing,
        exemption,
        heat,
        hours_aboard,
    })
}

/// The `fingers` tool's events, checked: known kinds, finite points, and
/// times in order within the call's limit.
fn fingers_arg(args: &Map<String, Value>) -> Result<Vec<super::fingers::ScriptedFinger>, String> {
    let Some(list) = args.get("events").and_then(Value::as_array) else {
        return Err("fingers needs events, a list of {finger, kind, x, y, at_ms}".to_string());
    };
    if list.is_empty() {
        return Err("fingers needs at least one event".to_string());
    }
    let mut events = Vec::with_capacity(list.len());
    let mut last_ms = 0.0;
    for (i, event) in list.iter().enumerate() {
        let n = i + 1;
        let finger = event
            .get("finger")
            .and_then(Value::as_i64)
            .ok_or_else(|| format!("event {n}: finger must be a whole number"))?;
        let kind_name = event.get("kind").and_then(Value::as_str).unwrap_or("");
        let kind = crate::touch::FingerKind::from_name(kind_name)
            .ok_or_else(|| format!("event {n}: kind must be down, move, up or cancel"))?;
        let point = |name: &str| -> Result<f32, String> {
            match event.get(name) {
                None | Some(Value::Null) if kind == crate::touch::FingerKind::Cancel => Ok(0.0),
                None | Some(Value::Null) => Err(format!("event {n}: {name} is missing")),
                Some(value) => match value.as_f64() {
                    Some(v) if v.is_finite() => Ok(v as f32),
                    _ => Err(format!("event {n}: {name} must be a number of points")),
                },
            }
        };
        let (x, y) = (point("x")?, point("y")?);
        let at_ms = event
            .get("at_ms")
            .and_then(Value::as_f64)
            .filter(|ms| ms.is_finite() && *ms >= 0.0)
            .ok_or_else(|| format!("event {n}: at_ms must be 0 or more"))?;
        if at_ms < last_ms {
            return Err(format!(
                "event {n}: at_ms {at_ms} comes before the event ahead of it ({last_ms})"
            ));
        }
        if at_ms > super::fingers::MAX_FINGERS_MS {
            return Err(format!(
                "event {n}: at_ms is past {} ms; split it into calls",
                super::fingers::MAX_FINGERS_MS
            ));
        }
        last_ms = at_ms;
        events.push(super::fingers::ScriptedFinger {
            finger,
            kind,
            x,
            y,
            at_ms,
        });
    }
    Ok(events)
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
    fn start_at_refuses_a_load_the_game_cannot_carry() {
        let unknown = serde_json::from_value(json!({
            "feature": "departure",
            "cargo_type": "moon_rocks"
        }))
        .unwrap();
        let refusal = build_command("start_at", &unknown).err().unwrap();
        assert!(refusal.contains("cargo catalog"), "{refusal}");
        for (args, wanted) in [
            (
                json!({"feature": "departure", "hazmat": "moon_dust"}),
                "hazardous material",
            ),
            (
                json!({"feature": "departure", "hazmat": "paint", "cargo_type": "fuel_bulk"}),
                "rides as hazardous",
            ),
            (json!({"feature": "departure", "hour": 168.0}), "hour"),
        ] {
            let args = serde_json::from_value(args).unwrap();
            let refusal = build_command("start_at", &args).err().unwrap();
            assert!(refusal.contains(wanted), "{refusal}");
        }
        for tons in [0.0, -3.0, 41.0] {
            let args = serde_json::from_value(json!({
                "feature": "departure",
                "cargo_tons": tons
            }))
            .unwrap();
            let refusal = build_command("start_at", &args).err().unwrap();
            assert!(refusal.contains("cargo_tons"), "{tons}: {refusal}");
        }
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
