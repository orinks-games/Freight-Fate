//! The grounded road-hazard catalog (the hazard half of
//! `freight_fate/sim/trip_models.py`): each hazard tagged with the
//! conditions under which it is plausible, and the eligibility filter that
//! draws only what the region, weather, terrain and hour allow.

use crate::sim::hos::time_of_day;
use crate::sim::weather::WeatherKind;

const CROSSWIND_REGIONS: &[&str] = &[
    "southern_plains",
    "heartland",
    "great_basin",
    "desert_southwest",
    "rockies",
];
const WET: &[WeatherKind] = &[
    WeatherKind::Rain,
    WeatherKind::HeavyRain,
    WeatherKind::Thunderstorm,
];
const HEAVY_WET: &[WeatherKind] = &[WeatherKind::HeavyRain, WeatherKind::Thunderstorm];
const WILDLIFE_TIMES: [&str; 3] = ["dawn", "dusk", "night"];

/// Where a dodge can go from the truck's own lane, as the hazard call names
/// it (owner, 2026-09-01, on I-90 with adaptive cruise on: "Change lanes or
/// brake!" left a blind driver guessing whether a lane change was even
/// possible, and the game already knew).
///
/// Read by `Trip::open_side_at` against the same rules a lane change is
/// refused by -- the lane exists, is not coned off, and no vehicle holds it
/// over the window the tap change takes -- so the call never names a lane the
/// dodge would refuse. A hazard with no open side is not dodgeable at all:
/// same "Brake!" family, same window, as a one-lane road.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenSide {
    Left,
    Right,
    Either,
    Neither,
}

impl OpenSide {
    pub fn from_sides(left: bool, right: bool) -> OpenSide {
        match (left, right) {
            (true, true) => OpenSide::Either,
            (true, false) => OpenSide::Left,
            (false, true) => OpenSide::Right,
            (false, false) => OpenSide::Neither,
        }
    }

    /// Whether there is anywhere at all to send the dodge.
    pub fn is_open(self) -> bool {
        self != OpenSide::Neither
    }

    /// The lane a pass takes, as a step from the truck's lane: `1` is one
    /// lane left (lane 0 is the right lane), `-1` one lane right. Left
    /// wherever left is open, the side traffic passes on; right only where
    /// it is the one lane open.
    pub fn pass_step(self) -> Option<i64> {
        match self {
            OpenSide::Left | OpenSide::Either => Some(1),
            OpenSide::Right => Some(-1),
            OpenSide::Neither => None,
        }
    }

    /// The lane answer, in the L key's own words ("left lane open", see
    /// docs/ontology.md), so the call and the readout never disagree.
    pub fn spoken(self) -> &'static str {
        match self {
            OpenSide::Left => "Left lane open.",
            OpenSide::Right => "Right lane open.",
            OpenSide::Either => "Either lane open.",
            OpenSide::Neither => "No lane open.",
        }
    }

    /// [`Self::spoken`] for a truck in `lane` of `count`, naming the open
    /// neighbour the way the L key and a lane change do: on a road three
    /// wide, the lane left of the right lane is the middle lane, and "Left
    /// lane open." then a move into "the middle lane" read as two different
    /// lanes (agent drive to Uvalde, 2026-10-06).
    pub fn spoken_from(self, lane: i64, count: i64) -> String {
        let neighbour = match self {
            OpenSide::Left => lane + 1,
            OpenSide::Right => lane - 1,
            OpenSide::Either | OpenSide::Neither => return self.spoken().to_string(),
        };
        let mut name = crate::sim::lane::lane_label(neighbour, count).to_string();
        if let Some(first) = name.get_mut(0..1) {
            first.make_ascii_uppercase();
        }
        format!("{name} lane open.")
    }
}

