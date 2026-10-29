//! Pickup facility and route-planning states for terminal dispatches (port
//! of `freight_fate/states/city_pickup.py`).

use serde_json::{json, Map, Value};
use std::collections::HashSet;

use ff_core::data::world::World;
use ff_core::data::world_models::Route;
use ff_core::models::business_constants::is_owner_operator;
use ff_core::models::dispatch_policy::dispatch_policy;
use ff_core::models::jobs::{job_from_payload, job_payload, plan_hos, Job};
use ff_core::models::trailer_yard::{
    pickup_plan, replacement_trailer, PickupPlan, TrailerUnit, TRAILER_SWAP_MIN,
};
use ff_core::music::{select_menu_music_sequence, MenuMusicProfile};
use ff_core::pyfmt::{fmt_f, fmt_grouped, round_py_n};
use ff_core::settings::Settings;
use ff_core::sim::hos::{limits, HosClock};
use ff_core::sim::real_weather_alerts::{route_option_weather_note, warm_route_alerts};
use ff_core::sim::route_roadwork::{
    choose_dispatch_route, dispatch_route_line, route_option_note, route_planning_note,
    route_state_keys, DispatchRouting,
};
use ff_core::sim::season::{adjust_for_calendar, real_clock_game_hours, temperature_c};
use ff_core::sim::surge::{liquid_load_for, LiquidCargo};
use ff_core::sim::trip_traffic::TrafficProvider;
use ff_core::sim::vehicle::TruckState;
use ff_core::sim::weather::WeatherSystem;

use crate::app::GameContext;
use crate::audio::facility_ambient_key;
use crate::discord_presence::PresenceState;
use crate::impl_state_for_menu;
use crate::meaningful_play::MeaningfulPlayReason;
use crate::states::base::{InputEvent, Key, Menu, MenuCore, MenuItem, TimedMessageState};
use crate::states::city::{
    base_menu_enter, base_menu_handle_event, launch_driving, profile, profile_mut, CityMenuState,
    DrivingLaunch, LaunchAnnouncement, LoadedDepartureResume, DRIVE_PHASE_DELIVERY,
};
use crate::states::driving_team::TeamMode;
use crate::states::main_menu::MainMenuState;

pub const PICKUP_CHECK_IN_MIN: f64 = 15.0;
pub const PICKUP_LOADING_MIN: f64 = 60.0;
pub const PICKUP_LOADING_WAIT_S: f64 = 1.5;

/// Whether this job's pickup facility is still in the world data.
///
/// A dispatch board is cached into the save, so it can outlive the world the
/// offers were built from: an update that renames or retires a facility
/// leaves a job nobody can be sent to. Accepting one is the only place that
/// resolves the pickup facility, so this is what stands between a stale save
/// and a hard failure there.
pub fn job_origin_exists(job: &Job, world: &World) -> bool {
    if job.bobtail {
        // A reposition's origin is a synthetic company yard, never a real
        // facility -- there is nothing here for a data update to retire. A
        // route that stops existing is instead handled gracefully at accept
        // time (BobtailDestState._start, JobBoardState._accept_reposition).
        return true;
    }
    world
        .facility_location(&job.origin, &job.origin_location)
        .is_ok()
}

/// The keyword arguments of `pickup_snapshot`, each with its Python default.
#[derive(Debug, Clone, Default)]
pub struct PickupSnapshotOptions {
    pub checked_in: bool,
    pub loaded: bool,
    pub air_brake: Option<Value>,
    pub engine_on: bool,
    pub speed_control_armed: bool,
    pub speed_control_target_mph: Option<f64>,
    pub trailer_refused: bool,
}

pub fn pickup_snapshot(job: &Job, opts: &PickupSnapshotOptions) -> Map<String, Value> {
    let mut data = Map::new();
    data.insert("kind".into(), Value::from("pickup"));
    data.insert("job".into(), Value::Object(job_payload(job)));
    data.insert("checked_in".into(), Value::from(opts.checked_in));
    data.insert("loaded".into(), Value::from(opts.loaded));
    data.insert("trailer_refused".into(), Value::from(opts.trailer_refused));
    data.insert("engine_on".into(), Value::from(opts.engine_on));
    data.insert(
        "speed_control_armed".into(),
        Value::from(opts.speed_control_armed),
    );
    data.insert(
        "speed_control_target_mph".into(),
        match opts.speed_control_target_mph {
            Some(v) => json!(v),
            None => Value::Null,
        },
    );
    if let Some(air_brake) = &opts.air_brake {
        data.insert("air_brake".into(), air_brake.clone());
    }
    data
}

/// `clock` is the driver's hours now, when hours of service are enforced:
/// mid-shift, a fresh clock left out the sleep the law would force. A team
/// load (`team`) changes drivers where a solo driver sleeps, so its plan
/// names no sleep.
pub fn route_planning_summary(route: &Route, clock: Option<&HosClock>, team: bool) -> String {
    let mut plan = plan_hos(route.miles(), Some(route), None, clock);
    if team {
        plan.sleeps = 0;
    }
    let hos_summary = plan.summary();
    let usable = route.accessible_stop_details(false);
    let fuel_stops = usable
        .iter()
        .filter(|stop| stop.actions.iter().any(|a| a == "fuel"))
        .count();
    let sleep_stops = usable
        .iter()
        .filter(|stop| stop.actions.iter().any(|a| a == "sleep"))
        .count();
    // The HOS line already names the sleep stops when it names any.
    let sleep_text = if plan.has_stop_coverage() || sleep_stops == 0 {
        String::new()
    } else {
        format!(" Sleep stops: {sleep_stops}.")
    };
    let tolls = route.estimated_tolls();
    let toll_text = if tolls > 0.0 {
        format!(
            " Estimated tolls, carrier-paid, {} dollars.",
            fmt_grouped(tolls, 0)
        )
    } else {
        String::new()
    };
    format!(
        "{hos_summary} Fuel stops: {fuel_stops}.{sleep_text}{toll_text} Terrain: {}.",
        route.terrain_summary()
    )
}

