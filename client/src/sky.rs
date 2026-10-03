//! The sky: a starry night all around the circuit, with planets and moons in color, clouds of light,
//! and a few shapes of neon turning slowly far out in space.
//!
//! Everything in it stands on a sphere [`RADIUS`] meters round that follows the camera without
//! turning with it, so the sky never comes any closer however far the car drives: it is as far as a
//! sky should be. Nothing is a texture: stars, planets and clouds are triangles colored at their
//! corners, rolled from a fixed seed, so every player sees the same sky. It is unlit, casts no
//! shadow, and ignores the fog the menus fade their grid into, so it shines behind them too. See
//! `docs/sky.md`.

use std::f32::consts::TAU;

use bevy::asset::RenderAssetUsages;
use bevy::light::NotShadowCaster;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;
use bevy::transform::TransformSystems;

use crate::camera::MainCamera;
use crate::dice::Dice;
use crate::geometry::{Frame, Geometry};

/// How far the sky stands from the camera, in meters: beyond everything on a circuit, and short of
/// the 1 km past which the camera sees nothing.
const RADIUS: f32 = 880.0;
/// The seed the whole sky is rolled from. Another number, another sky.
const SEED: u64 = 0x5_7A2_5EED;
/// How far the planets stand, in front of the stars: far enough for the rings of the giant to stay
/// short of the 1 km on their far side.
const PLANETS: f32 = 760.0;

/// How many stars there are, and how few of them are kept below the horizon, where the city and
/// the dark under the road hide most of them anyway.
const STARS: usize = 3400;
const STARS_KEPT_BELOW: f32 = 0.3;
/// How many more crowd along the band of the galaxy, and how far either side of it they stray, as
/// a share of the sky's radius.
const BAND_STARS: usize = 1600;
const BAND_SPREAD: f32 = 0.07;
/// Half the size of the faintest and of the brightest star, in meters, which comes to one pixel or
/// so and about four at the sky's distance.
const STAR_SIZE: (f32, f32) = (1.1, 4.2);
/// Stars brighter than this, out of 1, wear a cross of light.
const FLARE: f32 = 0.95;

/// Pieces round a planet's disc, and rings of them from its middle to its edge: enough for the
/// light to turn smoothly across it.
const DISC_SEGMENTS: usize = 64;
const DISC_RINGS: usize = 8;

/// The colors stars come in, and how often, out of the sum of the weights: mostly a cool white, a
/// few warm, cyan, pink or violet.
const STAR_COLORS: [(Color, u32); 5] = [
    (Color::srgb(0.85, 0.9, 1.0), 60),
    (Color::srgb(1.0, 0.88, 0.72), 15),
    (Color::srgb(0.6, 0.88, 1.0), 10),
    (Color::srgb(1.0, 0.7, 0.86), 8),
    (Color::srgb(0.82, 0.72, 1.0), 7),
];

pub struct SkyPlugin;

impl Plugin for SkyPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_sky).add_systems(
            PostUpdate,
            (follow_camera, spin_shapes).before(TransformSystems::Propagate),
        );
    }
}

/// The sky's center, which goes wherever the camera goes.
#[derive(Component)]
struct Sky;

/// A shape of the sky turning on itself: about `axis`, at `speed` radians a second.
#[derive(Component)]
struct Spin {
    axis: Vec3,
    speed: f32,
}

/// A direction of the sky: `azimuth` degrees round from the way the menus look (`-z`, toward `+x`),
/// and `elevation` degrees over the horizon.
fn toward(azimuth: f32, elevation: f32) -> Vec3 {
    let (azimuth, elevation) = (azimuth.to_radians(), elevation.to_radians());
    Vec3::new(
        azimuth.sin() * elevation.cos(),
        elevation.sin(),
        -azimuth.cos() * elevation.cos(),
    )
}

