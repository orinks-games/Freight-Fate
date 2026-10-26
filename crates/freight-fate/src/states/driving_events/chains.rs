//! The facility street chains: the surface chain that carries a delivery from
//! the destination ramp to the gate, the departure chain that carries a loaded
//! run out of the origin's gate, and the acceleration lane that ends it; and
//! a road stop's streets from its exit ramp to its lot.

use crate::speech::EventPriority;
use ff_core::data::world_models::Route;
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::weather::WeatherSystem;
use ff_core::speech_pacing::SpeechCategory;
use ff_core::units::spoken_feet_or_meters;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

/// Whether a road stop's streets from its exit ramp to its lot are driven.
/// OFF for 1.9 (owner decision, 2026-09-24): most exits have a ramp end
/// baked for one direction only, so the same truck stop had streets one way
/// and none the other (the Love's at Baird, I-20 exit 307, had none
/// westbound). A stop keeps its entrance at the ramp's end, as before
/// 2026-09-24, until the re-bake that finds each direction's ramp (2.0). A
/// drive copies this into `DrivingState::stop_streets_on`, which the 2.0
/// tests set.
pub const STOP_STREETS_IN_PLAY: bool = false;

impl DrivingState {
    /// The spoken state name for a city key, or "" when the world is silent.
    ///
    /// A street chain's legs carry no state segments -- they are built from
    /// local geometry, not from the corridor bake -- so the trip cannot work
    /// out for itself whose vehicle code governs its streets. The city can.
    pub fn city_state(&self, ctx: &GameContext, city: &str) -> String {
        ctx.world
            .city(city)
            .map(|city| city.state.clone())
            .unwrap_or_default()
    }

    /// The destination facility's tier-1 street chain, or None.
    ///
    /// The chain from the end of the ramp this delivery takes, when the bake
    /// has one from that terminal: it starts on the street the ramp meets.
    /// Otherwise the facility's default chain, routed from the city centre,
    /// and only a genuine multi-segment turn-level route makes one of those;
    /// a single synthetic leg would just be the old teleport with extra
    /// steps, so those facilities keep the scripted arrival.
    pub fn surface_chain_route(&self, ctx: &GameContext) -> Option<Route> {
        if !self.facility_streets_on {
            return None;
        }
        if let Some(node) = self.destination_terminal_node(ctx) {
            let exit_chain = ctx
                .world
                .facility_exit_route(&self.job.destination, &self.job.destination_location, node)
                .ok()
                .flatten();
            if let Some(route) = exit_chain.filter(|route| !route.legs.is_empty()) {
                return Some(route);
            }
        }
        let route = ctx
            .world
            .facility_approach_route(&self.job.destination, &self.job.destination_location)
            .ok()?;
        if route.legs.len() < 2 {
            return None;
        }
        if !route.legs.iter().any(|leg| leg.local_speed_mph > 0.0) {
            return None;
        }
        Some(route)
    }

    /// The OSM node the destination exit's ramp ends at, for the direction
    /// this delivery arrives in: the key of the street chain from there. Read
    /// off the highway trip, which is parked aside once the streets begin.
    pub fn destination_terminal_node(&self, ctx: &GameContext) -> Option<i64> {
        if self.phase != DRIVE_PHASE_DELIVERY {
            return None;
        }
        let highway = if self.surface_chain {
            self.highway_trip.as_ref()?
        } else {
            &self.trip
        };
        let exit_mi = match self.ramp_stop.as_ref() {
            Some(stop) if stop.stop_type == "delivery_destination" => {
                stop.interchange_mi.unwrap_or(stop.at_mi)
            }
            _ => {
                crate::states::driving_events::destination_exit::scan_destination_exit(
                    ctx.world, highway, true,
                )?
                .0
            }
        };
        highway.ramp_terminal_node_at(exit_mi)
    }

