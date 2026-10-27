//! Spoken message pairs: one definition, both renderings side by side.
//!
//! The terse contract (docs/speech-priority-research.md, R4) promises that
//! terse mode tells the player what to *do* and what it *cost*, and nothing
//! else -- in the shortest form the ontology allows. Making that real used
//! to depend on every call site remembering to hand-branch on the verbosity
//! setting, which is how 79 branches came to cover 711 speech call sites,
//! and how the terse hazard call drifted onto a synonym nobody diffed
//! against the help text.
//!
//! So the pair lives in ONE definition: a builder in this module renders the
//! normal and terse forms of a message side by side, where a reviewer sees
//! both, and the delivery layer (`GameContext::say` / `say_event`) picks the
//! rendering the player's speech mode asks for. Drift between the two forms
//! is structurally impossible, and the safety-critical pairs are pinned by
//! copy tests on top of that.
//!
//! Two rules bound every terse rendering (the research doc's R4):
//!
//! - **Compress words, never certainty.** A qualifier that changes a
//!   decision survives terse; parking certainty keeps all five values
//!   distinguishable.
//! - **A fixed slot grammar, recorded in docs/ontology.md.** Hazards speak
//!   as [thing, distance, target speed]; stops as [name, exit, distance,
//!   qualifier]. A bare trailing number is only parseable because the frame
//!   never shuffles, so no terse line may reorder its slots.
//!
//! Port of `freight_fate/speech_text.py`.

use std::fmt;

use crate::sim::trip_models::OpenSide;

/// The Python `_WORD_RE = [a-z0-9]+` over the lowercased text.
fn words(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_ascii_lowercase() && !c.is_ascii_digit())
        .filter(|word| !word.is_empty())
        .map(str::to_string)
        .collect()
}

/// Whether a facility's type prefix only repeats words already in its name.
///
/// "cross-dock Chicago Cross-Dock" and "port Port of Indiana-Burns Harbor"
/// say the type twice; dropping the prefix there removes repetition, not
/// vocabulary (research doc R6). Match on whole words so a short label does
/// not fire on a coincidental substring ("port" must not swallow "Newport").
/// A "travel center: Love's" keeps its prefix, because the name does not
/// carry the type.
pub fn type_prefix_is_redundant(label: &str, name: &str) -> bool {
    let label_words = words(label);
    if label_words.is_empty() {
        return false;
    }
    let name_words = words(name);
    label_words.iter().all(|word| name_words.contains(word))
}

/// A spoken city with a leading "the" for phrases like "the {city} service
/// area", unless the name already starts with its own article: "the
/// Chicago", but "The Dalles", never "the The Dalles".
pub fn the_city(city: &str) -> String {
    if city.starts_with("The ") {
        city.to_string()
    } else {
        format!("the {city}")
    }
}

/// A facility's name with its type prefix, unless the prefix is redundant.
pub fn typed_name(label: &str, name: &str, sep: &str) -> String {
    if type_prefix_is_redundant(label, name) {
        return name.to_string();
    }
    format!("{label}{sep}{name}")
}

/// A spoken line carrying both of its renderings.
///
/// In Python the instance WAS the normal rendering -- a `str` subclass -- so
/// everything that stores, compares, logs, or formats messages kept working
/// unchanged. Here `Display` is the normal rendering and plain text converts
/// into one via `From`, so a call site can pass a `&str` where a message is
/// accepted. The delivery layer picks the rendering the player's speech mode
/// asks for. `terse == None` means the line reads the same in both modes;
/// `terse == Some("")` means terse mode drops the line whole (an earcon or
/// silence carries it instead, and it never reaches the review log -- as far
/// as the drive is concerned it was not said).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SpokenMessage {
    pub normal: String,
    pub terse: Option<String>,
}

impl SpokenMessage {
    /// A line that reads the same in both modes.
    pub fn new(normal: impl Into<String>) -> Self {
        Self {
            normal: normal.into(),
            terse: None,
        }
    }

    /// A line with a terse form of its own.
    pub fn with_terse(normal: impl Into<String>, terse: impl Into<String>) -> Self {
        Self {
            normal: normal.into(),
            terse: Some(terse.into()),
        }
    }

    /// The Python constructor shape: `SpokenMessage(normal, terse=None)`.
    pub fn pair(normal: impl Into<String>, terse: Option<String>) -> Self {
        Self {
            normal: normal.into(),
            terse,
        }
    }

    pub fn render(&self, terse: bool) -> &str {
        match (&self.terse, terse) {
            (Some(short), true) => short,
            _ => &self.normal,
        }
    }

    /// Both renderings extended with one more sentence.
    ///
    /// Plain concatenation would flatten the pair back to a bare string and
    /// silently lose the terse form; this keeps the suffix on both. A
    /// dropped line (`terse == Some("")`) keeps only the suffix in terse
    /// mode: the base line was color, but the suffix was appended because it
    /// reports something that happened.
    pub fn plus(&self, suffix: &str) -> SpokenMessage {
        let terse = match &self.terse {
            None => None,
            Some(short) if !short.is_empty() => Some(format!("{short} {suffix}")),
            Some(_) => Some(suffix.to_string()),
        };
        SpokenMessage {
            normal: format!("{} {suffix}", self.normal),
            terse,
        }
    }
}

impl fmt::Display for SpokenMessage {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.normal)
    }
}

impl From<&str> for SpokenMessage {
    fn from(normal: &str) -> Self {
        SpokenMessage::new(normal)
    }
}

impl From<String> for SpokenMessage {
    fn from(normal: String) -> Self {
        SpokenMessage::new(normal)
    }
}

impl PartialEq<str> for SpokenMessage {
    fn eq(&self, other: &str) -> bool {
        self.normal == other
    }
}

impl PartialEq<&str> for SpokenMessage {
    fn eq(&self, other: &&str) -> bool {
        self.normal == *other
    }
}

impl AsRef<str> for SpokenMessage {
    fn as_ref(&self) -> &str {
        &self.normal
    }
}

/// Color, confirmation, or coaching: spoken in normal mode, carried by an
/// earcon or by silence in terse mode.
pub fn terse_silent(normal: impl Into<String>) -> SpokenMessage {
    SpokenMessage::with_terse(normal, "")
}

// -- hazard calls (R8) --------------------------------------------------------

/// The dodgeable hazard's call to action, and the phrase the help teaches
/// for it. One canonical phrase, never a synonym: a "swerve" rewording lived
/// only in terse mode, delivered only to the players who turned explanations
/// off -- exactly the drift docs/ontology.md exists to prevent. A copy test
/// scans src/ so the synonym cannot come back.
///
/// The lane change leads (owner, 2026-08-17). Both actions are still
/// offered, because a driver who cannot see the gap may reasonably prefer to
/// slow -- but the order is the recommendation, and at a hazard the first
/// word is the one that gets acted on. This call is only ever used where a
/// lane is genuinely open: `Trip::open_side_at` gates it, and the line ends
/// by NAMING that lane ("Left lane open.") so one tap is enough (owner,
/// 2026-09-01). A stretch with nowhere to go gets a bare "Brake!" and "No
/// lane open." instead. See [`in_lane_hazard_call`].
pub const HAZARD_DODGE_CALL: &str = "Change lanes or brake!";