fn spawn_sky(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let mut solid = Canvas::default();
    let mut glow = Canvas::default();
    let mut dice = Dice::new(SEED);
    stars(&mut solid, &mut glow, &mut dice);
    galaxy(&mut glow, &mut dice);
    nebulae(&mut glow, &mut dice);
    planets(&mut solid, &mut glow);

    // Unlit and outside the fog, the way neon is: the sky shows the colors it is given.
    let solid_material = materials.add(StandardMaterial {
        unlit: true,
        fog_enabled: false,
        ..default()
    });
    let glow_material = materials.add(StandardMaterial {
        unlit: true,
        fog_enabled: false,
        alpha_mode: AlphaMode::Add,
        ..default()
    });

    commands
        .spawn((
            Sky,
            Mesh3d(meshes.add(solid.into_mesh())),
            MeshMaterial3d(solid_material.clone()),
            NotShadowCaster,
            Transform::default(),
            Visibility::default(),
        ))
        .with_children(|sky| {
            sky.spawn((
                Mesh3d(meshes.add(glow.into_mesh())),
                MeshMaterial3d(glow_material),
                NotShadowCaster,
            ));
            for shape in shapes() {
                sky.spawn((
                    Mesh3d(meshes.add(shape.mesh.into_mesh())),
                    MeshMaterial3d(solid_material.clone()),
                    NotShadowCaster,
                    Transform::from_translation(shape.center).with_rotation(shape.tilt),
                    shape.spin,
                ));
            }
        });
}

/// The sky goes wherever the camera goes, without turning: what is drawn on it stays put against
/// the circuit, as far away as the sky is.
fn follow_camera(
    cameras: Query<&Transform, (With<MainCamera>, Without<Sky>)>,
    mut skies: Query<&mut Transform, With<Sky>>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    for mut sky in &mut skies {
        sky.translation = camera.translation;
    }
}

fn spin_shapes(time: Res<Time>, mut shapes: Query<(&Spin, &mut Transform)>) {
    for (spin, mut transform) in &mut shapes {
        transform.rotate_local_axis(
            Dir3::new(spin.axis).unwrap_or(Dir3::Y),
            spin.speed * time.delta_secs(),
        );
    }
}

/// Stars all round, mostly over the horizon, a few bright ones wearing a cross of light.
fn stars(solid: &mut Canvas, glow: &mut Canvas, dice: &mut Dice) {
    let mut placed = 0;
    while placed < STARS {
        let direction = anywhere(dice);
        if direction.y < -0.15 && dice.range(0.0, 1.0) > STARS_KEPT_BELOW {
            continue;
        }
        star(solid, glow, dice, direction);
        placed += 1;
    }
}

/// A star `direction` away: a diamond facing the camera, its size and color rolled.
fn star(solid: &mut Canvas, glow: &mut Canvas, dice: &mut Dice, direction: Vec3) {
    // Most stars are faint, a few are bright.
    let brightness = 0.22 + 0.78 * dice.range(0.0, 1.0).powi(3);
    let color = star_color(dice);
    let size = STAR_SIZE.0 + (STAR_SIZE.1 - STAR_SIZE.0) * brightness;
    let center = direction * RADIUS;
    let (right, up) = facing(direction);
    let lit = scale(color, brightness);
    solid.quad(
        [
            center + right * size,
            center + up * size,
            center - right * size,
            center - up * size,
        ],
        [lit; 4],
    );
    if brightness > FLARE {
        let reach = size * 6.0;
        let width = size * 0.18;
        for (along, across) in [(right, up), (up, right)] {
            let core = color.with_alpha(0.6);
            let tip = color.with_alpha(0.0);
            for end in [1.0, -1.0] {
                glow.quad(
                    [
                        center + across * width,
                        center + across * width + along * reach * end,
                        center - across * width + along * reach * end,
                        center - across * width,
                    ],
                    [core, tip, tip, core],
                );
            }
        }
    }
}

