//! Where the engine brake actually comes on, measured rather than argued.
//!
//! Owner report, 2026-08-24, two parts: "on uphill ascents the truck should
//! gain speed instead of using the engine brakes", and "edge cases where the
//! jake still activates on flat road with cruise on -- unless that's real
//! behaviour, in which case leave it".
//!
//! And a third report, 2026-08-24: "the jake activates on every single
//! descent it seems, even shallow descent like 1-3 percent."
//!
//! A retarder is a device for holding a loaded truck BACK. On a climb the
//! truck should be building speed, or at worst holding what it has, so a
//! retarder there is a controller asking for the opposite of what the road
//! needs. Two rules answer that between them, and until the shallow-descent
//! report they were one:
//!
//! * `DrivingState::on_downgrade` -- the two percent line the spoken G readout
//!   and the town ordinance exemption draw -- says whether the road is going
//!   downhill, which is what a retarder already up is HELD by.
//! * `DrivingState::retarder_warranted` says whether the service brakes could
//!   hold this hill on their own, which is what an assist may RAISE the
//!   retarder for. Derived, not chosen: see its own doc comment and
//!   `states_driving_jake_line`, which measures it against real drum heat.
//!
//! This file drives real frames over real baked road and COUNTS, so that "does
//! the jake come on where it should not" has an answer with numbers under it
//! instead of a bench built to match a memory. The grade-band table is the one
//! to read against the shallow-descent report: before the split it showed 33
//! seconds of retarder on two to three percent and 137 on three to four; after
//! it, zero in both.
//!
//! Every road here is seeded and its weather pinned: an unseeded delivery
//! draws its own route and its own sky, and letting that draw decide which
//! shape got measured is how three separate faults hid this week.
//!
//! Run `cargo test -p freight-fate --test it jake_sweep -- --nocapture` to
//! read the tables; the assertion at the bottom is what the sweep pins.

use std::collections::BTreeMap;

use ff_core::data::curves::RouteCurve;
use ff_core::data::world_models::{CorridorDetail, GradeSegment, Leg, Route, SpeedLimitSample};
use ff_core::sim::trip::{Trip, TripOptions};
use ff_core::sim::trip_models::Zone;
use ff_core::sim::weather::{WeatherKind, WeatherSystem};

use freight_fate::states::driving::DrivingState;

use crate::transcript_cruise_support::{
    frame, quiet, release_keys, start_drive, BENCH_MILES, DT, MPS_PER_MPH, START_MI,
};

/// The shipped default. The owner drives compressed time, so the sweep does
/// too -- a look-ahead sized in real seconds covers a different stretch of
/// road at 10x than it does at 1x, and the retarder decisions ride on those
/// look-aheads.
const SWEEP_TIME_SCALE: f64 = 10.0;

/// The line the game draws between level road and a grade at all
/// (`JAKE_ZONE_EXEMPT_GRADE_PCT`, `GRADE_WARN_CLEAR_PCT`, and the G readout).
/// This classifies the ROAD, and it is deliberately still the shallow line:
/// the strict question the assertions ask is "did a retarder stay up somewhere
/// that is not going downhill at all", and the shallow line is the strictest
/// way to ask it.
const GRADE_PCT: f64 = 2.0;

// -- what the sweep records ------------------------------------------------------------

/// Which controller was holding the retarder up on this frame.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
enum Cause {
    /// Adaptive cruise's own descent staging (`hold_cruise_from_above`).
    Cruise,
    /// The curve assist reaching for the grade under a bend (`update_lane`).
    CurveAssist,
    /// AMT retarder management, armed by the driver with J.
    AutoMode,
    /// The driver's own stalk and nothing else.
    Driver,
}

impl Cause {
    fn label(self) -> &'static str {
        match self {
            Cause::Cruise => "adaptive cruise",
            Cause::CurveAssist => "curve assist",
            Cause::AutoMode => "auto mode (J)",
            Cause::Driver => "driver stalk",
        }
    }
}

/// The road under the wheels at the moment the retarder was up.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
enum Road {
    Descent,
    Level,
    Climb,
}

impl Road {
    fn of(grade_pct: f64) -> Road {
        if grade_pct <= -GRADE_PCT {
            Road::Descent
        } else if grade_pct >= GRADE_PCT {
            Road::Climb
        } else {
            Road::Level
        }
    }

    fn label(self) -> &'static str {
        match self {
            Road::Descent => "descent",
            Road::Level => "level",
            Road::Climb => "climb",
        }
    }
}

/// The grade bands the report is read in -- the owner's own, so the before
/// and after tables answer the report in the words it was made in.
#[derive(Clone, Copy, PartialEq, Eq, Debug, PartialOrd, Ord)]
enum Band {
    Down0to1,
    Down1to2,
    Down2to3,
    Down3to4,
    Down4to6,
    Down6plus,
    LevelOrClimb,
}

