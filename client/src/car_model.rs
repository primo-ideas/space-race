//! The cars, as drawn: low, wide racing hovercraft in the spirit of F-Zero's machines, built from
//! flat-shaded triangles like everything else in the game.
//!
//! A car is a long wedge of a hull with a pointed nose, a cockpit bubble on its back, and two
//! engine pods slung beside it on a rear wing, each pod wearing a fin. It floats above the road on
//! nothing: its underside clears it by a quarter of a meter, a soft shadow sits it over the
//! road, and colored light spilling out from under its edge says it hovers. Its thrusters glow
//! at the back, and their flames lengthen with its speed and roar out on a boost.
//!
//! Every car shares the same shape; its livery says whose it is: the player's own in orange, lined
//! with orange neon, every other in blue lined with magenta. The neon and the thrusters are drawn
//! brighter than white, so the bloom gives them a halo.
//!
//! The model is written along `+x`, the direction of a zero heading, `y` up and `z` to the right,
//! its origin on the road under the middle of the car, where the simulation places it. It is about
//! 4.5 m long and 2.8 m wide, a little more than the 2.4 m circle the simulation collides.

use bevy::color::LinearRgba;
use bevy::prelude::*;

use crate::geometry::{Frame, Geometry};

/// How a car is painted.
#[derive(Debug, Clone, Copy)]
pub struct Livery {
    /// The hull's top and the pods' tops.
    pub body: Color,
    /// The neon lines along the hull and the light under it.
    pub accent: LinearRgba,
}

