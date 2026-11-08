# Freight Fate

Freight Fate is an audio trucking game for blind and visually impaired players: a cross-country
trucking simulation where you haul freight
between more than 600 American cities, manage fuel, tolls, weather, and
deadlines, and build a driving career.

Supports NVDA, JAWS, SAPI, VoiceOver, Speech Dispatcher, and more through the
native Prism speech bridge. Road events use procedural sounds. A simple visual
display mirrors all speech for sighted players and helpers.

## Features

- **Career mode**: accept jobs in metro freight markets, drive to pickup
  facilities, and deliver to receivers. Earn money and experience through 30
  career ranks, unlock cargo endorsements, and buy into a leased-on
  owner-operator business when you meet the career and cash requirements.
- **Freight markets**: the 623 cities represent metro service areas. Jobs take
  you to ports, rail and intermodal ramps, air cargo areas, parcel hubs,
  distribution centers, cold storage, food processors, farms and grain
  elevators, factories, steel and automotive sites, chemical terminals,
  construction yards, mines and quarries, lumber and paper facilities,
  cross-docks, and company yards.
- **Driving**: a Class 8 truck with 450 horsepower and ten gears. Choose manual
  shifting with a clutch or automatic transmission. Manage air-brake pressure,
  parking brakes, engine braking, grades, stalls, brake fade, and fuel economy.
- **Business progression**: a company driver gets assigned equipment, wages,
  and a carrier's dispatch terms. A leased-on owner-operator starts with a
  truck and pays its operating costs, taking on more financial risk. Owners
  can add reefer, flatbed, and bulk
  trailer programs, then work toward own authority, trailer ownership, and
  direct freight later in the career.
- **Trucks and upgrades**: owner-operators can buy a heavy hauler with more
  torque and a bigger tank, at the cost of worse aerodynamics. Engine tunes,
  aerodynamic kits, long-range tanks, and reinforced brakes change how a
  truck drives.
- **Freight prices**: cargo rates change daily, and each metro has its own
  freight mix. The job board reports tight and loose markets so you can choose
  where to look for work.
- **Road conditions**: regional weather changes grip and safe speeds.
  Construction, traffic, and hazards can slow a trip. Plan rest stops and
  service plazas, account for carrier-paid toll charges, and call roadside
  rescue if you run out of fuel. Stop information includes parking availability.