    /// Whether this delivery's ramp hands off to a street chain.
    ///
    /// Decides where the ARRIVAL is. With a chain, the ramp's end is a driving
    /// continuation (see `_update_ramp_terminal`'s handoff) and the gate is up
    /// to a mile of streets further on; without one, the ramp ends at the
    /// gate. The destination approach assist stopped the truck dead at the end
    /// of a ramp that had a mile of city still to drive, and left automatic
    /// speed control paused the arrival way for all of it (owner, Spokane,
    /// 2026-08-22: "it didn't stop where I can pull in"). Memoised: the answer
    /// is a property of the job, and the world lookup behind it is not free
    /// per frame.
    pub fn destination_street_chain_ahead(&mut self, ctx: &GameContext) -> bool {
        if self.destination_chain_ahead.is_none() {
            self.destination_chain_ahead = Some(self.surface_chain_route(ctx).is_some());
        }
        self.destination_chain_ahead.unwrap_or(false)
    }

    /// A throwaway weather system, so a new `Trip` can be built before the
    /// real one is moved across (Python aliased one object; `WeatherSystem`
    /// owns a live provider and cannot be cloned).
    fn placeholder_weather(&self) -> WeatherSystem {
        WeatherSystem::new("", Some(self.trip_seed), None, None, false)
    }

    /// Swap the finished highway trip for the facility's street chain.
    ///
    /// The clock, the day of the week, and the toll ledger carry over, so
    /// deadlines, rush hour, and settlement are unaffected: only the road
    /// under the wheels changes.
    pub fn begin_surface_chain(&mut self, ctx: &mut GameContext, announce: bool) -> bool {
        if self.surface_chain {
            return false; // already on the streets
        }
        let Some(route) = self.surface_chain_route(ctx) else {
            return false;
        };
        let local_state = self.city_state(ctx, &self.job.destination.clone());
        let first_corner = self.enter_streets(ctx, &route, local_state, false, announce);
        self.surface_chain = true;
        if announce {
            let street = Self::start_street_text(&route);
            ctx.audio.play_with("ui/notify", 0.7, 0.0);
            let message = format!(
                "{street}.{first_corner} {} to the facility gate.",
                self.trip.distance_text(route.miles())
            );
            self.announce_streets(ctx, message, !first_corner.is_empty());
        }
        true
    }

    /// "Start on X", the street a chain begins on.
    fn start_street_text(route: &Route) -> String {
        let first = &route.legs[0];
        if first.local_cue.is_empty() {
            format!("Start on {}", first.highway)
        } else {
            first.local_cue.trim_end_matches('.').to_string()
        }
    }

    /// The off-the-ramp line: the route channel, and the first corner's
    /// reaction window when it carries one.
    fn announce_streets(&mut self, ctx: &mut GameContext, message: String, with_corner: bool) {
        if with_corner {
            self.turn_grace_s = self.turn_grace_seconds(ctx, &message);
        }
        let mut opts = SayEvent::queued().priority(EventPriority::Important);
        opts.category = Some(SpeechCategory::Navigation);
        ctx.say_event_with(message, opts);
        self.mark_start_cue_said();
    }

