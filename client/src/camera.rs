//! The camera chasing the player's car, and the light.
//!
//! One camera, two ways of holding it. While the player drives, it hangs behind the car and lags a
//! little through a turn. From the moment they cross the finish line to the next grid, results
//! included, their car drives itself (see `docs/lobbies.md`) and the camera cuts between fixed
//! shots instead, the way a race is shown on television: there is nothing left to steer, so the
//! camera stops serving the driving and starts showing it.

use std::f32::consts::PI;
use std::sync::Arc;

use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::light::CascadeShadowConfigBuilder;
use bevy::prelude::*;
use space_race_protocol::PlayerStatus;
use space_race_sim::car::Car;
use space_race_sim::track::Track;

use crate::lobby::CurrentLobby;
use crate::race::{RaceUpdate, RaceView, shortest_angle};
use crate::screen::Screen;
use crate::track::CurrentTrack;
use crate::world;

/// Distance behind the car and height, in meters.
const DISTANCE: f32 = 8.5;
const HEIGHT: f32 = 3.2;
/// The camera aims this far ahead of the car, this high.
const LOOK_AHEAD: f32 = 6.0;
const LOOK_HEIGHT: f32 = 1.0;
/// How quickly the camera swings back behind the car when it turns, per second. The slight lag
/// shows the car turning, instead of the whole world rotating around a fixed car.
const HEADING_FOLLOW: f32 = 6.0;
/// The field of view widens with speed, up to `FOV_FAST` at `FOV_FULL_SPEED` m/s.
const FOV_SLOW: f32 = 65.0 * PI / 180.0;
const FOV_FAST: f32 = 78.0 * PI / 180.0;
const FOV_FULL_SPEED: f32 = 40.0;
/// Below this forward speed, in m/s, the camera follows the heading rather than the direction of
/// travel.
const MIN_TRAVEL_SPEED: f32 = 5.0;

/// How long one shot of the finish lasts, in seconds. Short enough that no shot outstays its
/// welcome, long enough to read what the car is doing.
const SHOT_SECONDS: f64 = 3.6;
/// The shots play in this order, over and over until the race ends. They alternate near and far, and
/// left and right, so two shots in a row never look alike.
const SHOTS: [Shot; 6] = [
    Shot::Crane,
    Shot::Beside { left: true },
    Shot::TrackSide { left: false },
    Shot::Low,
    Shot::Beside { left: false },
    Shot::Ahead,
];
/// The field of view of a shot that does not follow the car, and the one of a shot that hangs back
/// from it.
const FOV_FIXED: f32 = 42.0 * PI / 180.0;
const FOV_WIDE: f32 = 55.0 * PI / 180.0;
/// The camera keeping pace beside the car stands this far to its side and this high, and stays at
/// least this far inside the edge of the road.
const BESIDE_OUT: f32 = 6.0;
const BESIDE_HEIGHT: f32 = 2.8;
const BESIDE_EDGE: f32 = 1.5;
/// The camera on a post stands this far down the road, this far outside the wall, and this high:
/// high enough to look over the wall, even the raised outer wall of a banked turn.
const TRACK_SIDE_AHEAD: f32 = 45.0;
const TRACK_SIDE_OUT: f32 = 6.0;
const TRACK_SIDE_HEIGHT: f32 = 9.0;

pub struct CameraPlugin;

impl Plugin for CameraPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_camera_and_light)
            .add_systems(OnEnter(Screen::Lobby), reset_chase)
            .add_systems(
                Update,
                follow_car.after(RaceUpdate).run_if(in_state(Screen::Lobby)),
            );
    }
}

/// The camera the game is seen through: over the menus' backdrop, then behind the car.
#[derive(Component)]
pub struct MainCamera;

#[derive(Component, Default)]
struct ChaseCamera {
    /// The heading the camera looks along, lagging behind the car's.
    heading: Option<f32>,
    /// The shot playing since the player finished: its place in [`SHOTS`], and when it started.
    shot: Option<(usize, f64)>,
    /// Where a shot that stands still was put when it started.
    stand: Option<Vec3>,
}

/// One way of showing a car that drives itself. Most shots are worked out from the car alone; the
/// two that would otherwise end up inside a wall read the road instead.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shot {
    /// Low and close behind: the road rushing past under the car.
    Low,
    /// Keeping pace alongside, at the height of the walls.
    Beside { left: bool },
    /// Ahead of the car, looking back at it coming.
    Ahead,
    /// High behind, looking down on the line the car takes.
    Crane,
    /// Planted beside the road ahead, panning as the car goes by and away: the shot a camera on a
    /// post takes.
    TrackSide { left: bool },
}

