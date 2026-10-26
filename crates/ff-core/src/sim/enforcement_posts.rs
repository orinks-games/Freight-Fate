//! Enforcement posts: the places on a corridor where an officer may be
//! sitting (port of `freight_fate/sim/enforcement_posts.py`).
//!
//! A post is a PLACE WITH A BODY. It sits at a milepost, not across a span,
//! and observes up-corridor as far as its method reaches. Whether anyone is
//! sitting there today is settled once, when the trip is built, from the
//! trip seed -- most posts are empty most of the time, which is what makes
//! the staffed ones matter. Whether that body notices *you* is the graded
//! decision in `enforcement_observe`.
//!
//! Every kind is anchored to data the world already carries: median
//! crossovers off `leg.interchanges`, roving patrols (the trooper NPC is the
//! body), work-zone details on construction zones, scale aprons and open
//! scales on `weigh_station` stops, urban units on the urban radius, CMV
//! units on truck corridors, chain controls on `chain_law_areas`.
//!
//! Presence is not difficulty: placement and staffing read nothing from
//! `hazard_scale`, and there is no player setting here any more.

use std::fmt::Write;

use crate::pyfmt::{fmt_f, round_py_n};
use crate::pyrandom::PyRandom;
use crate::sim::season::day_of_week;
use crate::sim::trip::Trip;
use crate::sim::trip_models::{
    highway_class, COLD_PATROL_REGIONS, HOT_PATROL_REGIONS, URBAN_RADIUS_MI,
};
use crate::sim::trip_route_helpers::stop_offset_for_direction;

// -- methods ------------------------------------------------------------------
pub const METHOD_RADAR: &str = "radar"; // long reach, weather-tolerant
pub const METHOD_LIDAR: &str = "lidar"; // short, needs a clear line, dies in fog
pub const METHOD_PACING: &str = "pacing"; // a unit that falls in behind you
pub const METHOD_VISUAL: &str = "visual"; // eyes: damage, chains, lights, following
pub const METHOD_SCALE_SCREEN: &str = "scale_screen"; // the scale's own screening lane

/// Reach up-corridor, in miles, by method.
pub fn method_reach_mi(method: &str) -> f64 {
    match method {
        METHOD_RADAR => 1.0,
        METHOD_LIDAR => 0.5,
        METHOD_PACING => 0.6,
        METHOD_VISUAL => 0.2,
        METHOD_SCALE_SCREEN => 0.5,
        _ => 1.0,
    }
}

/// How far a pacing unit has to hold station behind the truck before it has
/// a speed. DISTANCE, not real seconds: the 1-mile tracking window past a
/// post lasts 5.5 real seconds at 65 mph and 10x compression, so a real-
/// seconds gate could never be met (2,000 miles at 12 over: 315 looks, zero
/// catches).
pub const PACING_MIN_MI: f64 = 0.4;
/// How far back a pacing unit stays with the truck; shared by the tracker
/// and the window the post is asked to look in.
pub const PACING_WINDOW_MI: f64 = 1.0;

// -- kinds --------------------------------------------------------------------
pub const KIND_MEDIAN: &str = "median_post";
pub const KIND_ROVING: &str = "roving_patrol";
pub const KIND_WORK_ZONE: &str = "work_zone_post";
pub const KIND_SCALE_APRON: &str = "scale_apron_post";
pub const KIND_FIXED_SCALE: &str = "fixed_scale";
pub const KIND_URBAN: &str = "urban_unit";
pub const KIND_CMV: &str = "cmv_unit";
pub const KIND_CHAIN: &str = "chain_control";

pub const POST_KINDS: [&str; 8] = [
    KIND_MEDIAN,
    KIND_ROVING,
    KIND_WORK_ZONE,
    KIND_SCALE_APRON,
    KIND_FIXED_SCALE,
    KIND_URBAN,
    KIND_CMV,
    KIND_CHAIN,
];

/// Only a commercial-vehicle unit and an open scale run a full inspection.
pub const INSPECTING_KINDS: [&str; 2] = [KIND_CMV, KIND_FIXED_SCALE];

/// Only a patrol kind ever runs a tableau (a trooper already working
/// somebody else): a crossover, a roving unit, or a city unit.
pub const TABLEAU_KINDS: [&str; 3] = [KIND_MEDIAN, KIND_ROVING, KIND_URBAN];

