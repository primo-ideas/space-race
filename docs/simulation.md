# Simulation

The driving simulation lives in the `sim/` crate. The server runs it with full authority. It sits
in a crate of its own rather than in `server/` so the client can run the exact same code later, to
predict its own car between server updates; nothing in it knows the wire exists (see
[Architecture](architecture.md#workspace-layout)).

## Conventions

- Cars move on a plane: `x` and `y` in meters, speeds in m/s. Banked roads add a height over that
  plane, which the car follows but does not simulate in three dimensions.
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

1. **Steering** sets a target yaw rate: `speed / turning radius`, scaled by the stick. The turning
   radius grows with speed, from `turn_radius_slow` to `turn_radius_fast`, so full lock stays
   controllable at speed. The yaw rate then follows that target with an exponential response
   (`steering_response`), which smooths digital input without adding noticeable delay on an
   analog stick. The car always steers at least as if it went `min_steering_speed`: there is no
   reverse, so this is what lets a car stopped against a wall turn away from it.
2. **The velocity is split** along the new heading. The part along the heading is driven by
   accelerating or coasting. The sideways part, whatever the rotation left behind, decays at
   the rate set by `grip`.
3. **Walls** push the car back and absorb the speed going into them.

Because the sideways part decays instead of vanishing, the car already slides a little in turns
and loses some speed doing so. **Drift** builds on that slide.

### Drift

Drift is the heart of the gameplay, in the spirit of Rocket Racing, and it is not a mode the player
switches on. **The car drifts when it comes off its axis**: turn hard enough and the rotation puts
the nose ahead of where the car travels, and past `drift_entry_angle` that slide is a drift.
Nothing is held down to start one, and nothing needs to be held to keep one.

`Car::slip` is that angle, between the velocity and the heading. It is the slide itself and not a
drawing of it: the steering rotates the nose, the grip pulls the velocity back toward it, and the
gap between the two is both what the player sees and what the simulation reads.

- **Starting.** Above `drift_min_speed`, the slip angle reaching `drift_entry_angle` starts a
  drift, toward whichever side the nose has come round. With the shipped tuning, full lock at speed
  puts the nose about 7 degrees off the travel and the entry sits at 5, so a firm turn breaks away
  and a gentle one does not.
- **Turning.** The stick sets a **signed curvature**: fully into the drift turns on
  `drift_radius_tight`, centered runs straight, and against the drift steers out of the slide. That
  sign is the whole of the control, and what lets a drift end at all -- a stick that could only
  turn more or less tightly could never bring the car back into line. The rotation follows it at
  `drift_steering_response`, high, so the car comes round at once instead of ploughing on while its
  rotation builds up.
- **Sliding.** The direction of travel swings toward the nose at `drift_grip`, **without losing
  speed**: a drift carries its speed through a turn that gripping would scrub it in. Only the drag
  slows it. `drift_grip` is the one number that decides the feel, and it is a trade: low means the
  car stays off its axis, unscrews into the turn and runs wide doing it; high means it snaps back
  into line and barely slides at all.
- **The button.** Never needed, and it does two things. Held, it multiplies the drift radius by
  `drift_button_tighten`, pulling the turn tighter than the stick alone can, and costs
  `drift_button_drag` in deceleration. Held as the car comes back into line, it keeps the drift
  alive instead of ending it, so a drift can be carried down a straight and into the next turn.
- **Braking.** The drag is `drift_drag` when the drift begins, plus `drift_drag_ramp` for every
  second of charge: the longer a drift is held, the harder it brakes.
- **Charging.** The drift builds a charge: seconds of drift, counted from half to one and a half
  times real time as the stick goes from centered to fully into the drift. Steering out of the
  drift charges at the slow end, never backwards.
- **Ending.** The slip angle falling below `drift_exit_angle` ends the drift and pays out, unless
  the button is held at that moment. Falling below `drift_min_speed` ends it and pays out too. The
  exit angle sits below the entry angle, so a drift does not flicker on and off around one
  threshold.
- **Boost.** Ending the drift converts the charge beyond `drift_min_charge` into
  `drift_boost_rate` seconds of boost per second, up to `drift_max_boost`. A boost raises the top
  speed to `boost_top_speed` and pushes at `boost_acceleration`, in full up to `top_speed` and
  fading out above it, even without the accelerator. After it, the car settles back to `top_speed`
  at the `coasting` rate.
- **Losing it.** A drift that touches a wall ends at once and pays nothing. Nothing more is needed
  to stop it restarting on the spot: a wall scrubs the sideways speed, which is exactly what puts
  the car back on its axis.

The car state carries all of it (`slip`, `drift`, `drift_charge`, `boost`), so the simulation stays
one deterministic step and the client will be able to predict drifts too.

The tuning stays on the server, so the snapshots carry two gauges the server works out for the HUD:
the boost that ending the drift now would give, and the boost left, both as shares of
`drift_max_boost`.

#### Why the slide had to become real

The two models before this one made the slide a drawing. The path followed the heading closely
(`drift_grip` 30, so the travel caught the nose up in a thirtieth of a second) and a separate
angle, turned into the drift and settled back afterward, showed a slide that never moved the car.
That was deliberate: the first model, with a real lag at `drift_grip` 3.5, carried the car about
`speed / drift_grip` -- some 11 m at 40 m/s -- before it turned at all, and it felt thrown to the
outside of every corner.

The cost of drawing the slide was that the car never actually came off its axis, so the drift
needed a button to say when it was on. Once the drift had to start by going off the axis and end by
coming back into it, the angle had to become real: there is nothing else to measure.

That brings the width back, and it is paid in road. At 35 m/s with `drift_grip` 4 the travel is
about a quarter of a second behind the nose, several car widths before the path comes round. On a
16 m road that is a wall; on the 28 m of `esplanade` it is a line. This is why the drift and the
width of the roads were settled in the same change, and why `serpentine`, 18 m wide with turns of
18 to 28 m chained with no straight between them, could not survive it and was removed (see
[Tracks](tracks.md)).

The other side of that is what the esplanade's bottlenecks are for: the road is 28 m wide because a
drift needs the room, and three stretches of it are 13 to 15 m because a drift that takes more room
than it has earned should find a wall.

#### What drifting is worth

Measured with the shipped tuning, the autopilot lapping each circuit twice: once never touching the
drift button, once holding it through the turns. Both laps slide, since going off the axis is no
longer a choice; only the button differs.

| Track | Without the button | Holding it |
| --- | --- | --- |
| **esplanade** (28 m, bottlenecks of 13 to 15 m) | 41.5 s | **39.2 s** |
| esplanade, as it was before v0.1.16 | 48.4 s | 46.6 s |
| four-corners (16 m), no longer shipped | 18.2 s | 17.2 s |
| hippodrome (16 m), no longer shipped | 14.9 s | 14.8 s |

The hippodrome was the thin margin, and it should have been: its turns were 38 m sweepers taken
nearly flat, where tightening the line buys little and the drag costs real speed. The redrawn
esplanade is the opposite, a circuit of nothing but turns, and holding the button is worth 2.3 s a
lap on it, 5.5%, the most of any circuit so far.

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

The road is a height field over the plane (see [Tracks](tracks.md#banking)): the car still moves in
two dimensions, and the track tells it the height and uphill direction under it. Each tick, gravity
pulls the car downhill by `gravity × gradient / (1 + |gradient|²)`, the horizontal part of the pull
on a body resting on the surface. It comes before the grip, which then holds against it the way
tires would.

In a banked turn that pull points to the inside of the turn: a slow or coasting car drifts down
the slope, and at speed it helps hold the line, a little. With the current grip the effect is
modest (the autopilot lapped the banked hippodrome, a circuit the game no longer ships, in 15.02 s
against 15.12 s flat); `gravity` in `car.ron` scales it, and 0 makes banking purely visual.

Only the planar motion is simulated: climbing the bank does not cost speed, and cars do not leave
the road.

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

It drives in one of two styles, and they differ only in the button: `Style::Grip` never holds it,
`Style::Drift` holds it through the turns. Neither can promise not to drift, since going off the
axis is no longer a choice -- a car that turns hard enough slides whatever the style -- so the two
laps measure what the button is worth rather than what drifting is worth.

Both decide from the road ahead rather than from their own steering, which swings as soon as a
drift turns the car harder: a turn of more than 0.25 rad over the next 16 m is taken at full lock,
which is what breaks the car off its axis, and the button is held while the road keeps turning that
way. `LapReport` counts drifts, the longest one, and ticks spent drifting and boosting.

In a drift the stick sets a curvature, not a rotation, and steering from the angle to the target
fails: a gain high enough to correct swings between the tightest and widest drifts, a lower one
lets the car slide into the inside wall. The drifting autopilot compares curvatures instead: the
arc through its target, tangent to its heading, against the one it drives, its yaw rate over its
speed, and pushes the stick into the drift when it needs a tighter arc. It aims 0.3 s ahead rather
than 0.5 s, which at drifting speeds cut through the inside of a hairpin. The shipped-track test
requires a boosting lap on every track, faster with the button than without, and clean but for a
graze: a tuning or a track where holding the drift stops paying fails CI.

`drive_lap` can make the autopilot decide from a car state several ticks old, as a client does
through snapshots and interpolation. A steering controller that works on live state can oscillate
into the walls once its information is late, so the tests check both: every shipped circuit must
have one style that laps it cleanly six ticks late. On the hippodrome, which the game no longer
ships, the autopilot stayed clean up to about 15 ticks (250 ms) of delay and started hitting walls
around 18. The redrawn esplanade leaves late information less room: six ticks late, the gripping
lap is still clean (40.8 s), but the drifting one scrapes into every bottleneck, 19 ticks against
the walls in a lap. A drift decided on old information runs wider, and a bottleneck is where that
shows.

That margin also turned out to be a latency detector. The first drivable client displayed the race
770 ms late because of a time-origin bug, and the autopilot, fine in every test, crashed on screen.
A player would have felt the same delay without being able to name it.

## Tests

- Geometry: circuits close, have the expected length, turn by their angles to within a millimeter,
  and projections find the right distance and side; a road running over itself is found.
- Car: acceleration toward top speed, coasting to a stop, steering direction, turning away from a
  wall while stopped against it, walls holding, a banked turn pulling a standing car to its inside,
  input quantization, tuning validation.
- Drift: turning hard starts one and gentle steering does not, with no button and none too slow,
  and its side is the way the car came round; it turns further and slides more than gripping; the
  button tightens it and costs speed; coming back into the axis ends it and pays out, unless the
  button is held through that moment; it runs wide but within the width of a road built for it;
  holding it brakes harder and harder; a boost pushes hard even at top speed; a short one gives
  nothing; one into a wall is lost.
- Start: grades follow the distance to the start either way, from the press that counts (held
  through the start, pressed again late, or never pressed).
- Autopilot: a full lap of an oval without touching the walls, from live state and with 3, 6 and 9
  ticks of delay, and of a banked oval; drifting laps that drift and boost.
- Banking: the parabolic cross-section, a slope matching the height change, the smooth rise along
  transitions, flat surfaces on flat tracks.

## Determinism

The simulation uses `f32` and trigonometry from the platform's math library, which is not
guaranteed to give identical last bits on every platform (native versus wasm, for instance). The
server is the authority, so a client that drifts by a few bits is corrected by the next update.
If exact replay across platforms ever matters, `glam` can be switched to its `libm` backend.
