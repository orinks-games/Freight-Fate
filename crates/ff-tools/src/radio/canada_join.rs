//! The pieces `import-canada-stations` joins with: ISED's operating
//! transmitters, the name cleaning the US tier's import used, and what a
//! Radio Browser row says about its own call sign, band, frequency and
//! province.

use std::collections::BTreeMap;
use std::path::Path;

use regex::Regex;
use std::sync::LazyLock as Lazy;

use super::dbf;
use crate::util::Result;

/// The ten provinces and three territories, by ISED's two-letter code.
pub const PROVINCES: &[(&str, &str)] = &[
    ("AB", "alberta"),
    ("BC", "british columbia"),
    ("MB", "manitoba"),
    ("NB", "new brunswick"),
    ("NL", "newfoundland"),
    ("NS", "nova scotia"),
    ("NT", "northwest territories"),
    ("NU", "nunavut"),
    ("ON", "ontario"),
    ("PE", "prince edward island"),
    ("QC", "quebec"),
    ("SK", "saskatchewan"),
    ("YT", "yukon"),
];

/// ISED's province code with Newfoundland's old `NF` folded to `NL`, or
/// `None` for a record outside Canada (the tables list US border stations).
pub fn province_code(ised: &str) -> Option<&'static str> {
    let code = if ised == "NF" { "NL" } else { ised };
    PROVINCES.iter().find(|(c, _)| *c == code).map(|(c, _)| *c)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Band {
    Am,
    Fm,
}

impl Band {
    pub fn label(self) -> &'static str {
        match self {
            Band::Am => "AM",
            Band::Fm => "FM",
        }
    }
}

/// One operating ISED transmitter.
#[derive(Clone, Debug)]
pub struct Transmitter {
    /// As ISED writes it: `CBLA-FM`, `CBLA-FM-4`, `CFRB`.
    pub call_sign: String,
    /// The station's call sign without band or rebroadcaster suffix: `CBLA`.
    pub base: String,
    pub band: Band,
    /// MHz for FM, kHz for AM.
    pub frequency: f64,
    pub province: &'static str,
    pub community: String,
    pub lat: f64,
    pub lon: f64,
    /// The largest of the four licensed FM powers, in kW; `None` for AM and
    /// for an FM record that states none.
    pub erp_kw: Option<f64>,
    /// No `-1`, `-2` rebroadcaster suffix.
    pub main: bool,
}

/// Every transmitter of one station (call sign base and band).
pub type Stations = BTreeMap<(String, Band), Vec<Transmitter>>;

static FM_CALL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(C[A-Z]{1,3}|VO[A-Z]{2})(?:-FM)?(-\d+)?$").expect("regex"));
static AM_CALL: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^(C[A-Z]{1,3}|VO[A-Z]{2})(-\d+)?$").expect("regex"));

/// ISED's DDMMSS (or DDDMMSS) digits as decimal degrees, west negative.
pub fn dms(raw: &str, west: bool) -> Option<f64> {
    let digits = raw.trim();
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let value: u64 = digits.parse().ok()?;
    let (d, m, s) = (value / 10_000, (value / 100) % 100, value % 100);
    if d == 0 || m >= 60 || s >= 60 {
        return None;
    }
    let degrees = d as f64 + m as f64 / 60.0 + s as f64 / 3600.0;
    let rounded = (degrees * 100_000.0).round() / 100_000.0;
    Some(if west { -rounded } else { rounded })
}

fn number(row: &dbf::Row, key: &str) -> f64 {
    row.get(key)
        .and_then(|v| v.trim().parse::<f64>().ok())
        .unwrap_or(0.0)
}

fn transmitter(row: &dbf::Row, band: Band) -> Option<Transmitter> {
    let operating = match band {
        Band::Fm => "OP",
        Band::Am => "O",
    };
    if row.get("BANNER").map(String::as_str) != Some(operating) {
        return None;
    }
    let province = province_code(row.get("PROVINCE")?)?;
    let call_sign = row.get("CALL_SIGN")?.trim().to_string();
    let pattern = match band {
        Band::Fm => &*FM_CALL,
        Band::Am => &*AM_CALL,
    };
    let caps = pattern.captures(&call_sign)?;
    let lat = dms(row.get("LATITUDE")?, false)?;
    let lon = dms(row.get("LONGITUDE")?, true)?;
    let erp_w = ["ERPVPK", "ERPHPK", "ERPVAV", "ERPHAV"]
        .iter()
        .map(|k| number(row, k))
        .fold(0.0, f64::max);
    Some(Transmitter {
        base: caps[1].to_string(),
        main: caps.get(2).is_none(),
        call_sign,
        band,
        frequency: number(row, "FREQUENCY"),
        province,
        community: row.get("CITY").cloned().unwrap_or_default(),
        lat,
        lon,
        erp_kw: (band == Band::Fm && erp_w > 0.0).then_some(erp_w / 1000.0),
    })
}

