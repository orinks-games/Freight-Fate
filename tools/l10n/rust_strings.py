"""Find the player-facing string literals in the game's Rust source.

The game builds its spoken lines with ``format!`` and string literals
scattered across ``crates/``. Rather than wrapping every call site in a
translation macro, the game translates each finished line at the speech
boundary (``ff_core::l10n``), matching it against the templates this module
extracts. So extraction only has to answer one question per literal: could
this text reach the player's ears?

It errs toward including a literal. A template nobody ever hears costs a
translator one line; a spoken line that was filtered out stays English.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path

# Macros whose literals are never spoken: logging, assertions, panics.
SKIP_MACROS = {
    "log",
    "info",
    "warn",
    "error",
    "debug",
    "trace",
    "transcript",
    "panic",
    "assert",
    "assert_eq",
    "assert_ne",
    "debug_assert",
    "debug_assert_eq",
    "debug_assert_ne",
    "unreachable",
    "todo",
    "unimplemented",
    "eprintln",
    "eprint",
    "println",
    "print",
    "include_str",
    "include_bytes",
    "env",
    "option_env",
    "concat",
    "matches",
    "cfg",
    "compile_error",
}

# Functions and methods whose literal arguments are never spoken.
SKIP_CALLS = {
    "expect",
    "expect_err",
    "context",
    "with_context",
    "var",
    "var_os",
    "set_var",
    "remove_var",
    "join",
    "with_extension",
    "get",
    "get_mut",
    "contains_key",
    "remove",
    "starts_with",
    "ends_with",
    "strip_prefix",
    "strip_suffix",
    "split",
    "splitn",
    "rsplit",
    "split_once",
    "rsplit_once",
    "find",
    "rfind",
    "trim_matches",
    "trim_start_matches",
    "trim_end_matches",
    "contains",
    "Path",
    "PathBuf",
    "open",
    "create",
    "Command",
    "arg",
    "args",
    "header",
    "query",
    "pointer",
    "target",
    # SDL hints (sdl2::hint::set) are configuration values, never spoken.
    "set",
}

FORMAT_MACROS = {"format", "write", "writeln", "format_args"}

# Path fragments whose files hold no player-facing text.
SKIP_PATH_PARTS = (
    "/tests/",
    "/benches/",
    "/bin/",
    "/agent_server",
    "/playtest",
    "/tests.rs",
    "_tests.rs",
    "jaws_script",
    # The visual display draws text in the window; nothing in it is spoken.
    "/visuals",
    "/build.rs",
    "/l10n",
    # Place names, song titles and radio clip names stay as they are.
    "/legacy_aliases.rs",
    "/real_traffic",
    "/music/",
    "/radio_content.rs",
    "/world_parsing",
    # Plumbing: integrity checks, the network clients, the settings file.
    "profile_integrity_invariants",
    "profile_invariants",
    "cloud_save_integrity",
    "/cloud_saves/api",
    "/settings/migrate",
    "/settings/paths",
    "/net.rs",
    "/net/",
    "/updater",
    "/online_presence",
    "discord_presence",
    "single_instance",
    "/app/logging",
    "/app/boot_timing",
)

IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
PLACEHOLDER_RE = re.compile(r"\{\{|\}\}|\{([^{}]*)\}")


@dataclass
class Literal:
    text: str
    line: int
    callee_stack: list[str]
    format_args: list[str] = field(default_factory=list)
    is_format: bool = False
    # Compared or matched against (`== "x"`, `"x" =>`): a key, not words.
    is_key: bool = False
    # A struct field set straight from it (`kind: "browse".to_string()`).
    is_field_value: bool = False


@dataclass
class Template:
    msgid: str
    refs: list[str]
    placeholders: list[str]
    # Where in its first file the line appears, for ordering only.
    line: int = 0


class _Lexer:
    """Just enough of a Rust lexer to find literals and bracket nesting."""

    def __init__(self, src: str):
        self.src = src
        self.i = 0
        self.line = 1
        self.tokens: list[tuple[str, str, int]] = []

    def run(self) -> list[tuple[str, str, int]]:
        src = self.src
        n = len(src)
        while self.i < n:
            c = src[self.i]
            if c == "\n":
                self.line += 1
                self.i += 1
            elif c.isspace():
                self.i += 1
            elif src.startswith("//", self.i):
                end = src.find("\n", self.i)
                self.i = n if end < 0 else end
            elif src.startswith("/*", self.i):
                self._block_comment()
            elif c == '"':
                self._string(self.i + 1, raw_hashes=None, kind="str")
            elif c in "rb" and self._raw_or_byte():
                pass
            elif c == "'":
                self._char_or_lifetime()
            elif c.isalpha() or c == "_":
                m = IDENT_RE.match(src, self.i)
                self.tokens.append(("ident", m.group(0), self.line))
                self.i = m.end()
            elif c.isdigit():
                m = re.compile(r"[0-9][0-9A-Za-z_.]*").match(src, self.i)
                text = m.group(0)
                # "1..3" is a range, not a float.
                if ".." in text:
                    text = text.split("..")[0]
                self.tokens.append(("num", text, self.line))
                self.i += len(text)
            else:
                self.tokens.append(("punct", c, self.line))
                self.i += 1
        return self.tokens

    def _block_comment(self) -> None:
        depth = 0
        src = self.src
        while self.i < len(src):
            if src.startswith("/*", self.i):
                depth += 1
                self.i += 2
            elif src.startswith("*/", self.i):
                depth -= 1
                self.i += 2
                if depth == 0:
                    return
            else:
                if src[self.i] == "\n":
                    self.line += 1
                self.i += 1

    def _raw_or_byte(self) -> bool:
        src = self.src
        m = re.compile(r'(b?)r(#*)"').match(src, self.i)
        if m:
            kind = "bytes" if m.group(1) else "str"
            self._string(m.end(), raw_hashes=m.group(2), kind=kind)
            return True
        if src.startswith('b"', self.i):
            self._string(self.i + 2, raw_hashes=None, kind="bytes")
            return True
        if src.startswith("b'", self.i):
            self.i += 1
            self._char_or_lifetime()
            return True
        return False

    def _string(self, start: int, raw_hashes: str | None, kind: str) -> None:
        src = self.src
        line = self.line
        if raw_hashes is not None:
            close = '"' + raw_hashes
            end = src.find(close, start)
            body = src[start:end]
            self.i = end + len(close)
        else:
            j = start
            out = []
            while src[j] != '"':
                if src[j] == "\\":
                    nxt = src[j + 1]
                    if nxt == "\n":
                        # Line continuation: skip the newline and leading space.
                        j += 2
                        while src[j] in " \t\n\r":
                            j += 1
                        continue
                    if nxt == "u":
                        close = src.index("}", j)
                        out.append(chr(int(src[j + 3 : close], 16)))
                        j = close + 1
                        continue
                    if nxt == "x":
                        out.append(chr(int(src[j + 2 : j + 4], 16)))
                        j += 4
                        continue
                    out.append({"n": "\n", "t": "\t", "r": "\r", "0": "\0"}.get(nxt, nxt))
                    j += 2
                    continue
                out.append(src[j])
                j += 1
            body = "".join(out)
            self.i = j + 1
        self.line += src.count("\n", start, self.i)
        self.tokens.append((kind, body, line))

    def _char_or_lifetime(self) -> None:
        src = self.src
        m = re.compile(r"'(\\u\{[0-9a-fA-F]+\}|\\.|[^\\'])'").match(src, self.i)
        if m:
            self.tokens.append(("char", m.group(1), self.line))
            self.i = m.end()
        else:
            m = IDENT_RE.match(src, self.i + 1)
            self.tokens.append(("lifetime", m.group(0) if m else "", self.line))
            self.i = m.end() if m else self.i + 1


def _strip_test_modules(tokens: list[tuple[str, str, int]]) -> list[tuple[str, str, int]]:
    """Drop ``#[cfg(test)]`` items (modules and functions) and ``#[test]`` fns."""
    out: list[tuple[str, str, int]] = []
    i = 0
    n = len(tokens)
    while i < n:
        tok = tokens[i]
        if tok == ("punct", "#", tok[2]) and i + 1 < n and tokens[i + 1][1] == "[":
            # Collect the attribute body.
            j = i + 2
            depth = 1
            body = []
            while j < n and depth:
                if tokens[j][1] == "[":
                    depth += 1
                elif tokens[j][1] == "]":
                    depth -= 1
                if depth:
                    body.append(tokens[j][1])
                j += 1
            attr = "".join(body)
            if attr in ("cfg(test)", "test") or attr.startswith("cfg(all(test"):
                # Skip to the end of the following item: a `;` at depth 0 or
                # a balanced `{...}` block.
                k = j
                while k < n and tokens[k][1] not in ("{", ";"):
                    k += 1
                if k < n and tokens[k][1] == "{":
                    depth = 0
                    while k < n:
                        if tokens[k][1] == "{":
                            depth += 1
                        elif tokens[k][1] == "}":
                            depth -= 1
                            if depth == 0:
                                break
                        k += 1
                i = k + 1
                continue
        out.append(tok)
        i += 1
    return out


