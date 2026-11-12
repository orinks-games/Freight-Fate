//! Live cattle at the shipper: what the driver hears when the gate shuts,
//! and the choice to wait out the heat of the day before loading.

use ff_core::pyfmt::fmt_grouped;
use ff_core::sim::livestock::{danger_temp_f, head_text, pounds, CattleKind, LIVESTOCK_CARGO_KEY};
use ff_core::sim::season::temperature_c;

use crate::app::GameContext;
use crate::states::base::MenuItem;
use crate::states::city::{profile, profile_mut};
use crate::states::city_pickup::PickupFacilityState;

/// Haulers load in the cool of the evening in summer. The wait runs to
/// this local hour. ASSUMED: the extension guides say evening or night
/// without naming an hour.
const EVENING_LOAD_HOUR: f64 = 20.0;
/// The first hour of the day a hot dock offers the evening load.
const HOT_DAY_FROM_HOUR: f64 = 10.0;

impl PickupFacilityState {
    fn is_cattle(&self) -> bool {
        self.job.cargo.key == LIVESTOCK_CARGO_KEY
    }

    /// "Loaded for Dodge City. 89 head of feeder calves, about 48,900 pounds.
    /// Health papers are in the cab." None for any other freight.
    pub(crate) fn cattle_loaded_line(&self) -> Option<String> {
        if !self.is_cattle() {
            return None;
        }
        let kind = CattleKind::for_origin(&self.job.origin_type);
        let head = kind.head_for_tons(self.job.weight_tons);
        // Feeder calves cross state lines on a vet's certificate; fed
        // cattle going straight to the plant do not need one.
        let papers = if kind == CattleKind::FeederCalves {
            " Health papers are in the cab."
        } else {
            ""
        };
        Some(format!(
            "Loaded for {}. {}, about {} pounds.{papers}",
            self.job.spoken_destination(),
            head_text(head, kind),
            fmt_grouped((pounds(self.job.weight_tons) / 100.0).round() * 100.0, 0)
        ))
    }

    /// The calendar hour at the dock, when the career's own calendar runs
    /// it. The real clock cannot be waited out from a menu.
    fn dock_hour(&self, ctx: &GameContext) -> Option<f64> {
        let p = profile(ctx);
        if ctx.settings.time_scale == 1.0 || p.live_calendar {
            return None;
        }
        Some(p.calendar_game_hours())
    }

    /// Hours to wait for the evening, when the dock is hot now.
    fn evening_wait_h(&self, ctx: &GameContext) -> Option<f64> {
        if !self.is_cattle() || self.loaded || !self.checked_in {
            return None;
        }
        let hours = self.dock_hour(ctx)?;
        let hour = hours.rem_euclid(24.0);
        if !(HOT_DAY_FROM_HOUR..EVENING_LOAD_HOUR).contains(&hour) {
            return None;
        }
        let region = &ctx.world.city(&self.job.origin).ok()?.region;
        let temp_f = temperature_c(region, hours) * 9.0 / 5.0 + 32.0;
        (temp_f >= danger_temp_f()).then_some(EVENING_LOAD_HOUR - hour)
    }

    /// "Load this evening", on a hot afternoon at a cattle dock.
    pub(crate) fn evening_load_row(&self, ctx: &GameContext) -> Option<MenuItem<Self>> {
        self.evening_wait_h(ctx)?;
        Some(
            MenuItem::new("Load this evening", |s: &mut Self, ctx| {
                s.load_this_evening(ctx)
            })
            .help(
                "Wait off duty until 8 in the evening, then load. The cattle ride cooler, and \
                 the 28 hours start later.",
            ),
        )
    }

    fn load_this_evening(&mut self, ctx: &mut GameContext) {
        let Some(wait_h) = self.evening_wait_h(ctx) else {
            return;
        };
        let facility = self.facility();
        {
            let p = profile_mut(ctx);
            let start = p.game_hours;
            p.game_hours += wait_h;
            let end = p.game_hours;
            p.duty_log
                .record("off_duty", start, end, &facility, "waiting for the evening");
            p.hos.off_duty(wait_h * 60.0);
        }
        self.load(ctx);
    }
}
