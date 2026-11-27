# Freight Fate on Android

The Android game is the desktop game, as on iPhone. The same Rust states and
menus run under SDL2's Android backend, and speech goes through Prism to
Android's text to speech. The screen is one full-screen touch area with the
iPhone's gestures: every gesture, threshold and setting in
[the iPhone guide](../ios/README.md#gestures) works the same way here, at the
wheel and in menus, and game controllers work as they do on the desktop.
Android 14 or newer is needed.

## TalkBack

TalkBack takes every touch for itself while it is on, and Android gives an
app no way to let touches through to it the way VoiceOver's direct touch
does. So the game's gestures need TalkBack suspended while you play:

- When the game comes to the front with TalkBack on, TalkBack says "Suspend
  TalkBack to play Freight Fate."
- The game speaks through text to speech whether TalkBack is on or not.
  There is no Main voice row on Android: a suspended TalkBack says nothing, so
  it is never the game's voice.

## Gestures

How the gestures work is in [the iPhone guide](../ios/README.md#gestures),
and Practice gestures teaches them in the game. Two things differ:

- Android's back gesture (or back button) goes back, as the two-finger scrub
  does. It never closes the game.
- Positions are measured in Android's density-independent pixels, 1/160 of an
  inch, so the settings' distances are about the same length on any phone
  as on an iPhone.

The haptics match the iPhone's: a tick for one step, a bump when a hold
starts or ends, a heavy click for the deep swipe, and a double click for the
engine key. Touch gestures in Settings turns them off.

[Tilt steering](../ios/README.md#tilt-steering) reads the accelerometer
through the same SDL path as on iPhone. SDL 2.26's Android sensor code calls
`ALooper_pollAll`, which NDK r27 and later refuse to compile, so the SDL
toolchain file forces in a header that stands in for it with
`ALooper_pollOnce` (`crates/freight-fate-android/cmake`). SDL reads
Android's orientation setting only when the window is made, so while tilt
steering is at the wheel the activity holds the screen in the orientation
it has, and puts SDL's own request back when tilt stops.

## Voices

Settings, Speech has three rows for the voice on Android, in place of the
single Speech voice row:

- **Voice engine**: any text to speech engine installed on the phone, not
  only the one chosen in Android's settings. Changing it picks one of its
  voices, in the language you were hearing when it has that language.
- **Voice language**: the languages that engine offers.
- **Speech voice**: that engine's voices in that language.

All three set the same saved voice. Speech rate, pitch and volume work as on
the desktop, with 50 percent as the voice's normal rate and pitch.

The first time the game lists the voices it connects to every installed
engine, which can take a few seconds. Moving to another engine's voice takes a
moment while that engine starts.

## Controllers

Any controller Android supports is read through SDL2's GameController
backend, with the same bindings, rumble and controller settings as the
desktop.

- On a USB cable, SDL rumbles a PlayStation, Xbox or Switch controller
  itself. A DualSense on a cable is also a sound card, and Android plays
  all of the phone's sound through it, so headphones go in the controller.
  Its last two channels drive the motors, so the game plays speech on the
  headphone channels only; otherwise Android copies the voice into all
  four and the motors buzz under every spoken line.
- Over Bluetooth, SDL 2.26 cannot rumble a controller, so the game asks
  Android to. Android gives no way to tell which of its controllers is the
  one SDL opened, so the rumble plays on every connected controller that has
  motors.

## Building

The APK is built on Windows, macOS or Linux without Gradle. You need:

- the pinned Rust toolchain, plus `rustup target add aarch64-linux-android`
  and `cargo install cargo-ndk`;
- the Android SDK (`ANDROID_HOME`) with the `android-36` platform, a
  build-tools release and the platform tools (`adb`);
- an Android NDK, r28 or newer (`ANDROID_NDK_HOME`, or the newest one
  under the SDK). Older ones link libraries that phones with 16 KB memory
  pages cannot load, and the build checks every library it packs for that;
- JDK 17 (`JAVA_HOME`, or `javac` on the path);
- CMake 3.24 or newer and Ninja.

```bash
cargo tools build-android --install --launch
```

That builds `build/android/FreightFate.apk`, installs it on the phone
connected over USB (with USB debugging on), and starts it. Other options:

- `--unsealed` leaves out the career sealing key, for a test build on a
  machine without it. Careers that build saves on the phone open only in
  other unsealed builds; cloud backups are unaffected.
- `--no-music` makes a smaller APK without the music pack.
- `--debug` builds the debug profile (always unsealed).
- `--abi x86_64` builds for the emulator instead of a phone.

The APK is signed with the Android debug key (`~/.android/debug.keystore`,
made on first use), which is what installing over USB needs.

What goes into the APK, and why each piece is there, is described at the top
of `crates/ff-tools/src/release/android.rs`. In short:

- `crates/freight-fate-android` is the library Android loads, `libmain.so`.
- SDL2 is built from source as its own `libSDL2.so`, because SDL's Java
  activity reaches it through JNI.
- The activity, `FreightFateActivity`, subclasses SDL's activity. It unpacks
  the game files into the app's storage and answers the game's few questions
  about the phone (TalkBack, haptics, screen density, a keyboard).
- The game compiles its own copies of three library files in place of the
  library's:
  - Prism's Android text to speech voice (`crates/freight-fate-android/prism/`),
    which centres rate and pitch on the voice's normal values, lists every
    engine's voices, and, while a controller's sound output is connected,
    hands each line to `SpeechRoute` to play on its headphone channels;
  - SDL's HIDAPI device manager (`crates/freight-fate-android/sdl/`), which
    Android 14 otherwise refuses at startup, leaving PlayStation controllers
    without rumble;
  - SDL's controller manager (`crates/freight-fate-android/sdl/`), which
    otherwise opens a Bluetooth keyboard as a controller, because Android
    lists its arrow keys as a D-pad and a volume wheel or media keys as a
    joystick.

  The build stops if a library's own file changes, so a new SDL or Prism
  is checked against the copy before it ships.

## Platform notes

- Saves, settings, the online sign-in and logs live in the app's private
  storage, under `files/FreightFate`. The game files are unpacked into
  `files/game` on first launch, and again after each install. Uninstalling
  the game deletes all of it, so turn on cloud backup first.
- The game does not update itself on Android; installing a new APK does.
  Settings has no Updates or Problem reports category (the log is in the
  app's storage, out of reach), and no braille-only Output row (it needs NVDA
  or JAWS).
- The main menu has no Quit. Close the game from the app switcher; it ends
  straight away, with no question first. A drive is saved only at a stop, as
  on the desktop, so closing mid-drive, or Android ending the game while it
  is in the background, loses the drive back to its last stop.
- The sign-in for orinks.net is kept in a file only the game can read,
  because Android has no keyring the game can use. That file is left out
  when Android copies the game's storage to a new phone, which links again.
- Only 64-bit ARM phones are built for so far.

## Manual test checklist

Everything in [the iPhone checklist](../ios/README.md#manual-voiceover-test-checklist)
that does not name VoiceOver applies, with TalkBack suspended. Then:

- With TalkBack on, open the game: TalkBack says to suspend it. Suspend it and
  the gestures work.
- The back gesture goes back in menus and pauses at the wheel; it never
  closes the game.
- Close the game from the app switcher right after a save, then open it: no
  report a problem question, and the career is where it was.
- Link the phone to an orinks.net account, close the game, open it again: it
  is still linked.
- In Settings, Speech, step through Voice engine, Voice language and Speech
  voice; each change is spoken in the new voice. Set pitch and rate to 50
  percent: the voice sounds normal.
- Connect a controller: menus and a drive work with it, and rumble plays.
- Connect a Bluetooth keyboard: no controller is announced, and the arrow
  keys move through menus and steer as arrow keys.
