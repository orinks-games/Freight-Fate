//! The overnight lot is full: fuel anyway, push on, or risk the shoulder
//! (`ParkingFullState`).

use ff_core::pyfmt::{fmt_f, fmt_grouped};
use ff_core::sim::hos;
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::vehicle::TruckState;

use crate::app::{GameContext, Say};
use crate::impl_state_for_menu;
use crate::states::base::{Menu, MenuCore, MenuItem};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{
    advance_rest_clock, clock_text, hos_mut_of, poi_ambient_key, profile_mut_of, profile_of,
    shut_down_engine, FacilityEngine, MOTEL_COST,
};
use crate::states::driving_menu_states::{keep_rows, DriveRef};
use crate::states::driving_rest_states::back_on_the_road_line;
use crate::states::driving_rest_states::fuel_pump::FuelPump;
use crate::states::driving_rest_states::shoulder::ShoulderSleepConfirmationState;

const PARKING_FULL_INTRO_HELP: &str = "Enter selects. Escape returns to the road.";

pub struct ParkingFullState {
    menu: MenuCore<Self>,
    driving: DriveRef,
    pub stop: RoadStop,
}

impl ParkingFullState {
    pub fn new(ctx: &GameContext, stop: RoadStop) -> Self {
        ParkingFullState {
            menu: MenuCore::new("Parking full").with_intro_help(PARKING_FULL_INTRO_HELP),
            driving: DriveRef::active(ctx),
            stop,
        }
    }

    /// The same screen over a drive the caller already shares (tests).
    pub fn with_drive(driving: DriveRef, stop: RoadStop) -> Self {
        ParkingFullState {
            menu: MenuCore::new("Parking full").with_intro_help(PARKING_FULL_INTRO_HELP),
            driving,
            stop,
        }
    }

    /// `enter()` run while the drive is still in hand -- see `drive_ref`.
    pub fn enter_over_drive(&mut self, ctx: &mut GameContext, driving: &mut DrivingState) {
        let items = self.rows(ctx, driving);
        self.menu.items = items;
        self.menu.index = self.menu.index.min(self.menu.items.len().saturating_sub(1));
        if let Some(key) = self.menu.open_sound_key.clone() {
            ctx.audio.play(&key);
        }
        self.announce_over_drive(ctx, driving);
    }

    fn announce_over_drive(&mut self, ctx: &mut GameContext, d: &mut DrivingState) {
        ctx.audio
            .set_ambient(Some(poi_ambient_key(&self.stop, d.trip.local_hour())));
        // The lot and the island are separate facilities, and a driver who
        // cannot park here can still fuel here. Saying so up front is what
        // stops a full lot from reading as a closed truck stop.
        let pumps = if self.stop.actions.iter().any(|a| a == "fuel") {
            " The fuel island is open."
        } else {
            ""
        };
        let name = self.stop.spoken_name();
        let hour = clock_text(d.trip.local_hour());
        let current = self.current_text(ctx);
        ctx.say(&format!(
            "The truck parking at {name} is full tonight.{pumps} It is {hour}. {current}"
        ));
    }

    fn rows(&mut self, ctx: &mut GameContext, d: &mut DrivingState) -> Vec<MenuItem<Self>> {
        let mut items: Vec<MenuItem<Self>> = Vec::new();
        if self.stop.actions.iter().any(|a| a == "fuel") {
            // Kill switch while parked: the road's engine key is out of reach
            // under this menu, and the fuel island refuses a running tractor.
            items.push(self.facility_engine_item_for(d.trip.truck.engine_on));
            // Pumps still lead the hospitality choices: a driver turned away
            // at 2 AM needs the tank before the choice of where to sleep.
            let label = self.fuel_label(ctx, d);
            items.push(
                MenuItem::new(label, |s: &mut Self, ctx| s.refuel(ctx)).help(
                    "Fills the tank at the posted diesel price plus a 35 dollar service fee. Engine \
                     must be off.",
                ),
            );
        }
        items.push(
            MenuItem::new("Drive on to the next stop", |s: &mut Self, ctx| {
                s.drive_on(ctx)
            })
            .help("Back to the road for the next rest stop."),
        );
        items.push(
            MenuItem::new(
                format!(
                    "Motel room: sleep 10 hours for {} dollars",
                    fmt_f(MOTEL_COST, 0)
                ),
                |s: &mut Self, ctx| s.motel(ctx),
            )
            .help(
                "A motel near the exit, paid from your own pocket. Legal 10-hour reset, you \
                 wake fresh.",
            ),
        );
        items.push(
            MenuItem::new("Park on the shoulder and sleep", |s: &mut Self, ctx| {
                s.shoulder(ctx)
            })
            .help(
                "Ten hours of poor sleep. Resets hours of service. Risks a parking fine or minor \
                 truck damage.",
            ),
        );
        items
    }

