//! The scenery of a circuit: the props the track file stands beside the road.
//!
//! Their neon goes into the track's own neon mesh and their bodies into a mesh of their own, so a
//! dressed circuit costs no more draw calls than a bare one, and they follow the same language:
//! dark geometric bodies that take the light, lined with neon that does not. Nothing here touches
//! the simulation, and every prop is placed from the road rather than in world coordinates (see
//! [`scenery`](space_race_sim::track::scenery)).
//!
//! The city's towers are lit as well as built here: bands of light or windows, a crown of neon,
//! and a warning light on the tallest roofs. What moves around them is the city's own (see
//! [`crate::city`]), told where the towers and the grandstands stand through the [`Landmarks`] the
//! scenery fills in as it builds.
//!
//! Each prop is built in a [`Frame`] of its own, standing at the foot of the wall at the level of
//! the road: `x` runs along the road, `y` up, and `z` away from the road, so the face at `z = 0` is
//! the one that looks at the driver, and the same numbers build the prop on either side of the road
//! and at any height the relief carries it to. Below that level, whatever stands off the road
//! reaches down into the dark on a footing that fades to black on the way to [`ABYSS`], the ground
//! nobody sees: the props seem to rise out of nothing, the way the road floats over it.

use bevy::prelude::*;
use space_race_sim::track::scenery::{Prop, Side};
use space_race_sim::track::{Track, TrackPoint};

use crate::city::Landmarks;
use crate::dice::{Dice, seed};
use crate::geometry::{Frame, Geometry};
use crate::track::{NEON_BLUE, SLAB, WALL_HEIGHT, WALL_THICKNESS};
use crate::world;

/// Every structure is this dark gray, a little bluer than the walls: enough to catch the light, not
/// enough to pull the eye from the road.
const STRUCTURE: Color = Color::srgb(0.12, 0.12, 0.15);
/// The buildings of the city are darker than the props along the road, so the city recedes into the
/// night behind its lights and the road stays the brightest surface in sight. Towers vary around
/// the middle shade, so a district reads as many buildings rather than one.
const CITY: Color = Color::srgb(0.07, 0.07, 0.09);
const TOWER_SHADES: [Color; 4] = [
    Color::srgb(0.055, 0.06, 0.075),
    CITY,
    Color::srgb(0.085, 0.085, 0.105),
    Color::srgb(0.06, 0.065, 0.08),
];
/// The neon of the city's floors and windows: the blue of the road, dimmed, so the road's own
/// neon stays the brightest thing in sight.
const NEON_FAINT: Color = Color::srgb(0.02, 0.24, 0.44);
/// What a footing fades to at the bottom: the black the whole circuit floats in.
pub(crate) const DARK: Color = Color::BLACK;

/// How far down everything standing off the road reaches, in meters above the ground. Footings fade
/// to black on the way down to it, well below the lowest road.
pub(crate) const ABYSS: f32 = -40.0;

/// What a tower's lights are rolled from, apart from the dice its shape comes from: every tower
/// keeps the shape it had before it had windows, whatever its lights do.
const LIGHTS_SEED: u64 = 0x11_6475;
/// The share of towers with windows lit here and there, rather than a band of light round every
/// floor.
const WINDOWED: f32 = 0.55;
/// How far apart the windows of a face are across it, in meters, and how much of that a window
/// takes; how much of a floor's height a window takes.
const WINDOW_PITCH: f32 = 2.6;
const WINDOW_WIDTH: f32 = 0.42;
const WINDOW_HEIGHT: f32 = 0.34;
/// How many of a windowed tower's windows are lit, at least and at most.
const WINDOWS_LIT: (f32, f32) = (0.2, 0.5);
/// The light in the windows: the warm light of homes, the cool light of offices, and now and then
/// a colored one. Saturated, or a lit window reads as a gray tile; dimmer than the road's neon, so
/// the city stays behind the road and only the brightest windows bloom a little.
const WINDOW_LIGHTS: [Color; 4] = [
    Color::linear_rgb(0.66, 0.3, 0.06),
    Color::linear_rgb(0.24, 0.4, 0.7),
    Color::linear_rgb(0.03, 0.45, 0.62),
    Color::linear_rgb(0.6, 0.04, 0.34),
];
/// A windowed tower's windows are mostly in its own tone, one of the first two lights, and this
/// share of them in any of the four.
const WINDOWS_ODD: f32 = 0.15;
/// The neon round the top of a tower: the road's blue on most of them, and on a few the magenta or
/// the teal of the neon shapes in the sky, by weight.
const CROWNS: [(Color, u32); 3] = [
    (NEON_BLUE, 10),
    (Color::srgb(1.0, 0.25, 0.65), 2),
    (Color::srgb(0.15, 0.95, 0.8), 2),
];
/// Most towers at least this tall over the road's level carry a warning light on their roof.
const BEACON_TALL: f32 = 30.0;
const BEACON_SHARE: f32 = 0.75;

/// A searchlight stands at each end of a grandstand's top row, this share of the way along it.
const SEARCHLIGHTS: [f32; 2] = [0.12, 0.88];

/// Where the neon rings sit up a pylon, as a share of its height, and how much of its base width is
/// left at the top.
const PYLON_RINGS: [f32; 3] = [0.3, 0.55, 0.8];
const PYLON_TAPER: f32 = 0.4;

/// How far the legs of a gantry stand out from the wall, and how deep its beam is, in meters.
const GANTRY_LEG: f32 = 1.4;
const GANTRY_BEAM: f32 = 1.4;

/// One row of a grandstand: how deep its step is and how high it rises, in meters.
const ROW_DEPTH: f32 = 1.7;
const ROW_RISE: f32 = 0.95;
/// A grandstand, or a facade, is swept in sections about this long, so it follows the curve of the
/// road.
const SWEEP_SECTION: f32 = 3.0;

/// How thick the neon frame of a billboard is, and how far apart its stripes stand, in meters.
const BILLBOARD_FRAME: f32 = 0.3;
const BILLBOARD_STRIPE_SPACING: f32 = 2.2;

/// One arrow of a marker board: how tall it is, how thick its bars are, and how wide the board is,
/// in meters.
const CHEVRON_HEIGHT: f32 = 0.85;
const CHEVRON_BAR: f32 = 0.22;
const CHEVRON_WIDTH: f32 = 2.6;

/// How far apart the lit floors of a tower or a facade are, at most and at least, in meters, and
/// how tall a band of light is.
const FLOOR_SPACING: (f32, f32) = (3.5, 6.0);
const FLOOR_BAND: f32 = 0.22;
/// How far a band of light stands proud of the face it runs around, in meters.
const PROUD: f32 = 0.08;

/// How wide and how deep the towers of a district are, in meters, and how much room is left
/// between two of them.
const DISTRICT_TOWER: (f32, f32) = (10.0, 20.0);
const DISTRICT_GAP: (f32, f32) = (2.0, 6.0);
/// How close a tower of a district may come to any part of the road, in meters from the outer face
/// of its wall, unless the file places the district closer to its own road than that.
const DISTRICT_CLEARANCE: f32 = 5.0;

/// A lit mullion runs up the face of a facade every this many of its sections.
const FACADE_MULLIONS: usize = 3;

/// A tunnel is built in sections this long, its walls and roof are this thick, and a ring of neon
/// runs round the inside every `TUNNEL_RING` meters, in meters.
const TUNNEL_SECTION: f32 = 2.0;
const TUNNEL_WALL: f32 = 1.5;
const TUNNEL_ROOF: f32 = 1.5;
const TUNNEL_RING: f32 = 12.0;
const TUNNEL_RING_WIDTH: f32 = 0.6;
/// Strips across the roof, so it follows the curve of a banked road like the road itself does.
const TUNNEL_STRIPS: usize = 8;
/// How far the inner face of a tunnel wall stands outside the road's own wall, so the two never
/// draw over each other.
const TUNNEL_GAP: f32 = 0.03;

