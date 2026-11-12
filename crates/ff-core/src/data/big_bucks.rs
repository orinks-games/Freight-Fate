//! Big Buck's -- roadside-landmark mini-game content, spoken line pools
//! (port of `freight_fate/data/big_bucks.py`).
//!
//! Big Buck's is an original parody of the famous Texas travel-center chain
//! that bans big rigs. This is the CONTENT layer: the spoken lines the
//! landmark interaction draws from, authored as data so it stays the
//! map/content author's lane and unit-tests with no audio. The interaction
//! STATE that consumes these pools -- the gate turn-away menu, the one-time
//! hint flag, the menace/ban escalation, the crowd cooldown -- is
//! gameplay-layer follow-on. It pairs with the amenities brand layer's
//! `Brand { key: "big_bucks", bans_big_rigs: true }`.
//!
//! Two refusal flavors, kept mechanically distinct:
//!
//! * STRUCTURAL -- your fault. You showed up trailered (loaded or deadhead,
//!   still an eighteen-wheeler): no-big-rigs signage, then a one-time teaching
//!   hint, then escalating menace lines, then a temporary ban plus a
//!   reputation ding. Comes from the truck's configuration, so it is
//!   deterministic.
//! * SITUATIONAL -- not your fault. Even bobtail and welcome, the lot can be
//!   mobbed. A soft "come back later" with NO penalty, mirroring the full-lot
//!   rest stop.
//!
//! Every string here is player-facing speech: no codes, no map tags, and
//! numbers spelled in words where a bare figure would read oddly through a
//! screen reader.

use crate::pyrandom::PyRandom;

pub const BIG_BUCKS_SOURCE: &str =
    "Original parody of a well-known Texas travel-center chain that bans big \
     rigs; names and products are invented to keep the joke and drop the mark.";

/// Forced trailered attempts *after* the first-offense hint before Big Buck's
/// bans you. The hint itself is attempt zero; the beaver's patience runs out
/// on the attempt that reaches this count.
pub const BAN_THRESHOLD: i64 = 3;

/// STRUCTURAL refusal -- spoken at the gate whenever you roll up trailered.
pub const NO_BIG_RIGS_SIGNAGE: &[&str] = &[
    "The sign over the entrance is not subtle: no eighteen-wheelers. Big Buck's turns away anything pulling a trailer.",
    "A grinning beaver on a billboard waves you off. No eighteen-wheelers, partner. Not today, not ever.",
];

/// STRUCTURAL -- said exactly once, the first time you are turned away, so a
/// new player learns the rule (and the bobtail-versus-deadhead distinction).
pub const FIRST_OFFENSE_HINT: &str =
    "You will never fit a trailer in that car lot. Drop your trailer somewhere and roll back in bobtail, just the tractor with no trailer, and the beaver will let you slip in.";

/// STRUCTURAL -- escalating flavor for repeat trailered attempts after the
/// hint. Indexed by how many times you have already been turned away since
/// the hint; the last line is the final warning before the ban.
pub const MENACE_LINES: &[&str] = &[
    "Still hauling a trailer. The greeter's smile is getting thin. You know the rule by now.",
    "That is three strikes' worth of trailer. A manager in a beaver cap is watching you idle at the entrance.",
    "Last warning: keep blocking the entrance with that rig and Big Buck's will ask you not to come back for a while.",
];

/// STRUCTURAL -- the ban lands, with a reputation ding handled by the
/// interaction.
pub const BAN_NOTICE: &str =
    "That does it. The beaver has had enough. You are banned from Big Buck's for a while, and word gets around. Your reputation takes a hit.";

/// STRUCTURAL -- arriving while the ban is still in effect.
pub const STILL_BANNED: &str =
    "The greeter recognizes your rig from the entrance camera and shakes his head. Still banned. Come back when things have cooled off.";

/// SITUATIONAL refusal -- you are bobtail and welcome, but the place is a
/// zoo. No penalty, no rep hit; just try again later. Riff pool -- add freely.
pub const CROWD_REFUSALS: &[&str] = &[
    "Five buses of middle-school thrill-seekers are raiding the fudge counter. Maybe give it a pass for now.",
    "A tour bus just unloaded forty road-trippers into the brisket line. Give it an hour.",
    "It is a summer Saturday and the lot is packed to the beaver's whiskers. Come back when it is calmer.",
    "Three motor homes, a car club, and a wedding party in the parking lot. No room even for a bobtail today.",
    "They are restocking the jerky wall and half the pumps are coned off. Swing back later.",
    "The famous restrooms have a line out the door. Trust us, come back.",
];

/// The reward: you dropped your trailer and slipped in bobtail. Welcome
/// flavor.
pub const ARRIVAL_GREETING: &[&str] = &[
    "Welcome to Big Buck's. Acres of gleaming fuel islands you are not allowed to use, the cleanest restrooms in three counties, and a wall of brisket that can be smelled from the interstate.",
    "You ease the bobtail tractor between two minivans and a beaver the size of a refrigerator waves hello. You made it in.",
];

