//! The cars, as drawn: low racing hovercraft in the spirit of F-Zero's machines, built from
//! flat-shaded triangles like everything else in the game.
//!
//! There are four pods to choose from in the garage, every one of them driving the same, since the
//! handling is the simulation's and the simulation knows nothing of looks:
//!
//! - **Arrow**, the first: a long wedge of a hull with a pointed nose, a cockpit bubble on its back,
//!   and two engine pods slung beside it on a rear wing, each wearing a fin.
//! - **Lance**: a needle, longer and slimmer, a delta wing at its tail with a fin at each tip, a
//!   pair of canards near the nose, and one big thruster.
//! - **Twin**: a catamaran, two slim hulls joined by a bridge, the cockpit on a nacelle between them.
//! - **Manta**: a flying wing, wide and flat, its edge drawn in neon, two tail fins and three
//!   thrusters across its back.
//!
//! Each floats above the road on nothing: its underside clears it by a quarter of a meter, a soft
//! shadow sits it over the road, and colored light spilling out from under its edge says it
//! hovers. Its thrusters glow at the back, and their flames lengthen with its speed and roar out
//! on a boost. The player picks its paint, which covers the tops of the hull and pods, and its
//! neon, which lines it and lights the road under it (see [`PAINTS`], [`NEONS`]); the neon and the
//! thrusters are drawn brighter than white, so the bloom gives them a halo.
//!
//! The models are written along `+x`, the direction of a zero heading, `y` up and `z` to the right,
//! their origin on the road under the middle of the car, where the simulation places it. They are
//! 4.4 to 4.9 m long and 2.5 to 3.1 m wide, about the 2.4 m circle the simulation collides.

use bevy::color::LinearRgba;
use bevy::prelude::*;
use star_racer_protocol::CarLook;

use crate::geometry::{Frame, Geometry};

/// The pods, in the order the garage offers them and the wire numbers them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CarModel {
    Arrow,
    Lance,
    Twin,
    Manta,
}

impl CarModel {
    pub const ALL: [Self; 4] = [Self::Arrow, Self::Lance, Self::Twin, Self::Manta];

    /// The pod numbered `index` on the wire; an unknown one is the first.
    pub fn of(index: u8) -> Self {
        Self::ALL
            .get(usize::from(index))
            .copied()
            .unwrap_or(Self::Arrow)
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Arrow => "ARROW",
            Self::Lance => "LANCE",
            Self::Twin => "TWIN",
            Self::Manta => "MANTA",
        }
    }

    /// A line about it, for the garage.
    pub fn blurb(self) -> &'static str {
        match self {
            Self::Arrow => "The first pod: a wedge between two engines.",
            Self::Lance => "A needle with a delta wing and one big thruster.",
            Self::Twin => "Two hulls, a bridge, and the cockpit between them.",
            Self::Manta => "A flying wing, flat and wide, lined in neon.",
        }
    }
}

/// The paints, in the order the wire numbers them, with their names.
pub const PAINTS: [(&str, Color); 10] = [
    ("ORANGE", Color::srgb(0.95, 0.36, 0.08)),
    ("BLUE", Color::srgb(0.14, 0.36, 0.86)),
    ("CRIMSON", Color::srgb(0.78, 0.07, 0.12)),
    ("LIME", Color::srgb(0.45, 0.80, 0.12)),
    ("GOLD", Color::srgb(0.95, 0.70, 0.12)),
    ("VIOLET", Color::srgb(0.46, 0.18, 0.82)),
    ("TEAL", Color::srgb(0.05, 0.60, 0.60)),
    ("PEARL", Color::srgb(0.85, 0.86, 0.90)),
    ("ROSE", Color::srgb(0.95, 0.30, 0.60)),
    ("CARBON", Color::srgb(0.20, 0.21, 0.24)),
];

/// The neon colors, brighter than white so they bloom, with their names.
pub const NEONS: [(&str, LinearRgba); 8] = [
    ("ORANGE", LinearRgba::new(3.2, 0.95, 0.15, 1.0)),
    ("MAGENTA", LinearRgba::new(2.4, 0.35, 2.6, 1.0)),
    ("CYAN", LinearRgba::new(0.2, 2.4, 3.2, 1.0)),
    ("GREEN", LinearRgba::new(0.4, 3.0, 0.6, 1.0)),
    ("RED", LinearRgba::new(3.2, 0.25, 0.25, 1.0)),
    ("YELLOW", LinearRgba::new(3.0, 2.6, 0.3, 1.0)),
    ("PURPLE", LinearRgba::new(1.6, 0.5, 3.2, 1.0)),
    ("WHITE", LinearRgba::new(2.6, 2.6, 2.8, 1.0)),
];

/// How a car is built and painted.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Livery {
    pub model: CarModel,
    /// The hull's top and the pods' tops.
    pub body: Color,
    /// The neon lines along the hull and the light under it.
    pub accent: LinearRgba,
}

