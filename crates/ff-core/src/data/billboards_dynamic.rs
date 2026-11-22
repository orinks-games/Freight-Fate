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

use chrono::{Datelike, NaiveDate, Weekday};

use crate::data::billboard_law::{country_of, Country};

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
    /// The Fourth of July: the United States only.
    Independence,
    Halloween,
    /// United States Thanksgiving, in late November.
    Thanksgiving,
    Christmas,
    /// The first of July: Canada only.
    CanadaDay,
    /// Canadian Thanksgiving, the second Monday of October.
    CanadianThanksgiving,
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

/// One holiday sign, and where its copy is true. A sign belongs to the
/// countries that keep its holiday: a United States Thanksgiving board is
/// read only on a road in the United States, so its pie never turns up in
/// Quebec, while a Christmas board reads on both sides of the border.
/// Within its countries an empty `states` list is true on any road; a line
/// naming a region lists the states it holds in, so a Salem sign is never
/// read in Texas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HolidaySign {
    pub text: &'static str,
    pub countries: &'static [Country],
    pub states: &'static [&'static str],
}

impl HolidaySign {
    /// True in this state (a two-letter code). An unanchored line holds
    /// anywhere in its countries, and a road whose state is unknown counts
    /// as the United States, as the whole map was before 2.0. A regional
    /// line is never read where the state is unknown.
    pub fn holds_in(&self, state: Option<&str>) -> bool {
        self.countries.contains(&road_country(state))
            && (self.states.is_empty() || state.is_some_and(|s| self.states.contains(&s)))
    }
}

/// The country a road's state code is in; unknown counts as the United
/// States.
fn road_country(state: Option<&str>) -> Country {
    state.map_or(Country::UnitedStates, country_of)
}

const BOTH: &[Country] = &[Country::UnitedStates, Country::Canada];
const US: &[Country] = &[Country::UnitedStates];
const CANADA: &[Country] = &[Country::Canada];

/// A line true on any road of either country.
const fn anywhere(text: &'static str) -> HolidaySign {
    HolidaySign {
        text,
        countries: BOTH,
        states: &[],
    }
}

/// A line true on any road in the United States only.
const fn us_only(text: &'static str) -> HolidaySign {
    HolidaySign {
        text,
        countries: US,
        states: &[],
    }
}

/// A line true on any road in Canada only.
const fn canada_only(text: &'static str) -> HolidaySign {
    HolidaySign {
        text,
        countries: CANADA,
        states: &[],
    }
}

/// A line true only in the United States states it names.
const fn in_states(text: &'static str, states: &'static [&'static str]) -> HolidaySign {
    HolidaySign {
        text,
        countries: US,
        states,
    }
}

const NEW_ENGLAND: &[&str] = &["CT", "MA", "ME", "NH", "RI", "VT"];
const CORN_BELT: &[&str] = &["IA", "IL", "IN", "KS", "MN", "ND", "NE", "OH", "SD"];
const BORDER_STATES: &[&str] = &["AZ", "CA", "NM", "TX"];

const NEW_YEAR_SIGNS: &[HolidaySign] = &[
    anywhere("Happy New Year. Your resolution is a clean logbook. Big Jim's resolution is more clients."),
    anywhere("New Year's week. Somewhere a fireworks barn sold out of everything loud, and the dogs are already under the bed."),
];

const INDEPENDENCE_SIGNS: &[HolidaySign] = &[
    us_only("Happy Fourth of July. Somewhere a brother-in-law is lighting the loud ones. Mind the smoke."),
    us_only("Independence week. Flags on every porch, and a grill going in every yard for a thousand miles."),
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
    us_only("Thanksgiving week. Somewhere a turkey is grateful you are hauling freight and not him."),
    us_only("Happy Thanksgiving. Every pie case in the country is full, and every pair of pants is worried."),
];

const CANADA_DAY_SIGNS: &[HolidaySign] = &[
    canada_only("Happy Canada Day. The fireworks start at dusk. This far north in July, dusk takes its time."),
    canada_only("Canada Day week. Fireworks over every lake, and somebody's uncle in charge of the barbecue again."),
];

const CANADIAN_THANKSGIVING_SIGNS: &[HolidaySign] = &[
    canada_only("Happy Thanksgiving. It comes in October here, while the leaves are still worth looking at. The turkey has no opinion on the date."),
    canada_only("Thanksgiving weekend. Canada holds it in October, and gets to the pie weeks ahead of the neighbors."),
];

const CHRISTMAS_SIGNS: &[HolidaySign] = &[
    anywhere("Merry Christmas. Somewhere a nativity scene has a semi in it this year. It is parked legally."),
    anywhere("Big Jim wishes you a merry Christmas. Big Jim still wants you to call."),
    anywhere("Christmas week. The freight is presents now, and every dock wants it yesterday."),
];

/// What is behind the tractor that a cargo sign can answer. Every field is
/// something the drive already knows: the job's cargo, the trailer on the
/// fifth wheel, the driver's carrier.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hauling<'a> {
    /// Live cattle on the delivery leg of a livestock load.
    pub cattle: bool,
    /// A load that needs the reefer, with the trailer hooked.
    pub reefer: bool,
    /// A set of doubles hooked.
    pub doubles: bool,
    /// The carrier whose name is on the door, for a company or leased-on
    /// driver; None for independent authority or no carrier.
    pub carrier: Option<&'a str>,
}

