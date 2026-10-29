//! Dynamic weather with regional flavor, driving modifiers, and forecasts.
//!
//! Weather evolves as a Markov chain. Each condition carries physics
//! modifiers (grip, drag, visibility) and an ambience sound key. A
//! deterministic seed makes trips reproducible in tests.
//!
//! Two clocks: the career/season clock (night, calendar, temperature) still
//! advances on game minutes, the same drive-time law as fuel and HOS. Spoken
//! weather color -- the condition timer and thunder -- ticks on sitting
//! minutes so 20x does not spawn 20x pokes. A rest skip uses game minutes
//! for both, because the skip is the time that passed.
//!
//! Port of `freight_fate/sim/weather.py`.

use super::season::{
    adjust_for_calendar, adjust_for_temperature, date_text, real_clock_game_hours, season,
    temperature_c,
};
use crate::pyfmt::fmt_f;
use crate::pyrandom::PyRandom;

mod tables;
#[cfg(test)]
mod tests;

pub use tables::{
    effects, region_weights, RegionWeights, WeatherEffects, WeatherKind, DEFAULT_WEIGHTS, EFFECTS,
    REGION_WEIGHTS,
};

/// `weights[kind] = f(weights.get(kind))` on an insertion-ordered weight
/// table: an existing key is updated in place, a new one is appended at the
/// end, exactly as a Python dict would order it for `random.choices`.
fn set_weight(
    weights: &mut Vec<(WeatherKind, f64)>,
    kind: WeatherKind,
    f: impl FnOnce(Option<f64>) -> f64,
) {
    match weights.iter_mut().find(|(k, _)| *k == kind) {
        Some(entry) => entry.1 = f(Some(entry.1)),
        None => weights.push((kind, f(None))),
    }
}

/// A dev/testing override locking the weather to one condition, from
/// `FREIGHT_FATE_FORCE_WEATHER` (e.g. `snow`, `heavy_rain`, `fog`,
/// `wind`). Empty or unrecognized -> None (normal weather).
fn forced_weather() -> Option<WeatherKind> {
    let name = std::env::var("FREIGHT_FATE_FORCE_WEATHER")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    if name.is_empty() {
        return None;
    }
    let normalized = name.replace('_', " ");
    WeatherKind::ALL.into_iter().find(|kind| {
        normalized == kind.name().to_lowercase()
            || normalized == kind.value()
            || normalized == kind.value().replace(' ', "_")
    })
}

/// Python `format(x, "g")`: six significant digits, trailing zeros stripped,
/// exponent form outside `1e-4 <= |x| < 1e6`.
fn fmt_g(x: f64) -> String {
    if x.is_nan() {
        return "nan".to_string();
    }
    if x.is_infinite() {
        return if x < 0.0 { "-inf" } else { "inf" }.to_string();
    }
    if x == 0.0 {
        return if x.is_sign_negative() { "-0" } else { "0" }.to_string();
    }
    const PRECISION: i32 = 6;
    // Round to six significant digits first; the exponent decides the form.
    let sci = format!("{:.*e}", (PRECISION - 1) as usize, x);
    let (mantissa, exp) = sci
        .split_once('e')
        .expect("{:e} always carries an exponent");
    let exp: i32 = exp.parse().expect("{:e} exponent is an integer");
    if !(-4..PRECISION).contains(&exp) {
        let sign = if exp < 0 { '-' } else { '+' };
        format!(
            "{}e{}{:02}",
            strip_trailing_zeros(mantissa),
            sign,
            exp.abs()
        )
    } else {
        strip_trailing_zeros(&format!("{:.*}", (PRECISION - 1 - exp) as usize, x))
    }
}

fn strip_trailing_zeros(s: &str) -> String {
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s.to_string()
    }
}

/// A source of real current conditions (see `sim::real_weather`).
///
/// The Python system duck-typed its provider and probed optional methods
/// with `getattr`; here the optional ones have defaults that reproduce the
/// "attribute missing" branch: no temperature reading, never offline, never
/// stale, never refreshing, never failed, no observation age.
pub trait WeatherProvider {
    /// Start (or refresh) a fetch for `city` at the given coordinates.
    fn request(&mut self, city: &str, lat: f64, lon: f64);

