//! Game-controller support, isolated in one module (port of
//! `freight_fate/controller.py`).
//!
//! Freight Fate is keyboard- and screen-reader-first; this adds an *optional*
//! second input device that never displaces the keyboard. It uses SDL's
//! GameController API, which maps any recognized pad onto the Xbox button
//! layout, so friendly names (A/B/X/Y, bumpers, D-pad) are the same
//! regardless of the physical controller.
//!
//! Design notes:
//!
//! * Event-driven. Buttons and hot-plug arrive as SDL `CONTROLLER*` events.
//!   The only continuous reads are the analog axes (steering, throttle,
//!   brake, clutch): those are cached from axis events and smoothed on
//!   [`ControllerManager::tick`], so the driving loop reads cached state
//!   rather than polling the device.
//! * Headless-safe. If the controller subsystem or a device is unavailable
//!   (CI, dummy SDL drivers), the manager degrades to "no controller" without
//!   raising.
//! * One active controller. Several may be connected; we bind the first and,
//!   if it is unplugged, raise a disconnect signal the app uses to pause and
//!   speak.
//!
//! # Shape of the port
//!
//! The manager never touches SDL directly: the device sits behind
//! [`PadDevice`] and the subsystem behind [`PadSubsystem`], both small
//! traits. `controller::sdl` implements them over `sdl2::controller`;
//! `controller::fakes` implements them for tests, which is how the whole
//! manager -- deadzones, smoothing, repeat, the hot-plug bookkeeping -- runs
//! without a pad or a window (the Python tests set `_controller = object()`).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use ff_core::input_hints::{control_hint, CONTROLLER, KEYBOARD, TOUCH};

/// The device hints name before the player touches anything: the screen on
/// iPhone and iPad, the keyboard everywhere else.
const DEFAULT_DEVICE: &str = if cfg!(target_os = "ios") {
    TOUCH
} else {
    KEYBOARD
};
use ff_core::rumble::{RumbleEngine, RumbleSink};

use crate::states::base::InputEvent;

pub mod diagnostics;
pub mod fakes;
pub mod sdl;

// Deadzones from the design spec: 10% for the sticks, 4% for the triggers.
pub const STICK_DEADZONE: f64 = 0.10;
pub const TRIGGER_DEADZONE: f64 = 0.04;
pub const AXIS_MAX: f64 = 32767.0;

// Held D-pad left/right auto-repeat for adjusting menu options.
pub const REPEAT_DELAY_S: f64 = 0.30; // wait before the first repeat
pub const REPEAT_FAST_S: f64 = 0.03; // fastest repeat once fully accelerated
pub const REPEAT_RAMP_S: f64 = 1.50; // time held over which it accelerates

// Minimal analog smoothing: triggers and clutch reach their target quickly
// but not instantly, matching how the keyboard ramps feel today.
pub const SMOOTH_RATE: f64 = 18.0;

// Controls SDL surfaces on a fully mapped pad; a reconnect that drops these
// is the signature of the "dead triggers/shoulders" fault, so we check them.
pub const ESSENTIAL_MAPPINGS: [&str; 4] = [
    "lefttrigger",
    "righttrigger",
    "leftshoulder",
    "rightshoulder",
];

/// `SDL_GameControllerButton`, the buttons any recognized pad is mapped onto.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControllerButton {
    A,
    B,
    X,
    Y,
    Back,
    Guide,
    Start,
    LeftStick,
    RightStick,
    LeftShoulder,
    RightShoulder,
    DPadUp,
    DPadDown,
    DPadLeft,
    DPadRight,
    Misc1,
    Paddle1,
    Paddle2,
    Paddle3,
    Paddle4,
    Touchpad,
}

/// `SDL_GameControllerAxis`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControllerAxis {
    LeftX,
    LeftY,
    RightX,
    RightY,
    TriggerLeft,
    TriggerRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ControllerAction {
    MenuUp,
    MenuDown,
    AdjustLeft,
    AdjustRight,
    Confirm,
    Back,
    Help,
}