impl Livery {
    /// What a look on the wire draws, anything out of range brought back in.
    pub fn of(look: CarLook) -> Self {
        let look = look.checked();
        Self {
            model: CarModel::of(look.model),
            body: PAINTS[usize::from(look.paint)].1,
            accent: NEONS[usize::from(look.neon)].1,
        }
    }
}

/// The dark parts: the hull's flanks under the neon, the pods' sides, the wings.
const GRAPHITE: Color = Color::srgb(0.075, 0.08, 0.095);
/// The underside, which the road never lights.
const UNDERSIDE: Color = Color::srgb(0.025, 0.025, 0.035);
/// The racing stripe down the middle of the hull.
const STRIPE: Color = Color::srgb(0.88, 0.88, 0.9);
/// The cockpit glass: dark, with a lighter sheen on top where the sky shows in it.
const GLASS: Color = Color::srgb(0.03, 0.05, 0.09);
const GLASS_SHEEN: Color = Color::srgb(0.12, 0.26, 0.38);
/// The inside of a thruster, white hot and a little blue.
const THRUSTER: LinearRgba = LinearRgba::new(2.2, 2.9, 3.6, 1.0);
/// A thruster's flame where it leaves the nozzle, fading to nothing at its tip.
const FLAME: LinearRgba = LinearRgba::new(0.28, 0.5, 1.5, 1.0);

/// A cross-section of a hull, pod or cockpit: where it stands along the car, how wide it is either
/// side of its middle, and how high its underside and its top are.
#[derive(Debug, Clone, Copy)]
struct Section {
    x: f32,
    half_width: f32,
    bottom: f32,
    top: f32,
}

const fn section(x: f32, half_width: f32, bottom: f32, top: f32) -> Section {
    Section {
        x,
        half_width,
        bottom,
        top,
    }
}

/// A thruster: where the middle of its mouth is, and its half width (along `z`) and half height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Nozzle {
    pub at: Vec3,
    pub half: Vec2,
}

const fn nozzle(x: f32, y: f32, z: f32, half_z: f32, half_y: f32) -> Nozzle {
    Nozzle {
        at: Vec3::new(x, y, z),
        half: Vec2::new(half_z, half_y),
    }
}

// Arrow.

/// The hull, from its tail to the tip of its nose: widest just ahead of the tail, where the wing
/// joins it, and drawn out to a point.
const ARROW_HULL: [Section; 6] = [
    section(-1.95, 0.60, 0.34, 0.74),
    section(-1.55, 0.74, 0.28, 0.86),
    section(-0.40, 0.78, 0.26, 0.86),
    section(0.70, 0.64, 0.27, 0.72),
    section(1.60, 0.40, 0.29, 0.56),
    section(2.40, 0.07, 0.33, 0.39),
];

/// An engine pod, either side of the hull, `ARROW_POD_OFFSET` meters out.
const ARROW_POD: [Section; 4] = [
    section(-2.15, 0.26, 0.30, 0.66),
    section(-1.80, 0.30, 0.26, 0.72),
    section(-0.20, 0.27, 0.27, 0.66),
    section(0.80, 0.08, 0.37, 0.47),
];
const ARROW_POD_OFFSET: f32 = 1.2;

/// The cockpit, a bubble on the hull's back.
const ARROW_COCKPIT: [Section; 4] = [
    section(-0.85, 0.20, 0.82, 0.87),
    section(-0.40, 0.34, 0.84, 1.13),
    section(0.35, 0.32, 0.79, 1.06),
    section(0.95, 0.09, 0.70, 0.76),
];

const ARROW_NOZZLES: [Nozzle; 4] = [
    nozzle(-1.97, 0.55, -0.34, 0.15, 0.09),
    nozzle(-1.97, 0.55, 0.34, 0.15, 0.09),
    nozzle(-2.17, 0.48, -ARROW_POD_OFFSET, 0.15, 0.13),
    nozzle(-2.17, 0.48, ARROW_POD_OFFSET, 0.15, 0.13),
];

// Lance.

const LANCE_HULL: [Section; 5] = [
    section(-2.20, 0.50, 0.32, 0.80),
    section(-1.70, 0.58, 0.27, 0.88),
    section(-0.20, 0.50, 0.27, 0.78),
    section(1.30, 0.32, 0.29, 0.58),
    section(2.70, 0.05, 0.35, 0.40),
];

const LANCE_COCKPIT: [Section; 4] = [
    section(-1.25, 0.16, 0.84, 0.89),
    section(-0.85, 0.28, 0.86, 1.12),
    section(-0.15, 0.26, 0.79, 1.03),
    section(0.35, 0.08, 0.70, 0.76),
];

/// How far out the tips of the delta wing are, either side.
const LANCE_SPAN: f32 = 1.38;

