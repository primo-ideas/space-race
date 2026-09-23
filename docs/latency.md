# Latency

How long the player waits between pressing something and seeing their car answer, what that
time is made of, how it is measured, and how the player's own car is predicted so that most of it
goes away. The measuring came first, before any attempt to reduce the wait: without it, every
change to the netcode would be judged by feel alone.

## Where the time went

The server has full authority. Before prediction, the client displayed every car, the player's
own included, from the server's snapshots, and a press travelled:

| Step | Cost |
| --- | --- |
| The input leaves, only when it changes, over the WebSocket | half a round trip |
| The server applies it at its next tick | 0 to 16.7 ms, 8 on average |
| The snapshot of that tick comes back | half a round trip |
| The race is displayed three ticks in the past, to have snapshots to interpolate between | about 42 ms |
| The frame that displays it, checked once a frame | up to 16.7 ms |

Measured against a server on the same machine, that was **75 ms** on average from a press to the
car answering on screen, **60 of them the client's own**, before any network at all.

## Prediction

`client/src/prediction.rs` drives the player's car on the client the moment they act, and lets
the server correct it.

**A timeline ahead of the server.** The client keeps a timeline of its own: the server's time as
the snapshots tell it, plus a **lead** of a round trip and two ticks of margin, following the
measured ping slowly so it never visibly stretches. An input the client applies on tick `n` of
that timeline then reaches the server before the server steps tick `n`: the snapshots' clock
trails the server by the way down, and the input takes the way up.

**Inputs placed on a tick.** Each input goes out stamped with the tick it takes effect on, the
next one on the timeline, and the server holds it until then (see
[Protocol](protocol.md#in-a-lobby)). Both sides step the car with the same inputs on the same
ticks, through the same code: `Car::step` while driving freely, and `race::racing_tick` in a race,
which holds the car on the grid, grades the start and pays its boost, so even the start is
predicted to the tick (see [Lobbies](lobbies.md#the-start)). The client needs the car's tuning for
that, which the lobby state now carries.

**Reconciliation.** The client keeps the last two seconds of predicted ticks, each with the input
in effect and the start's state. When a snapshot of the car arrives, the server's state goes in
place of the prediction for its tick and the ticks since are replayed on top of it, with the
inputs placed since. Snapshots arriving in a burst are reconciled once, from the newest: on a slow
machine a frame can bring dozens.

**Nothing to correct while inputs arrive in time.** The same inputs on the same ticks through the
same code give the same car. Across different machines that is exact for arithmetic but not
promised for every function a platform's math library computes -- `sin` or `exp` may differ in the
last bit between Windows, Linux and the browser -- which the reconciliation absorbs: the
difference is put right on every snapshot, so it can never grow. An input that arrives after its
tick takes effect on the server's next one, and the prediction is then off until the replay has
caught up.

**Corrections glide.** When a reconciliation moves the car, the display keeps where it was and
glides to the new place over a few frames (a time constant of 80 ms), unless the move is over
6 m: that is a teleport -- a new race lining the cars up -- and is shown as one.

**What stays in the past.** Only the player's own car is predicted, and only while they drive
it: not once they have crossed the line and the server drives it, nor while they spectate. The
other cars stay interpolated three ticks in the past, which is what makes them move smoothly, and
are therefore shown a little behind where they are -- as every online racing game shows its
opponents. The standings and progress stay the server's. The start lights and the race time
follow the player's timeline, so the lights turn green when their car can go.

**The display.** The car is shown a tick behind the timeline, between two predicted ticks, so it
moves smoothly at any frame rate. An input takes effect on the next tick, so it shows about two
frames after the press.

## What is measured

`client/src/latency.rs` times what follows and shows it over the race with **F3**, or from the
start with `--latency` natively and `?latency` on the web, which is how a phone gets to see them.
Every five seconds the figures also go to the log at `info` (`--log info`). Each figure is the
mean of the last 32 measurements, with the spread or the worst of them beside it, since a delay
is felt at its worst.

- **Ping.** The client sends `Ping(n)` twice a second, and the server answers `Pong(n)` the moment
  it reads it, whether the player is browsing or in a lobby. It is timed from the system that
  sent it, as an input is, so the two compare; on the web that includes waiting for the frame to
  end before the message leaves, which an input waits for too.
- **Input → server.** From the input changing to the arrival of the first snapshot that includes
  it. Since inputs wait on the server for their tick, this now includes the lead.
- **Input → screen.** From the same moment to the first frame showing the player's car with it:
  the prediction reaching its tick, or, without a prediction, the snapshot including it
  displayed. This is the one the player feels, short of the frame that sampled the input and the
  one that reaches the glass.
- **Correction.** How far the prediction was from the server's state when a snapshot put it right,
  in centimeters.
- **Lead**, **buffer** (how far the displayed race trails the newest snapshot, about 42 ms rather
  than 50 when snapshots arrive on time) and **frame**.

What ties a snapshot to an input is a number: every `Input` carries `seq`, counting the player's
inputs in the lobby from 1, and every car in every snapshot carries `input_seq`, the last of its
driver's inputs in effect when the server took it.

## Measurements

On Windows in release, against a server on the same machine, the autopilot drifting laps at 60
frames per second (2026-09-22). The circuit was the hippodrome, a 16 m oval the game no longer
ships (see [Tracks](tracks.md)); the figures are about the network and the frame, not the road:

| | Before prediction | With prediction |
| --- | --- | --- |
| Ping | 1 to 2 ms | 1 ms |
| Input → server | 12 to 15 ms | 36 to 37 ms, the lead included |
| **Input → screen** | **70 to 89 ms**, worst 102 | **33 to 34 ms**, worst 54 |
| Correction | | **0.0 cm, worst 0.0**, across whole races and the start |
| Lead | | 34 to 35 ms |

The player sees their car answer before the server has even acted on the input. The start is
predicted exactly as the server grades it: a capture a second after it shows the S the autopilot
earned and its boost, with not a centimeter corrected. With a second car in the race, drawn from
snapshots, and both clients sharing the machine at 30 frames per second, input to screen was
44 ms: at that frame rate the frame is what limits it.

Nothing has been measured against the deployed server yet: the native client cannot reach
`wss://`, so that measurement is the web client's, in a browser, with `?latency`.

## What could come next

- **One frame less.** The car is shown a tick behind the timeline so there are two ticks to
  interpolate between. Stepping one tick further, speculatively, with the current input, would
  show a press a frame sooner.
- **A timeline that bends rather than jumps.** The lead follows the ping slowly, and the timeline
  never goes back; a change of network is followed by a stretch of a few ticks.
