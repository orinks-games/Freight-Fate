//! Raw FFI bindings to prism (<https://github.com/ethindp/prism>), a
//! platform-agnostic speech and screen reader output library.
#![no_std]
#![allow(non_camel_case_types)]

use core::ffi::{c_char, c_int, c_void};

/// An opaque prism context, holding a backend registry.
#[repr(C)]
#[derive(Debug)]
pub struct PrismContext {
	_opaque: [u8; 0],
}

/// An opaque backend instance obtained from a registry.
#[repr(C)]
#[derive(Debug)]
pub struct PrismBackend {
	_opaque: [u8; 0],
}

/// An opaque, immutable set of backend registrations.
#[repr(C)]
#[derive(Debug)]
pub struct PrismRegistry {
	_opaque: [u8; 0],
}

/// An opaque, mutable collection of backend registrations.
#[repr(C)]
#[derive(Debug)]
pub struct PrismRegistryBuilder {
	_opaque: [u8; 0],
}

/// A backend's identifier, derived from its name.
pub type PrismBackendId = u64;
/// An error code. `PRISM_OK` means success; every other value is a failure.
pub type PrismError = c_int;
/// The severity of a log message, or a threshold for delivering them.
pub type PrismLogLevel = c_int;

/// Channel count was retrieved.
pub const PRISM_OK: PrismError = 0;
/// The backend has not been initialized.
pub const PRISM_ERROR_NOT_INITIALIZED: PrismError = 1;
/// The vtable's `size` member was zero, the declared feature set was inconsistent with the vtable, or the priority value was negative.
pub const PRISM_ERROR_INVALID_PARAM: PrismError = 2;
/// The backend does not support audio format queries.
pub const PRISM_ERROR_NOT_IMPLEMENTED: PrismError = 3;
/// No voices are available for this backend
pub const PRISM_ERROR_NO_VOICES: PrismError = 4;
/// The specified voice was not found
pub const PRISM_ERROR_VOICE_NOT_FOUND: PrismError = 5;
/// Speech synthesis failed.
pub const PRISM_ERROR_SPEAK_FAILURE: PrismError = 6;
/// Memory allocation failed during initialization.
pub const PRISM_ERROR_MEMORY_FAILURE: PrismError = 7;
/// A parameter value exceeded its valid range
pub const PRISM_ERROR_RANGE_OUT_OF_BOUNDS: PrismError = 8;
/// An internal error occurred during initialization.
pub const PRISM_ERROR_INTERNAL: PrismError = 9;
/// No speech is currently playing.
pub const PRISM_ERROR_NOT_SPEAKING: PrismError = 10;
/// Speech is not currently paused.
pub const PRISM_ERROR_NOT_PAUSED: PrismError = 11;
/// Speech is already paused.
pub const PRISM_ERROR_ALREADY_PAUSED: PrismError = 12;
/// `text` contains invalid UTF-8 sequences.
pub const PRISM_ERROR_INVALID_UTF8: PrismError = 13;
/// The builder is spent, or a backend with the same name or the same identifier is already present in the builder.
pub const PRISM_ERROR_INVALID_OPERATION: PrismError = 14;
/// The backend was already initialized.
pub const PRISM_ERROR_ALREADY_INITIALIZED: PrismError = 15;
/// The backend's underlying system component is unavailable.
pub const PRISM_ERROR_BACKEND_NOT_AVAILABLE: PrismError = 16;
/// An unspecified error occurred.
pub const PRISM_ERROR_UNKNOWN: PrismError = 17;
/// The audio format which the underlying engine returned to Prism cannot be understood by Prism, or it's parameters are nonsensical.
pub const PRISM_ERROR_INVALID_AUDIO_FORMAT: PrismError = 18;
/// The backend possesses an internal hard ceiling as to how many instances may be instantiated at any given time, and this limit would be exceeded were another to be initialized.
pub const PRISM_ERROR_INTERNAL_BACKEND_LIMIT_EXCEEDED: PrismError = 19;
/// An error occured when the backend was executing a function which has caused the backend to enter an undefined state. The caller should re-initialize the backend from scratch.
pub const PRISM_ERROR_BACKEND_ENTERED_UNDEFINED_STATE: PrismError = 20;
/// A shared library could not be opened, because no file exists at the given path, it is not a loadable image, it was built for a different architecture, or its initialization code failed
pub const PRISM_ERROR_LIBRARY_LOAD_FAILED: PrismError = 21;
/// A shared library was opened but does not export the plugin entry point
pub const PRISM_ERROR_LIBRARY_INVALID: PrismError = 22;
/// A plugin declined the host, or a backend descriptor declared an ABI generation this build of Prism does not accept
pub const PRISM_ERROR_INCOMPATIBLE_ABI: PrismError = 23;
/// The number of defined error codes. Not itself an error.
pub const PRISM_ERROR_COUNT: PrismError = 24;

/// The most verbose level, used for fine-grained tracing of internal operations.
pub const PRISM_LOG_LEVEL_TRACE: PrismLogLevel = 0;
/// Diagnostic information useful during development.
pub const PRISM_LOG_LEVEL_DEBUG: PrismLogLevel = 1;
/// Informational messages describing normal operation.
pub const PRISM_LOG_LEVEL_INFO: PrismLogLevel = 2;
/// Conditions that are not errors but MAY indicate a problem.
pub const PRISM_LOG_LEVEL_WARN: PrismLogLevel = 3;
/// Error conditions.
pub const PRISM_LOG_LEVEL_ERROR: PrismLogLevel = 4;
/// Not a message severity. When supplied to `prism_set_log_level`, it suppresses all messages, since no message has a severity greater than or equal to it.
pub const PRISM_LOG_LEVEL_NONE: PrismLogLevel = 5;

