//! Environment Canada's present weather codes, decoded from the published
//! table rather than from memory.
//!
//! `present_weather.tsv` beside this file is section 6.5.7 of the SWOB-ML
//! Product User Guide, version 8.17: every code with its English
//! description, plus a phrase in Environment Canada's own condition words
//! that then goes through [`map_condition_canada`] like an observed
//! condition, so a code and the city page's text land on the same game
//! condition. The file's header says which codes have no phrase and why.
//! This table is Environment Canada's, not the WMO one: code 65 is "Light
//! rain (not freezing, continuous)" here and heavy rain in WMO 4677.
//!
//! [`map_condition_canada`]: crate::sim::real_weather_canada::map_condition_canada

use std::collections::HashMap;
use std::sync::OnceLock;

const TABLE: &str = include_str!("present_weather.tsv");

/// One row of the table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Code {
    pub code: u16,
    /// Environment Canada condition words for the code, or empty when the
    /// code is not weather at the station now.
    pub phrase: &'static str,
    /// The guide's English description.
    pub description: &'static str,
}

fn table() -> &'static HashMap<u16, Code> {
    static TABLE_ROWS: OnceLock<HashMap<u16, Code>> = OnceLock::new();
    TABLE_ROWS.get_or_init(|| {
        TABLE
            .lines()
            .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
            .filter_map(|line| {
                let mut cols = line.splitn(3, '\t');
                let code = cols.next()?.trim().parse().ok()?;
                let phrase = cols.next()?.trim();
                let description = cols.next()?.trim();
                Some((
                    code,
                    Code {
                        code,
                        phrase,
                        description,
                    },
                ))
            })
            .collect()
    })
}

/// The table's row for a code, None for a code the guide does not list.
pub fn lookup(code: u16) -> Option<Code> {
    table().get(&code).copied()
}

/// Every row, in code order.
pub fn all() -> Vec<Code> {
    let mut rows: Vec<Code> = table().values().copied().collect();
    rows.sort_by_key(|row| row.code);
    rows
}

/// The condition words for a code that is weather at the station now;
/// None for one that is not, or one the guide does not list.
pub fn phrase(code: u16) -> Option<&'static str> {
    lookup(code)
        .map(|row| row.phrase)
        .filter(|phrase| !phrase.is_empty())
}
