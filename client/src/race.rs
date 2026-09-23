//! The race as the player sees it: server snapshots, interpolated into smooth car movement.
//!
//! Snapshots arrive 60 times per second, but never exactly on time. The client estimates the
//! server clock from them and displays the race slightly in the past, at
//! [`INTERPOLATION_DELAY`], so there is almost always a snapshot on each side of the displayed
//! moment to interpolate between.

use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

use bevy::platform::time::Instant;
use bevy::prelude::*;
use space_race_protocol::{CarId, CarSnapshot, Snapshot};
use space_race_sim::TICK_SECONDS;
use space_race_sim::car::Car;
use space_race_sim::track::Surface;

use crate::lobby::CurrentLobby;
use crate::net::NetEvent;
use crate::prediction::{Prediction, predict};
use crate::screen::Screen;
use crate::track::CurrentTrack;
use crate::world;

/// How far in the past the race is displayed, in seconds: three ticks.
const INTERPOLATION_DELAY: f64 = 3.0 * TICK_SECONDS as f64;

/// Share of the gap closed at each snapshot when snapshots arrive later than the clock estimate
/// expects. Small, so one delayed snapshot barely moves the estimate, but a lasting delay is
/// followed within a second or two.
const CLOCK_CATCH_UP: f64 = 0.02;

pub struct RacePlugin;

impl Plugin for RacePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SnapshotBuffer>()
            .init_resource::<Prediction>()
            .init_resource::<RaceView>()
            .init_resource::<Spectated>()
            .add_systems(Startup, create_car_assets)
            .add_systems(OnExit(Screen::Lobby), forget_race)
            .add_systems(
                Update,
                (receive_snapshots, predict, choose_spectated, display_cars)
                    .chain()
                    .in_set(RaceUpdate)
                    .run_if(in_state(Screen::Lobby)),
            );
    }
}

/// Updates [`RaceView`] and the displayed cars. Whatever follows the cars runs after it.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub struct RaceUpdate;

/// The race as currently displayed, for the camera, the HUD and the autopilot.
#[derive(Resource, Default)]
pub struct RaceView {
    /// Every car on the track.
    pub cars: Vec<CarSnapshot>,
    /// The player's own car, unless they spectate.
    pub own_car: Option<Car>,
    /// The car the camera follows: the player's own, or the one they watch.
    pub followed: Option<(CarId, Car)>,
    /// The player's own car as the server sent it, with its drift and boost gauges.
    pub own_snapshot: Option<CarSnapshot>,
    /// Height of the road under the followed car, for the camera.
    pub followed_height: f32,
    /// The server tick displayed, between two ticks most of the time. `None` before the first
    /// snapshot.
    pub tick: Option<f64>,
    /// The player's own timeline, ahead of the server, while their car is predicted: the clock
    /// their start lights and race time follow, so the lights turn green when their car can go.
    pub own_tick: Option<f64>,
}

/// The car a spectating player watches. `None` follows the leader.
#[derive(Resource, Default)]
pub struct Spectated(pub Option<CarId>);

#[derive(Component)]
struct CarVisual(CarId);

#[derive(Resource)]
struct CarAssets {
    body: Handle<Mesh>,
    cabin: Handle<Mesh>,
    nose: Handle<Mesh>,
    own_color: Handle<StandardMaterial>,
    other_color: Handle<StandardMaterial>,
    cabin_color: Handle<StandardMaterial>,
    nose_color: Handle<StandardMaterial>,
}

fn create_car_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Cars are modeled along `+x`, the direction of a zero heading.
    commands.insert_resource(CarAssets {
        body: meshes.add(Cuboid::new(3.6, 0.5, 1.8)),
        cabin: meshes.add(Cuboid::new(1.5, 0.45, 1.5)),
        nose: meshes.add(Cuboid::new(0.3, 0.12, 1.4)),
        own_color: materials.add(Color::srgb(0.95, 0.35, 0.1)),
        other_color: materials.add(Color::srgb(0.2, 0.45, 0.9)),
        cabin_color: materials.add(Color::srgb(0.12, 0.12, 0.15)),
        nose_color: materials.add(Color::srgb(1.0, 0.9, 0.2)),
    });
}

fn forget_race(
    mut commands: Commands,
    mut view: ResMut<RaceView>,
    mut spectated: ResMut<Spectated>,
    visuals: Query<Entity, With<CarVisual>>,
) {
    *view = RaceView::default();
    spectated.0 = None;
    for visual in &visuals {
        commands.entity(visual).despawn();
    }
}

