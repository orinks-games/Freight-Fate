//! Spoken forecast for a rest-stop sleep choice. This simulates the ELD on a
//! clone so a preview cannot change the driver's clock, deadline, or fatigue.

use ff_core::pyfmt::fmt_f;
use ff_core::sim::hos::{self, RestartOffer};

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::{clock_text, full_night_of, profile_of, MOTEL_COST};

/// Said in a sleep's forecast when live cattle would pass the 28-hour limit
/// before the driver wakes.
pub const CATTLE_LIMIT_IN_SLEEP_TEXT: &str = "The cattle pass the 28-hour limit while you sleep.";

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SleepChoice {
    Sleeper(i64),
    Lot,
    Motel,
    /// The cycle restart, in the sleeper berth or, `lot`, in the lot.
    Restart {
        lot: bool,
    },
}

impl SleepChoice {
    /// Hours this choice sleeps; a full reset sleeps the hours the rules in
    /// force reset on (`reset_hours`: 10, or 8 in Canada).
    fn hours(self, reset_hours: i64) -> i64 {
        match self {
            Self::Sleeper(10) | Self::Lot | Self::Motel => reset_hours,
            Self::Sleeper(hours) => hours,
            // The restart's minutes come from its offer, never from here.
            Self::Restart { .. } => 0,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Sleeper(_) | Self::Restart { lot: false } => "sleeper berth",
            Self::Lot | Self::Restart { lot: true } => "the lot",
            Self::Motel => "a motel room",
        }
    }
}