    fn drive_on(&mut self, ctx: &mut GameContext) {
        // No sleep happened here, so the engine is whatever it already was --
        // never claim it needs a restart it may not need.
        let engine_on = self
            .driving
            .read(|d| d.trip.truck.engine_on)
            .unwrap_or(false);
        ctx.audio.play("ui/menu_back");
        ctx.pop_state();
        ctx.say_with(back_on_the_road_line(engine_on), Say::new());
    }

    fn motel(&mut self, ctx: &mut GameContext) {
        let money = profile_of(ctx).money();
        if money < MOTEL_COST {
            ctx.audio.play("ui/error");
            ctx.say(&format!(
                "A motel room costs {} dollars and you have {}.",
                fmt_grouped(MOTEL_COST, 0),
                fmt_grouped(money, 0)
            ));
            return;
        }
        profile_mut_of(ctx).spend(MOTEL_COST);
        let Some(text) = self.driving.clone().with(ctx, |d, ctx| {
            // Same as every other sleep option: no truck idles all night just
            // because the driver bedded down in a motel instead of the
            // sleeper.
            let engine_off = shut_down_engine(d, ctx);
            advance_rest_clock(d, ctx, hos::SLEEP_MIN, None, "");
            hos_mut_of(ctx).sleep();
            profile_mut_of(ctx).fatigue = 0.0;
            let snapshot = d.snapshot(ctx);
            {
                let p = profile_mut_of(ctx);
                p.store_truck_condition(&d.trip.truck);
                p.active_trip = Some(snapshot);
            }
            let money = profile_of(ctx).money();
            format!(
                "{engine_off}Motel, {} dollars, 10 hours. {}. Driving and duty clocks reset. \
                 {} dollars. {} starts the engine.",
                fmt_grouped(MOTEL_COST, 0),
                clock_text(d.trip.local_hour()),
                fmt_grouped(money, 0),
                ctx.control_hint("engine")
            )
        }) else {
            return;
        };
        ctx.save_profile();
        ctx.audio.play("ui/notify");
        ctx.pop_state();
        ctx.say_with(text, Say::new());
        // No Five-by-Two here: the badge is ten hours IN THE BUNK, and a
        // motel bed is the night you specifically did not spend in it (owner
        // report, 2026-08-20). The cramped-lot sleep keeps the award -- the
        // stop has no beds, so the lot night IS a bunk night.
    }

    fn shoulder(&mut self, ctx: &mut GameContext) {
        let reason = format!(
            "The truck parking at {} is full tonight.",
            self.stop.spoken_name()
        );
        let state = ShoulderSleepConfirmationState::from_menu(
            self.driving.clone(),
            &reason,
            Some(self.stop.at_mi),
        );
        ctx.push_state(state);
    }

    /// [`FacilityEngine::facility_engine_item`] with engine state already in hand
    /// (rows build inside a DriveRef borrow).
    fn facility_engine_item_for(&self, engine_on: bool) -> MenuItem<Self> {
        if engine_on {
            MenuItem::new(
                crate::states::driving_core::FACILITY_ENGINE_SHUT_DOWN_ITEM,
                |s: &mut Self, ctx| s.toggle_facility_engine(ctx),
            )
            .help("Engine off while parked, no fuel burned. Required before the fuel island.")
        } else {
            MenuItem::new(
                crate::states::driving_core::FACILITY_ENGINE_START_ITEM,
                |s: &mut Self, ctx| s.toggle_facility_engine(ctx),
            )
            .help("Starts the engine. The parking brake needs 100 psi of air.")
        }
    }
}

impl FacilityEngine for ParkingFullState {
    fn facility_engine_on(&self, _ctx: &GameContext) -> bool {
        self.driving
            .read(|d| d.trip.truck.engine_on)
            .unwrap_or(false)
    }

    fn with_facility_truck<R>(
        &mut self,
        ctx: &mut GameContext,
        f: impl FnOnce(&mut GameContext, &mut TruckState) -> R,
    ) -> R {
        self.driving
            .clone()
            .call(self, ctx, |_s, ctx, d| f(ctx, &mut d.trip.truck))
            .expect("the parking-full menu keeps the drive under it")
    }
}

impl FuelPump for ParkingFullState {
    fn drive(&self) -> &DriveRef {
        &self.driving
    }

    fn stop(&self) -> &RoadStop {
        &self.stop
    }
}

impl Menu for ParkingFullState {
    fn menu(&self) -> &MenuCore<Self> {
        &self.menu
    }

    fn menu_mut(&mut self) -> &mut MenuCore<Self> {
        &mut self.menu
    }

    fn build_items(&mut self, ctx: &mut GameContext) -> Vec<MenuItem<Self>> {
        let built = self
            .driving
            .clone()
            .call(self, ctx, |s, ctx, d| s.rows(ctx, d));
        keep_rows(built, &self.driving, &self.menu.items)
    }

    fn announce_entry(&mut self, ctx: &mut GameContext) {
        self.driving
            .clone()
            .call(self, ctx, |s, ctx, d| s.announce_over_drive(ctx, d));
    }

    fn go_back(&mut self, ctx: &mut GameContext) {
        self.drive_on(ctx);
    }
}

impl_state_for_menu!(ParkingFullState);
