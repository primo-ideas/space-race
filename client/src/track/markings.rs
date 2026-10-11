//! What is painted on the road, and the light its walls throw onto it.
//!
//! The road was a dark band with a dashed line down its middle: speed showed, but a turn did not
//! until the walls began to curve. Kerbs now line the inside of every turn, so a bend reads from
//! far off and its apex from close up; a solid line runs inside each edge the whole lap round; the
//! starting grid is drawn on the road, one box per slot; and every neon stripe of the walls lights
//! the road at its foot, a pool of blue fading out across the tarmac, as a lamp would. The paint is
//! lit like the road is; the pools of light are added to whatever they fall on, kerbs included.

use std::f32::consts::PI;

use bevy::prelude::*;
use space_race_sim::track::Track;

use super::{MARKING, MARKING_HEIGHT, NEON_BLUE, WALL_STRIPE, for_each_segment, on_road};
use crate::geometry::Geometry;

/// Kerbs line both edges of the road wherever its centerline turns more tightly than this, in
/// radians per meter: a radius of 110 m. Every turn of the skyway is tighter than that at its
/// apex, and the transitions bring the kerbs in partway along.
const KERB_CURVATURE: f32 = 1.0 / 110.0;
/// How far a kerb reaches in from the foot of the wall, and how long each of its blocks is, in
/// meters.
const KERB_WIDTH: f32 = 1.6;
const KERB_BLOCK: f32 = 2.5;
/// The two colors the blocks of a kerb alternate between: the magenta of a race on, and white.
const KERB_RED: Color = Color::srgb(0.74, 0.09, 0.3);
const KERB_WHITE: Color = Color::srgb(0.62, 0.62, 0.6);

/// The line inside each edge: how far in from the foot of the wall, how wide, and how bright, in
/// meters. Inside the kerbs, and duller than the markings across the road.
const EDGE_INSET: f32 = KERB_WIDTH + 0.6;
const EDGE_WIDTH: f32 = 0.22;
const EDGE: Color = Color::srgb(0.48, 0.48, 0.45);

/// The boxes of the starting grid: how many, how wide across the road, how far back their arms
/// reach, how thick their lines are, and how far ahead of a car's middle the front line stands, in
/// meters.
const GRID_BOXES: usize = 8;
const BOX_WIDTH: f32 = 4.4;
const BOX_ARMS: f32 = 2.0;
const BOX_LINE: f32 = 0.22;
const BOX_AHEAD: f32 = 2.4;

/// The light a neon stripe throws on the road at its foot: how far across the road it reaches, in
/// meters, and how much of the stripe's color it adds at the wall. It fades out across the road and
/// toward both ends of the stripe.
const POOL_REACH: f32 = 4.2;
const POOL_ALPHA: f32 = 0.45;
/// The pools lie over everything painted on the road, so the kerbs catch the light as well.
const POOL_HEIGHT: f32 = 4.0 * MARKING_HEIGHT;

/// Kerbs of alternating blocks along the inside edge of every turn, from the foot of the wall in.
///
/// The inside only: the skyway is turns almost all the way round, and kerbs on both edges were a
/// lap of magenta and white. On the inside they say which way the road goes and where its apex is.
pub(super) fn kerbs(lit: &mut Geometry, track: &Track) {
    let blocks = (track.length() / KERB_BLOCK) as usize;
    for block in 0..blocks {
        let start = block as f32 * KERB_BLOCK;
        let curvature = track.point_at(start + KERB_BLOCK / 2.0).curvature;
        if curvature.abs() < KERB_CURVATURE {
            continue;
        }
        // A turn to the left has its inside on the left, where lateral offsets are positive.
        let side = curvature.signum();
        let color = if block % 2 == 0 { KERB_RED } else { KERB_WHITE };
        // Two pieces a block, so a kerb follows the curve it lines.
        for piece in 0..2 {
            let a = track.point_at(start + piece as f32 * KERB_BLOCK / 2.0);
            let b = track.point_at(start + (piece + 1) as f32 * KERB_BLOCK / 2.0);
            let wall = |half_width: f32| side * (half_width - 0.02);
            let inner = |half_width: f32| side * (half_width - KERB_WIDTH);
            lit.quad_facing_up(
                [
                    on_road(track, a, wall(a.half_width), 1.5 * MARKING_HEIGHT),
                    on_road(track, a, inner(a.half_width), 1.5 * MARKING_HEIGHT),
                    on_road(track, b, inner(b.half_width), 1.5 * MARKING_HEIGHT),
                    on_road(track, b, wall(b.half_width), 1.5 * MARKING_HEIGHT),
                ],
                color,
            );
        }
    }
}

/// A solid line inside each edge, all the way round.
pub(super) fn edge_lines(lit: &mut Geometry, track: &Track) {
    for side in [1.0_f32, -1.0] {
        let outer = |half_width: f32| side * (half_width - EDGE_INSET);
        let inner = |half_width: f32| side * (half_width - EDGE_INSET - EDGE_WIDTH);
        for_each_segment(track, |a, b, _| {
            lit.quad_facing_up(
                [
                    on_road(track, a, outer(a.half_width), MARKING_HEIGHT),
                    on_road(track, a, inner(a.half_width), MARKING_HEIGHT),
                    on_road(track, b, inner(b.half_width), MARKING_HEIGHT),
                    on_road(track, b, outer(b.half_width), MARKING_HEIGHT),
                ],
                EDGE,
            );
        });
    }
}

