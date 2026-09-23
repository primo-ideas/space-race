//! Behind the menus: a neon grid rushing toward the camera and fading into the dark, the game's
//! look in motion while nothing is being driven.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::pbr::{DistanceFog, FogFalloff};
use bevy::prelude::*;

use crate::camera::MainCamera;
use crate::screen::Screen;
use crate::ui::theme;

/// Grid spacing, how far it reaches, and how thick its lines are, in meters.
const SPACING: f32 = 6.0;
const HALF_WIDTH: f32 = 90.0;
const DEPTH: f32 = 170.0;
const LINE_WIDTH: f32 = 0.12;
/// The grid moves toward the camera at this speed, in m/s.
const SPEED: f32 = 14.0;
/// Where the camera stands and looks over the grid.
const CAMERA_HEIGHT: f32 = 3.0;
const LOOK_AT: Vec3 = Vec3::new(0.0, 1.8, -60.0);
const FOV: f32 = 65.0 * std::f32::consts::PI / 180.0;

pub struct BackdropPlugin;

impl Plugin for BackdropPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(OnEnter(Screen::Login), show)
            .add_systems(OnEnter(Screen::Lobbies), show)
            .add_systems(OnEnter(Screen::Lobby), hide)
            .add_systems(
                Update,
                (frame_camera, scroll).run_if(not(in_state(Screen::Lobby))),
            );
    }
}

#[derive(Component)]
struct Backdrop;

fn show(
    mut commands: Commands,
    backdrops: Query<(), With<Backdrop>>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    if !backdrops.is_empty() {
        return;
    }
    commands.spawn((
        Backdrop,
        Mesh3d(meshes.add(grid_mesh())),
        MeshMaterial3d(materials.add(StandardMaterial {
            base_color: theme::NEON,
            unlit: true,
            ..default()
        })),
        Transform::default(),
    ));
}

/// Places the camera over the grid, with fog to fade it into the dark. Every frame rather than on
/// entering a screen, since the first screen is entered before the camera exists.
fn frame_camera(
    mut commands: Commands,
    mut cameras: Query<
        (Entity, &mut Transform, &mut Projection, Has<DistanceFog>),
        With<MainCamera>,
    >,
) {
    for (camera, mut transform, mut projection, has_fog) in &mut cameras {
        let pose = Transform::from_xyz(0.0, CAMERA_HEIGHT, 0.0).looking_at(LOOK_AT, Vec3::Y);
        transform.set_if_neq(pose);
        if let Projection::Perspective(perspective) = projection.as_mut()
            && perspective.fov != FOV
        {
            perspective.fov = FOV;
        }
        if !has_fog {
            commands.entity(camera).insert(DistanceFog {
                color: Color::BLACK,
                falloff: FogFalloff::Linear {
                    start: 15.0,
                    end: 150.0,
                },
                ..default()
            });
        }
    }
}

fn hide(
    mut commands: Commands,
    backdrops: Query<Entity, With<Backdrop>>,
    cameras: Query<Entity, With<MainCamera>>,
) {
    for backdrop in &backdrops {
        commands.entity(backdrop).despawn();
    }
    for camera in &cameras {
        commands.entity(camera).remove::<DistanceFog>();
    }
}

/// Moves the grid toward the camera, jumping back by one spacing so it seems endless.
fn scroll(time: Res<Time>, mut backdrops: Query<&mut Transform, With<Backdrop>>) {
    for mut transform in &mut backdrops {
        transform.translation.z = (time.elapsed_secs() * SPEED) % SPACING;
    }
}

/// Flat lines on the ground, along and across the view, in front of the camera.
fn grid_mesh() -> Mesh {
    let mut positions = Vec::new();
    let mut indices = Vec::new();
    let mut quad = |corners: [Vec3; 4]| {
        let base = positions.len() as u32;
        positions.extend(corners.map(|corner| corner.to_array()));
        indices.extend([base, base + 1, base + 2, base, base + 2, base + 3]);
    };
    let half = LINE_WIDTH / 2.0;
    let far = -DEPTH;
    let near = 2.0 * SPACING;

    let lines_along = (HALF_WIDTH / SPACING) as i32;
    for index in -lines_along..=lines_along {
        let x = index as f32 * SPACING;
        quad([
            Vec3::new(x - half, 0.0, near),
            Vec3::new(x + half, 0.0, near),
            Vec3::new(x + half, 0.0, far),
            Vec3::new(x - half, 0.0, far),
        ]);
    }
    let lines_across = ((near - far) / SPACING) as i32;
    for index in 0..=lines_across {
        let z = near - index as f32 * SPACING;
        quad([
            Vec3::new(-HALF_WIDTH, 0.0, z + half),
            Vec3::new(HALF_WIDTH, 0.0, z + half),
            Vec3::new(HALF_WIDTH, 0.0, z - half),
            Vec3::new(-HALF_WIDTH, 0.0, z - half),
        ]);
    }

    let count = positions.len();
    Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::RENDER_WORLD,
    )
    .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, positions)
    .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, vec![[0.0, 1.0, 0.0]; count])
    .with_inserted_indices(Indices::U32(indices))
}
