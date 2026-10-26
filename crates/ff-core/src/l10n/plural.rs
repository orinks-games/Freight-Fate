//! Plural forms inside a translation: `{count:milla|millas}`.
//!
//! The English source picks "1 mile" or "2 miles" in code, so each English
//! form reaches the catalog as its own line. Russian and Polish need three
//! forms where English has two, chosen by the number's last digits, so a
//! translation can pick a word by the number a placeholder holds: the forms
//! after the colon are listed in the order the rules below number them.
//!
//! The same file writes a captured number the way the language reads it, so
//! a German voice does not take the comma in "25,000" for a decimal point.

/// The CLDR plural category index of `n` in `language`, as the forms of a
/// `{name:...}` choice are ordered: 0 = one, 1 = few (or other), 2 = many.
///
/// - English, Spanish, German, Italian, Portuguese, Turkish: one (n = 1),
///   other.
/// - French: one (0 and 1), other.
/// - Russian: one (1, 21, 31...), few (2-4, 22-24...), many (the rest).
/// - Polish: one (1 only), few (2-4, 22-24...), many (the rest).
/// - Chinese, Japanese: one form.
///
/// A fraction takes the last form but one for Russian and Polish ("other"
/// is "few" there) and the plural for the two-form languages.
pub fn plural_index(language: &str, n: f64) -> usize {
    let integer = n.fract() == 0.0;
    let i = n.abs().trunc() as u64;
    let base = language.split(['_', '-']).next().unwrap_or(language);
    match base {
        "zh" | "ja" | "ko" | "vi" => 0,
        "fr" => usize::from(i > 1),
        "ru" | "uk" => {
            if !integer {
                return 1;
            }
            let (ten, hundred) = (i % 10, i % 100);
            if ten == 1 && hundred != 11 {
                0
            } else if (2..=4).contains(&ten) && !(12..=14).contains(&hundred) {
                1
            } else {
                2
            }
        }
        "pl" => {
            if !integer {
                return 1;
            }
            let (ten, hundred) = (i % 10, i % 100);
            if i == 1 {
                0
            } else if (2..=4).contains(&ten) && !(12..=14).contains(&hundred) {
                1
            } else {
                2
            }
        }
        _ => usize::from(!(integer && i == 1)),
    }
}

/// The number a captured placeholder holds: its first run of digits, with
/// thousands separators and a decimal point read as English writes them.
/// `None` when the capture holds no digits at all.
pub fn number_in(text: &str) -> Option<f64> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let mut digits = String::new();
    let mut seen_point = false;
    let bytes = &text.as_bytes()[start..];
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        let next_is_digit = bytes.get(i + 1).is_some_and(|b| b.is_ascii_digit());
        if c.is_ascii_digit() {
            digits.push(c);
        } else if c == ',' && next_is_digit && !seen_point {
            // A thousands separator: "1,250".
        } else if c == '.' && next_is_digit && !seen_point {
            seen_point = true;
            digits.push('.');
        } else {
            break;
        }
        i += 1;
    }
    let value: f64 = digits.parse().ok()?;
    if start > 0 && text[..start].ends_with('-') {
        Some(-value)
    } else {
        Some(value)
    }
}

/// Pick the form of a `a|b|c` choice for the number in `capture`. A choice
/// with fewer forms than the rule asks for falls back to its last form; a
/// capture with no number takes the last form too (it is the plural, and a
/// plural reads right for "several" and "many").
pub fn choose<'a>(language: &str, forms: &'a [String], capture: &str) -> &'a str {
    let Some(last) = forms.last() else {
        return "";
    };
    let index = match number_in(capture) {
        Some(n) => plural_index(language, n),
        None => forms.len() - 1,
    };
    forms.get(index).unwrap_or(last)
}

/// A capture that is only an English-written number ("25,000", "4.89",
/// "-3"), rewritten with `language`'s separators: "25.000" and "4,89" in
/// German, "25 000" in French. `None` when the capture is anything else
/// (a clock time, a range, words) or the language writes numbers as English
/// does.
pub fn localize_number(language: &str, text: &str) -> Option<String> {
    let base = language.split(['_', '-']).next().unwrap_or(language);
    let (thousands, decimal) = match base {
        "de" | "es" | "it" | "pt" | "tr" => (".", ","),
        "fr" | "ru" | "pl" => ("\u{a0}", ","),
        _ => return None,
    };
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = match unsigned.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (unsigned, None),
    };
    let groups: Vec<&str> = whole.split(',').collect();
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let well_formed = groups.iter().all(|g| digits(g))
        && groups[1..].iter().all(|g| g.len() == 3)
        && groups[0].len() <= if groups.len() > 1 { 3 } else { usize::MAX }
        && fraction.is_none_or(digits);
    if !well_formed || (groups.len() == 1 && fraction.is_none()) {
        return None;
    }
    let mut out = String::with_capacity(text.len() + 2);
    if text.starts_with('-') {
        out.push('-');
    }
    out.push_str(&groups.join(thousands));
    if let Some(fraction) = fraction {
        out.push_str(decimal);
        out.push_str(fraction);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::plural_index;

    #[test]
    fn vietnamese_uses_one_plural_form_for_every_count() {
        for count in [0.0, 1.0, 2.0, 5.0, 21.0, 1.5] {
            assert_eq!(plural_index("vi", count), 0, "count {count}");
        }
    }
}
