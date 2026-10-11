# Night

The circuits race through a city at night: the road and its walls lit in neon, towers banded and
windowed with light, a starry sky over them (see [Sky](sky.md)). This page is about what makes that
night more than a black background: the glow of the neon, the haze that gives the city depth, the
light the walls throw on the road, what holds the road up, and what moves around it. It was the
user's request, "embellis le niveau qu'on a, vas-y franchement", made once the skyway had been
judged excellent.

Everything here is worked out by the client from what the track file already says. None of it is
written in the file, none of it travels on the wire, and the simulation never sees any of it.

## Glow

The camera renders in high dynamic range and blooms what is brightest (`client/src/camera.rs`).
The bloom only takes what stands above a threshold, and adds its glow on top of the image rather
than borrowing it from what glows: the dark road, the buildings and the cars stay exactly as crisp
as they were, and the neon, which is drawn at full brightness, wears a halo. Lights meant to glow
more than the neon -- a vehicle's headlights, a warning light on a roof -- are drawn brighter than
white, which a screen cannot show but the bloom can: they clip to their color at the core, and
their extra brightness goes into a wider halo.

Tone mapping stays off. Bevy's tone mappers desaturate bright colors, which once turned the neon
blue into a dull sky blue (see [Architecture](architecture.md#displaying-the-race)), and with the
bloom adding rather than borrowing light, nothing needs compressing: what was shown before is shown
the same, with light around it.

## Haze and moonlight

In a race, the camera sees through a haze of deep indigo that thickens with distance, as the square
of it: clear for the first couple of hundred meters, half gone at about six hundred, nearly gone at
a kilometer. The towers across the city fade into it, lighter than the black sky behind them, which
is how a city at night reads at a distance; the road ahead stays sharp. The sky ignores the haze,
as it ignores the menus' fog, so the stars shine over it.

The one directional light is moonlight now, a cool white, and the ambient light a deep blue, so the
surfaces the light does not reach are blue in the dark rather than gray.

The menus keep their own fog, black, which fades the neon grid rushing toward the camera. The race's
haze goes in when a lobby is entered and comes out when it is left.

## The road

What is painted on the road, and the light thrown on it, are built with the road
(`client/src/track/markings.rs`):

- **Kerbs** line the inside of every turn tighter than a radius of 110 m, blocks of 2.5 m
  alternating magenta and white, 1.6 m wide from the foot of the wall. A bend now reads from far off,
  which way it goes, and its apex from close up. They lined both edges at first, but the skyway is
  turns almost all the way round, and that was a lap of magenta and white.
- **An edge line**, a dull white, runs 2.2 m in from each wall, inside the kerbs, the whole lap round.
- **The starting grid** is drawn on the road, a box for each of the first eight slots.
- **The walls light the road.** At the foot of every neon stripe of the walls lies a pool of its
  blue, brightest at the wall and in the middle of the stripe, fading out over 4.2 m across the
  road and toward both ends of the stripe. It is added to what it falls on, the kerbs included, and
  it is the one thing on the road that is not opaque.

Everything painted on the road follows the road as it is built rather than as it is described. A
banked road's cross-section is a curve, and the road is built of eight flat strips across it, which
stand above the curve between their edges, by up to 4.5 cm in the sky bowl; the first edge lines,
painted on the curve itself, sank under the road there and showed as dashes. Markings now take the
height of the strip they lie on.

## What holds the road up

The road used to float on nothing. It still floats, but on piers now (`client/src/track/piers.rs`):
one every 46 m, a hammerhead whose head, as wide as a third of the road, meets the underside of the
slab at every corner however the road banks and climbs there, on a column reaching down into the
dark and fading to black on the way, as every footing of the scenery does. A band of light rings
each column under its head, and a line of light runs along the underside of the slab near each of
its edges. Seen from another part of the lap, the road is a structure with a height, rather than a
ribbon hung in the air.

The piers stand under the road, inside its walls, where nothing else can stand: the road never runs
over itself, and the scenery keeps to the outside of the walls.

## The city

The towers of the districts and those the file places get more than bands of light
(`client/src/scenery.rs`):

- **Windows.** About half of them have windows lit here and there instead of a band round every
  floor: a fifth to a half of their windows, most of them in the tower's own tone, the warm light of
  homes or the cool light of offices, and now and then a cyan or a magenta one. The light is
  saturated, since a window lit a pale gray read as a gray tile, and dimmer than the road's neon, so
  the city stays behind the road and only its brightest windows bloom a little.
- **Crowns.** The light round the top of a tower is the road's blue on most of them, and the magenta
  or the teal of the sky's neon shapes on about one in seven each.
- **Warning lights.** Most towers taller than 30 m carry a red light on their roof or on top of
  their mast. They blink in three groups, each in its own time.

What the city adds to a tower is rolled from dice of its own, apart from the dice the tower's shape
comes from, so every tower keeps the shape it had.

## What moves

`client/src/city.rs` spawns, with the track, what moves around it. Nothing is rebuilt while it
moves: a lane of traffic is one mesh slid along its own direction, a beam one mesh turned about its
foot, and the warning lights blink by changing the color of the one material their group shares.

- **Traffic.** Six corridors ring the city, one on each side of it, each two lanes of vehicles
  flying opposite ways at 30 to 62 m/s, at about the height of the highest roof. A corridor is a
  straight line 150 to 380 m outside the furthest edge of the road, past where the districts reach,
  and a straight line that far from the middle of the city stays that far all along it: no vehicle
  ever flies through a building. The first version flew them over the city instead, every corridor
  crossing it, and from the road, looking up, the sky was full of confetti; out beyond the city they
  stream along the skyline, where the eye already is. A vehicle is a dark hull with a white light at
  its nose and a red one at its tail, each a block standing out of the hull so it shows from every
  side: a stream coming at the camera is a string of white, one going away a string of red, one
  passing pairs of both. A lane's vehicles come in a pattern 260 m long, repeated down the lane, so
  sliding the lane by up to one pattern's length and starting again looks like a stream that never
  ends.
- **Searchlights.** Each grandstand has one at each end of its top row, in a housing ringed with
  light. Their beams, 330 m long and fading out along their length, lean 16 to 28 degrees from
  straight up, turn slowly, neighbors the opposite way, and lean in and out as they go. A beam is
  two cones, a core and a wider, fainter glow around it, so its light falls off toward its edges
  rather than stopping at a line.
- **The haze below.** A disc of faint light lies under the whole circuit, 58 m below the ground,
  under the footings that fade to black on their way down to it: violet everywhere, warmer and
  brighter under the towers, as if the streets of the city were lit far below. Looking off the edge
  of the road is no longer looking into black, and the footings stand out against it.

Like the sky and the districts, all of it is rolled from fixed seeds, so every player sees the same
city doing the same things.

## Cost

The track is four draw calls now: its lit surfaces, the scenery's and the piers' surfaces, which
cast no shadow, the neon, and the pools of light on the road. On the skyway they come to 155,100
triangles, from 131,400, built in 0.14 s on entering a lobby against 0.12: 275 towers, 71 warning
lights and 8 searchlights. What moves adds one draw call a lane of traffic, twelve in all, one a
beam, one a group of warning lights, and the haze one more.

Measured on the development machine (GTX 1050 Ti), in debug, without vsync, at the same moments of
the same drifting lap, the latency overlay averaging the last 32 frames: 3.5 to 3.6 ms a frame
before, 4.1 ms after, 275 to 285 frames a second against 243 to 247. Half a millisecond for all of
it, the bloom included.

## To be judged

By the user: whether the glow is too much or too little, how thick the haze is, the kerbs' colors,
whether the traffic, the searchlights and the haze below earn their place, and how busy the city has
become.
