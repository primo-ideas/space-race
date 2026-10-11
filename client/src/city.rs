//! What moves around a circuit at night, and the glow it all stands in: traffic streaming round the
//! city over its skyline, searchlights sweeping the sky from the grandstands, warning lights blinking
//! red on the tallest roofs, and the haze of the streets far below, which nobody sees but by their
//! light.
//!
//! None of it is written in the track file, and none of it touches the simulation. It is worked out
//! from what the file already builds: the scenery says where the towers and the grandstands stand
//! (see [`Landmarks`]), and the road says how far the city reaches. Like the sky, it is rolled from
//! fixed seeds, so every player sees the same city doing the same things.
//!
//! What moves is moved whole: a lane of traffic is one mesh slid along its own direction, a beam is
//! one mesh turned about its foot, and the warning lights blink by changing the color of the one
//! material each group of them shares. Nothing is rebuilt once the lobby is entered.

use std::f32::consts::TAU;

use bevy::light::NotShadowCaster;
use bevy::prelude::*;
use space_race_sim::track::Track;

use crate::dice::Dice;
use crate::geometry::Geometry;
use crate::screen::Screen;
use crate::world;

/// Seeds of what the city rolls, so it comes out the same for everyone.
const TRAFFIC_SEED: u64 = 0x7AFF_1C5E;
const SEARCHLIGHT_SEED: u64 = 0x5EA2_C41E;

/// Corridors of traffic round the city, each of them two lanes flying opposite ways, side by side
/// and a few meters apart in height.
const CORRIDORS: usize = 6;
const LANE_GAP: f32 = 14.0;
const LANE_STEP: f32 = 4.0;
/// How far outside the city a corridor flies, in meters past the furthest edge of its road: past
/// the districts, which reach less than a hundred meters out, so no vehicle ever flies through a
/// building. Overhead, the traffic was a sky full of confetti; out there, it streams along the
/// skyline, where the eye already is.
const LANE_OUTSIDE: (f32, f32) = (150.0, 380.0);
/// How high a corridor flies, from a little under the highest thing built to some way over it, in
/// meters: over the skyline, seen from the road.
const LANE_ALTITUDE: (f32, f32) = (-12.0, 38.0);
/// How far past the city the lanes run, in meters: past where the camera stops seeing.
const LANE_REACH: f32 = 1100.0;
/// The vehicles of a lane come in a pattern this long, repeated all the way down it, which is what
/// lets a lane flow forever: it slides by at most one pattern's length and starts again.
const TRAFFIC_PERIOD: f32 = 260.0;
/// How far apart two vehicles of a lane fly, at least and at most, in meters.
const VEHICLE_GAP: (f32, f32) = (18.0, 60.0);
/// How fast a corridor flows, in m/s.
const TRAFFIC_SPEED: (f32, f32) = (30.0, 62.0);
/// A vehicle's hull: its length, its height and its width, in meters, and how big its lights are.
const VEHICLE: Vec3 = Vec3::new(5.0, 1.2, 2.2);
const LIGHT: f32 = 1.0;
/// The lights of a vehicle: white ahead, red behind, each a block that shows from every side, so a
/// stream coming is a string of white, one going a string of red, and one passing pairs of both.
/// Brighter than white, so they glow.
const HEADLIGHT: Color = Color::linear_rgb(1.8, 1.55, 1.15);
const TAIL_LIGHT: Color = Color::linear_rgb(1.7, 0.08, 0.04);
const HULL: Color = Color::srgb(0.025, 0.03, 0.045);

/// A searchlight's beam: how long it is, and how many sides its cones are drawn with.
const BEAM_LENGTH: f32 = 330.0;
const BEAM_SIDES: usize = 20;
/// A beam is two cones, one inside the other: a core, and a wider, fainter glow around it, so its
/// light falls off toward its edges rather than stopping at a hard line. Each is how wide it is at
/// the foot and at the end, in meters, and how much of the beam's light it shows at the foot.
const BEAM_CONES: [(f32, f32, f32); 2] = [(0.5, 9.0, 0.1), (1.4, 22.0, 0.045)];
/// Where along the beam its rings stand, as shares of its length: close together at the foot, where
/// it fades fastest.
const BEAM_RINGS: [f32; 7] = [0.0, 0.03, 0.1, 0.22, 0.4, 0.65, 1.0];
/// The beam's light. It fades out along the beam.
const BEAM_COLOR: Color = Color::srgb(0.62, 0.78, 1.0);
/// How far a beam leans from straight up, in radians, how far that lean swings, and how fast a beam
/// turns, in radians a second.
const BEAM_TILT: (f32, f32) = (0.28, 0.48);
const BEAM_TILT_SWING: (f32, f32) = (0.06, 0.14);
const BEAM_TURN: (f32, f32) = (0.14, 0.3);

