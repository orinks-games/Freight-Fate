//! Do the assists hold the road's bends without rolling the truck? (owner
//! question, 2026-09-24). The exit assist matrix answers it for ramp curves;
//! this answers it for the mainline bends, on the curviest real roads in the
//! baked world, for every load the roll model prices differently.
//!
//! Each run starts a mile and a half before a three-mile stretch of mapped
//! bends, rolling at the limit (or under the number spoken for a bend right
//! there), and drives it through the real driving state at the shipped time
//! compression, weather clear, traffic off. Setups:
//!
//! - **Curve assistance**: the default (Balanced) assists, partial lane
//!   keeping, and a driver holding the throttle to the posted limit, or to
//!   the number curve assistance says it is slowing to while it has a bend.
//! - **All assists**: the All preset, adaptive cruise set to the limit, and
//!   the speed keeper; the driver's feet stay off the pedals.
//! - **Spoken number**: Balanced with curve assistance off. The driver obeys
//!   the number the cab speaks for each bend it calls (the load's own number
//!   where it is lower than the sign's) and any "slow to" warning, a mile an
//!   hour under, with a steady foot on the brake.
//! - **Cruise**: Balanced, adaptive cruise set to the limit.
//! - **Daring**: no speed assists (lane keeping full, so the road is steered),
//!   a full trailer, and a driver who takes each bend 10 mph over its sign.
//!
//! Every setup but Daring owes: no rollover, no freight moved by a bend (a
//! frame where the bend asks past the warning share; the driver's own
//! braking is reported but is the driver's), no trip off the pavement, no
//! bend asking more than `SHARE_CEILING` of the threshold, and none asking
//! past the warning share without a "too fast" heard first. The assisted
//! ones also owe no "too fast" warning (the assist owns the speed), no more
//! than one full application of the pedal per bend, and air above the
//! low-air warning. Daring owes a warning before any cost, and a roll only at
//! the roll model's own threshold -- the proof that this sweep can see a roll.
//!
//! `BEND_SWEEP_DUMP=1` prints every run's transcript; `BEND_SWEEP_ONLY=` a
//! substring of the stretch name narrows the roads;
//! `BEND_SWEEP_TRACE=Siskiyou/CurveAssist/Full` traces one run, twice a
//! second or with `BEND_SWEEP_EVERY=1` every frame. All need `--nocapture`.

use ff_core::data::curves::RouteCurve;
use ff_core::sim::lane::OFF_ROAD;
use ff_core::sim::surge::LiquidLoad;
use ff_core::sim::vehicle::{REFERENCE_CARGO_KG, ROLL_WARN_SHARE};
use ff_core::sim::weather::WeatherKind;
use freight_fate::playtest::harness::{PlaytestHarness, RouteSetup};
use freight_fate::states::base::{Key, Mods};
use freight_fate::states::driving::DrivingState;

use crate::transcript_cruise_support::{quiet, MPS_PER_MPH};

const DT: f64 = 1.0 / 30.0;
const MPH_PER_MPS: f64 = 2.23694;
/// Road driven before the stretch, so every assist has met the first bend
/// the way it meets any other.
const LEAD_MI: f64 = 1.5;
const STRETCH_MI: f64 = 3.0;
const TOO_FAST: &str = ", too fast. Slow to";
/// The unassisted drivers' steady service application.
const DRIVER_BRAKE: f64 = 0.35;
/// The most of its rollover threshold any bend may ask of the truck in a
/// setup that is not trying to roll it: the warning share (0.857) plus the
/// frame or two the load needs to be slowed, well short of 1.0.
const SHARE_CEILING: f64 = 0.90;
/// A bend asking past the warning share has to follow a "too fast" heard
/// within this many seconds, or the driver was never told.
const WARNED_WITHIN_S: f64 = 10.0;

/// A bend-dense stretch of a real leg: its endpoints in bake direction and
/// where the stretch starts on it. Ranked 2026-09-24 by mapped bends in three
/// miles and by the lowest advisory among them.
struct Stretch {
    name: &'static str,
    a: &'static str,
    b: &'static str,
    from_mi: f64,
}