/// Ask the state 511 feeds and the Weather Service now for what lies on a
/// load's route, so the answers are in the providers' caches by the time
/// dispatch chooses the lane at the pickup. Never blocks: the providers
/// fetch in the background and this returns at once. Nothing happens with
/// both live sources off.
pub fn warm_route_feeds(ctx: &mut GameContext, origin: &str, destination: &str) {
    let traffic = ctx.real_traffic_provider_arc();
    let alerts = ctx.weather_alerts_provider_arc();
    if traffic.is_none() && alerts.is_none() {
        return;
    }
    let Ok(routes) = ctx.world.supported_route_options(origin, destination, 3) else {
        return;
    };
    let mut seen: HashSet<String> = HashSet::new();
    for route in &routes {
        if let Some(traffic) = &traffic {
            for state in route_state_keys(route) {
                if seen.insert(state.clone()) {
                    traffic.fetch_construction(&state);
                }
            }
        }
        if let Some(alerts) = &alerts {
            warm_route_alerts(route, alerts, ctx.world);
        }
    }
}

/// Dispatch's ranking of the route options against the 511 construction
/// reports and the Weather Service warnings it can see. With both live
/// sources off the order is the world's.
fn dispatch_routing(ctx: &mut GameContext, routes: &[Route]) -> DispatchRouting {
    let provider = ctx.real_traffic_provider_arc();
    let provider: Option<&dyn TrafficProvider> =
        provider.as_deref().map(|p| p as &dyn TrafficProvider);
    let alerts = ctx.weather_alerts_provider_arc();
    choose_dispatch_route(routes, provider, alerts.as_deref(), ctx.world)
}

pub fn route_departure_summary(route: &Route, settings: &Settings) -> String {
    let tolls = route.estimated_tolls();
    let toll_text = if tolls > 0.0 {
        format!(" Carrier toll estimate {} dollars.", fmt_grouped(tolls, 0))
    } else {
        String::new()
    };
    format!(
        "Loaded trip is {} \
         via {}.{toll_text}",
        settings.distance_text(route.miles(), false),
        route.highways().join(", then ")
    )
}

/// The keyword arguments of `start_loaded_drive`.
#[derive(Debug, Clone, Default)]
pub struct LoadedDriveOptions {
    pub air_brake: Option<Value>,
    pub engine_on: bool,
    pub speed_control_armed: bool,
    pub speed_control_target_mph: Option<f64>,
    pub lead: String,
    pub trailer_refused: bool,
    /// The co-driver takes the wheel from the dock.
    pub team: Option<TeamMode>,
}

/// Build the loaded delivery trip and depart, narrating `lead` first.
///
/// Shared by the player-chosen route path (`RouteSelectState`) and the
/// dispatch-assigned route path so air-brake, engine, and speed-control
/// snapshots carry over identically on both.
pub fn start_loaded_drive(ctx: &mut GameContext, job: Job, route: Route, opts: LoadedDriveOptions) {
    let mut launch = DrivingLaunch::new(
        job,
        route,
        DRIVE_PHASE_DELIVERY,
        LaunchAnnouncement::LoadedDeparture { lead: opts.lead },
    );
    // A trailer the driver refused at the shipper must not follow them onto the
    // road: the scale house has to find the box they are actually pulling.
    launch.resume = Some(LoadedDepartureResume {
        air_brake: opts.air_brake,
        engine_on: opts.engine_on,
        speed_control_armed: opts.speed_control_armed,
        speed_control_target_mph: opts.speed_control_target_mph,
        trailer_refused: opts.trailer_refused,
        team: opts.team,
    });
    ctx.mark_meaningful_play(MeaningfulPlayReason::DriveStarted);
    launch_driving(ctx, launch);
}

// -- the facility engine kill switch ------------------------------------------------

// TODO(lead): FACILITY_ENGINE_* / DOCKING_MAX_MPH / set_engine_running /
// FacilityEngine belong in states::driving_core (FacilityEngineMixin);
// replace these with the real ones when that port lands.
pub const FACILITY_ENGINE_SHUT_DOWN_ITEM: &str = "Shut down the engine";
pub const FACILITY_ENGINE_START_ITEM: &str = "Start the engine";
/// dock/settle/rest actions need a complete stop
pub const DOCKING_MAX_MPH: f64 = 0.5;

/// Start or stop the engine from a menu, keeping the audio loop in step.
///
/// The driving frame loop only notices engine transitions that happen inside
/// `truck.update()`, so a change made while a menu holds the screen has to
/// move the audio itself; otherwise the loop idles on forever with the engine
/// off, or the truck runs in silence. Returns False only when the starter
/// refuses (no fuel), which leaves both the truck and the audio untouched.
pub fn set_engine_running(ctx: &mut GameContext, truck: &mut TruckState, running: bool) -> bool {
    if running {
        if !truck.start_engine() {
            return false;
        }
        ctx.audio.engine_start();
        return true;
    }
    if truck.engine_on {
        truck.stop_engine();
        ctx.audio.engine_stop();
    }
    true
}

/// Bring a running engine's idle back under a facility menu.
///
/// Arriving at a shipper or receiver replaces the drive, and leaving the
/// drive silences the world, engine loop included. A running engine then
/// idled in silence, and "Shut down the engine" had no loop to stop, so it
/// made no sound at all (tester report, 2026-09-28). No ignition crank: the
/// engine never stopped.
pub fn sync_facility_engine_audio(ctx: &mut GameContext, truck: &TruckState) {
    if truck.engine_on && !ctx.audio.engine_running() {
        ctx.audio.engine_start_with(false);
        ctx.audio.set_engine_rpm_with(truck.specs.idle_rpm, 0.0);
    }
}

/// The engine kill switch, offered where a facility menu has taken over.
///
/// Arriving at a shipper or a receiver parks the truck under half a mile an
/// hour and hands straight to a menu, so the road's engine control is out of
/// reach at exactly the moment a driver reaches for it: sitting at the gate,
/// or waiting on a dock crew (new player feedback, 2026-08-17). A state mixes
/// this in and supplies `facility_truck`; both facilities then offer the
/// same one row, worded the same way.
pub trait FacilityEngine: Menu {
    fn facility_truck(&self) -> &TruckState;
    fn facility_truck_mut(&mut self) -> &mut TruckState;

    /// Hook for a facility state that keeps a resume snapshot of its own.
    fn on_facility_engine_changed(&mut self, _ctx: &mut GameContext) {}

