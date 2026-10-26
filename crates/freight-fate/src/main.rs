// The game is a windowed application, so the binary is linked into the
// Windows GUI subsystem: no console window is ever created for it. The
// Python build did the same (`tools/build_release.py` passed Nuitka
// `--windows-console-mode=disable`); the port had lost it, and a packaged
// launch opened a black terminal beside the game. For a screen reader that
// is a second window in the player's world, competing for focus, so this is
// not cosmetic. `console::attach_parent` below gives the drive tools their
// output back.
#![cfg_attr(windows, windows_subsystem = "windows")]

//! `freightfate`: the game's entry point (`freight_fate/__main__.py`), plus
//! the drive tools that used to be separate Python scripts.
//!
//! Game switches: `--smoke` (boot, render five frames, exit 0 -- the CI build
//! check), `--headless` (no window, no speech), `--controller-diagnostics`
//! (the two-layer controller logger, see `controller::diagnostics`). Those
//! are parsed by `app::CliOptions` and handled by `app::main_with`.
//!
//! Tool switches, parsed here:
//!
//! * `--list-break-scenarios` -- name and one-line summary of every
//!   adversarial scenario (formerly `tools/playtest_break.py --list`).
//! * `--list-speech-backends` -- every screen reader and voice Prism knows
//!   on THIS machine, which of them can speak right now, and which one the
//!   game would choose. There is no allowlist in the game, so a reader the
//!   maintainers cannot install is answered by its own user running this.
//! * `--break-scenario NAME` / `--break-battery` `[--transcript]` -- run one
//!   scenario, or all of them, and print the verdict table.
//! * `--key-probe [--seconds N]` -- measure what the keyboard delivers to the
//!   game, screen reader included (formerly `tools/key_probe.py`).
//! * `--playtest-sandbox` -- prepare (and with `--launch`, run the real game
//!   in) a data directory that cannot reach the owner's account
//!   (formerly `tools/playtest_sandbox.py`).
//! * `--playtest-road --find FEATURE` -- start the real game at a named road
//!   feature, or at the loaded facility gate for `departure`
//!   (formerly `tools/playtest_road.py`).
//!
//! # Why the tool parsing is here and not in `app::CliOptions`
//!
//! `CliOptions` lives in `app.rs`, which this task does not own. Rather than
//! reach into a file another port owns, the tool switches are recognised here
//! and everything unrecognised is handed on to `CliOptions::parse` exactly as
//! `tools/playtest_sandbox.py` handed its own leftovers to
//! `freight_fate.app.main`. A later change can fold these into `CliOptions`
//! without moving any behaviour.

use std::path::PathBuf;

use freight_fate::app::{self, App, CliOptions};
use freight_fate::playtest::{breaker, observer, road, sandbox};
use freight_fate::speech::CaptureSpeech;

fn main() {
    #[cfg(target_os = "ios")]
    ios::run_under_uikit();
    #[cfg(not(target_os = "ios"))]
    std::process::exit(game_main());
}

/// The game process proper. On iOS this runs inside UIKit's application
/// loop, handed over by SDL once the app has finished launching.
fn game_main() -> i32 {
    // First of all, so the opening phase mark charges process creation and
    // dynamic linking to the launch instead of losing them.
    app::boot_timing::start();
    // This process is the game, so it -- and only it -- may reach the real
    // world: a web page in front of the player, the network, the player's own
    // save folder, and the platform store holding their driver token. Every
    // other process that links the crate, the test binaries above all, never
    // runs this function and so never gets any of these, and each door then
    // records the attempt and refuses instead of reaching the live thing.
    // See `freight_fate::browser`, `freight_fate::net`,
    // `ff_core::settings::paths` and `freight_fate::online_presence`.
    freight_fate::browser::allow_real_browser();
    freight_fate::net::allow_real_network();
    ff_core::settings::paths::allow_real_save_dir();
    freight_fate::online_presence::allow_real_secret_store();
    let args: Vec<String> = std::env::args().skip(1).collect();
    // Before anything writes a byte: a GUI-subsystem process starts with no
    // console and, on every shell tested, no standard handles either, so an
    // un-attached tool run prints into the void.
    console::attach_parent(&args);
    run(&args)
}

