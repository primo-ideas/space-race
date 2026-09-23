# Lobbies and races

Players race in lobbies. A lobby is created by a player, races on one track, and holds its players
race after race until the last one leaves. The rules live on the server (`server/src/lobby.rs`),
which has full authority; what they share with the client lives in `sim/src/race.rs`.

## Settings

The player who creates a lobby chooses, once and for all:

| Setting | Range | Default in the client |
| --- | --- | --- |
| Name | 1 to 32 characters, same rules as nicknames | "*nickname*'s lobby" |
| Track | any track of the server | the esplanade |
| Laps | 1 to 20 | 3 |
| Players to start | 1 to the maximum | 2 |
| Maximum players | 1 to 16, spectators included | 8 |

`LobbySettings::is_valid` checks the ranges on both sides; the server also refuses an unknown track.
There is no owner and nothing can be changed afterward: a lobby with other settings is another
lobby. The lobby closes, and disappears from the list, when its last player leaves.

## Phases

```text
        enough players             every racer finished,
Waiting ─────────────▶ Racing ─────── or timeout ───────▶ Results
   ▲                     ▲                                  │
   │                     └────────── enough players ────────┤
   └─────────────────────────── not enough players ─────────┘
```

- **Waiting**: fewer players than "players to start". Everyone drives freely, so waiting for a
  friend is not staring at a black screen.
- **Racing**: as soon as there are enough players, every player is lined up on the grid, in the
  order they joined, and held there for 6 s: inputs are kept but cars do not move, so everyone
  starts at the same tick. Nobody drives before the start, so there is no countdown to jump. The
  grid shows nothing for 3 s, time to look at the screen, then the four start lights light up, one
  a second: three amber, and the fourth, green, at the very start. A player joining while the cars are on the grid takes the
  first free slot and races. The race lasts until every racer has finished, or 30 s after the
  first one did. It also ends when no racer is left.
