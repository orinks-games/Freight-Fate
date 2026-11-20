# prism-sys 0.1.6, patched

The crates.io `prism-sys` 0.1.6 package (sha256
`fb7205352adf3f71f6873ea93d0e5d315cf8118c534a4db6343d32a3847d7cfe`), which
carries Prism at `291db9fe04ea9f5642266cbebfeb909db4f38553`. Everything here
is the package as published except the two files below. MIT licence, as
upstream (`prism/LICENSES`).

## Why

A player with several sound devices on Windows 11 heard SAPI and OneCore
speech through their TV while the game's own sound and the built-in voice
played on the default device. Both backends open "the default" with the
Speech stream category and no device named, and Windows did not resolve that
to the console default. The patch names the console default device. Each
falls back to the unpatched call, with one warning in the log, if the device
cannot be named.

## Changes against 0.1.6

`prism/source/speech_player.cpp` (SAPI playback through XAudio2):

- New `device_interface_path()` in the anonymous namespace: turns the
  MMDevice endpoint id from `default_device_id()` into the interface path
  XAudio2 expects, `\\?\SWD#MMDEVAPI#<id>#{e6327cad-dcec-4949-ae8a-991e976a79d2}`.
- `SpeechPlayer::create_engine()`: reads `default_device_id()` before creating
  the mastering voice (the later read is removed) and passes that path as
  `szDeviceId`. If the path is empty or the call fails, logs a warning and
  makes the original `nullptr` call.

`prism/source/backends/onecore.cpp` (OneCore playback through AudioGraph):

- Includes `<tuple>` and `winrt/Windows.Devices.Enumeration.h`; adds
  `using namespace winrt::Windows::Devices::Enumeration`.
- New `default_render_device()`: `MediaDevice::GetDefaultAudioRenderId(
  AudioDeviceRole::Default)` through `DeviceInformation::CreateFromIdAsync`,
  null on any failure (logged).
- New static `create_graph(render_device)`: the graph and output node creation
  that `build_graph_locked()` used to do inline, with
  `AudioGraphSettings::PrimaryRenderDevice` set when a device is given.
- `build_graph_locked()`: tries `create_graph()` on the named default device,
  then without a device if that fails, logging a warning.

## When the device goes away

A named device no longer follows default changes by itself. SAPI: unplugging
it raises XAudio2's `OnCriticalError`, which marks the engine failed, and the
next line rebuilds the engine on the new default; a default change while no
line is playing is caught by the existing `default_device_changed()` check
before the next line. OneCore: losing the device raises
`UnrecoverableErrorOccurred` and a default change fires
`DefaultAudioRenderDeviceChanged`; both mark the graph stale, and the next line
rebuilds it on the new default. In both, the line playing when the device is
lost is cut, as it was before.

## Removing it

Drop `vendor/prism-sys`, its `exclude` entry and the `[patch.crates-io]`
block in the root `Cargo.toml` once a prism-sys release names the device
itself (or lets the program choose one).
