# Translating Freight Fate

Everything the game speaks can be translated: menus, help, driving calls,
achievements, billboards. The radio, its station IDs and other recordings
stay in English.

The first translations were written by Claude, an AI, so they will have
mistakes a fluent player catches in seconds. Corrections are welcome from
anyone who plays in one of these languages. You do not need to know how to
program.

| File | Language |
| --- | --- |
| `es.po` | Spanish |
| `fr.po` | French |
| `de.po` | German |
| `pt_BR.po` | Brazilian Portuguese |
| `it.po` | Italian |
| `ru.po` | Russian |
| `pl.po` | Polish |
| `zh_CN.po` | Simplified Chinese |
| `ja.po` | Japanese |
| `tr.po` | Turkish |
| `vi.po` | Vietnamese |

To play in one of them, open Settings, then Speech; Language is the first
row. Left and Right change it. Automatic follows your system language.

## Report a wrong line (no GitHub skills needed)

Open a [translation correction](https://github.com/orinks-games/freight-fate/issues/new?template=translation.yml)
issue. It asks for the language, what the game said, and what it should
say. Message review (the comma key while driving) replays what the game
said, and your screen reader's speech history has the exact words. A maintainer
puts the fix into the file for you.

## Fix it yourself

Each language is one text file in this folder, in the gettext PO format.
A translation editor such as [Poedit](https://poedit.net) opens it, and so
does any plain text editor. Each entry is the English line, then its translation:

```po
msgid "Truck damage is {damage_pct} percent."
msgstr "El camión tiene un {damage_pct} por ciento de daños."
```

Change the `msgstr` line and leave the `msgid` line exactly as it is; the
game finds a line by its English. Then either open a pull request against
the `dev` branch, or attach the file to a translation correction issue.

### Hear your change before sending it

Put your edited file in a folder named `lang` inside the game's `saves`
folder (on a Mac, in `~/Library/Application Support/FreightFate`), so the
path ends in `saves/lang/es.po`, then restart the game. A file there
replaces the built-in translation for that language. Delete it to go back
to the built-in one.

To list every line the game spoke in English because no translation
matched, set the environment variable `FREIGHT_FATE_L10N_MISSES` to a file
path before starting the game. Each untranslated line is written there
once.

## Start a new language

`freight-fate.pot` in this folder is the template: every line the game can
speak, untranslated. To add a language the game does not have yet:

1. In Poedit, choose "Create new translation" and open
   `freight-fate.pot`, then pick your language. Any other PO editor, or a
   copy of the file renamed by hand, works too.
2. Save it here as `<code>.po`, where the code is your language's, such as
   `nl.po` for Dutch or `pt_PT.po` for European Portuguese.
3. In the file's header, set `X-Language-Name` to your language's name in
   English and `X-Language-Autonym` to its name in your language, such as
   `X-Language-Name: Dutch` and `X-Language-Autonym: Nederlands`. Settings
   reads both: "Dutch (Nederlands)".

That is all the game needs: with the file in `saves/lang/` it appears in
Settings after a restart, and a pull request that adds it here builds it
into the game. Translate as much or as little as you like; every empty
entry is spoken in English. When the template gains new lines, they arrive
in your file untranslated.

A new language starts with two plural forms (one, and the rest) and
English number formatting, such as 1,250.5. If yours needs more forms or
other separators, say so in the pull request and a maintainer adds them.

## Rules for every line

1. **Placeholders stay whole.** A word in braces, like `{city}` or
   `{distance}`, is filled in by the game. Move it wherever your grammar
   needs it, but copy the name inside the braces exactly. A translation that
   names a placeholder the English line does not have is ignored. A
   placeholder starting with `the_`, like `{the_city}`, gets the bare name
   without the English "the".
2. **Numbers stay as written.** Every digit in the English line appears in
   the translation.
3. **Key names stay in English**: Enter, Escape, Space, Tab, Page Up, F1 and
   the rest. They are the names on the keyboard and in the game's key
   settings.
4. **Place names stay in English**: cities, highways, streets, companies.
5. **One word for each thing.** Use the term in your language's glossary,
   `glossary/<code>.md`, every time. The English terms and what they mean
   are in [glossary/en.md](glossary/en.md).
6. **Write for the ear.** A screen reader will say every word. Short,
   plain sentences; no symbols that are read out as their names; no text in
   parentheses that only makes sense on a screen.
7. **Leave an entry empty** if you are unsure: an empty `msgstr` makes the
   game say that line in English. Mark a guess you want checked with a
   `#, fuzzy` line above it; the game skips fuzzy entries too.

### Plurals

English picks "1 mile" or "2 miles" in code, so both forms usually appear
as separate lines. When your language needs a word chosen by a number, write
the forms after the placeholder's name, separated by `|`:

```po
msgid "{count} miles to go."
msgstr "Осталось {count} {count:миля|мили|миль}."
```

The forms are in this order:

- Spanish, Portuguese, Italian, German, Turkish: singular, plural.
- French: singular (0 and 1), plural.
- Russian: one (1, 21, 31), few (2 to 4, 22 to 24), many (5 to 20, 25).
- Polish: one (1 only), few (2 to 4, 22 to 24), many (the rest).
- Chinese, Japanese: no plurals; one form, or none at all.

`{s_ending}` is the English plural "s" and nothing else. Leave it out, and
choose the word by the number instead:

```po
msgid "{breaks} 30-minute break{s_ending}"
msgstr "{breaks} {breaks:pausa|pausas} de 30 minutos"
```

### Lines built from pieces

Some lines are assembled from smaller pieces at run time, so the whole
sentence is not in the file. The game translates each sentence it can
match whole. A sentence it cannot match whole is split into clauses only if
every clause (other than a name or a number) is in the file; otherwise the
sentence stays in English, so a player never hears one sentence in two
languages. Entries marked "one clause of a longer line" are those clauses.
If you hear a line that stays English, report it with the correction issue
and we will add it.

A menu row's position ("4 of 8.") is its own entry, `{position} of
{count}.`, spoken after whichever row has focus.

### Two entries with the same English

An entry with a `msgctxt` line is a second use of the same English: the
`controller button` "Back" is the name of the controller's Back button,
which stays as the controller prints it. The plain "Back" is the menu row
that leaves a screen; translate that one.

## For maintainers

- `lang/freight-fate.pot` is the template: every line the game can speak,
  with the source file and line it comes from. It is generated; do not edit
  it by hand.
- `lang/extra.pot` holds lines the source never spells out whole, added by
  hand. Each one is a template like the others.
- After changing spoken text in the code, run
  `cargo tools l10n-extract`. It rewrites the template and adds
  new lines to every language file, untranslated. CI fails when the files
  are out of date (`--check`).
- `cargo tools l10n-check` checks every language file:
  placeholders, numbers, key names, and broken entries. `--stats` prints
  how much of each language is translated.
- A correction that arrives as an issue goes into the language file as a
  normal commit to `dev`; credit the reporter in the changelog.