/// Every operating Canadian transmitter in the two ISED tables, grouped by
/// station.
pub fn load_ised(dir: &Path) -> Result<Stations> {
    let mut stations = Stations::new();
    for (file, band) in [("fmstatio.dbf", Band::Fm), ("amstatio.dbf", Band::Am)] {
        for row in dbf::read(&dir.join(file))? {
            if let Some(t) = transmitter(&row, band) {
                stations.entry((t.base.clone(), band)).or_default().push(t);
            }
        }
    }
    Ok(stations)
}

/// The station's main transmitter: the unsuffixed record, the most
/// powerful one when ISED lists it twice. A station whose main record is
/// not operating has none.
pub fn main_transmitter(sites: &[Transmitter]) -> Option<&Transmitter> {
    sites
        .iter()
        .filter(|t| t.main)
        .fold(None, |best, t| match best {
            Some(b) if b.erp_kw.unwrap_or(0.0) >= t.erp_kw.unwrap_or(0.0) => Some(b),
            _ => Some(t),
        })
}

// -- name cleaning, as tools/import_radio_catalog.py did it for the US tier --

static NOISE_WORDS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(
        r"(?i)\b(?:aac\+?|mp3|ogg|opus|flac|hls|hd\d?|hi-?fi|stereo|kbps|kbit|\d{2,3}\s?k(?:bps|bit|b)?|stream(?:ing)?)\b",
    )
    .expect("regex")
});
static NAME_SPLIT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"\s*(?:\||-\s*-\s*-|/{2,})\s*").expect("regex"));
static EMPTY_BRACKETS: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"[(\[][^A-Za-z0-9]*[)\]]").expect("regex"));
static SPACES: Lazy<Regex> = Lazy::new(|| Regex::new(r"\s+").expect("regex"));
static LEADING_JUNK: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)^\s*(?:[&,\-/|:;]+|and\b)\s*").expect("regex"));
static DIAL_ONLY: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)^\s*(?:(?:FM|AM)?\s*\d{2,4}(?:\.\d)?\s*(?:FM|AM)?[\s&,\-/]*)+$")
        .expect("regex")
});
static LEADING_BARE_FREQ: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"^\s*\d{2,4}(?:\.\d)?\s+").expect("regex"));

fn py_trim<'a>(s: &'a str, chars: &str) -> &'a str {
    s.trim_matches(|c: char| chars.contains(c))
}

/// "NPR Syracuse University, NY (New" -> "NPR Syracuse University, NY".
pub fn drop_unclosed_parenthetical(name: &str) -> String {
    if name.matches('(').count() > name.matches(')').count() {
        if let Some(at) = name.rfind('(') {
            return name[..at].to_string();
        }
    }
    name.to_string()
}

/// "Birmingham s Beautiful QEZ" -> "Birmingham's Beautiful QEZ": a lone
/// lowercase s after a word of two or more letters, with more name to
/// follow, is the apostrophe the directory stripped. Words must already be
/// single-spaced.
pub fn restore_possessives(name: &str) -> String {
    let words: Vec<&str> = name.split(' ').collect();
    let mut out: Vec<String> = Vec::with_capacity(words.len());
    for (k, word) in words.iter().enumerate() {
        let follows = k + 1 < words.len();
        if *word == "s" && follows {
            if let Some(prev) = out.last_mut() {
                let letters = prev
                    .chars()
                    .rev()
                    .take_while(|c| c.is_ascii_alphabetic())
                    .count();
                let before = prev.chars().rev().nth(letters);
                if letters >= 2 && !before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    prev.push_str("'s");
                    continue;
                }
            }
        }
        out.push((*word).to_string());
    }
    out.join(" ")
}

/// A local station name with stranded leading junk taken out; "" when
/// nothing but dial positions is left.
pub fn clean_local_name(raw: &str) -> String {
    let collapsed = SPACES.replace_all(raw, " ");
    let mut name = drop_unclosed_parenthetical(collapsed.trim());
    loop {
        let stripped = LEADING_JUNK.replace(&name, "").into_owned();
        if stripped == name {
            break;
        }
        name = stripped;
    }
    if DIAL_ONLY.is_match(&name) {
        return String::new();
    }
    let bare = LEADING_BARE_FREQ.replace(&name, "").into_owned();
    if bare != name && !bare.is_empty() {
        name = LEADING_JUNK.replace(&bare, "").into_owned();
    }
    restore_possessives(py_trim(name.trim(), " -|/,:;&"))
}

