//! Interchange records: the fresh read written in the world source's shape,
//! merged over what the leg carried before.

use std::collections::HashSet;
use std::path::PathBuf;

use ff_core::pyfmt::round_py_n;

use super::discover::{self, signs, space_out, ExitNodes, Snapper, MIN_EXIT_SPACING_MI};
use super::ramps::{self, ramp_facts, ControlRead, Direction, RampCounts, RampFacts};
use super::topo::Topo;
use crate::pyjson::{Object, Value};

const OSM: &str = "https://www.openstreetmap.org/";

/// The source notes, worded once per run.
pub struct Sources {
    pub exit: String,
    pub control_terminal: String,
    pub control_wide: String,
    pub control_none: String,
    pub far_end: String,
    pub advisory: String,
    pub length: String,
    pub terminal: String,
    pub snapshot: String,
    pub date: String,
}

impl Sources {
    pub fn new(snapshot: &str, date: &str) -> Self {
        let from = format!("{snapshot} of {date}");
        Self {
            exit: format!(
                "Exit ref, name and destination signs read from OpenStreetMap \
                 highway=motorway_junction nodes on a motorway or trunk way running along the \
                 leg's archived route, and the destination tags of this exit's own \
                 motorway_link ramps, {from}. at_mi derived: the mean of the exit's junction \
                 nodes (one per carriageway) projected onto that route, rescaled to the leg's \
                 miles: {OSM}"
            ),
            control_terminal: format!(
                "OpenStreetMap highway=traffic_signals/highway=stop/highway=give_way node within \
                 {:.0} m of the walked terminal node where this exit's ramp chain ends (read; \
                 the walk supplies the node, the tag supplies the control), {from}: {OSM}",
                ramps::TERMINAL_CONTROL_M
            ),
            control_wide: format!(
                "OpenStreetMap highway=traffic_signals/highway=stop node on a motorway_link way \
                 within {:.0} m of this exit's junctions, read from the {from}; no control \
                 stood at a walked terminal: {OSM}",
                ramps::WIDE_CONTROL_M
            ),
            control_none: format!(
                "derived from ramp_far_end=motorway: every exit-ramp chain merges onto another \
                 motorway, so nothing stops traffic at the far end. Free flow is inferred from \
                 read OSM link topology, not from a tag -- OSM does not tag the absence of a \
                 control. {from}"
            ),
            far_end: format!(
                "derived: this exit's motorway_link chains walked from every gore within {:.0} m \
                 of its junction nodes, both carriageways, in the {from}. 'motorway' means \
                 every chain merges onto a highway=motorway way; 'surface' means at least one \
                 chain ends off the motorway network: {OSM}",
                ramps::FAR_END_M
            ),
            advisory: format!(
                "OpenStreetMap maxspeed:advisory on the motorway_link ways of this exit's ramp, \
                 per direction of travel (read; a directional suffix resolved from the way's \
                 drawn direction), the lowest on any branch of the ramp, {from}: {OSM}"
            ),
            length: format!(
                "derived from OpenStreetMap geometry: summed node-to-node distance along this \
                 exit's motorway_link way(s), from the gore node where the ramp leaves the \
                 motorway to the nearest node where it ends (a public road -- trunk to \
                 residential, not a service or private way -- or a dead end, else a motorway \
                 merge). Per direction of travel: the gore nearest the exit's junction nodes \
                 whose first ramp edge runs that way. Starts at the OSM diverge, at or near the \
                 gore, so any deceleration lane before it is not included. Screened: under \
                 {:.0} ft or over 1.5 mi dropped, not clamped. {from}: {OSM}",
                ramps::RAMP_LENGTH_MIN_FT
            ),
            terminal: format!(
                "read from OpenStreetMap topology: the node where the same walk that measures \
                 ramp_length ends (the public crossroad or dead end the motorway_link way \
                 reaches), per direction of travel. Only baked beside a length that passed its \
                 screen, and never for a ramp that ends in a merge. {from}: {OSM}"
            ),
            snapshot: snapshot.to_string(),
            date: date.to_string(),
        }
    }
}

