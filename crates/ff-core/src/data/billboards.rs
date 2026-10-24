//! Roadside billboard content -- the spoken flavor you pass on the
//! interstate (port of `freight_fate/data/billboards.py`).
//!
//! THIS SIDE DELIBERATELY LEADS THE PYTHON as of 2026-08-24. The port was a
//! line-for-line copy until a tester reported Oklahoma billboards being read
//! in Tennessee; the anchoring below is the fix, and the Python original still
//! has the shield-only lookup that caused it. The copy itself is unchanged in
//! both -- what moved is where each line is allowed to be read, plus two lines
//! evicted from the anywhere pool for naming places. Anyone reconciling the
//! two should port the anchors FORWARD, not strip them back.
//!
//! Billboards are ambient roadside color: short, funny, occasionally corridor-
//! specific signs the event voice reads on the low-priority ambient tier
//! (safety callouts always preempt them). This is the CONTENT layer -- the
//! sign copy, authored as data so it stays the map/content author's lane and
//! unit-tests with no audio. The placement/scheduling that actually speaks
//! them (riding the navigation-cue and ambient-chatter machinery) is
//! gameplay-layer follow-on.
//!
//! Two placement modes the content is written for:
//!
//! * ANYWHERE -- the corridor-agnostic pools (generic Americana, attorney ads,
//!   church signs, roadside oddities), drawn from a seeded per-trip RNG so a
//!   drive is deterministic and offline. THE ONLY THING THAT MAY GO IN THESE
//!   POOLS is a line that would be true beside any road in the country: an
//!   invented diner, a joke about the road, an adult superstore that names no
//!   town. A line naming a real town, region, exit, attraction, or a regional
//!   chain belongs to a corridor or a state-gated genre pool, not here.
//! * PLACED -- `CORRIDOR_BILLBOARDS` maps an interstate shield to signs for
//!   the real roadside culture of that route, so a South Dakota Interstate 90
//!   run passes the "free ice water, three hundred miles to go" genre and a
//!   Mojave Interstate 15 run passes alien jerky.
//!
//! A shield alone is not a place. Interstate 40 runs through Oklahoma AND
//! Tennessee, so keying only on the shield read Okemah and Muskogee to drivers
//! outside Knoxville -- a billboard is one of the few things telling a driver
//! who cannot see the road where they are, and a misplaced one is the game
//! asserting something untrue about the truck's position. So every corridor
//! line carries a `SignAnchor` saying where its copy is true, and the placer
//! refuses it anywhere else.
//!
//! Real roadside attractions are named the way real truck-stop brands already
//! are (nominative -- a driver really does pass Wall Drug on Interstate 90).
//! The sign COPY is original parody, not lifted ad text; how closely to echo
//! a real slogan is an owner call, like the radio-licensing and Big Buck's
//! decisions.
//!
//! Player-facing speech: no codes, no map tags, and numbers spelled in words
//! so a screen reader never reads a bare figure ("nine ninety-nine", not
//! "9.99").
//!
//! The 2026-08-12 owner batch (see the pools tagged with that date) is the one
//! deliberate exception: those lines are final tester-round copy the owner
//! asked to ship verbatim, numerals included ("$4.89/gallon", "37 miles", "6
//! spaces", "3rd"/"#2", "362 days"). The billboards tests carry a matching,
//! explicit allow-list for exactly those lines -- the digit-free rule still
//! applies to everything else, including any future addition to these same
//! pools.
//!
//! SONG TRIBUTES (2026-08-12): every achievement in achievements.py credits a
//! song, and the highway pays some of them back. Tribute signs name the artist
//! and the song title only -- never a lyric, never a quoted line -- and read
//! as home-turf pride, not advertising. A tribute that names a place lives in
//! `CORRIDOR_BILLBOARDS` under that shield with an anchor; only the ones whose
//! whole point is being from everywhere at once live in
//! `SONG_TRIBUTE_BILLBOARDS`, which the random picker reaches for about one
//! draw in ten (`TRIBUTE_DRAW_CHANCE`) so the roadside stays attorneys,
//! fireworks, and pie, with a tribute as an occasional treat rather than a
//! museum wall.

use once_cell::sync::Lazy;

use crate::pyrandom::PyRandom;

pub const BILLBOARDS_SOURCE: &str =
    "Original parody billboard copy evoking real interstate roadside culture; \
     real attraction names used nominatively, ad text invented.";

pub const GENERIC_BILLBOARDS: &[&str] = &[
    "Did you eat today? Thank a trucker.",
    "Last real coffee for two hundred miles. This is not a drill.",
    "Prime rib buffet, nine ninety-nine. Cardiologist not included.",
    "Hitchhikers may be escaping inmates. Drive friendly.",
    "Home of the fifty-pound cinnamon roll. Bring a friend. Bring two.",
    "Gun show and craft fair this weekend. Something for everyone.",
    "You are now leaving the middle of nowhere. Come back soon.",
    // 2026-08-12 owner batch -- roadfood and attraction signs from the tester
    // round. "3rd-best" and "#2" keep their numerals verbatim (owner sign-off,
    // see the digit-exemption note in test_billboards.py); everything else in
    // this pool stays digit-free by the usual rule.
    "Big Bob's Burgers: burgers so big, the Department of Transportation requires a second trailer.",
    "Mamma's Diner: home cooking! Because apparently your home doesn't have a kitchen.",
    "The Last Chance Cafe: 37 miles back was The First Chance Cafe.",
    "World's 3rd-best pie: we used to be #2. Then Jerry complained.",
    "Uncle Josh's Steakhouse: steaks so tender they're legally considered missing.",
    "Next exit: biggest pancake in the state! Probably. We haven't checked.",
    "Free coffee! With purchase of coffee. Extra refills fifty cents each.",
    "Next exit: homemade jerky and a gift shop that sells the same jerky.",
    "We have ice. We have bait. We have opinions about your bumper sticker.",
    "Diesel, showers, and a fried pie that will change your itinerary.",
    "All-you-can-eat catfish. Bring a bigger belt.",
    "World's largest frying pan. Breakfast is served. Bring a forklift.",
    "Homemade pie, next exit. Made this morning. The coffee is older.",
    "Fresh peaches, next exit. Pick your own or grab a bag.",
    "Antiques and ammunition. One building. Two hobbies.",
    "If you lived here, you'd be home by now. Nobody lives here.",
    "Truck wash, next exit. Your trailer used to be white.",
    "The coffee is fine. The pie is better. The gossip is unbeatable.",
    "Concrete deer and yard gnomes, next exit. We help you load them.",
    "Gift shop, next exit. Dashboard hula girls, half off.",
    "Local honey. Local jam. Local opinions, free with every purchase.",
    "The steak is bigger than the plate. The plate is bigger than your budget.",
    "Next exit: homemade ice cream. The cows are local. The freezer is older than you.",
    "Gun show this weekend, craft fair the next. Same tent.",
    "Buffet: if it isn't on a steam table, it isn't dinner.",
];

// Fireworks-barn genre -- southern/midwest interstate staple. Moved out of
// GENERIC so the pool can grow without mixing genres. No place names. Copy
// lives here; placement is state-gated via `regional_genre_signs` (not Anywhere).
pub const FIREWORKS_BILLBOARDS: &[&str] = &[
    "Fireworks, fireworks, fireworks. You're already past it.",
    "Fireworks barn, next exit. If you can still hear, you haven't shopped enough.",
    "Family fireworks. Professional regret. Open till the sheriff gets here.",
    "Buy one crate, get the ringing in your ears free.",
    "Stand back. Light fuse. That's the whole business plan. Next exit.",
    "Roman candles, bottle rockets, and a very optimistic fire extinguisher.",
    "Closed on Christmas. Open on every other bad idea.",
    "Fireworks warehouse. No smoking. We are serious. Look at the inventory.",
    "Your brother-in-law already bought the loud ones. Catch up, next exit.",
];

// Pecan-stand genre -- south-central interstate gift-shop food. Invented
// stands; no town names. Copy lives here; placement is TX/GA/LA/AL/MS only.
pub const PECAN_BILLBOARDS: &[&str] = &[
    "World's largest pecan. You'll smell it before you see it. Next exit.",
    "Pecan logs, pecan pie, pecan everything. Your passenger will complain. Buy two.",
    "If it isn't pecans, it isn't a gift. Next exit, we will not be argued with.",
    "World's second-largest pecan. The biggest one is three exits back and they know it.",
    "Fresh roasted pecans. The sample is free. The bag is not.",
    "Pecan pralines, next exit. Sticky steering wheels since nineteen fifty.",
    "Nuts. We sell them. We attract them. Next exit.",
    "Pecan brittle so hard it needs a load rating.",
];

// Adult-superstore / gentleman's-club / adult-bookstore / XXX-theater genre --
// rural interstate off-ramp staple. Opaque teasing register (Lion's Den
// next-exit vinyl): families share the road, so the board is vaguer than the
// store. Invented copy, no real-chain slogans, no named towns. Always eligible
// wherever commercial boards are legal (ME/VT/AK/HI gate). Grok Build rewrite
// 2026-09-16.
pub const ADULT_BILLBOARDS: &[&str] = &[
    "Adult superstore, next exit. Truckers welcome. We will not tell.",
    "Adult bookstore, next exit. Magazines you will not read. Parking you will use.",
    "Eighteen and over. Eighteen wheels preferred. Next exit.",
    "The sign just says Superstore. You already know which one. Next exit.",
    "Late night, well lit, no questions. Your logbook does not need this stop.",
    "Adult gifts, next exit. For someone else, obviously.",
    "If the billboard is this vague, the store is not. Next exit.",
    "Gentleman's club, next exit. Cold beer. Warm welcome. Dispatch stays outside.",
    "XXX theater, next exit. Dark room. Cheap seats. Nobody looking at you.",
    "Open all night. Cash if you prefer. Adult superstore, next exit.",
    "Adult bookstore, next exit. The back room is in the back. You already knew.",
    "No cover for truckers. Cover your tracks yourself. Next exit.",
    "Feature starts when you sit down. Adult theater, next exit.",
    "We sell what the other stores will not name. Superstore, next exit.",
    "Your CB will not mention this stop. Adult superstore, next exit.",
    "Last chance before morning. Gentleman's club, next exit. Last call is whenever.",
    "Locked cabinet. Open mind. Adult bookstore, next exit.",
    "Couples welcome. Couples optional. Next exit.",
    "Big lot. Tall doors. Adult superstore. You will fit.",
    "Continuous shows. Discreet exits. XXX theater, next exit.",
    "Stage is small. Tips are not. Gentleman's club, next exit.",
    "Come for the magazines. Leave with a bag you will hide. Bookstore, next exit.",
];

