//! Circuits: the description written in a file, and the geometry built from it.
//!
//! A description is a list of segments laid end to end, like a turtle drawing the centerline:
//! straights and turns, starting from the start line at the origin, heading `+x`. The built
//! [`Track`] samples that centerline into evenly spaced points, which is what the physics and the
//! renderer use.
//!
//! Turns can be banked. The road stays a height field over the plane: the simulation still moves
//! cars in two dimensions, and asks the [`Surface`] for the height and slope under them.

use std::error::Error;
use std::fmt;

use bitcode::{Decode, Encode};
use glam::{DVec2, Vec2};
use serde::Deserialize;

pub mod scenery;

use self::scenery::Prop;

/// Upper bound on the distance between two consecutive centerline points, in meters.
const MAX_POINT_SPACING: f64 = 1.0;

/// Integration steps between two centerline points. Clothoids have no closed form.
const SUBSTEPS: usize = 8;

/// How far the end of the centerline may land from its start. Smaller gaps come from integration
/// error or rounded numbers in the file, and are spread along the whole loop.
const CLOSURE_TOLERANCE: f64 = 0.5;

/// Turn angles must add up to one full turn within this many degrees.
const ANGLE_TOLERANCE: f32 = 0.01;

/// Starting grid, behind the start line: distance to the first slot, distance between slots, and
/// lateral offset of the slots as a share of the half width. Slots alternate left and right.
const GRID_FIRST_SLOT: f32 = 6.0;
const GRID_SLOT_SPACING: f32 = 4.0;
const GRID_LATERAL_SHARE: f32 = 0.45;

/// Steepest banking allowed, in degrees. Beyond it the outer edge rises faster than a car could
/// reasonably drive.
const MAX_BANKING: f32 = 60.0;

#[derive(Debug, Clone, PartialEq, Deserialize, Encode, Decode)]
#[serde(deny_unknown_fields)]
pub struct TrackDescription {
    pub name: String,
    /// Road width from wall to wall, in meters, where nothing narrows it.
    pub width: f32,
    /// The centerline, from the start line all the way back to it.
    pub segments: Vec<Segment>,
    /// Stretches where the road pinches in (see [`Narrows`]).
    #[serde(default)]
    pub narrows: Vec<Narrows>,
    /// Decoration standing beside the road, which the simulation never sees (see [`scenery`]).
    #[serde(default)]
    pub scenery: Vec<Prop>,
}

#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Encode, Decode)]
#[serde(deny_unknown_fields)]
pub enum Segment {
    Straight {
        length: f32,
    },
    /// Turns by `angle` degrees, positive to the left, along an arc of `radius` meters.
    ///
    /// With a `transition` (meters), the curvature ramps up linearly before the arc and back down
    /// after it, along clothoids. Steering into the turn is then progressive instead of sudden.
    /// The transitions take part of the angle, so the whole turn still turns by `angle`.
    ///
    /// With a `banking` (degrees), the road curves up toward the outside of the turn: flat at the
    /// inner edge, rising ever more steeply toward the outer edge, where its slope reaches
    /// `banking`. It rises along the entry transition and settles back along the exit one, so a
    /// banked turn needs a transition.
    Turn {
        angle: f32,
        radius: f32,
        #[serde(default)]
        transition: f32,
        #[serde(default)]
        banking: f32,
    },
}

/// A stretch where the road pinches in: the bottleneck of a circuit.
///
/// The centerline does not move. The road narrows evenly on both sides, from the full width of the
/// track down to [`Narrows::width`] and back, so the line through a bottleneck is the line the road
/// already took, with less room around it. That is what makes it a test: a drift that runs wide
/// finds a wall where a moment earlier there was tarmac.
///
/// A narrows is placed by its distance along the lap, the way a prop of the [`scenery`] is, rather
/// than on a segment: it keeps its place when the road around it is redrawn, and the editor to come
/// can drag one along the road.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Encode, Decode)]
#[serde(deny_unknown_fields)]
pub struct Narrows {
    /// Meters along the lap from the start line where the narrow stretch begins.
    pub at: f32,
    /// How far the road stays narrow, in meters.
    pub length: f32,
    /// Width from wall to wall along it, in meters.
    pub width: f32,
    /// How far the road takes to pinch in before the stretch, and to open out after it, in meters.
    #[serde(default = "narrows_blend")]
    pub blend: f32,
}

const fn narrows_blend() -> f32 {
    24.0
}

/// Narrower than this, in meters, and a bottleneck is a wall rather than a way through.
const MIN_NARROWS_WIDTH: f32 = 8.0;

