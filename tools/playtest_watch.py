"""Watch a live manual playtest and report what is worth interrupting for.

A playtest is somebody driving a truck by ear for half an hour. Whoever is
running the session cannot read a log at the same time, and the interesting
lines -- a traceback, a cloud request from a session that was supposed to be
sandboxed, a warning nobody heard over the engine -- are a few among tens of
thousands. This reads the session log as it is written and prints one line per
thing worth knowing, so a shell watching this script's stdout gets a
notification instead of a wall of text.

Three duties, deliberately different in cadence:

* **Now.** Errors, tracebacks, and any request that reached the site are
  printed the moment they land. These are the reason the watcher exists.
* **Occasionally.** Every few minutes, one check-in line: where the drive has
  got to and what was last spoken. It goes quiet on its own when nothing is
  happening -- a truck parked in a menu should not generate news -- and speaks
  up again when the road starts talking.
* **After.** When the game exits, one closing summary: how long, how much was
  spoken, what went wrong, and whether the sandbox really did stay off the
  network.

Run it against a session started by the game's sandbox launcher
(``freightfate --playtest-sandbox --launch``, or ``--playtest-road``)::

    uv run python tools/playtest_watch.py

Or point it at any session log, with a faster check-in for a short drive::

    uv run python tools/playtest_watch.py --log logs/playtest.log --every 60

It exits when the session does, so a shell can wait on it.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
DEFAULT_LOG = ROOT / "logs" / "playtest-manual.log"
SESSION_FILE = ROOT / "logs" / "playtest-session.json"

# How often the check-in fires, and how many silent check-ins pass before the
# watcher stops saying "still quiet". Driving speech is near-continuous, so a
# gap this long means the session is parked, in a menu, or over.
DEFAULT_EVERY_S = 180.0
QUIET_REPORTS_BEFORE_HUSH = 1

# How long to keep watching a log after the session file says the game is
# gone: the last frames of speech and the shutdown lines are still being
# flushed while the process tears down.
DRAIN_S = 3.0

POLL_S = 1.0

# A python log line: "2026-08-20 19:44:01,123 LEVEL logger: message".
LINE_RE = re.compile(
    r"^(?P<time>\d{4}-\d\d-\d\d \d\d:\d\d:\d\d),\d+ "
    # The Rust build logs under module paths (`freight_fate::cloud_saves`),
    # the Python one under dotted names. Accept both or every Rust line is
    # read as a traceback body -- which silently killed the network alarm.
    r"(?P<level>[A-Z]+) (?P<logger>[\w.:]+): (?P<message>.*)$"
)

TRANSCRIPT_LOGGER = "freight_fate.transcript"

# Loggers that have no business speaking during a sandboxed playtest. A line
# from any of them means the session reached, or tried to reach, the account
# it was supposed to be walled off from -- which is exactly the failure the
# sandbox exists to prevent, and therefore worth an immediate interruption.
NETWORK_LOGGERS = (
    "freight_fate.cloud_saves",
    "freight_fate.online_presence",
    "freight_fate.online_activation",
)

# What the watcher can and cannot see here, said plainly because the closing
# summary is only worth reading if it does not overstate itself. These
# loggers speak on FAILURE -- a refused upload, an unreachable site. A
# request that succeeds writes nothing, so silence here is not proof that
# nothing was sent. Reading the public drivers board, for instance, works in
# a sandbox and leaves no trace in the log at all.
#
# That is fine, because the sandbox's guarantee was never "no packets". It is
# that nothing can be PUBLISHED: with no online.json there is no driver, and
# cloud backup, the presence heartbeat and the profile update are all branches
# the game never takes. The identity audit below is what actually proves that;
# this channel only catches something trying and failing.

# Raw text that means a crash even when it arrives outside the log format --
# faulthandler writes native tracebacks straight into the file with no
# level, no logger, and no timestamp.
CRASH_MARKERS = (
    "Traceback (most recent call last)",
    "Current thread 0x",
    "Fatal Python error",
    "Windows fatal exception",
)

# The transcript's own bookkeeping prefixes. Useful to count, never useful to
# quote back as "the last thing the driver heard".
TRANSCRIPT_NOISE = ("[ladder]", "[pacer]")

# Instrumentation, not speech: a line the TRUCK wrote about what it did, with
# no player-facing utterance behind it. Counting these as spoken would inflate
# every check-in, and calling them "silenced" would be a lie -- nothing was
# cut. They get their own tally.
TRANSCRIPT_INSTRUMENTS = ("[jake]",)

# Lines the game logs on every start no matter what; flagging them as warnings
# would train the operator to ignore the warning channel on the first minute
# of every session.
BENIGN_WARNINGS = (
    "Could not set default device",
    "BASS could not load plugin",
)


class Session:
    """What the watcher believes about the live game process."""

    def __init__(self, path: Path) -> None:
        self.path = path
        self.pid: int | None = None
        self.log: Path | None = None
        self.sandbox: Path | None = None
        self.refresh()

    def refresh(self) -> None:
        try:
            data = json.loads(self.path.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return
        self.pid = data.get("pid")
        for attr, key in (("log", "log"), ("sandbox", "sandbox")):
            value = data.get(key)
            if isinstance(value, str):
                setattr(self, attr, Path(value))
        self._running = bool(data.get("running"))

    def alive(self) -> bool:
        """True while the game is still running.

        Two signals, because neither alone is trustworthy: the session file's
        own flag is not written by a process that crashed, and a pid can be
        missing on a session started some other way.
        """
        self.refresh()
        if not getattr(self, "_running", False):
            return False
        if self.pid is None:
            return True
        return _pid_alive(self.pid)


def _pid_alive(pid: int) -> bool:
    """Whether that process is still up, on Windows and on POSIX alike."""
    if sys.platform == "win32":
        import ctypes

        # PROCESS_QUERY_LIMITED_INFORMATION: enough to ask, not enough to
        # touch, so this works without any elevation.
        handle = ctypes.windll.kernel32.OpenProcess(0x1000, False, pid)
        if not handle:
            return False
        exit_code = ctypes.c_ulong()
        ok = ctypes.windll.kernel32.GetExitCodeProcess(handle, ctypes.byref(exit_code))
        ctypes.windll.kernel32.CloseHandle(handle)
        # 259 is STILL_ACTIVE.
        return bool(ok) and exit_code.value == 259
    import os

    try:
        os.kill(pid, 0)
    except ProcessLookupError:
        return False
    except PermissionError:
        return True
    return True


class Tail:
    """A log file read as it grows, surviving the game rotating it.

    The game renames an existing log to ``*.prev.log`` and opens a fresh one
    at startup, so a watcher armed a moment early would otherwise spend the
    session reading a file nobody is writing to any more. A file that shrank
    is that rotation, and the answer is to reopen it.
    """

    def __init__(self, path: Path) -> None:
        self.path = path
        self._handle = None
        self._pos = 0

    def _open(self) -> None:
        if not self.path.exists():
            return
        # Held open for the session on purpose: a tail that reopened per poll
        # would lose its read position every second.
        self._handle = open(self.path, encoding="utf-8", errors="replace")  # noqa: SIM115
        self._handle.seek(0, 2)
        self._pos = self._handle.tell()

    def lines(self) -> list[str]:
        if self._handle is None:
            self._open()
            if self._handle is None:
                return []
        try:
            size = self.path.stat().st_size
        except OSError:
            return []
        if size < self._pos:
            self._handle.close()
            self._handle = None
            self._pos = 0
            self._open()
            if self._handle is None:
                return []
            self._handle.seek(0)
        out = self._handle.readlines()
        self._pos = self._handle.tell()
        return [line.rstrip("\n") for line in out]


class Watcher:
    def __init__(self, every: float) -> None:
        self.every = every
        self.started = time.monotonic()
        self.spoken = 0
        self.silenced = 0
        self.instrumented = 0
        self.last_spoken: str | None = None
        self.errors: list[str] = []
        self.warnings: list[str] = []
        self.network: list[str] = []
        self.crashes: list[str] = []
        self.crash_context: list[str] = []
        self._since_checkin = 0
        self._quiet_reports = 0
        self._in_crash = False

    # -- the immediate channel ------------------------------------------------

    def consume(self, raw: str) -> list[str]:
        """Fold one log line in; return whatever must be said right now."""
        if any(marker in raw for marker in CRASH_MARKERS):
            self._in_crash = True
            self.crashes.append(raw.strip())
            return [f"CRASH: {raw.strip()}"]

        match = LINE_RE.match(raw)
        if match is None:
            # A traceback's body. It belongs with the crash it explains, and
            # counting each frame as its own error would report a single
            # exception as a dozen -- the number the summary leads with has
            # to mean "things went wrong this many times".
            if self._in_crash and raw.strip():
                self.crash_context.append(raw.strip())
            return []
        self._in_crash = False

        level = match["level"]
        # Normalise the Rust module path to the dotted name the rest of this
        # file (and NETWORK_LOGGERS) is written against.
        logger = match["logger"].replace("::", ".")
        message = match["message"]

        if logger == TRANSCRIPT_LOGGER:
            if message.startswith(TRANSCRIPT_INSTRUMENTS):
                self.instrumented += 1
                return []
            if message.startswith(TRANSCRIPT_NOISE):
                self.silenced += 1
                return []
            self.spoken += 1
            self._since_checkin += 1
            self.last_spoken = message.removeprefix("[event] ")
            return []

        if logger.startswith(NETWORK_LOGGERS):
            self.network.append(f"{logger}: {message}")
            return [f"ONLINE trouble in a sandboxed session -- {logger}: {message}"]

        if level in ("ERROR", "CRITICAL"):
            self.errors.append(f"{logger}: {message}")
            return [f"{level}: {logger}: {message}"]

        if level == "WARNING":
            if any(benign in message for benign in BENIGN_WARNINGS):
                return []
            self.warnings.append(f"{logger}: {message}")
            return [f"WARNING: {logger}: {message}"]

        return []

    # -- the occasional channel -----------------------------------------------

    def checkin(self) -> str | None:
        """One line on how the drive is going, or None to stay quiet."""
        elapsed = _duration(time.monotonic() - self.started)
        if self._since_checkin == 0:
            self._quiet_reports += 1
            if self._quiet_reports > QUIET_REPORTS_BEFORE_HUSH:
                return None
            return f"quiet -- nothing spoken in the last {_duration(self.every)} ({elapsed} in)"
        said = self._since_checkin
        self._since_checkin = 0
        self._quiet_reports = 0
        tail = f' Last: "{self.last_spoken}"' if self.last_spoken else ""
        return f"{elapsed} in, {_count(said, 'line')} spoken since last check-in.{tail}"

    # -- the closing channel --------------------------------------------------

    def summary(self, sandbox: Path | None) -> list[str]:
        out = [
            f"SESSION ENDED after {_duration(time.monotonic() - self.started)}: "
            f"{_count(self.spoken, 'line')} spoken, "
            f"{self.silenced} silenced by the rung or the pacer, "
            f"{self.instrumented} truck-state notes, "
            f"{_count(len(self.crashes), 'crash')}, "
            f"{_count(len(self.errors), 'error')}, "
            f"{_count(len(self.warnings), 'warning')}."
        ]
        for crash in self.crashes[:3]:
            out.append(f"  crash: {crash}")
        for frame in self.crash_context[:8]:
            out.append(f"    {frame}")
        for err in self.errors[:5]:
            out.append(f"  error: {err}")
        if len(self.errors) > 5:
            out.append(f"  ...and {len(self.errors) - 5} more errors in the log")
        for warn in self.warnings[:5]:
            out.append(f"  warning: {warn}")
        if len(self.warnings) > 5:
            out.append(f"  ...and {len(self.warnings) - 5} more warnings in the log")
        if self.network:
            out.append(
                f"  NETWORK: {_count(len(self.network), 'online call')} logged a problem "
                "from a sandboxed session"
            )
        else:
            # Deliberately not "nothing reached the site": these loggers speak
            # on failure, and a read that worked -- the public drivers board --
            # leaves no line at all. See NETWORK_LOGGERS.
            out.append("  network: no online call reported a problem")
        if sandbox is not None:
            out.extend(_sandbox_verdict(sandbox))
        return out


# What ``freight_fate::playtest::sandbox`` counts as the driver identity and
# as the settings that publish. Mirrored here, not imported: the watcher is a
# plain script and the sandbox is Rust. Change one, change both -- except
# meaningful_play.json: the sandbox strips it at start, but the game itself
# rewrites it during any drive (a pending-intent ledger, nothing uploadable with
# cloud_saves off), so a closing audit that names it is a false alarm.
IDENTITY_NAMES = frozenset(
    {
        "online.json",
        "online.token",
        "cloud_saves.json",
        "online-outbox.json",
    }
)
OFFLINE_SETTINGS = ("cloud_saves", "online_presence", "online_services")


def _is_identity(path: Path) -> bool:
    """True for a file that would carry the real account into a sandbox,
    backup spellings included (``online.json.pre-clerk.bak``)."""
    name = path.name
    return (
        name in IDENTITY_NAMES or name.startswith("online.json") or name.startswith("online.token")
    )


def audit(sandbox: Path) -> list[str]:
    """Every reason this sandbox could still reach the real account."""
    problems = [
        f"identity file in the sandbox: {path.relative_to(sandbox)}"
        for path in sorted(sandbox.rglob("*"))
        if path.is_file() and _is_identity(path)
    ]
    settings = sandbox / "settings.json"
    if settings.is_file():
        try:
            data = json.loads(settings.read_text(encoding="utf-8"))
        except (OSError, json.JSONDecodeError):
            return [*problems, "settings.json is unreadable; cannot confirm publishing is off"]
        data = data if isinstance(data, dict) else {}
        problems += [
            f"settings.json still has {key} on" for key in OFFLINE_SETTINGS if data.get(key)
        ]
    return problems


def _sandbox_verdict(sandbox: Path) -> list[str]:
    """Re-audit the sandbox after the session, not before it.

    A drive can create files -- this is the check that the session did not
    write an identity into a directory that started clean.
    """
    problems = audit(sandbox)
    if not problems:
        return ["  sandbox: still has no driver identity"]
    return ["  SANDBOX BREACHED:"] + [f"    - {p}" for p in problems]


def _duration(seconds: float) -> str:
    """A span said the way a person watching a playtest would say it."""
    if seconds < 90:
        return f"{seconds:.0f} sec"
    return f"{seconds / 60:.0f} min"


# Nouns whose plural is not the noun plus "s". Small on purpose: the summary
# is read by a person, and "0 crashs" undermines every other number beside it.
_PLURALS = {"crash": "crashes"}


def _count(n: int, noun: str) -> str:
    if n == 1:
        return f"{n} {noun}"
    return f"{n} {_PLURALS.get(noun, noun + 's')}"


def emit(text: str) -> None:
    print(text, flush=True)


def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    p = argparse.ArgumentParser(
        description="Report the interesting lines of a live manual playtest as they happen."
    )
    p.add_argument("--log", type=Path, help="session log (default: whatever the session names)")
    p.add_argument("--session", type=Path, default=SESSION_FILE, help="session state file")
    p.add_argument("--every", type=float, default=DEFAULT_EVERY_S, help="seconds between check-ins")
    p.add_argument(
        "--wait",
        type=float,
        default=120.0,
        help="seconds to wait for a session to start before giving up",
    )
    return p.parse_args(argv)


def main(argv: list[str] | None = None) -> int:
    args = parse_args(argv)
    session = Session(args.session)

    deadline = time.monotonic() + args.wait
    while not session.alive() and time.monotonic() < deadline:
        time.sleep(POLL_S)
    if not session.alive() and args.log is None:
        emit(
            "No playtest session is running (start one with: cargo run --release -p freight-fate --bin freightfate -- --playtest-sandbox --launch)."
        )
        return 1

    log_path = args.log or session.log or DEFAULT_LOG
    emit(f"Watching {log_path.name}; check-in every {_duration(args.every)}.")

    tail = Tail(log_path)
    watcher = Watcher(args.every)
    next_checkin = time.monotonic() + args.every
    ended_at: float | None = None

    while True:
        for raw in tail.lines():
            for note in watcher.consume(raw):
                emit(note)

        now = time.monotonic()
        if now >= next_checkin:
            note = watcher.checkin()
            if note:
                emit(note)
            next_checkin = now + args.every

        if ended_at is None:
            if not session.alive():
                ended_at = now
        elif now - ended_at >= DRAIN_S:
            break

        time.sleep(POLL_S)

    for raw in tail.lines():
        for note in watcher.consume(raw):
            emit(note)
    for line in watcher.summary(session.sandbox):
        emit(line)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
