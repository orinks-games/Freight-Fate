# Freight Fate on iPhone and iPad

The iOS game is the desktop game. The same Rust states and menus run under
SDL2's UIKit backend, and speech goes through Prism, which talks to VoiceOver
when it is running and to the system voice (AVSpeech) when it is not. The
screen is one full-screen touch area; in menus its gestures are the same key
presses the desktop game reads, at the wheel they run driving commands
directly, and game controllers work exactly as they do
on the desktop.

## Gestures

The core rule: **a flick is one step; a swipe and hold is continuous.** A
stroke that travels 30 points and lifts within 150 ms of getting there is a
flick. One that stays down is a hold in its direction until you lift. Every
gesture works anywhere on the screen; nothing depends on where you touch.
The game asks iOS to defer its edge gestures, so a stroke from an edge reaches
the game first.

With VoiceOver on, the game screen is a direct-touch area (silent on touch on
iOS 17 and later): once VoiceOver focus lands on it (it does at launch), your
gestures go straight to the game. VoiceOver's own standard actions are also
answered.

Outside the drive, and for the gestures the drive leaves fixed:

| Gesture | Key |
|---|---|
| Tap | Comma (repeat the last line) |
| Double tap, or VoiceOver double tap | Enter |
| Flick up / down | Up / Down |
| Flick left / right | Left / Right |
| Two-finger tap | Tab |
| Two-finger swipe up | F1 (help) |
| Two-finger swipe down | Escape |
| Two-finger swipe left / right | Comma / Period |
| VoiceOver magic tap (two-finger double tap) | Space |
| Three-finger swipe up / down | Home / End |
| Three-finger swipe left / right | Page Up / Page Down |
| Three-finger tap | F2: while driving, the list of every driving command; in a name field, read the name back |

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
| Flick up / down | Cruise off: resume the last speed / set the current speed. Cruise on: raise / lower the target |
| Hold still for half a second | Straighten the wheel until you lift |

Two fingers:

| Gesture | While driving |
|---|---|
| Turn about 30 degrees right, like a key | Start the engine |
| Turn about 30 degrees left | Stop the engine |
| Hold | Horn until you lift; turning the fingers cancels it |

A second finger while holding a pedal:

| Gesture | Default command |
|---|---|
| While holding gas, tap | Automatic speed control: adaptive cruise, or the speed keeper in low-speed zones |
| While holding a pedal, flick up / down | Shift up / down in manual; in automatic it says the current gear |
| While holding a pedal, flick left / right | Change one lane |
| While holding a pedal, swipe left / right and hold | Steer until you lift (lane keeping off or partial) |
| While holding brake, tap | Parking brake |
| While holding brake, double tap | Engine on or off |

Other driving gestures:

| Gesture | Default command |
|---|---|
| Tap | Speed |
| Two-finger tap | Status menu |
| Magic tap (two-finger double tap) | Pause |
| Three-finger swipe up | Route and location |
| Three-finger swipe down | Road ahead |
| Three-finger hold | Horn until you lift |

The tap, flick up and down, second-finger and multi-finger slots can be moved
to any other command in Settings, Gameplay, Controls, Touch gestures, the way
keyboard keys and controller buttons can. Two-finger swipe down pauses,
two-finger swipe up is help, two-finger swipes left and right review messages,
and three-finger swipes left and right tune the radio. Everything else is on
the driving command list: a three-finger tap opens it, swipe to a command, and
double tap runs it.

So to set cruise: swipe up and hold until you reach 20 miles per hour, tap
with a second finger, and lift.

The thresholds are settings (`touch_flick_points` 30, `touch_flick_ms` 150,
`touch_deep_swipe_factor` 3, `touch_still_hold_ms` 500,
`touch_rotate_degrees` 30, `touch_parking_brake_mph` 1) in the settings file.

Each action has a haptic: a tick for a flick or tap, a bump when a hold starts
or ends, a warning for the deep swipe, and a success tap for the engine key.
The Touch gestures settings switch turns them off.