/// One grounded road hazard and the conditions under which it can occur.
///
/// `None` on `regions`/`weather`/`terrain` means "no restriction on that
/// axis". `animal` hazards are biased to dawn, dusk, and night. `name` is the
/// short noun phrase a resolution line names this hazard by ("the deer").
///
/// `in_lane` marks a hazard that sits in YOUR lane and no other: an object
/// on the pavement, a car stopped on the shoulder, a coned-off lane. It is a
/// property of the THING, and it is deliberately not the same question as
/// whether the hazard can be dodged -- that also needs a lane to dodge into,
/// which is a property of the ROAD and the traffic on it
/// (`Trip::open_side_at`). Fog,
/// ice, a crosswind and a deer that may bolt either way are not in-lane: they
/// span the road, so no lane change answers them however many lanes there
/// are. The two are combined once, at the emitter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HazardDef {
    pub text: &'static str,
    pub weight: f64,
    pub regions: Option<&'static [&'static str]>,
    pub weather: Option<&'static [WeatherKind]>,
    pub terrain: Option<&'static [&'static str]>,
    pub animal: bool,
    pub in_lane: bool,
    /// For a thing in the lane that a truck passes slowly rather than stops
    /// for -- a mattress or a tarp it straddles, standing water it eases
    /// through, a car on the shoulder it gives room to: the speed braking
    /// alone has to reach when there is no lane to take. None is the near
    /// stop a hard object asks for (owner, 2026-10-19: too much braking).
    /// Assumed, not measured; the move-over figure follows the state laws'
    /// "slow to 20 under", taken from a 60 mph road.
    pub brake_to_mph: Option<f64>,
    pub name: &'static str,
}

/// An animal is braked for firmly in the lane, never swerved for: the
/// standard truck-driver training for a deer (owner, 2026-10-19). Braking a
/// loaded truck to a stop from highway speed does not save the animal and
/// invites the car behind into the trailer, so the brake aims at a speed
/// that buys the animal time to get across. Assumed, not measured.
pub const ANIMAL_BRAKE_TO_MPH: f64 = 35.0;

const fn hz(text: &'static str, weight: f64, name: &'static str) -> HazardDef {
    HazardDef {
        text,
        weight,
        regions: None,
        weather: None,
        terrain: None,
        animal: false,
        in_lane: false,
        brake_to_mph: None,
        name,
    }
}

const DEER_REGIONS: &[&str] = &[
    "northeast",
    "appalachia",
    "great_lakes",
    "upper_midwest",
    "corn_belt",
    "heartland",
    "mid_south",
    "atlantic_southeast",
    "southern_plains",
    "gulf_coast",
    "florida",
    "california",
];