/// The lines each kind of load can hear. A carrier line holds `{carrier}`
/// where the carrier's name is read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CargoPools {
    pub cattle: &'static [&'static str],
    pub reefer: &'static [&'static str],
    pub doubles: &'static [&'static str],
    pub carrier: &'static [&'static str],
}

/// Signs for a cattle hauler.
pub const CATTLE_LINES: &[&str] = &[
    "Welcome to cattle country. Your passengers already know.",
    "Easy on the brakes, driver. Everyone behind you is standing.",
    "Garden Gate Vegetarian Cafe. Endorsed by every passenger in your trailer.",
    "Big Earl's Steakhouse. If your trailer is mooing, keep driving. It upsets the customers.",
    "Leather goods outlet. Please park the trailer facing away.",
    "Cowboy church, Sundays. The herd is welcome. The herd stays in the trailer.",
];
/// Signs for a reefer load.
pub const REEFER_LINES: &[&str] = &[
    "Hauling cold? Somebody's ice cream is counting on you. No pressure.",
    "That hum behind your cab is a grocery store's whole weekend. Keep it cold.",
    "Lettuce does not care about your schedule. Lettuce cares about the temperature.",
    "Polar Pete's Ice Cream. Colder than your trailer. We checked.",
    "Fresh, never frozen, says the diner. Your trailer says otherwise.",
    "Frozen custard stand. Our freezer is older than yours, and it never complains.",
];
/// Signs for a set of doubles.
pub const DOUBLES_LINES: &[&str] = &[
    "Two trailers? Show-off.",
    "Buy one trailer, get one free. Looks like you already did.",
    "Backing lessons, by appointment. Doubles drivers, we cannot help you either.",
    "The second trailer goes where the first one went. That is the theory.",
    "Twin trailers: because one was not enough to worry about.",
];
/// Signs that name the driver's carrier.
pub const CARRIER_LINES: &[&str] = &[
    "{carrier} is hiring. You already work there. Tell a friend.",
    "Drive for {carrier}. Ask the driver in front of this sign how it is going.",
    "{carrier}: on time since this morning.",
    "How is my driving? {carrier} would also like to know.",
    "{carrier} salutes its drivers. This sign was cheaper than a raise.",
    "Proud home of {carrier}. Our drivers get home eventually.",
    "Big Jim Tolliver has never sued {carrier}. Keep it that way.",
    "This mile of highway is sponsored by {carrier}. The litter is not ours.",
];

/// The cargo lines that ship.
pub const CARGO_POOLS: CargoPools = CargoPools {
    cattle: CATTLE_LINES,
    reefer: REEFER_LINES,
    doubles: DOUBLES_LINES,
    carrier: CARRIER_LINES,
};

/// A cargo line takes one pool sign in this many, offset from the holiday
/// signs so a holiday week still shows through.
pub const CARGO_EVERY: usize = 3;
const CARGO_PHASE: usize = 1;