/// `_BUTTON_LABELS`: the spoken name of a button, "a button" for the rest.
pub fn button_label(button: ControllerButton) -> &'static str {
    match button {
        ControllerButton::A => "A button",
        ControllerButton::B => "B button",
        ControllerButton::X => "X button",
        ControllerButton::Y => "Y button",
        ControllerButton::LeftShoulder => "left bumper",
        ControllerButton::RightShoulder => "right bumper",
        ControllerButton::LeftStick => "left stick click",
        ControllerButton::RightStick => "right stick click",
        ControllerButton::DPadUp => "D-pad up",
        ControllerButton::DPadDown => "D-pad down",
        ControllerButton::DPadLeft => "D-pad left",
        ControllerButton::DPadRight => "D-pad right",
        ControllerButton::Back => "Back button",
        ControllerButton::Start => "Start button",
        _ => "a button",
    }
}

/// `_MENU_ACTIONS`: the semantic menu action of a button.
pub fn menu_action_for(button: ControllerButton) -> Option<ControllerAction> {
    match button {
        ControllerButton::DPadUp => Some(ControllerAction::MenuUp),
        ControllerButton::DPadDown => Some(ControllerAction::MenuDown),
        ControllerButton::DPadLeft => Some(ControllerAction::AdjustLeft),
        ControllerButton::DPadRight => Some(ControllerAction::AdjustRight),
        ControllerButton::A => Some(ControllerAction::Confirm),
        ControllerButton::B => Some(ControllerAction::Back),
        ControllerButton::Back => Some(ControllerAction::Help),
        _ => None,
    }
}

/// Rescale a normalized axis so it is 0 inside the deadzone and reaches
/// full range at the edge, avoiding a sudden jump just past the threshold.
pub fn deadzone(value: f64, dead: f64) -> f64 {
    let magnitude = value.abs();
    if magnitude <= dead {
        return 0.0;
    }
    let scaled = (magnitude - dead) / (1.0 - dead);
    if value >= 0.0 {
        scaled
    } else {
        -scaled
    }
}

/// `ControllerManager._smooth`.
pub fn smooth(current: f64, target: f64, dt: f64) -> f64 {
    current + (target - current) * (SMOOTH_RATE * dt).min(1.0)
}

/// The held D-pad left/right auto-repeat clock, on its own so the timing is
/// testable without a manager.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct RepeatTimer {
    button: Option<ControllerButton>,
    countdown: f64,
    held: f64,
}

impl RepeatTimer {
    pub fn button(&self) -> Option<ControllerButton> {
        self.button
    }

    /// A D-pad left/right press starts the clock; other buttons are ignored.
    pub fn press(&mut self, button: ControllerButton) {
        if matches!(
            button,
            ControllerButton::DPadLeft | ControllerButton::DPadRight
        ) {
            self.button = Some(button);
            self.countdown = REPEAT_DELAY_S;
            self.held = 0.0;
        }
    }

    pub fn release(&mut self, button: ControllerButton) {
        if self.button == Some(button) {
            self.button = None;
        }
    }

    pub fn clear(&mut self) {
        self.button = None;
    }

    /// Advance `dt`; how many repeats fire this frame (the interval shrinks
    /// from `REPEAT_DELAY_S` to `REPEAT_FAST_S` over `REPEAT_RAMP_S` held).
    pub fn tick(&mut self, dt: f64) -> u32 {
        if self.button.is_none() {
            return 0;
        }
        self.held += dt;
        self.countdown -= dt;
        let mut count = 0;
        while self.countdown <= 0.0 {
            count += 1;
            let fraction = (self.held / REPEAT_RAMP_S).min(1.0);
            let interval = REPEAT_DELAY_S - (REPEAT_DELAY_S - REPEAT_FAST_S) * fraction;
            self.countdown += interval;
        }
        count
    }
}

// -- the device seam -------------------------------------------------------------

/// One opened game controller (`pygame._sdl2.controller.Controller`).
pub trait PadDevice {
    /// `Controller.id` right after open -- possibly stale after a hot-plug.
    fn instance_id(&self) -> u32;
    /// Whether the pad is still physically present. The SDL implementation
    /// asks the device; a stub answers `true` so a spurious ADDED event does
    /// not drop a working binding.
    fn attached(&self) -> bool {
        true
    }
    /// Drive both motors (0..65535) for `duration_ms`.
    fn rumble(&mut self, low: u16, high: u16, duration_ms: u32);
    fn stop_rumble(&mut self);
    /// The SDL mapping string, when the device can report it.
    fn mapping(&self) -> Option<String> {
        None
    }
}