    /// Swap the highway trip at the end of a ramp for a street chain, park the
    /// highway aside, and hand the streets to the speed keeper when the
    /// terminal released the truck to facility stopping assistance. Returns
    /// the first corner's call, " Then ...", when the chain starts inside its
    /// window, for the caller's off-the-ramp line; empty otherwise.
    ///
    /// `live` is false when a save is being rebuilt: the truck is parked with
    /// its engine off and the restored position is not set yet, so both
    /// handoffs are left to the per-frame paths -- the keeper resumes once the
    /// truck rolls, and the first corner is called from where the truck
    /// really is. Latched here, a resumed drive never heard that corner's
    /// approach call, and the keeper refused a parked truck as it loaded.
    fn enter_streets(
        &mut self,
        ctx: &mut GameContext,
        route: &Route,
        local_state: String,
        road_stop: bool,
        live: bool,
    ) -> String {
        let options = TripOptions {
            time_scale: self.trip.time_scale,
            seed: Some(self.trip_seed ^ 0x5AFE),
            start_hour: self.trip.start_hour,
            imperial: self.trip.imperial(),
            hazard_scale: 0.0, // no random hazards on the last city miles
            career_hours: self.trip.career_hours,
            bobtail: self.trip.bobtail,
            destination_label: self.trip.destination_label.clone(),
            local_state,
            road_stop,
            ..Default::default()
        };
        let mut surface = Trip::new(
            route.clone(),
            self.trip.truck.clone(),
            self.placeholder_weather(),
            options,
        );
        // These streets ARE the run-in to the dock, so they run on the real
        // clock from their first foot. The exit watch sets this every frame,
        // but it has already run for this one, and a compressed tick here
        // moves the truck seven times further than the brakes can answer for.
        surface.dock_run_in = true;
        surface.game_minutes = self.trip.game_minutes; // deadline and clock continuity
        surface.toll_charges = self.trip.toll_charges.clone(); // settlement reads the live trip
        surface.hos_violation = self.trip.hos_violation;
        let mut old = self.replace_trip(surface);
        let weather = std::mem::replace(&mut old.weather, self.placeholder_weather());
        self.trip.weather = weather;
        self.highway_trip = Some(old);
        // A latched destination arrival belongs to the trip it began on. The
        // ramp's arrival stopped the truck at the ramp's end; the street chain
        // is a fresh approach with its own point half a mile on, and carrying
        // the latch over would hold the pedals -- throttle off, speed control
        // paused -- for the whole chain and coast the truck to a halt in the
        // road.
        self.destination_arrival_active = false;
        self.destination_assist_brake = 0.0;
        self.reset_exit_lane_state();
        self.exit_signal_on = false;
        // The ramp's transit pause ends HERE, not a frame later on a
        // no-braking condition: the streets begin with the truck at whatever
        // speed the terminal let through and, on this chain, a corner 264 feet
        // on. Nothing held that speed down -- cruise was paused for the ramp,
        // the keeper had not come back, and the driver was on the brake
        // because of it, which is the one thing that keeps the keeper from
        // coming back (owner, Spokane, 2026-08-22: "I had to brake to start
        // the turn onto city streets"). The terminal is honoured by driving
        // through it; automatic speed control belongs to the street zones
        // from the first foot of them.
        self.clear_stop_pause();
        // Facility stopping assistance drove the truck here from the terminal
        // (owner ruling, 2026-09-01: signal to entrance, hands off). The
        // streets are the speed keeper's job -- it holds the posted number,
        // eases for each judged corner, and creeps behind a queue -- so it
        // takes them explicitly, the way the acceleration lane is handed to it
        // on the way out, rather than waiting a frame for the resume path.
        // The arrival assist takes the pedals back at the gate as it always
        // has. `approach_pull_ahead_available` refused the pull-ahead with the
        // keeper off, so a chain reached this way always has it. An armed
        // session takes the streets the same way, now rather than a frame
        // later: a free-flowing ramp hands over at the ramp's own speed, and
        // for that frame nothing held the truck to the street's number.
        let pull_ahead = std::mem::take(&mut self.approach_pull_ahead);
        if !live {
            return String::new();
        }
        if (pull_ahead || self.speed_control_armed) && ctx.settings.speed_keeper {
            let (limit, zone_reason) = self.trip.speed_limit_at(self.trip.position_mi);
            if let Some(zone_reason) = zone_reason {
                if self.cruise_mph.is_some() {
                    self.cancel_cruise(ctx, true);
                }
                // Quietly: the street's "Speed limit 25." follows at once,
                // and "Speed keeper holding 25" before it said the same
                // number twice (owner, 2026-10-25: the move onto city
                // streets said too much).
                self.engage_keeper(ctx, limit, &zone_reason, Some(limit), false);
            }
        }
        // The first corner, if the chain starts inside its own window: it is
        // spoken in THIS line, at this line's priority, or it is not heard at
        // all. Raised on its own a frame later it queued behind the
        // off-the-ramp announcement and the gate warning as a droppable lead,
        // went stale, and was dropped -- twice on one arrival, the second
        // time after the loop-back, so "Turn right onto West Main Avenue"
        // was never once spoken (owner, Spokane, 2026-08-22).
        let mut first_corner = String::new();
        if let Some(corner) = self.turn_cue_in_play() {
            let ahead = corner.at_mi - self.trip.position_mi;
            if ahead > 0.0 && ahead <= self.turn_window_mi() {
                let call = self.turn_approach_text(ctx, &corner, ahead);
                first_corner = format!(" Then {}", lower_first(&call));
                self.turn_advised.insert(corner.key.clone());
                self.pace_clock_for_turn(&corner, ahead);
                // Spoken inside this line, so its chime goes with it.
                self.chime_corner(ctx, &corner);
                if ahead <= crate::states::driving_turns::TURN_NOW_MI {
                    self.turn_called_now.insert(corner.key.clone());
                }
            }
        }
        first_corner
    }

