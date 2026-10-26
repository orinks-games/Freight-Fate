#!/bin/bash
# Boot the Linux release on a distribution other than the one that built it.
#
# Run inside a bare container of the distribution under test, with the
# repository (or just `dist/`) mounted at /work:
#
#     docker run --rm -v "$PWD:/work" fedora:latest bash /work/tools/linux_smoke.sh
#
# Both downloads are booted exactly as a player would: the tarball unpacked
# and its executable run, the AppImage run in place (extracted first,
# because a container has no FUSE). Each boots for five frames with SDL's
# dummy video and audio drivers, and the session log then has to say that
# the game's own audio engine (Hearken, compiled in) is the one it runs.
# Speech is NOT disabled here:
# Prism is linked into the executable and opens the system's Speech
# Dispatcher at run time, so a container without it has to boot silent
# rather than fail.
#
# Every log is also required to hold no error-level line at all: a boot
# that reaches the menu while something inside it failed is not a pass.
#
# On Fedora the tarball is booted a second time under Xvfb with SDL's real
# X11 driver, to prove the statically linked SDL2 finds the system's X
# libraries at run time, and Speech Dispatcher is installed and started
# first so the log has to show Prism choosing it on its own -- the same
# line a player's desktop produces. The other distributions run the dummy
# driver with no speech daemon; that is the loader and glibc check, which
# is what differs between them.
#
# What each container is given is what every desktop install already has:
# libdbus-1 (the executable links it for the Secret Service keyring),
# libasound (ALSA; the audio engine links it to reach the sound server)
# and libstdc++. Debian, Ubuntu and openSUSE get no speech-dispatcher client
# library, which proves the game starts without it. Nothing else is
# installed -- a distribution that needs more is a finding, not something
# to paper over here.
#
# The container's own architecture decides which pair of downloads is
# booted: the x64 tarball and x86_64 AppImage on a PC runner, the arm64
# tarball and aarch64 AppImage on an ARM64 one (the Blazie BT Speak and BT
# Braille are Debian on a Raspberry Pi Compute Module, so the debian
# containers on the ARM runner are the closest stand-in the nightly has).
set -euo pipefail

case "$(uname -m)" in
  x86_64) tarball_arch=x64; appimage_arch=x86_64 ;;
  aarch64) tarball_arch=arm64; appimage_arch=aarch64 ;;
  *)
    echo "No Linux release is built for $(uname -m)." >&2
    exit 1
    ;;
esac

. /etc/os-release
echo "== $PRETTY_NAME ($(uname -m))"
case "${ID:-}" in
  ubuntu|debian)
    apt-get update -qq >/dev/null
    # Ubuntu 24.04 and Debian 13 renamed ALSA's library for the 64-bit time
    # transition (libasound2t64); the older releases keep libasound2.
    alsa=libasound2t64
    apt-cache show "$alsa" >/dev/null 2>&1 || alsa=libasound2
    apt-get install -y -qq --no-install-recommends libdbus-1-3 "$alsa" libstdc++6 >/dev/null
    ;;
  fedora)
    dnf install -y -q dbus-libs alsa-lib libstdc++ speech-dispatcher-libs xorg-x11-server-Xvfb \
      libX11 libXext libXrandr libXcursor libXi libXfixes libXScrnSaver libxkbcommon \
      mesa-libGL mesa-libEGL speech-dispatcher >/dev/null
    speech-dispatcher -d
    ;;
  arch)
    pacman -Sy --noconfirm --quiet dbus alsa-lib gcc-libs speech-dispatcher >/dev/null
    ;;
  opensuse-tumbleweed|opensuse-leap)
    zypper --quiet --non-interactive install libdbus-1-3 libasound2 libstdc++6 >/dev/null
    ;;
  *)
    echo "No package step for ${ID:-unknown}; booting with what the image has."
    ;;
esac
echo "glibc: $(ldd --version | head -1)"

export SDL_VIDEODRIVER=dummy SDL_AUDIODRIVER=dummy
export FREIGHT_FATE_DATA_DIR=/tmp/ff-data

check_log() {
  # $1: what booted, $2: its log.
  if [ ! -s "$2" ]; then
    echo "$1: the game wrote no session log." >&2
    exit 1
  fi
  required=("Audio backend: hearken")
  if [ "${ID:-}" = "fedora" ]; then
    required+=("Speech backend: Speech Dispatcher")
  fi
  for line in "${required[@]}"; do
    if ! grep -q "$line" "$2"; then
      echo "$1: the session log never says '$line':" >&2
      cat "$2" >&2
      exit 1
    fi
  done
  if grep -q " ERROR " "$2"; then
    echo "$1: the session log has error lines:" >&2
    grep " ERROR " "$2" >&2
    exit 1
  fi
  echo "$1: booted with no errors on the Hearken audio engine."
}

# Shell globs, not find: a bare openSUSE image has no findutils.
tarballs=(/work/dist/FreightFate-*-linux-"$tarball_arch".tar.gz)
tarball="${tarballs[0]}"
if [ ! -f "$tarball" ]; then
  echo "No Linux $tarball_arch tarball under /work/dist." >&2
  exit 1
fi
rm -rf /tmp/ff-tarball && mkdir -p /tmp/ff-tarball
tar -xzf "$tarball" -C /tmp/ff-tarball
FREIGHT_FATE_LOG_FILE=/tmp/ff-tarball.log timeout 120 /tmp/ff-tarball/FreightFate/FreightFate --smoke
check_log "tarball" /tmp/ff-tarball.log

appimages=(/work/dist/FreightFate-*-linux-"$appimage_arch".AppImage)
appimage="${appimages[0]}"
if [ ! -f "$appimage" ]; then
  echo "No Linux $appimage_arch AppImage under /work/dist." >&2
  exit 1
fi
cp "$appimage" /tmp/FreightFate.AppImage
chmod +x /tmp/FreightFate.AppImage
FREIGHT_FATE_LOG_FILE=/tmp/ff-appimage.log timeout 120 /tmp/FreightFate.AppImage --appimage-extract-and-run --smoke
check_log "AppImage" /tmp/ff-appimage.log

if [ "${ID:-}" = "fedora" ]; then
  unset SDL_VIDEODRIVER
  FREIGHT_FATE_LOG_FILE=/tmp/ff-x11.log timeout 120 xvfb-run -a /tmp/ff-tarball/FreightFate/FreightFate --smoke
  check_log "tarball under Xvfb (X11 driver)" /tmp/ff-x11.log
fi
echo "Linux smoke passed on $PRETTY_NAME ($(uname -m))."