impl Band {
    const ALL: [Band; 7] = [
        Band::Down0to1,
        Band::Down1to2,
        Band::Down2to3,
        Band::Down3to4,
        Band::Down4to6,
        Band::Down6plus,
        Band::LevelOrClimb,
    ];

    fn of(grade_pct: f64) -> Band {
        let down = -grade_pct;
        if down <= 0.0 {
            return Band::LevelOrClimb;
        }
        if down < 1.0 {
            Band::Down0to1
        } else if down < 2.0 {
            Band::Down1to2
        } else if down < 3.0 {
            Band::Down2to3
        } else if down < 4.0 {
            Band::Down3to4
        } else if down < 6.0 {
            Band::Down4to6
        } else {
            Band::Down6plus
        }
    }

    fn label(self) -> &'static str {
        match self {
            Band::Down0to1 => "down 0-1%",
            Band::Down1to2 => "down 1-2%",
            Band::Down2to3 => "down 2-3%",
            Band::Down3to4 => "down 3-4%",
            Band::Down4to6 => "down 4-6%",
            Band::Down6plus => "down 6%+",
            Band::LevelOrClimb => "level or climb",
        }
    }
}

/// The sustained-length buckets, in miles of descent end to end.
const LENGTH_BUCKETS: [(f64, f64, &str); 6] = [
    (0.0, 0.5, "under 0.5 mi"),
    (0.5, 1.0, "0.5 to 1 mi"),
    (1.0, 2.0, "1 to 2 mi"),
    (2.0, 5.0, "2 to 5 mi"),
    (5.0, 10.0, "5 to 10 mi"),
    (10.0, f64::INFINITY, "10 mi and up"),
];

fn length_bucket(miles: f64) -> &'static str {
    for (low, high, label) in LENGTH_BUCKETS {
        if miles >= low && miles < high {
            return label;
        }
    }
    "under 0.5 mi"
}

/// One frame with the retarder up, and everything that could have asked for it.
#[derive(Clone, Debug)]
struct Sample {
    road_name: &'static str,
    /// Which frame of this road it was, so the release LATENCY at a grade
    /// boundary can be told apart from a retarder that simply stayed up.
    index: usize,
    mile: f64,
    grade_pct: f64,
    speed_mph: f64,
    posted_mph: f64,
    stage: i32,
    /// A raised stage is not a bark. The retarder only retards off the fuel,
    /// coupled, and turning, which is the same gate the growl loop and the
    /// noise ordinance both read -- so this, not the stage, is what a driver
    /// hears and what the road actually feels.
    barking: bool,
    throttle: f64,
    cause: Cause,
    /// What the controller holding the pedal says it is holding, and why.
    holding: String,
    lead_mph: Option<f64>,
    curve_cap: Option<f64>,
    zone: Option<String>,
    /// How long the descent the truck is on runs, end to end, in miles.
    /// Zero when the road under the wheels is not a descent at all. This is
    /// the other half of the question "should the retarder be here": a three
    /// percent grade held for ten miles is a different road from three percent
    /// for a furlong, even though the slope reads the same.
    descent_len_mi: f64,
}

impl Sample {
    fn road(&self) -> Road {
        Road::of(self.grade_pct)
    }

    /// Which slope bucket this frame belongs to, in the owner's own bands.
    fn band(&self) -> Band {
        Band::of(self.grade_pct)
    }

    /// The one-line "name what asked for it" the report is made of.
    fn cause_line(&self) -> String {
        let lead = match self.lead_mph {
            Some(mph) => format!("lead {mph:.0}"),
            None => "no lead".to_string(),
        };
        let cap = match self.curve_cap {
            Some(mph) => format!("bend cap {mph:.0}"),
            None => "no bend cap".to_string(),
        };
        let zone = self.zone.clone().unwrap_or_else(|| "no zone".to_string());
        let bark = if self.barking {
            "barking"
        } else {
            "stage up, silent"
        };
        format!(
            "{} | {bark} | {} | {:.1} mph, posted {:.0}, throttle {:.2} | {lead} | {cap} | {zone}",
            self.cause.label(),
            self.holding,
            self.speed_mph,
            self.posted_mph,
            self.throttle,
        )
    }
}

/// Every retarder frame of one road, plus how many frames the road ran.
struct Trace {
    name: &'static str,
    frames: usize,
    /// (mile, mph) once per frame: the drive the numbers above came off.
    track: Vec<(f64, f64)>,
    /// The grade under the wheels on every frame, retarder or not, so the
    /// band table can say how much road was DRIVEN in each band and a zero
    /// in a band can be told apart from a band nothing ever drove.
    track_grade: Vec<f64>,
    /// The road class of EVERY frame, retarder or not: a retarder frame is
    /// only fairly called "off the grade" once the road has been off the
    /// grade for longer than one controller pass.
    road_class: Vec<Road>,
    retarding: Vec<Sample>,
    rises: Vec<Sample>,
}

