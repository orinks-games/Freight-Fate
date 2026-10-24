//! The rows of every settings category, and the Left/Right action table
//! that mirrors them (`SettingsCategoryState.build_items`, `_adjust`,
//! `_speech_control_specs`, `_driving_assist_specs` in `main_menu.py`).

use std::rc::Rc;

use ff_core::pyfmt::round_py_int;
use ff_core::settings::Settings;

use super::settings::{Adjust, SettingsCategoryState};
use super::settings_actions::{
    acc_gap_label, assist_preset_label, backup_announcements_label, cue_loudness_label,
    descent_level_label, event_voice_label, hos_label, lane_keeping_label, language_label,
    output_label, pace_label, score_display_label, update_channel,
};
use super::shortcuts::{ShortcutDevice, ShortcutsState};
use super::touch_gestures::TouchGesturesState;
use crate::app::GameContext;
use crate::states::base::{Label, Menu, MenuItem};
use crate::states::main_menu_help::render_help_line;
use crate::states::update::UpdateCheckState;

type Row = MenuItem<SettingsCategoryState>;

/// One Speech-category control: a label, the Left/Right action, its help.
pub(super) struct SpeechSpec {
    pub label: Label<SettingsCategoryState>,
    pub action: Adjust,
    pub help: &'static str,
}

fn on_off(flag: bool) -> &'static str {
    if flag {
        "on"
    } else {
        "off"
    }
}

/// The Radio sound row's value. Each is a whole literal so a catalog can
/// translate it.
fn radio_sound_name(cab_speakers: bool) -> &'static str {
    if cab_speakers {
        "Cab speakers"
    } else {
        "Clean"
    }
}

fn pct(level: f64) -> i64 {
    round_py_int(level * 100.0)
}

fn dyn_label(f: impl Fn(&Settings) -> String + 'static) -> Label<SettingsCategoryState> {
    Label::dynamic(move |_s, ctx| f(&ctx.settings))
}

/// The Controller row: the setting, and while it is on, the pad the game
/// is listening to, so a player can tell whether their controller was found.
fn controller_label(ctx: &GameContext) -> String {
    if !ctx.settings.controller_enabled {
        return "Controller: disabled".to_string();
    }
    if ctx.controller.connected() {
        format!("Controller: enabled, {} connected", ctx.controller.name())
    } else {
        "Controller: enabled, none connected".to_string()
    }
}

fn adjust(f: impl Fn(&mut SettingsCategoryState, &mut GameContext, i64) + 'static) -> Adjust {
    Rc::new(f)
}

/// A settings row. Its help may name a control as `{{id}}`, read in the
/// player's own keys the way the help pages read them.
fn row(label: Label<SettingsCategoryState>, action: Adjust, help: &str) -> Row {
    let help = help.to_string();
    row_dyn_help(label, action, move |_| help.clone())
}

/// A row whose help reads the settings as they stand, for help that has to
/// describe what another row has set.
fn row_dyn_help(
    label: Label<SettingsCategoryState>,
    action: Adjust,
    help: impl Fn(&Settings) -> String + 'static,
) -> Row {
    MenuItem::new(label, move |s: &mut SettingsCategoryState, ctx| {
        action(s, ctx, 1)
    })
    .help(Label::dynamic(move |_s, ctx| {
        render_help_line(ctx, &help(&ctx.settings))
    }))
}

/// What leans and which way to steer, as the Steering guide and Lane guide
/// sound rows under Audio have it set.
fn steering_lean_text(s: &Settings) -> String {
    let what = if s.lane_guide_tone {
        "a soft tone leans"
    } else {
        "the engine leans"
    };
    let way = if s.steering_guide_inverted {
        "away from"
    } else {
        "toward"
    };
    format!("{what} {way} where the wheel should go, so steer {way} it")
}

/// Only the iPhone and iPad game has a touch surface to bind.
fn touch_gestures_row() -> Row {
    MenuItem::new(
        "Touch gestures",
        |_s: &mut SettingsCategoryState, ctx: &mut GameContext| {
            ctx.push_state(TouchGesturesState::new())
        },
    )
    .help(
        "Choose which driving command each touch gesture runs, including a second \
         finger while holding gas or brake.",
    )
}

fn back_row() -> Row {
    MenuItem::new("Back", |s: &mut SettingsCategoryState, ctx| s.go_back(ctx))
}