/// A directory station's name with the stream jargon taken out, then the
/// leading junk.
pub fn clean_terrestrial_name(raw: &str) -> String {
    let first = NAME_SPLIT.split(raw.trim()).next().unwrap_or("");
    let quiet = NOISE_WORDS.replace_all(first, " ");
    // "( )" and "[]": what is left of a bracket the jargon emptied.
    let quiet = EMPTY_BRACKETS.replace_all(&quiet, " ");
    let name = drop_unclosed_parenthetical(&quiet);
    let collapsed = SPACES.replace_all(&name, " ");
    let name = py_trim(collapsed.trim(), " -|/,:;");
    let local = clean_local_name(name);
    if local.is_empty() {
        name.to_string()
    } else {
        local
    }
}

// -- what a Radio Browser row says about itself --

/// A Canadian broadcast call sign written in capitals inside a name,
/// optionally with its band: "CHFI-FM 98.1", "Virgin Radio 99.9 CKFM".
/// Capitals only: brand words shaped like call signs ("Chat", "Cool") are
/// written in mixed case, and every hit is checked against ISED anyway.
static CALL_IN_NAME: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"\b(C[BFHIJKY][A-Z]{1,2}|VO[A-Z]{2})(?:[- ](FM|AM))?\b").expect("regex")
});
/// A CBC stream mount: `/CBLAFM_CBC`, `/CBU2FM_CBC_SC`, `/CBXAM_CBC`.
static CBC_MOUNT: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"/(C[A-Z]{1,3}|VO[A-Z]{2})\d*(FM|AM)\d*_CBC").expect("regex"));
static FM_FREQ: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?:^|[^\d.,])(8[89]|9\d|10[0-7])[.,](\d)(?:[^\d]|$)").expect("regex")
});
static AM_FREQ: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\b(?:(\d{3,4})\s?AM|AM\s?(\d{3,4}))\b").expect("regex"));
static BAND_WORD: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\b(FM|AM)\b").expect("regex"));

/// Every call sign written in `name`, in order, with the band written next
/// to it if any.
pub fn call_signs_in(name: &str) -> Vec<(String, Option<Band>)> {
    CALL_IN_NAME
        .captures_iter(name)
        .map(|c| {
            let band = c.get(2).map(|b| match b.as_str() {
                "AM" => Band::Am,
                _ => Band::Fm,
            });
            (c[1].to_string(), band)
        })
        .collect()
}

/// The call sign and band in a CBC stream mount, if the URL is one.
pub fn cbc_mount(url: &str) -> Option<(String, Band)> {
    let upper = url.to_uppercase();
    let c = CBC_MOUNT.captures(&upper)?;
    let band = if &c[2] == "AM" { Band::Am } else { Band::Fm };
    Some((c[1].to_string(), band))
}

/// The one dial position written in `name`, or `None` when there is none
/// or more than one.
pub fn frequency_in(name: &str) -> Option<(Band, f64)> {
    let mut found: Vec<(Band, f64)> = Vec::new();
    for c in FM_FREQ.captures_iter(name) {
        if let Ok(f) = format!("{}.{}", &c[1], &c[2]).parse::<f64>() {
            found.push((Band::Fm, f));
        }
    }
    for c in AM_FREQ.captures_iter(name) {
        let digits = c.get(1).or_else(|| c.get(2)).map_or("", |m| m.as_str());
        if let Ok(k) = digits.parse::<f64>() {
            if (530.0..=1710.0).contains(&k) {
                found.push((Band::Am, k));
            }
        }
    }
    found.dedup_by(|a, b| a.0 == b.0 && (a.1 - b.1).abs() < 0.01);
    match found.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

/// The band a name says in words ("FM", "AM"), when it says only one.
pub fn band_word(name: &str) -> Option<Band> {
    let mut bands: Vec<Band> = BAND_WORD
        .captures_iter(name)
        .map(|c| {
            if c[1].eq_ignore_ascii_case("AM") {
                Band::Am
            } else {
                Band::Fm
            }
        })
        .collect();
    bands.sort();
    bands.dedup();
    match bands.as_slice() {
        [one] => Some(*one),
        _ => None,
    }
}

fn fold_accents(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            'é' | 'è' | 'ê' | 'ë' | 'É' | 'È' => 'e',
            'à' | 'â' | 'À' => 'a',
            'î' | 'ï' => 'i',
            'ô' => 'o',
            'û' | 'ù' => 'u',
            'ç' => 'c',
            other => other,
        })
        .collect::<String>()
        .to_lowercase()
}