/// iOS start-up: UIKit owns the main thread, so SDL starts the application
/// and calls back into [`game_main`] once it has launched. The game loop then
/// runs on the main thread exactly as on the desktop, SDL pumping UIKit's
/// run loop with every event poll.
#[cfg(target_os = "ios")]
mod ios {
    use std::ffi::{c_char, c_int, CString};

    type SdlMain = extern "C" fn(c_int, *mut *mut c_char) -> c_int;

    extern "C" {
        fn SDL_UIKitRunApp(argc: c_int, argv: *mut *mut c_char, main: SdlMain) -> c_int;
    }

    extern "C" fn sdl_main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
        super::game_main()
    }

    pub fn run_under_uikit() -> ! {
        let args: Vec<CString> = std::env::args_os()
            .filter_map(|arg| CString::new(arg.into_encoded_bytes()).ok())
            .collect();
        let mut argv: Vec<*mut c_char> = args.iter().map(|arg| arg.as_ptr().cast_mut()).collect();
        let argc = argv.len() as c_int;
        argv.push(std::ptr::null_mut());
        // SAFETY: `argv` is a NULL-terminated array of NUL-terminated strings
        // that `args` keeps alive for the whole call, which does not return
        // until the application ends.
        let code = unsafe { SDL_UIKitRunApp(argc, argv.as_mut_ptr(), sdl_main) };
        std::process::exit(code)
    }
}

/// Giving the drive tools their terminal back, without giving the player one.
///
/// `AttachConsole` never CREATES a console (that is `AllocConsole`), so it
/// cannot put a window on screen; it only borrows the one the parent shell
/// already owns. Nothing here can undo the guarantee at the top of the file.
mod console {
    /// Attach to the parent shell's console, when there is a reason to.
    ///
    /// # When
    ///
    /// Only when the command line carries at least one argument. A player
    /// launches Freight Fate with none -- a double-click, a shortcut, the
    /// Start menu -- and that launch stays completely detached: not attached
    /// to any console, so it cannot be killed by a Ctrl-C meant for the shell
    /// or by that shell's window closing. Every path that PRINTS needs an
    /// argument to reach it: `--help`, `-h`, `/?`, an unrecognized switch
    /// (which prints the usage and exits 2), `--list-break-scenarios`,
    /// `--break-scenario`, `--break-battery`, `--playtest-sandbox`,
    /// `--playtest-road`, `--smoke`, `--headless`. Keying on "any argument at
    /// all" rather than on a list of switches means a switch added later
    /// cannot be forgotten here and silently lose its output.
    #[cfg(windows)]
    pub fn attach_parent(args: &[String]) {
        use windows_sys::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
        use windows_sys::Win32::System::Console::{
            AttachConsole, GetStdHandle, SetStdHandle, ATTACH_PARENT_PROCESS, STD_ERROR_HANDLE,
            STD_INPUT_HANDLE, STD_OUTPUT_HANDLE,
        };

        if args.is_empty() {
            return;
        }
        // SAFETY: three kernel32 calls that take no pointers of ours. The
        // handles are the process's own standard handles, borrowed and put
        // back; none is closed here.
        unsafe {
            // Redirection has to survive the attach. `cmd.exe` hands a
            // redirected child its file or pipe in the standard slots WITHOUT
            // setting `STARTF_USESTDHANDLES`, and `AttachConsole` then
            // overwrites exactly those slots with the console's own handles:
            // measured on 2026-08-23, `freightfate --break-battery > out.txt`
            // from cmd left out.txt zero bytes and printed the whole run to
            // the terminal instead. So remember what was there and restore
            // whatever was real.
            let saved: [(u32, HANDLE); 3] = [
                (STD_OUTPUT_HANDLE, GetStdHandle(STD_OUTPUT_HANDLE)),
                (STD_ERROR_HANDLE, GetStdHandle(STD_ERROR_HANDLE)),
                (STD_INPUT_HANDLE, GetStdHandle(STD_INPUT_HANDLE)),
            ];
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                // No parent console: a double-click, or a launcher that has
                // none. Nothing to print to and nothing to fix.
                return;
            }
            for (slot, previous) in saved {
                // An empty slot is left as `AttachConsole` set it: it fills
                // the standard handles from the console it just attached,
                // which is what makes a plain terminal run print at all.
                if !previous.is_null() && previous != INVALID_HANDLE_VALUE {
                    SetStdHandle(slot, previous);
                }
            }
        }
    }

    #[cfg(not(windows))]
    pub fn attach_parent(_args: &[String]) {}
}

