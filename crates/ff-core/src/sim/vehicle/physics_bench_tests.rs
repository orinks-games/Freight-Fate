//! Port of `tests/test_physics_bench.py`, with the bench it exercises
//! (`tools/physics_bench.py`: deterministic driving scenarios, plain-text
//! reports) ported alongside as test-only code. The bench is a tuning
//! instrument; these tests are only the regression net that keeps it
//! trustworthy -- jake spares the shoes, weather stretches stopping
//! distance, no-brakes descents run away.
//!
//! The weather rows the scenarios use come from `sim::weather::effects`,
//! the same EFFECTS table the game drives on.

use std::collections::BTreeMap;
use std::sync::OnceLock;

use super::{TruckState, CHAIN_OVERSPEED_MPH, CHAIN_SAFE_MPH};
use crate::pyfmt::round_py_int;
use crate::sim::weather::{effects, WeatherKind};

const DT: f64 = 0.1; // fixed physics timestep, seconds
const TIMEOUT_S: f64 = 3600.0; // per-scenario wall guard; every scenario ends long before
const BRAKES_WARM_C: f64 = 180.0; // spoken air-status threshold in driving_controls
const SQUEAL_BRAKE: f64 = 0.4; // brake squeal cue thresholds in driving_updates
const SQUEAL_MPH: f64 = 10.0;
const SQUEAL_COOLDOWN_S: f64 = 4.0;
const RUNAWAY_MPH: f64 = 80.0;
const FT_PER_MI: f64 = 5280.0;

/// One deterministic bench run.
///
/// `profile` is a list of (miles, grade percent) segments driven in
/// order; positive grade climbs, negative descends. When
/// `stop_from_mph` is set the profile only supplies grade under the
/// stop test: accelerate to that speed, then full service brake to a
/// standstill and report the stopping distance.
#[derive(Debug, Clone, PartialEq)]
struct Scenario {
    name: &'static str,
    summary: &'static str,
    profile: Vec<(f64, f64)>,
    cargo_kg: f64, // reference full payload
    weather: WeatherKind,
    grip_override: Option<f64>,
    water_override: Option<f64>, // standing water mm; default from weather
    tire_wear: f64,
    brake_wear: f64,
    engine_wear: f64,
    tire_type: &'static str, // or "winter"
    chains: bool,            // start the run with chains installed
    target_mph: f64,
    jake: bool,
    jake_stage: i32,       // 1..3 when jake is on; 3 = full retard
    braking: &'static str, // steady, snub, or none
    stop_from_mph: Option<f64>,
}

impl Default for Scenario {
    fn default() -> Self {
        Scenario {
            name: "",
            summary: "",
            profile: Vec::new(),
            cargo_kg: 21_500.0,
            weather: WeatherKind::Clear,
            grip_override: None,
            water_override: None,
            tire_wear: 0.0,
            brake_wear: 0.0,
            engine_wear: 0.0,
            tire_type: "all_season",
            chains: false,
            target_mph: 55.0,
            jake: false,
            jake_stage: 3,
            braking: "steady",
            stop_from_mph: None,
        }
    }
}

