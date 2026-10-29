//! Billboards that notice the drive (owner, 2026-09-30).
//!
//! The roadside pool is drawn when a trip starts, so it cannot know what the
//! drive will do. When an everyday pool sign comes up, the driving state may
//! read one of these in its place: Big Jim after a collision, a citation or an
//! out-of-service order; the church signs and all-night diners when the driver
//! is drowsy or it is the small hours; a holiday in its week. Placed
//! attraction signs never change: each one is a place.
//!
//! No line here names an exit, a distance or a service. A sign that sends a
//! driver who cannot see the road toward a stop that is not there costs the
//! drive, so each line is true on any road, on its day.

/// What happened since the last pool sign that Big Jim answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Incident {
    Collision,
    Citation,
    OutOfService,
}

/// A holiday whose week puts its own signs up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holiday {
    NewYear,
    Independence,
    Halloween,
    Thanksgiving,
    Christmas,
}

pub const COLLISION_LINES: &[&str] = &[
    "Big Jim felt that one from his office. Whatever you hit back there, one call, that's all.",
    "Hit something a few miles back? Big Jim is not saying it was your fault. Big Jim is not saying it wasn't.",
    "That new dent already has a lawyer. Big Jim Tolliver, attorney at law.",
    "Big Jim heard the crunch. Big Jim always hears the crunch.",
];

pub const CITATION_LINES: &[&str] = &[
    "Fresh ticket on the dash? Big Jim reads those for fun.",
    "Big Jim saw the trooper's lights back there. Big Jim sees all the lights. Call Big Jim.",
    "A citation is not the end of the road. It is the start of a phone call to Big Jim.",
];

pub const OUT_OF_SERVICE_LINES: &[&str] = &[
    "Parked by an inspector? Big Jim has argued with a few. He lost, but he was loud about it.",
    "Big Jim cannot fix your brakes. Big Jim can complain about the inspector, by the hour.",
];

/// Church signs and all-night diners, for the drowsy and the small hours.
pub const SMALL_HOURS_LINES: &[&str] = &[
    "Even the Lord rested on the seventh day. You could try the seventh hour.",
    "Open all night: coffee, pie, and a booth soft enough to sleep in. Nobody here will judge you.",
    "Your eyelids are heavy, and so is your load. Rest is holy too.",
    "All-night diner. The waitress has seen that face before. It looked more awake then.",
    "Sleep is a gift. Unwrap it somewhere legal.",
];

/// One holiday sign, and where its copy is true. An empty `states` list is
/// true on any road; a line naming a region lists the states it holds in, so
/// a Salem sign is never read in Texas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HolidaySign {
    pub text: &'static str,
    pub states: &'static [&'static str],
}

impl HolidaySign {
    /// True in this state (a two-letter code), or anywhere for an unanchored
    /// line. A regional line is never read where the state is unknown.
    pub fn holds_in(&self, state: Option<&str>) -> bool {
        self.states.is_empty() || state.is_some_and(|s| self.states.contains(&s))
    }
}

const fn anywhere(text: &'static str) -> HolidaySign {
    HolidaySign { text, states: &[] }
}