/// The underlying engine or service is available. This determination is advisory; `prism_backend_initialize` MAY still fail.
pub const PRISM_BACKEND_IS_SUPPORTED_AT_RUNTIME: u64 = 1 << 0;
/// `prism_backend_speak` is implemented.
pub const PRISM_BACKEND_SUPPORTS_SPEAK: u64 = 1 << 2;
/// `prism_backend_speak_to_memory` is implemented.
pub const PRISM_BACKEND_SUPPORTS_SPEAK_TO_MEMORY: u64 = 1 << 3;
/// `prism_backend_braille` is implemented.
pub const PRISM_BACKEND_SUPPORTS_BRAILLE: u64 = 1 << 4;
/// `prism_backend_output` is implemented.
pub const PRISM_BACKEND_SUPPORTS_OUTPUT: u64 = 1 << 5;
/// `prism_backend_is_speaking` is implemented.
pub const PRISM_BACKEND_SUPPORTS_IS_SPEAKING: u64 = 1 << 6;
/// `prism_backend_stop` is implemented.
pub const PRISM_BACKEND_SUPPORTS_STOP: u64 = 1 << 7;
/// `prism_backend_pause` is implemented.
pub const PRISM_BACKEND_SUPPORTS_PAUSE: u64 = 1 << 8;
/// `prism_backend_resume` is implemented.
pub const PRISM_BACKEND_SUPPORTS_RESUME: u64 = 1 << 9;
/// `prism_backend_set_volume` is implemented.
pub const PRISM_BACKEND_SUPPORTS_SET_VOLUME: u64 = 1 << 10;
/// `prism_backend_get_volume` is implemented.
pub const PRISM_BACKEND_SUPPORTS_GET_VOLUME: u64 = 1 << 11;
/// `prism_backend_set_rate` is implemented.
pub const PRISM_BACKEND_SUPPORTS_SET_RATE: u64 = 1 << 12;
/// `prism_backend_get_rate` is implemented.
pub const PRISM_BACKEND_SUPPORTS_GET_RATE: u64 = 1 << 13;
/// `prism_backend_set_pitch` is implemented.
pub const PRISM_BACKEND_SUPPORTS_SET_PITCH: u64 = 1 << 14;
/// `prism_backend_get_pitch` is implemented.
pub const PRISM_BACKEND_SUPPORTS_GET_PITCH: u64 = 1 << 15;
/// `prism_backend_refresh_voices` is implemented.
pub const PRISM_BACKEND_SUPPORTS_REFRESH_VOICES: u64 = 1 << 16;
/// `prism_backend_count_voices` is implemented.
pub const PRISM_BACKEND_SUPPORTS_COUNT_VOICES: u64 = 1 << 17;
/// `prism_backend_get_voice_name` is implemented.
pub const PRISM_BACKEND_SUPPORTS_GET_VOICE_NAME: u64 = 1 << 18;
/// `prism_backend_get_voice_language` is implemented.
pub const PRISM_BACKEND_SUPPORTS_GET_VOICE_LANGUAGE: u64 = 1 << 19;
/// `prism_backend_get_voice` is implemented.
pub const PRISM_BACKEND_SUPPORTS_GET_VOICE: u64 = 1 << 20;
/// `prism_backend_set_voice` is implemented.
pub const PRISM_BACKEND_SUPPORTS_SET_VOICE: u64 = 1 << 21;
/// `prism_backend_get_channels` is implemented.
pub const PRISM_BACKEND_SUPPORTS_GET_CHANNELS: u64 = 1 << 22;
/// `prism_backend_get_sample_rate` is implemented.
pub const PRISM_BACKEND_SUPPORTS_GET_SAMPLE_RATE: u64 = 1 << 23;
/// `prism_backend_get_bit_depth` is implemented.
pub const PRISM_BACKEND_SUPPORTS_GET_BIT_DEPTH: u64 = 1 << 24;
/// Reserved.
pub const PRISM_BACKEND_PERFORMS_SILENCE_TRIMMING_ON_SPEAK: u64 = 1 << 25;
/// The backend trims leading and trailing silence from the audio stream before delivering it to the audio callback.
pub const PRISM_BACKEND_PERFORMS_SILENCE_TRIMMING_ON_SPEAK_TO_MEMORY: u64 = 1 << 26;
/// Reserved.
pub const PRISM_BACKEND_SUPPORTS_SPEAK_SSML: u64 = 1 << 27;
/// Reserved.
pub const PRISM_BACKEND_SUPPORTS_SPEAK_TO_MEMORY_SSML: u64 = 1 << 28;
/// The backend presents text through a screen reader.
pub const PRISM_BACKEND_IS_SCREEN_READER: u64 = 1 << 56;
/// The backend synthesizes speech without a screen reader.
pub const PRISM_BACKEND_IS_SPEECH_ENGINE: u64 = 1 << 57;
/// The backend is never selected by `prism_registry_create_best` or `prism_registry_acquire_best`, and is obtained only by requesting it by identifier or name.
pub const PRISM_BACKEND_REQUIRES_EXPLICIT_SELECTION: u64 = 1 << 58;
/// The highest bit reserved for feature flags.
pub const PRISM_BACKEND_FEATURE_MAX_BIT: u64 = 1 << 63;