/// The fields of each ramp fact, and the source prefixes this tool and the
/// Python bakes it replaces wrote. A fact whose source starts otherwise was
/// set by hand.
pub const GROUPS: &[(&[&str], &str, &[&str])] = &[
    (
        &["ramp_control"],
        "ramp_control_source",
        &[
            "OpenStreetMap highway=traffic_signals",
            "derived from ramp_far_end=motorway",
        ],
    ),
    (
        &["ramp_far_end"],
        "ramp_far_end_source",
        &["derived: this exit's motorway_link chains"],
    ),
    (
        &["ramp_advisory_forward", "ramp_advisory_backward"],
        "ramp_advisory_source",
        &["OpenStreetMap maxspeed:advisory"],
    ),
    (
        &[
            "ramp_length_ft_backward",
            "ramp_terminal_backward",
            "ramp_length_ft_forward",
            "ramp_terminal_forward",
            "ramp_terminal_source",
        ],
        "ramp_length_source",
        &["derived from OpenStreetMap geometry"],
    ),
];

/// Exit sources this tool and the Python wrote.
pub const EXIT_SOURCES: &[&str] = &[
    "OpenStreetMap highway=motorway_junction",
    "Exit ref, name and destination signs read from OpenStreetMap",
];

/// The record's key order, as the Python wrote it.
const KEY_ORDER: &[&str] = &[
    "at_mi",
    "exit_ref",
    "name",
    "destinations",
    "via",
    "highway",
    "source",
    "ramp_control",
    "ramp_control_source",
    "ramp_far_end",
    "ramp_far_end_source",
    "ramp_advisory_forward",
    "ramp_advisory_backward",
    "ramp_advisory_source",
    "ramp_length_ft_backward",
    "ramp_terminal_backward",
    "ramp_length_ft_forward",
    "ramp_terminal_forward",
    "ramp_length_source",
    "ramp_terminal_source",
];

fn str_of(o: &Object, key: &str) -> String {
    o.get(key).map(Value::py_str).unwrap_or_default()
}

fn ours(source: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|p| source.starts_with(p))
}

/// An exit read this run.
pub struct Fresh {
    pub at_mi: f64,
    pub record: Object,
    /// Terminal nodes by direction: backward, forward.
    pub terminals: [Option<i64>; 2],
}

fn direction_fields(o: &mut Object, dir: &str, d: &Direction) {
    if let Some(ft) = d.length_ft {
        o.insert(
            format!("ramp_length_ft_{dir}"),
            Value::Float(round_py_n(ft, 1)),
        );
    }
    if let Some((node, lat, lon)) = d.terminal {
        o.insert(
            format!("ramp_terminal_{dir}"),
            Value::Object(crate::pyobj! {
                "node" => Value::Int(i128::from(node)),
                "lat" => round_py_n(lat, 7),
                "lon" => round_py_n(lon, 7),
            }),
        );
    }
}

fn to_record(
    exit: &ExitNodes,
    dests: Vec<String>,
    via: String,
    highway: &str,
    facts: &RampFacts,
    s: &Sources,
) -> Object {
    let mut o = crate::pyobj! {
        "at_mi" => round_py_n(exit.at_mi, 1),
        "exit_ref" => exit.exit_ref.as_str(),
        "name" => exit.name.as_str(),
        "destinations" => Value::Array(dests.into_iter().map(Value::Str).collect()),
        "via" => via,
        "highway" => highway,
        "source" => s.exit.as_str(),
    };
    if let Some((kind, read)) = facts.control {
        o.insert("ramp_control", Value::Str(kind.to_string()));
        let src = match read {
            ControlRead::Terminal => &s.control_terminal,
            ControlRead::Wide => &s.control_wide,
            ControlRead::Merge => &s.control_none,
        };
        o.insert("ramp_control_source", Value::Str(src.clone()));
    }
    if let Some(end) = facts.far_end {
        o.insert("ramp_far_end", Value::Str(end.to_string()));
        o.insert("ramp_far_end_source", Value::Str(s.far_end.clone()));
    }
    let adv = [
        ("forward", facts.forward.advisory_mph),
        ("backward", facts.backward.advisory_mph),
    ];
    for (dir, mph) in adv {
        if let Some(mph) = mph {
            o.insert(
                format!("ramp_advisory_{dir}"),
                Value::Float(round_py_n(mph, 3)),
            );
        }
    }
    if adv.iter().any(|(_, m)| m.is_some()) {
        o.insert("ramp_advisory_source", Value::Str(s.advisory.clone()));
    }
    direction_fields(&mut o, "backward", &facts.backward);
    direction_fields(&mut o, "forward", &facts.forward);
    if facts.backward.length_ft.is_some() || facts.forward.length_ft.is_some() {
        o.insert("ramp_length_source", Value::Str(s.length.clone()));
    }
    if facts.backward.terminal.is_some() || facts.forward.terminal.is_some() {
        o.insert("ramp_terminal_source", Value::Str(s.terminal.clone()));
    }
    o
}