/// Of the cargo slots, the carrier takes one in this many (one pool sign in
/// nine), so a company driver is not read the carrier's name all career.
pub const CARRIER_EVERY: usize = 3;
const CARRIER_PHASE: usize = 2;

/// The cargo line for this sign, from these pools, or None.
///
/// Every third pool sign is a cargo slot. Every third cargo slot is the
/// carrier's, when the driver has one; the others draw only from the load
/// aboard (cattle, reefer, doubles), and stay everyday signs without one.
fn cargo_line(pools: &CargoPools, hauling: Hauling, nth: usize, offset: usize) -> Option<String> {
    if nth % CARGO_EVERY != CARGO_PHASE {
        return None;
    }
    let slot = nth / CARGO_EVERY;
    let carrier = hauling
        .carrier
        .map(str::trim)
        .filter(|name| !name.is_empty() && !pools.carrier.is_empty());
    if let Some(name) = carrier {
        if slot % CARRIER_EVERY == CARRIER_PHASE {
            let line = pools.carrier[(slot / CARRIER_EVERY + offset) % pools.carrier.len()];
            return Some(line.replace("{carrier}", name));
        }
    }
    let mut here: Vec<&str> = Vec::new();
    for (applies, pool) in [
        (hauling.cattle, pools.cattle),
        (hauling.reefer, pools.reefer),
        (hauling.doubles, pools.doubles),
    ] {
        if applies {
            here.extend_from_slice(pool);
        }
    }
    if here.is_empty() {
        return None;
    }
    // Count only the slots the load gets, so its pool is walked in order
    // around the carrier's.
    let ordinal = if carrier.is_some() {
        slot - (slot + CARRIER_EVERY - CARRIER_PHASE) / CARRIER_EVERY
    } else {
        slot
    };
    Some(here[(ordinal + offset) % here.len()].to_string())
}

/// Every sign a holiday puts up, wherever each one is true.
pub fn holiday_signs(holiday: Holiday) -> &'static [HolidaySign] {
    match holiday {
        Holiday::NewYear => NEW_YEAR_SIGNS,
        Holiday::Independence => INDEPENDENCE_SIGNS,
        Holiday::Halloween => HALLOWEEN_SIGNS,
        Holiday::Thanksgiving => THANKSGIVING_SIGNS,
        Holiday::Christmas => CHRISTMAS_SIGNS,
        Holiday::CanadaDay => CANADA_DAY_SIGNS,
        Holiday::CanadianThanksgiving => CANADIAN_THANKSGIVING_SIGNS,
    }
}

/// A holiday line takes one pool sign in this many. Signs stand thirty-five
/// to sixty-five miles apart, so that is one every couple of hundred miles.
pub const HOLIDAY_EVERY: usize = 4;

/// The small-hours signs take one pool sign in this many.
pub const SMALL_HOURS_EVERY: usize = 2;

/// Local hours the small-hours signs come out: one in the morning until five.
pub const SMALL_HOURS: std::ops::Range<f64> = 1.0..5.0;

/// The holiday whose week a calendar date falls in, on a road in this
/// state (a two-letter code; unknown counts as the United States).
///
/// United States Thanksgiving is the fourth Thursday of November, which
/// always lands between the 22nd and the 28th, so its week is that span.
/// Canadian Thanksgiving is the second Monday of October; its week is the
/// seven days ending on it. Canada Day's week runs June 28 to July 1.
pub fn holiday_on(date: NaiveDate, state: Option<&str>) -> Option<Holiday> {
    let canada = road_country(state) == Country::Canada;
    match (date.month(), date.day()) {
        (12, 29..=31) | (1, 1) => Some(Holiday::NewYear),
        (6, 28..=30) | (7, 1) if canada => Some(Holiday::CanadaDay),
        (6, 28..=30) | (7, 1..=4) if !canada => Some(Holiday::Independence),
        (10, 24..=31) => Some(Holiday::Halloween),
        (11, 22..=28) if !canada => Some(Holiday::Thanksgiving),
        (12, 10..=25) => Some(Holiday::Christmas),
        (10, _) if canada && canadian_thanksgiving_week(date) => {
            Some(Holiday::CanadianThanksgiving)
        }
        _ => None,
    }
}

