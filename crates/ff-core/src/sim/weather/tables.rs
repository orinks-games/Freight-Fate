//! The weather conditions, their physics modifiers and the per-region
//! likelihood tables: the data half of `freight_fate/sim/weather.py`, kept
//! apart from the evolving system so neither file outgrows a screen.

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum WeatherKind {
    Clear,
    Cloudy,
    Rain,
    HeavyRain,
    Thunderstorm,
    Snow,
    Ice,
    Fog,
    Wind,
}

impl WeatherKind {
    /// Every condition, in the Python enum's declaration order
    /// (`for kind in WeatherKind`).
    pub const ALL: [WeatherKind; 9] = [
        WeatherKind::Clear,
        WeatherKind::Cloudy,
        WeatherKind::Rain,
        WeatherKind::HeavyRain,
        WeatherKind::Thunderstorm,
        WeatherKind::Snow,
        WeatherKind::Ice,
        WeatherKind::Fog,
        WeatherKind::Wind,
    ];

    /// The spoken condition (`WeatherKind.X.value`).
    pub fn value(self) -> &'static str {
        match self {
            WeatherKind::Clear => "clear",
            WeatherKind::Cloudy => "cloudy",
            WeatherKind::Rain => "rain",
            WeatherKind::HeavyRain => "heavy rain",
            WeatherKind::Thunderstorm => "thunderstorm",
            WeatherKind::Snow => "snow",
            WeatherKind::Ice => "freezing rain",
            WeatherKind::Fog => "fog",
            WeatherKind::Wind => "high winds",
        }
    }

    /// The Python member name (`WeatherKind.X.name`), e.g. `"HEAVY_RAIN"`.
    pub fn name(self) -> &'static str {
        match self {
            WeatherKind::Clear => "CLEAR",
            WeatherKind::Cloudy => "CLOUDY",
            WeatherKind::Rain => "RAIN",
            WeatherKind::HeavyRain => "HEAVY_RAIN",
            WeatherKind::Thunderstorm => "THUNDERSTORM",
            WeatherKind::Snow => "SNOW",
            WeatherKind::Ice => "ICE",
            WeatherKind::Fog => "FOG",
            WeatherKind::Wind => "WIND",
        }
    }

    /// `WeatherKind(value)`: the condition for a spoken value, if any.
    pub fn from_value(value: &str) -> Option<WeatherKind> {
        WeatherKind::ALL
            .into_iter()
            .find(|kind| kind.value() == value)
    }
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct WeatherEffects {
    /// Traction multiplier.
    pub grip: f64,
    /// Aerodynamic drag multiplier (headwinds).
    pub drag_mult: f64,
    pub visibility_mi: f64,
    /// Ambience loop key, e.g. "weather/rain_light".
    pub sound: Option<&'static str>,
    /// 0..1 wind loop intensity.
    pub wind: f64,
    pub safe_speed_mph: f64,
    /// Standing water depth; drives hydroplane onset.
    pub water_mm: f64,
    /// What the tires touch: dry, wet, snow, or ice.
    pub surface: &'static str,
}

impl WeatherEffects {
    /// Weather that takes grip or sight: the one test the cruise cap, the
    /// hazard brake and the co-driver's target speed all read, so they never
    /// disagree about whether to slow for it.
    pub fn is_adverse(&self) -> bool {
        self.grip < 1.0 || self.visibility_mi < 8.0
    }

    // One row of the Python dataclass table, positional like the original.
    #[allow(clippy::too_many_arguments)]
    const fn new(
        grip: f64,
        drag_mult: f64,
        visibility_mi: f64,
        sound: Option<&'static str>,
        wind: f64,
        safe_speed_mph: f64,
        water_mm: f64,
        surface: &'static str,
    ) -> Self {
        WeatherEffects {
            grip,
            drag_mult,
            visibility_mi,
            sound,
            wind,
            safe_speed_mph,
            water_mm,
            surface,
        }
    }
}