// Local-radio genre -- small-market AM promotion that travels anywhere without
// claiming a particular town or call sign.
pub const REGIONAL_RADIO_BILLBOARDS: &[&str] = &[
    "AM radio, this hour: farm report, funeral notices, and a song you forgot you loved.",
    "Gospel hour on the local AM. The preacher is selling a tent, not a timeshare.",
    "Your local station: three songs, two ads, and a birthday shout-out for someone named Dale.",
    "Classic country all night. We play the hits and the ones the hits replaced.",
    "AM trucking radio. Static included at no extra charge.",
    "The station that plays only driving songs. You are the whole audience.",
    "Tune us in. Tune the other guy out. All request, no sleep.",
    "Overnight trucker radio. Road reports and old country till sunrise.",
    "Ten thousand watts of somebody's uncle with a stack of records.",
    "If your radio still has a knob, we still have a tower.",
    "Weather, crop reports, and a hymn at sunrise. You know the station.",
    "All truck, all night, all the same three commercials.",
    "Keep it here. The next station is somebody's talk show and a car dealership.",
];

// Travel-plaza / hotel / QSR genre -- OAAA interstate staples. Invented copy;
// a few nominative chain names with original parody, never lifted slogans.
pub const TRAVEL_PLAZA_BILLBOARDS: &[&str] = &[
    // Love's / Pilot: national travel-center chains. Broad Anywhere is honest.
    "Love's ahead. Diesel, a shower, and a coffee that will keep you legal for one more state.",
    "Pilot next exit. Parking if you are lucky. Coffee if you are desperate.",
    // Generic travel-center / motel / QSR -- invented, no regional chain.
    "Travel center: showers, diesel, and a gift shop selling hats you already own.",
    "Motel vacancy. Free ice. Free Wi-Fi. Free regret about the mattress.",
    "Chain hotel, next exit. Continental breakfast starts when the waffle iron wakes up.",
    "Burger drive-thru, next exit. The bag is small. The line is not.",
    "Exit food: burgers, fries, and a soda the size of a fuel can.",
    "Truck parking and a sit-down special. The special is that they still have parking.",
];

// Named regional chains. Copy is nominative original parody; placement is
// state-gated in `regional_genre_signs` because these brands do not exist
// beside every American interstate.
pub const SHEETZ_BILLBOARD: &str =
    "Sheetz next exit. Made-to-order, open late, and somehow always has a line.";
pub const WAWA_BILLBOARD: &str =
    "Wawa, next exit. Hoagies, coffee, and a parking lot that thinks it is a city.";
pub const RACETRAC_BILLBOARD: &str =
    "RaceTrac next exit. Fuel, drinks, and a bathroom you will actually use.";
pub const CRACKER_BARREL_BILLBOARD: &str =
    "Cracker Barrel, next exit. Rocking chairs out front, biscuits inside.";

/// Southern/midwest fireworks-stand country. Consumer fireworks barns are an
/// interstate staple here; New England and the far West are not that genre.
/// Bubba's on I-95 stays SC+NC on top of this list.
pub const FIREWORKS_STAND_STATES: &[&str] = &[
    "AL", "AR", "FL", "GA", "IA", "IL", "IN", "KS", "KY", "LA", "MI", "MO", "MS", "NC", "OH", "OK",
    "PA", "SC", "TN", "TX", "VA", "WI", "WV",
];

/// South-central pecan-stand country (Ruth: TX/GA/LA/AL/MS).
pub const PECAN_STATES: &[&str] = &["AL", "GA", "LA", "MS", "TX"];

pub const SHEETZ_STATES: &[&str] = &["MD", "MI", "NC", "OH", "PA", "VA", "WV"];
pub const WAWA_STATES: &[&str] = &["DE", "FL", "MD", "NJ", "PA", "VA"];
/// RaceTrac's real southeast-plus-Texas footprint.
// RaceTrac-branded stores as of 2026-09 (cstoredive.com: a first store in
// Indianapolis, a travel center at Findlay, Ohio, North Carolina in late
// 2025). Arkansas has
// only RaceWay, the franchise brand; Virginia has one travel-center site
// bought and none confirmed open.
pub const RACETRAC_STATES: &[&str] = &[
    "AL", "FL", "GA", "IN", "KY", "LA", "MS", "NC", "OH", "SC", "TN", "TX",
];
/// Cracker Barrel as a South/Midwest roadside, not a West-Coast or New England board.
pub const CRACKER_BARREL_STATES: &[&str] = &[
    "AL", "AR", "FL", "GA", "IA", "IL", "IN", "KS", "KY", "LA", "MI", "MN", "MO", "MS", "NC", "NE",
    "OH", "OK", "SC", "TN", "TX", "VA", "WI", "WV",
];

// Dated movie/TV promo genre -- now-showing / coming-soon register only.
// Never a permanent "this is franchise country" claim. No place names.
pub const POP_CULTURE_BILLBOARDS: &[&str] = &[
    "Now showing: a movie about a truck. You are living the sequel.",
    "Coming soon to the drive-in: a movie about a truck. You've seen the real thing.",
    "Weekend double feature at the drive-in. Windows up if it rains. Windows down if it smells.",
    "Now playing: something with explosions. Popcorn is extra.",
    "Coming Friday: the one with the car chase. Please do not practice on this interstate.",
    "Matinee special. Matinee is when you should be sleeping. We know.",
    "Streaming now, and somehow also on a billboard. Eyes on the road.",
    "Now showing: a comedy about a road trip. You already know how it ends.",
    "Coming soon: a documentary about highways. You could have narrated it.",
    "Drive-in movies, weekends only. Two features for one ticket.",
];

// The truck-wreck attorney genre -- a real interstate staple, and gently meta in
// a trucking sim. Big Jim is invented.
pub const ATTORNEY_BILLBOARDS: &[&str] = &[
    "Injured in a truck wreck? Not this trip, we hope. But remember Big Jim.",
    "One call, that's all. Big Jim Tolliver, attorney at law and bass fisherman.",
    "Hurt on the job? Big Jim gets you paid. Big Jim gets Big Jim paid more.",
    "Eighteen wheels of justice. Big Jim sues trucks. Awkward, we know.",
    "Big Jim, attorney. He's got as many problems as pounds, so he gets yours. Life broke you? He'll sue whoever.",
    "Big Jim saw that lane change. He is not mad. He is drafting.",
    "Rollover? Call Big Jim. He answers on the first ring.",
    "Big Jim Tolliver: because your insurance company has a lawyer too.",
    "Whiplash? Big Jim has a cousin who had that. Call him anyway.",
    "Big Jim does not sleep. Big Jim's paralegal does not sleep. The bill does not sleep.",
    "Hurt? Not your fault? Big Jim agrees, and he has not even met you.",
    "Big Jim's number is on the next six boards. You will remember it.",
    "Work injury? Big Jim used to drive. Now he sues the people who do.",
    "Big Jim. Not the other Jim. The billboard Jim. Write it on the visor.",
    "If the other guy had a lawyer, you would want Big Jim.",
];

// Church-sign genre -- earnest, punny, occasionally threatening.
pub const FAITH_BILLBOARDS: &[&str] = &[
    "Jesus is watching. So is the weigh station. Slow down.",
    "Where will you spend eternity? Smoking or non-smoking?",
    "Got God? Give him a try, he'll ride shotgun or side saddle with ya any day or night",
    "Honk if you love the Lord. Text and drive if you'd like to meet him.",
    "God answers knee-mail.",
    "A missed exit is not a sign from God. The next weigh station might be.",
    "Pray for travelers. Then use your turn signal.",
    "Church bake sale, next exit. Salvation and a cookie, in that order.",
    "God is good. The coffee at the next truck stop is merely adequate.",
    "Don't make me come down there. Signed, God. Also your safety director.",
    "This is your sign. The actual church is the next exit.",
    "Sunday service, seven in the morning. The doughnuts go first.",
    "Eternity is a long haul. Pack accordingly.",
    "Blessed are the peacemakers, and the folks who stay out of the left lane.",
    "Got faith? Keep both hands on the wheel anyway.",
];

// The mystery-spot / two-headed-snake / see-the-thing genre. Invented
// attractions only: a sign naming a real one (the twine ball, the rocking
// chair, the ketchup bottle, petrified wood) lives in `PLACED_ODDITIES`
// with the states it is true in, because a billboard is one of the few
// things telling a blind driver where they are (owner, 2026-09-16).
pub const ROADSIDE_ODDITIES: &[&str] = &[
    "Mystery Spot ahead. Gravity is a suggestion here. Nine ninety-five.",
    "See the two-headed rattlesnake. Alive-ish. Next exit.",
    "Alligator farm and fudge shop. Yes, the same building.",
    "Living through chemistry, get your chemistry fix next four exits.",
    "Zoo! Visit the animals or be one!",
    "See the albino alligator. He is on break. The gift shop is not.",
    "Gravity hill ahead. Your truck already knew.",
    "Mystery house. Crooked floors. Straight prices. Nine ninety-five.",
    "Live rattlesnakes. Dead air conditioning. Next exit.",
    "Cave tours, next exit. The crystals in the gift shop are glass.",
    "Two-headed calf, stuffed. One-headed cashier, not. Next exit.",
    "See the thing in a jar. We will not say which jar. Nine dollars.",
    "Miniature village, next exit. Your rig will not fit down Main Street.",
    "Tornado museum, next exit. Step inside the storm cellar.",
];

// Trucker-services genre -- signs pitched straight at the driver, not the
// tourist: fuel, parking, a bed, a nap, or someone offering to take the truck
// off their hands. 2026-08-12 owner batch. "6 spaces" and "$4.89/gallon" keep
// their numerals verbatim (owner sign-off, see the digit-exemption note in
// test_billboards.py).
pub const TRUCKER_SERVICES_BILLBOARDS: &[&str] = &[
    "Tired of driving? Pull over. Take a nap. Your dispatcher will still be mad.",
    "Your truck deserves better. You deserve a raise. Neither is happening here.",
    "We buy used trucks! Even the ones that sound like a washing machine full of rocks.",
    "Truckers welcome! Trucks less welcome. Our parking lot only has 6 spaces.",
    "Need diesel? Of course you do. $4.89/gallon. Please cry inside.",
    "Rest easy at the Budget Palace! Luxury not included.",
    "Showers with actual hot water. We are as surprised as you are.",
    "Truck parking, next exit. One spot left, behind the dumpster.",
    "Diesel and a hot dog that's been on the roller grill since Tuesday. Next exit.",
    "We put out cones for truck parking. Cars park in them anyway. Next exit.",
    "Tire shop that does not flinch at your recaps. Next exit.",
    "Bugs on the windshield? Free squeegee with every fill-up. Next exit.",
    "CB shop, next exit. New antennas, and a guy who will talk your ear off.",
    "Reefer repair. If it is warm, we can tell from here.",
    "The lot has spaces. They are occupied by people who said they would only be a minute.",
    "Fuel desk open all night. The smile closes at ten.",
    "Need a shower, a stall, and a twenty-minute lie-down. We can do two of those.",
    "We buy wrecks, runners, and whatever is smoking behind you. Cash today.",
];

