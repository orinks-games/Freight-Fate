#!/usr/bin/env python3
"""Tire whine on cross-grooved concrete and the hum of a steel grate bridge deck.

Both are made from sine tones and shaped noise, so there is no recording and
no license to carry. Every tone runs a whole number of cycles and every
noise band is built in the frequency domain, so each loop is exactly
periodic: it loops with no click and needs no crossfade.

Each loop is made for one reference speed. The game plays it at
rate = speed / reference speed, so the pitch follows the truck the way the
real sound does (pitch = speed / groove spacing).

  whine: reference 65 mph (29.06 m/s)
    a  13 mm grooves  2235 Hz   FHWA T 5040.36, the 13 mm random pattern
    b  19 mm grooves  1529 Hz   3/4 inch, "used by most agencies" (FHWA-HIF-18-038)
    c  26 mm grooves  1118 Hz   FHWA T 5040.36, the 26 mm random pattern
  grate: reference 45 mph (20.12 m/s)
    a  5 inch cross bars  158 Hz
    b  4 inch cross bars  198 Hz
  The grate bar spacings are assumed: open grid decks come in several
  patterns and no inventory records which one a bridge has.

Usage: synth_road_song.py OUT_DIR    (writes 16-bit mono WAV loops, 8 s each)
Needs numpy only. Encode with: ffmpeg -i x.wav -ac 1 -c:a libvorbis -q:a 5 x.ogg
"""
import sys
import wave

import numpy as np

SR = 44100
SECONDS = 8
N = SR * SECONDS
MPH = 0.44704
WHINE_REF_MPS = 65 * MPH
GRATE_REF_MPS = 45 * MPH

WHINE = {"a": 0.013, "b": 0.019, "c": 0.026}  # groove spacing, metres
GRATE = {"a": 0.127, "b": 0.1016}  # cross bar spacing, metres


def band(freqs, centre, rel_sigma):
    """A Gaussian band of noise amplitude around `centre`."""
    return np.exp(-0.5 * ((freqs - centre) / (centre * rel_sigma)) ** 2)


def periodic_noise(shape, rng):
    """Noise with the amplitude spectrum `shape`, exactly periodic over N."""
    phases = rng.uniform(0, 2 * np.pi, len(shape))
    spec = shape * np.exp(1j * phases)
    spec[0] = 0
    out = np.fft.irfft(spec, N)
    return out / np.sqrt(np.mean(out**2))


def slow_wander(rng, depth_db, top_hz=1.5):
    """A gain curve that drifts by about `depth_db`, periodic over the loop."""
    t = np.arange(N) / SR
    curve = np.zeros(N)
    k_top = int(top_hz * SECONDS)
    for k in range(1, k_top + 1):
        curve += rng.normal() / np.sqrt(k) * np.sin(2 * np.pi * k * t / SECONDS + rng.uniform(0, 6.28))
    curve /= np.max(np.abs(curve))
    return 10 ** (depth_db * curve / 20)


def steady_tone(freq, rng, wander_rad, phase_curve=None):
    """A sine at `freq` whose phase drifts a little, so it has some width
    without the level pumping a narrow band of noise would have. The
    frequency is rounded to a whole number of cycles over the loop."""
    t = np.arange(N) / SR
    freq = round(freq * SECONDS) / SECONDS
    if phase_curve is None:
        phase_curve = phase_wander(rng)
    return np.sin(2 * np.pi * freq * t + wander_rad * phase_curve + rng.uniform(0, 6.28))


def phase_wander(rng, top_hz=4.0):
    """A unit drift curve, periodic over the loop."""
    t = np.arange(N) / SR
    curve = np.zeros(N)
    for k in range(1, int(top_hz * SECONDS) + 1):
        curve += rng.normal() / k * np.sin(2 * np.pi * k * t / SECONDS + rng.uniform(0, 6.28))
    return curve / np.max(np.abs(curve))


def whine(spacing_m, seed):
    rng = np.random.default_rng(seed)
    f = np.fft.rfftfreq(N, 1 / SR)
    f0 = WHINE_REF_MPS / spacing_m
    # The tone itself, steady, with a quieter second tire a hair off it.
    tone = steady_tone(f0, rng, 2.5) + 0.2 * steady_tone(f0 * 1.003, rng, 2.5)
    tone /= np.sqrt(np.mean(tone**2))
    # The skirt random groove spacing smears around it.
    skirt = periodic_noise(band(f, f0, 0.035), rng) * 10 ** (-12 / 20)
    # The second harmonic, weaker.
    second = steady_tone(2 * f0, rng, 5.0) * np.sqrt(2) * 10 ** (-16 / 20)
    # A little tire hiss so the tone does not sit alone.
    hiss_shape = np.where((f > 500) & (f < 6000), 1 / np.maximum(f, 1) ** 0.5, 0)
    hiss = periodic_noise(hiss_shape, rng) * 10 ** (-17 / 20)
    out = (tone + skirt + second + hiss) * slow_wander(rng, 1.0)
    return out / np.sqrt(np.mean(out**2))


def grate(bar_m, seed):
    rng = np.random.default_rng(seed)
    f = np.fft.rfftfreq(N, 1 / SR)
    f0 = GRATE_REF_MPS / bar_m
    out = np.zeros(N)
    # Two axles' worth of the same note, the second a hair flat and quieter,
    # which gives the slow beat a grate deck has.
    drift = (phase_wander(rng), phase_wander(rng))
    for k in range(1, 15):
        fk = f0 * k
        # Harmonics fall away, with a hump from 450 to 900 Hz: the part of
        # the stack that reads as a sung note.
        level = k**-0.9 * (1 + 2.2 * np.exp(-0.5 * ((fk - 650) / 220) ** 2))
        out += level * steady_tone(fk, rng, 1.2 * k, drift[0])
        out += level * 0.45 * steady_tone(fk * 0.996, rng, 1.2 * k, drift[1])
    out /= np.sqrt(np.mean(out**2))
    # The open deck's roar under the note.
    roar_shape = np.where((f > 120) & (f < 2500), 1 / np.maximum(f, 1) ** 0.7, 0)
    out += periodic_noise(roar_shape, rng) * 10 ** (-8 / 20)
    out *= slow_wander(rng, 2.0, top_hz=3.0)
    return out / np.sqrt(np.mean(out**2))


def write(path, x, rms_db=-20.0):
    x = x * 10 ** (rms_db / 20)
    x = np.clip(x, -0.98, 0.98)
    with wave.open(path, "wb") as w:
        w.setnchannels(1)
        w.setsampwidth(2)
        w.setframerate(SR)
        w.writeframes((x * 32767).astype(np.int16).tobytes())


def main():
    out = sys.argv[1]
    for i, (name, spacing) in enumerate(WHINE.items()):
        write(f"{out}/tined_whine_{name}.wav", whine(spacing, 100 + i))
    for i, (name, bar) in enumerate(GRATE.items()):
        write(f"{out}/grate_hum_{name}.wav", grate(bar, 200 + i))


if __name__ == "__main__":
    main()