/// Physics modifiers per condition, in `WeatherKind::ALL` order.
///
/// Freezing rain never rolls in the random weather draw: it forms when rain
/// falls into the narrow band just below freezing (see season.rs) or when the
/// live NWS feed reports it. Its grip is glare-ice territory -- a third of
/// snow -- which is what makes it the one condition worth parking for.
pub const EFFECTS: [(WeatherKind, WeatherEffects); 9] = [
    (
        WeatherKind::Clear,
        WeatherEffects::new(1.00, 1.00, 10.0, None, 0.0, 70.0, 0.0, "dry"),
    ),
    (
        WeatherKind::Cloudy,
        WeatherEffects::new(1.00, 1.00, 8.0, None, 0.1, 70.0, 0.0, "dry"),
    ),
    (
        WeatherKind::Rain,
        WeatherEffects::new(
            0.80,
            1.05,
            4.0,
            Some("weather/rain_light"),
            0.2,
            55.0,
            1.5,
            "wet",
        ),
    ),
    (
        WeatherKind::HeavyRain,
        WeatherEffects::new(
            0.62,
            1.12,
            1.5,
            Some("weather/rain_heavy"),
            0.4,
            45.0,
            3.0,
            "wet",
        ),
    ),
    (
        WeatherKind::Thunderstorm,
        WeatherEffects::new(
            0.58,
            1.18,
            1.0,
            Some("weather/rain_heavy"),
            0.6,
            40.0,
            4.0,
            "wet",
        ),
    ),
    (
        WeatherKind::Snow,
        WeatherEffects::new(
            0.45,
            1.08,
            2.0,
            Some("weather/snow_wind"),
            0.5,
            35.0,
            0.0,
            "snow",
        ),
    ),
    (
        WeatherKind::Ice,
        WeatherEffects::new(
            0.15,
            1.02,
            3.0,
            Some("weather/rain_light"),
            0.2,
            20.0,
            0.0,
            "ice",
        ),
    ),
    // Fog has no sound of its own: no bed plays, and the road tells the
    // story through the visibility calls and the wet tires (owner ask,
    // 2026-09-13; the fog-horn loop it used to carry is still in the pack).
    (
        WeatherKind::Fog,
        WeatherEffects::new(0.92, 1.00, 0.3, None, 0.1, 40.0, 0.0, "wet"),
    ),
    (
        WeatherKind::Wind,
        WeatherEffects::new(0.90, 1.25, 7.0, None, 0.9, 55.0, 0.0, "dry"),
    ),
];

/// `EFFECTS[kind]`.
pub fn effects(kind: WeatherKind) -> WeatherEffects {
    EFFECTS
        .iter()
        .find(|(k, _)| *k == kind)
        .map(|(_, eff)| *eff)
        .expect("every WeatherKind has an EFFECTS row")
}

/// One region's likelihood table: `(condition, weight)` in the declaration
/// order of the Python dict, which is the order `random.choices` saw them.
pub type RegionWeights = [(WeatherKind, f64); 8];

const HEARTLAND_WEIGHTS: RegionWeights = [
    (WeatherKind::Clear, 4.0),
    (WeatherKind::Cloudy, 2.5),
    (WeatherKind::Rain, 2.0),
    (WeatherKind::HeavyRain, 1.0),
    (WeatherKind::Thunderstorm, 2.0),
    (WeatherKind::Snow, 1.0),
    (WeatherKind::Fog, 1.0),
    (WeatherKind::Wind, 2.0),
];