/// Builds every prop of a track into the meshes the track itself is built into, and says where the
/// city stands for what moves around it.
pub fn build(lit: &mut Geometry, neon: &mut Geometry, track: &Track, scenery: &[Prop]) -> Landmarks {
    let mut landmarks = Landmarks::default();
    // The ground the buildings the file places stand on, so the districts fill the rest of it: each
    // district avoids them, and those before it.
    let mut standing: Vec<[Vec2; 4]> = scenery
        .iter()
        .flat_map(|prop| ground_of(track, prop))
        .collect();
    for prop in scenery {
        match *prop {
            Prop::Gantry {
                at,
                clearance,
                depth,
            } => gantry(lit, neon, track, at, clearance, depth, &mut landmarks),
            Prop::Pylon {
                at,
                side,
                offset,
                height,
            } => {
                let frame = beside_road(track, at, side, offset);
                pylon(lit, neon, &frame, height);
                landmarks.rise_to(frame.height() + height + 0.5);
            }
            Prop::Grandstand {
                at,
                side,
                offset,
                length,
                rows,
            } => grandstand(
                lit,
                neon,
                track,
                (at, side, offset),
                (length, rows),
                &mut landmarks,
            ),
            Prop::Billboard {
                at,
                side,
                offset,
                width,
                height,
            } => billboard(
                lit,
                neon,
                &beside_road(track, at, side, offset),
                width,
                height,
            ),
            Prop::Chevrons {
                at,
                side,
                offset,
                count,
            } => chevrons(lit, neon, track, at, side, offset, count),
            Prop::Monolith {
                at,
                side,
                offset,
                height,
                width,
            } => {
                let frame = beside_road(track, at, side, offset);
                monolith(lit, neon, &frame, height, width);
                landmarks.rise_to(frame.height() + height + 0.6);
            }
            Prop::Tower {
                at,
                side,
                offset,
                height,
                width,
                depth,
            } => tower(
                lit,
                neon,
                &beside_road(track, at, side, offset),
                Vec3::new(width, height, depth),
                seed(&[at, offset, height, width, depth]),
                &mut landmarks,
            ),
            Prop::Skyline {
                at,
                side,
                offset,
                length,
                depth,
                height,
            } => skyline(
                lit,
                neon,
                track,
                (at, side, offset),
                Vec3::new(length, height, depth),
                &mut standing,
                &mut landmarks,
            ),
            Prop::Facade {
                at,
                side,
                offset,
                length,
                height,
                depth,
            } => facade(
                lit,
                neon,
                track,
                (at, side, offset),
                Vec3::new(length, height, depth),
                &mut landmarks,
            ),
            Prop::Tunnel {
                at,
                length,
                clearance,
            } => tunnel(lit, neon, track, at, length, clearance),
        }
    }
    landmarks
}

/// The ground a building placed by the file stands on, on the simulation plane: a tower's, a
/// slab's, and a facade's or a grandstand's, section by section. Smaller props take too little room
/// to keep a district's tower from standing beside them.
fn ground_of(track: &Track, prop: &Prop) -> Vec<[Vec2; 4]> {
    let swept = |at, side, offset, length, depth| {
        let frames = swept_frames(track, at, side, offset, length);
        frames
            .windows(2)
            .map(|pair| {
                [(0, 0.0), (1, 0.0), (1, depth), (0, depth)].map(|(frame, out): (usize, f32)| {
                    let world = pair[frame].point(Vec3::new(0.0, 0.0, out));
                    Vec2::new(world.x, -world.z)
                })
            })
            .collect()
    };
    match *prop {
        Prop::Tower {
            at,
            side,
            offset,
            height,
            width,
            depth,
        } => vec![ground_footprint(
            &beside_road(track, at, side, offset),
            Vec3::new(width, height, depth),
        )],
        Prop::Monolith {
            at,
            side,
            offset,
            height,
            width,
        } => vec![ground_footprint(
            &beside_road(track, at, side, offset),
            Vec3::new(width, height, width * 0.35),
        )],
        Prop::Facade {
            at,
            side,
            offset,
            length,
            depth,
            ..
        } => swept(at, side, offset, length, depth),
        Prop::Grandstand {
            at,
            side,
            offset,
            length,
            rows,
        } => swept(at, side, offset, length, rows as f32 * ROW_DEPTH),
        _ => Vec::new(),
    }
}

/// The frame of a prop standing beside the road: at the foot of the wall, `offset` meters further
/// out, at the level of the road's edge on that side, with `z` running away from the road.
fn beside_road(track: &Track, at: f32, side: Side, offset: f32) -> Frame {
    let point = track.point_at(at);
    let away = point.direction.perp() * side.sign();
    // The wall is where the road is here: a prop beside a bottleneck stands in with it.
    let foot = point.position + away * (point.half_width + WALL_THICKNESS + offset);
    // On a banked turn, the raised edge or the low one, whichever the prop stands by.
    let level = track.height_beside(&point, side.sign() * point.half_width);
    Frame::new(
        world::position(foot, level),
        world::direction(point.direction),
        world::direction(away),
    )
}

/// What a prop stands on: the box from `from` to `to` across the frame's ground, carried from the
/// road's level down into the dark, `color` at the top and black at the bottom.
fn footing(lit: &mut Geometry, frame: &Frame, from: Vec2, to: Vec2, color: Color) {
    lit.shaded_block(
        frame,
        Vec3::new(from.x, ABYSS - frame.height(), from.y),
        Vec3::new(to.x, 0.0, to.y),
        DARK,
        color,
    );
}

/// The footing of a prop swept along the road over `frames`, between `front` and `back` meters
/// out: its two long faces and its two ends, from the road's level down into the dark.
fn swept_footing(lit: &mut Geometry, frames: &[Frame], front: f32, back: f32, color: Color) {
    let low = |frame: &Frame, out: f32| frame.point(Vec3::new(0.0, ABYSS - frame.height(), out));
    let high = |frame: &Frame, out: f32| frame.point(Vec3::new(0.0, 0.0, out));
    let middle = (front + back) / 2.0;
    let shades = [DARK, DARK, color, color];
    for pair in frames.windows(2) {
        let (near, far) = (&pair[0], &pair[1]);
        let inside = near.point(Vec3::new(0.0, -1.0, middle));
        for out in [front, back] {
            lit.quad_facing_away_shaded(
                [low(near, out), low(far, out), high(far, out), high(near, out)],
                inside,
                shades,
            );
        }
    }
    for (frame, inward) in [(frames.first(), 1.0_f32), (frames.last(), -1.0)] {
        let Some(frame) = frame else {
            continue;
        };
        let inside = frame.point(Vec3::new(inward, -1.0, middle));
        lit.quad_facing_away_shaded(
            [
                low(frame, front),
                low(frame, back),
                high(frame, back),
                high(frame, front),
            ],
            inside,
            shades,
        );
    }
}

/// An arch over the road. Its frame stands on the centerline at ground level, so `z` is the lateral
/// offset, to the left as everywhere else, `y` the height above the ground, and the two legs are
/// the same shape mirrored.
fn gantry(
    lit: &mut Geometry,
    neon: &mut Geometry,
    track: &Track,
    at: f32,
    clearance: f32,
    depth: f32,
    landmarks: &mut Landmarks,
) {
    let point = track.point_at(at);
    let frame = Frame::new(
        world::position(point.position, 0.0),
        world::direction(point.direction),
        world::direction(point.direction.perp()),
    );

    let half_width = point.half_width;
    let edge = half_width + WALL_THICKNESS;
    let span = edge + GANTRY_LEG;
    let half_depth = depth / 2.0;
    // The beam clears the higher wall: on a banked turn, the outer one stands well above the road.
    let walls = f32::max(
        track.height_beside(&point, half_width),
        track.height_beside(&point, -half_width),
    ) + WALL_HEIGHT;
    let beam = walls + clearance;
    landmarks.rise_to(beam + GANTRY_BEAM);

    for side in [1.0_f32, -1.0] {
        let (near, far) = (side * edge, side * span);
        let (inner, outer) = (near.min(far), near.max(far));
        // Each leg stands by the edge of the road on its own side, and goes on down into the dark.
        let foot = track.height_beside(&point, side * half_width);
        lit.shaded_block(
            &frame,
            Vec3::new(-half_depth, ABYSS, inner),
            Vec3::new(half_depth, foot, outer),
            DARK,
            STRUCTURE,
        );
        lit.block(
            &frame,
            Vec3::new(-half_depth, foot, inner),
            Vec3::new(half_depth, beam + GANTRY_BEAM, outer),
            STRUCTURE,
        );
        // A band of light around each leg, at the height a driver reads road signs at.
        neon.block(
            &frame,
            Vec3::new(-half_depth - 0.06, foot + 2.2, inner - 0.06),
            Vec3::new(half_depth + 0.06, foot + 3.0, outer + 0.06),
            NEON_BLUE,
        );
    }

    lit.block(
        &frame,
        Vec3::new(-half_depth, beam, -span),
        Vec3::new(half_depth, beam + GANTRY_BEAM, span),
        STRUCTURE,
    );
    // Seen from the road the beam is a bar of light: one strip underneath, one across the face the
    // cars come at.
    neon.block(
        &frame,
        Vec3::new(-half_depth * 0.7, beam - 0.3, -span),
        Vec3::new(half_depth * 0.7, beam, span),
        NEON_BLUE,
    );
    neon.block(
        &frame,
        Vec3::new(-half_depth - 0.08, beam + 0.35, -span),
        Vec3::new(-half_depth + 0.1, beam + GANTRY_BEAM - 0.35, span),
        NEON_BLUE,
    );
}

