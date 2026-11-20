#!/usr/bin/env bash
# Outside ambience beds: two insect choruses for alternate nights, birds by
# day, the river under a bridge, and the sealed (window up) twin of each.
#
# Usage: sound-test/outside_ambience.sh SOURCES_DIR OUT_DIR
#   SOURCES_DIR holds the OpenGameArt originals (see assets/sounds/CREDITS.md):
#     night-a_crickets_1_wolfgang.mp3                   (Wolfgang_, CC0)
#     night-c_forest_night_jcsounds.mp3                 (JC Sounds, CC-BY 4.0)
#     day-a_park_ambience_birds_thimras_first150s.flac  (Thimras, CC0)
#     river-b_stream1_kurt.ogg                          (kurt, CC-BY 3.0)
#   OUT_DIR is an unpacked sound tree (assets/sounds); the files are written
#   to OUT_DIR/ambient/.
#
# A tuning round is a re-run: change a number below, run it, then repack with
# `cargo tools pack-sounds --out assets/sounds.pak`.
set -euo pipefail

SRC=${1:?sources directory}
OUT=${2:?unpacked sound tree}
AMB="$OUT/ambient"
TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

# Loudness targets, RMS dBFS. The old night bed (ambient/night, retired with
# this change) measured -38.5 and played at 1.0 on the Sfx bus (0.8 by
# default); these beds play at 1.0 on the Weather bus (0.65), so they carry
# the 1.8 dB difference and a warm night is as loud as before. The birds match
# it and the river sits 6 dB above.
BED_RMS=-36.7
RIVER_RMS=-30.7

# The window-up muffle (Orinks's pick, 2026-11-19: the heavy chain on every
# bed). A thick glass and a short cab reflection, no level make-up of its own.
# LIGHT, the window demos' chain, is kept for comparison rounds.
LIGHT="highshelf=f=1000:g=-12,lowpass=f=3000"
HEAVY="highshelf=f=1000:g=-16,lowpass=f=2400,lowpass=f=2400,lowshelf=f=100:g=5,aecho=1.0:0.8:2:0.28"
SEALED_insects_1=$HEAVY
SEALED_insects_2=$HEAVY
SEALED_birds_day=$HEAVY
SEALED_river=$HEAVY

# The insects must still tell the driver it is night through a closed window
# at highway speed. The heavy chain takes the crickets down about 30 dB, so
# their sealed twins get level make-up: cut what is under 400 Hz first (no
# insect is there, and the chain's low shelf would lift the forest rumble),
# then raise the 1.5 to 4 kHz band, where the cab lets anything through, to
# INSECT_BAND_RMS_<key>, but never past INSECT_SEALED_CAP_<key> under the clear copy's
# overall level, so the muffled bed is never the louder of the two.
# Night C's recording has its insects about 14 dB under night A's in the
# 2 to 4 kHz band, so its twin aims higher and may match its clear copy.
INSECT_BAND_RMS_insects_1=-40.6
INSECT_BAND_RMS_insects_2=-38
INSECT_SEALED_CAP_insects_1=1
INSECT_SEALED_CAP_insects_2=0
VORBIS=(-c:a libvorbis -q:a 5)
# The sealed twins have nothing above a few kHz and play centred: mono at
# 22.05 kHz, which keeps every sample position of the clear copy.
SEALED_FORMAT=(-ac 1 -ar 22050)

band_rms_of() {
  ffmpeg -nostats -i "$1" -af "highpass=f=1500,lowpass=f=4000,astats=measure_overall=RMS_level:measure_perchannel=none" \
    -f null - 2>&1 | awk '/RMS level dB/ {print $NF}' | tail -1
}

rms_of() {
  ffmpeg -nostats -i "$1" -af astats=measure_overall=RMS_level:measure_perchannel=none \
    -f null - 2>&1 | awk '/RMS level dB/ {print $NF}' | tail -1
}

duration_of() {
  ffprobe -v error -show_entries format=duration -of csv=p=0 "$1"
}

# Loop a clip with an F-second seam: the output is LEN seconds, and its first
# F seconds are the clip's head fading in under the F seconds after LEN, so the
# end runs straight into the start. The clip must be at least LEN + F long.
loop_seam() { # in out len f
  local in=$1 out=$2 len=$3 f=$4 end
  end=$(awk -v l="$len" -v f="$f" 'BEGIN {print l + f}')
  ffmpeg -v error -y -i "$in" -i "$in" -i "$in" -filter_complex "\
[0]atrim=0:$f,asetpts=PTS-STARTPTS,afade=t=in:d=$f:curve=qsin[h];\
[1]atrim=$len:$end,asetpts=PTS-STARTPTS,afade=t=out:d=$f:curve=qsin[t];\
[h][t]amix=inputs=2:normalize=0[x];\
[2]atrim=$f:$len,asetpts=PTS-STARTPTS[b];\
[x][b]concat=n=2:v=0:a=1" -c:a pcm_s16le "$out"
}