const STRETCHES: [Stretch; 12] = [
    Stretch {
        name: "I-70 Floyd Hill, CO",
        a: "denver_co_us",
        b: "silverthorne_co_us",
        from_mi: 27.5,
    },
    Stretch {
        name: "I-70 Glenwood Canyon, CO",
        a: "edwards_co_us",
        b: "glenwood_springs_co_us",
        from_mi: 38.7,
    },
    Stretch {
        name: "I-5 Siskiyou, CA-OR",
        a: "yreka_ca_us",
        b: "medford_or_us",
        from_mi: 27.3,
    },
    Stretch {
        name: "I-5 Shasta Lake, CA",
        a: "redding_ca_us",
        b: "mount_shasta_ca_us",
        from_mi: 13.6,
    },
    Stretch {
        name: "I-40 Pigeon River, NC-TN",
        a: "asheville_nc_us",
        b: "knoxville_tn_us",
        from_mi: 34.4,
    },
    Stretch {
        name: "I-80 Donner, CA",
        a: "sacramento_ca_us",
        b: "reno_nv_us",
        from_mi: 110.8,
    },
    Stretch {
        name: "I-90 Lookout Pass, ID-MT",
        a: "kellogg_id_us",
        b: "superior_mt_us",
        from_mi: 46.0,
    },
    Stretch {
        name: "US-550 Red Mountain, CO",
        a: "durango_co_us",
        b: "montrose_co_us",
        from_mi: 64.7,
    },
    Stretch {
        name: "US-62 Ozarks, AR",
        a: "bentonville_ar_us",
        b: "harrison_ar_us",
        from_mi: 34.6,
    },
    Stretch {
        name: "US-60 Salt River Canyon, AZ",
        a: "globe_az_us",
        b: "show_low_az_us",
        from_mi: 35.8,
    },
    Stretch {
        name: "CA-299 Trinity, CA",
        a: "eureka_ca_us",
        b: "redding_ca_us",
        from_mi: 41.0,
    },
    Stretch {
        name: "US-50 Allegheny, MD-WV",
        a: "cumberland_md_us",
        b: "winchester_va_us",
        from_mi: 34.6,
    },
];

#[derive(Clone, Copy, Debug, PartialEq)]
enum Load {
    Bobtail,
    Empty,
    Half,
    Full,
    Tank50,
    Tank95,
}

const LOADS: [Load; 6] = [
    Load::Bobtail,
    Load::Empty,
    Load::Half,
    Load::Full,
    Load::Tank50,
    Load::Tank95,
];