    /// One row that changes face, never two rows to arrow past.
    fn facility_engine_item(&self) -> MenuItem<Self> {
        if self.facility_truck().engine_on {
            return MenuItem::new(FACILITY_ENGINE_SHUT_DOWN_ITEM, |s: &mut Self, ctx| {
                s.toggle_facility_engine(ctx)
            })
            .help("No fuel burned and no idle noise while parked.");
        }
        MenuItem::new(FACILITY_ENGINE_START_ITEM, |s: &mut Self, ctx| {
            s.toggle_facility_engine(ctx)
        })
        .help("Starts the engine. The parking brake needs 100 psi of air.")
    }

    fn toggle_facility_engine(&mut self, ctx: &mut GameContext) {
        if self.facility_truck().speed_mph() > DOCKING_MAX_MPH {
            ctx.audio.play("ui/error");
            ctx.say("Stop before touching the engine.");
            return;
        }
        if self.facility_truck().engine_on {
            set_engine_running(ctx, self.facility_truck_mut(), false);
            self.on_facility_engine_changed(ctx);
            self.refresh(ctx, true);
            ctx.say("Engine off.");
            return;
        }
        if !set_engine_running(ctx, self.facility_truck_mut(), true) {
            ctx.audio.play("ui/error");
            ctx.say("The engine will not start.");
            return;
        }
        self.on_facility_engine_changed(ctx);
        self.refresh(ctx, true);
        let psi = self.facility_truck().air_pressure_psi();
        ctx.say(&format!(
            "Engine running. Air pressure {} psi.",
            fmt_f(psi, 0)
        ));
    }
}

// -- PickupFacilityState ---------------------------------------------------------------

/// The keyword arguments of `PickupFacilityState.__init__`, each with its
/// Python default. `truck` stands in for the `driving=` argument: the
/// arriving drive hands over the truck it was driving (plus its speed
/// control session) rather than a fresh one built from the profile.
#[derive(Debug, Default)]
pub struct PickupOptions {
    pub checked_in: bool,
    pub loaded: bool,
    pub truck: Option<TruckState>,
    pub air_brake: Option<Value>,
    pub engine_on: bool,
    pub speed_control_armed: bool,
    pub speed_control_target_mph: Option<f64>,
    pub announce_speed_control_status: bool,
    pub trailer_refused: bool,
}

pub struct PickupFacilityState {
    menu: MenuCore<Self>,
    pub job: Job,
    pub checked_in: bool,
    pub loaded: bool,
    /// A refused trailer is a decision the driver made, so it has to survive
    /// a save and follow them onto the road -- otherwise the walk-around is
    /// theatre and the inspector still writes up the box they refused.
    trailer_refused: bool,
    offer_refusal: bool,
    /// Gallons idled away at this facility, reported once when the load is
    /// on. Deliberately not saved: it is a line about the wait just served,
    /// not a running total the driver has to carry.
    idle_gallons: f64,
    just_loaded: bool,
    pub announce_speed_control_status: bool,
    pub truck: TruckState,
    pub speed_control_armed: bool,
    pub speed_control_target_mph: Option<f64>,
    /// Set by a team row just before departing; the plain row clears it.
    pub(crate) team_departure: Option<TeamMode>,
}

impl PickupFacilityState {
    pub fn new(ctx: &GameContext, job: Job, opts: PickupOptions) -> Self {
        let truck = match opts.truck {
            Some(truck) => truck,
            None => {
                let p = profile(ctx);
                let mut truck = TruckState::new(p.truck_specs());
                p.load_truck_condition(&mut truck);
                truck.restore_air_brake_snapshot(
                    opts.air_brake.as_ref().unwrap_or(&Value::Null),
                    true,
                );
                if opts.engine_on {
                    truck.start_engine();
                }
                truck
            }
        };
        PickupFacilityState {
            menu: MenuCore::new("Pickup facility")
                .with_open_sound(Some("facility/dock_gate"))
                .with_intro_help(
                    "Check in, then load cargo with the truck stopped. Escape repeats the \
                     pickup status.",
                ),
            job,
            checked_in: opts.checked_in,
            loaded: opts.loaded,
            trailer_refused: opts.trailer_refused,
            offer_refusal: false,
            idle_gallons: 0.0,
            just_loaded: false,
            announce_speed_control_status: opts.announce_speed_control_status,
            truck,
            speed_control_armed: opts.speed_control_armed,
            speed_control_target_mph: opts.speed_control_target_mph,
            team_departure: None,
        }
    }

    /// `from_snapshot`: rebuild a pickup from its save record, or None when
    /// the record does not parse.
    pub fn from_snapshot(ctx: &GameContext, data: &Value) -> Option<Self> {
        let data = data.as_object()?;
        let job = job_from_payload(data.get("job")?.as_object()?)?;
        let target = data.get("speed_control_target_mph").and_then(Value::as_f64);
        let flag = |key: &str| data.get(key).and_then(Value::as_bool).unwrap_or(false);
        Some(Self::new(
            ctx,
            job,
            PickupOptions {
                checked_in: flag("checked_in"),
                loaded: flag("loaded"),
                truck: None,
                air_brake: data.get("air_brake").cloned(),
                engine_on: flag("engine_on"),
                speed_control_armed: flag("speed_control_armed"),
                speed_control_target_mph: target,
                announce_speed_control_status: true,
                trailer_refused: flag("trailer_refused"),
            },
        ))
    }

    pub fn facility(&self) -> String {
        self.job.origin_facility_text()
    }

    pub fn trailer_refused(&self) -> bool {
        self.trailer_refused
    }

    /// Facility time is engine time, if the driver leaves it running.
    ///
    /// Check-in, a trailer swap and an hour on the dock never pass through
    /// the per-frame loop that burns fuel, so an idling truck used to sit
    /// there for free -- which left the kill switch with nothing to be worth
    /// (Jake, 2026-08-17). Same idle rate the road charges, ~0.8 gallons an
    /// hour, so shutting down is the driver's call and not a scolding.
    fn charge_facility_idle(&mut self, minutes: f64) {
        self.idle_gallons += self.truck.burn_idle_fuel_over_game_time(minutes * 60.0);
    }

    fn speed_control_pause_text(&self, ctx: &GameContext) -> String {
        if !self.speed_control_armed {
            return String::new();
        }
        let target = match self.speed_control_target_mph {
            Some(mph) => ctx.settings.speed_text(mph),
            None => "the posted limit when the open road begins".to_string(),
        };
        format!(" Speed control paused, target {target}.")
    }

