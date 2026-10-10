"""Build a standalone Freight Fate distribution.

Packages the Rust game and archives it for release:

* Windows: ``dist/FreightFate-<label>-windows-portable.zip``
* Linux:   ``dist/FreightFate-<label>-linux-x64.tar.gz`` on x86_64, and
  ``dist/FreightFate-<label>-linux-arm64.tar.gz`` on aarch64 (the Blazie
  BT Speak and BT Braille, Raspberry Pi and other ARM64 Linux machines)
* macOS Apple Silicon: ``dist/FreightFate-<label>-macos-arm64.zip``
* macOS Intel: ``dist/FreightFate-<label>-macos.zip``

``<label>`` is the project version from pyproject.toml, or the value of
``--tag`` (used for nightly developer snapshots).

Run from the repository root: ``uv run python tools/build_release.py``

The pipeline is ``cargo build --release -p freight-fate --no-default-features``
(no agent server in a player build; ``--cargo-target-dir`` picks the Cargo
target directory), then ``ff-bake``
to turn the JSON data tree into ``world.ffdata``, then a ``FreightFate/``
folder with the executable renamed to ``FreightFate``. On macOS it creates
``FreightFate.app`` with the executable under ``Contents/MacOS``, native
libraries under ``Contents/Frameworks``, and data, packs, build metadata, and
documents under ``Contents/Resources``. With ``MACOS_SIGN_IDENTITY`` set, the
app is signed with that Developer ID under the hardened runtime, and with the
``MACOS_NOTARY_*`` variables also set it is notarized and stapled before it is
archived; without them it is ad-hoc signed for local use. ``--rust`` is accepted for the
callers that still pass it; it is the only mode.
"""

from __future__ import annotations

import argparse
import hashlib
import importlib.util
import json
import os
import platform
import plistlib
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
import urllib.error
import urllib.request
import zipfile
from collections.abc import Mapping
from datetime import datetime, timezone
from pathlib import Path

import tomllib
from windows_runtime import REQUIRED_CRT, stage_windows_runtime, verify_windows_runtime

ROOT = Path(__file__).resolve().parent.parent
TOOLS = ROOT / "tools"
DIST = ROOT / "dist"
BUILD = ROOT / "build"
APP_NAME = "FreightFate"
# The checkout root stands in for the installed ``freight_fate/`` package:
# ``data/`` and ``assets/sounds/`` stage to the same relative paths under it.
# The packs and add-ons sit under ``assets/`` here but at the package top
# level (packs) and in ``lib/`` (add-ons) in a release.
PACKAGE_DIR = ROOT
SOURCE_ASSETS = "assets"
# Game-shipped BASS addon plugins (e.g. basshls for HLS radio streams),
# staged beside the executable with the rest of BASS.
ADDON_LIB_DIR = PACKAGE_DIR / SOURCE_ASSETS / "lib"
# Production, so a release no longer pulls its music through the staging host.
#
# This took two goes. The route is a 307 to FREIGHT_FATE_MUSIC_BLOB_URL, and
# that Vercel variable was scoped to Preview (dev) only, so production answered
# 503 and the first attempt at this line failed every platform of the snapshot
# build with "Music-pack download failed with HTTP status 503". The route's
# file being in the deployment was never the same as the route answering.
#
# The variable is on the Production scope as of 2026-09-20 and the route was
# checked answering 307 to the blob before this moved. If it ever 503s again,
# that variable is the first thing to look at -- and note Vercel bakes the
# environment at deploy time, so setting it takes a redeploy to have any
# effect. The sha256 below is what actually gates the download either way.
DEFAULT_MUSIC_URL = "https://www.orinks.net/downloads/music.pak"
DEFAULT_MUSIC_SHA256 = "c56401a2a45057faba0bd4bf7d024ce7dd3908f682f4c35526643a1870cb7798"
# Channel 3000's clips, packed by tools/build_channel3000.py from the
# channel3000-clips branch. Its own pack rather than more of music.pak, which
# the game reads whole into memory; the game opens this one only when the
# radio is first tuned to 87.7. The digest is of that tool's build of the
# branch after tools/level_channel3000.py --write (98 clips, 2026-10-10). Each
# rebuild is published under a new name beside the old one, so a branch still
# pinned to an older pack keeps building; a rebuild moves both lines.
DEFAULT_CHANNEL3000_URL = "https://crisp-crystal-9a9y.here.now/channel3000-a8343d91.pak"
DEFAULT_CHANNEL3000_SHA256 = "a8343d916fdd31083a46497dd393865cbc78f4045faa9c6ef42f817f684dd179"


def platform_native_exts(platform_name: str = sys.platform) -> set[str]:
    if platform_name == "win32":
        return {".dll"}
    if platform_name == "darwin":
        return {".dylib"}
    return {".so"}


def project_version() -> str:
    with open(ROOT / "pyproject.toml", "rb") as f:
        return tomllib.load(f)["project"]["version"]


def runtime_root(build_dir: Path) -> Path:
    if build_dir.suffix == ".app":
        return build_dir / "Contents" / "MacOS"
    return build_dir


def native_files(root: Path, exts: set[str] | None = None) -> list[Path]:
    suffixes = exts or platform_native_exts()
    return [path for path in root.rglob("*") if path.is_file() and path.suffix.lower() in suffixes]


def _load_tool(name: str):
    """Load a by-path tools module (tools is not a package)."""
    spec = importlib.util.spec_from_file_location(
        name, Path(__file__).resolve().parent / f"{name}.py"
    )
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def stage_sound_pack(build_dir: Path, root: Path | None = None) -> None:
    """Stage the approved encrypted packs and keep the credits readable."""
    root = root or runtime_root(build_dir)
    destination = root / "freight_fate" / "sounds.pak"
    music_destination = root / "freight_fate" / "music.pak"
    channel3000_destination = root / "freight_fate" / "channel3000.pak"
    destination.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(PACKAGE_DIR / SOURCE_ASSETS / "sounds.pak", destination)
    shutil.copy2(PACKAGE_DIR / SOURCE_ASSETS / "music.pak", music_destination)
    shutil.copy2(PACKAGE_DIR / SOURCE_ASSETS / "channel3000.pak", channel3000_destination)
    credits = PACKAGE_DIR / "assets" / "sounds" / "CREDITS.md"
    if not credits.exists():
        raise RuntimeError(f"Sound credits were not found: {credits}")
    shutil.copy2(credits, root / "SOUND_CREDITS.md")


def stage_release_docs(build_dir: Path, root: Path | None = None) -> None:
    """Copy player-facing release documents into the packaged runtime."""
    changelog = ROOT / "CHANGELOG.md"
    if not changelog.exists():
        raise RuntimeError(f"Changelog was not found: {changelog}")
    root = root or runtime_root(build_dir)
    shutil.copy2(changelog, root / "CHANGELOG.md")

    license_file = ROOT / "LICENSE"
    if not license_file.exists():
        raise RuntimeError(f"License was not found: {license_file}")
    # PolyForm's Notices term expects the terms to travel with every copy.
    shutil.copy2(license_file, root / "LICENSE.txt")

    manual = ROOT / "docs" / "user-manual.md"
    if not manual.exists():
        raise RuntimeError(f"User manual was not found: {manual}")
    shutil.copy2(manual, root / "USER_MANUAL.md")
    # Also ship a browser-friendly, accessible HTML rendering of the manual.
    manual_html = _load_tool("manual_html").markdown_to_html(
        manual.read_text(encoding="utf-8"), title="Freight Fate Player Manual"
    )
    (root / "USER_MANUAL.html").write_text(manual_html, encoding="utf-8")

    # The alpha test book travels with the build it describes. A tester who
    # has to go find the current copy in the repo ends up working an old one,
    # and its checklists are written against specific builds.
    test_book = ROOT / "docs" / "alpha-test-book.md"
    if not test_book.exists():
        raise RuntimeError(f"Alpha test book was not found: {test_book}")
    shutil.copy2(test_book, root / "ALPHA_TEST_BOOK.md")
    test_book_html = _load_tool("manual_html").markdown_to_html(
        test_book.read_text(encoding="utf-8"), title="Freight Fate Alpha Test Book"
    )
    (root / "ALPHA_TEST_BOOK.html").write_text(test_book_html, encoding="utf-8")


