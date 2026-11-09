# The CB radio: text and voice between drivers (2.0)

Design for Freight Fate 2.0, written 2026-11-09 for the owner's review. The
decisions at the end are open until the owner answers them; nothing here is
built, and none of it ships in the 2026 Christmas alpha. The same design lives
in the Wicket's project as a Claude Doc
(https://claude.ai/artifact/BzeaaaXP5ot7dy754DrXo1); this file is the copy the
Freight Fate repository keeps, as it keeps `co-driver-design.md`.

## Summary

Freight Fate 2.0 gets a working CB radio: one public channel every driver on
duty can hear, direct messages between two drivers, and push-to-talk voice on
the same channel. The game already has a fictional CB (the squelch sound and
the trooper reports drivers pass along), so real drivers' calls land in the
same place with the same sound, and nothing new has to be learned to hear
them. Text comes first, because it can be screened, reviewed and read back
with the message review keys. Voice follows on the same channel once the
Hearken rooms and talk crates exist.

Recommendation: build text on the CB in the first 2.0 nightly after the
Christmas content ships, then push-to-talk voice on the public channel, then
one-to-one voice calls. Keep the relay on orinks.net (a Durable Object per
room on Cloudflare), not peer to peer, and hold voice off by default until a
player turns it on. Both chat and voice need an orinks.net account to send;
anyone can listen.

It depends on the Hearken peer-play design (Hearken PR #59, merged 2026-11-09,
with text chat as its PR 2 and voice as PR 3) and on the approved plan to
move Freight Fate accounts onto shared Hearken accounts, which waits until
after Christmas.

## What this builds on

Most of the pieces exist. The CB reuses them rather than adding a second set.

| Piece | Where it is today | What the CB takes from it |
| --- | --- | --- |
| Fictional CB | `events/cb_radio_chatter` sound, trooper reports in `ff-core/src/sim/trip_traffic.rs`, Alt C repeats the last call | The sound, the name, and the repeat key, now covering real drivers' calls too |
| Drivers on duty board | Presence heartbeats to orinks.net, `freightFatePresence`, the Online menu, the pause menu row | Who is on the channel, and the row a direct message starts from |
| Driver accounts | Device tokens checked by `acceptDriverToken` in orinks-net `convex/freightFate.ts`, about 200 drivers | Who may send; the display name every call carries |
| Name screening | orinks-net `convex/moderation.ts`, the obscenity matcher and force-rename | The same screen on every message, and the moderator tooling to extend |
| Message review | `ff-core/src/message_log.rs`, Comma and Period step, brackets filter by category | A CB category, so a missed call can be read back without leaving the wheel |
| Driving speech ladder | `SpeechCategory` dispositions per rung in `speech_pacing.rs`, Ambient priority in the pacer | A CB category that never gets ahead of a safety line |
| Text entry | `states/text_entry.rs`, 24 characters, review cursor | The typing field, with a longer cap for messages |
| Driver apps tablet | Tab, Driver apps, the Road chatter app | The Road chatter app becomes the CB screen |
| Duty watch | Background poll that says "Road Star is on duty" | The same worker shape: a thread owns the socket, the loop drains a queue |
| Hearken audio | `Feed` for live audio on a bus, `Bus::VOICE`, the cab speakers insert, libopus already compiled | Received voice plays as a feed on the voice bus with a CB tone |
| Hearken rooms and talk | `hearken-rooms` (rooms with an event log, merged in Hearken #59) and `hearken-talk` (mic capture and Opus, planned) | The whole transport, so Freight Fate writes screens and speech, not networking |
| orinks.net | Cloudflare Workers plus Convex, games platform under `lib/games`, owner pages such as `/games/testers` | Durable Objects for the live rooms, Convex for history and blocks, an owner page for moderation |

Two things do not exist anywhere yet: microphone capture, and a WebSocket
client in the game. Both land in the Hearken crates, not in Freight Fate.

## The fiction: a CB in the cab

The cab already has two radios. The cab radio plays music and the stations on
the dial. The CB carries talk: today the trooper reports other drivers pass
along, tomorrow the drivers themselves. Nothing is called "chat" anywhere in
the game. It is the CB, a call, a channel, and a driver's handle, which is
their orinks.net driver name.

- **Channel 19** is the one public channel, the way it is on real highways.
  Every driver on duty with the CB on hears it, wherever they are on the map.
  One channel keeps a small community together; a channel per region would
  be empty most nights.
- **A private channel** is a direct message between two drivers. You open one
  from a driver's row on the drivers board, the directory or their profile,
  or by answering a call that came to you.
- **The trooper reports stay.** They keep playing from the game's own traffic
  model whether anyone is online or not, so a driver on an empty night still
  has a working CB. A real driver's call and a report share the squelch
  sound; the words tell them apart, because a real call starts with the
  handle.
- **Calls carry a handle and, if the driver allows it, where they are.**
  "Road Star, westbound on I-80 near Cheyenne: chains required at Elk
  Mountain." The place is the same broad line the drivers board already
  shows, never finer. A driver who shares nothing on the board sends a bare
  handle.
- **Voice is push-to-talk**, one speaker at a time, with the squelch opening
  and closing around each transmission. That is how a CB works, and it is
  also the simplest thing to run and moderate.

What the CB is not: it is not a lobby, not a friends list, and not a way to
drive together. Drivers on Channel 19 are on their own hauls; the channel is
company on a long night and a place to warn each other.

## Text on the channel

Text is the first thing to build and the thing every player gets, voice or
not. A call is up to 200 characters, is screened before it goes out, and
arrives as speech through the same channel as the game's own road lines.

**Sending from the wheel.** The CB key opens a short menu over the drive: Say
something, Quick calls, Listen back, and Who is on the channel. Say something
opens the typing field; Enter sends, Escape drops the text and puts you back
at the wheel at once. The drive holds still while the CB menu or the typing
field is open, exactly as it does under the Tab status screens: only the top
screen runs each frame, so typed letters never reach the truck and no road
time passes. Escape puts the road back at once. Push-to-talk is different:
holding Talk opens no screen, so the truck keeps rolling while you speak.

**Quick calls** are one press each, for the things drivers actually say on
the road, with the place filled in by the game: Trooper ahead, Scale is open
ahead, Weather is bad here, Road is clear here, Copy that, Thanks, Anyone
near me, Good night. They exist because typing a sentence with a screen
reader is slow, and because a bare "trooper ahead" with your mile marker is
the most useful thing the CB can carry.

**Hearing calls while driving.** A call plays the CB squelch, then speaks:
"Road Star: chains required at Elk Mountain." It goes through the event voice
at the lowest priority, so a safety or navigation line always comes first,
and a call that has waited more than twenty seconds behind other speech is
dropped from speech and kept in the log. A call also files under a new CB
category in message review, so Comma steps back to it, the bracket keys
filter to CB only, and Alt C repeats the last call as it does today. A
setting chooses what the channel does while you drive: speak every call,
speak only calls to me, squelch sound only, or off. Quiet and urgent-only
speech rungs keep that choice; they do not silence the CB on their own,
because the player turned it on separately.

**Off the road.** At a stop, in the terminal or on the main menu, a new call
plays the squelch and speaks after whatever is already speaking; arrowing
through a menu cuts it off and the log keeps it. The CB screen under the
Online menu lists the last twenty calls newest first, says how many drivers
are on the channel, and lets you send. The same screen sits in the pause menu
and in the Driver apps tablet, where it replaces the Road chatter app.

**Private channels.** On the drivers board, the directory and a driver's
profile, a Send a call row opens the typing field for that driver. A private
call arrives as "Private from Road Star: ..." and the CB screen offers Reply
to Road Star at the top while a conversation is open. Private channels need
an account on both ends. A blocked driver's calls never arrive and the
blocker's never reach them, with no notice either way.

**Joining mid-haul.** When the CB connects it fetches the last twenty calls
from the past hour, so Listen back has context, but it speaks none of them.
If nobody has called in an hour, the screen says so once.

**Who can send.** Sending needs an orinks.net driver account, because every
call carries a screened name and can be reported. Listening to Channel 19
needs nothing, like reading the drivers board. Guest drivers, once PR #577
ships, listen only.

**Limits.** One call every three seconds, twenty a minute, 200 characters.
Over the limit the game says "Too fast. Wait a moment." and sends nothing.

## Voice on the CB

Voice is push-to-talk on the same Channel 19, one speaker at a time, and it
is off until a player turns it on. It arrives after text because it needs two
things no Hearken game has yet, microphone capture and a live socket, and
because it cannot be screened the way text can.

**Talking.** Hold the Talk key. The squelch opens with a short click, you
speak, you let go, and the squelch closes. If someone else has the channel,
the game plays the busy double-click and says "Channel busy" once; nothing is
sent. A transmission ends on its own after 30 seconds. While you hold Talk
the game sends nothing else and records nothing before or after; the
microphone is open only while the key is down.

**Hearing.** Received voice plays on Hearken's voice bus as a live feed, in
the centre of the stereo field, through a CB tone: a band-pass around 300 to
3000 hertz, a little compression and a hiss under it, so it reads as a radio
in the cab and not as a second screen reader. The game's own speech still
goes over it, and the voice bus ducks under speech the way music does, so a
hazard line is never buried by a driver telling a story. Each transmission is
announced the first time a driver speaks in a session, "Road Star on the CB",
and after that only the squelch marks who is talking; Alt C says who spoke
last. Voice never goes through the screen reader and there is no transcript.

**Setting up a microphone.** Settings, Sound gets a Microphone row listing
input devices, a Microphone level row, and Test microphone, which records
three seconds and plays them back through the CB tone so the player hears
what others will hear. The first time Talk is held with no microphone chosen,
the game picks the system default and says so.

**Choosing voice.** A setting, Voice on the CB, has three values: off, listen
only, listen and talk. Off is the default. The first time a player opens the
CB, one line offers it: "Drivers can also talk on Channel 19. Voice is off;
turn it on in Settings, Online." That line is said once per installation.

**One-to-one voice calls.** Later, after channel voice works. Call on voice
from a driver's row rings them: "Road Star is calling on the CB. Enter
answers, Escape declines." Inside a call both sides still push to talk; an
open microphone is not offered in the first version. A call ends when either
side hangs up, when either goes off duty, or after five minutes of silence.

**What voice does not do.** It is not spatial; drivers are not placed on the
map around the truck, because a voice that drifts across the stereo field
while a hazard sound also pans would fight the game's own audio language. It
does not record. It does not work without an account.

**Why push-to-talk and a relay, not open microphones and peer to peer.**
Push-to-talk gives one speaker at a time, which a relay can enforce with a
floor and a mixer never has to solve; it keeps every player's microphone
closed by default, which matters when a screen reader is speaking in the
room; and it is what a CB does. A relay room on orinks.net is one socket per
player, works through any home router, and hides nobody's address from
anyone. The owner asked about RTC: the Hearken client API hides the
transport, so a peer-to-peer path can replace the relay later without
changing Freight Fate.

## Speech, keys, settings, help and sounds

Every spoken line is as short as it can be, the changed fact first. Keys
below are defaults; all three CB actions are movable from Settings, Gameplay,
Controls, and every spoken instruction names the key the player chose.

### Keys at the wheel

| Action | Default | What it does |
| --- | --- | --- |
| Open the CB | Grave (the key left of 1) | Opens the CB menu over the drive: Say something, Quick calls, Listen back, Who is on the channel |
| Quick calls | Shift Grave | Straight to the quick calls list |
| Talk | Backslash, held | Push-to-talk, once voice is on; with voice off it opens Say something instead |
| Repeat the last CB call | Alt C (exists today) | Repeats the last report or call, with the handle |

The controller gets no defaults; every button is taken. A player can move
Talk onto a button they do not use.

### Spoken lines

| Moment | Line |
| --- | --- |
| Opening the CB | "CB. 6 drivers on Channel 19." then the first menu row |
| Say something | "Say something. Type your call. Enter sends, Escape goes back." |
| Sent | squelch close, then "Sent." |
| A call arrives | squelch, then "Road Star: chains required at Elk Mountain." |
| A private call arrives | squelch, then "Private from Road Star: where are you parked tonight?" |
| A call failed screening | "That call breaks the CB rules. Nothing was sent." |
| Too many calls | "Too fast. Wait a moment." |
| No connection | "The CB has no signal. Check your connection." |
| No account | "Sending needs your orinks.net account. Set it up in Online." |
| Channel quiet | "Nobody has called in the last hour." |
| Talk pressed, channel taken | busy double-click, then "Channel busy." |
| Talk held 30 seconds | squelch close, then "Transmission ended." |
| First voice from a driver this session | "Road Star on the CB." |
| Incoming voice call | ring, then "Road Star is calling on the CB. Enter answers, Escape declines." |
| Blocking | "Road Star blocked. Enter on their name in Blocked drivers undoes it." |

Calls never say "CB chatter" or "message" first; the squelch marks them, and
the handle is the first word, so a player who stops listening after two
words still knows who spoke.

**Settings rows.** On the Online menu, next to the account rows that give
them meaning: CB radio: on or off; Channel 19 while driving: all calls, calls
to me, sound only, or off; Share where I am on the CB: on or off; Voice on
the CB: off, listen only, or listen and talk; Blocked drivers, a list where
Enter unblocks. Under Settings, Sound: Microphone, Microphone level, and Test
microphone. The CB master switch follows Online services: off there is off
everywhere.

**Help.** F1 on the CB screen reads its intro help. The driving commands list
on F2 gains the three CB keys. The user manual gets a section, The CB radio,
covering Channel 19, private channels, quick calls, voice, blocking and the
rules link. The ontology gets rows for CB, Channel 19, handle and private
channel.

**Sounds.** Four, all in Learn game sounds because a player has to know them:
squelch open and close (the existing CB sound family), the busy double-click,
and the private call ring. Received voice gets the CB tone described above;
it is a treatment, not a sound to learn. All four are synthesized through
Hearken's synth to match the existing cb_radio_chatter.

## Safety, moderation and privacy

A public channel for a community of about 200 drivers, run by one person,
has to moderate itself most of the time. The design leans on account-gated
sending, screening before anything goes out, and player-side blocking, with
reports as the backstop.

**Who may send.** Only a driver signed in to an orinks.net account, whose
name has already passed the name screen. One account, one handle. A name
reset by a moderator stops that driver sending until a new name passes.

**Screening.** Every text call goes through the same obscenity and hate-term
screen the names use, on the server, before it is stored or relayed. A call
that fails is refused and the sender hears why; nobody else ever sees it. The
screen also drops links and anything over 200 characters. Voice cannot be
screened; that is why it is off by default and why it is the last thing
built.

**Blocking.** Block driver sits on every call in Listen back and on every
driver row. A block is kept on the server, so it holds across computers and
across text and voice: the blocked driver's calls never reach the blocker,
the blocker's never reach them, and neither hears about it. Blocked drivers
on the Online menu lists them and Enter unblocks.

**Muting.** Mute the channel for an hour, on the CB menu, for a night when it
is too busy. Mute driver keeps hearing them off for the session without a
block.

**Reporting.** Report call on any call in Listen back sends the call, the
twenty calls around it, the reporter and the time to the server. A voice
report carries who spoke, when, the room and the reporter; the audio itself
is not kept. Three reports of one driver from three different accounts in a
day mutes that driver on the CB for 24 hours automatically and tells the
owner.

**Moderator tools.** An owner page on orinks.net, like the testers page: open
reports with their context, mute a driver's text or voice for a day, a week
or for good, and the force-rename that already exists. The published Freight
Fate Online Rules page gets a CB section: the naming rules apply to every
call, no harassment, no sharing other people's personal details, voice
follows the same rules and can be muted without notice.

**What leaves the player's computer.** A call carries the handle, the words,
and, only when Share where I am on the CB is on, the same broad place line
the drivers board shows. Never a save, a profile name, a file path or
anything about the real person. The microphone is open only while Talk is
held, and nothing is recorded by the game or stored by the server. Channel
history is kept seven days and then deleted; reported calls are kept ninety
days for review. Private channels are stored the same way and are readable
only by the two drivers and a moderator acting on a report.

**Age.** Freight Fate has no age gate and the CB does not add one. The rules
page says the CB is public talk between adults and that voice is off by
default; that is as far as a game this size can reasonably go, and it is a
limit the owner should know about.

## How it is built

Freight Fate writes screens, speech and settings. Hearken carries the words
and the voice. orinks.net hosts the rooms and keeps what has to be kept.
Nothing about Freight Fate goes into Hearken, which is the standing rule for
the engine.

A call leaves cab A over one socket to a Durable Object room on orinks.net.
The room screens it against Convex, stores it, and fans it out to every cab
on the channel; cab B plays the squelch and speaks it, or plays the voice
frames through a feed. Voice frames take the same path and are never stored.

### In Freight Fate (`crates/freight-fate`)

- A `cb` module shaped like `duty_watch` and `online_presence`: a worker
  thread owns the Hearken rooms client and its socket, the game loop only
  drains a bounded queue of arrived calls and pushes a bounded queue of
  outgoing ones, and shutdown is explicit and timed. The worker joins
  Channel 19 when the drive starts and the CB is on, and leaves when the
  driver goes off duty, so presence on the channel matches the drivers
  board.
- States under `states/cb/`: the CB menu over the drive, the CB screen
  (Listen back, Who is on the channel, Reply), Quick calls, the typing field
  as a `TextEntry` with a 200-character cap, Blocked drivers, the incoming
  call prompt, and the microphone test.
- `Action::CbOpen`, `Action::CbQuick`, `Action::CbTalk` in `bindings.rs`,
  with the reserved-key rules unchanged. `Action::Cb` keeps its name and
  gains the real calls.
- `SpeechCategory::Cb` in `ff-core/src/speech_pacing.rs`, spoken at Ambient
  priority with a twenty-second wait budget, and `MessageCategory::Cb` in
  `message_log.rs` so the bracket filter and Alt C reach it. The place tag
  comes from the same `PresenceState` the board already receives.
- Settings fields in `ff-core/src/settings.rs`: `cb_enabled`, `cb_driving`,
  `cb_share_place`, `cb_voice`, `microphone`, `microphone_level`, and the
  per-session mutes kept in memory only.
- Received voice: the `hearken-talk` jitter buffer is a `Feed`; the game
  starts it on `Bus::VOICE` with a new CB insert next to the cab speakers
  insert in `hearken-audio`'s voicing. That insert is generic (a band-pass
  radio tone) and belongs in Hearken.
- Help, manual, ontology rows, Learn game sounds entries, the F2 command
  list, and the changelog entry.

### In orinks.net (public repo, Cloudflare Workers plus Convex)

- Routes under `app/api/freight-fate/cb/` that mount the Hearken rooms server
  library with a Freight Fate accounts adapter: `accounts.player(token)` is
  `acceptDriverToken`, so the existing driver tokens work unchanged, and
  swaps to the shared Hearken accounts when that move lands.
- One Durable Object class for live rooms, one object per room (Channel 19,
  each private channel, each voice call), using WebSocket hibernation so an
  idle channel costs nothing. It relays events, holds the voice floor, and
  forwards the one speaker's Opus frames to everyone else in the room.
- Convex tables: `cbMessages` with a seven-day purge cron, `cbBlocks`,
  `cbMutes`, `cbReports`. Screening calls `moderation.ts`. The owner page at
  `/freight-fate/online/moderation` reads and writes them, gated the way the
  testers page is.
- The rules page gains its CB section.

### In Hearken

- `hearken-rooms` (merged in Hearken #59): protocol types, a Transport trait,
  a worker client with a Room handle carrying members, an event log and chat
  events, and polling that drops to every 10 seconds once quiet. No
  dependency on hearken-app, so Freight Fate embeds it directly. Auth is the
  account token only, so Freight Fate's driver tokens work unchanged.
- `hearken-talk` (planned as Hearken's PR 3): cpal microphone capture, Opus
  at 48 kilohertz mono in 20 millisecond frames through the libopus already
  in the tree, push-to-talk state, and a jitter buffer that implements
  `Feed`.
- Asked of Hearken's PR 2 (text chat) before it is built: a persistent
  channel kind joined by name with no member cap and no expiry, last-N
  history on join with seven-day retention, a create-or-find route for
  private channels that shows up in the other driver's room list, an
  optional screened `tag` field on chat events for the place line, refusal
  codes the game can speak, members with a stable id and display name,
  per-kind rate limits, per-pair mute, block and report held server-side,
  and game-overridable keys. The Hearken thread confirmed none of these
  conflict with what is merged.

Freight Fate pins the Hearken revision as it does today and vendors nothing
new. The agent server drive for the game side runs against Hearken's
`server/bin/dev-server.js` with two game instances, so a play test can hear a
call travel from one cab to another on one machine.

## The state machines, made explicit

The owner's standing rule (2026-11-09): find the implicit state machines and
make them explicit in the way that serves the player, the developer and the
agent play tester at once, without breaking what works. The CB has seven.
Each becomes one enum in `ff-core` with its transitions tested headless, each
transition has one spoken line or sound, and the agent server reports every
state so a play test can read what the game thinks is happening instead of
guessing from speech.

| Machine | States | Transitions the player hears | Where it lives |
| --- | --- | --- | --- |
| CB link | Off, Signed out, Connecting, On channel, Reconnecting, Muted for an hour | "CB on." / "The CB has no signal." on the first failure only / silence on reconnect, the squelch when the channel is back / "Channel muted for an hour." and "Channel back." | `ff-core::cb::Link`, driven by the worker, read by the Online menu row and `status` |
| Sending | Idle, Typing, Screening, Sent, Refused, Too fast | field prompt / "Sent." with the squelch close / the rules line / "Too fast. Wait a moment." | `ff-core::cb::Send`, one per call |
| Talk key | Idle, Asking for the channel, Talking, Busy, Cut at 30 seconds, Voice off | squelch open / busy double-click and "Channel busy." / squelch close / "Transmission ended." / with voice off the key opens Say something | `ff-core::cb::Talk`; the relay's floor is the authority, the enum mirrors it |
| A received call | Arrived, Spoken, Dropped to the log, Repeated, Reviewed | squelch then the words / nothing when dropped / Alt C / Comma and Period | `MessageLog` entry plus `SpeechCategory::Cb`; the drop after 20 seconds is the pacer's existing rule |
| Private channel | None, Open with a driver, Ringing out, Ringing in, In a call, Ended | "Private from Road Star: ..." / the ring line / "Call ended." | `ff-core::cb::Private`, one at a time; a second ring while in a call is refused with "Busy" |
| A driver's standing | Clear, Muted by me this session, Blocked by me, Muted by a moderator until, Needs a new name | "Road Star blocked." and its undo line; a moderator mute is silent to everyone but the muted driver, who hears "Your CB is muted until ..." | `cbBlocks` and `cbMutes` on the server, mirrored as `ff-core::cb::Standing` for the row labels |
| Microphone | Not chosen, System default, Chosen, Missing | "Using the system microphone." once / "No microphone found. Check the system sound settings." | `Settings::microphone` plus a probe result; Test microphone reads the state back |

The Talk key in full: Idle goes to Asking for the channel when Talk is held.
Asking goes to Talking when the relay says the channel is free (squelch
open, microphone live) or to Busy when another driver has it (busy
double-click, "Channel busy."). Talking goes back to Idle when the key comes
up (squelch closes) or to Cut at 30 seconds when it has been held that long
("Transmission ended."). Busy and Cut go back to Idle when the key comes up.
With Voice on the CB set to off, the key never enters this machine; it opens
Say something instead.

**Developer experience.** Every enum has a `next(event)` function and a test
table, so a wrong transition fails in `cargo test -p ff-core` with no window,
the way the speech pacer and the hours-of-service clock are tested today. The
worker thread owns the socket and sends `Link` transitions over the queue;
the game loop never sets one by hand. No booleans travel in pairs.

**Agent experience.** The agent server's `status` gains a `cb` block: the
link state, the talk state, the open private channel, the number of drivers
on the channel and the last call's handle. A `cb` scenario stages the
machines without a second game: deliver a call from a named handle, hold the
channel so Talk hears Busy, drop the link so Reconnecting is heard, mark a
driver blocked. With those, the required play test drives every row of the
table above from one instance, and the two-instance drive against the dev
server is kept for the end-to-end check.

**Without breaking anything.** Alt C keeps its name and its behaviour and
only gains calls to repeat. The message review keys, the speech rungs, the
Online menu's row order rules, the presence worker and the duty watch are
untouched; the CB is a new worker beside them that reads the same
`PresenceState`.

## Phases and what comes first

None of this is in the Christmas alpha. The order below puts the moderatable
part first and the part that needs new engine work last, and it waits for
the two pieces of work it depends on.

| Phase | What ships | Depends on | Threads |
| --- | --- | --- | --- |
| 0. Design | This document and the Hearken peer-play design, both reviewed by the owner | Nothing | Done |
| 1. Text on the CB | Channel 19, private channels, quick calls, Listen back, blocking, reporting, the moderation page, the rules section, help and sounds | Hearken's text chat PR; the Freight Fate accounts move, so the accounts adapter is written once | One orinks.net thread (rooms host, Durable Object, Convex tables, owner page), then one Freight Fate thread against `dev` |
| 2. Voice on Channel 19 | Push-to-talk, the microphone rows and test, the CB tone, the voice floor on the relay, voice mutes | `hearken-talk` merged; phase 1 played by testers for a few weeks | One Hearken thread for the CB insert, one Freight Fate thread |
| 3. Private voice calls | Call on voice, the ring, the call room | Phase 2 in use | One Freight Fate thread; cut if phase 2 shows little use |
| Later, if asked | Peer-to-peer voice behind the same client API; transcripts of voice for players who want text; Canadian and regional channels if the community grows | Hearken's transport swap | Not planned |

**Play testing.** Each Freight Fate phase gets the full agent-server drive
the project requires: two game instances against the local dev server, a
call sent from one cab and heard in the other at the wheel, in a status
screen, at a stop and on the main menu; the quiet and urgent-only rungs; the
bracket filter and Alt C; blocking both ways; a screened call refused; the
no-connection and no-account lines; and for voice, a transmission recorded
through the test microphone row and heard with the CB tone. The PR lists
what was played, as every Freight Fate PR does.

**Release notes.** One short line per phase in `CHANGELOG.md`, in player
terms.

## Decisions for the owner

Each one has a recommendation.

1. **The CB and Channel 19, or a plain "driver chat"?** Recommended: the CB.
   It is already in the game, it explains push-to-talk and one speaker at a
   time without a tutorial, and the trooper reports keep the channel alive
   on empty nights.
2. **Typing holds the truck still, or the road runs on under the field?**
   Recommended: hold still, as every Tab screen does today. Letting the
   drive run under a text field would be new behaviour, with a hazard
   arriving while the letter keys are taken. Quick calls are the fast path
   for a driver who wants to say something without a stop in time.
3. **Where you are on a call: on by default when the drivers board shares
   it, or off until chosen?** Recommended: follow the board. A player who
   shows their haul on the board has already chosen to share that much, and
   it has its own switch.
4. **Listening without an account.** Recommended: yes, like the board.
   Guests listen only.
5. **Voice default.** Recommended: off, offered once, with listen only as
   the middle step.
6. **Relay on orinks.net or peer to peer.** Recommended: the relay. It is
   simpler to run and moderate, hides addresses, and the Hearken client API
   leaves room to swap to peer to peer later if it ever matters.
7. **Keep any voice audio for reports?** Recommended: no in the first
   version. Reports of voice carry who and when; moderators can mute voice
   per driver. Revisit if abuse shows up.
8. **Build the CB before or after the accounts move?** Recommended: after.
   The accounts adapter is then written once against shared Hearken
   accounts instead of twice.
9. **Private voice calls in scope?** Recommended: phase 3, after channel
   voice has been heard for a few weeks, and cut if nobody uses channel
   voice.
10. **Automatic 24-hour mute after three reports in a day.** Recommended:
    yes, with a notice to the owner, so the channel protects itself
    overnight.

## Risks and open questions

- **An empty channel.** About 200 drivers, a handful on duty at once, means
  many nights with nobody talking. The trooper reports keep the CB alive,
  Who is on the channel and the duty watch say who could answer, and the
  quiet line says so once instead of leaving silence. If the channel stays
  empty after a season, voice is not worth building.
- **One moderator.** Screening, blocking, the automatic mute and the owner
  page carry most of it, but a report at 3 in the morning waits for the
  owner. The rules section should say replies are not instant.
- **Speech collision.** A chatty night plus road lines is too much for one
  ear. The twenty-second drop, the lowest priority, and the calls-to-me
  setting are the controls; the play test must drive a busy channel through
  a city arrival at the quiet rung and listen for anything lost.
- **Microphone permissions.** macOS asks the player for microphone access
  the first time and needs the usage string in the notarized app's property
  list; Windows has a privacy switch that silently blocks capture; the
  Retroid under Wine has no microphone at all. Test microphone has to say
  plainly "No microphone found" and point at the system setting.
- **The accounts move.** Building the CB on driver tokens and then moving to
  shared accounts means writing the accounts adapter twice. Waiting is the
  recommendation above; it also pushes the CB later than the owner may want.
- **Cost.** Durable Objects bill for time awake; WebSocket hibernation keeps
  an idle channel near free, and voice is about 24 kilobits a second per
  listener while someone talks. At this community's size the Workers paid
  plan already in place covers it; a check after the first month is the
  sanity test.
- **Latency.** A relay adds one hop. Text does not care; voice at 100 to 250
  milliseconds is fine for push-to-talk and would not be for an open
  conversation, which is one more reason the first version is push-to-talk.
- **Place tags.** The broad place line is what the board already shows, so
  the CB adds no new exposure, but a call is pushed to everyone rather than
  read on request. The separate switch covers it.
- **Hearken independence.** The CB insert, rooms and talk crates must stay
  generic. Anything Freight Fate-specific, the place tag, the quick calls,
  the driver tokens, lives in Freight Fate or orinks.net.
- **Open.** Whether the community wants voice at all; several players use
  Discord for that already. Whether other Hearken games will share a room
  kind with Channel 19 or want rooms of their own; the Hearken room
  primitive covers both. Whether Canada, once baked, wants a second channel.
