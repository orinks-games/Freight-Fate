//! Adversarial breaker battery: drive the sim in unreasonable ways, on
//! purpose (port of `tools/playtest_break.py`).
//!
//! Every scenario does something no sensible driver would do -- reverse down
//! the interstate, slam reverse at highway speed, coast a mountain in
//! neutral, dynamite the parking brake at 60 -- and then CHECKS the
//! invariants programmatically: does physics stay sane, does money/XP/HOS/rep
//! stay honest, and does the spoken text still tell a blind player the truth.
//! Run it after any feature lands to see what newly breaks.
//!
//! Scenarios are deterministic (fixed trip seed, pinned weather, no random
//! hazards or patrols unless the scenario is about them) and self-contained:
//! each builds its own app and [`DrivingState`] the way the tests do, drives
//! real `update_frame` frames, and returns CLEAN or ODD with a one-line note.
//! ODD means "a discrepancy a human should look at", not necessarily a bug --
//! the point is that the battery notices when the answer CHANGES.
//!
//! # Shape of the port
//!
//! Python registered scenarios with a decorator into a module-level dict.
//! Rust has no import side effects to rely on, so [`scenarios`] is one
//! explicit table built in [`super::break_scenarios`]. The registry order is
//! the table's order, which is the battery's run order.
//!
//! Held keys were `pygame.key.get_pressed` monkeypatched with a set; here
//! they are the app's own [`HeldKeys`][crate::app::HeldKeys], which is the
//! same thing the real loop fills in.

use std::cell::RefCell;
use std::rc::Rc;

use crate::app::testing::{FakeClock, TestApp};
use crate::app::SharedState;
use crate::states::base::{InputEvent, Key, Mods};
use crate::states::driving::DrivingState;
use crate::states::driving_core::DRIVE_PHASE_DELIVERY;
use crate::states::driving_menu_states::DriveRef;

use ff_core::data::curves::RouteCurve;
use ff_core::data::world_models::{GradeSegment, Leg, Route};
use ff_core::models::jobs::{cargo_type, Job};
use ff_core::models::profile::Profile;
use ff_core::sim::trip::Trip;
use ff_core::sim::weather::WeatherKind;

use super::harness::key_event;
use super::MPH_PER_MPS;

/// Coarse-but-stable frame step; halves the battery's wall time.
pub const DT: f64 = 1.0 / 30.0;

/// How a scenario came out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Clean,
    Odd,
    Error,
}

impl Verdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Verdict::Clean => "CLEAN",
            Verdict::Odd => "ODD",
            Verdict::Error => "ERROR",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Outcome {
    pub name: String,
    pub verdict: Verdict,
    pub note: String,
    pub findings: Vec<String>,
    pub transcript: Vec<String>,
}

/// One registered scenario: its name, its one-line summary, and the closure
/// that runs it.
pub struct Scenario {
    pub name: &'static str,
    pub description: &'static str,
    pub run: fn() -> Outcome,
}

/// Every scenario, in run order.
pub fn scenarios() -> &'static [Scenario] {
    super::break_scenarios::SCENARIOS
}

pub fn scenario_names() -> Vec<&'static str> {
    scenarios().iter().map(|s| s.name).collect()
}

/// The shape of a [`tweak_rigs`] hook.
type RigTweakFn = Rc<dyn Fn(&mut Rig)>;

thread_local! {
    /// The per-rig tweak [`tweak_rigs`] installs, if any.
    static RIG_TWEAK: RefCell<Option<RigTweakFn>> = const { RefCell::new(None) };
}

/// A [`tweak_rigs`] installation, uninstalled when it is dropped.
#[must_use = "the tweak is uninstalled as soon as this guard is dropped"]
pub struct RigTweak;

impl Drop for RigTweak {
    fn drop(&mut self) {
        RIG_TWEAK.with(|slot| *slot.borrow_mut() = None);
    }
}

