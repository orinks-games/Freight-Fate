//! The dial with Music source Synthesized (owner rulings, 2026-09-21).
//! Freight Fate's own stations leave the dial; the Roadhouse stays, playing
//! synthesized music. With streamer-safe mode on as well, the Roadhouse is
//! the only station, and every station command does nothing and says
//! nothing rather than moving (owner ruling, 2026-09-21).
//! Its own file because `state` is already past the 1000-line limit.

use super::{dial_group, RadioAction, RadioState, RadioStation, SAFE_ROUTE_PLAYLIST};

impl RadioState {
    pub fn with_synth_music(mut self, synth_music: bool) -> Self {
        self.synth_music = synth_music;
        self
    }

    /// Whether the Music source keeps `station` on the dial. Original keeps
    /// everything, and so does Synthesized for the Roadhouse; otherwise
    /// Synthesized drops Freight Fate's own stations (dial group 1), and
    /// with streamer-safe mode on, everything else too.
    pub fn synth_allows(&self, station: &RadioStation) -> bool {
        if !self.synth_music || station.id == SAFE_ROUTE_PLAYLIST {
            return true;
        }
        !self.streamer_safe && dial_group(station) != 1
    }

    /// Synthesized with streamer-safe mode on: the radio stays on the
    /// Roadhouse, and station commands do nothing and say nothing.
    pub fn station_locked(&self) -> bool {
        self.synth_music && self.streamer_safe
    }

    /// A station command's answer while the dial is locked: nothing moves
    /// and nothing is said (empty `message`; callers must not speak it).
    pub(super) fn locked_action(&mut self) -> Option<RadioAction> {
        if !self.station_locked() {
            return None;
        }
        let reception = self.current_reception();
        Some(RadioAction {
            message: String::new(),
            station: reception.station.clone(),
            enabled: self.enabled,
            reception,
            fallback_used: false,
            retried: false,
        })
    }

    /// In Synthesized mode a station that leaves the dial lands on the
    /// Roadhouse, the one station every Synthesized dial keeps.
    pub(super) fn synth_fallback(&self) -> Option<RadioStation> {
        if !self.synth_music {
            return None;
        }
        self.catalog
            .iter()
            .find(|s| s.id == SAFE_ROUTE_PLAYLIST && self.station_allowed(s))
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;

    const DALLAS: (f64, f64) = (32.7767, -96.7970);
    const NIGHT_LINE: &str = "ff-night-line";

    fn radio_with(synth_music: bool, streamer_safe: bool) -> RadioState {
        RadioState::new(default_radio_catalog().to_vec())
            .with_position(Some(DALLAS))
            .with_streamer_safe(streamer_safe)
            .with_synth_music(synth_music)
    }

    fn groups(radio: &RadioState) -> Vec<i32> {
        radio
            .receivable_stations()
            .iter()
            .map(|r| dial_group(&r.station))
            .collect()
    }

    #[test]
    fn synthesized_streamer_safe_allows_only_the_roadhouse() {
        let radio = radio_with(true, true);
        let allowed: Vec<String> = radio
            .receivable_stations()
            .into_iter()
            .filter(|r| !r.fallback)
            .map(|r| r.station.id)
            .collect();
        assert_eq!(allowed, vec![SAFE_ROUTE_PLAYLIST.to_string()]);
        let (hits, _) = radio.search("night", 40);
        assert!(hits.is_empty(), "{hits:?}");
    }

    #[test]
    fn synthesized_dial_drops_freight_fate_stations_but_keeps_the_rest() {
        let radio = radio_with(true, false);
        let groups = groups(&radio);
        assert!(
            !groups.contains(&1),
            "a Freight Fate station is still on the dial"
        );
        assert!(groups.contains(&0), "the Roadhouse is gone");
        assert!(groups.contains(&TERRESTRIAL_GROUP), "{groups:?}");
        assert!(groups.contains(&5), "AFN is gone: {groups:?}");
        assert!(groups.contains(&9), "web radio is gone: {groups:?}");
    }

    #[test]
    fn a_freight_fate_station_left_on_the_dial_lands_on_the_roadhouse() {
        for streamer_safe in [false, true] {
            let mut radio = radio_with(true, streamer_safe).with_station_id(NIGHT_LINE);
            assert_eq!(radio.current_station().id, SAFE_ROUTE_PLAYLIST);
        }
    }

    #[test]
    fn original_mode_dial_is_unchanged() {
        assert_eq!(dial_group(&station_of(NIGHT_LINE)), 1);
        for streamer_safe in [false, true] {
            let plain = RadioState::new(default_radio_catalog().to_vec())
                .with_position(Some(DALLAS))
                .with_streamer_safe(streamer_safe);
            let original = radio_with(false, streamer_safe);
            let ids = |r: &RadioState| -> Vec<String> {
                r.receivable_stations()
                    .into_iter()
                    .map(|r| r.station.id)
                    .collect()
            };
            assert_eq!(ids(&plain), ids(&original));
            // Freight Fate's own stations are safe fiction: on the dial in
            // Original with streamer-safe mode either way.
            assert!(groups(&original).contains(&1));
            let mut radio = original.with_station_id(NIGHT_LINE);
            assert_eq!(radio.current_station().id, NIGHT_LINE);
        }
    }

    fn station_of(id: &str) -> RadioStation {
        default_radio_catalog()
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .unwrap_or_else(|| panic!("{id} not in the catalog"))
    }

    #[test]
    fn synthesized_streamer_safe_leaves_every_station_command_silent() {
        let mut radio = radio_with(true, true);
        let before = radio.current_station().id;
        assert_eq!(before, SAFE_ROUTE_PLAYLIST);
        let actions = [
            radio.tune(1, None),
            radio.tune(-1, None),
            radio.tune_category(1, None),
            radio.tune_category(-1, None),
            radio.select_station(NIGHT_LINE, None),
            radio.select_station("afn-tokyo", None),
        ];
        for action in actions {
            assert_eq!(action.message, "");
            assert_eq!(action.station.id, before);
        }
        assert_eq!(radio.toggle_favorite(), "");
        assert!(radio.favorite_ids.is_empty());
        assert_eq!(radio.current_station().id, before);
        // Power still works, and the game can still put the radio on the
        // Roadhouse itself.
        assert_eq!(radio.toggle(None).message, "Radio off.");
        assert!(radio.toggle(None).message.starts_with("Radio on."));
        assert!(radio
            .select_station(SAFE_ROUTE_PLAYLIST, None)
            .message
            .starts_with("Selected"));
    }

    #[test]
    fn the_synthesized_category_key_skips_freight_fate_stations() {
        let mut radio = radio_with(true, false);
        // Distinct groups, not stations: one entry per station made this
        // loop thousands of category steps long, four minutes of CI alone.
        let mut categories = groups(&radio);
        categories.sort_unstable();
        categories.dedup();
        let categories = categories.len();
        for _ in 0..categories * 2 {
            let action = radio.tune_category(1, None);
            assert_ne!(dial_group(&action.station), 1, "{}", action.message);
        }
        for _ in 0..40 {
            let action = radio.tune(1, None);
            assert_ne!(dial_group(&action.station), 1, "{}", action.message);
        }
    }
}
