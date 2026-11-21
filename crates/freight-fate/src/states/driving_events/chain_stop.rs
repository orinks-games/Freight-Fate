//! Roadside chain work: putting snow chains on and taking them off,
//! tightening a slapping chain and fixing a broken one. The pause menu and
//! facility stopping assistance both call the same `do_*` actions, so the
//! work, the time and the lines are the same whoever starts it.

use ff_core::models::profile::CHAIN_REPAIR_LINKS_PER_SET;
use ff_core::pyfmt::fmt_f;
use ff_core::sim::chain_control::{snow_or_ice, ChainSetEvent};
use ff_core::sim::hos::limits;
use ff_core::sim::trip_models::CHAIN_LAW_LEAD_MI;
use ff_core::speech_pacing::SpeechCategory;

use crate::app::{GameContext, SayEvent};
use crate::states::driving::DrivingState;
use crate::states::driving_core::{
    advance_on_duty_clock, advance_on_duty_clock_holding_sky, clock_text, deadline_text, hos_of,
    is_night, profile_mut_of, profile_of, CHAIN_INSTALL_FATIGUE, CHAIN_INSTALL_MIN,
    CHAIN_INSTALL_NIGHT_FATIGUE, CHAIN_INSTALL_NIGHT_MULT, CHAIN_REMOVE_FATIGUE, CHAIN_REMOVE_MIN,
    CHAIN_REPAIR_MIN, CHAIN_REPAIR_WEAR_RESTORE, CHAIN_SAFE_MPH, CHAIN_TIGHTEN_MIN,
    CHAIN_TIGHTEN_WEAR_RESTORE,
};
use crate::states::driving_stops::assist_servo_brake;

/// The roadside step facility stopping assistance is pulling over for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainStopKind {
    Install,
    Remove,
    Tighten,
    Repair,
}

/// A pending chain stop: what, and the route mile it brakes for.
#[derive(Debug, Clone, PartialEq)]
pub struct ChainStop {
    pub kind: ChainStopKind,
    pub at_mi: f64,
    /// The driver's pedal was already down when the stop armed. Held on, it
    /// doesn't cancel; a fresh press does.
    pub held_input: bool,
}

/// The chain-up stop arms this far before a chain-law area that would chain
/// the rig: room to brake from highway speed to the pullout.
pub const CHAIN_STOP_ARM_MI: f64 = 1.5;
/// Chain-off waits for this much bare road (`ChainSet`'s bare-road cue).
pub use ff_core::sim::chain_control::CHAIN_OFF_BARE_MI;
/// A chain-up is not started with less than this many minutes left in the
/// 14-hour window beyond the install itself.
pub const CHAIN_STOP_HOS_MARGIN_MIN: f64 = 15.0;
/// The planned stopping deceleration for a roadside chain stop, m/s2.
const CHAIN_STOP_DECEL_MPS2: f64 = 1.2;
const METERS_PER_MILE: f64 = 1609.344;

impl DrivingState {
    /// Move the chains (`ChainSet::next`), the one place they move. A
    /// bare-road cue goes to facility stopping assistance when it is on,
    /// and is said to the driver otherwise: one line either way.
    pub fn chain_set_event(&mut self, event: ChainSetEvent) {
        let (next, cue) = self.chain_set.next(event);
        self.chain_set = next;
        if cue.is_some() {
            self.chain_bare_cue_pending = true;
        }
    }

    /// The chains against the road under them, once a frame after the
    /// physics, and the bare-road line when the assist won't take them off.
    pub fn update_chain_set(&mut self, ctx: &mut GameContext) {
        let snapped = self.trip.truck.chains_just_snapped;
        if snapped {
            self.chain_set_event(ChainSetEvent::WornThrough);
            self.chain_bare_cue_pending = false;
        }
        self.chain_set_event(ChainSetEvent::Road {
            snow: snow_or_ice(&self.trip.truck.surface),
            mile: self.trip.position_mi,
        });
        if self.chain_bare_cue_pending
            && (!ctx.settings.destination_approach_assist || !self.trip.truck.chains_on)
        {
            self.chain_bare_cue_pending = false;
            if self.trip.truck.chains_on {
                self.say_bare_road(ctx);
            }
        }
    }

