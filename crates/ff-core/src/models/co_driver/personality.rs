//! Co-driver personality (owner request, 2026-10-20): small habits a
//! co-driver shows at the wheel while the player rides along.
//!
//! Each co-driver has their own switch, off until the player turns it on
//! from the Co-drivers list or the pause menu, so a career finds each
//! personality one driver at a time. The switches live in the profile's
//! `achievement_stats` counters, like the rest of the team history, so a
//! save gains no new field.
//!
//! Every moment is flavour. None changes the drive, the radio the player
//! chose, the pay or the hours. They are rare on purpose: a gap of 40 to 80
//! game minutes between any two, at most four a shift, and each one once a
//! load. Asleep, there are none.
//!
//! The lines are narration with the name first ("June hums along."), the
//! way every other co-driver line reads. The game has one voice, and a
//! quote in it sounds like the game talking.
//!
//! Designed by a brainstorm across four lenses (real team driving, the
//! cast's own stories, what the drive can detect, and the sound pack), then
//! cut by an accessibility review and a realism review.

use serde_json::{Map, Value};

use crate::sim::weather::WeatherKind;

const STAT_PERSONALITY: &str = "team:personality:";

/// Fewest game minutes between two moments.
pub const MOMENT_GAP_MIN: f64 = 40.0;
/// The random part of the gap, added to [`MOMENT_GAP_MIN`].
pub const MOMENT_GAP_SPREAD_MIN: f64 = 40.0;
/// The first moment of a shift comes no sooner than this after the
/// handover, so the handover's own lines have the road to themselves.
pub const FIRST_MOMENT_MIN: f64 = 20.0;
/// Most spoken moments in one shift.
pub const MOMENTS_PER_SHIFT: u32 = 4;
/// No moment this soon after one of the co-driver's own notes (a stop
/// planned, a hazard cleared), so news is never crowded by flavour.
pub const QUIET_AFTER_NOTE_MIN: f64 = 5.0;