impl Trace {
    /// Frames since the road under the wheels was last a downgrade.
    ///
    /// The controllers decide on the mile they are at and the truck then
    /// moves, so the frame a grade ends is read back one pass late by
    /// construction. Anything past [`RELEASE_GRACE_FRAMES`] is a retarder
    /// that stayed up, not a retarder being handed back.
    fn since_descent(&self, index: usize) -> usize {
        let mut back = 0;
        while back <= index {
            if self.road_class[index - back] == Road::Descent {
                return back;
            }
            back += 1;
        }
        index + 1
    }
}

/// How long a controller gets to notice the grade ended: one frame for the
/// pass it already made on the old mile, and a couple more for the pedal to
/// come off. Anything longer is the retarder holding a road that is not there.
const RELEASE_GRACE_FRAMES: usize = 4;

// -- the roads -------------------------------------------------------------------------

/// A grade profile in `(start_mi, end_mi, percent)`.
type Grades = Vec<(f64, f64, f64)>;

/// Bench road with a grade profile, a posted number, bends and zones -- the
/// sweep's own `bench_road_segments`, which takes neither of the last two.
fn sweep_road(
    drive: &mut DrivingState,
    limit_mph: f64,
    grades: &Grades,
    curves: &[RouteCurve],
    zones: &[Zone],
) {
    let city = drive.trip.route.cities[0].clone();
    let detail = CorridorDetail {
        speed_limits: vec![SpeedLimitSample {
            at_mi: 0.0,
            mph: Some(limit_mph),
            source: "jake sweep".to_string(),
            hgv: false,
        }],
        grade_segments: grades
            .iter()
            .map(|(start_mi, end_mi, pct)| {
                GradeSegment::new(
                    *start_mi,
                    *end_mi,
                    *pct,
                    if pct.abs() >= 3.0 { "mountain" } else { "flat" },
                    "jake sweep",
                )
            })
            .collect(),
        ..Default::default()
    };
    let leg = Leg::new(&city, &city, BENCH_MILES, "I 90", "flat", Vec::new()).with_detail(detail);
    let route = Route::from_legs(vec![city.clone(), city], vec![leg]);
    let truck = drive.trip.truck.clone();
    let mut weather = WeatherSystem::new("heartland", Some(3), None, None, true);
    weather.current = WeatherKind::Clear;
    let mut trip = Trip::new(
        route,
        truck,
        weather,
        TripOptions {
            seed: Some(3),
            time_scale: SWEEP_TIME_SCALE,
            ..Default::default()
        },
    );
    quiet(&mut trip);
    trip.zones = zones.to_vec();
    trip.curves = curves.to_vec();
    trip.set_patrols(Vec::new());
    drive.trip = trip;
    drive.reset_turn_state_for_trip();
    drive.destination_exit_taken = true;
    drive.trip.position_mi = START_MI;
}

/// A bend at a mile, with an advisory the truck will be well over.
fn bend(start_mi: f64, span_mi: f64, advisory: i64) -> RouteCurve {
    RouteCurve {
        start_mi,
        apex_mi: start_mi + span_mi * 0.5,
        end_mi: start_mi + span_mi,
        direction: 'L',
        advisory_mph: advisory,
        min_radius_ft: 900,
        deflection_deg: 40.0,
        connector: false,
    }
}

/// One seeded, weather-pinned road, driven with cruise engaged and every
/// assist in its shipped default state.
struct Bench {
    name: &'static str,
    set_mph: f64,
    limit_mph: f64,
    grades: Grades,
    curves: Vec<RouteCurve>,
    zones: Vec<Zone>,
    weather: WeatherKind,
    /// A slower vehicle this far ahead, at this speed.
    lead: Option<(f64, f64)>,
    /// Arm the AMT retarder manager the way pressing J does.
    auto_mode: bool,
    /// The speed the driver was doing when they armed it, if that is not the
    /// speed cruise then goes on to hold.
    auto_mode_at: Option<f64>,
    /// Off for the roads where a driver is simply coasting, so the curve
    /// assist is the only thing that can reach for the retarder.
    cruise: bool,
    seconds: f64,
}

impl Bench {
    fn new(name: &'static str, set_mph: f64, grades: Grades) -> Bench {
        Bench {
            name,
            set_mph,
            limit_mph: 65.0,
            grades,
            curves: Vec::new(),
            zones: Vec::new(),
            weather: WeatherKind::Clear,
            lead: None,
            auto_mode: false,
            auto_mode_at: None,
            cruise: true,
            seconds: 90.0,
        }
    }
}

/// Flat road either side of a feature that starts a mile ahead of the truck.
fn profile(feature: &[(f64, f64)]) -> Grades {
    let mut out: Grades = vec![(0.0, START_MI + 1.0, 0.0)];
    let mut at = START_MI + 1.0;
    for (miles, pct) in feature {
        out.push((at, at + miles, *pct));
        at += miles;
    }
    out.push((at, BENCH_MILES, 0.0));
    out
}

