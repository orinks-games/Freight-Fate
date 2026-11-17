# Freight Fate

Freight Fate is an audio trucking game for blind and visually impaired
players. You haul freight between more than 600 American cities, plan for
fuel, tolls, weather, rest and deadlines, and build a driving career.

Speech works with NVDA, JAWS, SAPI, VoiceOver, Speech Dispatcher and other
screen readers and voices, and with a braille display through NVDA or JAWS.
The game window draws the road, the dashboard, the menus and captions of
everything the game says, for sighted players and helpers.

## Features

- **Career.** Take loads from shippers in each metro, drive
  empty to the pickup, and deliver to the receiver for money and experience.
  Climb 30 career levels, earn certificates and endorsements that open new
  cargo, and buy in as a leased-on owner-operator once you have the level
  and the money.
- **Freight markets.** Each city is a freight market with its own ports,
  rail and intermodal ramps, air cargo areas, parcel hubs, distribution
  centers, cold storage, food processors, farms and grain elevators, plants,
  steel and auto sites, chemical terminals, construction yards, mines and
  quarries, lumber and paper mills, cross-docks and company yards.
- **Driving.** A Class 8 truck with 450 horsepower and ten gears, manual
  with a clutch or automatic. Air brakes, the parking brake, the engine
  brake, grades, stalls, brake fade and fuel economy are all simulated.
- **Company driver or owner-operator.** A company carrier supplies the truck
  and pays its costs, and each carrier has its own wages, freight and
  deadlines. A leased-on owner-operator owns the truck and pays its bills.
  Owner-operators can add reefer, flatbed and bulk trailer programs, and at
  the end of the career run their own authority with their own trailers.
- **Trucks and upgrades.** Owner-operators can buy a heavy hauler with more
  torque and a bigger tank but worse aerodynamics, and fit any truck with an
  engine tune, an aerodynamic kit, a long-range tank or reinforced brakes.
  Each upgrade changes how the truck drives.
- **Freight pay.** Each cargo class has a pay rate that changes day by day,
  and each metro favors the freight its region produces. The dispatch board
  says when a cargo's market is tight or loose.
- **The road.** Regional weather changes grip and safe speeds. There are
  work zones, traffic, road hazards that need quick braking, real rest areas
  and service plazas with truck parking, toll roads, and roadside rescue
  when you run out of fuel.
