//! The driving school: spoken lessons on a consequence-free practice road
//! (port of `freight_fate/states/driving_school.py`).
//!
//! The school runs the real driving engine on a disposable copy of the
//! player's profile: every lesson consequence -- wear, fuel, money, hours,
//! fatigue -- lands on the copy and dies with the lesson. One guard in
//! `GameContext::save_profile` keeps the sandbox off disk, and the practice
//! drive restores the real profile on every exit path.
//!
//! Lessons are instructor state machines that duck-typed the first-run
//! `Tutorial` in Python; here they implement the same
//! [`crate::states::driving_core::Instructor`] trait, so the driving code's
//! existing `on_engine_started` / `on_parking_brake_released` /
//! `on_gear_engaged` / `update` calls drive a lesson unchanged. Instruction
//! stays on the speech channel (screen-reader rate, comma repeat, verbosity
//! all keep working); recorded instructor flavor lines can hang off the same
//! stage transitions later.
//!
//! # Shape of the port
//!
//! Python's `SchoolDrivingState` SUBCLASSED `DrivingState` to add the sandbox
//! restore on exit. Rust has no inheritance, so it wraps one and forwards
//! every `State` hook, adding `leave_sandbox` to `exit`. A lesson that
//! finishes pops itself off the stack directly (Python's `finish_lesson`
//! reached back through `self.driving`), guarded by its own `done` flag,
//! which is exactly what the `_lesson_finished` latch bought.

use ff_core::data::world_models::{Leg, Route};
use ff_core::models::jobs::{Job, CARGO_CATALOG};
use ff_core::models::profile::Profile;
use ff_core::radio::RadioState;
use ff_core::sim::vehicle::TruckState;

use crate::app::GameContext;
use crate::discord_presence::PresenceState;
use crate::impl_state_for_menu;
use crate::states::base::{InputEvent, Menu, MenuCore, MenuItem, State};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{Instructor, DRIVE_PHASE_SCHOOL};
use crate::touch::Gesture;

/// Long enough that no lesson meets the end of it.
pub const PRACTICE_ROAD_MILES: f64 = 25.0;

/// Swap the live profile for a throwaway copy; idempotent.
pub fn enter_sandbox(ctx: &mut GameContext) {
    if ctx.school_sandbox {
        return;
    }
    let copy = ctx.profile.as_ref().map(|p| p.duplicate());
    ctx.school_real_profile = ctx.profile.take();
    ctx.profile = copy;
    ctx.school_sandbox = true;
}

/// Restore the real profile; safe to call from every exit path.
pub fn leave_sandbox(ctx: &mut GameContext) {
    if !ctx.school_sandbox {
        return;
    }
    ctx.profile = ctx.school_real_profile.take();
    ctx.school_sandbox = false;
}

/// A flat, empty stretch of road going nowhere, starting here.
pub fn practice_route(ctx: &GameContext) -> (Job, Route) {
    let city = ctx.world.resolve_city_key(
        &ctx.profile
            .as_ref()
            .map(|p| p.current_city.clone())
            .unwrap_or_default(),
    );
    let leg = Leg::new(
        &city,
        &city,
        PRACTICE_ROAD_MILES,
        "the practice road",
        "flat",
        Vec::new(),
    );
    let mut job = Job::new(
        &CARGO_CATALOG["general"],
        0.0,
        &city,
        "the training yard",
        &city,
        PRACTICE_ROAD_MILES,
        0.0,
        24.0,
    );
    job.origin_type = "company_terminal".to_string();
    job.destination_location = "the training yard".to_string();
    job.destination_type = "company_terminal".to_string();
    job.bobtail = true;
    (job, Route::from_legs(vec![city.clone(), city], vec![leg]))
}

/// Lesson 1: engine, air, parking brake, roll to 30, stop.
///
/// Stages: 0 start the engine, 1 air and parking brake (or first gear on a
/// manual), 2 accelerate to 30, 3 brake to a full stop.
pub struct RollingBasicsLesson {
    pub stage: i32,
    pub done: bool,
    timer: f64,
    hinted: bool,
}

impl RollingBasicsLesson {
    pub const ROLL_MPH: f64 = 30.0;
    pub const STOP_MPH: f64 = 0.5;
    pub const HINT_S: f64 = 30.0;

    pub fn new() -> Self {
        RollingBasicsLesson {
            stage: 0,
            done: false,
            timer: 0.0,
            hinted: false,
        }
    }

    fn say(&self, ctx: &mut GameContext, text: String) {
        ctx.say_with(text, crate::app::Say::queued());
    }

    fn advance(&mut self, stage: i32) {
        self.stage = stage;
        self.timer = 0.0;
        self.hinted = false;
    }
}

