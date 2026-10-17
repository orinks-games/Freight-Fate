//! Team driving: the co-driver at the wheel (owner design, 2026-10-17).
//!
//! A long load can run as a team. The player hands the wheel over from the
//! pause menu, rides along in the sleeper or sleeps through the shift, and
//! takes it back with the same menu. While the co-driver drives:
//!
//! - **They use the player's own controls.** [`pilot`] decides each frame
//!   which pedal is down and which key to press, and the frame reads those
//!   through [`DrivingState::held`] in place of the keyboard and the pad.
//!   Every assist is on (the player's own set is stashed in settings and
//!   never written to the file), the box is automatic, and the screens a
//!   stop opens are worked by [`overlay`] through the same menu keys.
//! - **The logbooks swap.** The profile's clock is always the clock of
//!   whoever is driving, so every rule that reads it -- warnings, the
//!   inspection's hours check, the C readout -- reads the co-driver's while
//!   they drive. The player's own clock sits here and gets sleeper-berth
//!   time; the co-driver's sits here and gets the same while the player
//!   drives.
//! - **The road goes quiet.** `GameContext::co_driver_away` holds back
//!   every line the player did not ask for; only the summon speaks.
//! - **They hand back** when their shift ends, when the delivery is in, or
//!   when something comes up only the driver can decide.
//!
//! Nothing here is saved while the co-driver is at the wheel: a drive that
//! is saved, quit or resumed always resumes with the player driving.

pub mod handover;
pub mod overlay;
pub mod pilot;
pub mod radio;
pub mod readouts;
pub mod settle;

use serde_json::{json, Map, Value};

use ff_core::models::co_driver::{self, CoDriver, PairingReason};
use ff_core::sim::hos::HosClock;

use crate::app::{GameContext, TRANSCRIPT_TARGET};
use crate::bindings::Action;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use pilot::Pilot;
use radio::RadioMood;

/// How the player spends the co-driver's shift.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TeamMode {
    /// The drive plays out at the normal pace.
    RideAlong,
    /// The shift runs ahead without waiting, and the player wakes up where
    /// the truck got to.
    SleepThrough,
}

/// The pedals the co-driver is holding this frame.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Pedals {
    pub accelerate: bool,
    pub brake: bool,
}

/// What happened on one shift, for the wake-up summary.
#[derive(Debug, Clone, Default)]
pub struct ShiftLog {
    pub miles: f64,
    pub game_minutes: f64,
    pub fuel_stops: Vec<String>,
    pub scales: Vec<String>,
    pub notes: Vec<String>,
}

/// The co-driver while they hold the wheel.
pub struct Wheel {
    pub mode: TeamMode,
    pub pedals: Pedals,
    pub pilot: Pilot,
    pub radio: RadioMood,
    pub log: ShiftLog,
    /// Why they are handing back, once they are.
    pub summon: Option<String>,
}

/// The co-driver for this load.
pub struct TeamDrive {
    pub co_driver: &'static CoDriver,
    pub reason: PairingReason,
    /// The logbook the profile is not holding: the co-driver's while the
    /// player drives, the player's while the co-driver does.
    pub off_wheel_hos: HosClock,
    /// Miles the co-driver drove on this load.
    pub co_driver_mi: f64,
    pub wheel: Option<Wheel>,
    /// The last wake-up summary, kept for the Driver screen.
    pub last_summary: Vec<String>,
}

impl TeamDrive {
    pub fn new(co_driver: &'static CoDriver, reason: PairingReason) -> Self {
        Self {
            co_driver,
            reason,
            off_wheel_hos: HosClock::new(),
            co_driver_mi: 0.0,
            wheel: None,
            last_summary: Vec::new(),
        }
    }

    /// The co-driver's logbook, wherever it is right now.
    pub fn co_driver_hos<'a>(&'a self, ctx: &'a GameContext) -> &'a HosClock {
        if self.wheel.is_some() {
            hos_of(ctx)
        } else {
            &self.off_wheel_hos
        }
    }

