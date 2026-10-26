# Channel 3000 source notes

The briefs and staging files for Channel 3000, the in-game radio station at
87.7. The plan is `docs/superpowers/plans/2026-10-06-channel-3000.md`.

- `STATION.md`, `DAYTIME.md`, `NIGHT.md`: the station's briefs ("What plays
  when" is the schedule's spec).
- `clips.json`: kind, dayparts, opener, title and measured duration per clip,
  as `stage_clips.py` wrote it. `data/channel3000.json` is the committed copy
  the game reads.
- `stage_clips.py`: how the clips were made from the owner's local masters.

The clips themselves (`clips/*.opus`: Ogg Opus, 48 kbps stereo, 48 kHz, one
static gain each to -18 LUFS with a -1 dBTP ceiling) are not in git. They
used to sit on an orphan `channel3000-clips` branch, removed on 2026-10-11.
The owner keeps them locally, and the published `channel3000.pak` stores
every clip unchanged, so the pack is also a copy of them.