    /// The pre-trip on the trailer, done on purpose rather than to you.
    ///
    /// A defect on a hooked trailer used to surface only when an inspector
    /// found it, which for a blind driver meant the walk-around was something
    /// that happened to them at a scale house. It is a deliberate action now:
    /// walk the trailer, hear what is wrong with it, and decide.
    ///
    /// A defect found here can be refused -- the yard swaps the box, which
    /// costs the time a swap really costs and hands the problem back to the
    /// people whose problem it is.
    pub fn walk_around(&mut self, ctx: &mut GameContext) {
        let plan = self.pickup_plan(ctx);
        let Some(trailer) = plan.trailer.as_ref() else {
            ctx.say(
                "Nothing to walk around yet. The shipper is loading the trailer you came in with.",
            );
            return;
        };
        if self.trailer_refused {
            ctx.audio.play("ui/notify");
            let described = self
                .swapped_trailer(ctx)
                .map(|t| t.describe())
                .unwrap_or_default();
            ctx.say(&format!(
                "You already had the yard swap that one. {described} \
                 It checks out."
            ));
            return;
        }
        let Some(defect) = trailer.defect() else {
            ctx.audio.play("ui/notify");
            ctx.say(&format!(
                "{} checks out.{}",
                crate::states::city::py_capitalize(&trailer.spoken_name()),
                self.tank_walk_around_clause()
            ));
            return;
        };
        ctx.audio.play("ui/warning");
        let name = crate::states::city::py_capitalize(&trailer.spoken_name());
        ctx.say(&format!(
            "{name}: {defect}, {}. Take it and the write-up is yours at the first scale. \
             Refuse for a swap, about {} minutes.{}",
            trailer.condition_text(),
            fmt_f(TRAILER_SWAP_MIN, 0),
            self.tank_walk_around_clause()
        ));
        // The badge is for finding it: awarded at the hook, it announced the
        // write-up before the driver had looked, and to drivers who never did.
        ctx.award_achievement("hooked_a_bad_one");
        self.offer_refusal = true;
        self.refresh(ctx, false);
    }

    /// What the walk-around tells you about a tank you cannot see into.
    ///
    /// The one place fill level and tank type belong in speech: they are
    /// fixed for the whole run and they decide everything about how it will
    /// drive. Once the wheels are turning the audio carries it instead.
    fn tank_walk_around_clause(&self) -> String {
        let Some(liquid) = liquid_load_for(
            Some(self.job.cargo as &dyn LiquidCargo),
            self.job.weight_tons,
        ) else {
            return String::new();
        };
        let behaviour = if liquid.baffled {
            "The baffles settle it in a couple of cycles, but it still arrives after you do."
        } else {
            "Smooth bore, nothing in there slows it down."
        };
        format!(
            " The tank is {}, {}. {behaviour}",
            liquid.describe_fill(),
            liquid.describe_tank()
        )
    }

    pub fn refuse_trailer(&mut self, ctx: &mut GameContext) {
        let facility = self.facility();
        {
            let p = profile_mut(ctx);
            let start = p.game_hours;
            p.game_hours += TRAILER_SWAP_MIN / 60.0;
            let end = p.game_hours;
            p.duty_log
                .record("on_duty_not_driving", start, end, &facility, "trailer swap");
            p.hos.on_duty(TRAILER_SWAP_MIN);
        }
        self.charge_facility_idle(TRAILER_SWAP_MIN);
        self.trailer_refused = true;
        self.offer_refusal = false;
        self.save_state(ctx);
        self.refresh(ctx, false);
        ctx.audio.play("ui/notify");
        ctx.award_achievement("refused_the_trailer");
        let described = self
            .swapped_trailer(ctx)
            .map(|t| t.describe())
            .unwrap_or_default();
        ctx.say(&format!(
            "Refused, and the yard brings another. {described} The swap cost {} minutes.",
            fmt_f(TRAILER_SWAP_MIN, 0)
        ));
    }

    /// The clean box the yard brings out when a defect is refused.
    pub fn swapped_trailer(&self, ctx: &GameContext) -> Option<TrailerUnit> {
        let plan = self.pickup_plan(ctx);
        replacement_trailer(&self.job, plan.trailer.as_ref())
    }

    /// The trailer actually under the truck, swap included.
    pub fn hooked_trailer(&self, ctx: &GameContext) -> Option<TrailerUnit> {
        if self.trailer_refused {
            return self.swapped_trailer(ctx);
        }
        self.pickup_plan(ctx).trailer
    }

    fn save_state(&mut self, ctx: &mut GameContext) {
        let snapshot = pickup_snapshot(
            &self.job,
            &PickupSnapshotOptions {
                checked_in: self.checked_in,
                loaded: self.loaded,
                trailer_refused: self.trailer_refused,
                air_brake: Some(self.truck.air_brake_snapshot()),
                engine_on: self.truck.engine_on,
                speed_control_armed: self.speed_control_armed,
                speed_control_target_mph: self.speed_control_target_mph,
            },
        );
        let p = profile_mut(ctx);
        p.store_truck_condition(&self.truck);
        p.active_trip = Some(Value::Object(snapshot));
        ctx.save_profile();
    }

    pub fn check_in(&mut self, ctx: &mut GameContext) {
        let facility = self.facility();
        {
            let p = profile_mut(ctx);
            let start = p.game_hours;
            p.game_hours += PICKUP_CHECK_IN_MIN / 60.0;
            let end = p.game_hours;
            p.duty_log.record(
                "on_duty_not_driving",
                start,
                end,
                &facility,
                "shipper check-in",
            );
            p.hos.on_duty(PICKUP_CHECK_IN_MIN);
        }
        self.charge_facility_idle(PICKUP_CHECK_IN_MIN);
        self.checked_in = true;
        self.save_state(ctx);
        self.refresh(ctx, false);
        ctx.audio.play("ui/notify");
        let plan = self.pickup_plan(ctx);
        if plan.is_drop_hook() {
            ctx.say(&format!(
                "Checked in. Load on {} in the drop yard.",
                plan.trailer
                    .as_ref()
                    .map(|t| t.spoken_name())
                    .unwrap_or_default()
            ));
        } else {
            ctx.say("Checked in. Dock assigned.");
        }
    }

