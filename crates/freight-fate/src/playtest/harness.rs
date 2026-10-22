//! Headless playtest harness for transcript-backed gameplay verification
//! (port of `tests/playtest_harness.py`).
//!
//! # Shape of the port
//!
//! Python's harness was a context manager holding an `App`, with
//! `monkeypatch` swapping `ctx.say`/`ctx.say_event` for recorders. Here the
//! app is a [`TestApp`] -- the same headless rig every state test uses --
//! and the recorder is the [`CaptureSpeech`] already sitting under
//! `ctx.speech`.
//!
//! That seam is one rung LOWER than the Python harness's, and deliberately
//! so. The driving verbosity ladder's gate and the event pacer's
//! repeat/backlog handling both live *inside* `GameContext::say`/`say_event`;
//! replacing those (what the Python harness did) skipped both, so its
//! transcript showed what the game would say with no rung applied and no
//! repeat suppression running, not what a player actually hears.
//! `tools/playtest_break.py` had already moved its own rig down to
//! `ctx.speech` for exactly that reason, with a long note saying why. This
//! harness starts there.
//!
//! `self.driving` was a live reference to the state on the stack; here it is
//! a [`SharedState`] handle and [`PlaytestHarness::with_drive`] borrows the
//! [`DrivingState`] out of it. The app's own stack holds another `Rc` to the
//! same cell, so the borrow is safe for as long as the harness does not also
//! ask the app to tick the state -- which it never does, because every drive
//! entry point steps the state directly, exactly as the Python did.

use std::cell::RefCell;
use std::rc::Rc;

use ff_core::models::carrier_fleet::slip_seat_pool;
use ff_core::models::jobs::{cargo_type, Job};
use ff_core::models::profile::Profile;
use ff_core::models::trailer_yard::{delivery_plan, pickup_plan};
use ff_core::models::trucks::truck_model;
use ff_core::sim::trip_models::{
    NPCVehicle, TrafficPressure, TripEvent, TripEventData, TripEventKind, Zone,
};
use ff_core::sim::weather::WeatherKind;

use crate::app::testing::{FakeClock, TestApp};
use crate::app::{GameContext, SharedState};
use crate::speech::SpokenEntry;
use crate::states::base::{InputEvent, Key, Menu, Mods, TimedMessageState};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{DRIVE_PHASE_DELIVERY, RAMP_MAX_MPH};

use super::menu::{menu_labels_of, menu_rows};
use super::MPH_PER_MPS;

/// One frame at the fixed rate every step in this module uses.
const DT: f64 = 1.0 / 60.0;

/// `key_event(key, unicode="")`: the pygame `KEYDOWN` the Python harness
/// built for every press.
pub fn key_event(key: Key, text: Option<char>) -> InputEvent {
    InputEvent::KeyDown {
        key,
        mods: Mods::NONE,
        text,
        repeat: false,
    }
}

// -- the result ----------------------------------------------------------------------

/// Everything one playtest run produced: what was heard, and the gameplay
/// numbers a transcript alone cannot show.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PlaytestResult {
    /// The spoken lines in order, event lines marked `"[event] "`.
    pub transcript: Vec<String>,
    pub spoken: Vec<SpokenEntry>,
    pub deliveries: i64,
    pub destination: String,
    pub current_city: String,
    pub remaining_miles: f64,
    pub speeding_tickets: i64,
    pub inspection_fines: i64,
    pub speed_control_transitions: Vec<String>,
    /// The furthest the truck ever ran continuously over the limit. Distance,
    /// not seconds: an officer reads a speed over a stretch of road, and that
    /// stretch is the same stretch at every pacing. Replaces a real-time timer
    /// that measured the silent at-delivery charge, which no longer exists.
    pub max_over_limit_mi: f64,
    pub construction_entry_speed_mph: Option<f64>,
    pub heavy_traffic_entry_speed_mph: Option<f64>,
    pub destination_exit_speed_mph: Option<f64>,
    // Equipment and freight handling. A transcript alone cannot tell you which
    // tractor dispatch drew, whether the shipper staged a trailer or loaded at
    // a dock, or what was wrong with the box -- and those now decide how a run
    // reads, so a playtest that cannot see them is reviewing half the game.
    pub assigned_truck: String,
    pub assigned_truck_label: String,
    pub slip_seat_pool: Vec<String>,
    pub pickup_mode: String,
    pub pickup_minutes: f64,
    pub detention_minutes: f64,
    pub detention_pay: f64,
    pub trailer_number: String,
    pub trailer_condition: String,
    pub trailer_defect: String,
    pub trailer_refused: bool,
    pub delivery_mode: String,
    pub delivery_minutes: f64,
}

impl PlaytestResult {
    pub fn transcript_text(&self) -> String {
        self.transcript.join("\n")
    }

    pub fn assert_no_known_destination_exit_regressions(&self) {
        let lower: Vec<String> = self.transcript.iter().map(|l| l.to_lowercase()).collect();
        let destination_exit_lines = lower
            .iter()
            .filter(|line| {
                line.contains("destination exit") || line.contains("exit for the destination")
            })
            .count();
        assert!(destination_exit_lines <= 1, "{}", self.transcript_text());
        assert!(
            !lower
                .iter()
                .any(|line| has_word_run(line, "21 miles remaining")),
            "{}",
            self.transcript_text()
        );
        assert_eq!(self.remaining_miles, 0.0);
    }

    /// Assert phrases occur in order, allowing unrelated speech between them.
    pub fn assert_ordered(&self, phrases: &[&str]) {
        let mut cursor = 0usize;
        for phrase in phrases {
            let found = self.transcript[cursor..]
                .iter()
                .position(|line| line.contains(phrase))
                .map(|offset| cursor + offset + 1);
            match found {
                Some(next) => cursor = next,
                None => panic!(
                    "Missing or out-of-order phrase {phrase:?}\n{}",
                    self.transcript_text()
                ),
            }
        }
    }

    pub fn assert_screen_reader_friendly(&self) {
        assert!(!self.transcript.is_empty());
        assert!(self
            .transcript
            .iter()
            .all(|line| line.trim() == line && !line.is_empty()));
        let text = self.transcript_text().to_lowercase();
        for marker in ["osm_id", "amenity=", "highway=", "node/", "way/"] {
            assert!(!text.contains(marker), "raw map data spoken: {marker}");
        }
        for (i, entry) in self.spoken.iter().enumerate() {
            assert_eq!(entry.sequence, i as u64, "transcript lost a line");
        }
    }
}