const LANCE_NOZZLES: [Nozzle; 3] = [
    nozzle(-2.22, 0.56, 0.0, 0.30, 0.19),
    nozzle(-2.02, 0.47, -LANCE_SPAN, 0.07, 0.06),
    nozzle(-2.02, 0.47, LANCE_SPAN, 0.07, 0.06),
];

// Twin.

/// Each of the two hulls, `TWIN_OFFSET` meters either side of the middle.
const TWIN_HULL: [Section; 5] = [
    section(-2.10, 0.34, 0.30, 0.70),
    section(-1.70, 0.40, 0.26, 0.78),
    section(0.20, 0.38, 0.26, 0.74),
    section(1.50, 0.24, 0.28, 0.58),
    section(2.35, 0.05, 0.33, 0.38),
];
const TWIN_OFFSET: f32 = 0.92;

/// The nacelle between the hulls, which carries the cockpit.
const TWIN_NACELLE: [Section; 4] = [
    section(-1.40, 0.22, 0.44, 0.70),
    section(-0.90, 0.30, 0.42, 0.84),
    section(0.50, 0.28, 0.42, 0.78),
    section(1.45, 0.06, 0.50, 0.56),
];

const TWIN_COCKPIT: [Section; 4] = [
    section(-0.70, 0.16, 0.81, 0.85),
    section(-0.30, 0.25, 0.83, 1.07),
    section(0.40, 0.22, 0.77, 0.98),
    section(0.85, 0.06, 0.72, 0.76),
];

const TWIN_NOZZLES: [Nozzle; 3] = [
    nozzle(-2.12, 0.50, -TWIN_OFFSET, 0.20, 0.13),
    nozzle(-2.12, 0.50, TWIN_OFFSET, 0.20, 0.13),
    nozzle(-1.42, 0.57, 0.0, 0.12, 0.08),
];

// Manta.

/// The wing, one wide flat body: broadest at the back, swept to a blunt point.
const MANTA_WING: [Section; 5] = [
    section(-1.90, 1.40, 0.30, 0.52),
    section(-1.45, 1.55, 0.27, 0.60),
    section(-0.30, 1.25, 0.26, 0.66),
    section(0.95, 0.68, 0.27, 0.56),
    section(2.15, 0.10, 0.31, 0.37),
];

const MANTA_COCKPIT: [Section; 4] = [
    section(-0.60, 0.18, 0.62, 0.66),
    section(-0.20, 0.30, 0.64, 0.92),
    section(0.50, 0.26, 0.60, 0.86),
    section(0.95, 0.07, 0.56, 0.60),
];

const MANTA_FIN: f32 = 0.72;

const MANTA_NOZZLES: [Nozzle; 3] = [
    nozzle(-1.92, 0.42, -0.85, 0.22, 0.08),
    nozzle(-1.92, 0.42, 0.0, 0.22, 0.08),
    nozzle(-1.92, 0.42, 0.85, 0.22, 0.08),
];

/// Where a pod's thrusters are, whose flames are drawn from them.
pub fn nozzles(model: CarModel) -> &'static [Nozzle] {
    match model {
        CarModel::Arrow => &ARROW_NOZZLES,
        CarModel::Lance => &LANCE_NOZZLES,
        CarModel::Twin => &TWIN_NOZZLES,
        CarModel::Manta => &MANTA_NOZZLES,
    }
}

/// The eight points round a section, in order, as `(z, y)`: the top, the shoulder, the widest
/// point, the lower flank, the keel, and back up the other side.
fn ring(section: &Section) -> [Vec2; 8] {
    let Section {
        half_width: w,
        bottom,
        top,
        ..
    } = *section;
    let height = top - bottom;
    let shoulder = Vec2::new(0.62 * w, top - 0.08 * height);
    let widest = Vec2::new(w, bottom + 0.55 * height);
    let flank = Vec2::new(0.8 * w, bottom);
    [
        Vec2::new(0.0, top),
        shoulder,
        widest,
        flank,
        Vec2::new(0.0, bottom - 0.02),
        flank * Vec2::new(-1.0, 1.0),
        widest * Vec2::new(-1.0, 1.0),
        shoulder * Vec2::new(-1.0, 1.0),
    ]
}

/// The colors of the eight faces between the points of [`ring`]: the top two and the upper flanks
/// in `upper`, the lower flanks in `flank`, the underside in `under`.
fn face_colors(upper: Color, flank: Color, under: Color) -> [Color; 8] {
    [upper, upper, flank, under, under, flank, upper, upper]
}

/// The painted body colors: the paint on top, graphite flanks, the dark underside.
fn painted(livery: &Livery) -> [Color; 8] {
    face_colors(livery.body, GRAPHITE, UNDERSIDE)
}

