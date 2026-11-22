# Freight Fate on iPhone and iPad

The iOS game is the desktop game. The same Rust states and menus run under
SDL2's UIKit backend, and speech goes through Prism, which talks to VoiceOver
when it is running and to the system voice (AVSpeech) when it is not. The
screen is one full-screen touch area. Each screen has a touch mode (a menu,
a name field, the wheel, or Practice gestures) that decides what a gesture
does there: menus move the way VoiceOver does and their gestures press the
keys the desktop game reads, at the wheel they run driving commands directly, and game controllers
work exactly as they do on the desktop. The wheel is any drive, a Driving
School lesson included.

## Gestures

The core rule: **a flick is one step; a swipe and hold is continuous.** A
stroke that travels 30 points and lifts within 150 ms of getting there is a
flick. A finger still travelling briskly when the 150 ms ends is still a flick, so a long fast flick is not a hold. One that stays down is a hold in its direction until you lift. Every
gesture works anywhere on the screen; nothing depends on where you touch.
The game asks iOS to defer its edge gestures, so a stroke from an edge reaches
the game first.

With VoiceOver on, the game screen is a direct-touch area (silent on touch on
iOS 17 and later): once VoiceOver focus lands on it (it does at launch), your
gestures go straight to the game. VoiceOver's own standard actions are also
answered.

In menus, and the readers and prompts outside the drive:

| Gesture | Does | Key |
|---|---|---|
| Flick right / left | Next / previous row or line | Down / Up |
| Flick up / down | Raise / lower the value on the row, or the next / previous page in How to play | Right / Left |
| Swipe and hold, or the deep swipe | The flick in that direction, once | as the flick |
| Tap | Read the focused row or line again | none |
| Double tap, or VoiceOver double tap | Choose | Enter |
| VoiceOver swipe up / down (direct touch off) | Previous / next row | Up / Down |
| Two-finger tap | Stop speech, on every screen | Control |
| Two-finger double tap (magic tap) | Resume from the pause menu; nothing in other menus | none |
| Two-finger flick up | Help | F1 |
| Two-finger flick down, or two-finger scrub | Back | Escape |
| Two-finger flick left / right | Previous / next message | Comma / Period |
| Three-finger flick up / down | Top / bottom of the list | Home / End |
| Three-finger flick left / right | Previous / next page | Page Up / Page Down |
| Three-finger tap | What the screen can tell you: the job board's market summary, the weather along a route on the route picker, a download's progress | Tab |

In a name field the flicks keep the keys they had: flick left and right
review the letters, flick up and down are Up and Down, a three-finger tap
reads the name back (F2), and three-finger flicks up and down jump to the
start or end. A tap, a two-finger double tap and the two-finger flicks left
and right do nothing there, so they never type a comma, a space or a full
stop into the name.

With no hardware keyboard, menu help names gestures. A menu with no help of
its own says "Flick right or left to move and double tap to choose. Flick
down with two fingers to go back."

With no hardware keyboard, a menu that the two-finger flick down leaves, and
that has no row of its own going back ("Back", "Back to terminal", "Cancel",
"Keep driving"), gets a last row "Back". The main menu, the terminal and
prompts that must be answered get none.

The on-screen keyboard rises by itself when a name field opens and goes away
when it closes; the old three-finger double tap is gone.

## Driving gestures

One finger:

| Gesture | While driving |
|---|---|
| Swipe up and hold | Gas until you lift; lifting coasts. Releases a set parking brake ("Parking brake released") |
| Swipe down and hold | Brake until you lift |
| Deep swipe down (about three flicks long) | Emergency brake while moving; under 1 mph it sets the parking brake ("Parking brake set") |
| Swipe left / right and hold | Steer until you lift, with lane keeping off or partial. On full, flicks are the only lane control |
| Flick left / right | Change one lane |
| Flick up / down | Cruise off: flick up turns cruise on, at the remembered speed or else the current one; flick down does nothing. Cruise on: raise / lower the target |
| Hold still for half a second | Straighten the wheel until you lift |
| Double tap | Whatever the road is asking: confirm arrival at a facility gate, pull over for a trooper, open the stop the truck is stopped at on its ramp (check in at a scale), or take a called exit or stop (arm, confirm or cancel the exit signal). With nothing asking, it says there is no exit to signal for, as X does |

