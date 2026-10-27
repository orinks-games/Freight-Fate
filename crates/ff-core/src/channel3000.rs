//! Channel 3000 on 87.7: the owner's television programming as a radio
//! station, run on a time-of-day schedule.
//!
//! An FM radio can pick up analog TV channel 6's sound at 87.7, so the
//! station is a TV channel heard from the cab: daytime shows, prime-time
//! and late-night shows, overnight programming, and between them its own
//! glue -- idents, continuity announcers, commercials and short sketches.
//! The spec is `docs/superpowers/plans/2026-10-06-channel-3000.md`, and the
//! station brief (`docs/channel3000/STATION.md`) is where
//! "What plays when" comes from.
//!
//! This file is the headless half: the manifest (`data/channel3000.json`,
//! baked like the radio catalogs) and [`Schedule`], a pure function of a
//! seed, the airtime and the truck's local hour. The game crate plays
//! whatever [`Schedule::on_air`] names, as `c3k/<key>` from
//! `channel3000.pak` (see `assets_pack::streamed`).
//!
//! The rules, in the order the schedule applies them:
//!
//! - Dayparts follow the truck's LOCAL hour (`Trip::local_hour`): day 5 to
//!   19, prime 19 to 22, late 22 to 2, overnight 2 to 5, as the manifest
//!   says. The game clock runs at 10x and jumps when the driver sleeps, so
//!   the schedule is dayparts, never clock slots.
//! - A programme is never cut off. When one ends, the station looks at the
//!   hour: if the daypart changed since that programme began (or the
//!   station is just signing on), the break is the new daypart's ident and
//!   then its opener, and nothing else; otherwise it is an ident, then one,
//!   sometimes two, of a continuity line, one or two ads, or a short.
//! - Each clip is picked at the moment the one before it ends. If a break
//!   runs across a daypart boundary, the programme slot sees it and the
//!   new daypart's ident and opener go in first, so a programme is always
//!   introduced by its own daypart.
//! - Programmes come from the daypart's pool in a shuffled order with no
//!   repeat until the pool is used up, and never the same show (the key
//!   without its `_NN`) twice in a row while another show is available.
//!   That second rule wins when the two disagree: if the rest of a lap is
//!   only the show that just played, the next lap starts early.
//! - Glue avoids its last few picks the same way.
//! - Every draw is a crc32 of the seed and a counter, like the other
//!   stations' rotations (`radio_rotation`), so a seed pins a sequence.

#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::path::Path;

use once_cell::sync::Lazy;
use serde::Deserialize;

use crate::music::crc32;

/// The station's id in `radio_catalog.json`.
pub const CHANNEL_3000_ID: &str = "channel-3000";
/// The manifest, under the data root (and baked as `text:channel3000.json`).
pub const CHANNEL_3000_RESOURCE: &str = "channel3000.json";
/// The pack beside `music.pak` that carries every clip.
pub const CHANNEL_3000_PACK_NAME: &str = "channel3000.pak";
/// Every clip's asset key starts with this; `CombinedPack` routes it to
/// [`CHANNEL_3000_PACK_NAME`].
pub const CLIP_KEY_PREFIX: &str = "c3k/";

/// How many recent picks of its own kind a glue pick avoids, pool allowing.
const GLUE_MEMORY: usize = 4;
/// Glue picks remembered across every kind, enough to hold [`GLUE_MEMORY`]
/// of each.
const GLUE_HISTORY: usize = 32;
/// No real clip is this short; the floor only keeps a bad duration from
/// turning [`Schedule::advance`] into a loop that never ends.
const MIN_CLIP_S: f64 = 1.0;
/// A hard stop on clips ended in one advance: days of airtime.
const MAX_STEPS_PER_ADVANCE: usize = 20_000;

/// The asset key a clip plays under: `c3k/<key>`.
pub fn clip_asset_key(key: &str) -> String {
    format!("{CLIP_KEY_PREFIX}{key}")
}