# Set a file's RMS to TARGET dBFS.
level_to() { # in out target
  local now gain
  now=$(rms_of "$1")
  gain=$(awk -v t="$3" -v n="$now" 'BEGIN {print t - n}')
  ffmpeg -v error -y -i "$1" -af "volume=${gain}dB" -c:a pcm_s16le "$2"
}

# Decode a whole source to 44.1 kHz stereo WAV, with an optional filter.
decode() { # in out [filter]
  local filter=${3:-anull}
  ffmpeg -v error -y -i "$1" -af "$filter" -ar 44100 -ac 2 -c:a pcm_s16le "$2"
}

# One bed: decode, loop with a seam of SEAM seconds, set the level.
bed() { # key wav-in seam target
  local key=$1 in=$2 seam=$3 target=$4 body
  body=$(awk -v d="$(duration_of "$in")" -v f="$seam" 'BEGIN {print d - f}')
  loop_seam "$in" "$TMP/${key}_loop.wav" "$body" "$seam"
  level_to "$TMP/${key}_loop.wav" "$TMP/$key.wav" "$target"
}

# Insects, night one: made as a loop; a half-second seam costs nothing.
decode "$SRC/night-a_crickets_1_wolfgang.mp3" "$TMP/insects_1_in.wav"
bed insects_1 "$TMP/insects_1_in.wav" 0.5 "$BED_RMS"

# Insects, night two: 78 s with louder events in it; a 2 s seam. A third of
# its level is rumble under 250 Hz that no insect makes; cut it, so the level
# goes to the chorus.
decode "$SRC/night-c_forest_night_jcsounds.mp3" "$TMP/insects_2_in.wav" "highpass=f=250,highpass=f=250"
bed insects_2 "$TMP/insects_2_in.wav" 2 "$BED_RMS"

# Birds: 47 s from 10 s in (the steadiest stretch) gives a 45 s loop with a
# 2 s seam; rumble under 250 Hz cut.
ffmpeg -v error -y -ss 10 -t 47 -i "$SRC/day-a_park_ambience_birds_thimras_first150s.flac" \
  -af "highpass=f=250" -ar 44100 -ac 2 -c:a pcm_s16le "$TMP/birds_day_in.wav"
bed birds_day "$TMP/birds_day_in.wav" 2 "$BED_RMS"

# River: "Stream Sounds" stream1, 32 s. Its ends meet cleanly, but a 1 s
# seam evens out the level across the join.
decode "$SRC/river-b_stream1_kurt.ogg" "$TMP/river_in.wav"
bed river "$TMP/river_in.wav" 1 "$RIVER_RMS"

for key in insects_1 insects_2 birds_day river; do
  ffmpeg -v error -y -i "$TMP/$key.wav" "${VORBIS[@]}" "$AMB/$key.ogg"
  sealed_var="SEALED_$key"
  band_var="INSECT_BAND_RMS_$key"
  cap_var="INSECT_SEALED_CAP_$key"
  chain=${!sealed_var}
  # Cut to the clear copy's length: the two loop side by side, and an echo
  # tail a few milliseconds long would walk them apart a little every lap.
  len=$(duration_of "$TMP/$key.wav")
  if [ -n "${!band_var:-}" ]; then
    ffmpeg -v error -y -i "$TMP/$key.wav" -af "highpass=f=400,$chain" -t "$len" \
      "${SEALED_FORMAT[@]}" -c:a pcm_s16le "$TMP/${key}_sealed_raw.wav"
    gain=$(awk -v t="${!band_var}" -v b="$(band_rms_of "$TMP/${key}_sealed_raw.wav")" \
      -v cap="$(awk -v o="$(rms_of "$TMP/$key.wav")" -v c="${!cap_var}" 'BEGIN {print o - c}')" \
      -v f="$(rms_of "$TMP/${key}_sealed_raw.wav")" \
      'BEGIN { g = t - b; if (f + g > cap) g = cap - f; print g }')
    printf '%-18s make-up %+.1f dB\n' "${key}_sealed" "$gain"
    ffmpeg -v error -y -i "$TMP/${key}_sealed_raw.wav" -af "volume=${gain}dB,alimiter=limit=0.7:level=false" \
      "${SEALED_FORMAT[@]}" "${VORBIS[@]}" "$AMB/${key}_sealed.ogg"
  else
    ffmpeg -v error -y -i "$TMP/$key.wav" -af "$chain" -t "$len" \
      "${SEALED_FORMAT[@]}" "${VORBIS[@]}" "$AMB/${key}_sealed.ogg"
  fi
done

for key in insects_1 insects_2 birds_day river; do
  for twin in "$key" "${key}_sealed"; do
    printf '%-18s RMS %6s dBFS  1.5-4 kHz %6s dBFS  %ss\n' "$twin" "$(rms_of "$AMB/$twin.ogg")" \
      "$(band_rms_of "$AMB/$twin.ogg")" \
      "$(duration_of "$AMB/$twin.ogg")"
  done
done