Two fingers:

| Gesture | While driving |
|---|---|
| Turn about 30 degrees right, like a key | Start the engine |
| Turn about 30 degrees left | Stop the engine, under 1 mph only; moving, it does nothing |
| Tap | Stop speech, as Control does |
| Hold | Horn until you lift; turning the fingers cancels it |

A second finger while holding a pedal:

| Gesture | Default command |
|---|---|
| While holding gas, tap | Automatic speed control: adaptive cruise, or the speed keeper in low-speed zones |
| While holding a pedal, flick up / down | Shift up / down in manual, working the clutch for you; nothing in automatic |
| While holding a pedal, flick left / right | Change one lane |
| While holding a pedal, swipe left / right and hold | Steer until you lift (lane keeping off or partial) |
| While holding brake, tap | Nothing; assign a command in Touch gestures |
| While holding brake, double tap | Nothing; assign a command in Touch gestures |

Other driving gestures:

| Gesture | Default command |
|---|---|
| Tap | Speed |
| Two-finger double tap (magic tap) | Pause |
| Three-finger flick up | Route and location |
| Three-finger flick down | Road ahead |

The tap, flick up and down, second-finger and multi-finger slots can be moved
to any other command in Settings, Gameplay, Controls, Touch gestures, the way
keyboard keys and controller buttons can. A two-finger flick down or scrub
pauses, a two-finger flick up is help, two-finger flicks left and right review
messages, and three-finger flicks left and right tune the radio. Everything
else is on the driving command list: a three-finger tap opens it, flick to a
command, and double tap runs it. The Status menu is its first row, so a three-finger tap
and then a double tap opens it. The pause menu's second row, Driving commands,
opens the same list, and the Touch gestures screen can put it on any slot as
"Driving command list". A command chosen from the list resumes the drive and
runs. The game opts out of iOS's three-finger editing gestures (undo, redo and
the shortcut bar, iOS 13 and later) so they do not take the tap. The list is
the same on every device and always opens on its first row:

| Row | Opens or runs |
|---|---|
| Status menu | Runs |
| Speed and cruise | Speed, Posted speed limit, Safe speed, Cruise, Resume the last cruise speed, Raise and Lower the cruise target |
| Road and stops | Road ahead, Route and location, Rest stop, Signal for the exit or a pull-over, Weather, Grade, Lane position, Lane locator, Repeat the last announcement |
| Where am I | The state, road, town, direction and county |
| Truck | Engine on or off, Parking brake, Fuel, Driver window down or up, Fan speed, Heat or air conditioning |
| Gears and engine brake | Shift up and down, Neutral, Reverse, Automatic or manual shifting, Engine brake, Automatic engine brake on or off, Engine brake stages 1 to 3 |
| Radio and CB | Radio on or off, Next and Previous radio station, Radio volume, Radio category, Save the station as a favorite, Radio status, What the radio is playing, Repeat the last CB chatter |
| Hours and clock | Clock, Time at the wheel, When the break is due, What ends this shift |
| Copy the last spoken text | Runs |
| Driving help | Runs |
| Back to driving | Closes the list |

Double tap opens a group; a command in it closes the list and runs. A
two-finger flick down from a group returns to its row in the top list. Radio
volume and Radio category change in place: flick up to raise or step forward,
flick down to lower or step back, and the list stays open.

In a manual truck, a second-finger flick up or down (or Shift up or Shift down
from the list) presses the clutch, takes the gear and lets the clutch out
again after the time an automatic takes to shift, so a shift never grinds.
Slowing in fourth or taller, the truck takes neutral where a keyboard
driver's engine would stall once the brake lets go, brake held or not, and
says "Neutral. Hold a pedal and flick up with a second finger for a gear."
(just "Neutral." once you have shifted out of neutral by touch three times).
A flick up from neutral takes the lowest gear your speed does not over-rev:
first when stopped, a taller one while rolling. A keyboard or controller
still takes first. After a stall in a tall gear, the two-finger turn right takes neutral
first, says "Neutral.", and starts the engine; then hold a pedal and flick up
for first.

So to set cruise: swipe up and hold until you reach 20 miles per hour, tap
with a second finger, and lift.