/// A warning light on a roof: how big it is, in meters, and how red. Far brighter than white, so it
/// glows, and blinks on the darkest roof as well as the brightest.
const BEACON_SIZE: f32 = 1.2;
const BEACON_RED: LinearRgba = LinearRgba::new(3.0, 0.07, 0.035, 1.0);
/// The warning lights blink in a few groups, each in its own time, once every so many seconds, and
/// never go out quite completely.
const BEACON_GROUPS: usize = 3;
const BEACON_PERIOD: f64 = 1.7;
const BEACON_DIM: f32 = 0.04;

/// The haze of the city far below: how deep it lies, in meters above the ground, below the footings
/// that fade to black on their way down to it, and how far past the circuit it reaches.
const HAZE_DEPTH: f32 = -58.0;
const HAZE_REACH: f32 = 450.0;
const HAZE_RINGS: usize = 28;
const HAZE_SECTORS: usize = 64;
/// The haze everywhere under the circuit, fading out toward its rim, and the warmer glow of the
/// streets under the towers, as if lit by them. Added to whatever is behind it.
const HAZE_BASE: LinearRgba = LinearRgba::new(0.016, 0.006, 0.042, 1.0);
const HAZE_STREETS: LinearRgba = LinearRgba::new(0.045, 0.014, 0.03, 1.0);
/// How far around a tower the streets are lit, in meters.
const HAZE_SPREAD: f32 = 70.0;

pub struct CityPlugin;

impl Plugin for CityPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (flow_traffic, sweep_searchlights, blink_beacons).run_if(in_state(Screen::Lobby)),
        );
    }
}

/// What the scenery leaves for the city to light: filled in as the scenery is built.
#[derive(Debug, Default)]
pub struct Landmarks {
    /// The middle of every tower's ground, on the simulation plane: where the streets below are
    /// lit.
    pub towers: Vec<Vec2>,
    /// Where a warning light stands, on a roof or on a mast, in the world.
    pub beacons: Vec<Vec3>,
    /// Where a searchlight stands, on the top row of a grandstand, in the world.
    pub searchlights: Vec<Vec3>,
    /// How high the highest thing built stands over the ground, in meters: the traffic flies at
    /// about that height, over the skyline.
    pub highest: f32,
}

impl Landmarks {
    /// Something stands up to `height` meters over the ground.
    pub fn rise_to(&mut self, height: f32) {
        self.highest = self.highest.max(height);
    }
}

/// A lane of traffic, slid along its world direction at its speed, by up to one pattern's length.
#[derive(Component)]
struct Traffic {
    direction: Vec3,
    speed: f32,
}

/// A searchlight, turning about its foot and leaning in and out as it goes.
#[derive(Component)]
struct Searchlight {
    heading: f32,
    turn: f32,
    tilt: f32,
    swing: f32,
    swing_rate: f32,
}

impl Searchlight {
    /// How the beam stands `seconds` into the lobby: leaning `tilt` from straight up, give or take
    /// its swing, toward a heading that turns.
    fn rotation(&self, seconds: f32) -> Quat {
        let heading = self.heading + self.turn * seconds;
        let lean = self.tilt + self.swing * (self.swing_rate * seconds + self.heading).sin();
        Quat::from_rotation_y(heading) * Quat::from_rotation_x(lean)
    }
}

/// One group of warning lights, blinking `phase` of a period after the first.
#[derive(Component)]
struct Beacons {
    phase: f32,
}

/// Spawns everything that moves around `track`, and the haze under it, each entity carrying `scene`
/// so it goes when the track does.
pub fn spawn(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    track: &Track,
    landmarks: &Landmarks,
    scene: impl Bundle + Clone,
) {
    let extent = Extent::of(track);
    // The skyline: the highest of what stands beside the road, and of the road itself.
    let skyline = track
        .points()
        .iter()
        .map(|point| point.height)
        .fold(landmarks.highest, f32::max);
    spawn_traffic(commands, meshes, materials, &extent, skyline, &scene);
    spawn_searchlights(commands, meshes, materials, landmarks, &scene);
    spawn_beacons(commands, meshes, materials, landmarks, &scene);
    commands.spawn((
        scene,
        Mesh3d(meshes.add(haze(&extent, &landmarks.towers).into_mesh())),
        MeshMaterial3d(materials.add(StandardMaterial {
            unlit: true,
            alpha_mode: AlphaMode::Add,
            cull_mode: None,
            ..default()
        })),
        NotShadowCaster,
    ));
}