def verify_sound_packs(root: Path) -> None:
    """Open the staged packs and prove they carry what the game reads."""
    if root.suffix == ".app":
        root = root / "Contents" / "Resources"
    assets_pack = _load_tool("pack_sounds")._load_assets_pack()
    pack_names = assets_pack.SoundPack(root / "freight_fate" / "sounds.pak").names()
    if not any(name.endswith((".ogg", ".wav")) for name in pack_names):
        raise RuntimeError("Packaged sound pack contains no audio files")

    if "engine_classic/idle.ogg" not in pack_names:
        raise RuntimeError(
            "Packaged sound pack predates the classic engine voice, so the "
            "Settings engine-voice option would fall back silently; rebuild "
            "the committed pack with tools/pack_sounds.py on a builder "
            "machine and commit it"
        )

    music_pack_names = assets_pack.SoundPack(root / "freight_fate" / "music.pak").names()
    if not any(name.startswith("music/") for name in music_pack_names):
        raise RuntimeError("Packaged music pack contains no music files")

    channel3000_names = assets_pack.SoundPack(root / "freight_fate" / "channel3000.pak").names()
    if not any(name.startswith("c3k/") for name in channel3000_names):
        raise RuntimeError("Packaged Channel 3000 pack contains no clips")


def _is_snapshot_label(label: str) -> bool:
    """True for public 1.8 nightlies and Career 1.9 tester prereleases."""
    if label.startswith("nightly-"):
        return True
    prefix = "1.9-tester-"
    suffix = label[len(prefix) :] if label.startswith(prefix) else ""
    return len(suffix) == 8 and suffix.isdigit()


def build_commit() -> str:
    """The checked-out commit, so the updater can tell a same-day rebuild
    under one snapshot tag from the copy it replaced; "" outside git."""
    try:
        result = subprocess.run(
            ["git", "rev-parse", "HEAD"],
            cwd=ROOT,
            capture_output=True,
            text=True,
            check=True,
            timeout=30,
        )
    except (OSError, subprocess.SubprocessError):
        return ""
    return result.stdout.strip()


def label_package_version(label: str) -> str:
    """The version a build labelled ``label`` is: the label's own for a
    stable (``v1.9.1`` or ``1.9.1``), else the ``pyproject.toml`` version."""
    return project_version() if _is_snapshot_label(label) else label.removeprefix("v")


def stamp_build_info(build_dir: Path, label: str, root: Path | None = None) -> None:
    """Record what this build is, for the in-game updater.

    ``label`` is a snapshot tag (``nightly-20260611`` or
    ``1.9-tester-20260828``) or a plain version (``1.6.0``); the release
    tag for the latter is ``v``-prefixed.

    ``package_version`` is what the game reports as its own version, and
    what the updater compares releases against. A snapshot carries the
    exact ``pyproject.toml`` project version, since its label is a
    date-stamped tag. A stable build carries the label's version: the
    ``v1.9.0`` tag push built a game whose pyproject still read
    ``1.9.0.dev0``, so every 1.9.0 copy called itself a development build
    and was offered 1.9.0 again on every start.
    """
    snapshot = _is_snapshot_label(label)
    info = {
        "tag": label if snapshot else f"v{label.removeprefix('v')}",
        "channel": "dev" if snapshot else "stable",
        "built_at": datetime.now(timezone.utc).strftime("%Y-%m-%d"),
        "package_version": label_package_version(label),
        "commit": build_commit(),
    }
    info_path = (root or runtime_root(build_dir)) / "build_info.json"
    with open(info_path, "w", encoding="utf-8") as f:
        json.dump(info, f, indent=2)


MACOS_SIGN_IDENTITY_ENV = "MACOS_SIGN_IDENTITY"
MACOS_SIGN_KEYCHAIN_ENV = "MACOS_SIGN_KEYCHAIN"
MACOS_NOTARY_ENV = ("MACOS_NOTARY_KEY_PATH", "MACOS_NOTARY_KEY_ID", "MACOS_NOTARY_ISSUER_ID")


def macos_signing_identity(environ: Mapping[str, str] = os.environ) -> str:
    """The Developer ID to sign with, or empty for an ad-hoc local build."""
    return environ.get(MACOS_SIGN_IDENTITY_ENV, "").strip()


def macos_notary_credentials(
    environ: Mapping[str, str] = os.environ,
) -> tuple[str, str, str] | None:
    """App Store Connect key path, key ID and issuer, or None when unset."""
    values = tuple(environ.get(name, "").strip() for name in MACOS_NOTARY_ENV)
    if not any(values):
        return None
    if not all(values):
        missing = [name for name, value in zip(MACOS_NOTARY_ENV, values, strict=True) if not value]
        raise RuntimeError(f"notarization is half configured; missing {', '.join(missing)}")
    return values  # type: ignore[return-value]


def developer_id_codesign_command(identity: str, target: Path, keychain: str = "") -> list[str]:
    """codesign for one Mach-O or the bundle: hardened runtime, secure timestamp."""
    command = ["codesign", "--force", "--options", "runtime", "--timestamp"]
    if keychain:
        command += ["--keychain", keychain]
    return [*command, "--sign", identity, str(target)]


def sign_distribution(build_dir: Path, environ: Mapping[str, str] = os.environ) -> None:
    """Sign the finalized macOS app bundle, Developer ID when configured."""
    if sys.platform != "darwin":
        return
    verify_macos_native_dependencies(build_dir)
    identity = macos_signing_identity(environ)
    if identity:
        keychain = environ.get(MACOS_SIGN_KEYCHAIN_ENV, "").strip()
        # Inside out: nested libraries first, then the bundle seals them.
        # Notarization rejects --deep signing, so each dylib is signed alone.
        executable = build_dir / "Contents" / "MacOS" / APP_NAME
        for binary in macos_bundle_binaries(build_dir):
            if binary != executable:
                subprocess.run(
                    developer_id_codesign_command(identity, binary, keychain), check=True
                )
        subprocess.run(developer_id_codesign_command(identity, build_dir, keychain), check=True)
    else:
        subprocess.run(
            ["codesign", "--force", "--deep", "--sign", "-", str(build_dir)],
            check=True,
        )
    subprocess.run(
        ["codesign", "--verify", "--deep", "--strict", str(build_dir)],
        check=True,
    )


def notarize_distribution(build_dir: Path, environ: Mapping[str, str] = os.environ) -> None:
    """Notarize and staple a Developer ID signed app, when credentials are set."""
    if sys.platform != "darwin":
        return
    credentials = macos_notary_credentials(environ)
    if credentials is None:
        print("Skipped notarization: no App Store Connect key configured.")
        return
    if not macos_signing_identity(environ):
        raise RuntimeError("notarization needs a Developer ID signature; set MACOS_SIGN_IDENTITY")
    key_path, key_id, issuer = credentials
    with tempfile.TemporaryDirectory() as scratch:
        upload = Path(scratch) / f"{build_dir.stem}-notarize.zip"
        subprocess.run(
            ["ditto", "-c", "-k", "--keepParent", str(build_dir), str(upload)], check=True
        )
        result = subprocess.run(
            [
                "xcrun",
                "notarytool",
                "submit",
                str(upload),
                "--key",
                key_path,
                "--key-id",
                key_id,
                "--issuer",
                issuer,
                "--wait",
                "--timeout",
                "45m",
                "--output-format",
                "json",
            ],
            capture_output=True,
            text=True,
            check=False,
        )
    try:
        submission = json.loads(result.stdout)
    except json.JSONDecodeError:
        submission = {}
    status = submission.get("status", "")
    if status != "Accepted":
        submission_id = submission.get("id", "")
        if submission_id:
            subprocess.run(
                [
                    "xcrun",
                    "notarytool",
                    "log",
                    submission_id,
                    "--key",
                    key_path,
                    "--key-id",
                    key_id,
                    "--issuer",
                    issuer,
                ],
                check=False,
            )
        raise RuntimeError(
            f"Apple notarization returned {status or 'no status'} "
            f"(exit {result.returncode}): {result.stderr.strip()}"
        )
    print(f"Notarization accepted: {submission.get('id', '')}")
    subprocess.run(["xcrun", "stapler", "staple", str(build_dir)], check=True)
    subprocess.run(["xcrun", "stapler", "validate", str(build_dir)], check=True)
    subprocess.run(
        ["spctl", "--assess", "--type", "execute", "--verbose=2", str(build_dir)], check=True
    )