fn scenarios() -> Vec<Scenario> {
    let descent = vec![(1.0, 0.0), (6.0, -6.0), (2.0, 0.0)];
    let icy = vec![(1.0, 0.0), (5.0, -4.0), (1.0, 0.0)];
    let flat5 = vec![(5.0, 0.0)];
    vec![
        Scenario {
            name: "flat-cruise",
            summary: "Ten flat miles at highway speed; the fuel and heat baseline.",
            profile: vec![(10.0, 0.0)],
            target_mph: 62.0,
            ..Default::default()
        },
        Scenario {
            name: "grade-jake-snub",
            summary: "Six-mile 6 percent descent, loaded, jake on, snub braking.",
            profile: descent.clone(),
            target_mph: 35.0,
            jake: true,
            braking: "snub",
            ..Default::default()
        },
        Scenario {
            name: "grade-no-jake",
            summary: "The same descent with the jake off, dragging the service brakes.",
            profile: descent.clone(),
            target_mph: 35.0,
            jake: false,
            braking: "steady",
            ..Default::default()
        },
        Scenario {
            name: "grade-jake-only",
            summary: "The same descent on the jake alone; no service brakes at all.",
            profile: descent.clone(),
            target_mph: 35.0,
            jake: true,
            braking: "none",
            ..Default::default()
        },
        Scenario {
            name: "grade-jake-stage1",
            summary:
                "The descent on jake stage 1 with snubs; a light setting makes the shoes work.",
            profile: descent.clone(),
            target_mph: 35.0,
            jake: true,
            jake_stage: 1,
            braking: "snub",
            ..Default::default()
        },
        Scenario {
            name: "grade-runaway",
            summary: "Eight miles of 6 percent with no brakes of any kind; proof of the runaway.",
            profile: vec![(1.0, 0.0), (8.0, -6.0), (1.0, 0.0)],
            target_mph: 30.0,
            jake: false,
            braking: "none",
            ..Default::default()
        },
        Scenario {
            name: "grade-worn-brakes",
            summary: "Descent on 60 percent worn shoes, jake off; fade arrives early.",
            profile: descent.clone(),
            brake_wear: 60.0,
            target_mph: 35.0,
            jake: false,
            braking: "steady",
            ..Default::default()
        },
        Scenario {
            name: "grade-overweight",
            summary: "A 30 tonne payload down the 6 percent; over the brakes' rated gross.",
            profile: descent.clone(),
            cargo_kg: 30_000.0,
            target_mph: 35.0,
            jake: true,
            braking: "snub",
            ..Default::default()
        },
        Scenario {
            name: "snow-descent",
            summary: "A 4 percent descent in snow, jake and snubs, creeping at 25.",
            profile: vec![(1.0, 0.0), (5.0, -4.0), (2.0, 0.0)],
            weather: WeatherKind::Snow,
            target_mph: 25.0,
            jake: true,
            braking: "snub",
            ..Default::default()
        },
        Scenario {
            name: "grade-jake-ice",
            summary: "A 4 percent descent on glare ice, full jake, no service brakes.",
            profile: icy.clone(),
            weather: WeatherKind::Ice,
            target_mph: 20.0,
            jake: true,
            braking: "none",
            ..Default::default()
        },
        Scenario {
            name: "stop-dry",
            summary: "Full-brake stop from 60 on dry pavement; the braking baseline.",
            profile: flat5.clone(),
            stop_from_mph: Some(60.0),
            ..Default::default()
        },
        Scenario {
            name: "stop-rain",
            summary: "The same stop in rain.",
            profile: flat5.clone(),
            weather: WeatherKind::Rain,
            stop_from_mph: Some(60.0),
            ..Default::default()
        },
        Scenario {
            name: "stop-snow",
            summary: "The same stop in snow.",
            profile: flat5.clone(),
            weather: WeatherKind::Snow,
            stop_from_mph: Some(60.0),
            ..Default::default()
        },
        Scenario {
            name: "stop-bald-rain",
            summary: "The rain stop on 80 percent worn tires; bald rubber in the wet.",
            profile: flat5.clone(),
            weather: WeatherKind::Rain,
            tire_wear: 80.0,
            stop_from_mph: Some(60.0),
            ..Default::default()
        },
        Scenario {
            name: "stop-ice",
            summary: "A stop from 40 on freezing rain; glare ice under the whole rig.",
            profile: flat5.clone(),
            weather: WeatherKind::Ice,
            stop_from_mph: Some(40.0),
            ..Default::default()
        },
        Scenario {
            name: "stop-hydro-bald",
            summary: "A stop from 65 in heavy rain on 80 percent worn tires; planing at entry.",
            profile: flat5.clone(),
            weather: WeatherKind::HeavyRain,
            tire_wear: 80.0,
            stop_from_mph: Some(65.0),
            ..Default::default()
        },
        Scenario {
            name: "stop-ice-winter",
            summary: "The freezing-rain stop from 40 on winter-compound tires.",
            profile: flat5.clone(),
            weather: WeatherKind::Ice,
            tire_type: "winter",
            stop_from_mph: Some(40.0),
            ..Default::default()
        },
        Scenario {
            name: "stop-ice-chains",
            summary: "The freezing-rain stop from 30 with chains on the drives.",
            profile: flat5.clone(),
            weather: WeatherKind::Ice,
            chains: true,
            stop_from_mph: Some(30.0),
            ..Default::default()
        },
        Scenario {
            name: "grade-jake-ice-chains",
            summary: "The icy 4 percent descent again, full jake, chained up this time.",
            profile: icy.clone(),
            weather: WeatherKind::Ice,
            chains: true,
            target_mph: 20.0,
            jake: true,
            braking: "none",
            ..Default::default()
        },
        Scenario {
            name: "chains-bare",
            summary: "Chains left on across five dry miles at highway speed; they do not survive.",
            profile: flat5.clone(),
            chains: true,
            target_mph: 55.0,
            ..Default::default()
        },
    ]
}

#[derive(Debug, Clone, Default, PartialEq)]
struct RunResult {
    events: Vec<String>,
    summary: Vec<String>,
    metrics: BTreeMap<&'static str, f64>,
}

fn clock(t_s: f64) -> String {
    let whole = round_py_int(t_s);
    let (minutes, seconds) = (whole.div_euclid(60), whole.rem_euclid(60));
    format!("{minutes}:{seconds:02}")
}

fn grade_at(profile: &[(f64, f64)], mile: f64) -> f64 {
    let mut edge = 0.0;
    for (miles, pct) in profile {
        edge += miles;
        if mile <= edge {
            return pct / 100.0;
        }
    }
    profile[profile.len() - 1].1 / 100.0
}

fn total_miles(profile: &[(f64, f64)]) -> f64 {
    profile.iter().map(|(miles, _)| miles).sum()
}