fn spawn_camera_and_light(mut commands: Commands) {
    commands.spawn((
        MainCamera,
        Camera3d::default(),
        // Without tone mapping, neon colors stay as saturated as they are written.
        Tonemapping::None,
        Projection::Perspective(PerspectiveProjection {
            fov: FOV_SLOW,
            ..default()
        }),
        Transform::default(),
        ChaseCamera::default(),
    ));

    commands.spawn((
        DirectionalLight {
            shadow_maps_enabled: true,
            ..default()
        },
        Transform::from_xyz(30.0, 100.0, 50.0).looking_at(Vec3::ZERO, Vec3::Y),
        // Shadows only matter near the camera; fewer cascades are cheaper on modest GPUs. WebGL2
        // supports a single one.
        CascadeShadowConfigBuilder {
            num_cascades: if cfg!(target_arch = "wasm32") { 1 } else { 2 },
            first_cascade_far_bound: 25.0,
            maximum_distance: 120.0,
            ..default()
        }
        .build(),
    ));
}

/// A new lobby starts with the camera straight behind the car, not swinging from the last one.
fn reset_chase(mut cameras: Query<&mut ChaseCamera>) {
    for mut chase in &mut cameras {
        *chase = ChaseCamera::default();
    }
}

fn follow_car(
    view: Res<RaceView>,
    current: Res<CurrentLobby>,
    track: Option<Res<CurrentTrack>>,
    time: Res<Time>,
    mut cameras: Query<(&mut Transform, &mut Projection, &mut ChaseCamera)>,
) {
    let Some((_, car)) = view.followed else {
        return;
    };
    let Ok((mut transform, mut projection, mut chase)) = cameras.single_mut() else {
        return;
    };
    let finished = current.0.as_ref().is_some_and(|membership| {
        matches!(membership.own_status(), Some(PlayerStatus::Finished { .. }))
    });

    let car_position = world::position(car.position, view.followed_height);
    let fov = if finished {
        let track = track.map(|track| Arc::clone(&track.0));
        show_finish(
            &mut transform,
            &mut chase,
            track.as_deref(),
            &car,
            car_position,
            &time,
        )
    } else {
        // Back behind the car, from wherever the last shot left the camera.
        if chase.shot.take().is_some() {
            chase.heading = None;
        }
        chase_car(&mut transform, &mut chase, &car, car_position, &time)
    };

    if let Projection::Perspective(perspective) = projection.as_mut() {
        perspective.fov = fov;
    }
}

/// The camera hanging behind the car the player drives. Returns the field of view to see it with,
/// which widens with speed.
fn chase_car(
    transform: &mut Transform,
    chase: &mut ChaseCamera,
    car: &Car,
    car_position: Vec3,
    time: &Time,
) -> f32 {
    let target = travel_heading(car);
    let heading = match chase.heading {
        Some(current) => {
            let follow = 1.0 - (-HEADING_FOLLOW * time.delta_secs()).exp();
            current + shortest_angle(current, target) * follow
        }
        None => target,
    };
    chase.heading = Some(heading);

    let forward = world::direction(Vec2::from_angle(heading));
    transform.translation = car_position - forward * DISTANCE + Vec3::Y * HEIGHT;
    transform.look_at(
        car_position + forward * LOOK_AHEAD + Vec3::Y * LOOK_HEIGHT,
        Vec3::Y,
    );

    let speed_share = (car.velocity.length() / FOV_FULL_SPEED).min(1.0);
    FOV_SLOW + (FOV_FAST - FOV_SLOW) * speed_share
}

/// The camera once the player has finished: a shot that holds for [`SHOT_SECONDS`], then a cut to
/// the next one.
fn show_finish(
    transform: &mut Transform,
    chase: &mut ChaseCamera,
    track: Option<&Track>,
    car: &Car,
    car_position: Vec3,
    time: &Time,
) -> f32 {
    let now = time.elapsed_secs_f64();
    let (index, started) = match chase.shot {
        Some((index, started)) if now - started < SHOT_SECONDS => (index, started),
        // A cut: the next shot, and a stand to be chosen again if it needs one.
        Some((index, _)) => {
            chase.stand = None;
            ((index + 1) % SHOTS.len(), now)
        }
        None => {
            chase.stand = None;
            (0, now)
        }
    };
    chase.shot = Some((index, started));

    let shot = SHOTS[index];
    let heading = travel_heading(car);
    let forward = world::direction(Vec2::from_angle(heading));
    let left_of_car = world::direction(Vec2::from_angle(heading).perp());
    let elapsed = (now - started) as f32;
    // Without a track — which should not happen in a lobby — every shot falls back to the car.
    let from_car = |elapsed| shot.eye(car_position, forward, left_of_car, elapsed);

    let eye = match (shot, track) {
        // A shot that stands still is worked out once, when it starts.
        (Shot::TrackSide { left }, Some(track)) => *chase
            .stand
            .get_or_insert_with(|| track_side(track, car, left)),
        (Shot::TrackSide { .. }, None) => *chase.stand.get_or_insert_with(|| from_car(0.0)),
        (Shot::Beside { left }, Some(track)) => beside_over_the_road(track, car, left, elapsed),
        _ => from_car(elapsed),
    };
    transform.translation = eye;
    transform.look_at(car_position + Vec3::Y * LOOK_HEIGHT, Vec3::Y);
    shot.fov(car)
}