/// Whether a date is in the seven days ending on the second Monday of
/// October.
fn canadian_thanksgiving_week(date: NaiveDate) -> bool {
    NaiveDate::from_weekday_of_month_opt(date.year(), 10, Weekday::Mon, 2)
        .is_some_and(|monday| (0..7).contains(&(monday - date).num_days()))
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
    /// What the trailer is hauling.
    pub hauling: Hauling<'a>,
}

/// The line to read in place of an everyday pool sign, or None to read it.
///
/// Big Jim answers every incident, once, at the next sign. The small hours
/// take every other sign, the load every third (the carrier's name one in
/// nine) and a holiday every fourth,
/// so the ordinary roadside still shows through. Each pool is walked in order
/// from the trip's offset, so no line comes back until the rest of its pool
/// has been read.
pub fn dynamic_billboard(moment: BillboardMoment) -> Option<String> {
    dynamic_billboard_from(&CARGO_POOLS, moment)
}

fn dynamic_billboard_from(cargo: &CargoPools, moment: BillboardMoment) -> Option<String> {
    if let Some(line) = fixed_billboard(moment) {
        return Some(line.to_string());
    }
    if let Some(line) = cargo_line(cargo, moment.hauling, moment.nth, moment.offset) {
        return Some(line);
    }
    holiday_billboard(moment).map(str::to_string)
}

/// Big Jim and the small hours: the swaps that outrank the load.
fn fixed_billboard(moment: BillboardMoment) -> Option<&'static str> {
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
    None
}

