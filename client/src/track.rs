//! The circuit of the lobby, as the server described it: its geometry, and the mesh drawn from it.
//!
//! Deliberately plain, but built to read speed and position: the road in the dark, walls striped
//! every few meters so movement shows at the edge of the screen, a dashed centerline, a checkered
//! start line, kerbs through the turns and the light of the walls on the road (see [`markings`]),
//! and piers carrying it all down into the dark (see [`piers`]). Neon parts are unlit, so they stay
//! bright whatever the light.

mod markings;
mod piers;

use std::sync::Arc;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use space_race_sim::track::scenery::Prop;
use space_race_sim::track::{Track, TrackPoint};

use crate::city::{self, Landmarks};
use crate::geometry::Geometry;
use crate::lobby::{CurrentLobby, Tracks};
use crate::scenery;
use crate::screen::Screen;
use crate::world;

/// How high the walls stand over the road edge, and how thick they are, in meters. The scenery
/// stands beyond them.
pub const WALL_HEIGHT: f32 = 1.0;
pub const WALL_THICKNESS: f32 = 0.6;
/// How thick the road is under its surface, in meters: it floats in the dark as a slab, its walls
/// running down past its edges to the underside, however high the relief carries it.
pub const SLAB: f32 = 1.5;
/// Wall stripes alternate colors every this many meters.
const WALL_STRIPE: f32 = 4.0;
const DASH_LENGTH: f32 = 4.0;
const DASH_GAP: f32 = 4.0;
const DASH_WIDTH: f32 = 0.3;
/// Pieces per dash, so dashes follow the curve.
const DASH_PIECES: usize = 4;
/// The checkered start line has two rows of squares of this size.
const START_SQUARE: f32 = 1.0;
/// Markings sit this far above the road, so they do not flicker against it.
const MARKING_HEIGHT: f32 = 0.02;
/// Strips across the road, so its surface follows the curve of banked turns.
const ROAD_STRIPS: usize = 8;

const ROAD: Color = Color::srgb(0.1, 0.1, 0.11);
const WALL_DARK: Color = Color::srgb(0.17, 0.17, 0.19);
/// The one bright color of the game, shared by the walls, the markings and the scenery.
pub const NEON_BLUE: Color = Color::srgb(0.05, 0.6, 1.0);
const MARKING: Color = Color::srgb(0.92, 0.92, 0.85);
const CHECKER_DARK: Color = Color::srgb(0.05, 0.05, 0.05);

pub struct TrackPlugin;

impl Plugin for TrackPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Lobby), spawn_track)
            .add_systems(OnExit(Screen::Lobby), remove_track);
    }
}

/// The circuit of the lobby the player is in.
#[derive(Resource)]
pub struct CurrentTrack(pub Arc<Track>);

/// The roofs over the circuit: its tunnels, which no camera may rise through.
#[derive(Resource, Default)]
pub struct Roofs(pub Vec<Roof>);

/// A tunnel's roof: from `at` for `length` meters along the lap, `clearance` meters over the road.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Roof {
    pub at: f32,
    pub length: f32,
    pub clearance: f32,
}

impl Roofs {
    fn of(props: &[Prop]) -> Self {
        Self(
            props
                .iter()
                .filter_map(|prop| match *prop {
                    Prop::Tunnel {
                        at,
                        length,
                        clearance,
                    } => Some(Roof {
                        at,
                        length,
                        clearance,
                    }),
                    _ => None,
                })
                .collect(),
        )
    }

    /// How high the roof stands over the road `distance` meters along a lap `lap` meters round, if
    /// a tunnel covers the road there.
    pub fn clearance_at(&self, distance: f32, lap: f32) -> Option<f32> {
        self.0
            .iter()
            .find(|roof| (distance - roof.at).rem_euclid(lap) <= roof.length)
            .map(|roof| roof.clearance)
    }
}

/// Everything built for the lobby's circuit, the city around it included, which goes with it.
#[derive(Component, Clone)]
struct TrackScene;