/// A camera on a post beside the road ahead, high enough to see over the walls: the car comes to
/// it, goes by and away. Read from the track rather than from the car, or a turn would put the post
/// behind a wall, or on the road itself.
fn track_side(track: &Track, car: &Car, left: bool) -> Vec3 {
    let here = track.project(car.position);
    let ahead = track.point_at(here.distance + TRACK_SIDE_AHEAD);
    let side = if left { 1.0 } else { -1.0 };
    let lateral = side * (ahead.half_width + TRACK_SIDE_OUT);
    world::position(
        ahead.position + ahead.direction.perp() * lateral,
        track.height_beside(&ahead, lateral) + TRACK_SIDE_HEIGHT,
    )
}

/// The camera keeping pace beside the car, staying over the road: a fixed offset would push it
/// into a wall, or under the raised edge of a banked turn, whenever the car runs close to one. It
/// takes the roomier side when the car hugs the side it was meant to film from.
fn beside_over_the_road(track: &Track, car: &Car, left: bool, elapsed: f32) -> Vec3 {
    let here = track.project(car.position);
    let room = track.half_width_at(here.distance) - BESIDE_EDGE;
    let preferred = if left { 1.0 } else { -1.0 };
    let side = if here.lateral * preferred > room - BESIDE_OUT {
        -preferred
    } else {
        preferred
    };
    let lateral = (here.lateral + side * BESIDE_OUT).clamp(-room, room);
    // Sliding gently forward along the car, from its door toward its nose.
    let point = track.point_at(here.distance + elapsed * 0.9 - 1.0);
    world::position(
        point.position + point.direction.perp() * lateral,
        track.height_beside(&point, lateral) + BESIDE_HEIGHT,
    )
}

impl Shot {
    /// Where the camera stands for this shot, `elapsed` seconds into it, knowing only the car.
    /// [`Shot::Beside`] and [`Shot::TrackSide`] are placed from the track instead, whenever the
    /// lobby has one.
    fn eye(self, car: Vec3, forward: Vec3, left: Vec3, elapsed: f32) -> Vec3 {
        let side = |left_side: bool| if left_side { left } else { -left };
        match self {
            // Creeping closer, so the road speeds up under the car.
            Self::Low => car - forward * (6.5 - elapsed * 0.35) + Vec3::Y * 1.1,
            Self::Beside { left } => {
                car + side(left) * BESIDE_OUT
                    + forward * (elapsed * 0.9 - 1.0)
                    + Vec3::Y * BESIDE_HEIGHT
            }
            Self::Ahead => car + forward * 10.0 + Vec3::Y * 2.2,
            // Rising, which opens the track out under the car.
            Self::Crane => car - forward * 9.0 + Vec3::Y * (8.0 + elapsed * 1.2),
            Self::TrackSide { left } => {
                car + forward * TRACK_SIDE_AHEAD
                    + side(left) * (TRACK_SIDE_OUT + 8.0)
                    + Vec3::Y * TRACK_SIDE_HEIGHT
            }
        }
    }

    /// How wide the shot is: the closer the camera and the faster the car, the wider.
    fn fov(self, car: &Car) -> f32 {
        let speed_share = (car.velocity.length() / FOV_FULL_SPEED).min(1.0);
        match self {
            Self::Low | Self::Beside { .. } => FOV_SLOW + (FOV_FAST - FOV_SLOW) * speed_share,
            Self::Ahead | Self::Crane => FOV_WIDE,
            Self::TrackSide { .. } => FOV_FIXED,
        }
    }
}

/// Where the camera looks along: where the car goes once it moves forward, so a drift shows the car
/// sliding sideways instead of the whole view swinging with it; where it points otherwise, such as
/// after bouncing off a wall.
fn travel_heading(car: &Car) -> f32 {
    if car.forward_speed() > MIN_TRAVEL_SPEED {
        car.velocity.to_angle()
    } else {
        car.heading
    }
}

#[cfg(test)]
mod tests {
    use space_race_sim::track::{Segment, TrackDescription};

    use super::*;

    /// A car at the origin, heading along `+x`, which the world maps to `+x` on the ground.
    fn scene() -> (Vec3, Vec3, Vec3) {
        (Vec3::ZERO, Vec3::X, Vec3::NEG_Z)
    }

