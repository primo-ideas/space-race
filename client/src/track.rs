//! The circuit of the lobby, as the server described it: its geometry, and the mesh drawn from it.
//!
//! Deliberately plain, but built to read speed and position: the road alone in the dark, walls
//! striped every few meters so movement shows at the edge of the screen, a dashed centerline, and
//! a checkered start line. Neon parts are unlit, so they stay bright whatever the light.

use std::sync::Arc;

use bevy::prelude::*;
use space_race_sim::track::scenery::Prop;
use space_race_sim::track::{Track, TrackPoint};

use crate::geometry::Geometry;
use crate::lobby::{CurrentLobby, Tracks};
use crate::scenery;
use crate::screen::Screen;
use crate::world;

/// How high the walls stand over the road edge, and how thick they are, in meters. The scenery
/// stands beyond them.
pub const WALL_HEIGHT: f32 = 1.0;
pub const WALL_THICKNESS: f32 = 0.6;
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

#[derive(Component)]
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
        neon,
        triangles,
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
    commands.spawn((
        TrackScene,
        Mesh3d(meshes.add(neon)),
        MeshMaterial3d(neon_material),
    ));
    info!(
        name = track.name(),
        length = %format_args!("{:.1} m", track.length()),
        props = entry.description.scenery.len(),
        triangles,
        "track built"
    );
    commands.insert_resource(CurrentTrack(track));
}

fn remove_track(mut commands: Commands, scenes: Query<Entity, With<TrackScene>>) {
    for scene in &scenes {
        commands.entity(scene).despawn();
    }
    commands.remove_resource::<CurrentTrack>();
}

/// The track split by material: surfaces that take light, and neon that glows on its own. The
/// scenery goes into the same two meshes, so a decorated circuit costs no more draw calls.
struct TrackMeshes {
    lit: Mesh,
    neon: Mesh,
    triangles: usize,
}

fn track_meshes(track: &Track, props: &[Prop]) -> TrackMeshes {
    let mut lit = Geometry::default();
    let mut neon = Geometry::default();
    road(&mut lit, track);
    walls(&mut lit, &mut neon, track);
    center_dashes(&mut neon, track);
    start_line(&mut lit, track);
    scenery::build(&mut lit, &mut neon, track, props);
    TrackMeshes {
        triangles: lit.triangles() + neon.triangles(),
        lit: lit.into_mesh(),
        neon: neon.into_mesh(),
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

/// Walls striped dark gray and neon blue. They stand on the road edge, and their outer face goes
/// down to the ground, so the raised edge of a banked turn looks solid from outside.
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
                    at(a, outer(a), 0.0),
                    at(b, outer(b), 0.0),
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
        track.height_beside(&point, lateral) + above,
    )
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