/// The controller subsystem (`pygame._sdl2.controller` module).
pub trait PadSubsystem {
    fn count(&self) -> u32;
    fn is_controller(&self, index: u32) -> bool;
    fn name_for_index(&self, index: u32) -> Option<String>;
    fn open(&self, index: u32) -> Option<Box<dyn PadDevice>>;
}

/// Brings the subsystem up on demand; `None` when it is unavailable.
pub type PadSubsystemFactory = Box<dyn FnMut() -> Option<Box<dyn PadSubsystem>>>;

/// The bound pad and the two switches every device call is guarded by,
/// shared between the manager and the rumble sink.
/// Set (non-empty) to keep every gamepad out of the game for this process,
/// whatever the controller setting says. The agent server sets it: an
/// agent's capabilities are a player's KEYS, and the Xbox pad resting on
/// the operator's desk spent a whole afternoon toggling the parking brake,
/// stepping the cruise target and reading the speed into an agent's drive
/// (2026-09-01) before anyone thought of it.
pub const NO_CONTROLLER_ENV: &str = "FREIGHT_FATE_NO_CONTROLLER";

/// Whether pads may drive, from the player's setting and the override.
pub fn pads_allowed(setting_enabled: bool, override_value: Option<&std::ffi::OsStr>) -> bool {
    setting_enabled && !override_value.is_some_and(|v| !v.is_empty())
}

struct PadSlot {
    device: Option<Box<dyn PadDevice>>,
    enabled: bool,
    haptics: bool,
}

impl PadSlot {
    fn active(&self) -> bool {
        self.enabled && self.device.is_some()
    }
}

/// The haptics device layer: `_device_rumble` / `_device_stop_rumble`,
/// guarded by "active and haptics enabled".
pub struct DeviceRumbleSink {
    slot: Rc<RefCell<PadSlot>>,
}

impl RumbleSink for DeviceRumbleSink {
    fn send(&mut self, low: f64, high: f64, duration_ms: i32) {
        let mut slot = self.slot.borrow_mut();
        if !(slot.active() && slot.haptics) {
            return;
        }
        if let Some(device) = slot.device.as_mut() {
            device.rumble(to_motor(low), to_motor(high), duration_ms.max(0) as u32);
        }
    }

    fn stop(&mut self) {
        let mut slot = self.slot.borrow_mut();
        if let Some(device) = slot.device.as_mut() {
            device.stop_rumble();
        }
    }
}

/// 0..1 motor level to SDL's 16-bit range.
pub fn to_motor(level: f64) -> u16 {
    (level.clamp(0.0, 1.0) * 65535.0).round() as u16
}

/// Owns the active controller and translates its events for the game.
pub struct ControllerManager {
    slot: Rc<RefCell<PadSlot>>,
    /// Which device the player last used (`active_device`).
    pub active_device: &'static str,
    /// Whether a hardware keyboard is there to name keys on. Always off
    /// iOS; on iOS it follows GameController's connect and disconnect
    /// notifications, and without one no hint names a key.
    pub hardware_keyboard: bool,
    /// Set while a gesture is played as key presses, so those presses do
    /// not count as the keyboard being used.
    pub touch_keys: bool,
    instance_id: Option<u32>,
    name: String,
    disconnected: bool, // latched until the app consumes it
    // A freshly opened pad reports a device id that a hot-plug can leave
    // stale (the events then arrive under a different id). We treat the
    // opened id as provisional and adopt the id of the first real event we
    // see, so the binding always matches the live event stream.
    id_pending: bool,
    // Buttons currently held, so a duplicate press (some pads/drivers
    // deliver a button twice) fires its action only once, on the genuine
    // transition.
    buttons_down: HashSet<ControllerButton>,
    // Last "foreign" instance id we logged a dropped event for, so a stream
    // of events from a stale/other pad logs once rather than every frame.
    logged_mismatch_id: Option<u32>,
    /// Haptics live in their own SDL-free engine; we only supply the guarded
    /// device send/stop and drive it once per frame in `tick()`.
    pub rumble: RumbleEngine<DeviceRumbleSink>,
    // Raw (event) and smoothed (tick) analog targets. The clutch is a
    // digital bumper, so it stays instant like the keyboard Shift and is not
    // smoothed.
    steer_target: f64,
    throttle_target: f64,
    brake_target: f64,
    clutch: f64,
    throttle: f64,
    brake: f64,
    /// Right bumper held -> secondary bindings.
    pub modifier: bool,
    repeat: RepeatTimer,
    // Defer touching the SDL controller subsystem until support is on, so a
    // player with controllers disabled never enumerates or binds a pad. It
    // comes up lazily on the first enable (see set_enabled/init_subsystem).
    subsystem: Option<Box<dyn PadSubsystem>>,
    factory: Option<PadSubsystemFactory>,
}

