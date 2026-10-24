//! What meets you where a ramp joins the surface road: the light or the sign,
//! the cross-traffic bubble, the stop bar's countdown and its held tone, the
//! route-transition assist, and the crossing itself.

use crate::speech::EventPriority;
use ff_core::data::world_models::Interchange;
use ff_core::pyrandom::PyRandom;
use ff_core::sim::cross_traffic::{CrossTraffic, CrossVehicle};
use ff_core::sim::trip_models::RoadStop;
use ff_core::sim::trip_route_helpers::INTERCHANGE_IDENTITY_MI;
use ff_core::speech_pacing::SpeechCategory;
use ff_core::units::spoken_feet_or_meters;

use crate::app::{GameContext, IntoSpoken, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use crate::states::driving_events::street_controls::StreetLightPlan;
use crate::states::driving_stops::{bar_solid_zone_mi, bar_tick_range_mi};
use crate::states::driving_updates::live;

/// What a terminal violation met: a vehicle in the conflict window, one
/// arriving within a horn's length, or an empty crossroad.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CrossMeeting {
    Hit,
    Near,
    Empty,
}

impl DrivingState {
    /// The control at this stop's ramp end, decidable any time.
    ///
    /// Baked OSM data (a traffic_signals or stop node on the exit's ramp
    /// links) wins; otherwise a seeded urban/rural heuristic stands in --
    /// most urban diamond terminals are signalized, rural ones lean to stop
    /// signs, and a share flow free like a cloverleaf loop. Pure function of
    /// the trip seed, the stop, and baked data, so the signal-on announcement
    /// a mile out and the ramp itself always agree.
    pub fn ramp_control_for(
        &self,
        _ctx: &GameContext,
        stop: &RoadStop,
        rng: Option<&mut PyRandom>,
    ) -> String {
        if stop.stop_type == "weigh_station" {
            // A scale has its own deceleration ramp flowing straight into
            // the inspection lane -- no public crossroad, no light, no stop
            // sign. The scale bar itself is the terminal, and the arrival
            // stop machinery already owns it ("At the scale. Stop now").
            // The dice used to put a stop sign here, spoken with the
            // MAINLINE's limit on its far side (owner playtest, 2026-08-20,
            // "Stop sign at ramp end. Limit 70").
            return "none".to_string();
        }
        if !self.facility_streets_on {
            // Facility streets off is the 1.8 road: no ramp ends at a light
            // or a sign. The facility's ramp runs into the facility (owner,
            // 2026-10-11), and every other exit runs into its stop or back
            // onto the road (owner, 2026-10-24: "When off, players should
            // not encounter lights or stop signs").
            return "none".to_string();
        }
        // A stop matched to its interchange at bake time reads that record
        // and no other. The mile-marker search is only for a stop with no
        // match: a stop's mile is a projection, a mile or three off as often
        // as not, so the search found a recorded control for one ramp in
        // thirty and the dice spoke for the rest as if they were the ramp's.
        let mut control = match self.served_interchange(stop) {
            Some(interchange) => interchange.ramp_control.clone(),
            None => self.trip.ramp_control_at(stop.at_mi, 0.15),
        };
        if control.is_empty() && self.ramp_meets_a_freeway(stop) {
            // A system interchange: this ramp ends in a merge onto another
            // freeway, and nothing stops traffic there. Decided before the
            // dice rather than by them -- see FREEWAY_VIA_RE.
            control = "none".to_string();
        }
        if control.is_empty() {
            let mut owned;
            let rng = match rng {
                Some(rng) => rng,
                None => {
                    owned = self.ramp_rng(stop);
                    &mut owned
                }
            };
            let (signal_w, stop_w) = if self.trip.near_city(stop.at_mi) {
                RAMP_CONTROL_URBAN_WEIGHTS
            } else {
                RAMP_CONTROL_RURAL_WEIGHTS
            };
            let roll = rng.random();
            control = if roll < signal_w {
                "signal".to_string()
            } else if roll < stop_w {
                "stop".to_string()
            } else {
                "none".to_string()
            };
        }
        control
    }

    /// The dice for a ramp with no recorded control, seeded by the EXIT, not
    /// by the stop: two stops off one exit are one ramp. Seeded by each stop's
    /// own mile, exit 286A into Abilene came out a stop sign for the delivery
    /// and a traffic light for the truck stop 0.1 mile on, and the driver
    /// heard both (agent drive, 2026-09-23).
    fn ramp_rng(&self, stop: &RoadStop) -> PyRandom {
        // Both in the route's frame: the record's own mile is leg-local, and
        // the destination (which never carries `interchange_mi`) seeded off
        // it while the truck stop on the same exit seeded off the route mile
        // (2026-09-28).
        let exit_mi = stop
            .interchange_mi
            .or_else(|| self.trip.interchange_mile_at(stop.at_mi, 0.15))
            .unwrap_or(stop.at_mi);
        PyRandom::new_from_i64((self.trip_seed << 16) ^ (exit_mi * 100.0) as i64)
    }

    /// The interchange record this stop was matched to at bake time, by
    /// identity: the stop carries the record's own route mile.
    fn served_interchange(&self, stop: &RoadStop) -> Option<&Interchange> {
        self.trip
            .interchange_at(stop.interchange_mi?, INTERCHANGE_IDENTITY_MI)
    }