def smoke_check(build_dir: Path) -> None:
    """Boot the packaged game for a few frames with dummy drivers."""
    import os

    if build_dir.suffix == ".app":
        exe = build_dir / "Contents" / "MacOS" / APP_NAME
    else:
        exe = build_dir / (APP_NAME + (".exe" if sys.platform == "win32" else ""))
    env = {
        **os.environ,
        "SDL_VIDEODRIVER": "dummy",
        "SDL_AUDIODRIVER": "dummy",
        "FREIGHT_FATE_NO_SPEECH": "1",
    }
    command = [str(exe), "--smoke"]
    if build_dir.suffix == ".app":
        # A Mac app smokes headless; the GitHub-hosted macOS job selects the
        # explicit non-launch mode instead.
        command.append("--headless")
    with tempfile.TemporaryDirectory(prefix="freight-fate-smoke-") as temp_dir:
        smoke_root = Path(temp_dir)
        smoke_log = smoke_root / "game.log"
        env["FREIGHT_FATE_DATA_DIR"] = str(smoke_root / "player-data")
        env["FREIGHT_FATE_LOG_FILE"] = str(smoke_log)
        try:
            subprocess.run(command, check=True, cwd=exe.parent, env=env, timeout=120)
        except (subprocess.CalledProcessError, subprocess.TimeoutExpired):
            if smoke_log.is_file():
                print("Packaged smoke log before failure:", file=sys.stderr)
                print(smoke_log.read_text(encoding="utf-8", errors="replace"), file=sys.stderr)
            raise
    print("Smoke check passed: the packaged runtime boots and loads its resources.")


def strip_user_data(build_dir: Path) -> None:
    """Remove any saves/logs left in the build before archiving.

    Freight Fate is portable: a frozen build keeps profiles in a ``saves`` folder
    next to the exe. The smoke check boots the build (and ``profile.py`` may even
    migrate a nearby dev save into it), so a ``saves`` folder appears in the
    build tree. It must NEVER ship -- it would leak the builder's profile and
    signing key, or on CI ship a throwaway profile. The real saves live in the
    user's own game folder / AppData and are untouched by this.
    """
    roots = [build_dir]
    if build_dir.suffix == ".app":
        roots.append(build_dir / "Contents" / "MacOS")
    for root in roots:
        for name in ("saves", "logs"):
            leftover = root / name
            if leftover.exists():
                shutil.rmtree(leftover, ignore_errors=True)
                print(f"Stripped bundled '{name}/' from the build (never ship user data).")


def _archive_entries(out: Path) -> dict[str, tuple[int, int]]:
    """Map an archive's file entries to (size, permission mode)."""
    if out.name.endswith(".tar.gz"):
        with tarfile.open(out, "r:gz") as tar:
            return {m.name: (m.size, m.mode) for m in tar.getmembers() if m.isfile()}
    with zipfile.ZipFile(out) as z:
        return {
            info.filename: (info.file_size, (info.external_attr >> 16) & 0o7777)
            for info in z.infolist()
            if not info.is_dir()
        }


LINUX_TARBALL_SUFFIXES = ("-linux-x64.tar.gz", "-linux-arm64.tar.gz")


def linux_archive_suffix(machine: str | None = None) -> str:
    """The ``linux-<arch>`` label a Linux tarball is named with.

    ``x64`` for the PC build and ``arm64`` for the aarch64 one -- the Blazie
    BT Speak and BT Braille notetakers, Raspberry Pi, and other ARM64 Linux
    machines -- matching the Mac archive's ``arm64`` rather than the
    AppImage's ``aarch64`` because the tarball predates the AppImage and the
    in-game updater picks by this suffix. Anything else has no build.
    """
    machine = (machine or platform.machine()).lower()
    if machine in {"x86_64", "amd64"}:
        return "linux-x64"
    if machine in {"aarch64", "arm64"}:
        return "linux-arm64"
    raise RuntimeError(f"No Linux release archive name for machine {machine!r}.")


def verify_archive(out: Path) -> None:
    """Re-open the finished archive and prove the payload survived archiving.

    ``verify_rust_payload`` checks the staged build folder, but the
    archiving step after it is what players actually download. A bad glob,
    archiver quirk, or dropped permission bit here would ship a download with
    no runnable game inside (for example a Linux snapshot missing its
    executable) and nothing else would notice.
    """
    macos_archive = out.name.endswith(("-macos-arm64.zip", "-macos.zip"))
    if macos_archive:
        root = f"{APP_NAME}.app/Contents/MacOS"
        exe_entry = f"{root}/{APP_NAME}"
        needs_exec = True
    elif out.name.endswith(LINUX_TARBALL_SUFFIXES):
        root = APP_NAME
        exe_entry = f"{root}/{APP_NAME}"
        needs_exec = True
    elif out.name.endswith("-windows-portable.zip"):
        root = APP_NAME
        exe_entry = f"{root}/{APP_NAME}.exe"
        needs_exec = False
    else:
        raise RuntimeError(f"Unrecognized release archive name: {out.name}")

    entries = _archive_entries(out)
    if exe_entry not in entries:
        raise RuntimeError(f"Release archive is missing the executable {exe_entry}: {out.name}")
    size, mode = entries[exe_entry]
    if size == 0:
        raise RuntimeError(f"Release archive executable is empty: {exe_entry} in {out.name}")
    if needs_exec and not mode & 0o111:
        raise RuntimeError(
            f"Release archive executable lost its executable permission: {exe_entry} in {out.name}"
        )

    # A Mac bundle keeps its payload under Resources; the others beside the exe.
    payload_root = f"{APP_NAME}.app/Contents/Resources" if macos_archive else root
    required = (
        "build_info.json",
        "LICENSE.txt",
        "USER_MANUAL.md",
        "freight_fate/sounds.pak",
        "freight_fate/music.pak",
        "freight_fate/channel3000.pak",
        RUST_BAKED_FILE_ENTRY,
    )
    missing = [name for name in required if f"{payload_root}/{name}" not in entries]
    if macos_archive:
        bundle_required = (
            f"{APP_NAME}.app/Contents/Info.plist",
            *(f"{APP_NAME}.app/Contents/Frameworks/{name}" for name in MACOS_REQUIRED_LIBRARIES),
        )
        missing.extend(name for name in bundle_required if name not in entries)
        # No libSDL2 requirement: SDL2 ships compiled into the executable.
    elif out.name.endswith(LINUX_TARBALL_SUFFIXES):
        # Same libraries as the Mac bundle, flat beside the executable.
        missing.extend(
            f"{root}/{name}" for name in LINUX_REQUIRED_LIBRARIES if f"{root}/{name}" not in entries
        )
    else:
        # The delivered ZIP, not only the build runner, must carry the CRT.
        folded = {name.lower(): metadata for name, metadata in entries.items()}
        missing.extend(
            f"{root}/{name}"
            for name in REQUIRED_CRT
            if folded.get(f"{root}/{name}".lower(), (0, 0))[0] == 0
        )
    if missing:
        raise RuntimeError(
            f"Release archive is missing payload files: {', '.join(missing)} in {out.name}"
        )


