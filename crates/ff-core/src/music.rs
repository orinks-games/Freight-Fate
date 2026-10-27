//! Music catalog and deterministic track selection.
//!
//! Port of `freight_fate/music.py`. Selection is seeded by `zlib.crc32` of
//! `"<seed key>|<track key>"` -- standard CRC-32 (IEEE), which is what
//! `flate2::Crc` computes -- and a stable sort, so a Rust and a Python build
//! draw the same playlist for the same trip. The catalog itself lives in
//! `music/tables.rs` (the track pools) and `music/pools.rs` (host breaks,
//! station maps, the duration index); this file is the selection logic.

mod expansion;
mod pools;
mod tables;

pub use expansion::NIGHT_LINE_VOCAL_TRACKS;
use pools::TRACKS_BY_KEY;
pub use pools::*;
pub use tables::*;

#[derive(Debug, Clone, PartialEq)]
pub struct MusicTrack {
    pub key: String,
    pub title: String,
    pub description: String,
    pub duration_s: f64,
}

impl MusicTrack {
    pub(crate) fn new(key: &str, title: &str, description: &str, duration_s: f64) -> Self {
        Self {
            key: key.to_string(),
            title: title.to_string(),
            description: description.to_string(),
            duration_s,
        }
    }
}

/// Python `zlib.crc32(data)`: standard CRC-32 (IEEE), which `flate2::Crc` is.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = flate2::Crc::new();
    crc.update(data);
    crc.sum()
}

/// `sorted(pool, key=lambda track: zlib.crc32(f"{seed_key}|{track.key}"))`:
/// the stable shuffle every selection below uses.
fn crc_ordered(pool: &[MusicTrack], seed_key: &str) -> Vec<MusicTrack> {
    let mut ordered: Vec<MusicTrack> = pool.to_vec();
    ordered.sort_by_key(|track| crc32(format!("{seed_key}|{}", track.key).as_bytes()));
    ordered
}

// sim.hos's day clock: DAWN_START, DAY_START, DUSK_START, NIGHT_START. Kept
// here as a private mirror so this module builds ahead of the `sim` port;
// `sim::hos::is_night` is the canonical one.
const DAWN_START: f64 = 5.0;
const NIGHT_START: f64 = 21.0;

/// `sim.hos.is_night`: the career clock reads night outside 05:00-21:00.
pub fn is_night(game_hours: f64) -> bool {
    let h = game_hours.rem_euclid(24.0);
    !(DAWN_START..NIGHT_START).contains(&h)
}

/// The slice of the loaded career the menu music reads.
///
/// The Python selectors took the `Profile` itself and read `game_hours`,
/// `career.level` / `deliveries` / `total_miles`, the visible owned trucks,
/// the active truck key, `name` and `current_city`; `models::profile::Profile`
/// implements this so a menu can hand its profile over unchanged.
pub trait MenuMusicProfile {
    /// The absolute career clock in hours.
    fn game_hours(&self) -> f64;
    fn level(&self) -> i64;
    fn deliveries(&self) -> i64;
    fn total_miles(&self) -> f64;
    /// `len(profile.visible_owned_trucks())`.
    fn owned_truck_count(&self) -> usize;
    /// `profile.active_truck_key()`; `"rig"` is the starter.
    fn active_truck_key(&self) -> String;
    fn name(&self) -> String;
    fn current_city(&self) -> String;
    /// `profile.business_status`: company driver, leased owner-operator or
    /// independent authority. The synthesized ladder splits on it.
    fn business_status(&self) -> String;
}

/// True when the loaded career's clock currently reads night.
///
/// Reads the absolute career clock, not the current city's local time: menu
/// music is chosen before the world data (and with it the city's time zone)
/// is loaded, and a bed picked up to three hours off local dusk is cosmetic.
fn profile_is_night(profile: Option<&dyn MenuMusicProfile>) -> bool {
    match profile {
        None => false,
        Some(profile) => is_night(profile.game_hours() % 24.0),
    }
}