    /// The current condition for `city`, or None while nothing is known.
    fn get(&mut self, city: &str) -> Option<WeatherKind>;

    /// The station temperature in Celsius, when the provider has one.
    fn get_temperature(&mut self, _city: &str) -> Option<f64> {
        None
    }

    /// Whether the provider has no usable data and a fetch has failed.
    fn unavailable(&mut self, _city: &str) -> bool {
        false
    }

    /// Whether the observation on hand is older than the live window.
    fn stale(&mut self, _city: &str) -> bool {
        false
    }

    /// Whether a fetch for `city` is in flight right now.
    fn refreshing(&mut self, _city: &str) -> bool {
        false
    }

    /// Whether the most recent refresh attempt for `city` failed.
    fn refresh_failed(&mut self, _city: &str) -> bool {
        false
    }

    /// Age of the current observation in seconds, when known.
    fn observation_age_s(&mut self, _city: &str) -> Option<f64> {
        None
    }
}

/// Evolving weather for the current region of a trip.
///
/// With a `provider` (see `sim::real_weather`) attached, real current
/// conditions for the tracked city take priority; the simulated Markov
/// weather keeps running underneath as an offline fallback.
pub struct WeatherSystem {
    rng: PyRandom,
    pub region: String,
    pub provider: Option<Box<dyn WeatherProvider>>,
    pub live_weather_controls_calendar: bool,
    /// Career clock at this point in the trip. When provided, weather is
    /// season- and temperature-aware (snow only when cold, storms only when
    /// warm); when None, the simulated draw is used as-is so seed-based
    /// tests stay deterministic. It advances with the trip in update().
    pub game_hours: Option<f64>,
    pub city: Option<String>,
    pub city_coords: (f64, f64),
    /// True while real-world data is driving conditions.
    pub live: bool,
    /// True once any live observation has driven conditions this session.
    /// From then on a failing provider holds last-known conditions --
    /// simulated weather never takes over a sky the player has heard live
    /// (owner ruling, 2026-08-08).
    session_had_live: bool,
    /// The last raw live observation and the city it was for, plus the
    /// season-reconciled condition it produced. Held so live weather is
    /// reconciled once per observation instead of re-evaluated every tick.
    /// (Public because the driving tests reset them to force a re-read.)
    pub live_raw: Option<WeatherKind>,
    pub live_city: Option<String>,
    pub live_kind: Option<WeatherKind>,
    last_observed_kind: Option<WeatherKind>,
    last_observed_temperature: Option<f64>,
    carried_last_known: bool,
    fallback_active: bool,
    /// Dev/testing override, usually None.
    pub forced: Option<WeatherKind>,
    pub current: WeatherKind,
    pub minutes_until_change: f64,
    pub thunder_cooldown: f64,
}

impl WeatherSystem {
    /// `WeatherSystem(region, seed, provider, game_hours,
    /// live_weather_controls_calendar)`; the Python defaults are
    /// `("heartland", None, None, None, True)`.
    pub fn new(
        region: &str,
        seed: Option<i64>,
        provider: Option<Box<dyn WeatherProvider>>,
        game_hours: Option<f64>,
        live_weather_controls_calendar: bool,
    ) -> Self {
        let rng = match seed {
            Some(seed) => PyRandom::new_from_i64(seed),
            None => PyRandom::new_unseeded(),
        };
        let has_provider = provider.is_some();
        let mut system = WeatherSystem {
            rng,
            region: region.to_string(),
            provider,
            live_weather_controls_calendar,
            game_hours,
            city: None,
            city_coords: (0.0, 0.0),
            live: false,
            session_had_live: false,
            live_raw: None,
            live_city: None,
            live_kind: None,
            last_observed_kind: None,
            last_observed_temperature: None,
            carried_last_known: false,
            fallback_active: false,
            forced: forced_weather(),
            current: WeatherKind::Clear,
            minutes_until_change: 0.0,
            thunder_cooldown: 0.0,
        };
        // With real weather enabled, start neutral and wait for live data rather
        // than showing a simulated warm-up condition that the real data would
        // immediately replace. Simulated weather only appears if the provider
        // turns out to be offline (see update()).
        system.current = match system.forced {
            Some(forced) => forced,
            None if has_provider => WeatherKind::Clear,
            None => {
                let sampled = system.sample(region, None);
                system.seasonal(sampled)
            }
        };
        system.minutes_until_change = system.rng.uniform(25.0, 70.0);
        system.thunder_cooldown = 0.0;
        system
    }