fn run(args: &[String]) -> i32 {
    if has(args, "--help") || has(args, "-h") || has(args, "/?") {
        print!("{USAGE}");
        return 0;
    }
    // An unrecognized switch used to fall through to `CliOptions::parse`, which
    // ignores what it does not know -- so `--help` LAUNCHED THE GAME. Someone
    // asking what the flags are gets a window and a main menu instead of an
    // answer, and on a machine already running a copy the single-instance
    // guard then refuses it for reasons that look unrelated. Say what is
    // wrong, print the usage, and exit non-zero.
    if let Some(unknown) = args.iter().find(|a| {
        a.starts_with('-')
            && !KNOWN_SWITCHES
                .iter()
                .any(|k| *k == a.as_str() || a.starts_with(&format!("{k}=")))
    }) {
        eprintln!("freightfate: unrecognized switch {unknown}\n");
        eprint!("{USAGE}");
        return 2;
    }
    if args.iter().any(|a| a == "--list-break-scenarios") {
        return list_break_scenarios();
    }
    if args.iter().any(|a| a == "--list-speech-backends") {
        return list_speech_backends();
    }
    if let Some(name) = flag_value(args, "--break-scenario") {
        return run_break(&[name], has(args, "--transcript"));
    }
    if has(args, "--break-battery") {
        let names: Vec<String> = breaker::scenario_names()
            .into_iter()
            .map(str::to_string)
            .collect();
        return run_break(&names, has(args, "--transcript"));
    }
    if has(args, "--playtest-sandbox") {
        return playtest_sandbox(args);
    }
    if has(args, "--playtest-road") {
        return playtest_road(args);
    }
    if has(args, "--agent-server") {
        return agent_server(args);
    }
    if has(args, "--key-probe") {
        return key_probe(args);
    }
    app::main_with(CliOptions::parse(args.iter().cloned()))
}

/// `--key-probe [--seconds N] [--log PATH]`: show what the keyboard delivers
/// to the game (see `app::key_probe`), for a player whose screen reader
/// re-sends held keys. Speaks its findings and writes them to a log.
fn key_probe(args: &[String]) -> i32 {
    let seconds = flag_value(args, "--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(25);
    let mut app = match App::new() {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Could not start the key probe: {e}");
            return 1;
        }
    };
    let report = app.run_key_probe(seconds);
    let path = flag_value(args, "--log")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let stamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_secs());
            PathBuf::from(format!("key_probe-{stamp}.log"))
        });
    let text = report.text();
    println!("{text}");
    let saved = match std::fs::write(&path, &text) {
        Ok(()) => format!("The log is saved as {}.", path.display()),
        Err(e) => format!("The log could not be saved: {e}."),
    };
    println!("{saved}");
    let spoken = format!("{} {saved}", report.spoken());
    app.ctx.say(&spoken);
    // The speech worker talks on its own thread; give it time to finish
    // before the process, and the voice with it, goes away.
    let words = spoken.split_whitespace().count() as f64;
    std::thread::sleep(std::time::Duration::from_secs_f64(
        (1.0 + words / 2.5).min(30.0),
    ));
    0
}

#[cfg(feature = "agent-server")]
fn agent_server(args: &[String]) -> i32 {
    let launch = flag_value(args, "--find").map(|feature| freight_fate::agent_server::LaunchAt {
        feature,
        origin: flag_value(args, "--from"),
        destination: flag_value(args, "--to"),
        seed: flag_value(args, "--seed")
            .and_then(|s| s.parse().ok())
            .unwrap_or(7),
    });
    freight_fate::agent_server::run(
        has(args, "--reset"),
        launch,
        has(args, "--operator-keys"),
        has(args, "--staging"),
    )
}

/// Player builds leave the agent server out (the `agent-server` feature).
#[cfg(not(feature = "agent-server"))]
fn agent_server(_args: &[String]) -> i32 {
    eprintln!(
        "This build has no agent server. Build one with \
         `cargo build --release -p freight-fate --bin freightfate`."
    );
    2
}