/// Where the audio engine finds a music-channel track: Channel 3000's clips
/// under their own prefix, everything else under `music/`.
pub fn music_asset_key(track: &str) -> String {
    if track.starts_with(CLIP_KEY_PREFIX) {
        track.to_string()
    } else {
        format!("music/{track}")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Daypart {
    Day,
    Prime,
    Late,
    Overnight,
}

impl Daypart {
    pub const ALL: [Daypart; 4] = [
        Daypart::Day,
        Daypart::Prime,
        Daypart::Late,
        Daypart::Overnight,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Daypart::Day => "day",
            Daypart::Prime => "prime",
            Daypart::Late => "late",
            Daypart::Overnight => "overnight",
        }
    }

    fn index(self) -> usize {
        self as usize
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClipKind {
    Programme,
    Ident,
    Continuity,
    Ad,
    Short,
}

/// One clip: what it is, when it may air, what Now playing calls it, and
/// how long it runs (measured from the encoded file).
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Clip {
    pub key: String,
    pub kind: ClipKind,
    pub dayparts: Vec<Daypart>,
    #[serde(default)]
    pub opener: bool,
    pub title: String,
    pub duration_s: f64,
}

impl Clip {
    pub fn airs_in(&self, part: Daypart) -> bool {
        self.dayparts.contains(&part)
    }

    /// The show a programme belongs to: its key without a trailing `_NN`
    /// (`day_weigh_in_02` is `day_weigh_in`). A key with no episode number
    /// is a show of its own.
    pub fn show(&self) -> &str {
        match self.key.rsplit_once('_') {
            Some((show, number))
                if !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()) =>
            {
                show
            }
            _ => &self.key,
        }
    }
}

/// The hours of each daypart, `[start, end)` on the local clock; an end
/// below the start runs across midnight.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DaypartHours {
    pub day: [u8; 2],
    pub prime: [u8; 2],
    pub late: [u8; 2],
    pub overnight: [u8; 2],
}

impl DaypartHours {
    fn span(&self, part: Daypart) -> [u8; 2] {
        match part {
            Daypart::Day => self.day,
            Daypart::Prime => self.prime,
            Daypart::Late => self.late,
            Daypart::Overnight => self.overnight,
        }
    }
}

/// `data/channel3000.json`: `clips.json` from the clips branch, as is.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Manifest {
    pub schema: u32,
    #[serde(default)]
    pub notes: String,
    pub frequency_mhz: f64,
    pub dayparts: DaypartHours,
    pub clips: Vec<Clip>,
}

impl Manifest {
    pub fn from_json(text: &str) -> Result<Self, String> {
        serde_json::from_str(text).map_err(|e| format!("{CHANNEL_3000_RESOURCE}: {e}"))
    }

    /// The daypart a local hour (0 to 24, fractional) falls in. Every hour
    /// belongs to one: a manifest whose spans left a gap would fail
    /// [`Manifest::problems`], and the gap reads as day rather than nothing.
    pub fn daypart_at(&self, hour: f64) -> Daypart {
        let hour = hour.rem_euclid(24.0);
        Daypart::ALL
            .into_iter()
            .find(|part| {
                let [start, end] = self.dayparts.span(*part);
                let (start, end) = (f64::from(start), f64::from(end));
                if start < end {
                    start <= hour && hour < end
                } else {
                    hour >= start || hour < end
                }
            })
            .unwrap_or(Daypart::Day)
    }

    /// Indices of the clips of `kind` that air in `part`, openers left out:
    /// an opener only ever opens its daypart.
    fn pool(&self, part: Daypart, kind: ClipKind) -> Vec<usize> {
        self.clips
            .iter()
            .enumerate()
            .filter(|(_, c)| c.kind == kind && c.airs_in(part) && !c.opener)
            .map(|(i, _)| i)
            .collect()
    }

    fn opener(&self, part: Daypart) -> Option<usize> {
        self.clips.iter().position(|c| c.opener && c.airs_in(part))
    }