/// `(field, label, help)` for each driving assist row.
pub(super) const DRIVING_ASSIST_SPECS: [(&str, &str, &str); 12] = [
    (
        "automatic_emergency_braking",
        "Automatic emergency braking",
        "After a hazard warning, the truck brakes itself if you have not slowed enough.",
    ),
    (
        "lane_departure_warning",
        "Lane-departure warning",
        "A spoken and sounded warning when the truck drifts toward a lane edge.",
    ),
    (
        "stop_and_go_assist",
        "Stop-and-go assistance",
        "Adaptive cruise slows behind traffic and resumes when it is safe.",
    ),
    (
        "descent_speed_control",
        "Descent speed control",
        "With adaptive cruise on, holds a steep downhill grade at the speed your truck can take it at with its load, on the engine brake and short brake applications. Balanced and Interactive capture a lower target when you brake. Interactive also caps every descent at 55 miles per hour and intervenes harder.",
    ),
    (
        "exit_speed_assist",
        "Exit speed assistance",
        "For a signaled exit: slows a truck too fast for the gore, keeps the approach within 10 miles per hour of road speed, and brakes in the exit lane to the exit speed. You still take the exit.",
    ),
    (
        "destination_approach_assist",
        "Facility stopping assistance",
        "On the final approach, after any exit, it works the throttle and brakes: up to 12 miles per hour through the facility lane, creeping the last 200 feet, then stopping at pickup and delivery facilities, rest stops, and required weigh stations. It never chooses an exit, enters a yard, or docks. In a chain-law area it stops to put snow chains on and take them off, and it may spend on-duty minutes on chains: about 30 to chain up, 10 to take them off. It never buys chains. Realistic leaves it off; Balanced and All assists turn it on.",
    ),
    (
        "curve_speed_assist",
        "Curve assistance",
        "Takes mapped bends for you: slows to the advised speed on the service brakes, never the engine brake, and holds the wheel through the bend and through street corners. On a real downgrade it does raise the jake. While it steers, the engine leans only when you drift. Lane keeping is a separate setting and holds you between the lines the rest of the time.",
    ),
    (
        "route_transition_assist",
        "Route-transition assistance",
        "Speed and lane help at confirmed route transitions.",
    ),
    // The speed keeper is an input-accessibility aid, not a driving
    // assist -- it lives in Gameplay, Controls, and there is exactly one
    // row for it. It used to appear here too, a second live control for
    // the one setting, which is a real hazard in a spoken list.
    (
        "pedal_latch",
        "Latching brake",
        "Tap the brake, then press again and hold half a second: a click and a spoken confirmation latch it hands-free. One press of {{brake}} releases it; the accelerator releases it instantly. The throttle key never latches. Presets never change this.",
    ),
    (
        "predictive_cruise",
        "Predictive cruise",
        "Cruise reads the road a mile and a half ahead: a little extra speed before a climb, easing at a crest, no speed added before a descent. It says what it is doing once per hill. Presets never change this.",
    ),
    (
        "curve_callouts",
        "Curve callouts",
        "Bends that demand slowing are called before they arrive, like Sharp left, half a mile, advise 35. Bends you are already slow enough for stay silent. Press {{upcoming}} to list the next few either way. Presets never change this.",
    ),
    // The speed keeper holds a speed for you, so it belongs with the
    // rest of the driving help rather than in Controls, where it sat
    // among the keyboard, controller and units rows. Like the other
    // input-accessibility aids above it, presets never touch it.
    (
        "speed_keeper",
        "Speed keeper",
        "In low-speed zones, like facility roads, gates, and construction zones, {{cruise}} holds your current speed, then hands back to adaptive cruise on open roads. It eases off early for the next turn or the next lower limit. Braking cancels the session. Presets never change this.",
    ),
];

/// `getattr(s, field)` for the boolean assist rows.
pub(super) fn assist_flag(s: &Settings, field: &str) -> bool {
    match field {
        "automatic_emergency_braking" => s.automatic_emergency_braking,
        "lane_departure_warning" => s.lane_departure_warning,
        "stop_and_go_assist" => s.stop_and_go_assist,
        "exit_speed_assist" => s.exit_speed_assist,
        "destination_approach_assist" => s.destination_approach_assist,
        "curve_speed_assist" => s.curve_speed_assist,
        "route_transition_assist" => s.route_transition_assist,
        "predictive_cruise" => s.predictive_cruise,
        "curve_callouts" => s.curve_callouts,
        "speed_keeper" => s.speed_keeper,
        _ => false,
    }
}

fn assist_value_text(s: &Settings, field: &str) -> String {
    match field {
        "descent_speed_control" => descent_level_label(s),
        "pedal_latch" => s.pedal_latch.clone(),
        _ => on_off(assist_flag(s, field)).to_string(),
    }
}