// Calls the hazard warning tone already carries by itself. Terse drops them
// and keeps the body -- the thing and where it is.
//
// "Brake!" was here and is not any more (owner playtest, 2026-08-17). It
// reads like the same redundancy as "Brake now!" and is not: the emitter
// uses it ONLY where the hazard is dodgeable but no lane is open, so it is
// the one call that answers a question the driver is actively asking --
// can I go around this? Dropped, quiet left a noun phrase with no verb
// ("Brake lights right ahead.") and the owner reached for a lane change
// three times in one drive on a one-lane stretch of US-285, getting "there
// is no lane to your left here" each time. "Brake now!" stays implied: it
// marks a hazard that cannot be dodged at all, so there is no choice for
// the word to inform.
const TONE_IMPLIED_CALLS: [&str; 1] = ["Brake now!"];

/// The hazard warning: a call to action, then the thing and where.
///
/// Terse keeps the call only when it carries information the hazard tone
/// does not. A bare "Brake now!" is what the tone already said; "Brake!" is
/// the emitter's word for a thing in the lane with nowhere to go around it,
/// and survives. Either way the terse call is the normal call or nothing --
/// never a rewording. The in-lane family, which ends by naming the open
/// side, is [`in_lane_hazard_call`].
pub fn hazard_call(call: &str, body: &str) -> SpokenMessage {
    let normal = format!("{call} {body}");
    let terse = if TONE_IMPLIED_CALLS.contains(&call) {
        body.to_string()
    } else {
        normal.clone()
    };
    SpokenMessage::with_terse(normal, terse)
}

/// The warning for a thing sitting in the truck's lane: the call, the thing
/// and where, then the lane answer -- which neighbouring lane is open, or
/// that none is (owner, 2026-09-01).
///
/// "Change lanes or brake!" used to leave a blind driver guessing whether a
/// lane change was even possible, and reaching for one on a road that had
/// nowhere to go. Now the line ends with the side ("Left lane open."), in the
/// L key's own words, so one tap of that arrow is the whole answer; where
/// both neighbours are held or there is one lane this side it is "Brake!"
/// and "No lane open.", so the driver brakes without reaching for a move
/// they cannot make.
///
/// Terse drops [`HAZARD_DODGE_CALL`] and keeps the answer: "Slow car right
/// ahead. Left lane open." says everything the opener did, in fewer words.
/// "Brake!" stays in terse for the reason it always has (see
/// `TONE_IMPLIED_CALLS`): quiet must not leave a noun phrase with no verb.
pub fn in_lane_hazard_call(body: &str, side: OpenSide) -> SpokenMessage {
    in_lane_hazard_call_named(body, side, side.spoken())
}

/// [`in_lane_hazard_call`] with the lane answer already worded for the road
/// (see `OpenSide::spoken_from`).
pub fn in_lane_hazard_call_named(body: &str, side: OpenSide, answer: &str) -> SpokenMessage {
    if side.is_open() {
        SpokenMessage::with_terse(
            format!("{HAZARD_DODGE_CALL} {body} {answer}"),
            format!("{body} {answer}"),
        )
    } else {
        let normal = format!("Brake! {body} {answer}");
        SpokenMessage::with_terse(normal.clone(), normal)
    }
}

/// The same call when lane keeping on full answers it by passing: the thing
/// and where, then what the truck is doing ("Slow car right ahead. Passing
/// on the left."). No "Change lanes or brake!" -- the truck is the one
/// changing lanes -- and terse is the same line. None where no lane is open.
pub fn passing_hazard_call(body: &str, side: OpenSide) -> Option<SpokenMessage> {
    let side = if side.pass_step()? > 0 {
        "left"
    } else {
        "right"
    };
    Some(SpokenMessage::new(format!("{body} Passing on the {side}.")))
}

// -- traffic lead cues --------------------------------------------------------
// Terse slot grammar for hazard-family cues: [thing, distance, target speed].
// The trailing bare number is only parseable because the frame never
// shuffles; see the terse grammar table in docs/ontology.md.
//
// merging_traffic_cue is the one exception, at [thing, distance] -- a
// merging vehicle merges behind or passes on its own; the truck never has to
// change speed for it, so there is no target speed to append. Naming one in
// the old line ("be ready for 41 miles per hour") read as an instruction to
// slow down that nothing in the situation asked for.

pub fn merging_traffic_cue(vehicle_class: &str, gap: &str) -> SpokenMessage {
    SpokenMessage::with_terse(
        format!("Merging {vehicle_class} {gap} ahead."),
        format!("Merging {vehicle_class}, {gap}."),
    )
}

pub fn brake_lights_cue(
    gap: &str,
    speed_text: &str,
    speed_value: &str,
    cause: &str,
) -> SpokenMessage {
    // The cause rides only the full form: terse mode's compact slots stay
    // compact, and an UNKNOWN cause adds no clause at all -- brake lights
    // with nothing mile-mapped behind them are usually a wave in the
    // traffic, and inventing a reason would be worse than silence.
    let cause_clause = if cause.is_empty() {
        String::new()
    } else {
        format!(" {cause}")
    };
    SpokenMessage::with_terse(
        format!("Brake lights {gap} ahead.{cause_clause} Traffic at {speed_text}."),
        format!("Brake lights, {gap}, {speed_value}."),
    )
}

pub fn slow_lead_cue(
    vehicle_class: &str,
    gap: &str,
    speed_text: &str,
    speed_value: &str,
) -> SpokenMessage {
    SpokenMessage::with_terse(
        format!("Slow {vehicle_class} {gap} ahead, at {speed_text}."),
        format!("Slow {vehicle_class}, {gap}, {speed_value}."),
    )
}

// -- stops and parking certainty ----------------------------------------------

/// The normal labels compressed, no new words, and every value still
/// distinguishable: a qualifier a driver plans a ten-hour rest on is
/// certainty, and terse compresses words, never certainty. "Likely" is
/// spoken as silence in normal mode (a pre-existing design choice, noted for
/// the owner in the research doc); terse mirrors normal exactly, so silence
/// keeps meaning "likely" and nothing else.
pub const TERSE_PARKING_LABELS: [(&str, &str); 5] = [
    ("confirmed", "Parking confirmed."),
    ("likely", ""),
    ("limited", "Parking limited."),
    ("unknown", "Parking not verified."),
    ("none", "No truck parking."),
];

/// `TERSE_PARKING_LABELS.get(certainty)`.
pub fn terse_parking_label(certainty: &str) -> Option<&'static str> {
    TERSE_PARKING_LABELS
        .iter()
        .find(|(key, _)| *key == certainty)
        .map(|(_, label)| *label)
}

