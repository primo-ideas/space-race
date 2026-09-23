//! The scenery of a circuit: the props the track file stands beside the road.
//!
//! They are built into the same two meshes as the track itself, so a decorated circuit still costs
//! two draw calls, and they follow the same language: dark geometric bodies that take the light,
//! lined with neon that does not. Nothing here touches the simulation, and every prop is placed
//! from the road rather than in world coordinates (see
//! [`scenery`](space_race_sim::track::scenery)).
//!
//! Each prop is built in a [`Frame`] of its own, standing on the ground at the foot of the wall:
//! `x` runs along the road, `y` up, and `z` away from the road, so the face at `z = 0` is the one
//! that looks at the driver, and the same numbers build the prop on either side of the road.

use bevy::prelude::*;
use space_race_sim::track::Track;
use space_race_sim::track::scenery::{Prop, Side};

use crate::geometry::{Frame, Geometry};
use crate::track::{NEON_BLUE, WALL_HEIGHT, WALL_THICKNESS};
use crate::world;

/// Every structure is this dark gray, a little bluer than the walls: enough to catch the light, not
/// enough to pull the eye from the road.
const STRUCTURE: Color = Color::srgb(0.12, 0.12, 0.15);

/// Where the neon rings sit up a pylon, as a share of its height, and how much of its base width is
/// left at the top.
const PYLON_RINGS: [f32; 3] = [0.3, 0.55, 0.8];
const PYLON_TAPER: f32 = 0.4;

/// How far the legs of a gantry stand out from the wall, and how deep its beam is, in meters.
const GANTRY_LEG: f32 = 1.4;
const GANTRY_BEAM: f32 = 1.4;

/// One row of a grandstand: how deep its step is and how high it rises, in meters.
const ROW_DEPTH: f32 = 1.7;
const ROW_RISE: f32 = 0.95;
/// A grandstand is swept in sections about this long, so it follows the curve of the road.
const GRANDSTAND_SECTION: f32 = 3.0;

/// How thick the neon frame of a billboard is, and how far apart its stripes stand, in meters.
const BILLBOARD_FRAME: f32 = 0.3;
const BILLBOARD_STRIPE_SPACING: f32 = 2.2;

/// One arrow of a marker board: how tall it is, how thick its bars are, and how wide the board is,
/// in meters.
const CHEVRON_HEIGHT: f32 = 0.85;
const CHEVRON_BAR: f32 = 0.22;
const CHEVRON_WIDTH: f32 = 2.6;

/// Builds every prop of a track into the meshes the track itself is built into.
pub fn build(lit: &mut Geometry, neon: &mut Geometry, track: &Track, scenery: &[Prop]) {
    for prop in scenery {
        match *prop {
            Prop::Gantry {
                at,
                clearance,
                depth,
            } => gantry(lit, neon, track, at, clearance, depth),
            Prop::Pylon {
                at,
                side,
                offset,
                height,
            } => pylon(lit, neon, &beside_road(track, at, side, offset), height),
            Prop::Grandstand {
                at,
                side,
                offset,
                length,
                rows,
            } => grandstand(lit, neon, track, at, side, offset, length, rows),
            Prop::Billboard {
                at,
                side,
                offset,
                width,
                height,
            } => billboard(
                lit,
                neon,
                &beside_road(track, at, side, offset),
                width,
                height,
            ),
            Prop::Chevrons {
                at,
                side,
                offset,
                count,
            } => chevrons(lit, neon, track, at, side, offset, count),
            Prop::Monolith {
                at,
                side,
                offset,
                height,
                width,
            } => monolith(
                lit,
                neon,
                &beside_road(track, at, side, offset),
                height,
                width,
            ),
        }
    }
}

/// The frame of a prop standing beside the road: on the ground at the foot of the wall, `offset`
/// meters further out, with `z` running away from the road.
fn beside_road(track: &Track, at: f32, side: Side, offset: f32) -> Frame {
    let point = track.point_at(at);
    let away = point.direction.perp() * side.sign();
    // The wall is where the road is here: a prop beside a bottleneck stands in with it.
    let foot = point.position + away * (point.half_width + WALL_THICKNESS + offset);
    Frame::new(
        world::position(foot, 0.0),
        world::direction(point.direction),
        world::direction(away),
    )
}