/// Every switch the binary answers to, for the unrecognised-switch check.
/// A new switch must be added here or it will be refused -- deliberately: a
/// silent fall-through is what made `--help` launch the game.
const KNOWN_SWITCHES: &[&str] = &[
    "--agent-server",
    "--operator-keys",
    "--staging",
    "--ai",
    "--assists",
    "--at",
    "--break-battery",
    "--break-scenario",
    "--cargo",
    "--cargo-type",
    "--controller-diagnostics",
    "--cruise",
    "--curve-assist",
    "--descent",
    "--dir",
    "--find",
    "--facility",
    "--from",
    "--headless",
    "--help",
    "--hour",
    "--lane-keeping",
    "--key-probe",
    "--launch",
    "--lead",
    "--level",
    "--list-break-scenarios",
    "--list-speech-backends",
    "--log",
    "--max-advisory",
    "--max-miles",
    "--min-drop",
    "--min-pct",
    "--min-run",
    "--no-careers",
    "--no-cruise",
    "--no-sandbox",
    "--pick",
    "--planned-stop-assist",
    "--playtest-road",
    "--playtest-sandbox",
    "--predictive-cruise",
    "--print",
    "--reset",
    "--routes",
    "--sample",
    "--scan",
    "--seconds",
    "--seed",
    "--smoke",
    "--speed",
    "--to",
    "--transcript",
    "--transmission",
    "--trip-seed",
    "--verbosity",
    "--weather",
];

/// What `--help` prints. Kept in step with the module docs above.
const USAGE: &str = "freightfate -- Freight Fate

  freightfate                       play
  freightfate --smoke               boot, render five frames, exit (CI check)
  freightfate --headless            no window, no speech
  freightfate --controller-diagnostics
                                    log controller events, both layers

Drive tools:
  --playtest-road --find FEATURE    start at a road feature or departure gate
                                    (downgrade, upgrade, zone, limit-drop,
                                    stop, scale, curve, interchange, toll,
                                    chain-law, destination, departure, all); all scans every
                                    launchable road family and current world instance.
                                    Destination is
                                    the delivery off-ramp at the end of the
                                    route; departure scans a small route sample
                                    for loaded facility exits by default. Use
                                    --routes all for an exhaustive inventory;
                                    each result prints a direct --facility launch
                                    that does not rescan the world. State
                                    transitions requiring a live player history are not road-launchable;
                                    --from/--to/--seed/--scan and
                                    the assist switches refine the search
  --ai                               with a selected road scenario, use paced
                                    player controls in the real window; live
                                    player-facing speech streams to the terminal
  --playtest-sandbox [--launch]     a data directory that cannot reach the
                                    owner's account; --dir/--reset/--print
  --list-break-scenarios            name every adversarial scenario
  --list-speech-backends            name every screen reader and voice found
  --break-scenario NAME             run one, --transcript to hear it
  --break-battery                   run them all and print the verdicts
  --agent-server                    the real game with an MCP server on stdio,
                                    so an AI agent can play it (sandboxed;
                                    --reset for a fresh sandbox; add
                                    --find FEATURE [--from CITY] [--to CITY]
                                    to boot straight into a staged drive;
                                    --operator-keys keeps the window up and
                                    lets the operator's keyboard in, to play
                                    alongside the agent; --staging uses its
                                    own directory and its own driver on the
                                    staging site, cloud backup on)
  --key-probe [--seconds N]         show what your keyboard delivers to the game,
                                    for a screen reader that re-sends held keys
                                    (JAWS): hold each arrow key for a few seconds;
                                    it speaks its findings and writes a log
  --log PATH                        session log for the watcher
";

// -- flag helpers ----------------------------------------------------------------------

