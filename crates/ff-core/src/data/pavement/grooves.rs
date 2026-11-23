//! Bridge decks, and whether the surface under the tire is grooved across
//! the lane: the readings the tire whine and the grate hum play from.
//!
//! The deck is READ from the National Bridge Inventory (`cargo tools
//! nbi-decks`). Whether a surface is grooved across the lane is never read:
//! HPMS has no texture field and NBI none for the deck's finish. It is
//! DERIVED for pavement (build year and published practice) and ASSUMED for
//! concrete bridge decks, and [`Grooves`] says which. The rule is one
//! function, [`cross_grooved`], so it can be re-judged without a re-bake.

use super::{Surface, SurfaceRun};

/// NBI Item 107 (deck structure type) with Item 108A (wearing surface), as
/// the layer names them. FHWA Recording and Coding Guide for the Structure
/// Inventory and Appraisal of the Nation's Bridges (1995), pages 70 and 71.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Deck {
    /// Item 107 code 3: the tire runs on the open steel bars.
    OpenGrating,
    /// Item 107 code 1 or 2 (cast-in-place or precast concrete) under an
    /// Item 108A of 0 to 4 (bare deck, monolithic, integral, latex or low
    /// slump concrete): the tire runs on concrete.
    Concrete,
    /// Item 108A code 6, bituminous.
    Asphalt,
    /// Anything else: closed grating, steel plate, timber, an epoxy
    /// overlay, gravel.
    Other,
}

const DECKS: &[(Deck, &str)] = &[
    (Deck::OpenGrating, "open_grating"),
    (Deck::Concrete, "concrete"),
    (Deck::Asphalt, "asphalt"),
    (Deck::Other, "other"),
];

impl Deck {
    pub fn from_name(name: &str) -> Option<Self> {
        DECKS.iter().find(|(_, n)| *n == name).map(|(d, _)| *d)
    }

    pub fn name(self) -> &'static str {
        DECKS
            .iter()
            .find(|(d, _)| *d == self)
            .map_or("other", |(_, n)| n)
    }
}

/// How the game knows a surface is grooved across the lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Grooves {
    /// Exposed concrete built in the transverse-tining years and not
    /// improved since: DERIVED from HPMS years and published practice.
    DerivedFromBuildYear,
    /// A concrete bridge deck: the surface is READ, the grooves ASSUMED.
    AssumedDeck,
}

/// The national transverse-tining years, inclusive. The first is the
/// first construction season after FHWA Technical Advisory T 5140.10
/// (1979-09-18), which made transverse tining "the predominant form of
/// tining on highway pavements"; the last is the year of Technical Advisory
/// T 5040.36 (2005-06-17), after which "most states moved from transverse
/// tining and have adopted longitudinal tining" (IGGA/ACPA, "Longitudinal
/// versus Transverse Grooving", June 2020; FHWA-HIF-18-038). Both are the
/// advisories' own dates, not tuned.
pub const TRANSVERSE_FROM: i64 = 1980;
pub const TRANSVERSE_TO: i64 = 2005;

/// A state whose pavement practice is on record apart from the national
/// years: `None` for `from` means it never tined pavement across the lane.
pub struct StatePavement {
    pub state: &'static str,
    pub from: Option<i64>,
    pub source: &'static str,
}

const MARQUETTE: &str = "Drakopoulos, Shaw and Ornek, \"Wet Pavements Crash Study of \
                         Longitudinal and Transverse Tined PCC\", Marquette University for \
                         WisDOT, June 2007, appendix A, page A3";

/// Per-state pavement practice, a citation each. Only start years are on
/// record; every state keeps the national end year, since the Marquette
/// survey (2007) found these still tining across and says nothing of when
/// they stopped.
pub const STATE_PAVEMENT: &[StatePavement] = &[
    StatePavement {
        state: "California",
        from: None,
        source: "IGGA/ACPA, \"Longitudinal versus Transverse Grooving\", June 2020: California \
                 never used transverse tining on highways; Marquette 2007, appendix A, page A3: \
                 longitudinal tining since the 1970s on all high-speed pavements",
    },
    StatePavement {
        state: "Michigan",
        from: Some(1975),
        source: MARQUETTE,
    },
    StatePavement {
        state: "Wisconsin",
        from: Some(1978),
        source: MARQUETTE,
    },
];

/// A state whose concrete bridge decks are on record as grooved along the
/// lane, so the deck assumption does not hold there.
pub struct StateDecks {
    pub state: &'static str,
    pub source: &'static str,
}

pub const LONGITUDINAL_DECKS: &[StateDecks] = &[StateDecks {
    state: "Iowa",
    source: "Iowa DOT Standard Specifications (2008), section 2412.06: Interstate and Primary \
             bridge decks are grooved longitudinally",
}];

/// A concrete deck shorter than this defers to the pavement run around it:
/// the bridge joints' own bar (`sim::road_texture::joints::MIN_SPAN_MI`),
/// under which HPMS's structures are nearly all culverts.
pub const DECK_MIN_MI: f64 = 0.05;

/// Exposed concrete: the tire runs on a concrete surface HPMS reads.
pub fn exposed_concrete(surface: Surface) -> bool {
    matches!(
        surface,
        Surface::JointedPlainConcrete
            | Surface::JointedReinforcedConcrete
            | Surface::ContinuousConcrete
            | Surface::UnbondedJointedConcreteOverlay
            | Surface::BondedConcreteOverlay
    )
}

/// Whether a surface is grooved across the lane, and how the game knows.
///
/// - On a structure with a deck reading (`deck`, with the structure's
///   length `deck_mi`): an open grating is never grooved (it has its own
///   reading); a concrete deck of [`DECK_MIN_MI`] or more is
///   [`Grooves::AssumedDeck`] outside [`LONGITUDINAL_DECKS`]. California
///   tined its decks across the lane while tining its pavement along it
///   (Marquette 2007, appendix A, page A3), so the deck rule holds there.
///   Any other deck is not grooved. A shorter concrete deck defers to the
///   run around it, so a culvert in a grooved run does not go quiet.
/// - Otherwise the HPMS run (`run`): exposed concrete built inside the
///   state's transverse years ([`STATE_PAVEMENT`], else
///   [`TRANSVERSE_FROM`] to [`TRANSVERSE_TO`]) and not improved since is
///   [`Grooves::DerivedFromBuildYear`]. An improvement later than the build
///   is usually diamond grinding, which removes the grooves. No build year
///   means nothing says, and the road is quiet.
pub fn cross_grooved(
    deck: Option<(Deck, f64)>,
    run: Option<&SurfaceRun>,
    state: &str,
) -> Option<Grooves> {
    match deck {
        Some((Deck::Concrete, mi)) if mi >= DECK_MIN_MI => {
            return (!LONGITUDINAL_DECKS.iter().any(|s| s.state == state))
                .then_some(Grooves::AssumedDeck);
        }
        Some((Deck::Concrete, _)) | None => {}
        Some(_) => return None,
    }
    let run = run?;
    if !exposed_concrete(run.surface) {
        return None;
    }
    let built = run.built?;
    if run.improved.is_some_and(|y| y > built) {
        return None;
    }
    let from = match STATE_PAVEMENT.iter().find(|s| s.state == state) {
        Some(s) => s.from?,
        None => TRANSVERSE_FROM,
    };
    (from..=TRANSVERSE_TO)
        .contains(&built)
        .then_some(Grooves::DerivedFromBuildYear)
}