impl SettingsCategoryState {
    /// Speech-category controls as (label, action, help) triples.
    ///
    /// Built dynamically so the menu and the left/right adjust handler stay
    /// in sync, and so rate, pitch, volume, and voice only appear when the
    /// active voices actually support them (a running screen reader does
    /// not).
    pub(super) fn speech_control_specs(&self, ctx: &GameContext) -> Vec<SpeechSpec> {
        let speech = &ctx.speech;
        let mut specs = vec![
            SpeechSpec {
                label: dyn_label(|s| format!("Language: {}", language_label(s))),
                action: adjust(|s, ctx, d| s.cycle_language(ctx, d)),
                help: "Automatic follows your system language. Radio and \
                       recorded voices stay in English.",
            },
            SpeechSpec {
                label: dyn_label(|s| format!("Score display: {}", score_display_label(s))),
                action: adjust(|s, ctx, d| s.cycle_score_display(ctx, d)),
                help: "Choose whether reputation and dispatch trust read both their descriptive band and score, only the score, or only the description. A trust score is spoken only when service sets its band.",
            },
            SpeechSpec {
                label: dyn_label(|s| {
                    format!("Driving speech: {}", s.driving_speech.replace('_', " "))
                }),
                action: adjust(|s, ctx, d| s.cycle_driving_speech(ctx, d)),
                help: "How much the road tells you. Standard speaks every \
                       confirmation, status update and traffic call, and a \
                       driving tip once per leg. Quiet speaks short \
                       confirmations, lane openings, and status updates, but \
                       not the traffic around you or tips. Urgent only keeps \
                       safety warnings and directions requiring action. What \
                       a setting leaves out makes no sound and stays out of \
                       the event buffer. Readout keys always answer. \
                       Billboards, place names, and \
                       landmarks have their own switches below.",
            },
            SpeechSpec {
                label: dyn_label(|s| format!("Roadside chatter: {}", s.chatter_summary())),
                action: adjust(|s, ctx, d| s.set_all_chatter(ctx, d)),
                help: "Color between navigation cues: parks, rivers, mountain \
                       passes, museums, and billboards. Right arrow turns all \
                       on, Left arrow all off; the switches below pick each \
                       kind. Safety and navigation are never affected. Town \
                       names are the Place callouts row below.",
            },
            SpeechSpec {
                label: dyn_label(|s| {
                    format!("Speak parks and forests: {}", on_off(s.chatter_parks))
                }),
                action: adjust(|s, ctx, _d| s.toggle_chatter(ctx, "chatter_parks")),
                help: "Callouts entering a national park, national forest, or \
                       other protected public land.",
            },
            SpeechSpec {
                label: dyn_label(|s| {
                    format!("Speak river crossings: {}", on_off(s.chatter_rivers))
                }),
                action: adjust(|s, ctx, _d| s.toggle_chatter(ctx, "chatter_rivers")),
                help: "Callouts crossing a named river.",
            },
            SpeechSpec {
                label: dyn_label(|s| {
                    format!("Speak mountain passes: {}", on_off(s.chatter_passes))
                }),
                action: adjust(|s, ctx, _d| s.toggle_chatter(ctx, "chatter_passes")),
                help: "Callouts approaching a named mountain pass, and famous \
                       highway markers like the Loneliest Road in America.",
            },
            SpeechSpec {
                label: dyn_label(|s| {
                    format!(
                        "Speak museums and attractions: {}",
                        on_off(s.chatter_museums)
                    )
                }),
                action: adjust(|s, ctx, _d| s.toggle_chatter(ctx, "chatter_museums")),
                help: "Callouts for museums and roadside attractions near the route.",
            },
            SpeechSpec {
                label: dyn_label(|s| format!("Speak billboards: {}", on_off(s.chatter_billboards))),
                action: adjust(|s, ctx, _d| s.toggle_chatter(ctx, "chatter_billboards")),
                help: "Roadside billboards, read as you pass them: attorney ads \
                       and questionable tourist traps.",
            },
            SpeechSpec {
                label: dyn_label(|s| format!("Place callouts: {}", s.place_callouts)),
                action: adjust(|s, ctx, d| s.cycle_place_callouts(ctx, d)),
                help: "Place names along the road. Sparse speaks only the town \
                       names that explain a speed limit change, like Entering \
                       Strawberry before its 35. All adds the towns the route \
                       passes. Off silences place names; speed limits are never \
                       affected.",
            },
            SpeechSpec {
                label: dyn_label(|s| {
                    format!(
                        "Menu position announcements: {}",
                        on_off(s.announce_menu_position)
                    )
                }),
                action: adjust(|s, ctx, d| s.toggle_menu_position(ctx, d)),
                help: "On, menus say the position, like 3 of 10, after each option. \
                       Off, only the option.",
            },
            SpeechSpec {
                label: dyn_label(|s| {
                    format!(
                        "Say when a career is backed up: {}",
                        backup_announcements_label(s)
                    )
                }),
                action: adjust(|s, ctx, d| s.cycle_backup_announcements(ctx, d)),
                help: "How often you hear that a career is backed up to your \
                       orinks.net account. Every time, after each save. Once a \
                       session, the first backup of each career after the game \
                       starts. Never keeps it quiet. Backups keep going either \
                       way, and a refused backup is always spoken.",
            },
            SpeechSpec {
                label: dyn_label(|s| format!("Driving event voice: {}", event_voice_label(s))),
                action: adjust(|s, ctx, d| s.cycle_event_voice(ctx, d)),
                help: if cfg!(target_os = "ios") {
                    "Road events through the main voice or a separate system \
                     voice that VoiceOver cannot cut off. The rate, pitch, \
                     volume, and voice rows below appear only when the voice \
                     supports them; with VoiceOver running, set those in \
                     VoiceOver."
                } else {
                    "Road events through the main voice or a separate SAPI or \
                     OneCore voice a screen reader cannot cut off. The rate, \
                     pitch, volume, and voice rows below appear only when the \
                     voice supports them; with a screen reader running, set \
                     those in the screen reader."
                },
            },
        ];
        // Braille only needs NVDA or JAWS, which iPhone and iPad do not have.
        if !cfg!(target_os = "ios") {
            specs.push(SpeechSpec {
                label: dyn_label(|s| format!("Output: {}", output_label(s))),
                action: adjust(|s, ctx, d| s.toggle_braille_only(ctx, d)),
                help: "Speech and braille speaks every line and, with NVDA or JAWS, \
                       shows it on your braille display too. Braille only puts \
                       every line on the display and speaks nothing: menus, \
                       readouts, and road events alike. It needs NVDA or JAWS; \
                       with any other voice the game keeps speaking and says so.",
            });
        }
        if speech.backend_name().eq_ignore_ascii_case("jaws") {
            specs.push(SpeechSpec {
                label: Label::dynamic(|_s, ctx| {
                    let state = if ctx.services.jaws_script.installed() {
                        "faster"
                    } else {
                        "default"
                    };
                    format!("JAWS arrow keys: {state}")
                }),
                action: adjust(|s, ctx, d| s.toggle_jaws_arrow_script(ctx, d)),
                help: "JAWS reads each arrow key with its own script, which waits for \
                       the screen to change, so menus answer slowly and held arrows \
                       lag. Faster adds a small script for this game to your JAWS \
                       settings so the arrows answer at once. Default removes it. \
                       Restart JAWS if nothing changes.",
            });
        }
        if speech.supports_rate() {
            specs.push(SpeechSpec {
                label: dyn_label(|s| format!("Speech rate: {} percent", pct(s.speech_rate))),
                action: adjust(|s, ctx, d| s.adjust_speech(ctx, "speech_rate", 0.1 * d as f64)),
                help: "How fast the game's voice speaks.",
            });
        }
        if speech.supports_pitch() {
            specs.push(SpeechSpec {
                label: dyn_label(|s| format!("Speech pitch: {} percent", pct(s.speech_pitch))),
                action: adjust(|s, ctx, d| s.adjust_speech(ctx, "speech_pitch", 0.1 * d as f64)),
                help: "How high or low the game's voice sounds.",
            });
        }
        if speech.supports_volume() {
            specs.push(SpeechSpec {
                label: dyn_label(|s| format!("Speech volume: {} percent", pct(s.speech_volume))),
                action: adjust(|s, ctx, d| s.adjust_speech(ctx, "speech_volume", 0.1 * d as f64)),
                help: "Loudness of the game's voice, separate from sound volume.",
            });
        }
        if !speech.voice_names().is_empty() {
            specs.push(SpeechSpec {
                label: dyn_label(|s| {
                    let voice = if s.speech_voice.is_empty() {
                        "default"
                    } else {
                        s.speech_voice.as_str()
                    };
                    format!("Speech voice: {voice}")
                }),
                action: adjust(|s, ctx, d| s.cycle_voice(ctx, d)),
                help: "Which installed voice the game speaks with.",
            });
        }
        // Weather, traffic, and parking sources, and the live-weather calendar,
        // used to live here. They are world simulation, not speech, so they
        // moved to Gameplay, World and traffic.
        specs
    }