/// Chance a staffed patrol-kind post is already working a stop. Measured
/// over seeded long interstate runs it comes out closer to one tableau every
/// two to three hours -- "rare enough to stay an event, not wallpaper".
pub const TABLEAU_CHANCE: f64 = 0.55;
/// How far before the post the siren becomes audible, and how far this
/// post's own catches start being suppressed. The same distance on purpose.
pub const TABLEAU_SIREN_LEAD_MI: f64 = 1.5;
/// How far past the post the trooper stays occupied with their stop.
pub const TABLEAU_BUSY_PAST_MI: f64 = 2.0;

pub fn method_by_kind(kind: &str) -> &'static str {
    match kind {
        KIND_MEDIAN => METHOD_RADAR,
        KIND_ROVING => METHOD_PACING,
        KIND_WORK_ZONE => METHOD_VISUAL,
        KIND_SCALE_APRON => METHOD_RADAR,
        KIND_FIXED_SCALE => METHOD_SCALE_SCREEN,
        KIND_URBAN => METHOD_LIDAR,
        KIND_CMV => METHOD_VISUAL,
        KIND_CHAIN => METHOD_VISUAL,
        other => panic!("unknown enforcement post kind {other:?}"),
    }
}

/// Which way the post looks. "oncoming" is the median crossover watching
/// the other roadway; "both" is a crossover working either direction.
pub fn facing_by_kind(kind: &str) -> &'static str {
    match kind {
        KIND_MEDIAN | KIND_SCALE_APRON => "both",
        _ => "with_traffic",
    }
}

pub fn agency_by_kind(kind: &str) -> &'static str {
    match kind {
        KIND_FIXED_SCALE | KIND_CMV => "commercial vehicle enforcement",
        KIND_URBAN => "city police",
        _ => "state police",
    }
}

/// The internal context label the spoken lines interpolate as "a trooper on
/// this {reason}".
pub fn reason_by_kind(kind: &str) -> &'static str {
    match kind {
        KIND_MEDIAN | KIND_ROVING => "highway enforcement",
        KIND_WORK_ZONE => "work zone enforcement",
        KIND_SCALE_APRON => "scale apron",
        KIND_FIXED_SCALE => "weigh station",
        KIND_URBAN => "city enforcement",
        KIND_CMV => "commercial vehicle enforcement",
        KIND_CHAIN => "chain control",
        _ => "highway enforcement",
    }
}

/// Base chance a post has a body in it today, before the clock.
pub fn base_staffed(kind: &str) -> f64 {
    match kind {
        KIND_MEDIAN => 0.24,
        KIND_ROVING => 0.34,
        KIND_WORK_ZONE => 0.90,
        KIND_SCALE_APRON => 0.35,
        KIND_FIXED_SCALE => 1.0,
        KIND_URBAN => 0.22,
        KIND_CMV => 0.22,
        KIND_CHAIN => 0.50,
        other => panic!("unknown enforcement post kind {other:?}"),
    }
}

/// How willing this kind of post is to act at all, before severity.
pub fn base_notice(kind: &str) -> f64 {
    match kind {
        KIND_MEDIAN => 0.9,
        KIND_ROVING => 0.85,
        KIND_WORK_ZONE => 0.95,
        KIND_SCALE_APRON => 0.9,
        KIND_FIXED_SCALE => 1.0,
        KIND_URBAN => 0.7,
        KIND_CMV => 0.95,
        KIND_CHAIN => 0.9,
        _ => 0.0,
    }
}

// -- spacing ------------------------------------------------------------------
/// Target miles between posts of each spaced kind, on an interstate at the
/// neutral regional baseline. Region and road class reach SPACING ONLY,
/// never staffing: applying them to both squared the effect.
pub fn spacing_mi(kind: &str) -> f64 {
    match kind {
        KIND_MEDIAN => 40.0,
        KIND_ROVING => 80.0,
        KIND_CMV => 170.0,
        other => panic!("{other:?} is not a spaced post kind"),
    }
}

/// Road class scales spacing: an interstate carries the most enforcement per
/// mile, a two-lane state route the least.
pub fn class_spacing_mult(cls: &str) -> f64 {
    match cls {
        "interstate" => 1.0,
        "us_highway" => 1.5,
        _ => 2.4,
    }
}