/// How far a circuit reaches on the plane: its middle, and the distance from there to the furthest
/// edge of its road.
struct Extent {
    middle: Vec2,
    reach: f32,
}

impl Extent {
    fn of(track: &Track) -> Self {
        let points = track.points();
        let (low, high) = points.iter().fold(
            (Vec2::splat(f32::INFINITY), Vec2::splat(f32::NEG_INFINITY)),
            |(low, high), point| (low.min(point.position), high.max(point.position)),
        );
        let middle = (low + high) / 2.0;
        let reach = points
            .iter()
            .map(|point| point.position.distance(middle))
            .fold(0.0, f32::max)
            + track.half_width();
        Self { middle, reach }
    }
}

fn spawn_traffic(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    extent: &Extent,
    skyline: f32,
    scene: &(impl Bundle + Clone),
) {
    let material = materials.add(StandardMaterial {
        unlit: true,
        ..default()
    });
    let mut dice = Dice::new(TRAFFIC_SEED);
    for corridor in 0..CORRIDORS {
        for lane in corridor_lanes(&mut dice, corridor, extent, skyline) {
            commands.spawn((
                scene.clone(),
                Mesh3d(meshes.add(lane.mesh.into_mesh())),
                MeshMaterial3d(material.clone()),
                NotShadowCaster,
                Transform::default(),
                Traffic {
                    direction: lane.direction,
                    speed: lane.speed,
                },
            ));
        }
    }
}

/// A lane of traffic, built where it stands before it starts flowing.
struct Lane {
    mesh: Geometry,
    /// The world direction it flows in, level.
    direction: Vec3,
    speed: f32,
}

/// The two lanes of corridor number `corridor`: outside the city on a side of its own, square to
/// the way out from its middle, flying opposite ways. A straight line that far from the middle
/// stays that far all along it, so a lane never comes into the city however far it runs.
fn corridor_lanes(dice: &mut Dice, corridor: usize, extent: &Extent, highest: f32) -> [Lane; 2] {
    // The sides spread all the way round, so the city is ringed with traffic.
    let out = Vec2::from_angle((corridor as f32 + dice.range(-0.3, 0.3)) * TAU / CORRIDORS as f32);
    let along = out.perp();
    let middle =
        extent.middle + out * (extent.reach + dice.range(LANE_OUTSIDE.0, LANE_OUTSIDE.1));
    let altitude = highest + dice.range(LANE_ALTITUDE.0, LANE_ALTITUDE.1);
    let speed = dice.range(TRAFFIC_SPEED.0, TRAFFIC_SPEED.1);
    let length = extent.reach + LANE_REACH;
    [1.0_f32, -1.0].map(|way| {
        let direction = along * way;
        Lane {
            mesh: lane(
                dice,
                middle + out * way * LANE_GAP / 2.0,
                direction,
                altitude + way * LANE_STEP / 2.0,
                length,
            ),
            direction: world::direction(direction),
            speed: speed * dice.range(0.9, 1.1),
        }
    })
}

/// The vehicles of a lane through `middle`, flying along `direction` at `altitude`, from `length`
/// meters behind the middle to as far ahead: a pattern rolled once and repeated down the lane.
fn lane(dice: &mut Dice, middle: Vec2, direction: Vec2, altitude: f32, length: f32) -> Geometry {
    let mut pattern = Vec::new();
    let mut along = dice.range(0.0, VEHICLE_GAP.0);
    while along < TRAFFIC_PERIOD - VEHICLE_GAP.0 / 2.0 {
        // A little off the lane's line, up or down, so a stream is not a ruled line.
        pattern.push((along, dice.range(-1.0, 1.0)));
        along += dice.range(VEHICLE_GAP.0, VEHICLE_GAP.1);
    }
    let periods = (length / TRAFFIC_PERIOD).ceil() as i32;
    let forward = world::direction(direction);
    let mut geometry = Geometry::default();
    // One period more behind, which the lane slides into view as it flows.
    for period in -periods - 1..=periods {
        for (along, rise) in &pattern {
            let distance = period as f32 * TRAFFIC_PERIOD + along;
            let center = world::position(middle + direction * distance, altitude + rise);
            vehicle(&mut geometry, center, forward);
        }
    }
    geometry
}

