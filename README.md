# Freight Fate

An accessible, audio-first cross-country trucking simulation. Haul freight
between more than 600 American cities, manage fuel, tolls, weather, and
deadlines, and build a driving career entirely by ear.

Freight Fate is designed for blind and low-vision players first: every screen
is fully voiced through your screen reader (NVDA, JAWS, SAPI, VoiceOver,
Speech Dispatcher, and more through the native Prism speech bridge),
and the road speaks to you through a rich procedural soundscape. A simple
visual display mirrors all speech for sighted players and helpers.

## Features

- **Career mode** — accept jobs inside metro freight markets, deadhead to
  specific origin facilities, deliver to specific receivers, earn money and
  experience, level through a 30-rank trucking career, unlock cargo
  endorsements, then buy into a leased-on owner-operator path when your career
  and working capital are ready.
- **Realistic freight markets** — the 623-city route graph acts as metro
  service areas, while each market expands into representative ports, rail and
  intermodal ramps, air cargo areas, parcel hubs, distribution centers, cold
  storage, food processors, farms/elevators, manufacturing plants, steel and
  automotive sites, chemical terminals, construction yards, mines/quarries,
  lumber/paper facilities, cross-docks, and company yards.
- **Real driving** — a tuned Class 8 truck simulation: 450 horsepower,
  ten gears (manual with clutch, or automatic), air-brake pressure,
  parking brakes, engine braking, grades, stalls, brake fade, and
  fuel economy.
- **Business progression** — choose a grounded company-driver carrier with
  assigned equipment plus modest wage and dispatch tradeoffs, or start as a
  higher-risk leased-on owner-operator with owned starter equipment and real
  operating costs. Owner-operators can add specialty trailer programs for
  reefer, flatbed, and bulk freight, then prepare for a limited own-authority
  direct-freight mode with trailer ownership at the end of the career arc.
- **Trucks and upgrades** — owner-operators can earn their way into a heavy
  hauler with more torque and a bigger tank (and worse aerodynamics), and
  outfit any truck with an engine tune, aerodynamic kit, long-range tank, or
  reinforced brakes. Every purchase changes the physics.
- **A living market** — each cargo class has a pay rate that drifts day by
  day, and each metro weights freight by regional specialization. The job
  board tells you when electronics are tight or bulk freight has gone loose;
  chase the tight markets.
- **A living road** — dynamic regional weather that changes grip and safe
  speeds, construction and traffic zones, road hazards that demand quick
  braking, curated rest stops and service plazas with parking certainty,
  carrier-paid toll-road settlement charges, and roadside rescue when you run dry.
