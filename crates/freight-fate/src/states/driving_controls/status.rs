//! The Tab status screen's browse, and the two readouts it shares with the
//! spoken keys: the gear name and the air-brake sentence.

use ff_core::models::business::display_rank_for;
use ff_core::models::career::xp_to_next_level;
use ff_core::pyfmt::fmt_grouped;

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::*;

use super::WEAR_STATUS_PCT;

impl DrivingState {
    /// `_gear_text()`.
    pub fn gear_text(&self) -> String {
        let tr = &self.trip.truck.transmission;
        if tr.in_neutral() {
            return "neutral".to_string();
        }
        if tr.in_reverse() {
            return "reverse".to_string();
        }
        format!("gear {}", tr.gear)
    }

    /// `_career_status_line()`: level, rank, and the number still owed to the
    /// next level.
    ///
    /// The owed figure shipped into `Career.summary()` on 2026-08-17 -- built
    /// from Brandon's report of that day -- and `summary()` turned out to have
    /// no callers at all, so the answer existed and nothing spoke it. He asked
    /// again on 2026-08-20 ("this is still not in this build"), and he was
    /// exactly right. The status browse is where a driver asks mid-run.
    pub fn career_status_line(&self, ctx: &GameContext) -> String {
        let profile = profile_of(ctx);
        let career = &profile.career;
        let owed = xp_to_next_level(career.xp);
        let level = career.level();
        let title = display_rank_for(profile).title;
        let tail = match owed {
            Some(owed) => format!("{} experience to level {}", fmt_grouped(owed, 0), level + 1),
            None => "top career level".to_string(),
        };
        format!("Career: level {level}, {title}. {tail}.")
    }