/// Invalid/sentinel value (always 0)
pub const PRISM_BACKEND_INVALID: PrismBackendId = 0;
/// Microsoft SAPI (Windows)
pub const PRISM_BACKEND_SAPI: PrismBackendId = 0x1D6D_F724_22CE_EE66;
/// `AVSpeechSynthesizer` (macOS, iOS, tvOS, `WatchOS`, `VisionOS`)
pub const PRISM_BACKEND_AV_SPEECH: PrismBackendId = 0x28E3_4295_7780_5C24;
/// `VoiceOver` screen reader (macOS, `MacCatalyst`, iOS, `WatchOS`, tvOS, `VisionOS`)
pub const PRISM_BACKEND_VOICE_OVER: PrismBackendId = 0xCB48_9796_1A75_4BCB;
/// Speech Dispatcher (Linux/BSD, win32 via Wine)
pub const PRISM_BACKEND_SPEECH_DISPATCHER: PrismBackendId = 0xE3D6_F895_D949_EBFE;
/// NVDA screen reader (Windows)
pub const PRISM_BACKEND_NVDA: PrismBackendId = 0x89CC_19C5_C4AC_1A56;
/// JAWS screen reader (Windows)
pub const PRISM_BACKEND_JAWS: PrismBackendId = 0xAC3D_60E9_BD84_B53E;
/// Windows `OneCore` speech API (Windows 10+)
pub const PRISM_BACKEND_ONE_CORE: PrismBackendId = 0x6797_D32F_0D99_4CB4;
/// Orca screen reader (Linux/BSD, win32 via Wine)
pub const PRISM_BACKEND_ORCA: PrismBackendId = 0x10AA_1FC0_5A17_F96C;
/// Android screen readers (Android)
pub const PRISM_BACKEND_ANDROID_SCREEN_READER: PrismBackendId = 0xD199_C175_AEEC_494B;
/// Android TTS engine (Android)
pub const PRISM_BACKEND_ANDROID_TTS: PrismBackendId = 0xBC17_5831_BFE4_E5CC;
/// Web `SpeechSynthesis` API (web)
pub const PRISM_BACKEND_WEB_SPEECH: PrismBackendId = 0x3572_538D_44D4_4A8F;
/// `UIAutomation` backend (Windows only)
pub const PRISM_BACKEND_UIA: PrismBackendId = 0x6238_F019_DB67_8F8E;
/// Zhengdu Screen Reader (Windows)
pub const PRISM_BACKEND_ZDSR: PrismBackendId = 0x3D93_C56C_9E7F_2A2E;
/// `ZoomText` (Windows)
pub const PRISM_BACKEND_ZOOM_TEXT: PrismBackendId = 0xAE43_9D62_DC7B_1479;
/// `BoyPCReader` (windows only)
pub const PRISM_BACKEND_BOY_PC_READER: PrismBackendId = 0x285A_BA1C_16F3_300F;
/// `PCTalker` (windows only)
pub const PRISM_BACKEND_PC_TALKER: PrismBackendId = 0x344B_9519_62E3_B835;
/// Sense Reader screen reader (Windows)
pub const PRISM_BACKEND_SENSE_READER: PrismBackendId = 0xED47_6089_0B55_C2F2;
/// `SystemAccess` screen reader (windows) (only available if explicitly enabled at build time)
pub const PRISM_BACKEND_SYSTEM_ACCESS: PrismBackendId = 0x8380_F2A3_7B2C_3EB6;
/// `WindowEyes` screen reader (windows) (only available if explicitly enabled at build time)
pub const PRISM_BACKEND_WINDOW_EYES: PrismBackendId = 0x9120_D899_0878_5C13;
/// Spiel (Linux and BSDs only)
pub const PRISM_BACKEND_SPIEL: PrismBackendId = 0x478B_44F1_4AD3_D89C;
/// Microsoft GDK speech synthesis (Windows, Xbox) (only built when the GDK's headers are present)
pub const PRISM_BACKEND_GDK_SPEECH: PrismBackendId = 0xD39D_8723_4941_6B11;
/// Places text on the system clipboard instead of presenting it (only used when requested by identifier or name)
pub const PRISM_BACKEND_CLIPBOARD: PrismBackendId = 0x7056_908D_E8F9_F991;
/// Produces no output, for testing (only used when requested by identifier or name)
pub const PRISM_BACKEND_NO_SPEECH: PrismBackendId = 0xC2C0_EAB0_BF97_A5F0;

/// The `PrismConfig` layout version this binding describes.
pub const PRISM_CONFIG_VERSION: u8 = 4;
/// The plugin ABI generation this binding describes.
pub const PRISM_PLUGIN_ABI_VERSION: u64 = 1;

/// The type of a function invoked when a backend's runtime availability changes.
pub type PrismAvailabilityCallback =
	Option<unsafe extern "C" fn(userdata: *mut c_void, backend: PrismBackendId, name: *const c_char, available: bool)>;

/// The type of a function invoked once the poll thread has established its availability baseline.
pub type PrismAvailabilityBaselineCallback = Option<unsafe extern "C" fn(userdata: *mut c_void)>;

/// Receives audio samples from `prism_backend_speak_to_memory`.
pub type PrismAudioCallback = Option<
	unsafe extern "C" fn(
		userdata: *mut c_void,
		samples: *const f32,
		sample_count: usize,
		channels: usize,
		sample_rate: usize,
	),
>;

/// The type of a function invoked to deliver a single log message.
pub type PrismLogCallback = Option<
	unsafe extern "C" fn(userdata: *mut c_void, level: PrismLogLevel, source: *const c_char, message: *const c_char),
>;