pub const HOT_REGION_MULT: f64 = 1.3;
pub const COLD_REGION_MULT: f64 = 0.7;

/// Thin between 2 and 5 in the morning, thick through both commuter peaks.
pub const QUIET_HOURS: (f64, f64) = (2.0, 5.0);
pub const BUSY_HOURS: [(f64, f64); 2] = [(6.0, 9.0), (15.0, 18.0)];
pub const QUIET_HOUR_MULT: f64 = 0.55;
pub const BUSY_HOUR_MULT: f64 = 1.15;

/// Fixed scales are largely closed at the weekend (the openness roll, not a
/// staffing roll: a closed scale can still carry an apron post).
pub const SCALE_OPEN_WEEKDAY: f64 = 0.45;
pub const SCALE_OPEN_WEEKEND: f64 = 0.12;

/// No post is placed inside the first or last stretch of the run.
pub const EDGE_MARGIN_MI: f64 = 3.0;
/// A work-zone detail parks just inside the cones.
pub const WORK_ZONE_POST_OFFSET_MI: f64 = 0.4;

pub fn region_multiplier(region: &str) -> f64 {
    if HOT_PATROL_REGIONS.contains(&region) {
        return HOT_REGION_MULT;
    }
    if COLD_PATROL_REGIONS.contains(&region) {
        return COLD_REGION_MULT;
    }
    1.0
}

/// Time-of-day density. Thin in the small hours, thick at the peaks.
pub fn hour_multiplier(hour: f64) -> f64 {
    let h = hour.rem_euclid(24.0);
    if QUIET_HOURS.0 <= h && h < QUIET_HOURS.1 {
        return QUIET_HOUR_MULT;
    }
    for (start, end) in BUSY_HOURS {
        if start <= h && h < end {
            return BUSY_HOUR_MULT;
        }
    }
    1.0
}

/// Odds a fixed scale is open. Weekends are mostly dark; an unknown day
/// reads as a weekday, the busier case.
pub fn scale_open_chance(career_hours: Option<f64>) -> f64 {
    match career_hours {
        None => SCALE_OPEN_WEEKDAY,
        Some(hours) => {
            if day_of_week(hours) >= 5 {
                SCALE_OPEN_WEEKEND
            } else {
                SCALE_OPEN_WEEKDAY
            }
        }
    }
}

/// Python `f"{trip_seed}"` for an `int | None` seed.
pub fn seed_text(trip_seed: Option<i64>) -> String {
    match trip_seed {
        Some(seed) => seed.to_string(),
        None => "None".to_string(),
    }
}

/// The named, seeded draw key for one police decision. Never time-quantised:
/// a reload, a frame-rate change, or a different pacing must reproduce the
/// same road.
pub fn post_seed(trip_seed: Option<i64>, post_id: &str, purpose: &str) -> String {
    format!("{}:police:{post_id}:{purpose}", seed_text(trip_seed))
}

/// One place on the corridor where an officer may be sitting.
///
/// `at_mi` is a point, not a span. `reach_mi` is how far up-corridor the
/// post's method can observe, so the road it covers is `[at_mi - reach_mi,
/// at_mi]` for traffic coming toward it.
#[derive(Debug, Clone)]
pub struct EnforcementPost {
    pub at_mi: f64,
    pub kind: String,
    pub leg_index: usize,
    pub method: String,
    pub reach_mi: f64,
    pub facing: String,
    pub staffed: bool,
    pub agency: String,
    /// Slug of the anchoring world feature, never spoken.
    pub anchor: String,
    /// Region/class/hour density that produced this post.
    pub density: f64,
    pub notice: f64,
    /// Set by the trip once the player has heard this post announced. A post
    /// cannot observe a driver who was never told it was there.
    pub announced: bool,
    /// A post that has already looked at you and let it go does not re-decide.
    pub declined: bool,
    /// Whether this staffed patrol post already has somebody stopped.
    pub tableau: bool,
}

