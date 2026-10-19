from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
CI_WORKFLOW = ROOT / ".github" / "workflows" / "ci.yml"
# The packaging workflow, which is where an LFS fetch would do the damage.
# This was build.yml until the 1.9 cutover deleted it with the 1.8 line.
BUILD_WORKFLOW = ROOT / ".github" / "workflows" / "build-career-1.9.yml"
GITATTRIBUTES = ROOT / ".gitattributes"


def _load_ci_workflow() -> dict:
    return yaml.load(CI_WORKFLOW.read_text(encoding="utf-8"), Loader=yaml.BaseLoader)


def _checkout_step(job: dict) -> dict:
    return next(
        step for step in job["steps"] if step.get("uses", "").startswith("actions/checkout@")
    )


def test_nothing_depends_on_git_lfs_any_more() -> None:
    """The sound pack travels in git, so no job may wait on an LFS fetch.

    A build that shipped pointer files instead of the pack would produce a
    silent game, which is why the packaging jobs used to force `lfs: true`.
    The pack is an ordinary blob now (see .gitattributes), so a plain checkout
    already has it -- and an `lfs: pull` step left behind would be worse than
    redundant: it reintroduces a dependency on a budget that ran out, and a
    quota failure would look like a code failure.
    """
    assert "filter=lfs" not in GITATTRIBUTES.read_text(encoding="utf-8"), (
        "something is tracked by Git LFS again; the pack was moved out of it "
        "because an exhausted budget turned every run red at checkout"
    )
    for workflow in (CI_WORKFLOW, BUILD_WORKFLOW):
        assert "git lfs pull" not in workflow.read_text(encoding="utf-8"), workflow.name


def test_the_test_job_gets_the_sound_pack_from_a_plain_checkout() -> None:
    """The audio guards must not be able to pass against a pointer.

    This assertion has been through three shapes. It began as `lfs: true` on
    the test job, so the audio tests could not quietly pass against a pointer.
    A full fetch on both matrix runners on every push then spent about half a
    gigabyte of LFS bandwidth per commit, which exhausted the repository's
    budget and turned every run red at checkout, before a single test ran
    (2026-08-23); the job dropped to fetching sounds.pak alone. Once the
    budget was gone outright that fetch returned a pointer too, and the tests
    skipped themselves -- green, having checked nothing.

    So the pack is committed as an ordinary blob and the checkout is plain.
    The invariant that survived all three is the one asserted here: whatever
    the job does, it must end up holding a real pack.
    """
    test_job = _load_ci_workflow()["jobs"]["test"]
    checkout = _checkout_step(test_job)
    assert "lfs" not in (checkout.get("with") or {}), (
        "the pack is not an LFS object any more; an lfs flag here is a "
        "leftover that will read as though it were"
    )
    assert not any("lfs" in (step.get("run") or "") for step in test_job["steps"]), (
        "no LFS fetch step: a plain checkout already carries the pack"
    )


def test_ci_does_not_dispatch_the_retired_build_workflow() -> None:
    """build.yml went with the 1.8 line, so a CI job dispatching `Build`
    can only fail, and it failed every push to dev. Nightly recovery is
    retry-failed-nightly.yml's job, against the Career 1.9 snapshot."""
    workflow = _load_ci_workflow()

    assert "build" not in workflow["jobs"]
    assert "--workflow Build" not in CI_WORKFLOW.read_text(encoding="utf-8")


WORKFLOWS = ROOT / ".github" / "workflows"
RUST_WORKFLOW = WORKFLOWS / "rust.yml"
CHANGELOG_WORKFLOW = WORKFLOWS / "changelog.yml"


def _load(path: Path) -> dict:
    return yaml.load(path.read_text(encoding="utf-8"), Loader=yaml.BaseLoader)


def test_rust_ci_skips_pushes_and_prs_that_cannot_affect_it() -> None:
    """A docs-only merge to dev used to rebuild the Rust workspace on two
    Windows runners: only the PR trigger was path filtered. Push and PR now
    share one list."""
    triggers = _load(RUST_WORKFLOW)["on"]
    push_paths = triggers["push"]["paths"]
    assert triggers["pull_request"]["paths"] == push_paths
    for needed in (
        "crates/**",
        "Cargo.lock",
        "data/**",
        "assets/**",
        ".github/workflows/rust.yml",
        ".github/actions/**",
        "tools/check_test_targets.py",
    ):
        assert needed in push_paths, needed
    # Docs-only edits skip the Rust build.
    for skipped in ("docs/**", "README.md", "ROADMAP.md", "CHANGELOG.md"):
        assert skipped not in push_paths, skipped
    assert "workflow_dispatch" in triggers, "the manual full-suite run must stay"