// Fourth-wall genre -- the billboard knows you're reading it. 2026-08-12 owner
// batch.
pub const META_BILLBOARDS: &[&str] = &[
    "Congratulations! You have driven past another billboard.",
    "Don't like this billboard? Keep driving.",
];

/// Where a corridor sign is TRUE -- the geography a line's copy asserts.
///
/// A billboard is one of the few things that tells a driver who cannot see
/// the road where they are, so a sign naming a town is a claim about the
/// truck's position. Keying only on the shield made every such claim true the
/// whole length of an interstate: Interstate 40 runs through Oklahoma AND
/// Tennessee, so Okemah and Muskogee were being read outside Knoxville.
/// Every line now says where it holds, and the placer refuses it anywhere
/// else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SignAnchor {
    /// True the length of the shield, because the line names no place --
    /// an invented business, or a joke about the road itself.
    Corridor,
    /// True only inside these states (two-letter postal codes). The honest
    /// anchor for a line naming a region, or a town the world does not model
    /// as a node.
    States(&'static [&'static str]),
    /// True only while one of these cities is still AHEAD on the route and
    /// within `within_mi` of road. A billboard is read from a road on the way
    /// to the thing, so the window is measured along the route, not as the
    /// crow flies.
    Approaching {
        cities: &'static [&'static str],
        within_mi: f64,
    },
}

/// How far ahead of a named place its sign may be read, in road miles.
///
/// Signs land every `BILLBOARD_MIN_GAP_MI`..`BILLBOARD_MAX_GAP_MI` (thirty-five
/// to sixty-five), so a window has to clear sixty-five by a wide margin or an
/// anchored sign gets one coin-flip per trip and mostly never speaks. A
/// hundred and fifty gives it two to four chances while staying inside the
/// genre: the game's own longest-lead copy is Big Buck's at two hundred and
/// sixty-two miles, and that distance IS the joke. At a hundred and fifty,
/// "Jacksonville ahead" reaches back into Georgia and no further.
pub const SIGN_APPROACH_MI: f64 = 150.0;

/// One corridor sign: the copy, and where that copy is true.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CorridorSign {
    pub text: &'static str,
    pub anchor: SignAnchor,
}

/// A sign true anywhere on its shield -- it names no place.
const fn anywhere_on(text: &'static str) -> CorridorSign {
    CorridorSign {
        text,
        anchor: SignAnchor::Corridor,
    }
}