impl PartialEq for EnforcementPost {
    /// Python `compare=False` on announced/declined/tableau.
    fn eq(&self, other: &Self) -> bool {
        self.at_mi == other.at_mi
            && self.kind == other.kind
            && self.leg_index == other.leg_index
            && self.method == other.method
            && self.reach_mi == other.reach_mi
            && self.facing == other.facing
            && self.staffed == other.staffed
            && self.agency == other.agency
            && self.anchor == other.anchor
            && self.density == other.density
            && self.notice == other.notice
    }
}

impl EnforcementPost {
    /// The dataclass defaults: `EnforcementPost(at_mi, kind)`.
    pub fn new(at_mi: f64, kind: &str) -> Self {
        EnforcementPost {
            at_mi,
            kind: kind.to_string(),
            leg_index: 0,
            method: METHOD_RADAR.to_string(),
            reach_mi: 1.0,
            facing: "with_traffic".to_string(),
            staffed: false,
            agency: "state police".to_string(),
            anchor: String::new(),
            density: 1.0,
            notice: 0.9,
            announced: false,
            declined: false,
            tableau: false,
        }
    }

    pub fn id(&self) -> String {
        let mut id = String::new();
        self.write_id(&mut id);
        id
    }

    /// [`EnforcementPost::id`] written into `out` (cleared first), so a
    /// per-frame caller can reuse one buffer instead of allocating per post.
    pub fn write_id(&self, out: &mut String) {
        out.clear();
        out.push_str("post:");
        let _ = write!(out, "{}:", self.leg_index);
        if self.at_mi.is_nan() {
            out.push_str("nan");
        } else {
            let _ = write!(out, "{:.1}", self.at_mi);
        }
        out.push(':');
        out.push_str(&self.kind);
    }

    /// Short internal context label, interpolated into spoken lines.
    pub fn reason(&self) -> &'static str {
        reason_by_kind(&self.kind)
    }

    pub fn is_scale(&self) -> bool {
        self.kind == KIND_FIXED_SCALE || self.kind == KIND_SCALE_APRON
    }

    /// Whether this post runs a full equipment inspection, not just a ticket.
    pub fn inspects(&self) -> bool {
        INSPECTING_KINDS.contains(&self.kind.as_str())
    }

    /// First mile at which this post can see traffic coming toward it.
    pub fn watch_start_mi(&self) -> f64 {
        (self.at_mi - self.reach_mi).max(0.0)
    }

    // -- legacy read surface: a point post answers with the stretch it watches.

    pub fn start_mi(&self) -> f64 {
        self.watch_start_mi()
    }

    pub fn end_mi(&self) -> f64 {
        // A pacing unit is behind you and moving with you, so its window runs
        // as far back as it will hold station. Every other post is a point on
        // the road that you drive past and leave.
        if self.method == METHOD_PACING {
            return self.at_mi + PACING_WINDOW_MI;
        }
        self.at_mi + 0.3
    }

    pub fn covers(&self, mile: f64) -> bool {
        self.watch_start_mi() <= mile && mile <= self.end_mi()
    }

    /// Whether this post's trooper is occupied with somebody else here: from
    /// the siren lead to a couple of miles past the stop. Only this post's
    /// own catches are affected.
    pub fn tableau_busy_at(&self, mile: f64) -> bool {
        if !self.tableau {
            return false;
        }
        self.at_mi - TABLEAU_SIREN_LEAD_MI <= mile && mile <= self.at_mi + TABLEAU_BUSY_PAST_MI
    }
}

/// One post, with its staffing settled from the trip seed.
///
/// `staffed: None` rolls it; an explicit value is for the kinds whose
/// staffing is a fact rather than a chance (an open scale is staffed).
/// `staffing` carries only the clock: how busy a shift is, not how dense the
/// region is -- density is already spent on where the posts are.
#[allow(clippy::too_many_arguments)]
pub fn build_post(
    at_mi: f64,
    kind: &str,
    leg_index: usize,
    trip_seed: Option<i64>,
    density: f64,
    anchor: &str,
    staffed: Option<bool>,
    staffing: f64,
) -> EnforcementPost {
    let method = method_by_kind(kind);
    let mut post = EnforcementPost {
        at_mi: round_py_n(at_mi, 3),
        kind: kind.to_string(),
        leg_index,
        method: method.to_string(),
        reach_mi: method_reach_mi(method),
        facing: facing_by_kind(kind).to_string(),
        agency: agency_by_kind(kind).to_string(),
        anchor: anchor.to_string(),
        density,
        notice: base_notice(kind),
        ..EnforcementPost::new(at_mi, kind)
    };
    match staffed {
        None => {
            let chance = 0.97_f64.min(base_staffed(kind) * staffing.max(0.1));
            let rolled =
                PyRandom::new_from_str(&post_seed(trip_seed, &post.id(), "staffed")).random();
            post.staffed = rolled < chance;
        }
        Some(value) => post.staffed = value,
    }
    post
}