/// The starting grid, one box for each of the first slots: a line across the front of where a car
/// stands, and two arms reaching back along its sides.
pub(super) fn grid_boxes(lit: &mut Geometry, track: &Track) {
    for slot in 0..GRID_BOXES {
        let (position, _) = track.grid_slot(slot);
        let here = track.project(position);
        let front = here.distance + BOX_AHEAD;
        let (right, left) = (
            here.lateral - BOX_WIDTH / 2.0,
            here.lateral + BOX_WIDTH / 2.0,
        );
        // Across the front, then each arm, as (back, front, right, left).
        let lines = [
            (front - BOX_LINE, front, right, left),
            (front - BOX_ARMS, front, right, right + BOX_LINE),
            (front - BOX_ARMS, front, left - BOX_LINE, left),
        ];
        for (back, ahead, from, to) in lines {
            let (a, b) = (track.point_at(back), track.point_at(ahead));
            lit.quad_facing_up(
                [
                    on_road(track, a, from, 1.5 * MARKING_HEIGHT),
                    on_road(track, a, to, 1.5 * MARKING_HEIGHT),
                    on_road(track, b, to, 1.5 * MARKING_HEIGHT),
                    on_road(track, b, from, 1.5 * MARKING_HEIGHT),
                ],
                MARKING,
            );
        }
    }
}

/// A pool of blue light on the road at the foot of every neon stripe of the walls, brightest at
/// the wall and in the middle of the stripe, fading out across the road and toward its ends.
pub(super) fn wall_light(glow: &mut Geometry, track: &Track) {
    // The walls' neon stripes are the odd ones, counted from the start line.
    let neon = |distance: f32| (distance / WALL_STRIPE) as usize % 2 == 1;
    let along = |distance: f32| ((distance / WALL_STRIPE).fract() * PI).sin();
    let spacing = track.spacing();
    for side in [1.0_f32, -1.0] {
        for_each_segment(track, |a, b, distance| {
            if !neon(distance) {
                return;
            }
            let [near, far] = [distance, distance + spacing].map(|at| {
                // Past the end of the stripe, the next one starts dark.
                let share = if neon(at) { along(at) } else { 0.0 };
                NEON_BLUE.with_alpha(POOL_ALPHA * share)
            });
            let dark = NEON_BLUE.with_alpha(0.0);
            glow.quad_shaded(
                [
                    on_road(track, a, side * a.half_width, POOL_HEIGHT),
                    on_road(track, a, side * (a.half_width - POOL_REACH), POOL_HEIGHT),
                    on_road(track, b, side * (b.half_width - POOL_REACH), POOL_HEIGHT),
                    on_road(track, b, side * b.half_width, POOL_HEIGHT),
                ],
                Vec3::Y,
                [near, dark, dark, far],
            );
        });
    }
}

#[cfg(test)]
mod tests {
    use space_race_sim::track::{Segment, TrackDescription};

    use super::*;

    /// A counter-clockwise oval, 16 m wide: two straights of 100 m, two turns of radius 40.
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
            elevation: Vec::new(),
            narrows: Vec::new(),
            scenery: Vec::new(),
        })
        .unwrap()
    }

    fn on_plane(position: Vec3) -> Vec2 {
        Vec2::new(position.x, -position.z)
    }

    /// Kerbs line the turns and nothing else, from the wall in, on the inside: the oval turns left,
    /// so every kerb is on its left.
    #[test]
    fn kerbs_line_the_inside_of_the_turns_only() {
        let track = oval();
        let mut lit = Geometry::default();
        kerbs(&mut lit, &track);
        assert!(lit.triangles() > 0);
        for position in lit.positions() {
            let here = track.project(on_plane(position));
            let curvature = track.point_at(here.distance).curvature;
            assert!(curvature > 0.0, "a kerb on a straight, {here:?}");
            assert!(here.lateral > 0.0, "a kerb on the outside, {here:?}");
            let from_wall = track.half_width_at(here.distance) - here.lateral;
            assert!((-0.05..=KERB_WIDTH + 0.05).contains(&from_wall), "{from_wall}");
        }
    }

    /// One box for each of the first grid slots, every one of them on the road and around its car.
    #[test]
    fn the_grid_is_drawn_around_its_slots() {
        let track = oval();
        let mut lit = Geometry::default();
        grid_boxes(&mut lit, &track);
        // Three lines a box, two triangles a line.
        assert_eq!(lit.triangles(), GRID_BOXES * 3 * 2);
        for slot in 0..GRID_BOXES {
            let (car, _) = track.grid_slot(slot);
            let near = lit
                .positions()
                .filter(|position| on_plane(*position).distance(car) < BOX_WIDTH)
                .count();
            assert!(near >= 12, "slot {slot}: {near} corners near the car");
        }
        for position in lit.positions() {
            let here = track.project(on_plane(position));
            assert!(here.lateral.abs() < track.half_width());
        }
    }

    /// The light of a stripe is gone by the time it reaches across the road, and only ever lies at
    /// the foot of a neon stripe.
    #[test]
    fn the_walls_light_fades_across_the_road() {
        let track = oval();
        let mut glow = Geometry::default();
        wall_light(&mut glow, &track);
        assert!(glow.triangles() > 0);
        for (position, color) in glow.positions().zip(glow.colors()) {
            let here = track.project(on_plane(position));
            let from_wall = track.half_width_at(here.distance) - here.lateral.abs();
            if from_wall > POOL_REACH - 0.05 {
                assert_eq!(color[3], 0.0, "light {from_wall} m from the wall");
            }
            assert!(color[3] <= POOL_ALPHA + 1e-6);
        }
    }
}
