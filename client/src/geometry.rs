//! Flat-shaded, vertex-colored triangles, and the frames things are built in.
//!
//! The track and its scenery are each drawn as a single mesh, one for the surfaces that take light
//! and one for the neon that glows on its own, so a whole circuit costs two draw calls. This module
//! holds the buffers they are built into, and the few shapes everything is made of: quads, boxes,
//! bars running any which way, and profiles swept along the road.
//!
//! A [`Frame`] is a local set of coordinates on the ground: `x` runs along the road in the driving
//! direction, `y` up, and `z` away from the road. Every prop is written in its own frame, so the
//! same numbers build it on either side of the road and around any curve.

use bevy::asset::RenderAssetUsages;
use bevy::mesh::{Indices, PrimitiveTopology};
use bevy::prelude::*;

/// Where something is built: an origin on the ground, and the directions its own `x` and `z` run
/// in. `y` is always up.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    origin: Vec3,
    along: Vec3,
    out: Vec3,
}

impl Frame {
    /// `along` and `out` must be unit horizontal directions: along the road, and away from it.
    pub fn new(origin: Vec3, along: Vec3, out: Vec3) -> Self {
        Self { origin, along, out }
    }

    /// The world position of a point written in this frame.
    pub fn point(&self, local: Vec3) -> Vec3 {
        self.origin + self.along * local.x + Vec3::Y * local.y + self.out * local.z
    }

    /// The world position of a point of the frame's cross-section: `profile` is `(z, y)`, away from
    /// the road and up.
    pub fn cross_section(&self, profile: Vec2) -> Vec3 {
        self.point(Vec3::new(0.0, profile.y, profile.x))
    }

    /// A world position written back in this frame, the other way round from [`Frame::point`].
    #[cfg(test)]
    pub fn local(&self, world: Vec3) -> Vec3 {
        let offset = world - self.origin;
        Vec3::new(offset.dot(self.along), offset.y, offset.dot(self.out))
    }
}

/// Flat-shaded, vertex-colored triangles.
#[derive(Default)]
pub struct Geometry {
    positions: Vec<[f32; 3]>,
    normals: Vec<[f32; 3]>,
    colors: Vec<[f32; 4]>,
    indices: Vec<u32>,
}

impl Geometry {
    /// A quad from four corners in order around it, facing `normal`. The winding is derived from
    /// the normal, so callers cannot get it wrong.
    pub fn quad(&mut self, corners: [Vec3; 4], normal: Vec3, color: Color) {
        let base = self.positions.len() as u32;
        let color = color.to_linear().to_f32_array();
        for corner in corners {
            self.positions.push(corner.to_array());
            self.normals.push(normal.to_array());
            self.colors.push(color);
        }
        let winding = (corners[1] - corners[0]).cross(corners[2] - corners[0]);
        let order = if winding.dot(normal) >= 0.0 {
            [0, 1, 2, 0, 2, 3]
        } else {
            [0, 2, 1, 0, 3, 2]
        };
        self.indices.extend(order.map(|index| base + index));
    }

    /// A quad of a surface seen from above, such as the road: its normal comes from the corners
    /// themselves, so it follows the slope of banked turns.
    pub fn quad_facing_up(&mut self, corners: [Vec3; 4], color: Color) {
        let across = (corners[2] - corners[0]).cross(corners[3] - corners[1]);
        let normal = if across.y < 0.0 { -across } else { across };
        self.quad(corners, normal.normalize_or(Vec3::Y), color);
    }

    /// A quad facing away from `inside`, a point on its hidden side: the way every face of a solid
    /// is turned, whatever order its corners come in.
    pub fn quad_facing_away(&mut self, corners: [Vec3; 4], inside: Vec3, color: Color) {
        let across = (corners[2] - corners[0]).cross(corners[3] - corners[1]);
        let outward = corners[0] - inside;
        let normal = if across.dot(outward) < 0.0 {
            -across
        } else {
            across
        };
        self.quad(corners, normal.normalize_or(Vec3::Y), color);
    }

    /// A box between two opposite corners written in `frame`.
    pub fn block(&mut self, frame: &Frame, from: Vec3, to: Vec3, color: Color) {
        self.tapered_block(frame, from, to, 1.0, color);
    }