    /// `status_lines()`: the Tab status screen's browse.
    pub fn status_lines(&mut self, ctx: &mut GameContext) -> Vec<String> {
        let position = self.trip.position_mi;
        let (limit, reason) = self.trip.speed_limit_at(position);
        let imperial = ctx.settings.imperial_units;
        let progress = if self.phase == DRIVE_PHASE_PICKUP {
            self.pickup_progress_summary(ctx)
        } else {
            self.trip.progress_summary(imperial)
        };
        let zone = match reason {
            Some(reason) if !ff_core::sim::trip::is_street_zone_reason(&reason) => {
                format!(" in a {reason} zone")
            }
            _ => String::new(),
        };
        let calendar = self.calendar_phrase(ctx);
        let calendar = if calendar.is_empty() {
            "unknown".to_string()
        } else {
            calendar
        };
        let mut lines = vec![
            format!(
                "Speed: {}",
                ctx.settings.speed_text(self.trip.truck.speed_mph())
            ),
            format!("Limit: {}{zone}", ctx.settings.speed_text(limit)),
            self.trip.npc_traffic_status(),
            format!(
                "Progress: {} percent there",
                self.journey_progress_percent()
            ),
            format!("Route: {progress}"),
            self.career_status_line(ctx),
            format!(
                "Fuel: {:.0} percent",
                self.trip.truck.fuel_fraction() * 100.0
            ),
            format!("Air brakes: {}", self.air_status_text(true)),
            format!("Weather: {}", self.trip.weather.report_lead(imperial)),
            format!("Radio: {}", self.radio.status_text()),
            format!("Calendar: {calendar}"),
            format!(
                "Clock: {} {} ({})",
                clock_text(self.trip.local_hour()),
                self.clock_zone_label(ctx),
                time_of_day(self.trip.local_hour())
            ),
        ];
        if self.cruise_mph.is_some() {
            // The same live-then-set shape the keeper's row below uses: when
            // anything holds cruise under its set speed, the status screen has
            // to say which of the two numbers is real.
            lines.insert(1, format!("Cruise: {}", self.cruise_holding_text(ctx)));
        } else if self.keeper_mph.is_some() {
            lines.insert(
                1,
                format!("Speed control: {}", self.keeper_holding_text(ctx)),
            );
            lines.insert(
                2,
                format!("Open-road target: {}", self.open_road_target(ctx)),
            );
        } else if self.speed_control_armed {
            lines.insert(
                1,
                "Speed control: paused; resumes when the truck is rolling".to_string(),
            );
            lines.insert(
                2,
                format!("Open-road target: {}", self.open_road_target(ctx)),
            );
        }
        let truck = &self.trip.truck;
        if truck.damage_pct - self.start_damage > 1.0 {
            lines.push(format!(
                "Damage: new damage {:.0} percent",
                truck.damage_pct - self.start_damage
            ));
        }
        for (worn, label) in [
            (truck.tire_wear_pct, "Tires"),
            (truck.brake_wear_pct, "Brakes"),
            (truck.engine_wear_pct, "Engine"),
        ] {
            if worn >= WEAR_STATUS_PCT {
                if worn >= ff_core::sim::vehicle::COMPONENT_SERVICE_WARNING_PCT {
                    let component = match label {
                        "Tires" => crate::states::driving_damage::MaintenanceComponent::Tires,
                        "Brakes" => crate::states::driving_damage::MaintenanceComponent::Brakes,
                        _ => crate::states::driving_damage::MaintenanceComponent::Engine,
                    };
                    lines.push(crate::states::driving_damage::maintenance_status_line(
                        component, worn,
                    ));
                } else {
                    lines.push(format!("{label}: {worn:.0} percent worn"));
                }
            }
        }
        let now_h = self.absolute_game_hour(ctx, None);
        for entry in &profile_of(ctx).active_buffs {
            let left_h = entry
                .get("expires_h")
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0)
                - now_h;
            if left_h <= 0.0 {
                continue;
            }
            let left = if left_h >= 1.05 {
                format!("about {left_h:.0} hours left")
            } else {
                format!("about {:.0} minutes left", left_h * 60.0)
            };
            let label = entry
                .get("label")
                .and_then(|v| v.as_str())
                .unwrap_or("Buff");
            lines.push(format!("{label}: {left}"));
        }
        for info in self.rig_buffs.values() {
            lines.push(format!("{}: good for the rest of the trip", info.label));
        }
        if !ctx.settings.renders_terse() {
            let fatigue = profile_of(ctx).fatigue;
            if fatigue >= hos::FATIGUE_DROWSY {
                lines.push(format!("Fatigue: {fatigue:.0} percent"));
            }
            let mode = ctx.settings.hos_mode.clone();
            let summary = hos_of(ctx).summary(&mode);
            lines.push(format!("HOS: {}", summary.trim_end_matches('.')));
            let context = self.hos_route_context(ctx);
            if !context.is_empty() {
                lines.push(format!("HOS route: {context}"));
            }
        }
        lines
    }

    /// The open-road target the keeper's two status rows both print.
    fn open_road_target(&self, ctx: &GameContext) -> String {
        match self.speed_control_target_mph {
            Some(mph) => ctx.settings.speed_text(mph),
            None => "posted limit when the open road begins".to_string(),
        }
    }

    /// The air for the Space readout: the pressure, and only what is not
    /// normal. Ready air, a released parking brake and an idle compressor are
    /// the state of every moving truck, so saying them on each press buried
    /// the speed under them. The full set stays on Tab and the pause menu.
    /// Empty while the air is ready and the brakes are released: a driver
    /// looks at the gauge when the buzzer goes, not every glance at the dash
    /// (owner, 2026-10-14). The Tab screen keeps the full reading.
    pub fn air_brief_text(&self) -> String {
        let t = &self.trip.truck;
        let mut parts = Vec::new();
        if t.air_low_warning() {
            parts.push(format!("low air, {:.0} psi", t.air_pressure_psi()));
        } else if !t.air_ready() {
            parts.push(format!("air building, {:.0} psi", t.air_pressure_psi()));
        }
        if t.spring_brakes_active() {
            parts.push("spring brakes active".to_string());
        } else if t.parking_brake {
            parts.push("parking brake set".to_string());
        }
        parts.join(", ")
    }

    /// `_air_status_text(detailed=False)`.
    pub fn air_status_text(&self, detailed: bool) -> String {
        let t = &self.trip.truck;
        let brake = if t.spring_brakes_active() {
            "spring brakes active"
        } else if t.parking_brake {
            "parking brake set"
        } else {
            "parking brake released"
        };
        let pressure = if t.air_low_warning() {
            "low air"
        } else if t.air_ready() {
            "air ready"
        } else {
            "air building"
        };
        let compressor = if t.air_compressor_active {
            "compressor building"
        } else {
            "compressor idle"
        };
        let heat = if t.brake_temp_c >= t.specs.brake_fade_temp_c {
            "brakes hot"
        } else if t.brake_temp_c >= 180.0 {
            "brakes warm"
        } else {
            "brakes cool"
        };
        if detailed {
            return format!(
                "primary {:.0} psi, secondary {:.0} psi, trailer {:.0} psi, {pressure}, {brake}, \
                 {compressor}, {heat}",
                t.primary_air_psi, t.secondary_air_psi, t.trailer_air_psi
            );
        }
        format!(
            "air {:.0} psi, {pressure}, {brake}, {compressor}",
            t.air_pressure_psi()
        )
    }
}
