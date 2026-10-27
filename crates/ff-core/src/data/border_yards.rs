//! Border transfer yards for Mexico freight (`border_yards.json`).
//!
//! A US long-haul driver does not drive into Mexico. Southbound freight is
//! dropped at a transfer yard on the US side of the border, and a transfer
//! carrier takes the trailer across; northbound freight is picked up at the
//! same kind of yard once a transfer carrier has brought it over. So the
//! yards are ordinary facilities of a US map city, and the Mexican cities the
//! freight is really bound for are text on the load and nothing else: never
//! a map city, a route or a destination.
//!
//! Everything a player hears about it -- the load card line, the yard drop,
//! the northbound paperwork wait -- lives in the data file, so a wording pass
//! is a data edit. The bridge hours are story only: nothing here times,
//! charges or crosses the bridge.

use std::path::Path;

use once_cell::sync::OnceCell;
use serde::Deserialize;
use serde_json::{json, Value};

use super::data_resources::{read_data_text, read_text_at};
use super::world_models::DataError;

/// The data file, relative to the data root.
pub const BORDER_YARDS_FILE: &str = "border_yards.json";

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BorderYards {
    /// The US map city whose facilities the yards join.
    pub city: String,
    pub area: AreaBox,
    pub yards: Vec<BorderYard>,
    pub consignees: Vec<Consignee>,
    pub bridge: Bridge,
    pub paperwork_wait_minutes: WaitRange,
    pub text: BorderText,
}

/// The lat/lon box every yard must sit in.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct AreaBox {
    pub min_lat: f64,
    pub max_lat: f64,
    pub min_lon: f64,
    pub max_lon: f64,
}

