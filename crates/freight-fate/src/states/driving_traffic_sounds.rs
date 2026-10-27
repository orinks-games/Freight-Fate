//! The traffic you hear is the traffic that is there.
//!
//! Every NPC vehicle near the cab gets a sound: one steady loop for its class
//! (`traffic/<class>_loop`), whose level, pan and pitch are set every frame
//! from where that vehicle really is. A car coming up the left lane swells in
//! the left ear, is loudest alongside, and fades ahead with its pitch dropping
//! as it pulls away; a car crossing in front of a truck stopped at a ramp end
//! sweeps from the ear it came from to the other one, loudest at the
//! crossing. Nothing is scheduled ahead, so nothing can land early or late.
//!
//! This replaced one-shot whooshes (owner, 2026-10-08: "the sounds aren't
//! synced with the NPC traffic"). Those fired when a vehicle crossed the
//! bumper, but each recording peaked somewhere different, from a tenth of a
//! second to well over a second in, so the loudest moment arrived after the
//! car was already gone at ten times pacing; a six-second cooldown dropped
//! most passes outright; and on an exit ramp, where the highway odometer
//! holds, every mainline car still "passed" the truck dead centre as if it
//! drove through the cab.
//!
//! On a ramp the mainline stays where it is: off to the left, getting farther
//! away as the ramp leaves it, so it fades out down the ramp instead of
//! driving through the truck.
//!
//! Under the sounds sits the freeway's own distant traffic, a bed whose level
//! is the road's real presence ([`TrafficManager::freeway_presence_at`]), so a
//! busy interstate sounds busy and a quiet one at three in the morning does
//! not. Off a freeway there is no bed.
//!
//! Earcons, never sentences, for the reason the old whooshes gave: nothing
//! follows from hearing a car go by, and the spoken budget belongs to things
//! that cost money.
//!
//! [`TrafficManager::freeway_presence_at`]: ff_core::sim::traffic_manager::TrafficManager::freeway_presence_at

use crate::app::GameContext;
use crate::audio::{CH_TRAFFIC_BED, CH_TRAFFIC_SOUNDS};
use crate::states::driving::DrivingState;

/// Beyond this a vehicle is part of the bed, not a sound of its own. The
/// bubble keeps vehicles a tenth of a mile apart or more, so a 450-foot edge
/// left most of a busy interstate silent (owner drive, 2026-10-09).
pub const TRAFFIC_SOUND_HEAR_FT: f64 = 1500.0;
/// The vehicle ahead in the truck's own lane, the one the game calls out and
/// the cruise follows, is heard this far and always takes a sound first: with
/// the lanes beside filled, nearer cars took all three and the car the game
/// had just named stayed silent (owner drive, 2026-10-09).
pub const TRAFFIC_LEAD_HEAR_FT: f64 = 2640.0;
/// Inside this a sound is at full level: alongside, or a few car lengths off.
pub const TRAFFIC_SOUND_REF_FT: f64 = 60.0;
/// A sound's level at [`TRAFFIC_SOUND_REF_FT`]. It drops 3 dB with each
/// doubling of distance past that, so a car 500 feet up the road is still
/// about a third of this. The first cut peaked at 0.45 from 12 feet and fell
/// 6 dB a doubling, which put a car a hundred feet off at 0.05: under the
/// engine, and the owner heard traffic only as it passed (2026-10-09).
pub const TRAFFIC_SOUND_PEAK: f64 = 0.8;
/// Lane width, for how far over the next lane is.
pub const LANE_WIDTH_FT: f64 = 12.0;
/// Where an exit ramp leaves the mainline: the gore, a lane's width past the
/// right lane's edge.
pub const RAMP_SPLIT_FT: f64 = 24.0;
/// Sideways feet a ramp gains per foot along it: about a twenty-degree split.
pub const RAMP_DIVERGENCE: f64 = 0.35;
/// The crossroad is this far past a ramp's stop bar.
pub const CROSS_SETBACK_FT: f64 = 30.0;
const SPEED_OF_SOUND_MPH: f64 = 767.0;
/// Doppler is felt, not measured: kept to a few percent so a fast closing
/// speed never turns a car into a siren.
const DOPPLER_LIMIT: f64 = 0.05;
/// How quickly the bed follows the road, in seconds to close most of a gap.
const BED_SMOOTH_S: f64 = 2.0;
/// The bed's level at the busiest road the model has.
pub const TRAFFIC_BED_PEAK: f64 = 0.4;
const BED_FLOOR: f64 = 0.01;