def test_python_ci_skips_its_work_without_skipping_its_required_checks() -> None:
    """The two `test` matrix checks are required, and a required check that a
    trigger path filter skips never reports, which would block every
    Rust-only PR. So ci.yml triggers on everything, and a `changes` job
    gates the test steps instead."""
    workflow = _load(CI_WORKFLOW)
    for event in ("push", "pull_request"):
        trigger = workflow["on"][event]
        assert "paths" not in trigger and "paths-ignore" not in trigger, event

    changes = workflow["jobs"]["changes"]
    filter_step = next(
        step for step in changes["steps"] if step.get("uses", "").startswith("dorny/paths-filter@")
    )
    python_inputs = yaml.safe_load(filter_step["with"]["filters"])["python"]
    # `.github/**`: the guards read the composite actions as well as the
    # workflows.
    for needed in ("tools/**", "tests/**", "pyproject.toml", "uv.lock", ".github/**"):
        assert needed in python_inputs, needed

    test_job = workflow["jobs"]["test"]
    assert test_job["needs"] == "changes"
    gated = [step for step in test_job["steps"] if "skipping" not in step.get("name", "")]
    assert gated
    for step in gated:
        assert step.get("if") == "needs.changes.outputs.python != 'false'", step


def test_ci_cancels_superseded_runs() -> None:
    concurrency = _load(CI_WORKFLOW)["concurrency"]
    assert "github.ref" in concurrency["group"]
    assert concurrency["cancel-in-progress"] == "true"


def test_one_unfiltered_changelog_gate_covers_every_branch_the_builds_did() -> None:
    """The gate must see every change, so it cannot sit in a path-filtered
    workflow. It runs once, from changelog.yml, on every branch either build
    workflow used to gate."""
    for path in (CI_WORKFLOW, RUST_WORKFLOW):
        assert "changelog" not in _load(path)["jobs"], path.name

    workflow = _load(CHANGELOG_WORKFLOW)
    triggers = workflow["on"]
    for event in ("push", "pull_request"):
        assert "paths" not in triggers[event], event
        assert "paths-ignore" not in triggers[event], event
        covered = set(triggers[event]["branches"])
        for path in (CI_WORKFLOW, RUST_WORKFLOW):
            assert set(_load(path)["on"][event]["branches"]) <= covered, (event, path.name)

    run = "\n".join(step.get("run", "") for step in workflow["jobs"]["changelog"]["steps"])
    assert "tools/release_notes.py check" in run


ACTIONS = ROOT / ".github" / "actions"
REQUIRED_CHECKS = {"test (ubuntu-latest)", "test (windows-latest)", "changelog"}


def _all_workflows() -> list[Path]:
    return sorted(WORKFLOWS.glob("*.yml"))


def test_the_required_check_names_survive_the_runner_moves() -> None:
    """Branch protection on dev and main requires these contexts by name.
    The Python jobs keep the names the GitHub-hosted labels first gave them
    although they now run on pinned images; a renamed check would leave
    every PR waiting on one that never reports."""
    python = _load(CI_WORKFLOW)["jobs"]["test"]
    assert python["name"] == "test (${{ matrix.check }})"
    names = {f"test ({leg['check']})" for leg in python["strategy"]["matrix"]["include"]}
    gate = _load(CHANGELOG_WORKFLOW)["jobs"]["changelog"]["name"]
    assert names | {gate} == REQUIRED_CHECKS
    oses = {leg["os"] for leg in python["strategy"]["matrix"]["include"]}
    assert oses == {"windows-2025", "ubuntu-24.04"}


def test_bass_is_gone_from_ci() -> None:
    """Hearken's audio replaced BASS, so no workflow fetches, caches or
    checks it any more; a leftover step would fail on a missing tool."""
    for path in _all_workflows() + sorted(ACTIONS.glob("*/action.yml")):
        text = path.read_text(encoding="utf-8")
        assert "fetch_bass" not in text, path.name
        assert "bass-" not in text.lower(), path.name


def _runners(job: dict) -> list[str]:
    runs_on = job.get("runs-on", "")
    matrix = job.get("strategy", {}).get("matrix", {})
    legs = [leg for leg in matrix.get("include", []) if isinstance(leg, dict)]
    return [runs_on] + [str(v) for leg in legs for v in leg.values()]