    /// The Left/Right action for each row, in row order (`_adjust`'s table).
    pub(super) fn adjust_actions(&self, ctx: &GameContext) -> Vec<Adjust> {
        match self.category.as_str() {
            "speech" => self
                .speech_control_specs(ctx)
                .into_iter()
                .map(|spec| spec.action)
                .collect(),
            "difficulty" => vec![
                adjust(|s, ctx, d| s.cycle_pace(ctx, d)),
                adjust(|s, ctx, d| s.cycle_hos(ctx, d)),
                adjust(|s, ctx, d| s.toggle_hos_planning_hints(ctx, d)),
            ],
            "world" => vec![
                adjust(|s, ctx, d| s.toggle_real_weather(ctx, d)),
                adjust(|s, ctx, d| s.toggle_real_traffic(ctx, d)),
                adjust(|s, ctx, d| s.toggle_real_fuel_prices(ctx, d)),
                adjust(|s, ctx, d| s.toggle_real_parking(ctx, d)),
                adjust(|s, ctx, d| s.toggle_live_weather_calendar(ctx, d)),
                adjust(|s, ctx, d| s.toggle_facility_streets(ctx, d)),
            ],
            "controls" => vec![
                adjust(|s, ctx, d| s.toggle_units(ctx, d)),
                adjust(|s, ctx, d| s.toggle_transmission(ctx, d)),
                adjust(|s, ctx, d| s.cycle_automatic_direction_changes(ctx, d)),
                adjust(|s, ctx, d| s.toggle_controller(ctx, d)),
                adjust(|s, ctx, d| s.toggle_haptics(ctx, d)),
                #[cfg(feature = "visuals")]
                adjust(|s, ctx, d| s.toggle_visual_display(ctx, d)),
                adjust(|s, ctx, d| s.toggle_menu_accelerator_keys(ctx, d)),
            ],
            "assistance" => {
                let mut actions = vec![adjust(|s, ctx, d| s.cycle_assist_preset(ctx, d))];
                for (field, _, _) in DRIVING_ASSIST_SPECS {
                    actions.push(adjust(move |s, ctx, d| {
                        s.toggle_driving_assist(ctx, field, d)
                    }));
                }
                // The last row of this category was left out of the
                // arrow-key path, so Left and Right did nothing on it
                // while every other row answered. Every row added here
                // after Lane keeping must be appended below it, in the
                // same order build_items appends them.
                actions.push(adjust(|s, ctx, d| s.cycle_lane_keeping(ctx, d)));
                actions.push(adjust(|s, ctx, d| s.cycle_acc_gap(ctx, d)));
                actions
            }
            "audio" => vec![
                adjust(|s, ctx, d| s.volume(ctx, "master_volume", 0.1 * d as f64)),
                adjust(|s, ctx, d| s.volume(ctx, "sfx_volume", 0.1 * d as f64)),
                adjust(|s, ctx, d| s.cycle_cue_loudness(ctx, d)),
                adjust(|s, ctx, d| s.toggle_steering_guide_inverted(ctx, d)),
                adjust(|s, ctx, d| s.toggle_lane_guide_tone(ctx, d)),
                adjust(|s, ctx, d| s.volume(ctx, "weather_volume", 0.1 * d as f64)),
                adjust(|s, ctx, d| s.volume(ctx, "engine_volume", 0.1 * d as f64)),
                adjust(|s, ctx, d| s.toggle_engine_voice(ctx, d)),
                adjust(|s, ctx, d| s.toggle_jake_voice(ctx, d)),
                adjust(|s, ctx, d| s.volume(ctx, "music_volume", 0.1 * d as f64)),
                adjust(|s, ctx, d| s.toggle_music_source(ctx, d)),
                adjust(|s, ctx, d| s.roll_music_seed(ctx, d)),
                adjust(|s, ctx, d| s.volume(ctx, "radio_volume", 0.1 * d as f64)),
                adjust(|s, ctx, d| s.toggle_radio_streamer_safe(ctx, d)),
                adjust(|s, ctx, d| s.toggle_radio_shuffle_playlists(ctx, d)),
                adjust(|s, ctx, d| s.toggle_radio_cab_speakers(ctx, d)),
                adjust(|s, ctx, d| s.toggle_duck_for_speech(ctx, d)),
                adjust(|s, ctx, d| s.volume(ctx, "ui_volume", 0.1 * d as f64)),
            ],
            "updates" => vec![adjust(|s, ctx, d| s.toggle_update_channel(ctx, d))],
            // Reading the log's location is an action, not a value to
            // step through, so left/right does nothing on that row.
            "reports" => vec![adjust(|_s, _ctx, _d| {})],
            _ => Vec::new(),
        }
    }

