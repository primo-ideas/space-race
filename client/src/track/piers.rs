//! What holds the road up: a pier under it every few dozen meters, standing in the dark the whole
//! circuit floats over, and a line of light along the underside of its slab.
//!
//! The road used to float on nothing. Seen from another part of the lap -- the launch from the
//! ridge, the climb from the sink -- it read as a ribbon hung in the air, with nothing to say how
//! high it was. A pier is a hammerhead: a head as wide as a third of the road under the slab, its
//! top following the road's underside however the road banks and climbs there, on a column that
//! reaches down to the ground nobody sees and fades to black on the way, as every footing does.
//! The piers stand under the road, inside its walls, where nothing else stands: the road never runs
//! over itself, and the scenery keeps to the outside of the walls.

use bevy::prelude::*;
use space_race_sim::track::{Track, TrackPoint};

use super::{SLAB, WALL_THICKNESS, for_each_segment};
use crate::geometry::Geometry;
use crate::scenery::{ABYSS, DARK};
use crate::world;

/// A pier every this many meters along the lap, the first this far past the start line.
const PIER_SPACING: f32 = 46.0;
const PIER_FIRST: f32 = 23.0;
/// The head of a pier: how far it reaches across either side of the centerline, how long it is
/// along the road, and how far it drops under the slab, in meters.
const HEAD_REACH: f32 = 5.5;
const HEAD_LENGTH: f32 = 3.0;
const HEAD_DROP: f32 = 3.5;
/// The column under it: how far it reaches across either side, and how long it is along the road.
const COLUMN_REACH: f32 = 1.7;
const COLUMN_LENGTH: f32 = 2.2;
/// How far under the slab's underside the head stands, so the two never draw over each other.
const HEAD_GAP: f32 = 0.04;
/// The concrete of the piers: the walls' dark gray, a little bluer.
const PIER: Color = Color::srgb(0.11, 0.11, 0.135);
/// The band of light round a column, just under its head, and how tall it is, in meters.
const BAND_BELOW: f32 = 1.4;
const BAND_HEIGHT: f32 = 0.45;
const BAND: Color = Color::srgb(0.03, 0.42, 0.8);

/// The line of light under the slab: how far in from the slab's edge, how wide, and how bright.
const KEEL_INSET: f32 = 0.35;
const KEEL_WIDTH: f32 = 0.3;
const KEEL: Color = Color::srgb(0.03, 0.32, 0.62);

/// A pier every [`PIER_SPACING`] meters round the lap.
pub(super) fn piers(lit: &mut Geometry, neon: &mut Geometry, track: &Track) {
    let count = ((track.length() - PIER_FIRST) / PIER_SPACING).floor() as usize + 1;
    for index in 0..count {
        pier(lit, neon, track, PIER_FIRST + index as f32 * PIER_SPACING);
    }
}