/// The keyword arguments of the Python `stop_callout`.
#[derive(Clone, Debug)]
pub struct StopCalloutParts<'a> {
    pub planned_prefix: &'a str,
    pub typed_name: &'a str,
    pub plain_name: &'a str,
    pub exit_label: &'a str,
    pub distance: &'a str,
    pub parking_normal: &'a str,
    pub parking_certainty: &'a str,
    /// Names the control that signals for the exit; "X" by default.
    pub exit_hint: &'a str,
    /// Whether the stop sells fuel: `Some(true)` says "Fuel.", `Some(false)`
    /// says "No fuel." (terse keeps only the "No fuel." verdict), `None`
    /// says nothing -- a weigh station is not a place anyone fuels.
    pub fuel: Option<bool>,
}

impl Default for StopCalloutParts<'_> {
    fn default() -> Self {
        Self {
            planned_prefix: "",
            typed_name: "",
            plain_name: "",
            exit_label: "",
            distance: "",
            parking_normal: "",
            parking_certainty: "",
            exit_hint: "X",
            fuel: None,
        }
    }
}

/// The stop-ahead callout. Terse slots: [name, exit, distance, qualifier].
///
/// Terse drops the stop-type prefix (the proper name mostly repeats it),
/// the "in"/"at" scaffolding, and the key instruction -- and keeps the
/// parking qualifier, because that is the fact the plan turns on.
///
/// `exit_hint` names the control that signals for the exit; the driving
/// layer sets it to the device-correct hint, or to "" once the player has
/// demonstrated the exit signal enough times for the instruction to retire
/// (research doc R7). An empty hint drops the sentence entirely.
pub fn stop_callout(parts: &StopCalloutParts<'_>) -> SpokenMessage {
    let StopCalloutParts {
        planned_prefix,
        typed_name,
        plain_name,
        exit_label,
        distance,
        parking_normal,
        parking_certainty,
        exit_hint,
        fuel,
    } = *parts;
    let exit_part = if exit_label.is_empty() {
        String::new()
    } else {
        format!(" at {exit_label}")
    };
    let mut normal_parts = vec![format!(
        "{planned_prefix}{typed_name}{exit_part} in {distance}."
    )];
    if !parking_normal.is_empty() {
        // Its own sentence, so it opens like one: "in 5 miles. confirmed truck
        // parking." read as a run-on (agent drive, exit 286A, 2026-09-23).
        let mut chars = parking_normal.chars();
        let first = chars.next().map(|c| c.to_uppercase().collect::<String>());
        normal_parts.push(format!("{}{}.", first.unwrap_or_default(), chars.as_str()));
    }
    // Whether there are pumps is the fact a driver exits on: a truck-parking
    // lot named "Truck Stop" sold nothing (issue #272, "The 115 Truck Stop").
    match fuel {
        Some(true) => normal_parts.push("Fuel.".to_string()),
        Some(false) => normal_parts.push("No fuel.".to_string()),
        None => {}
    }
    if !exit_hint.is_empty() {
        normal_parts.push(format!("Press {exit_hint} to signal for the exit."));
    }
    let mut slots = vec![plain_name];
    if !exit_label.is_empty() {
        slots.push(exit_label);
    }
    slots.push(distance);
    let mut terse = format!("{planned_prefix}{}.", slots.join(", "));
    let parking_terse = terse_parking_label(parking_certainty)
        .unwrap_or_else(|| terse_parking_label("unknown").expect("unknown is in the table"));
    if !parking_terse.is_empty() {
        terse = format!("{terse} {parking_terse}");
    }
    if fuel == Some(false) {
        terse = format!("{terse} No fuel.");
    }
    SpokenMessage::with_terse(normal_parts.join(" "), terse)
}

// -- money lines ---------------------------------------------------------------

/// The charged toll: what it cost always speaks (ROUTE, never dropped);
/// terse keeps the cost and who pays, drops the bookkeeping prose.
pub fn toll_charged(
    method_label: &str,
    name: &str,
    amount_text: &str,
    estimated: bool,
) -> SpokenMessage {
    let estimate = if estimated { "estimated " } else { "" };
    SpokenMessage::with_terse(
        format!(
            "{method_label} toll at {name}, {estimate}{amount_text} dollars, billed to carrier settlement."
        ),
        format!("Toll, {amount_text} dollars, carrier."),
    )
}

// -- speed and cruise ----------------------------------------------------------

/// The over-the-limit warning that rides the overspeed chime.
pub fn overspeed_nag(limit_speed_text: &str, limit_value: &str) -> SpokenMessage {
    SpokenMessage::with_terse(
        format!("Over the limit of {limit_speed_text}."),
        format!("Limit {limit_value}."),
    )
}

/// The pacenote's own short form, or the pacenote itself.
///
/// In Python `SpokenMessage` subclassed `str`, so anything that concatenated
/// or formatted one got a plain `str` back and the short form was gone
/// without a word. Every helper that wraps a curve call has to reach for the
/// short form deliberately, which is what this is for.
fn pacenote_terse(pacenote: &SpokenMessage) -> &str {
    match &pacenote.terse {
        Some(short) if !short.is_empty() => short,
        _ => &pacenote.normal,
    }
}

/// The curve call plus the assist's easing clause.
///
/// Terse speaks the pacenote alone: its advisory number is the same number
/// cruise is easing to, and the deceleration itself is audible. (A dedicated
/// cruise earcon is on the roadmap; until it exists the curve chime plus the
/// pacenote carry the moment.)
pub fn cruise_curve_easing(pacenote: &SpokenMessage, advisory_speed_text: &str) -> SpokenMessage {
    SpokenMessage::with_terse(
        format!("{pacenote} Adaptive cruise easing to {advisory_speed_text}."),
        pacenote_terse(pacenote),
    )
}

/// The curve call plus the assist taking the brakes for it, with no cruise
/// session to name: manual pedals, or the speed keeper.
///
/// One utterance, not two. The approach servo (owner ruling, 2026-09-01:
/// assists should handle curves better to avoid load shifting damage) rides
/// the pacenote's own lead, and a separate "Curve assistance slowing"
/// a breath after the call is the double the ramp fix already removed.
/// Terse speaks the pacenote alone, exactly as the cruise easing form does:
/// the advisory number is the number the assist is braking to, and the
/// deceleration itself is audible.
pub fn curve_assist_slowing(pacenote: &SpokenMessage) -> SpokenMessage {
    SpokenMessage::with_terse(
        format!("{pacenote} Curve assistance slowing."),
        pacenote_terse(pacenote),
    )
}

/// The curve call plus the handback when the bend is under cruise's floor
/// AND curve assistance is on: cruise lets go, the assist takes the
/// brakes down to the advisory, and cruise comes back past the bend.
///
/// The plain form below told the driver to expect the bend to be theirs;
/// with the approach servo it is not, and a line that hands the bend over
/// while the truck brakes itself is untrue in the way that causes the
/// mistake -- a driver who then brakes on top of it cancels the very
/// assist that was doing the work. Terse keeps both handoffs.
pub fn cruise_curve_paused_assisted(pacenote: &SpokenMessage) -> SpokenMessage {
    SpokenMessage::with_terse(
        format!(
            "{pacenote} Adaptive cruise paused for the bend, curve assistance slowing. \
             Cruise resumes past the bend."
        ),
        format!(
            "{} Cruise paused; assistance slowing.",
            pacenote_terse(pacenote)
        ),
    )
}