/// Whether this co-driver's personality is on for the career.
pub fn personality_on(stats: &Map<String, Value>, key: &str) -> bool {
    stats
        .get(&format!("{STAT_PERSONALITY}{key}"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        > 0
}

/// Turn this co-driver's personality on or off.
pub fn set_personality(stats: &mut Map<String, Value>, key: &str, on: bool) {
    let stat = format!("{STAT_PERSONALITY}{key}");
    if on {
        stats.insert(stat, Value::from(1));
    } else {
        stats.remove(&stat);
    }
}

/// The switch's row, as a menu reads it.
pub fn switch_label(name: &str, on: bool) -> String {
    if on {
        format!("{name}'s personality: on")
    } else {
        format!("{name}'s personality: off")
    }
}

/// The switch's help.
pub fn switch_help(name: &str) -> String {
    format!(
        "While you ride along, {name} now and then shows a habit of their own. \
         Nothing they do changes the drive."
    )
}

/// One kind of moment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Moment {
    /// At night, a passing truck is clear of the nose: a flash of the
    /// lights says come on over, the old courtesy between truckers.
    NightLightsClear,
    /// Between one and four in the morning, dry weather, a long while into
    /// the shift: the window comes down for some air, and back up.
    CrackWindow,
    /// Back from a fuel stop or a break with something for the bunk.
    CoffeeOnReturn,
    /// A car cuts in close in front.
    CutInMutter,
    /// Dan walks around the trailer at every stop before pulling out.
    DanWalkAround,
    /// Grandpa's Radio comes on: Rusty leaves it playing.
    RustyGrandpasRadio,
    /// A song starts on one of June's stations.
    JuneHums,
    /// A diner or a cafe a few miles ahead.
    JuneDiner,
    /// A long descent.
    MarisolGrades,
    /// The weather turns wet.
    WaltSawItComing,
    /// Dial-up Summer comes on: Dee stops flipping.
    DeeDialUpSummer,
    /// The Lone Courier on Channel 3000.
    NateLoneCourier,
    /// Late at night on a quiet straight.
    LouMeridian,
    /// A long descent with the old trainer.
    RayHills,
}

impl Moment {
    /// The log key, for the transcript and the tests.
    pub fn key(self) -> &'static str {
        match self {
            Moment::NightLightsClear => "night_lights_clear",
            Moment::CrackWindow => "crack_window",
            Moment::CoffeeOnReturn => "coffee_on_return",
            Moment::CutInMutter => "cut_in_mutter",
            Moment::DanWalkAround => "dan_walk_around",
            Moment::RustyGrandpasRadio => "rusty_grandpas_radio",
            Moment::JuneHums => "june_hums",
            Moment::JuneDiner => "june_diner",
            Moment::MarisolGrades => "marisol_grades",
            Moment::WaltSawItComing => "walt_saw_it_coming",
            Moment::DeeDialUpSummer => "dee_dial_up_summer",
            Moment::NateLoneCourier => "nate_lone_courier",
            Moment::LouMeridian => "lou_meridian",
            Moment::RayHills => "ray_hills",
        }
    }

    /// The cast member whose signature this is; `None` for one any
    /// co-driver can show.
    pub fn owner(self) -> Option<&'static str> {
        match self {
            Moment::DanWalkAround => Some("dan"),
            Moment::RustyGrandpasRadio => Some("rusty"),
            Moment::JuneHums | Moment::JuneDiner => Some("june"),
            Moment::MarisolGrades => Some("marisol"),
            Moment::WaltSawItComing => Some("walt"),
            Moment::DeeDialUpSummer => Some("dee"),
            Moment::NateLoneCourier => Some("nate"),
            Moment::LouMeridian => Some("lou"),
            Moment::RayHills => Some("ray"),
            _ => None,
        }
    }

    /// The chance it happens when its moment comes, for this co-driver.
    /// Zero for someone who never does it.
    pub fn chance(self, key: &str) -> f64 {
        if self.owner().is_some_and(|owner| owner != key) {
            return 0.0;
        }
        match self {
            // The careful ones never miss it; the restless ones mostly do.
            Moment::NightLightsClear => match key {
                "dan" | "ray" => 0.3,
                "walt" | "rusty" | "marisol" | "june" => 0.15,
                _ => 0.1,
            },
            Moment::CrackWindow => match key {
                "june" | "dee" => 0.0,
                _ => 0.3,
            },
            Moment::CoffeeOnReturn => 0.33,
            Moment::CutInMutter => match key {
                "rusty" | "nate" => 0.4,
                _ => 0.0,
            },
            Moment::JuneHums => 0.25,
            Moment::JuneDiner => 0.6,
            Moment::WaltSawItComing | Moment::LouMeridian => 0.8,
            _ => 1.0,
        }
    }
}

/// The weather Walt saw coming, in a word, when the sky turns from dry to
/// wet. `None` for any other change.
pub fn wet_turn(from: WeatherKind, to: WeatherKind) -> Option<&'static str> {
    if !matches!(from, WeatherKind::Clear | WeatherKind::Cloudy) {
        return None;
    }
    match to {
        WeatherKind::Rain | WeatherKind::HeavyRain => Some("rain"),
        WeatherKind::Thunderstorm => Some("storm"),
        WeatherKind::Snow => Some("snow"),
        _ => None,
    }
}

/// Whether a stop's name is a diner or a cafe, by whole word: Gardiner is
/// a town, not a diner.
pub fn is_diner_name(name: &str) -> bool {
    name.split(|c: char| !c.is_alphanumeric() && c != '\'')
        .any(|word| {
            let word = word.to_lowercase();
            matches!(word.as_str(), "diner" | "cafe" | "café")
        })
}

/// A stop's name without its direction, for June's line: "Ariston Cafe
/// (northbound)" is "Ariston Cafe".
pub fn place_without_direction(name: &str) -> &str {
    match name.find(" (") {
        Some(at) => name[..at].trim_end(),
        None => name.trim_end(),
    }
}