/// The ambience under the main menu, picked by the career it would load the
/// way the music is: a truck stop lot after dark; by day, the yard where an
/// owner-operator's own truck waits, or the dispatch office a company driver
/// (or a new driver signing on) reports to.
pub fn select_menu_ambience(profile: Option<&dyn MenuMusicProfile>) -> &'static str {
    let [night_lot, yard, dispatch_office] = MENU_AMBIENCE;
    match profile {
        Some(_) if profile_is_night(profile) => night_lot,
        Some(p) if crate::models::business_constants::is_owner_operator(&p.business_status()) => {
            yard
        }
        _ => dispatch_office,
    }
}

/// Every bed [`select_menu_ambience`] can pick.
pub const MENU_AMBIENCE: [&str; 3] = [
    "ambient/menu_night_lot",
    "ambient/menu_yard",
    "ambient/menu_dispatch_office",
];

/// Choose a menu bed: the night theme after dark, else the milestone bed.
pub fn select_menu_music(profile: Option<&dyn MenuMusicProfile>) -> String {
    if profile_is_night(profile) {
        return MENU_NIGHT_TRACK.key.clone();
    }
    MENU_TRACKS[menu_milestone_index(profile)].key.clone()
}

pub fn menu_milestone_index(profile: Option<&dyn MenuMusicProfile>) -> usize {
    let Some(profile) = profile else {
        return 0;
    };
    let level = profile.level();
    let deliveries = profile.deliveries();
    let miles = profile.total_miles();
    let owned = profile.owned_truck_count();
    let truck = profile.active_truck_key();
    if level >= 21 || deliveries >= 75 || miles >= 40_000.0 {
        return 6;
    }
    if level >= 9 || deliveries >= 40 || miles >= 20_000.0 {
        return 5;
    }
    if level >= 7 || miles >= 10_000.0 {
        return 4;
    }
    if level >= 5 || owned >= 2 {
        return 3;
    }
    if level >= 3 || miles >= 2_500.0 {
        return 2;
    }
    if level >= 2 || deliveries >= 3 || truck != "rig" {
        return 1;
    }
    0
}

/// Menu playlist: the night theme leads after dark, else the milestone bed.
///
/// The milestone beds still rotate in after the night theme, so a career loaded
/// at night opens on the quiet night bed and keeps its usual variety. A few
/// radio instrumentals round out the rotation: mellow day picks behind the
/// milestone bed, night blues and jazz behind the night theme.
pub fn select_menu_music_sequence(profile: Option<&dyn MenuMusicProfile>) -> Vec<String> {
    let primary_index = menu_milestone_index(profile);
    let unlocked_count = (primary_index + 1).max(2);
    let options: Vec<MusicTrack> = MENU_TRACKS[..unlocked_count]
        .iter()
        .chain(MENU_ROTATION_TRACKS.iter())
        .chain(
            MENU_LATE_CAREER_ROTATION_TRACKS
                .iter()
                .filter(|_| primary_index >= MENU_LATE_CAREER_MILESTONE),
        )
        .cloned()
        .collect();
    let milestone_primary = MENU_TRACKS[primary_index].key.clone();
    let (primary, pool) = if profile_is_night(profile) {
        let pool: Vec<MusicTrack> = options
            .iter()
            .chain(MENU_NIGHT_ROTATION_TRACKS.iter())
            .cloned()
            .collect();
        (MENU_NIGHT_TRACK.key.clone(), pool)
    } else {
        let pool: Vec<MusicTrack> = options
            .iter()
            .filter(|track| track.key != milestone_primary)
            .chain(MENU_DAY_ROTATION_TRACKS.iter())
            .cloned()
            .collect();
        (milestone_primary, pool)
    };
    let (name, city, deliveries, miles) = match profile {
        Some(p) => (
            p.name(),
            p.current_city(),
            p.deliveries(),
            // str(int(total_miles)): truncation toward zero, like Python's int().
            p.total_miles().trunc() as i64,
        ),
        None => (String::new(), String::new(), 0, 0),
    };
    let seed_key = format!("{name}|{city}|{deliveries}|{miles}|{primary}");
    let rest = crc_ordered(&pool, &seed_key);
    std::iter::once(primary)
        .chain(rest.into_iter().map(|track| track.key))
        .collect()
}