fn glass() -> [Color; 8] {
    face_colors(GLASS_SHEEN, GLASS, GLASS)
}

/// Builds a closed body through `sections`, `z` meters to the side of the car's middle, its faces
/// colored by `colors` and its two ends closed in `tail` and `nose`.
fn loft(
    geometry: &mut Geometry,
    sections: &[Section],
    z: f32,
    colors: [Color; 8],
    tail: Color,
    nose: Color,
) {
    let point = |section: &Section, at: Vec2| Vec3::new(section.x, at.y, z + at.x);
    let middle = |section: &Section| Vec3::new(section.x, (section.bottom + section.top) / 2.0, z);
    for pair in sections.windows(2) {
        let (near, far) = (&pair[0], &pair[1]);
        let (near_ring, far_ring) = (ring(near), ring(far));
        let inside = (middle(near) + middle(far)) / 2.0;
        for face in 0..8 {
            let next = (face + 1) % 8;
            geometry.quad_facing_away(
                [
                    point(near, near_ring[face]),
                    point(near, near_ring[next]),
                    point(far, far_ring[next]),
                    point(far, far_ring[face]),
                ],
                inside,
                colors[face],
            );
        }
    }
    // The ends, as fans of triangles round their middle, each turned away from the body.
    for (end, color, inward) in [
        (&sections[0], tail, 1.0),
        (&sections[sections.len() - 1], nose, -1.0),
    ] {
        let center = middle(end);
        let inside = center + Vec3::X * inward;
        let points = ring(end);
        for face in 0..8 {
            let a = point(end, points[face]);
            let b = point(end, points[(face + 1) % 8]);
            geometry.quad_facing_away([center, a, b, b], inside, color);
        }
    }
}

/// A white stripe down the middle of a body, just over its top, along each of `runs`.
fn stripe(geometry: &mut Geometry, sections: &[Section], z: f32, runs: &[(f32, f32)]) {
    let top_at = |x: f32| {
        let pair = sections
            .windows(2)
            .find(|pair| x >= pair[0].x && x <= pair[1].x)
            .expect("the stripe stays on the body");
        let share = (x - pair[0].x) / (pair[1].x - pair[0].x);
        pair[0].top + (pair[1].top - pair[0].top) * share + 0.006
    };
    for &(from, to) in runs {
        let steps = 4;
        for step in 0..steps {
            let a = from + (to - from) * step as f32 / steps as f32;
            let b = from + (to - from) * (step + 1) as f32 / steps as f32;
            geometry.quad_facing_up(
                [
                    Vec3::new(a, top_at(a), z - 0.09),
                    Vec3::new(b, top_at(b), z - 0.09),
                    Vec3::new(b, top_at(b), z + 0.09),
                    Vec3::new(a, top_at(a), z + 0.09),
                ],
                STRIPE,
            );
        }
    }
}