    /// The streets from the end of a road stop's exit ramp to its lot, for
    /// this trip's direction, or None when the stop has none baked.
    pub fn stop_chain_route(&self, stop: &RoadStop) -> Option<Route> {
        if !self.stop_streets_on || stop.stop_type == "delivery_destination" {
            return None;
        }
        let highway = if self.stop_chain.is_some() {
            self.highway_trip.as_ref()?
        } else {
            &self.trip
        };
        highway
            .stop_approach_route(stop)
            .filter(|route| !route.legs.is_empty())
    }

    /// Off a road stop's exit ramp onto the streets to its lot.
    ///
    /// The same swap as a facility's chain: the highway trip is parked with
    /// its odometer at the exit, the streets carry their own limits, lights
    /// and signs, and the driveway is a turn into the lot. At the lot the
    /// highway comes back and the stop opens as it always has
    /// (`finish_stop_chain`).
    pub fn begin_stop_chain(&mut self, ctx: &mut GameContext, stop: &RoadStop) -> bool {
        if self.surface_chain || self.stop_chain.is_some() || self.departure_chain {
            return false;
        }
        let Some(route) = self.stop_chain_route(stop) else {
            return false;
        };
        let city = route.cities.first().cloned().unwrap_or_default();
        let local_state = self.city_state(ctx, &city);
        let first_corner = self.enter_streets(ctx, &route, local_state, true, true);
        self.stop_chain = Some(stop.clone());
        self.stop_chain_end_said = false;
        let street = Self::start_street_text(&route);
        ctx.audio.play_with("ui/notify", 0.7, 0.0);
        // In closing units: these streets are a few hundred feet to a mile,
        // and whole miles said "0 miles" for the lot.
        let message = format!(
            "Off the ramp. {street}.{first_corner} {} to {}.",
            self.closing_text(route.miles()),
            stop.spoken_name()
        );
        self.announce_streets(ctx, message, !first_corner.is_empty());
        true
    }

    /// At the end of a road stop's streets: stopped, the highway trip comes
    /// back and the stop opens; still rolling, the lot says where it is once.
    pub fn handle_stop_chain_end(&mut self, ctx: &mut GameContext) {
        let Some(stop) = self.stop_chain.clone() else {
            return;
        };
        if self.trip.truck.speed_mph() <= DOCKING_MAX_MPH {
            self.finish_stop_chain(ctx);
            self.open_poi_stop(ctx, &stop, true, None);
            return;
        }
        if self.cruise_mph.is_some() || self.keeper_mph.is_some() {
            // The lot is the stop: automatic speed control lets go here, as
            // it did at the end of a stop's ramp, and stays off until the
            // truck drives on (`resume_speed_control_if_ready`).
            self.pause_speed_control(ctx, true);
        }
        if !self.stop_chain_end_said {
            self.stop_chain_end_said = true;
            let place = stop.spoken_name();
            let message = if self.terse_speech(ctx) {
                format!("At {place}. Stop now.")
            } else {
                format!("At {place}. Come to a stop.")
            };
            let mut opts = SayEvent::new();
            opts.category = Some(SpeechCategory::Navigation);
            ctx.say_event_with(message, opts);
        }
    }