/// The route fields the drive playlist seed reads.
pub trait DriveMusicRoute {
    fn cities(&self) -> Vec<String>;
    fn highways(&self) -> Vec<String>;
    fn terrain_summary(&self) -> String;
}

fn route_key(route: &dyn DriveMusicRoute) -> String {
    format!(
        "{}|{}|{}",
        route.cities().join(","),
        route.highways().join(","),
        route.terrain_summary()
    )
}

/// Return a stable, deterministic day or night driving playlist.
///
/// `weather` is the weather kind's name (`weather_kind.name` in Python), or
/// an empty string for none.
pub fn select_drive_music_sequence(
    route: &dyn DriveMusicRoute,
    trip_seed: i64,
    hour: f64,
    weather: &str,
) -> Vec<String> {
    let options: &[MusicTrack] = if is_night(hour) {
        &NIGHT_DRIVE_TRACKS
    } else {
        &DAY_DRIVE_TRACKS
    };
    let seed_key = format!("{trip_seed}|{weather}|{}", route_key(route));
    crc_ordered(options, &seed_key)
        .into_iter()
        .map(|track| track.key)
        .collect()
}

/// Choose a stable day/night music bed for a trip context.
pub fn select_drive_music(
    route: &dyn DriveMusicRoute,
    trip_seed: i64,
    hour: f64,
    weather: &str,
) -> String {
    select_drive_music_sequence(route, trip_seed, hour, weather)
        .into_iter()
        .next()
        .expect("the drive pools are never empty")
}

/// The length of `key` if it can be known without audio: the shipped
/// catalog or a synthesized piece (composed, not rendered). Anything else,
/// such as a player file, is measured by the playing stream instead.
pub fn known_track_duration_s(key: &str) -> Option<f64> {
    if let Some(info) = TRACKS_BY_KEY.get(key) {
        return Some(info.duration_s);
    }
    crate::music_synth::duration_s(key)
}

/// Best-known duration for slow playlist rotation.
pub fn music_track_duration_s(track: &str) -> f64 {
    known_track_duration_s(track).unwrap_or(60.0)
}

/// A stable shuffled track order for one station on one trip.
pub fn select_station_playlist(playlist: &str, seed_key: &str) -> Vec<String> {
    crc_ordered(station_playlist(playlist), seed_key)
        .into_iter()
        .map(|track| track.key)
        .collect()
}

/// A stable shuffled host-break order for one station on one trip.
pub fn select_host_segments(host: &str, seed_key: &str) -> Vec<String> {
    crc_ordered(station_host_segments(host), seed_key)
        .into_iter()
        .map(|track| track.key)
        .collect()
}

#[cfg(test)]
mod tests {
    //! Music catalog selection. The Python file's App()-driven menu/drive
    //! rotation tests (`test_city_menu_uses_milestone_music` and the rest)
    //! and the on-disk asset sweeps belong to the game crate; the pure
    //! selection logic is pinned here against a profile fake.
    use super::*;
    use std::collections::HashSet;

    /// A `Profile` as the selectors see it.
    #[derive(Default)]
    struct FakeProfile {
        name: String,
        current_city: String,
        game_hours: f64,
        level: i64,
        deliveries: i64,
        total_miles: f64,
        owned_trucks: Vec<String>,
        truck: String,
        business_status: String,
    }

    impl FakeProfile {
        fn named(name: &str) -> Self {
            Self {
                name: name.to_string(),
                // Profile's defaults: the clock starts at 6 AM, level 1,
                // the starter rig owned and active.
                game_hours: 6.0,
                level: 1,
                owned_trucks: vec!["rig".to_string()],
                truck: "rig".to_string(),
                business_status: crate::models::business_constants::COMPANY_DRIVER.to_string(),
                ..Default::default()
            }
        }
    }