The thresholds are settings (`touch_flick_points` 30, `touch_flick_ms` 150,
`touch_deep_swipe_factor` 3, `touch_still_hold_ms` 500,
`touch_rotate_degrees` 30, `touch_parking_brake_mph` 1) in the settings file.
The deep swipe starts at the flick distance times the factor, 90 points by
default. The "Deep swipe starts at" row on the Touch gestures screen sets it
from 60 to 180 points in steps of 10 (flick up or down on the row); it
stores the factor, so a hand-edited file keeps working.

Each action has a haptic: a tick for a flick or tap, a bump when a hold starts
or ends, a warning for the deep swipe, and a success tap for the engine key.
The Touch gestures settings switch turns them off.

Before your first drive after touching the screen, the game offers Practice
gestures. It teaches the core rule first, then names each gesture without
moving the truck; either answer is remembered. With no hardware keyboard, the
practice row on the main and pause menus reads "Practice gestures" and opens
the same practice (with a keyboard it is "Practice keys and buttons"). Either
practice names keys, controller buttons and gestures alike. By touch, the
second two-finger flick down or scrub in a row leaves practice, however long
after the first; any other gesture in between starts the count over. A
keyboard still needs Escape twice within two seconds. When a swipe down and hold
lifts, practice reads how far it travelled and where the deep swipe starts
("Length 64. The deep swipe starts at 90."); a deep swipe reads its own
length ("Deep swipe. Length 112."). Only practice gets the lengths.

With no hardware keyboard connected, help, hints and prompts name gestures,
never keys. The game follows GameController keyboard connect and disconnect
notifications.

The game recognises the two-finger scrub itself (two or more quick reversals
of sideways direction within about a second, decided when the fingers lift),
so it goes back with direct touch on as well as off. Because of that, the
two-finger flicks left and right arrive when the fingers lift rather than
mid-stroke. VoiceOver's adjustable swipes (up and down with VoiceOver focus on
the game and direct touch off) move through a menu's rows, since they are then
the only way through a list.

## How gestures are recognised

Two recognisers read the same gestures, and Settings, Gameplay, Controls,
Touch gestures has a row at the end to choose between them, iPhone and iPad
only: "New gesture reading", off by default.

- Off, the touch surface's UIKit recognisers read the gestures
  (`ios/ff_touch.m`) and queue a code for each.
- On, every UIKit recogniser on the surface is disabled, and the surface
  queues its raw finger events instead (`ios/ff_fingers.m`): a finger id,
  where it is in points, and the touch's own timestamp. The game hands them
  to the recogniser Android uses (`ff_core::touch::recognizer`) at those
  timestamps and ticks it on the same clock each frame, so a slow frame
  cannot turn a flick into a hold. The haptics are the same table either
  way. VoiceOver's own actions (double tap with direct touch off, the
  adjustable swipes, the escape scrub, the magic tap, the three-finger
  scroll) are not finger events and arrive the same way with the row on or
  off.
- Switching lets go of anything a finger was holding first: a pedal, the
  horn, the straighten hold.

The shared recogniser was ported rule for rule from `ff_touch.m`, and where
the two differ the iPhone's behaviour is kept. Its rules are tested on the
desktop (`crates/ff-core/src/touch/recognizer_tests.rs`), and the agent
server's `fingers` tool plays raw fingers through it. Read against each
other on 2026-11-22, they differ in:

- The two-finger scrub's second: now timed from the second finger landing,
  as UIKit's is (it was timed from the first).
- UIKit's own taps, double taps and multi-finger flicks, whose distances and
  windows Apple does not publish: the shared one uses 0.4 seconds for the
  longest tap, 0.28 for a double tap's partner and a flick distance within
  half a second.
- The two-finger flick left or right is decided as the first finger lifts,
  not the last, and a scrub cannot be finished with one finger.
- A third finger during the horn leaves the horn on until the fingers lift. The row and the native
recognisers go once the shared one has been played on a phone.

To check on a phone with the row on: every gesture in Practice gestures
with VoiceOver on and off, a drive, and turning the row on and off in the
middle of a drive with a pedal held.

## Tilt steering

Settings, Gameplay, Controls, Touch gestures starts with "Tilt steering",
off by default. Off, nothing changes: swipes and flicks steer and the phone
never has to move. On, tilting the phone left or right steers as well, and
the swipes and flicks keep working.