/// Apply `f` to every [`Rig`] this thread builds until the guard is dropped.
///
/// The battery's scenarios take no arguments -- the registry is a table of
/// `fn() -> Outcome` -- so a caller that needs to shape the rig they all
/// build has nowhere to say so. Python's whole-drive proof of the driving
/// speech ladder (`test_a_drive_gets_quieter_as_the_rung_tightens`) said it
/// by monkeypatching `Rig.__init__` to force a speech rung and mark the
/// walkthrough done; this is that seam, made explicit. It runs after
/// construction, so it overrides everything the rig set up for itself.
///
/// Thread-local, and only one at a time: a rig pins its thread's save
/// directory for as long as it lives, so scenarios never build concurrently
/// on one thread anyway.
pub fn tweak_rigs(f: impl Fn(&mut Rig) + 'static) -> RigTweak {
    RIG_TWEAK.with(|slot| {
        let mut slot = slot.borrow_mut();
        assert!(
            slot.is_none(),
            "this thread already has a rig tweak installed; drop that guard first"
        );
        *slot = Some(Rc::new(f));
    });
    RigTweak
}

/// Run one scenario by name; `None` when the name is unknown.
pub fn run_scenario(name: &str) -> Option<Outcome> {
    let scenario = scenarios().iter().find(|s| s.name == name)?;
    // A scenario that panics is an ERROR, not a dead battery.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(scenario.run));
    Some(result.unwrap_or_else(|payload| Outcome {
        name: name.to_string(),
        verdict: Verdict::Error,
        note: "scenario crashed".to_string(),
        findings: vec![panic_text(&payload)],
        transcript: Vec::new(),
    }))
}

fn panic_text(payload: &Box<dyn std::any::Any + Send>) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        return (*text).to_string();
    }
    if let Some(text) = payload.downcast_ref::<String>() {
        return text.clone();
    }
    "panicked".to_string()
}

/// Fold rig invariant problems into the findings and pick the verdict.
pub fn outcome(name: &str, rig: &Rig, mut findings: Vec<String>, clean_note: &str) -> Outcome {
    findings.extend(rig.problems.iter().map(|p| format!("invariant: {p}")));
    let verdict = if findings.is_empty() {
        Verdict::Clean
    } else {
        Verdict::Odd
    };
    let note = findings
        .first()
        .cloned()
        .unwrap_or_else(|| clean_note.to_string());
    Outcome {
        name: name.to_string(),
        verdict,
        note,
        findings,
        transcript: rig.transcript(),
    }
}

/// [`outcome`] for a scenario with no rig at all -- a pure model probe, the
/// Python `_outcome(name, None, findings, note)`.
pub fn outcome_of(name: &str, findings: Vec<String>, clean_note: &str) -> Outcome {
    let verdict = if findings.is_empty() {
        Verdict::Clean
    } else {
        Verdict::Odd
    };
    let note = findings
        .first()
        .cloned()
        .unwrap_or_else(|| clean_note.to_string());
    Outcome {
        name: name.to_string(),
        verdict,
        note,
        findings,
        transcript: Vec::new(),
    }
}

/// A hand-built bend, for scenarios that need one at a known mile.
pub fn fabricated_curve(start_mi: f64, advisory: i64, direction: char) -> RouteCurve {
    RouteCurve {
        start_mi,
        apex_mi: start_mi + 0.1,
        end_mi: start_mi + 0.22,
        direction,
        advisory_mph: advisory,
        min_radius_ft: 250,
        deflection_deg: 130.0,
        connector: false,
    }
}