/// The curve call plus the handback when the bend is under cruise's floor.
///
/// A PAUSE, not a drop: the session stays armed and adaptive cruise comes
/// back on its own past the bend, once the truck is rolling off the brakes
/// at road speed (owner ruling, 2026-09-01 -- on the highway a hazard or a
/// curve pauses cruise and it comes back). The line says so, because the
/// old "Adaptive cruise off" told the driver to expect a session that had
/// in fact survived.
///
/// Was a bare `message + " Adaptive cruise off; ..."`, and that plus sign is
/// why the quiet rung still spoke full curve calls: concatenating a
/// `SpokenMessage` yields a plain `str`, so the short form was thrown away
/// before the delivery layer ever looked for it (owner playtest,
/// 2026-08-17). Terse keeps the handback -- cruise letting go of the pedal
/// is not a detail a driver can be left to infer -- and takes the
/// pacenote's short form.
pub fn cruise_curve_paused(pacenote: &SpokenMessage) -> SpokenMessage {
    SpokenMessage::with_terse(
        format!("{pacenote} Adaptive cruise paused for the bend. Resumes past it."),
        format!("{} Cruise paused.", pacenote_terse(pacenote)),
    )
}

// -- achievements --------------------------------------------------------------

/// The full achievement record: name plus flavor, for the message log, the
/// achievements menu, and a parked settlement readout. This is stored on the
/// award, not spoken live -- the live announce is [`achievement_announced`].
pub fn achievement_unlocked(name: &str, description: &str) -> SpokenMessage {
    SpokenMessage::with_terse(
        format!("New achievement! {name}. {description}"),
        format!("{name}."),
    )
}

/// The live announce: earcon plus the name, in either speech mode.
///
/// The flavor prose never speaks at speed (research doc R9); it waits in the
/// message log and the achievements menu. Normal mode hears "New
/// achievement! <name>."; terse hears the bare name, the sound having
/// already said "new".
pub fn achievement_announced(name: &str) -> SpokenMessage {
    SpokenMessage::with_terse(format!("New achievement! {name}."), format!("{name}."))
}

// -- roadside chatter ----------------------------------------------------------
// The five chatter switches decide WHAT is spoken; verbosity decides how much
// is said about it. Terse used to blanket-mute roadside chatter, which left a
// terse player five switches that were on, looked live, and did nothing
// (owner, 2026-08-15). Every enabled category now speaks in terse, in its
// short form: the name and the fact, with the framing dropped.

// Openings the baked lines use, longest first so "Crossing the" wins over
// "Crossing". What follows one of these is the name, which is the whole terse
// line: "Entering Hot Springs National Park" is "Hot Springs National Park".
const CHATTER_LEAD_INS: [&str; 9] = [
    "Museum ahead: ",
    "Billboard: ",
    "You are passing ",
    "You are crossing ",
    "Approaching ",
    "Crossing the ",
    "Crossing ",
    "Entering ",
    "Passing ",
];
// And the one that trails instead of leads ("Cullman County Museum ahead").
const CHATTER_TAIL: &str = " ahead";
// Past this, a first sentence is prose rather than a label, and its opening
// clause carries the name and the fact on its own: the heritage markers run to
// several lines of history.
const CHATTER_CLAUSE_MAX: usize = 60;

// Billboards are never cut. Every other chatter category is a LABEL plus
// framing -- "Entering Hot Springs National Park" carries the same information
// as "Hot Springs National Park", so terse loses nothing by dropping the
// frame. A billboard is not a label. It is the sign's own words, and its
// payload is usually the last sentence: "Meteor Crater is ahead, a hole in the
// desert nearly a mile wide. It is bigger than it sounds. Much bigger." cut to
// its first clause is the setup without the punchline. The function already
// spared short gags for this reason; the placed signs run long and were cut
// anyway (Brandon, 2026-08-22, asking for the billboards and the signs he
// passes to read in full at quiet).
//
// Terse has a control for these already, and it is the right one: billboards
// are the one chatter category with a dedicated on-off switch, so a player who
// finds them wordy turns them off rather than hearing half of each. Both keys
// ride `chatter_billboards` in `CHATTER_CATEGORY_FIELDS`, which
// `test_every_billboard_category_is_spared_the_cut` pins.
pub const UNCUT_CHATTER_CATEGORIES: [&str; 2] = ["billboard", "billboard_sign"];

/// The first sentence of `text`: everything before the first `[.!?]\s+`
/// that does not follow a capital letter (the Python
/// `_CHATTER_SENTENCE_END`, whose lookbehind the `regex` crate lacks).
///
/// A sentence end, but not after an initial: the museum bake carries names
/// like "Jamie L. Whitten Historical Center", and splitting at "L." would
/// speak a fragment.
fn first_sentence(text: &str) -> &str {
    let mut previous: Option<char> = None;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        if matches!(c, '.' | '!' | '?') {
            let after_initial = previous.is_some_and(|p| p.is_ascii_uppercase());
            let followed_by_space = chars.peek().is_some_and(|(_, next)| next.is_whitespace());
            if !after_initial && followed_by_space {
                return &text[..index];
            }
        }
        previous = Some(c);
    }
    text
}

/// A roadside callout with its terse short form.
///
/// Terse keeps the name and the fact and drops the framing around them. The
/// lines are baked prose rather than composed slots, so the short form is
/// cut from the text: the first sentence, its opening frame removed, and --
/// where that first sentence is prose long enough to have run past the fact
/// -- its opening clause alone.
///
/// Billboards are the exception and are never cut; see
/// `UNCUT_CHATTER_CATEGORIES` for why a sign is not a label.
///
/// Villages do not come through here. Town names answer to the
/// place-callouts ladder, not to the chatter switches (see
/// `CHATTER_CATEGORY_FIELDS`), and are already the bare "Passing X".
pub fn roadside_chatter(spoken: &str, category: &str) -> SpokenMessage {
    let normal = spoken.trim().to_string();
    if UNCUT_CHATTER_CATEGORIES.contains(&category) {
        // Whole, framing included: "Billboard:" is what tells a driver the
        // line is a sign they passed rather than the co-driver talking, and
        // a joke needs to be known as one to land.
        return SpokenMessage::new(normal);
    }
    let mut terse: &str = &normal;
    for lead in CHATTER_LEAD_INS {
        if let Some(rest) = terse.strip_prefix(lead) {
            terse = rest;
            break;
        }
    }
    // A short line already IS the fact -- a two-beat billboard gag is not
    // improved by losing its punchline. Only prose gets cut down.
    if terse.chars().count() > CHATTER_CLAUSE_MAX {
        terse = first_sentence(terse).trim();
        if terse.chars().count() > CHATTER_CLAUSE_MAX {
            if let Some((clause, _)) = terse.split_once(',') {
                terse = clause.trim();
            }
        }
    }
    let mut terse = terse.trim_end_matches(['.', '!', '?']).trim();
    if let Some(stem) = terse.strip_suffix(CHATTER_TAIL) {
        terse = stem.trim();
    }
    if terse.is_empty() {
        return SpokenMessage::with_terse(normal.clone(), normal);
    }
    let terse = format!("{terse}.");
    // Never hand back a "short" form that is not shorter; a line already at
    // its shortest reads the same in both modes.
    if terse.chars().count() < normal.chars().count() {
        SpokenMessage::with_terse(normal, terse)
    } else {
        SpokenMessage::new(normal)
    }
}