/// Everything of a car that takes the light: hulls, cockpit, pods, wings and fins.
pub fn hull_geometry(livery: &Livery) -> Geometry {
    let mut geometry = Geometry::default();
    let frame = Frame::new(Vec3::ZERO, Vec3::X, Vec3::Z);
    let g = &mut geometry;
    match livery.model {
        CarModel::Arrow => {
            loft(g, &ARROW_HULL, 0.0, painted(livery), GRAPHITE, livery.body);
            loft(g, &ARROW_COCKPIT, 0.0, glass(), GLASS, GLASS);
            for side in [-1.0, 1.0] {
                let z = side * ARROW_POD_OFFSET;
                loft(g, &ARROW_POD, z, painted(livery), GRAPHITE, GRAPHITE);
                // A fin on each pod, raked back and narrowing as it rises.
                g.tapered_block(
                    &frame,
                    Vec3::new(-2.05, 0.66, z - 0.035),
                    Vec3::new(-1.35, 1.08, z + 0.035),
                    0.35,
                    GRAPHITE,
                );
            }
            // The rear wing the pods hang from, through the hull, and a splitter under the nose.
            g.block(
                &frame,
                Vec3::new(-1.75, 0.44, -ARROW_POD_OFFSET),
                Vec3::new(-0.55, 0.52, ARROW_POD_OFFSET),
                GRAPHITE,
            );
            g.block(
                &frame,
                Vec3::new(1.05, 0.25, -0.95),
                Vec3::new(1.85, 0.29, 0.95),
                GRAPHITE,
            );
            stripe(g, &ARROW_HULL, 0.0, &[(-1.9, -0.9), (0.95, 2.1)]);
        }
        CarModel::Lance => {
            loft(g, &LANCE_HULL, 0.0, painted(livery), GRAPHITE, livery.body);
            loft(g, &LANCE_COCKPIT, 0.0, glass(), GLASS, GLASS);
            // The delta wing: a plate swept back, wider at its trailing edge, in paint.
            for side in [-1.0f32, 1.0] {
                let tip = side * LANCE_SPAN;
                let root = side * 0.45;
                let (low, high) = (0.42, 0.48);
                // Top, bottom and the swept leading edge.
                let corners = |y: f32| {
                    [
                        Vec3::new(-2.05, y, root),
                        Vec3::new(-2.05, y, tip),
                        Vec3::new(-1.75, y, tip),
                        Vec3::new(-0.35, y, root),
                    ]
                };
                let inside = Vec3::new(-1.3, (low + high) / 2.0, side * 0.8);
                let top = corners(high);
                let bottom = corners(low);
                g.quad_facing_away(top, inside, livery.body);
                g.quad_facing_away(bottom, inside, UNDERSIDE);
                for edge in 0..4 {
                    let next = (edge + 1) % 4;
                    g.quad_facing_away(
                        [bottom[edge], bottom[next], top[next], top[edge]],
                        inside,
                        GRAPHITE,
                    );
                }
                // A fin standing at each wing tip.
                g.tapered_block(
                    &frame,
                    Vec3::new(-2.05, 0.48, tip - 0.035),
                    Vec3::new(-1.55, 0.98, tip + 0.035),
                    0.4,
                    GRAPHITE,
                );
            }
            // Canards near the nose.
            g.tapered_block(
                &frame,
                Vec3::new(0.95, 0.40, -0.78),
                Vec3::new(1.45, 0.44, 0.78),
                0.9,
                GRAPHITE,
            );
            stripe(g, &LANCE_HULL, 0.0, &[(-2.1, -1.3), (0.4, 2.4)]);
        }
        CarModel::Twin => {
            for side in [-1.0, 1.0] {
                let z = side * TWIN_OFFSET;
                loft(g, &TWIN_HULL, z, painted(livery), GRAPHITE, livery.body);
                stripe(g, &TWIN_HULL, z, &[(0.3, 2.1)]);
            }
            loft(g, &TWIN_NACELLE, 0.0, painted(livery), GRAPHITE, GRAPHITE);
            loft(g, &TWIN_COCKPIT, 0.0, glass(), GLASS, GLASS);
            // The bridges between the hulls, fore and aft, and a fin on the nacelle.
            for (from, to) in [(0.25, 0.95), (-1.75, -0.95)] {
                g.block(
                    &frame,
                    Vec3::new(from, 0.48, -TWIN_OFFSET),
                    Vec3::new(to, 0.55, TWIN_OFFSET),
                    GRAPHITE,
                );
            }
            g.tapered_block(
                &frame,
                Vec3::new(-1.35, 0.70, -0.035),
                Vec3::new(-0.75, 1.15, 0.035),
                0.35,
                GRAPHITE,
            );
        }
        CarModel::Manta => {
            loft(g, &MANTA_WING, 0.0, painted(livery), GRAPHITE, livery.body);
            loft(g, &MANTA_COCKPIT, 0.0, glass(), GLASS, GLASS);
            for side in [-1.0, 1.0] {
                let z = side * MANTA_FIN;
                g.tapered_block(
                    &frame,
                    Vec3::new(-1.90, 0.55, z - 0.035),
                    Vec3::new(-1.20, 1.02, z + 0.035),
                    0.35,
                    GRAPHITE,
                );
            }
            stripe(g, &MANTA_WING, 0.0, &[(-1.85, -0.75), (1.0, 1.9)]);
        }
    }
    geometry
}

/// A thruster's glowing mouth, just behind the tail it sits in.
fn nozzle_light(geometry: &mut Geometry, frame: &Frame, nozzle: &Nozzle) {
    let Nozzle { at, half } = *nozzle;
    geometry.block(
        frame,
        Vec3::new(at.x - 0.02, at.y - half.y, at.z - half.x),
        Vec3::new(at.x + 0.03, at.y + half.y, at.z + half.x),
        Color::LinearRgba(THRUSTER),
    );
}

/// A neon line through `points`.
fn neon(geometry: &mut Geometry, frame: &Frame, points: &[Vec3], width: f32, color: Color) {
    for pair in points.windows(2) {
        geometry.bar(frame, pair[0], pair[1], width, color);
    }
}

/// The line along the widest point of a body, on one side, from its tail toward its nose, leaving
/// out its last `skip` sections.
fn widest_line(sections: &[Section], z: f32, side: f32, skip: usize) -> Vec<Vec3> {
    sections[..sections.len() - skip]
        .iter()
        .map(|section| {
            let widest = ring(section)[2];
            Vec3::new(section.x, widest.y, z + side * (widest.x + 0.01))
        })
        .collect()
}