/// Bake a constant grade over every leg of `trip`'s route.
///
/// Python's scenarios wrote `trip.grade_at = lambda mile: -0.06`. Rust has no
/// seam for an inherent method, so this builds the road that ANSWERS that
/// way: one `GradeSegment` spanning each leg, which is the record
/// [`Trip::grade_at`] reads before it falls back to the terrain default. Same
/// technique as `bench_road` in the transcript tests, but applied to the leg
/// the rig is already driving rather than to a synthetic one, so the real
/// route's cities, zones, stops and exits all survive -- several scenarios
/// (the jake ordinance at Buffalo mile 2, the ramp handback) are about those.
///
/// Two details the transcript agents' note calls out and this has to honour:
///
/// * `grade_at` NEGATES the segment for a leg driven b-to-a, so the segment
///   is signed by direction and every mile reads back exactly `grade_pct`.
/// * the baked range is CLOSED at both ends where the lambda was total. One
///   segment per leg covering `[0, miles]` leaves no gap to fall through, so
///   no boundary nudge is needed here.
///
/// `terrain` is copied off the leg, so `Trip::terrain_at` answers what it
/// always did: the scenario is changing the slope, not the country.
pub fn force_grade(trip: &mut Trip, grade_pct: f64) {
    let cities = trip.route.cities.clone();
    let mut legs: Vec<Leg> = Vec::with_capacity(trip.route.legs.len());
    for (index, leg) in trip.route.legs.iter().enumerate() {
        let forward = cities.get(index).is_some_and(|city| *city == leg.a);
        let signed = if forward {
            grade_pct * 100.0
        } else {
            -grade_pct * 100.0
        };
        let mut detail = leg.corridor().clone();
        detail.grade_segments = vec![GradeSegment::new(
            0.0,
            leg.miles.max(0.0),
            signed,
            &leg.terrain,
            "breaker rig",
        )];
        let mut rebuilt = Leg::new(
            &leg.a,
            &leg.b,
            leg.miles,
            &leg.highway,
            &leg.terrain,
            leg.stops.clone(),
        );
        rebuilt.id = leg.id;
        rebuilt.truck_advisory = leg.truck_advisory.clone();
        rebuilt.lanes = leg.lanes;
        rebuilt.local_cue = leg.local_cue.clone();
        rebuilt.local_speed_mph = leg.local_speed_mph;
        rebuilt.divided = leg.divided;
        rebuilt.meta_complete = leg.meta_complete;
        legs.push(rebuilt.with_detail(detail));
    }
    trip.route = Route::from_legs(cities, legs);
}

/// The keyword arguments of the Python `Rig.__init__`.
pub struct RigOptions {
    pub automatic: bool,
    pub business: Option<String>,
    pub tons: f64,
    pub seed: i64,
    pub keep_patrols: bool,
}

impl Default for RigOptions {
    fn default() -> Self {
        RigOptions {
            automatic: true,
            business: None,
            tons: 12.0,
            seed: 4242,
            keep_patrols: false,
        }
    }
}

/// One disposable app plus drive, wired for deterministic abuse.
///
/// Mirrors the test idiom: a real headless app, a fresh profile, a supported
/// route, and direct `update_frame` frames with speech captured and the
/// held-key set faked.
/// The rig's drive, in a slot it can be lifted out of and put back.
///
/// Field access (`rig.drive.trip`) and method calls go through `Deref`, so
/// every scenario reads the way the Python did. The slot exists for the
/// screens a drive PUSHES -- the rest stop, the arrival settlement, the
/// abandon confirmation -- which take a `DriveRef` onto a state that has to
/// BE on the stack, and which then pop themselves (and sometimes the drive)
/// back off it. [`Rig::with_drive_on_stack`] moves the drive into a shared
/// cell for the duration and moves it back out afterwards; it is empty only
/// inside that call.
pub struct DriveSlot(Option<DrivingState>);

impl std::ops::Deref for DriveSlot {
    type Target = DrivingState;

    fn deref(&self) -> &DrivingState {
        self.0
            .as_ref()
            .expect("the rig's drive is on the stack right now")
    }
}

impl std::ops::DerefMut for DriveSlot {
    fn deref_mut(&mut self) -> &mut DrivingState {
        self.0
            .as_mut()
            .expect("the rig's drive is on the stack right now")
    }
}

pub struct Rig {
    pub app: TestApp,
    pub drive: DriveSlot,
    last_game_minutes: f64,
    pub problems: Vec<String>,
    problem_keys: std::collections::HashSet<String>,
    /// The event pacer's clock, advanced by the frame step in [`Rig::step`].
    ///
    /// Same reason [`super::harness::PlaytestHarness`] has one, and the long
    /// note there is the explanation: the pacer drops a queued ambient line
    /// that would start speaking seconds after the moment it described, and
    /// it measures those seconds in REAL time because that is the only time
    /// a player's ear has. A rig that runs a scenario's whole stretch of
    /// road in a second of wall clock tells the pacer every ambient line is
    /// minutes late, and the battery then reads a silence no player would
    /// ever hear as the game's behaviour. Frames here are worth [`DT`]
    /// seconds; the clock says so.
    clock: FakeClock,
}

