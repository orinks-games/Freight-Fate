# Channel 3000 on the radio: the station brief

Channel 3000 is the television station from After Hours (`sketch-show-2/`).
In Freight Fate, the user's audio trucking game, it goes on the in-game radio
at **87.7 FM**. That's where an FM radio could pick up the sound of analog TV
channel 6, so a TV station on the radio dial has a real reason to be there.
The station plays the company's programmes (After Hours segments, Grimatonics,
Phoneme's Flying Circus, and new shows), and between them, the glue: idents,
the continuity announcer and commercials.

## How the station runs in the game

- **Dayparts, not clock slots.** The game runs at 10x by default (20x and 1x
  are settings), and slows or jumps its clock while you drive, park and sleep.
  A game hour is six real minutes at 10x, shorter than most programmes. So the
  schedule is four dayparts, read from the truck's LOCAL hour:

  | daypart | local hours | real minutes at 10x |
  |---|---|---|
  | `day` | 5:00 to 19:00 | 84 |
  | `prime` | 19:00 to 22:00 | 18 |
  | `late` | 22:00 to 2:00 | 24 |
  | `overnight` | 2:00 to 5:00 | 18 |

- **A programme is never cut off.** When one ends, the station checks the
  daypart. If it has changed, the next item is that daypart's opener;
  otherwise it draws the next programme from the daypart's pool.
- **Between programmes:** an ident, then a continuity line, one or two ads,
  or a short, sometimes two of them. The shorts are three Flying Circus
  sketches that stand alone and name no time of day: 04 The Lone Courier,
  06 Planet Office and 07 The Bold and the Rebooted (1.3 to 2.1 minutes,
  `sketch-show/mp3/`). They air in any daypart. Ads air only on Channel 3000.
- **Durations are exact.** The game advances its rotation on a table of
  durations, so every clip's length is measured from its final file.

## What plays when

Each daypart draws from its own pool. Its opener is the `host_channel3000`
line marked `opener: yes`.

| daypart | pool |
|---|---|
| `day` | the daytime shows (`day_*`, see `DAYTIME.md`); After Hours 04 Splat Patrol!, 05 Defenders of the Cloud, 06 Mister Sam's Corner |
| `prime` | After Hours 00 Sign-on, 01 Gig Street, 02 Puttin' on the Filters, 08 Incompatible, 09 News at Ten; Phoneme's Flying Circus, the whole 15-minute show as one programme (it is framed as an evening: "Good evening", "Tonight, on every channel at once", "Good night"); the new prime shows (`NIGHT.md`) |
| `late` | After Hours 07 The Last Payphone, 10 Are You Afraid of the Data?, 12 Starship Obsolete; Grimatonics (all but the three below); the new late show |
| `overnight` | After Hours 11 The Forever Plan, 13 Insomnia / Sign-off; Grimatonics' Brahms' Lullaby, Wee Willie Winkie and Row, Row, Row Your Boat; the new overnight shows |

After Hours and Phoneme's Flying Circus air as they were made, VocalWriter
voices talking and all. They were finished before the talking rule below,
and the user chose to keep them (2026-10-05). The rule is for new work.

## The glue pack, and its names

| kind | key | how many | length |
|---|---|---|---|
| ident | `id_channel3000_NN` | about 10 | 4 to 12 s |
| continuity line | `host_channel3000_NN` | about 20 | 5 to 20 s |
| commercial | `ad_c3k_<slug>` | 8 to 10 | 20 to 45 s |

Each clip is one `.song` file in this folder, named by its key
(`id_channel3000_01.song`). Its header says what it is, who plays whom, its
dayparts (`dayparts: any` or a list) and, for music, where the tune comes from.

## The ident motif

Every ident sings the station's name on the motif from the After Hours
sign-on (`sketch-show-2/00-sign-on.song`, the Set, Tracy, Andy and Webster
tracks at bar 2): **"Chan-nel Three Thou-sand"** in the rhythm eighth, eighth,
quarter, eighth, dotted quarter. Its top voice moves from the third to the
fifth of the chord (in G: B B D D D, over G B D). "Thousand" is sung with
phonemes, `[TH+AW] [z+EN+d]`. Keep that rhythm and that rising shape;
everything else changes from ident to ident: key, voices, texture, tempo, what
comes before or after it. Some idents add the frequency
("eighty-seven seven") or a line from the announcer.

Each daypart wants its own colour:
- `day`: bright, quick, a sung band.
- `prime`: ragtime, showbiz, the sign-on's world.
- `late`: hushed, a hummed chord, Abe's drone, a whisper.
- `overnight`: slow, a music box, a lullaby.

## Who's who