/// The sweep's roads: every shape the owner's two reports could live in.
fn roads() -> Vec<Bench> {
    let mut out = Vec::new();

    // 1. A plain sustained descent -- the retarder's own job, the control.
    out.push(Bench::new("long descent", 62.0, profile(&[(8.0, -6.0)])));

    // 2. A sag: down into up, which is the shape a valley floor makes.
    out.push(Bench::new("sag", 62.0, profile(&[(4.0, -6.0), (6.0, 5.0)])));

    // 3. The same sag with a bend across its bottom, so one curve-assist
    //    episode spans the descent AND the climb out of it.
    let mut sag_bend = Bench::new(
        "sag under a bend",
        62.0,
        profile(&[(4.0, -6.0), (6.0, 5.0)]),
    );
    sag_bend.curves = vec![bend(START_MI + 3.0, 4.0, 45)];
    out.push(sag_bend);

    // 4. A crest: up over the top and down the far side.
    out.push(Bench::new(
        "crest",
        62.0,
        profile(&[(5.0, 4.0), (6.0, -4.0)]),
    ));

    // 5. Rolling country: every dip crosses the descent trigger.
    let mut rollers: Grades = vec![(0.0, START_MI + 1.0, 0.0)];
    let mut at = START_MI + 1.0;
    let mut down = true;
    while at < START_MI + 25.0 {
        rollers.push((at, at + 0.6, if down { -4.0 } else { 4.0 }));
        at += 0.6;
        down = !down;
    }
    rollers.push((at, BENCH_MILES, 0.0));
    out.push(Bench::new("rolling country", 62.0, rollers));

    // 6. A mountain descent strung with bends.
    let mut mountain = Bench::new(
        "mountain descent with bends",
        62.0,
        profile(&[(14.0, -6.0)]),
    );
    mountain.curves = (0..6)
        .map(|i| bend(START_MI + 2.0 + i as f64 * 2.0, 0.8, 40))
        .collect();
    out.push(mountain);

    // 7. Flat, closing on a slower vehicle: the drums' work by doctrine.
    let mut traffic = Bench::new("flat, closing on a lead", 62.0, profile(&[]));
    traffic.lead = Some((0.4, 45.0));
    out.push(traffic);

    // 8. Flat, the posted number drops: a target speed, the drums' work.
    let mut drop = Bench::new("flat, posted limit drops", 62.0, profile(&[]));
    drop.zones = vec![Zone::new(
        START_MI + 2.0,
        START_MI + 40.0,
        45.0,
        "construction",
    )];
    out.push(drop);

    // 9. Flat, a bend: a target speed, the drums' work.
    let mut level_bend = Bench::new("flat, a bend", 62.0, profile(&[]));
    level_bend.curves = vec![bend(START_MI + 2.0, 3.0, 40)];
    out.push(level_bend);

    // 10. Flat, a storm: the weather ease is the drums' work on a slick road.
    let mut storm = Bench::new("flat, thunderstorm ease", 70.0, profile(&[]));
    storm.weather = WeatherKind::Thunderstorm;
    out.push(storm);

    // 11. A straight climb, entered hot: the hill eats the speed.
    let mut climb = Bench::new("climb entered hot", 55.0, profile(&[(20.0, 4.0)]));
    climb.set_mph = 55.0;
    out.push(climb);

    // 12. Flat with the driver's own AMT retarder manager armed.
    let mut armed = Bench::new("flat, auto mode armed", 62.0, profile(&[]));
    armed.auto_mode = true;
    out.push(armed);

    // 13. A descent that ends in a climb, auto mode armed over cruise.
    let mut armed_grade = Bench::new(
        "descent into climb, auto mode armed",
        62.0,
        profile(&[(4.0, -6.0), (8.0, 4.0)]),
    );
    armed_grade.auto_mode = true;
    out.push(armed_grade);

    // 14. The shape the curve assist's release rule never sees: ONE bend that
    //     starts on the way down and finishes on the way up, with an advisory
    //     low enough that the truck is still over it at the top. The assist's
    //     episode spans both roads, so nothing ends it at the bottom.
    let mut dip_bend = Bench::new(
        "tight bend from the dip into the climb",
        62.0,
        profile(&[(2.0, -6.0), (8.0, 4.0)]),
    );
    dip_bend.curves = vec![bend(START_MI + 2.0, 4.0, 30)];
    out.push(dip_bend);

    // 15. A descent that runs out straight into a bend on the flat: cruise is
    //     holding the hill on the retarder and then has a target speed to
    //     arrive at, which is the drums' work.
    let mut grade_then_bend = Bench::new(
        "descent straight into a level bend",
        62.0,
        profile(&[(4.0, -6.0)]),
    );
    grade_then_bend.curves = vec![bend(START_MI + 5.0, 4.0, 40)];
    out.push(grade_then_bend);

    // 16. A driver coasting a short pitch into a bend that flattens out, with
    //     no cruise engaged at all -- the one shape where the CURVE ASSIST is
    //     the only thing that can own the retarder, and where its episode
    //     outlives the grade that bought it.
    let mut coasting = Bench::new(
        "coasting a bend off a short pitch, no cruise",
        62.0,
        profile(&[(0.4, -6.0)]),
    );
    coasting.cruise = false;
    // A hard bend runs the clock at 1x (severe-curve decompression), so the
    // pitch has to be short enough that the truck reaches the flat half of
    // the bend inside the run.
    coasting.curves = vec![bend(START_MI + 1.1, 6.0, 30)];
    out.push(coasting);

    // 17. The same shape, but the road CLIMBS out of the pitch: the retarder
    //     the bend inherited is now pointed at a hill.
    let mut coasting_climb = Bench::new(
        "coasting a bend off a pitch into a climb, no cruise",
        62.0,
        profile(&[(0.4, -6.0), (8.0, 3.0)]),
    );
    coasting_climb.cruise = false;
    coasting_climb.curves = vec![bend(START_MI + 1.1, 6.0, 30)];
    out.push(coasting_climb);

    // 18. Auto mode armed at one number, cruise then set to a higher one:
    //     two controllers with two answers on level road.
    let mut two_numbers = Bench::new(
        "flat, auto mode armed below the cruise set",
        62.0,
        profile(&[]),
    );
    two_numbers.auto_mode = true;
    two_numbers.auto_mode_at = Some(45.0);
    out.push(two_numbers);

    // 19-23. THE ROADS THE OWNER'S REPORT LIVES ON, and which this sweep did
    //        not have until 2026-08-24: shallow sustained descents. Everything
    //        above is four percent or steeper, so a sweep of them can say
    //        nothing at all about "the jake activates on every single descent
    //        it seems, even shallow descent like 1-3 percent". One road per
    //        band, each long enough that the drums would reach their settling
    //        temperature if they were ever going to.
    for (name, pct) in [
        ("sustained 1 percent descent", -1.0),
        ("sustained 2 percent descent", -2.0),
        ("sustained 3 percent descent", -3.0),
        ("sustained 5 percent descent", -5.0),
        ("sustained 7 percent descent", -7.0),
    ] {
        out.push(Bench::new(name, 62.0, profile(&[(20.0, pct)])));
    }

    // 24. The long shallow haul: three percent for ten miles is the case the
    //     energy argument is usually made with -- more total heat into the
    //     drums than a short steep pitch -- and the drums still hold it,
    //     because they settle below fade and stay there however long it runs.
    out.push(Bench::new(
        "ten miles of three percent",
        62.0,
        profile(&[(10.0, -3.0)]),
    ));

    // 25. A quarter-mile of seven percent: steep enough that the drums could
    //     never hold it forever, short enough that they never get near fade.
    //     The sustained-run filter is what keeps this one quiet.
    out.push(Bench::new(
        "a quarter mile of seven percent",
        62.0,
        profile(&[(0.25, -7.0)]),
    ));

    out
}