impl Rig {
    pub fn new(opts: RigOptions) -> Rig {
        let mut app = TestApp::new();
        // See the `clock` field: simulated time, not the wall clock this rig
        // outruns by three orders of magnitude.
        let clock = app.fake_pacer_clock();
        app.ctx.settings.automatic_transmission = opts.automatic;
        // No station machinery, no network.
        app.ctx.settings.radio_enabled = false;
        // A career's current_city is a slug ("buffalo_ny_us"). The world
        // resolves the old display name for routing, so a label here drives
        // fine and only shows up later, when a save made from this harness is
        // refused by cloud backup as an unknown city.
        let origin = app.ctx.world.resolve_city_key("Buffalo");
        let mut profile = Profile::named_in("Breaker", &origin);
        if let Some(business) = &opts.business {
            profile.business_status = business.clone();
        }
        app.ctx.profile = Some(profile);
        let route = app
            .ctx
            .world
            .supported_route("Buffalo", "Rochester", None)
            .ok()
            .flatten()
            .expect("Buffalo to Rochester is a supported route");
        let mut job = Job::new(
            cargo_type("general").expect("general freight is in the catalog"),
            opts.tons,
            "Buffalo",
            "company yard",
            "Rochester",
            route.miles(),
            1000.0,
            12.0,
        );
        job.destination_location = "Rochester freight market".to_string();
        let mut drive = DrivingState::new(
            &mut app.ctx,
            job,
            route,
            Some(opts.seed),
            DRIVE_PHASE_DELIVERY,
            None,
        );
        drive.tutorial = None;
        // Stay on the highway trip, no street chain.
        drive.departure_checked = true;
        drive.trip.hazard_check_mi = 1e18;
        drive.trip.inspection_check_mi = 1e18;
        drive.trip.conditions_check_mi = 1e18;
        drive.trip.traffic_manager.vehicles.clear();
        drive.trip.traffic_pressures.clear();
        if !opts.keep_patrols {
            drive.trip.set_patrols(Vec::new());
        }
        // Pin the weather: current stays whatever the scenario sets.
        // `forced` is the lock the Python rig faked by replacing
        // `weather.update`: the condition stays whatever the scenario sets.
        drive.weather_mut().forced = Some(WeatherKind::Clear);
        drive.weather_mut().current = WeatherKind::Clear;
        let mut rig = Rig {
            app,
            drive: DriveSlot(Some(drive)),
            last_game_minutes: 0.0,
            problems: Vec::new(),
            problem_keys: std::collections::HashSet::new(),
            clock,
        };
        if let Some(tweak) = RIG_TWEAK.with(|slot| slot.borrow().clone()) {
            tweak(&mut rig);
        }
        rig
    }

    // -- speech ------------------------------------------------------------------

    /// Everything the cab has said, event lines marked.
    ///
    /// The capture sits at the VOICE layer, not on `ctx.say`: the driving
    /// verbosity ladder's gate and the event pacer's repeat/backlog handling
    /// both live inside `GameContext::say`/`say_event`, and a rig that
    /// replaced those (the old approach) showed what the game would say with
    /// no rung applied and no repeat suppression running, not what a player
    /// actually hears.
    pub fn transcript(&self) -> Vec<String> {
        self.app.speech().transcript_lines()
    }

    pub fn said(&self, phrase: &str) -> usize {
        self.transcript()
            .iter()
            .filter(|line| line.contains(phrase))
            .count()
    }

    /// How many times the GAME announced `phrase`, as against how many times
    /// the voice received it ([`Self::said`]).
    ///
    /// The two differ by the pacer's hand-backs: a ROUTE line cut by an
    /// interrupt is delivered again to finish it, and the capture records
    /// that as a second line. A driver whose line was cut the instant it
    /// started hears it once. Use this for "did the game decide this twice",
    /// and `said` for "how many times did it reach the ear" -- a line handed
    /// back over and over is a chant, and that one is `said`'s to catch.
    pub fn announced(&self, phrase: &str) -> usize {
        self.said(phrase)
            .saturating_sub(self.app.ctx.handed_back_count(phrase))
    }

    pub fn lines_with(&self, phrase: &str) -> Vec<String> {
        self.transcript()
            .into_iter()
            .filter(|line| line.contains(phrase))
            .collect()
    }

    // -- driving -----------------------------------------------------------------