fn receive_snapshots(
    mut events: MessageReader<NetEvent>,
    mut buffer: ResMut<SnapshotBuffer>,
    current: Res<CurrentLobby>,
    time: Res<Time<Real>>,
) {
    for event in events.read() {
        // Snapshots still on their way after leaving a lobby are not for this one.
        if let NetEvent::Snapshot { snapshot, received } = event
            && current.0.is_some()
        {
            buffer.push(snapshot.clone(), seconds_since_startup(&time, *received));
        }
    }
}

/// Left and right switch the watched car while spectating.
fn choose_spectated(
    mut spectated: ResMut<Spectated>,
    view: Res<RaceView>,
    keyboard: Res<ButtonInput<KeyCode>>,
    gamepads: Query<&Gamepad>,
) {
    if view.own_car.is_some() || view.cars.is_empty() {
        return;
    }
    let pressed = |keys: [KeyCode; 2], button: GamepadButton| {
        keyboard.any_just_pressed(keys) || gamepads.iter().any(|pad| pad.just_pressed(button))
    };
    let step = if pressed([KeyCode::KeyA, KeyCode::ArrowLeft], GamepadButton::DPadLeft) {
        -1
    } else if pressed(
        [KeyCode::KeyD, KeyCode::ArrowRight],
        GamepadButton::DPadRight,
    ) {
        1
    } else {
        return;
    };

    // In race order, so right goes to the car behind.
    let mut order: Vec<_> = view.cars.iter().collect();
    order.sort_by(|a, b| b.progress.total_cmp(&a.progress));
    let current = view
        .followed
        .and_then(|(id, _)| order.iter().position(|car| car.id == id))
        .unwrap_or(0);
    let next = (current as i32 + step).rem_euclid(order.len() as i32) as usize;
    spectated.0 = Some(order[next].id);
}

fn display_cars(
    mut commands: Commands,
    mut buffer: ResMut<SnapshotBuffer>,
    mut view: ResMut<RaceView>,
    current: Res<CurrentLobby>,
    spectated: Res<Spectated>,
    prediction: Res<Prediction>,
    assets: Res<CarAssets>,
    time: Res<Time<Real>>,
    track: Option<Res<CurrentTrack>>,
    mut visuals: Query<(Entity, &CarVisual, &mut Transform)>,
) {
    let own_id = current.0.as_ref().map(|membership| membership.car);
    let now = time
        .last_update()
        .map(|frame| seconds_since_startup(&time, frame));
    let mut cars = match now {
        Some(now) => buffer.sample(now),
        None => Vec::new(),
    };
    // The player's own car is predicted rather than interpolated (see prediction.rs), gauges and
    // all; how far it is into the race stays what the server said.
    let tuning = current
        .0
        .as_ref()
        .and_then(|membership| membership.state.as_ref())
        .map(|state| &state.tuning);
    if let (Some(own_id), Some(predicted), Some(tuning)) = (own_id, prediction.car(), tuning)
        && let Some(own) = cars.iter_mut().find(|car| car.id == own_id)
    {
        *own = CarSnapshot::new(own_id, predicted, own.progress, own.input_seq, tuning);
    }
    let find = |id: CarId| {
        cars.iter()
            .find(|car| car.id == id)
            .map(|car| (id, car.car))
    };
    let leader = cars
        .iter()
        .max_by(|a, b| a.progress.total_cmp(&b.progress))
        .map(|car| (car.id, car.car));
    let own = own_id.and_then(find);
    // The player's own car, or the watched one, or the leader.
    let followed = own.or_else(|| spectated.0.and_then(find)).or(leader);
    let surface = |car: &Car| match &track {
        Some(track) => track.0.surface(car.position),
        None => Surface {
            height: 0.0,
            gradient: Vec2::ZERO,
        },
    };

    view.own_car = own.map(|(_, car)| car);
    view.own_snapshot = own_id.and_then(|id| cars.iter().find(|car| car.id == id).copied());
    view.followed = followed;
    view.followed_height = followed.map_or(0.0, |(_, car)| surface(&car).height);
    view.tick = buffer
        .displayed_time()
        .map(|time| time / f64::from(TICK_SECONDS));
    view.own_tick = prediction.own_tick();

    let mut displayed = Vec::new();
    for (entity, visual, mut transform) in &mut visuals {
        match cars.iter().find(|car| car.id == visual.0) {
            Some(car) => {
                *transform = car_transform(&car.car, surface(&car.car));
                displayed.push(visual.0);
            }
            None => commands.entity(entity).despawn(),
        }
    }

    for CarSnapshot { id, car, .. } in &cars {
        if displayed.contains(id) {
            continue;
        }
        let body_color = if Some(*id) == own_id {
            assets.own_color.clone()
        } else {
            assets.other_color.clone()
        };
        commands.spawn((
            CarVisual(*id),
            car_transform(car, surface(car)),
            Visibility::default(),
            children![
                (
                    Mesh3d(assets.body.clone()),
                    MeshMaterial3d(body_color),
                    Transform::from_xyz(0.0, 0.45, 0.0),
                ),
                (
                    Mesh3d(assets.cabin.clone()),
                    MeshMaterial3d(assets.cabin_color.clone()),
                    Transform::from_xyz(-0.4, 0.92, 0.0),
                ),
                (
                    Mesh3d(assets.nose.clone()),
                    MeshMaterial3d(assets.nose_color.clone()),
                    Transform::from_xyz(1.7, 0.76, 0.0),
                ),
            ],
        ));
    }
    view.cars = cars;
}