// -- driving one road ------------------------------------------------------------------

fn drive_road(road: &Bench) -> Trace {
    let mut harness = start_drive(road.name);
    harness.app.ctx.settings.time_scale = SWEEP_TIME_SCALE;
    harness.app.ctx.settings.automatic_transmission = true;
    // Every assist in its shipped default: this is the truck the owner drives.
    harness.app.ctx.settings.curve_speed_assist = true;
    harness.app.ctx.settings.descent_speed_control = "realistic".to_string();
    harness.app.ctx.settings.speed_keeper = true;
    release_keys(&mut harness);

    let grades = road.grades.clone();
    let curves = road.curves.clone();
    let zones = road.zones.clone();
    let limit = road.limit_mph;
    let set_mph = road.set_mph;
    let weather = road.weather;
    harness.with_drive(move |d, _| {
        sweep_road(d, limit, &grades, &curves, &zones);
        d.weather_mut().forced = Some(weather);
        d.weather_mut().current = weather;
        d.truck_mut().transmission.automatic = true;
        d.truck_mut().cargo_kg = 18_000.0;
        d.truck_mut().start_engine();
        d.truck_mut().set_air_ready(false);
        d.truck_mut().velocity_mps = set_mph * MPS_PER_MPH;
        d.truck_mut().transmission.gear = d.truck().transmission.num_gears();
    });
    if let Some((gap_mi, lead_mph)) = road.lead {
        harness.add_npc_traffic_ahead("cruiser", gap_mi, lead_mph, 0);
        harness.with_drive(|d, _| d.trip.traffic_manager.rolling_bubble = false);
    }
    if road.cruise {
        harness.with_drive(move |d, ctx| d.engage_cruise(ctx, set_mph, false));
    }
    if road.auto_mode {
        // Exactly what pressing J on an automatic box does
        // (`driving_controls::vehicle`), without depending on key routing.
        let armed_at = road.auto_mode_at;
        harness.with_drive(move |d, _| {
            d.auto_jake = true;
            d.auto_jake_hold_mph =
                Some(5.0f64.max(armed_at.unwrap_or_else(|| d.truck().speed_mph())));
            d.auto_jake_cooldown_s = 0.0;
            d.truck_mut().engine_brake_stage = 1;
        });
    }

    let total = (road.seconds / DT) as usize;
    let mut retarding = Vec::new();
    let mut rises = Vec::new();
    let mut road_class = Vec::with_capacity(total);
    let mut track = Vec::with_capacity(total);
    let mut track_grade = Vec::with_capacity(total);
    let mut previous_stage = harness.read_drive(|d| d.truck().engine_brake_stage);
    for index in 0..total {
        frame(&mut harness, DT);
        let (mile, mph, grade_pct) = harness.read_drive(|d| {
            (
                d.trip.position_mi,
                d.truck().speed_mph(),
                d.trip.grade_at(d.trip.position_mi) * 100.0,
            )
        });
        track.push((mile, mph));
        track_grade.push(grade_pct);
        road_class.push(Road::of(grade_pct));
        match harness.with_drive(|d, _| sample_frame(d, road.name, index)) {
            Some(sample) => {
                if sample.stage > previous_stage {
                    rises.push(sample.clone());
                }
                previous_stage = sample.stage;
                retarding.push(sample);
            }
            None => previous_stage = 0,
        }
    }
    Trace {
        name: road.name,
        frames: total,
        track,
        track_grade,
        road_class,
        retarding,
        rises,
    }
}