/// Whether `post` already has somebody stopped, on this trip's seed. Only a
/// staffed patrol-kind post is ever eligible.
pub fn assign_tableau(post: &EnforcementPost, trip_seed: Option<i64>) -> bool {
    if !TABLEAU_KINDS.contains(&post.kind.as_str()) || !post.staffed {
        return false;
    }
    let roll = PyRandom::new_from_str(&post_seed(trip_seed, &post.id(), "tableau")).random();
    roll < TABLEAU_CHANCE
}

/// Trip-side placement and lookup for enforcement posts (Python's
/// `EnforcementPostMixin`).
impl Trip {
    // -- placement -------------------------------------------------------------

    pub fn post_density_at(&self, mile: f64) -> f64 {
        let (leg_i, _) = self.leg_at_mile(mile);
        let cls = highway_class(&self.route.legs[leg_i].highway);
        let mut density = 1.0 / class_spacing_mult(cls);
        density *= region_multiplier(&self.region_at(mile));
        density *= hour_multiplier(self.local_start_hour());
        density
    }

    /// Posts of a spaced kind laid down along the whole route. The step is
    /// the kind's target spacing divided by the local density, from the same
    /// walk, with no extra random draw.
    pub fn spaced_posts(&self, kind: &str) -> Vec<EnforcementPost> {
        let total = self.route.miles();
        let mut posts = Vec::new();
        let mut mile = EDGE_MARGIN_MI;
        let mut guard = 0;
        while mile <= total - EDGE_MARGIN_MI && guard < 4000 {
            guard += 1;
            let density = self.post_density_at(mile);
            let step = spacing_mi(kind) / density.max(0.2);
            let (leg_i, _) = self.leg_at_mile(mile);
            let mut at = mile;
            let mut anchor = String::new();
            if kind == KIND_MEDIAN {
                // A crossover is a piece of interstate infrastructure, so the
                // post lands on the nearest interchange rather than a bare
                // milepost.
                if let Some((snapped, label)) = self.nearest_interchange_mile(mile, step * 0.5) {
                    at = snapped;
                    anchor = label;
                }
            }
            posts.push(build_post(
                at,
                kind,
                leg_i,
                self.seed,
                density,
                &anchor,
                None,
                hour_multiplier(self.local_start_hour()),
            ));
            mile += step;
        }
        posts
    }

    /// Route mile and exit label of the interchange nearest `mile`.
    pub fn nearest_interchange_mile(&self, mile: f64, within_mi: f64) -> Option<(f64, String)> {
        let mut best: Option<(f64, String)> = None;
        let mut best_d = within_mi;
        for (i, (start, leg)) in self
            .leg_starts
            .iter()
            .zip(self.route.legs.iter())
            .enumerate()
        {
            let forward = self.route.cities[i] == leg.a;
            for ix in leg.interchanges() {
                let offset = stop_offset_for_direction(ix.at_mi, leg.miles, forward);
                let route_mi = start + offset;
                let d = (route_mi - mile).abs();
                if d <= best_d {
                    best_d = d;
                    best = Some((route_mi, ix.exit_ref.clone()));
                }
            }
        }
        best
    }

    /// Every post on this route, sorted up-corridor. Reads neither
    /// `hazard_scale` nor any presence setting.
    pub fn place_enforcement_posts(&self) -> Vec<EnforcementPost> {
        if self.is_facility_approach_route() {
            return Vec::new();
        }
        let mut posts = Vec::new();
        for kind in [KIND_MEDIAN, KIND_ROVING, KIND_CMV] {
            posts.extend(self.spaced_posts(kind));
        }
        posts.extend(self.work_zone_posts());
        posts.extend(self.scale_posts());
        posts.extend(self.urban_posts());
        if self.chain_law_level() > 0 {
            posts.extend(self.chain_posts());
        }
        // A stable sort, as Python's is.
        posts.sort_by(|a, b| a.at_mi.partial_cmp(&b.at_mi).expect("finite mileposts"));
        for post in posts.iter_mut() {
            post.tableau = assign_tableau(post, self.seed);
        }
        posts
    }

