"""tools/ios_signing.py: JWT shape, API sequence, error text, cleanup."""

from __future__ import annotations

import base64
import importlib.util
import json
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location("ios_signing", ROOT / "tools" / "ios_signing.py")
ios_signing = importlib.util.module_from_spec(spec)
sys.modules["ios_signing"] = ios_signing
spec.loader.exec_module(ios_signing)

needs_openssl = pytest.mark.skipif(shutil.which("openssl") is None, reason="no openssl")


def _der(r: int, s: int) -> bytes:
    def enc(n: int) -> bytes:
        raw = n.to_bytes((n.bit_length() + 7) // 8 or 1, "big")
        if raw[0] & 0x80:
            raw = b"\x00" + raw
        return b"\x02" + bytes([len(raw)]) + raw

    body = enc(r) + enc(s)
    return b"\x30" + bytes([len(body)]) + body


def test_der_to_raw_pads_and_strips() -> None:
    r, s = 0x80 << 248, 5  # r needs a leading zero in DER, s is short
    raw = ios_signing.der_to_raw_signature(_der(r, s))
    assert len(raw) == 64
    assert int.from_bytes(raw[:32], "big") == r
    assert int.from_bytes(raw[32:], "big") == s


def test_der_to_raw_rejects_junk() -> None:
    with pytest.raises(ios_signing.SigningError):
        ios_signing.der_to_raw_signature(b"nope")


def test_token_claims(monkeypatch, tmp_path) -> None:
    monkeypatch.setattr(ios_signing, "sign_es256", lambda data, path: b"\x01" * 64)
    token = ios_signing.make_token("KEY123", "issuer-1", tmp_path / "k.p8", now=1000)
    head, payload, sig = token.split(".")

    def dec(part: str) -> dict:
        return json.loads(base64.urlsafe_b64decode(part + "=" * (-len(part) % 4)))

    assert dec(head) == {"alg": "ES256", "kid": "KEY123", "typ": "JWT"}
    claims = dec(payload)
    assert claims["iss"] == "issuer-1" and claims["aud"] == "appstoreconnect-v1"
    assert claims["exp"] - claims["iat"] <= 20 * 60
    assert len(base64.urlsafe_b64decode(sig + "==")) == 64


@needs_openssl
def test_real_openssl_signature_is_64_bytes(tmp_path) -> None:
    key = tmp_path / "k.p8"
    subprocess.run(
        ["openssl", "genpkey", "-algorithm", "EC", "-pkeyopt", "ec_paramgen_curve:P-256",
         "-out", str(key)],
        check=True, capture_output=True,
    )  # fmt: skip
    assert len(ios_signing.sign_es256(b"abc", key)) == 64


@needs_openssl
def test_csr_and_key_files(tmp_path) -> None:
    csr = ios_signing.make_key_and_csr(tmp_path, "Freight Fate CI 1")
    assert "BEGIN CERTIFICATE REQUEST" in csr
    assert (tmp_path / ios_signing.KEY_FILE).exists()


class FakeApi:
    def __init__(self, bundles=None, fail=None):
        self.calls: list[tuple[str, str, dict | None]] = []
        self.bundles = bundles if bundles is not None else []
        self.fail = fail or {}

    def __call__(self, token, method, path, body=None):
        self.calls.append((method, path.split("?")[0], body))
        key = (method, path.split("?")[0])
        if key in self.fail:
            raise self.fail[key]
        if key == ("GET", "/v1/bundleIds"):
            return {"data": self.bundles}
        if key == ("GET", "/v1/certificates"):
            return {"data": []}
        if key == ("POST", "/v1/bundleIds"):
            return {"data": {"id": "B1"}}
        if key == ("POST", "/v1/certificates"):
            cert = base64.b64encode(b"certbytes").decode()
            return {"data": {"id": "C1", "attributes": {"certificateContent": cert}}}
        if key == ("POST", "/v1/profiles"):
            prof = base64.b64encode(b"profilebytes").decode()
            return {"data": {"id": "P1", "attributes": {"profileContent": prof}}}
        if key == ("GET", "/v1/apps"):
            return {"data": self.bundles}
        return {}


@pytest.fixture
def api(monkeypatch, tmp_path):
    fake = FakeApi()
    monkeypatch.setattr(ios_signing, "api_request", fake)
    monkeypatch.setattr(ios_signing, "_token", lambda: "tok")

    def fake_csr(out, common_name):
        (out / ios_signing.KEY_FILE).write_text("k")
        return "csr"

    monkeypatch.setattr(ios_signing, "make_key_and_csr", fake_csr)
    return fake


def test_prepare_registers_bundle_and_links_profile(api, tmp_path) -> None:
    assert ios_signing.main(["prepare", "--out", str(tmp_path)]) == 0
    posts = [(c[1], c[2]) for c in api.calls if c[0] == "POST"]
    assert [p[0] for p in posts] == ["/v1/bundleIds", "/v1/certificates", "/v1/profiles"]
    cert_attrs = posts[1][1]["data"]["attributes"]
    assert cert_attrs["certificateType"] == "IOS_DISTRIBUTION" and cert_attrs["csrContent"] == "csr"
    rel = posts[2][1]["data"]["relationships"]
    assert rel["bundleId"]["data"]["id"] == "B1"
    assert rel["certificates"]["data"][0]["id"] == "C1"
    assert posts[2][1]["data"]["attributes"]["profileType"] == "IOS_APP_STORE"
    assert (tmp_path / ios_signing.PROFILE_FILE).read_bytes() == b"profilebytes"
    assert "BEGIN CERTIFICATE" in (tmp_path / ios_signing.CERT_FILE).read_text()
    assert json.loads((tmp_path / ios_signing.STATE_FILE).read_text()) == {
        "certificate_id": "C1",
        "profile_id": "P1",
    }


def test_prepare_reuses_existing_bundle_id(api, tmp_path) -> None:
    api.bundles = [{"id": "EXIST", "attributes": {"identifier": ios_signing.BUNDLE_ID}}]
    assert ios_signing.main(["prepare", "--out", str(tmp_path)]) == 0
    assert ("POST", "/v1/bundleIds") not in [(c[0], c[1]) for c in api.calls]


def test_prepare_ignores_prefix_bundle_matches(api, tmp_path) -> None:
    api.bundles = [{"id": "X", "attributes": {"identifier": ios_signing.BUNDLE_ID + ".extra"}}]
    ios_signing.main(["prepare", "--out", str(tmp_path)])
    assert ("POST", "/v1/bundleIds") in [(c[0], c[1]) for c in api.calls]


def test_forbidden_key_gets_plain_message(api, tmp_path, capsys) -> None:
    err = ios_signing.ApiError("GET", "/v1/bundleIds", 403, "no")
    api.fail[("GET", "/v1/bundleIds")] = err
    assert ios_signing.main(["prepare", "--out", str(tmp_path)]) == 1
    out = capsys.readouterr().out
    assert "App Manager or Admin" in out


def test_unauthorized_key_message(api, tmp_path, capsys) -> None:
    api.fail[("GET", "/v1/certificates")] = ios_signing.ApiError("GET", "/v1/certificates", 401, "")
    assert ios_signing.main(["prepare", "--out", str(tmp_path)]) == 1
    assert "ASC_KEY_P8_BASE64" in capsys.readouterr().out


def test_certificate_limit_message(api, tmp_path, capsys) -> None:
    api.fail[("POST", "/v1/certificates")] = ios_signing.ApiError(
        "POST", "/v1/certificates", 409, "limit reached"
    )
    assert ios_signing.main(["prepare", "--out", str(tmp_path)]) == 1
    assert "revoke an old" in capsys.readouterr().out


def test_missing_app_record_says_what_to_do(api, capsys) -> None:
    assert ios_signing.main(["check-app"]) == 1
    out = capsys.readouterr().out
    assert "net.orinks.freightfate" in out and "must match" in out


def test_app_record_present(api, capsys) -> None:
    api.bundles = [{"id": "A"}]
    assert ios_signing.main(["check-app"]) == 0


def test_cleanup_deletes_profile_then_cert_and_files(api, tmp_path) -> None:
    (tmp_path / ios_signing.STATE_FILE).write_text(
        json.dumps({"certificate_id": "C1", "profile_id": "P1"})
    )
    (tmp_path / ios_signing.KEY_FILE).write_text("secret")
    assert ios_signing.main(["cleanup", "--out", str(tmp_path)]) == 0
    deletes = [c[1] for c in api.calls if c[0] == "DELETE"]
    assert deletes == ["/v1/profiles/P1", "/v1/certificates/C1"]
    assert list(tmp_path.iterdir()) == []


def test_cleanup_failure_warns_and_still_removes_files(api, tmp_path, capsys) -> None:
    api.fail[("DELETE", "/v1/certificates/C1")] = ios_signing.ApiError(
        "DELETE", "/v1/certificates/C1", 500, "x"
    )
    (tmp_path / ios_signing.STATE_FILE).write_text(json.dumps({"certificate_id": "C1"}))
    (tmp_path / ios_signing.KEY_FILE).write_text("secret")
    assert ios_signing.main(["cleanup", "--out", str(tmp_path)]) == 0
    assert "::warning::" in capsys.readouterr().out
    assert not (tmp_path / ios_signing.KEY_FILE).exists()


def test_cleanup_with_no_state_is_quiet(tmp_path) -> None:
    assert ios_signing.main(["cleanup", "--out", str(tmp_path)]) == 0


def test_bundle_id_comes_from_build_ios() -> None:
    assert ios_signing.BUNDLE_ID == ios_signing._BUILD_IOS.BUNDLE_ID


def test_next_build_continues_after_highest() -> None:
    builds = [("1.9.0", "7"), ("1.9.0", "12"), ("1.8.0", "1.9.0.dev0")]
    assert ios_signing.next_build_number(builds, "1.9.0", 3) == (13, 12)


def test_next_build_never_below_run_number() -> None:
    assert ios_signing.next_build_number([("1.9.0", "4")], "1.9.0", 30) == (31, 4)
    assert ios_signing.next_build_number([], "1.9.0", 0) == (1, 0)


def test_next_build_refuses_lower_version() -> None:
    with pytest.raises(ios_signing.SigningError, match="higher than this build"):
        ios_signing.next_build_number([("1.9.1", "2")], "1.9.0", 1)