Before your first drive after touching the screen, the game offers Practice
gestures. It teaches the core rule first, then names each gesture without
moving the truck; either answer is remembered.

With no hardware keyboard connected, help, hints and prompts name gestures,
never keys. The game follows GameController keyboard connect and disconnect
notifications.

Because the game screen takes touches directly, VoiceOver's scrub (two-finger
Z) reads there as a two-finger swipe; use a two-finger swipe down to go back.
VoiceOver's adjustable swipes (up and down with VoiceOver focus on the game)
also send Up and Down.

## Controllers

Any controller iOS supports (Xbox, PlayStation, MFi, Switch Pro) is read
through SDL2's GameController backend, with the same bindings, rumble and
controller settings as the desktop.

## Building

You need a Mac with Xcode, the pinned Rust toolchain, `uv`, and CMake.

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
uv sync --group dev
uv run python tools/build_ios.py --install --launch   # Simulator
```

For a device, pass a signing identity and a provisioning profile for the
bundle identifier `net.orinks.freightfate`:

```bash
uv run python tools/build_ios.py --device \
    --sign-identity "Apple Development: ..." \
    --provisioning-profile path/to/profile.mobileprovision
```

The script builds the Rust game for the iOS target, bakes the world data,
downloads and verifies the BASS iOS frameworks (never committed), stages the
sound and music packs, writes `Info.plist`, and signs `build/ios/<sdk>/FreightFate.app`.

Prism compiles its VoiceOver/AVSpeech backend from source for iOS; the
`freight-fate` build script links the UIKit, AVFoundation and GameController
frameworks it and SDL2 need, plus clang's iOS runtime for `@available` checks.

## Platform notes

- Saves, settings and logs live in the app sandbox under
  `Library/Application Support/FreightFate`.
- The game does not update itself on iOS; the App Store or TestFlight does,
  so Settings has no Updates category. The main menu has no Quit (iOS closes
  apps, they do not quit themselves), and a two-finger swipe down there does
  nothing. Settings also leaves out Problem reports (the log is in the
  sandbox, out of reach) and the braille-only Output row (it needs NVDA or
  JAWS).
- Spoken prompts name the gesture for a control ("press a second-finger
  double tap while holding brake to start the engine"), or its row
  on the three-finger tap command list when no gesture runs it. Pressing a
  key on a hardware keyboard, or a controller button, switches them back to
  key or button names until the screen is touched again.
- The Simulator has no BASS audio device, so only speech is heard there.

## Manual VoiceOver test checklist

- Test with VoiceOver on and off; on iOS 17 or later, confirm direct touch is silent when the screen is touched.
- Help and practice say the core rule first: a flick is one step, a swipe and hold is continuous.
- At the centre, corners and every edge: swipe up and hold for gas, lift and hear the truck coast; swipe down and hold for brake. Edge strokes must not open Control Center, Notification Center or the app switcher on the first swipe.
- Flick up and down with cruise off (resume, set) and on (target up, down); flick left and right for one lane each.
- With lane keeping off and partial, swipe left or right and hold to steer; on full, the hold says to flick and flicks still change lanes.
- Hold one finger still for half a second to straighten. A plain tap must still be Speed and a double tap Enter.
- Deep swipe down while moving for the emergency brake; stopped, it says "Parking brake set", and gas then says "Parking brake released".
- Turn two fingers right to start the engine and left to stop it; hold two fingers for the horn, then turn them and confirm the horn stops.
- While holding gas: second-finger tap sets cruise, flicks up and down shift in manual and say the gear in automatic, side flicks change lanes, side holds steer.
- With no hardware keyboard, no help, hint or field prompt names a key; connect one and keys come back.
- Verify magic tap pauses, the scrub escapes, the three-finger double tap does nothing, and the haptics switch changes feedback.
- Enter Practice gestures, verify the double two-finger swipe down exits, and verify the first-drive practice offer appears once.
