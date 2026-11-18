//! Android: the game's side of its activity, `FreightFateActivity`.
//!
//! The Android library (`crates/freight-fate-android`) hands the Java VM to
//! [`set_java_vm`] from `JNI_OnLoad`, then calls [`start`] on SDL's main
//! thread before the game begins. From then on the game reaches the
//! activity's few helpers (unpacking the game files, TalkBack's state)
//! through this module, from whichever thread asks; every call attaches
//! that thread to the VM first, as the runtime requires of a thread it did
//! not start.

use std::path::PathBuf;
use std::sync::OnceLock;

use jni::objects::{JObject, JString};
use jni::refs::Global;
use jni::{jni_sig, jni_str, JavaVM};

/// The process's Java VM, from `JNI_OnLoad`.
static VM: OnceLock<JavaVM> = OnceLock::new();

/// The activity SDL runs in, held for the life of the process.
static ACTIVITY: OnceLock<Global<JObject<'static>>> = OnceLock::new();

/// Record the process's Java VM.
///
/// # Safety
///
/// `vm` must be the pointer the runtime passed to the library's
/// `JNI_OnLoad`, which stays valid for the life of the process.
pub unsafe fn set_java_vm(vm: *mut jni::sys::JavaVM) {
    // SAFETY: the caller promises `vm` is the live process VM.
    let _ = VM.set(unsafe { JavaVM::from_raw(vm) });
}

/// Take hold of SDL's activity, register it with `ndk-context` (which cpal
/// and webbrowser read), and unpack the game files from the APK. Returns
/// the app's private files folder.
///
/// # Safety
///
/// `activity` must be the local reference `SDL_AndroidGetActivity` returned
/// on this thread and not yet deleted.
pub unsafe fn start(activity: jni::sys::jobject) -> Result<PathBuf, String> {
    let vm = VM.get().ok_or("the Java VM was never recorded")?;
    let files = vm
        .attach_current_thread(|env| -> jni::errors::Result<Option<String>> {
            // SAFETY: the caller hands over a live local reference. It
            // belongs to SDL's frame, not the one this attachment opened, so
            // it is left for that frame to release.
            let local = unsafe { JObject::from_raw(env, activity) };
            let global = env.new_global_ref(&local)?;
            let path = env
                .call_method(
                    global.as_obj(),
                    jni_str!("prepareGameFiles"),
                    jni_sig!(() -> JString),
                    &[],
                )?
                .l()?;
            let path = if path.is_null() {
                None
            } else {
                Some(env.cast_local::<JString>(path)?.try_to_string(env)?)
            };
            // SAFETY: the VM and the global reference both live for the rest
            // of the process (the global is stored below and never dropped),
            // and this runs once, before anything reads the context.
            unsafe {
                ndk_context::initialize_android_context(
                    vm.get_raw().cast(),
                    global.as_obj().as_raw().cast(),
                );
            }
            let _ = ACTIVITY.set(global);
            Ok(path)
        })
        .map_err(|err| format!("could not reach the activity: {err}"))?;
    files
        .map(PathBuf::from)
        .ok_or_else(|| "the game files could not be unpacked".to_string())
}

/// Attach the calling thread to the Java VM for the rest of its life.
///
/// Prism's Android voices call into Java from the speech worker, a thread
/// the runtime did not start; it attaches itself here before building them.
pub fn attach_current_thread() {
    let Some(vm) = VM.get() else {
        return;
    };
    if let Err(err) = vm.attach_current_thread(|_env| Ok::<(), jni::errors::Error>(())) {
        log::warn!(
            "Could not attach the {:?} thread to the Java VM: {err}",
            std::thread::current().name()
        );
    }
}

/// Whether TalkBack (or another screen reader) is exploring by touch, which
/// takes the screen's gestures for itself.
pub fn touch_exploration_enabled() -> bool {
    call_bool(jni_str!("touchExplorationEnabled"))
}