/// A struct containing configuration parameters for Prism or it's back-ends to use.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PrismConfig {
	/// The version of this structure. This field MUST NOT be modified.
	pub version: u8,
	/// The registry the created context will be bound to. This MAY be `NULL`, in which case the context uses the global registry. If non-null, it MUST be a registry obtained from `prism_registry_freeze`. This field was added in version 3 of this structure.
	pub registry: *mut PrismRegistry,
	/// A function invoked when a backend's runtime availability changes, or `NULL`. When this field is `NULL`, the context performs no background availability polling and creates no poll thread. When it is non-null, the context runs an internal thread that samples backend availability and invokes this callback on each confirmed transition. The behavior of this callback and the polling model are described in the chapter on background availability enumeration. This field was added in version 3 of this structure.
	pub availability_callback: PrismAvailabilityCallback,
	/// An opaque pointer passed unmodified to `availability_callback` on each invocation. Prism does not interpret or take ownership of this value. It is ignored when `availability_callback` is `NULL`. This field was added in version 3 of this structure.
	pub availability_userdata: *mut c_void,
	/// The base interval, in milliseconds, between availability scans. A value of `0` selects the default of 1000 milliseconds. It is ignored when `availability_callback` is `NULL`. This field was added in version 3 of this structure.
	pub availability_poll_interval_ms: u32,
	/// The number of consecutive agreeing samples required before a change in a backend's availability is confirmed and reported. A value of `0` selects the default of 2. A value of `1` confirms every observed change immediately, without debouncing. It is ignored when `availability_callback` is `NULL`. This field was added in version 3 of this structure.
	pub availability_debounce_samples: u32,
	/// The upper bound, in milliseconds, for adaptive backoff of the sampling interval. While availability is unchanging, the interval doubles from `availability_poll_interval_ms` toward this bound, and returns to the base interval as soon as any change is observed. A value of `0`, or any value not greater than the base interval, disables backoff and holds the interval constant. It is ignored when `availability_callback` is `NULL`. This field was added in version 3 of this structure.
	pub availability_backoff_max_ms: u32,
	/// When `true`, and when the library was built with power-management support, the poll thread is paused automatically when the operating system suspends and resumed when it wakes. When `false`, or on builds and platforms without power-management support, this field has no effect and the application MAY drive pausing itself. Use `prism_availability_auto_power_supported` to determine whether this field is honored. It is ignored when `availability_callback` is `NULL`. This field was added in version 3 of this structure.
	pub availability_auto_power_manage: bool,
	/// A function invoked exactly once per context, on the poll thread, after the first availability scan has completed and before the first invocation of `availability_callback`, or `NULL`. It receives `availability_userdata`. It is ignored when `availability_callback` is `NULL`. This field was added in version 4 of this structure.
	pub availability_baseline_callback: PrismAvailabilityBaselineCallback,
}

/// A table of function pointers implementing a custom backend.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PrismBackendVTable {
	/// The size of this structure as known to the application. This member MUST be set to `sizeof(PrismBackendVTable)`. Prism reads at most `size` bytes from the structure. If `size` is smaller than the size of the structure as this version of Prism defines it, the members beyond `size` are treated as null; if it is larger, the additional bytes are ignored. This scheme permits the structure to grow in later library versions without invalidating applications compiled against earlier ones.
	pub size: usize,
	/// An optional function producing a per-instance state pointer. If non-null, Prism invokes it exactly once for each backend instance constructed from the registration, passing the registration's `userdata`, and thereafter passes the returned pointer as the `instance` argument to every other member invoked for that instance. Should `create` return `NULL`, construction of the instance fails. If `create` is null, the registration's `userdata` pointer is passed as the `instance` argument directly, and all instances of the backend consequently share it.
	pub create: Option<unsafe extern "C" fn(userdata: *mut c_void) -> *mut c_void>,
	/// An optional function releasing a state pointer previously returned by `create`. If both `create` and `destroy` are non-null, Prism invokes `destroy` exactly once for each backend instance, at the time the instance is freed. `destroy` is never invoked if `create` is null.
	pub destroy: Option<unsafe extern "C" fn(instance: *mut c_void)>,
	/// An optional runtime availability probe. If non-null, Prism invokes it to determine the `PRISM_BACKEND_IS_SUPPORTED_AT_RUNTIME` bit reported by `prism_backend_get_features`; if null, the bit declared at registration is reported unchanged. Because `prism_backend_get_features` MAY be called before initialization, `is_supported` MAY be invoked before `initialize` has succeeded, and an implementation of it MUST NOT assume the instance has been initialized. This member designates no operation and is therefore exempt from the feature consistency requirement of `prism_registry_builder_add_backend`.
	pub is_supported: Option<unsafe extern "C" fn(instance: *mut c_void) -> bool>,
	/// Implements `prism_backend_initialize`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub initialize: Option<unsafe extern "C" fn(instance: *mut c_void) -> PrismError>,
	/// Implements `prism_backend_speak`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub speak: Option<unsafe extern "C" fn(instance: *mut c_void, text: *const c_char, interrupt: bool) -> PrismError>,
	/// Implements `prism_backend_speak_to_memory`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub speak_to_memory: Option<
		unsafe extern "C" fn(
			instance: *mut c_void,
			text: *const c_char,
			callback: PrismAudioCallback,
			callback_userdata: *mut c_void,
		) -> PrismError,
	>,
	/// Implements `prism_backend_braille`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub braille: Option<unsafe extern "C" fn(instance: *mut c_void, text: *const c_char) -> PrismError>,
	/// Implements `prism_backend_output`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub output: Option<unsafe extern "C" fn(instance: *mut c_void, text: *const c_char, interrupt: bool) -> PrismError>,
	/// Implements `prism_backend_stop`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub stop: Option<unsafe extern "C" fn(instance: *mut c_void) -> PrismError>,
	/// Implements `prism_backend_pause`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub pause: Option<unsafe extern "C" fn(instance: *mut c_void) -> PrismError>,
	/// Implements `prism_backend_resume`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub resume: Option<unsafe extern "C" fn(instance: *mut c_void) -> PrismError>,
	/// Implements `prism_backend_is_speaking`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub is_speaking: Option<unsafe extern "C" fn(instance: *mut c_void, out_speaking: *mut bool) -> PrismError>,
	/// Implements `prism_backend_set_volume`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub set_volume: Option<unsafe extern "C" fn(instance: *mut c_void, volume: f32) -> PrismError>,
	/// Implements `prism_backend_get_volume`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub get_volume: Option<unsafe extern "C" fn(instance: *mut c_void, out_volume: *mut f32) -> PrismError>,
	/// Implements `prism_backend_set_rate`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub set_rate: Option<unsafe extern "C" fn(instance: *mut c_void, rate: f32) -> PrismError>,
	/// Implements `prism_backend_get_rate`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub get_rate: Option<unsafe extern "C" fn(instance: *mut c_void, out_rate: *mut f32) -> PrismError>,
	/// Implements `prism_backend_set_pitch`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub set_pitch: Option<unsafe extern "C" fn(instance: *mut c_void, pitch: f32) -> PrismError>,
	/// Implements `prism_backend_get_pitch`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub get_pitch: Option<unsafe extern "C" fn(instance: *mut c_void, out_pitch: *mut f32) -> PrismError>,
	/// Implements `prism_backend_refresh_voices`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub refresh_voices: Option<unsafe extern "C" fn(instance: *mut c_void) -> PrismError>,
	/// Implements `prism_backend_count_voices`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub count_voices: Option<unsafe extern "C" fn(instance: *mut c_void, out_count: *mut usize) -> PrismError>,
	/// Implements `prism_backend_get_voice_name`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub get_voice_name: Option<
		unsafe extern "C" fn(instance: *mut c_void, voice_id: usize, out_name: *mut *const c_char) -> PrismError,
	>,
	/// Implements `prism_backend_get_voice_language`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub get_voice_language: Option<
		unsafe extern "C" fn(instance: *mut c_void, voice_id: usize, out_language: *mut *const c_char) -> PrismError,
	>,
	/// Implements `prism_backend_set_voice`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub set_voice: Option<unsafe extern "C" fn(instance: *mut c_void, voice_id: usize) -> PrismError>,
	/// Implements `prism_backend_get_voice`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub get_voice: Option<unsafe extern "C" fn(instance: *mut c_void, out_voice_id: *mut usize) -> PrismError>,
	/// Implements `prism_backend_get_channels`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub get_channels: Option<unsafe extern "C" fn(instance: *mut c_void, out_channels: *mut usize) -> PrismError>,
	/// Implements `prism_backend_get_sample_rate`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub get_sample_rate: Option<unsafe extern "C" fn(instance: *mut c_void, out_sample_rate: *mut usize) -> PrismError>,
	/// Implements `prism_backend_get_bit_depth`, taking the instance pointer in place of the backend. Null means the operation is unimplemented.
	pub get_bit_depth: Option<unsafe extern "C" fn(instance: *mut c_void, out_bit_depth: *mut usize) -> PrismError>,
}