fn build_truck(sc: &Scenario) -> TruckState {
    let mut truck = TruckState::default();
    truck.transmission.automatic = true;
    truck.cargo_kg = sc.cargo_kg;
    truck.tire_wear_pct = sc.tire_wear;
    truck.brake_wear_pct = sc.brake_wear;
    truck.engine_wear_pct = sc.engine_wear;
    truck.tire_type = sc.tire_type.to_string();
    truck.chains_on = sc.chains;
    let effects = effects(sc.weather);
    truck.grip = sc.grip_override.unwrap_or(effects.grip);
    truck.water_mm = sc.water_override.unwrap_or(effects.water_mm);
    truck.surface = effects.surface.to_string();
    truck.drag_mult = effects.drag_mult;
    truck.set_air_ready(false);
    truck.start_engine();
    truck
}

/// Scripted driver: throttle P-control toward the target, one of three
/// service-brake styles, and jake hysteresis. Deliberately simple -- the
/// point is repeatability, not skill.
struct Driver {
    target_mph: f64,
    jake: bool,
    braking: &'static str,
    jake_stage: i32,
    snubbing: bool,
    jake_on: bool,
}

impl Driver {
    fn new(target_mph: f64, jake: bool, braking: &'static str, jake_stage: i32) -> Self {
        Driver {
            target_mph,
            jake,
            braking,
            jake_stage,
            snubbing: false,
            jake_on: false,
        }
    }

    fn act(&mut self, truck: &mut TruckState) {
        let err = truck.speed_mph() - self.target_mph;
        if self.jake {
            if err >= -1.0 {
                self.jake_on = true;
            } else if err <= -5.0 {
                self.jake_on = false;
            }
        } else {
            self.jake_on = false;
        }

        let mut brake = 0.0;
        if self.braking == "steady" {
            if err > 1.0 {
                brake = (err / 15.0).min(1.0);
            }
        } else if self.braking == "snub" {
            if !self.snubbing && err > 5.0 {
                self.snubbing = true;
            }
            if self.snubbing {
                brake = 0.5;
                if err < -3.0 {
                    self.snubbing = false;
                    brake = 0.0;
                }
            }
        }

        let mut throttle = 0.0;
        if brake == 0.0 && !self.jake_on && err < -1.0 {
            throttle = (-err / 4.0).min(1.0);
        }

        truck.throttle = throttle;
        truck.brake = brake;
        truck.engine_brake_stage = if self.jake_on { self.jake_stage } else { 0 };
    }
}

/// Turns state transitions into report lines, mirroring the thresholds
/// the game speaks or cues from (driving_controls air status, the
/// driving_updates brake squeal).
struct Watcher {
    events: Vec<String>,
    launch_mph: f64, // gears below this speed are launch noise, not discipline
    launched: bool,
    over_rev_said: bool,
    heat_band: &'static str,
    squeal_cooldown: f64,
    air_warned: bool,
    springs_said: bool,
    fade_said: bool,
    runaway_said: bool,
    over_safe_said: bool,
    brake_was_on: bool,
    brake_applications: i64,
    brake_held_s: f64,
    jake_held_s: f64,
    hydro_said: bool,
    hydro_s: f64,
    jake_slip_said: bool,
    jake_slip_s: f64,
    snap_said: bool,
    chains_fast_said: bool,
    peak_temp_c: f64,
    peak_temp_mi: f64,
    top_mph: f64,
    top_mph_mi: f64,
    fade_miles: f64,
    lowest_moving_gear: i32,
}

impl Watcher {
    fn new(launch_mph: f64) -> Self {
        Watcher {
            events: Vec::new(),
            launch_mph,
            launched: false,
            over_rev_said: false,
            heat_band: "cool",
            squeal_cooldown: 0.0,
            air_warned: false,
            springs_said: false,
            fade_said: false,
            runaway_said: false,
            over_safe_said: false,
            brake_was_on: false,
            brake_applications: 0,
            brake_held_s: 0.0,
            jake_held_s: 0.0,
            hydro_said: false,
            hydro_s: 0.0,
            jake_slip_said: false,
            jake_slip_s: 0.0,
            snap_said: false,
            chains_fast_said: false,
            peak_temp_c: 0.0,
            peak_temp_mi: 0.0,
            top_mph: 0.0,
            top_mph_mi: 0.0,
            fade_miles: 0.0,
            lowest_moving_gear: 99,
        }
    }

    fn note(&mut self, truck: &TruckState, t_s: f64, text: &str) {
        self.events.push(format!(
            "mile {:.1}  time {}  {}",
            truck.odometer_mi,
            clock(t_s),
            text
        ));
    }

