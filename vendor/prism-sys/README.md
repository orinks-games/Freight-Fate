# Prismer

Rust bindings to [prism](https://github.com/ethindp/prism), the platform-agnostic reader interface for speech and messages. Prism unifies screen readers and TTS engines (SAPI, NVDA, JAWS, VoiceOver, AVSpeech, speech-dispatcher, Orca, Android TTS, WebSpeech, and more) behind one API, so this crate gives your Rust application speech, braille, and screen reader output on every major platform.

## Crates

- `prism-sys`: raw FFI bindings to the prism C API, `no_std`.
- `prismer`: safe idiomatic wrapper.

## Quick start

```rust
let prism = prismer::Prism::new()?;
let backend = prism.create_best()?;
backend.speak("Hello from Rust!", false)?;
```

See `crates/prismer/examples/speak.rs` for a fuller example, including feature detection and waiting for speech to finish.

## Picking a backend

There are four ways to get a backend:

| Method | Backend state | Initialized on return |
| --- | --- | --- |
| `create_best()` | private to you | yes |
| `create(id)` | private to you | no, call `initialize()` |
| `acquire_best()` | shared with other callers | yes |
| `acquire(id)` | shared with other callers | maybe, call `initialize()` and treat `AlreadyInitialized` as success |

Use `create_best()` unless you have a specific reason not to. The `acquire` family returns a cached instance, so a voice, rate, or pitch set through one handle is visible through every other handle to that backend. Reach for it only when sharing that state is what you want.

## Building

Prism is vendored as a git submodule and built automatically by `prism-sys`'s build script, so all you need is a C++23 toolchain and CMake 3.24+:

```sh
git clone --recurse-submodules https://github.com/accessware/prismer
cd prismer
cargo build
```

If you cloned without `--recurse-submodules`, run `git submodule update --init` first.

To link against a prebuilt prism instead (skipping the CMake build), set `PRISM_LIB_DIR` to the directory containing the library:

```sh
set PRISM_LIB_DIR=path\to\prism\build
cargo build
```

By default prism is built and linked as a shared library; note that your application must be able to find it (next to the executable on Windows, or on the loader path elsewhere) at runtime. Enable the `static` feature to build and link it statically instead. The build script links the C++ runtime and the system libraries prism needs for you.

On Apple platforms that only holds when cargo does the final link. If you build a Rust `staticlib` and link it from Xcode, add the system frameworks to your Xcode target yourself: Foundation and AVFoundation everywhere, plus AppKit, CoreServices, Security, IOKit and CoreFoundation on macOS, UIKit on iOS, tvOS and visionOS, or WatchKit on watchOS. prism's Objective-C++ also needs `libobjc`, which Xcode links for any target with Objective-C or Swift code of its own.

## Custom backends

You can write a backend in Rust and register it alongside prism's own. Implement `CustomBackend`, declare the operations you implemented, and freeze a registry:

```rust
use prismer::{CustomBackend, Features, Prism, RegistryBuilder};

struct Printer;

impl CustomBackend for Printer {
	fn speak(&mut self, text: &str, interrupt: bool) -> prismer::Result<()> {
		println!("{text}");
		Ok(())
	}
}

let mut builder = RegistryBuilder::new()?;
let id = builder.add_backend("Example Printer", 10, Features::SPEAK, || Some(Printer))?;
let registry = builder.freeze()?;
let prism = Prism::builder().registry(&registry).build()?;
let backend = prism.create(id)?;
```

Every trait method defaults to `NotImplemented`, and the vtable handed to prism gets a pointer only for the features you declare, so the two can never disagree. The factory closure runs once per instance, so instances never share state. See `crates/prismer/examples/custom_backend.rs` for a full program.

`RegistryBuilder::add_library` loads a prism plugin shared library and registers the backends it supplies.

## Logging

The `log` module wraps prism's process-wide logger:

```rust
prismer::log::set_level(prismer::log::Level::Warn);
prismer::log::set_handler(|level, source, message| {
	eprintln!("[{level:?}] {source}: {message}");
});
```

The handler runs on prism's logging thread, so keep it quick. It is leaked on purpose: prism may still deliver to a replaced handler, so there is no safe moment to free one.

## Status

Early draft, but the C API is fully covered: context init and configuration (including availability callbacks), registry queries, backend creation and acquisition, speech, braille, output, playback control, volume, rate, pitch, voice enumeration and selection, audio format queries, speak-to-memory with a closure callback, custom backends and plugin libraries through the registry builder, and the logging API.

## License

MIT. Prism itself is MPL-2.0.