fn has(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

/// `--name value` or `--name=value`.
fn flag_value(args: &[String], name: &str) -> Option<String> {
    let prefix = format!("{name}=");
    for (i, arg) in args.iter().enumerate() {
        if let Some(value) = arg.strip_prefix(&prefix) {
            return Some(value.to_string());
        }
        if arg == name {
            return args.get(i + 1).cloned();
        }
    }
    None
}

fn flag_f64(args: &[String], name: &str) -> Option<f64> {
    flag_value(args, name).and_then(|v| v.parse().ok())
}

fn flag_i64(args: &[String], name: &str) -> Option<i64> {
    flag_value(args, name).and_then(|v| v.parse().ok())
}

fn flag_usize(args: &[String], name: &str) -> Option<usize> {
    flag_value(args, name).and_then(|v| v.parse().ok())
}

/// A `--name on|off` switch.
fn flag_on_off(args: &[String], name: &str) -> Option<bool> {
    flag_value(args, name).map(|v| v == "on")
}

// -- the adversarial battery -----------------------------------------------------------

fn list_break_scenarios() -> i32 {
    let scenarios = breaker::scenarios();
    let width = scenarios.iter().map(|s| s.name.len()).max().unwrap_or(0);
    for scenario in scenarios {
        println!("{:<width$}  {}", scenario.name, scenario.description);
    }
    0
}

/// Every backend Prism registers here, and what the game makes of each.
///
/// The game has no list of approved screen readers: `pick_backend` walks the
/// registry in priority order and keeps the first whose own runtime check
/// says it can speak. So ZDSR, PC-Talker, BoYing, SenseReader, System Access
/// and ZoomText are already chosen automatically wherever they run -- and
/// that is precisely what nobody here can confirm, because none of those
/// readers can be installed on this machine. This switch moves the question
/// to somebody who has one: run it, read the table, send it back.
fn list_speech_backends() -> i32 {
    use freight_fate::speech::{narrator_running, usable, PrismRegistry, VoiceRegistry};

    let registry = match PrismRegistry::new() {
        Ok(registry) => registry,
        Err(err) => {
            eprintln!("Prism did not start, so nothing here can speak: {err}");
            return 1;
        }
    };
    let chosen = freight_fate::speech::pick_backend(&registry, None).map(|backend| backend.name());
    println!(
        "{:<20} {:>8}  {:<10} NOTE",
        "BACKEND", "PRIORITY", "CAN SPEAK"
    );
    for index in 0..registry.backend_count() {
        let Some(id) = registry.id_at(index) else {
            continue;
        };
        let name = registry.name_of(id).unwrap_or_else(|| id.to_string());
        // UIA exists on every modern Windows but only Narrator reads it, so
        // it is answered before the acquire that would fail confusingly.
        if name == "UIA" && !narrator_running() {
            println!(
                "{:<20} {:>8}  {:<10} Narrator is not running",
                name,
                registry.priority_of(id),
                "no"
            );
            continue;
        }
        let (speaks, note) = match registry.acquire(id) {
            Ok(backend) => {
                let features = backend.features();
                let note = if Some(name.clone()) == chosen {
                    "chosen"
                } else if !features.is_supported_at_runtime {
                    "not running here"
                } else {
                    ""
                };
                (usable(backend.as_ref()), note.to_string())
            }
            // The ordinary case for every reader this machine does not have.
            Err(freight_fate::speech::backend::SpeechError::BackendNotAvailable) => {
                (false, "not installed or not running here".to_string())
            }
            Err(err) => (false, err.to_string()),
        };
        println!(
            "{:<20} {:>8}  {:<10} {}",
            name,
            registry.priority_of(id),
            if speaks { "yes" } else { "no" },
            note
        );
    }
    match chosen {
        Some(name) => println!(
            "
The game would speak through {name}."
        ),
        None => println!(
            "
Nothing on this machine can speak; the game would run mute."
        ),
    }
    0
}

fn run_break(names: &[String], transcript: bool) -> i32 {
    app::configure_logging();
    let known = breaker::scenario_names();
    let unknown: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|name| !known.contains(name))
        .collect();
    if !unknown.is_empty() {
        eprintln!(
            "unknown scenario: {} (try --list-break-scenarios)",
            unknown.join(", ")
        );
        return 2;
    }
    let mut outcomes = Vec::new();
    for name in names {
        println!("running {name} ...");
        let Some(outcome) = breaker::run_scenario(name) else {
            continue;
        };
        for finding in &outcome.findings {
            let shown: String = finding.replace('\n', " ").chars().take(300).collect();
            println!("  [{}] {shown}", outcome.verdict.as_str());
        }
        if transcript {
            for line in &outcome.transcript {
                println!("    | {line}");
            }
        }
        outcomes.push(outcome);
    }
    breaker::print_summary(&outcomes);
    let errors = outcomes
        .iter()
        .filter(|o| o.verdict == breaker::Verdict::Error)
        .count();
    i32::from(errors > 0)
}