- **Real-world weather, optional.** Set Weather source to real world under
  Settings, Gameplay, World and traffic, and each city uses its current
  conditions from the free
  [National Weather Service](https://www.weather.gov/documentation/services-web-api)
  API, including active warnings along your route. If it is raining in
  Chicago right now, it is raining in your game. No API key is needed, and
  the game falls back to simulated weather offline. Turn off Live weather
  controls calendar to keep the career's own dates and seasons with live
  conditions.
- **Route planning.** Owner-operators compare route options for each job:
  distance, highways, states, grades, tolls, stops and weather forecasts.
- **Sound.** The engine note follows the RPM. Every sound and music track is
  listed with its source in the audio credits. If no sound device can be
  opened, the game starts without sound and says so.
- **Menus and readouts.** Menus have first-letter navigation and F1 help on
  every item. While driving, keys read out speed, the route, the hours left
  and more. A message log on every screen lets you step back through what
  was said, and Driving speech sets how much the road says on its own:
  Standard, Quiet or Urgent only.
- **Discord presence, optional.** When Discord is running, your profile can
  show what you are doing: in the menus, at the terminal, driving, resting
  or delivering, with the broad route and cargo. It never shares save files
  or personal details. It is on by default; turn it off with Discord
  presence in the Online menu on the main menu. The game runs the same with
  or without Discord.

## Download and play

1. Open [Freight Fate releases](https://github.com/orinks-games/Freight-Fate/releases).
2. Choose the latest numbered stable release. Stable is recommended for most
   players. To test what comes next, choose the newest prerelease instead.
   Its dated tag looks like `1.9-tester-20260829`, spoken as "1 point 9 tester,
   August 29, 2026." To get later prereleases as updates, set Update channel
   to beta under Settings, Updates. A career saved in a tester build may not
   load in an older stable release.
3. Under Assets, Windows players download the file ending in
   `-windows-portable.zip`. On an Apple Silicon Mac, download the file ending
   in `-macos-arm64.zip`. Intel Macs are not supported. Linux has a tarball
   and an AppImage each for PCs (`-linux-x64.tar.gz`,
   `-linux-x86_64.AppImage`) and for ARM64 machines such as the Blazie BT
   Speak and BT Braille or a Raspberry Pi (`-linux-arm64.tar.gz`,
   `-linux-aarch64.AppImage`). Choose the one for your machine's processor.
   An AppImage runs directly, and a tarball needs extracting. Both need the
   ALSA sound library (`libasound2` on Debian and Ubuntu, `alsa-lib` on
   Fedora and Arch), which nearly every Linux desktop already has.
4. Extract the zip file. On Windows, open the extracted `FreightFate` folder
   and run `FreightFate.exe`. On an Apple Silicon Mac, move `FreightFate.app`
   to Applications. If macOS blocks the first launch, open System Settings,
   choose Privacy & Security, and allow Freight Fate to open.

The [Freight Fate Player Manual](docs/user-manual.md) covers careers,
dispatch, driving, route stops, saves, settings, audio, speech, and
troubleshooting.

Want to help with code, docs, or world data? Start with
[the Freight Fate contribution guide](CONTRIBUTING.md).

## Run from source

Freight Fate is written in Rust, and so are its build, packaging and data
tools (`cargo tools help` lists them).

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

Then clone the repository and run the game:

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
macOS app zip, or the Linux tarball. Nobody needs to find or copy audio
files by hand.

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

If the build succeeds but the archive seems to vanish on Windows, check
your antivirus: freshly built unsigned executables are sometimes
quarantined on sight. Add an exclusion for the `dist/` folder or restore
the file from quarantine.

## Controls

Freight Fate plays with the keyboard or a game controller, and both work at
the same time. Spoken prompts name whichever you last used, so "press X to take
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

The message log works on every screen, driving included. It keeps the last
200 things the game said, menus and driving events together.

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
| B (hold) | Emergency brake, the hardest possible stop |
| Left / Right arrow | Steer. With lane keeping on full, a tap changes lanes instead |
| E | Start / stop engine |
| P | Release / set parking brake. Setting it at speed grinds flat spots into the tires. It is not a brake pedal |
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
| Shift+K | Resume the last speed after braking canceled it |
| Plus / minus (keypad too) | Raise / lower the cruise target to the next multiple of 5. Ctrl changes it by 1 |

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
| D | One safe speed for right here, with the weather, a signaled exit and the next bend already counted in |
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
| I | Lane locator on / off: a soft tock once a beat, panned to where you sit inside your lane. Needs lane keeping on partial or off |
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
| Shift+Y | What the radio is playing |

### Controller

Plug in an Xbox, PlayStation, or other compatible controller and the game uses
the first one it finds. You can plug one in or unplug it mid-game, and
unplugging pauses the drive. Button names below use the Xbox layout, and other
pads map to the same positions. To play with the keyboard only, turn off
Controller under Settings, Gameplay, Controls.

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
| RB + D-pad Down | Plan a stop while rolling. Fully stopped, open a route stop, or emergency shoulder sleep away from stops |
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

The left and right triggers are analog: hold them part way for partial throttle
or braking, instead of the ramped hold the arrow keys use. There is no separate
controller emergency brake. Press the left trigger all the way for the hardest
stop.

### Air-brake model

Freight Fate models air pressure without asking players to run a full CDL
inspection before every dispatch. A cold trip starts with low pressure and
the parking brake set. Start the engine, wait for the compressor to build
air to 100 psi, then press `P` to release the parking brake. Repeated brake
applications draw from separate truck and trailer air tanks, low air warns
around 60 psi, and spring brakes apply around 40 psi. In normal driving you
rarely notice the air. It matters when you pump the brakes hard, ignore low
air, or drive damaged equipment. Press `Tab` while
driving to review primary, secondary, and trailer air, compressor state,
parking or spring brake state, and brake heat.

The thresholds come from official CDL and air-brake references:
[FMCSA](https://www.fmcsa.dot.gov/sites/fmcsa.dot.gov/files/docs/brake_safety_systems_02-14.pdf)
describes typical 110-130 psi compressor cut-out, cut-in about 20 psi lower,
and an 85-to-100 psi build-up check; the
[California DMV](https://www.dmv.ca.gov/portal/handbook/commercial-driver-handbook/section-5-air-brakes/)
places low-air warnings between 55 and 75 psi; the
[Georgia DDS](https://dds.georgia.gov/section-52-53) describes spring-brake
and governor checks; and
[SGI](https://sgi.sk.ca/air-brake/-/knowledge_base/air-brake/air-governor)
lists 120-145 psi cut-out and 100 psi minimum cut-in. Build-up time is
shortened, so startup takes a short wait instead of several minutes.

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

Review a playthrough by reading the transcript of what the game says. The
playtest harness drives the real game states without a window, and the Cargo
integration tests use it. Run a focused transcript
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

A tester snapshot is built from each push to `dev` (and once a night as a
fallback), but only when the commits since the last snapshot add curated
Unreleased entries, or when a commit message includes `nightly: build` or
`[nightly build]` for an intentional snapshot refresh. Use `changelog: none` or
`[skip changelog]` only when every commit in the change set is non-user-facing.

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
