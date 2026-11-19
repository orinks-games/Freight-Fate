//! Freight Fate on Android: the `libmain.so` that SDL's activity loads.
//!
//! The app is `FreightFateActivity` (Java, under `java/`), a subclass of
//! SDL's `SDLActivity`. It loads `libc++_shared.so`, `libSDL2.so` and this
//! library, then SDL starts a thread and calls
//! [`SDL_main`] on it. That thread is the game's main thread from then on,
//! exactly as the process's main thread is on the desktop.
//!
//! Nothing here exists off Android, so a desktop workspace build compiles
//! this crate to an empty library.
#![cfg(target_os = "android")]

use std::ffi::{c_char, c_int, c_void};

extern "C" {
    /// SDL's activity, as a local reference on the calling thread.
    fn SDL_AndroidGetActivity() -> *mut c_void;
    /// `djinni::jniInit(JavaVM*)`, inside Prism's static library: it
    /// registers the native half of Prism's Android voices. Prism ships
    /// djinni's own `JNI_OnLoad` for this, but this library has to define
    /// `JNI_OnLoad` itself, so the archive member holding djinni's is never
    /// linked and its work is done here instead.
    #[link_name = "_ZN6djinni7jniInitEP7_JavaVM"]
    fn djinni_jni_init(vm: *mut jni::sys::JavaVM);
    /// `djinni::jniShutdown()`, the other half.
    #[link_name = "_ZN6djinni11jniShutdownEv"]
    fn djinni_jni_shutdown();
}

/// The runtime calls this as `System.loadLibrary("main")` loads the library;
/// it is the one place the Java VM is handed over directly.
///
/// # Safety
///
/// Only the Java runtime calls this, once, with its own live VM.
#[no_mangle]
pub unsafe extern "system" fn JNI_OnLoad(
    vm: *mut jni::sys::JavaVM,
    _reserved: *mut c_void,
) -> jni::sys::jint {
    // SAFETY: `vm` is the process VM the runtime passes to `JNI_OnLoad`,
    // valid for the life of the process; djinni keeps it for the same span.
    unsafe {
        freight_fate::android::set_java_vm(vm);
        djinni_jni_init(vm);
    }
    jni::sys::JNI_VERSION_1_6
}

/// The runtime's matching call when the library is unloaded.
#[no_mangle]
pub extern "system" fn JNI_OnUnload(_vm: *mut jni::sys::JavaVM, _reserved: *mut c_void) {
    // SAFETY: undoes `djinni_jni_init` above, once, as the library goes.
    unsafe { djinni_jni_shutdown() };
}

/// SDL's entry point: the game, run as a player launches it (no arguments).
#[no_mangle]
pub extern "C" fn SDL_main(_argc: c_int, _argv: *mut *mut c_char) -> c_int {
    // First of all, so the opening phase mark charges library loading to
    // the launch instead of losing it.
    freight_fate::app::boot_timing::start();
    // SAFETY: SDL's main thread is attached to the VM, and the activity
    // reference is fresh from SDL and handed straight on.
    let files = match unsafe { freight_fate::android::start(SDL_AndroidGetActivity().cast()) } {
        Ok(files) => files,
        Err(err) => {
            log::error!("Freight Fate could not start: {err}");
            return 1;
        }
    };
    ff_core::settings::paths::set_android_files_dir(files);
    // Speak through the system's text to speech voice, never TalkBack. The
    // game's gestures need TalkBack suspended, and a suspended TalkBack
    // still counts as enabled, so Prism's automatic choice would hand every
    // line to a screen reader that says nothing. Set before any game thread
    // starts; an explicit choice in the environment still wins.
    if std::env::var_os(freight_fate::speech::SPEECH_BACKEND_ENV).is_none() {
        std::env::set_var(
            freight_fate::speech::SPEECH_BACKEND_ENV,
            "Android Text to Speech",
        );
    }
    // The same doors `main.rs` opens for a player's launch: this process is
    // the game, so it may reach the network, the player's saves and the
    // browser.
    freight_fate::browser::allow_real_browser();
    freight_fate::net::allow_real_network();
    ff_core::settings::paths::allow_real_save_dir();
    freight_fate::online_presence::allow_real_secret_store();
    let code =
        freight_fate::app::main_with(freight_fate::app::CliOptions::parse(std::iter::empty()));
    // End the process with the game. Android may otherwise keep it and
    // start the next launch in it, and this library's start-up (the Java VM
    // and activity it records, the context registered for cpal) runs once
    // per process.
    std::process::exit(code)
}