/// An arch over the road. Its frame stands on the centerline, so `z` is the lateral offset, to the
/// left as everywhere else, and the two legs are the same shape mirrored.
fn gantry(
    lit: &mut Geometry,
    neon: &mut Geometry,
    track: &Track,
    at: f32,
    clearance: f32,
    depth: f32,
) {
    let point = track.point_at(at);
    let frame = Frame::new(
        world::position(point.position, 0.0),
        world::direction(point.direction),
        world::direction(point.direction.perp()),
    );

    let half_width = point.half_width;
    let edge = half_width + WALL_THICKNESS;
    let span = edge + GANTRY_LEG;
    let half_depth = depth / 2.0;
    // The beam clears the higher wall: on a banked turn, the outer one stands well above the road.
    let walls = f32::max(
        track.height_beside(&point, half_width),
        track.height_beside(&point, -half_width),
    ) + WALL_HEIGHT;
    let beam = walls + clearance;

    for side in [1.0_f32, -1.0] {
        let (near, far) = (side * edge, side * span);
        lit.block(
            &frame,
            Vec3::new(-half_depth, 0.0, near.min(far)),
            Vec3::new(half_depth, beam + GANTRY_BEAM, near.max(far)),
            STRUCTURE,
        );
        // A band of light around each leg, at the height a driver reads road signs at.
        neon.block(
            &frame,
            Vec3::new(-half_depth - 0.06, 2.2, near.min(far) - 0.06),
            Vec3::new(half_depth + 0.06, 3.0, near.max(far) + 0.06),
            NEON_BLUE,
        );
    }

    lit.block(
        &frame,
        Vec3::new(-half_depth, beam, -span),
        Vec3::new(half_depth, beam + GANTRY_BEAM, span),
        STRUCTURE,
    );
    // Seen from the road the beam is a bar of light: one strip underneath, one across the face the
    // cars come at.
    neon.block(
        &frame,
        Vec3::new(-half_depth * 0.7, beam - 0.3, -span),
        Vec3::new(half_depth * 0.7, beam, span),
        NEON_BLUE,
    );
    neon.block(
        &frame,
        Vec3::new(-half_depth - 0.08, beam + 0.35, -span),
        Vec3::new(-half_depth + 0.1, beam + GANTRY_BEAM - 0.35, span),
        NEON_BLUE,
    );
}

/// A tapered mast ringed with neon, and a light on top.
fn pylon(lit: &mut Geometry, neon: &mut Geometry, frame: &Frame, height: f32) {
    let half = (height * 0.045).clamp(0.4, 1.2);
    lit.tapered_block(
        frame,
        Vec3::new(-half, 0.0, 0.0),
        Vec3::new(half, height, 2.0 * half),
        PYLON_TAPER,
        STRUCTURE,
    );
    for share in PYLON_RINGS {
        // The mast narrows evenly as it rises, so its rings narrow with it.
        let ring = half * (1.0 - (1.0 - PYLON_TAPER) * share) * 1.3;
        let base = height * share;
        neon.block(
            frame,
            Vec3::new(-ring, base, half - ring),
            Vec3::new(ring, base + 0.25, half + ring),
            NEON_BLUE,
        );
    }
    let cap = half * PYLON_TAPER;
    neon.block(
        frame,
        Vec3::new(-cap, height, half - cap),
        Vec3::new(cap, height + 0.5, half + cap),
        NEON_BLUE,
    );
}

