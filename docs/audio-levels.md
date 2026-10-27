# Audio levels

Measured 2026-10-03 with `tools/loudness.py` (ITU-R BS.1770-4 / EBU R128;
agrees with ffmpeg's `loudnorm` within 0.1 LU on music). Per-file numbers,
before and after, are in `audio-levels/music.tsv` and
`audio-levels/sounds.tsv`.

## Music pack

The game applies no per-track gain: the music volume (menus, drive music) and
radio volume settings are the only thing between a file and the player. Every
track in `music.pak` (songs, hosts, ads, station IDs, all on the one music
channel) was given one static gain to -18 LUFS integrated, with a -1 dBTP
true-peak ceiling, by `tools/encode_music_opus.py`. No compression or
limiting. Tracks already within 0.5 dB were left as shipped.

| Group | Tracks | Before, LUFS | After, LUFS |
|---|---|---|---|
| ad | 21 | -24.1 to -15.6 | -21.7 to -17.8 |
| drive | 21 | -25.5 to -12.6 | -18.1 to -17.9 |
| host | 152 | -22.5 to -15.2 | -20.2 to -17.5 |
| id | 67 | -22.1 to -12.0 | -18.6 to -17.6 |
| menu | 10 | -26.0 to -12.9 | -18.1 to -17.9 |
| night | 19 | -26.0 to -13.3 | -18.1 to -17.9 |
| open | 1 | -25.9 | -18.1 |
| radio | 135 | -17.5 to -9.9 | -18.2 to -17.8 |

23 speech segments (8 ads, 14 host breaks, 1 station ID) have peaks too high
for the full gain and stop at the ceiling, up to 3.7 dB under target
(`peak_limited` in the TSV). Opus encoding leaves 15 tracks a fraction of a
dB over -1 dBTP (worst -0.4); none reaches full scale.

At the default settings (music 0.5, radio 0.25) the average drive song now
sits about 3 dB quieter than before and the average radio song about 4.5 dB
quieter, while the five menu and night tracks that were mastered at -26 LUFS
are 8 dB louder. The defaults were left alone: both settings have headroom.

## Sound pack

Sound effects are not meant to be one loudness, and call sites set per-cue
volumes against these files, so they were grouped by bus and role and only
plain outliers changed. Short cues are compared by their loudest 400 ms block
(max momentary) and true peak. None of the changed cues had a call-site volume
compensating for its level.

| Sound | Gain | Why |
|---|---|---|
| events/traffic_slowing | +14 dB | About 20 dB under every other road event cue, all played at full volume |
| weather/rain_light | +6 dB | 21 dB under heavy rain at the same weather volume; nearly inaudible over the engine |
| weather/snow_wind | +7 dB | 27 dB under heavy rain; raised to its peak ceiling |
| poi/rest_stop_night | +6 dB | 10 dB under the daytime rest stop bed it replaces at night |
| vehicle/road | +6 dB | 2026-10-08: about 15 dB under the engine at highway speed, so the road was barely there (owner: "we need road sounds on the interstate") |

Left as they are: the shift-sound bank variants 09, 11 and 15 read 5 to 11 dB
under their siblings, but their peaks are already near full scale, so static
gain cannot close the gap (on the roadmap). No whole-bus mismatch was big
enough to change a bus constant.

## Traffic sounds (2026-10-08)

The traffic loops (`traffic/*_loop`) were given static gain to -20 LUFS for a
car and a little more for heavier vehicles (semi +3 dB, bus +2, box truck
+1.5, tractor and motorcycle +1, pickup +0.5), and the freeway bed
(`traffic/highway_bed`) to -22 LUFS. The game scales each one by distance
from its vehicle (`TRAFFIC_SOUND_PEAK` one lane over, falling as one over the
distance) and the bed by the road's traffic presence (`TRAFFIC_BED_PEAK` at
the busiest road), so those two constants are where a listening pass tunes
them.

2026-10-09, the owner's first listening drive: no single vehicle could be
heard over the engine. `TRAFFIC_SOUND_PEAK` went from 0.45 at 12 feet to 0.8
at 60 feet, and the hearing edge from 450 to 1,500 feet, since the bubble
keeps vehicles a tenth of a mile apart or more. Past 60 feet a sound now drops
3 dB per doubling of distance, not 6: at 6 dB the owner still heard a vehicle
only as it passed. On an exit ramp the divergence alone takes the other 3 dB,
so the freeway still falls away as the ramp leaves it. The loops were then
lifted 12 dB around 1.2 kHz and cut 3 dB around 300 Hz, restored to their own
loudness: as rendered, over 90 percent of a car's energy sat under 500 Hz with
the engine, which masked it. The bed was re-made steady
(`sound-test/highway_bed.py`) and stays at -22 LUFS.