/// The parts that shine by themselves: the thrusters' insides and the neon lines along the hull,
/// drawn unlit and brighter than white.
pub fn lights_geometry(livery: &Livery) -> Geometry {
    let mut geometry = Geometry::default();
    let frame = Frame::new(Vec3::ZERO, Vec3::X, Vec3::Z);
    let accent = Color::LinearRgba(livery.accent);
    let g = &mut geometry;
    for nozzle in nozzles(livery.model) {
        nozzle_light(g, &frame, nozzle);
    }
    for side in [-1.0, 1.0] {
        match livery.model {
            CarModel::Arrow => {
                neon(g, &frame, &widest_line(&ARROW_HULL, 0.0, side, 1), 0.045, accent);
                // And one along the top of each pod.
                let pod_line: Vec<Vec3> = ARROW_POD[..3]
                    .iter()
                    .map(|section| {
                        Vec3::new(section.x, section.top + 0.01, side * ARROW_POD_OFFSET)
                    })
                    .collect();
                neon(g, &frame, &pod_line, 0.04, accent);
            }
            CarModel::Lance => {
                neon(g, &frame, &widest_line(&LANCE_HULL, 0.0, side, 1), 0.045, accent);
                // Along the swept leading edge of the wing.
                neon(
                    g,
                    &frame,
                    &[
                        Vec3::new(-0.40, 0.49, side * 0.47),
                        Vec3::new(-1.74, 0.49, side * LANCE_SPAN),
                    ],
                    0.04,
                    accent,
                );
            }
            CarModel::Twin => {
                // Along the outer flank of each hull.
                let line = widest_line(&TWIN_HULL, side * TWIN_OFFSET, side, 1);
                neon(g, &frame, &line, 0.045, accent);
            }
            CarModel::Manta => {
                // The whole edge of the wing, nose to tail, is the neon.
                neon(g, &frame, &widest_line(&MANTA_WING, 0.0, side, 0), 0.05, accent);
            }
        }
    }
    if livery.model == CarModel::Twin {
        // Down the nacelle's back.
        let line: Vec<Vec3> = TWIN_NACELLE[..3]
            .iter()
            .map(|section| Vec3::new(section.x, section.top + 0.01, 0.0))
            .collect();
        neon(g, &frame, &line, 0.04, accent);
    }
    geometry
}

/// A pod's outline on the road, as an oval: half its length and half its width, about the same for
/// every pod.
/// The light and the shadow under the car are drawn in multiples of it.
const FOOTPRINT: Vec2 = Vec2::new(2.25, 1.35);

/// Around the oval the light and the shadow are drawn as, in this many steps: enough that its
/// rim never shows a corner.
const OVAL_STEPS: usize = 24;

/// Concentric ovals on the road under the car, from its middle outward: each ring is a size, in
/// multiples of [`FOOTPRINT`], and the color everything at that size takes, blended between rings.
/// A first ring of size 0 is the middle itself. The road may curve away under a flat shape on a
/// banked turn, so whatever is drawn this way fades to nothing at its rim, where it would sink.
fn oval_rings(height: f32, rings: &[(f32, Color)]) -> Geometry {
    let mut geometry = Geometry::default();
    let point = |size: f32, step: usize| {
        let angle = step as f32 / OVAL_STEPS as f32 * std::f32::consts::TAU;
        let at = FOOTPRINT * size * Vec2::new(angle.cos(), angle.sin());
        Vec3::new(at.x, height, at.y)
    };
    for pair in rings.windows(2) {
        let ((inner, inner_color), (outer, outer_color)) = (pair[0], pair[1]);
        for step in 0..OVAL_STEPS {
            geometry.quad_shaded(
                [
                    point(inner, step),
                    point(inner, step + 1),
                    point(outer, step + 1),
                    point(outer, step),
                ],
                Vec3::Y,
                [inner_color, inner_color, outer_color, outer_color],
            );
        }
    }
    geometry
}

/// The light the car throws on the road under it, added to whatever it falls on. It spills out
/// from under the hull's edge: none in the middle, which the hull hides and the shadow darkens,
/// most of it just outside the hull's outline, and none a meter and a half beyond.
///
/// It used to be a rectangle as wide as the car and a half, evenly lit to its middle: from the
/// chase camera a slab of muddy orange, wider than the car, whose straight edge cut across a
/// banked road.
pub fn under_glow_geometry(livery: &Livery) -> Geometry {
    let glow = |share: f32| {
        Color::LinearRgba(LinearRgba::new(
            livery.accent.red * share,
            livery.accent.green * share,
            livery.accent.blue * share,
            1.0,
        ))
    };
    oval_rings(
        0.11,
        &[
            (0.7, glow(0.0)),
            (1.0, glow(GLOW)),
            (1.25, glow(GLOW * 0.4)),
            (1.6, glow(0.0)),
        ],
    )
}

/// How much of a livery's neon the light under the car is at its brightest, just outside the hull.
const GLOW: f32 = 0.1;