impl ControllerManager {
    /// A manager over a subsystem factory (`None` = no subsystem on this
    /// machine: keyboard only, as under the dummy drivers).
    pub fn new(enabled: bool, haptics: bool, factory: Option<PadSubsystemFactory>) -> Self {
        let slot = Rc::new(RefCell::new(PadSlot {
            device: None,
            enabled,
            haptics,
        }));
        let mut manager = Self {
            slot: Rc::clone(&slot),
            active_device: DEFAULT_DEVICE,
            hardware_keyboard: !cfg!(target_os = "ios"),
            touch_keys: false,
            instance_id: None,
            name: String::new(),
            disconnected: false,
            id_pending: false,
            buttons_down: HashSet::new(),
            logged_mismatch_id: None,
            rumble: RumbleEngine::new(DeviceRumbleSink { slot }),
            steer_target: 0.0,
            throttle_target: 0.0,
            brake_target: 0.0,
            clutch: 0.0,
            throttle: 0.0,
            brake: 0.0,
            modifier: false,
            repeat: RepeatTimer::default(),
            subsystem: None,
            factory,
        };
        if enabled {
            manager.init_subsystem();
        } else {
            log::debug!("Controller support off at startup; subsystem not initialized");
        }
        manager
    }

    /// A manager that never reaches a subsystem (headless runs and tests).
    pub fn detached(enabled: bool, haptics: bool) -> Self {
        Self::new(enabled, haptics, None)
    }

    // -- device lifecycle -----------------------------------------------------

    /// Initialize the controller subsystem and bind the first pad.
    ///
    /// Idempotent and headless-safe: a failure leaves the subsystem absent
    /// (keyboard only), and a repeat call while already initialized is a
    /// no-op.
    fn init_subsystem(&mut self) {
        if self.subsystem.is_some() {
            return;
        }
        let Some(factory) = self.factory.as_mut() else {
            return;
        };
        match factory() {
            Some(subsystem) => {
                self.subsystem = Some(subsystem);
                log::debug!("Controller subsystem initialized");
                self.open_first();
            }
            None => log::info!("Controller subsystem unavailable; keyboard only"),
        }
    }

    /// Silence and release the pad, then uninitialize the subsystem so no
    /// controller handles or SDL state remain while support is off.
    fn teardown_subsystem(&mut self) {
        self.rumble.reset(); // silence the pad before we drop it
        self.close_controller();
        if self.subsystem.take().is_some() {
            log::debug!("Controller subsystem torn down");
        }
    }

    /// Whether the subsystem is up (`_sdl is not None`).
    pub fn subsystem_up(&self) -> bool {
        self.subsystem.is_some()
    }

    pub fn connected(&self) -> bool {
        self.slot.borrow().device.is_some()
    }

    /// True when a controller is connected and controller input is on.
    pub fn active(&self) -> bool {
        self.slot.borrow().active()
    }

