//! The name a layer speaks for an OpenStreetMap element.
//!
//! A spoken name uses `name:en` when the element has one. Otherwise it is
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

/// The spoken name from an element's `name` and `name:en` tags.
pub fn spoken_name(name: &str, name_en: Option<&str>) -> String {
    if let Some(en) = name_en.map(str::trim).filter(|n| !n.is_empty()) {
        let en = latin_only(en);
        if !en.is_empty() {
            return en;
        }
    }
    latin_only(name.trim())
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
    fn the_english_name_wins_when_there_is_one() {
        assert_eq!(spoken_name("ᒪᐢᑿᒌᐢ", Some("Maskwacis")), "Maskwacis");
        assert_eq!(spoken_name("Maskwacis", Some("  ")), "Maskwacis");
        assert_eq!(spoken_name("ᒪᐢᑿᒌᐢ Maskwacis", None), "Maskwacis");
    }
}
