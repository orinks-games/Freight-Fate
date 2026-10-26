# Channel 3000 daytime

The `day` daypart (5:00 to 19:00 local, see `STATION.md`) needs about three
hours of programming, and had about a quarter of an hour (the two
Saturday-morning cartoons and Mister Sam's Corner; Phoneme's Flying Circus
is framed as an evening and airs in prime, with three of its sketches as
shorts between programmes). This brief is for
the new daytime shows. `STATION.md`'s rules all apply; read it first.

## What daytime is

Daytime TV, heard from a truck cab. Bright and busy in the morning,
comfortable in the afternoon. It's the gentlest part of Channel 3000, with
less of the dark and sad than late night, but the same eye for today's
things: apps, ratings, subscriptions, gig work, being alone on the road,
machines that mean well. The jokes come from the shows taking themselves
seriously.

Every show is a spoof of a kind of daytime television, with its own new theme
tune, and a new cast arrangement. Like After Hours, scenes are spoken over a
quiet sung bed, and the music is the theme, stings and sung numbers.

## Made for listening while driving

- It works as audio alone. When something happens, someone says so in the
  scene, the way radio drama does. Never "look at this".
- Characters name each other often enough that a listener who tunes in
  halfway knows who's talking.
- Each episode is complete in itself, 4 to 7 minutes long, and opens and
  closes on its theme, so the station can drop it in anywhere in the day.
- Truckers are listening: the road, the cab, the load, the truck stop, the
  dispatcher, the log book. Affectionate, never mocking.
- No dates, no weather, and no real places, brands, shows or people. No
  times of day except "morning" in a morning show.

## The first four shows

| file | show | the TV it spoofs | starting point |
|---|---|---|---|
| `day_rise_and_grind_01.song` | Rise and Grind | a morning show | Skip (DECtalk Dennis) and Joy (Microsoft Mary) host, far too cheerful. A cooking bit, the SSI-263 reading the horoscopes ("Taurus: you will be in a truck"), a guest. New bouncy theme. |
| `day_as_the_wheels_turn_01.song` | As the Wheels Turn | a daytime soap | Episode 1 of a serial at a family-run truck stop: a long-lost twin, a secret in the walk-in freezer, an organ sting on every revelation. DECtalk Ursula is the grand matriarch. New soap theme. |
| `day_weigh_in_01.song` | Weigh In! | a game show | At a weigh station: two truckers guess their own loads' weights, with lights, a buzzer, a big wheel and a studio audience. The host is not Desmond (he hosts too much); try Robert or Microsoft Sam. New game-show theme. |
| `day_highway_helpline_01.song` | Highway Helpline | a call-in advice show | DECtalk Harry, deep and droll, takes calls from drivers: a GPS that's jealous, a co-driver who hums, a dog who's better at backing up than the driver. Soft-jazz bed, a call-in jingle. |

Each show has three episodes (`_01` to `_03`). The station plays them in any
order, so every episode stands alone: As the Wheels Turn opens each with a
"Previously on" recap of the standing situation and ends on a cliffhanger it
never follows up. Character names stay unique across the station (Earl, the
soap's regular and a Helpline caller, is the one shared man on purpose).

## The next three shows

Day has about 108 minutes; these add about 55, three standalone episodes
each, for two game days without a repeat.

| file | show | the TV it spoofs | theme | starting point |
|---|---|---|---|---|
| `day_traffic_court_0N` | Traffic Court | a daytime courtroom show | C minor, 3/4, about 108, a stately minuet | A judge with no patience and a soft heart hears small disputes between drivers: who had the parking spot, a borrowed CB never returned, custody of the truck-stop dog. A bailiff, a sworn-in witness, a verdict, a gavel. |
| `day_wild_highways_0N` | Wild Highways | a nature documentary | G major, 6/8, about 54, sweeping | A hushed naturalist observes the wildlife of the road as if on safari: the parking-lot crows, drivers at the fuel island, the great migration of the motor homes, the night-shift waitress at the watering hole. Wonder, not mockery. |
| `day_this_old_truck_0N` | This Old Truck | a restoration show | A major, 2/4, about 104, folksy, with vocal percussion | Two mechanics restore one forty-year-old truck, a part each episode, explaining every step to the home viewer; a sung "time-lapse"; the owner's reveal. |

Each writer chooses the key, meter and tempo, but the four must differ from
each other and from the After Hours segments' (`sketch-show-2/SHOW.md`).
Robert and Sarah lead the sung numbers; Tracy and Webster only harmonise.
Nobody talks in a VocalWriter voice: spoken parts are DECtalk or Microsoft,
the SSI-263 for gags (see `STATION.md`, Who's who).

## What each writer hands back

`channel-3000-radio/<file>.song` and its rendered `.wav`. The header says
what it is, who plays whom, its daypart (`day`), and which music is new.
Run `scan.py` and `mixcheck.py` (from `STATION.md`) and fix what they show.
Then write the transcript, in the same format as the After Hours site's
(`C:\Users\joshu\orinks-net-ch3000\lib\after-hours-transcripts.ts`), as a JSON
array of strings in `channel-3000-radio/<file>.transcript.json`. Draft it
with `python sketch-show-2/tools/transcript.py FILE` and clean it up.