/// A vehicle at `center`, flying along `forward`: a dark hull, a white light at its nose and a red
/// one at its tail, each standing out of the hull's end so it shows from every side.
fn vehicle(geometry: &mut Geometry, center: Vec3, forward: Vec3) {
    let across = forward.cross(Vec3::Y).normalize_or(Vec3::Z);
    let half = VEHICLE / 2.0;
    let ring = |up: f32| {
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(along, side)| {
            center + forward * along * half.x + across * side * half.z + Vec3::Y * up * half.y
        })
    };
    geometry.solid(ring(-1.0), ring(1.0), HULL, HULL);
    for (end, light) in [(1.0, HEADLIGHT), (-1.0, TAIL_LIGHT)] {
        let middle = center + forward * end * (half.x + LIGHT * 0.3);
        cube(geometry, middle - Vec3::Y * LIGHT / 2.0, LIGHT, light);
    }
}

fn spawn_searchlights(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    landmarks: &Landmarks,
    scene: &(impl Bundle + Clone),
) {
    let mesh = meshes.add(beam().into_mesh());
    // Added to what is behind it, and seen from inside as well as from outside.
    let material = materials.add(StandardMaterial {
        unlit: true,
        alpha_mode: AlphaMode::Add,
        cull_mode: None,
        ..default()
    });
    let mut dice = Dice::new(SEARCHLIGHT_SEED);
    for (index, foot) in landmarks.searchlights.iter().enumerate() {
        // Neighbors turn opposite ways, so the beams of one stand cross and part again.
        let way = if index % 2 == 0 { 1.0 } else { -1.0 };
        let light = Searchlight {
            heading: dice.range(0.0, TAU),
            turn: way * dice.range(BEAM_TURN.0, BEAM_TURN.1),
            tilt: dice.range(BEAM_TILT.0, BEAM_TILT.1),
            swing: dice.range(BEAM_TILT_SWING.0, BEAM_TILT_SWING.1),
            swing_rate: dice.range(0.2, 0.45),
        };
        commands.spawn((
            scene.clone(),
            Mesh3d(mesh.clone()),
            MeshMaterial3d(material.clone()),
            NotShadowCaster,
            Transform::from_translation(*foot).with_rotation(light.rotation(0.0)),
            light,
        ));
    }
}

/// A searchlight's beam, standing straight up from its foot: cones open at both ends, one inside the
/// other, their light fading out along them.
fn beam() -> Geometry {
    let mut geometry = Geometry::default();
    for (foot, end, alpha) in BEAM_CONES {
        let point = |share: f32, side: usize| {
            let angle = TAU * side as f32 / BEAM_SIDES as f32;
            let radius = foot + (end - foot) * share;
            Vec3::new(angle.cos() * radius, share * BEAM_LENGTH, angle.sin() * radius)
        };
        let light = |share: f32| BEAM_COLOR.with_alpha(alpha * (1.0 - share).powf(1.6));
        for rings in BEAM_RINGS.windows(2) {
            let (near, far) = (rings[0], rings[1]);
            for side in 0..BEAM_SIDES {
                let corners = [
                    point(near, side),
                    point(near, side + 1),
                    point(far, side + 1),
                    point(far, side),
                ];
                let outward = (corners[0] + corners[1]).with_y(0.0).normalize_or(Vec3::X);
                geometry.quad_shaded(
                    corners,
                    outward,
                    [light(near), light(near), light(far), light(far)],
                );
            }
        }
    }
    geometry
}

fn spawn_beacons(
    commands: &mut Commands,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    landmarks: &Landmarks,
    scene: &(impl Bundle + Clone),
) {
    for group in 0..BEACON_GROUPS {
        let mut geometry = Geometry::default();
        for top in landmarks.beacons.iter().skip(group).step_by(BEACON_GROUPS) {
            cube(&mut geometry, *top, BEACON_SIZE, Color::WHITE);
        }
        if geometry.triangles() == 0 {
            continue;
        }
        // The red is the material's, so the whole group blinks by changing that one color.
        let material = materials.add(StandardMaterial {
            base_color: BEACON_RED.into(),
            unlit: true,
            ..default()
        });
        commands.spawn((
            scene.clone(),
            Mesh3d(meshes.add(geometry.into_mesh())),
            MeshMaterial3d(material),
            NotShadowCaster,
            Beacons {
                phase: group as f32 / BEACON_GROUPS as f32,
            },
        ));
    }
}