def is_career_19_label(label: str) -> bool:
    """A Career 1.9 tester snapshot or a stable tag from v1.9.0 on.

    Their Apple Silicon archive is named ``-macos-arm64`` (the workflow
    uploads only that name, and its stable step adds the ``-macos`` copy
    the updaters look for); a 1.8 stable tag keeps the plain ``-macos``.
    """
    if label.startswith("1.9-tester-"):
        return True
    match = re.match(r"v?(\d+)\.(\d+)\.", label)
    return bool(match) and (int(match[1]), int(match[2])) >= (1, 9)


def archive(build_dir: Path, label: str) -> Path:
    if sys.platform == "win32":
        out = DIST / f"{APP_NAME}-{label}-windows-portable.zip"
        with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
            for path in sorted(build_dir.rglob("*")):
                z.write(path, Path(APP_NAME) / path.relative_to(build_dir))
    elif sys.platform == "darwin":
        is_apple_silicon = platform.machine().lower() in {"arm64", "aarch64"}
        mac_suffix = "macos-arm64" if is_career_19_label(label) and is_apple_silicon else "macos"
        out = DIST / f"{APP_NAME}-{label}-{mac_suffix}.zip"
        subprocess.run(["ditto", "-c", "-k", "--keepParent", str(build_dir), str(out)], check=True)
    else:
        out = DIST / f"{APP_NAME}-{label}-{linux_archive_suffix()}.tar.gz"
        with tarfile.open(out, "w:gz") as tar:
            tar.add(build_dir, arcname=APP_NAME)
    return out


# -- The Rust payload ---------------------------------------------------------
#
# The staged folder keeps the layout the in-game updater and
# ``verify_archive`` expect -- a top-level ``FreightFate`` folder holding
# ``FreightFate(.exe)``, ``build_info.json``, the docs, and a ``freight_fate/``
# folder with the packs and the baked data container
# (``ff_core::data::data_resources::data_root`` looks for
# ``<exe dir>/freight_fate/data``). The native libraries (SDL2, BASS and its
# plugins) sit flat beside the executable; Prism is linked into it, which is where the crates'
# build scripts stage them and where their loaders look first.

# ``[[bin]] name`` in crates/freight-fate/Cargo.toml. The staged copy is
# renamed to APP_NAME: ``updater::is_frozen`` (both ports) accepts the stem
# case-insensitively, but the apply script, ``extracted_root`` and
# ``verify_archive`` all spell it ``FreightFate``.
RUST_PACKAGE = "freight-fate"
RUST_BIN_STEM = "freightfate"
RUST_STAGE_DIR = BUILD / APP_NAME
# The baked binary data container. ``ff-bake`` builds it out of the JSON
# tree through the game's own loaders, and the release ships it INSTEAD of
# that tree: 142 MB of JSON becomes roughly 7 MB, and the game maps it rather
# than parsing 94 MB of leg shards before the menu. See
# ``ff_core::data::baked``.
RUST_BAKED_FILE = "world.ffdata"
# Where the container sits inside the staged payload, relative to its root.
RUST_BAKED_FILE_ENTRY = f"freight_fate/data/{RUST_BAKED_FILE}"
# The container's first eight bytes (``ff_core::data::baked::MAGIC``). Checked
# in the staged payload so a wrong or truncated file fails the build.
BAKED_MAGIC = b"FFDATA\x00\x00"
RUST_BAKE_PACKAGE = "ff-core"
RUST_BAKE_BIN = "ff-bake"
# The runtime JSON files the container replaces. Nothing here is staged; the
# list is what ``verify_rust_payload`` checks did NOT leak into the payload,
# so a half-migrated build that ships both is caught rather than shipped.
RUST_BAKED_SOURCE_FILES = (
    "buffs.json",
    "channel3000.json",
    "city_services.json",
    "facility_approaches.json",
    "facility_endpoints.json",
    "local_approaches.json",
    "local_geometry.json",
    "radio_catalog.json",
    "radio_community.json",
    "radio_imported.json",
    "street_limits.json",
    "world_data/index.json",
    "world_data/geo.json",
    "world_data/us/cities.json",
    "world_data/us/gameplay/curves.jsonl",
    "world_data/us/gameplay/curve_artifacts.jsonl",
)
# Loose runtime data files still shipped as files, because the container does
# not cover them. Empty today -- everything the game loads is baked -- and
# kept so a new runtime data file has somewhere to be registered before
# anyone has to teach the baker about it.
RUST_DATA_FILES: tuple[str, ...] = ()
RUST_DATA_GLOBS: tuple[str, ...] = ()
# The committed loose sound tree (``assets/sounds``) travels as editable
# files: the Rust asset loader reads it beside the pack. The licensed overlay
# never ships loose.
LOOSE_SOUND_TREE = Path("assets") / "sounds"
LICENSED_SOUND_TREE = "sounds-licensed"
# Git LFS writes this text in place of a pack that was never fetched.
LFS_POINTER_PREFIX = b"version https://git-lfs"
# Build-only leftovers in the Cargo profile directory that are not runtime
# libraries even though they carry a native suffix on some platforms.
CARGO_NON_RUNTIME_SUFFIXES = {".pdb", ".d", ".rlib", ".lib", ".exp"}
MACOS_REQUIRED_LIBRARIES = (
    "libbass.dylib",
    "libbassopus.dylib",
    "libbasshls.dylib",
    "libbassflac.dylib",
)
# The Linux tarball's equivalents, flat beside the executable.
LINUX_REQUIRED_LIBRARIES = (
    "libbass.so",
    "libbassopus.so",
    "libbasshls.so",
    "libbassflac.so",
)


def rust_exe_name(platform_name: str = sys.platform) -> str:
    """The executable file name the staged build ships."""
    return APP_NAME + (".exe" if platform_name == "win32" else "")


def cargo_exe_name(platform_name: str = sys.platform) -> str:
    """The executable file name ``cargo build`` writes."""
    return RUST_BIN_STEM + (".exe" if platform_name == "win32" else "")


def cargo_build_command(target_dir: Path | None = None) -> list[str]:
    # No default features: the agent server (``--agent-server``) is dev
    # tooling, and a player has no use for an agent at the wheel.
    cmd = ["cargo", "build", "--release", "-p", RUST_PACKAGE, "--no-default-features"]
    if target_dir is not None:
        cmd.extend(["--target-dir", str(target_dir)])
    return cmd


def cargo_profile_dir(target_dir: Path | None = None) -> Path:
    """Where ``cargo build --release`` leaves the binary and staged libraries."""
    return (target_dir if target_dir is not None else ROOT / "target") / "release"


def bake_command(out: Path, target_dir: Path | None = None, check: bool = False) -> list[str]:
    """``ff-bake`` over the checked-in data tree."""
    cmd = ["cargo", "run", "--release", "-p", RUST_BAKE_PACKAGE, "--bin", RUST_BAKE_BIN]
    if target_dir is not None:
        cmd.extend(["--target-dir", str(target_dir)])
    cmd.extend(["--", "--data-dir", str(PACKAGE_DIR / "data"), "--out", str(out)])
    if check:
        cmd.append("--check")
    return cmd


def bake_world_data(target_dir: Path | None = None, out: Path | None = None) -> Path:
    """Build the baked data container and return where it landed.

    Runs before staging so the container is just another file in the layout
    plan, and so a data tree that fails to load fails the build here rather
    than in front of a player.
    """
    source = PACKAGE_DIR / "data"
    if not (source / "world_data").is_dir():
        raise RuntimeError(f"Runtime data tree is missing from the checkout: {source}")
    destination = out if out is not None else BUILD / RUST_BAKED_FILE
    destination.parent.mkdir(parents=True, exist_ok=True)
    subprocess.run(bake_command(destination, target_dir), cwd=ROOT, check=True)
    if not destination.is_file():
        raise RuntimeError(f"ff-bake wrote no container at {destination}")
    return destination


def is_lfs_pointer(path: Path) -> bool:
    """True when ``path`` is a Git LFS pointer rather than the real file."""
    try:
        if path.stat().st_size > 1024:
            return False
        with open(path, "rb") as f:
            return f.read(len(LFS_POINTER_PREFIX)) == LFS_POINTER_PREFIX
    except OSError:
        return False