const fn in_states(text: &'static str, states: &'static [&'static str]) -> HolidaySign {
    HolidaySign { text, states }
}

const NEW_ENGLAND: &[&str] = &["CT", "MA", "ME", "NH", "RI", "VT"];
const CORN_BELT: &[&str] = &["IA", "IL", "IN", "KS", "MN", "ND", "NE", "OH", "SD"];
const BORDER_STATES: &[&str] = &["AZ", "CA", "NM", "TX"];

const NEW_YEAR_SIGNS: &[HolidaySign] = &[
    anywhere("Happy New Year. Your resolution is a clean logbook. Big Jim's resolution is more clients."),
    anywhere("New Year's week. Somewhere a fireworks barn sold out of everything loud, and the dogs are already under the bed."),
];

const INDEPENDENCE_SIGNS: &[HolidaySign] = &[
    anywhere("Happy Fourth of July. Somewhere a brother-in-law is lighting the loud ones. Mind the smoke."),
    anywhere("Independence week. Flags on every porch, and a grill going in every yard for a thousand miles."),
];

const HALLOWEEN_SIGNS: &[HolidaySign] = &[
    anywhere("Halloween week. The corn mazes are open, and the scarecrows are judging your lane discipline."),
    anywhere("Happy Halloween. The scariest thing on this road is still the weigh station."),
    anywhere("Halloween week. Somewhere a porch light is on, and the candy bowl is going fast."),
    anywhere("Trick or treat. Big Jim is handing out business cards this year. Nobody wants them."),
    anywhere("Costume idea: a driver with a clean logbook. Nobody will believe it."),
    anywhere("Halloween week. Kids in costumes are out after dark. Take it slow where the houses start."),
    anywhere("Haunted house season. Nothing in there is as frightening as a dock appointment before sunrise."),
    anywhere("Halloween week. The hayrides are running, and the tractors are slower than you."),
    in_states("Illinois grows more pumpkins than any other state. Most of them end up in a pie.", &["IL"]),
    in_states("Salem, Massachusetts, spends all of October on Halloween. Witches welcome. Trailers, less so.", &["MA"]),
    in_states("New England at Halloween. The leaves are mostly down, and the cider doughnuts are still hot.", NEW_ENGLAND),
    in_states("Sleepy Hollow is on the Hudson. The headless horseman rides all week. Keep your own head on the road.", &["NY"]),
    in_states("New Orleans takes its ghosts seriously. The cemetery tours are booked all week.", &["LA"]),
    in_states("Halloween week in farm country. Harvest is still on, so watch for grain trucks pulling out slow.", CORN_BELT),
    in_states("Día de los Muertos is days away. The marigolds and sugar skulls are already out.", BORDER_STATES),
];

const THANKSGIVING_SIGNS: &[HolidaySign] = &[
    anywhere("Thanksgiving week. Somewhere a turkey is grateful you are hauling freight and not him."),
    anywhere("Happy Thanksgiving. Every pie case in the country is full, and every pair of pants is worried."),
];

const CHRISTMAS_SIGNS: &[HolidaySign] = &[
    anywhere("Merry Christmas. Somewhere a nativity scene has a semi in it this year. It is parked legally."),
    anywhere("Big Jim wishes you a merry Christmas. Big Jim still wants you to call."),
    anywhere("Christmas week. The freight is presents now, and every dock wants it yesterday."),
];

/// Every sign a holiday puts up, wherever each one is true.
pub fn holiday_signs(holiday: Holiday) -> &'static [HolidaySign] {
    match holiday {
        Holiday::NewYear => NEW_YEAR_SIGNS,
        Holiday::Independence => INDEPENDENCE_SIGNS,
        Holiday::Halloween => HALLOWEEN_SIGNS,
        Holiday::Thanksgiving => THANKSGIVING_SIGNS,
        Holiday::Christmas => CHRISTMAS_SIGNS,
    }
}

/// A holiday line takes one pool sign in this many. Signs stand thirty-five
/// to sixty-five miles apart, so that is one every couple of hundred miles.
pub const HOLIDAY_EVERY: usize = 4;

/// The small-hours signs take one pool sign in this many.
pub const SMALL_HOURS_EVERY: usize = 2;

/// Local hours the small-hours signs come out: one in the morning until five.
pub const SMALL_HOURS: std::ops::Range<f64> = 1.0..5.0;

/// The holiday whose week a calendar date falls in.
///
/// Thanksgiving is the fourth Thursday of November, which always lands
/// between the 22nd and the 28th, so its week is that span.
pub fn holiday_on(month: u32, day: u32) -> Option<Holiday> {
    match (month, day) {
        (12, 29..=31) | (1, 1) => Some(Holiday::NewYear),
        (6, 28..=30) | (7, 1..=4) => Some(Holiday::Independence),
        (10, 24..=31) => Some(Holiday::Halloween),
        (11, 22..=28) => Some(Holiday::Thanksgiving),
        (12, 10..=25) => Some(Holiday::Christmas),
        _ => None,
    }
}

