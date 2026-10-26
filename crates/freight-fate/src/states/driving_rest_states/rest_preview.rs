//! Spoken forecast for a rest-stop sleep choice. This simulates the ELD on a
//! clone so a preview cannot change the driver's clock, deadline, or fatigue.

use ff_core::pyfmt::fmt_f;
use ff_core::sim::hos;

use crate::app::GameContext;
use crate::states::driving::DrivingState;
use crate::states::driving_core::{clock_text, profile_of, MOTEL_COST};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SleepChoice {
    Sleeper(i64),
    Lot,
    Motel,
}

impl SleepChoice {
    fn hours(self) -> i64 {
        match self {
            Self::Sleeper(hours) => hours,
            Self::Lot | Self::Motel => 10,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Sleeper(_) => "sleeper berth",
            Self::Lot => "the lot",
            Self::Motel => "a motel room",
        }
    }
}

pub(super) fn sleep_preview(d: &DrivingState, ctx: &GameContext, choice: SleepChoice) -> String {
    let profile = profile_of(ctx);
    // The reset row sleeps only what is left of a rest already under way, so
    // the forecast uses the same minutes the sleep will spend.
    let owed = match choice {
        SleepChoice::Sleeper(10) => profile.hos.reset_minutes_left(),
        _ => None,
    };
    let minutes = owed.unwrap_or(choice.hours() as f64 * 60.0);
    let span = hos::duration_text(minutes / 60.0);
    let more = if owed.is_some() { " more" } else { "" };
    let mut after = profile.hos.clone();
    let completed_split = match choice {
        SleepChoice::Sleeper(10) | SleepChoice::Lot | SleepChoice::Motel => {
            after.sleep();
            false
        }
        SleepChoice::Sleeper(_) => after.sleeper_split_rest(minutes),
    };
    let full_reset = after
        .history
        .last()
        .is_some_and(|event| event.source == "full_reset");
    let fatigue_after = match choice {
        SleepChoice::Sleeper(_) if full_reset => hos::rest_sleep(profile.fatigue),
        SleepChoice::Motel => hos::rest_sleep(profile.fatigue),
        SleepChoice::Lot => hos::rest_shoulder(profile.fatigue),
        SleepChoice::Sleeper(_) => {
            hos::rest_sleeper_split(profile.fatigue, minutes, completed_split)
        }
    };
    let effect = match choice {
        SleepChoice::Lot | SleepChoice::Motel => {
            "A full 10-hour sleep resets your driving hours and legal driving window."
        }
        SleepChoice::Sleeper(_) if full_reset => {
            "A full 10-hour sleep resets your driving hours and legal driving window."
        }
        SleepChoice::Sleeper(_) if completed_split => {
            "This completes a sleeper split and credits the paired rests."
        }
        SleepChoice::Sleeper(7 | 8) => {
            "Your legal driving window pauses while you sleep. This rest alone does not restore driving time; pairing the split can."
        }
        SleepChoice::Sleeper(_) => {
            "This sleep alone does not restore driving time. Your legal driving window keeps counting until the split is paired."
        }
    };
    let wake_hour = d.trip.local_hour() + minutes / 60.0;
    let legal = match hos::limits(&ctx.settings.hos_mode) {
        Some((drive_limit, duty_limit, _)) => {
            let drive_left = (drive_limit - after.driving_min).max(0.0) / 60.0;
            let duty_left = (duty_limit - after.duty_min).max(0.0) / 60.0;
            if duty_left <= 0.0 {
                format!(
                    "Afterward, driving allowance is {} hours, but your legal driving window is closed.",
                    fmt_f(drive_left, 1)
                )
            } else {
                let cutoff = clock_text((wake_hour + duty_left) % 24.0);
                format!(
                    "Afterward, driving allowance is {} hours; your legal driving window closes in {} hours at {cutoff}.",
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
    let pending = after
        .split_pending_summary()
        .map(|summary| format!(" {summary}"))
        .unwrap_or_default();
    let cost = if choice == SleepChoice::Motel {
        format!(" The room costs {} dollars.", fmt_f(MOTEL_COST, 0))
    } else {
        String::new()
    };
    format!(
        "Sleep {span}{more} at {}. {effect} {legal} Fatigue {} to {}. Clock +{span}. \
         {deadline}{pending}{cost} Select again to sleep.",
        choice.name(),
        fmt_f(profile.fatigue, 0),
        fmt_f(fatigue_after, 0)
    )
}
