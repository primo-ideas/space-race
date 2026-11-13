//! The drift charge, drawn on the road under the player's car, the way Rocket Racing draws it: an
//! arc of segments wrapped around the back of the car that lights up from the middle outward as
//! the drift builds up its boost, and is gone when the car is not drifting.
//!
//! It follows the direction the car travels rather than the body, which a drift swings far across
//! it, so it stays under the car and square to the camera, which follows the travel too.

use std::f32::consts::PI;

use bevy::prelude::*;
use star_racer_sim::track::Surface;

use crate::race::{RaceUpdate, RaceView};
use crate::screen::Screen;
use crate::track::CurrentTrack;
use crate::ui::theme;
use crate::world;

/// Segments of the arc, an odd number so one sits in the middle.
const SEGMENTS: usize = 21;
/// How far from the middle of the car the arc runs, and how far round the back of it either way,
/// in meters and radians.
const RADIUS: f32 = 3.2;
const HALF_SPAN: f32 = 50.0 * PI / 180.0;
/// Each segment's size along the arc and across it, and how far over the road it floats, in meters.
const SEGMENT_LENGTH: f32 = 0.18;
const SEGMENT_WIDTH: f32 = 0.34;
const ABOVE_ROAD: f32 = 0.06;
/// A segment not yet lit: the same green, too dark to glow. Green, like the drift gauge, rather
/// than the blue of the road markings it crosses.
const UNLIT: Color = Color::srgb(0.03, 0.12, 0.07);

pub struct DriftMeterPlugin;

impl Plugin for DriftMeterPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, create_assets)
            .add_systems(OnEnter(Screen::Lobby), spawn_meter)
            .add_systems(OnExit(Screen::Lobby), remove_meter)
            .add_systems(
                Update,
                show_charge
                    .after(RaceUpdate)
                    .run_if(in_state(Screen::Lobby)),
            );
    }
}

#[derive(Resource)]
struct MeterAssets {
    segment: Handle<Mesh>,
    lit: Handle<StandardMaterial>,
    unlit: Handle<StandardMaterial>,
}

/// The arc as a whole, placed under the car.
#[derive(Component)]
struct DriftMeter;

/// One segment, by how far it is from the middle of the arc: 0 for the middle one.
#[derive(Component)]
struct Segment(usize);

fn create_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let neon = |color: Color| StandardMaterial {
        base_color: color,
        unlit: true,
        ..default()
    };
    commands.insert_resource(MeterAssets {
        segment: meshes.add(Cuboid::new(SEGMENT_WIDTH, 0.04, SEGMENT_LENGTH)),
        lit: materials.add(neon(theme::GREEN)),
        unlit: materials.add(neon(UNLIT)),
    });
}

fn spawn_meter(mut commands: Commands, assets: Res<MeterAssets>) {
    let middle = SEGMENTS / 2;
    commands
        .spawn((DriftMeter, Transform::default(), Visibility::Hidden))
        .with_children(|meter| {
            for index in 0..SEGMENTS {
                // Round the back of the car, which faces `-x` in the meter's own frame, `+z` being
                // its right.
                let angle = (index as f32 / (SEGMENTS - 1) as f32 * 2.0 - 1.0) * HALF_SPAN;
                meter.spawn((
                    Segment(index.abs_diff(middle)),
                    Mesh3d(assets.segment.clone()),
                    MeshMaterial3d(assets.unlit.clone()),
                    Transform::from_xyz(-RADIUS * angle.cos(), 0.0, RADIUS * angle.sin())
                        .with_rotation(Quat::from_rotation_y(angle)),
                ));
            }
        });
}

fn remove_meter(mut commands: Commands, meters: Query<Entity, With<DriftMeter>>) {
    for meter in &meters {
        commands.entity(meter).despawn();
    }
}

/// How many frames the arc is drawn out of sight once the player has a car, before any drift.
const WARM_UP_FRAMES: u32 = 10;

/// Where the arc is drawn while it warms up: ahead of the car, where the chase camera looks, and
/// inside the road's slab, 1.5 m thick, which hides it. Under the car it was out of the camera's
/// view, culled before anything was compiled for it.
const WARM_UP_AHEAD: f32 = 12.0;
const WARM_UP_DEPTH: f32 = 0.8;

/// Puts the arc under the player's own car while it drifts, lit as far as the drift has charged.
///
/// Before the first drift, it is drawn for a few frames under the road, out of sight, both its
/// materials at once. Its segments are the one mesh of a race laid out unlike the rest, so they
/// need render pipelines of their own, and a pipeline is only made the first time something drawn
/// needs it. Natively that happens out of the way; in a browser it is compiled there and then, and
/// the first drift of a run froze the page for seconds (the user's report on 2026-11-13: a freeze
/// a few seconds into the warm-up before a race). Drawn once out of sight, it costs that at the
/// lobby's entry instead, with everything else that is compiled then.
fn show_charge(
    view: Res<RaceView>,
    track: Option<Res<CurrentTrack>>,
    assets: Res<MeterAssets>,
    mut meters: Query<(&mut Transform, &mut Visibility), With<DriftMeter>>,
    mut segments: Query<(&Segment, &mut MeshMaterial3d<StandardMaterial>)>,
    mut warmed: Local<u32>,
) {
    let Ok((mut transform, mut visibility)) = meters.single_mut() else {
        return;
    };
    if *warmed < WARM_UP_FRAMES
        && let (Some(snapshot), Some(track)) = (view.own_snapshot, track.as_ref())
    {
        *warmed += 1;
        let car = snapshot.car;
        let ahead = car.position + car.forward() * WARM_UP_AHEAD;
        let surface: Surface = track.0.surface(ahead);
        *transform =
            Transform::from_translation(world::position(ahead, surface.height - WARM_UP_DEPTH));
        *visibility = Visibility::Inherited;
        for (segment, mut material) in &mut segments {
            material.0 = if segment.0 % 2 == 0 {
                assets.lit.clone()
            } else {
                assets.unlit.clone()
            };
        }
        return;
    }
    let drifting = view
        .own_snapshot
        .filter(|snapshot| snapshot.car.is_drifting());
    let (Some(snapshot), Some(track)) = (drifting, track) else {
        *visibility = Visibility::Hidden;
        return;
    };
    let car = snapshot.car;
    let surface: Surface = track.0.surface(car.position);
    let travel = if car.velocity.length_squared() > 1e-6 {
        car.velocity.to_angle()
    } else {
        car.heading
    };
    let rotation = world::rotation_on_surface(travel, surface.gradient);
    *transform = Transform::from_translation(
        world::position(car.position, surface.height) + rotation * Vec3::Y * ABOVE_ROAD,
    )
    .with_rotation(rotation);
    *visibility = Visibility::Inherited;

    // From the middle outward, symmetrically: the middle segment first, the two ends last.
    let lit = (snapshot.drift_gauge * (SEGMENTS / 2 + 1) as f32).round() as usize;
    for (segment, mut material) in &mut segments {
        let wanted = if segment.0 < lit {
            &assets.lit
        } else {
            &assets.unlit
        };
        if material.0 != *wanted {
            material.0 = wanted.clone();
        }
    }
}
