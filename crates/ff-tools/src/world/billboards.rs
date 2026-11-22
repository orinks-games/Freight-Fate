//! `bake-billboards`: place an approved sign sheet's billboards on the legs.
//!
//! A port of the deleted `tools/bake_billboards.py`, plus the checks the
//! sheets used to run by hand. A sheet (`data/spider/signsheets/*.md`) holds
//! one block per sign:
//!
//! ```text
//! ### <name>
//! - treatment: billboard | landmark | remove | skip
//! - leg: <from_slug> -> <to_slug>   (the direction the driver reads it)
//! - at_mi: <float>                   (miles from <from_slug>)
//! - spoken: <exact spoken text>
//! - facing: both                     (optional: heard from either side)
//! ```
//!
//! A billboard becomes a `billboard_sign` landmark and a landmark a
//! `highway_marker`, merged into the leg's landmarks (a same-name curated
//! record is replaced, so a re-bake is idempotent). A sheet names the leg the
//! way the driver reads the sign; a leg stored the other way round gets the
//! milepost mirrored and the record faces the sheet's direction.
//!
//! Every sign is checked before anything is written: the leg exists, the
//! milepost is on it, the text has no digits, a billboard stands where the
//! sign law lets one stand (`ff_core::data::billboard_law` and the leg's own
//! `billboard_bans`), and no other callout heard the same way is within the
//! landmark spacing, which would silence one of them. Any failure writes
//! nothing.

use ff_core::data::billboard_law::board_law;
use ff_core::sim::trip_models::LANDMARK_MIN_SPACING_MI;

use super::layers::{corridor_mut, leg_id, leg_miles, round, Run};
use crate::cli::{Args, Spec};
use crate::pyjson::{Object, Value};
use crate::util::{self, Result};

const USAGE: &str = "usage: bake-billboards --sheet FILE [--sheet FILE ...] [--write]";

/// One block of a sheet.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sign {
    pub name: String,
    pub treatment: String,
    pub leg: String,
    pub at_mi: String,
    pub spoken: String,
    pub facing: String,
}

/// Parse `### name` blocks with `- key: value` fields. A `## ` heading or a
/// `---` rule ends the block, so a sheet's notes never read as fields.
pub fn parse_sheet(text: &str) -> Vec<Sign> {
    let mut signs: Vec<Sign> = Vec::new();
    let mut open = false;
    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some(name) = line.strip_prefix("### ") {
            signs.push(Sign {
                name: name.trim().to_string(),
                ..Sign::default()
            });
            open = true;
        } else if line.starts_with("## ") || line.trim() == "---" {
            open = false;
        } else if open {
            let Some(rest) = line.trim_start().strip_prefix("- ") else {
                continue;
            };
            let Some((key, value)) = rest.split_once(':') else {
                continue;
            };
            let Some(sign) = signs.last_mut() else {
                continue;
            };
            let value = value.trim().to_string();
            match key.trim().to_lowercase().as_str() {
                "treatment" => sign.treatment = value.to_lowercase(),
                "leg" => sign.leg = value,
                "at_mi" => sign.at_mi = value,
                "spoken" => sign.spoken = value,
                "facing" => sign.facing = value.to_lowercase(),
                _ => {}
            }
        }
    }
    signs
}

fn category_for(treatment: &str) -> Option<&'static str> {
    match treatment {
        "billboard" => Some("billboard_sign"),
        "landmark" => Some("highway_marker"),
        _ => None,
    }
}

fn split_leg(sign: &Sign) -> Result<(String, String)> {
    sign.leg
        .split_once("->")
        .map(|(a, b)| (a.trim().to_string(), b.trim().to_string()))
        .ok_or_else(|| format!("{}: leg needs 'from_slug -> to_slug'", sign.name))
}