    /// `build_items` for this category.
    pub(super) fn category_items(&self, ctx: &GameContext) -> Vec<Row> {
        match self.category.as_str() {
            "assistance" => self.assistance_items(),
            "difficulty" => vec![
                row(
                    dyn_label(|s| format!("Driving mode: {}", pace_label(s))),
                    adjust(|s, ctx, d| s.cycle_pace(ctx, d)),
                    "Pacing and pressure. Relaxed passes about ten minutes of \
                     road and clock each real minute, with wider hazard windows, \
                     gentler damage and fatigue, calmer speech, and the most \
                     time to respond. Standard keeps balanced pressure and moves \
                     twice as fast, about twenty minutes each real minute. Real time keeps Standard's \
                     pressure and runs the driving clock at the speed of a real \
                     clock, lined up with your computer's date and time; delivery \
                     time remaining and hours of service do not move. Changeable \
                     mid-drive from the pause menu; the new pacing starts when \
                     the truck next stops.",
                ),
                row(
                    dyn_label(|s| format!("Hours of service: {}", hos_label(s))),
                    adjust(|s, ctx, d| s.cycle_hos(ctx, d)),
                    "Realistic: full hours rules and normal road hazards. \
                     Relaxed: the same 11-hour drive, 14-hour window, and \
                     30-minute break, with lighter fines, fewer inspections, and \
                     rare road hazards.",
                ),
                row(
                    dyn_label(|s| {
                        format!(
                            "Hours of service planning hints: {}",
                            if s.hos_planning_hints { "On" } else { "Off" }
                        )
                    }),
                    adjust(|s, ctx, d| s.toggle_hos_planning_hints(ctx, d)),
                    "Optional early advice for a break or sleep stop with time to spare. If an earlier stop fits, the hint also names the last legally reachable fallback. While rolling, use the Rest control to select the recommended stop; use it again to cancel. Standard driving speech speaks one suggestion before the next hours warning. The driving time readout gives full hours and route details. Quiet and Urgent only keep the automatic hint silent. Your required hours warnings and readout controls still work when this is off. It also reminds you before a chain-law road if you have no chains: on the dispatch board when the load crosses a road where trucks must carry chains this season, or where live weather or a weather alert puts a chain law in effect for your truck, while the terminal garage can still sell you a set, and on the road when a posted chain law requires chains on your truck. It never buys anything.",
                ),
                // The overspeed warning no longer has a row. It armed at the
                // same 5-over pace predictive cruise itself holds, so it
                // chimed at drivers for a speed the truck picked, and the
                // setting existed to switch that off. It now arms above
                // cruise's pace and below the enforcement leeway, which
                // leaves nothing worth turning off.
                back_row(),
            ],
            "world" => vec![
                row(
                    dyn_label(|s| {
                        format!(
                            "Weather source: {}",
                            if s.real_weather {
                                "real world"
                            } else {
                                "simulated"
                            }
                        )
                    }),
                    adjust(|s, ctx, d| s.toggle_real_weather(ctx, d)),
                    "Real world uses live city conditions when available, and reads \
                     the Weather Service's active warnings along your route: dispatch \
                     plans around a blizzard, ice storm, hurricane or tornado warning, \
                     and the cab reads a warning out as you drive into it.",
                ),
                row(
                    dyn_label(|s| {
                        format!(
                            "Traffic source: {}",
                            if s.real_traffic {
                                "real time"
                            } else {
                                "simulated"
                            }
                        )
                    }),
                    adjust(|s, ctx, d| s.toggle_real_traffic(ctx, d)),
                    "Real time uses live traffic incidents from state 511 \
                     services when available.",
                ),
                row(
                    dyn_label(|s| {
                        format!(
                            "Fuel prices: {}",
                            if s.real_fuel_prices {
                                "this week's national average"
                            } else {
                                "simulated"
                            }
                        )
                    }),
                    adjust(|s, ctx, d| s.toggle_real_fuel_prices(ctx, d)),
                    "This week's national average puts the federal weekly diesel survey \
                     price at every pump, with each region's usual difference on top. \
                     Simulated draws a price per region for the session.",
                ),
                row(
                    dyn_label(|s| {
                        format!(
                            "Parking source: {}",
                            if s.real_parking {
                                "real time"
                            } else {
                                "simulated"
                            }
                        )
                    }),
                    adjust(|s, ctx, d| s.toggle_real_parking(ctx, d)),
                    "Real time uses live truck parking availability from \
                     TPIMS when available.",
                ),
                row(
                    dyn_label(|s| {
                        format!(
                            "Live weather controls calendar: {}",
                            on_off(s.live_weather_controls_calendar)
                        )
                    }),
                    adjust(|s, ctx, d| s.toggle_live_weather_calendar(ctx, d)),
                    "On, live weather uses today's real date and season. Off, \
                     the career date advances at midnight and seasons pass while \
                     weather still comes from the real world.",
                ),
                row(
                    dyn_label(|s| format!("Facility streets: {}", on_off(s.facility_streets))),
                    adjust(|s, ctx, d| s.toggle_facility_streets(ctx, d)),
                    "On, you drive the real streets between the highway and a \
                     facility, turn by turn. Off, as in version 1.8, the exit ramp \
                     ends at the facility gate, no exit has a light or stop sign, \
                     a loaded run leaves straight onto the highway, and a pickup \
                     in town is one stretch of road. \
                     Applies from your next drive.",
                ),
                back_row(),
            ],
            "controls" => vec![
                row(
                    dyn_label(|s| {
                        format!(
                            "Units: {}",
                            if s.imperial_units {
                                "imperial, miles"
                            } else {
                                "metric, kilometers"
                            }
                        )
                    }),
                    adjust(|s, ctx, d| s.toggle_units(ctx, d)),
                    "Distance and speed in miles or kilometers.",
                ),
                row(
                    dyn_label(|s| {
                        format!(
                            "Transmission: {}",
                            if s.automatic_transmission {
                                "automatic"
                            } else {
                                "manual"
                            }
                        )
                    }),
                    adjust(|s, ctx, d| s.toggle_transmission(ctx, d)),
                    "Automatic shifts for you. Manual uses the clutch \
                     with W and Q to shift up and down.",
                ),
                row(
                    dyn_label(|s| {
                        format!(
                            "Automatic direction changes: {}",
                            s.automatic_direction_changes
                        )
                    }),
                    adjust(|s, ctx, d| s.cycle_automatic_direction_changes(ctx, d)),
                    "Both styles change direction with a fresh press at a \
                     standstill; a brake held through a stop just holds the \
                     truck. Deliberate requires the release and press \
                     everywhere. Automatic transmission only.",
                ),
                row(
                    Label::dynamic(|_s, ctx| controller_label(ctx)),
                    adjust(|s, ctx, d| s.toggle_controller(ctx, d)),
                    "Game-controller input alongside the keyboard, which \
                     always stays active. The first connected controller is \
                     used.",
                ),
                row(
                    dyn_label(|s| {
                        format!(
                            "Haptics: {}",
                            if s.haptics_enabled {
                                "enabled"
                            } else {
                                "disabled"
                            }
                        )
                    }),
                    adjust(|s, ctx, d| s.toggle_haptics(ctx, d)),
                    "Controller rumble for hazards, hard braking, the rumble \
                     strip, and road seams. Needs a controller connected.",
                ),
                // A build without the visual display has nothing to turn off.
                #[cfg(feature = "visuals")]
                row(
                    dyn_label(|s| format!("Visual display: {}", on_off(s.visual_display))),
                    adjust(|s, ctx, d| s.toggle_visual_display(ctx, d)),
                    "Draws the road, the dashboard, menus and captions of what \
                     is spoken in the game window, for sighted and low-vision \
                     players. Off keeps the window dark and saves battery. \
                     Speech and sound are the same either way.",
                ),
                row(
                    dyn_label(|s| format!("Menu accelerator keys: {}", on_off(s.menu_accelerator_keys))),
                    adjust(|s, ctx, d| s.toggle_menu_accelerator_keys(ctx, d)),
                    "When on, typing a unique first letter immediately activates that menu item. Shared initials only move between matches; press Enter to activate. Off only moves the selection.",
                ),
                // Enter opens each; Left and Right have nothing to step through,
                // and the adjust table above stops before these rows.
                MenuItem::new(
                    "Keyboard shortcuts",
                    |_s: &mut SettingsCategoryState, ctx| {
                        ctx.push_state(ShortcutsState::new(ShortcutDevice::Keyboard))
                    },
                )
                .help(
                    "Move any driving key to another key. Enter on a control, then \
                     press the key you want for it.",
                ),
                MenuItem::new(
                    "Controller buttons",
                    |_s: &mut SettingsCategoryState, ctx| {
                        ctx.push_state(ShortcutsState::new(ShortcutDevice::Controller))
                    },
                )
                .help(
                    "Move any driving control to another pad button, plain or with \
                     the right bumper held.",
                ),
                // The speed keeper moved to Driving assistance: it holds a speed
                // for you, which is what every other row on that screen does.
                // Controls is the keyboard, the controller, and the units the
                // numbers arrive in.
            ]
            .into_iter()
            .chain(cfg!(target_os = "ios").then(touch_gestures_row))
            .chain([back_row()])
            .collect(),
            "audio" => self.audio_items(),
            "speech" => {
                let mut items: Vec<Row> = self
                    .speech_control_specs(ctx)
                    .into_iter()
                    .map(|spec| row(spec.label, spec.action, spec.help))
                    .collect();
                items.push(back_row());
                items
            }
            "reports" => vec![
                MenuItem::new(
                    "Where the game log is saved",
                    |s: &mut SettingsCategoryState, ctx| s.say_log_location(ctx),
                )
                .help(
                    "The session log records everything the game said out \
                     loud. Send it with a bug report.",
                ),
                back_row(),
            ],
            _ => vec![
                row(
                    dyn_label(|s| {
                        format!(
                            "Update channel: {}",
                            if update_channel(s) == "dev" {
                                "developer snapshots"
                            } else {
                                "stable releases"
                            }
                        )
                    }),
                    adjust(|s, ctx, d| s.toggle_update_channel(ctx, d)),
                    "Stable releases or developer snapshots.",
                ),
                MenuItem::new(
                    "Check for updates",
                    |_s: &mut SettingsCategoryState, ctx| ctx.push_state(UpdateCheckState::new()),
                )
                .help("Look for a new version now."),
                back_row(),
            ],
        }
    }