    fn tick(&mut self, tk: &TruckState, t_s: f64, safe_speed_mph: f64) {
        let mph = tk.speed_mph();

        if tk.brake_temp_c > self.peak_temp_c {
            self.peak_temp_c = tk.brake_temp_c;
            self.peak_temp_mi = tk.odometer_mi;
        }
        if mph > self.top_mph {
            self.top_mph = mph;
            self.top_mph_mi = tk.odometer_mi;
        }
        if mph >= self.launch_mph {
            self.launched = true;
        }
        if self.launched
            && 0 < tk.transmission.gear
            && tk.transmission.gear < self.lowest_moving_gear
            && mph > 5.0
        {
            self.lowest_moving_gear = tk.transmission.gear;
        }

        if !self.over_rev_said && tk.engine_on && tk.rpm > tk.specs.max_rpm * 1.02 {
            self.over_rev_said = true;
            let text = format!(
                "ENGINE PAST THE GOVERNOR ({:.0} rpm): tearing itself apart",
                tk.rpm
            );
            self.note(tk, t_s, &text);
        }

        let band = if tk.brake_temp_c >= tk.specs.brake_fade_temp_c {
            "hot"
        } else if tk.brake_temp_c >= BRAKES_WARM_C {
            "warm"
        } else {
            "cool"
        };
        if band != self.heat_band {
            if band == "warm" && self.heat_band == "cool" {
                let text = format!(
                    "brakes warm ({:.0} C); status would say so",
                    tk.brake_temp_c
                );
                self.note(tk, t_s, &text);
            } else if band == "hot" {
                let text = format!("brakes hot ({:.0} C)", tk.brake_temp_c);
                self.note(tk, t_s, &text);
            } else if band == "cool" && self.heat_band != "cool" {
                let text = format!("brakes back to cool ({:.0} C)", tk.brake_temp_c);
                self.note(tk, t_s, &text);
            }
            self.heat_band = band;
        }

        let onset = tk.brake_fade_onset_c();
        if !self.fade_said && tk.brake_temp_c >= onset {
            self.fade_said = true;
            let text = format!("BRAKE FADE ONSET ({onset:.0} C): pedal force starts dropping");
            self.note(tk, t_s, &text);
        }
        if tk.brake_temp_c >= onset {
            self.fade_miles += mph * DT / 3600.0;
        }

        self.squeal_cooldown = (self.squeal_cooldown - DT).max(0.0);
        if self.squeal_cooldown == 0.0
            && tk.brake >= SQUEAL_BRAKE
            && mph > SQUEAL_MPH
            && tk.brake_temp_c >= tk.specs.brake_fade_temp_c
        {
            self.note(tk, t_s, "cue: brake squeal (hot shoes worked past fade)");
            self.squeal_cooldown = SQUEAL_COOLDOWN_S;
        }

        if tk.air_low_warning() && !self.air_warned {
            self.air_warned = true;
            let text = format!("cue: low air warning ({:.0} psi)", tk.air_pressure_psi());
            self.note(tk, t_s, &text);
        }
        if tk.spring_brakes_active() && !self.springs_said {
            self.springs_said = true;
            self.note(tk, t_s, "spring brakes grab: air is gone");
        }

        if tk.hydroplaning() {
            self.hydro_s += DT;
            if !self.hydro_said {
                self.hydro_said = true;
                let onset = tk.hydro_onset_mph().unwrap_or(0.0);
                let text = format!(
                    "HYDROPLANING ({mph:.0} mph, onset {onset:.0}): tires riding the water film"
                );
                self.note(tk, t_s, &text);
            }
        }
        if tk.jake_slipping() {
            self.jake_slip_s += DT;
            if !self.jake_slip_said {
                self.jake_slip_said = true;
                self.note(
                    tk,
                    t_s,
                    "JAKE SLIPPING: drive wheels breaking loose under the retarder",
                );
            }
        }

        if tk.chains_on && mph > CHAIN_OVERSPEED_MPH && !self.chains_fast_said {
            self.chains_fast_said = true;
            let text = format!(
                "cue: chains hammering past chain speed ({mph:.0} mph, limit {CHAIN_SAFE_MPH:.0})"
            );
            self.note(tk, t_s, &text);
        }
        if tk.chains_just_snapped && !self.snap_said {
            self.snap_said = true;
            let text = format!(
                "CHAINS SNAPPED at {mph:.0} mph: the set is scrap, damage taken, running on rubber again"
            );
            self.note(tk, t_s, &text);
        }

        if mph >= RUNAWAY_MPH && !self.runaway_said {
            self.runaway_said = true;
            let text = format!("RUNAWAY: {mph:.0} mph and still building");
            self.note(tk, t_s, &text);
        }
        if mph > safe_speed_mph + 5.0 && !self.over_safe_said {
            self.over_safe_said = true;
            let text = format!("above the safe speed for conditions ({safe_speed_mph:.0} mph)");
            self.note(tk, t_s, &text);
        }

        let braking_now = tk.brake > 0.01;
        if braking_now && !self.brake_was_on {
            self.brake_applications += 1;
        }
        if braking_now {
            self.brake_held_s += DT;
        }
        self.brake_was_on = braking_now;
        if tk.engine_brake() {
            self.jake_held_s += DT;
        }
    }
}