/// A structure pairing a log callback with an opaque user pointer.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct PrismLogHandler {
	/// The callback invoked to deliver messages, or `NULL` to install no handler. When `fn` is `NULL`, messages are discarded.
	pub fn_: PrismLogCallback,
	/// An opaque pointer passed unmodified to `fn` on each invocation. Prism does not interpret or take ownership of this value. The lifetime of this value is the lifetime of the handler function, and therefore this value must be valid for as long as the handler is alive.
	pub userdata: *mut c_void,
}

/// A structure through which a backend supplied by a plugin reaches the facilities Prism makes available to it. Prism provides one to each backend it registers from a plugin, as described under Host services.
#[repr(C)]
#[derive(Debug)]
pub struct PrismPluginServices {
	/// The size of this structure as Prism understands it. A backend MUST consult at most this many bytes and MUST treat any member beyond it as absent. A later generation MAY append members after those defined here.
	pub struct_size: u32,
	/// Reserved for future use. Prism sets this member to zero, and a backend MUST ignore it.
	pub reserved: u32,
	/// A function through which the backend records diagnostics. Its first argument MUST be the services object Prism supplied. Prism records the diagnostic under the name of the backend to which the services object belongs; the backend does not supply a source. This member is never `NULL`.
	pub log: Option<unsafe extern "C" fn(self_: *const Self, level: PrismLogLevel, message: *const c_char)>,
}

/// The structure Prism passes as the `userdata` argument to the `create` member of a backend supplied by a plugin. The context is valid only for the duration of the `create` call; the services object it names is valid for the longer period given under Host services.
#[repr(C)]
#[derive(Debug)]
pub struct PrismPluginInstanceContext {
	/// The size of this structure as Prism understands it. A backend MUST consult at most this many bytes and MUST treat any member beyond it as absent.
	pub struct_size: u32,
	/// Reserved for future use. Prism sets this member to zero, and a backend MUST ignore it.
	pub reserved: u32,
	/// The backend's services object. A backend MAY retain it for the period given under Host services. This member is never `NULL`.
	pub services: *const PrismPluginServices,
	/// The value of the `userdata` member of the descriptor from which the backend was registered.
	pub userdata: *mut c_void,
}

