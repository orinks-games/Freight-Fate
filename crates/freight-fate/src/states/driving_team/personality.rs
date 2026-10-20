//! The co-driver's personality at the wheel (owner request, 2026-10-20).
//!
//! What each moment is, and why it is so rare, is in
//! [`ff_core::models::co_driver::personality`]. This half watches the drive
//! for the moments, keeps the pacing, and says them. It runs only while the
//! player rides along with a co-driver whose switch is on; it never touches
//! the pedals, the cab, the radio the player chose, or the pay.

use ff_core::cab_sounds::WINDOW_MOTOR_KEY;
use ff_core::message_log::MessageCategory;
use ff_core::models::co_driver::personality::{self as quirks, Moment};
use ff_core::pyrandom::PyRandom;
use ff_core::sim::hos;
use ff_core::sim::weather::WeatherKind;

use crate::app::{GameContext, Say, TRANSCRIPT_TARGET};
use crate::states::driving::DrivingState;
use crate::states::driving_core::profile_of;
use crate::states::driving_updates::live;

use super::TeamMode;

/// The truck door, for a co-driver getting out and back in.
const DOOR_KEY: &str = "vehicle/truck_door";
/// The driver's side: the co-driver's door and window.
const DRIVER_SIDE_PAN: f64 = -0.5;
/// How far past the truck's nose a passing truck's tail is before the
/// lights flash: about 30 feet.
const CLEAR_OF_NOSE_MI: f64 = 30.0 / 5280.0;
/// A car cutting in this close in front is worth a word.
const CUT_IN_GAP_MI: f64 = 150.0 / 5280.0;
/// A descent at least this long, at this grade or steeper, is a real hill.
const LONG_DESCENT_MI: f64 = 2.0;
const LONG_DESCENT_PCT: f64 = 4.0;
/// How far ahead June hears about a diner.
const DINER_AHEAD_MI: f64 = 3.0;

/// The co-driver's habits over one shift.
pub struct PersonalityMood {
    rng: PyRandom,
    /// Game minute of the next moment allowed.
    next_moment_min: f64,
    /// Spoken moments this shift.
    spoken: u32,
    /// Game minute of the co-driver's last note.
    pub last_note_min: Option<f64>,
    last_weather: Option<WeatherKind>,
    last_track: Option<String>,
    on_downgrade: bool,
    /// Trucks alongside or just behind in the next lane over.
    alongside: Vec<String>,
    /// The vehicle ahead in the lane, and the lanes of what was near.
    last_lead: Option<String>,
    near_lanes: Vec<(String, i64)>,
    diners_heard: Vec<String>,
    /// Sounds still to come, in real seconds.
    pending: Vec<(f64, &'static str)>,
    /// Game minute the cracked window goes back up.
    pub window_up_at: Option<f64>,
}

impl PersonalityMood {
    pub fn new(co_driver_key: &str, seed: i64, now_min: f64) -> Self {
        Self {
            rng: PyRandom::new_from_str(&format!("co-driver-personality:{co_driver_key}:{seed}")),
            next_moment_min: now_min + quirks::FIRST_MOMENT_MIN,
            spoken: 0,
            last_note_min: None,
            last_weather: None,
            last_track: None,
            on_downgrade: false,
            alongside: Vec::new(),
            last_lead: None,
            near_lanes: Vec::new(),
            diners_heard: Vec::new(),
            pending: Vec::new(),
            window_up_at: None,
        }
    }
}

/// One moment, ready to say.
struct Candidate {
    moment: Moment,
    place: String,
    weather: &'static str,
}

impl Candidate {
    fn new(moment: Moment) -> Self {
        Self {
            moment,
            place: String::new(),
            weather: "",
        }
    }
}

impl DrivingState {
    /// The co-driver's name and key while they drive with their personality
    /// on and the player riding along.
    fn personality_at_wheel(&self, ctx: &GameContext) -> Option<(&'static str, &'static str)> {
        let team = self.team.as_ref()?;
        let wheel = team.wheel.as_ref()?;
        let on = quirks::personality_on(&profile_of(ctx).achievement_stats, team.co_driver.key);
        (on && wheel.mode == TeamMode::RideAlong)
            .then_some((team.co_driver.name, team.co_driver.key))
    }

