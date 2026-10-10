"""Generate in-cab radio music, host segments, and static via ElevenLabs.

Build-time only, like generate_sounds.py. Reads the ElevenLabs key from an
out-of-repo file (or ELEVENLABS_API_KEY / local ignored .env), composes
station music with the Eleven Music API, voices the Freight Fate Roadhouse
and Night Line hosts with the TTS API, and writes the project's Ogg Vorbis
convention with ffmpeg. The static burst is procedural (numpy) and costs no
credits. Never run at runtime; the key is never bundled.

Imaging (station-ID SFX beds) generates via the Sound Effects API, gated
behind --sfx since it spends credits; the post-production chain applied to
station-ID voice reads and ad reads (imaging_process / broadcast_compress /
mix_id_bed) is pure numpy and costs nothing -- see
tests/test_radio_imaging_chain.py for its numeric contract.

Usage:
    uv run python tools/generate_radio.py                # everything
    uv run python tools/generate_radio.py --hosts        # host lines only
    uv run python tools/generate_radio.py --music        # music only
    uv run python tools/generate_radio.py --static       # static burst only
    uv run python tools/generate_radio.py radio_country_backroads
    uv run python tools/generate_radio.py --voices --dry-run  # show what would be added
    uv run python tools/generate_radio.py --voices             # add missing cast voices
    uv run python tools/generate_radio.py --sfx                # imaging SFX beds (spends credits)

Plan-driven runners (tools/radio_generate_content.py, reading
tools/radio_content_plan.py -- STATIONS, AD_PLAN, SONG_PLAN):
    uv run python tools/generate_radio.py --plan-hosts            # every station's host lines
    uv run python tools/generate_radio.py --plan-hosts roadhouse  # one station's host lines
    uv run python tools/generate_radio.py --plan-ids               # spoken IDs + jingles, every station
    uv run python tools/generate_radio.py --plan-ids roadhouse    # one station's ID + jingles
    uv run python tools/generate_radio.py --plan-ads                # the shared ad rotation
    uv run python tools/generate_radio.py --plan-songs oldies       # one song pool
    uv run python tools/generate_radio.py --plan-songs oldies --limit 3  # cap fresh spend, resume later
    uv run python tools/generate_radio.py --plan-hosts --force      # regenerate even if the file exists
    uv run python tools/generate_radio.py --probe                   # measure real per-song credit cost
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import tempfile
import urllib.error
import urllib.parse
import urllib.request
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from generate_sounds import ASSETS, _api_key, stash_master  # noqa: E402

# Current ElevenLabs models (2026-10-08). Eleven v4 is the expressive TTS
# model (same audio tags and 1x character cost as v3, which carried the
# first two waves); music_v1 is deprecated in favour of music_v2_5. v3
# only accepted stability at 0.0, 0.5 or 1.0 (creative, natural, robust)
# and had no ``style`` knob, so the delivery recipe is the natural setting
# plus the same similarity the v2 reads used.
TTS_MODEL = "eleven_v4"
MUSIC_MODEL = "music_v2_5"
TTS_VOICE_SETTINGS = {"stability": 0.5, "similarity_boost": 0.75, "use_speaker_boost": True}

MUSIC_API = "https://api.elevenlabs.io/v1/music?output_format=mp3_44100_128"
TTS_API = "https://api.elevenlabs.io/v1/text-to-speech/{voice_id}?output_format=mp3_44100_128"
VOICES_API = "https://api.elevenlabs.io/v1/voices"
LIBRARY_SEARCH_API = "https://api.elevenlabs.io/v1/shared-voices?search={query}&page_size=5"
ADD_VOICE_API = "https://api.elevenlabs.io/v1/voices/add/{public_user_id}/{voice_id}"
SFX_API = "https://api.elevenlabs.io/v1/sound-generation"
VOICE_CACHE = Path(__file__).resolve().parent / ".radio_voices.json"

# key -> (prompt, length_ms, force_instrumental)
MUSIC_SPECS: dict[str, tuple[str, int, bool]] = {
    # Country pool: fictional heartland stations
    "radio_country_backroads": (
        "Modern outlaw country song about driving a semi truck down two-lane "
        "backroads at sunrise, warm male vocals, telecaster twang, steady "
        "trucker shuffle beat, radio-friendly mix",
        150_000,
        False,
    ),
    "radio_country_two_lane": (
        "Easygoing classic country song about small towns rolling past a "
        "truck window, gentle male vocals, pedal steel guitar, brushed drums, "
        "warm nostalgic AM radio feel",
        150_000,
        False,
    ),
    "radio_country_diesel_heart": (
        "Upbeat country rock song about a diesel engine and the open "
        "interstate, confident female vocals, fiddle and electric guitar, "
        "driving rhythm, modern country radio sound",
        150_000,
        False,
    ),
    # Classic rock pool: city rock stations
    "radio_rock_open_throttle": (
        "Classic rock anthem about hauling freight through the night, "
        "gritty male vocals, crunchy electric guitars, driving drums, "
        "seventies highway rock energy, radio mix",
        150_000,
        False,
    ),
    "radio_rock_night_shift": (
        "Mid-tempo classic rock song about working the night shift on the "
        "road, soulful male vocals, hammond organ and electric guitar, "
        "steady groove, FM rock radio feel",
        150_000,
        False,
    ),
    "radio_rock_chrome_horizon": (
        "Melodic heartland rock song about chrome wheels and a wide open "
        "horizon, earnest male vocals, jangly guitars, big chorus, "
        "eighties arena rock radio sound",
        150_000,
        False,
    ),
    # Blues and soul pool: southern stations
    "radio_blues_delta_mile": (
        "Slow electric delta blues song about one more mile before home, "
        "weathered male vocals, slide guitar, sparse drums, late evening "
        "juke joint atmosphere",
        150_000,
        False,
    ),
    "radio_blues_crossroad_coffee": (
        "Warm soul blues song about coffee at a crossroads diner, smooth "
        "female vocals, horns and electric piano, laid back groove, "
        "southern soul radio feel",
        150_000,
        False,
    ),
    # Extra late-night bed for the Night Line rotation
    "radio_night_low_beams": (
        "Slow instrumental late night jazz for empty interstate driving, "
        "muted trumpet, soft brushed drums, upright bass, sparse electric "
        "piano, lonely and calm, no vocals",
        180_000,
        True,
    ),
}

# Roadhouse: warm gravelly daytime trucker DJ. Night Line: calm late-night voice.
HOST_VOICE_PREFERENCES = {
    "roadhouse": ("Brian", "Bill", "George", "Adam", "Daniel"),
    "nightline": ("Matilda", "Alice", "Sarah", "Rachel", "Charlotte"),
}

HOST_LINES: dict[str, tuple[str, ...]] = {
    "roadhouse": (
        "You're rolling with the Freight Fate Roadhouse, coast to coast, "
        "wherever the load takes you. Keep it between the lines, driver.",
        "That rig of yours sounds hungry for miles. More music coming right up on the Roadhouse.",
        "This one goes out to everybody staring down a long white line this "
        "morning. Hammer down, stay safe.",
        "Roadhouse radio, friend of the working driver. Check your mirrors, "
        "check your coffee, and roll on.",
        "If you're hauling through weather out there, take her slow. The "
        "Roadhouse will keep you company all the way.",
        "From the yard to the receiver and every mile between, this is the "
        "Freight Fate Roadhouse. Back to the music.",
    ),
    "nightline": (
        "This is the Night Line. Just you, me, and a few hundred miles of quiet highway.",
        "For every driver watching the small hours roll past the "
        "windshield, stay sharp out there. The Night Line's with you.",
        "You're on the Night Line, playing it slow and low until the sun finds you.",
        "If your eyes are getting heavy, find some parking and let the bunk "
        "win. The music will still be here.",
        "Somewhere out there a reefer hums in a dark lot and the coffee's "
        "gone cold. This one's for the long haul.",
        "Night Line time. Dim the dash lights, ease your shoulders, and let the miles pour.",
    ),
}


def _post_bytes(url: str, key: str, body: dict, timeout: int = 600) -> bytes:
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode("utf-8"),
        headers={
            "xi-api-key": key,
            "Content-Type": "application/json",
            "Accept": "audio/mpeg",
        },
    )
    with urllib.request.urlopen(req, timeout=timeout) as resp:
        cost = resp.headers.get("character-cost")
        if cost:
            print(f"    cost {cost} credits", flush=True)
        return resp.read()


def _write_ogg(mp3: bytes, out: Path) -> None:
    stash_master(mp3, out.stem)
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(suffix=".mp3", delete=False) as tmp:
        tmp.write(mp3)
        tmp_path = tmp.name
    try:
        subprocess.run(
            [
                "ffmpeg",
                "-y",
                "-loglevel",
                "error",
                "-i",
                tmp_path,
                "-c:a",
                "libvorbis",
                "-q:a",
                "5",
                str(out),
            ],
            check=True,
        )
    finally:
        os.unlink(tmp_path)
    print(f"    wrote {out} ({out.stat().st_size:,} bytes)", flush=True)


def _pick_voice(key: str, station: str) -> tuple[str, str]:
    req = urllib.request.Request(VOICES_API, headers={"xi-api-key": key})
    with urllib.request.urlopen(req, timeout=60) as resp:
        voices = json.load(resp).get("voices", [])
    by_name = {v.get("name", ""): v.get("voice_id", "") for v in voices}
    for name in HOST_VOICE_PREFERENCES[station]:
        if by_name.get(name):
            return name, by_name[name]
    if voices:
        v = voices[0]
        return v.get("name", "unknown"), v.get("voice_id", "")
    raise SystemExit("No ElevenLabs voices available on this account")


def _match_account_voice(candidate: str, voices: list[dict]) -> str | None:
    """Case-insensitive match against an account voice list.

    Account voices carry descriptive display names ("Clyde - Radio
    Announcer"), so the primary match is on the name prefix before a
    " - " separator; an exact full-name match is the fallback tier.
    """
    target = candidate.strip().casefold()
    for v in voices:
        prefix = v.get("name", "").split(" - ", 1)[0].strip()
        if prefix.casefold() == target:
            return v.get("voice_id", "")
    for v in voices:
        if v.get("name", "").strip().casefold() == target:
            return v.get("voice_id", "")
    return None


def _search_library(key: str, name: str) -> list[dict]:
    req = urllib.request.Request(
        LIBRARY_SEARCH_API.format(query=urllib.parse.quote(name)),
        headers={"xi-api-key": key},
    )
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.load(resp).get("voices", [])


def _find_in_library(key: str, name: str) -> dict | None:
    for hit in _search_library(key, name):
        if hit.get("name", "").lower() == name.lower():
            return hit
    return None


def _add_from_library(key: str, name: str) -> str:
    hit = _find_in_library(key, name)
    if not hit:
        print(f"  library search found nothing usable for '{name}'", flush=True)
        return ""
    owner_id = hit.get("public_owner_id", "")
    body = {"new_name": name}
    add = urllib.request.Request(
        ADD_VOICE_API.format(public_user_id=owner_id, voice_id=hit["voice_id"]),
        data=json.dumps(body).encode(),
        headers={"xi-api-key": key, "Content-Type": "application/json"},
    )
    with urllib.request.urlopen(add, timeout=60) as resp:
        return json.load(resp).get("voice_id", "")


def provision_voices(key: str, *, dry_run: bool = False) -> dict[str, str]:
    """Ensure every cast voice exists on the account; add from the shared
    library when missing. Returns name -> voice_id.

    In dry-run mode, the account voice list and the library search are both
    read-only GETs; the add-voice endpoint is never called and nothing is
    cached, so it is safe to run against the owner's real ElevenLabs account
    at any time.
    """
    from radio_content_plan import AD_PLAN, STATIONS

    wanted: dict[str, tuple[str, ...]] = {}
    for plan in STATIONS.values():
        wanted[plan.voice] = plan.voice_fallbacks
    for ad in AD_PLAN:
        wanted.setdefault(ad.voice, ())

    req = urllib.request.Request(VOICES_API, headers={"xi-api-key": key})
    with urllib.request.urlopen(req, timeout=60) as resp:
        voices = json.load(resp).get("voices", [])

    resolved: dict[str, str] = {}
    for name, fallbacks in wanted.items():
        # Tier 1: the primary cast name is already on the account.
        voice_id = _match_account_voice(name, voices)
        if voice_id:
            resolved[name] = voice_id
            print(f"  {name} -> {voice_id}", flush=True)
            continue

        # Tier 2: the primary cast name is addable from the shared library.
        # A library add for the exact intended voice outranks settling for
        # an on-account fallback -- fallbacks are a last resort, not a
        # shortcut around an approved library add.
        if dry_run:
            hit = _find_in_library(key, name)
            if hit:
                print(
                    f"  {name} -> WOULD ADD from library "
                    f"(match: {hit.get('name')}, category: {hit.get('category', 'unknown')})",
                    flush=True,
                )
                continue
        else:
            added = _add_from_library(key, name)
            if added:
                resolved[name] = added
                print(f"  {name} -> {added} (added from library)", flush=True)
                continue

        # Tier 3: fall back to whatever's already on the account.
        fallback = None
        for candidate in fallbacks:
            fallback_id = _match_account_voice(candidate, voices)
            if fallback_id:
                fallback = (candidate, fallback_id)
                break
        if fallback:
            candidate, voice_id = fallback
            resolved[name] = voice_id
            print(f"  {name} -> {voice_id} (resolved via fallback {candidate})", flush=True)
            continue

        if dry_run:
            print(f"  {name} -> NOT FOUND anywhere", flush=True)
        else:
            raise SystemExit(f"No voice found for cast '{name}' -- adjust the plan")

    if not dry_run:
        VOICE_CACHE.write_text(
            json.dumps(resolved, indent=2, sort_keys=True) + "\n", encoding="utf-8"
        )
        print(f"  cached voice map to {VOICE_CACHE}", flush=True)
    return resolved


def generate_music(key: str, wanted: list[str]) -> None:
    for spec_key in wanted:
        prompt, length_ms, instrumental = MUSIC_SPECS[spec_key]
        print(f"  composing {spec_key} ({length_ms / 1000:.0f}s)...", flush=True)
        body = {
            "prompt": prompt,
            "music_length_ms": length_ms,
            "model_id": MUSIC_MODEL,
            "force_instrumental": instrumental,
        }
        try:
            mp3 = _post_bytes(MUSIC_API, key, body)
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", "ignore")[:300]
            print(f"    FAILED {spec_key}: HTTP {exc.code} {detail}", flush=True)
            continue
        _write_ogg(mp3, ASSETS / "music" / f"{spec_key}.ogg")


def generate_hosts(key: str) -> None:
    for station, lines in HOST_LINES.items():
        name, voice_id = _pick_voice(key, station)
        print(f"  {station} host voice: {name}", flush=True)
        for i, line in enumerate(lines, start=1):
            out = ASSETS / "music" / f"host_{station}_{i:02d}.ogg"
            print(f"  speaking host_{station}_{i:02d}...", flush=True)
            body = {
                "text": line,
                "model_id": TTS_MODEL,
                "voice_settings": TTS_VOICE_SETTINGS,
            }
            try:
                mp3 = _post_bytes(TTS_API.format(voice_id=voice_id), key, body, timeout=180)
            except urllib.error.HTTPError as exc:
                detail = exc.read().decode("utf-8", "ignore")[:300]
                print(f"    FAILED host_{station}_{i:02d}: HTTP {exc.code} {detail}", flush=True)
                continue
            _write_ogg(mp3, out)


def _fm_hiss(rng, rate: int, seconds: float):
    """Shaped FM interstation noise; the shared recipe for all fringe assets.

    FM fringe noise is not AM crackle -- the limiter rejects impulse
    noise (owner's ham-ear ruling 2026-07-23). What a real receiver
    plays between stations is the demodulator's triangular noise
    spectrum (+6 dB/octave) rolled off by the 75 microsecond
    de-emphasis network: a full, smooth frying hiss with body around
    1-2 kHz, no pops. A second gentle pole (~5 kHz) stands in for the
    receiver's audio stage: pure post-de-emphasis noise is rise-then-
    flat and reads thin and digital on full-range playback.
    """
    import numpy as np

    noise = rng.normal(0.0, 1.0, int(rate * seconds))
    # FM demod noise rises 6 dB/octave: a differentiator is exactly that.
    shaped = np.diff(noise, prepend=0.0)
    for rc in (75e-6, 1.0 / (2.0 * np.pi * 5000.0)):
        alpha = (1.0 / rate) / (rc + 1.0 / rate)
        acc = 0.0
        out_buf = np.empty_like(shaped)
        for i, x in enumerate(shaped):
            acc += alpha * (x - acc)
            out_buf[i] = acc
        shaped = out_buf
    return shaped


def _write_asset(sample, rate: int, relpath: str) -> None:
    import soundfile as sf

    out = ASSETS / relpath
    out.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(suffix=".wav", delete=False) as tmp:
        tmp_path = tmp.name
    try:
        sf.write(tmp_path, sample.astype("float32"), rate, format="WAV")
        subprocess.run(
            [
                "ffmpeg",
                "-y",
                "-loglevel",
                "error",
                "-i",
                tmp_path,
                "-c:a",
                "libvorbis",
                str(out),
            ],
            check=True,
        )
    finally:
        os.unlink(tmp_path)
    print(f"    wrote {out} ({out.stat().st_size:,} bytes)", flush=True)


def generate_static() -> None:
    """Procedural FM interstation hiss burst; no API credits."""
    import numpy as np

    rng = np.random.default_rng(1290)
    rate = 44100
    seconds = 2.4
    shaped = _fm_hiss(rng, rate, seconds)
    t = np.linspace(0.0, seconds, shaped.size)
    # slow AGC-like wander, subtle: +/-20 percent over ~1 Hz, never impulsive
    wander = 1.0 + 0.2 * np.sin(2.0 * np.pi * (0.7 * t + 0.3 * np.sin(2.0 * np.pi * 0.23 * t)))
    fade = np.minimum(1.0, np.minimum(t / 0.05, (seconds - t) / 0.4))
    sample = shaped * wander * fade
    sample = 0.8 * sample / np.max(np.abs(sample))
    _write_asset(sample, rate, "radio/static_burst.ogg")


def generate_fringe() -> None:
    """FM fringe runtime kit: a seamless hiss-bed loop and the picket bank.

    The bed loops on its own channel with distance-scaled gain; pickets are
    short sharp-edged splashes (FM capture is a threshold -- the gating is
    abrupt, owner ruling 2026-07-23) fired at the Rayleigh-flutter rate by
    the driving state. Edges: ~3 ms attack, ~12 ms release; anything softer
    smears the picket, anything shorter clicks.
    """
    import numpy as np

    rate = 44100
    rng = np.random.default_rng(4471)
    # Bed: generate long, then splice the tail over the head (equal-power,
    # 50 ms) so the loop point is seamless in a noise signal.
    seconds = 4.0
    xfade = int(0.05 * rate)
    raw = _fm_hiss(rng, rate, seconds + 0.05)
    head = raw[:xfade].copy()
    body = raw[xfade : xfade + int(seconds * rate)].copy()
    ramp = np.linspace(0.0, 1.0, xfade)
    body[-xfade:] = body[-xfade:] * np.sqrt(1.0 - ramp) + head * np.sqrt(ramp)
    body = 0.7 * body / np.max(np.abs(body))
    _write_asset(body, rate, "radio/fm_hiss_loop.ogg")

    durations = (0.05, 0.07, 0.09, 0.11, 0.13, 0.16)
    for i, dur in enumerate(durations, start=1):
        splash = _fm_hiss(rng, rate, dur)
        n = splash.size
        attack = int(0.003 * rate)
        release = int(0.012 * rate)
        env = np.ones(n)
        env[:attack] = np.linspace(0.0, 1.0, attack)
        env[-release:] = np.linspace(1.0, 0.0, release)
        splash = splash * env
        splash = 0.85 * splash / np.max(np.abs(splash))
        _write_asset(splash, rate, f"radio/picket_{i:02d}.ogg")


# --- Imaging post-production chain -----------------------------------------
#
# Loudness-match contract: imaging elements (station-ID voice reads, ad
# reads, and the SFX beds mixed under them) all normalize to the same RMS
# target so they sit level against the baked music beds without a runtime
# ducking surprise. If the station music generation path (generate_music)
# ever grows its own loudness pass, target this same -16 dBFS RMS so music
# and imaging share one loudness floor.
TARGET_RMS_DBFS = -16.0

# Per-key SFX duration in seconds, matched to what each prompt in
# radio_content_plan.SFX_PROMPTS actually states: "about half a second"
# means 0.6s, not a rounded-up 2s. The API accepts duration_seconds from
# 0.5 to 22; prompts with no explicit length ("very short decay", "short
# rock stinger", "fading out") get a duration sized to their described
# character instead of a placeholder. Unlisted keys fall back to 1.5s.
SFX_DURATIONS: dict[str, float] = {
    "radio_imaging_whoosh_short": 0.6,  # "about half a second"
    "radio_imaging_whoosh_long": 2.0,  # "about two seconds"
    "radio_imaging_impact": 0.6,  # "very short decay"
    "radio_imaging_riser": 1.2,  # "One second riser" + a beat to cut clean
    "radio_imaging_stinger": 1.5,  # "short" chord + "a quick reverb tail"
    "radio_imaging_shimmer": 2.5,  # bell tones "fading out", the longest of the set
}


def _rms_dbfs(samples) -> float:
    if samples.size == 0:
        return -120.0
    import numpy as np

    rms = float(np.sqrt(np.mean(np.square(samples))))
    return 20.0 * np.log10(rms) if rms > 0.0 else -120.0


def _normalize_to_target(samples, target_dbfs: float = TARGET_RMS_DBFS, peak_cap: float = 0.95):
    """RMS-target normalize, then cap true peak at ``peak_cap``.

    RMS normalization first (matches perceived loudness across elements
    with very different crest factors -- a reverb tail vs. a dry read),
    then a peak cap so an occasional transient never clips.

    Returns ``(samples, peak_limited)``. A high-crest-factor input (sparse
    loud peaks over a quiet floor) can ask for more gain than the peak cap
    allows, landing the achieved RMS below ``target_dbfs`` -- that's what
    ``peak_limited`` flags, so a caller with an asset label can report it
    instead of it going unnoticed until a listen-check.
    """
    import numpy as np

    if samples.size == 0:
        return samples, False
    current = _rms_dbfs(samples)
    gain = 10.0 ** ((target_dbfs - current) / 20.0)
    out = samples * gain
    peak = float(np.max(np.abs(out))) if out.size else 0.0
    peak_limited = peak > peak_cap
    if peak_limited:
        out = out * (peak_cap / peak)
    return out, peak_limited


def _edge_fade(samples, rate: int, *, attack_s: float = 0.003, release_s: float = 0.012):
    """Short attack/release taper at both edges of the final output.

    Same convention as generate_fringe's picket splashes: 3 ms attack,
    12 ms release -- anything softer smears the edge, anything shorter
    clicks. Applied last, before loudness normalization, exactly like the
    fringe splashes fade then peak-normalize.
    """
    import numpy as np

    n = samples.size
    if n < 2:
        return samples
    attack = min(int(attack_s * rate), n // 2)
    release = min(int(release_s * rate), n - attack)
    env = np.ones(n, dtype=np.float64)
    if attack > 0:
        env[:attack] = np.linspace(0.0, 1.0, attack)
    if release > 0:
        env[-release:] = np.linspace(1.0, 0.0, release)
    return samples * env


def _biquad(samples, b0: float, b1: float, b2: float, a0: float, a1: float, a2: float):
    """Direct Form I biquad, sample-by-sample like the file's other
    hand-rolled filters (_fm_hiss). Cheap enough at imaging-element
    lengths (a few seconds); not meant for long-form audio.
    """
    import numpy as np

    b0, b1, b2, a1, a2 = b0 / a0, b1 / a0, b2 / a0, a1 / a0, a2 / a0
    out = np.empty_like(samples)
    x1 = x2 = y1 = y2 = 0.0
    for i, x in enumerate(samples):
        y = b0 * x + b1 * x1 + b2 * x2 - a1 * y1 - a2 * y2
        out[i] = y
        x2, x1 = x1, x
        y2, y1 = y1, y
    return out


def _highpass_biquad(samples, rate: int, freq: float, q: float = 0.707):
    """RBJ audio-EQ-cookbook high-pass; used at ~120 Hz to clear rumble."""
    import numpy as np

    w0 = 2.0 * np.pi * freq / rate
    alpha = np.sin(w0) / (2.0 * q)
    cosw0 = np.cos(w0)
    b0 = (1.0 + cosw0) / 2.0
    b1 = -(1.0 + cosw0)
    b2 = (1.0 + cosw0) / 2.0
    a0 = 1.0 + alpha
    a1 = -2.0 * cosw0
    a2 = 1.0 - alpha
    return _biquad(samples, b0, b1, b2, a0, a1, a2)


def _peaking_biquad(samples, rate: int, freq: float, gain_db: float, q: float = 1.0):
    """RBJ audio-EQ-cookbook peaking filter; used as a ~3 kHz presence lift."""
    import numpy as np

    a = 10.0 ** (gain_db / 40.0)
    w0 = 2.0 * np.pi * freq / rate
    alpha = np.sin(w0) / (2.0 * q)
    cosw0 = np.cos(w0)
    b0 = 1.0 + alpha * a
    b1 = -2.0 * cosw0
    b2 = 1.0 - alpha * a
    a0 = 1.0 + alpha / a
    a1 = -2.0 * cosw0
    a2 = 1.0 - alpha / a
    return _biquad(samples, b0, b1, b2, a0, a1, a2)


def _soft_knee_compress(
    samples,
    rate: int,
    *,
    threshold_db: float = -18.0,
    ratio: float = 3.0,
    knee_db: float = 6.0,
    attack_s: float = 0.005,
    release_s: float = 0.08,
    makeup_db: float = 6.0,
):
    """Feed-forward soft-knee compressor: an attack/release-smoothed level
    envelope drives a soft-knee gain curve (Giannoulis et al. digital
    dynamic-range-compressor formula), applied back onto the signal.
    """
    import numpy as np

    eps = 1e-9
    level_db = 20.0 * np.log10(np.abs(samples) + eps)
    attack = np.exp(-1.0 / (rate * attack_s))
    release = np.exp(-1.0 / (rate * release_s))
    env_db = np.empty_like(level_db)
    prev = float(level_db[0]) if level_db.size else -120.0
    for i, x in enumerate(level_db):
        coeff = attack if x > prev else release
        prev = coeff * prev + (1.0 - coeff) * x
        env_db[i] = prev

    over2 = 2.0 * (env_db - threshold_db)
    knee_out = env_db + (1.0 / ratio - 1.0) * np.square(env_db - threshold_db + knee_db / 2.0) / (
        2.0 * knee_db
    )
    above_out = threshold_db + (env_db - threshold_db) / ratio
    compressed_db = np.where(
        over2 < -knee_db, env_db, np.where(over2 <= knee_db, knee_out, above_out)
    )
    gain_db = (compressed_db - env_db) + makeup_db
    return samples * (10.0 ** (gain_db / 20.0))


def _detuned_double(
    samples, rate: int, seed: int, *, delay_s: float = 0.015, level_db: float = -6.0
):
    """A 15 ms detuned copy at -6 dB under the dry signal (a classic
    broadcast-imaging "double" for a fuller, wider voice read). Detune
    direction/amount is seeded so it is deterministic per station ID.
    """
    import numpy as np

    rng = np.random.default_rng(seed)
    cents = float(rng.uniform(8.0, 15.0)) * (1.0 if rng.integers(0, 2) else -1.0)
    ratio = 2.0 ** (cents / 1200.0)
    n = samples.size
    src_idx = np.clip(np.arange(n) * ratio, 0, n - 1)
    detuned = np.interp(src_idx, np.arange(n), samples) * (10.0 ** (level_db / 20.0))

    delay_n = int(delay_s * rate)
    total = n + delay_n
    out = np.zeros(total, dtype=np.float64)
    out[:n] += samples
    out[delay_n : delay_n + n] += detuned
    return out


def _fft_convolve(a, b):
    """Full linear convolution via FFT -- same result as np.convolve(mode=
    "full") but fast enough for a ~0.25 s reverb tail against several
    seconds of dry signal.
    """
    import numpy as np

    n = a.size + b.size - 1
    size = 1
    while size < n:
        size *= 2
    fa = np.fft.rfft(a, size)
    fb = np.fft.rfft(b, size)
    return np.fft.irfft(fa * fb, size)[:n]


def _short_reverb(samples, rate: int, seed: int, *, tail_s: float = 0.25, wet: float = 0.18):
    """Short, bright seeded reverb: convolve the dry signal with an
    exponentially-decaying noise impulse response. Not a physical room
    model -- a cheap, deterministic "imaging sheen" tail.
    """
    import numpy as np

    rng = np.random.default_rng(seed + 1)
    n = max(1, int(rate * tail_s))
    t = np.arange(n) / rate
    ir = rng.normal(0.0, 1.0, n) * np.exp(-t / (tail_s / 5.0))
    ir = ir / (np.sqrt(np.sum(np.square(ir))) + 1e-9)

    wet_signal = _fft_convolve(samples, ir)
    out = np.zeros(max(samples.size, wet_signal.size), dtype=np.float64)
    out[: samples.size] += samples
    out[: wet_signal.size] += wet * wet_signal
    return out


def imaging_process(
    samples, rate: int, seed: int, *, doubled: bool = True, label: str | None = None
):
    """Broadcast-style post-production chain for station-ID voice reads.

    High-pass at ~120 Hz clears rumble, a ~3 kHz presence peak keeps the
    read cutting over a music bed, soft-knee compression evens out the
    delivery, an optional 15 ms detuned double widens it, and a short
    seeded reverb tail adds sheen -- then a 3 ms/12 ms edge fade (the
    generate_fringe convention) and an RMS-target normalize to
    TARGET_RMS_DBFS with a 0.95 peak cap (see the loudness-match contract
    above). Deterministic: the same seed on the same input always
    produces byte-identical output, so a rebuild without a plan change
    reproduces the exact same asset.

    If ``label`` is given, prints the achieved RMS dBFS for that asset
    (flagging "(peak-limited)" when the peak cap kept it under target) --
    a one-line note runners can surface so a quiet spot is caught at
    generation time instead of only at a Task 5 listen-check.
    """
    import numpy as np

    samples = np.asarray(samples, dtype=np.float64)
    out = _highpass_biquad(samples, rate, 120.0)
    out = _peaking_biquad(out, rate, 3000.0, gain_db=4.0, q=1.0)
    out = _soft_knee_compress(out, rate)
    if doubled:
        out = _detuned_double(out, rate, seed)
    out = _short_reverb(out, rate, seed)
    out = _edge_fade(out, rate)
    out, peak_limited = _normalize_to_target(out)
    if label is not None:
        note = " (peak-limited)" if peak_limited else ""
        print(f"    {label}: RMS {_rms_dbfs(out):.1f} dBFS{note}", flush=True)
    return out.astype(np.float32)


def broadcast_compress(samples, rate: int, *, label: str | None = None):
    """Light broadcast chain for ad reads: gentle soft-knee compression
    only (no EQ, no doubling, no reverb -- ad copy needs to read clean,
    not sound like a station ID), a 3 ms/12 ms edge fade, then an
    RMS-target normalize to the same TARGET_RMS_DBFS as imaging_process
    (the loudness-match contract above) so ads sit level with station IDs
    and the music beds around them.

    ``label``, if given, prints the achieved RMS dBFS the same way as
    imaging_process -- real speech can have enough crest factor that the
    peak cap holds the result under target; the print makes that visible
    per asset instead of silent.
    """
    import numpy as np

    samples = np.asarray(samples, dtype=np.float64)
    out = _soft_knee_compress(
        samples, rate, threshold_db=-20.0, ratio=2.0, knee_db=6.0, makeup_db=4.0
    )
    out = _edge_fade(out, rate)
    out, peak_limited = _normalize_to_target(out)
    if label is not None:
        note = " (peak-limited)" if peak_limited else ""
        print(f"    {label}: RMS {_rms_dbfs(out):.1f} dBFS{note}", flush=True)
    return out.astype(np.float32)


def mix_id_bed(voice, sfx_layers: dict, rate: int):
    """Mix a station-ID voice read (already run through imaging_process)
    against optional SFX layers: a "whoosh" laid under the voice head
    (starting at time 0) and a "riser" timed to land into the voice's
    tail. Missing keys are simply skipped. A 3 ms/12 ms edge fade (the
    generate_fringe convention) runs before the final mix is
    peak-normalized to 0.9, leaving headroom for whatever plays next in
    the rotation.
    """
    import numpy as np

    voice = np.asarray(voice, dtype=np.float64)
    whoosh = sfx_layers.get("whoosh")
    riser = sfx_layers.get("riser")

    total = voice.size
    if whoosh is not None:
        total = max(total, whoosh.size)
    riser_start = max(0, voice.size - riser.size) if riser is not None else 0
    if riser is not None:
        total = max(total, riser_start + riser.size)

    mix = np.zeros(total, dtype=np.float64)
    mix[: voice.size] += voice
    if whoosh is not None:
        mix[: whoosh.size] += np.asarray(whoosh, dtype=np.float64) * 0.5
    if riser is not None:
        mix[riser_start : riser_start + riser.size] += np.asarray(riser, dtype=np.float64) * 0.5

    mix = _edge_fade(mix, rate)
    peak = float(np.max(np.abs(mix))) if mix.size else 0.0
    if peak > 0.0:
        mix = mix * (0.9 / peak)
    return mix.astype(np.float32)


def generate_sfx(key: str, prompts: dict[str, str]) -> None:
    """POST each radio_content_plan.SFX_PROMPTS entry to the ElevenLabs
    Sound Effects API and write assets/radio/imaging/<key>.ogg via the
    project's shared ogg convention (_write_ogg -> ffmpeg). Spends
    credits -- only ever invoked behind the --sfx CLI flag, never as part
    of the default "everything" run.
    """
    for spec_key, prompt in prompts.items():
        duration = SFX_DURATIONS.get(spec_key, 1.5)
        print(f"  requesting sfx {spec_key} ({duration:.1f}s)...", flush=True)
        body = {"text": prompt, "duration_seconds": duration}
        try:
            mp3 = _post_bytes(SFX_API, key, body, timeout=120)
        except urllib.error.HTTPError as exc:
            detail = exc.read().decode("utf-8", "ignore")[:300]
            print(f"    FAILED {spec_key}: HTTP {exc.code} {detail}", flush=True)
            continue
        _write_ogg(mp3, ASSETS / "radio" / "imaging" / f"{spec_key}.ogg")


def report_durations() -> None:
    import soundfile as sf

    print("\nMeasured durations (paste into music.py):")
    paths: list[Path] = []
    for pattern in ("radio_*.ogg", "host_*.ogg", "id_*.ogg", "ad_*.ogg"):
        paths.extend(sorted((ASSETS / "music").glob(pattern)))
    for path in paths:
        info = sf.info(str(path))
        print(f"  {path.stem}: {info.frames / info.samplerate:.1f}s")


def _take_flag(argv: list[str], flag: str) -> bool:
    """Pop a plain boolean flag out of ``argv`` in place -- no value
    consumption, so a stray token after it is left for the caller to
    reject instead of being silently swallowed."""
    if flag not in argv:
        return False
    argv.remove(flag)
    return True


def _take_flag_arg(argv: list[str], flag: str) -> tuple[bool, str | None]:
    """Pop ``flag`` out of ``argv`` in place; if the flag is present and the
    token right after it isn't itself a ``--flag``, pop and return that too
    as the flag's optional value (a station key or song pool)."""
    if flag not in argv:
        return False, None
    i = argv.index(flag)
    argv.pop(i)
    if i < len(argv) and not argv[i].startswith("--"):
        return True, argv.pop(i)
    return True, None


def main(argv: list[str] | None = None) -> int:
    argv = list(sys.argv[1:] if argv is None else argv)

    plan_hosts, plan_hosts_key = _take_flag_arg(argv, "--plan-hosts")
    plan_ids, plan_ids_key = _take_flag_arg(argv, "--plan-ids")
    plan_ads = _take_flag(argv, "--plan-ads")
    plan_songs, plan_songs_pool = _take_flag_arg(argv, "--plan-songs")
    probe = _take_flag(argv, "--probe")
    force = _take_flag(argv, "--force")
    has_limit, limit_str = _take_flag_arg(argv, "--limit")
    limit: int | None = None
    if has_limit:
        if not plan_songs:
            raise SystemExit("--limit is only meaningful together with --plan-songs")
        if limit_str is None:
            raise SystemExit("--limit requires a value, e.g. --limit 3")
        try:
            parsed_limit = int(limit_str)
        except ValueError:
            parsed_limit = 0  # falls through to the shared "positive integer" error below
        if parsed_limit <= 0:
            raise SystemExit(f"--limit requires a positive integer, got {limit_str!r}")
        limit = parsed_limit

    if plan_hosts or plan_ids or plan_ads or plan_songs or probe:
        if plan_songs and not plan_songs_pool:
            raise SystemExit("--plan-songs requires a POOL argument, e.g. --plan-songs oldies")
        if argv:
            raise SystemExit(
                f"Unrecognized argument(s) with a --plan-*/--probe flag: {' '.join(argv)}"
            )
        from radio_generate_content import (
            run_plan_ads,
            run_plan_hosts,
            run_plan_ids,
            run_plan_songs,
            run_probe,
        )

        key = _api_key()
        if plan_hosts:
            run_plan_hosts(key, plan_hosts_key, force=force)
        if plan_ids:
            run_plan_ids(key, plan_ids_key, force=force)
        if plan_ads:
            run_plan_ads(key, force=force)
        if plan_songs:
            run_plan_songs(key, plan_songs_pool, force=force, limit=limit)
        if probe:
            run_probe(key, force=force)
        report_durations()
        return 0

    flags = {arg for arg in argv if arg.startswith("--")}
    keys = [arg for arg in argv if not arg.startswith("--")]
    if "--voices" in flags:
        dry_run = "--dry-run" in flags
        key = _api_key()
        if dry_run:
            print("Dry run: read-only account + library lookups, nothing added.", flush=True)
        resolved = provision_voices(key, dry_run=dry_run)
        if not dry_run:
            print(f"\n{len(resolved)} voice(s) resolved and cached.", flush=True)
        return 0
    if "--sfx" in flags:
        from radio_content_plan import SFX_PROMPTS

        key = _api_key()
        generate_sfx(key, SFX_PROMPTS)
        return 0
    do_all = not flags and not keys
    if keys:
        unknown = [k for k in keys if k not in MUSIC_SPECS]
        if unknown:
            raise SystemExit(f"Unknown music keys: {', '.join(unknown)}")
    if "--static" in flags or do_all:
        generate_static()
    if "--fringe" in flags or do_all:
        generate_fringe()
    needs_api = do_all or "--music" in flags or "--hosts" in flags or keys
    if needs_api:
        key = _api_key()
        if "--hosts" in flags or do_all:
            generate_hosts(key)
        if "--music" in flags or do_all or keys:
            generate_music(key, keys or list(MUSIC_SPECS))
    report_durations()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