impl Livery {
    /// The player's own car.
    pub const OWN: Self = Self {
        body: Color::srgb(0.95, 0.36, 0.08),
        accent: LinearRgba::new(3.2, 0.95, 0.15, 1.0),
    };
    /// Everyone else's.
    pub const OTHER: Self = Self {
        body: Color::srgb(0.14, 0.36, 0.86),
        accent: LinearRgba::new(2.4, 0.35, 2.6, 1.0),
    };
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

/// The hull, from its tail to the tip of its nose: widest just ahead of the tail, where the wing
/// joins it, and drawn out to a point.
const HULL: [Section; 6] = [
    section(-1.95, 0.60, 0.34, 0.74),
    section(-1.55, 0.74, 0.28, 0.86),
    section(-0.40, 0.78, 0.26, 0.86),
    section(0.70, 0.64, 0.27, 0.72),
    section(1.60, 0.40, 0.29, 0.56),
    section(2.40, 0.07, 0.33, 0.39),
];

/// An engine pod, either side of the hull, `POD_OFFSET` meters out.
const POD: [Section; 4] = [
    section(-2.15, 0.26, 0.30, 0.66),
    section(-1.80, 0.30, 0.26, 0.72),
    section(-0.20, 0.27, 0.27, 0.66),
    section(0.80, 0.08, 0.37, 0.47),
];
const POD_OFFSET: f32 = 1.2;

/// The cockpit, a bubble on the hull's back.
const COCKPIT: [Section; 4] = [
    section(-0.85, 0.20, 0.82, 0.87),
    section(-0.40, 0.34, 0.84, 1.13),
    section(0.35, 0.32, 0.79, 1.06),
    section(0.95, 0.09, 0.70, 0.76),
];

/// Where the thrusters' flames start, at the back of the hull and of the pods.
pub const HULL_NOZZLE_X: f32 = -1.97;
pub const POD_NOZZLE_X: f32 = -2.17;
/// The two nozzles of the hull, either side of its middle, and their half size.
const HULL_NOZZLE_Z: f32 = 0.34;
const HULL_NOZZLE_HALF: Vec2 = Vec2::new(0.15, 0.09);
const HULL_NOZZLE_Y: f32 = 0.55;
const POD_NOZZLE_HALF: Vec2 = Vec2::new(0.15, 0.13);
const POD_NOZZLE_Y: f32 = 0.48;

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

/// Everything of a car that takes the light: hull, cockpit, pods, wings and fins.
pub fn hull_geometry(livery: &Livery) -> Geometry {
    let mut geometry = Geometry::default();
    let frame = Frame::new(Vec3::ZERO, Vec3::X, Vec3::Z);

    loft(
        &mut geometry,
        &HULL,
        0.0,
        face_colors(livery.body, GRAPHITE, UNDERSIDE),
        GRAPHITE,
        livery.body,
    );
    loft(
        &mut geometry,
        &COCKPIT,
        0.0,
        face_colors(GLASS_SHEEN, GLASS, GLASS),
        GLASS,
        GLASS,
    );
    for side in [-1.0, 1.0] {
        loft(
            &mut geometry,
            &POD,
            side * POD_OFFSET,
            face_colors(livery.body, GRAPHITE, UNDERSIDE),
            GRAPHITE,
            GRAPHITE,
        );
        // A fin on each pod, raked back and narrowing as it rises.
        geometry.tapered_block(
            &frame,
            Vec3::new(-2.05, 0.66, side * POD_OFFSET - 0.035),
            Vec3::new(-1.35, 1.08, side * POD_OFFSET + 0.035),
            0.35,
            GRAPHITE,
        );
    }

    // The rear wing the pods hang from, through the hull, and a splitter under the nose.
    geometry.block(
        &frame,
        Vec3::new(-1.75, 0.44, -POD_OFFSET),
        Vec3::new(-0.55, 0.52, POD_OFFSET),
        GRAPHITE,
    );
    geometry.block(
        &frame,
        Vec3::new(1.05, 0.25, -0.95),
        Vec3::new(1.85, 0.29, 0.95),
        GRAPHITE,
    );

    // A white stripe down the middle of the hull, just over its top.
    let top_at = |x: f32| {
        let pair = HULL
            .windows(2)
            .find(|pair| x >= pair[0].x && x <= pair[1].x)
            .expect("the stripe stays on the hull");
        let share = (x - pair[0].x) / (pair[1].x - pair[0].x);
        pair[0].top + (pair[1].top - pair[0].top) * share + 0.006
    };
    let stripe_runs = [(-1.9, -0.9), (0.95, 2.1)];
    for (from, to) in stripe_runs {
        let steps = 4;
        for step in 0..steps {
            let a = from + (to - from) * step as f32 / steps as f32;
            let b = from + (to - from) * (step + 1) as f32 / steps as f32;
            geometry.quad_facing_up(
                [
                    Vec3::new(a, top_at(a), -0.09),
                    Vec3::new(b, top_at(b), -0.09),
                    Vec3::new(b, top_at(b), 0.09),
                    Vec3::new(a, top_at(a), 0.09),
                ],
                STRIPE,
            );
        }
    }
    geometry
}

/// The parts that shine by themselves: the thrusters' insides and the neon lines along the hull,
/// drawn unlit and brighter than white.
pub fn lights_geometry(livery: &Livery) -> Geometry {
    let mut geometry = Geometry::default();
    let frame = Frame::new(Vec3::ZERO, Vec3::X, Vec3::Z);
    let thruster = Color::LinearRgba(THRUSTER);
    let accent = Color::LinearRgba(livery.accent);

    for side in [-1.0, 1.0] {
        // The hull's two nozzles and the pod's one, just behind their tails.
        let hull_z = side * HULL_NOZZLE_Z;
        geometry.block(
            &frame,
            Vec3::new(
                HULL_NOZZLE_X - 0.02,
                HULL_NOZZLE_Y - HULL_NOZZLE_HALF.y,
                hull_z - HULL_NOZZLE_HALF.x,
            ),
            Vec3::new(
                HULL_NOZZLE_X + 0.03,
                HULL_NOZZLE_Y + HULL_NOZZLE_HALF.y,
                hull_z + HULL_NOZZLE_HALF.x,
            ),
            thruster,
        );
        let pod_z = side * POD_OFFSET;
        geometry.block(
            &frame,
            Vec3::new(
                POD_NOZZLE_X - 0.02,
                POD_NOZZLE_Y - POD_NOZZLE_HALF.y,
                pod_z - POD_NOZZLE_HALF.x,
            ),
            Vec3::new(
                POD_NOZZLE_X + 0.03,
                POD_NOZZLE_Y + POD_NOZZLE_HALF.y,
                pod_z + POD_NOZZLE_HALF.x,
            ),
            thruster,
        );

        // A neon line along the widest point of the hull, from the tail toward the nose.
        let line: Vec<Vec3> = HULL[..5]
            .iter()
            .map(|section| {
                let widest = ring(section)[2];
                Vec3::new(section.x, widest.y, side * (widest.x + 0.01))
            })
            .collect();
        for pair in line.windows(2) {
            geometry.bar(&frame, pair[0], pair[1], 0.045, accent);
        }
        // And one along the top of each pod.
        let pod_line: Vec<Vec3> = POD[..3]
            .iter()
            .map(|section| Vec3::new(section.x, section.top + 0.01, side * POD_OFFSET))
            .collect();
        for pair in pod_line.windows(2) {
            geometry.bar(&frame, pair[0], pair[1], 0.04, accent);
        }
    }
    geometry
}

/// The hull's outline on the road, as an oval: half its length and half its width, pods included.
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

/// Two thrusters' flames, either side of the middle `z` meters out, each one meter long and
/// pointing back from the origin: the entity they hang from stands at the nozzles and stretches
/// them along `x` to the length the speed calls for. Their color fades from the nozzle to nothing
/// at the tip, and they are added to what is behind them.
fn flames_geometry(z: f32, half: Vec2) -> Geometry {
    let mut geometry = Geometry::default();
    let base = Color::LinearRgba(FLAME);
    let tip_color = Color::LinearRgba(LinearRgba::BLACK);
    for side in [-1.0, 1.0] {
        let middle = Vec3::new(0.0, 0.0, side * z);
        let tip = middle - Vec3::X;
        let corners = [
            middle + Vec3::new(0.0, -half.y, -half.x),
            middle + Vec3::new(0.0, -half.y, half.x),
            middle + Vec3::new(0.0, half.y, half.x),
            middle + Vec3::new(0.0, half.y, -half.x),
        ];
        let inside = middle - Vec3::X * 0.3;
        for face in 0..4 {
            let a = corners[face];
            let b = corners[(face + 1) % 4];
            geometry.quad_facing_away_shaded(
                [a, b, tip, tip],
                inside,
                [base, base, tip_color, tip_color],
            );
        }
    }
    geometry
}

/// The flames of the hull's two thrusters.
pub fn hull_flames_geometry() -> Geometry {
    flames_geometry(HULL_NOZZLE_Z, HULL_NOZZLE_HALF * 0.9)
}

/// The flames of the pods' thrusters.
pub fn pod_flames_geometry() -> Geometry {
    flames_geometry(POD_OFFSET, POD_NOZZLE_HALF * 0.9)
}

/// Where the hull's flames hang from, and the pods'.
pub fn hull_flames_origin() -> Vec3 {
    Vec3::new(HULL_NOZZLE_X - 0.02, HULL_NOZZLE_Y, 0.0)
}

pub fn pod_flames_origin() -> Vec3 {
    Vec3::new(POD_NOZZLE_X - 0.02, POD_NOZZLE_Y, 0.0)
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

    /// The car is about as big as what the simulation collides, it hovers clear of the road, and
    /// its nose points forward.
    #[test]
    fn the_car_is_the_size_of_its_collisions_and_hovers() {
        let hull = hull_geometry(&Livery::OWN);
        let (min, max) = bounds(&hull);
        assert!(max.x > 2.3 && max.x < 2.5, "{max}");
        assert!(min.x > -2.3, "{min}");
        assert!(max.z < 1.6 && min.z > -1.6, "{min} {max}");
        assert!(min.y > 0.2, "the underside clears the road: {min}");
        assert!(max.y < 1.2, "{max}");
        // The tip of the nose is on the middle of the car.
        let tip = hull.positions().max_by(|a, b| a.x.total_cmp(&b.x)).unwrap();
        assert!(tip.z.abs() < 0.1, "{tip}");
    }

    #[test]
    fn the_car_stays_light_to_draw() {
        let triangles = hull_geometry(&Livery::OWN).triangles()
            + lights_geometry(&Livery::OWN).triangles()
            + under_glow_geometry(&Livery::OWN).triangles()
            + shadow_geometry().triangles()
            + hull_flames_geometry().triangles()
            + pod_flames_geometry().triangles();
        // A few thousand for a full grid of eight, against the 155,000 of the skyway and its city.
        assert!(triangles < 1000, "{triangles}");
    }

    /// The neon and the thrusters are brighter than white, so they bloom; the hull is not.
    #[test]
    fn the_lights_glow_and_the_hull_does_not() {
        let brightest = |geometry: Geometry| {
            geometry
                .colors()
                .map(|[r, g, b, _]| r.max(g).max(b))
                .fold(0.0f32, f32::max)
        };
        assert!(brightest(lights_geometry(&Livery::OTHER)) > 2.0);
        assert!(brightest(hull_geometry(&Livery::OTHER)) <= 1.0);
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
        let glow = under_glow_geometry(&Livery::OWN);
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