- The tilt is the angle of the screen's left to right edge from level, so it
  reads the same with the phone held upright like a wheel or lying flatter in
  the hands. Right edge down steers right.
- Level is the pose the phone is in when the truck starts being driven:
  entering the drive, resuming it from the pause menu or any screen over it,
  or coming back to the app. The game takes it from a quarter second of
  steady readings and steers nothing until then. "Set tilt level", last in
  the Truck group of the driving command list, takes level again and says
  "Level set."; it has no gesture until one is put on it in Touch gestures,
  and it is listed only while tilt steering is on.
- Within 4 degrees of level nothing steers. "Tilt for full steering" (15 to
  45 degrees in steps of 5, 30 by default, listed only while tilt steering
  is on) is where steering is full; between the two it rises in a straight
  line.
- The tilt goes where a controller's left stick goes. With lane keeping on
  full, a tilt past half way changes one lane, and the phone has to come back
  near level before the next. With lane keeping off or partial it steers in
  proportion. A held steering swipe wins over a tilt, and a controller's
  stick wins over both.
- It steers only while you drive: not in menus, while paused, while the
  co-driver drives, or in Practice gestures.
- While tilt steering is on and the drive is on top, the screen does not turn
  with the phone. It holds the orientation it had when the drive came up and
  turns again once a menu covers the drive.
- With no motion sensor the row stays Off and says "This device has no motion
  sensor." If readings stop for a second mid-drive, tilt steers nothing and
  the game says "No tilt reading. Swipe to steer." once; when they return it
  takes level again and says "Tilt steering back."