#[cfg(test)]
mod tests {
    //! Ported from `tests/test_terse_contract.py`,
    //! `tests/test_speech_verbosity_pairs.py` (the pure half),
    //! `tests/test_facility_naming.py`, and the `speech_text` cases in
    //! `tests/test_roadside_chatter.py`, `tests/test_instruction_retirement.py`
    //! and `tests/test_driving_speech_ladder.py`.
    use super::*;

    #[test]
    fn test_the_city_keeps_a_name_that_carries_its_own_article() {
        assert_eq!(the_city("The Dalles"), "The Dalles");
        assert_eq!(the_city("Chicago"), "the Chicago");
        assert_eq!(the_city("Theodore"), "the Theodore");
    }

    // -- the hazard call (R8) --------------------------------------------------

    #[test]
    fn test_the_dodge_call_names_the_open_side_and_terse_keeps_the_answer() {
        // Owner, 2026-09-01: the call ends with the side a tap can go, in the
        // L key's own words. Terse drops the opener -- the named lane says
        // everything it did -- and is never a synonym for it.
        let pair = in_lane_hazard_call("Slow car right ahead.", OpenSide::Left);
        assert_eq!(
            pair.normal,
            "Change lanes or brake! Slow car right ahead. Left lane open."
        );
        assert_eq!(
            pair.terse.as_deref(),
            Some("Slow car right ahead. Left lane open.")
        );
        assert_eq!(
            in_lane_hazard_call("Debris on the road.", OpenSide::Right).normal,
            "Change lanes or brake! Debris on the road. Right lane open."
        );
        assert_eq!(
            in_lane_hazard_call("Debris on the road.", OpenSide::Either).normal,
            "Change lanes or brake! Debris on the road. Either lane open."
        );
    }

    #[test]
    fn test_on_a_road_three_wide_the_call_names_the_lane_a_tap_lands_in() {
        // Agent drive to Uvalde, 2026-10-06: from the right lane of three the
        // call said "Left lane open." and the tap then said "Changing to the
        // middle lane." The answer names the lane the L key would.
        assert_eq!(OpenSide::Left.spoken_from(0, 3), "Middle lane open.");
        assert_eq!(OpenSide::Right.spoken_from(2, 3), "Middle lane open.");
        assert_eq!(OpenSide::Left.spoken_from(1, 3), "Left lane open.");
        assert_eq!(OpenSide::Right.spoken_from(1, 3), "Right lane open.");
        assert_eq!(OpenSide::Either.spoken_from(1, 3), "Either lane open.");
        // Two wide, nothing changes.
        assert_eq!(OpenSide::Left.spoken_from(0, 2), "Left lane open.");
        assert_eq!(OpenSide::Right.spoken_from(1, 2), "Right lane open.");
        let named = OpenSide::Left.spoken_from(0, 3);
        assert_eq!(
            in_lane_hazard_call_named("A sudden lane closure ahead.", OpenSide::Left, &named)
                .normal,
            "Change lanes or brake! A sudden lane closure ahead. Middle lane open."
        );
    }

    #[test]
    fn test_with_no_lane_open_the_call_is_brake_and_says_so_in_both_modes() {
        // Nowhere to go: the "Brake!" family, and the driver is told not to
        // reach for a lane change. "Brake!" survives terse, as it always has.
        let pair = in_lane_hazard_call("Slow car right ahead.", OpenSide::Neither);
        assert_eq!(pair.normal, "Brake! Slow car right ahead. No lane open.");
        assert_eq!(pair.terse.as_deref(), Some(pair.normal.as_str()));
        assert!(!pair.normal.contains("Change lanes"));
    }

    /// The Python half that reads `main_menu_help.py` stays with the help
    /// port; the phrase itself is pinned here.
    #[test]
    fn test_the_passing_call_names_the_side_the_truck_takes() {
        let left = passing_hazard_call("Slow car right ahead.", OpenSide::Either).unwrap();
        assert_eq!(left.normal, "Slow car right ahead. Passing on the left.");
        assert_eq!(left.terse, None);
        let right = passing_hazard_call("Slow semi ahead.", OpenSide::Right).unwrap();
        assert_eq!(right.normal, "Slow semi ahead. Passing on the right.");
        assert!(passing_hazard_call("Slow car ahead.", OpenSide::Neither).is_none());
    }

    #[test]
    fn test_the_dodge_call_is_the_phrase_the_help_teaches() {
        assert_eq!(
            HAZARD_DODGE_CALL.trim_end_matches('!'),
            "Change lanes or brake"
        );
    }

    /// Only "Brake now!" is implied by the tone now.
    ///
    /// "Brake!" was here until 2026-08-17. It looks like the same redundancy
    /// and is not: the emitter uses it ONLY where the hazard is dodgeable but
    /// no lane is open, so it answers the question the driver is asking.
    /// With it dropped, quiet left a noun phrase with no verb and the owner
    /// reached for a lane change three times on a one-lane stretch.
    #[test]
    fn test_tone_implied_calls_drop_to_the_body_in_terse() {
        assert_eq!(
            hazard_call("Brake now!", "Deer in the road.")
                .terse
                .as_deref(),
            Some("Deer in the road.")
        );
        assert_eq!(
            hazard_call("Brake now!", "Deer in the road.").normal,
            "Brake now! Deer in the road."
        );
        assert_eq!(
            hazard_call("Brake!", "Mattress in your lane.")
                .terse
                .as_deref(),
            Some("Brake! Mattress in your lane.")
        );
    }

    // -- certainty survives compression ----------------------------------------

    #[test]
    fn test_parking_certainty_keeps_all_five_values_distinguishable() {
        // The Python compared against `world_constants.PARKING_CERTAINTY_LABELS`;
        // its keys are pinned here until the data module is ported.
        let mut keys: Vec<&str> = TERSE_PARKING_LABELS.iter().map(|(k, _)| *k).collect();
        keys.sort_unstable();
        assert_eq!(keys, ["confirmed", "likely", "limited", "none", "unknown"]);
        let spoken: Vec<&str> = TERSE_PARKING_LABELS
            .iter()
            .filter(|(key, _)| *key != "likely")
            .map(|(_, label)| *label)
            .collect();
        assert!(
            spoken.iter().all(|label| !label.is_empty()),
            "a certainty value lost its words"
        );
        let mut distinct = spoken.clone();
        distinct.sort_unstable();
        distinct.dedup();
        assert_eq!(
            distinct.len(),
            spoken.len(),
            "two certainty values collapsed"
        );
    }

    #[test]
    fn test_likely_is_silence_in_terse_exactly_as_in_normal() {
        // PARKING_CERTAINTY_LABELS["likely"] == "" on the data side.
        assert_eq!(terse_parking_label("likely"), Some(""));
    }