#: The hole an English plural ending ("s", "es") fills; a translation may
#: leave it out.
PLURAL_ENDING = "s_ending"
ENGLISH_ENDINGS = {"", "s", "es", "S", "ES"}


def _arg_name(expr: list[tuple[str, str, int]]) -> str:
    """A readable placeholder name for one ``format!`` argument expression."""
    # Drop a trailing method chain that only converts: .to_string(), .round().
    noise = {
        "to_string",
        "clone",
        "as_str",
        "round",
        "abs",
        "max",
        "min",
        "floor",
        "ceil",
        "trunc",
        "to_lowercase",
        "to_uppercase",
        "trim",
        "unwrap_or",
        "unwrap_or_default",
        "unwrap",
        "as_ref",
        "display",
        "len",
        "into",
        "deref",
        "borrow",
        "to_owned",
    }
    idents = [t for t in expr if t[0] == "ident"]
    # `if n == 1 { "" } else { "s" }`: the English plural ending, which a
    # translation leaves out and replaces with a {count:form|forms} choice.
    strings = [t[1] for t in expr if t[0] == "str"]
    if any(t[1] == "if" for t in idents) and strings and set(strings) <= ENGLISH_ENDINGS:
        return PLURAL_ENDING
    if not idents:
        return "value"
    # Walk back over call groups: `fmt_miles(d)` names the function.
    depth = 0
    names: list[str] = []
    for kind, text, _ in reversed(expr):
        if text == ")":
            depth += 1
        elif text == "(":
            depth -= 1
        elif kind == "ident" and depth == 0:
            names.append(text)
    for name in names:
        if name in noise or name in ("as", "f64", "f32", "u32", "i64", "u64", "usize", "i32"):
            continue
        if name in ("self", "ctx", "trip", "s", "x"):
            continue
        for prefix in ("fmt_", "format_", "spoken_", "speak_", "say_", "render_"):
            if name.startswith(prefix) and len(name) > len(prefix):
                name = name[len(prefix) :]
        return name
    return idents[-1][1]