fn star_color(dice: &mut Dice) -> Color {
    let total: u32 = STAR_COLORS.iter().map(|(_, weight)| weight).sum();
    let mut roll = dice.below(total as usize) as u32;
    for (color, weight) in STAR_COLORS {
        if roll < weight {
            return color;
        }
        roll -= weight;
    }
    STAR_COLORS[0].0
}

/// The galaxy: a band of faint light across the sky, crowded with stars.
fn galaxy(glow: &mut Canvas, dice: &mut Dice) {
    // The band runs round the great circle square to this axis, tilted across the sky.
    let axis = Vec3::new(0.35, 0.55, 0.76).normalize();
    let first = axis.any_orthonormal_vector();
    let second = axis.cross(first);
    let along = |angle: f32| first * angle.cos() + second * angle.sin();

    // A soft river of light along it, brighter and wider in places.
    let clouds = 90;
    for cloud in 0..clouds {
        let angle = TAU * (cloud as f32 + dice.range(-0.3, 0.3)) / clouds as f32;
        let swell = 0.5 + 0.5 * (angle * 3.0).sin().abs();
        let direction = (along(angle) + axis * dice.range(-0.04, 0.04)).normalize();
        let color = if dice.range(0.0, 1.0) < 0.3 {
            Color::srgb(0.75, 0.55, 1.0)
        } else {
            Color::srgb(0.55, 0.7, 1.0)
        };
        blob(
            glow,
            direction,
            dice.range(60.0, 110.0) * (0.6 + 0.4 * swell),
            color.with_alpha(dice.range(0.04, 0.08) * swell),
        );
    }

    // And stars crowding along it.
    let mut solid_stars = Canvas::default();
    for _ in 0..BAND_STARS {
        let angle = dice.range(0.0, TAU);
        let stray = (dice.range(-1.0, 1.0) + dice.range(-1.0, 1.0)) * BAND_SPREAD;
        let direction = (along(angle) + axis * stray).normalize();
        // Small and faint: the crowd is the band, not each of them.
        let center = direction * RADIUS;
        let (right, up) = facing(direction);
        let size = STAR_SIZE.0 * dice.range(0.8, 1.2);
        let lit = scale(star_color(dice), dice.range(0.25, 0.6));
        solid_stars.quad(
            [
                center + right * size,
                center + up * size,
                center - right * size,
                center - up * size,
            ],
            [lit; 4],
        );
    }
    // Added to the glow, so they brighten the band rather than block it.
    glow.extend(solid_stars);
}

/// Clouds of colored light: a few nebulae, each a heap of soft blobs of two colors.
fn nebulae(glow: &mut Canvas, dice: &mut Dice) {
    for (direction, size, colors) in [
        // Magenta and violet, in front of the menus.
        (
            toward(18.0, 24.0),
            150.0,
            [Color::srgb(0.95, 0.2, 0.75), Color::srgb(0.5, 0.25, 1.0)],
        ),
        // Teal and blue, high over the south-east.
        (
            toward(125.0, 38.0),
            190.0,
            [Color::srgb(0.1, 0.85, 0.8), Color::srgb(0.15, 0.4, 1.0)],
        ),
        // Orange and rose, low in the west.
        (
            toward(-125.0, 12.0),
            130.0,
            [Color::srgb(1.0, 0.45, 0.15), Color::srgb(1.0, 0.25, 0.5)],
        ),
        // A faint violet veil overhead.
        (
            toward(-40.0, 62.0),
            220.0,
            [Color::srgb(0.55, 0.3, 1.0), Color::srgb(0.3, 0.5, 1.0)],
        ),
    ] {
        let (right, up) = facing(direction);
        for blob_index in 0..22 {
            let spread = Vec2::new(dice.range(-1.0, 1.0), dice.range(-0.6, 0.6)) * size;
            let where_ = (direction * RADIUS + right * spread.x + up * spread.y).normalize();
            let color = colors[blob_index % 2];
            blob(
                glow,
                where_,
                dice.range(0.35, 0.8) * size,
                color.with_alpha(dice.range(0.05, 0.12)),
            );
        }
    }
}