/// A cube `size` meters across, standing on `foot`.
fn cube(geometry: &mut Geometry, foot: Vec3, size: f32, color: Color) {
    let half = size / 2.0;
    let middle = foot + Vec3::Y * half;
    let corner = |x: f32, y: f32, z: f32| middle + Vec3::new(x, y, z) * half;
    for axis in 0..3 {
        for sign in [-1.0_f32, 1.0] {
            // The face square to `axis`, on the `sign` side.
            let face = |a: f32, b: f32| match axis {
                0 => corner(sign, a, b),
                1 => corner(a, sign, b),
                _ => corner(a, b, sign),
            };
            geometry.quad_facing_away(
                [face(-1.0, -1.0), face(1.0, -1.0), face(1.0, 1.0), face(-1.0, 1.0)],
                middle,
                color,
            );
        }
    }
}

/// The haze of the city far below: a disc of faint light under the whole circuit, warmer and
/// brighter under the towers, fading out toward its rim.
fn haze(extent: &Extent, towers: &[Vec2]) -> Geometry {
    let radius = extent.reach + HAZE_REACH;
    let at = |ring: usize, sector: usize| {
        let share = ring as f32 / HAZE_RINGS as f32;
        extent.middle + Vec2::from_angle(TAU * sector as f32 / HAZE_SECTORS as f32) * radius * share
    };
    let light = |position: Vec2| -> Color {
        let rim = (1.0 - position.distance(extent.middle) / radius).clamp(0.0, 1.0);
        let lit: f32 = towers
            .iter()
            .map(|tower| (-tower.distance_squared(position) / (HAZE_SPREAD * HAZE_SPREAD)).exp())
            .sum();
        // Many towers together light the streets no more than fully.
        let streets = 1.0 - (-0.6 * lit).exp();
        let fade = rim * rim * (3.0 - 2.0 * rim);
        let channel = |base: f32, warm: f32| (base + warm * streets) * fade;
        Color::linear_rgb(
            channel(HAZE_BASE.red, HAZE_STREETS.red),
            channel(HAZE_BASE.green, HAZE_STREETS.green),
            channel(HAZE_BASE.blue, HAZE_STREETS.blue),
        )
    };
    let mut geometry = Geometry::default();
    for ring in 0..HAZE_RINGS {
        for sector in 0..HAZE_SECTORS {
            let corners = [
                at(ring, sector),
                at(ring, sector + 1),
                at(ring + 1, sector + 1),
                at(ring + 1, sector),
            ];
            geometry.quad_shaded(
                corners.map(|corner| world::position(corner, HAZE_DEPTH)),
                Vec3::Y,
                corners.map(light),
            );
        }
    }
    geometry
}

/// Slides every lane of traffic along its direction, by up to one pattern's length, so the vehicles
/// seem to fly on forever.
fn flow_traffic(time: Res<Time>, mut lanes: Query<(&Traffic, &mut Transform)>) {
    let seconds = time.elapsed_secs_f64();
    for (lane, mut transform) in &mut lanes {
        let slid = (seconds * f64::from(lane.speed)).rem_euclid(f64::from(TRAFFIC_PERIOD));
        transform.translation = lane.direction * slid as f32;
    }
}

fn sweep_searchlights(time: Res<Time>, mut lights: Query<(&Searchlight, &mut Transform)>) {
    let seconds = time.elapsed_secs();
    for (light, mut transform) in &mut lights {
        transform.rotation = light.rotation(seconds);
    }
}