/// A sign true only in these states.
const fn in_states(text: &'static str, states: &'static [&'static str]) -> CorridorSign {
    CorridorSign {
        text,
        anchor: SignAnchor::States(states),
    }
}

/// A sign true only while one of these cities is ahead, within the standard
/// approach.
const fn approaching(text: &'static str, cities: &'static [&'static str]) -> CorridorSign {
    CorridorSign {
        text,
        anchor: SignAnchor::Approaching {
            cities,
            within_mi: SIGN_APPROACH_MI,
        },
    }
}

// Corridor-keyed signs, mapped by INTERSTATE shield. The lookup normalizes
// "I-90", "I 90" and "Interstate 90" to the same corridor, and deliberately
// does NOT match "US-90" or "AZ-90": a sign written for Interstate 90's
// roadside is not true beside a different road that happens to share a number.
pub const CORRIDOR_BILLBOARDS: &[(&str, &[CorridorSign])] = &[
    ("I-90", &[
        // Wall Drug is Wall, South Dakota. Official campaign boards cover
        // SD / WY / western MN (Argus Leader); farthest cited is Greybull WY
        // ~394 mi. Montana stays off -- too far west of the real pool.
        // Only a line with no distance or "ahead" lives here: a state anchor
        // cannot tell which side of Wall the truck is on. The countdown is
        // placed data on the legs through Wall, one set per direction.
        in_states("Wall Drug. Five-cent coffee since your grandfather was your age.", &["SD", "WY", "MN"]),
        // Landmark attraction -- Cleveland's ticketed Rock Hall.
        approaching("Cleveland ahead. The Rock and Roll Hall of Fame sits on the lake. Chuck Berry, Aretha Franklin, and a glass pyramid you can actually visit.", &["cleveland_oh_us"]),
        // Song tributes -- Boston (The Willis Brothers), Moorcroft, Wyoming
        // (Chancey Williams), and the Idaho panhandle (Colby Acuff).
        approaching("Boston ahead. The Willis Brothers needed forty acres to turn a rig around in this town. It hasn't gotten any wider.", &["boston_ma_us"]),
        in_states("Wyoming, Land of the Buffalo. Chancey Williams sings it from right up the road. Buffalo cross wherever they please.", &["WY"]),
        in_states("Idaho panhandle country. Colby Acuff and the Western White Pines both grew up here.", &["ID"]),
        // Paying attraction -- World's Only Corn Palace, Mitchell SD on I-90.
        in_states("Mitchell, South Dakota -- the Corn Palace is a real building they redecorate with corn. You may look. The pigeons already did.", &["SD"]),
        // Paying attraction -- Spam Museum, Austin MN on I-90.
        in_states("Minnesota. The Spam Museum is a real place they built for a canned lunch. Samples included. The trailer is not.", &["MN"]),
    ]),
    ("I-95", &[
        // South of the Border's "ahead" signs are a placed countdown on the
        // legs either side of Dillon, one set per direction, for the same
        // reason as Wall Drug: a state anchor reads them after the exit.
        // 2026-08-12 owner batch -- Carolinas fireworks-stand country, the
        // genre South of the Border already trades on. "362 days" keeps its
        // numerals verbatim (owner sign-off, see test_billboards.py).
        // Carolinas fireworks country only -- never Maine I-95 (commercial boards banned).
        in_states("Bubba's Fireworks: open 362 days a year! Closed on the other three because we're usually in the hospital.", &["SC", "NC"]),
        // Song tributes -- the Jersey Turnpike (Elle King) and Jacksonville
        // (Lynyrd Skynyrd). Haynesville Woods was radio memory, not a paid board.
        in_states("New Jersey: more state than it gets credit for. Elle King sings one called Jersey Giant.", &["NJ"]),
        approaching("Jacksonville ahead, hometown of Lynyrd Skynyrd. Down here even the breeze plays guitar -- listen to their Call Me the Breeze.", &["jacksonville_fl_us"]),
        // Peanut-stand genre -- southside Virginia I-95, not a New England board.
        in_states("Virginia peanuts, next exit. Cooked in the shell. Your cab will smell like a ballpark for a week.", &["VA"]),
    ]),
    // A line saying "next exit" or "ahead" about one real place cannot be
    // state-anchored: it would be read anywhere in the state, either way.
    // Those are placed data now (signsheets/pool-moves-2026-09-30.md):
    // the Cabazon dinosaurs, Baker's alien jerky, Little America, the
    // Grapevine, Hope, Muskogee and the Casey rocking chair. The Thing's
    // "two hundred miles of suspense" was cut; its placed countdown says it.
    ("I-10", &[
        // Song tributes -- the southern transcontinental collects song towns:
        // Phoenix, Houston, Baton Rouge, Biloxi, El Paso, and the Big Thicket.
        approaching("Phoenix ahead, eventually. The desert gave us By the Time I Get to Phoenix.", &["phoenix_az_us"]),
        approaching("Houston ahead. Larry Gatlin measured this trip in days and just called the song Houston.", &["houston_tx_us"]),
        // Gilley's, the Urban Cowboy honky-tonk, stood in Pasadena outside
        // Houston until it closed in nineteen eighty-nine and burned the year
        // after; the bull El Toro went to Gilley's Dallas. Past tense on
        // purpose: a board promising the Pasadena room would be a lie.
        approaching("Houston ahead. Gilley's, the Urban Cowboy honky-tonk, stood in Pasadena until the eighties. The bull moved to Dallas. The boots stayed.", &["houston_tx_us"]),
        approaching("Baton Rouge ahead. Kris Kristofferson set Me and Bobby McGee hitchhiking out of here. Pick up the song, not the hitchhikers.", &["baton_rouge_la_us"]),
        in_states("Biloxi by two? Only if you keep it moving. Ellis Bullard makes it sound easy.", &["MS"]),
        approaching("El Paso, out past the haze. Marty Robbins sang El Paso City and the Streets of Laredo. West Texas gave him the material.", &["el_paso_tx_us"]),
        approaching("The Big Thicket, off to the north -- deep pines and deeper voices. George Jones grew up in there. Welcome to Possum country.", &["beaumont_tx_us", "houston_tx_us"]),
        // Tejano genre -- south Texas approaches only, not a statewide claim.
        approaching("San Antonio ahead. Fiesta in the spring, Tejano all year. The Alamo does not take eighteen-wheelers.", &["san_antonio_tx_us"]),
        // Landmark attraction -- a ticketed New Orleans jazz venue.
        approaching("New Orleans ahead. Preservation Hall still sells a seat for the jazz. Leave the trailer off the Quarter.", &["new_orleans_la_us"]),
        // Food boards -- Acadiana on I-10.
        in_states("Cajun country. Boudin, cracklins, and a drive-thru that does not need a window. Next exit.", &["LA"]),
    ]),
    ("I-15", &[
        in_states("The Mad Greek. Gyros in the middle of the Mojave. Trust the desert.", &["CA"]),
        // Song tribute -- Las Vegas (Elvis Presley).
        approaching("Las Vegas ahead. Elvis said Viva. The lights are on all night.", &["las_vegas_nv_us"]),
        // Casino genre -- Las Vegas is an approach, not all of Interstate 15.
        approaching("Las Vegas ahead. Another casino, another buffet bigger than your trailer. The odds are not.", &["las_vegas_nv_us"]),
    ]),
    ("I-40", &[
        in_states("Historic Route sixty-six. Get your kicks, then get back on schedule.", &["CA", "AZ", "NM", "TX", "OK"]),
        // Song tributes -- the old Route Sixty-Six corridor is wall-to-wall
        // song country: Winslow, Memphis, Muskogee, Okemah, the Smokies, and
        // the mother road itself.
        in_states("Historic Route Sixty-Six, right under your wheels. Bobby Troup gave it a song and half the country followed. The old road still takes visitors.", &["CA", "AZ", "NM", "TX", "OK"]),
        approaching("Winslow, Arizona, home of the world-famous corner. The Eagles sang Take It Easy about it, so the town built a park. Statue included.", &["winslow_az_us"]),
        approaching("Memphis, on down the road. Every highway in Tennessee gets there eventually. Tom T. Hall named his route That's How I Got to Memphis.", &["memphis_tn_us"]),
        // Landmark attraction -- Graceland is a ticketed Memphis site.
        approaching("Memphis ahead. Graceland is a real house with a real ticket line. Elvis lived there. The jumpsuits are in the museum.", &["memphis_tn_us"]),
        // Casino genre -- Oklahoma City's casino boards belong on its approach.
        approaching("Oklahoma City ahead. Casino billboards outnumber the cattle. You have been warned.", &["oklahoma_city_ok_us"]),
        in_states("Okemah, Oklahoma. Home of Woody Guthrie. This Land Is Your Land. This billboard is somebody else's.", &["OK"]),
        approaching("East Tennessee, home of Dolly Parton. The Smokies raised her. Nobody's worked a longer shift with a bigger smile.", &["knoxville_tn_us"]),
        // Ticketed Memphis tourism -- Beale Street, not a Music Highway state marker.
        approaching("Memphis. Beale Street still sells a ticket. The freight uses a different door.", &["memphis_tn_us"]),
        // Ticketed Nashville tourism -- Grand Ole Opry. Not I-40 Music Highway state signs.
        approaching("Nashville ahead. The Grand Ole Opry still sells a ticket. Bring a song or bring freight. They take both.", &["nashville_tn_us"]),
        approaching("Albuquerque ahead. Red chile, green chile, and a sky that does not quit. Fuel up.", &["albuquerque_nm_us"]),
    ]),
    ("I-80", &[
        in_states("One of the world's largest porch swings. Seats eighteen. Zero of them truckers.", &["NE"]),
        // Song tribute -- San Francisco Bay at the far western end (Otis
        // Redding).
        // Otis is the San Francisco bay, not the Humboldt. Nevada I-80 is
        // the wrong end of this road.
        in_states("The Dock of the Bay is at the far end of this road. Otis Redding held the best seat. No parking for trailers.", &["CA"]),
        // Casino genre -- Reno only, never a Nevada-wide anchor.
        approaching("Reno ahead. Biggest Little City, and the neon starts early. Cash the bonus, not the truck.", &["reno_nv_us"]),
        // Paying truck-stop landmark -- Iowa 80, Walcott IA on I-80.
        in_states("Iowa Eighty, the world's largest truck stop, is on this road. Parking is a competitive sport. So is the food court.", &["IA"]),
    ]),
    // 2026-08-12 owner batch -- the Rockies climb, where the scenery genuinely
    // earns the joke.
    ("I-70", &[
        anywhere_on("Have you seen the scenery? Neither has your driver!"),
        // Song tributes -- the Front Range (Joe Walsh) and Kansas City
        // (Roger Miller). Black Bear Road is a Jeep trail, not a paid board
        // on Interstate 70.
        // Westbound only: the Rockies are behind anyone driving east.
        approaching("The Rockies, straight ahead and getting bigger. Joe Walsh saw this view and wrote Rocky Mountain Way.", &["denver_co_us"]),
        approaching("Kansas City ahead. Roger Miller gave it Kansas City Star, the same year he was King of the Road.", &["kansas_city_mo_us"]),
        // Paying attraction -- OZ Museum, Wamego KS (I-70 billboards documented).
        in_states("Kansas sky, as advertised. The Oz Museum is a few exits off this road in Wamego. Ruby slippers not required.", &["KS"]),
        approaching("Saint Louis ahead. The Gateway Arch is the big one. You may look. The trailer stays on this side of the river.", &["st_louis_mo_us"]),
    ]),
    // Song tributes -- the Missouri and Oklahoma road. Franklin County,
    // Missouri (Union, Pacific, Saint Clair, Sullivan) carries the Franklin
    // County Trucking Company's name, by owner order; no source places the
    // band there (its founders are in Illinois and Kentucky), so the line
    // claims the name only (owner, 2026-09-30). Tulsa belongs to Don Williams.
    ("I-44", &[
        // Moved from Interstate 40 Arkansas: Meramec Caverns is Stanton,
        // Missouri, on Interstate 44, a real paid-board attraction.
        in_states("Meramec-style caverns ahead. Outlaws hid here. So can you, for the price of a ticket.", &["MO"]),
        in_states("Franklin County, Missouri, same name as the Franklin County Trucking Company. If you drive, they already wrote your song.", &["MO"]),
        approaching("Tulsa ahead. Set your watch to Tulsa Time. Don Williams says it runs a little easier.", &["tulsa_ok_us"]),
        // Casino genre -- the Tulsa approach.
        approaching("Tulsa ahead. Casino lights off the right. Don't bet the load.", &["tulsa_ok_us"]),
        // Paying venue -- Blueberry Hill / Chuck Berry room, Delmar Loop, St. Louis approach on I-44.
        approaching("Saint Louis ahead. Blueberry Hill on Delmar is a real room; Chuck Berry played its Duck Room for years. The duck walk is not a traffic pattern.", &["st_louis_mo_us"]),
    ]),
    // Song tributes -- the Texas-to-Minnesota main street of country music:
    // San Antonio, Austin, Waco, Abbott, Fort Worth, and Wichita.
    ("I-35", &[
        in_states("Abbott, Texas -- Willie Nelson's hometown. He's on the road again.", &["TX"]),
        approaching("Waco ahead. Croy and the Boys wrote Don't Let Me Die in Waco. The city would like everyone to relax.", &["waco_tx_us"]),
        approaching("Austin ahead. Dale Watson territory -- honky-tonk for people who read weigh station signs.", &["austin_tx_us"]),
        approaching("San Antonio, down the road. Western swing was born in Texas and Bob Wills drove it. New San Antonio Rose still blooms.", &["san_antonio_tx_us"]),
        // Tejano genre -- south Texas approaches only, never all of Texas.
        approaching("Laredo ahead. Tejano on the AM and the river just south. Keep the accordion, skip the sightseeing detour.", &["laredo_tx_us"]),
        approaching("San Antonio ahead. Tejano weekend on the AM. Conjunto is a dance hall and this highway.", &["san_antonio_tx_us"]),
        // Casino genre -- Oklahoma City's casino boards belong on its approach.
        approaching("Oklahoma City ahead. Native casino country. The boards started a hundred miles ago.", &["oklahoma_city_ok_us"]),
        // McAllen is I-2 / I-69C / US-281, not Interstate 35 (I-35 ends at
        // Laredo). The Valley Tejano board was killed rather than moved onto
        // an unmapped shield. Laredo and San Antonio keep the I-35 Tejano lane.
        // Paying venue -- Billy Bob's Texas, Fort Worth.
        approaching("Fort Worth ahead. Billy Bob's Texas is a real honky-tonk with a zip code. Bob Wills already got the other board.", &["fort_worth_tx_us"]),
        // The Longhorn Ballroom, Dallas: Bob Wills opened it in nineteen
        // fifty, the Sex Pistols played it in nineteen seventy-eight, and it
        // reopened restored in twenty twenty-three.
        approaching("Dallas ahead. The Longhorn Ballroom, built for Bob Wills, is open again. Freight to the dock, not the dance floor.", &["dallas_tx_us"]),
        // Kolache stands at West, Texas on I-35 -- invented copy, no stolen bakery slogan.
        approaching("Waco ahead. West, Texas, is kolache country. The bakery has been stopping traffic since the interstate was new.", &["waco_tx_us"]),
        in_states("Flat Kansas: wheat, sky, and telephone poles. Wichita Lineman was about a man up one of these.", &["KS"]),
        // Paying attraction -- Mall of America, Bloomington MN, at Interstate
        // 494 and Minnesota 77, just south of Minneapolis. The world has no
        // Bloomington node, so the sign rides the Minneapolis approach.
        approaching("Minneapolis ahead. The Mall of America is just south of town in Bloomington, with a roller coaster under the roof. The parking ramps were not built for trailers.", &["minneapolis_mn_us"]),
    ]),
    // Song tributes -- the Central Valley grade and the Bakersfield Sound.
    ("I-5", &[
        in_states("Bakersfield Sound country. Buck Owens and Merle Haggard tuned it, Red Simpson trucked it, Dwight Yoakam kept it running. Turn it up.", &["CA"]),
        // Moved off the corridor-less tribute pool: "far off this road" is a
        // claim about WHERE the truck is, and on the national pool it was
        // being read in Georgia. Redwood country is the far northern end of
        // this run and nowhere else.
        in_states("Redwood country, far off this road: trees taller than your rig is long. Andrew Gabbard wrote one called Redwood.", &["CA", "OR"]),
        // Landmark attraction -- Seattle's ticketed music museum.
        approaching("Seattle ahead. The museum of pop culture is the colorful blob by the Needle. Jimi Hendrix is inside; the rain is not.", &["seattle_wa_us"]),
        // Paying venue -- Buck Owens' Crystal Palace, Bakersfield.
        in_states("Bakersfield. The Bakersfield Sound still plays. The freight just passes through.", &["CA"]),
    ]),
    // Song tributes -- the Delta highway: Dyess, Arkansas (Johnny Cash) and
    // the old rail line to New Orleans (Willie Nelson's version).
    ("I-55", &[
        in_states("Arkansas Delta bottomland. Johnny Cash grew up picking cotton out here in Dyess. Hold your lane and walk the line.", &["AR"]),
        approaching("The old rail line to New Orleans runs this same stretch. Willie Nelson sang the City of New Orleans down it.", &["new_orleans_la_us"]),
    ]),
    // Song tributes -- Alabama into Tennessee: Hank Williams's home state,
    // Birmingham (Andrea and Mud), and Nashville (Curtis Grimes).
    ("I-65", &[
        in_states("Alabama, Hank Williams's home state. Move It On Over -- he meant the dog, but the left lane applies.", &["AL"]),
        approaching("Birmingham by eight thirty in the morning? It's been done. Andrea and Mud wrote a song about it.", &["birmingham_al_us"]),
        approaching("Nashville ahead, where a songwriter waits ten years for one hit. Curtis Grimes wrote Ten Year Town about the wait.", &["nashville_tn_us"]),
        // Ticketed Nashville tourism -- Ryman Auditorium.
        approaching("Nashville, Music City. The Ryman Auditorium is the mother church. Hats off, then back on the interstate.", &["nashville_tn_us"]),
    ]),
    // Song tribute -- Fort Payne, Alabama, hometown of the band Alabama.
    ("I-59", &[
        in_states("Fort Payne, Alabama -- hometown of the band Alabama. They sang Roll On for every eighteen wheeler on this road.", &["AL"]),
    ]),
    // Song tributes -- the long north-south haul: Saginaw and Detroit at the
    // top, Macon, Georgia at the bottom.
    ("I-75", &[
        approaching("Detroit City ahead. Bobby Bare sang it for every homesick southerner on the assembly lines up here.", &["detroit_mi_us"]),
        approaching("Saginaw, Michigan, up the interstate. Lefty Frizzell made a fishing town famous. The bay is cold.", &["saginaw_mi_us"]),
        approaching("Macon, Georgia -- the Allman Brothers' town. Southbound never sounded better than it does on this stretch.", &["macon_ga_us"]),
        // Produce stands -- not See Rock City.
        in_states("Georgia peaches, next few exits. The stands are real. The claims about whose are best are advertising.", &["GA"]),
        in_states("Florida citrus, next few exits. The bags are heavy. The claims about fresh are mostly true.", &["FL"]),
        in_states("Horse country. The Kentucky Horse Park is a real farm with a Hall of Champions. Your trailer is not invited to the paddock.", &["KY"]),
    ]),
    // Song tributes -- Atlanta owns this corridor: Jerry Reed, Alan Jackson,
    // and Gladys Knight all call it home.
    ("I-85", &[
        approaching("Atlanta ahead, Jerry Reed's town. He made hauling freight in a hurry flat-out famous -- East Bound and Down.", &["atlanta_ga_us"]),
        in_states("Newnan, Georgia raised Alan Jackson. He learned to drive on the back roads out here and wrote Drive about it.", &["GA"]),
        approaching("Atlanta ahead. Gladys Knight caught the Midnight Train home to it.", &["atlanta_ga_us"]),
    ]),
    // Song tribute -- Chattanooga (Glenn Miller).
    ("I-24", &[
        approaching("Chattanooga ahead. The Choo Choo is a real train, parked at the old terminal station. No ticket required.", &["chattanooga_tn_us"]),
    ]),
    // Song tributes -- Wisconsin gave trucking Dave Dudley; Detroit gave
    // everyone Motown and Bob Seger.
    ("I-94", &[
        in_states("Wisconsin made Dave Dudley, and Dave Dudley made Six Days on the Road. Every truck stop jukebox since owes him a quarter.", &["WI"]),
        approaching("Detroit, the Motown assembly line. Marvin Gaye, Stevie Wonder, and no mountain high enough to slow the freight.", &["detroit_mi_us"]),
        approaching("Detroit builds engines, and it built Bob Seger. Night Moves and Turn the Page came off these roads.", &["detroit_mi_us"]),
        in_states("Wisconsin. Cheese, really good cheese, and a dairy billboard that has been up since your last inspection.", &["WI"]),
        approaching("Milwaukee ahead. The Harley-Davidson Museum is a real building. The bikes inside are not street-legal, and neither is your trailer in the lobby.", &["milwaukee_wi_us"]),
        // Paying attraction -- Mall of America, Bloomington MN, at Interstate
        // 494 and Minnesota 77, just south of Minneapolis. The world has no
        // Bloomington node, so the sign rides the Minneapolis approach.
        approaching("Minneapolis ahead. The Mall of America is just south of town in Bloomington, with a roller coaster under the roof. The parking ramps were not built for trailers.", &["minneapolis_mn_us"]),
    ]),
    // I-96 Ionia County / Billy Strings and I-71 Cincinnati / Arlo McKinley
    // were radio memory, not paid boards. Pulled; no replacement songs.
    // Song tributes -- Charleston, West Virginia raised Red Sovine and Kathy
    // Mattea both.
    ("I-77", &[
        approaching("Charleston, West Virginia raised Red Sovine, the voice of every ghost rig and truck stop prayer. Give the Phantom the right of way.", &["charleston_wv_us"]),
        in_states("Kathy Mattea grew up just down the river. Her big one, Eighteen Wheels and a Dozen Roses, is about the last run before retirement.", &["WV"]),
        // Moved off the corridor-less tribute pool for the same reason: a
        // holler is an Appalachian hollow, and "the little valleys off this
        // road" only holds up on a road in those mountains. Rewritten
        // 2026-08-20 after a tester was honestly befuddled by the old
        // three-fragment form -- spoken text gets one pass at the ear, so it
        // has to carry its own context.
        in_states("Hollers ahead: a porch, a banjo, somebody picking. Dirty Grass Soul sang Back to the Holler. Leave the trailer; a holler road can't turn a rig.", &["WV", "VA", "KY", "NC"]),
    ]),
    // Song tribute -- Smoky Mountain fog (Flatt and Scruggs). Russell County
    // / Forty-Nine Winchester was radio memory, not a paid board.
    ("I-81", &[
        in_states("Mountain fog ahead, the local specialty. Think Foggy Mountain Breakdown, and use low beams.", &["VA", "TN", "WV"]),
        // Paying museum -- Birthplace of Country Music, Bristol on the state line.
        in_states("Bristol, on the state line. The Birthplace of Country Music Museum is a real hall. Carter Family country starts here.", &["TN", "VA"]),
    ]),
    // Song tributes -- Kentucky: the bluegrass east (Brit Taylor) and the
    // coalfields (Tennessee Ernie Ford).
    ("I-64", &[
        in_states("Eastern Kentucky, where the grass really does look blue. Brit Taylor wrote Kentucky Blue about home.", &["KY"]),
        in_states("Coal country. Tennessee Ernie Ford counted Sixteen Tons of it and famously came up broke.", &["KY", "WV", "VA"]),
        approaching("Louisville ahead. They make the bats here. The Louisville Slugger Museum is the giant one you cannot miss.", &["louisville_ky_us"]),
    ]),
    // Hope, Arkansas (Brennen Leigh) is placed at its exit; see above I-10.
    ("I-30", &[
        // The Longhorn Ballroom, Dallas: Bob Wills opened it in nineteen
        // fifty, the Sex Pistols played it in nineteen seventy-eight, and it
        // reopened restored in twenty twenty-three.
        approaching("Dallas ahead. The Longhorn Ballroom, built for Bob Wills, is open again. Freight to the dock, not the dance floor.", &["dallas_tx_us"]),
    ]),
    // I-8 Mexican Radio / Wall of Voodoo was radio memory of Rio Grande
    // border-blasters, not a paid board on Interstate 8. Pulled.
    // Song tribute -- Nazareth, Pennsylvania (The Band).
    ("I-78", &[
        in_states("Nazareth, Pennsylvania, a few exits north. The Weight is about a stranger rolling in looking for a bed. Book ahead.", &["PA"]),
    ]),
    // Song tribute -- Abilene, Texas (George Hamilton the Fourth).
    ("I-20", &[
        // Moved off the corridor-less tribute pool 2026-08-21: that pool is
        // for songs whose whole point is being from everywhere at once, and
        // these two name a place. On the loose pool they played in Maine
        // (owner). I-20 is the West Texas run they are actually about.
        in_states("West Texas cotton flats made Waylon Jennings. One question -- Are You Sure Hank Done It This Way -- and outlaw country was born.", &["TX"]),
        in_states("Somewhere out there is Lubbock, Texas. Mac Davis kept it in his rear view mirror until he missed it.", &["TX"]),
        approaching("Abilene ahead. George Hamilton the Fourth made the town sound gentle as a Sunday. Watch for crosswinds.", &["abilene_tx_us"]),
        // Gilley's Dallas, open since two thousand three with the original
        // bull from the film; an event hall now, not a nightly honky-tonk.
        approaching("Dallas ahead. Gilley's Dallas keeps the original bull from Urban Cowboy. Eight seconds is the ride. The dock appointment is not.", &["dallas_tx_us"]),
    ]),
    // Theme-park approach boards on I-4 -- tourism OOH, no Disney slogans.
    ("I-4", &[
        approaching("Orlando ahead. The theme parks are off this road and not for freight. Fuel up, look once, keep going.", &["orlando_fl_us"]),
    ]),
    // Space Center Houston tourism on I-45.
    ("I-45", &[
        approaching("Houston ahead. Space Center Houston is a real room they flew from. The trailer stays on the planet.", &["houston_tx_us"]),
        // Gilley's, the Urban Cowboy honky-tonk, stood in Pasadena outside
        // Houston until it closed in nineteen eighty-nine and burned the year
        // after; the bull El Toro went to Gilley's Dallas. Past tense on
        // purpose: a board promising the Pasadena room would be a lie.
        approaching("Houston ahead. Gilley's, the Urban Cowboy honky-tonk, stood in Pasadena until the eighties. The bull moved to Dallas. The boots stayed.", &["houston_tx_us"]),
        // Gilley's Dallas, open since two thousand three with the original
        // bull from the film; an event hall now, not a nightly honky-tonk.
        approaching("Dallas ahead. Gilley's Dallas keeps the original bull from Urban Cowboy. Eight seconds is the ride. The dock appointment is not.", &["dallas_tx_us"]),
        // The Longhorn Ballroom, Dallas: Bob Wills opened it in nineteen
        // fifty, the Sex Pistols played it in nineteen seventy-eight, and it
        // reopened restored in twenty twenty-three.
        approaching("Dallas ahead. The Longhorn Ballroom, built for Bob Wills, is open again. Freight to the dock, not the dance floor.", &["dallas_tx_us"]),
    ]),
];

// Big Buck's approach signs -- the parody Texas mega-stop runs its own billboard
// campaign, the one every driver passes for a couple hundred miles before the
// exit. Real Buc-ee's hypes its spotless restrooms across half a state ("only
// two hundred sixty-two miles, you can hold it") and puns off its beaver mascot;
// this is original copy in that register. These fire as you NEAR a Big Buck's
// landmark, not from the corridor-agnostic pool, so the beaver only turns up
// where the beaver actually is. Pairs with the big_bucks brand and gate content.
pub const BIG_BUCKS_BILLBOARDS: &[&str] = &[
    // The "you can hold it" bladder-buster core -- the whole genre in one line.
    "Big Buck's. Two hundred sixty-two miles. You can hold it.",
    "Potty like a rockstar. One hundred ninety miles to go, superstar.",
    "Restrooms so clean your mother would approve. She'd also ask why you never call.",
    "Hold it. Just hold it. We believe in you. Ninety more miles.",
    "The cleanest restrooms in America, and you'll spend the next four hundred miles thinking about them.",
    // Home of the Bladder Buster -- the bucket of soda that undoes all of the above.
    "Home of the Bladder Buster. Sixty-four ounces. You come out a changed driver.",
    // Beaver-mascot puns -- their bread and butter.
    "Hello. Is it the beaver you're looking for?",
    "That billboard was printed upside down on purpose. Made you look. Big Buck's, next exit.",
    "A beaver the size of a refrigerator is waving at you. Ninety miles. Wave back.",
    // The food wall.
    "Fresh brisket, a wall of jerky, and Beaver Bites. Ninety miles. Try to think about anything else.",
    "Fudge made fresh this morning. Beaver Bites made fresh this morning. Your diet, made yesterday.",
    // The trucker irony -- sparing, because this is a trucking sim and it stings.
    "Big Buck's ahead. Acres of gleaming fuel islands, and not one of them for you. Drop the trailer and dream.",
    "Big Buck's. The beaver has a restroom. You have a bladder. Race is on. One hundred twenty miles.",
    "Kolaches at Big Buck's. You do not know what that is. You will.",
    "The beaver says the coffee is fresh. The beaver says a lot of things. Eighty miles.",
    "Big Buck's jerky wall: taller than your trailer is long. One hundred miles. Stretch.",
    "Cleanest floors in the state. You will take your boots off in the parking lot. You will not.",
    "Beaver nuggets, beaver soda, beaver everything. Your cardiologist has left the group chat.",
    "One hundred forty miles to a bathroom you will photograph. Do not photograph it.",
    "Big Buck's: so many snacks the cart needs a CDL. Next few exits, then a few more.",
    "The beaver is judging your fuel choice. Diesel is not on the menu. Drop the trailer.",
    "Big Buck's gift wall. You will buy a shirt. You will wear it ironically. You will mean it.",
    "Hold it like you mean it. Forty more miles. The beaver believes in you, mostly.",
    "They have a car wash for cars. They have a dream for you. Big Buck's, keep holding.",
];

// Song tributes drawn from the ANYWHERE pool, on any road in the country. The
// bar is therefore absolute: a line belongs here only if its whole point is
// being from everywhere at once, so that it stays true whichever mile marker
// the truck happens to be passing. Same rules as every tribute -- artist names
// and song titles only, never a lyric.
//
// Two lines were evicted from here on 2026-08-24 for failing that bar. Redwood
// country and the Appalachian hollers each said "off this road", which is a
// claim about where the truck IS, and a national draw was making that claim in
// the wrong half of the country. They now sit in the Interstate 5 and
// Interstate 77 pools behind state anchors. NOTHING THAT NAMES A PLACE GOES IN
// HERE.
pub const SONG_TRIBUTE_BILLBOARDS: &[&str] = &[
    "Hank Snow claimed he'd been everywhere. This mile marker confirms it.",
    "The girl on the billboard? Wrong billboard. Del Reeves saw her a few hundred miles back. Eyes on the road.",
];

// The whole weighting mechanism for the tribute pool: the corridor-agnostic
// picker rolls once per sign and takes a tribute at this rate, otherwise
// drawing from the everyday pools. At one draw in ten, with signs every
// thirty-five to sixty-five miles, a coast-to-coast run passes a small
// handful of tributes -- the roadside stays attorneys, fireworks, and pie.
pub const TRIBUTE_DRAW_CHANCE: f64 = 0.1;

/// The interstate a shield names: "I-90", "I 90", "i-90" and "Interstate 90"
/// all give `Some(90)`.
///
/// Anything that is not an interstate gives `None`, and that is the point.
/// The old lookup took the first run of digits and threw the prefix away, so
/// "US-90", "AZ-90" and "SR-90" all keyed to Interstate 90 -- which put South
/// Dakota's Wall Drug signs on US-90 in Louisiana, Michigan's Ionia County
/// sign on K-96 in Kansas, and the Mexican border-radio sign on MS-8 in
/// Mississippi. A shield number is only meaningful together with its prefix.
fn interstate_number(highway: &str) -> Option<u32> {
    let trimmed = highway.trim();
    let split = trimmed.find(|c: char| c.is_ascii_digit())?;
    let (prefix, digits) = trimmed.split_at(split);
    let prefix: String = prefix
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .flat_map(|c| c.to_lowercase())
        .collect();
    if prefix != "i" && prefix != "interstate" {
        return None;
    }
    let number: String = digits.chars().take_while(|c| c.is_ascii_digit()).collect();
    // Trailing text after the number (a business loop, a concurrency) is a
    // different road from the mainline the copy was written for.
    if digits.len() != number.len() {
        return None;
    }
    number.parse().ok()
}

/// Every corridor-agnostic sign, for the random picker.
static ROADSIDE: Lazy<Vec<&'static str>> = Lazy::new(|| {
    [
        GENERIC_BILLBOARDS,
        ADULT_BILLBOARDS,
        ATTORNEY_BILLBOARDS,
        FAITH_BILLBOARDS,
        ROADSIDE_ODDITIES,
        TRUCKER_SERVICES_BILLBOARDS,
        REGIONAL_RADIO_BILLBOARDS,
        TRAVEL_PLAZA_BILLBOARDS,
        POP_CULTURE_BILLBOARDS,
        META_BILLBOARDS,
    ]
    .into_iter()
    .flatten()
    .copied()
    .collect()
});