impl Load {
    fn apply(self, d: &mut DrivingState) {
        let truck = d.truck_mut();
        truck.trailer_attached = self != Load::Bobtail;
        truck.liquid = None;
        truck.cargo_kg = match self {
            Load::Bobtail | Load::Empty => 0.0,
            Load::Half => 0.5 * REFERENCE_CARGO_KG,
            Load::Full => REFERENCE_CARGO_KG,
            Load::Tank50 => 0.5 * REFERENCE_CARGO_KG,
            Load::Tank95 => 0.95 * REFERENCE_CARGO_KG,
        };
        truck.liquid = match self {
            Load::Tank50 => Some(LiquidLoad::new(0.5, false)),
            Load::Tank95 => Some(LiquidLoad::new(0.95, false)),
            _ => None,
        };
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Setup {
    CurveAssist,
    AllAssists,
    SpokenNumber,
    Cruise,
    Daring,
}

impl Setup {
    fn assisted(self) -> bool {
        matches!(self, Setup::CurveAssist | Setup::AllAssists | Setup::Cruise)
    }

    fn cruise(self) -> bool {
        matches!(self, Setup::AllAssists | Setup::Cruise)
    }
}

/// What one run did.
#[derive(Debug, Default)]
struct Run {
    max_share: f64,
    warnings: usize,
    rolled: bool,
    cargo_pct: f64,
    /// The part of it moved while the bend asked past the warning share.
    bend_cargo_pct: f64,
    off_road: bool,
    /// Whether anything had cost the truck or the load when the first warning
    /// was heard.
    cost_before_warning: bool,
    cost: bool,
    /// The roll share on the frame the truck went over.
    share_at_roll: f64,
    /// The service pedal's rises, in full applications.
    rises: f64,
    /// The lowest the air tanks fell to, and where the low-air warning is.
    min_psi: f64,
    low_air_psi: f64,
    /// The highest roll share reached past the warning share with no "too
    /// fast" heard in the `WARNED_WITHIN_S` before it; 0 if none.
    silent_over_warn: f64,
    bends: usize,
    heard: Vec<String>,
}

fn start(stretch: &Stretch, setup: Setup, load: Load) -> PlaytestHarness {
    let mut harness = PlaytestHarness::new();
    {
        let s = &mut harness.app.ctx.settings;
        s.automatic_transmission = true;
        match setup {
            Setup::CurveAssist | Setup::Cruise => {}
            Setup::SpokenNumber => s.curve_speed_assist = false,
            Setup::AllAssists => {
                s.apply_driving_assistance_preset("all");
                s.speed_keeper = true;
            }
            Setup::Daring => {
                s.apply_driving_assistance_preset("all");
                s.automatic_emergency_braking = false;
                s.stop_and_go_assist = false;
                s.descent_speed_control = "off".to_string();
                s.exit_speed_assist = false;
                s.destination_approach_assist = false;
                s.curve_speed_assist = false;
                s.route_transition_assist = false;
                s.speed_keeper = false;
            }
        }
    }
    harness.start_route(
        stretch.a,
        stretch.b,
        RouteSetup::seeded(4242).named("Bend Sweep"),
    );
    let at = (stretch.from_mi - LEAD_MI).max(0.5);
    harness.with_drive(move |d, ctx| {
        quiet(&mut d.trip);
        if let Some(profile) = ctx.profile.as_mut() {
            profile.tutorial_done = true;
        }
        d.tutorial = None;
        d.departure_checked = true;
        d.destination_exit_taken = true;
        d.weather_mut().current = WeatherKind::Clear;
        load.apply(d);
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().transmission.gear = 10;
        d.truck_mut().rpm = 1500.0;
        d.trip.position_mi = at;
        // Rolling the way a driver who has been doing this road right would
        // be: at the limit, or a mile an hour under the number spoken for a
        // bend the truck is in or about to meet. Launched at the limit into
        // a 20 mph hairpin a few hundred feet ahead, nothing could save it.
        let (limit, _) = d.trip.speed_limit_at(at);
        let near: Vec<_> = d
            .trip
            .curves
            .iter()
            .filter(|c| !c.connector && c.end_mi >= at && c.start_mi < at + 1.0)
            .copied()
            .collect();
        let speed = near
            .iter()
            .map(|c| d.spoken_advisory_mph(c) as f64 - 1.0)
            .fold(limit, f64::min);
        d.truck_mut().velocity_mps = speed * MPS_PER_MPH;
        d.truck_mut().throttle = 0.3;
    });
    if setup.cruise() {
        // Set to the limit, whatever the truck is doing as it is set.
        harness.with_drive(move |d, ctx| {
            let (limit, _) = d.trip.speed_limit_at(at);
            d.engage_cruise(ctx, limit, false);
        });
    }
    harness.clear_speech();
    harness
}

/// Deceleration needed to be at `end_mph` in `miles`, m/s^2.
fn needed(speed_mph: f64, end_mph: f64, miles: f64) -> f64 {
    let v = speed_mph / MPH_PER_MPS;
    let e = end_mph / MPH_PER_MPS;
    ((v * v - e * e) / (2.0 * (miles * 1609.344).max(1.0))).max(0.0)
}

/// Every speed the driver is holding to here, each with the road to where it
/// starts (0 when it applies now): the posted limit, and per bend within a
/// mile the number `per_bend` gives it (None: nothing to hold for it).
fn targets(
    d: &mut DrivingState,
    per_bend: impl Fn(&DrivingState, &RouteCurve) -> Option<f64>,
) -> Vec<(f64, f64)> {
    let position = d.trip.position_mi;
    let (limit, _) = d.trip.speed_limit_at(position);
    let mut out = vec![(limit, 0.0)];
    let bends: Vec<_> = d
        .trip
        .curves
        .iter()
        .filter(|c| !c.connector && c.end_mi >= position && c.start_mi - position < 1.0)
        .copied()
        .collect();
    for bend in bends {
        if let Some(mph) = per_bend(d, &bend) {
            out.push((mph, (bend.start_mi - position).max(0.0)));
        }
    }
    out
}

/// The spoken-number driver: the number the cab speaks for each bend it has
/// called. A bend the cab never called is one the driver was told nothing
/// about.
fn spoken_number(d: &DrivingState, bend: &RouteCurve) -> Option<f64> {
    d.trip
        .curve_called(bend)
        .then(|| d.spoken_advisory_mph(bend) as f64)
}

/// The daring driver: 10 over every sign, whatever the posted limit.
fn ten_over(_: &DrivingState, bend: &RouteCurve) -> Option<f64> {
    Some(bend.advisory_mph as f64 + 10.0)
}

/// The accelerator key, held or not.
fn throttle(harness: &mut PlaytestHarness, go: bool) {
    if go {
        harness.app.ctx.input.press(Key::Up, Mods::NONE);
    } else {
        harness.app.ctx.input.release(Key::Up, Mods::NONE);
    }
}

/// The last "slow to N" heard, in mph.
fn last_slow_to(heard: &[String]) -> Option<f64> {
    heard.iter().rev().find_map(|line| {
        let at = line.find("Slow to ")?;
        line[at + 8..]
            .split(|c: char| !c.is_ascii_digit())
            .next()?
            .parse::<f64>()
            .ok()
    })
}

fn drive(stretch: &Stretch, setup: Setup, load: Load) -> Run {
    let mut harness = start(stretch, setup, load);
    let end_mi = stretch.from_mi + STRETCH_MI;
    let mut run = Run {
        bends: harness.read_drive(|d| {
            d.trip
                .curves
                .iter()
                .filter(|c| !c.connector && c.start_mi >= stretch.from_mi && c.start_mi < end_mi)
                .count()
        }),
        min_psi: f64::MAX,
        low_air_psi: harness.read_drive(|d| d.truck().specs.air_low_warning_psi),
        ..Run::default()
    };
    let mut last_pedal = 0.0;
    let mut clock = 0.0f64;
    let mut braking = false;
    // The warning the spoken-number driver is obeying, until its bend ends.
    let mut slow_to: Option<(f64, f64)> = None;
    let mut warnings_obeyed = 0;
    let mut recent = std::collections::VecDeque::new();
    let mut lines_traced = 0;
    let mut foot = 0.0f64;
    let mut last_speed = 0.0f64;
    let mut warnings_seen = 0;
    let mut last_warned_at = f64::NEG_INFINITY;
    // `BEND_SWEEP_TRACE=Siskiyou/CurveAssist/Full` prints the pedals, the
    // bend and the roll share twice a second down that run.
    let trace = std::env::var("BEND_SWEEP_TRACE").is_ok_and(|v| {
        v.split_once('/').is_some_and(|(name, rest)| {
            stretch.name.contains(name) && rest == format!("{setup:?}/{load:?}")
        })
    });
    for _ in 0..(30 * 60 * 25) {
        if !harness.has_drive() {
            break;
        }
        // The deceleration the driver is braking for, m/s^2.
        let mut required = 0.0f64;
        let (brake, go) = match setup {
            Setup::AllAssists | Setup::Cruise => (false, false),
            Setup::CurveAssist => harness.with_drive(|d, _| {
                let (limit, _) = d.trip.speed_limit_at(d.trip.position_mi);
                let target = d
                    .curve_servo
                    .as_ref()
                    .map_or(limit, |servo| servo.target_mph.min(limit));
                let speed = d.truck().speed_mph();
                (false, d.truck().brake < 0.01 && speed < target - 1.0)
            }),
            Setup::SpokenNumber | Setup::Daring => {
                let heard = harness.transcript();
                let warnings = heard.iter().filter(|l| l.contains(TOO_FAST)).count();
                let fresh = (warnings > warnings_obeyed)
                    .then(|| last_slow_to(&heard))
                    .flatten();
                warnings_obeyed = warnings;
                harness.with_drive(|d, _| {
                    let position = d.trip.position_mi;
                    if let Some(mph) = fresh.filter(|_| setup == Setup::SpokenNumber) {
                        // Held to the end of the bend it is about: the one
                        // underfoot, else the next.
                        let until = d
                            .trip
                            .curve_at(position)
                            .or_else(|| d.trip.next_curve_within(1.0).map(|(_, c)| c))
                            .map_or(position + 0.3, |c| c.end_mi);
                        slow_to = Some((mph, until));
                    }
                    let mut holds = if setup == Setup::Daring {
                        targets(d, ten_over)
                    } else {
                        targets(d, spoken_number)
                    };
                    if let Some((mph, _)) = slow_to.filter(|(_, until)| position <= *until) {
                        holds.push((mph, 0.0));
                    }
                    let speed = d.truck().speed_mph();
                    // The obedient driver keeps a mile an hour under each
                    // number; the daring one rides a little over his own.
                    let (under, slack) = if setup == Setup::Daring {
                        (0.0, 1.5)
                    } else {
                        (1.0, 0.5)
                    };
                    let mut brake = false;
                    for &(target, road) in &holds {
                        let aim = target - under;
                        let (on, rate) = if road > 0.0 {
                            let demand = needed(speed, aim, road);
                            (demand > 1.2 || (braking && demand > 0.3), demand)
                        } else {
                            let over = speed > aim + slack || (braking && speed > aim - 1.0);
                            (over, if speed > aim { 1.0 } else { 0.0 })
                        };
                        if on {
                            brake = true;
                            required = required.max(rate);
                        }
                    }
                    let lowest = holds.iter().map(|h| h.0).fold(f64::MAX, f64::min);
                    (brake, !brake && speed < lowest - under - 1.5)
                })
            }
        };
        braking = brake;
        // The driver's brake is a steady foot, the way a player on a pedal
        // holds one application down a slope, not the key's full-on ramp
        // tapped on and off: that pumps the air and slops a tank's load, and
        // measures the script, not the bend. The foot starts at
        // `DRIVER_BRAKE`, presses harder while the truck slows slower than
        // the road left asks, and eases while it slows a good deal faster.
        // Set above what the frame's own release lets fall away, so the
        // physics feels the foot.
        throttle(&mut harness, go);
        if brake {
            let speed = harness.read_drive(|d| d.truck().speed_mph());
            let slowing = (last_speed - speed) / MPH_PER_MPS / DT;
            foot = if foot == 0.0 {
                DRIVER_BRAKE
            } else if slowing < required {
                (foot + 0.02).min(1.0)
            } else if slowing > required + 1.0 {
                (foot - 0.02).max(0.1)
            } else {
                foot
            };
            last_speed = speed;
            harness.with_drive(move |d, _| {
                let floor = foot + DT * 2.2 * 3.0;
                d.truck_mut().brake = d.truck().brake.max(floor);
            });
        } else {
            foot = 0.0;
        }
        if trace {
            let line = harness.with_drive(|d, _| {
                let position = d.trip.position_mi;
                let (limit, _) = d.trip.speed_limit_at(position);
                let bend = d.trip.curve_at(position).filter(|c| !c.connector);
                format!(
                    "t {clock:6.1} mi {position:7.3} v {:5.2} lim {limit:3.0} gr {:+5.1} cargo {:.2} bend {:?} share {:.3} live {:.3} brk {:.2} thr {:.2} servo {:?} cruise {:?}/{:?} jake {} keys {brake}/{go} slosh {:?} gear {} edge {:.2}",
                    d.truck().speed_mph(),
                    d.truck().grade * 100.0,
                    d.truck().cargo_damage_pct,
                    bend.map(|c| (c.advisory_mph, c.min_radius_ft, d.spoken_advisory_mph(&c))),
                    d.truck().roll_share(),
                    d.truck().live_roll_threshold_g(),
                    d.truck().brake,
                    d.truck().throttle,
                    d.curve_servo.as_ref().map(|s| (s.target_mph, s.start_mi, s.hold_to_mi)),
                    d.cruise_mph,
                    d.cruise_curve_mph,
                    d.truck().engine_brake_stage,
                    d.truck().liquid.as_ref().map(|l| (
                        (l.longitudinal.x * 100.0).round() / 100.0,
                        (l.longitudinal.v * 100.0).round() / 100.0,
                        (d.truck().surge_force_n() / 100.0).round() / 10.0,
                    )),
                    d.truck().transmission.gear,
                    d.lane.edge_excursion(),
                )
            });
            let every = if std::env::var("BEND_SWEEP_EVERY").is_ok() {
                1
            } else {
                15
            };
            if (clock * 30.0).round() as i64 % every == 0 {
                eprintln!("{line}");
            }
            recent.push_back(line);
            if recent.len() > 45 {
                recent.pop_front();
            }
        }
        // The truck as the frame found it, to price the bend it goes over in.
        let before = (setup == Setup::Daring).then(|| harness.read_drive(|d| d.truck().clone()));
        harness.advance_clock(DT);
        harness.with_drive(|d, ctx| d.update_frame(ctx, DT));
        harness.finish_timed_state();
        clock += DT;
        if !harness.has_drive() {
            break;
        }
        let (share, cargo, off_road, pedal, psi, position) = harness.read_drive(|d| {
            (
                d.truck().roll_share(),
                d.truck().cargo_damage_pct,
                d.lane.edge_excursion() >= OFF_ROAD,
                d.truck().brake,
                d.truck().air_pressure_psi(),
                d.trip.position_mi,
            )
        });
        let heard = harness.transcript();
        if trace {
            for line in &heard[lines_traced.min(heard.len())..] {
                eprintln!("SAID at t {clock:.1} mi {position:.3}: {line}");
            }
            lines_traced = heard.len();
        }
        let text = heard.join("\n");
        run.rolled = text.contains("rolled over") || text.contains("Rolled over");
        if let Some(mut truck) = before.filter(|_| run.rolled) {
            // What the bend it went over in asked of the truck at the speed
            // it came in with.
            run.share_at_roll = harness.read_drive(|d| {
                let bend = d.trip.curve_at(d.trip.position_mi).filter(|c| !c.connector);
                bend.map_or(0.0, |bend| {
                    truck.corner_radius_ft = bend.min_radius_ft as f64;
                    truck.corner_bank = d.bend_bank(&bend);
                    truck.roll_share()
                })
            });
        }
        run.max_share = run.max_share.max(share);
        // Freight moved on a frame the bend was asking past the warning share
        // is the bend's doing; otherwise it is the brakes'.
        if cargo > run.cargo_pct && (run.rolled || share > ROLL_WARN_SHARE) {
            run.bend_cargo_pct += cargo - run.cargo_pct;
        }
        run.cargo_pct = run.cargo_pct.max(cargo);
        run.off_road |= off_road;
        let warned = text.matches(TOO_FAST).count();
        if warned > warnings_seen {
            warnings_seen = warned;
            last_warned_at = clock;
        }
        if share > ROLL_WARN_SHARE && clock - last_warned_at > WARNED_WITHIN_S {
            run.silent_over_warn = run.silent_over_warn.max(share);
        }
        let warned_now = warned > 0;
        if warned_now && run.warnings == 0 {
            if trace {
                eprintln!("FIRST WARNING HEARD at t {clock:.1}");
            }
            run.warnings = 1;
            run.cost_before_warning = run.cost;
        }
        let costing = run.bend_cargo_pct > 0.0 || off_road || run.rolled;
        if trace && costing && !run.cost {
            eprintln!(
                "FIRST COST at t {clock:.1}: bend cargo {:.2}, off road {off_road}, rolled {}",
                run.bend_cargo_pct, run.rolled
            );
        }
        run.cost |= costing;
        // The air model charges by the pedal's rise, so the rises, summed in
        // full applications, are what the brakes cost the tanks
        // (`states_driving_air_brakes`).
        run.rises += (pedal - last_pedal).max(0.0);
        last_pedal = pedal;
        run.min_psi = run.min_psi.min(psi);
        if run.rolled || position > end_mi {
            if run.rolled && trace {
                eprintln!("---- the frames before the roll");
                for line in &recent {
                    eprintln!("{line}");
                }
            }
            break;
        }
    }
    throttle(&mut harness, false);
    run.heard = harness.transcript();
    run
}

fn faults(setup: Setup, run: &Run) -> Vec<String> {
    let mut faults = Vec::new();
    if run.bends == 0 {
        faults.push("no bends on the stretch".to_string());
    }
    let text = run.heard.join("\n");
    match setup {
        Setup::Daring => {
            if run.cost && (run.warnings == 0 || run.cost_before_warning) {
                faults.push("cost before any warning".to_string());
            }
            // Priced at the speed the frame began with: one frame's worth
            // of speed short of what put it over.
            if run.rolled && run.share_at_roll < 0.97 {
                faults.push(format!(
                    "rolled at {:.3} of the threshold",
                    run.share_at_roll
                ));
            }
        }
        _ => {
            if run.rolled {
                faults.push("rolled over".to_string());
            }
            if run.bend_cargo_pct > 0.0 {
                faults.push(format!(
                    "the bend moved the load {:.2} percent",
                    run.bend_cargo_pct
                ));
            }
            // A half-full tank's swing carried from one bend into the next
            // reached 0.98 on Lookout Pass with nothing said, and US-62's
            // esses put a bobtail off the pavement on the spoken numbers
            // (bend sweep, 2026-09-24).
            if run.off_road {
                faults.push("left the pavement".to_string());
            }
            if run.max_share > SHARE_CEILING {
                faults.push(format!(
                    "a bend asked {:.3} of the threshold",
                    run.max_share
                ));
            }
            if run.silent_over_warn > 0.0 {
                faults.push(format!(
                    "a bend asked {:.3} of the threshold with no warning heard",
                    run.silent_over_warn
                ));
            }
        }
    }
    if setup.assisted() {
        let warned = text.matches(TOO_FAST).count();
        if warned > 0 {
            faults.push(format!("heard \"too fast\" {warned} times"));
        }
        // One application per bend is the servo's own design ("holds a bend
        // on a downgrade on one application", ROADMAP 2026-09-18); pumping
        // makes many, and the tanks show it.
        if run.rises > run.bends as f64 {
            faults.push(format!(
                "the pedal rose {:.1} full applications over {} bends",
                run.rises, run.bends
            ));
        }
        if run.min_psi <= run.low_air_psi {
            faults.push(format!("the tanks fell to {:.0} psi", run.min_psi));
        }
    }
    faults
}

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "sweep: every assist setup and load over twelve bendy roads"
)]
fn test_the_assists_hold_every_mapped_bend_without_rolling_the_truck() {
    let only = std::env::var("BEND_SWEEP_ONLY").ok();
    let dump = std::env::var("BEND_SWEEP_DUMP").is_ok();
    let mut failures = Vec::new();
    let mut control_rolls = 0;
    let setups = [
        Setup::CurveAssist,
        Setup::AllAssists,
        Setup::SpokenNumber,
        Setup::Cruise,
    ];
    for stretch in &STRETCHES {
        if only.as_deref().is_some_and(|o| !stretch.name.contains(o)) {
            continue;
        }
        let mut cases: Vec<(Setup, Load)> = setups
            .iter()
            .flat_map(|s| LOADS.iter().map(move |l| (*s, *l)))
            .collect();
        cases.push((Setup::Daring, Load::Full));
        for (setup, load) in cases {
            let run = drive(stretch, setup, load);
            let found = faults(setup, &run);
            control_rolls += usize::from(setup == Setup::Daring && run.rolled);
            let row = format!(
                "{:28} {:12} {:7} bends {:2} share {:.3} warn {} rolled {} off {} cargo {:5.1} (bend {:5.1}) rises {:5.2} psi {:5.1}",
                stretch.name,
                format!("{setup:?}"),
                format!("{load:?}"),
                run.bends,
                run.max_share,
                run.heard.iter().filter(|l| l.contains(TOO_FAST)).count(),
                run.rolled,
                run.off_road,
                run.cargo_pct,
                run.bend_cargo_pct,
                run.rises,
                run.min_psi,
            );
            eprintln!("{row}");
            if dump || !found.is_empty() {
                eprintln!("==== {} x {setup:?} x {load:?} {found:?}", stretch.name);
                if dump {
                    eprintln!("{}", run.heard.join("\n"));
                }
            }
            if !found.is_empty() {
                let tail: Vec<&String> = run.heard.iter().rev().take(12).rev().collect();
                failures.push(format!(
                    "{} x {setup:?} x {load:?}: {found:?}\n  last heard: {tail:#?}",
                    stretch.name
                ));
            }
        }
    }
    assert!(
        only.is_some() || control_rolls > 0,
        "the daring driver never rolled: the sweep cannot see a roll"
    );
    assert!(
        failures.is_empty(),
        "{} faults\n{}",
        failures.len(),
        failures.join("\n\n")
    );
}