pub static HAZARDS: &[HazardDef] = &[
    // Nationwide staples. Named debris, not "debris": a driver clearing a lane
    // blind needs to know WHAT is in it (Brandon, 2026-08-20). The split's
    // weights sum to the 1.2 the one generic entry carried.
    HazardDef {
        in_lane: true,
        ..hz(
            "a ladder fallen from a truck in the lane",
            0.25,
            "the ladder",
        )
    },
    HazardDef {
        in_lane: true,
        ..hz("loose lumber dropped across the lane", 0.25, "the lumber")
    },
    HazardDef {
        brake_to_mph: Some(35.0),
        in_lane: true,
        ..hz("a mattress lying in the lane", 0.2, "the mattress")
    },
    HazardDef {
        in_lane: true,
        ..hz("spilled cargo boxes across the lane", 0.2, "the boxes")
    },
    HazardDef {
        brake_to_mph: Some(35.0),
        in_lane: true,
        ..hz("a shredded truck tarp in the lane", 0.15, "the tarp")
    },
    HazardDef {
        in_lane: true,
        ..hz("debris on the road", 0.15, "the debris")
    },
    HazardDef {
        in_lane: true,
        ..hz("retread debris from a blown tire", 1.0, "the tire debris")
    },
    // The move-over law in action: shift a lane away from the shoulder.
    HazardDef {
        brake_to_mph: Some(40.0),
        in_lane: true,
        ..hz(
            "a vehicle stopped on the shoulder",
            1.0,
            "the stopped vehicle",
        )
    },
    // "A slow vehicle ahead" is deliberately NOT here (owner call,
    // 2026-08-20): a slow vehicle in the flow is the traffic bubble's job.
    HazardDef {
        in_lane: true,
        ..hz("a sudden lane closure ahead", 0.8, "the lane closure")
    },
    hz(
        "stopped traffic around a fender bender",
        0.9,
        "the stopped traffic",
    ),
    // Wildlife: dawn/dusk/night, regional species.
    HazardDef {
        animal: true,
        brake_to_mph: Some(ANIMAL_BRAKE_TO_MPH),
        regions: Some(DEER_REGIONS),
        ..hz("a deer crossing the road", 1.3, "the deer")
    },
    HazardDef {
        animal: true,
        brake_to_mph: Some(ANIMAL_BRAKE_TO_MPH),
        regions: Some(&["rockies", "great_basin", "pacific_northwest"]),
        ..hz("an elk crossing the road", 1.1, "the elk")
    },
    // Named animals for the generic slot, weights summing to the 0.7 the one
    // generic entry carried.
    HazardDef {
        animal: true,
        brake_to_mph: Some(ANIMAL_BRAKE_TO_MPH),
        ..hz("a dog loose on the road", 0.2, "the dog")
    },
    HazardDef {
        animal: true,
        brake_to_mph: Some(ANIMAL_BRAKE_TO_MPH),
        ..hz("a coyote crossing the road", 0.15, "the coyote")
    },
    HazardDef {
        animal: true,
        brake_to_mph: Some(ANIMAL_BRAKE_TO_MPH),
        ..hz("loose livestock on the road", 0.15, "the livestock")
    },
    HazardDef {
        animal: true,
        brake_to_mph: Some(ANIMAL_BRAKE_TO_MPH),
        ..hz("a raccoon in the lane", 0.1, "the raccoon")
    },
    HazardDef {
        animal: true,
        brake_to_mph: Some(ANIMAL_BRAKE_TO_MPH),
        ..hz("an animal on the road", 0.1, "the animal")
    }, // honest fallback
    // Wet weather only.
    HazardDef {
        brake_to_mph: Some(35.0),
        weather: Some(WET),
        in_lane: true,
        ..hz(
            "standing water flooding the lane",
            1.1,
            "the standing water",
        )
    },
    HazardDef {
        weather: Some(HEAVY_WET),
        ..hz(
            "the trailer hydroplaning on standing water",
            1.0,
            "the hydroplaning",
        )
    },
    HazardDef {
        weather: Some(&[WeatherKind::Thunderstorm]),
        regions: Some(&[
            "southern_plains",
            "heartland",
            "corn_belt",
            "mid_south",
            "rockies",
            "great_lakes",
        ]),
        ..hz("hail hammering the windshield", 0.7, "the hail")
    },
    // Snow and ice only.
    HazardDef {
        weather: Some(&[WeatherKind::Snow]),
        ..hz("a snow squall whiting out the lane", 1.0, "the snow squall")
    },
    HazardDef {
        weather: Some(&[WeatherKind::Snow, WeatherKind::Ice]),
        ..hz("ice on the bridge deck", 1.0, "the ice")
    },
    HazardDef {
        weather: Some(&[WeatherKind::Snow, WeatherKind::Ice]),
        terrain: Some(&["mountain", "hills"]),
        ..hz("black ice on the shaded grade", 1.1, "the black ice")
    },
    // Freezing rain only.
    HazardDef {
        weather: Some(&[WeatherKind::Ice]),
        ..hz("glaze ice sheeting the whole lane", 1.3, "the glaze ice")
    },
    HazardDef {
        weather: Some(&[WeatherKind::Ice]),
        in_lane: true,
        ..hz("a car spun out on the glaze ahead", 1.1, "the spun-out car")
    },
    // Dense fog only.
    HazardDef {
        weather: Some(&[WeatherKind::Fog]),
        ..hz("brake lights looming in dense fog", 1.2, "the brake lights")
    },
    // High wind: crosswind shove and blowing debris in open country.
    HazardDef {
        weather: Some(&[WeatherKind::Wind]),
        regions: Some(CROSSWIND_REGIONS),
        ..hz(
            "a crosswind gust shoving the trailer",
            1.2,
            "the crosswind gust",
        )
    },
    HazardDef {
        weather: Some(&[WeatherKind::Wind]),
        regions: Some(&["desert_southwest", "southern_plains", "great_basin"]),
        ..hz("a dust storm dropping visibility", 0.9, "the dust storm")
    },
    HazardDef {
        brake_to_mph: Some(45.0),
        weather: Some(&[WeatherKind::Wind]),
        regions: Some(&["desert_southwest", "great_basin", "southern_plains"]),
        in_lane: true,
        ..hz("tumbleweeds piling in your lane", 0.5, "the tumbleweeds")
    },
    // Mountain terrain only.
    HazardDef {
        terrain: Some(&["mountain"]),
        regions: Some(&[
            "rockies",
            "appalachia",
            "great_basin",
            "pacific_northwest",
            "california",
        ]),
        in_lane: true,
        ..hz("rockfall debris on the road", 1.0, "the rockfall")
    },
    // Given room, not braked hard for on the grade it is running away on.
    HazardDef {
        terrain: Some(&["mountain"]),
        brake_to_mph: Some(40.0),
        ..hz(
            "a runaway truck on the grade ahead",
            0.8,
            "the runaway truck",
        )
    },
];