    /// The player's logbook, wherever it is right now.
    pub fn player_hos<'a>(&'a self, ctx: &'a GameContext) -> &'a HosClock {
        if self.wheel.is_some() {
            &self.off_wheel_hos
        } else {
            hos_of(ctx)
        }
    }

    /// The snapshot record. Always written as if the player held the wheel,
    /// because that is how a saved drive resumes.
    pub fn snapshot(&self, ctx: &GameContext) -> Value {
        json!({
            "co_driver": self.co_driver.key,
            "reason": reason_key(self.reason),
            "co_driver_hos": self.co_driver_hos(ctx).to_dict(),
            "player_hos": self.player_hos(ctx).to_dict(),
            "co_driver_mi": self.co_driver_mi,
            "last_summary": self.last_summary,
        })
    }

    /// Rebuild from [`TeamDrive::snapshot`]; the player's logbook goes back
    /// into the profile if the save caught the co-driver at the wheel.
    pub fn from_snapshot(ctx: &mut GameContext, data: &Map<String, Value>) -> Option<Self> {
        let key = data.get("co_driver")?.as_str()?;
        let member = co_driver::co_driver(key)?;
        let reason = data
            .get("reason")
            .and_then(Value::as_str)
            .map(reason_from_key)
            .unwrap_or(PairingReason::Classmate);
        let mut team = TeamDrive::new(member, reason);
        if let Some(hos) = data.get("co_driver_hos") {
            team.off_wheel_hos = HosClock::from_dict(hos);
        }
        if let Some(hos) = data.get("player_hos") {
            *hos_mut_of(ctx) = HosClock::from_dict(hos);
        }
        team.co_driver_mi = data
            .get("co_driver_mi")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
        team.last_summary = data
            .get("last_summary")
            .and_then(Value::as_array)
            .map(|lines| {
                lines
                    .iter()
                    .filter_map(|line| line.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        Some(team)
    }
}

fn reason_key(reason: PairingReason) -> &'static str {
    match reason {
        PairingReason::FirstTeam => "first",
        PairingReason::Classmate => "classmate",
        PairingReason::Hire => "hire",
        PairingReason::Meridian => "meridian",
        PairingReason::Trainer => "trainer",
    }
}

fn reason_from_key(key: &str) -> PairingReason {
    match key {
        "first" => PairingReason::FirstTeam,
        "hire" => PairingReason::Hire,
        "meridian" => PairingReason::Meridian,
        "trainer" => PairingReason::Trainer,
        _ => PairingReason::Classmate,
    }
}

impl DrivingState {
    /// Whether the co-driver holds the wheel right now.
    pub fn co_driver_driving(&self) -> bool {
        self.team.as_ref().is_some_and(|team| team.wheel.is_some())
    }

    /// Whether this held control is down: the co-driver's pedals while they
    /// drive, the player's keyboard otherwise. The co-driver never steers
    /// or touches the emergency brake; the assists do the one and the
    /// automatic braking the other.
    pub fn held(&self, ctx: &GameContext, action: Action) -> bool {
        match self.team.as_ref().and_then(|team| team.wheel.as_ref()) {
            Some(wheel) => match action {
                Action::Accelerate => wheel.pedals.accelerate,
                Action::Brake => wheel.pedals.brake,
                _ => false,
            },
            None => ctx.bindings.pressed(&ctx.input, action),
        }
    }

    /// Whether the controller's sticks and triggers are live: never while
    /// the co-driver drives.
    pub fn pad_live(&self, ctx: &GameContext) -> bool {
        ctx.controller.active() && !self.co_driver_driving()
    }

    /// Whether a team is worth offering for the rest of this trip: the
    /// delivery leg, and long enough that driving it alone needs a sleep.
    pub fn team_available(&self, ctx: &GameContext) -> bool {
        if self.phase != DRIVE_PHASE_DELIVERY || self.trip.finished {
            return false;
        }
        if self.team.is_some() {
            return true;
        }
        let left_mi = (self.trip.total_miles() - self.trip.position_mi).max(0.0);
        // The planner's own pace for the drive ahead: the posted limits
        // less the stops, which a half-hour per hundred miles stands for.
        let drive_min_left = left_mi / 55.0 * 60.0;
        let mode = ctx.settings.hos_mode.clone();
        co_driver::team_worthwhile(self.team_player_hos(ctx), &mode, drive_min_left)
    }

    fn team_player_hos<'a>(&'a self, ctx: &'a GameContext) -> &'a HosClock {
        match &self.team {
            Some(team) => team.player_hos(ctx),
            None => hos_of(ctx),
        }
    }

    /// Pair a co-driver for this load if there is none yet.
    pub fn ensure_team(&mut self, ctx: &mut GameContext) {
        if self.team.is_some() {
            return;
        }
        let profile = profile_of(ctx);
        let pairing = co_driver::pair(
            &profile.achievement_stats,
            &profile.business_status,
            profile.career.level(),
            self.trip_seed,
        );
        let mut team = TeamDrive::new(pairing.co_driver, pairing.reason);
        // A co-driver joining mid-load has been in the bunk, rested.
        team.off_wheel_hos.sleep();
        self.team = Some(team);
    }

    /// The line naming the co-driver, for the handover.
    pub fn team_pairing_line(&self, ctx: &GameContext) -> String {
        let Some(team) = &self.team else {
            return String::new();
        };
        let pairing = co_driver::Pairing {
            co_driver: team.co_driver,
            reason: team.reason,
        };
        let carrier = ff_core::models::career::carrier_name_of(profile_of(ctx));
        co_driver::pairing_line(&pairing, &carrier)
    }

    /// Hand the wheel to the co-driver.
    pub fn hand_wheel_to_co_driver(&mut self, ctx: &mut GameContext, mode: TeamMode) {
        self.ensure_team(ctx);
        let radio_on = self.radio.enabled;
        let Some(team) = self.team.as_mut() else {
            return;
        };
        if team.wheel.is_some() {
            return;
        }
        std::mem::swap(hos_mut_of(ctx), &mut team.off_wheel_hos);
        let seed = self.trip_seed + self.trip.game_minutes as i64;
        team.wheel = Some(Wheel {
            mode,
            pedals: Pedals::default(),
            pilot: Pilot::new(),
            radio: RadioMood::new(team.co_driver, radio_on, seed),
            log: ShiftLog::default(),
            summon: None,
        });
        ctx.settings.hand_assists_to_co_driver();
        ctx.co_driver_away = true;
        if mode == TeamMode::SleepThrough {
            ctx.co_driver_fast_forward = true;
            ctx.audio
                .set_volumes(&crate::audio::VolumeUpdate::default().master(0.0));
        }
        // Nothing the player was holding carries over to the co-driver.
        self.brake_latch = Default::default();
        self.microsleep_deadline = None;
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[co-driver] {} takes the wheel ({mode:?})",
            team.co_driver.name
        );
    }

    /// Give the wheel back to the player. Returns the shift's log.
    pub fn take_wheel_back(&mut self, ctx: &mut GameContext) -> Option<ShiftLog> {
        let team = self.team.as_mut()?;
        let wheel = team.wheel.take()?;
        std::mem::swap(hos_mut_of(ctx), &mut team.off_wheel_hos);
        ctx.settings.take_assists_back();
        ctx.co_driver_away = false;
        if std::mem::take(&mut ctx.co_driver_fast_forward) {
            ctx.apply_volumes();
        }
        log::info!(
            target: TRANSCRIPT_TARGET,
            "[co-driver] hands back after {:.1} miles",
            wheel.log.miles
        );
        Some(wheel.log)
    }

    /// The per-frame team bookkeeping, run with the hours: the logbook not
    /// in use rests, the player's fatigue clears while the co-driver drives,
    /// and the co-driver's miles add up.
    pub fn update_team_hours(&mut self, ctx: &mut GameContext, dt: f64, gm: f64) {
        let speed_mph = self.trip.truck.speed_mph();
        let Some(team) = self.team.as_mut() else {
            return;
        };
        team.off_wheel_hos.sleeper(gm);
        let Some(wheel) = team.wheel.as_mut() else {
            return;
        };
        let miles = speed_mph.max(0.0) * dt / 3600.0;
        team.co_driver_mi += miles;
        wheel.log.miles += miles;
        wheel.log.game_minutes += gm;
        let profile = profile_mut_of(ctx);
        profile.fatigue = (profile.fatigue - co_driver::sleeper_fatigue_relief(gm)).max(0.0);
    }
}

/// What the co-driver says when they need the player back.
fn summon_line(name: &str, reason: &str) -> String {
    match reason {
        "gate" => format!("{name} has the truck at the receiver's gate."),
        "shift" => format!("{name} needs you back at the wheel. Their hours are up."),
        "damage" => format!("{name} needs you. The truck took damage."),
        "fuel" => format!("{name} needs you. The tank is empty."),
        "off the road" => format!("{name} needs you. The truck has left the road."),
        "stalled" => format!("{name} needs you. The truck is stopped and will not go on."),
        _ => format!("{name} needs you to decide this one."),
    }
}

/// The wake-up summary after a slept-through shift.
fn wake_summary(name: &str, log: &ShiftLog, ctx: &GameContext) -> String {
    let hours = log.game_minutes / 60.0;
    let slept = if hours >= 1.0 {
        format!("You slept {} hours.", ff_core::pyfmt::fmt_f(hours, 1))
    } else {
        format!("You slept {} minutes.", log.game_minutes.round())
    };
    let mut drove = format!(
        "{name} drove {}",
        ctx.settings.distance_text(log.miles, false)
    );
    if let Some(stop) = log.fuel_stops.last() {
        drove.push_str(&format!(" and fueled at {stop}"));
    }
    drove.push('.');
    format!("{slept} {drove}")
}

impl DrivingState {
    /// The co-driver hands back: the wheel is the player's again, the sound
    /// comes back, and one line says why.
    pub fn co_driver_summon(&mut self, ctx: &mut GameContext, reason: &str) {
        let Some(team) = self.team.as_ref() else {
            return;
        };
        let name = team.co_driver.name;
        let mode = team.wheel.as_ref().map(|wheel| wheel.mode);
        let Some(log) = self.take_wheel_back(ctx) else {
            return;
        };
        log::info!(target: TRANSCRIPT_TARGET, "[co-driver] summons the driver: {reason}");
        let mut lines = Vec::new();
        if mode == Some(TeamMode::SleepThrough) {
            lines.push(wake_summary(name, &log, ctx));
        }
        lines.push(summon_line(name, reason));
        if let Some(team) = self.team.as_mut() {
            team.last_summary = lines.clone();
        }
        ctx.audio.play("ui/notify");
        let text = lines.join(" ");
        ctx.player_asked(|ctx| ctx.say_with(text, crate::app::Say::new()));
    }

    /// The co-driver chose a row on a stop's screen: keep the shift's log,
    /// and a stop they are leaving is no longer one they are making for.
    pub fn note_co_driver_choice(&mut self, _ctx: &mut GameContext, label: &str) {
        let stop = self
            .trip
            .nearest_stop_within(1.5)
            .map(|stop| stop.spoken_name());
        let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) else {
            return;
        };
        if label.starts_with("Refuel") {
            if let Some(stop) = stop {
                wheel.log.fuel_stops.push(stop);
            }
        } else if label == "Back to the road" {
            wheel.pilot.making_for = None;
        }
    }
}