/// The retarder's state this frame, and everything that could have asked for it.
fn sample_frame(d: &mut DrivingState, road_name: &'static str, index: usize) -> Option<Sample> {
    let stage = d.truck().engine_brake_stage;
    if stage <= 0 {
        return None;
    }
    let cause = if d.cruise_jake_stage > 0 {
        Cause::Cruise
    } else if d.curve_assist_jake {
        Cause::CurveAssist
    } else if d.auto_jake {
        Cause::AutoMode
    } else {
        Cause::Driver
    };
    let mile = d.trip.position_mi;
    let holding = if let Some(held) = d.cruise_held_mph {
        let reason = if d.cruise_held_reason.is_empty() {
            "no stated reason".to_string()
        } else {
            d.cruise_held_reason.clone()
        };
        format!("cruise holding {held:.0} {reason}")
    } else if let Some(keeper) = d.keeper_mph {
        format!("keeper holding {keeper:.0} in the {} zone", d.keeper_zone)
    } else if let Some(hold) = d.auto_jake_hold_mph {
        format!("auto mode holding {hold:.0}")
    } else {
        "nothing engaged".to_string()
    };
    let lead_mph = d.trip.traffic_context().map(|c| c.lead.speed_mph);
    let zone = d
        .trip
        .zones
        .iter()
        .find(|z| z.start_mi <= mile && mile <= z.end_mi)
        .map(|z| z.reason.clone());
    Some(Sample {
        road_name,
        index,
        mile,
        grade_pct: d.trip.grade_at(mile) * 100.0,
        speed_mph: d.truck().speed_mph(),
        posted_mph: d.trip.speed_limit_at(mile).0,
        stage,
        barking: d.truck().jake_retard_torque_nm() > 0.0,
        throttle: d.truck().throttle,
        cause,
        holding,
        lead_mph,
        curve_cap: d.cruise_curve_mph,
        zone,
        descent_len_mi: descent_length_mi(d, mile),
    })
}

/// How far the descent under `mile` runs, backwards and forwards, at the
/// stride the baked grade segments use and the same "still the same grade"
/// line the game's own `grade_run_mi` draws.
fn descent_length_mi(d: &DrivingState, mile: f64) -> f64 {
    const STEP_MI: f64 = 0.25;
    const CAP_MI: f64 = 40.0;
    if d.trip.grade_at(mile) * 100.0 > -GRADE_PCT {
        return 0.0;
    }
    let total = d.trip.total_miles();
    let mut length = STEP_MI;
    let mut probe = mile;
    while length < CAP_MI {
        probe += STEP_MI;
        if probe >= total || d.trip.grade_at(probe) * 100.0 > -GRADE_PCT {
            break;
        }
        length += STEP_MI;
    }
    let mut probe = mile;
    while length < CAP_MI {
        probe -= STEP_MI;
        if probe <= 0.0 || d.trip.grade_at(probe) * 100.0 > -GRADE_PCT {
            break;
        }
        length += STEP_MI;
    }
    length
}

// -- the report ------------------------------------------------------------------------