def _literal_args(tokens, start: int) -> tuple[list[list[tuple[str, str, int]]], int]:
    """Split the comma-separated arguments after a format string literal."""
    args: list[list[tuple[str, str, int]]] = []
    depth = 0
    cur: list[tuple[str, str, int]] = []
    i = start
    while i < len(tokens):
        text = tokens[i][1]
        kind = tokens[i][0]
        if kind == "punct" and text in "([{":
            depth += 1
        elif kind == "punct" and text in ")]}":
            if depth == 0:
                break
            depth -= 1
        if kind == "punct" and text == "," and depth == 0:
            if cur:
                args.append(cur)
            cur = []
        else:
            cur.append(tokens[i])
        i += 1
    if cur:
        args.append(cur)
    return args, i


def literals_in(src: str) -> list[Literal]:
    tokens = _strip_test_modules(_Lexer(src).run())
    found: list[Literal] = []
    stack: list[str] = []
    attr_depth = 0
    i = 0
    n = len(tokens)
    while i < n:
        kind, text, line = tokens[i]
        if kind == "punct" and text in "([{":
            callee = ""
            if i >= 1 and tokens[i - 1][1] == "!" and i >= 2 and tokens[i - 2][0] == "ident":
                callee = tokens[i - 2][1] + "!"
            elif i >= 1 and tokens[i - 1][0] == "ident":
                callee = tokens[i - 1][1]
            elif text == "[" and (
                (i >= 1 and tokens[i - 1][1] == "#")
                or (i >= 2 and tokens[i - 1][1] == "!" and tokens[i - 2][1] == "#")
            ):
                callee = "#attr"
            # `log::warn!(...)` -> callee "warn!".
            stack.append(callee)
            if callee == "#attr":
                attr_depth += 1
        elif kind == "punct" and text in ")]}":
            if stack and stack.pop() == "#attr":
                attr_depth -= 1
        elif kind == "str":
            if attr_depth:
                i += 1
                continue
            lit = Literal(text=text, line=line, callee_stack=list(stack))
            before = "".join(t[1] for t in tokens[max(0, i - 2) : i])
            after = "".join(t[1] for t in tokens[i + 1 : i + 3])
            lit.is_key = (
                before.endswith(("==", "!="))
                or after.startswith(("=>", "|", "==", "!="))
                or (before.endswith("|") and not before.endswith("||"))
            )
            lit.is_field_value = (
                i >= 2
                and tokens[i - 1][1] == ":"
                and tokens[i - 2][0] == "ident"
                and (i < 3 or tokens[i - 3][1] != ":")
            )
            if stack and stack[-1].rstrip("!") in FORMAT_MACROS and stack[-1].endswith("!"):
                # The format string is the first literal directly after the
                # opening paren (format!) or after the writer arg (write!).
                prev = tokens[i - 1][1]
                if prev == "(" or (prev == "," and stack[-1] in ("write!", "writeln!")):
                    lit.is_format = True
                    j = i + 1
                    if j < n and tokens[j][1] == ",":
                        args, _ = _literal_args(tokens, j + 1)
                        lit.format_args = [_arg_name(a) for a in args]
            found.append(lit)
        i += 1
    return found