fn holiday_billboard(moment: BillboardMoment) -> Option<&'static str> {
    let walk = |len: usize, every: usize| (moment.nth / every + moment.offset) % len;
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

    const HOLIDAYS: [Holiday; 7] = [
        Holiday::NewYear,
        Holiday::Independence,
        Holiday::Halloween,
        Holiday::Thanksgiving,
        Holiday::Christmas,
        Holiday::CanadaDay,
        Holiday::CanadianThanksgiving,
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
    fn a_united_states_holiday_is_kept_off_canadian_roads() {
        let moment = |state| BillboardMoment {
            holiday: Some(Holiday::Thanksgiving),
            state,
            ..BillboardMoment::default()
        };
        for holiday in [Holiday::Independence, Holiday::Thanksgiving] {
            for sign in holiday_signs(holiday) {
                assert_eq!(sign.countries, US, "{}", sign.text);
            }
        }
        for state in ["QC", "ON", "BC", "YT"] {
            assert_eq!(holiday_billboard(moment(Some(state))), None, "{state}");
        }
        assert!(holiday_billboard(moment(Some("NY"))).is_some());
        assert!(holiday_billboard(moment(Some("AK"))).is_some());
        assert!(holiday_billboard(moment(None)).is_some());
    }

    fn day(y: i32, m: u32, d: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(y, m, d).unwrap()
    }

    #[test]
    fn holiday_weeks_cover_the_day_itself() {
        let us = |m, d| holiday_on(day(2026, m, d), Some("NY"));
        assert_eq!(us(1, 1), Some(Holiday::NewYear));
        assert_eq!(us(12, 31), Some(Holiday::NewYear));
        assert_eq!(us(7, 4), Some(Holiday::Independence));
        assert_eq!(us(7, 5), None);
        assert_eq!(us(10, 31), Some(Holiday::Halloween));
        assert_eq!(us(12, 25), Some(Holiday::Christmas));
        assert_eq!(us(12, 26), None);
        // The fourth Thursday of November: the 26th in 2026, the 22nd in 2029.
        assert_eq!(us(11, 26), Some(Holiday::Thanksgiving));
        assert_eq!(us(11, 22), Some(Holiday::Thanksgiving));
        assert_eq!(us(11, 21), None);
        assert_eq!(us(9, 30), None);
        assert_eq!(
            us(10, 12),
            None,
            "no Canadian Thanksgiving south of the line"
        );
        assert_eq!(
            holiday_on(day(2026, 7, 1), None),
            Some(Holiday::Independence)
        );
    }

    #[test]
    fn canada_keeps_its_own_holidays() {
        let ca = |y, m, d| holiday_on(day(y, m, d), Some("ON"));
        assert_eq!(ca(2026, 6, 28), Some(Holiday::CanadaDay));
        assert_eq!(ca(2026, 7, 1), Some(Holiday::CanadaDay));
        assert_eq!(ca(2026, 7, 2), None, "the Fourth is not Canada's");
        assert_eq!(ca(2026, 11, 26), None, "nor is November's Thanksgiving");
        assert_eq!(ca(2026, 12, 25), Some(Holiday::Christmas));
        assert_eq!(ca(2026, 1, 1), Some(Holiday::NewYear));
        assert_eq!(ca(2026, 10, 31), Some(Holiday::Halloween));
        // The second Monday of October: the 12th in 2026, the 13th in 2025.
        assert_eq!(ca(2026, 10, 12), Some(Holiday::CanadianThanksgiving));
        assert_eq!(ca(2026, 10, 6), Some(Holiday::CanadianThanksgiving));
        assert_eq!(ca(2026, 10, 5), None);
        assert_eq!(ca(2026, 10, 13), None);
        assert_eq!(ca(2025, 10, 13), Some(Holiday::CanadianThanksgiving));
        assert_eq!(ca(2025, 10, 6), None);
    }

    #[test]
    fn shared_holidays_read_in_both_countries() {
        let moment = |holiday, state| BillboardMoment {
            holiday: Some(holiday),
            state: Some(state),
            ..BillboardMoment::default()
        };
        for holiday in [Holiday::NewYear, Holiday::Halloween, Holiday::Christmas] {
            for state in ["NY", "QC", "BC"] {
                let line = holiday_billboard(moment(holiday, state));
                assert!(line.is_some(), "{holiday:?} in {state}");
            }
        }
        // A regional Halloween line keeps its states: Salem is never read in Quebec.
        let regional = holiday_signs(Holiday::Halloween)
            .iter()
            .filter(|s| !s.states.is_empty());
        for sign in regional {
            assert!(!sign.holds_in(Some("QC")), "{}", sign.text);
        }
        // Canada's own holidays read only north of the line.
        for holiday in [Holiday::CanadaDay, Holiday::CanadianThanksgiving] {
            assert!(holiday_billboard(moment(holiday, "ON")).is_some());
            assert_eq!(holiday_billboard(moment(holiday, "NY")), None);
        }
    }

    #[test]
    fn big_jim_answers_an_incident_before_anything_else() {
        let moment = BillboardMoment {
            incident: Some(Incident::Citation),
            tired: true,
            holiday: Some(Holiday::Christmas),
            ..Default::default()
        };
        assert!(CITATION_LINES.contains(&dynamic_billboard(moment).unwrap().as_str()));
        let moment = BillboardMoment {
            incident: Some(Incident::Collision),
            nth: 7,
            ..Default::default()
        };
        assert!(COLLISION_LINES.contains(&dynamic_billboard(moment).unwrap().as_str()));
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

    fn halloween_in(state: Option<&str>, nth: usize, offset: usize) -> Option<String> {
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
                .any(|line| line.as_deref() == Some(sign.text));
            assert!(
                !heard_somewhere_else,
                "{} read outside {:?}",
                sign.text, sign.states
            );
            let home = Some(sign.states[0]);
            let heard_at_home = (0..200)
                .any(|n| halloween_in(home, n * HOLIDAY_EVERY, 0).as_deref() == Some(sign.text));
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

    const TEST_POOLS: CargoPools = CargoPools {
        cattle: &["Cattle one.", "Cattle two."],
        reefer: &["Reefer one."],
        doubles: &["Doubles one."],
        carrier: &["{carrier} sign."],
    };

    fn cargo_at(hauling: Hauling, nth: usize) -> Option<String> {
        dynamic_billboard_from(
            &TEST_POOLS,
            BillboardMoment {
                hauling,
                nth,
                ..Default::default()
            },
        )
    }

    #[test]
    fn a_load_takes_one_pool_sign_in_three() {
        let cattle = Hauling {
            cattle: true,
            ..Default::default()
        };
        let swapped: Vec<usize> = (0..9).filter(|&n| cargo_at(cattle, n).is_some()).collect();
        assert_eq!(swapped, vec![1, 4, 7]);
        assert_eq!(cargo_at(Hauling::default(), 1), None);
    }

    #[test]
    fn only_the_load_aboard_is_answered() {
        let reefer = Hauling {
            reefer: true,
            ..Default::default()
        };
        assert_eq!(cargo_at(reefer, 1).as_deref(), Some("Reefer one."));
        let blank = Hauling {
            carrier: Some("  "),
            ..Default::default()
        };
        assert_eq!(cargo_at(blank, 7), None);
    }

    /// Which pool each of the first eighteen pool signs is read from:
    /// '.' an everyday sign, 'L' the load's, 'C' the carrier's.
    fn cadence(hauling: Hauling) -> String {
        (0..18)
            .map(|n| match cargo_at(hauling, n) {
                None => '.',
                Some(line) if line.contains("Mesa Freight") => 'C',
                Some(_) => 'L',
            })
            .collect()
    }

    #[test]
    fn the_carrier_takes_one_pool_sign_in_nine() {
        let mesa = Some("Mesa Freight");
        let table = [
            // A company driver with nothing special aboard.
            (
                Hauling {
                    carrier: mesa,
                    ..Default::default()
                },
                ".......C........C.",
            ),
            // Cattle for a company: the load's slots around the carrier's.
            (
                Hauling {
                    cattle: true,
                    carrier: mesa,
                    ..Default::default()
                },
                ".L..L..C..L..L..C.",
            ),
            // Cattle on one's own authority: every cargo slot is the load's.
            (
                Hauling {
                    cattle: true,
                    ..Default::default()
                },
                ".L..L..L..L..L..L.",
            ),
            (Hauling::default(), ".................."),
        ];
        for (hauling, want) in table {
            assert_eq!(cadence(hauling), want, "{hauling:?}");
        }
    }

    #[test]
    fn carrier_lines_never_mix_into_the_load() {
        let both = Hauling {
            reefer: true,
            carrier: Some("Mesa Freight"),
            ..Default::default()
        };
        for n in [1, 4, 10, 13] {
            assert_eq!(cargo_at(both, n).as_deref(), Some("Reefer one."));
        }
        assert_eq!(cargo_at(both, 7).as_deref(), Some("Mesa Freight sign."));
    }

    #[test]
    fn the_load_walks_its_pool_around_the_carrier() {
        let cattle = Hauling {
            cattle: true,
            carrier: Some("Mesa Freight"),
            ..Default::default()
        };
        let heard: Vec<_> = [1, 4, 10, 13]
            .iter()
            .map(|&n| cargo_at(cattle, n).unwrap())
            .collect();
        assert_eq!(
            heard,
            ["Cattle one.", "Cattle two.", "Cattle one.", "Cattle two."]
        );
    }

    #[test]
    fn big_jim_and_the_small_hours_outrank_the_load() {
        let moment = BillboardMoment {
            incident: Some(Incident::Collision),
            hauling: Hauling {
                doubles: true,
                ..Default::default()
            },
            nth: 1,
            ..Default::default()
        };
        let line = dynamic_billboard_from(&TEST_POOLS, moment).unwrap();
        assert!(COLLISION_LINES.contains(&line.as_str()));
    }

    #[test]
    fn a_cattle_pool_is_walked_before_it_repeats() {
        let cattle = Hauling {
            cattle: true,
            ..Default::default()
        };
        let heard: Vec<_> = [1, 4]
            .iter()
            .map(|&n| cargo_at(cattle, n).unwrap())
            .collect();
        assert_ne!(heard[0], heard[1]);
    }

    #[test]
    fn shipped_cargo_lines_are_speakable() {
        for line in [CATTLE_LINES, REEFER_LINES, DOUBLES_LINES, CARRIER_LINES].concat() {
            assert!(!line.chars().any(|c| c.is_ascii_digit()), "{line}");
            assert!(line.split_whitespace().count() <= 25, "{line}");
            let lower = line.to_lowercase();
            for claim in ["next exit", "miles ahead", "exit ", "turn off"] {
                assert!(!lower.contains(claim), "{line} names a stop");
            }
        }
        assert!(CARRIER_LINES.iter().all(|line| line.contains("{carrier}")));
    }
}