/// The shadow under the car: a dark oval, darkest under the middle of the hull and fading out by
/// the hull's edge, laid over the road with its own transparency (see `race.rs`). The moon casts
/// a shadow of the car too, but it is nearly overhead and dim against the night's ambient, so the
/// car hovered over nothing; this is what sits it over the road, the way a hovercraft's air
/// cushion would darken what it presses on.
pub fn shadow_geometry() -> Geometry {
    let shade = |alpha: f32| Color::LinearRgba(LinearRgba::new(0.0, 0.0, 0.0, alpha));
    oval_rings(
        0.09,
        &[
            (0.0, shade(SHADOW)),
            (0.55, shade(SHADOW)),
            (0.9, shade(SHADOW * 0.55)),
            (1.15, shade(0.0)),
        ],
    )
}

/// How dark the shadow is at its darkest, from 0 to 1.
const SHADOW: f32 = 0.7;

/// One thruster's flame, one meter long and pointing back from the origin, its mouth a unit square
/// either side of it: the entity it hangs from stands at the nozzle, scales it across to the
/// nozzle's size and along `x` to the length the speed calls for (see [`flame_scale`]). Its color
/// fades from the mouth to nothing at the tip, and it is added to what is behind it.
pub fn flame_geometry() -> Geometry {
    let mut geometry = Geometry::default();
    let base = Color::LinearRgba(FLAME);
    let tip_color = Color::LinearRgba(LinearRgba::BLACK);
    let tip = -Vec3::X;
    let corners = [
        Vec3::new(0.0, -1.0, -1.0),
        Vec3::new(0.0, -1.0, 1.0),
        Vec3::new(0.0, 1.0, 1.0),
        Vec3::new(0.0, 1.0, -1.0),
    ];
    let inside = -Vec3::X * 0.3;
    for face in 0..4 {
        let a = corners[face];
        let b = corners[(face + 1) % 4];
        geometry.quad_facing_away_shaded([a, b, tip, tip], inside, [base, base, tip_color, tip_color]);
    }
    geometry
}

/// Where a nozzle's flame hangs from: just behind its mouth.
pub fn flame_origin(nozzle: &Nozzle) -> Vec3 {
    nozzle.at - Vec3::X * 0.02
}

/// How a nozzle's flame is scaled across: a little inside its mouth.
pub fn flame_scale(nozzle: &Nozzle, length: f32) -> Vec3 {
    Vec3::new(length, nozzle.half.y * 0.9, nozzle.half.x * 0.9)
}

/// How long a car's flames are, in meters: a short blue tongue at rest, longer with the speed, and
/// roaring out on a boost. `flicker` is from -1 to 1, a different one every frame.
pub fn flame_length(speed_share: f32, boosting: bool, flicker: f32) -> f32 {
    let steady = 0.3 + 0.6 * speed_share.clamp(0.0, 1.3);
    let roar = if boosting { 0.85 } else { 0.0 };
    (steady + roar) * (1.0 + 0.08 * flicker.clamp(-1.0, 1.0))
}


#[cfg(test)]
mod tests {
    use super::*;