def test_every_job_runs_on_github_hosted_runners() -> None:
    """Freight Fate is public, so GitHub's runners cost nothing; the owner
    keeps the paid Blacksmith minutes for the private repositories
    (2026-10-17)."""
    for path in _all_workflows():
        for name, job in _load(path)["jobs"].items():
            for runner in _runners(job):
                assert "blacksmith" not in runner, (path.name, name)
                assert "self-hosted" not in runner, (path.name, name)


def test_no_job_needs_a_secret_to_fetch_hearken() -> None:
    """Hearken is public, so Cargo fetches it anonymously and a fork's pull
    request builds like any other."""
    assert not (ACTIONS / "hearken-access").exists()
    for path in _all_workflows() + sorted(ACTIONS.glob("*/action.yml")):
        text = path.read_text(encoding="utf-8")
        assert "HEARKEN_TOKEN" not in text, path.name
        assert "hearken-access" not in text, path.name


def test_every_desktop_job_that_compiles_sets_up_through_rust_setup() -> None:
    """A job that runs cargo build, test or clippy for the desktop goes
    through rust-setup (dependencies, toolchain, caches) first. The iOS
    workflows install their own target and need none of it. `cargo fmt`
    reads only the workspace manifests and needs neither."""
    compiling = (
        "cargo build",
        "cargo test",
        "cargo clippy",
        "build-release.ps1",
        "build_release.py --rust",
    )
    for path in _all_workflows():
        if path.name.startswith("ios-"):
            continue
        for name, job in _load(path)["jobs"].items():
            steps = job.get("steps", [])
            if not any(any(c in s.get("run", "") for c in compiling) for s in steps):
                continue
            setup = [s for s in steps if s.get("uses") == "./.github/actions/rust-setup"]
            assert setup, (path.name, name)
            first_cargo = next(
                i for i, s in enumerate(steps) if any(c in s.get("run", "") for c in compiling)
            )
            assert steps.index(setup[0]) < first_cargo, (path.name, name)


def test_rust_ci_runs_its_jobs_side_by_side() -> None:
    """The same shape as Hearken's and Echo Parlor's CI: quick checks on a
    small runner, clippy once on Linux, the tests on Windows and Linux in
    parallel, nothing waiting on anything else."""
    jobs = _load(RUST_WORKFLOW)["jobs"]
    assert set(jobs) == {"checks", "clippy", "test", "test-macos"}
    assert all("needs" not in job for job in jobs.values())

    checks = jobs["checks"]
    assert checks["runs-on"] == "ubuntu-24.04"
    runs = "\n".join(s.get("run", "") for s in checks["steps"])
    assert "cargo fmt --all --check" in runs
    assert "tools/l10n/extract.py --check" in runs
    assert "tools/check_test_targets.py" in runs
    # No build here, so no Hearken and no data/ download.
    assert not any("rust-setup" in s.get("uses", "") for s in checks["steps"])
    assert "sparse-checkout" in _checkout_step(checks)["with"]

    clippy = jobs["clippy"]
    assert clippy["runs-on"] == "ubuntu-24.04"
    assert any(
        s.get("run") == "cargo clippy --workspace --all-targets --locked -- -D warnings"
        for s in clippy["steps"]
    )

    test = jobs["test"]
    legs = {leg["name"]: leg for leg in test["strategy"]["matrix"]["include"]}
    assert legs["Windows"]["os"] == "windows-2025"
    assert legs["Linux"]["os"] == "ubuntu-24.04"
    # ff-core is headless, so it is proved once, on Linux.
    assert legs["Windows"]["packages"] == "-p freight-fate"
    assert legs["Linux"]["packages"] == "-p ff-core -p freight-fate"
    # The quick set on pushes and PRs; the sweeps on a manual run.
    assert "--cfg ci_quick" in test["env"]["RUSTFLAGS"]
    assert "workflow_dispatch" in _load(RUST_WORKFLOW)["on"]
    runs = [s.get("run", "") for s in test["steps"]]
    assert "cargo test ${{ matrix.packages }} --no-run --locked --timings" in runs
    assert "cargo test ${{ matrix.packages }} --locked" in runs
    assert "cargo test -p freight-fate --test agent_server --locked" in runs
    assert not any("lfs" in (s.get("with") or {}) for s in test["steps"])