    /// The attached provider, for the driving layer.
    pub fn provider(&self) -> Option<&dyn WeatherProvider> {
        self.provider.as_deref()
    }

    pub fn provider_mut(&mut self) -> Option<&mut (dyn WeatherProvider + '_)> {
        match self.provider.as_mut() {
            Some(provider) => Some(provider.as_mut()),
            None => None,
        }
    }

    /// Clock that drives season and temperature.
    ///
    /// With live weather enabled (a provider is attached), seasons follow the
    /// real calendar so the reported season matches the live conditions;
    /// otherwise they follow the career clock, and are off entirely when no
    /// career clock was supplied.
    fn season_clock(&self) -> Option<f64> {
        if self.provider.is_some() && self.live_weather_controls_calendar {
            return Some(real_clock_game_hours(None));
        }
        self.game_hours
    }

    /// The real station temperature in Celsius while live weather is driving
    /// conditions, or None (still loading, offline, or provider has no reading).
    fn observed_temperature(&mut self) -> Option<f64> {
        if !self.live {
            return None;
        }
        let city = self.city.as_deref()?;
        let provider = self.provider.as_deref_mut()?;
        provider.get_temperature(city)
    }

    /// Outdoor temperature in Celsius. Prefers the real station observation
    /// while live weather is active, falling back to the seasonal model; None
    /// when seasons are off and no live reading is available.
    fn temperature(&mut self) -> Option<f64> {
        if let Some(observed) = self.observed_temperature() {
            return Some(observed);
        }
        let clock = self.season_clock()?;
        Some(temperature_c(&self.region, clock))
    }

    /// Reconcile a simulated condition with the season's temperature.
    fn seasonal(&mut self, kind: WeatherKind) -> WeatherKind {
        // When live weather does not control the calendar, precipitation must
        // agree with the career season even if the real station is currently
        // reporting a wintry condition. This prevents summer snow and cold-
        // season thunderstorms in the career's independent calendar.
        let temp = if self.provider.is_some() && !self.live_weather_controls_calendar {
            self.season_clock()
                .map(|clock| temperature_c(&self.region, clock))
        } else {
            self.temperature()
        };
        let Some(temp) = temp else {
            return kind;
        };
        if self.provider.is_some() && self.live_weather_controls_calendar {
            // The real sky on the real date: the season guard is for an
            // independent career calendar, and a real October snow at a
            // Denver station is snow, not rain (seasonal audit, 2026-10-01).
            return adjust_for_temperature(kind, Some(temp));
        }
        adjust_for_calendar(kind, Some(temp), self.season_clock())
    }

    /// Whether a winter warning belongs in this drive: always on the live
    /// calendar, and only in the career's winter on its own calendar, where
    /// the sky is already kept out of season snow.
    pub fn winter_fits_calendar(&self) -> bool {
        if self.provider.is_some() && self.live_weather_controls_calendar {
            return true;
        }
        self.season_clock()
            .is_none_or(|clock| season(clock) == "winter")
    }

    /// Track the city whose real weather should apply (provider mode).
    pub fn set_city(&mut self, city: &str, lat: f64, lon: f64) {
        if self.city.as_deref() != Some(city) {
            self.carried_last_known = self.carried_last_known || self.live;
            self.live = false;
            self.live_raw = None;
            self.live_city = None;
            self.live_kind = None;
            self.fallback_active = false;
        }
        self.city = Some(city.to_string());
        self.city_coords = (lat, lon);
    }