    pub fn enabled(&self) -> bool {
        self.slot.borrow().enabled
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// The bound instance id, `None` when nothing is bound.
    pub fn instance_id(&self) -> Option<u32> {
        self.instance_id
    }

    /// Whether the bound id is still provisional (see `id_pending`).
    pub fn id_pending(&self) -> bool {
        self.id_pending
    }

    /// Test seam: mark the bound id latched or provisional.
    pub fn set_id_pending(&mut self, pending: bool) {
        self.id_pending = pending;
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.slot.borrow_mut().enabled = enabled;
        if enabled {
            self.init_subsystem(); // bring the subsystem up on first enable
        } else {
            self.teardown_subsystem(); // close handles + uninit while off
            self.reset_analog();
            self.active_device = DEFAULT_DEVICE;
        }
    }

    pub fn set_haptics_enabled(&mut self, enabled: bool) {
        self.slot.borrow_mut().haptics = enabled;
        if !enabled {
            self.rumble.reset();
        }
    }

    /// Bind `device` directly (the test seam `m._controller = object()`, and
    /// what `open_index` does once the subsystem handed a pad over).
    pub fn bind_device(&mut self, device: Box<dyn PadDevice>, name: &str) {
        self.instance_id = Some(device.instance_id());
        // Provisional: the first real event's instance id is authoritative
        // (see id_pending), which self-heals a stale id left by a hot-plug.
        self.id_pending = true;
        self.name = if name.is_empty() {
            "controller".to_string()
        } else {
            name.to_string()
        };
        log::info!(
            "Controller connected: {} (instance {:?})",
            self.name,
            self.instance_id
        );
        let mapping = device.mapping();
        self.slot.borrow_mut().device = Some(device);
        self.log_capabilities(mapping.as_deref());
    }

    /// Bind the GameController at `index`; true on success.
    fn open_index(&mut self, index: u32) -> bool {
        let (device, name) = {
            let Some(subsystem) = self.subsystem.as_ref() else {
                return false;
            };
            if !subsystem.is_controller(index) {
                return false;
            }
            let Some(device) = subsystem.open(index) else {
                log::debug!("Could not open controller at slot {index}");
                return false;
            };
            (device, subsystem.name_for_index(index).unwrap_or_default())
        };
        self.bind_device(device, &name);
        true
    }

    fn open_first(&mut self) {
        let count = match self.subsystem.as_ref() {
            Some(subsystem) => subsystem.count(),
            None => return,
        };
        for index in 0..count {
            if self.open_index(index) {
                return;
            }
        }
    }

    /// DEBUG snapshot of the bound pad's mapping, plus a WARNING if the
    /// trigger and shoulder controls are unmapped -- the fingerprint of the
    /// reconnect bug where those inputs stop responding while the rest of
    /// the pad works.
    fn log_capabilities(&self, mapping: Option<&str>) {
        let Some(mapping) = mapping.filter(|m| !m.is_empty()) else {
            return;
        };
        let keys = mapping_keys(mapping);
        log::debug!("Controller {:?} mapping keys: {:?}", self.name, keys);
        let missing: Vec<&str> = ESSENTIAL_MAPPINGS
            .iter()
            .copied()
            .filter(|name| !keys.iter().any(|k| k == name))
            .collect();
        if !missing.is_empty() {
            log::warn!(
                "Controller {:?} reconnected without {} mapped; \
                 brake/throttle and bumpers may not respond",
                self.name,
                missing.join(", ")
            );
        }
    }

    /// Release the controller before dropping our reference, so its
    /// bookkeeping is torn down cleanly across a hot-plug.
    fn close_controller(&mut self) {
        self.slot.borrow_mut().device = None;
        self.instance_id = None;
        self.id_pending = false;
        self.buttons_down.clear();
    }

    /// Whether the bound pad is still physically present.
    fn is_attached(&self) -> bool {
        self.slot
            .borrow()
            .device
            .as_ref()
            .is_some_and(|device| device.attached())
    }

    /// Drop the current pad and bind whatever is connected now.
    ///
    /// The first real event's instance id (see `id_pending`) corrects a
    /// stale id left by a hot-plug, so we never cycle the SDL subsystem:
    /// quitting and re-initializing it while the event loop is live
    /// re-registers SDL's controller event watch and makes every controller
    /// event arrive twice.
    pub fn reopen(&mut self) {
        self.close_controller();
        self.open_first();
    }

    /// Return and clear the pending-disconnect flag (app pauses on True).
    pub fn take_disconnect(&mut self) -> bool {
        if self.disconnected {
            self.disconnected = false;
            return true;
        }
        false
    }

    fn reset_analog(&mut self) {
        self.steer_target = 0.0;
        self.throttle_target = 0.0;
        self.brake_target = 0.0;
        self.clutch = 0.0;
        self.throttle = 0.0;
        self.brake = 0.0;
        self.modifier = false;
        self.repeat.clear();
        self.buttons_down.clear();
        self.rumble.reset();
    }

    // -- input notification ---------------------------------------------------

    /// Record that the keyboard was just used, so hints name keys.
    pub fn note_keyboard(&mut self) {
        if !self.touch_keys {
            self.active_device = KEYBOARD;
        }
    }

    /// Record that the touch screen was just used, so hints name gestures.
    pub fn note_touch(&mut self) {
        self.active_device = TOUCH;
    }

    /// `"controller"` when a pad is active and was used last, `"touch"`
    /// when the screen was, else `"keyboard"`.
    pub fn device(&self) -> &'static str {
        if self.active() && self.active_device == CONTROLLER {
            CONTROLLER
        } else if self.active_device == TOUCH || !self.hardware_keyboard {
            TOUCH
        } else {
            KEYBOARD
        }
    }