    /// Whether this exit's ramp lands on another freeway.
    ///
    /// The baked `ramp_far_end` answers first: it is walked link topology, a
    /// fact about the road the ramp reaches. `surface` in particular
    /// SUPPRESSES the `via` guess below -- via is signage (where the exit
    /// points), not the road the ramp lands on, and measured against walked
    /// topology the signage guess called a controlled surface terminal "free
    /// flow" on about a third of the exits it fired on.
    ///
    /// The via fallback survives for exits the walk could not judge: 4,999 of
    /// the world's 18,011 exits lead to an interstate and every one of them
    /// used to take its chances with the urban/rural weights below, which
    /// handed stop signs to roughly half the rural ones -- a stop sign where
    /// an interstate meets an interstate does not exist (owner, 2026-08-17).
    pub fn ramp_meets_a_freeway(&self, stop: &RoadStop) -> bool {
        let interchange = self
            .served_interchange(stop)
            .or_else(|| self.trip.interchange_at(stop.at_mi, 0.15));
        let Some(interchange) = interchange else {
            return false;
        };
        if interchange.ramp_far_end == "motorway" {
            return true;
        }
        if interchange.ramp_far_end == "surface" {
            return false;
        }
        freeway_via_matches(&interchange.via.to_uppercase())
    }

    /// Set up the terminal control state for the ramp just taken.
    pub fn begin_ramp_terminal(&mut self, ctx: &GameContext, stop: &RoadStop) {
        let mut rng = self.ramp_rng(stop);
        let control = self.ramp_control_for(ctx, stop, Some(&mut rng));
        let timing_key = self.ramp_terminal_timing_key(stop);
        self.begin_terminal_control(control, &mut rng, &timing_key, stop.at_mi, true, None);
    }

    /// Set up the control state for a terminal about to be met: a ramp's
    /// end, or a light or sign on a facility's streets
    /// (`driving_events/street_controls.rs`). `rng` is the control's own
    /// seeded stream, `timing_key` fixes its light's timing plan, and
    /// `at_mi` seeds its crossroad; `cross_traffic` false builds no crossroad
    /// (an all-way stop, where the cross street stops too).
    pub(crate) fn begin_terminal_control(
        &mut self,
        control: String,
        rng: &mut PyRandom,
        timing_key: &str,
        at_mi: f64,
        cross_traffic: bool,
        street_light: Option<StreetLightPlan>,
    ) {
        self.ramp_control = control;
        let mut profile_rng = PyRandom::new_from_str(timing_key);
        self.ramp_light_profile = profile_rng.randrange(RAMP_LIGHT_PROFILE_COUNT) as u8;
        self.ramp_light_timer = 0.0;
        self.street_light_split = street_light.map(|plan| (plan.red_s, plan.green_s));
        let random_offset = rng.random() * self.ramp_light_cycle_s();
        self.ramp_light_offset_s = street_light.map_or(random_offset, |plan| plan.offset_s);
        self.ramp_light_announced = false;
        self.ramp_light_last_phase = String::new();
        self.ramp_terminal_done = self.ramp_control == "none";
        self.ramp_waiting_at_light = false;
        self.ramp_creep_prompt_said = false;
        self.ramp_creep_prompt_gap_mi = 0.0;
        self.ramp_gap_milestones_said.clear();
        self.ramp_bar_tick_timer = 0.0;
        self.ramp_assist_said = false;
        self.ramp_green_roll_said = false;
        self.ramp_assist_brake = 0.0;
        self.ramp_waiting_at_sign = false;
        self.approach_pull_ahead = false;
        self.approach_pull_ahead_canceled = false;
        // The cross bubble: a controlled terminal means a real crossroad, so
        // simulate it. Seeded like the control itself so the same terminal
        // always carries the same traffic day; the near-city split reuses the
        // same urban/rural judgment the control dice already trust.
        self.cross_bubble = if cross_traffic
            && matches!(
                self.ramp_control.as_str(),
                "signal" | "stop" | "yield" | "roundabout"
            ) {
            // A roundabout entry is gap acceptance against circulating
            // traffic: yield rates, spoken as a roundabout.
            let control = if self.ramp_control == "roundabout" {
                "yield"
            } else {
                self.ramp_control.as_str()
            };
            let mut bubble = CrossTraffic::new(
                (self.trip_seed << 16) ^ (at_mi * 100.0) as i64 ^ 0x5AFE,
                control,
                self.trip.near_city(at_mi),
            );
            if self.ramp_control == "signal" && self.ramp_light_holds_cross_traffic() {
                // The pre-roll above begins with the cross street flowing. If
                // arrival lands in the player's green, yellow, or clearance,
                // replay enough stopped-cross-street time for anything already
                // past its bar to leave before that first phase is announced.
                bubble.player_has_green = true;
                for _ in 0..(RAMP_LIGHT_RED_CLEARANCE_S / 0.25) as usize {
                    bubble.update(0.25);
                }
            }
            Some(bubble)
        } else {
            None
        };
    }