    /// Everything wrong with the manifest, in plain words; empty when the
    /// station can run. The same rules `tools/build_channel3000.py` checks
    /// before it packs, so a hand edit to the committed copy is caught too.
    pub fn problems(&self) -> Vec<String> {
        let mut problems = Vec::new();
        if self.schema != 1 {
            problems.push(format!("schema is {}, expected 1", self.schema));
        }
        let mut seen = std::collections::HashSet::new();
        for clip in &self.clips {
            if !seen.insert(clip.key.as_str()) {
                problems.push(format!("clip {} is listed twice", clip.key));
            }
            if clip.duration_s.is_nan() || clip.duration_s <= 0.0 {
                problems.push(format!("clip {} has no duration", clip.key));
            }
            if clip.dayparts.is_empty() {
                problems.push(format!("clip {} has no dayparts", clip.key));
            }
            if clip.title.trim().is_empty() {
                problems.push(format!("clip {} has no title", clip.key));
            }
            if clip.opener && clip.kind != ClipKind::Continuity {
                problems.push(format!("clip {} opens but is not continuity", clip.key));
            }
        }
        for part in Daypart::ALL {
            let [start, end] = self.dayparts.span(part);
            if start >= 24 || end >= 24 || start == end {
                problems.push(format!("daypart {} has bad hours", part.name()));
            }
            for kind in [ClipKind::Programme, ClipKind::Ident] {
                if self.pool(part, kind).is_empty() {
                    problems.push(format!("daypart {} has no {kind:?}", part.name()));
                }
            }
            let openers = self
                .clips
                .iter()
                .filter(|c| c.opener && c.airs_in(part))
                .count();
            if openers != 1 {
                problems.push(format!("daypart {} has {openers} openers", part.name()));
            }
        }
        problems
    }
}

/// The manifest under `data_root`, or why not. A release with no loose tree
/// reads the baked copy through the same data-resource reader.
pub fn load_manifest(data_root: &Path) -> Result<Manifest, String> {
    let path = data_root.join(CHANNEL_3000_RESOURCE);
    let text = crate::data::data_resources::read_text_at(&path)
        .ok_or_else(|| format!("{} is missing", path.display()))?;
    let manifest = Manifest::from_json(&text)?;
    let problems = manifest.problems();
    if !problems.is_empty() {
        return Err(format!("{CHANNEL_3000_RESOURCE}: {}", problems.join("; ")));
    }
    Ok(manifest)
}

static DEFAULT_MANIFEST: Lazy<Option<Manifest>> = Lazy::new(|| {
    load_manifest(crate::data::data_resources::data_root())
        .map_err(|err| log::warn!("Channel 3000 is off the dial: {err}"))
        .ok()
});

/// The shipped manifest, loaded once; `None` (and a log line) when this
/// build has none or it is broken, which keeps the station off the dial.
pub fn default_manifest() -> Option<&'static Manifest> {
    DEFAULT_MANIFEST.as_ref()
}

/// What a break slot will hold once it comes up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Ident,
    Opener,
    Continuity,
    Ad,
    Short,
    Programme,
}

/// The order a daypart's programmes come out in, one lap at a time.
#[derive(Debug, Clone, Default)]
struct ProgrammeBag {
    /// Programmes still to play this lap, in play order.
    remaining: VecDeque<usize>,
    lap: u64,
}

/// One station's running order, from sign-on.
#[derive(Debug, Clone)]
pub struct Schedule {
    manifest: &'static Manifest,
    seed: String,
    on_air: Option<usize>,
    elapsed_s: f64,
    /// The rest of the break, then the programme it leads into.
    slots: VecDeque<Slot>,
    /// The daypart the break in `slots` was planned for.
    break_part: Daypart,
    /// The daypart when the last programme began; `None` before sign-on.
    programme_part: Option<Daypart>,
    bags: [ProgrammeBag; 4],
    last_show: Option<String>,
    recent_glue: VecDeque<usize>,
    draws: u64,
    /// Moves every time a new clip goes on the air.
    serial: u64,
}

