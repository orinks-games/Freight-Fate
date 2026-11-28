# Freight Fate 2.0 radio clips

The source for the 2.0 `music.pak` rebuild: the Roadhouse host breaks, the
spoken legal ID and liners, the sung Coast to Coast ID, and fifteen carrier
hiring ads. Nothing in the game or the release reads this folder. The game
hears these clips only through the rebuilt pack pinned in
`crates/ff-tools/src/release/packs.rs`.

- `pack-overlay/music/` holds the 27 clips, flat, each named by the key the
  game asks for. It is shaped as a `pack-sounds` overlay.
- `Roadhouse.txt` has the scripts.
- `hosts-encode-report.json` and `ads-encode-report.json` are the encode
  reports.

Encoding: Ogg Opus, 48 kHz stereo, 80 kbps, static gain to -18 LUFS
integrated, -1 dBTP true-peak ceiling, no compression. Host, legal ID and
liner reads are ElevenLabs `eleven_v4`. Ads were mixed first, then encoded
the same way from the approved finals.

## Rebuilding the pack

The rebuild is the previous pack plus this folder. The base pack is the one
pinned in `crates/ff-tools/src/release/packs.rs`: now
`https://crisp-crystal-9a9y.here.now/music-9782a9ba.pak`, SHA-256
`9782a9ba3f259f60609a5df5097cfcc2a9ddbc970d607687fe115608ee4bf691`
(442 entries, 385,764,865 bytes). Put it at `assets/music.pak`, then:

```
cargo tools pack-sounds --overlay assets/radio-2.0/pack-overlay \
  --out <scratch>/sounds.pak --music-out build/music-next.pak
```

The `--out` file is a by-product; discard it. The result is published as
`music-<first 8 hex of its SHA-256>.pak` beside the old pack, so a branch
that pins the old pack keeps building.

Start from the pinned pack, not the 1.9 `music.pak`: the pinned pack also
carries `music/sleeper_berth`, the 13.8 s loop that plays while the player
sleeps through a co-driver's shift. That clip is in no repo folder (music
files stay out of the repo), so a rebuild from the 1.9 pack would drop it.
It went in on 2026-11-28 by overlaying a scratch folder holding
`music/sleeper_berth.opus` on `music-9f4c05a1.pak` (441 entries), the pack
this folder first produced.

## Hosts

Voice: Clyde - Radio Announcer (`QMJTqaMXmGnG8TCm8WQG`), the Roadhouse host.
These replace the earlier `host_roadhouse_01` to `_08` reads.

| Key | Duration |
| --- | --- |
| `host_roadhouse_01` | 5.28 s |
| `host_roadhouse_02` | 4.56 s |
| `host_roadhouse_03` | 5.84 s |
| `host_roadhouse_04` | 5.52 s |
| `host_roadhouse_05` | 5.12 s |
| `host_roadhouse_06` | 5.60 s |
| `host_roadhouse_07` (Clyde names himself) | 6.24 s |
| `host_roadhouse_08` | 6.40 s |

## Station IDs

| Key | What it is | Duration |
| --- | --- | --- |
| `id_roadhouse_03` | Spoken legal ID: "F-F-R, Freight Fate Roadhouse, coast to coast." Replaces the earlier read. | 4.48 s |
| `id_roadhouse_08` | Sung Coast to Coast ID | 10.64 s |
| `id_roadhouse_09` | Spoken liner: "Freight Fate Roadhouse. Coast to coast, dock to dock." | 4.16 s |
| `id_roadhouse_10` | Spoken liner: "You're on the Freight Fate Roadhouse. Stay with us." | 3.36 s |

The spoken reads are in Clyde's voice. `id_roadhouse_01`, `_02`, `_04`,
`_05`, `_06` and `_07` are sung IDs already in the pack and stay as they
are.

## Carrier hiring ads

The Roadhouse airs only these fifteen. The six formats that carried the
earlier hiring ads (country, classic rock, gospel, Tejano, blues, jazz)
carry them too, beside their other ads. Meridian Freight Lines is a
fictional advertiser and stays off the Roadhouse.

| Key | Carrier | Voice | Notes |
| --- | --- | --- | --- |
| `ad_northstar_freight_hiring` | Northstar Freight Lines | Hale - Great for Commercials | Replaces the packed spot |
| `ad_prairie_link_hiring` | Prairie Link Regional | Beth - Commercial, Upbeat, Engaging | Replaces the packed spot |
| `ad_summit_value_hiring` | Summit Value Logistics | Hale - Great for Commercials | Replaces the packed spot |
| `ad_great_lakes_training_hiring` | Great Lakes Training Transport | Rachelle - TV & Radio Commercials | New |
| `ad_winterset_refrigerated_hiring` | Winterset Refrigerated Lines | Laurel - Warm, Kind & Confident | New |
| `ad_double_springs_hiring` | Double Springs Linehaul | Hale - Great for Commercials | New |
| `ad_open_hearth_hiring` | Open Hearth Flatbed Lines | A. Michael Smith - iDJMic | New |
| `ad_longspur_hiring` | Longspur Transport | Noah - Confident Canadian Narrator | New |
| `ad_great_shield_hiring` | Great Shield Transport | A. Michael Smith - iDJMic | New |
| `ad_chehalis_freight_hiring` | Chehalis Freight Lines | Ivan Smith | New |
| `ad_tehachapi_hiring` | Tehachapi Motor Lines | Laurel - Warm, Kind & Confident | New |
| `ad_lone_mesa_hiring` | Lone Mesa Freight | Chad | New |
| `ad_androscoggin_hiring` | Androscoggin Freight | Jerry B. | New |
| `ad_chatanika_freight_hiring` | Chatanika Freight Lines | Coach Dave | New |
| `ad_olentangy_valley_hiring` | Olentangy Valley Freight | Piper - Energetic, Friendly & Upbeat | New |