    fn mood_mut(&mut self) -> Option<&mut PersonalityMood> {
        self.team
            .as_mut()
            .and_then(|team| team.wheel.as_mut())
            .map(|wheel| &mut wheel.personality)
    }

    fn moment_used(&self, moment: Moment) -> bool {
        self.team
            .as_ref()
            .is_some_and(|team| team.moments.contains(&moment))
    }

    /// Once a frame while the co-driver drives and the player listens.
    pub fn update_co_driver_personality(&mut self, ctx: &mut GameContext, dt: f64) {
        let Some((name, key)) = self.personality_at_wheel(ctx) else {
            return;
        };
        self.play_pending_personality_sounds(ctx, dt);
        // Every watcher runs every frame, so an edge is seen when it
        // happens, not when the pacing next allows a moment.
        let candidate = self.personality_watch(key);
        let Some(candidate) = candidate else {
            return;
        };
        if self.moment_used(candidate.moment) || !self.personality_open(ctx) {
            return;
        }
        let chance = candidate.moment.chance(key);
        let Some(mood) = self.mood_mut() else {
            return;
        };
        if chance <= 0.0 || mood.rng.random() >= chance {
            return;
        }
        let line = quirks::moment_line(
            candidate.moment,
            name,
            key,
            &candidate.place,
            candidate.weather,
        );
        let sound = (candidate.moment == Moment::CrackWindow).then_some(WINDOW_MOTOR_KEY);
        if candidate.moment == Moment::CrackWindow {
            let now = self.trip.game_minutes;
            if let Some(mood) = self.mood_mut() {
                mood.window_up_at = Some(now + 2.0 + mood.rng.random() * 3.0);
            }
        }
        if matches!(
            candidate.moment,
            Moment::RustyGrandpasRadio | Moment::DeeDialUpSummer
        ) {
            // They leave the song on to its end.
            let track = self.co_driver_radio_track();
            if let Some(wheel) = self.team.as_mut().and_then(|team| team.wheel.as_mut()) {
                wheel.radio.hold_track = track;
            }
        }
        self.say_moment(ctx, candidate.moment, line, sound);
    }

    /// Whether a moment may come now: the gap since the last, the shift's
    /// cap, and nothing more important going on.
    fn personality_open(&self, ctx: &GameContext) -> bool {
        let now = self.trip.game_minutes;
        let Some(wheel) = self.team.as_ref().and_then(|team| team.wheel.as_ref()) else {
            return false;
        };
        let mood = &wheel.personality;
        let recent_note = mood
            .last_note_min
            .is_some_and(|at| now - at < quirks::QUIET_AFTER_NOTE_MIN);
        now >= mood.next_moment_min
            && mood.spoken < quirks::MOMENTS_PER_SHIFT
            && !recent_note
            && wheel.summon.is_none()
            && !live::hazard_active()
            && self.hazard_deadline.is_none()
            && self.pull_over.is_none()
            && self.trip.truck.speed_mph() > 5.0
            && !ctx.co_driver_fast_forward
    }

    /// Say one moment: the message review keeps it with the co-driver's
    /// other lines, and the pacing moves on.
    fn say_moment(
        &mut self,
        ctx: &mut GameContext,
        moment: Moment,
        line: String,
        sound: Option<&'static str>,
    ) {
        let now = self.trip.game_minutes;
        if let Some(team) = self.team.as_mut() {
            team.moments.push(moment);
            if let Some(wheel) = team.wheel.as_mut() {
                let mood = &mut wheel.personality;
                mood.spoken += 1;
                mood.next_moment_min = now
                    + quirks::MOMENT_GAP_MIN
                    + mood.rng.random() * quirks::MOMENT_GAP_SPREAD_MIN;
            }
        }
        log::info!(target: TRANSCRIPT_TARGET, "[co-driver] moment: {}", moment.key());
        ctx.message_log.add(&line, MessageCategory::CoDriver);
        ctx.player_asked(|ctx| {
            if let Some(sound) = sound {
                ctx.audio.play_with(sound, 0.6, DRIVER_SIDE_PAN);
            }
            ctx.say_with(line, Say::queued().review(false));
        });
    }