    /// Chains on with no snow under them: they wear through in a few miles
    /// and the snap damages the truck, so the line keeps its words at quiet
    /// verbosity, like the slap warning.
    pub fn say_bare_road(&mut self, ctx: &mut GameContext) {
        ctx.say_event_with(
            "Bare road. Stop and take the chains off.",
            SayEvent::new().category(SpeechCategory::Safety),
        );
    }

    /// A usable set aboard: owned, and the truck's own meter says not
    /// snapped (the profile only catches up at a stop).
    pub fn chains_aboard(&self, ctx: &GameContext) -> bool {
        profile_of(ctx).chains_owned() && self.trip.truck.chain_wear_pct < 100.0
    }

    /// Facility stopping assistance is braking into the chain-up area to
    /// put chains on. The chain control stands at the start of that area,
    /// so a truck pulling in to chain up is not one running the control:
    /// without this the trooper wrote the ticket while the truck was still
    /// braking for the pullout (issue #502).
    pub fn chain_up_pending(&self) -> bool {
        self.chain_stop
            .as_ref()
            .is_some_and(|stop| stop.kind == ChainStopKind::Install)
    }

    /// A new set goes in the side box on the road: fresh, with its spare
    /// repair links. The truck's own meter is reset too, because it is what
    /// says whether a set aboard can go on.
    pub fn stow_new_chain_set(&mut self, ctx: &mut GameContext) {
        let p = profile_mut_of(ctx);
        p.set_chains_owned(true);
        p.set_chain_wear_pct(0.0);
        p.set_chain_repair_links(CHAIN_REPAIR_LINKS_PER_SET);
        self.trip.truck.chain_wear_pct = 0.0;
    }

    /// The minutes a chain-up takes now: longer in the dark, and longer
    /// for a tired driver.
    pub fn chain_install_min(&self, ctx: &GameContext) -> f64 {
        let night = is_night(self.trip.local_hour());
        let base = CHAIN_INSTALL_MIN * if night { CHAIN_INSTALL_NIGHT_MULT } else { 1.0 };
        self.hand_work_min(ctx, base)
    }

    /// Put the chains on at a stop (on duty), or None after "Come to a complete stop first."
    /// Shared by the pause menu and facility stopping assistance.
    pub fn do_install_chains(&mut self, ctx: &mut GameContext) -> Option<String> {
        if self.trip.truck.speed_mph() > 3.0 {
            ctx.say("Come to a complete stop first.");
            return None;
        }
        let night = is_night(self.trip.local_hour());
        let minutes = self.chain_install_min(ctx);
        let fatigue = if night {
            CHAIN_INSTALL_NIGHT_FATIGUE
        } else {
            CHAIN_INSTALL_FATIGUE
        };
        // The simulated sky holds while the chains go on: the snow that
        // posted the law must not end during the work (player report,
        // 2026-11-21).
        advance_on_duty_clock_holding_sky(
            self,
            ctx,
            minutes,
            Some("on_duty_not_driving"),
            "chain up",
        );
        {
            let p = profile_mut_of(ctx);
            p.fatigue = (p.fatigue + fatigue).min(100.0);
        }
        self.trip.truck.chains_on = true;
        // The road as it reads after the clock moved, not before the work.
        let surface_snow = snow_or_ice(self.trip.weather.effects().surface);
        self.chain_set_event(ChainSetEvent::Installed {
            surface_snow,
            mile: self.trip.position_mi,
        });
        self.chains_fast_active = false;
        self.chain_slap_said = false;
        self.chain_tightened = false;
        self.chain_assist_tighten_tried = false;
        ctx.audio.play("ui/notify");
        let effort = if night {
            "In the dark, it took more out of you. "
        } else {
            ""
        };
        let bare = if !surface_snow {
            " The road here is bare. Chains wear out fast until you reach snow."
        } else {
            ""
        };
        let text = format!(
            "Chains on in {} minutes. {effort}Keep under {}.{bare} It is {}. {}",
            fmt_f(minutes, 0),
            ctx.settings.speed_text(CHAIN_SAFE_MPH),
            clock_text(self.trip.local_hour()),
            deadline_text(self, ctx)
        );
        Some(text)
    }