impl Narrows {
    /// The width the road has `distance` meters along a lap `length` meters round, on a track
    /// `width` meters wide elsewhere.
    ///
    /// Within the stretch it is [`Narrows::width`]; on either side of it the walls move out over
    /// [`Narrows::blend`] meters, along the smoothstep the banking also uses, so the road has no
    /// crease where it starts pinching.
    fn width_at(&self, distance: f32, length: f32, width: f32) -> f32 {
        let along = (distance - self.at).rem_euclid(length);
        let share = if along <= self.length {
            0.0
        } else if along < self.length + self.blend {
            (along - self.length) / self.blend
        } else if along > length - self.blend {
            (length - along) / self.blend
        } else {
            1.0
        };
        let smooth = share * share * (3.0 - 2.0 * share);
        self.width + (width - self.width) * smooth
    }

    /// Checks a narrows against a track `length` meters round and `width` meters wide: it must lie
    /// on the lap, leave a way through, and have room for both its blends.
    fn validate(&self, length: f32, width: f32) -> Result<(), &'static str> {
        if !(self.at.is_finite() && (0.0..=length).contains(&self.at)) {
            return Err("the distance along the track must be between zero and its length");
        }
        if !(self.length.is_finite() && self.length > 0.0) {
            return Err("the length must be a positive number of meters");
        }
        if !(self.width.is_finite() && (MIN_NARROWS_WIDTH..=width).contains(&self.width)) {
            return Err(
                "the width must be at least 8 meters, and narrower than the track itself: \
                 a narrows pinches the road in, it never widens it",
            );
        }
        if !(self.blend.is_finite() && self.blend >= 0.0) {
            return Err("the blend must be zero or a positive number of meters");
        }
        if self.length + 2.0 * self.blend > length {
            return Err("the narrow stretch and its two blends are longer than the lap");
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrackError {
    InvalidWidth,
    NoSegments,
    InvalidSegment {
        index: usize,
        reason: &'static str,
    },
    NotALoop {
        total_angle: f32,
    },
    NotClosed {
        gap: f32,
    },
    /// A narrows cannot pinch the road as written.
    InvalidNarrows {
        index: usize,
        reason: &'static str,
    },
    /// A prop of the scenery cannot be placed or built as written.
    InvalidProp {
        index: usize,
        reason: &'static str,
    },
    /// The road runs over itself: at `at` meters from the start line, it is less than a road width
    /// from the road at `over` meters.
    Overlaps {
        at: f32,
        over: f32,
    },
}

impl fmt::Display for TrackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWidth => write!(f, "the width must be a positive number"),
            Self::NoSegments => write!(f, "the track has no segments"),
            Self::InvalidSegment { index, reason } => write!(f, "segment {index}: {reason}"),
            Self::InvalidNarrows { index, reason } => write!(f, "narrows {index}: {reason}"),
            Self::InvalidProp { index, reason } => write!(f, "prop {index}: {reason}"),
            Self::NotALoop { total_angle } => write!(
                f,
                "the turn angles add up to {total_angle} degrees instead of 360 or -360"
            ),
            Self::NotClosed { gap } => write!(
                f,
                "the centerline ends {gap:.2} m away from where it starts \
                 (tolerance: {CLOSURE_TOLERANCE} m)"
            ),
            Self::Overlaps { at, over } => write!(
                f,
                "the road at {at:.0} m from the start line runs over the road at {over:.0} m"
            ),
        }
    }
}

impl Error for TrackError {}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackPoint {
    pub position: Vec2,
    /// Unit tangent, in the driving direction.
    pub direction: Vec2,
    /// Slope of the road at its raised edge, as a ratio (`tan` of the banking angle). Positive when
    /// the left edge is the raised one, negative for the right edge, zero on flat road.
    pub bank: f32,
    /// Half the width of the road here, in meters: the track's own half width, pinched in by any
    /// [`Narrows`] (see [`Track::half_width_at`]).
    pub half_width: f32,
}

/// The road surface under a position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Surface {
    /// Height above the flat road, in meters.
    pub height: f32,
    /// How much the height grows per meter along `x` and along `y`: it points uphill.
    pub gradient: Vec2,
}

/// Where a position lies relative to the centerline.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrackProjection {
    /// Distance along the centerline from the start line, in `[0, length)`.
    pub distance: f32,
    /// Distance from the centerline, positive on the left.
    pub lateral: f32,
    /// Closest point of the centerline.
    pub closest: Vec2,
}

#[derive(Debug, Clone)]
pub struct Track {
    name: String,
    half_width: f32,
    narrowest_half_width: f32,
    length: f32,
    spacing: f32,
    /// A closed loop: the point after the last one is the first one.
    points: Vec<TrackPoint>,
}