/// Browsable catalog once you are inside -- a money sink and, later, the buff
/// menu. Content only here; the buff effects are gameplay-layer.
///
/// The comfort-stop buff pair -- names live here, effects are gameplay-layer.
/// The Bladder Buster caffeinates you but fills the tank, nudging you toward
/// an optional (non-HOS) comfort stop; the Iron Bladder is the premium item
/// that skips that stop outright. Both move fatigue and optional minutes only,
/// never the legal Hours-of-Service clock.
pub const MENU: &[&str] = &[
    "a brisket sandwich the size of a hubcap",
    "Beaver Bites, glazed corn-nut things that are dangerously good",
    "a slab of homemade fudge from the fudge counter",
    "a foot of house-smoked jerky off the jerky wall",
    "the Bladder Buster, a sixty-four-ounce soda cup you could bathe a puppy in",
    "the Iron Bladder, premium road briefs for the driver who refuses to stop, priced like they know you have no choice",
    "a souvenir tee that says I Survived Big Buck's",
];

/// Pick a situational (not-your-fault) refusal line, deterministically.
#[allow(clippy::explicit_auto_deref)] // the deref picks `T = &str`; clippy's hint fails to infer
pub fn crowd_refusal(rng: &mut PyRandom) -> &'static str {
    *rng.choice(CROWD_REFUSALS)
}

/// Pick a no-big-rigs gate line for a trailered arrival, deterministically.
#[allow(clippy::explicit_auto_deref)]
pub fn signage(rng: &mut PyRandom) -> &'static str {
    *rng.choice(NO_BIG_RIGS_SIGNAGE)
}

/// Pick a welcome line for a successful bobtail arrival, deterministically.
#[allow(clippy::explicit_auto_deref)]
pub fn arrival_greeting(rng: &mut PyRandom) -> &'static str {
    *rng.choice(ARRIVAL_GREETING)
}

/// Escalating line for a repeat trailered attempt after the first hint.
///
/// `prior_offenses` is how many times you have already been turned away since
/// the hint (so the first menace line is `prior_offenses == 1`). Clamped to
/// the final warning past the end of the ladder.
pub fn menace_line(prior_offenses: i64) -> &'static str {
    let index = (prior_offenses.max(1) - 1).min(MENACE_LINES.len() as i64 - 1);
    MENACE_LINES[index as usize]
}

/// Whether a trailered attempt should now trigger the temporary ban.
pub fn is_ban_earned(prior_offenses: i64) -> bool {
    prior_offenses >= BAN_THRESHOLD
}

#[cfg(test)]
mod tests {
    //! Port of `tests/test_big_bucks.py`.
    use super::*;

    fn all_pools() -> Vec<&'static [&'static str]> {
        vec![
            NO_BIG_RIGS_SIGNAGE,
            &[FIRST_OFFENSE_HINT],
            MENACE_LINES,
            &[BAN_NOTICE],
            &[STILL_BANNED],
            CROWD_REFUSALS,
            ARRIVAL_GREETING,
            MENU,
        ]
    }

    fn all_lines() -> Vec<&'static str> {
        all_pools().into_iter().flatten().copied().collect()
    }

    #[test]
    fn test_pools_are_non_empty() {
        for pool in all_pools() {
            assert!(!pool.is_empty(), "{pool:?}");
        }
    }

    #[test]
    fn test_lines_are_clean_spoken_text() {
        for line in all_lines() {
            assert!(line.trim() == line && !line.is_empty());
            // No raw map tags, codes, or stray markers reach speech.
            let lowered = line.to_lowercase();
            for marker in ["amenity=", "osm", "node/", "way/", "_"] {
                assert!(!lowered.contains(marker), "{line}");
            }
            // Numbers are spelled out, so a screen reader never reads a bare figure.
            assert!(!line.chars().any(|ch| ch.is_ascii_digit()), "{line}");
        }
    }

    #[test]
    fn test_menace_ladder_escalates_and_clamps() {
        assert_eq!(menace_line(1), MENACE_LINES[0]);
        assert_eq!(menace_line(2), MENACE_LINES[1]);
        // Past the end of the ladder, the final warning repeats.
        assert_eq!(menace_line(99), MENACE_LINES[MENACE_LINES.len() - 1]);
        // Defensive: a zero/negative count still returns the first line.
        assert_eq!(menace_line(0), MENACE_LINES[0]);
    }

    #[test]
    fn test_ban_is_earned_at_threshold() {
        assert!(!is_ban_earned(BAN_THRESHOLD - 1));
        assert!(is_ban_earned(BAN_THRESHOLD));
        assert!(is_ban_earned(BAN_THRESHOLD + 5));
    }

    #[test]
    fn test_pickers_are_deterministic_and_in_pool() {
        type Picker = fn(&mut PyRandom) -> &'static str;
        let cases: [(Picker, &[&str]); 3] = [
            (crowd_refusal, CROWD_REFUSALS),
            (signage, NO_BIG_RIGS_SIGNAGE),
            (arrival_greeting, ARRIVAL_GREETING),
        ];
        for (picker, pool) in cases {
            let first = picker(&mut PyRandom::new_from_i64(7));
            assert!(pool.contains(&first));
            // Same seed reproduces the choice -- offline-deterministic.
            assert_eq!(picker(&mut PyRandom::new_from_i64(7)), first);
        }
    }

    #[test]
    fn test_crowd_refusal_has_no_reputation_language() {
        // Situational refusals are not the player's fault, so they must not threaten
        // a penalty the way the structural ban notice does.
        for line in CROWD_REFUSALS {
            let lowered = line.to_lowercase();
            assert!(!lowered.contains("banned"));
            assert!(!lowered.contains("reputation"));
        }
    }
}