/// One vehicle worth hearing this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct HeardVehicle {
    /// Stable for as long as the vehicle exists: `main:<key>` or `cross:<id>`.
    pub id: String,
    pub key: &'static str,
    pub distance_ft: f64,
    pub volume: f64,
    pub pan: f64,
    pub rate: f64,
}

/// The loop for a traffic class, falling back to the car.
pub fn traffic_sound_key(vehicle_class: &str) -> &'static str {
    match vehicle_class.trim().to_lowercase().as_str() {
        "semi" => "traffic/semi_loop",
        "box truck" | "service vehicle" => "traffic/box_truck_loop",
        "pickup" => "traffic/pickup_loop",
        "motorcycle" => "traffic/motorcycle_loop",
        "bus" => "traffic/bus_loop",
        "tractor" => "traffic/tractor_loop",
        "state trooper" => "traffic/trooper_loop",
        _ => "traffic/car_loop",
    }
}

/// Level for a vehicle `distance_ft` away: falling as one over the square
/// root of the distance, gentler than open air, because the cab mix has to
/// carry a car the player cannot see; and eased to nothing over the last stretch before the hearing edge so a
/// sound never appears or vanishes with a step.
pub fn traffic_sound_volume(distance_ft: f64) -> f64 {
    traffic_sound_volume_within(distance_ft, TRAFFIC_SOUND_HEAR_FT)
}

/// [`traffic_sound_volume`] with its own hearing edge.
fn traffic_sound_volume_within(distance_ft: f64, hear_ft: f64) -> f64 {
    if distance_ft >= hear_ft {
        return 0.0;
    }
    let near =
        TRAFFIC_SOUND_PEAK * (TRAFFIC_SOUND_REF_FT / distance_ft.max(TRAFFIC_SOUND_REF_FT)).sqrt();
    let edge = ((hear_ft - distance_ft) / (hear_ft * 0.3)).clamp(0.0, 1.0);
    near * edge
}

/// How much of the road's length counts against a vehicle's side when it is
/// panned. The true angle put a car one lane over and a hundred feet ahead
/// 10 percent off centre, so most traffic sounded mono and a pass swept only
/// in its last second (owner drive, 2026-10-09); at this share that car is
/// well over on its side and the sweep starts a few hundred feet out.
const PAN_ALONG_SHARE: f64 = 0.15;
/// A vehicle behind the cab is this much quieter than one the same distance
/// ahead: stereo cannot tell front from back, and the trailer stands
/// between the cab and the road behind it.
pub const TRAFFIC_BEHIND_SHARE: f64 = 0.75;

/// Pan for a vehicle `across_ft` to the side (negative left) and `along_ft`
/// ahead or behind: hard to its side when alongside, easing toward the
/// middle as it gets far up or down the road. A vehicle in the truck's own
/// lane stays centred.
pub fn traffic_sound_pan(across_ft: f64, along_ft: f64) -> f64 {
    let spread = across_ft.abs() + along_ft.abs() * PAN_ALONG_SHARE;
    if spread <= 0.0 {
        return 0.0;
    }
    (0.85 * across_ft / spread).clamp(-0.85, 0.85)
}

/// Playback rate: a slower vehicle sounds lower, and one closing on the cab
/// a few percent higher than one pulling away. `approach_mph` is how fast the
/// distance is shrinking, in real miles per hour.
pub fn traffic_sound_rate(vehicle_mph: f64, approach_mph: f64) -> f64 {
    let pace = 0.8 + 0.2 * (vehicle_mph.abs() / 60.0).clamp(0.0, 1.25);
    let doppler =
        (1.0 + approach_mph / SPEED_OF_SOUND_MPH).clamp(1.0 - DOPPLER_LIMIT, 1.0 + DOPPLER_LIMIT);
    pace * doppler
}

/// [`TRAFFIC_BEHIND_SHARE`] for a vehicle behind the cab, eased in over the
/// truck's own length so a pass does not step down as it clears the bumper.
fn behind_share(along_ft: f64) -> f64 {
    let behind = (-along_ft / 70.0).clamp(0.0, 1.0);
    1.0 - (1.0 - TRAFFIC_BEHIND_SHARE) * behind
}

/// A standing vehicle has no tire roar: an idling queue is quieter than one
/// rolling.
fn rolling_share(vehicle_mph: f64) -> f64 {
    0.25 + 0.75 * (vehicle_mph.abs() / 45.0).clamp(0.0, 1.0)
}