- **Talking is DECtalk and Microsoft; VocalWriter only sings.** The user
  found that VocalWriter's singers, made to sing, still want to sing when
  they talk. So every spoken part is a DECtalk or Microsoft voice, shared
  between the two engines, and the SSI-263 talks only for a gag (a machine,
  a robot, a joke). VocalWriter voices sing: songs, beds, stings, and idents
  that sing. A character who talks and sings speaks in a DECtalk or
  Microsoft voice on one track and sings in a VocalWriter voice on another.
- **The continuity announcers take turns.** The user wanted more than Mike:
  Microsoft Mike, Mary and Sam (in Hall), DECtalk Paul, Betty, Harry,
  Ursula, Rita, Frank and Dennis, each matched to the line's daypart and
  mood, spoken (`say`) over a soft hummed bed or over nothing. They are warm,
  a little formal, and dry. The same goes for the idents that speak.
- Commercials: DECtalk Paul for pitchmen and fast small print, Betty and
  Ursula as voice-overs, Microsoft Sam, Mike and Mary and DECtalk Dennis,
  Harry and Frank for customers and drivers, the SSI-263 for machines,
  Robert and Sarah leading the jingles.
- The whole cast and what each voice measured as are in `VOICES.md` and
  `sketch-show-2/BIBLE.md`.

## The rules

House rules, from the user. Follow them exactly.

- **Every part is a singer.** The band is voices: hummed chords (`[UW]`),
  "dum" and "bom" sung basses, "pah" and "bah" brass, "doo" pads, vocal
  percussion. Instruments only as a rare colour or a bookend (`Bell`,
  `Crystal`, a `Marimba` music box).
- **Never** the broken voices (Miles, Ellen1, Kae1, Sonny1, Ed1) or the choir
  voices (Chorus Low, Chorus Mid, ChorusHi).
- **Reverb** `reverb 40,24` on the song. Effects may have a bigger room on
  their own track.
- **No hissing effects:** no `[SH]`, `[s]` or `[f]` sung as sounds, and no
  breath-noise wind. Whispered words are fine.
- **Sing leads plainly:** no `chorus` control, no `vibrato 0` and no octave
  jumps on a sung lead. Robert is the preferred lead, Sarah leads too, Tracy
  and Webster sing harmony only, and Abe's low drone is loved.
- **Harmony parts sit on the chord's notes.** Only the melody may use passing
  notes.
- **Lyrics fit the rhythm.** Stressed syllables on strong beats, no little
  word on a long or strong note, and the syllables counted against the notes
  exactly. Check every sung line with
  `python sketch-show-2/tools/scan.py FILE` and rewrite until it reads
  naturally.
- **No stated morals.** The user asked for an AI-writing pass over the
  station's scripts (the `avoid-ai-writing` skill). Its main finding was
  episodes ending on a stated moral or a greeting-card "somebody" line
  ("somebody's dreaming about you", "Somebody spotted for every one of us,
  once"), plus aphorisms ("that's the whole point", "Not for the coffee...
  because...") and the same "to everybody out there" sign-off everywhere.
  End on the story's own last beat or a concrete, practical line; keep each
  show's catchphrase. Read your spoken lines against that skill before
  handing over.
- **All words and music are new.** No real brands, companies, products,
  people or places. No words from any copyrighted song.

The game's own rules for anything spoken on its radio:
- **Plain road language,** and no key or menu names.
- **No dates and no weather.** Real weather runs in the game, so a forecast
  would contradict it.
- **No times of day in a clip tagged `any`.** "Tonight", "good morning" and
  "at this hour" go only in clips tagged with the dayparts they fit.
- **Truckers are listening.** Speak to someone driving: the road, the cab,
  the night, the coffee. Don't tell them to look at anything.

## Making and checking a clip

```
python songmaker.py channel-3000-radio/id_channel3000_01.song
python sketch-show-2/tools/scan.py channel-3000-radio/id_channel3000_01.song
python sketch-show-2/tools/mixcheck.py channel-3000-radio/id_channel3000_01.song 4
```

`songmaker.py` renders the `.wav` beside the song. The mix check's targets:
speech 5 to 7 dB above whatever plays under it, sung leads clearly above the
band, the master peak below -0.5 dBFS, and nothing clipping. Volumes run from
0 to 100 and can't go higher, so turn the band down to make room.

DECtalk's speech is mixed in dry after the render. A clip with only DECtalk
speech and no notes leaves svs.exe an empty song and the render fails, so
put a DECtalk line over a bed or give it to a Microsoft voice. In a scene,
a Microsoft voice beside a dry DECtalk one may want a small room of its own
(`reverb 15,8`) so they sound like the same studio.

Don't edit `songmaker.py`, `talk.py`, or any file but your own: others are
rendering at the same time.
