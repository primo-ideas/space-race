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
   wheel does not: the change of radius when a drift starts or ends. The car always steers at
   least as if it went `min_steering_speed`: there is no reverse, so this is what lets a car stopped
   against a wall turn away from it.
2. **The car's weight** then holds the rotation back: it changes by no more than
   `yaw_acceleration` per second, gathering steadily and dying away the same way when the stick
   lets go (see [Weight](#weight)).
3. **The velocity is split** along the new heading. The part along the heading is driven by
   accelerating or coasting. The sideways part, whatever the rotation left behind, decays at
   the rate set by `grip`.
4. **Walls** push the car back and absorb the speed going into them.

Because the sideways part decays instead of vanishing, the car already slides a little in turns
and loses some speed doing so. **Drift** builds on that slide.

### Weight

The car is heavy since v0.1.14, at the user's request ("add some heaviness to the gameplay"),
in all four of the ways they were offered: momentum, weight in the turns, a heavier wheel, and
weight that shows. The first three are the simulation's, and every one of them is a number in
`car.ron`; the last is the client's, the body on springs and the camera that feels them (see
[Architecture](architecture.md#displaying-the-race)).

- **A heavier wheel.** `yaw_acceleration`, 250 degrees per second per second, caps how quickly the
  rotation changes. It still starts on the very tick the stick moves, which is what the user asked
  for on 2026-09-28, but it gathers steadily rather than at once, and dies away steadily once the
  stick lets go, so the car carries on round a little. Small corrections never meet it: half lock
  at top speed turns the car at 12 degrees per second after 50 ms and 25 after 100 ms, against 15
  and 26 before. Full lock does, 12 and 25 against 30 and 51, and a slalom does, the rotation
  taking about 0.6 s to swing from one side to the other. `steering_rate`, the wheel itself, stays
  at the 14 the user had settled on: lowered to 10 or 8 as well, it made the car one the autopilot
  could no longer drift cleanly from information six ticks old.
- **Weight in the turns.** `grip` is down from 10 to 6, so the travel trails the nose by a sixth
  of a second rather than a tenth: the car is carried further to the outside before it follows its
  nose, half lock puts the nose 6 degrees off the travel instead of 3, and a turn let go of carries
  the travel on 11 degrees instead of 8. A drift is not affected: it goes where the stick says, as
  the user wanted it in v0.1.13. Breaking away by turning hard moved with the grip, from 5 degrees
  to 9, so full lock at speed still breaks away and half lock still does not. Since v0.1.32
  the grip is 9 and the angle 7: the user found the car carried out too far and too fast (see
  [Less carried out](#less-carried-out)).

- **Momentum.** The car takes its time getting up to speed, `acceleration` down from 22 to 16:
  0 to 30 m/s in 3.5 s instead of 2.5, and to 95% of top speed in 7.5 s instead of 5.4. Above top
  speed it keeps its momentum, and a wall costs more (see [Accelerating](#accelerating-no-brake)
  and [Walls](#walls)).

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
  A press is a press: a button held down since before starts nothing. It is remembered for
  `drift_press_buffer`, 0.25 s since v0.1.35 (0.15 before): a thumb often lands on the button a
  moment before the stick has left the center, or before the car is fast enough, and that press
  used to start nothing, with nothing to say why. Turning hard enough starts
  one too, without the button: while the car grips, `Car::slip` is the real angle between its nose
  and its travel, and its reaching `drift_entry_angle` breaks the car away toward the side it came
  round. With the shipped tuning, the entry sits at 5 degrees: at top speed full lock breaks away
  after 0.32 s and three quarters lock after 0.35 s, six tenths never does (see
  [Into a slide more easily](#faster-and-into-a-slide-more-easily)).
- **The angle.** The body swings toward `drift_angle` times `1 - (1 - s)²`, where `s` is how far
  the steering points into the drift: most of the angle with the first half of the steering, all
  of it at full lock, none with the steering centered or against the drift. The curve eases in
  from the center, where a square root, tried first, swung the body on every twitch of the stick.
  The body **unscrews**: its swing gathers speed at `drift_swing_acceleration` and no faster, up to
  `drift_swing_rate`, and slows down the same way to stop on its angle, so it comes off the travel
  on the very tick the drift starts, slowly, then faster and faster, and never lunges, whether the
  drift starts, the stick eases or the drift ends. It brakes on a curve that allows for the tick,
  so it lands on its angle instead of hunting around it. With the shipped 36 degrees, 150 degrees
  per second per second and 45 per second (since v0.1.35; 42, 260 and 75 in v0.1.32 and v0.1.33,
  55, 450 and 110 before), a tap at full lock has the body 6 degrees across after a quarter of a
  second, 17 after half a second, 28 after three quarters and all 36 at 1 s. Since v0.1.37 the
  swing has a **knee**: past `drift_swing_knee` (15 degrees) the body swings further out at no
  more than `drift_swing_knee_rate` (half) of its fastest swing, reaching that slower rate at its
  acceleration rather than at once, and swings back into line unhindered: 6, 17, 23 and 29
  degrees at 0.25, 0.5, 0.75 and 1 s (see [Faster still](#faster-still-and-a-knee)). When a drift
  starts, the body takes over whatever slide the car had and the heading becomes the travel, so
  nothing on screen jumps.
- **Turning.** The stick sets a **signed curvature**: fully to one side turns on
  `drift_radius_tight`, centered runs straight, whichever side the drift is on. That is the curve
  of a drift under way: from the press, the curve tightens from the gripping car's to it as the
  body swings out, so a drift turns harder as it gets going rather than all at once, and a car
  breaking away by turning hard carries on on the curve it was gripping on. The curve follows the
  wheel at `drift_steering_response`, almost at once, and the travel keeps all of its speed as it
  turns.
- **Speed.** A drift costs nothing: without the button, the car keeps its speed however long it
  drifts, as in the recording.
- **The button.** Never needed after the press, and holding it does three things. It multiplies
  the drift radius by `drift_button_tighten`, pulling the turn tighter than the stick alone can,
  more and more from the press until it has been held `drift_button_tighten_time`: the tap of a
  thumb that starts a drift, a tenth of a second or so, barely tightens it, where it used to pull
  the turn 40% tighter for as long as the tap lasted and let it go again. It
  keeps the drift alive when the body comes back into line, so a drift can be carried down a
  straight; and with the stick more than half way against the drift, it carries the drift over to
  the other side, charge and all, so an S is one drift, the body swinging twice its angle across
  in about 1.2 s. And it costs speed: `drift_button_drag`
  from the moment it is pressed, plus `drift_button_drag_ramp` for every second it stays held, so a
  drift tightened for long brakes harder and harder.
- **Charging.** The drift builds a charge: seconds of drift, counted from half to one and a half
  times real time as the stick goes from centered to fully into the drift. Steering out of the
  drift charges at the slow end, never backwards.
- **Ending.** The body back within `drift_exit_angle` of the travel, with the stick keeping it
  there, centered or against the drift, ends the drift and pays out, unless the button is held.
  With the shipped tuning, a drift ends 0.8 s after the stick straightens (0.6 s before v0.1.35,
  the body now swinging back as slowly as it swings out), as the steering unwinds and the body
  swings back into line. Falling
  below `drift_min_speed` ends it and pays out too.
- **Boost.** Ending the drift converts the charge beyond `drift_min_charge` into
  `drift_boost_rate` seconds of boost per second, up to `drift_max_boost`: with the shipped tuning,
  the full 1.2 s of boost takes two to three and a half seconds of drift. A boost raises the top
  speed to `boost_top_speed` and pushes at `boost_acceleration`, in full up to `top_speed` and
  fading out above it, even without the accelerator. After it, the car keeps its momentum: what
  is left above `top_speed` fades at `overspeed_drag` (see
  [Accelerating](#accelerating-no-brake)). The boost was raised on 2026-11-11 at the user's
  request ("Increase the boost out of a drift"): to 52 m/s from 48, three tenths above `top_speed`
  where the recording's fifth had been kept, pushing at 45 m/s² instead of 35, for up to 1.2 s
  instead of 1, and charged a quarter faster. What a full boost adds over top speed went from 8
  to 12 m/s, and the skyway's drifting lap from 45.9 to 44.4 s before any turbo. At 54 m/s the
  autopilot was still clean from live state but spent a second against the walls when six ticks
  late, so 52 is where it stopped. Since v0.1.37 it is 75 m/s at 65 m/s² (58 at 50 in v0.1.35), still three tenths
  above a `top_speed` raised to 58.
- **Losing it.** A drift that hits a wall ends at once and pays nothing. The body settles back
  into line on its own, and the button, still held, starts nothing until it is pressed again. A
  graze is forgiven: meeting the wall at less than `drift_wall_tolerance`, 3 m/s into it, keeps
  the drift, though it charges nothing on the ticks it touches. The tail of a car swung 36 degrees (55 then)
  across the road brushes a wall often, and losing two seconds of charge to it was a punishment
  out of proportion.

The car state carries all of it (`slip`, `body`, `drift`, `drift_charge`, `drift_held`,
`drift_button`, `drift_press`, `boost`), so the simulation stays one deterministic step, and the client predicts
a drift exactly as it predicts the rest.

The tuning stays on the server, so the snapshots carry two gauges the server works out for the HUD:
the boost that ending the drift now would give, and the boost left, both as shares of
`drift_max_boost`. The client draws the first twice: as the green DRIFT gauge over the speed, and
as Rocket Racing does, an arc of green segments on the road around the back of the player's car,
lit from the middle outward as the charge builds and gone when the car is not drifting. The arc
follows the travel rather than the body, so it stays under the car and square to the camera.

### Turbo

Drifting fills a second thing, as in Rocket Racing: the **turbo meter**, three charges
(`TURBO_CHARGES`), which a press of the turbo button spends one at a time (2026-11-11, the user's
request "Add a turbo bar").

- **Filling.** Every second of drift charge fills `turbo_fill` charges, 0.2 with the shipped
  tuning: a charge of turbo for five seconds of charge, which a drift at full lock builds in 3.3 s
  and one barely steered in 10. It fills as the drift goes, not when it ends, so the meter is seen
  to move, and a drift lost to a wall keeps what it had filled. The skyway's ten drifts fill a
  little over two charges a lap.
- **Firing.** A press with at least one whole charge spends it on `turbo_seconds` of boost, 2 s,
  toward `turbo_top_speed`, 87 m/s (67 in v0.1.35, 60 before), half as much again as `top_speed`,
  pushing at `turbo_acceleration`, 80 m/s² (62, 55), in full up to `top_speed` and fading out toward its own top speed
  as a boost's does. A press is a press: holding the button fires once. A press while a charge is
  firing does nothing and spends nothing, and so does one with less than a whole charge.
- **With a boost.** The turbo is the stronger of the two and takes over while it fires; a drift's
  boost running under it runs out unused. Drifting through a turbo is allowed, and fills the
  meter again.
- **After it.** The car keeps its momentum as after any boost, the excess over `top_speed` fading
  at `overspeed_drag`.

The meter starts every race empty, the cars being put back on the grid. It is carried in the car
(`turbo`, `turbo_boost`, `turbo_button`), so the client predicts a turbo as it predicts a drift,
and a snapshot adds the gauge of what is left of the charge being fired. The HUD draws the meter
as three bars over the speed (see [Interface](ui.md)).

To be judged by the user: how quickly it fills, what a charge is worth, and whether a wall should
cost some of it.

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
7. **2026-10-03.** When steering, the user said, the drift waited a little, then turned too hard:
   rather than resisting, it should unscrew at once, slowly, then more and more. Traced tick by
   tick, the spring of the step before pulled hardest at the start, so the body barely moved for
   a few ticks and then lunged: tapped at full lock with a press of a tenth of a second, the
   nose's rotation peaked at 288 degrees per second after 0.12 s and fell back under 100, the tap
   itself pulling the turn 40% tighter while it lasted. Broken into by turning hard, the car
   gripped for a quarter of a second, then the drift's tighter curve and the spring came in on
   the same tick, the rotation jumping by 93 degrees per second. The body's swing now gathers
   speed at a steady rate, up to a fastest swing; the curve tightens from the gripping car's to
   the drift's as the body swings out; and the button's tightening comes in over 0.4 s of
   holding. The nose now turns at 53 degrees per second after 50 ms, 150 after 0.18 s and at most
   192, from a quarter of a second to half a second, changing by no more than about 18 degrees per
   second from one tick to the next, whether the drift is tapped into or broken into. The price is
   a swing of three quarters of a second rather than 0.4, and an S carried through on the button
   in 1.2 s rather than half a second.

Measured at full lock from top speed on an open road, with the shipped tuning, tapping the button
for a tenth of a second:

| | 2026-09-20 | 2026-09-28 | 2026-10-03 to v0.1.31 | Rocket Racing, recorded |
| --- | --- | --- | --- | --- |
| Into a drift | after 0.27 s of full lock | on the press | on the press | on the press |
| Angle | 20 degrees, 26 held, after a second | 37 degrees at 0.25 s, 52 at 0.5 s | 1 degree at 50 ms, 15 at 0.25 s, 44 at 0.5 s, 55 at 0.75 s | 45 to 55, at 0.35 s |
| The nose's turn after 50 ms and 100 ms | | | 35 and 70 degrees per second, 53 and 96 before the car had weight | |
| The nose's fastest turn | | 288 degrees per second, at 0.12 s | 193, from 0.25 s to 0.5 s | |
| Speed after 2 s | 5% lost, 9% held | none, 6% held | none, 7% held | none |
| Back into line | 0.65 s after straightening | 0.5 s | 0.55 s | about 0.2 s |
| Boost | to 52 m/s, 30% over | to 48 m/s, 20% over | to 48 m/s, 20% over | 21% over |

Since v0.1.32 the angle is 42 degrees and slower to come: 9 at 0.25 s, 27 at 0.5 s, 41 at 0.75 s
(see [Less carried out](#less-carried-out)).

#### Less carried out

On 2026-11-11 the user found that the car "déporte trop et trop vite quand on drift et/ou
tourne", carried out too far and too fast, and asked to refine it, heavier still if need be
(v0.1.32). Measured first, with a scratch tool outside the repository that steps the shipped car
from top speed on an open road and laps the skyway, two things carried it out, and the weight was
not one of them:

- **Gripping, the travel trailed the nose too far.** At `grip` 6, three quarters lock ran the
  travel 8.4 degrees off the nose, half lock 5.7: the car went wide of where it pointed. Full lock
  took the slide past `drift_entry_angle` (9) in 0.45 s, and the car broke into a drift.
- **Drifting, the body was thrown across.** The chase camera looks along the travel and the car is
  drawn along its body, so the body swinging out is what the player sees as the car being carried
  sideways: 15 degrees at 0.25 s, 43 at 0.5 s, 55 at 0.75 s, at up to 110 degrees per second.

The path of a drift was not wide: it follows the stick, as it has since v0.1.3.

| At top speed | Before | v0.1.32 |
| --- | --- | --- |
| Travel off the nose, half lock, after 1 s | 5.7 degrees | 3.7 |
| Travel off the nose, three quarters lock, after 1 s | 8.4 degrees | 5.5 |
| Full lock without the button breaks into a drift | after 0.45 s | after 0.52 s |
| Body across the travel, a tap at full lock, at 0.25, 0.5, 0.75 s | 15, 43, 55 degrees | 9, 27, 41 |
| The body's fastest swing | 110 degrees per second | 75 |
| Skyway, gripping, live and six ticks late | 52.6 s, 52.8 s, clean | 51.1 s, 51.3 s, clean |
| Skyway, drifting, live | 41.5 s, clean | 41.3 s, clean |
| Skyway, drifting, six ticks late | 45.1 s, 35 ticks against a wall | 43.2 s, 3 ticks |

The numbers: `grip` 6 to 9, `drift_entry_angle` 9 to 7 so that full lock still breaks away (the
user's rule of 2026-09-20) and three quarters lock still does not, `drift_angle` 55 to 42,
`drift_swing_acceleration` 450 to 260 and `drift_swing_rate` 110 to 75. Tried and left: more
weight in the rotation, `yaw_acceleration` 250 to 225 or 200, barely changed the carry (half a
degree) and made the late autopilot scrape 40 to 136 ticks; `grip` 10 kept full lock from ever
breaking away. The rotation itself is unchanged: full lock still turns the drifting car at about
87 degrees per second, and holding the button at 121. All of it is in `car.ron` and reloads while
driving.

#### Faster, and into a slide more easily

On 2026-11-12 the user asked for three things (v0.1.35): the pods faster by default, the car
carried sideways less fast once it is in a slide, and a slide easier to get into. Measured with a
scratch tool outside the repository, as for v0.1.32, stepping the shipped car from top speed on an
open road and lapping the skyway:

| | v0.1.32 to v0.1.34 | v0.1.35 |
| --- | --- | --- |
| Top speed | 40 m/s, 144 km/h | 45 m/s, 162 km/h |
| 0 to 30 m/s, and to 95% of top speed | 3.5 s, 7.5 s | 2.75 s, 7.5 s |
| Full lock without the button breaks into a drift | after 0.52 s | after 0.32 s |
| Three quarters lock without the button | never | after 0.35 s |
| Body across the travel, a tap at full lock, at 0.25, 0.5, 0.75, 1 s | 10, 29, 41, 42 degrees | 6, 17, 28, 36 |
| The body's fastest swing | 75 degrees per second | 45 |
| A drift ends after the stick straightens | 0.6 s | 0.8 s |
| Skyway, gripping, live and six ticks late | 51.1 s, 51.3 s, clean | 45.3 s, 45.6 s, clean |
| Skyway, drifting, live | 41.3 s, clean | 36.6 s, clean |
| Skyway, drifting, six ticks late | 43.2 s, 3 ticks against a wall | 42.3 s, 3 ticks |

- **Faster.** `top_speed` 40 to 45, and `acceleration` 16 to 18 with it: the acceleration fades
  out toward top speed, so their ratio is how heavy the car feels, and it is kept, the car taking
  as long as before to reach its top speed, just a higher one. Boost and turbo follow at the same
  shares of it: `boost_top_speed` 52 to 58 (`boost_acceleration` 45 to 50), `turbo_top_speed` 60 to
  67 (`turbo_acceleration` 55 to 62). The turning radii stay, so the car turns as tightly in
  meters and faster in degrees per second: the drifting car at full lock at 99 degrees per second
  instead of 87.
- **Carried sideways less fast.** As in v0.1.32, what is seen as the car being carried sideways is
  the body swinging across the travel the chase camera looks along: `drift_swing_acceleration`
  260 to 150 and `drift_swing_rate` 75 to 45, and `drift_angle` 42 to 36, so that at the higher
  speed the car slides across its own body no faster than before (26 m/s at full swing, against
  27). The price is the end of a drift, the body swinging back as slowly: 0.8 s after the stick
  straightens instead of 0.6. A `drift_exit_angle` of 10 or 12 to win it back made the autopilot
  six ticks late scrape the walls for 46 to 86 ticks, so it stays at 8.
- **Into a slide more easily.** `drift_entry_angle` 7 to 5, so three quarters lock breaks away as
  well as full lock, and sooner; the higher speed adds to it, a car sliding further off its nose
  the faster it turns. `drift_press_buffer` 0.15 to 0.25 s, so a press of the button made a
  little earlier still counts, and `drift_min_speed` 15 to 12, so a car slowed by a wall can slide
  again sooner. The autopilot that never touches the button now breaks away once a lap on the
  skyway, for about 1.3 s.

#### Faster still, and a knee

The same day the user asked for the game faster again, picking 30% from the choices offered, and
for the car to be carried across "less directly past a certain angle" of steering or drift
(v0.1.37). Measured with the same scratch tool:

- **30% faster.** `top_speed` 45 to 58 m/s (209 km/h), `acceleration` 18 to 23.2 (the same 7.5 s
  to 95% of it, 0 to 30 m/s in 1.8 s), boost 75 m/s at 65 m/s², turbo 87 m/s at 80 m/s². Alone,
  that broke the car: sliding grows with speed, so half lock broke into a drift, and the
  autopilot six ticks late spent 34 ticks against the walls gripping and 259 drifting. Three
  numbers bring the slide back to what it was at 45 m/s: `grip` 9 to 11.6 (the same 9 × 58 / 45),
  `turn_radius_fast` 32 to 38 and `drift_radius_tight` 26 to 30 m, the car turning a little wider
  in meters and faster in degrees per second than before (full lock at top speed 87 degrees per
  second gripping, against 81). Full lock breaks away at 0.35 s, three quarters lock at 0.52 s.
  `drift_entry_angle` 4.5, to keep three quarters lock at 0.37 s, made the late autopilot scrape
  in both styles, so it stays at 5.
- **The knee.** Two new numbers, `drift_swing_knee` and `drift_swing_knee_rate` (see
  [Drift](#drift)): the slide comes as directly as before up to 15 degrees across, then grows at
  half the rate. At 18 degrees and 0.4 it held back about as much and the late autopilot scraped
  more; at 15 and 0.5 the drifting autopilot six ticks late improved (97 ticks against a wall
  instead of 134 without the knee). The body still reaches its full 36 degrees, a quarter of a
  second later.

| At 58 m/s | No knee, `grip` 9, radii 32 and 26 | Shipped |
| --- | --- | --- |
| Half lock without the button | breaks away at 0.45 s | never |
| Body across, a tap at full lock, at 0.5, 0.75, 1 s | 17, 28, 36 degrees | 17, 23, 29 |
| Skyway, gripping, live and six ticks late | 30.3 s clean, 45.0 s with 34 ticks | 36.5 s, 36.2 s, clean |
| Skyway, drifting, live | 29.4 s, clean | 29.4 s, clean |
| Skyway, drifting, six ticks late | 65.0 s, 259 ticks | 41.5 s, 97 ticks |

The drifting autopilot six ticks late is what a client's `--autopilot-drift` drives, and at this
speed it scrapes: unattended checks on the client still finish their laps, but the clean lap a
client has is the gripping one. The server's autopilot, which drives a finished car, sees the car
live and is clean.

#### What drifting is worth

Measured with the shipped tuning, the autopilot lapping each circuit twice: once never touching the
drift button, once tapping it into the turns. The rows below the skyway were measured with the
models of their time.

| Track | Without the button | With it |
| --- | --- | --- |
| **skyway** (34 m, relief from 3 to 47 m) | 36.5 s | **29.4 s** |
| skyway, at 45 m/s (v0.1.35 and v0.1.36) | 45.3 s | 36.6 s |
| skyway, at 40 m/s (v0.1.32 to v0.1.34) | 51.1 s | 41.3 s |
| skyway, before the handling was carried out less (v0.1.31) | 52.6 s | 41.5 s |
| skyway, the same without firing the turbo | 52.6 s | 44.4 s |
| skyway, before the bigger boost and the turbo (v0.1.22) | 52.6 s | 45.9 s |
| skyway, before the car had weight (v0.1.13) | 49.9 s | 45.9 s |
| skyway, with the drift of 2026-09-20 | 49.9 s | 46.9 s |
| esplanade (28 m, bottlenecks of 13 to 15 m), no longer shipped | 41.5 s | 39.2 s |
| esplanade, as it was before v0.1.16 | 48.4 s | 46.6 s |
| four-corners (16 m), no longer shipped | 18.2 s | 17.2 s |
| hippodrome (16 m), no longer shipped | 14.9 s | 14.8 s |

The hippodrome was the thin margin, and it should have been: its turns were 38 m sweepers taken
nearly flat, where tightening the line buys little and the drag costs real speed. The skyway is the
opposite, slaloms and bowls with hardly a straight, and drifting is worth 11.1 s a lap on it, 21%,
the most of any circuit so far: ten drifts, 22 s of the 41.5 spent sliding, a boost out of every
one, and two charges of turbo fired on the way, worth 2.9 s between them (at 45 m/s since v0.1.35,
8.7 s, 19%, three charges fired). Without the button, the autopilot never turned hard enough on it
to break away until v0.1.35, and now does once a lap. The car's
weight (v0.1.14) widened the gap from 3.9 s: the gripping lap lost 2.7 s, to the slower start, the
heavier wheel and the looser grip, while the drifting lap lost nothing, its boosts now lingering
where they used to be gone in two seconds.

The rule `shipped_tracks_are_drivable` enforces is what holds whatever the table says: on every
shipped circuit the drifting lap must be the faster one, and a tuning or a road where holding the
button stops paying fails CI.

### Accelerating, no brake

The game is full speed all the time: there is **no brake and no reverse**, only an accelerate
button. Acceleration fades linearly to zero at `top_speed`, so the car approaches top speed
smoothly instead of hitting a ceiling. Letting go of the button coasts to a stop.

Above `top_speed`, the car keeps its **momentum**: whatever put it there, a boost or a dive, the
excess fades by `overspeed_drag`, a share of itself every second, a lot of it quickly and the last
of it slowly, as air would hold back a heavy car. With the shipped 0.5 and a top speed of 45 m/s, a
boost that took the car to 58 m/s leaves it at about 53 a second after it ends and 48 three seconds
after, where before the car had weight it was back at top speed within two seconds. Coasting still applies on top of it when the accelerator is let go.

Pressing the accelerator right at the start of a race gives up to `start_boost` seconds of boost,
graded from S to E (see [Lobbies](lobbies.md#the-start)).

### Inputs

Inputs are quantized (booleans to accelerate, to drift and to fire the turbo, an `i8` for
steering) before they reach the simulation. They are a few bytes on the wire, and both sides simulate from the same integers
instead of floats that could differ in their last bits.

### Steering from each device

The simulation takes one number for the steering, from full left to full right, and three kinds
of hands give it. The client shapes each before it becomes an input (`client/src/controls.rs`,
v0.1.21), so the car underneath is the same for everyone and only what the hands can say differs:

- **A stick** gets a dead zone of 0.1, so a worn stick resting a little off center does not steer
  the car into a slow drift across the road, and full lock at 0.94, which a stick pushed at an
  angle, its thumb also rising, never quite reaches at its rim. Between the two it is in
  proportion, as before: the handling with a gamepad was judged right, and only its ends change.
- **Keys and the D-pad** say left or right and nothing between, so every tap was a full turn of
  the wheel, and at top speed a key held for a moment broke the car into a drift. They now ease in:
  half lock on the press, then 2.5 more per second, full lock 0.2 s later, in steps of a tick so a
  held key sends no more inputs than the server has ticks. A tap is a correction and a held key a
  turn; holding it still breaks into a drift, a little later. Releasing goes straight to the
  center, the car's own `steering_rate` easing it back. With both directions held, the last one
  pressed steers, as on a touchscreen, rather than the two cancelling out.
- **Thumbs on a screen** say how hard by where they rest (see [Interface](ui.md#the-controls-over-the-race)).

Every action has a second key, and by default the arrow keys and Space work as W, A, D and Shift
do, for whoever's hand falls there. The devices still add up: whichever is used drives. Which
key and which button does what is the player's to change ([Interface](ui.md#settings)); what
follows names the defaults.

The turbo is E or the down arrow on a keyboard, never Control, which with W held closes a
browser's tab, and which a page does not let a player bind either; Y, the top face button, on a
gamepad; and the left branch of the cross on a touchscreen. The drift is Shift or Space, and the
right trigger on a gamepad (the user's choice of 2026-11-11; it was X, with the turbo on A or the
left bumper). A trigger counts as held from a third of its travel, well before Bevy calls it
pressed at three quarters: a drift is tapped into, and nobody taps a trigger to its stop.

Each device also says what happened, for eyes that are on the road: a gamepad rumbles on a wall hit,
harder and longer the more speed it took (from a quarter to full strength, 85 to 160 ms, at most
every 0.3 s), with a light tick of its high motor when the car breaks into a drift, a short
roll of both motors when a boost fires, and a hard kick of both, a third of a second, for the
turbo; a phone's vibration motor does the same on Android.

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
the shipped car settles at 36 m/s instead of 40. A dive carries it past top speed, to where the
pull meets the momentum fading: `top_speed` plus the pull over `overspeed_drag`, nearly 5 m/s over
on a 25% grade with the shipped tuning. On the skyway the gripping autopilot reaches 42 m/s at the
foot of the dive, where it never went past 40 before the car had weight.

Only the planar motion is simulated, and that pull is all the slopes do: cars never leave the road,
and a crest taken flat out does not throw them into the air.

### Walls

The road is bounded by walls at half the width from the centerline — the width the road has at the
car, which a bottleneck pinches in (see [Tracks](tracks.md#bottlenecks)). The car collides as a
circle of `collision_radius`: if it overlaps a wall it is pushed back along the direction from the
centerline, the speed going into the wall is removed (a `wall_bounce` share of it comes back), and
scraping along the wall slows the car down (`wall_friction`), for as long as it leans on it: a
car running along a wall without going into it, by less than a centimeter a second, only brushes
it and loses nothing. Where the road is pinching in, that
push is the bottleneck squeezing a car that hugs the wall toward the middle of the road.

The hit itself rubs too: it takes speed along the wall, `wall_impact_friction` m/s for every m/s it
takes into it, as friction does in an impact. A graze costs little and a blow at an angle a lot:
at 40 m/s and 15 degrees, the stick left alone, the shipped car falls to 32 m/s, where it used to
keep 38, and being slow to get back up to speed, it is still more than a meter per second down
four seconds later.

The wall also **deflects** a car that grips (2026-11-11): on every tick it runs into the wall,
its nose comes round toward where the hit sent it, along the wall and a little off it with the
bounce, at `wall_alignment`, 60 per second, nearly all the way in three ticks. Without it the
nose stayed pointed into the wall, the grip kept driving the car back into it, and every tick was
another hit, each rubbing off a fortieth of the car's speed, until the player steered away: left
alone after a hit at 7 degrees and 35 m/s, the test car was down to 15 m/s two seconds later,
and is now at 30. A shallow hit cost more than a square one. The picture does not jump with the
car: the body (`Car::body`) is left pointing where it did and swings back into line in its own
time, the way it takes over the slide when a drift starts. Only the way the car was already going: one
that hits head on, or stands against the wall, is left to turn away by itself. A hit hard enough
to break the car into a drift, from about 9 degrees at speed, already followed the wall, a
drift's heading being its travel; this is for the shallow hits and the slow ones.

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

`sim::autopilot` drives toward a point ahead on its line, at full speed. It is not an
opponent: it exists so tests can prove a circuit is drivable — a full lap, fast enough, without
touching a wall — and so the client can drive itself for unattended checks.

It drives in one of two styles, and they differ only in the button: `Style::Grip` never touches
it, `Style::Drift` taps it into the turns. Gripping can still drift, since turning hard enough
breaks the car away without the button, though on the skyway it never turns that hard; so the two
laps measure what the button is worth.

Both decide from the road ahead rather than from their own steering: the drifting style taps the
button, with the stick into the turn, when the road turns by more than 0.25 rad over the next
16 m. A press is a press, so a tap that came too early to start anything is let go of and made
again. `LapReport` counts drifts, the longest one, ticks spent drifting and boosting, and charges
of turbo fired.

The drifting style also fires the turbo its drifts fill: with a whole charge, out of a drift and
of any boost, where the centerline turns by less than 0.3 rad at every one of four points over
the next 60 m. On the skyway that is two charges a lap, and it ends the lap with part of a third
it found no room to fire.

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
52.8 s gripping, 46.1 s drifting. Since the bigger boost and the turbo, the drifting lap six ticks
late was 45.1 s with 35 ticks against the walls, three charges of turbo fired at 60 m/s on old
information; the gripping one stayed clean, and from live state the drifting one too. Since v0.1.32
(more grip, a smaller and slower swing) it is 43.2 s with 3 ticks against the walls. Weight makes late information costlier: with the wheel slowed as
well as the rotation (`steering_rate` 10 or 8 rather than 14), the drifting autopilot six ticks
late swung from wall to wall through the slaloms, 160 ticks against them in a lap.

That margin also turned out to be a latency detector. The first drivable client displayed the race
770 ms late because of a time-origin bug, and the autopilot, fine in every test, crashed on screen.
A player would have felt the same delay without being able to name it.


### Personalities

Since v0.1.43 (2026-11-13, the user's word "Ajoute de la variance dans les bots, car sur la video,
ca donne pas envie ils se suivent tous"), every autopilot car drives with a `Personality` of its
own, drawn from its car number (`Personality::of`, a hash, so the same car drives the same way on
the server and on a client's screen): where its line runs across the road, as a share of the room
between the walls less 4.5 m (centered anywhere within 40% of it either side), a weave of 8 to 28%
of it either side of that, 220 to 520 m long, how far ahead it looks (0.85 to 1.2 of the neutral
driver), how sharp a turn must be for it to drift (0.75 to 1.35), and how straight the road must be
for it to fire its turbo (0.8 to 1.6). The cars that have finished a race and are driving
themselves home, and a client's `--autopilot`, spread out across the road instead of following
each other down the middle, and take the speed pads or miss them.

The neutral driver, `Personality::default()`, still follows the centerline: it is what the tests
measure circuits with, and it misses every pad of the skyway, which all stand off the middle, so
the laps the tests require are the ones they were. Eight personalities on the skyway, drifting from
live state: laps of 28.1 to 35.0 s (29.2 neutral), at most 12 ticks against a wall; gripping six
ticks late, 32.3 to 36.1 s. A wider range of lines (55%, weaves to 40%) put the widest drivers on
the walls for 28 ticks a lap.

## Tests

- Geometry: circuits close, have the expected length, turn by their angles to within a millimeter,
  and projections find the right distance and side; a road running over itself is found.
- Car: acceleration toward top speed, coasting to a stop, steering direction, turning on the very
  tick the stick moves, gently, then a little harder every tick after, turning away from a
  wall while stopped against it, walls holding, a banked turn pulling a standing car to its inside,
  a climb slowing the car and a dive speeding it up, input quantization, tuning validation.
- Weight: the rotation starting on the very tick and changing by no more than `yaw_acceleration`,
  and a heavy car carrying on round further than a light one once the stick lets go; a dive
  carrying the car past top speed, and a boost's excess fading by the same share every second; a
  blow against a wall costing more than four times a graze.
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
  body settles back and the held button starts nothing; a press made a tenth of a second before
  the stick turns still starts a drift and one made 0.4 s before does not; a graze keeps the
  drift and charges nothing, a hit loses it.
- Turbo: nothing to fire on an empty meter; four seconds of drift at full lock fill more than one
  charge, and the meter stops at three; a press spends one charge, a held button no more, and the
  car goes faster than any boost takes it, up to the turbo's top speed and no further.
- Walls: a wall turns the nose of a car that hits it at a shallow angle along itself, which keeps
  it more than 3 m/s faster two seconds later than a car left with its nose in.
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