impl Default for RollingBasicsLesson {
    fn default() -> Self {
        Self::new()
    }
}

impl Instructor for RollingBasicsLesson {
    fn begin(&mut self, ctx: &mut GameContext) {
        let text = format!(
            "Driving school. The practice road: flat, empty, and none of it counts toward your \
             career, truck, or money. Lesson one, rolling basics. Press {} to start the engine.",
            ctx.control_hint("engine")
        );
        self.say(ctx, text);
    }

    fn on_engine_started(&mut self, ctx: &mut GameContext) {
        if self.stage != 0 {
            return;
        }
        self.advance(1);
        let text = if ctx.settings.automatic_transmission {
            format!(
                "Engine running. When you hear air ready, press {} to release the parking \
                 brake, then hold {} to accelerate. The transmission shifts for you.",
                ctx.control_hint("parking_brake"),
                ctx.control_hint("accelerate")
            )
        } else {
            format!(
                "Engine running. When you hear air ready, press {} to release the parking \
                 brake, hold {}, select {} for first gear, and release the clutch.",
                ctx.control_hint("parking_brake"),
                ctx.control_hint("clutch"),
                ctx.control_hint("gear_first")
            )
        };
        self.say(ctx, text);
    }

    fn on_parking_brake_released(&mut self, ctx: &mut GameContext) -> bool {
        if self.stage != 1 {
            return false;
        }
        if ctx.settings.automatic_transmission {
            self.advance(2);
            let text = format!(
                "Parking brake released. Hold {} up to thirty.",
                ctx.control_hint("accelerate")
            );
            self.say(ctx, text);
        } else {
            self.timer = 0.0;
            self.say(
                ctx,
                "Parking brake released. Shift into first gear.".to_string(),
            );
        }
        true
    }

    fn on_gear_engaged(&mut self, ctx: &mut GameContext) {
        if self.stage != 1 {
            return;
        }
        self.advance(2);
        let text = format!(
            "In gear. Hold {} up to thirty.",
            ctx.control_hint("accelerate")
        );
        self.say(ctx, text);
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64, truck: &TruckState) {
        if self.done {
            return;
        }
        self.timer += dt;
        if self.stage == 2 && truck.speed_mph() >= Self::ROLL_MPH {
            self.advance(3);
            let text = format!(
                "Thirty. Ease off and brake gently with {} to a full stop.",
                ctx.control_hint("brake")
            );
            self.say(ctx, text);
        } else if self.stage == 3 && truck.speed_mph() <= Self::STOP_MPH {
            self.done = true;
            self.say(
                ctx,
                "Full stop. Lesson one complete. Back to the school.".to_string(),
            );
            // `finish_lesson`: the practice drive comes off the stack, once.
            ctx.pop_state();
        } else if self.timer > Self::HINT_S && !self.hinted {
            self.hinted = true;
            if self.stage == 0 {
                let text = format!(
                    "Reminder: press {} to start the engine.",
                    ctx.control_hint("engine")
                );
                self.say(ctx, text);
            } else if self.stage == 1 && truck.parking_brake {
                // Same contradiction the first-run tutorial had: once the air
                // is up, "wait for air" is wrong and P is the whole reminder.
                let text = if truck.air_ready() {
                    format!(
                        "Reminder: air is ready. Press {} to release the parking brake.",
                        ctx.control_hint("parking_brake")
                    )
                } else {
                    format!(
                        "Reminder: wait for air pressure to reach one hundred psi, then press {} \
                         to release the parking brake.",
                        ctx.control_hint("parking_brake")
                    )
                };
                self.say(ctx, text);
            } else if self.stage == 2 {
                let text = format!(
                    "Reminder: hold {} until you reach thirty miles per hour.",
                    ctx.control_hint("accelerate")
                );
                self.say(ctx, text);
            }
        }
    }
}

/// Which lesson a school row starts.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LessonKind {
    RollingBasics,
}

impl LessonKind {
    fn instructor(self) -> Box<dyn Instructor> {
        match self {
            LessonKind::RollingBasics => Box::new(RollingBasicsLesson::new()),
        }
    }
}

/// `LESSONS`: name, lesson, blurb.
pub const LESSONS: [(&str, LessonKind, &str); 1] = [(
    "Lesson 1: Rolling basics",
    LessonKind::RollingBasics,
    "Start the engine, build air, release the parking brake, take the truck to thirty, and brake \
     to a smooth full stop.",
)];

/// A practice drive: the real engine, a sandbox profile, an instructor.
pub struct SchoolDrivingState {
    drive: DrivingState,
}