/// Every corridor-agnostic sign (the everyday pools, no tributes).
pub fn roadside_billboards() -> &'static [&'static str] {
    &ROADSIDE
}

/// Pick a corridor-agnostic billboard, deterministically for a seeded RNG.
///
/// Rarely (`TRIBUTE_DRAW_CHANCE`) the pick comes from the low-weight song
/// tribute pool instead of the everyday pools.
#[allow(clippy::explicit_auto_deref)] // the deref picks `T = &str`; clippy's hint fails to infer
pub fn random_billboard(rng: &mut PyRandom) -> &'static str {
    if rng.random() < TRIBUTE_DRAW_CHANCE {
        return *rng.choice(SONG_TRIBUTE_BILLBOARDS);
    }
    *rng.choice(&ROADSIDE)
}

/// Signs specific to an interstate's real roadside culture, each with the
/// geography its copy is true in, or `&[]` if the road has no pool.
///
/// The lookup normalizes shield format ("I-90", "I 90", "Interstate 90") but
/// not shield TYPE: a US or state route sharing the number gets nothing.
pub fn corridor_signs(highway: &str) -> &'static [CorridorSign] {
    let Some(number) = interstate_number(highway) else {
        return &[];
    };
    CORRIDOR_BILLBOARDS
        .iter()
        .find(|(shield, _)| interstate_number(shield) == Some(number))
        .map(|(_, pool)| *pool)
        .unwrap_or(&[])
}