def require_real_pack(path: Path) -> None:
    """Refuse to stage a pack that is only a Git LFS pointer."""
    if not path.exists():
        raise RuntimeError(f"Sound pack was not found: {path}")
    if is_lfs_pointer(path):
        raise RuntimeError(
            f"{path.name} is a Git LFS pointer, not the pack itself; run "
            "`git lfs pull` in this checkout before building"
        )


def file_sha256(path: Path) -> str:
    """Return the SHA-256 digest of ``path`` using bounded memory."""
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def _pinned_download_config(
    env: Mapping[str, str], prefix: str, default_url: str, default_sha256: str
) -> tuple[str, str]:
    """``<prefix>_URL`` and ``<prefix>_SHA256`` from ``env``, else the pins."""
    url = env.get(f"{prefix}_URL", default_url)
    expected_sha256 = env.get(f"{prefix}_SHA256", default_sha256).lower()
    if len(expected_sha256) != 64 or any(
        character not in "0123456789abcdef" for character in expected_sha256
    ):
        raise RuntimeError(f"{prefix}_SHA256 must be a 64-character hexadecimal digest")
    return url, expected_sha256


def music_download_config(env: Mapping[str, str] = os.environ) -> tuple[str, str]:
    """Return the music-pack URL and required lowercase SHA-256 digest."""
    return _pinned_download_config(
        env, "FREIGHT_FATE_MUSIC", DEFAULT_MUSIC_URL, DEFAULT_MUSIC_SHA256
    )


def channel3000_download_config(env: Mapping[str, str] = os.environ) -> tuple[str, str]:
    """Return the Channel 3000 pack URL and required lowercase SHA-256 digest."""
    return _pinned_download_config(
        env, "FREIGHT_FATE_CHANNEL3000", DEFAULT_CHANNEL3000_URL, DEFAULT_CHANNEL3000_SHA256
    )


def download_to_path(request: urllib.request.Request, destination: Path) -> None:
    """Stream one authenticated HTTP request to disk with bounded memory."""
    with urllib.request.urlopen(request) as response, destination.open("wb") as output:
        shutil.copyfileobj(response, output, length=1024 * 1024)


def _ensure_pinned_pack(path: Path, url: str, expected_sha256: str, label: str) -> None:
    """Download ``url`` to ``path`` unless a copy with the pinned digest is there.

    The download lands in a temporary file beside ``path`` and replaces it
    only once its digest matches, so a failed or tampered download never
    costs the copy already there. ``label`` names the pack in errors
    ("Music-pack download failed").
    """
    if path.is_file() and not is_lfs_pointer(path) and file_sha256(path) == expected_sha256:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        dir=path.parent, prefix=f"{path.name}.", suffix=".download", delete=False
    ) as temp:
        temporary = Path(temp.name)
    try:
        try:
            request = urllib.request.Request(
                url,
                headers={"User-Agent": "Freight-Fate-release-builder/1.9"},
            )
            download_to_path(request, temporary)
        except urllib.error.HTTPError as exc:
            raise RuntimeError(
                f"{label} download failed with HTTP status {exc.code}. "
                "Check your connection and retry the build."
            ) from exc
        except (urllib.error.URLError, ConnectionError, TimeoutError) as exc:
            detail = exc.reason if isinstance(exc, urllib.error.URLError) else str(exc)
            raise RuntimeError(
                f"{label} download failed: {detail}. Check your connection and retry the build."
            ) from exc
        actual_sha256 = file_sha256(temporary)
        if actual_sha256 != expected_sha256:
            raise RuntimeError(
                f"Downloaded {path.name} failed SHA-256 verification: "
                f"expected {expected_sha256}, got {actual_sha256}"
            )
        temporary.replace(path)
        print(f"Downloaded and verified {path.name} ({actual_sha256}).")
    except Exception:
        temporary.unlink(missing_ok=True)
        raise


def ensure_music_pack(path: Path = PACKAGE_DIR / SOURCE_ASSETS / "music.pak") -> None:
    """Download and verify the public music pack when it is not already present."""
    url, expected_sha256 = music_download_config()
    _ensure_pinned_pack(path, url, expected_sha256, "Music-pack")


def ensure_channel3000_pack(
    path: Path = PACKAGE_DIR / SOURCE_ASSETS / "channel3000.pak",
) -> None:
    """Download and verify Channel 3000's pack when it is not already present."""
    url, expected_sha256 = channel3000_download_config()
    _ensure_pinned_pack(path, url, expected_sha256, "Channel 3000 pack")


def rust_data_files(package_dir: Path = PACKAGE_DIR) -> list[Path]:
    """The data files to ship, as paths relative to ``package_dir``."""
    data_dir = package_dir / "data"
    files: list[Path] = []
    for relative in RUST_DATA_FILES:
        source = data_dir / relative
        if not source.is_file():
            raise RuntimeError(f"Runtime data file is missing from the checkout: {source}")
        files.append(Path("data") / relative)
    for pattern in RUST_DATA_GLOBS:
        matches = sorted(data_dir.glob(pattern))
        if not matches:
            raise RuntimeError(f"No runtime data files match {pattern} under {data_dir}")
        files.extend(Path("data") / path.relative_to(data_dir) for path in matches)
    return files