    /// Control phrase for `action` naming whatever device is in use.
    pub fn hint(&self, action: &str) -> String {
        control_hint(action, self.device())
    }

    // -- event handling -------------------------------------------------------

    /// Update device/axis/modifier state from a `CONTROLLER*` event.
    ///
    /// Returns true only for a button press/release that belongs to the
    /// bound controller and should be forwarded to the active state. Axis,
    /// device, and rejected (foreign-id) events return false, so the app
    /// never routes a stray event -- e.g. a duplicate from a pad that
    /// enumerates twice -- into a game action.
    pub fn process_event(&mut self, event: &InputEvent) -> bool {
        let instance_id = match event {
            InputEvent::ControllerAdded { device_index } => {
                log::debug!("Controller device added (slot {device_index})");
                // Re-open when nothing is bound, or the bound pad has quietly
                // detached (a missed REMOVED event) -- otherwise a stale
                // binding would block the reconnect.
                if self.connected() && self.is_attached() {
                    log::debug!(
                        "Ignoring add; a pad is already bound (instance {:?})",
                        self.instance_id
                    );
                    return false;
                }
                // Re-open in place. The first real event's id (see
                // id_pending) corrects a stale id from the hot-plug; we
                // deliberately do not cycle the SDL subsystem, which would
                // double every controller event.
                self.reopen();
                return false;
            }
            InputEvent::ControllerRemoved { instance_id } => {
                log::debug!("Controller device removed (instance {instance_id})");
                if self.instance_id.is_some() && Some(*instance_id) == self.instance_id {
                    self.close_controller();
                    self.reset_analog();
                    self.disconnected = true;
                    self.logged_mismatch_id = None;
                    log::info!("Controller disconnected");
                }
                return false;
            }
            InputEvent::ControllerButtonDown { instance_id, .. }
            | InputEvent::ControllerButtonUp { instance_id, .. }
            | InputEvent::ControllerAxis { instance_id, .. } => *instance_id,
            _ => return false,
        };
        if !self.active() {
            return false;
        }
        if Some(instance_id) != self.instance_id {
            if self.id_pending {
                // First real event after (re)open: its id is authoritative,
                // so a stale id from the open after a hot-plug is corrected
                // here.
                log::info!(
                    "Controller bound to instance {instance_id} (opened as {:?})",
                    self.instance_id
                );
                self.instance_id = Some(instance_id);
                self.id_pending = false;
                self.logged_mismatch_id = None;
            } else {
                // A genuinely foreign id: a second pad, or a duplicate from
                // a pad that enumerates twice. Drop it (log once) so it never
                // doubles a toggle or other action.
                if Some(instance_id) != self.logged_mismatch_id {
                    self.logged_mismatch_id = Some(instance_id);
                    log::debug!(
                        "Dropping controller event from instance {instance_id} (bound to {:?})",
                        self.instance_id
                    );
                }
                return false;
            }
        } else {
            // Matched the bound id, so it is live: stop treating the id as
            // provisional (a duplicate under a different id is now a
            // duplicate).
            self.id_pending = false;
        }
        match event {
            InputEvent::ControllerAxis { axis, value, .. } => {
                self.on_axis(*axis, *value);
                false
            }
            InputEvent::ControllerButtonDown { button, .. } => {
                if self.buttons_down.contains(button) {
                    // Duplicate press with no intervening release: ignore it
                    // so a pad that delivers a button twice cannot fire the
                    // action twice.
                    log::debug!(
                        "Ignoring duplicate button-down {button:?} (instance {instance_id})"
                    );
                    return false;
                }
                self.buttons_down.insert(*button);
                self.active_device = CONTROLLER;
                log::debug!("Button down {button:?} (instance {instance_id})");
                self.on_button_down(*button);
                true
            }
            InputEvent::ControllerButtonUp { button, .. } => {
                if !self.buttons_down.remove(button) {
                    return false; // stray/duplicate release
                }
                log::debug!("Button up {button:?} (instance {instance_id})");
                self.on_button_up(*button);
                true
            }
            _ => false,
        }
    }