// -- the sandbox launcher ---------------------------------------------------------------

fn playtest_sandbox(args: &[String]) -> i32 {
    let dir = flag_value(args, "--dir")
        .map(PathBuf::from)
        .unwrap_or_else(sandbox::default_sandbox);
    let source = sandbox::real_saves();
    if let Err(e) = sandbox::prepare(
        &dir,
        has(args, "--reset"),
        !has(args, "--no-careers"),
        &source,
    ) {
        eprintln!("Could not prepare the sandbox: {e}");
        return 1;
    }
    println!("{}", sandbox::describe(&dir));
    if !sandbox::audit(&dir).is_empty() {
        // Refusing is the point. A sandbox that is only mostly isolated is
        // worse than no sandbox at all, because the operator stops watching.
        eprintln!("\nRefusing to launch: fix the above, or pass --reset.");
        return 1;
    }
    if has(args, "--print") {
        println!("\nFREIGHT_FATE_DATA_DIR={}", dir.display());
        return 0;
    }
    if !has(args, "--launch") {
        return 0;
    }
    let log_path = flag_value(args, "--log")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            ff_core::settings::game_root()
                .join("logs")
                .join("playtest-manual.log")
        });
    open_log(&log_path);
    println!("\nSession log: {}", log_path.display());
    let session = sandbox::open_session(&dir, &log_path);
    println!("Session file: {}", session.display());

    // Anything unrecognised is handed straight to the game -- `--smoke` above
    // all, which is how a sandbox launch gets tested without a person having
    // to sit through a window opening.
    let passthrough: Vec<String> = args
        .iter()
        .filter(|a| {
            !matches!(
                a.as_str(),
                "--playtest-sandbox" | "--reset" | "--no-careers" | "--launch" | "--print"
            ) && !a.starts_with("--dir")
                && !a.starts_with("--log")
        })
        .cloned()
        .collect();
    let code = app::main_with(CliOptions::parse(passthrough));
    sandbox::close_session();
    code
}

// -- the road launcher ------------------------------------------------------------------

fn playtest_road(args: &[String]) -> i32 {
    let headless = flag_f64(args, "--headless").unwrap_or(0.0);
    let opts = road_options(args);

    if has(args, "--ai") && headless > 0.0 {
        eprintln!("--ai requires the real window; remove --headless.");
        return 1;
    }
    if has(args, "--ai") && opts.lane_keeping.as_deref() != Some("full") {
        eprintln!(
            "--ai requires full lane keeping; remove the conflicting --lane-keeping setting."
        );
        return 1;
    }

    if headless == 0.0 && !opts.scan {
        // `playtest_road` builds its own app and runs it, which means it never
        // passes through the guard `app::main_with` uses. Launching one while
        // a game was already open gave the owner two windows, the new drive
        // rolling unfocused behind the old one, and an open weigh station that
        // came and went unheard (2026-08-21). A bench is exempt: it opens no
        // window and several are fine at once -- and so is `--scan`, which
        // `road::plan` answers off the world data and returns before any app
        // is built. Refusing a search because a drive is open cost the tool
        // its one use during a playtest: finding where to go next.
        let mut guard = freight_fate::single_instance::SingleInstanceGuard::new(
            freight_fate::single_instance::SINGLE_INSTANCE_MUTEX_NAME,
        );
        if !guard.acquire() {
            eprintln!(
                "Freight Fate is already running. Close it first. A second window would \
                 start rolling behind the one you are looking at."
            );
            return 1;
        }
        let code = road_session(&opts, headless, args);
        guard.release();
        return code;
    }
    road_session(&opts, headless, args)
}

