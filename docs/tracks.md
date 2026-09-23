# Tracks

A track is described by the centerline of its road, as a list of segments laid end to end: a
turtle drawing the road. The format is defined by `TrackDescription` in
`sim/src/track.rs`, and files are written in [RON](https://github.com/ron-rs/ron).

Track files live in `server/data/tracks/`, one per track, named by its key: the server loads them
all, and the player who creates a lobby picks one. The game ships one circuit today, and nothing
anywhere assumes that: a second file in that directory is a second track on the lobby's track
stepper. Clients receive every track when they connect (see [Protocol](protocol.md#tracks)). Every
shipped track is tested: it must load, and the autopilot must lap it drifting without touching a
wall, faster than gripping (see [Simulation](simulation.md#autopilot)).

`esplanade.ron` is the one circuit the game ships, and the one the drift is drawn for: 28 m from
wall to wall, 1.62 km a lap, nineteen turns and hardly a straight. Most of the turns run straight
into the next one; the rest are joined by twenty or thirty meters of road, and the longest straight
on the lap is the 72 m run to the line, which is there because the starting grid needs somewhere to
stand. Six turns are banked, the tightest is the 28 m hairpin two thirds of the way round, and
three bottlenecks pinch the road to 13, 14 and 15 m (see [Bottlenecks](#bottlenecks)). The drifting
autopilot laps it in 39.2 s without touching a wall, against 41.5 s gripping.

Width is the point. A slide worth having carries the car several meters off its line before the
path comes round (see [Simulation](simulation.md#drift)), and on a 16 m road that is already a
wall. Three other circuits lived here and are gone: `hippodrome.ron`, a 16 m oval that was the
first test track, `four-corners.ron`, a 16 m rounded rectangle, and `serpentine.ron`, 18 m wide
with turns of 18 to 28 m chained without a straight between them. All three were drawn before the
car could really slide, and none of them left room for it; the last of the three was removed rather
than widened, and the first two followed when the esplanade was redrawn to be the circuit the game
is about.

## Format

```ron
(
    name: "Example",
    // Road width from wall to wall, in meters, where nothing narrows it.
    width: 16.0,
    segments: [
        Straight(length: 50.0),
        // Angles in degrees, positive to the left. Transition lengths in meters.
        Turn(angle: 180.0, radius: 40.0, transition: 20.0, banking: 30.0),
        Straight(length: 100.0),
        Turn(angle: 180.0, radius: 40.0, transition: 20.0, banking: 30.0),
        Straight(length: 50.0),
    ],
    // Optional: where the road pinches in (see Bottlenecks).
    narrows: [
        (at: 120.0, length: 30.0, width: 10.0, blend: 20.0),
    ],
)
```

- The track starts **on the start line**, at the origin, heading `+x`, and the segments must bring
  it back there. Putting the start line in the middle of a straight, as above, leaves room for the
  starting grid behind it.
- `Straight(length)`.
- `Turn(angle, radius, transition, banking)`: `angle` in degrees, positive to the left, along an
  arc of `radius`. `transition` and `banking` are optional.

Unknown fields are errors, so a typo such as `transtion` does not silently fall back to the
default.

## Transitions

A plain turn switches from straight to its full curvature at once: the driver has to snap the
stick to a new position at the exact entry point. Real roads avoid that with **clothoids**, curves
whose curvature grows linearly with distance, and so does this format.

With a `transition` of `L` meters, the curvature ramps from zero to `1 / radius` over `L` meters,
stays constant along the arc, and ramps back down over another `L` meters. The whole turn still
turns by `angle`. Compared with the same turn without transitions:

- the arc is `L` meters shorter, but the two clothoids add `2 L`, so the turn is `L` meters longer;
- the turn bulges slightly outward (by about `L² / (12 radius)` across the two ends), which is why
  both turns of an oval must use the same transition to close.

## Banking

`banking` tilts the road of a turn toward its inside, like a velodrome. It is the slope, in degrees,
the road reaches at the **outer** edge.

The cross-section is not a flat tilted plane but a curve: a parabola, flat at the inner edge and
steeper and steeper toward the outer edge. With a half width `w` and a banking angle `β`, the outer
edge stands `w × tan β` above the inner one, and the centerline a quarter of that. On the
esplanade's hairpin (28 m wide, 16°), the outer edge rises 4.0 m. A high line is steeper than a low
one, which gives the driver a real choice of line through the turn.

The banking follows the transitions: it rises from flat to full along the entry transition, stays
full along the arc, and settles back along the exit one. Within a transition it follows a
smoothstep, the cubic that a Catmull-Rom or Hermite spline gives between a flat key and a banked
one: the road starts and finishes rising gently, with no crease where the transition meets the
straight or the arc. That is why a banked turn needs a transition: without one, the road would
have a step.

The centerline itself stays exactly what the clothoids describe. The banking only adds a height to
the road on each side of it: the track is a height field over the plane, so the simulation still
moves cars in two dimensions (see [Simulation](simulation.md#slopes)).

## Bottlenecks

A `narrows` pinches the road in for a stretch, and is what makes a wide circuit a test of
precision: the drift needs room to run wide (see [Simulation](simulation.md#drift)), and a
bottleneck takes that room away exactly where it was being enjoyed.

```ron
narrows: [
    (at: 712.0, length: 30.0, width: 13.0, blend: 28.0),
],
```

- `at`: meters along the lap where the narrow stretch begins, like a prop of the scenery, rather
  than which segment it falls on. It therefore keeps its place when the road is redrawn around it.
- `length`: how far the road stays narrow.
- `width`: from wall to wall along it, at least 8 m and never wider than the track.
- `blend`: how far the road takes to pinch in before the stretch and to open out after it, 24 m by
  default. It follows the same smoothstep as the banking, so the walls start and finish moving
  gently and the road has no crease. A blend of zero gives a step in the wall, which is allowed and
  looks it.

The centerline never moves: both walls come in by the same amount, and the line through a
bottleneck is the line the road already took. A bottleneck is not a corner, it is a corner with the
room taken away. Everything that reads the width reads it where it is — the walls a car is pushed
back from, the road and the walls the client builds, the props beside them, the starting grid, the
camera, the thumbnail the menus draw — so a prop standing against the wall comes in with it, and an
arch over a bottleneck is narrower than one over the open road.

The banking keeps its **slope** through a bottleneck rather than its height: on a narrower road the
same `banking` raises the outer edge less, since the edge is closer to the middle.

Narrows may overlap: the road is as wide as the narrowest of them says. Where nothing narrows it,
`width` is the width, which is why `Track::half_width` is the widest the road ever is and
`Track::narrowest_half_width` what a car has to fit through.

### Placing one

Where a bottleneck starts matters more than how narrow it is. A drift runs wide out of a turn, so
walls that close during a turn's exit meet the car while it is still out there. The esplanade's
gate was first drawn at 694 m, its blend starting inside the exit of the esses: the drifting
autopilot left them 7 m off the centerline, met the walls coming in, and scraped along them for
nine ticks, losing 12 m/s. Moved to 712 m, narrow over the end of the straight and into the fast
right after it, the same 13 m leaves the same drift 2.4 m to spare. The width had nothing to do
with it.

`shipped_tracks_are_drivable` is how a placement is judged: the drifting autopilot must get through
without more than a graze. How close it comes is worth knowing too, and the tightest of the three
is the funnel, where the drift out of the right-hander before it passes 0.6 m from the wall. At
14 m it passed 0.1 m from it, which would have made every change to the car's tuning a coin toss in
CI, so it was opened to 15.

## Scenery

Props stand beside the road: a `scenery` list in the same file, next to the segments that draw it.
They are decoration and nothing else. The simulation never sees them — they stand outside the
walls, where no car can reach — so adding one cannot change how a circuit drives. The format is
`Prop` in `sim/src/track/scenery.rs`; props travel to clients with the rest of the
description, and the client builds them into the same two meshes as the track itself, so a
decorated circuit still costs two draw calls.

```ron
scenery: [
    Gantry(at: 0.0),
    Chevrons(at: 46.0, side: Right, offset: 1.0),
    Grandstand(at: 196.0, side: Right, offset: 7.0, length: 64.0, rows: 8),
    Monolith(at: 120.0, side: Right, offset: 95.0, height: 44.0, width: 14.0),
],
```

A prop is placed the way a marshal would describe a spot on a circuit, never in world coordinates:

- `at`: meters along the lap from the start line.
- `side`: `Left` or `Right`, looking along the driving direction.
- `offset`: meters from the outer face of the wall to the face of the prop that looks at the road.
  Zero leaves it leaning against the wall, and no prop ever overhangs the road.

A prop therefore keeps its place when the road around it is redrawn, and the editor to come can move
one by dragging it along the road. Every size has a default, so a file writes only what it wants
different.

| Prop | What it is | Sizes, with their defaults |
| --- | --- | --- |
| `Gantry` | An arch over the road, its legs just outside the walls. It spans the road, so it takes no `side` and no `offset`. | `clearance` 7 m over the higher wall, `depth` 2 m |
| `Pylon` | A tapered mast ringed with neon, lit on top. | `offset` 3 m, `height` 16 m |
| `Grandstand` | A stepped tribune swept along the road, so it follows the curve, a neon line along the front of every row. | `offset` 5 m, `length` 40 m, `rows` 6 |
| `Billboard` | A panel on two posts, its neon frame and stripes facing the road. | `offset` 4 m, `width` 10 m, `height` 5 m |
| `Chevrons` | A marker board of stacked arrows, facing back down the road at the cars coming. | `offset` 1.5 m, `count` 3 |
| `Monolith` | A tall slab far out in the dark, seamed with neon. | `offset` 70 m, `height` 32 m, `width` 12 m |

Two props read the road rather than the file. A gantry measures its clearance from the higher of the
two walls, so an arch over a banked turn rises with the road instead of cutting into it. A marker
board reads the bend from the centerline a dozen meters either side of it and turns its arrows into
the turn, whichever side of the road it stands on; where the road runs straight the arrows point away
from it, which is the sign of a board in the wrong place.

Nothing checks a prop against the rest of the circuit: a track is built from its own file, and a
prop is only known to stand off the road where it is placed. A circuit doubles back on itself,
though, so a stand or a slab set far out on one turn can land on another part of the lap. The
shipped tracks are checked against their whole centerline by a test in `server/src/content.rs`.

`esplanade.ron` is dressed all the way round, with 83 props: an arch over the line and one over
each of the three bottlenecks, a marker board into every turn, stands around the turns worth
watching — the biggest wrapped around the outside of the hairpin — pylons at the apexes and along
the fast stretches, panels where the eye needs something to measure speed against, and nine slabs
out in the dark, six outside the circuit and three in the infield.

A bottleneck is signed by its own arch and by four pylons standing hard against the walls, two at
each end. Because a prop is placed from the wall and the wall moves, those pylons come in with the
road and draw the gap from far back down the straight.

## Validation

`Track::build` refuses a description, with a message pointing at the faulty segment, when:

- the width, a length or a radius is not a positive number;
- a radius is not larger than half the width, since the inner edge of the road would fold over
  itself;
- the transitions are longer than the turn itself;
- a banking is outside 0 to 60 degrees, or a banked turn has no transition;
- the angles do not add up to one full turn, left or right;
- the centerline does not come back within 0.5 m of the start line;
- the road runs over itself: two centerline points less than a road width apart, although further
  apart along the loop than `π × width / 2`. The tightest turn allowed has a radius of half the
  width, and half a turn of it is that long and brings the centerline back exactly a width apart,
  so closer points are not a turn but another part of the road;
- a narrows sits off the lap, is not a positive number of meters long, pinches the road below 8 m
  or wider than the track itself, has a negative blend, or is longer than the lap with both its
  blends. The message names it by its place in the list;
- a prop of the scenery stands off the lap, over the road (a negative `offset`), or has a size that
  is not a positive number of meters. The message names the prop by its place in the list.

Smaller gaps than that are integration or rounding error, and are spread evenly along the loop so
the road closes exactly. Comparing every pair of points costs about half a million distances on a
drift circuit, once when the track is built.

## Built geometry

The built `Track` samples the centerline every meter or less, at even spacing, integrating
curvature exactly within each piece. The simulation and the renderer both work from those points:

- `project` finds a position's distance along the track and its lateral offset, positive on the
  left. The simulation uses it for walls, and later for lap progress.
- `point_at` returns the centerline point, direction, banking and half width at any distance;
  `half_width_at` is that half width alone, which is what the walls are read from.
- `surface` gives the height and uphill direction of the road under a position, and
  `height_beside` the height at any lateral offset. The simulation uses the slope, the renderer
  the heights.
- `grid_slot` places cars on the starting grid behind the start line, alternating sides.

`project` compares against every segment of the centerline. That costs about a thousand distance
computations per car per tick on a half-kilometer track, which is negligible for now; a spatial
index or a search around the previous position can replace it if tracks grow large.