- **Real-world weather (optional)**: set Weather source under Settings,
  Gameplay, World and traffic to "real world" to use current conditions from
  the free [National Weather Service](https://www.weather.gov/documentation/services-web-api)
  API. It needs no API key and falls back to simulated weather offline. You can
  keep the career calendar and seasons advancing while using live conditions.
- **Route planning**: compare distance, highways, states, grades, terrain,
  tolls, points of interest, and forecasts. Some mapped places lack verified
  truck-stop information; the game marks those gaps. Pickup and delivery
  facilities have their own approaches within each metro area.
- **Sound and music**: effects and music are project assets, with sources
  listed in the audio credits. Hearken plays them, and the engine note follows
  RPM. If no sound device opens, the game continues silently and reports the
  problem. Learn game sounds lets you listen to cues and learn their meanings.
- **Menus and speech**: first-letter navigation, contextual F1 help, driving
  readout keys, and a message log available from any screen. Choose Standard,
  Quiet, or Urgent only driving speech; readout keys answer in every mode.
  Score display under Settings, Speech reads reputation as a description and
  number, percentage only, or description only. A trust line can read
  "Dispatch trust: full. Reputation 85 out of 100."
- **Discord Rich Presence (optional)**: while Discord is running, your profile
  can show general game activity, route, and cargo. It never shares save files
  or personal details. It is on by default; turn it off under Discord presence
  in the main menu's Online screen. The game runs without Discord.

## Download and play

1. Open [Freight Fate releases](https://github.com/orinks-games/Freight-Fate/releases).
2. Choose the latest numbered stable release. Stable is recommended for most
   players. If you want to test Career 1.9, choose the newest prerelease build.
   Under Settings, Updates, choose `Update channel: beta` for testing
   releases or `Update channel: alpha` for earlier builds. A career saved in
   a tester build may not load in an older stable release.
3. Under Assets, Windows players should download the file ending in
   `-windows-portable.zip`. Career 1.9 testers using an Apple Silicon Mac
   should download the file ending in `-macos-arm64.zip`. The Career 1.9 Mac
   build does not support Intel Macs. Career 1.9 testers on Linux have a
   tarball and an AppImage each for PCs (`-linux-x64.tar.gz`,
   `-linux-x86_64.AppImage`) and for ARM64 machines such as the Blazie BT
   Speak and BT Braille or a Raspberry Pi (`-linux-arm64.tar.gz`,
   `-linux-aarch64.AppImage`). Choose the archive for your machine's
   processor. AppImages run directly; tarballs need extracting. Both need
   the ALSA sound library (`libasound2` on Debian and Ubuntu, `alsa-lib` on
   Fedora and Arch), which nearly every Linux desktop already has.
4. Extract the zip file. On Windows, open the extracted `FreightFate` folder
   and run `FreightFate.exe`. On an Apple Silicon Mac, move `FreightFate.app`
   to Applications. If macOS blocks the first launch, open System Settings,
   choose Privacy & Security, and allow Freight Fate to open.

For a guide to careers, dispatch, driving,
route stops, saves, settings, audio, speech, and troubleshooting, see the
[Freight Fate Player Manual](docs/user-manual.md).

Want to help with code, docs, or world data? Start with
[the Freight Fate contribution guide](CONTRIBUTING.md).

## Run from source

Career 1.9 is a native Rust game, and its build, packaging and data tools
are Rust too (`cargo tools help` lists them).

Install these prerequisites first:

- [Git](https://git-scm.com/downloads)
- [Rust through rustup](https://www.rust-lang.org/tools/install)
- [CMake](https://cmake.org/download/) 3.24 or newer: the Prism speech
  bridge, SDL2 and the Opus decoder are compiled from source.
- On Windows, the Visual Studio Build Tools (see the standalone build
  below). On macOS, the Xcode Command Line Tools (`xcode-select --install`).
  On Linux, GCC 13 or newer, ALSA's development files (`libasound2-dev` on
  Debian and Ubuntu, `alsa-lib-devel` on Fedora) for sound, and the SDL2
  development packages the nightly Career 1.9 workflow installs.

Hearken's [Getting started](https://github.com/orinks-games/hearken#getting-started)
gives the install commands for each of these on macOS, Windows and Linux.
It also installs the `hearken` command, whose `hearken doctor` checks that
nothing is missing, and `hearken play orinks-games/Freight-Fate` clones,
builds and starts the game in one step.

Then clone Career 1.9 and run the game:

```powershell
git clone https://github.com/orinks-games/Freight-Fate.git
cd Freight-Fate
cargo run --release -p freight-fate
```

Cargo fetches everything the game links, including Hearken, its engine for
speech, sound and the radio, which comes from its own git repository; there
is no native audio library to download first.

The first Cargo build takes longer because it compiles the workspace. Later
runs reuse `target/`.

On macOS, the repository supplies the CMake compatibility setting needed to
compile its bundled SDL2. Source builders do not need to add
`CMAKE_POLICY_VERSION_MINIMUM` to a private `.cargo/config.toml`.

## Build a standalone copy

After installing Git, Rust and CMake (Prism is compiled from C++
source), run the release builder for your platform from the repository root.
On Windows, from PowerShell:

```powershell
./build-release.ps1
```

On macOS or Linux, from a terminal:

```bash
./build-release.sh
```

Either script checks the prerequisites, installs the pinned Rust toolchain,
builds the native game, fetches and
verifies the public music pack, checks the staged game, and writes
the archive for that platform to `dist/`: the Windows portable zip, the
macOS app zip, or the Linux tarball. Players using those archives and
contributors using these standalone build scripts do not need to find or
copy audio files themselves.

Windows also needs the Visual Studio Build Tools with the "Desktop
development with C++" workload and its optional C++ ATL component (Prism's
speech backends include `atlbase.h`); the script checks for both and prints
the `winget` or Visual Studio Installer command that adds what is missing.
Linux needs GCC 13 or newer, ALSA's development files, plus the SDL2
development packages the nightly Career 1.9 workflow installs; on Ubuntu 22.04 the script picks up
`gcc-13`/`g++-13` itself when the default `gcc` is older. macOS builds SDL2
from source, so do not install Homebrew's `sdl2`.

For iPhone and iPad builds and the touch gestures, see `docs/ios.md`.

### Advanced maintainer overrides

The release script passes extra options to `cargo tools build-release`. Maintainers
can use `--tag` or `--cargo-target-dir` when preparing a specific release.
`FREIGHT_FATE_MUSIC_URL` and `FREIGHT_FATE_MUSIC_SHA256` remain emergency
overrides for pinned release inputs.
Normal players and ordinary source builders do not need these overrides.

If the build succeeds but the archive seems to vanish on Windows, check
your antivirus: freshly built unsigned executables are sometimes
quarantined on sight. Add an exclusion for the `dist/` folder or restore
the file from quarantine.

## Controls

Freight Fate plays with the keyboard or a game controller. The keyboard
stays active when you use a controller. Spoken prompts name whichever you
last used, so "press X to take it" on the keyboard becomes "press D-pad down
to take it" on a controller.

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
| Ctrl+Shift+C | Copy the last thing the game said, on any screen |

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
| Either Shift (hold) | Clutch (manual mode) |
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
| Shift+K | Resume the last speed after braking canceled it |
| Plus / minus (keypad too) | Move the cruise target to the next multiple of five; hold Control to change it by one |

Route actions:

| Key | Action |
| --- | --- |
| X | Signal for the next announced exit, or cancel that signal. Also signals a pull-over when a trooper lights you up |
| T | Plan a stop for fuel, a break, sleep, or food while rolling; at a stop, open its menu; fully stopped away from stops, open emergency shoulder sleep |
| Enter | Go inside on a city-service arrival, or open a facility arrival once fully stopped |
| Tab | Driving status menu |
| Escape | Pause menu |

Driving readouts:

| Key | Action |
| --- | --- |
| Space | Speed, gear, RPM, the speed-control mode in use, and air pressure |
| S | Posted speed limit. On a ramp with a signal, the light and the distance to the stop bar |
| D | Safe speed for current weather, your signaled exit, and the bend ahead |
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
| I | Lane locator on / off. Available with lane keeping partial or off; hear the cue in Learn game sounds |
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
automatically on other pads. Turn controller support off under Settings,
Gameplay, Controls, Controller if you prefer keyboard only.

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
| RB + D-pad Up | Route progress |
| RB + D-pad Down | Plan a stop while moving; open stop services or emergency shoulder sleep when fully stopped |
| RB + D-pad Left / Right | Lower / raise the open-road cruise target |
| RB + Start | Driving status menu |

#### Radio

| Button | Action |
| --- | --- |
| Right stick left / right | Tune to the previous / next station |
| Right stick down / up | Lower / raise radio volume (hold to repeat) |
| RB + right stick up / down | Radio on / off; save or unsave the current station as a favorite |
| RB + right stick left / right | Previous / next radio category |
| RB + left stick click (L3) | Radio status |
| RB + Back / Select | What the radio is playing |

The left and right triggers are analog: hold them wherever you like for partial
throttle or braking, rather than the ramped hold the arrow keys use. There is no
controller emergency brake — press the left trigger all the way for the hardest
stop.

### Air-brake model

A cold trip starts with low air pressure and
the parking brake set. Start the engine, wait for the compressor to build
air to 100 psi, then press `P` to release the parking brake. Repeated brake
applications draw from separate truck and trailer air tanks, low air warns
around 60 psi, and spring brakes apply around 40 psi. Warnings tell you when
air is low or brakes are hot. Press `Tab` while driving to review primary, secondary, and trailer air, compressor state,
parking or spring brake state, and brake heat.

The thresholds follow these CDL and air-brake references:
[FMCSA](https://www.fmcsa.dot.gov/sites/fmcsa.dot.gov/files/docs/brake_safety_systems_02-14.pdf)
describes typical 110-130 psi compressor cut-out, cut-in about 20 psi lower,
and an 85-to-100 psi build-up check; the
[California DMV](https://www.dmv.ca.gov/portal/handbook/commercial-driver-handbook/section-5-air-brakes/)
places low-air warnings between 55 and 75 psi; the
[Georgia DDS](https://dds.georgia.gov/section-52-53) describes spring-brake
and governor checks; and
[SGI](https://sgi.sk.ca/air-brake/-/knowledge_base/air-brake/air-governor)
lists 120-145 psi cut-out and 100 psi minimum cut-in. Build-up time is
shortened so you can build pressure without waiting several minutes.

## Development

Gameplay code and gameplay tests for Career 1.9 live in the Cargo workspace.
Use the Rust checks for gameplay changes:

```bash
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test -p ff-core
cargo test -p freight-fate
```

When changing packaging, release notes, translations or another tool, run
its tests too:

```bash
cargo test -p ff-tools
```

The Python game was removed after its last release, `v1.8.8.1`, and the
Python tooling after it; both survive only in git history.

Install the git hooks once per clone with
`git config core.hooksPath .githooks`. The pre-push hook runs the release-note gate before publishing commits.
User-facing changes need a player-facing `CHANGELOG.md` entry unless the
entire change set is non-player-facing and uses `changelog: none` or
`[skip changelog]`.

### World data

`data/world_source/` is the tree you edit, but the game loads the indexed
`data/world_data/` tree. After changing world data,
regenerate the index so the two stay in sync:

```bash
cargo tools index-world          # rewrite world_data/ from world_source/
cargo tools index-world --check  # verify in sync (CI and the commit hook do this)
```

The commit hook and CI both fail if `world_data/` drifts from the source,
so commit the regenerated `world_data/` files alongside your source edits.

Both trees group legs by their starting state in files such as `legs/TX.json`
and `legs/CA.json`, so a one-leg edit changes only its state's file.

### Playtesting

Review a playthrough through the transcript of what the game says. The Rust
playtest harness drives the real game states
headlessly and is used by the Cargo integration tests. Run a focused transcript
test by name while iterating, then the whole game crate before landing:

```bash
cargo test -p freight-fate --test it transcript_shane_deadline -- --nocapture
cargo test -p freight-fate
```

A manual test scenario starts from the game binary itself:
`cargo run --release -p freight-fate --bin freightfate -- --playtest-road --find <feature>`
drops you at a road feature, and `-- --playtest-sandbox --launch` opens the full
game in a throwaway save directory. `cargo tools playtest-watch` follows either
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
[the Freight Fate audio credits](assets/sounds/CREDITS.md). The window's text
is set in [Overpass](https://github.com/RedHatOfficial/Overpass), built into
the game under the [SIL Open Font License 1.1](assets/fonts/OFL.txt).

**Audio:** playback, decoding and the radio use Hearken, Freight Fate's own
Rust audio engine (MIT or Apache-2.0), built on Kira and Symphonia. Earlier
releases used the proprietary BASS library; nothing of it ships any more.