/// A soft disc of light `radius` meters across, `color` at its middle fading to nothing at its edge.
fn blob(glow: &mut Canvas, direction: Vec3, radius: f32, color: Color) {
    let center = direction * RADIUS;
    let (right, up) = facing(direction);
    let middle = color.with_alpha(color.alpha() * 0.45);
    let edge = color.with_alpha(0.0);
    let segments = 24;
    let point = |segment: usize, share: f32| {
        let angle = TAU * segment as f32 / segments as f32;
        center + (right * angle.cos() + up * angle.sin()) * radius * share
    };
    for segment in 0..segments {
        let next = segment + 1;
        glow.triangle(
            [center, point(segment, 0.5), point(next, 0.5)],
            [color, middle, middle],
        );
        glow.quad(
            [
                point(segment, 0.5),
                point(segment, 1.0),
                point(next, 1.0),
                point(next, 0.5),
            ],
            [middle, edge, edge, middle],
        );
    }
}

/// The planets and moons: lit from one side by a sun nobody sees, a halo round each, rings round
/// the giant.
fn planets(solid: &mut Canvas, glow: &mut Canvas) {
    // The giant, magenta and banded, over the launch.
    let giant = toward(75.0, 19.0) * PLANETS;
    let giant_radius = 78.0;
    let light = Vec3::new(-0.55, 0.5, 0.67).normalize();
    planet(solid, giant, giant_radius, |point| {
        let band = (point.y * 11.0 + (point.x * 3.0).sin() * 0.6).sin() * 0.5 + 0.5;
        let surface = Color::srgb(0.85, 0.22, 0.7).mix(&Color::srgb(0.45, 0.18, 0.85), band);
        shade(surface, point, light)
    });
    halo(glow, giant, giant_radius, Color::srgb(1.0, 0.35, 0.85));
    // Its rings, nearly edge on: the half behind the giant is hidden by it.
    for (inner, outer, color) in [
        (1.35, 1.6, Color::srgb(0.3, 0.85, 1.0)),
        (1.65, 1.72, Color::srgb(0.15, 0.4, 0.55)),
        (1.76, 2.15, Color::srgb(0.75, 0.55, 1.0)),
    ] {
        ring(
            solid,
            giant,
            (inner * giant_radius, outer * giant_radius),
            73.0_f32.to_radians(),
            -0.35,
            color,
        );
    }

    // A great pale moon, a crescent lit from behind, over the west.
    let moon = toward(-100.0, 15.0) * PLANETS;
    let moon_light = Vec3::new(0.85, 0.35, -0.4).normalize();
    planet(solid, moon, 52.0, |point| {
        let craters = ((point.x * 7.0).sin() * (point.y * 6.0).cos()).abs() * 0.12;
        let surface = Color::srgb(0.78 - craters, 0.88 - craters, 1.0 - craters);
        shade(surface, point, moon_light)
    });
    halo(glow, moon, 52.0, Color::srgb(0.6, 0.85, 1.0));

    // A small orange world to the south, and a teal moon in front of the menus.
    let ember = toward(175.0, 14.0) * PLANETS;
    let ember_light = Vec3::new(-0.6, 0.4, 0.7).normalize();
    planet(solid, ember, 24.0, |point| {
        let band = (point.y * 5.0).sin() * 0.5 + 0.5;
        let surface = Color::srgb(1.0, 0.5, 0.15).mix(&Color::srgb(0.85, 0.28, 0.12), band);
        shade(surface, point, ember_light)
    });
    halo(glow, ember, 24.0, Color::srgb(1.0, 0.5, 0.2));

    let teal = toward(-28.0, 22.0) * PLANETS;
    let teal_light = Vec3::new(0.6, 0.55, 0.58).normalize();
    planet(solid, teal, 15.0, |point| {
        shade(Color::srgb(0.2, 0.9, 0.85), point, teal_light)
    });
    halo(glow, teal, 15.0, Color::srgb(0.2, 1.0, 0.9));
}