/// A structure describing the loading Prism library, passed to a plugin's entry point.
#[repr(C)]
#[derive(Debug)]
pub struct PrismPluginHost {
	/// The plugin ABI generation this implementation provides, equal to the `PRISM_PLUGIN_ABI_VERSION` against which Prism was compiled. A plugin MAY compare this value against its own requirements and decline to supply backends if it requires a newer host.
	pub abi_version: u64,
	/// The size of this structure as Prism understands it. A plugin MUST consult at most this many bytes and MUST treat any member beyond it as absent.
	pub struct_size: u32,
	/// Reserved for future use. Prism sets this member to zero, and a plugin MUST ignore it.
	pub reserved: u32,
	/// A function through which the plugin MAY record diagnostics during the entry point call. Its first argument MUST be the host descriptor Prism supplied; the plugin does not supply a source, and Prism records the diagnostic under a source that names the library being loaded. This member is never `NULL`. A plugin SHOULD use it to state the reason for declining a host, and MUST NOT retain the pointer beyond the entry point call.
	pub log: Option<unsafe extern "C" fn(self_: *const Self, level: PrismLogLevel, message: *const c_char)>,
}

/// A descriptor supplied by a plugin to describe a single backend.
#[repr(C)]
#[derive(Debug)]
pub struct PrismPluginBackend {
	/// The plugin ABI generation this descriptor was built against. A plugin MUST set this member to the `PRISM_PLUGIN_ABI_VERSION` against which it was compiled. Prism rejects a descriptor whose generation it does not accept, as described under ABI compatibility.
	pub abi_version: u64,
	/// The size of this structure as the plugin understands it. A plugin MUST set this member to `sizeof(PrismPluginBackend)`. Prism consults at most this many bytes.
	pub struct_size: u32,
	/// Reserved for future use. A plugin MUST set this member to zero.
	pub reserved: u32,
	/// The backend's name, as a null-terminated UTF-8 string, subject to the same requirements and consequences as the `name` parameter of `prism_registry_builder_add_backend`. The backend's identifier is derived from it by the hash function described in the chapter on backend identifiers. The string is copied during loading.
	pub name: *const c_char,
	/// The backend's priority. Higher values indicate higher priority. This value MUST be non-negative unless the loading call supplies a priority override, in which case it is ignored.
	pub priority: c_int,
	/// The feature set the backend declares, formed by `ORing` `PRISM_BACKEND_*` feature constants together. It MUST be consistent with the vtable in the sense required of any custom backend.
	pub features: u64,
	/// The vtable implementing the backend, subject to every requirement placed on a vtable by the chapter on custom backends. This member MUST NOT be `NULL`, and its `size` member MUST be set as that chapter requires. The vtable MUST provide a `create` member, as described under Host services. The vtable is copied during loading; the code it names remains valid while the library remains loaded.
	pub vtable: *const PrismBackendVTable,
	/// An opaque pointer that Prism delivers to the backend's `create` member as the `userdata` member of a `PrismPluginInstanceContext` (see `PrismPluginInstanceContext`). It has the meaning the `userdata` parameter of `prism_registry_builder_add_backend` would have, and permits several descriptors that share one vtable to be distinguished at instance creation. This member MAY be `NULL`. Prism does not free it.
	pub userdata: *mut c_void,
	/// An informational version identifying the plugin's own release, distinct from the ABI generation and used for no compatibility decision. The value is three 16-bit components, major, minor, and patch, packed most-significant first, with the least-significant 16 bits reserved and set to zero. A plugin MAY set it to zero if it has no version to report.
	pub plugin_version: u64,
}

/// The entry point a prism plugin shared library exports.
pub type PrismPluginQueryFn =
	Option<unsafe extern "C" fn(host: *const PrismPluginHost, index: usize) -> *const PrismPluginBackend>;

