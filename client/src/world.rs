//! From the simulation plane to Bevy's 3D world.
//!
//! The simulation works on a plane with `x` and `y` and angles counter-clockwise from `+x` (see
//! `docs/simulation.md`). Bevy is `y`-up and right-handed, so the plane becomes the ground with
//! simulation `y` along `-z`. Seen from above, left turns stay left turns.

use bevy::math::{Mat3, Quat, Vec2, Vec3};

/// A point of the simulation plane, `height` meters above the ground.
pub fn position(point: Vec2, height: f32) -> Vec3 {
    Vec3::new(point.x, height, -point.y)
}

/// A direction of the simulation plane, on the ground.
pub fn direction(direction: Vec2) -> Vec3 {
    Vec3::new(direction.x, 0.0, -direction.y)
}

/// Rotation of a car with a simulation heading, resting on a surface with `gradient` (see
/// `Surface::gradient`): `+x` points along the heading, following the slope, and `+y` along the
/// surface normal. On flat ground it is a rotation about `+y` by the heading, which takes `+x` to
/// `(cos, 0, -sin)`, the world direction of that heading.
pub fn rotation_on_surface(heading: f32, gradient: Vec2) -> Quat {
    let up = normal(gradient);
    let heading = Vec2::from_angle(heading);
    // Along the heading, the surface rises by `gradient · heading` per meter.
    let forward = Vec3::new(heading.x, gradient.dot(heading), -heading.y);
    let forward = (forward - up * forward.dot(up)).normalize();
    Quat::from_mat3(&Mat3::from_cols(forward, up, forward.cross(up)))
}

/// Normal of a surface with `gradient`, pointing up. The height `h(x, y)` becomes `h(x, -z)` in the
/// world, so its normal `(-dh/dx, 1, -dh/dz)` is `(-gradient.x, 1, gradient.y)`.
pub fn normal(gradient: Vec2) -> Vec3 {
    Vec3::new(-gradient.x, 1.0, gradient.y).normalize()
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    #[test]
    fn rotation_matches_position_mapping() {
        for heading in [0.0, 0.7, FRAC_PI_2, -2.5] {
            let expected = direction(Vec2::from_angle(heading));
            let rotated = rotation_on_surface(heading, Vec2::ZERO) * Vec3::X;
            assert!(rotated.distance(expected) < 1e-5, "{heading}");
        }
    }

    #[test]
    fn left_stays_left_seen_from_above() {
        // Simulation left of +x is +y; in the world, left of +x seen from above (+y up) is -z.
        assert_eq!(position(Vec2::Y, 0.0), Vec3::NEG_Z);
    }

    #[test]
    fn a_flat_surface_gives_the_plain_heading_rotation() {
        for heading in [0.0, 1.2, -2.8] {
            let flat = rotation_on_surface(heading, Vec2::ZERO);
            assert!(
                flat.angle_between(Quat::from_rotation_y(heading)) < 1e-4,
                "{heading}"
            );
        }
    }

    #[test]
    fn a_car_on_a_slope_leans_with_it() {
        // Heading +x on a road rising to its left (+y): the car's right side, local +z, dips.
        let gradient = Vec2::new(0.0, 0.5);
        let rotation = rotation_on_surface(0.0, gradient);
        assert!((rotation * Vec3::Y).distance(normal(gradient)) < 1e-5);
        assert!((rotation * Vec3::Z).y < -0.3, "{}", rotation * Vec3::Z);
        assert!((rotation * Vec3::X).distance(Vec3::X) < 1e-5);
    }

    #[test]
    fn the_normal_leans_away_from_the_rise() {
        // Rising toward simulation +y, which is world -z: the normal leans toward +z.
        let normal = normal(Vec2::new(0.0, 1.0));
        assert!(normal.z > 0.5 && normal.y > 0.5, "{normal}");
    }
}