/// A planet's disc `radius` meters across at `center`, facing the camera, colored by `paint` at
/// each point of it, given from `(-1, -1)` to `(1, 1)` across the disc.
fn planet(canvas: &mut Canvas, center: Vec3, radius: f32, paint: impl Fn(Vec2) -> Color) {
    let (right, up) = facing(center.normalize());
    let point = |segment: usize, ring: usize| {
        let angle = TAU * segment as f32 / DISC_SEGMENTS as f32;
        Vec2::new(angle.cos(), angle.sin()) * ring as f32 / DISC_RINGS as f32
    };
    let place = |point: Vec2| center + (right * point.x + up * point.y) * radius;
    for segment in 0..DISC_SEGMENTS {
        let next = segment + 1;
        let middle = Vec2::ZERO;
        let (a, b) = (point(segment, 1), point(next, 1));
        canvas.triangle(
            [place(middle), place(a), place(b)],
            [paint(middle), paint(a), paint(b)],
        );
        for ring in 1..DISC_RINGS {
            let corners = [
                point(segment, ring),
                point(segment, ring + 1),
                point(next, ring + 1),
                point(next, ring),
            ];
            canvas.quad(corners.map(place), corners.map(&paint));
        }
    }
}

/// The color of a sphere's surface at `point` of its disc, lit from `light`, which points from the
/// planet toward its sun, `z` toward the camera: the far side of a planet lit from behind stays a
/// dark rim, and its lit edge a crescent.
fn shade(surface: Color, point: Vec2, light: Vec3) -> Color {
    let depth = (1.0 - point.length_squared()).max(0.0).sqrt();
    let normal = Vec3::new(point.x, point.y, depth);
    let lit = normal.dot(light).max(0.0);
    // Never quite black on the night side, so the planet stands out against the sky behind it.
    scale(surface, 0.08 + 0.92 * lit)
}

/// A glow round a planet, from its edge out to a third of its radius further.
fn halo(glow: &mut Canvas, center: Vec3, radius: f32, color: Color) {
    let (right, up) = facing(center.normalize());
    let segments = DISC_SEGMENTS;
    let inner = color.with_alpha(0.35);
    let outer = color.with_alpha(0.0);
    let point = |segment: usize, reach: f32| {
        let angle = TAU * segment as f32 / segments as f32;
        center + (right * angle.cos() + up * angle.sin()) * radius * reach
    };
    for segment in 0..segments {
        glow.quad(
            [
                point(segment, 0.98),
                point(segment, 1.35),
                point(segment + 1, 1.35),
                point(segment + 1, 0.98),
            ],
            [inner, outer, outer, inner],
        );
    }
}

/// A flat ring round a planet at `center`, between `reach.0` and `reach.1` meters from it, tilted
/// `tilt` radians from facing the camera and turned `roll` radians about the line of sight.
fn ring(canvas: &mut Canvas, center: Vec3, reach: (f32, f32), tilt: f32, roll: f32, color: Color) {
    let forward = center.normalize();
    let (right, up) = facing(forward);
    let (right, up) = (
        right * roll.cos() + up * roll.sin(),
        up * roll.cos() - right * roll.sin(),
    );
    // The ring's plane holds `right`, and `up` leaned toward the far side by the tilt.
    let across = up * tilt.cos() + forward * tilt.sin();
    let segments = 96;
    let point = |segment: usize, distance: f32| {
        let angle = TAU * segment as f32 / segments as f32;
        center + (right * angle.cos() + across * angle.sin()) * distance
    };
    for segment in 0..segments {
        // Brighter on the near side, where it crosses in front of the planet.
        let near = 0.75 + 0.25 * (TAU * segment as f32 / segments as f32).sin().abs();
        let shade = scale(color, near);
        canvas.quad(
            [
                point(segment, reach.0),
                point(segment, reach.1),
                point(segment + 1, reach.1),
                point(segment + 1, reach.0),
            ],
            [shade; 4],
        );
    }
}

