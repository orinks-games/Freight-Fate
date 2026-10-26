//! The drive as the road view needs it: the road's shape for the next
//! three-quarters of a mile, what is on it, what the sky is doing, and the
//! dashboard readings.
//!
//! Every sign here comes from the drive's own data -- the posted limits the
//! truck is held to, the curves it will have to slow for, the exits and
//! stops on its route, the work zones and billboards the trip placed. The
//! scenery between them (trees, posts, buildings) is decoration seeded from
//! the road position, so it is the same each time a stretch is driven.

use ff_core::data::curves::RouteCurve;
use ff_core::sim::trip::Trip;
use ff_core::sim::trip_models::highway_class;
use ff_core::sim::weather::WeatherKind;

use ff_core::l10n::Translator;

use super::tr_drawn;
use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::constants::DRIVE_PHASE_PICKUP;

pub const FT_PER_MI: f64 = 5280.0;
/// The road is sampled from just behind the camera to this far ahead.
pub const ROAD_START_FT: f32 = -240.0;
pub const ROAD_STEP_FT: f32 = 24.0;
pub const ROAD_SAMPLES: usize = 170;
/// Farthest anything is placed (a little short of the last sample).
pub const VIEW_FT: f32 = ROAD_START_FT + ROAD_STEP_FT * (ROAD_SAMPLES as f32 - 2.0);
/// Traffic this far behind still shows on the dashboard's traffic radar.
pub const RADAR_BEHIND_FT: f32 = 1320.0;

/// The road's shape ahead, sampled every [`ROAD_STEP_FT`].
#[derive(Clone, Debug, PartialEq)]
pub struct RoadAhead {
    /// Curvature at each sample, 1/ft; positive bends right.
    pub curvature: Vec<f32>,
    /// Grade at each sample as a fraction; positive climbs.
    pub grade: Vec<f32>,
}

