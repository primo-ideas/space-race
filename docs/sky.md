# Sky

Over the circuit, a starry night: stars all round, a band of galaxy across them, clouds of colored
light, planets and moons, and a few shapes of neon turning slowly far out in space. It was the
user's request, "un ciel nuit étoilé avec des astres colorés, des formes", made while the skyway
was being dressed as a city at night (see [Tracks](tracks.md#the-skyways-city)). The code is
`client/src/sky.rs`.

## At infinity

Everything in the sky stands on a sphere 880 m round, centered on the camera: each frame, before
transforms are propagated, the sky is moved to wherever the camera is, without turning. However far
the car drives, the sky comes no closer and keeps its place against the circuit, as a sky does.
880 m puts it beyond everything on a circuit and short of the 1 km past which Bevy's camera culls
what it draws; the planets stand nearer, at 760 m, so the far side of the giant's rings stays short
of that kilometer too, and they pass in front of the stars.

It is the same sky everywhere: behind the menus too, since it ignores the black fog the menus fade
their grid into (`fog_enabled: false`), and on every circuit.

## Made of triangles

There is no texture, as there is no audio file (see [Sound](sound.md)): everything is triangles
colored at their corners, rolled from one fixed seed with the generator the city's districts use
(`client/src/dice.rs`), so every player sees the same sky. It is unlit, so it shows the colors it is
given, and casts no shadow.

- **Stars**: 3,400 diamonds turned toward the camera, kept mostly over the horizon (below it, the
  city and the dark under the road hide them anyway, and only three in ten are kept). Most are
  faint and small, about a pixel, a few bright ones about four; the brightest, about one in fifty,
  wear a cross of light. Their colors are mostly a cool white, a few warm, cyan, pink or violet.
- **The galaxy**: a band across the sky, tilted, of soft blue and violet light, swelling and
  thinning along it, crowded with 1,600 more faint stars.
- **Nebulae**: four clouds of soft blobs in two colors each: magenta and violet in front of the
  menus, teal and blue high over the south-east, orange and rose low in the west, and a faint violet
  veil overhead.
- **Planets**: discs facing the camera, shaded as spheres lit from one side by a sun nobody sees,
  never quite black on their night side so they stand out against the sky, and each with a halo.
  A magenta giant banded in violet, with three rings nearly edge on, hangs over the launch and the
  run to the line; a great pale moon, lit from behind into a crescent, in the west; a small orange
  world in the south; a teal moon in front of the menus. The half of the giant's rings behind it is
  hidden by its disc, simply because it is further away.
- **Shapes**: frames of neon bars turning slowly on themselves: a magenta octahedron over the
  north-east, two cyan rings one inside the other over the south-west, an amber tetrahedron in the
  south-east.

The chase camera looks a little down at the road, and sees the sky up to about 25 to 30 degrees over
the horizon, so whatever is meant to be seen while driving stands lower than that.

## Cost

Five meshes, so five draw calls: the stars, the planets and their rings in one opaque mesh; the
glow of the galaxy, the nebulae, the halos and the stars' crosses in one blended additively, which
only ever brightens what is behind it; and one per shape, which turns. The opaque sky is drawn
behind everything else, so a building hides the stars it stands in front of.

## To be judged

By the user: the colors, how busy the sky is against the city, where the planets hang, and whether
the shapes earn their place.