/// A shape of the sky: its mesh, where it stands, how it is tilted, and how it turns.
struct Shape {
    mesh: Geometry,
    center: Vec3,
    tilt: Quat,
    spin: Spin,
}

/// The shapes: neon frames hanging far out in space, turning slowly on themselves.
fn shapes() -> Vec<Shape> {
    let frame = Frame::new(Vec3::ZERO, Vec3::X, Vec3::Z);
    let bar = 1.8;

    // An octahedron of magenta light, over the north-east.
    let mut octahedron = Geometry::default();
    let tips = [
        Vec3::X,
        Vec3::NEG_X,
        Vec3::Y,
        Vec3::NEG_Y,
        Vec3::Z,
        Vec3::NEG_Z,
    ]
    .map(|tip| tip * 34.0);
    for (index, a) in tips.iter().enumerate() {
        for b in &tips[index + 1..] {
            // Every pair of tips but the opposite ones is an edge.
            if a.dot(*b) > -1.0 {
                octahedron.bar(&frame, *a, *b, bar, Color::srgb(1.0, 0.3, 0.85));
            }
        }
    }

    // Two rings of cyan, one inside the other, low over the south-west.
    let mut halo = Geometry::default();
    for (radius, color) in [
        (62.0, Color::srgb(0.2, 0.95, 1.0)),
        (44.0, Color::srgb(0.1, 0.55, 1.0)),
    ] {
        let pieces = 48;
        for piece in 0..pieces {
            let angle = |piece: usize| TAU * piece as f32 / pieces as f32;
            let at = |piece| Vec3::new(angle(piece).cos(), angle(piece).sin(), 0.0) * radius;
            halo.bar(&frame, at(piece), at(piece + 1), bar, color);
        }
    }

    // A tetrahedron of amber light, in the south-east.
    let mut tetrahedron = Geometry::default();
    let corners = [
        Vec3::new(1.0, 1.0, 1.0),
        Vec3::new(1.0, -1.0, -1.0),
        Vec3::new(-1.0, 1.0, -1.0),
        Vec3::new(-1.0, -1.0, 1.0),
    ]
    .map(|corner| corner * 20.0);
    for (index, a) in corners.iter().enumerate() {
        for b in &corners[index + 1..] {
            tetrahedron.bar(&frame, *a, *b, bar, Color::srgb(1.0, 0.65, 0.2));
        }
    }

    vec![
        Shape {
            mesh: octahedron,
            center: toward(35.0, 24.0) * RADIUS * 0.95,
            tilt: Quat::from_rotation_z(0.4),
            spin: Spin {
                axis: Vec3::Y,
                speed: 0.12,
            },
        },
        Shape {
            mesh: halo,
            center: toward(-150.0, 18.0) * RADIUS * 0.95,
            tilt: Quat::from_rotation_x(1.1) * Quat::from_rotation_y(0.5),
            spin: Spin {
                axis: Vec3::Z,
                speed: 0.05,
            },
        },
        Shape {
            mesh: tetrahedron,
            center: toward(150.0, 22.0) * RADIUS * 0.95,
            tilt: Quat::IDENTITY,
            spin: Spin {
                axis: Vec3::new(0.3, 1.0, 0.2),
                speed: -0.09,
            },
        },
    ]
}

/// A direction picked evenly over the whole sphere.
fn anywhere(dice: &mut Dice) -> Vec3 {
    let height = dice.range(-1.0, 1.0);
    let angle = dice.range(0.0, TAU);
    let across = (1.0 - height * height).max(0.0).sqrt();
    Vec3::new(across * angle.cos(), height, across * angle.sin())
}

/// Two directions across the line of sight toward `direction`: right and up, as the camera sees a
/// shape there.
fn facing(direction: Vec3) -> (Vec3, Vec3) {
    let right = direction
        .cross(Vec3::Y)
        .try_normalize()
        .unwrap_or(Vec3::X);
    (right, right.cross(direction))
}