    /// How this load gets on the truck: dropped and hooked, or live loaded.
    ///
    /// Derived from the job and the profile every time rather than stored, so
    /// it survives a save and a reload without a byte of new schema and always
    /// agrees with itself.
    pub fn pickup_plan(&self, ctx: &GameContext) -> PickupPlan {
        pickup_plan(&self.job, profile(ctx))
    }

    pub fn load(&mut self, ctx: &mut GameContext) {
        if !self.checked_in {
            ctx.say("Check in at the shipping office before loading.");
            return;
        }
        if self.truck.speed_mph() > DOCKING_MAX_MPH {
            ctx.audio.play("ui/error");
            ctx.say("Stop before loading.");
            return;
        }
        self.truck.throttle = 0.0;
        self.truck.brake = 1.0;
        self.truck.set_parking_brake();
        let plan = self.pickup_plan(ctx);
        let facility = self.facility();
        let (title, message, status) = if plan.is_drop_hook() {
            (
                "Hooking the loaded trailer",
                format!(
                    "Dropping your empty at {facility} and hooking {}, loaded with {} tons \
                     of {}.",
                    plan.trailer
                        .as_ref()
                        .map(|t| t.spoken_name())
                        .unwrap_or_default(),
                    fmt_f(self.job.weight_tons, 0),
                    self.job.cargo.label
                ),
                "Dropping and hooking.",
            )
        } else {
            (
                "Loading cargo",
                format!(
                    "Loading {} tons of {} at {facility}.",
                    fmt_f(self.job.weight_tons, 0),
                    self.job.cargo.label
                ),
                "Loading cargo.",
            )
        };
        // The pickup screen is the active state while this runs, so that is
        // the handle the timer's completion reaches it through.
        let pickup = ctx
            .state()
            .expect("the pickup facility is the active state");
        ctx.push_state(
            TimedMessageState::new(title, &message, status, PICKUP_LOADING_WAIT_S, move |ctx| {
                if let Ok(mut state) = pickup.try_borrow_mut() {
                    if let Some(pickup) = state.as_any_mut().downcast_mut::<PickupFacilityState>() {
                        pickup.finish_load(ctx);
                    }
                }
                ctx.pop_state();
            })
            .sound_key(Some("poi/dock_and_deliver")),
        );
    }

    /// `_finish_load` minus its closing `pop_state`, which the timer's
    /// completion runs once this state is no longer borrowed.
    fn finish_load(&mut self, ctx: &mut GameContext) {
        let plan = self.pickup_plan(ctx);
        let facility = self.facility();
        {
            let p = profile_mut(ctx);
            let start = p.game_hours;
            p.game_hours += plan.minutes / 60.0;
            let end = p.game_hours;
            let activity = if plan.is_drop_hook() {
                "dropping and hooking"
            } else {
                "loading"
            };
            p.duty_log
                .record("on_duty_not_driving", start, end, &facility, activity);
            p.hos.on_duty(plan.minutes);
        }
        self.charge_facility_idle(plan.minutes);
        self.loaded = true;
        self.just_loaded = true;
        self.save_state(ctx);
        // first_pickup is retired as an award; this hook now awards the merged
        // "first_day" badge (first_dispatch + air_ready + first_pickup). The
        // three retired ids keep their catalog entries so the cloud
        // validator's allow-list never sees a removed id.
        ctx.award_achievement("first_day");
        if plan.is_drop_hook() {
            ctx.award_achievement("first_drop_hook");
        } else if plan.detention_minutes > 0.0 && is_owner_operator(&profile(ctx).business_status) {
            ctx.award_achievement("detention_paid");
        }
    }

    pub fn depart_for_destination(&mut self, ctx: &mut GameContext) {
        if !self.loaded {
            ctx.say("Load the cargo before departing.");
            return;
        }
        let routes = ctx
            .world
            .supported_route_options(&self.job.origin, &self.job.destination, 3)
            .unwrap_or_default();
        if routes.is_empty() {
            ctx.audio.play("ui/error");
            ctx.say("Dispatch cannot find a route for this load.");
            return;
        }
        // The world ranks the options by distance; dispatch re-ranks them by
        // the construction the state 511 feeds report on each, the way a
        // dispatcher checks 511 before naming the lane.
        let routing = dispatch_routing(ctx, &routes);
        if dispatch_policy(profile(ctx)).assigns_route {
            // Company drivers run the lane dispatch gives them. Route choice
            // is an owner-operator freedom.
            let construction = dispatch_route_line(&routes, &routing, ctx.world, &ctx.settings);
            let route = routes[routing.pick()].clone();
            let mut lead = String::new();
            if !construction.is_empty() {
                lead.push_str(&construction);
                lead.push(' ');
            }
            lead.push_str(&format!(
                "Dispatch routed you to {}. ",
                self.job.destination_facility_text()
            ));
            start_loaded_drive(
                ctx,
                self.job.clone(),
                route,
                LoadedDriveOptions {
                    air_brake: Some(self.truck.air_brake_snapshot()),
                    engine_on: self.truck.engine_on,
                    speed_control_armed: self.speed_control_armed,
                    speed_control_target_mph: self.speed_control_target_mph,
                    trailer_refused: self.trailer_refused,
                    lead,
                    team: self.team_departure,
                },
            );
            return;
        }
        // Dispatch's pick reads as route 1; each option says what 511 has on it.
        let dispatch_note = route_planning_note(&routes, &routing, ctx.world);
        let notes: Vec<String> = routing
            .order
            .iter()
            .map(|&i| {
                let mut note = routing
                    .report(i)
                    .map(|report| route_option_note(report, &ctx.settings))
                    .unwrap_or_default();
                let weather = routing
                    .weather_report(i)
                    .map(route_option_weather_note)
                    .unwrap_or_default();
                if !weather.is_empty() {
                    if !note.is_empty() {
                        note.push(' ');
                    }
                    note.push_str(&weather);
                }
                note
            })
            .collect();
        let routes: Vec<Route> = routing.order.iter().map(|&i| routes[i].clone()).collect();
        let select = RouteSelectState::new(
            ctx,
            self.job.clone(),
            routes,
            RouteSelectOptions {
                back_label: "Back to pickup facility".to_string(),
                air_brake: Some(self.truck.air_brake_snapshot()),
                engine_on: self.truck.engine_on,
                speed_control_armed: self.speed_control_armed,
                speed_control_target_mph: self.speed_control_target_mph,
                trailer_refused: self.trailer_refused,
                notes,
                dispatch_note,
                team: self.team_departure,
            },
        );
        ctx.push_state(select);
    }