fn road_session(opts: &road::RoadOptions, headless: f64, args: &[String]) -> i32 {
    let ai = has(args, "--ai");
    // ON BY DEFAULT since 2026-08-20, because the opt-in version was a trap.
    // A bench run started to answer one question about the engine brake, with
    // the flag simply not typed, created a "Playtest" career in the real save
    // directory and uploaded it to the owner's account minutes after that
    // account had been deliberately emptied. The tool cannot tell a throwaway
    // career from a real one; the only safe default is the one that cannot
    // reach the account at all.
    let sandbox_dir = if opts.sandbox {
        let dir = sandbox::default_sandbox();
        if let Err(e) = sandbox::prepare(&dir, false, true, &sandbox::real_saves()) {
            eprintln!("Could not prepare the sandbox: {e}");
            return 1;
        }
        println!("{}", sandbox::describe(&dir));
        if !sandbox::audit(&dir).is_empty() {
            eprintln!("Refusing to drive: the sandbox is not isolated.");
            return 1;
        }
        println!();
        Some(dir)
    } else {
        None
    };

    // A bench and a live drive both defaulting to playtest.log meant a bench
    // run mid-session rotated the transcript out from under the person
    // driving, and pointed the session watcher at the wrong road.
    let log_path = match &opts.log {
        Some(path) => PathBuf::from(path),
        None if headless > 0.0 => ff_core::settings::game_root()
            .join("logs")
            .join("playtest-bench.log"),
        None => ff_core::settings::game_root()
            .join("logs")
            .join("playtest.log"),
    };
    open_log(&log_path);
    if headless > 0.0 {
        for (name, value) in [
            ("FREIGHT_FATE_NO_SPEECH", "1"),
            ("SDL_VIDEODRIVER", "dummy"),
            ("SDL_AUDIODRIVER", "dummy"),
        ] {
            if std::env::var_os(name).is_none() {
                std::env::set_var(name, value);
            }
        }
    }

    // Pick the spot first, against the world data alone: nothing below needs
    // a window until we actually drive.
    let hit = match road::plan(opts) {
        road::RoadPlan::Done(code) => return code,
        road::RoadPlan::Drive(hit) => hit,
    };

    let mut observer = if ai {
        match observer::AutonomousObserver::new(hit.clone()) {
            Ok(observer) => Some(observer),
            Err(boundary) => {
                eprintln!("{boundary}");
                return 1;
            }
        }
    } else {
        None
    };

    if ai {
        std::env::set_var("FREIGHT_FATE_STREAM_TRANSCRIPT", "1");
    }
    app::configure_logging();
    if headless > 0.0 {
        let mut app = App::new_headless(Box::new(CaptureSpeech::new()));
        let (driving, start_mi) = road::build_driving(&mut app.ctx, &hit, opts);
        road::print_setup(&mut app.ctx, &hit, start_mi, opts);
        app.push_state(driving);
        let frames = (60.0 * 60.0 * headless) as usize;
        for _ in 0..frames {
            app.tick(1.0 / 60.0);
        }
        app.shutdown();
        println!("\nBench done. Transcript written to {}", log_path.display());
        return 0;
    }

    let mut app = match App::new() {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Fatal error: {e}");
            return 1;
        }
    };
    // The session must not leak its lane-keeping/assists overrides into the
    // player's real settings: shutdown saves settings on the way out, which
    // persisted a playtest's flags as the player's own choices (owner-hit
    // 2026-07-27: the steering override became the saved setting).
    let settings_path = ff_core::settings::data_dir().join("settings.json");
    let settings_before = std::fs::read(&settings_path).ok();

    // `App::run` takes its first screen from `set_initial_state`, so the
    // staged drive simply IS the screen the loop starts on -- and quitting to
    // the main menu reaches the REAL menu, with its working Exit.
    app.set_initial_state(road_builder(hit, args));
    if let Some(dir) = &sandbox_dir {
        // Announce the live session so cargo tools playtest-watch can follow this
        // drive the same way it follows a sandbox launch.
        sandbox::open_session(dir, &log_path);
    }
    if observer.is_some() {
        println!("\n  AI observer is using the normal frame/input path. Player-facing speech streams below.");
        println!("  It exits at the discovered feature, a keeper-to-cruise handoff, or a named safety boundary.");
    } else {
        println!("\n  G grade, J engine brake, K automatic speed control, Down arrow brakes (hands it back).");
        println!("  To leave: Escape pauses; quit to the main menu, then Exit as usual.");
    }
    println!("  Transcript: {}\n", log_path.display());
    if let Some(observer) = observer.as_mut() {
        app.run_with_player_input(None, |app, dt| observer.step(app, dt));
        match observer.outcome() {
            Some(observer::ObserverOutcome::Complete(detail)) => println!("AI complete: {detail}."),
            Some(observer::ObserverOutcome::Boundary(detail)) => println!("AI boundary: {detail}."),
            None => println!("AI boundary: the observer ended without a declared outcome."),
        }
    } else {
        app.run(None);
    }
    if sandbox_dir.is_some() {
        sandbox::close_session();
    }
    match settings_before {
        Some(bytes) => {
            let _ = std::fs::write(&settings_path, bytes);
        }
        None => {
            let _ = std::fs::remove_file(&settings_path);
        }
    }
    println!("\nDone. Transcript written to {}", log_path.display());
    0
}