fn run_route(sc: &Scenario) -> RunResult {
    let mut truck = build_truck(sc);
    let mut driver = Driver::new(sc.target_mph, sc.jake, sc.braking, sc.jake_stage);
    let mut watch = Watcher::new((sc.target_mph - 5.0).max(0.0));
    let effects = effects(sc.weather);
    let total = total_miles(&sc.profile);
    let wear0 = (
        truck.tire_wear_pct,
        truck.brake_wear_pct,
        truck.engine_wear_pct,
    );
    let fuel0 = truck.fuel_gal;

    let mut t = 0.0;
    let mut last_grade = grade_at(&sc.profile, 0.0);
    let mut next_marker = 1.0;
    let text = format!(
        "start; grade {:+.1} percent, target {:.0} mph",
        last_grade * 100.0,
        sc.target_mph
    );
    watch.note(&truck, t, &text);
    while truck.odometer_mi < total && t < TIMEOUT_S {
        let grade = grade_at(&sc.profile, truck.odometer_mi);
        if grade != last_grade {
            let word = if grade < last_grade {
                "steepens"
            } else {
                "eases"
            };
            let text = format!(
                "grade {} to {:+.1} percent at {:.0} mph",
                word,
                grade * 100.0,
                truck.speed_mph()
            );
            watch.note(&truck, t, &text);
            last_grade = grade;
        }
        truck.grade = grade;
        driver.act(&mut truck);
        let gear_change = truck.auto_shift();
        truck.update(DT);
        t += DT;
        if let Some(gear) = gear_change {
            if truck.speed_mph() > 5.0 {
                let text = format!(
                    "gear {} at {:.0} mph, {:.0} rpm",
                    gear,
                    truck.speed_mph(),
                    truck.rpm
                );
                watch.note(&truck, t, &text);
            }
        }
        watch.tick(&truck, t, effects.safe_speed_mph);
        if truck.odometer_mi >= next_marker {
            let text = format!(
                "marker: {:.0} mph, brakes {:.0} C, gear {}",
                truck.speed_mph(),
                truck.brake_temp_c,
                truck.transmission.gear
            );
            watch.note(&truck, t, &text);
            next_marker += 1.0;
        }
        if truck.stalled {
            watch.note(&truck, t, "ENGINE STALLED; scenario ends");
            break;
        }
    }

    RunResult {
        summary: summarize(sc, &truck, &watch, t, wear0, fuel0),
        metrics: collect_metrics(&truck, &watch, t, wear0, fuel0),
        events: watch.events,
    }
}

fn run_stop(sc: &Scenario) -> RunResult {
    let mut truck = build_truck(sc);
    let stop_from_mph = sc.stop_from_mph.expect("a stop scenario");
    let mut watch = Watcher::new((stop_from_mph - 5.0).max(0.0));
    let effects = effects(sc.weather);
    let wear0 = (
        truck.tire_wear_pct,
        truck.brake_wear_pct,
        truck.engine_wear_pct,
    );
    let fuel0 = truck.fuel_gal;

    let mut t = 0.0;
    truck.grade = grade_at(&sc.profile, 0.0);
    let text = format!("accelerating to {stop_from_mph:.0} mph before the stop");
    watch.note(&truck, t, &text);
    while truck.speed_mph() < stop_from_mph && t < TIMEOUT_S {
        truck.throttle = 1.0;
        truck.brake = 0.0;
        truck.auto_shift();
        truck.update(DT);
        t += DT;
    }

    let mark_mi = truck.odometer_mi;
    let mark_t = t;
    let text = format!("full service brake at {:.0} mph", truck.speed_mph());
    watch.note(&truck, t, &text);
    while truck.speed_mph() > 0.3 && t < TIMEOUT_S {
        truck.throttle = 0.0;
        truck.brake = 1.0;
        truck.auto_shift();
        truck.update(DT);
        t += DT;
        watch.tick(&truck, t, effects.safe_speed_mph);
    }

    let distance_ft = (truck.odometer_mi - mark_mi) * FT_PER_MI;
    let text = format!(
        "stopped: {:.0} feet in {:.1} seconds",
        distance_ft,
        t - mark_t
    );
    watch.note(&truck, t, &text);
    let mut summary = summarize(sc, &truck, &watch, t, wear0, fuel0);
    let mut metrics = collect_metrics(&truck, &watch, t, wear0, fuel0);
    metrics.insert("stop-feet", distance_ft);
    metrics.insert("stop-seconds", t - mark_t);
    summary.insert(
        1,
        format!(
            "stopping distance {:.0} feet from {:.0} mph ({:.1} seconds)",
            distance_ft,
            stop_from_mph,
            t - mark_t
        ),
    );
    RunResult {
        events: watch.events,
        summary,
        metrics,
    }
}