/// A stepped tribune, swept along the road so it curves with it, a neon line along every row.
fn grandstand(
    lit: &mut Geometry,
    neon: &mut Geometry,
    track: &Track,
    at: f32,
    side: Side,
    offset: f32,
    length: f32,
    rows: u8,
) {
    let sections = (length / GRANDSTAND_SECTION).ceil().max(1.0) as usize;
    let frames: Vec<Frame> = (0..=sections)
        .map(|section| {
            let along = length * section as f32 / sections as f32;
            beside_road(track, at + along, side, offset)
        })
        .collect();

    let back = rows as f32 * ROW_DEPTH;
    let top = rows as f32 * ROW_RISE;
    // The front of the bottom row, then every riser and tread, then the back down to the ground.
    let mut profile = vec![Vec2::ZERO];
    for row in 0..rows {
        let front = row as f32 * ROW_DEPTH;
        let height = (row + 1) as f32 * ROW_RISE;
        profile.push(Vec2::new(front, height));
        profile.push(Vec2::new(front + ROW_DEPTH, height));
    }
    profile.push(Vec2::new(back, 0.0));
    lit.sweep(
        &frames,
        &profile,
        Vec2::new(back / 2.0, top / 4.0),
        STRUCTURE,
    );

    for row in 0..rows {
        let front = row as f32 * ROW_DEPTH;
        let height = (row + 1) as f32 * ROW_RISE + 0.03;
        neon.sweep(
            &frames,
            &[Vec2::new(front, height), Vec2::new(front + 0.45, height)],
            Vec2::new(front + 0.2, height - 1.0),
            NEON_BLUE,
        );
    }

    // The two ends, each a staircase closed off by one rectangle per row.
    for (frame, inward) in [(frames.first(), 1.0_f32), (frames.last(), -1.0)] {
        let Some(frame) = frame else {
            continue;
        };
        for row in 0..rows {
            let (front, behind) = (row as f32 * ROW_DEPTH, (row + 1) as f32 * ROW_DEPTH);
            let height = (row + 1) as f32 * ROW_RISE;
            let corners = [
                (front, 0.0),
                (behind, 0.0),
                (behind, height),
                (front, height),
            ]
            .map(|(depth, rise)| frame.cross_section(Vec2::new(depth, rise)));
            let inside = frame.point(Vec3::new(
                inward * GRANDSTAND_SECTION / 2.0,
                height / 2.0,
                (front + behind) / 2.0,
            ));
            lit.quad_facing_away(corners, inside, STRUCTURE);
        }
    }
}

/// A panel on two posts, its neon frame and stripes facing the road.
fn billboard(lit: &mut Geometry, neon: &mut Geometry, frame: &Frame, width: f32, height: f32) {
    let half = width / 2.0;
    let panel = height * 0.45;
    for side in [-1.0_f32, 1.0] {
        let post = side * (half - (half * 0.2).clamp(0.5, 1.5));
        lit.block(
            frame,
            Vec3::new(post - 0.3, 0.0, 0.35),
            Vec3::new(post + 0.3, panel + 0.4, 0.95),
            STRUCTURE,
        );
    }
    lit.block(
        frame,
        Vec3::new(-half, panel, 0.3),
        Vec3::new(half, height, 0.6),
        STRUCTURE,
    );

    // The frame around the panel, then stripes leaning the way the cars run.
    let edge = BILLBOARD_FRAME;
    for (from, to) in [
        (
            Vec3::new(-half, panel, 0.15),
            Vec3::new(half, panel + edge, 0.32),
        ),
        (
            Vec3::new(-half, height - edge, 0.15),
            Vec3::new(half, height, 0.32),
        ),
        (
            Vec3::new(-half, panel, 0.15),
            Vec3::new(-half + edge, height, 0.32),
        ),
        (
            Vec3::new(half - edge, panel, 0.15),
            Vec3::new(half, height, 0.32),
        ),
    ] {
        neon.block(frame, from, to, NEON_BLUE);
    }

    let (bottom, top) = (panel + edge * 1.6, height - edge * 1.6);
    let margin = half - edge * 1.6;
    if top > bottom && margin > 0.0 {
        let lean = (top - bottom).min(2.0 * margin);
        let stripes = (((2.0 * margin - lean) / BILLBOARD_STRIPE_SPACING) as usize + 1).min(5);
        for stripe in 0..stripes {
            let start = -margin + stripe as f32 * BILLBOARD_STRIPE_SPACING;
            neon.bar(
                frame,
                Vec3::new(start, bottom, 0.2),
                Vec3::new(start + lean, top, 0.2),
                edge * 0.8,
                NEON_BLUE,
            );
        }
    }
}