    /// Back onto the highway trip from a road stop's streets, with the clock,
    /// the toll ledger and the truck carried over: the stop and the ramp back
    /// up are the highway's, as they always were.
    pub fn finish_stop_chain(&mut self, _ctx: &mut GameContext) {
        if self.stop_chain.take().is_none() {
            return;
        }
        let Some(mut highway) = self.highway_trip.take() else {
            return;
        };
        highway.game_minutes = self.trip.game_minutes;
        highway.toll_charges = self.trip.toll_charges.clone();
        highway.hos_violation = self.trip.hos_violation;
        highway.truck = self.trip.truck.clone();
        let mut streets = self.replace_trip(highway);
        let weather = std::mem::replace(&mut streets.weather, self.placeholder_weather());
        self.trip.weather = weather;
        self.destination_arrival_active = false;
        self.destination_assist_brake = 0.0;
        self.reset_turn_state_for_trip();
    }

    /// The origin facility's street chain driven outbound, or None.
    ///
    /// Same bar as the arrival side: only a genuine multi-segment turn-level
    /// chain qualifies; other facilities keep the scripted departure straight
    /// onto the highway.
    pub fn departure_chain_route(&self, ctx: &GameContext) -> Option<Route> {
        if self.phase != DRIVE_PHASE_DELIVERY || !self.facility_streets_on {
            return None;
        }
        ctx.world
            .facility_departure_route(&self.job.origin, &self.job.origin_location)
            .ok()
            .flatten()
    }

    /// Start the loaded run on the origin facility's street chain.
    ///
    /// The full highway trip built at dispatch is parked aside; the truck
    /// pulls out of the gate onto real streets and the on-ramp merge hands the
    /// highway trip back with the clock and toll ledger intact.
    pub fn begin_departure_chain(&mut self, ctx: &mut GameContext, announce: bool) -> bool {
        if self.departure_chain || self.surface_chain {
            return false;
        }
        let Some(route) = self.departure_chain_route(ctx) else {
            return false;
        };
        let local_state = self.city_state(ctx, &self.job.origin.clone());
        let options = TripOptions {
            time_scale: self.trip.time_scale,
            seed: Some(self.trip_seed ^ 0xD00D),
            start_hour: self.trip.start_hour,
            imperial: self.trip.imperial(),
            hazard_scale: 0.0, // no random hazards on the first city miles
            career_hours: self.trip.career_hours,
            bobtail: self.trip.bobtail,
            local_state,
            // Driven outbound: the gate is the first thing behind you,
            // and this chain ends at the on-ramp.
            outbound: true,
            ..Default::default()
        };
        let merge_highway = self
            .trip
            .route
            .legs
            .first()
            .map(|leg| leg.highway.clone())
            .unwrap_or_default();
        let surface = Trip::new(
            route.clone(),
            self.trip.truck.clone(),
            self.placeholder_weather(),
            options,
        );
        let mut highway = self.replace_trip(surface);
        let weather = std::mem::replace(&mut highway.weather, self.placeholder_weather());
        self.trip.weather = weather;
        self.highway_trip = Some(highway);
        self.departure_chain = true;
        // Pulling out of the gate ENDS the stop that paused automatic speed
        // control. An arrival pause is deliberately never lifted by the resume
        // path -- only a departure clears one -- and nothing was clearing this
        // one, so the pause a driver earned by arriving to load survived being
        // loaded and followed them onto the road: armed, paused, and refusing
        // to engage for the rest of the run (Brandon, 2026-08-21). This is the
        // departure that clears it.
        self.clear_stop_pause();
        if announce {
            let first = &route.legs[0];
            let street = if first.local_cue.is_empty() {
                format!("Start on {}", first.highway)
            } else {
                first.local_cue.trim_end_matches('.').to_string()
            };
            let distance = self.trip.distance_text(route.miles());
            let mut opts = SayEvent::queued().priority(EventPriority::Important);
            opts.category = Some(SpeechCategory::Navigation);
            ctx.say_event_with(
                format!("{street}. {distance} to the {merge_highway} on-ramp."),
                opts,
            );
            self.mark_start_cue_said();
        }
        true
    }