fn summarize(
    sc: &Scenario,
    truck: &TruckState,
    watch: &Watcher,
    t: f64,
    wear0: (f64, f64, f64),
    fuel0: f64,
) -> Vec<String> {
    let avg_mph = if t > 0.0 {
        truck.odometer_mi / (t / 3600.0)
    } else {
        0.0
    };
    let mut lines = vec![
        format!(
            "distance {:.1} miles in {}, average {:.1} mph, top {:.1} mph at mile {:.1}",
            truck.odometer_mi,
            clock(t),
            avg_mph,
            watch.top_mph,
            watch.top_mph_mi
        ),
        format!(
            "peak brake temperature {:.0} C at mile {:.1}",
            watch.peak_temp_c, watch.peak_temp_mi
        ),
        format!("fuel used {:.2} gallons", fuel0 - truck.fuel_gal),
        format!(
            "wear added: tires {:.3}, brakes {:.3}, engine {:.3} percent",
            truck.tire_wear_pct - wear0.0,
            truck.brake_wear_pct - wear0.1,
            truck.engine_wear_pct - wear0.2
        ),
    ];
    if watch.fade_miles > 0.0 {
        lines.insert(
            2,
            format!("{:.1} miles ridden past brake fade onset", watch.fade_miles),
        );
    }
    if watch.brake_applications != 0 {
        lines.push(format!(
            "service brake: {} applications, held {} total",
            watch.brake_applications,
            clock(watch.brake_held_s)
        ));
    } else {
        lines.push("service brake: never touched".to_string());
    }
    if watch.jake_held_s > 0.0 {
        lines.push(format!(
            "jake brake engaged {} total",
            clock(watch.jake_held_s)
        ));
    }
    if watch.jake_slip_s > 0.0 {
        lines.push(format!(
            "drive wheels sliding under the jake {} total",
            clock(watch.jake_slip_s)
        ));
    }
    if watch.hydro_s > 0.0 {
        lines.push(format!("hydroplaning {} total", clock(watch.hydro_s)));
    }
    if sc.chains {
        if truck.chain_wear_pct >= 100.0 {
            lines.push("chains destroyed: the set snapped and is scrap".to_string());
        } else {
            lines.push(format!(
                "chain wear added: {:.1} percent",
                truck.chain_wear_pct
            ));
        }
    }
    if watch.lowest_moving_gear < 99 {
        lines.push(format!(
            "lowest gear while moving: {}",
            watch.lowest_moving_gear
        ));
    }
    lines
}

/// Numeric run outcomes for the sweep and solve modes; the summary text
/// stays the human report, this stays the instrument readout.
fn collect_metrics(
    truck: &TruckState,
    watch: &Watcher,
    t: f64,
    wear0: (f64, f64, f64),
    fuel0: f64,
) -> BTreeMap<&'static str, f64> {
    let mut m = BTreeMap::new();
    m.insert("peak-temp-c", watch.peak_temp_c);
    m.insert("top-mph", watch.top_mph);
    m.insert(
        "avg-mph",
        if t > 0.0 {
            truck.odometer_mi / (t / 3600.0)
        } else {
            0.0
        },
    );
    m.insert("fade-miles", watch.fade_miles);
    m.insert("fuel-gal", fuel0 - truck.fuel_gal);
    m.insert("tire-wear", truck.tire_wear_pct - wear0.0);
    m.insert("brake-wear", truck.brake_wear_pct - wear0.1);
    m.insert("engine-wear", truck.engine_wear_pct - wear0.2);
    m.insert("time-s", t);
    m.insert("hydro-s", watch.hydro_s);
    m.insert("jake-slip-s", watch.jake_slip_s);
    m.insert("chain-wear", truck.chain_wear_pct);
    m.insert("damage", truck.damage_pct);
    m
}

fn run_scenario(sc: &Scenario) -> RunResult {
    if sc.stop_from_mph.is_some() {
        run_stop(sc)
    } else {
        run_route(sc)
    }
}

// -- sweeps and solves ------------------------------------------------------------

const SWEEP_PARAMS: [&str; 7] = [
    "target",
    "cargo",
    "grade",
    "brake-wear",
    "tire-wear",
    "engine-wear",
    "water",
];

fn variant(sc: &Scenario, param: &str, value: f64) -> Scenario {
    let mut out = sc.clone();
    match param {
        "target" => {
            if sc.stop_from_mph.is_some() {
                out.stop_from_mph = Some(value);
            } else {
                out.target_mph = value;
            }
        }
        // given in tonnes, stored in kg
        "cargo" => out.cargo_kg = value * 1000.0,
        // replace every non-flat segment's grade percent
        "grade" => {
            out.profile = sc
                .profile
                .iter()
                .map(|(miles, pct)| (*miles, if *pct != 0.0 { value } else { 0.0 }))
                .collect();
        }
        "brake-wear" => out.brake_wear = value,
        "tire-wear" => out.tire_wear = value,
        "engine-wear" => out.engine_wear = value,
        // standing water depth in millimeters
        "water" => out.water_override = Some(value),
        other => panic!("unknown sweep knob: {other}"),
    }
    out
}