- **Real-world weather (optional)** — flip Settings → Speech and weather →
  Weather source to
  "real world" and each city uses its live current conditions from the free
  [National Weather Service](https://www.weather.gov/documentation/services-web-api)
  API. If it is raining in Chicago right now, it is raining in your game. Works
  without an API key and falls back to simulated weather offline. A separate
  calendar setting lets career dates and seasons keep advancing while live
  conditions remain enabled.
- **Route planning** — route options per job with distance, highways, state
  context, grade/terrain, toll events, curated POIs, and weather forecasts.
  Geometry coverage is broad, while generated placeholder POIs are reported as
  data gaps instead of dispatch-ready truck stops. Facilities add local pickup
  and delivery realism without pretending that every suburb or shipper needs a
  separate highway node.
- **Original audio** — sound effects and music are original project assets,
  with sources documented in the audio credits. The Rust audio engine plays
  them through BASS, with the engine note pitch-tracking RPM in real time. If
  BASS is unavailable, the game continues with a silent audio backend rather
  than failing to start.
- **Screen reader native** — menus with first-letter navigation, contextual
  F1 help everywhere, on-demand information keys while driving, a message log
  you can walk back through from any screen, and a choice of terse or normal
  speech verbosity.
- **Discord Rich Presence (optional)** — when Discord is running, your profile
  can show what you are up to: in the main menu, at the terminal, driving a
  route, resting, or delivering, with the broad route and cargo. Only general
  game activity is shared — never your save files or personal details — and it
  is on by default but easily switched off under Discord presence on the
  Online menu, on the main menu. The game starts and runs whether or
  not Discord is open.

## Download and play

1. Open [Freight Fate releases](https://github.com/orinks-games/Freight-Fate/releases).
2. Choose the latest numbered stable release. Stable is recommended for most
   players. If you want to test Career 1.9, choose the newest prerelease build.
   Its dated tag looks like `1.9-tester-20260829`, spoken as "1 point 9 tester,
   August 29, 2026." In Settings, choose `Update channel: developer snapshots`
   to receive later Career 1.9 prerelease builds. A career saved in a tester
   build may not load in an older stable release.
3. Under Assets, Windows players should download the file ending in
   `-windows-portable.zip`. Career 1.9 testers using an Apple Silicon Mac
   should download the file ending in `-macos-arm64.zip`. The Career 1.9 Mac
   build does not support Intel Macs. Career 1.9 testers on Linux have a
   tarball and an AppImage each for PCs (`-linux-x64.tar.gz`,
   `-linux-x86_64.AppImage`) and for ARM64 machines such as the Blazie BT
   Speak and BT Braille or a Raspberry Pi (`-linux-arm64.tar.gz`,
   `-linux-aarch64.AppImage`). Choose the archive for your machine's
   processor. AppImages run directly; tarballs need extracting.
4. Extract the zip file. On Windows, open the extracted `FreightFate` folder
   and run `FreightFate.exe`. On an Apple Silicon Mac, move `FreightFate.app`
   to Applications. If macOS blocks the first launch, open System Settings,
   choose Privacy & Security, and allow Freight Fate to open.

For a complete player-facing guide to careers, dispatch, driving,
route stops, saves, settings, audio, speech, and troubleshooting, see the
[Freight Fate Player Manual](docs/user-manual.md).

Want to help with code, docs, or world data? Start with
[the Freight Fate contribution guide](CONTRIBUTING.md).

## Run from source

Career 1.9 is a native Rust game. Python is not part of the game; it remains
in this repository only for build, packaging, data-generation, and other
maintainer tools under `tools/`.

Install these prerequisites first:

- [Git](https://git-scm.com/downloads)
- [Rust through rustup](https://www.rust-lang.org/tools/install)
- [uv](https://docs.astral.sh/uv/getting-started/installation/)

Then clone Career 1.9 and run the game:

```powershell
git clone https://github.com/orinks-games/Freight-Fate.git
cd Freight-Fate
uv sync --group dev
uv run python tools/fetch_bass.py
cargo run --release -p freight-fate
```

The BASS fetcher downloads and verifies the required audio libraries. A raw
source run needs this command because `cargo run` does not fetch BASS itself.

The first Cargo build takes longer because it compiles the workspace. Later
runs reuse `target/`.

On macOS, the repository supplies the CMake compatibility setting needed to
compile its bundled SDL2. Source builders do not need to add
`CMAKE_POLICY_VERSION_MINIMUM` to a private `.cargo/config.toml`.

## Build a standalone copy

After installing Git, Rust, `uv`, and CMake (Prism is compiled from C++
source), run the release builder for your platform from the repository root.
On Windows, from PowerShell:

```powershell
./build-release.ps1
```

On macOS or Linux, from a terminal:

```bash
./build-release.sh
```

Either script checks the prerequisites, installs the pinned Rust toolchain
and the project's Python tooling, builds the native game, fetches and
verifies BASS and the public music pack, checks the staged game, and writes
the archive for that platform to `dist/`: the Windows portable zip, the
macOS app zip, or the Linux tarball. Players using those archives and
contributors using these standalone build scripts do not need to find or
copy audio files themselves.

Windows also needs the Visual Studio Build Tools with the "Desktop
development with C++" workload and its optional C++ ATL component (Prism's
speech backends include `atlbase.h`); the script checks for both and prints
the `winget` or Visual Studio Installer command that adds what is missing.
Linux needs GCC 13 or newer plus the SDL2 development packages the nightly
Career 1.9 workflow installs; on Ubuntu 22.04 the script picks up
`gcc-13`/`g++-13` itself when the default `gcc` is older. macOS builds SDL2
from source, so do not install Homebrew's `sdl2`.

For iPhone and iPad builds and the touch gestures, see `docs/ios.md`.

### Advanced maintainer overrides

The release script passes extra options to `tools/build_release.py`. Maintainers
can use `--tag` or `--cargo-target-dir` when preparing a specific release.
`FREIGHT_FATE_MUSIC_URL`, `FREIGHT_FATE_MUSIC_SHA256`, and
`FREIGHT_FATE_BASS_PATH` remain emergency overrides for pinned release inputs.
Normal players and ordinary source builders do not need these overrides.

If the build succeeds but the archive seems to vanish on Windows, check
your antivirus: freshly built unsigned executables are sometimes
quarantined on sight. Add an exclusion for the `dist/` folder or restore
the file from quarantine.

## Controls

Freight Fate plays with the keyboard or a game controller, and both stay active
at all times. Spoken prompts name whichever you last used, so "press X to take
it" on the keyboard becomes "press D-pad down to take it" on a controller.

### Keyboard

#### Menus

| Key | Action |
| --- | --- |
| Up / Down | Navigate |
| Enter | Select |
| Escape | Back |
| Home / End | First / last item |
| Any letter | Jump to the next item starting with it |
| F1 | Contextual help |

#### Messages, on every screen

The message log works the same way everywhere in the game, driving included.
It keeps the last 200 things the game said, in one history shared by menu
speech and driving events.

| Key | Action |
| --- | --- |
| Comma | Repeat the last spoken line; press again quickly to step one line older each time |
| Period | Move toward newer messages |
| Ctrl+Comma | Jump to the oldest message |
| Ctrl+Period | Jump to the newest message |
| Left / Right bracket | Switch between all messages, general messages, and driving events |
| Ctrl+C | Copy the message you are on |

#### Driving

Moving the truck:

| Key | Action |
| --- | --- |
| Up arrow (hold) | Throttle |
| Down arrow (hold) | Brake |
| B (hold) | Emergency brake — the hardest possible stop |
| Left / Right arrow | Steer. With lane keeping on full, a tap changes lanes instead |
| E | Start / stop engine |
| P | Release / set parking brake. Setting it at speed grinds flat spots into the tread and costs real tire wear — it is the emergency backup, not a brake |
| H | Horn |

Gears and the engine brake:

| Key | Action |
| --- | --- |
| Left Shift (hold) | Clutch (manual mode) |
| W | Shift up a gear (manual mode) |
| Q | Shift down a gear (manual mode) |
| N | Neutral (manual mode) |
| Backspace | Reverse (manual mode) |
| Alt+T | Switch between automatic and manual shifting, mid-drive |
| J | Engine brake on / off |
| 1, 2, 3 | Engine brake stage (one, two, three) |
| Alt+J | Whether J arms the automatic engine brake or leaves it to you |

Speed control:

| Key | Action |
| --- | --- |
| K | Automatic speed control on / off. Parked with the brake set, it latches a high idle instead |
| Shift+K | Resume the last speed after braking cancelled it |
| Plus / minus (keypad too) | Raise / lower the remembered open-road target by 5 |

Route actions:

| Key | Action |
| --- | --- |
| X | Signal for the next announced exit, or cancel that signal. Also signals a pull-over when a trooper lights you up |
| T | Plan the next sleep-capable stop while rolling; at a stop, open its menu; fully stopped away from stops, open emergency shoulder sleep |
| Enter | Go inside on a city-service arrival, or open a facility arrival once fully stopped |
| Tab | Driving status menu |
| Escape | Pause menu |

Asking the truck and the road questions:

| Key | Action |
| --- | --- |
| Space | Speed, gear, RPM, the speed-control mode in use, and air pressure |
| S | Posted speed limit. On a ramp with a signal, the light and the distance to the stop bar |
| D | The one safe speed for right here — weather, an armed exit, and the bend ahead already folded in |
| G | The grade under the wheels, whether the truck is holding it, and the next grade ahead |
| R | Route progress, distance left, and where you are |
| Shift+R | Next listed highway exit |
| U | What is coming up: imposed limits, stops, exits, and bends |
| C | Clock, deadline, date and season, and the hours limit that comes first |
| Alt+A | Time at the wheel so far this shift |
| Alt+S | When your 30 minute break is due |
| Alt+D | What ends this shift, and where you can stop before it |
| F | Fuel and range |
| V | Weather and forecast |
| L | Lane position, and whether the lane beside you is open or blocked |
| I | Lane locator on / off — a soft tock once a beat, panned to where you sit inside your lane. Available on lane keeping partial or off |
| A | Repeat the last driving announcement |
| F1 | List all controls |
| Left or Right Ctrl | Stop the driving event voice mid-sentence |

The radio:

| Key | Action |
| --- | --- |
| M | Radio on / off |
| Page Down / Page Up | Tune to the next / previous receivable station (semicolon and apostrophe also work) |
| Ctrl + a tuning key | Jump a whole dial category |
| O | Save or unsave the current station as a favorite |
| Y | Station, volume, and streamer-safe status |

### Controller

Plug in an Xbox, PlayStation, or other compatible controller and the game picks
up the first one automatically. It detects a controller connected or unplugged
mid-game — unplugging pauses the drive — and you can switch back to the keyboard
at any time. Button names below use the Xbox layout; the equivalents map
automatically on other pads. Turn controller support off under Settings →
Gameplay → Controller if you prefer keyboard only.

#### Menus

| Button | Action |
| --- | --- |
| D-pad Up / Down | Navigate |
| D-pad Left / Right | Adjust the selected option (hold to repeat) |
| A | Select (like Enter) |
| B | Back (like Escape) |
| Back / Select | Contextual help (like F1) |

#### Driving

| Button | Action |
| --- | --- |
| Left stick | Steering |
| Right trigger | Throttle |
| Left trigger | Brake (press fully for the hardest stop) |
| Left bumper (LB) | Clutch (manual mode) |
| A | Shift up a gear (manual mode) |
| X | Shift down a gear (manual mode) |
| Y | Automatic speed control on / off |
| B | Speak speed, gear, RPM, speed-control mode, and air pressure |
| D-pad Up | Route progress |
| D-pad Down | Take exit / signal a pull-over |
| D-pad Left | Weather and forecast |
| D-pad Right | Clock, deadline, and the full hours of service report |
| Left stick click (L3) | Horn |
| Right stick click (R3) | Engine brake toggle |
| Start | Pause / resume |
| Back / Select | Controller help |

Hold the right bumper (RB) as a modifier for a second layer of driving bindings:

| Button | Action |
| --- | --- |
| RB + A | Start / stop engine |
| RB + B | Fuel and range |
| RB + Y | Release / set parking brake |
| RB + D-pad Up | Next listed highway exit |
| RB + D-pad Down | Open a route stop, or emergency shoulder sleep away from stops, when fully stopped. |
| RB + D-pad Left / Right | Lower / raise the open-road cruise target |
| RB + Start | Driving status menu |

The left and right triggers are analog: hold them wherever you like for partial
throttle or braking, rather than the ramped hold the arrow keys use. There is no
controller emergency brake — press the left trigger all the way for the hardest
stop.

### Air-brake model

Freight Fate models air pressure without asking players to run a full CDL
inspection before every dispatch. A cold trip starts with low pressure and
the parking brake set. Start the engine, wait for the compressor to build
air to 100 psi, then press `P` to release the parking brake. Repeated brake
applications draw from separate truck and trailer air tanks, low air warns
around 60 psi, and spring brakes apply around 40 psi. Normal driving should
feel familiar; the extra detail mostly gives clearer warnings if you pump the
brakes hard, ignore low air, or drive damaged equipment. Press `Tab` while
driving to review primary, secondary, and trailer air, compressor state,
parking or spring brake state, and brake heat.

The thresholds are grounded in official CDL and air-brake references:
[FMCSA](https://www.fmcsa.dot.gov/sites/fmcsa.dot.gov/files/docs/brake_safety_systems_02-14.pdf)
describes typical 110-130 psi compressor cut-out, cut-in about 20 psi lower,
and an 85-to-100 psi build-up check; the
[California DMV](https://www.dmv.ca.gov/portal/handbook/commercial-driver-handbook/section-5-air-brakes/)
places low-air warnings between 55 and 75 psi; the
[Georgia DDS](https://dds.georgia.gov/section-52-53) describes spring-brake
and governor checks; and
[SGI](https://sgi.sk.ca/air-brake/-/knowledge_base/air-brake/air-governor)
lists 120-145 psi cut-out and 100 psi minimum cut-in. Build-up time is
compressed for playability so startup is a short, understandable pause
rather than minutes of waiting.

## Development

Gameplay code and gameplay tests for Career 1.9 live in the Cargo workspace.
Use the Rust checks for gameplay changes:

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test -p ff-core
cargo test -p freight-fate
```

Python remains supported for repository tooling. When changing packaging,
world-data generation, release notes, or another Python tool, set up its
environment and run focused Python checks for that tool:

```bash
uv sync --group dev
uv run pytest tests/test_build_release_rust.py
uv run ruff check tests tools
```

Do not add Python gameplay code or Python gameplay tests. The Python game was
removed after its last release, `v1.8.8.1`; it survives only in git history.

The pre-push hook runs the release-note gate before publishing commits.
User-facing changes need a player-facing `CHANGELOG.md` entry unless the
entire change set is non-player-facing and uses `changelog: none` or
`[skip changelog]`.

### World data

The route tools edit `data/world_source/`, but the game loads the indexed
`data/world_data/` tree. After changing world data,
regenerate the index so the two stay in sync:

```bash
uv run python tools/index_world.py          # rewrite world_data/ from world_source/
uv run python tools/index_world.py --check  # verify in sync (CI + pre-commit do this)
```

A pre-commit hook and a test both fail if `world_data/` drifts from the source,
so commit the regenerated `world_data/` files alongside your source edits.

Both trees group legs by their starting state in files such as `legs/TX.json`
and `legs/CA.json`. Tools do not edit these files directly:
`tools/world_source.py` gives them `load_world()`, which returns the whole world
as one dict, and `save_world(data)`, which writes only the shards that actually
changed. A one-leg edit therefore changes only its state's file.

### Playtesting

Freight Fate is audio-first, so the way to review a playthrough is the transcript
of what the game says. The Rust playtest harness drives the real game states
headlessly and is used by the Cargo integration tests. Run a focused transcript
test by name while iterating, then the whole game crate before landing:

```bash
cargo test -p freight-fate --test it transcript_shane_deadline -- --nocapture
cargo test -p freight-fate
```

A manual test scenario starts from the game binary itself:
`cargo run --release -p freight-fate --bin freightfate -- --playtest-road --find <feature>`
drops you at a road feature, and `-- --playtest-sandbox --launch` opens the full
game in a throwaway save directory. `tools/playtest_watch.py` follows either
session's log.

### Changelog and snapshots

Player-facing changes should add a short bullet under `## Unreleased` in
`CHANGELOG.md`. Developer snapshots use those curated entries for release
notes; they do not turn git commit subjects into player-facing copy.

Scheduled snapshots build only when new curated Unreleased entries are present,
or when a commit message includes `nightly: build` or `[nightly build]` for an
intentional snapshot refresh. Use `changelog: none` or `[skip changelog]` only
when every commit in the change set is non-user-facing.

## License

Freight Fate is licensed under the
[PolyForm Noncommercial License 1.0.0](LICENSE): the source is public and free
to use, modify, and share for any noncommercial purpose, but only the copyright
holder may sell it or put it to commercial use. This is a source-available
license, not an OSI-approved open-source license. Bundled audio credits and
provenance are tracked in
[the Freight Fate audio credits](assets/sounds/CREDITS.md).

**BASS license caveat:** audio playback uses the
[BASS](https://www.un4seen.com/) library through Freight Fate's Rust bindings.
BASS is proprietary and **free for non-commercial use only**. If
Freight Fate is ever sold commercially (Steam, itch.io paid downloads, and
so on), a paid license must be purchased from
[un4seen developments](https://www.un4seen.com/bass.html#license) first.
The same terms cover the bundled BASSHLS addon
(`basshls.dll`), which lets the radio play HLS streams; its license text ships
alongside it. When BASS is unavailable, the Rust game runs without sound.
