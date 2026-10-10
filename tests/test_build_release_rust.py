"""The ``--rust`` mode of tools/build_release.py: layout planning without cargo.

Everything here runs against a fake Cargo profile directory and a fake
package tree, so it proves what the staged ``FreightFate/`` folder would hold
(and what it would refuse) without building or copying the real game.
"""

from __future__ import annotations

import importlib.util
import io
import json
import plistlib
import subprocess
import urllib.error
import zipfile
from pathlib import Path

import pytest


def load_build_release_module():
    path = Path(__file__).resolve().parents[1] / "tools" / "build_release.py"
    spec = importlib.util.spec_from_file_location("build_release", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def make_package_tree(package_dir: Path, build_release) -> None:
    """A package tree carrying every shipped data file plus source-only noise."""
    data = package_dir / "data"
    for relative in build_release.RUST_DATA_FILES + build_release.RUST_BAKED_SOURCE_FILES:
        path = data / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("{}\n", encoding="utf-8")
    legs = data / "world_data" / "us" / "legs"
    legs.mkdir(parents=True, exist_ok=True)
    (legs / "TX.json").write_text("{}\n", encoding="utf-8")
    (legs / "CA.json").write_text("{}\n", encoding="utf-8")
    # Source-only material that must never reach a player.
    (data / "world_source" / "legs").mkdir(parents=True)
    (data / "world_source" / "legs" / "TX.json").write_text("{}\n", encoding="utf-8")
    (data / "__pycache__").mkdir()
    (data / "__pycache__" / "world.cpython-312.pyc").write_bytes(b"")
    (data / "world.py").write_text("", encoding="utf-8")
    (data / "world_data" / "us" / "geometry").mkdir()
    (data / "world_data" / "us" / "geometry" / "tx.jsonl").write_text("", encoding="utf-8")
    (data / "world_data" / "us" / "gameplay" / "ramps.jsonl").write_text("", encoding="utf-8")
    sounds = package_dir / "assets" / "sounds"
    (sounds / "engine_classic").mkdir(parents=True)
    (sounds / "CREDITS.md").write_text("# Credits\n", encoding="utf-8")
    (sounds / "engine_classic" / "idle.ogg").write_bytes(b"ogg")
    licensed = package_dir / "assets" / "sounds-licensed" / "engine"
    licensed.mkdir(parents=True)
    (licensed / "idle.ogg").write_bytes(b"never ships loose")


def make_container(path: Path, build_release, size: int = 2_000_000) -> Path:
    """A stand-in for what ``ff-bake`` writes: right magic, plausible size."""
    path.parent.mkdir(parents=True, exist_ok=True)
    magic = build_release.BAKED_MAGIC
    path.write_bytes(magic + bytes(size - len(magic)))
    return path


def make_profile_dir(profile_dir: Path, exe_name: str) -> None:
    """What ``cargo build --release`` leaves behind on Windows."""
    profile_dir.mkdir(parents=True)
    (profile_dir / exe_name).write_bytes(b"MZ")
    for name in ("SDL2.dll", "bass.dll", "bassopus.dll", "basshls.dll"):
        (profile_dir / name).write_bytes(b"dll")
    (profile_dir / "freightfate.pdb").write_bytes(b"pdb")
    (profile_dir / "libfreight_fate.rlib").write_bytes(b"rlib")
    deps = profile_dir / "deps"
    deps.mkdir()
    (deps / "serde_derive-abc123.dll").write_bytes(b"proc macro")


def track_everything(root: Path) -> None:
    """Make `root` a repository whose whole tree is tracked.

    The staging plan asks git which loose sounds are committed, so a fake
    package tree only means anything once git knows about it. Staging the
    index is enough -- `git ls-files` reads the index, so nothing has to be
    committed and no author identity is needed.
    """
    for args in (("init", "-q"), ("add", "-A")):
        subprocess.run(["git", *args], cwd=root, check=True, capture_output=True)


@pytest.fixture
def planned(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    package_dir = tmp_path / "repo"
    make_package_tree(package_dir, build_release)
    track_everything(tmp_path)
    # Left on the disk after git was told what the project ships: the 254 MB
    # of source audio that got into one release is exactly this, and it must
    # not reach the plan.
    (package_dir / "assets" / "sounds" / "idle_take3_source.wav").write_bytes(b"local accident")
    addon = tmp_path / "addon_lib"
    addon.mkdir()
    (addon / "basshls.dll").write_bytes(b"hls")
    (addon / "basshls.txt").write_text("licence", encoding="utf-8")
    monkeypatch.setattr(build_release, "ADDON_LIB_DIR", addon)
    profile_dir = tmp_path / "target" / "release"
    make_profile_dir(profile_dir, "freightfate.exe")
    baked = make_container(tmp_path / "build" / build_release.RUST_BAKED_FILE, build_release)
    plan = build_release.plan_rust_layout(
        profile_dir,
        package_dir=package_dir,
        platform_name="win32",
        native_exts={".dll"},
        baked_data=baked,
    )
    return build_release, plan, profile_dir, package_dir


def destinations(plan) -> set[str]:
    return {relative.as_posix() for _, relative in plan}


def test_plan_renames_the_cargo_binary_to_the_updater_name(planned):
    _, plan, profile_dir, _ = planned
    exe_sources = [(src, rel) for src, rel in plan if rel.as_posix() == "FreightFate.exe"]
    assert exe_sources == [(profile_dir / "freightfate.exe", Path("FreightFate.exe"))]
    assert "freightfate.exe" not in destinations(plan)


def test_exe_name_follows_the_platform():
    build_release = load_build_release_module()
    assert build_release.rust_exe_name("win32") == "FreightFate.exe"
    assert build_release.rust_exe_name("linux") == "FreightFate"
    assert build_release.rust_exe_name("darwin") == "FreightFate"
    assert build_release.cargo_exe_name("win32") == "freightfate.exe"
    assert build_release.cargo_exe_name("linux") == "freightfate"


def test_plan_ships_the_baked_container_instead_of_the_json_tree(planned):
    build_release, plan, _, _ = planned
    dests = destinations(plan)
    assert f"freight_fate/data/{build_release.RUST_BAKED_FILE}" in dests
    # Everything the container holds stays home: 142 MB of JSON is exactly
    # what shipping the container was for.
    for relative in build_release.RUST_BAKED_SOURCE_FILES:
        assert f"freight_fate/data/{relative}" not in dests
    assert not any("/world_data/" in dest for dest in dests)
    assert "freight_fate/data/world_data/us/legs/TX.json" not in dests
    for relative in build_release.RUST_DATA_FILES:
        assert f"freight_fate/data/{relative}" in dests
    assert not any("world_source" in dest for dest in dests)
    assert not any("__pycache__" in dest for dest in dests)
    assert not any(dest.endswith(".py") for dest in dests)
    assert not any("/geometry/" in dest for dest in dests)


def test_plan_refuses_to_stage_without_a_baked_container(planned, tmp_path):
    build_release, _, profile_dir, package_dir = planned
    with pytest.raises(RuntimeError, match="baked data container is missing"):
        build_release.plan_rust_layout(
            profile_dir, package_dir=package_dir, platform_name="win32", native_exts={".dll"}
        )
    with pytest.raises(RuntimeError, match="baked data container is missing"):
        build_release.plan_rust_layout(
            profile_dir,
            package_dir=package_dir,
            platform_name="win32",
            native_exts={".dll"},
            baked_data=tmp_path / "nowhere" / "world.ffdata",
        )


def test_bake_command_names_the_data_tree_and_the_container(tmp_path):
    build_release = load_build_release_module()
    out = tmp_path / "world.ffdata"
    cmd = build_release.bake_command(out)
    assert cmd[:7] == ["cargo", "run", "--release", "-p", "ff-core", "--bin", "ff-bake"]
    assert cmd[cmd.index("--out") + 1] == str(out)
    assert cmd[cmd.index("--data-dir") + 1] == str(build_release.PACKAGE_DIR / "data")
    assert "--check" not in cmd
    target = tmp_path / "t18"
    checked = build_release.bake_command(out, target, check=True)
    assert checked[checked.index("--target-dir") + 1] == str(target)
    assert checked[-1] == "--check"


def test_plan_ships_committed_sounds_but_never_the_licensed_overlay(planned):
    _, plan, _, _ = planned
    dests = destinations(plan)
    assert "freight_fate/assets/sounds/CREDITS.md" in dests
    assert "freight_fate/assets/sounds/engine_classic/idle.ogg" in dests
    assert not any("sounds-licensed" in dest for dest in dests)


def test_plan_leaves_uncommitted_sounds_on_the_builders_disk(planned):
    """The 548 MB release: loose audio nobody had committed went out in it.

    Committed means asked of git, so a file sitting in the tree that git has
    never heard of is a local accident and stays home.
    """
    _, plan, _, _ = planned
    assert "freight_fate/assets/sounds/idle_take3_source.wav" not in destinations(plan)


def test_plan_stages_only_top_level_runtime_libraries(planned):
    _, plan, _, _ = planned
    dests = destinations(plan)
    for name in ("SDL2.dll", "bass.dll", "bassopus.dll", "basshls.dll"):
        assert name in dests
    assert "freightfate.pdb" not in dests
    assert "libfreight_fate.rlib" not in dests
    assert not any("serde_derive" in dest for dest in dests)
    # The game's own fallback plugin folder gets the add-on, not its licence note.
    assert "freight_fate/lib/basshls.dll" in dests
    assert "freight_fate/lib/basshls.txt" not in dests


def test_plan_refuses_a_profile_dir_without_the_binary(tmp_path):
    build_release = load_build_release_module()
    package_dir = tmp_path / "repo"
    make_package_tree(package_dir, build_release)
    profile_dir = tmp_path / "target" / "release"
    profile_dir.mkdir(parents=True)
    with pytest.raises(RuntimeError, match="no executable"):
        build_release.plan_rust_layout(profile_dir, package_dir=package_dir, platform_name="win32")


def test_plan_refuses_a_checkout_missing_a_loose_runtime_data_file(tmp_path, monkeypatch):
    """A file registered as shipping loose has to be there.

    ``RUST_DATA_FILES`` is empty today -- the container covers everything the
    game loads -- so the rule is exercised through a file added to it, which
    is what registering a new runtime data file looks like.
    """
    build_release = load_build_release_module()
    package_dir = tmp_path / "repo"
    make_package_tree(package_dir, build_release)
    monkeypatch.setattr(build_release, "RUST_DATA_FILES", ("late_addition.json",))
    profile_dir = tmp_path / "target" / "release"
    make_profile_dir(profile_dir, "freightfate")
    baked = make_container(tmp_path / "build" / build_release.RUST_BAKED_FILE, build_release)
    with pytest.raises(RuntimeError, match="late_addition.json"):
        build_release.plan_rust_layout(
            profile_dir, package_dir=package_dir, platform_name="linux", baked_data=baked
        )


def test_bake_refuses_a_checkout_without_a_data_tree(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    monkeypatch.setattr(build_release, "PACKAGE_DIR", tmp_path / "repo")
    with pytest.raises(RuntimeError, match="Runtime data tree is missing"):
        build_release.bake_world_data(out=tmp_path / "world.ffdata")


def test_lfs_pointer_is_refused_with_a_pull_hint(tmp_path):
    build_release = load_build_release_module()
    pointer = tmp_path / "sounds.pak"
    pointer.write_bytes(
        b"version https://git-lfs.github.com/spec/v1\noid sha256:0123456789abcdef\nsize 7781859\n"
    )
    assert build_release.is_lfs_pointer(pointer)
    with pytest.raises(RuntimeError, match="git lfs pull"):
        build_release.require_real_pack(pointer)

    real = tmp_path / "music.pak"
    real.write_bytes(b"FFPK1\x00" + b"\x00" * 64)
    assert not build_release.is_lfs_pointer(real)
    build_release.require_real_pack(real)

    with pytest.raises(RuntimeError, match="not found"):
        build_release.require_real_pack(tmp_path / "absent.pak")


def test_music_download_config_uses_public_defaults(monkeypatch):
    build_release = load_build_release_module()
    monkeypatch.delenv("FREIGHT_FATE_MUSIC_URL", raising=False)
    monkeypatch.delenv("FREIGHT_FATE_MUSIC_SHA256", raising=False)
    # Production, once FREIGHT_FATE_MUSIC_BLOB_URL existed on that scope and
    # the route was seen answering 307 (2026-09-20). An earlier move to this
    # host, made while the variable was Preview-only, 503d every platform of
    # the snapshot build -- so if this pin ever has to move back, check the
    # route with curl rather than checking that its file deployed.
    url, sha = build_release.music_download_config()
    assert url == "https://www.orinks.net/downloads/music.pak"
    assert sha == build_release.DEFAULT_MUSIC_SHA256


def test_music_download_config_allows_independent_overrides():
    build_release = load_build_release_module()
    assert build_release.music_download_config(
        {
            "FREIGHT_FATE_MUSIC_URL": "https://example.test/music.pak",
            "FREIGHT_FATE_MUSIC_SHA256": "A" * 64,
        }
    ) == ("https://example.test/music.pak", "a" * 64)


def test_ensure_music_pack_keeps_a_verified_existing_pack(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    pack = tmp_path / "music.pak"
    pack.write_bytes(b"approved pack")
    digest = build_release.hashlib.sha256(pack.read_bytes()).hexdigest()
    monkeypatch.setenv("FREIGHT_FATE_MUSIC_SHA256", digest)
    monkeypatch.setattr(
        build_release,
        "download_to_path",
        lambda *_: pytest.fail("downloaded"),
    )
    build_release.ensure_music_pack(pack)


def test_music_download_config_rejects_a_non_hex_digest(monkeypatch):
    build_release = load_build_release_module()
    monkeypatch.setenv("FREIGHT_FATE_MUSIC_SHA256", "not-a-digest")
    with pytest.raises(
        RuntimeError,
        match="FREIGHT_FATE_MUSIC_SHA256 must be a 64-character hexadecimal digest",
    ):
        build_release.music_download_config()


def test_download_to_path_streams_the_response(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    destination = tmp_path / "music.pak"
    response = io.BytesIO(b"streamed pack")
    monkeypatch.setattr(build_release.urllib.request, "urlopen", lambda _request: response)

    build_release.download_to_path(
        urllib.request.Request("https://example.test/music.pak"), destination
    )

    assert destination.read_bytes() == b"streamed pack"
    assert response.closed


def test_ensure_music_pack_atomically_installs_a_verified_download(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    pack = tmp_path / "music.pak"
    payload = b"replacement pack"
    digest = build_release.hashlib.sha256(payload).hexdigest()
    monkeypatch.setenv("FREIGHT_FATE_MUSIC_URL", "https://example.test/music.pak")
    monkeypatch.setenv("FREIGHT_FATE_MUSIC_SHA256", digest)

    def download(url, destination):
        assert url.full_url == "https://example.test/music.pak"
        Path(destination).write_bytes(payload)

    monkeypatch.setattr(build_release, "download_to_path", download)
    build_release.ensure_music_pack(pack)

    assert pack.read_bytes() == payload
    assert list(tmp_path.glob("*.download")) == []


def test_ensure_music_pack_identifies_the_release_downloader(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    pack = tmp_path / "music.pak"
    payload = b"replacement pack"
    digest = build_release.hashlib.sha256(payload).hexdigest()
    monkeypatch.setenv("FREIGHT_FATE_MUSIC_URL", "https://example.test/music.pak")
    monkeypatch.setenv("FREIGHT_FATE_MUSIC_SHA256", digest)

    def download(request, destination):
        assert isinstance(request, urllib.request.Request)
        assert request.full_url == "https://example.test/music.pak"
        assert request.get_header("User-agent") == "Freight-Fate-release-builder/1.9"
        Path(destination).write_bytes(payload)

    monkeypatch.setattr(build_release, "download_to_path", download)
    build_release.ensure_music_pack(pack)

    assert pack.read_bytes() == payload


def test_ensure_music_pack_rejects_a_mismatched_download(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    pack = tmp_path / "music.pak"
    monkeypatch.setenv("FREIGHT_FATE_MUSIC_SHA256", "a" * 64)
    monkeypatch.setattr(
        build_release,
        "download_to_path",
        lambda _url, destination: Path(destination).write_bytes(b"unapproved pack"),
    )

    with pytest.raises(RuntimeError, match="failed SHA-256 verification"):
        build_release.ensure_music_pack(pack)

    assert not pack.exists()
    assert list(tmp_path.glob("*.download")) == []


def test_ensure_music_pack_replaces_a_mismatched_existing_pack_after_verification(
    tmp_path, monkeypatch
):
    build_release = load_build_release_module()
    pack = tmp_path / "music.pak"
    pack.write_bytes(b"old pack")
    payload = b"new approved pack"
    digest = build_release.hashlib.sha256(payload).hexdigest()
    monkeypatch.setenv("FREIGHT_FATE_MUSIC_SHA256", digest)
    monkeypatch.setattr(
        build_release,
        "download_to_path",
        lambda _url, destination: Path(destination).write_bytes(payload),
    )

    build_release.ensure_music_pack(pack)

    assert pack.read_bytes() == payload
    assert list(tmp_path.glob("*.download")) == []


def test_ensure_music_pack_preserves_existing_pack_when_download_fails(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    pack = tmp_path / "music.pak"
    old_payload = b"old pack"
    pack.write_bytes(old_payload)
    monkeypatch.setenv(
        "FREIGHT_FATE_MUSIC_SHA256",
        build_release.hashlib.sha256(b"new approved pack").hexdigest(),
    )

    def fail_download(_url, _destination):
        raise urllib.error.URLError("download unavailable")

    monkeypatch.setattr(build_release, "download_to_path", fail_download)
    with pytest.raises(
        RuntimeError,
        match=(
            "Music-pack download failed: download unavailable. "
            "Check your connection and retry the build."
        ),
    ) as exc:
        build_release.ensure_music_pack(pack)

    assert isinstance(exc.value.__cause__, urllib.error.URLError)
    assert pack.read_bytes() == old_payload
    assert list(tmp_path.glob("*.download")) == []


def test_ensure_music_pack_reports_http_status_and_preserves_the_cause(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    pack = tmp_path / "music.pak"
    monkeypatch.setenv("FREIGHT_FATE_MUSIC_SHA256", "a" * 64)

    def fail_download(url, _destination):
        raise urllib.error.HTTPError(url, 503, "Service Unavailable", None, None)

    monkeypatch.setattr(build_release, "download_to_path", fail_download)
    with pytest.raises(
        RuntimeError,
        match=(
            "Music-pack download failed with HTTP status 503. "
            "Check your connection and retry the build."
        ),
    ) as exc:
        build_release.ensure_music_pack(pack)

    assert isinstance(exc.value.__cause__, urllib.error.HTTPError)
    assert exc.value.__cause__.code == 503
    assert not pack.exists()
    assert list(tmp_path.glob("*.download")) == []


def test_ensure_music_pack_does_not_mislabel_local_io_failures(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    pack = tmp_path / "music.pak"
    monkeypatch.setenv("FREIGHT_FATE_MUSIC_SHA256", "a" * 64)

    def fail_download(_url, _destination):
        raise PermissionError("destination is read-only")

    monkeypatch.setattr(build_release, "download_to_path", fail_download)
    with pytest.raises(PermissionError, match="destination is read-only"):
        build_release.ensure_music_pack(pack)

    assert not pack.exists()
    assert list(tmp_path.glob("*.download")) == []


def test_cargo_command_honours_the_target_dir(tmp_path):
    build_release = load_build_release_module()
    assert build_release.cargo_build_command() == [
        "cargo",
        "build",
        "--release",
        "-p",
        "freight-fate",
        # A player build carries no agent server.
        "--no-default-features",
    ]
    target = tmp_path / "t53"
    assert build_release.cargo_build_command(target)[-2:] == ["--target-dir", str(target)]
    assert build_release.cargo_profile_dir(target) == target / "release"
    assert build_release.cargo_profile_dir() == build_release.ROOT / "target" / "release"


def test_prepare_rust_release_dependencies_fetches_bass_then_music(monkeypatch):
    """Release builds restore native audio before fetching the music and Channel 3000 packs."""
    build_release = load_build_release_module()
    calls = []
    monkeypatch.setattr(
        build_release.subprocess,
        "run",
        lambda command, **kwargs: calls.append((command, kwargs)),
    )
    monkeypatch.setattr(build_release, "ensure_music_pack", lambda: calls.append(("music", {})))
    monkeypatch.setattr(
        build_release, "ensure_channel3000_pack", lambda: calls.append(("channel3000", {}))
    )

    build_release.prepare_rust_release_dependencies()

    assert calls[0][0] == [build_release.sys.executable, str(build_release.TOOLS / "fetch_bass.py")]
    assert calls[0][1] == {"cwd": build_release.ROOT, "check": True}
    assert calls[1][0] == "music"
    assert calls[2][0] == "channel3000"


def test_windows_release_wrapper_is_the_complete_beginner_command():
    root = Path(__file__).resolve().parents[1]
    script = (root / "build-release.ps1").read_text(encoding="utf-8")
    readme_heading = "## Build a standalone copy"
    assert readme_heading in (root / "README.md").read_text(encoding="utf-8")
    assert readme_heading.removeprefix("## ") in script
    assert "Get-Command $tool" in script
    for tool in ("rustup", "rustc", "cargo", "uv", "cmake"):
        assert f'"{tool}"' in script
    assert "atlbase.h" in script
    assert "Microsoft.VisualStudio.Component.VC.ATL" in script
    assert "rustup show" in script
    assert "uv sync --group dev" in script
    assert "--group build" not in script
    assert "uv run python tools/build_release.py --rust --smoke" in script
    assert "Start-Process" not in script


def test_unix_release_wrapper_mirrors_the_windows_one():
    root = Path(__file__).resolve().parents[1]
    path = root / "build-release.sh"
    script = path.read_text(encoding="utf-8")
    readme = (root / "README.md").read_text(encoding="utf-8")
    assert "./build-release.sh" in readme
    tracked_mode = subprocess.run(
        ["git", "ls-files", "-s", "build-release.sh"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    ).stdout.split()[0]
    assert tracked_mode == "100755"
    assert script.startswith("#!/usr/bin/env bash\n")
    assert "Build a standalone copy" in script
    for tool in ("rustup", "rustc", "cargo", "uv", "cmake"):
        assert f"need {tool}" in script
    assert "rustup show" in script
    assert "uv sync --group dev" in script
    assert "--group build" not in script
    assert 'tools/build_release.py --rust --smoke "$@"' in script
    assert "gcc-13" in script


def test_main_accepts_the_rust_flags(capsys):
    build_release = load_build_release_module()
    with pytest.raises(SystemExit) as exc:
        import sys

        old = sys.argv
        sys.argv = ["build_release.py", "--help"]
        try:
            build_release.main()
        finally:
            sys.argv = old
    assert exc.value.code == 0
    out = capsys.readouterr().out
    assert "--rust" in out
    assert "--cargo-target-dir" in out
    assert "--smoke" in out
    assert "--macos-non-launch-verify" in out


def test_windows_smoke_timeout_uses_isolated_data_and_prints_the_packaged_log(
    tmp_path, monkeypatch, capsys
):
    """The retained Windows smoke reports phases without reading player data."""
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate"
    executable = app / "FreightFate.exe"
    app.mkdir()
    executable.write_bytes(b"Mach-O")
    monkeypatch.setattr(build_release.sys, "platform", "win32")

    def time_out(command, **kwargs):
        smoke_env = kwargs["env"]
        log_path = Path(smoke_env["FREIGHT_FATE_LOG_FILE"])
        player_data = Path(smoke_env["FREIGHT_FATE_DATA_DIR"])
        assert player_data == log_path.parent / "player-data"
        log_path.parent.mkdir(parents=True, exist_ok=True)
        log_path.write_text(
            "phase: smoke checks 12 ms (elapsed 50 ms)\n"
            "phase: driver identity 17 ms (elapsed 67 ms)\n",
            encoding="utf-8",
        )
        raise subprocess.TimeoutExpired(command, kwargs["timeout"])

    monkeypatch.setattr(build_release.subprocess, "run", time_out)

    with pytest.raises(subprocess.TimeoutExpired):
        build_release.smoke_check(app)

    err = capsys.readouterr().err
    assert "Packaged smoke log before failure:" in err
    assert "phase: smoke checks 12 ms" in err
    assert "phase: driver identity 17 ms" in err


def test_stable_macos_smoke_uses_the_bounded_headless_app_path(tmp_path, monkeypatch):
    """The Career-only pivot must not break the generic stable Mac builder."""
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    executable = app / "Contents" / "MacOS" / "FreightFate"
    executable.parent.mkdir(parents=True)
    executable.write_bytes(b"Mach-O")
    calls = []

    monkeypatch.setattr(
        build_release.subprocess,
        "run",
        lambda command, **kwargs: calls.append((command, kwargs)),
    )

    build_release.smoke_check(app)

    assert calls[0][0] == [str(executable), "--smoke", "--headless"]
    assert calls[0][1]["cwd"] == executable.parent
    assert calls[0][1]["timeout"] == 120


def make_macos_profile(profile_dir: Path) -> None:
    profile_dir.mkdir(parents=True)
    (profile_dir / "freightfate").write_bytes(b"Mach-O")
    for name in (
        "libbass.dylib",
        "libbasshls.dylib",
        "libbassopus.dylib",
        "libbassflac.dylib",
    ):
        (profile_dir / name).write_bytes(b"dylib")


def test_macos_stage_is_a_player_ready_app_bundle(tmp_path, monkeypatch):
    """A bare executable folder cannot be opened like a normal Mac app."""
    build_release = load_build_release_module()
    package_dir = tmp_path / "repo"
    make_package_tree(package_dir, build_release)
    track_everything(tmp_path)
    (package_dir / "assets" / "sounds.pak").write_bytes(b"FFPK1 sounds")
    (package_dir / "assets" / "music.pak").write_bytes(b"FFPK1 music")
    (package_dir / "assets" / "channel3000.pak").write_bytes(b"FFPK1 c3k")
    profile_dir = tmp_path / "target" / "release"
    make_macos_profile(profile_dir)
    baked = make_container(tmp_path / "world.ffdata", build_release)
    monkeypatch.setattr(build_release, "PACKAGE_DIR", package_dir)
    monkeypatch.setattr(build_release, "ADDON_LIB_DIR", tmp_path / "no-addons")
    monkeypatch.setattr(build_release, "macos_dynamic_sdl_dependency", lambda _exe: None)
    monkeypatch.setattr(build_release, "relocate_macos_libraries", lambda _app: None)

    app = build_release.stage_rust_build(
        profile_dir,
        build_dir=tmp_path / "build" / "FreightFate",
        baked_data=baked,
        platform_name="darwin",
        label="1.9-tester-20260830",
    )

    assert app == tmp_path / "build" / "FreightFate.app"
    assert (app / "Contents" / "MacOS" / "FreightFate").is_file()
    assert list((app / "Contents" / "MacOS").iterdir()) == [
        app / "Contents" / "MacOS" / "FreightFate"
    ]
    resources = app / "Contents" / "Resources"
    build_release.stamp_build_info(app, "1.9-tester-20260830", resources)
    stamp = json.loads((resources / "build_info.json").read_text(encoding="utf-8"))
    assert stamp["commit"] == build_release.build_commit()
    build_release.stage_release_docs(app, resources)
    assert (resources / "freight_fate" / "data" / "world.ffdata").is_file()
    assert (resources / "freight_fate" / "sounds.pak").is_file()
    assert (resources / "freight_fate" / "music.pak").is_file()
    assert (resources / "freight_fate" / "channel3000.pak").is_file()
    assert (resources / "SOUND_CREDITS.md").is_file()
    for name in (
        "build_info.json",
        "CHANGELOG.md",
        "LICENSE.txt",
        "USER_MANUAL.md",
        "USER_MANUAL.html",
        "ALPHA_TEST_BOOK.md",
        "ALPHA_TEST_BOOK.html",
    ):
        assert (resources / name).is_file()
    frameworks = app / "Contents" / "Frameworks"
    for name in (
        "libbass.dylib",
        "libbasshls.dylib",
        "libbassopus.dylib",
        "libbassflac.dylib",
    ):
        assert (frameworks / name).is_file()
    with (app / "Contents" / "Info.plist").open("rb") as stream:
        info = plistlib.load(stream)
    assert info["CFBundleExecutable"] == "FreightFate"
    assert info["CFBundleName"] == "Freight Fate"
    assert info["CFBundleIdentifier"] == "net.orinks.freight-fate"
    assert info["CFBundleVersion"] == "2026.08.30"
    assert info["CFBundleGetInfoString"] == "Freight Fate 1.9.0 (1.9-tester-20260830)"
    assert "NSAppleEventsUsageDescription" not in info
    assert "LSMinimumSystemVersion" not in info


def test_macos_stage_refuses_missing_player_libraries(tmp_path, monkeypatch):
    """A silent or Homebrew-dependent app must never become a release."""
    build_release = load_build_release_module()
    package_dir = tmp_path / "repo"
    make_package_tree(package_dir, build_release)
    track_everything(tmp_path)
    profile_dir = tmp_path / "target" / "release"
    profile_dir.mkdir(parents=True)
    (profile_dir / "freightfate").write_bytes(b"Mach-O")
    baked = make_container(tmp_path / "world.ffdata", build_release)
    monkeypatch.setattr(build_release, "PACKAGE_DIR", package_dir)
    monkeypatch.setattr(build_release, "ADDON_LIB_DIR", tmp_path / "no-addons")

    with pytest.raises(RuntimeError, match="missing macOS player library libbass.dylib"):
        build_release.stage_rust_build(
            profile_dir,
            build_dir=tmp_path / "build" / "FreightFate",
            baked_data=baked,
            platform_name="darwin",
        )


def test_macos_dynamic_sdl_dependency_reads_the_install_names(tmp_path, monkeypatch):
    """Any recorded SDL2 install name is reported; none at all is None."""
    build_release = load_build_release_module()
    exe = tmp_path / "freightfate"
    exe.write_bytes(b"Mach-O")
    sdl = "/opt/homebrew/opt/sdl2-compat/lib/libSDL2-2.0.0.dylib"
    otool = f"{exe}:\n\t{sdl} (compatibility version 1.0.0)\n\t/usr/lib/libSystem.B.dylib\n"
    result = subprocess.CompletedProcess(["otool", "-L", str(exe)], 0, otool, "")
    monkeypatch.setattr(build_release.subprocess, "run", lambda *_args, **_kwargs: result)
    assert build_release.macos_dynamic_sdl_dependency(exe) == Path(sdl)

    static_otool = f"{exe}:\n\t/usr/lib/libSystem.B.dylib\n"
    static_result = subprocess.CompletedProcess(["otool", "-L", str(exe)], 0, static_otool, "")
    monkeypatch.setattr(build_release.subprocess, "run", lambda *_args, **_kwargs: static_result)
    assert build_release.macos_dynamic_sdl_dependency(exe) is None


def test_macos_stage_refuses_a_dynamically_linked_sdl(tmp_path, monkeypatch):
    """Homebrew's sdl2 is sdl2-compat: it loads SDL3 at runtime, so a zip
    built against it dies on every player Mac without Homebrew. Staging must
    fail the build, not bundle the shim ("failed to load sdl3", 2026-08-30).
    """
    build_release = load_build_release_module()
    package_dir = tmp_path / "repo"
    make_package_tree(package_dir, build_release)
    track_everything(tmp_path)
    (package_dir / "assets" / "sounds.pak").write_bytes(b"FFPK1 sounds")
    (package_dir / "assets" / "music.pak").write_bytes(b"FFPK1 music")
    (package_dir / "assets" / "channel3000.pak").write_bytes(b"FFPK1 c3k")
    profile_dir = tmp_path / "target" / "release"
    make_macos_profile(profile_dir)
    baked = make_container(tmp_path / "world.ffdata", build_release)
    monkeypatch.setattr(build_release, "PACKAGE_DIR", package_dir)
    monkeypatch.setattr(build_release, "ADDON_LIB_DIR", tmp_path / "no-addons")
    monkeypatch.setattr(
        build_release,
        "macos_dynamic_sdl_dependency",
        lambda _exe: Path("/opt/homebrew/opt/sdl2-compat/lib/libSDL2-2.0.0.dylib"),
    )

    with pytest.raises(RuntimeError, match="links SDL2 dynamically"):
        build_release.stage_rust_build(
            profile_dir,
            build_dir=tmp_path / "build" / "FreightFate",
            baked_data=baked,
            platform_name="darwin",
        )


def make_linux_profile(profile_dir: Path) -> None:
    """What ``cargo build --release`` leaves behind on Linux: the executable
    and BASS with its decoders (Prism is linked in)."""
    profile_dir.mkdir(parents=True)
    (profile_dir / "freightfate").write_bytes(b"ELF")
    for name in (
        "libbass.so",
        "libbassopus.so",
        "libbasshls.so",
        "libbassflac.so",
    ):
        (profile_dir / name).write_bytes(b"so")
    (profile_dir / "freightfate.d").write_text("deps", encoding="utf-8")
    (profile_dir / "libfreight_fate.rlib").write_bytes(b"rlib")


def test_linux_stage_ships_bass_and_its_decoders(tmp_path, monkeypatch):
    """The Linux payload carries BASS and its decoders beside the executable."""
    build_release = load_build_release_module()
    package_dir = tmp_path / "repo"
    make_package_tree(package_dir, build_release)
    track_everything(tmp_path)
    (package_dir / "assets" / "sounds.pak").write_bytes(b"FFPK1 sounds")
    (package_dir / "assets" / "music.pak").write_bytes(b"FFPK1 music")
    (package_dir / "assets" / "channel3000.pak").write_bytes(b"FFPK1 c3k")
    profile_dir = tmp_path / "target" / "release"
    make_linux_profile(profile_dir)
    baked = make_container(tmp_path / "world.ffdata", build_release)
    monkeypatch.setattr(build_release, "PACKAGE_DIR", package_dir)
    monkeypatch.setattr(build_release, "ADDON_LIB_DIR", tmp_path / "no-addons")
    monkeypatch.setattr(build_release, "linux_dynamic_sdl_dependency", lambda _exe: None)

    build_dir = build_release.stage_rust_build(
        profile_dir,
        build_dir=tmp_path / "build" / "FreightFate",
        baked_data=baked,
        platform_name="linux",
        label="1.9-tester-20260902",
    )

    assert build_dir == tmp_path / "build" / "FreightFate"
    assert (build_dir / "FreightFate").is_file()
    for name in build_release.LINUX_REQUIRED_LIBRARIES:
        assert (build_dir / name).is_file(), name
    assert not (build_dir / "freightfate.d").exists()
    assert not (build_dir / "libfreight_fate.rlib").exists()
    assert (build_dir / "freight_fate" / "data" / "world.ffdata").is_file()


@pytest.mark.parametrize("missing_name", ["libbass.so", "libbassopus.so"])
def test_linux_stage_refuses_missing_player_libraries(tmp_path, monkeypatch, missing_name):
    """A mute or speechless tarball must never become a release."""
    build_release = load_build_release_module()
    package_dir = tmp_path / "repo"
    make_package_tree(package_dir, build_release)
    track_everything(tmp_path)
    profile_dir = tmp_path / "target" / "release"
    make_linux_profile(profile_dir)
    (profile_dir / missing_name).unlink()
    baked = make_container(tmp_path / "world.ffdata", build_release)
    monkeypatch.setattr(build_release, "PACKAGE_DIR", package_dir)
    monkeypatch.setattr(build_release, "ADDON_LIB_DIR", tmp_path / "no-addons")

    with pytest.raises(RuntimeError, match=f"missing Linux player library {missing_name}"):
        build_release.stage_rust_build(
            profile_dir,
            build_dir=tmp_path / "build" / "FreightFate",
            baked_data=baked,
            platform_name="linux",
        )


def test_linux_dynamic_sdl_dependency_reads_the_needed_sonames(tmp_path, monkeypatch):
    """Any NEEDED libSDL2 soname is reported; a static build reports none."""
    build_release = load_build_release_module()
    exe = tmp_path / "freightfate"
    exe.write_bytes(b"ELF")
    dynamic = (
        "Dynamic section at offset 0x1000 contains 30 entries:\n"
        "  Tag        Type                         Name/Value\n"
        " 0x0000000000000001 (NEEDED)             Shared library: [libSDL2-2.0.so.0]\n"
        " 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]\n"
        " 0x000000000000001d (RUNPATH)            Library runpath: [$ORIGIN]\n"
    )
    result = subprocess.CompletedProcess(["readelf", "-d", str(exe)], 0, dynamic, "")
    monkeypatch.setattr(build_release.subprocess, "run", lambda *_args, **_kwargs: result)
    assert build_release.linux_needed_libraries(exe) == ["libSDL2-2.0.so.0", "libc.so.6"]
    assert build_release.linux_dynamic_sdl_dependency(exe) == "libSDL2-2.0.so.0"

    static = (
        " 0x0000000000000001 (NEEDED)             Shared library: [libgcc_s.so.1]\n"
        " 0x0000000000000001 (NEEDED)             Shared library: [libc.so.6]\n"
    )
    static_result = subprocess.CompletedProcess(["readelf", "-d", str(exe)], 0, static, "")
    monkeypatch.setattr(build_release.subprocess, "run", lambda *_args, **_kwargs: static_result)
    assert build_release.linux_dynamic_sdl_dependency(exe) is None


def test_linux_stage_refuses_a_dynamically_linked_sdl(tmp_path, monkeypatch):
    """Every distribution ships a different libSDL2-2.0.so.0; a tarball
    linked against the builder's would not start on the others."""
    build_release = load_build_release_module()
    package_dir = tmp_path / "repo"
    make_package_tree(package_dir, build_release)
    track_everything(tmp_path)
    (package_dir / "assets" / "sounds.pak").write_bytes(b"FFPK1 sounds")
    (package_dir / "assets" / "music.pak").write_bytes(b"FFPK1 music")
    (package_dir / "assets" / "channel3000.pak").write_bytes(b"FFPK1 c3k")
    profile_dir = tmp_path / "target" / "release"
    make_linux_profile(profile_dir)
    baked = make_container(tmp_path / "world.ffdata", build_release)
    monkeypatch.setattr(build_release, "PACKAGE_DIR", package_dir)
    monkeypatch.setattr(build_release, "ADDON_LIB_DIR", tmp_path / "no-addons")
    monkeypatch.setattr(
        build_release, "linux_dynamic_sdl_dependency", lambda _exe: "libSDL2-2.0.so.0"
    )

    with pytest.raises(RuntimeError, match="links SDL2 dynamically"):
        build_release.stage_rust_build(
            profile_dir,
            build_dir=tmp_path / "build" / "FreightFate",
            baked_data=baked,
            platform_name="linux",
        )


def write_linux_tarball(path: Path, names: list[str], build_release) -> None:
    import tarfile

    with tarfile.open(path, "w:gz") as tar:
        for name in names:
            data = (
                build_release.BAKED_MAGIC + bytes(8)
                if name.endswith(build_release.RUST_BAKED_FILE)
                else b"payload"
            )
            info = tarfile.TarInfo(f"FreightFate/{name}")
            info.size = len(data)
            info.mode = 0o755 if name == "FreightFate" else 0o644
            tar.addfile(info, io.BytesIO(data))


@pytest.mark.parametrize("missing_name", ["libbass.so", "libbassopus.so", "libbassflac.so"])
def test_linux_archive_verifier_rejects_a_rust_tarball_missing_a_library(tmp_path, missing_name):
    build_release = load_build_release_module()
    names = [
        "FreightFate",
        "build_info.json",
        "LICENSE.txt",
        "USER_MANUAL.md",
        "freight_fate/sounds.pak",
        "freight_fate/music.pak",
        "freight_fate/channel3000.pak",
        build_release.RUST_BAKED_FILE_ENTRY,
        *(name for name in build_release.LINUX_REQUIRED_LIBRARIES if name != missing_name),
    ]
    out = tmp_path / "FreightFate-1.9-tester-20260902-linux-x64.tar.gz"
    write_linux_tarball(out, names, build_release)

    with pytest.raises(RuntimeError, match=f"FreightFate/{missing_name}"):
        build_release.verify_archive(out)

    write_linux_tarball(out, [*names, missing_name], build_release)
    build_release.verify_archive(out)


@pytest.mark.parametrize(
    "machine, suffix",
    [
        ("x86_64", "linux-x64"),
        ("AMD64", "linux-x64"),
        # The Blazie BT Speak and BT Braille, Raspberry Pi, and GitHub's ARM
        # runner all report aarch64; the Mac spelling is accepted too.
        ("aarch64", "linux-arm64"),
        ("arm64", "linux-arm64"),
    ],
)
def test_linux_tarball_is_named_for_its_architecture(machine, suffix):
    build_release = load_build_release_module()
    assert build_release.linux_archive_suffix(machine) == suffix
    assert f"-{suffix}.tar.gz" in build_release.LINUX_TARBALL_SUFFIXES


def test_linux_tarball_has_no_name_for_an_architecture_nobody_builds():
    build_release = load_build_release_module()
    with pytest.raises(RuntimeError, match="riscv64"):
        build_release.linux_archive_suffix("riscv64")


def test_linux_archive_verifier_checks_the_arm64_rust_tarball_too(tmp_path):
    """The ARM64 tarball is the same layout under a different name; the
    verifier must recognise it rather than reject the name outright, and
    must demand the same libraries of it."""
    build_release = load_build_release_module()
    names = [
        "FreightFate",
        "build_info.json",
        "LICENSE.txt",
        "USER_MANUAL.md",
        "freight_fate/sounds.pak",
        "freight_fate/music.pak",
        "freight_fate/channel3000.pak",
        build_release.RUST_BAKED_FILE_ENTRY,
        *(name for name in build_release.LINUX_REQUIRED_LIBRARIES if name != "libbass.so"),
    ]
    out = tmp_path / "FreightFate-1.9-tester-20260902-linux-arm64.tar.gz"
    write_linux_tarball(out, names, build_release)
    with pytest.raises(RuntimeError, match="FreightFate/libbass.so"):
        build_release.verify_archive(out)
    write_linux_tarball(out, [*names, "libbass.so"], build_release)
    build_release.verify_archive(out)


def load_build_appimage_module():
    path = Path(__file__).resolve().parents[1] / "tools" / "build_appimage.py"
    spec = importlib.util.spec_from_file_location("build_appimage", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


@pytest.mark.parametrize(
    "machine, arch", [("x86_64", "x86_64"), ("aarch64", "aarch64"), ("arm64", "aarch64")]
)
def test_appimage_tooling_and_name_follow_the_architecture(machine, arch):
    """linuxdeploy and the AppImage runtime are published per `uname -m`
    under one release; the output file is named the same way."""
    build_appimage = load_build_appimage_module()
    assert build_appimage.appimage_architecture(machine) == arch
    assert build_appimage.LINUXDEPLOY_URL.format(arch=arch).endswith(f"linuxdeploy-{arch}.AppImage")
    assert build_appimage.APPIMAGE_RUNTIME_URL.format(arch=arch).endswith(f"runtime-{arch}")
    assert arch in build_appimage.SUPPORTED_ARCHITECTURES


def test_appimage_tooling_refuses_an_architecture_it_has_no_tools_for():
    build_appimage = load_build_appimage_module()
    with pytest.raises(RuntimeError, match="riscv64"):
        build_appimage.appimage_architecture("riscv64")


def test_macos_linked_libraries_ignores_fat_macho_slice_headers(tmp_path, monkeypatch):
    """Every fat-binary architecture header names the file, not a dependency."""
    build_release = load_build_release_module()
    dylib = tmp_path / "libbass.dylib"
    dylib.write_bytes(b"Mach-O")

    def run(command, **_kwargs):
        assert command == ["otool", "-L", str(dylib)]
        output = (
            f"{dylib} (architecture x86_64):\n"
            "\t@rpath/libbass.dylib (compatibility version 1.0.0)\n"
            "\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0)\n"
            f"{dylib} (architecture arm64):\n"
            "\t@rpath/libbass.dylib (compatibility version 1.0.0)\n"
            "\t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0)\n"
        )
        return subprocess.CompletedProcess(command, 0, output, "")

    monkeypatch.setattr(build_release.subprocess, "run", run)

    assert build_release.macos_linked_libraries(dylib) == [
        "@rpath/libbass.dylib",
        "/usr/lib/libSystem.B.dylib",
    ]


def test_macos_relocation_makes_sdl_bundle_relative(tmp_path, monkeypatch):
    """The app must not retain a build-machine Homebrew path."""
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    exe = app / "Contents" / "MacOS" / "FreightFate"
    frameworks = app / "Contents" / "Frameworks"
    exe.parent.mkdir(parents=True)
    frameworks.mkdir(parents=True)
    exe.write_bytes(b"Mach-O")
    sdl = frameworks / "libSDL2-2.0.0.dylib"
    sdl.write_bytes(b"SDL")
    calls = []

    def run(command, **kwargs):
        calls.append((command, kwargs))
        if command[:2] == ["otool", "-L"]:
            return subprocess.CompletedProcess(
                command,
                0,
                f"{exe}:\n\t/opt/homebrew/lib/libSDL2-2.0.0.dylib (compatibility version 1.0.0)\n",
                "",
            )
        return subprocess.CompletedProcess(command, 0, "", "")

    monkeypatch.setattr(build_release.subprocess, "run", run)
    build_release.relocate_macos_libraries(app)

    commands = [call[0] for call in calls]
    assert [
        "install_name_tool",
        "-change",
        "/opt/homebrew/lib/libSDL2-2.0.0.dylib",
        "@rpath/libSDL2-2.0.0.dylib",
        str(exe),
    ] in commands
    assert [
        "install_name_tool",
        "-add_rpath",
        "@executable_path/../Frameworks",
        str(exe),
    ] in commands
    assert ["install_name_tool", "-id", "@rpath/libSDL2-2.0.0.dylib", str(sdl)] in commands


def test_macos_archive_name_matches_updater_suffix(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    app.mkdir()
    monkeypatch.setattr(build_release, "DIST", tmp_path / "dist")
    monkeypatch.setattr(build_release.sys, "platform", "darwin")
    monkeypatch.setattr(build_release.platform, "machine", lambda: "arm64")
    monkeypatch.setattr(build_release.subprocess, "run", lambda *_args, **_kwargs: None)
    (tmp_path / "dist").mkdir()

    out = build_release.archive(app, "1.9-tester-20260830")

    assert out.name == "FreightFate-1.9-tester-20260830-macos-arm64.zip"


def test_apple_silicon_stable_archive_keeps_legacy_suffix(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    app.mkdir()
    monkeypatch.setattr(build_release, "DIST", tmp_path / "dist")
    monkeypatch.setattr(build_release.sys, "platform", "darwin")
    monkeypatch.setattr(build_release.platform, "machine", lambda: "arm64")
    monkeypatch.setattr(build_release.subprocess, "run", lambda *_args, **_kwargs: None)
    (tmp_path / "dist").mkdir()

    out = build_release.archive(app, "v1.8.8")

    assert out.name == "FreightFate-v1.8.8-macos.zip"


def test_apple_silicon_stable_19_archive_matches_the_workflow_upload(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    app.mkdir()
    monkeypatch.setattr(build_release, "DIST", tmp_path / "dist")
    monkeypatch.setattr(build_release.sys, "platform", "darwin")
    monkeypatch.setattr(build_release.platform, "machine", lambda: "arm64")
    monkeypatch.setattr(build_release.subprocess, "run", lambda *_args, **_kwargs: None)
    (tmp_path / "dist").mkdir()

    out = build_release.archive(app, "v1.9.0")

    # build-career-1.9.yml uploads only `*-macos-arm64.zip` and fails on none.
    assert out.name == "FreightFate-v1.9.0-macos-arm64.zip"


def test_intel_macos_archive_keeps_legacy_suffix(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    app.mkdir()
    monkeypatch.setattr(build_release, "DIST", tmp_path / "dist")
    monkeypatch.setattr(build_release.sys, "platform", "darwin")
    monkeypatch.setattr(build_release.platform, "machine", lambda: "x86_64")
    monkeypatch.setattr(build_release.subprocess, "run", lambda *_args, **_kwargs: None)
    (tmp_path / "dist").mkdir()

    out = build_release.archive(app, "1.8.8")

    assert out.name == "FreightFate-1.8.8-macos.zip"


def test_macos_uses_non_launch_verification_before_archive(tmp_path, monkeypatch):
    """Hosted Mac proof stays structural, native, signed, and non-launching."""
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    archive = tmp_path / "FreightFate-test-macos.zip"
    events = []
    monkeypatch.setattr(build_release.sys, "platform", "darwin")
    monkeypatch.setattr(build_release, "prepare_rust_release_dependencies", lambda: None)
    monkeypatch.setattr(build_release, "run_cargo", lambda _target: tmp_path / "release")
    monkeypatch.setattr(build_release, "bake_world_data", lambda _target: tmp_path / "world.ffdata")
    monkeypatch.setattr(build_release, "stage_rust_build", lambda *_args, **_kwargs: app)
    monkeypatch.setattr(build_release, "stamp_build_info", lambda *_args: events.append("stamp"))
    monkeypatch.setattr(build_release, "stage_release_docs", lambda *_args: events.append("docs"))
    monkeypatch.setattr(build_release, "verify_rust_payload", lambda _app: events.append("verify"))
    monkeypatch.setattr(build_release, "smoke_check", lambda _app: events.append("smoke"))
    monkeypatch.setattr(build_release, "strip_user_data", lambda _app: events.append("strip"))
    monkeypatch.setattr(build_release, "sign_distribution", lambda _app: events.append("sign"))
    monkeypatch.setattr(
        build_release, "notarize_distribution", lambda _app: events.append("notarize")
    )
    monkeypatch.setattr(build_release, "archive", lambda *_args: archive)
    monkeypatch.setattr(build_release, "verify_archive", lambda _archive: events.append("archive"))

    build_release.build_rust("test", None, False, macos_non_launch_verify=True)

    assert events == ["stamp", "docs", "verify", "strip", "sign", "notarize", "archive"]


def test_windows_keeps_packaged_process_smoke(tmp_path, monkeypatch):
    """Removing the unsupported hosted Mac launch must not weaken Windows."""
    build_release = load_build_release_module()
    build = tmp_path / "FreightFate"
    archive = tmp_path / "FreightFate-test-windows-portable.zip"
    events = []
    monkeypatch.setattr(build_release.sys, "platform", "win32")
    monkeypatch.setattr(build_release, "prepare_rust_release_dependencies", lambda: None)
    monkeypatch.setattr(build_release, "run_cargo", lambda _target: tmp_path / "release")
    monkeypatch.setattr(build_release, "bake_world_data", lambda _target: tmp_path / "world.ffdata")
    monkeypatch.setattr(build_release, "stage_rust_build", lambda *_args, **_kwargs: build)
    monkeypatch.setattr(build_release, "stamp_build_info", lambda *_args: events.append("stamp"))
    monkeypatch.setattr(build_release, "stage_release_docs", lambda *_args: events.append("docs"))
    monkeypatch.setattr(build_release, "verify_rust_payload", lambda _app: events.append("verify"))
    monkeypatch.setattr(build_release, "smoke_check", lambda _app: events.append("smoke"))
    monkeypatch.setattr(build_release, "strip_user_data", lambda _app: events.append("strip"))
    monkeypatch.setattr(build_release, "sign_distribution", lambda _app: events.append("sign"))
    monkeypatch.setattr(build_release, "archive", lambda *_args: archive)
    monkeypatch.setattr(build_release, "verify_archive", lambda _archive: events.append("archive"))

    build_release.build_rust("test", None, True)

    assert events == ["stamp", "docs", "verify", "strip", "smoke", "strip", "archive"]


def test_full_macos_bundle_verification_reads_packs_from_resources(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    executable = app / "Contents" / "MacOS" / "FreightFate"
    resources = app / "Contents" / "Resources"
    frameworks = app / "Contents" / "Frameworks"
    executable.parent.mkdir(parents=True)
    resources.mkdir(parents=True)
    frameworks.mkdir(parents=True)
    executable.write_bytes(b"Mach-O")
    executable.chmod(0o755)
    for name in (
        "libbass.dylib",
        "libbassopus.dylib",
        "libbasshls.dylib",
        "libbassflac.dylib",
        "libSDL2-2.0.0.dylib",
    ):
        (frameworks / name).write_bytes(b"dylib")
    required = (
        "build_info.json",
        "LICENSE.txt",
        "CHANGELOG.md",
        "USER_MANUAL.md",
        "USER_MANUAL.html",
        "ALPHA_TEST_BOOK.md",
        "ALPHA_TEST_BOOK.html",
        "SOUND_CREDITS.md",
        "freight_fate/sounds.pak",
        "freight_fate/music.pak",
        "freight_fate/channel3000.pak",
        "freight_fate/assets/sounds/CREDITS.md",
    )
    for relative in required:
        path = resources / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"payload")
    make_container(resources / "freight_fate" / "data" / "world.ffdata", build_release)
    opened = []

    class FakeSoundPack:
        def __init__(self, path):
            opened.append(Path(path))

        def names(self):
            return ["engine_classic/idle.ogg", "music/road_song.ogg", "c3k/day_show_01.opus"]

    fake_assets = type("FakeAssets", (), {"SoundPack": FakeSoundPack})
    fake_tool = type("FakeTool", (), {"_load_assets_pack": staticmethod(lambda: fake_assets)})
    monkeypatch.setattr(build_release, "_load_tool", lambda _name: fake_tool)
    monkeypatch.setattr(build_release.sys, "platform", "darwin")

    build_release.verify_rust_payload(app, platform_name="darwin")

    assert opened == [
        resources / "freight_fate" / "sounds.pak",
        resources / "freight_fate" / "music.pak",
        resources / "freight_fate" / "channel3000.pak",
    ]


@pytest.mark.parametrize(
    ("missing_name", "error_fragment"),
    [
        ("libbass.dylib", "libbass.dylib"),
        ("libbassopus.dylib", "libbassopus.dylib"),
        ("libbasshls.dylib", "libbasshls.dylib"),
        ("libbassflac.dylib", "libbassflac.dylib"),
    ],
)
def test_macos_staged_payload_rejects_each_missing_runtime_library(
    tmp_path, monkeypatch, missing_name, error_fragment
):
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    executable = app / "Contents" / "MacOS" / "FreightFate"
    resources = app / "Contents" / "Resources"
    frameworks = app / "Contents" / "Frameworks"
    executable.parent.mkdir(parents=True)
    resources.mkdir(parents=True)
    frameworks.mkdir(parents=True)
    executable.write_bytes(b"Mach-O")
    executable.chmod(0o755)
    for name in build_release.MACOS_REQUIRED_LIBRARIES:
        if name != missing_name:
            (frameworks / name).write_bytes(b"dylib")
    required = (
        "build_info.json",
        "LICENSE.txt",
        "CHANGELOG.md",
        "USER_MANUAL.md",
        "USER_MANUAL.html",
        "ALPHA_TEST_BOOK.md",
        "ALPHA_TEST_BOOK.html",
        "SOUND_CREDITS.md",
        "freight_fate/sounds.pak",
        "freight_fate/music.pak",
        "freight_fate/channel3000.pak",
        "freight_fate/assets/sounds/CREDITS.md",
    )
    for relative in required:
        path = resources / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(b"payload")
    make_container(resources / "freight_fate" / "data" / "world.ffdata", build_release)
    fake_assets = type("FakeAssets", (), {"SoundPack": lambda _self, _path: None})
    fake_tool = type("FakeTool", (), {"_load_assets_pack": staticmethod(lambda: fake_assets)})
    monkeypatch.setattr(build_release, "_load_tool", lambda _name: fake_tool)

    with pytest.raises(RuntimeError, match=error_fragment):
        build_release.verify_rust_payload(app, platform_name="darwin")


def write_macos_archive(
    path: Path,
    payload_root: str,
    include_icon: bool = True,
) -> None:
    entries = {
        "FreightFate.app/Contents/MacOS/FreightFate": b"Mach-O",
        "FreightFate.app/Contents/Info.plist": b"plist",
        "FreightFate.app/Contents/Frameworks/libbass.dylib": b"bass",
        "FreightFate.app/Contents/Frameworks/libbassopus.dylib": b"bassopus",
        "FreightFate.app/Contents/Frameworks/libbasshls.dylib": b"basshls",
        "FreightFate.app/Contents/Frameworks/libbassflac.dylib": b"bassflac",
        "FreightFate.app/Contents/Frameworks/libSDL2-2.0.0.dylib": b"sdl",
        f"{payload_root}/build_info.json": b"{}",
        f"{payload_root}/LICENSE.txt": b"license",
        f"{payload_root}/USER_MANUAL.md": b"manual",
        f"{payload_root}/freight_fate/sounds.pak": b"sounds",
        f"{payload_root}/freight_fate/music.pak": b"music",
        f"{payload_root}/freight_fate/channel3000.pak": b"c3k",
        f"{payload_root}/freight_fate/data/world.ffdata": b"FFDATA",
    }
    if include_icon:
        entries["FreightFate.app/Contents/Resources/FreightFate.icns"] = b"icon"
    with zipfile.ZipFile(path, "w") as archive:
        for name, content in entries.items():
            info = zipfile.ZipInfo(name)
            info.external_attr = (0o755 if name.endswith("/FreightFate") else 0o644) << 16
            archive.writestr(info, content)


def test_archive_verifier_detects_new_rust_payload_by_resources_build_info(tmp_path):
    build_release = load_build_release_module()
    archive = tmp_path / "FreightFate-1.9-tester-20260830-macos.zip"
    write_macos_archive(archive, "FreightFate.app/Contents/Resources")
    build_release.verify_archive(archive)


@pytest.mark.parametrize(
    ("missing_name", "error_fragment"),
    [
        ("libbass.dylib", "libbass.dylib"),
        ("libbassopus.dylib", "libbassopus.dylib"),
        ("libbasshls.dylib", "libbasshls.dylib"),
        ("libbassflac.dylib", "libbassflac.dylib"),
    ],
)
def test_macos_archive_verifier_rejects_each_missing_runtime_library(
    tmp_path, missing_name, error_fragment
):
    archive = tmp_path / "FreightFate-broken-macos-arm64.zip"
    write_macos_archive(archive, "FreightFate.app/Contents/Resources")
    rewritten = tmp_path / "rewritten.zip"
    missing_entry = f"FreightFate.app/Contents/Frameworks/{missing_name}"
    with zipfile.ZipFile(archive) as source, zipfile.ZipFile(rewritten, "w") as target:
        for info in source.infolist():
            if info.filename != missing_entry:
                target.writestr(info, source.read(info.filename))
    archive.unlink()
    rewritten.rename(archive)

    build_release = load_build_release_module()
    with pytest.raises(RuntimeError, match=error_fragment):
        build_release.verify_archive(archive)


def test_macos_audit_rejects_builder_local_dependencies(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    exe = app / "Contents" / "MacOS" / "FreightFate"
    exe.parent.mkdir(parents=True)
    exe.write_bytes(b"Mach-O")
    monkeypatch.setattr(
        build_release,
        "macos_linked_libraries",
        lambda _binary: ["/opt/homebrew/Cellar/sdl2/2.30/lib/libSDL2.dylib"],
    )

    with pytest.raises(RuntimeError, match="builder-local dependency"):
        build_release.verify_macos_native_dependencies(app)


def test_macos_signing_verifies_the_final_bundle(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    app.mkdir()
    calls = []
    monkeypatch.setattr(build_release.sys, "platform", "darwin")
    monkeypatch.setattr(
        build_release.subprocess,
        "run",
        lambda command, **kwargs: calls.append((command, kwargs)),
    )
    monkeypatch.setattr(build_release, "verify_macos_native_dependencies", lambda _app: None)

    build_release.sign_distribution(app, environ={})

    assert calls == [
        (["codesign", "--force", "--deep", "--sign", "-", str(app)], {"check": True}),
        (
            ["codesign", "--verify", "--deep", "--strict", str(app)],
            {"check": True},
        ),
    ]


def test_macos_developer_id_signing_seals_libraries_before_the_bundle(tmp_path, monkeypatch):
    """Notarization rejects --deep: each dylib is signed alone, then the app."""
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    (app / "Contents" / "MacOS").mkdir(parents=True)
    (app / "Contents" / "MacOS" / "FreightFate").write_bytes(b"")
    frameworks = app / "Contents" / "Frameworks"
    frameworks.mkdir()
    for name in ("libbass.dylib", "libbassopus.dylib"):
        (frameworks / name).write_bytes(b"")
    calls = []
    monkeypatch.setattr(build_release.sys, "platform", "darwin")
    monkeypatch.setattr(
        build_release.subprocess, "run", lambda command, **_kwargs: calls.append(command)
    )
    monkeypatch.setattr(build_release, "verify_macos_native_dependencies", lambda _app: None)
    identity = "Developer ID Application: Test (TEAM123456)"

    build_release.sign_distribution(
        app,
        environ={"MACOS_SIGN_IDENTITY": identity, "MACOS_SIGN_KEYCHAIN": "/tmp/ci.keychain-db"},
    )

    def signed(target):
        return [
            "codesign",
            "--force",
            "--options",
            "runtime",
            "--timestamp",
            "--keychain",
            "/tmp/ci.keychain-db",
            "--sign",
            identity,
            str(target),
        ]

    assert calls == [
        signed(frameworks / "libbass.dylib"),
        signed(frameworks / "libbassopus.dylib"),
        signed(app),
        ["codesign", "--verify", "--deep", "--strict", str(app)],
    ]
    assert not any("--deep" in command and "--sign" in command for command in calls)


def test_macos_notarization_is_skipped_without_credentials(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    calls = []
    monkeypatch.setattr(build_release.sys, "platform", "darwin")
    monkeypatch.setattr(build_release.subprocess, "run", lambda *args, **_kw: calls.append(args))

    build_release.notarize_distribution(tmp_path / "FreightFate.app", environ={})

    assert calls == []


def test_macos_notarization_refuses_half_configured_credentials(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    monkeypatch.setattr(build_release.sys, "platform", "darwin")

    with pytest.raises(RuntimeError, match="MACOS_NOTARY_ISSUER_ID"):
        build_release.notarize_distribution(
            tmp_path / "FreightFate.app",
            environ={
                "MACOS_SIGN_IDENTITY": "Developer ID Application: Test (TEAM123456)",
                "MACOS_NOTARY_KEY_PATH": "/tmp/AuthKey.p8",
                "MACOS_NOTARY_KEY_ID": "ABC123DEFG",
            },
        )


NOTARY_ENV = {
    "MACOS_SIGN_IDENTITY": "Developer ID Application: Test (TEAM123456)",
    "MACOS_NOTARY_KEY_PATH": "/tmp/AuthKey.p8",
    "MACOS_NOTARY_KEY_ID": "ABC123DEFG",
    "MACOS_NOTARY_ISSUER_ID": "00000000-0000-0000-0000-000000000000",
}


def fake_notary(monkeypatch, build_release, status):
    calls = []

    def run(command, **_kwargs):
        calls.append(command)
        stdout = ""
        if command[:3] == ["xcrun", "notarytool", "submit"]:
            stdout = json.dumps({"id": "sub-1", "status": status})
        return subprocess.CompletedProcess(command, 0, stdout=stdout, stderr="")

    monkeypatch.setattr(build_release.sys, "platform", "darwin")
    monkeypatch.setattr(build_release.subprocess, "run", run)
    return calls


def test_macos_notarization_staples_and_assesses_an_accepted_app(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    calls = fake_notary(monkeypatch, build_release, "Accepted")

    build_release.notarize_distribution(app, environ=NOTARY_ENV)

    tools = [command[:3] for command in calls]
    assert tools[0][0] == "ditto"
    assert tools[1] == ["xcrun", "notarytool", "submit"]
    assert "--wait" in calls[1]
    assert calls[2:] == [
        ["xcrun", "stapler", "staple", str(app)],
        ["xcrun", "stapler", "validate", str(app)],
        ["spctl", "--assess", "--type", "execute", "--verbose=2", str(app)],
    ]


def test_macos_notarization_fails_the_build_when_apple_rejects(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    calls = fake_notary(monkeypatch, build_release, "Invalid")

    with pytest.raises(RuntimeError, match="Invalid"):
        build_release.notarize_distribution(app, environ=NOTARY_ENV)

    assert calls[-1][:4] == ["xcrun", "notarytool", "log", "sub-1"]
    assert not any(command[:2] == ["xcrun", "stapler"] for command in calls)


def test_secret_scan_rejects_planted_credentials(tmp_path):
    """A payload carrying anything secret-shaped must never become a release."""
    build_release = load_build_release_module()
    staged = tmp_path / "FreightFate"
    staged.mkdir()
    (staged / "build_info.json").write_text(
        '{"deploy": "prod:scrupulous-ferret-428|' + "a" * 40 + '"}', encoding="utf-8"
    )
    with pytest.raises(RuntimeError, match="Convex deploy key"):
        build_release.verify_no_shipped_secrets(staged)

    (staged / "build_info.json").write_text('{"channel": "dev"}', encoding="utf-8")
    (staged / "notes.md").write_text("token ghp_" + "b" * 36, encoding="utf-8")
    with pytest.raises(RuntimeError, match="GitHub token"):
        build_release.verify_no_shipped_secrets(staged)

    (staged / "notes.md").unlink()
    (staged / ".env.local").write_text("VERCEL=1", encoding="utf-8")
    with pytest.raises(RuntimeError, match="secret-shaped file name"):
        build_release.verify_no_shipped_secrets(staged)


def test_secret_scan_passes_a_clean_payload_and_skips_binaries(tmp_path):
    """Real payload text is clean, and media/binaries are not scanned at all."""
    build_release = load_build_release_module()
    staged = tmp_path / "FreightFate"
    (staged / "freight_fate").mkdir(parents=True)
    (staged / "build_info.json").write_text('{"channel": "dev"}', encoding="utf-8")
    (staged / "CHANGELOG.md").write_text("- **Fixed** a thing.\n", encoding="utf-8")
    # A token-shaped byte run inside a pack must not trip the scan: packs are
    # opaque media, and false positives would train everyone to ignore it.
    (staged / "freight_fate" / "sounds.pak").write_bytes(b"FFPK1 ghp_" + b"c" * 36)
    build_release.verify_no_shipped_secrets(staged)


@pytest.mark.parametrize(
    ("label", "tag", "channel", "version"),
    [
        # The tag push passes the tag itself; v1.9.0 once stamped `vv1.9.0`
        # with pyproject's `1.9.0.dev0`, and the game offered itself forever.
        ("v1.9.1", "v1.9.1", "stable", "1.9.1"),
        ("1.9.1", "v1.9.1", "stable", "1.9.1"),
        ("1.9-tester-20261005", "1.9-tester-20261005", "dev", None),
    ],
)
def test_build_info_stamp_names_the_release(tmp_path, label, tag, channel, version):
    build_release = load_build_release_module()
    build_release.stamp_build_info(tmp_path, label, tmp_path)
    info = json.loads((tmp_path / "build_info.json").read_text(encoding="utf-8"))
    assert info["tag"] == tag
    assert info["channel"] == channel
    assert info["package_version"] == (version or build_release.project_version())


def test_stable_mac_bundle_reports_the_tag_version(tmp_path):
    build_release = load_build_release_module()
    app = tmp_path / "FreightFate.app"
    build_release.write_macos_info_plist(app, "v1.9.1")
    with (app / "Contents" / "Info.plist").open("rb") as stream:
        info = plistlib.load(stream)
    assert info["CFBundleShortVersionString"] == "1.9.1"
    assert info["CFBundleVersion"] == "1.9.1"


def test_channel3000_download_config_uses_public_defaults(monkeypatch):
    build_release = load_build_release_module()
    monkeypatch.delenv("FREIGHT_FATE_CHANNEL3000_URL", raising=False)
    monkeypatch.delenv("FREIGHT_FATE_CHANNEL3000_SHA256", raising=False)
    url, sha = build_release.channel3000_download_config()
    assert url == build_release.DEFAULT_CHANNEL3000_URL
    assert url.startswith("https://")
    assert sha == build_release.DEFAULT_CHANNEL3000_SHA256
    assert len(sha) == 64


def test_channel3000_download_config_rejects_a_non_hex_digest():
    build_release = load_build_release_module()
    with pytest.raises(
        RuntimeError,
        match="FREIGHT_FATE_CHANNEL3000_SHA256 must be a 64-character hexadecimal digest",
    ):
        build_release.channel3000_download_config(
            {"FREIGHT_FATE_CHANNEL3000_SHA256": "not-a-digest"}
        )


def test_ensure_channel3000_pack_installs_a_verified_download(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    pack = tmp_path / "channel3000.pak"
    payload = b"approved channel 3000 pack"
    monkeypatch.setenv("FREIGHT_FATE_CHANNEL3000_URL", "https://example.test/channel3000.pak")
    monkeypatch.setenv(
        "FREIGHT_FATE_CHANNEL3000_SHA256", build_release.hashlib.sha256(payload).hexdigest()
    )

    def download(request, destination):
        assert request.full_url == "https://example.test/channel3000.pak"
        Path(destination).write_bytes(payload)

    monkeypatch.setattr(build_release, "download_to_path", download)
    build_release.ensure_channel3000_pack(pack)

    assert pack.read_bytes() == payload
    assert list(tmp_path.glob("*.download")) == []


def test_ensure_channel3000_pack_names_itself_when_the_download_fails(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    pack = tmp_path / "channel3000.pak"
    monkeypatch.setenv("FREIGHT_FATE_CHANNEL3000_SHA256", "a" * 64)

    def fail_download(url, _destination):
        raise urllib.error.HTTPError(url, 404, "Not Found", None, None)

    monkeypatch.setattr(build_release, "download_to_path", fail_download)
    with pytest.raises(
        RuntimeError, match="Channel 3000 pack download failed with HTTP status 404"
    ):
        build_release.ensure_channel3000_pack(pack)
    assert not pack.exists()


def test_ensure_channel3000_pack_rejects_a_mismatched_download(tmp_path, monkeypatch):
    build_release = load_build_release_module()
    pack = tmp_path / "channel3000.pak"
    monkeypatch.setenv("FREIGHT_FATE_CHANNEL3000_SHA256", "a" * 64)
    monkeypatch.setattr(
        build_release,
        "download_to_path",
        lambda _url, destination: Path(destination).write_bytes(b"unapproved pack"),
    )
    with pytest.raises(RuntimeError, match="channel3000.pak failed SHA-256 verification"):
        build_release.ensure_channel3000_pack(pack)
    assert not pack.exists()