    pub fn work_zone_posts(&self) -> Vec<EnforcementPost> {
        let mut posts = Vec::new();
        for zone in &self.zones {
            if zone.reason != "construction" {
                continue;
            }
            let at = zone.end_mi.min(zone.start_mi + WORK_ZONE_POST_OFFSET_MI);
            let (leg_i, _) = self.leg_at_mile(at);
            let mut post = build_post(
                at,
                KIND_WORK_ZONE,
                leg_i,
                self.seed,
                self.post_density_at(at),
                &format!("zone:{}", fmt_f(zone.start_mi, 1)),
                None,
                hour_multiplier(self.local_start_hour()),
            );
            // A detail watches the whole coned stretch, not two tenths of it.
            post.reach_mi = post.reach_mi.max(3.0_f64.min(zone.end_mi - zone.start_mi));
            posts.push(post);
        }
        posts
    }

    /// Open scales and the closed scales that still carry an apron post: a
    /// trooper parked on a dark scale apron is one of the most common real
    /// sightings there is.
    pub fn scale_posts(&self) -> Vec<EnforcementPost> {
        let mut posts = Vec::new();
        let open_chance = scale_open_chance(self.career_hours);
        for stop in &self.stops {
            if stop.stop_type != "weigh_station" {
                continue;
            }
            let (leg_i, _) = self.leg_at_mile(stop.at_mi);
            let density = self.post_density_at(stop.at_mi);
            let scale_id = format!("post:{leg_i}:{}:{KIND_FIXED_SCALE}", fmt_f(stop.at_mi, 1));
            let is_open = PyRandom::new_from_str(&post_seed(
                self.seed,
                &scale_id,
                &format!("scale:{}:open", stop.key()),
            ))
            .random()
                < open_chance;
            if is_open {
                posts.push(build_post(
                    stop.at_mi,
                    KIND_FIXED_SCALE,
                    leg_i,
                    self.seed,
                    density,
                    &stop.key(),
                    Some(true),
                    1.0,
                ));
            } else {
                posts.push(build_post(
                    stop.at_mi,
                    KIND_SCALE_APRON,
                    leg_i,
                    self.seed,
                    density,
                    &stop.key(),
                    None,
                    1.0,
                ));
            }
        }
        posts
    }

    /// One unit per route city, on the urban radius that already exists.
    pub fn urban_posts(&self) -> Vec<EnforcementPost> {
        let mut posts = Vec::new();
        let total = self.route.miles();
        for (i, milepost) in self.city_mileposts.iter().enumerate() {
            let at = milepost - URBAN_RADIUS_MI * 0.5;
            if !(EDGE_MARGIN_MI <= at && at <= total - EDGE_MARGIN_MI) {
                continue;
            }
            let (leg_i, _) = self.leg_at_mile(at);
            let city = &self.route.cities[i.min(self.route.cities.len() - 1)];
            posts.push(build_post(
                at,
                KIND_URBAN,
                leg_i,
                self.seed,
                self.post_density_at(at),
                city,
                None,
                hour_multiplier(self.local_start_hour()),
            ));
        }
        posts
    }

    pub fn chain_posts(&self) -> Vec<EnforcementPost> {
        let mut posts = Vec::new();
        for &(start, _end) in &self.chain_law_areas {
            let (leg_i, _) = self.leg_at_mile(start);
            posts.push(build_post(
                start,
                KIND_CHAIN,
                leg_i,
                self.seed,
                self.post_density_at(start),
                &format!("chain:{}", fmt_f(start, 1)),
                None,
                hour_multiplier(self.local_start_hour()),
            ));
        }
        posts
    }