    /// Fix a broken chain with a spare link (on duty).
    /// Shared by the pause menu and facility stopping assistance.
    pub fn do_repair_chain(&mut self, ctx: &mut GameContext) -> Option<String> {
        if self.trip.truck.speed_mph() > 3.0 {
            ctx.say("Come to a complete stop first.");
            return None;
        }
        let links = profile_of(ctx).chain_repair_links();
        if links <= 0 {
            ctx.audio.play("ui/error");
            return Some("No repair links left. The chain stays off.".to_string());
        }
        let minutes = self.hand_work_min(ctx, CHAIN_REPAIR_MIN);
        advance_on_duty_clock(
            self,
            ctx,
            minutes,
            Some("on_duty_not_driving"),
            "repair chain",
        );
        profile_mut_of(ctx).set_chain_repair_links(links - 1);
        self.trip.truck.chain_wear_pct = 100.0 - CHAIN_REPAIR_WEAR_RESTORE;
        self.trip.truck.chains_on = true;
        self.chain_set_event(ChainSetEvent::Repaired {
            surface_snow: snow_or_ice(self.trip.weather.effects().surface),
            mile: self.trip.position_mi,
        });
        self.chains_fast_active = false;
        self.chain_slap_said = false;
        self.chain_tightened = false;
        self.chain_assist_tighten_tried = false;
        ctx.audio.play("ui/notify");
        Some(format!(
            "Chain repaired with a spare link. Back on in {} minutes. Check it again soon. \
             It is {}. {}",
            fmt_f(minutes, 0),
            clock_text(self.trip.local_hour()),
            deadline_text(self, ctx)
        ))
    }

    /// Take up the slack in a slapping chain (on duty).
    /// Shared by the pause menu and facility stopping assistance.
    pub fn do_tighten_chains(&mut self, ctx: &mut GameContext) -> Option<String> {
        if self.trip.truck.speed_mph() > 3.0 {
            ctx.say("Come to a complete stop first.");
            return None;
        }
        let minutes = self.hand_work_min(ctx, CHAIN_TIGHTEN_MIN);
        advance_on_duty_clock(
            self,
            ctx,
            minutes,
            Some("on_duty_not_driving"),
            "tighten chains",
        );
        self.trip.truck.chain_wear_pct =
            (self.trip.truck.chain_wear_pct - CHAIN_TIGHTEN_WEAR_RESTORE).max(0.0);
        self.chain_tightened = true;
        ctx.audio.play("ui/notify");
        Some(format!(
            "Chains tightened in {} minutes. It is {}. {}",
            fmt_f(minutes, 0),
            clock_text(self.trip.local_hour()),
            deadline_text(self, ctx)
        ))
    }

    /// Take the chains off at a stop (on duty).
    /// Shared by the pause menu and facility stopping assistance.
    pub fn do_remove_chains(&mut self, ctx: &mut GameContext) -> Option<String> {
        if self.trip.truck.speed_mph() > 3.0 {
            ctx.say("Come to a complete stop first.");
            return None;
        }
        let minutes = self.hand_work_min(ctx, CHAIN_REMOVE_MIN);
        advance_on_duty_clock(
            self,
            ctx,
            minutes,
            Some("on_duty_not_driving"),
            "remove chains",
        );
        {
            let p = profile_mut_of(ctx);
            p.fatigue = (p.fatigue + CHAIN_REMOVE_FATIGUE).min(100.0);
        }
        self.trip.truck.chains_on = false;
        self.chain_set_event(ChainSetEvent::Removed);
        self.chain_bare_cue_pending = false;
        ctx.audio.play("ui/notify");
        let wear = self.trip.truck.chain_wear_pct;
        let state_word = if wear >= 75.0 {
            "The set is about done.".to_string()
        } else if wear >= 1.0 {
            format!("The set is {} percent worn.", fmt_f(wear, 0))
        } else {
            "The set is still fresh.".to_string()
        };
        let text = format!(
            "Chains off in {} minutes. {state_word} It is {}. {}",
            fmt_f(minutes, 0),
            clock_text(self.trip.local_hour()),
            deadline_text(self, ctx)
        );
        Some(text)
    }