    #[test]
    fn test_unknown_is_spoken_not_verified_never_unverified() {
        assert_eq!(
            terse_parking_label("unknown"),
            Some("Parking not verified.")
        );
    }

    // -- slot grammar (docs/ontology.md) ---------------------------------------

    #[test]
    fn test_hazard_family_cues_speak_thing_distance_target_speed() {
        assert_eq!(
            brake_lights_cue("2.1 miles", "38 miles per hour", "38", "")
                .terse
                .as_deref(),
            Some("Brake lights, 2.1 miles, 38.")
        );
        assert_eq!(
            slow_lead_cue("car", "1.2 miles", "40 miles per hour", "40")
                .terse
                .as_deref(),
            Some("Slow car, 1.2 miles, 40.")
        );
    }

    #[test]
    fn test_hazard_family_cues_keep_their_normal_coaching() {
        assert_eq!(
            brake_lights_cue("2.1 miles", "38 miles per hour", "38", "").normal,
            "Brake lights 2.1 miles ahead. Traffic at 38 miles per hour."
        );
        assert_eq!(
            slow_lead_cue("car", "1.2 miles", "40 miles per hour", "40").normal,
            "Slow car 1.2 miles ahead, at 40 miles per hour."
        );
    }

    #[test]
    fn test_brake_lights_cause_rides_the_full_form_only() {
        let pair = brake_lights_cue("2.1 miles", "38 miles per hour", "38", "Merge ahead.");
        assert_eq!(
            pair.normal,
            "Brake lights 2.1 miles ahead. Merge ahead. Traffic at 38 miles per hour."
        );
        assert_eq!(pair.terse.as_deref(), Some("Brake lights, 2.1 miles, 38."));
    }

    /// A merging vehicle merges behind or passes on its own -- no target
    /// speed for the truck to be ready for, so the frame is [thing,
    /// distance] rather than the full hazard-family [thing, distance, target
    /// speed].
    #[test]
    fn test_merging_cue_drops_the_speed_advisory() {
        let pair = merging_traffic_cue("box truck", "0.4 miles");
        assert_eq!(pair.terse.as_deref(), Some("Merging box truck, 0.4 miles."));
        assert_eq!(pair.normal, "Merging box truck 0.4 miles ahead.");
        assert!(!pair.normal.contains("be ready for"));
        assert!(!pair.terse.as_deref().unwrap().contains("be ready for"));
    }

    #[test]
    fn test_stop_callout_speaks_name_exit_distance_qualifier() {
        let pair = stop_callout(&StopCalloutParts {
            planned_prefix: "",
            typed_name: "travel center: Flying J Travel Center Corfu",
            plain_name: "Flying J Travel Center Corfu",
            exit_label: "exit 48A",
            distance: "5 miles",
            parking_normal: "confirmed truck parking",
            parking_certainty: "confirmed",
            ..Default::default()
        });
        assert_eq!(
            pair.normal,
            "travel center: Flying J Travel Center Corfu at exit 48A in 5 miles. \
             Confirmed truck parking. Press X to signal for the exit."
        );
        assert_eq!(
            pair.terse.as_deref(),
            Some("Flying J Travel Center Corfu, exit 48A, 5 miles. Parking confirmed.")
        );
    }

    #[test]
    fn test_stop_callout_without_exit_or_parking_keeps_the_frame() {
        let pair = stop_callout(&StopCalloutParts {
            planned_prefix: "Planned stop, ",
            typed_name: "public rest area: Sweetwater Rest Area",
            plain_name: "Sweetwater Rest Area",
            exit_label: "",
            distance: "3 miles",
            parking_normal: "",
            parking_certainty: "likely",
            ..Default::default()
        });
        assert_eq!(
            pair.terse.as_deref(),
            Some("Planned stop, Sweetwater Rest Area, 3 miles.")
        );
        assert!(pair.normal.contains("Press X"));
    }

    #[test]
    fn test_the_terse_stop_callout_never_drops_a_bad_parking_verdict() {
        let pair = stop_callout(&StopCalloutParts {
            planned_prefix: "",
            typed_name: "truck fuel station: Corner Pump",
            plain_name: "Corner Pump",
            exit_label: "",
            distance: "2 miles",
            parking_normal: "no truck parking",
            parking_certainty: "none",
            ..Default::default()
        });
        assert_eq!(
            pair.terse.as_deref(),
            Some("Corner Pump, 2 miles. No truck parking.")
        );
    }

    #[test]
    fn test_an_unknown_certainty_reads_as_not_verified() {
        let pair = stop_callout(&StopCalloutParts {
            plain_name: "Corner Pump",
            distance: "2 miles",
            parking_certainty: "made-up",
            ..Default::default()
        });
        assert_eq!(
            pair.terse.as_deref(),
            Some("Corner Pump, 2 miles. Parking not verified.")
        );
    }

    #[test]
    fn test_stop_callout_drops_the_exit_instruction_when_the_hint_is_empty() {
        let callout = |exit_hint: &str| {
            stop_callout(&StopCalloutParts {
                planned_prefix: "",
                typed_name: "travel center: Flying J",
                plain_name: "Flying J",
                exit_label: "exit 48A",
                distance: "5 miles",
                parking_normal: "confirmed truck parking",
                parking_certainty: "confirmed",
                exit_hint,
                fuel: None,
            })
        };

        let taught = callout("X");
        let retired = callout("");
        assert!(taught.normal.contains("Press X to signal for the exit."));
        assert!(!retired.normal.contains("signal for the exit"));
        // The route facts survive either way.
        assert!(retired.normal.contains("Flying J"));
        assert!(retired.normal.contains("Confirmed truck parking"));
    }

    #[test]
    fn test_stop_callout_says_whether_the_stop_sells_fuel() {
        let callout = |fuel: Option<bool>| {
            stop_callout(&StopCalloutParts {
                typed_name: "truck parking: The 115 Truck Stop",
                plain_name: "The 115 Truck Stop",
                exit_label: "exit 115",
                distance: "5 miles",
                parking_normal: "confirmed truck parking",
                parking_certainty: "confirmed",
                fuel,
                ..Default::default()
            })
        };
        let none = callout(Some(false));
        assert_eq!(
            none.normal,
            "truck parking: The 115 Truck Stop at exit 115 in 5 miles. \
             Confirmed truck parking. No fuel. Press X to signal for the exit."
        );
        assert_eq!(
            none.terse.as_deref(),
            Some("The 115 Truck Stop, exit 115, 5 miles. Parking confirmed. No fuel.")
        );
        let pumps = callout(Some(true));
        assert_eq!(
            pumps.normal,
            "truck parking: The 115 Truck Stop at exit 115 in 5 miles. \
             Confirmed truck parking. Fuel. Press X to signal for the exit."
        );
        // Terse keeps only the verdict that changes the plan.
        assert_eq!(
            pumps.terse.as_deref(),
            Some("The 115 Truck Stop, exit 115, 5 miles. Parking confirmed.")
        );
        let unsaid = callout(None);
        assert!(!unsaid.normal.contains("fuel."), "{}", unsaid.normal);
        assert!(!unsaid.terse.as_deref().unwrap().contains("fuel"));
    }