def _skipped(lit: Literal) -> bool:
    for callee in lit.callee_stack:
        bare = callee.rstrip("!")
        if callee.endswith("!") and bare in SKIP_MACROS:
            return True
        if not callee.endswith("!") and bare in SKIP_CALLS and callee == lit.callee_stack[-1]:
            return True
    return False


_IDENTIFIERISH = re.compile(r"^[a-z0-9_\-./:]+$|^[A-Z0-9_]+$")
_PATHISH = re.compile(
    r"(\.(json|wav|ogg|opus|txt|toml|dll|so|dylib|exe|png|log|md|ffdata|pak|mp3)\b)|://|\\\\|^/|::"
)


def is_player_text(text: str, is_format: bool, is_key: bool = False) -> bool:
    """Whether a literal reads as words a player could hear."""
    stripped = PLACEHOLDER_RE.sub("", text).strip()
    if not re.search(r"[A-Za-z]{2}", stripped):
        return False
    if _PATHISH.search(text):
        return False
    # Multi-line literals are help text, kept; JSON or code blobs are not.
    if "\n" in text and not is_format and text.lstrip().startswith(("{", "[", "<")):
        return False
    if " " not in stripped:
        # A single word: keep Capitalized words ("Off", "Settings"), drop
        # identifiers and keys ("driving", "fuel_stop", "SAPI").
        if stripped in ("True", "False", "None"):
            return False
        if re.match(r"^[A-Z][a-z]+[.!?:,]?$|^[A-Za-z][a-z]+(-[A-Za-z]+)+$", stripped):
            return True
        # A lowercase word is a value spoken in a sentence ("off", "sparse")
        # unless the code only compares or matches it.
        # A lowercase word leading into a placeholder ("automatic, {name}")
        # is a phrase a value is spoken inside.
        if is_format and re.match(
            r"^[a-z]{3,}, \{[A-Za-z_][A-Za-z0-9_]*(:[^}]*)?\}$", text.strip()
        ):
            return True
        # A label leading into a value ("ELD: {summary}").
        if is_format and re.match(r"^[A-Z][A-Za-z]+: \{[A-Za-z_][A-Za-z0-9_]*\}$", text.strip()):
            return True
        # A key and what it reads ("{fuel} fuel.", "{horn} horn."): the
        # help's one-word sentences.
        if is_format and re.match(r"^\{[A-Za-z_][A-Za-z0-9_]*\} [a-z]{2,}\.$", text.strip()):
            return True
        if is_key or not re.match(r"^[a-z]{2,}$", stripped):
            return False
    return True