    /// Facility stopping assistance's roadside chain work: it pulls over to
    /// put chains on before a chain-law area that chains the rig, takes them
    /// off once they have run bare road (`ChainSet`'s cue), tightens a slapping chain and
    /// fixes a broken one. Run with the other assists' pedals, ahead of the
    /// physics. It only spends on-duty minutes: it never buys chains or
    /// links, chooses a route, or chains up outside a chain-law area.
    /// `driver_input` is the driver's own brake or throttle this frame.
    pub fn update_chain_stop_assist(&mut self, ctx: &mut GameContext, driver_input: bool) {
        if !ctx.settings.destination_approach_assist {
            // Turned off mid-drive: anything pending goes with it. A bare-road
            // cue it had not answered yet is the driver's now.
            self.chain_stop = None;
            self.chain_stop_brake = 0.0;
            self.chain_assist_snapped = false;
            if std::mem::take(&mut self.chain_bare_cue_pending) && self.trip.truck.chains_on {
                self.say_bare_road(ctx);
            }
            return;
        }
        if self.departure_chain || self.stop_chain.is_some() || self.ramp_mi.is_some() {
            return;
        }
        if self.chain_stop.is_some() {
            self.run_chain_stop(ctx, driver_input);
            return;
        }
        self.arm_chain_stop(ctx, driver_input);
    }

    fn arm_stop(&mut self, ctx: &mut GameContext, kind: ChainStopKind, at_mi: f64, held: bool) {
        self.chain_stop = Some(ChainStop {
            kind,
            at_mi,
            held_input: held,
        });
        self.chain_stop_brake = 0.0;
        // Cruise and the keeper hand back to the brake; they resume once
        // rolling again, under #437's chain cap while the chains are on.
        self.pause_speed_control(ctx, true);
    }

    /// Where a stop asked for now can brake to at a comfortable rate.
    fn chain_stop_point(&self) -> f64 {
        let v = self.trip.truck.velocity_mps.max(0.0);
        let meters = v * v / (2.0 * CHAIN_STOP_DECEL_MPS2);
        self.trip.position_mi + (meters / METERS_PER_MILE).max(0.15)
    }

    fn say_chain_stop(ctx: &mut GameContext, line: String) {
        ctx.say_event_with(line, SayEvent::new().category(SpeechCategory::Navigation));
    }