impl Schedule {
    /// A station that has not signed on yet; the first [`Schedule::advance`]
    /// (or [`Schedule::start`]) puts its first clip on the air.
    pub fn new(manifest: &'static Manifest, seed: &str) -> Self {
        Self {
            manifest,
            seed: seed.to_string(),
            on_air: None,
            elapsed_s: 0.0,
            slots: VecDeque::new(),
            break_part: Daypart::Day,
            programme_part: None,
            bags: Default::default(),
            last_show: None,
            recent_glue: VecDeque::new(),
            draws: 0,
            serial: 0,
        }
    }

    pub fn manifest(&self) -> &'static Manifest {
        self.manifest
    }

    /// The clip on the air, once signed on.
    pub fn on_air(&self) -> Option<&'static Clip> {
        self.on_air.map(|i| &self.manifest.clips[i])
    }

    /// Seconds already gone from the clip on the air.
    pub fn elapsed_s(&self) -> f64 {
        self.elapsed_s
    }

    /// A number that changes whenever a new clip goes on the air, so the
    /// player can tell a clip change from the same clip running on.
    pub fn serial(&self) -> u64 {
        self.serial
    }

    /// Sign on at `local_hour` if the station has not already.
    pub fn start(&mut self, local_hour: f64) {
        if self.on_air.is_none() {
            self.next_clip(local_hour);
        }
    }

    /// Run the station `dt` seconds of airtime at `local_hour`. Every clip
    /// that ends inside the step is followed by the one the hour calls for.
    pub fn advance(&mut self, dt: f64, local_hour: f64) {
        self.start(local_hour);
        self.elapsed_s += dt.max(0.0);
        for _ in 0..MAX_STEPS_PER_ADVANCE {
            let Some(clip) = self.on_air() else {
                return;
            };
            let length = clip.duration_s.max(MIN_CLIP_S);
            if self.elapsed_s < length {
                return;
            }
            self.elapsed_s -= length;
            self.next_clip(local_hour);
        }
    }

    /// A uniform draw in `[0, 1)`: crc32 of the seed and a counter, so the
    /// whole running order is a function of the seed and the hours.
    fn draw(&mut self, what: &str) -> f64 {
        self.draws += 1;
        let value = crc32(format!("{}|{}|{what}", self.seed, self.draws).as_bytes());
        f64::from(value) / (f64::from(u32::MAX) + 1.0)
    }

    fn pick_index(&mut self, len: usize, what: &str) -> usize {
        ((self.draw(what) * len as f64) as usize).min(len.saturating_sub(1))
    }

    /// Put the next clip on the air.
    fn next_clip(&mut self, local_hour: f64) {
        let part = self.manifest.daypart_at(local_hour);
        if self.slots.is_empty() {
            self.plan_break(part);
        }
        if self.slots.front() == Some(&Slot::Programme) && part != self.break_part {
            // The break ran across a daypart boundary: the new daypart
            // introduces its own programme.
            self.slots.clear();
            self.plan_change(part);
        }
        let Some(slot) = self.slots.pop_front() else {
            return;
        };
        let clip = match slot {
            Slot::Programme => {
                self.programme_part = Some(self.break_part);
                self.next_programme(self.break_part)
            }
            Slot::Opener => self.manifest.opener(self.break_part),
            Slot::Ident => self.pick_glue(ClipKind::Ident),
            Slot::Continuity => self.pick_glue(ClipKind::Continuity),
            Slot::Ad => self.pick_glue(ClipKind::Ad),
            Slot::Short => self.pick_glue(ClipKind::Short),
        };
        // A slot with nothing to fill it (a manifest that failed its checks)
        // moves on rather than leaving dead air.
        match clip {
            Some(index) => {
                self.on_air = Some(index);
                self.serial += 1;
            }
            None if !self.slots.is_empty() => self.next_clip(local_hour),
            None => {}
        }
    }

    /// The break after a programme (or before the first one).
    fn plan_break(&mut self, part: Daypart) {
        if self.programme_part != Some(part) {
            self.plan_change(part);
            return;
        }
        self.break_part = part;
        self.slots.push_back(Slot::Ident);
        let mut kinds = vec![Slot::Continuity, Slot::Ad, Slot::Short];
        let count = if self.draw("break-length") < 0.3 {
            2
        } else {
            1
        };
        for _ in 0..count {
            // Continuity is the usual filler, ads next, the sketches rarest.
            let weights: Vec<f64> = kinds
                .iter()
                .map(|kind| match kind {
                    Slot::Continuity => 0.5,
                    Slot::Ad => 0.35,
                    _ => 0.15,
                })
                .collect();
            let total: f64 = weights.iter().sum();
            let mut roll = self.draw("break-kind") * total;
            let mut chosen = kinds.len() - 1;
            for (i, weight) in weights.iter().enumerate() {
                if roll < *weight {
                    chosen = i;
                    break;
                }
                roll -= weight;
            }
            let kind = kinds.remove(chosen);
            self.slots.push_back(kind);
            if kind == Slot::Ad && self.draw("ad-pair") < 0.4 {
                self.slots.push_back(Slot::Ad);
            }
        }
        self.slots.push_back(Slot::Programme);
    }

    /// A daypart begins: its ident, its opener, and its first programme.
    fn plan_change(&mut self, part: Daypart) {
        self.break_part = part;
        self.slots
            .extend([Slot::Ident, Slot::Opener, Slot::Programme]);
    }

    fn pick_glue(&mut self, kind: ClipKind) -> Option<usize> {
        let pool = self.manifest.pool(self.break_part, kind);
        if pool.is_empty() {
            return None;
        }
        // The last few of this kind, never the whole pool: a pool of two
        // simply alternates.
        let manifest = self.manifest;
        let avoid: Vec<usize> = self
            .recent_glue
            .iter()
            .rev()
            .copied()
            .filter(|i| manifest.clips[*i].kind == kind)
            .take(GLUE_MEMORY.min(pool.len() - 1))
            .collect();
        let candidates: Vec<usize> = pool.into_iter().filter(|i| !avoid.contains(i)).collect();
        let pick = candidates[self.pick_index(candidates.len(), "glue")];
        self.recent_glue.push_back(pick);
        while self.recent_glue.len() > GLUE_HISTORY {
            self.recent_glue.pop_front();
        }
        Some(pick)
    }

    /// One lap of `part`'s programmes, crc-shuffled by the seed and the lap,
    /// leaving out any already waiting in the bag.
    fn lap_order(&self, part: Daypart, lap: u64, skip: &VecDeque<usize>) -> Vec<usize> {
        let mut pool: Vec<usize> = self
            .manifest
            .pool(part, ClipKind::Programme)
            .into_iter()
            .filter(|i| !skip.contains(i))
            .collect();
        pool.sort_by_key(|i| {
            let key = &self.manifest.clips[*i].key;
            (
                crc32(format!("{}|{}|lap {lap}|{key}", self.seed, part.name()).as_bytes()),
                *i,
            )
        });
        pool
    }

    fn next_programme(&mut self, part: Daypart) -> Option<usize> {
        let slot = part.index();
        if self.bags[slot].remaining.is_empty() {
            self.refill(part);
        }
        let last_show = self.last_show.clone();
        let differs =
            |manifest: &Manifest, i: usize| last_show.as_deref() != Some(manifest.clips[i].show());
        let manifest = self.manifest;
        let mut found = self.bags[slot]
            .remaining
            .iter()
            .position(|i| differs(manifest, *i));
        if found.is_none() {
            // The rest of this lap is the show that just played. Another
            // show back to back matters more than finishing the lap first:
            // the next lap joins the end of this one, and a different show
            // comes from it.
            self.refill(part);
            found = self.bags[slot]
                .remaining
                .iter()
                .position(|i| differs(manifest, *i));
        }
        // Only one show in the whole daypart: it plays again.
        let index = self.bags[slot].remaining.remove(found.unwrap_or(0))?;
        self.last_show = Some(manifest.clips[index].show().to_string());
        Some(index)
    }

    fn refill(&mut self, part: Daypart) {
        let slot = part.index();
        let lap = self.bags[slot].lap;
        let order = self.lap_order(part, lap, &self.bags[slot].remaining);
        let bag = &mut self.bags[slot];
        bag.remaining.extend(order);
        bag.lap += 1;
    }
}
