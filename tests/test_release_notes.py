"""Curated release-note generation for build snapshots."""

import importlib.util
import subprocess
import sys
from pathlib import Path

import yaml


def load_release_notes_module():
    path = Path(__file__).resolve().parents[1] / "tools" / "release_notes.py"
    spec = importlib.util.spec_from_file_location("release_notes", path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[spec.name] = module
    spec.loader.exec_module(module)
    return module


def git(repo: Path, *args: str) -> str:
    return subprocess.check_output(
        ["git", *args],
        cwd=repo,
        text=True,
        encoding="utf-8",
    ).strip()


def commit(repo: Path, message: str) -> None:
    git(repo, "add", ".")
    git(repo, "commit", "-m", message)


def make_repo(tmp_path: Path, changelog: str) -> Path:
    repo = tmp_path / "repo"
    repo.mkdir()
    git(repo, "init", "-q")
    git(repo, "config", "user.email", "tests@example.test")
    git(repo, "config", "user.name", "Tests")
    (repo / "CHANGELOG.md").write_text(changelog, encoding="utf-8")
    commit(repo, "chore: seed changelog")
    return repo


def changelog(unreleased: str, stable: str = "") -> str:
    return f"# Changelog\n\n## Unreleased\n\n{unreleased}\n\n{stable}".rstrip() + "\n"


def version_only_changelog(version_block: str, stable: str = "") -> str:
    return f"# Changelog\n\n{version_block}\n\n{stable}".rstrip() + "\n"


def test_nightly_notes_use_curated_unreleased_entries(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    repo = make_repo(
        tmp_path,
        changelog("### Added\n- **Dispatch.** New spoken board details.\n"),
    )
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.nightly_notes()

    assert "A preview of the next release" in notes
    assert "## Changes since the previous snapshot" in notes
    assert "## Added" in notes
    assert "- **Dispatch.** New spoken board details." in notes
    assert "chore: seed changelog" not in notes


def test_first_career_snapshot_uses_accurate_release_notes_heading(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    repo = make_repo(
        tmp_path,
        changelog("### Added\n- **Career.** A new driver can begin a career.\n"),
    )
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.nightly_notes(first_snapshot=True)

    assert "## Changes in this snapshot" in notes
    assert "Changes since the previous snapshot" not in notes


def test_first_career_snapshot_notes_fit_github_without_cutting_entries(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    added_entries = "\n".join(
        f"- **Career improvement {index}.** " + ("Player-facing detail. " * 90)
        for index in range(100)
    )
    fixed_entries = "\n".join(
        f"- **Career fix {index}.** " + ("Clear fix detail. " * 90) for index in range(100)
    )
    repo = make_repo(
        tmp_path,
        changelog(f"### Added\n{added_entries}\n\n### Fixed\n{fixed_entries}\n"),
    )
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.nightly_notes(first_snapshot=True)

    assert len(notes) <= release_notes.GITHUB_RELEASE_NOTES_SAFE_CHARACTERS
    assert "### Added" in notes
    assert "### Fixed" in notes
    assert "\n## Added\n" not in notes
    assert "\n## Fixed\n" not in notes
    assert "**Career improvement 0.**" in notes
    assert "**Career fix 0.**" in notes
    assert "## Complete change list" in notes
    assert "CHANGELOG.md" in notes
    emitted = {
        release_notes.format_entry(entry)
        for section in release_notes.parse_sections(notes, min_heading_level=3)
        for entry in section.entries
    }
    source = {
        release_notes.format_entry(entry)
        for entry in [*added_entries.splitlines(), *fixed_entries.splitlines()]
    }
    assert emitted
    assert emitted <= source


def test_later_snapshot_notes_are_bounded_too(tmp_path, monkeypatch):
    # A rewrite of the curated entries makes every bullet read as new to the
    # previous tag; the page must still fit, and say where the rest is.
    release_notes = load_release_notes_module()
    repo = make_repo(tmp_path, changelog("### Added\n- **Old wording.** Long ago.\n"))
    git(repo, "tag", "nightly-20260615")
    added_entries = "\n".join(
        f"- **Career improvement {index}.** " + ("Player-facing detail. " * 90)
        for index in range(100)
    )
    fixed_entries = "\n".join(
        f"- **Career fix {index}.** " + ("Clear fix detail. " * 90) for index in range(100)
    )
    (repo / "CHANGELOG.md").write_text(
        changelog(f"### Added\n{added_entries}\n\n### Fixed\n{fixed_entries}\n"),
        encoding="utf-8",
    )
    commit(repo, "docs: rewrite every entry")
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.nightly_notes(previous_tag="nightly-20260615")

    assert len(notes) <= release_notes.GITHUB_RELEASE_NOTES_SAFE_CHARACTERS
    assert "## Changes since the previous snapshot" in notes
    assert "\n## Added\n" in notes
    assert "\n## Fixed\n" in notes
    assert "**Career improvement 0.**" in notes
    assert "**Career fix 0.**" in notes
    assert "## Complete change list" in notes
    assert "more changes than fit on this page" in notes
    assert "**Old wording.**" not in notes


def test_republished_entries_are_not_new_but_listed_leads_still_are(tmp_path, monkeypatch):
    # A rewrite reworded every bullet after the previous snapshot; the
    # republished file names that commit, and the one bullet that was added
    # between the tag and the rewrite, so the snapshot lists only that one
    # plus anything added after the rewrite.
    release_notes = load_release_notes_module()
    repo = make_repo(
        tmp_path,
        changelog("### Added\n- **Old wording.** A long story about the feature.\n"),
    )
    git(repo, "tag", "nightly-20260615")
    (repo / "CHANGELOG.md").write_text(
        changelog(
            "### Added\n- **Old wording.** A long story about the feature.\n"
            "- **Pausing no longer takes you off duty.** Long story.\n"
        ),
        encoding="utf-8",
    )
    commit(repo, "fix: pause stays on duty")
    (repo / "CHANGELOG.md").write_text(
        changelog(
            "### Added\n- **New wording.** Short.\n"
            "- **Pausing no longer takes you off duty.** Short.\n"
        ),
        encoding="utf-8",
    )
    commit(repo, "docs(changelog): rewrite")
    rewrite = git(repo, "rev-parse", "HEAD")
    (repo / "tools").mkdir()
    (repo / "tools" / "release_notes_republished.txt").write_text(
        f"# rewrite\nref = {rewrite}\nnew = Pausing no longer takes you off duty.\n",
        encoding="utf-8",
    )
    (repo / "CHANGELOG.md").write_text(
        changelog(
            "### Added\n- **Tonight's change.** Short.\n- **New wording.** Short.\n"
            "- **Pausing no longer takes you off duty.** Short.\n"
        ),
        encoding="utf-8",
    )
    commit(repo, "feat: tonight")
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.nightly_notes(previous_tag="nightly-20260615")

    assert "**Tonight's change.**" in notes
    assert "**Pausing no longer takes you off duty.**" in notes
    assert "**New wording.**" not in notes


def test_release_notes_size_check_rejects_oversized_input(tmp_path, capsys):
    release_notes = load_release_notes_module()
    notes = tmp_path / "notes.md"
    notes.write_text("x" * 101, encoding="utf-8")

    result = release_notes.main(["check-size", "--input", str(notes), "--max-characters", "100"])

    captured = capsys.readouterr()
    assert result == 1
    assert "101 characters" in captured.err
    assert "100-character publication limit" in captured.err


def test_release_notes_size_check_counts_crlf_characters(tmp_path, capsys):
    release_notes = load_release_notes_module()
    notes = tmp_path / "notes.md"
    notes.write_bytes(b"x\r\n" * 40)

    result = release_notes.main(["check-size", "--input", str(notes), "--max-characters", "100"])

    assert result == 1
    assert "120 characters" in capsys.readouterr().err


def test_first_snapshot_budget_reserves_the_written_newline():
    release_notes = load_release_notes_module()

    assert release_notes.first_snapshot_fits("x" * 119_999)
    assert not release_notes.first_snapshot_fits("x" * 120_000)


def test_stable_notes_extract_matching_version_block(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    repo = make_repo(
        tmp_path,
        changelog(
            "### Added\n- Next thing.\n",
            "## 1.6.0 - 2026-06-15\n\n### Fixed\n- Stable fix.\n",
        ),
    )
    monkeypatch.setattr(release_notes, "ROOT", repo)

    assert release_notes.stable_notes("v1.6.0") == "## Fixed\n- Stable fix."


def test_stable_notes_fall_back_to_unreleased_when_version_missing(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    repo = make_repo(tmp_path, changelog("### Changed\n- Upcoming change.\n"))
    monkeypatch.setattr(release_notes, "ROOT", repo)

    assert release_notes.stable_notes("9.9.9") == "## Changed\n- Upcoming change."


def test_stable_notes_are_bounded_and_point_at_the_changelog(tmp_path, monkeypatch):
    # 1.9's Unreleased block is past GitHub's limit on its own; the v1.9.0
    # tag build checks the size after every platform has built.
    release_notes = load_release_notes_module()
    added_entries = "\n".join(
        f"- **Career improvement {index}.** " + ("Player-facing detail. " * 90)
        for index in range(100)
    )
    fixed_entries = "\n".join(
        f"- **Career fix {index}.** " + ("Clear fix detail. " * 90) for index in range(100)
    )
    repo = make_repo(
        tmp_path, changelog(f"### Added\n{added_entries}\n\n### Fixed\n{fixed_entries}\n")
    )
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.stable_notes("1.9.0")

    assert len(notes) + 1 <= release_notes.GITHUB_RELEASE_NOTES_SAFE_CHARACTERS
    assert notes.startswith("## Added\n- **Career improvement 0.**")
    assert "\n## Fixed\n- **Career fix 0.**" in notes
    assert notes.endswith(release_notes.STABLE_COMPLETE_LIST)


def test_stable_notes_lead_with_compatibility(tmp_path, monkeypatch):
    # Players upgrading in place need to hear first whether their career
    # comes across; snapshot notes keep the usual order.
    release_notes = load_release_notes_module()
    repo = make_repo(
        tmp_path,
        changelog("### Added\n- New thing.\n\n### Compatibility\n- Old careers stay behind.\n"),
    )
    monkeypatch.setattr(release_notes, "ROOT", repo)

    assert release_notes.stable_notes("1.9.0") == (
        "## Compatibility\n- Old careers stay behind.\n\n## Added\n- New thing."
    )


def test_stable_notes_publish_the_curated_summary_and_link_the_full_list(tmp_path, monkeypatch):
    # 1.9.0 opens with a hand-written summary; the per-snapshot detail sits
    # in its own block below and must not leak into the release page.
    release_notes = load_release_notes_module()
    repo = make_repo(
        tmp_path,
        changelog(
            "",
            "## 1.9.0 - 2026-10-04\n\n"
            "### Changes\n- A change.\n\n"
            "### Highlights\n- The big one.\n\n"
            "### Fixes\n- A fix.\n\n"
            "### Compatibility\n- Old careers stay behind.\n\n"
            "### New features\n- A feature.\n\n"
            "## 1.9.0 complete change list\n\n### Added\n- Snapshot detail.\n",
        ),
    )
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.stable_notes("v1.9.0")

    assert notes == (
        "## Compatibility\n- Old careers stay behind.\n\n"
        "## Highlights\n- The big one.\n\n"
        "## New features\n- A feature.\n\n"
        "## Fixes\n- A fix.\n\n"
        "## Changes\n- A change.\n\n" + release_notes.STABLE_FULL_CHANGELOG.format(version="1.9.0")
    )
    assert "blob/v1.9.0/CHANGELOG.md" in notes


def test_curated_summary_never_feeds_nightly_notes(tmp_path):
    release_notes = load_release_notes_module()
    text = changelog(
        "",
        "## 1.9.0 - 2026-10-04\n\n### Highlights\n- The big one.\n\n"
        "## 1.9.0 complete change list\n\n### Added\n- Snapshot detail.\n",
    )

    sections = release_notes.nightly_candidate_sections(text, set())

    assert sections == [release_notes.ChangelogSection("Added", ("- Snapshot detail.",))]
    assert release_notes.nightly_candidate_sections(text, {"1.9.0"}) == []


def test_nightly_notes_exclude_entries_from_previous_nightly(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    repo = make_repo(tmp_path, changelog("### Added\n- Old curated note.\n"))
    git(repo, "tag", "nightly-20260615")
    (repo / "CHANGELOG.md").write_text(
        changelog("### Added\n- Old curated note.\n- New curated note.\n"),
        encoding="utf-8",
    )
    commit(repo, "feat: add new work")
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.nightly_notes(previous_tag="nightly-20260615")

    assert "- New curated note." in notes
    assert "- Old curated note." not in notes


def test_nightly_notes_exclude_previous_release_body_sections(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    repo = make_repo(tmp_path, changelog("### Fixed\n- **Updater.** Packaged updates work.\n"))
    git(repo, "tag", "nightly-20260615")
    (repo / "CHANGELOG.md").write_text(
        changelog(
            "### Fixed\n"
            "- **Updater.** Packaged updates work.\n"
            "- **Help.** Upgrade help explains what to buy.\n"
        ),
        encoding="utf-8",
    )
    commit(repo, "docs: improve help")
    previous_notes = repo / "previous-notes.md"
    previous_notes.write_text(
        "Preview snapshot.\n\n"
        "## Changes since the previous snapshot\n\n"
        "## Fixed\n"
        "- **Updater.** Packaged updates work.\n",
        encoding="utf-8",
    )
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.nightly_notes(
        previous_tag="nightly-20260615",
        exclude_notes=str(previous_notes),
    )

    assert "- **Help.** Upgrade help explains what to buy." in notes
    assert "- **Updater.** Packaged updates work." not in notes


def test_nightly_notes_exclude_stable_release_body_sections(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    repo = make_repo(tmp_path, changelog("### Fixed\n- **Updater.** Packaged updates work.\n"))
    git(repo, "tag", "nightly-20260615")
    (repo / "CHANGELOG.md").write_text(
        changelog(
            "### Added\n"
            "- **Radio.** New radio chatter.\n"
            "### Fixed\n"
            "- **Updater.** Packaged updates work.\n"
            "- **Help.** Upgrade help explains what to buy.\n"
        ),
        encoding="utf-8",
    )
    commit(repo, "docs: improve help")
    stable_notes = repo / "stable-notes.md"
    stable_notes.write_text(
        "## Added\n"
        "- **Radio.** New radio chatter.\n\n"
        "## Fixed\n"
        "- **Updater.** Packaged updates work.\n",
        encoding="utf-8",
    )
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.nightly_notes(
        previous_tag="nightly-20260615",
        exclude_stable_notes=str(stable_notes),
    )

    assert "- **Help.** Upgrade help explains what to buy." in notes
    assert "- **Radio.** New radio chatter." not in notes
    assert "- **Updater.** Packaged updates work." not in notes


def test_nightly_notes_use_new_version_block_entries(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    repo = make_repo(
        tmp_path,
        version_only_changelog("## 1.6.0 - 2026-06-15\n\n### Added\n- Old player-facing note.\n"),
    )
    git(repo, "tag", "nightly-20260615")
    (repo / "CHANGELOG.md").write_text(
        version_only_changelog(
            "## 1.6.0 - 2026-06-15\n\n"
            "### Added\n"
            "- Old player-facing note.\n"
            "- New player-facing note.\n"
        ),
        encoding="utf-8",
    )
    commit(repo, "feat: add player-facing work")
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.nightly_notes(previous_tag="nightly-20260615")

    assert "- New player-facing note." in notes
    assert "- Old player-facing note." not in notes


def test_nightly_notes_skip_already_released_version_block(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    base = changelog(
        "### Added\n- Pre-release staged note.\n",
        "## 1.6.0 - 2026-06-15\n\n### Added\n- Shipped feature.\n",
    )
    repo = make_repo(tmp_path, base)
    git(repo, "tag", "nightly-20260615")
    git(repo, "tag", "v1.6.0")
    (repo / "CHANGELOG.md").write_text(
        changelog(
            "### Added\n- Pre-release staged note.\n- **Achievements.** New badges.\n",
            "## 1.6.0 - 2026-06-15\n\n### Added\n- Shipped feature.\n",
        ),
        encoding="utf-8",
    )
    commit(repo, "feat: achievements")
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.nightly_notes(previous_tag="nightly-20260615")

    # New Unreleased work surfaces even though a released version block shares
    # the same "Added" subsection title.
    assert "- **Achievements.** New badges." in notes
    # The shipped 1.6.0 block has a stable tag, so it is not re-advertised.
    assert "- Shipped feature." not in notes
    # Already carried in the previous snapshot.
    assert "- Pre-release staged note." not in notes


def test_format_sections_merges_duplicate_titles(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    section = release_notes.ChangelogSection
    out = release_notes.format_sections(
        [
            section("Added", ("- Unreleased badge.",)),
            section("Added", ("- Staged feature.",)),
        ]
    )

    assert out.count("## Added") == 1
    assert "- Unreleased badge." in out
    assert "- Staged feature." in out


def test_should_build_nightly_ignores_internal_version_block_entries(tmp_path, monkeypatch, capsys):
    release_notes = load_release_notes_module()
    repo = make_repo(
        tmp_path,
        version_only_changelog(
            "## 1.6.0 - 2026-06-15\n\n### Internal\n- Old build script cleanup.\n"
        ),
    )
    git(repo, "tag", "nightly-20260615")
    (repo / "CHANGELOG.md").write_text(
        version_only_changelog(
            "## 1.6.0 - 2026-06-15\n\n"
            "### Internal\n"
            "- Old build script cleanup.\n"
            "- New test-only helper.\n"
        ),
        encoding="utf-8",
    )
    commit(repo, "test: add helper")
    monkeypatch.setattr(release_notes, "ROOT", repo)
    args = type(
        "Args",
        (),
        {
            "previous_tag": "nightly-20260615",
            "exclude_notes": "",
            "latest_stable_tag": "",
            "exclude_stable_notes": "",
            "head": "HEAD",
        },
    )()

    assert release_notes.should_build_nightly_command(args) == 0

    assert "should_build=false" in capsys.readouterr().out


def test_nightly_notes_no_entry_behavior_is_explicit(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    repo = make_repo(tmp_path, changelog(""))
    git(repo, "tag", "nightly-20260615")
    (repo / "README.md").write_text("Docs only\n", encoding="utf-8")
    commit(repo, "docs: update readme")
    monkeypatch.setattr(release_notes, "ROOT", repo)

    notes = release_notes.nightly_notes(previous_tag="nightly-20260615")

    assert notes.endswith("- No user-facing changes")


def test_should_build_nightly_uses_curated_entries(tmp_path, monkeypatch, capsys):
    release_notes = load_release_notes_module()
    repo = make_repo(tmp_path, changelog("### Added\n- Old curated note.\n"))
    git(repo, "tag", "nightly-20260615")
    (repo / "CHANGELOG.md").write_text(
        changelog("### Added\n- Old curated note.\n- New curated note.\n"),
        encoding="utf-8",
    )
    commit(repo, "feat: add new work")
    monkeypatch.setattr(release_notes, "ROOT", repo)
    args = type(
        "Args",
        (),
        {
            "previous_tag": "nightly-20260615",
            "exclude_notes": "",
            "latest_stable_tag": "",
            "exclude_stable_notes": "",
            "head": "HEAD",
        },
    )()

    assert release_notes.should_build_nightly_command(args) == 0

    assert "should_build=true" in capsys.readouterr().out


def test_should_build_nightly_skips_without_entries_or_marker(tmp_path, monkeypatch, capsys):
    release_notes = load_release_notes_module()
    repo = make_repo(tmp_path, changelog("### Added\n- Old curated note.\n"))
    git(repo, "tag", "nightly-20260615")
    (repo / "README.md").write_text("Docs only\n", encoding="utf-8")
    commit(repo, "docs: update readme")
    monkeypatch.setattr(release_notes, "ROOT", repo)
    args = type(
        "Args",
        (),
        {
            "previous_tag": "nightly-20260615",
            "exclude_notes": "",
            "latest_stable_tag": "",
            "exclude_stable_notes": "",
            "head": "HEAD",
        },
    )()

    assert release_notes.should_build_nightly_command(args) == 0

    assert "should_build=false" in capsys.readouterr().out


def test_should_build_nightly_allows_explicit_marker(tmp_path, monkeypatch, capsys):
    release_notes = load_release_notes_module()
    repo = make_repo(tmp_path, changelog("### Added\n- Old curated note.\n"))
    git(repo, "tag", "nightly-20260615")
    (repo / "README.md").write_text("Nightly refresh\n", encoding="utf-8")
    commit(repo, "chore: refresh snapshot\n\nnightly: build")
    monkeypatch.setattr(release_notes, "ROOT", repo)
    args = type(
        "Args",
        (),
        {
            "previous_tag": "nightly-20260615",
            "exclude_notes": "",
            "latest_stable_tag": "",
            "exclude_stable_notes": "",
            "head": "HEAD",
        },
    )()

    assert release_notes.should_build_nightly_command(args) == 0

    assert "should_build=true" in capsys.readouterr().out


def test_generated_notes_flatten_to_speakable_lines(tmp_path, monkeypatch):
    release_notes = load_release_notes_module()
    repo = make_repo(
        tmp_path,
        changelog(
            "### Added\n"
            "- **Cruise control.** See [manual](https://example.test)\n"
            "  before setting speed.\n"
        ),
    )
    monkeypatch.setattr(release_notes, "ROOT", repo)

    spoken = release_notes.flatten_markdown(release_notes.nightly_notes())

    assert "Added" in spoken
    assert "Cruise control. See manual before setting speed." in spoken
    assert all("**" not in line and "https://" not in line for line in spoken)


def test_check_accepts_single_push_release_sync(tmp_path, monkeypatch, capsys):
    """A release-sync push lands new bullets already under a tagged version
    heading (the v1.8.3 release did the merge and the heading in one push);
    the changelog gate must credit those bullets to the push, not dismiss
    the block as already released."""
    release_notes = load_release_notes_module()
    stable_history = "## 1.8.1 - 2026-07-13\n\n### Fixed\n- Old stable fix.\n"
    repo = make_repo(tmp_path, changelog("", stable_history))
    git(repo, "tag", "v1.8.1")
    base = git(repo, "rev-parse", "HEAD")

    (repo / "data").mkdir()
    (repo / "data" / "cities.json").write_text("{}\n", encoding="utf-8")
    (repo / "CHANGELOG.md").write_text(
        changelog(
            "",
            "## 1.8.3 - 2026-07-14\n\n### Fixed\n- Restoring a cloud backup works again.\n\n"
            + stable_history,
        ),
        encoding="utf-8",
    )
    commit(repo, "release: merge dev into main for 1.8.3")
    git(repo, "tag", "v1.8.3")
    monkeypatch.setattr(release_notes, "ROOT", repo)

    args = release_notes.argparse.Namespace(base=base, head="HEAD")
    assert release_notes.check_command(args) == 0

    # The same push without new bullets must still fail the gate.
    git(repo, "tag", "-d", "v1.8.3")
    (repo / "CHANGELOG.md").write_text(
        changelog("", "## 1.8.3 - 2026-07-14\n\n" + stable_history), encoding="utf-8"
    )
    commit(repo, "release: heading only")
    capsys.readouterr()
    assert release_notes.check_command(args) == 1


def test_one_workflow_builds_the_game_and_it_is_the_rust_one():
    """`build.yml` is gone and must not come back.

    It built the Python game (Nuitka) and scheduled itself against `dev`.
    At the 1.9 cutover (2026-09-20) dev became the Rust line, and the owner
    ruled the same day that 1.8 gets no further releases of any kind, so the
    workflow had no job left: its nightly could not succeed and its tag
    trigger would have built the wrong thing for a `v1.9.0`.

    Restoring it would put two workflows on one cron again, or hand a
    stable tag to the Nuitka path. If a stable release path is needed, it
    belongs in `build-career-1.9.yml` alongside the Rust toolchain and BASS
    steps that only that workflow has.
    """
    workflows = Path(__file__).resolve().parents[1] / ".github" / "workflows"

    assert not (workflows / "build.yml").exists()

    snapshot = (workflows / "build-career-1.9.yml").read_text(encoding="utf-8")
    assert 'cron: "37 2 * * *"' in snapshot
    assert "tools/release_notes.py should-build-nightly" in snapshot
    assert "--exclude-notes previous-notes.md" in snapshot
    assert "--exclude-stable-notes latest-stable-notes.md" in snapshot
    assert "tools/release_notes.py nightly" in snapshot

    # The retry watches the one surviving workflow, and dispatches against
    # the branch it actually builds -- it named feat/career-1.9 until the
    # cutover moved the nightly to dev.
    retry = (workflows / "retry-failed-nightly.yml").read_text(encoding="utf-8")
    assert "workflows: [Career 1.9 snapshot]" in retry
    assert "feat/career-1.9" not in retry
    assert 'SNAPSHOT_BRANCH: "dev"' in retry


def test_career_19_snapshot_workflow_contract():
    workflow = (
        Path(__file__).resolve().parents[1] / ".github" / "workflows" / "build-career-1.9.yml"
    ).read_text(encoding="utf-8")

    # `dev` since the 1.9 cutover (2026-09-20): dev IS the 1.9 line now, and
    # this is the only workflow that builds the Rust game, so it is the
    # nightly. build.yml's schedule was retired in the same change because it
    # builds the Python game, which dev no longer has.
    assert "inputs.branch || 'dev'" in workflow
    # Tag pushes build the tagged ref, not tip-of-dev by accident.
    assert "github.event_name == 'push' && github.ref" in workflow
    assert "group: career-19-snapshot\n" in workflow
    assert "career-19-snapshot-${{ github.ref }}" not in workflow
    assert 'cron: "37 2 * * *"' in workflow
    assert "tag=1.9-tester-$(date -u +%Y%m%d)" in workflow
    assert "commit_sha: ${{ steps.check.outputs.commit_sha }}" in workflow
    assert 'echo "commit_sha=$(git rev-parse HEAD)"' in workflow
    # Format, Windows, macOS, Linux build, the Linux distro smoke, and the
    # release.
    assert workflow.count("ref: ${{ needs.prepare.outputs.commit_sha }}") == 6
    assert 'git tag --list "1.9-tester-*"' in workflow
    assert "tools/release_notes.py should-build-nightly" in workflow
    assert "tools/release_notes.py nightly" in workflow
    assert "tools/release_notes.py nightly --first-snapshot" in workflow
    assert "tools/release_notes.py check-size --input notes.md --max-characters 120000" in workflow
    assert "./build-release.ps1" in workflow
    assert "windows-portable.zip" in workflow
    assert "macos-arm64.zip" in workflow
    # Tester path still cuts a prerelease; the stable tag path does not.
    assert "--prerelease" in workflow
    assert "Create stable release" in workflow
    assert "tools/release_notes.py stable --version" in workflow
    assert 'tags:\n      - "v*.*.*"' in workflow or '- "v*.*.*"' in workflow
    assert "is_prerelease: ${{ steps.check.outputs.is_prerelease }}" in workflow
    assert "COMMIT_SHA: ${{ needs.prepare.outputs.commit_sha }}" in workflow
    assert '--target "$COMMIT_SHA"' in workflow
    assert '--target "$CAREER_BRANCH"' not in workflow
    assert "needs.build_windows.result == 'success'" in workflow
    assert "needs.build_macos.result == 'success'" in workflow
    assert "needs.build_linux.result == 'success'" in workflow
    assert "needs.smoke_linux.result == 'success'" in workflow


def test_career_19_snapshot_builds_and_boots_a_linux_release():
    """The Linux tarball and AppImage ship only after both boot untouched on
    the popular distributions, from a build whose glibc floor is 22.04's."""
    workflow = (
        Path(__file__).resolve().parents[1] / ".github" / "workflows" / "build-career-1.9.yml"
    ).read_text(encoding="utf-8")
    assert "tools/build_release.py --rust --smoke" in workflow
    assert "tools/build_appimage.py --rust" in workflow
    assert "bash /work/tools/linux_smoke.sh" in workflow
    # One build job, two native legs: the PC one on 22.04 and the ARM64 one
    # (the Blazie BT Speak and BT Braille, Raspberry Pi) on GitHub's
    # hosted ARM runner of the same release, so both carry the same glibc
    # floor. Each leg uploads its own pair of downloads.
    assert "            runner: ubuntu-22.04\n" in workflow
    assert "            runner: ubuntu-22.04-arm\n" in workflow
    assert "            tarball: linux-x64\n" in workflow
    assert "            tarball: linux-arm64\n" in workflow
    assert "name: Linux-${{ matrix.arch }}" in workflow
    assert "dist/FreightFate-*-${{ matrix.tarball }}.tar.gz" in workflow
    assert "dist/FreightFate-*-linux-${{ matrix.arch }}.AppImage" in workflow
    # The smoke matrix runs on both architectures, minus Arch on ARM64
    # (Docker Hub's archlinux image is amd64 only).
    assert "arch: [x86_64, aarch64]" in workflow
    assert (
        "runs-on: ${{ matrix.arch == 'aarch64' && 'ubuntu-24.04-arm' || 'ubuntu-latest' }}"
        in workflow
    )
    assert (
        "        exclude:\n          - arch: aarch64\n            image: archlinux:latest\n"
        in workflow
    )
    for image in (
        "ubuntu:22.04",
        "ubuntu:24.04",
        "debian:12",
        "debian:13",
        "fedora:latest",
        "archlinux:latest",
        "opensuse/tumbleweed:latest",
    ):
        assert f"- {image}\n" in workflow, image
    assert (
        "FreightFate-*-linux-x64.tar.gz FreightFate-*-linux-x86_64.AppImage "
        "FreightFate-*-linux-arm64.tar.gz FreightFate-*-linux-aarch64.AppImage > checksums.txt"
        in workflow
    )

    smoke = (Path(__file__).resolve().parents[1] / "tools" / "linux_smoke.sh").read_text(
        encoding="utf-8"
    )
    # Speech is not disabled in the container boot: Prism really opens the
    # system's Speech Dispatcher, or boots silent where there is none.
    assert "FREIGHT_FATE_NO_SPEECH" not in smoke
    assert "Speech backend: Speech Dispatcher" in smoke
    assert 'grep -q " ERROR "' in smoke
    assert "--appimage-extract-and-run --smoke" in smoke
    assert "xvfb-run" in smoke
    # The script boots whichever pair matches the container it runs in.
    assert 'case "$(uname -m)"' in smoke
    assert "x86_64) tarball_arch=x64; appimage_arch=x86_64 ;;" in smoke
    assert "aarch64) tarball_arch=arm64; appimage_arch=aarch64 ;;" in smoke
    assert '/work/dist/FreightFate-*-linux-"$tarball_arch".tar.gz' in smoke
    assert '/work/dist/FreightFate-*-linux-"$appimage_arch".AppImage' in smoke


def test_career_19_snapshot_builds_an_apple_silicon_macos_release():
    workflow_path = (
        Path(__file__).resolve().parents[1] / ".github" / "workflows" / "build-career-1.9.yml"
    )
    workflow = yaml.load(workflow_path.read_text(encoding="utf-8"), Loader=yaml.BaseLoader)
    jobs = workflow["jobs"]
    mac = jobs["build_macos"]

    assert mac["runs-on"] == "macos-26"
    steps = mac["steps"]
    named_steps = {
        step["name"]: (index, step) for index, step in enumerate(steps) if "name" in step
    }
    assert any(step.get("uses") == "astral-sh/setup-uv@v8.2.0" for step in steps)
    assert named_steps["Install the pinned Rust toolchain"][1]["run"] == "rustup show"
    # No brewed SDL anywhere in the job: Homebrew's sdl2 is sdl2-compat,
    # which loads SDL3 at runtime and dies on player Macs. SDL2 is compiled
    # into the executable instead.
    assert all(
        "brew install" not in step.get("run", "") or "sdl" not in step.get("run", "")
        for step in steps
    )
    assert "uv run python tools/fetch_bass.py\n" in named_steps["Fetch BASS"][1]["run"]
    assert "uv run python tools/fetch_bass.py --check" in named_steps["Fetch BASS"][1]["run"]
    assert (
        named_steps["Lint Rust targets"][1]["run"]
        == "cargo clippy --all-targets --locked -- -D warnings"
    )
    assert named_steps["Test Rust workspace"][1]["run"] == "cargo test -p ff-core -p freight-fate"
    # The launch smoke is BACK on macOS: the hang that forced non-launch
    # verification was the boot probes, moved off the boot path 2026-08-30.
    # A packaged app that cannot boot must fail the build, not ship.
    assert (
        "uv run python tools/build_release.py --rust --smoke --tag"
        in named_steps["Build the macOS release"][1]["run"]
    )
    assert "--macos-non-launch-verify" not in named_steps["Build the macOS release"][1]["run"]
    assert "--skip-smoke" not in named_steps["Build the macOS release"][1]["run"]
    assert "Contents/MacOS/FreightFate" not in named_steps["Build the macOS release"][1]["run"]
    assert all("build-release.ps1" not in step.get("run", "") for step in steps)
    upload = next(step for step in steps if step.get("uses") == "actions/upload-artifact@v7")
    assert upload["with"]["path"] == "dist/FreightFate-*-macos-arm64.zip"


def test_career_19_release_requires_and_verifies_every_platform_archive():
    workflow_path = (
        Path(__file__).resolve().parents[1] / ".github" / "workflows" / "build-career-1.9.yml"
    )
    workflow = yaml.load(workflow_path.read_text(encoding="utf-8"), Loader=yaml.BaseLoader)
    release = workflow["jobs"]["release"]

    assert release["needs"] == [
        "prepare",
        "format",
        "build_windows",
        "build_macos",
        "build_linux",
        "smoke_linux",
    ]
    assert "needs.format.result == 'success'" in release["if"]
    assert "needs.build_windows.result == 'success'" in release["if"]
    assert "needs.build_macos.result == 'success'" in release["if"]
    assert "needs.build_linux.result == 'success'" in release["if"]
    assert "needs.smoke_linux.result == 'success'" in release["if"]
    downloads = [
        step for step in release["steps"] if step.get("uses") == "actions/download-artifact@v7"
    ]
    assert {step["with"]["name"] for step in downloads} == {
        "Windows",
        "macOS-arm64",
        "Linux-x86_64",
        "Linux-aarch64",
    }
    # Not `assets/`: that is a tracked source folder (sounds.pak, the add-ons)
    # since the Python sunset, and `gh release create ... <dir>/*` would
    # publish it next to the archives.
    assert all(step["with"]["path"] == "release-assets" for step in downloads)
    create = next(step for step in release["steps"] if step.get("name") == "Create prerelease")
    assert 'gh release create "$TAG" release-assets/*' in create["run"]
    assert "--prerelease" in create["run"]
    assert "is_prerelease == 'true'" in create["if"]
    stable = next(step for step in release["steps"] if step.get("name") == "Create stable release")
    assert "for f in release-assets/*; do" in stable["run"]
    assert 'gh release create "$TAG" "${assets[@]}"' in stable["run"]
    # The plain Mac copy is the arm64 app again; its label says so, since
    # GitHub shows the label where the file name would be.
    assert "(in-game updater copy, same app as the macos-arm64 zip)" in stable["run"]
    assert "--prerelease" not in stable["run"]
    assert "is_prerelease == 'false'" in stable["if"]
    assert "Freight Fate $VERSION" in stable["run"]
    # 1.8.8.1 and the 1.9 updater both pick a stable Mac archive by `-macos.zip`.
    assert 'cp "$zip" "${zip%-macos-arm64.zip}-macos.zip"' in stable["run"]
    assert stable["run"].index('-macos.zip"') < stable["run"].index("gh release create")
    assert "sha256sum FreightFate-* > checksums.txt" in stable["run"]
    verify = next(
        step for step in release["steps"] if step.get("name") == "Verify release archives"
    )
    assert "release-assets/FreightFate-*-windows-portable.zip" in verify["run"]
    assert "release-assets/FreightFate-*-macos-arm64.zip" in verify["run"]
    assert "release-assets/FreightFate-*-linux-x64.tar.gz" in verify["run"]
    assert "release-assets/FreightFate-*-linux-x86_64.AppImage" in verify["run"]
    assert "release-assets/FreightFate-*-linux-arm64.tar.gz" in verify["run"]
    assert "release-assets/FreightFate-*-linux-aarch64.AppImage" in verify["run"]
    assert verify["run"].count('"${#') == 6
    checksum = next(
        step
        for step in release["steps"]
        if step.get("name") == "Prepare and verify release notes and checksums"
    )
    assert (
        "sha256sum FreightFate-*-windows-portable.zip FreightFate-*-macos-arm64.zip "
        "FreightFate-*-linux-x64.tar.gz FreightFate-*-linux-x86_64.AppImage "
        "FreightFate-*-linux-arm64.tar.gz FreightFate-*-linux-aarch64.AppImage" in checksum["run"]
    )


def test_readme_names_the_apple_silicon_mac_archive():
    readme = (Path(__file__).resolve().parents[1] / "README.md").read_text(encoding="utf-8")
    assert "-macos-arm64.zip" in readme
    assert "Apple Silicon" in readme


def test_readme_names_both_linux_architectures():
    readme = (Path(__file__).resolve().parents[1] / "README.md").read_text(encoding="utf-8")
    for archive in (
        "-linux-x64.tar.gz",
        "-linux-x86_64.AppImage",
        "-linux-arm64.tar.gz",
        "-linux-aarch64.AppImage",
    ):
        assert archive in readme
    assert "BT Speak" in " ".join(readme.split())


def test_career_19_snapshot_prepares_bass_before_rust_validation():
    workflow_path = (
        Path(__file__).resolve().parents[1] / ".github" / "workflows" / "build-career-1.9.yml"
    )
    workflow = yaml.load(workflow_path.read_text(encoding="utf-8"), Loader=yaml.BaseLoader)
    for job_name, build_step_name in (
        ("build_windows", "Build the portable release"),
        ("build_macos", "Build the macOS release"),
    ):
        steps = workflow["jobs"][job_name]["steps"]
        named_steps = {
            step["name"]: (index, step) for index, step in enumerate(steps) if "name" in step
        }

        bass_index, bass_step = named_steps["Fetch BASS"]
        assert "uv run python tools/fetch_bass.py\n" in bass_step["run"]
        assert "uv run python tools/fetch_bass.py --check" in bass_step["run"]
        assert bass_index < named_steps["Lint Rust targets"][0]
        assert bass_index < named_steps["Test Rust workspace"][0]
        assert named_steps["Test Rust workspace"][0] < named_steps[build_step_name][0]


def test_career_19_snapshot_runs_each_platform_independent_check_once():
    """Formatting and the whole-map sweeps do not depend on the platform, so
    each runs on one runner; every check still gates the release."""
    workflow_path = (
        Path(__file__).resolve().parents[1] / ".github" / "workflows" / "build-career-1.9.yml"
    )
    jobs = yaml.load(workflow_path.read_text(encoding="utf-8"), Loader=yaml.BaseLoader)["jobs"]

    def step(job, name):
        return next(s for s in jobs[job]["steps"] if s.get("name") == name)

    fmt_jobs = [
        name
        for name, job in jobs.items()
        if any("cargo fmt" in s.get("run", "") for s in job.get("steps", []))
    ]
    assert fmt_jobs == ["format"]
    assert jobs["format"]["runs-on"] == "ubuntu-latest"
    assert step("format", "Check Rust formatting")["run"] == "cargo fmt --all --check"

    # The sweeps run once, on Windows; the other runners take the quick set,
    # with clippy on the same RUSTFLAGS so the debug native builds are shared.
    assert "RUSTFLAGS" not in step("build_windows", "Test Rust workspace").get("env", {})
    for job in ("build_macos", "build_linux"):
        for name in ("Lint Rust targets", "Test Rust workspace"):
            assert step(job, name)["env"]["RUSTFLAGS"] == "--cfg ci_quick", (job, name)
        assert "RUSTFLAGS" not in jobs[job].get("env", {})

    # Windows clippy is skipped only on the schedule, whose dev tip rust.yml
    # already linted; tag pushes and manual runs still lint.
    assert step("build_windows", "Lint Rust targets")["if"] == "github.event_name != 'schedule'"
    for job in ("build_macos", "build_linux"):
        assert "if" not in step(job, "Lint Rust targets")


def test_career_19_retry_is_bounded_to_one_delayed_attempt():
    workflow = (
        Path(__file__).resolve().parents[1] / ".github" / "workflows" / "retry-failed-nightly.yml"
    ).read_text(encoding="utf-8")

    # One workflow to watch since the cutover deleted build.yml, and the
    # branch it retries against is dev -- this named feat/career-1.9 while
    # 1.9 was a side line, which would have dispatched the retry at the
    # wrong ref the first time the nightly failed.
    assert "workflows: [Career 1.9 snapshot]" in workflow
    assert "Build" not in workflow.split("jobs:")[0]
    assert "feat/career-1.9" not in workflow
    assert "github.event.workflow_run.run_attempt == 1" in workflow
    assert 'SNAPSHOT_WORKFLOW: "Career 1.9 snapshot"' in workflow
    assert 'SNAPSHOT_BRANCH: "dev"' in workflow
    assert 'TAG="1.9-tester-$(date -u +%Y%m%d)"' in workflow
    assert ('gh workflow run "$SNAPSHOT_WORKFLOW" --repo "$GITHUB_REPOSITORY"') in workflow
    assert '"$TAG already exists; the snapshot recovered while waiting."' in workflow


def test_the_gate_covers_the_shipping_runtime_but_not_its_tests():
    # The gate was written when src/ WAS the game. The Rust port moved every
    # line of gameplay into crates/ and the gate was not widened with it, so
    # a change to the shipping runtime could land with no entry at all.
    module = load_release_notes_module()
    for path in (
        "crates/freight-fate/src/states/driving_turns.rs",
        "crates/ff-core/src/sim/trip.rs",
        "data/facility_endpoints.json",
        "assets/sounds.pak",
        "docs/ontology.md",
        "CHANGELOG.md",
    ):
        assert module.is_user_facing_path(path), path
    # A test or bench is not a player-facing change: under the Python layout
    # tests/ sat beside the game and was never gated.
    for path in (
        "crates/ff-core/tests/it/sim_trip_cues.rs",
        "crates/freight-fate/benches/frame_time.rs",
        "tools/build_facility_endpoints.py",
        "data/spider/gap-fill/gap_scan_nn.py",
        ".github/workflows/rust.yml",
    ):
        assert not module.is_user_facing_path(path), path


def test_auto_base_follows_the_release_line_the_branch_was_cut_from(tmp_path, monkeypatch):
    """A hotfix is cut from main and never contains dev.

    Resolving its base to dev counted dev's bullets as already present and
    rejected the push over a changelog entry that was right there -- which is
    what the 1.8.6.2 hotfix hit. Ancestry cannot decide it either, since the
    two lines each carry commits the other does not.
    """
    release_notes = load_release_notes_module()
    repo = make_repo(tmp_path, changelog("- Seed entry."))
    monkeypatch.setattr(release_notes, "ROOT", repo)
    seed = git(repo, "rev-parse", "HEAD")  # git init's branch name varies
    # dev moves ahead of main, as it always is between releases...
    git(repo, "checkout", "-q", "-b", "dev")
    for n in range(4):
        (repo / f"dev{n}.txt").write_text("dev work\n", encoding="utf-8")
        commit(repo, f"feat: dev only {n}")
    git(repo, "update-ref", "refs/remotes/origin/dev", "HEAD")

    # ...and main carries release commits of its own that dev never sees, so
    # neither branch contains the other. That is the shape this has to read.
    git(repo, "checkout", "-q", "-b", "stable-line", seed)
    (repo / "release.txt").write_text("1.0\n", encoding="utf-8")
    commit(repo, "release: 1.0")
    git(repo, "update-ref", "refs/remotes/origin/main", "HEAD")

    git(repo, "checkout", "-q", "-b", "hotfix/1.0.1", "refs/remotes/origin/main")
    (repo / "fix.txt").write_text("hotfix\n", encoding="utf-8")
    commit(repo, "fix: stable only")
    assert release_notes.nearest_release_line() == "origin/main"
    assert release_notes.resolve_base("auto") == "origin/main"

    # An ordinary branch off dev still resolves to dev...
    git(repo, "checkout", "-q", "-b", "feat/thing", "refs/remotes/origin/dev")
    (repo / "feature.txt").write_text("feature\n", encoding="utf-8")
    commit(repo, "feat: a feature")
    assert release_notes.nearest_release_line() == "origin/dev"

    # ...and still does once dev has moved on without it.
    git(repo, "checkout", "-q", "dev")
    (repo / "more.txt").write_text("more dev work\n", encoding="utf-8")
    commit(repo, "feat: more dev")
    git(repo, "update-ref", "refs/remotes/origin/dev", "HEAD")
    git(repo, "checkout", "-q", "feat/thing")
    assert release_notes.nearest_release_line() == "origin/dev"

    # An explicit base is never second-guessed.
    assert release_notes.resolve_base("origin/main") == "origin/main"