/// A tapered mast ringed with neon, and a light on top.
fn pylon(lit: &mut Geometry, neon: &mut Geometry, frame: &Frame, height: f32) {
    let half = (height * 0.045).clamp(0.4, 1.2);
    footing(
        lit,
        frame,
        Vec2::new(-half, 0.0),
        Vec2::new(half, 2.0 * half),
        STRUCTURE,
    );
    lit.tapered_block(
        frame,
        Vec3::new(-half, 0.0, 0.0),
        Vec3::new(half, height, 2.0 * half),
        PYLON_TAPER,
        STRUCTURE,
    );
    for share in PYLON_RINGS {
        // The mast narrows evenly as it rises, so its rings narrow with it.
        let ring = half * (1.0 - (1.0 - PYLON_TAPER) * share) * 1.3;
        let base = height * share;
        neon.block(
            frame,
            Vec3::new(-ring, base, half - ring),
            Vec3::new(ring, base + 0.25, half + ring),
            NEON_BLUE,
        );
    }
    let cap = half * PYLON_TAPER;
    neon.block(
        frame,
        Vec3::new(-cap, height, half - cap),
        Vec3::new(cap, height + 0.5, half + cap),
        NEON_BLUE,
    );
}

/// The frames a prop swept along the road is built on: one every [`SWEEP_SECTION`] or so, from
/// `at` to `length` meters further.
fn swept_frames(track: &Track, at: f32, side: Side, offset: f32, length: f32) -> Vec<Frame> {
    let sections = (length / SWEEP_SECTION).ceil().max(1.0) as usize;
    (0..=sections)
        .map(|section| {
            let along = length * section as f32 / sections as f32;
            beside_road(track, at + along, side, offset)
        })
        .collect()
}

/// A stepped tribune, swept along the road so it curves with it, a neon line along every row, and
/// a searchlight at each end of its top row.
fn grandstand(
    lit: &mut Geometry,
    neon: &mut Geometry,
    track: &Track,
    (at, side, offset): (f32, Side, f32),
    (length, rows): (f32, u8),
    landmarks: &mut Landmarks,
) {
    let frames = swept_frames(track, at, side, offset, length);

    let back = rows as f32 * ROW_DEPTH;
    let top = rows as f32 * ROW_RISE;
    // The front of the bottom row, then every riser and tread, then the back down to the road's
    // level, where the footing takes over.
    let mut profile = vec![Vec2::ZERO];
    for row in 0..rows {
        let front = row as f32 * ROW_DEPTH;
        let height = (row + 1) as f32 * ROW_RISE;
        profile.push(Vec2::new(front, height));
        profile.push(Vec2::new(front + ROW_DEPTH, height));
    }
    profile.push(Vec2::new(back, 0.0));
    lit.sweep(
        &frames,
        &profile,
        Vec2::new(back / 2.0, top / 4.0),
        STRUCTURE,
    );
    swept_footing(lit, &frames, 0.0, back, STRUCTURE);

    for row in 0..rows {
        let front = row as f32 * ROW_DEPTH;
        let height = (row + 1) as f32 * ROW_RISE + 0.03;
        neon.sweep(
            &frames,
            &[Vec2::new(front, height), Vec2::new(front + 0.45, height)],
            Vec2::new(front + 0.2, height - 1.0),
            NEON_BLUE,
        );
    }

    // The two ends, each a staircase closed off by one rectangle per row.
    for (frame, inward) in [(frames.first(), 1.0_f32), (frames.last(), -1.0)] {
        let Some(frame) = frame else {
            continue;
        };
        for row in 0..rows {
            let (front, behind) = (row as f32 * ROW_DEPTH, (row + 1) as f32 * ROW_DEPTH);
            let height = (row + 1) as f32 * ROW_RISE;
            let corners = [
                (front, 0.0),
                (behind, 0.0),
                (behind, height),
                (front, height),
            ]
            .map(|(depth, rise)| frame.cross_section(Vec2::new(depth, rise)));
            let inside = frame.point(Vec3::new(
                inward * SWEEP_SECTION / 2.0,
                height / 2.0,
                (front + behind) / 2.0,
            ));
            lit.quad_facing_away(corners, inside, STRUCTURE);
        }
    }

    // The searchlights, each in a housing on the top row, ringed with light; their beams are the
    // city's to turn.
    let middle = back - ROW_DEPTH / 2.0;
    for share in SEARCHLIGHTS {
        let frame = &frames[((frames.len() - 1) as f32 * share).round() as usize];
        lit.block(
            frame,
            Vec3::new(-0.7, top, middle - 0.7),
            Vec3::new(0.7, top + 0.8, middle + 0.7),
            STRUCTURE,
        );
        neon.block(
            frame,
            Vec3::new(-0.78, top + 0.8, middle - 0.78),
            Vec3::new(0.78, top + 1.0, middle + 0.78),
            NEON_BLUE,
        );
        landmarks
            .searchlights
            .push(frame.point(Vec3::new(0.0, top + 1.0, middle)));
        landmarks.rise_to(frame.height() + top + 1.0);
    }
}

/// A panel on two posts, its neon frame and stripes facing the road.
fn billboard(lit: &mut Geometry, neon: &mut Geometry, frame: &Frame, width: f32, height: f32) {
    let half = width / 2.0;
    let panel = height * 0.45;
    for side in [-1.0_f32, 1.0] {
        let post = side * (half - (half * 0.2).clamp(0.5, 1.5));
        footing(
            lit,
            frame,
            Vec2::new(post - 0.3, 0.35),
            Vec2::new(post + 0.3, 0.95),
            STRUCTURE,
        );
        lit.block(
            frame,
            Vec3::new(post - 0.3, 0.0, 0.35),
            Vec3::new(post + 0.3, panel + 0.4, 0.95),
            STRUCTURE,
        );
    }
    lit.block(
        frame,
        Vec3::new(-half, panel, 0.3),
        Vec3::new(half, height, 0.6),
        STRUCTURE,
    );

    // The frame around the panel, then stripes leaning the way the cars run.
    let edge = BILLBOARD_FRAME;
    for (from, to) in [
        (
            Vec3::new(-half, panel, 0.15),
            Vec3::new(half, panel + edge, 0.32),
        ),
        (
            Vec3::new(-half, height - edge, 0.15),
            Vec3::new(half, height, 0.32),
        ),
        (
            Vec3::new(-half, panel, 0.15),
            Vec3::new(-half + edge, height, 0.32),
        ),
        (
            Vec3::new(half - edge, panel, 0.15),
            Vec3::new(half, height, 0.32),
        ),
    ] {
        neon.block(frame, from, to, NEON_BLUE);
    }

    let (bottom, top) = (panel + edge * 1.6, height - edge * 1.6);
    let margin = half - edge * 1.6;
    if top > bottom && margin > 0.0 {
        let lean = (top - bottom).min(2.0 * margin);
        let stripes = (((2.0 * margin - lean) / BILLBOARD_STRIPE_SPACING) as usize + 1).min(5);
        for stripe in 0..stripes {
            let start = -margin + stripe as f32 * BILLBOARD_STRIPE_SPACING;
            neon.bar(
                frame,
                Vec3::new(start, bottom, 0.2),
                Vec3::new(start + lean, top, 0.2),
                edge * 0.8,
                NEON_BLUE,
            );
        }
    }
}