impl DrivingState {
    pub fn reset_traffic_sounds(&mut self, ctx: &mut GameContext) {
        for (slot, channel) in self.traffic_sounds.iter_mut().zip(CH_TRAFFIC_SOUNDS) {
            if slot.take().is_some() {
                ctx.audio.stop_loop_with(channel, 200);
            }
        }
        if self.traffic_bed_volume > 0.0 {
            ctx.audio.stop_loop_with(CH_TRAFFIC_BED, 400);
        }
        self.traffic_bed_volume = 0.0;
        self.traffic_ramp_rolled_ft = 0.0;
    }

    /// Every vehicle near enough to hear: the lead in the truck's lane first,
    /// then nearest first.
    pub fn heard_traffic(&self) -> Vec<HeardVehicle> {
        let mut heard = Vec::new();
        let truck_mph = self.trip.truck.speed_mph();
        let on_ramp = self.ramp_mi.is_some();
        let player_lane = self.trip.traffic_manager.player_lane;
        let lead_key = if on_ramp {
            None
        } else {
            self.trip
                .traffic_manager
                .lead_vehicle(self.trip.position_mi, truck_mph)
                .map(|context| context.lead.key)
        };
        for vehicle in &self.trip.traffic_manager.vehicles {
            let along = (vehicle.position_mi - self.trip.position_mi) * 5280.0;
            let is_lead = lead_key.as_deref() == Some(vehicle.key.as_str());
            let hear_ft = if is_lead {
                TRAFFIC_LEAD_HEAR_FT
            } else {
                TRAFFIC_SOUND_HEAR_FT
            };
            // Lane indices count leftward, so a higher lane is to the left.
            // On a ramp, how much farther off the divergence alone has put
            // it: the freeway falls away faster than the gentle in-traffic
            // law, as a road the cab is leaving does.
            let mut diverged = 1.0;
            let (across, closing_mph) = if on_ramp {
                // The odometer holds on the ramp: the mainline streams past a
                // standing point at its own speed, off to the left and
                // farther each foot the truck rolls down the ramp.
                let at_gore = (vehicle.lane.max(0) as f64 + 0.5) * LANE_WIDTH_FT + RAMP_SPLIT_FT;
                let over = at_gore + self.traffic_ramp_rolled_ft * RAMP_DIVERGENCE;
                diverged = (at_gore.hypot(along) / over.hypot(along)).sqrt();
                (-over, vehicle.speed_mph)
            } else {
                let lanes = (vehicle.lane - player_lane) as f64;
                (-lanes * LANE_WIDTH_FT, vehicle.speed_mph - truck_mph)
            };
            let distance = along.hypot(across);
            if distance >= hear_ft {
                continue;
            }
            // d(distance)/dt along the road: a vehicle ahead moving away, or
            // behind and gaining, changes it by `closing` times the share of
            // the distance that lies along the road.
            let along_share = if distance > 0.0 {
                along / distance
            } else {
                0.0
            };
            let approach = -closing_mph * along_share;
            heard.push(HeardVehicle {
                id: format!("main:{}", vehicle.key),
                key: traffic_sound_key(&vehicle.vehicle_class),
                distance_ft: distance,
                volume: traffic_sound_volume_within(distance, hear_ft)
                    * diverged
                    * behind_share(along)
                    * rolling_share(vehicle.speed_mph),
                pan: traffic_sound_pan(across, along),
                rate: traffic_sound_rate(vehicle.speed_mph, approach),
            });
        }
        if let (Some(bubble), true) = (self.cross_bubble.as_ref(), self.terminal_live()) {
            let setback =
                self.terminal_gap_mi().unwrap_or(0.0).max(0.0) * 5280.0 + CROSS_SETBACK_FT;
            for vehicle in &bubble.vehicles {
                // Cross traffic drives toward positive positions; the side it
                // came from is where a negative position puts it.
                let entered_left = vehicle.from_side == "left";
                let toward = if entered_left { 1.0 } else { -1.0 };
                let across = vehicle.position_mi * 5280.0 * toward;
                let distance = across.hypot(setback);
                if distance >= TRAFFIC_SOUND_HEAR_FT {
                    continue;
                }
                let across_share = if distance > 0.0 {
                    across / distance
                } else {
                    0.0
                };
                let approach = -vehicle.speed_mph * toward * across_share;
                heard.push(HeardVehicle {
                    id: format!("cross:{}", vehicle.id),
                    key: traffic_sound_key(vehicle.vehicle_class),
                    distance_ft: distance,
                    volume: traffic_sound_volume(distance) * rolling_share(vehicle.speed_mph),
                    pan: traffic_sound_pan(across, setback),
                    rate: traffic_sound_rate(vehicle.speed_mph, approach),
                });
            }
        }
        heard.retain(|v| v.volume > 0.0);
        let lead_id = lead_key.map(|key| format!("main:{key}"));
        heard.sort_by(|a, b| {
            let a_lead = lead_id.as_deref() == Some(a.id.as_str());
            let b_lead = lead_id.as_deref() == Some(b.id.as_str());
            b_lead
                .cmp(&a_lead)
                .then(a.distance_ft.total_cmp(&b.distance_ft))
        });
        heard
    }