    /// The briefing names the street the chain starts on, so the route's own
    /// start cue would say it a second time a moment later: "Start on 4th
    /// Avenue Southeast." straight after "...onto city streets. Start on 4th
    /// Avenue Southeast." (agent drives, Aberdeen and Abilene, 2026-09-23).
    fn mark_start_cue_said(&mut self) {
        for half in ["advance", "near"] {
            self.trip
                .announced_navigation
                .insert(format!("local:start:{half}"));
        }
    }

    /// End of the streets: up the on-ramp and onto the highway trip.
    pub fn finish_departure_chain(&mut self, ctx: &mut GameContext) {
        let game_minutes = self.trip.game_minutes;
        let toll_charges = self.trip.toll_charges.clone();
        let hos_violation = self.trip.hos_violation;
        let Some(mut highway) = self.highway_trip.take() else {
            return;
        };
        highway.game_minutes = game_minutes; // clock continuity
        highway.toll_charges = toll_charges; // settlement reads the live trip
        highway.hos_violation = hos_violation;
        highway.truck = self.trip.truck.clone();
        let mut surface = self.replace_trip(highway);
        let weather = std::mem::replace(&mut surface.weather, self.placeholder_weather());
        self.trip.weather = weather;
        self.departure_chain = false;
        // Coming up the ramp you are in the right lane, merging left.
        self.lane.recentre(0);
        let merge_highway = self
            .trip
            .route
            .legs
            .first()
            .map(|leg| leg.highway.clone())
            .unwrap_or_default();
        // The acceleration lane is a real stretch of road with a real length,
        // not a moment. Handing straight to the highway meant arriving at the
        // taper doing whatever the last corner left you at -- about 17 mph in
        // a measured run, which is the "came to a stop" a tester reported
        // (Brandon, 2026-08-21). Now the lane exists, sized from the highway
        // it feeds, and the truck has room to build speed on it.
        let (highway_mph, _) = self.trip.speed_limit_at(0.0);
        let grade = self.trip.grade_at(0.0);
        let lane_mi = acceleration_lane_mi(highway_mph, grade * 100.0);
        self.departure_ramp_mi = Some(lane_mi);
        self.departure_merge_road_mph = highway_mph;
        let capability_mph = acceleration_lane_capability_mph(&self.trip.truck, lane_mi, grade);
        let merge_target_mph = merge_traffic_target_mph(highway_mph).max(CRUISE_MIN_MPH);
        self.departure_cruise_handoff_mph =
            (capability_mph + 0.5 >= merge_target_mph).then_some(merge_target_mph);
        self.departure_merge_recovery = false;
        self.clock_pacing.reset_merge_recovery();
        // A real length of road is only room to build speed on if it is spent
        // at the rate a truck really covers it. The exit watch pins the lane
        // to the real clock every frame, but it has already run for this one,
        // and the shortest lane the table gives is 360 feet -- of which a
        // single compressed tick eats two dozen.
        self.trip.controlled_ramp = true;
        let lane_text = spoken_feet_or_meters(
            self.departure_ramp_mi.unwrap_or(0.0),
            ctx.settings.imperial_units,
        );
        // Hand the lane to the KEEPER explicitly, here, rather than leaving it
        // to the resume path to work out. The swap happens late in the frame,
        // so for one tick the new road looks like open highway with no zone on
        // it -- and the keeper duly handed off to cruise, which cannot hold
        // below its own minimum speed. The result was cruise nominally engaged
        // and nothing at all touching the throttle: the truck coasted the
        // entire acceleration lane at zero throttle (measured out of Aberdeen,
        // 2026-08-21). The keeper is the automation for this stretch and it
        // takes it directly.
        if ctx.settings.speed_keeper && self.speed_control_armed {
            if self.cruise_mph.is_some() {
                self.cancel_cruise(ctx, true);
            }
            self.engage_keeper(
                ctx,
                highway_mph,
                "acceleration lane",
                Some(highway_mph),
                false,
            );
        }
        ctx.audio.play_with("vehicle/signal_tone", 0.6, -0.6);
        let mut opts = SayEvent::queued().priority(EventPriority::Important);
        opts.category = Some(SpeechCategory::Navigation);
        ctx.say_event_with(
            format!("Up the ramp onto {merge_highway}. {lane_text} of acceleration lane."),
            opts,
        );
    }