/// Read one leg's exits and ramps, spaced for speech.
pub fn read_leg(
    topo: &Topo,
    snapper: &Snapper,
    exits: Vec<ExitNodes>,
    highway: &str,
    sources: &Sources,
    counts: &mut RampCounts,
) -> Vec<Fresh> {
    let signed: Vec<(ExitNodes, Vec<String>, String)> = exits
        .into_iter()
        .map(|e| {
            let (dests, via) = signs(topo, &e);
            (e, dests, via)
        })
        .filter(|(e, d, _)| !(e.exit_ref.is_empty() && d.is_empty() && e.name.is_empty()))
        .collect();
    let kept = space_out(
        signed,
        |e| e.0.at_mi,
        |e| usize::from(!e.0.exit_ref.is_empty()) + e.1.len(),
    );
    kept.into_iter()
        .map(|(exit, dests, via)| {
            let facts = ramp_facts(topo, &exit, snapper, counts);
            let record = to_record(&exit, dests, via, highway, &facts, sources);
            Fresh {
                at_mi: round_py_n(exit.at_mi, 1),
                record,
                terminals: [
                    facts.backward.terminal.map(|t| t.0),
                    facts.forward.terminal.map(|t| t.0),
                ],
            }
        })
        .collect()
}

/// What the merge kept from the old data, summed over the run.
#[derive(Debug, Default, Clone, Copy)]
pub struct Carry {
    pub hand_set_facts: usize,
    pub chain_terminals_kept: usize,
    pub chain_terminals_lost: usize,
}

pub struct Merged {
    pub records: Vec<Value>,
    pub matched: usize,
    pub hand_set: usize,
    /// Each old record's mile, and the mile of the record that replaces it
    /// (None when it is gone).
    pub moved: Vec<(f64, Option<f64>)>,
}

fn bare_ref(o: &Object) -> String {
    str_of(o, "exit_ref").split_whitespace().collect()
}

fn terminal_node(o: &Object, dir: &str) -> Option<i64> {
    o.get(&format!("ramp_terminal_{dir}"))
        .and_then(|t| t.get("node"))
        .and_then(Value::as_i64)
}

/// Old records matched to fresh ones, one to one: the same exit number
/// within 5 miles (a re-derived mile can move that far on a leg rerouted
/// after its exits were found), or both unnumbered within a mile.
fn match_records(old: &[(usize, &Object, f64)], fresh: &[Fresh]) -> Vec<(usize, usize)> {
    let mut pairs: Vec<(f64, usize, usize)> = Vec::new();
    for (oi, o, mi) in old {
        let r = bare_ref(o);
        for (fi, f) in fresh.iter().enumerate() {
            let fr = bare_ref(&f.record);
            let gap = (mi - f.at_mi).abs();
            let ok = if r.is_empty() {
                fr.is_empty() && gap <= 1.0
            } else {
                r == fr && gap <= 5.0
            };
            if ok {
                pairs.push((gap, *oi, fi));
            }
        }
    }
    pairs.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)).then(a.2.cmp(&b.2)));
    let (mut used_o, mut used_f) = (HashSet::new(), HashSet::new());
    let mut out = Vec::new();
    for (_, oi, fi) in pairs {
        if used_o.contains(&oi) || used_f.contains(&fi) {
            continue;
        }
        used_o.insert(oi);
        used_f.insert(fi);
        out.push((oi, fi));
    }
    out
}