    /// Give the nearest vehicles their sounds and set each one from where its
    /// vehicle is. Runs after the trip has stepped the bubble, so the
    /// positions are this frame's.
    pub fn update_traffic_sounds(&mut self, ctx: &mut GameContext, dt: f64) {
        if self.ramp_mi.is_some() {
            self.traffic_ramp_rolled_ft += self.trip.truck.velocity_mps.abs() * dt * 3.28084;
        } else {
            self.traffic_ramp_rolled_ft = 0.0;
        }
        let mut heard = self.heard_traffic();
        heard.truncate(CH_TRAFFIC_SOUNDS.len());

        // A vehicle keeps its slot while it stays among the nearest; the
        // slots it leaves go quiet, and a newcomer takes a free one.
        for (slot, channel) in self.traffic_sounds.iter_mut().zip(CH_TRAFFIC_SOUNDS) {
            let kept = slot
                .as_ref()
                .is_some_and(|id| heard.iter().any(|v| &v.id == id));
            if !kept && slot.take().is_some() {
                ctx.audio.stop_loop_with(channel, 250);
            }
        }
        for vehicle in heard {
            let index = match self
                .traffic_sounds
                .iter()
                .position(|slot| slot.as_deref() == Some(vehicle.id.as_str()))
            {
                Some(index) => index,
                None => match self.traffic_sounds.iter().position(Option::is_none) {
                    Some(index) => {
                        self.traffic_sounds[index] = Some(vehicle.id.clone());
                        index
                    }
                    None => continue,
                },
            };
            let channel = CH_TRAFFIC_SOUNDS[index];
            // start_loop dedupes on a running key, so this is also the level
            // update, and it restarts the loop if anything stopped it.
            ctx.audio
                .start_loop_with(channel, vehicle.key, vehicle.volume, 150);
            ctx.audio.set_loop_pan(channel, vehicle.pan);
            ctx.audio.set_loop_rate(channel, vehicle.rate);
            if let Some(id) = vehicle.id.strip_prefix("cross:") {
                if let Some(bubble) = self.cross_bubble.as_mut() {
                    for v in bubble.vehicles.iter_mut() {
                        if v.id.to_string() == id {
                            v.sound_started = true;
                        }
                    }
                }
            }
        }
        self.update_traffic_bed(ctx, dt);
    }

    /// What the freeway's distant traffic should be at, before smoothing.
    pub fn traffic_bed_target(&self) -> f64 {
        let presence = self
            .trip
            .traffic_manager
            .freeway_presence_at(self.trip.position_mi);
        let mut level = TRAFFIC_BED_PEAK * presence;
        if self.ramp_mi.is_some() {
            // Down the ramp the freeway falls behind; at the ramp's end it is
            // still there, faintly, past the crossroad.
            let away = (self.traffic_ramp_rolled_ft / 1200.0).clamp(0.0, 1.0);
            level *= 1.0 - 0.7 * away;
        }
        level
    }

    fn update_traffic_bed(&mut self, ctx: &mut GameContext, dt: f64) {
        let target = self.traffic_bed_target();
        let blend = if dt > 0.0 {
            (dt / BED_SMOOTH_S).min(1.0)
        } else {
            1.0
        };
        self.traffic_bed_volume += (target - self.traffic_bed_volume) * blend;
        if self.traffic_bed_volume < BED_FLOOR {
            if self.traffic_bed_volume > 0.0 && target < BED_FLOOR {
                ctx.audio.stop_loop_with(CH_TRAFFIC_BED, 800);
                self.traffic_bed_volume = 0.0;
            }
            return;
        }
        ctx.audio.start_loop_with(
            CH_TRAFFIC_BED,
            "traffic/highway_bed",
            self.traffic_bed_volume,
            800,
        );
    }
}