    fn play_pending_personality_sounds(&mut self, ctx: &mut GameContext, dt: f64) {
        let now = self.trip.game_minutes;
        let Some(mood) = self.mood_mut() else {
            return;
        };
        let mut due: Vec<&'static str> = Vec::new();
        mood.pending.retain_mut(|(left, key)| {
            *left -= dt;
            if *left <= 0.0 {
                due.push(key);
                false
            } else {
                true
            }
        });
        if mood.window_up_at.is_some_and(|at| now >= at) {
            mood.window_up_at = None;
            due.push(WINDOW_MOTOR_KEY);
        }
        if due.is_empty() {
            return;
        }
        ctx.player_asked(|ctx| {
            for key in due {
                ctx.audio.play_with(key, 0.6, DRIVER_SIDE_PAN);
            }
        });
    }

    /// The window goes back up as the co-driver hands back, so the player
    /// does not take the wheel to a window they never heard close.
    pub fn close_co_driver_window(&mut self, ctx: &mut GameContext) {
        let riding = self
            .team
            .as_ref()
            .and_then(|team| team.wheel.as_ref())
            .is_some_and(|wheel| wheel.mode == TeamMode::RideAlong);
        let Some(mood) = self.mood_mut() else {
            return;
        };
        mood.pending.clear();
        if mood.window_up_at.take().is_some() && riding {
            ctx.player_asked(|ctx| ctx.audio.play_with(WINDOW_MOTOR_KEY, 0.6, DRIVER_SIDE_PAN));
        }
    }

    /// The track on the air, the way the co-driver hears it: the playlist
    /// entry (what was scheduled, not the stand-in a synth piece still
    /// composing plays), or Channel 3000's clip.
    pub fn co_driver_radio_track(&self) -> Option<String> {
        if !self.radio.enabled {
            return None;
        }
        if self.radio_station_id == ff_core::channel3000::CHANNEL_3000_ID {
            return self
                .channel3000
                .as_ref()
                .and_then(|schedule| schedule.on_air())
                .map(|clip| clip.key.clone());
        }
        if self.radio_playlist.is_empty() {
            return None;
        }
        Some(self.radio_playlist[self.radio_track_index % self.radio_playlist.len()].clone())
    }

    /// Watch the drive for this co-driver's moments. A signature comes
    /// before a moment anyone can have.
    fn personality_watch(&mut self, key: &str) -> Option<Candidate> {
        let radio = self.watch_radio(key);
        let weather = self.watch_weather(key);
        let hill = self.watch_hill(key);
        let diner = self.watch_diner(key);
        let passing = self.watch_passing_truck();
        let cut_in = self.watch_cut_in(key);
        radio
            .or(weather)
            .or(hill)
            .or(diner)
            .or_else(|| self.watch_lou(key))
            .or(passing)
            .or(cut_in)
            .or_else(|| self.watch_window(key))
    }

    fn watch_radio(&mut self, key: &str) -> Option<Candidate> {
        let track = self.co_driver_radio_track();
        let station = self.radio_station_id.clone();
        let leans = self.team.as_ref()?.co_driver.habits.leans;
        let mood = self.mood_mut()?;
        if mood.last_track == track {
            return None;
        }
        let first = mood.last_track.is_none();
        mood.last_track = track.clone();
        // A track already on at the handover is not news.
        if first {
            return None;
        }
        let track = track?;
        let moment = match key {
            "rusty" if track.contains("grandpas_radio") => Moment::RustyGrandpasRadio,
            "dee" if track.contains("dial_up_summer") => Moment::DeeDialUpSummer,
            "nate" if track.contains("the_lone_courier") => Moment::NateLoneCourier,
            "june" if track.starts_with("radio_") && leans.contains(&station.as_str()) => {
                Moment::JuneHums
            }
            _ => return None,
        };
        Some(Candidate::new(moment))
    }