    #[test]
    fn test_a_repeat_mention_still_carries_no_fuel() {
        // After the first mention the proper name stands alone; the fuel
        // verdict must not leave with the stop type.
        let pair = stop_callout(&StopCalloutParts {
            typed_name: "The 115 Truck Stop",
            plain_name: "The 115 Truck Stop",
            distance: "2 miles",
            parking_certainty: "likely",
            fuel: Some(false),
            ..Default::default()
        });
        assert_eq!(
            pair.normal,
            "The 115 Truck Stop in 2 miles. No fuel. Press X to signal for the exit."
        );
        assert_eq!(
            pair.terse.as_deref(),
            Some("The 115 Truck Stop, 2 miles. No fuel.")
        );
    }

    #[test]
    fn test_the_charged_toll_speaks_what_amount_who_pays() {
        let pair = toll_charged("E-ZPass", "New York State Thruway settlement", "15", true);
        assert_eq!(
            pair.normal,
            "E-ZPass toll at New York State Thruway settlement, \
             estimated 15 dollars, billed to carrier settlement."
        );
        assert_eq!(pair.terse.as_deref(), Some("Toll, 15 dollars, carrier."));
    }

    #[test]
    fn test_the_speed_nag_compresses_to_the_limit() {
        let pair = overspeed_nag("65 miles per hour", "65");
        assert_eq!(pair.normal, "Over the limit of 65 miles per hour.");
        assert_eq!(pair.terse.as_deref(), Some("Limit 65."));
    }

    #[test]
    fn test_the_cruise_easing_clause_folds_into_the_pacenote_in_terse() {
        let pair = cruise_curve_easing(
            &"Sharp left, half a mile. Advise 35 miles per hour.".into(),
            "35 miles per hour",
        );
        assert_eq!(
            pair.normal,
            "Sharp left, half a mile. Advise 35 miles per hour. \
             Adaptive cruise easing to 35 miles per hour."
        );
        assert_eq!(
            pair.terse.as_deref(),
            Some("Sharp left, half a mile. Advise 35 miles per hour.")
        );
    }

    #[test]
    fn test_an_achievement_is_its_name_alone_in_terse() {
        let pair = achievement_unlocked(
            "Bumper-to-Bumper Blues",
            "Heavy traffic, and you kept it sane.",
        );
        assert_eq!(
            pair.normal,
            "New achievement! Bumper-to-Bumper Blues. Heavy traffic, and you kept it sane."
        );
        assert_eq!(pair.terse.as_deref(), Some("Bumper-to-Bumper Blues."));
        let live = achievement_announced("Bumper-to-Bumper Blues");
        assert_eq!(live.normal, "New achievement! Bumper-to-Bumper Blues.");
        assert_eq!(live.terse.as_deref(), Some("Bumper-to-Bumper Blues."));
    }

    // -- the delivery layer resolves normal/terse pairs (R5) ------------------

    #[test]
    fn test_a_pair_behaves_as_its_normal_string() {
        let pair = SpokenMessage::with_terse(
            "Watch your speed. The limit is 65 miles per hour.",
            "Limit 65.",
        );
        assert_eq!(pair, "Watch your speed. The limit is 65 miles per hour.");
        assert!(pair.normal.contains("limit"));
        assert_eq!(format!("{pair}"), pair.normal);
        let from_text: SpokenMessage = "Collision!".into();
        assert_eq!(from_text.normal, "Collision!");
        assert_eq!(from_text.terse, None);
    }

    #[test]
    fn test_render_picks_the_mode() {
        let pair = SpokenMessage::with_terse(
            "Watch your speed. The limit is 65 miles per hour.",
            "Limit 65.",
        );
        assert_eq!(
            pair.render(false),
            "Watch your speed. The limit is 65 miles per hour."
        );
        assert_eq!(pair.render(true), "Limit 65.");
    }

    #[test]
    fn test_no_terse_rendering_means_both_modes_speak_the_same() {
        let line = SpokenMessage::new("Collision! The truck took damage.");
        assert_eq!(line.render(true), line.render(false));
        assert_eq!(line.render(true), line.to_string());
    }

    #[test]
    fn test_empty_terse_rendering_drops_the_line_whole() {
        let line = terse_silent("You slow nearly to a stop and ease around it. Well done.");
        assert_eq!(
            line.render(false),
            "You slow nearly to a stop and ease around it. Well done."
        );
        assert_eq!(line.render(true), "");
    }

    #[test]
    fn test_plus_extends_both_renderings() {
        let pair = SpokenMessage::with_terse(
            "Change lanes or brake! Slow car ahead.",
            "Change lanes or brake! Slow car ahead.",
        );
        let grown = pair.plus("Automatic speed control canceled.");
        assert!(grown
            .render(false)
            .ends_with("Automatic speed control canceled."));
        assert!(grown
            .render(true)
            .ends_with("Automatic speed control canceled."));
        assert!(grown.render(true).starts_with("Change lanes or brake!"));
        // A line with no terse form stays that way.
        let plain = SpokenMessage::new("Collision!").plus("Total damage 12 percent.");
        assert_eq!(plain.terse, None);
        assert_eq!(plain.normal, "Collision! Total damage 12 percent.");
    }

    #[test]
    fn test_plus_on_a_dropped_line_keeps_the_suffix_in_terse() {
        let grown = terse_silent("Through the bend, held your line.").plus("Cruise resuming.");
        assert_eq!(grown.render(true), "Cruise resuming.");
    }

    // -- R6: facility names ----------------------------------------------------

    #[test]
    fn test_a_redundant_type_prefix_is_dropped() {
        assert!(type_prefix_is_redundant("cross-dock", "Chicago Cross-Dock"));
        assert!(type_prefix_is_redundant(
            "port",
            "Port of Indiana-Burns Harbor"
        ));
        assert!(type_prefix_is_redundant(
            "travel center",
            "Flying J Travel Center Corfu"
        ));
        assert_eq!(
            typed_name("cross-dock", "Chicago Cross-Dock", ": "),
            "Chicago Cross-Dock"
        );
    }

    #[test]
    fn test_a_prefix_the_name_does_not_carry_survives() {
        assert!(!type_prefix_is_redundant("travel center", "Love's"));
        assert!(!type_prefix_is_redundant(
            "service plaza",
            "Petro Stopping Centers"
        ));
        // A short label must not fire on a coincidental substring.
        assert!(!type_prefix_is_redundant("port", "Newport Terminal"));
        assert_eq!(
            typed_name("travel center", "Love's", " "),
            "travel center Love's"
        );
        // An empty label is never redundant.
        assert!(!type_prefix_is_redundant("", "Love's"));
    }

    // -- roadside chatter ------------------------------------------------------