/// What the drive offers a pool sign to react to.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct BillboardMoment<'a> {
    pub incident: Option<Incident>,
    /// Drowsy, or the small hours on the local clock.
    pub tired: bool,
    pub holiday: Option<Holiday>,
    /// Pool signs already heard this trip: paces the swaps and walks each
    /// pool in order, so a trip does not hear one line twice in a row.
    pub nth: usize,
    /// Where this trip starts in each pool, so one trip does not open with
    /// the line the last one did.
    pub offset: usize,
    /// The two-letter state the truck is in, if known.
    pub state: Option<&'a str>,
}

/// The line to read in place of an everyday pool sign, or None to read it.
///
/// Big Jim answers every incident, once, at the next sign. The small hours
/// take every other sign and a holiday every fourth, so the ordinary roadside
/// still shows through. Each pool is walked in order from the trip's offset,
/// so no line comes back until the rest of its pool has been read.
pub fn dynamic_billboard(moment: BillboardMoment) -> Option<&'static str> {
    let walk = |len: usize, every: usize| (moment.nth / every + moment.offset) % len;
    if let Some(incident) = moment.incident {
        let pool = match incident {
            Incident::Collision => COLLISION_LINES,
            Incident::Citation => CITATION_LINES,
            Incident::OutOfService => OUT_OF_SERVICE_LINES,
        };
        return Some(pool[walk(pool.len(), 1)]);
    }
    if moment.tired && moment.nth.is_multiple_of(SMALL_HOURS_EVERY) {
        let pool = SMALL_HOURS_LINES;
        return Some(pool[walk(pool.len(), SMALL_HOURS_EVERY)]);
    }
    let holiday = moment.holiday?;
    if !moment.nth.is_multiple_of(HOLIDAY_EVERY) {
        return None;
    }
    let here: Vec<&'static str> = holiday_signs(holiday)
        .iter()
        .filter(|sign| sign.holds_in(moment.state))
        .map(|sign| sign.text)
        .collect();
    (!here.is_empty()).then(|| here[walk(here.len(), HOLIDAY_EVERY)])
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOLIDAYS: [Holiday; 5] = [
        Holiday::NewYear,
        Holiday::Independence,
        Holiday::Halloween,
        Holiday::Thanksgiving,
        Holiday::Christmas,
    ];

    fn every_line() -> Vec<&'static str> {
        let mut lines: Vec<&str> = [
            COLLISION_LINES,
            CITATION_LINES,
            OUT_OF_SERVICE_LINES,
            SMALL_HOURS_LINES,
        ]
        .concat();
        for holiday in HOLIDAYS {
            lines.extend(holiday_signs(holiday).iter().map(|sign| sign.text));
        }
        lines
    }

    #[test]
    fn every_line_is_speakable_and_promises_no_stop() {
        for line in every_line() {
            assert!(!line.chars().any(|c| c.is_ascii_digit()), "{line}");
            assert!(line.split_whitespace().count() <= 25, "{line}");
            let lower = line.to_lowercase();
            for claim in ["next exit", "miles ahead", "exit ", "turn off"] {
                assert!(!lower.contains(claim), "{line} names a stop");
            }
        }
    }

    #[test]
    fn holiday_weeks_cover_the_day_itself() {
        assert_eq!(holiday_on(1, 1), Some(Holiday::NewYear));
        assert_eq!(holiday_on(12, 31), Some(Holiday::NewYear));
        assert_eq!(holiday_on(7, 4), Some(Holiday::Independence));
        assert_eq!(holiday_on(7, 5), None);
        assert_eq!(holiday_on(10, 31), Some(Holiday::Halloween));
        assert_eq!(holiday_on(12, 25), Some(Holiday::Christmas));
        assert_eq!(holiday_on(12, 26), None);
        // The fourth Thursday of November: the 26th in 2026, the 22nd in 2029.
        assert_eq!(holiday_on(11, 26), Some(Holiday::Thanksgiving));
        assert_eq!(holiday_on(11, 22), Some(Holiday::Thanksgiving));
        assert_eq!(holiday_on(11, 21), None);
        assert_eq!(holiday_on(9, 30), None);
    }

    #[test]
    fn big_jim_answers_an_incident_before_anything_else() {
        let moment = BillboardMoment {
            incident: Some(Incident::Citation),
            tired: true,
            holiday: Some(Holiday::Christmas),
            ..Default::default()
        };
        assert!(CITATION_LINES.contains(&dynamic_billboard(moment).unwrap()));
        let moment = BillboardMoment {
            incident: Some(Incident::Collision),
            nth: 7,
            ..Default::default()
        };
        assert!(COLLISION_LINES.contains(&dynamic_billboard(moment).unwrap()));
    }

    #[test]
    fn the_ordinary_roadside_still_shows_through() {
        let tired = |nth| BillboardMoment {
            tired: true,
            nth,
            ..Default::default()
        };
        assert!(dynamic_billboard(tired(0)).is_some());
        assert!(dynamic_billboard(tired(1)).is_none());
        let holiday = |nth| BillboardMoment {
            holiday: Some(Holiday::Halloween),
            nth,
            ..Default::default()
        };
        let swapped = (0..12).filter(|&n| dynamic_billboard(holiday(n)).is_some());
        assert_eq!(swapped.count(), 3);
        assert_eq!(dynamic_billboard(BillboardMoment::default()), None);
    }

    fn halloween_in(state: Option<&str>, nth: usize, offset: usize) -> Option<&'static str> {
        dynamic_billboard(BillboardMoment {
            holiday: Some(Holiday::Halloween),
            nth,
            offset,
            state,
            ..Default::default()
        })
    }

    #[test]
    fn a_regional_holiday_sign_is_read_only_in_its_states() {
        for sign in HALLOWEEN_SIGNS
            .iter()
            .filter(|sign| !sign.states.is_empty())
        {
            let heard_somewhere_else = ["TX", "WA", "FL", "GA"]
                .into_iter()
                .filter(|state| !sign.states.contains(state))
                .chain([""])
                .flat_map(|state| {
                    let state = (!state.is_empty()).then_some(state);
                    (0..200).map(move |n| halloween_in(state, n * HOLIDAY_EVERY, 0))
                })
                .any(|line| line == Some(sign.text));
            assert!(
                !heard_somewhere_else,
                "{} read outside {:?}",
                sign.text, sign.states
            );
            let home = Some(sign.states[0]);
            let heard_at_home =
                (0..200).any(|n| halloween_in(home, n * HOLIDAY_EVERY, 0) == Some(sign.text));
            assert!(heard_at_home, "{} never read in {:?}", sign.text, home);
        }
    }

    #[test]
    fn no_holiday_line_repeats_before_its_pool_runs_out() {
        for state in [None, Some("MA"), Some("TX"), Some("IL")] {
            let pool = HALLOWEEN_SIGNS.iter().filter(|s| s.holds_in(state)).count();
            assert!(pool >= 8, "only {pool} Halloween signs in {state:?}");
            let heard: Vec<_> = (0..pool)
                .map(|n| halloween_in(state, n * HOLIDAY_EVERY, 3).unwrap())
                .collect();
            let mut distinct = heard.clone();
            distinct.sort_unstable();
            distinct.dedup();
            assert_eq!(distinct.len(), pool, "{state:?} repeated: {heard:?}");
        }
    }

    #[test]
    fn the_next_trip_opens_on_a_different_holiday_line() {
        assert_ne!(halloween_in(None, 0, 0), halloween_in(None, 0, 1));
        // Christmas has three lines: every one of them gets its turn.
        let christmas: Vec<_> = (0..3)
            .map(|n| {
                dynamic_billboard(BillboardMoment {
                    holiday: Some(Holiday::Christmas),
                    nth: n * HOLIDAY_EVERY,
                    ..Default::default()
                })
            })
            .collect();
        assert_eq!(
            christmas
                .iter()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            3
        );
    }
}