    fn on_axis(&mut self, axis: ControllerAxis, raw: i16) {
        let value = raw as f64 / AXIS_MAX;
        match axis {
            ControllerAxis::LeftX => {
                self.steer_target = deadzone(value, STICK_DEADZONE);
                if self.steer_target != 0.0 {
                    self.active_device = CONTROLLER;
                }
            }
            ControllerAxis::TriggerRight => {
                self.throttle_target = deadzone(value.max(0.0), TRIGGER_DEADZONE);
                if self.throttle_target != 0.0 {
                    self.active_device = CONTROLLER;
                }
            }
            ControllerAxis::TriggerLeft => {
                self.brake_target = deadzone(value.max(0.0), TRIGGER_DEADZONE);
                if self.brake_target != 0.0 {
                    self.active_device = CONTROLLER;
                }
            }
            _ => {}
        }
    }

    fn on_button_down(&mut self, button: ControllerButton) {
        match button {
            ControllerButton::RightShoulder => self.modifier = true,
            ControllerButton::LeftShoulder => self.clutch = 1.0, // instant, like holding Shift
            _ => {}
        }
        self.repeat.press(button);
    }

    fn on_button_up(&mut self, button: ControllerButton) {
        match button {
            ControllerButton::RightShoulder => self.modifier = false,
            ControllerButton::LeftShoulder => self.clutch = 0.0,
            _ => {}
        }
        self.repeat.release(button);
    }

    /// The semantic menu action for a `ControllerButtonDown`, or None.
    pub fn menu_action(&self, event: &InputEvent) -> Option<ControllerAction> {
        match event {
            InputEvent::ControllerButtonDown { button, .. } => menu_action_for(*button),
            _ => None,
        }
    }

    pub fn button_label(&self, button: ControllerButton) -> &'static str {
        button_label(button)
    }

    // -- per-frame update -----------------------------------------------------

    /// Smooth analog axes and return synthetic D-pad repeat events.
    pub fn tick(&mut self, dt: f64) -> Vec<InputEvent> {
        // Always drive the rumble engine so queued effects decay and expire
        // even when no controller is bound; the device send is guarded
        // either way.
        self.rumble.tick(dt);
        if !self.active() {
            return Vec::new();
        }
        self.throttle = smooth(self.throttle, self.throttle_target, dt);
        self.brake = smooth(self.brake, self.brake_target, dt);
        self.repeats(dt)
    }

    fn repeats(&mut self, dt: f64) -> Vec<InputEvent> {
        let Some(button) = self.repeat.button() else {
            return Vec::new();
        };
        let count = self.repeat.tick(dt);
        let instance_id = self.instance_id.unwrap_or(0);
        (0..count)
            .map(|_| InputEvent::ControllerButtonDown {
                button,
                instance_id,
            })
            .collect()
    }

    // -- analog reads for the driving loop ------------------------------------

    pub fn steering(&self) -> f64 {
        if self.active() {
            self.steer_target
        } else {
            0.0
        }
    }

    pub fn throttle(&self) -> f64 {
        if self.active() {
            self.throttle
        } else {
            0.0
        }
    }

    pub fn brake(&self) -> f64 {
        if self.active() {
            self.brake
        } else {
            0.0
        }
    }

    /// Instantaneous trigger position, before smoothing. Press/release
    /// detection (e.g. the reverse shift gesture) must read this so a quick
    /// tap registers -- the smoothed reads above lag behind the trigger.
    pub fn throttle_target(&self) -> f64 {
        if self.active() {
            self.throttle_target
        } else {
            0.0
        }
    }

    pub fn brake_target(&self) -> f64 {
        if self.active() {
            self.brake_target
        } else {
            0.0
        }
    }

    pub fn clutch(&self) -> f64 {
        if self.active() {
            self.clutch
        } else {
            0.0
        }
    }

    pub fn shutdown(&mut self) {
        self.teardown_subsystem();
        // The production factory captures an SDL handle so controller support
        // can be enabled later. Final shutdown has no later: release that
        // handle before the window begins its exit path.
        self.factory = None;
    }
}