/// Real roadside attractions from the oddities genre, each true only in
/// the states where a driver could actually turn off for it. Sources: the
/// Cawker City, Kansas ball (off Interstate 70 on US-24) and Darwin,
/// Minnesota's one-man ball (US-12); the Collinsville, Illinois ketchup bottle water tower
/// (Interstates 55 and 70 east of Saint Louis); and the Petrified Forest
/// gift shops on Interstate 40 through Holbrook, Arizona, with the trade
/// running into New Mexico.
pub const PLACED_ODDITIES: &[CorridorSign] = &[
    in_states(
        "World's largest ball of twine. Bigger than your problems. Probably.",
        &["KS", "MN"],
    ),
    in_states(
        "World's largest ketchup bottle. French fries not included.",
        &["IL", "MO"],
    ),
    in_states(
        "Petrified wood, petrified staff, very lively gift shop.",
        &["AZ", "NM"],
    ),
];

/// State-gated genre signs that are true on any interstate inside those
/// states -- fireworks barns, pecan stands, and named regional travel plazas.
/// The placer concatenates this with `corridor_signs` so a fireworks barn can
/// speak on I-26 in South Carolina even though that shield has no authored
/// corridor pool.
static REGIONAL_GENRE: Lazy<Vec<CorridorSign>> = Lazy::new(|| {
    let mut out = Vec::new();
    for text in FIREWORKS_BILLBOARDS {
        out.push(CorridorSign {
            text,
            anchor: SignAnchor::States(FIREWORKS_STAND_STATES),
        });
    }
    for text in PECAN_BILLBOARDS {
        out.push(CorridorSign {
            text,
            anchor: SignAnchor::States(PECAN_STATES),
        });
    }
    out.push(in_states(SHEETZ_BILLBOARD, SHEETZ_STATES));
    out.push(in_states(WAWA_BILLBOARD, WAWA_STATES));
    out.push(in_states(RACETRAC_BILLBOARD, RACETRAC_STATES));
    out.push(in_states(CRACKER_BARREL_BILLBOARD, CRACKER_BARREL_STATES));
    out.extend(PLACED_ODDITIES.iter().copied());
    out
});

pub fn regional_genre_signs() -> &'static [CorridorSign] {
    &REGIONAL_GENRE
}

/// Just the copy from `corridor_signs`, with the anchors dropped -- for
/// content checks over the catalog. THE PLACER MUST NOT USE THIS: it is the
/// unfiltered pool, and reading from it anywhere on the shield is the bug
/// this module exists to prevent.
pub fn corridor_billboards(highway: &str) -> Vec<&'static str> {
    corridor_signs(highway).iter().map(|s| s.text).collect()
}

/// Every Big Buck's approach sign, for the near-a-Big-Buck's picker.
pub fn big_bucks_billboards() -> &'static [&'static str] {
    BIG_BUCKS_BILLBOARDS
}

#[cfg(test)]
mod tests {
    //! Port of `tests/test_billboards.py`.
    use super::*;