    /// The renderer itself, on the shapes the bake actually produces.
    #[test]
    fn test_roadside_chatter_short_forms_keep_the_fact() {
        let cases = [
            (
                "Entering Hot Springs National Park.",
                "Hot Springs National Park.",
            ),
            ("Crossing the Cahaba River.", "Cahaba River."),
            ("Approaching Lone Pine Saddle.", "Lone Pine Saddle."),
            ("Cullman County Museum ahead.", "Cullman County Museum."),
            // An initial inside a name is not a sentence end.
            (
                "Museum ahead: Jamie L. Whitten Historical Center.",
                "Jamie L. Whitten Historical Center.",
            ),
            // Prose keeps its opening clause: the name and the fact.
            (
                "You are passing Ozark beside Fort Novosel, the home of Army Aviation, \
                 where every Army helicopter pilot learns to fly.",
                "Ozark beside Fort Novosel.",
            ),
            // The lead-in stripping itself, on a billboard-shaped line. Real
            // billboards come through under their own category and are never
            // cut at all -- see `test_a_billboard_is_never_cut_down`.
            ("Billboard: Free ice water.", "Free ice water."),
            ("Billboard: Eat here. Get gas.", "Eat here. Get gas."),
        ];
        for (spoken, expected) in cases {
            let message = roadside_chatter(spoken, "test");
            assert_eq!(message.normal, spoken);
            assert_eq!(message.render(true), expected, "{spoken}");
            assert!(message.render(true).chars().count() < spoken.chars().count());
        }
    }

    #[test]
    fn test_roadside_chatter_only_offers_a_form_that_is_actually_shorter() {
        // No lead-in, no tail, already short: no terse form is offered.
        let message = roadside_chatter("Mile 40.", "test");
        assert_eq!(message.normal, "Mile 40.");
        assert_eq!(message.terse, None);
        // Long prose with no sentence end or comma is cut nowhere, but the
        // trailing period still comes off and back, so it stays the same.
        let prose =
            "Entering a long stretch of road with nothing whatsoever to say about it at all here";
        let long = roadside_chatter(prose, "test");
        assert_eq!(
            long.render(true),
            "a long stretch of road with nothing whatsoever to say about it at all here."
        );
    }

    /// Brandon, 2026-08-22: the billboards and the signs you pass should read
    /// in full at quiet.
    ///
    /// Every other chatter category is a label wrapped in framing, so terse
    /// drops the framing and loses nothing. A billboard is the sign's own
    /// words, and the payload is usually the last sentence -- cutting it to
    /// the opening clause leaves the setup without the punchline. Both
    /// billboard categories keep the whole line, framing included, at every
    /// rung.
    #[test]
    fn test_a_billboard_is_never_cut_down() {
        let signs = [
            "Billboard: Meteor Crater is ahead, a hole in the desert nearly a mile \
             wide that was punched out by a rock from space. It is bigger than it \
             sounds. Much bigger.",
            "Billboard: Idaho panhandle country. Colby Acuff and the Western White \
             Pines both grew up here.",
        ];
        for category in UNCUT_CHATTER_CATEGORIES {
            for spoken in signs {
                let message = roadside_chatter(spoken, category);
                assert_eq!(message.normal, spoken);
                assert_eq!(
                    message.render(true),
                    spoken,
                    "{category} lost its punchline"
                );
            }
        }

        // A label still shortens, so this spares the gags without undoing terse.
        let label = roadside_chatter("Entering Hot Springs National Park.", "national_park");
        assert_eq!(label.render(true), "Hot Springs National Park.");
    }

    /// The spared set and the switch must name the same categories.
    ///
    /// A new billboard bake category that rode the billboards switch but
    /// missed the spared set would be silently cut back down, which is the
    /// bug this fixed arriving again under a different key.
    #[test]
    fn test_every_billboard_category_is_spared_the_cut() {
        let mut on_the_switch: Vec<&str> = crate::settings::CHATTER_CATEGORY_FIELDS
            .iter()
            .filter(|(_, field)| *field == "chatter_billboards")
            .map(|(category, _)| *category)
            .collect();
        on_the_switch.sort_unstable();
        let mut spared = UNCUT_CHATTER_CATEGORIES.to_vec();
        spared.sort_unstable();
        assert_eq!(on_the_switch, spared);
    }

    // -- from the ladder file --------------------------------------------------

    /// A plus sign is all it took to lose the quiet rung's whole benefit.
    ///
    /// `SpokenMessage` subclassed `str`, so `message + " ..."` handed back a
    /// plain `str` and the terse rendering was gone without a word. The curve
    /// call was built as a pair, and one branch concatenated a cruise
    /// handback onto it, so at quiet the driver still heard the full sentence
    /// (owner playtest, 2026-08-17).
    #[test]
    fn test_wrapping_a_curve_call_never_flattens_its_short_form() {
        let pacenote = SpokenMessage::with_terse(
            "Sharp right, half a mile. Advise 35 miles per hour.",
            "Sharp right, 35 miles per hour.",
        );
        for wrapped in [
            cruise_curve_paused(&pacenote),
            cruise_curve_easing(&pacenote, "35 miles per hour"),
        ] {
            let terse = wrapped
                .terse
                .as_deref()
                .expect("the pair must survive the wrapper");
            assert!(
                !terse.contains("half a mile"),
                "the short form must stay short"
            );
            assert_ne!(terse, wrapped.to_string());
        }
        // And the handback is a pause, in both rungs: the session survives
        // the bend, so "off" would be a lie the driver plans around.
        let paused = cruise_curve_paused(&pacenote);
        assert_eq!(
            paused.normal,
            "Sharp right, half a mile. Advise 35 miles per hour. Adaptive cruise paused for the \
             bend. Resumes past it."
        );
        assert_eq!(
            paused.terse.as_deref(),
            Some("Sharp right, 35 miles per hour. Cruise paused.")
        );
    }

    /// Owner playtest, 2026-08-17: three lane-change attempts in one drive
    /// on a one-lane stretch of US-285, each answered with "there is no lane
    /// to your left here".
    ///
    /// The emitter uses a bare "Brake!" ONLY where the hazard is dodgeable
    /// but no lane is open, so it is the one call that answers the question
    /// the driver is actually asking -- can I go around this? Terse used to
    /// drop it as tone-implied, leaving a noun phrase with no verb. "Brake
    /// now!" stays implied: it marks a hazard that cannot be dodged, so
    /// there is no choice for the word to inform.
    #[test]
    fn test_a_dodgeable_hazard_with_nowhere_to_go_keeps_the_word_brake() {
        let no_lane = hazard_call("Brake!", "Brake lights right ahead.");
        assert!(
            no_lane.render(true).starts_with("Brake!"),
            "{}",
            no_lane.render(true)
        );

        let undodgeable = hazard_call("Brake now!", "Rockfall right ahead.");
        assert!(!undodgeable.render(true).starts_with("Brake now!"));
        assert_eq!(undodgeable.render(true), "Rockfall right ahead.");

        // The open-lane call survives whole: it names the better option.
        let dodgeable = hazard_call(HAZARD_DODGE_CALL, "Deer right ahead.");
        assert!(dodgeable.render(true).starts_with(HAZARD_DODGE_CALL));
    }
}
