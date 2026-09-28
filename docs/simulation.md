# Simulation

The driving simulation lives in the `sim/` crate. The server runs it with full authority. It sits
in a crate of its own rather than in `server/` so the client can run the exact same code later, to
predict its own car between server updates; nothing in it knows the wire exists (see
[Architecture](architecture.md#workspace-layout)).

## Conventions

- Cars move on a plane: `x` and `y` in meters, speeds in m/s. The relief and the banking of the
  road add a height over that plane, which the car follows but does not simulate in three
  dimensions.
- Angles are radians counter-clockwise from `+x`, so **a positive angle turns left**. Track files
  use degrees with the same sign, because people write them.
- Steering input is the exception: **positive steers right**, like a gamepad stick. The car code
  converts once, in `Car::steer`.
- The simulation advances in fixed ticks of `TICK_SECONDS` (60 per second). A fixed step keeps the
  behavior independent of frame rate and server load, and it is what the client's prediction of
  the player's car rests on (see [Latency](latency.md#prediction)): replaying the same inputs
  must produce the same result.

## The car model

The model is arcade on purpose. The goal is a car that feels precise and readable with a gamepad,
not a car that behaves like a real one.

The one structural choice is that **velocity and heading are separate**. Every tick:

1. **Steering** first turns the wheel, `Car::steering`, toward the stick: every tick it closes a
   share of the gap, `steering_rate` per second. It moves on the very tick the stick does, so
   there is no delay before the car turns, and it eases in as it gets there: the car turns gently
   at first and a little harder as the wheel comes round, whether the stick is flicked or a key
   pressed. The wheel, not the stick, sets a target yaw rate: `speed / turning radius`, scaled by
   it. The turning radius grows with speed, from `turn_radius_slow` to `turn_radius_fast`, so full
   lock stays controllable at speed. The yaw rate follows that target at `steering_response`,
   well above the wheel's rate, so it follows the wheel almost at once and only smooths what the
   wheel does not: the change of radius when a drift starts or ends. With the shipped tuning, half
   lock at top speed turns the car at a seventh of its eventual rate on the first tick, two fifths
   after 50 ms and nearly three quarters after 100 ms. The car always steers at least as if it went
   `min_steering_speed`: there is no reverse, so this is what lets a car stopped against a wall
   turn away from it.
2. **The velocity is split** along the new heading. The part along the heading is driven by
   accelerating or coasting. The sideways part, whatever the rotation left behind, decays at
   the rate set by `grip`.
3. **Walls** push the car back and absorb the speed going into them.

Because the sideways part decays instead of vanishing, the car already slides a little in turns
and loses some speed doing so. **Drift** builds on that slide.

### Drift

Drift is the heart of the gameplay, in the spirit of Rocket Racing, and the model is drawn from a
recording of it: the car snaps sideways in a third of a second, holds 45 to 55 degrees across its
travel all through the turn, keeps its line and its speed, and pays a boost when it straightens.
The player taps the drift button to go into a drift and lets go of it at once; holding it is only
for tightening a turn further.

That is two things kept apart. **Where the car goes** is set by the stick: in a drift, the stick
turns the travel itself, on a curve of its own. **Where the body points** is the drift angle,
`Car::body`, which swings far across the travel and back: it is drawn, and it decides when a drift
ends, but it never carries the car anywhere. A slide that moved the car would run it wide of every
corner by as far as its travel lags its nose; this one does not.

- **Starting.** Above `drift_min_speed`, a press of the drift button with the stick turned at
  least a fifth of the way throws the car into a drift toward the stick, on the tick it is pressed.
  A press is a press: a button held down since before starts nothing. Turning hard enough starts
  one too, without the button: while the car grips, `Car::slip` is the real angle between its nose
  and its travel, and its reaching `drift_entry_angle` breaks the car away toward the side it came
  round. With the shipped tuning, full lock at speed puts the nose about 7 degrees off the travel
  and the entry sits at 5.
- **The angle.** The body swings toward `drift_angle` times `1 - (1 - s)²`, where `s` is how far
  the steering points into the drift: most of the angle with the first half of the steering, all
  of it at full lock, none with the steering centered or against the drift. The curve eases in
  from the center, where a square root, tried first, swung the body on every twitch of the stick.
  The body swings like a critically damped spring of natural frequency `drift_angle_response`: its
  speed builds up and dies down instead of jumping, whether the drift starts, the stick eases or
  the drift ends. With the shipped 55 degrees and 10 per second, it is 37 degrees across a quarter
  of a second after the press and 52 after half a second. When a drift starts, the body takes over
  whatever slide the car had and the heading becomes the travel, so nothing on screen jumps.
- **Turning.** The stick sets a **signed curvature**: fully to one side turns on
  `drift_radius_tight`, centered runs straight, whichever side the drift is on. The curve follows
  the wheel at `drift_steering_response`, almost at once, and the travel keeps all of its speed as
  it turns.
- **Speed.** A drift costs nothing: without the button, the car keeps its speed however long it
  drifts, as in the recording.
- **The button.** Never needed after the press, and holding it does three things. It multiplies
  the drift radius by `drift_button_tighten`, pulling the turn tighter than the stick alone can. It
  keeps the drift alive when the body comes back into line, so a drift can be carried down a
  straight; and with the stick more than half way against the drift, it carries the drift over to
  the other side, charge and all, so an S is one drift. And it costs speed: `drift_button_drag`
  from the moment it is pressed, plus `drift_button_drag_ramp` for every second it stays held, so a
  drift tightened for long brakes harder and harder.
- **Charging.** The drift builds a charge: seconds of drift, counted from half to one and a half
  times real time as the stick goes from centered to fully into the drift. Steering out of the
  drift charges at the slow end, never backwards.
- **Ending.** The body back within `drift_exit_angle` of the travel, with the stick keeping it
  there, centered or against the drift, ends the drift and pays out, unless the button is held.
  With the shipped tuning, a drift ends half a second after the stick straightens, as the steering
  unwinds and the body swings back into line. Falling
  below `drift_min_speed` ends it and pays out too.
- **Boost.** Ending the drift converts the charge beyond `drift_min_charge` into
  `drift_boost_rate` seconds of boost per second, up to `drift_max_boost`: with the shipped tuning,
  the full second of boost takes two to three seconds of drift. A boost raises the top speed to
  `boost_top_speed`, a fifth above `top_speed` as in the recording, and pushes at
  `boost_acceleration`, in full up to `top_speed` and fading out above it, even without the
  accelerator. After it, the car settles back to `top_speed` at the `coasting` rate.
- **Losing it.** A drift that touches a wall ends at once and pays nothing. The body settles back
  into line on its own, and the button, still held, starts nothing until it is pressed again.

The car state carries all of it (`slip`, `body`, `drift`, `drift_charge`, `drift_held`,
`drift_button`, `boost`), so the simulation stays one deterministic step, and the client predicts
a drift exactly as it predicts the rest.

The tuning stays on the server, so the snapshots carry two gauges the server works out for the HUD:
the boost that ending the drift now would give, and the boost left, both as shares of
`drift_max_boost`. The client draws the first twice: as the green DRIFT gauge over the speed, and
as Rocket Racing does, an arc of green segments on the road around the back of the player's car,
lit from the middle outward as the charge builds and gone when the car is not drifting. The arc
follows the travel rather than the body, so it stays under the car and square to the camera.

#### How the drift got here

Four models came before this one, and each taught something.

1. **v0.1.12.** The button, held with the stick turned, locked a side, and the travel caught up with
   the nose at `drift_grip` 3.5. That real lag carried the car about `speed / drift_grip`, some
   11 m at 40 m/s, before it turned at all: the drift felt thrown to the outside of every corner.
2. **v0.1.13 to v0.1.18.** The slide became a drawing: the path followed the heading closely and a
   separate angle, 20 to 45 degrees, showed a slide that never moved the car. The button said when
   the drift was on, since the car never came off its axis.
3. **2026-09-20.** The drift was to start by going off the axis and end by coming back into it,
   the button no longer needed, so the angle had to become real, at `drift_grip` 4. It settled at
   20 degrees, 26 with the button held, a second to get there, and the travel a quarter of a second
   behind the nose ran the car several car widths wide: why `serpentine` went and the roads grew to
   28 m. The drift also dragged, harder the longer it lasted.
4. **2026-09-27.** A recording of Rocket Racing showed what the drift is meant to be: twice the
   angle, at once, on the line, with no speed lost, and the button only tapped. A real angle that
   large would run the car tens of meters wide, so the angle is a drawing again; but it is the
   drawing that decides, which keeps the rule of 2026-09-20: off the axis starts a drift, back into
   line ends it, and the button held keeps it. The cost moved from the drift to the button, since
   the recording drifts for seconds at top speed without holding anything.
5. **2026-09-28.** Much better, the user said, but the steering had to be smoother. It jerked: the
   stick went straight into the yaw rate and the body followed its target at a plain rate, so the
   nose's rotation jumped by 479 degrees per second on the tick a drift began, turned back on
   itself when the stick eased from full lock to half, and jumped by 382 the other way when the
   stick straightened. The wheel now turns toward the stick at `steering_rate` and the body swings
   like a spring, and the rotation never changes by much more than 40 degrees per second from one
   tick to the next. The price is a slower swing: 45 degrees after about 0.4 s rather than 0.25,
   and a drift that ends half a second after the stick straightens rather than a quarter.
6. **2026-09-28, later.** There was a small delay before the car turned, the user said, which had
   to go: it should deviate gently at once, then a little more. The wheel of the step before
   turned at a fixed rate from where it stood, and the yaw rate followed it through a lag of its
   own, two delays in a row: half lock turned the car at 3% of its eventual rate on the first
   tick and 17% after 50 ms. The wheel now closes a share of its gap every tick, so it moves most
   on the very tick the stick does and eases in as it arrives, and the yaw rate follows it almost
   at once: 14% on the first tick, 42% after 50 ms and 72% after 100 ms, much as before the wheel
   existed, but still with no jump from one tick to the next.

Measured at full lock from top speed on an open road, with the shipped tuning:

| | 2026-09-20 | Now | Rocket Racing, recorded |
| --- | --- | --- | --- |
| Into a drift | after 0.27 s of full lock | on the press | on the press |
| Angle | 20 degrees, 26 held, after a second | 37 degrees at 0.25 s, 52 at 0.5 s | 45 to 55, at 0.35 s |
| Speed after 2 s | 5% lost, 9% held | none, 6% held | none |
| Back into line | 0.65 s after straightening | 0.5 s | about 0.2 s |
| Boost | to 52 m/s, 30% over | to 48 m/s, 20% over | 21% over |

#### What drifting is worth

Measured with the shipped tuning, the autopilot lapping each circuit twice: once never touching the
drift button, once tapping it into the turns. The rows below the skyway were measured with the
models of their time.

| Track | Without the button | With it |
| --- | --- | --- |
| **skyway** (34 m, relief from 3 to 47 m) | 49.9 s | **46.0 s** |
| skyway, with the drift of 2026-09-20 | 49.9 s | 46.9 s |
| esplanade (28 m, bottlenecks of 13 to 15 m), no longer shipped | 41.5 s | 39.2 s |
| esplanade, as it was before v0.1.16 | 48.4 s | 46.6 s |
| four-corners (16 m), no longer shipped | 18.2 s | 17.2 s |
| hippodrome (16 m), no longer shipped | 14.9 s | 14.8 s |

The hippodrome was the thin margin, and it should have been: its turns were 38 m sweepers taken
nearly flat, where tightening the line buys little and the drag costs real speed. The skyway is the
opposite, slaloms and bowls with hardly a straight, and drifting is worth 3.8 s a lap on it, 8%,
the most of any circuit so far: ten drifts, 24 s of the 46 spent sliding, and a boost out of every
one. Without the button, the autopilot never turns hard enough on it to break away.

The rule `shipped_tracks_are_drivable` enforces is what holds whatever the table says: on every
shipped circuit the drifting lap must be the faster one, and a tuning or a road where holding the
button stops paying fails CI.

### Accelerating, no brake

The game is full speed all the time: there is **no brake and no reverse**, only an accelerate
button. Acceleration fades linearly to zero at `top_speed`, so the car approaches top speed
smoothly instead of hitting a ceiling. Letting go of the button coasts to a stop.

Pressing the accelerator right at the start of a race gives up to `start_boost` seconds of boost,
graded from S to E (see [Lobbies](lobbies.md#the-start)).

### Inputs

Inputs are quantized (booleans to accelerate and to drift, an `i8` for steering) before they reach
the simulation. They are a few bytes on the wire, and both sides simulate from the same integers
instead of floats that could differ in their last bits.

### Slopes

The road is a height field over the plane (see [Tracks](tracks.md#relief)): the car still moves in
two dimensions, and the track tells it the height and uphill direction under it, the relief and the
banking together. Each tick, gravity
pulls the car downhill by `gravity × gradient / (1 + |gradient|²)`, the horizontal part of the pull
on a body resting on the surface. It comes before the grip, which then holds against it the way
tires would.

In a banked turn that pull points to the inside of the turn: a slow or coasting car drifts down
the slope, and at speed it helps hold the line, a little. With the current grip the effect is
modest (the autopilot lapped the banked hippodrome, a circuit the game no longer ships, in 15.02 s
against 15.12 s flat); `gravity` in `car.ron` scales it, and 0 makes the relief and the banking
purely visual.

Along the road the same pull slows a car climbing and speeds one diving. Flat out, a climb costs
speed where the engine's push, which fades to nothing at top speed, meets the pull: on a 15% climb
the shipped car settles at 37 m/s instead of 40. A dive does not carry it far past top speed:
above it, the accelerator pulls the car back at the coasting rate, 4 m/s², more than the 3.8 m/s²
the steepest centerline a track may have pulls it forward with. Only a boost goes faster downhill
than on the flat.

Only the planar motion is simulated, and that pull is all the slopes do: cars never leave the road,
and a crest taken flat out does not throw them into the air.

### Walls

The road is bounded by walls at half the width from the centerline — the width the road has at the
car, which a bottleneck pinches in (see [Tracks](tracks.md#bottlenecks)). The car collides as a
circle of `collision_radius`: if it overlaps a wall it is pushed back along the direction from the
centerline, the speed going into the wall is removed (a `wall_bounce` share of it comes back), and
scraping along the wall slows the car down (`wall_friction`). Where the road is pinching in, that
push is the bottleneck squeezing a car that hugs the wall toward the middle of the road.

Cars do not collide with each other yet.

### Race progress

How far a car has driven, which lap it is on and how cars rank are in `sim::race`, described with
the race rules in [Lobbies](lobbies.md#progress-laps-and-finishing).

## Tuning

Every number above lives in `CarTuning` (see the field documentation in `sim/src/car.rs`).
The server loads it from `server/data/car.ron`, shared by every lobby, and reloads it within a
second of every save, so the
feel can be tuned while driving, without recompiling or reconnecting (see
[Game data](architecture.md#game-data)).
`CarTuning::validate` rejects values that would break the simulation, such as a zero radius.

## Autopilot

`sim::autopilot` drives toward a point ahead on the centerline, at full speed. It is not an
opponent: it exists so tests can prove a circuit is drivable — a full lap, fast enough, without
touching a wall — and so the client can drive itself for unattended checks.

It drives in one of two styles, and they differ only in the button: `Style::Grip` never touches
it, `Style::Drift` taps it into the turns. Gripping can still drift, since turning hard enough
breaks the car away without the button, though on the skyway it never turns that hard; so the two
laps measure what the button is worth.

Both decide from the road ahead rather than from their own steering: the drifting style taps the
button, with the stick into the turn, when the road turns by more than 0.25 rad over the next
16 m. A press is a press, so a tap that came too early to start anything is let go of and made
again. `LapReport` counts drifts, the longest one, and ticks spent drifting and boosting.

In a drift the stick sets the curve the travel follows, as it sets the nose's rotation while the
car grips, on a radius of the same kind, so the one pursuit drives both: full lock for 0.25 rad
between the heading and a point on the centerline ahead. A drift aims 0.3 s ahead rather than
0.5 s, which at drifting speeds cut through the inside of a hairpin, and holds the button only
while the pursuit asks for more than full lock into the drift, since holding costs speed. Coming out
of the turn, the stick straightens, the body comes back into line, and the drift pays its boost.
The shipped-track test requires a boosting lap on every track, faster with the button than without,
and clean but for a graze: a tuning or a track where drifting stops paying fails CI.

`drive_lap` can make the autopilot decide from a car state several ticks old, as a client does
through snapshots and interpolation. A steering controller that works on live state can oscillate
into the walls once its information is late, so the tests check both: every shipped circuit must
have one style that laps it cleanly six ticks late. On the hippodrome, which the game no longer
ships, the autopilot stayed clean up to about 15 ticks (250 ms) of delay and started hitting walls
around 18. The esplanade, with its bottlenecks, left late information less room: six ticks late,
its gripping lap stayed clean (40.8 s), but the drifting one scraped into every bottleneck, 19 ticks
against the walls in a lap. A drift decided on old information runs wider, and a bottleneck is
where that shows. The skyway, 34 m wide and never pinched, is clean six ticks late in both styles:
50.0 s gripping, 46.3 s drifting.

That margin also turned out to be a latency detector. The first drivable client displayed the race
770 ms late because of a time-origin bug, and the autopilot, fine in every test, crashed on screen.
A player would have felt the same delay without being able to name it.

## Tests

- Geometry: circuits close, have the expected length, turn by their angles to within a millimeter,
  and projections find the right distance and side; a road running over itself is found.
- Car: acceleration toward top speed, coasting to a stop, steering direction, turning on the very
  tick the stick moves, gently, then a little harder every tick after, turning away from a
  wall while stopped against it, walls holding, a banked turn pulling a standing car to its inside,
  a climb slowing the car and a dive speeding it up, input quantization, tuning validation.
- Drift: a press of the button starts one at once toward the stick, but not with the stick
  centered nor too slow; turning hard starts one without the button and gentle steering does not,
  toward the side the car came round; the body swings most of the way across in a third of a
  second, further with the stick further into the drift; the nose turns without jerking, its
  rotation changing little from one tick to the next into a drift, easing the stick and
  straightening; the car travels the curve the stick asks
  for without running wide; a drift keeps its speed, and only the button brakes it, harder the
  longer it is held; straightening ends it and pays out, unless the button is held; the button
  held with the stick against the drift carries it over to the other side with its charge; a
  boost pushes hard even at top speed; a short drift gives nothing; one into a wall is lost, the
  body settles back and the held button starts nothing.
- Start: grades follow the distance to the start either way, from the press that counts (held
  through the start, pressed again late, or never pressed).
- Autopilot: a full lap of an oval without touching the walls, from live state and with 3, 6 and 9
  ticks of delay, and of a banked oval; drifting laps that drift and boost.
- Banking: the parabolic cross-section, a slope matching the height change, the smooth rise along
  transitions, flat surfaces on flat tracks.
- Relief: the road passes through every height with a grade that never jumps, and across the start
  line too; the slope along the road matches the height change, in a turn and along a transition,
  and climbs more steeply on the inside of a turn; a single height lifts the whole road; heights out
  of order, off the lap or too steep are refused.

## Determinism

The simulation uses `f32` and trigonometry from the platform's math library, which is not
guaranteed to give identical last bits on every platform (native versus wasm, for instance). The
server is the authority, so a client that drifts by a few bits is corrected by the next update.
If exact replay across platforms ever matters, `glam` can be switched to its `libm` backend.