    impl MenuMusicProfile for FakeProfile {
        fn game_hours(&self) -> f64 {
            self.game_hours
        }
        fn level(&self) -> i64 {
            self.level
        }
        fn deliveries(&self) -> i64 {
            self.deliveries
        }
        fn total_miles(&self) -> f64 {
            self.total_miles
        }
        fn owned_truck_count(&self) -> usize {
            self.owned_trucks.len()
        }
        fn active_truck_key(&self) -> String {
            self.truck.clone()
        }
        fn name(&self) -> String {
            self.name.clone()
        }
        fn current_city(&self) -> String {
            self.current_city.clone()
        }
        fn business_status(&self) -> String {
            self.business_status.clone()
        }
    }

    struct FakeRoute;

    impl DriveMusicRoute for FakeRoute {
        fn cities(&self) -> Vec<String> {
            vec!["Denver".into(), "Salt Lake City".into()]
        }
        fn highways(&self) -> Vec<String> {
            vec!["I-70".into(), "US-6".into(), "I-15".into()]
        }
        fn terrain_summary(&self) -> String {
            "mountain".into()
        }
    }

    #[test]
    fn test_crc32_matches_zlib() {
        // zlib.crc32(b"hello") == 907060870; zlib.crc32(b"") == 0
        assert_eq!(crc32(b"hello"), 907060870);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn test_menu_music_tracks_career_milestones() {
        let rookie = FakeProfile::named("Rookie");
        assert_eq!(select_menu_music(Some(&rookie)), "menu_theme");

        let mut rookie = rookie;
        rookie.deliveries = 3;
        assert_eq!(select_menu_music(Some(&rookie)), "menu_first_rig");

        // 2,500 xp lands the Python Profile on career level 3.
        let mut regional = FakeProfile::named("Regional");
        regional.level = 3;
        assert_eq!(select_menu_music(Some(&regional)), "menu_regional_carrier");

        let mut fleet = FakeProfile::named("Fleet");
        fleet.owned_trucks = vec!["rig".into(), "heavy_hauler".into()];
        assert_eq!(select_menu_music(Some(&fleet)), "menu_fleet_owner");

        let mut coast = FakeProfile::named("Coast");
        coast.total_miles = 10_000.0;
        assert_eq!(select_menu_music(Some(&coast)), "menu_coast_to_coast");

        let mut legend = FakeProfile::named("Legend");
        legend.deliveries = 40;
        assert_eq!(select_menu_music(Some(&legend)), "menu_legendary_haul");
    }

    #[test]
    fn test_menu_music_sequence_is_milestone_pool() {
        let rookie = FakeProfile::named("Rookie");
        let rookie_pool = select_menu_music_sequence(Some(&rookie));
        assert_eq!(rookie_pool[0], "menu_theme");
        assert!(rookie_pool.len() > 1);
        assert!(rookie_pool.contains(&"menu_theme".to_string()));
        assert!(rookie_pool.contains(&"menu_urban_roll".to_string()));

        let mut coast = FakeProfile::named("Coast");
        coast.total_miles = 10_000.0;
        let coast_pool = select_menu_music_sequence(Some(&coast));
        assert_eq!(coast_pool[0], "menu_coast_to_coast");
        assert!(coast_pool.len() > rookie_pool.len());
        assert!(coast_pool.contains(&"menu_theme".to_string()));
    }

    #[test]
    fn test_bossa_to_blues_joins_the_menu_with_million_mile_morning() {
        let bossa = "drive_bossa_to_blues".to_string();
        let mut regional = FakeProfile::named("Regional");
        regional.level = 8;
        regional.game_hours = 12.0;
        assert!(!select_menu_music_sequence(Some(&regional)).contains(&bossa));

        for hours in [12.0, 23.0] {
            let mut late = FakeProfile::named("Late");
            late.level = 9;
            late.game_hours = hours;
            assert_eq!(
                menu_milestone_index(Some(&late)),
                MENU_LATE_CAREER_MILESTONE
            );
            assert!(select_menu_music_sequence(Some(&late)).contains(&bossa));
        }
        assert_eq!(
            MENU_TRACKS[MENU_LATE_CAREER_MILESTONE].key,
            "menu_legendary_haul"
        );
    }

    #[test]
    fn test_menu_day_rotation_borrows_radio_instrumentals() {
        let mut day = FakeProfile::named("Rookie");
        day.game_hours = 12.0;
        let pool = select_menu_music_sequence(Some(&day));
        for track in MENU_DAY_ROTATION_TRACKS.iter() {
            assert!(pool.contains(&track.key));
        }
        // The night blues stay behind the night theme only.
        for track in MENU_NIGHT_ROTATION_TRACKS.iter() {
            assert!(!pool.contains(&track.key));
        }
    }

    #[test]
    fn test_menu_music_uses_night_theme_after_dark() {
        // A daytime career still gets its milestone bed.
        let mut day = FakeProfile::named("Rookie");
        day.game_hours = 12.0;
        assert_eq!(select_menu_music(Some(&day)), "menu_theme");
        assert_eq!(select_menu_music_sequence(Some(&day))[0], "menu_theme");

        // The same career loaded at night opens on the night bed, with the
        // milestone beds still rotating in after it.
        let mut night = FakeProfile::named("Rookie");
        night.game_hours = 23.0;
        assert_eq!(select_menu_music(Some(&night)), "menu_theme_night");
        let seq = select_menu_music_sequence(Some(&night));
        assert_eq!(seq[0], "menu_theme_night");
        assert!(seq.contains(&"menu_theme".to_string()));
        assert!(seq.len() > 1);

        // The night menu rotates in the night instrumentals, not the day picks.
        for track in MENU_NIGHT_ROTATION_TRACKS.iter() {
            assert!(seq.contains(&track.key));
        }
        for track in MENU_DAY_ROTATION_TRACKS.iter() {
            assert!(!seq.contains(&track.key));
        }

        // No loaded career (title screen, no saves) falls back to the day bed.
        assert_eq!(select_menu_music(None), "menu_theme");
    }

    #[test]
    fn test_menu_ambience_follows_the_clock_and_the_business() {
        let mut company = FakeProfile::named("Rookie");
        company.game_hours = 12.0;
        assert_eq!(
            select_menu_ambience(Some(&company)),
            "ambient/menu_dispatch_office"
        );

        let mut owner = FakeProfile::named("Owner");
        owner.game_hours = 12.0;
        owner.business_status =
            crate::models::business_constants::INDEPENDENT_AUTHORITY.to_string();
        assert_eq!(select_menu_ambience(Some(&owner)), "ambient/menu_yard");

        // After dark everyone is at the truck stop, whatever they drive for.
        owner.game_hours = 23.0;
        assert_eq!(select_menu_ambience(Some(&owner)), "ambient/menu_night_lot");

        // No career yet: a new driver signing on at dispatch.
        assert_eq!(select_menu_ambience(None), "ambient/menu_dispatch_office");
    }

    #[test]
    fn test_drive_music_sequence_is_stable_pool_for_trip_and_separates_day_night() {
        let route = FakeRoute;
        let day = select_drive_music_sequence(&route, 12345, 13.0, "");
        assert_eq!(day, select_drive_music_sequence(&route, 12345, 13.5, ""));
        assert_eq!(day.len(), DAY_DRIVE_TRACKS.len());
        assert!(day.iter().collect::<HashSet<_>>().len() > 1);
        let day_keys: HashSet<&str> = DAY_DRIVE_TRACKS.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(
            day.iter().map(String::as_str).collect::<HashSet<_>>(),
            day_keys
        );
        assert_eq!(select_drive_music(&route, 12345, 13.0, ""), day[0]);

        let night = select_drive_music_sequence(&route, 12345, 23.0, "");
        assert_eq!(night, select_drive_music_sequence(&route, 12345, 23.5, ""));
        assert_eq!(night.len(), NIGHT_DRIVE_TRACKS.len());
        assert!(night.iter().collect::<HashSet<_>>().len() > 1);
        let night_keys: HashSet<&str> = NIGHT_DRIVE_TRACKS.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(
            night.iter().map(String::as_str).collect::<HashSet<_>>(),
            night_keys
        );
        assert_ne!(night, day);
    }

    #[test]
    fn test_jazz_pool_exists_and_ships_assets() {
        assert_eq!(JAZZ_TRACKS.len(), 15);
        assert_eq!(station_playlist("jazz"), JAZZ_TRACKS.as_slice());
    }

    #[test]
    fn test_seventh_menu_milestone_unlocks_at_level_21() {
        assert_eq!(MENU_TRACKS[6].key, "menu_progress");
        let mut high = FakeProfile::named("High");
        high.level = 21;
        high.owned_trucks.clear();
        assert_eq!(menu_milestone_index(Some(&high)), 6);
        let mut mid = FakeProfile::named("Mid");
        mid.level = 9;
        mid.deliveries = 40;
        mid.total_miles = 20_000.0;
        mid.owned_trucks.clear();
        assert_eq!(menu_milestone_index(Some(&mid)), 5);
    }

    #[test]
    fn test_station_playlist_selection_is_deterministic_and_complete() {
        let first = select_station_playlist("classic_rock", "seed|wgrx-chicago");
        let second = select_station_playlist("classic_rock", "seed|wgrx-chicago");
        assert_eq!(first, second);
        let rock: HashSet<&str> = CLASSIC_ROCK_TRACKS.iter().map(|t| t.key.as_str()).collect();
        assert_eq!(
            first.iter().map(String::as_str).collect::<HashSet<_>>(),
            rock
        );
        let other = select_station_playlist("classic_rock", "seed|kdrt-phoenix");
        assert_eq!(
            other.iter().collect::<HashSet<_>>(),
            first.iter().collect::<HashSet<_>>()
        );

        let hosts = select_host_segments("roadhouse", "seed|route_playlist");
        let roadhouse: HashSet<&str> = ROADHOUSE_HOST_SEGMENTS
            .iter()
            .map(|t| t.key.as_str())
            .collect();
        assert_eq!(
            hosts.iter().map(String::as_str).collect::<HashSet<_>>(),
            roadhouse
        );
        assert!(select_host_segments("", "seed|none").is_empty());
    }

    #[test]
    fn test_host_segments_number_one_hundred_fifty_two() {
        // 19 voiced stations, 8 breaks each (the asset contract).
        assert_eq!(ALL_HOST_SEGMENTS.len(), 152);
        assert_eq!(ROADHOUSE_HOST_SEGMENTS[0].key, "host_roadhouse_01");
        assert_eq!(
            ROADHOUSE_HOST_SEGMENTS[0].title,
            "Freight Fate Roadhouse host break 1"
        );
        assert_eq!(NIGHTLINE_HOST_SEGMENTS[7].duration_s, 7.4);
    }

    #[test]
    fn test_music_track_duration_falls_back_to_a_minute() {
        assert_eq!(music_track_duration_s("open_road"), 131.6);
        assert_eq!(music_track_duration_s("host_roadhouse_01"), 6.7);
        assert_eq!(music_track_duration_s("no_such_key"), 60.0);
    }

    #[test]
    fn test_every_track_key_is_unique() {
        let keys: Vec<&str> = ALL_MUSIC_TRACKS
            .iter()
            .chain(ALL_HOST_SEGMENTS.iter())
            .map(|t| t.key.as_str())
            .collect();
        let unique: HashSet<&str> = keys.iter().cloned().collect();
        assert_eq!(keys.len(), unique.len());
    }
}