def rust_loose_sound_files(package_dir: Path = PACKAGE_DIR) -> list[Path]:
    """The COMMITTED loose sound tree, relative to ``package_dir``.

    Committed means asked of git, not read off the disk. Globbing the working
    tree shipped whatever audio a developer happened to have sitting there:
    on 2026-08-24 that was 254 MB of loose source audio the packs already
    contain, in a release that should have been 284 MB and came out at 548.
    The checkout it had always been built from happened to have an almost
    empty tree, which is the only reason the sizes had looked right.

    A release is a statement about what the project ships, so it is built
    from what the project has committed. Anything else is a local accident.

    Git is asked about the repository the tree is actually in, not about
    this file's own checkout: ``package_dir`` is a parameter, and pinning
    the question to ``ROOT`` meant any tree outside it -- a second
    checkout, or the fake one the tests stage from -- raised on the way in
    rather than answering.
    """
    tree = package_dir / LOOSE_SOUND_TREE
    if not tree.is_dir():
        raise RuntimeError(f"Committed sound tree was not found: {tree}")
    listed = subprocess.run(
        ["git", "ls-files", "-z", "--", "."],
        cwd=tree,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    tracked = [tree / name for name in listed.split("\0") if name]
    if not tracked:
        raise RuntimeError(f"git lists no committed files under {tree}")
    return [
        path.relative_to(package_dir)
        for path in sorted(tracked)
        if path.is_file() and "__pycache__" not in path.parts
    ]


def rust_native_libraries(profile_dir: Path, exts: set[str] | None = None) -> list[Path]:
    """Shared libraries the crates' build scripts staged beside the binary.

    Top level only: ``deps/`` holds proc-macro DLLs that are build-time
    artifacts, not runtime libraries.
    """
    suffixes = exts or platform_native_exts()

    def is_runtime_library(path: Path) -> bool:
        suffix = path.suffix.lower()
        if suffix in CARGO_NON_RUNTIME_SUFFIXES:
            return False
        return suffix in suffixes

    return sorted(
        path for path in profile_dir.iterdir() if path.is_file() and is_runtime_library(path)
    )


def macos_linked_libraries(executable: Path) -> list[str]:
    """Return the install names recorded in a Mach-O executable."""
    result = subprocess.run(
        ["otool", "-L", str(executable)],
        check=True,
        capture_output=True,
        text=True,
    )
    install_names: list[str] = []
    for line in result.stdout.splitlines():
        # Dependency rows are indented.  A fat Mach-O repeats an unindented
        # ``<file> (architecture ...):`` header for every slice; that file
        # path is not an install name and must not fail the builder-path audit.
        if not line[:1].isspace():
            continue
        install_name = line.strip().split(" (", 1)[0]
        if install_name and install_name not in install_names:
            install_names.append(install_name)
    return install_names


def macos_dynamic_sdl_dependency(executable: Path) -> Path | None:
    """The SDL2 install name recorded in ``executable``, or ``None``.

    ``None`` is the only shippable answer: the `bundled` + `static-link`
    build compiles SDL2 in, so nothing SDL is loaded at run time. Any
    recorded SDL2 install name means Cargo linked a system SDL -- and since
    Homebrew retired the real SDL2, that library is sdl2-compat, which
    loads SDL3 dynamically where no install-name audit can see it and dies
    on every player Mac without Homebrew ("failed to load sdl3",
    2026-08-30). The staging step refuses to package such an executable.
    """
    for install_name in macos_linked_libraries(executable):
        candidate = Path(install_name)
        if candidate.name.startswith("libSDL2") and candidate.name.endswith(".dylib"):
            return candidate
    return None


def linux_needed_libraries(executable: Path) -> list[str]:
    """The sonames an ELF executable declares it needs (``readelf -d``)."""
    result = subprocess.run(
        ["readelf", "-d", str(executable)],
        check=True,
        capture_output=True,
        text=True,
    )
    needed: list[str] = []
    for line in result.stdout.splitlines():
        if "(NEEDED)" not in line:
            continue
        # ``0x0000000000000001 (NEEDED)  Shared library: [libc.so.6]``
        soname = line.rsplit("[", 1)[-1].rstrip("]").strip()
        if soname and soname not in needed:
            needed.append(soname)
    return needed


def linux_dynamic_sdl_dependency(executable: Path) -> str | None:
    """The SDL2 soname recorded in ``executable``, or ``None``.

    ``None`` is the only shippable answer, for the reason the macOS audit
    gives and one more: every distribution ships a different
    ``libSDL2-2.0.so.0``, and a tarball that has to start on Fedora, Arch
    and Debian alike cannot link against any one of them. The `bundled` +
    `static-link` build compiles SDL2 in, and staging refuses anything else.
    """
    for soname in linux_needed_libraries(executable):
        if soname.startswith("libSDL2"):
            return soname
    return None


def macos_bundle_version(label: str) -> str:
    """A numeric Finder build identity, including the snapshot date."""
    prefix = "1.9-tester-"
    if label.startswith(prefix) and label[len(prefix) :].isdigit():
        date = label[len(prefix) :]
        if len(date) == 8:
            return f"{date[:4]}.{date[4:6]}.{date[6:]}"
    numeric = ".".join(part for part in label.lstrip("v").split(".") if part.isdigit())
    return numeric or "1"


def write_macos_info_plist(app: Path, label: str) -> None:
    """Write the minimal metadata Finder and assistive technology need."""
    short_version = label_package_version(label).split(".dev", 1)[0]
    info = {
        "CFBundleDevelopmentRegion": "en",
        "CFBundleDisplayName": "Freight Fate",
        "CFBundleExecutable": APP_NAME,
        "CFBundleIdentifier": "net.orinks.freight-fate",
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleGetInfoString": f"Freight Fate {short_version} ({label})",
        "CFBundleName": "Freight Fate",
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": short_version,
        "CFBundleVersion": macos_bundle_version(label),
    }
    # No NSAppleEventsUsageDescription, on purpose: without it macOS refuses
    # Prism's AppleScript route to VoiceOver at once instead of asking. The
    # question blocked the game's main thread until answered, freezing the
    # game at launch (issue 266). Speech still reaches VoiceOver through
    # accessibility announcements, which need no permission.
    info_path = app / "Contents" / "Info.plist"
    info_path.parent.mkdir(parents=True, exist_ok=True)
    with info_path.open("wb") as stream:
        plistlib.dump(info, stream, sort_keys=True)


def macos_bundle_binaries(app: Path) -> list[Path]:
    """Mach-O files whose dependencies must stay inside the app or macOS."""
    executable = app / "Contents" / "MacOS" / APP_NAME
    frameworks = app / "Contents" / "Frameworks"
    return [executable, *sorted(frameworks.rglob("*.dylib"))]


def macos_system_install_name(name: str) -> bool:
    return name.startswith(
        ("@rpath/", "@loader_path/", "@executable_path/", "/usr/lib/", "/System/Library/")
    )


def relocate_macos_libraries(app: Path) -> None:
    """Rewrite every bundled dependency away from the builder's filesystem."""
    executable = app / "Contents" / "MacOS" / APP_NAME
    frameworks = app / "Contents" / "Frameworks"
    bundled = {path.name: path for path in frameworks.rglob("*.dylib")}
    for binary in macos_bundle_binaries(app):
        if binary.suffix == ".dylib":
            subprocess.run(
                ["install_name_tool", "-id", f"@rpath/{binary.name}", str(binary)],
                check=True,
            )
        for dependency in macos_linked_libraries(binary):
            if macos_system_install_name(dependency):
                continue
            replacement = bundled.get(Path(dependency).name)
            if replacement is not None:
                subprocess.run(
                    [
                        "install_name_tool",
                        "-change",
                        dependency,
                        f"@rpath/{replacement.name}",
                        str(binary),
                    ],
                    check=True,
                )
    subprocess.run(
        [
            "install_name_tool",
            "-add_rpath",
            "@executable_path/../Frameworks",
            str(executable),
        ],
        check=True,
    )


def verify_macos_native_dependencies(app: Path) -> None:
    """Reject any Mach-O dependency the player's Mac cannot satisfy.

    Two failure shapes: an absolute path into the builder's disk (the
    classic Homebrew leak), and an ``@rpath`` name whose dylib was never
    bundled -- statically clean, dead on arrival. Neither is allowed out.
    """
    frameworks = app / "Contents" / "Frameworks"
    bundled = {path.name for path in frameworks.rglob("*.dylib")}
    forbidden: list[str] = []
    for binary in macos_bundle_binaries(app):
        for dependency in macos_linked_libraries(binary):
            if dependency.startswith("/") and not macos_system_install_name(dependency):
                forbidden.append(f"{binary.name}: {dependency}")
            elif dependency.startswith("@rpath/"):
                leaf = Path(dependency).name
                if leaf != binary.name and leaf not in bundled:
                    forbidden.append(f"{binary.name}: {dependency} (not bundled)")
    if forbidden:
        raise RuntimeError("macOS app contains a builder-local dependency: " + "; ".join(forbidden))


def plan_rust_layout(
    profile_dir: Path,
    package_dir: Path = PACKAGE_DIR,
    platform_name: str = sys.platform,
    native_exts: set[str] | None = None,
    baked_data: Path | None = None,
) -> list[tuple[Path, Path]]:
    """Every (source, destination relative to the staged folder) pair.

    Pure: nothing is copied, so tests can check the plan against a fake
    profile directory and package tree without running cargo. ``baked_data``
    is the container ``bake_world_data`` produced; it is required, because a
    payload without it has no world at all.
    """
    exe = profile_dir / cargo_exe_name(platform_name)
    if not exe.is_file():
        raise RuntimeError(f"cargo build left no executable at {exe}")
    if baked_data is None or not baked_data.is_file():
        raise RuntimeError(
            "the baked data container is missing; run bake_world_data() before staging"
        )
    plan: list[tuple[Path, Path]] = [(exe, Path(rust_exe_name(platform_name)))]
    for lib in rust_native_libraries(profile_dir, native_exts):
        plan.append((lib, Path(lib.name)))
    package = Path("freight_fate")
    plan.append((baked_data, package / "data" / RUST_BAKED_FILE))
    for relative in rust_data_files(package_dir):
        plan.append((package_dir / relative, package / relative))
    for relative in rust_loose_sound_files(package_dir):
        plan.append((package_dir / relative, package / relative))
    # The game's fallback location for BASS add-on plugins
    # (``audio::assets::plugin_lib_dir`` is ``<exe dir>/freight_fate/lib``);
    # the copy beside bass.dll is what normally loads, this one covers a
    # player who swaps in their own BASS.
    if ADDON_LIB_DIR.is_dir():
        suffixes = native_exts or platform_native_exts(platform_name)
        for path in sorted(ADDON_LIB_DIR.iterdir()):
            if path.is_file() and path.suffix.lower() in suffixes:
                plan.append((path, package / "lib" / path.name))
    return plan


def run_cargo(target_dir: Path | None = None) -> Path:
    """Build the release binary and return the profile directory."""
    subprocess.run(cargo_build_command(target_dir), cwd=ROOT, check=True)
    return cargo_profile_dir(target_dir)


def fetch_bass_command() -> list[str]:
    """Return the checkout-local BASS preparation command."""
    return [sys.executable, str(TOOLS / "fetch_bass.py")]


def prepare_rust_release_dependencies() -> None:
    """Restore native audio and the verified music pack before Cargo runs."""
    subprocess.run(fetch_bass_command(), cwd=ROOT, check=True)
    ensure_music_pack()
    ensure_channel3000_pack()


def stage_rust_build(
    profile_dir: Path,
    build_dir: Path = RUST_STAGE_DIR,
    baked_data: Path | None = None,
    platform_name: str = sys.platform,
    label: str | None = None,
) -> Path:
    """Assemble the Rust release folder from the plan plus the packs and docs."""
    if platform_name == "darwin":
        for name in MACOS_REQUIRED_LIBRARIES:
            if not (profile_dir / name).is_file():
                raise RuntimeError(f"Rust build is missing macOS player library {name}")
    elif platform_name == "linux":
        for name in LINUX_REQUIRED_LIBRARIES:
            if not (profile_dir / name).is_file():
                hint = (
                    " Run `uv run python tools/fetch_bass.py` and build again."
                    if name.startswith("libbass")
                    else ""
                )
                raise RuntimeError(f"Rust build is missing Linux player library {name}.{hint}")
    require_real_pack(PACKAGE_DIR / SOURCE_ASSETS / "sounds.pak")
    require_real_pack(PACKAGE_DIR / SOURCE_ASSETS / "music.pak")
    require_real_pack(PACKAGE_DIR / SOURCE_ASSETS / "channel3000.pak")
    plan = plan_rust_layout(
        profile_dir,
        platform_name=platform_name,
        native_exts=platform_native_exts(platform_name),
        baked_data=baked_data,
    )
    if platform_name == "darwin":
        build_dir = build_dir.with_suffix(".app")
    if build_dir.exists():
        shutil.rmtree(build_dir)
    executable_root = runtime_root(build_dir)
    resource_root = (
        build_dir / "Contents" / "Resources" if platform_name == "darwin" else executable_root
    )
    executable_root.mkdir(parents=True)
    resource_root.mkdir(parents=True, exist_ok=True)
    frameworks = build_dir / "Contents" / "Frameworks"
    if platform_name == "darwin":
        frameworks.mkdir(parents=True)
    for source, relative in plan:
        destination = (
            frameworks / source.name
            if platform_name == "darwin" and source.suffix == ".dylib"
            else (
                executable_root / relative
                if relative == Path(rust_exe_name(platform_name))
                else resource_root / relative
            )
        )
        destination.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(source, destination)
    if platform_name == "darwin":
        # Fail fast, before any packaging: a dynamically linked SDL2 can
        # only be Homebrew's sdl2-compat, which needs an SDL3 the player
        # does not have. The build must carry SDL2 statically.
        sdl = macos_dynamic_sdl_dependency(profile_dir / cargo_exe_name(platform_name))
        if sdl is not None:
            raise RuntimeError(
                f"macOS executable links SDL2 dynamically ({sdl}); Homebrew's "
                "sdl2 is now sdl2-compat, which loads SDL3 at runtime and "
                "fails on player Macs. Build with the crate's `bundled` + "
                "`static-link` SDL2 features instead."
            )
        write_macos_info_plist(build_dir, label or project_version())
        relocate_macos_libraries(build_dir)
    elif platform_name == "linux":
        # The same refusal for the same reason, plus one: a system SDL2 is a
        # different file on every distribution, and the tarball has to start
        # on all of them.
        sdl = linux_dynamic_sdl_dependency(profile_dir / cargo_exe_name(platform_name))
        if sdl is not None:
            raise RuntimeError(
                f"Linux executable links SDL2 dynamically ({sdl}); the tarball "
                "has to run on every distribution, so build with the crate's "
                "`bundled` + `static-link` SDL2 features instead."
            )
    if platform_name == "win32":
        stage_windows_runtime(executable_root)
    if platform_name != "win32":
        exe = executable_root / rust_exe_name(platform_name)
        exe.chmod(exe.stat().st_mode | 0o755)
    stage_sound_pack(build_dir, resource_root)
    return build_dir


# A player's machine is hostile territory: anything in the payload can and
# will be read, so a leaked key is not a risk but a disclosure. The shapes
# below cover every kind of credential this project touches (Convex deploy
# keys, Vercel blob tokens, GitHub tokens, private key blocks) plus the
# common cloud formats, matched against the payload's text files. Binaries
# and media are skipped: the game's own code never embeds text credentials,
# and scanning a 100 MB executable for 20-character patterns is pure noise.
SECRET_CONTENT_PATTERNS: tuple[tuple[str, re.Pattern[str]], ...] = (
    ("private key block", re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----")),
    ("GitHub token", re.compile(r"\bgh[pousr]_[A-Za-z0-9]{20,}")),
    ("GitHub fine-grained token", re.compile(r"\bgithub_pat_[A-Za-z0-9_]{20,}")),
    (
        "Convex deploy key",
        re.compile(r"\b(?:prod|preview|dev):[A-Za-z0-9-]+\|[A-Za-z0-9+/=_-]{20,}"),
    ),
    ("Vercel blob token", re.compile(r"\bvercel_blob_rw_[A-Za-z0-9_]{10,}")),
    ("AWS access key", re.compile(r"\bAKIA[0-9A-Z]{16}\b")),
    ("Slack token", re.compile(r"\bxox[baprs]-[A-Za-z0-9-]{10,}")),
    ("OpenAI-style secret key", re.compile(r"\bsk-[A-Za-z0-9_-]{24,}")),
    ("JWT", re.compile(r"\beyJ[A-Za-z0-9_-]{14,}\.eyJ[A-Za-z0-9_-]{14,}\.")),
)

SECRET_SCAN_SUFFIXES = {
    ".cfg",
    ".css",
    ".html",
    ".ini",
    ".js",
    ".json",
    ".md",
    ".plist",
    ".toml",
    ".txt",
    ".xml",
    ".yaml",
    ".yml",
}


def verify_no_shipped_secrets(build_dir: Path) -> None:
    """Fail the build if the staged payload carries anything secret-shaped.

    Two checks: file NAMES that only ever hold credentials (`.env*`,
    ``*.pem``, anything mentioning a deploy key), and file CONTENT matching
    a known token format. Both fail fast and name the file, because the one
    thing worse than a leaked key is a leaked key nobody noticed shipping.
    """
    findings: list[str] = []
    for path in sorted(build_dir.rglob("*")):
        if not path.is_file():
            continue
        relative = path.relative_to(build_dir).as_posix()
        name = path.name.lower()
        if name.startswith(".env") or name.endswith(".pem") or "deploy_key" in name:
            findings.append(f"{relative}: secret-shaped file name")
            continue
        if path.suffix.lower() not in SECRET_SCAN_SUFFIXES:
            continue
        try:
            text = path.read_text(encoding="utf-8", errors="ignore")
        except OSError:
            continue
        for label, pattern in SECRET_CONTENT_PATTERNS:
            if pattern.search(text):
                findings.append(f"{relative}: looks like a {label}")
    if findings:
        raise RuntimeError(
            "Release payload contains secret-shaped content and must not ship: "
            + "; ".join(findings)
        )


def verify_rust_payload(build_dir: Path, platform_name: str = sys.platform) -> None:
    """Prove the staged Rust folder holds what the binary loads."""
    executable_root = runtime_root(build_dir)
    root = build_dir / "Contents" / "Resources" if build_dir.suffix == ".app" else executable_root
    exe = executable_root / rust_exe_name(platform_name)
    required = [
        exe,
        root / "build_info.json",
        root / "LICENSE.txt",
        root / "CHANGELOG.md",
        root / "USER_MANUAL.md",
        root / "USER_MANUAL.html",
        root / "ALPHA_TEST_BOOK.md",
        root / "ALPHA_TEST_BOOK.html",
        root / "SOUND_CREDITS.md",
        root / "freight_fate" / "sounds.pak",
        root / "freight_fate" / "music.pak",
        root / "freight_fate" / "channel3000.pak",
        root / "freight_fate" / LOOSE_SOUND_TREE / "CREDITS.md",
    ]
    data_dir = root / "freight_fate" / "data"
    container = data_dir / RUST_BAKED_FILE
    required.append(container)
    required.extend(data_dir / relative for relative in RUST_DATA_FILES)
    missing = [path for path in required if not path.exists()]
    if missing:
        raise RuntimeError(
            "Rust payload is incomplete: "
            + ", ".join(path.relative_to(build_dir).as_posix() for path in missing)
        )
    # Not just present: the real container. A truncated copy, or the wrong
    # file under the right name, must not reach a player as "no world data".
    with open(container, "rb") as f:
        if f.read(8) != BAKED_MAGIC:
            raise RuntimeError(f"{RUST_BAKED_FILE} is not a Freight Fate baked data container")
    if container.stat().st_size < 1_000_000:
        raise RuntimeError(
            f"{RUST_BAKED_FILE} is only {container.stat().st_size} bytes; "
            "the real container holds the whole map"
        )
    # And the JSON it replaced stays home: shipping both doubles the download
    # and leaves two answers to every question.
    doubled = [
        relative
        for relative in RUST_BAKED_SOURCE_FILES
        if (data_dir / relative).exists() and relative not in RUST_DATA_FILES
    ]
    if (data_dir / "world_data").exists():
        doubled.append("world_data/")
    if doubled:
        raise RuntimeError(
            "Rust payload ships the JSON the baked container replaced: "
            + ", ".join(sorted(doubled)[:10])
        )

    package = root / "freight_fate"
    leaked = [
        path.relative_to(build_dir).as_posix()
        for path in package.rglob("*")
        if path.name in ("world_source", "__pycache__", LICENSED_SOUND_TREE) or path.suffix == ".py"
    ]
    if leaked:
        raise RuntimeError(
            "Rust payload ships source-only files: " + ", ".join(sorted(leaked)[:10])
        )

    if platform_name == "win32":
        verify_windows_runtime(executable_root)
        for name in ("SDL2.dll", "bass.dll"):
            if not (root / name).exists():
                # BASS is fetched rather than committed, so a checkout that
                # skipped the fetch would otherwise build a silent release and
                # say nothing about why.
                hint = (
                    " Run `uv run python tools/fetch_bass.py` and build again."
                    if name.startswith("bass")
                    else ""
                )
                raise RuntimeError(f"Rust payload is missing the native library {name}.{hint}")
    elif platform_name == "darwin":
        frameworks = build_dir / "Contents" / "Frameworks"
        # SDL2 is deliberately absent: it is compiled into the executable
        # (`bundled` + `static-link`), and staging refuses a dynamic link.
        missing_macos = [
            name for name in MACOS_REQUIRED_LIBRARIES if not (frameworks / name).exists()
        ]
        if missing_macos:
            raise RuntimeError(
                "Rust macOS payload is missing native libraries: " + ", ".join(missing_macos)
            )
    elif platform_name == "linux":
        # SDL2 and Prism are compiled in here too; what must be beside the
        # executable is BASS with its decoders.
        missing_linux = [name for name in LINUX_REQUIRED_LIBRARIES if not (root / name).exists()]
        if missing_linux:
            raise RuntimeError(
                "Rust Linux payload is missing native libraries: " + ", ".join(missing_linux)
            )
    elif not native_files(root):
        print(
            "Warning: no native libraries staged beside the executable; the game "
            "will need system SDL2/BASS on this platform."
        )

    verify_sound_packs(build_dir)
    verify_no_shipped_secrets(build_dir)

    if platform_name != "win32" and os.name != "nt" and not exe.stat().st_mode & 0o111:
        raise RuntimeError(f"Packaged executable is not runnable: {exe.relative_to(build_dir)}")


def build_rust(
    label: str,
    target_dir: Path | None,
    run_smoke: bool,
    *,
    macos_non_launch_verify: bool = False,
) -> Path:
    """The whole ``--rust`` pipeline, ending with the verified archive."""
    if run_smoke and macos_non_launch_verify:
        raise RuntimeError("process smoke and macOS non-launch verification are mutually exclusive")
    if macos_non_launch_verify and sys.platform != "darwin":
        raise RuntimeError("macOS non-launch verification requires macOS")

    prepare_rust_release_dependencies()
    profile_dir = run_cargo(target_dir)
    baked_data = bake_world_data(target_dir)
    build_dir = stage_rust_build(profile_dir, baked_data=baked_data, label=label)
    resource_root = (
        build_dir / "Contents" / "Resources" if build_dir.suffix == ".app" else build_dir
    )
    stamp_build_info(build_dir, label, resource_root)
    stage_release_docs(build_dir, resource_root)
    verify_rust_payload(build_dir)
    strip_user_data(build_dir)
    if run_smoke:
        smoke_check(build_dir)
        # Retain the last-line privacy defense even though smoke redirects its
        # data and log paths.  A runtime fallback must never enter the archive.
        strip_user_data(build_dir)
    elif macos_non_launch_verify:
        # GitHub-hosted macOS has repeatedly left a correctly relocated,
        # deeply signed arm64 process before Rust logging for the full bound.
        # The release proof therefore stays non-launching: payload structure
        # above, native dependency audit + strict signature verification below,
        # and archive structure/content verification after compression.
        print("Using non-launch macOS packaged verification on the hosted runner.")
    else:
        print("Skipped the packaged process smoke check.")
    if sys.platform == "darwin":
        # Prove the archive input after every possible smoke-side mutation.
        sign_distribution(build_dir)
        notarize_distribution(build_dir)
    DIST.mkdir(parents=True, exist_ok=True)
    out = archive(build_dir, label)
    verify_archive(out)
    return out


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--tag", default="", help="release label override, e.g. 1.9-tester-20260828"
    )
    parser.add_argument("--skip-smoke", action="store_true", help="skip booting the staged build")
    parser.add_argument(
        "--check-dependencies",
        action="store_true",
        help="accepted for old callers; there is nothing to check before cargo runs",
    )
    parser.add_argument(
        "--rust",
        action="store_true",
        help="accepted for old callers; the Rust game is the only build",
    )
    parser.add_argument(
        "--cargo-target-dir",
        default="",
        help="Cargo target directory (default: target/)",
    )
    parser.add_argument(
        "--smoke",
        action="store_true",
        help="boot the staged build headless before archiving it",
    )
    parser.add_argument(
        "--macos-non-launch-verify",
        action="store_true",
        help=(
            "verify a macOS app's payload, native dependencies, signature, "
            "and archive without launching it"
        ),
    )
    args = parser.parse_args()

    if args.macos_non_launch_verify and args.smoke:
        parser.error("--macos-non-launch-verify cannot be combined with --smoke")
    if args.macos_non_launch_verify and args.skip_smoke:
        parser.error("--macos-non-launch-verify cannot be combined with --skip-smoke")
    if args.macos_non_launch_verify and sys.platform != "darwin":
        parser.error("--macos-non-launch-verify requires macOS")

    if args.check_dependencies:
        print("Release dependency check passed: the Rust build fetches its own.")
        return 0

    label = args.tag or project_version()
    target_dir = Path(args.cargo_target_dir).resolve() if args.cargo_target_dir else None
    out = build_rust(
        label,
        target_dir,
        run_smoke=args.smoke and not args.skip_smoke,
        macos_non_launch_verify=args.macos_non_launch_verify,
    )
    print(f"Built {out} ({out.stat().st_size / 1e6:.1f} MB)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