unsafe extern "C" {
	/// Creates a new configuration structure which can be passed to `prism_init`.
	///
	/// # Safety
	///
	/// Safe to call at any time; this is `unsafe` only because it crosses the FFI boundary.
	pub fn prism_config_init() -> PrismConfig;
	/// Creates a new Prism context.
	///
	/// # Safety
	///
	/// `cfg` must be null, or point to a writable, initialized `PrismConfig`.
	pub fn prism_init(cfg: *mut PrismConfig) -> *mut PrismContext;
	/// Destroys a Prism context and releases associated resources.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	/// The context is invalid once this returns.
	pub fn prism_shutdown(ctx: *mut PrismContext);
	/// Pauses the availability poll thread.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	pub fn prism_availability_poll_pause(ctx: *mut PrismContext);
	/// Resumes the availability poll thread.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	pub fn prism_availability_poll_resume(ctx: *mut PrismContext);
	/// Reports whether this build can pause and resume polling automatically in response to operating-system power transitions.
	///
	/// # Safety
	///
	/// Safe to call at any time; this is `unsafe` only because it crosses the FFI boundary.
	pub fn prism_availability_auto_power_supported() -> bool;
	/// Returns the number of backends registered in the registry.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	pub fn prism_registry_count(ctx: *mut PrismContext) -> usize;
	/// Returns the backend ID at the specified index in the registry.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	pub fn prism_registry_id_at(ctx: *mut PrismContext, index: usize) -> PrismBackendId;
	/// Looks up a backend by name and returns its ID.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	/// `name` must be a non-null, NUL-terminated UTF-8 string valid for the call.
	pub fn prism_registry_id(ctx: *mut PrismContext, name: *const c_char) -> PrismBackendId;
	/// Returns the human-readable name of a backend given its ID.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	pub fn prism_registry_name(ctx: *mut PrismContext, id: PrismBackendId) -> *const c_char;
	/// Returns the priority value of a backend.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	pub fn prism_registry_priority(ctx: *mut PrismContext, id: PrismBackendId) -> c_int;
	/// Checks whether a backend with the given ID exists in the registry.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	pub fn prism_registry_exists(ctx: *mut PrismContext, id: PrismBackendId) -> bool;
	/// Retrieves a cached backend instance if one exists.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	pub fn prism_registry_get(ctx: *mut PrismContext, id: PrismBackendId) -> *mut PrismBackend;
	/// Creates a new backend instance.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	pub fn prism_registry_create(ctx: *mut PrismContext, id: PrismBackendId) -> *mut PrismBackend;
	/// Creates a new instance of the highest-priority backend that successfully initializes.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	pub fn prism_registry_create_best(ctx: *mut PrismContext) -> *mut PrismBackend;
	/// Acquires a backend instance, reusing a cached instance if available or creating a new one otherwise.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	pub fn prism_registry_acquire(ctx: *mut PrismContext, id: PrismBackendId) -> *mut PrismBackend;
	/// Acquires the highest-priority backend that successfully initializes, reusing a cached instance if available.
	///
	/// # Safety
	///
	/// `ctx` must be a live context from `prism_init` that has not been passed to `prism_shutdown`.
	pub fn prism_registry_acquire_best(ctx: *mut PrismContext) -> *mut PrismBackend;
	/// Releases a backend instance.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// The pointer is invalid once this returns.
	pub fn prism_backend_free(backend: *mut PrismBackend);
	/// Returns the human-readable name of a backend.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	pub fn prism_backend_name(backend: *mut PrismBackend) -> *const c_char;
	/// Returns a bitmask of all features supported by this backend, as well as other information.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	pub fn prism_backend_get_features(backend: *mut PrismBackend) -> u64;
	/// Initializes a backend instance.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	pub fn prism_backend_initialize(backend: *mut PrismBackend) -> PrismError;
	/// Synthesizes speech from the given text and plays it through the default audio output.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `text` must be a non-null, NUL-terminated UTF-8 string valid for the call.
	pub fn prism_backend_speak(backend: *mut PrismBackend, text: *const c_char, interrupt: bool) -> PrismError;
	/// Synthesizes speech from the given text and delivers the audio samples to a callback function.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `text` must be a non-null, NUL-terminated UTF-8 string valid for the call.
	/// `userdata` is passed through untouched; it must satisfy whatever the paired callback expects and stay alive as long as that callback can run.
	pub fn prism_backend_speak_to_memory(
		backend: *mut PrismBackend,
		text: *const c_char,
		callback: PrismAudioCallback,
		userdata: *mut c_void,
	) -> PrismError;
	/// Outputs text to a connected braille display.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `text` must be a non-null, NUL-terminated UTF-8 string valid for the call.
	pub fn prism_backend_braille(backend: *mut PrismBackend, text: *const c_char) -> PrismError;
	/// Outputs text using all available modalities supported by the backend.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `text` must be a non-null, NUL-terminated UTF-8 string valid for the call.
	pub fn prism_backend_output(backend: *mut PrismBackend, text: *const c_char, interrupt: bool) -> PrismError;
	/// Immediately stops any currently playing speech.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	pub fn prism_backend_stop(backend: *mut PrismBackend) -> PrismError;
	/// Pauses currently playing speech.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	pub fn prism_backend_pause(backend: *mut PrismBackend) -> PrismError;
	/// Resumes previously paused speech.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	pub fn prism_backend_resume(backend: *mut PrismBackend) -> PrismError;
	/// Queries whether the backend is currently producing speech output.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `out_speaking` must be null, or a writable pointer to a `bool`.
	pub fn prism_backend_is_speaking(backend: *mut PrismBackend, out_speaking: *mut bool) -> PrismError;
	/// Sets the speech volume.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	pub fn prism_backend_set_volume(backend: *mut PrismBackend, volume: f32) -> PrismError;
	/// Retrieves the current speech volume.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `out_volume` must be null, or a writable pointer to a `f32`.
	pub fn prism_backend_get_volume(backend: *mut PrismBackend, out_volume: *mut f32) -> PrismError;
	/// Sets the speech rate (speed).
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	pub fn prism_backend_set_rate(backend: *mut PrismBackend, rate: f32) -> PrismError;
	/// Retrieves the current speech rate.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `out_rate` must be null, or a writable pointer to a `f32`.
	pub fn prism_backend_get_rate(backend: *mut PrismBackend, out_rate: *mut f32) -> PrismError;
	/// Sets the speech pitch.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	pub fn prism_backend_set_pitch(backend: *mut PrismBackend, pitch: f32) -> PrismError;
	/// Retrieves the current speech pitch.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `out_pitch` must be null, or a writable pointer to a `f32`.
	pub fn prism_backend_get_pitch(backend: *mut PrismBackend, out_pitch: *mut f32) -> PrismError;
	/// Refreshes the backend's internal voice list.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	pub fn prism_backend_refresh_voices(backend: *mut PrismBackend) -> PrismError;
	/// Returns the number of voices available from the backend.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `out_count` must be null, or a writable pointer to a `usize`.
	pub fn prism_backend_count_voices(backend: *mut PrismBackend, out_count: *mut usize) -> PrismError;
	/// Retrieves the human-readable name of a voice.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `out_name` must be null, or a writable pointer to a `*const c_char`.
	pub fn prism_backend_get_voice_name(
		backend: *mut PrismBackend,
		voice_id: usize,
		out_name: *mut *const c_char,
	) -> PrismError;
	/// Retrieves the language code or language string of a voice.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `out_language` must be null, or a writable pointer to a `*const c_char`.
	pub fn prism_backend_get_voice_language(
		backend: *mut PrismBackend,
		voice_id: usize,
		out_language: *mut *const c_char,
	) -> PrismError;
	/// Selects a voice to use for subsequent speech synthesis.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	pub fn prism_backend_set_voice(backend: *mut PrismBackend, voice_id: usize) -> PrismError;
	/// Retrieves the index of the currently selected voice.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `out_voice_id` must be null, or a writable pointer to a `usize`.
	pub fn prism_backend_get_voice(backend: *mut PrismBackend, out_voice_id: *mut usize) -> PrismError;
	/// Retrieves the number of audio channels produced by the backend.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `out_channels` must be null, or a writable pointer to a `usize`.
	pub fn prism_backend_get_channels(backend: *mut PrismBackend, out_channels: *mut usize) -> PrismError;
	/// Retrieves the sample rate of audio produced by the backend.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `out_sample_rate` must be null, or a writable pointer to a `usize`.
	pub fn prism_backend_get_sample_rate(backend: *mut PrismBackend, out_sample_rate: *mut usize) -> PrismError;
	/// Retrieves the native bit depth of audio produced by the backend.
	///
	/// # Safety
	///
	/// `backend` must be a live backend that has not been passed to `prism_backend_free`.
	/// `out_bit_depth` must be null, or a writable pointer to a `usize`.
	pub fn prism_backend_get_bit_depth(backend: *mut PrismBackend, out_bit_depth: *mut usize) -> PrismError;
	/// Returns a human-readable description of an error code.
	///
	/// # Safety
	///
	/// Safe to call at any time; this is `unsafe` only because it crosses the FFI boundary.
	pub fn prism_error_string(error: PrismError) -> *const c_char;
	/// Creates a new registry builder seeded with the compiled-in backends.
	///
	/// # Safety
	///
	/// Safe to call at any time; this is `unsafe` only because it crosses the FFI boundary.
	pub fn prism_registry_builder_new() -> *mut PrismRegistryBuilder;
	/// Adds a custom backend to a registry builder.
	///
	/// # Safety
	///
	/// `builder` must be a live builder that has not been freed.
	/// `vtable` must point to an initialized `PrismBackendVTable` whose `size` member is set, valid for the call.
	/// `name` must be a non-null, NUL-terminated UTF-8 string valid for the call.
	/// `out_id` must be null, or a writable pointer to a `PrismBackendId`.
	/// `userdata` is passed through untouched; it must satisfy whatever the paired callback expects and stay alive as long as that callback can run.
	pub fn prism_registry_builder_add_backend(
		builder: *mut PrismRegistryBuilder,
		name: *const c_char,
		priority: c_int,
		features: u64,
		vtable: *const PrismBackendVTable,
		userdata: *mut c_void,
		userdata_free: Option<unsafe extern "C" fn(*mut c_void)>,
		out_id: *mut PrismBackendId,
	) -> PrismError;
	/// Loads a plugin from a shared library and adds each backend it supplies to a registry builder.
	///
	/// # Safety
	///
	/// `builder` must be a live builder that has not been freed.
	/// `path` must be a non-null, NUL-terminated UTF-8 string valid for the call.
	/// `out_count` must be null, or a writable pointer to a `usize`.
	pub fn prism_registry_builder_add_library(
		builder: *mut PrismRegistryBuilder,
		path: *const c_char,
		priority_override: c_int,
		out_count: *mut usize,
	) -> PrismError;
	/// Freezes a builder, producing an immutable registry.
	///
	/// # Safety
	///
	/// `builder` must be a live builder that has not been freed.
	pub fn prism_registry_freeze(builder: *mut PrismRegistryBuilder) -> *mut PrismRegistry;
	/// Releases a registry builder.
	///
	/// # Safety
	///
	/// `builder` must be a live builder that has not been freed.
	/// The builder is invalid once this returns.
	pub fn prism_registry_builder_free(builder: *mut PrismRegistryBuilder);
	/// Increments the reference count of a registry.
	///
	/// # Safety
	///
	/// `registry` must be null, or a live registry the caller holds a reference to.
	pub fn prism_registry_retain(registry: *mut PrismRegistry) -> *mut PrismRegistry;
	/// Decrements the reference count of a registry, finalizing it when the count reaches zero.
	///
	/// # Safety
	///
	/// `registry` must be null, or a live registry the caller holds a reference to.
	/// The caller must not release more references than it holds.
	pub fn prism_registry_release(registry: *mut PrismRegistry);
	/// Installs the handler that receives log messages, replacing any previously installed handler.
	///
	/// # Safety
	///
	/// `handler.fn_`, if set, must be safe to call with `handler.userdata` from prism's logging thread, and that userdata must outlive the handler.
	pub fn prism_set_log_handler(handler: PrismLogHandler) -> PrismLogHandler;
	/// Sets the minimum severity of messages that will be delivered.
	///
	/// # Safety
	///
	/// Safe to call at any time; this is `unsafe` only because it crosses the FFI boundary.
	pub fn prism_set_log_level(level: PrismLogLevel) -> PrismLogLevel;
	/// Emits a log message.
	///
	/// # Safety
	///
	/// `source` must be a non-null, NUL-terminated UTF-8 string valid for the call.
	/// `message` must be a non-null, NUL-terminated UTF-8 string valid for the call.
	pub fn prism_log(level: PrismLogLevel, source: *const c_char, message: *const c_char);
	/// Blocks until all messages queued before the call have been delivered.
	///
	/// # Safety
	///
	/// Safe to call at any time; this is `unsafe` only because it crosses the FFI boundary.
	pub fn prism_log_flush();
	/// Stops the logging thread and releases the resources associated with the logger.
	///
	/// # Safety
	///
	/// Safe to call at any time; this is `unsafe` only because it crosses the FFI boundary.
	pub fn prism_log_shutdown();
	/// Returns the version of the loaded Prism library as an encoded integer.
	///
	/// # Safety
	///
	/// Safe to call at any time; this is `unsafe` only because it crosses the FFI boundary.
	pub fn prism_version() -> u32;
	/// Returns the version of the loaded Prism library as a human-readable string.
	///
	/// # Safety
	///
	/// Safe to call at any time; this is `unsafe` only because it crosses the FFI boundary.
	pub fn prism_version_string() -> *const c_char;
}