    fn sample(&mut self, region: &str, near: Option<WeatherKind>) -> WeatherKind {
        let mut weights: Vec<(WeatherKind, f64)> = region_weights(region).to_vec();
        if let Some(near) = near {
            // weather tends to evolve gradually: boost "adjacent" conditions
            let adjacency: &[WeatherKind] = match near {
                WeatherKind::Clear => &[WeatherKind::Cloudy, WeatherKind::Wind],
                WeatherKind::Cloudy => &[WeatherKind::Clear, WeatherKind::Rain, WeatherKind::Fog],
                WeatherKind::Rain => &[WeatherKind::Cloudy, WeatherKind::HeavyRain],
                WeatherKind::HeavyRain => &[WeatherKind::Rain, WeatherKind::Thunderstorm],
                WeatherKind::Thunderstorm => &[WeatherKind::HeavyRain, WeatherKind::Rain],
                WeatherKind::Snow => &[WeatherKind::Cloudy, WeatherKind::Snow],
                WeatherKind::Ice => &[WeatherKind::Rain, WeatherKind::Snow, WeatherKind::Cloudy],
                WeatherKind::Fog => &[WeatherKind::Cloudy, WeatherKind::Clear],
                WeatherKind::Wind => &[WeatherKind::Clear, WeatherKind::Cloudy],
            };
            for &kind in adjacency {
                set_weight(&mut weights, kind, |w| w.unwrap_or(0.5) * 3.0);
            }
            set_weight(&mut weights, near, |w| w.unwrap_or(1.0) * 2.0);
        }
        let kinds: Vec<WeatherKind> = weights.iter().map(|(k, _)| *k).collect();
        let values: Vec<f64> = weights.iter().map(|(_, w)| *w).collect();
        self.rng.choices(&kinds, Some(&values), None, 1)[0]
    }

    pub fn set_region(&mut self, region: &str) {
        self.region = region.to_string();
    }

    /// Advance by game minutes. Returns the new condition if it changed.
    ///
    /// Rest skips and unit tests that are themselves in game minutes pass
    /// one clock: sitting and drive time are the same interval. Driving
    /// frames use [`Self::update_paced`] so color does not ride compression.
    pub fn update(&mut self, game_minutes: f64) -> Option<WeatherKind> {
        self.update_paced(game_minutes, game_minutes)
    }

    /// Advance the career clock by `game_minutes` and the spoken-color
    /// timer by `sitting_minutes`.
    pub fn update_paced(&mut self, game_minutes: f64, sitting_minutes: f64) -> Option<WeatherKind> {
        self.thunder_cooldown = (self.thunder_cooldown - sitting_minutes).max(0.0);
        if let Some(hours) = self.game_hours.as_mut() {
            *hours += game_minutes / 60.0; // advance the career clock
        }

        if let Some(forced) = self.forced {
            // Locked condition for testing: ignore the provider and simulation.
            if self.current != forced {
                self.current = forced;
                return Some(forced);
            }
            return None;
        }

        let changed = self.poll_provider();
        if self.live {
            self.fallback_active = false;
            return changed;
        }

        if self.provider.is_some() && !self.provider_offline() {
            // Real weather is enabled and still loading: hold the current
            // condition (clear at the start of a drive) instead of running a
            // simulated warm-up. Only fall through to simulation when the
            // provider is genuinely offline.
            return None;
        }

        if self.provider.is_some() && self.session_had_live {
            // The sky has been live this session: a failing provider holds
            // the last known conditions while the retry cadence keeps trying.
            // Simulated transitions never take over from real weather.
            return None;
        }

        if self.provider.is_some() && !self.fallback_active {
            self.fallback_active = true;
            self.carried_last_known = false;
            self.minutes_until_change = self.rng.uniform(25.0, 70.0);
            let region = self.region.clone();
            let sampled = self.sample(&region, Some(self.current));
            let new = self.seasonal(sampled);
            if new != self.current {
                self.current = new;
                return Some(new);
            }
        }

        self.minutes_until_change -= sitting_minutes;
        if self.minutes_until_change > 0.0 {
            return None;
        }
        self.minutes_until_change = self.rng.uniform(25.0, 70.0);
        let region = self.region.clone();
        let sampled = self.sample(&region, Some(self.current));
        let new = self.seasonal(sampled);
        if new != self.current {
            self.current = new;
            return Some(new);
        }
        None
    }

    /// Whether the provider has no usable data and a fetch has failed.
    ///
    /// While a first fetch is still pending this is False, so the system holds
    /// steady instead of flickering through simulated weather. Providers that
    /// do not report availability (test fakes) are treated as still loading.
    fn provider_offline(&mut self) -> bool {
        let Some(city) = self.city.as_deref() else {
            return false;
        };
        let Some(provider) = self.provider.as_deref_mut() else {
            return false;
        };
        provider.unavailable(city)
    }