    fn watch_weather(&mut self, key: &str) -> Option<Candidate> {
        let now = self.trip.weather.current;
        let mood = self.mood_mut()?;
        let before = mood.last_weather.replace(now)?;
        if key != "walt" {
            return None;
        }
        let word = quirks::wet_turn(before, now)?;
        Some(Candidate {
            weather: word,
            ..Candidate::new(Moment::WaltSawItComing)
        })
    }

    fn watch_hill(&mut self, key: &str) -> Option<Candidate> {
        let down = self.on_downgrade();
        let mood = self.mood_mut()?;
        let started = down && !mood.on_downgrade;
        mood.on_downgrade = down;
        let moment = match key {
            "marisol" => Moment::MarisolGrades,
            "ray" => Moment::RayHills,
            _ => return None,
        };
        let long = started
            && self.grade_run_over_mi(self.trip.position_mi, -1, LONG_DESCENT_PCT)
                > LONG_DESCENT_MI;
        long.then(|| Candidate::new(moment))
    }

    fn watch_diner(&mut self, key: &str) -> Option<Candidate> {
        if key != "june" {
            return None;
        }
        let stop = self.trip.upcoming_stop(DINER_AHEAD_MI)?;
        if !quirks::is_diner_name(&stop.name) {
            return None;
        }
        let name = stop.name.clone();
        let place = quirks::place_without_direction(&name).to_string();
        let mood = self.mood_mut()?;
        if mood.diners_heard.contains(&name) {
            return None;
        }
        mood.diners_heard.push(name);
        Some(Candidate {
            place,
            ..Candidate::new(Moment::JuneDiner)
        })
    }

    fn watch_lou(&self, key: &str) -> Option<Candidate> {
        if key != "lou" {
            return None;
        }
        let wheel = self.team.as_ref()?.wheel.as_ref()?;
        let quiet = hos::is_night(self.trip.local_hour())
            && wheel.log.game_minutes >= 60.0
            && self.trip.next_curve_within(10.0).is_none()
            && self.trip.upcoming_stop(10.0).is_none();
        quiet.then(|| Candidate::new(Moment::LouMeridian))
    }

    /// A truck in the next lane over pulling clear of the nose at night.
    fn watch_passing_truck(&mut self) -> Option<Candidate> {
        let front = self.trip.position_mi;
        let night = hos::is_night(self.trip.local_hour());
        let truck_mph = self.trip.truck.speed_mph();
        let mut beside: Vec<String> = Vec::new();
        let mut cleared = false;
        let was_beside = self
            .team
            .as_ref()?
            .wheel
            .as_ref()?
            .personality
            .alongside
            .clone();
        for vehicle in &self.trip.traffic_manager.vehicles {
            if vehicle.vehicle_class != "semi"
                || vehicle.relative_lane.abs() != 1
                || vehicle.speed_mph <= truck_mph
            {
                continue;
            }
            let tail = vehicle.position_mi;
            if tail <= front + CLEAR_OF_NOSE_MI && tail > front - 0.05 {
                beside.push(vehicle.key.clone());
            } else if tail > front + CLEAR_OF_NOSE_MI && was_beside.contains(&vehicle.key) {
                cleared = true;
            }
        }
        self.mood_mut()?.alongside = beside;
        (cleared && night).then(|| Candidate::new(Moment::NightLightsClear))
    }