- In Practice gestures, with tilt steering on, a tilt past half way names
  itself and its angle from flat once per push ("Tilt left, 18 degrees.
  Steers left, or changes one lane with lane keeping on full."), so you can
  check which way the sensor reads.

The accelerometer is read through SDL2's sensor API (the units, axes and
filtering are written down in `ff-core`'s `sim::tilt_sensor`). The screen
lock sets SDL's `SDL_IOS_ORIENTATIONS` hint to the current orientation and
asks UIKit to read it again (`ios/ff_orientation.m`).

## Controllers

Any controller iOS supports (Xbox, PlayStation, MFi, Switch Pro) is read
through SDL2's GameController backend, with the same bindings, rumble and
controller settings as the desktop.

## Building

You need a Mac with Xcode, the pinned Rust toolchain, and CMake.

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
cargo tools build-ios --install --launch   # Simulator
```

### An IPA for sideloading

To build for a real iPhone or iPad and produce an IPA without supplying an
Apple signing certificate or provisioning profile:

```bash
cargo tools build-ios --sideload
```

The output is `build/ios/FreightFate.ipa`, containing
`Payload/FreightFate.app`. Import that IPA into your sideloading tool, such as
[Sideloadly](https://sideloadly.io/), which signs and installs it using your
Apple account. The builder applies only an ad-hoc signature; the IPA needs
to be re-signed before a device can run it. Signing happens in the
sideloading tool, so the build does not need your Apple credentials.

This option selects the device target automatically. A Simulator `.app`
cannot be made into a device build just by zipping it or changing its
extension. `--sideload` cannot be combined with signing credentials or the
Simulator's `--install` / `--launch` options. Add `--no-music` for a smaller
IPA without the music pack.

### An IPA signed with your own certificate

For a signed device IPA, pass a signing identity and a provisioning profile
for the bundle identifier `net.orinks.freightfate`:

```bash
cargo tools build-ios --device --ipa \
    --sign-identity "Apple Development: ..." \
    --provisioning-profile path/to/profile.mobileprovision
```

This also writes `build/ios/FreightFate.ipa`. For direct installation,
use a development or ad-hoc distribution profile that includes your device;
an App Store profile is for App Store Connect / TestFlight uploads, not
direct sideloading. See Apple's
[registered-device distribution guide](https://developer.apple.com/documentation/xcode/distributing-your-app-to-registered-devices).
Omit `--ipa` if you only need the signed `.app` bundle.

The command builds the Rust game for the iOS target, bakes the world data,
stages the sound and music packs, writes `Info.plist`, and signs `build/ios/<sdk>/FreightFate.app`.

Prism compiles its VoiceOver/AVSpeech backend from source for iOS; the
`freight-fate` build script links the UIKit, AVFoundation and GameController
frameworks it and SDL2 need, plus clang's iOS runtime for `@available` checks.

## Platform notes

- Saves, settings and logs live in the app sandbox under
  `Library/Application Support/FreightFate`.
- The game does not update itself on iOS; the App Store or TestFlight does,
  so Settings has no Updates category. The main menu has no Quit (iOS closes
  apps, they do not quit themselves), and a two-finger flick down there does
  nothing. Backgrounding the app clears the crash marker; returning to the
  app starts tracking again.
- Leaving the app at the wheel (a call, Siri, Control Center, the
  notification banner pulled down, the app switcher, the Home gesture, the
  lock button) pauses the drive: the horn stops, held pedals let go, and the
  pause menu opens. A call or alarm that takes the sound, or headphones or a
  Bluetooth device going away, pauses it too. Coming back reads the pause
  menu from its title; any other screen reads its focused row again. The
  rules are `ff_core::app_lifecycle`.
- Going to the background saves the career, with the drive in progress
  where it was, so iOS ending the app there loses nothing: Continue resumes
  the drive at that spot, parked. When the app comes back the save points at
  the last stop again, so the pause menu's "the drive resumes from the last
  stop" stays true. Settings, Problem reports lets you turn crash
  prompts back on. The log-location row is left out because the log is in the
  sandbox, and the braille-only Output row needs NVDA or JAWS.
- The radio has no personal playlists on iPhone or Android. Their Playlists
  folder would sit in the sandbox, where the player cannot put files, so the
  dial, Radio status, Settings and How to play leave them out.
- Spoken prompts name the gesture for a control in a sentence of their own
  ("Turn two fingers right to restart.", "Double tap to signal for the
  exit."), or its group on the three-finger tap command list when no
  gesture runs it. Pressing a
  key on a hardware keyboard, or a controller button, switches them back to
  key or button names until the screen is touched again.
- Game audio in the Simulator has not been checked since Hearken replaced
  BASS (which had no Simulator device); speech is heard there. The game
  sets the `AVAudioSession` category to Playback before the audio engine
  opens (`ios/ff_lifecycle.m`), so it plays with the ring/silent switch on
  silent, and stops other apps' audio while it plays. cpal stops and resumes
  its stream around an interruption; the game asks for the session again
  when it is active.

## Manual VoiceOver test checklist

- Leaving and coming back, mid-drive at speed: take a call, pull down
  Control Center, press the lock button, and swipe up to the Home screen.
  Each time the truck stops where it was, and coming back reads "Paused."
  and the focused row. Do it again from a menu (Status, the terminal): the
  focused row is read once.
- An alarm or timer going off mid-drive pauses the drive; the game's sound
  comes back when it is dismissed.
- With the ring/silent switch on silent, the game's sound plays.
- Unplug wired headphones or switch off Bluetooth headphones mid-drive: the
  drive pauses, as a controller unplugged does.
- Mid-drive, swipe up to the app switcher and swipe the game away. Open it
  again and Continue: the drive resumes where it was, not at the last stop.

- Test with VoiceOver on and off; on iOS 17 or later, confirm direct touch is silent when the screen is touched.
- Help and practice say the core rule first: a flick is one step, a swipe and hold is continuous.
- At the centre, corners and every edge: swipe up and hold for gas, lift and hear the truck coast; swipe down and hold for brake. Edge strokes must not open Control Center, Notification Center or the app switcher on the first swipe.
- Flick up with cruise off: the first time it sets the current speed, after that it resumes the last one. Flick down with cruise off does nothing and says nothing; a short brake swipe must not set cruise. With cruise on, flicks raise and lower the target. Flick left and right for one lane each.
- With lane keeping off and partial, swipe left or right and hold to steer; on full, the hold says to flick and flicks still change lanes.
- Hold one finger still for half a second to straighten. A plain tap must still be Speed.
- Double tap when an exit is called (the exit signal arms), when a trooper's lights come on (the truck signals for the shoulder), stopped at a facility gate (it opens), and stopped at the end of a stop's ramp ("Double tap to pull in." and the stop opens) or on a scale ("Double tap to check in."). With nothing called, it says there is no exit to signal for.
- Two-finger tap during a long line in a menu and at the wheel: the line stops, and the menu or drive stays where it was.
- In a menu, flick right and left move through the rows and flick up and down change a value (a volume row in Settings). A tap reads the row again; tapping twice in a row does not step through messages.
- A two-finger double tap on the main menu does nothing; on the pause menu it resumes the drive.
- The two-finger scrub goes back, with direct touch on and off. Two-finger flicks left and right still review messages, and a slow two-finger flick is not taken for a scrub.
- Three-finger tap at the wheel, then double tap: the Status menu opens. In the list, double tap a group, flick through it, and two-finger flick down: the list is back on that group's row. On Radio volume, flick up twice: the volume rises twice and the list stays open. On the job board, a three-finger tap reads the market summary; on the route picker, the weather along the route.
- Deep swipe down while moving for the emergency brake; stopped, it says "Parking brake set", and gas then says "Parking brake released".
- Turn two fingers right to start the engine and left to stop it while stopped; at speed the turn left does nothing. Hold two fingers for the horn, then turn them and confirm the horn stops.
- While holding gas: second-finger tap sets cruise, flicks up and down shift in manual and do nothing in automatic, side flicks change lanes, side holds steer.
- In a manual truck (Automatic or manual shifting, in Gears and engine brake): pull away with a brake hold and a second-finger flick up for first, then shift up through several gears on the gas with no grinding. Brake to a stop in tenth: hear "Neutral." with the gear sentence on the way down, lift, and the engine keeps running; hold a pedal and flick up for first. Slow to about 10 miles per hour in tenth, hear "Neutral.", and flick up while rolling: a gear that fits the speed goes in, not first.
- In a Driving School lesson: the deep swipe while moving is the emergency brake, the still hold straightens, and gas releases the parking brake, as in a delivery.
- With no hardware keyboard, no help, hint or field prompt names a key; connect one and keys come back.
- Turn Tilt steering on in Touch gestures and open Practice gestures: tilt the right edge down past about 17 degrees and hear "Tilt right" with the angle, then the left edge for "Tilt left". Back at level, nothing is said.
- With tilt steering on and lane keeping on full, start a drive holding the phone however is comfortable. Tilt right and hold: one lane change only. Bring it back level and tilt again: one more.
- With lane keeping off, tilt gently and hear the truck drift that way in proportion; a steering swipe still steers.
- At the wheel with tilt steering on, turn the phone from portrait to landscape: the screen stays put. Open the pause menu and turn it: the screen turns. Resume holding the phone tilted: the new pose is level.
- Put "Set tilt level" on a gesture slot, tilt the phone a little at the wheel, run it, and hear "Level set."; the truck no longer drifts.
- Repeat the tilt checks with VoiceOver on.
- Verify magic tap pauses, a three-finger hold does nothing, the three-finger double tap does nothing, and the haptics switch changes feedback.
- Enter Practice gestures from the main menu row, flick up: it is named as a flick, never as an arrow key. Flick down with two fingers, wait ten seconds, and flick down again: practice closes. Verify the first-drive practice offer appears once.
- Pause the drive: Driving commands is the second row. Open it, choose a radio command: the drive resumes and the command runs.
- On Touch gestures, put "Driving command list" on the tap: a tap at the wheel opens the list.
- Open a list that had no way back, such as Call dispatch on the pause menu: its last row is Back, and double tapping it goes back. A list that already had one, such as Manage careers, has no second Back.
- Tap with three fingers many times at the wheel: the command list opens every time, and no undo banner or shortcut bar appears.
- In Practice gestures, swipe down and hold, then lift: hear the length and where the deep swipe starts. Carry a swipe into the deep swipe and lift: hear "Deep swipe." with its length. Try an ordinary brake swipe several times and note the lengths.
- Turn on "New gesture reading" (the last row of Touch gestures) and run
  this whole list again, VoiceOver on and off. Then, mid-drive with gas
  held, pause, turn the row off and back on, and resume: the gas has let
  go, and the next swipe and hold is read.
- On Touch gestures, flick up and down on "Deep swipe starts at": it moves in tens between 60 and 180, and a swipe in practice then needs that length to become the deep swipe.