/// Seconds from app startup to `instant`. Snapshot arrival times and the current frame time both go
/// through here, so they share one origin: `Time::elapsed` counts from the first frame instead,
/// which starts most of a second after startup on a slow machine. Mixing the two once displayed
/// the race 770 ms late instead of 50.
pub(crate) fn seconds_since_startup(time: &Time<Real>, instant: Instant) -> f64 {
    instant
        .saturating_duration_since(time.startup())
        .as_secs_f64()
}

/// A car resting on the road: at its height, leaning with its slope, its body turned into a drift.
fn car_transform(car: &Car, surface: Surface) -> Transform {
    Transform::from_translation(world::position(car.position, surface.height)).with_rotation(
        world::rotation_on_surface(car.body_heading(), surface.gradient),
    )
}

#[derive(Resource, Default)]
pub struct SnapshotBuffer {
    /// In tick order: the connection is ordered.
    snapshots: VecDeque<Snapshot>,
    /// Local time minus server time, in seconds, for the least delayed snapshots.
    clock_offset: Option<f64>,
    /// The server time last displayed, in seconds.
    displayed_time: Option<f64>,
}

impl SnapshotBuffer {
    /// Adds a snapshot received at local time `received_at`, in seconds.
    pub fn push(&mut self, snapshot: Snapshot, received_at: f64) {
        let offset = received_at - server_time(snapshot.tick);
        self.clock_offset = Some(match self.clock_offset {
            // Later than expected: probably a delayed snapshot, follow slowly.
            Some(current) if offset > current => current + (offset - current) * CLOCK_CATCH_UP,
            // Earlier than expected: the estimate was pessimistic, adopt it.
            _ => offset,
        });
        self.snapshots.push_back(snapshot);
    }

    /// The server time last displayed by [`SnapshotBuffer::sample`], in seconds.
    pub fn displayed_time(&self) -> Option<f64> {
        self.displayed_time
    }

    /// The server's time at local time `now`, in seconds, as far as the snapshots tell: the time
    /// of a snapshot that would arrive now as promptly as the promptest ever did. It trails the
    /// server's own clock by the way down, which the prediction's lead makes up for.
    pub fn server_time(&self, now: f64) -> Option<f64> {
        self.clock_offset.map(|offset| now - offset)
    }

    /// Every car at local time `now`, interpolated between the snapshots around the displayed
    /// moment. Holds the latest state if no newer snapshot has arrived yet.
    pub fn sample(&mut self, now: f64) -> Vec<CarSnapshot> {
        let Some(offset) = self.clock_offset else {
            return Vec::new();
        };
        let displayed_time = now - offset - INTERPOLATION_DELAY;
        self.displayed_time = Some(displayed_time);

        // Keep one snapshot at or before the displayed moment, drop the older ones.
        while self
            .snapshots
            .get(1)
            .is_some_and(|next| server_time(next.tick) <= displayed_time)
        {
            self.snapshots.pop_front();
        }

        let Some(from) = self.snapshots.front() else {
            return Vec::new();
        };
        let Some(to) = self.snapshots.get(1) else {
            return from.cars.clone();
        };
        let span = server_time(to.tick) - server_time(from.tick);
        let t = ((displayed_time - server_time(from.tick)) / span).clamp(0.0, 1.0) as f32;
        to.cars
            .iter()
            .map(|target| {
                match from.cars.iter().find(|previous| previous.id == target.id) {
                    Some(previous) => {
                        let lerp = |from: f32, to: f32| from + (to - from) * t;
                        CarSnapshot {
                            id: target.id,
                            car: interpolate(&previous.car, &target.car, t),
                            progress: lerp(previous.progress, target.progress),
                            drift_gauge: lerp(previous.drift_gauge, target.drift_gauge),
                            boost_gauge: lerp(previous.boost_gauge, target.boost_gauge),
                            input_seq: target.input_seq,
                        }
                    }
                    // Just joined: nothing to interpolate from.
                    None => *target,
                }
            })
            .collect()
    }
}