/// A marker board of stacked arrows. Unlike the props beside it, the board faces back down the
/// road, at the cars coming, so its arrows point across it: the way the road bends.
fn chevrons(
    lit: &mut Geometry,
    neon: &mut Geometry,
    track: &Track,
    at: f32,
    side: Side,
    offset: f32,
    count: u8,
) {
    let frame = beside_road(track, at, side, offset);
    // The bend is read from the road; on the prop's own side, the driver's left is `side.sign()`
    // times the frame's `z`.
    let arrow = bend_at(track, at) * side.sign();
    let base = 1.1;
    let top = base + count as f32 * CHEVRON_HEIGHT;

    for post in [0.45, CHEVRON_WIDTH - 0.45] {
        footing(
            lit,
            &frame,
            Vec2::new(-0.15, post - 0.15),
            Vec2::new(0.15, post + 0.15),
            STRUCTURE,
        );
        lit.block(
            &frame,
            Vec3::new(-0.15, 0.0, post - 0.15),
            Vec3::new(0.15, base + 0.3, post + 0.15),
            STRUCTURE,
        );
    }
    lit.block(
        &frame,
        Vec3::new(-0.15, base, 0.0),
        Vec3::new(0.15, top, CHEVRON_WIDTH),
        STRUCTURE,
    );

    let (tip, tail) = if arrow >= 0.0 {
        (CHEVRON_WIDTH - 0.3, 0.3)
    } else {
        (0.3, CHEVRON_WIDTH - 0.3)
    };
    for index in 0..count {
        let middle = base + (index as f32 + 0.5) * CHEVRON_HEIGHT;
        let reach = CHEVRON_HEIGHT * 0.35;
        for rise in [-reach, reach] {
            neon.bar(
                &frame,
                Vec3::new(-0.22, middle + rise, tail),
                Vec3::new(-0.22, middle, tip),
                CHEVRON_BAR,
                NEON_BLUE,
            );
        }
    }
}

/// A tall slab far out in the dark, seamed with neon and lit on top.
fn monolith(lit: &mut Geometry, neon: &mut Geometry, frame: &Frame, height: f32, width: f32) {
    let half = width / 2.0;
    let depth = width * 0.35;
    footing(
        lit,
        frame,
        Vec2::new(-half, 0.0),
        Vec2::new(half, depth),
        STRUCTURE,
    );
    lit.tapered_block(
        frame,
        Vec3::new(-half, 0.0, 0.0),
        Vec3::new(half, height, depth),
        0.82,
        STRUCTURE,
    );
    neon.bar(
        frame,
        Vec3::new(0.0, height * 0.06, 0.0),
        Vec3::new(0.0, height * 0.94, 0.0),
        width * 0.06,
        NEON_BLUE,
    );
    let cap = half * 0.4;
    neon.block(
        frame,
        Vec3::new(-cap, height, depth / 2.0 - cap),
        Vec3::new(cap, height + 0.6, depth / 2.0 + cap),
        NEON_BLUE,
    );
}

/// A tower of the city: `size` is its width along the road, its height over the road's level and
/// its depth away from the road, and `seed` picks its shape and its lights, so the same file always
/// builds the same tower. Its frame stands at the middle of the face that looks at the road.
///
/// It comes in three shapes: a plain block banded with lit floors, a block with a narrower one set
/// back on top of it, and a block crowned by a tapering spire and a mast with a light. It is lit
/// with bands of light round its floors or with windows lit here and there, crowned with neon, and,
/// if it is tall, carries a warning light on its roof or its mast.
fn tower(
    lit: &mut Geometry,
    neon: &mut Geometry,
    frame: &Frame,
    size: Vec3,
    seed: u64,
    landmarks: &mut Landmarks,
) {
    let mut dice = Dice::new(seed);
    let mut lights = Lighting::roll(seed);
    let (half, height, depth) = (size.x / 2.0, size.y, size.z);
    let color = TOWER_SHADES[dice.below(TOWER_SHADES.len())];
    let spacing = dice.range(FLOOR_SPACING.0, FLOOR_SPACING.1);
    let (low, high) = (Vec2::new(-half, 0.0), Vec2::new(half, depth));
    let middle = (low + high) / 2.0;
    footing(lit, frame, low, high, color);
    let ground = frame.point(Vec3::new(middle.x, 0.0, middle.y));
    landmarks.towers.push(Vec2::new(ground.x, -ground.z));
    // Where a warning light would stand, over the ground.
    let mut summit = height;

    match dice.below(3) {
        0 => {
            storey(
                lit,
                neon,
                frame,
                (low, high),
                (0.0, height),
                spacing,
                color,
                &mut lights,
            );
        }
        1 => {
            // The setback: a narrower block centered on the first.
            let shoulder = height * dice.range(0.5, 0.7);
            storey(
                lit,
                neon,
                frame,
                (low, high),
                (0.0, shoulder),
                spacing,
                color,
                &mut lights,
            );
            let inset = (high - low) * dice.range(0.15, 0.25);
            let (upper_low, upper_high) = (low + inset, high - inset);
            storey(
                lit,
                neon,
                frame,
                (upper_low, upper_high),
                (shoulder, height),
                spacing,
                color,
                &mut lights,
            );
            band(neon, frame, (low, high), shoulder - 0.4, 0.4, lights.crown);
            // Lit corners up the upper block, seen from far off.
            for corner in [
                Vec2::new(upper_low.x, upper_low.y),
                Vec2::new(upper_high.x, upper_low.y),
                Vec2::new(upper_high.x, upper_high.y),
                Vec2::new(upper_low.x, upper_high.y),
            ] {
                let outward = (corner - middle).normalize_or_zero() * PROUD;
                neon.bar(
                    frame,
                    Vec3::new(corner.x + outward.x, shoulder, corner.y + outward.y),
                    Vec3::new(corner.x + outward.x, height, corner.y + outward.y),
                    0.3,
                    NEON_FAINT,
                );
            }
        }
        _ => {
            let crown = height * dice.range(0.75, 0.88);
            storey(
                lit,
                neon,
                frame,
                (low, high),
                (0.0, crown),
                spacing,
                color,
                &mut lights,
            );
            lit.tapered_block(
                frame,
                Vec3::new(low.x, crown, low.y),
                Vec3::new(high.x, height, high.y),
                0.3,
                color,
            );
            // A mast on the spire, and its light.
            let mast = (height * 0.15).clamp(3.0, 12.0);
            lit.block(
                frame,
                Vec3::new(middle.x - 0.25, height, middle.y - 0.25),
                Vec3::new(middle.x + 0.25, height + mast, middle.y + 0.25),
                color,
            );
            neon.block(
                frame,
                Vec3::new(middle.x - 0.5, height + mast, middle.y - 0.5),
                Vec3::new(middle.x + 0.5, height + mast + 1.0, middle.y + 0.5),
                lights.crown,
            );
            summit = height + mast + 1.0;
        }
    }

    if height >= BEACON_TALL && lights.dice.range(0.0, 1.0) < BEACON_SHARE {
        landmarks
            .beacons
            .push(frame.point(Vec3::new(middle.x, summit, middle.y)));
        summit += 1.5;
    }
    landmarks.rise_to(frame.height() + summit);
}

/// How a tower is lit, apart from its shape: windows or bands of light round its floors, and the
/// color of the neon round its top. Rolled from dice of its own, which then go on rolling which
/// windows are lit.
struct Lighting {
    /// The tone most of its windows are lit in, if it has windows rather than bands, and the share
    /// of them that are lit.
    windows: Option<(Color, f32)>,
    crown: Color,
    dice: Dice,
}

impl Lighting {
    fn roll(seed: u64) -> Self {
        let mut dice = Dice::new(seed ^ LIGHTS_SEED);
        let windows = (dice.range(0.0, 1.0) < WINDOWED).then(|| {
            let tone = WINDOW_LIGHTS[dice.below(2)];
            (tone, dice.range(WINDOWS_LIT.0, WINDOWS_LIT.1))
        });
        let total: u32 = CROWNS.iter().map(|(_, weight)| weight).sum();
        let mut roll = dice.below(total as usize) as u32;
        let crown = CROWNS
            .iter()
            .find(|(_, weight)| {
                let found = roll < *weight;
                roll = roll.saturating_sub(*weight);
                found
            })
            .map_or(NEON_BLUE, |(color, _)| *color);
        Self {
            windows,
            crown,
            dice,
        }
    }
}