/// The pier `at` meters along the lap: its head, the column under it down to the dark, and the band
/// of light round the column.
fn pier(lit: &mut Geometry, neon: &mut Geometry, track: &Track, at: f32) {
    let point = track.point_at(at);
    let place = |along: f32, lateral: f32, height: f32| {
        world::position(
            point.position + point.direction * along + point.direction.perp() * lateral,
            height,
        )
    };
    // Around the head or the column, in order, as (along, across).
    let ring = |length: f32, reach: f32| {
        [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
            .map(|(along, across)| (along * length / 2.0, across * reach))
    };

    // The head's top meets the underside of the slab at each of its corners, so it follows the
    // road's banking and its climb.
    let underside = |along: f32, lateral: f32| {
        track.height_beside(&track.point_at(at + along), lateral) - SLAB - HEAD_GAP
    };
    let top = ring(HEAD_LENGTH, HEAD_REACH)
        .map(|(along, lateral)| place(along, lateral, underside(along, lateral)));
    let lowest = top.iter().map(|corner| corner.y).fold(f32::INFINITY, f32::min);
    let neck = lowest - HEAD_DROP;
    let column = |height: f32| {
        ring(COLUMN_LENGTH, COLUMN_REACH).map(|(along, lateral)| place(along, lateral, height))
    };
    lit.solid(column(neck), top, PIER, PIER);
    lit.solid(column(ABYSS), column(neck), DARK, PIER);

    let band = |height: f32| {
        ring(COLUMN_LENGTH + 0.16, COLUMN_REACH + 0.08)
            .map(|(along, lateral)| place(along, lateral, height))
    };
    let band_top = neck - BAND_BELOW;
    neon.solid(band(band_top - BAND_HEIGHT), band(band_top), BAND, BAND);
}

/// A line of light along the underside of the slab, near each of its edges, all the way round.
pub(super) fn keel_lines(neon: &mut Geometry, track: &Track) {
    let edge = |point: TrackPoint| point.half_width + WALL_THICKNESS - KEEL_INSET;
    for side in [1.0_f32, -1.0] {
        let at = |point: TrackPoint, lateral: f32| {
            world::position(
                point.position + point.direction.perp() * lateral,
                track.height_beside(&point, lateral) - SLAB - 0.03,
            )
        };
        for_each_segment(track, |a, b, _| {
            let outer = |point: TrackPoint| side * edge(point);
            let inner = |point: TrackPoint| side * (edge(point) - KEEL_WIDTH);
            let corners = [
                at(a, outer(a)),
                at(a, inner(a)),
                at(b, inner(b)),
                at(b, outer(b)),
            ];
            // Facing down, away from the slab over it.
            let above = corners.iter().sum::<Vec3>() / 4.0 + Vec3::Y;
            neon.quad_facing_away(corners, above, KEEL);
        });
    }
}

#[cfg(test)]
mod tests {
    use space_race_sim::track::{Elevation, Segment, TrackDescription};

    use super::*;

    /// A counter-clockwise oval, 16 m wide, its turns banked, lifted 25 m and climbing.
    fn oval() -> Track {
        let turn = Segment::Turn {
            angle: 180.0,
            radius: 40.0,
            transition: 20.0,
            banking: 30.0,
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
            elevation: vec![
                Elevation {
                    at: 0.0,
                    height: 25.0,
                },
                Elevation {
                    at: 200.0,
                    height: 40.0,
                },
            ],
            narrows: Vec::new(),
            scenery: Vec::new(),
        })
        .unwrap()
    }

    /// Every pier stands under the road and inside its walls, never pokes up through it, and
    /// reaches down to the dark, black at the bottom.
    #[test]
    fn piers_stand_under_the_road_down_to_the_dark() {
        let track = oval();
        let (mut lit, mut neon) = (Geometry::default(), Geometry::default());
        piers(&mut lit, &mut neon, &track);
        assert!(lit.triangles() > 0 && neon.triangles() > 0);
        for (position, color) in lit.positions().zip(lit.colors()) {
            let here = track.project(Vec2::new(position.x, -position.z));
            assert!(here.lateral.abs() < track.half_width(), "{here:?}");
            let road = track.surface_at(&here).height;
            assert!(position.y < road - SLAB + 0.3, "{} m under the road", road - position.y);
            if (position.y - ABYSS).abs() < 1e-3 {
                assert_eq!(&color[..3], &[0.0, 0.0, 0.0]);
            }
        }
        let lowest = lit.positions().map(|position| position.y).fold(0.0, f32::min);
        assert!((lowest - ABYSS).abs() < 1e-3, "{lowest}");
    }

    /// In the middle of a banked turn, a pier's head still meets the underside on both sides: its
    /// top follows the slope of the road rather than standing level under its low edge.
    #[test]
    fn a_pier_head_follows_a_banked_road() {
        let track = oval();
        let at = 50.0 + 20.0 + (std::f32::consts::PI * 40.0 - 20.0) / 2.0;
        let (mut lit, mut neon) = (Geometry::default(), Geometry::default());
        pier(&mut lit, &mut neon, &track, at);
        let point = track.point_at(at);
        for lateral in [-HEAD_REACH, HEAD_REACH] {
            let underside = track.height_beside(&point, lateral) - SLAB;
            let closest = lit
                .positions()
                .filter(|position| {
                    let here = track.project(Vec2::new(position.x, -position.z));
                    (here.lateral - lateral).abs() < 0.2
                })
                .map(|position| (underside - position.y).abs())
                .fold(f32::INFINITY, f32::min);
            assert!(closest < 0.4, "{closest} m from the underside at {lateral} m");
        }
    }

    #[test]
    fn the_keel_lines_run_under_the_slab() {
        let track = oval();
        let mut neon = Geometry::default();
        keel_lines(&mut neon, &track);
        for position in neon.positions() {
            let here = track.project(Vec2::new(position.x, -position.z));
            let point = track.point_at(here.distance);
            let underside = track.height_beside(&point, here.lateral) - SLAB;
            assert!(position.y < underside && position.y > underside - 0.1);
        }
    }
}