/// Each group of warning lights flashes once a period, briefly and softly, a share of a period
/// after the one before it.
fn blink_beacons(
    time: Res<Time>,
    beacons: Query<(&Beacons, &MeshMaterial3d<StandardMaterial>)>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let periods = time.elapsed_secs_f64() / BEACON_PERIOD;
    for (group, material) in &beacons {
        let phase = (periods.fract() as f32 + group.phase).fract();
        let flash = (phase * TAU).sin().max(0.0).powi(3);
        let shine = BEACON_DIM + (1.0 - BEACON_DIM) * flash;
        if let Some(mut material) = materials.get_mut(&material.0) {
            material.base_color = Color::linear_rgb(
                BEACON_RED.red * shine,
                BEACON_RED.green * shine,
                BEACON_RED.blue * shine,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use space_race_sim::track::{Elevation, Segment, TrackDescription};

    use super::*;

    fn oval() -> Track {
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
            elevation: vec![Elevation {
                at: 0.0,
                height: 20.0,
            }],
            narrows: Vec::new(),
            scenery: Vec::new(),
        })
        .unwrap()
    }

    /// Every lane keeps outside the city, past where its districts reach, at about the height of
    /// its highest roof; and they ring it rather than all flying past one side.
    #[test]
    fn the_traffic_rings_the_city_from_outside() {
        let extent = Extent::of(&oval());
        let highest = 90.0;
        let mut dice = Dice::new(TRAFFIC_SEED);
        let mut sides = Vec::new();
        for corridor in 0..CORRIDORS {
            for lane in corridor_lanes(&mut dice, corridor, &extent, highest) {
                let mut nearest = Vec2::ZERO;
                for position in lane.mesh.positions() {
                    let on_plane = Vec2::new(position.x, -position.z) - extent.middle;
                    assert!(
                        on_plane.length() > extent.reach + 130.0,
                        "a vehicle {} m from the middle",
                        on_plane.length()
                    );
                    assert!((highest - 20.0..highest + 45.0).contains(&position.y));
                    if nearest == Vec2::ZERO || on_plane.length() < nearest.length() {
                        nearest = on_plane;
                    }
                }
                sides.push(nearest.normalize());
            }
        }
        // Some lane on every side of the city.
        for heading in [Vec2::X, Vec2::Y, Vec2::NEG_X, Vec2::NEG_Y] {
            assert!(sides.iter().any(|side| side.dot(heading) > 0.5), "{heading}");
        }
    }

    /// A lane slid by one pattern's length looks exactly as it did: that is what lets it flow on
    /// forever. Every vehicle lands where another one was, but at the far ends.
    #[test]
    fn a_lane_slid_by_its_pattern_is_the_same_lane() {
        let mut dice = Dice::new(3);
        let lane = lane(&mut dice, Vec2::ZERO, Vec2::X, 100.0, 1000.0);
        let positions: Vec<Vec3> = lane.positions().collect();
        let slid = Vec3::X * TRAFFIC_PERIOD;
        let inside = |position: &Vec3| position.x.abs() < 800.0;
        for position in positions.iter().filter(|position| inside(position)) {
            let moved = *position + slid;
            assert!(
                positions.iter().any(|other| other.distance(moved) < 1e-2),
                "nothing at {moved}"
            );
        }
    }

    /// A beam is brightest at its foot and fades to nothing at its end.
    #[test]
    fn a_beam_fades_out_along_its_length() {
        let beam = beam();
        for (position, color) in beam.positions().zip(beam.colors()) {
            if position.y > BEAM_LENGTH - 1.0 {
                assert_eq!(color[3], 0.0, "{position}");
            }
            if position.y < 1.0 {
                assert!(color[3] > 0.04, "{position}");
            }
        }
    }

    /// The haze lies under everything a circuit floats over, footings included, gives no light at
    /// its rim, and more where the towers stand.
    #[test]
    fn the_haze_lies_under_everything_and_fades_at_its_rim() {
        let extent = Extent::of(&oval());
        let lit = haze(&extent, &[extent.middle]);
        assert!(lit.positions().all(|position| position.y < -40.0));
        let radius = extent.reach + HAZE_REACH;
        for (position, color) in lit.positions().zip(lit.colors()) {
            let distance = Vec2::new(position.x, -position.z).distance(extent.middle);
            if distance > radius - 1.0 {
                assert!(color[..3].iter().all(|channel| *channel < 1e-4), "{color:?}");
            }
        }
        // The first corner of all is the middle of the disc, right over the tower.
        let unlit = haze(&extent, &[]);
        let middle = |haze: &Geometry| haze.colors().next().unwrap()[0];
        assert!(middle(&lit) > middle(&unlit) * 2.0);
    }

    #[test]
    fn a_searchlight_leans_from_straight_up_and_turns() {
        let light = Searchlight {
            heading: 0.3,
            turn: 0.2,
            tilt: 0.4,
            swing: 0.1,
            swing_rate: 0.3,
        };
        for seconds in [0.0, 5.0, 40.0] {
            let up = light.rotation(seconds) * Vec3::Y;
            let lean = up.angle_between(Vec3::Y);
            assert!((0.29..=0.51).contains(&lean), "{lean}");
        }
        let first = light.rotation(0.0) * Vec3::Y;
        let later = light.rotation(5.0) * Vec3::Y;
        assert!(first.with_y(0.0).angle_between(later.with_y(0.0)) > 0.5);
    }
}