/// A marker board of stacked arrows. Unlike the props beside it, the board faces back down the
/// road, at the cars coming, so its arrows point across it: the way the road bends.
fn chevrons(
    lit: &mut Geometry,
    neon: &mut Geometry,
    track: &Track,
    at: f32,
    side: Side,
    offset: f32,
    count: u8,
) {
    let frame = beside_road(track, at, side, offset);
    // The bend is read from the road; on the prop's own side, the driver's left is `side.sign()`
    // times the frame's `z`.
    let arrow = bend_at(track, at) * side.sign();
    let base = 1.1;
    let top = base + count as f32 * CHEVRON_HEIGHT;

    for post in [0.45, CHEVRON_WIDTH - 0.45] {
        lit.block(
            &frame,
            Vec3::new(-0.15, 0.0, post - 0.15),
            Vec3::new(0.15, base + 0.3, post + 0.15),
            STRUCTURE,
        );
    }
    lit.block(
        &frame,
        Vec3::new(-0.15, base, 0.0),
        Vec3::new(0.15, top, CHEVRON_WIDTH),
        STRUCTURE,
    );

    let (tip, tail) = if arrow >= 0.0 {
        (CHEVRON_WIDTH - 0.3, 0.3)
    } else {
        (0.3, CHEVRON_WIDTH - 0.3)
    };
    for index in 0..count {
        let middle = base + (index as f32 + 0.5) * CHEVRON_HEIGHT;
        let reach = CHEVRON_HEIGHT * 0.35;
        for rise in [-reach, reach] {
            neon.bar(
                &frame,
                Vec3::new(-0.22, middle + rise, tail),
                Vec3::new(-0.22, middle, tip),
                CHEVRON_BAR,
                NEON_BLUE,
            );
        }
    }
}

/// A tall slab far out in the dark, seamed with neon and lit on top.
fn monolith(lit: &mut Geometry, neon: &mut Geometry, frame: &Frame, height: f32, width: f32) {
    let half = width / 2.0;
    let depth = width * 0.35;
    lit.tapered_block(
        frame,
        Vec3::new(-half, 0.0, 0.0),
        Vec3::new(half, height, depth),
        0.82,
        STRUCTURE,
    );
    neon.bar(
        frame,
        Vec3::new(0.0, height * 0.06, 0.0),
        Vec3::new(0.0, height * 0.94, 0.0),
        width * 0.06,
        NEON_BLUE,
    );
    let cap = half * 0.4;
    neon.block(
        frame,
        Vec3::new(-cap, height, depth / 2.0 - cap),
        Vec3::new(cap, height + 0.6, depth / 2.0 + cap),
        NEON_BLUE,
    );
}

/// Which way the road bends where a marker board stands: `1` to the left, `-1` to the right, `0`
/// where it runs straight. Read from the centerline either side of the board, so an arrow points
/// into the turn it marks whatever the file says.
fn bend_at(track: &Track, at: f32) -> f32 {
    /// How far either side of the board the bend is read, in meters.
    const REACH: f32 = 12.0;

    let before = track.point_at(at - REACH).direction;
    let after = track.point_at(at + REACH).direction;
    let turn = before.perp_dot(after);
    if turn.abs() < 0.02 {
        0.0
    } else {
        turn.signum()
    }
}

#[cfg(test)]
mod tests {
    use space_race_sim::track::{Segment, TrackDescription};

    use super::*;