    /// Run down the acceleration lane after pulling out of a facility.
    ///
    /// The lane is the one place a loaded truck is SUPPOSED to be slower than
    /// the road it is joining. The Green Book sizes these lanes for a
    /// passenger car -- 75 percent of highway speed is its own design target
    /// -- so a rig that reaches the taper under the limit has not failed at
    /// anything, and the game must not pretend otherwise. What a real driver
    /// does about it is take a bigger gap, which is what the closing line
    /// says.
    pub fn update_departure_ramp(&mut self, ctx: &mut GameContext, moved_mi: f64) {
        let Some(left) = self.departure_ramp_mi else {
            if self.departure_merge_recovery {
                // Recovered against the same road the warning named.
                let limit = if self.departure_merge_road_mph > 0.0 {
                    self.departure_merge_road_mph
                } else {
                    self.trip.speed_limit_at(self.trip.position_mi).0
                };
                if self.trip.truck.speed_mph() + 0.5 >= merge_traffic_target_mph(limit) {
                    self.end_departure_merge_recovery("merge speed reached");
                }
            }
            return;
        };
        let left = left - 0.0f64.max(moved_mi);
        self.departure_ramp_mi = Some(left);
        if left > 0.0 {
            return;
        }
        self.departure_ramp_mi = None;
        self.departure_cruise_handoff_mph = None;
        let position = self.trip.position_mi;
        // The road being JOINED, not the zone under the wheels. Read at the
        // taper, a ramp posted below its own highway made the game measure
        // the truck against the ramp and call a slow join "up to speed": out
        // of Ardmore Company Yard the truck reached the taper 14 miles an
        // hour under the road and heard the plain "Lane ending. Merge left."
        // (departure sweep, 2026-09-20). The number a merging driver needs is
        // the speed of the traffic they are merging with.
        let limit = if self.departure_merge_road_mph > 0.0 {
            self.departure_merge_road_mph
        } else {
            self.trip.speed_limit_at(position).0
        };
        let speed = self.trip.truck.speed_mph();
        // Under the limit by enough to matter is the NORMAL outcome for a
        // loaded truck, so it is said as a fact about the gap you need, never
        // as a fault. Only a truck that is genuinely up to speed gets the
        // plain merge line.
        let message = if speed + 0.5 < merge_traffic_target_mph(limit) {
            // The length just consumed remains the map-derived fallback; do
            // not invent more Carlisle pavement for a truck that needs more
            // room. It is now on the mainline's right lane, where adaptive
            // cruise still protects its following gap. Keeping this handoff
            // on the real clock until it is close to traffic speed prevents
            // time compression from turning a slow, loaded join into a
            // sudden highway-speed transition.
            self.start_departure_merge_recovery(speed, limit);
            format!(
                "Lane ending at {}, traffic at {}. Take a big gap.",
                ctx.settings.speed_text(speed),
                ctx.settings.speed_text(limit)
            )
        } else if self.lane_count_here() > 1 {
            // Said as done, because it is: the truck is already in the
            // mainline's right lane here. "Merge left" asked for a move the
            // game had made, and a driver who obeyed it went on into the
            // passing lane -- or, on a one-lane road, heard "No lane to your
            // left here" (agent drives out of Aberdeen, 2026-09-23).
            "Lane ended. In the right lane.".to_string()
        } else {
            "Lane ended.".to_string()
        };
        ctx.audio.play_with("vehicle/signal_tone", 0.6, -0.6);
        let mut opts = SayEvent::queued().priority(EventPriority::Important);
        opts.category = Some(SpeechCategory::Navigation);
        ctx.say_event_with(message, opts);
    }
}

/// Python's `text[:1].lower() + text[1:]`.
fn lower_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        None => String::new(),
        Some(first) => first.to_lowercase().collect::<String>() + chars.as_str(),
    }
}