/// Per-region likelihood weights for each condition.
pub const REGION_WEIGHTS: [(&str, RegionWeights); 16] = [
    (
        "northeast",
        [
            (WeatherKind::Clear, 4.0),
            (WeatherKind::Cloudy, 3.0),
            (WeatherKind::Rain, 2.0),
            (WeatherKind::HeavyRain, 1.0),
            (WeatherKind::Thunderstorm, 0.5),
            (WeatherKind::Snow, 1.5),
            (WeatherKind::Fog, 1.0),
            (WeatherKind::Wind, 0.5),
        ],
    ),
    (
        "appalachia",
        [
            (WeatherKind::Clear, 3.5),
            (WeatherKind::Cloudy, 3.0),
            (WeatherKind::Rain, 2.0),
            (WeatherKind::HeavyRain, 1.0),
            (WeatherKind::Thunderstorm, 1.0),
            (WeatherKind::Snow, 1.5),
            (WeatherKind::Fog, 2.5),
            (WeatherKind::Wind, 1.0),
        ],
    ),
    (
        "great_lakes",
        [
            (WeatherKind::Clear, 3.5),
            (WeatherKind::Cloudy, 3.5),
            (WeatherKind::Rain, 2.0),
            (WeatherKind::HeavyRain, 1.0),
            (WeatherKind::Thunderstorm, 1.5),
            (WeatherKind::Snow, 2.5),
            (WeatherKind::Fog, 1.0),
            (WeatherKind::Wind, 1.5),
        ],
    ),
    (
        // Coldest tier: long snowy winters, lake-effect and northwoods snow.
        "upper_midwest",
        [
            (WeatherKind::Clear, 3.0),
            (WeatherKind::Cloudy, 3.5),
            (WeatherKind::Rain, 1.5),
            (WeatherKind::HeavyRain, 0.5),
            (WeatherKind::Thunderstorm, 1.5),
            (WeatherKind::Snow, 3.5),
            (WeatherKind::Fog, 1.0),
            (WeatherKind::Wind, 2.0),
        ],
    ),
    (
        // Continental interior: warm-season thunderstorms, less snow than the
        // lakeshore (no lake-effect), river-valley fog.
        "corn_belt",
        [
            (WeatherKind::Clear, 3.5),
            (WeatherKind::Cloudy, 3.0),
            (WeatherKind::Rain, 2.0),
            (WeatherKind::HeavyRain, 1.0),
            (WeatherKind::Thunderstorm, 2.0),
            (WeatherKind::Snow, 1.5),
            (WeatherKind::Fog, 1.5),
            (WeatherKind::Wind, 1.5),
        ],
    ),
    ("heartland", HEARTLAND_WEIGHTS),
    (
        "southern_plains",
        [
            (WeatherKind::Clear, 5.0),
            (WeatherKind::Cloudy, 2.0),
            (WeatherKind::Rain, 1.5),
            (WeatherKind::HeavyRain, 1.0),
            (WeatherKind::Thunderstorm, 2.5),
            (WeatherKind::Snow, 0.3),
            (WeatherKind::Fog, 0.5),
            (WeatherKind::Wind, 3.0),
        ],
    ),
    (
        "mid_south",
        [
            (WeatherKind::Clear, 4.0),
            (WeatherKind::Cloudy, 2.5),
            (WeatherKind::Rain, 2.0),
            (WeatherKind::HeavyRain, 1.5),
            (WeatherKind::Thunderstorm, 2.0),
            (WeatherKind::Snow, 0.4),
            (WeatherKind::Fog, 1.5),
            (WeatherKind::Wind, 0.7),
        ],
    ),
    (
        "atlantic_southeast",
        [
            (WeatherKind::Clear, 4.5),
            (WeatherKind::Cloudy, 2.5),
            (WeatherKind::Rain, 2.0),
            (WeatherKind::HeavyRain, 1.5),
            (WeatherKind::Thunderstorm, 2.5),
            (WeatherKind::Snow, 0.2),
            (WeatherKind::Fog, 1.0),
            (WeatherKind::Wind, 0.6),
        ],
    ),
    (
        "gulf_coast",
        [
            (WeatherKind::Clear, 4.0),
            (WeatherKind::Cloudy, 2.5),
            (WeatherKind::Rain, 2.0),
            (WeatherKind::HeavyRain, 2.0),
            (WeatherKind::Thunderstorm, 3.0),
            (WeatherKind::Snow, 0.05),
            (WeatherKind::Fog, 1.5),
            (WeatherKind::Wind, 0.8),
        ],
    ),
    (
        "florida",
        [
            (WeatherKind::Clear, 4.5),
            (WeatherKind::Cloudy, 2.5),
            (WeatherKind::Rain, 2.0),
            (WeatherKind::HeavyRain, 2.0),
            (WeatherKind::Thunderstorm, 3.5),
            (WeatherKind::Snow, 0.0),
            (WeatherKind::Fog, 0.8),
            (WeatherKind::Wind, 0.8),
        ],
    ),
    (
        "rockies",
        [
            (WeatherKind::Clear, 4.0),
            (WeatherKind::Cloudy, 2.5),
            (WeatherKind::Rain, 1.0),
            (WeatherKind::HeavyRain, 0.5),
            (WeatherKind::Thunderstorm, 1.0),
            (WeatherKind::Snow, 3.0),
            (WeatherKind::Fog, 1.0),
            (WeatherKind::Wind, 2.0),
        ],
    ),
    (
        "great_basin",
        [
            (WeatherKind::Clear, 5.5),
            (WeatherKind::Cloudy, 1.5),
            (WeatherKind::Rain, 0.7),
            (WeatherKind::HeavyRain, 0.3),
            (WeatherKind::Thunderstorm, 0.8),
            (WeatherKind::Snow, 1.5),
            (WeatherKind::Fog, 0.5),
            (WeatherKind::Wind, 2.5),
        ],
    ),
    (
        "desert_southwest",
        [
            (WeatherKind::Clear, 7.0),
            (WeatherKind::Cloudy, 1.5),
            (WeatherKind::Rain, 0.5),
            (WeatherKind::HeavyRain, 0.4),
            (WeatherKind::Thunderstorm, 1.0),
            (WeatherKind::Snow, 0.15),
            (WeatherKind::Fog, 0.2),
            (WeatherKind::Wind, 2.0),
        ],
    ),
    (
        "california",
        [
            (WeatherKind::Clear, 5.0),
            (WeatherKind::Cloudy, 3.0),
            (WeatherKind::Rain, 1.3),
            (WeatherKind::HeavyRain, 0.5),
            (WeatherKind::Thunderstorm, 0.3),
            (WeatherKind::Snow, 0.1),
            (WeatherKind::Fog, 2.5),
            (WeatherKind::Wind, 1.0),
        ],
    ),
    (
        "pacific_northwest",
        [
            (WeatherKind::Clear, 2.5),
            (WeatherKind::Cloudy, 4.0),
            (WeatherKind::Rain, 3.5),
            (WeatherKind::HeavyRain, 1.5),
            (WeatherKind::Thunderstorm, 0.5),
            (WeatherKind::Snow, 1.0),
            (WeatherKind::Fog, 2.0),
            (WeatherKind::Wind, 1.0),
        ],
    ),
];

/// `REGION_WEIGHTS["heartland"]`.
pub const DEFAULT_WEIGHTS: RegionWeights = HEARTLAND_WEIGHTS;

/// `REGION_WEIGHTS.get(region, DEFAULT_WEIGHTS)`, in declaration order.
pub fn region_weights(region: &str) -> &'static [(WeatherKind, f64)] {
    REGION_WEIGHTS
        .iter()
        .find(|(name, _)| *name == region)
        .map(|(_, weights)| &weights[..])
        .unwrap_or(&DEFAULT_WEIGHTS[..])
}