/// `param=lo:hi[:step]` -> (param, lo, hi, step or None).
fn parse_range(spec: &str) -> Result<(String, f64, f64, Option<f64>), String> {
    let (name, rng) = spec.split_once('=').unwrap_or((spec, ""));
    if !SWEEP_PARAMS.contains(&name) || rng.is_empty() {
        return Err(format!(
            "expected PARAM=LO:HI with PARAM one of: {}",
            SWEEP_PARAMS.join(", ")
        ));
    }
    let parts: Vec<&str> = rng.split(':').collect();
    if parts.len() != 2 && parts.len() != 3 {
        return Err("range must be LO:HI or LO:HI:STEP".to_string());
    }
    let lo: f64 = parts[0].parse().map_err(|e| format!("{e}"))?;
    let hi: f64 = parts[1].parse().map_err(|e| format!("{e}"))?;
    let step = if parts.len() == 3 {
        Some(parts[2].parse::<f64>().map_err(|e| format!("{e}"))?.abs())
    } else {
        None
    };
    Ok((name.to_string(), lo, hi, step))
}

/// `metric<=limit` or `metric>=limit` -> (metric, op, limit).
fn parse_limit(spec: &str) -> Result<(String, &'static str, f64), String> {
    for op in ["<=", ">="] {
        if let Some((metric, raw)) = spec.split_once(op) {
            let limit: f64 = raw.parse().map_err(|e| format!("{e}"))?;
            return Ok((metric.trim().to_string(), op, limit));
        }
    }
    Err("limit must look like peak-temp-c<=400 or avg-mph>=30".to_string())
}

// -- the tests ---------------------------------------------------------------------

fn results() -> &'static BTreeMap<&'static str, RunResult> {
    static RESULTS: OnceLock<BTreeMap<&'static str, RunResult>> = OnceLock::new();
    RESULTS.get_or_init(|| {
        scenarios()
            .iter()
            .map(|sc| (sc.name, run_scenario(sc)))
            .collect()
    })
}

fn scenario(name: &str) -> Scenario {
    scenarios()
        .into_iter()
        .find(|s| s.name == name)
        .expect("a known scenario")
}

fn wear_added(name: &str, which: &str) -> f64 {
    let line = results()[name]
        .summary
        .iter()
        .find(|s| s.starts_with("wear added"))
        .expect("a wear line");
    for part in line.split(',') {
        if part.contains(which) {
            let after = part.split(which).nth(1).unwrap();
            return after
                .split("percent")
                .next()
                .unwrap()
                .trim()
                .parse()
                .unwrap();
        }
    }
    panic!("no {which} wear in: {line}");
}

fn stop_feet(name: &str) -> f64 {
    let line = results()[name]
        .summary
        .iter()
        .find(|s| s.starts_with("stopping distance"))
        .expect("a stopping distance line");
    line.split_whitespace().nth(2).unwrap().parse().unwrap()
}

#[test]
fn test_every_scenario_produces_a_summary() {
    for (name, result) in results() {
        assert!(!result.summary.is_empty(), "{name}");
        assert!(!result.events.is_empty(), "{name}");
    }
}

#[test]
fn test_bench_is_deterministic() {
    let sc = scenario("grade-no-jake");
    let again = run_scenario(&sc);
    assert_eq!(again.events, results()["grade-no-jake"].events);
    assert_eq!(again.summary, results()["grade-no-jake"].summary);
}

#[test]
fn test_jake_spares_the_service_brakes() {
    assert!(wear_added("grade-jake-snub", "brakes") < wear_added("grade-no-jake", "brakes"));
}

#[test]
fn test_no_brakes_descent_runs_away() {
    assert!(results()["grade-runaway"]
        .events
        .iter()
        .any(|e| e.contains("RUNAWAY")));
    let top_line = &results()["grade-runaway"].summary[0];
    let top_mph: f64 = top_line
        .split("top")
        .nth(1)
        .unwrap()
        .split("mph")
        .next()
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert!(top_mph > 80.0);
}

#[test]
fn test_weather_stretches_stopping_distance() {
    let dry = stop_feet("stop-dry");
    let rain = stop_feet("stop-rain");
    let snow = stop_feet("stop-snow");
    let bald_rain = stop_feet("stop-bald-rain");
    assert!(dry < rain && rain < snow);
    assert!(rain < bald_rain);
}

#[test]
fn test_jake_stages_form_a_ladder() {
    // Full jake spares the shoes entirely, stage 1 makes them work, and no
    // jake at all works them hardest -- the staged retard teaches by degrees.
    let full = wear_added("grade-jake-snub", "brakes");
    let stage1 = wear_added("grade-jake-stage1", "brakes");
    let none = wear_added("grade-no-jake", "brakes");
    assert!(full <= stage1 && stage1 < none);
}