/// The jurisdiction name at a mile along the leg in its stored direction.
fn state_at(leg: &Value, mi: f64) -> Option<String> {
    let rows = leg.get("corridor")?.get("state_miles")?.as_array()?;
    let mut end = 0.0;
    let mut last = None;
    for row in rows {
        let name = row.get("state").map(Value::py_str)?;
        end += row.get("miles").and_then(Value::as_f64).unwrap_or(0.0);
        if mi < end {
            return Some(name);
        }
        last = Some(name);
    }
    last
}

/// Why a billboard may not stand at this stored-direction mile, if it may not.
fn law_refuses(leg: &Value, mi: f64) -> Option<String> {
    let bans = leg
        .get("corridor")
        .and_then(|c| c.get("billboard_bans"))
        .and_then(Value::as_array);
    for ban in bans.into_iter().flatten() {
        let from = ban.get("from_mi").and_then(Value::as_f64).unwrap_or(0.0);
        let to = ban.get("to_mi").and_then(Value::as_f64).unwrap_or(0.0);
        if from <= mi && mi < to {
            let name = ban.get("name").map(Value::py_str).unwrap_or_default();
            return Some(format!("inside the {name} billboard ban"));
        }
    }
    let state = state_at(leg, mi)?;
    (!board_law(&state).allows_placed()).then(|| format!("{state} bans billboards"))
}

/// The directions a landmark record is heard in: (forward, reverse).
fn heard(record: &Value) -> (bool, bool) {
    let dirs: Vec<String> = record
        .get("directions")
        .and_then(Value::as_array)
        .map(|a| a.iter().map(Value::py_str).collect())
        .unwrap_or_default();
    if dirs.is_empty() || dirs.iter().any(|d| d == "both") {
        return (true, true);
    }
    (
        dirs.iter().any(|d| d == "forward"),
        dirs.iter().any(|d| d == "reverse"),
    )
}

/// A built record, ready to merge.
struct Placed {
    leg_index: usize,
    record: Object,
}

fn build(sign: &Sign, legs: &[Value], errors: &mut Vec<String>) -> Option<Placed> {
    let category = category_for(&sign.treatment)?;
    let (from, to) = match split_leg(sign) {
        Ok(pair) => pair,
        Err(e) => {
            errors.push(e);
            return None;
        }
    };
    let found = legs
        .iter()
        .position(|l| leg_id(l) == format!("{from}:{to}"));
    let reversed = legs
        .iter()
        .position(|l| leg_id(l) == format!("{to}:{from}"));
    let Some(leg_index) = found.or(reversed) else {
        errors.push(format!("{}: no leg {from} <-> {to}", sign.name));
        return None;
    };
    let leg = &legs[leg_index];
    let miles = leg_miles(leg);
    let Ok(at) = sign.at_mi.parse::<f64>() else {
        errors.push(format!(
            "{}: at_mi {:?} is not a number",
            sign.name, sign.at_mi
        ));
        return None;
    };
    if !(0.0..=miles).contains(&at) {
        errors.push(format!(
            "{}: at_mi {at} is off a {miles}-mile leg",
            sign.name
        ));
        return None;
    }
    let spoken = sign.spoken.trim();
    if spoken.is_empty() {
        errors.push(format!("{}: no spoken text", sign.name));
        return None;
    }
    if spoken.chars().any(|c| c.is_ascii_digit()) {
        errors.push(format!(
            "{}: spoken text has a digit; spell it out",
            sign.name
        ));
    }
    let forward = found.is_some();
    let stored_at = round(if forward { at } else { miles - at }, 1);
    if category == "billboard_sign" {
        if let Some(why) = law_refuses(leg, stored_at) {
            errors.push(format!("{} on {}: {why}", sign.name, sign.leg));
        }
    }
    let direction = if sign.facing == "both" {
        "both"
    } else if forward {
        "forward"
    } else {
        "reverse"
    };
    let mut record = Object::new();
    record.insert("name", Value::Str(sign.name.clone()));
    record.insert("category", Value::Str(category.to_string()));
    record.insert("kind", Value::Str("point".to_string()));
    record.insert("at_mi", Value::Float(stored_at));
    record.insert("spoken", Value::Str(spoken.to_string()));
    record.insert(
        "directions",
        Value::Array(vec![Value::Str(direction.to_string())]),
    );
    Some(Placed { leg_index, record })
}