impl SchoolDrivingState {
    pub fn new(ctx: &mut GameContext, lesson: LessonKind) -> Self {
        let (job, route) = practice_route(ctx);
        let mut drive = DrivingState::new(ctx, job, route, None, DRIVE_PHASE_SCHOOL, None);
        // The instructor rides the first-run tutorial's hooks; the school
        // replaces whatever the base state decided about the tutorial.
        drive.tutorial = Some(lesson.instructor());
        SchoolDrivingState { drive }
    }

    pub fn drive(&self) -> &DrivingState {
        &self.drive
    }

    pub fn drive_mut(&mut self) -> &mut DrivingState {
        &mut self.drive
    }
}

impl State for SchoolDrivingState {
    fn paces_main_speech(&self) -> bool {
        self.drive.paces_main_speech()
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        self.drive.enter(ctx);
    }

    fn exit(&mut self, ctx: &mut GameContext) {
        self.drive.exit(ctx);
        // Every path out of a practice drive restores the real profile:
        // lesson completion, Escape, even an abandon from the pause menu.
        leave_sandbox(ctx);
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        self.drive.handle_event(ctx, event);
    }

    fn handle_gesture(&mut self, ctx: &mut GameContext, gesture: Gesture) -> bool {
        self.drive.handle_gesture(ctx, gesture)
    }

    fn handle_controller(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        self.drive.handle_controller(ctx, event);
    }

    fn on_controller_disconnect(&mut self, ctx: &mut GameContext) {
        self.drive.on_controller_disconnect(ctx);
    }

    fn update(&mut self, ctx: &mut GameContext, dt: f64) {
        self.drive.update(ctx, dt);
    }

    fn lines(&self, ctx: &GameContext) -> Vec<String> {
        self.drive.lines(ctx)
    }

    fn presence(&self, ctx: &GameContext) -> Option<PresenceState> {
        self.drive.presence(ctx)
    }

    fn online_presence(&self, ctx: &GameContext) -> Option<PresenceState> {
        self.drive.online_presence(ctx)
    }

    fn ticks_covered_music(&self) -> bool {
        self.drive.ticks_covered_music()
    }

    fn tick_covered_music(&mut self, ctx: &mut GameContext, dt: f64) {
        self.drive.tick_covered_music(ctx, dt);
    }

    fn applies_radio_settings(&self) -> bool {
        self.drive.applies_radio_settings()
    }

    fn apply_radio_settings_now(&mut self, ctx: &mut GameContext) {
        self.drive.apply_radio_settings_now(ctx);
    }

    fn radio(&self) -> Option<&RadioState> {
        self.drive.radio()
    }
}

pub struct DrivingSchoolState {
    menu: MenuCore<Self>,
}

const SCHOOL_INTRO_HELP: &str =
    "Lessons run on a practice road where nothing counts: no money, no wear, no hours. Escape \
     returns to the terminal.";

impl DrivingSchoolState {
    pub fn new() -> Self {
        DrivingSchoolState {
            menu: MenuCore::new("Driving school").with_intro_help(SCHOOL_INTRO_HELP),
        }
    }

    fn start(&mut self, ctx: &mut GameContext, lesson: LessonKind) {
        enter_sandbox(ctx);
        let state = SchoolDrivingState::new(ctx, lesson);
        ctx.push_state(state);
    }
}

impl Default for DrivingSchoolState {
    fn default() -> Self {
        Self::new()
    }
}

impl Menu for DrivingSchoolState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        // Belt and suspenders: if a practice drive ever unwinds without its
        // exit hook, re-entering the school still restores the player.
        leave_sandbox(ctx);
        let items = self.build_items(ctx);
        self.menu.items = items;
        self.menu.index = self.menu.index.min(self.menu.items.len().saturating_sub(1));
        if let Some(key) = self.menu.open_sound_key.clone() {
            ctx.audio.play(&key);
        }
        self.announce_entry(ctx);
    }

    fn build_items(&mut self, _ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = LESSONS
            .iter()
            .map(|(name, lesson, blurb)| {
                let lesson = *lesson;
                MenuItem::new(*name, move |s: &mut Self, ctx: &mut GameContext| {
                    s.start(ctx, lesson)
                })
                .help(*blurb)
            })
            .collect();
        items.push(
            MenuItem::new("Back to terminal", |s: &mut Self, ctx| s.go_back(ctx))
                .help("Leave the school."),
        );
        items
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
    }
}

impl_state_for_menu!(DrivingSchoolState);

/// `Profile.from_dict(profile.to_dict())`: the throwaway copy the sandbox
/// runs on.
trait DuplicateProfile {
    fn duplicate(&self) -> Profile;
}

impl DuplicateProfile for Profile {
    fn duplicate(&self) -> Profile {
        Profile::from_dict(&self.to_dict())
    }
}
