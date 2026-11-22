//! A state or province as a sentence names it: "the Northwest Territories",
//! "the District of Columbia", every other one bare ("Alberta", "Yukon").
//!
//! The article is part of the returned literal, so the translation
//! extractor sees "the Northwest Territories" whole and each catalog can
//! render the name with its own grammar. A caller puts the result in a
//! hole the catalog translates (`{name}`), never in a proper-name hole
//! (`{state}`), which would carry the English "the" into every language.

/// The name with its article where English uses one; `name` unchanged
/// otherwise. Takes the full name or the two-letter code.
pub fn with_article(name: &str) -> &str {
    match name {
        "Northwest Territories" | "NT" => "the Northwest Territories",
        "District of Columbia" | "DC" => "the District of Columbia",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::with_article;

    #[test]
    fn the_territories_and_the_district_take_the_article() {
        assert_eq!(
            with_article("Northwest Territories"),
            "the Northwest Territories"
        );
        assert_eq!(with_article("NT"), "the Northwest Territories");
        assert_eq!(
            with_article("District of Columbia"),
            "the District of Columbia"
        );
    }

    #[test]
    fn every_other_place_stays_bare() {
        for name in ["Yukon", "Alberta", "Washington", "British Columbia", "Ohio"] {
            assert_eq!(with_article(name), name);
        }
    }
}
