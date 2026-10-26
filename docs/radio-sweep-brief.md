# Radio sweep brief: real stations, honest call signs

For Oatis, own worktree off feat/career-1.9 (tip 6f1e7ac+), branch
`radio/station-sweep`. Owner approved 2026-07-20. NEVER work in the
main checkout. Data-only where possible; one player changelog bullet
at the end.

## Job 1: fictional call signs must not squat real stations

The catalog's 15 fictional stations carry invented call signs, and at
least one collides: our "KDRT Desert Rock 101.5" (Phoenix, fictional)
vs the real KDRT-LP, community radio in Davis, California -- a player
who knows the real dial reported the confusion. Audit every fictional
call sign (FFR, FFN, SAT, KRWL, WHWY, KPLN, KBSK, WGRX, KDRT, KCHM,
KRDG, KSND, WDLT, WBYU, WSOL) against the FCC's license search; WDLT
and WSOL look suspect on sight. For each collision, propose a
replacement sign that the FCC lists as unassigned, keep the station's
brand name ("Desert Rock 101.5" survives, its letters change), apply
the rename in `radio_catalog.json`, and record old -> new in the
marker so the owner can forward the list to Josh (the stations are his
creations; he gets the final word before the release cut). No spoken
audio references any call sign (only FFR/FFN have hosts, and they use
brand names), so renames are data-only.

## Job 2: community, college, and NPR coverage

There is no policy against real terrestrial stations -- 63 already
stream. The gaps are just unswept ground:

- Add the freeform and community institutions with reliable public
  streams: WFMU (Jersey City), KABF (Little Rock), and college or
  community stations that fill thin markets. Quality bar over
  quantity: a station joins only with a stream that passes
  `cargo tools check-radio-streams`.
- NPR coverage audit: for each catalog market, is at least one NPR
  member station receivable? Fill the holes so local news works most
  places a route goes.
- Retry the known dead list from ROADMAP (KDHX St. Louis, KPFT
  Houston, WABE Atlanta) and the uncovered markets (Rio Grande
  Valley, Savannah, Amarillo). A station that stays dead keeps
  `supported: false` with a dated note.

Method, as the desert-Southwest sweep did it: find each station's own
stream URL from its website (never TuneIn -- partner-gated -- and
Radio Browser only as a finding aid, never a runtime dependency);
real transmitter lat/lon and an honest `range_miles`; `real_stream:
true, safe_for_streaming: false`; a source note per station. Verify
every added or repointed stream with `cargo tools check-radio-streams`
and report movers.

## Gates and handshake

`uv run pytest tests/test_radio.py tests/test_radio_regional.py
tests/test_radio_playlists.py`, then the full suite; ruff;
compileall; `refresh_map_data.py --radio` green on everything
touched. When done: push the branch to fork (NEVER origin), write
`C:/dev/Freight-Fate/logs/oatis-radio-sweep-done.json` with stations
added per market, renames old -> new with the FCC evidence, retried
dead stations and their outcomes, and the NPR coverage table. Phil
reviews against this brief and merges.