- **Results**: 8 s of results. Whoever crossed the line is still watching their car drive itself
  (see [After the line](#after-the-line)); everyone else drives freely again. Then the grid again
  if there are still enough players, or waiting, where everyone has their car back.

These durations are `lobby::Timings`; tests shorten them.

## The start

Pressing the accelerator right at the start gives a boost. The press that counts is the one held
at the start, or the first one after it, and its distance to the start, early or late, grades it:

| Grade | Within | Boost | HUD |
| --- | --- | --- | --- |
| S | 50 ms | all of `start_boost` | PERFECT START |
| A | 100 ms | 80 % | GREAT START |
| B | 150 ms | 60 % | GOOD START |
| C | 225 ms | 40 % | FAIR START |
| D | 300 ms | 20 % | SLOW START |
| E | beyond, or no press | none | NO START BOOST |

Holding the accelerator from the grid is an early press like any other and gets an E: a good start
is timed on the rhythm of the start lights, the start being the fourth, green one, and reacting
to it (a couple of hundred milliseconds for a human)
lands around C or D. `sim::race::StartTiming` follows each racer's accelerator and grades the
start; the lobby gives the boost on the tick the grade is decided and publishes the grade in the
lobby state, which the HUD shows for a few seconds. A racer who never presses gets E once no press
could still arrive.

**Grading on the tick the press takes effect.** The player's car is predicted on a timeline of
its own, ahead of the server by a round trip and a margin (see [Latency](latency.md#prediction)),
and the start lights follow that timeline: they turn green when the player's car can go. An input
carries the tick it takes effect on, the server holds it until then, and the start is graded on
that tick -- the same on both sides, through the same code (`race::racing_tick`), so the grade
and the boost the client predicts are the ones the server gives. Latency costs nothing while the
lead covers the round trip. A press that arrives after its tick takes effect on the server's next
one and is graded there: late, as it really was. A client cannot place a press further ahead than
`MAX_INPUT_LEAD`, half a second, so a press sent early and placed on the start is not possible:
it takes effect early and grades E.

A start with no press is given up, graded E, once the grading window is past (`race::give_up_tick`):
no press after that could grade better.

## Joining

Joining while waiting, on the grid or during results gives the player a car at once, on the first
free grid slot, and a place in the coming race. Joining after the start makes them a
**spectator**: no car, the camera on a racer, and a car of their own as soon as the race ends. A
lobby that holds its maximum refuses players, spectators included.

In the lobby list, a lobby is therefore either **about to race**, where joining means racing next,
or **racing**, where joining means watching first. The list shows which, with the seconds left on
the grid or the leader's lap; the lobby publishes its listing again at the start so the two never
blur.

## Progress, laps and finishing

`sim::race::Progress` measures how far a car has driven along the track, every lap counted: the
start line is 0, one lap later is the track length. It follows the change of the car's projection
on the centerline each tick, taking the short way around the start line, so driving backward
counts against it and crossing the line back and forth gains nothing. Cars on the grid start a few
meters below zero.

A car has finished once its progress reaches `laps × length`. Its time is counted from the start
tick to the moment it crossed, interpolated within the last tick from its progress before and
after, so two cars finishing in the same tick are still told apart.

Snapshots carry every car's progress during races, interpolated with the car on the client. The
client ranks with the same `RaceStanding::rank` as the server: finished cars first, fastest first,
then racing cars, furthest first. The lap shown is `race::lap(progress)`.

Results list finishers by time, then the racers still on track by progress, marked as not
finished. A player who leaves after finishing keeps their result; one who leaves before is not
listed.

### After the line

A player who has finished keeps their car on the track, but stops driving it: the server feeds it
the drifting autopilot instead of the player's input. Their last input would otherwise leave a car
parked across the road, or coasting to a stop on the racing line, while the others are still
coming in. Cars do not collide with one another, so a car driving itself home can never get in the
way of one still racing.

**It lasts until the next race lines up**, results included, and not only until the race ends.
`PlayerStatus::Finished` means exactly that, from the line to the next grid, and it is the one
thing that takes the wheel from a player. A race can end on the very tick its last racer crosses
the line — a solo race always does — and giving the car straight back would make the finish a
single tick long, seen by nobody. Whoever did not cross the line, and a spectator given a car at
the end of the race, drive freely through the results as before.

The client stops chasing it at the same moment. Instead it cuts between shots every 3.6 s, the way
a race is shown on television: low behind the car, alongside it, ahead of it looking back, high
above the line it takes, and one camera planted beside the road that the car drives past and away
from. Most shots are worked out from the car alone; the two that would otherwise end up inside a
wall, the one keeping pace beside it and the one on a post, read the road instead. A cut is a cut,
never a sweep. The player still sees their place and their time in the HUD, whose hints fall back
to the one key that still does anything, the one that leaves, and the race ends for everyone as
usual.

## On the server

```text
Session ──CreateLobby / JoinLobby──▶ Directory ──spawns / finds──▶ Lobby task
   ▲                                    │  ▲                          │
   │◀────── watch: every lobby ─────────┘  └──── Changed / Closed ────┘
   │◀────── watch: lobby state, mpsc: snapshots ──────────────────────┘
```

- **The directory** (`server/src/directory.rs`) is one task owning the list of lobbies. It
  validates settings, spawns lobby tasks, hands out lobby handles to sessions that want to join,
  and publishes the list through a watch channel whenever a lobby reports a change.
- **Each lobby** is a task of its own, ticking at 60 Hz, owning its players and cars, like the
  single race did before lobbies. Lobbies do not share anything, so one busy lobby cannot slow down
  another beyond sharing the machine.
- **The lobby state** (phase and players) goes to its members through a watch channel: a member
  always gets the latest state, and a slow connection skips intermediate ones instead of queuing
  them. Snapshots keep their own small queue per member, dropped when full, as before.
- **A browsing player** follows the directory's watch channel. The list holds deadlines rather than
  remaining times, converted when sent, so a list that waited is still right.
- **A join** goes to the lobby itself. If the lobby closed in between, its command channel is gone
  and the join fails with `NoSuchLobby`: nothing needs locking to be consistent.
- **The creator** is put in the lobby before its task starts, so a new lobby is never empty.

The one lock of the server holds the connected players: it refuses a second connection with the
same key (`AlreadyConnected`), and remembers each player's nickname and where they are, which is
published to browsing players as `Players` (see [Protocol](protocol.md#browsing-lobbies)). It is
held while one player is added, moved or removed and the list that follows is published, never
across an await, and a guard removes the player however the connection ends.

## Car tuning

`car.ron` is shared by every lobby. A task checks it every second and publishes changes through a
watch channel; each lobby picks up a new tuning at its next tick. A tuning must suit every track:
its collision radius must fit on the narrowest one.