fn print_tables(traces: &[Trace]) {
    println!(
        "
== the drives the numbers came off =="
    );
    println!(
        "{:<52} {:>9} {:>9} {:>7} {:>7} {:>8} {:>7} {:>7}",
        "road", "from mi", "to mi", "min mph", "max mph", "descent", "level", "climb"
    );
    for trace in traces {
        let miles: Vec<f64> = trace.track.iter().map(|(mi, _)| *mi).collect();
        let speeds: Vec<f64> = trace.track.iter().map(|(_, mph)| *mph).collect();
        let count = |want: Road| trace.road_class.iter().filter(|r| **r == want).count();
        println!(
            "{:<52} {:>9.2} {:>9.2} {:>7.1} {:>7.1} {:>8} {:>7} {:>7}",
            trace.name,
            miles.first().copied().unwrap_or(0.0),
            miles.last().copied().unwrap_or(0.0),
            speeds.iter().cloned().fold(f64::INFINITY, f64::min),
            speeds.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
            count(Road::Descent),
            count(Road::Level),
            count(Road::Climb),
        );
    }
    println!(
        "
== retarder frames by cause and by the road under the wheels =="
    );
    println!("(stage up / genuinely barking -- a stage held under throttle retards nothing");
    println!(" and makes no noise, which is the gate the growl loop reads)");
    println!(
        "{:<38} {:>16} {:>13} {:>13} {:>13}",
        "road", "cause", "descent", "level", "climb"
    );
    let mut totals: BTreeMap<(Cause, Road), (usize, usize)> = BTreeMap::new();
    for trace in traces {
        let mut by_cause: BTreeMap<Cause, [(usize, usize); 3]> = BTreeMap::new();
        for sample in &trace.retarding {
            let slot = by_cause.entry(sample.cause).or_insert([(0, 0); 3]);
            let index = match sample.road() {
                Road::Descent => 0,
                Road::Level => 1,
                Road::Climb => 2,
            };
            slot[index].0 += 1;
            let total = totals
                .entry((sample.cause, sample.road()))
                .or_insert((0, 0));
            total.0 += 1;
            if sample.barking {
                slot[index].1 += 1;
                total.1 += 1;
            }
        }
        if by_cause.is_empty() {
            println!(
                "{:<38} {:>16} {:>13} {:>13} {:>13}",
                trace.name, "-- none --", "0/0", "0/0", "0/0"
            );
        }
        for (cause, counts) in by_cause {
            let cell = |c: (usize, usize)| format!("{}/{}", c.0, c.1);
            println!(
                "{:<38} {:>16} {:>13} {:>13} {:>13}",
                trace.name,
                cause.label(),
                cell(counts[0]),
                cell(counts[1]),
                cell(counts[2])
            );
        }
    }
    let all_frames: usize = traces.iter().map(|t| t.frames).sum();
    println!(
        "
== whole sweep: retarder time per cause =="
    );
    println!(
        "{all_frames} frames driven ({:.0} seconds of road)",
        all_frames as f64 * DT
    );
    for ((cause, road), (up, barking)) in &totals {
        println!(
            "{:<16} on {:<8} stage up {:>6.2} s ({:>5.2}%), barking {:>6.2} s ({:>5.2}%)",
            cause.label(),
            road.label(),
            *up as f64 * DT,
            100.0 * *up as f64 / all_frames as f64,
            *barking as f64 * DT,
            100.0 * *barking as f64 / all_frames as f64,
        );
    }

    println!(
        "
== RETARDER SECONDS BY GRADE BAND (the owner's bands) =="
    );
    println!("(genuinely barking, not merely staged; every cause, every road)");
    println!(
        "{:<18} {:>12} {:>12} {:>12} {:>12} {:>12}",
        "grade band", "cruise s", "curve s", "auto s", "driver s", "TOTAL s"
    );
    let mut by_band: BTreeMap<(Band, Cause), usize> = BTreeMap::new();
    let mut band_frames: BTreeMap<Band, usize> = BTreeMap::new();
    for trace in traces {
        for (index, class) in trace.road_class.iter().enumerate() {
            let _ = class;
            let grade = trace.track_grade[index];
            *band_frames.entry(Band::of(grade)).or_insert(0) += 1;
        }
        for sample in &trace.retarding {
            if !sample.barking {
                continue;
            }
            *by_band.entry((sample.band(), sample.cause)).or_insert(0) += 1;
        }
    }
    for band in Band::ALL {
        let get = |cause: Cause| *by_band.get(&(band, cause)).unwrap_or(&0) as f64 * DT;
        let cruise = get(Cause::Cruise);
        let curve = get(Cause::CurveAssist);
        let auto = get(Cause::AutoMode);
        let driver = get(Cause::Driver);
        println!(
            "{:<18} {:>12.2} {:>12.2} {:>12.2} {:>12.2} {:>12.2}",
            band.label(),
            cruise,
            curve,
            auto,
            driver,
            cruise + curve + auto + driver,
        );
    }
    println!(
        "
   for scale, seconds of road DRIVEN in each band:"
    );
    for band in Band::ALL {
        println!(
            "   {:<18} {:>10.2} s",
            band.label(),
            *band_frames.get(&band).unwrap_or(&0) as f64 * DT
        );
    }

    println!(
        "
== RETARDER SECONDS BY SUSTAINED DESCENT LENGTH =="
    );
    println!("(how long the hill the truck was on runs, end to end)");
    println!(
        "{:<18} {:>12} {:>12} {:>12} {:>12} {:>12}",
        "descent length", "cruise s", "curve s", "auto s", "driver s", "TOTAL s"
    );
    let mut by_length: BTreeMap<(&str, Cause), usize> = BTreeMap::new();
    for trace in traces {
        for sample in &trace.retarding {
            if !sample.barking {
                continue;
            }
            let bucket = length_bucket(sample.descent_len_mi);
            *by_length.entry((bucket, sample.cause)).or_insert(0) += 1;
        }
    }
    for (_, _, label) in LENGTH_BUCKETS {
        let get = |cause: Cause| *by_length.get(&(label, cause)).unwrap_or(&0) as f64 * DT;
        let cruise = get(Cause::Cruise);
        let curve = get(Cause::CurveAssist);
        let auto = get(Cause::AutoMode);
        let driver = get(Cause::Driver);
        println!(
            "{:<18} {:>12.2} {:>12.2} {:>12.2} {:>12.2} {:>12.2}",
            label,
            cruise,
            curve,
            auto,
            driver,
            cruise + curve + auto + driver,
        );
    }

    println!(
        "
== every stage rise, and what asked for it =="
    );
    for trace in traces {
        for rise in &trace.rises {
            println!(
                "{:<38} mile {:>7.2} grade {:>+6.2}% [{}] stage {} -- {}",
                rise.road_name,
                rise.mile,
                rise.grade_pct,
                rise.road().label(),
                rise.stage,
                rise.cause_line()
            );
        }
    }
    println!();
}

/// Every frame where something retarded off a downgrade, past the pass the
/// controller needs to notice the grade ended.
fn off_grade(traces: &[Trace], causes: &[Cause], barking_only: bool) -> Vec<String> {
    let mut out = Vec::new();
    for trace in traces {
        for sample in &trace.retarding {
            if !causes.contains(&sample.cause) {
                continue;
            }
            if barking_only && !sample.barking {
                continue;
            }
            if sample.road() == Road::Descent {
                continue;
            }
            if trace.since_descent(sample.index) <= RELEASE_GRACE_FRAMES {
                continue;
            }
            out.push(format!(
                "{} at mile {:.2}, grade {:+.2}% [{}], {} frames past the grade: {}",
                sample.road_name,
                sample.mile,
                sample.grade_pct,
                sample.road().label(),
                trace.since_descent(sample.index),
                sample.cause_line()
            ));
        }
    }
    out
}

// -- the sweep -------------------------------------------------------------------------

#[test]
#[cfg_attr(
    not(feature = "slow-tests"),
    ignore = "sweep: every bench road driven end to end"
)]
fn test_the_retarder_only_ever_answers_a_grade() {
    let traces: Vec<Trace> = roads().iter().map(drive_road).collect();
    print_tables(&traces);

    // The control: a sustained descent HAS to reach for the retarder, or the
    // sweep is measuring a truck with no retarder at all and every "nothing
    // on the flat" line below is vacuous.
    let descent = traces
        .iter()
        .find(|t| t.name == "long descent")
        .expect("the long descent road");
    assert!(
        descent.retarding.iter().any(|s| s.road() == Road::Descent),
        "no retarder at all on a sustained six percent descent"
    );

    // The report's own first question: an ASSIST may hold the retarder up on
    // a grade and nowhere else. Off the grade it must hand it back, not carry
    // it onto the flat and not bark it at a hill the truck has to climb.
    let assists = off_grade(&traces, &[Cause::Cruise, Cause::CurveAssist], false);
    let shown: Vec<&String> = assists.iter().take(12).collect();
    assert!(
        assists.is_empty(),
        "{} assist-held retarder frames off a downgrade; first few:
{:#?}",
        assists.len(),
        shown
    );

    // And the second: with cruise on and the road level, nothing retards at
    // all. Auto mode is the driver's own, so it is allowed to hold a stage --
    // but a stage that BARKS on level road is retardation nobody asked for.
    let all = off_grade(
        &traces,
        &[
            Cause::Cruise,
            Cause::CurveAssist,
            Cause::AutoMode,
            Cause::Driver,
        ],
        true,
    );
    let shown: Vec<&String> = all.iter().take(12).collect();
    assert!(
        all.is_empty(),
        "{} frames of real retardation off a downgrade; first few:
{:#?}",
        all.len(),
        shown
    );
}