/// The staged drive, as the app's initial-state factory.
fn road_builder(hit: road::Hit, args: &[String]) -> app::InitialState {
    let opts = road_options(args);
    Box::new(move |ctx| {
        let (driving, start_mi) = road::build_driving(ctx, &hit, &opts);
        road::print_setup(ctx, &hit, start_mi, &opts);
        freight_fate::app::share(driving)
    })
}

fn road_options(args: &[String]) -> road::RoadOptions {
    let feature = flag_value(args, "--find").unwrap_or_default();
    let routes = flag_value(args, "--routes").unwrap_or_else(|| {
        if feature == "departure" {
            road::RANDOM_ROUTES.to_string()
        } else {
            "all".to_string()
        }
    });
    let mut opts = road::RoadOptions {
        origin: flag_value(args, "--from"),
        destination: flag_value(args, "--to"),
        facility: flag_value(args, "--facility"),
        routes,
        seed: flag_i64(args, "--seed"),
        sample: flag_usize(args, "--sample").unwrap_or(road::RANDOM_SAMPLE),
        max_miles: flag_f64(args, "--max-miles").unwrap_or(road::RANDOM_MAX_MILES),
        feature,
        at: flag_f64(args, "--at"),
        pick: flag_usize(args, "--pick").unwrap_or(0),
        trip_seed: flag_i64(args, "--trip-seed"),
        scan: has(args, "--scan"),
        min_pct: flag_f64(args, "--min-pct").unwrap_or(3.0),
        min_run: flag_f64(args, "--min-run").unwrap_or(1.0),
        min_drop: flag_f64(args, "--min-drop").unwrap_or(10.0),
        max_advisory: flag_f64(args, "--max-advisory").unwrap_or(45.0),
        lead: flag_f64(args, "--lead"),
        cruise: flag_f64(args, "--cruise").unwrap_or(0.0),
        speed: flag_f64(args, "--speed").unwrap_or(62.0),
        cargo: flag_f64(args, "--cargo").unwrap_or(20.0),
        cargo_type: flag_value(args, "--cargo-type").unwrap_or_else(|| "general".to_string()),
        level: flag_i64(args, "--level"),
        descent: flag_value(args, "--descent"),
        assists: flag_value(args, "--assists"),
        planned_stop_assist: flag_on_off(args, "--planned-stop-assist"),
        predictive_cruise: flag_on_off(args, "--predictive-cruise"),
        lane_keeping: flag_value(args, "--lane-keeping")
            .or_else(|| has(args, "--ai").then(|| "full".to_string())),
        curve_assist: flag_on_off(args, "--curve-assist"),
        transmission: flag_value(args, "--transmission"),
        verbosity: flag_value(args, "--verbosity"),
        weather: flag_value(args, "--weather"),
        hour: flag_f64(args, "--hour"),
        log: flag_value(args, "--log"),
        sandbox: !has(args, "--no-sandbox"),
        unit: None,
    };
    if has(args, "--no-cruise") {
        opts.cruise = 0.0;
    }
    if opts.feature.is_empty() && opts.at.is_none() {
        opts.feature = "downgrade".to_string();
    }
    opts
}

/// Point this session's log at `path` (`FREIGHT_FATE_LOG_FILE`), at INFO.
fn open_log(path: &std::path::Path) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::env::set_var("FREIGHT_FATE_LOG_FILE", path);
    if std::env::var_os("FREIGHT_FATE_LOG").is_none() {
        std::env::set_var("FREIGHT_FATE_LOG", "INFO");
    }
}