/// One block of a tower, from `low` to `high` across and from `heights.0` to `heights.1` up, banded
/// with lit floors every `spacing` meters or windowed, and outlined in light along its top.
#[allow(clippy::too_many_arguments)]
fn storey(
    lit: &mut Geometry,
    neon: &mut Geometry,
    frame: &Frame,
    (low, high): (Vec2, Vec2),
    (bottom, top): (f32, f32),
    spacing: f32,
    color: Color,
    lights: &mut Lighting,
) {
    lit.block(
        frame,
        Vec3::new(low.x, bottom, low.y),
        Vec3::new(high.x, top, high.y),
        color,
    );
    match lights.windows {
        Some((tone, share)) => windows(
            neon,
            frame,
            (low, high),
            (bottom, top),
            spacing,
            (tone, share),
            &mut lights.dice,
        ),
        None => {
            let mut floor = bottom + spacing;
            while floor < top - spacing * 0.5 {
                band(neon, frame, (low, high), floor, FLOOR_BAND, NEON_FAINT);
                floor += spacing;
            }
        }
    }
    band(neon, frame, (low, high), top - 0.35, 0.35, lights.crown);
}

/// Windows on the four faces of the block from `low` to `high`, one row a floor, `share` of them
/// lit, most of them in `tone`: a quad of light each, standing a little proud of the face.
fn windows(
    neon: &mut Geometry,
    frame: &Frame,
    (low, high): (Vec2, Vec2),
    (bottom, top): (f32, f32),
    spacing: f32,
    (tone, share): (Color, f32),
    dice: &mut Dice,
) {
    let middle = (low + high) / 2.0;
    // Each face from one corner to the next across the frame's ground, `x` along the road and `y`
    // away from it, and the way it looks.
    let faces = [
        (Vec2::new(low.x, low.y), Vec2::new(high.x, low.y), Vec2::NEG_Y),
        (Vec2::new(high.x, low.y), Vec2::new(high.x, high.y), Vec2::X),
        (Vec2::new(high.x, high.y), Vec2::new(low.x, high.y), Vec2::Y),
        (Vec2::new(low.x, high.y), Vec2::new(low.x, low.y), Vec2::NEG_X),
    ];
    let floors = ((top - bottom) / spacing).floor() as usize;
    for (from, to, out) in faces {
        let width = from.distance(to);
        let columns = (width / WINDOW_PITCH).floor() as usize;
        if columns == 0 {
            continue;
        }
        let across = (to - from) / width;
        let margin = (width - columns as f32 * WINDOW_PITCH) / 2.0;
        let proud = out * PROUD;
        let corner = |at: Vec2, height: f32| {
            frame.point(Vec3::new(at.x + proud.x, height, at.y + proud.y))
        };
        for floor in 0..floors {
            let sill = bottom + (floor as f32 + (1.0 - WINDOW_HEIGHT) / 2.0) * spacing;
            let lintel = sill + WINDOW_HEIGHT * spacing;
            for column in 0..columns {
                if dice.range(0.0, 1.0) >= share {
                    continue;
                }
                let start = from
                    + across * (margin + (column as f32 + (1.0 - WINDOW_WIDTH) / 2.0) * WINDOW_PITCH);
                let end = start + across * WINDOW_WIDTH * WINDOW_PITCH;
                let light = if dice.range(0.0, 1.0) < WINDOWS_ODD {
                    WINDOW_LIGHTS[dice.below(WINDOW_LIGHTS.len())]
                } else {
                    tone
                };
                let (light, dimmed) = (light.to_linear(), dice.range(0.55, 1.0));
                neon.quad_facing_away(
                    [
                        corner(start, sill),
                        corner(end, sill),
                        corner(end, lintel),
                        corner(start, lintel),
                    ],
                    frame.point(Vec3::new(middle.x, sill, middle.y)),
                    Color::linear_rgb(
                        light.red * dimmed,
                        light.green * dimmed,
                        light.blue * dimmed,
                    ),
                );
            }
        }
    }
}

/// A band of light `thickness` meters tall around the block from `low` to `high`, standing a little
/// proud of its faces, from `height` up.
fn band(
    neon: &mut Geometry,
    frame: &Frame,
    (low, high): (Vec2, Vec2),
    height: f32,
    thickness: f32,
    color: Color,
) {
    neon.block(
        frame,
        Vec3::new(low.x - PROUD, height, low.y - PROUD),
        Vec3::new(high.x + PROUD, height + thickness, high.y + PROUD),
        color,
    );
}

/// A district: towers filling the ground beside the road, `size.x` meters along it from `at` and
/// `size.z` meters out, the tallest `size.y` meters over the road.
///
/// The towers are laid out in rows running along the road, the nearest row the lowest, and each one
/// is kept only if it stands clear of every part of the road, its own and any other, and of the
/// buildings already `standing`, to which it then adds its own. Around the inside of a turn the
/// rows close up, and the towers that no longer fit are simply left out; across the space between
/// two legs of a circuit, the far rows stop short of the other leg.
fn skyline(
    lit: &mut Geometry,
    neon: &mut Geometry,
    track: &Track,
    (at, side, offset): (f32, Side, f32),
    size: Vec3,
    standing: &mut Vec<[Vec2; 4]>,
    landmarks: &mut Landmarks,
) {
    let (length, tallest, depth) = (size.x, size.y, size.z);
    let mut dice = Dice::new(seed(&[at, side.sign(), offset, length, depth, tallest]));
    let clearance = DISTRICT_CLEARANCE.min(offset);

    let mut out = 0.0;
    let mut row = 0;
    while out < depth {
        let row_depth = dice
            .range(DISTRICT_TOWER.0, DISTRICT_TOWER.1)
            .min(depth - out);
        let mut along = dice.range(0.0, DISTRICT_GAP.1);
        while along < length {
            let width = dice
                .range(DISTRICT_TOWER.0, DISTRICT_TOWER.1)
                .min(length - along);
            let tower_depth = row_depth * dice.range(0.75, 1.0);
            let share = if row == 0 {
                dice.range(0.35, 0.75)
            } else {
                dice.range(0.5, 1.0)
            };
            let frame = beside_road(track, at + along + width / 2.0, side, offset + out);
            let size = Vec3::new(width, tallest * share, tower_depth);
            let footprint = ground_footprint(&frame, size);
            let tower_seed = dice.next();
            if width > DISTRICT_TOWER.0 * 0.5
                && clear_of_the_road(track, &footprint, clearance)
                && standing
                    .iter()
                    .all(|other| apart(&footprint, other, DISTRICT_GAP.0))
            {
                tower(lit, neon, &frame, size, tower_seed, landmarks);
                standing.push(footprint);
            }
            along += width + dice.range(DISTRICT_GAP.0, DISTRICT_GAP.1);
        }
        out += row_depth + dice.range(DISTRICT_GAP.0, DISTRICT_GAP.1);
        row += 1;
    }
}

/// The four corners of a tower's ground, on the simulation plane where the road is measured: `size`
/// across from the middle of the frame's face, as [`tower`] builds it.
fn ground_footprint(frame: &Frame, size: Vec3) -> [Vec2; 4] {
    let half = size.x / 2.0;
    [(-half, 0.0), (half, 0.0), (half, size.z), (-half, size.z)].map(|(along, out)| {
        let world = frame.point(Vec3::new(along, 0.0, out));
        Vec2::new(world.x, -world.z)
    })
}

/// Whether a footprint stands at least `clearance` meters outside the walls of every part of the
/// road: its corners, the middles of its sides and its middle are checked against the nearest
/// stretch of road to each.
fn clear_of_the_road(track: &Track, footprint: &[Vec2; 4], clearance: f32) -> bool {
    let middle = footprint.iter().copied().sum::<Vec2>() / 4.0;
    let sides = (0..4).map(|corner| (footprint[corner] + footprint[(corner + 1) % 4]) / 2.0);
    footprint
        .iter()
        .copied()
        .chain(sides)
        .chain([middle])
        .all(|position| {
            let projection = track.project(position);
            projection.lateral.abs() - track.half_width_at(projection.distance)
                >= WALL_THICKNESS + clearance
        })
}

/// Whether two convex footprints stand at least `gap` meters apart along some direction across
/// one of their sides: the separating axis test, with room to spare.
fn apart(a: &[Vec2; 4], b: &[Vec2; 4], gap: f32) -> bool {
    let axes = (0..4).flat_map(|corner| {
        [a, b].map(|shape| (shape[(corner + 1) % 4] - shape[corner]).perp().normalize_or_zero())
    });
    axes.into_iter().any(|axis| {
        let span = |shape: &[Vec2; 4]| {
            shape.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), corner| {
                let along = corner.dot(axis);
                (low.min(along), high.max(along))
            })
        };
        let ((a_low, a_high), (b_low, b_high)) = (span(a), span(b));
        a_high + gap <= b_low || b_high + gap <= a_low
    })
}