    pub fn prepare(&mut self, speed_mph: f64, gear: Option<i32>) {
        self.drive.truck_mut().start_engine();
        self.drive.truck_mut().set_air_ready(false);
        self.drive.truck_mut().velocity_mps = speed_mph / MPH_PER_MPS;
        match gear {
            Some(gear) => self.drive.truck_mut().transmission.gear = gear,
            None => {
                if self.drive.truck().transmission.automatic && speed_mph > 5.0 {
                    self.drive.truck_mut().transmission.gear = 8;
                }
            }
        }
    }

    pub fn press(&mut self, key: Key) {
        let event: InputEvent = key_event(key, None);
        self.drive.handle_key_event(&mut self.app.ctx, &event);
        self.app.ctx.run_deferred();
    }

    pub fn hold(&mut self, key: Key) {
        self.app.ctx.input.press(key, Mods::NONE);
    }

    pub fn release(&mut self, key: Key) {
        self.app.ctx.input.release(key, Mods::NONE);
    }

    /// Hand the event pacer `seconds` of simulated real time.
    ///
    /// [`Rig::step`] does this for the frames it runs; a scenario that
    /// drives `update_frame` itself has to keep the same books, or the
    /// pacer thinks those frames took no time (see the `clock` field).
    pub fn advance_clock(&self, seconds: f64) {
        self.clock.advance(seconds);
    }

    /// Run full `update_frame` frames; returns frames actually run. `until`
    /// stops early the first time it answers true.
    pub fn step(&mut self, frames: usize, dt: f64, until: Option<&dyn Fn(&Rig) -> bool>) -> usize {
        for i in 0..frames {
            self.clock.advance(dt);
            self.drive.update_frame(&mut self.app.ctx, dt);
            self.app.ctx.run_deferred();
            if i % 10 == 0 {
                self.check_invariants();
            }
            if until.is_some_and(|f| f(self)) {
                self.check_invariants();
                return i + 1;
            }
        }
        self.check_invariants();
        frames
    }

    /// `step(frames)` at the battery's default step, no early exit.
    pub fn run_frames(&mut self, frames: usize) -> usize {
        self.step(frames, DT, None)
    }

    // -- screens the drive pushes ----------------------------------------------------

    /// Put the drive on the app's state stack for the duration of `f`.
    ///
    /// The Python rig could hand `self.d` straight to `ctx.push_state` and to
    /// a screen's constructor, because a Python object is already a shared
    /// reference. Here the drive is owned by the rig, so this moves it into a
    /// shared cell, pushes that (entering it, as `ctx.push_state(d)` did),
    /// runs `f` with a [`DriveRef`] onto the same object, and moves it back.
    ///
    /// A screen that pops the drive off the stack itself -- the abandon
    /// confirmation replaces it with the city menu -- is fine: what comes
    /// back is still the same drive, because the rig held the only other
    /// handle on it. Anything still on the stack above the drive is popped
    /// here, so the next scenario step starts from a clean stack.
    pub fn with_drive_on_stack<R>(&mut self, f: impl FnOnce(&mut Rig, DriveRef) -> R) -> R {
        let drive = self
            .drive
            .0
            .take()
            .expect("the drive is already on the stack");
        let host = Rc::new(RefCell::new(drive));
        let shared: SharedState = host.clone();
        self.app.ctx.push_shared(Rc::clone(&shared));
        self.app.ctx.run_deferred();
        let handle = DriveRef::of(&shared);
        let out = f(self, handle);
        self.app.ctx.run_deferred();
        // Unwind whatever is still above (or is) the drive, without entering
        // anything on the way out: the rig owns the stack again from here.
        while self.app.ctx.state().is_some() {
            let top_is_drive = self
                .app
                .ctx
                .state()
                .is_some_and(|state| Rc::ptr_eq(&state, &shared));
            self.app.ctx.pop_state_with(false, false);
            if top_is_drive {
                break;
            }
        }
        self.app.ctx.run_deferred();
        drop(shared);
        let drive = Rc::try_unwrap(host)
            .unwrap_or_else(|_| panic!("something kept a handle on the rig's drive"))
            .into_inner();
        self.drive.0 = Some(drive);
        out
    }

    // -- menus -----------------------------------------------------------------------

    /// Press a key at whatever screen is on top (not at the wheel).
    pub fn key_screen(&mut self, key: Key) {
        let event = key_event(key, None);
        self.app.dispatch_to_state(&event);
        self.app.ctx.run_deferred();
    }