/// The field names of an SDL mapping string (`"a:b0,b:b1,..."`), without
/// the leading GUID and name fields.
pub fn mapping_keys(mapping: &str) -> Vec<String> {
    mapping
        .split(',')
        .filter_map(|field| field.split_once(':'))
        .filter(|(_, value)| !value.is_empty())
        .map(|(key, _)| key.trim().to_string())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn deadzone_rescales_to_full_range() {
        assert_eq!(deadzone(0.05, STICK_DEADZONE), 0.0);
        assert_eq!(deadzone(-0.05, STICK_DEADZONE), 0.0);
        assert!((deadzone(1.0, STICK_DEADZONE) - 1.0).abs() < 1e-12);
        assert!((deadzone(-1.0, STICK_DEADZONE) + 1.0).abs() < 1e-12);
        let just_past = deadzone(0.11, STICK_DEADZONE);
        assert!(just_past > 0.0 && just_past < 0.02);
    }

    #[test]
    fn repeat_timer_waits_then_accelerates() {
        let mut timer = RepeatTimer::default();
        timer.press(ControllerButton::DPadLeft);
        assert_eq!(timer.tick(0.1), 0);
        assert_eq!(timer.tick(0.25), 1); // crosses the 0.3 s initial delay
        timer.release(ControllerButton::DPadLeft);
        assert_eq!(timer.tick(1.0), 0);
        // Fully ramped, a frame of 0.1 s yields several repeats.
        timer.press(ControllerButton::DPadRight);
        let _ = timer.tick(REPEAT_RAMP_S + 0.3);
        assert!(timer.tick(0.1) >= 3);
    }

    #[test]
    fn repeat_timer_ignores_other_buttons() {
        let mut timer = RepeatTimer::default();
        timer.press(ControllerButton::A);
        assert_eq!(timer.button(), None);
        assert_eq!(timer.tick(1.0), 0);
    }

    #[test]
    fn mapping_keys_skip_the_guid_and_name() {
        let keys = mapping_keys(
            "030000005e040000ea02000000007200,Xbox,a:b0,b:b1,lefttrigger:a2,righttrigger:a5,",
        );
        assert_eq!(keys, vec!["a", "b", "lefttrigger", "righttrigger"]);
    }

    #[test]
    fn motor_levels_fill_the_sixteen_bits() {
        assert_eq!(to_motor(0.0), 0);
        assert_eq!(to_motor(1.0), 65535);
        assert_eq!(to_motor(2.0), 65535);
    }

    #[test]
    fn shutdown_releases_the_sdl_factory_reference() {
        struct DropNotice(Rc<Cell<bool>>);
        impl Drop for DropNotice {
            fn drop(&mut self) {
                self.0.set(true);
            }
        }

        let dropped = Rc::new(Cell::new(false));
        let notice = DropNotice(Rc::clone(&dropped));
        let factory: PadSubsystemFactory = Box::new(move || {
            let _keep_alive = &notice;
            None
        });
        let mut manager = ControllerManager::new(false, false, Some(factory));

        manager.shutdown();

        assert!(
            dropped.get(),
            "controller shutdown retained its SDL-owning factory"
        );
    }
}

#[cfg(test)]
mod pads_allowed_tests {
    #[test]
    fn the_no_controller_override_wins_over_the_setting() {
        use std::ffi::OsStr;
        assert!(super::pads_allowed(true, None));
        assert!(super::pads_allowed(true, Some(OsStr::new(""))));
        assert!(!super::pads_allowed(true, Some(OsStr::new("1"))));
        assert!(!super::pads_allowed(false, None));
    }
}