fn spawn_track(
    mut commands: Commands,
    current: Res<CurrentLobby>,
    tracks: Res<Tracks>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let Some(membership) = &current.0 else {
        return;
    };
    let Some(entry) = tracks.0.get(&membership.settings.track) else {
        error!(key = %membership.settings.track, "the lobby races on a track the server did not send");
        return;
    };
    let track = Arc::clone(&entry.track);

    // Colors come from the vertices; the materials only set how the surfaces take light.
    let TrackMeshes {
        lit,
        scenery,
        neon,
        glow,
        triangles,
        landmarks,
    } = track_meshes(&track, &entry.description.scenery);
    let lit_material = materials.add(StandardMaterial {
        perceptual_roughness: 0.9,
        ..default()
    });
    let neon_material = materials.add(StandardMaterial {
        unlit: true,
        ..default()
    });
    commands.spawn((
        TrackScene,
        Mesh3d(meshes.add(lit)),
        MeshMaterial3d(lit_material),
    ));
    // The scenery casts no shadow: under a tunnel the road stays as readable as anywhere else, and
    // the city is not drawn again into the shadow maps every frame. Nor does it shine: seen along
    // its faces, as the road shows them, a building would catch the light whatever its color, and
    // the city has to stay dark behind its neon.
    let scenery_material = materials.add(StandardMaterial {
        perceptual_roughness: 1.0,
        reflectance: 0.0,
        ..default()
    });
    commands.spawn((
        TrackScene,
        Mesh3d(meshes.add(scenery)),
        MeshMaterial3d(scenery_material),
        NotShadowCaster,
    ));
    commands.spawn((
        TrackScene,
        Mesh3d(meshes.add(neon)),
        MeshMaterial3d(neon_material),
    ));
    // The light the walls throw on the road, added to whatever it falls on.
    commands.spawn((
        TrackScene,
        Mesh3d(meshes.add(glow)),
        MeshMaterial3d(materials.add(StandardMaterial {
            unlit: true,
            alpha_mode: AlphaMode::Add,
            ..default()
        })),
        NotShadowCaster,
    ));
    city::spawn(
        &mut commands,
        &mut meshes,
        &mut materials,
        &track,
        &landmarks,
        TrackScene,
    );
    info!(
        name = track.name(),
        length = %format_args!("{:.1} m", track.length()),
        props = entry.description.scenery.len(),
        triangles,
        towers = landmarks.towers.len(),
        beacons = landmarks.beacons.len(),
        searchlights = landmarks.searchlights.len(),
        "track built"
    );
    commands.insert_resource(CurrentTrack(track));
    commands.insert_resource(Roofs::of(&entry.description.scenery));
}

fn remove_track(mut commands: Commands, scenes: Query<Entity, With<TrackScene>>) {
    for scene in &scenes {
        commands.entity(scene).despawn();
    }
    commands.remove_resource::<CurrentTrack>();
    commands.remove_resource::<Roofs>();
}

/// The track split by material: surfaces that take light, neon that glows on its own, and the
/// light the walls throw on the road, added to whatever it falls on. The scenery's neon goes into
/// the same mesh as the track's, and its surfaces, with the piers', into one of their own, which
/// casts no shadow, so a decorated circuit costs four draw calls. And where the scenery left room
/// for the city to light.
struct TrackMeshes {
    lit: Mesh,
    scenery: Mesh,
    neon: Mesh,
    glow: Mesh,
    triangles: usize,
    landmarks: Landmarks,
}

fn track_meshes(track: &Track, props: &[Prop]) -> TrackMeshes {
    let mut lit = Geometry::default();
    let mut neon = Geometry::default();
    let mut glow = Geometry::default();
    road(&mut lit, track);
    underside(&mut lit, track);
    walls(&mut lit, &mut neon, track);
    center_dashes(&mut neon, track);
    start_line(&mut lit, track);
    markings::kerbs(&mut lit, track);
    markings::edge_lines(&mut lit, track);
    markings::grid_boxes(&mut lit, track);
    markings::wall_light(&mut glow, track);
    let mut scenery = Geometry::default();
    piers::piers(&mut scenery, &mut neon, track);
    piers::keel_lines(&mut neon, track);
    let landmarks = scenery::build(&mut scenery, &mut neon, track, props);
    TrackMeshes {
        triangles: lit.triangles() + scenery.triangles() + neon.triangles() + glow.triangles(),
        lit: lit.into_mesh(),
        scenery: scenery.into_mesh(),
        neon: neon.into_mesh(),
        glow: glow.into_mesh(),
        landmarks,
    }
}