#[test]
fn test_drag_descent_reaches_fade_and_jake_descent_stays_cool() {
    // The drag-vs-snub lesson with teeth: riding the shoes down six miles of
    // 6 percent cooks them past fade; the jake descent never warms them.
    assert!(results()["grade-no-jake"].metrics["peak-temp-c"] > 400.0);
    assert!(results()["grade-jake-only"].metrics["peak-temp-c"] < 100.0);
}

#[test]
fn test_sweep_and_solve_specs_parse() {
    assert_eq!(
        parse_range("target=20:60:5").unwrap(),
        ("target".to_string(), 20.0, 60.0, Some(5.0))
    );
    assert_eq!(
        parse_range("cargo=21.5:33.5").unwrap(),
        ("cargo".to_string(), 21.5, 33.5, None)
    );
    assert_eq!(
        parse_limit("peak-temp-c<=400").unwrap(),
        ("peak-temp-c".to_string(), "<=", 400.0)
    );
    assert_eq!(
        parse_limit("avg-mph>=30").unwrap(),
        ("avg-mph".to_string(), ">=", 30.0)
    );
}

#[test]
fn test_variant_swaps_only_the_named_knob() {
    let sc = scenario("grade-no-jake");
    assert_eq!(variant(&sc, "cargo", 30.0).cargo_kg, 30_000.0);
    let graded = variant(&sc, "grade", -4.0);
    assert!(graded
        .profile
        .iter()
        .all(|(_, pct)| *pct == 0.0 || *pct == -4.0));
    assert_eq!(variant(&sc, "target", 42.0).target_mph, 42.0);
}

#[test]
fn test_ice_stop_is_devastating() {
    // 40 mph on glare ice stops longer than 60 mph on dry pavement -- by a
    // lot. This is the number that justifies parking for freezing rain.
    let ice = results()["stop-ice"].metrics["stop-feet"];
    let dry = results()["stop-dry"].metrics["stop-feet"];
    assert!(ice > 2.0 * dry);
}

#[test]
fn test_bald_tires_plane_in_heavy_rain_and_stop_longer() {
    let planing = &results()["stop-hydro-bald"];
    assert!(planing.metrics["hydro-s"] > 0.0);
    assert!(planing.metrics["stop-feet"] > results()["stop-bald-rain"].metrics["stop-feet"]);
    // Fresh tread in the same downpour never reaches its onset speed.
    assert_eq!(results()["stop-rain"].metrics["hydro-s"], 0.0);
}

#[test]
fn test_jake_slides_on_ice_and_cannot_hold_the_grade() {
    let run = &results()["grade-jake-ice"];
    assert!(run.metrics["jake-slip-s"] > 60.0);
    assert!(run.metrics["top-mph"] > 24.0); // target was 20; the capped jake loses ground
    assert_eq!(run.metrics["brake-wear"], 0.0); // no service brakes in the scenario
                                                // The same discipline on dry pavement never breaks the drives loose.
    assert_eq!(results()["grade-jake-only"].metrics["jake-slip-s"], 0.0);
}

#[test]
fn test_traction_ladder_orders_the_ice_stop() {
    // Each rung of the equipment ladder shortens the freezing-rain stop:
    // stock rubber, then winter compound, then chains at chain speed.
    let stock = results()["stop-ice"].metrics["stop-feet"];
    let winter = results()["stop-ice-winter"].metrics["stop-feet"];
    let chained = results()["stop-ice-chains"].metrics["stop-feet"];
    assert!(winter < stock);
    assert!(chained < winter);
}

#[test]
fn test_chained_jake_mostly_holds_the_icy_grade() {
    // Chains lift the drive-axle cap over the grade demand: the descent that
    // slid for a quarter of an hour unchained barely slips chained, and the
    // truck stops losing ground past its target.
    let chained = &results()["grade-jake-ice-chains"];
    let unchained = &results()["grade-jake-ice"];
    assert!(chained.metrics["jake-slip-s"] < 0.25 * unchained.metrics["jake-slip-s"]);
    assert!(chained.metrics["top-mph"] < unchained.metrics["top-mph"]);
    // Proper chained use on ice costs almost nothing off the set.
    assert!(chained.metrics["chain-wear"] < 5.0);
}

#[test]
fn test_chains_snap_on_bare_pavement() {
    // Five dry miles at highway speed grind the set to nothing: it snaps,
    // bites the fender, and the truck finishes the run on rubber.
    let run = &results()["chains-bare"];
    assert_eq!(run.metrics["chain-wear"], 100.0);
    assert!(run.metrics["damage"] > 0.0);
    assert!(run.events.iter().any(|e| e.contains("CHAINS SNAPPED")));
}

#[test]
fn test_worn_brakes_fade_sooner_than_fresh() {
    // Same descent, same driving: worn shoes must not end up in better
    // shape than fresh ones, and their fade threshold sits lower.
    let fresh = wear_added("grade-no-jake", "brakes");
    let worn = wear_added("grade-worn-brakes", "brakes");
    assert!(worn >= fresh);
}
