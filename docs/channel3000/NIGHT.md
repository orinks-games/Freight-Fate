# Channel 3000 at night

The night (`prime`, `late` and `overnight`, see `STATION.md`) is 60 real
minutes a game night at 10x, and has about 80 minutes of programming, most of
it After Hours. Prime has one evening's worth, overnight not quite one. The
aim is two game nights without a repeat. This brief is for the new night
shows. `STATION.md`'s rules all apply, and `DAYTIME.md`'s "made for
listening while driving" section applies here too; read both first.

## What night is

Each daypart has its own colour (from `STATION.md`):
- `prime`: showbiz, the big shows, everyone on their best behaviour.
- `late`: hushed, a hummed chord, Abe's drone, a whisper.
- `overnight`: slow, a music box, a lullaby. Keeping a driver company at
  three in the morning: gentle and warm, never sleepy-making, never sad
  without a turn toward comfort.

Every show is a spoof of a kind of television, with its own new theme tune
and its own cast arrangement. Scenes are spoken over a quiet sung bed; the
music is the theme, stings and sung numbers. Each episode is 4 to 7 minutes,
complete in itself, can play in any order, and opens and closes on its theme.
Times of day may be named only where they fit the daypart ("tonight" in
prime and late; nothing later than "the small hours" overnight).

## The new shows

| file | show | the TV it spoofs | daypart | starting point |
|---|---|---|---|---|
| `night_late_picture_show_01`, `_02` | The Late, Late Picture Show | the overnight old-movie slot | overnight | A movie host introduces a black-and-white picture and keeps nodding off during it. We hear the film: clipped old-movie dialogue over a sung "orchestra". Episode 1, a romance (two drivers who only ever meet at one all-night diner); episode 2, a western on wheels. The host wakes for the ending. |
| `night_stories_01`, `_02` | Stories for the Ones at Home | a bedtime-story programme | overnight | A warm storyteller reads a bedtime story for the children of drivers, so the drivers listening can hear what their kids hear. A music box, a sung lullaby to close. Gentle, not sleepy: the driver is awake and driving. |
| `night_mile_marker_general_01` | Mile Marker General | a prime-time hospital drama | prime | A hospital beside the interstate where every patient is a driver. Slow-motion crises that turn out small ("We're losing him!" He needs a nap), a love triangle among the doctors, a sung montage. |
| `night_the_big_time_01` | The Big Time | a prime-time talent show | prime | Contestants from the road sing for the judges and the phone vote: a showcase for the singers. Three acts, three judges, a winner. |
| `night_up_all_night_01` | Up All Night | a late-night talk show | late | The host's monologue to the drivers, a singing house band, a guest, a quiet closing number. Hushed, dry, kind. |

| `night_starhauler_01`, `_02` | Starhauler | a 1960s TV space adventure | prime | A space freighter and its small crew make one delivery an episode: in episode 1, two thousand tons of Venusian cola to the colony on Saturn's outer rings (a listener's idea). The road, the log book, the dispatcher and the truck stop, in space. Theme in C-sharp minor, 4/4, about 104, a fanfare with a sung theremin. |

Each writer chooses the key, meter and tempo, but every theme and sung
number must differ from each other's, from the daytime shows' (`DAYTIME.md`
and the `day_*` headers) and from After Hours' (`sketch-show-2/SHOW.md`).
Character names must be new on the station: check the other shows' casts.
DECtalk Val speaks at Paul's pitch; don't cast her as a woman who talks
(`VOICES.md`).

## What each writer hands back

`channel-3000-radio/<file>.song`, its rendered `.wav`, a `.transcript.json`
(as `DAYTIME.md` describes) and a 192 kbps MP3 normalised to -16 LUFS. The
header says what it is, who plays whom, its daypart and which music is new.
Run `scan.py` and `mixcheck.py` (from `STATION.md`) and fix what they show.