/// The road surface, in strips across it. Each strip is a share of the half width rather than a
/// number of meters, so the surface follows the walls in and out through a bottleneck.
fn road(geometry: &mut Geometry, track: &Track) {
    for_each_segment(track, |a, b, _| {
        for index in 0..ROAD_STRIPS {
            let left = 1.0 - 2.0 * index as f32 / ROAD_STRIPS as f32;
            let right = left - 2.0 / ROAD_STRIPS as f32;
            let corners = [
                on_road(track, a, left * a.half_width, 0.0),
                on_road(track, a, right * a.half_width, 0.0),
                on_road(track, b, right * b.half_width, 0.0),
                on_road(track, b, left * b.half_width, 0.0),
            ];
            geometry.quad_facing_up(corners, ROAD);
        }
    });
}

/// The underside of the road, [`SLAB`] under its surface and reaching out under the walls: seen
/// from lower down the relief, the road is a slab rather than a sheet that vanishes from below.
fn underside(geometry: &mut Geometry, track: &Track) {
    for_each_segment(track, |a, b, _| {
        let edge = |point: TrackPoint| point.half_width + WALL_THICKNESS;
        for index in 0..ROAD_STRIPS {
            let left = 1.0 - 2.0 * index as f32 / ROAD_STRIPS as f32;
            let right = left - 2.0 / ROAD_STRIPS as f32;
            let corners = [
                on_road(track, a, left * edge(a), -SLAB),
                on_road(track, a, right * edge(a), -SLAB),
                on_road(track, b, right * edge(b), -SLAB),
                on_road(track, b, left * edge(b), -SLAB),
            ];
            let above = corners.iter().sum::<Vec3>() / 4.0 + Vec3::Y;
            geometry.quad_facing_away(corners, above, WALL_DARK);
        }
    });
}

/// Walls striped dark gray and neon blue. They stand on the road edge, and their outer face goes
/// down past it to the underside of the road, so the raised edge of a banked turn looks solid from
/// outside.
fn walls(lit: &mut Geometry, neon: &mut Geometry, track: &Track) {
    for side in [1.0, -1.0] {
        for_each_segment(track, |a, b, distance| {
            let (geometry, color) = if (distance / WALL_STRIPE) as usize % 2 == 0 {
                (&mut *lit, WALL_DARK)
            } else {
                (&mut *neon, NEON_BLUE)
            };
            // The road edge, which a bottleneck brings in toward the centerline.
            let inner = |point: TrackPoint| side * point.half_width;
            let outer = |point: TrackPoint| side * (point.half_width + WALL_THICKNESS);
            let base = |point: TrackPoint| track.height_beside(&point, inner(point));
            let at = |point: TrackPoint, lateral: f32, height: f32| {
                world::position(beside(point, lateral), height)
            };
            let [top_a, top_b] = [a, b].map(|point| base(point) + WALL_HEIGHT);
            let facing_road = world::direction(a.direction.perp() * -side);

            geometry.quad(
                [
                    at(a, inner(a), base(a)),
                    at(b, inner(b), base(b)),
                    at(b, inner(b), top_b),
                    at(a, inner(a), top_a),
                ],
                facing_road,
                color,
            );
            geometry.quad_facing_up(
                [
                    at(a, inner(a), top_a),
                    at(b, inner(b), top_b),
                    at(b, outer(b), top_b),
                    at(a, outer(a), top_a),
                ],
                color,
            );
            geometry.quad(
                [
                    at(a, outer(a), base(a) - SLAB),
                    at(b, outer(b), base(b) - SLAB),
                    at(b, outer(b), top_b),
                    at(a, outer(a), top_a),
                ],
                -facing_road,
                color,
            );
        });
    }
}