/// Callouts heard the same way within the landmark spacing of a new sign.
fn crowding(placed: &Placed, legs: &[Value], others: &[&Placed]) -> Vec<String> {
    let rec = Value::Object(placed.record.clone());
    let at = rec.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
    let name = rec.get("name").map(Value::py_str).unwrap_or_default();
    let (fwd, rev) = heard(&rec);
    let existing = legs[placed.leg_index]
        .get("corridor")
        .and_then(|c| c.get("landmarks"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let fresh = others
        .iter()
        .filter(|o| o.leg_index == placed.leg_index)
        .map(|o| Value::Object(o.record.clone()));
    existing
        .into_iter()
        .chain(fresh)
        .filter(|other| other.get("name").map(Value::py_str).as_deref() != Some(name.as_str()))
        .filter(|other| {
            let (f, r) = heard(other);
            (fwd && f) || (rev && r)
        })
        .filter_map(|other| {
            let other_at = other.get("at_mi").and_then(Value::as_f64)?;
            ((other_at - at).abs() < LANDMARK_MIN_SPACING_MI).then(|| {
                format!(
                    "{name} at {at} mile is {:.1} mile from {}",
                    (other_at - at).abs(),
                    other.get("name").map(Value::py_str).unwrap_or_default()
                )
            })
        })
        .collect()
}

fn merge(leg: &mut Value, record: Object) -> Result<()> {
    let corridor = corridor_mut(leg)?;
    if !matches!(corridor.get("landmarks"), Some(Value::Array(_))) {
        corridor.insert("landmarks", Value::Array(Vec::new()));
    }
    let Some(Value::Array(landmarks)) = corridor.get_mut("landmarks") else {
        return Err("landmarks is not a list".into());
    };
    let name = record.get("name").cloned();
    let category = record.get("category").cloned();
    landmarks
        .retain(|lm| !(lm.get("name") == name.as_ref() && lm.get("category") == category.as_ref()));
    landmarks.push(Value::Object(record));
    landmarks.sort_by(|a, b| {
        let at = |v: &Value| v.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
        at(a).total_cmp(&at(b))
    });
    Ok(())
}

fn remove(sign: &Sign, legs: &mut [Value]) -> Result<()> {
    let (from, to) = split_leg(sign)?;
    let wanted = [format!("{from}:{to}"), format!("{to}:{from}")];
    let leg = legs
        .iter_mut()
        .find(|l| wanted.contains(&leg_id(l)))
        .ok_or_else(|| format!("{}: no leg {from} <-> {to}", sign.name))?;
    let corridor = corridor_mut(leg)?;
    let Some(Value::Array(landmarks)) = corridor.get_mut("landmarks") else {
        return Err(format!("{}: no billboard to remove", sign.name));
    };
    let before = landmarks.len();
    landmarks.retain(|lm| {
        !(lm.get("name").map(Value::py_str).as_deref() == Some(sign.name.as_str())
            && lm.get("category").map(Value::py_str).as_deref() == Some("billboard_sign"))
    });
    if landmarks.len() == before {
        return Err(format!(
            "{}: no billboard by that name on {}",
            sign.name, sign.leg
        ));
    }
    Ok(())
}

pub fn main(argv: &[String]) -> Result<i32> {
    let args = Args::parse(
        argv,
        &Spec {
            values: &["sheet"],
            flags: &["write"],
        },
    )
    .map_err(|e| format!("{e}\n{USAGE}"))?;
    let sheets = args.all("sheet");
    if sheets.is_empty() {
        return Err(USAGE.into());
    }
    let mut signs = Vec::new();
    for sheet in sheets {
        signs.extend(parse_sheet(&util::read_text(std::path::Path::new(sheet))?));
    }
    let mut run = Run::load()?;
    let legs = run.legs_mut()?;
    let mut errors = Vec::new();
    let mut placed: Vec<Placed> = Vec::new();
    let mut skipped = 0;
    for sign in signs.iter().filter(|s| s.treatment != "remove") {
        match build(sign, legs, &mut errors) {
            Some(p) => placed.push(p),
            None => skipped += 1,
        }
    }
    for (i, p) in placed.iter().enumerate() {
        let others: Vec<&Placed> = placed
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, o)| o)
            .collect();
        errors.extend(crowding(p, legs, &others));
    }
    for p in &placed {
        let rec = Value::Object(p.record.clone());
        println!(
            "  {:14} {} @ {:>6} mi  {}",
            rec.get("category").map(Value::py_str).unwrap_or_default(),
            leg_id(&legs[p.leg_index]),
            rec.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0),
            rec.get("name").map(Value::py_str).unwrap_or_default()
        );
    }
    if !errors.is_empty() {
        for e in &errors {
            eprintln!("ERROR: {e}");
        }
        eprintln!("{} problems; nothing written", errors.len());
        return Ok(1);
    }
    for sign in signs.iter().filter(|s| s.treatment == "remove") {
        remove(sign, legs)?;
        println!("  removed        {}  {}", sign.leg, sign.name);
    }
    let count = placed.len();
    for p in placed {
        merge(&mut legs[p.leg_index], p.record)?;
    }
    println!("\n{count} signs placed, {skipped} skipped");
    run.finish(args.flag("write"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sheet_reads_its_blocks_and_ignores_its_notes() {
        let text = "# Sheet\n\nLaw: notes.\n\n## Liard River Hot Springs\n\nFacts - not: a field.\n\n\
                    ### Liard (northbound)\n- treatment: billboard\n- leg: a_bc_ca -> b_yt_ca\n\
                    - at_mi: 12.5\n- spoken: Billboard: Liard River Hot Springs.\n\n## Sources\n- leg: x -> y\n";
        let signs = parse_sheet(text);
        assert_eq!(signs.len(), 1);
        assert_eq!(signs[0].name, "Liard (northbound)");
        assert_eq!(signs[0].treatment, "billboard");
        assert_eq!(signs[0].leg, "a_bc_ca -> b_yt_ca");
        assert_eq!(signs[0].at_mi, "12.5");
        assert_eq!(signs[0].spoken, "Billboard: Liard River Hot Springs.");
    }

    fn leg(state_miles: &[(&str, f64)], bans: &[(f64, f64)]) -> Value {
        let rows = state_miles
            .iter()
            .map(|(s, m)| {
                let mut o = Object::new();
                o.insert("state", Value::Str((*s).into()));
                o.insert("miles", Value::Float(*m));
                Value::Object(o)
            })
            .collect();
        let bans = bans
            .iter()
            .map(|(a, b)| {
                let mut o = Object::new();
                o.insert("from_mi", Value::Float(*a));
                o.insert("to_mi", Value::Float(*b));
                o.insert("name", Value::Str("Muncho Lake Provincial Park".into()));
                Value::Object(o)
            })
            .collect();
        let mut corridor = Object::new();
        corridor.insert("state_miles", Value::Array(rows));
        corridor.insert("billboard_bans", Value::Array(bans));
        let mut l = Object::new();
        l.insert("corridor", Value::Object(corridor));
        Value::Object(l)
    }

    #[test]
    fn the_sign_law_refuses_a_board_where_none_may_stand() {
        let l = leg(
            &[("British Columbia", 100.0), ("Alberta", 50.0)],
            &[(20.0, 30.0)],
        );
        assert_eq!(law_refuses(&l, 10.0), None);
        assert!(law_refuses(&l, 25.0).unwrap().contains("Muncho Lake"));
        assert!(law_refuses(&l, 120.0).unwrap().contains("Alberta"));
        let yukon = leg(&[("Yukon", 80.0)], &[]);
        assert_eq!(
            law_refuses(&yukon, 40.0),
            None,
            "attractions only lets a placed sign stand"
        );
    }
}