/// The province a Radio Browser row's free-text `state` names: a full name
/// ("Ontario", "Québec", "Manitoba, Winnipeg", "Yukon Territory") or a code
/// written as its own word ("Toronto, ON"). A city alone names none.
pub fn province_named(state: &str) -> Option<&'static str> {
    let folded = fold_accents(state);
    if let Some((code, _)) = PROVINCES.iter().find(|(_, name)| folded.contains(name)) {
        return Some(code);
    }
    let words: Vec<&str> = state
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|w| !w.is_empty())
        .collect();
    PROVINCES
        .iter()
        .find(|(code, _)| words.contains(code))
        .map(|(code, _)| *code)
}

/// The FM coverage radius in miles for a licensed power: the band table the
/// US tier and the site's `rangeFor` use. AM is a flat 25 miles.
pub fn range_for(band: Band, erp_kw: Option<f64>) -> f64 {
    const RANGE_BY_ERP_MI: [(f64, f64); 5] = [
        (0.1, 8.0),
        (1.0, 15.0),
        (6.0, 22.0),
        (25.0, 32.0),
        (50.0, 42.0),
    ];
    if band == Band::Am {
        return 25.0;
    }
    let Some(kw) = erp_kw else {
        return 22.0;
    };
    RANGE_BY_ERP_MI
        .iter()
        .find(|(ceiling, _)| kw < *ceiling)
        .map_or(55.0, |(_, miles)| *miles)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ised_positions_read_as_degrees_west_negative() {
        assert_eq!(dms("434000", false), Some(43.66667));
        assert_eq!(dms("0793000", true), Some(-79.5));
        assert_eq!(dms("1350300", true), Some(-135.05));
        assert_eq!(dms("", false), None);
        assert_eq!(dms("0", false), None);
    }

    #[test]
    fn names_say_their_call_sign_band_and_dial_position() {
        assert_eq!(
            call_signs_in("CHFI-FM 98.1 Toronto"),
            vec![("CHFI".to_string(), Some(Band::Fm))]
        );
        assert_eq!(
            call_signs_in("Virgin Radio 99.9 CKFM"),
            vec![("CKFM".to_string(), None)]
        );
        assert!(call_signs_in("Chat Radio").is_empty());
        assert_eq!(
            cbc_mount("https://26183.live.streamtheworld.com/CBLAFM2_CBC_SC"),
            Some(("CBLA".to_string(), Band::Fm))
        );
        assert_eq!(frequency_in("CKOI 96,9 Montréal"), Some((Band::Fm, 96.9)));
        assert_eq!(frequency_in("AM 1010 CFRB"), Some((Band::Am, 1010.0)));
        assert_eq!(frequency_in("Newstalk 1010"), None);
        assert_eq!(frequency_in("98.1 and 101.3"), None);
        assert_eq!(frequency_in("Radio 2000"), None);
    }

    #[test]
    fn a_state_field_names_a_province_or_none() {
        assert_eq!(province_named("Québec"), Some("QC"));
        assert_eq!(province_named(" Ontario"), Some("ON"));
        assert_eq!(province_named("Toronto, ON"), Some("ON"));
        assert_eq!(province_named("Newfoundland and Labrador"), Some("NL"));
        assert_eq!(province_named("Yukon Territory"), Some("YT"));
        assert_eq!(province_named("Toronto"), None);
        assert_eq!(province_named("Windsor CT"), None);
        assert_eq!(province_code("NF"), Some("NL"));
        assert_eq!(province_code("NY"), None);
    }

    #[test]
    fn names_are_cleaned_the_way_the_us_tier_was() {
        assert_eq!(clean_terrestrial_name("CJAD 800 | 128k AAC"), "CJAD 800");
        assert_eq!(
            clean_terrestrial_name("Hampton s Jazz Radio"),
            "Hampton's Jazz Radio"
        );
        assert_eq!(clean_terrestrial_name("98.5 Rouge (New"), "Rouge");
        assert_eq!(
            clean_terrestrial_name("CKJR \"Sports 1440\" Wetaskiwin, AB ( )"),
            "CKJR \"Sports 1440\" Wetaskiwin, AB"
        );
        assert_eq!(
            clean_terrestrial_name("Énergie 98,9 Québec"),
            "Énergie 98,9 Québec"
        );
    }

    #[test]
    fn coverage_follows_the_us_power_table() {
        assert_eq!(range_for(Band::Am, None), 25.0);
        assert_eq!(range_for(Band::Fm, Some(0.05)), 8.0);
        assert_eq!(range_for(Band::Fm, Some(100.0)), 55.0);
        assert_eq!(range_for(Band::Fm, None), 22.0);
    }
}