/// A long building following the road for `size.x` meters, `size.y` high over the road's level and
/// `size.z` deep, its face lined with lit floors and mullions, its roof edge a line of light.
fn facade(
    lit: &mut Geometry,
    neon: &mut Geometry,
    track: &Track,
    (at, side, offset): (f32, Side, f32),
    size: Vec3,
    landmarks: &mut Landmarks,
) {
    let (length, height, depth) = (size.x, size.y, size.z);
    let frames = swept_frames(track, at, side, offset, length);
    for frame in &frames {
        landmarks.rise_to(frame.height() + height);
    }
    lit.sweep(
        &frames,
        &[
            Vec2::new(0.0, 0.0),
            Vec2::new(0.0, height),
            Vec2::new(depth, height),
            Vec2::new(depth, 0.0),
        ],
        Vec2::new(depth / 2.0, height / 2.0),
        CITY,
    );
    swept_footing(lit, &frames, 0.0, depth, CITY);
    for (frame, inward) in [(frames.first(), 1.0_f32), (frames.last(), -1.0)] {
        let Some(frame) = frame else {
            continue;
        };
        let corners = [(0.0, 0.0), (depth, 0.0), (depth, height), (0.0, height)]
            .map(|(out, up)| frame.cross_section(Vec2::new(out, up)));
        let inside = frame.point(Vec3::new(inward, height / 2.0, depth / 2.0));
        lit.quad_facing_away(corners, inside, CITY);
    }

    // Floors of light across the face the road sees, a ribbon standing just proud of it.
    let spacing = FLOOR_SPACING.0;
    let mut floor = spacing;
    while floor < height - spacing * 0.5 {
        neon.sweep(
            &frames,
            &[
                Vec2::new(-PROUD, floor),
                Vec2::new(-PROUD, floor + FLOOR_BAND),
            ],
            Vec2::new(1.0, floor),
            NEON_FAINT,
        );
        floor += spacing;
    }
    // The roof's edges, on the road's side and on the far one, which other legs of the circuit see.
    for (out, inside) in [(-PROUD, 1.0), (depth + PROUD, depth - 1.0)] {
        neon.sweep(
            &frames,
            &[Vec2::new(out, height - 0.4), Vec2::new(out, height)],
            Vec2::new(inside, height - 0.2),
            NEON_BLUE,
        );
    }
    for frame in frames.iter().step_by(FACADE_MULLIONS) {
        neon.block(
            frame,
            Vec3::new(-0.15, 0.0, -PROUD - 0.05),
            Vec3::new(0.15, height - 0.4, -PROUD + 0.05),
            NEON_FAINT,
        );
    }
}

/// A roof over the road from `at` for `length` meters, `clearance` over the road everywhere across
/// it, so it banks with a banked turn, on walls standing just outside the road's own. It hangs from
/// the road like the walls do, down to the underside of its slab, and is ringed inside with neon.
fn tunnel(
    lit: &mut Geometry,
    neon: &mut Geometry,
    track: &Track,
    at: f32,
    length: f32,
    clearance: f32,
) {
    let sections = (length / TUNNEL_SECTION).ceil().max(1.0) as usize;
    let points: Vec<TrackPoint> = (0..=sections)
        .map(|section| track.point_at(at + length * section as f32 / sections as f32))
        .collect();
    let place = |point: &TrackPoint, lateral: f32, height: f32| {
        world::position(point.position + point.direction.perp() * lateral, height)
    };
    // The inner and outer faces of a wall, the roof's underside and top, and the wall's foot.
    let inner = |point: &TrackPoint| point.half_width + WALL_THICKNESS + TUNNEL_GAP;
    let outer = |point: &TrackPoint| inner(point) + TUNNEL_WALL;
    let under = |point: &TrackPoint, lateral: f32| track.height_beside(point, lateral) + clearance;
    let over = |point: &TrackPoint, lateral: f32| under(point, lateral) + TUNNEL_ROOF;
    let foot = |point: &TrackPoint, side: f32| {
        track.height_beside(point, side * point.half_width) - SLAB
    };
    // Across the roof: a share of the way from one side to the other, `reach` meters each side of
    // the centerline.
    let across = |share: f32, reach: f32| (1.0 - 2.0 * share) * reach;
    let strips = || {
        (0..TUNNEL_STRIPS).map(|strip| {
            (
                strip as f32 / TUNNEL_STRIPS as f32,
                (strip + 1) as f32 / TUNNEL_STRIPS as f32,
            )
        })
    };

    for pair in points.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        for side in [1.0_f32, -1.0] {
            // A point inside the wall: every face of it looks away from there.
            let middle = side * (inner(a) + outer(a)) / 2.0;
            let inside = place(a, middle, (foot(a, side) + under(a, middle)) / 2.0);
            // The inner face, up to the roof's underside, and the outer one, up to its top.
            for outside in [false, true] {
                let lateral = |point: &TrackPoint| {
                    side * if outside {
                        outer(point)
                    } else {
                        inner(point)
                    }
                };
                let top = |point: &TrackPoint| {
                    if outside {
                        over(point, lateral(point))
                    } else {
                        under(point, lateral(point))
                    }
                };
                lit.quad_facing_away(
                    [
                        place(a, lateral(a), foot(a, side)),
                        place(b, lateral(b), foot(b, side)),
                        place(b, lateral(b), top(b)),
                        place(a, lateral(a), top(a)),
                    ],
                    inside,
                    CITY,
                );
            }
            lit.quad_facing_away(
                [
                    place(a, side * inner(a), foot(a, side)),
                    place(b, side * inner(b), foot(b, side)),
                    place(b, side * outer(b), foot(b, side)),
                    place(a, side * outer(a), foot(a, side)),
                ],
                inside,
                CITY,
            );
        }
        // The roof: its underside between the walls, facing down, and its top across them.
        for (near, far) in strips() {
            let (ra, rb) = (inner(a), inner(b));
            let corners = [
                place(a, across(near, ra), under(a, across(near, ra))),
                place(a, across(far, ra), under(a, across(far, ra))),
                place(b, across(far, rb), under(b, across(far, rb))),
                place(b, across(near, rb), under(b, across(near, rb))),
            ];
            let middle = corners.iter().copied().sum::<Vec3>() / 4.0;
            lit.quad_facing_away(corners, middle + Vec3::Y, CITY);

            let (ra, rb) = (outer(a), outer(b));
            let corners = [
                place(a, across(near, ra), over(a, across(near, ra))),
                place(a, across(far, ra), over(a, across(far, ra))),
                place(b, across(far, rb), over(b, across(far, rb))),
                place(b, across(near, rb), over(b, across(near, rb))),
            ];
            let middle = corners.iter().copied().sum::<Vec3>() / 4.0;
            lit.quad_facing_away(corners, middle - Vec3::Y, CITY);
        }
    }

    // The two mouths: the wall and roof cut across, facing out of the tunnel, outlined in light.
    for (point, outward) in [(points[0], -1.0_f32), (points[sections], 1.0)] {
        let behind = world::direction(point.direction) * -outward;
        for side in [1.0_f32, -1.0] {
            let corners = [
                place(&point, side * inner(&point), foot(&point, side)),
                place(&point, side * outer(&point), foot(&point, side)),
                place(&point, side * outer(&point), over(&point, side * outer(&point))),
                place(&point, side * inner(&point), under(&point, side * inner(&point))),
            ];
            let middle = corners.iter().copied().sum::<Vec3>() / 4.0;
            lit.quad_facing_away(corners, middle + behind, CITY);
            // The light up the side of the mouth.
            let road = track.height_beside(&point, side * point.half_width);
            let rim = side * (inner(&point) + TUNNEL_RING_WIDTH);
            let light = [
                place(&point, side * inner(&point), road),
                place(&point, rim, road),
                place(&point, rim, under(&point, rim)),
                place(&point, side * inner(&point), under(&point, side * inner(&point))),
            ]
            .map(|corner| corner - behind * 0.05);
            neon.quad_facing_away(light, middle + behind, NEON_BLUE);
        }
        for (near, far) in strips() {
            let reach = inner(&point);
            let (l0, l1) = (across(near, reach), across(far, reach));
            let corners = [
                place(&point, l0, under(&point, l0)),
                place(&point, l1, under(&point, l1)),
                place(&point, l1, over(&point, l1)),
                place(&point, l0, over(&point, l0)),
            ];
            let middle = corners.iter().copied().sum::<Vec3>() / 4.0;
            lit.quad_facing_away(corners, middle + behind, CITY);
            // The light across the top of the mouth.
            let light = [
                place(&point, l0, under(&point, l0)),
                place(&point, l1, under(&point, l1)),
                place(&point, l1, under(&point, l1) + TUNNEL_RING_WIDTH),
                place(&point, l0, under(&point, l0) + TUNNEL_RING_WIDTH),
            ]
            .map(|corner| corner - behind * 0.05);
            neon.quad_facing_away(light, middle + behind, NEON_BLUE);
        }
    }

    // Rings of light round the inside, on the walls above the road's own and under the roof.
    let rings = (length / TUNNEL_RING).floor() as usize;
    for ring in 1..rings.max(1) {
        let start = at + ring as f32 * TUNNEL_RING;
        let (a, b) = (track.point_at(start), track.point_at(start + TUNNEL_RING_WIDTH));
        for side in [1.0_f32, -1.0] {
            let lateral = |point: &TrackPoint| side * (inner(point) - 2.0 * TUNNEL_GAP);
            let low = |point: &TrackPoint| {
                track.height_beside(point, side * point.half_width) + WALL_HEIGHT
            };
            let corners = [
                place(&a, lateral(&a), low(&a)),
                place(&b, lateral(&b), low(&b)),
                place(&b, lateral(&b), under(&b, lateral(&b))),
                place(&a, lateral(&a), under(&a, lateral(&a))),
            ];
            let outside = place(&a, side * outer(&a), low(&a));
            neon.quad_facing_away(corners, outside, NEON_BLUE);
        }
        for (near, far) in strips() {
            let lower = |point: &TrackPoint, lateral: f32| under(point, lateral) - 2.0 * TUNNEL_GAP;
            let (ra, rb) = (inner(&a), inner(&b));
            let corners = [
                place(&a, across(near, ra), lower(&a, across(near, ra))),
                place(&a, across(far, ra), lower(&a, across(far, ra))),
                place(&b, across(far, rb), lower(&b, across(far, rb))),
                place(&b, across(near, rb), lower(&b, across(near, rb))),
            ];
            let middle = corners.iter().copied().sum::<Vec3>() / 4.0;
            neon.quad_facing_away(corners, middle + Vec3::Y, NEON_BLUE);
        }
    }
}