impl AreaBox {
    pub fn contains(&self, lat: f64, lon: f64) -> bool {
        (self.min_lat..=self.max_lat).contains(&lat) && (self.min_lon..=self.max_lon).contains(&lon)
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BorderYard {
    pub id: String,
    pub name: String,
    pub spoken: String,
    #[serde(rename = "type")]
    pub facility_type: String,
    pub address: String,
    pub lat: f64,
    pub lon: f64,
    pub ships: Vec<String>,
    pub receives: Vec<String>,
    pub source: String,
    pub source_note: String,
}

/// A Mexican city named as the final consignee of southbound freight.
/// `name` keeps the accents for a later switch; `spoken` is what is said.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Consignee {
    pub name: String,
    pub spoken: String,
    pub state: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Bridge {
    pub name: String,
    pub source: String,
    pub commercial_hours: BridgeHours,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BridgeHours {
    pub weekday: OpenHours,
    pub saturday: OpenHours,
    pub sunday: OpenHours,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct OpenHours {
    pub open_hour: u32,
    pub close_hour: u32,
    pub spoken: String,
}

impl OpenHours {
    pub fn is_open(&self, hour: u32) -> bool {
        hour >= self.open_hour && hour < self.close_hour
    }
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct WaitRange {
    pub min: u32,
    pub max: u32,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BorderText {
    pub load_card: String,
    pub yard_drop: String,
    pub northbound_wait: String,
    pub northbound_wait_after_hours: String,
    pub no_load_ready: String,
}

impl BridgeHours {
    /// The commercial hours for a weekday index, Monday = 0 .. Sunday = 6.
    pub fn for_weekday(&self, weekday: u32) -> &OpenHours {
        match weekday % 7 {
            5 => &self.saturday,
            6 => &self.sunday,
            _ => &self.weekday,
        }
    }
}

impl BorderYards {
    pub fn yard(&self, facility_id: &str) -> Option<&BorderYard> {
        self.yards.iter().find(|y| y.id == facility_id)
    }

    /// The yard pins in the raw shape `parse_location` reads, so a yard is
    /// built exactly like any other curated facility.
    pub fn location_values(&self) -> Vec<Value> {
        self.yards
            .iter()
            .map(|y| {
                json!({
                    "id": y.id,
                    "name": y.name,
                    "spoken": y.spoken,
                    "type": y.facility_type,
                    "lat": y.lat,
                    "lon": y.lon,
                    "ships": y.ships,
                    "receives": y.receives,
                    "cargo": y.ships.iter().chain(y.receives.iter()).fold(
                        Vec::<&String>::new(),
                        |mut acc, c| {
                            if !acc.contains(&c) {
                                acc.push(c);
                            }
                            acc
                        },
                    ),
                    "traits": ["border_transfer_yard"],
                    "source_note": y.source_note,
                })
            })
            .collect()
    }

    /// The consignee a load is bound for, picked from `key` (any stable
    /// text about the load) so it never changes between hearing the offer
    /// and dropping the trailer, and needs no save field.
    pub fn consignee_for(&self, key: &str) -> Option<&Consignee> {
        if self.consignees.is_empty() {
            return None;
        }
        let index = (fnv1a(key) % self.consignees.len() as u64) as usize;
        self.consignees.get(index)
    }

    /// A paperwork wait in whole minutes inside the data's range, from a
    /// stable key.
    pub fn paperwork_minutes(&self, key: &str) -> u32 {
        let range = &self.paperwork_wait_minutes;
        let span = range.max.saturating_sub(range.min) as u64 + 1;
        range.min + (fnv1a(key) % span) as u32
    }

    pub fn load_card_line(&self, yard: &BorderYard, consignee: &Consignee, city: &str) -> String {
        fill(
            &self.text.load_card,
            &[
                ("{consignee}", &consignee.spoken),
                ("{yard}", &yard.spoken),
                ("{city}", city),
            ],
        )
    }

    pub fn yard_drop_line(&self, yard: &BorderYard, consignee: &Consignee) -> String {
        fill(
            &self.text.yard_drop,
            &[("{consignee}", &consignee.spoken), ("{yard}", &yard.spoken)],
        )
    }

    /// The northbound paperwork line. Outside the bridge's commercial hours
    /// the story says so, quoting that day's hours.
    pub fn wait_line(&self, yard: &BorderYard, minutes: u32, weekday: u32, hour: u32) -> String {
        let hours = self.bridge.commercial_hours.for_weekday(weekday);
        let minutes = minutes.to_string();
        if hours.is_open(hour) {
            fill(
                &self.text.northbound_wait,
                &[("{yard}", &yard.spoken), ("{minutes}", &minutes)],
            )
        } else {
            fill(
                &self.text.northbound_wait_after_hours,
                &[
                    ("{yard}", &yard.spoken),
                    ("{minutes}", &minutes),
                    ("{hours}", &hours.spoken),
                ],
            )
        }
    }

    /// Every rule a load-time screen enforces. Errors name the record.
    pub fn validate(&self) -> Result<(), DataError> {
        let bad = |msg: String| Err(DataError::value(format!("{BORDER_YARDS_FILE}: {msg}")));
        if self.yards.is_empty() {
            return bad("no yards".to_string());
        }
        for yard in &self.yards {
            if !yard.source.starts_with("https://") {
                return bad(format!("yard {} has no source URL", yard.id));
            }
            if yard.source_note.trim().is_empty() || yard.address.trim().is_empty() {
                return bad(format!(
                    "yard {} needs an address and a source note",
                    yard.id
                ));
            }
            if !self.area.contains(yard.lat, yard.lon) {
                return bad(format!(
                    "yard {} sits outside the border city area",
                    yard.id
                ));
            }
        }
        if self.consignees.is_empty() {
            return bad("no consignees".to_string());
        }
        for consignee in &self.consignees {
            if consignee.source.trim().is_empty() {
                return bad(format!("consignee {} has no source", consignee.spoken));
            }
            if !consignee.spoken.is_ascii() {
                return bad(format!(
                    "consignee {} is not spoken in plain text",
                    consignee.spoken
                ));
            }
        }
        if !self.bridge.source.starts_with("https://") {
            return bad("bridge hours have no source URL".to_string());
        }
        let range = &self.paperwork_wait_minutes;
        if range.min == 0 || range.min > range.max {
            return bad("paperwork wait range is empty".to_string());
        }
        let text = &self.text;
        for line in [
            &text.load_card,
            &text.yard_drop,
            &text.northbound_wait,
            &text.northbound_wait_after_hours,
            &text.no_load_ready,
        ] {
            if line.trim().is_empty() || !line.is_ascii() {
                return bad(format!("spoken line is empty or not plain text: {line:?}"));
            }
        }
        Ok(())
    }
}

fn fill(template: &str, pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .fold(template.to_string(), |acc, (k, v)| acc.replace(k, v))
}

/// FNV-1a: a stable 64-bit hash, so a pick never depends on the platform or
/// the standard library's randomised hasher.
fn fnv1a(text: &str) -> u64 {
    text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

pub fn parse_border_yards(text: &str) -> Result<BorderYards, DataError> {
    let data: BorderYards = serde_json::from_str(text)
        .map_err(|e| DataError::value(format!("{BORDER_YARDS_FILE}: {e}")))?;
    data.validate()?;
    Ok(data)
}

/// The yards under a specific data root, for the world loader; `None` when
/// the file is absent (a trimmed test tree).
pub fn border_yards_at(data_dir: &Path) -> Result<Option<BorderYards>, DataError> {
    match read_text_at(&data_dir.join(BORDER_YARDS_FILE)) {
        Some(text) => parse_border_yards(&text).map(Some),
        None => Ok(None),
    }
}

static BORDER_YARDS: OnceCell<Option<BorderYards>> = OnceCell::new();

/// The yards under the game's data root, loaded once. An invalid file is
/// logged and treated as absent, so bad story data costs the yard lines,
/// not the game.
pub fn border_yards() -> Option<&'static BorderYards> {
    BORDER_YARDS
        .get_or_init(|| {
            let text = read_data_text(BORDER_YARDS_FILE)?;
            match parse_border_yards(&text) {
                Ok(data) => Some(data),
                Err(err) => {
                    log::warn!("{err}");
                    None
                }
            }
        })
        .as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_are_stable_and_in_range() {
        let data = border_yards().expect("border_yards.json loads");
        let a = data.consignee_for("general:dallas_tx_us:12.0");
        assert_eq!(a, data.consignee_for("general:dallas_tx_us:12.0"));
        for i in 0..200 {
            let m = data.paperwork_minutes(&format!("key{i}"));
            assert!((30..=90).contains(&m), "{m}");
        }
    }

    #[test]
    fn after_hours_line_quotes_that_days_hours() {
        let data = border_yards().expect("border_yards.json loads");
        let yard = &data.yards[0];
        let open = data.wait_line(yard, 45, 2, 10);
        assert!(!open.contains("closed"), "{open}");
        let sunday_night = data.wait_line(yard, 45, 6, 20);
        assert!(sunday_night.contains("8 AM to 4 PM"), "{sunday_night}");
        let saturday_evening = data.wait_line(yard, 45, 5, 17);
        assert!(
            saturday_evening.contains("normally closes at 4 PM on Saturdays"),
            "{saturday_evening}"
        );
        assert!(!saturday_evening.contains("today"), "{saturday_evening}");
        let weekday_dawn = data.wait_line(yard, 45, 1, 5);
        assert!(weekday_dawn.contains("7 AM to midnight"), "{weekday_dawn}");
    }
}
