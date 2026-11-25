# Freight Fate 2.0 radio staging

Pack-ready audio and scripts for a later `music.pak` rebuild on `feat/career-2.0`. This folder is staging only. It does not change `packs.rs`, and this pull request does not rebuild or upload the pack.

Airing the fifteen carrier ads on the Roadhouse is implemented in https://github.com/orinks-games/Freight-Fate/pull/841, which depends on this pull request. Use the ad file names below as the pack / rotation keys in that work. `ad_northstar_freight_hiring`, `ad_prairie_link_hiring`, and `ad_summit_value_hiring` replace the hiring spots already in the pack. The other twelve keys are new.

Encoding: Ogg Opus, 48 kHz stereo, 80 kbps, static gain to −18 LUFS integrated, −1 dBTP true-peak ceiling, no compression. Host, legal ID, and liner reads are ElevenLabs `eleven_v4`. Ads were mixed first, then encoded the same way from the approved top-level finals.

Spoken copy for the Roadhouse lines and the fifteen ad scripts is in `Roadhouse.txt`.

## Hosts (`hosts/`)

All eight use voice Clyde - Radio Announcer (`QMJTqaMXmGnG8TCm8WQG`), the existing Freight Fate Roadhouse host voice. Intended pack names match the file names.

| File | What it is | Duration |
| --- | --- | --- |
| `host_roadhouse_01.opus` | Host break 1 | 5.29 s |
| `host_roadhouse_02.opus` | Host break 2 | 4.57 s |
| `host_roadhouse_03.opus` | Host break 3 | 5.85 s |
| `host_roadhouse_04.opus` | Host break 4 | 5.53 s |
| `host_roadhouse_05.opus` | Host break 5 | 5.13 s |
| `host_roadhouse_06.opus` | Host break 6 | 5.61 s |
| `host_roadhouse_07.opus` | Host break 7 (Clyde names himself) | 6.25 s |
| `host_roadhouse_08.opus` | Host break 8 | 6.41 s |

These replace the packed `host_roadhouse_01` … `host_roadhouse_08` reads.

## Spoken legal ID and liners (`ids/`)

Same host voice, Clyde - Radio Announcer (`QMJTqaMXmGnG8TCm8WQG`).

| File | What it is | Duration | Intended pack name |
| --- | --- | --- | --- |
| `id_roadhouse_03.opus` | Spoken legal ID: “F-F-R, Freight Fate Roadhouse, coast to coast.” | 4.49 s | `id_roadhouse_03` |
| `id_roadhouse_05.opus` | Spoken liner 5 | 4.17 s | `id_roadhouse_05` |
| `id_roadhouse_06.opus` | Spoken liner 6 | 3.37 s | `id_roadhouse_06` |

These are the spoken liner and legal-ID slots from the radio content plan (`id_01` / `id_02` / `id_04` / `id_07` / `id_08` stay sung IDs). Confirm the current pack’s `id_roadhouse_05` before overwriting it: an earlier check found a sung ID already living under that key.

## Coast to Coast sung ID (`jingles/`)

| File | What it is | Intended pack name |
| --- | --- | --- |
| `id_roadhouse_08.opus` | Sung Coast to Coast station ID, already wired in this pull request’s first commit | `id_roadhouse_08` |

This file is not a new `eleven_v4` render. It is included here so the pack rebuild can pick up every new Roadhouse asset from one folder.

## Carrier hiring ads (`ads/`)

Fifteen approved finals. Rotation keys are the file names without `.opus`. The Roadhouse (`route`) should air only these fifteen spots. Other format stations keep their existing ads and also carry these. Meridian Freight Lines stays a fictional advertiser on the other stations and does not play on the Roadhouse.

| File / pack key | Carrier | Voice | Notes |
| --- | --- | --- | --- |
| `ad_northstar_freight_hiring.opus` | Northstar Freight Lines | Hale - Great for Commercials | Replaces the packed hiring spot |
| `ad_prairie_link_hiring.opus` | Prairie Link Regional | Beth - Commercial, Upbeat, Engaging | Replaces the packed hiring spot |
| `ad_summit_value_hiring.opus` | Summit Value Logistics | Hale - Great for Commercials | Replaces the packed hiring spot |
| `ad_great_lakes_training_hiring.opus` | Great Lakes Training Transport | Rachelle – TV & Radio Commercials | New |
| `ad_winterset_refrigerated_hiring.opus` | Winterset Refrigerated Lines | Laurel - Warm, Kind & Confident | New |
| `ad_double_springs_hiring.opus` | Double Springs Linehaul | Hale - Great for Commercials | New |
| `ad_open_hearth_hiring.opus` | Open Hearth Flatbed Lines | A. Michael Smith - iDJMic | New |
| `ad_longspur_hiring.opus` | Longspur Transport | Noah - Confident Canadian Narrator | New |
| `ad_great_shield_hiring.opus` | Great Shield Transport | A. Michael Smith - iDJMic | New |
| `ad_chehalis_freight_hiring.opus` | Chehalis Freight Lines | Ivan Smith | New |
| `ad_tehachapi_hiring.opus` | Tehachapi Motor Lines | Laurel - Warm, Kind & Confident | New |
| `ad_lone_mesa_hiring.opus` | Lone Mesa Freight | Chad | New |
| `ad_androscoggin_hiring.opus` | Androscoggin Freight | Jerry B. | New |
| `ad_chatanika_freight_hiring.opus` | Chatanika Freight Lines | Coach Dave | New |
| `ad_olentangy_valley_hiring.opus` | Olentangy Valley Freight | Piper - Energetic, Friendly & Upbeat | New |

## Pack rebuild

Do not merge this pull request, or #841, until the new `music.pak` is published and its SHA is pinned in `packs.rs` in a follow-up. A build with these keys and the current pack would play dead air for every new file.
