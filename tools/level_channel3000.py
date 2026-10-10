"""Level Channel 3000's talk to the rest of the radio, in place on its clips.

The clips were each given one static gain to -18 LUFS integrated, like
music.pak. That suits a song, which sits near one level throughout, and it
suits the station's own hosts and idents. It does not suit the shows: a
drama's integrated level is set by its loud theme and stings, so static gain
left the talking between them 2 to 4 dB under every other station, while the
stings landed above. A driver turned the in-cab radio up to follow the
dialogue, got blasted by the next sting, and found every other station too
loud at that setting.

Measured on the 3 s short-term loudness a listener actually follows, the
music.pak radio songs, hosts, ads and idents sit at a median of about -18
LUFS with a loudness range of 1 to 8 LU. The shows sat at a median of -19
to -25 LUFS with ranges up to 17 LU.

So every clip but the Grimatonics songs (music, held to music.pak's
static-gain rule) goes through ``loudness.ride_gain``, a slow leveller that
brings the short-term loudness toward the target the way a TV station's
processor does, then a static trim back to the integrated target and
``loudness.limit_true_peak`` under the ceiling. Clips that were already
level (the hosts and idents) come out nearly unchanged.

    uv run python tools/level_channel3000.py ../c3k-clips          # report
    uv run python tools/level_channel3000.py ../c3k-clips --write  # rewrite

``--write`` re-encodes each clip in the folder and refreshes its loudness
fields in ``clips.json``; then rebuild the pack with
``tools/build_channel3000.py``. Durations are kept: the schedule runs on them.
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from concurrent.futures import ProcessPoolExecutor
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parent))
import loudness  # noqa: E402

TARGET_LUFS = -18.0
MAX_TRUE_PEAK = -1.0
# The limiter aims under the ceiling: the Opus encoder's own overshoot
# lands on top of whatever the limiter leaves.
LIMIT_AT = -2.0
BITRATE = 48_000
# Songs follow music.pak's rule: one static gain, dynamics untouched.
STATIC_PREFIXES = ("grimatonics_",)


def decode(path: Path) -> np.ndarray:
    raw = subprocess.run(
        ["ffmpeg", "-v", "error", "-i", str(path), "-f", "f32le", "-ac", "2", "-ar", "48000", "-"],
        capture_output=True,
        check=True,
    ).stdout
    return np.frombuffer(raw, dtype="<f4").reshape(-1, 2).T.astype(np.float64)


def encode(samples: np.ndarray, dst: Path) -> None:
    """Ogg Opus at ``BITRATE``, constrained VBR, as ``stage_clips.py`` made them."""
    pcm = np.clip(samples, -1.0, 1.0).T.astype("<f4").tobytes()
    subprocess.run(
        [
            "ffmpeg",
            "-v",
            "error",
            "-y",
            "-f",
            "f32le",
            "-ar",
            str(loudness.RATE),
            "-ac",
            "2",
            "-i",
            "pipe:0",
            "-c:a",
            "libopus",
            "-b:a",
            str(BITRATE),
            "-vbr",
            "constrained",
            str(dst),
        ],
        input=pcm,
        check=True,
    )


def median_short_term(samples: np.ndarray) -> float:
    """The median 3 s short-term loudness: where the talking sits."""
    levels = loudness._lufs(loudness._k_weighted_power(samples, 3.0, 1.0))
    levels = levels[levels > -70.0]
    return float(np.median(levels)) if levels.size else float("-inf")


def level(samples: np.ndarray) -> np.ndarray:
    """Ride, trim to the integrated target, and hold the peaks down."""
    out = samples * 10.0 ** (loudness.ride_gain(samples, TARGET_LUFS) / 20.0)
    # The limiter takes a little off the integrated level; twice settles it.
    for _ in range(2):
        out = out * 10.0 ** ((TARGET_LUFS - loudness.integrated(out)) / 20.0)
        out = loudness.limit_true_peak(out, LIMIT_AT)
    return out


def level_clip(job: tuple[str, str, bool]) -> dict:
    path, key, write = Path(job[0]), job[1], job[2]
    before = decode(path)
    row = {
        "key": key,
        "before_median": median_short_term(before),
        "before_lra": loudness.loudness_range(before),
    }
    if key.startswith(STATIC_PREFIXES):
        row.update(levelled=False, after_median=row["before_median"], after_lra=row["before_lra"])
        return row
    after = level(before)
    row.update(levelled=True)
    if write:
        tmp = path.with_suffix(".tmp.opus")
        encode(after, tmp)
        # Encoder overshoot past the ceiling: trim by the excess and redo,
        # until it lands under (the overshoot moves with the trim).
        decoded = decode(tmp)
        trim = 0.0
        for _ in range(4):
            over = loudness.true_peak(decoded) - MAX_TRUE_PEAK
            if over <= 0:
                break
            trim += over + 0.1
            encode(after * 10.0 ** (-trim / 20.0), tmp)
            decoded = decode(tmp)
        tmp.replace(path)
        after = decoded
        row.update(
            lufs=round(loudness.integrated(after), 2),
            true_peak=round(loudness.true_peak(after), 2),
            samples=after.shape[1],
        )
    row.update(after_median=median_short_term(after), after_lra=loudness.loudness_range(after))
    return row


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path, help="Folder with clips/ and clips.json.")
    parser.add_argument("--write", action="store_true", help="Re-encode the clips in place.")
    parser.add_argument("--jobs", type=int, default=max(1, (os.cpu_count() or 2) // 2))
    parser.add_argument(
        "--only", action="append", default=[], metavar="KEY", help="Just this clip; repeatable."
    )
    args = parser.parse_args(argv)

    manifest_path = args.source / "clips.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    clips_dir = args.source / "clips"
    jobs = [
        (str(clips_dir / f"{c['key']}.opus"), c["key"], args.write)
        for c in manifest["clips"]
        if not args.only or c["key"] in args.only
    ]
    with ProcessPoolExecutor(args.jobs) as pool:
        rows = {row["key"]: row for row in pool.map(level_clip, jobs)}

    print(f"{'clip':40} {'median before':>13} {'after':>6} {'LRA before':>10} {'after':>6}")
    for key in sorted(rows):
        r = rows[key]
        print(
            f"{key[:40]:40} {r['before_median']:13.1f} {r['after_median']:6.1f}"
            f" {r['before_lra']:10.1f} {r['after_lra']:6.1f}{'' if r['levelled'] else '  (song)'}"
        )
    if not args.write:
        print("\nDry run. Re-run with --write to re-encode.")
        return 0

    for clip in manifest["clips"]:
        r = rows.get(clip["key"])
        if r is None or not r["levelled"]:
            continue
        clip["lufs"], clip["true_peak"] = r["lufs"], r["true_peak"]
        clip["levelled"] = True
        # The same samples go back in, so the length cannot move; check it.
        seconds = r["samples"] / loudness.RATE
        if abs(seconds - clip["duration_s"]) > 0.01:
            raise SystemExit(f"{clip['key']} changed length: {clip['duration_s']} -> {seconds:.3f}")
    manifest["notes"] = (
        "Channel 3000's clips for channel3000.pak: Ogg Opus, 48 kbps stereo, 48 kHz, "
        "each at -18 LUFS with a -1 dBTP ceiling. The Grimatonics songs have one static gain, "
        "like music.pak; every other clip is levelled by tools/level_channel3000.py so its talk "
        "sits at the level of the other stations. Made by stage_clips.py from the Dark Nursery "
        "Rhymes masters. Dayparts by the truck's local hour: day 5-19, prime 19-22, late 22-2, "
        "overnight 2-5."
    )
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n", encoding="utf-8")
    print(f"\nRewrote {sum(r['levelled'] for r in rows.values())} clips and {manifest_path}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