    /// `_status`: the pickup status line.
    pub fn status(&mut self, ctx: &mut GameContext) {
        let state = if self.loaded {
            "loaded and sealed"
        } else if self.checked_in {
            "checked in, waiting to load"
        } else {
            "not checked in"
        };
        let brake = if self.truck.parking_brake {
            "parking brake set"
        } else {
            "parking brake released"
        };
        let engine = if self.truck.engine_on {
            "Engine running"
        } else {
            "Engine off"
        };
        let text = format!(
            "Pickup {}: {state}. {} tons of {} for {}. {}. {engine}. Air {} psi, \
             {brake}.{}",
            self.facility(),
            fmt_f(self.job.weight_tons, 0),
            self.job.cargo.label,
            self.job.destination_facility_text(),
            ctx.settings.speed_text(self.truck.speed_mph()),
            fmt_f(self.truck.air_pressure_psi(), 0),
            self.speed_control_pause_text(ctx)
        );
        ctx.say(&text);
    }

    fn save_and_quit(&mut self, ctx: &mut GameContext) {
        self.save_state(ctx);
        ctx.say("Saved. The pickup resumes here.");
        MainMenuState::arm_update_check(&ctx.settings);
        ctx.reset_to(MainMenuState::new());
    }

    fn cancel(&mut self, ctx: &mut GameContext) {
        {
            let p = profile_mut(ctx);
            p.active_trip = None;
            p.dispatch_board_cache = None;
            // Every other way a run ends frees the advance for the next load.
            p.pay_advance_used_for_load = false;
        }
        ctx.save_profile();
        let terminal = crate::states::city::home_terminal(ctx);
        ctx.say(&format!("Pickup canceled. Returned to {}.", terminal.name));
        let city = CityMenuState::new(ctx, false);
        ctx.reset_to(city);
    }
}

impl FacilityEngine for PickupFacilityState {
    fn facility_truck(&self) -> &TruckState {
        &self.truck
    }

    fn facility_truck_mut(&mut self) -> &mut TruckState {
        &mut self.truck
    }

    fn on_facility_engine_changed(&mut self, ctx: &mut GameContext) {
        // The snapshot already carries engine_on, so a shutdown here resumes
        // shut down rather than the truck quietly restarting itself on load.
        self.save_state(ctx);
    }
}