    fn assistance_items(&self) -> Vec<Row> {
        let mut items = vec![row(
            dyn_label(|s| format!("Driving assistance preset: {}", assist_preset_label(s))),
            adjust(|s, ctx, d| s.cycle_assist_preset(ctx, d)),
            "Realistic is modern truck safety support. Balanced adds partial lane keeping, a firmer hand on descents, and stopping at your destination. All assists turns on every assist and sets lane keeping to full: the truck holds the lane, a tap changes lanes, and your destination exit is taken for you. Changing one assist makes this Custom. You still choose routes and handle yards and docks. Presets never change pacing, hours rules, transmission, weather, or hazards.",
        )];
        for (field, label, help_text) in DRIVING_ASSIST_SPECS {
            items.push(row(
                dyn_label(move |s| format!("{label}: {}", assist_value_text(s, field))),
                adjust(move |s, ctx, d| s.toggle_driving_assist(ctx, field, d)),
                help_text,
            ));
        }
        items.push(row_dyn_help(
            dyn_label(|s| format!("Lane keeping: {}", lane_keeping_label(s))),
            adjust(|s, ctx, d| s.cycle_lane_keeping(ctx, d)),
            |s| {
                "How much of the lane-holding work the truck does. Full \
                 holds the lane, turns {{steer_left}} and {{steer_right}} into tap lane \
                 changes, and takes your exits, including the destination \
                 exit, without a signal. Partial steers the truck through \
                 the road's bends and drifts gently, with generous steering \
                 help; lane changes and speed are yours. Off drifts like a \
                 real wheel; bends are yours unless curve assistance is \
                 on, and every exit needs its signal and its exit lane. On \
                 partial or off {lean}. The road sound only says where you \
                 sit in your lane, and the road edge answers: a stutter \
                 clipping the rumble strip, a buzz fully on it, gravel off \
                 the pavement. Steering guide and Lane guide sound, under \
                 Audio, change which way to steer and what leans. \
                 Realistic sets this off, Balanced partial, All assists \
                 full."
                    .replace("{lean}", &steering_lean_text(s))
            },
        ));
        items.push(row(
            dyn_label(|s| format!("Following gap: {}", acc_gap_label(s))),
            adjust(|s, ctx, d| s.cycle_acc_gap(ctx, d)),
            "How much room adaptive cruise leaves to the vehicle \
             ahead. Close is two and a half seconds, normal three, far \
             three and a half. Weather widens the gap whichever you \
             pick. All three stay clear of a following-too-close \
             citation. The preset above never changes it.",
        ));
        // Lane and edge cue volume moved to Audio, next to the Gameplay
        // cues volume it scales. It is a volume, and a second volume
        // control hiding in the assists list is how it came to be a row
        // nobody could explain.
        items.push(back_row());
        items
    }