    fn arm_chain_stop(&mut self, ctx: &mut GameContext, held: bool) {
        let here = self.trip.position_mi;
        // B4: a chain broke. Pull over and fix it with a spare link.
        if self.chain_assist_snapped {
            self.chain_assist_snapped = false;
            if profile_of(ctx).chains_owned() {
                if profile_of(ctx).chain_repair_links() <= 0 {
                    Self::say_chain_stop(
                        ctx,
                        "No repair links left. The chain stays off.".to_string(),
                    );
                    return;
                }
                let at = self.chain_stop_point();
                self.arm_stop(ctx, ChainStopKind::Repair, at, held);
                Self::say_chain_stop(
                    ctx,
                    format!(
                        "Chain broke. Pulling over to fix it with a spare link, about {} \
                         minutes on duty.",
                        fmt_f(self.hand_work_min(ctx, CHAIN_REPAIR_MIN), 0)
                    ),
                );
                return;
            }
        }
        let in_area = self.trip.chain_law_area_at(here);
        // B3: the slap warning, inside a chain area, once per install.
        if self.trip.truck.chains_on
            && self.chain_slap_said
            && !self.chain_tightened
            && !self.chain_assist_tighten_tried
            && in_area.is_some()
        {
            self.chain_assist_tighten_tried = true;
            let at = self.chain_stop_point();
            self.arm_stop(ctx, ChainStopKind::Tighten, at, held);
            Self::say_chain_stop(
                ctx,
                format!(
                    "Chain slapping. Pulling over to tighten, about {} minutes on duty.",
                    fmt_f(self.hand_work_min(ctx, CHAIN_TIGHTEN_MIN), 0)
                ),
            );
            return;
        }
        // B1: chain up before an area that chains this rig.
        let next = self
            .trip
            .chain_law_areas
            .iter()
            .enumerate()
            .find(|(_, (start, _))| *start >= here && *start - here <= CHAIN_STOP_ARM_MI)
            .map(|(i, (start, end))| (i, *start, *end));
        if let Some((area, start, end)) = next {
            let probe = (start + CHAIN_LAW_LEAD_MI).min(end);
            if !self.chain_stop_areas_done.contains(&area)
                && !self.trip.truck.chains_on
                && !self.trip.rig_compliant_at(probe)
            {
                self.chain_stop_areas_done.insert(area);
                if !self.chains_aboard(ctx) {
                    Self::say_chain_stop(
                        ctx,
                        "No chains aboard. Facility stopping can't chain you up.".to_string(),
                    );
                    return;
                }
                let minutes = self.chain_install_min(ctx);
                let window = limits(&ctx.settings.hos_mode)
                    .map(|(_, duty_limit, _)| duty_limit - hos_of(ctx).duty_min);
                if let Some(left) =
                    window.filter(|left| *left < minutes + CHAIN_STOP_HOS_MARGIN_MIN)
                {
                    Self::say_chain_stop(
                        ctx,
                        format!(
                            "Chain-up needs about {} minutes and your duty window has {} \
                             left. Find a place to stop.",
                            fmt_f(minutes, 0),
                            fmt_f(left.max(0.0), 0)
                        ),
                    );
                    return;
                }
                let at = start.max(self.chain_stop_point().min(start + CHAIN_LAW_LEAD_MI));
                self.arm_stop(ctx, ChainStopKind::Install, at, held);
                Self::say_chain_stop(
                    ctx,
                    format!(
                        "Chain-up area. Stopping to put chains on, about {} minutes on \
                         duty.",
                        fmt_f(minutes, 0)
                    ),
                );
                return;
            }
        }
        // B2: chain off once the chains have run bare road (`ChainSet`'s
        // cue, inside a chain-law area or out of one: on a bare road no law
        // is posted to keep them on for).
        if std::mem::take(&mut self.chain_bare_cue_pending) && self.trip.truck.chains_on {
            let at = self.chain_stop_point();
            self.arm_stop(ctx, ChainStopKind::Remove, at, held);
            Self::say_chain_stop(
                ctx,
                format!(
                    "Bare road. Taking chains off, about {} minutes on duty.",
                    fmt_f(self.hand_work_min(ctx, CHAIN_REMOVE_MIN), 0)
                ),
            );
        }
    }

    /// Brake for the pending chain stop and, stopped, do the work.
    fn run_chain_stop(&mut self, ctx: &mut GameContext, driver_input: bool) {
        let Some(stop) = self.chain_stop.clone() else {
            return;
        };
        if driver_input && !stop.held_input {
            // The driver takes it back; that step isn't retried.
            self.chain_stop = None;
            self.chain_stop_brake = 0.0;
            Self::say_chain_stop(ctx, "Chain stop canceled. Chains are yours.".to_string());
            return;
        }
        if !driver_input {
            if let Some(pending) = self.chain_stop.as_mut() {
                pending.held_input = false;
            }
        }
        if self.trip.truck.speed_mph() <= 3.0 {
            self.chain_stop = None;
            self.chain_stop_brake = 0.0;
            let truck = &mut self.trip.truck;
            truck.throttle = 0.0;
            truck.brake = 1.0;
            truck.velocity_mps = 0.0;
            truck.set_parking_brake();
            let text = match stop.kind {
                ChainStopKind::Install => self.do_install_chains(ctx),
                ChainStopKind::Remove => self.do_remove_chains(ctx),
                ChainStopKind::Tighten => self.do_tighten_chains(ctx),
                ChainStopKind::Repair => self.do_repair_chain(ctx),
            };
            if let Some(text) = text {
                Self::say_chain_stop(ctx, format!("{text} Release the parking brake to go on."));
            }
            return;
        }
        let gap_m = ((stop.at_mi - self.trip.position_mi) * METERS_PER_MILE).max(5.0);
        let v = self.trip.truck.velocity_mps.max(0.0);
        let needed = (v * v / (2.0 * gap_m)).max(0.3);
        self.chain_stop_brake = assist_servo_brake(self.chain_stop_brake, needed, &self.trip.truck);
        let truck = &mut self.trip.truck;
        truck.throttle = 0.0;
        truck.brake = truck.brake.max(self.chain_stop_brake);
    }
}