impl Menu for PickupFacilityState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn presence(&self, _ctx: &GameContext) -> Option<PresenceState> {
        let activity = if self.loaded {
            "Loaded and ready to roll"
        } else if self.checked_in {
            "Loading at the dock"
        } else {
            "At a pickup facility"
        };
        Some(PresenceState::new(
            activity,
            &format!(
                "{} for {}",
                self.job.cargo.label,
                self.job.spoken_destination()
            ),
        ))
    }

    fn enter(&mut self, ctx: &mut GameContext) {
        let sequence =
            select_menu_music_sequence(ctx.profile.as_ref().map(|p| p as &dyn MenuMusicProfile));
        let refs: Vec<&str> = sequence.iter().map(String::as_str).collect();
        ctx.play_music_sequence("menu", &refs);
        sync_facility_engine_audio(ctx, &self.truck);
        base_menu_enter(self, ctx);
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        ctx.audio
            .set_ambient(Some(facility_ambient_key(&self.job.origin_type)));
        // Loading takes a while; by departure the live answers are usually in.
        warm_route_feeds(ctx, &self.job.origin, &self.job.destination);
        let plan = self.pickup_plan(ctx);
        let facility = self.facility();
        let lead = if self.loaded {
            let mut lead = format!("Loaded and sealed for {}.", self.job.spoken_destination());
            if self.just_loaded {
                let verb = if plan.is_drop_hook() {
                    "Dropping and hooking"
                } else {
                    "Loading"
                };
                lead.push_str(&format!(" {verb} took {} minutes.", fmt_f(plan.minutes, 0)));
                if plan.detention_minutes > 0.0 {
                    let dollars = fmt_grouped(plan.detention_pay(), 0);
                    let wait = fmt_f(plan.detention_minutes, 0);
                    if is_owner_operator(&profile(ctx).business_status) {
                        lead.push_str(&format!(
                            " {wait} minutes past the free time, {dollars} dollars detention \
                             owed to you at settlement."
                        ));
                    } else {
                        lead.push_str(&format!(
                            " {wait} minutes past the free time, {dollars} dollars detention \
                             owed to the carrier."
                        ));
                    }
                }
                if let Some(trailer) = plan.trailer.as_ref() {
                    lead.push_str(&format!(" {}", trailer.describe()));
                }
                // Rounded to a tenth, so the smallest number that can be
                // spoken is one worth hearing rather than "0.0 gallons".
                // Off now means every idle gallon came before the shutdown;
                // a bare "burned idling" after the load read as the engine
                // having run through it (tester report, 2026-09-28).
                if round_py_n(self.idle_gallons, 1) > 0.0 {
                    let when = if self.truck.engine_on {
                        ""
                    } else {
                        " before you shut down"
                    };
                    lead.push_str(&format!(
                        " {} gallons burned idling{when}.",
                        fmt_f(self.idle_gallons, 1)
                    ));
                }
                self.idle_gallons = 0.0;
                self.just_loaded = false;
            }
            lead
        } else if self.checked_in {
            if plan.is_drop_hook() {
                format!(
                    "Checked in at {facility}. Load on {} in the drop yard.",
                    plan.trailer
                        .as_ref()
                        .map(|t| t.spoken_name())
                        .unwrap_or_default()
                )
            } else {
                format!("Checked in at {facility}. Dock assigned.")
            }
        } else {
            format!("Arrived at {facility}.")
        };
        let speed_control = if self.announce_speed_control_status {
            self.speed_control_pause_text(ctx)
        } else {
            String::new()
        };
        self.announce_speed_control_status = false;
        let intro = self
            .team_intro(ctx)
            .map(|intro| format!(" {intro}"))
            .unwrap_or_default();
        let current = self.current_text(ctx);
        ctx.say(&format!("{lead}{speed_control}{intro} {current}"));
    }

    fn exit(&mut self, ctx: &mut GameContext) {
        ctx.audio.set_ambient(None);
        // Save and quit, or cancel: the idle must not follow the driver to
        // the title or the terminal. A departing drive brings its own back.
        ctx.audio.engine_stop_with(false);
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let plan = self.pickup_plan(ctx);
        let primary = if self.loaded {
            MenuItem::new("Depart for destination", |s: &mut Self, ctx| {
                s.team_departure = None;
                s.depart_for_destination(ctx)
            })
            .help("Starts the loaded run.")
        } else if self.checked_in && plan.is_drop_hook() {
            MenuItem::new("Drop and hook in the yard", |s: &mut Self, ctx| s.load(ctx)).help(
                "Hooks the trailer the shipper already loaded. Quicker than a dock, but the \
                 trailer is whatever the yard has.",
            )
        } else if self.checked_in {
            let dock_help = if is_owner_operator(&profile(ctx).business_status) {
                "Past two hours at the dock earns detention pay on this settlement."
            } else {
                "Past two hours at the dock is billed as detention on the carrier settlement, \
                 not to you."
            };
            MenuItem::new("Load cargo at dock", |s: &mut Self, ctx| s.load(ctx)).help(dock_help)
        } else {
            MenuItem::new("Check in at shipping office", |s: &mut Self, ctx| {
                s.check_in(ctx)
            })
            .help("Gets the dock assignment.")
        };
        let mut items = vec![primary];
        items.extend(self.team_departure_rows(ctx));
        if self.loaded && plan.trailer.is_some() {
            items.push(
                MenuItem::new("Walk around the trailer", |s: &mut Self, ctx| {
                    s.walk_around(ctx)
                })
                .help(
                    "Lamps, brake adjustment, and tires. Anything wrong is yours once you pull \
                     out of the gate.",
                ),
            );
        }
        if self.offer_refusal && !self.trailer_refused {
            items.push(
                MenuItem::new("Refuse this trailer", |s: &mut Self, ctx| {
                    s.refuse_trailer(ctx)
                })
                .help(format!(
                    "The yard brings a sound one. About {} minutes, and the write-up stays \
                     with them.",
                    fmt_f(TRAILER_SWAP_MIN, 0)
                )),
            );
        }
        items.push(self.facility_engine_item());
        items.push(
            MenuItem::new("Pickup status", |s: &mut Self, ctx| s.status(ctx))
                .help("Facility, cargo, destination, and next step."),
        );
        items.push(
            MenuItem::new("Save and quit to main menu", |s: &mut Self, ctx| {
                s.save_and_quit(ctx)
            })
            .help("Saves the pickup to resume here."),
        );
        items.push(
            MenuItem::new(
                "Cancel pickup and return to terminal",
                |s: &mut Self, ctx| s.cancel(ctx),
            )
            .help("Gives up this job and returns to the terminal."),
        );
        items
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        self.status(ctx);
    }

    fn lines(&self, ctx: &GameContext) -> Vec<String> {
        let state = if self.loaded {
            "Loaded and sealed"
        } else if self.checked_in {
            "Checked in"
        } else {
            "Check-in required"
        };
        let mut lines = vec![
            self.menu.title.clone(),
            format!("Facility: {}", self.facility()),
            format!(
                "Cargo: {} tons of {}",
                fmt_f(self.job.weight_tons, 0),
                self.job.cargo.label
            ),
            format!("Destination: {}", self.job.spoken_destination()),
            format!("Status: {state}"),
            format!(
                "Speed: {}",
                ctx.settings.hud_speed_text(self.truck.speed_mph())
            ),
            format!(
                "Engine: {}",
                if self.truck.engine_on {
                    "running"
                } else {
                    "off"
                }
            ),
            format!(
                "Air: {} psi   \
                 {}",
                fmt_f(self.truck.air_pressure_psi(), 0),
                if self.truck.parking_brake {
                    "parking set"
                } else {
                    "parking released"
                }
            ),
        ];
        if self.speed_control_armed {
            let target = match self.speed_control_target_mph {
                Some(mph) => ctx.settings.speed_text(mph),
                None => "posted limit when the open road begins".to_string(),
            };
            lines.push(format!(
                "Speed control: paused   Open-road target: {target}"
            ));
        }
        lines.push(String::new());
        for (i, item) in self.menu.items.iter().enumerate() {
            let marker = if i == self.menu.index { "> " } else { "  " };
            lines.push(format!("{marker}{}", item.text(self, ctx)));
        }
        lines
    }
}

impl_state_for_menu!(PickupFacilityState);

// -- RouteSelectState ---------------------------------------------------------------------

/// The keyword arguments of `RouteSelectState.__init__`.
#[derive(Debug, Clone)]
pub struct RouteSelectOptions {
    pub back_label: String,
    pub air_brake: Option<Value>,
    pub engine_on: bool,
    pub speed_control_armed: bool,
    pub speed_control_target_mph: Option<f64>,
    pub trailer_refused: bool,
    /// One per route, in the routes' order: the 511 construction on that
    /// option in dispatch's words, or empty.
    pub notes: Vec<String>,
    /// Why dispatch put the first route first, when construction decided it.
    pub dispatch_note: String,
    /// The co-driver takes the wheel from the dock.
    pub team: Option<TeamMode>,
}

impl Default for RouteSelectOptions {
    fn default() -> Self {
        RouteSelectOptions {
            back_label: "Back to dispatch board".to_string(),
            air_brake: None,
            engine_on: false,
            speed_control_armed: false,
            speed_control_target_mph: None,
            trailer_refused: false,
            notes: Vec::new(),
            dispatch_note: String::new(),
            team: None,
        }
    }
}

pub struct RouteSelectState {
    menu: MenuCore<Self>,
    pub job: Job,
    pub routes: Vec<Route>,
    opts: RouteSelectOptions,
}

