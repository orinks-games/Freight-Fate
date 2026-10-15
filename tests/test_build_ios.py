"""Device IPA packaging and signing modes, without compiling the game."""

# ruff: noqa: W191 -- tabs follow the repository contributor instructions.

from __future__ import annotations

import plistlib
import shutil
import sys
import zipfile
from types import SimpleNamespace
from unittest.mock import Mock

import build_ios
import pytest


@pytest.fixture
def builder(tmp_path, monkeypatch):
	app = tmp_path / "iphoneos" / "FreightFate.app"
	app.mkdir(parents=True)
	frameworks = [app / "Frameworks" / "bass.framework"]
	profile_dir = tmp_path / "target"
	release = SimpleNamespace(project_version=lambda: "1.9.3")
	mocks = {
		"load_build_release": Mock(return_value=release),
		"cargo_build": Mock(return_value=profile_dir),
		"stage_app": Mock(return_value=(app, frameworks)),
		"codesign": Mock(),
		"package_ipa": Mock(return_value=tmp_path / "FreightFate.ipa"),
		"simctl": Mock(),
		"provisioning_entitlements": Mock(return_value={"get-task-allow": True}),
	}
	for name, mock in mocks.items():
		monkeypatch.setattr(build_ios, name, mock)
	monkeypatch.setattr(build_ios, "IOS_BUILD", tmp_path)
	return SimpleNamespace(
		app=app,
		frameworks=frameworks,
		profile_dir=profile_dir,
		release=release,
		root=tmp_path,
		**mocks,
	)


def test_sideload_builds_device_ipa_without_credentials(builder, capsys):
	assert build_ios.main(["--sideload", "--debug", "--no-music", "--build-number", "7"]) == 0
	builder.cargo_build.assert_called_once_with(build_ios.DEVICE, release=False)
	builder.stage_app.assert_called_once_with(
		builder.profile_dir, build_ios.DEVICE, "1.9.3", builder.release, False, "7"
	)
	builder.codesign.assert_called_once_with(builder.app, builder.frameworks, "-", None)
	builder.package_ipa.assert_called_once_with(builder.app)
	builder.provisioning_entitlements.assert_not_called()
	builder.simctl.assert_not_called()
	assert not (builder.app / "embedded.mobileprovision").exists()
	assert "Re-sign and install the IPA" in capsys.readouterr().out


def test_signed_device_ipa_keeps_profile_and_entitlements(builder):
	profile = builder.root / "test.mobileprovision"
	profile.write_bytes(b"profile")
	assert (
		build_ios.main(
			[
				"--device",
				"--ipa",
				"--sign-identity",
				"Apple Development: Test",
				"--provisioning-profile",
				str(profile),
			]
		)
		== 0
	)
	builder.codesign.assert_called_once_with(
		builder.app,
		builder.frameworks,
		"Apple Development: Test",
		builder.root / "entitlements.plist",
	)
	assert (builder.app / "embedded.mobileprovision").read_bytes() == b"profile"
	assert plistlib.loads((builder.root / "entitlements.plist").read_bytes()) == {
		"get-task-allow": True,
	}
	builder.package_ipa.assert_called_once_with(builder.app)


def test_simulator_still_installs_app_without_an_ipa(builder):
	assert build_ios.main(["--install", "--launch"]) == 0
	builder.cargo_build.assert_called_once_with(build_ios.SIMULATOR, release=True)
	builder.codesign.assert_called_once_with(builder.app, builder.frameworks, "-", None)
	builder.package_ipa.assert_not_called()
	assert builder.simctl.call_args_list == [
		(("install", "booted", str(builder.app)),),
		(("launch", "booted", build_ios.BUNDLE_ID),),
	]


@pytest.mark.parametrize(
	"arguments",
	[
		["--ipa"],
		["--device"],
		["--sideload", "--install"],
		["--sideload", "--launch"],
		["--sideload", "--sign-identity", "Test"],
		["--sideload", "--provisioning-profile", "test.mobileprovision"],
	],
)
def test_invalid_signing_modes_fail_before_building(builder, arguments):
	with pytest.raises(SystemExit) as error:
		build_ios.main(arguments)
	assert error.value.code == 2
	builder.cargo_build.assert_not_called()
	builder.stage_app.assert_not_called()
	builder.codesign.assert_not_called()
	builder.package_ipa.assert_not_called()


@pytest.mark.skipif(sys.platform != "darwin" or not shutil.which("ditto"), reason="uses ditto")
def test_ipa_preserves_payload_resources_and_executable_mode(tmp_path, monkeypatch):
	monkeypatch.setattr(build_ios, "IOS_BUILD", tmp_path)
	app = tmp_path / "iphoneos" / "FreightFate.app"
	app.mkdir(parents=True)
	executable = app / "FreightFate"
	executable.write_bytes(b"device executable")
	executable.chmod(0o755)
	(app / "Info.plist").write_bytes(
		plistlib.dumps(build_ios.info_plist("1.9.3", build_ios.DEVICE))
	)
	resources = app / "freight_fate"
	resources.mkdir()
	(resources / "sounds.pak").write_bytes(b"sounds")
	framework = app / "Frameworks" / "bass.framework"
	framework.mkdir(parents=True)
	(framework / "bass").write_bytes(b"framework")
	(framework / "alias").symlink_to("bass")
	(tmp_path / "FreightFate.ipa").write_bytes(b"old artifact")

	ipa = build_ios.package_ipa(app)
	with zipfile.ZipFile(ipa) as archive:
		prefix = "Payload/FreightFate.app/"
		assert archive.read(prefix + "FreightFate") == b"device executable"
		assert archive.getinfo(prefix + "FreightFate").external_attr >> 16 & 0o111 == 0o111
		assert archive.read(prefix + "freight_fate/sounds.pak") == b"sounds"
		assert archive.read(prefix + "Frameworks/bass.framework/bass") == b"framework"
		assert archive.read(prefix + "Frameworks/bass.framework/alias") == b"bass"
		info = plistlib.loads(archive.read(prefix + "Info.plist"))
		assert info["CFBundleSupportedPlatforms"] == ["iPhoneOS"]
		assert info["CFBundleExecutable"] == "FreightFate"