fn center_dashes(geometry: &mut Geometry, track: &Track) {
    let period = DASH_LENGTH + DASH_GAP;
    let half = DASH_WIDTH / 2.0;
    for dash in 0..(track.length() / period) as usize {
        let start = dash as f32 * period;
        for piece in 0..DASH_PIECES {
            let piece_length = DASH_LENGTH / DASH_PIECES as f32;
            let a = track.point_at(start + piece as f32 * piece_length);
            let b = track.point_at(start + (piece + 1) as f32 * piece_length);
            let corners = [
                on_road(track, a, half, MARKING_HEIGHT),
                on_road(track, a, -half, MARKING_HEIGHT),
                on_road(track, b, -half, MARKING_HEIGHT),
                on_road(track, b, half, MARKING_HEIGHT),
            ];
            geometry.quad_facing_up(corners, NEON_BLUE);
        }
    }
}

fn start_line(geometry: &mut Geometry, track: &Track) {
    let half_width = track.half_width_at(0.0);
    let columns = (2.0 * half_width / START_SQUARE).round() as usize;
    for row in 0..2 {
        // Above the dashes, which it crosses.
        let a = track.point_at((row as f32 - 1.0) * START_SQUARE);
        let b = track.point_at(row as f32 * START_SQUARE);
        for column in 0..columns {
            let right = -half_width + column as f32 * START_SQUARE;
            let left = right + START_SQUARE;
            let corners = [
                on_road(track, a, right, 2.0 * MARKING_HEIGHT),
                on_road(track, b, right, 2.0 * MARKING_HEIGHT),
                on_road(track, b, left, 2.0 * MARKING_HEIGHT),
                on_road(track, a, left, 2.0 * MARKING_HEIGHT),
            ];
            let color = if (row + column) % 2 == 0 {
                CHECKER_DARK
            } else {
                MARKING
            };
            geometry.quad_facing_up(corners, color);
        }
    }
}

/// The world position `lateral` meters beside a centerline point, `above` meters over the road.
fn on_road(track: &Track, point: TrackPoint, lateral: f32, above: f32) -> Vec3 {
    world::position(
        beside(point, lateral),
        built_height(track, &point, lateral) + above,
    )
}

/// The height of the road as it is built, `lateral` meters beside a centerline point: straight
/// across each of its strips, from the curved cross-section's height at one edge of the strip to
/// its height at the other. A banked road is curved, its strips are flat, and in between the strips
/// stand above the curve, by 4.5 cm in the most steeply banked turn of the skyway: a line painted
/// on the curve itself sank under the road there, and showed as dashes. Off the road, it is the
/// curve's height, which carries on flat beyond the edges.
fn built_height(track: &Track, point: &TrackPoint, lateral: f32) -> f32 {
    let strip = 2.0 * point.half_width / ROAD_STRIPS as f32;
    let from_right = (lateral + point.half_width) / strip;
    if !(0.0..=ROAD_STRIPS as f32).contains(&from_right) {
        return track.height_beside(point, lateral);
    }
    let edge = from_right.floor().min(ROAD_STRIPS as f32 - 1.0);
    let across = |edge: f32| track.height_beside(point, edge * strip - point.half_width);
    let (near, far) = (across(edge), across(edge + 1.0));
    near + (far - near) * (from_right - edge)
}

/// Calls `f` with each pair of consecutive centerline points, closing the loop, and the distance
/// along the track of the first one.
fn for_each_segment(track: &Track, mut f: impl FnMut(TrackPoint, TrackPoint, f32)) {
    let points = track.points();
    for (index, point) in points.iter().enumerate() {
        let next = points[(index + 1) % points.len()];
        f(*point, next, index as f32 * track.spacing());
    }
}

/// The point `lateral` meters beside a centerline point, positive on the left.
fn beside(point: TrackPoint, lateral: f32) -> Vec2 {
    point.position + point.direction.perp() * lateral
}