impl Track {
    pub fn build(description: &TrackDescription) -> Result<Self, TrackError> {
        let profile = CurvatureProfile::new(description)?;
        let length = profile.length();
        for (index, narrows) in description.narrows.iter().enumerate() {
            narrows
                .validate(length as f32, description.width)
                .map_err(|reason| TrackError::InvalidNarrows { index, reason })?;
        }
        let count = (length / MAX_POINT_SPACING).ceil() as usize;
        let spacing = length / count as f64;
        let step = spacing / SUBSTEPS as f64;

        let mut positions = Vec::with_capacity(count);
        let mut headings = Vec::with_capacity(count);
        let mut position = DVec2::ZERO;
        let mut heading = 0.0;
        for index in 0..count {
            positions.push(position);
            headings.push(heading);
            for substep in 0..SUBSTEPS {
                let start = (index * SUBSTEPS + substep) as f64 * step;
                profile.advance(&mut position, &mut heading, start, start + step);
            }
        }

        let gap = position;
        if gap.length() > CLOSURE_TOLERANCE {
            return Err(TrackError::NotClosed {
                gap: gap.length() as f32,
            });
        }
        let points: Vec<TrackPoint> = positions
            .iter()
            .zip(&headings)
            .enumerate()
            .map(|(index, (position, heading))| {
                let correction = gap * (index as f64 / count as f64);
                let distance = index as f64 * spacing;
                TrackPoint {
                    position: (*position - correction).as_vec2(),
                    direction: Vec2::from_angle(*heading as f32),
                    bank: profile.bank_at(distance) as f32,
                    half_width: width_at(description, length as f32, distance as f32) / 2.0,
                }
            })
            .collect();
        let centerline: Vec<Vec2> = points.iter().map(|point| point.position).collect();
        if let Some((at, over)) = find_overlap(&centerline, spacing as f32, description.width) {
            return Err(TrackError::Overlaps { at, over });
        }
        for (index, prop) in description.scenery.iter().enumerate() {
            prop.validate(length as f32)
                .map_err(|reason| TrackError::InvalidProp { index, reason })?;
        }

        let narrowest_half_width = points
            .iter()
            .map(|point| point.half_width)
            .fold(f32::INFINITY, f32::min);

        Ok(Self {
            name: description.name.clone(),
            half_width: description.width / 2.0,
            narrowest_half_width,
            length: length as f32,
            spacing: spacing as f32,
            points,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    /// Half the width of the road where nothing narrows it: the widest it ever is.
    pub fn half_width(&self) -> f32 {
        self.half_width
    }

    /// Half the width of the road at its tightest bottleneck, which is what a car has to fit
    /// through to get round.
    pub fn narrowest_half_width(&self) -> f32 {
        self.narrowest_half_width
    }

    /// Half the width of the road `distance` meters from the start line. Any distance works, the
    /// loop wraps.
    pub fn half_width_at(&self, distance: f32) -> f32 {
        self.point_at(distance).half_width
    }

    /// Length of the centerline, in meters.
    pub fn length(&self) -> f32 {
        self.length
    }

    /// Evenly spaced centerline points; the first one is on the start line.
    pub fn points(&self) -> &[TrackPoint] {
        &self.points
    }

    /// Distance along the centerline between two consecutive [`Track::points`].
    pub fn spacing(&self) -> f32 {
        self.spacing
    }

    /// Centerline point at `distance` from the start line. Any distance works, the loop wraps.
    pub fn point_at(&self, distance: f32) -> TrackPoint {
        let position = distance.rem_euclid(self.length) / self.spacing;
        let index = (position as usize).min(self.points.len() - 1);
        let t = position - index as f32;
        let a = self.points[index];
        let b = self.points[(index + 1) % self.points.len()];
        TrackPoint {
            position: a.position.lerp(b.position, t),
            direction: a.direction.lerp(b.direction, t).normalize_or_zero(),
            bank: a.bank + (b.bank - a.bank) * t,
            half_width: a.half_width + (b.half_width - a.half_width) * t,
        }
    }

    /// Height of the road `lateral` meters beside a centerline point, positive on the left.
    ///
    /// The cross-section is a parabola: flat at the lower edge, and steepest at the raised edge,
    /// where the slope is `point.bank`. Beyond the edges it continues flat and at the edge height.
    pub fn height_beside(&self, point: &TrackPoint, lateral: f32) -> f32 {
        let (rise, share) = self.cross_section(point, lateral);
        rise * share * share
    }

    /// The surface under a position: its height and uphill direction.
    pub fn surface(&self, position: Vec2) -> Surface {
        self.surface_at(&self.project(position))
    }

    /// The surface at an already computed projection, to avoid projecting twice.
    pub fn surface_at(&self, projection: &TrackProjection) -> Surface {
        let point = self.point_at(projection.distance);
        let (rise, share) = self.cross_section(&point, projection.lateral);
        // d(rise × share²) / d(lateral), where share grows by 1 / (2 half widths) per meter toward
        // the raised edge.
        let slope = if (0.0..1.0).contains(&share) {
            rise * share * point.bank.signum() / point.half_width
        } else {
            0.0
        };
        Surface {
            height: rise * share * share,
            gradient: point.direction.perp() * slope,
        }
    }

    /// Height of the raised edge above the lower one, and how far `lateral` is from the lower edge
    /// toward the raised one, from 0 to 1.
    ///
    /// The cross-section is measured from the road as it is here: where a bottleneck pinches the
    /// walls in, the banking keeps its slope, so the raised edge stands lower than it would on the
    /// full width.
    fn cross_section(&self, point: &TrackPoint, lateral: f32) -> (f32, f32) {
        // With the parabola `rise × share²`, the slope at the raised edge is `2 rise / width`.
        let rise = point.half_width * point.bank.abs();
        let toward_raised = lateral * point.bank.signum() / point.half_width;
        (rise, ((toward_raised + 1.0) / 2.0).clamp(0.0, 1.0))
    }

    pub fn project(&self, position: Vec2) -> TrackProjection {
        let count = self.points.len();
        let segment = |index: usize| {
            (
                self.points[index].position,
                self.points[(index + 1) % count].position,
            )
        };
        let (index, t) = (0..count)
            .map(|index| {
                let (a, b) = segment(index);
                let t = segment_parameter(position, a, b);
                (index, t, position.distance_squared(a.lerp(b, t)))
            })
            .min_by(|x, y| x.2.total_cmp(&y.2))
            .map(|(index, t, _)| (index, t))
            .expect("a track always has points");

        let (a, b) = segment(index);
        let closest = a.lerp(b, t);
        let offset = position - closest;
        let side = if offset.dot((b - a).perp()) < 0.0 {
            -1.0
        } else {
            1.0
        };
        TrackProjection {
            distance: ((index as f32 + t) * self.spacing).rem_euclid(self.length),
            lateral: side * offset.length(),
            closest,
        }
    }

    /// Position and heading of a starting grid slot. Any slot index works, further ones are further
    /// back.
    pub fn grid_slot(&self, slot: usize) -> (Vec2, f32) {
        let point = self.point_at(-(GRID_FIRST_SLOT + slot as f32 * GRID_SLOT_SPACING));
        let side = if slot % 2 == 0 { 1.0 } else { -1.0 };
        let lateral = side * GRID_LATERAL_SHARE * point.half_width;
        (
            point.position + point.direction.perp() * lateral,
            point.direction.to_angle(),
        )
    }
}

/// The width of the road `distance` meters along a track `length` meters round: the width of the
/// track, pinched in by whichever narrows reaches furthest in here.
fn width_at(description: &TrackDescription, length: f32, distance: f32) -> f32 {
    description
        .narrows
        .iter()
        .fold(description.width, |width, narrows| {
            width.min(narrows.width_at(distance, length, description.width))
        })
}

/// Where a closed centerline of evenly spaced `positions` brings its road over itself: two points
/// less than a road width apart, although far enough apart along the loop that no allowed turn
/// brings them that close. Returns their distances along the loop.
///
/// The tightest turn allowed has a radius of half the width, and half a turn of it, `π × width / 2`
/// long, brings the centerline back exactly a width apart. Tighter spots cannot be turns.
fn find_overlap(positions: &[Vec2], spacing: f32, width: f32) -> Option<(f32, f32)> {
    let length = positions.len() as f32 * spacing;
    let far_along = std::f32::consts::FRAC_PI_2 * width;
    for (i, a) in positions.iter().enumerate() {
        for (j, b) in positions.iter().enumerate().skip(i + 1) {
            let along = (j - i) as f32 * spacing;
            if along.min(length - along) > far_along && a.distance(*b) < width {
                return Some((i as f32 * spacing, j as f32 * spacing));
            }
        }
    }
    None
}

/// Parameter in `[0, 1]` of the point of segment `ab` closest to `position`.
fn segment_parameter(position: Vec2, a: Vec2, b: Vec2) -> f32 {
    let ab = b - a;
    let length_squared = ab.length_squared();
    if length_squared == 0.0 {
        return 0.0;
    }
    ((position - a).dot(ab) / length_squared).clamp(0.0, 1.0)
}

/// The centerline as pieces of linearly varying curvature (1 / radius, positive to the left), each
/// with the banking it starts and ends with.
struct CurvatureProfile {
    pieces: Vec<Piece>,
    /// Distance at which each piece starts.
    starts: Vec<f64>,
}

#[derive(Clone, Copy)]
struct Piece {
    length: f64,
    start_curvature: f64,
    end_curvature: f64,
    /// Signed slope at the raised edge, as in [`TrackPoint::bank`].
    start_bank: f64,
    end_bank: f64,
}

impl CurvatureProfile {
    fn new(description: &TrackDescription) -> Result<Self, TrackError> {
        if !(description.width.is_finite() && description.width > 0.0) {
            return Err(TrackError::InvalidWidth);
        }
        if description.segments.is_empty() {
            return Err(TrackError::NoSegments);
        }

        let half_width = description.width / 2.0;
        let mut pieces = Vec::new();
        let mut total_angle = 0.0;
        for (index, segment) in description.segments.iter().enumerate() {
            let invalid = |reason| TrackError::InvalidSegment { index, reason };
            match *segment {
                Segment::Straight { length } => {
                    if !(length.is_finite() && length > 0.0) {
                        return Err(invalid("the length must be a positive number"));
                    }
                    pieces.push(Piece::flat(length.into(), 0.0, 0.0));
                }
                Segment::Turn {
                    angle,
                    radius,
                    transition,
                    banking,
                } => {
                    if !(angle.is_finite() && angle != 0.0) {
                        return Err(invalid("the angle must be a non-zero number"));
                    }
                    if !(radius.is_finite() && radius > half_width) {
                        return Err(invalid(
                            "the radius must be larger than half the track width, \
                             or the inner edge folds over itself",
                        ));
                    }
                    if !(transition.is_finite() && transition >= 0.0) {
                        return Err(invalid("the transition must be zero or a positive number"));
                    }
                    if !(banking.is_finite() && (0.0..=MAX_BANKING).contains(&banking)) {
                        return Err(invalid("the banking must be between 0 and 60 degrees"));
                    }
                    if banking > 0.0 && transition == 0.0 {
                        return Err(invalid(
                            "a banked turn needs a transition to rise into its banking, \
                             or the road would have a step",
                        ));
                    }
                    // Both transitions together cover the angle `transition / radius`, which leaves
                    // the arc exactly `transition` shorter than a plain turn.
                    let turn_length = f64::from(angle.abs()).to_radians() * f64::from(radius);
                    let arc_length = turn_length - f64::from(transition);
                    if arc_length < 0.0 {
                        return Err(invalid(
                            "the transitions are too long for this angle and radius",
                        ));
                    }

                    let curvature = f64::from(angle.signum()) / f64::from(radius);
                    // The outside of the turn is raised: the right edge in a left turn.
                    let bank = -f64::from(angle.signum()) * f64::from(banking).to_radians().tan();
                    if transition > 0.0 {
                        pieces.push(Piece {
                            length: transition.into(),
                            start_curvature: 0.0,
                            end_curvature: curvature,
                            start_bank: 0.0,
                            end_bank: bank,
                        });
                    }
                    if arc_length > 0.0 {
                        pieces.push(Piece::flat(arc_length, curvature, bank));
                    }
                    if transition > 0.0 {
                        pieces.push(Piece {
                            length: transition.into(),
                            start_curvature: curvature,
                            end_curvature: 0.0,
                            start_bank: bank,
                            end_bank: 0.0,
                        });
                    }
                    total_angle += angle;
                }
            }
        }

        if (total_angle.abs() - 360.0).abs() > ANGLE_TOLERANCE {
            return Err(TrackError::NotALoop { total_angle });
        }

        let starts = pieces
            .iter()
            .scan(0.0, |distance, piece| {
                let start = *distance;
                *distance += piece.length;
                Some(start)
            })
            .collect();
        Ok(Self { pieces, starts })
    }

    fn length(&self) -> f64 {
        self.starts.last().unwrap() + self.pieces.last().unwrap().length
    }

    /// Moves `position` and `heading` along the centerline from distance `from` to `to`.
    ///
    /// The interval is split where pieces join, because the curvature jumps or bends there. Within
    /// a piece the curvature is linear, so the heading is exact and only the position is
    /// approximated, by the heading half way.
    fn advance(&self, position: &mut DVec2, heading: &mut f64, from: f64, to: f64) {
        let mut start = from;
        while start < to {
            let index = self
                .starts
                .partition_point(|piece_start| *piece_start <= start)
                .saturating_sub(1);
            let piece_end = self.starts.get(index + 1).copied().unwrap_or(f64::INFINITY);
            let end = to.min(piece_end);
            if end <= start {
                break;
            }

            let length = end - start;
            let start_curvature = self.curvature_on(index, start);
            let end_curvature = self.curvature_on(index, end);
            let middle_heading = *heading + length / 8.0 * (3.0 * start_curvature + end_curvature);
            *position += DVec2::from_angle(middle_heading) * length;
            *heading += length / 2.0 * (start_curvature + end_curvature);
            start = end;
        }
    }

    fn curvature_on(&self, index: usize, distance: f64) -> f64 {
        let piece = self.pieces[index];
        let t = self.progress_on(index, distance);
        piece.start_curvature + (piece.end_curvature - piece.start_curvature) * t
    }

    /// Banking at `distance`. Within a piece it follows a smoothstep rather than a straight line,
    /// the cubic a Catmull-Rom or Hermite spline gives between two flat keys: the road starts and
    /// finishes rising gently, with no crease where the transitions meet the straight and the arc.
    fn bank_at(&self, distance: f64) -> f64 {
        let index = self
            .starts
            .partition_point(|start| *start <= distance)
            .saturating_sub(1);
        let piece = self.pieces[index];
        let t = self.progress_on(index, distance);
        let smooth = t * t * (3.0 - 2.0 * t);
        piece.start_bank + (piece.end_bank - piece.start_bank) * smooth
    }

    /// How far `distance` is into piece `index`, from 0 to 1.
    fn progress_on(&self, index: usize, distance: f64) -> f64 {
        ((distance - self.starts[index]) / self.pieces[index].length).clamp(0.0, 1.0)
    }
}

impl Piece {
    /// A piece of constant curvature and banking.
    fn flat(length: f64, curvature: f64, bank: f64) -> Self {
        Self {
            length,
            start_curvature: curvature,
            end_curvature: curvature,
            start_bank: bank,
            end_bank: bank,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::PI;

    use super::*;
    use crate::test_support::{banked_oval, oval};

    /// Distance to the middle of the first turn's arc, on [`banked_oval`] with a 100 m straight,
    /// radius 40 and 20 m transitions: half straight, entry transition, half arc.
    fn first_arc_middle() -> f32 {
        50.0 + 20.0 + (PI * 40.0 - 20.0) / 2.0
    }

    #[test]
    fn banked_turn_curves_up_toward_the_outside() {
        let track = Track::build(&banked_oval(100.0, 40.0, 20.0, 30.0)).unwrap();
        let point = track.point_at(first_arc_middle());
        let half_width = track.half_width();

        // A left turn raises its right edge.
        assert!(
            (point.bank + 30f32.to_radians().tan()).abs() < 1e-3,
            "{point:?}"
        );
        let inner = track.height_beside(&point, half_width);
        let middle = track.height_beside(&point, 0.0);
        let outer = track.height_beside(&point, -half_width);
        assert!(inner.abs() < 1e-4, "{inner}");
        // A parabola: a quarter of the rise half way, the rest over the outer half.
        let rise = half_width * 30f32.to_radians().tan();
        assert!((middle - rise / 4.0).abs() < 1e-3, "{middle}");
        assert!((outer - rise).abs() < 1e-3, "{outer}");

        // Uphill is toward the outside of the turn, and steepest near the outer edge.
        let right = -point.direction.perp();
        let near_outer = track.surface(point.position + right * (half_width - 0.5));
        let near_middle = track.surface(point.position);
        assert!(near_outer.gradient.dot(right) > near_middle.gradient.dot(right));
        assert!(near_middle.gradient.dot(right) > 0.0);
    }

    #[test]
    fn slope_matches_the_height_change() {
        let track = Track::build(&banked_oval(100.0, 40.0, 20.0, 30.0)).unwrap();
        let point = track.point_at(first_arc_middle());
        let right = -point.direction.perp();
        for lateral in [-6.0, -2.0, 0.0, 3.0, 7.0] {
            let position = point.position + point.direction.perp() * lateral;
            let step = 0.01;
            let numeric = (track.surface(position + right * step).height
                - track.surface(position - right * step).height)
                / (2.0 * step);
            let analytic = track.surface(position).gradient.dot(right);
            assert!(
                (numeric - analytic).abs() < 1e-2,
                "{lateral}: {numeric} {analytic}"
            );
        }
    }

    #[test]
    fn banking_rises_smoothly_along_the_transition_and_straights_stay_flat() {
        let track = Track::build(&banked_oval(100.0, 40.0, 20.0, 30.0)).unwrap();
        let full = 30f32.to_radians().tan();
        let bank = |distance: f32| track.point_at(distance).bank.abs();

        assert_eq!(bank(0.0), 0.0);
        assert!(bank(49.0) < 1e-6, "still on the straight");
        // Half way through the entry transition, the smoothstep is half way too.
        assert!((bank(60.0) - full / 2.0).abs() < 1e-2, "{}", bank(60.0));
        // It starts rising gently: after a tenth of the transition, far less than a tenth.
        assert!(bank(52.0) < 0.05 * full, "{}", bank(52.0));
        assert!((bank(first_arc_middle()) - full).abs() < 1e-4);
    }

    #[test]
    fn flat_tracks_have_a_flat_surface() {
        let track = Track::build(&oval(100.0, 40.0, 20.0)).unwrap();
        for distance in [0.0, 70.0, 150.0] {
            let point = track.point_at(distance);
            let surface = track.surface(point.position + point.direction.perp() * 5.0);
            assert_eq!(surface.height, 0.0);
            assert_eq!(surface.gradient, Vec2::ZERO);
        }
    }

    #[test]
    fn oval_closes_and_has_the_expected_length() {
        let track = Track::build(&oval(100.0, 40.0, 20.0)).unwrap();
        // Two 100 m straights and two half turns of radius 40. Each pair of 20 m transitions
        // shortens the arc by 20 m but adds 40 m of clothoids.
        let expected = 2.0 * 100.0 + 2.0 * (PI * 40.0 + 20.0);
        assert!(
            (track.length() - expected).abs() < 0.01,
            "{}",
            track.length()
        );

        let points = track.points();
        let closing_step = points[0]
            .position
            .distance(points[points.len() - 1].position);
        assert!(
            (closing_step - track.spacing()).abs() < 0.01,
            "{closing_step}"
        );
    }

    /// An oval with one bottleneck: 10 m wide from 40 to 70 m, pinching in over 20 m either side.
    fn pinched_oval() -> TrackDescription {
        let mut description = oval(100.0, 40.0, 20.0);
        description.narrows = vec![Narrows {
            at: 40.0,
            length: 30.0,
            width: 10.0,
            blend: 20.0,
        }];
        description
    }

    #[test]
    fn a_bottleneck_pinches_the_road_in_and_opens_it_out_again() {
        let track = Track::build(&pinched_oval()).unwrap();
        let width = |distance| 2.0 * track.half_width_at(distance);

        assert_eq!(track.half_width(), 8.0, "the road is 16 m wide elsewhere");
        assert!((width(10.0) - 16.0).abs() < 1e-3, "{}", width(10.0));
        for distance in [40.0, 55.0, 70.0] {
            assert!((width(distance) - 10.0).abs() < 0.1, "{}", width(distance));
        }
        // Half way through a blend, the walls are half way in, and they start moving gently.
        assert!((width(30.0) - 13.0).abs() < 0.3, "{}", width(30.0));
        assert!((width(80.0) - 13.0).abs() < 0.3, "{}", width(80.0));
        assert!(width(22.0) > 15.5, "{}", width(22.0));
        assert!((track.narrowest_half_width() - 5.0).abs() < 0.05);
    }

    /// The banking keeps its slope through a bottleneck, so the narrower road rises less.
    #[test]
    fn a_bottleneck_carries_its_banking_across_a_narrower_road() {
        let mut description = banked_oval(100.0, 40.0, 20.0, 30.0);
        description.narrows = vec![Narrows {
            at: first_arc_middle() - 15.0,
            length: 30.0,
            width: 10.0,
            blend: 15.0,
        }];
        let track = Track::build(&description).unwrap();
        let point = track.point_at(first_arc_middle());
        let full = 30f32.to_radians().tan();

        assert!((point.bank.abs() - full).abs() < 1e-3, "{point:?}");
        // The outer edge stands the slope times the half width above the inner one, and the half
        // width here is 5 m rather than 8.
        let outer = track.height_beside(&point, -point.half_width);
        assert!((outer - 5.0 * full).abs() < 1e-2, "{outer}");
    }

    #[test]
    fn a_bottleneck_must_leave_a_way_through_and_fit_on_the_lap() {
        let narrows = |at, length, width, blend| {
            let mut description = oval(100.0, 40.0, 20.0);
            description.narrows = vec![Narrows {
                at,
                length,
                width,
                blend,
            }];
            Track::build(&description)
        };
        assert!(narrows(40.0, 30.0, 10.0, 20.0).is_ok());
        // Wider than the road: a narrows pinches in, it never widens.
        assert!(narrows(40.0, 30.0, 20.0, 20.0).is_err());
        // Narrower than a road can be.
        assert!(narrows(40.0, 30.0, 2.0, 20.0).is_err());
        assert!(narrows(40.0, 0.0, 10.0, 20.0).is_err());
        assert!(narrows(-5.0, 30.0, 10.0, 20.0).is_err());
        assert!(narrows(40.0, 30.0, 10.0, -1.0).is_err());
        // Longer, with its blends, than the whole lap.
        assert!(matches!(
            narrows(40.0, 30.0, 10.0, 400.0),
            Err(TrackError::InvalidNarrows { index: 0, .. })
        ));
    }

    #[test]
    fn projection_finds_distance_and_side() {
        let track = Track::build(&oval(100.0, 40.0, 20.0)).unwrap();
        for distance in [0.0, 30.0, 75.0, 160.0, 300.0, 400.5] {
            let point = track.point_at(distance);
            let left = point.position + point.direction.perp() * 3.0;
            let projection = track.project(left);
            assert!((projection.distance - distance).abs() < 0.05, "{distance}");
            assert!((projection.lateral - 3.0).abs() < 0.05, "{distance}");

            let right = point.position - point.direction.perp() * 3.0;
            assert!(
                (track.project(right).lateral + 3.0).abs() < 0.05,
                "{distance}"
            );
        }
    }

    #[test]
    fn start_line_is_at_the_origin_heading_x() {
        let track = Track::build(&oval(100.0, 40.0, 20.0)).unwrap();
        let start = track.point_at(0.0);
        assert!(start.position.length() < 1e-4);
        assert!(start.direction.distance(Vec2::X) < 1e-4);
    }

    #[test]
    fn turn_really_turns_by_its_angle() {
        for transition in [0.0, 20.0] {
            let track = Track::build(&oval(100.0, 40.0, transition)).unwrap();
            // Half way through the lap, past the first turn, the centerline heads back along `-x`.
            let middle = track.point_at(track.length() / 2.0);
            assert!(middle.direction.distance(-Vec2::X) < 1e-5, "{middle:?}");
            assert!(middle.position.x.abs() < 1e-3, "{middle:?}");
        }

        // Without transitions the turn is a half circle: the far straight is two radii away, on
        // the left.
        let track = Track::build(&oval(100.0, 40.0, 0.0)).unwrap();
        let middle = track.point_at(track.length() / 2.0);
        assert!((middle.position.y - 80.0).abs() < 1e-3, "{middle:?}");
    }

    #[test]
    fn grid_slots_are_behind_the_start_line_on_both_sides() {
        let track = Track::build(&oval(100.0, 40.0, 20.0)).unwrap();
        let (first, heading) = track.grid_slot(0);
        let (second, _) = track.grid_slot(1);
        assert!(first.x < 0.0 && second.x < first.x);
        assert!(first.y > 0.0 && second.y < 0.0);
        assert!(heading.abs() < 1e-4);
    }

    #[test]
    fn angles_must_make_a_loop() {
        let mut description = oval(100.0, 40.0, 20.0);
        description.segments[1] = Segment::Turn {
            angle: 170.0,
            radius: 40.0,
            transition: 20.0,
            banking: 0.0,
        };
        assert!(matches!(
            Track::build(&description),
            Err(TrackError::NotALoop { .. })
        ));
    }

    #[test]
    fn mismatched_straights_leave_a_gap() {
        let mut description = oval(100.0, 40.0, 20.0);
        description.segments[2] = Segment::Straight { length: 90.0 };
        assert!(matches!(
            Track::build(&description),
            Err(TrackError::NotClosed { .. })
        ));
    }

    #[test]
    fn a_road_running_over_itself_is_found() {
        // A hairpin: out along y = 0, back along y = 12, a road 16 m wide.
        let out = (0..50).map(|x| Vec2::new(x as f32, 0.0));
        let turn = (0..19).map(|step| {
            let angle = -std::f32::consts::FRAC_PI_2 + step as f32 / 6.0;
            Vec2::new(50.0, 6.0) + Vec2::from_angle(angle) * 6.0
        });
        let back = (0..50).rev().map(|x| Vec2::new(x as f32, 12.0));
        let positions: Vec<Vec2> = out.chain(turn).chain(back).collect();

        let (at, over) = find_overlap(&positions, 1.0, 16.0).expect("the rows are 12 m apart");
        assert!(at < over, "{at} {over}");
        assert_eq!(find_overlap(&positions, 1.0, 11.0), None);
    }

    #[test]
    fn invalid_segments_are_refused() {
        let invalid = [
            Segment::Straight { length: 0.0 },
            Segment::Turn {
                angle: 180.0,
                radius: 5.0,
                transition: 0.0,
                banking: 0.0,
            },
            Segment::Turn {
                angle: 180.0,
                radius: 40.0,
                transition: 200.0,
                banking: 0.0,
            },
            Segment::Turn {
                angle: 180.0,
                radius: 40.0,
                transition: 20.0,
                banking: 75.0,
            },
            // Banked without a transition: the road would have a step.
            Segment::Turn {
                angle: 180.0,
                radius: 40.0,
                transition: 0.0,
                banking: 20.0,
            },
        ];
        for segment in invalid {
            let mut description = oval(100.0, 40.0, 20.0);
            description.segments[1] = segment;
            assert!(
                matches!(
                    Track::build(&description),
                    Err(TrackError::InvalidSegment { index: 1, .. })
                ),
                "{segment:?}"
            );
        }
    }
}