pub(super) fn sleep_preview(d: &DrivingState, ctx: &GameContext, choice: SleepChoice) -> String {
    let profile = profile_of(ctx);
    let night = full_night_of(ctx, d);
    // The reset row sleeps only what is left of a rest already under way, so
    // the forecast uses the same minutes the sleep will spend.
    let owed = match choice {
        SleepChoice::Sleeper(10) => profile.hos.reset_minutes_left(night),
        _ => None,
    };
    // The restart rests what is left of it, the same minutes the pick takes.
    let restart = match choice {
        SleepChoice::Restart { .. } => Some(profile.hos.restart_offer(&ctx.settings.hos_mode)),
        _ => None,
    };
    let reset_hours = night.hours();
    let minutes = match restart {
        Some(offer) => offer.minutes(),
        None => owed.unwrap_or(choice.hours(reset_hours) as f64 * 60.0),
    };
    let span = hos::duration_text(minutes / 60.0);
    let more = if owed.is_some() || matches!(restart, Some(RestartOffer::Finish { .. })) {
        " more"
    } else {
        ""
    };
    let mut after = profile.hos.clone();
    let completed_split = match choice {
        SleepChoice::Sleeper(10) | SleepChoice::Lot | SleepChoice::Motel => {
            after.rest_reset(night);
            false
        }
        SleepChoice::Restart { lot } => {
            after.finish_restart(restart_status(lot));
            false
        }
        SleepChoice::Sleeper(_) => after.sleeper_split_rest(minutes),
    };
    // An 8-hour Canadian night ends the Canadian counts without the US
    // full_reset record, and is a full night all the same.
    let full_reset = matches!(
        choice,
        SleepChoice::Sleeper(10) | SleepChoice::Restart { .. }
    ) || after
        .history
        .last()
        .is_some_and(|event| event.source == "full_reset");
    // A coffee or drink that wears off during the sleep is paid back first,
    // as the sleep itself does it.
    let wake_h = d.absolute_game_hour(ctx, None) + minutes / 60.0;
    let lying_down = (profile.fatigue + profile.stimulant_loan().owed_by(wake_h)).min(100.0);
    let fatigue_after = match choice {
        SleepChoice::Restart { lot: false } => hos::rest_sleep(lying_down, minutes),
        SleepChoice::Restart { lot: true } => hos::rest_poor_sleep(lying_down, minutes),
        SleepChoice::Sleeper(_) if full_reset => hos::rest_sleep(lying_down, minutes),
        SleepChoice::Motel => hos::rest_sleep(lying_down, minutes),
        SleepChoice::Lot => hos::rest_poor_sleep(lying_down, minutes),
        SleepChoice::Sleeper(_) => hos::rest_sleeper_split(lying_down, minutes, completed_split),
    };
    let full_sleep_line = if reset_hours == 8 {
        "A full 8-hour sleep resets your driving hours and legal driving window."
    } else {
        "A full 10-hour sleep resets your driving hours and legal driving window."
    };
    let restart_effect = restart.map(RestartOffer::effect_text);
    let effect = match choice {
        SleepChoice::Restart { .. } => restart_effect.as_deref().unwrap_or(full_sleep_line),
        SleepChoice::Lot | SleepChoice::Motel => full_sleep_line,
        SleepChoice::Sleeper(_) if full_reset => full_sleep_line,
        SleepChoice::Sleeper(_) if completed_split => {
            "This finishes a split rest, and both parts count."
        }
        SleepChoice::Sleeper(7 | 8) => {
            "Your legal driving window pauses while you sleep. This rest alone does not restore driving time. Pairing the split can."
        }
        SleepChoice::Sleeper(_) => {
            "This sleep alone does not restore driving time. Your legal driving window keeps counting until the split is paired."
        }
    };
    let wake_hour = d.trip.local_hour() + minutes / 60.0;
    let legal = match after.drive_and_window_left_min(&ctx.settings.hos_mode) {
        Some((drive_left, duty_left)) => {
            let drive_left = drive_left / 60.0;
            let duty_left = duty_left / 60.0;
            if duty_left <= 0.0 {
                format!(
                    "Afterward, driving allowance is {} hours, but your legal driving window is closed.",
                    fmt_f(drive_left, 1)
                )
            } else {
                let cutoff = clock_text((wake_hour + duty_left) % 24.0);
                format!(
                    "Afterward, driving allowance is {} hours. Your legal driving window closes in {} hours at {cutoff}.",
                    fmt_f(drive_left, 1),
                    fmt_f(duty_left, 1)
                )
            }
        }
        None => "Hours of service enforcement is off.".to_string(),
    };
    let deadline_left = d.job.deadline_game_h - (d.trip.game_minutes + minutes) / 60.0;
    let deadline = if deadline_left > 0.0 {
        format!(
            "The delivery deadline will be in {} hours.",
            fmt_f(deadline_left, 1)
        )
    } else if deadline_left < 0.0 {
        format!(
            "You will be {} hours past the delivery deadline.",
            fmt_f(-deadline_left, 1)
        )
    } else {
        "The delivery deadline will have arrived.".to_string()
    };
    // A sleep that carries live cattle past their 28-hour limit costs the
    // load every hour past it; the forecast is the last place to hear it.
    let cattle = d
        .livestock()
        .filter(|load| load.aboard())
        .map(|load| load.limit_left_min())
        .filter(|left| *left > 0.0 && *left <= minutes)
        .map(|_| format!(" {CATTLE_LIMIT_IN_SLEEP_TEXT}"))
        .unwrap_or_default();
    let pending = after
        .split_pending_spoken()
        .map(|summary| format!(" {summary}"))
        .unwrap_or_default();
    let cost = if choice == SleepChoice::Motel {
        format!(" The room costs {} dollars.", fmt_f(MOTEL_COST, 0))
    } else {
        String::new()
    };
    if restart.is_some() {
        return format!(
            "Rest {span}{more} at {}. {effect} {legal} Fatigue {} to {}. Clock moves ahead \
             {span}. {deadline}{cattle} Select again to rest.",
            choice.name(),
            fmt_f(profile.fatigue, 0),
            fmt_f(fatigue_after, 0)
        );
    }
    format!(
        "Sleep {span}{more} at {}. {effect} {legal} Fatigue {} to {}. Clock moves ahead {span}. \
         {deadline}{cattle}{pending}{cost} Select again to sleep.",
        choice.name(),
        fmt_f(profile.fatigue, 0),
        fmt_f(fatigue_after, 0)
    )
}

/// The duty status a restart is logged under: the berth, or off duty in
/// the lot, the way the lot's night is logged.
pub(super) fn restart_status(lot: bool) -> &'static str {
    if lot {
        "off_duty"
    } else {
        "sleeper_berth"
    }
}