    /// A box whose top face is its bottom one scaled by `top_scale` about the middle: a mast or a
    /// slab that narrows as it rises.
    pub fn tapered_block(
        &mut self,
        frame: &Frame,
        from: Vec3,
        to: Vec3,
        top_scale: f32,
        color: Color,
    ) {
        let half = (to - from).abs() / 2.0;
        let middle = (from + to) / 2.0;
        let ring = |scale: f32, y: f32| {
            [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]].map(|[x, z]| {
                frame.point(Vec3::new(
                    middle.x + x * half.x * scale,
                    y,
                    middle.z + z * half.z * scale,
                ))
            })
        };
        let (low, high) = (from.y.min(to.y), from.y.max(to.y));
        self.solid(ring(1.0, low), ring(top_scale, high), color);
    }

    /// A square bar of `thickness` between two points of `frame`, running any which way: the
    /// diagonal of a chevron as easily as the upright of a post.
    pub fn bar(&mut self, frame: &Frame, from: Vec3, to: Vec3, thickness: f32, color: Color) {
        let Some(axis) = (to - from).try_normalize() else {
            return;
        };
        // Any direction across the bar will do; only a bar running straight up needs another one.
        let across = if axis.y.abs() < 0.9 { Vec3::Y } else { Vec3::Z };
        let side = axis.cross(across).normalize();
        let up = side.cross(axis);
        let half = thickness / 2.0;
        let ring = |end: Vec3| {
            [[-1.0, -1.0], [1.0, -1.0], [1.0, 1.0], [-1.0, 1.0]]
                .map(|[s, u]| frame.point(end + side * (s * half) + up * (u * half)))
        };
        self.solid(ring(from), ring(to), color);
    }

    /// Sweeps a `profile`, written in the `(z, y)` cross-section of each frame, along a run of
    /// frames: the way a grandstand follows the curve of the road, or a neon line runs along it.
    ///
    /// `inside` is a point of the cross-section on the hidden side of the profile, which turns the
    /// surface the right way out: the middle of the solid the profile draws, or a point below a
    /// ribbon meant to be seen from above.
    pub fn sweep(&mut self, frames: &[Frame], profile: &[Vec2], inside: Vec2, color: Color) {
        for pair in frames.windows(2) {
            let (near, far) = (&pair[0], &pair[1]);
            for step in profile.windows(2) {
                self.quad_facing_away(
                    [
                        near.cross_section(step[0]),
                        near.cross_section(step[1]),
                        far.cross_section(step[1]),
                        far.cross_section(step[0]),
                    ],
                    near.cross_section(inside),
                    color,
                );
            }
        }
    }

    /// The six faces of a box, from its bottom and top rings of corners, each given in the same
    /// order around the box.
    fn solid(&mut self, bottom: [Vec3; 4], top: [Vec3; 4], color: Color) {
        let middle = (bottom.iter().chain(&top).copied().sum::<Vec3>()) / 8.0;
        self.quad_facing_away(bottom, middle, color);
        self.quad_facing_away(top, middle, color);
        for corner in 0..4 {
            let next = (corner + 1) % 4;
            self.quad_facing_away(
                [bottom[corner], bottom[next], top[next], top[corner]],
                middle,
                color,
            );
        }
    }

    /// How many triangles have been built so far.
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3
    }

    /// Every vertex built so far, for tests that check where a shape landed.
    #[cfg(test)]
    pub fn positions(&self) -> impl Iterator<Item = Vec3> {
        self.positions.iter().map(|position| Vec3::from(*position))
    }

    pub fn into_mesh(self) -> Mesh {
        Mesh::new(
            PrimitiveTopology::TriangleList,
            RenderAssetUsages::RENDER_WORLD,
        )
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, self.positions)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, self.normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_COLOR, self.colors)
        .with_inserted_indices(Indices::U32(self.indices))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const COLOR: Color = Color::WHITE;

    /// A frame beside a road running along `+x`, with the prop side toward `+z`.
    fn frame() -> Frame {
        Frame::new(Vec3::ZERO, Vec3::X, Vec3::Z)
    }

    #[test]
    fn quads_face_their_normal_whatever_the_corner_order() {
        let square = [Vec3::ZERO, Vec3::X, Vec3::X + Vec3::NEG_Z, Vec3::NEG_Z];
        for corners in [square, [square[3], square[2], square[1], square[0]]] {
            let mut geometry = Geometry::default();
            geometry.quad(corners, Vec3::Y, COLOR);
            let [a, b, c] =
                [0, 1, 2].map(|i| Vec3::from(geometry.positions[geometry.indices[i] as usize]));
            // Counter-clockwise seen from the normal side is Bevy's front face.
            assert!((b - a).cross(c - a).dot(Vec3::Y) > 0.0);
        }
    }

    #[test]
    fn sloped_quads_face_up_along_their_slope() {
        // Rising toward -z: the surface faces up and a little toward +z, away from the rise.
        let slope = [
            Vec3::ZERO,
            Vec3::X,
            Vec3::new(1.0, 1.0, -2.0),
            Vec3::new(0.0, 1.0, -2.0),
        ];
        for corners in [slope, [slope[3], slope[2], slope[1], slope[0]]] {
            let mut geometry = Geometry::default();
            geometry.quad_facing_up(corners, COLOR);
            let normal = Vec3::from(geometry.normals[0]);
            assert!(normal.y > 0.8 && normal.z > 0.3, "{normal}");
            assert!((normal.length() - 1.0).abs() < 1e-5);
        }
    }

    #[test]
    fn a_block_stands_where_its_frame_puts_it() {
        let frame = Frame::new(Vec3::new(10.0, 0.0, 4.0), Vec3::NEG_Z, Vec3::NEG_X);
        let mut geometry = Geometry::default();
        geometry.block(
            &frame,
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(1.0, 6.0, 2.0),
            COLOR,
        );

        // The frame's x runs along -z and its z along -x, so the block sits there, not along the
        // world axes.
        let positions: Vec<Vec3> = geometry.positions().collect();
        let min = positions.iter().copied().reduce(Vec3::min).unwrap();
        let max = positions.iter().copied().reduce(Vec3::max).unwrap();
        assert!(min.abs_diff_eq(Vec3::new(8.0, 0.0, 3.0), 1e-5), "{min}");
        assert!(max.abs_diff_eq(Vec3::new(10.0, 6.0, 5.0), 1e-5), "{max}");
        assert_eq!(geometry.triangles(), 12);
    }

    #[test]
    fn every_face_of_a_solid_faces_outward() {
        let mut geometry = Geometry::default();
        geometry.tapered_block(
            &frame(),
            Vec3::new(-1.0, 0.0, 0.0),
            Vec3::new(1.0, 8.0, 2.0),
            0.4,
            COLOR,
        );
        let middle = Vec3::new(0.0, 4.0, 1.0);
        for face in 0..6 {
            let corner = Vec3::from(geometry.positions[face * 4]);
            let normal = Vec3::from(geometry.normals[face * 4]);
            assert!((corner - middle).dot(normal) > 0.0, "face {face}");
        }
    }

    #[test]
    fn a_bar_runs_from_one_point_to_the_other() {
        let mut geometry = Geometry::default();
        let (from, to) = (Vec3::new(0.0, 1.0, 0.0), Vec3::new(3.0, 4.0, 0.0));
        geometry.bar(&frame(), from, to, 0.4, COLOR);

        // Its corners surround both ends, within half a thickness.
        for end in [from, to] {
            let closest = geometry
                .positions()
                .map(|position| position.distance(end))
                .fold(f32::INFINITY, f32::min);
            assert!(closest < 0.4, "{end}: {closest}");
        }
        assert_eq!(geometry.triangles(), 12);
    }

    #[test]
    fn a_sweep_follows_its_frames_and_faces_away_from_the_inside() {
        // Two frames a meter apart, the second turned a quarter turn: the surface follows.
        let frames = [
            Frame::new(Vec3::ZERO, Vec3::X, Vec3::Z),
            Frame::new(Vec3::X, Vec3::Z, Vec3::NEG_X),
        ];
        let mut geometry = Geometry::default();
        geometry.sweep(
            &frames,
            &[Vec2::new(0.0, 0.0), Vec2::new(0.0, 2.0)],
            Vec2::new(1.0, 1.0),
            COLOR,
        );
        let positions: Vec<Vec3> = geometry.positions().collect();
        assert!(
            positions.contains(&Vec3::new(0.0, 2.0, 0.0)),
            "{positions:?}"
        );
        assert!(
            positions.contains(&Vec3::new(1.0, 2.0, 0.0)),
            "{positions:?}"
        );
        // The inside lies toward +z of the first frame, so the face looks the other way.
        assert!(Vec3::from(geometry.normals[0]).z < 0.0);
    }
}