    /// Whether live weather is selected but no observation is ready yet.
    pub fn live_weather_loading(&mut self) -> bool {
        self.provider.is_some() && !self.live && !self.provider_offline()
    }

    /// Canonical source state for speech, menus, and event labels.
    pub fn source_status(&mut self) -> &'static str {
        if self.provider.is_none() {
            return "simulated";
        }
        if self.live {
            if let (Some(city), Some(provider)) =
                (self.city.as_deref(), self.provider.as_deref_mut())
            {
                if provider.stale(city) {
                    return "last_known";
                }
            }
            return "live";
        }
        if self.carried_last_known || self.session_had_live {
            // Live conditions have driven this sky before; whatever is wrong
            // with the network right now, the player keeps them as last-known
            // (with their honest age) rather than a simulated substitute.
            return "last_known";
        }
        if self.provider_offline() {
            return "fallback";
        }
        "loading"
    }

    /// Short, source-first wording shared by every player-facing report.
    pub fn source_label(&mut self) -> &'static str {
        match self.source_status() {
            "live" => "Live weather for your current route position",
            "loading" => "Live weather is loading for your current route position",
            "last_known" => "Last-known live weather for your current route position",
            "fallback" => "Simulated fallback weather; live weather is unavailable",
            _ => "Simulated weather",
        }
    }

    /// Whether the provider is actively fetching the tracked location.
    pub fn live_weather_refreshing(&mut self) -> bool {
        let Some(city) = self.city.as_deref() else {
            return false;
        };
        let Some(provider) = self.provider.as_deref_mut() else {
            return false;
        };
        provider.refreshing(city)
    }

    /// Whether the tracked location's most recent refresh attempt failed.
    pub fn live_weather_refresh_failed(&mut self) -> bool {
        let Some(city) = self.city.as_deref() else {
            return false;
        };
        let Some(provider) = self.provider.as_deref_mut() else {
            return false;
        };
        provider.refresh_failed(city)
    }

    /// A standalone age sentence for the current station observation.
    pub fn observation_age_text(&mut self) -> Option<String> {
        let city = self.city.as_deref()?;
        let provider = self.provider.as_deref_mut()?;
        let age_s = provider.observation_age_s(city)?;
        let minutes = ((age_s / 60.0).floor() as i64).max(0);
        if minutes < 1 {
            return Some("The observation is less than a minute old".to_string());
        }
        let unit = if minutes == 1 { "minute" } else { "minutes" };
        Some(format!("The observation is {minutes} {unit} old"))
    }

    /// Honest last-known status for announcements and source reports: an
    /// update in progress or a failed check, or nothing.
    ///
    /// No observation age. A driver has no use for how many minutes old the
    /// station reading is in a spoken line (player report, 2026-10-14); the
    /// Weather app's Observation age row keeps it.
    pub fn last_known_notice(&mut self) -> String {
        if self.live_weather_refreshing() {
            "Live weather is updating for your current location".to_string()
        } else if self.live_weather_refresh_failed() {
            "The latest live weather check failed".to_string()
        } else {
            String::new()
        }
    }

    /// Stable value for a dedicated status or tablet observation-age row.
    pub fn observation_age_value(&mut self) -> String {
        if matches!(self.source_status(), "simulated" | "fallback") {
            return "not applicable".to_string();
        }
        let Some(age) = self.observation_age_text() else {
            return "unavailable".to_string();
        };
        age.strip_prefix("The observation is ")
            .unwrap_or(&age)
            .to_string()
    }

    /// Standalone age text for a live observation, when available.
    pub fn live_observation_notice(&mut self) -> String {
        match self.observation_age_text() {
            Some(age) if !age.is_empty() => age,
            _ => "Observation age is unavailable".to_string(),
        }
    }

    pub fn report_lead(&mut self, imperial: bool) -> String {
        let status = self.source_status();
        let conditions = self.source_conditions(imperial);
        match status {
            "loading" => format!(
                "{}. Temporary neutral driving conditions in use",
                self.source_label()
            ),
            "live" => format!("Live weather: {conditions}"),
            "last_known" => {
                let notice = self.last_known_notice();
                if notice.is_empty() {
                    format!("Last-known live weather: {conditions}")
                } else {
                    format!("Last-known live weather: {conditions}. {notice}")
                }
            }
            "fallback" => {
                format!("Simulated fallback weather: {conditions}. Live weather is unavailable")
            }
            _ => format!("Simulated weather: {conditions}"),
        }
    }

    pub fn event_source_label(&mut self) -> &'static str {
        match self.source_status() {
            "live" => "Live weather",
            "loading" => "Live weather",
            "last_known" => "Last-known live weather",
            "fallback" => "Live weather is unavailable. Simulated fallback weather",
            _ => "Simulated weather",
        }
    }

    pub fn conditions_label(&mut self) -> &'static str {
        match self.source_status() {
            "live" => "Live conditions",
            "loading" => "Temporary conditions while live weather loads",
            "last_known" => "Last-known conditions",
            "fallback" => "Simulated fallback conditions",
            _ => "Simulated conditions",
        }
    }

    /// Describe conditions without presenting modeled data as observed.
    pub fn source_conditions(&mut self, imperial: bool) -> String {
        let status = self.source_status();
        if status == "loading" {
            return "neutral conditions".to_string();
        }
        if !matches!(status, "live" | "last_known") {
            return self.describe(imperial, false);
        }
        let observed_kind = if self.live {
            self.live_raw
        } else {
            self.last_observed_kind
        };
        let observed_kind = observed_kind.unwrap_or(self.current);
        let mut parts = vec![observed_kind.value().to_string()];
        let observed = if self.live {
            self.observed_temperature()
        } else {
            self.last_observed_temperature
        };
        if let Some(observed) = observed {
            if imperial {
                parts.push(format!("{} degrees", fmt_f(observed * 9.0 / 5.0 + 32.0, 0)));
            } else {
                parts.push(format!("{} degrees Celsius", fmt_f(observed, 0)));
            }
        }
        let observation = parts.join(", ");
        if observed_kind != self.current {
            return format!(
                "observation {observation}; treated as {} for driving",
                self.current.value()
            );
        }
        observation
    }

    pub fn has_simulated_forecast(&mut self) -> bool {
        matches!(self.source_status(), "simulated" | "fallback")
    }

    /// Apply real-world conditions when a provider is attached.
    ///
    /// Returns the new condition if real data changed it; otherwise None.
    /// While real data is available the simulated transitions are paused.
    fn poll_provider(&mut self) -> Option<WeatherKind> {
        let city = self.city.clone()?;
        let (lat, lon) = self.city_coords;
        let kind = {
            let provider = self.provider.as_deref_mut()?;
            provider.request(&city, lat, lon);
            provider.get(&city)
        };
        let Some(kind) = kind else {
            self.carried_last_known = self.carried_last_known || self.live;
            self.live = false;
            self.live_raw = None;
            self.live_city = None;
            self.live_kind = None;
            return None;
        };
        self.live = true;
        self.session_had_live = true;
        self.carried_last_known = false;
        let same_observation = Some(kind) == self.live_raw && self.city == self.live_city;
        if !same_observation {
            self.last_observed_kind = Some(kind);
            self.last_observed_temperature = self.observed_temperature();
        }
        // Reconcile the raw observation to the career season once, when the
        // observation (or the city it is for) changes -- not every tick. The
        // season temperature swings across freezing on a diurnal cycle, so
        // re-reconciling each tick would flip live precipitation between rain
        // and freezing rain on its own, which live weather must never do.
        if same_observation {
            if let Some(live_kind) = self.live_kind {
                if self.current != live_kind {
                    self.current = live_kind;
                    return Some(live_kind);
                }
            }
            return None;
        }
        self.live_raw = Some(kind);
        self.live_city = self.city.clone();
        let guarded = self.seasonal(kind);
        self.live_kind = Some(guarded);
        if guarded != self.current {
            self.current = guarded;
            return Some(guarded);
        }
        None
    }

    /// Occasional thunder strikes during a thunderstorm.
    pub fn should_thunder(&mut self) -> bool {
        if self.current != WeatherKind::Thunderstorm || self.thunder_cooldown > 0.0 {
            return false;
        }
        if self.rng.random() < 0.4 {
            self.thunder_cooldown = self.rng.uniform(2.0, 6.0);
            return true;
        }
        false
    }

    pub fn effects(&self) -> WeatherEffects {
        effects(self.current)
    }

    /// Modeled outdoor temperature in Celsius, or None when seasons are off.
    pub fn temperature_c(&mut self) -> Option<f64> {
        self.temperature()
    }

    /// Current season (real calendar with live weather, else career clock).
    pub fn season(&self) -> Option<&'static str> {
        self.season_clock().map(season)
    }

    /// Calendar date (real with live weather, else the career clock), e.g.
    /// 'March 21'; None when no clock is available.
    pub fn date_text(&self) -> Option<String> {
        self.season_clock().map(date_text)
    }

    /// Probable conditions ahead (informational, not binding).
    ///
    /// Draws from a copy of the generator state, so the weather timeline the
    /// player is actually driving through never moves.
    pub fn forecast(&mut self, segments: usize) -> Vec<WeatherKind> {
        let mut rng = PyRandom::from_state(&self.rng.getstate());
        let mut out = Vec::with_capacity(segments);
        let mut cur = self.current;
        for _ in 0..segments {
            let mut weights: Vec<(WeatherKind, f64)> = region_weights(&self.region).to_vec();
            set_weight(&mut weights, cur, |w| w.unwrap_or(1.0) * 2.5);
            let kinds: Vec<WeatherKind> = weights.iter().map(|(k, _)| *k).collect();
            let values: Vec<f64> = weights.iter().map(|(_, w)| *w).collect();
            let drawn = rng.choices(&kinds, Some(&values), None, 1)[0];
            cur = self.seasonal(drawn);
            out.push(cur);
        }
        out
    }

    /// `describe(imperial, observed_temperature_only=...)`.
    pub fn describe(&mut self, imperial: bool, observed_temperature_only: bool) -> String {
        let eff = self.effects();
        let mut parts = vec![self.current.value().to_string()];
        let temp_c = if observed_temperature_only {
            self.observed_temperature()
        } else {
            self.temperature()
        };
        if let Some(temp_c) = temp_c {
            if imperial {
                parts.push(format!("{} degrees", fmt_f(temp_c * 9.0 / 5.0 + 32.0, 0)));
            } else {
                parts.push(format!("{} degrees Celsius", fmt_f(temp_c, 0)));
            }
        }
        if eff.visibility_mi < 2.0 {
            let visibility = if imperial {
                format!("{} miles", fmt_g(eff.visibility_mi))
            } else {
                format!("{} kilometers", fmt_g(eff.visibility_mi * 1.609344))
            };
            parts.push(format!("visibility {visibility}"));
        }
        if self.current == WeatherKind::Ice {
            parts.push("ice on the road".to_string());
        } else if eff.grip < 0.7 {
            parts.push("slick roads".to_string());
        }
        if eff.wind > 0.6 {
            parts.push("strong crosswinds".to_string());
        }
        parts.join(", ")
    }
}