impl RoadAhead {
    pub fn sample_s(i: usize) -> f32 {
        ROAD_START_FT + ROAD_STEP_FT * i as f32
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaneLook {
    /// Travel lanes on the truck's side, right lane = 0.
    pub count: i32,
    pub truck_lane: i32,
    /// The truck's offset from its lane's centre, feet, positive right.
    pub offset_ft: f32,
    /// The truck's heading against the road's, radians, positive right.
    pub yaw: f32,
    /// A median (or barrier) beyond the left lane rather than oncoming traffic.
    pub divided: bool,
    pub shoulder_right_ft: f32,
    pub shoulder_left_ft: f32,
    pub lane_width_ft: f32,
    /// On an exit or entrance ramp: one lane, no left shoulder.
    pub ramp: bool,
    /// Town streets (to and from a facility): curbs, buildings, lights.
    pub street: bool,
    /// Near a city on the route: buildings and lights along the highway.
    pub urban: bool,
    /// How hard the rumble strip is buzzing, 0..1.
    pub rumble: f32,
    /// Traffic holds the lane to the left / right beside the truck: the
    /// blind-spot lights. Read through the same check the game uses for a
    /// sideswipe and for the spoken "lane open" cue, so a lane drawn clear
    /// is one the truck can move into.
    pub blocked_left: bool,
    pub blocked_right: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    None,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VehicleKind {
    Car,
    Semi,
    BoxTruck,
    Service,
    Trooper,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TrafficView {
    /// Distance from the truck's front bumper to this vehicle's rear, feet.
    pub ahead_ft: f32,
    /// Absolute lane, right lane = 0 (the truck's numbering).
    pub lane: i32,
    pub kind: VehicleKind,
    pub braking: bool,
    pub signal: Signal,
    /// Roof beacons lit (a trooper running lights and siren).
    pub beacons: bool,
    /// Stable per vehicle: picks its paint.
    pub seed: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum SignKind {
    /// MUTCD R2-1.
    SpeedLimit(u32),
    /// Curve warning (W1-2) with its advisory speed plaque.
    Curve {
        left: bool,
        mph: u32,
    },
    /// Chevron alignment marker (W1-8) on the outside of a sharp bend.
    Chevron {
        left: bool,
    },
    RoadWork,
    EndRoadWork,
    /// Green guide sign: destination lines and an optional distance line.
    Guide {
        lines: Vec<String>,
        distance: Option<String>,
    },
    /// Blue service sign.
    Service {
        lines: Vec<String>,
    },
    /// A state line.
    Welcome(String),
    Billboard {
        text: String,
        seed: u32,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignView {
    pub ahead_ft: f32,
    /// 1 right of the road, -1 left.
    pub side: i8,
    pub kind: SignKind,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WorkZone {
    pub start_ft: f32,
    pub end_ft: f32,
    pub closed_lane: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Biome {
    /// The cactus deserts of the far Southwest.
    Desert,
    /// Sagebrush and juniper: the Great Basin, the Wyoming and Colorado
    /// high country, the Snake River plain.
    Steppe,
    Plains,
    Woodland,
    Conifer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Terrain {
    Flat,
    Hills,
    Mountains,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Season {
    Winter,
    Spring,
    Summer,
    Autumn,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WeatherLook {
    pub kind: WeatherKind,
    pub visibility_ft: f32,
    pub wind: f32,
    pub wet: bool,
    pub snow_cover: bool,
    /// Local wall-clock hour, 0..24.
    pub hour: f32,
    pub season: Season,
    pub biome: Biome,
    pub terrain: Terrain,
    /// Compass bearing of travel, radians clockwise from north, when the
    /// route's coordinates are known: where the sun and moon sit.
    pub heading: Option<f32>,
}

impl WeatherLook {
    pub fn precipitating(&self) -> bool {
        matches!(
            self.kind,
            WeatherKind::Rain
                | WeatherKind::HeavyRain
                | WeatherKind::Thunderstorm
                | WeatherKind::Snow
        )
    }
}

/// Dashboard readings, already in the player's units where they are text.
#[derive(Clone, Debug, PartialEq)]
pub struct Hud {
    pub title: String,
    pub speed_mph: f64,
    pub limit_mph: Option<f64>,
    pub limit_reason: Option<String>,
    pub rpm: f64,
    pub gear: String,
    pub automatic: bool,
    pub throttle: f64,
    pub brake: f64,
    pub cruise_mph: Option<f64>,
    pub keeper_mph: Option<f64>,
    pub jake_stage: i32,
    pub fuel_fraction: f64,
    pub air_psi: f64,
    pub air_low: bool,
    pub parking_brake: bool,
    pub spring_brakes: bool,
    pub engine_on: bool,
    pub truck_damage: f64,
    pub cargo_damage: f64,
    pub signal: Signal,
    pub reverse: bool,
    pub progress: f64,
    /// "312 mi left" in the player's units.
    pub remaining_text: String,
    pub destination: String,
    /// The next thing to do on the route and how far off it is.
    pub next_cue: Option<(String, String)>,
    /// The next bend: left?, advisory, distance text, severity word.
    pub curve: Option<(bool, i64, String, &'static str)>,
    pub clock: String,
    pub weather_name: String,
    pub lane_name: String,
    pub lane_assist: String,
    pub imperial: bool,
    pub siren: bool,
    pub pulled_over: bool,
    pub deadhead: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DriveView {
    /// The truck's place on the route in feet: anchors painted lines and
    /// roadside scenery so they stay put as the truck passes them.
    pub position_ft: f64,
    pub road: RoadAhead,
    pub lanes: LaneLook,
    pub traffic: Vec<TrafficView>,
    pub signs: Vec<SignView>,
    pub zones: Vec<WorkZone>,
    pub weather: WeatherLook,
    pub hud: Hud,
}

impl DriveView {
    /// Whether something on screen blinks by itself (turn signals,
    /// beacons), so a stopped truck still needs fresh frames.
    pub fn lights_flash(&self) -> bool {
        self.hud.signal != Signal::None
            || self.hud.siren
            || self
                .traffic
                .iter()
                .any(|v| v.beacons || v.signal != Signal::None)
    }

    pub fn capture(d: &DrivingState, ctx: &GameContext) -> DriveView {
        let trip = &d.trip;
        let pos = trip.position_mi;
        let truck = &trip.truck;
        let road = sample_road(trip);
        let lanes = lane_look(d);
        let weather = weather_look(trip);
        let traffic = traffic_views(d);
        let signs = sign_views(&ctx.translator, trip, &lanes);
        let zones = trip
            .zones
            .iter()
            .filter(|z| z.reason.contains("construction"))
            .filter_map(|z| {
                let start_ft = ((z.start_mi - pos) * FT_PER_MI) as f32;
                let end_ft = ((z.end_mi - pos) * FT_PER_MI) as f32;
                (end_ft > ROAD_START_FT && start_ft < VIEW_FT).then_some(WorkZone {
                    start_ft,
                    end_ft,
                    closed_lane: z.closed_lane.map(|l| l as i32),
                })
            })
            .collect();
        let settings = &ctx.settings;
        let (limit, reason) = d.display_speed_limit();
        let gear = match truck.transmission.gear {
            g if g < 0 => "R".to_string(),
            0 => "N".to_string(),
            g if truck.transmission.in_neutral() => format!("N{g}"),
            g => g.to_string(),
        };
        let signal = if d.exit_blinker_on() {
            Signal::Right
        } else if let Some(target) = d.lane_change_target {
            if target > d.lane.lane {
                Signal::Left
            } else {
                Signal::Right
            }
        } else {
            Signal::None
        };
        let total = trip.total_miles().max(0.01);
        let remaining = trip.remaining_miles().max(0.0);
        let decimals = if remaining < 10.0 { 1 } else { 0 };
        let remaining_text = tr_drawn(
            &ctx.translator,
            &format!(
                "{} {} left",
                settings.distance_value(remaining, decimals, true),
                settings.distance_unit_text(true)
            ),
        );
        let distance_text = |miles: f64| {
            let decimals = if miles < 10.0 { 1 } else { 0 };
            format!(
                "{} {}",
                settings.distance_value(miles.max(0.0), decimals, true),
                settings.distance_unit_text(true)
            )
        };
        let next_cue = next_cue(trip).map(|(text, ahead)| (text, distance_text(ahead)));
        let curve = trip.next_curve_within(1.0).map(|(ahead, c)| {
            (
                c.direction == 'L',
                c.advisory_mph,
                distance_text(ahead),
                c.severity(),
            )
        });
        let title = d.visible_lines(ctx).into_iter().next().unwrap_or_default();
        let hud = Hud {
            title,
            speed_mph: truck.speed_mph(),
            limit_mph: (limit > 0.0).then_some(limit),
            limit_reason: reason,
            rpm: truck.rpm,
            gear,
            automatic: truck.transmission.automatic,
            throttle: truck.throttle,
            brake: truck.brake,
            cruise_mph: d.cruise_mph,
            keeper_mph: d.keeper_mph,
            jake_stage: truck.engine_brake_stage,
            fuel_fraction: truck.fuel_fraction(),
            air_psi: truck.air_pressure_psi(),
            air_low: truck.air_low_warning(),
            parking_brake: truck.parking_brake,
            spring_brakes: truck.spring_brakes_active(),
            engine_on: truck.engine_on && !truck.stalled,
            truck_damage: truck.damage_pct,
            cargo_damage: truck.cargo_damage_pct,
            signal,
            reverse: truck.transmission.gear < 0,
            progress: ((total - remaining) / total).clamp(0.0, 1.0),
            remaining_text,
            destination: trip.destination_label.clone(),
            next_cue,
            curve,
            clock: clock_text(weather.hour),
            weather_name: capitalize(trip.weather.current.value()),
            lane_name: capitalize(d.lane.lane_name()),
            lane_assist: settings.lane_keeping.clone(),
            imperial: settings.imperial_units,
            siren: d.siren.active,
            pulled_over: d.pull_over.is_some(),
            deadhead: d.phase == DRIVE_PHASE_PICKUP,
        };
        DriveView {
            position_ft: pos * FT_PER_MI,
            road,
            lanes,
            traffic,
            signs,
            zones,
            weather,
            hud,
        }
    }
}

fn capitalize(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// "2:35 PM" from a 0..24 hour.
pub fn clock_text(hour: f32) -> String {
    let minutes = (hour.rem_euclid(24.0) * 60.0).floor() as u32;
    let (h, m) = (minutes / 60, minutes % 60);
    let suffix = if h < 12 { "AM" } else { "PM" };
    let h12 = match h % 12 {
        0 => 12,
        n => n,
    };
    format!("{h12}:{m:02} {suffix}")
}

/// Curvature and grade along the road ahead.
fn sample_road(trip: &Trip) -> RoadAhead {
    let pos = trip.position_mi;
    let total = trip.total_miles().max(0.0);
    let mile_at = |i: usize| pos + RoadAhead::sample_s(i) as f64 / FT_PER_MI;
    let lo = mile_at(0);
    let hi = mile_at(ROAD_SAMPLES);
    // Curves are stored in route order; only the few in view matter.
    let in_view: Vec<&RouteCurve> = trip
        .curves
        .iter()
        .filter(|c| c.start_mi.max(c.end_mi) >= lo && c.start_mi.min(c.end_mi) <= hi)
        .collect();
    let mut curvature = Vec::with_capacity(ROAD_SAMPLES);
    for i in 0..ROAD_SAMPLES {
        let mile = mile_at(i);
        let mut k = 0.0f32;
        for c in &in_view {
            k += curve_curvature(c, mile);
        }
        curvature.push(k);
    }
    // Grades change by segment; read one every fourth sample and blend, so
    // a crest rounds over instead of folding.
    let mut coarse = Vec::new();
    let mut i = 0;
    while i < ROAD_SAMPLES + 4 {
        coarse.push(trip.grade_at(mile_at(i).clamp(0.0, total)) as f32);
        i += 4;
    }
    let mut grade = Vec::with_capacity(ROAD_SAMPLES);
    for i in 0..ROAD_SAMPLES {
        let at = i as f32 / 4.0;
        let j = (at.floor() as usize).min(coarse.len().saturating_sub(2));
        let t = at - j as f32;
        let a = coarse.get(j).copied().unwrap_or(0.0);
        let b = coarse.get(j + 1).copied().unwrap_or(a);
        grade.push((a + (b - a) * t).clamp(-0.12, 0.12));
    }
    RoadAhead { curvature, grade }
}

/// A curve's bend at `mile`: its radius through the middle, eased in and
/// out over the first and last fifth so the road does not kink.
fn curve_curvature(c: &RouteCurve, mile: f64) -> f32 {
    let lo = c.start_mi.min(c.end_mi);
    let hi = c.start_mi.max(c.end_mi);
    if mile < lo || mile > hi || c.min_radius_ft <= 0 {
        return 0.0;
    }
    let span = (hi - lo).max(1e-6);
    let ease = (span * 0.2).max(60.0 / FT_PER_MI).min(span * 0.5);
    let into = ((mile - lo) / ease).min((hi - mile) / ease).clamp(0.0, 1.0);
    let k = 1.0 / c.min_radius_ft as f64;
    let sign = if c.direction == 'L' { -1.0 } else { 1.0 };
    (sign * k * into) as f32
}

fn lane_look(d: &DrivingState) -> LaneLook {
    let trip = &d.trip;
    let leg = trip.route.legs.get(trip.current_leg_index());
    let class = leg
        .map(|l| highway_class(&l.highway))
        .unwrap_or("interstate");
    let divided = trip
        .lanes_at(None)
        .map(|(_, divided)| divided)
        .or_else(|| leg.and_then(|l| l.divided))
        .unwrap_or(class == "interstate");
    let ramp = d.ramp_mi.is_some() || trip.on_ramp;
    let street = d.surface_chain || d.departure_chain;
    let (right, left, width) = match (street, ramp, class) {
        (true, _, _) => (2.0, 1.0, 11.0),
        (_, true, _) => (8.0, 2.0, 14.0),
        (_, _, "interstate") => (10.0, 4.0, 12.0),
        (_, _, "us_highway") => (8.0, 3.0, 12.0),
        _ => (6.0, 2.0, 11.0),
    };
    let count = if ramp {
        1
    } else {
        d.lane.lane_count.clamp(1, 6) as i32
    };
    let lane = d.lane.lane;
    let blocked =
        |other: i64| (0..d.lane.lane_count).contains(&other) && d.lane_gap_blocker(other).is_some();
    LaneLook {
        count,
        truck_lane: (d.lane.lane as i32).clamp(0, count - 1),
        // The lane model's unit is half a lane, six feet.
        offset_ft: (d.lane.offset * 6.0) as f32,
        yaw: d.lane.yaw_rad as f32,
        divided,
        shoulder_right_ft: right,
        shoulder_left_ft: left,
        lane_width_ft: width,
        ramp,
        street,
        urban: street || trip.near_city(trip.position_mi),
        rumble: d.lane.rumble_level() as f32,
        blocked_left: blocked(lane + 1),
        blocked_right: blocked(lane - 1),
    }
}

fn weather_look(trip: &Trip) -> WeatherLook {
    let kind = trip.weather.current;
    let effects = trip.weather.effects();
    let season = match trip.weather.season().unwrap_or("summer") {
        "winter" => Season::Winter,
        "spring" => Season::Spring,
        "autumn" => Season::Autumn,
        _ => Season::Summer,
    };
    let (lat, lon) = trip.latlon_at(None);
    let terrain = match trip.terrain_at(None).as_str() {
        "mountain" => Terrain::Mountains,
        "hills" => Terrain::Hills,
        _ => Terrain::Flat,
    };
    WeatherLook {
        kind,
        visibility_ft: (effects.visibility_mi * FT_PER_MI) as f32,
        wind: effects.wind as f32,
        wet: effects.water_mm > 0.0
            || matches!(
                kind,
                WeatherKind::Rain | WeatherKind::HeavyRain | WeatherKind::Thunderstorm
            ),
        snow_cover: matches!(kind, WeatherKind::Snow | WeatherKind::Ice)
            || trip.truck.surface == "snow",
        hour: trip.local_hour() as f32,
        season,
        biome: biome(lat, lon, terrain),
        terrain,
        heading: bearing(trip),
    }
}

/// Which way the road runs here, from two points a few tenths of a mile
/// apart on it.
fn bearing(trip: &Trip) -> Option<f32> {
    let pos = trip.position_mi;
    let (lat0, lon0) = trip.latlon_at(Some((pos - 0.15).max(0.0)));
    let (lat1, lon1) = trip.latlon_at(Some(pos + 0.15));
    if (lat0 == 0.0 && lon0 == 0.0) || (lat1 == 0.0 && lon1 == 0.0) {
        return None;
    }
    let dy = lat1 - lat0;
    let dx = (lon1 - lon0) * lat0.to_radians().cos();
    if dx.abs() < 1e-9 && dy.abs() < 1e-9 {
        return None;
    }
    Some(dx.atan2(dy).rem_euclid(std::f64::consts::TAU) as f32)
}

/// Broad landscape from where the truck is: cactus desert in the far
/// Southwest, sagebrush across the rest of the dry West, conifers in the
/// mountains and up the Pacific coast, open plains, the wooded east.
fn biome(lat: f64, lon: f64, terrain: Terrain) -> Biome {
    if lat == 0.0 && lon == 0.0 {
        return Biome::Woodland;
    }
    if lon < -104.0 {
        if terrain == Terrain::Mountains || (lon < -121.5 && lat >= 40.0) {
            return Biome::Conifer;
        }
        if lat < 36.0 && lon > -118.5 {
            return Biome::Desert;
        }
        if (-122.6..-119.0).contains(&lon) && (35.0..40.5).contains(&lat) {
            return Biome::Plains; // California's Central Valley farmland
        }
        return Biome::Steppe;
    }
    if lon < -95.5 {
        return Biome::Plains;
    }
    Biome::Woodland
}

fn hash_str(text: &str) -> u32 {
    // FNV-1a: stable across runs and platforms, which is all paint needs.
    let mut h: u32 = 0x811c_9dc5;
    for byte in text.bytes() {
        h ^= byte as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}

fn traffic_views(d: &DrivingState) -> Vec<TrafficView> {
    let pos = d.trip.position_mi;
    let siren = d.siren.active;
    d.trip
        .npc_vehicles()
        .iter()
        .filter_map(|v| {
            let ahead_ft = ((v.position_mi - pos) * FT_PER_MI) as f32;
            if !(-RADAR_BEHIND_FT..=VIEW_FT).contains(&ahead_ft) {
                return None;
            }
            let kind = match v.vehicle_class.as_str() {
                "semi" => VehicleKind::Semi,
                "box truck" => VehicleKind::BoxTruck,
                "service vehicle" => VehicleKind::Service,
                "state trooper" => VehicleKind::Trooper,
                _ => VehicleKind::Car,
            };
            let signal = match (v.intent.as_str(), v.relative_lane) {
                ("merging" | "passing", r) if r != 0 => {
                    if v.exit_at_mi.is_some() {
                        Signal::Right
                    } else {
                        Signal::Left
                    }
                }
                _ => Signal::None,
            };
            Some(TrafficView {
                ahead_ft,
                lane: v.lane as i32,
                kind,
                braking: v.intent == "braking" || v.target_speed_mph < v.speed_mph - 2.0,
                signal,
                beacons: kind == VehicleKind::Trooper && siren,
                seed: hash_str(&v.key),
            })
        })
        .collect()
}

/// The next thing the driver must do, worded for a sign.
fn next_cue(trip: &Trip) -> Option<(String, f64)> {
    let pos = trip.position_mi;
    trip.navigation_cues
        .iter()
        .filter(|c| c.at_mi > pos && c.at_mi - pos < 60.0)
        .filter(|c| {
            matches!(
                c.kind.as_str(),
                "maneuver" | "interchange" | "onramp" | "local_turn" | "state_crossing" | "toll"
            )
        })
        .min_by(|a, b| a.at_mi.total_cmp(&b.at_mi))
        .map(|c| (capitalize(c.text.trim_end_matches('.')), c.at_mi - pos))
}

/// Every sign in view, from the route's own data.
fn sign_views(tr: &Translator, trip: &Trip, lanes: &LaneLook) -> Vec<SignView> {
    let pos = trip.position_mi;
    let ahead = |mile: f64| ((mile - pos) * FT_PER_MI) as f32;
    let in_view = |ft: f32| (0.0..VIEW_FT).contains(&ft);
    let mut out = Vec::new();
    let mut push = |ft: f32, side: i8, kind: SignKind| {
        if in_view(ft) {
            out.push(SignView {
                ahead_ft: ft,
                side,
                kind,
            });
        }
    };
    let window_mi = VIEW_FT as f64 / FT_PER_MI + 0.2;

    // Posted limits: a drop where it takes effect, and the limit repeated
    // every four miles as real highways do -- each one the limit at that
    // very spot, so a repeater never disagrees with what is enforced.
    if !lanes.street && !lanes.ramp {
        if let Some((boundary, limit)) = trip.next_limit_drop() {
            push(
                ahead(boundary),
                1,
                SignKind::SpeedLimit(limit.round() as u32),
            );
        }
        let first = (pos / 4.0).ceil() * 4.0;
        let mut mile = first;
        while mile < pos + window_mi {
            let limit = trip.corridor_limit_at(mile);
            if limit > 0.0 {
                push(ahead(mile), 1, SignKind::SpeedLimit(limit.round() as u32));
            }
            mile += 4.0;
        }
    }

    // Bends: the warning ahead of each, chevrons around the sharp ones.
    for c in trip
        .curves
        .iter()
        .filter(|c| !c.connector && c.start_mi > pos - 0.2 && c.start_mi < pos + window_mi + 0.2)
    {
        let left = c.direction == 'L';
        let warn_at = ahead(c.start_mi) - 500.0;
        push(
            warn_at,
            1,
            SignKind::Curve {
                left,
                mph: c.advisory_mph.max(5) as u32,
            },
        );
        if matches!(c.severity(), "sharp" | "hairpin") {
            let (lo, hi) = (
                ahead(c.start_mi.min(c.end_mi)),
                ahead(c.start_mi.max(c.end_mi)),
            );
            let mut s = lo.max(0.0);
            let outside: i8 = if left { 1 } else { -1 };
            while s <= hi {
                push(s, outside, SignKind::Chevron { left });
                s += 160.0;
            }
        }
    }

    // Work zones.
    for z in trip
        .zones
        .iter()
        .filter(|z| z.reason.contains("construction"))
    {
        push(ahead(z.start_mi) - 1500.0, 1, SignKind::RoadWork);
        push(
            ahead(z.start_mi),
            1,
            SignKind::SpeedLimit(z.limit_mph.round() as u32),
        );
        push(ahead(z.end_mi), 1, SignKind::EndRoadWork);
    }

    // Route guidance: half a mile out, and at the point itself.
    for cue in trip
        .navigation_cues
        .iter()
        .filter(|c| c.at_mi > pos - 0.1 && c.at_mi < pos + window_mi + 0.6)
    {
        let text = capitalize(cue.text.trim_end_matches('.'));
        match cue.kind.as_str() {
            "maneuver" | "interchange" | "onramp" => {
                push(
                    ahead(cue.at_mi) - 2640.0,
                    1,
                    SignKind::Guide {
                        lines: vec![text.clone()],
                        distance: Some(tr_drawn(tr, "1/2 MILE")),
                    },
                );
                push(
                    ahead(cue.at_mi),
                    1,
                    SignKind::Guide {
                        lines: vec![text],
                        distance: None,
                    },
                );
            }
            "state_crossing" => {
                if let Some(state) = cue
                    .text
                    .split(" into ")
                    .nth(1)
                    .map(|rest| rest.split(" near ").next().unwrap_or(rest).trim())
                {
                    push(ahead(cue.at_mi), 1, SignKind::Welcome(state.to_string()));
                }
            }
            "toll" => push(
                ahead(cue.at_mi) - 1000.0,
                1,
                SignKind::Service {
                    lines: vec![tr_drawn(tr, "TOLL"), tr_drawn(tr, "PLAZA AHEAD")],
                },
            ),
            _ => {}
        }
    }

    // Stops: a blue sign a mile out and at the exit.
    for stop in trip
        .stops
        .iter()
        .filter(|s| s.at_mi > pos - 0.1 && s.at_mi < pos + window_mi + 1.1)
    {
        let label = match stop.stop_type.as_str() {
            "weigh_station" => vec![tr_drawn(tr, "WEIGH STATION"), tr_drawn(tr, "ALL TRUCKS")],
            "public_rest_area" => vec![tr_drawn(tr, "REST AREA")],
            _ => vec![stop.name.to_uppercase()],
        };
        let mut far = label.clone();
        far.push(tr_drawn(tr, "1 MILE"));
        push(
            ahead(stop.at_mi) - FT_PER_MI as f32,
            1,
            SignKind::Service { lines: far },
        );
        push(ahead(stop.at_mi), 1, SignKind::Service { lines: label });
    }

    // Billboards and their words, as the trip placed them.
    for board in trip
        .billboards
        .iter()
        .filter(|b| b.at_mi > pos - 0.1 && b.at_mi < pos + window_mi)
    {
        let text = board
            .spoken
            .trim_start_matches("Billboard:")
            .trim()
            .trim_end_matches('.')
            .to_string();
        let seed = hash_str(&board.key);
        let side = if seed.is_multiple_of(3) { -1 } else { 1 };
        push(ahead(board.at_mi), side, SignKind::Billboard { text, seed });
    }

    out.sort_by(|a, b| b.ahead_ft.total_cmp(&a.ahead_ft));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_clock_reads_like_a_dashboard() {
        assert_eq!(clock_text(0.0), "12:00 AM");
        assert_eq!(clock_text(13.5), "1:30 PM");
        assert_eq!(clock_text(23.99), "11:59 PM");
        assert_eq!(clock_text(24.25), "12:15 AM");
    }

    #[test]
    fn a_curve_eases_in_and_bends_the_way_it_turns() {
        let curve = RouteCurve {
            start_mi: 10.0,
            apex_mi: 10.1,
            end_mi: 10.2,
            direction: 'L',
            advisory_mph: 40,
            min_radius_ft: 800,
            deflection_deg: 45.0,
            connector: false,
        };
        assert_eq!(curve_curvature(&curve, 9.9), 0.0);
        let edge = curve_curvature(&curve, 10.001);
        let middle = curve_curvature(&curve, 10.1);
        assert!(middle < 0.0, "a left bend curves left");
        assert!((middle + 1.0 / 800.0).abs() < 1e-6);
        assert!(edge.abs() < middle.abs(), "eased in at the start");
    }

    #[test]
    fn landscapes_follow_the_map() {
        assert_eq!(biome(33.45, -112.07, Terrain::Flat), Biome::Desert); // Phoenix
        assert_eq!(biome(40.06, -106.39, Terrain::Hills), Biome::Steppe); // Kremmling, Colorado
        assert_eq!(biome(39.5, -117.0, Terrain::Flat), Biome::Steppe); // central Nevada
        assert_eq!(biome(45.5, -122.7, Terrain::Flat), Biome::Conifer); // Portland
        assert_eq!(biome(39.7, -105.5, Terrain::Mountains), Biome::Conifer); // Rockies
        assert_eq!(biome(38.9, -99.3, Terrain::Flat), Biome::Plains); // Kansas
        assert_eq!(biome(35.9, -83.9, Terrain::Hills), Biome::Woodland); // Tennessee
    }
}