    /// Stable identity for the timing profile, independent of a drive's seed.
    fn ramp_terminal_timing_key(&self, stop: &RoadStop) -> String {
        let mut leg_start_mi = 0.0;
        for leg in &self.trip.route.legs {
            if stop.at_mi <= leg_start_mi + leg.miles + 0.001 {
                return format!(
                    "ramp-light|{}|{}|{}|{:.3}|{}|{}|{}",
                    leg.a,
                    leg.b,
                    leg.highway,
                    stop.at_mi - leg_start_mi,
                    stop.name,
                    stop.stop_type,
                    stop.exit_label,
                );
            }
            leg_start_mi += leg.miles;
        }
        format!(
            "ramp-light|{}|{}|{}|{}",
            self.trip.route.cities.join(">"),
            stop.key(),
            stop.stop_type,
            stop.exit_label,
        )
    }

    /// This terminal's fixed red interval in real seconds (a street signal's
    /// own split, `street_controls.rs`).
    pub fn ramp_light_red_s(&self) -> f64 {
        if let Some((red_s, _)) = self.street_light_split.filter(|_| self.on_street_control()) {
            return red_s;
        }
        RAMP_LIGHT_RED_S + f64::from(self.ramp_light_profile) * RAMP_LIGHT_RED_STEP_S
    }

    /// This terminal's fixed green interval in real seconds.
    pub fn ramp_light_green_s(&self) -> f64 {
        if let Some((_, green_s)) = self.street_light_split.filter(|_| self.on_street_control()) {
            return green_s;
        }
        RAMP_LIGHT_GREEN_S + f64::from(self.ramp_light_profile) * RAMP_LIGHT_GREEN_STEP_S
    }

    /// One complete fixed timing plan in real seconds.
    pub fn ramp_light_cycle_s(&self) -> f64 {
        self.ramp_light_red_s() + self.ramp_light_green_s() + RAMP_LIGHT_YELLOW_S
    }

    fn ramp_light_into_cycle_s(&self) -> f64 {
        (self.ramp_light_offset_s + self.ramp_light_timer).rem_euclid(self.ramp_light_cycle_s())
    }

    /// Whether the cross street is held at its bar. The last part of the
    /// player's red is an internal all-red clearance; it is not a fourth
    /// player-facing light phase and therefore needs no extra spoken change.
    fn ramp_light_holds_cross_traffic(&self) -> bool {
        self.ramp_light_into_cycle_s() >= self.ramp_light_red_s() - RAMP_LIGHT_RED_CLEARANCE_S
    }