def test_the_windows_test_leg_audits_the_game_s_imports() -> None:
    """The snapshot's payload DLL check also runs in PR CI, on the executable
    the Windows test build already compiled, so an import a clean Windows
    lacks fails the PR that adds it."""
    test = _load(RUST_WORKFLOW)["jobs"]["test"]
    steps = test["steps"]
    runs = [s.get("run", "") for s in steps]
    audit = next(s for s in steps if "tools/check_windows_imports.py" in s.get("run", ""))
    assert audit["if"] == "matrix.name == 'Windows'"
    # After the build that leaves target/debug/freightfate.exe.
    build = runs.index("cargo test ${{ matrix.packages }} --no-run --locked --timings")
    assert steps.index(audit) > build
    paths = _load(RUST_WORKFLOW)["on"]["push"]["paths"]
    for needed in (
        "tools/check_windows_imports.py",
        "tools/windows_runtime.py",
        "tools/build_release.py",
    ):
        assert needed in paths, needed


def test_macos_tests_run_on_a_manual_run_or_a_release_tag_only() -> None:
    """The Mac link (Prism's frameworks, the clang runtime) and the
    CoreAudio build are proved before a release, as Hearken's CI does, but
    the dearest runner is not spent on every push or PR."""
    workflow = _load(RUST_WORKFLOW)
    assert "v*" in workflow["on"]["push"]["tags"]
    mac = workflow["jobs"]["test-macos"]
    assert mac["if"] == (
        "github.event_name == 'workflow_dispatch' || startsWith(github.ref, 'refs/tags/v')"
    )
    assert mac["runs-on"].startswith("macos-")
    assert mac["env"]["SDL_AUDIODRIVER"] == "dummy"
    runs = [s.get("run", "") for s in mac["steps"]]
    assert "cargo test -p ff-core -p freight-fate --locked" in runs
    assert "cargo test -p freight-fate --test agent_server --locked" in runs


def test_linux_builds_get_the_alsa_headers_hearken_plays_through() -> None:
    """Hearken plays through Kira and cpal, which link ALSA on Linux."""
    action = (ACTIONS / "linux-deps" / "action.yml").read_text(encoding="utf-8")
    assert "libasound2-dev" in action


def test_the_test_target_guard_passes() -> None:
    """One `it` binary per crate plus the listed exceptions, and every file
    under tests/it compiled in."""
    import subprocess
    import sys

    result = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "check_test_targets.py")],
        capture_output=True,
        text=True,
    )
    assert result.returncode == 0, result.stdout + result.stderr


def _run_stale_copy_guard(tmp_path: Path, event: str, dry_run: str, differs: bool) -> int:
    """Run Prepare's stale-copy step with a stand-in git."""
    import os
    import subprocess

    prepare = _load(BUILD_WORKFLOW)["jobs"]["prepare"]
    step = next(
        s
        for s in prepare["steps"]
        if s.get("name") == "Refuse to run a stale copy of this workflow"
    )
    fake = tmp_path / "git"
    fake.write_text(
        '#!/bin/sh\nif [ "$1" = diff ]; then exit ' + ("1" if differs else "0") + "; fi\nexit 0\n"
    )
    fake.chmod(0o755)
    env = {
        **os.environ,
        "PATH": f"{tmp_path}{os.pathsep}{os.environ['PATH']}",
        "GITHUB_WORKFLOW_SHA": "0" * 40,
        "GITHUB_EVENT_NAME": event,
        "GITHUB_REF_NAME": "main",
        "CAREER_BRANCH": "dev",
        "DRY_RUN": dry_run,
    }
    assert step["env"]["DRY_RUN"] == "${{ inputs.dry_run }}"
    return subprocess.run(["bash", "-e", "-c", step["run"]], env=env).returncode


def test_a_stale_snapshot_workflow_stops_any_run_that_would_publish(tmp_path: Path) -> None:
    """2026-10-19: a dispatch from main built for 25 minutes, then Release
    refused the assets main's stale copy expected. A publishing dispatch now
    stops in Prepare like a scheduled run; a dry run still only warns."""
    assert _run_stale_copy_guard(tmp_path, "schedule", "", differs=True) != 0
    assert _run_stale_copy_guard(tmp_path, "workflow_dispatch", "false", differs=True) != 0
    assert _run_stale_copy_guard(tmp_path, "workflow_dispatch", "true", differs=True) == 0
    assert _run_stale_copy_guard(tmp_path, "push", "", differs=True) == 0
    assert _run_stale_copy_guard(tmp_path, "workflow_dispatch", "false", differs=False) == 0
