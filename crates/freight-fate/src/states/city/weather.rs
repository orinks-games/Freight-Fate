//! The terminal's "Time and weather" screen (`CityMenuState._time_weather`):
//! the clock, the calendar, the career day, and the sky over the city, one
//! line each.

use ff_core::music::crc32;
use ff_core::pyfmt::fmt_f;
use ff_core::sim::hos::{clock_text, time_of_day};
use ff_core::sim::season::{
    adjust_for_calendar, date_text, player_calendar_hours, season, temperature_c,
};
use ff_core::sim::timezones::{city_zone, to_local};
use ff_core::sim::weather::{WeatherKind, WeatherSystem};

use crate::app::GameContext;
use crate::states::city::{game_hours_int, profile};

/// What the live provider knows about one city right now.
struct LiveReading {
    kind: Option<WeatherKind>,
    last_known: bool,
    observation_age: Option<f64>,
    refreshing: bool,
    loading: bool,
    observed_temperature: Option<f64>,
}

pub(crate) fn time_and_weather_lines(ctx: &mut GameContext) -> Vec<String> {
    let world = ctx.world;
    let (city_key, city_name, region, lat, lon, game_hours, calendar_hours, day) = {
        let p = profile(ctx);
        let city = world.city(&p.current_city);
        let (key, name, region, lat, lon) = match city {
            Ok(c) => (
                c.key.clone(),
                c.name.clone(),
                c.region.clone(),
                c.lat,
                c.lon,
            ),
            Err(_) => (
                p.current_city.clone(),
                p.current_city.clone(),
                String::new(),
                0.0,
                0.0,
            ),
        };
        (
            key,
            name,
            region,
            lat,
            lon,
            p.game_hours,
            p.calendar_game_hours(),
            p.market_day() + 1,
        )
    };
    let zone = match world.city(&city_key) {
        Ok(city) => city_zone(city),
        Err(_) => ff_core::sim::timezones::EASTERN,
    };
    let hour = to_local(game_hours, zone).rem_euclid(24.0);
    let live_calendar_setting = ctx.settings.live_weather_controls_calendar;
    let imperial = ctx.settings.imperial_units;
    let real_weather_on = ctx.settings.real_weather;
    let reading: Option<LiveReading> = ctx.real_weather_provider().map(|provider| {
        // Keyed by the city key, not the spoken name: two cities can share
        // a spoken name but they are different places with different skies.
        provider.request(&city_key, lat, lon);
        let kind = provider.get(&city_key);
        match kind {
            Some(kind) => LiveReading {
                kind: Some(kind),
                last_known: provider.stale(&city_key),
                observation_age: provider.observation_age_s(&city_key),
                refreshing: provider.refreshing(&city_key),
                loading: false,
                observed_temperature: provider.get_temperature(&city_key),
            },
            None => LiveReading {
                kind: None,
                last_known: false,
                observation_age: None,
                refreshing: false,
                loading: !provider.unavailable(&city_key),
                observed_temperature: None,
            },
        }
    });
    let has_provider = reading.is_some();
    let live = reading.as_ref().is_some_and(|r| r.kind.is_some());
    let loading = reading.as_ref().is_some_and(|r| r.loading);
    let last_known = reading.as_ref().is_some_and(|r| r.last_known);
    let refreshing = reading.as_ref().is_some_and(|r| r.refreshing);
    let observation_age = reading.as_ref().and_then(|r| r.observation_age);
    // Live conditions and the calendar are separate player choices. The
    // legacy default follows today's real date; an independent calendar
    // advances with career time even while conditions remain live.
    let season_hours = player_calendar_hours(
        game_hours,
        Some(calendar_hours),
        has_provider && live_calendar_setting,
    );
    let mut desc: Option<String> = None;
    if live {
        let reading = reading.as_ref().expect("live implies a reading");
        let kind = reading.kind.expect("live implies a kind");
        let observed = reading.observed_temperature;
        // The calendar toggle controls date, season, and plausibility -- it
        // never replaces a real station temperature with a modeled one.
        let guard_temp = match observed {
            Some(t) if live_calendar_setting => t,
            _ => temperature_c(&region, season_hours),
        };
        let mut parts = vec![
            adjust_for_calendar(kind, Some(guard_temp), Some(season_hours))
                .value()
                .to_string(),
        ];
        match observed {
            Some(observed) => {
                if imperial {
                    parts.push(format!("{} degrees", fmt_f(observed * 9.0 / 5.0 + 32.0, 0)));
                } else {
                    parts.push(format!("{} degrees Celsius", fmt_f(observed, 0)));
                }
            }
            None => parts.push("temperature unavailable".to_string()),
        }
        desc = Some(parts.join(", "));
    }
    let desc = if loading {
        "still loading".to_string()
    } else {
        match desc {
            Some(desc) => desc,
            None => {
                // deterministic per city and hour, so asking twice agrees
                let seed = crc32(format!("{city_key}:{}", game_hours_int(game_hours)).as_bytes());
                WeatherSystem::new(
                    &region,
                    Some(i64::from(seed)),
                    None,
                    Some(season_hours),
                    true,
                )
                .describe(imperial, false)
            }
        }
    };
    let source = if last_known {
        "Last-known live weather"
    } else if live {
        "Live weather"
    } else if loading {
        "Live weather loading"
    } else if real_weather_on {
        "Simulated fallback weather"
    } else {
        "Simulated weather"
    };
    let mut freshness = String::new();
    if live {
        if let Some(age_s) = observation_age {
            let minutes = ((age_s / 60.0).floor() as i64).max(0);
            let age = if minutes < 1 {
                "less than a minute".to_string()
            } else {
                format!(
                    "{minutes} {}",
                    if minutes == 1 { "minute" } else { "minutes" }
                )
            };
            freshness = format!(" The observation is {age} old.");
        }
    }
    if last_known && refreshing {
        freshness.push_str(" Live weather is updating.");
    }
    let mut lines = vec![
        format!(
            "It is {} {}, {}.",
            clock_text(hour),
            zone.name,
            time_of_day(hour)
        ),
        format!("{}, {}.", date_text(season_hours), season(season_hours)),
        format!("Day {day} of your career."),
        format!("{source} in {city_name}: {desc}."),
    ];
    let freshness = freshness.trim();
    if !freshness.is_empty() {
        lines.push(freshness.to_string());
    }
    lines
}