    /// `_ramp_light_phase()`.
    pub fn ramp_light_phase(&self) -> &'static str {
        let into = self.ramp_light_into_cycle_s();
        let red_s = self.ramp_light_red_s();
        if into < red_s {
            return "red";
        }
        if into < red_s + self.ramp_light_green_s() {
            return "green";
        }
        "yellow"
    }

    /// Only true red punishes a crossing: entering on yellow is legal,
    /// exactly like the real law.
    pub fn ramp_light_is_red(&self) -> bool {
        self.ramp_light_phase() == "red"
    }

    /// Advance the terminal light in real time and speak state changes.
    pub fn update_ramp_light(&mut self, ctx: &mut GameContext, dt: f64) {
        // The bar's cues run first and unconditionally. They are the only code
        // that stops the solid tone, and every early return below used to skip
        // them: a driver who reached the tone and then crossed the bar --
        // green, red, or stop sign -- carried it through the rest of the run
        // and out into the menus (Shane, 2026-08-03).
        self.update_ramp_bar_ticks(ctx, dt);
        if self.terminal_live() && !self.ramp_terminal_done && self.ramp_control == "signal" {
            // A street signal runs on the trip's own clock, the one its
            // coordination is planned on (`street_controls.rs`); a ramp end
            // on real seconds, the way it always has.
            self.ramp_light_timer += if self.on_street_control() {
                dt * self.trip.effective_time_scale()
            } else {
                dt
            };
        }
        self.update_cross_bubble(dt);
        if !self.terminal_live() || self.ramp_terminal_done {
            return;
        }
        if matches!(self.ramp_control.as_str(), "stop" | "yield" | "roundabout") {
            // A sign has no phases, but its bar needs a position just
            // as much as a light's: without the countdown, the ticks, and
            // the stopped-short guidance, the sign was one announce line
            // and then silence until the damage message (playtest
            // 2026-07-22, Milwaukee grain elevator, 15 percent).
            self.update_ramp_queue_guidance(ctx);
            self.update_ramp_gap_countdown(ctx);
            return;
        }
        if self.ramp_control != "signal" {
            return;
        }
        self.update_ramp_queue_guidance(ctx);
        self.update_ramp_gap_countdown(ctx);
        let phase = self.ramp_light_phase();
        if !self.ramp_light_announced || phase == self.ramp_light_last_phase {
            return;
        }
        self.ramp_light_last_phase = phase.to_string();
        if self.ramp_waiting_at_light && phase == "green" {
            // The wait at the stop bar ends; the driveway is just ahead.
            self.ramp_waiting_at_light = false;
            self.ramp_terminal_done = true;
            if self.approach_pull_ahead_available(ctx) {
                self.approach_pull_ahead = true;
            }
            ctx.audio.play_with("events/ramp_light_green", 0.8, 0.0);
            self.say_route_navigation(ctx, "Light green.");
            return;
        }
        // Keep every phase change on the route channel so the driver hears
        // it promptly, with only the color in the cycling announcement.
        if phase == "red" {
            ctx.audio.play_with("events/ramp_light_red", 0.7, 0.0);
            self.say_route_navigation(ctx, "Light red.");
        } else if phase == "yellow" {
            ctx.audio.play_with("ui/notify", 0.7, 0.0);
            self.say_route_navigation(ctx, "Light yellow.");
        } else {
            ctx.audio.play_with("events/ramp_light_green", 0.7, 0.0);
            self.say_route_navigation(ctx, "Light green.");
        }
    }

    /// Which distances to the stop bar are worth SAYING on this rung.
    ///
    /// The bar already has a non-spoken instrument: inside
    /// `RAMP_BAR_TICK_RANGE_MI` a centre tick speeds up as the bar closes,
    /// and fuses to a solid tone at the end. Rate carries distance, silence
    /// means stopped. So a spoken milestone inside that range is speech
    /// restating what the driver is already listening to -- four calls on
    /// every ramp terminal, of which the last two were audible twice.
    ///
    /// Standard keeps the calls the tick cannot make, the ones out beyond its
    /// range. Quiet keeps one: the rung means less automatic speech, the
    /// terminal callout has already named the light or the sign, and the tick
    /// does the rest of the work (owner, 2026-08-21).
    pub fn ramp_bar_milestones(&self, ctx: &GameContext) -> Vec<i64> {
        let imperial = ctx.settings.imperial_units;
        let thresholds: &[i64] = if imperial {
            &RAMP_GAP_MILESTONES_FT
        } else {
            &RAMP_GAP_MILESTONES_M
        };
        let unit_mi = if imperial {
            1.0 / 5280.0
        } else {
            1.0 / 1609.344
        };
        let mut outside_tick: Vec<i64> = thresholds
            .iter()
            .copied()
            .filter(|threshold| *threshold as f64 * unit_mi > RAMP_BAR_TICK_RANGE_MI)
            .collect();
        // Never silent: a unit system whose milestones all sit inside the tick
        // range still gets its farthest call, so the bar is never announced by
        // sound alone to a driver who has the tick turned down.
        if outside_tick.is_empty() {
            outside_tick = thresholds[..1].to_vec();
        }
        if self.terse_speech(ctx) {
            // Quiet gets the far call and the HANDOFF call -- the one at the
            // distance where the tick starts, so the words hand the driver to
            // the sound rather than simply stopping (owner, after driving it,
            // 2026-08-21: "leave 300 in because that's when the stop bar beeps
            // come in, so the sound will do the guiding at that point"). In
            // feet that is 300 exactly; in metres the nearest milestone to the
            // same physical distance.
            let handoff = thresholds
                .iter()
                .copied()
                .min_by(|a, b| {
                    ((*a as f64 * unit_mi) - RAMP_BAR_TICK_RANGE_MI)
                        .abs()
                        .total_cmp(&(((*b as f64 * unit_mi) - RAMP_BAR_TICK_RANGE_MI).abs()))
                })
                .expect("the milestone table is never empty");
            let far = outside_tick[0];
            return if handoff == far {
                vec![far]
            } else {
                vec![far, handoff]
            };
        }
        // Two is the owner's number (2026-08-21), and it makes both unit
        // systems behave alike: the tick rule alone left metric with a third
        // call at 100 metres that imperial had no equivalent for.
        outside_tick.into_iter().take(2).collect()
    }

    /// Run the crossroad's own traffic while the terminal is live.
    ///
    /// Real seconds, like the light: the terminal already stops the clock
    /// compressing, and a gap that shrank at 4x would be unreadable. What the
    /// driver hears of it is the traffic sounds
    /// (`driving_traffic_sounds.rs`): each crossing vehicle is panned and
    /// levelled from where it is every frame, so the gap IS the audio.
    pub fn update_cross_bubble(&mut self, dt: f64) {
        if self.cross_bubble.is_none() {
            return;
        }
        if !self.terminal_live() || self.ramp_terminal_done {
            // The terminal released the driver; the crossroad is behind them.
            self.cross_bubble = None;
            return;
        }
        if self.ramp_control == "signal" {
            // The cross street runs the orthogonal phase. It is held through
            // the player's green and yellow, plus the final red-clearance
            // interval before green. That shared red is what lets traffic
            // already past its bar clear the conflict point.
            let green = self.ramp_light_holds_cross_traffic();
            if let Some(bubble) = self.cross_bubble.as_mut() {
                bubble.player_has_green = green;
            }
        }
        if let Some(bubble) = self.cross_bubble.as_mut() {
            bubble.update(dt);
        }
    }

    /// What a terminal violation met, and the vehicle it met.
    ///
    /// With no bubble to consult (older saves mid-ramp) this used to answer
    /// with the old certainty -- the violation hits -- which is exactly the
    /// guaranteed clip the owner's 2026-07-15 playtest called backwards.
    /// Now the crossroad is rolled on the spot instead: the same seeded
    /// traffic day `begin_ramp_terminal` would have built, asked the same
    /// question, so a blown light meets whatever that road carries.
    pub fn cross_violation_meets(&mut self) -> (CrossMeeting, Option<CrossVehicle>) {
        if self.cross_bubble.is_none() {
            let at_mi = self.terminal_seed_mi();
            let control = match self.ramp_control.as_str() {
                "roundabout" => "yield",
                "signal" | "stop" | "yield" => self.ramp_control.as_str(),
                _ => return (CrossMeeting::Empty, None),
            };
            self.cross_bubble = Some(CrossTraffic::new(
                (self.trip_seed << 16) ^ (at_mi * 100.0) as i64 ^ 0x5AFE,
                control,
                self.trip.near_city(at_mi),
            ));
        }
        let Some(bubble) = self.cross_bubble.as_ref() else {
            return (CrossMeeting::Empty, None);
        };
        if let Some(vehicle) = bubble.occupant() {
            return (CrossMeeting::Hit, Some(vehicle.clone()));
        }
        if let Some(vehicle) = bubble.approaching(2.0) {
            return (CrossMeeting::Near, Some(vehicle.clone()));
        }
        (CrossMeeting::Empty, None)
    }

    /// How hard a blown terminal hits, by what actually arrived: a semi or
    /// a bus broadsides the trailer, a car clips it.
    pub fn cross_hit_severity(base: f64, vehicle: Option<&CrossVehicle>) -> f64 {
        match vehicle {
            Some(vehicle) if HEAVY_CROSS_CLASSES.contains(&vehicle.vehicle_class) => {
                base * HEAVY_CROSS_HIT_MULTIPLIER
            }
            _ => base,
        }
    }

    /// "cross traffic clipped the trailer" or "a semi hit the trailer
    /// broadside": the collision clause for the vehicle a violation met.
    pub fn cross_hit_clause(vehicle: Option<&CrossVehicle>) -> String {
        match vehicle {
            Some(vehicle) if HEAVY_CROSS_CLASSES.contains(&vehicle.vehicle_class) => {
                format!("a {} hit the trailer broadside", vehicle.vehicle_class)
            }
            _ => "cross traffic clipped the trailer".to_string(),
        }
    }

    /// The crossing cue for the vehicle a violation met.
    pub fn cross_vehicle_sound(vehicle: Option<&CrossVehicle>) -> String {
        match vehicle {
            None => "traffic/car_cross".to_string(),
            Some(vehicle) => format!("traffic/{}_cross", vehicle.vehicle_class.replace(' ', "_")),
        }
    }

    /// Tell a driver stopped short of the stop bar to close the gap.
    ///
    /// A cautious stop on the first "brake to a stop" callout can land a
    /// quarter mile short of the bar, where one green is never enough road
    /// from a standstill. Without this prompt that plays as a light stuck in
    /// an endless loop (playtest transcript, 2026-07-16).
    pub fn update_ramp_queue_guidance(&mut self, ctx: &mut GameContext) {
        if !self.ramp_light_announced || self.ramp_waiting_at_light {
            return;
        }
        let Some(gap_mi) = self.terminal_gap_mi() else {
            return;
        };
        if gap_mi <= 0.0 || self.stopped_at_the_bar() {
            // At the bar, or stopped where the held tone said to be stopped:
            // the terminal's own "Stopped at the sign" owns that stop.
            return;
        }
        if self.trip.truck.speed_mph() > RED_STOP_MPH {
            return;
        }
        // Once per stop. A truck braking to a crawl bobs across the stopped
        // line more than once, and each crossing re-armed this: "Stopped
        // short of the stop sign." twice for one stop (agent drive B,
        // 2026-09-24). A new stop has to be somewhere new.
        if self.ramp_creep_prompt_said
            && self.ramp_creep_prompt_gap_mi - gap_mi < RAMP_CREEP_REARM_MI
        {
            return;
        }
        if ctx.settings.route_transition_assist && gap_mi <= RAMP_ASSIST_HOLD_MI {
            // Inside the hold window the assist owns the stop and says so
            // itself. This runs first in the frame, so it used to say
            // "Stopped short of the light" around the assist's own "Stopped
            // at the red light" (agent drive, 2026-09-22).
            return;
        }
        self.ramp_creep_prompt_said = true;
        self.ramp_creep_prompt_gap_mi = gap_mi;
        // Always with the distance. Under two hundred feet it used to go
        // unsaid, and the driver had to press S to learn "about 50 feet"
        // (agent drive B, 2026-09-24).
        let gap = self.short_distance_text(ctx, gap_mi);
        if matches!(self.ramp_control.as_str(), "stop" | "yield" | "roundabout") {
            let noun = match self.ramp_control.as_str() {
                "stop" => "the stop sign",
                "yield" => "the yield line",
                _ => "the roundabout entry",
            };
            let message = format!("Stopped {gap} short of {noun}.");
            // ROUTE, not the ambient default. This is an instruction about a
            // STANDING condition -- the truck is stopped short of the bar and
            // stays stopped until the driver acts -- so the staleness rule that
            // drops a line "starting after the moment it described" is reading
            // a moment that has not passed. It dropped exactly this line in the
            // owner playtest of 2026-08-17, leaving the truck 1,350 feet short
            // through a whole green-yellow-red cycle with nothing said; the same
            // failure the comment below already records from 2026-07-19. ROUTE
            // waits its turn behind anything urgent, and is never dropped.
            self.say_route_navigation(ctx, &message);
            return;
        }
        let message = if self.ramp_light_phase() == "green" {
            format!("Stopped {gap} short of the light. It is green.")
        } else {
            format!("Stopped {gap} short of the light.")
        };
        // ROUTE, not the ambient default. This is an instruction about a
        // STANDING condition -- the truck is stopped short of the bar and
        // stays stopped until the driver acts -- so the staleness rule that
        // drops a line "starting after the moment it described" is reading
        // a moment that has not passed. It dropped exactly this line in the
        // owner playtest of 2026-08-17, leaving the truck 1,350 feet short
        // through a whole green-yellow-red cycle with nothing said; the same
        // failure the comment below already records from 2026-07-19. ROUTE
        // waits its turn behind anything urgent, and is never dropped.
        self.say_route_navigation(ctx, &message);
    }

    /// Stopped where the bar's held tone told the driver to be stopped, at a
    /// control that asks for a stop: that is a stop at the bar.
    ///
    /// The tone starts at the solid zone (about sixty feet out, the owner's
    /// spec), and the terminal only counted a stop at zero feet. So a driver
    /// who did exactly what the tone asked was told "Stopped short of the
    /// stop sign." with the sign fifty feet away (agent drive B, 2026-09-24).
    /// Route-transition assistance already holds anywhere inside sixty feet
    /// (`RAMP_ASSIST_HOLD_MI`); this gives the driver's own stop the same
    /// line.
    pub(crate) fn stopped_at_the_bar(&self) -> bool {
        let Some(gap_mi) = self.terminal_gap_mi() else {
            return false;
        };
        let must_stop = match self.ramp_control.as_str() {
            "stop" | "yield" | "roundabout" => true,
            "signal" => self.ramp_light_is_red(),
            _ => false,
        };
        must_stop
            && self.ramp_light_announced
            && !self.ramp_terminal_done
            && self.trip.truck.speed_mph() <= RED_STOP_MPH
            && gap_mi <= bar_solid_zone_mi(&self.trip.truck)
    }

    /// Count the stop bar down while the truck is rolling toward it.
    ///
    /// The stopped-driver prompt above names the gap only at a standstill, so
    /// a rolling driver had no idea where the bar was: the owner crept 1300
    /// feet in stop-and-listen hops across three light cycles (playtest log,
    /// 2026-07-19). Rolling milestone calls give the bar a position the same
    /// way the exit countdown gives the exit one.
    pub fn update_ramp_gap_countdown(&mut self, ctx: &mut GameContext) {
        if !self.ramp_light_announced || self.ramp_waiting_at_light || !self.bar_cues_owed() {
            return;
        }
        let Some(gap_mi) = self.terminal_gap_mi() else {
            return;
        };
        if gap_mi <= 0.0 {
            return;
        }
        if self.trip.truck.speed_mph() <= RED_STOP_MPH {
            return;
        }
        let thresholds = self.ramp_bar_milestones(ctx);
        let imperial = ctx.settings.imperial_units;
        let unit_mi = if imperial {
            1.0 / 5280.0
        } else {
            1.0 / 1609.344
        };
        let unit_word = if imperial { "feet" } else { "meters" };
        for threshold in thresholds {
            if gap_mi <= threshold as f64 * unit_mi
                && !self.ramp_gap_milestones_said.contains(&threshold)
            {
                self.ramp_gap_milestones_said.insert(threshold);
                self.say_route_navigation(ctx, &format!("{threshold} {unit_word}."));
                return;
            }
        }
    }

    /// The continuous tone of the bar's final zone.
    ///
    /// Held, not started: the tone is re-asserted on every tick it applies to
    /// and lapses on its own as soon as it is not, so it cannot survive this
    /// state losing the frame to a menu or an arrival screen. Turning it back
    /// off here is still instant.
    pub fn set_bar_solid(&mut self, ctx: &mut GameContext, on: bool) {
        if on {
            // 0.85 read as jarring against everything else on the road
            // (Darren, 2026-08-15): a continuous tone at nearly full scale
            // sits far louder than the intermittent cues around it, and this
            // one plays while the driver is concentrating on stopping. The
            // tone still has to be unmistakable, so it stays the loudest
            // continuous cue -- just no longer the loudest thing in the cab.
            ctx.audio
                .hold_alert_with("vehicle/bar_solid", BAR_SOLID_VOLUME, 60);
        } else if self.bar_solid_on {
            ctx.audio.release_alert();
        }
        self.bar_solid_on = on;
    }

    /// Parking-sensor tick for the stop bar's last few hundred feet.
    ///
    /// Rate carries the distance -- faster is closer -- and silence means
    /// stopped, so the cue never nags a driver holding at the bar. Center pan,
    /// unlike the side-panned curve cues, so the two never read as the same
    /// instrument (owner ask, 2026-07-19). Inside the last stretch of leeway,
    /// still moving, the ticks fuse into a continuous tone (owner spec,
    /// written into the manual 2026-07-27): at the solid tone you had better
    /// be close to stopped.
    pub fn update_ramp_bar_ticks(&mut self, ctx: &mut GameContext, dt: f64) {
        if !self.ramp_light_announced || self.ramp_waiting_at_light || !self.bar_cues_owed() {
            self.set_bar_solid(ctx, false);
            return;
        }
        let Some(gap_mi) = self.terminal_gap_mi() else {
            self.set_bar_solid(ctx, false);
            return;
        };
        if self.ramp_terminal_done {
            self.set_bar_solid(ctx, false);
            return;
        }
        if self.trip.truck.speed_mph() <= RED_STOP_MPH {
            self.set_bar_solid(ctx, false);
            return;
        }
        if self.ramp_control == "signal" && self.ramp_light_phase() == "green" {
            // A green is not a stop. The ticks and the held "you must be
            // stopping" tone played all the way to the bar after "Light
            // green.", so a driver without the assist was told by sound to
            // stop on a green (agent drives A, D and E, 2026-09-24). They come
            // back if the light turns while the truck is still short of it.
            self.set_bar_solid(ctx, false);
            return;
        }
        // Both distances come from what this truck can actually stop in, with
        // the old constants as their floors: a load that stops longer -- hot
        // brakes, ice, a downgrade, liquid running forward in a tank -- hears
        // the bar earlier, because it needs the road earlier.
        let tick_range_mi = bar_tick_range_mi(&self.trip.truck);
        let solid_mi = bar_solid_zone_mi(&self.trip.truck);
        if gap_mi > tick_range_mi || gap_mi < 0.0 {
            self.set_bar_solid(ctx, false);
            return;
        }
        if gap_mi <= solid_mi {
            self.set_bar_solid(ctx, true);
            return;
        }
        self.set_bar_solid(ctx, false);
        let closeness = 1.0 - gap_mi / tick_range_mi;
        let period =
            RAMP_BAR_TICK_SLOW_S - closeness * (RAMP_BAR_TICK_SLOW_S - RAMP_BAR_TICK_FAST_S);
        self.ramp_bar_tick_timer += dt;
        if self.ramp_bar_tick_timer >= period {
            self.ramp_bar_tick_timer = 0.0;
            // Full volume: at 0.5 the owner judged it missable by someone
            // not listening for it (2026-07-19). The dedicated beep the old
            // note asked for arrived with the curve bink (2026-07-27).
            ctx.audio.play_with("vehicle/curve_bink", 0.9, 0.0);
        }
    }

    /// Light phase and bar distance on demand, for the info keys.
    ///
    /// "Stop at the bar" is only an instruction if the bar has a position; a
    /// sighted driver reads it off the windshield, so speech must answer the
    /// same question whenever the driver asks (owner ask, 2026-07-19).
    pub fn ramp_light_query_text(&mut self, ctx: &GameContext) -> Option<String> {
        let gap_mi = self.terminal_gap_mi()?;
        if !matches!(self.ramp_control.as_str(), "signal" | "stop") || self.ramp_terminal_done {
            return None;
        }
        if self.ramp_control == "stop" {
            if gap_mi <= 0.0 {
                return Some("At the stop bar. Stop sign.".to_string());
            }
            let limit_text = self.approach_limit_text(ctx);
            let limit_clause = if limit_text.is_empty() {
                String::new()
            } else {
                format!(", speed limit {limit_text}")
            };
            return Some(format!(
                "Stop sign, about {} to the stop bar{limit_clause}.",
                self.short_distance_text(ctx, gap_mi)
            ));
        }
        let phase = self.ramp_light_phase();
        if gap_mi <= 0.0 {
            return Some(format!("At the stop bar. Light {phase}."));
        }
        let limit_text = self.approach_limit_text(ctx);
        let limit_clause = if limit_text.is_empty() {
            String::new()
        } else {
            format!(", speed limit {limit_text}")
        };
        Some(format!(
            "Light {phase}, about {} to the stop bar{limit_clause}.",
            self.short_distance_text(ctx, gap_mi)
        ))
    }

    /// A short gap in round spoken units: feet or meters, never decimals.
    pub fn short_distance_text(&self, ctx: &GameContext, miles: f64) -> String {
        spoken_feet_or_meters(miles, ctx.settings.imperial_units)
    }

    /// The enforced limit AT THE STOP BAR, spoken.
    ///
    /// The terminal callouts named the control but never the limit the
    /// approach is driven at (owner report 2026-07-23). First cut read the
    /// limit at the truck's position -- which mid-ramp still said 55, the
    /// highway's number, useless for a light a quarter mile ahead (owner's
    /// log, same night). The honest number is the zone at the bar itself: the
    /// street being entered.
    pub fn approach_limit_text(&mut self, ctx: &GameContext) -> String {
        match self.street_limit_past_bar_mph() {
            Some(limit) => ctx.settings.speed_text(limit),
            None => String::new(),
        }
    }

    /// The limit on the road past the ramp's stop bar, or None when the probe
    /// can only see the mainline's own number through the gap (see below).
    pub fn street_limit_past_bar_mph(&mut self) -> Option<f64> {
        let mut bar_mi = self.trip.position_mi;
        if let Some(gap_mi) = self.terminal_gap_mi() {
            bar_mi += 0.0f64.max(gap_mi);
        }
        // Probe just PAST the bar, not at it: the entered road's zone (the
        // facility access 25, the street's 35) begins on the far side, so a
        // probe at the bar itself still read the corridor's 55 -- the owner
        // was told "speed limit 55 on the approach" at a stop sign whose far
        // side was a 25 access road (log, 2026-07-23, Merced).
        bar_mi += 0.05;
        bar_mi = bar_mi.min(0.0f64.max(self.trip.total_miles() - 0.01));
        let (limit, _) = self.trip.speed_limit_at(bar_mi);
        // Screened for self-contradiction, not extremity: a street behind a
        // ramp terminal is never posted at the corridor's own highway number,
        // so a probe that comes back with one found no street zone at all --
        // it read the mainline through the gap and told the owner "Stop sign
        // at ramp end. Limit 70" at two exits running (playtest, 2026-08-20).
        // Better no limit clause than a wrong one.
        let position = self.trip.position_mi;
        let (corridor_limit, _) = self.trip.speed_limit_at(position);
        if limit >= corridor_limit && corridor_limit > RAMP_MAX_MPH {
            return None;
        }
        Some(limit)
    }

    /// Mid-ramp callout naming the control at the terminal.
    pub fn announce_ramp_terminal(&mut self, ctx: &mut GameContext) {
        self.ramp_light_announced = true;
        let limit_text = self.approach_limit_text(ctx);
        let terse = self.terse_speech(ctx);
        if self.ramp_control == "signal" {
            let phase = self.ramp_light_phase();
            self.ramp_light_last_phase = phase.to_string();
            ctx.audio.play_with(
                if phase == "red" {
                    "events/ramp_light_red"
                } else {
                    "events/ramp_light_green"
                },
                0.8,
                0.0,
            );
            self.say_route_navigation(ctx, &format!("Light {phase}."));
        } else if self.ramp_control == "stop" {
            ctx.audio.play_with("ui/notify", 0.7, 0.0);
            if terse {
                let limit_clause = if limit_text.is_empty() {
                    String::new()
                } else {
                    format!(" Limit {limit_text}.")
                };
                self.say_route_navigation(ctx, &format!("Stop sign at ramp end.{limit_clause}"));
                return;
            }
            let approach_clause = if limit_text.is_empty() {
                String::new()
            } else {
                format!(" Speed limit {limit_text} on the approach.")
            };
            self.say_route_navigation(
                ctx,
                &format!("Stop sign at the end of the ramp.{approach_clause}"),
            );
        } else if matches!(self.ramp_control.as_str(), "yield" | "roundabout") {
            ctx.audio.play_with("ui/notify", 0.7, 0.0);
            let terse_noun = if self.ramp_control == "roundabout" {
                "Roundabout"
            } else {
                "Yield"
            };
            if terse {
                let limit_clause = if limit_text.is_empty() {
                    String::new()
                } else {
                    format!(" Limit {limit_text}.")
                };
                self.say_route_navigation(ctx, &format!("{terse_noun} at ramp end.{limit_clause}"));
                return;
            }
            // The instruction is the sign's real rule: slow for the gap, and
            // the stop is only owed when the road is not clear. "Brake to a
            // stop" here would teach the stop-sign habit at a sign whose
            // whole point is that a clear road never demands it.
            let message = if self.ramp_control == "roundabout" {
                "Roundabout at the end of the ramp."
            } else {
                "Yield sign at the end of the ramp."
            };
            let approach_clause = if limit_text.is_empty() {
                String::new()
            } else {
                format!(" Speed limit {limit_text} on the approach.")
            };
            self.say_route_navigation(ctx, &format!("{message}{approach_clause}"));
        }
    }

    /// One ROUTE-priority navigation line, the shape this whole section uses.
    ///
    /// The live facts are stamped first: a release ("Gap in traffic",
    /// "Light green") lands in the same frame the hold ends, and the pacer
    /// asks the hold line whether it is still true while delivering it.
    pub(crate) fn say_route_navigation(&self, ctx: &mut GameContext, message: &str) {
        self.refresh_live_facts();
        let mut opts = SayEvent::queued().priority(EventPriority::Important);
        opts.category = Some(SpeechCategory::Navigation);
        ctx.say_event_with(message.to_string(), opts);
    }

    /// One ROUTE-priority confirmation line. A `SpokenMessage` pair gives
    /// the quiet rungs its short form.
    pub(crate) fn say_route_confirmation(&self, ctx: &mut GameContext, message: impl IntoSpoken) {
        self.say_route_line(ctx, message, SpeechCategory::Confirmation);
    }

    /// One ROUTE-priority line in a category of the caller's choosing.
    pub(crate) fn say_route_line(
        &self,
        ctx: &mut GameContext,
        message: impl IntoSpoken,
        category: SpeechCategory,
    ) {
        self.refresh_live_facts();
        let mut opts = SayEvent::queued().priority(EventPriority::Important);
        opts.category = Some(category);
        ctx.say_event_with(message, opts);
    }

    /// A line about the truck held at the bar ("holding for your gap",
    /// "holding the brakes for green"). True only while the hold lasts: the
    /// gap arriving a moment later used to replay it right before "Gap in
    /// traffic. Clear; pull ahead" (agent drive, yield at exit 255,
    /// 2026-09-24).
    pub(crate) fn say_terminal_hold(
        &self,
        ctx: &mut GameContext,
        message: &str,
        category: SpeechCategory,
    ) {
        self.refresh_live_facts();
        let opts = SayEvent::queued()
            .priority(EventPriority::Important)
            .category(category)
            .valid(live::ramp_holding);
        ctx.say_event_with(message.to_string(), opts);
    }
}