/// Which way the road bends where a marker board stands: `1` to the left, `-1` to the right, `0`
/// where it runs straight. Read from the centerline either side of the board, so an arrow points
/// into the turn it marks whatever the file says.
fn bend_at(track: &Track, at: f32) -> f32 {
    /// How far either side of the board the bend is read, in meters.
    const REACH: f32 = 12.0;

    let before = track.point_at(at - REACH).direction;
    let after = track.point_at(at + REACH).direction;
    let turn = before.perp_dot(after);
    if turn.abs() < 0.02 {
        0.0
    } else {
        turn.signum()
    }
}

#[cfg(test)]
mod tests {
    use space_race_sim::track::{Elevation, Segment, TrackDescription};

    use super::*;

    /// A counter-clockwise oval, 16 m wide, its turns banked: the straights read as straight and
    /// the turns bend to the left.
    fn oval() -> Track {
        oval_through(Vec::new())
    }

    /// [`oval`], lifted by the heights of `elevation`.
    fn oval_through(elevation: Vec<Elevation>) -> Track {
        let turn = Segment::Turn {
            angle: 180.0,
            radius: 40.0,
            transition: 20.0,
            banking: 20.0,
        };
        Track::build(&TrackDescription {
            name: "Oval".into(),
            width: 16.0,
            segments: vec![
                Segment::Straight { length: 50.0 },
                turn,
                Segment::Straight { length: 100.0 },
                turn,
                Segment::Straight { length: 50.0 },
            ],
            elevation,
            narrows: Vec::new(),
            scenery: Vec::new(),
        })
        .unwrap()
    }

    /// Every kind of prop, on both sides and on a straight as well as in a turn.
    fn every_prop() -> Vec<Prop> {
        vec![
            Prop::Gantry {
                at: 0.0,
                clearance: 7.0,
                depth: 2.0,
            },
            Prop::Pylon {
                at: 30.0,
                side: Side::Left,
                offset: 2.0,
                height: 16.0,
            },
            Prop::Grandstand {
                at: 60.0,
                side: Side::Right,
                offset: 5.0,
                length: 40.0,
                rows: 6,
            },
            Prop::Billboard {
                at: 200.0,
                side: Side::Left,
                offset: 4.0,
                width: 10.0,
                height: 5.0,
            },
            Prop::Chevrons {
                at: 80.0,
                side: Side::Right,
                offset: 1.0,
                count: 3,
            },
            Prop::Monolith {
                at: 150.0,
                side: Side::Left,
                offset: 70.0,
                height: 32.0,
                width: 12.0,
            },
            Prop::Tower {
                at: 120.0,
                side: Side::Right,
                offset: 8.0,
                height: 40.0,
                width: 18.0,
                depth: 18.0,
            },
            Prop::Skyline {
                at: 160.0,
                side: Side::Right,
                offset: 6.0,
                length: 80.0,
                depth: 50.0,
                height: 40.0,
            },
            Prop::Facade {
                at: 100.0,
                side: Side::Right,
                offset: 4.0,
                length: 60.0,
                height: 18.0,
                depth: 14.0,
            },
            Prop::Tunnel {
                at: 180.0,
                length: 60.0,
                clearance: 9.0,
            },
        ]
    }

    fn build_prop(track: &Track, prop: Prop) -> (Geometry, Geometry) {
        let (mut lit, mut neon) = (Geometry::default(), Geometry::default());
        build(&mut lit, &mut neon, track, &[prop]);
        (lit, neon)
    }

    #[test]
    fn every_prop_builds_a_body_and_neon_on_it() {
        let track = oval();
        for prop in every_prop() {
            let (lit, neon) = build_prop(&track, prop);
            assert!(lit.triangles() > 0, "{prop:?} has no body");
            assert!(neon.triangles() > 0, "{prop:?} has no neon");
        }
    }

    #[test]
    fn no_prop_stands_over_the_road() {
        let track = oval();
        for prop in every_prop() {
            let (lit, neon) = build_prop(&track, prop);
            for position in lit.positions().chain(neon.positions()) {
                // Back to the simulation plane, where the road is measured.
                let on_plane = Vec2::new(position.x, -position.z);
                let projection = track.project(on_plane);
                let lateral = projection.lateral.abs();
                let road = track.surface_at(&projection).height;
                let clear = lateral > track.half_width()
                    // A gantry or a tunnel spans the road, well over the walls.
                    || position.y > road + WALL_HEIGHT + 2.0;
                assert!(clear, "{prop:?} reaches {lateral:.2} m from the centerline");
            }
        }
    }

    #[test]
    fn props_stand_where_the_file_places_them() {
        let track = oval();
        for side in [Side::Left, Side::Right] {
            let (lit, _) = build_prop(
                &track,
                Prop::Pylon {
                    at: 25.0,
                    side,
                    offset: 2.0,
                    height: 12.0,
                },
            );
            for position in lit.positions() {
                let projection = track.project(Vec2::new(position.x, -position.z));
                assert!((projection.distance - 25.0).abs() < 3.0, "{projection:?}");
                assert_eq!(projection.lateral.signum(), side.sign(), "{projection:?}");
            }
        }
    }

    /// The road climbs 30 m: a pylon beside it stands at its level, and its footing reaches down
    /// into the dark below, black at the bottom.
    #[test]
    fn props_stand_at_the_level_of_the_road_beside_them() {
        let track = oval_through(vec![Elevation {
            at: 0.0,
            height: 30.0,
        }]);
        let (lit, neon) = build_prop(
            &track,
            Prop::Pylon {
                at: 25.0,
                side: Side::Left,
                offset: 2.0,
                height: 12.0,
            },
        );
        let highest = neon.positions().map(|position| position.y).fold(0.0, f32::max);
        assert!((highest - (30.0 + 12.0 + 0.5)).abs() < 0.01, "{highest} m up");
        let lowest = lit.positions().map(|position| position.y).fold(0.0, f32::min);
        assert!((lowest - ABYSS).abs() < 0.01, "{lowest} m down");
        for (position, color) in lit.positions().zip(lit.colors()) {
            if (position.y - ABYSS).abs() < 0.01 {
                assert_eq!(&color[..3], &[0.0, 0.0, 0.0], "lit at the bottom");
            }
        }
    }