    /// A counter-clockwise oval, 16 m wide, its turns banked: the straights read as straight and
    /// the turns bend to the left.
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
            narrows: Vec::new(),
            scenery: Vec::new(),
        })
        .unwrap()
    }

    /// Every kind of prop, on both sides and on a straight as well as in a turn.
    fn every_prop() -> Vec<Prop> {
        vec![
            Prop::Gantry {
                at: 0.0,
                clearance: 7.0,
                depth: 2.0,
            },
            Prop::Pylon {
                at: 30.0,
                side: Side::Left,
                offset: 2.0,
                height: 16.0,
            },
            Prop::Grandstand {
                at: 60.0,
                side: Side::Right,
                offset: 5.0,
                length: 40.0,
                rows: 6,
            },
            Prop::Billboard {
                at: 200.0,
                side: Side::Left,
                offset: 4.0,
                width: 10.0,
                height: 5.0,
            },
            Prop::Chevrons {
                at: 80.0,
                side: Side::Right,
                offset: 1.0,
                count: 3,
            },
            Prop::Monolith {
                at: 150.0,
                side: Side::Left,
                offset: 70.0,
                height: 32.0,
                width: 12.0,
            },
        ]
    }

    fn build_prop(track: &Track, prop: Prop) -> (Geometry, Geometry) {
        let (mut lit, mut neon) = (Geometry::default(), Geometry::default());
        build(&mut lit, &mut neon, track, &[prop]);
        (lit, neon)
    }

    #[test]
    fn every_prop_builds_a_body_and_neon_on_it() {
        let track = oval();
        for prop in every_prop() {
            let (lit, neon) = build_prop(&track, prop);
            assert!(lit.triangles() > 0, "{prop:?} has no body");
            assert!(neon.triangles() > 0, "{prop:?} has no neon");
        }
    }

    #[test]
    fn no_prop_stands_over_the_road() {
        let track = oval();
        for prop in every_prop() {
            let (lit, neon) = build_prop(&track, prop);
            for position in lit.positions().chain(neon.positions()) {
                // Back to the simulation plane, where the road is measured.
                let on_plane = Vec2::new(position.x, -position.z);
                let lateral = track.project(on_plane).lateral.abs();
                let clear = lateral > track.half_width()
                    // A gantry spans the road, well over the walls.
                    || position.y > WALL_HEIGHT + 2.0;
                assert!(clear, "{prop:?} reaches {lateral:.2} m from the centerline");
            }
        }
    }

    #[test]
    fn props_stand_where_the_file_places_them() {
        let track = oval();
        for side in [Side::Left, Side::Right] {
            let (lit, _) = build_prop(
                &track,
                Prop::Pylon {
                    at: 25.0,
                    side,
                    offset: 2.0,
                    height: 12.0,
                },
            );
            for position in lit.positions() {
                let projection = track.project(Vec2::new(position.x, -position.z));
                assert!((projection.distance - 25.0).abs() < 3.0, "{projection:?}");
                assert_eq!(projection.lateral.signum(), side.sign(), "{projection:?}");
            }
        }
    }

    #[test]
    fn a_marker_board_reads_the_bend_from_the_road() {
        let track = oval();
        // The oval turns left, and its straights are straight.
        assert_eq!(bend_at(&track, 25.0), 0.0);
        assert_eq!(bend_at(&track, 110.0), 1.0);
        assert_eq!(bend_at(&track, 250.0), 0.0);
    }

    #[test]
    fn arrows_point_into_the_turn_from_either_side() {
        let track = oval();
        // The oval turns left. A board on the right of the road points at the road, one on the
        // left points away from it: either way, the way the road goes.
        for (side, tip_toward_road) in [(Side::Right, true), (Side::Left, false)] {
            let at = 110.0;
            let (_, neon) = build_prop(
                &track,
                Prop::Chevrons {
                    at,
                    side,
                    offset: 1.0,
                    count: 3,
                },
            );
            // Where the arrows meet in a point the neon stands at one height; where they open, at
            // two. That tells the tip end of the board from the tail end.
            let frame = beside_road(&track, at, side, 1.0);
            let spread = |edge: f32| {
                let heights = neon
                    .positions()
                    .map(|position| frame.local(position))
                    .filter(|local| (local.z - edge).abs() < 0.6)
                    .map(|local| local.y);
                let (low, high) = heights.fold((f32::INFINITY, f32::NEG_INFINITY), |bounds, y| {
                    (bounds.0.min(y), bounds.1.max(y))
                });
                high - low
            };
            let (tip, tail) = if tip_toward_road {
                (0.3, CHEVRON_WIDTH - 0.3)
            } else {
                (CHEVRON_WIDTH - 0.3, 0.3)
            };
            assert!(
                spread(tip) < spread(tail),
                "{side:?}: {} at the tip, {} at the tail",
                spread(tip),
                spread(tail)
            );
        }
    }
}