/// Whether this hazard sits in your lane alone -- the half of "can I go
/// around it" that belongs to the hazard rather than to the road.
pub fn hazard_is_in_lane(text: &str) -> bool {
    HAZARDS.iter().any(|h| h.in_lane && h.text == text)
}

/// What braking alone has to reach for this hazard, when it is less than a
/// near stop (see `HazardDef::brake_to_mph`).
pub fn hazard_brake_to_mph(text: &str) -> Option<f64> {
    HAZARDS
        .iter()
        .find(|h| h.text == text)
        .and_then(|h| h.brake_to_mph)
}

/// Whether this hazard is an animal on the road.
pub fn hazard_is_animal(text: &str) -> bool {
    HAZARDS.iter().any(|h| h.animal && h.text == text)
}

/// The short noun phrase a resolution line names this hazard by; "it" for a
/// text that somehow is not in the table rather than ever failing mid-drive.
pub fn hazard_name(text: &str) -> &'static str {
    HAZARDS
        .iter()
        .find(|h| h.text == text)
        .map(|h| h.name)
        .unwrap_or("it")
}

/// Hazards plausible for the current context, as `(text, weight)` pairs.
/// The nationwide staples have no restrictions, so the list is never empty.
pub fn eligible_hazards(
    region: &str,
    weather: WeatherKind,
    terrain: &str,
    game_hours: f64,
) -> Vec<(&'static str, f64)> {
    let nocturnal = WILDLIFE_TIMES.contains(&time_of_day(game_hours));
    let mut out = Vec::new();
    for hazard in HAZARDS {
        if hazard.regions.is_some_and(|r| !r.contains(&region)) {
            continue;
        }
        if hazard.weather.is_some_and(|w| !w.contains(&weather)) {
            continue;
        }
        if hazard.terrain.is_some_and(|t| !t.contains(&terrain)) {
            continue;
        }
        let mut weight = hazard.weight;
        if hazard.animal {
            weight *= if nocturnal { 2.2 } else { 0.25 };
        }
        out.push((hazard.text, weight));
    }
    out
}