/// The words of a moment. `place` and `weather` fill the lines that name
/// one; `key` picks a co-driver's own wording where they have one.
pub fn moment_line(moment: Moment, name: &str, key: &str, place: &str, weather: &str) -> String {
    match moment {
        Moment::NightLightsClear => {
            format!("{name} flashes the lights to let a passing truck back in.")
        }
        Moment::CrackWindow => format!("{name} cracks the window for some air."),
        Moment::CoffeeOnReturn => {
            if key == "rusty" {
                format!("{name} brought you some jerky.")
            } else {
                format!("{name} brought you a coffee.")
            }
        }
        Moment::CutInMutter => {
            if key == "nate" {
                format!("{name} grumbles at a car that cut in.")
            } else {
                format!("{name} mutters something about four-wheelers.")
            }
        }
        Moment::DanWalkAround => format!("{name} walks around the trailer before pulling out."),
        Moment::RustyGrandpasRadio => format!("{name} leaves this one playing."),
        Moment::JuneHums => format!("{name} hums along."),
        Moment::JuneDiner => format!("{name} knows {place} from her diner days."),
        Moment::MarisolGrades => format!("{name} has hauled water down worse grades than this."),
        Moment::WaltSawItComing => format!("{name} saw this {weather} coming."),
        Moment::DeeDialUpSummer => format!("{name} leaves the dial alone for once."),
        Moment::NateLoneCourier => format!("{name} says this one is about him."),
        Moment::LouMeridian => format!("{name} starts to tell you about Meridian, then stops."),
        Moment::RayHills => format!("{name} takes the hill slow, the way he taught you."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::co_driver::CAST;

    #[test]
    fn every_switch_starts_off_and_turns_on_and_off() {
        let mut stats = Map::new();
        for member in CAST {
            assert!(!personality_on(&stats, member.key));
        }
        set_personality(&mut stats, "june", true);
        assert!(personality_on(&stats, "june"));
        assert!(!personality_on(&stats, "dan"));
        set_personality(&mut stats, "june", false);
        assert!(!personality_on(&stats, "june"));
        assert!(stats.is_empty(), "off leaves nothing in the save");
    }

    #[test]
    fn every_cast_member_has_a_signature() {
        const ALL: [Moment; 14] = [
            Moment::NightLightsClear,
            Moment::CrackWindow,
            Moment::CoffeeOnReturn,
            Moment::CutInMutter,
            Moment::DanWalkAround,
            Moment::RustyGrandpasRadio,
            Moment::JuneHums,
            Moment::JuneDiner,
            Moment::MarisolGrades,
            Moment::WaltSawItComing,
            Moment::DeeDialUpSummer,
            Moment::NateLoneCourier,
            Moment::LouMeridian,
            Moment::RayHills,
        ];
        for member in CAST {
            assert!(
                ALL.iter().any(|m| m.owner() == Some(member.key)),
                "{} has no signature",
                member.key
            );
        }
        for moment in ALL {
            if let Some(owner) = moment.owner() {
                assert_eq!(moment.chance("someone_else"), 0.0, "{}", moment.key());
                assert!(moment.chance(owner) > 0.0);
            }
            let line = moment_line(moment, "Dan", "dan", "Ariston Cafe", "rain");
            let words = line.split_whitespace().count();
            assert!(words <= 12, "{line}");
            assert!(line.starts_with("Dan "), "{line}");
            assert!(line.ends_with('.'), "{line}");
        }
    }

    #[test]
    fn diners_match_by_whole_word() {
        assert!(is_diner_name("Ariston Cafe (northbound)"));
        assert!(is_diner_name("Peggy Sue's Fifties Diner"));
        assert!(!is_diner_name("Gardiner Travel Plaza"));
        assert!(!is_diner_name("McAfee Rest Area"));
        assert_eq!(
            place_without_direction("Ariston Cafe (northbound)"),
            "Ariston Cafe"
        );
        assert_eq!(place_without_direction("Rock Cafe"), "Rock Cafe");
    }

    #[test]
    fn walt_notices_only_a_dry_sky_turning_wet() {
        assert_eq!(
            wet_turn(WeatherKind::Clear, WeatherKind::Rain),
            Some("rain")
        );
        assert_eq!(
            wet_turn(WeatherKind::Cloudy, WeatherKind::Thunderstorm),
            Some("storm")
        );
        assert_eq!(
            wet_turn(WeatherKind::Clear, WeatherKind::Snow),
            Some("snow")
        );
        assert_eq!(wet_turn(WeatherKind::Rain, WeatherKind::HeavyRain), None);
        assert_eq!(wet_turn(WeatherKind::Clear, WeatherKind::Fog), None);
    }
}
