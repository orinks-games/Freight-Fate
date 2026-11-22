//! The name a layer speaks for an OpenStreetMap element.
//!
//! A spoken name is `name` as signed when a voice can read all of it. When it
//! holds other script, `name:en` is used where the element has one, else
//! `name` with every run of characters outside the Latin script taken out,
//! so "ᐸᐢᒁᐤ Pasqua First Nation" is said as "Pasqua First Nation". Accented
//! letters are Latin and stay ("Québec", "Thérèse"). When nothing readable is
//! left the caller gets an empty string and the place is not spoken: a voice
//! reads syllabics, Cyrillic or Han either as nothing or as a string of
//! "unknown character" names, which is worse than silence.
//!
//! IPA letters (U+0250 to U+02AF, the "ʔ" glottal stop in "ʔEsdilagh") and
//! spacing modifier letters count as not readable: Unicode files most of them
//! under the Latin script, but no game voice pronounces them.

/// True for a character a game voice reads as part of a Latin name: ASCII,
/// the Latin-1 and Latin Extended letters and punctuation, Latin Extended
/// Additional, combining accents, and general punctuation (dashes, curly
/// quotes).
pub fn is_readable(c: char) -> bool {
    matches!(c,
        '\u{0}'..='\u{7f}'
        | '\u{a0}'..='\u{24f}'
        | '\u{300}'..='\u{36f}'
        | '\u{1e00}'..='\u{1eff}'
        | '\u{2000}'..='\u{206f}')
}

/// `raw` with every run of unreadable characters removed, separators left
/// stranded at either end trimmed ("/", "-", ",", ";", ":"), empty
/// brackets dropped, and spaces collapsed. Empty when no letter is left.
pub fn latin_only(raw: &str) -> String {
    if raw.chars().all(is_readable) {
        return raw.to_string();
    }
    let kept: String = raw
        .chars()
        .map(|c| if is_readable(c) { c } else { ' ' })
        .collect();
    let joined = kept.split_whitespace().collect::<Vec<_>>().join(" ");
    let joined = joined.replace("( )", "").replace("()", "");
    let joined = joined.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = joined.trim_matches(|c: char| {
        matches!(
            c,
            '/' | '-' | ',' | ';' | ':' | '|' | ' ' | '\u{2013}' | '\u{2014}'
        )
    });
    let trimmed = trimmed.trim();
    if trimmed.chars().any(char::is_alphabetic) {
        trimmed.to_string()
    } else {
        String::new()
    }
}

/// The spoken name from an element's `name` and `name:en` tags: `name` as
/// signed when a voice can read all of it, so Quebec stays "Montréal" and
/// "Québec" (by ruling, 2026-11-22); `name:en` only when `name` holds script
/// a voice cannot read; else the readable part of `name`.
pub fn spoken_name(name: &str, name_en: Option<&str>) -> String {
    let name = name.trim();
    if name.chars().all(is_readable) {
        return stray_capitals_fixed(name);
    }
    if let Some(en) = name_en.map(str::trim).filter(|n| !n.is_empty()) {
        let en = latin_only(en);
        if !en.is_empty() {
            return stray_capitals_fixed(&en);
        }
    }
    stray_capitals_fixed(&latin_only(name))
}

/// Words that are spelled with two capitals on purpose.
const TWO_CAPITAL_WORDS: &[&str] = &["ONroute"];

/// `name` with a mapper's slipped shift key undone: a word of two capitals
/// and then only lower case ("JIm River", "PIcnic Area") keeps its first
/// capital alone. A voice reads "JIm" letter by letter. Brand spellings in
/// [`TWO_CAPITAL_WORDS`] stay.
pub fn stray_capitals_fixed(name: &str) -> String {
    name.split(' ')
        .map(|word| {
            let chars: Vec<char> = word.chars().collect();
            let slipped = chars.len() >= 3
                && chars[0].is_uppercase()
                && chars[1].is_uppercase()
                && chars[2..].iter().all(|c| c.is_lowercase())
                && !TWO_CAPITAL_WORDS.contains(&word);
            if slipped {
                let mut out = String::new();
                out.push(chars[0]);
                out.extend(chars[1].to_lowercase());
                out.extend(&chars[2..]);
                out
            } else {
                word.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn syllabics_are_taken_out_and_the_latin_name_stays() {
        assert_eq!(
            latin_only("ᐸᐢᒁᐤ Pasqua First Nation"),
            "Pasqua First Nation"
        );
        assert_eq!(
            latin_only("ᓃᐱᓰᐦᑯᐹᐏᔨᓃᓈᕽ Beardy's and Okemasis'"),
            "Beardy's and Okemasis'"
        );
        assert_eq!(latin_only("ᑳ ᐯᔭᒁᐢᑯᓇᒼ One Arrow"), "One Arrow");
        assert_eq!(latin_only("ʔEsdilagh"), "Esdilagh");
    }

    #[test]
    fn accented_latin_letters_stay() {
        assert_eq!(latin_only("Québec"), "Québec");
        assert_eq!(latin_only("Sainte-Thérèse"), "Sainte-Thérèse");
    }

    #[test]
    fn a_name_with_nothing_readable_is_not_spoken() {
        assert_eq!(latin_only("ᒪᐢᑿᒌᐢ"), "");
        assert_eq!(latin_only("北京 / 123"), "");
    }

    #[test]
    fn stranded_separators_and_empty_brackets_go() {
        assert_eq!(latin_only("Москва / Moscow"), "Moscow");
        assert_eq!(latin_only("Lake (ᓴᑲᐦᐃᑲᐣ)"), "Lake");
        assert_eq!(
            latin_only("I-95 Southbound ─ Johnston County"),
            "I-95 Southbound Johnston County"
        );
    }

    #[test]
    fn the_english_name_stands_in_for_unreadable_script() {
        assert_eq!(spoken_name("ᒪᐢᑿᒌᐢ", Some("Maskwacis")), "Maskwacis");
        assert_eq!(spoken_name("Maskwacis", Some("  ")), "Maskwacis");
        assert_eq!(spoken_name("ᒪᐢᑿᒌᐢ Maskwacis", None), "Maskwacis");
    }

    #[test]
    fn a_slipped_capital_is_undone_and_a_brand_kept() {
        assert_eq!(stray_capitals_fixed("JIm River"), "Jim River");
        assert_eq!(stray_capitals_fixed("PIcnic Area"), "Picnic Area");
        assert_eq!(
            stray_capitals_fixed("ONroute Tilbury North"),
            "ONroute Tilbury North"
        );
        assert_eq!(
            stray_capitals_fixed("US 2 and BC Hwy 3"),
            "US 2 and BC Hwy 3"
        );
        assert_eq!(spoken_name("JIm River", None), "Jim River");
    }

    #[test]
    fn a_readable_name_is_kept_as_signed_over_its_english_one() {
        assert_eq!(spoken_name("Montréal", Some("Montreal")), "Montréal");
        assert_eq!(spoken_name("Québec", Some("Quebec City")), "Québec");
    }
}