/// Shared by the weather and season test modules.
#[cfg(test)]
pub(crate) mod test_support {
    use super::{WeatherProvider, WeatherSystem};
    use std::sync::{RwLock, RwLockReadGuard, RwLockWriteGuard};

    /// `WeatherSystem::new` reads `FREIGHT_FATE_FORCE_WEATHER`, which is
    /// process-global while cargo runs tests on parallel threads: the one
    /// test that sets it takes the write side, every construction the read
    /// side, so a forced condition never leaks into a neighbour.
    pub static ENV_LOCK: RwLock<()> = RwLock::new(());

    pub fn env_read() -> RwLockReadGuard<'static, ()> {
        ENV_LOCK.read().unwrap_or_else(|e| e.into_inner())
    }

    pub fn env_write() -> RwLockWriteGuard<'static, ()> {
        ENV_LOCK.write().unwrap_or_else(|e| e.into_inner())
    }

    /// `WeatherSystem::new` under the env read lock.
    pub fn new_system(
        region: &str,
        seed: Option<i64>,
        provider: Option<Box<dyn WeatherProvider>>,
        game_hours: Option<f64>,
        live_weather_controls_calendar: bool,
    ) -> WeatherSystem {
        let _guard = env_read();
        WeatherSystem::new(
            region,
            seed,
            provider,
            game_hours,
            live_weather_controls_calendar,
        )
    }
}