    fn bounds(geometry: &Geometry) -> (Vec3, Vec3) {
        geometry.positions().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(min, max), point| (min.min(point), max.max(point)),
        )
    }

    fn livery(model: CarModel) -> Livery {
        Livery {
            model,
            body: PAINTS[0].1,
            accent: NEONS[1].1,
        }
    }

    /// Every pod is about as big as what the simulation collides, it hovers clear of the road, and
    /// its nose points forward.
    #[test]
    fn every_pod_is_the_size_of_its_collisions_and_hovers() {
        for model in CarModel::ALL {
            let hull = hull_geometry(&livery(model));
            let (min, max) = bounds(&hull);
            assert!(max.x > 2.1 && max.x < 2.75, "{model:?} {max}");
            assert!(min.x > -2.3, "{model:?} {min}");
            assert!(max.z < 1.65 && min.z > -1.65, "{model:?} {min} {max}");
            assert!(min.y > 0.2, "{model:?}: the underside clears the road: {min}");
            assert!(max.y < 1.2, "{model:?} {max}");
            // The tip of the nose is on the middle of the car, or of each hull for the Twin.
            let tip = hull.positions().max_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
            let middle = if model == CarModel::Twin { TWIN_OFFSET } else { 0.0 };
            assert!((tip.z.abs() - middle).abs() < 0.1, "{model:?} {tip}");
        }
    }

    /// Every pod's thrusters are at the back, the Twin's middle one at the end of its nacelle, and
    /// the flames hang behind them.
    #[test]
    fn the_thrusters_are_at_the_tail() {
        for model in CarModel::ALL {
            let (min, _) = bounds(&hull_geometry(&livery(model)));
            for nozzle in nozzles(model) {
                assert!(nozzle.at.x - min.x < 0.8, "{model:?} {nozzle:?} {min}");
                assert!(flame_origin(nozzle).x < nozzle.at.x);
            }
        }
    }

    #[test]
    fn every_pod_stays_light_to_draw() {
        for model in CarModel::ALL {
            let livery = livery(model);
            let triangles = hull_geometry(&livery).triangles()
                + lights_geometry(&livery).triangles()
                + under_glow_geometry(&livery).triangles()
                + shadow_geometry().triangles()
                + flame_geometry().triangles() * nozzles(model).len();
            // A few thousand for a full grid of eight, against the 155,000 of the skyway and its
            // city.
            assert!(triangles < 1000, "{model:?} {triangles}");
        }
    }

    /// The neon and the thrusters are brighter than white, so they bloom; the hull is not, whatever
    /// its paint.
    #[test]
    fn the_lights_glow_and_the_hull_does_not() {
        let brightest = |geometry: Geometry| {
            geometry
                .colors()
                .map(|[r, g, b, _]| r.max(g).max(b))
                .fold(0.0f32, f32::max)
        };
        for model in CarModel::ALL {
            for paint in 0..PAINTS.len() {
                let livery = Livery::of(CarLook {
                    model: model as u8,
                    paint: paint as u8,
                    neon: 0,
                });
                assert!(brightest(hull_geometry(&livery)) <= 1.0);
            }
            for neon in 0..NEONS.len() {
                let livery = Livery::of(CarLook {
                    model: model as u8,
                    paint: 0,
                    neon: neon as u8,
                });
                assert!(brightest(lights_geometry(&livery)) > 2.0, "{model:?} {neon}");
            }
        }
    }

    /// The lists match what the wire allows, and every look on it draws.
    #[test]
    fn every_look_on_the_wire_draws() {
        use star_racer_protocol::{CAR_MODELS, CAR_NEONS, CAR_PAINTS};
        assert_eq!(CarModel::ALL.len(), usize::from(CAR_MODELS));
        assert_eq!(PAINTS.len(), usize::from(CAR_PAINTS));
        assert_eq!(NEONS.len(), usize::from(CAR_NEONS));
        for (index, model) in CarModel::ALL.into_iter().enumerate() {
            assert_eq!(CarModel::of(index as u8), model);
        }
        let wild = Livery::of(CarLook {
            model: 200,
            paint: 200,
            neon: 200,
        });
        assert!(CarModel::ALL.contains(&wild.model));
    }

    /// The light and the shadow under the car are ovals around it, fading to nothing at their rim,
    /// where a banked road could curve up through them; the light is brightest at the hull's edge,
    /// not in its middle, and is never a lit sheet that blooms.
    #[test]
    fn the_light_and_the_shadow_under_the_car_fade_out_at_their_rims() {
        let reach = |geometry: &Geometry| {
            geometry
                .positions()
                .map(|point| (point.x / FOOTPRINT.x).hypot(point.z / FOOTPRINT.y))
                .fold(0.0f32, f32::max)
        };
        let glow = under_glow_geometry(&livery(CarModel::Arrow));
        let shadow = shadow_geometry();
        // Round: every point of the rim is as far out, in the oval's own measure.
        for geometry in [&glow, &shadow] {
            let rim = reach(geometry);
            let colors: Vec<_> = geometry.colors().collect();
            for (point, color) in geometry.positions().zip(colors) {
                let out = (point.x / FOOTPRINT.x).hypot(point.z / FOOTPRINT.y);
                if (out - rim).abs() < 1e-3 {
                    assert!(color.iter().all(|&channel| channel == 0.0 || channel == 1.0));
                    assert!(color[3] == 0.0 || color[..3] == [0.0; 3], "{color:?}");
                }
            }
        }
        assert!(reach(&glow) < 1.7 && reach(&shadow) < 1.2);
        // Brightest at the hull's edge, dim in the middle, and nowhere near blooming.
        let brightness_at = |size: f32| {
            glow.positions()
                .zip(glow.colors())
                .filter(|(point, _)| {
                    ((point.x / FOOTPRINT.x).hypot(point.z / FOOTPRINT.y) - size).abs() < 1e-3
                })
                .map(|(_, [r, g, b, _])| r.max(g).max(b))
                .fold(0.0f32, f32::max)
        };
        assert_eq!(brightness_at(0.7), 0.0);
        assert!(brightness_at(1.0) < 0.5);
    }

    #[test]
    fn the_flames_grow_with_the_speed_and_roar_on_a_boost() {
        let resting = flame_length(0.0, false, 0.0);
        let cruising = flame_length(1.0, false, 0.0);
        let boosting = flame_length(1.0, true, 0.0);
        assert!(resting > 0.2 && resting < cruising && cruising < boosting);
        assert!(boosting > 1.8 * cruising);
        assert!((flame_length(1.0, false, 1.0) / cruising - 1.08).abs() < 1e-4);
    }
}