fn ordered(o: Object) -> Object {
    let mut out = Object::new();
    for key in KEY_ORDER {
        if let Some(v) = o.get(key) {
            out.insert(*key, v.clone());
        }
    }
    for (k, v) in o.iter() {
        if !KEY_ORDER.contains(&k.as_str()) {
            out.insert(k.clone(), v.clone());
        }
    }
    out
}

/// Merge a leg's fresh read over its old records (see the module docs for
/// what is kept).
pub fn merge(
    old: &[Value],
    fresh: Vec<Fresh>,
    chain_terminals: &HashSet<i64>,
    sources: &Sources,
    carry: &mut Carry,
) -> Merged {
    let mut hand: Vec<Object> = Vec::new();
    let mut read: Vec<(usize, &Object, f64)> = Vec::new();
    for (i, v) in old.iter().enumerate() {
        let Some(o) = v.as_object() else {
            continue;
        };
        let mi = o.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0);
        if ours(&str_of(o, "source"), EXIT_SOURCES) {
            read.push((i, o, mi));
        } else {
            hand.push(o.clone());
        }
    }
    let hand_miles: Vec<f64> = hand
        .iter()
        .filter_map(|o| o.get("at_mi").and_then(Value::as_f64))
        .collect();
    let fresh: Vec<Fresh> = fresh
        .into_iter()
        .filter(|f| {
            hand_miles
                .iter()
                .all(|m| (m - f.at_mi).abs() >= MIN_EXIT_SPACING_MI)
        })
        .collect();
    let pairs = match_records(&read, &fresh);
    let mut moved: Vec<(f64, Option<f64>)> = Vec::new();
    for (oi, o, mi) in &read {
        let to = pairs
            .iter()
            .find(|(p, _)| p == oi)
            .map(|(_, fi)| fresh[*fi].at_mi);
        if to.is_none() {
            for dir in ["backward", "forward"] {
                if terminal_node(o, dir).is_some_and(|n| chain_terminals.contains(&n)) {
                    carry.chain_terminals_lost += 1;
                }
            }
        }
        moved.push((*mi, to));
    }
    let mut records: Vec<(f64, Object)> = hand
        .iter()
        .map(|o| {
            (
                o.get("at_mi").and_then(Value::as_f64).unwrap_or(0.0),
                o.clone(),
            )
        })
        .collect();
    for (fi, f) in fresh.iter().enumerate() {
        let mut rec = f.record.clone();
        if let Some(&(oi, _)) = pairs.iter().find(|(_, p)| *p == fi) {
            let Some(o) = old[oi].as_object() else {
                continue;
            };
            // A ramp fact set by hand on the old record stays.
            for (fields, source_key, prefixes) in GROUPS {
                let src = str_of(o, source_key);
                if src.is_empty() || ours(&src, prefixes) {
                    continue;
                }
                for key in fields.iter().chain(std::iter::once(source_key)) {
                    rec.remove(key);
                    if let Some(v) = o.get(key) {
                        rec.insert(*key, v.clone());
                    }
                }
                carry.hand_set_facts += 1;
            }
            // A ramp end a street chain starts from stays where it was.
            for (di, dir) in ["backward", "forward"].iter().enumerate() {
                let Some(node) = terminal_node(o, dir) else {
                    continue;
                };
                if !chain_terminals.contains(&node) || f.terminals[di] == Some(node) {
                    continue;
                }
                if !ours(&str_of(o, "ramp_length_source"), GROUPS[3].2) {
                    continue; // already carried whole as hand-set
                }
                for key in [
                    format!("ramp_length_ft_{dir}"),
                    format!("ramp_terminal_{dir}"),
                ] {
                    rec.remove(&key);
                    if let Some(v) = o.get(&key) {
                        rec.insert(key, v.clone());
                    }
                }
                let note = format!(
                    " The {dir} ramp's length and terminal are kept from the previous bake, not \
                     the {} of {}: a baked street chain starts at that terminal.",
                    sources.snapshot, sources.date
                );
                for (key, base) in [
                    ("ramp_length_source", &sources.length),
                    ("ramp_terminal_source", &sources.terminal),
                ] {
                    let text = rec
                        .get(key)
                        .map(Value::py_str)
                        .unwrap_or_else(|| base.clone());
                    rec.insert(key, Value::Str(text + &note));
                }
                carry.chain_terminals_kept += 1;
            }
        }
        records.push((f.at_mi, rec));
    }
    records.sort_by(|a, b| a.0.total_cmp(&b.0));
    Merged {
        records: records
            .into_iter()
            .map(|(_, o)| Value::Object(ordered(o)))
            .collect(),
        matched: pairs.len(),
        hand_set: hand.len(),
        moved,
    }
}