    /// A chain control is set up when the law is posted and packed away when
    /// it lifts (Caltrans: controls go up "when conditions require"), so its
    /// post is on the route only while `chain_law_level` says the law is in
    /// effect. It used to stand at every chain-up area all year: a CB call
    /// about the chain control checking rigs, and its trooper watching for
    /// anything else, on dry October pavement (owner, 2026-10-01). Re-placed
    /// from the seed, so a post that comes back is the same post, and one the
    /// CB already called is not called again.
    pub fn sync_chain_posts(&mut self) {
        let in_effect = self.chain_law_level() > 0;
        let placed = self.posts.iter().any(|post| post.kind == KIND_CHAIN);
        if in_effect && !placed {
            let chain = self.chain_posts();
            if chain.is_empty() {
                return;
            }
            self.posts.extend(chain);
            self.posts
                .sort_by(|a, b| a.at_mi.partial_cmp(&b.at_mi).expect("finite mileposts"));
        } else if !in_effect && placed {
            self.posts.retain(|post| post.kind != KIND_CHAIN);
        }
    }

    // -- lookup ----------------------------------------------------------------

    /// The staffed post watching this mile, most attentive first.
    pub fn active_post_at(&self, mile: f64) -> Option<&EnforcementPost> {
        self.posts
            .iter()
            .filter(|p| p.staffed && p.covers(mile))
            .max_by(|a, b| {
                let ka = (a.notice * a.density, base_notice(&a.kind));
                let kb = (b.notice * b.density, base_notice(&b.kind));
                ka.partial_cmp(&kb).expect("finite post weights")
            })
    }

    /// Staffed posts that could still catch a driver at this mile. A post
    /// running a tableau is dropped for the duration of its busy window.
    pub fn posts_watching(&self, mile: f64) -> Vec<&EnforcementPost> {
        self.posts
            .iter()
            .filter(|p| p.staffed && p.covers(mile) && !p.tableau_busy_at(mile))
            .collect()
    }

    /// Nearest post at or ahead of the truck inside the lookahead.
    pub fn next_post_within(&self, within_mi: f64) -> Option<&EnforcementPost> {
        let pos = self.position_mi;
        self.posts
            .iter()
            .filter(|p| p.end_mi() >= pos && p.at_mi - pos <= within_mi)
            .min_by(|a, b| {
                let ka = (a.at_mi - pos).max(0.0);
                let kb = (b.at_mi - pos).max(0.0);
                ka.partial_cmp(&kb).expect("finite mileposts")
            })
    }

    /// Posts on this route the player will hear something from: every
    /// staffed post, and every scale open or closed.
    pub fn audible_enforcement_contacts(&self) -> Vec<&EnforcementPost> {
        self.posts
            .iter()
            .filter(|p| p.staffed || p.is_scale())
            .collect()
    }

    /// A mutable handle on one post by id, for the driving layer's
    /// `declined`/`announced` bookkeeping.
    pub fn post_mut(&mut self, post_id: &str) -> Option<&mut EnforcementPost> {
        let mut id = String::new();
        self.posts.iter_mut().find(|p| {
            p.write_id(&mut id);
            id == post_id
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn post_seeding_is_deterministic() {
        let a = build_post(12.345, KIND_MEDIAN, 0, Some(7), 1.0, "", None, 1.0);
        let b = build_post(12.345, KIND_MEDIAN, 0, Some(7), 1.0, "", None, 1.0);
        assert_eq!(a.staffed, b.staffed);
        assert_eq!(a.at_mi, 12.345);
        assert_eq!(a.id(), "post:0:12.3:median_post");
        assert_eq!(post_seed(None, "p", "x"), "None:police:p:x");
    }

    #[test]
    fn hour_and_region_multipliers() {
        assert_eq!(hour_multiplier(3.0), QUIET_HOUR_MULT);
        assert_eq!(hour_multiplier(7.5), BUSY_HOUR_MULT);
        assert_eq!(hour_multiplier(12.0), 1.0);
        assert_eq!(region_multiplier("northeast"), HOT_REGION_MULT);
        assert_eq!(region_multiplier("rockies"), COLD_REGION_MULT);
        assert_eq!(scale_open_chance(Some(3.0 * 24.0)), SCALE_OPEN_WEEKEND);
    }

    #[test]
    fn a_pacing_post_watches_a_window_behind_it() {
        let post = EnforcementPost {
            method: METHOD_PACING.to_string(),
            ..EnforcementPost::new(10.0, KIND_ROVING)
        };
        assert_eq!(post.end_mi(), 11.0);
        assert!(post.covers(10.5));
    }
}