    fn audio_items(&self) -> Vec<Row> {
        vec![
            row(
                dyn_label(|s| format!("Master volume: {} percent", pct(s.master_volume))),
                adjust(|s, ctx, d| s.volume(ctx, "master_volume", 0.1 * d as f64)),
                "Overall game volume.",
            ),
            row(
                dyn_label(|s| format!("Gameplay cues volume: {} percent", pct(s.sfx_volume))),
                adjust(|s, ctx, d| s.volume(ctx, "sfx_volume", 0.1 * d as f64)),
                "Horn, alerts, road, facility, and gameplay cue sounds.",
            ),
            row(
                dyn_label(|s| format!("Lane and edge cue volume: {}", cue_loudness_label(s))),
                adjust(|s, ctx, d| s.cycle_cue_loudness(ctx, d)),
                "How loud the lane and edge cues are next to everything \
                 else: the rumble-strip and shoulder textures, the lane \
                 locator on I, and the warning bars before a hairpin. It \
                 rides on the Gameplay cues volume above and moves those \
                 cues alone. Quieter sits under the engine, standard \
                 matches it, louder cuts through.",
            ),
            row(
                dyn_label(|s| {
                    format!(
                        "Steering guide: {}",
                        if s.steering_guide_inverted {
                            "steer away from the engine"
                        } else {
                            "steer toward the engine"
                        }
                    )
                }),
                adjust(|s, ctx, d| s.toggle_steering_guide_inverted(ctx, d)),
                "Which way to steer when the engine sound moves to one side. \
                 Toward the engine is the default: the engine moves to the \
                 side you have to turn to, then back to the middle as you \
                 turn. Away from the engine flips it, for drivers used to \
                 that from audio racing games.",
            ),
            row(
                dyn_label(|s| {
                    format!(
                        "Lane guide sound: {}",
                        if s.lane_guide_tone { "tone" } else { "engine" }
                    )
                }),
                adjust(|s, ctx, d| s.toggle_lane_guide_tone(ctx, d)),
                "What leans toward the side to steer. The engine is the \
                 default: the engine you already hear, moving toward the \
                 side you need and coming back to the middle once you are \
                 through. Tone plays a soft note instead, panned the same \
                 way, for setups where the engine is hard to place. A held \
                 note is tiring over a long haul.",
            ),
            row(
                dyn_label(|s| format!("Weather sounds volume: {} percent", pct(s.weather_volume))),
                adjust(|s, ctx, d| s.volume(ctx, "weather_volume", 0.1 * d as f64)),
                "Rain, wind, thunder, snow, and fog sounds.",
            ),
            row(
                dyn_label(|s| format!("Engine sounds volume: {} percent", pct(s.engine_volume))),
                adjust(|s, ctx, d| s.volume(ctx, "engine_volume", 0.1 * d as f64)),
                "Engine start, shutdown, and running engine sounds.",
            ),
            row(
                dyn_label(|s| format!("Engine sound: {}", s.engine_voice)),
                adjust(|s, ctx, d| s.toggle_engine_voice(ctx, d)),
                "Real is the engine recorded from a working truck cab, \
                 following the rpm. Classic is the original engine sound. \
                 Changes apply at once, even while driving.",
            ),
            row(
                dyn_label(|s| {
                    format!(
                        "Engine brake sound: {}",
                        if s.jake_voice == "real" { "recorded" } else { "classic" }
                    )
                }),
                adjust(|s, ctx, d| s.toggle_jake_voice(ctx, d)),
                "Recorded is the real engine brake growl, the jake. \
                 Classic is the synthesized growl from earlier versions. \
                 Changes apply at once, even while driving.",
            ),
            row(
                dyn_label(|s| format!("Music volume: {} percent", pct(s.music_volume))),
                adjust(|s, ctx, d| s.volume(ctx, "music_volume", 0.1 * d as f64)),
                "Menu and facility background music volume.",
            ),
            row(
                dyn_label(|s| {
                    format!(
                        "Music source: {}",
                        if s.synth_music { "Synthesized" } else { "Original" }
                    )
                }),
                adjust(|s, ctx, d| s.toggle_music_source(ctx, d)),
                "Synthesized plays menu music and the Roadhouse station made by the \
                 game itself, with no AI-made songs or voices, and takes Freight \
                 Fate's other stations off the dial. Original plays the full \
                 soundtrack.",
            ),
            // Enter types a seed; Left and Right roll one (the adjust table).
            MenuItem::new(
                dyn_label(|s| format!("Music seed: {}", s.music_seed)),
                |_s: &mut SettingsCategoryState, ctx| {
                    ctx.push_state(SettingsCategoryState::music_seed_entry())
                },
            )
            .help(
                "Enter types a seed, a whole number. Left or Right rolls a new one. \
                 Every synthesized piece changes with it.",
            ),
            row(
                dyn_label(|s| format!("In-cab radio volume: {} percent", pct(s.radio_volume))),
                adjust(|s, ctx, d| s.volume(ctx, "radio_volume", 0.1 * d as f64)),
                "Radio volume while driving. Low by default so speech, engine, and safety cues stay clear.",
            ),
            row(
                dyn_label(|s| {
                    format!("Radio streamer-safe mode: {}", on_off(s.radio_streamer_safe))
                }),
                adjust(|s, ctx, d| s.toggle_radio_streamer_safe(ctx, d)),
                "Off plays the full dial, including real public streams and \
                 personal playlists. On keeps the radio to built-in safe \
                 stations, for streaming or recording. With Music source set to \
                 Synthesized, On keeps the radio on the Roadhouse and station \
                 keys do nothing.",
            ),
            row(
                dyn_label(|s| {
                    format!(
                        "Shuffle personal playlists: {}",
                        on_off(s.radio_shuffle_playlists)
                    )
                }),
                adjust(|s, ctx, d| s.toggle_radio_shuffle_playlists(ctx, d)),
                "On plays each of your playlists in a random order, every track \
                 once before any repeats. Off plays the file top to bottom.",
            ),
            row(
                dyn_label(|s| {
                    let sound = radio_sound_name(s.radio_cab_speakers);
                    format!("Radio sound: {sound}")
                }),
                adjust(|s, ctx, d| s.toggle_radio_cab_speakers(ctx, d)),
                "Cab speakers plays the radio through the truck's door \
                 speakers in the small cab, and lowering the driver window \
                 brightens it. Clean plays it exactly as the station sends it.",
            ),
            row(
                dyn_label(|s| {
                    format!(
                        "Game sounds step back for speech: {}",
                        on_off(s.duck_audio_for_speech)
                    )
                }),
                adjust(|s, ctx, d| s.toggle_duck_for_speech(ctx, d)),
                "While the road voice speaks, the engine, weather, and \
                 radio drop to half volume, then come back.",
            ),
            row(
                dyn_label(|s| format!("Menu and UI sounds volume: {} percent", pct(s.ui_volume))),
                adjust(|s, ctx, d| s.volume(ctx, "ui_volume", 0.1 * d as f64)),
                "Menu movement, selection, warning, and cash sounds.",
            ),
            back_row(),
        ]
    }
}