/// The `ramp_length_bake` meta: coverage over every exit on the map, the
/// length distribution, and this run's screen.
pub fn layer_meta(
    world: &Object,
    s: &Sources,
    counts: &RampCounts,
    carry: &Carry,
    pbfs: &[PathBuf],
) -> Object {
    let exits: Vec<&Object> = world
        .get("legs")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|l| l.get("corridor")?.get("interchanges")?.as_array())
        .flatten()
        .filter_map(Value::as_object)
        .collect();
    let mut lengths: Vec<f64> = exits
        .iter()
        .flat_map(|o| ["ramp_length_ft_forward", "ramp_length_ft_backward"].map(|k| o.get(k)))
        .flatten()
        .filter_map(Value::as_f64)
        .collect();
    lengths.sort_by(f64::total_cmp);
    let covered = exits
        .iter()
        .filter(|o| o.contains_key("ramp_length_source"))
        .count();
    let both = exits
        .iter()
        .filter(|o| {
            o.contains_key("ramp_terminal_forward") && o.contains_key("ramp_terminal_backward")
        })
        .count();
    let terminals = exits
        .iter()
        .flat_map(|o| {
            ["ramp_terminal_forward", "ramp_terminal_backward"].map(|k| o.contains_key(k))
        })
        .filter(|b| *b)
        .count();
    let pct = |p: f64| -> Value {
        if lengths.is_empty() {
            Value::Null
        } else {
            let i = ((p / 100.0 * lengths.len() as f64) as usize).min(lengths.len() - 1);
            Value::Float(lengths[i])
        }
    };
    let ratio = |n: usize| {
        if exits.is_empty() {
            0.0
        } else {
            round_py_n(n as f64 / exits.len() as f64, 4)
        }
    };
    let names: Vec<Value> = pbfs
        .iter()
        .map(|p| {
            Value::Str(
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
            )
        })
        .collect();
    crate::pyobj! {
        "kind" => "derived",
        "measure" => s.length.as_str(),
        "starts_at" => "the OSM node where the motorway_link way leaves the motorway way: at or \
                        near the gore (painted-gore start to physical nose, by mapper). The \
                        deceleration lane or taper lies before it and is not included.",
        "ends_at" => "the nearest surface-road node or dead end along the link ways, else the \
                      merge onto another motorway",
        "tool" => "cargo tools osm-exits",
        "inputs" => crate::pyobj! {
            "extracts" => Value::Array(names),
            "snapshot" => s.snapshot.as_str(),
            "extract_date" => s.date.as_str(),
        },
        "exits" => exits.len(),
        "exits_with_length" => covered,
        "coverage_ratio" => ratio(covered),
        "exits_with_both_terminals" => both,
        "both_terminals_ratio" => ratio(both),
        "directional_terminals" => terminals,
        "directional_lengths" => lengths.len(),
        "percentiles_ft" => crate::pyobj! {
            "p5" => pct(5.0), "p25" => pct(25.0), "p50" => pct(50.0),
            "p75" => pct(75.0), "p95" => pct(95.0),
        },
        "screen" => crate::pyobj! {
            "min_ft" => ramps::RAMP_LENGTH_MIN_FT,
            "max_ft" => ramps::RAMP_LENGTH_MAX_FT,
            "gores_measured" => counts.gores_measured,
            "dropped_too_short" => counts.too_short,
            "dropped_too_long" => counts.too_long,
            "no_terminal_reached" => counts.unmeasured,
        },
        "far_end" => crate::pyobj! {
            "exits_read" => counts.exits,
            "motorway" => counts.far_motorway,
            "surface" => counts.far_surface,
            "no_gore" => counts.no_gore,
            "control_none_derived" => counts.none_derived,
            "controls_read_at_terminal" => counts.terminal_reads,
            "controls_read_exit_wide" => counts.wide_reads,
        },
        "kept" => crate::pyobj! {
            "hand_set_facts" => carry.hand_set_facts,
            "chain_terminals_kept" => carry.chain_terminals_kept,
            "chain_terminals_lost" => carry.chain_terminals_lost,
        },
        "search_m" => crate::pyobj! {
            "junction_corridor" => discover::CORRIDOR_M,
            "gore" => ramps::FAR_END_M,
            "sign_gore" => discover::SIGN_GORE_M,
            "terminal_control" => ramps::TERMINAL_CONTROL_M,
            "wide_control" => ramps::WIDE_CONTROL_M,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pyjson;

    fn fresh(at: f64, exit_ref: &str, terminal: Option<i64>) -> Fresh {
        let s = Sources::new("test cut", "2026-10-19");
        let mut record = crate::pyobj! {
            "at_mi" => at, "exit_ref" => exit_ref, "name" => "",
            "destinations" => Value::Array(vec![]), "via" => "", "highway" => "I-20",
            "source" => s.exit.as_str(),
        };
        if let Some(n) = terminal {
            record.insert("ramp_length_ft_forward", Value::Float(900.0));
            record.insert(
                "ramp_terminal_forward",
                Value::Object(crate::pyobj! { "node" => Value::Int(i128::from(n)), "lat" => 30.0, "lon" => -97.0 }),
            );
            record.insert("ramp_length_source", Value::Str(s.length.clone()));
            record.insert("ramp_terminal_source", Value::Str(s.terminal.clone()));
        }
        Fresh {
            at_mi: at,
            record,
            terminals: [None, terminal],
        }
    }

    #[test]
    fn hand_set_records_and_facts_stay_and_chain_terminals_do_not_move() {
        let old = pyjson::loads(
            r#"[
              {"at_mi": 5.0, "exit_ref": "", "name": "Port Road", "destinations": [], "via": "",
               "highway": "I-20", "source": "Texas DOT exit list, checked by hand"},
              {"at_mi": 20.4, "exit_ref": "307", "name": "", "destinations": ["Baird"], "via": "",
               "highway": "I-20", "source": "OpenStreetMap highway=motorway_junction exit ref",
               "ramp_control": "stop", "ramp_control_source": "Seen on the owner's drive",
               "ramp_length_ft_forward": 1200.0,
               "ramp_terminal_forward": {"node": 77, "lat": 30.0, "lon": -97.0},
               "ramp_length_source": "derived from OpenStreetMap geometry: old",
               "ramp_terminal_source": "read from OpenStreetMap topology: old"},
              {"at_mi": 40.0, "exit_ref": "330", "name": "", "destinations": [], "via": "",
               "highway": "I-20", "source": "OpenStreetMap highway=motorway_junction exit ref"}
            ]"#,
        )
        .unwrap();
        let old = old.as_array().unwrap().clone();
        let s = Sources::new("test cut", "2026-10-19");
        let mut carry = Carry::default();
        let chains = HashSet::from([77]);
        // A fresh exit at 6.0 crowds the hand-set one and is dropped; 307
        // moves to 20.6 and reads terminal 78; 330 is gone.
        let merged = merge(
            &old,
            vec![fresh(6.0, "1", None), fresh(20.6, "307", Some(78))],
            &chains,
            &s,
            &mut carry,
        );
        assert_eq!(merged.records.len(), 2);
        assert_eq!(merged.hand_set, 1);
        let rec = merged.records[1].as_object().unwrap();
        assert_eq!(str_of(rec, "ramp_control"), "stop");
        assert_eq!(terminal_node(rec, "forward"), Some(77));
        assert!(str_of(rec, "ramp_terminal_source").contains("kept from the previous bake"));
        assert_eq!(carry.hand_set_facts, 1);
        assert_eq!(carry.chain_terminals_kept, 1);
        assert_eq!(merged.moved, vec![(20.4, Some(20.6)), (40.0, None)]);
    }
}