impl RouteSelectState {
    pub fn new(
        ctx: &mut GameContext,
        job: Job,
        routes: Vec<Route>,
        opts: RouteSelectOptions,
    ) -> Self {
        let world = ctx.world;
        if let Some(provider) = ctx.real_weather_provider() {
            for route in &routes {
                for name in &route.cities {
                    if let Some(city) = world.cities.get(name) {
                        provider.request(&city.key, city.lat, city.lon);
                    }
                }
            }
        }
        RouteSelectState {
            menu: MenuCore::new("Route planning")
                .with_intro_help("W reads the weather along a route. Enter starts the drive."),
            job,
            routes,
            opts,
        }
    }

    /// The cities a route passes through, state-qualified so an unknown
    /// town still points the compass ("McCall, Idaho": ah, we head north).
    /// Long chains are capped for speech; F1 help reads the full list.
    fn via_text(ctx: &GameContext, route: &Route) -> String {
        let vias = Self::vias(ctx, route);
        if vias.is_empty() {
            return "passing no major cities".to_string();
        }
        if vias.len() > 3 {
            return format!(
                "through {}, and {} more",
                vias[..3].join(", "),
                vias.len() - 3
            );
        }
        format!("through {}", vias.join(", "))
    }

    fn vias(ctx: &GameContext, route: &Route) -> Vec<String> {
        let n = route.cities.len();
        if n < 2 {
            return Vec::new();
        }
        route.cities[1..n - 1]
            .iter()
            .map(|name| ctx.world.spoken_city(name, Some(true)))
            .collect()
    }

    fn speak_forecast(&mut self, ctx: &mut GameContext, route_index: usize) {
        let world = ctx.world;
        let live_calendar = ctx.settings.live_weather_controls_calendar;
        let calendar_hours = profile(ctx).calendar_game_hours();
        let cities: Vec<String> = self.routes[route_index]
            .cities
            .iter()
            .skip(1)
            .take(5)
            .cloned()
            .collect();
        let live: Option<Vec<String>> = ctx.real_weather_provider().map(|provider| {
            let mut parts = Vec::new();
            for name in &cities {
                let Some(city) = world.cities.get(name) else {
                    continue;
                };
                let kind = provider.get(&city.key);
                let spoken_city = world.spoken_city(name, Some(true));
                let Some(kind) = kind else {
                    let status = if provider.unavailable(&city.key) {
                        "no live weather"
                    } else {
                        "live weather still loading"
                    };
                    parts.push(format!("{spoken_city}: {status}"));
                    continue;
                };
                let hours = if live_calendar {
                    real_clock_game_hours(None)
                } else {
                    calendar_hours
                };
                let observed = if live_calendar {
                    provider.get_temperature(&city.key)
                } else {
                    None
                };
                let kind = adjust_for_calendar(
                    kind,
                    Some(observed.unwrap_or_else(|| temperature_c(&city.region, hours))),
                    Some(hours),
                );
                parts.push(format!("{spoken_city}: {}", kind.value()));
            }
            parts
        });
        if let Some(parts) = live {
            ctx.say(&format!("Weather along the route. {}.", parts.join(". ")));
            return;
        }
        let mut regions: Vec<String> = Vec::new();
        for city_name in &self.routes[route_index].cities {
            let Some(city) = world.cities.get(city_name) else {
                continue;
            };
            if regions.last() != Some(&city.region) {
                regions.push(city.region.clone());
            }
        }
        let parts: Vec<String> = regions
            .iter()
            .take(4)
            .map(|region| {
                let ws = WeatherSystem::new(region, None, None, None, true);
                format!("{}: {}", region.replace('_', " "), ws.current.value())
            })
            .collect();
        ctx.say(&format!("Forecast along the route. {}.", parts.join(". ")));
    }

    fn start(&mut self, ctx: &mut GameContext, route_index: usize) {
        let route = self.routes[route_index].clone();
        start_loaded_drive(
            ctx,
            self.job.clone(),
            route,
            LoadedDriveOptions {
                air_brake: self.opts.air_brake.clone(),
                engine_on: self.opts.engine_on,
                speed_control_armed: self.opts.speed_control_armed,
                speed_control_target_mph: self.opts.speed_control_target_mph,
                trailer_refused: self.opts.trailer_refused,
                // The route screen just named the destination.
                lead: String::new(),
                team: self.opts.team,
            },
        );
    }
}

impl Menu for RouteSelectState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        let current = self.current_text(ctx);
        let dispatch_note = if self.opts.dispatch_note.is_empty() {
            String::new()
        } else {
            format!("{} ", self.opts.dispatch_note)
        };
        ctx.say(&format!(
            "Route planning to {}. \
             {} route option{}. {dispatch_note}{current}",
            self.job.spoken_destination(),
            self.routes.len(),
            if self.routes.len() != 1 { "s" } else { "" }
        ));
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        let clock = ctx
            .profile
            .as_ref()
            .filter(|_| limits(&ctx.settings.hos_mode).is_some())
            .map(|p| p.hos.clone());
        for (i, route) in self.routes.iter().enumerate() {
            let note = self
                .opts
                .notes
                .get(i)
                .filter(|note| !note.is_empty())
                .map(|note| format!(" {note}"))
                .unwrap_or_default();
            let label = format!(
                "Route {}: \
                 {}, \
                 {}. \
                 {}{note}",
                i + 1,
                route.describe(&ctx.settings.distance_text(route.miles(), false)),
                Self::via_text(ctx, route),
                route_planning_summary(route, clock.as_ref(), self.job.team)
            );
            let mut vias = Self::vias(ctx, route);
            if vias.is_empty() {
                vias.push("no major cities".to_string());
            }
            items.push(
                MenuItem::new(label, move |s: &mut Self, ctx| s.start(ctx, i))
                    .help(format!("Via {}. W for weather.", vias.join(", "))),
            );
        }
        let back_label = self.opts.back_label.clone();
        items.push(MenuItem::new(back_label, |s: &mut Self, ctx| {
            s.go_back(ctx)
        }));
        items
    }

    fn handle_event(&mut self, ctx: &mut GameContext, event: &InputEvent) {
        if let Some((key, _, _)) = event.key_down() {
            if key == Key::W && self.menu.index < self.routes.len() {
                let index = self.menu.index;
                self.speak_forecast(ctx, index);
                return;
            }
        }
        base_menu_handle_event(self, ctx, event);
    }
}

impl_state_for_menu!(RouteSelectState);