    /// Every option on the current screen.
    pub fn menu_labels(&self) -> Vec<String> {
        match self.app.ctx.state() {
            Some(state) => super::menu::menu_labels_of(&*state.borrow(), &self.app.ctx),
            None => Vec::new(),
        }
    }

    fn focused_label(&self) -> Option<String> {
        let state = self.app.ctx.state()?;
        let borrowed = state.borrow();
        let (labels, index) = super::menu::menu_rows(&*borrowed, &self.app.ctx)?;
        labels.get(index).cloned()
    }

    /// Arrow down to the first row whose label CONTAINS `needle`, and press it.
    ///
    /// Contains rather than equals because the rows this battery reaches
    /// quote live numbers ("Motel room: sleep 10 hours for 95 dollars").
    /// Returns false when no such row exists, which several scenarios read as
    /// a finding rather than a crash.
    pub fn select_menu_containing(&mut self, needle: &str) -> bool {
        let labels = self.menu_labels();
        if !labels.iter().any(|row| row.contains(needle)) {
            return false;
        }
        for _ in 0..=labels.len() {
            if self
                .focused_label()
                .is_some_and(|label| label.contains(needle))
            {
                self.key_screen(Key::Return);
                return true;
            }
            self.key_screen(Key::Down);
        }
        false
    }

    // -- invariants ----------------------------------------------------------------

    fn problem(&mut self, key: &str, text: String) {
        if self.problem_keys.insert(key.to_string()) {
            self.problems.push(text);
        }
    }

    pub fn check_invariants(&mut self) {
        let speed = self.drive.truck().velocity_mps;
        if !speed.is_finite() {
            self.problem("speed", format!("speed went non-finite: {speed}"));
        }
        let (money, fatigue) = match &self.app.ctx.profile {
            Some(profile) => (profile.money(), profile.fatigue),
            None => (0.0, 0.0),
        };
        if !money.is_finite() {
            self.problem("money", format!("money went non-finite: {money}"));
        }
        let position = self.drive.trip.position_mi;
        let total = self.drive.trip.total_miles();
        if !(0.0..=total + 1e-6).contains(&position) {
            self.problem(
                "pos",
                format!("position {position:.2} outside [0, {total:.2}]"),
            );
        }
        let minutes = self.drive.trip.game_minutes;
        if minutes < self.last_game_minutes - 1e-9 {
            self.problem("clock", "trip clock ran backward".to_string());
        }
        self.last_game_minutes = minutes;
        let truck = self.drive.truck();
        let bands = [
            ("damage", truck.damage_pct),
            ("tire wear", truck.tire_wear_pct),
            ("brake wear", truck.brake_wear_pct),
            ("engine wear", truck.engine_wear_pct),
        ];
        for (label, value) in bands {
            if !(0.0..=100.0 + 1e-6).contains(&value) {
                self.problem(label, format!("{label} out of range: {value}"));
            }
        }
        let (fuel, tank) = {
            let truck = self.drive.truck();
            (truck.fuel_gal, truck.specs.fuel_tank_gal)
        };
        if !(0.0..=tank + 1e-6).contains(&fuel) {
            self.problem("fuel", format!("fuel out of range: {fuel}"));
        }
        if !(0.0..=100.0 + 1e-6).contains(&fatigue) {
            self.problem("fatigue", format!("fatigue out of range: {fatigue}"));
        }
    }
}

/// Print the battery's summary table the way the Python CLI did.
pub fn print_summary(outcomes: &[Outcome]) {
    println!();
    println!("{}", "=".repeat(100));
    println!("{:<34} {:<8} note", "scenario", "verdict");
    println!("{}", "-".repeat(100));
    for outcome in outcomes {
        let note = outcome.note.replace('\n', " ");
        let note: String = note.chars().take(120).collect();
        println!(
            "{:<34} {:<8} {note}",
            outcome.name,
            outcome.verdict.as_str()
        );
    }
    println!("{}", "=".repeat(100));
    let odd = outcomes
        .iter()
        .filter(|o| o.verdict == Verdict::Odd)
        .count();
    let err = outcomes
        .iter()
        .filter(|o| o.verdict == Verdict::Error)
        .count();
    println!(
        "{} scenarios: {} clean, {odd} odd, {err} errors",
        outcomes.len(),
        outcomes.len() - odd - err
    );
}