    /// A district given more ground than there is, across the infield of the oval and out the
    /// other side: it fills what it can, and none of its towers stands on or against the road. The
    /// clearance is checked at the corners and the middles of a tower's sides, so a turn can come a
    /// little closer between them.
    #[test]
    fn a_district_fills_the_ground_it_is_given_and_leaves_the_road_clear() {
        let track = oval();
        let (lit, _) = build_prop(
            &track,
            Prop::Skyline {
                at: 200.0,
                side: Side::Left,
                offset: 6.0,
                length: 90.0,
                depth: 200.0,
                height: 40.0,
            },
        );
        // Each tower is at least a footing and a body, 24 triangles.
        assert!(lit.triangles() >= 24 * 4, "{} triangles", lit.triangles());
        for position in lit.positions() {
            let projection = track.project(Vec2::new(position.x, -position.z));
            let outside = projection.lateral.abs() - track.half_width_at(projection.distance);
            assert!(
                outside > WALL_THICKNESS + DISTRICT_CLEARANCE - 0.5,
                "a tower {outside:.2} m from the road"
            );
        }
    }

    #[test]
    fn a_district_comes_out_the_same_every_time() {
        let track = oval();
        let district = Prop::Skyline {
            at: 160.0,
            side: Side::Right,
            offset: 6.0,
            length: 80.0,
            depth: 50.0,
            height: 40.0,
        };
        let (first, _) = build_prop(&track, district);
        let (second, _) = build_prop(&track, district);
        assert!(first.positions().eq(second.positions()));
    }

    #[test]
    fn footprints_apart_or_not() {
        let square = |x: f32| {
            [
                Vec2::new(x, 0.0),
                Vec2::new(x + 10.0, 0.0),
                Vec2::new(x + 10.0, 10.0),
                Vec2::new(x, 10.0),
            ]
        };
        assert!(apart(&square(0.0), &square(13.0), 2.0));
        assert!(!apart(&square(0.0), &square(11.0), 2.0));
        assert!(!apart(&square(0.0), &square(5.0), 2.0));
    }

    /// The roof of a tunnel stands `clearance` over the road wherever the road is, across it and
    /// along it, so a banked turn banks the roof too.
    #[test]
    fn a_tunnel_roof_follows_the_road_across() {
        let track = oval();
        // Through the first banked turn.
        let (lit, _) = build_prop(
            &track,
            Prop::Tunnel {
                at: 70.0,
                length: 60.0,
                clearance: 9.0,
            },
        );
        let mut roof = 0;
        for position in lit.positions() {
            let projection = track.project(Vec2::new(position.x, -position.z));
            let point = track.point_at(projection.distance);
            if projection.lateral.abs() < point.half_width {
                let road = track.height_beside(&point, projection.lateral);
                let above = position.y - road;
                assert!(above > 8.9, "the roof {above:.2} m over the road");
                roof += 1;
            }
        }
        assert!(roof > 0);
    }

    /// A tall tower carries its warning light on top of everything it is built of, and says where it
    /// stands; whatever its lights, a tower's body is the same.
    #[test]
    fn a_tall_tower_carries_a_warning_light_on_top() {
        let track = oval();
        let mut lit_towers = 0;
        for at in [100.0, 120.0, 140.0, 230.0, 260.0, 290.0] {
            let tower = Prop::Tower {
                at,
                side: Side::Right,
                offset: 8.0,
                height: 45.0,
                width: 18.0,
                depth: 18.0,
            };
            let (mut lit, mut neon) = (Geometry::default(), Geometry::default());
            let landmarks = build(&mut lit, &mut neon, &track, &[tower]);
            assert_eq!(landmarks.towers.len(), 1);
            let top = lit
                .positions()
                .chain(neon.positions())
                .map(|position| position.y)
                .fold(f32::NEG_INFINITY, f32::max);
            for beacon in &landmarks.beacons {
                assert!((beacon.y - top).abs() < 0.01, "{} against {top}", beacon.y);
                lit_towers += 1;
            }
            assert!(landmarks.highest >= top);
        }
        assert!(lit_towers > 0, "no tower had a warning light");
    }

    /// The windows of a tower lie on its faces, a little proud of them, never inside it nor out over
    /// the road.
    #[test]
    fn windows_lie_on_the_faces_of_their_tower() {
        let track = oval();
        let frame = beside_road(&track, 120.0, Side::Right, 8.0);
        let (low, high) = (Vec2::new(-9.0, 0.0), Vec2::new(9.0, 18.0));
        let mut neon = Geometry::default();
        windows(
            &mut neon,
            &frame,
            (low, high),
            (0.0, 40.0),
            4.0,
            (WINDOW_LIGHTS[0], 1.0),
            &mut Dice::new(1),
        );
        assert!(neon.triangles() > 50);
        for position in neon.positions() {
            let local = frame.local(position);
            let outside_x = (local.x - low.x).abs().min((local.x - high.x).abs());
            let outside_z = (local.z - low.y).abs().min((local.z - high.y).abs());
            assert!(
                (outside_x - PROUD).abs() < 1e-3 || (outside_z - PROUD).abs() < 1e-3,
                "{local}"
            );
            assert!((0.0..40.0).contains(&local.y), "{local}");
        }
    }

    /// A grandstand has a searchlight at each end of its top row, out beyond the wall.
    #[test]
    fn a_grandstand_has_a_searchlight_at_each_end() {
        let track = oval();
        let stand = Prop::Grandstand {
            at: 60.0,
            side: Side::Right,
            offset: 5.0,
            length: 40.0,
            rows: 6,
        };
        let (mut lit, mut neon) = (Geometry::default(), Geometry::default());
        let landmarks = build(&mut lit, &mut neon, &track, &[stand]);
        assert_eq!(landmarks.searchlights.len(), 2);
        for light in &landmarks.searchlights {
            let here = track.project(Vec2::new(light.x, -light.z));
            assert!(here.lateral < -track.half_width() - 5.0, "{here:?}");
            assert!(light.y > 6.0 * ROW_RISE, "{light}");
        }
    }

    #[test]
    fn a_marker_board_reads_the_bend_from_the_road() {
        let track = oval();
        // The oval turns left, and its straights are straight.
        assert_eq!(bend_at(&track, 25.0), 0.0);
        assert_eq!(bend_at(&track, 110.0), 1.0);
        assert_eq!(bend_at(&track, 250.0), 0.0);
    }

    #[test]
    fn arrows_point_into_the_turn_from_either_side() {
        let track = oval();
        // The oval turns left. A board on the right of the road points at the road, one on the
        // left points away from it: either way, the way the road goes.
        for (side, tip_toward_road) in [(Side::Right, true), (Side::Left, false)] {
            let at = 110.0;
            let (_, neon) = build_prop(
                &track,
                Prop::Chevrons {
                    at,
                    side,
                    offset: 1.0,
                    count: 3,
                },
            );
            // Where the arrows meet in a point the neon stands at one height; where they open, at
            // two. That tells the tip end of the board from the tail end.
            let frame = beside_road(&track, at, side, 1.0);
            let spread = |edge: f32| {
                let heights = neon
                    .positions()
                    .map(|position| frame.local(position))
                    .filter(|local| (local.z - edge).abs() < 0.6)
                    .map(|local| local.y);
                let (low, high) = heights.fold((f32::INFINITY, f32::NEG_INFINITY), |bounds, y| {
                    (bounds.0.min(y), bounds.1.max(y))
                });
                high - low
            };
            let (tip, tail) = if tip_toward_road {
                (0.3, CHEVRON_WIDTH - 0.3)
            } else {
                (CHEVRON_WIDTH - 0.3, 0.3)
            };
            assert!(
                spread(tip) < spread(tail),
                "{side:?}: {} at the tip, {} at the tail",
                spread(tip),
                spread(tail)
            );
        }
    }
}