/// `re.search(r"\bPHRASE\b", line)` for the one phrase that used it.
fn has_word_run(line: &str, phrase: &str) -> bool {
    let mut from = 0usize;
    while let Some(at) = line[from..].find(phrase) {
        let start = from + at;
        let end = start + phrase.len();
        let before_ok = start == 0
            || !line[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        let after_ok = end == line.len()
            || !line[end..]
                .chars()
                .next()
                .is_some_and(|c| c.is_alphanumeric() || c == '_');
        if before_ok && after_ok {
            return true;
        }
        from = start + 1;
    }
    false
}

// -- entry-point options --------------------------------------------------------------

/// The keyword arguments of `start_route`.
pub struct RouteSetup {
    pub profile_name: String,
    pub cargo: String,
    pub tons: f64,
    /// The whole city chain when the caller wants a particular routing;
    /// must start at `origin` and end at `destination`.
    pub route_cities: Option<Vec<String>>,
    pub trip_seed: Option<i64>,
    /// The delivery's facility at the far end, when the caller needs a
    /// particular one. `None` uses the terminal, which is what dispatch's
    /// own default draws. It has to be set BEFORE the drive is built: the
    /// trip sizes its arrival approach from the facility's own record at
    /// construction, so a facility assigned afterwards is measured against
    /// the terminal's approach for the rest of the run.
    pub destination_location: Option<String>,
    /// The facility the load is picked up FROM, when the caller needs a
    /// particular one. `None` uses the terminal. Same rule as its arrival
    /// twin: the departure chain is chosen off this on the drive's very
    /// first frame, so a facility assigned later is never the one driven.
    pub origin_location: Option<String>,
}

impl Default for RouteSetup {
    fn default() -> Self {
        RouteSetup {
            profile_name: "Route Playtest".to_string(),
            cargo: "general".to_string(),
            tons: 18.0,
            route_cities: None,
            trip_seed: None,
            destination_location: None,
            origin_location: None,
        }
    }
}

impl RouteSetup {
    /// `start_route(..., trip_seed=seed)`, the transcript suite's shape.
    pub fn seeded(seed: i64) -> Self {
        RouteSetup {
            trip_seed: Some(seed),
            ..Default::default()
        }
    }

    pub fn named(mut self, profile_name: &str) -> Self {
        self.profile_name = profile_name.to_string();
        self
    }

    pub fn cities(mut self, cities: &[&str]) -> Self {
        self.route_cities = Some(cities.iter().map(|c| c.to_string()).collect());
        self
    }

    /// Deliver to this facility rather than the destination's terminal.
    pub fn destination_location(mut self, location: &str) -> Self {
        self.destination_location = Some(location.to_string());
        self
    }

    /// Load at this facility rather than the origin's terminal.
    pub fn origin_location(mut self, location: &str) -> Self {
        self.origin_location = Some(location.to_string());
        self
    }
}

/// `configure_profile`: shape the career before the dispatch board opens.
pub type ConfigureProfile = Box<dyn FnOnce(&mut Profile)>;

/// The keyword arguments of `start_delivery`.
pub struct StartDelivery {
    pub profile_name: String,
    pub job_rank: usize,
    pub route_rank: usize,
    pub configure_profile: Option<ConfigureProfile>,
    pub stop_at_pickup: bool,
    pub arm_speed_control_on_deadhead: bool,
    pub walk_around_trailer: bool,
    pub refuse_bad_trailer: bool,
}

impl Default for StartDelivery {
    fn default() -> Self {
        StartDelivery {
            profile_name: "Playtest".to_string(),
            job_rank: 0,
            route_rank: 0,
            configure_profile: None,
            stop_at_pickup: false,
            arm_speed_control_on_deadhead: false,
            walk_around_trailer: false,
            refuse_bad_trailer: false,
        }
    }
}

impl StartDelivery {
    pub fn named(profile_name: &str) -> Self {
        StartDelivery {
            profile_name: profile_name.to_string(),
            ..Default::default()
        }
    }

    pub fn configure(mut self, f: impl FnOnce(&mut Profile) + 'static) -> Self {
        self.configure_profile = Some(Box::new(f));
        self
    }
}

// -- the harness ----------------------------------------------------------------------

/// Drive real game states under test without opening a visible window.
pub struct PlaytestHarness {
    /// The headless app, its isolated data directory and its capture.
    pub app: TestApp,
    /// The metrics half of [`PlaytestResult`]; the transcript half is read
    /// live off the capture by [`PlaytestHarness::result`].
    metrics: PlaytestResult,
    driving: Option<SharedState>,
    /// The event pacer's clock, advanced by [`DT`] at every site here that
    /// steps a frame. Do not "simplify" this back to the wall clock.
    ///
    /// The pacer budgets in REAL seconds because a player can only take in
    /// so many words per second: a queued ambient line that would start
    /// speaking more than `FF_PACER.ambient_wait_s` after the
    /// moment it described is dropped, because by then the truck is past the
    /// thing it was about. That is right for a player, whose truck covers
    /// half a mile in those three seconds.
    ///
    /// A harness that simulates frames covers 534 miles of road in about
    /// fourteen wall-clock seconds. On the real clock every ambient line on
    /// a long delivery therefore looks minutes late the instant it is
    /// submitted, and the pacer -- correctly, given what it was told --
    /// dropped nearly all of them: measured on one seeded
    /// Indianapolis-Nashville-Atlanta delivery, 75 ambient lines dropped
    /// against 32 spoken, the city passings and state crossings among the
    /// casualties. The same delivery with this clock drops 5 and speaks
    /// 106. Nothing was wrong with the road, the lines, or the pacer; the
    /// harness was lying to it about how much time the drive had spent.
    ///
    /// So the harness gives the pacer the same real-time budget the drive it
    /// is simulating would give it: one sixtieth of a second per simulated
    /// frame. Advance it at EVERY frame-stepping site -- miss one and the
    /// drops come back only on the paths that go through it, which is worse
    /// than losing them everywhere.
    ///
    /// (The Python harness never had this problem because it recorded ABOVE
    /// the pacer, which is exactly the seam this port moved down on purpose;
    /// see the module note.)
    clock: FakeClock,
}

impl Default for PlaytestHarness {
    fn default() -> Self {
        Self::new()
    }
}

impl PlaytestHarness {
    /// `with PlaytestHarness(monkeypatch) as harness`.
    ///
    /// The one-time orinks.net offer (`should_offer_online`) is gated on
    /// `settings.online_offer_seen` OR a stored account identity -- and that
    /// identity lives in the platform secret store (keyring), not under
    /// `FREIGHT_FATE_DATA_DIR`. So a fresh/temp data dir alone does not make
    /// a playtest deterministic: on a machine with no stored identity the
    /// offer appears and `start_delivery`'s first assert fails; on a
    /// developer's own machine with a stored identity it happens to be
    /// skipped, silently masking the gap. [`TestApp`] seeds the gate before
    /// the app reads settings so every run behaves the same regardless of
    /// who is signed in -- a playtest must not depend on the operator's
    /// login state, and the harness must never press through the offer
    /// itself (that would silently answer a consent prompt on the player's
    /// behalf and make the transcript misleading).
    pub fn new() -> PlaytestHarness {
        let mut app = TestApp::new();
        // See the `clock` field: the pacer must be on simulated time, not on
        // the wall clock this harness outruns.
        let clock = app.fake_pacer_clock();
        PlaytestHarness {
            app,
            metrics: PlaytestResult::default(),
            driving: None,
            clock,
        }
    }

    /// One simulated frame of real time for the event pacer.
    ///
    /// Every frame-stepping entry point calls this exactly once per frame it
    /// runs. Public because a test that steps `update_frame` itself (rather
    /// than through [`PlaytestHarness::drive_frames`]) has to keep the same
    /// books, or its own frames are the ones the pacer thinks took no time.
    pub fn advance_frame_clock(&self) {
        self.clock.advance(DT);
    }

    /// Advance the pacer's clock by an arbitrary number of seconds.
    ///
    /// For a test staging a gap the drive did not simulate -- a long wait at
    /// a dock, a pause off the road -- so the next line is judged against
    /// the time a player would really have spent there.
    pub fn advance_clock(&self, seconds: f64) {
        self.clock.advance(seconds);
    }

    /// What the event pacer currently thinks the time is.
    pub fn clock_now(&self) -> f64 {
        self.clock.now()
    }

    // -- reading the run --------------------------------------------------------------

    /// A snapshot of the run: the metrics collected so far plus the whole
    /// transcript as it stands.
    pub fn result(&self) -> PlaytestResult {
        let capture = self.app.speech();
        PlaytestResult {
            transcript: capture.transcript_lines(),
            spoken: capture.entries().to_vec(),
            ..self.metrics.clone()
        }
    }

    /// The transcript lines only.
    pub fn transcript(&self) -> Vec<String> {
        self.app.speech().transcript_lines()
    }

    pub fn transcript_text(&self) -> String {
        self.transcript().join("\n")
    }

    /// Everything recorded so far, both channels.
    pub fn spoken(&self) -> Vec<SpokenEntry> {
        self.app.speech().entries().to_vec()
    }

    /// Drop everything said so far (the Python `result.transcript.clear()`).
    pub fn clear_speech(&self) {
        self.app.clear_speech();
    }

    // -- reaching the drive -----------------------------------------------------------

    /// The drive under the harness, as the app holds it.
    pub fn shared_driving(&self) -> Option<SharedState> {
        self.driving.clone()
    }

    /// Whether a drive has been started.
    pub fn has_drive(&self) -> bool {
        self.driving.is_some()
    }

    /// Run `f` on the active drive and the context (`harness.driving`).
    ///
    /// Panics when no drive has been started, which is the Python
    /// `assert self.driving is not None`.
    ///
    /// On the way out it puts the road-event breather back on the harness's
    /// simulated clock. `RoadEventBreather` holds off a repeat limit, traffic
    /// or zone line for 10 to 15 REAL seconds, and this harness drives
    /// minutes of road in a fraction of one; left on the wall clock it
    /// suppresses almost every such line, and exactly how many survive
    /// depends on how fast the machine ran the loop -- a test that passes
    /// alone and fails in a loaded suite. Four different tests were failing
    /// that way, one per run and never the same one twice, which is what
    /// first put the wiring in at the entry points.
    ///
    /// It belongs HERE and not only there because a fresh `Trip` carries a
    /// fresh breather on the wall clock, and every bench road and every
    /// street chain builds its trip and assigns it straight onto the drive.
    /// Wired only at `start_delivery`, the very next `with_drive` that laid a
    /// bench road tore the wiring out again -- and the exit-call audit in
    /// `transcript_spoken_truth` was still losing a call about one full-suite
    /// run in five because of it (2026-08-24).
    pub fn with_drive<R>(&mut self, f: impl FnOnce(&mut DrivingState, &mut GameContext) -> R) -> R {
        let shared = self
            .driving
            .clone()
            .expect("start_route or start_delivery has not put a drive on the stack");
        let mut borrowed = shared.borrow_mut();
        let drive = borrowed
            .as_any_mut()
            .downcast_mut::<DrivingState>()
            .expect("the harness's drive handle is a DrivingState");
        let out = f(drive, &mut self.app.ctx);
        drive.trip.event_breather.set_clock(self.clock.boxed());
        drop(borrowed);
        self.app.ctx.run_deferred();
        out
    }

    /// Read the drive without touching the context.
    pub fn read_drive<R>(&self, f: impl FnOnce(&DrivingState) -> R) -> R {
        let shared = self
            .driving
            .clone()
            .expect("start_route or start_delivery has not put a drive on the stack");
        let borrowed = shared.borrow();
        let drive = borrowed
            .as_any()
            .downcast_ref::<DrivingState>()
            .expect("the harness's drive handle is a DrivingState");
        f(drive)
    }

    /// Whether the active state is a `T`.
    pub fn state_is<T: 'static>(&self) -> bool {
        self.app
            .state()
            .is_some_and(|state| state.borrow().as_any().downcast_ref::<T>().is_some())
    }

    /// Assert the active state is a `T`, showing what it actually is and
    /// what the screen was saying -- an assertion this far into a drive is
    /// unreadable without both.
    pub fn expect_state<T: 'static>(&self, why: &str) {
        if self.state_is::<T>() {
            return;
        }
        let lines = self.app.visible_lines().join(" | ");
        panic!(
            "{why}: expected {}, screen shows [{lines}]
{}",
            std::any::type_name::<T>(),
            self.transcript_text()
        );
    }

    /// Run `f` on the active state downcast to `T`.
    pub fn with_state<T: 'static, R>(
        &mut self,
        f: impl FnOnce(&mut T, &mut GameContext) -> R,
    ) -> R {
        let state = self.app.state().expect("a state on the stack");
        let mut borrowed = state.borrow_mut();
        let typed = borrowed
            .as_any_mut()
            .downcast_mut::<T>()
            .unwrap_or_else(|| panic!("active state is not a {}", std::any::type_name::<T>()));
        let out = f(typed, &mut self.app.ctx);
        drop(borrowed);
        self.app.ctx.run_deferred();
        out
    }

    // -- the timed-state pump ---------------------------------------------------------

    /// `_finish_timed_state`: run a brief spoken transition out.
    ///
    /// A [`TimedMessageState`] only counts down; driving frames at it would
    /// step the whole app for no reason and, worse, let the drive underneath
    /// keep rolling. Step the state itself until its countdown ends, exactly
    /// as `app.state.update(1/60)` did.
    pub fn finish_timed_state(&mut self) {
        loop {
            let Some(state) = self.app.ctx.state() else {
                return;
            };
            let remaining = state
                .borrow()
                .as_any()
                .downcast_ref::<TimedMessageState>()
                .map(|timed| timed.remaining)
                .unwrap_or(0.0);
            if remaining <= 0.0 {
                return;
            }
            self.advance_frame_clock();
            state.borrow_mut().update(&mut self.app.ctx, DT);
            self.app.ctx.run_deferred();
        }
    }

    // -- starting a run ---------------------------------------------------------------

    /// Take the wheel exactly where `--playtest-road --find FEATURE` would.
    ///
    /// The whole value of a finder is WHERE it drops you, and that is decided
    /// by [`road::build_driving`][super::road::build_driving] -- the job it
    /// builds, the seed it carries over from the search, and the lead it
    /// subtracts. A test that staged an equivalent drive by hand would be
    /// checking its own arithmetic, not the tool's, so this runs the tool's
    /// own construction and hands back the mile it chose.
    ///
    /// Returns the starting mile.
    pub fn start_road_feature(
        &mut self,
        hit: &super::road::Hit,
        opts: &super::road::RoadOptions,
    ) -> f64 {
        let (drive, start_mi) = super::road::build_driving(&mut self.app.ctx, hit, opts);
        self.app.push_state(drive);
        self.driving = self.app.state();
        self.neutralize_random_trip_friction();
        start_mi
    }

    /// Set up a delivery on a specific supported route, skipping the menus.
    ///
    /// Useful for exercising one corridor's routing/data (e.g. a leg whose
    /// geometry changed) rather than whatever job the dispatch board offers.
    /// Pair with [`PlaytestHarness::drive_delivery_to_completion`].
    pub fn start_route(
        &mut self,
        origin: &str,
        destination: &str,
        setup: RouteSetup,
    ) -> PlaytestResult {
        self.app.ctx.profile = Some(Profile::named_in(&setup.profile_name, origin));
        let owned: Vec<String> = setup
            .route_cities
            .clone()
            .unwrap_or_else(|| vec![origin.to_string(), destination.to_string()]);
        if owned.first().map(String::as_str) != Some(origin)
            || owned.last().map(String::as_str) != Some(destination)
        {
            panic!("route_cities must start at origin and end at destination");
        }
        let route = self
            .app
            .ctx
            .world
            .route_from_cities(&owned)
            .unwrap_or_else(|| panic!("No supported route {}", owned.join(" -> ")));
        let miles = route.miles().round();
        let cargo = cargo_type(&setup.cargo)
            .unwrap_or_else(|| panic!("{:?} is not in the cargo catalog", setup.cargo));
        // The dispatch board fills the spoken endpoints in; this seam skips
        // the board, and without them every line that names the city aloud
        // fell back to the map key -- "in hattiesburg_ms_us" on the
        // missed-exit call, and "hattiesburg_ms_us Terminal" in the opening
        // summary. A transcript test cannot tell that placeholder from a real
        // defect, so the seam speaks the same names a real job does. Resolved
        // first because callers hand this either a key or a display name.
        let origin_spoken = {
            let key = self.app.ctx.world.resolve_city_key(origin);
            self.app.ctx.world.spoken_city(&key, None)
        };
        let destination_spoken = {
            let key = self.app.ctx.world.resolve_city_key(destination);
            self.app.ctx.world.spoken_city(&key, None)
        };
        let origin_location = setup
            .origin_location
            .clone()
            .unwrap_or_else(|| format!("{origin_spoken} Terminal"));
        let mut job = Job::new(
            cargo,
            setup.tons,
            origin,
            &origin_location,
            destination,
            miles,
            (miles * 10.0).max(500.0),
            (miles / 25.0).max(2.0),
        );
        job.destination_location = setup
            .destination_location
            .clone()
            .unwrap_or_else(|| format!("{destination_spoken} Terminal"));
        job.origin_spoken = origin_spoken;
        job.destination_spoken = destination_spoken;
        let drive = DrivingState::new(
            &mut self.app.ctx,
            job,
            route,
            setup.trip_seed,
            DRIVE_PHASE_DELIVERY,
            None,
        );
        self.app.push_state(drive);
        self.driving = self.app.state();
        self.neutralize_random_trip_friction();
        self.result()
    }

    /// Walk the real menus from the main menu to the wheel, loaded.
    ///
    /// Every step is a keypress on the state the app actually has, so a
    /// screen that grows a row does not silently move what the harness
    /// presses.
    pub fn start_delivery(&mut self, setup: StartDelivery) -> PlaytestResult {
        use crate::states::city::{CityMenuState, JobBoardState, RouteSelectState};
        use crate::states::city_pickup::PickupFacilityState;
        use crate::states::main_menu::MainMenuState;

        self.app.push_state(MainMenuState::new());
        self.select_current_menu_text("New career");
        for ch in setup.profile_name.chars() {
            let key = if ch == ' ' {
                Key::Space
            } else {
                Key::from_char(ch)
            };
            self.key(key_event(key, Some(ch)));
        }
        self.key(key_event(Key::Return, None));
        // Career start, home terminal, home city: three confirmations.
        self.key(key_event(Key::Return, None));
        self.key(key_event(Key::Return, None));
        self.key(key_event(Key::Return, None));
        assert!(
            self.state_is::<CityMenuState>(),
            "the new-career flow did not land at the terminal"
        );
        if let Some(configure) = setup.configure_profile {
            let profile = self
                .app
                .ctx
                .profile
                .as_mut()
                .expect("a new career has a profile");
            configure(profile);
        }

        self.key(key_event(Key::Return, None));
        assert!(self.state_is::<JobBoardState>(), "Enter opened no board");
        let assigned = self.with_state::<JobBoardState, _>(|board, _| board.assigned_mode());
        if assigned {
            // New company hires are assigned a load: job_rank spends declines
            // to reach an alternative instead of browsing the board.
            self.accept_assigned_job(setup.job_rank, setup.arm_speed_control_on_deadhead);
        } else {
            self.choose_unlocked_job(setup.job_rank);
        }
        if self.state_is::<PickupFacilityState>() {
            // A load staged at the home yard opens the shipping office in
            // place: there is no deadhead to drive, so the truck is already
            // where the drive below would have taken it.
            assert!(
                !setup.arm_speed_control_on_deadhead,
                "this career's assigned load is staged at the yard; there is no                  deadhead to arm speed control on"
            );
        } else {
            assert!(self.state_is::<DrivingState>(), "the board did not drive");
            self.driving = self.app.state();
            assert_eq!(self.read_drive(|d| d.phase), "pickup");

            if setup.arm_speed_control_on_deadhead {
                self.with_drive(|drive, ctx| {
                    drive.truck_mut().start_engine();
                    drive.truck_mut().set_air_ready(false);
                    drive.truck_mut().velocity_mps = 5.0;
                    drive.handle_key_event(ctx, &key_event(Key::K, None));
                    assert!(drive.speed_control_armed);
                });
            }

            self.with_drive(|drive, _| {
                drive.trip.position_mi = drive.trip.total_miles();
                drive.trip.finished = true;
            });
            if setup.arm_speed_control_on_deadhead {
                self.advance_frame_clock();
                self.with_drive(|drive, ctx| {
                    drive.truck_mut().velocity_mps = 26.8;
                    drive.update_frame(ctx, DT);
                });
                assert!(self.state_is::<DrivingState>());
            }
            self.advance_frame_clock();
            self.with_drive(|drive, ctx| {
                drive.truck_mut().velocity_mps = 0.0;
                drive.truck_mut().set_parking_brake();
                drive.update_frame(ctx, DT);
            });
            self.finish_timed_state();
        }
        assert!(
            self.state_is::<PickupFacilityState>(),
            "stopping at the shipper opened no pickup menu"
        );
        self.record_equipment();
        if setup.stop_at_pickup {
            return self.result();
        }
        self.select_menu_item("Check in at shipping office");
        // Either way the freight gets aboard; naming both keeps the harness
        // working whichever kind of shipper dispatch drew.
        let labels = self.menu_labels();
        let load = ["Drop and hook in the yard", "Load cargo at dock"]
            .into_iter()
            .find(|label| labels.iter().any(|row| row == label))
            .unwrap_or_else(|| panic!("no way to load: {labels:?}"));
        self.select_menu_item(load);
        self.finish_timed_state();
        if setup.walk_around_trailer {
            self.walk_around_trailer();
            if setup.refuse_bad_trailer {
                self.refuse_trailer();
            }
        }
        self.record_equipment();
        self.select_menu_item("Depart for destination");
        if self.state_is::<RouteSelectState>() {
            // Owner-operators and authority choose their routing.
            self.choose_route(setup.route_rank);
        }
        // Company drivers run dispatch's assigned route: route_rank is unused.
        assert!(self.state_is::<DrivingState>(), "departure did not drive");
        self.driving = self.app.state();
        assert_eq!(self.read_drive(|d| d.phase), DRIVE_PHASE_DELIVERY);
        self.neutralize_random_trip_friction();
        self.result()
    }

    // -- driving --------------------------------------------------------------------

    /// Put the active delivery truck in a road-ready deterministic state.
    pub fn prepare_for_driving(&mut self, speed_mph: f64) {
        self.with_drive(|drive, ctx| {
            if !drive.truck().engine_on {
                drive.handle_key_event(ctx, &key_event(Key::E, None));
            }
            drive.truck_mut().transmission.automatic = true;
            drive.truck_mut().set_air_ready(false);
            drive.truck_mut().velocity_mps = speed_mph / 2.2369362920544;
        });
    }

    /// Press a key at the wheel (`self.driving.handle_event(...)`).
    pub fn press_key(&mut self, key: Key, text: Option<char>) {
        self.with_drive(|drive, ctx| drive.handle_key_event(ctx, &key_event(key, text)));
    }

    /// Press a key at whatever screen is on top.
    pub fn key(&mut self, event: InputEvent) {
        self.app.dispatch_to_state(&event);
    }

    pub fn drive_frames(&mut self, frames: usize) {
        for _ in 0..frames {
            self.drive_one_frame();
        }
    }

    /// Run real automatic speed control through a targeted route segment.
    pub fn drive_speed_control_segment(
        &mut self,
        start_mi: f64,
        end_mi: f64,
        set_mph: f64,
    ) -> PlaytestResult {
        self.with_drive(|drive, ctx| {
            drive.tutorial = None;
            drive.trip.position_mi = start_mi;
            // Keep patrol randomness from converting the same overspeed
            // incident into a traffic stop instead of the settlement strike
            // measured here.
            drive.trip.set_patrols(Vec::new());
            drive.truck_mut().start_engine();
            drive.truck_mut().set_air_ready(false);
            drive.truck_mut().transmission.automatic = true;
            drive.truck_mut().transmission.gear = 10;
            drive.truck_mut().velocity_mps = set_mph / MPH_PER_MPS;
            drive.truck_mut().throttle = 0.35;
            drive.handle_key_event(ctx, &key_event(Key::K, None));
        });

        let mut last_mode = String::new();
        // Budgeted for a road with traffic on it. Once the rolling bubble kept
        // the highway populated, adaptive cruise started doing its job -- the
        // truck follows slower vehicles instead of holding the set speed the
        // whole way -- and the same segment covered about 9 percent less ground
        // per frame. That is the feature working; the segment still completes,
        // it just takes the time a real one would.
        let mut reached = false;
        for _ in 0..120_000 {
            self.advance_frame_clock();
            let (mode, over_limit_mi, zone_reason, speed_mph, position_mi) =
                self.with_drive(|drive, ctx| {
                    drive.update_frame(ctx, DT);
                    let mode = speed_control_mode(drive);
                    let (_, reason) = drive.trip.speed_limit_at(drive.trip.position_mi);
                    (
                        mode,
                        drive.over_limit_mi,
                        reason.unwrap_or_default(),
                        drive.truck().speed_mph(),
                        drive.trip.position_mi,
                    )
                });
            if mode != last_mode {
                self.metrics.speed_control_transitions.push(mode.clone());
                last_mode = mode;
            }
            self.metrics.max_over_limit_mi = self.metrics.max_over_limit_mi.max(over_limit_mi);
            if zone_reason == "construction" && self.metrics.construction_entry_speed_mph.is_none()
            {
                self.metrics.construction_entry_speed_mph = Some(speed_mph);
            }
            if zone_reason == "heavy traffic"
                && self.metrics.heavy_traffic_entry_speed_mph.is_none()
            {
                self.metrics.heavy_traffic_entry_speed_mph = Some(speed_mph);
            }
            if position_mi >= end_mi {
                reached = true;
                break;
            }
        }
        if !reached {
            let at = self.read_drive(|d| d.trip.position_mi);
            panic!("speed-control segment never reached {end_mi:.1} miles; stopped at {at:.1}");
        }

        let (tickets, fines) = self.read_drive(|d| (d.speeding_tickets, d.hos_fine_count));
        self.metrics.speeding_tickets = tickets;
        self.metrics.inspection_fines = fines;
        self.result()
    }

    /// Reach the spoken settlement without adding unrelated road events.
    pub fn settle_delivery_after_segment(&mut self) -> PlaytestResult {
        use crate::states::driving_menu_states::{ArrivalState, FacilityArrivalState};

        self.with_drive(|drive, ctx| {
            drive.trip.position_mi = drive.trip.total_miles();
            drive.trip.finished = true;
            drive.destination_exit_taken = true;
            drive.truck_mut().velocity_mps = 0.0;
            drive.truck_mut().set_parking_brake();
            drive.handle_arrival_gate(ctx);
        });
        self.finish_timed_state();
        self.expect_state::<FacilityArrivalState>("the gate did not open the dock menu");
        self.key(key_event(Key::Return, None));
        self.finish_timed_state();
        self.expect_state::<ArrivalState>("the dock menu did not settle the delivery");
        self.record_completion();
        self.result()
    }

    /// Follow the spoken destination-exit path with K, X, and ramp braking.
    pub fn drive_destination_exit_with_speed_control(
        &mut self,
        set_mph: f64,
        restricted_zone_reason: Option<&str>,
    ) -> PlaytestResult {
        use crate::states::driving_menu_states::{ArrivalState, FacilityArrivalState};

        self.with_drive(|drive, ctx| {
            drive.tutorial = None;
            drive.trip.set_patrols(Vec::new());
            drive.truck_mut().start_engine();
            drive.truck_mut().set_air_ready(false);
            drive.truck_mut().transmission.automatic = true;
            drive.truck_mut().transmission.gear = 10;
            drive.truck_mut().velocity_mps = set_mph / MPH_PER_MPS;
            drive.truck_mut().throttle = 0.35;
            let destination = drive
                .destination_exit_stop(ctx)
                .expect("this route has a destination exit");
            if let Some(reason) = restricted_zone_reason {
                let zone_end = destination.at_mi - 2.5;
                drive
                    .trip
                    .zones
                    .push(Zone::new(zone_end - 1.0, zone_end, RAMP_MAX_MPH, reason));
                drive
                    .trip
                    .zones
                    .sort_by(|a, b| a.start_mi.total_cmp(&b.start_mi));
            }
            drive.trip.position_mi = (destination.at_mi - drive.exit_window_mi() - 0.25).max(0.0);
            drive.handle_key_event(ctx, &key_event(Key::K, None));
        });

        let mut signaled = false;
        let mut last_mode = String::new();
        // Budgeted for a road with traffic on it, for the same reason
        // `drive_speed_control_segment` is.
        let mut arrived = false;
        for _ in 0..120_000 {
            // The Python harness patched `pygame.key.get_pressed` with an
            // `ExitKeys` object that brakes toward the ramp's own profile.
            // Here the same rule writes the held-key set the drive reads.
            self.hold_exit_ramp_keys();
            self.advance_frame_clock();
            let (mode, over_limit_mi, ramp_mi, announced, speed_mph) =
                self.with_drive(|drive, ctx| {
                    let cut_out = drive.truck().specs.air_governor_cut_out_psi;
                    drive.truck_mut().set_air_pressure_psi(cut_out);
                    drive.truck_mut().parking_brake = false;
                    drive.update_frame(ctx, DT);
                    (
                        speed_control_mode(drive),
                        drive.over_limit_mi,
                        drive.ramp_mi,
                        !drive.destination_exit_announced_key.is_empty(),
                        drive.truck().speed_mph(),
                    )
                });
            if mode != last_mode {
                self.metrics.speed_control_transitions.push(mode.clone());
                last_mode = mode;
            }
            self.metrics.max_over_limit_mi = self.metrics.max_over_limit_mi.max(over_limit_mi);
            if announced && !signaled {
                self.press_key(Key::X, None);
                signaled = true;
            }
            if ramp_mi.is_some() && self.metrics.destination_exit_speed_mph.is_none() {
                self.metrics.destination_exit_speed_mph = Some(speed_mph);
            }
            // Arrival runs a short spoken pull-in beat before the dock menu, so
            // let it finish instead of driving frames at a state that is only
            // counting down.
            self.finish_timed_state();
            if self.state_is::<FacilityArrivalState>() {
                arrived = true;
                break;
            }
        }
        if !arrived {
            let (position, speed, ramp) =
                self.read_drive(|d| (d.trip.position_mi, d.truck().speed_mph(), d.ramp_mi));
            panic!(
                "automatic speed control never completed the destination exit: \
                 position={position:.2}, speed={speed:.1}, ramp={ramp:?}, \
                 signaled={signaled}\n{}",
                self.transcript_text()
            );
        }

        self.metrics.speeding_tickets = self.read_drive(|d| d.speeding_tickets);
        self.key(key_event(Key::Return, None));
        self.finish_timed_state();
        assert!(self.state_is::<ArrivalState>());
        self.record_completion();
        self.result()
    }

    pub fn drive_delivery_to_completion(&mut self) -> PlaytestResult {
        use crate::states::driving_menu_states::{ArrivalState, FacilityArrivalState};

        self.prepare_for_driving(30.0);

        let crawl_mph = 15.0;
        let total_miles = self.read_drive(|d| d.trip.total_miles());
        let max_frames = (total_miles / crawl_mph * 3600.0 * 60.0) as usize + 60 * 60;
        let mut finished = false;
        for _ in 0..max_frames {
            self.drive_one_frame();
            if self.read_drive(|d| d.trip.finished) {
                let full_stop = self.with_drive(|drive, ctx| {
                    drive.truck_mut().velocity_mps = 0.0;
                    drive.truck_mut().set_parking_brake();
                    drive.handle_arrival_gate(ctx);
                    drive.arrival_full_stop_said
                });
                if full_stop {
                    self.press_key(Key::Return, None);
                }
                self.finish_timed_state();
                finished = true;
                break;
            }
        }
        if !finished {
            let (at, total) = self.read_drive(|d| (d.trip.position_mi, d.trip.total_miles()));
            panic!("delivery never finished in {max_frames} frames: {at:.1}/{total:.1} mi");
        }

        assert!(self.state_is::<FacilityArrivalState>());
        self.finish_delivery();
        assert!(self.state_is::<ArrivalState>());
        self.record_completion();
        self.result()
    }

    /// Fast-forward the active delivery to its settlement screen.
    ///
    /// The career-arc playtests care about what settlement *says* --
    /// level-ups, tractor assignments, badges -- not the road in between,
    /// so this teleports to the destination gate instead of driving it.
    /// Pair with [`PlaytestHarness::read_settlement_lines`] to walk the
    /// arrival menu the way a screen-reader player would.
    pub fn settle_current_delivery(&mut self) -> PlaytestResult {
        use crate::states::driving_menu_states::{ArrivalState, FacilityArrivalState};

        self.prepare_for_driving(30.0);
        self.with_drive(|drive, ctx| {
            drive.trip.position_mi = drive.trip.total_miles();
            drive.trip.finished = true;
            drive.truck_mut().velocity_mps = 0.0;
            drive.truck_mut().set_parking_brake();
            drive.handle_arrival_gate(ctx);
        });
        self.finish_timed_state();
        assert!(self.state_is::<FacilityArrivalState>());
        self.finish_delivery();
        assert!(self.state_is::<ArrivalState>());
        self.record_completion();
        self.result()
    }

    /// Arrow through every settlement line so each is spoken in order.
    pub fn read_settlement_lines(&mut self) -> PlaytestResult {
        use crate::states::driving_menu_states::ArrivalState;

        assert!(self.state_is::<ArrivalState>());
        let rows = self.menu_labels().len();
        for _ in 0..rows.saturating_sub(1) {
            self.key(key_event(Key::Down, None));
        }
        self.result()
    }

    // -- staging the road ------------------------------------------------------------

    pub fn add_npc_traffic_ahead(
        &mut self,
        behavior: &str,
        gap_mi: f64,
        speed_mph: f64,
        relative_lane: i64,
    ) -> NPCVehicle {
        let vehicle = self.read_drive(|drive| {
            NPCVehicle::new(
                "harness:npc",
                drive.trip.position_mi + gap_mi,
                speed_mph,
                speed_mph,
                relative_lane,
                behavior,
            )
        });
        let staged = vehicle.clone();
        self.with_drive(move |drive, _| {
            drive.trip.traffic_manager.vehicles = vec![staged.into()];
        });
        vehicle
    }

    pub fn add_traffic_pressure_ahead(
        &mut self,
        gap_mi: f64,
        kind: &str,
        direction: &str,
        reason: &str,
    ) -> TrafficPressure {
        let start = self.read_drive(|drive| drive.trip.position_mi + gap_mi);
        let pressure = TrafficPressure {
            start_mi: start,
            end_mi: start + 1.0,
            kind: kind.to_string(),
            direction: direction.to_string(),
            intensity: 0.8,
            target_speed_mph: 42.0,
            reason: reason.to_string(),
        };
        let staged = pressure.clone();
        self.with_drive(move |drive, _| {
            drive.trip.traffic_pressures = vec![staged];
        });
        pressure
    }

    /// Re-enable one deterministic trip event after default neutralization.
    pub fn emit_trip_event(&mut self, kind: TripEventKind, text: &str, data: TripEventData) {
        let event = TripEvent {
            kind,
            message: text.into(),
            data,
        };
        self.with_drive(|drive, ctx| drive.handle_trip_event(ctx, &event));
    }

    // -- menus -----------------------------------------------------------------------

    /// Every option on the current menu, for reviewing what is reachable.
    pub fn menu_labels(&self) -> Vec<String> {
        match self.app.state() {
            Some(state) => menu_labels_of(&*state.borrow(), &self.app.ctx),
            None => Vec::new(),
        }
    }

    /// Choose a menu item by its label instead of by blind Enter presses.
    ///
    /// The pickup and arrival menus grow items as features land -- the
    /// walk-around, refusing a trailer -- and a harness that presses Enter a
    /// fixed number of times silently starts driving a different button when
    /// that happens. Naming what it wants keeps a playtest honest.
    pub fn select_menu_item(&mut self, label: &str) {
        let labels = self.menu_labels();
        assert!(
            labels.iter().any(|row| row == label),
            "{label:?} not in {labels:?}"
        );
        for _ in 0..=labels.len() {
            if self.focused_label().as_deref() == Some(label) {
                self.key(key_event(Key::Return, None));
                return;
            }
            self.key(key_event(Key::Down, None));
        }
        panic!("{label:?} never took focus; saw {labels:?}");
    }

    /// Arrow down to the row whose label starts with `prefix` and activate
    /// it, for rows that carry a value after the name ("Fuel: 12 miles").
    pub fn select_menu_item_starting(&mut self, prefix: &str) {
        let labels = self.menu_labels();
        for _ in 0..=labels.len() {
            if self
                .focused_label()
                .is_some_and(|label| label.starts_with(prefix))
            {
                self.key(key_event(Key::Return, None));
                return;
            }
            self.key(key_event(Key::Down, None));
        }
        panic!("no row starts with {prefix:?}: {labels:?}");
    }

    /// T while rolling, then the purpose list's row for `purpose` ("Fuel",
    /// "Sleep", "Break"). A T the drive answers without the list (a cancel,
    /// a scale, a stopped truck) is just the key press.
    pub fn plan_stop(&mut self, purpose: &str) {
        self.press_key(Key::T, Some('t'));
        if self.state_is::<crate::states::driving_menu_states::StopPurposeState>() {
            self.select_menu_item_starting(&format!("{purpose}:"));
        }
    }

    /// `_select_current_menu_text`: arrow down to `text` and activate it,
    /// with a bounded, descriptive failure when it is not reachable.
    pub fn select_current_menu_text(&mut self, text: &str) {
        let labels = self.menu_labels();
        for _ in 0..labels.len() {
            if self.focused_label().as_deref() == Some(text) {
                self.key(key_event(Key::Return, None));
                return;
            }
            self.key(key_event(Key::Down, None));
        }
        panic!("Menu item {text:?} not reachable with Down: {labels:?}");
    }

    /// The row with focus on the active screen.
    pub fn focused_label(&self) -> Option<String> {
        let state = self.app.state()?;
        let borrowed = state.borrow();
        let (labels, index) = menu_rows(&*borrowed, &self.app.ctx)?;
        labels.get(index).cloned()
    }

    /// Do the pre-trip on the hooked trailer, if there is one to walk.
    pub fn walk_around_trailer(&mut self) -> PlaytestResult {
        if self
            .menu_labels()
            .iter()
            .any(|row| row == "Walk around the trailer")
        {
            self.select_menu_item("Walk around the trailer");
        }
        self.result()
    }

    /// Send a defective trailer back; returns whether there was one to refuse.
    pub fn refuse_trailer(&mut self) -> bool {
        if !self
            .menu_labels()
            .iter()
            .any(|row| row == "Refuse this trailer")
        {
            return false;
        }
        self.select_menu_item("Refuse this trailer");
        self.metrics.trailer_refused = true;
        true
    }

    // -- equipment -------------------------------------------------------------------

    /// Capture the tractor, the trailer, and how the freight is handled.
    ///
    /// Read rather than driven, so it is safe to call at any point in a run.
    pub fn record_equipment(&mut self) -> PlaytestResult {
        let profile = self
            .app
            .ctx
            .profile
            .as_ref()
            .expect("record_equipment needs a career");
        let key = profile.active_truck_key();
        self.metrics.assigned_truck = key.clone();
        self.metrics.assigned_truck_label = truck_model(&key)
            .map(|model| model.label.to_string())
            .unwrap_or_default();
        if !profile.owns_equipment() {
            self.metrics.slip_seat_pool = slip_seat_pool(profile)
                .into_iter()
                .filter_map(|spare| truck_model(spare).map(|model| model.label.to_string()))
                .collect();
        }
        let Some(job) = self.active_job() else {
            return self.result();
        };
        let profile = self.app.ctx.profile.as_ref().expect("a career");
        let plan = pickup_plan(&job, profile);
        self.metrics.pickup_mode = if plan.is_drop_hook() {
            "drop and hook"
        } else {
            "live load"
        }
        .to_string();
        self.metrics.pickup_minutes = plan.minutes;
        self.metrics.detention_minutes = plan.detention_minutes;
        self.metrics.detention_pay = plan.detention_pay();
        if let Some(trailer) = &plan.trailer {
            self.metrics.trailer_number = trailer.number.clone();
            self.metrics.trailer_condition = trailer.condition_text().to_string();
            self.metrics.trailer_defect = trailer.defect().unwrap_or_default().to_string();
        }
        let drop = delivery_plan(&job, profile);
        self.metrics.delivery_mode = if drop.is_drop_hook() {
            "drop the trailer"
        } else {
            "live unload"
        }
        .to_string();
        self.metrics.delivery_minutes = drop.minutes;
        self.result()
    }

    // -- internals -------------------------------------------------------------------

    /// The job the active screen or the drive is working (`getattr(state,
    /// "job", None) or getattr(self.driving, "job", None)`).
    fn active_job(&self) -> Option<Job> {
        use crate::states::city_pickup::PickupFacilityState;

        if let Some(state) = self.app.state() {
            let borrowed = state.borrow();
            if let Some(pickup) = borrowed.as_any().downcast_ref::<PickupFacilityState>() {
                return Some(pickup.job.clone());
            }
            if let Some(drive) = borrowed.as_any().downcast_ref::<DrivingState>() {
                return Some(drive.job.clone());
            }
        }
        self.driving.as_ref().and_then(|shared| {
            shared
                .borrow()
                .as_any()
                .downcast_ref::<DrivingState>()
                .map(|drive| drive.job.clone())
        })
    }

    /// End the delivery whichever way this receiver takes freight.
    fn finish_delivery(&mut self) {
        self.record_equipment();
        let labels = self.menu_labels();
        let action = [
            "Drop the loaded trailer and hook an empty",
            "Dock and deliver",
        ]
        .into_iter()
        .find(|label| labels.iter().any(|row| row == label))
        .unwrap_or_else(|| panic!("no way to deliver: {labels:?}"));
        self.select_menu_item(action);
        self.finish_timed_state();
    }

    /// The four numbers every completion path reports.
    fn record_completion(&mut self) {
        let (destination, remaining) =
            self.read_drive(|d| (d.job.destination.clone(), d.trip.remaining_miles()));
        let profile = self
            .app
            .ctx
            .profile
            .as_ref()
            .expect("a completed delivery has a career");
        self.metrics.deliveries = profile.career.deliveries;
        self.metrics.current_city = profile.current_city.clone();
        self.metrics.destination = destination;
        self.metrics.remaining_miles = remaining;
    }

    fn choose_unlocked_job(&mut self, rank: usize) {
        use crate::states::city::JobBoardState;

        let target_index = self.with_state::<JobBoardState, _>(|board, ctx| {
            let mut unlocked: Vec<(usize, f64)> = board
                .jobs
                .iter()
                .enumerate()
                // The harness stages FREIGHT runs: an assigned reposition on
                // the board is the feature working, not the job this preset
                // wants.
                .filter(|(_, job)| board.locked_reason(ctx, job).is_empty() && !job.bobtail)
                .map(|(i, job)| (i, job.distance_mi))
                .collect();
            assert!(!unlocked.is_empty(), "no unlocked freight on the board");
            unlocked.sort_by(|a, b| a.1.total_cmp(&b.1));
            unlocked[rank % unlocked.len()].0
        });
        let rows = self.menu_labels().len();
        for _ in 0..=rows {
            let at = self.with_state::<JobBoardState, _>(|board, _| board.menu().index);
            if at == target_index {
                self.key(key_event(Key::Return, None));
                return;
            }
            self.key(key_event(Key::Down, None));
        }
        panic!("Job index {target_index} not keyboard reachable");
    }

    fn accept_assigned_job(&mut self, rank: usize, need_deadhead: bool) {
        use crate::states::city::JobBoardState;

        // The harness stages FREIGHT runs: if dispatch's assignment is a
        // reposition (the feature working -- dispatch's call, not yours),
        // spend a decline to reach freight instead. The same when the
        // scenario is about the deadhead and the load is staged in the
        // home yard, which opens the shipping office in place of one.
        loop {
            let skip = self.with_state::<JobBoardState, _>(|board, ctx| {
                !board.jobs.is_empty()
                    && (board.assigned_job().bobtail
                        || (need_deadhead && board.assigned_load_is_staged_here(ctx)))
            });
            if !skip || !self.move_to_decline() {
                break;
            }
            self.key(key_event(Key::Return, None));
        }
        for _ in 0..rank {
            if !self.move_to_decline() {
                break; // out of declines or no alternative freight
            }
            self.key(key_event(Key::Return, None));
        }
        self.key(key_event(Key::Home, None));
        self.key(key_event(Key::Return, None));
    }

    /// Move the cursor onto the board's Decline row; false when there is none.
    fn move_to_decline(&mut self) -> bool {
        let labels = self.menu_labels();
        if !labels.iter().any(|row| row.starts_with("Decline")) {
            return false;
        }
        for _ in 0..=labels.len() {
            if self
                .focused_label()
                .is_some_and(|row| row.starts_with("Decline"))
            {
                return true;
            }
            self.key(key_event(Key::Down, None));
        }
        panic!("Decline action not keyboard reachable");
    }

    fn choose_route(&mut self, rank: usize) {
        use crate::states::city::RouteSelectState;

        let target_index =
            self.with_state::<RouteSelectState, _>(|state, _| rank % state.routes.len());
        let rows = self.menu_labels().len();
        for _ in 0..=rows {
            let at = self.with_state::<RouteSelectState, _>(|state, _| state.menu().index);
            if at == target_index {
                self.key(key_event(Key::Return, None));
                return;
            }
            self.key(key_event(Key::Down, None));
        }
        panic!("Route index {target_index} not keyboard reachable");
    }

    fn neutralize_random_trip_friction(&mut self) {
        self.with_drive(|drive, _| {
            drive.trip.hazard_check_mi = 1e9;
            drive.trip.inspection_check_mi = 1e9;
            // Traffic IS random friction for these measurements, and emptying
            // the list is not enough on its own -- the rolling bubble tops the
            // road up again on the next update. Left on, a speed-control
            // segment settled in behind a slower vehicle and stopped at the
            // same mile no matter how many frames it was given.
            drive.trip.traffic_manager.rolling_bubble = false;
            drive.trip.traffic_manager.vehicles.clear();
            drive.trip.traffic_pressures.clear();
            drive.weather_mut().current = WeatherKind::Clear;
        });
    }

    fn drive_one_frame(&mut self) {
        self.advance_frame_clock();
        let events = self.with_drive(|drive, _| {
            let (limit_mph, _reason) = drive.trip.speed_limit_at(drive.trip.position_mi);
            let target_mph = (limit_mph + 5.0).max(25.0);
            if drive.truck().speed_mph() > target_mph {
                drive.truck_mut().throttle = 0.0;
                drive.truck_mut().brake = 0.5;
            } else {
                drive.truck_mut().throttle = 0.8;
                drive.truck_mut().brake = 0.0;
            }
            drive.truck_mut().grip = 1.0;
            drive.truck_mut().grade = 0.0;
            let (tank, cut_out) = {
                let specs = &drive.truck().specs;
                (specs.fuel_tank_gal, specs.air_governor_cut_out_psi)
            };
            drive.truck_mut().fuel_gal = tank;
            drive.truck_mut().set_air_pressure_psi(cut_out);
            drive.truck_mut().parking_brake = false;
            drive.truck_mut().auto_shift();
            drive.truck_mut().update(DT);
            drive.trip.update(DT)
        });
        for event in &events {
            self.with_drive(|drive, ctx| drive.handle_trip_event(ctx, event));
        }
        // The real frame drains the ambient channel here, between handling
        // the trip's events and stepping the hazard (`update_frame`). Leaving
        // it out made this harness lie about spoken output: an ambient line
        // that could not speak at once sets a spacing cooldown that only this
        // call ever decrements, so the FIRST such line jammed the channel for
        // the rest of the drive and every later one waited forever. A whole
        // delivery produced 42 waiting ambient lines and spoke none of them,
        // which is how a mapped state line came to look permanently
        // unreachable and got written up as a strict xfail against the game
        // rather than against this loop.
        self.with_drive(|drive, ctx| {
            drive.update_ambient_events(ctx, DT);
            drive.update_hazard(ctx, DT);
            if drive.hazard_deadline.is_some() {
                drive.truck_mut().velocity_mps = 5.0;
            }
        });
    }

    /// The Python `ExitKeys` held-key stand-in: brake toward the speed the
    /// remaining ramp distance allows, and roll on when well under it.
    fn hold_exit_ramp_keys(&mut self) {
        let (down, up) = self.read_drive(|drive| {
            let Some(ramp_mi) = drive.ramp_mi else {
                return (false, false);
            };
            let remaining = ramp_mi.max(0.0);
            // A driver who knows the realistic exit (2026-09-24): road speed
            // into the deceleration lane, the exit speed by the ramp curve,
            // then a steady stop to the end. The old rule, seventy times the
            // miles left, was a half-mile ramp's profile and braked hard at
            // the gore of every shorter one.
            const SHED_MPS2: f64 = 1.5;
            let profile = |miles: f64, end_mph: f64| {
                let end = end_mph / MPH_PER_MPS;
                (end * end + 2.0 * SHED_MPS2 * miles.max(0.0) * 1609.344).sqrt() * MPH_PER_MPS
            };
            let exit_mph = drive.armed_ramp_mph(None);
            let to_the_end = profile(remaining, 0.0).max(4.0);
            let target_mph = if remaining == 0.0 {
                0.0
            } else if let Some(lane_left) = drive.deceleration_lane_left_mi() {
                profile(lane_left, exit_mph).min(to_the_end)
            } else {
                exit_mph.min(to_the_end)
            };
            let speed = drive.truck().speed_mph();
            (
                speed > target_mph + 1.0,
                remaining > 0.0 && speed < target_mph - 1.0,
            )
        });
        // Right into the exit lane where the cab says it opens.
        let right = self.read_drive(|drive| drive.lane.exit_lane_open);
        for (key, held) in [(Key::Down, down), (Key::Up, up), (Key::Right, right)] {
            if held {
                self.app.ctx.input.press(key, Mods::NONE);
            } else {
                self.app.ctx.input.release(key, Mods::NONE);
            }
        }
    }
}

/// `"keeper"` / `"cruise"` / `"off"`: which controller has the pedals.
fn speed_control_mode(drive: &DrivingState) -> String {
    if drive.keeper_mph.is_some() {
        "keeper"
    } else if drive.cruise_mph.is_some() {
        "cruise"
    } else {
        "off"
    }
    .to_string()
}

/// Shared by the harness and the break battery: a `Rc<RefCell<T>>` handle
/// the caller keeps while the app owns the object.
pub type Shared<T> = Rc<RefCell<T>>;