    /// A car pulling into the lane close in front, while the co-driver
    /// was not the one changing lanes.
    fn watch_cut_in(&mut self, key: &str) -> Option<Candidate> {
        let position = self.trip.position_mi;
        let lead = self
            .trip
            .traffic_manager
            .lead_vehicle(position, self.trip.truck.speed_mph());
        let near: Vec<(String, i64)> = self
            .trip
            .traffic_manager
            .vehicles
            .iter()
            .filter(|vehicle| (vehicle.position_mi - position).abs() < 0.2)
            .map(|vehicle| (vehicle.key.clone(), vehicle.lane))
            .collect();
        let steady = self.lane_change_target.is_none() && self.passing.is_none();
        let mood = self.mood_mut()?;
        let previous_lead = mood.last_lead.take();
        let previous_lanes = std::mem::replace(&mut mood.near_lanes, near);
        let lead = lead?;
        mood.last_lead = Some(lead.lead.key.clone());
        if !matches!(key, "rusty" | "nate") || !steady {
            return None;
        }
        if previous_lead.as_ref() == Some(&lead.lead.key) || lead.gap_mi > CUT_IN_GAP_MI {
            return None;
        }
        let car = matches!(
            lead.lead.vehicle_class.as_str(),
            "car" | "pickup" | "motorcycle"
        );
        let moved_over = previous_lanes
            .iter()
            .any(|(k, lane)| *k == lead.lead.key && *lane != lead.lead.lane);
        (car && (moved_over || lead.lead.intent == "merging"))
            .then(|| Candidate::new(Moment::CutInMutter))
    }

    /// Small hours, dry, a long while into the shift, and a straight road.
    fn watch_window(&self, key: &str) -> Option<Candidate> {
        if Moment::CrackWindow.chance(key) <= 0.0 || self.cab.window_open() {
            return None;
        }
        let wheel = self.team.as_ref()?.wheel.as_ref()?;
        let hour = self.trip.local_hour();
        let dry = matches!(
            self.trip.weather.current,
            WeatherKind::Clear | WeatherKind::Cloudy
        );
        let fits = (1.0..4.0).contains(&hour)
            && wheel.log.game_minutes >= 120.0
            && dry
            && self.trip.next_curve_within(5.0).is_none();
        fits.then(|| Candidate::new(Moment::CrackWindow))
    }

    /// The co-driver is done at a stop, with `note` their line about it.
    /// Dan walks around the trailer first; anyone may come back with
    /// something for the bunk after a plain fuel stop or a break. Returns
    /// the note with anything added.
    pub fn personality_at_stop(
        &mut self,
        ctx: &mut GameContext,
        note: String,
        fueled: bool,
        took_break: bool,
    ) -> String {
        let Some((name, key)) = self.personality_at_wheel(ctx) else {
            return note;
        };
        let mut note = note;
        let mut door = false;
        if key == "dan" {
            // Every stop, by ear: out, a walk around the trailer, back in.
            // The words only the first time.
            let first = !self.moment_used(Moment::DanWalkAround);
            if let Some(mood) = self.mood_mut() {
                let gap = 8.0 + mood.rng.random() * 2.0;
                mood.pending.push((0.0, DOOR_KEY));
                mood.pending.push((gap, DOOR_KEY));
            }
            if first {
                if let Some(team) = self.team.as_mut() {
                    team.moments.push(Moment::DanWalkAround);
                }
                log::info!(target: TRANSCRIPT_TARGET, "[co-driver] moment: dan_walk_around");
                note = format!(
                    "{note} {}",
                    quirks::moment_line(Moment::DanWalkAround, name, key, "", "")
                );
            }
        } else {
            door = true;
        }
        let coffee_fits = fueled != took_break
            && !self.moment_used(Moment::CoffeeOnReturn)
            && self
                .team
                .as_ref()
                .and_then(|team| team.wheel.as_ref())
                .is_some_and(|wheel| wheel.personality.spoken < quirks::MOMENTS_PER_SHIFT);
        let coffee = coffee_fits
            && self
                .mood_mut()
                .is_some_and(|mood| mood.rng.random() < Moment::CoffeeOnReturn.chance(key));
        if coffee {
            if let Some(team) = self.team.as_mut() {
                team.moments.push(Moment::CoffeeOnReturn);
                if let Some(wheel) = team.wheel.as_mut() {
                    wheel.personality.spoken += 1;
                }
            }
            log::info!(target: TRANSCRIPT_TARGET, "[co-driver] moment: coffee_on_return");
            note = format!(
                "{note} {}",
                quirks::moment_line(Moment::CoffeeOnReturn, name, key, "", "")
            );
            if door {
                ctx.player_asked(|ctx| ctx.audio.play_with(DOOR_KEY, 0.6, DRIVER_SIDE_PAN));
            }
        }
        note
    }
}