/// Whether a hardware keyboard is connected and open.
pub fn hardware_keyboard_connected() -> bool {
    call_bool(jni_str!("hardwareKeyboardConnected"))
}

/// The screen's pixels per point (Android's display density: a point is a
/// dp, 1/160 of an inch). `0.0` when the activity cannot be reached.
pub fn display_density() -> f32 {
    let (Some(vm), Some(activity)) = (VM.get(), ACTIVITY.get()) else {
        return 0.0;
    };
    vm.attach_current_thread(|env| -> jni::errors::Result<f32> {
        env.call_method(
            activity.as_obj(),
            jni_str!("displayDensity"),
            jni_sig!(() -> float),
            &[],
        )?
        .f()
    })
    .unwrap_or_else(|err| {
        log::warn!("Activity call failed: {err}");
        0.0
    })
}

/// The phone's language as Android names it in that language ("English
/// (United States)"), the way Prism names each voice's language; `None`
/// when the activity cannot be reached.
pub fn default_language_name() -> Option<String> {
    let (Some(vm), Some(activity)) = (VM.get(), ACTIVITY.get()) else {
        return None;
    };
    vm.attach_current_thread(|env| -> jni::errors::Result<String> {
        let name = env
            .call_method(
                activity.as_obj(),
                jni_str!("defaultLanguageName"),
                jni_sig!(() -> JString),
                &[],
            )?
            .l()?;
        env.cast_local::<JString>(name)?.try_to_string(env)
    })
    .map_err(|err| log::warn!("Activity call failed: {err}"))
    .ok()
}

/// Play a gesture haptic: 0 a bump, 1 a tick, 2 a warning, 3 a success.
pub fn haptic(kind: i32) {
    let (Some(vm), Some(activity)) = (VM.get(), ACTIVITY.get()) else {
        return;
    };
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        env.call_method(
            activity.as_obj(),
            jni_str!("haptic"),
            jni_sig!((kind: jint) -> void),
            &[jni::JValue::Int(kind)],
        )?;
        Ok(())
    });
    if let Err(err) = result {
        log::warn!("Haptic failed: {err}");
    }
}

/// Rumble a controller through Android, for one SDL cannot rumble (a
/// controller on Bluetooth): both motors 0 to 65535 for `duration_ms`, as
/// SDL takes them; 0 and 0 stops them. The activity plays it on its own
/// thread, so this returns at once.
pub fn pad_rumble(low: u16, high: u16, duration_ms: u32) {
    let (Some(vm), Some(activity)) = (VM.get(), ACTIVITY.get()) else {
        return;
    };
    let level = |motor: u16| i32::from(motor >> 8);
    let result = vm.attach_current_thread(|env| -> jni::errors::Result<()> {
        env.call_method(
            activity.as_obj(),
            jni_str!("padRumble"),
            jni_sig!((low: jint, high: jint, duration_ms: jint) -> void),
            &[
                jni::JValue::Int(level(low)),
                jni::JValue::Int(level(high)),
                jni::JValue::Int(i32::try_from(duration_ms).unwrap_or(i32::MAX)),
            ],
        )?;
        Ok(())
    });
    // Sent every frame while a rumble plays: one line says it is failing.
    static LOGGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if let Err(err) = result {
        if !LOGGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
            log::warn!("Controller rumble failed: {err}");
        }
    }
}

/// Call a `boolean name()` method on the activity; `false` when the activity
/// cannot be reached.
fn call_bool(name: &'static jni::strings::JNIStr) -> bool {
    let (Some(vm), Some(activity)) = (VM.get(), ACTIVITY.get()) else {
        return false;
    };
    vm.attach_current_thread(|env| -> jni::errors::Result<bool> {
        env.call_method(activity.as_obj(), name, jni_sig!(() -> boolean), &[])?
            .z()
    })
    .unwrap_or_else(|err| {
        log::warn!("Activity call failed: {err}");
        false
    })
}