/// A color made brighter or darker, its alpha kept.
fn scale(color: Color, factor: f32) -> Color {
    let linear = color.to_linear();
    LinearRgba::new(
        linear.red * factor,
        linear.green * factor,
        linear.blue * factor,
        linear.alpha,
    )
    .into()
}

/// Triangles colored at their corners, every one turned toward the middle of the sky, where the
/// camera is.
#[derive(Default)]
struct Canvas {
    positions: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Canvas {
    fn triangle(&mut self, corners: [Vec3; 3], colors: [Color; 3]) {
        let base = self.positions.len() as u32;
        for (corner, color) in corners.into_iter().zip(colors) {
            self.positions.push(corner.to_array());
            self.colors.push(color.to_linear().to_f32_array());
        }
        // Counter-clockwise seen from the middle of the sky, where the camera is.
        let [a, b, c] = corners;
        let facing_in = (b - a).cross(c - a).dot(-a) > 0.0;
        let order = if facing_in { [0, 1, 2] } else { [0, 2, 1] };
        self.indices.extend(order.map(|index| base + index));
    }

    fn quad(&mut self, corners: [Vec3; 4], colors: [Color; 4]) {
        let [a, b, c, d] = corners;
        let [ca, cb, cc, cd] = colors;
        self.triangle([a, b, c], [ca, cb, cc]);
        self.triangle([a, c, d], [ca, cc, cd]);
    }

    /// Adds another canvas's triangles to this one.
    fn extend(&mut self, other: Canvas) {
        let base = self.positions.len() as u32;
        self.positions.extend(other.positions);
        self.colors.extend(other.colors);
        self.indices
            .extend(other.indices.into_iter().map(|index| base + index));
    }

    fn into_mesh(self) -> Mesh {
        // Unlit, the normals only have to exist: toward the middle, like the triangles.
        let normals: Vec<[f32; 3]> = self
            .positions
            .iter()
            .map(|position| (-Vec3::from(*position)).normalize_or(Vec3::Y).to_array())
            .collect();
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_of_the_sky() {
        // Straight ahead of the menus, east, and overhead.
        assert!(toward(0.0, 0.0).distance(Vec3::NEG_Z) < 1e-5);
        assert!(toward(90.0, 0.0).distance(Vec3::X) < 1e-5);
        assert!(toward(30.0, 90.0).distance(Vec3::Y) < 1e-5);
    }

    #[test]
    fn everything_in_the_sky_faces_the_camera_from_where_the_sky_is() {
        let mut solid = Canvas::default();
        let mut glow = Canvas::default();
        let mut dice = Dice::new(SEED);
        stars(&mut solid, &mut glow, &mut dice);
        planets(&mut solid, &mut glow);
        for canvas in [&solid, &glow] {
            for triangle in canvas.indices.chunks(3) {
                let [a, b, c] = [0, 1, 2].map(|corner| {
                    Vec3::from(canvas.positions[triangle[corner] as usize])
                });
                // Out beyond the circuit, short of where the camera stops seeing.
                for corner in [a, b, c] {
                    assert!((500.0..1000.0).contains(&corner.length()), "{corner}");
                }
                // Turned toward the middle, where the camera is.
                let normal = (b - a).cross(c - a);
                assert!(normal.dot(-a) >= 0.0, "{a} {b} {c}");
            }
        }
    }

    #[test]
    fn a_planet_lit_from_behind_is_a_crescent() {
        let surface = Color::WHITE;
        let light = Vec3::new(1.0, 0.0, -0.3).normalize();
        let edge = shade(surface, Vec2::new(0.95, 0.0), light).to_linear().red;
        let middle = shade(surface, Vec2::ZERO, light).to_linear().red;
        let far = shade(surface, Vec2::new(-0.95, 0.0), light).to_linear().red;
        assert!(edge > middle && middle <= far + 1e-3, "{edge} {middle} {far}");
        assert!(far > 0.0, "the night side stays visible");
    }
}