HELP_TOKEN_RE = re.compile(r"\{\{(?:(pad|key):)?([a-z_][a-z0-9_]*)\}\}")
HOLE_MARK = "\x00"


def _help_tokens(text: str, names: list[str]) -> str:
    """``{{engine}}`` and ``{{pad:horn}}``: the help's control names, filled
    in before the line is spoken, so each is a hole too."""

    def hole(m: re.Match) -> str:
        device, action = m.group(1), m.group(2)
        name = f"{device or 'control'}_{action}"
        if name not in names:
            names.append(name)
        return HOLE_MARK + name + HOLE_MARK

    return HELP_TOKEN_RE.sub(hole, text)


def _marks_to_holes(text: str) -> str:
    return re.sub(HOLE_MARK + "([^" + HOLE_MARK + "]*)" + HOLE_MARK, r"{\1}", text)


def to_template(lit: Literal) -> tuple[str, list[str]]:
    """The catalog form of a literal: ``{name}`` placeholders, no format specs."""
    names: list[str] = []
    if not lit.is_format:
        return _marks_to_holes(_help_tokens(lit.text, names)), names
    used: dict[str, int] = {}
    positional = iter(range(len(lit.format_args) + 64))

    def name_for(spec: str) -> str:
        key = spec.split(":", 1)[0].strip()
        if key == "":
            idx = next(positional)
            key = lit.format_args[idx] if idx < len(lit.format_args) else f"value{idx + 1}"
        elif key.isdigit():
            idx = int(key)
            key = lit.format_args[idx] if idx < len(lit.format_args) else f"value{idx + 1}"
        key = key.split(".")[-1]
        return key

    out = []
    pos = 0
    seen_specs: dict[str, str] = {}
    for m in PLACEHOLDER_RE.finditer(lit.text):
        out.append(lit.text[pos : m.start()])
        token = m.group(0)
        if token == "{{":
            out.append("{")
        elif token == "}}":
            out.append("}")
        else:
            spec = m.group(1)
            raw_key = spec.split(":", 1)[0].strip()
            if raw_key and not raw_key.isdigit() and raw_key in seen_specs:
                name = seen_specs[raw_key]
            else:
                base = name_for(spec)
                count = used.get(base, 0)
                used[base] = count + 1
                name = base if count == 0 else f"{base}{count + 1}"
                if raw_key and not raw_key.isdigit():
                    seen_specs[raw_key] = name
                names.append(name)
            out.append(HOLE_MARK + name + HOLE_MARK)
        pos = m.end()
    out.append(lit.text[pos:])
    return _marks_to_holes(_help_tokens("".join(out), names)), names


def extract(root: Path, crates: list[str]) -> dict[str, Template]:
    templates: dict[str, Template] = {}
    for crate in crates:
        for path in sorted((root / "crates" / crate / "src").rglob("*.rs")):
            rel = path.relative_to(root).as_posix()
            if any(part in "/" + rel for part in SKIP_PATH_PARTS):
                continue
            src = path.read_text(encoding="utf-8")
            lits = literals_in(src)
            # A lowercase word the file compares against (`kind == "browse"`)
            # is a key where a struct field is set to it, too.
            keys = {lit.text for lit in lits if lit.is_key and re.match(r"^[a-z]{2,}$", lit.text)}
            for lit in lits:
                if lit.is_field_value and lit.text in keys:
                    continue
                if _skipped(lit) or not is_player_text(lit.text, lit.is_format, lit.is_key):
                    continue
                msgid, names = to_template(lit)
                msgid = msgid.strip("\n")
                if not msgid.strip():
                    continue
                # The file, not the line: a reference that moved with every
                # edit would make the template stale on unrelated changes.
                if msgid in templates:
                    if rel not in templates[msgid].refs:
                        templates[msgid].refs.append(rel)
                else:
                    templates[msgid] = Template(
                        msgid=msgid, refs=[rel], placeholders=names, line=lit.line
                    )
    return templates
