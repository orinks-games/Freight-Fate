#!/usr/bin/env bash
# Road seams and bridge joints, made from the in-repo synthesized seam thump
# (vehicle/road_joint.ogg): pitched cuts for the steer axle, lower ones for
# the drive tandem, and a heavier, lower bridge end joint. Each bank plays a
# cut at random, so no two seams land the same.
#
# Usage: sound-test/road_seams.sh UNPACKED_SOUND_TREE
#   Reads vehicle/road_joint.ogg from the tree and writes the banks next to
#   it. Repack with `cargo tools pack-sounds --out assets/sounds.pak`.
set -euo pipefail

TREE=${1:?unpacked sound tree}
SRC="$TREE/vehicle/road_joint.ogg"
VEH="$TREE/vehicle"
VORBIS=(-ac 1 -c:a libvorbis -q:a 5)

# Playback rate of each cut: 1.0 is the thump as recorded.
STEER_RATES=(0.94 0.98 1.02 1.06)
DRIVE_RATES=(0.82 0.86 0.90 0.94)
BRIDGE_RATES=(0.60 0.65 0.70)

cut() { # rate extra-filter out
  ffmpeg -v error -y -i "$SRC" \
    -af "asetrate=44100*$1,aresample=44100,$2,alimiter=limit=0.9:level=false" \
    "${VORBIS[@]}" "$3"
}

i=0
for r in "${STEER_RATES[@]}"; do
  i=$((i + 1))
  cut "$r" "anull" "$VEH/seam_steer_0$i.ogg"
done
# The drive tandem: lower, and a little duller under the trailer.
i=0
for r in "${DRIVE_RATES[@]}"; do
  i=$((i + 1))
  cut "$r" "lowpass=f=2500" "$VEH/seam_drive_0$i.ogg"
done
# A bridge end joint: the steel plate of an expansion joint, heavier and
# lower, with a second strike 30 ms on where the tire leaves the plate.
i=0
for r in "${BRIDGE_RATES[@]}"; do
  i=$((i + 1))
  cut "$r" "lowshelf=f=120:g=4,aecho=1.0:0.6:30:0.5,lowpass=f=1800" "$VEH/bridge_joint_0$i.ogg"
done

for f in "$VEH"/seam_steer_0*.ogg "$VEH"/seam_drive_0*.ogg "$VEH"/bridge_joint_0*.ogg; do
  printf '%-22s %s\n' "$(basename "$f")" \
    "$(ffmpeg -nostats -i "$f" -af astats=measure_overall=Peak_level+RMS_level:measure_perchannel=none -f null - 2>&1 |
      awk '/Peak level dB|RMS level dB/ {printf "%s %s  ", $(NF-3), $NF}')"
done