    fn pools() -> Vec<&'static [&'static str]> {
        vec![
            GENERIC_BILLBOARDS,
            FIREWORKS_BILLBOARDS,
            PECAN_BILLBOARDS,
            ADULT_BILLBOARDS,
            ATTORNEY_BILLBOARDS,
            FAITH_BILLBOARDS,
            ROADSIDE_ODDITIES,
            TRUCKER_SERVICES_BILLBOARDS,
            REGIONAL_RADIO_BILLBOARDS,
            TRAVEL_PLAZA_BILLBOARDS,
            POP_CULTURE_BILLBOARDS,
            META_BILLBOARDS,
            SONG_TRIBUTE_BILLBOARDS,
            BIG_BUCKS_BILLBOARDS,
        ]
    }

    // 2026-08-12 owner batch: final tester-round copy the owner asked to ship
    // verbatim, numerals included -- see the matching note in the module
    // docs. This is the only sanctioned way a line may contain a digit;
    // anything not on this exact list must stay spelled out in words.
    const DIGIT_EXEMPT_LINES: &[&str] = &[
        "The Last Chance Cafe: 37 miles back was The First Chance Cafe.",
        "World's 3rd-best pie: we used to be #2. Then Jerry complained.",
        "Truckers welcome! Trucks less welcome. Our parking lot only has 6 spaces.",
        "Need diesel? Of course you do. $4.89/gallon. Please cry inside.",
        "Bubba's Fireworks: open 362 days a year! Closed on the other three because we're usually in the hospital.",
    ];

    fn all_lines() -> Vec<&'static str> {
        let mut out: Vec<&str> = pools().into_iter().flatten().copied().collect();
        for (_, pool) in CORRIDOR_BILLBOARDS {
            out.extend(pool.iter().map(|sign| sign.text));
        }
        out.extend(regional_genre_signs().iter().map(|sign| sign.text));
        out.extend([
            SHEETZ_BILLBOARD,
            WAWA_BILLBOARD,
            RACETRAC_BILLBOARD,
            CRACKER_BARREL_BILLBOARD,
        ]);
        out
    }

    #[test]
    fn test_real_attractions_are_placed_never_anywhere() {
        // A sign naming a real thing is a claim about where the truck is.
        let real = [
            "ball of twine",
            "rocking chair",
            "ketchup bottle",
            "petrified wood",
        ];
        for line in roadside_billboards() {
            let lowered = line.to_lowercase();
            for thing in real {
                assert!(!lowered.contains(thing), "{line} is in the anywhere pool");
            }
        }
        let twine = regional_genre_signs()
            .iter()
            .find(|sign| sign.text.contains("ball of twine"))
            .expect("the twine ball is a placed sign");
        assert_eq!(twine.anchor, SignAnchor::States(&["KS", "MN"]));
        for sign in PLACED_ODDITIES {
            assert!(matches!(sign.anchor, SignAnchor::States(states) if !states.is_empty()));
        }
    }

    #[test]
    fn test_pools_are_non_empty() {
        for pool in pools() {
            assert!(!pool.is_empty());
        }
        assert!(!CORRIDOR_BILLBOARDS.is_empty());
    }

    #[test]
    fn test_lines_are_clean_spoken_text() {
        for line in all_lines() {
            assert!(line.trim() == line && !line.is_empty());
            let lowered = line.to_lowercase();
            for marker in ["amenity=", "osm", "node/", "way/", "_"] {
                assert!(!lowered.contains(marker), "{line}");
            }
            // Numbers spelled out so a screen reader never reads a bare figure,
            // except the small, explicit owner-approved allow-list above.
            if !DIGIT_EXEMPT_LINES.contains(&line) {
                assert!(!line.chars().any(|ch| ch.is_ascii_digit()), "{line}");
            }
        }
    }

    #[test]
    fn test_digit_exempt_lines_are_still_present_and_exact() {
        // Guards the allow-list itself: every exempt line must be real content,
        // not a typo that silently stopped matching (which would let a bare
        // digit slip back under the general rule's radar).
        let found = all_lines();
        for line in DIGIT_EXEMPT_LINES {
            assert!(found.contains(line), "{line}");
        }
    }

    #[test]
    fn test_corridor_lookup_normalizes_shield_format() {
        let expected = corridor_billboards("I-90");
        assert!(!expected.is_empty());
        for shield in ["I-90", "I 90", "Interstate 90", "i-90"] {
            assert_eq!(corridor_billboards(shield), expected, "{shield}");
        }
    }

    #[test]
    fn test_unknown_corridor_returns_empty() {
        assert!(corridor_billboards("I-976").is_empty());
        assert!(corridor_billboards("some county road").is_empty());
    }

    #[test]
    fn test_a_shield_number_never_matches_a_different_kind_of_road() {
        // Taking the digits and dropping the prefix put Wall Drug on US-90 in
        // Louisiana, the Ionia County sign on K-96 in Kansas, and the Mexican
        // border-radio sign on MS-8 in Mississippi. Every one of these shields
        // is a real road in the world data that shares a number with a mapped
        // interstate, and none of them may inherit that interstate's roadside.
        for shield in [
            "US-90", "AZ-90", "US-95", "AZ-95", "US-10", "US-15", "US-40", "US-80", "AZ-80",
            "KY-80", "SR-80", "US-70", "TX-70", "CA-44", "US-35", "ID-55", "NJ-55", "US-65",
            "US-59", "WY-59", "US-75", "FL-85", "US-85", "US-24", "K-96", "US-96", "US-71",
            "AZ-77", "US-77", "US-81", "US-64", "US-30", "MS-8", "US-78", "US-20",
        ] {
            assert!(
                corridor_billboards(shield).is_empty(),
                "{shield} inherited an interstate's signs"
            );
        }
        // Nor may a business loop or spur take the mainline's copy.
        assert!(corridor_billboards("I-90 Business").is_empty());
        // And the interstates themselves still resolve.
        assert!(!corridor_billboards("I-90").is_empty());
    }

    #[test]
    fn test_the_anywhere_pools_hold_nothing_that_names_a_place() {
        // The whole reason a sign turns up in the wrong state: a line naming a
        // real town, region, road or attraction sitting in a pool the picker
        // draws from anywhere in the country. Names that ARE fine here are
        // invented businesses (Big Jim, Big Bob's, Mamma's) and generic
        // Americana, because those are true beside any road.
        const PLACE_NAMES: &[&str] = &[
            "Alabama",
            "Appalachian",
            "Arizona",
            "Arkansas",
            "Atlanta",
            "Bakersfield",
            "casino",
            "Casino",
            "California",
            "Cleveland",
            "Georgia",
            "Graceland",
            "Hall of Fame",
            "Kansas",
            "Kentucky",
            "Laredo",
            "Memphis",
            "Michigan",
            "Missouri",
            "Nashville",
            "Nevada",
            "New Orleans",
            "Oklahoma",
            "Oklahoma City",
            "Oregon",
            "Preservation Hall",
            "Reno",
            "Redwood",
            "Rockies",
            "Seattle",
            "Tejano",
            "Tennessee",
            "Texas",
            "Virginia",
            "Wisconsin",
            "Wyoming",
            "Alamo",
            "Fiesta",
            "hollers",
            "Interstate",
            "Route Sixty-Six",
            "Wall Drug",
            "Albuquerque",
            "Beale Street",
            "Bristol",
            "Chicago",
            "Corn Palace",
            "Crystal Palace",
            "Florida",
            "Fort Worth",
            "Grand Ole Opry",
            "Harley-Davidson",
            "Houston",
            "Iowa Eighty",
            "Louisville",
            "McAllen",
            "Mall of America",
            "Milwaukee",
            "Minnesota",
            "Mitchell",
            "Orlando",
            "Oz Museum",
            "Ryman",
            "Saint Louis",
            "Space Center",
            "Spam Museum",
            "Wamego",
            "Gateway Arch",
            "Blueberry Hill",
            "Billy Bob",
            "Birthplace of Country Music",
            "Gilley",
            "Longhorn Ballroom",
            "Kentucky Horse Park",
            "Cajun",
            "Sheetz",
            "Wawa",
            "RaceTrac",
            "Cracker Barrel",
        ];
        let anywhere: Vec<&str> = roadside_billboards()
            .iter()
            .chain(SONG_TRIBUTE_BILLBOARDS.iter())
            .copied()
            .collect();
        for line in anywhere {
            for name in PLACE_NAMES {
                assert!(
                    !line.contains(name),
                    "the anywhere pool claims {name}: {line}"
                );
            }
        }
    }

    #[test]
    fn test_every_corridor_sign_carries_the_geography_its_copy_claims() {
        // An anchor is only meaningful if it is filled in: an empty state list
        // or an empty city list can never match, and a zero window silently
        // retires the sign. Corridor is reserved for lines that name no place,
        // so those are listed explicitly rather than reachable by default.
        const NAMES_NO_PLACE: &[&str] = &["Have you seen the scenery"];
        let mut signs: Vec<(&str, &CorridorSign)> = Vec::new();
        for (shield, pool) in CORRIDOR_BILLBOARDS {
            for sign in pool.iter() {
                signs.push((shield, sign));
            }
        }
        for sign in regional_genre_signs() {
            signs.push(("regional", sign));
        }
        for (shield, sign) in signs {
            match sign.anchor {
                SignAnchor::Corridor => assert!(
                    NAMES_NO_PLACE.iter().any(|k| sign.text.contains(k)),
                    "{shield}: unanchored line names a place: {}",
                    sign.text
                ),
                SignAnchor::States(states) => {
                    assert!(!states.is_empty(), "{shield}: {}", sign.text);
                    for state in states {
                        assert_eq!(state.len(), 2, "{shield}: {state}");
                        assert!(state.chars().all(|c| c.is_ascii_uppercase()));
                    }
                }
                SignAnchor::Approaching { cities, within_mi } => {
                    assert!(!cities.is_empty(), "{shield}: {}", sign.text);
                    for city in cities {
                        // A world key, not a spoken name -- the placer
                        // matches it against the route's own city list.
                        assert!(city.ends_with("_us"), "{shield}: {city}");
                    }
                    // The window has to outrun the sign spacing, or an
                    // anchored line gets at most one chance per trip.
                    assert!(within_mi > 65.0, "{shield}: {within_mi}");
                }
            }
        }
    }

    #[test]
    fn test_big_bucks_billboards_accessor_returns_pool() {
        assert_eq!(big_bucks_billboards(), BIG_BUCKS_BILLBOARDS);
        assert!(!BIG_BUCKS_BILLBOARDS.is_empty());
    }

    #[test]
    fn test_random_billboard_is_deterministic_and_in_pool() {
        let pick = random_billboard(&mut PyRandom::new_from_i64(3));
        let mut everything: Vec<&str> = roadside_billboards().to_vec();
        everything.extend(SONG_TRIBUTE_BILLBOARDS.iter().copied());
        assert!(everything.contains(&pick));
        assert_eq!(random_billboard(&mut PyRandom::new_from_i64(3)), pick);
    }

    #[test]
    fn test_song_tribute_corridors_resolve_by_shield() {
        // The tribute batch added corridor keys beyond the original attractions
        // set; the number-normalized lookup must serve them like any other.
        let fctc = corridor_billboards("I-44");
        assert!(fctc
            .iter()
            .any(|line| line.contains("Franklin County Trucking Company")));
        assert_eq!(corridor_billboards("Interstate 44"), fctc);
        for shield in [
            "I-35", "I-5", "I-55", "I-65", "I-59", "I-75", "I-85", "I-24", "I-94", "I-77", "I-81",
            "I-64", "I-30", "I-78", "I-20", "I-4", "I-45",
        ] {
            assert!(!corridor_billboards(shield).is_empty(), "{shield}");
        }
        // I-96, I-71, and I-8 used to carry one radio-memory tribute each.
        // Those lines were pulled; the shields have no honest paid-board
        // replacement in this commit, so an empty pool is the honest result.
        for shield in ["I-96", "I-71", "I-8"] {
            assert!(
                corridor_billboards(shield).is_empty(),
                "{shield} still has a pulled radio-memory line"
            );
        }
    }

    #[test]
    fn test_wall_drug_stays_on_south_dakota_wyoming_and_minnesota() {
        let signs: Vec<_> = corridor_signs("I-90")
            .iter()
            .filter(|s| s.text.contains("Wall Drug"))
            .collect();
        assert_eq!(
            signs.len(),
            1,
            "the direction-free Wall Drug line must remain"
        );
        for sign in signs {
            assert_eq!(
                sign.anchor,
                SignAnchor::States(&["SD", "WY", "MN"]),
                "{}",
                sign.text
            );
        }
    }

    #[test]
    fn test_meramec_moved_to_interstate_44_missouri() {
        assert!(
            corridor_signs("I-40")
                .iter()
                .all(|s| !s.text.contains("caverns ahead")),
            "Meramec is not on Interstate 40"
        );
        let signs: Vec<_> = corridor_signs("I-44")
            .iter()
            .filter(|s| s.text.contains("caverns ahead"))
            .collect();
        assert_eq!(signs.len(), 1);
        assert_eq!(signs[0].anchor, SignAnchor::States(&["MO"]));
        assert!(signs[0].text.contains("Meramec-style caverns"));
    }

    #[test]
    fn test_dock_of_the_bay_stays_california_not_nevada() {
        let signs: Vec<_> = corridor_signs("I-80")
            .iter()
            .filter(|s| s.text.contains("Dock of the Bay"))
            .collect();
        assert_eq!(signs.len(), 1);
        assert_eq!(signs[0].anchor, SignAnchor::States(&["CA"]));
    }

    #[test]
    fn test_new_genre_and_landmark_lines_stay_on_their_approaches() {
        let expected: &[(&str, &str, &str)] = &[
            ("I-15", "Another casino", "las_vegas_nv_us"),
            ("I-80", "Reno ahead", "reno_nv_us"),
            ("I-40", "Casino billboards", "oklahoma_city_ok_us"),
            ("I-35", "Native casino", "oklahoma_city_ok_us"),
            ("I-44", "Casino lights", "tulsa_ok_us"),
            ("I-35", "Laredo ahead", "laredo_tx_us"),
            ("I-35", "Tejano weekend", "san_antonio_tx_us"),
            ("I-10", "Fiesta in the spring", "san_antonio_tx_us"),
            ("I-90", "Rock and Roll Hall of Fame", "cleveland_oh_us"),
            ("I-40", "Graceland", "memphis_tn_us"),
            ("I-10", "Preservation Hall", "new_orleans_la_us"),
            ("I-5", "museum of pop culture", "seattle_wa_us"),
        ];
        for (shield, phrase, city) in expected {
            let signs: Vec<_> = corridor_signs(shield)
                .iter()
                .filter(|sign| sign.text.contains(phrase))
                .collect();
            assert_eq!(signs.len(), 1, "{shield}: {phrase}");
            match signs[0].anchor {
                SignAnchor::Approaching { cities, within_mi } => {
                    assert_eq!(cities, &[*city], "{shield}: {phrase}");
                    assert_eq!(within_mi, SIGN_APPROACH_MI, "{shield}: {phrase}");
                }
                anchor => panic!("{shield}: {phrase} has {anchor:?}, not an approach anchor"),
            }
            assert!(!roadside_billboards()
                .iter()
                .any(|line| line.contains(phrase)));
            assert!(!SONG_TRIBUTE_BILLBOARDS
                .iter()
                .any(|line| line.contains(phrase)));
        }

        let reno = corridor_signs("I-80")
            .iter()
            .find(|sign| sign.text.contains("Reno ahead"))
            .unwrap();
        assert_ne!(reno.anchor, SignAnchor::States(&["NV"]));
        for shield in ["I-20", "I-40", "I-45", "I-37"] {
            assert!(
                corridor_signs(shield)
                    .iter()
                    .all(|sign| !sign.text.contains("Tejano")),
                "{shield} has a Tejano line"
            );
        }
    }

    #[test]
    fn test_pulled_radio_memory_and_jeep_trail_lines_are_gone() {
        // Radio-memory tributes and a Jeep trail are not paid boards. They
        // come out of the catalog entirely -- no rewrite, no clone onto a
        // new road.
        const PULLED: &[&str] = &[
            "All his exes live around here somewhere",
            "Amarillo can wait till morning",
            "Black Bear Road",
            "Mexican Radio",
            "Haynesville Woods",
            "Ionia County, Michigan",
            "Arlo McKinley",
            "Russell County line",
            "Forty-Nine Winchester",
        ];
        for line in all_lines() {
            for phrase in PULLED {
                assert!(!line.contains(phrase), "pulled line still live: {line}");
            }
        }
    }

    #[test]
    fn test_bubbas_fireworks_stays_in_the_carolinas() {
        let signs: Vec<_> = corridor_signs("I-95")
            .iter()
            .filter(|s| s.text.contains("Bubba's Fireworks"))
            .collect();
        assert_eq!(signs.len(), 1);
        assert_eq!(signs[0].anchor, SignAnchor::States(&["SC", "NC"]));
    }

    #[test]
    fn test_big_bucks_pool_is_not_in_the_anywhere_roadside() {
        for line in BIG_BUCKS_BILLBOARDS {
            assert!(
                !roadside_billboards().contains(line),
                "Big Buck's leaked into anywhere: {line}"
            );
        }
    }

    #[test]
    fn test_new_paying_attractions_stay_anchored() {
        let expected: &[(&str, &str)] = &[
            ("I-90", "Corn Palace"),
            ("I-90", "Spam Museum"),
            ("I-80", "Iowa Eighty"),
            ("I-5", "Bakersfield Sound still plays"),
            ("I-81", "Birthplace of Country Music"),
            ("I-75", "Georgia peaches"),
            ("I-4", "theme parks are off this road"),
            ("I-45", "Space Center Houston"),
            ("I-35", "Billy Bob's Texas"),
            ("I-40", "Beale Street"),
            ("I-40", "Grand Ole Opry"),
            ("I-65", "Ryman Auditorium"),
        ];
        for (shield, phrase) in expected {
            let signs: Vec<_> = corridor_signs(shield)
                .iter()
                .filter(|s| s.text.contains(phrase))
                .collect();
            assert_eq!(signs.len(), 1, "{shield}: {phrase}");
            assert!(!matches!(signs[0].anchor, SignAnchor::Corridor));
        }
        assert!(!all_lines().iter().any(|l| l.contains("Rock City")));
        assert!(!all_lines().iter().any(|l| l.contains("Music Highway")));
        assert!(
            !all_lines().iter().any(|l| l.contains("McAllen ahead")),
            "I-35 McAllen board must stay killed"
        );
        assert!(
            !all_lines().iter().any(|l| l.contains("Lumper service")),
            "lumper line must stay dropped"
        );
    }

    #[test]
    fn test_ruth_cut_moves_fireworks_pecan_and_named_plazas_off_anywhere() {
        for line in FIREWORKS_BILLBOARDS
            .iter()
            .chain(PECAN_BILLBOARDS)
            .copied()
            .chain([
                SHEETZ_BILLBOARD,
                WAWA_BILLBOARD,
                RACETRAC_BILLBOARD,
                CRACKER_BARREL_BILLBOARD,
            ])
        {
            assert!(
                !roadside_billboards().contains(&line),
                "still in Anywhere roadside: {line}"
            );
        }
        // Love's / Pilot / generic plaza stay national.
        assert!(roadside_billboards()
            .iter()
            .any(|l| l.contains("Love's ahead")));
        assert!(roadside_billboards()
            .iter()
            .any(|l| l.contains("Pilot next exit")));
        assert!(roadside_billboards()
            .iter()
            .any(|l| l.contains("Travel center:")));

        let regional = regional_genre_signs();
        for line in FIREWORKS_BILLBOARDS {
            let sign = regional.iter().find(|s| s.text == *line).expect(line);
            assert_eq!(
                sign.anchor,
                SignAnchor::States(FIREWORKS_STAND_STATES),
                "{line}"
            );
        }
        for line in PECAN_BILLBOARDS {
            let sign = regional.iter().find(|s| s.text == *line).expect(line);
            assert_eq!(sign.anchor, SignAnchor::States(PECAN_STATES), "{line}");
        }
        let named = [
            (SHEETZ_BILLBOARD, SHEETZ_STATES),
            (WAWA_BILLBOARD, WAWA_STATES),
            (RACETRAC_BILLBOARD, RACETRAC_STATES),
            (CRACKER_BARREL_BILLBOARD, CRACKER_BARREL_STATES),
        ];
        for (line, states) in named {
            let sign = regional.iter().find(|s| s.text == line).expect(line);
            assert_eq!(sign.anchor, SignAnchor::States(states), "{line}");
        }
        assert_eq!(ADULT_BILLBOARDS.len(), 22);
        assert!(roadside_billboards()
            .iter()
            .any(|l| l.contains("Adult superstore")));
        // Always-on adult pool: no settings/toggle copy leaked into the catalog.
        for line in all_lines() {
            let lower = line.to_lowercase();
            assert!(!lower.contains("content filter"));
            assert!(!lower.contains("adult billboards off"));
        }
    }

    #[test]
    fn test_song_tributes_stay_rare_in_the_random_draw() {
        // The tribute pool is a low-weight fold-in: roughly TRIBUTE_DRAW_CHANCE
        // of corridor-agnostic picks, so the roadside never becomes a museum.
        let mut rng = PyRandom::new_from_i64(7);
        let draws = 4000;
        let hits = (0..draws)
            .filter(|_| SONG_TRIBUTE_BILLBOARDS.contains(&random_billboard(&mut rng)))
            .count();
        let fraction = hits as f64 / draws as f64;
        assert!(0.05 < fraction && fraction < 0.2, "{fraction}");
    }
}