    #[test]
    fn every_shot_stands_clear_of_the_car_and_off_the_ground() {
        let (car, forward, left) = scene();
        for shot in SHOTS {
            for elapsed in [0.0, SHOT_SECONDS as f32] {
                let eye = shot.eye(car, forward, left, elapsed);
                let distance = eye.distance(car);
                assert!((4.0..60.0).contains(&distance), "{shot:?}: {distance} m");
                assert!(eye.y > 0.8, "{shot:?}: {} m up", eye.y);
            }
        }
    }

    #[test]
    fn shots_look_at_the_car_from_different_sides() {
        let (car, forward, left) = scene();
        let eyes: Vec<Vec3> = SHOTS
            .iter()
            .map(|shot| shot.eye(car, forward, left, 0.0))
            .collect();
        // Behind, beside on either side, and ahead: no two shots stand in the same place.
        for (index, eye) in eyes.iter().enumerate() {
            for other in &eyes[index + 1..] {
                assert!(eye.distance(*other) > 2.0, "{eye} and {other}");
            }
        }
        // One shot at least looks back at the car from in front of it.
        assert!(eyes.iter().any(|eye| (*eye - car).dot(forward) > 5.0));
        // And one from each side.
        assert!(eyes.iter().any(|eye| (*eye - car).dot(left) > 3.0));
        assert!(eyes.iter().any(|eye| (*eye - car).dot(left) < -3.0));
    }

    #[test]
    fn a_shot_that_stands_still_is_the_one_the_car_drives_past() {
        let (car, forward, left) = scene();
        let stand = Shot::TrackSide { left: false }.eye(car, forward, left, 0.0);
        // It waits well down the road, so the car comes to it before going away.
        assert!((stand - car).dot(forward) > 30.0);
    }

    /// A banked oval, as tight as the drift circuit's hairpins: a shot placed by dead reckoning
    /// alone would end up through a wall here.
    fn oval() -> Track {
        let turn = Segment::Turn {
            angle: 180.0,
            radius: 20.0,
            transition: 10.0,
            banking: 20.0,
        };
        Track::build(&TrackDescription {
            name: "Oval".into(),
            width: 16.0,
            segments: vec![
                Segment::Straight { length: 30.0 },
                turn,
                Segment::Straight { length: 60.0 },
                turn,
                Segment::Straight { length: 30.0 },
            ],
            narrows: Vec::new(),
            scenery: Vec::new(),
        })
        .unwrap()
    }

    /// A car `lateral` meters from the centerline, `at` meters along the lap.
    fn car_on_track(track: &Track, at: f32, lateral: f32) -> Car {
        let point = track.point_at(at);
        let mut car = Car::new(
            point.position + point.direction.perp() * lateral,
            point.direction.to_angle(),
        );
        car.velocity = point.direction * 40.0;
        car
    }

    /// The lateral offset of a world position from the centerline, positive on the left.
    fn lateral_of(track: &Track, position: Vec3) -> f32 {
        track.project(Vec2::new(position.x, -position.z)).lateral
    }

    #[test]
    fn the_camera_on_a_post_stands_outside_the_wall_and_down_the_road() {
        let track = oval();
        // In a turn, where dead reckoning would put the post through the outer wall.
        let car = car_on_track(&track, 45.0, 0.0);
        for left in [true, false] {
            let stand = track_side(&track, &car, left);
            let lateral = lateral_of(&track, stand);
            assert!(lateral.abs() > track.half_width(), "{lateral} m");
            assert_eq!(lateral.signum(), if left { 1.0 } else { -1.0 });
            // High enough to look over a wall, and far enough down the road to be driven past.
            assert!(stand.y > 5.0, "{} m up", stand.y);
            let ahead = track.project(Vec2::new(stand.x, -stand.z)).distance;
            let along = (ahead - 45.0).rem_euclid(track.length());
            assert!((30.0..60.0).contains(&along), "{along} m down the road");
        }
    }

    #[test]
    fn the_camera_beside_the_car_stays_over_the_road() {
        let track = oval();
        let room = track.half_width() - BESIDE_EDGE;
        // Including a car hugging either wall, where a fixed offset would leave the road.
        for lateral in [0.0, room, -room] {
            let car = car_on_track(&track, 45.0, lateral);
            for left in [true, false] {
                let eye = beside_over_the_road(&track, &car, left, 0.0);
                let beside = lateral_of(&track, eye);
                assert!(beside.abs() <= room + 0.1, "{beside} m from the centerline");
                assert!(
                    (beside - lateral).abs() > 2.0,
                    "{beside} m, right on the car at {lateral} m"
                );
            }
        }
    }
}