fn server_time(tick: u32) -> f64 {
    f64::from(tick) * f64::from(TICK_SECONDS)
}

/// Continuous values are interpolated; the drift side and flags are taken from the newer state.
pub(crate) fn interpolate(from: &Car, to: &Car, t: f32) -> Car {
    let lerp = |from: f32, to: f32| from + (to - from) * t;
    Car {
        position: from.position.lerp(to.position, t),
        velocity: from.velocity.lerp(to.velocity, t),
        heading: from.heading + shortest_angle(from.heading, to.heading) * t,
        slip: lerp(from.slip, to.slip),
        yaw_rate: lerp(from.yaw_rate, to.yaw_rate),
        drift_charge: lerp(from.drift_charge, to.drift_charge),
        boost: lerp(from.boost, to.boost),
        ..*to
    }
}

/// Signed angle from `from` to `to`, the short way around.
pub fn shortest_angle(from: f32, to: f32) -> f32 {
    (to - from + PI).rem_euclid(TAU) - PI
}

#[cfg(test)]
mod tests {
    use bevy::math::Vec2;

    use super::*;

    fn snapshot(tick: u32, x: f32, heading: f32) -> Snapshot {
        Snapshot {
            tick,
            cars: vec![CarSnapshot {
                id: CarId(0),
                car: Car::new(Vec2::new(x, 0.0), heading),
                progress: 0.0,
                drift_gauge: 0.0,
                boost_gauge: 0.0,
                input_seq: 0,
            }],
        }
    }

    const TICK: f64 = TICK_SECONDS as f64;

    #[test]
    fn interpolates_between_the_snapshots_around_the_displayed_moment() {
        let mut buffer = SnapshotBuffer::default();
        // Snapshots arrive exactly on time, one second of local time after server time zero.
        for tick in 0..10 {
            buffer.push(
                snapshot(tick, tick as f32, 0.0),
                1.0 + f64::from(tick) * TICK,
            );
        }
        // At tick 8.5 in local time, the display is three ticks behind: tick 5.5.
        let cars = buffer.sample(1.0 + 8.5 * TICK);
        assert!((cars[0].car.position.x - 5.5).abs() < 1e-3, "{cars:?}");
    }

    #[test]
    fn holds_the_latest_state_when_snapshots_stop() {
        let mut buffer = SnapshotBuffer::default();
        buffer.push(snapshot(0, 0.0, 0.0), 0.0);
        buffer.push(snapshot(1, 1.0, 0.0), TICK);
        let cars = buffer.sample(10.0);
        assert_eq!(cars[0].car.position.x, 1.0);
    }

    #[test]
    fn one_late_snapshot_barely_moves_the_clock() {
        let mut buffer = SnapshotBuffer::default();
        buffer.push(snapshot(0, 0.0, 0.0), 0.0);
        buffer.push(snapshot(1, 1.0, 0.0), TICK + 0.2);
        let offset = buffer.clock_offset.unwrap();
        assert!(offset < 0.01, "{offset}");

        // An early snapshot is adopted at once.
        buffer.push(snapshot(2, 2.0, 0.0), 2.0 * TICK - 0.05);
        assert!((buffer.clock_offset.unwrap() + 0.05).abs() < 1e-9);
    }

    #[test]
    fn heading_interpolates_the_short_way_across_the_back() {
        let from = snapshot(0, 0.0, PI - 0.1).cars[0].car;
        let to = snapshot(1, 0.0, -PI + 0.1).cars[0].car;
        let middle = interpolate(&from, &to, 0.5);
        assert!(
            (shortest_angle(middle.heading, PI)).abs() < 1e-4,
            "{}",
            middle.heading
        );
    }
}
